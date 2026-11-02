//! The merged (composite) image of Photoshop PSD, PSDT and PSB files, read
//! from the public Adobe file format specification: header, colour mode
//! data, image resources (ICC profile, EXIF orientation, version info,
//! indexed transparency), the layer section (skipped, but for whether the
//! merged image has transparency) and the image data, raw or RLE.
//!
//! Nothing is read into memory whole. The row table gives each row's place
//! in the file, so any part of the picture is decoded on its own, row by
//! row. Beyond the pixels the caller asks for, memory is at most
//! [`CHUNK_BYTES`] of compressed data and a decoded row per channel in use
//! (five at most), and the row table: 8 bytes per row per channel in use,
//! 12 MB for a 300000-row PSB.
//!
//! Colour goes to 8-bit sRGB:
//! - With an embedded ICC profile, RGB, grayscale, CMYK and indexed pixels
//!   go through it (`moxcms`).
//! - Without one, RGB and gray are taken as sRGB (32-bit samples as linear
//!   sRGB), and CMYK goes through the naive `R = (1 - C)(1 - K)`.
//! - Lab is converted from its D50 definition, and duotone shows the gray
//!   image the file stores (its ink curves are undocumented). Bitmap is
//!   black and white.
//!
//! Only the first extra channel is alpha, and only when the file says the
//! merged image has transparency: a negative layer count or a `Mtrn`,
//! `Mt16` or `Mt32` block in the layer section. Any other extra channel is
//! a saved selection or a spot colour. Photoshop stores such a merged image
//! blended over white; the white is taken back out.

use std::fs::File;
use std::io::{BufReader, ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use image::metadata::Orientation;
use image::{DynamicImage, RgbaImage};
use moxcms::{ColorProfile, Layout, TransformExecutor, TransformOptions};

/// Compressed bytes read from the file at a time, per channel.
pub const CHUNK_BYTES: u64 = 8 << 20;

const MAX_SIDE_PSD: u32 = 30_000;
const MAX_SIDE_PSB: u32 = 300_000;
/// Larger resources than these are ignored, not read.
const MAX_ICC_BYTES: u32 = 64 << 20;
const MAX_EXIF_BYTES: u32 = 1 << 20;

/// Whether `ext` (lowercase, no dot) is a Photoshop document.
pub fn handles(ext: &str) -> bool {
    matches!(ext, "psd" | "psdt" | "psb")
}

/// Why a file's merged image can't be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PsdError {
    /// Saved without one (Maximize Compatibility off).
    NoComposite,
    /// Zip compression, which Photoshop doesn't use for the merged image.
    Compression(u16),
    /// A colour mode and depth that aren't handled (multichannel, say).
    Mode { mode: u16, depth: u16 },
    /// Not a PSD, truncated, or a part of it contradicts the rest.
    Damaged(String),
}

impl std::fmt::Display for PsdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PsdError::NoComposite => write!(f, "saved without a merged image"),
            PsdError::Compression(n) => write!(f, "merged image compression {n} is not handled"),
            PsdError::Mode { mode, depth } => write!(f, "colour mode {mode} at {depth} bits is not handled"),
            PsdError::Damaged(why) => write!(f, "damaged file: {why}"),
        }
    }
}

impl std::error::Error for PsdError {}

fn damaged(why: impl Into<String>) -> PsdError {
    PsdError::Damaged(why.into())
}

fn io(e: std::io::Error) -> PsdError {
    if e.kind() == ErrorKind::UnexpectedEof { damaged("the file ends early") } else { damaged(e.to_string()) }
}

/// The image's size after its orientation, from the merged image when it
/// can be read, else from the header alone.
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    if let Ok(c) = Composite::open(path) {
        return Some(c.size());
    }
    let mut src = Source::open(path).ok()?;
    let h = Header::read(&mut src).ok()?;
    Some((h.width, h.height))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Bitmap,
    Gray,
    Indexed,
    Rgb,
    Cmyk,
    Duotone,
    Lab,
}

impl Mode {
    /// Channels holding the colour.
    fn channels(self) -> usize {
        match self {
            Mode::Bitmap | Mode::Gray | Mode::Indexed | Mode::Duotone => 1,
            Mode::Rgb | Mode::Lab => 3,
            Mode::Cmyk => 4,
        }
    }
}

struct Source {
    r: BufReader<File>,
    len: u64,
}

impl Source {
    fn open(path: &Path) -> Result<Self, PsdError> {
        let file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        Ok(Source { r: BufReader::new(file), len })
    }

    fn pos(&mut self) -> Result<u64, PsdError> {
        self.r.stream_position().map_err(io)
    }

    fn seek(&mut self, to: u64) -> Result<(), PsdError> {
        self.r.seek(SeekFrom::Start(to)).map_err(io).map(|_| ())
    }

