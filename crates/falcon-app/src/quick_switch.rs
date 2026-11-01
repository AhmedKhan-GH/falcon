//! Go to Folder, Eagle's quick switcher: which rows each tab lists for a
//! typed keyword, and how Tab moves between the tabs. `ui/dialogs/quick_switch.rs`
//! draws it.

use std::collections::HashMap;

use falcon_library::model::{Folder, Item, TagGroup};
use falcon_library::search::{self, Query};
use falcon_library::smart::SmartFolder;

/// With more folders than this (and some history) the Folders tab starts
/// with the recently opened ones.
pub const RECENT_FOLDERS_MIN: usize = 15;
/// The same for smart folders.
pub const RECENT_SMART_MIN: usize = 10;
/// How many recent rows are listed first.
pub const RECENT_SHOWN: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Folders,
    Tags,
    Smart,
    Items,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Folders, Tab::Tags, Tab::Smart, Tab::Items];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Folders => "Folders",
            Tab::Tags => "Tags",
            Tab::Smart => "Smart Folders",
            Tab::Items => "Items",
        }
    }

    /// The tab after this one (Tab key), wrapping.
    pub fn next(self) -> Tab {
        Tab::ALL[(Tab::ALL.iter().position(|t| *t == self).unwrap_or(0) + 1) % 4]
    }

    /// The tab before this one (Shift+Tab), wrapping.
    pub fn prev(self) -> Tab {
        Tab::ALL[(Tab::ALL.iter().position(|t| *t == self).unwrap_or(0) + 3) % 4]
    }

    /// What the tab says when it has nothing to list, for an empty library
    /// part or a keyword with no match.
    pub fn empty_text(self, searching: bool) -> &'static str {
        match (self, searching) {
            (Tab::Folders, false) => "No folders",
            (Tab::Folders, true) => "No folders found",
            (Tab::Tags, false) => "No tags",
            (Tab::Tags, true) => "No tags found",
            (Tab::Smart, false) => "No smart folders",
            (Tab::Smart, true) => "No smart folders found",
            (Tab::Items, false) => "No items",
            (Tab::Items, true) => "No items found",
        }
    }
}

/// One row of the list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Folder or smart folder id, tag name, or item id.
    pub id: String,
    pub name: String,
    pub depth: usize,
    /// Parent folder, tag group, or item extension, shown muted at the right.
    pub detail: Option<String>,
    /// A row from the history, listed before the tree.
    pub recent: bool,
    /// The folder's colour key.
    pub color: Option<String>,
}

/// A folder or smart folder flattened in pre-order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub depth: usize,
    pub parent: Option<String>,
    pub color: Option<String>,
}

pub fn folder_nodes(roots: &[Folder]) -> Vec<Node> {
    fn go(f: &Folder, depth: usize, parent: Option<&str>, out: &mut Vec<Node>) {
        out.push(Node { id: f.id.clone(), name: f.name.clone(), depth, parent: parent.map(str::to_string), color: f.icon_color.clone() });
        for c in &f.children {
            go(c, depth + 1, Some(&f.name), out);
        }
    }
    let mut out = Vec::new();
    for r in roots {
        go(r, 0, None, &mut out);
    }
    out
}

pub fn smart_nodes(roots: &[SmartFolder]) -> Vec<Node> {
    fn go(f: &SmartFolder, depth: usize, parent: Option<&str>, out: &mut Vec<Node>) {
        out.push(Node { id: f.id.clone(), name: f.name.clone(), depth, parent: parent.map(str::to_string), color: f.icon_color.clone() });
        for c in &f.children {
            go(c, depth + 1, Some(&f.name), out);
        }
    }
    let mut out = Vec::new();
    for r in roots {
        go(r, 0, None, &mut out);
    }
    out
}

fn entry(n: &Node, recent: bool) -> Entry {
    Entry { id: n.id.clone(), name: n.name.clone(), depth: n.depth, detail: n.parent.clone(), recent, color: n.color.clone() }
}

