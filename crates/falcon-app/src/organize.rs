//! UI-independent logic for organising: drop targets in the sidebar trees,
//! locked folders, a folder's own sort, tag group sections and saved
//! filters. The egui code in `app.rs` only draws and dispatches.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use falcon_library::model::{Folder, FolderOrder, Item, TagGroup};
use falcon_library::search::{self, OrderBy};
use falcon_library::smart::SmartFolder;

// ---------------------------------------------------------------------------
// Trees

/// A node of a sidebar tree: folders and smart folders.
pub trait Node: Sized {
    fn id(&self) -> &str;
    fn kids(&self) -> &[Self];
}

impl Node for Folder {
    fn id(&self) -> &str {
        &self.id
    }
    fn kids(&self) -> &[Self] {
        &self.children
    }
}

impl Node for SmartFolder {
    fn id(&self) -> &str {
        &self.id
    }
    fn kids(&self) -> &[Self] {
        &self.children
    }
}

/// Finds a node anywhere in the tree.
pub fn find<'a, T: Node>(roots: &'a [T], id: &str) -> Option<&'a T> {
    roots.iter().find_map(|n| if n.id() == id { Some(n) } else { find(n.kids(), id) })
}

/// A node's parent id (`None` at the top level) and its index among its
/// siblings.
pub fn locate<'a, T: Node>(roots: &'a [T], id: &str) -> Option<(Option<&'a str>, usize)> {
    fn go<'a, T: Node>(list: &'a [T], parent: Option<&'a str>, id: &str) -> Option<(Option<&'a str>, usize)> {
        for (i, n) in list.iter().enumerate() {
            if n.id() == id {
                return Some((parent, i));
            }
            if let Some(hit) = go(n.kids(), Some(n.id()), id) {
                return Some(hit);
            }
        }
        None
    }
    go(roots, None, id)
}

/// Whether `id` is `node` or one of its descendants.
pub fn within<T: Node>(node: &T, id: &str) -> bool {
    node.id() == id || node.kids().iter().any(|k| within(k, id))
}

