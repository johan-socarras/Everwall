#![windows_subsystem = "windows"]
//! Everwall: a different wallpaper on each screen, chosen from collections of
//! folders, placed to suit each screen's shape. Lives in the tray; the window
//! only exists while it is open.

mod accent;
mod cli;
mod commands;
mod engine;
#[cfg(target_os = "linux")]
mod gnome;
mod library;
mod model;
mod render;
mod screens;
mod system;

use engine::{Cmd, Shared};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, Wry};

pub struct AppCore {
    pub core: engine::Core,
    pub index: engine::Index,
    tx: Mutex<Sender<Cmd>>,
}

impl AppCore {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.lock().unwrap().send(cmd);
    }
}

struct TrayMenu {
    open: MenuItem<Wry>,
    next: MenuItem<Wry>,
    pause: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
}

/// (open, next, pause, quit) in the chosen language.
fn tray_labels(english: bool) -> [&'static str; 4] {
    if english {
        ["Open Everwall", "Next wallpaper on all screens", "Pause automatic changes", "Quit"]
    } else {
        ["Abrir Everwall", "Siguiente fondo en todas", "Pausar cambios automáticos", "Salir"]
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value_of = |flag: &str| args.iter().position(|a| a == flag).map(|i| args.get(i + 1).cloned());
    if let Some(out) = value_of("--diagnostics") {
        cli::diagnostics(out);
        return;
    }
    if let Some(out) = value_of("--samples") {
        cli::samples(out);
        return;
    }
    #[cfg(target_os = "linux")]
    if has("--uninstall") {
        cli::uninstall();
        return;
    }
    // --shortcut N comes from a GNOME custom shortcut (see system::start_hotkeys).
    let shortcut = value_of("--shortcut").flatten();
    let order = if let Some(key) = shortcut.and_then(|n| n.parse::<i32>().ok()) {
        format!("shortcut {key}")
    } else if has("--next") {
        "next".to_string()
    } else if has("--tray") {
        String::new()
    } else {
        "open".to_string()
    };
    if !system::claim_single_instance() {
        if !order.is_empty() {
            system::send_order(&order);
        }
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::state,
            commands::save_screen,
            commands::save_general,
            commands::new_collection,
            commands::save_collection,
            commands::delete_collection,
            commands::pick_folder,
            commands::count,
            commands::thumbnail,
            commands::next,
            commands::previous,
            commands::favorite,
            commands::hide,
            commands::unhide,
            commands::reveal,
            commands::pause,
            commands::restore_original,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let core = Arc::new(Mutex::new(Shared::load()));
            let index = Arc::new(Mutex::new(library::DimIndex::load()));
            let notify_handle = handle.clone();
            let tx = engine::spawn(core.clone(), index.clone(), move || {
                let _ = notify_handle.emit("changed", ());
                sync_tray(&notify_handle);
            });
            app.manage(AppCore { core: core.clone(), index, tx: Mutex::new(tx) });
            build_tray(app)?;

            let hotkey_handle = handle.clone();
            let english = core.lock().unwrap().settings.language == "en";
            let hotkeys = system::start_hotkeys(english, move |key| on_hotkey(&hotkey_handle, key));
            model::log(&format!("shortcuts registered: {hotkeys:?}"));
            let start_with_windows = {
                let mut sh = core.lock().unwrap();
                sh.hotkeys = hotkeys;
                sh.settings.start_with_windows
            };
            if start_with_windows {
                system::set_startup(true);
            }
            let order_handle = handle.clone();
            system::listen_orders(move |order| run_order(&order_handle, &order));
            match order.as_str() {
                "open" => open_settings(&handle, None),
                "" => {}
                other => run_order(&handle, other),
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Everwall could not start")
        .run(|_app, event| {
            // Closing the window must not end the app: it lives in the tray.
            // Only "Quit" (an explicit exit code) gets through.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let (paused, english) = {
        let core = app.state::<AppCore>();
        let sh = core.core.lock().unwrap();
        (sh.settings.paused, sh.settings.language == "en")
    };
    let [open_text, next_text, pause_text, quit_text] = tray_labels(english);
    let open = MenuItem::with_id(app, "open", open_text, true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", next_text, true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", pause_text, true, paused, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_text, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &next, &pause, &separator, &quit])?;
    app.manage(TrayMenu { open, next, pause, quit });
    let mut tray = TrayIconBuilder::with_id("everwall")
        .tooltip(tooltip(paused, english))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => open_settings(app, None),
            "next" => app.state::<AppCore>().send(Cmd::Next(None)),
            "pause" => {
                // the menu already flipped its own tick; follow it
                let on = app.state::<TrayMenu>().pause.is_checked().unwrap_or(false);
                commands::pause(app.clone(), on);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                open_settings(tray.app_handle(), None);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn tooltip(paused: bool, english: bool) -> &'static str {
    match (paused, english) {
        (false, _) => "Everwall",
        (true, false) => "Everwall (en pausa)",
        (true, true) => "Everwall (paused)",
    }
}

/// Tray tick, texts and tooltip follow the settings, whoever changed them.
pub fn sync_tray(app: &AppHandle) {
    let Some(core) = app.try_state::<AppCore>() else { return };
    let (paused, english) = {
        let sh = core.core.lock().unwrap();
        (sh.settings.paused, sh.settings.language == "en")
    };
    if let Some(menu) = app.try_state::<TrayMenu>() {
        let [open, next, pause, quit] = tray_labels(english);
        let _ = menu.open.set_text(open);
        let _ = menu.next.set_text(next);
        let _ = menu.pause.set_text(pause);
        let _ = menu.quit.set_text(quit);
        let _ = menu.pause.set_checked(paused);
    }
    if let Some(tray) = app.tray_by_id("everwall") {
        let _ = tray.set_tooltip(Some(tooltip(paused, english)));
    }
}

pub fn open_settings(app: &AppHandle, focus: Option<String>) {
    if let (Some(core), Some(id)) = (app.try_state::<AppCore>(), focus.clone()) {
        core.core.lock().unwrap().focus = Some(id);
    }
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app.emit("focus", focus);
        return;
    }
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
        .title("Everwall")
        .inner_size(1080.0, 780.0)
        .min_inner_size(820.0, 600.0)
        .center()
        .build();
    if let Err(error) = built {
        model::log(&format!("could not open the window: {error}"));
    }
}

/// An order from a second launch: "next", "shortcut N" or "open".
fn run_order(app: &AppHandle, order: &str) {
    if order == "next" {
        app.state::<AppCore>().send(Cmd::Next(None));
    } else if let Some(key) = order.strip_prefix("shortcut ").and_then(|n| n.parse().ok()) {
        on_hotkey(app, key);
    } else {
        open_from_thread(app, None);
    }
}

fn open_from_thread(app: &AppHandle, focus: Option<String>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || open_settings(&handle, focus));
}

fn on_hotkey(app: &AppHandle, key: i32) {
    let Some(core) = app.try_state::<AppCore>() else { return };
    // the screen under the mouse; where nobody can tell (Wayland), the main one
    let cursor = screens::cursor();
    let target = {
        let sh = core.core.lock().unwrap();
        let under = cursor.and_then(|(x, y)| sh.screens.iter().find(|s| s.contains(x, y)));
        under.or(sh.screens.iter().find(|s| s.primary)).or(sh.screens.first()).map(|s| {
            let single = sh
                .state
                .screens
                .get(&s.id)
                .and_then(|st| st.current())
                .filter(|c| c.sources.len() == 1)
                .map(|c| c.sources[0].clone());
            (s.id.clone(), single)
        })
    };
    let Some((id, single)) = target else { return };
    match (key, single) {
        (1, _) => core.send(Cmd::Next(Some(id))),
        (2, _) => core.send(Cmd::Prev(id)),
        (3, Some(path)) => {
            commands::set_favorite(&core, &path, None);
            let _ = app.emit("changed", ());
        }
        (4, Some(path)) => commands::hide_image(&core, &path),
        // a triptych: which of the three? The user picks in the window
        (3 | 4, None) => open_from_thread(app, Some(id)),
        _ => {}
    }
}
