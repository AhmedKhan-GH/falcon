//! AutoCAD drawings: the preview in the file's image section, which the
//! seeker at byte 13 points to. The section starts with a 16-byte sentinel,
//! then a u32 size, a u8 entry count and 9-byte entries (code, start, size).
//! Code 2 is a DIB (a BMP without its file header) and 6 is a PNG. WMF
//! previews (code 3) are not read.

use std::fs::File;
use std::path::Path;

use image::DynamicImage;

use super::{decode, le32, read_at, read_head, MAX_ITEM};

const SENTINEL: [u8; 16] = [0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B];
const PNG_MAGIC: &[u8] = b"\x89PNG";
/// The sentinel, the size, the count and the most entries a count can hold.
const SECTION_MAX: u64 = 16 + 4 + 1 + 9 * 255;

pub(super) fn read(mut file: File, _path: &Path) -> Option<DynamicImage> {
    let len = file.metadata().ok()?.len();
    let head = read_head(&mut file, 0x11)?;
    if !head.starts_with(b"AC10") {
        return None;
    }
    let seeker = u64::from(le32(&head, 0x0D)?);
    let section = read_at(&mut file, len, seeker, len.checked_sub(seeker)?.min(SECTION_MAX), MAX_ITEM)?;
    if section.get(..16)? != SENTINEL {
        return None;
    }
    let count = usize::from(*section.get(20)?);
    for i in 0..count {
        let at = 21 + i * 9;
        let (Some(&code), Some(start), Some(size)) = (section.get(at), le32(&section, at + 1), le32(&section, at + 5)) else {
            break;
        };
        if code != 2 && code != 6 {
            continue;
        }
        let Some(bytes) = read_at(&mut file, len, start.into(), size.into(), MAX_ITEM) else { continue };
        let img = if bytes.starts_with(PNG_MAGIC) { decode(&bytes) } else { dib(&bytes) };
        if img.is_some() {
            return img;
        }
    }
    None
}

