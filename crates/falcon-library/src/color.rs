//! Turns a picture's pixels from its embedded ICC profile into sRGB for
//! showing. Falcon draws sRGB, so a picture tagged Adobe RGB or Display P3
//! would otherwise look dull. Colours outside sRGB are clipped.
//!
//! This is for what's on screen and in thumbnails. Rotating, flipping,
//! cropping and exporting rewrite the file's own pixels, so they keep
//! [`crate::thumbnail::decode`], which leaves them alone.

use image::{DynamicImage, GrayAlphaImage, RgbaImage};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformExecutor, TransformOptions};

/// Colours compared to tell a profile that changes nothing, such as sRGB's own.
const PROBE: [[u8; 4]; 8] = [
    [0, 0, 0, 255],
    [255, 255, 255, 255],
    [128, 128, 128, 255],
    [200, 60, 40, 255],
    [40, 200, 60, 255],
    [40, 60, 200, 255],
    [64, 32, 16, 255],
    [230, 200, 120, 255],
];

/// `img` in sRGB through `icc`. A profile that can't be read, doesn't match
/// the picture's colours or changes nothing leaves `img` as it is.
pub fn to_srgb(img: DynamicImage, icc: &[u8]) -> DynamicImage {
    let Ok(profile) = ColorProfile::new_from_slice(icc) else { return img };
    let srgb = ColorProfile::new_srgb();
    let options = TransformOptions::default();
    let gray = !img.color().has_color();
    match profile.color_space {
        DataColorSpace::Rgb if !gray && !is_float(&img) => {
            let Ok(transform) = profile.create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgba, options) else { return img };
            let mut probe = [0u8; PROBE.len() * 4];
            if transform.transform(PROBE.as_flattened(), &mut probe).is_err() || unchanged(PROBE.as_flattened(), &probe) {
                return img;
            }
            let mut rgba = img.to_rgba8();
            if !convert(&mut rgba, transform.as_ref()) {
                return img;
            }
            DynamicImage::ImageRgba8(rgba)
        }
        DataColorSpace::Gray if gray && !is_float(&img) => {
            let Ok(transform) = profile.create_transform_8bit(Layout::GrayAlpha, &srgb, Layout::Rgba, options) else { return img };
            gray_to_srgb(&img.to_luma_alpha8(), transform.as_ref()).map_or(img, DynamicImage::ImageRgba8)
        }
        _ => img,
    }
}

/// Whether showing a colour picture through `icc` changes it, so a caller
/// with many frames can skip profiles that do nothing.
pub fn changes_rgb(icc: &[u8]) -> bool {
    let Ok(profile) = ColorProfile::new_from_slice(icc) else { return false };
    if profile.color_space != DataColorSpace::Rgb {
        return false;
    }
    let Ok(transform) = profile.create_transform_8bit(Layout::Rgba, &ColorProfile::new_srgb(), Layout::Rgba, TransformOptions::default()) else {
        return false;
    };
    let mut probe = [0u8; PROBE.len() * 4];
    transform.transform(PROBE.as_flattened(), &mut probe).is_ok() && !unchanged(PROBE.as_flattened(), &probe)
}

fn is_float(img: &DynamicImage) -> bool {
    matches!(img, DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_))
}

/// No sample moved by more than rounding.
fn unchanged(a: &[u8], b: &[u8]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 1)
}

