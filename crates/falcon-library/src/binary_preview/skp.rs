use std::fs::File;
use std::path::Path;

use image::DynamicImage;

pub(super) fn read(_file: File, _path: &Path) -> Option<DynamicImage> {
    None
}
