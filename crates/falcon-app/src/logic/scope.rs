//! Scopes (the sidebar's views), the fixed navigation counts, and filtering,
//! sorting and shuffling items into the visible list.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use falcon_library::model::{Folder, Item};
use falcon_library::search::{self, Filter, OrderBy, Query};
use super::folders::find_folder;

/// Which part of the window the arrow keys act on, after Eagle's focus
/// zones: the grid (or viewer), or the sidebar once one of its rows was
/// clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zone {
    #[default]
    Content,
    Sidebar,
}

/// The sidebar row ↑ (or ↓ with `down`) opens, as an index into `rows`,
/// the views of the visible rows top to bottom. The open row is `cursor`
/// when that row still shows `current` (Quick Access repeats folders),
/// else the first row showing it. No row above the top or below the
/// bottom; from a view with no row, ↓ goes to the first.
pub fn sidebar_step(rows: &[Scope], cursor: Option<usize>, current: &Scope, down: bool) -> Option<usize> {
    let same = |a: &Scope, b: &Scope| matches!((a, b), (Scope::Random(_), Scope::Random(_))) || a == b;
    let at = cursor.filter(|&c| rows.get(c).is_some_and(|r| same(r, current))).or_else(|| rows.iter().position(|r| same(r, current)));
    match (at, down) {
        (None, true) => (!rows.is_empty()).then_some(0),
        (None, false) => None,
        (Some(i), true) => (i + 1 < rows.len()).then_some(i + 1),
        (Some(i), false) => i.checked_sub(1),
    }
}

/// Which sidebar entry is active.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    Uncategorized,
    Untagged,
    /// Most recently changed items first.
    Recent,
    /// A shuffled view; the seed changes each time the entry is opened.
    Random(u64),
    /// The tag overview page.
    AllTags,
    /// The duplicates page.
    Duplicates,
    Trash,
    /// Items placed directly in a folder (Eagle hides subfolder content by
    /// default; with it shown the grid passes the members as `only`).
    Folder(String),
    Tag(String),
    /// A smart folder's matches.
    Smart(String),
}

impl Scope {
    /// Title shown in the toolbar and inspector.
    pub fn title(&self, roots: &[Folder]) -> String {
        match self {
            Scope::All => "All".into(),
            Scope::Uncategorized => "Uncategorized".into(),
            Scope::Untagged => "Untagged".into(),
            Scope::Recent => "Recently Used".into(),
            Scope::Random(_) => "Random".into(),
            Scope::AllTags => "All Tags".into(),
            Scope::Duplicates => "Duplicates".into(),
            Scope::Trash => "Trash".into(),
            Scope::Folder(id) => find_folder(roots, id).map_or_else(|| "Folder".into(), |f| f.name.clone()),
            Scope::Tag(t) => t.clone(),
            Scope::Smart(_) => "Smart Folder".into(),
        }
    }
}

/// Counts for the fixed sidebar entries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NavCounts {
    pub all: usize,
    pub uncategorized: usize,
    pub untagged: usize,
    pub trash: usize,
    pub tags: usize,
}

/// Whether an item belongs in Uncategorized, as Eagle decides it: no
/// folders, or a single folder id that names no folder (left behind when
/// a folder was deleted elsewhere). Two stale ids don't count, as in Eagle.
pub fn uncategorized(item: &Item, known: &impl Fn(&str) -> bool) -> bool {
    match item.folders.as_slice() {
        [] => true,
        [only] => !only.is_empty() && !known(only),
        _ => false,
    }
}

pub fn nav_counts(items: &[Item], roots: &[Folder]) -> NavCounts {
    let index = search::folder_index(roots);
    let known = |id: &str| index.contains_key(id);
    let mut c = NavCounts::default();
    let mut tags = HashSet::new();
    for i in items {
        if i.is_deleted {
            c.trash += 1;
            continue;
        }
        c.all += 1;
        c.uncategorized += usize::from(uncategorized(i, &known));
        c.untagged += usize::from(i.tags.is_empty());
        tags.extend(i.tags.iter());
    }
    c.tags = tags.len();
    c
}

/// The folder to ask about before trashing `items`, as Eagle does: the
/// open folder (one, not several picked), when some item is in more than
/// one folder.
pub fn trash_question(scope: &Scope, combined: bool, items: &[&Item]) -> Option<String> {
    match scope {
        Scope::Folder(id) if !combined && items.iter().any(|i| i.folders.len() > 1) => Some(id.clone()),
        _ => None,
    }
}