    fn skip(&mut self, n: u64) -> Result<(), PsdError> {
        let to = self.pos()?.saturating_add(n);
        self.seek(to)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], PsdError> {
        let mut b = [0; N];
        self.r.read_exact(&mut b).map_err(io)?;
        Ok(b)
    }

    fn u16(&mut self) -> Result<u16, PsdError> {
        self.array().map(u16::from_be_bytes)
    }

    fn u32(&mut self) -> Result<u32, PsdError> {
        self.array().map(u32::from_be_bytes)
    }

    /// A section length: 32 bits in a PSD, 64 in a PSB.
    fn length(&mut self, psb: bool) -> Result<u64, PsdError> {
        if psb { self.array().map(u64::from_be_bytes) } else { self.u32().map(u64::from) }
    }

    /// `n` bytes from here; the length is checked against the file first.
    fn bytes(&mut self, n: u64) -> Result<Vec<u8>, PsdError> {
        let pos = self.pos()?;
        if pos.saturating_add(n) > self.len {
            return Err(damaged("a section runs past the end of the file"));
        }
        let mut b = vec![0; n as usize];
        self.r.read_exact(&mut b).map_err(io)?;
        Ok(b)
    }
}

struct Header {
    psb: bool,
    channels: u16,
    height: u32,
    width: u32,
    depth: u16,
    mode: u16,
}

impl Header {
    fn read(src: &mut Source) -> Result<Self, PsdError> {
        if &src.array::<4>()? != b"8BPS" {
            return Err(damaged("not a Photoshop document"));
        }
        let psb = match src.u16()? {
            1 => false,
            2 => true,
            v => return Err(damaged(format!("unknown version {v}"))),
        };
        src.array::<6>()?;
        let (channels, height, width, depth, mode) = (src.u16()?, src.u32()?, src.u32()?, src.u16()?, src.u16()?);
        let max = if psb { MAX_SIDE_PSB } else { MAX_SIDE_PSD };
        if width == 0 || height == 0 || width > max || height > max {
            return Err(damaged(format!("impossible size {width} x {height}")));
        }
        if !(1..=56).contains(&channels) {
            return Err(damaged(format!("impossible channel count {channels}")));
        }
        Ok(Header { psb, channels, height, width, depth, mode })
    }

    fn colour(&self) -> Result<Mode, PsdError> {
        let refused = PsdError::Mode { mode: self.mode, depth: self.depth };
        let mode = match (self.mode, self.depth) {
            (0, 1) => Mode::Bitmap,
            (1, 8 | 16 | 32) => Mode::Gray,
            (2, 8) => Mode::Indexed,
            (3, 8 | 16 | 32) => Mode::Rgb,
            (4, 8 | 16) => Mode::Cmyk,
            (8, 8 | 16) => Mode::Duotone,
            (9, 8 | 16) => Mode::Lab,
            _ => return Err(refused),
        };
        Ok(mode)
    }
}

/// What the image resources section says about the merged image.
#[derive(Default)]
struct Resources {
    icc: Option<Vec<u8>>,
    transparent_index: Option<u8>,
    orientation: Option<Orientation>,
    no_merged_data: bool,
}

impl Resources {
    fn read(src: &mut Source, end: u64) -> Result<Self, PsdError> {
        let mut out = Resources::default();
        while src.pos()? + 12 <= end {
            // The signature is 8BIM in nearly every block, but others occur.
            src.array::<4>()?;
            let id = src.u16()?;
            let name = src.array::<1>()?[0] as u64;
            // The Pascal name, with its length byte, padded to even.
            let name_rest = name + 1 - (name % 2);
            src.skip(name_rest)?;
            let size = src.u32()?;
            let start = src.pos()?;
            let next = start.saturating_add(u64::from(size) + u64::from(size % 2));
            if next > end + 1 {
                return Err(damaged("an image resource runs past its section"));
            }
            match id {
                1039 if size <= MAX_ICC_BYTES => out.icc = Some(src.bytes(size.into())?),
                1047 if size >= 2 => out.transparent_index = u8::try_from(src.u16()?).ok(),
                1057 if size >= 5 => {
                    src.u32()?;
                    out.no_merged_data = src.array::<1>()?[0] == 0;
                }
                1058 if size <= MAX_EXIF_BYTES => {
                    let data = src.bytes(size.into())?;
                    let tiff = data.strip_prefix(b"Exif\0\0").unwrap_or(&data);
                    out.orientation = crate::exif::orientation(tiff).and_then(|v| u8::try_from(v).ok()).and_then(Orientation::from_exif);
                }
                _ => {}
            }
            src.seek(next)?;
        }
        Ok(out)
    }
}

