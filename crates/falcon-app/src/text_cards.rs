//! Text items' cards. Eagle draws a `.txt` item in the grid as its text,
//! not as a picture: the item's name as a heading, then the first lines,
//! each trimmed. The text comes from the item record's `text` key
//! (`falcon_library::text`); an item without one (imported by an older
//! falcon) has the start of its file read once and kept here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use falcon_library::model::Item;

/// Lines a card shows at most, as Eagle's card template keeps.
pub const MAX_LINES: usize = 50;

/// Below this side (points) a card is too small for text; it shows the
/// file-type placeholder instead.
pub const MIN_SIDE: f32 = 60.0;

/// Type sizes and margins of a text card, by its width: Eagle shrinks the
/// text and the margins on small and middle-sized boxes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub body: f32,
    pub heading: f32,
    /// Left and right margin.
    pub side: f32,
    pub top: f32,
}

pub fn look(width: f32) -> Look {
    if width < 160.0 {
        Look { body: 11.0, heading: 13.0, side: width * 0.08, top: width * 0.05 }
    } else if width < 240.0 {
        Look { body: 12.0, heading: 14.0, side: width * 0.08, top: width * 0.08 }
    } else {
        Look { body: 14.0, heading: 16.0, side: 20.0, top: 20.0 }
    }
}

/// The lines a card shows: the first [`MAX_LINES`] lines of `text`, each
/// trimmed (blank ones stay, as empty paragraphs).
pub fn lines(text: &str) -> Vec<&str> {
    text.split('\n').take(MAX_LINES).map(str::trim).collect()
}

/// Text read from files for items whose record has no `text`, keyed by
/// the original's path and modification time so an edit is picked up.
#[derive(Default)]
pub struct TextCards {
    read: HashMap<(PathBuf, u64), Arc<str>>,
}

/// How many files' text is kept before the cache starts over.
const CAPACITY: usize = 2000;

/// The text kept in the item's record, if any.
pub fn record_text(item: &Item) -> Option<&str> {
    item.extra.get(falcon_library::text::TEXT_KEY).and_then(|v| v.as_str())
}

impl TextCards {
    /// The start of the file of `item` (in the library at `root`), for an
    /// item whose record has no text ([`record_text`]).
    pub fn read(&mut self, item: &Item, root: &Path) -> Arc<str> {
        let path = item.original_path(root);
        let key = (path, item.mtime);
        if let Some(t) = self.read.get(&key) {
            return t.clone();
        }
        // A file still in iCloud isn't downloaded for a card.
        let text: Arc<str> = if crate::icloud::is_dataless(&key.0) {
            Arc::from("")
        } else {
            Arc::from(falcon_library::text::preview(&key.0).unwrap_or_default())
        };
        if self.read.len() >= CAPACITY {
            self.read.clear();
        }
        self.read.insert(key, text.clone());
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_show_the_first_fifty_trimmed_lines() {
        assert_eq!(lines("  One \n\nTwo\t\r\n"), ["One", "", "Two", ""]);
        let many: String = (0..80).map(|n| format!("{n}\n")).collect();
        let shown = lines(&many);
        assert_eq!(shown.len(), MAX_LINES);
        assert_eq!(shown.last(), Some(&"49"));
    }

    #[test]
    fn small_cards_use_smaller_type_and_margins() {
        assert_eq!(look(400.0), Look { body: 14.0, heading: 16.0, side: 20.0, top: 20.0 });
        assert_eq!(look(200.0).body, 12.0);
        assert_eq!(look(100.0).heading, 13.0);
        assert!(look(100.0).side < 20.0);
    }

    #[test]
    fn files_are_read_once_per_modification() {
        let tmp = tempfile::tempdir().unwrap();
        let info = tmp.path().join("images").join("A1.info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("Note.txt"), "from the file").unwrap();
        let mut item: Item = serde_json::from_value(serde_json::json!({
            "id": "A1", "name": "Note", "size": 13, "btime": 1, "mtime": 1, "ext": "txt", "tags": [], "folders": [],
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": 1, "lastModified": 1
        }))
        .unwrap();
        let mut cards = TextCards::default();
        assert_eq!(record_text(&item), None);
        assert_eq!(&*cards.read(&item, tmp.path()), "from the file");
        std::fs::write(info.join("Note.txt"), "changed").unwrap();
        assert_eq!(&*cards.read(&item, tmp.path()), "from the file", "kept until the mtime changes");
        item.mtime = 2;
        assert_eq!(&*cards.read(&item, tmp.path()), "changed");
        item.extra.insert("text".into(), "from the record".into());
        assert_eq!(record_text(&item), Some("from the record"));
    }
}
