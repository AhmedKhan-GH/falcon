//! Marquee and keyboard selection in the grid, and the item to select after a removal.

use std::collections::HashSet;
use super::layout::Place;

/// What to select once `removed` leave the grid (`order`): the first
/// item after the first removed one that stays, else the nearest one
/// before it, as Eagle does after Remove from Folder.
pub fn next_after_removal(order: &[String], removed: &[String]) -> Option<String> {
    let first = order.iter().position(|id| removed.contains(id))?;
    let stays = |id: &&String| !removed.contains(id);
    order[first..].iter().find(stays).or_else(|| order[..first].iter().rev().find(stays)).cloned()
}

/// How a rubber-band drag combines with the selection it started from,
/// after Eagle's `rect-select`: plain replaces it, ⌘ flips every item the
/// band touches, ⇧ adds to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarqueeMode {
    Replace,
    Toggle,
    Add,
}

pub fn marquee_mode(command: bool, shift: bool) -> MarqueeMode {
    if command {
        MarqueeMode::Toggle
    } else if shift {
        MarqueeMode::Add
    } else {
        MarqueeMode::Replace
    }
}

/// The rectangle `x, y, w, h` spanned by two corners, either way round.
pub fn rect_between(a: [f32; 2], b: [f32; 2]) -> Place {
    [a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()]
}

/// Indices of the cards the band `rect` touches (an edge counts, as in
/// Eagle).
pub fn rect_hits(cards: &[Place], rect: Place) -> Vec<usize> {
    let [x, y, w, h] = rect;
    cards
        .iter()
        .enumerate()
        .filter(|(_, c)| !(c[1] + c[3] < y || c[1] > y + h || c[0] + c[2] < x || c[0] > x + w))
        .map(|(n, _)| n)
        .collect()
}

/// The selection while a band touches `hits` (positions in `order`),
/// starting from `base`, in grid order.
pub fn marquee_selection(order: &[String], base: &[String], hits: &[usize], mode: MarqueeMode) -> Vec<String> {
    let hit: HashSet<&str> = hits.iter().filter_map(|&n| order.get(n)).map(String::as_str).collect();
    let was: HashSet<&str> = base.iter().map(String::as_str).collect();
    order
        .iter()
        .filter(|id| {
            let (h, w) = (hit.contains(id.as_str()), was.contains(id.as_str()));
            match mode {
                MarqueeMode::Replace => h,
                MarqueeMode::Add => h || w,
                MarqueeMode::Toggle => h != w,
            }
        })
        .cloned()
        .collect()
}

/// How far to scroll this frame while a band is dragged with the pointer
/// at `y` in a view from `top` to `bottom`: within 24 px of an edge (or
/// past it) the view scrolls toward it, as Eagle's does.
pub fn edge_scroll(y: f32, top: f32, bottom: f32) -> f32 {
    const ZONE: f32 = 24.0;
    const STEP: f32 = 16.0;
    if y <= top + ZONE {
        -STEP
    } else if y >= bottom - ZONE {
        STEP
    } else {
        0.0
    }
}

