//! Rotating, flipping and cropping an item's original file, following
//! Eagle's viewer.
//!
//! Eagle rewrites the original in place and keeps its name. A JPEG is turned
//! losslessly by changing its EXIF orientation; PNG, WebP and BMP are drawn
//! rotated and encoded again. Cropping re-encodes PNG, JPEG and WebP. After
//! either, the item is refreshed like "Refresh Thumbnail": size and file
//! times are read again, width and height measured, the thumbnail redrawn
//! and the palette recomputed.
//!
//! Undo can't bring back overwritten pixels, so these edits mark the open
//! undo transaction irreversible (as a permanent delete does). Eagle has no
//! undo for them either.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageFormat};

use crate::error::{Error, Result};
use crate::import::ImportOptions;
use crate::library::Library;
use crate::thumbnail;

/// A rectangle in the original image's displayed pixels (EXIF orientation
/// applied), as Eagle's crop tool measures it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Formats Eagle's rotate button works on.
pub fn can_rotate(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp")
}

/// A mirror direction for [`Library::flip_item`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flip {
    /// Left and right swap.
    Horizontal,
    /// Top and bottom swap.
    Vertical,
}

/// Formats Eagle's flip button writes to disk (AVIF is accepted by Eagle
/// but fails when written, and falcon can't encode it).
pub fn can_flip(ext: &str) -> bool {
    can_rotate(ext)
}

/// Formats Eagle's crop tool works on.
pub fn can_crop(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp")
}

impl Library {
    /// Turns an item's original by `quarter_turns` × 90° clockwise (negative
    /// turns go anticlockwise). JPEGs only get a new EXIF orientation, so
    /// their pixels are untouched; PNG, WebP and BMP are re-encoded.
    /// Animated PNG and WebP are refused, as Eagle refuses APNG.
    pub fn rotate_item(&mut self, id: &str, quarter_turns: i32) -> Result<()> {
        let item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let ext = item.ext.to_lowercase();
        if !can_rotate(&ext) {
            return Err(self.invalid(format!("falcon can't rotate .{} files", item.ext)));
        }
        let turns = quarter_turns.rem_euclid(4) as u8;
        if turns == 0 {
            return Ok(());
        }
        let raw = item.original_path(self.root());
        let data = fs::read(&raw).map_err(|e| Error::io(&raw, e))?;
        let out = if ext == "jpg" || ext == "jpeg" {
            let current = jpeg_orientation(&data).ok_or_else(|| self.invalid("not a JPEG file"))?;
            set_jpeg_orientation(&data, turn_orientation(current, turns)).ok_or_else(|| self.invalid("cannot write the JPEG's orientation"))?
        } else {
            self.refuse_animated(&data)?;
            let img = thumbnail::decode(&raw).map_err(|e| self.invalid(e.to_string()))?;
            let img = match turns {
                1 => img.rotate90(),
                2 => img.rotate180(),
                _ => img.rotate270(),
            };
            encode_like(&img, &ext, &data).map_err(|e| self.invalid(e))?
        };
        self.rewrite(item, &raw, &out)
    }

    /// Mirrors an item's original as shown (EXIF orientation applied). Like
    /// rotating, a JPEG only gets a new EXIF orientation and keeps its
    /// pixels; PNG, WebP and BMP are drawn mirrored and re-encoded. The
    /// item is refreshed (size, dimensions, thumbnail, palette) and the
    /// open undo transaction marked irreversible. Animated PNG and WebP
    /// are refused.
    pub fn flip_item(&mut self, id: &str, flip: Flip) -> Result<()> {
        let item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let ext = item.ext.to_lowercase();
        if !can_flip(&ext) {
            return Err(self.invalid(format!("falcon can't flip .{} files", item.ext)));
        }
        let raw = item.original_path(self.root());
        let data = fs::read(&raw).map_err(|e| Error::io(&raw, e))?;
        let out = if ext == "jpg" || ext == "jpeg" {
            let current = jpeg_orientation(&data).ok_or_else(|| self.invalid("not a JPEG file"))?;
            set_jpeg_orientation(&data, flip_orientation(current, flip)).ok_or_else(|| self.invalid("cannot write the JPEG's orientation"))?
        } else {
            self.refuse_animated(&data)?;
            let img = thumbnail::decode(&raw).map_err(|e| self.invalid(e.to_string()))?;
            let img = match flip {
                Flip::Horizontal => img.fliph(),
                Flip::Vertical => img.flipv(),
            };
            encode_like(&img, &ext, &data).map_err(|e| self.invalid(e))?
        };
        self.rewrite(item, &raw, &out)
    }

