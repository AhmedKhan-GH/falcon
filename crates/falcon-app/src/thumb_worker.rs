//! Bounded background thumbnail worker pool (Adobe Bridge style).
//!
//! Decouples fast Link-mode asset registration from heavy thumbnail decoding
//! and palette extraction. Workers run outside the library lock with priority
//! scheduling (selected/visible > near-viewport > off-screen), deduplication,
//! failure isolation, anti-starvation, and generation-checked lifecycle safety.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use eframe::egui;
use falcon_library::import::{needs_deferred_thumbnail, stage_deferred_thumbnail};
use falcon_library::model::Item;

use crate::app::{Shared, lock};

/// Maximum retry attempts for a transient failure before isolating the item.
const MAX_RETRIES: u8 = 1;

/// Anti-starvation: after serving this many High-priority jobs in a row,
/// serve one Normal or Low job if available.
const HIGH_BURST_LIMIT: u32 = 8;

/// Priority tier for deferred thumbnail generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub(crate) enum ThumbPriority {
    Low = 0,
    Normal = 1,
    High = 2,
}

#[derive(Debug, Clone)]
pub(crate) struct ThumbProgress {
    pub total: usize,
    pub done: usize,
    #[allow(dead_code)]
    pub failed: usize,
    pub active: bool,
}

#[derive(Default)]
struct QueueState {
    high: VecDeque<String>,
    normal: VecDeque<String>,
    low: VecDeque<String>,
    queued: HashSet<String>,
    in_flight: HashSet<String>,
    retries: HashMap<String, u8>,
    permanently_failed: HashSet<String>,
    completed_ids: Vec<String>,
    total: usize,
    done: usize,
    failed: usize,
    high_streak: u32,
    extract_palette: bool,
    stopped: bool,
}

impl QueueState {
    fn pop_next(&mut self) -> Option<String> {
        let pick_lower = self.high_streak >= HIGH_BURST_LIMIT && (!self.normal.is_empty() || !self.low.is_empty());
        let id = if pick_lower {
            self.high_streak = 0;
            self.normal.pop_front().or_else(|| self.low.pop_front())
        } else if let Some(id) = self.high.pop_front() {
            self.high_streak += 1;
            Some(id)
        } else {
            self.high_streak = 0;
            self.normal.pop_front().or_else(|| self.low.pop_front())
        };
        if let Some(ref id) = id {
            self.queued.remove(id);
            self.in_flight.insert(id.clone());
        }
        id
    }

    fn enqueue(&mut self, id: String, prio: ThumbPriority) -> bool {
        if self.stopped
            || self.queued.contains(&id)
            || self.in_flight.contains(&id)
            || self.permanently_failed.contains(&id)
        {
            return false;
        }
        self.queued.insert(id.clone());
        match prio {
            ThumbPriority::High => self.high.push_back(id),
            ThumbPriority::Normal => self.normal.push_back(id),
            ThumbPriority::Low => self.low.push_back(id),
        }
        self.total += 1;
        true
    }

    fn promote_visible(&mut self, visible: &HashSet<String>, selected: Option<&str>) {
        if self.normal.is_empty() && self.low.is_empty() {
            return;
        }
        let mut promoted = Vec::new();
        self.normal.retain(|id| {
            if visible.contains(id) || selected == Some(id.as_str()) {
                promoted.push(id.clone());
                false
            } else {
                true
            }
        });
        self.low.retain(|id| {
            if visible.contains(id) || selected == Some(id.as_str()) {
                promoted.push(id.clone());
                false
            } else {
                true
            }
        });
        // Put selected at the very front of High, visible right behind it.
        for id in promoted {
            if selected == Some(id.as_str()) {
                self.high.push_front(id);
            } else {
                self.high.push_back(id);
            }
        }
    }
}

/// Bounded background thumbnail worker pool tied to a single library instance.
pub(crate) struct ThumbWorker {
    state: Arc<(Mutex<QueueState>, Condvar)>,
    generation: Arc<AtomicU64>,
    cancel_flag: Arc<AtomicBool>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    expected_root: PathBuf,
}

