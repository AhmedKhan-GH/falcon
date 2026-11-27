//! Folder synchronization that holds the library lock only briefly.
//!
//! [`Library::sync_watched_folder`] does everything under `&mut Library`,
//! which is fine for tests and small folders but would freeze the app for a
//! large one. [`sync_shared`] takes the shared library instead and splits a
//! pass into phases, so that everything slow happens with the lock released:
//!
//! 1. *begin* (short lock): check the folder, mark it `Syncing`, copy out what
//!    the pass needs to know (linked sources, provenance, item slots);
//! 2. *scan* (no lock): walk the source tree;
//! 3. *folders* (short locks): make the missing library folders;
//! 4. *plan* (no lock): decide, per file, import / refresh / skip / conflict,
//!    reading sidecars, canonicalizing paths and hashing fingerprints;
//! 5. *apply*: staging (decoding, thumbnails, metadata) on a bounded set of
//!    worker threads with no lock, then one short lock per commit;
//! 6. *finish* (short lock): save provenance and the final status.
//!
//! The same Copy and Link rules as the plain pass apply; nothing outside the
//! library is ever written, and a managed original is never overwritten. A
//! cancel is noticed between files, so it lands after at most one file's staging.
//! Any failure leaves the folder `Error`, a cancel leaves it `Idle`, and a
//! later pass continues without duplicating anything.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::fault::{self, FaultPoint};
use crate::folder_sync::{SyncStats, find_folder_in, is_sync_skipped, mtime_ms, now_ms, source_key};
use crate::import::{ImportOptions, StagedThumbnail, stage, stage_deferred_thumbnail};
use crate::library::Library;
use crate::linked::{self, FileIdentity};
use crate::model::Item;
use crate::storage_mode::{StorageMode, SyncSource, SyncStatus, WatchedFolder};

/// Where a pass is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncPhase {
    #[default]
    Starting,
    Scanning,
    Folders,
    Planning,
    Importing,
    Refreshing,
    Finishing,
}

impl SyncPhase {
    /// Short label for the interface.
    pub fn label(self) -> &'static str {
        match self {
            SyncPhase::Starting => "Starting",
            SyncPhase::Scanning => "Scanning",
            SyncPhase::Folders => "Preparing folders",
            SyncPhase::Planning => "Comparing",
            SyncPhase::Importing => "Importing",
            SyncPhase::Refreshing => "Refreshing",
            SyncPhase::Finishing => "Finishing",
        }
    }
}

/// A snapshot of a pass for the progress display.
#[derive(Debug, Clone, Default)]
pub struct SyncProgress {
    pub phase: SyncPhase,
    /// Files to import or refresh (known once planning is done).
    pub total: usize,
    pub done: usize,
    pub added: usize,
    pub modified: usize,
    pub conflicts: usize,
    /// The file being worked on.
    pub current: String,
}

/// What a pass is allowed to use and how it reports.
pub struct SyncControl<'a> {
    /// Checked between files; set it to stop the pass.
    pub cancel: &'a AtomicBool,
    /// Called after each step, from any thread. Keep it cheap.
    pub progress: &'a (dyn Fn(&SyncProgress) + Sync),
    /// Staging threads (decode, thumbnails, metadata); 1 runs inline.
    pub threads: usize,
    /// Link imports skip the thumbnail and leave it to a background worker
    /// ([`SyncStats::pending_thumbnail_ids`] lists them).
    pub defer_thumbnails: bool,
    /// After each commit under the lock the committer rests for this fraction
    /// of the time the commit took (0 for none), so the lock is free for the
    /// interface for at least that share of the time. 1.0 leaves it half the time.
    pub pace: f32,
    /// Files modified within this many milliseconds are left for a later pass
    /// (they may still be being written). 0 trusts every file.
    pub settle_ms: u64,
    /// Target length of one lock hold: staged files are committed in groups
    /// that fit it (1 to 8 files), with one flush per group.
    pub hold_budget_ms: u64,
    /// Least rest after a hold when `pace` is above 0.
    pub min_rest_ms: u64,
}

fn lock(m: &Mutex<Library>) -> MutexGuard<'_, Library> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Changes the watched folder `id` if it still exists (a removed folder is
/// never brought back by a pass that was running when it was removed).
/// `f` returns whether it changed anything; nothing is written otherwise.
fn update_wf(lib: &mut Library, id: &str, f: impl FnOnce(&mut WatchedFolder) -> bool) -> Result<bool> {
    let Some(mut wf) = lib.config().find_watched_folder(id) else { return Ok(false) };
    if !f(&mut wf) {
        return Ok(false);
    }
    lib.upsert_watched_folder(wf)?;
    Ok(true)
}

/// Refuses to commit into a library that was moved, closed and reopened
/// elsewhere, or replaced, since the pass began.
fn check_root(lib: &Library, snap: &Snapshot) -> Result<()> {
    if lib.root() == snap.root {
        Ok(())
    } else {
        Err(Error::Refused(format!("the library moved from {} to {} during the sync", snap.root.display(), lib.root().display())))
    }
}

/// Writes `Syncing` once, just before the pass first changes anything.
struct Marker<'a> {
    shared: &'a Mutex<Library>,
    snap: &'a Snapshot,
    done: AtomicBool,
}

impl Marker<'_> {
    fn mark(&self) -> Result<()> {
        if self.done.swap(true, Ordering::Relaxed) {
            return Ok(());
        }
        let mut lib = lock(self.shared);
        check_root(&lib, self.snap)?;
        update_wf(&mut lib, &self.snap.wf.id, |wf| {
            wf.status = SyncStatus::Syncing;
            wf.last_error = None;
            true
        })?;
        drop(lib);
        fault::check(FaultPoint::SyncAfterMarkedSyncing, &self.snap.wf.source_path)
    }

    fn marked(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
}

/// Everything the pass needs from the library, copied out under one short lock.
struct Snapshot {
    wf: WatchedFolder,
    root: PathBuf,
    /// Linked item ids, to read their sidecars with the lock released.
    linked_ids: Vec<String>,
    provenance: BTreeMap<String, SyncSource>,
    /// Non-trashed items, and their id by (folder, name, extension).
    alive: HashSet<String>,
    slots: HashMap<(String, String, String), Vec<String>>,
}

enum Begin {
    Offline(SyncStats),
    Ready(Box<Snapshot>),
}

