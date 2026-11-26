//! The worker thread behind AI Search. It owns the embedder and one
//! library's index at a time, keeps the index up to date with the library,
//! and answers searches ahead of indexing.
//!
//! The library lock is only held to read the list of items, never while
//! pictures are read or the model runs. Indexing is throttled: after each
//! batch the worker waits as long as the batch took, so it uses at most half
//! of what the machine could give it and browsing stays smooth.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use falcon_library::Library;
use falcon_library::linked::{AiSourceCondition, AiSourceSnapshot, evaluate_ai_source_snapshot};
use falcon_library::thumbnail;
use image::DynamicImage;

use crate::embed::Embedder;
use crate::store::{Hit, Index};

pub type Shared = Arc<Mutex<Library>>;
pub type Loader = Box<dyn FnMut() -> Result<Box<dyn Embedder>, String> + Send>;

/// Items per trip through the model.
pub const BATCH: usize = 8;
/// How often the library is looked at for changes.
const CHECK_EVERY: Duration = Duration::from_secs(2);
/// How often every thumbnail file is looked at, for one that changed without
/// its item changing.
const FULL_CHECK_EVERY: Duration = Duration::from_secs(300);
/// The model is dropped after this long with nothing to do: it holds about
/// 600 MB.
const UNLOAD_AFTER: Duration = Duration::from_secs(120);

/// How close to the query a result must be. CLIP's text-to-picture scores
/// are low: on the pictures in `tests/real_model.rs` a match scored 0.23 to
/// 0.36 and anything else stayed below 0.25, while pictures of one class
/// averaged 0.61 against each other and other classes 0.28.
pub const TEXT_MIN: f32 = 0.20;
pub const IMAGE_MIN: f32 = 0.55;
/// Text results further than this below the best one are dropped.
pub const TEXT_SPREAD: f32 = 0.08;

/// What to search by.
pub enum Query {
    Text(String),
    Image(Box<DynamicImage>),
    /// A picture on disk (a dropped file).
    ImageFile(PathBuf),
    /// The bytes of a picture file.
    ImageBytes(Vec<u8>),
    /// Items like this one.
    Item(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchError {
    /// AI Search is turned off or the model isn't installed.
    NotReady,
    /// No library is open.
    NoLibrary,
    /// The picture couldn't be read.
    BadImage(String),
    /// There is no such item.
    NoItem,
    /// The model failed.
    Failed(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::NotReady => write!(f, "AI Search isn't ready"),
            SearchError::NoLibrary => write!(f, "no library is open"),
            SearchError::BadImage(e) => write!(f, "can't read the image: {e}"),
            SearchError::NoItem => write!(f, "item not found"),
            SearchError::Failed(e) => write!(f, "{e}"),
        }
    }
}

/// What the worker is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Turned off in Preferences.
    #[default]
    Off,
    /// The model isn't installed.
    NoModel,
    /// Loading the model.
    Loading,
    /// Reading the library's pictures.
    Indexing,
    /// Indexing stopped by the user, with items left.
    Paused,
    /// Everything is indexed.
    Ready,
    /// The model failed; `Status::error` says how.
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub phase: Phase,
    /// Items in the library, not counting the Trash.
    pub total: usize,
    /// Of those, how many have been looked at.
    pub done: usize,
    /// Of those, how many had no picture to read.
    pub skipped: usize,
    /// Items whose last known valid embedding is kept while source refresh is pending/failed.
    pub stale: usize,
    /// Items whose source is currently offline, missing, invalid, or temporarily inaccessible.
    pub unavailable: usize,
    /// "Metal" or "CPU", once the model has loaded.
    pub device: Option<String>,
    pub error: Option<String>,
    /// Pictures per second over the last batch, before the throttle's wait.
    pub rate: f32,
    /// The library this is about: the worker's, which can be a moment behind
    /// the one it was last given.
    pub root: Option<PathBuf>,
}

impl Status {
    /// The model is installed and turned on, so searches can be answered.
    pub fn searchable(&self) -> bool {
        matches!(self.phase, Phase::Loading | Phase::Indexing | Phase::Paused | Phase::Ready)
    }

    /// Every picture of this library has been looked at, so a search sees
    /// all of it.
    pub fn index_ready(&self, root: &Path) -> bool {
        self.phase == Phase::Ready && self.root.as_deref() == Some(root)
    }
}

