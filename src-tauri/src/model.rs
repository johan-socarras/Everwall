//! What the user chose (Settings, settings.json) and what the app did (State,
//! state.json). Both live in %LOCALAPPDATA%\Everwall on Windows and in
//! ~/.local/share/Everwall on Linux, one machine each: monitor ids and folder
//! paths only make sense on the PC that wrote them.

use crate::system;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const ALL_ID: &str = "all";
pub const FAVORITES_ID: &str = "favorites";
pub const SETTINGS_FILE: &str = "settings.json";
pub const STATE_FILE: &str = "state.json";
pub const INDEX_FILE: &str = "index.json";
const LOG_FILE: &str = "everwall.log";
const OLD_LOG_FILE: &str = "everwall.old.log";
/// Names of the collections Everwall makes itself; collection_name shows
/// them in Spanish when the window is.
const ALL_NAME: &str = "All my wallpapers";
pub const NEW_NAME: &str = "New collection";
const FOLDER_PREFIX: &str = "Folder ";
/// Entries kept per screen for "Previous".
pub const HISTORY: usize = 30;

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("Everwall")
}

pub fn cache_dir() -> PathBuf {
    data_dir().join("cache")
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Collection {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default = "yes")]
    pub recursive: bool,
}

fn yes() -> bool {
    true
}

/// How a screen wants its images placed. Auto decides per image.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Auto,
    Fill,
    Blur,
    Triptych,
}

/// What was actually drawn.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    Fill,
    Blur,
    Triptych,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Morning,
    Afternoon,
    Night,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ScreenSettings {
    pub collection: String,
    pub by_time: bool,
    pub morning: String,
    pub afternoon: String,
    pub night: String,
    /// Minutes as text ("5", "30"...), or "daily", "start" (only when the app
    /// starts) or "manual".
    pub every: String,
    pub fit: Fit,
}

impl Default for ScreenSettings {
    fn default() -> Self {
        Self {
            collection: ALL_ID.into(),
            by_time: false,
            morning: ALL_ID.into(),
            afternoon: ALL_ID.into(),
            night: ALL_ID.into(),
            every: "30".into(),
            fit: Fit::Auto,
        }
    }
}

impl ScreenSettings {
    pub fn collection_for(&self, slot: Slot) -> &str {
        if !self.by_time {
            return &self.collection;
        }
        match slot {
            Slot::Morning => &self.morning,
            Slot::Afternoon => &self.afternoon,
            Slot::Night => &self.night,
        }
    }

    pub fn uses(&self, id: &str) -> bool {
        self.collection == id || (self.by_time && (self.morning == id || self.afternoon == id || self.night == id))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub collections: Vec<Collection>,
    pub screens: BTreeMap<String, ScreenSettings>,
    /// Start with the session (the name stays for the settings already saved).
    pub start_with_windows: bool,
    /// Off by default on Linux: GNOME does not say what is full screen, only
    /// that something asked to keep the screen on, and some apps ask all the
    /// time, which would stop every automatic change.
    pub pause_fullscreen: bool,
    pub accent: bool,
    /// Monitor id whose wallpaper sets the accent colour; empty = the main one.
    pub accent_screen: String,
    /// The accent colour changes at most once every this many hours: each
    /// change makes Windows repaint every window for a minute or two.
    pub accent_every: u32,
    pub morning_from: u32,
    pub afternoon_from: u32,
    pub night_from: u32,
    pub paused: bool,
    /// "es" or "en": the window, the tray menu and the messages.
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            collections: Vec::new(),
            screens: BTreeMap::new(),
            start_with_windows: true,
            pause_fullscreen: cfg!(windows),
            accent: true,
            accent_screen: String::new(),
            accent_every: 3,
            morning_from: 6,
            afternoon_from: 12,
            night_from: 19,
            paused: false,
            language: system::system_language().into(),
        }
    }
}

impl Settings {
    pub fn tr<'a>(&self, es: &'a str, en: &'a str) -> &'a str {
        if self.language == "en" {
            en
        } else {
            es
        }
    }

    /// First run: one collection with everything under Pictures/Wallpapers
    /// (or Pictures if there is no such folder) and one per subfolder.
    pub fn with_defaults() -> Self {
        let mut settings = Self::default();
        let pictures = dirs::picture_dir().unwrap_or_else(|| PathBuf::from("."));
        let wallpapers = pictures.join("Wallpapers");
        let root = if wallpapers.is_dir() { wallpapers } else { pictures };
        settings.collections.push(Collection {
            id: ALL_ID.into(),
            name: ALL_NAME.into(),
            folders: vec![root.display().to_string()],
            recursive: true,
        });
        if let Ok(entries) = std::fs::read_dir(&root) {
            let mut subfolders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            subfolders.sort();
            for folder in subfolders.iter().take(20) {
                let name = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with('.') {
                    continue;
                }
                let id = settings.new_collection_id();
                settings.collections.push(Collection {
                    id,
                    name: format!("{FOLDER_PREFIX}{name}"),
                    folders: vec![folder.display().to_string()],
                    recursive: true,
                });
            }
        }
        settings
    }