/// Whether the layer section (starting after its own length, ending at
/// `end`) says the merged image has transparency.
fn merged_transparency(src: &mut Source, psb: bool, end: u64) -> Result<bool, PsdError> {
    if src.pos()? + if psb { 8 } else { 4 } > end {
        return Ok(false);
    }
    let info = src.length(psb)?;
    let start = src.pos()?;
    if start.saturating_add(info) > end {
        return Err(damaged("the layer info runs past its section"));
    }
    let mut alpha = info >= 2 && (src.u16()? as i16) < 0;
    src.seek(start + info)?;
    if src.pos()? + 4 > end {
        return Ok(alpha);
    }
    let mask = u64::from(src.u32()?);
    src.skip(mask)?;
    // Additional layer information: 16- and 32-bit files keep their layer
    // info here, and the Mt blocks flag the merged transparency.
    while src.pos()? + 12 <= end {
        let mut sig = src.array::<4>()?;
        if &sig != b"8BIM" && &sig != b"8B64" {
            // Some writers pad blocks to four bytes rather than two.
            let back = src.pos()? - 2;
            src.seek(back)?;
            sig = src.array::<4>()?;
            if &sig != b"8BIM" && &sig != b"8B64" {
                break;
            }
        }
        let key = src.array::<4>()?;
        let long = psb && matches!(&key, b"LMsk" | b"Lr16" | b"Lr32" | b"Layr" | b"Mt16" | b"Mt32" | b"Mtrn" | b"Alph" | b"FMsk" | b"lnk2" | b"FEid" | b"FXid" | b"PxSD");
        let len = src.length(long)?;
        let at = src.pos()?;
        match &key {
            b"Mtrn" | b"Mt16" | b"Mt32" => alpha = true,
            b"Lr16" | b"Lr32" if len >= 2 => alpha |= (src.u16()? as i16) < 0,
            _ => {}
        }
        src.seek(at.saturating_add(len).saturating_add(len % 2))?;
    }
    Ok(alpha)
}

/// Where each row of each channel in use lies in the file.
enum Rows {
    Raw { start: u64 },
    /// `(height + 1)` offsets per channel in use.
    Rle { offsets: Vec<u64> },
}

/// An open merged image. Reading it doesn't hold the file in memory.
pub struct Composite {
    file: File,
    width: usize,
    height: usize,
    depth: u16,
    mode: Mode,
    alpha: bool,
    /// Bytes in a row of one channel in the file.
    row_bytes: usize,
    rows: Rows,
    icc: Option<Vec<u8>>,
    /// The colour table: 256 reds, 256 greens, 256 blues.
    palette: Vec<u8>,
    transparent_index: Option<u8>,
    orientation: Orientation,
}

impl Composite {
    /// Reads the structure of the file at `path`, checking every length
    /// against the file's size; pixels are read when asked for, so a huge
    /// header on a small file is refused here.
    pub fn open(path: &Path) -> Result<Self, PsdError> {
        let mut src = Source::open(path)?;
        let h = Header::read(&mut src)?;
        let mode = h.colour()?;
        let colour_len = u64::from(src.u32()?);
        let palette = if mode == Mode::Indexed {
            if colour_len != 768 {
                return Err(damaged("an indexed image without its colour table"));
            }
            src.bytes(768)?
        } else {
            src.skip(colour_len)?;
            Vec::new()
        };
        let resources_len = u64::from(src.u32()?);
        let resources_end = src.pos()?.saturating_add(resources_len);
        if resources_end > src.len {
            return Err(damaged("the image resources run past the end of the file"));
        }
        let resources = Resources::read(&mut src, resources_end)?;
        src.seek(resources_end)?;
        let layers_len = src.length(h.psb)?;
        let layers_end = src.pos()?.saturating_add(layers_len);
        if layers_end > src.len {
            return Err(damaged("the layer section runs past the end of the file"));
        }
        let alpha = merged_transparency(&mut src, h.psb, layers_end)?;
        src.seek(layers_end)?;
        // Maximize Compatibility off: a blank stands in for the merged
        // image, and the version info says so. Some writers leave it out.
        if resources.no_merged_data || layers_end == src.len {
            return Err(PsdError::NoComposite);
        }
        let compression = src.u16()?;
        if compression > 1 {
            return Err(PsdError::Compression(compression));
        }
        let needed = mode.channels();
        let all = usize::from(h.channels);
        if all < needed {
            return Err(damaged(format!("{all} channels where {needed} are needed")));
        }
        let alpha = alpha && all > needed;
        let used = needed + usize::from(alpha);
        let (width, height) = (h.width as usize, h.height as usize);
        let row_bytes = if h.depth == 1 { width.div_ceil(8) } else { width * usize::from(h.depth) / 8 };
        let rest = src.len - src.pos()?;
        let rows = if compression == 0 {
            let total = (row_bytes as u64).saturating_mul(height as u64).saturating_mul(all as u64);
            if total > rest {
                return Err(damaged("the file ends before the merged image does"));
            }
            Rows::Raw { start: src.pos()? }
        } else {
            let entry = if h.psb { 4u64 } else { 2 };
            let table = (height as u64).saturating_mul(all as u64).saturating_mul(entry);
            if table > rest {
                return Err(damaged("the file ends inside the row table"));
            }
            let data = src.pos()? + table;
            let longest = 2 * row_bytes as u64 + 16;
            let mut offsets = Vec::with_capacity((height + 1) * used);
            let mut at = data;
            for channel in 0..all {
                for _ in 0..height {
                    let n = if h.psb { u64::from(src.u32()?) } else { u64::from(src.u16()?) };
                    if n > longest {
                        return Err(damaged("a compressed row longer than any row can be"));
                    }
                    if channel < used {
                        offsets.push(at);
                    }
                    at += n;
                }
                if channel < used {
                    offsets.push(at);
                }
            }
            if at > src.len {
                return Err(damaged("the file ends before the merged image does"));
            }
            Rows::Rle { offsets }
        };
        let file = src.r.into_inner();
        Ok(Composite {
            file,
            width,
            height,
            depth: h.depth,
            mode,
            alpha,
            row_bytes,
            rows,
            icc: resources.icc,
            palette,
            transparent_index: resources.transparent_index,
            orientation: resources.orientation.unwrap_or(Orientation::NoTransforms),
        })
    }