/// The rows of the Folders or Smart Folders tab. Without a keyword and with
/// more than `recent_min` nodes, the first up to [`RECENT_SHOWN`] history
/// ids that still exist come first (so they appear twice), then the whole
/// tree. With a keyword the nodes are ranked by name, flat.
pub fn nodes_list(nodes: &[Node], history: &[String], keyword: &str, recent_min: usize) -> Vec<Entry> {
    if !keyword.is_empty() {
        return search::rank_names(nodes.iter().map(|n| n.name.as_str()), keyword).into_iter().map(|i| entry(&nodes[i], false)).collect();
    }
    let mut out = Vec::new();
    if nodes.len() > recent_min {
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        out.extend(history.iter().filter_map(|id| by_id.get(id.as_str())).take(RECENT_SHOWN).map(|n| entry(n, true)));
    }
    out.extend(nodes.iter().map(|n| entry(n, false)));
    out
}

/// Live tags (name, count not needed) in Eagle's order: grouped tags first
/// by their first group's position, then ungrouped, each by plain string
/// order within a group. With a keyword, ranked.
pub fn tags_list(tags: &[String], groups: &[TagGroup], keyword: &str) -> Vec<Entry> {
    let mut first: HashMap<&str, usize> = HashMap::new();
    for (g, group) in groups.iter().enumerate() {
        for t in &group.tags {
            first.entry(t.as_str()).or_insert(g);
        }
    }
    let mut list: Vec<&String> = tags.iter().collect();
    list.sort_by(|a, b| {
        let ga = first.get(a.as_str()).copied().unwrap_or(usize::MAX);
        let gb = first.get(b.as_str()).copied().unwrap_or(usize::MAX);
        ga.cmp(&gb).then_with(|| a.cmp(b))
    });
    let make = |t: &String| Entry {
        id: t.clone(),
        name: t.clone(),
        depth: 0,
        detail: first.get(t.as_str()).map(|g| groups[*g].name.clone()),
        recent: false,
        color: None,
    };
    if keyword.is_empty() {
        return list.into_iter().map(make).collect();
    }
    search::rank_names(list.iter().map(|t| t.as_str()), keyword).into_iter().map(|i| make(list[i])).collect()
}

/// The Items tab: every live item, or those the keyword finds (no typo
/// tolerance).
pub fn items_list(items: &[Item], roots: &[Folder], keyword: &str) -> Vec<Entry> {
    let live = items.iter().filter(|i| !i.is_deleted);
    let make = |i: &Item| Entry {
        id: i.id.clone(),
        name: i.name.clone(),
        depth: 0,
        detail: (!i.ext.is_empty()).then(|| i.ext.clone()),
        recent: false,
        color: None,
    };
    if keyword.is_empty() {
        return live.map(make).collect();
    }
    let index = search::folder_index(roots);
    let q = Query::parse(keyword);
    let qlen = keyword.chars().count();
    live.filter(|i| q.matches(&search::search_text(i, &index, qlen))).map(make).collect()
}

