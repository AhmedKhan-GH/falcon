//! UI-independent logic: filtering, sorting and counting.

use std::collections::{HashMap, HashSet};

use falcon_library::model::{Folder, Item};

/// Which sidebar entry is active.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    Uncategorized,
    Trash,
    /// A folder and everything below it.
    Folder(String),
    Tag(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// `modificationTime`.
    #[default]
    DateAdded,
    Name,
    Size,
    Dimensions,
}

impl SortKey {
    pub const ALL: [SortKey; 4] = [SortKey::DateAdded, SortKey::Name, SortKey::Size, SortKey::Dimensions];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::DateAdded => "Date added",
            SortKey::Name => "Name",
            SortKey::Size => "Size",
            SortKey::Dimensions => "Dimensions",
        }
    }
}

/// Ids of `folder` and all its descendants.
pub fn subtree_ids(folder: &Folder) -> HashSet<&str> {
    let mut out = HashSet::new();
    folder.walk(&mut |f| {
        out.insert(f.id.as_str());
    });
    out
}

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

/// Number of live (not trashed) items in each folder, counting its subfolders.
/// An item in both a folder and its child counts once.
pub fn folder_counts(roots: &[Folder], items: &[Item]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for r in roots {
        r.walk(&mut |f| {
            let ids = subtree_ids(f);
            let n = items
                .iter()
                .filter(|i| !i.is_deleted && i.folders.iter().any(|x| ids.contains(x.as_str())))
                .count();
            out.insert(f.id.clone(), n);
        });
    }
    out
}

/// Live tags with their item counts, most used first, then alphabetical.
pub fn tag_counts(items: &[Item]) -> Vec<(String, usize)> {
    let mut map: HashMap<&str, usize> = HashMap::new();
    for i in items.iter().filter(|i| !i.is_deleted) {
        for t in &i.tags {
            *map.entry(t).or_default() += 1;
        }
    }
    let mut v: Vec<_> = map.into_iter().map(|(t, n)| (t.to_string(), n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    v
}

/// Case-insensitive substring match on name, tags, annotation and extension.
/// An empty query matches everything.
pub fn matches_query(item: &Item, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let has = |s: &str| s.to_lowercase().contains(&q);
    has(&item.name) || has(&item.ext) || has(&item.annotation) || item.tags.iter().any(|t| has(t))
}

fn in_scope(item: &Item, scope: &Scope, folder_ids: Option<&HashSet<&str>>) -> bool {
    match scope {
        Scope::Trash => item.is_deleted,
        _ if item.is_deleted => false,
        Scope::All => true,
        Scope::Uncategorized => item.folders.is_empty(),
        Scope::Folder(_) => folder_ids.is_some_and(|ids| item.folders.iter().any(|f| ids.contains(f.as_str()))),
        Scope::Tag(t) => item.tags.iter().any(|x| x == t),
    }
}

/// Indices into `items` that pass the scope and the search query, sorted.
pub fn visible(
    items: &[Item],
    roots: &[Folder],
    scope: &Scope,
    query: &str,
    key: SortKey,
    ascending: bool,
) -> Vec<usize> {
    let ids = match scope {
        Scope::Folder(id) => find_folder(roots, id).map(subtree_ids),
        _ => None,
    };
    let mut out: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| in_scope(i, scope, ids.as_ref()) && matches_query(i, query))
        .map(|(n, _)| n)
        .collect();
    out.sort_by(|&a, &b| {
        let (x, y) = (&items[a], &items[b]);
        let ord = match key {
            SortKey::DateAdded => x.modification_time.cmp(&y.modification_time),
            SortKey::Name => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
            SortKey::Size => x.size.cmp(&y.size),
            SortKey::Dimensions => (x.width * x.height).cmp(&(y.width * y.height)),
        };
        let ord = if ascending { ord } else { ord.reverse() };
        ord.then_with(|| x.id.cmp(&y.id))
    });
    out
}

/// Human-readable byte size.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

/// Local date and time for a millisecond timestamp.
pub fn format_ms(ms: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => "-".into(),
    }
}

