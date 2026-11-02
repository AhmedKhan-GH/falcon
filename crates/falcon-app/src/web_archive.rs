//! MHTML archives in the web viewer. WebKit on macOS doesn't open MHTML,
//! so the archive is unpacked (`falcon_library::mhtml`) on a worker into a
//! folder of its own under `<temp>/falcon-mhtml/`, and the viewer loads the
//! page from there with read access to that folder only. The pages carry a
//! Content Security Policy that lets them load only the unpacked files and
//! inline data, so nothing is fetched from the network.
//!
//! The folder is removed when the viewer leaves the item (the first frame
//! that doesn't ask for it), when the web view that owns it goes (a
//! preview window closing) and when falcon quits; folders of a falcon that
//! didn't quit cleanly are swept the next time an archive is opened.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};

use falcon_library::mhtml::{self, Limits, Policy};

/// Where unpacked archives go: `<temp>/falcon-mhtml`.
pub fn root() -> PathBuf {
    std::env::temp_dir().join("falcon-mhtml")
}

/// How old another process's folder must be before a sweep removes it.
const STALE: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);

/// An unpacked archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    /// The root page (`index.html` in the archive's folder).
    pub root: PathBuf,
    /// The page's address and title, as saved in the archive.
    pub address: Option<String>,
    pub subject: Option<String>,
}

/// Where an archive is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Pending,
    Ready(Ready),
    /// It couldn't be unpacked; the reason, for the log.
    Failed(String),
}

type Slot = Arc<Mutex<Option<Result<Ready, String>>>>;

struct Job {
    key: String,
    dir: PathBuf,
    result: Slot,
    cancel: Arc<AtomicBool>,
}

/// The archive one web view shows: at most one is unpacked at a time.
#[derive(Default)]
pub struct Archives {
    job: Option<Job>,
    /// Whether this frame asked for the archive.
    used: bool,
}

static SERIAL: AtomicU64 = AtomicU64::new(0);
static SWEEP: Once = Once::new();

impl Archives {
    /// The archive of the item `key`, whose file is `file`: unpacked on a
    /// worker the first time it is asked for (the one shown before is
    /// removed). `ctx` is woken when it is ready.
    pub fn get(&mut self, key: &str, file: &Path, ctx: &eframe::egui::Context) -> State {
        let ctx = ctx.clone();
        self.get_with(key, file, Limits::default(), move || ctx.request_repaint())
    }

    fn get_with(&mut self, key: &str, file: &Path, limits: Limits, wake: impl FnOnce() + Send + 'static) -> State {
        self.used = true;
        if self.job.as_ref().is_none_or(|j| j.key != key) {
            self.clear();
            self.job = Some(start(key, file, limits, wake));
        }
        let Some(job) = &self.job else { return State::Pending };
        match &*job.result.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
            None => State::Pending,
            Some(Ok(r)) => State::Ready(r.clone()),
            Some(Err(e)) => State::Failed(e.clone()),
        }
    }

    /// Called once a frame after drawing: an archive this frame didn't ask
    /// for is removed.
    pub fn end_frame(&mut self) {
        if !std::mem::take(&mut self.used) {
            self.clear();
        }
    }

    /// Removes the unpacked archive, if any.
    pub fn clear(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::SeqCst);
            let _ = std::fs::remove_dir_all(&job.dir);
        }
    }

    /// The folder of the archive shown, for tests.
    #[cfg(test)]
    fn dir(&self) -> Option<&Path> {
        self.job.as_ref().map(|j| j.dir.as_path())
    }
}

impl Drop for Archives {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Starts unpacking `file` into a new folder under [`root`].
fn start(key: &str, file: &Path, limits: Limits, wake: impl FnOnce() + Send + 'static) -> Job {
    SWEEP.call_once(sweep);
    let pid = std::process::id();
    let n = SERIAL.fetch_add(1, Ordering::Relaxed);
    let dir = root().join(format!("{pid}-{n}"));
    let result: Slot = Arc::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let (slot, stop, path, folder) = (result.clone(), cancel.clone(), file.to_path_buf(), dir.clone());
    let work = move || {
        let out = std::fs::create_dir_all(&folder)
            .map_err(mhtml::MhtmlError::from)
            .and_then(|()| mhtml::unpack_file(&path, &folder, limits, Policy::Offline))
            .map(|u| Ready { root: u.root, address: u.address, subject: u.subject })
            .map_err(|e| e.to_string());
        if stop.load(Ordering::SeqCst) {
            // The viewer moved on while this ran.
            let _ = std::fs::remove_dir_all(&folder);
            return;
        }
        if let Err(e) = &out {
            if !cfg!(test) {
                crate::errlog::error(&format!("web archive {}: {e}", path.display()));
            }
            let _ = std::fs::remove_dir_all(&folder);
        }
        *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(out);
        wake();
    };
    if std::thread::Builder::new().name("falcon-mhtml".into()).spawn(work).is_err() {
        *result.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Err("no thread".into()));
    }
    Job { key: key.to_string(), dir, result, cancel }
}

