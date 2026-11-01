//! Merging duplicate items, as Eagle's duplicate panel does: one item of
//! each group is kept and takes the name, link, note, folders, tags and
//! rating chosen for the group; the others go to the Trash. Nothing is
//! deleted, and the kept item is saved like any other edit, so an open
//! undo transaction can put everything back.

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::error::Result;
use crate::library::Library;
use crate::model::Item;

/// Eagle cuts a joined note to this many characters.
const MERGED_NOTE_LIMIT: usize = 4096;

/// What a group's items offer, for the merge review: each distinct
/// non-empty name, link and note in the group's order, every folder and
/// tag, a note joining all notes when there are several, and the highest
/// rating.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Choices {
    pub names: Vec<String>,
    pub urls: Vec<String>,
    pub notes: Vec<String>,
    pub folders: Vec<String>,
    /// Sorted by name, ignoring case, as Eagle lists them.
    pub tags: Vec<String>,
    /// All notes joined by line breaks, cut to 4096 characters; only when
    /// the items have more than one distinct note.
    pub merged_note: Option<String>,
    pub star: Option<u64>,
}

fn push_unique(list: &mut Vec<String>, s: &str) {
    if !s.is_empty() && !list.iter().any(|x| x == s) {
        list.push(s.to_string());
    }
}

/// An item's rating (1 to 5), if it has one.
fn star(item: &Item) -> Option<u64> {
    item.extra.get("star").and_then(|v| v.as_u64()).filter(|&s| s > 0)
}

impl Choices {
    pub fn of(items: &[&Item]) -> Choices {
        let mut c = Choices::default();
        for item in items {
            push_unique(&mut c.names, &item.name);
            push_unique(&mut c.urls, &item.url);
            push_unique(&mut c.notes, &item.annotation);
            for f in &item.folders {
                push_unique(&mut c.folders, f);
            }
            for t in &item.tags {
                push_unique(&mut c.tags, t);
            }
            c.star = c.star.max(star(item));
        }
        c.tags.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
        if c.notes.len() > 1 {
            c.merged_note = Some(c.notes.join("\n").chars().take(MERGED_NOTE_LIMIT).collect());
        }
        c
    }

    /// Eagle's starting values: the first name, link and note, every
    /// folder and tag, and the highest rating.
    pub fn defaults(&self, keep: &str, trash: Vec<String>) -> Merge {
        Merge {
            keep: keep.to_string(),
            trash,
            name: self.names.first().cloned(),
            url: self.urls.first().cloned().unwrap_or_default(),
            note: self.notes.first().cloned().unwrap_or_default(),
            folders: self.folders.clone(),
            tags: self.tags.clone(),
            star: self.star,
        }
    }
}

/// One group's merge: the item kept, the items trashed, and the values
/// the kept item ends up with.
#[derive(Debug, Clone, PartialEq)]
pub struct Merge {
    pub keep: String,
    pub trash: Vec<String>,
    /// The kept item's new name (`None` keeps its own).
    pub name: Option<String>,
    pub url: String,
    pub note: String,
    pub folders: Vec<String>,
    pub tags: Vec<String>,
    /// The rating; `None` leaves the kept item's own, as Eagle does.
    pub star: Option<u64>,
}

/// What [`Library::merge_duplicates`] did.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MergeReport {
    pub kept: usize,
    pub trashed: usize,
    /// Bytes of the trashed originals.
    pub bytes: u64,
}

impl Library {
    /// Applies merges: each kept item takes its merge's values, and the
    /// others move to the Trash. Items that are gone, already trashed, or
    /// kept by some merge are never trashed, so a mistake can't lose the
    /// copy meant to stay.
    pub fn merge_duplicates(&mut self, merges: &[Merge]) -> Result<MergeReport> {
        let keeps: HashSet<&str> = merges.iter().map(|m| m.keep.as_str()).collect();
        let mut report = MergeReport::default();
        let mut trash = Vec::new();
        for m in merges {
            let Some(mut item) = self.item(&m.keep).cloned() else { continue };
            if let Some(name) = m.name.as_ref().filter(|n| !n.trim().is_empty()) {
                item.name = name.clone();
            }
            item.url = m.url.clone();
            item.annotation = m.note.clone();
            item.folders = m.folders.clone();
            item.tags = m.tags.clone();
            if let Some(s) = m.star {
                item.extra.insert("star".into(), s.into());
            }
            // A kept item that was in the Trash comes back, as it does
            // when Eagle saves it.
            item.is_deleted = false;
            self.update_item(item)?;
            report.kept += 1;
            for id in &m.trash {
                if keeps.contains(id.as_str()) || trash.contains(id) {
                    continue;
                }
                if let Some(it) = self.item(id).filter(|i| !i.is_deleted) {
                    report.bytes += it.size;
                    trash.push(id.clone());
                }
            }
        }
        report.trashed = trash.len();
        self.set_trashed(&trash, true)?;
        Ok(report)
    }
}

