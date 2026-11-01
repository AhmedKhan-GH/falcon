//! Tag edits on selected items and the tag history, as Eagle's tag picker
//! makes them.
//!
//! Eagle's picker adds tags one at a time: each goes onto every selected
//! item that lacks it and then to the front of `historyTags`, so after
//! adding several the last one leads the history. Taking a tag off items
//! also drops it from the history once no item has it any more.

use crate::Result;
use crate::edit::HISTORY_LIMIT;
use crate::library::Library;

/// The longest tag Eagle keeps, in characters.
pub const TAG_MAX_CHARS: usize = 1024;

/// A typed tag as Eagle stores it: trimmed and cut to 1024 characters.
/// `None` when nothing is left.
pub fn clean(tag: &str) -> Option<String> {
    let t: String = tag.trim().chars().take(TAG_MAX_CHARS).collect();
    (!t.is_empty()).then_some(t)
}

/// Eagle's `addHistoryTag`: moves `tag` to the front of the history,
/// which keeps at most 120 tags.
pub fn push_history(history: &mut Vec<String>, tag: &str) {
    if tag.is_empty() {
        return;
    }
    history.retain(|h| h != tag);
    history.insert(0, tag.to_string());
    history.truncate(HISTORY_LIMIT);
}

impl Library {
    /// Adds tags to items, as Eagle's picker does: each tag (cleaned with
    /// [`clean`]) is appended to the items that lack it, then moved to the
    /// front of `historyTags`. Returns how many items changed.
    pub fn add_tags_to_items(&mut self, ids: &[String], tags: &[String]) -> Result<usize> {
        let tags: Vec<String> = tags.iter().filter_map(|t| clean(t)).collect();
        if tags.is_empty() || ids.is_empty() {
            return Ok(0);
        }
        let mut changed = 0;
        for id in ids {
            let Some(mut item) = self.item(id).cloned() else { continue };
            let before = item.tags.len();
            for t in &tags {
                if !item.tags.contains(t) {
                    item.tags.push(t.clone());
                }
            }
            if item.tags.len() != before {
                self.update_item(item)?;
                changed += 1;
            }
        }
        for t in &tags {
            push_history(&mut self.tags.history_tags, t);
        }
        self.save_tags()?;
        Ok(changed)
    }

    /// Takes tags off items, as Eagle's picker and the × on a tag chip do.
    /// A tag that no item outside the Trash has any more also leaves
    /// `historyTags`. Returns how many items changed.
    pub fn remove_tags_from_items(&mut self, ids: &[String], tags: &[String]) -> Result<usize> {
        let mut changed = 0;
        for id in ids {
            let Some(mut item) = self.item(id).cloned() else { continue };
            let before = item.tags.len();
            item.tags.retain(|t| !tags.contains(t));
            if item.tags.len() != before {
                self.update_item(item)?;
                changed += 1;
            }
        }
        let gone: Vec<&String> = tags.iter().filter(|t| !self.items().iter().any(|i| !i.is_deleted && i.tags.contains(t))).collect();
        if changed > 0 && gone.iter().any(|t| self.tags.history_tags.contains(t)) {
            self.tags.history_tags.retain(|h| !gone.contains(&h));
            self.save_tags()?;
        }
        Ok(changed)
    }

