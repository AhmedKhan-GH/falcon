//! Background image decoding and an LRU texture cache.

use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui::{Color32, ColorImage, Context, Pos2, Rect, TextureHandle, TextureOptions};
use falcon_library::model::Item;
use falcon_library::thumbnail;
use lru::LruCache;

/// How large a version of an image is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Grid,
    Preview,
    Full,
    /// The original at its full resolution, cut into tiles; only for the
    /// item in the viewer (see [`Loader::original`]).
    Original,
}

impl Tier {
    fn max_side(self) -> u32 {
        match self {
            Tier::Grid => 512,
            Tier::Preview => 1024,
            Tier::Full => crate::logic::FULL_TIER_SIDE,
            Tier::Original => u32::MAX,
        }
    }

    fn capacity(self) -> usize {
        match self {
            Tier::Grid => 900,
            Tier::Preview => 24,
            Tier::Full => 4,
            Tier::Original => 1,
        }
    }
}

pub type Key = (String, Tier);

#[derive(Clone)]
pub struct Job {
    pub key: Key,
    path: PathBuf,
    svg: bool,
    /// Render with QuickLook (formats falcon can't decode, at full size).
    quicklook: bool,
    /// For [`Tier::Original`]: the side of the tiles to cut it into.
    tile: usize,
    /// For an SVG zoomed in: the visible part to render sharp.
    region: Option<Region>,
    /// A RAW photo: show its embedded preview (QuickLook if it has none).
    raw: bool,
}

/// The part of an SVG on screen: fractions of the drawing
/// (`[x0, y0, x1, y1]`) and the pixel size to render it at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Region {
    pub rect: [f32; 4],
    pub size: [u32; 2],
    seq: u64,
}

/// A sharp render of the visible part of the viewed SVG.
pub struct SvgPatch {
    pub id: String,
    /// Where it sits, as a fraction of the drawing.
    pub rect: Rect,
    pub texture: TextureHandle,
    seq: u64,
}

/// The job decoding `item`'s original at full resolution, in tiles of at
/// most `tile` pixels (see `logic::tile_side`).
pub fn original_job(item: &Item, root: &std::path::Path, tile: usize) -> Option<Job> {
    let ext = item.ext.to_lowercase();
    crate::logic::full_resolution(&ext)
        .then(|| Job { key: (item.id.clone(), Tier::Original), path: item.original_path(root), svg: false, quicklook: false, tile, region: None, raw: falcon_library::exif::is_raw(&ext) })
}

/// Picks the file to decode for `item`: the thumbnail when it exists (unless
/// the full-size original is wanted and decodable), else a supported original.
pub fn job_for(item: &Item, root: &std::path::Path, tier: Tier) -> Option<Job> {
    let ext = item.ext.to_lowercase();
    let supported = thumbnail::is_supported(&ext);
    let original = supported.then(|| item.original_path(root));
    if tier == Tier::Full && falcon_library::exif::is_raw(&ext) {
        let quicklook = cfg!(target_os = "macos");
        return Some(Job { key: (item.id.clone(), tier), path: item.original_path(root), svg: false, quicklook, tile: 0, region: None, raw: true });
    }
    if tier == Tier::Full && !supported && cfg!(target_os = "macos") {
        return Some(Job { key: (item.id.clone(), tier), path: item.original_path(root), svg: false, quicklook: true, tile: 0, region: None, raw: false });
    }
    let (path, svg) = match (tier, item.thumbnail_path(root), original) {
        (Tier::Full, _, Some(o)) => (o, ext == "svg"),
        (_, Some(t), _) => (t, false),
        (_, None, Some(o)) => (o, ext == "svg"),
        (_, None, None) => return None,
    };
    Some(Job { key: (item.id.clone(), tier), path, svg, quicklook: false, tile: 0, region: None, raw: false })
}

