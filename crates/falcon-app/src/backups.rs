//! When the app backs up an open library's `metadata.json`.
//!
//! Eagle backs up a second after a change (it debounces its `mtime.json`
//! rewrite by 1 s and backs up right after it) and every 30 minutes while a
//! library is open. Each open workspace keeps a [`Schedule`]; the frame
//! loop asks it whether a backup is due and runs
//! [`falcon_library::backup::backup_library`] on a background thread, so
//! the copy never blocks the window. The schedule takes the time as an
//! argument and owns no timer, so tests drive it directly.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long after the last write a backup waits.
pub const DEBOUNCE: Duration = Duration::from_secs(1);
/// Backups also run this often while a library is open.
pub const PERIOD: Duration = Duration::from_secs(30 * 60);

/// One open library's backup timers.
#[derive(Debug)]
pub struct Schedule {
    /// When the debounced backup after a write is due.
    pending: Option<Instant>,
    /// When the next periodic backup is due.
    periodic: Instant,
    /// Set while a background backup runs, so two never overlap.
    running: Arc<AtomicBool>,
}

impl Schedule {
    pub fn new(now: Instant) -> Self {
        Schedule { pending: None, periodic: now + PERIOD, running: Arc::new(AtomicBool::new(false)) }
    }

    /// A write to `mtime.json` or `metadata.json` happened at `now`: back
    /// up [`DEBOUNCE`] later, restarting the wait on every further write.
    pub fn changed(&mut self, now: Instant) {
        self.pending = Some(now + DEBOUNCE);
    }

    /// Whether a write is waiting for its backup.
    #[cfg(test)]
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether a backup should start at `now`. If so, both timers restart
    /// and the schedule counts as running until the returned guard is
    /// dropped. While one runs, nothing starts; a due backup waits for it.
    pub fn start(&mut self, now: Instant) -> Option<Running> {
        let due = self.pending.is_some_and(|t| t <= now) || self.periodic <= now;
        if !due || self.running.swap(true, Ordering::AcqRel) {
            return None;
        }
        self.pending = None;
        self.periodic = now + PERIOD;
        Some(Running(self.running.clone()))
    }

    /// How long until [`Schedule::start`] may next return a backup.
    pub fn wait(&self, now: Instant) -> Duration {
        let next = self.pending.map_or(self.periodic, |t| t.min(self.periodic));
        next.saturating_duration_since(now)
    }

    /// Takes the pending backup now, ignoring the debounce, for a library
    /// that is closing. Returns false when nothing was pending.
    pub fn take_pending(&mut self) -> bool {
        self.pending.take().is_some()
    }
}

/// Marks a [`Schedule`] as running until dropped.
#[derive(Debug)]
pub struct Running(Arc<AtomicBool>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Backs up the library at `root` on a background thread.
pub fn spawn(root: PathBuf, running: Running) {
    std::thread::spawn(move || {
        if let Err(e) = falcon_library::backup::backup_library(&root) {
            eprintln!("falcon: metadata backup of {} failed: {e}", root.display());
        }
        drop(running);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_debounced() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        assert!(s.start(t0).is_none());
        s.changed(t0);
        assert!(s.start(t0 + DEBOUNCE / 2).is_none());
        // Another write restarts the wait.
        s.changed(t0 + DEBOUNCE / 2);
        assert!(s.start(t0 + DEBOUNCE).is_none());
        assert_eq!(s.wait(t0 + DEBOUNCE), DEBOUNCE / 2);
        let run = s.start(t0 + DEBOUNCE * 2);
        assert!(run.is_some());
        assert!(!s.has_pending());
    }

    #[test]
    fn runs_every_period_without_writes() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        assert_eq!(s.wait(t0), PERIOD);
        assert!(s.start(t0 + PERIOD - DEBOUNCE).is_none());
        drop(s.start(t0 + PERIOD).expect("periodic backup"));
        // The period restarts from the last backup, including one after a write.
        assert!(s.start(t0 + PERIOD + DEBOUNCE).is_none());
        s.changed(t0 + PERIOD * 3 / 2);
        drop(s.start(t0 + PERIOD * 3 / 2 + DEBOUNCE).expect("debounced backup"));
        assert_eq!(s.wait(t0 + PERIOD * 3 / 2 + DEBOUNCE), PERIOD);
    }

    #[test]
    fn never_overlaps_a_running_backup() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        s.changed(t0);
        let run = s.start(t0 + DEBOUNCE).unwrap();
        s.changed(t0 + DEBOUNCE);
        assert!(s.start(t0 + DEBOUNCE * 3).is_none(), "still running");
        assert!(s.has_pending(), "the write keeps waiting");
        drop(run);
        assert!(s.start(t0 + DEBOUNCE * 3).is_some());
    }

    #[test]
    fn closing_takes_the_pending_backup() {
        let t0 = Instant::now();
        let mut s = Schedule::new(t0);
        assert!(!s.take_pending());
        s.changed(t0);
        assert!(s.take_pending());
        assert!(!s.has_pending());
    }

    #[test]
    fn backup_library_copies_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = falcon_library::Library::create(tmp.path().join("T.library")).unwrap();
        let made = falcon_library::backup::backup_library(lib.root()).unwrap().expect("first backup");
        assert_eq!(std::fs::read(made).unwrap(), std::fs::read(lib.root().join("metadata.json")).unwrap());
        // Unchanged size: skipped.
        assert_eq!(falcon_library::backup::backup_library(lib.root()).unwrap(), None);
    }
}
