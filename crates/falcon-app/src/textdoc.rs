//! The rules of the text (`.txt`) viewer, kept apart from its drawing so
//! they can be tested: when typing is saved, how a file is read, the text
//! sizes, links, and when a file changed outside is read again.

use std::ops::Range;

/// The file File ▸ New ▸ Text File imports: Eagle's blank template is an
/// empty `Untitled.txt`.
pub const NEW_FILE_NAME: &str = "Untitled.txt";

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

/// Eagle's text sizes: its page scale goes from 62.5% to 187.5% in steps
/// of 31.25%, so the body runs from 14 to 42 points in five sizes.
pub const ZOOM_LEVELS: u8 = 5;

/// The text scale at zoom `level` (0 is the smallest and the default).
pub fn zoom_scale(level: u8) -> f32 {
    1.0 + 0.5 * f32::from(level.min(ZOOM_LEVELS - 1))
}

/// The zoom level one step in (`true`) or out from `level`, kept in range.
pub fn zoom_step(level: u8, zoom_in: bool) -> u8 {
    if zoom_in { (level + 1).min(ZOOM_LEVELS - 1) } else { level.saturating_sub(1) }
}

/// Eagle zooms at most once every 200 ms while the wheel turns.
pub const WHEEL_ZOOM_GAP: f64 = 0.2;

/// How often the open file is checked for changes made outside.
pub const POLL_INTERVAL: f64 = 1.0;

/// What to do about the file on disk when it was modified outside: read it
/// again, unless typing is waiting to be saved (the save wins, as it would
/// in Eagle half a second later), or unless the file became empty while
/// the editor has text (Eagle keeps the text then, as a sync tool may
/// truncate a file before writing it).
pub fn should_reload(changed: bool, typing_pending: bool, disk_empty: bool, editor_empty: bool) -> bool {
    changed && !typing_pending && !(disk_empty && !editor_empty)
}

/// The web links in `text`, as byte ranges: `http://`, `https://` and
/// `www.` addresses starting a word, up to the next space, without
/// trailing punctuation or an unmatched closing bracket.
pub fn find_links(text: &str) -> Vec<Range<usize>> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let Some(start) = ["http://", "https://", "www."].iter().filter_map(|p| lower[at..].find(p).map(|i| at + i)).min() else { break };
        let starts_word = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '/' | '.' | '@' | '-' | '_'));
        let end = text[start..].find(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '`')).map_or(text.len(), |i| start + i);
        let end = trim_link_end(&text[start..end]) + start;
        let prefix = ["http://", "https://", "www."].iter().find(|p| lower[start..].starts_with(*p)).map_or(0, |p| p.len());
        if starts_word && end > start + prefix {
            out.push(start..end);
            at = end;
        } else {
            at = start + prefix.max(1);
        }
    }
    out
}

/// The length of `link` without trailing punctuation, or closing brackets
/// that open nowhere inside it.
fn trim_link_end(link: &str) -> usize {
    let mut end = link.len();
    loop {
        let Some(c) = link[..end].chars().next_back() else { return end };
        let unmatched = |open: char, close: char| c == close && link[..end].matches(open).count() < link[..end].matches(close).count();
        if matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*') || unmatched('(', ')') || unmatched('[', ']') || unmatched('{', '}') {
            end -= c.len_utf8();
        } else {
            return end;
        }
    }
}

/// The address a link opens: `www.` links get `https://`.
pub fn link_target(link: &str) -> String {
    if link.to_ascii_lowercase().starts_with("www.") { format!("https://{link}") } else { link.to_string() }
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

    #[test]
    fn zoom_has_eagles_five_sizes() {
        assert_eq!(zoom_scale(0), 1.0);
        assert_eq!(zoom_scale(4), 3.0);
        assert_eq!(zoom_scale(9), 3.0);
        assert_eq!(zoom_step(0, false), 0);
        assert_eq!(zoom_step(0, true), 1);
        assert_eq!(zoom_step(4, true), 4);
    }

    #[test]
    fn outside_changes_reload_unless_typing_or_truncated() {
        assert!(should_reload(true, false, false, false));
        assert!(!should_reload(false, false, false, false));
        assert!(!should_reload(true, true, false, false), "unsaved typing wins");
        assert!(!should_reload(true, false, true, false), "an emptied file keeps the text");
        assert!(should_reload(true, false, true, true));
    }

    fn links(text: &str) -> Vec<&str> {
        find_links(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn links_are_found_and_trimmed() {
        assert_eq!(links("see https://eagle.cool/a?b=1, then"), ["https://eagle.cool/a?b=1"]);
        assert_eq!(links("(at www.example.com/x_(y))."), ["www.example.com/x_(y)"]);
        assert_eq!(links("HTTP://A.B and http://c.d/e."), ["HTTP://A.B", "http://c.d/e"]);
        assert_eq!(links("[http://x.y/z]"), ["http://x.y/z"]);
        assert_eq!(links("über https://例え.jp/パス 次"), ["https://例え.jp/パス"]);
        assert_eq!(links("<https://a.b>"), ["https://a.b"]);
        // Not links: a bare prefix, or one inside a word or address.
        assert!(links("http:// and www. alone").is_empty());
        assert!(links("abchttp://x.y me@www.x.y").is_empty());
        assert_eq!(link_target("www.a.b"), "https://www.a.b");
        assert_eq!(link_target("http://a.b"), "http://a.b");
    }
}
