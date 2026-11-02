//! UI-independent logic for organising: drop targets in the sidebar trees,
//! locked folders, a folder's own sort, tag group sections and saved
//! filters. The egui code in `ui/organize.rs` only draws and dispatches.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use falcon_library::edit::{FolderPlace, SubfolderSort};
use falcon_library::model::{Folder, FolderOrder, Item, TagGroup};
use falcon_library::search::{self, OrderBy};
use falcon_library::smart::SmartFolder;

use crate::locale::{tr, tr_key};

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

/// Expands or collapses every node with children, as Eagle's `*` and `/`
/// do: everything opens if the reference node is collapsed, else
/// everything closes. The reference is the open node's parent, or the
/// first top-level node.
pub fn toggle_all<T: Node>(roots: &[T], open: Option<&str>, collapsed: &mut HashSet<String>) {
    fn each<T: Node>(list: &[T], f: &mut impl FnMut(&str)) {
        for n in list {
            if !n.kids().is_empty() {
                f(n.id());
                each(n.kids(), f);
            }
        }
    }
    let parent = open.and_then(|id| locate(roots, id)).and_then(|(p, _)| p);
    let Some(reference) = parent.or(roots.first().map(Node::id)) else { return };
    let expand = collapsed.contains(reference);
    each(roots, &mut |id| {
        if expand {
            collapsed.remove(id);
        } else {
            collapsed.insert(id.to_string());
        }
    });
}

/// Sets whether `n` is open, leaving nodes without children out of the
/// set (a leaf has no open state).
fn set_open<T: Node>(n: &T, expand: bool, collapsed: &mut HashSet<String>) {
    if n.kids().is_empty() {
        return;
    }
    if expand {
        collapsed.remove(n.id());
    } else {
        collapsed.insert(n.id().to_string());
    }
}

/// Flips one node, then gives each of its direct children the same state,
/// as Eagle's expand or collapse of a folder does: everything opens if
/// the node is collapsed, else closes.
pub fn toggle_with_children<T: Node>(roots: &[T], id: &str, collapsed: &mut HashSet<String>) {
    let Some(n) = find(roots, id) else { return };
    let expand = collapsed.contains(id);
    set_open(n, expand, collapsed);
    for k in n.kids() {
        set_open(k, expand, collapsed);
    }
}

/// Gives every sibling of a node (the node included, at its parent's level
/// or the top) the state the node is not in now, as Eagle's same-level
/// expand or collapse does.
pub fn toggle_level<T: Node>(roots: &[T], id: &str, collapsed: &mut HashSet<String>) {
    let Some((parent, _)) = locate(roots, id) else { return };
    let level = match parent.and_then(|p| find(roots, p)) {
        Some(p) => p.kids(),
        None => roots,
    };
    let expand = collapsed.contains(id);
    for n in level {
        set_open(n, expand, collapsed);
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

/// Whether a drop of `dragged` on `target` is refused: the target is one of
/// them or lies inside one, as Eagle's `moveFoldersToFolder` refuses.
pub fn drop_refused<T: Node>(roots: &[T], dragged: &[String], target: &str) -> bool {
    dragged.iter().any(|d| find(roots, d).is_some_and(|n| within(n, target)))
}

/// What dropping dragged folders on a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderDrop {
    /// One folder: its new parent and index, as [`tree_drop`] gives them.
    One(String, Option<String>, usize),
    /// Several: all of them next to or into the target in one step.
    Many(Vec<String>, String, FolderPlace),
}

/// The move a drop of folders `dragged` on `target` asks for. `None` when it
/// is refused or, for one folder, changes nothing.
pub fn folder_drop<T: Node>(roots: &[T], dragged: &[String], target: &str, zone: DropZone) -> Option<FolderDrop> {
    if let [one] = dragged {
        let (parent, index) = tree_drop(roots, one, target, zone)?;
        return Some(FolderDrop::One(one.clone(), parent, index));
    }
    if dragged.is_empty() || drop_refused(roots, dragged, target) {
        return None;
    }
    let place = match zone {
        DropZone::Before => FolderPlace::Above,
        DropZone::Into => FolderPlace::Into,
        DropZone::After => FolderPlace::Below,
    };
    Some(FolderDrop::Many(dragged.to_vec(), target.to_string(), place))
}

/// Eagle's Edit ▸ Move Folder commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrange {
    Up,
    Down,
    Top,
    Bottom,
}