    /// Crops an item's original to `rect`, in displayed pixels (the part
    /// outside the image is ignored), and saves it under the same name. A
    /// JPEG is encoded at quality 100 without its EXIF block, as a canvas
    /// export does, so its orientation is baked into the pixels.
    pub fn crop_item(&mut self, id: &str, rect: PixelRect) -> Result<()> {
        let item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let raw = item.original_path(self.root());
        let Some(out) = self.cropped(&item, rect)? else { return Ok(()) };
        self.rewrite(item, &raw, &out)
    }

    /// Eagle's crop "Save As": the cropped picture becomes a new item and
    /// the original is left alone. The new item has the original's name,
    /// tags, folders, source URL and rating, and its date added is just
    /// after the original's so the two sit together. Returns its id; a
    /// crop of the whole picture still makes a copy.
    pub fn crop_item_as_new(&mut self, id: &str, rect: PixelRect) -> Result<String> {
        let item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let out = match self.cropped(&item, rect)? {
            Some(bytes) => bytes,
            None => {
                let raw = item.original_path(self.root());
                fs::read(&raw).map_err(|e| Error::io(&raw, e))?
            }
        };
        let dir = tempfile::Builder::new().prefix("falcon-crop").tempdir().map_err(|e| Error::io(self.root(), e))?;
        let file = dir.path().join(format!("{}.{}", item.name, item.ext));
        fs::write(&file, &out).map_err(|e| Error::io(&file, e))?;
        let opts = ImportOptions { folders: item.folders.clone(), tags: item.tags.clone(), url: item.url.clone(), ..Default::default() };
        self.check_folders(&file, &opts)?;
        let staged = crate::import::stage(&file, &opts)?;
        let new_id = self.commit_import(staged)?;
        let mut copy = self.item(&new_id).cloned().ok_or_else(|| self.invalid("the cropped copy went missing"))?;
        copy.modification_time = item.modification_time + 1;
        if let Some(star) = item.extra.get("star").filter(|v| v.as_u64().is_some_and(|n| n > 0)) {
            copy.extra.insert("star".into(), star.clone());
        }
        self.update_item(copy)?;
        Ok(new_id)
    }

    /// The item's original cropped to `rect` and encoded in its format, or
    /// `None` when `rect` covers the whole picture.
    fn cropped(&self, item: &crate::model::Item, rect: PixelRect) -> Result<Option<Vec<u8>>> {
        let ext = item.ext.to_lowercase();
        if !can_crop(&ext) {
            return Err(self.invalid(format!("falcon can't crop .{} files", item.ext)));
        }
        let raw = item.original_path(self.root());
        let data = fs::read(&raw).map_err(|e| Error::io(&raw, e))?;
        self.refuse_animated(&data)?;
        let img = thumbnail::decode(&raw).map_err(|e| self.invalid(e.to_string()))?;
        let x = rect.x.min(img.width());
        let y = rect.y.min(img.height());
        let w = rect.width.min(img.width() - x);
        let h = rect.height.min(img.height() - y);
        if w == 0 || h == 0 {
            return Err(self.invalid("the crop area is empty"));
        }
        if (x, y, w, h) == (0, 0, img.width(), img.height()) {
            return Ok(None);
        }
        encode_like(&img.crop_imm(x, y, w, h), &ext, &data).map(Some).map_err(|e| self.invalid(e))
    }

    fn refuse_animated(&self, data: &[u8]) -> Result<()> {
        if is_animated(data) { Err(self.invalid("animated images can't be edited")) } else { Ok(()) }
    }

    /// Writes the new bytes over the original, then refreshes the item and
    /// marks the undo step irreversible. The file is replaced atomically,
    /// so a failed write leaves the original as it was.
    fn rewrite(&mut self, item: crate::model::Item, raw: &Path, bytes: &[u8]) -> Result<()> {
        replace_atomically(raw, bytes)?;
        let r = self.redraw_item(item);
        if let Some(j) = &mut self.journal {
            j.irreversible = true;
        }
        r
    }
}

/// Replaces `path` with `bytes` atomically: the bytes go into `path.staged`
/// beside it and are renamed over the original once synced, so a crash
/// leaves either the old file or the new one. A failure removes the staged
/// file and leaves the original untouched.
fn replace_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut name = path.as_os_str().to_owned();
    name.push(".staged");
    let staged = std::path::PathBuf::from(name);
    // Copying first gives the staged file the original's permissions and
    // extended attributes, which the rename then carries over.
    fs::copy(path, &staged).map_err(|e| Error::io(&staged, e))?;
    let done = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&staged)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        })
        .and_then(|()| fs::rename(&staged, path));
    done.map_err(|e| {
        let _ = fs::remove_file(&staged);
        Error::io(path, e)
    })
}

