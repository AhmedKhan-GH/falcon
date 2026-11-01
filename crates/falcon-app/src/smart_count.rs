//! The smart folder dialog's live count: how many items the rules being
//! edited would show, as Eagle's dialog says under its rules. Counting
//! runs on a worker thread once the rules have stopped changing for
//! [`DEBOUNCE`], over a snapshot of the library taken when the dialog
//! first counts; a result for rules that have changed since is dropped.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use falcon_library::model::Item;
use falcon_library::search::FolderIndex;
use falcon_library::smart::{self, SmartFolder};
use serde_json::Value;

/// How long the rules must stay unchanged before they are counted.
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// What the count runs over.
pub struct Snapshot {
    pub items: Vec<Item>,
    pub folders: FolderIndex,
    pub roots: Vec<SmartFolder>,
}

/// What to do this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing to start; check again after this long (`None`: no need).
    Wait(Option<Duration>),
    /// Count the rules with this key now.
    Start(String),
}

/// The dialog's count state. Cloning a draft starts its count over.
#[derive(Default)]
pub struct LiveCount {
    /// The key of the rules last seen, and when they changed to it.
    key: Option<String>,
    changed: Option<Instant>,
    /// The key last handed to a worker.
    started: Option<String>,
    rx: Option<mpsc::Receiver<(String, usize)>>,
    /// The latest count, and the key it was made for.
    shown: Option<(String, usize)>,
    snapshot: Option<Arc<Snapshot>>,
}

impl Clone for LiveCount {
    fn clone(&self) -> Self {
        LiveCount::default()
    }
}

/// The key for a set of rules inside `parent`: their JSON text, so any
/// edit that changes what is saved changes the key.
pub fn key(conditions: &[Value], parent: Option<&str>) -> String {
    format!("{}\u{0}{}", parent.unwrap_or(""), Value::Array(conditions.to_vec()))
}

impl LiveCount {
    /// Notes the rules shown this frame (`None` while they can't be
    /// saved) and says whether to start counting them.
    pub fn step(&mut self, key: Option<&str>, now: Instant) -> Step {
        let Some(key) = key else {
            self.key = None;
            return Step::Wait(None);
        };
        if self.key.as_deref() != Some(key) {
            self.key = Some(key.to_string());
            // The first rules count at once; later edits wait for a pause.
            self.changed = if self.started.is_none() { None } else { Some(now) };
        }
        if self.started.as_deref() == Some(key) {
            return Step::Wait(None);
        }
        match self.changed {
            Some(at) if now.duration_since(at) < DEBOUNCE => Step::Wait(Some(DEBOUNCE - now.duration_since(at))),
            _ => Step::Start(key.to_string()),
        }
    }

    /// Takes a finished count. Returns true when one arrived.
    pub fn receive(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut got = false;
        while let Ok((k, n)) = rx.try_recv() {
            self.shown = Some((k, n));
            got = true;
        }
        got
    }

    /// The count to show, and whether it is for the rules shown now (false
    /// while a newer count is pending).
    pub fn shown(&self) -> Option<(usize, bool)> {
        let (k, n) = self.shown.as_ref()?;
        Some((*n, self.key.as_deref() == Some(k.as_str())))
    }

    /// Whether rules are shown that have no count yet.
    pub fn counting(&self) -> bool {
        self.key.is_some() && self.shown().is_none_or(|(_, current)| !current)
    }

    /// Counts `conditions` inside `parent` on a worker thread, over the
    /// snapshot (taken with `snapshot` the first time), and asks `ctx` to
    /// redraw when done.
    pub fn start(&mut self, key: String, conditions: Vec<Value>, parent: Option<String>, snapshot: impl FnOnce() -> Snapshot, ctx: &eframe::egui::Context) {
        let snap = self.snapshot.get_or_insert_with(|| Arc::new(snapshot())).clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.started = Some(key.clone());
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("smart-count".into())
            .spawn(move || {
                let n = smart::count_draft(&snap.items, &snap.folders, &snap.roots, parent.as_deref(), &conditions);
                let _ = tx.send((key, n));
                ctx.request_repaint();
            })
            .ok();
    }
}

/// The line under the rules: "1 item found", "N items found".
pub fn found_text(n: usize) -> String {
    crate::locale::tr_n(n, "{n} item found", "{n} items found")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_rules_count_at_once_and_edits_wait_for_a_pause() {
        let t0 = Instant::now();
        let mut c = LiveCount::default();
        assert_eq!(c.step(None, t0), Step::Wait(None));
        assert_eq!(c.step(Some("a"), t0), Step::Start("a".into()));
        c.started = Some("a".into());
        assert_eq!(c.step(Some("a"), t0), Step::Wait(None));
        // An edit waits for the rules to settle; typing on restarts the wait.
        assert_eq!(c.step(Some("ab"), t0), Step::Wait(Some(DEBOUNCE)));
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(c.step(Some("abc"), t1), Step::Wait(Some(DEBOUNCE)));
        assert_eq!(c.step(Some("abc"), t1 + Duration::from_millis(200)), Step::Wait(Some(Duration::from_millis(50))));
        assert_eq!(c.step(Some("abc"), t1 + DEBOUNCE), Step::Start("abc".into()));
    }

    #[test]
    fn a_count_for_older_rules_is_shown_as_stale() {
        let t0 = Instant::now();
        let mut c = LiveCount::default();
        c.step(Some("a"), t0);
        assert!(c.counting());
        c.shown = Some(("a".into(), 3));
        assert_eq!(c.shown(), Some((3, true)));
        assert!(!c.counting());
        c.started = Some("a".into());
        c.step(Some("b"), t0);
        assert_eq!(c.shown(), Some((3, false)));
        assert!(c.counting());
    }

    #[test]
    fn keys_follow_rules_and_parent() {
        let r = [serde_json::json!({"rules": []})];
        assert_ne!(key(&r, None), key(&r, Some("P")));
        assert_ne!(key(&r, None), key(&[], None));
        assert_eq!(found_text(1), "1 item found");
        assert_eq!(found_text(0), "0 items found");
    }
}
