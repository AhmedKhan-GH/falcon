//! The status toast: one short message at a time, with how long it stays,
//! an optional Undo button and an optional link to a folder. Drawing lives in
//! `ui/toast.rs`; this module holds the pure model so it can be tested.

use std::time::Instant;

/// Whether the message reports something that went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Error,
}

/// How long a toast stays up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Life {
    /// A quick confirmation (something was copied, a rating was set).
    Brief,
    /// Long enough to read the text.
    Normal,
    /// Carries an Undo button, so it stays a little longer.
    Undoable,
}

/// A clickable folder name at the end of the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    /// The folder id the link opens.
    pub folder: String,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub msg: String,
    pub at: Instant,
    pub kind: Kind,
    pub life: Life,
    pub undo: bool,
    pub link: Option<Link>,
}

const BRIEF: f32 = 1.0;
const UNDOABLE: f32 = 5.0;
const ERROR: f32 = 6.0;

impl Toast {
    pub fn new(msg: impl Into<String>, kind: Kind, life: Life) -> Self {
        Toast { msg: msg.into(), at: Instant::now(), kind, life, undo: false, link: None }
    }
}

/// Seconds the toast stays up; over the viewer, plain notes go quicker.
pub fn seconds(t: &Toast, over_viewer: bool) -> f32 {
    let least = if over_viewer { 1.2 } else { 3.0 };
    let normal = (t.msg.chars().count() as f32 / 20.0).max(least);
    let by_life = match t.life {
        Life::Brief => BRIEF,
        Life::Normal => normal,
        Life::Undoable => normal.max(UNDOABLE),
    };
    match t.kind {
        Kind::Error => by_life.max(ERROR),
        Kind::Info => by_life,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(msg: &str, kind: Kind, life: Life) -> Toast {
        Toast::new(msg, kind, life)
    }

    #[test]
    fn brief_is_about_a_second() {
        let t = toast("Copied 3 tags", Kind::Info, Life::Brief);
        assert_eq!(seconds(&t, false), 1.0);
        assert_eq!(seconds(&t, true), 1.0);
    }

    #[test]
    fn normal_keeps_the_old_rule() {
        let short = toast("Saved", Kind::Info, Life::Normal);
        assert_eq!(seconds(&short, false), 3.0);
        assert_eq!(seconds(&short, true), 1.2);
        let long = toast(&"x".repeat(100), Kind::Info, Life::Normal);
        assert_eq!(seconds(&long, false), 5.0);
        assert_eq!(seconds(&long, true), 5.0);
    }

    #[test]
    fn undoable_is_at_least_five_seconds_and_never_below_normal() {
        let t = toast("Moved 2 to Trash", Kind::Info, Life::Undoable);
        assert_eq!(seconds(&t, false), 5.0);
        assert_eq!(seconds(&t, true), 5.0);
        let long = toast(&"x".repeat(200), Kind::Info, Life::Undoable);
        assert_eq!(seconds(&long, false), 10.0);
    }

    #[test]
    fn errors_last_at_least_six_seconds() {
        let t = toast("Could not save: disk full", Kind::Error, Life::Normal);
        assert_eq!(seconds(&t, false), 6.0);
        let long = toast(&"x".repeat(200), Kind::Error, Life::Normal);
        assert_eq!(seconds(&long, false), 10.0);
    }
}
