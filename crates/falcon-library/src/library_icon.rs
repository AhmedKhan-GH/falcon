//! The library's own icon, `icon.png` in the library root. Eagle shows it
//! next to the library name in the library switcher. The caller checks the
//! sandbox (`sandbox::guard`) before writing or deleting it.

use std::io::Write;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;

use crate::{Error, Result};

/// The icon's file name inside the library folder.
pub const ICON_FILE: &str = "icon.png";

/// The length the icon's shorter side is scaled to, in pixels.
const SHORT_SIDE: u32 = 64;

/// The icon file, when the library has one.
pub fn icon_path(root: &Path) -> Option<PathBuf> {
    let p = root.join(ICON_FILE);
    p.is_file().then_some(p)
}

/// Size of an image of `w` × `h` scaled so its shorter side is 64 px.
fn scaled(w: u32, h: u32) -> (u32, u32) {
    let k = f64::from(SHORT_SIDE) / f64::from(w.min(h).max(1));
    let side = |n: u32| ((f64::from(n) * k).round() as u32).max(1);
    (side(w), side(h))
}

/// Makes `source` (jpg, jpeg, png, webp or gif, first frame) the library's
/// icon: scaled uniformly so the shorter side is 64 px, saved as PNG. The
/// file is written next to `icon.png` and renamed over it, so a failure
/// leaves the old icon alone.
pub fn set_icon(root: &Path, source: &Path) -> Result<()> {
    let refuse = |reason: String| Error::Import { path: source.to_path_buf(), reason };
    let img = image::open(source).map_err(|e| refuse(format!("not an image falcon can read ({e})")))?;
    let (w, h) = scaled(img.width(), img.height());
    let small = image::DynamicImage::ImageRgba8(img.resize_exact(w, h, FilterType::Lanczos3).to_rgba8());
    let mut png = std::io::Cursor::new(Vec::new());
    small.write_to(&mut png, image::ImageFormat::Png).map_err(|e| refuse(format!("the icon could not be encoded ({e})")))?;

    let target = root.join(ICON_FILE);
    let mut tmp = tempfile::Builder::new().prefix(".icon-").suffix(".tmp").tempfile_in(root).map_err(|e| Error::io(root, e))?;
    tmp.write_all(png.get_ref()).and_then(|()| tmp.flush()).map_err(|e| Error::io(tmp.path(), e))?;
    tmp.persist(&target).map_err(|e| Error::io(&target, e.error))?;
    Ok(())
}

/// Deletes the icon; nothing happens when there is none.
pub fn remove_icon(root: &Path) -> Result<()> {
    let target = root.join(ICON_FILE);
    match std::fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(target, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let p = dir.join(name);
        image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255])).save(&p).unwrap();
        p
    }

    fn size(p: &Path) -> (u32, u32) {
        image::image_dimensions(p).unwrap()
    }

    #[test]
    fn wide_image_scales_down() {
        let d = tempfile::tempdir().unwrap();
        let src = make(d.path(), "a.png", 200, 100);
        assert_eq!(icon_path(d.path()), None);
        set_icon(d.path(), &src).unwrap();
        assert_eq!(size(&icon_path(d.path()).unwrap()), (128, 64));
    }

    #[test]
    fn small_image_scales_up() {
        let d = tempfile::tempdir().unwrap();
        let src = make(d.path(), "a.png", 32, 48);
        set_icon(d.path(), &src).unwrap();
        assert_eq!(size(&d.path().join(ICON_FILE)), (64, 96));
    }

    #[test]
    fn extreme_ratio_keeps_one_pixel() {
        assert_eq!(scaled(1000, 1), (64000, 64));
        assert_eq!(scaled(1, 1000), (64, 64000));
        assert_eq!(scaled(64, 64), (64, 64));
    }

    #[test]
    fn replaces_the_old_icon_and_leaves_no_temp_file() {
        let d = tempfile::tempdir().unwrap();
        set_icon(d.path(), &make(d.path(), "a.png", 64, 64)).unwrap();
        set_icon(d.path(), &make(d.path(), "b.png", 100, 200)).unwrap();
        assert_eq!(size(&d.path().join(ICON_FILE)), (64, 128));
        let stray = std::fs::read_dir(d.path()).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with(".icon-")).count();
        assert_eq!(stray, 0);
    }

    #[test]
    fn remove_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        remove_icon(d.path()).unwrap();
        set_icon(d.path(), &make(d.path(), "a.png", 10, 10)).unwrap();
        remove_icon(d.path()).unwrap();
        assert_eq!(icon_path(d.path()), None);
        remove_icon(d.path()).unwrap();
    }

    #[test]
    fn a_bad_source_keeps_the_existing_icon() {
        let d = tempfile::tempdir().unwrap();
        set_icon(d.path(), &make(d.path(), "a.png", 64, 64)).unwrap();
        let before = std::fs::read(d.path().join(ICON_FILE)).unwrap();
        let bad = d.path().join("notes.png");
        std::fs::write(&bad, b"this is text").unwrap();
        assert!(set_icon(d.path(), &bad).is_err());
        assert!(set_icon(d.path(), &d.path().join("missing.png")).is_err());
        assert_eq!(std::fs::read(d.path().join(ICON_FILE)).unwrap(), before);
    }
}
