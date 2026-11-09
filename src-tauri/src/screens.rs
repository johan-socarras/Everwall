//! The monitors and how a wallpaper gets onto them. Windows sets one image per
//! monitor; GNOME takes a single image for the whole desktop, so on Linux the
//! screens' images are joined into one wide picture.

use crate::render::Area;
use serde::Serialize;

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[derive(Clone, Debug, Serialize)]
pub struct Screen {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    #[serde(skip)]
    pub area: Area,
    pub primary: bool,
}

impl Screen {
    pub fn landscape(&self) -> bool {
        self.w >= self.h
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w as i32 && y < self.y + self.h as i32
    }
}