/// The move that shifts `id` among its siblings, in the form `tree_drop`
/// returns (parent, index counted after the node leaves). `None` when it
/// is already at that end or not in the tree.
pub fn arrange<T: Node>(roots: &[T], id: &str, how: Arrange) -> Option<(Option<String>, usize)> {
    let (parent, at) = locate(roots, id)?;
    let len = match parent {
        None => roots.len(),
        Some(p) => find(roots, p)?.kids().len(),
    };
    let to = match how {
        Arrange::Up => at.checked_sub(1)?,
        Arrange::Down => at + 1,
        Arrange::Top => 0,
        Arrange::Bottom => len - 1,
    };
    (to != at && to < len).then(|| (parent.map(str::to_string), to))
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

/// Applies a Touch ID answer to the folder it was asked for: on success
/// only that folder joins `unlocked`. Returns whether it unlocked.
pub fn touch_unlock(folder: &str, ok: bool, unlocked: &mut HashSet<String>) -> bool {
    if ok {
        unlocked.insert(folder.to_string());
    }
    ok
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
        return Err(if remove { tr("The password is wrong.") } else { tr("The old password is wrong.") });
    }
    if remove {
        return Ok(());
    }
    if new.is_empty() {
        return Err(tr("Enter a password."));
    }
    if new.chars().any(|c| c as u32 > 0xff) {
        return Err(tr("Use only Latin letters, digits and symbols."));
    }
    if new != again {
        return Err(tr("The passwords don't match."));
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
        FolderOrder::By(k) => tr(k.label()),
        FolderOrder::Duration => tr("Duration"),
        FolderOrder::Random => tr("Random"),
        FolderOrder::Manual => tr("Manual"),
    }
}

/// The two direction labels for a key (time keys read newest/oldest).
pub fn direction_labels(o: FolderOrder) -> [&'static str; 2] {
    match o {
        FolderOrder::By(k) if k.is_time() => [tr("Newest First"), tr("Oldest First")],
        _ => [tr("Ascending"), tr("Descending")],
    }
}

/// A folder's or smart folder's stored sort, if it has one falcon knows.
/// `sortIncrease` defaults to `true`, as Eagle sets it with the key.
pub fn own_order(order_by: Option<&str>, increase: Option<bool>) -> Option<(FolderOrder, bool)> {
    order_by.and_then(FolderOrder::parse).map(|o| (o, increase.unwrap_or(true)))
}

/// The key `logic::visible` should sort by in a view with its own sort:
/// keys the main list knows (duration included) pass through; random and manual
/// start from import order and are finished by [`finish_order`].
pub fn base_key(own: Option<(FolderOrder, bool)>, global: (OrderBy, bool)) -> (OrderBy, bool) {
    match own {
        Some((FolderOrder::By(k), inc)) => (k, inc),
        Some((FolderOrder::Duration, inc)) => (OrderBy::Duration, inc),
        Some(_) => (OrderBy::Import, true),
        None => global,
    }
}

