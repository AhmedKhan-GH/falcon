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
//! the folders whose name has its letters in order ([`fuzzy_rank`]), flat
//! and best match first, and offers to create a folder with that name when
//! none is called exactly that. Pinyin is not matched.
//!
//! The list is built when the picker opens and again when the tab, the
//! search or the expanded folders change, not when a tick changes, so
//! rows do not jump while the user ticks them (as Eagle's).

use std::collections::HashSet;

use eframe::egui::{Pos2, Rect, Vec2};
use falcon_library::model::Folder;
use falcon_library::search;

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

/// A key the panel's footer names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyHint {
    /// ↑↓ move the highlight.
    Move,
    /// ←→ fold and unfold the highlighted folder.
    Fold,
    /// ↩ ticks or unticks the highlighted folder.
    Tick,
    /// ⌘↩ does that and leaves the search typed.
    TickKeepSearch,
    /// ↩ on the Create row makes the folder.
    Create,
    /// Tab shows the next tab.
    ChangeTab,
    /// Esc closes the panel.
    Close,
}

impl Picker {
    /// The keys the panel's footer names just now, only those that do
    /// something: the tree folds without a search and not on the Recent
    /// tab, whose rows have no children, and ⌘ only matters with a search
    /// to keep. On the Create row ↩ creates.
    pub fn key_hints(&self) -> Vec<KeyHint> {
        let on_create = matches!(self.highlight.and_then(|h| self.listed.get(h)), Some(Row::Create));
        let searching = !self.search.is_empty();
        let mut out = vec![KeyHint::Move];
        if !searching && self.tab != Tab::Recent {
            out.push(KeyHint::Fold);
        }
        out.push(if on_create { KeyHint::Create } else { KeyHint::Tick });
        if searching && !on_create {
            out.push(KeyHint::TickKeepSearch);
        }
        out.extend([KeyHint::ChangeTab, KeyHint::Close]);
        out
    }

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

/// Where the panel's top left goes (window points): beside the pointer,
/// 10 points right and 10 up as Eagle's, inside the window and clear of
/// the inspector. Too far right it flips to the pointer's left and then
/// slides until it clears the inspector (the pointer may be on it); too
/// low it lifts. In a window too small for it, the top left stays in view.
pub fn place_panel(window: Rect, inspector_left: Option<f32>, pointer: Pos2, size: Vec2) -> Pos2 {
    /// The gap kept to the window's edges and the inspector.
    const MARGIN: f32 = 8.0;
    let right = inspector_left.unwrap_or(window.right()).min(window.right()) - MARGIN;
    let bottom = window.bottom() - MARGIN;
    let mut x = pointer.x + 10.0;
    if x + size.x > right {
        x = (pointer.x - size.x - 20.0).min(right - size.x);
    }
    let y = (pointer.y - 10.0).min(bottom - size.y);
    Pos2::new(x.max(window.left() + MARGIN), y.max(window.top() + MARGIN))
}

/// How a search's letters sit in a name, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchKind {
    /// The name starts with the search.
    Prefix,
    /// The search runs unbroken from the start of a word, or its letters
    /// are all first letters of words ("ab" in "Art Books").
    WordStart,
    /// The search runs unbroken inside a word.
    Contiguous,
    /// The letters are there in order, with others between them.
    Scattered,
}

/// How well a name matches a search; a smaller value is a better match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FuzzyRank {
    pub kind: MatchKind,
    /// Other letters between the first and the last one matched.
    gaps: usize,
    /// Letters before the first one matched.
    start: usize,
    /// The name's length, so an exact name leads the names it begins.
    len: usize,
}

/// A text as [`fuzzy_rank`] compares it: each letter folded (by
/// `search::rank_key`: decomposed, no accents, lower case), without
/// spaces, and whether it begins a word. A word begins after a space or
/// punctuation, at a capital after a small letter, and where letters and
/// digits meet.
fn fuzzy_key(text: &str) -> Vec<(char, bool)> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if c.is_whitespace() {
            prev = Some(' ');
            continue;
        }
        let folded = search::rank_key(c.encode_utf8(&mut [0; 4]));
        if folded.is_empty() {
            continue; // a combining accent belongs to the letter before it
        }
        let begins = c.is_alphanumeric() && prev.is_none_or(|p| !p.is_alphanumeric() || (p.is_lowercase() && c.is_uppercase()) || p.is_alphabetic() != c.is_alphabetic());
        out.extend(folded.chars().enumerate().map(|(n, l)| (l, begins && n == 0)));
        prev = Some(c);
    }
    out
}