impl ThumbWorker {
    /// Spawns a bounded worker pool (up to 4 threads) for `lib` at `expected_root`.
    pub(crate) fn new(
        shared: Shared,
        expected_root: PathBuf,
        ctx: egui::Context,
        extract_palette: bool,
    ) -> Self {
        let thread_count = std::thread::available_parallelism()
            .map_or(2, |n| n.get().clamp(1, 4));
        let state = Arc::new((
            Mutex::new(QueueState {
                extract_palette,
                ..Default::default()
            }),
            Condvar::new(),
        ));
        let generation = Arc::new(AtomicU64::new(1));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::with_capacity(thread_count);

        for idx in 0..thread_count {
            let state = Arc::clone(&state);
            let shared = Arc::clone(&shared);
            let expected_root = expected_root.clone();
            let generation = Arc::clone(&generation);
            let cancel_flag = Arc::clone(&cancel_flag);
            let ctx = ctx.clone();

            let handle = std::thread::Builder::new()
                .name(format!("falcon-thumb-{idx}"))
                .spawn(move || {
                    worker_loop(state, shared, expected_root, generation, cancel_flag, ctx);
                })
                .expect("spawn thumbnail worker");
            workers.push(handle);
        }

        Self {
            state,
            generation,
            cancel_flag,
            workers: Mutex::new(workers),
            expected_root,
        }
    }

    /// Scans `items` for any deferred thumbnails still missing on disk
    /// (including restart recovery) and queues them at `Low` priority.
    pub(crate) fn rediscover_pending(&self, items: &[Item]) {
        let mut added = 0usize;
        {
            let (lock_q, _) = &*self.state;
            let mut q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
            for item in items {
                if needs_deferred_thumbnail(item, &self.expected_root)
                    && q.enqueue(item.id.clone(), ThumbPriority::Low)
                {
                    added += 1;
                }
            }
        }
        if added > 0 {
            let (_, cv) = &*self.state;
            cv.notify_all();
        }
    }

    /// Enqueues newly registered item IDs for deferred thumbnail generation.
    pub(crate) fn enqueue_batch(&self, ids: &[String], prio: ThumbPriority) {
        let mut added = 0usize;
        {
            let (lock_q, _) = &*self.state;
            let mut q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
            for id in ids {
                if q.enqueue(id.clone(), prio) {
                    added += 1;
                }
            }
        }
        if added > 0 {
            let (_, cv) = &*self.state;
            cv.notify_all();
        }
    }

    /// Promotes currently visible and selected items to `High` priority
    /// without cancelling or restarting in-flight jobs.
    pub(crate) fn update_priorities(&self, visible: &HashSet<String>, selected: Option<&str>) {
        let (lock_q, _) = &*self.state;
        let mut q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
        q.promote_visible(visible, selected);
    }

    /// Drains IDs whose thumbnails just completed so the UI `Loader` can
    /// refresh their textures immediately.
    pub(crate) fn take_completed(&self) -> Vec<String> {
        let (lock_q, _) = &*self.state;
        let mut q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
        std::mem::take(&mut q.completed_ids)
    }

    /// Current thumbnail worker progress snapshot.
    pub(crate) fn progress(&self) -> ThumbProgress {
        let (lock_q, _) = &*self.state;
        let q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
        let active = !q.queued.is_empty() || !q.in_flight.is_empty();
        ThumbProgress {
            total: q.total,
            done: q.done,
            failed: q.failed,
            active,
        }
    }

    /// Expected library root this worker pool is bound to.
    pub(crate) fn root(&self) -> &PathBuf {
        &self.expected_root
    }

    /// Stops all workers cleanly and waits for them to exit.
    pub(crate) fn shutdown(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.cancel_flag.store(true, Ordering::SeqCst);
        {
            let (lock_q, cv) = &*self.state;
            let mut q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
            q.stopped = true;
            q.high.clear();
            q.normal.clear();
            q.low.clear();
            q.queued.clear();
            cv.notify_all();
        }
        let handles: Vec<_> = {
            let mut w = self.workers.lock().unwrap_or_else(|p| p.into_inner());
            w.drain(..).collect()
        };
        for handle in handles {
            let _ = handle.join();
        }
    }
}

