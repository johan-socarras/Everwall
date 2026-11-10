//! The engine: one thread that owns the wallpaper API and does every change,
//! so two changes never race. It wakes on orders from the window, the tray or
//! the hotkeys, and every 20 seconds to see whether a screen is due.

use crate::library::{self, DimIndex, ImageInfo};
use crate::model::{self, Fit, Layout, ScreenSettings, Settings, Shown, State};
use crate::render;
use crate::screens::{self, Desktop, Screen};
use crate::{accent, system};
use image::RgbImage;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum Cmd {
    /// Forward in that screen's history, or a new wallpaper at its end.
    /// None = every screen.
    Next(Option<String>),
    /// Always a new wallpaper (its settings or its collection changed).
    Fresh(String),
    Prev(String),
    /// Settings changed: look again at what is due.
    Changed,
    Accent,
    RestoreOriginal,
}

/// Everything the window reads; the engine thread writes it.
pub struct Shared {
    pub settings: Settings,
    pub state: State,
    pub screens: Vec<Screen>,
    pub hotkeys: Vec<bool>,
    pub error: Option<String>,
    pub focus: Option<String>,
    pub busy: bool,
}

impl Shared {
    pub fn load() -> Self {
        let settings = model::load::<Settings>(model::SETTINGS_FILE).unwrap_or_else(|| {
            let settings = Settings::with_defaults();
            model::save(model::SETTINGS_FILE, &settings);
            settings
        });
        Self {
            settings,
            state: model::load::<State>(model::STATE_FILE).unwrap_or_default(),
            screens: Vec::new(),
            hotkeys: Vec::new(),
            error: None,
            focus: None,
            busy: false,
        }
    }

    pub fn save_settings(&self) {
        model::save(model::SETTINGS_FILE, &self.settings);
    }

    pub fn save_state(&self) {
        model::save(model::STATE_FILE, &self.state);
    }

    /// Screens whose wallpaper uses this image right now.
    pub fn showing(&self, path: &str) -> Vec<String> {
        self.state
            .screens
            .iter()
            .filter(|(_, st)| st.current().map(|c| c.sources.iter().any(|s| s == path)).unwrap_or(false))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

pub type Core = Arc<Mutex<Shared>>;
pub type Index = Arc<Mutex<DimIndex>>;

pub fn mini_of(file: &str) -> PathBuf {
    PathBuf::from(format!("{}-mini.jpg", file.trim_end_matches(".jpg")))
}

pub fn spawn(core: Core, index: Index, notify: impl Fn() + Send + 'static) -> Sender<Cmd> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let desktop = match Desktop::new() {
            Ok(desktop) => desktop,
            Err(error) => {
                model::log(&format!("could not open the {} wallpaper API: {error}", system::NAME));
                let mut sh = core.lock().unwrap();
                let message = if sh.settings.language == "en" {
                    format!("{} would not let the wallpaper change", system::NAME)
                } else {
                    format!("{} no dejó cambiar el fondo", system::NAME)
                };
                sh.error = Some(format!("{message}: {error}"));
                drop(sh);
                notify();
                return;
            }
        };
        let seed = model::now_millis() as u64 | 1;
        let mut engine = Engine {
            core,
            index,
            desktop,
            notify: Box::new(notify),
            rng: seed,
            broken: BTreeSet::new(),
            last_tick: Instant::now(),
            was_paused: false,
        };
        engine.start();
        engine.run(rx);
    });
    tx
}

struct Engine {
    core: Core,
    index: Index,
    desktop: Desktop,
    notify: Box<dyn Fn() + Send>,
    rng: u64,
    /// Images that failed to open in this session; never picked again.
    broken: BTreeSet<String>,
    last_tick: Instant,
    /// Paused at the last tick: leaving the pause puts the current
    /// wallpapers back on screen at once.
    was_paused: bool,
}

const TICK: Duration = Duration::from_secs(20);

