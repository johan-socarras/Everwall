//! What the settings window can ask. Reads come straight from the shared
//! state; anything that changes a wallpaper goes to the engine thread.

use crate::engine::{self, Cmd};
use crate::model::{Collection, ScreenSettings, Settings, NEW_NAME};
use crate::{library, render, system, AppCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use tauri::{AppHandle, Manager, State, WebviewWindow};

#[derive(Serialize, Deserialize)]
pub struct General {
    pub start_with_windows: bool,
    pub pause_fullscreen: bool,
    pub accent: bool,
    pub accent_screen: String,
    pub accent_every: u32,
    pub morning_from: u32,
    pub afternoon_from: u32,
    pub night_from: u32,
    pub language: String,
}

impl From<&Settings> for General {
    fn from(s: &Settings) -> Self {
        Self {
            start_with_windows: s.start_with_windows,
            pause_fullscreen: s.pause_fullscreen,
            accent: s.accent,
            accent_screen: s.accent_screen.clone(),
            accent_every: s.accent_every,
            morning_from: s.morning_from,
            afternoon_from: s.afternoon_from,
            night_from: s.night_from,
            language: s.language.clone(),
        }
    }
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

// Built into the program, so a copy installed by copying just the executable
// still carries its license and the licenses of the libraries inside it.
const LICENSE: &str = include_str!("../../LICENSE");
const THIRD_PARTY_LICENSES: &str = include_str!("../../THIRD-PARTY-LICENSES.txt");

/// What the About page shows: the version of the Tauri package and both
/// license texts, as they are.
#[tauri::command]
pub fn about(app: AppHandle) -> Value {
    json!({
        "version": app.package_info().version.to_string(),
        "license": LICENSE,
        "third_party": THIRD_PARTY_LICENSES,
    })
}

#[tauri::command]
pub fn state(app: State<AppCore>) -> Value {
    let mut sh = app.core.lock().unwrap();
    let focus = sh.focus.take();
    let slot = sh.settings.current_slot();
    let screens: Vec<Value> = sh
        .screens
        .iter()
        .map(|s| {
            let settings = sh.settings.screens.get(&s.id).cloned().unwrap_or_default();
            let st = sh.state.screens.get(&s.id);
            let current = st.and_then(|st| st.current()).map(|c| {
                json!({
                    "layout": c.layout,
                    "sources": c.sources.iter().map(|p| json!({
                        "path": p,
                        "name": file_name(p),
                        "favorite": sh.state.favorites.contains(p),
                    })).collect::<Vec<_>>(),
                    "mini": render::data_url(&engine::mini_of(&c.file)),
                })
            });
            json!({
                "id": s.id,
                "name": s.name,
                "x": s.x, "y": s.y, "w": s.w, "h": s.h,
                "primary": s.primary,
                "settings": settings,
                "current": current,
                "back": st.map(|st| st.pos > 0).unwrap_or(false),
            })
        })
        .collect();
    // which shortcuts the system accepted, in order; the window names them
    let hotkeys: Vec<bool> = (0..system::HOTKEYS.len()).map(|i| sh.hotkeys.get(i).copied().unwrap_or(false)).collect();
    json!({
        "screens": screens,
        "collections": sh.settings.collections,
        "general": General::from(&sh.settings),
        "hotkeys": hotkeys,
        "hidden": sh.state.hidden,
        "favorites": sh.state.favorites.len(),
        "paused": sh.settings.paused,
        "error": sh.error,
        "busy": sh.busy,
        "focus": focus,
        "slot": slot,
        "original": !sh.state.original.is_empty(),
        "system": if cfg!(windows) { "windows" } else { "linux" },
    })
}

#[tauri::command]
pub fn save_screen(app: State<AppCore>, id: String, settings: ScreenSettings) {
    let fresh = {
        let mut sh = app.core.lock().unwrap();
        let old = sh.settings.screens.get(&id).cloned().unwrap_or_default();
        let slot = sh.settings.current_slot();
        let fresh = old.collection_for(slot) != settings.collection_for(slot) || old.fit != settings.fit;
        sh.settings.screens.insert(id.clone(), settings);
        sh.save_settings();
        fresh
    };
    app.send(if fresh { Cmd::Fresh(id) } else { Cmd::Changed });
}

#[tauri::command]
pub fn save_general(handle: AppHandle, general: General) {
    let app = handle.state::<AppCore>();
    let accent_changed = {
        let mut sh = app.core.lock().unwrap();
        let s = &mut sh.settings;
        let before = (s.accent, s.accent_screen.clone());
        s.start_with_windows = general.start_with_windows;
        s.pause_fullscreen = general.pause_fullscreen;
        s.accent = general.accent;
        s.accent_screen = general.accent_screen.clone();
        if (1..=24).contains(&general.accent_every) {
            s.accent_every = general.accent_every;
        }
        if general.language == "es" || general.language == "en" {
            s.language = general.language.clone();
        }
        // the three start hours must go in order through the day
        if general.morning_from < general.afternoon_from && general.afternoon_from < general.night_from && general.night_from < 24 {
            s.morning_from = general.morning_from;
            s.afternoon_from = general.afternoon_from;
            s.night_from = general.night_from;
        }
        let changed = before != (s.accent, s.accent_screen.clone());
        sh.save_settings();
        changed
    };
    system::set_startup(general.start_with_windows);
    crate::sync_tray(&handle);
    if accent_changed {
        app.send(Cmd::Accent);
    }
    app.send(Cmd::Changed);
}

#[tauri::command]
pub fn new_collection(app: State<AppCore>) -> Collection {
    let mut sh = app.core.lock().unwrap();
    let collection = Collection {
        id: sh.settings.new_collection_id(),
        name: NEW_NAME.into(),
        folders: Vec::new(),
        recursive: true,
    };
    sh.settings.collections.push(collection.clone());
    sh.save_settings();
    collection
}

#[tauri::command]
pub fn save_collection(app: State<AppCore>, collection: Collection) {
    let affected: Vec<String> = {
        let mut sh = app.core.lock().unwrap();
        let slot = sh.settings.current_slot();
        let contents_changed = match sh.settings.collections.iter_mut().find(|c| c.id == collection.id) {
            Some(existing) => {
                let changed = existing.folders != collection.folders || existing.recursive != collection.recursive;
                *existing = collection.clone();
                changed
            }
            None => {
                sh.settings.collections.push(collection.clone());
                true
            }
        };
        sh.save_settings();
        if !contents_changed {
            return;
        }
        sh.settings
            .screens
            .iter()
            .filter(|(_, ss)| ss.collection_for(slot) == collection.id)
            .map(|(id, _)| id.clone())
            .collect()
    };
    for id in affected {
        app.send(Cmd::Fresh(id));
    }
}

#[tauri::command]
pub fn delete_collection(app: State<AppCore>, id: String) -> Result<(), String> {
    let affected: Vec<String> = {
        let mut sh = app.core.lock().unwrap();
        if sh.settings.collections.len() <= 1 {
            return Err(sh.settings.tr("Tiene que quedar al menos una colección.", "At least one collection has to stay.").into());
        }
        let affected = sh.settings.forget_collection(&id);
        sh.save_settings();
        affected
    };
    for screen in affected {
        app.send(Cmd::Fresh(screen));
    }
    Ok(())
}

#[tauri::command]
pub async fn pick_folder(window: WebviewWindow) -> Option<String> {
    #[cfg(windows)]
    let owner = window.hwnd().ok().map(|h| h.0 as isize);
    #[cfg(not(windows))]
    let owner = None;
    let title = {
        let core = window.state::<AppCore>();
        let sh = core.core.lock().unwrap();
        sh.settings.tr("Elige una carpeta con fondos", "Choose a folder with wallpapers").to_string()
    };
    // The dialog wants a thread of its own (on Windows, with COM set up for it).
    tauri::async_runtime::spawn_blocking(move || std::thread::spawn(move || system::pick_folder(owner, &title)).join().ok().flatten())
        .await
        .ok()
        .flatten()
}

#[tauri::command]
pub async fn count(app: AppHandle, id: String) -> Value {
    let (settings, favorites, hidden, index) = {
        let core = app.state::<AppCore>();
        let sh = core.core.lock().unwrap();
        (sh.settings.clone(), sh.state.favorites.clone(), sh.state.hidden.clone(), core.index.clone())
    };
    tauri::async_runtime::spawn_blocking(move || {
        let images = {
            let mut index = index.lock().unwrap();
            library::images(&settings, &favorites, &hidden, &id, &mut index)
        };
        let portrait = images.iter().filter(|i| i.portrait()).count();
        json!({ "total": images.len(), "portrait": portrait, "landscape": images.len() - portrait })
    })
    .await
    .unwrap_or(Value::Null)
}

#[tauri::command]
pub async fn thumbnail(path: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || render::preview(&path, 260)).await.ok().flatten()
}

#[tauri::command]
pub fn next(app: State<AppCore>, id: Option<String>) {
    app.send(Cmd::Next(id));
}

#[tauri::command]
pub fn previous(app: State<AppCore>, id: String) {
    app.send(Cmd::Prev(id));
}

#[tauri::command]
pub fn favorite(app: State<AppCore>, path: String, on: bool) {
    set_favorite(&app, &path, Some(on));
}

/// `value` None toggles (the hotkey).
pub fn set_favorite(app: &AppCore, path: &str, value: Option<bool>) {
    let mut sh = app.core.lock().unwrap();
    let on = value.unwrap_or(!sh.state.favorites.contains(path));
    if on {
        sh.state.favorites.insert(path.to_string());
    } else {
        sh.state.favorites.remove(path);
    }
    sh.save_state();
}

#[tauri::command]
pub fn hide(app: State<AppCore>, path: String) {
    hide_image(&app, &path);
}

/// Never again: out of every collection, and off the screens showing it.
pub fn hide_image(app: &AppCore, path: &str) {
    let showing = {
        let mut sh = app.core.lock().unwrap();
        sh.state.hidden.insert(path.to_string());
        sh.state.favorites.remove(path);
        sh.save_state();
        sh.showing(path)
    };
    for id in showing {
        app.send(Cmd::Fresh(id));
    }
}

#[tauri::command]
pub fn unhide(app: State<AppCore>, path: String) {
    let mut sh = app.core.lock().unwrap();
    sh.state.hidden.remove(&path);
    sh.save_state();
}

#[tauri::command]
pub fn reveal(path: String) {
    system::reveal(&path);
}

#[tauri::command]
pub fn pause(app: AppHandle, on: bool) {
    {
        let core = app.state::<AppCore>();
        let mut sh = core.core.lock().unwrap();
        sh.settings.paused = on;
        sh.save_settings();
    }
    crate::sync_tray(&app);
    app.state::<AppCore>().send(Cmd::Changed);
}

#[tauri::command]
pub fn restore_original(app: State<AppCore>) {
    app.send(Cmd::RestoreOriginal);
}
