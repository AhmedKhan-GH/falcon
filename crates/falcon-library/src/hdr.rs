//! High dynamic range images (Radiance HDR and OpenEXR): decoding to linear
//! floating point and tone mapping for display.
//!
//! Eagle tone-maps these with Reinhard at an exposure of 1.0 and sRGB
//! output; [`tone_map`] does the same per channel: scale by the exposure,
//! `c / (1 + c)`, then the linear to sRGB encode.

use std::path::Path;

use image::{Rgba, Rgba32FImage, RgbaImage};

use crate::thumbnail::ThumbError;

/// Whether `ext` (lowercase) is a high dynamic range format.
pub fn is_hdr(ext: &str) -> bool {
    matches!(ext, "hdr" | "exr")
}

fn err(e: impl std::fmt::Display) -> ThumbError {
    ThumbError(e.to_string())
}

/// Decodes `path` to linear RGBA floats, turned by its orientation if any.
pub fn decode_linear(path: &Path) -> Result<Rgba32FImage, ThumbError> {
    let img = image::ImageReader::open(path).map_err(err)?.with_guessed_format().map_err(err)?.decode().map_err(err)?;
    Ok(img.into_rgba32f())
}

/// Exposure multiplier for a slider in stops (EV): 2^stops.
pub fn stops_to_exposure(stops: f32) -> f32 {
    stops.exp2()
}

/// Linear light to an sRGB-encoded value, both 0..1.
fn srgb_encode(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn channel(c: f32, exposure: f32) -> u8 {
    let c = c * exposure;
    // NaN and negative values come out black; infinity saturates to white.
    let c = if c.is_nan() || c < 0.0 { 0.0 } else if c.is_infinite() { 1.0 } else { c / (1.0 + c) };
    (srgb_encode(c) * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// Tone maps linear `img` for display: Reinhard at `exposure` (a multiplier,
/// see [`stops_to_exposure`]), sRGB encoded. Alpha is the image's own,
/// clamped to 0..1 (Radiance files have none, so it is 1).
pub fn tone_map(img: &Rgba32FImage, exposure: f32) -> RgbaImage {
    // Encoding is a pow per channel; a table over 12 bits of the Reinhard
    // output would be coarser than the 8-bit result needs, but the direct
    // maths is simple and fast enough per pixel here.
    let mut out = RgbaImage::new(img.width(), img.height());
    for (src, dst) in img.pixels().zip(out.pixels_mut()) {
        let Rgba([r, g, b, a]) = *src;
        let a = if a.is_nan() { 1.0 } else { a.clamp(0.0, 1.0) };
        *dst = Rgba([channel(r, exposure), channel(g, exposure), channel(b, exposure), (a * 255.0 + 0.5) as u8]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> Rgba32FImage {
        Rgba32FImage::from_fn(4, 3, |x, y| Rgba([x as f32 * 0.5, y as f32, 4.0, 1.0]))
    }

    #[test]
    fn extensions() {
        assert!(is_hdr("hdr") && is_hdr("exr"));
        assert!(!is_hdr("png") && !is_hdr("HDR"));
    }

    #[test]
    fn radiance_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.hdr");
        let img = gradient();
        let pixels: Vec<image::Rgb<f32>> = img.pixels().map(|p| image::Rgb([p[0], p[1], p[2]])).collect();
        let file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        image::codecs::hdr::HdrEncoder::new(file).encode(&pixels, 4, 3).unwrap();
        let back = decode_linear(&path).unwrap();
        assert_eq!(back.dimensions(), (4, 3));
        let p = back.get_pixel(2, 1);
        // RGBE keeps about 8 bits of mantissa shared by the channels.
        assert!((p[0] - 1.0).abs() < 0.03 && (p[1] - 1.0).abs() < 0.03 && (p[2] - 4.0).abs() < 0.05, "{p:?}");
        assert_eq!(p[3], 1.0);
    }

    #[test]
    fn openexr_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.exr");
        let img = Rgba32FImage::from_fn(4, 3, |x, y| Rgba([x as f32 * 0.5, y as f32, 4.0, 0.5]));
        image::DynamicImage::ImageRgba32F(img).save(&path).unwrap();
        let back = decode_linear(&path).unwrap();
        assert_eq!(back.dimensions(), (4, 3));
        assert_eq!(*back.get_pixel(2, 1), Rgba([1.0, 1.0, 4.0, 0.5]));
        assert_eq!(tone_map(&back, 1.0).get_pixel(0, 0)[3], 128, "the file's alpha is kept");
    }

    #[test]
    fn reinhard_srgb() {
        let img = gradient();
        let out = tone_map(&img, 1.0);
        assert_eq!(out.get_pixel(0, 0).0[..2], [0, 0], "zero stays black");
        // 1.0 -> 0.5 linear -> 0.7354 sRGB.
        assert_eq!(out.get_pixel(2, 0)[0], 188);
        assert_eq!(out.get_pixel(0, 0)[3], 255);
        let bright = tone_map(&img, 4.0);
        assert!(bright.get_pixel(2, 0)[0] > out.get_pixel(2, 0)[0]);
        let dark = tone_map(&img, 0.25);
        assert!(dark.get_pixel(2, 0)[0] < out.get_pixel(2, 0)[0]);
        let ramp: Vec<u8> = (0..40).map(|i| channel(i as f32 * 0.25, 1.0)).collect();
        assert!(ramp.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(channel(f32::NAN, 1.0), 0);
        assert_eq!(channel(-3.0, 1.0), 0);
        assert_eq!(channel(f32::INFINITY, 1.0), 255);
    }

    #[test]
    fn stops() {
        assert_eq!(stops_to_exposure(0.0), 1.0);
        assert_eq!(stops_to_exposure(2.0), 4.0);
        assert_eq!(stops_to_exposure(-1.0), 0.5);
    }
}
