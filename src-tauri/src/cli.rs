//! Two command-line helpers that never touch the wallpaper or the settings:
//!   everwall --diagnostics [file]   what Everwall sees (screens, collections...)
//!   everwall --samples [folder]     every layout rendered for every screen
//! and, on Linux, the one install.sh --uninstall uses:
//!   everwall --uninstall            the wallpaper and accent colour from before
//!                                   Everwall back, its shortcuts and autostart out

use crate::library::{self, DimIndex};
use crate::model::{self, Layout, Settings, State};
use crate::screens::Desktop;
use crate::{render, system};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

fn read_settings() -> (Settings, State) {
    (
        model::load::<Settings>(model::SETTINGS_FILE).unwrap_or_else(Settings::with_defaults),
        model::load::<State>(model::STATE_FILE).unwrap_or_default(),
    )
}

pub fn diagnostics(out: Option<String>) {
    let mut r = String::new();
    let now = system::local_now();
    let _ = writeln!(r, "Everwall {} - diagnostics {}", env!("CARGO_PKG_VERSION"), now.stamp);
    let _ = writeln!(r, "System: {} | language: {}", system::NAME, system::system_language());
    match Desktop::new() {
        Ok(desktop) => {
            for s in desktop.screens() {
                let _ = writeln!(
                    r,
                    "Screen \"{}\" {}x{} at ({},{}) | work area {}x{} from ({},{}) | primary={}\n  id: {}\n  wallpaper: {}",
                    s.name, s.w, s.h, s.x, s.y, s.area.w, s.area.h, s.area.x, s.area.y, s.primary, s.id,
                    desktop.get(&s.id).unwrap_or_default()
                );
            }
        }
        Err(error) => {
            let _ = writeln!(r, "Wallpaper API: ERROR {error}");
        }
    }
    let _ = writeln!(r, "Full screen or busy: {}", crate::screens::busy());
    #[cfg(target_os = "linux")]
    for inhibitor in crate::screens::inhibitors() {
        let _ = writeln!(r, "  keeping the screen on: {inhibitor}");
    }
    let (settings, state) = read_settings();
    let _ = writeln!(r, "Time of day: {:?} (hour {})", settings.slot(now.hour), now.hour);
    let mut index = DimIndex::load();
    for c in &settings.collections {
        let images = library::images(&settings, &state.favorites, &state.hidden, &c.id, &mut index);
        let portrait = images.iter().filter(|i| i.portrait()).count();
        let _ = writeln!(
            r,
            "Collection {} \"{}\": {} images ({} portrait, {} landscape) in {:?}",
            c.id, c.name, images.len(), portrait, images.len() - portrait, c.folders
        );
    }
    let _ = writeln!(r, "Favourites: {} | hidden: {}", state.favorites.len(), state.hidden.len());
    let _ = writeln!(r, "Start with the system: {:?}", system::startup_value());
    let _ = writeln!(r, "Data in {}", model::data_dir().display());
    let path = out.map(PathBuf::from).unwrap_or_else(|| model::data_dir().join("diagnostics.txt"));
    let _ = std::fs::create_dir_all(model::data_dir());
    let _ = std::fs::write(path, r);
}

#[cfg(target_os = "linux")]
pub fn uninstall() {
    let (_, state) = read_settings();
    if !state.original.is_empty() {
        if let Ok(desktop) = Desktop::new() {
            desktop.restore(&state.original, &[]);
        }
    }
    if let Some(backup) = &state.accent_backup {
        crate::accent::restore(backup);
    }
    system::uninstall();
}

pub fn samples(out: Option<String>) {
    let dir = out.map(PathBuf::from).unwrap_or_else(|| model::data_dir().join("samples"));
    let _ = std::fs::create_dir_all(&dir);
    let mut report = String::new();
    let (settings, state) = read_settings();
    let Some(first) = settings.collections.first() else { return };
    let mut index = DimIndex::load();
    let pool = library::images(&settings, &state.favorites, &state.hidden, &first.id, &mut index);
    let portraits: Vec<String> = pool.iter().filter(|i| i.portrait()).map(|i| i.path.clone()).collect();
    let landscapes: Vec<String> = pool.iter().filter(|i| !i.portrait()).map(|i| i.path.clone()).collect();
    let Ok(desktop) = Desktop::new() else { return };
    for (n, s) in desktop.screens().iter().enumerate() {
        let mut jobs: Vec<(Layout, Vec<String>)> = Vec::new();
        if let Some(p) = portraits.first() {
            jobs.push((Layout::Fill, vec![p.clone()]));
            jobs.push((Layout::Blur, vec![p.clone()]));
        }
        if let Some(l) = landscapes.first() {
            jobs.push((Layout::Fill, vec![l.clone()]));
            jobs.push((Layout::Blur, vec![l.clone()]));
        }
        let other = if s.landscape() { &portraits } else { &landscapes };
        if other.len() >= 3 {
            let k = other.len();
            jobs.push((Layout::Triptych, vec![other[0].clone(), other[k / 2].clone(), other[k - 1].clone()]));
        }
        for (k, (layout, sources)) in jobs.iter().enumerate() {
            let images: Vec<_> = sources.iter().filter_map(|p| render::load(p).ok()).collect();
            if images.len() != sources.len() {
                continue;
            }
            let start = Instant::now();
            let image = render::render(*layout, &images, s.w, s.h, s.area);
            let file = dir.join(format!("screen{}-{:?}-{}.jpg", n + 1, layout, k));
            let mini = dir.join(format!("screen{}-{:?}-{}-mini.jpg", n + 1, layout, k));
            let saved = render::save(&image, &file, &mini);
            let _ = writeln!(
                report,
                "{} ({}x{}) {:?}: {} ms {:?}",
                s.name, s.w, s.h, layout, start.elapsed().as_millis(), saved
            );
        }
    }
    let _ = std::fs::write(dir.join("samples.txt"), report);
}
