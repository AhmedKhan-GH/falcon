//! The preview picture of an InDesign document or template.

use std::fs::File;
use std::io::BufReader;
use std::ops::ControlFlow;
use std::path::Path;

use base64::Engine;
use image::DynamicImage;

use super::{decode, scan};

const OPEN: &[u8] = b"<xmpGImg:image>";
const CLOSE: &[u8] = b"</xmpGImg:image>";

/// The base64 JPEG in the file's XMP packet (`xmpGImg:image`). The packet is
/// written again on each save, so the file can hold several; the largest
/// picture wins. Each one is decoded as it is found and only the best is kept.
pub(super) fn read(file: File, _path: &Path) -> Option<DynamicImage> {
    let mut best: Option<DynamicImage> = None;
    scan(BufReader::new(file), OPEN, CLOSE, |hit| {
        if let Some(img) = xmp_image(hit)
            && best.as_ref().is_none_or(|b| area(&img) > area(b))
        {
            best = Some(img);
        }
        ControlFlow::Continue(())
    });
    best
}

fn area(img: &DynamicImage) -> u64 {
    u64::from(img.width()) * u64::from(img.height())
}

/// A picture from a tag stretch (markers included) whose text is base64 with
/// numeric character references such as `&#xA;` between the lines.
fn xmp_image(stretch: &[u8]) -> Option<DynamicImage> {
    let text = stretch.strip_prefix(OPEN)?.strip_suffix(CLOSE)?;
    let clean = strip_entities(text)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(clean).ok()?;
    decode(&bytes)
}

/// `text` without whitespace and without numeric character references
/// (`&#xA;`, `&#10;`). None when a `&` does not start a complete one.
fn strip_entities(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'&' {
            let end = tail.iter().take(9).position(|&c| c == b';')?;
            let body = tail.get(..end)?.strip_prefix(b"#")?;
            let ok = match body.strip_prefix(b"x").or_else(|| body.strip_prefix(b"X")) {
                Some(h) => !h.is_empty() && h.iter().all(u8::is_ascii_hexdigit),
                None => !body.is_empty() && body.iter().all(u8::is_ascii_digit),
            };
            if !ok {
                return None;
            }
            rest = tail.get(end + 1..)?;
        } else {
            if !b.is_ascii_whitespace() {
                out.push(b);
            }
            rest = tail;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary_preview::MAX_ITEM;

    use image::GenericImageView;

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 20, 30])))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
            .unwrap();
        out
    }

    fn tag(b64: &str) -> String {
        let wrapped: String = b64.as_bytes().chunks(76).map(|c| format!("{}&#xA;", std::str::from_utf8(c).unwrap())).collect();
        format!("<xmpGImg:image>{wrapped}</xmpGImg:image>")
    }

    fn indd_with(images: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![7u8; 5000];
        for img in images {
            out.extend(tag(&base64::engine::general_purpose::STANDARD.encode(img)).as_bytes());
            out.extend(vec![0u8; 3000]);
        }
        out
    }

    fn size(dir: &Path, name: &str, bytes: &[u8]) -> Option<(u32, u32)> {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        read(File::open(&p).unwrap(), &p).map(|i| i.dimensions())
    }

    #[test]
    fn takes_the_largest_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(size(dir.path(), "a.indd", &indd_with(&[jpeg(32, 32), jpeg(64, 48)])), Some((64, 48)));
        assert_eq!(size(dir.path(), "b.indd", &indd_with(&[jpeg(64, 48), jpeg(32, 32)])), Some((64, 48)));
    }

    #[test]
    fn template_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(size(dir.path(), "a.indt", &indd_with(&[jpeg(20, 10)])), Some((20, 10)));
    }

    #[test]
    fn bad_thumbnail_then_good_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = indd_with(&[]);
        f.extend(b"<xmpGImg:image>AAAA</xmpGImg:image><xmpGImg:image>!!</xmpGImg:image>");
        f.extend(indd_with(&[jpeg(24, 16)]));
        assert_eq!(size(dir.path(), "a.indd", &f), Some((24, 16)));
    }

    #[test]
    fn malformed_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let cut = indd_with(&[jpeg(40, 40)]);
        let cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"<xmpGImg:image>".to_vec(),
            b"<xmpGImg:image>not base64 !!</xmpGImg:image>".to_vec(),
            b"<xmpGImg:image>AAAA</xmpGImg:image>".to_vec(),
            b"<xmpGImg:image>QUJD&#xA</xmpGImg:image>".to_vec(),
            b"<xmpGImg:image>QUJD&#x</xmpGImg:image>".to_vec(),
            b"<xmpGImg:image>QUJD&amp;</xmpGImg:image>".to_vec(),
            b"<xmpGImg:image>QUJD&".to_vec(),
            cut[..5200].to_vec(),
        ];
        for (i, b) in cases.iter().enumerate() {
            assert_eq!(size(dir.path(), &format!("m{i}.indd"), b), None, "case {i}");
        }
    }

    #[test]
    fn endless_body_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let mut long = b"<xmpGImg:image>".to_vec();
        long.resize(MAX_ITEM + (3 << 20), b'A');
        assert_eq!(size(dir.path(), "long.indd", &long), None);
        // A good thumbnail after it is still found.
        long.extend(indd_with(&[jpeg(8, 8)]));
        assert_eq!(size(dir.path(), "long2.indd", &long), Some((8, 8)));
    }

    #[test]
    fn entity_stripper() {
        assert_eq!(strip_entities(b"ab&#xA;c d&#10;\n&#X0d;e").unwrap(), b"abcde");
        assert!(strip_entities(b"ab&#xA").is_none());
        assert!(strip_entities(b"ab&").is_none());
        assert!(strip_entities(b"&#;").is_none());
        assert!(strip_entities(b"&#xZ;").is_none());
        assert!(strip_entities(b"&lt;").is_none());
        assert!(strip_entities(b"&#x0000000000;").is_none());
        assert_eq!(strip_entities(b"").unwrap(), b"");
    }
}
