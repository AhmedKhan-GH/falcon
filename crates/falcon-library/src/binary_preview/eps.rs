//! EPS and PostScript previews: the TIFF of a DOS EPS binary header, the
//! `%%BeginPreview` bitmap (EPSI), and Illustrator's `%AI7_Thumbnail` block.

use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::path::Path;

use image::DynamicImage;

use super::{decode, find, le32, read_at, read_head, HEAD, MAX_ITEM};

/// The longest side of a preview bitmap read.
const MAX_SIDE: u32 = 8192;
/// The most pixels in a preview bitmap read.
const MAX_PIXELS: u64 = 16 << 20;

pub(super) fn read(mut file: File, _path: &Path) -> Option<DynamicImage> {
    let len = file.metadata().ok()?.len();
    let head = read_head(&mut file, 30)?;
    if head.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) {
        let (ps_off, ps_len) = (le32(&head, 4)?, le32(&head, 8)?);
        let (tiff_off, tiff_len) = (le32(&head, 20)?, le32(&head, 24)?);
        if tiff_len > 0
            && let Some(img) = read_at(&mut file, len, tiff_off.into(), tiff_len.into(), MAX_ITEM).and_then(|b| decode(&b))
        {
            return Some(img);
        }
        let ps = read_at(&mut file, len, ps_off.into(), u64::from(ps_len).min(HEAD), MAX_ITEM)?;
        return ps_preview(&ps);
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    ps_preview(&read_head(&mut file, HEAD)?)
}

/// The preview in PostScript text: an EPSI bitmap, else an Illustrator thumbnail.
fn ps_preview(ps: &[u8]) -> Option<DynamicImage> {
    epsi(ps).or_else(|| ai7_thumbnail(ps))
}

/// Width and height from the numbers of a header line, when inside the limits.
fn dims(w: u32, h: u32) -> Option<(u32, u32)> {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    Some((w, h))
}

/// The numbers after `marker` on its line, and the text from the line's end.
/// The line has to end inside `ps`.
fn header_line<'a>(ps: &'a [u8], marker: &[u8]) -> Option<(Vec<u32>, &'a [u8])> {
    let at = find(ps, marker)?;
    let rest = ps.get(at.checked_add(marker.len())?..)?;
    let eol = rest.iter().position(|&b| b == b'\n' || b == b'\r')?;
    let nums = std::str::from_utf8(&rest[..eol]).ok()?.split_whitespace().map_while(|t| t.parse().ok()).collect();
    Some((nums, &rest[eol..]))
}

fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&b| b == b'\n' || b == b'\r').map(|l| l.trim_ascii())
}

/// The grey bitmap of an `%%BeginPreview: width height depth lines` block:
/// hex lines behind `%`, rows starting on a byte boundary, 0 black.
fn epsi(ps: &[u8]) -> Option<DynamicImage> {
    let (nums, rest) = header_line(ps, b"%%BeginPreview:")?;
    let [w, h, depth, ..] = nums[..] else { return None };
    let (w, h) = dims(w, h)?;
    if !matches!(depth, 1 | 2 | 4 | 8) {
        return None;
    }
    let row_bytes = (w as usize * depth as usize).div_ceil(8);
    let need = row_bytes * h as usize;
    let mut data = Vec::with_capacity(need);
    let mut half: Option<u8> = None;
    'lines: for line in lines(rest) {
        if line.starts_with(b"%%EndPreview") {
            break;
        }
        let Some(hex) = line.strip_prefix(b"%") else { continue };
        for &c in hex {
            let Some(v) = (c as char).to_digit(16) else { continue };
            match half.take() {
                None => half = Some(v as u8),
                Some(hi) => {
                    data.push(hi << 4 | v as u8);
                    if data.len() == need {
                        break 'lines;
                    }
                }
            }
        }
    }
    if data.len() < need {
        return None;
    }
    let max = (1u32 << depth) - 1;
    let mut img = image::GrayImage::new(w, h);
    for y in 0..h as usize {
        let row = &data[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..w as usize {
            let bit = x * depth as usize;
            let v = u32::from(row[bit / 8] >> (8 - depth as usize - bit % 8)) & max;
            img.put_pixel(x as u32, y as u32, image::Luma([(v * 255 / max) as u8]));
        }
    }
    Some(DynamicImage::ImageLuma8(img))
}