/// Where the letters of `word` first follow each other in order in `text`
/// from `from` on: the index of the last one. `from` holds the first.
fn subsequence_end(text: &[char], word: &[char], from: usize) -> Option<usize> {
    let mut at = from;
    for c in &word[1..] {
        at += 1 + text[at + 1..].iter().position(|t| t == c)?;
    }
    Some(at)
}

/// How `name` matches the search `query`, or `None` when its letters are
/// not all in the name in order. Comparison ignores case, accents and
/// spaces and goes by whole characters, so any script works. Pinyin is not
/// matched, on purpose.
pub fn fuzzy_rank(name: &str, query: &str) -> Option<FuzzyRank> {
    let key = fuzzy_key(name);
    let word: Vec<char> = fuzzy_key(query).into_iter().map(|(c, _)| c).collect();
    let letters: Vec<char> = key.iter().map(|(c, _)| *c).collect();
    let len = key.len();
    let rank = |kind, gaps, start| Some(FuzzyRank { kind, gaps, start, len });
    if word.is_empty() {
        return rank(MatchKind::Prefix, 0, 0);
    }
    // Unbroken: the start of a word beats the middle of one.
    let runs: Vec<usize> = letters.windows(word.len()).enumerate().filter(|(_, w)| *w == word.as_slice()).map(|(at, _)| at).collect();
    if runs.first() == Some(&0) {
        return rank(MatchKind::Prefix, 0, 0);
    }
    if let Some(at) = runs.iter().find(|at| key[**at].1) {
        return rank(MatchKind::WordStart, 0, *at);
    }
    // Every letter the first of a word.
    let firsts: Vec<(usize, char)> = key.iter().enumerate().filter(|(_, (_, begins))| *begins).map(|(at, (c, _))| (at, *c)).collect();
    let initials: Vec<char> = firsts.iter().map(|(_, c)| *c).collect();
    if let Some(from) = initials.iter().position(|c| *c == word[0])
        && let Some(end) = subsequence_end(&initials, &word, from)
    {
        let (start, last) = (firsts[from].0, firsts[end].0);
        return rank(MatchKind::WordStart, last + 1 - start - word.len(), start);
    }
    if let Some(at) = runs.first() {
        return rank(MatchKind::Contiguous, 0, *at);
    }
    // Scattered: the tightest run of the letters, the earliest if tied.
    let (gaps, start) = (0..len)
        .filter(|from| letters[*from] == word[0])
        .filter_map(|from| subsequence_end(&letters, &word, from).map(|end| (end + 1 - from - word.len(), from)))
        .min()?;
    rank(MatchKind::Scattered, gaps, start)
}