/// Moves a highlight by `delta`, staying inside `len` rows.
pub fn step(index: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (index as i64 + delta as i64).clamp(0, len as i64 - 1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vec<Folder> {
        let mut a = Folder::new("A", "Alpha", 0);
        let mut b = Folder::new("B", "Beta", 0);
        b.children.push(Folder::new("C", "Gamma", 0));
        a.children.push(b);
        vec![a, Folder::new("D", "Delta", 0)]
    }

    #[test]
    fn tab_cycles_both_ways() {
        assert_eq!(Tab::Folders.next(), Tab::Tags);
        assert_eq!(Tab::Items.next(), Tab::Folders);
        assert_eq!(Tab::Folders.prev(), Tab::Items);
        assert_eq!(Tab::Smart.prev(), Tab::Tags);
    }

    #[test]
    fn folders_are_pre_order_with_parent_detail() {
        let nodes = folder_nodes(&tree());
        let l = nodes_list(&nodes, &["D".into()], "", RECENT_FOLDERS_MIN);
        let names: Vec<_> = l.iter().map(|e| (e.name.as_str(), e.depth)).collect();
        assert_eq!(names, [("Alpha", 0), ("Beta", 1), ("Gamma", 2), ("Delta", 0)], "few folders: no recents");
        assert_eq!(l[2].detail.as_deref(), Some("Beta"));
        assert_eq!(l[0].detail, None);
    }

    #[test]
    fn recents_come_first_for_big_libraries_and_repeat() {
        let nodes: Vec<Node> = (0..16).map(|n| Node { id: n.to_string(), name: format!("f{n}"), depth: 0, parent: None, color: None }).collect();
        let hist: Vec<String> = ["5", "gone", "9"].iter().map(|s| s.to_string()).collect();
        let l = nodes_list(&nodes, &hist, "", RECENT_FOLDERS_MIN);
        assert_eq!(l.len(), 18);
        assert_eq!((l[0].id.as_str(), l[0].recent), ("5", true));
        assert_eq!((l[1].id.as_str(), l[1].recent), ("9", true));
        assert_eq!((l[2].id.as_str(), l[2].recent), ("0", false));
        assert_eq!(l.iter().filter(|e| e.id == "5").count(), 2);
        let small = nodes_list(&nodes[..15], &hist, "", RECENT_FOLDERS_MIN);
        assert_eq!(small.len(), 15);
        let ten = nodes_list(&nodes, &hist, "", RECENT_SMART_MIN);
        assert!(ten[0].recent);
        let kw = nodes_list(&nodes, &hist, "f5", RECENT_FOLDERS_MIN);
        assert!(!kw.is_empty() && kw.iter().all(|e| !e.recent));
        assert_eq!(kw[0].id, "5");
    }

    #[test]
    fn history_is_capped_at_ten() {
        let nodes: Vec<Node> = (0..30).map(|n| Node { id: n.to_string(), name: format!("f{n}"), depth: 0, parent: None, color: None }).collect();
        let hist: Vec<String> = (0..20).map(|n| n.to_string()).collect();
        let l = nodes_list(&nodes, &hist, "", RECENT_FOLDERS_MIN);
        assert_eq!(l.iter().filter(|e| e.recent).count(), 10);
    }

    fn group(name: &str, tags: &[&str]) -> TagGroup {
        TagGroup { id: name.into(), name: name.into(), tags: tags.iter().map(|s| s.to_string()).collect(), color: None, description: None, extra: Default::default() }
    }

    #[test]
    fn tags_order_by_group_then_plain_string_order() {
        let tags: Vec<String> = ["b", "Z", "a", "m", "c"].iter().map(|s| s.to_string()).collect();
        let groups = [group("G1", &["m", "a"]), group("G2", &["c", "a"])];
        let l = tags_list(&tags, &groups, "");
        let order: Vec<_> = l.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(order, ["a", "m", "c", "Z", "b"]);
        assert_eq!(l[0].detail.as_deref(), Some("G1"));
        assert_eq!(l[2].detail.as_deref(), Some("G2"));
        assert_eq!(l[3].detail, None);
        let k = tags_list(&tags, &groups, "m");
        assert_eq!(k[0].name, "m");
    }

    #[test]
    fn items_skip_deleted_and_match_keywords() {
        let item = |id: &str, name: &str, del: bool| -> Item {
            serde_json::from_value(serde_json::json!({"id": id, "name": name, "ext": "png", "tags": ["cat"], "folders": [], "isDeleted": del})).unwrap()
        };
        let items = [item("1", "sunset", false), item("2", "sunrise", true), item("3", "mountain", false)];
        assert_eq!(items_list(&items, &[], "").len(), 2);
        let l = items_list(&items, &[], "sun");
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].id.as_str(), l[0].detail.as_deref()), ("1", Some("png")));
        assert_eq!(items_list(&items, &[], "cat").len(), 2);
        assert!(items_list(&items, &[], "sunet").is_empty(), "no typo tolerance");
    }

    #[test]
    fn step_clamps() {
        assert_eq!(step(0, -1, 5), 0);
        assert_eq!(step(4, 1, 5), 4);
        assert_eq!(step(1, 1, 5), 2);
        assert_eq!(step(0, 1, 0), 0);
    }
}
