//! Folder auto-tags: the tags stored in a folder's `tags` key, which Eagle
//! adds to every item put into the folder.
//!
//! Eagle works out, for each folder, the folder's own tags followed by its
//! parent's, its grandparent's and so on up to the top; it keeps the first
//! copy of each tag and then reverses the whole list. That list (the
//! runtime `extendTags`, which Eagle strips before writing `metadata.json`)
//! is what gets applied, so a subfolder's items also get every ancestor's
//! auto-tags. The cached `extendTags` key, when a file has one, is ignored
//! here: the list is always worked out from `tags`.
//!
//! How the list is applied depends on the way the item arrives:
//! importing (or pasting) into a folder appends the tags the item lacks;
//! adding to a folder appends them only when the item was not already in
//! it; dropping onto a folder appends them either way. Tags are never
//! removed and never reordered.

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::{Folder, Item};

/// The folders from the top level down to `id`, or `None` when no folder
/// has that id.
fn chain<'a>(folders: &'a [Folder], id: &str) -> Option<Vec<&'a Folder>> {
    for f in folders {
        if f.id == id {
            return Some(vec![f]);
        }
        if let Some(mut below) = chain(&f.children, id) {
            below.insert(0, f);
            return Some(below);
        }
    }
    None
}

/// The tags Eagle applies to an item put into folder `id`: the folder's
/// own tags, then each ancestor's in turn up to the top level, with only
/// the first copy of each kept, and the result reversed. Empty for an
/// unknown folder.
pub fn folder_auto_tags(folders: &[Folder], id: &str) -> Vec<String> {
    let Some(chain) = chain(folders, id) else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for f in chain.iter().rev() {
        for t in &f.tags {
            if seen.insert(t.as_str()) {
                out.push(t.clone());
            }
        }
    }
    out.reverse();
    out
}

/// The auto-tags for an item going into several folders at once: each
/// folder's list in the order given, first copy of each tag kept.
pub fn auto_tags_for(folders: &[Folder], ids: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        append_new(&mut out, &folder_auto_tags(folders, id));
    }
    out
}

/// Appends the tags in `add` that `tags` lacks, in order. Returns whether
/// any were added.
pub fn append_new(tags: &mut Vec<String>, add: &[String]) -> bool {
    let before = tags.len();
    for t in add {
        if !tags.contains(t) {
            tags.push(t.clone());
        }
    }
    tags.len() != before
}

/// Adds `item` to folder `id` as Eagle's add-to-folder does: when the item
/// is not in the folder yet it joins it, comes out of the Trash and gets
/// the folder's auto-tags. Returns false, with the item untouched, when it
/// was already in the folder.
pub fn join_folder(item: &mut Item, folders: &[Folder], id: &str) -> bool {
    if item.folders.iter().any(|f| f == id) {
        return false;
    }
    item.folders.push(id.to_string());
    item.is_deleted = false;
    append_new(&mut item.tags, &folder_auto_tags(folders, id));
    true
}

/// Ids of folder `id` and every folder below it.
fn subtree_ids(folders: &[Folder], id: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(f) = chain(folders, id).and_then(|c| c.last().copied()) {
        f.walk(&mut |x| {
            out.insert(x.id.clone());
        });
    }
    out
}

impl Library {
    /// Sets folder `id`'s auto-tags (its `tags` key) and, as Eagle's
    /// auto-tag dialog does when the list changed, appends the folder's new
    /// tags to every item in the folder or in any folder below it (trashed
    /// items included). Tags taken off the folder stay on the items.
    /// Duplicates in `tags` are dropped, keeping the first. Only `tags`
    /// changes in `metadata.json`; the folder's `modificationTime` and any
    /// cached `extendTags` are left alone. Returns how many items changed.
    pub fn set_folder_auto_tags(&mut self, id: &str, tags: &[String]) -> Result<usize> {
        let mut clean: Vec<String> = Vec::new();
        append_new(&mut clean, tags);
        let Some(folder) = chain(&self.metadata.folders, id).and_then(|c| c.last().copied()) else {
            return Err(Error::Import { path: self.root().to_path_buf(), reason: format!("no folder {id}") });
        };
        if folder.tags == clean {
            return Ok(0);
        }
        set_tags(&mut self.metadata.folders, id, clean.clone());
        self.save_metadata()?;
        let scope = subtree_ids(&self.metadata.folders, id);
        let changed: Vec<Item> = self
            .items()
            .iter()
            .filter(|i| i.folders.iter().any(|f| scope.contains(f)))
            .filter_map(|i| {
                let mut i = i.clone();
                append_new(&mut i.tags, &clean).then_some(i)
            })
            .collect();
        let n = changed.len();
        for item in changed {
            self.update_item(item)?;
        }
        Ok(n)
    }
}

