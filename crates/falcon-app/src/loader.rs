//! Background image decoding and an LRU texture cache.

use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::Ordering;

use eframe::egui::{Color32, ColorImage, Context, Pos2, Rect, TextureHandle, TextureOptions};
use falcon_library::model::Item;
use falcon_library::psd::Composite;
use falcon_library::thumbnail;
use lru::LruCache;

/// How large a version of an image is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Grid,
    Preview,
    Full,
    /// A grid card larger than the grid tier's render, decoded from the
    /// original when the layout panel's Thumbnail option is Quality.
    Card,
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
            Tier::Card => crate::logic::CARD_TIER_SIDE,
            Tier::Original => u32::MAX,
        }
    }

    fn capacity(self) -> usize {
        match self {
            Tier::Grid => 900,
            Tier::Preview => 24,
            Tier::Full => 4,
            // At most 16 × 1600 × 1600 × 4 bytes, about 160 MB.
            Tier::Card => 16,
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
    /// For an SVG or a QuickLook format zoomed in: the visible part to
    /// render sharp.
    region: Option<Region>,
    /// A RAW photo: show its embedded preview (QuickLook if it has none).
    raw: bool,
    /// A page of a multi-page TIFF to decode (0: the file as usual).
    page: usize,
    /// A Radiance HDR or OpenEXR image: tone mapped at the loader's exposure.
    hdr: bool,
}

/// The part of a zoomed picture on screen: fractions of the drawing
/// (`[x0, y0, x1, y1]`); for an SVG the pixel size to render it at, for a
/// QuickLook format the long side to render the whole page at (`render`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Region {
    pub rect: [f32; 4],
    pub size: [u32; 2],
    pub render: u32,
    seq: u64,
}

/// A sharp render of the visible part of the viewed SVG or QuickLook
/// format.
pub struct Patch {
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
        .then(|| Job { key: (item.id.clone(), Tier::Original), path: item.original_path(root), svg: false, quicklook: false, tile, region: None, raw: falcon_library::exif::is_raw(&ext), page: 0, hdr: falcon_library::hdr::is_hdr(&ext) })
}

/// The job asking QuickLook for the visible part of `item` at the zoom
/// shown (see [`Loader::want_region`]), for the formats Eagle draws again
/// natively when zoomed (`logic::hires_quicklook`). It shares its key with
/// the full tier, as an SVG region does.
pub fn quicklook_region_job(item: &Item, root: &std::path::Path) -> Option<Job> {
    let ext = item.ext.to_lowercase();
    (crate::logic::hires_quicklook(&ext) && (cfg!(target_os = "macos") || falcon_library::psd::handles(&ext)))
        .then(|| Job { key: (item.id.clone(), Tier::Full), path: item.original_path(root), svg: false, quicklook: true, tile: 0, region: None, raw: false, page: 0, hdr: false })
}

/// The job decoding page `page` (0-based, above 0) of a multi-page TIFF at
/// the full tier, kept under the key `<id>#page<n>`.
pub fn tiff_page_job(item: &Item, root: &std::path::Path, page: usize) -> Job {
    let key = (format!("{}#page{page}", item.id), Tier::Full);
    Job { key, path: item.original_path(root), svg: false, quicklook: false, tile: 0, region: None, raw: false, page, hdr: false }
}

/// The job decoding `item`'s original for a large grid card, for the
/// formats Eagle swaps in (`logic::quality_card_format`).
pub fn card_job(item: &Item, root: &std::path::Path) -> Option<Job> {
    let ext = item.ext.to_lowercase();
    crate::logic::quality_card_format(&ext)
        .then(|| Job { key: (item.id.clone(), Tier::Card), path: item.original_path(root), svg: false, quicklook: false, tile: 0, region: None, raw: false, page: 0, hdr: false })
}

