//! Windows keeps the accent colour in a few HKCU values (the same ones
//! Settings > Personalization > Colors writes); the old values are saved
//! first so switching the option off puts back exactly what the user had.

use super::to_hsl;
use crate::model::SavedValue;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

const ACCENT_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
const DWM_KEY: &str = r"Software\Microsoft\Windows\DWM";
const DESKTOP_KEY: &str = r"Control Panel\Desktop";

const VALUES: [(&str, &str); 7] = [
    (ACCENT_KEY, "AccentPalette"),
    (ACCENT_KEY, "AccentColorMenu"),
    (ACCENT_KEY, "StartColorMenu"),
    (DWM_KEY, "AccentColor"),
    (DWM_KEY, "ColorizationColor"),
    (DWM_KEY, "ColorizationAfterglow"),
    (DESKTOP_KEY, "AutoColorization"),
];

pub fn backup() -> Vec<SavedValue> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    VALUES
        .iter()
        .map(|(key, name)| {
            let raw = hkcu.open_subkey(key).ok().and_then(|k| k.get_raw_value(name).ok());
            SavedValue {
                key: key.to_string(),
                name: name.to_string(),
                kind: raw.as_ref().map(|r| r.vtype.clone() as u32),
                bytes: raw.map(|r| r.bytes).unwrap_or_default(),
            }
        })
        .collect()
}

pub fn restore(saved: &[SavedValue]) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for value in saved {
        let Ok((key, _)) = hkcu.create_subkey(&value.key) else { continue };
        match value.kind {
            Some(kind) => {
                let _ = key.set_raw_value(&value.name, &RegValue { bytes: value.bytes.clone(), vtype: reg_type(kind) });
            }
            None => {
                let _ = key.delete_value(&value.name);
            }
        }
    }
    broadcast();
}

fn reg_type(kind: u32) -> RegType {
    match kind {
        1 => REG_SZ,
        2 => REG_EXPAND_SZ,
        4 => REG_DWORD,
        7 => REG_MULTI_SZ,
        11 => REG_QWORD,
        _ => REG_BINARY,
    }
}

/// Writes the accent palette built around `rgb`: three lighter shades, the
/// accent itself, three darker ones (Windows' own layout of AccentPalette).
pub fn apply(rgb: [u8; 3]) -> Result<[u8; 3], String> {
    let (h, s, l) = to_hsl(rgb);
    let s = s.clamp(0.45, 0.85);
    let l = l.clamp(0.40, 0.55);
    let shades = [0.30, 0.20, 0.10, 0.0, -0.08, -0.16, -0.24, -0.30];
    let palette: Vec<[u8; 3]> = shades.iter().map(|d| from_hsl(h, s, (l + d).clamp(0.08, 0.92))).collect();
    let mut bytes = Vec::with_capacity(32);
    for c in &palette {
        bytes.extend_from_slice(&[c[0], c[1], c[2], 0]);
    }
    let accent = palette[3];
    let abgr = |c: [u8; 3]| 0xFF00_0000u32 | (c[2] as u32) << 16 | (c[1] as u32) << 8 | c[0] as u32;
    let argb = |c: [u8; 3]| 0xC400_0000u32 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;

    let fail = |e: std::io::Error| e.to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(ACCENT_KEY).map_err(fail)?;
    key.set_raw_value("AccentPalette", &RegValue { bytes, vtype: REG_BINARY }).map_err(fail)?;
    key.set_value("AccentColorMenu", &abgr(accent)).map_err(fail)?;
    key.set_value("StartColorMenu", &abgr(palette[5])).map_err(fail)?;
    let (key, _) = hkcu.create_subkey(DWM_KEY).map_err(fail)?;
    key.set_value("AccentColor", &abgr(accent)).map_err(fail)?;
    key.set_value("ColorizationColor", &argb(accent)).map_err(fail)?;
    key.set_value("ColorizationAfterglow", &argb(accent)).map_err(fail)?;
    // Windows' own "pick from background" would overwrite ours.
    let (key, _) = hkcu.create_subkey(DESKTOP_KEY).map_err(fail)?;
    key.set_value("AutoColorization", &0u32).map_err(fail)?;
    broadcast();
    Ok(accent)
}

/// Tells every window the colours changed. It waits on each window in turn
/// (measured: 5 to 9 s on a three-monitor PC), so it runs on its own thread and the
/// next wallpaper change does not wait for it.
fn broadcast() {
    std::thread::spawn(|| {
        let topic: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(topic.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                1500,
                None,
            );
        }
    });
}

fn from_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to(r), to(g), to(b)]
}
