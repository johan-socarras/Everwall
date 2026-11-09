//! Small pieces of the operating system: local time, the system language,
//! starting with the session, one instance only (a second launch passes its
//! order to the first), the keyboard shortcuts, the folder picker and showing
//! a file in the file manager. One module per system, with the same functions.

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

/// What the messages call the system ("Windows did not accept…").
#[cfg(windows)]
pub const NAME: &str = "Windows";
#[cfg(target_os = "linux")]
pub const NAME: &str = "GNOME";

pub struct LocalTime {
    /// yyyymmdd
    pub day: u32,
    pub hour: u32,
    pub stamp: String,
}

/// Ids of the four shortcuts: next, back, favourite, hide. The window names
/// the keys in its own language.
pub const HOTKEYS: [i32; 4] = [1, 2, 3, 4];