/// Picks the file to decode for `item`: the thumbnail when it exists (unless
/// the full-size original is wanted and decodable), else a supported original.
pub fn job_for(item: &Item, root: &std::path::Path, tier: Tier) -> Option<Job> {
    let ext = item.ext.to_lowercase();
    let supported = thumbnail::is_supported(&ext);
    let original = supported.then(|| item.original_path(root));
    if tier == Tier::Full && falcon_library::exif::is_raw(&ext) {
        let quicklook = cfg!(target_os = "macos");
        return Some(Job { key: (item.id.clone(), tier), path: item.original_path(root), svg: false, quicklook, tile: 0, region: None, raw: true, page: 0, hdr: false });
    }
    if tier == Tier::Full && (ext == "tga" || falcon_library::hdr::is_hdr(&ext)) {
        // Decoded by falcon itself (HDR and EXR tone mapped), not QuickLook.
        return Some(Job { key: (item.id.clone(), tier), path: item.original_path(root), svg: false, quicklook: false, tile: 0, region: None, raw: false, page: 0, hdr: falcon_library::hdr::is_hdr(&ext) });
    }
    // A custom thumbnail stands for a format with no picture of its own.
    if tier == Tier::Full
        && crate::logic::custom_thumbnail_is_picture(&ext, thumbnail::has_custom_thumbnail(item))
        && let Some(t) = item.thumbnail_path(root)
    {
        return Some(Job { key: (item.id.clone(), tier), path: t, svg: false, quicklook: false, tile: 0, region: None, raw: false, page: 0, hdr: false });
    }
    if tier == Tier::Full && falcon_library::psd::handles(&ext) {
        // The merged image, QuickLook where it can't be read and exists.
        return Some(Job { key: (item.id.clone(), tier), path: item.original_path(root), svg: false, quicklook: cfg!(target_os = "macos"), tile: 0, region: None, raw: false, page: 0, hdr: false });
    }
    if tier == Tier::Full && !supported && cfg!(target_os = "macos") {
        return Some(Job { key: (item.id.clone(), tier), path: item.original_path(root), svg: false, quicklook: true, tile: 0, region: None, raw: false, page: 0, hdr: false });
    }
    let (path, svg) = match (tier, item.thumbnail_path(root), original) {
        (Tier::Full, _, Some(o)) => (o, ext == "svg"),
        (_, Some(t), _) => (t, false),
        (_, None, Some(o)) => (o, ext == "svg"),
        (_, None, None) => return None,
    };
    Some(Job { key: (item.id.clone(), tier), path, svg, quicklook: false, tile: 0, region: None, raw: false, page: 0, hdr: false })
}

struct Shared {
    queue: Mutex<VecDeque<Job>>,
    cv: Condvar,
    /// Set when the loader is dropped; the workers then stop.
    closed: std::sync::atomic::AtomicBool,
    /// Grayscale preview: the viewer's tiers decode desaturated.
    gray: std::sync::atomic::AtomicBool,
    /// The HDR exposure (an `f32`'s bits): a multiplier, 1.0 by default.
    exposure: std::sync::atomic::AtomicU32,
    /// The last HDR image decoded, so moving the exposure slider tone maps
    /// again without reading the file: its path, the long side it was
    /// reduced to (`u32::MAX`: not reduced) and the linear pixels.
    linear: Mutex<Option<(PathBuf, u32, Arc<image::Rgba32FImage>)>>,
    /// The last whole-page QuickLook render for a zoomed region: its path,
    /// the long side asked for and the pixels, so panning or zooming out
    /// cuts from it without running QuickLook again. One at most
    /// (`logic::QUICKLOOK_RENDER_PIXELS`), dropped when the viewer leaves
    /// the item.
    rendered: Mutex<Option<(PathBuf, u32, Arc<image::RgbaImage>)>>,
}

/// What a worker hands back: one image, or a full-resolution one in tiles.
enum Pixels {
    Whole(ColorImage),
    Tiled { size: [usize; 2], tiles: Vec<(crate::logic::TileSpan, ColorImage)> },
    /// A render of part of an SVG or a QuickLook page.
    Patch(Region, ColorImage),
}

struct Decoded {
    key: Key,
    image: Option<Pixels>,
    /// From a region job, which shares its key with the full tier.
    region: bool,
    /// Decoded desaturated (grayscale preview).
    gray: bool,
    /// For an HDR job: the exposure (bits) it was tone mapped at.
    exposure: Option<u32>,
}

/// Whether a tier follows the grayscale preview: every one, as Eagle's
/// filter grays the grid's thumbnails as well as the viewer.
fn grays(_tier: Tier) -> bool {
    true
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
    /// The viewed SVG's or QuickLook format's sharp render of what is on
    /// screen, the region last asked for (with its item), and the count of
    /// regions asked for.
    patch: Option<Patch>,
    region_last: Option<(String, [f32; 4], [u32; 2], u32)>,
    region_seq: u64,
    /// Grayscale preview, and the cached renders made before it last
    /// changed: still shown until their replacement arrives, so switching
    /// doesn't blank the view.
    gray: bool,
    stale: HashSet<Key>,
    /// The HDR exposure (a multiplier) and the keys queued for HDR images.
    exposure: f32,
    hdr_keys: HashSet<Key>,
}