/// `#rrggbb`.
pub fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[allow(clippy::too_many_arguments)]
    fn item(id: &str, name: &str, folders: &[&str], tags: &[&str], deleted: bool, t: u64, size: u64, w: u64) -> Item {
        let v = json!({
            "id": id, "name": name, "size": size, "btime": 0, "mtime": 0, "ext": "png",
            "tags": tags, "folders": folders, "isDeleted": deleted, "url": "",
            "annotation": "Hello Note", "modificationTime": t, "height": 10, "width": w,
            "lastModified": 0, "palettes": []
        });
        Item::from_json_str(&v.to_string()).unwrap()
    }

    fn folder(id: &str, children: Vec<Folder>) -> Folder {
        let v = json!({
            "id": id, "name": id, "description": "", "children": children,
            "modificationTime": 0, "tags": [], "password": "", "passwordTips": ""
        });
        serde_json::from_value(v).unwrap()
    }

    fn fixture() -> (Vec<Folder>, Vec<Item>) {
        let roots = vec![folder("A", vec![folder("B", vec![])]), folder("C", vec![])];
        let items = vec![
            item("1", "Zebra", &["A"], &["red", "Big"], false, 30, 100, 5),
            item("2", "apple", &["B"], &["red"], false, 10, 300, 50),
            item("3", "Mango", &["A", "B"], &[], false, 20, 200, 20),
            item("4", "gone", &["C"], &["red"], true, 40, 1, 1),
            item("5", "loose", &[], &["blue"], false, 50, 2, 2),
        ];
        (roots, items)
    }

    fn names(items: &[Item], v: &[usize]) -> Vec<String> {
        v.iter().map(|&i| items[i].name.clone()).collect()
    }

    #[test]
    fn counts_include_subfolders_once() {
        let (roots, items) = fixture();
        let c = folder_counts(&roots, &items);
        assert_eq!(c["A"], 3);
        assert_eq!(c["B"], 2);
        assert_eq!(c["C"], 0); // its only item is trashed
    }

    #[test]
    fn tag_counts_skip_trash_and_sort() {
        let (_, items) = fixture();
        let t = tag_counts(&items);
        assert_eq!(t[0], ("red".into(), 2));
        assert_eq!(t.len(), 3);
        assert!(t.contains(&("blue".into(), 1)));
    }

    #[test]
    fn scopes() {
        let (roots, items) = fixture();
        let v = |s: Scope| visible(&items, &roots, &s, "", SortKey::Name, true);
        assert_eq!(names(&items, &v(Scope::All)), ["apple", "loose", "Mango", "Zebra"]);
        assert_eq!(names(&items, &v(Scope::Uncategorized)), ["loose"]);
        assert_eq!(names(&items, &v(Scope::Trash)), ["gone"]);
        assert_eq!(names(&items, &v(Scope::Folder("A".into()))), ["apple", "Mango", "Zebra"]);
        assert_eq!(names(&items, &v(Scope::Folder("B".into()))), ["apple", "Mango"]);
        assert_eq!(names(&items, &v(Scope::Tag("red".into()))), ["apple", "Zebra"]);
    }

    #[test]
    fn search_is_case_insensitive_over_fields() {
        let (roots, items) = fixture();
        let q = |s: &str| visible(&items, &roots, &Scope::All, s, SortKey::Name, true).len();
        assert_eq!(q("ZEB"), 1);
        assert_eq!(q("big"), 1); // tag
        assert_eq!(q("hello"), 4); // annotation
        assert_eq!(q("PNG"), 4); // ext
        assert_eq!(q("nothing"), 0);
        assert_eq!(q("  "), 4);
    }

    #[test]
    fn sorting() {
        let (roots, items) = fixture();
        let s = |k, asc| names(&items, &visible(&items, &roots, &Scope::All, "", k, asc));
        assert_eq!(s(SortKey::DateAdded, true), ["apple", "Mango", "Zebra", "loose"]);
        assert_eq!(s(SortKey::DateAdded, false), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(SortKey::Size, false), ["apple", "Mango", "Zebra", "loose"]);
        assert_eq!(s(SortKey::Dimensions, true), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(SortKey::Name, false), ["Zebra", "Mango", "loose", "apple"]);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(hex([255, 0, 16]), "#ff0010");
    }
}

/// One row of the justified grid: items `start..end` drawn `height` tall.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    pub height: f32,
}

/// Packs items with the given aspect ratios (width / height) into rows of
/// `width`, each close to `target` tall; full rows are scaled to fit exactly.
pub fn justify(aspects: &[f32], width: f32, target: f32, gap: f32) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut start, mut sum) = (0, 0.0f32);
    for (i, &a) in aspects.iter().enumerate() {
        sum += a;
        let n = (i + 1 - start) as f32;
        let avail = width - gap * (n - 1.0);
        if sum * target >= avail {
            rows.push(Row { start, end: i + 1, height: avail / sum });
            start = i + 1;
            sum = 0.0;
        }
    }
    if start < aspects.len() {
        rows.push(Row { start, end: aspects.len(), height: target });
    }
    rows
}

#[cfg(test)]
mod justify_tests {
    use super::*;

    #[test]
    fn rows_fill_width() {
        let rows = justify(&[1.0; 7], 400.0, 100.0, 0.0);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].start, rows[0].end), (0, 4));
        assert!((rows[0].height - 100.0).abs() < 1e-3);
        assert_eq!(rows[1], Row { start: 4, end: 7, height: 100.0 });
    }

    #[test]
    fn gaps_are_accounted_for() {
        let rows = justify(&[2.0, 2.0, 2.0], 410.0, 100.0, 10.0);
        // two items: 400 wide at h=100 needs 410 incl. gap -> closes at the second
        assert_eq!(rows[0].end, 2);
        let used = rows[0].height * 4.0 + 10.0;
        assert!((used - 410.0).abs() < 1e-3);
    }

    #[test]
    fn empty() {
        assert!(justify(&[], 100.0, 50.0, 4.0).is_empty());
    }
}