pub struct Options {
    /// Where the libraries' indexes are kept: falcon's app data.
    pub index_dir: PathBuf,
    /// Written in each index, which is thrown away when this changes.
    pub model_id: String,
    pub dim: usize,
    /// Whether the worker waits between batches.
    pub throttle: bool,
    pub is_installed: Box<dyn Fn() -> bool + Send>,
    pub loader: Loader,
    /// Runs when the status changes or a search finishes.
    pub notify: Box<dyn Fn() + Send>,
}

enum Msg {
    Library(Option<Shared>),
    Enabled(bool),
    Paused(bool),
    ModelChanged,
    Rebuild,
    Refresh,
    ForgetIndexes,
    RelocateIndex(PathBuf, PathBuf),
    Search(Query, usize, Sender<Result<Vec<Hit>, SearchError>>),
    Stop,
}

/// The handle the app keeps. Dropping it stops the worker.
pub struct Service {
    tx: Sender<Msg>,
    status: Arc<Mutex<Status>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Service {
    pub fn start(options: Options) -> Service {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let thread = std::thread::Builder::new()
            .name("falcon-ai-search".to_string())
            .spawn(move || Worker::new(options, shared).run(&rx))
            .ok();
        Service { tx, status, thread }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The library to index and search; only one at a time.
    pub fn set_library(&self, library: Option<Shared>) {
        let _ = self.tx.send(Msg::Library(library));
    }

    pub fn set_enabled(&self, on: bool) {
        let _ = self.tx.send(Msg::Enabled(on));
    }

    /// Stops or resumes indexing. Searches still work.
    pub fn set_paused(&self, paused: bool) {
        let _ = self.tx.send(Msg::Paused(paused));
    }

    /// The model was installed or removed.
    pub fn model_changed(&self) {
        let _ = self.tx.send(Msg::ModelChanged);
    }

    /// Throws the shown library's index away and starts it again.
    pub fn rebuild(&self) {
        let _ = self.tx.send(Msg::Rebuild);
    }

    /// Re-checks source availability and incremental stamps immediately.
    pub fn refresh(&self) {
        let _ = self.tx.send(Msg::Refresh);
    }

    /// Deletes every library's index.
    pub fn forget_indexes(&self) {
        let _ = self.tx.send(Msg::ForgetIndexes);
    }

    /// Moves a library's index to its new path after the library moved.
    pub fn relocate_index(&self, old_root: PathBuf, new_root: PathBuf) {
        let _ = self.tx.send(Msg::RelocateIndex(old_root, new_root));
    }

    /// Starts a search; the answer arrives on the returned channel.
    pub fn search(&self, query: Query, limit: usize) -> Receiver<Result<Vec<Hit>, SearchError>> {
        let (reply, answer) = mpsc::channel();
        let _ = self.tx.send(Msg::Search(query, limit, reply));
        answer
    }

    /// A search that waits for its answer, for callers off the UI thread.
    pub fn search_blocking(&self, query: Query, limit: usize, timeout: Duration) -> Result<Vec<Hit>, SearchError> {
        self.search(query, limit).recv_timeout(timeout).unwrap_or_else(|_| Err(SearchError::Failed("timed out".to_string())))
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Distinguishes SVG rasterizer stamps from legacy failed raw-file stamps so
/// previously skipped `< 100 KB` SVGs retry automatically without invalidating
/// existing valid vectors in the index.
const SVG_RASTER_STAMP_TAG: u64 = 0x5356_4732_0000_0000;
const MAX_AI_SVG_BYTES: u64 = 32 * 1024 * 1024;
const MAX_AI_SVG_DIM: u32 = 4096;

/// One item waiting to be read.
struct Job {
    id: String,
    stamp: u64,
    picture: PathBuf,
    source: AiSourceSnapshot,
    use_svg_raster: bool,
}

/// An item as the index looks at it. Made under the library lock without
/// performing blocking external-volume or payload reads.
struct Entry {
    id: String,
    ext: String,
    /// An item in the Trash keeps its vector, but isn't indexed while there.
    trashed: bool,
    thumbnail: PathBuf,
    source: AiSourceSnapshot,
}

struct Worker {
    options: Options,
    status: Arc<Mutex<Status>>,
    library: Option<Shared>,
    root: Option<PathBuf>,
    index: Option<Index>,
    embedder: Option<Box<dyn Embedder>>,
    queue: VecDeque<Job>,
    enabled: bool,
    paused: bool,
    installed: bool,
    /// The model failed; nothing more is tried until the user acts.
    failure: Option<String>,
    loading: bool,
    device: Option<String>,
    total: usize,
    trashed: usize,
    skipped: usize,
    stale: usize,
    unavailable: usize,
    /// Item IDs that are currently offline/missing/permission-denied/transient
    /// so we can detect reconnects incrementally without full reindex.
    unavailable_ids: HashSet<String>,
    /// Refresh stamps that failed decode while preserving an old vector as stale,
    /// preventing tight retry loops until the file stamp changes again.
    failed_refresh_stamps: HashMap<String, u64>,
    /// What the library looked like when last compared.
    signature: Option<u64>,
    last_check: Option<Instant>,
    last_full_check: Option<Instant>,
    last_used: Instant,
    /// Not before this, because of the throttle.
    next_batch: Instant,
    rate: f32,
}

impl Worker {
    fn new(options: Options, status: Arc<Mutex<Status>>) -> Worker {
        let installed = (options.is_installed)();
        Worker {
            options,
            status,
            library: None,
            root: None,
            index: None,
            embedder: None,
            queue: VecDeque::new(),
            enabled: false,
            paused: false,
            installed,
            failure: None,
            loading: false,
            device: None,
            total: 0,
            trashed: 0,
            skipped: 0,
            stale: 0,
            unavailable: 0,
            unavailable_ids: HashSet::new(),
            failed_refresh_stamps: HashMap::new(),
            signature: None,
            last_check: None,
            last_full_check: None,
            last_used: Instant::now(),
            next_batch: Instant::now(),
            rate: 0.0,
        }
    }

    fn run(mut self, rx: &Receiver<Msg>) {
        self.publish();
        loop {
            let now = Instant::now();
            let wait = if self.active() && self.last_check.is_none() {
                Duration::ZERO
            } else if self.wants_batch() {
                self.next_batch.saturating_duration_since(now)
            } else {
                CHECK_EVERY
            };
            // Everything waiting goes first, so a search never waits for
            // more than the batch under way.
            let mut message = rx.recv_timeout(wait);
            loop {
                match message {
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                        if let Some(index) = self.index.as_mut() {
                            let _ = index.flush();
                        }
                        return;
                    }
                    Ok(msg) => self.handle(msg),
                    Err(RecvTimeoutError::Timeout) => break,
                }
                message = rx.try_recv().map_err(|err| match err {
                    TryRecvError::Empty => RecvTimeoutError::Timeout,
                    TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                });
            }
            self.check_library();
            if self.wants_batch() && Instant::now() >= self.next_batch {
                self.batch();
            }
            self.unload_when_idle();
        }
    }

    fn active(&self) -> bool {
        self.enabled && self.installed
    }

    fn wants_batch(&self) -> bool {
        self.active() && !self.paused && self.failure.is_none() && !self.queue.is_empty()
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Library(library) => {
                if let Some(index) = self.index.as_mut() {
                    let _ = index.flush();
                }
                self.index = None;
                self.root = library.as_ref().map(|library| lock(library).root().to_path_buf());
                self.library = library;
                self.queue.clear();
                self.total = 0;
                self.trashed = 0;
                self.skipped = 0;
                self.stale = 0;
                self.unavailable = 0;
                self.unavailable_ids.clear();
                self.failed_refresh_stamps.clear();
                self.force_check();
            }
            Msg::Enabled(on) => {
                self.enabled = on;
                self.failure = None;
                if !on {
                    self.embedder = None;
                    self.device = None;
                }
                self.force_check();
            }
            Msg::Paused(paused) => self.paused = paused,
            Msg::ModelChanged => {
                self.installed = (self.options.is_installed)();
                self.embedder = None;
                self.device = None;
                self.failure = None;
                self.force_check();
            }
            Msg::Rebuild => {
                if let Some(index) = self.open_index() {
                    let _ = index.reset();
                }
                self.failed_refresh_stamps.clear();
                self.unavailable_ids.clear();
                self.failure = None;
                self.force_check();
            }
            Msg::Refresh => {
                self.failed_refresh_stamps.clear();
                self.failure = None;
                self.force_check();
            }
            Msg::ForgetIndexes => {
                self.index = None;
                self.queue.clear();
                self.unavailable_ids.clear();
                self.failed_refresh_stamps.clear();
                self.stale = 0;
                self.unavailable = 0;
                if let Ok(files) = std::fs::read_dir(&self.options.index_dir) {
                    for file in files.flatten() {
                        let _ = std::fs::remove_file(file.path());
                    }
                }
                self.force_check();
            }
            Msg::RelocateIndex(old_root, new_root) => {
                if let Some(index) = self.index.as_mut() {
                    let _ = index.flush();
                }
                self.index = None;
                self.queue.clear();
                let _ = crate::store::relocate(&self.options.index_dir, &old_root, &new_root, &self.options.model_id, self.options.dim);
                self.force_check();
            }
            Msg::Search(query, limit, reply) => {
                let _ = reply.send(self.search(query, limit));
                self.last_used = Instant::now();
                (self.options.notify)();
            }
            Msg::Stop => {}
        }
        self.publish();
    }

    /// Compare with the library at the next chance, whatever its signature.
    fn force_check(&mut self) {
        self.signature = None;
        self.last_check = None;
        self.last_full_check = None;
    }

    fn open_index(&mut self) -> Option<&mut Index> {
        if self.index.is_none() {
            let root = lock(self.library.as_ref()?).root().to_path_buf();
            match Index::open(&self.options.index_dir, &root, &self.options.model_id, self.options.dim) {
                Ok(index) => self.index = Some(index),
                Err(err) => self.failure = Some(err.to_string()),
            }
        }
        self.index.as_mut()
    }

    /// Looks for changes in the library and makes the queue from them.
    fn check_library(&mut self) {
        if !self.active() {
            return;
        }
        let now = Instant::now();
        if self.last_check.is_some_and(|at| now.duration_since(at) < CHECK_EVERY) {
            return;
        }
        self.last_check = Some(now);
        let Some(library) = self.library.clone() else { return };
        let full = self.last_full_check.is_none_or(|at| now.duration_since(at) >= FULL_CHECK_EVERY);
        let has_watch = !self.unavailable_ids.is_empty() || self.index.as_ref().is_some_and(|i| i.stale_count() > 0);
        // Only the list is read under the lock.
        let (signature, entries) = {
            let lib = lock(&library);
            if self.index.as_ref().is_some_and(|index| !index.is_for(lib.root())) {
                self.index = None;
                self.signature = None;
            }
            let signature = signature(&lib);
            if !full && !has_watch && self.signature == Some(signature) {
                return;
            }
            (signature, entries(&lib))
        };
        self.signature = Some(signature);
        self.last_full_check = Some(now);
        self.compare(&entries);
        self.publish();
        (self.options.notify)();
    }

    /// Queues what changed and forgets what's gone, while strictly preserving
    /// embeddings for unavailable/offline/missing/invalid/permission-denied sources.
    fn compare(&mut self, entries: &[Entry]) {
        self.unavailable_ids.clear();
        let mut new_unavail = Vec::new();
        let mut unavailable = 0usize;
        let mut failed_stamps = std::mem::take(&mut self.failed_refresh_stamps);
        let Some(index) = self.open_index() else {
            self.failed_refresh_stamps = failed_stamps;
            return;
        };
        let mut queue = VecDeque::new();
        let mut live = HashSet::with_capacity(entries.len());
        let (mut total, mut trashed, mut skipped) = (0, 0, 0);
        for entry in entries {
            live.insert(entry.id.as_str());
            if entry.trashed {
                trashed += 1;
                continue;
            }
            total += 1;
            match evaluate_ai_source_snapshot(&entry.source) {
                AiSourceCondition::Available(src_path) => {
                    let has_thumbnail = entry.thumbnail.is_file();
                    let is_linked = entry.source.is_linked_hint();
                    let stamp = compute_source_stamp(
                        has_thumbnail,
                        &entry.thumbnail,
                        is_linked,
                        &src_path,
                        &entry.ext,
                    );
                    let existing_vec = index.vector(&entry.id).is_some();
                    if index.stamp(&entry.id) == Some(stamp) {
                        index.clear_stale(&entry.id);
                        failed_stamps.remove(&entry.id);
                        if !existing_vec {
                            skipped += 1;
                        }
                    } else if existing_vec && failed_stamps.get(&entry.id) == Some(&stamp) {
                        index.mark_stale(&entry.id);
                    } else if !has_thumbnail && !thumbnail::is_supported(&entry.ext) {
                        if existing_vec {
                            index.mark_stale(&entry.id);
                            failed_stamps.insert(entry.id.clone(), stamp);
                        } else {
                            let _ = index.skip(&entry.id, stamp);
                            skipped += 1;
                        }
                    } else {
                        let prefer_source = is_linked
                            && has_thumbnail
                            && existing_vec
                            && thumbnail::is_supported(&entry.ext);
                        let picture = if !has_thumbnail || prefer_source {
                            src_path
                        } else {
                            entry.thumbnail.clone()
                        };
                        let use_svg_raster = (!has_thumbnail || prefer_source)
                            && entry.ext.eq_ignore_ascii_case("svg");
                        queue.push_back(Job {
                            id: entry.id.clone(),
                            stamp,
                            picture,
                            source: entry.source.clone(),
                            use_svg_raster,
                        });
                    }
                }
                AiSourceCondition::Offline
                | AiSourceCondition::Missing
                | AiSourceCondition::InvalidReference
                | AiSourceCondition::PermissionDenied
                | AiSourceCondition::TemporarilyUnavailable => {
                    // Never erase existing embeddings or emit SKIPPED for unavailable sources.
                    new_unavail.push(entry.id.clone());
                    unavailable += 1;
                }
            }
        }
        for id in index.known_ids() {
            if !live.contains(id.as_str()) {
                let _ = index.remove(&id);
                failed_stamps.remove(&id);
            }
        }
        let _ = index.flush();
        let _ = index.compact();
        let stale_count = index.stale_count();
        self.stale = stale_count;
        self.unavailable = unavailable;
        self.unavailable_ids.extend(new_unavail);
        self.failed_refresh_stamps = failed_stamps;
        self.queue = queue;
        self.total = total;
        self.trashed = trashed;
        self.skipped = skipped;
    }

    /// Reads a batch of pictures and stores what the model makes of them.
    fn batch(&mut self) {
        if self.ensure_embedder().is_err() {
            return;
        }
        let started = Instant::now();
        let count = self.queue.len().min(BATCH);
        let jobs: Vec<Job> = self.queue.drain(..count).collect();
        let mut pictures = Vec::with_capacity(count);
        let mut good = Vec::with_capacity(count);
        let mut decode_failed = Vec::new();
        let mut unavailable_during_batch = Vec::new();
        for job in jobs {
            // A damaged file that panics a decoder is one unreadable item,
            // not the end of the worker.
            match std::panic::catch_unwind(|| decode_for_index(&job.picture, job.use_svg_raster)) {
                Ok(Ok(picture)) => {
                    pictures.push(picture);
                    good.push(job);
                }
                _ => {
                    match evaluate_ai_source_snapshot(&job.source) {
                        AiSourceCondition::Available(_) => decode_failed.push(job),
                        AiSourceCondition::Offline
                        | AiSourceCondition::Missing
                        | AiSourceCondition::InvalidReference
                        | AiSourceCondition::PermissionDenied
                        | AiSourceCondition::TemporarilyUnavailable => unavailable_during_batch.push(job),
                    }
                }
            }
        }
        let vectors = match self.embedder.as_mut() {
            Some(embedder) if !pictures.is_empty() => embedder.embed_images(&pictures),
            _ => Ok(Vec::new()),
        };
        drop(pictures);
        let elapsed = started.elapsed();
        match vectors {
            Ok(vectors) => {
                let mut newly_skipped = 0usize;
                let mut failed_stamps = std::mem::take(&mut self.failed_refresh_stamps);
                let mut new_unavail = Vec::new();
                let mut new_stale = 0usize;
                if let Some(index) = self.open_index() {
                    for (job, vector) in good.iter().zip(&vectors) {
                        failed_stamps.remove(&job.id);
                        let _ = index.put(&job.id, job.stamp, vector);
                    }
                    for job in &decode_failed {
                        if index.vector(&job.id).is_some() {
                            index.mark_stale(&job.id);
                            failed_stamps.insert(job.id.clone(), job.stamp);
                        } else {
                            let _ = index.skip(&job.id, job.stamp);
                            newly_skipped += 1;
                        }
                    }
                    for job in &unavailable_during_batch {
                        new_unavail.push(job.id.clone());
                        if index.vector(&job.id).is_some() {
                            index.mark_stale(&job.id);
                        }
                    }
                    let _ = index.flush();
                    new_stale = index.stale_count();
                }
                self.failed_refresh_stamps = failed_stamps;
                for id in new_unavail {
                    if self.unavailable_ids.insert(id) {
                        self.unavailable += 1;
                    }
                }
                self.stale = new_stale;
                self.skipped += newly_skipped;
                self.rate = count as f32 / elapsed.as_secs_f32().max(1e-3);
            }
            Err(err) => {
                let mut new_stale = self.stale;
                let mut failed_stamps = std::mem::take(&mut self.failed_refresh_stamps);
                if let Some(index) = self.open_index() {
                    for job in good.iter().chain(decode_failed.iter()).chain(unavailable_during_batch.iter()) {
                        if index.vector(&job.id).is_some() {
                            index.mark_stale(&job.id);
                            failed_stamps.insert(job.id.clone(), job.stamp);
                        }
                    }
                    new_stale = index.stale_count();
                }
                self.failed_refresh_stamps = failed_stamps;
                self.stale = new_stale;
                self.failure = Some(err);
                self.queue.clear();
            }
        }
        self.last_used = Instant::now();
        self.next_batch = Instant::now() + if self.options.throttle { elapsed } else { Duration::ZERO };
        self.publish();
        (self.options.notify)();
    }

    fn ensure_embedder(&mut self) -> Result<&mut Box<dyn Embedder>, String> {
        if self.embedder.is_none() {
            self.loading = true;
            self.publish();
            (self.options.notify)();
            let loaded = (self.options.loader)();
            self.loading = false;
            match loaded {
                Ok(embedder) if embedder.dim() == self.options.dim => {
                    self.device = Some(embedder.device());
                    self.embedder = Some(embedder);
                }
                Ok(_) => self.failure = Some("the model's vectors have an unexpected length".to_string()),
                Err(err) => self.failure = Some(err),
            }
            self.publish();
        }
        match self.embedder.as_mut() {
            Some(embedder) => Ok(embedder),
            None => Err(self.failure.clone().unwrap_or_default()),
        }
    }

    fn unload_when_idle(&mut self) {
        if self.embedder.is_some() && self.queue.is_empty() && self.last_used.elapsed() > UNLOAD_AFTER {
            self.embedder = None;
        }
    }

    fn search(&mut self, query: Query, limit: usize) -> Result<Vec<Hit>, SearchError> {
        if !self.active() {
            return Err(SearchError::NotReady);
        }
        let Some(library) = self.library.clone() else { return Err(SearchError::NoLibrary) };
        let mut image_query = true;
        let vector = match query {
            Query::Text(text) => {
                image_query = false;
                if text.trim().is_empty() {
                    return Ok(Vec::new());
                }
                self.embedder_for_search()?.embed_text(&text).map_err(SearchError::Failed)?
            }
            Query::Image(image) => self.embed_one(&image)?,
            Query::ImageFile(path) => {
                let image = thumbnail::decode(&path).map_err(|e| SearchError::BadImage(e.to_string()))?;
                self.embed_one(&image)?
            }
            Query::ImageBytes(bytes) => {
                let image = image::load_from_memory(&bytes).map_err(|e| SearchError::BadImage(e.to_string()))?;
                self.embed_one(&image)?
            }
            Query::Item(id) => {
                let known = self.open_index().and_then(|index| index.vector(&id).map(<[f32]>::to_vec));
                match known {
                    Some(vector) => vector,
                    None => {
                        let entry = {
                            let lib = lock(&library);
                            let item = lib.item(&id).ok_or(SearchError::NoItem)?;
                            entry_of(&lib, item)
                        };
                        let has_thumbnail = entry.thumbnail.is_file();
                        let picture = if has_thumbnail {
                            entry.thumbnail
                        } else {
                            match evaluate_ai_source_snapshot(&entry.source) {
                                AiSourceCondition::Available(p) => p,
                                other => return Err(SearchError::BadImage(format!("source unavailable ({other:?})"))),
                            }
                        };
                        let use_svg_raster = !has_thumbnail && entry.ext.eq_ignore_ascii_case("svg");
                        let image = decode_for_index(&picture, use_svg_raster).map_err(|e| SearchError::BadImage(e.to_string()))?;
                        self.embed_one(&image)?
                    }
                }
            }
        };
        let want = limit.saturating_add(self.trashed);
        let Some(index) = self.open_index() else { return Err(SearchError::Failed("no index".to_string())) };
        let mut hits = index.search(&vector, want, if image_query { IMAGE_MIN } else { TEXT_MIN });
        if !image_query && let Some(best) = hits.first().map(|h| h.score) {
            hits.retain(|h| h.score >= best - TEXT_SPREAD);
        }
        // Items in the Trash keep their vectors but aren't results.
        let lib = lock(&library);
        hits.retain(|h| lib.item(&h.id).is_some_and(|i| !i.is_deleted));
        hits.truncate(limit);
        Ok(hits)
    }

    fn embed_one(&mut self, image: &DynamicImage) -> Result<Vec<f32>, SearchError> {
        let embedder = self.embedder_for_search()?;
        let mut vectors = embedder.embed_images(std::slice::from_ref(image)).map_err(SearchError::Failed)?;
        vectors.pop().ok_or_else(|| SearchError::Failed("the model gave no result".to_string()))
    }

    fn embedder_for_search(&mut self) -> Result<&mut Box<dyn Embedder>, SearchError> {
        self.ensure_embedder().map_err(SearchError::Failed)
    }

    fn publish(&self) {
        let queued = self.queue.len();
        let phase = if !self.enabled {
            Phase::Off
        } else if !self.installed {
            Phase::NoModel
        } else if self.failure.is_some() {
            Phase::Failed
        } else if self.loading {
            Phase::Loading
        } else if self.library.is_some() && self.signature.is_none() {
            // Not compared with the library yet, so what's left isn't known.
            Phase::Indexing
        } else if queued == 0 {
            Phase::Ready
        } else if self.paused {
            Phase::Paused
        } else {
            Phase::Indexing
        };
        let status = Status {
            phase,
            total: self.total,
            done: self.total.saturating_sub(queued),
            skipped: self.skipped,
            stale: self.stale,
            unavailable: self.unavailable,
            device: self.device.clone(),
            error: self.failure.clone(),
            rate: self.rate,
            root: self.root.clone(),
        };
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = status;
    }
}

fn lock(library: &Shared) -> std::sync::MutexGuard<'_, Library> {
    library.lock().unwrap_or_else(|p| p.into_inner())
}

/// A cheap fingerprint of what the index cares about. It changes when items
/// come, go, change or move to or from the Trash.
fn signature(lib: &Library) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(lib.items().len() as u64).to_le_bytes());
    for item in lib.items() {
        hasher.update(item.id.as_bytes());
        hasher.update(&item.last_modified.to_le_bytes());
        hasher.update(&item.modification_time.to_le_bytes());
        hasher.update(&[u8::from(item.is_deleted)]);
    }
    let hash = hasher.finalize();
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&hash.as_bytes()[..8]);
    u64::from_le_bytes(bytes)
}

