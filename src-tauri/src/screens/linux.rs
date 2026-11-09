//! GNOME: the monitors come from Mutter's display configuration over D-Bus.
//! GNOME shows a single picture across all of them, so each screen's image is
//! kept here and `flush` joins them into one picture the size of the whole
//! desktop, set with "spanned" placement: every screen shows its own part.

use super::Screen;
use crate::render::Area;
use crate::{gnome, model, render};
use gio::glib::Variant;
use gio::prelude::*;
use image::RgbImage;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

const BACKGROUND: &str = "org.gnome.desktop.background";
/// What Everwall changes in the background settings, and gives back.
const KEYS: [&str; 3] = ["picture-uri", "picture-uri-dark", "picture-options"];

/// Lives on the engine thread only.
pub struct Desktop {
    /// The image each screen should show, by screen id.
    files: RefCell<BTreeMap<String, String>>,
    /// Something changed since the last joined picture.
    dirty: Cell<bool>,
}

impl Desktop {
    pub fn new() -> Result<Self, String> {
        if gnome::settings(BACKGROUND, None).is_none() {
            return Err(format!("{BACKGROUND} is not installed"));
        }
        Ok(Self { files: RefCell::default(), dirty: Cell::new(false) })
    }

    /// Connected monitors, left to right.
    pub fn screens(&self) -> Vec<Screen> {
        let mut out = monitors().unwrap_or_else(|error| {
            model::log(&format!("could not read the screens from Mutter: {error}"));
            Vec::new()
        });
        let areas = work_areas();
        for s in &mut out {
            s.area = area_of(s, &areas);
        }
        out.sort_by_key(|s| (s.x, s.y));
        out
    }

    /// Only remembered: the screen changes with the next `flush`.
    pub fn set(&self, id: &str, file: &str) -> Result<(), String> {
        self.files.borrow_mut().insert(id.to_string(), file.to_string());
        self.dirty.set(true);
        Ok(())
    }

    /// The image Everwall gave this screen; before that, the desktop's own
    /// picture.
    pub fn get(&self, id: &str) -> Option<String> {
        if let Some(file) = self.files.borrow().get(id) {
            return Some(file.clone());
        }
        let uri = gnome::settings(BACKGROUND, None)?.string("picture-uri");
        gio::File::for_uri(&uri).path().map(|p| p.display().to_string())
    }

    /// The joined picture is always set as "spanned" in `flush`.
    pub fn use_fill(&self) {}

    /// The background settings before Everwall changed them, as GVariant text.
    pub fn originals(&self, _screens: &[Screen]) -> BTreeMap<String, String> {
        let Some(settings) = gnome::settings(BACKGROUND, None) else { return BTreeMap::new() };
        KEYS.iter().map(|key| (key.to_string(), settings.value(key).print(true).to_string())).collect()
    }

    pub fn restore(&self, original: &BTreeMap<String, String>, _screens: &[Screen]) {
        let Some(settings) = gnome::settings(BACKGROUND, None) else { return };
        for key in KEYS {
            if let Some(value) = original.get(key).and_then(|text| Variant::parse(None, text).ok()) {
                let _ = settings.set_value(key, &value);
            }
        }
        gnome::sync();
        self.files.borrow_mut().clear();
        self.dirty.set(false);
    }

