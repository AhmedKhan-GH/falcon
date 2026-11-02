//! Textures of 3D models: the pictures a model's materials map onto its
//! surfaces, decoded to RGBA within a memory bound, and how the viewer and
//! the thumbnail renderer sample them.
//!
//! A reader asks a [`Loader`] for each picture, by the name the file gives
//! (a sidecar file) or with its bytes (embedded in the model). Names are
//! looked up only next to the model or in a folder below it: a relative
//! path that stays inside the model's folder is tried as written, and any
//! name (absolute, climbing out with `..`, or relative) is also tried as
//! its file name alone, next to the model. Nothing outside the folder is
//! ever read. A picture that is missing or can't be decoded gives `None`
//! and the mesh keeps its material colour.
//!
//! Memory: each picture is decoded to at most [`MAX_SIDE`] pixels on its
//! longer side (larger ones are scaled down, by ImageIO while decoding on
//! macOS), and one model's pictures together take at most [`BUDGET`]
//! bytes of RGBA; a picture that doesn't fit what is left is halved until
//! it does. The same file or the same embedded bytes are decoded once.
//!
//! Texture coordinates are kept in picture space: u to the right and v
//! downwards, (0, 0) the top left corner. Readers of formats that count v
//! upwards (OBJ, FBX, COLLADA, 3DS) flip it.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

use image::imageops::FilterType;
use image::{ImageReader, RgbaImage};

use super::math::V3;

/// Longest side a texture is decoded to.
pub const MAX_SIDE: u32 = 4096;
/// Bytes of RGBA one model's textures may take together (64 MB).
pub const BUDGET: usize = 64 * 1024 * 1024;
/// Most bytes the `image` crate may allocate decoding one picture at full
/// size before it is scaled down (where ImageIO can't decode it smaller).
const DECODE_LIMIT: u64 = 512 * 1024 * 1024;

/// What happens to texture coordinates outside 0..1, per axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Wrap {
    /// The picture tiles.
    #[default]
    Repeat,
    /// The edge pixels stretch out.
    Clamp,
    /// The picture tiles, every other copy mirrored.
    Mirror,
}

/// A decoded picture: straight-alpha RGBA8 in its file's own (sRGB)
/// values, rows top to bottom.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    /// The file name or a description of where it came from.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({:?}, {}x{})", self.name, self.width, self.height)
    }
}

impl Image {
    /// The pixel at (`x`, `y`), which must be inside.
    fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// The average colour, weighted by alpha, over at most 64 x 64 evenly
    /// spread pixels; `None` when it is fully transparent.
    pub fn average(&self) -> Option<V3> {
        let (sx, sy) = ((self.width / 64).max(1), (self.height / 64).max(1));
        let (mut sum, mut weight) = ([0.0f64; 3], 0.0f64);
        for y in (0..self.height).step_by(sy as usize) {
            for x in (0..self.width).step_by(sx as usize) {
                let p = self.texel(x, y);
                let a = f64::from(p[3]) / 255.0;
                for k in 0..3 {
                    sum[k] += f64::from(p[k]) / 255.0 * a;
                }
                weight += a;
            }
        }
        (weight > 0.0).then(|| sum.map(|s| (s / weight) as f32))
    }
}

/// A material's texture: the picture and how it is laid on.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub image: Arc<Image>,
    /// Multiplies the picture (in linear light), in display values: glTF's
    /// base colour factor; white for the other formats, whose viewers in
    /// Eagle replace the material colour with the picture.
    pub tint: V3,
    /// Wrap along u and along v.
    pub wrap: [Wrap; 2],
    /// Pixels with alpha below this are not drawn (glTF's `MASK` mode);
    /// `None` draws every pixel opaque.
    pub alpha_cutoff: Option<f32>,
}

impl Texture {
    /// `image` tiled, untinted, opaque.
    pub fn new(image: Arc<Image>) -> Texture {
        Texture { image, tint: [1.0; 3], wrap: [Wrap::Repeat; 2], alpha_cutoff: None }
    }