fn begin(lib: &mut Library, wf_id: &str) -> Result<Begin> {
    let mut wf = lib
        .config()
        .find_watched_folder(wf_id)
        .ok_or_else(|| Error::Refused(format!("no synchronized folder {wf_id}")))?;
    if !wf.source_path.exists() || !wf.source_path.is_dir() {
        wf.status = SyncStatus::Offline;
        wf.last_error = Some("Source folder is offline or unreachable".into());
        let _ = lib.upsert_watched_folder(wf);
        return Ok(Begin::Offline(SyncStats { offline: true, ..Default::default() }));
    }
    if find_folder_in(&lib.metadata.folders, &wf.folder_id).is_none() {
        wf.status = SyncStatus::Error;
        wf.last_error = Some(format!("Target library folder {} not found", wf.folder_id));
        let _ = lib.upsert_watched_folder(wf.clone());
        return Err(Error::Refused(format!("Target library folder {} not found", wf.folder_id)));
    }
    // `Syncing` is written only when the pass has something to do (see
    // `Marker`), so an unchanged folder costs no writes.
    let prefix = format!("{}|", wf.id);
    let provenance = lib.config().sync_sources.iter().filter(|(k, _)| k.starts_with(&prefix)).map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut alive = HashSet::new();
    let mut slots: HashMap<(String, String, String), Vec<String>> = HashMap::new();
    // Only Copy mode asks which items exist and where; Link mode reads sidecars.
    for item in lib.items().iter().filter(|_| wf.storage_mode == StorageMode::Copy) {
        if item.is_deleted {
            continue;
        }
        alive.insert(item.id.clone());
        for f in &item.folders {
            slots.entry((f.clone(), item.name.clone(), item.ext.to_lowercase())).or_default().push(item.id.clone());
        }
    }
    Ok(Begin::Ready(Box::new(Snapshot {
        root: lib.root().to_path_buf(),
        linked_ids: lib.config().linked_ids.iter().cloned().collect(),
        provenance,
        alive,
        slots,
        wf,
    })))
}

struct ScanFile {
    path: PathBuf,
    rel_dir: PathBuf,
    size: u64,
    mtime: u64,
}

struct Scan {
    /// Relative directories, parents before children (the root is implicit).
    dirs: Vec<PathBuf>,
    files: Vec<ScanFile>,
    skipped: usize,
    /// Files changed too recently to be trusted as complete.
    unsettled: usize,
}

/// A file modified more recently than this may still be being written, and is
/// left for the next pass.
pub const SETTLE_MS: u64 = 2_000;

/// Walks the source tree without the lock. Links are not followed.
fn scan_tree(wf: &WatchedFolder, ctl: &SyncControl<'_>) -> Scan {
    let settle = ctl.settle_ms;
    let mut scan = Scan { dirs: Vec::new(), files: Vec::new(), skipped: 0, unsettled: 0 };
    let now = now_ms();
    let mut queue = vec![PathBuf::new()];
    while let Some(rel) = queue.pop() {
        if ctl.cancel.load(Ordering::Relaxed) {
            break;
        }
        if !rel.as_os_str().is_empty() {
            scan.dirs.push(rel.clone());
        }
        let Ok(entries) = fs::read_dir(wf.source_path.join(&rel)) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if wf.recursive && !name.starts_with('.') {
                    queue.push(rel.join(&name));
                }
            } else if kind.is_file() {
                let Ok(meta) = entry.metadata() else { continue };
                if is_sync_skipped(&name, meta.len()) {
                    scan.skipped += 1;
                    continue;
                }
                let mtime = mtime_ms(&meta);
                if settle > 0 && now.saturating_sub(mtime) < settle && mtime <= now + 60_000 {
                    scan.unsettled += 1;
                    continue;
                }
                scan.files.push(ScanFile { path: entry.path(), rel_dir: rel.clone(), size: meta.len(), mtime });
            }
        }
    }
    scan.dirs.sort_by(|a, b| a.components().count().cmp(&b.components().count()).then_with(|| a.cmp(b)));
    scan.files.sort_by(|a, b| a.path.cmp(&b.path));
    scan
}

/// Makes the library folders for `dirs` (short lock per batch) and returns
/// relative directory -> library folder id.
fn ensure_folders(shared: &Mutex<Library>, snap: &Snapshot, marker: &Marker<'_>, dirs: &[PathBuf], ctl: &SyncControl<'_>) -> Result<HashMap<PathBuf, String>> {
    let mut map: HashMap<PathBuf, String> = HashMap::new();
    map.insert(PathBuf::new(), snap.wf.folder_id.clone());
    // One folder per lock: making a folder writes `metadata.json` to disk, so
    // a batch under one lock would hold it for many writes.
    for batch in dirs.chunks(1) {
        if ctl.cancel.load(Ordering::Relaxed) {
            break;
        }
        let started = Instant::now();
        let mut lib = lock(shared);
        for rel in batch {
            let parent_rel = rel.parent().unwrap_or(Path::new("")).to_path_buf();
            let parent_id = map.get(&parent_rel).cloned().unwrap_or_else(|| snap.wf.folder_id.clone());
            let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let existing = find_folder_in(&lib.metadata.folders, &parent_id)
                .and_then(|p| p.children.iter().find(|c| c.name == name))
                .map(|c| c.id.clone());
            let id = match existing {
                Some(id) => id,
                None => {
                    drop(lib);
                    marker.mark()?;
                    lib = lock(shared);
                    check_root(&lib, snap)?;
                    fault::check(FaultPoint::SyncBeforeFolderCreate, &snap.wf.source_path.join(rel))?;
                    lib.create_folder(Some(&parent_id), &name)?
                }
            };
            map.insert(rel.clone(), id);
        }
        drop(lib);
        if ctl.pace > 0.0 {
            std::thread::sleep(started.elapsed().mul_f32(ctl.pace));
        }
    }
    Ok(map)
}

enum Op {
    Import { path: PathBuf, folder_id: String, key: String },
    Refresh { id: String, source: PathBuf, old: FileIdentity },
}

#[derive(Default)]
struct Plan {
    ops: Vec<Op>,
    /// Provenance entries to record without importing (adopted or touched).
    provenance: Vec<(String, SyncSource)>,
    skipped: usize,
    conflicts: usize,
}

