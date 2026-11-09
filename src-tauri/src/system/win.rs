//! Windows: local time, starting with Windows, one instance only (a second
//! launch passes its order to the first), global hotkeys, the folder picker
//! and showing a file in Explorer.

use super::{LocalTime, HOTKEYS};
use crate::model;
use std::time::Duration;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_DOWN, VK_LEFT, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

pub fn local_now() -> LocalTime {
    let t = unsafe { GetLocalTime() };
    LocalTime {
        day: t.wYear as u32 * 10000 + t.wMonth as u32 * 100 + t.wDay as u32,
        hour: t.wHour as u32,
        stamp: format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        ),
    }
}

/// "es" when Windows shows its interface in Spanish, "en" otherwise: the
/// language the app starts in until the user picks one.
pub fn system_language() -> &'static str {
    const LANG_SPANISH: u16 = 0x0a;
    let primary = unsafe { GetUserDefaultUILanguage() } & 0x3ff;
    if primary == LANG_SPANISH {
        "es"
    } else {
        "en"
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "Everwall";

/// Points the Run entry at this very exe, wherever it was unzipped or
/// installed; starting a moved copy points it at the new place. A build
/// straight out of a cargo target folder (it has `.fingerprint` beside it)
/// must not end up starting with Windows.
pub fn set_startup(enable: bool) {
    let Ok(exe) = std::env::current_exe() else { return };
    let from_cargo = exe.parent().map(|dir| dir.join(".fingerprint").is_dir()).unwrap_or(false);
    let exe = exe.display().to_string();
    if from_cargo {
        model::log(&format!("start with Windows: {exe} is a cargo build, leaving the registry alone"));
        return;
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok((key, _)) = hkcu.create_subkey(RUN_KEY) else { return };
    if enable {
        let _ = key.set_value(RUN_NAME, &format!("\"{exe}\" --tray"));
    } else {
        let _ = key.delete_value(RUN_NAME);
    }
}

pub fn startup_value() -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY).ok()?.get_value(RUN_NAME).ok()
}

/// True for the first Everwall running in this session.
pub fn claim_single_instance() -> bool {
    unsafe {
        match CreateMutexW(None, true, w!("Local\\Everwall-Instance")) {
            // The handle stays open on purpose: it is what marks us as running.
            Ok(_) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        }
    }
}

fn order_file() -> std::path::PathBuf {
    model::data_dir().join("order.txt")
}

/// From a second launch: leave the order and wake the running instance.
pub fn send_order(order: &str) {
    let _ = std::fs::create_dir_all(model::data_dir());
    let _ = std::fs::write(order_file(), order);
    unsafe {
        if let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\Everwall-Order")) {
            let _ = SetEvent(event);
        }
    }
}

pub fn listen_orders(on_order: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || unsafe {
        let Ok(event) = CreateEventW(None, false, false, w!("Local\\Everwall-Order")) else { return };
        loop {
            WaitForSingleObject(event, INFINITE);
            if let Ok(text) = std::fs::read_to_string(order_file()) {
                let _ = std::fs::remove_file(order_file());
                on_order(text.trim().to_string());
            }
        }
    });
}

/// Registers Ctrl+Alt+Shift + →, ←, ↑, ↓ (in the order of HOTKEYS) on their
/// own thread (they belong to the thread that registers them) and returns
/// which ones Windows accepted. Ctrl+Alt+Shift because Ctrl+Alt alone is AltGr
/// on a Spanish keyboard and would steal characters like @ or #.
pub fn start_hotkeys(_english: bool, on_key: impl Fn(i32) + Send + 'static) -> Vec<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || unsafe {
        let keys = [VK_RIGHT, VK_LEFT, VK_UP, VK_DOWN];
        let modifiers = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        let ok: Vec<bool> = HOTKEYS
            .iter()
            .zip(keys)
            .map(|(id, vk)| RegisterHotKey(None, *id, modifiers, vk.0 as u32).is_ok())
            .collect();
        let _ = tx.send(ok);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_HOTKEY {
                on_key(msg.wParam.0 as i32);
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(3)).unwrap_or_default()
}

pub fn pick_folder(owner: Option<isize>, title: &str) -> Option<String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        let _ = dialog.SetTitle(&HSTRING::from(title));
        dialog.Show(owner.map(|h| HWND(h as *mut _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        path
    }
}

pub fn reveal(path: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{path}\"")).spawn();
}
