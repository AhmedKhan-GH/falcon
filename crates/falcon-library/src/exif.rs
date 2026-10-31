//! Camera metadata stored at import as `rawMetas`, in the shape Eagle writes
//! for RAW photos (see the Import chapter of the docs).
//!
//! Eagle gets these values from dcraw's identify output; falcon reads the
//! same facts from the file's EXIF and formats them the way dcraw prints them.

use std::fs::File;
use std::io::BufReader;
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

fn text(exif: &::exif::Exif, tag: Tag) -> Option<String> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(v) => {
            let s = String::from_utf8_lossy(v.first()?).trim_matches(char::from(0)).trim().to_string();
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

fn number(exif: &::exif::Exif, tag: Tag) -> Option<f64> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        Value::SRational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        v => v.get_uint(0).map(f64::from),
    }
}

/// Reads camera facts from a photo's EXIF; `None` when it has none.
pub fn read(path: &Path) -> Option<CameraMeta> {
    let file = File::open(path).ok()?;
    let exif = ::exif::Reader::new().read_from_container(&mut BufReader::new(file)).ok()?;
    let camera = match (text(&exif, Tag::Make), text(&exif, Tag::Model)) {
        (None, None) => None,
        (make, model) => Some(camera_name(make.as_deref().unwrap_or(""), model.as_deref().unwrap_or(""))),
    };
    let timestamp = text(&exif, Tag::DateTimeOriginal)
        .or_else(|| text(&exif, Tag::DateTime))
        .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S").ok())
        .and_then(|d| Local.from_local_datetime(&d).earliest())
        .map(|d| d.timestamp_millis());
    let meta = CameraMeta {
        timestamp,
        camera,
        iso: number(&exif, Tag::PhotographicSensitivity).map(|v| v as u32).filter(|&v| v > 0),
        focal_length: number(&exif, Tag::FocalLength).filter(|&v| v > 0.0),
        shutter: number(&exif, Tag::ExposureTime).filter(|&v| v > 0.0),
        aperture: number(&exif, Tag::FNumber).filter(|&v| v > 0.0),
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
}
