//! Embedded keywords that can become tags on import: IPTC keywords (IIM
//! record 2, dataset 25) and XMP `dc:subject`, read from JPEG, PNG and TIFF.
//! See the Import chapter of the docs for how Eagle does it.

use std::path::Path;

/// Header of the APP1 segment that holds an XMP packet.
const XMP_APP1: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// Keyword of the PNG `iTXt` chunk that holds an XMP packet.
const XMP_PNG: &[u8] = b"XML:com.adobe.xmp";
/// Longest keyword field Eagle accepts, in UTF-16 units.
const MAX_FIELD: usize = 200;

/// The keywords of a file, IPTC first, then XMP, for the formats falcon
/// reads (jpg, jpeg, png, tif, tiff). Each field over 200 characters is
/// dropped, the rest are split on `;`, and duplicates and empty parts are
/// removed.
pub fn read(path: &Path, ext: &str) -> Vec<String> {
    let Ok(data) = std::fs::read(path) else { return Vec::new() };
    let (iptc, xmp) = match crate::alias::canonical(ext).as_str() {
        "jpg" | "jpeg" => jpeg_blocks(&data),
        "png" => (None, png_xmp(&data)),
        "tiff" => tiff_blocks(&data),
        _ => (None, None),
    };
    let mut fields = iptc.map(|b| iim_keywords(&b)).unwrap_or_default();
    fields.extend(xmp.map(|x| xmp_subjects(&String::from_utf8_lossy(&x))).unwrap_or_default());
    split_fields(fields)
}

/// Applies Eagle's field rules: skip fields over 200 characters, split on
/// `;`, drop empty parts and repeats.
pub fn split_fields(fields: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in fields.iter().filter(|f| f.encode_utf16().count() <= MAX_FIELD) {
        for part in f.split(';') {
            if !part.is_empty() && !out.iter().any(|o| o == part) {
                out.push(part.to_string());
            }
        }
    }
    out
}

/// The IPTC block (from Photoshop's APP13 resource 0x0404) and the XMP
/// packet of a JPEG, scanning the segments up to the image data.
pub fn jpeg_blocks(data: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let (mut iptc, mut xmp) = (None, None);
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return (None, None);
    }
    let mut at = 2;
    while at + 4 <= data.len() && data[at] == 0xFF {
        let marker = data[at + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]]));
        let Some(body) = data.get(at + 4..at + 2 + len) else { break };
        if marker == 0xED && iptc.is_none() {
            iptc = photoshop_iptc(body);
        } else if marker == 0xE1 && xmp.is_none() && body.starts_with(XMP_APP1) {
            xmp = Some(body[XMP_APP1.len()..].to_vec());
        }
        at += 2 + len;
    }
    (iptc, xmp)
}

/// Resource 0x0404 (IPTC-NAA) inside a "Photoshop 3.0" image-resource block.
fn photoshop_iptc(body: &[u8]) -> Option<Vec<u8>> {
    let mut at = b"Photoshop 3.0\0".len();
    if !body.starts_with(b"Photoshop 3.0") {
        return None;
    }
    while at + 12 <= body.len() {
        if &body[at..at + 4] != b"8BIM" {
            return None;
        }
        let id = u16::from_be_bytes([body[at + 4], body[at + 5]]);
        // Pascal name, padded to an even length including its length byte.
        let name_len = usize::from(body[at + 6]);
        let name_total = (name_len + 1).div_ceil(2) * 2;
        let size_at = at + 6 + name_total;
        let size = u32::from_be_bytes(body.get(size_at..size_at + 4)?.try_into().ok()?) as usize;
        let start = size_at + 4;
        let block = body.get(start..start.checked_add(size)?)?;
        if id == 0x0404 {
            return Some(block.to_vec());
        }
        at = start + size + (size & 1);
    }
    None
}