/// Ids of the path from the top level down to `id`, ending with `id`.
pub fn path_to<T: Node>(roots: &[T], id: &str) -> Vec<String> {
    fn go<T: Node>(list: &[T], id: &str, path: &mut Vec<String>) -> bool {
        for n in list {
            path.push(n.id().to_string());
            if n.id() == id || go(n.kids(), id, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    go(roots, id, &mut path);
    path
}

/// Every node with its depth, depth first, leaving out `exclude` and its
/// subtree (a node can't become its own descendant's child).
pub fn flatten<'a, T: Node>(roots: &'a [T], exclude: Option<&str>) -> Vec<(&'a T, usize)> {
    fn go<'a, T: Node>(list: &'a [T], depth: usize, exclude: Option<&str>, out: &mut Vec<(&'a T, usize)>) {
        for n in list {
            if Some(n.id()) == exclude {
                continue;
            }
            out.push((n, depth));
            go(n.kids(), depth + 1, exclude, out);
        }
    }
    let mut out = Vec::new();
    go(roots, 0, exclude, &mut out);
    out
}

/// Where a drop over a sidebar row lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    /// On the insertion line above the row.
    Before,
    /// Onto the row: nest inside it.
    Into,
    /// On the insertion line below the row.
    After,
}

/// Share of a row's height at its top and at its bottom that reorders;
/// the middle half nests.
pub const DROP_EDGE: f32 = 0.25;

/// The zone of a row (`top`, `height`) the pointer at `y` is over. Rows
/// that can't take children split in half.
pub fn drop_zone(top: f32, height: f32, y: f32, can_nest: bool) -> DropZone {
    let f = if height > 0.0 { (y - top) / height } else { 0.5 };
    let edge = if can_nest { DROP_EDGE } else { 0.5 };
    if f < edge {
        DropZone::Before
    } else if f >= 1.0 - edge {
        DropZone::After
    } else {
        DropZone::Into
    }
}

/// The move a drop asks for: the new parent and the index among its
/// children counted after the dragged node has left its old place (what
/// `Library::reorder_folder` and `move_smart_folder` take; `usize::MAX`
/// appends). `None` when the drop is refused (onto itself or into its own
/// subtree) or changes nothing.
pub fn tree_drop<T: Node>(roots: &[T], dragged: &str, target: &str, zone: DropZone) -> Option<(Option<String>, usize)> {
    let node = find(roots, dragged)?;
    if within(node, target) {
        return None;
    }
    let (from_parent, from) = locate(roots, dragged)?;
    if zone == DropZone::Into {
        return Some((Some(target.to_string()), usize::MAX));
    }
    let (parent, at) = locate(roots, target)?;
    let mut pos = if zone == DropZone::After { at + 1 } else { at };
    if from_parent == parent && from < pos {
        pos -= 1;
    }
    if from_parent == parent && from == pos {
        return None;
    }
    Some((parent.map(str::to_string), pos))
}

// ---------------------------------------------------------------------------
// Locked folders

/// The ids of folders whose items are hidden: every locked folder not
/// unlocked this session, with all of its subfolders.
pub fn hidden_folders(roots: &[Folder], unlocked: &HashSet<String>) -> HashSet<String> {
    let mut out = HashSet::new();
    for f in roots {
        f.walk(&mut |x| {
            if x.is_locked() && !unlocked.contains(&x.id) {
                x.walk(&mut |y| {
                    out.insert(y.id.clone());
                });
            }
        });
    }
    out
}

/// Whether a folder is locked and not yet unlocked.
pub fn is_shut(f: &Folder, unlocked: &HashSet<String>) -> bool {
    f.is_locked() && !unlocked.contains(&f.id)
}

/// The outermost still-locked folder on the way to `id` (itself
/// included): the folder whose password must be typed to browse `id`.
pub fn lock_gate<'a>(roots: &'a [Folder], id: &str, unlocked: &HashSet<String>) -> Option<&'a Folder> {
    path_to(roots, id).iter().filter_map(|p| find(roots, p)).find(|f| is_shut(f, unlocked))
}

/// Whether an item is filed in any hidden folder.
pub fn in_hidden(item: &Item, hidden: &HashSet<String>) -> bool {
    !hidden.is_empty() && item.folders.iter().any(|f| hidden.contains(f))
}

/// `order` (indices into `items`) without items filed in a hidden folder.
pub fn without_hidden(order: Vec<usize>, items: &[Item], hidden: &HashSet<String>) -> Vec<usize> {
    if hidden.is_empty() {
        return order;
    }
    order.into_iter().filter(|&i| !in_hidden(&items[i], hidden)).collect()
}

