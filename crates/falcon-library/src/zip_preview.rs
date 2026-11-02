//! The preview picture inside the zip containers of Sketch, XMind and
//! Keynote files. Used at import when QuickLook has no render (no generator
//! for the app is installed). The archive is untrusted: only one named entry
//! is read, at most [`MAX_ENTRY`] bytes of it, and anything malformed means
//! "no preview".

use std::io::Read;
use std::path::Path;

use image::DynamicImage;

/// The most bytes read from one entry, whatever the zip header claims.
pub const MAX_ENTRY: u64 = 16 << 20;
/// The largest preview picture accepted, per side.
const MAX_SIDE: u32 = 16_384;
/// The most memory the picture decoder may use.
const MAX_ALLOC: u64 = 256 << 20;

/// The entries that hold a file's preview, best first.
pub fn entries(ext: &str) -> &'static [&'static str] {
    match ext {
        "sketch" => &["previews/preview.png"],
        "xmind" => &["Thumbnails/thumbnail.png"],
        "key" => &["preview.jpg", "QuickLook/Thumbnail.jpg"],
        _ => &[],
    }
}

pub fn handles(ext: &str) -> bool {
    !entries(ext).is_empty()
}

/// The preview picture of the file at `path`; `None` when it isn't a zip,
/// has no such entry, or the entry is too big or not a picture.
pub fn preview(path: &Path, ext: &str) -> Option<DynamicImage> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    for name in entries(ext) {
        let Ok(entry) = zip.by_name(name) else { continue };
        if entry.is_dir() || entry.size() > MAX_ENTRY {
            continue;
        }
        let mut bytes = Vec::new();
        // `take` bounds the read even when the header lies about the size.
        if entry.take(MAX_ENTRY + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > MAX_ENTRY {
            continue;
        }
        if let Some(img) = decode(&bytes) {
            return Some(img);
        }
    }
    None
}

fn decode(bytes: &[u8]) -> Option<DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = image::ImageDecoder::orientation(&mut decoder).ok()?;
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    img.apply_orientation(orientation);
    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use image::GenericImageView;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([10, 200, 30])))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 20, 30])))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
            .unwrap();
        out
    }

    fn make_zip(dir: &Path, file: &str, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = dir.join(file);
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, data) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        path
    }

    #[test]
    fn sketch_xmind_and_key_previews_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_zip(dir.path(), "a.sketch", &[("document.json", b"{}"), ("previews/preview.png", &png(30, 20))]);
        assert_eq!(preview(&p, "sketch").unwrap().dimensions(), (30, 20));
        let p = make_zip(dir.path(), "a.xmind", &[("Thumbnails/thumbnail.png", &png(8, 9))]);
        assert_eq!(preview(&p, "xmind").unwrap().dimensions(), (8, 9));
        let p = make_zip(dir.path(), "new.key", &[("preview.jpg", &jpeg(16, 12))]);
        assert_eq!(preview(&p, "key").unwrap().dimensions(), (16, 12));
        let p = make_zip(dir.path(), "old.key", &[("QuickLook/Thumbnail.jpg", &jpeg(12, 16))]);
        assert_eq!(preview(&p, "key").unwrap().dimensions(), (12, 16));
        // The wrong format's entry name is not used.
        assert!(preview(&p, "sketch").is_none());
    }

    #[test]
    fn missing_and_malformed_previews_give_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_zip(dir.path(), "none.sketch", &[("document.json", b"{}")]);
        assert!(preview(&p, "sketch").is_none());
        let p = make_zip(dir.path(), "junk.sketch", &[("previews/preview.png", b"not a picture")]);
        assert!(preview(&p, "sketch").is_none());
        let p = make_zip(dir.path(), "dir.sketch", &[("previews/preview.png/", b"")]);
        assert!(preview(&p, "sketch").is_none());
        // A truncated picture.
        let full = png(64, 64);
        let p = make_zip(dir.path(), "cut.sketch", &[("previews/preview.png", &full[..full.len() / 2])]);
        assert!(preview(&p, "sketch").is_none());
        // Not a zip at all, empty, a missing file, and a cut-off zip.
        let junk = dir.path().join("junk2.xmind");
        std::fs::write(&junk, b"PK\x03\x04 and then garbage").unwrap();
        assert!(preview(&junk, "xmind").is_none());
        std::fs::write(&junk, b"").unwrap();
        assert!(preview(&junk, "xmind").is_none());
        assert!(preview(&dir.path().join("absent.key"), "key").is_none());
        let good = make_zip(dir.path(), "g.key", &[("preview.jpg", &jpeg(16, 16))]);
        let bytes = std::fs::read(&good).unwrap();
        std::fs::write(dir.path().join("cutzip.key"), &bytes[..bytes.len() - 10]).unwrap();
        assert!(preview(&dir.path().join("cutzip.key"), "key").is_none());
    }

    #[test]
    fn hostile_zips_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        // An entry over the cap, stored so that the file stays small only if
        // it compresses: zeros deflate to almost nothing.
        let big = vec![0u8; (MAX_ENTRY + 1024) as usize];
        let p = make_zip(dir.path(), "big.sketch", &[("previews/preview.png", &big)]);
        assert!(std::fs::metadata(&p).unwrap().len() < 1 << 20);
        assert!(preview(&p, "sketch").is_none());
        // A picture whose header claims a huge canvas is refused by the
        // decoder's limits, not allocated.
        let mut huge = png(4, 4);
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        let p = make_zip(dir.path(), "huge.sketch", &[("previews/preview.png", &huge)]);
        assert!(preview(&p, "sketch").is_none());
        // An end-of-directory record that claims 65535 entries.
        let mut eocd = b"PK\x05\x06\0\0\0\0".to_vec();
        eocd.extend([0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]);
        let p = dir.path().join("many.xmind");
        std::fs::write(&p, eocd).unwrap();
        assert!(preview(&p, "xmind").is_none());
    }
}
