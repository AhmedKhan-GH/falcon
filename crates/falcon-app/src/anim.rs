//! Animated images as a stream of frames, decoded on a background thread.
//!
//! A [`Player`] opens a GIF, animated WebP or APNG. For a GIF it indexes its frames and their delays in a cheap
//! first pass (walking the file's blocks, decompressing nothing), and asks
//! its worker thread for the frames around the playhead; the worker reads
//! each frame from the file as it is needed. Only a bounded window of
//! decoded frames, plus canvas snapshots for seeking, is ever held, so a GIF
//! of thousands of frames costs about as much memory as one of a few dozen.
//! WebP and APNG frame counts and delays come from their chunk headers, and
//! their frames are decoded in order by the `image` crate. Frame timing, loop
//! ranges, speed steps and the scrub bar's mapping are pure functions with
//! tests.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use eframe::egui;
use image::RgbaImage;

/// The formats a [`Player`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gif,
    WebP,
    /// PNG, animated when it is an APNG.
    Png,
}

/// The format of an extension falcon may animate. WebP and PNG files are
/// often stills; their players then report [`State::Still`].
pub fn kind(ext: &str) -> Option<Kind> {
    match ext.to_lowercase().as_str() {
        "gif" => Some(Kind::Gif),
        "webp" => Some(Kind::WebP),
        "png" => Some(Kind::Png),
        _ => None,
    }
}

/// A frame's delay as browsers (and so Eagle's viewer) show it: delays of
/// 10 ms or less play at 100 ms.
pub fn frame_delay_ms(raw: u32) -> u32 {
    if raw <= 10 { 100 } else { raw }
}

// ---------------------------------------------------------------------------
// Timing

/// A loop range: playback repeats frames `first..=last`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopRange {
    pub first: usize,
    pub last: usize,
}

/// The frames playback runs through: the range, or all `total` frames.
fn bounds(total: usize, range: Option<LoopRange>) -> (usize, usize) {
    let last = total.saturating_sub(1);
    match range {
        Some(r) => (r.first.min(last), r.last.min(last).max(r.first.min(last))),
        None => (0, last),
    }
}

/// The frame after `cur` in play order, wrapping inside the range.
pub fn next_frame(cur: usize, total: usize, range: Option<LoopRange>) -> usize {
    let (a, b) = bounds(total, range);
    if cur < a || cur >= b { a } else { cur + 1 }
}

/// `cur` moved into the range: a frame outside it goes to the range's
/// first frame, as in Eagle.
pub fn into_range(cur: usize, total: usize, range: Option<LoopRange>) -> usize {
    let (a, b) = bounds(total, range);
    if cur < a || cur > b { a } else { cur }
}

/// `cur` moved `n` frames, stopping at the range's ends (frame steps don't
/// wrap).
pub fn step_frame(cur: usize, n: i64, total: usize, range: Option<LoopRange>) -> usize {
    let (a, b) = bounds(total, range);
    (cur as i64 + n).clamp(a as i64, b as i64) as usize
}

/// Advances playback by `dt_ms` from `offset_ms` into `frame`. Playback
/// waits at the end of a frame when the next one isn't decoded yet
/// (`ready` says which are). Returns the new frame and offset.
pub fn advance(
    delays: &[u32],
    range: Option<LoopRange>,
    frame: usize,
    offset_ms: f64,
    dt_ms: f64,
    ready: impl Fn(usize) -> bool,
) -> (usize, f64) {
    let total = delays.len();
    if total == 0 {
        return (0, 0.0);
    }
    let (a, b) = bounds(total, range);
    let mut cur = into_range(frame, total, range);
    // Skip whole loops at once after a long pause between repaints.
    let span: f64 = delays[a..=b].iter().map(|&d| f64::from(frame_delay_ms(d))).sum();
    let mut left = offset_ms.max(0.0) + dt_ms.max(0.0);
    if span > 0.0 && left > span * 2.0 {
        left = left % span + span;
    }
    for _ in 0..(total * 3 + 2) {
        let d = f64::from(frame_delay_ms(delays[cur]));
        if left < d {
            return (cur, left);
        }
        let next = next_frame(cur, total, range);
        if !ready(next) {
            return (cur, d);
        }
        left -= d;
        cur = next;
    }
    (cur, 0.0)
}

/// A new loop range after its start handle is dropped on frame `at`; the
/// ends swap when they cross, as in Eagle.
pub fn range_with_first(range: Option<LoopRange>, at: usize, total: usize) -> LoopRange {
    let last = range.map_or(total.saturating_sub(1), |r| r.last);
    LoopRange { first: at.min(last), last: at.max(last) }
}

/// A new loop range after its end handle is dropped on frame `at`.
pub fn range_with_last(range: Option<LoopRange>, at: usize) -> LoopRange {
    let first = range.map_or(0, |r| r.first);
    LoopRange { first: first.min(at), last: first.max(at) }
}

/// The frame under `x` on a scrub bar from `left`, `width` wide, of `total`
/// frames (0-based), as Eagle maps it.
pub fn frame_at_x(x: f32, left: f32, width: f32, total: usize) -> usize {
    if total == 0 || width <= 0.0 {
        return 0;
    }
    let f = ((x - left) / width * total as f32).round();
    (f.max(0.0) as usize).min(total - 1)
}

/// Where frame `index` sits on the scrub bar: the first frame at the left
/// end, the last at the right end.
pub fn frame_x(index: usize, total: usize, left: f32, width: f32) -> f32 {
    if total < 2 {
        return left;
    }
    left + width * (index.min(total - 1) as f32 / (total - 1) as f32)
}

/// The frame counter: the current frame (1-based) padded to the total's
/// digits, then the total.
pub fn counter_label(index: usize, total: usize) -> (String, String) {
    let digits = total.to_string().len();
    (format!("{:0digits$}", index + 1), format!("/ {total}"))
}

/// Eagle's GIF playback speeds, slowest first.
pub const SPEEDS: [f32; 11] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 8.0];

/// The next speed step up or down from `speed`.
pub fn step_speed(speed: f32, up: bool) -> f32 {
    if up {
        SPEEDS.iter().copied().find(|&s| s > speed + 1e-4).unwrap_or(speed)
    } else {
        SPEEDS.iter().rev().copied().find(|&s| s < speed - 1e-4).unwrap_or(speed)
    }
}

/// A speed as the menu shows it: "1x", "0.25x", "1.75x".
pub fn speed_label(speed: f32) -> String {
    format!("{}x", format!("{speed:.2}").trim_end_matches('0').trim_end_matches('.'))
}

