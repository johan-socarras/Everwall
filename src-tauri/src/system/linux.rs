//! Linux (GNOME): local time, the session language, starting at log-in (an
//! XDG autostart entry), one instance only (a Unix socket: a second launch
//! passes its order through it), the keyboard shortcuts (GNOME custom
//! shortcuts that run `everwall --shortcut N`), the folder picker (zenity) and
//! showing a file in the file manager.

use super::{LocalTime, HOTKEYS};
use crate::{gnome, model};
use gio::glib::{self, ToVariant, Variant};
use gio::prelude::*;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn local_now() -> LocalTime {
    let Ok(t) = glib::DateTime::now_local() else {
        return LocalTime { day: 0, hour: 0, stamp: String::new() };
    };
    LocalTime {
        day: t.year() as u32 * 10000 + t.month() as u32 * 100 + t.day_of_month() as u32,
        hour: t.hour() as u32,
        stamp: format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.year(),
            t.month(),
            t.day_of_month(),
            t.hour(),
            t.minute(),
            t.second()
        ),
    }
}

/// "es" when the session's messages are in Spanish, "en" otherwise: the
/// language the app starts in until the user picks one.
pub fn system_language() -> &'static str {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    if locale.starts_with("es") {
        "es"
    } else {
        "en"
    }
}

/// A build straight out of a cargo target folder (it has `.fingerprint`
/// beside it) must not end up starting with the session or behind a shortcut.
fn from_cargo(exe: &Path) -> bool {
    exe.parent().map(|dir| dir.join(".fingerprint").is_dir()).unwrap_or(false)
}

fn autostart_file() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("autostart").join("everwall.desktop"))
}

/// Points the autostart entry at this very program, wherever it was
/// unpacked or installed; starting a moved copy points it at the new place.
pub fn set_startup(enable: bool) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(file) = autostart_file() else { return };
    if from_cargo(&exe) {
        model::log(&format!("start with the session: {} is a cargo build, leaving autostart alone", exe.display()));
        return;
    }
    if !enable {
        let _ = std::fs::remove_file(file);
        return;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Everwall\nComment=A different wallpaper on each screen\n\
         Exec=\"{}\" --tray\nIcon=everwall\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if std::fs::read_to_string(&file).ok().as_deref() != Some(entry.as_str()) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, entry);
    }
}

pub fn startup_value() -> Option<String> {
    let text = std::fs::read_to_string(autostart_file()?).ok()?;
    text.lines().find_map(|line| line.strip_prefix("Exec=")).map(str::to_string)
}

fn socket_path() -> PathBuf {
    dirs::runtime_dir().unwrap_or_else(model::data_dir).join("everwall.sock")
}

static LISTENER: OnceLock<UnixListener> = OnceLock::new();

/// True for the first Everwall running in this session: the one that owns
/// the socket. A socket left by a crash answers nobody and is replaced.
pub fn claim_single_instance() -> bool {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return false;
    }
    let _ = std::fs::remove_file(&path);
    if let Ok(listener) = UnixListener::bind(&path) {
        let _ = LISTENER.set(listener);
    }
    true
}

/// From a second launch: hand the order to the running instance.
pub fn send_order(order: &str) {
    if let Ok(mut stream) = UnixStream::connect(socket_path()) {
        let _ = stream.write_all(order.as_bytes());
    }
}

pub fn listen_orders(on_order: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        let Some(listener) = LISTENER.get() else { return };
        for stream in listener.incoming().flatten() {
            let mut text = String::new();
            let _ = stream.take(256).read_to_string(&mut text);
            on_order(text.trim().to_string());
        }
    });
}

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_KEY: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const CUSTOM_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
/// Super+Ctrl+Alt + →, ←, ↑, ↓, in the order of HOTKEYS: GNOME already uses
/// Ctrl+Alt+Shift + arrows to move windows between workspaces.
pub const KEYS: [(&str, &str, &str, &str); 4] = [
    ("everwall-next", "Right", "Everwall: siguiente fondo", "Everwall: next wallpaper"),
    ("everwall-back", "Left", "Everwall: fondo anterior", "Everwall: previous wallpaper"),
    ("everwall-favourite", "Up", "Everwall: favorito", "Everwall: favourite"),
    ("everwall-hide", "Down", "Everwall: no volver a mostrarlo", "Everwall: never show it again"),
];
const MODIFIERS: &str = "<Super><Control><Alt>";

/// A Wayland program cannot grab keys for itself, so the shortcuts are GNOME
/// custom shortcuts (the ones in Settings > Keyboard) that run this program
/// with `--shortcut N`; the running instance gets it as an order. Returns which
/// ones no other GNOME shortcut already uses.
pub fn start_hotkeys(english: bool, _on_key: impl Fn(i32) + Send + 'static) -> Vec<bool> {
    let Ok(exe) = std::env::current_exe() else { return vec![false; HOTKEYS.len()] };
    if from_cargo(&exe) {
        model::log(&format!("shortcuts: {} is a cargo build, leaving them alone", exe.display()));
        return vec![false; HOTKEYS.len()];
    }
    let Some(media) = gnome::settings(MEDIA_KEYS, None) else { return vec![false; HOTKEYS.len()] };
    let ours: BTreeSet<String> = KEYS.iter().map(|k| format!("{CUSTOM_PATH}{}/", k.0)).collect();
    let mut list: Vec<String> = media.strv("custom-keybindings").iter().map(|p| p.to_string()).collect();
    let taken = taken_shortcuts(&list, &ours);
    let mut free = Vec::new();
    for ((name, key, es, en), id) in KEYS.iter().zip(HOTKEYS) {
        let path = format!("{CUSTOM_PATH}{name}/");
        let binding = format!("{MODIFIERS}{key}");
        let Some(entry) = gnome::settings(CUSTOM_KEY, Some(&path)) else {
            free.push(false);
            continue;
        };
        let _ = entry.set_string("name", if english { en } else { es });
        let _ = entry.set_string("command", &format!("\"{}\" --shortcut {id}", exe.display()));
        let _ = entry.set_string("binding", &binding);
        if !list.contains(&path) {
            list.push(path);
        }
        free.push(!taken.contains(&normalize(&binding)));
    }
    let paths: Vec<&str> = list.iter().map(String::as_str).collect();
    let _ = media.set_strv("custom-keybindings", paths.as_slice());
    gnome::sync();
    free
}