struct Shared {
    queue: Mutex<VecDeque<Job>>,
    cv: Condvar,
    /// Set when the loader is dropped; the workers then stop.
    closed: std::sync::atomic::AtomicBool,
    /// Grayscale preview: the viewer's tiers decode desaturated.
    gray: std::sync::atomic::AtomicBool,
}

/// What a worker hands back: one image, or a full-resolution one in tiles.
enum Pixels {
    Whole(ColorImage),
    Tiled { size: [usize; 2], tiles: Vec<(crate::logic::TileSpan, ColorImage)> },
    /// A render of part of an SVG.
    Patch(Region, ColorImage),
}

struct Decoded {
    key: Key,
    image: Option<Pixels>,
    /// From an SVG region job, which shares its key with the full tier.
    region: bool,
    /// Decoded desaturated (grayscale preview).
    gray: bool,
}

/// Whether a tier follows the grayscale preview: the viewer's sizes, not
/// the grid's cards.
fn grays(tier: Tier) -> bool {
    tier != Tier::Grid
}

/// Desaturates decoded pixels in place.
fn desaturate(p: &mut Pixels) {
    match p {
        Pixels::Whole(img) | Pixels::Patch(_, img) => crate::logic::grayscale_rgba(img.as_raw_mut()),
        Pixels::Tiled { tiles, .. } => {
            for (_, img) in tiles {
                crate::logic::grayscale_rgba(img.as_raw_mut());
            }
        }
    }
}

/// One texture of a full-resolution image.
pub struct Tile {
    pub texture: TextureHandle,
    /// Where it sits, as a fraction of the image (0..1 on both axes).
    pub rect: Rect,
    /// The part of the texture to draw (the tile without its padding).
    pub uv: Rect,
}

/// The viewed item's original at full resolution.
pub struct Original {
    pub id: String,
    /// Decoded size in pixels.
    pub size: [usize; 2],
    pub tiles: Vec<Tile>,
}

/// An original still being uploaded, a few tiles per frame.
struct Staged {
    original: Original,
    rest: Vec<(crate::logic::TileSpan, ColorImage)>,
}

/// Sharp pixels when enlarged, smooth when shrunk (Eagle's "pixelated"
/// rendering; egui has no mipmaps, so shrinking stays linear).
const PIXELATED: TextureOptions = TextureOptions { magnification: eframe::egui::TextureFilter::Nearest, ..TextureOptions::LINEAR };

/// Bytes of tile pixels uploaded per frame, so a 100-megapixel original
/// doesn't stall one frame with a 400 MB upload.
const UPLOAD_BUDGET: usize = 48 << 20;

/// Decodes on worker threads; textures are uploaded from the UI thread.
pub struct Loader {
    shared: Arc<Shared>,
    rx: std::sync::mpsc::Receiver<Decoded>,
    caches: HashMap<Tier, LruCache<String, TextureHandle>>,
    pending: HashSet<Key>,
    failed: HashSet<Key>,
    /// The viewed item's full-resolution original, at most one at a time.
    original: Option<Original>,
    staged: Option<Staged>,
    /// Draw the viewer's renders with sharp pixels when enlarged.
    pixelated: bool,
    /// The viewed SVG's sharp render of what is on screen, the region last
    /// asked for (with its item), and the count of regions asked for.
    svg_patch: Option<SvgPatch>,
    svg_last: Option<(String, [f32; 4], [u32; 2])>,
    svg_seq: u64,
    /// Grayscale preview, and the cached renders made before it last
    /// changed: still shown until their replacement arrives, so switching
    /// doesn't blank the view.
    gray: bool,
    stale: HashSet<Key>,
}

