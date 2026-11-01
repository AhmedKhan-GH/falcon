//! Folders: tree lookups and counts, dropping items on a folder, selecting rows in
//! the folder tree, and cleaning folder names.

use crate::locale::tr_n;
use std::collections::{HashMap, HashSet};
use falcon_library::model::{Folder, Item};
use super::scope::Scope;

/// Finds a folder by id anywhere in the tree.
pub fn find_folder<'a>(roots: &'a [Folder], id: &str) -> Option<&'a Folder> {
    let mut found = None;
    for r in roots {
        r.walk(&mut |f| {
            if f.id == id {
                found = Some(f);
            }
        });
    }
    found
}

/// A folder's names from the top level down, joined with ` / ` (the
/// folder chip's tooltip, as Eagle titles it).
pub fn folder_path(roots: &[Folder], id: &str) -> Option<String> {
    fn down<'a>(list: &'a [Folder], id: &str, path: &mut Vec<&'a str>) -> bool {
        for f in list {
            path.push(&f.name);
            if f.id == id || down(&f.children, id, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    down(roots, id, &mut path).then(|| path.join(" / "))
}

/// The folder `id` and its ancestors, nearest first and the top-level
/// folder last, as `(id, name)` pairs. Empty when the folder is unknown.
pub fn ancestor_chain(roots: &[Folder], id: &str) -> Vec<(String, String)> {
    fn down(list: &[Folder], id: &str, chain: &mut Vec<(String, String)>) -> bool {
        for f in list {
            chain.push((f.id.clone(), f.name.clone()));
            if f.id == id || down(&f.children, id, chain) {
                return true;
            }
            chain.pop();
        }
        false
    }
    let mut chain = Vec::new();
    if down(roots, id, &mut chain) {
        chain.reverse();
        chain
    } else {
        Vec::new()
    }
}

/// Number of live items placed directly in each folder, as Eagle's sidebar
/// shows them. Folders with no direct items are absent.
pub fn folder_counts(items: &[Item]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for i in items.iter().filter(|i| !i.is_deleted) {
        let unique: HashSet<&String> = i.folders.iter().collect();
        for f in unique {
            *out.entry(f.clone()).or_default() += 1;
        }
    }
    out
}

/// What dropping grid items onto a sidebar folder does, after Eagle's
/// `classifySelected`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderDrop {
    /// Take the items out of these folders (the open one, or every folder
    /// picked in the sidebar) and put them in the target.
    Move(Vec<String>),
    /// Put the items in the target as well, keeping their other folders.
    Add,
}

/// The drop's effect for the open `scope`, with ⌥ held or not. Inside a
/// folder a drop moves the items to the target and ⌥ adds them instead;
/// from every other view (All, a tag, a smart folder, the Trash…) it adds.
/// Dropping on the folder that is open changes nothing (`None`).
#[cfg(test)]
pub fn folder_drop(scope: &Scope, target: &str, alt: bool) -> Option<FolderDrop> {
    folder_drop_picked(scope, &[], target, alt)
}

/// [`folder_drop`] while `picked` folders are selected in the sidebar
/// (⌘- or ⇧-clicks). With two or more picked, a drop without ⌥ takes the
/// items out of every picked folder, as Eagle does with its selected
/// folders, whichever view is open.
pub fn folder_drop_picked(scope: &Scope, picked: &[String], target: &str, alt: bool) -> Option<FolderDrop> {
    match scope {
        _ if picked.len() > 1 && !alt => Some(FolderDrop::Move(picked.to_vec())),
        Scope::Folder(open) if open == target => None,
        Scope::Folder(open) if !alt => Some(FolderDrop::Move(vec![open.clone()])),
        _ => Some(FolderDrop::Add),
    }
}

/// Applies a folder drop to one item: it leaves the open folder on a move,
/// joins the target, gets the target's `auto_tags` it lacks (even when it
/// was in the target already, as in Eagle) and comes out of the Trash.
/// Returns whether the item changed.
pub fn apply_folder_drop(item: &mut Item, target: &str, drop: &FolderDrop, auto_tags: &[String]) -> bool {
    let before = (item.folders.clone(), item.is_deleted, item.tags.len());
    falcon_library::autotag::append_new(&mut item.tags, auto_tags);
    if let FolderDrop::Move(from) = drop {
        item.folders.retain(|f| !from.contains(f));
    }
    if !item.folders.iter().any(|f| f == target) {
        item.folders.push(target.to_string());
    }
    item.is_deleted = false;
    (item.folders.clone(), item.is_deleted, item.tags.len()) != before
}

/// The notice after a folder drop up to the folder name ("Moved 3 items to");
/// the toast shows the name as a link.
pub fn folder_drop_lead(drop: &FolderDrop, n: usize) -> String {
    match drop {
        FolderDrop::Move(_) => tr_n(n, "Moved {n} item to", "Moved {n} items to"),
        FolderDrop::Add => tr_n(n, "Added {n} item to", "Added {n} items to"),
    }
}

/// Several sidebar folders, or several smart folders, picked with ⌘ and ⇧
/// clicks, as in Eagle. The view then shows the items of all of them, and
/// the context menu acts on all of them. Empty means the plain single
/// view of the open scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeSel {
    /// Picked ids, in the order they were picked.
    pub ids: Vec<String>,
    /// Where a ⇧-click range starts: the last row a ⌘-click added (Eagle's
    /// current row). Without one the range starts at the open folder.
    pub anchor: Option<String>,
}

