//! Converting WebP items to PNG or JPG (Eagle's Convert WebP, offered in the
//! item menu when the item is a WebP).
//!
//! What Eagle does, which falcon follows: animated WebPs are skipped; the
//! picture is decoded and written as PNG, or as JPG at full quality, under
//! the item's name with the new extension; the WebP goes; the item takes the
//! new extension and file size. Its thumbnail stays, except that an image
//! no larger than [`NO_THUMBNAIL_SIZE`] on both sides loses its thumbnail
//! and shows its original instead. Dimensions, palette, tags and everything
//! else stay.
//!
//! falcon differs in three ways. The new file is written next to the old
//! one under a temporary name, flushed, and swapped in only once complete;
//! if saving the item fails, the old file comes back. The old WebP is not
//! deleted but handed to the caller's `discard` (the app moves it to the
//! Trash), so a conversion can be undone by hand. And a transparent WebP
//! converted to JPG is laid on white (Eagle's canvas turns transparency
//! black). The undo step is marked irreversible, as for other edits that
//! rewrite the original.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use image::ImageEncoder;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::thumbnail::NO_THUMBNAIL_SIZE;

/// The formats Eagle converts WebP to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpg,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
        }
    }
}

/// What converting one item did.
#[derive(Debug, Clone, PartialEq)]
pub enum Converted {
    /// Converted; the old WebP went to `discard`.
    Done,
    /// Converted, but `discard` failed, so the old WebP was left at this
    /// (hidden) path in the item's folder.
    KeptOld(PathBuf),
    /// An animated WebP, left alone.
    Animated,
    /// Not a WebP item.
    NotWebp,
}

/// Whether WebP bytes hold an animation (an `ANMF` frame chunk), as Eagle
/// tests it.
pub fn is_animated(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|w| w == b"ANMF")
}

