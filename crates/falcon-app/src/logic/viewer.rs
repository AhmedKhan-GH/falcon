//! The detail viewer's shell: the span between the panels, the title bar inset,
//! the counter and stepping between items.

use crate::locale::tr;

/// Room the macOS window buttons need at the left of a bar that starts at
/// the window's left edge.
pub const TRAFFIC_LIGHTS: f32 = 72.0;

/// The left and right edges of the viewer, given the window's edges and
/// the widths of the sidebar and inspector when they are shown. As in
/// Eagle's detail mode, the viewer takes the space between the two panels
/// (all of it when both are hidden).
pub fn viewer_span(left: f32, right: f32, sidebar: Option<f32>, inspector: Option<f32>) -> (f32, f32) {
    let l = left + sidebar.unwrap_or(0.0);
    let r = (right - inspector.unwrap_or(0.0)).max(l);
    (l, r)
}

/// Where the first control of a bar starts, measured from the bar's left
/// edge: after the window buttons when nothing is to its left.
pub fn bar_inset(at_window_edge: bool) -> f32 {
    if at_window_edge { 20.0 + TRAFFIC_LIGHTS + 6.0 } else { 20.0 }
}

/// Eagle's toggle for both panels: shows both when the sidebar is hidden,
/// otherwise hides both. Takes and returns (sidebar shown, inspector
/// shown).
pub fn toggle_all(sidebar: bool, _inspector: bool) -> (bool, bool) {
    if sidebar { (false, false) } else { (true, true) }
}

/// The viewer bar's counter: the item's place in the grid order, from 1,
/// and the number of items.
pub fn viewer_counter(order: &[String], id: &str) -> Option<(usize, usize)> {
    order.iter().position(|x| x == id).map(|n| (n + 1, order.len()))
}

/// The end of the grid a viewer step ran into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    First,
    Last,
}

impl Edge {
    /// The note the viewer shows when a step can go no further.
    pub fn note(self) -> &'static str {
        match self {
            Edge::First => tr("This is the first item"),
            Edge::Last => tr("This is the last item"),
        }
    }
}

/// Where a viewer step of `step` from position `at` (none: the item is not
/// in the grid) lands among `len` items, or the end it ran into.
pub fn viewer_step(len: usize, at: Option<usize>, step: isize) -> Result<usize, Edge> {
    if len == 0 {
        return Err(if step < 0 { Edge::First } else { Edge::Last });
    }
    let Some(at) = at else { return Ok(0) };
    let to = at as isize + step;
    if to < 0 {
        Err(Edge::First)
    } else if to >= len as isize {
        Err(Edge::Last)
    } else {
        Ok(to as usize)
    }
}

#[cfg(test)]
mod viewer_shell_tests {
    use super::*;

    #[test]
    fn steps_stop_at_the_ends() {
        assert_eq!(viewer_step(3, Some(1), 1), Ok(2));
        assert_eq!(viewer_step(3, Some(1), -1), Ok(0));
        assert_eq!(viewer_step(3, Some(2), 1), Err(Edge::Last));
        assert_eq!(viewer_step(3, Some(0), -1), Err(Edge::First));
        assert_eq!(viewer_step(1, Some(0), 1), Err(Edge::Last));
        // An item that left the grid steps to the first one.
        assert_eq!(viewer_step(3, None, 1), Ok(0));
        assert_eq!(viewer_step(0, None, -1), Err(Edge::First));
        assert_ne!(Edge::First.note(), Edge::Last.note());
    }

    #[test]
    fn counter_counts_from_one() {
        let order: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(viewer_counter(&order, "a"), Some((1, 3)));
        assert_eq!(viewer_counter(&order, "c"), Some((3, 3)));
        assert_eq!(viewer_counter(&order, "z"), None);
        assert_eq!(viewer_counter(&[], "a"), None);
    }

    #[test]
    fn toggle_all_follows_the_sidebar() {
        assert_eq!(toggle_all(true, true), (false, false));
        assert_eq!(toggle_all(false, false), (true, true));
        // Mixed states go by the sidebar, as in Eagle.
        assert_eq!(toggle_all(true, false), (false, false));
        assert_eq!(toggle_all(false, true), (true, true));
        // Twice gets back to both shown or both hidden.
        let (s, i) = toggle_all(true, false);
        assert_eq!(toggle_all(s, i), (true, true));
    }

    #[test]
    fn viewer_sits_between_the_panels() {
        let (sw, iw) = (240.0, 260.0);
        assert_eq!(viewer_span(0.0, 1440.0, Some(sw), Some(iw)), (240.0, 1180.0));
        assert_eq!(viewer_span(0.0, 1440.0, None, Some(iw)), (0.0, 1180.0));
        assert_eq!(viewer_span(0.0, 1440.0, Some(sw), None), (240.0, 1440.0));
        assert_eq!(viewer_span(0.0, 1440.0, None, None), (0.0, 1440.0));
        // A window narrower than both panels leaves an empty span, never a
        // negative one.
        assert_eq!(viewer_span(0.0, 400.0, Some(sw), Some(iw)), (240.0, 240.0));
    }

    #[test]
    fn bars_leave_room_for_the_window_buttons() {
        assert_eq!(bar_inset(false), 20.0);
        assert_eq!(bar_inset(true), 98.0);
    }
}
