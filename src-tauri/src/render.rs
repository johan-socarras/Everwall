//! Drawing the final wallpaper at the exact pixel size of each screen, so
//! Windows never has to stretch or crop it:
//! - Fill: one image covering the screen (cropped to its shape).
//! - Blur: the whole image centred over a blurred, darkened copy of itself.
//! - Triptych: three images of the other shape over a blurred background:
//!   portraits side by side on a wide screen, landscapes stacked on a tall one.
//!
//! Blur and Triptych keep their panels inside the work area, clear of the
//! taskbar.

use crate::model::Layout;
use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};
use std::path::Path;

/// Part of the screen not covered by the taskbar, relative to the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub fn load(path: &str) -> Result<RgbImage, String> {
    let reader = ImageReader::open(path).map_err(|e| e.to_string())?;
    let reader = reader.with_guessed_format().map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    Ok(image.to_rgb8())
}

pub fn render(layout: Layout, sources: &[RgbImage], w: u32, h: u32, area: Area) -> RgbImage {
    // A work area that makes no sense (or none at all) means the whole screen.
    let area = if area.w < w / 2 || area.h < h / 2 || area.x + area.w > w || area.y + area.h > h {
        Area { x: 0, y: 0, w, h }
    } else {
        area
    };
    match layout {
        Layout::Fill => cover(&sources[0], w, h),
        Layout::Blur => {
            let source = &sources[0];
            let mut out = blurred(source, w, h);
            let margin = (area.w.min(area.h) as f32 * 0.05).round() as u32;
            let (iw, ih) = source.dimensions();
            let scale = ((area.w - 2 * margin) as f64 / iw as f64).min((area.h - 2 * margin) as f64 / ih as f64);
            let fw = ((iw as f64 * scale).round() as u32).max(1);
            let fh = ((ih as f64 * scale).round() as u32).max(1);
            let front = resize_hq(source, fw, fh);
            let x = area.x + (area.w - fw) / 2;
            let y = area.y + (area.h - fh) / 2;
            let r = radius(area);
            shadow(&mut out, x, y, fw, fh, r, margin.max(10) as f32 * 0.6);
            paste_rounded(&mut out, &front, x, y, r);
            out
        }
        Layout::Triptych => {
            // side by side on a wide screen, one above the other on a tall one
            let middle = &sources[sources.len() / 2];
            let mut out = blurred(middle, w, h);
            let n = sources.len() as u32;
            let stacked = area.h > area.w;
            let gap = (area.w.min(area.h) as f32 * 0.035).round().max(6.0) as u32;
            let r = radius(area);
            let (cell_w, cell_h) = if stacked {
                (area.w - 2 * gap, (area.h - (n + 1) * gap) / n)
            } else {
                ((area.w - (n + 1) * gap) / n, area.h - 2 * gap)
            };
            let first = if stacked {
                area.y + gap + (area.h - (n + 1) * gap - n * cell_h) / 2
            } else {
                area.x + gap + (area.w - (n + 1) * gap - n * cell_w) / 2
            };
            for (i, source) in sources.iter().enumerate() {
                let cell = cover(source, cell_w, cell_h);
                let (x, y) = if stacked {
                    (area.x + gap, first + i as u32 * (cell_h + gap))
                } else {
                    (first + i as u32 * (cell_w + gap), area.y + gap)
                };
                shadow(&mut out, x, y, cell_w, cell_h, r, gap as f32 * 0.7);
                paste_rounded(&mut out, &cell, x, y, r);
            }
            out
        }
    }
}

fn radius(area: Area) -> f32 {
    (area.w.min(area.h) as f32 * 0.014).max(4.0)
}

/// Scales and crops the centre so the image covers exactly w x h.
pub fn cover(image: &RgbImage, w: u32, h: u32) -> RgbImage {
    let (iw, ih) = image.dimensions();
    let scale = (w as f64 / iw as f64).max(h as f64 / ih as f64);
    let cw = ((w as f64 / scale).round() as u32).clamp(1, iw);
    let ch = ((h as f64 / scale).round() as u32).clamp(1, ih);
    let crop = imageops::crop_imm(image, (iw - cw) / 2, (ih - ch) / 2, cw, ch).to_image();
    resize_hq(&crop, w, h)
}