/// Decides what to do with every scanned file, with the lock released.
fn plan(snap: &Snapshot, scan: &Scan, dir_map: &HashMap<PathBuf, String>, ctl: &SyncControl<'_>) -> Plan {
    let mut plan = Plan { skipped: scan.skipped, ..Default::default() };
    let wf = &snap.wf;
    // Linked sources: canonical path -> (item id, identity on record).
    let mut linked: HashMap<PathBuf, (String, FileIdentity)> = HashMap::new();
    if wf.storage_mode == StorageMode::Link {
        for id in &snap.linked_ids {
            let sidecar = snap.root.join("images").join(format!("{id}.info")).join(linked::LINK_SIDECAR);
            if let Ok(Some(rec)) = linked::load_validated_sidecar_at(&sidecar, id, true, &snap.root) {
                let key = fs::canonicalize(&rec.source_path).unwrap_or(rec.source_path);
                linked.insert(key, (id.clone(), rec.identity));
            }
        }
    }
    let mut claimed: HashSet<String> = snap.provenance.values().map(|s| s.item_id.clone()).collect();
    for file in &scan.files {
        if ctl.cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(folder_id) = dir_map.get(&file.rel_dir) else {
            plan.skipped += 1;
            continue;
        };
        match wf.storage_mode {
            StorageMode::Link => {
                let canonical = fs::canonicalize(&file.path).unwrap_or_else(|_| file.path.clone());
                match linked.get(&canonical) {
                    Some((id, old)) => {
                        if old.size != file.size || old.mtime_ms != file.mtime {
                            plan.ops.push(Op::Refresh { id: id.clone(), source: canonical, old: old.clone() });
                        } else {
                            plan.skipped += 1;
                        }
                    }
                    None => plan.ops.push(Op::Import { path: file.path.clone(), folder_id: folder_id.clone(), key: String::new() }),
                }
            }
            StorageMode::Copy => {
                let key = source_key(wf, &file.path);
                match snap.provenance.get(&key) {
                    Some(prev) => {
                        if !snap.alive.contains(&prev.item_id) || (prev.size == file.size && prev.mtime_ms == file.mtime) {
                            plan.skipped += 1;
                        } else {
                            let fp = linked::compute_fast_fingerprint(&file.path, file.size).ok();
                            if prev.size == file.size && fp.is_some() && fp == prev.fingerprint {
                                plan.provenance.push((key, SyncSource { mtime_ms: file.mtime, ..prev.clone() }));
                                plan.skipped += 1;
                            } else {
                                plan.conflicts += 1;
                            }
                        }
                    }
                    None => {
                        let stem = file.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        let ext = file.path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                        let adopt = snap
                            .slots
                            .get(&(folder_id.clone(), stem, ext))
                            .and_then(|ids| ids.iter().find(|id| !claimed.contains(*id)))
                            .cloned();
                        if let Some(id) = adopt {
                            claimed.insert(id.clone());
                            let fingerprint = linked::compute_fast_fingerprint(&file.path, file.size).ok();
                            plan.provenance.push((key, SyncSource { item_id: id, size: file.size, mtime_ms: file.mtime, fingerprint }));
                            plan.skipped += 1;
                        } else {
                            plan.ops.push(Op::Import { path: file.path.clone(), folder_id: folder_id.clone(), key });
                        }
                    }
                }
            }
        }
    }
    plan
}

