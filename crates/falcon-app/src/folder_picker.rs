//! The rules of the Add to Folders picker (Find ▸ Add to Folders…, ⇧⌘J):
//! which rows each tab lists for the search typed, which row the keyboard
//! highlights, and what closing the picker changes. The dialog is drawn by
//! `ui/dialogs/add_to_folders.rs`.
//!
//! Like Eagle's, the picker starts with every folder the items all share
//! ticked; ticking adds the items to a folder and unticking a shared
//! folder takes them out of it. Nothing is applied until the picker
//! closes, and then only if a tick changed. The All tab lists the recent
//! and ticked folders above the tree (top level expanded), the Recent tab
//! only those, and the Selected tab the ticked folders. A search lists
//! the folders whose name contains it, flat, and offers to create a folder
//! with that name when none is called exactly that.
//!
//! The list is built when the picker opens and again when the tab, the
//! search or the expanded folders change, not when a tick changes, so
//! rows do not jump while the user ticks them (as Eagle's).

use std::collections::HashSet;

use falcon_library::model::Folder;

/// Recently used folders the picker lists (as Eagle, the first eight).
pub const RECENT_SHOWN: usize = 8;

/// Where a folder made from the picker goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewPlace {
    /// First on the top level (the heading's + and Create, as Eagle).
    Top,
    /// Last inside this folder.
    Inside(String),
    /// Right after this folder, under the same parent.
    After(String),
}

/// One row of the picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A recently used folder (id).
    Recent(String),
    /// The "Folders" heading above the tree (the action editor's list).
    Heading,
    /// A line between the recent folders and the tree.
    Separator,
    /// A folder of the tree, at a depth; `kids` when it has visible
    /// children to expand.
    Folder { id: String, depth: usize, kids: bool },
    /// Create a folder named as the search.
    Create,
}

impl Row {
    /// Whether the keyboard highlight can stop on the row.
    pub fn pickable(&self) -> bool {
        !matches!(self, Row::Heading | Row::Separator)
    }
}

/// The picker's tabs, in Eagle's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    All,
    Recent,
    Selected,
}

impl Tab {
    /// The tab Tab switches to: All, Recent, Selected, then All again.
    pub fn next(self) -> Tab {
        match self {
            Tab::All => Tab::Recent,
            Tab::Recent => Tab::Selected,
            Tab::Selected => Tab::All,
        }
    }
}

/// Rows of the Recent tab at most (Eagle's 20).
pub const RECENT_TAB_SHOWN: usize = 20;
/// Rows of recent folders above the tree on the All tab, before one more
/// is allowed for each ticked folder (Eagle's 5).
pub const RECENT_ABOVE_TREE: usize = 5;

/// The picker as filled in so far.
#[derive(Clone, Debug, Default)]
pub struct Picker {
    /// The items being filed.
    pub items: Vec<String>,
    /// The folder being viewed, if any.
    pub current: Option<String>,
    /// Folders every item was already in.
    pub existing: Vec<String>,
    /// Folders ticked.
    pub checked: HashSet<String>,
    /// Folders whose children show (without a search).
    pub expanded: HashSet<String>,
    pub search: String,
    pub tab: Tab,
    /// Take the items out of `current` on closing (Eagle's Add to Folder
    /// dialog has this box; its tabbed panel doesn't).
    pub remove_from_current: bool,
    /// The rows listed; rebuilt by `refresh`.
    pub listed: Vec<Row>,
    /// `listed` is out of date: the tab, search or expanded folders changed.
    pub dirty: bool,
    /// The highlighted row's index in `listed`; none until the keyboard or
    /// the pointer picks one (or a search finds something).
    pub highlight: Option<usize>,
    /// Where the pointer was when the picker opened (window points); the
    /// panel opens beside it.
    pub anchor: Option<(f32, f32)>,
    /// A new folder being named: where it goes, and the name so far.
    pub naming: Option<(NewPlace, String)>,
    /// The keyboard moved the highlight: scroll it into view.
    pub scroll: bool,
    /// The next `refresh` keeps the highlight.
    pub keep_highlight: bool,
}

impl Picker {
    /// A picker for `items`, which all share `existing`; top-level
    /// folders start expanded.
    pub fn new(items: Vec<String>, current: Option<String>, existing: Vec<String>, roots: &[Folder]) -> Self {
        Picker {
            items,
            current,
            checked: existing.iter().cloned().collect(),
            existing,
            expanded: roots.iter().map(|f| f.id.clone()).collect(),
            dirty: true,
            ..Default::default()
        }
    }