/// ⇧→ (`forward`) or ⇧← on the selection, as Eagle's multiple select
/// does: the selected run grows by one item at its end away from the
/// anchor (the last item clicked or moved to), or shrinks back toward the
/// anchor when it already reaches past it on the other side. Returns the
/// new selection and the item to scroll to, or `None` when nothing is
/// selected or the run is at the end of the grid.
pub fn extend_selection(order: &[String], selected: &[String], anchor: Option<&str>, forward: bool) -> Option<(Vec<String>, String)> {
    let pos: Vec<usize> = selected.iter().filter_map(|id| order.iter().position(|x| x == id)).collect();
    let (start, end) = (*pos.iter().min()?, *pos.iter().max()?);
    let a = anchor.and_then(|a| order.iter().position(|x| x == a)).unwrap_or(if forward { start } else { end });
    let mut out = selected.to_vec();
    let (drop, add) = if forward { (start < a, end + 1) } else { (end > a, start.wrapping_sub(1)) };
    if drop {
        let gone = if forward { start } else { end };
        out.retain(|id| *id != order[gone]);
        let near = if forward { gone + 1 } else { gone - 1 };
        return Some((out, order[near].clone()));
    }
    let id = order.get(add)?.clone();
    out.push(id.clone());
    Some((out, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_after_removal() {
        let order: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(next_after_removal(&order, &ids(&["b"])).as_deref(), Some("c"));
        assert_eq!(next_after_removal(&order, &ids(&["b", "c"])).as_deref(), Some("d"));
        assert_eq!(next_after_removal(&order, &ids(&["c", "d"])).as_deref(), Some("b"));
        assert_eq!(next_after_removal(&order, &ids(&["a", "b", "c", "d"])), None);
        assert_eq!(next_after_removal(&order, &ids(&["x"])), None);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn band_hits_touching_cards() {
        let cards = [[0.0, 0.0, 10.0, 10.0], [20.0, 0.0, 10.0, 10.0], [0.0, 20.0, 10.0, 10.0]];
        assert_eq!(rect_hits(&cards, rect_between([25.0, 25.0], [5.0, 5.0])), [0, 1, 2]);
        assert_eq!(rect_hits(&cards, rect_between([12.0, 0.0], [18.0, 30.0])), Vec::<usize>::new(), "the gap between columns");
        assert_eq!(rect_hits(&cards, [10.0, 10.0, 0.0, 0.0]), [0], "an edge counts");
        assert_eq!(rect_between([5.0, 8.0], [1.0, 2.0]), [1.0, 2.0, 4.0, 6.0]);
    }

    #[test]
    fn band_modes_follow_eagle() {
        let order = ids(&["a", "b", "c", "d"]);
        let base = ids(&["a", "b"]);
        assert_eq!(marquee_mode(false, false), MarqueeMode::Replace);
        assert_eq!(marquee_mode(true, true), MarqueeMode::Toggle);
        assert_eq!(marquee_mode(false, true), MarqueeMode::Add);
        assert_eq!(marquee_selection(&order, &base, &[1, 2], MarqueeMode::Replace), ids(&["b", "c"]));
        assert_eq!(marquee_selection(&order, &base, &[1, 2], MarqueeMode::Add), ids(&["a", "b", "c"]));
        assert_eq!(marquee_selection(&order, &base, &[1, 2], MarqueeMode::Toggle), ids(&["a", "c"]));
        assert_eq!(marquee_selection(&order, &base, &[], MarqueeMode::Toggle), base, "shrinking the band gives items back");
        assert_eq!(marquee_selection(&order, &base, &[9], MarqueeMode::Replace), Vec::<String>::new());
    }

    #[test]
    fn edges_scroll() {
        assert_eq!(edge_scroll(105.0, 100.0, 500.0), -16.0);
        assert_eq!(edge_scroll(50.0, 100.0, 500.0), -16.0, "past the top");
        assert_eq!(edge_scroll(300.0, 100.0, 500.0), 0.0);
        assert_eq!(edge_scroll(490.0, 100.0, 500.0), 16.0);
    }

    #[test]
    fn shift_arrows_grow_and_shrink_the_run() {
        let order = ids(&["a", "b", "c", "d", "e"]);
        // From c: ⇧→ twice grows to e, then stops at the end.
        let (s1, to) = extend_selection(&order, &ids(&["c"]), Some("c"), true).unwrap();
        assert_eq!((s1.clone(), to.as_str()), (ids(&["c", "d"]), "d"));
        let (s2, _) = extend_selection(&order, &s1, Some("c"), true).unwrap();
        assert_eq!(s2, ids(&["c", "d", "e"]));
        assert_eq!(extend_selection(&order, &s2, Some("c"), true), None);
        // ⇧← shrinks back toward the anchor, then grows past it.
        let (s3, to) = extend_selection(&order, &s2, Some("c"), false).unwrap();
        assert_eq!((s3, to.as_str()), (ids(&["c", "d"]), "d"));
        let (s4, _) = extend_selection(&order, &ids(&["c"]), Some("c"), false).unwrap();
        assert_eq!(s4, ids(&["c", "b"]));
        let (s5, to) = extend_selection(&order, &s4, Some("c"), true).unwrap();
        assert_eq!((s5, to.as_str()), (ids(&["c"]), "c"), "⇧→ drops b first");
        assert_eq!(extend_selection(&order, &ids(&["a"]), Some("a"), false), None, "at the start");
        assert_eq!(extend_selection(&order, &[], None, true), None);
        // No anchor: the run grows away from its first (or last) item.
        assert_eq!(extend_selection(&order, &ids(&["b"]), None, true).unwrap().0, ids(&["b", "c"]));
    }
}
