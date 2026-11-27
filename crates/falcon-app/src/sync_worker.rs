//! Background folder synchronization: a scheduler with one worker thread per
//! open library, and the watchers that ask it to sync.
//!
//! The passes themselves are `falcon_library::background_sync::sync_shared`,
//! which keeps the library lock short. This module decides *when* and
//! *whether* one runs:
//!
//! - one pass at a time per library (bounded concurrency, bounded memory);
//! - a request for a folder already queued does nothing, and a request for the
//!   folder being synced is coalesced into a single follow-up pass;
//! - a pass can be stopped (it lands between files, the work done is kept);
//! - an automatic folder is watched with file events and synced once its source
//!   has been quiet for [`QUIET`], however many events came;
//! - dropping the scheduler stops the running pass and joins the thread, so a
//!   closed or moved library is never written to afterwards.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use falcon_library::SyncStats;
use falcon_library::background_sync::{SyncControl, SyncProgress, sync_shared};
use falcon_server::Shared;

/// How long a source folder must stay quiet before an automatic sync starts.
pub const QUIET: Duration = Duration::from_secs(3);
/// Without file events, how often an automatic folder is synced anyway.
pub const POLL_FALLBACK: Duration = Duration::from_secs(300);
/// Staging threads of one pass.
const STAGE_THREADS: usize = 3;
/// After each commit under the library lock the committer rests for this
/// share of the commit's time, leaving the interface at least half of it.
const PACE: f32 = 1.0;
/// How often a watcher looks at whether its source is there (an unplugged
/// drive, a renamed folder).
const PROBE: Duration = if cfg!(test) { Duration::from_millis(100) } else { Duration::from_secs(2) };
/// Passes that find files still being written are repeated this often, a
/// bounded number of times.
const UNSETTLED_RETRY: Duration = Duration::from_secs(4);
const UNSETTLED_RETRIES: u32 = 30;
/// How recently a file may have changed and still be left alone as possibly
/// unfinished (none under test, where files are made just before the pass).
const SETTLE: u64 = if cfg!(test) { 0 } else { falcon_library::background_sync::SETTLE_MS };
/// Least time between repaint requests caused by progress.
const REPAINT_EVERY: Duration = Duration::from_millis(120);

/// A pass that ended.
pub struct Finished {
    pub wf_id: String,
    pub result: Result<SyncStats, String>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<String>,
    running: Option<String>,
    /// Folders asked for again while their pass was running.
    rerun: HashSet<String>,
    progress: Option<(String, SyncProgress)>,
    finished: Vec<Finished>,
    /// Passes to repeat once their unsettled files have had time to settle.
    later: Vec<(Instant, String)>,
    unsettled_tries: HashMap<String, u32>,
}

struct Inner {
    state: Mutex<State>,
    wake: Condvar,
    /// Stops the running pass; cleared when the next one starts.
    cancel: AtomicBool,
    stop: AtomicBool,
}

impl Inner {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Queues `wf_id`. False when it was already waiting; a request for the
    /// folder being synced is remembered and runs once more afterwards.
    fn request(&self, wf_id: &str) -> bool {
        let mut st = self.state();
        if st.running.as_deref() == Some(wf_id) {
            return st.rerun.insert(wf_id.to_string());
        }
        if st.queue.iter().any(|q| q == wf_id) {
            return false;
        }
        st.queue.push_back(wf_id.to_string());
        drop(st);
        self.wake.notify_all();
        true
    }
}

/// The scheduler of one library.
pub struct SyncScheduler {
    inner: Arc<Inner>,
    root: PathBuf,
    thread: Option<JoinHandle<()>>,
}

impl SyncScheduler {
    pub fn new(shared: Shared, root: PathBuf, repaint: impl Fn() + Send + Sync + 'static) -> SyncScheduler {
        let inner = Arc::new(Inner { state: Mutex::default(), wake: Condvar::new(), cancel: AtomicBool::new(false), stop: AtomicBool::new(false) });
        let worker = Arc::clone(&inner);
        let thread = std::thread::Builder::new()
            .name("folder-sync".into())
            .spawn(move || worker_loop(&worker, &shared, &repaint))
            .ok();
        SyncScheduler { inner, root, thread }
    }

    /// The library folder this scheduler was made for.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Asks for a pass over `wf_id`. False when one was already waiting.
    pub fn request(&self, wf_id: &str) -> bool {
        self.inner.request(wf_id)
    }

