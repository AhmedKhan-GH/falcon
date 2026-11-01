//! The All Tags page's sidebar, after Eagle's tag manager: which list is
//! open (All, Uncategorized, Starred or one tag group), where ↑ and ↓ go
//! in the "tags" focus zone, which tags each list shows, and what a tag
//! dropped on a sidebar row does. `ui/grid/tag_sidebar.rs` draws it.

use falcon_library::model::TagGroup;

/// The list the All Tags page shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TagView {
    /// Every tag.
    #[default]
    All,
    /// Tags in no tag group.
    Uncategorized,
    /// The starred tags.
    Starred,
    /// One tag group, by id.
    Group(String),
}

/// The list ↑ (or ↓ with `down`) opens from `view`, as Eagle walks its tag
/// sidebar: All, Uncategorized, Starred, then the groups in order; nothing
/// above All or below the last group. A group that is gone counts as All.
pub fn step(view: &TagView, groups: &[TagGroup], down: bool) -> Option<TagView> {
    let rows: Vec<TagView> =
        [TagView::All, TagView::Uncategorized, TagView::Starred].into_iter().chain(groups.iter().map(|g| TagView::Group(g.id.clone()))).collect();
    let at = rows.iter().position(|r| r == view).unwrap_or(0);
    let to = if down { at + 1 } else { at.checked_sub(1)? };
    rows.get(to).cloned()
}

/// The open list, or All when it names a group that no longer exists.
pub fn valid(view: &TagView, groups: &[TagGroup]) -> TagView {
    match view {
        TagView::Group(id) if !groups.iter().any(|g| &g.id == id) => TagView::All,
        v => v.clone(),
    }
}

/// Tags in use that no group holds.
pub fn uncategorized(counts: &[(String, usize)], groups: &[TagGroup]) -> Vec<(String, usize)> {
    counts.iter().filter(|(t, _)| !groups.iter().any(|g| g.tags.contains(t))).cloned().collect()
}

/// The starred tags with their counts (0 for one no item has), in the
/// order `counts` is sorted in; starred tags missing from `counts` follow
/// in starred order.
pub fn starred(counts: &[(String, usize)], starred: &[String]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = counts.iter().filter(|(t, _)| starred.contains(t)).cloned().collect();
    for s in starred {
        if !out.iter().any(|(t, _)| t == s) {
            out.push((s.clone(), 0));
        }
    }
    out
}

/// A group's tags with their counts, in the order `counts` is sorted in
/// (members no item has go last, in group order), as Eagle sorts a
/// group's list by the page's sort.
pub fn group_tags(counts: &[(String, usize)], group: &TagGroup) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = counts.iter().filter(|(t, _)| group.tags.contains(t)).cloned().collect();
    for t in &group.tags {
        if !out.iter().any(|(x, _)| x == t) {
            out.push((t.clone(), 0));
        }
    }
    out
}

/// What dropping dragged tags on a sidebar row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drop {
    /// Star them (the Starred row).
    Star(Vec<String>),
    /// File them in a group, leaving their other groups unless `copy`
    /// (⌥ held while dropping).
    Group { id: String, tags: Vec<String>, copy: bool },
}

/// The drop of `tags` on `target`, or `None` when it takes none: as in
/// Eagle, a group takes the drop only when some tag isn't in it yet, the
/// Starred row always does (starring again moves tags to the front), and
/// All and Uncategorized take no drops.
pub fn drop_on(target: &TagView, tags: &[String], groups: &[TagGroup], copy: bool) -> Option<Drop> {
    if tags.is_empty() {
        return None;
    }
    match target {
        TagView::Starred => Some(Drop::Star(tags.to_vec())),
        TagView::Group(id) => {
            let g = groups.iter().find(|g| &g.id == id)?;
            tags.iter().any(|t| !g.tags.contains(t)).then(|| Drop::Group { id: id.clone(), tags: tags.to_vec(), copy })
        }
        _ => None,
    }
}

/// The tags a drag carries: the picked tags when the dragged one is among
/// them, else just the dragged one (which then becomes the pick), as
/// Eagle's tag manager drags.
pub fn dragged(picked: &[String], tag: &str) -> Vec<String> {
    if picked.iter().any(|t| t == tag) { picked.to_vec() } else { vec![tag.to_string()] }
}