// ---------------------------------------------------------------------------
// Frame sources

/// Decodes frame `i` of an animation of known size and delays.
trait Source {
    fn size(&self) -> (u32, u32);
    fn delays(&self) -> &[u32];
    fn frame(&mut self, i: usize) -> Result<RgbaImage, String>;
}

/// Largest canvas a GIF may have, in pixels.
const MAX_PIXELS: u64 = 64_000_000;

/// Where one GIF frame's data is in the file, with what it needs to be
/// drawn.
struct FrameRef {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    interlaced: bool,
    palette: Option<Vec<u8>>,
    transparent: Option<u8>,
    dispose: gif::DisposalMethod,
    delay_cs: u16,
    /// Offset of the LZW minimum code size byte, and the length of it plus
    /// the data sub-blocks and their terminator.
    data: (u64, usize),
}

/// A GIF's screen size, global palette and frames, found by walking its
/// blocks without decompressing anything.
struct GifIndex {
    w: u16,
    h: u16,
    palette: Option<Vec<u8>>,
    frames: Vec<FrameRef>,
}

/// A reader that counts the bytes it has passed.
struct Walker<R> {
    r: R,
    pos: u64,
}

impl<R: std::io::BufRead + std::io::Seek> Walker<R> {
    fn bytes<const N: usize>(&mut self) -> std::io::Result<[u8; N]> {
        let mut b = [0; N];
        self.r.read_exact(&mut b)?;
        self.pos += N as u64;
        Ok(b)
    }

    fn byte(&mut self) -> std::io::Result<u8> {
        Ok(self.bytes::<1>()?[0])
    }

    fn u16(&mut self) -> std::io::Result<u16> {
        Ok(u16::from_le_bytes(self.bytes::<2>()?))
    }

    fn vec(&mut self, n: usize) -> std::io::Result<Vec<u8>> {
        let mut v = vec![0; n];
        self.r.read_exact(&mut v)?;
        self.pos += n as u64;
        Ok(v)
    }

    /// Skips data sub-blocks up to and including their terminator.
    fn skip_blocks(&mut self) -> std::io::Result<()> {
        loop {
            let n = self.byte()?;
            if n == 0 {
                return Ok(());
            }
            self.r.seek_relative(i64::from(n))?;
            self.pos += u64::from(n);
            // `seek_relative` past the end doesn't fail; reading does.
        }
    }
}

/// Size of a colour table from a packed field's low bits.
fn table_len(packed: u8) -> usize {
    3 * (1 << ((packed & 7) + 1))
}

impl GifIndex {
    /// Walks the GIF's blocks. A file damaged part way keeps the frames
    /// before the damage.
    fn read(r: impl std::io::BufRead + std::io::Seek) -> Result<GifIndex, String> {
        let mut w = Walker { r, pos: 0 };
        let err = |e: std::io::Error| e.to_string();
        let head: [u8; 6] = w.bytes().map_err(err)?;
        if &head[..3] != b"GIF" {
            return Err("not a GIF".into());
        }
        let (sw, sh) = (w.u16().map_err(err)?, w.u16().map_err(err)?);
        let [packed, _bg, _aspect] = w.bytes().map_err(err)?;
        let palette = if packed & 0x80 != 0 { Some(w.vec(table_len(packed)).map_err(err)?) } else { None };
        let mut frames = Vec::new();
        // The graphic control extension for the next image.
        let mut control: Option<(gif::DisposalMethod, u16, Option<u8>)> = None;
        let mut walk = || -> std::io::Result<()> {
            loop {
                match w.byte()? {
                    0x21 => {
                        let label = w.byte()?;
                        if label == 0xF9 {
                            let size = w.byte()?;
                            if size >= 4 {
                                let [p, d0, d1, t] = w.bytes()?;
                                let dispose = gif::DisposalMethod::from_u8((p >> 2) & 7).unwrap_or(gif::DisposalMethod::Any);
                                control = Some((dispose, u16::from_le_bytes([d0, d1]), (p & 1 != 0).then_some(t)));
                                w.r.seek_relative(i64::from(size - 4))?;
                                w.pos += u64::from(size - 4);
                            } else {
                                w.r.seek_relative(i64::from(size))?;
                                w.pos += u64::from(size);
                            }
                        }
                        w.skip_blocks()?;
                    }
                    0x2C => {
                        let (left, top, width, height) = (w.u16()?, w.u16()?, w.u16()?, w.u16()?);
                        let p = w.byte()?;
                        let local = if p & 0x80 != 0 { Some(w.vec(table_len(p))?) } else { None };
                        let start = w.pos;
                        w.byte()?;
                        w.skip_blocks()?;
                        let (dispose, delay_cs, transparent) = control.take().unwrap_or((gif::DisposalMethod::Any, 0, None));
                        frames.push(FrameRef {
                            left,
                            top,
                            width,
                            height,
                            interlaced: p & 0x40 != 0,
                            palette: local,
                            transparent,
                            dispose,
                            delay_cs,
                            data: (start, (w.pos - start) as usize),
                        });
                    }
                    _ => return Ok(()),
                }
            }
        };
        let _ = walk();
        // A last frame cut short is dropped.
        let end = w.r.seek(std::io::SeekFrom::End(0)).map_err(err)?;
        while frames.last().is_some_and(|f| f.data.0 + f.data.1 as u64 > end) {
            frames.pop();
        }
        Ok(GifIndex { w: sw, h: sh, palette, frames })
    }
}

/// A GIF indexed once; each frame is read from the file, decompressed and
/// composited when asked for. Snapshots of the canvas every `every` frames
/// make seeking cheap.
struct GifSource {
    w: u32,
    h: u32,
    file: std::fs::File,
    index: GifIndex,
    delays: Vec<u32>,
    decoder: gif::streaming_decoder::FrameDecoder,
    /// The canvas as frame `next` will be drawn onto.
    canvas: Vec<u8>,
    next: usize,
    /// Canvases as frame `k` is drawn onto, for every `every`-th `k`.
    checkpoints: BTreeMap<usize, Vec<u8>>,
    every: usize,
    data: Vec<u8>,
    pixels: Vec<u8>,
}