    /// The picture at `uv` (picture space, 0..1 across it), filtered
    /// bilinearly in linear light, as display values and straight alpha.
    /// The tint is not applied.
    pub fn sample(&self, uv: [f32; 2]) -> [f32; 4] {
        let img = &*self.image;
        let (w, h) = (img.width.max(1), img.height.max(1));
        // Texel centres sit at half-integers.
        let fx = wrap_coord(uv[0], self.wrap[0]) * w as f32 - 0.5;
        let fy = wrap_coord(uv[1], self.wrap[1]) * h as f32 - 0.5;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let pick = |i: i64, n: u32, wrap: Wrap| -> u32 {
            let n = i64::from(n);
            match wrap {
                Wrap::Clamp => i.clamp(0, n - 1) as u32,
                Wrap::Repeat => i.rem_euclid(n) as u32,
                Wrap::Mirror => {
                    let m = i.rem_euclid(2 * n);
                    (if m < n { m } else { 2 * n - 1 - m }) as u32
                }
            }
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let xs = [pick(x0, w, self.wrap[0]), pick(x0 + 1, w, self.wrap[0])];
        let ys = [pick(y0, h, self.wrap[1]), pick(y0 + 1, h, self.wrap[1])];
        let lut = linear_lut();
        let mut acc = [0.0f32; 4];
        for (j, &y) in ys.iter().enumerate() {
            let wy = if j == 0 { 1.0 - ty } else { ty };
            for (i, &x) in xs.iter().enumerate() {
                let wx = if i == 0 { 1.0 - tx } else { tx };
                let p = img.texel(x, y);
                let a = f32::from(p[3]) / 255.0;
                let wt = wx * wy;
                // Weighted by alpha, so transparent pixels don't bleed
                // their colour into the edge.
                for k in 0..3 {
                    acc[k] += lut[p[k] as usize] * a * wt;
                }
                acc[3] += a * wt;
            }
        }
        if acc[3] <= 0.0 {
            return [0.0; 4];
        }
        [linear_to_srgb(acc[0] / acc[3]), linear_to_srgb(acc[1] / acc[3]), linear_to_srgb(acc[2] / acc[3]), acc[3]]
    }
}

/// `color` (display values) times the texel `rgb` (display values),
/// multiplied in linear light: the textured base colour the viewer's
/// shader computes the same way.
pub fn modulate(color: V3, rgb: [f32; 4]) -> V3 {
    std::array::from_fn(|k| linear_to_srgb(srgb_to_linear(color[k]) * srgb_to_linear(rgb[k])))
}

/// A texture coordinate brought into 0..1 by `wrap`.
pub fn wrap_coord(t: f32, wrap: Wrap) -> f32 {
    if !t.is_finite() {
        return 0.0;
    }
    match wrap {
        Wrap::Clamp => t.clamp(0.0, 1.0),
        Wrap::Repeat => t - t.floor(),
        Wrap::Mirror => {
            let m = t.rem_euclid(2.0);
            if m <= 1.0 { m } else { 2.0 - m }
        }
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Linear light of each 8-bit sRGB value.
fn linear_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0)))
}

/// The size a `w` x `h` picture is decoded at: at most `max_side` on its
/// longer side, then halved until it takes no more than `max_bytes` of
/// RGBA. `None` when not even one pixel fits.
pub fn fit_size(w: u32, h: u32, max_side: u32, max_bytes: usize) -> Option<(u32, u32)> {
    if w == 0 || h == 0 || max_bytes < 4 {
        return None;
    }
    let s = (f64::from(max_side) / f64::from(w.max(h))).min(1.0);
    let (mut tw, mut th) = (((f64::from(w) * s).round() as u32).max(1), ((f64::from(h) * s).round() as u32).max(1));
    while tw as usize * th as usize * 4 > max_bytes {
        tw = (tw / 2).max(1);
        th = (th / 2).max(1);
    }
    Some((tw, th))
}

/// Decodes a picture file's bytes to RGBA within `max_side` and
/// `max_bytes` (see [`fit_size`]). Formats the `image` crate reads (PNG,
/// JPEG, TGA, BMP, TIFF, DDS, …) go through it; anything else, and pictures
/// to be scaled down, through ImageIO on macOS.
pub fn decode(data: &[u8], max_side: u32, max_bytes: usize) -> Option<RgbaImage> {
    let size = ImageReader::new(Cursor::new(data)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok());
    let img = match size {
        Some((w, h)) => {
            let (tw, th) = fit_size(w, h, max_side, max_bytes)?;
            if (tw, th) == (w, h) {
                decode_full(data)?
            } else {
                match crate::imageio::decode_bytes_within(data, tw.max(th)) {
                    Ok(img) => img,
                    Err(_) => decode_full(data)?,
                }
            }
        }
        None => crate::imageio::decode_bytes_within(data, max_side).ok()?,
    };
    let (tw, th) = fit_size(img.width(), img.height(), max_side, max_bytes)?;
    Some(if (tw, th) == img.dimensions() { img } else { image::imageops::resize(&img, tw, th, FilterType::Triangle) })
}