/// Takes out what Everwall put in the session: its shortcuts and its
/// autostart entry (install.sh --uninstall).
pub fn uninstall() {
    if let Some(file) = autostart_file() {
        let _ = std::fs::remove_file(file);
    }
    let Some(media) = gnome::settings(MEDIA_KEYS, None) else { return };
    let ours: BTreeSet<String> = KEYS.iter().map(|k| format!("{CUSTOM_PATH}{}/", k.0)).collect();
    for path in &ours {
        if let Some(entry) = gnome::settings(CUSTOM_KEY, Some(path)) {
            for key in ["name", "command", "binding"] {
                entry.reset(key);
            }
        }
    }
    let list: Vec<String> = media.strv("custom-keybindings").iter().map(|p| p.to_string()).filter(|p| !ours.contains(p)).collect();
    let paths: Vec<&str> = list.iter().map(String::as_str).collect();
    let _ = media.set_strv("custom-keybindings", paths.as_slice());
    gnome::sync();
}

/// Every shortcut GNOME's window manager, shell, media keys and the other
/// custom shortcuts use, normalised.
fn taken_shortcuts(custom: &[String], ours: &BTreeSet<String>) -> BTreeSet<String> {
    let mut taken = BTreeSet::new();
    let mut add = |value: Variant| match value.type_().as_str() {
        "as" => taken.extend(value.get::<Vec<String>>().unwrap_or_default().iter().map(|s| normalize(s))),
        "s" => taken.extend(value.get::<String>().map(|s| normalize(&s))),
        _ => {}
    };
    let schemas = [
        "org.gnome.desktop.wm.keybindings",
        "org.gnome.shell.keybindings",
        "org.gnome.mutter.keybindings",
        "org.gnome.mutter.wayland.keybindings",
        MEDIA_KEYS,
    ];
    for schema in schemas {
        let Some(settings) = gnome::settings(schema, None) else { continue };
        let Some(keys) = settings.settings_schema().map(|s| s.list_keys()) else { continue };
        for key in keys {
            if key.as_str() != "custom-keybindings" {
                add(settings.value(&key));
            }
        }
    }
    for path in custom.iter().filter(|p| !ours.contains(*p)) {
        if let Some(entry) = gnome::settings(CUSTOM_KEY, Some(path)) {
            add(entry.value("binding"));
        }
    }
    taken.remove("");
    taken
}

/// "<Shift><Control><Alt>Left" and "<Ctrl><Alt><Shift>left" are the same key.
fn normalize(accelerator: &str) -> String {
    let mut rest = accelerator.trim();
    let mut modifiers = Vec::new();
    while let Some(after) = rest.strip_prefix('<') {
        let Some((modifier, tail)) = after.split_once('>') else { break };
        let modifier = modifier.to_lowercase();
        modifiers.push(match modifier.as_str() {
            "ctrl" | "primary" | "control_l" => "control".to_string(),
            _ => modifier,
        });
        rest = tail;
    }
    if rest.is_empty() {
        return String::new();
    }
    modifiers.sort();
    modifiers.dedup();
    format!("{}{}", modifiers.iter().map(|m| format!("<{m}>")).collect::<String>(), rest.to_lowercase())
}

/// zenity is GNOME's own little dialog program; the window has no handle to
/// give it.
pub fn pick_folder(_owner: Option<isize>, title: &str) -> Option<String> {
    let output = std::process::Command::new("zenity")
        .args(["--file-selection", "--directory", "--title", title])
        .output()
        .map_err(|error| model::log(&format!("zenity: {error}")))
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !path.is_empty()).then_some(path)
}

/// Opens the folder with the file selected (Files and most file managers
/// answer org.freedesktop.FileManager1); otherwise just the folder.
pub fn reveal(path: &str) {
    let uri = gio::File::for_path(path).uri().to_string();
    let args = (vec![uri], String::new()).to_variant();
    let shown = gnome::call(
        "org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1",
        "ShowItems",
        Some(&args),
    );
    if shown.is_err() {
        if let Some(folder) = Path::new(path).parent() {
            let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_shortcut_written_two_ways_is_the_same_key() {
        assert_eq!(normalize("<Shift><Control><Alt>Left"), normalize("<Ctrl><Alt><Shift>left"));
        assert_ne!(normalize("<Super><Control><Alt>Left"), normalize("<Control><Alt><Shift>Left"));
        assert_eq!(normalize("<Super>"), "");
    }
}