    /// Joins the screens' images into one picture and sets it. A new file
    /// name each time, or GNOME would keep showing the one it already loaded.
    pub fn flush(&self, screens: &[Screen]) -> Result<(), String> {
        if !self.dirty.replace(false) || screens.is_empty() {
            return Ok(());
        }
        let left = screens.iter().map(|s| s.x).min().unwrap_or(0);
        let top = screens.iter().map(|s| s.y).min().unwrap_or(0);
        let right = screens.iter().map(|s| s.x + s.w as i32).max().unwrap_or(0);
        let bottom = screens.iter().map(|s| s.y + s.h as i32).max().unwrap_or(0);
        let mut out = RgbImage::new((right - left).max(1) as u32, (bottom - top).max(1) as u32);
        let files = self.files.borrow().clone();
        for s in screens {
            let Some(file) = files.get(&s.id) else { continue };
            let image = match render::load(file) {
                Ok(image) if image.dimensions() == (s.w, s.h) => image,
                Ok(image) => render::cover(&image, s.w, s.h),
                Err(error) => {
                    model::log(&format!("{file}: {error}"));
                    continue;
                }
            };
            image::imageops::replace(&mut out, &image, (s.x - left) as i64, (s.y - top) as i64);
        }
        let dir = model::data_dir().join("desktop");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("everwall-{}.jpg", model::now_millis()));
        render::write_jpeg(&out, &file, 95)?;
        let settings = gnome::settings(BACKGROUND, None).ok_or_else(|| format!("{BACKGROUND} is not installed"))?;
        let uri = gio::File::for_path(&file).uri();
        let fail = |e: gio::glib::BoolError| e.to_string();
        settings.set_string("picture-options", "spanned").map_err(fail)?;
        settings.set_string("picture-uri", &uri).map_err(fail)?;
        settings.set_string("picture-uri-dark", &uri).map_err(fail)?;
        gnome::sync();
        // GNOME already read the new picture's name; the old ones can go
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten().filter(|e| e.path() != file) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        Ok(())
    }
}

/// One entry per logical monitor (a mirrored pair counts once), in the
/// desktop's own coordinates. With fractional scaling in logical layout mode
/// the sizes are logical pixels, which is what "spanned" spreads the picture
/// over.
fn monitors() -> Result<Vec<Screen>, String> {
    let state = gnome::call(
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
        "GetCurrentState",
        None,
    )?;
    // (serial, monitors a((ssss)a(siiddada{sv})a{sv}), logical a(iiduba(ssss)a{sv}), properties a{sv})
    let physical = state.child_value(1);
    let logical = state.child_value(2);
    let logical_layout = prop(&state.child_value(3), "layout-mode").and_then(|v| v.get::<u32>()).unwrap_or(1) == 1;

    struct Physical {
        id: String,
        name: String,
        w: i32,
        h: i32,
    }
    let mut by_connector: HashMap<String, Physical> = HashMap::new();
    for monitor in physical.iter() {
        let spec = monitor.child_value(0);
        let text = |i: usize| spec.child_value(i).get::<String>().unwrap_or_default();
        let (connector, vendor, product, serial) = (text(0), text(1), text(2), text(3));
        let current = monitor.child_value(1).iter().find(|mode| {
            prop(&mode.child_value(6), "is-current").and_then(|v| v.get::<bool>()).unwrap_or(false)
        });
        let Some(mode) = current else { continue };
        let name = prop(&monitor.child_value(2), "display-name").and_then(|v| v.get::<String>()).unwrap_or_else(|| product.clone());
        // vendor, model and serial follow the monitor to another cable; two
        // identical monitors without a serial fall back to the connector
        let id = if serial.trim().is_empty() || serial == "0x00000000" {
            format!("{vendor}-{product}-{connector}")
        } else {
            format!("{vendor}-{product}-{serial}")
        };
        let (w, h) = (mode.child_value(1).get::<i32>().unwrap_or(0), mode.child_value(2).get::<i32>().unwrap_or(0));
        by_connector.insert(connector, Physical { id, name, w, h });
    }

    let mut out = Vec::new();
    for lm in logical.iter() {
        let x = lm.child_value(0).get::<i32>().unwrap_or(0);
        let y = lm.child_value(1).get::<i32>().unwrap_or(0);
        let scale = lm.child_value(2).get::<f64>().unwrap_or(1.0);
        let transform = lm.child_value(3).get::<u32>().unwrap_or(0);
        let primary = lm.child_value(4).get::<bool>().unwrap_or(false);
        let members = lm.child_value(5);
        if members.n_children() == 0 {
            continue;
        }
        let connector = members.child_value(0).child_value(0).get::<String>().unwrap_or_default();
        let Some(p) = by_connector.get(&connector) else { continue };
        // odd transforms are the 90 and 270 degree turns
        let (mut w, mut h) = if transform % 2 == 1 { (p.h, p.w) } else { (p.w, p.h) };
        if logical_layout && scale > 0.0 {
            w = (w as f64 / scale).round() as i32;
            h = (h as f64 / scale).round() as i32;
        }
        if w <= 0 || h <= 0 {
            continue;
        }
        let (w, h) = (w as u32, h as u32);
        out.push(Screen { id: p.id.clone(), name: p.name.clone(), x, y, w, h, area: Area { x: 0, y: 0, w, h }, primary });
    }
    Ok(out)
}