impl Drop for ThumbWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(
    state: Arc<(Mutex<QueueState>, Condvar)>,
    shared: Shared,
    expected_root: PathBuf,
    generation: Arc<AtomicU64>,
    cancel_flag: Arc<AtomicBool>,
    ctx: egui::Context,
) {
    let my_gen = generation.load(Ordering::SeqCst);
    loop {
        if cancel_flag.load(Ordering::SeqCst) || generation.load(Ordering::SeqCst) != my_gen {
            return;
        }
        let (id, extract_palette) = {
            let (lock_q, cv) = &*state;
            let mut q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
            loop {
                if q.stopped || cancel_flag.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(id) = q.pop_next() {
                    break (id, q.extract_palette);
                }
                q = cv.wait(q).unwrap_or_else(|p| p.into_inner());
            }
        };

        // Snapshot item & source path under brief library lock.
        let snapshot = {
            let lib = lock(&shared);
            if lib.root() != expected_root {
                None
            } else {
                lib.item(&id).map(|item| {
                    let info_dir = item.info_dir(&expected_root);
                    let source = lib.resolved_source_path(&id).ok();
                    (item.clone(), info_dir, source)
                })
            }
        };

        let mut ok = false;
        let mut retryable = false;

        if let Some((item, info_dir, Some(source))) = snapshot {
            if !cancel_flag.load(Ordering::SeqCst) && generation.load(Ordering::SeqCst) == my_gen {
                match stage_deferred_thumbnail(&item, &source, &info_dir, extract_palette) {
                    Ok(Some(staged)) => {
                        let mut lib = lock(&shared);
                        if lib.root() == expected_root && generation.load(Ordering::SeqCst) == my_gen {
                            match lib.commit_deferred_thumbnail(&id, staged) {
                                Ok(committed) => ok = committed,
                                Err(_) => retryable = true,
                            }
                        }
                    }
                    Ok(None) => {
                        // Unsupported, text, or mid-decode modified file; mark done cleanly.
                        ok = true;
                    }
                    Err(_) => {
                        // Volume disconnected or transient I/O error.
                        retryable = true;
                    }
                }
            }
        } else {
            // Item deleted, missing source, or library switched/moved.
            ok = true;
        }

        let wake_ui = {
            let (lock_q, _) = &*state;
            let mut q = lock_q.lock().unwrap_or_else(|p| p.into_inner());
            q.in_flight.remove(&id);
            if ok {
                q.done += 1;
                q.completed_ids.push(id);
                true
            } else if retryable && !q.stopped {
                let count = q.retries.entry(id.clone()).or_insert(0);
                if *count < MAX_RETRIES {
                    *count += 1;
                    q.queued.insert(id.clone());
                    q.low.push_back(id);
                    false
                } else {
                    q.permanently_failed.insert(id);
                    q.done += 1;
                    q.failed += 1;
                    false
                }
            } else {
                q.done += 1;
                q.failed += 1;
                false
            }
        };

        if wake_ui {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_library::Library;
    use falcon_library::import::ImportOptions;
    use falcon_library::storage_mode::StorageMode;
    use image::{DynamicImage, Rgb, RgbImage};
    use std::time::{Duration, Instant};

    #[test]
    fn deferred_worker_generates_thumbnails_and_resumes_after_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_dir = tmp.path().join("Lib.library");
        let src_dir = tmp.path().join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();

        let mut lib = Library::create(&lib_dir).unwrap();
        lib.set_storage_mode(StorageMode::Link).unwrap();

        let mut ids = Vec::new();
        for i in 0..4 {
            let src = src_dir.join(format!("img_{i}.png"));
            let img = RgbImage::from_fn(1200, 900, |x, y| {
                Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
            });
            DynamicImage::ImageRgb8(img).save(&src).unwrap();
            let id = lib
                .import_file(
                    &src,
                    &ImportOptions {
                        defer_thumbnail: true,
                        palette: false,
                        ..Default::default()
                    },
                )
                .unwrap();
            let item = lib.item(&id).unwrap();
            assert_eq!(item.width, 1200);
            assert_eq!(item.height, 900);
            assert!(item.thumbnail_path(&lib_dir).is_none());
            assert!(needs_deferred_thumbnail(item, &lib_dir));
            ids.push(id);
        }
        drop(lib);

        // Reopen library (simulating restart) and rediscover pending thumbnails.
        let lib = Library::open(&lib_dir).unwrap();
        let items: Vec<Item> = lib.items().to_vec();
        let shared = Arc::new(Mutex::new(lib));
        let ctx = egui::Context::default();
        let worker = ThumbWorker::new(Arc::clone(&shared), lib_dir.clone(), ctx, false);
        worker.rediscover_pending(&items);

        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if !worker.progress().active && worker.progress().done == 4 {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        let prog = worker.progress();
        assert_eq!(prog.done, 4);
        assert_eq!(prog.failed, 0);
        worker.shutdown();

        let lib = lock(&shared);
        for id in &ids {
            let item = lib.item(id).unwrap();
            assert!(item.thumbnail_path(&lib_dir).is_some(), "thumbnail missing for {id}");
            assert!(!needs_deferred_thumbnail(item, &lib_dir));
        }
    }

    #[test]
    fn priority_promotion_moves_visible_and_selected_to_high() {
        let mut q = QueueState::default();
        for i in 0..5 {
            q.enqueue(format!("ID{i}"), ThumbPriority::Low);
        }
        let mut visible = HashSet::new();
        visible.insert("ID3".to_string());
        visible.insert("ID4".to_string());
        q.promote_visible(&visible, Some("ID4"));

        assert_eq!(q.pop_next().as_deref(), Some("ID4"));
        assert_eq!(q.pop_next().as_deref(), Some("ID3"));
    }
}
