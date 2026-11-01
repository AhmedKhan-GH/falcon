//! Saving a text (`.txt`) item edited in the app's text viewer.
//!
//! Eagle's text editor writes the new text to a temporary file beside the
//! original (`<name>.txt.<ms>`) and moves it over the original, so a
//! half-written file is never left in place. It refuses to save empty
//! text. It leaves the item's `metadata.json` alone, so `size` and
//! `mtime` go stale; falcon refreshes them (and `lastModified`, as every
//! item edit does) so the inspector and size filters stay right. `btime`
//! and `modificationTime` keep their values.
//!
//! A text item also carries the start of its text in the item record, under
//! the `text` key, as Eagle's text importer writes it: the grid draws the
//! card from it instead of a thumbnail (there is no `_thumbnail.png`), and
//! search reads it. [`preview`] makes that value.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::error::{Error, Result};
use crate::library::Library;

/// The item key holding the start of a text item's text.
pub const TEXT_KEY: &str = "text";

/// How much text the record keeps: 32 Ki UTF-16 code units, as Eagle's
/// importer cuts it.
pub const PREVIEW_UNITS: usize = 32 * 1024;

/// The keys of a text item's `metadata.json` in the order Eagle writes
/// them: no `width`, `height` or `palettes`, and `text` just before
/// `lastModified`.
pub const RECORD_KEYS: [&str; 14] =
    ["id", "name", "size", "btime", "mtime", "ext", "tags", "folders", "isDeleted", "url", "annotation", "modificationTime", "text", "lastModified"];

/// The start of `text` that fits in [`PREVIEW_UNITS`] UTF-16 code units,
/// never splitting a character.
pub fn cut(text: &str) -> &str {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        units += c.len_utf16();
        if units > PREVIEW_UNITS {
            return &text[..at];
        }
    }
    text
}

/// The value of [`TEXT_KEY`] for the text file at `path`: its start, read
/// as UTF-8 with invalid bytes replaced, as Eagle reads it.
pub fn preview(path: &Path) -> std::io::Result<String> {
    // Every character takes at most 4 bytes, so this is always enough.
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(PREVIEW_UNITS as u64 * 4).read_to_end(&mut bytes)?;
    Ok(cut(&String::from_utf8_lossy(&bytes)).to_string())
}

impl Library {
    /// Replaces the text of the `.txt` item `id` with `text` and refreshes
    /// its `size` and `mtime`. Empty text is refused, as in Eagle. The
    /// save is not recorded in an open undo transaction: the text has its
    /// own undo in the editor, and reverting only the metadata would leave
    /// it describing a file it no longer matches.
    pub fn save_text(&mut self, id: &str, text: &str) -> Result<()> {
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        if !item.ext.eq_ignore_ascii_case("txt") {
            return Err(self.invalid(format!("{}.{} is not a text file", item.name, item.ext)));
        }
        if text.is_empty() {
            return Err(self.invalid("empty text is not saved"));
        }
        let raw = item.original_path(self.root());
        let stamp = crate::edit::now_ms();
        let tmp = raw.with_file_name(format!("{}.{}.{stamp}", item.name, item.ext));
        let written = (|| -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            fs::rename(&tmp, &raw)
        })();
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(Error::io(&raw, e));
        }
        let meta = fs::metadata(&raw).map_err(|e| Error::io(&raw, e))?;
        item.size = meta.len();
        item.mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(item.mtime, |d| d.as_millis() as u64);
        // The card shows the new text.
        item.extra.insert(TEXT_KEY.into(), cut(text).into());
        let journal = self.journal.take();
        let r = self.update_item(item);
        self.journal = journal;
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn library_with_text(body: &str) -> (tempfile::TempDir, Library, String) {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Notes.txt");
        fs::write(&src, body).unwrap();
        let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
        let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
        (tmp, lib, id)
    }

    #[test]
    fn saving_replaces_the_file_and_refreshes_size_and_mtime() {
        let (_tmp, mut lib, id) = library_with_text("hello");
        let before = lib.item(&id).unwrap().clone();
        lib.save_text(&id, "hello, longer world").unwrap();
        let after = lib.item(&id).unwrap().clone();
        let raw = after.original_path(lib.root());
        assert_eq!(fs::read_to_string(&raw).unwrap(), "hello, longer world");
        assert_eq!(after.size, 19);
        assert!(after.mtime >= before.mtime);
        assert_eq!(after.btime, before.btime, "the birth time is kept");
        assert_eq!(after.modification_time, before.modification_time, "the import time is kept");
        assert!(after.last_modified > before.last_modified);
        // Nothing is left beside the original.
        let names: Vec<String> = fs::read_dir(after.info_dir(lib.root())).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.iter().filter(|n| n.starts_with("Notes.txt")).count(), 1, "{names:?}");
        // And it reads back from disk the same way.
        let reopened = Library::open(lib.root()).unwrap();
        assert_eq!(reopened.item(&id).unwrap().size, 19);
    }

    #[test]
    fn empty_text_and_other_formats_are_refused() {
        let (_tmp, mut lib, id) = library_with_text("keep me");
        assert!(lib.save_text(&id, "").is_err());
        let raw = lib.item(&id).unwrap().original_path(lib.root());
        assert_eq!(fs::read_to_string(raw).unwrap(), "keep me");
        assert!(lib.save_text("nope", "x").is_err());
    }

    #[test]
    fn saving_puts_the_new_text_on_the_card() {
        let (_tmp, mut lib, id) = library_with_text("first");
        assert_eq!(lib.item(&id).unwrap().extra[TEXT_KEY], "first");
        lib.save_text(&id, "second draft").unwrap();
        let reopened = Library::open(lib.root()).unwrap();
        assert_eq!(reopened.item(&id).unwrap().extra[TEXT_KEY], "second draft");
    }

    #[test]
    fn the_preview_stops_at_32_ki_utf16_units_without_splitting_a_character() {
        let ascii = "a".repeat(PREVIEW_UNITS + 10);
        assert_eq!(cut(&ascii).len(), PREVIEW_UNITS);
        // An emoji takes two units: the one that would straddle the limit goes.
        let wide = format!("{}{}", "a".repeat(PREVIEW_UNITS - 1), "😀");
        assert_eq!(cut(&wide), "a".repeat(PREVIEW_UNITS - 1));
        assert_eq!(cut("short"), "short");
    }

    #[test]
    fn the_preview_reads_invalid_bytes_as_replacement_characters() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.txt");
        fs::write(&p, b"ok \xff end").unwrap();
        assert_eq!(preview(&p).unwrap(), "ok \u{fffd} end");
    }

    #[test]
    fn saves_stay_out_of_the_undo_history() {
        let (_tmp, mut lib, id) = library_with_text("one");
        lib.begin_transaction("typing");
        lib.save_text(&id, "two").unwrap();
        assert!(lib.end_transaction().is_none());
    }
}