    /// Ticks or unticks a folder.
    pub fn toggle(&mut self, id: &str) {
        if !self.checked.remove(id) {
            self.checked.insert(id.to_string());
        }
    }

    /// A folder chosen from the list: ticks or unticks it. A search is
    /// cleared by it unless `keep_search` (⌘ held), as in Eagle.
    pub fn pick(&mut self, id: &str, keep_search: bool) {
        self.toggle(id);
        if !keep_search && !self.search.is_empty() {
            self.search.clear();
            self.dirty = true;
        }
    }

    /// Shows another tab.
    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.dirty = true;
    }

    /// Opens or closes a folder of the tree; the highlight stays.
    pub fn set_expanded(&mut self, id: &str, open: bool) {
        let changed = if open { self.expanded.insert(id.to_string()) } else { self.expanded.remove(id) };
        if changed {
            self.dirty = true;
            self.keep_highlight = true;
        }
    }

    /// Rebuilds `listed` if something that changes it happened. With a
    /// search the first row is highlighted; without one nothing is, unless
    /// only folders were opened or closed.
    pub fn refresh(&mut self, roots: &[Folder], recent: &[String], shut: impl Fn(&Folder) -> bool) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.listed = tab_rows(self, roots, recent, shut);
        if std::mem::take(&mut self.keep_highlight) {
            self.highlight = self.highlight.filter(|i| *i < self.listed.len());
        } else {
            self.highlight = if self.search.trim().is_empty() { None } else { self.listed.iter().position(Row::pickable) };
        }
    }

    /// The folders to add the items to and to take them out of, in tree
    /// order: ticked folders they weren't all in, and shared folders
    /// unticked. Both are empty when nothing changed. A folder only some
    /// of the items are in starts unticked, so ticking it adds the items
    /// that lack it.
    pub fn changes(&self, roots: &[Folder]) -> (Vec<String>, Vec<String>) {
        let mut order = Vec::new();
        for r in roots {
            r.walk(&mut |f| order.push(f.id.clone()));
        }
        let add = order.iter().filter(|id| self.checked.contains(*id) && !self.existing.contains(id)).cloned().collect();
        let mut remove: Vec<String> = order.iter().filter(|id| self.existing.contains(id) && !self.checked.contains(*id)).cloned().collect();
        if self.remove_from_current
            && let Some(cur) = &self.current
            && !self.checked.contains(cur)
            && !remove.contains(cur)
        {
            remove.push(cur.clone());
        }
        (add, remove)
    }

    /// Moves the highlight to the next (`down`) or previous pickable row,
    /// staying put at either end. With none highlighted, down picks the
    /// first row and up does nothing.
    pub fn step(&mut self, down: bool) {
        let rows = &self.listed;
        self.highlight = match (self.highlight, down) {
            (None, true) => rows.iter().position(Row::pickable),
            (None, false) => None,
            (Some(i), true) => (i + 1..rows.len()).find(|j| rows[*j].pickable()).or(Some(i)),
            (Some(i), false) => (0..i.min(rows.len())).rev().find(|j| rows[*j].pickable()).or(Some(i)),
        };
    }
}

/// Whether `f` or a folder below it has `needle` (lowercased) in its name.
fn matches(f: &Folder, needle: &str) -> bool {
    f.name.to_lowercase().contains(needle) || f.children.iter().any(|c| matches(c, needle))
}

/// The rows for the picker's state. `recent` is the recently used folder
/// list, newest first; folders that no longer exist are skipped. The
/// children of a locked folder (`shut`) are left out.
pub fn rows(p: &Picker, roots: &[Folder], recent: &[String], shut: impl Fn(&Folder) -> bool) -> Vec<Row> {
    let needle = p.search.trim().to_lowercase();
    let mut out = Vec::new();
    if needle.is_empty() {
        let exists = |id: &str| crate::logic::find_folder(roots, id).is_some();
        out.extend(recent.iter().take(RECENT_SHOWN).filter(|id| exists(id)).map(|id| Row::Recent(id.clone())));
        out.push(Row::Heading);
    }
    fn walk(list: &[Folder], depth: usize, p: &Picker, needle: &str, shut: &dyn Fn(&Folder) -> bool, out: &mut Vec<Row>) {
        for f in list {
            if !needle.is_empty() && !matches(f, needle) {
                continue;
            }
            let kids = !f.children.is_empty() && !shut(f);
            out.push(Row::Folder { id: f.id.clone(), depth, kids: kids && needle.is_empty() });
            if kids && (!needle.is_empty() || p.expanded.contains(&f.id)) {
                walk(&f.children, depth + 1, p, needle, shut, out);
            }
        }
    }
    walk(roots, 0, p, &needle, &shut, &mut out);
    let typed = p.search.trim();
    if !typed.is_empty() {
        let taken = out.iter().any(|r| match r {
            Row::Folder { id, .. } => crate::logic::find_folder(roots, id).is_some_and(|f| f.name == typed),
            _ => false,
        });
        if !taken {
            out.push(Row::Create);
        }
    }
    out
}