/// Encodes `img` in the item's format. JPEG uses quality 100 and BMP is
/// plain. WebP stays lossless when the original was lossless and otherwise
/// uses lossy quality 100.
fn encode_like(img: &DynamicImage, ext: &str, original: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::new();
    match ext {
        "jpg" | "jpeg" => {
            let rgb = img.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 100).encode_image(&rgb).map_err(|e| e.to_string())?;
        }
        "png" => {
            let img = if img.color().has_alpha() { img.clone() } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
            img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).map_err(|e| e.to_string())?;
        }
        "bmp" => img.write_to(&mut Cursor::new(&mut out), ImageFormat::Bmp).map_err(|e| e.to_string())?,
        "webp" => {
            let rgba = img.to_rgba8();
            let enc = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
            let mem = if webp_is_lossless(original) { enc.encode_lossless() } else { enc.encode(100.0) };
            out = mem.to_vec();
        }
        _ => return Err(format!("cannot write .{ext} files")),
    }
    Ok(out)
}

/// RIFF chunks of a WebP file: (fourcc, payload).
fn webp_chunks(data: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return out;
    }
    let mut at = 12;
    while at + 8 <= data.len() {
        let len = u32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
        let end = (at + 8 + len).min(data.len());
        out.push((&data[at..at + 4], &data[at + 8..end]));
        at = at + 8 + len + (len & 1);
    }
    out
}

fn webp_is_lossless(data: &[u8]) -> bool {
    webp_chunks(data).iter().any(|(id, _)| *id == b"VP8L")
}

/// An animated PNG (an `acTL` chunk) or WebP (an `ANIM` chunk).
pub fn is_animated(data: &[u8]) -> bool {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut at = 8;
        while at + 8 <= data.len() {
            let len = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
            match &data[at + 4..at + 8] {
                b"acTL" => return true,
                b"IDAT" | b"IEND" => return false,
                _ => {}
            }
            at += 12 + len;
        }
        return false;
    }
    webp_chunks(data).iter().any(|(id, _)| *id == b"ANIM")
}

/// EXIF orientation as (mirrored, clockwise quarter turns applied after the
/// mirror). 1 is upright; 6, 3 and 8 turn it 90°, 180° and 270°; 2, 7, 4
/// and 5 are the same turns of the mirrored image.
fn orientation_parts(o: u16) -> (bool, u8) {
    match o {
        6 => (false, 1),
        3 => (false, 2),
        8 => (false, 3),
        2 => (true, 0),
        7 => (true, 1),
        4 => (true, 2),
        5 => (true, 3),
        _ => (false, 0),
    }
}

/// The orientation after turning an image shown with orientation `o` by
/// `turns` more clockwise quarter turns.
pub fn turn_orientation(o: u16, turns: u8) -> u16 {
    let (mirrored, q) = orientation_parts(o);
    match (mirrored, (q + turns) % 4) {
        (false, 0) => 1,
        (false, 1) => 6,
        (false, 2) => 3,
        (false, 3) => 8,
        (true, 0) => 2,
        (true, 1) => 7,
        (true, 2) => 4,
        _ => 5,
    }
}

/// The orientation after mirroring an image shown with orientation `o`.
/// A horizontal mirror of a turned image is the mirrored image turned the
/// other way; a vertical mirror is a horizontal one plus a half turn.
pub fn flip_orientation(o: u16, flip: Flip) -> u16 {
    let (mirrored, q) = orientation_parts(o);
    let back = (4 - q) % 4;
    let base = if mirrored { turn_orientation(1, back) } else { turn_orientation(2, back) };
    match flip {
        Flip::Horizontal => base,
        Flip::Vertical => turn_orientation(base, 2),
    }
}

/// A JPEG segment: marker byte, offset of the marker and total length
/// including the marker and length bytes.
struct Segment {
    marker: u8,
    start: usize,
    len: usize,
}

/// The segments before the image data, or `None` if `data` isn't a JPEG.
fn jpeg_segments(data: &[u8]) -> Option<Vec<Segment>> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 2;
    while at + 4 <= data.len() {
        if data[at] != 0xFF {
            return None;
        }
        let marker = data[at + 1];
        if marker == 0xFF {
            at += 1;
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([data[at + 2], data[at + 3]]) as usize + 2;
        if at + len > data.len() {
            return None;
        }
        out.push(Segment { marker, start: at, len });
        at += len;
    }
    Some(out)
}

/// The `Exif\0\0` APP1 segment, if any.
fn exif_segment(data: &[u8], segs: &[Segment]) -> Option<(usize, usize)> {
    segs.iter().find(|s| s.marker == 0xE1 && data[s.start + 4..s.start + s.len].starts_with(b"Exif\0\0")).map(|s| (s.start, s.len))
}

