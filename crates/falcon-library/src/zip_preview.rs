//! The preview picture inside the zip containers of Sketch, XMind, Keynote
//! and IDML files (and SketchUp files saved as a zip). Used at import when
//! QuickLook has no render (no generator for the app is installed).
//!
//! The archive is untrusted. Entries are only matched by name, never used as
//! paths. At most a few entries are read, each at most [`MAX_ENTRY`] bytes,
//! and anything malformed means "no preview".

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

/// The most extra entries [`found`] returns.
const MAX_FOUND: usize = 8;
/// The IDML entry that carries an XMP thumbnail.
const IDML_METADATA: &str = "META-INF/metadata.xml";

fn is_picture(name: &str) -> bool {
    name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg")
}

/// More entries to try after [`entries`], found by their names in the
/// archive. For Keynote: a preview or QuickLook thumbnail under the
/// package's own `<name>.key/` folder, then `Data/st-*` slide pictures
/// (also under that folder), in name order. For IDML and zipped SketchUp
/// files: any `thumbnail*` or `preview*` picture. At most [`MAX_FOUND`]
/// are returned, none of them already in [`entries`].
fn found(names: &[String], ext: &str) -> Vec<String> {
    let mut out: Vec<String> = match ext {
        "key" => {
            const TAILS: [&str; 4] = ["preview.jpg", "preview.png", "quicklook/thumbnail.jpg", "quicklook/thumbnail.png"];
            // The part of a name below an optional `<name>.key/` folder.
            let below = |l: &str| -> String {
                match l.split_once('/') {
                    Some((first, rest)) if first.ends_with(".key") => rest.to_owned(),
                    _ => l.to_owned(),
                }
            };
            let mut previews: Vec<&String> = Vec::new();
            let mut slides: Vec<&String> = Vec::new();
            for n in names {
                let l = n.to_ascii_lowercase();
                let rest = below(&l);
                if TAILS.contains(&rest.as_str()) {
                    previews.push(n);
                } else if let Some(file) = rest.strip_prefix("data/")
                    && file.starts_with("st-")
                    && !file.contains('/')
                    && is_picture(file)
                {
                    slides.push(n);
                }
            }
            slides.sort();
            previews.into_iter().chain(slides).cloned().collect()
        }
        "idml" | "skp" => names
            .iter()
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                let name = l.rsplit('/').next().unwrap_or("");
                (name.starts_with("thumbnail") || name.starts_with("preview")) && is_picture(name)
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    out.retain(|n| !entries(ext).contains(&n.as_str()));
    out.dedup();
    out.truncate(MAX_FOUND);
    out
}

/// The bytes of the file entry `name`; `None` when it is absent, a
/// directory, or over [`MAX_ENTRY`].
fn read_entry<R: std::io::Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let entry = zip.by_name(name).ok()?;
    if entry.is_dir() || entry.size() > MAX_ENTRY {
        return None;
    }
    let mut bytes = Vec::new();
    // `take` bounds the read even when the header lies about the size.
    entry.take(MAX_ENTRY + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_ENTRY).then_some(bytes)
}

/// The picture in the `xmpGImg:image` element of an IDML `metadata.xml`: a
/// base64 body that may be split by `&#xA;` entities. `None` when the element
/// is missing or the body is not a picture.
fn xmp_thumbnail(xml: &[u8]) -> Option<DynamicImage> {
    use base64::Engine;
    const OPEN: &str = "<xmpGImg:image>";
    const CLOSE: &str = "</xmpGImg:image>";
    let text = std::str::from_utf8(xml).ok()?;
    let start = text.find(OPEN)?.checked_add(OPEN.len())?;
    let body = text.get(start..)?;
    let body = body.get(..body.find(CLOSE)?)?;
    let cleaned: String = body.replace("&#xA;", "").replace("&#xD;", "").chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD.decode(cleaned.as_bytes()).ok()?;
    decode(&bytes)
}

