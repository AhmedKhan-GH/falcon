//! The thumbnail stored in a Blender file's `TEST` block.
//!
//! Plain and gzip-compressed files are read. Zstd-compressed files (magic
//! `28 B5 2F FD`, used by Blender 3.0 and later when compression is on) are
//! not: no zstd decompressor is in the dependency tree, so they give `None`.

use std::fs::File;
use std::path::Path;

use image::{DynamicImage, RgbaImage};

use super::{read_head, HEAD};

/// The longest side of a thumbnail that is accepted.
const MAX_SIDE: u32 = 4096;

pub(super) fn read(mut file: File, _path: &Path) -> Option<DynamicImage> {
    let head = read_head(&mut file, HEAD)?;
    let data = if head.starts_with(&[0x1F, 0x8B]) { gunzip_prefix(&head)? } else { head };
    thumbnail(&data)
}

/// The first bytes of a gzip stream's content, from a prefix of the stream.
fn gunzip_prefix(gz: &[u8]) -> Option<Vec<u8>> {
    let flags = *gz.get(3)?;
    let mut at = 10usize;
    if flags & 4 != 0 {
        let n = u16::from_le_bytes(gz.get(at..at.checked_add(2)?)?.try_into().ok()?);
        at = at.checked_add(2)?.checked_add(usize::from(n))?;
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            let end = gz.get(at..)?.iter().position(|&b| b == 0)?;
            at = at.checked_add(end)?.checked_add(1)?;
        }
    }
    if flags & 2 != 0 {
        at = at.checked_add(2)?;
    }
    let deflate = gz.get(at..)?;
    inflate_prefix(deflate, HEAD as usize)
}

