//! Reordering items by hand: where a drag would drop in each layout.

use super::layout::{Layout, Place};

/// Width of the drop strip Eagle puts just outside each card's left and
/// right edges while items can be reordered by hand.
pub const SORT_STRIP: f32 = 16.0;

/// Height of the drop bar at the top or bottom of a list row.
pub const SORT_BAR: f32 = 4.0;

/// Where dragged items would land in a hand-sorted grid: before or after
/// the card `target`, and the bar to draw (`x, y, w, h` in content space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManualDrop {
    pub target: usize,
    pub after: bool,
    pub bar: [f32; 4],
}

/// Hit-tests a drag at `(x, y)` (content space) against the cards in
/// `places` from [`place`]. Cards for which `skip` is true (the dragged
/// ones) take no drops. In the card layouts, a card's box (image plus
/// `caption`) widened by [`SORT_STRIP`] on each side takes the drop: the
/// left half inserts before it, the right half after. Where the strips of
/// neighbours overlap, the card nearest the pointer wins, and on a tie the
/// later card. In the list layout the top half of a row inserts before it
/// and the bottom half after. The bar sits in the `gap` beside the card
/// (or across the row edge in the list).
pub fn manual_drop(layout: Layout, places: &[Place], caption: f32, gap: f32, skip: impl Fn(usize) -> bool, x: f32, y: f32) -> Option<ManualDrop> {
    if layout == Layout::List {
        let n = places.iter().enumerate().position(|(n, p)| !skip(n) && y >= p[1] && y < p[1] + p[3])?;
        let [px, py, w, h] = places[n];
        let after = y >= py + h / 2.0;
        let edge = if after { py + h } else { py };
        return Some(ManualDrop { target: n, after, bar: [px, edge - SORT_BAR / 2.0, w, SORT_BAR] });
    }
    let mut best: Option<(usize, f32)> = None;
    for (n, &[px, py, w, h]) in places.iter().enumerate() {
        if skip(n) || y < py || y >= py + h + caption || x < px - SORT_STRIP || x >= px + w + SORT_STRIP {
            continue;
        }
        let dist = if x < px { px - x } else if x > px + w { x - px - w } else { 0.0 };
        if best.is_none_or(|(_, d)| dist <= d) {
            best = Some((n, dist));
        }
    }
    let (n, _) = best?;
    let [px, py, w, h] = places[n];
    let after = x >= px + w / 2.0;
    let bw = SORT_STRIP.min(gap.max(SORT_BAR));
    let cx = if after { px + w + gap / 2.0 } else { px - gap / 2.0 };
    Some(ManualDrop { target: n, after, bar: [cx - bw / 2.0, py, bw, h + caption] })
}

#[cfg(test)]
mod manual_drop_tests {
    use super::*;
    use crate::logic::layout::{Layout, place};

    const GAP: f32 = 8.0;

    const CAP: f32 = 46.0;

    fn none(_: usize) -> bool {
        false
    }

