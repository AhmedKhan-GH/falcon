//! Hand-written PSD and PSB files, compared pixel by pixel with what they
//! were written to hold.

use super::*;

/// PackBits, in the mix of runs Photoshop writes.
fn pack(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        let mut run = 1;
        while i + run < row.len() && row[i + run] == row[i] && run < 128 {
            run += 1;
        }
        if run > 1 {
            out.extend([(257 - run) as u8, row[i]]);
            i += run;
        } else {
            let start = i;
            while i < row.len() && i - start < 128 && (i + 1 >= row.len() || row[i] != row[i + 1]) {
                i += 1;
            }
            out.push((i - start - 1) as u8);
            out.extend(&row[start..i]);
        }
    }
    out
}

#[derive(Clone)]
struct Doc {
    psb: bool,
    width: u32,
    height: u32,
    depth: u16,
    mode: u16,
    rle: bool,
    /// Big-endian samples of each channel, row after row.
    channels: Vec<Vec<u8>>,
    colour_data: Vec<u8>,
    resources: Vec<(u16, Vec<u8>)>,
    layer_section: Vec<u8>,
}

impl Doc {
    fn new(width: u32, height: u32, depth: u16, mode: u16, channels: Vec<Vec<u8>>) -> Self {
        Doc { psb: false, width, height, depth, mode, rle: false, channels, colour_data: Vec::new(), resources: Vec::new(), layer_section: Vec::new() }
    }

    fn row_bytes(&self) -> usize {
        if self.depth == 1 { (self.width as usize).div_ceil(8) } else { self.width as usize * usize::from(self.depth) / 8 }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend(b"8BPS");
        f.extend(if self.psb { 2u16 } else { 1u16 }.to_be_bytes());
        f.extend([0; 6]);
        f.extend((self.channels.len() as u16).to_be_bytes());
        f.extend(self.height.to_be_bytes());
        f.extend(self.width.to_be_bytes());
        f.extend(self.depth.to_be_bytes());
        f.extend(self.mode.to_be_bytes());
        f.extend((self.colour_data.len() as u32).to_be_bytes());
        f.extend(&self.colour_data);
        let mut res = Vec::new();
        for (id, data) in &self.resources {
            res.extend(b"8BIM");
            res.extend(id.to_be_bytes());
            res.extend([0, 0]);
            res.extend((data.len() as u32).to_be_bytes());
            res.extend(data);
            if data.len() % 2 == 1 {
                res.push(0);
            }
        }
        f.extend((res.len() as u32).to_be_bytes());
        f.extend(res);
        if self.psb {
            f.extend((self.layer_section.len() as u64).to_be_bytes());
        } else {
            f.extend((self.layer_section.len() as u32).to_be_bytes());
        }
        f.extend(&self.layer_section);
        f.extend(u16::from(self.rle).to_be_bytes());
        if self.rle {
            let rows: Vec<Vec<u8>> = self.channels.iter().flat_map(|c| c.chunks(self.row_bytes()).map(pack)).collect();
            for r in &rows {
                if self.psb {
                    f.extend((r.len() as u32).to_be_bytes());
                } else {
                    f.extend((r.len() as u16).to_be_bytes());
                }
            }
            rows.iter().for_each(|r| f.extend(r));
        } else {
            self.channels.iter().for_each(|c| f.extend(c));
        }
        f
    }

    fn with_alpha_flag(mut self) -> Self {
        self.layer_section = layers_flagging_alpha(self.psb);
        self
    }

    fn write(&self, dir: &Path) -> std::path::PathBuf {
        let path = dir.join(if self.psb { "x.psb" } else { "x.psd" });
        std::fs::write(&path, self.bytes()).unwrap();
        path
    }

    /// Everything decoded, row after row.
    fn pixels(&self) -> Result<Vec<u8>, PsdError> {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Composite::open(&self.write(dir.path()))?;
        let (w, h) = c.size();
        Ok(c.read_rects(&[[0, 0, w, h]])?.remove(0).into_raw())
    }