/// Puts a BMP file header in front of a DIB and decodes it.
fn dib(d: &[u8]) -> Option<DynamicImage> {
    let header = usize::try_from(le32(d, 0)?).ok()?;
    if !(40..=124).contains(&header) || d.len() < header {
        return None;
    }
    let bits = u16::from_le_bytes(d.get(14..16)?.try_into().ok()?);
    let compression = le32(d, 16)?;
    let used = usize::try_from(le32(d, 32)?).ok()?.min(256);
    // Masks follow a 40-byte header only; later headers hold them inside.
    let masks = match (header, compression) {
        (40, 3) => 12,
        (40, 6) => 16,
        _ => 0,
    };
    // Indexed images have a palette; deeper ones only when a count is stated.
    let colors = match bits {
        0 => 0,
        1..=8 if used == 0 => 1usize << bits,
        1..=8 => used.min(1usize << bits),
        _ => used,
    };
    let offset = 14usize.checked_add(header)?.checked_add(masks)?.checked_add(colors.checked_mul(4)?)?;
    if offset - 14 > d.len() {
        return None;
    }
    let total = d.len().checked_add(14)?;
    let mut bmp = Vec::with_capacity(total);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&u32::try_from(total).ok()?.to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
    bmp.extend_from_slice(d);
    decode(&bmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use std::io::Write;

    fn encoded(img: DynamicImage, fmt: image::ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), fmt).unwrap();
        out
    }
    fn png(w: u32, h: u32) -> Vec<u8> {
        encoded(DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([10, 200, 30]))), image::ImageFormat::Png)
    }
    fn dib_of(w: u32, h: u32) -> Vec<u8> {
        let bmp = encoded(DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([1, 2, 3]))), image::ImageFormat::Bmp);
        bmp[14..].to_vec()
    }
    /// An 8-bit indexed DIB with a 256-entry palette.
    fn dib_8bit(w: u32, h: u32) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(40u32.to_le_bytes());
        d.extend(w.to_le_bytes());
        d.extend(h.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(8u16.to_le_bytes());
        d.extend([0u8; 24]); // no compression, size, resolution, counts
        for i in 0..256u32 {
            d.extend([i as u8, 0, 0, 0]);
        }
        let row = (w as usize).div_ceil(4) * 4;
        d.extend(vec![7u8; row * h as usize]);
        d
    }
    fn dwg_file(entries: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"AC1018\0\0\0\0\0\0\0".to_vec();
        out.extend(0x20u32.to_le_bytes());
        out.resize(0x20, 0);
        out.extend(SENTINEL);
        out.extend(0u32.to_le_bytes());
        out.push(entries.len() as u8);
        let mut at = 0x20 + 21 + entries.len() * 9;
        for (code, body) in entries {
            out.push(*code);
            out.extend((at as u32).to_le_bytes());
            out.extend((body.len() as u32).to_le_bytes());
            at += body.len();
        }
        for (_, body) in entries {
            out.extend(body);
        }
        out
    }
    fn size_of(bytes: &[u8]) -> Option<(u32, u32)> {
        let mut f = tempfile::tempfile().unwrap();
        f.write_all(bytes).unwrap();
        std::io::Seek::rewind(&mut f).unwrap();
        read(f, Path::new("x.dwg")).map(|i| i.dimensions())
    }

    #[test]
    fn reads_a_dib_preview() {
        assert_eq!(size_of(&dwg_file(&[(1, vec![0; 80]), (2, dib_of(16, 9))])), Some((16, 9)));
    }

    #[test]
    fn reads_a_palette_dib_preview() {
        assert_eq!(size_of(&dwg_file(&[(2, dib_8bit(10, 6))])), Some((10, 6)));
    }

    #[test]
    fn reads_a_png_preview_and_skips_a_wmf() {
        assert_eq!(size_of(&dwg_file(&[(1, vec![0; 80]), (3, vec![1; 50]), (6, png(20, 30))])), Some((20, 30)));
    }

    #[test]
    fn a_wmf_alone_gives_nothing() {
        assert!(size_of(&dwg_file(&[(3, vec![1; 50])])).is_none());
    }

    #[test]
    fn malformed_files_give_nothing() {
        let good = dwg_file(&[(2, dib_of(8, 8))]);
        assert!(size_of(&good).is_some());
        let mut cases: Vec<Vec<u8>> = vec![
            vec![],
            b"AC1018".to_vec(),
            vec![0; 100],
            good[..good.len() - 30].to_vec(),
            good[..0x28].to_vec(),
        ];
        let mut seeker = good.clone();
        seeker[13..17].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(seeker);
        let mut huge = good.clone();
        huge[0x20 + 26..0x20 + 30].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(huge);
        let mut sentinel = good.clone();
        sentinel[0x20] ^= 0xFF;
        cases.push(sentinel);
        let mut count = dwg_file(&[(1, vec![0; 20])]);
        count[0x20 + 20] = 255;
        cases.push(count);
        cases.push(dwg_file(&[(2, vec![0xAA; 200])]));
        let mut wide = dib_of(8, 8);
        wide[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(dwg_file(&[(2, wide)]));
        for (i, c) in cases.iter().enumerate() {
            assert!(size_of(c).is_none(), "case {i}");
        }
    }

    #[test]
    fn odd_bit_depths_do_not_panic() {
        for bits in [0u16, 1, 4, 8, 16, 24, 32, 255, u16::MAX] {
            for used in [0u32, 1, 256, u32::MAX] {
                let mut d = dib_8bit(4, 4);
                d[14..16].copy_from_slice(&bits.to_le_bytes());
                d[32..36].copy_from_slice(&used.to_le_bytes());
                let _ = dib(&d);
                let _ = size_of(&dwg_file(&[(2, d)]));
            }
        }
    }

    #[test]
    fn a_header_size_past_the_data_gives_nothing() {
        let mut d = dib_of(4, 4);
        d[0..4].copy_from_slice(&124u32.to_le_bytes());
        d.truncate(100);
        assert!(dib(&d).is_none());
    }
}