/// Whether `f` or a folder below it matches the search.
fn matches(f: &Folder, needle: &str) -> bool {
    fuzzy_rank(&f.name, needle).is_some() || f.children.iter().any(|c| matches(c, needle))
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

/// Takes a folder off the recently used list (Remove on a Recent row's
/// right-click menu). The folder itself stays.
pub fn forget_recent(recent: &mut Vec<String>, id: &str) {
    recent.retain(|r| r != id);
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

/// The entries that match the search, best first; ties keep their order
/// and no search keeps every entry where it is.
fn ranked<'a, 'b>(list: impl Iterator<Item = &'a Entry<'b>>, typed: &str) -> Vec<(FuzzyRank, &'a Entry<'b>)> {
    let mut list: Vec<_> = list.filter_map(|e| fuzzy_rank(&e.folder.name, typed).map(|r| (r, e))).collect();
    if !typed.is_empty() {
        list.sort_by_key(|(r, _)| *r);
    }
    list
}

/// The rows of the picker's current tab. `recent` is the recently used
/// folder list, newest first.
///
/// All: the recent and ticked folders (ticked first, then those the items
/// were already in, then newest first; five plus one per ticked folder),
/// a line, then the tree; with a search, the recent ones that match, a
/// line and the other matching folders, flat, each block best match first.
/// A recent folder whose letters are merely scattered waits in the second
/// block, so it doesn't take the highlight from a real match. Recent: the
/// same recent and ticked folders, up to twenty, that match the search,
/// best match first. Selected: the ticked folders in tree order, flat,
/// which a search does not narrow (only a folder inside a closed one is hidden while there is no search).
/// A search with no folder of exactly that name ends any tab with Create.
pub fn tab_rows(p: &Picker, roots: &[Folder], recent: &[String], shut: impl Fn(&Folder) -> bool) -> Vec<Row> {
    let all = entries(roots, &p.expanded, &shut);
    let typed = p.search.trim();
    let searching = !typed.is_empty();
    let kids = |e: &Entry| !e.folder.children.is_empty() && !shut(e.folder);
    // Recent and ticked folders, in the order they are listed.
    let mut keyed: Vec<(usize, &Entry)> = all.iter().enumerate().filter(|(_, e)| p.checked.contains(&e.folder.id) || recent.contains(&e.folder.id)).collect();
    keyed.sort_by_key(|(at, e)| {
        let id = &e.folder.id;
        (!p.checked.contains(id), !p.existing.contains(id), recent.iter().position(|r| r == id).unwrap_or(usize::MAX), *at)
    });
    let recents = ranked(keyed.into_iter().map(|(_, e)| e), typed);
    let recent_row = |e: &&Entry| Row::Recent(e.folder.id.clone());

    let mut out: Vec<Row> = Vec::new();
    match p.tab {
        Tab::All => {
            let limit = RECENT_ABOVE_TREE + all.iter().filter(|e| p.checked.contains(&e.folder.id)).count();
            let top: Vec<&Entry> = recents.iter().filter(|(r, _)| r.kind != MatchKind::Scattered).map(|(_, e)| *e).take(limit).collect();
            out.extend(top.iter().map(recent_row));
            let rest: Vec<Row> = if !searching {
                all.iter().filter(|e| e.shown).map(|e| Row::Folder { id: e.folder.id.clone(), depth: e.depth, kids: kids(e) }).collect()
            } else {
                let others = all.iter().filter(|e| !top.iter().any(|t| t.folder.id == e.folder.id));
                ranked(others, typed).into_iter().map(|(_, e)| Row::Folder { id: e.folder.id.clone(), depth: 0, kids: false }).collect()
            };
            if !out.is_empty() && !rest.is_empty() {
                out.push(Row::Separator);
            }
            out.extend(rest);
        }
        Tab::Recent => out.extend(recents.iter().take(RECENT_TAB_SHOWN).map(|(_, e)| recent_row(e))),
        Tab::Selected => {
            out.extend(
                all.iter()
                    .filter(|e| p.checked.contains(&e.folder.id) && (e.shown || searching))
                    .map(|e| Row::Folder { id: e.folder.id.clone(), depth: 0, kids: !searching && kids(e) }),
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
        p.search = "ts".into(); // Unbroken in Cats, scattered in Kittens.
        assert_eq!(list(&p, &roots, &["B"]), ["A1@0", "A11@0", "-", "create"]);
        p.search = "kit".into();
        assert_eq!(list(&p, &roots, &[]), ["A11@0", "-", "create"]);
        // A recent match leads and is not listed twice.
        p.search = "s".into();
        assert_eq!(list(&p, &roots, &["B", "A2"]), ["r:A2", "r:B", "-", "A1@0", "A@0", "A11@0", "-", "create"]);
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

    fn kind(name: &str, query: &str) -> Option<MatchKind> {
        fuzzy_rank(name, query).map(|r| r.kind)
    }

    #[test]
    fn a_match_is_a_prefix_a_word_start_contiguous_or_scattered() {
        assert_eq!(kind("Animals", "ani"), Some(MatchKind::Prefix));
        assert_eq!(kind("Old Animals", "ani"), Some(MatchKind::WordStart));
        assert_eq!(kind("oldAnimals", "ani"), Some(MatchKind::WordStart), "a capital starts a word");
        assert_eq!(kind("photo2024", "2024"), Some(MatchKind::WordStart), "digits start a word");
        assert_eq!(kind("Art Books", "ab"), Some(MatchKind::WordStart), "first letters of words");
        assert_eq!(kind("Humanities", "ani"), Some(MatchKind::Contiguous));
        assert_eq!(kind("Kittens", "ts"), Some(MatchKind::Scattered));
        assert_eq!(kind("Animals", "lam"), None, "letters out of order");
        assert_eq!(kind("Cat", "cats"), None, "more letters than the name");
    }

    #[test]
    fn matching_ignores_case_accents_and_spaces_in_any_script() {
        assert_eq!(kind("ANIMALS", "ani"), Some(MatchKind::Prefix));
        assert_eq!(kind("animals", "ANI"), Some(MatchKind::Prefix));
        assert_eq!(kind("Café Photos", "cafe"), Some(MatchKind::Prefix));
        assert_eq!(kind("Cafe\u{301} Photos", "CAFÉ"), Some(MatchKind::Prefix), "a decomposed accent");
        assert_eq!(kind("Art Books", "art books"), Some(MatchKind::Prefix));
        assert_eq!(kind("Art Books", "artb"), Some(MatchKind::Prefix));
        assert_eq!(kind("Фотографии", "ФОТО"), Some(MatchKind::Prefix));
        assert_eq!(kind("Моя Фотография", "фот"), Some(MatchKind::WordStart));
        assert_eq!(kind("Größe", "GRÖ"), Some(MatchKind::Prefix));
        assert_eq!(kind("写真フォルダ", "フォルダ"), Some(MatchKind::Contiguous));
        assert_eq!(kind("写真フォルダ", "写フ"), Some(MatchKind::Scattered));
        assert_eq!(kind("사진 폴더", "폴더"), Some(MatchKind::WordStart));
    }

    #[test]
    fn pinyin_is_not_matched() {
        assert_eq!(fuzzy_rank("照片", "zhaopian"), None);
        assert_eq!(fuzzy_rank("照片", "zp"), None);
    }

    #[test]
    fn ranks_sort_prefix_then_word_start_then_contiguous_then_scattered() {
        let mut names = vec!["Coat Hangers", "Tomcat", "Old Cat Pictures", "Cat", "Catalogue", "C a t", "Crates", "Crate Art", "Dog"];
        names.retain(|n| fuzzy_rank(n, "cat").is_some());
        names.sort_by_key(|n| fuzzy_rank(n, "cat"));
        // An exact name leads what it begins, and among scattered matches
        // the same gaps go to the earlier start, then the shorter name.
        assert_eq!(names, ["Cat", "C a t", "Catalogue", "Old Cat Pictures", "Tomcat", "Crates", "Crate Art", "Coat Hangers"]);
    }

    #[test]
    fn a_tighter_scatter_beats_a_looser_one_at_any_start() {
        // The tightest run counts, not the first c.
        let rank = fuzzy_rank("cxxaxxtxxcxat", "cat").unwrap();
        assert_eq!(rank.kind, MatchKind::Scattered);
        assert_eq!((rank.gaps, rank.start), (1, 9));
        assert!(fuzzy_rank("cxat", "cat") < fuzzy_rank("cxxat", "cat"));
    }

    #[test]
    fn results_are_ranked_and_scattered_recents_do_not_lead() {
        let roots: Vec<Folder> = ["Coat", "Tomcat", "Cat", "Old Cat"].iter().enumerate().map(|(n, name)| Folder::new(format!("f{n}"), *name, 0)).collect();
        let mut p = picker(&[], &roots);
        p.search = "Cat".into();
        // Best first, whatever the tree's order.
        assert_eq!(list(&p, &roots, &[]), ["f2@0", "f3@0", "f1@0", "f0@0"]);
        // A recent real match stays above the line; a scattered one doesn't.
        assert_eq!(list(&p, &roots, &["f1", "f0"]), ["r:f1", "-", "f2@0", "f3@0", "f0@0"]);
        // The Recent tab has no second block, so it lists both, ranked.
        p.tab = Tab::Recent;
        assert_eq!(list(&p, &roots, &["f0", "f1"]), ["r:f1", "r:f0"]);
        // And the first row is what the keyboard starts on.
        p.tab = Tab::All;
        p.dirty = true;
        p.refresh(&roots, &[], |_| false);
        assert_eq!(p.listed[0], Row::Folder { id: "f2".into(), depth: 0, kids: false });
        assert_eq!(p.highlight, Some(0));
    }

    #[test]
    fn the_action_editors_search_keeps_the_tree_and_matches_fuzzily() {
        let roots = tree();
        let mut p = picker(&[], &roots);
        p.search = "ktn".into();
        assert_eq!(ids(&rows(&p, &roots, &[], |_| false)), ["A@0", "A1@1", "A11@2", "create"]);
    }

    #[test]
    fn removing_a_recent_folder_forgets_it_but_not_a_tick() {
        let roots = tree();
        let mut recent: Vec<String> = vec!["B".into(), "A2".into(), "A1".into()];
        forget_recent(&mut recent, "A2");
        assert_eq!(recent, ["B", "A1"]);
        forget_recent(&mut recent, "gone");
        assert_eq!(recent, ["B", "A1"]);
        // The rebuilt Recent tab no longer lists it; the folder is still in
        // the tree, and a ticked one stays in the block for its tick.
        let mut p = picker(&["A1"], &roots);
        p.tab = Tab::Recent;
        let names: Vec<&str> = recent.iter().map(String::as_str).collect();
        assert_eq!(list(&p, &roots, &names), ["r:A1", "r:B"]);
        forget_recent(&mut recent, "A1");
        let names: Vec<&str> = recent.iter().map(String::as_str).collect();
        assert_eq!(list(&p, &roots, &names), ["r:A1", "r:B"]);
        forget_recent(&mut recent, "B");
        assert_eq!(list(&p, &roots, &[]), ["r:A1"]);
    }

    #[test]
    fn the_footer_names_only_the_keys_that_work() {
        use KeyHint::*;
        let roots = tree();
        let mut p = picker(&[], &roots);
        p.refresh(&roots, &[], |_| false);
        assert_eq!(p.key_hints(), [Move, Fold, Tick, ChangeTab, Close]);
        // The Recent tab's rows have no children to fold.
        p.set_tab(Tab::Recent);
        assert_eq!(p.key_hints(), [Move, Tick, ChangeTab, Close]);
        p.set_tab(Tab::Selected);
        assert_eq!(p.key_hints(), [Move, Fold, Tick, ChangeTab, Close]);
        // With a search ←→ move in the text and ⌘↩ keeps it.
        p.set_tab(Tab::All);
        p.search = "dog".into();
        p.dirty = true;
        p.refresh(&roots, &[], |_| false);
        assert_eq!(p.key_hints(), [Move, Tick, TickKeepSearch, ChangeTab, Close]);
        // On the Create row ↩ creates, with or without ⌘.
        p.search = "zebra".into();
        p.dirty = true;
        p.refresh(&roots, &[], |_| false);
        assert_eq!(p.listed, [Row::Create]);
        assert_eq!(p.key_hints(), [Move, Create, ChangeTab, Close]);
    }

    fn place(inspector: Option<f32>, pointer: (f32, f32)) -> (f32, f32) {
        let window = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
        let at = place_panel(window, inspector, Pos2::new(pointer.0, pointer.1), Vec2::new(320.0, 460.0));
        (at.x, at.y)
    }

    #[test]
    fn the_panel_opens_beside_the_pointer() {
        assert_eq!(place(None, (100.0, 100.0)), (110.0, 90.0));
        assert_eq!(place(Some(700.0), (100.0, 100.0)), (110.0, 90.0));
        // Exactly fitting against the inspector, it stays.
        assert_eq!(place(Some(700.0), (362.0, 100.0)), (372.0, 90.0));
    }

    #[test]
    fn the_panel_flips_left_of_the_pointer_rather_than_cover_the_inspector() {
        assert_eq!(place(Some(700.0), (500.0, 100.0)), (160.0, 90.0));
        // No inspector: it fits, right up to the window's edge.
        assert_eq!(place(None, (500.0, 100.0)), (510.0, 90.0));
        assert_eq!(place(None, (900.0, 100.0)), (560.0, 90.0));
    }

    #[test]
    fn the_panel_slides_clear_when_the_pointer_is_on_the_inspector() {
        // The inspector's + at 720: flipping alone would end at 700, on it.
        assert_eq!(place(Some(700.0), (720.0, 100.0)), (372.0, 90.0));
        assert_eq!(place(Some(700.0), (990.0, 100.0)), (372.0, 90.0));
    }

    #[test]
    fn the_panel_lifts_and_stays_in_the_window() {
        assert_eq!(place(None, (100.0, 700.0)), (110.0, 332.0));
        assert_eq!(place(None, (100.0, 2.0)), (110.0, 8.0));
        // A window too small for it keeps the top left in view.
        let tiny = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 200.0));
        assert_eq!(place_panel(tiny, Some(100.0), Pos2::new(150.0, 150.0), Vec2::new(320.0, 460.0)), Pos2::new(8.0, 8.0));
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