/// Takes `item` out of folder `fid` when it is in another folder too
/// (returns true); an item in that folder alone is left for the Trash.
pub fn leave_folder(item: &mut Item, fid: &str) -> bool {
    if item.folders.len() < 2 {
        return false;
    }
    item.folders.retain(|f| f != fid);
    true
}

/// Live tags with their item counts, sorted by name.
/// What a field shows for several items: their common value, or nothing
/// when they differ (Eagle's `calculateUrl` and `calculateAnnotation`).
pub fn shared_text<'a>(mut values: impl Iterator<Item = &'a str>) -> &'a str {
    let Some(first) = values.next() else { return "" };
    if values.all(|v| v == first) { first } else { "" }
}

/// The folders every one of `items` is in, in the first item's order and
/// each once (Eagle's multi-selection inspector shows a chip for these).
pub fn shared_folders(items: &[&Item]) -> Vec<String> {
    let Some(first) = items.first() else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for f in &first.folders {
        if !out.contains(f) && items.iter().all(|i| i.folders.contains(f)) {
            out.push(f.clone());
        }
    }
    out
}

pub fn tag_counts(items: &[Item]) -> Vec<(String, usize)> {
    let mut map: HashMap<&str, usize> = HashMap::new();
    for i in items.iter().filter(|i| !i.is_deleted) {
        for t in &i.tags {
            *map.entry(t).or_default() += 1;
        }
    }
    let mut v: Vec<_> = map.into_iter().map(|(t, n)| (t.to_string(), n)).collect();
    v.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then_with(|| a.0.cmp(&b.0)));
    v
}

fn in_scope(item: &Item, scope: &Scope, index: &search::FolderIndex) -> bool {
    match scope {
        Scope::Trash => item.is_deleted,
        // Recently Used lists what was opened, trashed items too, as in
        // Eagle; the caller's `only` set holds the list.
        Scope::Recent => true,
        _ if item.is_deleted => false,
        Scope::All | Scope::Random(_) | Scope::AllTags | Scope::Duplicates => true,
        Scope::Uncategorized => uncategorized(item, &|id| index.contains_key(id)),
        Scope::Untagged => item.tags.is_empty(),
        Scope::Folder(id) => item.folders.iter().any(|f| f == id),
        Scope::Tag(t) => item.tags.iter().any(|x| x == t),
        // Membership is decided by the caller's `only` set.
        Scope::Smart(_) => true,
    }
}