fn decode_full(data: &[u8]) -> Option<RgbaImage> {
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DECODE_LIMIT);
    reader.limits(limits);
    Some(reader.decode().ok()?.into_rgba8())
}

/// Where a texture named `path` in a model file may be read, relative to
/// the model's folder, in the order to try: the path as written when it
/// is relative and stays inside the folder, then its file name alone.
/// Backslashes count as separators and a `file://` prefix is dropped.
pub fn candidates(path: &str) -> Vec<String> {
    let p = path.trim().replace('\\', "/");
    let p = p.strip_prefix("file://").map_or(p.as_str(), |r| r.strip_prefix("localhost").unwrap_or(r));
    let drive = p.len() >= 2 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic();
    let absolute = p.starts_with('/') || drive;
    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    let Some(&name) = parts.last() else { return Vec::new() };
    let name = if drive && parts.len() == 1 { &name[2..] } else { name };
    if name.is_empty() || name == ".." {
        return Vec::new();
    }
    let mut out = Vec::new();
    if !absolute && !parts.contains(&"..") {
        out.push(parts.join("/"));
    }
    if out.first().is_none_or(|f| f != name) {
        out.push(name.to_string());
    }
    out
}

/// Fetches a file next to the model by relative name.
pub type Side<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// Decodes one model's textures: looks pictures up next to the model,
/// keeps the decoded total within [`BUDGET`] and decodes each file or
/// embedded picture once.
pub struct Loader<'a> {
    side: Side<'a>,
    left: usize,
    /// By the path read and by the bytes' hash.
    by_path: HashMap<String, Option<Arc<Image>>>,
    by_hash: HashMap<[u8; 32], Option<Arc<Image>>>,
}