    pub fn slot(&self, hour: u32) -> Slot {
        if hour >= self.night_from || hour < self.morning_from {
            Slot::Night
        } else if hour >= self.afternoon_from {
            Slot::Afternoon
        } else {
            Slot::Morning
        }
    }

    pub fn current_slot(&self) -> Slot {
        self.slot(system::local_now().hour)
    }

    pub fn collection(&self, id: &str) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }

    /// What the window and the messages call a collection: the names
    /// Everwall gave itself follow the language until the user renames them.
    pub fn collection_name(&self, id: &str) -> String {
        if id == FAVORITES_ID {
            return self.tr("Favoritos", "Favourites").into();
        }
        let Some(c) = self.collection(id) else { return id.to_string() };
        if self.language == "en" {
            return c.name.clone();
        }
        if c.id == ALL_ID && c.name == ALL_NAME {
            return "Todos mis fondos".into();
        }
        if c.name == NEW_NAME {
            return "Colección nueva".into();
        }
        c.name.strip_prefix(FOLDER_PREFIX).map(|rest| format!("Carpeta {rest}")).unwrap_or_else(|| c.name.clone())
    }

    pub fn new_collection_id(&self) -> String {
        (1..).map(|n| format!("c{n}")).find(|id| self.collection(id).is_none()).unwrap_or_default()
    }

    /// Takes a collection out and points every screen that named it at "All
    /// my wallpapers" (or the first collection left), in all four places:
    /// the times of day too while that option is off, so turning it on later
    /// never finds a deleted collection. Returns the screens that used it.
    pub fn forget_collection(&mut self, id: &str) -> Vec<String> {
        self.collections.retain(|c| c.id != id);
        let fallback = self.collection(ALL_ID).or(self.collections.first()).map(|c| c.id.clone()).unwrap_or_default();
        let mut affected = Vec::new();
        for (screen, ss) in self.screens.iter_mut() {
            if ss.uses(id) {
                affected.push(screen.clone());
            }
            for field in [&mut ss.collection, &mut ss.morning, &mut ss.afternoon, &mut ss.night] {
                if *field == id {
                    *field = fallback.clone();
                }
            }
        }
        affected
    }
}

/// One wallpaper that was shown: the source images, how they were laid out
/// and the rendered file (emptied once the cache drops it).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Shown {
    pub sources: Vec<String>,
    pub layout: Layout,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub size: (u32, u32),
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ScreenState {
    pub history: Vec<Shown>,
    pub pos: usize,
    pub last_change: i64,
    /// Local date (yyyymmdd) of the last change, for "once a day".
    pub last_day: u32,
    /// Sources shown lately, oldest first; the choice avoids them.
    pub recent: VecDeque<String>,
}

impl ScreenState {
    pub fn current(&self) -> Option<&Shown> {
        self.history.get(self.pos)
    }

    pub fn push(&mut self, shown: Shown, day: u32) {
        if !self.history.is_empty() {
            self.history.truncate(self.pos + 1);
        }
        for source in &shown.sources {
            self.recent.retain(|r| r != source);
            self.recent.push_back(source.clone());
        }
        while self.recent.len() > 80 {
            self.recent.pop_front();
        }
        self.history.push(shown);
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        self.pos = self.history.len() - 1;
        self.last_change = now();
        self.last_day = day;
    }
}

/// A registry value (on GNOME, a GSettings key) as it was before Everwall
/// changed the accent colour. `kind` None means the value did not exist.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SavedValue {
    pub key: String,
    pub name: String,
    pub kind: Option<u32>,
    pub bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct State {
    pub screens: BTreeMap<String, ScreenState>,
    pub favorites: BTreeSet<String>,
    pub hidden: BTreeSet<String>,
    /// Wallpaper of each screen before Everwall first ran (on GNOME, the
    /// background settings, as GVariant text).
    pub original: BTreeMap<String, String>,
    pub accent_backup: Option<Vec<SavedValue>>,
    /// Accent colour Everwall last wrote, and when (unix seconds).
    pub accent_applied: Option<[u8; 3]>,
    pub accent_at: i64,
    pub slot: Option<Slot>,
}

pub fn load<T: for<'de> Deserialize<'de>>(name: &str) -> Option<T> {
    let text = std::fs::read_to_string(data_dir().join(name)).ok()?;
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            log(&format!("could not read {name} ({error}); using the defaults"));
            None
        }
    }
}

/// Writes through a temporary file so a crash never leaves half a JSON.
pub fn save<T: Serialize>(name: &str, value: &T) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join(format!("{name}.tmp"));
    if let Ok(text) = serde_json::to_string_pretty(value) {
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(name));
        }
    }
}