/// The name of the folder that holds `id`, if it is not at the top level.
pub fn parent_name<'a>(roots: &'a [Folder], id: &str) -> Option<&'a str> {
    fn find<'a>(list: &'a [Folder], id: &str, parent: Option<&'a Folder>) -> Option<Option<&'a Folder>> {
        for f in list {
            if f.id == id {
                return Some(parent);
            }
            if let Some(found) = find(&f.children, id, Some(f)) {
                return Some(found);
            }
        }
        None
    }
    find(roots, id, None).flatten().map(|f| f.name.as_str())
}

/// One folder of the tree as the tabs see it.
struct Entry<'a> {
    folder: &'a Folder,
    depth: usize,
    /// All its ancestors are open, so the tree shows it.
    shown: bool,
}

/// The folders in tree order. Folders inside a locked folder (`shut`) are
/// left out.
fn entries<'a>(roots: &'a [Folder], expanded: &HashSet<String>, shut: &dyn Fn(&Folder) -> bool) -> Vec<Entry<'a>> {
    fn walk<'a>(list: &'a [Folder], depth: usize, shown: bool, expanded: &HashSet<String>, shut: &dyn Fn(&Folder) -> bool, out: &mut Vec<Entry<'a>>) {
        for f in list {
            out.push(Entry { folder: f, depth, shown });
            if !shut(f) {
                walk(&f.children, depth + 1, shown && expanded.contains(&f.id), expanded, shut, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(roots, 0, true, expanded, shut, &mut out);
    out
}

/// The rows of the picker's current tab. `recent` is the recently used
/// folder list, newest first.
///
/// All: the recent and ticked folders (ticked first, then those the items
/// were already in, then newest first; five plus one per ticked folder),
/// a line, then the tree; with a search, the recent ones that match, a
/// line and the other matching folders, flat. Recent: the same recent and
/// ticked folders, up to twenty, that match the search. Selected: the
/// ticked folders in tree order, flat, which a search does not narrow
/// (only a folder inside a closed one is hidden while there is no search).
/// A search with no folder of exactly that name ends any tab with Create.
pub fn tab_rows(p: &Picker, roots: &[Folder], recent: &[String], shut: impl Fn(&Folder) -> bool) -> Vec<Row> {
    let all = entries(roots, &p.expanded, &shut);
    let typed = p.search.trim();
    let needle = typed.to_lowercase();
    let named = |e: &Entry| needle.is_empty() || e.folder.name.to_lowercase().contains(&needle);
    let kids = |e: &Entry| !e.folder.children.is_empty() && !shut(e.folder);

    // Recent and ticked folders, in the order they are listed.
    let mut keyed: Vec<(usize, &Entry)> = all.iter().enumerate().filter(|(_, e)| p.checked.contains(&e.folder.id) || recent.contains(&e.folder.id)).collect();
    keyed.sort_by_key(|(at, e)| {
        let id = &e.folder.id;
        (!p.checked.contains(id), !p.existing.contains(id), recent.iter().position(|r| r == id).unwrap_or(usize::MAX), *at)
    });
    let recents: Vec<&Entry> = keyed.into_iter().map(|(_, e)| e).filter(|e| named(e)).collect();
    let recent_row = |e: &&Entry| Row::Recent(e.folder.id.clone());

    let mut out: Vec<Row> = Vec::new();
    match p.tab {
        Tab::All => {
            let limit = RECENT_ABOVE_TREE + all.iter().filter(|e| p.checked.contains(&e.folder.id)).count();
            let top: Vec<&Entry> = recents.iter().copied().take(limit).collect();
            out.extend(top.iter().map(recent_row));
            let rest: Vec<Row> = if needle.is_empty() {
                all.iter().filter(|e| e.shown).map(|e| Row::Folder { id: e.folder.id.clone(), depth: e.depth, kids: kids(e) }).collect()
            } else {
                all.iter()
                    .filter(|e| named(e) && !top.iter().any(|t| t.folder.id == e.folder.id))
                    .map(|e| Row::Folder { id: e.folder.id.clone(), depth: 0, kids: false })
                    .collect()
            };
            if !out.is_empty() && !rest.is_empty() {
                out.push(Row::Separator);
            }
            out.extend(rest);
        }
        Tab::Recent => out.extend(recents.iter().take(RECENT_TAB_SHOWN).map(recent_row)),
        Tab::Selected => {
            out.extend(
                all.iter()
                    .filter(|e| p.checked.contains(&e.folder.id) && (e.shown || !needle.is_empty()))
                    .map(|e| Row::Folder { id: e.folder.id.clone(), depth: 0, kids: needle.is_empty() && kids(e) }),
            );
        }
    }
    if !typed.is_empty() && !all.iter().any(|e| e.folder.name == typed) {
        if !out.is_empty() {
            out.push(Row::Separator);
        }
        out.push(Row::Create);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, name: &str, children: Vec<Folder>) -> Folder {
        let mut f = Folder::new(id, name, 0);
        f.children = children;
        f
    }

    fn tree() -> Vec<Folder> {
        vec![
            folder("A", "Animals", vec![folder("A1", "Cats", vec![folder("A11", "Kittens", vec![])]), folder("A2", "Dogs", vec![])]),
            folder("B", "Buildings", vec![]),
        ]
    }

    fn ids(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Recent(id) => format!("r:{id}"),
                Row::Heading => "head".into(),
                Row::Separator => "-".into(),
                Row::Folder { id, depth, .. } => format!("{id}@{depth}"),
                Row::Create => "create".into(),
            })
            .collect()
    }

    fn picker(existing: &[&str], roots: &[Folder]) -> Picker {
        Picker::new(vec!["i".into()], None, existing.iter().map(|s| s.to_string()).collect(), roots)
    }

    fn list(p: &Picker, roots: &[Folder], recent: &[&str]) -> Vec<String> {
        let recent: Vec<String> = recent.iter().map(|s| s.to_string()).collect();
        ids(&tab_rows(p, roots, &recent, |_| false))
    }

    #[test]
    fn the_tabs_go_all_recent_selected_and_wrap() {
        assert_eq!(Tab::default(), Tab::All);
        assert_eq!(Tab::All.next(), Tab::Recent);
        assert_eq!(Tab::Recent.next(), Tab::Selected);
        assert_eq!(Tab::Selected.next(), Tab::All);
    }

    #[test]
    fn all_lists_recent_then_the_tree_with_the_top_level_open() {
        let roots = tree();
        let p = picker(&[], &roots);
        // Folders that no longer exist are skipped.
        assert_eq!(list(&p, &roots, &["B", "gone", "A2"]), ["r:B", "r:A2", "-", "A@0", "A1@1", "A2@1", "B@0"]);
        // No recent folders: just the tree.
        assert_eq!(list(&p, &roots, &[]), ["A@0", "A1@1", "A2@1", "B@0"]);
    }

    #[test]
    fn all_puts_ticked_folders_first_and_allows_one_more_row_for_each() {
        let roots = tree();
        let mut p = picker(&["A2"], &roots);
        // A2 is ticked and not recent, so it leads the recent block.
        assert_eq!(list(&p, &roots, &["B"])[..3], ["r:A2", "r:B", "-"]);
        p.toggle("A2");
        p.toggle("B");
        assert_eq!(list(&p, &roots, &["A1", "B"])[..3], ["r:B", "r:A1", "-"]);
        // Seven recent folders and one ticked: the block holds six.
        let many = Folder::new("X", "X", 0);
        let roots2: Vec<Folder> = (0..7).map(|n| Folder::new(format!("f{n}"), format!("f{n}"), 0)).chain([many]).collect();
        let mut p2 = picker(&[], &roots2);
        let recent: Vec<&str> = vec!["f0", "f1", "f2", "f3", "f4", "f5", "f6"];
        let shown = list(&p2, &roots2, &recent);
        assert_eq!(shown.iter().filter(|r| r.starts_with("r:")).count(), 5);
        p2.toggle("f6");
        let shown = list(&p2, &roots2, &recent);
        assert_eq!(shown.iter().filter(|r| r.starts_with("r:")).count(), 6);
    }

    #[test]
    fn recent_lists_recent_and_ticked_folders_only_up_to_twenty() {
        let roots = tree();
        let mut p = picker(&["A11"], &roots);
        p.tab = Tab::Recent;
        assert_eq!(list(&p, &roots, &["B", "gone", "A2"]), ["r:A11", "r:B", "r:A2"]);
        assert_eq!(list(&p, &roots, &[]), ["r:A11"]);
        let flat: Vec<Folder> = (0..30).map(|n| Folder::new(format!("f{n}"), format!("f{n}"), 0)).collect();
        let ids30: Vec<String> = flat.iter().map(|f| f.id.clone()).collect();
        let p = Picker { tab: Tab::Recent, ..Picker::new(vec![], None, vec![], &flat) };
        assert_eq!(tab_rows(&p, &flat, &ids30, |_| false).len(), RECENT_TAB_SHOWN);
    }

    #[test]
    fn selected_lists_ticked_folders_flat_and_ignores_the_search() {
        let roots = tree();
        let mut p = picker(&["A2", "B"], &roots);
        p.tab = Tab::Selected;
        assert_eq!(list(&p, &roots, &["A1"]), ["A2@0", "B@0"]);
        p.toggle("A11");
        // A11 sits inside Cats, which is closed, so it shows only once Cats opens or a search is typed.
        assert_eq!(list(&p, &roots, &[]), ["A2@0", "B@0"]);
        p.set_expanded("A1", true);
        assert_eq!(list(&p, &roots, &[]), ["A11@0", "A2@0", "B@0"]);
        // The search does not narrow it; no folder is called "dog", so Create ends the list.
        p.search = "dog".into();
        assert_eq!(list(&p, &roots, &[]), ["A11@0", "A2@0", "B@0", "-", "create"]);
        // No folder ticked: nothing but Create.
        p.checked.clear();
        assert_eq!(list(&p, &roots, &[]), ["create"]);
        p.search.clear();
        assert!(list(&p, &roots, &[]).is_empty());
    }

    #[test]
    fn a_search_lists_matching_folders_flat_below_matching_recent_ones() {
        let roots = tree();
        let mut p = picker(&[], &roots);
        p.search = "ts".into(); // Cats only; Kittens has no "ts"
        assert_eq!(list(&p, &roots, &["B"]), ["A1@0", "-", "create"]);
        p.search = "kit".into();
        assert_eq!(list(&p, &roots, &[]), ["A11@0", "-", "create"]);
        // A recent match leads and is not listed twice.
        p.search = "s".into();
        assert_eq!(list(&p, &roots, &["B", "A2"]), ["r:B", "r:A2", "-", "A@0", "A1@0", "A11@0", "-", "create"]);
        // An exact name hides Create.
        p.search = "Dogs".into();
        assert_eq!(list(&p, &roots, &[]), ["A2@0"]);
        p.search = "zebra".into();
        assert_eq!(list(&p, &roots, &[]), ["create"]);
        // The Recent tab filters its own list.
        p.tab = Tab::Recent;
        p.search = "dog".into();
        assert_eq!(list(&p, &roots, &["B", "A2"]), ["r:A2", "-", "create"]);
    }

    #[test]
    fn the_parent_name_is_the_folder_one_level_up() {
        let roots = tree();
        assert_eq!(parent_name(&roots, "A11"), Some("Cats"));
        assert_eq!(parent_name(&roots, "A"), None);
        assert_eq!(parent_name(&roots, "gone"), None);
    }

    #[test]
    fn locked_folders_hide_their_children() {
        let roots = tree();
        let p = picker(&[], &roots);
        let rows = tab_rows(&p, &roots, &[], |f| f.id == "A");
        assert_eq!(ids(&rows), ["A@0", "B@0"]);
        assert!(matches!(rows[0], Row::Folder { kids: false, .. }));
    }

    #[test]
    fn the_list_is_rebuilt_on_tab_search_and_opening_not_on_ticks() {
        let roots = tree();
        let mut p = picker(&[], &roots);
        p.refresh(&roots, &[], |_| false);
        assert_eq!(ids(&p.listed), ["A@0", "A1@1", "A2@1", "B@0"]);
        assert_eq!(p.highlight, None);
        // Ticking leaves the rows alone until something else rebuilds them.
        p.pick("B", false);
        p.refresh(&roots, &[], |_| false);
        assert_eq!(ids(&p.listed), ["A@0", "A1@1", "A2@1", "B@0"]);
        p.set_tab(Tab::Selected);
        p.refresh(&roots, &[], |_| false);
        assert_eq!(ids(&p.listed), ["B@0"]);
        // Unticking in the Selected tab keeps the row until the tab changes.
        p.pick("B", false);
        p.refresh(&roots, &[], |_| false);
        assert_eq!(ids(&p.listed), ["B@0"]);
        p.set_tab(Tab::All);
        p.set_tab(Tab::Selected);
        p.refresh(&roots, &[], |_| false);
        assert!(p.listed.is_empty());
    }

    #[test]
    fn choosing_a_folder_clears_the_search_unless_command_is_held() {
        let roots = tree();
        let mut p = picker(&[], &roots);
        p.search = "dog".into();
        p.refresh(&roots, &[], |_| false);
        assert_eq!(p.highlight, Some(0));
        p.pick("A2", true);
        assert_eq!(p.search, "dog");
        assert!(p.checked.contains("A2"));
        p.pick("A1", false);
        assert_eq!(p.search, "");
        assert!(p.dirty);
        p.refresh(&roots, &[], |_| false);
        assert_eq!(p.highlight, None);
    }

    #[test]
    fn closing_adds_ticked_and_removes_unticked_shared_folders() {
        let roots = tree();
        let mut p = picker(&["A1", "B"], &roots);
        assert!(p.checked.contains("A1") && p.checked.contains("B"));
        // Nothing changed: nothing to apply.
        assert_eq!(p.changes(&roots), (vec![], vec![]));
        p.toggle("A2");
        p.toggle("B");
        assert_eq!(p.changes(&roots), (vec!["A2".to_string()], vec!["B".to_string()]));
        // Ticking and unticking the same folder again is no change.
        p.toggle("A2");
        p.toggle("B");
        assert_eq!(p.changes(&roots), (vec![], vec![]));
        p.checked.clear();
        assert_eq!(p.changes(&roots), (vec![], vec!["A1".to_string(), "B".to_string()]));
    }

    #[test]
    fn several_items_start_with_only_the_shared_folders_ticked() {
        let roots = tree();
        // One item is in A1 and B, the other in A1 only (`shared_folders`,
        // tested in `logic::scope`, gives A1): B starts unticked, and ticking
        // it adds the item that lacks it.
        let mut p = Picker::new(vec!["i1".into(), "i2".into()], None, vec!["A1".into()], &roots);
        assert_eq!(p.changes(&roots), (vec![], vec![]));
        p.toggle("B");
        assert_eq!(p.changes(&roots), (vec!["B".to_string()], vec![]));
        // Unticking the shared folder takes it from both.
        p.toggle("A1");
        assert_eq!(p.changes(&roots), (vec!["B".to_string()], vec!["A1".to_string()]));
    }

    #[test]
    fn the_highlight_skips_separators() {
        let mut p = Picker { listed: vec![Row::Recent("B".into()), Row::Separator, Row::Folder { id: "A".into(), depth: 0, kids: false }], ..Default::default() };
        // Up with nothing highlighted does nothing; down picks the first row.
        p.step(false);
        assert_eq!(p.highlight, None);
        p.step(true);
        assert_eq!(p.highlight, Some(0));
        p.step(true);
        assert_eq!(p.highlight, Some(2));
        p.step(true);
        assert_eq!(p.highlight, Some(2));
        p.step(false);
        assert_eq!(p.highlight, Some(0));
    }

    #[test]
    fn rows_for_the_action_editor_list_are_unchanged() {
        let roots = tree();
        let p = picker(&[], &roots);
        let recent = vec!["B".to_string(), "gone".into(), "A2".into()];
        let rows = rows(&p, &roots, &recent, |_| false);
        assert_eq!(ids(&rows), ["r:B", "r:A2", "head", "A@0", "A1@1", "A2@1", "B@0"]);
    }

    #[test]
    fn closing_can_take_the_items_out_of_the_open_folder() {
        let roots = vec![folder("A", "A", vec![]), folder("B", "B", vec![])];
        let mut p = Picker::new(vec!["i".into()], Some("A".into()), vec![], &roots);
        p.toggle("B");
        assert_eq!(p.changes(&roots), (vec!["B".to_string()], vec![]));
        p.remove_from_current = true;
        assert_eq!(p.changes(&roots), (vec!["B".to_string()], vec!["A".to_string()]));
        // Ticking the open folder keeps them in it.
        p.toggle("A");
        assert_eq!(p.changes(&roots).1, Vec::<String>::new());
    }
}
