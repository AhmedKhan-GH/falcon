//! Camera metadata stored at import as `rawMetas`, in the shape Eagle writes
//! for RAW photos (see the Import chapter of the docs).
//!
//! Eagle gets these values from dcraw's identify output; falcon reads the
//! same facts from the file's EXIF and formats them the way dcraw prints them.

use std::path::Path;

use chrono::{Local, NaiveDateTime, TimeZone};
use ::exif::{In, Tag, Value};
use serde_json::{Map, Value as Json, json};

/// Extensions Eagle treats as RAW photos and analyses with dcraw.
pub const RAW_EXTENSIONS: [&str; 18] =
    ["3fr", "arw", "orf", "mrw", "nef", "nrw", "raw", "cr2", "crw", "dng", "erf", "pef", "raf", "rw2", "sr2", "srw", "x3f", "cr3"];

/// Other photo formats whose EXIF falcon can read.
pub const EXIF_EXTENSIONS: [&str; 8] = ["jpg", "jpeg", "tif", "tiff", "heic", "heif", "png", "webp"];

pub fn is_raw(ext: &str) -> bool {
    RAW_EXTENSIONS.contains(&ext)
}

/// Camera facts read from EXIF.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraMeta {
    /// Date taken, ms since the epoch, read as local time.
    pub timestamp: Option<i64>,
    /// Make and model, as dcraw names them.
    pub camera: Option<String>,
    pub iso: Option<u32>,
    /// Millimetres.
    pub focal_length: Option<f64>,
    /// Seconds.
    pub shutter: Option<f64>,
    /// The f-number.
    pub aperture: Option<f64>,
}

impl CameraMeta {
    pub fn is_empty(&self) -> bool {
        *self == CameraMeta::default()
    }

    /// The `rawMetas` object: `timestamp`, `camera`, `isoSpeed`,
    /// `focalLength`, `shutter`, `aperture`, each only when known.
    pub fn to_raw_metas(&self) -> Map<String, Json> {
        let mut m = Map::new();
        if let Some(t) = self.timestamp {
            m.insert("timestamp".into(), json!(t));
        }
        if let Some(c) = &self.camera {
            m.insert("camera".into(), json!(c));
        }
        if let Some(i) = self.iso {
            m.insert("isoSpeed".into(), json!(i));
        }
        if let Some(f) = self.focal_length {
            m.insert("focalLength".into(), json!(format!("{f:.1} mm")));
        }
        if let Some(s) = self.shutter {
            m.insert("shutter".into(), json!(shutter_text(s)));
        }
        if let Some(a) = self.aperture {
            m.insert("aperture".into(), json!(format!("f/{a:.1}")));
        }
        m
    }
}

/// dcraw's exposure text: `1/320.0 sec` under a second, else `2.0 sec`.
pub fn shutter_text(s: f64) -> String {
    if s > 0.0 && s < 1.0 { format!("1/{:.1} sec", 1.0 / s) } else { format!("{s:.1} sec") }
}

/// Maker names dcraw shortens a Make tag to when the tag contains them.
const MAKERS: [&str; 24] = [
    "AgfaPhoto", "Canon", "Casio", "Epson", "Fujifilm", "Hasselblad", "Kodak", "Konica", "Leaf", "Leica", "Mamiya", "Minolta",
    "Motorola", "Nikon", "Nokia", "Olympus", "Panasonic", "Pentax", "Phase One", "Ricoh", "Samsung", "Sigma", "Sinar", "Sony",
];

/// `Make Model` the way dcraw prints it: the maker shortened to its usual
/// name and repeated maker words removed from the model.
pub fn camera_name(make: &str, model: &str) -> String {
    let make = make.trim();
    let lower = make.to_lowercase();
    let make = MAKERS.iter().find(|m| lower.contains(&m.to_lowercase())).map_or(make.to_string(), |m| m.to_string());
    let mut model = model.trim();
    if let Some(rest) = model.get(make.len()..).filter(|_| model.to_lowercase().starts_with(&make.to_lowercase()))
        && rest.starts_with(' ')
    {
        model = rest.trim_start();
    }
    match (make.is_empty(), model.is_empty()) {
        (true, _) => model.to_string(),
        (_, true) => make,
        _ => format!("{make} {model}"),
    }
}