    /// Raw and RLE must agree, in PSD and in PSB; gives the pixels.
    fn all_forms(&self) -> Vec<u8> {
        let raw = self.pixels().unwrap();
        for (psb, rle) in [(false, true), (true, false), (true, true)] {
            let mut d = self.clone();
            d.rle = rle;
            if d.psb != psb {
                d.psb = psb;
                if !d.layer_section.is_empty() {
                    d.layer_section = layers_flagging_alpha(psb);
                }
            }
            assert_eq!(d.pixels().unwrap(), raw, "psb {psb} rle {rle}");
        }
        raw
    }
}

/// A layer section with one negative layer count.
fn layers_flagging_alpha(psb: bool) -> Vec<u8> {
    let info = (-1i16).to_be_bytes();
    let mut s = Vec::new();
    if psb {
        s.extend((info.len() as u64).to_be_bytes());
    } else {
        s.extend((info.len() as u32).to_be_bytes());
    }
    s.extend(info);
    s.extend(0u32.to_be_bytes());
    s
}

fn wide(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|s| s.to_be_bytes()).collect()
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|s| s.to_be_bytes()).collect()
}

#[test]
fn rgb_8_bit() {
    let d = Doc::new(2, 2, 8, 3, vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10, 11, 12]]);
    assert_eq!(d.all_forms(), [1, 5, 9, 255, 2, 6, 10, 255, 3, 7, 11, 255, 4, 8, 12, 255]);
}

#[test]
fn rgb_16_bit_rounds_to_8() {
    let r = wide(&[0, 65535, 257, 32896]);
    let d = Doc::new(4, 1, 16, 3, vec![r.clone(), r.clone(), r]);
    let px = d.all_forms();
    assert_eq!(&px[..4], [0, 0, 0, 255]);
    assert_eq!(&px[4..8], [255, 255, 255, 255]);
    assert_eq!(&px[8..12], [1, 1, 1, 255]);
    assert_eq!(&px[12..16], [128, 128, 128, 255]);
}

#[test]
fn rgb_32_bit_is_linear_light() {
    let r = floats(&[0.0, 1.0, 0.2140, 5.0]);
    let d = Doc::new(4, 1, 32, 3, vec![r.clone(), r.clone(), r]);
    let px = d.all_forms();
    assert_eq!(px[0], 0);
    assert_eq!(px[4], 255);
    assert_eq!(px[8], 127, "0.214 linear is mid gray");
    assert_eq!(px[12], 255, "over-range clamps");
}

#[test]
fn gray_8_and_16_bit() {
    assert_eq!(Doc::new(2, 1, 8, 1, vec![vec![10, 200]]).all_forms(), [10, 10, 10, 255, 200, 200, 200, 255]);
    assert_eq!(Doc::new(2, 1, 16, 1, vec![wide(&[0x8080, 0xffff])]).all_forms(), [128, 128, 128, 255, 255, 255, 255, 255]);
}

#[test]
fn duotone_shows_its_gray_channel() {
    let mut d = Doc::new(2, 1, 8, 8, vec![vec![10, 200]]);
    d.colour_data = vec![0; 524];
    assert_eq!(d.all_forms(), [10, 10, 10, 255, 200, 200, 200, 255]);
}

#[test]
fn bitmap_one_is_black() {
    // 10 pixels: row bytes round up.
    let d = Doc::new(10, 1, 1, 0, vec![vec![0b1010_0000, 0b0100_0000]]);
    let px = d.all_forms();
    let bits: Vec<u8> = px.chunks(4).map(|p| p[0]).collect();
    assert_eq!(bits, [0, 255, 0, 255, 255, 255, 255, 255, 255, 0]);
}