/// The thumbnail of a `%AI7_Thumbnail: width height depth` block. Its
/// `%%BeginData` section holds hex lines behind `%`: a 768-byte palette of 256
/// RGB triples, the text `RLE`, then one palette index per pixel with
/// `FD count value` as a run and `FD FD` as a literal FD byte. Only depth 8
/// is read, and only when the pixels come to exactly width times height.
fn ai7_thumbnail(ps: &[u8]) -> Option<DynamicImage> {
    let (nums, rest) = header_line(ps, b"%AI7_Thumbnail:")?;
    let [w, h, depth, ..] = nums[..] else { return None };
    let (w, h) = dims(w, h)?;
    if depth != 8 {
        return None;
    }
    let total = w as usize * h as usize;
    let mut bytes = Vec::new();
    let mut half: Option<u8> = None;
    let mut in_data = false;
    for line in lines(rest) {
        if !in_data {
            if line.starts_with(b"%%BeginData") {
                in_data = true;
            } else if !line.is_empty() && !line.starts_with(b"%%") && !line.starts_with(b"%AI") {
                return None;
            }
            continue;
        }
        if line.starts_with(b"%%EndData") {
            break;
        }
        for &c in line.strip_prefix(b"%")? {
            let v = (c as char).to_digit(16)? as u8;
            match half.take() {
                None => half = Some(v),
                Some(hi) => bytes.push(hi << 4 | v),
            }
        }
        // Palette, marker and the pixels, which a run can pack tightly: the
        // hex can not be longer than a few bytes per pixel.
        if bytes.len() > 768 + 3 + total.checked_mul(3)? {
            return None;
        }
    }
    if half.is_some() || bytes.len() < 771 || &bytes[768..771] != b"RLE" {
        return None;
    }
    let (palette, body) = (&bytes[..768], &bytes[771..]);
    let mut idx: Vec<u8> = Vec::with_capacity(total);
    let mut it = body.iter().copied();
    while let Some(b) = it.next() {
        if b != 0xFD {
            if idx.len() >= total {
                return None;
            }
            idx.push(b);
            continue;
        }
        let count = it.next()?;
        if count == 0xFD {
            if idx.len() >= total {
                return None;
            }
            idx.push(0xFD);
            continue;
        }
        let value = it.next()?;
        if count == 0 || idx.len() + usize::from(count) > total {
            return None;
        }
        idx.resize(idx.len() + usize::from(count), value);
    }
    if idx.len() != total {
        return None;
    }
    let mut rgb = Vec::with_capacity(total * 3);
    for &i in &idx {
        rgb.extend_from_slice(&palette[usize::from(i) * 3..usize::from(i) * 3 + 3]);
    }
    image::RgbImage::from_raw(w, h, rgb).map(DynamicImage::ImageRgb8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    fn preview(p: &Path, ext: &str) -> Option<DynamicImage> {
        super::super::preview(p, ext)
    }

    fn size(p: &Path, ext: &str) -> Option<(u32, u32)> {
        use image::GenericImageView;
        preview(p, ext).map(|i| i.dimensions())
    }

    fn tiff(w: u32, h: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(image::GrayImage::new(w, h)).write_to(&mut out, image::ImageFormat::Tiff).unwrap();
        out.into_inner()
    }

    fn dos(ps: &[u8], tiff_off: u32, tiff_len: u32, tail: &[u8]) -> Vec<u8> {
        let mut d = vec![0xC5, 0xD0, 0xD3, 0xC6];
        for v in [30u32, ps.len() as u32, 0, 0, tiff_off, tiff_len] {
            d.extend(v.to_le_bytes());
        }
        d.extend([0xFF, 0xFF]);
        d.extend_from_slice(ps);
        d.extend_from_slice(tail);
        d
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.chunks(16).map(|c| format!("%{}\n", c.iter().map(|b| format!("{b:02x}")).collect::<String>())).collect()
    }

    /// A palette where index i is (i, 255 - i, 7), the RLE marker and `body`.
    fn ai7(w: u32, h: u32, depth: u32, body: &[u8]) -> Vec<u8> {
        let mut data: Vec<u8> = (0..=255u8).flat_map(|i| [i, 255 - i, 7]).collect();
        data.extend(b"RLE");
        data.extend_from_slice(body);
        format!("%!PS-Adobe-3.0\n%AI7_Thumbnail: {w} {h} {depth}\n%%BeginData: {} Hex Bytes\n{}%%EndData\n%%EndComments\n", data.len(), hex(&data)).into_bytes()
    }

    const PS: &[u8] = b"%!PS-Adobe-3.0 EPSF-3.0\n%%EndComments\nshowpage\n";

    #[test]
    fn dos_header_tiff() {
        let dir = tempfile::tempdir().unwrap();
        let t = tiff(12, 7);
        let d = dos(PS, 30 + PS.len() as u32, t.len() as u32, &t);
        assert_eq!(size(&write(dir.path(), "dos.eps", &d), "eps"), Some((12, 7)));
    }

    #[test]
    fn dos_header_bad_tiff_falls_back_to_ps() {
        let dir = tempfile::tempdir().unwrap();
        let epsi = b"%!PS\n%%BeginPreview: 2 1 8 1\n%00ff\n%%EndPreview\n";
        let d = dos(epsi, 1000, 500, &[]);
        assert_eq!(size(&write(dir.path(), "far.eps", &d), "eps"), Some((2, 1)));
        // Present but undecodable TIFF bytes also fall back.
        let d = dos(epsi, 30 + epsi.len() as u32, 4, b"junk");
        assert_eq!(size(&write(dir.path(), "junk.eps", &d), "eps"), Some((2, 1)));
        // Nothing usable in the PostScript part either.
        let d = dos(PS, 1000, 500, &[]);
        assert!(preview(&write(dir.path(), "none.eps", &d), "eps").is_none());
    }

    #[test]
    fn epsi_depth_1_and_8_with_both_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        for (i, nl) in ["\n", "\r", "\r\n"].iter().enumerate() {
            let one = format!("%!PS-Adobe-3.0 EPSF-3.0{nl}%%BeginPreview: 10 2 1 2{nl}% FFC0{nl}% 0000{nl}%%EndPreview{nl}");
            let img = preview(&write(dir.path(), &format!("one{i}.eps"), one.as_bytes()), "eps").unwrap();
            assert_eq!((img.width(), img.height()), (10, 2));
            let g = img.to_luma8();
            // 1 is white, 0 is black; the six padding bits are ignored.
            assert_eq!((g.get_pixel(0, 0).0[0], g.get_pixel(9, 0).0[0], g.get_pixel(0, 1).0[0], g.get_pixel(9, 1).0[0]), (255, 255, 0, 0));
            let eight = format!("%!PS-Adobe-3.0 EPSF-3.0{nl}%%BeginPreview: 4 1 8 1{nl}%00 80 ff 10{nl}%%EndPreview{nl}");
            let img = preview(&write(dir.path(), &format!("eight{i}.eps"), eight.as_bytes()), "eps").unwrap();
            let g = img.to_luma8();
            assert_eq!([0, 1, 2, 3].map(|x| g.get_pixel(x, 0).0[0]), [0, 128, 255, 16]);
        }
    }

    #[test]
    fn epsi_depth_2_and_4() {
        let dir = tempfile::tempdir().unwrap();
        let two = b"%!PS\n%%BeginPreview: 4 1 2 1\n%1B\n%%EndPreview\n";
        let g = preview(&write(dir.path(), "two.eps", two), "eps").unwrap().to_luma8();
        assert_eq!([0, 1, 2, 3].map(|x| g.get_pixel(x, 0).0[0]), [0, 85, 170, 255]);
        let four = b"%!PS\n%%BeginPreview: 2 1 4 1\n%0F\n%%EndPreview\n";
        let g = preview(&write(dir.path(), "four.eps", four), "eps").unwrap().to_luma8();
        assert_eq!([g.get_pixel(0, 0).0[0], g.get_pixel(1, 0).0[0]], [0, 255]);
    }

    #[test]
    fn ai7_block() {
        let dir = tempfile::tempdir().unwrap();
        // 2 x 3: 1, a run of 3 twos, a literal FD, then a run of 1 of index 5.
        let p = write(dir.path(), "t.ai", &ai7(2, 3, 8, &[1, 0xFD, 3, 2, 0xFD, 0xFD, 0xFD, 1, 5]));
        let img = preview(&p, "ai").unwrap().to_rgb8();
        assert_eq!((img.width(), img.height()), (2, 3));
        let px = |x, y| img.get_pixel(x, y).0;
        assert_eq!(px(0, 0), [1, 254, 7]);
        assert_eq!(px(1, 0), [2, 253, 7]);
        assert_eq!(px(0, 1), [2, 253, 7]);
        assert_eq!(px(1, 1), [2, 253, 7]);
        assert_eq!(px(0, 2), [0xFD, 2, 7]);
        assert_eq!(px(1, 2), [5, 250, 7]);
        assert_eq!(size(&write(dir.path(), "t.ait", &ai7(1, 1, 8, &[0])), "ait"), Some((1, 1)));
    }

    #[test]
    fn ai7_rejects_what_does_not_add_up() {
        let dir = tempfile::tempdir().unwrap();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("few", ai7(2, 3, 8, &[1, 2, 3, 4, 5])),
            ("many", ai7(2, 3, 8, &[1, 2, 3, 4, 5, 6, 7])),
            ("run over", ai7(2, 3, 8, &[1, 0xFD, 9, 2])),
            ("cut escape", ai7(2, 3, 8, &[1, 2, 3, 4, 5, 0xFD])),
            ("cut run", ai7(2, 3, 8, &[1, 2, 3, 4, 5, 0xFD, 1])),
            ("zero run", ai7(2, 3, 8, &[1, 2, 3, 4, 5, 0xFD, 0, 9])),
            ("depth 4", ai7(2, 3, 4, &[1, 2, 3, 4, 5, 6])),
            ("zero dims", ai7(0, 3, 8, &[])),
            ("huge dims", ai7(99999, 99999, 8, &[1])),
            ("wide", ai7(8193, 1, 8, &[1])),
            ("pixels", ai7(8192, 8192, 8, &[1])),
        ];
        for (name, bytes) in cases {
            assert!(preview(&write(dir.path(), "x.ai", &bytes), "ai").is_none(), "{name}");
        }
        // No RLE marker: the bytes after the palette are not read as pixels.
        let mut data: Vec<u8> = (0..=255u8).flat_map(|i| [i, i, i]).collect();
        data.extend([1, 2, 3, 4, 5, 6]);
        let no_rle = format!("%!PS\n%AI7_Thumbnail: 2 3 8\n%%BeginData: 774 Hex Bytes\n{}%%EndData\n", hex(&data));
        assert!(preview(&write(dir.path(), "n.ai", no_rle.as_bytes()), "ai").is_none());
        // Odd hex digit count, and a palette cut short.
        let odd = b"%!PS\n%AI7_Thumbnail: 1 1 8\n%%BeginData: 1 Hex Bytes\n%abc\n%%EndData\n";
        assert!(preview(&write(dir.path(), "o.ai", odd), "ai").is_none());
        // Header line cut off, and no data section.
        assert!(preview(&write(dir.path(), "c.ai", b"%!PS\n%AI7_Thumbnail: 2 3"), "ai").is_none());
        assert!(preview(&write(dir.path(), "d.ai", b"%!PS\n%AI7_Thumbnail: 2 3 8\n%%EndComments\n"), "ai").is_none());
    }

    #[test]
    fn no_preview_or_malformed_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        let bad: [&[u8]; 11] = [
            PS,
            b"",
            b"\xC5\xD0\xD3\xC6",
            b"\xC5\xD0\xD3\xC6\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\0\0\0\0\0\0\0\0\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF",
            b"%%BeginPreview: 99999 99999 8 1\n% 00\n",
            b"%%BeginPreview: 4294967295 4294967295 8 1\n% 00\n",
            b"%%BeginPreview: 0 5 8 1\n% 00\n",
            b"%%BeginPreview: 5 0 8 1\n% 00\n",
            b"%%BeginPreview: 4 4 3 1\n% 0000\n",
            b"%%BeginPreview: 4",
            b"%%BeginPreview: 4 4 8 1\n% 0000\n%%EndPreview\n",
        ];
        for (i, b) in bad.iter().enumerate() {
            assert!(preview(&write(dir.path(), &format!("b{i}.eps"), b), "eps").is_none(), "case {i}");
        }
        assert!(preview(&dir.path().join("absent.eps"), "eps").is_none());
    }

    #[test]
    fn ai_postscript_stub_without_preview() {
        let dir = tempfile::tempdir().unwrap();
        let stub = b"%!PS-Adobe-3.0\n%%Creator: x\n%AI3_DocumentPreview: None\n%AI5_FileFormat 3\n";
        assert!(preview(&write(dir.path(), "stub.ai", stub), "ai").is_none());
        let p = write(dir.path(), "old.ai", b"%!PS-Adobe-3.0\n%%BeginPreview: 2 2 8 1\n%00ff\n%80 40\n%%EndPreview\n");
        assert_eq!(size(&p, "ai"), Some((2, 2)));
    }
}