    /// The picture's size in pixels, upright.
    pub fn size(&self) -> (u32, u32) {
        let (w, h) = (self.width as u32, self.height as u32);
        if self.swaps_sides() { (h, w) } else { (w, h) }
    }

    /// Whether the picture has transparency.
    pub fn has_alpha(&self) -> bool {
        self.alpha || (self.mode == Mode::Indexed && self.transparent_index.is_some())
    }

    fn swaps_sides(&self) -> bool {
        matches!(self.orientation, Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH)
    }

    /// The rectangle `[x0, y0, x1, y1]` of the upright picture in the file's
    /// own pixels, clamped to the picture. Empty if outside it.
    fn to_file_rect(&self, rect: [u32; 4]) -> [usize; 4] {
        let (uw, uh) = self.size();
        let [ux0, uy0, ux1, uy1] = [rect[0].min(uw), rect[1].min(uh), rect[2].min(uw), rect[3].min(uh)].map(|v| v as usize);
        let (sw, sh) = (self.width, self.height);
        let map = |u: usize, v: usize| match self.orientation {
            Orientation::NoTransforms => (u, v),
            Orientation::Rotate90 => (v, sh - u),
            Orientation::Rotate180 => (sw - u, sh - v),
            Orientation::Rotate270 => (sw - v, u),
            Orientation::FlipHorizontal => (sw - u, v),
            Orientation::FlipVertical => (u, sh - v),
            Orientation::Rotate90FlipH => (v, u),
            Orientation::Rotate270FlipH => (sw - v, sh - u),
        };
        let (a, b) = (map(ux0, uy0), map(ux1, uy1));
        [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)]
    }

    fn upright(&self, img: RgbaImage) -> RgbaImage {
        if self.orientation == Orientation::NoTransforms {
            return img;
        }
        let mut d = DynamicImage::ImageRgba8(img);
        d.apply_orientation(self.orientation);
        d.into_rgba8()
    }

    /// The rectangles `[x0, y0, x1, y1]` of the upright picture at full
    /// resolution as straight-alpha RGBA. Every row of the file is decoded
    /// once however many rectangles it crosses; memory is the rectangles'
    /// pixels at 4 bytes each, plus what the module notes describe.
    pub fn read_rects(&mut self, rects: &[[u32; 4]]) -> Result<Vec<RgbaImage>, PsdError> {
        let spans: Vec<[usize; 4]> = rects.iter().map(|r| self.to_file_rect(*r)).collect();
        let mut out: Vec<RgbaImage> = spans.iter().map(|s| RgbaImage::new((s[2] - s[0]) as u32, (s[3] - s[1]) as u32)).collect();
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for s in spans.iter().filter(|s| s[2] > s[0] && s[3] > s[1]) {
            (x0, y0, x1, y1) = (x0.min(s[0]), y0.min(s[1]), x1.max(s[2]), y1.max(s[3]));
        }
        if x1 > x0 {
            self.for_each_row([x0, y0, x1, y1], 1, |y, row| {
                for (s, img) in spans.iter().zip(&mut out) {
                    if (s[1]..s[3]).contains(&y) && s[2] > s[0] {
                        let width = (s[2] - s[0]) * 4;
                        let at = (y - s[1]) * width;
                        let from = (s[0] - x0) * 4;
                        let target: &mut [u8] = img;
                        target[at..at + width].copy_from_slice(&row[from..from + width]);
                    }
                }
            })?;
        }
        Ok(out.into_iter().map(|img| self.upright(img)).collect())
    }

    /// The rectangle `[x0, y0, x1, y1]` of the upright picture, box-filtered
    /// down so its long side is at most `long` pixels; never enlarged. The
    /// memory beyond the module notes' is 40 bytes per pixel of one row of
    /// the result, and 4 per pixel of the result. Reduced to under a third,
    /// rows are skipped so a huge file stays quick: at most a factor of 3
    /// rows are averaged, across every column.
    pub fn read_scaled(&mut self, rect: [u32; 4], long: u32) -> Result<RgbaImage, PsdError> {
        let [x0, y0, x1, y1] = self.to_file_rect(rect);
        let (sw, sh) = (x1 - x0, y1 - y0);
        if sw == 0 || sh == 0 {
            return Err(damaged("an empty part of the picture was asked for"));
        }
        let scale = (f64::from(long.max(1)) / sw.max(sh) as f64).min(1.0);
        let ow = ((sw as f64 * scale).round() as usize).clamp(1, sw);
        let oh = ((sh as f64 * scale).round() as usize).clamp(1, sh);
        let stride = (sh / oh / 3).max(1);
        // The output column of each source column, and how many land in it.
        let cols: Vec<u32> = (0..sw).map(|x| (x * ow / sw) as u32).collect();
        let mut weight = vec![0u64; ow];
        for &c in &cols {
            weight[c as usize] += 1;
        }
        // Premultiplied sums, so transparent pixels lend no colour.
        let mut sums = vec![[0u64; 4]; ow];
        let mut out = RgbaImage::new(ow as u32, oh as u32);
        let mut current = 0usize;
        let mut rows_in = 0u64;
        let flush = |out: &mut RgbaImage, sums: &mut [[u64; 4]], at: usize, rows: u64| {
            if rows == 0 {
                return;
            }
            let buf: &mut [u8] = out;
            let row = &mut buf[at * ow * 4..(at + 1) * ow * 4];
            for ((px, s), w) in row.as_chunks_mut::<4>().0.iter_mut().zip(sums.iter_mut()).zip(&weight) {
                match s[3] {
                    0 => {}
                    a => {
                        for c in 0..3 {
                            px[c] = ((s[c] + a / 2) / a) as u8;
                        }
                        let n = w * rows;
                        px[3] = ((a + n / 2) / n) as u8;
                    }
                }
                *s = [0; 4];
            }
        };
        self.for_each_row([x0, y0, x1, y1], stride, |y, row| {
            let at = (y - y0) * oh / sh;
            if at != current {
                flush(&mut out, &mut sums, current, rows_in);
                // A row a stride skipped over repeats the one before.
                for gap in current + 1..at {
                    let (done, rest) = out.split_at_mut(gap * ow * 4);
                    rest[..ow * 4].copy_from_slice(&done[(gap - 1) * ow * 4..]);
                }
                current = at;
                rows_in = 0;
            }
            for (px, &c) in row.as_chunks::<4>().0.iter().zip(&cols) {
                let a = u64::from(px[3]);
                let s = &mut sums[c as usize];
                s[0] += u64::from(px[0]) * a;
                s[1] += u64::from(px[1]) * a;
                s[2] += u64::from(px[2]) * a;
                s[3] += a;
            }
            rows_in += 1;
        })?;
        flush(&mut out, &mut sums, current, rows_in);
        for gap in current + 1..oh {
            let (done, rest) = out.split_at_mut(gap * ow * 4);
            rest[..ow * 4].copy_from_slice(&done[(gap - 1) * ow * 4..]);
        }
        Ok(self.upright(out))
    }

    /// Calls `f` with each `stride`-th row of `rect` (in the file's own
    /// pixels) as straight-alpha 8-bit sRGB.
    fn for_each_row(&mut self, rect: [usize; 4], stride: usize, f: impl FnMut(usize, &[u8])) -> Result<(), PsdError> {
        match self.depth {
            16 => self.rows_of::<u16>(rect, stride, f),
            32 => self.rows_of::<f32>(rect, stride, f),
            _ => self.rows_of::<u8>(rect, stride, f),
        }
    }

    fn rows_of<T: Sample>(&mut self, rect: [usize; 4], stride: usize, mut f: impl FnMut(usize, &[u8])) -> Result<(), PsdError> {
        let [x0, y0, x1, y1] = rect;
        let n = x1 - x0;
        let colour = self.mode.channels();
        let used = colour + usize::from(self.alpha);
        let mut chunks: Vec<Chunk> = (0..used).map(|_| Chunk::default()).collect();
        // A decoded row per channel: bitmap rows are expanded to bytes.
        let mut planes = vec![vec![0u8; self.width * T::BYTES]; used];
        let mut unpacked = vec![0u8; self.row_bytes];
        let converter = Converter::<T>::new(self.mode, self.icc.as_deref());
        let mut rgba = vec![0u8; n * 4];
        let mut work = Work::<T>::new(n, colour);
        let mut y = y0;
        while y < y1 {
            for (channel, plane) in planes.iter_mut().enumerate() {
                let raw = self.read_row(&mut chunks[channel], channel, y, y1, stride)?;
                match &self.rows {
                    Rows::Raw { .. } => unpacked.copy_from_slice(raw),
                    Rows::Rle { .. } => unpack(raw, &mut unpacked)?,
                }
                if self.mode == Mode::Bitmap {
                    // 1 is black.
                    for (x, p) in plane.iter_mut().enumerate() {
                        *p = if unpacked[x / 8] & (0x80 >> (x % 8)) != 0 { 0 } else { 255 };
                    }
                } else {
                    plane.copy_from_slice(&unpacked);
                }
            }
            work.load(&planes, x0, n, self.alpha, colour);
            self.convert(&converter, &mut work, &mut rgba)?;
            f(y, &rgba);
            y += stride;
        }
        Ok(())
    }

    /// The bytes of row `y` of a channel in use, from its chunk, which is
    /// read when it doesn't hold them.
    fn read_row<'c>(&mut self, chunk: &'c mut Chunk, channel: usize, y: usize, end: usize, stride: usize) -> Result<&'c [u8], PsdError> {
        let span = |r: usize| -> (u64, u64) {
            match &self.rows {
                Rows::Raw { start } => {
                    let rb = self.row_bytes as u64;
                    let at = start + channel as u64 * self.height as u64 * rb + r as u64 * rb;
                    (at, at + rb)
                }
                Rows::Rle { offsets } => {
                    let i = channel * (self.height + 1) + r;
                    (offsets[i], offsets[i + 1])
                }
            }
        };
        if !(chunk.first..chunk.end).contains(&y) {
            let (from, mut to) = span(y);
            let mut last = y + 1;
            // Rows being skipped aren't read ahead.
            while stride == 1 && last < end && span(last).1 - from <= CHUNK_BYTES {
                to = span(last).1;
                last += 1;
            }
            chunk.first = y;
            chunk.end = 0;
            chunk.base = from;
            chunk.data.resize((to - from) as usize, 0);
            self.file.seek(SeekFrom::Start(from)).map_err(io)?;
            self.file.read_exact(&mut chunk.data).map_err(io)?;
            chunk.end = last;
        }
        let (from, to) = span(y);
        Ok(&chunk.data[(from - chunk.base) as usize..(to - chunk.base) as usize])
    }

    /// Turns the loaded samples in `work` into RGBA.
    fn convert<T: Sample>(&self, conv: &Converter<T>, work: &mut Work<T>, rgba: &mut [u8]) -> Result<(), PsdError> {
        let n = work.alpha.len();
        // The file's samples are blended over white; in every mode but Lab
        // full-scale is white (CMYK is stored inverted: full scale, no ink).
        if self.alpha && self.mode != Mode::Lab {
            unmatte(&mut work.colour, &work.alpha, self.mode.channels());
        }
        let encoded = conv.transform.is_some();
        match self.mode {
            Mode::Rgb => match &conv.transform {
                Some(t) => t.transform(&work.colour, &mut work.rgb).map_err(|e| damaged(e.to_string()))?,
                None => work.rgb.copy_from_slice(&work.colour),
            },
            Mode::Gray | Mode::Duotone | Mode::Bitmap => match &conv.transform {
                Some(t) => t.transform(&work.colour, &mut work.rgb).map_err(|e| damaged(e.to_string()))?,
                None => {
                    for (px, v) in work.rgb.as_chunks_mut::<3>().0.iter_mut().zip(&work.colour) {
                        px.fill(*v);
                    }
                }
            },
            Mode::Indexed => {
                // Only 8-bit: the sample is the index.
                for (px, v) in work.indexed.as_chunks_mut::<3>().0.iter_mut().zip(&work.colour) {
                    let i = usize::from(v.to_u8());
                    px.copy_from_slice(&[T::from_u8(self.palette[i]), T::from_u8(self.palette[256 + i]), T::from_u8(self.palette[512 + i])]);
                }
                match &conv.transform {
                    Some(t) => t.transform(&work.indexed, &mut work.rgb).map_err(|e| damaged(e.to_string()))?,
                    None => work.rgb.copy_from_slice(&work.indexed),
                }
            }
            Mode::Cmyk => match &conv.transform {
                Some(t) => {
                    // ICC CMYK counts ink; the file counts its absence.
                    for v in &mut work.colour {
                        *v = T::from_unit(1.0 - v.unit());
                    }
                    t.transform(&work.colour, &mut work.rgb).map_err(|e| damaged(e.to_string()))?;
                }
                None => {
                    for (px, s) in work.rgb.as_chunks_mut::<3>().0.iter_mut().zip(work.colour.as_chunks::<4>().0) {
                        let k = s[3].unit();
                        px.copy_from_slice(&[T::from_unit(s[0].unit() * k), T::from_unit(s[1].unit() * k), T::from_unit(s[2].unit() * k)]);
                    }
                }
            },
            Mode::Lab => {
                for (px, s) in work.rgb.as_chunks_mut::<3>().0.iter_mut().zip(work.colour.as_chunks::<3>().0) {
                    let [r, g, b] = lab_to_srgb(s[0].unit() * 100.0, s[1].lab_ab(), s[2].lab_ab());
                    px.copy_from_slice(&[T::from_unit(r), T::from_unit(g), T::from_unit(b)]);
                }
            }
        }
        // Lab is already encoded for display.
        let encoded = encoded || self.mode == Mode::Lab;
        for i in 0..n {
            let px = &mut rgba[i * 4..i * 4 + 4];
            for (c, out) in px.iter_mut().take(3).enumerate() {
                let v = work.rgb[i * 3 + c];
                *out = if encoded { v.to_u8() } else { v.display() };
            }
            px[3] = if self.alpha {
                work.alpha[i].to_u8()
            } else if self.mode == Mode::Indexed && self.transparent_index == Some(work.colour[i].to_u8()) {
                0
            } else {
                255
            };
        }
        Ok(())
    }
}