    /// Stops the pass over `wf_id` if it is running, and forgets a waiting one.
    pub fn cancel(&self, wf_id: &str) {
        let mut st = self.inner.state();
        st.queue.retain(|q| q != wf_id);
        st.rerun.remove(wf_id);
        if st.running.as_deref() == Some(wf_id) {
            self.inner.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Stops the running pass and forgets every waiting one.
    pub fn cancel_all(&self) {
        let mut st = self.inner.state();
        st.queue.clear();
        st.rerun.clear();
        st.later.clear();
        if st.running.is_some() {
            self.inner.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Stops everything and waits, at most `timeout`, for the running pass to
    /// end. True when the scheduler is idle (nothing will touch the library
    /// until a new request). Used before a library is moved or closed; it
    /// never waits longer than `timeout`, so the interface cannot hang on it.
    pub fn quiesce(&self, timeout: Duration) -> bool {
        self.cancel_all();
        let until = Instant::now() + timeout;
        loop {
            if self.inner.state().running.is_none() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Whether `wf_id` is running or waiting.
    pub fn is_active(&self, wf_id: &str) -> bool {
        let st = self.inner.state();
        st.running.as_deref() == Some(wf_id) || st.queue.iter().any(|q| q == wf_id)
    }

    /// How far the running pass is, with its folder id.
    pub fn progress(&self) -> Option<(String, SyncProgress)> {
        self.inner.state().progress.clone()
    }

    /// The passes that ended since the last call.
    pub fn take_finished(&self) -> Vec<Finished> {
        std::mem::take(&mut self.inner.state().finished)
    }

    /// A handle that watchers use to ask for passes.
    fn requester(&self) -> Arc<Inner> {
        Arc::clone(&self.inner)
    }
}

impl Drop for SyncScheduler {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        self.inner.cancel.store(true, Ordering::Relaxed);
        self.inner.wake.notify_all();
        // An idle worker is joined. One in the middle of a pass is not waited
        // for (that could freeze the interface for as long as a large file
        // takes to read): it sees `stop`, commits nothing more, and ends on
        // its own, holding only its own handle to the library.
        let idle = self.inner.state().running.is_none();
        if let Some(t) = self.thread.take()
            && idle
        {
            let _ = t.join();
        }
    }
}

fn worker_loop(inner: &Inner, shared: &Shared, repaint: &(dyn Fn() + Send + Sync)) {
    loop {
        let wf_id = {
            let mut st = inner.state();
            loop {
                if inner.stop.load(Ordering::Relaxed) {
                    return;
                }
                let now = Instant::now();
                let due: Vec<String> = st.later.iter().filter(|(t, _)| *t <= now).map(|(_, id)| id.clone()).collect();
                st.later.retain(|(t, _)| *t > now);
                for id in due {
                    if !st.queue.contains(&id) {
                        st.queue.push_back(id);
                    }
                }
                if let Some(id) = st.queue.pop_front() {
                    st.running = Some(id.clone());
                    break id;
                }
                st = match st.later.iter().map(|(t, _)| *t).min() {
                    Some(next) => inner.wake.wait_timeout(st, next.saturating_duration_since(now)).unwrap_or_else(|p| p.into_inner()).0,
                    None => inner.wake.wait(st).unwrap_or_else(|p| p.into_inner()),
                };
            }
        };
        inner.cancel.store(false, Ordering::Relaxed);

        let last_repaint = Mutex::new(Instant::now() - REPAINT_EVERY);
        let on_progress = |p: &SyncProgress| {
            inner.state().progress = Some((wf_id.clone(), p.clone()));
            let mut last = last_repaint.lock().unwrap_or_else(|p| p.into_inner());
            if last.elapsed() >= REPAINT_EVERY {
                *last = Instant::now();
                repaint();
            }
        };
        let control = SyncControl { cancel: &inner.cancel, progress: &on_progress, threads: STAGE_THREADS, defer_thumbnails: true, pace: PACE, settle_ms: SETTLE };
        // A panic in a pass must not take the worker (and every later sync) with it.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sync_shared(shared, &wf_id, &control)))
            .map_err(|_| "the synchronization stopped unexpectedly".to_string())
            .and_then(|r| r.map_err(|e| e.to_string()));

        {
            let mut st = inner.state();
            st.running = None;
            st.progress = None;
            if st.rerun.remove(&wf_id) && !inner.stop.load(Ordering::Relaxed) && !st.queue.contains(&wf_id) {
                st.queue.push_back(wf_id.clone());
            }
            // Files that were still being written: look again shortly.
            match &result {
                Ok(stats) if stats.unsettled > 0 && !stats.cancelled && !inner.stop.load(Ordering::Relaxed) => {
                    let tries = st.unsettled_tries.entry(wf_id.clone()).or_insert(0);
                    if *tries < UNSETTLED_RETRIES {
                        *tries += 1;
                        st.later.push((Instant::now() + UNSETTLED_RETRY, wf_id.clone()));
                    }
                }
                _ => {
                    st.unsettled_tries.remove(&wf_id);
                }
            }
            st.finished.push(Finished { wf_id, result });
        }
        repaint();
    }
}

/// One watched source folder's file-event watcher.
struct Watch {
    source: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The watchers of the automatic folders of one library.
#[derive(Default)]
pub struct SyncWatchers {
    watches: HashMap<String, Watch>,
}

impl SyncWatchers {
    /// Brings the watchers in line with `wanted` (folder id, source): starts
    /// the missing ones, drops those no longer wanted or whose source moved.
    /// A new watcher also asks for a pass at once, to catch up on what changed
    /// while it was not watching. Returns the folders newly watched.
    pub fn reconcile(&mut self, scheduler: &SyncScheduler, wanted: &[(String, PathBuf)]) -> Vec<String> {
        self.watches.retain(|id, w| wanted.iter().any(|(wid, src)| wid == id && *src == w.source));
        let mut started = Vec::new();
        for (id, source) in wanted {
            if self.watches.contains_key(id) || !source.is_dir() {
                continue;
            }
            let stop = Arc::new(AtomicBool::new(false));
            let (flag, requester, wf_id, src) = (Arc::clone(&stop), scheduler.requester(), id.clone(), source.clone());
            let thread = std::thread::Builder::new().name("folder-sync-watch".into()).spawn(move || watch_loop(&src, &wf_id, &requester, &flag)).ok();
            self.watches.insert(id.clone(), Watch { source: source.clone(), stop, thread });
            scheduler.request(id);
            started.push(id.clone());
        }
        started
    }

    /// Folders being watched.
    pub fn len(&self) -> usize {
        self.watches.len()
    }
}

fn open_events(source: &Path) -> (Option<std::sync::mpsc::Receiver<()>>, Option<crate::fsevents::Stream>) {
    match if cfg!(test) { None } else { crate::fsevents::watch(source, 0.5) } {
        Some((stream, rx)) => (Some(rx), Some(stream)),
        None => (None, None),
    }
}

/// Watches one source folder and asks for a pass when it has been quiet for
/// [`QUIET`] after changes, when it comes back after being offline, and every
/// [`POLL_FALLBACK`] regardless (the fallback also covers dropped file
/// events). A pass over an unchanged folder is cheap, so none of this costs
/// much when nothing happens.
fn watch_loop(source: &Path, wf_id: &str, requester: &Inner, stop: &AtomicBool) {
    let (mut rx, mut _stream) = open_events(source);
    if rx.is_none() && !cfg!(test) {
        crate::errlog::error(&format!("folder sync: no file events for {}, syncing it every {} s", source.display(), POLL_FALLBACK.as_secs()));
    }
    let mut offline = !source.is_dir();
    let mut last_event: Option<Instant> = None;
    let mut last_request = Instant::now();
    let mut last_probe = Instant::now();
    while !stop.load(Ordering::Relaxed) && !requester.stop.load(Ordering::Relaxed) {
        match &rx {
            Some(r) => match r.recv_timeout(Duration::from_millis(100)) {
                Ok(()) => {
                    // A burst counts once, and keeps pushing the start back.
                    while r.try_recv().is_ok() {}
                    last_event = Some(Instant::now());
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => rx = None,
            },
            None => std::thread::sleep(Duration::from_millis(100)),
        }
        if last_probe.elapsed() >= PROBE {
            last_probe = Instant::now();
            let present = source.is_dir();
            if present && offline {
                // The drive is back: a fresh event stream, and a pass once it settles.
                offline = false;
                (rx, _stream) = open_events(source);
                last_event = Some(Instant::now());
            } else if !present && !offline {
                // Unplugged or renamed: the stream is dropped, and one pass
                // records the folder as offline (nothing is deleted).
                offline = true;
                rx = None;
                _stream = None;
                last_event = None;
                last_request = Instant::now();
                requester.request(wf_id);
            }
        }
        if offline {
            continue;
        }
        let quiet = last_event.is_some_and(|t| t.elapsed() >= QUIET_FOR_TEST);
        if quiet || last_request.elapsed() >= POLL_FALLBACK {
            last_event = None;
            last_request = Instant::now();
            requester.request(wf_id);
        }
    }
}

/// [`QUIET`], shortened under test.
const QUIET_FOR_TEST: Duration = if cfg!(test) { Duration::from_millis(200) } else { QUIET };

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_library::storage_mode::{StorageMode, SyncMode, SyncStatus, WatchedFolder};
    use falcon_library::Library;

    fn shared_with_folder(tmp: &Path, files: usize, mode: StorageMode) -> (Shared, PathBuf) {
        let mut lib = Library::create(tmp.join("L.library")).unwrap();
        let fid = lib.create_folder(None, "Synced").unwrap();
        let src = tmp.join("src");
        std::fs::create_dir_all(&src).unwrap();
        for i in 0..files {
            image::RgbImage::from_pixel(8, 8, image::Rgb([i as u8, 1, 2])).save(src.join(format!("p{i}.png"))).unwrap();
        }
        lib.upsert_watched_folder(WatchedFolder {
            id: "wf".into(),
            source_path: src.clone(),
            folder_id: fid,
            storage_mode: mode,
            sync_mode: SyncMode::Auto,
            recursive: true,
            last_synced: None,
            status: SyncStatus::Idle,
            last_error: None,
        })
        .unwrap();
        (Arc::new(Mutex::new(lib)), src)
    }

    fn wait_finished(s: &SyncScheduler, n: usize) -> Vec<Finished> {
        let mut out = Vec::new();
        let until = Instant::now() + Duration::from_secs(60);
        while out.len() < n && Instant::now() < until {
            out.extend(s.take_finished());
            std::thread::sleep(Duration::from_millis(10));
        }
        out
    }

    #[test]
    fn a_request_runs_one_pass_and_duplicates_coalesce() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _) = shared_with_folder(tmp.path(), 20, StorageMode::Copy);
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        assert!(s.request("wf"));
        // Asked again while queued or running: no second queue entry.
        let second = s.request("wf");
        let done = wait_finished(&s, 1);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].result.as_ref().unwrap().added, 20);
        // A running pass remembers one request for after it, however many came.
        if second {
            let more = wait_finished(&s, 1);
            assert_eq!(more[0].result.as_ref().unwrap().added, 0, "the follow-up finds nothing new");
        }
        assert_eq!(shared.lock().unwrap().items().len(), 20);
        assert!(!s.is_active("wf") || s.take_finished().is_empty());
    }

    #[test]
    fn cancel_stops_the_pass_and_keeps_the_work_done() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _) = shared_with_folder(tmp.path(), 400, StorageMode::Copy);
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        s.request("wf");
        let until = Instant::now() + Duration::from_secs(30);
        while s.progress().is_none_or(|(_, p)| p.added < 5) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(2));
        }
        let asked = Instant::now();
        s.cancel("wf");
        let done = wait_finished(&s, 1);
        let latency = asked.elapsed();
        let stats = done[0].result.as_ref().unwrap();
        assert!(stats.cancelled && stats.added < 400, "stopped part-way: {}", stats.added);
        assert!(latency < Duration::from_secs(5), "cancel took {latency:?}");
        assert_eq!(shared.lock().unwrap().items().len(), stats.added);
        assert_eq!(shared.lock().unwrap().watched_folders()[0].status, SyncStatus::Idle);
    }

    #[test]
    fn dropping_the_scheduler_stops_the_worker_without_hanging() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _) = shared_with_folder(tmp.path(), 300, StorageMode::Copy);
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        s.request("wf");
        std::thread::sleep(Duration::from_millis(30));
        let started = Instant::now();
        drop(s);
        assert!(started.elapsed() < Duration::from_secs(10), "drop does not wait for a long pass");
        // The abandoned pass ends by itself, in a terminal state, and commits nothing after that.
        let until = Instant::now() + Duration::from_secs(20);
        while shared.lock().unwrap().watched_folders()[0].status == SyncStatus::Syncing && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let n = shared.lock().unwrap().items().len();
        std::thread::sleep(Duration::from_millis(300));
        let lib = shared.lock().unwrap();
        assert!(matches!(lib.watched_folders()[0].status, SyncStatus::Idle | SyncStatus::Success));
        assert_eq!(lib.items().len(), n, "no commits after the scheduler was dropped");
    }

    #[test]
    fn an_unreachable_source_is_reported_offline_not_as_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, src) = shared_with_folder(tmp.path(), 2, StorageMode::Link);
        std::fs::remove_dir_all(&src).unwrap();
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        s.request("wf");
        let done = wait_finished(&s, 1);
        assert!(done[0].result.as_ref().unwrap().offline);
        assert_eq!(shared.lock().unwrap().watched_folders()[0].status, SyncStatus::Offline);
    }

    #[test]
    fn watchers_follow_the_wanted_list_and_ask_for_a_catch_up_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, src) = shared_with_folder(tmp.path(), 3, StorageMode::Copy);
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        let mut w = SyncWatchers::default();
        let wanted = vec![("wf".to_string(), src.clone())];
        assert_eq!(w.reconcile(&s, &wanted), vec!["wf".to_string()]);
        assert_eq!(w.reconcile(&s, &wanted), Vec::<String>::new(), "already watched");
        let done = wait_finished(&s, 1);
        assert_eq!(done[0].result.as_ref().unwrap().added, 3, "caught up on start");
        w.reconcile(&s, &[]);
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn a_source_that_goes_away_and_comes_back_is_resynced_automatically() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, src) = shared_with_folder(tmp.path(), 3, StorageMode::Copy);
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        let mut w = SyncWatchers::default();
        let wanted = vec![("wf".to_string(), src.clone())];
        w.reconcile(&s, &wanted);
        assert_eq!(wait_finished(&s, 1)[0].result.as_ref().unwrap().added, 3);

        let away = tmp.path().join("unplugged");
        std::fs::rename(&src, &away).unwrap();
        let offline = wait_finished(&s, 1);
        assert!(offline[0].result.as_ref().unwrap().offline, "the unplug is recorded");
        assert_eq!(shared.lock().unwrap().watched_folders()[0].status, SyncStatus::Offline);

        std::fs::rename(&away, &src).unwrap();
        image::RgbImage::from_pixel(8, 8, image::Rgb([200, 1, 2])).save(src.join("new.png")).unwrap();
        let back = wait_finished(&s, 1);
        assert_eq!(back[0].result.as_ref().unwrap().added, 1, "the new file came in once it was back");
        assert_eq!(shared.lock().unwrap().watched_folders()[0].status, SyncStatus::Success);
        assert_eq!(shared.lock().unwrap().items().len(), 4);
    }

    #[test]
    fn quiesce_waits_for_the_running_pass_but_never_longer_than_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _) = shared_with_folder(tmp.path(), 600, StorageMode::Copy);
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        s.request("wf");
        let until = Instant::now() + Duration::from_secs(30);
        while s.progress().is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(2));
        }
        let t = Instant::now();
        assert!(s.quiesce(Duration::from_secs(10)), "idle soon after cancel");
        assert!(t.elapsed() < Duration::from_secs(10));
        assert!(!s.is_active("wf"));
        let n = shared.lock().unwrap().items().len();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(shared.lock().unwrap().items().len(), n, "nothing commits after quiesce");
        // No request pending: a scheduler that was quiesced can be dropped at once.
        let d = Instant::now();
        drop(s);
        assert!(d.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn several_folders_share_one_worker_without_overlapping() {
        let tmp = tempfile::tempdir().unwrap();
        let (shared, _src) = shared_with_folder(tmp.path(), 4, StorageMode::Copy);
        {
            let mut lib = shared.lock().unwrap();
            let fid = lib.create_folder(None, "Second").unwrap();
            let src2 = tmp.path().join("src2");
            std::fs::create_dir_all(&src2).unwrap();
            for i in 0..5 {
                image::RgbImage::from_pixel(8, 8, image::Rgb([i, 9, 9])).save(src2.join(format!("q{i}.png"))).unwrap();
            }
            lib.upsert_watched_folder(WatchedFolder {
                id: "wf2".into(),
                source_path: src2,
                folder_id: fid,
                storage_mode: StorageMode::Copy,
                sync_mode: SyncMode::Auto,
                recursive: true,
                last_synced: None,
                status: SyncStatus::Idle,
                last_error: None,
            })
            .unwrap();
        }
        let s = SyncScheduler::new(shared.clone(), tmp.path().join("L.library"), || {});
        s.request("wf");
        s.request("wf2");
        let done = wait_finished(&s, 2);
        assert_eq!(done.len(), 2);
        assert_eq!(done.iter().map(|d| d.result.as_ref().unwrap().added).sum::<usize>(), 9);
        assert_eq!(shared.lock().unwrap().items().len(), 9);
    }
}