impl TreeSel {
    /// Two or more picked: the batch menu applies.
    pub fn is_multi(&self) -> bool {
        self.ids.len() > 1
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|x| x == id)
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    /// A ⌘-click on `id` while `open` is the folder shown. A first ⌘-click
    /// picks the open folder too; after that the click toggles `id`, but
    /// the last picked row stays picked. Adding a row makes it the anchor.
    pub fn toggle(&mut self, open: Option<&str>, id: &str) {
        if self.ids.is_empty()
            && let Some(o) = open
        {
            self.ids.push(o.to_string());
        }
        match self.ids.iter().position(|x| x == id) {
            Some(at) if self.ids.len() > 1 => {
                self.ids.remove(at);
            }
            Some(_) => {}
            None => {
                self.ids.push(id.to_string());
                self.anchor = Some(id.to_string());
            }
        }
    }

    /// A ⇧-click on `id`: every row of `rows` (the rows shown, top to
    /// bottom) from the anchor, or else the open folder, through `id` joins
    /// the picked ones; the anchor stays. Returns false, changing nothing,
    /// when there is no start row among `rows`, as Eagle ignores the click.
    pub fn extend(&mut self, rows: &[String], open: Option<&str>, id: &str) -> bool {
        let Some(start) = self.anchor.as_deref().or(open) else { return false };
        let (Some(a), Some(b)) = (rows.iter().position(|r| r == start), rows.iter().position(|r| r == id)) else { return false };
        for r in &rows[a.min(b)..=a.max(b)] {
            if !self.contains(r) {
                self.ids.push(r.clone());
            }
        }
        true
    }
}