/// Compressed bytes of a run of rows of one channel.
#[derive(Default)]
struct Chunk {
    first: usize,
    end: usize,
    base: u64,
    data: Vec<u8>,
}

/// PackBits: `dst` must come out exactly full.
fn unpack(src: &[u8], dst: &mut [u8]) -> Result<(), PsdError> {
    let (mut i, mut o) = (0, 0);
    while o < dst.len() {
        let &n = src.get(i).ok_or_else(|| damaged("a compressed row ends early"))?;
        i += 1;
        if n == 128 {
            continue;
        }
        if n < 128 {
            let len = usize::from(n) + 1;
            let run = src.get(i..i + len).ok_or_else(|| damaged("a compressed row ends early"))?;
            dst.get_mut(o..o + len).ok_or_else(|| damaged("a compressed row is too long"))?.copy_from_slice(run);
            i += len;
            o += len;
        } else {
            let len = 257 - usize::from(n);
            let &v = src.get(i).ok_or_else(|| damaged("a compressed row ends early"))?;
            i += 1;
            dst.get_mut(o..o + len).ok_or_else(|| damaged("a compressed row is too long"))?.fill(v);
            o += len;
        }
    }
    Ok(())
}

/// Takes the white out of samples blended over it.
fn unmatte<T: Sample>(colour: &mut [T], alpha: &[T], channels: usize) {
    for (px, a) in colour.chunks_exact_mut(channels).zip(alpha) {
        let a = a.unit();
        if a > 0.0 && a < 1.0 {
            for v in px {
                *v = T::from_unit((v.unit() - (1.0 - a)) / a);
            }
        }
    }
}