/// The first primary-image field with tag number `number` in any of the
/// blocks. Matching on the number alone lets a block that holds EXIF tags in
/// its first IFD (CR3's `CMT2`) count like a proper EXIF sub-IFD.
fn field(blocks: &[::exif::Exif], number: u16) -> Option<&::exif::Field> {
    blocks.iter().flat_map(|e| e.fields()).find(|f| f.ifd_num == In::PRIMARY && f.tag.number() == number)
}

fn text(blocks: &[::exif::Exif], tag: Tag) -> Option<String> {
    match &field(blocks, tag.number())?.value {
        Value::Ascii(v) => {
            let s = String::from_utf8_lossy(v.first()?).trim_matches(char::from(0)).trim().to_string();
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

fn number(blocks: &[::exif::Exif], tag: Tag) -> Option<f64> {
    match &field(blocks, tag.number())?.value {
        Value::Rational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        Value::SRational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        v => v.get_uint(0).map(f64::from),
    }
}

/// The EXIF blocks in a photo file. TIFF-based RAW files are read
/// directly; ORF and RW2 are TIFF with their own magic number in place of
/// 42; RAF carries a JPEG preview whose EXIF holds the camera; CR3 is an
/// ISO media file whose Canon box holds TIFF blocks `CMT1` (the main IFD)
/// and `CMT2` (the EXIF IFD). Anything else goes to the EXIF reader's own
/// container detection (JPEG, HEIF, PNG, WebP).
fn exif_blocks(data: &[u8]) -> Vec<::exif::Exif> {
    let reader = ::exif::Reader::new();
    let tiff = |bytes: Vec<u8>| reader.read_raw(bytes).ok();
    match data.get(..4).unwrap_or_default() {
        b"II*\0" | b"MM\0*" => tiff(data.to_vec()).into_iter().collect(),
        // Olympus ORF ("IIRO", "IIRS", "MMOR") and Panasonic RW2 ("IIU\0").
        b"IIRO" | b"IIRS" | b"IIU\0" | b"MMOR" => {
            let mut bytes = data.to_vec();
            let magic = if bytes[0] == b'I' { b"*\0" } else { b"\0*" };
            bytes[2..4].copy_from_slice(magic);
            tiff(bytes).into_iter().collect()
        }
        _ if data.starts_with(b"FUJIFILMCCD-RAW") => {
            let be32 = |o: usize| data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
            let jpeg = be32(84).zip(be32(88)).and_then(|(at, len)| data.get(at..at.checked_add(len)?));
            jpeg.and_then(|j| reader.read_from_container(&mut std::io::Cursor::new(j)).ok()).into_iter().collect()
        }
        b"\0MRM" => mrw_ttw(data).and_then(|t| tiff(t.to_vec())).into_iter().collect(),
        b"FOVb" => x3f_preview(data).and_then(|j| reader.read_from_container(&mut std::io::Cursor::new(j)).ok()).into_iter().collect(),
        _ if data.get(4..12) == Some(b"ftypcrx ") => cr3_tiff_blocks(data).into_iter().filter_map(|b| tiff(b.to_vec())).collect(),
        _ => reader.read_from_container(&mut std::io::Cursor::new(data)).ok().into_iter().collect(),
    }
}

/// ISO media boxes in `data`: (type, payload).
fn boxes(data: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as u64;
        let kind = &data[at + 4..at + 8];
        let (header, size) = match size {
            0 => (8, (data.len() - at) as u64),
            1 => match data.get(at + 8..at + 16) {
                Some(b) => (16, u64::from_be_bytes(b.try_into().expect("8 bytes"))),
                None => break,
            },
            n => (8, n),
        };
        let Some(end) = usize::try_from(size).ok().and_then(|s| at.checked_add(s)).filter(|&e| e <= data.len() && size >= header) else { break };
        out.push((kind, &data[at + header as usize..end]));
        at = end;
    }
    out
}

fn le32(data: &[u8], at: usize) -> Option<usize> {
    data.get(at..at.checked_add(4)?).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

fn be32(data: &[u8], at: usize) -> Option<usize> {
    data.get(at..at.checked_add(4)?).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// The TIFF block (`\0TTW`) of a Minolta MRW file. The file starts with
/// `\0MRM` and the big-endian offset of the image data (counted from byte
/// 8); before it come blocks of a 4-byte tag and a big-endian length
/// (`\0PRD`, `\0TTW`, `\0WBG`, `\0RIF`). The TTW payload is a complete TIFF.
fn mrw_ttw(data: &[u8]) -> Option<&[u8]> {
    let end = be32(data, 4)?.checked_add(8)?.min(data.len());
    let mut at = 8usize;
    // A handful of blocks exist; the cap stops a corrupt chain of tiny ones.
    for _ in 0..32 {
        let tag = data.get(at..at.checked_add(4)?)?;
        let len = be32(data, at + 4)?;
        let body = at.checked_add(8)?;
        let next = body.checked_add(len)?;
        if next > end {
            return None;
        }
        if tag == b"\0TTW" {
            return data.get(body..next);
        }
        at = next;
    }
    None
}

/// The JPEG preview of a Sigma/Foveon X3F file. The last 4 bytes (LE) point
/// at the `SECd` directory: tag, version, entry count, then entries of
/// (offset, length, 4-char type). An `IMAG` or `IMA2` section has a 28-byte
/// header (`SECi`, version, type, format, columns, rows, row size) before its
/// data; any such section whose data starts with a JPEG signature is taken
/// (the format code is not trusted, as it differs between camera
/// generations). The header's own rotation field (offset 36) is left out.
fn x3f_preview(data: &[u8]) -> Option<&[u8]> {
    let dir = le32(data, data.len().checked_sub(4)?)?;
    if data.get(dir..dir.checked_add(4)?)? != b"SECd" {
        return None;
    }
    let count = le32(data, dir + 8)?.min(256);
    for i in 0..count {
        let e = dir.checked_add(12)?.checked_add(i * 12)?;
        let (offset, length) = (le32(data, e)?, le32(data, e + 4)?);
        let kind = data.get(e + 8..e + 12)?;
        if kind != b"IMAG" && kind != b"IMA2" || length < 28 {
            continue;
        }
        let section = data.get(offset..offset.checked_add(length)?)?;
        if section.starts_with(b"SECi") && section[28..].starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(&section[28..]);
        }
    }
    None
}

/// Orientation of a Canon CRW (CIFF) file. Header: `II`, LE header length,
/// `HEAPCCDR`; the heap runs from the header length to the end of the file.
/// The last 4 bytes of a heap give the offset (from the heap start) of its
/// record table: u16 count, then 10-byte entries of u16 tag, u32 size, u32
/// offset. Tag bits 0x3800 are the data type; 0x2800 and 0x3000 mean the
/// data is itself a heap, so the search recurses (to a fixed depth). Bits
/// 0x4000 mean the data sits inside the entry and has no heap location.
/// Record 0x1810 (ImageInfo) holds width, height, aspect ratio (f32) and a
/// rotation in degrees at byte 12, the clockwise turn needed to display the
/// image upright, so 90 is EXIF 6 and 270 is EXIF 8 (as ExifTool reports it).
fn crw_orientation(data: &[u8]) -> Option<u32> {
    if data.get(..2)? != b"II" || data.get(6..14)? != b"HEAPCCDR" {
        return None;
    }
    let start = le32(data, 2)?;
    let angle = ciff_rotation(data.get(start..)?, 0, &mut 0)?;
    match angle.rem_euclid(360) {
        0 => Some(1),
        90 => Some(6),
        180 => Some(3),
        270 => Some(8),
        _ => None,
    }
}

fn ciff_rotation(heap: &[u8], depth: u32, seen: &mut u32) -> Option<i32> {
    if depth > 6 {
        return None;
    }
    let table = le32(heap, heap.len().checked_sub(4)?)?;
    let count = heap.get(table..table.checked_add(2)?).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)?;
    for i in 0..count {
        *seen += 1;
        if *seen > 512 {
            return None;
        }
        let e = table.checked_add(2)?.checked_add(i * 10)?;
        let tag = heap.get(e..e + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))?;
        if tag & 0xC000 != 0 {
            continue;
        }
        let (size, offset) = (le32(heap, e + 2)?, le32(heap, e + 6)?);
        let body = heap.get(offset..offset.checked_add(size)?)?;
        match tag & 0x3800 {
            0x2800 | 0x3000 => {
                if let Some(a) = ciff_rotation(body, depth + 1, seen) {
                    return Some(a);
                }
            }
            _ if tag == 0x1810 => return body.get(12..16).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            _ => {}
        }
    }
    None
}

/// Canon's metadata box inside CR3's `moov`.
const CANON_UUID: [u8; 16] = [0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48];

/// The `CMT1` and `CMT2` TIFF blocks of a CR3 file.
fn cr3_tiff_blocks(data: &[u8]) -> Vec<&[u8]> {
    let Some((_, moov)) = boxes(data).into_iter().find(|(k, _)| *k == b"moov") else { return Vec::new() };
    let Some((_, canon)) = boxes(moov).into_iter().find(|(k, p)| *k == b"uuid" && p.starts_with(&CANON_UUID)) else { return Vec::new() };
    boxes(&canon[16..]).into_iter().filter(|(k, _)| *k == b"CMT1" || *k == b"CMT2").map(|(_, p)| p).collect()
}

/// Reads camera facts from a photo's EXIF; `None` when it has none.
pub fn read(path: &Path) -> Option<CameraMeta> {
    let data = std::fs::read(path).ok()?;
    from_blocks(&exif_blocks(&data))
}

/// The EXIF orientation (1-8) of a photo file's main image, read from the
/// same places as [`read`]: the TIFF IFDs of TIFF-based RAW files, the EXIF
/// of the JPEG preview inside RAF, the `CMT1` block of CR3, the `\0TTW` TIFF
/// of MRW, the JPEG preview of X3F and the CIFF `ImageInfo` rotation of CRW.
pub fn orientation(data: &[u8]) -> Option<u32> {
    let from_exif = field(&exif_blocks(data), Tag::Orientation.number()).and_then(|f| f.value.get_uint(0)).filter(|v| (1..=8).contains(v));
    from_exif.or_else(|| crw_orientation(data))
}

fn from_blocks(exif: &[::exif::Exif]) -> Option<CameraMeta> {
    if exif.is_empty() {
        return None;
    }
    let camera = match (text(exif, Tag::Make), text(exif, Tag::Model)) {
        (None, None) => None,
        (make, model) => Some(camera_name(make.as_deref().unwrap_or(""), model.as_deref().unwrap_or(""))),
    };
    let timestamp = text(exif, Tag::DateTimeOriginal)
        .or_else(|| text(exif, Tag::DateTime))
        .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S").ok())
        .and_then(|d| Local.from_local_datetime(&d).earliest())
        .map(|d| d.timestamp_millis());
    let meta = CameraMeta {
        timestamp,
        camera,
        iso: number(exif, Tag::PhotographicSensitivity).map(|v| v as u32).filter(|&v| v > 0),
        focal_length: number(exif, Tag::FocalLength).filter(|&v| v > 0.0),
        shutter: number(exif, Tag::ExposureTime).filter(|&v| v > 0.0),
        aperture: number(exif, Tag::FNumber).filter(|&v| v > 0.0),
    };
    (!meta.is_empty()).then_some(meta)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn dcraw_formats() {
        assert_eq!(shutter_text(1.0 / 320.0), "1/320.0 sec");
        assert_eq!(shutter_text(2.0), "2.0 sec");
        assert_eq!(camera_name("NIKON CORPORATION", "NIKON D850"), "Nikon D850");
        assert_eq!(camera_name("Canon", "Canon EOS R5"), "Canon EOS R5");
        assert_eq!(camera_name("SONY", "ILCE-7M3"), "Sony ILCE-7M3");
        assert_eq!(camera_name("Apple", "iPhone 15 Pro"), "Apple iPhone 15 Pro");
        let m = CameraMeta {
            timestamp: Some(1),
            camera: Some("Pentax K-1".into()),
            iso: Some(200),
            focal_length: Some(200.0),
            shutter: Some(1.0 / 320.0),
            aperture: Some(3.2),
        };
        let v = serde_json::to_string(&m.to_raw_metas()).unwrap();
        assert_eq!(
            v,
            r#"{"timestamp":1,"camera":"Pentax K-1","isoSpeed":200,"focalLength":"200.0 mm","shutter":"1/320.0 sec","aperture":"f/3.2"}"#
        );
    }

    /// A little-endian TIFF with Make, Model and an EXIF IFD holding
    /// ExposureTime, FNumber, ISO, DateTimeOriginal and FocalLength.
    pub(crate) fn tiff_with_exif() -> Vec<u8> {
        let mut t = Vec::new();
        let push16 = |t: &mut Vec<u8>, v: u16| t.extend_from_slice(&v.to_le_bytes());
        let push32 = |t: &mut Vec<u8>, v: u32| t.extend_from_slice(&v.to_le_bytes());
        t.extend_from_slice(b"II*\0");
        push32(&mut t, 8);
        // IFD0 at 8: 3 entries -> 2 + 36 + 4 = 42 bytes, data from 50.
        let make = b"NIKON CORPORATION\0";
        let model = b"NIKON D850\0";
        let (make_at, model_at) = (50u32, 50 + make.len() as u32);
        let exif_at = model_at + model.len() as u32;
        push16(&mut t, 3);
        for (tag, typ, count, val) in [(0x010F, 2, make.len() as u32, make_at), (0x0110, 2, model.len() as u32, model_at), (0x8769, 4, 1, exif_at)] {
            push16(&mut t, tag);
            push16(&mut t, typ);
            push32(&mut t, count);
            push32(&mut t, val);
        }
        push32(&mut t, 0);
        t.resize(50, 0);
        t.extend_from_slice(make);
        t.extend_from_slice(model);
        // EXIF IFD: 5 entries -> 2 + 60 + 4 = 66 bytes, data after.
        let date = b"2024:03:15 12:00:00\0";
        let data_at = exif_at + 66;
        let (exp_at, fnum_at, date_at, focal_at) = (data_at, data_at + 8, data_at + 16, data_at + 16 + date.len() as u32);
        push16(&mut t, 5);
        for (tag, typ, count, val) in
            [(0x829A, 5, 1, exp_at), (0x829D, 5, 1, fnum_at), (0x8827, 3, 1, 400), (0x9003, 2, date.len() as u32, date_at), (0x920A, 5, 1, focal_at)]
        {
            push16(&mut t, tag);
            push16(&mut t, typ);
            push32(&mut t, count);
            push32(&mut t, val);
        }
        push32(&mut t, 0);
        for (n, d) in [(1, 250), (28, 10)] {
            push32(&mut t, n);
            push32(&mut t, d);
        }
        t.extend_from_slice(date);
        push32(&mut t, 50);
        push32(&mut t, 1);
        t
    }

    #[test]
    fn reads_tiff_exif() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.nef");
        std::fs::write(&p, tiff_with_exif()).unwrap();
        let m = read(&p).unwrap();
        assert_eq!(m.camera.as_deref(), Some("Nikon D850"));
        assert_eq!(m.iso, Some(400));
        assert_eq!(m.aperture, Some(2.8));
        assert_eq!(m.focal_length, Some(50.0));
        assert_eq!(shutter_text(m.shutter.unwrap()), "1/250.0 sec");
        let expected = Local.with_ymd_and_hms(2024, 3, 15, 12, 0, 0).unwrap().timestamp_millis();
        assert_eq!(m.timestamp, Some(expected));
    }

    /// A little-endian TIFF with one IFD of `(tag, type, count, bytes)`
    /// entries; values over four bytes go after the IFD.
    fn tiff(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let mut t = b"II*\0\x08\0\0\0".to_vec();
        let mut data_at = 8 + 2 + entries.len() * 12 + 4;
        let mut tail = Vec::new();
        t.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, typ, count, bytes) in entries {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&typ.to_le_bytes());
            t.extend_from_slice(&count.to_le_bytes());
            if bytes.len() <= 4 {
                let mut v = bytes.clone();
                v.resize(4, 0);
                t.extend_from_slice(&v);
            } else {
                t.extend_from_slice(&(data_at as u32).to_le_bytes());
                tail.extend_from_slice(bytes);
                data_at += bytes.len();
            }
        }
        t.extend_from_slice(&[0; 4]);
        t.extend(tail);
        t
    }

    fn ascii(tag: u16, s: &str) -> (u16, u16, u32, Vec<u8>) {
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        (tag, 2, b.len() as u32, b)
    }

    fn rational(tag: u16, n: u32, d: u32) -> (u16, u16, u32, Vec<u8>) {
        (tag, 5, 1, [n.to_le_bytes(), d.to_le_bytes()].concat())
    }

    fn mp4_box(kind: &[u8], payload: &[u8]) -> Vec<u8> {
        [&((payload.len() + 8) as u32).to_be_bytes()[..], kind, payload].concat()
    }

    fn read_bytes(name: &str, bytes: &[u8]) -> Option<CameraMeta> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        read(&p)
    }

    /// The facts [`tiff_with_exif`] holds, as `rawMetas`.
    fn nikon_metas() -> String {
        let t = Local.with_ymd_and_hms(2024, 3, 15, 12, 0, 0).unwrap().timestamp_millis();
        format!(r#"{{"timestamp":{t},"camera":"Nikon D850","isoSpeed":400,"focalLength":"50.0 mm","shutter":"1/250.0 sec","aperture":"f/2.8"}}"#)
    }

    #[test]
    fn reads_orf_and_rw2_with_their_own_magic() {
        for magic in [*b"IIRO", *b"IIRS", *b"IIU\0"] {
            let mut raw = tiff_with_exif();
            raw[..4].copy_from_slice(&magic);
            let m = read_bytes("x.orf", &raw).unwrap_or_else(|| panic!("{magic:?}"));
            assert_eq!(serde_json::to_string(&m.to_raw_metas()).unwrap(), nikon_metas());
        }
    }

    #[test]
    fn reads_raf_from_its_jpeg_preview() {
        let tiff = tiff_with_exif();
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        raf.resize(100, 0);
        raf[84..88].copy_from_slice(&100u32.to_be_bytes());
        raf[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
        raf.extend_from_slice(&jpeg);
        raf.extend_from_slice(&[0u8; 64]); // the CFA data would follow
        let m = read_bytes("x.raf", &raf).unwrap();
        assert_eq!(serde_json::to_string(&m.to_raw_metas()).unwrap(), nikon_metas());
        // A preview pointer past the end gives nothing rather than garbage.
        raf[84..88].copy_from_slice(&9999u32.to_be_bytes());
        assert_eq!(read_bytes("x.raf", &raf), None);
    }

    #[test]
    fn reads_cr3_canon_boxes() {
        let cmt1 = tiff(&[ascii(0x010F, "Canon"), ascii(0x0110, "Canon EOS R5"), ascii(0x0132, "2024:01:02 03:04:05")]);
        let cmt2 = tiff(&[
            rational(0x829A, 1, 1000),
            rational(0x829D, 40, 10),
            (0x8827, 3, 1, 800u16.to_le_bytes().to_vec()),
            ascii(0x9003, "2024:01:02 03:04:06"),
            rational(0x920A, 85, 1),
        ]);
        let canon = [&CANON_UUID[..], &mp4_box(b"CNCV", b"CanonCR3_001/00.09.00/00.00.00"), &mp4_box(b"CMT1", &cmt1), &mp4_box(b"CMT2", &cmt2), &mp4_box(b"CMT3", b"II*\0")].concat();
        let moov = mp4_box(b"moov", &[mp4_box(b"uuid", &canon), mp4_box(b"trak", &[0; 16])].concat());
        let cr3 = [mp4_box(b"ftyp", b"crx \0\0\0\x01crx isom"), moov, mp4_box(b"mdat", &[0; 32])].concat();
        let m = read_bytes("x.cr3", &cr3).unwrap();
        let t = Local.with_ymd_and_hms(2024, 1, 2, 3, 4, 6).unwrap().timestamp_millis();
        assert_eq!(
            serde_json::to_string(&m.to_raw_metas()).unwrap(),
            format!(r#"{{"timestamp":{t},"camera":"Canon EOS R5","isoSpeed":800,"focalLength":"85.0 mm","shutter":"1/1000.0 sec","aperture":"f/4.0"}}"#)
        );
        assert_eq!(orientation(&cr3), None);
        // Without the Canon box there is nothing to read.
        let bare = [mp4_box(b"ftyp", b"crx \0\0\0\x01crx isom"), mp4_box(b"moov", &mp4_box(b"trak", &[0; 16]))].concat();
        assert_eq!(read_bytes("y.cr3", &bare), None);
    }

    /// Orientation comes out of RAF's JPEG preview and CR3's `CMT1`, as well
    /// as plain TIFF-based files.
    #[test]
    fn orientation_of_raf_cr3_and_tiff() {
        let turned = tiff(&[ascii(0x010F, "X"), (0x0112, 3, 1, 6u16.to_le_bytes().to_vec())]);
        assert_eq!(orientation(&turned), Some(6));
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((turned.len() + 8) as u16).to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&turned);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        raf.resize(100, 0);
        raf[84..88].copy_from_slice(&100u32.to_be_bytes());
        raf[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
        raf.extend_from_slice(&jpeg);
        assert_eq!(orientation(&raf), Some(6));
        let canon = [&CANON_UUID[..], &mp4_box(b"CMT1", &tiff(&[(0x0112, 3, 1, 8u16.to_le_bytes().to_vec())]))].concat();
        let cr3 = [mp4_box(b"ftyp", b"crx \0\0\0\x01crx isom"), mp4_box(b"moov", &mp4_box(b"uuid", &canon))].concat();
        assert_eq!(orientation(&cr3), Some(8));
        // Out-of-range values are ignored.
        assert_eq!(orientation(&tiff(&[(0x0112, 3, 1, 9u16.to_le_bytes().to_vec())])), None);
    }

    /// A minimal MRW: `\0MRM`, data offset, then PRD, TTW and WBG blocks.
    fn mrw(ttw: &[u8]) -> Vec<u8> {
        let blk = |tag: &[u8; 4], body: &[u8]| [&tag[..], &(body.len() as u32).to_be_bytes(), body].concat();
        let blocks = [blk(b"\0PRD", &[0; 24]), blk(b"\0TTW", ttw), blk(b"\0WBG", &[0; 12])].concat();
        [&b"\0MRM"[..], &(blocks.len() as u32).to_be_bytes(), &blocks, &[0; 16]].concat()
    }

    fn jpeg_with_exif(tiff: &[u8]) -> Vec<u8> {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(tiff);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn orientation_of_mrw_ttw_block() {
        for o in 1..=8u16 {
            let raw = mrw(&tiff(&[(0x0112, 3, 1, o.to_le_bytes().to_vec())]));
            assert_eq!(orientation(&raw), Some(u32::from(o)));
        }
        let ttw = tiff(&[ascii(0x010F, "Minolta")]);
        assert_eq!(orientation(&mrw(&ttw)), None);
        let raw = mrw(&tiff(&[(0x0112, 3, 1, 6u16.to_le_bytes().to_vec())]));
        assert_eq!(orientation(&raw[..20]), None);
        assert_eq!(orientation(&raw[..raw.len() / 2]), None);
        // A block length past the data area is refused.
        let mut bad = raw.clone();
        bad[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(orientation(&bad), None);
        assert_eq!(orientation(b"\0MRM"), None);
    }

    /// An X3F with a raw `IMAG` section and a preview section.
    fn x3f(preview: &[u8]) -> Vec<u8> {
        let mut f = b"FOVb".to_vec();
        f.resize(104, 0);
        let sec = |body: &[u8]| [&b"SECi"[..], &[0; 24], body].concat();
        let raw = sec(&[0u8; 20]);
        let prev = sec(preview);
        let raw_at = f.len();
        f.extend_from_slice(&raw);
        let prev_at = f.len();
        f.extend_from_slice(&prev);
        let dir_at = f.len() as u32;
        f.extend_from_slice(b"SECd");
        f.extend_from_slice(&0x0002_0000u32.to_le_bytes());
        f.extend_from_slice(&2u32.to_le_bytes());
        for (at, len, kind) in [(raw_at, raw.len(), b"IMAG"), (prev_at, prev.len(), b"IMA2")] {
            f.extend_from_slice(&(at as u32).to_le_bytes());
            f.extend_from_slice(&(len as u32).to_le_bytes());
            f.extend_from_slice(kind);
        }
        f.extend_from_slice(&dir_at.to_le_bytes());
        f
    }

    #[test]
    fn orientation_of_x3f_preview() {
        for o in [1u16, 3, 6, 8] {
            let raw = x3f(&jpeg_with_exif(&tiff(&[(0x0112, 3, 1, o.to_le_bytes().to_vec())])));
            assert_eq!(orientation(&raw), Some(u32::from(o)));
        }
        let raw = x3f(&jpeg_with_exif(&tiff(&[(0x0112, 3, 1, 6u16.to_le_bytes().to_vec())])));
        assert_eq!(orientation(&raw[..raw.len() - 8]), None);
        assert_eq!(orientation(&raw[..60]), None);
        assert_eq!(orientation(&x3f(&[1, 2, 3, 4])), None);
        assert_eq!(orientation(b"FOVb"), None);
        // A directory pointer past the end gives nothing.
        let mut bad = raw.clone();
        let n = bad.len();
        bad[n - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(orientation(&bad), None);
    }

    /// A CRW: header, then a heap whose root table holds a sub-heap
    /// (0x300a, image properties) with a 0x1810 record of `angle` degrees.
    fn crw(angle: i32) -> Vec<u8> {
        let entry = |tag: u16, size: usize, off: usize| [&tag.to_le_bytes()[..], &(size as u32).to_le_bytes(), &(off as u32).to_le_bytes()].concat();
        // Inner heap: 26 bytes of ImageInfo, table, table offset.
        let mut info = vec![0u8; 12];
        info.extend_from_slice(&angle.to_le_bytes());
        info.extend_from_slice(&[0; 10]);
        let mut inner = info.clone();
        let table_at = inner.len() as u32;
        inner.extend_from_slice(&1u16.to_le_bytes());
        inner.extend_from_slice(&entry(0x1810, info.len(), 0));
        inner.extend_from_slice(&table_at.to_le_bytes());
        // Outer heap: a decoy record, the inner heap, table, table offset.
        let mut heap = vec![0xAA; 8];
        let inner_at = heap.len();
        heap.extend_from_slice(&inner);
        let table_at = heap.len() as u32;
        heap.extend_from_slice(&2u16.to_le_bytes());
        heap.extend_from_slice(&entry(0x2005, 8, 0));
        heap.extend_from_slice(&entry(0x300A, inner.len(), inner_at));
        heap.extend_from_slice(&table_at.to_le_bytes());
        [&b"II"[..], &26u32.to_le_bytes(), b"HEAPCCDR", &[0; 12], &heap].concat()
    }

    #[test]
    fn orientation_of_crw_image_info() {
        for (angle, want) in [(0, Some(1)), (90, Some(6)), (180, Some(3)), (270, Some(8)), (-90, Some(8)), (45, None)] {
            assert_eq!(orientation(&crw(angle)), want, "{angle}");
        }
        let raw = crw(90);
        assert_eq!(orientation(&raw[..raw.len() - 1]), None);
        assert_eq!(orientation(&raw[..20]), None);
        assert_eq!(orientation(&raw[..14]), None);
        // A header length or table offset past the end gives nothing.
        let mut bad = raw.clone();
        bad[2..6].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(orientation(&bad), None);
        let mut bad = raw.clone();
        let n = bad.len();
        bad[n - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(orientation(&bad), None);
        // A heap that nests itself ends at the depth cap.
        let mut looped = raw.clone();
        let n = looped.len();
        looped[26..30].copy_from_slice(&[0; 4]);
        looped[n - 4..].copy_from_slice(&(n as u32 - 26 - 4).to_le_bytes());
        assert_eq!(orientation(&looped), None);
        assert_eq!(orientation(b"II\x1a\0\0\0HEAPCCDR"), None);
    }
}