/// One value of an a{sv} dictionary.
fn prop(dict: &Variant, key: &str) -> Option<Variant> {
    gio::glib::VariantDict::new(Some(dict)).lookup_value(key, None)
}

/// Each monitor's work area (what the top bar, panels and docks leave free),
/// as Mutter publishes it to X11 programs in _GTK_WORKAREAS_D0. Wayland has no
/// way to ask for it, so this goes through xprop and XWayland; without them
/// every screen counts as free.
fn work_areas() -> Vec<[i32; 4]> {
    let Ok(output) = std::process::Command::new("xprop").args(["-root", "_GTK_WORKAREAS_D0"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let Some((_, numbers)) = text.split_once('=') else { return Vec::new() };
    let numbers: Vec<i32> = numbers.split(',').filter_map(|n| n.trim().parse().ok()).collect();
    numbers.as_chunks::<4>().0.to_vec()
}

fn area_of(s: &Screen, areas: &[[i32; 4]]) -> Area {
    let (sw, sh) = (s.w as i32, s.h as i32);
    areas
        .iter()
        .find(|[x, y, w, h]| *x >= s.x && *y >= s.y && x + w <= s.x + sw && y + h <= s.y + sh && *w > sw / 2 && *h > sh / 2)
        .map(|[x, y, w, h]| Area { x: (x - s.x) as u32, y: (y - s.y) as u32, w: *w as u32, h: *h as u32 })
        .unwrap_or(Area { x: 0, y: 0, w: s.w, h: s.h })
}

/// Something asked GNOME not to blank the screen: a full-screen video or
/// game (browsers also ask while any video plays). The closest GNOME has to
/// Windows' "busy" state.
pub fn busy() -> bool {
    use gio::glib::ToVariant;
    const IDLE: u32 = 8;
    gnome::call(
        "org.gnome.SessionManager",
        "/org/gnome/SessionManager",
        "org.gnome.SessionManager",
        "IsInhibited",
        Some(&(IDLE,).to_variant()),
    )
    .ok()
    .and_then(|reply| reply.child_value(0).get::<bool>())
    .unwrap_or(false)
}

/// Who is keeping the screen awake right now (for --diagnostics).
pub fn inhibitors() -> Vec<String> {
    let manager = ("org.gnome.SessionManager", "/org/gnome/SessionManager", "org.gnome.SessionManager");
    let Ok(reply) = gnome::call(manager.0, manager.1, manager.2, "GetInhibitors", None) else { return Vec::new() };
    let paths: Vec<String> = reply.child_value(0).iter().filter_map(|p| p.str().map(str::to_string)).collect();
    paths
        .iter()
        .map(|path| {
            let ask = |method: &str| {
                gnome::call(manager.0, path, "org.gnome.SessionManager.Inhibitor", method, None)
                    .ok()
                    .map(|r| r.child_value(0).print(false).to_string())
                    .unwrap_or_default()
            };
            format!("{} ({}) flags {}", ask("GetAppId"), ask("GetReason"), ask("GetFlags"))
        })
        .collect()
}

/// Wayland does not tell other programs where the pointer is.
pub fn cursor() -> Option<(i32, i32)> {
    None
}