fn set_tags(folders: &mut [Folder], id: &str, tags: Vec<String>) {
    for f in folders {
        if f.id == id {
            f.tags = tags;
            return;
        }
        set_tags(&mut f.children, id, tags.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, tags: &[&str], children: Vec<Folder>) -> Folder {
        let mut f = Folder::new(id, id, 0);
        f.tags = tags.iter().map(|t| t.to_string()).collect();
        f.children = children;
        f
    }

    fn tree() -> Vec<Folder> {
        vec![
            folder("top", &["a", "b"], vec![folder("mid", &["c", "b"], vec![folder("leaf", &["d", "e"], vec![]), folder("bare", &[], vec![])])]),
            folder("other", &["x"], vec![]),
        ]
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn own_tags_then_ancestors_reversed() {
        let t = tree();
        assert_eq!(folder_auto_tags(&t, "top"), s(&["b", "a"]));
        // [c, b] + [a, b] → first copies [c, b, a] → reversed.
        assert_eq!(folder_auto_tags(&t, "mid"), s(&["a", "b", "c"]));
        // [d, e] + [c, b] + [a, b] → [d, e, c, b, a] → reversed.
        assert_eq!(folder_auto_tags(&t, "leaf"), s(&["a", "b", "c", "e", "d"]));
        // A folder with no tags of its own still inherits.
        assert_eq!(folder_auto_tags(&t, "bare"), s(&["a", "b", "c"]));
        assert!(folder_auto_tags(&t, "missing").is_empty());
    }

    #[test]
    fn several_folders_keep_first_copies() {
        let t = tree();
        assert_eq!(auto_tags_for(&t, &s(&["other", "mid"])), s(&["x", "a", "b", "c"]));
        assert_eq!(auto_tags_for(&t, &s(&["top", "mid"])), s(&["b", "a", "c"]));
        assert!(auto_tags_for(&t, &[]).is_empty());
    }

    #[test]
    fn append_keeps_order_and_skips_present() {
        let mut tags = s(&["b", "z"]);
        assert!(append_new(&mut tags, &s(&["a", "b", "c"])));
        assert_eq!(tags, s(&["b", "z", "a", "c"]));
        assert!(!append_new(&mut tags, &s(&["a", "c"])));
    }

    fn item(folders: &[&str], tags: &[&str]) -> Item {
        let mut i: Item = serde_json::from_str(r#"{"id":"I","name":"n","size":1,"btime":0,"mtime":0,"ext":"png","tags":[],"folders":[],"isDeleted":true,"url":"","annotation":"","modificationTime":0,"height":0,"width":0,"lastModified":0,"palettes":[]}"#).unwrap();
        i.folders = s(folders);
        i.tags = s(tags);
        i
    }

    #[test]
    fn joining_adds_tags_only_once() {
        let t = tree();
        let mut it = item(&["other"], &["c", "mine"]);
        assert!(join_folder(&mut it, &t, "mid"));
        assert_eq!(it.folders, s(&["other", "mid"]));
        assert_eq!(it.tags, s(&["c", "mine", "a", "b"]));
        assert!(!it.is_deleted);
        // Already in the folder: nothing changes, even tags removed since.
        it.tags = s(&["mine"]);
        assert!(!join_folder(&mut it, &t, "mid"));
        assert_eq!(it.tags, s(&["mine"]));
    }

    #[test]
    fn setting_tags_reaches_items_below() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("A.library")).unwrap();
        lib.metadata.folders = tree();
        lib.save_metadata().unwrap();
        for (id, folders) in [("A1", vec!["leaf"]), ("A2", vec!["top"]), ("A3", vec!["other"]), ("A4", vec!["bare"])] {
            let mut i = item(&[], &["k"]);
            i.id = id.into();
            i.folders = s(&folders);
            std::fs::create_dir_all(i.info_dir(lib.root())).unwrap();
            std::fs::write(i.original_path(lib.root()), b"x").unwrap();
            lib.register_item(i).unwrap();
        }
        // The new list goes to items in "mid" and below; "top" is above.
        assert_eq!(lib.set_folder_auto_tags("mid", &s(&["n", "k", "n"])).unwrap(), 2);
        assert_eq!(lib.item("A1").unwrap().tags, s(&["k", "n"]));
        assert_eq!(lib.item("A4").unwrap().tags, s(&["k", "n"]));
        assert_eq!(lib.item("A2").unwrap().tags, s(&["k"]));
        assert_eq!(lib.item("A3").unwrap().tags, s(&["k"]));
        let mid = &lib.metadata.folders[0].children[0];
        assert_eq!((mid.tags.clone(), mid.extend_tags.clone()), (s(&["n", "k"]), None));
        // The same list again changes nothing.
        assert_eq!(lib.set_folder_auto_tags("mid", &s(&["n", "k"])).unwrap(), 0);
        // Taking a tag off leaves it on the items.
        assert_eq!(lib.set_folder_auto_tags("mid", &s(&["k"])).unwrap(), 0);
        assert_eq!(lib.item("A1").unwrap().tags, s(&["k", "n"]));
        let reopened = Library::open(lib.root()).unwrap();
        assert_eq!(reopened.metadata.folders[0].children[0].tags, s(&["k"]));
        assert!(lib.set_folder_auto_tags("missing", &[]).is_err());
    }
}