impl Loader {
    pub fn new(ctx: &Context) -> Self {
        let shared = Arc::new(Shared { queue: Mutex::new(VecDeque::new()), cv: Condvar::new(), closed: Default::default(), gray: Default::default(), exposure: std::sync::atomic::AtomicU32::new(1.0f32.to_bits()), linear: Mutex::new(None), rendered: Mutex::new(None) });
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
                    let exposure = shared.exposure.load(Ordering::Relaxed);
                    let mut image = decode(&job, f32::from_bits(exposure), &shared.linear, &shared.rendered);
                    if gray && let Some(p) = &mut image {
                        desaturate(p);
                    }
                    if tx.send(Decoded { key: job.key, image, region: job.region.is_some(), gray, exposure: job.hdr.then_some(exposure) }).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        let caches = [Tier::Grid, Tier::Preview, Tier::Full, Tier::Card]
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
            patch: None,
            region_last: None,
            region_seq: 0,
            gray: false,
            stale: HashSet::new(),
            exposure: 1.0,
            hdr_keys: HashSet::new(),
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
            // Tone mapped before the exposure moved: asked again.
            if d.exposure.is_some_and(|e| e != self.exposure.to_bits()) {
                self.pending.remove(&d.key);
                continue;
            }
            if d.region {
                let Some(Pixels::Patch(region, img)) = d.image else { continue };
                // Only the latest region for the item still on screen.
                let current = self.region_last.as_ref().is_some_and(|l| l.0 == d.key.0);
                if current && self.patch.as_ref().is_none_or(|p| p.seq < region.seq) {
                    let [x0, y0, x1, y1] = region.rect;
                    let texture = ctx.load_texture(format!("{}-region", d.key.0), img, TextureOptions::LINEAR);
                    let rect = Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1));
                    self.patch = Some(Patch { id: d.key.0, rect, texture, seq: region.seq });
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
                    // The same item's old tiles stay up while the new ones
                    // upload (after a sampling or exposure change).
                    if self.original.as_ref().is_some_and(|o| o.id != d.key.0) {
                        self.original = None;
                    }
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

    /// Switches the viewer's renders between sharp and smooth enlarging.
    /// Sampling is fixed when a texture is uploaded, so on a change what is
    /// cached is marked stale and made again, still shown until the new
    /// textures arrive (zooming across 200% doesn't blank the view).
    /// Decodes in flight are uploaded with the new sampling.
    pub fn set_pixelated(&mut self, on: bool) {
        if self.pixelated == on {
            return;
        }
        self.pixelated = on;
        self.stale.extend(self.caches[&Tier::Full].iter().map(|(id, _)| (id.clone(), Tier::Full)));
        if let Some(o) = &self.original {
            self.stale.insert((o.id.clone(), Tier::Original));
        }
        self.staged = None;
    }

    /// Turns the grayscale preview on or off for every render, the grid's
    /// cards included. What is cached is marked stale and
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
        self.region_last = None;
        self.patch = None;
    }

    /// Sets the exposure (a multiplier) HDR and EXR images are tone mapped
    /// at. On a change, what is cached for them is marked stale and made
    /// again, still shown until the new pixels arrive.
    pub fn set_exposure(&mut self, exposure: f32) {
        if self.exposure == exposure {
            return;
        }
        self.exposure = exposure;
        self.shared.exposure.store(exposure.to_bits(), Ordering::Relaxed);
        for key in &self.hdr_keys {
            let have = match key.1 {
                Tier::Original => self.original.as_ref().is_some_and(|o| o.id == key.0) || self.staged.as_ref().is_some_and(|s| s.original.id == key.0),
                tier => self.caches.get(&tier).is_some_and(|c| c.peek(&key.0).is_some()),
            };
            if have {
                self.stale.insert(key.clone());
            }
        }
    }

    /// The full-resolution original of `id`, once every tile is uploaded.
    pub fn original(&self, id: &str) -> Option<&Original> {
        self.original.as_ref().filter(|o| o.id == id)
    }

    /// Asks for the part `rect` of `job` (a full-tier SVG job, or a
    /// [`quicklook_region_job`]) to be rendered at `size` (SVG) or with the
    /// whole page `render` pixels on its long side (QuickLook), unless that
    /// is what was last asked for. The job goes to the front of the queue;
    /// an older region still queued is dropped.
    pub fn want_region(&mut self, mut job: Job, rect: [f32; 4], size: [u32; 2], render: u32) {
        let last = (job.key.0.clone(), rect, size, render);
        if self.region_last.as_ref() == Some(&last) {
            return;
        }
        self.region_last = Some(last);
        self.region_seq += 1;
        job.region = Some(Region { rect, size, render, seq: self.region_seq });
        let mut q = self.shared.queue.lock().unwrap();
        q.retain(|j| j.region.is_none());
        q.push_front(job);
        drop(q);
        self.shared.cv.notify_all();
    }