impl Loader {
    pub fn new(ctx: &Context) -> Self {
        let shared = Arc::new(Shared { queue: Mutex::new(VecDeque::new()), cv: Condvar::new(), closed: Default::default(), gray: Default::default() });
        let (tx, rx) = std::sync::mpsc::channel();
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 6));
        for _ in 0..workers {
            let (shared, tx, ctx) = (shared.clone(), tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let mut q = shared.queue.lock().unwrap();
                        loop {
                            if shared.closed.load(std::sync::atomic::Ordering::Relaxed) {
                                return;
                            }
                            if let Some(j) = q.pop_front() {
                                break j;
                            }
                            q = shared.cv.wait(q).unwrap();
                        }
                    };
                    let gray = grays(job.key.1) && shared.gray.load(std::sync::atomic::Ordering::Relaxed);
                    let mut image = decode(&job);
                    if gray && let Some(p) = &mut image {
                        desaturate(p);
                    }
                    if tx.send(Decoded { key: job.key, image, region: job.region.is_some(), gray }).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        let caches = [Tier::Grid, Tier::Preview, Tier::Full]
            .into_iter()
            .map(|t| (t, LruCache::new(NonZeroUsize::new(t.capacity()).unwrap())))
            .collect();
        Loader {
            shared,
            rx,
            caches,
            pending: HashSet::new(),
            failed: HashSet::new(),
            original: None,
            staged: None,
            pixelated: false,
            svg_patch: None,
            svg_last: None,
            svg_seq: 0,
            gray: false,
            stale: HashSet::new(),
        }
    }

    /// Uploads finished decodes (bounded per call so frames stay short).
    pub fn poll(&mut self, ctx: &Context) {
        self.upload_staged(ctx);
        for _ in 0..24 {
            let Ok(d) = self.rx.try_recv() else { return };
            // Decoded before the grayscale preview changed: asked again.
            if grays(d.key.1) && d.gray != self.gray {
                if !d.region {
                    self.pending.remove(&d.key);
                }
                continue;
            }
            if d.region {
                let Some(Pixels::Patch(region, img)) = d.image else { continue };
                // Only the latest region for the item still on screen.
                let current = self.svg_last.as_ref().is_some_and(|l| l.0 == d.key.0);
                if current && self.svg_patch.as_ref().is_none_or(|p| p.seq < region.seq) {
                    let [x0, y0, x1, y1] = region.rect;
                    let texture = ctx.load_texture(format!("{}-svg-region", d.key.0), img, TextureOptions::LINEAR);
                    let rect = Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1));
                    self.svg_patch = Some(SvgPatch { id: d.key.0, rect, texture, seq: region.seq });
                }
                continue;
            }
            // A decode started before `forget` would bring back the old pixels.
            if !self.pending.remove(&d.key) {
                continue;
            }
            match d.image {
                Some(Pixels::Whole(img)) => {
                    let tex = ctx.load_texture(format!("{}-{:?}", d.key.0, d.key.1), img, self.options(d.key.1));
                    self.stale.remove(&d.key);
                    self.caches.get_mut(&d.key.1).unwrap().put(d.key.0, tex);
                }
                Some(Pixels::Tiled { size, tiles }) => {
                    self.stale.remove(&d.key);
                    self.original = None;
                    self.staged = Some(Staged { original: Original { id: d.key.0, size, tiles: Vec::new() }, rest: tiles });
                    self.upload_staged(ctx);
                }
                // Region jobs were handled above.
                Some(Pixels::Patch(..)) | None => {
                    self.failed.insert(d.key);
                }
            }
        }
        ctx.request_repaint();
    }

    /// Uploads the staged original's next tiles, within the frame's budget;
    /// once all are up it becomes [`Loader::original`].
    fn upload_staged(&mut self, ctx: &Context) {
        let Some(staged) = &mut self.staged else { return };
        let mut bytes = 0;
        while bytes < UPLOAD_BUDGET {
            let Some((span, img)) = staged.rest.pop() else { break };
            bytes += img.pixels.len() * 4;
            let [w, h] = staged.original.size.map(|v| v as f32);
            let [x0, y0, x1, y1] = span.core.map(|v| v as f32);
            let [px0, py0, px1, py1] = span.padded.map(|v| v as f32);
            let (tw, th) = (px1 - px0, py1 - py0);
            let n = staged.original.tiles.len();
            let options = if self.pixelated { PIXELATED } else { TextureOptions::LINEAR };
            let texture = ctx.load_texture(format!("{}-original-{n}", staged.original.id), img, options);
            staged.original.tiles.push(Tile {
                texture,
                rect: Rect::from_min_max(Pos2::new(x0 / w, y0 / h), Pos2::new(x1 / w, y1 / h)),
                uv: Rect::from_min_max(
                    Pos2::new((x0 - px0) / tw, (y0 - py0) / th),
                    Pos2::new((x1 - px0) / tw, (y1 - py0) / th),
                ),
            });
        }
        if staged.rest.is_empty() {
            self.original = self.staged.take().map(|s| s.original);
        } else {
            ctx.request_repaint();
        }
    }

    /// Texture sampling for `tier`: the viewer's renders (full and
    /// original) can be pixelated; thumbnails are always smooth.
    fn options(&self, tier: Tier) -> TextureOptions {
        match tier {
            Tier::Full | Tier::Original if self.pixelated => PIXELATED,
            _ => TextureOptions::LINEAR,
        }
    }

    /// Switches the viewer's renders between sharp and smooth enlarging;
    /// on a change they are dropped and decoded again. Sampling is chosen
    /// at upload, so decodes in flight come out right.
    pub fn set_pixelated(&mut self, on: bool) {
        if self.pixelated == on {
            return;
        }
        self.pixelated = on;
        self.caches.get_mut(&Tier::Full).unwrap().clear();
        self.original = None;
        self.staged = None;
    }

    /// Turns the grayscale preview on or off for the viewer's renders (the
    /// grid's cards keep their colours). What is cached is marked stale and
    /// decoded again, still shown until the new pixels arrive.
    pub fn set_grayscale(&mut self, on: bool) {
        if self.gray == on {
            return;
        }
        self.gray = on;
        self.shared.gray.store(on, std::sync::atomic::Ordering::Relaxed);
        for (tier, cache) in &self.caches {
            if grays(*tier) {
                self.stale.extend(cache.iter().map(|(id, _)| (id.clone(), *tier)));
            }
        }
        if let Some(o) = &self.original {
            self.stale.insert((o.id.clone(), Tier::Original));
        }
        self.staged = None;
        self.svg_last = None;
        self.svg_patch = None;
    }

    /// The full-resolution original of `id`, once every tile is uploaded.
    pub fn original(&self, id: &str) -> Option<&Original> {
        self.original.as_ref().filter(|o| o.id == id)
    }

    /// Asks for the part `rect` of the SVG `job` (a full-tier SVG job) to be
    /// rendered at `size`, unless that is what was last asked for. The job
    /// goes to the front of the queue; an older region still queued is
    /// dropped.
    pub fn want_svg_region(&mut self, mut job: Job, rect: [f32; 4], size: [u32; 2]) {
        let last = (job.key.0.clone(), rect, size);
        if self.svg_last.as_ref() == Some(&last) {
            return;
        }
        self.svg_last = Some(last);
        self.svg_seq += 1;
        job.region = Some(Region { rect, size, seq: self.svg_seq });
        let mut q = self.shared.queue.lock().unwrap();
        q.retain(|j| j.region.is_none());
        q.push_front(job);
        drop(q);
        self.shared.cv.notify_all();
    }

    /// The sharp render of part of the SVG `id`, if one has arrived.
    pub fn svg_patch(&self, id: &str) -> Option<&SvgPatch> {
        self.svg_patch.as_ref().filter(|p| p.id == id)
    }

    /// Drops the full-resolution original unless it belongs to `id` (the
    /// item in the viewer, if any). Called every frame, so leaving an item
    /// frees its textures at once.
    pub fn keep_original(&mut self, id: Option<&str>) {
        if self.original.as_ref().is_some_and(|o| Some(o.id.as_str()) != id) {
            self.original = None;
        }
        if self.staged.as_ref().is_some_and(|s| Some(s.original.id.as_str()) != id) {
            self.staged = None;
        }
        if self.svg_last.as_ref().is_some_and(|l| Some(l.0.as_str()) != id) {
            self.svg_last = None;
            self.svg_patch = None;
        }
    }

    /// Returns the cached texture, refreshing its LRU position.
    pub fn get(&mut self, id: &str, tier: Tier) -> Option<&TextureHandle> {
        self.caches.get_mut(&tier).unwrap().get(id)
    }

    pub fn has_failed(&self, id: &str, tier: Tier) -> bool {
        self.failed.contains(&(id.to_string(), tier))
    }

    /// Declares what the current frame needs. Queued jobs that are no longer
    /// wanted are dropped; new ones go to the front, nearest-first.
    pub fn want(&mut self, jobs: Vec<Job>) {
        let wanted: HashSet<&Key> = jobs.iter().map(|j| &j.key).collect();
        let mut q = self.shared.queue.lock().unwrap();
        let pending = &mut self.pending;
        q.retain(|j| {
            // SVG regions are managed by `want_svg_region`.
            let keep = wanted.contains(&j.key) || j.region.is_some();
            if !keep {
                pending.remove(&j.key);
            }
            keep
        });
        let mut added = false;
        for j in jobs {
            let have = match j.key.1 {
                Tier::Original => {
                    self.original.as_ref().is_some_and(|o| o.id == j.key.0) || self.staged.as_ref().is_some_and(|s| s.original.id == j.key.0)
                }
                tier => self.caches[&tier].peek(&j.key.0).is_some(),
            } && !self.stale.contains(&j.key);
            if self.failed.contains(&j.key) || self.pending.contains(&j.key) || have {
                continue;
            }
            self.pending.insert(j.key.clone());
            q.push_back(j);
            added = true;
        }
        drop(q);
        if added {
            self.shared.cv.notify_all();
        }
    }

    /// Drops one item's textures in every size, after its file or
    /// thumbnail changed, so they are decoded again.
    pub fn forget(&mut self, id: &str) {
        self.stale.retain(|k| k.0 != id);
        for (tier, c) in &mut self.caches {
            c.pop(id);
            self.failed.remove(&(id.to_string(), *tier));
            self.pending.remove(&(id.to_string(), *tier));
        }
        let original = (id.to_string(), Tier::Original);
        self.failed.remove(&original);
        self.pending.remove(&original);
        if self.original.as_ref().is_some_and(|o| o.id == id) {
            self.original = None;
        }
        if self.staged.as_ref().is_some_and(|s| s.original.id == id) {
            self.staged = None;
        }
        if self.svg_last.as_ref().is_some_and(|l| l.0 == id) {
            self.svg_last = None;
            self.svg_patch = None;
        }
    }
}