#[test]
fn indexed_with_a_transparent_entry() {
    let mut palette = vec![0u8; 768];
    (palette[1], palette[256 + 1], palette[512 + 1]) = (10, 20, 30);
    (palette[2], palette[256 + 2], palette[512 + 2]) = (40, 50, 60);
    let mut d = Doc::new(2, 1, 8, 2, vec![vec![1, 2]]);
    d.colour_data = palette;
    assert_eq!(d.all_forms(), [10, 20, 30, 255, 40, 50, 60, 255]);
    d.resources.push((1047, 2u16.to_be_bytes().to_vec()));
    assert_eq!(d.all_forms(), [10, 20, 30, 255, 40, 50, 60, 0]);
}

/// Stored inverted: 255 is no ink. With no profile, the naive conversion.
#[test]
fn cmyk_without_a_profile_is_naive() {
    let c = vec![255, 0, 255, 255];
    let m = vec![255, 255, 0, 255];
    let y = vec![255, 255, 255, 255];
    let k = vec![255, 255, 255, 128];
    let d = Doc::new(4, 1, 8, 4, vec![c.clone(), m.clone(), y.clone(), k.clone()]);
    let px = d.all_forms();
    assert_eq!(&px[..4], [255, 255, 255, 255], "paper");
    assert_eq!(&px[4..8], [0, 255, 255, 255], "full cyan");
    assert_eq!(&px[8..12], [255, 0, 255, 255], "full magenta");
    assert_eq!(&px[12..16], [128, 128, 128, 255], "half black");
    let w = |v: &[u8]| wide(&v.iter().map(|s| u16::from(*s) * 257).collect::<Vec<_>>());
    let d16 = Doc::new(4, 1, 16, 4, vec![w(&c), w(&m), w(&y), w(&k)]);
    assert_eq!(d16.all_forms(), px, "the same at 16 bits");
}

#[cfg(target_os = "macos")]
#[test]
fn cmyk_goes_through_its_embedded_profile() {
    let Ok(icc) = std::fs::read("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc") else { return };
    let mut d = Doc::new(2, 1, 8, 4, vec![vec![255, 0], vec![255, 255], vec![255, 255], vec![255, 255]]);
    d.resources.push((1039, icc));
    let px = d.all_forms();
    assert!(px[..3].iter().all(|v| *v >= 240), "paper stays near white: {:?}", &px[..3]);
    // A profile's cyan is well inside sRGB's corner, unlike the naive (0, 255, 255).
    assert!(px[4] < 90 && px[5] > 120 && px[5] < 200 && px[6] > 180, "cyan: {:?}", &px[4..7]);
}

#[test]
fn lab_converts_from_d50() {
    let d = Doc::new(3, 1, 8, 9, vec![vec![255, 0, 128], vec![128, 128, 128], vec![128, 128, 128]]);
    let px = d.all_forms();
    assert_eq!(&px[..3], [255, 255, 255]);
    assert_eq!(&px[4..7], [0, 0, 0]);
    // L* 50 is sRGB 119 at any white point.
    assert!((i32::from(px[8]) - 119).abs() <= 1, "{:?}", &px[8..11]);
    assert_eq!(px[8], px[9]);
    let d = Doc::new(1, 1, 16, 9, vec![wide(&[0xffff]), wide(&[0x8000]), wide(&[0x8000])]);
    assert_eq!(d.all_forms(), [255, 255, 255, 255]);
}

#[test]
fn adobe_rgb_profile_is_applied() {
    let icc = ColorProfile::new_adobe_rgb().encode().unwrap();
    let mut d = Doc::new(1, 1, 8, 3, vec![vec![128], vec![200], vec![128]]);
    let plain = d.all_forms();
    assert_eq!(&plain[..3], [128, 200, 128]);
    d.resources.push((1039, icc));
    let px = d.all_forms();
    assert!(px[0] < 128 && px[1] > 200, "wider gamut pushes green out: {:?}", &px[..3]);
    let mut d16 = Doc::new(1, 1, 16, 3, vec![wide(&[128 * 257]), wide(&[200 * 257]), wide(&[128 * 257])]);
    d16.resources = d.resources.clone();
    let px16 = d16.all_forms();
    for c in 0..3 {
        assert!((i32::from(px[c]) - i32::from(px16[c])).abs() <= 2, "{px:?} {px16:?}");
    }
}

