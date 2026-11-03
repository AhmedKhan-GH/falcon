//! The embedder interface and CLIP's image preprocessing.

use image::{DynamicImage, RgbImage, imageops::FilterType};

/// The side of the square CLIP looks at.
pub const IMAGE_SIZE: u32 = 224;

/// CLIP's channel means and deviations, from the model's
/// `preprocessor_config.json`.
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];

/// Turns pictures and text into unit-length vectors in one shared space.
/// The worker owns one and calls it from its own thread only.
pub trait Embedder: Send {
    /// One vector per picture, in order.
    fn embed_images(&mut self, images: &[DynamicImage]) -> Result<Vec<Vec<f32>>, String>;
    fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String>;
    /// The length of the vectors.
    fn dim(&self) -> usize;
    /// What the model runs on, for Preferences ("Metal" or "CPU").
    fn device(&self) -> String;
}

/// A picture as CLIP wants it: transparency flattened onto white, the short
/// side scaled to [`IMAGE_SIZE`] with a bicubic filter, the middle square
/// cut out, then channels-first floats normalised per channel.
pub fn pixel_values(image: &DynamicImage) -> Vec<f32> {
    let rgb = center_square(&flatten(image));
    let side = IMAGE_SIZE as usize;
    let mut out = vec![0.0; 3 * side * side];
    for (x, y, p) in rgb.enumerate_pixels() {
        for c in 0..3 {
            out[c * side * side + y as usize * side + x as usize] = (f32::from(p[c]) / 255.0 - MEAN[c]) / STD[c];
        }
    }
    out
}

/// Flattens transparency onto white: the colour under a transparent pixel is
/// often black or left over from editing and means nothing.
fn flatten(image: &DynamicImage) -> RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y);
        let a = u32::from(p[3]);
        let mix = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

fn center_square(rgb: &RgbImage) -> RgbImage {
    let (w, h) = (rgb.width().max(1), rgb.height().max(1));
    let scale = f64::from(IMAGE_SIZE) / f64::from(w.min(h));
    let nw = ((f64::from(w) * scale).round() as u32).max(IMAGE_SIZE);
    let nh = ((f64::from(h) * scale).round() as u32).max(IMAGE_SIZE);
    let scaled = image::imageops::resize(rgb, nw, nh, FilterType::CatmullRom);
    image::imageops::crop_imm(&scaled, (nw - IMAGE_SIZE) / 2, (nh - IMAGE_SIZE) / 2, IMAGE_SIZE, IMAGE_SIZE).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn pixel_values_are_channels_first_and_normalised() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(10, 20, image::Rgb([255, 0, 0])));
        let v = pixel_values(&img);
        assert_eq!(v.len(), 3 * 224 * 224);
        let side = 224 * 224;
        assert!((v[0] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-3, "red channel comes first");
        assert!((v[side] - (0.0 - MEAN[1]) / STD[1]).abs() < 1e-3);
        assert!((v[2 * side] - (0.0 - MEAN[2]) / STD[2]).abs() < 1e-3);
    }

    #[test]
    fn the_middle_of_a_wide_picture_is_kept() {
        let mut img = RgbImage::from_pixel(400, 100, image::Rgb([0, 0, 0]));
        for x in 150..250 {
            for y in 0..100 {
                img.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        let v = pixel_values(&DynamicImage::ImageRgb8(img));
        // The crop is exactly the white square.
        assert!((v[112 * 224 + 112] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-3);
        assert!((v[112 * 224 + 3] - (1.0 - MEAN[0]) / STD[0]).abs() < 0.1);
    }

    #[test]
    fn transparency_is_flattened_onto_white() {
        let img = image::RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        let v = pixel_values(&DynamicImage::ImageRgba8(img));
        assert!((v[0] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-3);
    }

    #[test]
    fn tiny_pictures_still_give_a_full_square() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 3000, image::Rgb([9, 9, 9])));
        assert_eq!(pixel_values(&img).len(), 3 * 224 * 224);
    }
}