/// Byte order of a TIFF block.
#[derive(Clone, Copy)]
struct Tiff(bool);

impl Tiff {
    fn u16(self, b: &[u8], at: usize) -> Option<u16> {
        let v = [*b.get(at)?, *b.get(at + 1)?];
        Some(if self.0 { u16::from_le_bytes(v) } else { u16::from_be_bytes(v) })
    }
    fn u32(self, b: &[u8], at: usize) -> Option<u32> {
        let v = [*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?];
        Some(if self.0 { u32::from_le_bytes(v) } else { u32::from_be_bytes(v) })
    }
    fn put16(self, v: u16) -> [u8; 2] {
        if self.0 { v.to_le_bytes() } else { v.to_be_bytes() }
    }
    fn put32(self, v: u32) -> [u8; 4] {
        if self.0 { v.to_le_bytes() } else { v.to_be_bytes() }
    }
}

/// Reads a TIFF header: byte order and IFD0 offset.
fn tiff_header(tiff: &[u8]) -> Option<(Tiff, usize)> {
    let order = match tiff.get(..2)? {
        b"II" => Tiff(true),
        b"MM" => Tiff(false),
        _ => return None,
    };
    Some((order, order.u32(tiff, 4)? as usize))
}

const ORIENTATION_TAG: u16 = 0x0112;

/// Offset (within the TIFF block) of IFD0's orientation entry.
fn orientation_entry(tiff: &[u8]) -> Option<(Tiff, usize)> {
    let (order, ifd) = tiff_header(tiff)?;
    let n = order.u16(tiff, ifd)? as usize;
    (0..n).map(|i| ifd + 2 + i * 12).find(|&e| order.u16(tiff, e) == Some(ORIENTATION_TAG)).map(|e| (order, e))
}

/// A JPEG's EXIF orientation: 1 when it has none, `None` if it isn't a JPEG.
pub fn jpeg_orientation(data: &[u8]) -> Option<u16> {
    let segs = jpeg_segments(data)?;
    let Some((start, len)) = exif_segment(data, &segs) else { return Some(1) };
    let tiff = &data[start + 10..start + len];
    Some(orientation_entry(tiff).and_then(|(order, e)| order.u16(tiff, e + 8)).filter(|o| (1..=8).contains(o)).unwrap_or(1))
}

