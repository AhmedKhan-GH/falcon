//! The rules of the Add to Folders picker (Find ▸ Add to Folders…, ⇧⌘J):
//! which rows it lists for the search typed, which row the keyboard
//! highlights, and what saving changes. The dialog is drawn by
//! `ui/dialogs/add_to_folders.rs`.
//!
//! Like Eagle's, the picker starts with every folder the items all share
//! ticked; ticking adds the items to a folder, unticking a shared folder
//! takes them out of it, and an optional box takes them out of the open
//! folder. With no search it lists the recently used folders above the
//! tree (top level expanded); with a search it lists the folders whose
//! name, or a descendant's, contains it, and offers to create a folder
//! with that name when none is called exactly that.

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
    /// The "Folders" heading above the tree.
    Heading,
    /// A folder of the tree, at a depth; `kids` when it has visible
    /// children to expand.
    Folder { id: String, depth: usize, kids: bool },
    /// Create a folder named as the search.
    Create,
}

impl Row {
    /// Whether the keyboard highlight can stop on the row.
    pub fn pickable(&self) -> bool {
        !matches!(self, Row::Heading)
    }
}

/// The picker as filled in so far.
#[derive(Clone, Debug, Default)]
pub struct Picker {
    /// The items being filed.
    pub items: Vec<String>,
    /// The folder open when the picker opened.
    pub current: Option<String>,
    /// Folders every item was already in.
    pub existing: Vec<String>,
    /// Folders ticked.
    pub checked: HashSet<String>,
    /// Folders whose children show (without a search).
    pub expanded: HashSet<String>,
    pub search: String,
    /// The highlighted row's index in `rows`.
    pub highlight: usize,
    /// Take the items out of `current` on saving.
    pub remove_from_current: bool,
    /// A new folder being named: where it goes, and the name so far.
    pub naming: Option<(NewPlace, String)>,
    /// The keyboard moved the highlight: scroll it into view.
    pub scroll: bool,
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
            ..Default::default()
        }
    }

    /// Ticks or unticks a folder.
    pub fn toggle(&mut self, id: &str) {
        if !self.checked.remove(id) {
            self.checked.insert(id.to_string());
        }
    }

    /// Whether Save can be pressed: something is ticked (as Eagle).
    pub fn can_save(&self) -> bool {
        !self.checked.is_empty()
    }

    /// The folders to add the items to and to take them out of, in tree
    /// order: ticked folders they weren't all in; shared folders unticked;
    /// and the open folder when the box is ticked and it isn't ticked.
    pub fn changes(&self, roots: &[Folder]) -> (Vec<String>, Vec<String>) {
        let mut order = Vec::new();
        for r in roots {
            r.walk(&mut |f| order.push(f.id.clone()));
        }
        let add = order.iter().filter(|id| self.checked.contains(*id) && !self.existing.contains(id)).cloned().collect();
        let mut remove: Vec<String> = self.existing.iter().filter(|id| !self.checked.contains(*id)).cloned().collect();
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
    /// staying put at either end.
    pub fn step(&mut self, rows: &[Row], down: bool) {
        let n = rows.len();
        if n == 0 {
            return;
        }
        let mut i = self.highlight.min(n - 1);
        loop {
            let next = if down { i + 1 } else { i.wrapping_sub(1) };
            if next >= n {
                break;
            }
            i = next;
            if rows[i].pickable() {
                self.highlight = i;
                return;
            }
        }
        if !rows[self.highlight.min(n - 1)].pickable() {
            self.highlight = first_pickable(rows);
        }
    }
}

/// The first row the highlight can stop on.
pub fn first_pickable(rows: &[Row]) -> usize {
    rows.iter().position(Row::pickable).unwrap_or(0)
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
                Row::Folder { id, depth, .. } => format!("{id}@{depth}"),
                Row::Create => "create".into(),
            })
            .collect()
    }

    #[test]
    fn without_a_search_recent_folders_then_the_tree_with_the_top_level_open() {
        let roots = tree();
        let p = Picker::new(vec!["i".into()], None, vec![], &roots);
        let recent = vec!["B".to_string(), "gone".into(), "A2".into()];
        let rows = rows(&p, &roots, &recent, |_| false);
        assert_eq!(ids(&rows), ["r:B", "r:A2", "head", "A@0", "A1@1", "A2@1", "B@0"]);
        assert!(matches!(rows[3], Row::Folder { kids: true, .. }));
    }

    #[test]
    fn a_search_keeps_matches_and_their_ancestors_and_offers_create() {
        let roots = tree();
        let mut p = Picker::new(vec![], None, vec![], &roots);
        p.search = "kit".into();
        assert_eq!(ids(&rows(&p, &roots, &["B".into()], |_| false)), ["A@0", "A1@1", "A11@2", "create"]);
        // An exact name match hides Create.
        p.search = "Dogs".into();
        assert_eq!(ids(&rows(&p, &roots, &[], |_| false)), ["A@0", "A2@1"]);
        p.search = "zebra".into();
        assert_eq!(ids(&rows(&p, &roots, &[], |_| false)), ["create"]);
    }

    #[test]
    fn locked_folders_hide_their_children() {
        let roots = tree();
        let p = Picker::new(vec![], None, vec![], &roots);
        let rows = rows(&p, &roots, &[], |f| f.id == "A");
        assert_eq!(ids(&rows), ["head", "A@0", "B@0"]);
    }

    #[test]
    fn saving_adds_ticked_and_removes_unticked_shared_and_the_open_folder() {
        let roots = tree();
        let mut p = Picker::new(vec!["i".into()], Some("A1".into()), vec!["A1".into(), "B".into()], &roots);
        assert!(p.checked.contains("A1") && p.checked.contains("B"));
        p.toggle("A2");
        p.toggle("B");
        assert_eq!(p.changes(&roots), (vec!["A2".to_string()], vec!["B".to_string()]));
        p.remove_from_current = true;
        // The open folder stays while it is ticked.
        assert_eq!(p.changes(&roots).1, ["B"]);
        p.toggle("A1");
        assert_eq!(p.changes(&roots).1, ["A1", "B"]);
        p.checked.clear();
        assert!(!p.can_save());
    }

    #[test]
    fn the_highlight_skips_the_heading() {
        let rows = vec![Row::Recent("B".into()), Row::Heading, Row::Folder { id: "A".into(), depth: 0, kids: false }];
        let mut p = Picker::default();
        p.step(&rows, true);
        assert_eq!(p.highlight, 2);
        p.step(&rows, true);
        assert_eq!(p.highlight, 2);
        p.step(&rows, false);
        assert_eq!(p.highlight, 0);
        assert_eq!(first_pickable(&[Row::Heading, Row::Create]), 1);
    }
}