/// Encodes `img` in `format`: PNG as decoded, JPG at quality 100 with any
/// transparency laid on white.
pub fn encode(img: &image::DynamicImage, format: Format) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::new();
    match format {
        Format::Png => {
            img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        }
        Format::Jpg => {
            let rgba = img.to_rgba8();
            let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
            for (d, s) in rgb.pixels_mut().zip(rgba.pixels()) {
                let a = s[3] as u32;
                for c in 0..3 {
                    d[c] = ((s[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                }
            }
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 100)
                .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(out)
}

impl Library {
    /// Converts WebP item `id` to `format` (see the module docs). `discard`
    /// receives the old WebP once the item is saved.
    pub fn convert_webp(&mut self, id: &str, format: Format, discard: impl FnOnce(&Path) -> std::io::Result<()>) -> Result<Converted> {
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        if !item.ext.eq_ignore_ascii_case("webp") {
            return Ok(Converted::NotWebp);
        }
        let old = item.original_path(self.root());
        let bytes = fs::read(&old).map_err(|e| Error::io(&old, e))?;
        if is_animated(&bytes) {
            return Ok(Converted::Animated);
        }
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP).map_err(|e| self.invalid(format!("can't read the WebP: {e}")))?;
        let encoded = encode(&img, format).map_err(|e| self.invalid(format!("can't write the {}: {e}", format.ext())))?;
        let dir = item.info_dir(self.root());
        let name = crate::disk_name::on_disk(&item.name);
        let new = dir.join(&*crate::disk_name::on_disk(&format!("{name}.{}", format.ext())));
        if new.exists() {
            return Err(self.invalid(format!("{} already exists", new.display())));
        }
        let staged = dir.join(format!(".{name}.{}.falcon-convert", format.ext()));
        let kept = dir.join(format!(".{name}.webp.falcon-old"));
        let written = fs::File::create(&staged).and_then(|mut f| f.write_all(&encoded).and_then(|()| f.sync_all()));
        if let Err(e) = written {
            let _ = fs::remove_file(&staged);
            return Err(Error::io(&staged, e));
        }
        if let Err(e) = fs::rename(&old, &kept) {
            let _ = fs::remove_file(&staged);
            return Err(Error::io(&old, e));
        }
        if let Err(e) = fs::rename(&staged, &new) {
            let _ = fs::rename(&kept, &old);
            let _ = fs::remove_file(&staged);
            return Err(Error::io(&new, e));
        }
        let thumb = item.thumbnail_path(self.root());
        let small = img.width() <= NO_THUMBNAIL_SIZE && img.height() <= NO_THUMBNAIL_SIZE;
        item.ext = format.ext().to_string();
        item.size = encoded.len() as u64;
        if small {
            item.no_thumbnail = Some(true);
            item.remove_thumbnail = None;
        }
        if let Err(e) = self.update_item(item) {
            let _ = fs::remove_file(&new);
            let _ = fs::rename(&kept, &old);
            return Err(e);
        }
        if let Some(j) = &mut self.journal {
            j.irreversible = true;
        }
        // Eagle drops a small image's thumbnail; the original stands in.
        if small && let Some(t) = thumb.filter(|t| t.file_name().is_some_and(|n| n.to_string_lossy().ends_with("_thumbnail.png"))) {
            let _ = fs::remove_file(t);
        }
        Ok(match discard(&kept) {
            Ok(()) => Converted::Done,
            Err(_) => Converted::KeptOld(kept),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn webp(dir: &Path, name: &str, w: u32, h: u32, alpha: u8) -> PathBuf {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 10, 10, alpha]));
        let enc = webp::Encoder::from_rgba(img.as_raw(), w, h).encode_lossless();
        let p = dir.join(name);
        fs::write(&p, &*enc).unwrap();
        p
    }

    fn setup(w: u32, h: u32, alpha: u8) -> (tempfile::TempDir, Library, String) {
        let d = tempfile::tempdir().unwrap();
        let mut lib = Library::create(d.path().join("L.library")).unwrap();
        let src = webp(d.path(), "pic.webp", w, h, alpha);
        let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
        (d, lib, id)
    }

    #[test]
    fn converts_to_png_and_hands_over_the_old_file() {
        let (_d, mut lib, id) = setup(40, 30, 255);
        let mut handed = None;
        let got = lib.convert_webp(&id, Format::Png, |p| {
            handed = Some(p.to_path_buf());
            fs::remove_file(p)
        });
        assert_eq!(got.unwrap(), Converted::Done);
        let item = lib.item(&id).unwrap().clone();
        assert_eq!(item.ext, "png");
        let path = item.original_path(lib.root());
        assert_eq!(item.size, fs::metadata(&path).unwrap().len());
        assert_eq!(image::open(&path).unwrap().width(), 40);
        assert_eq!(item.width, 40);
        assert!(handed.unwrap().to_string_lossy().ends_with(".pic.webp.falcon-old"));
        // Only the new original and metadata.json are left (small: no thumbnail).
        let mut names: Vec<String> = fs::read_dir(item.info_dir(lib.root())).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["metadata.json", "pic.png"]);
        assert_eq!(item.no_thumbnail, Some(true));
        // Saved to disk.
        let reopened = Library::open(lib.root()).unwrap();
        assert_eq!(reopened.item(&id).unwrap().ext, "png");
    }

    #[test]
    fn jpg_lays_transparency_on_white() {
        let (_d, mut lib, id) = setup(8, 8, 0);
        lib.convert_webp(&id, Format::Jpg, |p| fs::remove_file(p)).unwrap();
        let item = lib.item(&id).unwrap().clone();
        assert_eq!(item.ext, "jpg");
        let px = image::open(item.original_path(lib.root())).unwrap().to_rgb8().get_pixel(4, 4).0;
        assert!(px.iter().all(|&c| c > 245), "{px:?}");
    }

    #[test]
    fn a_failed_discard_keeps_the_old_file() {
        let (_d, mut lib, id) = setup(8, 8, 255);
        let got = lib.convert_webp(&id, Format::Png, |_| Err(std::io::Error::other("no Trash"))).unwrap();
        let Converted::KeptOld(p) = got else { panic!("{got:?}") };
        assert!(p.exists());
    }

    #[test]
    fn a_large_image_keeps_its_thumbnail() {
        let (_d, mut lib, id) = setup(1200, 20, 255);
        let thumb = lib.item(&id).unwrap().thumbnail_path(lib.root()).unwrap();
        assert!(thumb.exists());
        lib.convert_webp(&id, Format::Png, |p| fs::remove_file(p)).unwrap();
        assert!(thumb.exists());
        assert_ne!(lib.item(&id).unwrap().no_thumbnail, Some(true));
    }

    #[test]
    fn skips_animations_other_types_and_existing_names() {
        let (d, mut lib, id) = setup(8, 8, 255);
        let item = lib.item(&id).unwrap().clone();
        fs::write(item.info_dir(lib.root()).join("pic.png"), b"x").unwrap();
        assert!(lib.convert_webp(&id, Format::Png, |p| fs::remove_file(p)).is_err());
        assert_eq!(lib.item(&id).unwrap().ext, "webp");
        assert!(item.original_path(lib.root()).exists());
        let txt = d.path().join("a.txt");
        fs::write(&txt, b"hi").unwrap();
        let tid = lib.import_file(&txt, &ImportOptions::default()).unwrap();
        assert_eq!(lib.convert_webp(&tid, Format::Png, |p| fs::remove_file(p)).unwrap(), Converted::NotWebp);
        assert!(is_animated(b"RIFF....WEBPVP8XANIM....ANMF"));
        assert!(!is_animated(b"RIFF....WEBPVP8 "));
    }

    #[test]
    fn the_undo_step_is_irreversible() {
        let (_d, mut lib, id) = setup(8, 8, 255);
        lib.begin_transaction("Convert");
        lib.convert_webp(&id, Format::Png, |p| fs::remove_file(p)).unwrap();
        assert!(lib.end_transaction().unwrap().irreversible);
    }
}
