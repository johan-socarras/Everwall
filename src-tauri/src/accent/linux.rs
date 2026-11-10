//! GNOME (47 and later) has nine named accent colours in
//! org.gnome.desktop.interface accent-color, not any colour: Everwall picks
//! the one closest to the wallpaper's. The old value is saved first so
//! switching the option off puts back exactly what the user had.

use super::to_hsl;
use crate::gnome;
use crate::model::SavedValue;
use gio::prelude::*;

const INTERFACE: &str = "org.gnome.desktop.interface";
const KEY: &str = "accent-color";

/// GNOME's own accent colours (libadwaita's), slate last: it is the one for
/// grey wallpapers.
const COLOURS: [(&str, [u8; 3]); 9] = [
    ("blue", [0x35, 0x84, 0xE4]),
    ("teal", [0x21, 0x90, 0xA4]),
    ("green", [0x3A, 0x94, 0x4A]),
    ("yellow", [0xC8, 0x88, 0x00]),
    ("orange", [0xED, 0x5B, 0x00]),
    ("red", [0xE6, 0x2D, 0x42]),
    ("pink", [0xD5, 0x61, 0x99]),
    ("purple", [0x91, 0x41, 0xAC]),
    ("slate", [0x6F, 0x83, 0x96]),
];

fn interface() -> Option<gio::Settings> {
    let settings = gnome::settings(INTERFACE, None)?;
    gnome::has_key(&settings, KEY).then_some(settings)
}

pub fn backup() -> Vec<SavedValue> {
    let Some(settings) = interface() else { return Vec::new() };
    vec![SavedValue {
        key: INTERFACE.into(),
        name: KEY.into(),
        kind: Some(1),
        bytes: settings.string(KEY).as_bytes().to_vec(),
    }]
}

pub fn restore(saved: &[SavedValue]) {
    let Some(settings) = interface() else { return };
    for value in saved.iter().filter(|v| v.key == INTERFACE && v.name == KEY) {
        let _ = settings.set_string(KEY, &String::from_utf8_lossy(&value.bytes));
    }
    gnome::sync();
}

/// Sets the GNOME accent colour nearest to `rgb` and returns that colour.
pub fn apply(rgb: [u8; 3]) -> Result<[u8; 3], String> {
    let settings = interface().ok_or("this GNOME has no accent colour setting (it came with GNOME 47)")?;
    let (name, colour) = nearest(rgb);
    settings.set_string(KEY, name).map_err(|e| e.to_string())?;
    gnome::sync();
    Ok(colour)
}

/// By hue; a washed-out or very dark colour gets slate.
fn nearest(rgb: [u8; 3]) -> (&'static str, [u8; 3]) {
    let (h, s, l) = to_hsl(rgb);
    if s < 0.22 || l < 0.12 {
        return COLOURS[8];
    }
    let distance = |c: [u8; 3]| {
        let d = (to_hsl(c).0 - h).abs();
        d.min(360.0 - d)
    };
    COLOURS[..8].iter().copied().min_by(|a, b| distance(a.1).total_cmp(&distance(b.1))).unwrap_or(COLOURS[8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_colour_maps_to_the_nearest_gnome_accent() {
        assert_eq!(nearest([0x35, 0x84, 0xE4]).0, "blue");
        assert_eq!(nearest([230, 40, 60]).0, "red");
        assert_eq!(nearest([60, 150, 70]).0, "green");
        assert_eq!(nearest([128, 128, 128]).0, "slate");
    }
}