/// Lanczos3, after a quick box reduction when the source is far bigger
/// (a 6000 px photo would otherwise take seconds).
fn resize_hq(image: &RgbImage, w: u32, h: u32) -> RgbImage {
    let (iw, ih) = image.dimensions();
    if iw == w && ih == h {
        return image.clone();
    }
    if iw > w * 3 && ih > h * 3 {
        let reduced = imageops::thumbnail(image, w * 2, h * 2);
        return imageops::resize(&reduced, w, h, FilterType::Lanczos3);
    }
    imageops::resize(image, w, h, FilterType::Lanczos3)
}

/// The image covering the screen, heavily blurred and darkened. Blurring a
/// 1/16 copy and scaling it back up costs almost nothing and looks the same.
fn blurred(image: &RgbImage, w: u32, h: u32) -> RgbImage {
    let sw = (w / 16).max(8);
    let sh = (h / 16).max(8);
    let small = cover(image, sw, sh);
    let mut buf: Vec<f32> = small.as_raw().iter().map(|&v| v as f32).collect();
    let r = (sw.min(sh) as f32 / 12.0).round().max(2.0) as usize;
    box_blur(&mut buf, sw as usize, sh as usize, 3, r, 3);
    let data: Vec<u8> = buf.iter().map(|v| (v * 0.58).round().clamp(0.0, 255.0) as u8).collect();
    let small = RgbImage::from_raw(sw, sh, data).expect("same size as the source");
    imageops::resize(&small, w, h, FilterType::CatmullRom)
}

/// Three box passes approximate a gaussian; `ch` channels interleaved.
fn box_blur(buf: &mut [f32], w: usize, h: usize, ch: usize, r: usize, passes: usize) {
    let mut tmp = vec![0f32; buf.len()];
    for _ in 0..passes {
        blur_line(buf, &mut tmp, w, h, ch, r, true);
        blur_line(&tmp, buf, w, h, ch, r, false);
    }
}

fn blur_line(src: &[f32], dst: &mut [f32], w: usize, h: usize, ch: usize, r: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        for c in 0..ch {
            let index = |i: isize| -> usize {
                let i = i.clamp(0, len as isize - 1) as usize;
                if horizontal {
                    (line * w + i) * ch + c
                } else {
                    (i * w + line) * ch + c
                }
            };
            let mut sum = 0.0;
            for k in -(r as isize)..=(r as isize) {
                sum += src[index(k)];
            }
            for i in 0..len as isize {
                dst[index(i)] = sum * norm;
                sum += src[index(i + r as isize + 1)] - src[index(i - r as isize)];
            }
        }
    }
}

/// How much of the pixel centred at (px, py) falls inside a w x h rectangle
/// with corners of radius r (antialiased edge of about one pixel).
fn coverage(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let dx = (r - px).max(px - (w - r));
    let dy = (r - py).max(py - (h - r));
    if dx <= 0.0 || dy <= 0.0 {
        return 1.0;
    }
    (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0)
}

fn paste_rounded(out: &mut RgbImage, image: &RgbImage, x: u32, y: u32, r: f32) {
    let (w, h) = image.dimensions();
    for iy in 0..h {
        for ix in 0..w {
            let (px, py) = (x + ix, y + iy);
            if px >= out.width() || py >= out.height() {
                continue;
            }
            let a = coverage(ix as f32 + 0.5, iy as f32 + 0.5, w as f32, h as f32, r);
            if a <= 0.0 {
                continue;
            }
            let s = image.get_pixel(ix, iy).0;
            let d = out.get_pixel_mut(px, py);
            if a >= 1.0 {
                d.0 = s;
            } else {
                for (dst, src) in d.0.iter_mut().zip(s) {
                    *dst = (src as f32 * a + *dst as f32 * (1.0 - a)).round() as u8;
                }
            }
        }
    }
}