/// Returns `data` with its EXIF orientation set to `o`; the image data is
/// copied unchanged. An existing tag is overwritten in place. If the EXIF
/// block has no orientation, a copy of IFD0 with the tag added is appended
/// to the block and the header pointed at it (offsets elsewhere stay valid).
/// A JPEG with no EXIF gets a minimal block after its JFIF header.
pub fn set_jpeg_orientation(data: &[u8], o: u16) -> Option<Vec<u8>> {
    let segs = jpeg_segments(data)?;
    let Some((start, len)) = exif_segment(data, &segs) else {
        let mut seg = vec![0xFF, 0xE1, 0, 0];
        seg.extend_from_slice(b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01");
        seg.extend_from_slice(&ORIENTATION_TAG.to_be_bytes());
        seg.extend_from_slice(&[0, 3, 0, 0, 0, 1]);
        seg.extend_from_slice(&o.to_be_bytes());
        seg.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let n = (seg.len() - 2) as u16;
        seg[2..4].copy_from_slice(&n.to_be_bytes());
        let at = segs.first().filter(|s| s.marker == 0xE0).map_or(2, |s| s.start + s.len);
        return Some([&data[..at], &seg, &data[at..]].concat());
    };
    let tiff_at = start + 10;
    let tiff = &data[tiff_at..start + len];
    if let Some((order, e)) = orientation_entry(tiff) {
        let mut out = data.to_vec();
        let at = tiff_at + e;
        out[at + 2..at + 4].copy_from_slice(&order.put16(3));
        out[at + 4..at + 8].copy_from_slice(&order.put32(1));
        out[at + 8..at + 12].copy_from_slice(&[0; 4]);
        out[at + 8..at + 10].copy_from_slice(&order.put16(o));
        return Some(out);
    }
    let (order, ifd) = tiff_header(tiff)?;
    let n = order.u16(tiff, ifd)? as usize;
    let entries = tiff.get(ifd + 2..ifd + 2 + n * 12)?;
    let next = tiff.get(ifd + 2 + n * 12..ifd + 6 + n * 12)?;
    let mut new_entry = Vec::with_capacity(12);
    new_entry.extend_from_slice(&order.put16(ORIENTATION_TAG));
    new_entry.extend_from_slice(&order.put16(3));
    new_entry.extend_from_slice(&order.put32(1));
    new_entry.extend_from_slice(&order.put16(o));
    new_entry.extend_from_slice(&[0, 0]);
    // Entries stay sorted by tag, as TIFF asks.
    let pos = (0..n).find(|&i| order.u16(entries, i * 12).is_some_and(|t| t > ORIENTATION_TAG)).unwrap_or(n);
    let mut block = tiff.to_vec();
    if block.len() % 2 == 1 {
        block.push(0);
    }
    let new_ifd = block.len();
    block.extend_from_slice(&order.put16(n as u16 + 1));
    block.extend_from_slice(&entries[..pos * 12]);
    block.extend_from_slice(&new_entry);
    block.extend_from_slice(&entries[pos * 12..]);
    block.extend_from_slice(next);
    block[4..8].copy_from_slice(&order.put32(new_ifd as u32));
    let seg_len = 2 + 6 + block.len();
    if seg_len > 0xFFFF {
        return None;
    }
    let mut seg = vec![0xFF, 0xE1];
    seg.extend_from_slice(&(seg_len as u16).to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(&block);
    Some([&data[..start], &seg, &data[start + len..]].concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    /// A noisy picture, so the encoded file is large and colourful.
    fn picture(w: u32, h: u32) -> RgbImage {
        let mut rng = fastrand::Rng::with_seed(7);
        RgbImage::from_fn(w, h, |x, y| {
            let n = rng.u8(..40);
            Rgb([((x * 255) / w) as u8 ^ n, ((y * 255) / h) as u8, if x < w / 2 { 200 } else { n }])
        })
    }

    fn encode(img: &RgbImage, fmt: ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), fmt).unwrap();
        out
    }

    fn library_with(file: &str, bytes: &[u8]) -> (tempfile::TempDir, Library, String) {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
        let src = tmp.path().join(file);
        fs::write(&src, bytes).unwrap();
        let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
        (tmp, lib, id)
    }

    fn keys(lib: &Library, id: &str) -> Vec<String> {
        let item = lib.item(id).unwrap();
        let text = fs::read_to_string(item.info_dir(lib.root()).join("metadata.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v.as_object().unwrap().keys().cloned().collect()
    }

    fn thumb_size(lib: &Library, id: &str) -> Option<(u32, u32)> {
        let item = lib.item(id).unwrap();
        item.thumbnail_path(lib.root()).map(|p| thumbnail::decode(&p).unwrap()).map(|i| (i.width(), i.height()))
    }

    #[test]
    fn orientation_turns() {
        assert_eq!(turn_orientation(1, 1), 6);
        assert_eq!(turn_orientation(1, 3), 8);
        assert_eq!(turn_orientation(6, 3), 1);
        assert_eq!(turn_orientation(3, 2), 1);
        assert_eq!(turn_orientation(2, 1), 7);
        assert_eq!(turn_orientation(4, 1), 5);
        assert_eq!(turn_orientation(5, 1), 2);
        assert_eq!(turn_orientation(5, 3), 4);
        for o in 1..=8 {
            assert_eq!(turn_orientation(turn_orientation(o, 1), 3), o);
        }
    }

    #[test]
    fn flip_orientations_match_eagles_tables() {
        // Eagle's lookup tables for orientations 1 to 8.
        let horizontal = [2, 1, 4, 3, 6, 5, 8, 7];
        let vertical = [4, 3, 2, 1, 8, 7, 6, 5];
        for o in 1..=8u16 {
            assert_eq!(flip_orientation(o, Flip::Horizontal), horizontal[o as usize - 1], "H {o}");
            assert_eq!(flip_orientation(o, Flip::Vertical), vertical[o as usize - 1], "V {o}");
        }
    }

    #[test]
    fn flipping_a_png_mirrors_its_pixels() {
        let src = picture(600, 400);
        let png = encode(&src, ImageFormat::Png);
        let (_tmp, mut lib, id) = library_with("wide.png", &png);
        let before = lib.item(&id).unwrap().clone();
        let order = keys(&lib, &id);
        lib.begin_transaction("Flip");
        lib.flip_item(&id, Flip::Horizontal).unwrap();
        assert!(lib.end_transaction().unwrap().irreversible);
        let after = lib.item(&id).unwrap().clone();
        assert_eq!((after.width, after.height), (600, 400));
        assert!(after.last_modified > before.last_modified && !after.palettes.is_empty());
        assert_eq!(keys(&lib, &id), order);
        let img = thumbnail::decode(&after.original_path(lib.root())).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), src.get_pixel(599, 0));
        assert_eq!(img.get_pixel(10, 300), src.get_pixel(589, 300));
        lib.flip_item(&id, Flip::Vertical).unwrap();
        let img = thumbnail::decode(&after.original_path(lib.root())).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0), src.get_pixel(599, 399));
    }

    #[test]
    fn flipping_a_jpeg_only_changes_its_orientation() {
        let src = picture(300, 200);
        let jpg = encode(&src, ImageFormat::Jpeg);
        let (_tmp, mut lib, id) = library_with("photo.jpg", &jpg);
        lib.flip_item(&id, Flip::Horizontal).unwrap();
        let item = lib.item(&id).unwrap().clone();
        let data = fs::read(item.original_path(lib.root())).unwrap();
        assert_eq!(jpeg_orientation(&data), Some(2));
        let pixels = |b: &[u8]| image::load_from_memory(b).unwrap().to_rgb8();
        assert!(pixels(&data).into_raw() == pixels(&jpg).into_raw(), "the compressed image is untouched");
        // Shown mirrored: the left edge now holds what was on the right.
        let shown = thumbnail::decode(&item.original_path(lib.root())).unwrap().to_rgb8();
        let stored = pixels(&jpg);
        assert_eq!(shown.get_pixel(0, 100), stored.get_pixel(299, 100));
        assert_eq!((item.width, item.height), (300, 200));
        lib.flip_item(&id, Flip::Vertical).unwrap();
        assert_eq!(jpeg_orientation(&fs::read(item.original_path(lib.root())).unwrap()), Some(3));
        let gif = encode(&picture(10, 10), ImageFormat::Gif);
        let (_tmp, mut lib, id) = library_with("a.gif", &gif);
        assert!(lib.flip_item(&id, Flip::Horizontal).is_err());
    }

    #[test]
    fn jpeg_orientation_is_written_without_touching_pixels() {
        let jpg = encode(&picture(40, 20), ImageFormat::Jpeg);
        assert_eq!(jpeg_orientation(&jpg), Some(1));
        // No EXIF yet: a block is added.
        let turned = set_jpeg_orientation(&jpg, 6).unwrap();
        assert_eq!(jpeg_orientation(&turned), Some(6));
        // Existing tag: overwritten in place, same length.
        let again = set_jpeg_orientation(&turned, 3).unwrap();
        assert_eq!(again.len(), turned.len());
        assert_eq!(jpeg_orientation(&again), Some(3));
        // EXIF without an orientation tag (a single other tag): IFD0 is copied with it added.
        let mut seg = vec![0xFF, 0xE1, 0, 0];
        seg.extend_from_slice(b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0");
        seg.extend_from_slice(&[0x31, 0x01, 2, 0, 4, 0, 0, 0, b'f', b'a', b'l', 0]);
        seg.extend_from_slice(&[0, 0, 0, 0]);
        let n = (seg.len() - 2) as u16;
        seg[2..4].copy_from_slice(&n.to_be_bytes());
        let with_exif = [&jpg[..2], &seg, &jpg[2..]].concat();
        assert_eq!(jpeg_orientation(&with_exif), Some(1));
        let added = set_jpeg_orientation(&with_exif, 8).unwrap();
        assert_eq!(jpeg_orientation(&added), Some(8));
        let exif = ::exif::Reader::new().read_from_container(&mut Cursor::new(&added)).unwrap();
        assert!(exif.get_field(::exif::Tag::Software, ::exif::In::PRIMARY).is_some(), "other tags survive");
        // The image data after the EXIF block is the same bytes.
        assert!(added.ends_with(&jpg[2..]));
        // Decoders see the turn.
        let img = image::load_from_memory(&added).unwrap();
        assert_eq!((img.width(), img.height()), (40, 20));
        let path = tempfile::tempdir().unwrap();
        let p = path.path().join("a.jpg");
        fs::write(&p, &added).unwrap();
        let shown = thumbnail::decode(&p).unwrap();
        assert_eq!((shown.width(), shown.height()), (20, 40));
    }

    #[test]
    fn animation_is_detected() {
        let png = encode(&picture(4, 4), ImageFormat::Png);
        assert!(!is_animated(&png));
        let mut apng = png[..33].to_vec();
        apng.extend_from_slice(&[0, 0, 0, 8]);
        apng.extend_from_slice(b"acTL\0\0\0\x02\0\0\0\0\0\0\0\0");
        apng.extend_from_slice(&png[33..]);
        assert!(is_animated(&apng));
    }

    #[test]
    fn rotating_a_png_redraws_everything() {
        let png = encode(&picture(1200, 800), ImageFormat::Png);
        let (_tmp, mut lib, id) = library_with("wide.png", &png);
        let before = lib.item(&id).unwrap().clone();
        assert_eq!((before.width, before.height), (1200, 800));
        assert_eq!(thumb_size(&lib, &id), Some((480, 320)));
        let order = keys(&lib, &id);
        lib.rotate_item(&id, -1).unwrap();
        let after = lib.item(&id).unwrap().clone();
        assert_eq!((after.width, after.height), (800, 1200));
        assert_eq!(thumb_size(&lib, &id), Some((320, 480)));
        assert!(!after.palettes.is_empty());
        assert_ne!(after.size, 0);
        assert_eq!(after.modification_time, before.modification_time);
        assert!(after.last_modified > before.last_modified);
        assert_eq!(keys(&lib, &id), order);
        // Anticlockwise: the original's top-right corner is now top-left.
        let img = thumbnail::decode(&after.original_path(lib.root())).unwrap().to_rgb8();
        let src = picture(1200, 800);
        assert_eq!(img.get_pixel(0, 0), src.get_pixel(1199, 0));
    }

    #[test]
    fn rotating_a_jpeg_only_changes_its_orientation() {
        let jpg = encode(&picture(1000, 600), ImageFormat::Jpeg);
        let (_tmp, mut lib, id) = library_with("photo.jpg", &jpg);
        let order = keys(&lib, &id);
        lib.rotate_item(&id, 1).unwrap();
        let item = lib.item(&id).unwrap().clone();
        assert_eq!((item.width, item.height), (600, 1000));
        let data = fs::read(item.original_path(lib.root())).unwrap();
        assert_eq!(jpeg_orientation(&data), Some(6));
        let pixels = |b: &[u8]| image::load_from_memory(b).unwrap().to_rgb8().into_raw();
        assert!(pixels(&data) == pixels(&jpg), "the compressed image is untouched");
        assert_eq!(item.size, data.len() as u64);
        assert_eq!(keys(&lib, &id), order);
        lib.rotate_item(&id, 3).unwrap();
        assert_eq!(jpeg_orientation(&fs::read(item.original_path(lib.root())).unwrap()), Some(1));
        assert_eq!(lib.item(&id).unwrap().width, 1000);
    }

    #[test]
    fn cropping_a_jpeg_and_a_png() {
        let jpg = encode(&picture(1600, 1000), ImageFormat::Jpeg);
        let (_tmp, mut lib, id) = library_with("big.jpg", &jpg);
        assert_eq!(thumb_size(&lib, &id), Some((512, 320)));
        let order = keys(&lib, &id);
        lib.crop_item(&id, PixelRect { x: 100, y: 100, width: 1200, height: 600 }).unwrap();
        let item = lib.item(&id).unwrap().clone();
        assert_eq!((item.width, item.height), (1200, 600));
        assert_eq!(thumb_size(&lib, &id), Some((640, 320)));
        assert!(!item.palettes.is_empty());
        assert_eq!(keys(&lib, &id), order);
        // A crop past the edge is clipped to the image.
        lib.crop_item(&id, PixelRect { x: 1000, y: 0, width: 900, height: 900 }).unwrap();
        assert_eq!((lib.item(&id).unwrap().width, lib.item(&id).unwrap().height), (200, 600));
        assert!(lib.crop_item(&id, PixelRect { x: 5000, y: 0, width: 10, height: 10 }).is_err());

        // A small PNG with transparency: no thumbnail before or after.
        let rgba = RgbaImage::from_fn(300, 200, |x, _| Rgba([200, (x % 255) as u8, 30, if x < 150 { 255 } else { 0 }]));
        let mut png = Vec::new();
        rgba.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let (_tmp2, mut lib, id) = library_with("small.png", &png);
        let order = keys(&lib, &id);
        lib.crop_item(&id, PixelRect { x: 0, y: 0, width: 150, height: 100 }).unwrap();
        let item = lib.item(&id).unwrap();
        assert_eq!((item.width, item.height, item.no_thumbnail), (150, 100, Some(true)));
        assert_eq!(thumb_size(&lib, &id), None);
        assert_eq!(keys(&lib, &id), order);
        let img = thumbnail::decode(&item.original_path(lib.root())).unwrap();
        assert!(img.color().has_alpha());
    }

    #[test]
    fn crop_save_as_adds_an_item_and_keeps_the_original() {
        let jpg = encode(&picture(1600, 1000), ImageFormat::Jpeg);
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
        let folder = lib.create_folder(None, "F").unwrap();
        let src = tmp.path().join("big.jpg");
        fs::write(&src, &jpg).unwrap();
        let opts = ImportOptions { folders: vec![folder.clone()], tags: vec!["t".into()], url: "https://example.com/x".into(), annotation: "note".into(), ..Default::default() };
        let id = lib.import_file(&src, &opts).unwrap();
        let mut rated = lib.item(&id).unwrap().clone();
        rated.extra.insert("star".into(), 4.into());
        lib.update_item(rated).unwrap();
        let before = lib.item(&id).unwrap().clone();
        let original_bytes = fs::read(before.original_path(lib.root())).unwrap();
        let meta_before = fs::read(before.info_dir(lib.root()).join("metadata.json")).unwrap();

        let new_id = lib.crop_item_as_new(&id, PixelRect { x: 100, y: 100, width: 800, height: 400 }).unwrap();
        assert_ne!(new_id, id);
        assert_eq!(fs::read(before.original_path(lib.root())).unwrap(), original_bytes, "the original file is untouched");
        assert_eq!(fs::read(before.info_dir(lib.root()).join("metadata.json")).unwrap(), meta_before, "and so is its record");
        let copy = lib.item(&new_id).unwrap().clone();
        assert_eq!((copy.width, copy.height), (800, 400));
        assert_eq!((&copy.name, &copy.ext, &copy.folders, &copy.tags, &copy.url), (&before.name, &before.ext, &before.folders, &before.tags, &before.url));
        assert_eq!(copy.annotation, "", "Eagle doesn't carry the note over");
        assert_eq!(copy.extra.get("star"), Some(&serde_json::Value::from(4)));
        assert_eq!(copy.modification_time, before.modification_time + 1);
        assert!(!copy.palettes.is_empty());
        // Saved like any item, and found again on reopening.
        let again = Library::open(lib.root()).unwrap();
        assert_eq!(again.item(&new_id).unwrap().width, 800);
    }

    fn names_in(dir: &Path) -> Vec<String> {
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn atomic_replace_swaps_the_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.png");
        fs::write(&p, b"old").unwrap();
        replace_atomically(&p, b"brand new").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"brand new");
        assert_eq!(names_in(tmp.path()), vec!["x.png".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replace_renames_instead_of_truncating() {
        use std::io::Read;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.png");
        fs::write(&p, b"old bytes").unwrap();
        let mut held = fs::File::open(&p).unwrap();
        replace_atomically(&p, b"new").unwrap();
        let mut seen = Vec::new();
        held.read_to_end(&mut seen).unwrap();
        assert_eq!(seen, b"old bytes");
        assert_eq!(fs::read(&p).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replace_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.png");
        fs::write(&p, b"old").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        replace_atomically(&p, b"new").unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn atomic_replace_of_a_missing_file_fails_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("gone.png");
        assert!(replace_atomically(&p, b"new").is_err());
        assert!(names_in(tmp.path()).is_empty());
    }

    #[test]
    fn rewriting_leaves_no_backup_behind() {
        let png = encode(&picture(300, 200), ImageFormat::Png);
        let (_tmp, mut lib, id) = library_with("a.png", &png);
        lib.crop_item(&id, PixelRect { x: 0, y: 0, width: 100, height: 100 }).unwrap();
        let dir = lib.item(&id).unwrap().info_dir(lib.root());
        let names: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().all(|n| !n.ends_with(".bk") && !n.ends_with(".staged")), "{names:?}");
    }

    #[test]
    fn cropping_to_a_small_image_drops_the_thumbnail_file() {
        let jpg = encode(&picture(1600, 1000), ImageFormat::Jpeg);
        let (_tmp, mut lib, id) = library_with("big.jpg", &jpg);
        assert!(thumb_size(&lib, &id).is_some());
        lib.crop_item(&id, PixelRect { x: 0, y: 0, width: 400, height: 300 }).unwrap();
        let item = lib.item(&id).unwrap();
        assert_eq!(item.no_thumbnail, Some(true));
        assert_eq!(thumb_size(&lib, &id), None, "a stale thumbnail would be shown instead of the original");
    }

    #[test]
    fn edits_are_irreversible_and_unsupported_formats_refused() {
        let png = encode(&picture(100, 100), ImageFormat::Png);
        let (_tmp, mut lib, id) = library_with("a.png", &png);
        lib.begin_transaction("Rotate");
        lib.rotate_item(&id, 1).unwrap();
        assert!(lib.end_transaction().unwrap().irreversible);
        let gif = encode(&picture(10, 10), ImageFormat::Gif);
        let (_tmp, mut lib, id) = library_with("a.gif", &gif);
        assert!(lib.rotate_item(&id, 1).is_err());
        assert!(lib.crop_item(&id, PixelRect { x: 0, y: 0, width: 5, height: 5 }).is_err());
    }
}