/// A folder name typed for New Folder with Selection, cleaned as Eagle
/// does on macOS: runs of white space become one space, `:`, `/` and `\`
/// become spaces, and the ends are trimmed.
pub fn clean_folder_name(name: &str) -> String {
    let squeezed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    squeezed.replace([':', '/', '\\'], " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::test_util::fixture;

    #[test]
    fn folder_paths_join_ancestors() {
        let (roots, _) = fixture();
        assert_eq!(folder_path(&roots, "B").as_deref(), Some("A-name / B-name"));
        assert_eq!(folder_path(&roots, "C").as_deref(), Some("C-name"));
        assert_eq!(folder_path(&roots, "Z"), None);
    }

    #[test]
    fn ancestor_chain_runs_from_the_folder_up_to_the_top() {
        let (roots, _) = fixture();
        let ids = |id: &str| ancestor_chain(&roots, id).into_iter().map(|(i, _)| i).collect::<Vec<_>>();
        assert_eq!(ids("B"), ["B", "A"]);
        assert_eq!(ids("C"), ["C"]);
        assert_eq!(ancestor_chain(&roots, "B")[1].1, "A-name");
        assert!(ancestor_chain(&roots, "Z").is_empty());
    }

    #[test]
    fn folder_counts_are_direct_and_skip_trash() {
        let (_, items) = fixture();
        let c = folder_counts(&items);
        assert_eq!(c["A"], 2);
        assert_eq!(c["B"], 2);
        assert!(!c.contains_key("C"));
    }
}

#[cfg(test)]
mod folder_drop_tests {
    use super::*;
    use crate::logic::scope::Scope;
    use serde_json::json;

    fn item(folders: &[&str], deleted: bool) -> Item {
        let v = json!({
            "id": "I1", "name": "a", "size": 1, "btime": 0, "mtime": 0, "ext": "png",
            "tags": [], "folders": folders, "isDeleted": deleted, "url": "",
            "annotation": "", "modificationTime": 0, "height": 10, "width": 10,
            "lastModified": 0, "palettes": []
        });
        Item::from_json_str(&v.to_string()).unwrap()
    }

    #[test]
    fn a_folder_moves_and_alt_adds() {
        let open = Scope::Folder("A".into());
        assert_eq!(folder_drop(&open, "B", false), Some(FolderDrop::Move(vec!["A".into()])));
        assert_eq!(folder_drop(&open, "B", true), Some(FolderDrop::Add));
    }

    #[test]
    fn other_views_add() {
        for scope in [Scope::All, Scope::Uncategorized, Scope::Untagged, Scope::Recent, Scope::Random(3), Scope::Trash, Scope::Tag("t".into()), Scope::Smart("S".into())] {
            assert_eq!(folder_drop(&scope, "B", false), Some(FolderDrop::Add), "{scope:?}");
            assert_eq!(folder_drop(&scope, "B", true), Some(FolderDrop::Add), "{scope:?}");
        }
    }

    #[test]
    fn dropping_on_the_open_folder_does_nothing() {
        let open = Scope::Folder("A".into());
        assert_eq!(folder_drop(&open, "A", false), None);
        assert_eq!(folder_drop(&open, "A", true), None);
    }

    #[test]
    fn applying_a_move() {
        let mut it = item(&["A", "C"], false);
        assert!(apply_folder_drop(&mut it, "B", &FolderDrop::Move(vec!["A".into()]), &[]));
        assert_eq!(it.folders, ["C", "B"]);
        // Already there and not in the open folder: nothing to write.
        assert!(!apply_folder_drop(&mut it, "B", &FolderDrop::Move(vec!["A".into()]), &[]));
        assert_eq!(it.folders, ["C", "B"]);
    }

    #[test]
    fn applying_an_add() {
        let mut it = item(&["A"], false);
        assert!(apply_folder_drop(&mut it, "B", &FolderDrop::Add, &[]));
        assert_eq!(it.folders, ["A", "B"]);
        assert!(!apply_folder_drop(&mut it, "B", &FolderDrop::Add, &[]));
        // A trashed item already in the folder still comes back.
        let mut gone = item(&["B"], true);
        assert!(apply_folder_drop(&mut gone, "B", &FolderDrop::Add, &[]));
        assert!(!gone.is_deleted);
        assert_eq!(gone.folders, ["B"]);
    }

    #[test]
    fn a_drop_adds_auto_tags_even_when_already_filed() {
        let tags = ["k".to_string(), "new".to_string()];
        let mut it = item(&["B"], false);
        it.tags = vec!["k".into()];
        // Nothing moves, but the missing auto-tag is still added.
        assert!(apply_folder_drop(&mut it, "B", &FolderDrop::Add, &tags));
        assert_eq!((it.folders.as_slice(), it.tags.as_slice()), (["B".to_string()].as_slice(), tags.as_slice()));
        assert!(!apply_folder_drop(&mut it, "B", &FolderDrop::Add, &tags));
    }

    #[test]
    fn several_picked_folders_all_lose_the_items() {
        let picked = vec!["A".to_string(), "C".to_string()];
        let open = Scope::Folder("A".into());
        assert_eq!(folder_drop_picked(&open, &picked, "B", false), Some(FolderDrop::Move(picked.clone())));
        // ⌥ still only adds; one picked folder is the plain rule.
        assert_eq!(folder_drop_picked(&open, &picked, "B", true), Some(FolderDrop::Add));
        assert_eq!(folder_drop_picked(&open, &picked[..1], "A", false), None);
        // Dropping on one of the picked folders: out of the others, kept in it.
        let mut it = item(&["A", "C", "D"], false);
        assert!(apply_folder_drop(&mut it, "C", &FolderDrop::Move(picked), &[]));
        assert_eq!(it.folders, ["D", "C"]);
    }

    #[test]
    fn notices() {
        assert_eq!(folder_drop_lead(&FolderDrop::Move(vec!["A".into()]), 3), "Moved 3 items to");
        assert_eq!(folder_drop_lead(&FolderDrop::Add, 1), "Added 1 item to");
    }
}

#[cfg(test)]
mod tree_sel_tests {
    use super::*;

    fn rows(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_click_picks_the_open_folder_then_toggles() {
        let mut s = TreeSel::default();
        s.toggle(Some("A"), "C");
        assert_eq!(s.ids, ["A", "C"]);
        assert_eq!(s.anchor.as_deref(), Some("C"));
        s.toggle(Some("A"), "B");
        assert_eq!(s.ids, ["A", "C", "B"]);
        // Toggling off keeps the anchor where it was.
        s.toggle(Some("A"), "C");
        assert_eq!(s.ids, ["A", "B"]);
        assert_eq!(s.anchor.as_deref(), Some("B"));
        s.toggle(Some("A"), "A");
        assert_eq!(s.ids, ["B"]);
        // The last one can't be taken away.
        s.toggle(Some("A"), "B");
        assert_eq!(s.ids, ["B"]);
        assert!(!s.is_multi());
        // ⌘-clicking the open folder alone picks only it.
        let mut s = TreeSel::default();
        s.toggle(Some("A"), "A");
        assert_eq!(s.ids, ["A"]);
        assert_eq!(s.anchor, None);
    }

    #[test]
    fn shift_click_adds_the_rows_between() {
        let shown = rows(&["A", "A1", "A2", "B", "C"]);
        let mut s = TreeSel::default();
        assert!(s.extend(&shown, Some("A1"), "B"));
        assert_eq!(s.ids, ["A1", "A2", "B"]);
        // Upwards from the open folder, adding to what is picked.
        assert!(s.extend(&shown, Some("A1"), "A"));
        assert_eq!(s.ids, ["A1", "A2", "B", "A"]);
        // From the ⌘-click anchor rather than the open folder.
        let mut s = TreeSel::default();
        s.toggle(Some("A"), "C");
        assert!(s.extend(&shown, Some("A"), "A2"));
        assert_eq!(s.ids, ["A", "C", "A2", "B"]);
        assert_eq!(s.anchor.as_deref(), Some("C"));
    }

    #[test]
    fn shift_click_without_a_start_row_does_nothing() {
        let shown = rows(&["A", "B"]);
        let mut s = TreeSel::default();
        assert!(!s.extend(&shown, None, "B"));
        // The open folder is hidden (collapsed or filtered out).
        assert!(!s.extend(&shown, Some("Z"), "B"));
        assert!(s.ids.is_empty());
        s.clear();
        assert_eq!(s, TreeSel::default());
    }
}

#[cfg(test)]
mod clean_name_tests {
    use super::*;

    #[test]
    fn folder_names_are_cleaned_like_eagle() {
        assert_eq!(clean_folder_name("  Mood\tboard   2026 "), "Mood board 2026");
        assert_eq!(clean_folder_name("a/b:c\\d"), "a b c d");
        assert_eq!(clean_folder_name("a / b"), "a   b", "slashes go after the spaces are squeezed");
        assert_eq!(clean_folder_name(" / "), "");
    }
}
