//! Watching an open library for changes made by other apps, as Eagle polls
//! `metadata.json` and `mtime.json` every 4 seconds. Each open workspace
//! owns a [`Watcher`]: a thread that looks at the two files' modification
//! times and, when one moved, reads and parses that file off the UI
//! thread and hands it over. The frame loop applies it with the library
//! locked (`falcon_library::sync`). A library folder that disappears is
//! reported once.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use falcon_library::model::{LibraryMetadata, MtimeIndex};
use falcon_library::sync::{self, Probe};

/// What changed on disk.
#[derive(Debug)]
pub enum Change {
    Metadata(Box<LibraryMetadata>),
    Mtime(MtimeIndex),
    /// `metadata.json` (or the whole folder) is gone.
    Missing,
}

/// The polling thread for one library; it stops when dropped.
pub struct Watcher {
    rx: Receiver<Change>,
    stop: Arc<AtomicBool>,
}

impl Watcher {
    /// Starts watching `root`, waking the UI with `wake` when something
    /// arrives. What is on disk now is taken as already seen.
    pub fn start(root: PathBuf, wake: impl Fn() + Send + 'static) -> Watcher {
        Self::with_period(root, sync::POLL, wake)
    }

    fn with_period(root: PathBuf, period: Duration, wake: impl Fn() + Send + 'static) -> Watcher {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("falcon-library-watch".into())
            .spawn(move || run(root, period, tx, flag, wake))
            .expect("spawn the library watcher");
        Watcher { rx, stop }
    }

    /// Everything reported since the last call.
    pub fn drain(&self) -> Vec<Change> {
        self.rx.try_iter().collect()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn run(root: PathBuf, period: Duration, tx: Sender<Change>, stop: Arc<AtomicBool>, wake: impl Fn()) {
    let mut seen = Probe::of(&root);
    let mut missing_sent = false;
    let step = Duration::from_millis(100).min(period);
    loop {
        // Sleep in short steps so a closed library's thread ends promptly.
        let mut slept = Duration::ZERO;
        while slept < period {
            if stop.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(step);
            slept += step;
        }
        let now = Probe::of(&root);
        let mut sent = false;
        if !now.present {
            if !missing_sent {
                missing_sent = tx.send(Change::Missing).is_ok();
                sent = true;
            }
        } else {
            missing_sent = false;
            // A file caught half-written doesn't parse; it is read again on
            // the next poll, since it isn't marked as seen.
            if now.metadata != seen.metadata
                && let Ok(m) = sync::read_metadata(&root)
            {
                seen.metadata = now.metadata;
                sent |= tx.send(Change::Metadata(Box::new(m))).is_ok();
            }
            if now.mtime.is_some()
                && now.mtime != seen.mtime
                && let Ok(m) = sync::read_mtime(&root)
            {
                seen.mtime = now.mtime;
                sent |= tx.send(Change::Mtime(m)).is_ok();
            }
        }
        if sent {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_library::Library;
    use std::time::Instant;

    fn wait_for(w: &Watcher, pred: impl Fn(&Change) -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if w.drain().iter().any(&pred) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn reports_outside_changes_and_a_vanished_library() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("W.library");
        let mut lib = Library::create(&root).unwrap();
        let w = Watcher::with_period(root.clone(), Duration::from_millis(50), || {});
        std::thread::sleep(Duration::from_millis(120));
        assert!(w.drain().is_empty(), "nothing changed yet");
        std::thread::sleep(Duration::from_millis(20));
        lib.create_folder(None, "New").unwrap();
        assert!(wait_for(&w, |c| matches!(c, Change::Metadata(m) if m.folders.len() == 1)));
        drop(lib);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wait_for(&w, |c| matches!(c, Change::Missing)));
    }
}
