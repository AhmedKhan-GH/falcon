//! iCloud Drive originals that are not on disk yet.
//!
//! With "Optimise Mac Storage", iCloud evicts file contents and leaves a
//! *dataless* file: `stat` still reports the full size, but the inode has
//! the `SF_DATALESS` flag (`ls -lO` shows `compressed,dataless`) and any
//! read blocks until iCloud has downloaded the bytes. Older systems leave a
//! hidden `.NAME.icloud` placeholder beside a missing file instead.
//!
//! Nothing here may run a read on the UI thread. `Fetches::ensure` answers
//! from `stat` alone (instant) and, for a dataless file, starts one
//! background thread that asks iCloud for the file
//! (`NSFileManager startDownloadingUbiquitousItemAtURL:error:`) and polls
//! until the flag clears. When the ubiquity API refuses (a dataless file
//! from another file provider), that thread reads the first byte instead,
//! which also makes the provider fetch the file. Downloads are never
//! cancelled or evicted at the system level: cancelling only stops our
//! polling.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::locale::{tr, tr_n, trf};

use eframe::egui;

/// `SF_DATALESS` from `<sys/stat.h>`: the file's contents live in the cloud.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// How long to wait for one file before giving up (a 350 MB book over a
/// slow link takes minutes).
pub const TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// How often the background thread checks the file.
const POLL: Duration = Duration::from_millis(250);

/// Whether `flags` (`st_flags`) mark a dataless file.
pub fn flags_dataless(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

/// The placeholder iCloud leaves for an evicted `dir/NAME`: `dir/.NAME.icloud`.
pub fn placeholder(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    Some(path.with_file_name(format!(".{name}.icloud")))
}

/// Whether to say the file wasn't found: the picture failed to load, the
/// original is not on disk, and it is not waiting on iCloud. (The card
/// tooltip is `logic::broken_thumb_hint`; the viewer uses the same text.)
pub fn missing_hint(failed: bool, exists: bool, dataless: bool) -> bool {
    failed && !exists && !dataless
}

/// [`missing_hint`] for the file at `path`. Only `stat`s, so call it only
/// once a load has failed.
pub fn is_missing(path: &Path) -> bool {
    missing_hint(true, path.exists(), is_dataless(path))
}

/// Whether reading `path` would wait for a download: the file is dataless,
/// or it is missing and iCloud's placeholder stands in for it. Only `stat`s,
/// which never triggers a download.
pub fn is_dataless(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => flags_dataless(st_flags(&m)),
        Err(_) => placeholder(path).is_some_and(|p| p.is_file()),
    }
}

#[cfg(target_os = "macos")]
fn st_flags(m: &std::fs::Metadata) -> u32 {
    std::os::macos::fs::MetadataExt::st_flags(m)
}

#[cfg(not(target_os = "macos"))]
fn st_flags(_: &std::fs::Metadata) -> u32 {
    0
}

/// Where a file stands.
#[derive(Clone, Debug, PartialEq)]
pub enum FetchState {
    /// On disk: reading it won't wait.
    Local,
    /// iCloud is fetching it; `progress` (0..=1) when known.
    Downloading { progress: Option<f32> },
    Failed(String),
}

impl FetchState {
    pub fn is_local(&self) -> bool {
        matches!(self, FetchState::Local)
    }

    /// The line the viewer shows while it waits.
    pub fn message(&self) -> String {
        match self {
            FetchState::Local => String::new(),
            FetchState::Downloading { progress: Some(p) } => trf("Downloading from iCloud… {percent}%", &[("percent", &format!("{:.0}", p.clamp(0.0, 1.0) * 100.0))]),
            FetchState::Downloading { progress: None } => tr("Downloading from iCloud…").into(),
            FetchState::Failed(e) => trf("Couldn't download from iCloud: {e}", &[("e", e)]),
        }
    }
}

/// What one look at the file found.
#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    Local,
    Remote { progress: Option<f32> },
    Error(String),
}

/// The download's state machine, kept free of I/O so it can be tested:
/// it starts `Downloading`, and ends `Local` or `Failed` (an error, a
/// cancel or the timeout); once ended it never changes.
#[derive(Debug)]
pub struct Tracker {
    timeout: Duration,
    state: FetchState,
}