/// A CIE Lab colour (D50) as sRGB samples in 0..1.
fn lab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let inv = |t: f32| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    let (x, y, z) = (0.9642 * inv(fx), inv(fy), 0.8249 * inv(fz));
    // XYZ (D50) to linear sRGB, Bradford adapted.
    let r = 3.133856 * x - 1.616867 * y - 0.490615 * z;
    let g = -0.978768 * x + 1.916142 * y + 0.033454 * z;
    let bl = 0.071945 * x - 0.228991 * y + 1.405243 * z;
    [srgb_encode(r), srgb_encode(g), srgb_encode(bl)]
}

fn srgb_encode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

type Executor<T> = Arc<dyn TransformExecutor<T> + Send + Sync>;

/// A sample of the file's depth.
trait Sample: Copy + Default + 'static {
    const BYTES: usize;
    fn be(b: &[u8]) -> Self;
    /// In 0..1 (a 32-bit sample may lie outside).
    fn unit(self) -> f32;
    fn from_unit(v: f32) -> Self;
    fn from_u8(v: u8) -> Self;
    /// Rounded to 8 bits, as it stands.
    fn to_u8(self) -> u8;
    /// Rounded to 8 bits for display with no profile to go through.
    fn display(self) -> u8 {
        self.to_u8()
    }
    /// An `a*` or `b*` value in Lab.
    fn lab_ab(self) -> f32;
    fn executor(profile: &ColorProfile, src: Layout, dst: &ColorProfile) -> Option<Executor<Self>>;
}

