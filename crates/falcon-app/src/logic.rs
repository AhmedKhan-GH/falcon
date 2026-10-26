//! UI-independent logic: filtering, sorting, counting and formatting.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use falcon_library::model::{Folder, Item};
use falcon_library::search::{self, Filter, OrderBy, Query};

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
    Trash,
    /// Items placed directly in a folder (Eagle hides subfolder content by
    /// default).
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
            Scope::Trash => "Trash".into(),
            Scope::Folder(id) => find_folder(roots, id).map_or_else(|| "Folder".into(), |f| f.name.clone()),
            Scope::Tag(t) => t.clone(),
            Scope::Smart(_) => "Smart Folder".into(),
        }
    }
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

/// Counts for the fixed sidebar entries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NavCounts {
    pub all: usize,
    pub uncategorized: usize,
    pub untagged: usize,
    pub trash: usize,
    pub tags: usize,
}

pub fn nav_counts(items: &[Item]) -> NavCounts {
    let mut c = NavCounts::default();
    let mut tags = HashSet::new();
    for i in items {
        if i.is_deleted {
            c.trash += 1;
            continue;
        }
        c.all += 1;
        c.uncategorized += usize::from(i.folders.is_empty());
        c.untagged += usize::from(i.tags.is_empty());
        tags.extend(i.tags.iter());
    }
    c.tags = tags.len();
    c
}

/// Live tags with their item counts, sorted by name.
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

fn in_scope(item: &Item, scope: &Scope) -> bool {
    match scope {
        Scope::Trash => item.is_deleted,
        _ if item.is_deleted => false,
        Scope::All | Scope::Recent | Scope::Random(_) | Scope::AllTags => true,
        Scope::Uncategorized => item.folders.is_empty(),
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

#[allow(clippy::too_many_arguments)]
/// Indices into `items` that pass the scope, the search query and the filter
/// panel, in Eagle's order. A colour filter sorts by colour distance.
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
    let index = search::folder_index(roots);
    let q = Query::parse(query);
    let qlen = query.trim().chars().count();
    let mut hits: Vec<(usize, f64)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| in_scope(i, scope) && only.is_none_or(|o| o.contains(&i.id)))
        .filter(|(_, i)| q.is_empty() || q.matches(&search::search_text(i, &index, qlen)))
        .filter_map(|(n, i)| filter.check(i).map(|d| (n, d)))
        .collect();
    match scope {
        Scope::Random(seed) => hits.sort_by_key(|&(i, _)| shuffle_key(*seed, &items[i].id)),
        Scope::Recent => hits.sort_by(|a, b| items[b.0].last_modified.cmp(&items[a.0].last_modified)),
        Scope::Trash => hits.sort_by_key(|&(i, _)| {
            let it = &items[i];
            std::cmp::Reverse(it.extra.get("deletedTime").and_then(|v| v.as_u64()).unwrap_or(it.modification_time))
        }),
        _ if filter.color.is_some() => hits.sort_by(|a, b| a.1.total_cmp(&b.1)),
        _ => {
            let mut refs: Vec<&Item> = hits.iter().map(|&(i, _)| &items[i]).collect();
            search::sort(&mut refs, order, increase);
            let pos: HashMap<&str, usize> = hits.iter().map(|&(i, _)| (items[i].id.as_str(), i)).collect();
            return refs.iter().map(|it| pos[it.id.as_str()]).collect();
        }
    }
    hits.into_iter().map(|(i, _)| i).collect()
}

/// Byte size the way Eagle prints it: `512 B`, `5.34 KB`, `17.19 KB`.
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
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} {}", UNITS[u])
}

/// Local date and time the way Eagle prints it: `2026/10/26 2:15`.
pub fn format_ms(ms: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(t) => t.format("%Y/%m/%d %-H:%M").to_string(),
        None => "-".into(),
    }
}

/// `#RRGGBB`.
pub fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// Grid layout: how many square cards of `size` fit in `width`.
pub fn columns(width: f32, size: f32, gap: f32) -> usize {
    (((width + gap) / (size + gap)).floor() as usize).max(1)
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
            "lastModified": t, "palettes": []
        });
        Item::from_json_str(&v.to_string()).unwrap()
    }

    fn folder(id: &str, children: Vec<Folder>) -> Folder {
        let v = json!({
            "id": id, "name": format!("{id}-name"), "description": "", "children": children,
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
    fn folder_counts_are_direct_and_skip_trash() {
        let (_, items) = fixture();
        let c = folder_counts(&items);
        assert_eq!(c["A"], 2);
        assert_eq!(c["B"], 2);
        assert!(!c.contains_key("C"));
    }

    #[test]
    fn nav_counts_cover_fixed_entries() {
        let (_, items) = fixture();
        assert_eq!(nav_counts(&items), NavCounts { all: 4, uncategorized: 1, untagged: 1, trash: 1, tags: 3 });
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
        assert_eq!(v(Scope::Recent), ["loose", "Zebra", "Mango", "apple"]);
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
    fn sorting_follows_eagle_directions() {
        let (roots, items) = fixture();
        let s = |k, inc| names(&items, &visible(&items, &roots, &Scope::All, "", &Filter::default(), k, inc, None));
        // Time keys are newest first when "increase" is on.
        assert_eq!(s(OrderBy::Import, true), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(OrderBy::Import, false), ["apple", "Mango", "Zebra", "loose"]);
        assert_eq!(s(OrderBy::FileSize, true), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(OrderBy::Resolution, false), ["apple", "Mango", "Zebra", "loose"]);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(5465), "5.34 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(hex([255, 0, 16]), "#FF0010");
        assert_eq!(columns(300.0, 100.0, 8.0), 2);
    }
}
