//! Finding the images of a collection and knowing their shape without
//! decoding them: sizes come from the file header and are remembered in
//! index.json, keyed by path, length and modification time.

use crate::model::{self, Settings, FAVORITES_ID};
use image::metadata::Orientation;
use image::{ImageDecoder, ImageReader};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "bmp"];

#[derive(Clone, Debug)]
pub struct ImageInfo {
    pub path: String,
    pub folder: String,
    pub w: u32,
    pub h: u32,
}

impl ImageInfo {
    pub fn portrait(&self) -> bool {
        self.h > self.w
    }

    pub fn ratio(&self) -> f64 {
        self.w as f64 / self.h.max(1) as f64
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Entry {
    len: u64,
    mtime: i64,
    w: u32,
    h: u32,
}

#[derive(Serialize, Deserialize, Default)]
pub struct DimIndex {
    entries: HashMap<String, Entry>,
    #[serde(skip)]
    dirty: bool,
}

impl DimIndex {
    pub fn load() -> Self {
        model::load(model::INDEX_FILE).unwrap_or_default()
    }

    fn save_if_dirty(&mut self) {
        if self.dirty {
            model::save(model::INDEX_FILE, self);
            self.dirty = false;
        }
    }

    fn dims(&mut self, path: &Path) -> Option<(u32, u32)> {
        let meta = std::fs::metadata(path).ok()?;
        let len = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let key = path.to_string_lossy().to_string();
        if let Some(e) = self.entries.get(&key) {
            if e.len == len && e.mtime == mtime {
                return Some((e.w, e.h));
            }
        }
        let (w, h) = read_dims(path)?;
        self.entries.insert(key, Entry { len, mtime, w, h });
        self.dirty = true;
        Some((w, h))
    }
}

/// Width and height as the image is meant to be seen (EXIF rotation applied).
pub fn read_dims(path: &Path) -> Option<(u32, u32)> {
    let mut decoder = ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let turned = matches!(
        decoder.orientation().unwrap_or(Orientation::NoTransforms),
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH
    );
    Some(if turned { (h, w) } else { (w, h) })
}

pub fn list_files(folders: &[String], recursive: bool) -> Vec<PathBuf> {
    let own_data = model::data_dir();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<(PathBuf, u32)> = folders.iter().map(|f| (PathBuf::from(f), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        if dir.starts_with(&own_data) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if recursive && depth < 12 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if EXTENSIONS.contains(&ext.as_str()) && seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The images of a collection that can be shown: readable and not hidden.
pub fn images(
    settings: &Settings,
    favorites: &BTreeSet<String>,
    hidden: &BTreeSet<String>,
    id: &str,
    index: &mut DimIndex,
) -> Vec<ImageInfo> {
    let files: Vec<PathBuf> = if id == FAVORITES_ID {
        favorites.iter().map(PathBuf::from).filter(|p| p.is_file()).collect()
    } else {
        match settings.collection(id) {
            Some(c) => list_files(&c.folders, c.recursive),
            None => Vec::new(),
        }
    };
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let path = file.to_string_lossy().to_string();
        if hidden.contains(&path) {
            continue;
        }
        if let Some((w, h)) = index.dims(&file) {
            let folder = file.parent().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            out.push(ImageInfo { path, folder, w, h });
        }
    }
    index.save_if_dirty();
    out
}