/// Keywords (record 2, dataset 25) from an IPTC IIM block.
pub fn iim_keywords(block: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 5 <= block.len() {
        if block[at] != 0x1C {
            at += 1;
            continue;
        }
        let (record, dataset) = (block[at + 1], block[at + 2]);
        let mut len = usize::from(u16::from_be_bytes([block[at + 3], block[at + 4]]));
        let mut start = at + 5;
        if len & 0x8000 != 0 {
            // Extended length: the low bits say how many bytes hold it.
            let n = len & 0x7FFF;
            let Some(bytes) = block.get(start..start + n) else { break };
            len = bytes.iter().fold(0usize, |a, &b| (a << 8) | usize::from(b));
            start += n;
        }
        let Some(value) = block.get(start..start + len) else { break };
        if record == 2 && dataset == 25 {
            out.push(String::from_utf8_lossy(value).into_owned());
        }
        at = start + len;
    }
    out
}

/// The XMP packet of a PNG's `iTXt` chunk.
pub fn png_xmp(data: &[u8]) -> Option<Vec<u8>> {
    if data.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut at = 8;
    while at + 12 <= data.len() {
        let len = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        let kind = &data[at + 4..at + 8];
        let body = data.get(at + 8..at + 8 + len)?;
        if kind == b"iTXt" && body.starts_with(XMP_PNG) && body.get(XMP_PNG.len()) == Some(&0) {
            let rest = &body[XMP_PNG.len() + 1..];
            let (compressed, rest) = (*rest.first()? == 1, rest.get(2..)?);
            // Skip the language tag and the translated keyword.
            let lang_end = rest.iter().position(|&b| b == 0)?;
            let rest = &rest[lang_end + 1..];
            let kw_end = rest.iter().position(|&b| b == 0)?;
            let text = &rest[kw_end + 1..];
            return if compressed { miniz_oxide::inflate::decompress_to_vec_zlib(text).ok() } else { Some(text.to_vec()) };
        }
        if kind == b"IEND" {
            break;
        }
        at += 12 + len;
    }
    None
}

/// The IPTC (tag 33723) and XMP (tag 700) entries of a TIFF's first IFD.
pub fn tiff_blocks(data: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let le = match data.get(..4) {
        Some(b"II*\0") => true,
        Some(b"MM\0*") => false,
        _ => return (None, None),
    };
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) });
    let u32_at = |o: usize| {
        data.get(o..o + 4).map(|b| {
            let a = [b[0], b[1], b[2], b[3]];
            if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) }
        })
    };
    let (mut iptc, mut xmp) = (None, None);
    let Some(ifd) = u32_at(4).map(|o| o as usize) else { return (None, None) };
    let count = u16_at(ifd).unwrap_or(0);
    for i in 0..usize::from(count) {
        let e = ifd + 2 + i * 12;
        let (Some(tag), Some(typ), Some(n)) = (u16_at(e), u16_at(e + 2), u32_at(e + 4)) else { break };
        let unit = match typ {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 => 4,
            _ => continue,
        };
        let size = n as usize * unit;
        let start = if size <= 4 { e + 8 } else { u32_at(e + 8).unwrap_or(0) as usize };
        let Some(bytes) = data.get(start..start.saturating_add(size)) else { continue };
        match tag {
            33723 => iptc = Some(bytes.to_vec()),
            700 => xmp = Some(bytes.to_vec()),
            _ => {}
        }
    }
    (iptc, xmp)
}