impl<'a> Loader<'a> {
    pub fn new(side: Side<'a>) -> Loader<'a> {
        Loader::with_budget(side, BUDGET)
    }

    /// A loader with its own byte budget.
    pub fn with_budget(side: Side<'a>, budget: usize) -> Loader<'a> {
        Loader { side, left: budget, by_path: HashMap::new(), by_hash: HashMap::new() }
    }

    /// Bytes of RGBA still free.
    pub fn left(&self) -> usize {
        self.left
    }

    /// The picture a model names by `path` (see [`candidates`]).
    pub fn file(&mut self, path: &str) -> Option<Arc<Image>> {
        self.file_in("", path)
    }

    /// The picture named by `path` in a file that sits in the folder `dir`
    /// (relative to the model, as an MTL library in a subfolder): tried
    /// there first, then as [`Loader::file`] would.
    pub fn file_in(&mut self, dir: &str, path: &str) -> Option<Arc<Image>> {
        let mut tries = Vec::new();
        let dir = dir.trim_matches('/');
        if !dir.is_empty() {
            for c in candidates(path) {
                tries.extend(candidates(&format!("{dir}/{c}")).into_iter().take(1));
            }
        }
        tries.extend(candidates(path));
        for p in tries {
            if let Some(hit) = self.by_path.get(&p) {
                if hit.is_some() {
                    return hit.clone();
                }
                continue;
            }
            let Some(bytes) = (self.side)(&p) else {
                self.by_path.insert(p, None);
                continue;
            };
            let name = p.rsplit('/').next().unwrap_or(&p).to_string();
            let img = self.embedded(&name, &bytes);
            self.by_path.insert(p, img.clone());
            if img.is_some() {
                return img;
            }
        }
        None
    }

    /// A picture whose file bytes are `data` (embedded in the model, or
    /// read by [`Loader::file`]), called `name`.
    pub fn embedded(&mut self, name: &str, data: &[u8]) -> Option<Arc<Image>> {
        let hash = *blake3::hash(data).as_bytes();
        if let Some(hit) = self.by_hash.get(&hash) {
            return hit.clone();
        }
        let img = decode(data, MAX_SIDE, self.left).map(|img| {
            let (width, height) = img.dimensions();
            Arc::new(Image { name: name.to_string(), width, height, rgba: img.into_raw() })
        });
        if let Some(i) = &img {
            self.left -= i.rgba.len();
        }
        self.by_hash.insert(hash, img.clone());
        img
    }
}

/// The smaller levels of `img`'s mipmap chain, each half the one before
/// (at least one pixel), down to 1 x 1: alpha-weighted 2 x 2 averages in
/// linear light, in sRGB values like the picture. The viewer uploads
/// them under the full picture.
pub fn mip_chain(img: &Image) -> Vec<(u32, u32, Vec<u8>)> {
    let lut = linear_lut();
    let mut out: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    let (mut w, mut h) = (img.width, img.height);
    while w > 1 || h > 1 {
        let (pw, ph) = (w, h);
        let src: &[u8] = out.last().map_or(&img.rgba, |l| &l.2);
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut level = vec![0u8; nw as usize * nh as usize * 4];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                let mut n = 0.0f32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(pw - 1), (y * 2 + dy).min(ph - 1));
                    let i = (sy as usize * pw as usize + sx as usize) * 4;
                    let a = f32::from(src[i + 3]) / 255.0;
                    for k in 0..3 {
                        acc[k] += lut[src[i + k] as usize] * a;
                    }
                    acc[3] += a;
                    n += 1.0;
                }
                let o = (y as usize * nw as usize + x as usize) * 4;
                for k in 0..3 {
                    let v = if acc[3] > 0.0 { linear_to_srgb(acc[k] / acc[3]) } else { 0.0 };
                    level[o + k] = (v * 255.0).round() as u8;
                }
                level[o + 3] = (acc[3] / n * 255.0).round() as u8;
            }
        }
        out.push((nw, nh, level));
        (w, h) = (nw, nh);
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG of `w` x `h` pixels of one colour.
    pub(crate) fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        RgbaImage::from_pixel(w, h, image::Rgba(rgba)).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    /// A 2 x 2 PNG: red, green on top; blue, white below.
    pub(crate) fn png_2x2() -> Vec<u8> {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
        img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
        img.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    /// An indexed PNG of `w` x `h` pixels all of palette colour `rgb`,
    /// written by hand: the pixel data is all zero bytes, deflated as one
    /// literal and then matches of 258 bytes, so a picture of any size is
    /// a few bytes per 258 pixels to make.
    pub(crate) fn huge_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let start = out.len();
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let crc = crc32(&out[start..]);
            out.extend_from_slice(&crc.to_be_bytes());
        }
        fn crc32(b: &[u8]) -> u32 {
            let mut c = !0u32;
            for &x in b {
                c ^= u32::from(x);
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        struct Bits {
            out: Vec<u8>,
            acc: u64,
            n: u32,
        }
        impl Bits {
            fn put(&mut self, v: u32, len: u32) {
                self.acc |= u64::from(v) << self.n;
                self.n += len;
                while self.n >= 8 {
                    self.out.push(self.acc as u8);
                    self.acc >>= 8;
                    self.n -= 8;
                }
            }
            /// A fixed-Huffman code, written most significant bit first.
            fn code(&mut self, v: u32, len: u32) {
                let rev = v.reverse_bits() >> (32 - len);
                self.put(rev, len);
            }
        }
        let total = u64::from(h) * (u64::from(w) + 1);
        let mut z = vec![0x78, 0x01];
        let mut bits = Bits { out: Vec::new(), acc: 0, n: 0 };
        bits.put(1, 1); // final block
        bits.put(1, 2); // fixed Huffman
        bits.code(0x30, 8); // literal 0
        let mut left = total - 1;
        while left >= 3 {
            let len = left.min(258);
            // Leave at least 3 for the last match, or finish with literals.
            let len = if left - len > 0 && left - len < 3 { len - 3 } else { len };
            match len {
                258 => bits.code(0b1100_0101, 8), // code 285
                l => {
                    // Lengths 3..=257 by the deflate length table.
                    let (code, base, extra) = length_code(l as u32);
                    if code < 280 {
                        bits.code(code - 256, 7);
                    } else {
                        bits.code(0b1100_0000 + (code - 280), 8);
                    }
                    bits.put(l as u32 - base, extra);
                }
            }
            bits.code(0, 5); // distance 1
            left -= len;
        }
        for _ in 0..left {
            bits.code(0x30, 8);
        }
        bits.code(0, 7); // end of block
        if bits.n > 0 {
            bits.out.push(bits.acc as u8);
        }
        z.extend_from_slice(&bits.out);
        // Adler-32 of `total` zero bytes.
        let (a, b) = (1u64, total % 65521);
        z.extend_from_slice(&(((b << 16) | a) as u32).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"PLTE", &rgb);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    fn length_code(l: u32) -> (u32, u32, u32) {
        const BASE: [u32; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
        const EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
        let i = BASE.iter().rposition(|&b| b <= l).unwrap();
        (257 + i as u32, BASE[i], EXTRA[i])
    }

    fn image(w: u32, h: u32, px: &[[u8; 4]]) -> Arc<Image> {
        Arc::new(Image { name: "t".into(), width: w, height: h, rgba: px.iter().flatten().copied().collect() })
    }

    #[test]
    fn paths_stay_next_to_the_model() {
        assert_eq!(candidates("tex/wood.png"), ["tex/wood.png", "wood.png"]);
        assert_eq!(candidates("./wood.png"), ["wood.png"]);
        assert_eq!(candidates("tex\\wood.png"), ["tex/wood.png", "wood.png"]);
        // Climbing out or absolute: only the file name, next to the model.
        assert_eq!(candidates("../x.png"), ["x.png"]);
        assert_eq!(candidates("a/../../x.png"), ["x.png"]);
        assert_eq!(candidates("/etc/x.png"), ["x.png"]);
        assert_eq!(candidates("C:\\work\\x.png"), ["x.png"]);
        assert_eq!(candidates("C:x.png"), ["x.png"]);
        assert_eq!(candidates("file:///C:/work/g.png"), ["g.png"]);
        assert!(candidates("..").is_empty() && candidates("").is_empty() && candidates("/").is_empty());
    }

    #[test]
    fn traversal_and_absolute_paths_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("m");
        std::fs::create_dir_all(model.join("sub")).unwrap();
        std::fs::write(dir.path().join("x.png"), png(1, 1, [255, 0, 0, 255])).unwrap();
        std::fs::write(model.join("sub/y.png"), png(1, 1, [0, 255, 0, 255])).unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let root = model.clone();
        let side = |n: &str| {
            asked.borrow_mut().push(n.to_string());
            std::fs::read(root.join(n)).ok()
        };
        let mut l = Loader::new(&side);
        assert!(l.file("../x.png").is_none(), "above the model");
        assert!(l.file(dir.path().join("x.png").to_str().unwrap()).is_none(), "absolute");
        assert!(asked.borrow().iter().all(|n| n == "x.png"), "only the bare name was asked for: {:?}", asked.borrow());
        // Below the model, by its path or by name from a subfolder file.
        assert_eq!(l.file("sub/y.png").unwrap().texel(0, 0), [0, 255, 0, 255]);
        assert!(l.file_in("sub", "y.png").is_some());
    }

    #[test]
    fn the_same_picture_decodes_once() {
        let data = png(4, 4, [9, 9, 9, 255]);
        let side = |n: &str| (n == "a.png" || n == "b.png").then(|| data.clone());
        let mut l = Loader::new(&side);
        let a = l.file("a.png").unwrap();
        let b = l.file("b.png").unwrap();
        assert!(Arc::ptr_eq(&a, &b), "same bytes, one picture");
        assert_eq!(l.left(), BUDGET - 64);
        assert!(l.file("missing.png").is_none());
        assert!(l.embedded("junk", b"not a picture").is_none());
        assert_eq!(l.left(), BUDGET - 64);
        let q = l.embedded("q", &png_2x2()).unwrap();
        assert_eq!((q.width, q.height, q.texel(0, 1)), (2, 2, [0, 0, 255, 255]));
    }

    #[test]
    fn huge_pictures_are_scaled_down_within_the_budget() {
        // A 10000 x 10000 picture comes back 4096 x 4096.
        let big = huge_png(10_000, 10_000, [200, 10, 20]);
        assert!(big.len() < 8_000_000);
        let side = |_: &str| None;
        let mut l = Loader::new(&side);
        let img = l.embedded("big", &big).unwrap();
        assert_eq!((img.width, img.height), (MAX_SIDE, MAX_SIDE));
        let mid = img.texel(2000, 2000);
        assert!(mid[0] > 190 && mid[1] < 30 && mid[3] == 255, "{mid:?}");
        // 4096² RGBA is the whole budget: a second, different picture has
        // nothing left and is not decoded.
        assert_eq!(l.left(), 0);
        assert!(l.embedded("other", &huge_png(5000, 5000, [1, 2, 3])).is_none());
        // With half the budget used, the next is halved to fit.
        let mut l = Loader::with_budget(&side, BUDGET);
        let a = l.embedded("a", &huge_png(4096, 2048, [1, 1, 1])).unwrap();
        assert_eq!((a.width, a.height), (4096, 2048));
        let b = l.embedded("b", &huge_png(4096, 4096, [2, 2, 2])).unwrap();
        assert_eq!((b.width, b.height), (2048, 2048));
        assert_eq!(l.left(), BUDGET - (4096 * 2048 + 2048 * 2048) * 4);
    }

    #[test]
    fn fitting_sizes() {
        assert_eq!(fit_size(100, 50, 4096, BUDGET), Some((100, 50)));
        assert_eq!(fit_size(10_000, 5_000, 4096, BUDGET), Some((4096, 2048)));
        assert_eq!(fit_size(4096, 4096, 4096, BUDGET / 2), Some((2048, 2048)));
        assert_eq!(fit_size(8, 8, 4096, 3), None);
        assert_eq!(fit_size(0, 8, 4096, BUDGET), None);
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap_coord(1.25, Wrap::Repeat), 0.25);
        assert_eq!(wrap_coord(-0.25, Wrap::Repeat), 0.75);
        assert_eq!(wrap_coord(1.25, Wrap::Clamp), 1.0);
        assert_eq!(wrap_coord(1.25, Wrap::Mirror), 0.75);
        assert_eq!(wrap_coord(-0.25, Wrap::Mirror), 0.25);
        assert_eq!(wrap_coord(f32::NAN, Wrap::Repeat), 0.0);
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn sampling_is_bilinear_and_wraps() {
        let t = Texture::new(image(2, 2, &[[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]]));
        // Texel centres give the texels.
        assert!(close(t.sample([0.25, 0.25]), [1.0, 0.0, 0.0, 1.0]));
        assert!(close(t.sample([0.75, 0.75]), [1.0; 4]));
        assert!(close(t.sample([0.25, 0.75]), [0.0, 0.0, 1.0, 1.0]), "v runs down the picture");
        // Between red and green: half of each in linear light, brighter
        // than 0.5 in display values.
        let mid = t.sample([0.5, 0.25]);
        assert!((mid[0] - mid[1]).abs() < 1e-5 && mid[0] > 0.7 && mid[2] == 0.0, "{mid:?}");
        // Repeat: one past the right edge is the left column again.
        assert!(close(t.sample([1.25, 0.25]), t.sample([0.25, 0.25])));
        // Clamp at the corner keeps the corner texel; repeat blends with
        // the opposite side.
        let mut c = t.clone();
        c.wrap = [Wrap::Clamp; 2];
        assert!(close(c.sample([0.0, 0.0]), [1.0, 0.0, 0.0, 1.0]));
        assert!(!close(t.sample([0.0, 0.0]), [1.0, 0.0, 0.0, 1.0]));
        // Transparent texels don't darken their neighbours.
        let a = Texture::new(image(2, 1, &[[255, 0, 0, 255], [0, 0, 0, 0]]));
        assert!(close(a.sample([0.5, 0.5]), [1.0, 0.0, 0.0, 0.5]));
    }

    #[test]
    fn modulating_multiplies_in_linear_light() {
        assert_eq!(modulate([1.0; 3], [0.2, 0.4, 0.6, 1.0]).map(|c| (c * 1000.0).round()), [200.0, 400.0, 600.0]);
        let half = modulate([0.5; 3], [0.5, 0.5, 0.5, 1.0]);
        assert!(half[0] < 0.25 + 0.2 && half[0] > 0.2, "{half:?}");
    }

    #[test]
    fn mipmaps_halve_to_one_pixel() {
        let img = Image { name: String::new(), width: 4, height: 2, rgba: [[255u8, 0, 0, 255], [0, 0, 255, 255]].repeat(4).concat() };
        let chain = mip_chain(&img);
        assert_eq!(chain.iter().map(|l| (l.0, l.1)).collect::<Vec<_>>(), [(2, 1), (1, 1)]);
        // Red and blue columns average in linear light.
        let p = &chain[1].2;
        assert!(p[0] > 180 && p[0] == p[2] && p[1] == 0 && p[3] == 255, "{p:?}");
        assert!(mip_chain(&Image { name: String::new(), width: 1, height: 1, rgba: vec![0; 4] }).is_empty());
    }

    #[test]
    fn averages_weigh_alpha() {
        let img = image(2, 1, &[[0, 0, 255, 255], [255, 0, 0, 0]]);
        assert_eq!(img.average(), Some([0.0, 0.0, 1.0]));
        assert_eq!(image(1, 1, &[[1, 2, 3, 0]]).average(), None);
    }
}