impl Engine {
    fn start(&mut self) {
        self.refresh_screens();
        self.desktop.use_fill();
        let screens = self.screens();
        let paused = {
            let mut sh = self.core.lock().unwrap();
            if sh.state.original.is_empty() {
                sh.state.original = self.desktop.originals(&screens);
                sh.save_state();
            }
            let slot = sh.settings.current_slot();
            sh.state.slot = Some(slot);
            sh.settings.paused
        };
        self.was_paused = paused;
        model::log(&format!("start: {} screens{}", screens.len(), if paused { ", paused" } else { "" }));
        // Paused, the desktop stays as the user left it: after "Restore my
        // previous wallpapers" that is their own wallpaper again.
        if !paused {
            for s in &screens {
                if self.due(&s.id, true) {
                    self.fresh(&s.id);
                } else {
                    self.apply_current(&s.id);
                }
            }
        }
        // The colour Everwall set last time is still in the registry: starting
        // again changes it only if it was never set (or the option is off and
        // there is a backup to give back).
        let (enabled, applied) = {
            let sh = self.core.lock().unwrap();
            (sh.settings.accent, sh.state.accent_applied.is_some())
        };
        if !enabled || !applied {
            self.accent(false);
        }
        self.flush();
        (self.notify)();
    }

    fn run(&mut self, rx: Receiver<Cmd>) {
        loop {
            let wait = TICK.saturating_sub(self.last_tick.elapsed());
            match rx.recv_timeout(wait) {
                Ok(cmd) => {
                    self.handle(cmd);
                    self.flush();
                    (self.notify)();
                }
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if self.last_tick.elapsed() >= TICK {
                self.tick();
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Next(Some(id)) => self.forward(&id),
            Cmd::Next(None) => {
                for s in self.screens() {
                    self.forward(&s.id);
                }
            }
            Cmd::Fresh(id) => self.fresh(&id),
            Cmd::Prev(id) => self.back(&id),
            Cmd::Changed => self.tick(),
            Cmd::Accent => self.accent(true),
            Cmd::RestoreOriginal => self.restore_original(),
        }
    }

    fn tick(&mut self) {
        self.last_tick = Instant::now();
        let paused = self.core.lock().unwrap().settings.paused;
        let resumed = self.was_paused && !paused;
        self.was_paused = paused;
        // A screen that appeared or changed size gets its wallpaper again at
        // its new size, and so does every screen when the pause ends. While
        // paused nothing is put on screen: the user may have their own
        // wallpapers back.
        if self.refresh_screens() || resumed {
            if !paused {
                for s in self.screens() {
                    self.apply_current(&s.id);
                }
                self.flush();
            }
            (self.notify)();
        }
        let (pause_fullscreen, slot, slot_changed) = {
            let sh = self.core.lock().unwrap();
            let slot = sh.settings.current_slot();
            (sh.settings.pause_fullscreen, slot, sh.state.slot != Some(slot))
        };
        if paused || (pause_fullscreen && screens::busy()) {
            return;
        }
        if slot_changed {
            let mut sh = self.core.lock().unwrap();
            sh.state.slot = Some(slot);
            sh.save_state();
        }
        let mut changed = false;
        for s in self.screens() {
            let by_time = self.core.lock().unwrap().settings.screens.get(&s.id).map(|ss| ss.by_time).unwrap_or(false);
            if (slot_changed && by_time) || self.due(&s.id, false) {
                self.fresh(&s.id);
                changed = true;
            }
        }
        if changed {
            self.flush();
            (self.notify)();
        }
    }

    fn screens(&self) -> Vec<Screen> {
        self.core.lock().unwrap().screens.clone()
    }

    fn screen(&self, id: &str) -> Option<Screen> {
        self.core.lock().unwrap().screens.iter().find(|s| s.id == id).cloned()
    }

    /// Reads the monitors again; true if anything moved, appeared or resized.
    fn refresh_screens(&mut self) -> bool {
        let list = self.desktop.screens();
        let mut sh = self.core.lock().unwrap();
        let changed = sh.screens.len() != list.len()
            || sh.screens.iter().zip(&list).any(|(a, b)| {
                a.id != b.id || a.w != b.w || a.h != b.h || a.x != b.x || a.y != b.y || a.area != b.area
            });
        let mut added = false;
        for s in &list {
            if !sh.settings.screens.contains_key(&s.id) {
                sh.settings.screens.insert(s.id.clone(), ScreenSettings::default());
                added = true;
            }
        }
        if added {
            sh.save_settings();
        }
        sh.screens = list;
        changed
    }

    fn due(&self, id: &str, starting: bool) -> bool {
        let sh = self.core.lock().unwrap();
        let every = sh.settings.screens.get(id).map(|s| s.every.clone()).unwrap_or_default();
        let Some(st) = sh.state.screens.get(id) else { return true };
        if st.current().is_none() {
            return true;
        }
        match every.as_str() {
            "manual" => false,
            "start" => starting,
            "daily" => st.last_day != system::local_now().day,
            minutes => minutes.parse::<i64>().map(|m| model::now() - st.last_change >= m * 60).unwrap_or(false),
        }
    }

    fn forward(&mut self, id: &str) {
        let moved = {
            let mut sh = self.core.lock().unwrap();
            match sh.state.screens.get_mut(id) {
                Some(st) if st.pos + 1 < st.history.len() => {
                    st.pos += 1;
                    st.last_change = model::now();
                    true
                }
                _ => false,
            }
        };
        if moved {
            self.apply_current(id);
        } else {
            self.fresh(id);
        }
    }

    fn back(&mut self, id: &str) {
        let moved = {
            let mut sh = self.core.lock().unwrap();
            match sh.state.screens.get_mut(id) {
                Some(st) if st.pos > 0 && !st.history.is_empty() => {
                    st.pos -= 1;
                    st.last_change = model::now();
                    true
                }
                _ => false,
            }
        };
        if moved {
            self.apply_current(id);
        }
    }

    /// A new wallpaper for this screen from its collection.
    fn fresh(&mut self, id: &str) {
        let Some(screen) = self.screen(id) else { return };
        for _attempt in 0..3 {
            let (settings, favorites, hidden, recent, in_use) = {
                let sh = self.core.lock().unwrap();
                let recent: BTreeSet<String> =
                    sh.state.screens.get(id).map(|s| s.recent.iter().cloned().collect()).unwrap_or_default();
                let in_use: BTreeSet<String> = sh
                    .state
                    .screens
                    .iter()
                    .filter(|(k, _)| k.as_str() != id)
                    .filter_map(|(_, s)| s.current())
                    .flat_map(|c| c.sources.iter().cloned())
                    .collect();
                (sh.settings.clone(), sh.state.favorites.clone(), sh.state.hidden.clone(), recent, in_use)
            };
            let screen_settings = settings.screens.get(id).cloned().unwrap_or_default();
            let collection = screen_settings.collection_for(settings.current_slot()).to_string();
            let pool: Vec<ImageInfo> = {
                let mut index = self.index.lock().unwrap();
                library::images(&settings, &favorites, &hidden, &collection, &mut index)
            };
            let pool: Vec<ImageInfo> = pool.into_iter().filter(|i| !self.broken.contains(&i.path)).collect();
            if pool.is_empty() {
                let name = settings.collection_name(&collection);
                let message = if settings.language == "en" {
                    format!("The collection “{name}” has no images that can be opened.")
                } else {
                    format!("La colección «{name}» no tiene imágenes que se puedan abrir.")
                };
                self.fail(&message);
                return;
            }
            let picks = Picks { favorites: &favorites, recent: &recent, in_use: &in_use };
            let (layout, sources) = self.choose(&screen, &screen_settings, &pool, &picks);
            if self.show(&screen, layout, sources) {
                return;
            }
        }
    }

    /// A triptych is three images of the other shape: portraits side by side
    /// on a wide screen, landscapes stacked on a tall one.
    fn choose(&mut self, screen: &Screen, ss: &ScreenSettings, pool: &[ImageInfo], picks: &Picks) -> (Layout, Vec<String>) {
        let all: Vec<&ImageInfo> = pool.iter().collect();
        let matching: Vec<&ImageInfo> = pool.iter().filter(|i| i.portrait() != screen.landscape()).collect();
        let opposite: Vec<&ImageInfo> = pool.iter().filter(|i| i.portrait() == screen.landscape()).collect();
        let screen_ratio = screen.w as f64 / screen.h as f64;
        let fits = |i: &ImageInfo| crop_loss(i.ratio(), screen_ratio) <= 0.30;
        match ss.fit {
            Fit::Triptych if opposite.len() >= 3 => {
                let middle = self.pick(&opposite, picks, &BTreeSet::new()).clone();
                (Layout::Triptych, self.triptych(&middle, &opposite, picks))
            }
            Fit::Fill => {
                let from = if matching.is_empty() { &all } else { &matching };
                (Layout::Fill, vec![self.pick(from, picks, &BTreeSet::new()).path.clone()])
            }
            Fit::Blur => (Layout::Blur, vec![self.pick(&all, picks, &BTreeSet::new()).path.clone()]),
            _ => {
                let chosen = self.pick(&all, picks, &BTreeSet::new()).clone();
                if chosen.portrait() == screen.landscape() {
                    if opposite.len() >= 3 {
                        (Layout::Triptych, self.triptych(&chosen, &opposite, picks))
                    } else {
                        (Layout::Blur, vec![chosen.path])
                    }
                } else if fits(&chosen) {
                    (Layout::Fill, vec![chosen.path])
                } else {
                    (Layout::Blur, vec![chosen.path])
                }
            }
        }
    }

    /// Two more images of the same shape around `middle`, from its own folder
    /// when it has enough (a folder is usually one theme, so the three go
    /// together).
    fn triptych(&mut self, middle: &ImageInfo, shaped: &[&ImageInfo], picks: &Picks) -> Vec<String> {
        let mut skip: BTreeSet<String> = BTreeSet::new();
        skip.insert(middle.path.clone());
        let same: Vec<&ImageInfo> = shaped.iter().filter(|i| i.folder == middle.folder).copied().collect();
        let mut sides = Vec::new();
        for _ in 0..2 {
            let from = if same.iter().any(|i| !skip.contains(&i.path)) { &same } else { shaped };
            let side = self.pick(from, picks, &skip).path.clone();
            skip.insert(side.clone());
            sides.push(side);
        }
        vec![sides[0].clone(), middle.path.clone(), sides[1].clone()]
    }

    /// Weighted draw (favourites count three times), avoiding first what was
    /// shown lately and what other screens show now, and relaxing that only
    /// when nothing else is left.
    fn pick<'a>(&mut self, from: &[&'a ImageInfo], picks: &Picks, skip: &BTreeSet<String>) -> &'a ImageInfo {
        for level in 0..4 {
            let candidates: Vec<&'a ImageInfo> = from
                .iter()
                .copied()
                .filter(|i| match level {
                    0 => !skip.contains(&i.path) && !picks.recent.contains(&i.path) && !picks.in_use.contains(&i.path),
                    1 => !skip.contains(&i.path) && !picks.in_use.contains(&i.path),
                    2 => !skip.contains(&i.path),
                    _ => true,
                })
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let weight = |i: &ImageInfo| if picks.favorites.contains(&i.path) { 3u64 } else { 1 };
            let total: u64 = candidates.iter().map(|i| weight(i)).sum();
            let mut roll = self.random() % total;
            for candidate in &candidates {
                let w = weight(candidate);
                if roll < w {
                    return candidate;
                }
                roll -= w;
            }
            return candidates[0];
        }
        from[0]
    }

    fn random(&mut self) -> u64 {
        // xorshift64: plenty for choosing wallpapers, and no extra crate
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Renders and sets a new wallpaper; false if a source would not open.
    fn show(&mut self, screen: &Screen, layout: Layout, sources: Vec<String>) -> bool {
        self.set_busy(true);
        let result = self.render_to_file(screen, layout, &sources);
        self.set_busy(false);
        let (file, image) = match result {
            Ok(done) => done,
            Err(error) => {
                model::log(&format!("could not render for {}: {error}", screen.name));
                return false;
            }
        };
        if let Err(error) = self.desktop.set(&screen.id, &file) {
            self.fail(&self.rejected(&screen.name, &error));
            return true;
        }
        let day = system::local_now().day;
        {
            let mut sh = self.core.lock().unwrap();
            let shown = Shown { sources: sources.clone(), layout, file, size: (screen.w, screen.h), at: model::now() };
            sh.state.screens.entry(screen.id.clone()).or_default().push(shown, day);
            sh.error = None;
            sh.save_state();
        }
        model::log(&format!("{}: {:?} with {}", screen.name, layout, names(&sources)));
        self.cleanup();
        if self.accent_screen().as_deref() == Some(screen.id.as_str()) {
            self.accent_from(&image, false);
        }
        true
    }

    fn render_to_file(&mut self, screen: &Screen, layout: Layout, sources: &[String]) -> Result<(String, RgbImage), String> {
        let mut images = Vec::with_capacity(sources.len());
        for source in sources {
            match render::load(source) {
                Ok(image) => images.push(image),
                Err(error) => {
                    self.broken.insert(source.clone());
                    return Err(format!("{source}: {error}"));
                }
            }
        }
        let out = render::render(layout, &images, screen.w, screen.h, screen.area);
        let dir = model::cache_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let base = format!("{}-{}", short_id(&screen.id), model::now_millis());
        let file = dir.join(format!("{base}.jpg"));
        render::save(&out, &file, &mini_of(&file.display().to_string()))?;
        Ok((file.display().to_string(), out))
    }

    /// Puts the screen's current history entry on screen, rendering it again
    /// if the screen changed size or the cache already dropped the file.
    fn apply_current(&mut self, id: &str) {
        let Some(screen) = self.screen(id) else { return };
        let entry = {
            let sh = self.core.lock().unwrap();
            sh.state.screens.get(id).and_then(|s| s.current().cloned())
        };
        let Some(entry) = entry else {
            self.fresh(id);
            return;
        };
        if Path::new(&entry.file).is_file() && entry.size == (screen.w, screen.h) {
            let already = self.desktop.get(id).map(|p| p.eq_ignore_ascii_case(&entry.file)).unwrap_or(false);
            if !already {
                if let Err(error) = self.desktop.set(id, &entry.file) {
                    self.fail(&self.rejected(&screen.name, &error));
                    return;
                }
                if self.accent_screen().as_deref() == Some(id) {
                    if let Ok(image) = render::load(&entry.file) {
                        self.accent_from(&image, false);
                    }
                }
            }
            return;
        }
        if !entry.sources.iter().all(|s| Path::new(s).is_file()) {
            self.fresh(id);
            return;
        }
        self.set_busy(true);
        let result = self.render_to_file(&screen, entry.layout, &entry.sources);
        self.set_busy(false);
        match result {
            Ok((file, image)) => {
                if let Err(error) = self.desktop.set(id, &file) {
                    self.fail(&self.rejected(&screen.name, &error));
                    return;
                }
                {
                    let mut sh = self.core.lock().unwrap();
                    if let Some(st) = sh.state.screens.get_mut(id) {
                        let pos = st.pos;
                        if let Some(current) = st.history.get_mut(pos) {
                            current.file = file;
                            current.size = (screen.w, screen.h);
                        }
                    }
                    sh.save_state();
                }
                self.cleanup();
                if self.accent_screen().as_deref() == Some(id) {
                    self.accent_from(&image, false);
                }
            }
            Err(_) => self.fresh(id),
        }
    }

    /// Keeps the rendered files of the last 8 entries of each screen (and the
    /// current one); older entries keep their sources and render again if
    /// someone goes back that far.
    fn cleanup(&mut self) {
        let keep: BTreeSet<String> = {
            let mut sh = self.core.lock().unwrap();
            let mut keep = BTreeSet::new();
            for st in sh.state.screens.values_mut() {
                let (len, pos) = (st.history.len(), st.pos);
                for (i, entry) in st.history.iter_mut().enumerate() {
                    if i + 8 < len && i != pos {
                        entry.file.clear();
                    } else if !entry.file.is_empty() {
                        keep.insert(entry.file.to_lowercase());
                        keep.insert(mini_of(&entry.file).display().to_string().to_lowercase());
                    }
                }
            }
            sh.save_state();
            keep
        };
        let Ok(entries) = std::fs::read_dir(model::cache_dir()) else { return };
        for entry in entries.flatten() {
            let path = entry.path().display().to_string();
            if !keep.contains(&path.to_lowercase()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// The screen that sets the accent colour, if the option is on.
    fn accent_screen(&self) -> Option<String> {
        let sh = self.core.lock().unwrap();
        if !sh.settings.accent {
            return None;
        }
        let chosen = &sh.settings.accent_screen;
        sh.screens
            .iter()
            .find(|s| !chosen.is_empty() && &s.id == chosen)
            .or_else(|| sh.screens.iter().find(|s| s.primary))
            .or(sh.screens.first())
            .map(|s| s.id.clone())
    }

    /// Option switched or screen changed (`force`): apply from the current
    /// wallpaper now, or put back the colours the user had.
    fn accent(&mut self, force: bool) {
        let Some(id) = self.accent_screen() else {
            let backup = self.core.lock().unwrap().state.accent_backup.take();
            if let Some(backup) = backup {
                accent::restore(&backup);
                let mut sh = self.core.lock().unwrap();
                sh.state.accent_applied = None;
                sh.state.accent_at = 0;
                sh.save_state();
                model::log("accent colour: back to the previous one");
            }
            return;
        };
        let file = {
            let sh = self.core.lock().unwrap();
            sh.state.screens.get(&id).and_then(|s| s.current()).map(|c| c.file.clone())
        };
        if let Some(Ok(image)) = file.filter(|f| Path::new(f).is_file()).map(|f| render::load(&f)) {
            self.accent_from(&image, force);
        }
    }

    /// Every accent change makes Windows repaint every window: measured on
    /// a three-monitor PC, Explorer stays at 40-90 % of a core for 1.5 to
    /// 2 minutes after each one, and the PC stutters. So the colour changes at
    /// most once every `accent_every` hours (unless the user just switched the
    /// option), and never for a colour close to the one already set. Both are
    /// kept in state.json, so restarting the app does not change it either.
    fn accent_from(&mut self, image: &RgbImage, force: bool) {
        let colour = accent::dominant(image);
        {
            let mut sh = self.core.lock().unwrap();
            let hours = sh.settings.accent_every.max(1) as i64;
            if !force && model::now() - sh.state.accent_at < hours * 3600 {
                return;
            }
            if sh.state.accent_applied.map(|old| close(old, colour)).unwrap_or(false) {
                return;
            }
            if sh.state.accent_backup.is_none() {
                sh.state.accent_backup = Some(accent::backup());
                sh.save_state();
            }
        }
        match accent::apply(colour) {
            Ok(c) => {
                let mut sh = self.core.lock().unwrap();
                sh.state.accent_applied = Some(colour);
                sh.state.accent_at = model::now();
                sh.save_state();
                model::log(&format!("accent colour #{:02X}{:02X}{:02X}", c[0], c[1], c[2]));
            }
            Err(error) => model::log(&format!("accent colour: {error}")),
        }
    }

    /// Back to the wallpaper and accent colour the user had before Everwall, and
    /// pause, or the next tick would change it again.
    fn restore_original(&mut self) {
        let original = self.core.lock().unwrap().state.original.clone();
        self.desktop.restore(&original, &self.screens());
        {
            let mut sh = self.core.lock().unwrap();
            sh.settings.paused = true;
            sh.settings.accent = false;
            sh.save_settings();
        }
        self.accent(true);
        model::log("back to the previous wallpaper; Everwall is paused");
    }

    fn set_busy(&self, busy: bool) {
        self.core.lock().unwrap().busy = busy;
        (self.notify)();
    }

    /// Where the system takes the whole desktop at once (GNOME), the
    /// wallpapers set since the last call go on screen together now.
    fn flush(&mut self) {
        if let Err(error) = self.desktop.flush(&self.screens()) {
            let message = if self.core.lock().unwrap().settings.language == "en" {
                format!("{} did not accept the wallpaper: {error}", system::NAME)
            } else {
                format!("{} no aceptó el fondo: {error}", system::NAME)
            };
            self.fail(&message);
        }
    }

    fn rejected(&self, screen: &str, error: &str) -> String {
        if self.core.lock().unwrap().settings.language == "en" {
            format!("{} did not accept the wallpaper for {screen}: {error}", system::NAME)
        } else {
            format!("{} no aceptó el fondo de {screen}: {error}", system::NAME)
        }
    }

    fn fail(&self, message: &str) {
        model::log(message);
        self.core.lock().unwrap().error = Some(message.to_string());
        (self.notify)();
    }
}

struct Picks<'a> {
    favorites: &'a BTreeSet<String>,
    recent: &'a BTreeSet<String>,
    in_use: &'a BTreeSet<String>,
}

/// Two accent candidates that would look alike (RGB distance under 48).
fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    let d: i32 = (0..3).map(|i| (a[i] as i32 - b[i] as i32).pow(2)).sum();
    d < 48 * 48
}

/// Share of the image lost when cropping it to the screen's shape.
fn crop_loss(image_ratio: f64, screen_ratio: f64) -> f64 {
    1.0 - (image_ratio / screen_ratio).min(screen_ratio / image_ratio)
}

fn short_id(id: &str) -> String {
    // FNV-1a: a stable, short file prefix per monitor
    let mut hash: u32 = 0x811c9dc5;
    for b in id.to_lowercase().bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{hash:08x}")
}

fn names(sources: &[String]) -> String {
    sources
        .iter()
        .map(|s| Path::new(s).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropping_decides_between_full_screen_and_a_blurred_copy() {
        let wide = 16.0 / 9.0;
        assert!(crop_loss(wide, wide).abs() < 1e-9);
        // a 3:2 photo on a 16:9 screen loses 15.6 %: it fills the screen
        assert!(crop_loss(1.5, wide) <= 0.30);
        // a square one would lose 43.75 %: whole, over a blurred copy
        assert!(crop_loss(1.0, wide) > 0.30);
        // the same loss whichever shape is the wider one
        assert!((crop_loss(9.0 / 16.0, 0.5) - crop_loss(0.5, 9.0 / 16.0)).abs() < 1e-12);
    }

    #[test]
    fn accent_colours_that_look_alike_count_as_close() {
        assert!(close([200, 40, 40], [210, 50, 45]));
        assert!(!close([200, 40, 40], [40, 40, 200]));
    }

    #[test]
    fn the_file_prefix_of_a_monitor_is_stable_and_short() {
        assert_eq!(short_id(r"\\?\DISPLAY#ABC"), short_id(r"\\?\display#abc"));
        assert_ne!(short_id("left"), short_id("right"));
        assert_eq!(short_id("left").len(), 8);
    }
}