/// The preview picture of the file at `path`; `None` when it isn't a zip,
/// has no such entry, or the entry is too big or not a picture.
pub fn preview(path: &Path, ext: &str) -> Option<DynamicImage> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let names: Vec<String> = zip.file_names().filter_map(Result::ok).map(std::borrow::Cow::into_owned).collect();
    let tried: Vec<String> = entries(ext).iter().map(|n| (*n).to_owned()).chain(found(&names, ext)).collect();
    for name in &tried {
        if let Some(img) = read_entry(&mut zip, name).and_then(|bytes| decode(&bytes)) {
            return Some(img);
        }
    }
    if ext == "idml" {
        return read_entry(&mut zip, IDML_METADATA).and_then(|xml| xmp_thumbnail(&xml));
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

    fn xmp(body: &str) -> Vec<u8> {
        format!("<?xml version=\"1.0\"?><x:xmpmeta><xmpGImg:format>JPEG</xmpGImg:format><xmpGImg:image>{body}</xmpGImg:image></x:xmpmeta>").into_bytes()
    }

    fn b64_split(bytes: &[u8]) -> String {
        use base64::Engine;
        let s = base64::engine::general_purpose::STANDARD.encode(bytes);
        s.as_bytes().chunks(76).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("&#xA;")
    }

    #[test]
    fn idml_xmp_thumbnail_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let body = b64_split(&jpeg(40, 30));
        assert!(body.contains("&#xA;"));
        let m = xmp(&body);
        let p = make_zip(dir.path(), "x.idml", &[("designmap.xml", b"<x/>"), ("META-INF/metadata.xml", &m)]);
        assert_eq!(preview(&p, "idml").unwrap().dimensions(), (40, 30));
        // Only IDML looks there.
        assert!(preview(&p, "skp").is_none());
        // A named picture wins over the metadata.
        let p = make_zip(dir.path(), "y.idml", &[("META-INF/metadata.xml", &m), ("META-INF/Thumbnail.png", &png(7, 7))]);
        assert_eq!(preview(&p, "idml").unwrap().dimensions(), (7, 7));
    }

    #[test]
    fn idml_malformed_metadata_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let good = b64_split(&jpeg(40, 30));
        let cases: Vec<Vec<u8>> = vec![
            xmp("!!!! not base64 ????"),
            xmp("QUJD"),
            xmp(""),
            // A cut entity, and a lone ampersand.
            xmp(&good.replacen("&#xA;", "&#x", 1)),
            xmp(&good.replacen("&#xA;", "&", 1)),
            // No closing tag, no element, not UTF-8.
            format!("<xmpGImg:image>{good}").into_bytes(),
            b"<x:xmpmeta/>".to_vec(),
            vec![0xFF, 0xFE, 0x00, 0x3C],
            // Closing tag before the opening one.
            format!("</xmpGImg:image><xmpGImg:image>{good}").into_bytes(),
            b"<xmpGImg:image>".to_vec(),
        ];
        for (i, m) in cases.iter().enumerate() {
            let p = make_zip(dir.path(), &format!("m{i}.idml"), &[("META-INF/metadata.xml", m)]);
            assert!(preview(&p, "idml").is_none(), "case {i}");
        }
    }

    #[test]
    fn found_matches_whole_path_components() {
        let names: Vec<String> = ["mydata/st-1.jpg", "Data/st-2.jpg", "Data/sub/st-3.jpg", "Data/st-4.txt", "x/preview.jpg", "Pkg.key/Data/st-5.png"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(found(&names, "key"), ["Data/st-2.jpg", "Pkg.key/Data/st-5.png"]);
        let names: Vec<String> = ["Pkg.key/preview.jpg", "preview.jpg", "other.key/x/preview.jpg"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(found(&names, "key"), ["Pkg.key/preview.jpg"], "the top-level one is in entries()");
        let many: Vec<String> = (0..50).map(|i| format!("Data/st-{i:02}.jpg")).collect();
        assert_eq!(found(&many, "key").len(), MAX_FOUND);
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