/// Produces `count` values on up to `threads` threads with no lock held, and
/// hands each to `consume` on the calling thread, which is the only one that
/// ever commits. At most `2 * threads` produced values wait at once, so memory
/// stays bounded. Stops handing out work after `halt` is set; values already
/// produced are dropped unconsumed once it is.
fn pipeline<T: Send>(
    count: usize,
    threads: usize,
    halt: &AtomicBool,
    produce: &(dyn Fn(usize) -> Option<T> + Sync),
    consume: &mut dyn FnMut(usize, Option<T>),
) {
    if threads <= 1 || count <= 1 {
        for i in 0..count {
            if halt.load(Ordering::Relaxed) {
                return;
            }
            let value = produce(i);
            consume(i, value);
        }
        return;
    }
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, Option<T>)>(threads * 2);
    std::thread::scope(|scope| {
        for _ in 0..threads.min(count) {
            let tx = tx.clone();
            let next = &next;
            scope.spawn(move || {
                loop {
                    if halt.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= count {
                        return;
                    }
                    if tx.send((i, produce(i))).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx);
        for (i, value) in rx {
            if halt.load(Ordering::Relaxed) {
                continue; // drain: dropping the value cleans up its staging
            }
            consume(i, value);
        }
    });
}

/// Source-derived keys of `Item::extra` a refresh may replace.
const REFRESHED_EXTRA: [&str; 5] = ["rawMetas", "resolutionWidth", "resolutionHeight", "duration", "bpm"];

struct Prepared {
    id: String,
    /// The identity on record when the pass was planned.
    old: FileIdentity,
    source: PathBuf,
    size: u64,
    mtime: u64,
    extra: Vec<(String, Option<serde_json::Value>)>,
    staged: Option<StagedThumbnail>,
    draft_identity: FileIdentity,
    volume: linked::VolumeIdentity,
    bookmark: Option<String>,
}

/// Reads the changed source and builds its thumbnail and metadata, with no lock held.
fn prepare_refresh(root: &Path, item: &Item, source: &Path, old: &FileIdentity) -> Option<Prepared> {
    let meta = fs::metadata(source).ok()?;
    let mut updated = item.clone();
    updated.size = meta.len();
    updated.mtime = mtime_ms(&meta);
    crate::import::refresh_source_metadata(&mut updated, source);
    let staged = stage_deferred_thumbnail(&updated, source, &item.info_dir(root), true).ok().flatten();
    // Whatever was read must still describe the file.
    let after = fs::metadata(source).ok()?;
    if after.len() != meta.len() || mtime_ms(&after) != mtime_ms(&meta) {
        return None;
    }
    let draft = linked::build_draft(source, root, &after, now_ms());
    let extra = REFRESHED_EXTRA.iter().map(|k| (k.to_string(), updated.extra.get(*k).cloned())).collect();
    // Palette and size come from `staged`; dimensions too. Nothing else of the item is taken.
    Some(Prepared {
        id: item.id.clone(),
        old: old.clone(),
        source: source.to_path_buf(),
        size: meta.len(),
        mtime: mtime_ms(&meta),
        extra,
        staged,
        draft_identity: draft.identity,
        volume: draft.volume,
        bookmark: draft.bookmark_base64,
    })
}

/// Applies a prepared refresh under a short lock. `false` when it was stale:
/// the record or the source changed since planning, so the next pass takes it.
fn commit_refresh(lib: &mut Library, p: Prepared) -> Result<bool> {
    fault::check(FaultPoint::SyncBeforeRefresh, &p.source)?;
    let Some(record) = lib.link_record(&p.id)? else { return Ok(false) };
    if record.identity.size != p.old.size || record.identity.mtime_ms != p.old.mtime_ms {
        return Ok(false);
    }
    match fs::metadata(&p.source) {
        Ok(m) if m.len() == p.size && mtime_ms(&m) == p.mtime => {}
        _ => return Ok(false),
    }
    let Some(current) = lib.item(&p.id).cloned() else { return Ok(false) };
    let thumb = current.info_dir(lib.root()).join(current.thumbnail_name());
    let drawn = match p.staged {
        Some(staged) => lib.commit_deferred_thumbnail(&p.id, staged).is_ok(),
        None => false,
    };
    let mut item = lib.item(&p.id).cloned().unwrap_or(current);
    if !drawn {
        // Nothing could be drawn from the new content, and the old picture
        // must not stay up for it; the thumbnail worker tries again.
        let _ = fs::remove_file(&thumb);
        item.no_thumbnail = None;
    }
    item.size = p.size;
    item.mtime = p.mtime;
    item.last_modified = now_ms().max(item.last_modified);
    for (k, v) in p.extra {
        match v {
            Some(v) => {
                item.extra.insert(k, v);
            }
            None => {
                item.extra.shift_remove(&k);
            }
        }
    }
    lib.register_item(item.clone())?;
    fault::check(FaultPoint::SyncBeforeRefreshSidecar, &p.source)?;
    let mut next = record;
    next.identity = p.draft_identity;
    next.volume = p.volume;
    if p.bookmark.is_some() {
        next.bookmark_base64 = p.bookmark;
    }
    next.last_verified_at_ms = now_ms();
    linked::write_sidecar(&item, lib.root(), &next)?;
    Ok(true)
}

#[derive(Default)]
struct Outcome {
    added_ids: Vec<String>,
    refreshed_ids: Vec<String>,
    pending_thumbnails: Vec<String>,
    provenance: Vec<(String, SyncSource)>,
    skipped: usize,
    first_error: Option<Error>,
}

/// Synchronizes the watched folder `wf_id` of the shared library, with the
/// library locked only briefly. See the module documentation.
pub fn sync_shared(shared: &Mutex<Library>, wf_id: &str, ctl: &SyncControl<'_>) -> Result<SyncStats> {
    let result = run(shared, wf_id, ctl);
    if let Err(e) = &result {
        let mut lib = lock(shared);
        if let Some(mut wf) = lib.config().find_watched_folder(wf_id)
            && wf.status != SyncStatus::Offline
        {
            wf.status = SyncStatus::Error;
            wf.last_error = Some(e.to_string());
            let _ = lib.upsert_watched_folder(wf);
        }
    }
    result
}

fn run(shared: &Mutex<Library>, wf_id: &str, ctl: &SyncControl<'_>) -> Result<SyncStats> {
    let report = |p: &SyncProgress| (ctl.progress)(p);
    let mut progress = SyncProgress::default();
    report(&progress);

    let snap = match begin(&mut lock(shared), wf_id)? {
        Begin::Offline(stats) => return Ok(stats),
        Begin::Ready(s) => s,
    };
    let marker = Marker { shared, snap: &snap, done: AtomicBool::new(false) };

    progress.phase = SyncPhase::Scanning;
    report(&progress);
    let scan = scan_tree(&snap.wf, ctl);

    progress.phase = SyncPhase::Folders;
    report(&progress);
    let dir_map = ensure_folders(shared, &snap, &marker, &scan.dirs, ctl)?;

    progress.phase = SyncPhase::Planning;
    report(&progress);
    let plan = plan(&snap, &scan, &dir_map, ctl);
    let mut stats = SyncStats { skipped: plan.skipped, conflicts: plan.conflicts, unsettled: scan.unsettled, ..Default::default() };
    let mut provenance = plan.provenance;

    let mut imports: Vec<(&PathBuf, &String, &String)> = Vec::new();
    let mut refreshes: Vec<(&String, &PathBuf, &FileIdentity)> = Vec::new();
    for op in &plan.ops {
        match op {
            Op::Import { path, folder_id, key } => imports.push((path, folder_id, key)),
            Op::Refresh { id, source, old } => refreshes.push((id, source, old)),
        }
    }
    progress.total = imports.len() + refreshes.len();
    progress.conflicts = plan.conflicts;

    let outcome = Mutex::new(Outcome::default());
    let counters = Mutex::new(progress.clone());
    let publish = |f: &dyn Fn(&mut SyncProgress)| {
        let mut p = counters.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut p);
        report(&p);
    };
    let stop = AtomicBool::new(false);
    let cancelled = || ctl.cancel.load(Ordering::Relaxed);
    let stop_all = || cancelled() || stop.load(Ordering::Relaxed);
    // `for_each_parallel` watches one flag, so fold both into it.
    let halt = AtomicBool::new(false);
    let sync_halt = || {
        if stop_all() {
            halt.store(true, Ordering::Relaxed);
        }
    };

    // ---- imports -------------------------------------------------------
    publish(&|p| p.phase = SyncPhase::Importing);
    let mode = snap.wf.storage_mode;
    let defer = ctl.defer_thumbnails && mode == StorageMode::Link;
    // After a hold the committer rests a share of it (at least `min_rest_ms`)
    // so a waiting interface thread gets the lock.
    let rest = |took: Duration| {
        if ctl.pace > 0.0 {
            std::thread::sleep(took.mul_f32(ctl.pace).max(Duration::from_millis(ctl.min_rest_ms)));
        }
    };
    struct Produced {
        staged: crate::import::Staged,
        fingerprint: Option<String>,
    }
    let mut batch: Vec<(usize, Produced)> = Vec::new();
    let mut avg_commit_ms = 20.0f64;
    // Commits a group of staged files under ONE lock hold and flushes what they
    // left pending inside it, so the interface finds nothing to flush after
    // each file. The group is sized so the hold stays near `hold_budget_ms`.
    let commit_batch = |group: Vec<(usize, Produced)>, avg: &mut f64| {
        if group.is_empty() || halt.load(Ordering::Relaxed) {
            return; // staged files left over after a stop are dropped, cleaning their staging
        }
        if let Err(e) = marker.mark() {
            outcome.lock().unwrap_or_else(|p| p.into_inner()).first_error.get_or_insert(e);
            stop.store(true, Ordering::Relaxed);
            halt.store(true, Ordering::Relaxed);
            return;
        }
        let started = Instant::now();
        let count = group.len();
        let mut done: Vec<(usize, std::result::Result<String, Error>, u64, u64, Option<String>)> = Vec::new();
        {
            let mut lib = lock(shared);
            for (i, Produced { staged, fingerprint }) in group {
                let (path, _, _) = imports[i];
                if let Err(e) = check_root(&lib, &snap).and_then(|()| fault::check(FaultPoint::SyncBeforeImport, path)) {
                    done.push((i, Err(e), 0, 0, None));
                    break;
                }
                let (size, mtime) = (staged.item().size, staged.item().mtime);
                let committed = lib.commit_import(staged);
                let failed = committed.is_err();
                done.push((i, committed, size, mtime, fingerprint));
                if failed {
                    break;
                }
            }
            if lib.has_pending_writes()
                && let Err(e) = lib.flush()
            {
                done.push((usize::MAX, Err(e), 0, 0, None));
            }
        }
        let took = started.elapsed();
        *avg = 0.7 * *avg + 0.3 * (took.as_secs_f64() * 1000.0 / count as f64);
        rest(took);
        for (i, committed, size, mtime, fingerprint) in done {
            match committed {
                Ok(id) => {
                    let (path, _, key) = imports[i];
                    let mut out = outcome.lock().unwrap_or_else(|p| p.into_inner());
                    if mode == StorageMode::Copy {
                        out.provenance.push((key.clone(), SyncSource { item_id: id.clone(), size, mtime_ms: mtime, fingerprint }));
                    }
                    if defer {
                        out.pending_thumbnails.push(id.clone());
                    }
                    out.added_ids.push(id);
                    drop(out);
                    publish(&|p| {
                        p.done += 1;
                        p.added += 1;
                        p.current = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    });
                }
                Err(e) => {
                    outcome.lock().unwrap_or_else(|p| p.into_inner()).first_error.get_or_insert(e);
                    stop.store(true, Ordering::Relaxed);
                    halt.store(true, Ordering::Relaxed);
                }
            }
        }
    };
    pipeline::<Produced>(
        imports.len(),
        ctl.threads,
        &halt,
        &|i| {
            sync_halt();
            if halt.load(Ordering::Relaxed) {
                return None;
            }
            let (path, folder_id, _) = imports[i];
            let opts = ImportOptions { folders: vec![folder_id.clone()], storage_mode: Some(mode), defer_thumbnail: defer, ..Default::default() };
            let staged = stage(path, &opts).ok()?;
            let fingerprint = (mode == StorageMode::Copy).then(|| linked::compute_fast_fingerprint(path, staged.item().size).ok()).flatten();
            Some(Produced { staged, fingerprint })
        },
        &mut |i, produced| {
            sync_halt();
            let Some(produced) = produced else {
                outcome.lock().unwrap_or_else(|p| p.into_inner()).skipped += 1;
                publish(&|p| p.done += 1);
                return;
            };
            if halt.load(Ordering::Relaxed) {
                return;
            }
            batch.push((i, produced));
            let due = (ctl.hold_budget_ms as f64 / avg_commit_ms.max(1.0)).clamp(1.0, 8.0) as usize;
            if batch.len() >= due {
                commit_batch(std::mem::take(&mut batch), &mut avg_commit_ms);
            }
        },
    );
    // What was staged last still has to be committed (unless the pass stopped).
    commit_batch(std::mem::take(&mut batch), &mut avg_commit_ms);

    // ---- refreshes (Link) ---------------------------------------------------
    if !refreshes.is_empty() && !stop_all() {
        publish(&|p| p.phase = SyncPhase::Refreshing);
        // One short lock to copy the items; the work below holds none.
        let items: HashMap<String, Item> = {
            let lib = lock(shared);
            refreshes.iter().filter_map(|(id, _, _)| lib.item(id).map(|i| ((*id).clone(), i.clone()))).collect()
        };
        pipeline::<Option<Prepared>>(
            refreshes.len(),
            ctl.threads,
            &halt,
            &|i| {
                sync_halt();
                if halt.load(Ordering::Relaxed) {
                    return None;
                }
                let (id, source, old) = refreshes[i];
                let item = items.get(id)?;
                Some(prepare_refresh(&snap.root, item, source, old))
            },
            &mut |i, prepared| {
                sync_halt();
                if halt.load(Ordering::Relaxed) {
                    return;
                }
                let id = refreshes[i].0;
                let started = Instant::now();
                let result = match prepared.flatten() {
                    Some(p) => marker.mark().and_then(|()| {
                        let mut lib = lock(shared);
                        check_root(&lib, &snap).and_then(|()| commit_refresh(&mut lib, p))
                    }),
                    None => Ok(false),
                };
                rest(started.elapsed());
                let mut out = outcome.lock().unwrap_or_else(|p| p.into_inner());
                match result {
                    Ok(true) => {
                        out.refreshed_ids.push(id.clone());
                        let pending = lock(shared).item(id).is_some_and(|item| crate::import::needs_deferred_thumbnail(item, &snap.root));
                        if pending {
                            out.pending_thumbnails.push(id.clone());
                        }
                        drop(out);
                        publish(&|p| {
                            p.done += 1;
                            p.modified += 1;
                        });
                    }
                    Ok(false) => {
                        out.skipped += 1;
                        drop(out);
                        publish(&|p| p.done += 1);
                    }
                    Err(e) => {
                        out.first_error.get_or_insert(e);
                        stop.store(true, Ordering::Relaxed);
                        halt.store(true, Ordering::Relaxed);
                    }
                }
            },
        );
    }

    // ---- finish -----------------------------------------------------------
    let out = outcome.into_inner().unwrap_or_else(|p| p.into_inner());
    provenance.extend(out.provenance);
    publish(&|p| p.phase = SyncPhase::Finishing);
    let offline;
    {
        let mut lib = lock(shared);
        check_root(&lib, &snap)?;
        let still_listed = lib.config().find_watched_folder(&snap.wf.id).is_some();
        // What was done is recorded even when the pass stopped early, so the
        // next pass does not import it again. A folder removed meanwhile gets
        // nothing written back.
        if !provenance.is_empty() && still_listed {
            let prefix = format!("{}|", snap.wf.id);
            let mut all: BTreeMap<String, SyncSource> = snap.provenance.clone();
            all.extend(provenance);
            lib.store_sync_sources(&prefix, &all)?;
        }
        if let Some(e) = out.first_error {
            return Err(e);
        }
        fault::check(FaultPoint::SyncBeforeFinalize, &snap.wf.source_path)?;
        // A source that vanished during the pass (an unplugged drive) is
        // offline, whatever the staging of its files reported.
        offline = !snap.wf.source_path.is_dir();
        let was_marked = marker.marked();
        let cancelled_now = cancelled();
        update_wf(&mut lib, &snap.wf.id, |wf| {
            let (status, error) = if offline {
                (SyncStatus::Offline, Some("Source folder is offline or unreachable".to_string()))
            } else if cancelled_now {
                (SyncStatus::Idle, Some("Stopped before it finished; the next sync continues".to_string()))
            } else {
                (SyncStatus::Success, None)
            };
            // An unchanged folder in its settled state costs no write.
            if !was_marked && wf.status == status && wf.last_error == error {
                return false;
            }
            if status == SyncStatus::Success {
                wf.last_synced = Some(now_ms());
            }
            wf.status = status;
            wf.last_error = error;
            true
        })?;
    }

    stats.added = out.added_ids.len();
    stats.modified = out.refreshed_ids.len();
    stats.skipped += out.skipped;
    stats.added_ids = out.added_ids;
    stats.refreshed_ids = out.refreshed_ids;
    stats.pending_thumbnail_ids = out.pending_thumbnails;
    stats.cancelled = cancelled();
    stats.offline = offline;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_mode::SyncMode;
    use std::time::{Duration, Instant};

    fn wf_for(id: &str, src: &Path, folder: &str, mode: StorageMode) -> WatchedFolder {
        WatchedFolder {
            id: id.into(),
            source_path: src.to_path_buf(),
            folder_id: folder.into(),
            storage_mode: mode,
            sync_mode: SyncMode::Manual,
            recursive: true,
            last_synced: None,
            status: SyncStatus::Idle,
            last_error: None,
        }
    }

    fn write_png(path: &Path, w: u32, h: u32, shade: u8) {
        image::RgbImage::from_pixel(w, h, image::Rgb([shade, (w % 250) as u8, 200])).save(path).unwrap();
    }

    /// A shared library with one watched folder over `count` PNGs spread over
    /// `dirs` subfolders.
    fn setup(tmp: &Path, mode: StorageMode, count: usize, dirs: usize) -> (Mutex<Library>, PathBuf, String) {
        let mut lib = Library::create(tmp.join("L.library")).unwrap();
        let fid = lib.create_folder(None, "Synced").unwrap();
        let src = tmp.join("src");
        for d in 0..dirs.max(1) {
            fs::create_dir_all(src.join(format!("d{d}"))).unwrap();
        }
        for i in 0..count {
            write_png(&src.join(format!("d{}", i % dirs.max(1))).join(format!("img{i}.png")), 8 + (i % 7) as u32, 8, (i % 251) as u8);
        }
        lib.upsert_watched_folder(wf_for("wf", &src, &fid, mode)).unwrap();
        (Mutex::new(lib), src, fid)
    }

    fn ctl<'a>(cancel: &'a AtomicBool, progress: &'a (dyn Fn(&SyncProgress) + Sync), threads: usize) -> SyncControl<'a> {
        SyncControl { cancel, progress, threads, defer_thumbnails: false, pace: 0.0, settle_ms: 0, hold_budget_ms: 40, min_rest_ms: 0 }
    }

    fn quiet(_: &SyncProgress) {}

    fn status(shared: &Mutex<Library>) -> String {
        lock(shared).config().linked_folders[0]["status"].as_str().unwrap().to_string()
    }

    #[test]
    fn link_pass_imports_refreshes_and_is_idempotent_with_any_thread_count() {
        for threads in [1usize, 4] {
            let tmp = tempfile::tempdir().unwrap();
            let (shared, src, _) = setup(tmp.path(), StorageMode::Link, 12, 3);
            let cancel = AtomicBool::new(false);
            let first = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, threads)).unwrap();
            assert_eq!((first.added, first.modified), (12, 0), "threads={threads}");
            assert_eq!(status(&shared), "success");
            let (id, file) = {
                let lib = lock(&shared);
                let id = lib.items()[0].id.clone();
                let rec = lib.link_record(&id).unwrap().unwrap();
                (id, rec.source_path)
            };
            lock(&shared).add_tags_to_items(std::slice::from_ref(&id), &["keep".to_string()]).unwrap();

            let second = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, threads)).unwrap();
            assert_eq!((second.added, second.modified, second.skipped), (0, 0, 12), "idempotent, threads={threads}");

            write_png(&file, 640, 480, 77);
            let third = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, threads)).unwrap();
            assert_eq!((third.added, third.modified), (0, 1), "threads={threads}");
            let lib = lock(&shared);
            let item = lib.item(&id).unwrap();
            assert_eq!((item.width, item.height), (640, 480));
            assert!(item.tags.contains(&"keep".to_string()), "user data kept");
            assert_eq!(lib.items().len(), 12);
            assert_eq!(image::open(&file).unwrap().width(), 640, "the referenced original is untouched");
            drop(lib);
            let fourth = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, threads)).unwrap();
            assert_eq!(fourth.modified, 0);
            let _ = src;
        }
    }

    #[test]
    fn copy_pass_survives_renames_reports_conflicts_and_never_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, src, _) = setup(tmp.path(), StorageMode::Copy, 6, 2);
        let cancel = AtomicBool::new(false);
        let first = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 3)).unwrap();
        assert_eq!(first.added, 6);
        let id = first.added_ids[0].clone();
        {
            let mut lib = lock(&shared);
            let mut item = lib.item(&id).unwrap().clone();
            item.name = "Renamed".into();
            lib.update_item(item).unwrap();
        }
        let again = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 3)).unwrap();
        assert_eq!((again.added, again.conflicts), (0, 0));
        assert_eq!(lock(&shared).items().len(), 6);

        // Change one source: reported, not applied.
        let victim = src.join("d0").join("img0.png");
        let managed_before = {
            let lib = lock(&shared);
            fs::read(lib.managed_writable_path(&id, "t").unwrap()).unwrap()
        };
        write_png(&victim, 33, 21, 5);
        let changed = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 3)).unwrap();
        assert_eq!((changed.added, changed.conflicts), (0, 1));
        let lib = lock(&shared);
        assert_eq!(fs::read(lib.managed_writable_path(&id, "t").unwrap()).unwrap(), managed_before);
    }

    #[test]
    fn cancel_keeps_what_was_done_and_the_next_pass_finishes_without_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _, _) = setup(tmp.path(), StorageMode::Copy, 10, 2);
        let cancel = AtomicBool::new(false);
        let stop_after_three = |p: &SyncProgress| {
            if p.added >= 3 {
                cancel.store(true, Ordering::Relaxed);
            }
        };
        let partial = sync_shared(&shared, "wf", &ctl(&cancel, &stop_after_three, 1)).unwrap();
        assert!(partial.cancelled);
        assert_eq!(partial.added, 3);
        assert_eq!(status(&shared), "idle");
        assert!(lock(&shared).watched_folders()[0].last_error.as_deref().is_some_and(|e| e.contains("Stopped")));

        cancel.store(false, Ordering::Relaxed);
        let rest = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 2)).unwrap();
        assert_eq!((rest.cancelled, rest.added), (false, 7));
        assert_eq!(lock(&shared).items().len(), 10, "no duplicates");
        assert_eq!(status(&shared), "success");
    }

    #[test]
    fn offline_source_is_reported_and_nothing_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, src, _) = setup(tmp.path(), StorageMode::Link, 3, 1);
        let cancel = AtomicBool::new(false);
        sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 2)).unwrap();
        let moved = tmp.path().join("unplugged");
        fs::rename(&src, &moved).unwrap();
        let stats = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 2)).unwrap();
        assert!(stats.offline);
        assert_eq!(status(&shared), "offline");
        assert_eq!(lock(&shared).items().len(), 3, "items stay");
        fs::rename(&moved, &src).unwrap();
        let back = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 2)).unwrap();
        assert_eq!((back.added, back.modified), (0, 0));
        assert_eq!(status(&shared), "success");
    }

    #[test]
    fn failure_at_every_boundary_ends_in_error_and_the_next_pass_recovers() {
        use crate::fault::{FaultPoint as P, install};
        let scenarios: [(P, StorageMode, bool); 9] = [
            (P::SyncAfterMarkedSyncing, StorageMode::Link, false),
            (P::SyncBeforeFolderCreate, StorageMode::Link, false),
            (P::SyncBeforeImport, StorageMode::Link, false),
            (P::SyncBeforeImport, StorageMode::Copy, false),
            (P::SyncBeforeProvenanceSave, StorageMode::Copy, false),
            (P::SyncBeforeFinalize, StorageMode::Copy, false),
            (P::SyncBeforeFinalize, StorageMode::Link, false),
            (P::SyncBeforeRefresh, StorageMode::Link, true),
            (P::SyncBeforeRefreshSidecar, StorageMode::Link, true),
        ];
        for (point, mode, refresh) in scenarios {
            let tmp = tempfile::tempdir().unwrap();
            let (shared, src, _) = setup(tmp.path(), mode, 4, 2);
            let cancel = AtomicBool::new(false);
            if refresh {
                sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 1)).unwrap();
                write_png(&src.join("d0").join("img0.png"), 70, 50, 9);
            }
            let old_identity = {
                let lib = lock(&shared);
                lib.items().first().and_then(|i| lib.link_record(&i.id).ok().flatten()).map(|r| (r.item_id, r.identity))
            };
            {
                let _guard = install(move |p| if p == point { Err(std::io::Error::other("injected")) } else { Ok(()) });
                assert!(sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 1)).is_err(), "{point:?} {mode:?}");
            }
            assert_eq!(status(&shared), "error", "{point:?} {mode:?}");
            if point == P::SyncBeforeRefreshSidecar {
                let lib = lock(&shared);
                let (id, identity) = old_identity.clone().unwrap();
                assert_eq!(lib.link_record(&id).unwrap().unwrap().identity, identity, "old identity stays on record");
            }
            let stats = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 1)).unwrap();
            assert_eq!(status(&shared), "success", "{point:?} {mode:?}: recovered");
            assert_eq!(lock(&shared).items().len(), 4, "{point:?} {mode:?}: no duplicates");
            if refresh {
                assert_eq!(stats.modified, 1, "{point:?}: the change is found again");
            }
            let last = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 1)).unwrap();
            assert_eq!((last.added, last.modified, last.conflicts), (0, 0, 0), "{point:?} {mode:?}: idempotent");
        }
    }

    #[test]
    fn an_unchanged_folder_costs_no_writes() {
        for mode in [StorageMode::Link, StorageMode::Copy] {
            let tmp = tempfile::tempdir().unwrap();
            let (shared, _, _) = setup(tmp.path(), mode, 6, 2);
            let cancel = AtomicBool::new(false);
            sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 2)).unwrap();
            let config = lock(&shared).root().join("falcon.json");
            let (bytes, stamp) = (fs::read(&config).unwrap(), fs::metadata(&config).unwrap().modified().unwrap());
            let meta = lock(&shared).root().join("metadata.json");
            let meta_stamp = fs::metadata(&meta).unwrap().modified().unwrap();
            for _ in 0..3 {
                let again = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 2)).unwrap();
                assert_eq!((again.added, again.modified, again.conflicts), (0, 0, 0));
            }
            assert_eq!(fs::read(&config).unwrap(), bytes, "{mode:?}: falcon.json is not rewritten");
            assert_eq!(fs::metadata(&config).unwrap().modified().unwrap(), stamp, "{mode:?}");
            assert_eq!(fs::metadata(&meta).unwrap().modified().unwrap(), meta_stamp, "{mode:?}: nothing else written");
        }
    }

    #[test]
    fn a_folder_removed_during_a_pass_is_not_brought_back() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _, _) = setup(tmp.path(), StorageMode::Copy, 12, 2);
        let cancel = AtomicBool::new(false);
        let removed = AtomicBool::new(false);
        let remove_mid_pass = |p: &SyncProgress| {
            if p.added >= 2 && !removed.swap(true, Ordering::Relaxed) {
                lock(&shared).remove_watched_folder("wf").unwrap();
            }
        };
        let _ = sync_shared(&shared, "wf", &ctl(&cancel, &remove_mid_pass, 1));
        assert!(removed.load(Ordering::Relaxed));
        let lib = lock(&shared);
        assert!(lib.watched_folders().is_empty(), "the stale pass did not re-create it");
        assert!(lib.config().sync_sources.is_empty(), "nor leave its provenance behind");
    }

    #[test]
    fn nothing_is_committed_into_a_library_that_moved_during_the_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _, _) = setup(tmp.path(), StorageMode::Copy, 8, 1);
        let cancel = AtomicBool::new(false);
        let moved = AtomicBool::new(false);
        let move_mid_pass = |p: &SyncProgress| {
            if p.added >= 2 && !moved.swap(true, Ordering::Relaxed) {
                lock(&shared).set_root(PathBuf::from("/nonexistent/elsewhere.library"));
            }
        };
        let result = sync_shared(&shared, "wf", &ctl(&cancel, &move_mid_pass, 1));
        assert!(result.is_err_and(|e| e.to_string().contains("moved")));
        assert_eq!(lock(&shared).items().len(), 2, "no commits after the move");
    }

    #[test]
    fn files_still_being_written_are_left_for_a_later_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _, _) = setup(tmp.path(), StorageMode::Copy, 5, 1);
        let cancel = AtomicBool::new(false);
        let mut control = ctl(&cancel, &quiet, 1);
        control.settle_ms = 3_600_000; // everything counts as just written
        let first = sync_shared(&shared, "wf", &control).unwrap();
        assert_eq!((first.added, first.unsettled), (0, 5));
        control.settle_ms = 0;
        assert_eq!(sync_shared(&shared, "wf", &control).unwrap().added, 5);
    }

    #[test]
    fn a_source_that_vanishes_mid_pass_ends_offline_not_success() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, src, _) = setup(tmp.path(), StorageMode::Copy, 10, 1);
        let cancel = AtomicBool::new(false);
        let gone = tmp.path().join("unplugged");
        let unplug = |p: &SyncProgress| {
            if p.added == 3 && !gone.exists() {
                fs::rename(&src, &gone).unwrap();
            }
        };
        let stats = sync_shared(&shared, "wf", &ctl(&cancel, &unplug, 1)).unwrap();
        assert!(stats.offline);
        assert_eq!(status(&shared), "offline");
        fs::rename(&gone, &src).unwrap();
        let back = sync_shared(&shared, "wf", &ctl(&cancel, &quiet, 1)).unwrap();
        assert_eq!(status(&shared), "success");
        assert_eq!(lock(&shared).items().len(), 10, "{} + {} and no duplicates", stats.added, back.added);
    }

    #[test]
    fn deferred_link_imports_are_listed_for_the_thumbnail_worker() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _, _) = setup(tmp.path(), StorageMode::Link, 5, 1);
        let cancel = AtomicBool::new(false);
        let control = SyncControl { cancel: &cancel, progress: &quiet, threads: 2, defer_thumbnails: true, pace: 0.0, settle_ms: 0, hold_budget_ms: 40, min_rest_ms: 0 };
        let stats = sync_shared(&shared, "wf", &control).unwrap();
        assert_eq!(stats.added, 5);
        assert_eq!(stats.pending_thumbnail_ids.len(), 5);
    }

    /// `FALCON_BENCH=1000,5000,10000 cargo test -p falcon-library --release
    /// background_sync::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "measurement, not a check"]
    fn bench() {
        let sizes: Vec<usize> = std::env::var("FALCON_BENCH").unwrap_or_else(|_| "1000".into()).split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let runs: usize = std::env::var("FALCON_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let legacy = std::env::var("FALCON_LEGACY").is_ok();
        for mode in [StorageMode::Link, StorageMode::Copy] {
            for &n in &sizes {
                // Fewer repeats for the largest sizes, which take minutes each.
                for run in 0..(if n >= 10_000 { 1 } else { runs }) {
                    println!("BENCH-RUN mode={mode:?} n={n} run={run}");
                    bench_once(mode, n, false);
                    if legacy && n <= 1000 && run == 0 {
                        bench_once(mode, n, true);
                    }
                }
            }
        }
    }

    fn bench_pace() -> f32 {
        std::env::var("FALCON_PACE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.15)
    }

    fn bench_budget() -> u64 {
        std::env::var("FALCON_BUDGET").ok().and_then(|v| v.parse().ok()).unwrap_or(40)
    }

    /// CPU seconds this process has used so far (`ps` reports hundredths).
    fn cpu_secs() -> f64 {
        let out = std::process::Command::new("ps").args(["-o", "cputime=", "-p", &std::process::id().to_string()]).output().ok();
        let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        // [[hh:]mm:]ss.cc
        text.split(':').try_fold(0.0, |acc, part| part.parse::<f64>().ok().map(|v| acc * 60.0 + v)).unwrap_or(0.0)
    }

    fn rss_kb() -> u64 {
        std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
            .unwrap_or(0)
    }

    fn bench_once(mode: StorageMode, n: usize, legacy: bool) {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _, _) = setup(tmp.path(), mode, n, 20);
        lock(&shared).set_write_mode(crate::undo::WriteMode::Batched).unwrap();
        let done = AtomicBool::new(false);
        let rss_before = rss_kb();
        let cpu_before = cpu_secs();
        let (stalls, peak_rss, elapsed, added) = std::thread::scope(|scope| {
            let probe = scope.spawn(|| {
                let mut waits = Vec::new();
                // A frame of the app: lock, draw, flush what is pending, unlock.
                while !done.load(Ordering::Relaxed) {
                    let t = Instant::now();
                    let mut lib = lock(&shared);
                    let got = t.elapsed();
                    if lib.has_pending_writes() {
                        let _ = lib.flush();
                    }
                    drop(lib);
                    waits.push(got);
                    std::thread::sleep(Duration::from_millis(2));
                }
                waits
            });
            let sampler = scope.spawn(|| {
                let mut peak = 0;
                while !done.load(Ordering::Relaxed) {
                    peak = peak.max(rss_kb());
                    std::thread::sleep(Duration::from_millis(50));
                }
                peak
            });
            let started = Instant::now();
            let cancel = AtomicBool::new(false);
            let added = if legacy {
                let mut lib = lock(&shared);
                let mut wf = lib.config().find_watched_folder("wf").unwrap();
                lib.sync_watched_folder(&mut wf).unwrap().added
            } else {
                let c = SyncControl { cancel: &cancel, progress: &quiet, threads: 4, defer_thumbnails: mode == StorageMode::Link, pace: bench_pace(), settle_ms: 0, hold_budget_ms: bench_budget(), min_rest_ms: 2 };
                sync_shared(&shared, "wf", &c).unwrap().added
            };
            let elapsed = started.elapsed();
            done.store(true, Ordering::Relaxed);
            (probe.join().unwrap(), sampler.join().unwrap(), elapsed, added)
        });
        let mut ms: Vec<f64> = stalls.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct = |p: f64| ms.get(((ms.len() as f64 - 1.0) * p) as usize).copied().unwrap_or(0.0);
        let cpu = cpu_secs() - cpu_before;
        println!(
            "BENCH mode={mode:?} n={n} path={} added={added} duration={:.2} rate={:.1} probes={} p50={:.2} p95={:.2} p99={:.2} max={:.2} cpu_pct={:.0} rss_mb={}",
            if legacy { "legacy" } else { "background" },
            elapsed.as_secs_f64(),
            added as f64 / elapsed.as_secs_f64(),
            ms.len(),
            pct(0.5),
            pct(0.95),
            pct(0.99),
            ms.last().copied().unwrap_or(0.0),
            100.0 * cpu / elapsed.as_secs_f64(),
            peak_rss.saturating_sub(rss_before) / 1024,
        );
        // An unchanged pass (what an automatic folder runs every five minutes).
        if !legacy {
            let cancel = AtomicBool::new(false);
            let c = SyncControl { cancel: &cancel, progress: &quiet, threads: 4, defer_thumbnails: mode == StorageMode::Link, pace: bench_pace(), settle_ms: 0, hold_budget_ms: bench_budget(), min_rest_ms: 2 };
            let (cpu0, t0) = (cpu_secs(), Instant::now());
            let mut times = Vec::new();
            for _ in 0..3 {
                let t = Instant::now();
                let st = sync_shared(&shared, "wf", &c).unwrap();
                assert_eq!((st.added, st.modified), (0, 0));
                times.push(t.elapsed().as_secs_f64());
            }
            println!(
                "BENCH-IDLE mode={mode:?} n={n} pass_secs={:.3},{:.3},{:.3} cpu_pct={:.0}",
                times[0], times[1], times[2],
                100.0 * (cpu_secs() - cpu0) / t0.elapsed().as_secs_f64()
            );
        }
        // Cancellation latency on a fresh library of the same size.
        if !legacy {
            let tmp2 = tempfile::tempdir().unwrap();
            let (shared2, _, _) = setup(tmp2.path(), mode, n, 20);
            lock(&shared2).set_write_mode(crate::undo::WriteMode::Batched).unwrap();
            let cancel = AtomicBool::new(false);
            let at = Mutex::new(None::<Instant>);
            let progress = |p: &SyncProgress| {
                if p.added >= n / 2 && !cancel.swap(true, Ordering::Relaxed) {
                    *at.lock().unwrap() = Some(Instant::now());
                }
            };
            let c = SyncControl { cancel: &cancel, progress: &progress, threads: 4, defer_thumbnails: mode == StorageMode::Link, pace: bench_pace(), settle_ms: 0, hold_budget_ms: bench_budget(), min_rest_ms: 2 };
            let stats = sync_shared(&shared2, "wf", &c).unwrap();
            let latency = at.lock().unwrap().map(|t| t.elapsed());
            println!("BENCH-CANCEL mode={mode:?} n={n} cancelled={} added_when_stopped={} latency_ms={:.1}", stats.cancelled, stats.added, latency.map_or(f64::NAN, |l| l.as_secs_f64() * 1000.0));
        }
    }

    /// `cargo test -p falcon-library --release background_sync::tests::commit_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "measurement, not a check"]
    fn commit_cost() {
        for mode in [StorageMode::Link, StorageMode::Copy] {
            let tmp = tempfile::tempdir().unwrap();
            let (shared, src, _) = setup(tmp.path(), mode, 200, 4);
            // The app keeps its library in batched mode while it is open.
            lock(&shared).set_write_mode(crate::undo::WriteMode::Batched).unwrap();
            let mut files: Vec<PathBuf> = Vec::new();
            for d in 0..4 {
                for e in fs::read_dir(src.join(format!("d{d}"))).unwrap().flatten() {
                    files.push(e.path());
                }
            }
            let (mut stage_ms, mut commit_ms, mut flush_ms) = (Vec::new(), Vec::new(), Vec::new());
            for (n, f) in files.iter().enumerate() {
                let opts = ImportOptions { storage_mode: Some(mode), defer_thumbnail: mode == StorageMode::Link, ..Default::default() };
                let t = Instant::now();
                let staged = stage(f, &opts).unwrap();
                stage_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                let mut lib = lock(&shared);
                let t = Instant::now();
                lib.commit_import(staged).unwrap();
                commit_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                // What the app does at the end of each frame; here once per 10 commits.
                if n % 10 == 9 && lib.has_pending_writes() {
                    let t = Instant::now();
                    lib.flush().unwrap();
                    flush_ms.push(t.elapsed().as_secs_f64() * 1000.0);
                }
            }
            let stat = |v: &mut Vec<f64>| {
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                (v.iter().sum::<f64>() / v.len() as f64, v[v.len() / 2], v[v.len() * 99 / 100])
            };
            println!(
                "COST mode={mode:?} stage={:.1?} commit_only={:.1?} flush_of_10={:.1?}  (mean,p50,p99 ms)",
                stat(&mut stage_ms),
                stat(&mut commit_ms),
                stat(&mut flush_ms)
            );
        }
    }
}