pub fn log(message: &str) {
    use std::io::Write;
    // several threads log; one line at a time
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(LOG_FILE);
    if std::fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join(OLD_LOG_FILE));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {message}", system::local_now().stamp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(id: &str) -> Collection {
        Collection { id: id.into(), name: format!("{FOLDER_PREFIX}{id}"), folders: Vec::new(), recursive: true }
    }

    fn shown(source: &str) -> Shown {
        Shown { sources: vec![source.into()], layout: Layout::Fill, file: String::new(), size: (1, 1), at: 0 }
    }

    #[test]
    fn each_hour_falls_in_its_time_of_day() {
        let settings = Settings { morning_from: 6, afternoon_from: 12, night_from: 19, ..Settings::default() };
        let slots: Vec<Slot> = [5, 6, 11, 12, 18, 19, 23].into_iter().map(|hour| settings.slot(hour)).collect();
        use Slot::*;
        assert_eq!(slots, [Night, Morning, Morning, Afternoon, Afternoon, Night, Night]);
    }

    #[test]
    fn times_of_day_count_only_while_that_option_is_on() {
        let mut screen = ScreenSettings { collection: ALL_ID.into(), morning: "c1".into(), ..ScreenSettings::default() };
        assert_eq!(screen.collection_for(Slot::Morning), ALL_ID);
        assert!(!screen.uses("c1"));
        screen.by_time = true;
        assert_eq!(screen.collection_for(Slot::Morning), "c1");
        assert!(screen.uses("c1"));
    }

    #[test]
    fn deleting_a_collection_leaves_no_screen_pointing_at_it() {
        let mut settings =
            Settings { collections: vec![collection(ALL_ID), collection("c1"), collection("c2")], ..Settings::default() };
        // c1 as the morning collection of a screen with times of day off,
        // and as the collection of another one
        settings.screens.insert("left".into(), ScreenSettings { morning: "c1".into(), ..ScreenSettings::default() });
        settings.screens.insert("right".into(), ScreenSettings { collection: "c1".into(), ..ScreenSettings::default() });

        assert_eq!(settings.forget_collection("c1"), ["right"]);
        assert!(settings.collection("c1").is_none());
        for screen in settings.screens.values() {
            for field in [&screen.collection, &screen.morning, &screen.afternoon, &screen.night] {
                assert_eq!(field, ALL_ID);
            }
        }
    }

    #[test]
    fn without_all_my_wallpapers_the_first_collection_left_takes_over() {
        let mut settings = Settings { collections: vec![collection("c1"), collection("c2")], ..Settings::default() };
        settings.screens.insert("main".into(), ScreenSettings { collection: "c1".into(), ..ScreenSettings::default() });
        settings.forget_collection("c1");
        assert_eq!(settings.screens["main"].collection, "c2");
    }

    #[test]
    fn a_new_collection_takes_the_first_free_id() {
        let settings =
            Settings { collections: vec![collection(ALL_ID), collection("c1"), collection("c3")], ..Settings::default() };
        assert_eq!(settings.new_collection_id(), "c2");
    }

    #[test]
    fn a_new_wallpaper_after_going_back_drops_the_ones_ahead() {
        let mut screen = ScreenState::default();
        for source in ["a", "b", "c"] {
            screen.push(shown(source), 1);
        }
        screen.pos = 0; // "Previous" twice
        screen.push(shown("d"), 1);
        let sources: Vec<&str> = screen.history.iter().map(|s| s.sources[0].as_str()).collect();
        assert_eq!(sources, ["a", "d"]);
        assert_eq!(screen.pos, 1);
    }

    #[test]
    fn the_history_keeps_only_the_latest_wallpapers() {
        let mut screen = ScreenState::default();
        for n in 0..HISTORY + 5 {
            screen.push(shown(&n.to_string()), 1);
        }
        assert_eq!(screen.history.len(), HISTORY);
        assert_eq!(screen.history[0].sources[0], "5");
        assert_eq!(screen.pos, HISTORY - 1);
    }

    #[test]
    fn names_everwall_gave_itself_follow_the_language() {
        let all = Collection { id: ALL_ID.into(), name: ALL_NAME.into(), folders: Vec::new(), recursive: true };
        let mut settings = Settings { collections: vec![all, collection("Trips")], ..Settings::default() };
        settings.language = "es".into();
        assert_eq!(settings.collection_name(ALL_ID), "Todos mis fondos");
        assert_eq!(settings.collection_name("Trips"), "Carpeta Trips");
        assert_eq!(settings.collection_name(FAVORITES_ID), "Favoritos");
        settings.language = "en".into();
        assert_eq!(settings.collection_name(ALL_ID), ALL_NAME);
        assert_eq!(settings.collection_name("Trips"), "Folder Trips");
    }
}