impl GifSource {
    /// Indexes `path`; `budget` bounds the bytes spent on checkpoints.
    fn open(path: &Path, budget: usize) -> Result<GifSource, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let index = GifIndex::read(std::io::BufReader::new(file.try_clone().map_err(|e| e.to_string())?))?;
        let (w, h) = (u32::from(index.w), u32::from(index.h));
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return Err(format!("unsupported GIF size {w}×{h}"));
        }
        if index.frames.is_empty() {
            return Err("no frames".into());
        }
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = gif::streaming_decoder::FrameDecoder::new(options);
        if let Some(p) = &index.palette {
            decoder.set_global_palette(p.clone());
        }
        let delays = index.frames.iter().map(|f| u32::from(f.delay_cs) * 10).collect();
        let canvas_bytes = (w * h * 4) as usize;
        let n = index.frames.len();
        let every = (n * canvas_bytes).div_ceil(budget.max(1)).clamp(8, n.max(8)).next_multiple_of(8);
        let canvas = vec![0; canvas_bytes];
        let checkpoints = BTreeMap::from([(0, canvas.clone())]);
        Ok(GifSource { w, h, file, index, delays, decoder, canvas, next: 0, checkpoints, every, data: Vec::new(), pixels: Vec::new() })
    }

    /// Bytes held by checkpoints.
    #[cfg(test)]
    fn checkpoint_bytes(&self) -> usize {
        self.checkpoints.values().map(Vec::len).sum()
    }

    /// Reads frame `k`'s pixels (RGBA, the frame's own rectangle) into
    /// `self.pixels`.
    fn decode(&mut self, k: usize) -> Result<(), String> {
        use std::io::{Read, Seek};
        let f = &self.index.frames[k];
        let (at, len) = f.data;
        let mut raw = vec![0; len];
        self.file.seek(std::io::SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        self.file.read_exact(&mut raw).map_err(|e| e.to_string())?;
        // The minimum code size, then the sub-blocks without their lengths.
        self.data.clear();
        self.data.push(raw[0]);
        let mut i = 1;
        while i < raw.len() {
            let n = raw[i] as usize;
            if n == 0 {
                break;
            }
            self.data.extend_from_slice(&raw[(i + 1).min(raw.len())..(i + 1 + n).min(raw.len())]);
            i += 1 + n;
        }
        let frame = gif::Frame {
            delay: f.delay_cs,
            dispose: f.dispose,
            transparent: f.transparent,
            needs_user_input: false,
            top: f.top,
            left: f.left,
            width: f.width,
            height: f.height,
            interlaced: f.interlaced,
            palette: f.palette.clone(),
            buffer: std::borrow::Cow::Borrowed(&self.data),
        };
        self.pixels.resize(usize::from(f.width) * usize::from(f.height) * 4, 0);
        self.decoder.decode_lzw_encoded_frame_into_buffer(&frame, &mut self.pixels).map_err(|e| e.to_string())
    }

    /// Draws frame `next` onto the canvas, returns the picture if asked,
    /// then disposes of the frame.
    fn draw_next(&mut self, keep: bool) -> Result<Option<RgbaImage>, String> {
        let k = self.next;
        if k.is_multiple_of(self.every) && !self.checkpoints.contains_key(&k) {
            self.checkpoints.insert(k, self.canvas.clone());
        }
        // A frame that fails to decode leaves the canvas as it was.
        let drawn = self.decode(k).is_ok();
        let f = &self.index.frames[k];
        let (fw, fh) = (usize::from(f.width), usize::from(f.height));
        let (cw, ch) = (self.w as usize, self.h as usize);
        let (left, top) = (usize::from(f.left), usize::from(f.top));
        // The part of the frame inside the canvas.
        let x0 = left.min(cw);
        let x1 = (left + fw).min(cw);
        let y0 = top.min(ch);
        let y1 = (top + fh).min(ch);
        let saved: Option<Vec<u8>> = (f.dispose == gif::DisposalMethod::Previous).then(|| {
            let mut s = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
            for y in y0..y1 {
                s.extend_from_slice(&self.canvas[(y * cw + x0) * 4..(y * cw + x1) * 4]);
            }
            s
        });
        if drawn {
            for y in y0..y1 {
                for x in x0..x1 {
                    let src = ((y - top) * fw + (x - left)) * 4;
                    if self.pixels[src + 3] != 0 {
                        let dst = (y * cw + x) * 4;
                        self.canvas[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
                    }
                }
            }
        }
        let out = keep.then(|| RgbaImage::from_raw(self.w, self.h, self.canvas.clone())).flatten();
        match f.dispose {
            gif::DisposalMethod::Background => {
                for y in y0..y1 {
                    self.canvas[(y * cw + x0) * 4..(y * cw + x1) * 4].fill(0);
                }
            }
            gif::DisposalMethod::Previous => {
                if let Some(s) = saved {
                    let row = (x1 - x0) * 4;
                    for (n, y) in (y0..y1).enumerate() {
                        self.canvas[(y * cw + x0) * 4..(y * cw + x1) * 4].copy_from_slice(&s[n * row..(n + 1) * row]);
                    }
                }
            }
            _ => {}
        }
        self.next = k + 1;
        Ok(out)
    }
}

impl Source for GifSource {
    fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }

    fn delays(&self) -> &[u32] {
        &self.delays
    }

    fn frame(&mut self, i: usize) -> Result<RgbaImage, String> {
        if i >= self.index.frames.len() {
            return Err(format!("no frame {i}"));
        }
        // Going back, or far ahead: start from the nearest snapshot.
        if i < self.next || i - self.next > self.every {
            let (&k, c) = self.checkpoints.range(..=i).next_back().expect("frame 0 is always a checkpoint");
            if k > self.next || i < self.next {
                self.canvas.copy_from_slice(c);
                self.next = k;
            }
        }
        while self.next < i {
            self.draw_next(false)?;
        }
        self.draw_next(true)?.ok_or_else(|| "bad frame".to_string())
    }
}