fn entries(lib: &Library) -> Vec<Entry> {
    lib.items().iter().map(|item| entry_of(lib, item)).collect()
}

fn entry_of(lib: &Library, item: &falcon_library::model::Item) -> Entry {
    Entry {
        id: item.id.clone(),
        ext: item.ext.clone(),
        trashed: item.is_deleted,
        thumbnail: item.info_dir(lib.root()).join(item.thumbnail_name()),
        source: lib.ai_source_snapshot(item),
    }
}

fn compute_source_stamp(
    has_thumbnail: bool,
    thumbnail: &Path,
    is_linked: bool,
    src_path: &Path,
    ext: &str,
) -> u64 {
    let raw_src_stamp = file_stamp(src_path);
    let src_stamp = if !has_thumbnail && ext.eq_ignore_ascii_case("svg") && raw_src_stamp != 0 {
        raw_src_stamp ^ SVG_RASTER_STAMP_TAG
    } else {
        raw_src_stamp
    };
    if has_thumbnail {
        let thumb_stamp = file_stamp(thumbnail);
        if is_linked {
            thumb_stamp.rotate_left(17) ^ src_stamp
        } else {
            thumb_stamp
        }
    } else {
        src_stamp
    }
}

fn decode_for_index(path: &Path, use_svg_raster: bool) -> Result<DynamicImage, thumbnail::ThumbError> {
    if use_svg_raster {
        return thumbnail::render_svg_bounded(path, thumbnail::max_side("svg"), MAX_AI_SVG_BYTES, MAX_AI_SVG_DIM);
    }
    thumbnail::decode(path)
}

/// What says a file changed: its modified time and size.
fn file_stamp(path: &Path) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else { return 0 };
    let time = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
    time ^ meta.len().rotate_left(48)
}