#[test]
fn a_bad_profile_counts_as_none() {
    let mut d = Doc::new(1, 1, 8, 3, vec![vec![1], vec![2], vec![3]]);
    d.resources.push((1039, vec![9; 40]));
    assert_eq!(d.all_forms(), [1, 2, 3, 255]);
}

/// An extra channel is a selection unless the layer section says otherwise.
#[test]
fn alpha_only_when_the_file_says_so() {
    let d = Doc::new(2, 1, 8, 3, vec![vec![10, 20], vec![10, 20], vec![10, 20], vec![0, 128]]);
    assert_eq!(d.all_forms(), [10, 10, 10, 255, 20, 20, 20, 255]);
    let flagged = d.with_alpha_flag();
    // The fully transparent pixel has no colour to find; the half one had
    // white under it: (20 - 255 * 127/255) / (128/255), clamped.
    let px = flagged.all_forms();
    assert_eq!(px[3], 0);
    assert_eq!(px[7], 128);
    // Five channels: the second extra one is a selection.
    let d = Doc::new(1, 1, 8, 3, vec![vec![200], vec![200], vec![200], vec![255], vec![0]]).with_alpha_flag();
    assert_eq!(d.all_forms(), [200, 200, 200, 255]);
}

#[test]
fn white_is_taken_out_of_a_transparent_merged_image() {
    // Red at half alpha over white: stored (255, 128, 128).
    let d = Doc::new(1, 1, 8, 3, vec![vec![255], vec![128], vec![128], vec![128]]).with_alpha_flag();
    let px = d.all_forms();
    assert_eq!(px[0], 255);
    assert!(px[1] <= 2 && px[2] <= 2, "{px:?}");
    assert_eq!(px[3], 128);
    let d = Doc::new(1, 1, 16, 1, vec![wide(&[0x8000]), wide(&[0x8000])]).with_alpha_flag();
    let px = d.all_forms();
    assert!(px[0] <= 2 && px[3] == 128, "black at half alpha: {px:?}");
}

/// 16-bit files keep their layer info in an `Lr16` block; PSB gives it a
/// long length.
#[test]
fn alpha_flag_in_additional_layer_info() {
    for psb in [false, true] {
        for key in [b"Lr16", b"Mt16", b"Mtrn"] {
            let mut s = Vec::new();
            let len64 = |s: &mut Vec<u8>, v: u64| if psb { s.extend(v.to_be_bytes()) } else { s.extend((v as u32).to_be_bytes()) };
            len64(&mut s, 0); // empty layer info
            s.extend(0u32.to_be_bytes()); // global mask
            s.extend(b"8BIM");
            s.extend(b"Patt");
            s.extend(2u32.to_be_bytes());
            s.extend([0, 0]);
            s.extend(b"8BIM");
            s.extend(key);
            if key == b"Lr16" {
                len64(&mut s, 2);
                s.extend((-3i16).to_be_bytes());
            } else {
                len64(&mut s, 0);
            }
            let mut d = Doc::new(1, 1, 16, 1, vec![wide(&[0xffff]), wide(&[0x8000])]);
            d.psb = psb;
            d.layer_section = s;
            assert_eq!(d.pixels().unwrap()[3], 128, "{psb} {key:?}");
        }
    }
}