/// Applies the keys the main list can't sort by. Random shuffles by
/// `seed` (fixed for one visit). Manual sorts by each item's
/// hand-arranged position in `folder` ([`search::sort_manual`]); outside a
/// folder it keeps the incoming order.
pub fn finish_order(order: &mut [usize], items: &[Item], own: Option<(FolderOrder, bool)>, folder: Option<&str>, seed: u64) {
    match own {
        Some((FolderOrder::Random, _)) => order.sort_by_key(|&i| shuffle_key(seed, &items[i].id)),
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
// Sorting folders by title

/// The folder menu's Sort By entries, in Eagle's order: this level A to Z
/// and Z to A, then every level A to Z and Z to A.
pub const FOLDER_TITLE_SORTS: [(&str, SubfolderSort); 4] = [
    (tr_key("Title, This Level (A→Z)"), SubfolderSort { descending: false, all_levels: false }),
    (tr_key("Title, This Level (Z→A)"), SubfolderSort { descending: true, all_levels: false }),
    (tr_key("Title, All Levels (A→Z)"), SubfolderSort { descending: false, all_levels: true }),
    (tr_key("Title, All Levels (Z→A)"), SubfolderSort { descending: true, all_levels: true }),
];

/// The list [`falcon_library::Library::sort_subfolders`] should sort for
/// a Sort By entry picked on a folder whose parent is `parent` (`None`:
/// the top level). As in Eagle, "this level" is the folder's own level
/// (it and its siblings), and "all levels" is the whole tree from the top.
pub fn title_sort_parent(parent: Option<&str>, sort: SubfolderSort) -> Option<&str> {
    if sort.all_levels { None } else { parent }
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
    /// The group's description, empty when it has none.
    pub description: String,
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
            let description = g.description.clone().unwrap_or_default();
            out.push(TagSection { id: g.id.clone(), name: g.name.clone(), color: g.color.clone(), description, tags });
        }
    }
    let rest = counts.iter().filter(|(t, _)| !grouped.contains(t) && matches(t)).cloned().collect();
    (out, rest)
}

/// The index to pass to `Library::reorder_tag_group` when the group
/// `dragged` is dropped before (or `after`) the group `target`: the place
/// among the other groups once `dragged` has left its own. `None` when the
/// drop changes nothing or names an unknown group.
pub fn group_drop(groups: &[TagGroup], dragged: &str, target: &str, after: bool) -> Option<usize> {
    let from = groups.iter().position(|g| g.id == dragged)?;
    let to = groups.iter().position(|g| g.id == target)?;
    let dest = if after { to + 1 } else { to };
    let index = if from < dest { dest - 1 } else { dest };
    (index != from).then_some(index)
}