/// A soft shadow under a rounded panel, falling slightly downwards.
fn shadow(out: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, r: f32, spread: f32) {
    let spread = spread.max(2.0);
    let pad = (spread * 2.0).ceil() as i64;
    let drop = (spread * 0.35).round() as i64;
    let (mw, mh) = (w as i64 + 2 * pad, h as i64 + 2 * pad);
    let mut mask = vec![0f32; (mw * mh) as usize];
    for my in 0..h as i64 {
        for mx in 0..w as i64 {
            mask[((my + pad) * mw + mx + pad) as usize] =
                coverage(mx as f32 + 0.5, my as f32 + 0.5, w as f32, h as f32, r);
        }
    }
    box_blur(&mut mask, mw as usize, mh as usize, 1, (spread / 2.0).round().max(1.0) as usize, 3);
    let (ow, oh) = (out.width() as i64, out.height() as i64);
    for my in 0..mh {
        for mx in 0..mw {
            let a = mask[(my * mw + mx) as usize];
            if a <= 0.002 {
                continue;
            }
            let (px, py) = (x as i64 - pad + mx, y as i64 - pad + my + drop);
            if px < 0 || py < 0 || px >= ow || py >= oh {
                continue;
            }
            let k = 1.0 - 0.5 * a;
            let p = out.get_pixel_mut(px as u32, py as u32);
            for c in 0..3 {
                p.0[c] = (p.0[c] as f32 * k) as u8;
            }
        }
    }
}

/// The wallpaper plus a small copy for the settings window.
pub fn save(image: &RgbImage, path: &Path, mini: &Path) -> Result<(), String> {
    write_jpeg(image, path, 95)?;
    let (w, h) = image.dimensions();
    let mw = 420u32;
    let mh = ((h as u64 * mw as u64) / w.max(1) as u64).max(1) as u32;
    write_jpeg(&imageops::thumbnail(image, mw, mh), mini, 82)
}

pub fn write_jpeg(image: &RgbImage, path: &Path, quality: u8) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    JpegEncoder::new_with_quality(&mut writer, quality).encode_image(image).map_err(|e| e.to_string())?;
    use std::io::Write;
    writer.flush().map_err(|e| e.to_string())
}

pub fn data_url(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Small preview of a source image (for the list of what is on screen).
pub fn preview(path: &str, max: u32) -> Option<String> {
    let image = load(path).ok()?;
    let (w, h) = image.dimensions();
    let scale = (max as f64 / w as f64).min(max as f64 / h as f64).min(1.0);
    let small = imageops::thumbnail(&image, ((w as f64 * scale) as u32).max(1), ((h as f64 * scale) as u32).max(1));
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 80).encode_image(&small).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn plain(w: u32, h: u32, colour: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb(colour))
    }

    #[test]
    fn every_layout_comes_out_at_the_exact_screen_size() {
        let portrait = plain(60, 90, [200, 80, 40]);
        let landscape = plain(90, 60, [40, 80, 200]);
        let three = [portrait.clone(), portrait.clone(), portrait.clone()];
        for (w, h) in [(192, 108), (108, 192), (171, 97)] {
            let area = Area { x: 0, y: 0, w, h: h - 5 };
            assert_eq!(render(Layout::Fill, std::slice::from_ref(&landscape), w, h, area).dimensions(), (w, h));
            assert_eq!(render(Layout::Blur, std::slice::from_ref(&portrait), w, h, area).dimensions(), (w, h));
            assert_eq!(render(Layout::Triptych, &three, w, h, area).dimensions(), (w, h));
        }
    }

    #[test]
    fn the_panel_stays_out_of_the_taskbar() {
        // a white image on a 160x120 screen whose bottom 20 px are the taskbar
        let out = render(Layout::Blur, &[plain(40, 60, [255; 3])], 160, 120, Area { x: 0, y: 0, w: 160, h: 100 });
        assert!(out.get_pixel(80, 50).0[0] > 250, "the panel is in the middle of the work area");
        for x in 0..160 {
            assert!(out.get_pixel(x, 110).0[0] < 200, "only the darkened background under the taskbar");
        }
    }

    #[test]
    fn cover_fills_the_requested_size_from_any_shape() {
        for (w, h) in [(100, 50), (50, 100), (77, 77)] {
            assert_eq!(cover(&plain(64, 48, [9, 9, 9]), w, h).dimensions(), (w, h));
        }
    }
}