/// Size and frame delays (ms) of an animated WebP, from its chunk headers;
/// `None` for a still WebP or anything else.
fn webp_info(r: impl std::io::BufRead + std::io::Seek) -> Option<((u32, u32), Vec<u32>)> {
    let mut w = Walker { r, pos: 0 };
    let head: [u8; 12] = w.bytes().ok()?;
    if &head[..4] != b"RIFF" || &head[8..] != b"WEBP" {
        return None;
    }
    let u24 = |b: &[u8]| u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16;
    let mut size = None;
    let mut delays = Vec::new();
    while let Ok(c) = w.bytes::<8>() {
        let len = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
        let padded = i64::from(len) + i64::from(len & 1);
        let mut used = 0;
        match &c[..4] {
            b"VP8X" if len >= 10 => {
                let x: [u8; 10] = w.bytes().ok()?;
                if x[0] & 0x02 == 0 {
                    return None;
                }
                size = Some((u24(&x[4..7]) + 1, u24(&x[7..10]) + 1));
                used = 10;
            }
            b"ANMF" if len >= 16 => {
                let a: [u8; 16] = w.bytes().ok()?;
                delays.push(u24(&a[12..15]));
                used = 16;
            }
            // Image data before any VP8X chunk: a simple, still WebP.
            b"VP8 " | b"VP8L" if size.is_none() => return None,
            _ => {}
        }
        if w.r.seek_relative(padded - used).is_err() {
            break;
        }
    }
    Some((size?, delays)).filter(|(_, d)| !d.is_empty())
}

/// Size and frame delays (ms) of an animated PNG, from its `acTL` and
/// `fcTL` chunks; `None` for a still PNG or anything else.
fn apng_info(r: impl std::io::BufRead + std::io::Seek) -> Option<((u32, u32), Vec<u32>)> {
    let mut w = Walker { r, pos: 0 };
    let sig: [u8; 8] = w.bytes().ok()?;
    if &sig != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let (mut size, mut animated) = (None, false);
    let mut delays = Vec::new();
    while let Ok(c) = w.bytes::<8>() {
        let len = be(&c[..4]);
        let mut used = 0;
        match &c[4..] {
            b"IHDR" if len >= 8 => {
                let d: [u8; 8] = w.bytes().ok()?;
                size = Some((be(&d[..4]), be(&d[4..])));
                used = 8;
            }
            b"acTL" => animated = true,
            b"fcTL" if len >= 26 => {
                let d: [u8; 26] = w.bytes().ok()?;
                let num = u32::from(u16::from_be_bytes([d[20], d[21]]));
                let den = match u16::from_be_bytes([d[22], d[23]]) {
                    0 => 100,
                    n => u32::from(n),
                };
                delays.push(num * 1000 / den);
                used = 26;
            }
            // `acTL` must come before the image data.
            b"IDAT" if !animated => return None,
            b"IEND" => break,
            _ => {}
        }
        if w.r.seek_relative(i64::from(len) - used + 4).is_err() {
            break;
        }
    }
    Some((size?, delays)).filter(|(_, d)| animated && !d.is_empty())
}

/// An animated WebP or PNG, decoded in order by the `image` crate (which
/// composites the frames). Going back starts again from the first frame.
struct SeqSource {
    path: PathBuf,
    kind: Kind,
    size: (u32, u32),
    delays: Vec<u32>,
    frames: Option<image::Frames<'static>>,
    next: usize,
}

impl SeqSource {
    /// Reads the headers of `path`; `Ok(None)` when it isn't animated.
    fn open(path: &Path, kind: Kind) -> Result<Option<SeqSource>, String> {
        let r = std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
        let info = match kind {
            Kind::WebP => webp_info(r),
            _ => apng_info(r),
        };
        Ok(info.map(|(size, delays)| SeqSource { path: path.to_path_buf(), kind, size, delays, frames: None, next: 0 }))
    }

    fn start(&self) -> Result<image::Frames<'static>, String> {
        use image::AnimationDecoder;
        let r = std::io::BufReader::new(std::fs::File::open(&self.path).map_err(|e| e.to_string())?);
        let e = |e: image::ImageError| e.to_string();
        Ok(match self.kind {
            Kind::WebP => image::codecs::webp::WebPDecoder::new(r).map_err(e)?.into_frames(),
            _ => image::codecs::png::PngDecoder::new(r).map_err(e)?.apng().map_err(e)?.into_frames(),
        })
    }
}

impl Source for SeqSource {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn delays(&self) -> &[u32] {
        &self.delays
    }

    fn frame(&mut self, i: usize) -> Result<RgbaImage, String> {
        if self.frames.is_none() || i < self.next {
            self.frames = Some(self.start()?);
            self.next = 0;
        }
        let frames = self.frames.as_mut().expect("started above");
        while self.next <= i {
            let f = frames.next().ok_or_else(|| format!("no frame {i}"))?.map_err(|e| e.to_string());
            self.next += 1;
            if self.next > i {
                return f.map(image::Frame::into_buffer);
            }
        }
        Err(format!("no frame {i}"))
    }
}

/// A file with no animation: one frame, never decoded (the image viewer
/// shows it as a still).
struct StillSource;

impl Source for StillSource {
    fn size(&self) -> (u32, u32) {
        (0, 0)
    }

    fn delays(&self) -> &[u32] {
        &[0]
    }

    fn frame(&mut self, _: usize) -> Result<RgbaImage, String> {
        Err("a still".into())
    }
}

fn open_source(path: &Path, kind: Kind, budget: usize) -> Result<Box<dyn Source>, String> {
    match kind {
        Kind::Gif => Ok(Box::new(GifSource::open(path, budget)?)),
        Kind::WebP | Kind::Png => match SeqSource::open(path, kind)? {
            Some(s) => Ok(Box::new(s)),
            None => Ok(Box::new(StillSource)),
        },
    }
}

// ---------------------------------------------------------------------------
// The worker thread

enum Req {
    /// Decode these frames, in order, replacing the previous list.
    Want(Vec<usize>),
    /// A small picture of a frame, for the scrub bar.
    Thumb(usize),
    /// A frame at the image's own size.
    Export(usize),
}

enum Msg {
    Info { size: (u32, u32), delays: Vec<u32> },
    /// One frame or none: not an animation.
    Still,
    Failed(String),
    Frame(usize, egui::ColorImage),
    Thumb(usize, egui::ColorImage),
    Export(usize, RgbaImage),
}

/// Long side of scrub-bar thumbnails, in pixels.
pub const THUMB_SIDE: u32 = 160;

fn fit(img: RgbaImage, max: Option<u32>) -> RgbaImage {
    match max {
        Some(m) if img.width().max(img.height()) > m => {
            let s = f64::from(m) / f64::from(img.width().max(img.height()));
            let w = ((f64::from(img.width()) * s).round() as u32).max(1);
            let h = ((f64::from(img.height()) * s).round() as u32).max(1);
            image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
        }
        _ => img,
    }
}