    /// The sharp render of part of `id`, if one has arrived.
    pub fn patch(&self, id: &str) -> Option<&Patch> {
        self.patch.as_ref().filter(|p| p.id == id)
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
        if self.region_last.as_ref().is_some_and(|l| Some(l.0.as_str()) != id) {
            self.region_last = None;
            self.patch = None;
            *self.shared.rendered.lock().unwrap_or_else(|p| p.into_inner()) = None;
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
            // Regions are managed by `want_region`.
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
            if j.hdr {
                self.hdr_keys.insert(j.key.clone());
            }
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
        if self.region_last.as_ref().is_some_and(|l| l.0 == id) {
            self.region_last = None;
            self.patch = None;
        }
        // The file may have changed under the kept QuickLook render.
        *self.shared.rendered.lock().unwrap_or_else(|p| p.into_inner()) = None;
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

/// Decodes `job`; HDR images are tone mapped at `exposure`, reading the file
/// only when `linear` doesn't already hold it.
fn decode(job: &Job, exposure: f32, linear: &Mutex<Option<(PathBuf, u32, Arc<image::Rgba32FImage>)>>, rendered: &Rendered) -> Option<Pixels> {
    if let Some(region) = job.region
        && job.quicklook
    {
        if !is_psd(&job.path) {
            return quicklook_patch(&job.path, region, rendered);
        }
        // QuickLook's limits apply again if the merged image fails.
        let fallback = Region { render: region.render.min(crate::logic::QUICKLOOK_RENDER_PIXELS.sqrt() as u32), ..region };
        return psd_patch(&job.path, region).or_else(|| quicklook_patch(&job.path, fallback, rendered));
    }
    if let Some(region) = job.region {
        let data = std::fs::read(&job.path).ok()?;
        let rgba = thumbnail::render_svg_region(&data, region.rect, (region.size[0], region.size[1])).ok()?.into_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        return Some(Pixels::Patch(region, ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())));
    }
    if job.key.1 == Tier::Original {
        return decode_tiled(job, exposure, linear);
    }
    let max = job.key.1.max_side();
    if job.page > 0 {
        let rgba = crate::tiff_pages::decode_page(&job.path, job.page, max)?;
        return Some(Pixels::Whole(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())));
    }
    if is_psd(&job.path)
        && let Some(rgba) = psd_whole(&job.path, max)
    {
        return Some(Pixels::Whole(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())));
    }
    let img = if job.hdr {
        tone_mapped(job, max, exposure, linear)?
    } else if job.raw {
        match raw_preview(&job.path) {
            Some(img) => img,
            None if job.quicklook => thumbnail::quicklook(&job.path, max).ok()?,
            None => return None,
        }
    } else if job.quicklook {
        native_or_quicklook(&job.path, max)?
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

/// Whether `path` is a Photoshop document, read for its merged image.
fn is_psd(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| falcon_library::psd::handles(&e.to_lowercase()))
}

/// A PSD's merged image, at most `max` pixels on its long side.
fn psd_whole(path: &std::path::Path, max: u32) -> Option<image::RgbaImage> {
    let mut c = Composite::open(path).ok()?;
    let (w, h) = c.size();
    c.read_scaled([0, 0, w, h], max).ok()
}

/// The part of a PSD's merged image on screen, at the scale of a whole
/// picture `region.render` pixels on its long side. Only those rows are
/// decoded and only the result is kept, so it costs what the screen shows.
/// The rectangle given back is the whole pixels cut, so the part lines up
/// with the render under it however few pixels are on screen.
fn psd_patch(path: &std::path::Path, region: Region) -> Option<Pixels> {
    let mut c = Composite::open(path).ok()?;
    let (w, h) = c.size();
    let [x0, y0, x1, y1] = region.rect;
    let (fw, fh) = (f64::from(w), f64::from(h));
    let below = |f: f32, n: f64| (f64::from(f.clamp(0.0, 1.0)) * n).floor() as u32;
    let above = |f: f32, n: f64| (f64::from(f.clamp(0.0, 1.0)) * n).ceil() as u32;
    let (px0, py0) = (below(x0, fw).min(w - 1), below(y0, fh).min(h - 1));
    let (px1, py1) = (above(x1, fw).clamp(px0 + 1, w), above(y1, fh).clamp(py0 + 1, h));
    let scale = (f64::from(region.render) / fw.max(fh)).min(1.0);
    let long = (f64::from((px1 - px0).max(py1 - py0)) * scale).ceil() as u32;
    let part = c.read_scaled([px0, py0, px1, py1], long).ok()?;
    let exact = Region { rect: [(f64::from(px0) / fw) as f32, (f64::from(py0) / fh) as f32, (f64::from(px1) / fw) as f32, (f64::from(py1) / fh) as f32], ..region };
    Some(Pixels::Patch(exact, ColorImage::from_rgba_unmultiplied([part.width() as usize, part.height() as usize], part.as_raw())))
}

/// A PSD's merged image in tiles, read straight from the file row by row;
/// `None` past `logic::COMPOSITE_TILED_PIXELS` or when it can't be read.
/// Peak memory is the picture at 4 bytes a pixel (with each tile's overlap)
/// plus one tile while it is converted, and `falcon_library::psd`'s buffers.
fn psd_tiled(job: &Job) -> Option<Pixels> {
    let mut c = Composite::open(&job.path).ok()?;
    let (w, h) = c.size();
    if !crate::logic::composite_tiled(w, h) {
        return None;
    }
    let spans = crate::logic::tile_spans(w as usize, h as usize, job.tile);
    let rects: Vec<[u32; 4]> = spans.iter().map(|s| s.padded.map(|v| v as u32)).collect();
    let images = c.read_rects(&rects).ok()?;
    let tiles = spans.into_iter().zip(images).map(|(span, img)| (span, ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))).collect();
    Some(Pixels::Tiled { size: [w as usize, h as usize], tiles })
}