/// The tags a rubber band picks: every chip whose rectangle meets the
/// band, in drawing order, each once (Eagle's tag rectangle select). The
/// band replaces the pick.
pub fn banded(chips: &[(String, eframe::egui::Rect)], band: eframe::egui::Rect) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (t, r) in chips {
        if r.intersects(band) && !out.contains(t) {
            out.push(t.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, tags: &[&str]) -> TagGroup {
        serde_json::from_value(serde_json::json!({"id": id, "name": id, "tags": tags})).unwrap()
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn c(v: &[(&str, usize)]) -> Vec<(String, usize)> {
        v.iter().map(|(t, n)| (t.to_string(), *n)).collect()
    }

    #[test]
    fn arrows_walk_all_uncategorized_starred_then_groups() {
        let gs = [group("g1", &[]), group("g2", &[])];
        let g = |id: &str| TagView::Group(id.into());
        assert_eq!(step(&TagView::All, &gs, false), None);
        assert_eq!(step(&TagView::All, &gs, true), Some(TagView::Uncategorized));
        assert_eq!(step(&TagView::Uncategorized, &gs, true), Some(TagView::Starred));
        assert_eq!(step(&TagView::Starred, &gs, true), Some(g("g1")));
        assert_eq!(step(&g("g1"), &gs, true), Some(g("g2")));
        assert_eq!(step(&g("g2"), &gs, true), None);
        assert_eq!(step(&g("g1"), &gs, false), Some(TagView::Starred));
        assert_eq!(step(&TagView::Starred, &[], true), None);
        assert_eq!(step(&g("gone"), &gs, true), Some(TagView::Uncategorized));
        assert_eq!(valid(&g("gone"), &gs), TagView::All);
        assert_eq!(valid(&g("g2"), &gs), g("g2"));
    }

    #[test]
    fn a_band_picks_the_chips_it_touches() {
        use eframe::egui::{Rect, pos2};
        let chip = |t: &str, x: f32, y: f32| (t.to_string(), Rect::from_min_size(pos2(x, y), eframe::egui::vec2(50.0, 20.0)));
        let chips = [chip("a", 0.0, 0.0), chip("b", 60.0, 0.0), chip("c", 0.0, 30.0), chip("a", 60.0, 30.0)];
        assert_eq!(banded(&chips, Rect::from_two_pos(pos2(55.0, 10.0), pos2(45.0, 40.0))), s(&["a", "c"]));
        assert_eq!(banded(&chips, Rect::from_two_pos(pos2(45.0, 10.0), pos2(65.0, 40.0))), s(&["a", "b", "c"]));
        assert_eq!(banded(&chips, Rect::from_two_pos(pos2(52.0, 22.0), pos2(58.0, 28.0))), Vec::<String>::new());
        assert_eq!(banded(&chips, Rect::from_two_pos(pos2(70.0, 5.0), pos2(80.0, 10.0))), s(&["b"]));
    }

    #[test]
    fn lists_follow_groups_and_stars() {
        let counts = c(&[("b", 5), ("a", 3), ("c", 1)]);
        let gs = [group("g", &["c", "zero", "a"])];
        assert_eq!(uncategorized(&counts, &gs), c(&[("b", 5)]));
        assert_eq!(group_tags(&counts, &gs[0]), c(&[("a", 3), ("c", 1), ("zero", 0)]));
        assert_eq!(starred(&counts, &s(&["c", "none", "b"])), c(&[("b", 5), ("c", 1), ("none", 0)]));
    }

    #[test]
    fn drops_star_tags_or_file_them_when_a_group_lacks_one() {
        let gs = [group("g", &["a"])];
        let tags = s(&["a", "b"]);
        assert_eq!(drop_on(&TagView::Starred, &tags, &gs, false), Some(Drop::Star(tags.clone())));
        assert_eq!(drop_on(&TagView::Group("g".into()), &tags, &gs, true), Some(Drop::Group { id: "g".into(), tags: tags.clone(), copy: true }));
        assert_eq!(drop_on(&TagView::Group("g".into()), &s(&["a"]), &gs, false), None);
        assert_eq!(drop_on(&TagView::Group("x".into()), &tags, &gs, false), None);
        assert_eq!(drop_on(&TagView::All, &tags, &gs, false), None);
        assert_eq!(drop_on(&TagView::Uncategorized, &tags, &gs, false), None);
        assert_eq!(drop_on(&TagView::Starred, &[], &gs, false), None);
        assert_eq!(dragged(&tags, "b"), tags);
        assert_eq!(dragged(&tags, "c"), s(&["c"]));
    }
}
