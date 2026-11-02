//! The preview picture inside the zip containers of Sketch, XMind, Keynote
//! and IDML files (and SketchUp files saved as a zip). Used at import when QuickLook has no render (no generator
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
    !entries(ext).is_empty() || ext == "idml"
}

/// More entries to try after [`entries`], found by their names in the
/// archive: a Keynote package's own folder prefix, its `Data/st-*` slide
/// pictures, and for IDML and zipped SketchUp files any `thumbnail*` or
/// `preview*` picture. At most a few are returned.
fn found(names: &[String], ext: &str) -> Vec<String> {
    let lower = |n: &String| n.to_ascii_lowercase();
    let picture = |n: &str| n.ends_with(".png") || n.ends_with(".jpg") || n.ends_with(".jpeg");
    let mut out: Vec<String> = match ext {
        "key" => {
            let tails = ["preview.jpg", "preview.png", "quicklook/thumbnail.jpg", "quicklook/thumbnail.png"];
            let mut v: Vec<&String> = names
                .iter()
                .filter(|n| {
                    let l = lower(n);
                    let tail = l.split_once('/').map(|(first, rest)| (first.ends_with(".key"), rest));
                    tails.contains(&l.as_str()) || tail.is_some_and(|(pkg, rest)| pkg && tails.contains(&rest))
                })
                .collect();
            let mut slides: Vec<&String> = names
                .iter()
                .filter(|n| {
                    let l = lower(n);
                    let name = l.rsplit('/').next().unwrap_or("");
                    l.contains("data/") && name.starts_with("st-") && picture(name)
                })
                .collect();
            slides.sort();
            v.extend(slides);
            v.into_iter().cloned().collect()
        }
        "idml" | "skp" => names
            .iter()
            .filter(|n| {
                let l = lower(n);
                let name = l.rsplit('/').next().unwrap_or("");
                !l.ends_with('/') && (name.starts_with("thumbnail") || name.starts_with("preview")) && picture(name)
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    out.truncate(8);
    out
}

/// The preview picture of the file at `path`; `None` when it isn't a zip,
/// has no such entry, or the entry is too big or not a picture.
pub fn preview(path: &Path, ext: &str) -> Option<DynamicImage> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let names: Vec<String> = zip.file_names().filter_map(Result::ok).map(std::borrow::Cow::into_owned).collect();
    let tried: Vec<String> = entries(ext).iter().map(|n| (*n).to_owned()).chain(found(&names, ext)).collect();
    for name in &tried {
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

pub(crate) fn decode(bytes: &[u8]) -> Option<DynamicImage> {
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
    fn keynote_packages_and_idml_previews_are_found_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = make_zip(dir.path(), "pkg.key", &[("Presentation.key/Index.zip", b"x"), ("Presentation.key/preview.jpg", &jpeg(14, 10))]);
        assert_eq!(preview(&p, "key").unwrap().dimensions(), (14, 10));
        let p = make_zip(dir.path(), "slides.key", &[("Data/st-b-2.jpg", &jpeg(9, 9)), ("Data/st-a-1.jpg", &jpeg(11, 7)), ("Data/other.jpg", &jpeg(5, 5))]);
        assert_eq!(preview(&p, "key").unwrap().dimensions(), (11, 7), "the first slide picture by name");
        let p = make_zip(dir.path(), "none.key", &[("Presentation.key/Index.zip", b"x"), ("Data/image.jpg", &jpeg(5, 5))]);
        assert!(preview(&p, "key").is_none());
        let p = make_zip(dir.path(), "a.idml", &[("designmap.xml", b"<x/>"), ("META-INF/Thumbnail.png", &png(33, 22))]);
        assert_eq!(preview(&p, "idml").unwrap().dimensions(), (33, 22));
        let p = make_zip(dir.path(), "none.idml", &[("designmap.xml", b"<x/>"), ("Resources/Graphic.xml", b"<x/>")]);
        assert!(preview(&p, "idml").is_none());
        let p = make_zip(dir.path(), "bad.idml", &[("preview.png", b"junk")]);
        assert!(preview(&p, "idml").is_none());
        assert!(handles("idml"));
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