/// The kept whole-page QuickLook render (see `Shared::rendered`).
type Rendered = Mutex<Option<(PathBuf, u32, Arc<image::RgbaImage>)>>;

/// The part `region.rect` of the page at `path`, from a QuickLook render
/// with `region.render` pixels on its long side: the kept render when it is
/// of this file and at least that large (scaled down to the size asked),
/// else a new one, which is kept in its place.
fn quicklook_patch(path: &std::path::Path, region: Region, rendered: &Rendered) -> Option<Pixels> {
    let want = region.render;
    let kept = rendered.lock().ok()?.as_ref().filter(|(p, side, _)| p == path && *side >= want).map(|(_, _, img)| img.clone());
    let page = match kept {
        Some(img) => img,
        None => {
            // The old render goes first, so two never sit in memory.
            *rendered.lock().ok()? = None;
            let img = Arc::new(thumbnail::quicklook(path, want).ok()?.into_rgba8());
            *rendered.lock().ok()? = Some((path.to_path_buf(), want, img.clone()));
            img
        }
    };
    let (x, y, w, h) = crate::logic::crop_span(region.rect, page.width(), page.height());
    let part = image::imageops::crop_imm(&*page, x, y, w, h).to_image();
    // A larger render kept from before is brought to the size asked.
    let long = page.width().max(page.height());
    let part = if long > want {
        let s = want as f32 / long as f32;
        let (pw, ph) = (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1));
        image::imageops::resize(&part, pw, ph, image::imageops::FilterType::Triangle)
    } else {
        part
    };
    let size = [part.width() as usize, part.height() as usize];
    Some(Pixels::Patch(region, ColorImage::from_rgba_unmultiplied(size, part.as_raw())))
}

/// Whether ImageIO decodes `path` here (AVIF, HEIC/HEIF, JPEG XL on a
/// system that reads it).
fn imageio(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(falcon_library::imageio::available)
}

/// A format falcon's own decoder can't read: ImageIO's full decode when it
/// reads the format, else (or when that fails) a QuickLook render.
fn native_or_quicklook(path: &std::path::Path, max: u32) -> Option<image::DynamicImage> {
    if imageio(path)
        && let Ok(img) = falcon_library::imageio::decode(path)
    {
        return Some(img);
    }
    thumbnail::quicklook(path, max).ok()
}

/// A RAW photo's embedded JPEG preview.
fn raw_preview(path: &std::path::Path) -> Option<image::DynamicImage> {
    thumbnail::raw_preview(&std::fs::read(path).ok()?).ok()
}

/// The HDR image of `job` tone mapped at `exposure`, at most `max` pixels on
/// its long side. The linear pixels are kept for the next exposure: reduced
/// to `max` when that is all that was asked for (the full tier), else whole;
/// a whole one serves every size.
fn tone_mapped(job: &Job, max: u32, exposure: f32, linear: &Mutex<Option<(PathBuf, u32, Arc<image::Rgba32FImage>)>>) -> Option<image::DynamicImage> {
    let cached = linear.lock().ok()?.as_ref().filter(|(p, side, _)| *p == job.path && *side >= max).map(|(_, _, img)| img.clone());
    let img = match cached {
        Some(img) => img,
        None => {
            let img = falcon_library::hdr::decode_linear(&job.path).ok()?;
            let long = img.width().max(img.height());
            let (side, img) = if long > max {
                let scale = max as f32 / long as f32;
                let (w, h) = (((img.width() as f32 * scale).round() as u32).max(1), ((img.height() as f32 * scale).round() as u32).max(1));
                (max, image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle))
            } else {
                (u32::MAX, img)
            };
            let img = Arc::new(img);
            *linear.lock().ok()? = Some((job.path.clone(), side, img.clone()));
            img
        }
    };
    // A whole image kept for a smaller size is reduced again here.
    let long = img.width().max(img.height());
    if long > max {
        let scale = max as f32 / long as f32;
        let (w, h) = (((img.width() as f32 * scale).round() as u32).max(1), ((img.height() as f32 * scale).round() as u32).max(1));
        let small = image::imageops::resize(&*img, w, h, image::imageops::FilterType::Triangle);
        return Some(image::DynamicImage::ImageRgba8(falcon_library::hdr::tone_map(&small, exposure)));
    }
    Some(image::DynamicImage::ImageRgba8(falcon_library::hdr::tone_map(&img, exposure)))
}