/// Closing a library drops its loader: wake the idle workers so they exit.
impl Drop for Loader {
    fn drop(&mut self) {
        // Under the queue lock, so no worker is between its check and its wait.
        let q = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        self.shared.closed.store(true, std::sync::atomic::Ordering::Relaxed);
        drop(q);
        self.shared.cv.notify_all();
    }
}

fn decode(job: &Job) -> Option<Pixels> {
    if let Some(region) = job.region {
        let data = std::fs::read(&job.path).ok()?;
        let rgba = thumbnail::render_svg_region(&data, region.rect, (region.size[0], region.size[1])).ok()?.into_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        return Some(Pixels::Patch(region, ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())));
    }
    if job.key.1 == Tier::Original {
        return decode_tiled(job);
    }
    let max = job.key.1.max_side();
    let img = if job.raw {
        match raw_preview(&job.path) {
            Some(img) => img,
            None if job.quicklook => thumbnail::quicklook(&job.path, max).ok()?,
            None => return None,
        }
    } else if job.quicklook {
        thumbnail::quicklook(&job.path, max).ok()?
    } else if job.svg {
        let data = std::fs::read(&job.path).ok()?;
        thumbnail::render_svg(&data, Some(max)).ok()?
    } else {
        thumbnail::decode(&job.path).ok()?
    };
    let img = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img };
    let rgba = img.to_rgba8();
    Some(Pixels::Whole(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())))
}