impl Sample for u8 {
    const BYTES: usize = 1;
    fn be(b: &[u8]) -> Self {
        b[0]
    }
    fn unit(self) -> f32 {
        f32::from(self) / 255.0
    }
    fn from_unit(v: f32) -> Self {
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    }
    fn from_u8(v: u8) -> Self {
        v
    }
    fn to_u8(self) -> u8 {
        self
    }
    fn lab_ab(self) -> f32 {
        f32::from(self) - 128.0
    }
    fn executor(profile: &ColorProfile, src: Layout, dst: &ColorProfile) -> Option<Executor<Self>> {
        profile.create_transform_8bit(src, dst, Layout::Rgb, TransformOptions::default()).ok().map(|t| t as Executor<Self>)
    }
}

impl Sample for u16 {
    const BYTES: usize = 2;
    fn be(b: &[u8]) -> Self {
        u16::from_be_bytes([b[0], b[1]])
    }
    fn unit(self) -> f32 {
        f32::from(self) / 65535.0
    }
    fn from_unit(v: f32) -> Self {
        (v.clamp(0.0, 1.0) * 65535.0).round() as u16
    }
    fn from_u8(v: u8) -> Self {
        u16::from(v) * 257
    }
    fn to_u8(self) -> u8 {
        ((u32::from(self) * 255 + 32767) / 65535) as u8
    }
    fn lab_ab(self) -> f32 {
        f32::from(self) / 256.0 - 128.0
    }
    fn executor(profile: &ColorProfile, src: Layout, dst: &ColorProfile) -> Option<Executor<Self>> {
        profile.create_transform_16bit(src, dst, Layout::Rgb, TransformOptions::default()).ok()
    }
}