#[test]
fn no_composite_is_told_apart() {
    let mut d = Doc::new(1, 1, 8, 3, vec![vec![255], vec![255], vec![255]]);
    // Version info: version 1, hasRealMergedData 0.
    d.resources.push((1057, vec![0, 0, 0, 1, 0]));
    assert_eq!(d.pixels(), Err(PsdError::NoComposite));
    d.resources[0].1 = vec![0, 0, 0, 1, 1];
    assert!(d.pixels().is_ok());
    // A file that stops after the layer section has none either.
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = Doc::new(1, 1, 8, 3, vec![vec![255], vec![255], vec![255]]).bytes();
    bytes.truncate(bytes.len() - 5);
    let path = dir.path().join("x.psd");
    std::fs::write(&path, bytes).unwrap();
    assert!(matches!(Composite::open(&path), Err(PsdError::NoComposite)));
}

#[test]
fn zip_compression_and_multichannel_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let d = Doc::new(1, 1, 8, 3, vec![vec![1], vec![2], vec![3]]);
    let mut bytes = d.bytes();
    let at = bytes.len() - 5;
    bytes[at + 1] = 3;
    let path = dir.path().join("x.psd");
    std::fs::write(&path, bytes).unwrap();
    assert!(matches!(Composite::open(&path), Err(PsdError::Compression(3))));
    let d = Doc::new(1, 1, 8, 7, vec![vec![1], vec![2]]);
    assert_eq!(d.pixels(), Err(PsdError::Mode { mode: 7, depth: 8 }));
}

#[test]
fn truncated_and_corrupt_files_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.psd");
    let open = |bytes: &[u8]| {
        std::fs::write(&path, bytes).unwrap();
        Composite::open(&path).and_then(|mut c| {
            let (w, h) = c.size();
            c.read_rects(&[[0, 0, w, h]])
        })
    };
    let channels = vec![vec![7u8; 64 * 64], (0..64 * 64).map(|i| (i % 251) as u8).collect(), vec![9u8; 64 * 64]];
    for rle in [false, true] {
        let mut d = Doc::new(64, 64, 8, 3, channels.clone());
        d.rle = rle;
        let full = d.bytes();
        assert!(open(&full).is_ok());
        // Every cut past the point where the image data begins is an error,
        // and no cut panics.
        let data_start = 26 + 4 + 4 + 4;
        for cut in (0..full.len() - 1).step_by(97) {
            let r = open(&full[..cut]);
            if cut > data_start {
                assert!(matches!(r, Err(PsdError::Damaged(_))), "rle {rle} cut {cut}");
            } else {
                assert!(r.is_err(), "rle {rle} cut {cut}");
            }
        }
    }
    assert!(matches!(open(b"not a psd at all, nothing like one"), Err(PsdError::Damaged(_))));
    // An RLE stream that overruns its row.
    let mut d = Doc::new(4, 1, 8, 1, vec![vec![1, 2, 3, 4]]);
    d.rle = true;
    let mut bad = d.bytes();
    let n = bad.len();
    // One 127-copy run where the row has 4 pixels.
    bad[n - 5] = 0x83;
    bad[n - 4] = 9;
    bad.truncate(n - 3);
    let table = n - 7;
    bad[table + 1] = 2;
    assert!(matches!(open(&bad), Err(PsdError::Damaged(_))), "{:?}", open(&bad).map(|_| ()));
}

/// A 30000 x 30000 header on a small file is refused at once, before any
/// allocation of pixels, in both forms; so are impossible sizes.
#[test]
fn a_huge_header_with_tiny_data_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.psd");
    for compression in [0u16, 1] {
        let mut f = Doc::new(1, 1, 8, 3, vec![vec![1], vec![2], vec![3]]).bytes();
        f[14..18].copy_from_slice(&30000u32.to_be_bytes());
        f[18..22].copy_from_slice(&30000u32.to_be_bytes());
        let at = f.len() - 5;
        f[at..at + 2].copy_from_slice(&compression.to_be_bytes());
        std::fs::write(&path, &f).unwrap();
        let t = std::time::Instant::now();
        assert!(matches!(Composite::open(&path), Err(PsdError::Damaged(_))));
        assert!(t.elapsed().as_secs() < 2);
        f[14..18].copy_from_slice(&30001u32.to_be_bytes());
        std::fs::write(&path, &f).unwrap();
        assert!(matches!(Composite::open(&path), Err(PsdError::Damaged(_))));
    }
}