/// Decodes the original at full size and cuts it into tiles, all on the
/// worker; the UI thread only uploads them.
fn decode_tiled(job: &Job, exposure: f32, linear: &Mutex<Option<(PathBuf, u32, Arc<image::Rgba32FImage>)>>) -> Option<Pixels> {
    if is_psd(&job.path) {
        return psd_tiled(job);
    }
    let img = if job.hdr {
        tone_mapped(job, u32::MAX, exposure, linear)?
    } else if job.raw {
        raw_preview(&job.path)?
    } else if imageio(&job.path) {
        falcon_library::imageio::decode(&job.path).ok()?
    } else {
        thumbnail::decode(&job.path).ok()?
    };
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
        Job { key: ("x".into(), Tier::Original), path, svg: false, quicklook: false, tile, region: None, raw: false, page: 0, hdr: false }
    }

    /// A zoomed QuickLook region is cut from the kept page render when it
    /// is large enough (no QuickLook run), scaled to the size asked.
    #[test]
    fn quicklook_regions_cut_from_the_kept_render() {
        let path = PathBuf::from("/nonexistent/deck.pptx");
        let page = image::RgbaImage::from_fn(1000, 500, |x, y| image::Rgba([(x / 4) as u8, (y / 2) as u8, 0, 255]));
        let rendered: Rendered = Mutex::new(Some((path.clone(), 1000, Arc::new(page))));
        let job = Job { key: ("x".into(), Tier::Full), path, svg: false, quicklook: true, tile: 0, region: Some(Region { rect: [0.5, 0.0, 1.0, 0.5], size: [0, 0], render: 1000, seq: 1 }), raw: false, page: 0, hdr: false };
        let Some(Pixels::Patch(_, img)) = decode(&job, 1.0, &Mutex::new(None), &rendered) else { panic!("no patch") };
        assert_eq!(img.size, [500, 250]);
        assert_eq!(img.pixels[0], Color32::from_rgb(125, 0, 0));
        // Zoomed out to half: the same render, scaled down.
        let job = Job { region: Some(Region { rect: [0.0, 0.0, 1.0, 1.0], size: [0, 0], render: 500, seq: 2 }), ..job };
        let Some(Pixels::Patch(_, img)) = decode(&job, 1.0, &Mutex::new(None), &rendered) else { panic!("no patch") };
        assert_eq!(img.size, [500, 250]);
    }

    #[test]
    fn originals_decode_into_padded_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), 300, 130);
        let Some(Pixels::Tiled { size, tiles }) = decode(&original(path, 128), 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("not tiled") };
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

    /// A raw RGB PSD holding a ramp: red is `x`, green is `y`, blue is 7.
    fn write_psd(dir: &std::path::Path, name: &str, w: u32, h: u32) -> PathBuf {
        let mut f = b"8BPS\0\x01\0\0\0\0\0\0\0\x03".to_vec();
        f.extend(h.to_be_bytes());
        f.extend(w.to_be_bytes());
        f.extend([0, 8, 0, 3]);
        f.extend([0; 12]);
        f.extend([0, 0]);
        let (wu, hu) = (w as usize, h as usize);
        f.extend((0..wu * hu).map(|i| (i % wu % 256) as u8));
        f.extend((0..wu * hu).map(|i| (i / wu % 256) as u8));
        f.extend(std::iter::repeat_n(7u8, wu * hu));
        let path = dir.join(name);
        std::fs::write(&path, f).unwrap();
        path
    }

    /// A merged image past QuickLook's cap: tiles carry the file's pixels.
    #[test]
    fn psd_originals_decode_into_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_psd(dir.path(), "x.psd", 300, 130);
        let Some(Pixels::Tiled { size, tiles }) = decode(&original(path, 128), 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("not tiled") };
        assert_eq!(size, [300, 130]);
        assert_eq!(tiles.len(), 3 * 2);
        for (span, img) in &tiles {
            let [x0, y0, x1, y1] = span.padded;
            assert_eq!(img.size, [x1 - x0, y1 - y0]);
            let (x, y) = (x1 - 1, y1 - 1);
            assert_eq!(img.pixels[img.pixels.len() - 1], Color32::from_rgb((x % 256) as u8, y as u8, 7));
        }
    }

    /// The full tier scales the merged image; the part on screen comes from
    /// it at the zoom shown, aligned to whole pixels.
    #[test]
    fn psd_full_tier_and_regions_come_from_the_merged_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_psd(dir.path(), "x.psd", 400, 200);
        let job = Job { key: ("x".into(), Tier::Full), path: path.clone(), svg: false, quicklook: true, tile: 0, region: None, raw: false, page: 0, hdr: false };
        // Small enough to decode whole, with no QuickLook needed.
        let Some(Pixels::Whole(img)) = decode(&job, 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("no full tier") };
        assert_eq!(img.size, [400, 200]);
        assert_eq!(img.pixels[399], Color32::from_rgb(143, 0, 7));
        let region = Region { rect: [0.5, 0.5, 0.6251, 0.625], size: [0, 0], render: 400, seq: 1 };
        let job = Job { region: Some(region), ..job };
        let Some(Pixels::Patch(got, img)) = decode(&job, 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("no patch") };
        assert_eq!(img.size, [51, 25], "cut outward to whole pixels");
        assert_eq!(got.rect[2], 251.0 / 400.0);
        assert_eq!(img.pixels[0], Color32::from_rgb(200, 100, 7));
        // At half scale the same part is half the size.
        let job = Job { region: Some(Region { render: 200, ..region }), ..job };
        let Some(Pixels::Patch(_, img)) = decode(&job, 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("no patch") };
        assert_eq!(img.size, [26, 13]);
    }

    /// An RLE RGB PSB with a row pattern that doesn't compress to nothing,
    /// written a row at a time.
    fn write_large_psb(path: &std::path::Path, w: u32, h: u32) {
        use std::io::Write;
        let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        out.write_all(b"8BPS\0\x02\0\0\0\0\0\0\0\x03").unwrap();
        out.write_all(&h.to_be_bytes()).unwrap();
        out.write_all(&w.to_be_bytes()).unwrap();
        out.write_all(&[0, 8, 0, 3]).unwrap();
        out.write_all(&[0; 16]).unwrap();
        out.write_all(&[0, 1]).unwrap();
        // Runs of 16 pixels: 2 bytes each, for 8 times compression.
        let row_of = |seed: u32| -> Vec<u8> { (0..w.div_ceil(16)).flat_map(|i| { let run = 16.min(w - i * 16) as u8; if run == 1 { [0, (i ^ seed) as u8] } else { [(257 - u32::from(run)) as u8, (i ^ seed) as u8] } }).collect() };
        for _ in 0..3 * h {
            out.write_all(&(row_of(0).len() as u32).to_be_bytes()).unwrap();
        }
        for y in 0..3 * h {
            out.write_all(&row_of(y / 64)).unwrap();
        }
    }

    /// A 20000 x 12000 merged image in tiles and a 40000 x 30000 one read in
    /// part. Slow in a debug build and writes about 1.3 GB of files, so run
    /// it with `cargo test --release -p falcon-app -- --ignored large_psd`.
    /// See the peak memory with `/usr/bin/time -l` on the test binary.
    #[test]
    #[ignore]
    fn large_psd_decodes_within_its_memory_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.psb");
        let which = std::env::var("FALCON_LARGE_PSD").unwrap_or_default();
        if which != "part" {
            write_large_psb(&path, 20000, 12000);
            let t = std::time::Instant::now();
            let Some(Pixels::Tiled { size, tiles }) = decode(&original(path.clone(), 2048), 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("not tiled") };
            eprintln!("240 MP in tiles: {:?}, {} tiles", t.elapsed(), tiles.len());
            assert_eq!(size, [20000, 12000]);
            drop(tiles);
        }
        if which != "tiles" {
            write_large_psb(&path, 40000, 30000);
            let t = std::time::Instant::now();
            assert!(decode(&original(path.clone(), 2048), 1.0, &Mutex::new(None), &Mutex::new(None)).is_none(), "past the budget for tiles");
            eprintln!("refused in {:?}", t.elapsed());
            let full = Job { key: ("x".into(), Tier::Full), path: path.clone(), svg: false, quicklook: true, tile: 0, region: None, raw: false, page: 0, hdr: false };
            let t = std::time::Instant::now();
            let Some(Pixels::Whole(img)) = decode(&full, 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("no full tier") };
            eprintln!("full tier {:?} in {:?}", img.size, t.elapsed());
            let region = Region { rect: [0.5, 0.5, 0.55, 0.55], size: [0, 0], render: 40000, seq: 1 };
            let t = std::time::Instant::now();
            let Some(Pixels::Patch(_, img)) = decode(&Job { region: Some(region), ..full }, 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("no patch") };
            eprintln!("part {:?} in {:?}", img.size, t.elapsed());
            assert!(img.size[0].abs_diff(2000) <= 1 && img.size[1].abs_diff(1500) <= 1, "{:?}", img.size);
        }
    }

    /// Bad data ends in no pixels (QuickLook's turn), with no panic.
    #[test]
    fn a_damaged_psd_gives_no_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_psd(dir.path(), "x.psd", 300, 130);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(decode(&original(path, 128), 1.0, &Mutex::new(None), &Mutex::new(None)).is_none());
    }

    /// An EXR is tone mapped at the exposure asked for, from the linear
    /// pixels kept after the first read; a TGA decodes in full.
    #[test]
    fn hdr_tone_maps_from_the_kept_linear_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.exr");
        let lin = image::Rgba32FImage::from_pixel(8, 4, image::Rgba([1.0, 1.0, 1.0, 1.0]));
        image::DynamicImage::ImageRgba32F(lin).save(&path).unwrap();
        let full = Job { key: ("x".into(), Tier::Full), path: path.clone(), svg: false, quicklook: false, tile: 0, region: None, raw: false, page: 0, hdr: true };
        let kept = Mutex::new(None);
        let Some(Pixels::Whole(a)) = decode(&full, 1.0, &kept, &Mutex::new(None)) else { panic!("no full tier") };
        assert_eq!(a.size, [8, 4]);
        assert_eq!(a.pixels[0], Color32::from_rgb(188, 188, 188));
        // The file is gone, yet a new exposure is made from what was kept.
        std::fs::remove_file(&path).unwrap();
        let Some(Pixels::Whole(b)) = decode(&full, 4.0, &kept, &Mutex::new(None)) else { panic!("not kept") };
        assert!(b.pixels[0].r() > 188);
        let original = Job { key: ("x".into(), Tier::Original), tile: 128, ..full };
        let Some(Pixels::Tiled { size, .. }) = decode(&original, 1.0, &kept, &Mutex::new(None)) else { panic!("not tiled") };
        assert_eq!(size, [8, 4]);
    }

    #[test]
    fn tga_originals_decode_in_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.tga");
        image::RgbaImage::from_pixel(300, 130, image::Rgba([10, 20, 30, 255])).save(&path).unwrap();
        let Some(Pixels::Tiled { size, .. }) = decode(&original(path, 128), 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("not tiled") };
        assert_eq!(size, [300, 130]);
    }

    /// A HEIC original (which falcon's own decoder can't read) decodes
    /// through ImageIO into tiles, upright, and its full tier too.
    #[cfg(target_os = "macos")]
    #[test]
    fn heic_originals_decode_through_imageio() {
        use falcon_library::imageio::{WriteFrame, write};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.heic");
        let img = image::RgbaImage::from_fn(300, 130, |x, _| image::Rgba([(x % 256) as u8, 0, 0, 255]));
        write(&path, "heic", &[WriteFrame { image: &img, orientation: 6, delay: None }]).unwrap();
        let Some(Pixels::Tiled { size, tiles }) = decode(&original(path.clone(), 128), 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("not tiled") };
        assert_eq!(size, [130, 300], "turned by its orientation");
        assert_eq!(tiles.len(), 2 * 3);
        let full = Job { key: ("x".into(), Tier::Full), path, svg: false, quicklook: true, tile: 0, region: None, raw: false, page: 0, hdr: false };
        let Some(Pixels::Whole(img)) = decode(&full, 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("no full tier") };
        assert_eq!(img.size, [130, 300]);
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
        let Some(Pixels::Tiled { size, tiles }) = decode(&original(path, crate::logic::tile_side(16384)), 1.0, &Mutex::new(None), &Mutex::new(None)) else { panic!("not tiled") };
        eprintln!("decoded and tiled in {:?}: {} tiles", t.elapsed(), tiles.len());
        assert_eq!(size, [12000, 8000]);
        assert!(tiles.iter().all(|(_, img)| img.size[0] <= 2048 && img.size[1] <= 2048));
        let biggest = tiles.iter().map(|(_, img)| img.pixels.len() * 4).max().unwrap();
        assert!(biggest <= UPLOAD_BUDGET, "a frame uploads at least one whole tile");
        eprintln!("frames to upload at {} MB a frame: {}", UPLOAD_BUDGET >> 20, tiles.iter().map(|(_, i)| i.pixels.len() * 4).sum::<usize>().div_ceil(UPLOAD_BUDGET));
    }
}