fn color_image(img: &RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

fn worker(path: PathBuf, kind: Kind, opts: Opts, ctx: egui::Context, rx: Receiver<Req>, tx: Sender<Msg>) {
    let send = |m: Msg| {
        let ok = tx.send(m).is_ok();
        ctx.request_repaint();
        ok
    };
    let mut src = match open_source(&path, kind, opts.checkpoint_budget) {
        Ok(s) => s,
        Err(e) => {
            send(Msg::Failed(e));
            return;
        }
    };
    if src.delays().len() < 2 {
        send(Msg::Still);
        return;
    }
    if !send(Msg::Info { size: src.size(), delays: src.delays().to_vec() }) {
        return;
    }
    let mut want: std::collections::VecDeque<usize> = Default::default();
    loop {
        // Everything asked for so far: the latest list and thumbnail win.
        let mut reqs = Vec::new();
        if want.is_empty() {
            match rx.recv() {
                Ok(r) => reqs.push(r),
                Err(_) => return,
            }
        }
        loop {
            match rx.try_recv() {
                Ok(r) => reqs.push(r),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let mut thumb = None;
        for r in reqs {
            match r {
                Req::Want(v) => want = v.into(),
                Req::Thumb(i) => thumb = Some(i),
                Req::Export(i) => {
                    if let Ok(img) = src.frame(i)
                        && !send(Msg::Export(i, img))
                    {
                        return;
                    }
                }
            }
        }
        let msg = match thumb {
            Some(i) => src.frame(i).ok().map(|img| Msg::Thumb(i, color_image(&fit(img, Some(THUMB_SIDE))))),
            None => want.pop_front().and_then(|i| Some(Msg::Frame(i, color_image(&fit(src.frame(i).ok()?, opts.max_side))))),
        };
        if let Some(m) = msg
            && !send(m)
        {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// The player

/// How a [`Player`] decodes.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// Frames are scaled down to this long side, in pixels.
    pub max_side: Option<u32>,
    /// Bytes of decoded frames kept as textures.
    pub frame_budget: usize,
    /// Bytes of canvas snapshots the decoder keeps for seeking.
    pub checkpoint_budget: usize,
}

impl Opts {
    /// For the viewer: frames at up to 4096 px, 128 MB of frames.
    pub fn viewer() -> Opts {
        Opts { max_side: Some(4096), frame_budget: 128 << 20, checkpoint_budget: 48 << 20 }
    }
}

/// Where a player is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Reading the file.
    Opening,
    /// An animation, ready to play.
    Ready,
    /// One frame: show the image as a still.
    Still,
    Failed(String),
}

/// An animated image playing on a background decoder. Call
/// [`Player::update`] once per repaint, then draw [`Player::texture`].
pub struct Player {
    ctx: egui::Context,
    tx: Sender<Req>,
    rx: Receiver<Msg>,
    name: String,
    pub state: State,
    delays: Vec<u32>,
    cap: usize,
    budget: usize,
    max_side: Option<u32>,
    frames: HashMap<usize, egui::TextureHandle>,
    thumbs: HashMap<usize, egui::TextureHandle>,
    thumb_asked: Option<usize>,
    requested: Vec<usize>,
    export: Option<(usize, RgbaImage)>,
    /// The frame on screen and how far into it playback is.
    frame: usize,
    offset: f64,
    shown: Option<egui::TextureHandle>,
    pub playing: bool,
    pub speed: f32,
    pub range: Option<LoopRange>,
    last: Option<f64>,
}

/// Thumbnails kept for the scrub bar.
const THUMB_CAP: usize = 48;

impl Player {
    /// Starts decoding `path` on a new thread.
    pub fn open(ctx: &egui::Context, path: PathBuf, kind: Kind, opts: Opts) -> Player {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let (tx, wrx) = mpsc::channel();
        let (wtx, rx) = mpsc::channel();
        let c = ctx.clone();
        let _ = std::thread::Builder::new().name("anim".into()).spawn(move || worker(path, kind, opts, c, wrx, wtx));
        Player {
            ctx: ctx.clone(),
            tx,
            rx,
            name: format!("anim-{}", SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
            state: State::Opening,
            delays: Vec::new(),
            cap: 2,
            budget: opts.frame_budget,
            max_side: opts.max_side,
            frames: HashMap::new(),
            thumbs: HashMap::new(),
            thumb_asked: None,
            requested: Vec::new(),
            export: None,
            frame: 0,
            offset: 0.0,
            shown: None,
            playing: true,
            speed: 1.0,
            range: None,
            last: None,
        }
    }

    /// Number of frames (0 until known).
    pub fn count(&self) -> usize {
        self.delays.len()
    }

    /// The frame on screen (0-based).
    pub fn frame(&self) -> usize {
        self.frame
    }

    /// Frames decoded and held now.
    #[cfg(test)]
    pub fn held(&self) -> usize {
        self.frames.len()
    }

    /// The most frames ever held at once.
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// The picture to draw: the current frame, or the last one shown while
    /// it decodes.
    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        self.frames.get(&self.frame).or(self.shown.as_ref())
    }

    /// Takes in decoded frames, moves playback on to `now` (seconds, egui's
    /// clock) and asks for the frames needed next.
    pub fn update(&mut self, now: f64) {
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Info { size, delays }) => {
                    let (w, h) = size;
                    let side = self.max_side_hint(w, h);
                    let bytes = (side.0 as usize * side.1 as usize * 4).max(1);
                    self.cap = (self.budget / bytes).clamp(3, 2048);
                    self.delays = delays;
                    self.state = State::Ready;
                }
                Ok(Msg::Still) => self.state = State::Still,
                Ok(Msg::Failed(e)) => self.state = State::Failed(e),
                Ok(Msg::Frame(i, img)) => {
                    let tex = self.ctx.load_texture(format!("{}-{i}", self.name), img, egui::TextureOptions::LINEAR);
                    self.frames.insert(i, tex);
                    self.evict();
                }
                Ok(Msg::Thumb(i, img)) => {
                    let tex = self.ctx.load_texture(format!("{}-t{i}", self.name), img, egui::TextureOptions::LINEAR);
                    if self.thumbs.len() >= THUMB_CAP
                        && let Some(&far) = self.thumbs.keys().max_by_key(|&&k| k.abs_diff(i))
                    {
                        self.thumbs.remove(&far);
                    }
                    self.thumbs.insert(i, tex);
                }
                Ok(Msg::Export(i, img)) => self.export = Some((i, img)),
                Err(_) => break,
            }
        }
        let dt = self.last.map_or(0.0, |l| (now - l).max(0.0) * 1000.0);
        self.last = Some(now);
        if self.state != State::Ready {
            return;
        }
        if self.playing && self.frames.contains_key(&self.frame) {
            let frames = &self.frames;
            let (f, o) = advance(&self.delays, self.range, self.frame, self.offset, dt * f64::from(self.speed), |i| frames.contains_key(&i));
            self.frame = f;
            self.offset = o;
        }
        if let Some(t) = self.frames.get(&self.frame) {
            self.shown = Some(t.clone());
        }
        self.request();
        if self.playing {
            let d = f64::from(frame_delay_ms(self.delays[self.frame]));
            let wait = ((d - self.offset) / f64::from(self.speed)).max(4.0);
            self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait / 1000.0));
        }
    }

    /// The decoded frame size for an image of `w`×`h`.
    fn max_side_hint(&self, w: u32, h: u32) -> (u32, u32) {
        match self.max_side {
            Some(m) if w.max(h) > m => {
                let s = f64::from(m) / f64::from(w.max(h));
                ((f64::from(w) * s) as u32, (f64::from(h) * s) as u32)
            }
            _ => (w, h),
        }
    }

    /// The frames wanted next, in order: the current one, then those
    /// playback reaches soon (or the neighbours, when paused).
    fn wanted(&self) -> Vec<usize> {
        let total = self.count();
        let ahead = (self.cap / 2).clamp(1, 24);
        let mut out = vec![self.frame];
        if self.playing {
            let mut f = self.frame;
            for _ in 0..ahead {
                f = next_frame(f, total, self.range);
                if out.contains(&f) {
                    break;
                }
                out.push(f);
            }
        } else {
            for n in [1, -1] {
                let f = step_frame(self.frame, n, total, self.range);
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        out
    }

    fn request(&mut self) {
        let missing: Vec<usize> = self.wanted().into_iter().filter(|i| !self.frames.contains_key(i)).collect();
        // Ask again only for something new; the worker is still on the rest.
        if missing.iter().any(|i| !self.requested.contains(i)) {
            let _ = self.tx.send(Req::Want(missing.clone()));
            self.requested = missing;
        }
    }

    /// Drops the frames playback reaches last until the cache fits.
    fn evict(&mut self) {
        let total = self.count().max(1);
        while self.frames.len() > self.cap {
            let cur = self.frame;
            let far = self.frames.keys().copied().filter(|&k| k != cur).max_by_key(|&k| (k + total - cur) % total);
            match far {
                Some(k) => {
                    self.frames.remove(&k);
                }
                None => break,
            }
        }
    }

    /// Shows frame `i` (clamped), pausing nothing.
    pub fn seek(&mut self, i: usize) {
        self.frame = i.min(self.count().saturating_sub(1));
        self.offset = 0.0;
    }

    /// Pauses and moves `n` frames, inside the loop range.
    pub fn step(&mut self, n: i64) {
        self.playing = false;
        let f = step_frame(self.frame, n, self.count(), self.range);
        self.seek(f);
    }

    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }

    /// Sets the loop range (`None` plays every frame) and keeps playback
    /// inside it.
    pub fn set_range(&mut self, range: Option<LoopRange>) {
        self.range = range;
        let f = into_range(self.frame, self.count(), range);
        if f != self.frame {
            self.seek(f);
        }
    }

    /// A small picture of frame `i` for the scrub bar, asked for if missing.
    pub fn thumb(&mut self, i: usize) -> Option<egui::TextureHandle> {
        if let Some(t) = self.thumbs.get(&i) {
            return Some(t.clone());
        }
        if self.thumb_asked != Some(i) {
            self.thumb_asked = Some(i);
            let _ = self.tx.send(Req::Thumb(i));
        }
        // The nearest one held stands in meanwhile.
        self.thumbs.iter().min_by_key(|(k, _)| k.abs_diff(i)).map(|(_, t)| t.clone())
    }

    /// Asks for frame `i` at full size; [`Player::take_export`] returns it.
    pub fn request_export(&mut self, i: usize) {
        let _ = self.tx.send(Req::Export(i));
    }

    pub fn take_export(&mut self) -> Option<(usize, RgbaImage)> {
        self.export.take()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes a `w`×`h` GIF of `n` frames to `path`: each frame a small
    /// moving square over the first frame's full background, with the given
    /// disposal, each `delay_cs` centiseconds long.
    pub(crate) fn write_gif(path: &Path, w: u16, h: u16, n: usize, delay_cs: u16) {
        let file = std::fs::File::create(path).unwrap();
        let palette = [0u8, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        let mut enc = gif::Encoder::new(file, w, h, &palette).unwrap();
        enc.set_repeat(gif::Repeat::Infinite).unwrap();
        for k in 0..n {
            let mut f = if k == 0 {
                gif::Frame { width: w, height: h, buffer: vec![2u8; w as usize * h as usize].into(), ..Default::default() }
            } else {
                let s = 4u16.min(w).min(h);
                let left = (k as u16 * 3) % (w - s + 1);
                let top = (k as u16 * 5) % (h - s + 1);
                let dispose = match k % 3 {
                    0 => gif::DisposalMethod::Keep,
                    1 => gif::DisposalMethod::Background,
                    _ => gif::DisposalMethod::Previous,
                };
                // Pixel 3 (out of the palette) is the transparent index.
                let mut buf = vec![(k % 2) as u8; s as usize * s as usize];
                buf[0] = 3;
                buf[5] = 1;
                // Some frames bring their own palette and are interlaced.
                let local = (k % 4 == 3).then(|| vec![9u8, 9, 9, 200, 100, 50, 1, 2, 3, 4, 5, 6]);
                let interlaced = k % 4 == 3;
                gif::Frame { left, top, width: s, height: s, dispose, transparent: Some(3), palette: local, interlaced, buffer: buf.into(), ..Default::default() }
            };
            f.delay = delay_cs;
            if k == 0 {
                f.dispose = gif::DisposalMethod::Keep;
            }
            enc.write_frame(&f).unwrap();
        }
    }

    #[test]
    fn delays_follow_browsers() {
        assert_eq!(frame_delay_ms(0), 100);
        assert_eq!(frame_delay_ms(10), 100);
        assert_eq!(frame_delay_ms(20), 20);
    }

    #[test]
    fn play_order_and_ranges() {
        assert_eq!(next_frame(0, 3, None), 1);
        assert_eq!(next_frame(2, 3, None), 0);
        let r = Some(LoopRange { first: 2, last: 4 });
        assert_eq!(next_frame(4, 10, r), 2);
        assert_eq!(next_frame(7, 10, r), 2);
        assert_eq!(next_frame(3, 10, r), 4);
        assert_eq!(into_range(7, 10, r), 2);
        assert_eq!(into_range(3, 10, r), 3);
        assert_eq!(step_frame(4, 1, 10, r), 4, "steps stop at the range's end");
        assert_eq!(step_frame(2, -10, 10, r), 2);
        assert_eq!(step_frame(5, 10, 10, None), 9);
        assert_eq!(step_frame(0, -1, 10, None), 0);
    }

    #[test]
    fn advancing() {
        let d = [100, 200, 50];
        let all = |_| true;
        assert_eq!(advance(&d, None, 0, 0.0, 50.0, all), (0, 50.0));
        assert_eq!(advance(&d, None, 0, 50.0, 60.0, all), (1, 10.0));
        assert_eq!(advance(&d, None, 2, 40.0, 20.0, all), (0, 10.0), "wraps to the first frame");
        // Waits on the last decoded frame.
        assert_eq!(advance(&d, None, 0, 90.0, 500.0, |i| i == 0), (0, 100.0));
        // A loop range keeps playback in 1..=2.
        let r = Some(LoopRange { first: 1, last: 2 });
        assert_eq!(advance(&d, r, 2, 0.0, 60.0, all), (1, 10.0));
        // A very long gap skips whole loops rather than spinning.
        let (f, o) = advance(&d, None, 0, 0.0, 350.0 * 1000.0 + 120.0, all);
        assert_eq!((f, o.round()), (1, 20.0));
        // Zero delays play at 100 ms.
        assert_eq!(advance(&[0, 0], None, 0, 0.0, 150.0, all), (1, 50.0));
    }

    #[test]
    fn range_handles() {
        assert_eq!(range_with_first(None, 3, 10), LoopRange { first: 3, last: 9 });
        assert_eq!(range_with_last(None, 5), LoopRange { first: 0, last: 5 });
        let r = Some(LoopRange { first: 3, last: 6 });
        assert_eq!(range_with_first(r, 8, 10), LoopRange { first: 6, last: 8 }, "crossed ends swap");
        assert_eq!(range_with_last(r, 1), LoopRange { first: 1, last: 3 });
    }

    #[test]
    fn scrub_mapping() {
        assert_eq!(frame_at_x(0.0, 0.0, 100.0, 10), 0);
        assert_eq!(frame_at_x(100.0, 0.0, 100.0, 10), 9);
        assert_eq!(frame_at_x(-5.0, 0.0, 100.0, 10), 0);
        assert_eq!(frame_at_x(54.0, 0.0, 100.0, 10), 5);
        assert_eq!(frame_x(0, 10, 0.0, 90.0), 0.0);
        assert_eq!(frame_x(9, 10, 0.0, 90.0), 90.0);
        assert_eq!(frame_x(3, 1, 7.0, 90.0), 7.0);
    }

    #[test]
    fn labels_and_speeds() {
        assert_eq!(counter_label(11, 1500), ("0012".to_string(), "/ 1500".to_string()));
        assert_eq!(counter_label(0, 9), ("1".to_string(), "/ 9".to_string()));
        assert_eq!(speed_label(1.0), "1x");
        assert_eq!(speed_label(0.25), "0.25x");
        assert_eq!(speed_label(1.75), "1.75x");
        assert_eq!(step_speed(1.0, true), 1.25);
        assert_eq!(step_speed(8.0, true), 8.0);
        assert_eq!(step_speed(0.25, false), 0.25);
        assert_eq!(step_speed(3.0, false), 2.0);
    }

    #[test]
    fn gif_frames_match_the_image_crate() {
        use image::AnimationDecoder;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.gif");
        write_gif(&path, 20, 12, 30, 4);
        let reference: Vec<RgbaImage> = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()))
            .unwrap()
            .into_frames()
            .map(|f| f.unwrap().into_buffer())
            .collect();
        // A tiny checkpoint budget forces snapshots every 8 frames.
        let src = GifSource::open(&path, 1).unwrap();
        assert_eq!(src.size(), (20, 12));
        assert_eq!(src.delays(), &[40; 30][..]);
        assert_eq!(src.every, 32);
        let mut src = GifSource::open(&path, 20 * 12 * 4 * 4).unwrap();
        assert_eq!(src.every, 8);
        // In order, backwards and jumping about.
        let order: Vec<usize> = (0..30).chain((0..30).rev()).chain([3, 27, 9, 0, 17, 16, 29]).collect();
        for i in order {
            assert!(src.frame(i).unwrap() == reference[i], "frame {i} differs");
        }
    }

    #[test]
    fn damaged_gifs_keep_their_first_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cut.gif");
        write_gif(&path, 20, 12, 30, 4);
        let len = std::fs::metadata(&path).unwrap().len();
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(len * 2 / 3).unwrap();
        let mut src = GifSource::open(&path, 1 << 20).unwrap();
        let n = src.delays().len();
        assert!(n > 5 && n < 30, "{n} frames");
        for i in 0..n {
            src.frame(i).unwrap();
        }
        std::fs::write(&path, b"GIF89a").unwrap();
        assert!(GifSource::open(&path, 1 << 20).is_err());
    }

    #[test]
    fn long_gifs_stay_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.gif");
        write_gif(&path, 64, 48, 5000, 2);
        let canvas = 64 * 48 * 4;
        let budget = 200 * canvas;
        let mut src = GifSource::open(&path, budget).unwrap();
        assert_eq!(src.delays().len(), 5000);
        for i in (0..5000).step_by(7).chain([4999, 0, 2500]) {
            src.frame(i).unwrap();
        }
        assert!(src.checkpoint_bytes() <= budget + canvas, "checkpoints use {} bytes", src.checkpoint_bytes());
        // Only an index of the frames is held, not their data.
        assert_eq!(src.index.frames.len(), 5000);
    }

    #[test]
    fn player_holds_a_bounded_window() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.gif");
        write_gif(&path, 64, 48, 5000, 2);
        let ctx = egui::Context::default();
        let opts = Opts { max_side: None, frame_budget: 40 * 64 * 48 * 4, checkpoint_budget: 1 << 20 };
        let mut p = Player::open(&ctx, path, Kind::Gif, opts);
        let mut now = 0.0;
        let start = std::time::Instant::now();
        let mut most = 0;
        while start.elapsed().as_secs() < 20 && p.frame() < 600 {
            p.update(now);
            most = most.max(p.held());
            now += 0.05;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(p.state, State::Ready);
        assert_eq!(p.count(), 5000);
        assert_eq!(p.capacity(), 40);
        assert!(p.frame() >= 600, "playback reached frame {}", p.frame());
        assert!(most <= 40, "held {most} frames");
        // Seeking far back still works.
        p.seek(10);
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < 10 && !p.frames.contains_key(&10) {
            p.playing = false;
            p.update(now);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(p.frames.contains_key(&10));
    }

    /// `n` frames of a `w`×`h` picture whose colour changes each frame.
    fn pictures(w: u32, h: u32, n: usize) -> Vec<Vec<u8>> {
        (0..n).map(|k| (0..w * h).flat_map(|p| [(k * 40) as u8, (p % 256) as u8, 200, 255]).collect()).collect()
    }

    fn write_webp(path: &Path, w: u32, h: u32, delays: &[i32]) {
        let config = webp::WebPConfig::new().unwrap();
        let pics = pictures(w, h, delays.len());
        let mut enc = webp::AnimEncoder::new(w, h, &config);
        let mut t = 0;
        for (pic, d) in pics.iter().zip(delays) {
            enc.add_frame(webp::AnimFrame::from_rgba(pic, w, h, t));
            t += d;
        }
        std::fs::write(path, &*enc.encode()).unwrap();
    }

    fn write_apng(path: &Path, w: u32, h: u32, delays: &[(u16, u16)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_animated(delays.len() as u32, 0).unwrap();
        let mut writer = enc.write_header().unwrap();
        for (pic, &(num, den)) in pictures(w, h, delays.len()).iter().zip(delays) {
            writer.set_frame_delay(num, den).unwrap();
            writer.write_image_data(pic).unwrap();
        }
        writer.finish().unwrap();
    }

    fn open_seq(path: &Path, kind: Kind) -> SeqSource {
        SeqSource::open(path, kind).unwrap().expect("animated")
    }

    #[test]
    fn animated_webp_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.webp");
        write_webp(&path, 16, 10, &[40, 90, 250]);
        let mut src = open_seq(&path, Kind::WebP);
        assert_eq!(src.size(), (16, 10));
        // libwebp gives the last frame its encoder's default length.
        assert_eq!(&src.delays()[..2], &[40, 90]);
        assert_eq!(src.delays().len(), 3);
        let f2 = src.frame(2).unwrap();
        let f0 = src.frame(0).unwrap();
        assert_eq!(f2.dimensions(), (16, 10));
        assert!(f0 != f2);
        assert!(src.frame(0).unwrap() == f0);
        assert!(src.frame(3).is_err());
        // A still WebP has no animation.
        let still = dir.path().join("s.webp");
        let pic = &pictures(8, 8, 1)[0];
        std::fs::write(&still, &*webp::Encoder::from_rgba(pic, 8, 8).encode(80.0)).unwrap();
        assert!(SeqSource::open(&still, Kind::WebP).unwrap().is_none());
    }

    #[test]
    fn apng_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        write_apng(&path, 12, 9, &[(1, 10), (25, 1000), (3, 0)]);
        let mut src = open_seq(&path, Kind::Png);
        assert_eq!(src.size(), (12, 9));
        assert_eq!(src.delays(), &[100, 25, 30], "a zero denominator means hundredths");
        let f1 = src.frame(1).unwrap();
        assert_eq!(f1.dimensions(), (12, 9));
        assert!(src.frame(0).unwrap() != f1);
        assert!(src.frame(2).is_ok());
        // A still PNG has none.
        let still = dir.path().join("s.png");
        image::RgbaImage::new(4, 4).save(&still).unwrap();
        assert!(SeqSource::open(&still, Kind::Png).unwrap().is_none());
        let ctx = egui::Context::default();
        let mut p = Player::open(&ctx, still, Kind::Png, Opts::viewer());
        let start = std::time::Instant::now();
        while p.state == State::Opening && start.elapsed().as_secs() < 5 {
            p.update(0.0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(p.state, State::Still);
    }

    #[test]
    fn webp_and_apng_play() {
        let dir = tempfile::tempdir().unwrap();
        let webp = dir.path().join("a.webp");
        write_webp(&webp, 16, 10, &[40, 40, 40, 40]);
        let apng = dir.path().join("a.png");
        write_apng(&apng, 16, 10, &[(4, 100); 4]);
        for (path, kind) in [(webp, Kind::WebP), (apng, Kind::Png)] {
            let ctx = egui::Context::default();
            let mut p = Player::open(&ctx, path, kind, Opts::viewer());
            let (mut now, start) = (0.0, std::time::Instant::now());
            let mut seen = std::collections::HashSet::new();
            while seen.len() < 4 && start.elapsed().as_secs() < 10 {
                p.update(now);
                if p.texture().is_some() {
                    seen.insert(p.frame());
                }
                now += 0.02;
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert_eq!(p.state, State::Ready, "{kind:?}");
            assert_eq!(seen.len(), 4, "{kind:?} played {seen:?}");
        }
    }

    #[test]
    fn thumbnails_and_exports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.gif");
        write_gif(&path, 400, 200, 20, 5);
        let ctx = egui::Context::default();
        let mut p = Player::open(&ctx, path, Kind::Gif, Opts::viewer());
        p.playing = false;
        let start = std::time::Instant::now();
        let (mut thumb, mut export, mut asked) = (None, None, false);
        while (thumb.is_none() || export.is_none()) && start.elapsed().as_secs() < 10 {
            p.update(0.0);
            if p.state == State::Ready {
                if !asked {
                    p.request_export(3);
                    asked = true;
                }
                thumb = thumb.or_else(|| p.thumb(7).filter(|_| p.thumbs.contains_key(&7)).map(|t| t.size()));
                export = export.or_else(|| p.take_export());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(thumb, Some([THUMB_SIDE as usize, THUMB_SIDE as usize / 2]));
        let (i, img) = export.unwrap();
        assert_eq!((i, img.dimensions()), (3, (400, 200)));
    }

    #[test]
    fn one_frame_is_a_still() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("one.gif");
        write_gif(&path, 8, 8, 1, 0);
        let ctx = egui::Context::default();
        let mut p = Player::open(&ctx, path, Kind::Gif, Opts::viewer());
        let start = std::time::Instant::now();
        while p.state == State::Opening && start.elapsed().as_secs() < 5 {
            p.update(0.0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(p.state, State::Still);
    }
}