    /// Eagle's Paste Tags: appends the copied tags each item lacks, in
    /// order, leaving the history alone. Returns how many items changed.
    pub fn paste_tags(&mut self, ids: &[String], tags: &[String]) -> Result<usize> {
        let mut changed = 0;
        for id in ids {
            let Some(mut item) = self.item(id).cloned() else { continue };
            if crate::autotag::append_new(&mut item.tags, tags) {
                self.update_item(item)?;
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Eagle's Clear Tags: empties the items' tags, leaving the history
    /// alone. Returns how many items had tags.
    pub fn clear_tags(&mut self, ids: &[String]) -> Result<usize> {
        let mut changed = 0;
        for id in ids {
            let Some(mut item) = self.item(id).cloned() else { continue };
            if !item.tags.is_empty() {
                item.tags.clear();
                self.update_item(item)?;
                changed += 1;
            }
        }
        Ok(changed)
    }
}

/// The tag manager's commands on several tags at once.
impl Library {
    /// Eagle's Merge Tags: renames each of `tags` to `into` everywhere
    /// ([`Library::rename_tag`]: items, history, starred tags and groups),
    /// so items keep one copy. Returns how many items changed.
    pub fn merge_tags(&mut self, tags: &[String], into: &str) -> Result<usize> {
        let into = into.trim();
        if into.is_empty() {
            return Ok(0);
        }
        let affected: std::collections::HashSet<String> =
            self.items().iter().filter(|i| i.tags.iter().any(|t| t != into && tags.contains(t))).map(|i| i.id.clone()).collect();
        for t in tags {
            if t != into {
                self.rename_tag(t, into)?;
            }
        }
        Ok(affected.len())
    }

    /// Deletes several tags ([`Library::delete_tag`]). Returns how many
    /// items changed.
    pub fn delete_tags(&mut self, tags: &[String]) -> Result<usize> {
        let affected = self.items().iter().filter(|i| i.tags.iter().any(|t| tags.contains(t))).count();
        for t in tags {
            self.delete_tag(t)?;
        }
        Ok(affected)
    }

    /// Stars several tags, the first one ending up first (Eagle's
    /// `addStarredTags`), or unstars them.
    pub fn star_tags(&mut self, tags: &[String], starred: bool) -> Result<()> {
        self.tags.starred_tags.retain(|t| !tags.contains(t));
        if starred {
            let mut front: Vec<String> = Vec::new();
            for t in tags {
                if !front.contains(t) {
                    front.push(t.clone());
                }
            }
            front.append(&mut self.tags.starred_tags);
            self.tags.starred_tags = front;
        }
        self.save_tags()
    }

    /// Eagle's Add to Group: files tags in a group as well, leaving them in
    /// their other groups (where [`Library::add_tags_to_group`] moves them).
    pub fn add_tags_to_group_too(&mut self, id: &str, tags: &[String]) -> Result<()> {
        let Some(g) = self.metadata.tags_groups.iter_mut().find(|g| g.id == id) else {
            return self.add_tags_to_group(id, tags);
        };
        for t in tags.iter().map(|t| t.replace(['\r', '\n'], "")) {
            if !g.tags.contains(&t) {
                g.tags.push(t);
            }
        }
        self.save_metadata()
    }

    /// Empties `historyTags` (Eagle's Clear Tag History).
    pub fn clear_tag_history(&mut self) -> Result<()> {
        self.tags.history_tags.clear();
        self.save_tags()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::Item;

    pub(crate) fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// A library with items `A1`… holding the given tags.
    pub(crate) fn library(dir: &std::path::Path, items: &[&[&str]]) -> Library {
        let mut lib = Library::create(dir.join("T.library")).unwrap();
        for (n, tags) in items.iter().enumerate() {
            let mut i: Item = serde_json::from_str(r#"{"id":"I","name":"n","size":1,"btime":0,"mtime":0,"ext":"png","tags":[],"folders":[],"isDeleted":false,"url":"","annotation":"","modificationTime":0,"height":0,"width":0,"lastModified":0,"palettes":[]}"#).unwrap();
            i.id = format!("A{}", n + 1);
            i.tags = s(tags);
            std::fs::create_dir_all(i.info_dir(lib.root())).unwrap();
            std::fs::write(i.original_path(lib.root()), b"x").unwrap();
            lib.register_item(i).unwrap();
        }
        lib
    }

    fn tags_json(lib: &Library) -> String {
        std::fs::read_to_string(lib.root().join("tags.json")).unwrap()
    }

    #[test]
    fn history_moves_to_front_and_caps() {
        let mut h = s(&["a", "b", "c"]);
        push_history(&mut h, "c");
        assert_eq!(h, s(&["c", "a", "b"]));
        push_history(&mut h, "");
        assert_eq!(h.len(), 3);
        for n in 0..200 {
            push_history(&mut h, &format!("t{n}"));
        }
        assert_eq!(h.len(), HISTORY_LIMIT);
        assert_eq!(h[0], "t199");
    }

    #[test]
    fn cleaning_trims_and_cuts() {
        assert_eq!(clean("  sky "), Some("sky".into()));
        assert_eq!(clean("   "), None);
        assert_eq!(clean(&"é".repeat(2000)).unwrap().chars().count(), TAG_MAX_CHARS);
    }

    #[test]
    fn adding_appends_and_leads_history_with_the_last_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = library(tmp.path(), &[&["x"], &["b"]]);
        lib.tags.history_tags = s(&["old", "b"]);
        let n = lib.add_tags_to_items(&s(&["A1", "A2"]), &s(&[" a ", "b", "c"])).unwrap();
        assert_eq!(n, 2);
        assert_eq!(lib.item("A1").unwrap().tags, s(&["x", "a", "b", "c"]));
        assert_eq!(lib.item("A2").unwrap().tags, s(&["b", "a", "c"]));
        assert_eq!(lib.tags.history_tags, s(&["c", "b", "a", "old"]));
        // Compact, keys in Eagle's order.
        assert_eq!(tags_json(&lib), r#"{"historyTags":["c","b","a","old"],"starredTags":[]}"#);
        // Tags every item has already still count as used.
        assert_eq!(lib.add_tags_to_items(&s(&["A1"]), &s(&["x"])).unwrap(), 0);
        assert_eq!(lib.tags.history_tags[0], "x");
    }

    #[test]
    fn removing_drops_unused_tags_from_history() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = library(tmp.path(), &[&["a", "b"], &["b"]]);
        lib.tags.history_tags = s(&["a", "b", "z"]);
        assert_eq!(lib.remove_tags_from_items(&s(&["A1"]), &s(&["a", "b"])).unwrap(), 1);
        assert_eq!(lib.item("A1").unwrap().tags, Vec::<String>::new());
        // `b` is still on A2; `z` was not touched.
        assert_eq!(lib.tags.history_tags, s(&["b", "z"]));
        assert_eq!(Library::open(lib.root()).unwrap().tags.history_tags, s(&["b", "z"]));
    }

    #[test]
    fn merging_renames_into_one_name_everywhere() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = library(tmp.path(), &[&["cat", "kitty", "x"], &["kitten"], &["dog"]]);
        lib.record_tag_use(&s(&["kitty", "dog", "cat"])).unwrap();
        lib.star_tags(&s(&["kitten"]), true).unwrap();
        let g = lib.create_tag_group("Pets", &s(&["kitty", "dog", "cat"]), None).unwrap();
        let n = lib.merge_tags(&s(&["cat", "kitty", "kitten"]), " cat ").unwrap();
        assert_eq!(n, 2);
        assert_eq!(lib.item("A1").unwrap().tags, s(&["cat", "x"]));
        assert_eq!(lib.item("A2").unwrap().tags, s(&["cat"]));
        assert_eq!(lib.item("A3").unwrap().tags, s(&["dog"]));
        assert_eq!(lib.tags.history_tags, s(&["dog", "cat"]));
        assert_eq!(lib.tags.starred_tags, s(&["cat"]));
        assert_eq!(lib.metadata.tags_groups.iter().find(|x| x.id == g).unwrap().tags, s(&["dog", "cat"]));
        assert_eq!(lib.merge_tags(&s(&["dog"]), "  ").unwrap(), 0, "an empty name does nothing");
    }

    #[test]
    fn several_tags_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = library(tmp.path(), &[&["a", "b", "c"], &["b"]]);
        lib.star_tags(&s(&["z"]), true).unwrap();
        lib.star_tags(&s(&["a", "b", "a"]), true).unwrap();
        assert_eq!(lib.tags.starred_tags, s(&["a", "b", "z"]));
        lib.star_tags(&s(&["a", "z"]), false).unwrap();
        assert_eq!(lib.tags.starred_tags, s(&["b"]));
        assert_eq!(lib.delete_tags(&s(&["a", "b"])).unwrap(), 2);
        assert_eq!(lib.item("A1").unwrap().tags, s(&["c"]));
        assert!(lib.tags.starred_tags.is_empty());

        let one = lib.create_tag_group("One", &s(&["c"]), None).unwrap();
        let two = lib.create_tag_group("Two", &[], None).unwrap();
        lib.add_tags_to_group_too(&two, &s(&["c", "d\n", "c"])).unwrap();
        let reopened = Library::open(lib.root()).unwrap();
        let tags_of = |id: &str| reopened.metadata.tags_groups.iter().find(|g| g.id == id).unwrap().tags.clone();
        assert_eq!(tags_of(&one), s(&["c"]), "the tag stays in its first group");
        assert_eq!(tags_of(&two), s(&["c", "d"]));
        assert!(lib.add_tags_to_group_too("nope", &s(&["c"])).is_err());

        lib.record_tag_use(&s(&["c"])).unwrap();
        lib.clear_tag_history().unwrap();
        assert_eq!(tags_json(&lib), r#"{"historyTags":[],"starredTags":[]}"#);
    }

    #[test]
    fn paste_appends_without_duplicates_and_clear_empties() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = library(tmp.path(), &[&["b", "x"], &[], &["a", "b"]]);
        lib.record_tag_use(&s(&["h"])).unwrap();
        let all = s(&["A1", "A2", "A3"]);
        assert_eq!(lib.paste_tags(&all, &s(&["a", "b"])).unwrap(), 2);
        assert_eq!(lib.item("A1").unwrap().tags, s(&["b", "x", "a"]));
        assert_eq!(lib.item("A2").unwrap().tags, s(&["a", "b"]));
        assert_eq!(lib.item("A3").unwrap().tags, s(&["a", "b"]));
        assert_eq!(lib.clear_tags(&s(&["A1", "A2", "missing"])).unwrap(), 2);
        let reopened = Library::open(lib.root()).unwrap();
        assert!(reopened.item("A1").unwrap().tags.is_empty());
        assert_eq!(reopened.item("A3").unwrap().tags, s(&["a", "b"]));
        assert_eq!(reopened.tags.history_tags, s(&["h"]), "paste and clear leave the history");
        assert_eq!(lib.clear_tags(&s(&["A1"])).unwrap(), 0);
    }
}
