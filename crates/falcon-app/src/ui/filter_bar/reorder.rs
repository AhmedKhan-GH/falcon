//! Dragging the filter bar's chips sideways to reorder them. A press on a
//! chip becomes a drag once the pointer has moved [`DRAG_START`] points,
//! so a plain click still opens the chip; on release the filter list is
//! stored in its new order (`SearchPrefs::move_kind`).

use eframe::egui::{CursorIcon, Pos2, Rect, Ui, vec2};

use crate::app::App;
use crate::search_state::FilterKind;
use crate::theme as t;

/// How far the pointer must move before a press on a chip is a drag.
const DRAG_START: f32 = 10.0;

/// The shown chip a drop at `x` goes before: the first whose centre is
/// right of `x`; `None` when `x` is past them all.
pub fn slot_at(chips: &[(FilterKind, Rect)], x: f32) -> Option<FilterKind> {
    chips.iter().find(|(_, r)| r.center().x > x).map(|(k, _)| *k)
}

/// What to pass to `move_kind` for a drop of `kind` into `slot` among the
/// `shown` chips. After the last shown chip means just before the filter
/// that follows it in the full `order` (the end if none does).
pub fn drop_before(order: &[FilterKind], shown: &[FilterKind], kind: FilterKind, slot: Option<FilterKind>) -> Option<FilterKind> {
    if slot.is_some() {
        return slot;
    }
    let last = shown.iter().rev().find(|k| **k != kind)?;
    let at = order.iter().position(|k| k == last)?;
    order[at + 1..].iter().copied().find(|k| *k != kind)
}

impl App {
    /// Tracks a press and drag on the chips drawn this frame, paints the
    /// insertion line and applies the drop.
    pub(crate) fn reorder_chips(&mut self, ui: &mut Ui, drawn: &[(FilterKind, Rect)]) {
        let ctx = ui.ctx().clone();
        let (pressed, down, released, origin, pos) =
            ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.press_origin(), i.pointer.latest_pos()));
        let fui = &mut self.ws.filter_ui;
        if pressed {
            fui.press = origin.and_then(|o| drawn.iter().find(|(_, r)| r.contains(o)).map(|(k, _)| (*k, o)));
            fui.dragging = None;
        }
        if let (Some((kind, o)), Some(p), true) = (fui.press, pos, down)
            && fui.dragging.is_none()
            && p.distance(o) > DRAG_START
        {
            fui.dragging = Some(kind);
        }
        if let Some(kind) = fui.dragging
            && let Some(p) = pos
        {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            let slot = slot_at(drawn, p.x);
            let x = match slot.and_then(|s| drawn.iter().find(|(k, _)| *k == s)) {
                Some((_, r)) => r.left() - 3.0,
                None => drawn.last().map_or(p.x, |(_, r)| r.right() + 3.0),
            };
            if let Some((_, r)) = drawn.first() {
                let line = Rect::from_center_size(Pos2::new(x, r.center().y), vec2(2.0, r.height()));
                ui.painter().rect_filled(line, 1.0, t::p().primary);
            }
            if released {
                let shown: Vec<FilterKind> = drawn.iter().map(|(k, _)| *k).collect();
                let before = drop_before(&self.search.ordered(), &shown, kind, slot);
                self.search.move_kind(kind, before);
            }
            ctx.request_repaint();
        }
        if !down || released {
            self.ws.filter_ui.press = None;
            self.ws.filter_ui.dragging = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chips(kinds: &[FilterKind]) -> Vec<(FilterKind, Rect)> {
        kinds.iter().enumerate().map(|(i, k)| (*k, Rect::from_min_size(Pos2::new(i as f32 * 100.0, 0.0), vec2(90.0, 26.0)))).collect()
    }

    #[test]
    fn the_slot_is_the_first_chip_right_of_the_pointer() {
        let c = chips(&[FilterKind::Color, FilterKind::Tags, FilterKind::Rating]);
        assert_eq!(slot_at(&c, -5.0), Some(FilterKind::Color));
        assert_eq!(slot_at(&c, 60.0), Some(FilterKind::Tags));
        assert_eq!(slot_at(&c, 160.0), Some(FilterKind::Rating));
        assert_eq!(slot_at(&c, 400.0), None);
    }

    #[test]
    fn dropping_after_the_last_chip_goes_before_the_next_filter_in_order() {
        let order = [FilterKind::Color, FilterKind::Tags, FilterKind::Size, FilterKind::Rating, FilterKind::Url];
        let shown = [FilterKind::Color, FilterKind::Tags, FilterKind::Rating];
        assert_eq!(drop_before(&order, &shown, FilterKind::Color, Some(FilterKind::Tags)), Some(FilterKind::Tags));
        assert_eq!(drop_before(&order, &shown, FilterKind::Color, None), Some(FilterKind::Url));
        assert_eq!(drop_before(&order, &shown, FilterKind::Rating, None), Some(FilterKind::Size));
        let last = [FilterKind::Color, FilterKind::Url];
        assert_eq!(drop_before(&order, &last, FilterKind::Color, None), None);
        assert_eq!(drop_before(&order, &[FilterKind::Color], FilterKind::Color, None), None);
    }
}