/// Checks the password dialog. Creating needs the password twice;
/// changing needs the old one (checked by `old_ok`) and the new one twice;
/// removing needs the current one. Eagle's `btoa` can't store characters
/// past U+00FF, so those are refused here too.
pub fn check_password_form(create: bool, remove: bool, new: &str, again: &str, old_ok: impl FnOnce() -> bool) -> Result<(), &'static str> {
    if !create && !old_ok() {
        return Err(if remove { "The password is wrong." } else { "The old password is wrong." });
    }
    if remove {
        return Ok(());
    }
    if new.is_empty() {
        return Err("Enter a password.");
    }
    if new.chars().any(|c| c as u32 > 0xff) {
        return Err("Use only Latin letters, digits and symbols.");
    }
    if new != again {
        return Err("The passwords don't match.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// A folder's own sort

/// Folder sort keys in the order the folder menu lists them.
pub const FOLDER_ORDERS: [FolderOrder; 11] = [
    FolderOrder::By(OrderBy::Import),
    FolderOrder::By(OrderBy::Mtime),
    FolderOrder::By(OrderBy::Btime),
    FolderOrder::By(OrderBy::Name),
    FolderOrder::By(OrderBy::Ext),
    FolderOrder::By(OrderBy::FileSize),
    FolderOrder::By(OrderBy::Resolution),
    FolderOrder::By(OrderBy::Rating),
    FolderOrder::By(OrderBy::Tags),
    FolderOrder::Duration,
    FolderOrder::Random,
];

/// Menu label of a folder sort key.
pub fn order_label(o: FolderOrder) -> &'static str {
    match o {
        FolderOrder::By(k) => k.label(),
        FolderOrder::Duration => "Duration",
        FolderOrder::Random => "Random",
        FolderOrder::Manual => "Manual",
    }
}

/// The two direction labels for a key (time keys read newest/oldest).
pub fn direction_labels(o: FolderOrder) -> [&'static str; 2] {
    match o {
        FolderOrder::By(k) if k.is_time() => ["Newest First", "Oldest First"],
        _ => ["Ascending", "Descending"],
    }
}

/// A folder's or smart folder's stored sort, if it has one falcon knows.
/// `sortIncrease` defaults to `true`, as Eagle sets it with the key.
pub fn own_order(order_by: Option<&str>, increase: Option<bool>) -> Option<(FolderOrder, bool)> {
    order_by.and_then(FolderOrder::parse).map(|o| (o, increase.unwrap_or(true)))
}

/// The key `logic::visible` should sort by in a view with its own sort:
/// keys the main list knows pass through; duration, random and manual
/// start from import order and are finished by [`finish_order`].
pub fn base_key(own: Option<(FolderOrder, bool)>, global: (OrderBy, bool)) -> (OrderBy, bool) {
    match own {
        Some((FolderOrder::By(k), inc)) => (k, inc),
        Some((FolderOrder::Manual, _)) => (OrderBy::Import, true),
        Some(_) => (OrderBy::Import, true),
        None => global,
    }
}

/// Applies the keys the main list can't sort by. Random shuffles by
/// `seed` (fixed for one visit); duration sorts by length, shortest first
/// when `increase`, items without one last. Manual sorts by each item's
/// hand-arranged position in `folder` ([`search::sort_manual`]); outside a
/// folder it keeps the incoming order.
pub fn finish_order(order: &mut [usize], items: &[Item], own: Option<(FolderOrder, bool)>, folder: Option<&str>, seed: u64) {
    match own {
        Some((FolderOrder::Random, _)) => order.sort_by_key(|&i| shuffle_key(seed, &items[i].id)),
        Some((FolderOrder::Duration, inc)) => order.sort_by(|&a, &b| {
            match (search::duration(&items[a]), search::duration(&items[b])) {
                (Some(x), Some(y)) => if inc { x.total_cmp(&y) } else { y.total_cmp(&x) },
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        }),
        Some((FolderOrder::Manual, inc)) => {
            let Some(folder) = folder else { return };
            let mut refs: Vec<&Item> = order.iter().map(|&i| &items[i]).collect();
            search::sort_manual(&mut refs, folder, inc);
            let pos: std::collections::HashMap<&str, usize> = order.iter().map(|&i| (items[i].id.as_str(), i)).collect();
            for (slot, it) in order.iter_mut().zip(refs) {
                *slot = pos[it.id.as_str()];
            }
        }
        _ => {}
    }
}

/// Whether items in the grid can be dragged into a new hand-made order,
/// as Eagle allows: only inside a folder, while its sort is Manual or
/// import time (its own, or the global one when it has none), and with no
/// filter or search narrowing the view. Dropping switches the folder to
/// Manual.
pub fn manual_sortable(in_folder: bool, own: Option<(FolderOrder, bool)>, global: OrderBy, filtered: bool, searching: bool) -> bool {
    let key_ok = match own {
        Some((FolderOrder::Manual | FolderOrder::By(OrderBy::Import), _)) => true,
        Some(_) => false,
        None => global == OrderBy::Import,
    };
    in_folder && key_ok && !filtered && !searching
}

/// The `before` item for [`falcon_library::Library::move_items_manually`]
/// when `moving` items are dropped before or `after` the shown item at
/// `target` in `order`: the first item at the drop point that isn't
/// moving, or `None` for the end.
pub fn manual_before(order: &[String], moving: &[String], target: usize, after: bool) -> Option<String> {
    let start = if after { target + 1 } else { target };
    order.iter().skip(start).find(|id| !moving.contains(id)).cloned()
}

/// Whether moving `moving` (as one block, in their shown order) to just
/// before `before` (`None`: the end) would leave `order` as it is.
pub fn manual_noop(order: &[String], moving: &[String], before: Option<&str>) -> bool {
    let block: Vec<&String> = order.iter().filter(|id| moving.contains(id)).collect();
    let mut out: Vec<&String> = order.iter().filter(|id| !moving.contains(id)).collect();
    let at = before.and_then(|b| out.iter().position(|id| *id == b)).unwrap_or(out.len());
    out.splice(at..at, block);
    out.iter().map(|s| s.as_str()).eq(order.iter().map(String::as_str))
}

fn shuffle_key(seed: u64, id: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    id.hash(&mut h);
    h.finish()
}

/// The icon name a smart folder is drawn with: its own, else `grid` for a
/// group (the icon Eagle gives new groups), else none (the smart folder
/// icon).
pub fn smart_icon(sf: &SmartFolder) -> Option<&str> {
    match sf.icon.as_deref().filter(|i| !i.is_empty()) {
        None if sf.is_group() => Some("grid"),
        icon => icon,
    }
}

// ---------------------------------------------------------------------------
// Tag groups

/// One group on the All Tags page with its member tags and their counts
/// (0 for a member no live item carries).
#[derive(Debug, Clone, PartialEq)]
pub struct TagSection {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub tags: Vec<(String, usize)>,
}

/// Splits the tag list into groups (in the library's order, members in
/// group order) and the tags in no group. `filter` (lowercase) keeps
/// matching tags; a group whose name matches keeps all its members.
/// Groups left empty by the filter are dropped, unless the filter is empty.
pub fn tag_sections(counts: &[(String, usize)], groups: &[TagGroup], filter: &str) -> (Vec<TagSection>, Vec<(String, usize)>) {
    let matches = |t: &str| filter.is_empty() || t.to_lowercase().contains(filter);
    let count = |t: &str| counts.iter().find(|(x, _)| x == t).map_or(0, |(_, n)| *n);
    let mut grouped = HashSet::new();
    let mut out = Vec::new();
    for g in groups {
        grouped.extend(g.tags.iter().cloned());
        let all = matches(&g.name);
        let tags: Vec<(String, usize)> = g.tags.iter().filter(|t| all || matches(t)).map(|t| (t.clone(), count(t))).collect();
        if filter.is_empty() || !tags.is_empty() {
            out.push(TagSection { id: g.id.clone(), name: g.name.clone(), color: g.color.clone(), tags });
        }
    }
    let rest = counts.iter().filter(|(t, _)| !grouped.contains(t) && matches(t)).cloned().collect();
    (out, rest)
}

/// The group a tag is filed in, if any.
pub fn group_of<'a>(groups: &'a [TagGroup], tag: &str) -> Option<&'a TagGroup> {
    groups.iter().find(|g| g.tags.iter().any(|t| t == tag))
}

