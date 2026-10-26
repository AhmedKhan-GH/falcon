//! Dominant-colour palettes, matching Eagle's `palettes` field.

use image::DynamicImage;

use crate::model::Palette;

/// Extracts the palette Eagle would store for `img`.
pub fn extract(img: &DynamicImage) -> Vec<Palette> {
    let _ = img;
    todo!("MMCQ port in progress")
}