/// The `rdf:li` items of an XMP packet's `dc:subject`.
pub fn xmp_subjects(xmp: &str) -> Vec<String> {
    let Some(start) = xmp.find("<dc:subject") else { return Vec::new() };
    let rest = &xmp[start..];
    let end = rest.find("</dc:subject>").unwrap_or(rest.len());
    let mut body = &rest[..end];
    let mut out = Vec::new();
    while let Some(i) = body.find("<rdf:li") {
        body = &body[i..];
        let Some(open_end) = body.find('>') else { break };
        if body[..open_end].ends_with('/') {
            body = &body[open_end + 1..];
            continue;
        }
        body = &body[open_end + 1..];
        let Some(close) = body.find("</rdf:li>") else { break };
        let text = unescape(body[..close].trim());
        if !text.is_empty() {
            out.push(text);
        }
        body = &body[close..];
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else { break };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag>
<rdf:li>sunset</rdf:li><rdf:li>sea &amp; sky</rdf:li><rdf:li>beach</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#;

    fn iim(keywords: &[&str]) -> Vec<u8> {
        let mut b = vec![0x1C, 2, 0, 0, 2, 0, 4]; // record version
        for k in keywords {
            b.extend_from_slice(&[0x1C, 2, 25]);
            b.extend_from_slice(&(k.len() as u16).to_be_bytes());
            b.extend_from_slice(k.as_bytes());
        }
        b
    }

    fn segment(marker: u8, body: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        s.extend_from_slice(body);
        s
    }

    /// A JPEG header with an APP13 IPTC block and an APP1 XMP packet (no
    /// image data; only the segments are read).
    pub(crate) fn jpeg_with_keywords() -> Vec<u8> {
        let block = iim(&["sunset", "a;b", "sunset"]);
        let mut ps = b"Photoshop 3.0\0".to_vec();
        ps.extend_from_slice(b"8BIM");
        ps.extend_from_slice(&0x0404u16.to_be_bytes());
        ps.extend_from_slice(&[0, 0]); // empty name, padded
        ps.extend_from_slice(&(block.len() as u32).to_be_bytes());
        ps.extend_from_slice(&block);
        if block.len() % 2 == 1 {
            ps.push(0);
        }
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(segment(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
        jpeg.extend(segment(0xED, &ps));
        let mut app1 = XMP_APP1.to_vec();
        app1.extend_from_slice(XMP.as_bytes());
        jpeg.extend(segment(0xE1, &app1));
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0, 2, 0xFF, 0xD9]);
        jpeg
    }

    pub(crate) fn png_with_xmp(compressed: bool) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(2, 2, image::Rgb([200, 30, 30]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut body = XMP_PNG.to_vec();
        body.extend_from_slice(&[0, u8::from(compressed), 0, 0, 0]);
        if compressed {
            body.extend(miniz_oxide::deflate::compress_to_vec_zlib(XMP.as_bytes(), 6));
        } else {
            body.extend_from_slice(XMP.as_bytes());
        }
        let mut chunk = (body.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"iTXt");
        chunk.extend_from_slice(&body);
        chunk.extend_from_slice(&[0; 4]); // CRC, not checked here
        // Insert after IHDR (8-byte signature + 25-byte chunk).
        png.splice(33..33, chunk);
        png
    }

    #[test]
    fn jpeg_keywords() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("k.jpg");
        std::fs::write(&p, jpeg_with_keywords()).unwrap();
        assert_eq!(read(&p, "jpg"), ["sunset", "a", "b", "sea & sky", "beach"]);
    }

    #[test]
    fn png_keywords() {
        for compressed in [false, true] {
            assert_eq!(xmp_subjects(&String::from_utf8(png_xmp(&png_with_xmp(compressed)).unwrap()).unwrap()), ["sunset", "sea & sky", "beach"]);
        }
    }

    #[test]
    fn tiff_keywords() {
        let block = iim(&["red"]);
        let mut t = b"II*\0\x08\0\0\0".to_vec();
        t.extend_from_slice(&2u16.to_le_bytes());
        let data_at = 8 + 2 + 24 + 4;
        for (tag, typ, n, off) in [(700u16, 1u16, XMP.len() as u32, data_at as u32), (33723, 7, block.len() as u32, (data_at + XMP.len()) as u32)] {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&typ.to_le_bytes());
            t.extend_from_slice(&n.to_le_bytes());
            t.extend_from_slice(&off.to_le_bytes());
        }
        t.extend_from_slice(&0u32.to_le_bytes());
        t.extend_from_slice(XMP.as_bytes());
        t.extend_from_slice(&block);
        let (iptc, xmp) = tiff_blocks(&t);
        assert_eq!(iim_keywords(&iptc.unwrap()), ["red"]);
        assert_eq!(xmp_subjects(&String::from_utf8(xmp.unwrap()).unwrap()).len(), 3);
    }

    #[test]
    fn field_rules() {
        let long = "x".repeat(201);
        assert_eq!(split_fields(vec![long, "a;;b".into(), "b".into()]), ["a", "b"]);
        assert_eq!(unescape("&#x41;&#66;&bogus &lt;"), "AB&bogus <");
    }
}