/// A legitimately huge blank PSB: the file is small as RLE is, the header
/// is honest, and a part of it reads with memory for that part alone.
#[test]
fn a_part_of_a_huge_image_reads_alone() {
    let dir = tempfile::tempdir().unwrap();
    // 60000 x 40000 gray: 2400 MP, written row by row.
    let (w, h) = (60000usize, 40000usize);
    let mut row = pack(&vec![200u8; w]);
    // One marked row, near the middle.
    let marked = {
        let mut r = vec![200u8; w];
        r[30000..30010].fill(7);
        pack(&r)
    };
    let mut f = Vec::new();
    f.extend(b"8BPS");
    f.extend(2u16.to_be_bytes());
    f.extend([0; 6]);
    f.extend(1u16.to_be_bytes());
    f.extend((h as u32).to_be_bytes());
    f.extend((w as u32).to_be_bytes());
    f.extend(8u16.to_be_bytes());
    f.extend(1u16.to_be_bytes());
    f.extend(0u32.to_be_bytes());
    f.extend(0u32.to_be_bytes());
    f.extend(0u64.to_be_bytes());
    f.extend(1u16.to_be_bytes());
    for y in 0..h {
        let n = if y == 20000 { marked.len() } else { row.len() };
        f.extend((n as u32).to_be_bytes());
    }
    for y in 0..h {
        f.extend(if y == 20000 { &marked } else { &row });
    }
    row.clear();
    let path = dir.path().join("x.psb");
    std::fs::write(&path, &f).unwrap();
    drop(f);
    let mut c = Composite::open(&path).unwrap();
    assert_eq!(c.size(), (60000, 40000));
    let part = c.read_rects(&[[29995, 19999, 30015, 20002]]).unwrap().remove(0);
    assert_eq!(part.dimensions(), (20, 3));
    assert_eq!(part.get_pixel(5, 1).0, [7, 7, 7, 255]);
    assert_eq!(part.get_pixel(4, 1).0, [200, 200, 200, 255]);
    assert_eq!(part.get_pixel(5, 0).0, [200, 200, 200, 255]);
    // A shrunken view of all of it stays small however large the file.
    let small = c.read_scaled([0, 0, 60000, 40000], 300).unwrap();
    assert_eq!(small.dimensions(), (300, 200));
    assert_eq!(small.get_pixel(10, 10).0, [200, 200, 200, 255]);
}

#[test]
fn rects_and_scaling_match_the_whole() {
    let (w, h) = (97usize, 61usize);
    let r: Vec<u8> = (0..w * h).map(|i| (i % w) as u8).collect();
    let g: Vec<u8> = (0..w * h).map(|i| (i / w) as u8).collect();
    let b: Vec<u8> = (0..w * h).map(|i| (i % 251) as u8).collect();
    let dir = tempfile::tempdir().unwrap();
    let mut d = Doc::new(w as u32, h as u32, 8, 3, vec![r, g, b]);
    d.rle = true;
    let path = d.write(dir.path());
    let mut c = Composite::open(&path).unwrap();
    let whole = c.read_rects(&[[0, 0, w as u32, h as u32]]).unwrap().remove(0);
    let rects = [[0, 0, 40, 30], [39, 29, 97, 61], [10, 50, 11, 51], [90, 0, 200, 5]];
    let parts = c.read_rects(&rects).unwrap();
    assert_eq!(parts[3].dimensions(), (7, 5), "clamped to the picture");
    for (rect, part) in rects.iter().zip(&parts) {
        for (x, y, px) in part.enumerate_pixels() {
            assert_eq!(px, whole.get_pixel(rect[0] + x, rect[1] + y));
        }
    }
    // Long side asked past the source: no enlargement.
    assert_eq!(c.read_scaled([0, 0, 97, 61], 500).unwrap().dimensions(), (97, 61));
    assert_eq!(c.read_scaled([0, 0, 97, 61], 500).unwrap(), whole);
    let half = c.read_scaled([0, 0, 96, 60], 48).unwrap();
    assert_eq!(half.dimensions(), (48, 30));
    // Box of 2 x 2 on the R ramp: the mean of x and x + 1, rounded.
    assert_eq!(half.get_pixel(5, 0)[0], 11);
    assert_eq!(half.get_pixel(0, 5)[1], 11);
    assert_eq!(half.get_pixel(0, 0)[3], 255);
}