impl Tracker {
    pub fn new(timeout: Duration) -> Self {
        Tracker { timeout, state: FetchState::Downloading { progress: None } }
    }

    pub fn state(&self) -> &FetchState {
        &self.state
    }

    pub fn finished(&self) -> bool {
        !matches!(self.state, FetchState::Downloading { .. })
    }

    /// Takes in one look at the file, `elapsed` since the start.
    pub fn observe(&mut self, seen: Seen, elapsed: Duration, cancelled: bool) -> &FetchState {
        if self.finished() {
            return &self.state;
        }
        self.state = match seen {
            Seen::Local => FetchState::Local,
            Seen::Error(e) => FetchState::Failed(e),
            _ if cancelled => FetchState::Failed(tr("cancelled").into()),
            _ if elapsed >= self.timeout => FetchState::Failed(tr_n((self.timeout.as_secs() / 60) as usize, "no answer after {n} minute", "no answer after {n} minutes")),
            Seen::Remote { progress } => FetchState::Downloading { progress },
        };
        &self.state
    }
}

/// Blocks until `path` is on disk, the download fails, `timeout` passes or
/// `cancel` is set, reporting each state change. Call it off the UI thread.
/// A file already local returns at once without any read.
pub fn ensure_local(path: &Path, timeout: Duration, cancel: &AtomicBool, mut report: impl FnMut(&FetchState)) -> FetchState {
    if !is_dataless(path) {
        return FetchState::Local;
    }
    let start = Instant::now();
    let mut tracker = Tracker::new(timeout);
    report(tracker.state());
    // The ubiquity API first; a provider it doesn't know gets a read of the
    // first byte on its own thread, so cancel and timeout still work.
    let reader = match platform::start_download(path) {
        Ok(()) => None,
        Err(_) => {
            let (tx, rx) = std::sync::mpsc::channel();
            let p = path.to_path_buf();
            std::thread::spawn(move || {
                let _ = tx.send(first_byte(&p));
            });
            Some(rx)
        }
    };
    loop {
        let seen = match &reader {
            None => platform::look(path),
            Some(rx) => match rx.try_recv() {
                Ok(Ok(())) => Seen::Local,
                Ok(Err(e)) => Seen::Error(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => Seen::Remote { progress: None },
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Seen::Error(tr("the read stopped").into()),
            },
        };
        let before = tracker.state().clone();
        tracker.observe(seen, start.elapsed(), cancel.load(Ordering::Relaxed));
        if *tracker.state() != before {
            report(tracker.state());
        }
        if tracker.finished() {
            return tracker.state().clone();
        }
        std::thread::sleep(POLL);
    }
}

fn first_byte(path: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut b = [0u8; 1];
    f.read(&mut b).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    use objc2_foundation::{NSError, NSFileManager, NSString, NSURL, NSURLUbiquitousItemDownloadingErrorKey};

    use super::{Seen, is_dataless};

    fn url(path: &Path) -> objc2::rc::Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    /// Asks iCloud to download the file; an error if it isn't an iCloud item.
    pub fn start_download(path: &Path) -> Result<(), String> {
        let fm = NSFileManager::defaultManager();
        fm.startDownloadingUbiquitousItemAtURL_error(&url(path)).map_err(|e| e.localizedDescription().to_string())
    }

    /// Whether the file is local yet, or iCloud reported an error. A fresh
    /// URL each time, as NSURL caches resource values.
    pub fn look(path: &Path) -> Seen {
        if !is_dataless(path) {
            return Seen::Local;
        }
        let u = url(path);
        let mut value = None;
        // SAFETY: the key's value is an NSError (or nil), read as AnyObject.
        let got = unsafe { u.getResourceValue_forKey_error(&mut value, NSURLUbiquitousItemDownloadingErrorKey) };
        if got.is_ok()
            && let Some(v) = value
            && let Ok(err) = v.downcast::<NSError>()
        {
            return Seen::Error(err.localizedDescription().to_string());
        }
        Seen::Remote { progress: None }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    use super::{Seen, is_dataless};

    pub fn start_download(_: &Path) -> Result<(), String> {
        Err(tr("no iCloud here").into())
    }

    pub fn look(path: &Path) -> Seen {
        if is_dataless(path) { Seen::Remote { progress: None } } else { Seen::Local }
    }
}

struct Job {
    state: Arc<Mutex<FetchState>>,
    cancel: Arc<AtomicBool>,
    /// Started for the viewer: cancelled once the viewer shows something else.
    transient: bool,
}

/// The app's background downloads, one per path.
#[derive(Default)]
pub struct Fetches {
    jobs: HashMap<PathBuf, Job>,
}

impl Fetches {
    /// Where `path` stands. A local file answers `Local` at once from
    /// `stat`; a dataless one starts a background download (once) and
    /// answers its state. Repaints `ctx` as the state changes.
    pub fn ensure(&mut self, ctx: &egui::Context, path: &Path) -> FetchState {
        self.start(ctx, path, true)
    }

    /// Starts downloading `path` if it is dataless, for an action that will
    /// be retried (it is not cancelled when the viewer moves on).
    pub fn prefetch(&mut self, ctx: &egui::Context, path: &Path) -> FetchState {
        self.start(ctx, path, false)
    }

    fn start(&mut self, ctx: &egui::Context, path: &Path, transient: bool) -> FetchState {
        if let Some(j) = self.jobs.get_mut(path) {
            let s = j.state.lock().unwrap().clone();
            j.transient &= transient;
            match s {
                FetchState::Local => {
                    self.jobs.remove(path);
                    return s;
                }
                // An action asking again after a failure tries again.
                FetchState::Failed(_) if !transient => {
                    self.jobs.remove(path);
                }
                _ => return s,
            }
        }
        if !is_dataless(path) {
            return FetchState::Local;
        }
        let state = Arc::new(Mutex::new(FetchState::Downloading { progress: None }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (s, c, p, ctx) = (state.clone(), cancel.clone(), path.to_path_buf(), ctx.clone());
        std::thread::spawn(move || {
            let end = ensure_local(&p, TIMEOUT, &c, |st| {
                *s.lock().unwrap() = st.clone();
                ctx.request_repaint();
            });
            *s.lock().unwrap() = end;
            ctx.request_repaint();
        });
        let now = state.lock().unwrap().clone();
        self.jobs.insert(path.to_path_buf(), Job { state, cancel, transient });
        now
    }

    /// Forgets a failed download so the next `ensure` tries again.
    pub fn retry(&mut self, path: &Path) {
        if let Some(j) = self.jobs.remove(path) {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Stops polling the viewer's downloads other than `keep`'s, and drops
    /// finished ones.
    pub fn prune(&mut self, keep: Option<&Path>) {
        self.jobs.retain(|p, j| {
            if j.transient && Some(p.as_path()) != keep {
                j.cancel.store(true, Ordering::Relaxed);
                return false;
            }
            !j.state.lock().unwrap().is_local()
        });
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn the_missing_hint_needs_a_failed_load_and_a_plain_missing_file() {
        assert!(missing_hint(true, false, false));
        assert!(!missing_hint(false, false, false));
        assert!(!missing_hint(true, true, false));
        assert!(!missing_hint(true, false, true));
    }

    #[test]
    fn a_missing_file_is_told_from_an_icloud_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("a.png");
        assert!(is_missing(&gone));
        std::fs::write(placeholder(&gone).unwrap(), b"").unwrap();
        assert!(!is_missing(&gone));
        std::fs::write(&gone, b"x").unwrap();
        assert!(!is_missing(&gone));
    }

    #[test]
    fn a_download_ends_local() {
        let mut t = Tracker::new(10 * S);
        assert_eq!(t.state(), &FetchState::Downloading { progress: None });
        assert_eq!(t.observe(Seen::Remote { progress: Some(0.4) }, S, false), &FetchState::Downloading { progress: Some(0.4) });
        assert!(!t.finished());
        assert_eq!(t.observe(Seen::Local, 2 * S, false), &FetchState::Local);
        assert!(t.finished());
        // Ended states stay put.
        assert_eq!(t.observe(Seen::Error("late".into()), 3 * S, true), &FetchState::Local);
    }

    #[test]
    fn errors_cancels_and_timeouts_fail() {
        let mut t = Tracker::new(10 * S);
        assert_eq!(t.observe(Seen::Error("offline".into()), S, false), &FetchState::Failed("offline".into()));
        assert_eq!(t.observe(Seen::Local, S, false), &FetchState::Failed("offline".into()));

        let mut t = Tracker::new(10 * S);
        assert_eq!(t.observe(Seen::Remote { progress: None }, S, true), &FetchState::Failed(tr("cancelled").into()));

        let mut t = Tracker::new(10 * S);
        t.observe(Seen::Remote { progress: None }, 9 * S, false);
        assert!(!t.finished());
        assert!(matches!(t.observe(Seen::Remote { progress: None }, 10 * S, false), FetchState::Failed(_)));
        // A file that arrives on the last look still counts.
        let mut t = Tracker::new(10 * S);
        assert_eq!(t.observe(Seen::Local, 11 * S, true), &FetchState::Local);
    }

    #[test]
    fn messages_read_plainly() {
        assert_eq!(FetchState::Downloading { progress: None }.message(), "Downloading from iCloud…");
        assert_eq!(FetchState::Downloading { progress: Some(0.256) }.message(), "Downloading from iCloud… 26%");
        assert_eq!(FetchState::Failed("offline".into()).message(), "Couldn't download from iCloud: offline");
    }

    #[test]
    fn the_dataless_flag_and_placeholders() {
        assert!(flags_dataless(0x4000_0020));
        assert!(!flags_dataless(0x20));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("book.pdf");
        std::fs::write(&f, b"%PDF").unwrap();
        assert!(!is_dataless(&f));
        assert_eq!(placeholder(&f).unwrap(), dir.path().join(".book.pdf.icloud"));
        // A missing file with iCloud's placeholder beside it is remote.
        let gone = dir.path().join("gone.mp4");
        assert!(!is_dataless(&gone));
        std::fs::write(placeholder(&gone).unwrap(), b"").unwrap();
        assert!(is_dataless(&gone));
    }

    #[test]
    fn local_files_finish_off_the_ui_thread_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("clip.mov");
        std::fs::write(&f, b"data").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let p = f.clone();
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let mut reports = 0;
            let end = ensure_local(&p, TIMEOUT, &cancel, |_| reports += 1);
            let _ = tx.send((end, reports));
        });
        // The calling thread only waits on the channel, with a deadline.
        let (end, reports) = rx.recv_timeout(Duration::from_secs(5)).expect("ensure_local returned");
        assert_eq!(end, FetchState::Local);
        assert_eq!(reports, 0, "a local file reports nothing");
    }

    #[test]
    fn fetches_answer_local_files_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.png");
        std::fs::write(&f, b"x").unwrap();
        let ctx = egui::Context::default();
        let mut fetches = Fetches::default();
        let t = Instant::now();
        assert_eq!(fetches.ensure(&ctx, &f), FetchState::Local);
        assert!(t.elapsed() < Duration::from_secs(1));
        assert!(fetches.is_empty(), "no thread for a local file");
    }

    #[test]
    fn a_placeholder_without_a_provider_fails_in_the_background() {
        // A missing file beside a `.icloud` placeholder that iCloud doesn't
        // know: the ubiquity call refuses, the fallback read fails, and the
        // job ends Failed without the caller ever blocking.
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone.epub");
        std::fs::write(placeholder(&gone).unwrap(), b"").unwrap();
        let ctx = egui::Context::default();
        let mut fetches = Fetches::default();
        let t = Instant::now();
        assert_eq!(fetches.ensure(&ctx, &gone), FetchState::Downloading { progress: None });
        assert!(t.elapsed() < Duration::from_secs(1));
        let deadline = Instant::now() + Duration::from_secs(10);
        let end = loop {
            let s = fetches.ensure(&ctx, &gone);
            if !matches!(s, FetchState::Downloading { .. }) || Instant::now() > deadline {
                break s;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(matches!(end, FetchState::Failed(_)), "{end:?}");
        fetches.prune(None);
        assert!(fetches.is_empty());
    }
}
