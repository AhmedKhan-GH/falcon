//! The rules of the text (`.txt`) viewer, kept apart from its drawing so
//! they can be tested: when typing is saved, and how a file is read.

/// Eagle saves half a second after the last key press.
pub const AUTOSAVE_DELAY: f64 = 0.5;

/// Larger files open read-only: egui lays out the whole text every
/// frame it changes, which gets slow well before this.
pub const MAX_EDITABLE: usize = 2 * 1024 * 1024;

/// Waits for typing to pause before saving, as Eagle's editor does: every
/// edit restarts the wait, and the save is due once `AUTOSAVE_DELAY` has
/// passed without one. Times are seconds (egui's input time).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Autosave {
    last_edit: Option<f64>,
}

impl Autosave {
    /// Notes an edit made at `now`.
    pub fn edited(&mut self, now: f64) {
        self.last_edit = Some(now);
    }

    /// Whether the waiting edit should be saved at `now`.
    pub fn due(&self, now: f64) -> bool {
        self.last_edit.is_some_and(|t| now - t >= AUTOSAVE_DELAY)
    }

    /// Seconds until the save is due, for scheduling the next frame.
    pub fn wait(&self, now: f64) -> Option<f64> {
        self.last_edit.map(|t| (t + AUTOSAVE_DELAY - now).max(0.0))
    }

    /// Forgets the waiting edit (it was saved, or there is nothing to save).
    pub fn clear(&mut self) {
        self.last_edit = None;
    }
}

/// Whether `text` should be written over a file holding `saved`. Eagle
/// never saves empty text, so clearing a note doesn't empty the file.
pub fn needs_save(text: &str, saved: &str) -> bool {
    !text.is_empty() && text != saved
}

/// A text file as the viewer shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub text: String,
    /// Why it can't be edited, if it can't: saving would change bytes the
    /// viewer can't show.
    pub read_only: Option<&'static str>,
}

/// Reads a file's bytes for the viewer. UTF-8 (with or without a byte
/// order mark, which is kept) is editable; anything else is shown with
/// the undecodable bytes replaced, read-only, and so are very large files.
pub fn decode(bytes: Vec<u8>) -> Loaded {
    let large = bytes.len() > MAX_EDITABLE;
    match String::from_utf8(bytes) {
        Ok(text) => Loaded { text, read_only: large.then_some("The file is too large to edit here") },
        Err(e) => Loaded {
            text: String::from_utf8_lossy(e.as_bytes()).into_owned(),
            read_only: Some("The file isn't UTF-8 text, so it is shown read-only"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autosave_waits_for_a_pause_in_typing() {
        let mut a = Autosave::default();
        assert!(!a.due(10.0) && a.wait(10.0).is_none());
        a.edited(1.0);
        assert!(!a.due(1.4));
        // Another key press restarts the wait.
        a.edited(1.4);
        assert!(!a.due(1.8));
        assert!(a.due(1.9));
        assert_eq!(a.wait(1.6).map(|w| (w * 10.0).round()), Some(3.0));
        assert_eq!(a.wait(5.0), Some(0.0));
        a.clear();
        assert!(!a.due(5.0) && a.wait(5.0).is_none());
    }

    #[test]
    fn empty_or_unchanged_text_is_not_saved() {
        assert!(needs_save("b", "a"));
        assert!(!needs_save("a", "a"));
        assert!(!needs_save("", "a"), "clearing the text never empties the file");
    }

    #[test]
    fn files_decode_as_utf8_or_read_only() {
        let ok = decode("héllo\n".as_bytes().to_vec());
        assert_eq!(ok, Loaded { text: "héllo\n".into(), read_only: None });
        let bom = decode(b"\xEF\xBB\xBFhi".to_vec());
        assert_eq!(bom.text, "\u{FEFF}hi", "the byte order mark is kept for saving");
        let latin1 = decode(b"caf\xE9".to_vec());
        assert_eq!(latin1.text, "caf\u{FFFD}");
        assert!(latin1.read_only.is_some());
        let big = decode(vec![b'a'; MAX_EDITABLE + 1]);
        assert!(big.read_only.is_some());
    }
}