/// Where Eagle ranks a format when picking the best of similar pictures;
/// formats not listed come last.
fn format_rank(ext: &str) -> usize {
    const ORDER: [&str; 10] = ["png", "bmp", "jpg", "jpeg", "webp", "avif", "jxl", "heic", "heif", "jfif"];
    ORDER.iter().position(|e| ext.eq_ignore_ascii_case(e)).unwrap_or(ORDER.len())
}

/// Orders similar pictures best first, as Eagle picks the copy to keep:
/// the most pixels, then for the same format the bigger file, else the
/// better format (PNG, BMP, JPG, JPEG, WebP, AVIF, JXL, HEIC, HEIF, JFIF).
/// The sort is stable, so ties keep their order.
pub fn best_first(items: &mut [&Item]) {
    items.sort_by(|a, b| {
        let area = |i: &Item| i.width.saturating_mul(i.height);
        area(b).cmp(&area(a)).then_with(|| {
            if a.ext.eq_ignore_ascii_case(&b.ext) {
                b.size.cmp(&a.size)
            } else {
                match (format_rank(&a.ext), format_rank(&b.ext)) {
                    (x, y) if x == y => Ordering::Equal,
                    (x, y) => x.cmp(&y),
                }
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(v: serde_json::Value) -> Item {
        let mut base = json!({"id": "x", "name": "x", "ext": "png", "size": 1, "width": 10, "height": 10, "tags": [], "folders": [], "isDeleted": false, "url": "", "annotation": "", "modificationTime": 0, "lastModified": 0});
        for (k, val) in v.as_object().unwrap() {
            base[k] = val.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn choices_collect_each_value_once() {
        let a = item(json!({"id": "a", "name": "cat", "url": "https://a", "annotation": "first", "folders": ["F1"], "tags": ["b", "A"], "star": 2}));
        let b = item(json!({"id": "b", "name": "cat copy", "url": "", "annotation": "second", "folders": ["F2", "F1"], "tags": ["A", "c"], "star": 4}));
        let c = item(json!({"id": "c", "name": "cat", "annotation": "first"}));
        let ch = Choices::of(&[&a, &b, &c]);
        assert_eq!(ch.names, ["cat", "cat copy"]);
        assert_eq!(ch.urls, ["https://a"]);
        assert_eq!(ch.notes, ["first", "second"]);
        assert_eq!(ch.folders, ["F1", "F2"]);
        assert_eq!(ch.tags, ["A", "b", "c"]);
        assert_eq!(ch.merged_note.as_deref(), Some("first\nsecond"));
        assert_eq!(ch.star, Some(4));
        let m = ch.defaults("b", vec!["a".into(), "c".into()]);
        assert_eq!((m.name.as_deref(), m.url.as_str(), m.note.as_str(), m.star), (Some("cat"), "https://a", "first", Some(4)));
        // One note: no joined note; no ratings: none.
        let solo = Choices::of(&[&c, &c]);
        assert_eq!(solo.merged_note, None);
        assert_eq!(solo.star, None);
        let long = item(json!({"id": "l", "annotation": "é".repeat(5000)}));
        assert_eq!(Choices::of(&[&a, &long]).merged_note.unwrap().chars().count(), MERGED_NOTE_LIMIT);
    }

    #[test]
    fn the_best_copy_comes_first() {
        let big = item(json!({"id": "big", "ext": "jpg", "width": 200, "height": 100, "size": 5}));
        let png = item(json!({"id": "png", "ext": "png", "width": 100, "height": 100, "size": 1}));
        let jpg = item(json!({"id": "jpg", "ext": "jpg", "width": 100, "height": 100, "size": 9}));
        let jpg_small = item(json!({"id": "jpg2", "ext": "JPG", "width": 100, "height": 100, "size": 3}));
        let gif = item(json!({"id": "gif", "ext": "gif", "width": 100, "height": 100, "size": 99}));
        let mut v = vec![&gif, &jpg_small, &png, &jpg, &big];
        best_first(&mut v);
        let ids: Vec<&str> = v.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["big", "png", "jpg", "jpg2", "gif"]);
    }
}