/// Up to `max` bytes inflated from `input`. A cut or over-long stream still
/// gives what was inflated before the cut; no output at all gives `None`.
fn inflate_prefix(mut input: &[u8], max: usize) -> Option<Vec<u8>> {
    use miniz_oxide::inflate::core::{decompress, inflate_flags, DecompressorOxide};
    use miniz_oxide::inflate::TINFLStatus;
    let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let mut out = vec![0u8; input.len().saturating_mul(2).clamp(1, max)];
    let mut state = Box::<DecompressorOxide>::default();
    let mut pos = 0usize;
    loop {
        let (status, used, wrote) = decompress(&mut state, input, &mut out, pos, flags);
        pos = pos.saturating_add(wrote).min(out.len());
        match status {
            TINFLStatus::HasMoreOutput if out.len() < max => {
                input = input.get(used..)?;
                let grown = out.len().saturating_mul(2).min(max);
                out.resize(grown, 0);
            }
            _ => break,
        }
    }
    out.truncate(pos);
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn thumbnail(d: &[u8]) -> Option<DynamicImage> {
    let hdr = d.get(..9)?;
    if &hdr[..7] != b"BLENDER" {
        return None;
    }
    // "BLENDER17-01v0502" (header size, format version, Blender version) has
    // 32-byte block headers; "BLENDER-v305" and "BLENDER_v305" (pointer
    // size, endianness) have 16 bytes plus the pointer size.
    let (mut at, big, ptr, wide) = if hdr[7].is_ascii_digit() {
        let n: usize = std::str::from_utf8(&hdr[7..9]).ok()?.parse().ok()?;
        (n, false, 8usize, true)
    } else {
        (12, hdr[8] == b'V', if hdr[7] == b'_' { 4 } else { 8 }, false)
    };
    let u32_at = |p: usize| -> Option<u32> {
        let b: [u8; 4] = d.get(p..p.checked_add(4)?)?.try_into().ok()?;
        Some(if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    };
    let u64_at = |p: usize| -> Option<u64> {
        Some(u64::from_le_bytes(d.get(p..p.checked_add(8)?)?.try_into().ok()?))
    };
    while at < d.len() {
        let code = d.get(at..at.checked_add(4)?)?;
        let (len, hlen) = if wide {
            (u64_at(at.checked_add(16)?)?, 32usize)
        } else {
            (u64::from(u32_at(at.checked_add(4)?)?), 16usize.checked_add(ptr)?)
        };
        let body = at.checked_add(hlen)?;
        if code == b"ENDB" {
            return None;
        }
        if code == b"TEST" {
            let (w, h) = (u32_at(body)?, u32_at(body.checked_add(4)?)?);
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                return None;
            }
            let n = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
            if len < (n as u64).checked_add(8)? {
                return None;
            }
            let start = body.checked_add(8)?;
            let px = d.get(start..start.checked_add(n)?)?;
            let mut img = RgbaImage::from_raw(w, h, px.to_vec())?;
            // Rows are stored from the bottom.
            image::imageops::flip_vertical_in_place(&mut img);
            return Some(DynamicImage::ImageRgba8(img));
        }
        at = body.checked_add(usize::try_from(len).ok()?)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    /// A blend with the given header style and a REND, TEST and ENDB block.
    fn blend_file(new: bool, big: bool, w: u32, h: u32) -> Vec<u8> {
        let mut out = if new {
            b"BLENDER17-01v0502".to_vec()
        } else if big {
            b"BLENDER-V305".to_vec()
        } else {
            b"BLENDER_v305".to_vec()
        };
        let ptr = if !new && !big { 4 } else { 8 };
        let put = |v: u32| if big { v.to_be_bytes() } else { v.to_le_bytes() };
        let block = |out: &mut Vec<u8>, code: &[u8; 4], body: &[u8]| {
            out.extend(code);
            if new {
                out.extend(0u32.to_le_bytes());
                out.extend(0x10u64.to_le_bytes());
                out.extend((body.len() as u64).to_le_bytes());
                out.extend(1u64.to_le_bytes());
            } else {
                out.extend(put(body.len() as u32));
                out.extend(vec![0u8; ptr]);
                out.extend(put(0));
                out.extend(put(1));
            }
            out.extend(body);
        };
        block(&mut out, b"REND", &[1u8; 16]);
        let mut test = Vec::new();
        test.extend(put(w));
        test.extend(put(h));
        for y in 0..h {
            for _ in 0..w {
                test.extend([y as u8, 0, 0, 255]);
            }
        }
        block(&mut out, b"TEST", &test);
        block(&mut out, b"ENDB", &[]);
        out
    }

    fn run(bytes: &[u8]) -> Option<DynamicImage> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.blend");
        std::fs::write(&p, bytes).unwrap();
        read(File::open(&p).unwrap(), &p)
    }

    fn size(bytes: &[u8]) -> Option<(u32, u32)> {
        run(bytes).map(|i| i.dimensions())
    }

    #[test]
    fn thumbnail_is_read_upright_in_each_header_style() {
        for (name, new, big) in [("new", true, false), ("old4", false, false), ("big8", false, true)] {
            let img = run(&blend_file(new, big, 5, 3)).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(img.dimensions(), (5, 3), "{name}");
            // Rows are stored bottom first: the last stored row (2) is on top.
            assert_eq!(img.to_rgba8().get_pixel(0, 0).0[0], 2, "{name}");
            assert_eq!(img.to_rgba8().get_pixel(0, 2).0[0], 0, "{name}");
        }
    }

    #[test]
    fn gzip_files_are_read_and_cut_ones_keep_the_start() {
        let raw = blend_file(true, false, 4, 4);
        let deflated = miniz_oxide::deflate::compress_to_vec(&raw, 6);
        let mut gz = vec![0x1F, 0x8B, 8, 8, 0, 0, 0, 0, 0, 3];
        gz.extend(b"name\0");
        gz.extend(&deflated);
        assert_eq!(size(&gz), Some((4, 4)));
        // Compressed data cut short still has the start.
        let mut cut = blend_file(true, false, 64, 64);
        cut.extend((0..20000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8));
        let mut gz = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
        gz.extend(miniz_oxide::deflate::compress_to_vec(&cut, 1));
        assert_eq!(size(&gz), Some((64, 64)));
        assert_eq!(size(&gz[..gz.len() - 300]), Some((64, 64)));
        assert!(run(&[0x1F, 0x8B, 8, 4, 0, 0, 0, 0, 0, 3, 0xFF, 0xFF]).is_none());
        assert!(run(&[0x1F, 0x8B, 8, 4]).is_none());
    }

    #[test]
    fn zstd_empty_short_and_noise_files_give_nothing() {
        assert!(run(&[0x28, 0xB5, 0x2F, 0xFD, 0, 0, 0, 0]).is_none());
        assert!(run(b"").is_none());
        assert!(run(b"BLENDER").is_none());
        assert!(run(b"BLENDER-").is_none());
        assert!(run(&[0xAB; 400]).is_none());
        assert!(run(b"BLENDER_v305").is_none());
    }

    #[test]
    fn bad_sizes_and_lengths_give_nothing() {
        // Dimensions over the cap, and larger than the block.
        let mut big = blend_file(true, false, 4, 4);
        let dims = 17 + 32 + 16 + 32;
        big[dims..dims + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(run(&big).is_none());
        big[dims..dims + 4].copy_from_slice(&4097u32.to_le_bytes());
        assert!(run(&big).is_none());
        big[dims..dims + 4].copy_from_slice(&8u32.to_le_bytes());
        assert!(run(&big).is_none());
        // The data ends inside the pixels.
        let cut = blend_file(false, false, 4, 4);
        assert!(run(&cut[..cut.len() - 40]).is_none());
        // A block length past the end, huge or not wrapping.
        for len in [u64::MAX, u64::MAX - 20, u64::MAX / 2, 1 << 40, 1 << 20] {
            let mut b = blend_file(true, false, 4, 4);
            b[17 + 16..17 + 24].copy_from_slice(&len.to_le_bytes());
            assert!(run(&b).is_none(), "{len}");
        }
    }
}
