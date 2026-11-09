//! Windows: the monitors and their wallpapers through Windows' own
//! per-monitor wallpaper interface (IDesktopWallpaper, the one Settings uses),
//! plus the monitors' real names from the display configuration.

use super::Screen;
use crate::render::Area;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, SHQueryUserNotificationState, DWPOS_FILL, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Owns the COM object, so it lives and dies on one thread.
pub struct Desktop {
    wallpaper: IDesktopWallpaper,
}

impl Desktop {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let wallpaper: IDesktopWallpaper =
                CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
            Ok(Self { wallpaper })
        }
    }

    /// Connected monitors, left to right. Windows also lists monitors it
    /// remembers but are off; those have no rectangle and are skipped.
    pub fn screens(&self) -> Vec<Screen> {
        let names = friendly_names();
        let mut out = Vec::new();
        unsafe {
            let count = self.wallpaper.GetMonitorDevicePathCount().unwrap_or(0);
            for i in 0..count {
                let Ok(raw) = self.wallpaper.GetMonitorDevicePathAt(i) else { continue };
                let id = take(raw);
                let Ok(rect) = self.wallpaper.GetMonitorRECT(&HSTRING::from(id.as_str())) else { continue };
                let w = (rect.right - rect.left).max(0) as u32;
                let h = (rect.bottom - rect.top).max(0) as u32;
                if w == 0 || h == 0 {
                    continue;
                }
                let name = names.get(&id.to_lowercase()).cloned().unwrap_or_else(|| fallback_name(&id));
                out.push(Screen {
                    area: work_area(&rect),
                    name,
                    x: rect.left,
                    y: rect.top,
                    w,
                    h,
                    primary: rect.left == 0 && rect.top == 0,
                    id,
                });
            }
        }
        out.sort_by_key(|s| (s.x, s.y));
        out
    }

    pub fn set(&self, id: &str, file: &str) -> Result<(), String> {
        unsafe { self.wallpaper.SetWallpaper(&HSTRING::from(id), &HSTRING::from(file)).map_err(|e| e.to_string()) }
    }

    pub fn get(&self, id: &str) -> Option<String> {
        unsafe { self.wallpaper.GetWallpaper(&HSTRING::from(id)).ok().map(take) }
    }

    /// Everwall renders every image at the screen's exact size, so "Fill" is
    /// the position that leaves it untouched.
    pub fn use_fill(&self) {
        unsafe {
            let _ = self.wallpaper.SetPosition(DWPOS_FILL);
        }
    }

    /// Each monitor's wallpaper before Everwall changed it.
    pub fn originals(&self, screens: &[Screen]) -> BTreeMap<String, String> {
        screens.iter().filter_map(|s| self.get(&s.id).map(|path| (s.id.clone(), path))).collect()
    }

    pub fn restore(&self, original: &BTreeMap<String, String>, screens: &[Screen]) {
        for s in screens {
            if let Some(path) = original.get(&s.id).filter(|p| Path::new(p).is_file()) {
                let _ = self.set(&s.id, path);
            }
        }
    }

    /// Windows takes each monitor's image at once in `set`: nothing waits.
    pub fn flush(&self, _screens: &[Screen]) -> Result<(), String> {
        Ok(())
    }
}

fn take(text: PWSTR) -> String {
    unsafe {
        let value = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0 as *const _));
        value
    }
}

fn fallback_name(id: &str) -> String {
    id.split('#').nth(1).unwrap_or("Screen").to_string()
}

fn work_area(rect: &RECT) -> Area {
    let w = (rect.right - rect.left).max(0) as u32;
    let h = (rect.bottom - rect.top).max(0) as u32;
    unsafe {
        let monitor = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = info.rcWork;
            let x = (r.left - rect.left).max(0) as u32;
            let y = (r.top - rect.top).max(0) as u32;
            let ww = ((r.right - r.left).max(0) as u32).min(w.saturating_sub(x));
            let wh = ((r.bottom - r.top).max(0) as u32).min(h.saturating_sub(y));
            if ww > w / 2 && wh > h / 2 {
                return Area { x, y, w: ww, h: wh };
            }
        }
    }
    Area { x: 0, y: 0, w, h }
}

/// "HP E202" and friends, keyed by the same device path IDesktopWallpaper
/// uses as monitor id.
fn friendly_names() -> HashMap<String, String> {
    let mut names = HashMap::new();
    unsafe {
        let (mut paths_len, mut modes_len) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_len, &mut modes_len) != ERROR_SUCCESS {
            return names;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_len as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_len as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut paths_len,
            paths.as_mut_ptr(),
            &mut modes_len,
            modes.as_mut_ptr(),
            None,
        ) != ERROR_SUCCESS
        {
            return names;
        }
        for path in paths.iter().take(paths_len as usize) {
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut target.header) != 0 {
                continue;
            }
            let name = wide(&target.monitorFriendlyDeviceName);
            let device = wide(&target.monitorDevicePath);
            if !name.is_empty() && !device.is_empty() {
                names.insert(device.to_lowercase(), name);
            }
        }
    }
    names
}

fn wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// A full-screen game, video or presentation is in front.
pub fn busy() -> bool {
    unsafe {
        matches!(SHQueryUserNotificationState(),
            Ok(s) if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE)
    }
}

pub fn cursor() -> Option<(i32, i32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point).ok()? };
    Some((point.x, point.y))
}