/// A RAW photo's embedded JPEG preview.
fn raw_preview(path: &std::path::Path) -> Option<image::DynamicImage> {
    thumbnail::raw_preview(&std::fs::read(path).ok()?).ok()
}

/// Decodes the original at full size and cuts it into tiles, all on the
/// worker; the UI thread only uploads them.
fn decode_tiled(job: &Job) -> Option<Pixels> {
    let img = if job.raw { raw_preview(&job.path)? } else { thumbnail::decode(&job.path).ok()? };
    let rgba = img.into_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let raw = rgba.as_raw();
    let tiles = crate::logic::tile_spans(w, h, job.tile)
        .into_iter()
        .map(|span| {
            let [x0, y0, x1, y1] = span.padded;
            let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0));
            for y in y0..y1 {
                let row = &raw[(y * w + x0) * 4..(y * w + x1) * 4];
                pixels.extend(row.as_chunks::<4>().0.iter().map(|&[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a)));
            }
            (span, ColorImage::new([x1 - x0, y1 - y0], pixels))
        })
        .collect();
    Some(Pixels::Tiled { size: [w, h], tiles })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png(dir: &std::path::Path, w: u32, h: u32) -> PathBuf {
        use image::ImageEncoder;
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x / 64 + y / 64) % 256) as u8]));
        let path = dir.join("big.png");
        let file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::NoFilter)
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
        path
    }

    fn original(path: PathBuf, tile: usize) -> Job {
        Job { key: ("x".into(), Tier::Original), path, svg: false, quicklook: false, tile, region: None, raw: false }
    }

    #[test]
    fn originals_decode_into_padded_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), 300, 130);
        let Some(Pixels::Tiled { size, tiles }) = decode(&original(path, 128)) else { panic!("not tiled") };
        assert_eq!(size, [300, 130]);
        assert_eq!(tiles.len(), 3 * 2);
        for (span, img) in &tiles {
            let [x0, y0, x1, y1] = span.padded;
            assert_eq!(img.size, [x1 - x0, y1 - y0]);
            assert!(img.size[0] <= 128 && img.size[1] <= 128);
            // The tile's first pixel is the image's pixel at its padded origin.
            assert_eq!(img.pixels[0], Color32::from_rgb((x0 % 251) as u8, (y0 % 241) as u8, ((x0 / 64 + y0 / 64) % 256) as u8));
        }
    }

    /// A 12000 x 8000 PNG decodes and tiles on the worker; the UI thread
    /// only uploads a few tiles a frame. Slow in a debug build, so run it
    /// with `cargo test --release -p falcon-app -- --ignored large_original`.
    #[test]
    #[ignore]
    fn large_original_decodes_off_the_ui_thread() {
        let dir = tempfile::tempdir().unwrap();
        let t = std::time::Instant::now();
        let path = write_png(dir.path(), 12000, 8000);
        eprintln!("wrote 12000x8000 PNG ({} MB) in {:?}", std::fs::metadata(&path).unwrap().len() >> 20, t.elapsed());
        let t = std::time::Instant::now();
        let Some(Pixels::Tiled { size, tiles }) = decode(&original(path, crate::logic::tile_side(16384))) else { panic!("not tiled") };
        eprintln!("decoded and tiled in {:?}: {} tiles", t.elapsed(), tiles.len());
        assert_eq!(size, [12000, 8000]);
        assert!(tiles.iter().all(|(_, img)| img.size[0] <= 2048 && img.size[1] <= 2048));
        let biggest = tiles.iter().map(|(_, img)| img.pixels.len() * 4).max().unwrap();
        assert!(biggest <= UPLOAD_BUDGET, "a frame uploads at least one whole tile");
        eprintln!("frames to upload at {} MB a frame: {}", UPLOAD_BUDGET >> 20, tiles.iter().map(|(_, i)| i.pixels.len() * 4).sum::<usize>().div_ceil(UPLOAD_BUDGET));
    }
}