/// A tag group description as Eagle keeps it: one line (line breaks
/// become spaces) without surrounding spaces.
pub fn clean_description(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ").trim().to_string()
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
            "image" => tr("image search"),
            "semantic" => tr("semantic search"),
            _ => tr("other rules"),
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Adds the tags typed into the auto-tag field to `tags`: the text is split
/// at commas and line breaks, each part trimmed, and empty parts and tags
/// already listed are skipped. Returns whether anything was added.
pub fn add_typed_tags(tags: &mut Vec<String>, typed: &str) -> bool {
    let parts: Vec<String> = typed.split([',', '\n', '\r']).map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect();
    falcon_library::autotag::append_new(tags, &parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dropping_picked_folders_moves_them_together() {
        let roots = vec![folder("a", vec![folder("a1", vec![folder("a1x", vec![])])]), folder("b", vec![]), folder("c", vec![])];
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let picked = ids(&["a", "b"]);
        assert_eq!(folder_drop(&roots, &picked, "c", DropZone::Into), Some(FolderDrop::Many(picked.clone(), "c".into(), FolderPlace::Into)));
        assert_eq!(folder_drop(&roots, &picked, "c", DropZone::Before), Some(FolderDrop::Many(picked.clone(), "c".into(), FolderPlace::Above)));
        assert_eq!(folder_drop(&roots, &picked, "c", DropZone::After), Some(FolderDrop::Many(picked.clone(), "c".into(), FolderPlace::Below)));
        // Refused on a picked folder and anywhere inside one, in every zone.
        for zone in [DropZone::Before, DropZone::Into, DropZone::After] {
            for target in ["a", "b", "a1", "a1x"] {
                assert_eq!(folder_drop(&roots, &picked, target, zone), None, "{target}");
            }
        }
        assert!(drop_refused(&roots, &picked, "a1x"));
        assert!(!drop_refused(&roots, &picked, "c"));
        // One folder keeps the exact index a single reorder gets.
        assert_eq!(folder_drop(&roots, &ids(&["c"]), "a", DropZone::Before), Some(FolderDrop::One("c".into(), None, 0)));
        assert_eq!(folder_drop(&roots, &ids(&["a"]), "a1x", DropZone::Into), None);
        assert_eq!(folder_drop::<Folder>(&roots, &[], "c", DropZone::Into), None);
    }

    #[test]
    fn touch_id_unlocks_only_the_asked_folder() {
        let mut open = HashSet::new();
        assert!(!touch_unlock("a", false, &mut open));
        assert!(open.is_empty());
        assert!(touch_unlock("a", true, &mut open));
        assert_eq!(open, HashSet::from(["a".to_string()]));
    }

    #[test]
    fn toggle_with_children_sets_the_node_and_its_direct_children() {
        let tree = vec![folder("a", vec![folder("a1", vec![folder("a1x", vec![])]), folder("a2", vec![]), folder("a3", vec![folder("a3x", vec![])])]), folder("b", vec![folder("b1", vec![])])];
        let mut collapsed = HashSet::new();
        // Open, so a and its children with kids close; a2 is a leaf and stays out.
        toggle_with_children(&tree, "a", &mut collapsed);
        assert_eq!(collapsed, ["a", "a1", "a3"].map(String::from).into());
        // Now collapsed, so they open again.
        toggle_with_children(&tree, "a", &mut collapsed);
        assert!(collapsed.is_empty());
        // Grandchildren are untouched, and a leaf or unknown id does nothing.
        collapsed.insert("a1x".into());
        toggle_with_children(&tree, "a1x", &mut collapsed);
        toggle_with_children(&tree, "nope", &mut collapsed);
        assert_eq!(collapsed, ["a1x"].map(String::from).into());
    }

    #[test]
    fn toggle_level_sets_every_sibling() {
        let tree = vec![folder("a", vec![folder("a1", vec![folder("a1x", vec![])]), folder("a2", vec![]), folder("a3", vec![folder("a3x", vec![])])]), folder("b", vec![folder("b1", vec![])]), folder("c", vec![])];
        let mut collapsed = HashSet::new();
        // Top level: a and b close, c is a leaf; deeper nodes stay open.
        toggle_level(&tree, "b", &mut collapsed);
        assert_eq!(collapsed, ["a", "b"].map(String::from).into());
        // b is collapsed, so the whole level opens.
        toggle_level(&tree, "a", &mut collapsed);
        assert!(collapsed.is_empty());
        // Inside a: the clicked node decides, for a's children only.
        toggle_level(&tree, "a3", &mut collapsed);
        assert_eq!(collapsed, ["a1", "a3"].map(String::from).into());
        toggle_level(&tree, "a1", &mut collapsed);
        assert!(collapsed.is_empty());
    }

    #[test]
    fn toggle_all_follows_the_reference_node() {
        let tree = vec![folder("a", vec![folder("a1", vec![folder("a1x", vec![])])]), folder("b", vec![folder("b1", vec![])]), folder("c", vec![])];
        let mut collapsed = HashSet::new();
        // Everything is open, so everything with children closes.
        toggle_all(&tree, None, &mut collapsed);
        assert_eq!(collapsed, ["a", "a1", "b"].map(String::from).into());
        // Now the first is closed, so everything opens.
        toggle_all(&tree, None, &mut collapsed);
        assert!(collapsed.is_empty());
        // With b1 open, its parent b decides: b is closed, so all open.
        collapsed.insert("b".into());
        toggle_all(&tree, Some("b1"), &mut collapsed);
        assert!(collapsed.is_empty());
        toggle_all(&[] as &[Folder], None, &mut collapsed);
        assert!(collapsed.is_empty());
    }

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
    fn arrange_moves_among_siblings() {
        let t = vec![folder("A", vec![]), folder("B", vec![folder("B1", vec![]), folder("B2", vec![]), folder("B3", vec![])]), folder("C", vec![])];
        assert_eq!(arrange(&t, "B", Arrange::Up), Some((None, 0)));
        assert_eq!(arrange(&t, "B", Arrange::Down), Some((None, 2)));
        assert_eq!(arrange(&t, "A", Arrange::Up), None);
        assert_eq!(arrange(&t, "C", Arrange::Down), None);
        assert_eq!(arrange(&t, "C", Arrange::Top), Some((None, 0)));
        assert_eq!(arrange(&t, "A", Arrange::Top), None);
        assert_eq!(arrange(&t, "A", Arrange::Bottom), Some((None, 2)));
        assert_eq!(arrange(&t, "C", Arrange::Bottom), None);
        // Subfolders stay inside their parent.
        assert_eq!(arrange(&t, "B2", Arrange::Up), Some((Some("B".into()), 0)));
        assert_eq!(arrange(&t, "B2", Arrange::Down), Some((Some("B".into()), 2)));
        assert_eq!(arrange(&t, "B1", Arrange::Bottom), Some((Some("B".into()), 2)));
        assert_eq!(arrange(&t, "B3", Arrange::Top), Some((Some("B".into()), 0)));
        assert_eq!(arrange(&t, "Z", Arrange::Up), None);
        let lone = vec![folder("A", vec![])];
        for how in [Arrange::Up, Arrange::Down, Arrange::Top, Arrange::Bottom] {
            assert_eq!(arrange(&lone, "A", how), None);
        }
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
        // Duration is the main sort's key (search::sort); finishing leaves it.
        assert_eq!(base_key(Some((FolderOrder::Duration, false)), (OrderBy::Name, true)), (OrderBy::Duration, false));
        let mut o = vec![0, 1, 2, 3];
        finish_order(&mut o, &items, Some((FolderOrder::Duration, true)), None, 0);
        assert_eq!(o, [0, 1, 2, 3]);
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
    fn tag_groups_reorder_by_drop() {
        let groups = vec![group("a", "A", &[]), group("b", "B", &[]), group("c", "C", &[]), group("d", "D", &[])];
        assert_eq!(group_drop(&groups, "a", "c", false), Some(1), "a lands between b and c");
        assert_eq!(group_drop(&groups, "a", "c", true), Some(2));
        assert_eq!(group_drop(&groups, "d", "a", false), Some(0));
        assert_eq!(group_drop(&groups, "c", "a", true), Some(1));
        assert_eq!(group_drop(&groups, "a", "d", true), Some(3));
        assert_eq!(group_drop(&groups, "b", "c", false), None, "already there");
        assert_eq!(group_drop(&groups, "b", "a", true), None);
        assert_eq!(group_drop(&groups, "b", "b", true), None);
        assert_eq!(group_drop(&groups, "x", "a", true), None);
        // Each index is what reorder_tag_group expects: remove, then insert.
        for (d, t, after) in [("a", "c", false), ("d", "b", true), ("b", "d", false)] {
            let at = group_drop(&groups, d, t, after).unwrap();
            let mut ids: Vec<&str> = groups.iter().map(|g| g.id.as_str()).filter(|id| *id != d).collect();
            ids.insert(at, d);
            let pos = |x: &str| ids.iter().position(|i| *i == x).unwrap();
            if after {
                assert_eq!(pos(d), pos(t) + 1, "{d} after {t}");
            } else {
                assert_eq!(pos(d) + 1, pos(t), "{d} before {t}");
            }
        }
        assert_eq!(clean_description("  Line one\nline two\r\nthree "), "Line one line two three");
        assert_eq!(clean_description("\n"), "");
    }

    #[test]
    fn title_sorts_follow_eagles_menu() {
        let s = FOLDER_TITLE_SORTS.map(|(_, s)| (s.all_levels, s.descending));
        assert_eq!(s, [(false, false), (false, true), (true, false), (true, true)]);
        let this = FOLDER_TITLE_SORTS[1].1;
        let all = FOLDER_TITLE_SORTS[2].1;
        assert_eq!(title_sort_parent(Some("p"), this), Some("p"), "this level is the folder's siblings");
        assert_eq!(title_sort_parent(None, this), None);
        assert_eq!(title_sort_parent(Some("p"), all), None, "all levels start at the top");
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
        let keys: Vec<String> = ["semantic", "image", "semantic", "bogus"].iter().map(|s| s.to_string()).collect();
        assert_eq!(unsupported_names(&keys), ["semantic search", "image search", "other rules"]);
    }

    #[test]
    fn typed_tags_split_at_commas() {
        let mut tags = vec!["a".to_string()];
        assert!(add_typed_tags(&mut tags, " b, a ,,c\nd "));
        assert_eq!(tags, ["a", "b", "c", "d"]);
        assert!(!add_typed_tags(&mut tags, " , b"));
    }
}