impl Sample for f32 {
    const BYTES: usize = 4;
    fn be(b: &[u8]) -> Self {
        f32::from_be_bytes([b[0], b[1], b[2], b[3]])
    }
    fn unit(self) -> f32 {
        self
    }
    fn from_unit(v: f32) -> Self {
        v
    }
    fn from_u8(v: u8) -> Self {
        f32::from(v) / 255.0
    }
    fn to_u8(self) -> u8 {
        // NaN becomes 0.
        (self.clamp(0.0, 1.0) * 255.0).round() as u8
    }
    /// 32-bit samples are linear light.
    fn display(self) -> u8 {
        (srgb_encode(self) * 255.0).round() as u8
    }
    fn lab_ab(self) -> f32 {
        self * 255.0 - 128.0
    }
    fn executor(profile: &ColorProfile, src: Layout, dst: &ColorProfile) -> Option<Executor<Self>> {
        profile.create_transform_f32(src, dst, Layout::Rgb, TransformOptions::default()).ok()
    }
}

/// The embedded profile's transform to sRGB, when the mode uses one and it
/// is a profile for that mode. A bad profile is treated as none.
struct Converter<T: Sample> {
    transform: Option<Executor<T>>,
}

impl<T: Sample> Converter<T> {
    fn new(mode: Mode, icc: Option<&[u8]>) -> Self {
        let layout = match mode {
            Mode::Rgb | Mode::Indexed => Some(Layout::Rgb),
            Mode::Gray => Some(Layout::Gray),
            // CMYK samples use moxcms's four-channel layout.
            Mode::Cmyk => Some(Layout::Rgba),
            Mode::Lab | Mode::Duotone | Mode::Bitmap => None,
        };
        let space = match mode {
            Mode::Rgb | Mode::Indexed => moxcms::DataColorSpace::Rgb,
            Mode::Gray => moxcms::DataColorSpace::Gray,
            _ => moxcms::DataColorSpace::Cmyk,
        };
        let transform = layout.zip(icc).and_then(|(layout, icc)| {
            let profile = ColorProfile::new_from_slice(icc).ok()?;
            (profile.color_space == space).then(|| T::executor(&profile, layout, &ColorProfile::new_srgb())).flatten()
        });
        Converter { transform }
    }
}

/// Buffers for one row, interleaved for the colour transform.
struct Work<T> {
    colour: Vec<T>,
    alpha: Vec<T>,
    indexed: Vec<T>,
    rgb: Vec<T>,
}

impl<T: Sample> Work<T> {
    fn new(n: usize, channels: usize) -> Self {
        Work { colour: vec![T::default(); n * channels], alpha: vec![T::default(); n], indexed: vec![T::default(); n * 3], rgb: vec![T::default(); n * 3] }
    }

    fn load(&mut self, planes: &[Vec<u8>], x0: usize, n: usize, alpha: bool, channels: usize) {
        for (c, plane) in planes.iter().take(channels).enumerate() {
            let samples = plane[x0 * T::BYTES..(x0 + n) * T::BYTES].chunks_exact(T::BYTES);
            for (i, s) in samples.enumerate() {
                self.colour[i * channels + c] = T::be(s);
            }
        }
        if alpha {
            let samples = planes[channels][x0 * T::BYTES..(x0 + n) * T::BYTES].chunks_exact(T::BYTES);
            for (v, s) in self.alpha.iter_mut().zip(samples) {
                *v = T::be(s);
            }
        }
    }
}

#[cfg(test)]
mod tests;
