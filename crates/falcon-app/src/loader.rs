//! Background image decoding and an LRU texture cache.

use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use falcon_library::model::Item;
use falcon_library::thumbnail;
use lru::LruCache;

/// How large a version of an image is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Grid,
    Preview,
    Full,
}

impl Tier {
    fn max_side(self) -> u32 {
        match self {
            Tier::Grid => 512,
            Tier::Preview => 1024,
            Tier::Full => 3072,
        }
    }

    fn capacity(self) -> usize {
        match self {
            Tier::Grid => 900,
            Tier::Preview => 24,
            Tier::Full => 4,
        }
    }
}

pub type Key = (String, Tier);

#[derive(Clone)]
pub struct Job {
    pub key: Key,
    path: PathBuf,
    svg: bool,
}

/// Picks the file to decode for `item`: the thumbnail when it exists (unless
/// the full-size original is wanted and decodable), else a supported original.
pub fn job_for(item: &Item, root: &std::path::Path, tier: Tier) -> Option<Job> {
    let ext = item.ext.to_lowercase();
    let original = thumbnail::is_supported(&ext).then(|| item.original_path(root));
    let (path, svg) = match (tier, item.thumbnail_path(root), original) {
        (Tier::Full, _, Some(o)) => (o, ext == "svg"),
        (_, Some(t), _) => (t, false),
        (_, None, Some(o)) => (o, ext == "svg"),
        (_, None, None) => return None,
    };
    Some(Job { key: (item.id.clone(), tier), path, svg })
}

struct Shared {
    queue: Mutex<VecDeque<Job>>,
    cv: Condvar,
}

struct Decoded {
    key: Key,
    image: Option<ColorImage>,
}

/// Decodes on worker threads; textures are uploaded from the UI thread.
pub struct Loader {
    shared: Arc<Shared>,
    rx: std::sync::mpsc::Receiver<Decoded>,
    caches: HashMap<Tier, LruCache<String, TextureHandle>>,
    pending: HashSet<Key>,
    failed: HashSet<Key>,
}

impl Loader {
    pub fn new(ctx: &Context) -> Self {
        let shared = Arc::new(Shared { queue: Mutex::new(VecDeque::new()), cv: Condvar::new() });
        let (tx, rx) = std::sync::mpsc::channel();
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 6));
        for _ in 0..workers {
            let (shared, tx, ctx) = (shared.clone(), tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let mut q = shared.queue.lock().unwrap();
                        loop {
                            if let Some(j) = q.pop_front() {
                                break j;
                            }
                            q = shared.cv.wait(q).unwrap();
                        }
                    };
                    let image = decode(&job);
                    if tx.send(Decoded { key: job.key, image }).is_err() {
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
        Loader { shared, rx, caches, pending: HashSet::new(), failed: HashSet::new() }
    }

    /// Uploads finished decodes (bounded per call so frames stay short).
    pub fn poll(&mut self, ctx: &Context) {
        for _ in 0..24 {
            let Ok(d) = self.rx.try_recv() else { return };
            self.pending.remove(&d.key);
            match d.image {
                Some(img) => {
                    let tex = ctx.load_texture(format!("{}-{:?}", d.key.0, d.key.1), img, TextureOptions::LINEAR);
                    self.caches.get_mut(&d.key.1).unwrap().put(d.key.0, tex);
                }
                None => {
                    self.failed.insert(d.key);
                }
            }
        }
        ctx.request_repaint();
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
            let keep = wanted.contains(&j.key);
            if !keep {
                pending.remove(&j.key);
            }
            keep
        });
        let mut added = false;
        for j in jobs {
            if self.failed.contains(&j.key)
                || self.pending.contains(&j.key)
                || self.caches[&j.key.1].peek(&j.key.0).is_some()
            {
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

    /// Forgets everything cached for `id` (after the library changes).
    pub fn clear_all(&mut self) {
        for c in self.caches.values_mut() {
            c.clear();
        }
        self.failed.clear();
    }
}

fn decode(job: &Job) -> Option<ColorImage> {
    let max = job.key.1.max_side();
    let img = if job.svg {
        let data = std::fs::read(&job.path).ok()?;
        thumbnail::render_svg(&data, Some(max.min(1024))).ok()?
    } else {
        thumbnail::decode(&job.path).ok()?
    };
    let img = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}