    #[test]
    fn grid_halves_pick_before_or_after() {
        // Three 100 pt columns in 316 pt: cards at x = 0, 108, 216.
        let (p, _, cols) = place(Layout::Grid, &[1.0; 5], 316.0, 100.0, GAP, CAP, 10.0);
        assert_eq!(cols, 3);
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, none, 20.0, 50.0).unwrap();
        assert_eq!((d.target, d.after), (0, false));
        assert_eq!(d.bar, [-8.0, 0.0, 8.0, 146.0]);
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, none, 190.0, 120.0).unwrap();
        assert_eq!((d.target, d.after), (1, true), "the caption takes drops too");
        assert_eq!(d.bar[0], 208.0 + 4.0 - 4.0);
        // Second row: y starts at 156.
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, none, 120.0, 160.0).unwrap();
        assert_eq!((d.target, d.after), (4, false));
        // Below the last row, or in the row gap, nothing.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 50.0, 150.0), None);
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 50.0, 400.0), None);
    }

    #[test]
    fn grid_gap_goes_to_the_nearer_card() {
        let (p, _, _) = place(Layout::Grid, &[1.0; 3], 316.0, 100.0, GAP, CAP, 10.0);
        // The gap is 100..108; 101 is nearer card 0, 107 nearer card 1.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 101.0, 10.0).map(|d| (d.target, d.after)), Some((0, true)));
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 107.0, 10.0).map(|d| (d.target, d.after)), Some((1, false)));
        // Exactly halfway, the later card wins.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 104.0, 10.0).map(|d| (d.target, d.after)), Some((1, false)));
        // The strip reaches 16 pt past the outer edge.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, -15.0, 10.0).map(|d| d.target), Some(0));
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, -17.0, 10.0), None);
    }

    #[test]
    fn dragged_cards_take_no_drop() {
        let (p, _, _) = place(Layout::Grid, &[1.0; 3], 316.0, 100.0, GAP, CAP, 10.0);
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, |n| n == 1, 150.0, 10.0);
        assert_eq!(d, None, "over the dragged card itself");
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, |n| n == 1, 107.0, 10.0).unwrap();
        assert_eq!((d.target, d.after), (0, true), "its neighbour's strip still works");
    }

    #[test]
    fn waterfall_uses_each_cards_own_column_box() {
        // Two columns of 104 pt: a tall card, then two short ones.
        let (p, _, cols) = place(Layout::Waterfall, &[0.5, 2.0, 2.0], 216.0, 100.0, GAP, CAP, 10.0);
        assert_eq!(cols, 2);
        // Card 1 is in column 2 at the top; card 2 goes under it.
        assert_eq!(p[1][0], 112.0);
        assert_eq!(p[2][0], 112.0);
        let below = p[2][1] + 5.0;
        let d = manual_drop(Layout::Waterfall, &p, CAP, GAP, none, 200.0, below).unwrap();
        assert_eq!((d.target, d.after), (2, true));
        assert_eq!(d.bar[3], p[2][3] + CAP);
        // The tall card's lower part, beside card 2.
        let d = manual_drop(Layout::Waterfall, &p, CAP, GAP, none, 10.0, below).unwrap();
        assert_eq!((d.target, d.after), (0, false));
    }

    #[test]
    fn justified_rows_have_no_caption() {
        let (p, _, _) = place(Layout::Justified, &[1.5, 1.5, 1.0], 300.0, 100.0, GAP, 0.0, 10.0);
        let d = manual_drop(Layout::Justified, &p, 0.0, GAP, none, p[1][0] + 1.0, p[1][1] + 1.0).unwrap();
        assert_eq!((d.target, d.after), (1, false));
        assert_eq!(d.bar[3], p[1][3]);
        let last = p[2];
        let d = manual_drop(Layout::Justified, &p, 0.0, GAP, none, last[0] + last[2] + 3.0, last[1] + 1.0).unwrap();
        assert_eq!((d.target, d.after), (2, true));
    }

    #[test]
    fn list_rows_split_top_and_bottom() {
        let (p, _, _) = place(Layout::List, &[1.0; 3], 500.0, 100.0, GAP, 0.0, 10.0);
        let d = manual_drop(Layout::List, &p, 0.0, GAP, none, 300.0, 50.0).unwrap();
        assert_eq!((d.target, d.after), (1, false));
        assert_eq!(d.bar, [0.0, 42.0, 500.0, SORT_BAR]);
        let d = manual_drop(Layout::List, &p, 0.0, GAP, none, 300.0, 80.0).unwrap();
        assert_eq!((d.target, d.after), (1, true));
        assert_eq!(d.bar[1], 86.0);
        assert_eq!(manual_drop(Layout::List, &p, 0.0, GAP, none, 300.0, 200.0), None);
        assert_eq!(manual_drop(Layout::List, &p, 0.0, GAP, |n| n == 1, 300.0, 50.0), None);
    }

    #[test]
    fn zero_sized_items_get_placeholder_cards() {
        // A 0×0 item (a .md or .epub) has no aspect; the grid code passes 0
        // or NaN and `place` falls back to a square placeholder box.
        for layout in Layout::ALL {
            let caption = if layout.has_caption() { CAP } else { 0.0 };
            let (p, _, _) = place(layout, &[0.0, f32::NAN, 1.0], 316.0, 100.0, GAP, caption, 10.0);
            for (n, r) in p.iter().enumerate() {
                assert!(r.iter().all(|v| v.is_finite()) && r[2] > 0.0 && r[3] > 0.0, "{layout:?} card {n}: {r:?}");
                let d = manual_drop(layout, &p, caption, GAP, none, r[0] + 1.0, r[1] + 1.0).unwrap();
                assert_eq!((d.target, d.after), (n, false), "{layout:?}");
            }
        }
        // A degenerate box never panics and never divides by zero.
        let d = manual_drop(Layout::Grid, &[[0.0, 0.0, 0.0, 0.0]], 0.0, 0.0, none, 0.0, 0.0);
        assert_eq!(d, None);
        assert_eq!(manual_drop(Layout::Grid, &[], CAP, GAP, none, 0.0, 0.0), None);
    }
}