/// A transparent pixel lends no colour to the average.
#[test]
fn scaling_weighs_by_alpha() {
    let d = Doc::new(2, 1, 8, 3, vec![vec![0, 255], vec![0, 255], vec![255, 255], vec![255, 0]]).with_alpha_flag();
    let dir = tempfile::tempdir().unwrap();
    let mut c = Composite::open(&d.write(dir.path())).unwrap();
    assert_eq!(c.read_scaled([0, 0, 2, 1], 1).unwrap().get_pixel(0, 0).0, [0, 0, 255, 128]);
}

/// Each EXIF orientation, against what the image crate does to the same
/// picture, whole and cut.
#[test]
fn orientations_apply_to_whole_and_parts() {
    let (w, h) = (5u32, 3u32);
    let plane: Vec<u8> = (0..w * h).map(|i| (i * 16) as u8).collect();
    let base = image::RgbaImage::from_fn(w, h, |x, y| {
        let v = (y * w + x) * 16;
        image::Rgba([v as u8, v as u8, v as u8, 255])
    });
    let dir = tempfile::tempdir().unwrap();
    for o in 1..=8u8 {
        let mut d = Doc::new(w, h, 8, 1, vec![plane.clone()]);
        // A little big-endian TIFF with the one tag.
        let mut tiff = b"MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
        tiff.extend([0, o, 0, 0, 0, 0, 0, 0, 0, 0]);
        d.resources.push((1058, tiff));
        let mut c = Composite::open(&d.write(dir.path())).unwrap();
        let mut want = DynamicImage::ImageRgba8(base.clone());
        want.apply_orientation(Orientation::from_exif(o).unwrap());
        let want = want.into_rgba8();
        assert_eq!(c.size(), want.dimensions(), "{o}");
        let (uw, uh) = c.size();
        assert_eq!(c.read_rects(&[[0, 0, uw, uh]]).unwrap().remove(0), want, "{o}");
        assert_eq!(c.read_scaled([0, 0, uw, uh], 50).unwrap(), want, "{o}");
        let part = c.read_rects(&[[1, 1, 3, 2]]).unwrap().remove(0);
        assert_eq!(part.dimensions(), (2, 1), "{o}");
        assert_eq!(part.get_pixel(0, 0), want.get_pixel(1, 1), "{o}");
        assert_eq!(part.get_pixel(1, 0), want.get_pixel(2, 1), "{o}");
    }
}

/// A wide picture read in chunks: rows beyond one chunk arrive right.
#[test]
fn reads_across_chunks() {
    // Incompressible enough, and more than one chunk per channel.
    let (w, h) = (4096usize, 4096usize);
    let plane: Vec<u8> = (0..w * h).map(|i| (i.wrapping_mul(2654435761) >> 11) as u8).collect();
    for rle in [false, true] {
        let mut d = Doc::new(w as u32, h as u32, 8, 1, vec![plane.clone()]);
        d.rle = rle;
        let px = d.pixels().unwrap();
        assert!(px.chunks(4).map(|p| p[0]).eq(plane.iter().copied()), "rle {rle}");
    }
}
