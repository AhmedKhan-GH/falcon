//! Affinity documents (`.af`, `.afdesign`, `.afphoto`, `.afpub`, `.aftemplate`):
//! Serif `nsrP` containers whose `Thmb` block holds a PNG preview (usually up to
//! 512 px on its long side).

use std::fs::File;
use std::path::Path;

use image::DynamicImage;

use super::{MAX_ITEM, decode, read_at, read_head};

const MAGIC: &[u8; 4] = b"\x00\xffKA";
const FORMAT_TAG: &[u8; 8] = b"nsrP#Inf";
const THMB_TAG: &[u8; 8] = b"\xff\xff\xff\xffThmb";
const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const THMB_HEADER: usize = 29;
const TAIL: u64 = 2 << 20;

/// Extracts the PNG preview inside an Affinity file. Checks the `Thmb` offset
/// stated at byte 24 of the `nsrP#Inf` header first, then scans the last
/// [`TAIL`] bytes in case an older file omits the pointer.
pub(super) fn read(mut file: File, _path: &Path) -> Option<DynamicImage> {
    let len = file.metadata().ok()?.len();
    let head = read_head(&mut file, 32)?;
    if head.get(..4)? != MAGIC || head.get(8..16)? != FORMAT_TAG {
        return None;
    }
    let off = u64::from_le_bytes(head.get(24..32)?.try_into().ok()?);
    if let Some(img) = read_thmb_at(&mut file, len, off) {
        return Some(img);
    }
    let tail_start = len.saturating_sub(TAIL);
    let tail = read_at(&mut file, len, tail_start, len - tail_start, TAIL as usize)?;
    let at = tail.windows(THMB_TAG.len()).rposition(|w| w == THMB_TAG)?;
    decode_thmb_slice(tail.get(at..)?)
}

fn read_thmb_at(file: &mut File, file_len: u64, off: u64) -> Option<DynamicImage> {
    let hdr = read_at(file, file_len, off, THMB_HEADER as u64, MAX_ITEM)?;
    let png_len = thmb_png_len(&hdr)?;
    let payload_off = off.checked_add(THMB_HEADER as u64)?;
    let png = read_at(file, file_len, payload_off, png_len, MAX_ITEM)?;
    decode_png(&png)
}

fn decode_thmb_slice(slice: &[u8]) -> Option<DynamicImage> {
    let png_len = usize::try_from(thmb_png_len(slice)?).ok()?;
    let end = THMB_HEADER.checked_add(png_len)?;
    decode_png(slice.get(THMB_HEADER..end)?)
}

/// Validates a `Thmb` header and returns the embedded PNG length (`u32` at +24).
fn thmb_png_len(hdr: &[u8]) -> Option<u64> {
    if hdr.len() < THMB_HEADER || hdr.get(..8)? != THMB_TAG {
        return None;
    }
    let png_len = u64::from(u32::from_le_bytes(hdr.get(24..28)?.try_into().ok()?));
    (png_len > 0 && png_len <= MAX_ITEM as u64).then_some(png_len)
}

fn decode_png(bytes: &[u8]) -> Option<DynamicImage> {
    if !bytes.starts_with(PNG_SIG) {
        return None;
    }
    decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([20, 140, 220, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    fn thmb_chunk(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(THMB_TAG);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32 + 13).to_le_bytes());
        out.extend_from_slice(&0x1du64.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.push(1);
        out.extend_from_slice(payload);
        out
    }

    fn affinity_file(padding: usize, thmb_override: Option<u64>, payload: Option<&[u8]>) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&12u32.to_le_bytes());
        out.extend_from_slice(FORMAT_TAG);
        out.extend_from_slice(&0u64.to_le_bytes());
        let thmb_pos_field = out.len();
        out.extend_from_slice(&0u64.to_le_bytes());
        out.resize(32 + padding, 0x55);
        let actual_off = if let Some(p) = payload {
            let at = out.len() as u64;
            out.extend_from_slice(&thmb_chunk(p));
            at
        } else {
            out.extend_from_slice(b"#Filno-thumb");
            32
        };
        let ptr = thmb_override.unwrap_or(actual_off);
        out[thmb_pos_field..thmb_pos_field + 8].copy_from_slice(&ptr.to_le_bytes());
        out
    }

    fn read_bytes(bytes: &[u8]) -> Option<DynamicImage> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("doc.af");
        std::fs::write(&p, bytes).unwrap();
        read(File::open(&p).unwrap(), &p)
    }

    #[test]
    fn reads_embedded_thmb_png_by_header_offset() {
        let bytes = affinity_file(256, None, Some(&png(84, 60)));
        let img = read_bytes(&bytes).expect("decoded Affinity preview");
        assert_eq!((img.width(), img.height()), (84, 60));
    }

    #[test]
    fn falls_back_to_tail_scan_when_pointer_misses() {
        let bytes = affinity_file(128, Some(32), Some(&png(40, 30)));
        let img = read_bytes(&bytes).expect("tail fallback finds Thmb");
        assert_eq!((img.width(), img.height()), (40, 30));
    }

    #[test]
    fn missing_wrong_magic_or_corrupt_returns_none() {
        assert!(read_bytes(b"").is_none());
        assert!(read_bytes(b"not affinity").is_none());
        // Document saved without a Thmb block (#Fil instead).
        assert!(read_bytes(&affinity_file(64, None, None)).is_none());
        // Thmb payload that is not a PNG.
        assert!(read_bytes(&affinity_file(64, None, Some(b"not a png at all"))).is_none());
        // Truncated PNG payload.
        let p = png(32, 32);
        assert!(read_bytes(&affinity_file(64, None, Some(&p[..p.len() / 2]))).is_none());
        // Out-of-bounds offset.
        let mut oob = affinity_file(64, None, None);
        oob[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(read_bytes(&oob).is_none());
        // Excessive claimed PNG length (> MAX_ITEM) and zero PNG length: rejected before reading.
        let mut huge = affinity_file(64, None, Some(&p));
        let off = u64::from_le_bytes(huge[24..32].try_into().unwrap()) as usize;
        huge[off + 24..off + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_bytes(&huge).is_none());
        huge[off + 24..off + 28].copy_from_slice(&0u32.to_le_bytes());
        assert!(read_bytes(&huge).is_none());
        // Corrupt PNG IDAT payload after valid IHDR: rejected cleanly by decoder.
        let mut bad_png = p.clone();
        let mid = bad_png.len() / 2;
        bad_png[mid] ^= 0xFF;
        assert!(read_bytes(&affinity_file(64, None, Some(&bad_png))).is_none());
    }
}