/// Removes this process's folders (at quit).
pub fn remove_own() {
    let prefix = format!("{}-", std::process::id());
    remove_where(&root(), |name, _| name.starts_with(&prefix));
}

/// Removes folders other falcons left more than [`STALE`] ago.
fn sweep() {
    let prefix = format!("{}-", std::process::id());
    remove_where(&root(), |name, age| !name.starts_with(&prefix) && age.is_some_and(|a| a > STALE));
}

/// Removes the folders in `dir` for which `pick(name, age)` holds.
fn remove_where(dir: &Path, pick: impl Fn(&str, Option<std::time::Duration>) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let age = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok());
        if pick(&name, age) && e.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const SAMPLE: &[u8] = b"MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"B\"\r\n\r\n--B\r\nContent-Type: text/html\r\nContent-Location: https://example.com/\r\n\r\n<html><head></head><body><img src=\"a.png\"></body></html>\r\n--B\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: base64\r\nContent-Location: https://example.com/a.png\r\n\r\niVBORw0KGgo=\r\n--B--\r\n";

    fn wait(a: &mut Archives, key: &str, file: &Path) -> State {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let s = a.get_with(key, file, Limits::default(), || {});
            if s != State::Pending || Instant::now() > end {
                return s;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn folders_live_while_shown_and_go_after() {
        let src = tempfile::tempdir().unwrap();
        let file = src.path().join("page.mhtml");
        std::fs::write(&file, SAMPLE).unwrap();
        let mut a = Archives::default();
        let State::Ready(r) = wait(&mut a, "item1", &file) else { panic!("not unpacked") };
        let dir = a.dir().unwrap().to_path_buf();
        assert!(dir.starts_with(root()), "under falcon's temp folder");
        assert_eq!(r.root, dir.join("index.html"));
        assert!(r.root.is_file() && dir.read_dir().unwrap().count() == 2);
        assert!(!dir.starts_with(src.path()), "nothing is written beside the original");
        // A frame that asked keeps it.
        a.end_frame();
        assert!(dir.is_dir());
        // Another item replaces it.
        let State::Ready(r2) = wait(&mut a, "item2", &file) else { panic!() };
        assert!(!dir.exists(), "the previous archive's folder is removed");
        let dir2 = r2.root.parent().unwrap().to_path_buf();
        assert_ne!(dir, dir2);
        // A frame that didn't ask removes it.
        a.end_frame();
        a.end_frame();
        assert!(!dir2.exists());
        // So does dropping the owner (a preview window closing).
        let mut b = Archives::default();
        let State::Ready(r3) = wait(&mut b, "item3", &file) else { panic!() };
        drop(b);
        assert!(!r3.root.exists());
    }

    #[test]
    fn a_broken_archive_fails_and_leaves_nothing() {
        let src = tempfile::tempdir().unwrap();
        let file = src.path().join("bad.mhtml");
        std::fs::write(&file, b"this is not an archive").unwrap();
        let mut a = Archives::default();
        assert!(matches!(wait(&mut a, "bad", &file), State::Failed(_)));
        assert!(!a.dir().unwrap().exists());
    }

    #[test]
    fn sweeping_spares_this_process_and_recent_folders() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["1-0", "2-0", &format!("{}-0", std::process::id())] {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        }
        let prefix = format!("{}-", std::process::id());
        // Pretend "1-0" is old.
        remove_where(dir.path(), |name, _| !name.starts_with(&prefix) && name == "1-0");
        assert!(!dir.path().join("1-0").exists());
        assert!(dir.path().join("2-0").exists());
        assert!(dir.path().join(format!("{}-0", std::process::id())).exists());
    }
}
