//! Duplicate groups: building, pruning and editing the groups the page lists.

/// One group of the duplicates page.
#[derive(Debug, Clone, PartialEq)]
pub struct DupGroup {
    /// Byte-identical originals (true) or similar pictures (false).
    pub exact: bool,
    pub ids: Vec<String>,
    /// Bits each member's perceptual hash differs from the first member's
    /// (0 for exact groups and for members without a hash).
    pub distances: Vec<u32>,
    /// The member a merge keeps: the first, until the user picks another.
    pub keep: String,
    /// Ticked for merging (every group starts ticked, as in Eagle).
    pub picked: bool,
}

/// A group of `ids` (two or more, the best copy first), with each
/// member's distance from the first.
pub fn dup_group(exact: bool, ids: Vec<String>, hash: impl Fn(&str) -> Option<u64>) -> DupGroup {
    let first = ids.first().and_then(|id| hash(id));
    let distances = ids
        .iter()
        .map(|id| match (first, hash(id)) {
            (Some(a), Some(b)) if !exact => (a ^ b).count_ones(),
            _ => 0,
        })
        .collect();
    DupGroup { exact, keep: ids.first().cloned().unwrap_or_default(), ids, distances, picked: true }
}

/// The groups with only the members `alive` keeps; a group left with one
/// member drops out, and one whose kept member went keeps its first.
pub fn live_groups(groups: &[DupGroup], alive: impl Fn(&str) -> bool) -> Vec<DupGroup> {
    groups
        .iter()
        .filter_map(|g| {
            let keep: Vec<usize> = (0..g.ids.len()).filter(|&n| alive(&g.ids[n])).collect();
            (keep.len() > 1).then(|| {
                let ids: Vec<String> = keep.iter().map(|&n| g.ids[n].clone()).collect();
                let kept = if ids.contains(&g.keep) { g.keep.clone() } else { ids[0].clone() };
                DupGroup { exact: g.exact, distances: keep.iter().map(|&n| g.distances[n]).collect(), ids, keep: kept, picked: g.picked }
            })
        })
        .collect()
}

/// Takes `id` out of group `at` (Eagle's Remove This Item): a group left
/// with one member leaves the list, and a removed kept member hands the
/// part to the first one left.
pub fn remove_member(groups: &mut Vec<DupGroup>, at: usize, id: &str) {
    let Some(g) = groups.get_mut(at) else { return };
    if let Some(n) = g.ids.iter().position(|x| x == id) {
        g.ids.remove(n);
        g.distances.remove(n);
    }
    if g.ids.len() < 2 {
        groups.remove(at);
    } else if g.keep == id {
        g.keep = g.ids[0].clone();
    }
}

#[cfg(test)]
mod duplicate_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn duplicate_groups() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let hashes: HashMap<&str, u64> = [("a", 0b0000), ("b", 0b0000), ("c", 0b0111), ("d", 0b0001)].into();
        let h = |id: &str| hashes.get(id).copied();
        let exact = dup_group(true, s(&["c", "d"]), h);
        assert_eq!((exact.distances.clone(), exact.keep.as_str(), exact.picked), (vec![0, 0], "c", true));
        let near = dup_group(false, s(&["c", "d", "a"]), h);
        assert_eq!(near.distances, vec![0, 2, 3]);
        let mut g = vec![dup_group(true, s(&["a", "b"]), h), near];
        g[1].keep = "d".into();
        g[1].picked = false;
        // Trashed members leave; a group of one disappears; the pick stays.
        let live = live_groups(&g, |id| id != "b");
        assert_eq!(live.len(), 1);
        assert_eq!((live[0].ids.clone(), live[0].keep.as_str(), live[0].picked), (s(&["c", "d", "a"]), "d", false));
        // A kept member that goes hands the part to the first left.
        let live = live_groups(&g, |id| id != "d");
        assert_eq!(live[1].keep, "c");
        // Removing members from the results.
        remove_member(&mut g, 1, "d");
        assert_eq!((g[1].ids.clone(), g[1].distances.clone(), g[1].keep.as_str()), (s(&["c", "a"]), vec![0, 3], "c"));
        remove_member(&mut g, 0, "a");
        assert_eq!(g.len(), 1, "a group of one leaves the list");
        assert_eq!(g[0].ids, s(&["c", "a"]));
    }
}
