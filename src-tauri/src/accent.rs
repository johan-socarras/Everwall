//! The accent colour taken from a wallpaper: the colour itself is worked out
//! here; writing it (and giving back the old one) is up to each system.

use image::RgbImage;

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

/// The most present vivid colour: hues weighted by saturation and brightness,
/// grey and near-black pixels ignored. A grey image gives a quiet slate blue.
pub fn dominant(image: &RgbImage) -> [u8; 3] {
    let small = image::imageops::thumbnail(image, 64, 36);
    let mut bins = [[0f32; 4]; 24];
    for p in small.pixels() {
        let [r, g, b] = p.0;
        let (h, s, v) = to_hsv([r, g, b]);
        if s < 0.18 || v < 0.15 {
            continue;
        }
        let weight = s * v;
        let i = ((h / 15.0) as usize).min(23);
        bins[i][0] += weight;
        bins[i][1] += r as f32 * weight;
        bins[i][2] += g as f32 * weight;
        bins[i][3] += b as f32 * weight;
    }
    let around = |i: usize| [(i + 23) % 24, i, (i + 1) % 24];
    let best = (0..24)
        .max_by(|&a, &b| {
            let wa: f32 = around(a).iter().map(|&k| bins[k][0]).sum();
            let wb: f32 = around(b).iter().map(|&k| bins[k][0]).sum();
            wa.total_cmp(&wb)
        })
        .unwrap_or(0);
    let mut sum = [0f32; 4];
    for k in around(best) {
        for c in 0..4 {
            sum[c] += bins[k][c];
        }
    }
    if sum[0] < 1.0 {
        return [0x5B, 0x6B, 0x7F];
    }
    [(sum[1] / sum[0]) as u8, (sum[2] / sum[0]) as u8, (sum[3] / sum[0]) as u8]
}

fn to_hsv([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = hue(r, g, b, max, d);
    let s = if max > 0.0 { d / max } else { 0.0 };
    (h, s, max)
}

fn to_hsl([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.0;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    (hue(r, g, b, max, d), s, l)
}

fn hue(r: f32, g: f32, b: f32, max: f32, d: f32) -> f32 {
    if d == 0.0 {
        return 0.0;
    }
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    if h < 0.0 { h + 360.0 } else { h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn a_vivid_wallpaper_gives_its_own_colour() {
        let [r, g, b] = dominant(&RgbImage::from_pixel(64, 36, Rgb([220, 30, 30])));
        assert!(r > 200 && g < 60 && b < 60, "#{r:02x}{g:02x}{b:02x}");
    }

    #[test]
    fn a_grey_wallpaper_gives_a_quiet_slate_blue() {
        assert_eq!(dominant(&RgbImage::from_pixel(64, 36, Rgb([128, 128, 128]))), [0x5B, 0x6B, 0x7F]);
    }

    #[test]
    fn the_colour_that_covers_most_of_the_wallpaper_wins() {
        // two thirds green, one third blue
        let mut image = RgbImage::from_pixel(60, 30, Rgb([30, 180, 60]));
        for x in 40..60 {
            for y in 0..30 {
                image.put_pixel(x, y, Rgb([30, 60, 200]));
            }
        }
        let [r, g, b] = dominant(&image);
        assert!(g > r && g > b, "#{r:02x}{g:02x}{b:02x}");
    }
}