/// A name for a new group or saved filter that isn't taken yet:
/// `base`, then `base 2`, `base 3`, ...
pub fn fresh_name<'a>(base: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: HashSet<&str> = taken.into_iter().collect();
    if !taken.contains(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} {n}")).find(|s| !taken.contains(s.as_str())).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Saved filters

/// Plain names for the rule keys falcon's filter can't apply yet (from
/// `SavedFilterQuery::unsupported`), for the warning shown on apply.
pub fn unsupported_names(keys: &[String]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for k in keys {
        let name = match k.as_str() {
            "import" => "date imported",
            "mtime" => "date modified",
            "duration" => "duration",
            "bpm" => "BPM",
            "font" => "font status",
            "camera" => "camera",
            "annotation" => "comments",
            "image" => "image search",
            "semantic" => "semantic search",
            "folder.excludes.NoFolders" => "excluding uncategorized",
            s if s.starts_with("shape.") => "aspect ratio",
            _ => "other rules",
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(id: &str, children: Vec<Folder>) -> Folder {
        let v = json!({
            "id": id, "name": id, "description": "", "children": children,
            "modificationTime": 0, "tags": [], "password": "", "passwordTips": ""
        });
        serde_json::from_value(v).unwrap()
    }

    fn locked(mut f: Folder) -> Folder {
        f.password = "MTIz".into();
        f
    }

    fn item(id: &str, folders: &[&str], duration: Option<f64>) -> Item {
        let mut v = json!({
            "id": id, "name": id, "size": 1, "btime": 0, "mtime": 0, "ext": "mp4", "tags": [], "folders": folders,
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": 0, "height": 1, "width": 1,
            "lastModified": 0, "palettes": []
        });
        if let Some(d) = duration {
            v["duration"] = json!(d);
        }
        Item::from_json_str(&v.to_string()).unwrap()
    }

    /// A ─ B, C, D (B, C, D children of A), E at the top level.
    fn tree() -> Vec<Folder> {
        vec![folder("A", vec![folder("B", vec![]), folder("C", vec![]), folder("D", vec![])]), folder("E", vec![])]
    }

    #[test]
    fn zones_split_rows_in_quarters() {
        assert_eq!(drop_zone(100.0, 28.0, 100.0, true), DropZone::Before);
        assert_eq!(drop_zone(100.0, 28.0, 106.9, true), DropZone::Before);
        assert_eq!(drop_zone(100.0, 28.0, 107.0, true), DropZone::Into);
        assert_eq!(drop_zone(100.0, 28.0, 114.0, true), DropZone::Into);
        assert_eq!(drop_zone(100.0, 28.0, 120.9, true), DropZone::Into);
        assert_eq!(drop_zone(100.0, 28.0, 121.0, true), DropZone::After);
        assert_eq!(drop_zone(100.0, 28.0, 127.9, true), DropZone::After);
        // Past the edges still counts as the nearest line.
        assert_eq!(drop_zone(100.0, 28.0, 90.0, true), DropZone::Before);
        assert_eq!(drop_zone(100.0, 28.0, 140.0, true), DropZone::After);
        // Rows that can't nest split in half.
        assert_eq!(drop_zone(100.0, 28.0, 113.0, false), DropZone::Before);
        assert_eq!(drop_zone(100.0, 28.0, 114.0, false), DropZone::After);
    }

    #[test]
    fn tree_lookups() {
        let t = tree();
        assert_eq!(locate(&t, "C"), Some((Some("A"), 1)));
        assert_eq!(locate(&t, "E"), Some((None, 1)));
        assert_eq!(locate(&t, "Z"), None);
        assert_eq!(path_to(&t, "D"), ["A", "D"]);
        assert!(path_to(&t, "Z").is_empty());
        assert!(within(&t[0], "C") && !within(&t[0], "E"));
        let flat: Vec<(&str, usize)> = flatten(&t, None).iter().map(|(f, d)| (f.id.as_str(), *d)).collect();
        assert_eq!(flat, [("A", 0), ("B", 1), ("C", 1), ("D", 1), ("E", 0)]);
        let flat: Vec<&str> = flatten(&t, Some("A")).iter().map(|(f, _)| f.id.as_str()).collect();
        assert_eq!(flat, ["E"]);
    }

    #[test]
    fn drops_between_siblings_count_after_removal() {
        let t = tree();
        let to = |p: Option<&str>, i: usize| Some((p.map(str::to_string), i));
        // B (index 0) dropped after D: D is at 2, insertion at 3, minus B's slot.
        assert_eq!(tree_drop(&t, "B", "D", DropZone::After), to(Some("A"), 2));
        // D dropped before B goes to the front.
        assert_eq!(tree_drop(&t, "D", "B", DropZone::Before), to(Some("A"), 0));
        // Dropping where it already is changes nothing.
        assert_eq!(tree_drop(&t, "C", "B", DropZone::After), None);
        assert_eq!(tree_drop(&t, "C", "D", DropZone::Before), None);
        // Across parents: no adjustment.
        assert_eq!(tree_drop(&t, "E", "C", DropZone::Before), to(Some("A"), 1));
        assert_eq!(tree_drop(&t, "C", "E", DropZone::After), to(None, 2));
        assert_eq!(tree_drop(&t, "C", "A", DropZone::Before), to(None, 0));
        // Into appends to the target's children.
        assert_eq!(tree_drop(&t, "E", "B", DropZone::Into), to(Some("B"), usize::MAX));
    }

    #[test]
    fn drops_into_self_or_subtree_are_refused() {
        let t = tree();
        assert_eq!(tree_drop(&t, "A", "A", DropZone::Into), None);
        assert_eq!(tree_drop(&t, "A", "C", DropZone::Into), None);
        assert_eq!(tree_drop(&t, "A", "C", DropZone::Before), None);
        assert_eq!(tree_drop(&t, "Z", "C", DropZone::Before), None);
    }

    #[test]
    fn smart_folders_use_the_same_drop_rules() {
        let sf = |id: &str, children: Vec<SmartFolder>| SmartFolder {
            id: id.into(),
            name: id.into(),
            conditions: vec![],
            children,
            icon: None,
            icon_color: None,
            description: String::new(),
            order_by: None,
            sort_increase: None,
        };
        let t = vec![sf("S1", vec![sf("S2", vec![])]), sf("S3", vec![])];
        assert_eq!(tree_drop(&t, "S3", "S1", DropZone::Into), Some((Some("S1".into()), usize::MAX)));
        assert_eq!(tree_drop(&t, "S2", "S3", DropZone::After), Some((None, 2)));
        assert_eq!(tree_drop(&t, "S1", "S2", DropZone::Into), None);
    }

    #[test]
    fn locked_folders_hide_their_subtree_until_unlocked() {
        let t = vec![locked(folder("A", vec![folder("B", vec![])])), folder("C", vec![locked(folder("D", vec![]))])];
        let mut unlocked = HashSet::new();
        let hidden = hidden_folders(&t, &unlocked);
        let mut h: Vec<&str> = hidden.iter().map(String::as_str).collect();
        h.sort();
        assert_eq!(h, ["A", "B", "D"]);
        assert_eq!(lock_gate(&t, "B", &unlocked).map(|f| f.id.as_str()), Some("A"));
        assert_eq!(lock_gate(&t, "C", &unlocked), None);

        let items = vec![item("1", &["A"], None), item("2", &["B"], None), item("3", &["C"], None), item("4", &["C", "D"], None), item("5", &[], None)];
        let order: Vec<usize> = (0..items.len()).collect();
        // An item filed anywhere hidden is hidden, even if also filed elsewhere.
        assert_eq!(without_hidden(order.clone(), &items, &hidden), [2, 4]);

        unlocked.insert("A".to_string());
        let hidden = hidden_folders(&t, &unlocked);
        assert_eq!(lock_gate(&t, "B", &unlocked), None);
        assert_eq!(without_hidden(order.clone(), &items, &hidden), [0, 1, 2, 4]);
        assert_eq!(without_hidden(order, &items, &HashSet::new()).len(), 5);
    }

    #[test]
    fn password_form_rules() {
        let ok = || true;
        let bad = || false;
        assert_eq!(check_password_form(true, false, "abc", "abc", bad), Ok(()));
        assert!(check_password_form(true, false, "", "", bad).is_err());
        assert!(check_password_form(true, false, "abc", "abd", bad).is_err());
        assert!(check_password_form(true, false, "密码", "密码", bad).is_err());
        assert_eq!(check_password_form(true, false, "café", "café", bad), Ok(()));
        assert!(check_password_form(false, false, "new", "new", bad).is_err(), "change needs the old password");
        assert_eq!(check_password_form(false, false, "new", "new", ok), Ok(()));
        assert!(check_password_form(false, true, "", "", bad).is_err());
        assert_eq!(check_password_form(false, true, "", "", ok), Ok(()));
    }

    #[test]
    fn own_sort_overrides_the_global_one() {
        assert_eq!(own_order(None, Some(false)), None);
        assert_eq!(own_order(Some("BOGUS"), None), None);
        assert_eq!(own_order(Some("NAME"), None), Some((FolderOrder::By(OrderBy::Name), true)));
        let g = (OrderBy::Rating, false);
        assert_eq!(base_key(None, g), g);
        assert_eq!(base_key(Some((FolderOrder::By(OrderBy::Name), false)), g), (OrderBy::Name, false));
        assert_eq!(base_key(Some((FolderOrder::Random, false)), g), (OrderBy::Import, true));
        assert_eq!(base_key(Some((FolderOrder::Manual, false)), g), (OrderBy::Import, true));
        assert_eq!(direction_labels(FolderOrder::By(OrderBy::Btime))[0], "Newest First");
        assert_eq!(direction_labels(FolderOrder::Duration)[0], "Ascending");
    }

    #[test]
    fn duration_and_random_finish_the_order() {
        let items = vec![item("a", &[], Some(30.0)), item("b", &[], None), item("c", &[], Some(5.5)), item("d", &[], Some(12.0))];
        let mut o = vec![0, 1, 2, 3];
        finish_order(&mut o, &items, Some((FolderOrder::Duration, true)), None, 0);
        assert_eq!(o, [2, 3, 0, 1]);
        finish_order(&mut o, &items, Some((FolderOrder::Duration, false)), None, 0);
        assert_eq!(o, [0, 3, 2, 1]);
        let mut r1 = vec![0, 1, 2, 3];
        let mut r2 = vec![3, 2, 1, 0];
        finish_order(&mut r1, &items, Some((FolderOrder::Random, true)), None, 42);
        finish_order(&mut r2, &items, Some((FolderOrder::Random, true)), None, 42);
        assert_eq!(r1, r2, "one seed gives one order");
        let mut m = vec![2, 0, 1];
        finish_order(&mut m, &items, Some((FolderOrder::Manual, true)), None, 1);
        assert_eq!(m, [2, 0, 1], "manual outside a folder keeps the incoming order");
    }

    #[test]
    fn manual_sorts_by_the_folders_order_map() {
        let mut items = vec![item("a", &["f"], None), item("b", &["f"], None), item("c", &["f"], None), item("d", &["f"], None)];
        items[0].set_manual_order("f", "2");
        items[1].set_manual_order("f", "5");
        items[2].set_manual_order("g", "9");
        items[3].set_manual_order("f", "3.5");
        // Larger values first; c has no entry for f and falls back to its
        // modificationTime (0), which sorts last.
        let mut m = vec![0, 1, 2, 3];
        finish_order(&mut m, &items, Some((FolderOrder::Manual, true)), Some("f"), 0);
        assert_eq!(m, [1, 3, 0, 2]);
        finish_order(&mut m, &items, Some((FolderOrder::Manual, false)), Some("f"), 0);
        assert_eq!(m, [2, 0, 3, 1]);
    }

    #[test]
    fn manual_drag_is_allowed_like_eagle() {
        let g = OrderBy::Name;
        assert!(manual_sortable(true, Some((FolderOrder::Manual, false)), g, false, false));
        assert!(manual_sortable(true, Some((FolderOrder::By(OrderBy::Import), true)), g, false, false));
        assert!(manual_sortable(true, None, OrderBy::Import, false, false));
        assert!(!manual_sortable(true, None, g, false, false));
        assert!(!manual_sortable(true, Some((FolderOrder::Random, true)), OrderBy::Import, false, false));
        assert!(!manual_sortable(false, Some((FolderOrder::Manual, true)), g, false, false), "only inside a folder");
        assert!(!manual_sortable(true, Some((FolderOrder::Manual, true)), g, true, false), "not while filtered");
        assert!(!manual_sortable(true, Some((FolderOrder::Manual, true)), g, false, true), "not while searching");
    }

    #[test]
    fn manual_drop_targets() {
        let order: Vec<String> = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
        let mv = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(manual_before(&order, &mv(&["a"]), 2, false).as_deref(), Some("c"));
        assert_eq!(manual_before(&order, &mv(&["a"]), 2, true).as_deref(), Some("d"));
        assert_eq!(manual_before(&order, &mv(&["d"]), 2, true).as_deref(), Some("e"), "skips moving items");
        assert_eq!(manual_before(&order, &mv(&["a"]), 4, true), None, "after the last is the end");
        assert_eq!(manual_before(&order, &mv(&["e"]), 3, true), None);
        assert!(manual_noop(&order, &mv(&["b"]), Some("c")));
        assert!(manual_noop(&order, &mv(&["e"]), None));
        assert!(manual_noop(&order, &mv(&["b", "c"]), Some("d")));
        assert!(!manual_noop(&order, &mv(&["b"]), Some("d")));
        assert!(!manual_noop(&order, &mv(&["a", "c"]), Some("d")), "a gap closes up");
        assert!(!manual_noop(&order, &mv(&["a"]), None));
    }

    #[test]
    fn groups_fall_back_to_the_grid_icon() {
        let sf = |icon: Option<&str>, cond: serde_json::Value| SmartFolder {
            id: "s".into(),
            name: "s".into(),
            conditions: serde_json::from_value(cond).unwrap(),
            children: Vec::new(),
            icon: icon.map(str::to_string),
            icon_color: None,
            description: String::new(),
            order_by: None,
            sort_increase: None,
        };
        let rule = json!([{"match": "AND", "rules": []}]);
        assert_eq!(smart_icon(&sf(None, json!([]))), Some("grid"));
        assert_eq!(smart_icon(&sf(Some(""), json!([]))), Some("grid"));
        assert_eq!(smart_icon(&sf(Some("rocket"), json!([]))), Some("rocket"));
        assert_eq!(smart_icon(&sf(None, rule.clone())), None);
        assert_eq!(smart_icon(&sf(Some("grid"), rule)), Some("grid"));
    }

    fn group(id: &str, name: &str, tags: &[&str]) -> TagGroup {
        serde_json::from_value(json!({"id": id, "name": name, "tags": tags})).unwrap()
    }

    #[test]
    fn tag_sections_split_grouped_and_loose_tags() {
        let counts = vec![("blue".to_string(), 2), ("cat".to_string(), 5), ("red".to_string(), 1), ("tree".to_string(), 3)];
        let groups = vec![group("g1", "Colours", &["red", "blue", "green"]), group("g2", "Empty", &[])];
        let (secs, rest) = tag_sections(&counts, &groups, "");
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].tags, [("red".to_string(), 1), ("blue".to_string(), 2), ("green".to_string(), 0)]);
        assert!(secs[1].tags.is_empty());
        assert_eq!(rest, [("cat".to_string(), 5), ("tree".to_string(), 3)]);

        let (secs, rest) = tag_sections(&counts, &groups, "re");
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].tags, [("red".to_string(), 1), ("green".to_string(), 0)]);
        assert_eq!(rest, [("tree".to_string(), 3)]);
        // A matching group name keeps every member.
        let (secs, _) = tag_sections(&counts, &groups, "colo");
        assert_eq!(secs[0].tags.len(), 3);
        assert_eq!(group_of(&groups, "blue").map(|g| g.id.as_str()), Some("g1"));
        assert_eq!(group_of(&groups, "cat"), None);
    }

    #[test]
    fn fresh_names_skip_taken_ones() {
        assert_eq!(fresh_name("New Group", []), "New Group");
        assert_eq!(fresh_name("New Group", ["New Group", "New Group 2"]), "New Group 3");
    }

    #[test]
    fn unsupported_rules_have_plain_names() {
        let keys: Vec<String> = ["import", "shape.43", "shape.169", "camera"].iter().map(|s| s.to_string()).collect();
        assert_eq!(unsupported_names(&keys), ["date imported", "aspect ratio", "camera"]);
    }
}