/// Runs `transform` over `rgba` in place, a band of rows per thread.
fn convert(rgba: &mut RgbaImage, transform: &(dyn TransformExecutor<u8> + Send + Sync)) -> bool {
    let row = rgba.width() as usize * 4;
    if row == 0 {
        return true;
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let band = (rgba.height() as usize).div_ceil(threads).max(1) * row;
    std::thread::scope(|s| {
        let workers: Vec<_> = rgba
            .chunks_mut(band)
            .map(|pixels| {
                s.spawn(move || {
                    let source = pixels.to_vec();
                    transform.transform(&source, pixels).is_ok()
                })
            })
            .collect();
        workers.into_iter().all(|w| w.join().unwrap_or(false))
    })
}

fn gray_to_srgb(gray: &GrayAlphaImage, transform: &(dyn TransformExecutor<u8> + Send + Sync)) -> Option<RgbaImage> {
    let mut rgba = vec![0u8; gray.width() as usize * gray.height() as usize * 4];
    transform.transform(gray.as_raw(), &mut rgba).ok()?;
    RgbaImage::from_raw(gray.width(), gray.height(), rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgba};

    fn pixel(img: &DynamicImage) -> [u8; 4] {
        img.to_rgba8().get_pixel(0, 0).0
    }

    fn solid(c: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 5, Rgba(c)))
    }

    #[test]
    fn a_wider_profile_moves_colours_as_srgb_would_show_them() {
        let icc = ColorProfile::new_adobe_rgb().encode().unwrap();
        let [r, g, b, a] = pixel(&to_srgb(solid([128, 200, 128, 200]), &icc));
        assert!(r < 128 && g > 200, "wider gamut pushes green out: {r} {g} {b}");
        assert_eq!(a, 200);
    }

    #[test]
    fn display_p3_lands_on_its_srgb_value() {
        let icc = ColorProfile::new_display_p3().encode().unwrap();
        let [r, g, b, _] = pixel(&to_srgb(solid([200, 100, 50, 255]), &icc));
        // Display P3 (200, 100, 50) through ColorSync, from `sips -m`: (215, 93, 31).
        assert!(r.abs_diff(215) <= 2 && g.abs_diff(93) <= 2 && b.abs_diff(31) <= 3, "{r} {g} {b}");
    }

    #[test]
    fn every_row_is_converted_whatever_the_thread_count() {
        let icc = ColorProfile::new_display_p3().encode().unwrap();
        let out = to_srgb(DynamicImage::ImageRgba8(RgbaImage::from_pixel(7, 301, Rgba([200, 100, 50, 255]))), &icc).to_rgba8();
        let first = *out.get_pixel(0, 0);
        assert_ne!(first.0[0], 200);
        assert!(out.pixels().all(|p| *p == first));
    }

    #[test]
    fn srgb_keeps_the_picture_as_it_is() {
        let icc = ColorProfile::new_srgb().encode().unwrap();
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([200, 100, 50])));
        let out = to_srgb(img, &icc);
        assert!(matches!(out, DynamicImage::ImageRgb8(_)), "untouched, not rebuilt");
        assert_eq!(pixel(&out), [200, 100, 50, 255]);
    }

    #[test]
    fn only_a_wider_colour_profile_counts_as_a_change() {
        assert!(changes_rgb(&ColorProfile::new_display_p3().encode().unwrap()));
        assert!(!changes_rgb(&ColorProfile::new_srgb().encode().unwrap()));
        assert!(!changes_rgb(&ColorProfile::new_gray_with_gamma(1.0).encode().unwrap()));
        assert!(!changes_rgb(b"not a profile"));
    }

    #[test]
    fn an_unreadable_profile_changes_nothing() {
        assert_eq!(pixel(&to_srgb(solid([200, 100, 50, 255]), b"not a profile")), [200, 100, 50, 255]);
    }

    #[test]
    fn a_profile_for_other_colours_changes_nothing() {
        let icc = ColorProfile::new_gray_with_gamma(1.0).encode().unwrap();
        assert_eq!(pixel(&to_srgb(solid([200, 100, 50, 255]), &icc)), [200, 100, 50, 255]);
    }

    #[test]
    fn a_linear_gray_profile_brightens_midtones() {
        let icc = ColorProfile::new_gray_with_gamma(1.0).encode().unwrap();
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(2, 2, Luma([64])));
        let [r, g, b, a] = pixel(&to_srgb(img, &icc));
        assert!(r > 100 && r == g && g == b, "{r} {g} {b}");
        assert_eq!(a, 255);
    }
}