fn shuffle_key(seed: u64, id: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    id.hash(&mut h);
    h.finish()
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
/// [`visible_in`] with every part of each item searched.
pub fn visible(
    items: &[Item],
    roots: &[Folder],
    scope: &Scope,
    query: &str,
    filter: &Filter,
    order: OrderBy,
    increase: bool,
    only: Option<&HashSet<String>>,
) -> Vec<usize> {
    visible_in(items, roots, scope, query, &search::SearchScope::default(), filter, order, increase, only)
}

#[allow(clippy::too_many_arguments)]
/// Indices into `items` that pass the scope, the search query and the filter
/// panel, in Eagle's order. A colour filter sorts by colour distance. The
/// query is matched with typo tolerance; exact matches come first, then
/// typo matches by number of edits, each group in that order.
/// Only the parts of each item `parts` turns on are searched (the search
/// box's Search Scope menu).
pub fn visible_in(
    items: &[Item],
    roots: &[Folder],
    scope: &Scope,
    query: &str,
    parts: &search::SearchScope,
    filter: &Filter,
    order: OrderBy,
    increase: bool,
    only: Option<&HashSet<String>>,
) -> Vec<usize> {
    let index = search::folder_index(roots);
    let q = Query::parse(query);
    let qlen = query.trim().chars().count();
    let hits: Vec<(usize, f64, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| in_scope(i, scope, &index) && only.is_none_or(|o| o.contains(&i.id)))
        .filter_map(|(n, i)| {
            let cost = if q.is_empty() { 0 } else { q.fuzzy_score(&search::search_text_in(i, &index, qlen, parts))? };
            filter.check(i).map(|d| (n, d, cost))
        })
        .collect();
    let cost: HashMap<usize, usize> = hits.iter().map(|&(n, _, c)| (n, c)).collect();
    let mut hits: Vec<(usize, f64)> = hits.into_iter().map(|(n, d, _)| (n, d)).collect();
    match scope {
        Scope::Random(seed) => hits.sort_by_key(|&(i, _)| shuffle_key(*seed, &items[i].id)),
        Scope::Trash => hits.sort_by_key(|&(i, _)| {
            let it = &items[i];
            std::cmp::Reverse(it.extra.get("deletedTime").and_then(|v| v.as_u64()).unwrap_or(it.modification_time))
        }),
        _ if filter.color.is_some() => hits.sort_by(|a, b| a.1.total_cmp(&b.1)),
        _ => {
            let mut refs: Vec<&Item> = hits.iter().map(|&(i, _)| &items[i]).collect();
            search::sort(&mut refs, order, increase);
            let pos: HashMap<&str, usize> = hits.iter().map(|&(i, _)| (items[i].id.as_str(), i)).collect();
            let mut out: Vec<usize> = refs.iter().map(|it| pos[it.id.as_str()]).collect();
            out.sort_by_key(|i| cost[i]);
            return out;
        }
    }
    // Exact matches keep their order ahead of typo matches, fewest edits first.
    let mut out: Vec<usize> = hits.into_iter().map(|(i, _)| i).collect();
    out.sort_by_key(|i| cost[i]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::test_util::{fixture, item, names};

    #[test]
    fn shared_text_is_common_or_empty() {
        assert_eq!(shared_text(["a", "a"].into_iter()), "a");
        assert_eq!(shared_text(["a", "b"].into_iter()), "");
        assert_eq!(shared_text(std::iter::empty()), "");
    }

    #[test]
    fn shared_folders_follow_the_first_item() {
        let it = |id: &str, f: &[&str]| item(id, id, f, &[], false, 0, 1, 1);
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let a = it("a", &["x", "y", "z", "y"]);
        let b = it("b", &["z", "x"]);
        let c = it("c", &["q"]);
        assert!(shared_folders(&[]).is_empty());
        assert_eq!(shared_folders(&[&a]), s(&["x", "y", "z"]));
        assert_eq!(shared_folders(&[&a, &b]), s(&["x", "z"]));
        assert_eq!(shared_folders(&[&b, &a]), s(&["z", "x"]));
        assert!(shared_folders(&[&a, &b, &c]).is_empty());
    }

    #[test]
    fn nav_counts_cover_fixed_entries() {
        let (roots, items) = fixture();
        assert_eq!(nav_counts(&items, &roots), NavCounts { all: 4, uncategorized: 1, untagged: 1, trash: 1, tags: 3 });
    }

    #[test]
    fn trashing_in_a_folder_asks_about_items_in_other_folders() {
        let (_, items) = fixture();
        let open = Scope::Folder("A".into());
        let (zebra, mango) = (&items[0], &items[2]);
        assert_eq!(trash_question(&open, false, &[zebra]), None);
        assert_eq!(trash_question(&open, false, &[zebra, mango]).as_deref(), Some("A"));
        assert_eq!(trash_question(&open, true, &[mango]), None, "several picked folders don't ask");
        assert_eq!(trash_question(&Scope::All, false, &[mango]), None);
        let mut m = mango.clone();
        assert!(leave_folder(&mut m, "A"));
        assert_eq!(m.folders, ["B"]);
        let mut z = zebra.clone();
        assert!(!leave_folder(&mut z, "A"));
        assert_eq!(z.folders, ["A"]);
    }

    #[test]
    fn uncategorized_counts_one_stale_folder_id() {
        let (roots, mut items) = fixture();
        items.push(item("6", "orphan", &["Gone"], &["red"], false, 60, 3, 3));
        items.push(item("7", "two-stale", &["Gone", "Lost"], &["red"], false, 70, 4, 4));
        items.push(item("8", "stale-and-live", &["Gone", "A"], &["red"], false, 80, 5, 5));
        assert_eq!(nav_counts(&items, &roots).uncategorized, 2);
        let v = visible(&items, &roots, &Scope::Uncategorized, "", &Filter::default(), OrderBy::Name, true, None);
        assert_eq!(names(&items, &v), ["loose", "orphan"]);
    }

    #[test]
    fn scopes() {
        let (roots, items) = fixture();
        let v = |s: Scope| names(&items, &visible(&items, &roots, &s, "", &Filter::default(), OrderBy::Name, true, None));
        assert_eq!(v(Scope::All), ["apple", "loose", "Mango", "Zebra"]);
        assert_eq!(v(Scope::Uncategorized), ["loose"]);
        assert_eq!(v(Scope::Untagged), ["Mango"]);
        assert_eq!(v(Scope::Trash), ["gone"]);
        assert_eq!(v(Scope::Folder("A".into())), ["Mango", "Zebra"]);
        assert_eq!(v(Scope::Tag("red".into())), ["apple", "Zebra"]);
        // Recently Used is whatever `only` lets through, trashed items too.
        let recent: HashSet<String> = ["4", "5"].iter().map(|s| s.to_string()).collect();
        let r = visible(&items, &roots, &Scope::Recent, "", &Filter::default(), OrderBy::Name, true, Some(&recent));
        assert_eq!(names(&items, &r), ["gone", "loose"]);
        assert_eq!(v(Scope::Random(7)).len(), 4);
    }

    #[test]
    fn search_terms_and_negation() {
        let (roots, items) = fixture();
        let q = |s: &str| names(&items, &visible(&items, &roots, &Scope::All, s, &Filter::default(), OrderBy::Name, true, None));
        assert_eq!(q("ZEB"), ["Zebra"]);
        assert_eq!(q("red big"), ["Zebra"]);
        assert_eq!(q("red -big"), ["apple"]);
        assert_eq!(q(".png").len(), 4);
        assert_eq!(q("B-name"), ["apple", "Mango"]); // folder name
        assert_eq!(q("zebra OR mango"), ["Mango", "Zebra"]);
        assert_eq!(q("  ").len(), 4);
    }

    #[test]
    fn search_tolerates_typos_after_exact_hits() {
        let (roots, items) = fixture();
        let q = |s: &str| names(&items, &visible(&items, &roots, &Scope::All, s, &Filter::default(), OrderBy::Name, true, None));
        assert_eq!(q("zebar"), ["Zebra"]);
        assert_eq!(q("mangp"), ["Mango"]);
        assert!(q("qqqqq").is_empty());
    }

    #[test]
    fn sorting_follows_eagle_directions() {
        let (roots, items) = fixture();
        let s = |k, inc| names(&items, &visible(&items, &roots, &Scope::All, "", &Filter::default(), k, inc, None));
        // Time keys are newest first when "increase" is on.
        assert_eq!(s(OrderBy::Import, true), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(OrderBy::Import, false), ["apple", "Mango", "Zebra", "loose"]);
        assert_eq!(s(OrderBy::FileSize, true), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(OrderBy::Resolution, false), ["apple", "Mango", "Zebra", "loose"]);
    }
}

#[cfg(test)]
mod sidebar_tests {
    use super::*;

    #[test]
    fn sidebar_step_walks_the_visible_rows() {
        let f = |id: &str| Scope::Folder(id.to_string());
        // All, Random, Trash, Quick Access "b", then folders a and b.
        let rows = [Scope::All, Scope::Random(7), Scope::Trash, f("b"), f("a"), f("b")];
        assert_eq!(sidebar_step(&rows, None, &Scope::All, true), Some(1));
        assert_eq!(sidebar_step(&rows, None, &Scope::All, false), None);
        // Any shuffle seed is the Random row.
        assert_eq!(sidebar_step(&rows, None, &Scope::Random(99), true), Some(2));
        // A folder shown twice: the row the keys last opened counts.
        assert_eq!(sidebar_step(&rows, Some(5), &f("b"), false), Some(4));
        assert_eq!(sidebar_step(&rows, None, &f("b"), false), Some(2));
        // A stale cursor falls back to the first matching row.
        assert_eq!(sidebar_step(&rows, Some(0), &f("a"), true), Some(5));
        assert_eq!(sidebar_step(&rows, Some(5), &f("b"), true), None);
        // A view with no row: ↓ starts at the top.
        assert_eq!(sidebar_step(&rows, None, &Scope::Duplicates, true), Some(0));
        assert_eq!(sidebar_step(&rows, None, &Scope::Duplicates, false), None);
        assert_eq!(sidebar_step(&[], None, &Scope::All, true), None);
    }
}
