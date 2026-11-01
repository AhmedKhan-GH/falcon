//! The viewer's crop tool and rotate buttons. The rectangle math is plain
//! functions with tests; the drawing is an `impl App` block (this file is a
//! child module of `app`).

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};
use falcon_library::model::Item;
use falcon_library::transform::{self, PixelRect};

use super::image_edit::Op;
use super::{App, Action, icon_button};
use crate::icons as ic;
use crate::theme::{self as t, font, mono};

/// What a drag on the crop rectangle changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Move,
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    pub fn cursor(self) -> egui::CursorIcon {
        use egui::CursorIcon as C;
        match self {
            Handle::Move => C::Move,
            Handle::N | Handle::S => C::ResizeVertical,
            Handle::E | Handle::W => C::ResizeHorizontal,
            Handle::NE | Handle::SW => C::ResizeNeSw,
            Handle::NW | Handle::SE => C::ResizeNwSe,
        }
    }
}

/// Smallest crop side, in image pixels.
pub const MIN_SIDE: f32 = 1.0;

/// Maps a rectangle in image pixels to the screen, for an image of size
/// `natural` drawn into `shown`.
pub fn to_screen(r: Rect, shown: Rect, natural: Vec2) -> Rect {
    let s = shown.width() / natural.x.max(1.0);
    Rect::from_min_max(shown.min + r.min.to_vec2() * s, shown.min + r.max.to_vec2() * s)
}

/// Maps a screen point to image pixels (not clamped).
pub fn to_image(p: Pos2, shown: Rect, natural: Vec2) -> Pos2 {
    let s = shown.width().max(f32::EPSILON) / natural.x.max(1.0);
    Pos2::ZERO + (p - shown.min) / s
}

/// Which handle of the on-screen crop rectangle `r` is under `pos`: the
/// corners and edges within `tol` points, else the inside (move).
pub fn hit(r: Rect, pos: Pos2, tol: f32) -> Option<Handle> {
    if !r.expand(tol).contains(pos) {
        return None;
    }
    let near = |a: f32, b: f32| (a - b).abs() <= tol;
    let (l, rt, top, b) = (near(pos.x, r.left()), near(pos.x, r.right()), near(pos.y, r.top()), near(pos.y, r.bottom()));
    Some(match (l, rt, top, b) {
        (true, _, true, _) => Handle::NW,
        (_, true, true, _) => Handle::NE,
        (true, _, _, true) => Handle::SW,
        (_, true, _, true) => Handle::SE,
        (true, ..) => Handle::W,
        (_, true, ..) => Handle::E,
        (_, _, true, _) => Handle::N,
        (.., true) => Handle::S,
        _ if r.contains(pos) => Handle::Move,
        _ => return None,
    })
}

/// The crop rectangle (image pixels) after dragging `handle` by `delta`
/// image pixels from `start`. It stays inside `bounds` (the image size) and
/// at least [`MIN_SIDE`] on each side; a move keeps the size.
pub fn drag(start: Rect, handle: Handle, delta: Vec2, bounds: Vec2) -> Rect {
    if handle == Handle::Move {
        let dx = delta.x.clamp(-start.min.x, bounds.x - start.max.x);
        let dy = delta.y.clamp(-start.min.y, bounds.y - start.max.y);
        return start.translate(vec2(dx, dy));
    }
    let mut r = start;
    let (west, east, north, south) = match handle {
        Handle::N => (false, false, true, false),
        Handle::S => (false, false, false, true),
        Handle::E => (false, true, false, false),
        Handle::W => (true, false, false, false),
        Handle::NE => (false, true, true, false),
        Handle::NW => (true, false, true, false),
        Handle::SE => (false, true, false, true),
        Handle::SW => (true, false, false, true),
        Handle::Move => unreachable!(),
    };
    if west {
        r.min.x = (start.min.x + delta.x).clamp(0.0, start.max.x - MIN_SIDE);
    }
    if east {
        r.max.x = (start.max.x + delta.x).clamp(start.min.x + MIN_SIDE, bounds.x);
    }
    if north {
        r.min.y = (start.min.y + delta.y).clamp(0.0, start.max.y - MIN_SIDE);
    }
    if south {
        r.max.y = (start.max.y + delta.y).clamp(start.min.y + MIN_SIDE, bounds.y);
    }
    r
}

/// Whole pixels for the library: edges rounded to the nearest pixel.
pub fn pixel_rect(r: Rect) -> PixelRect {
    let x = r.min.x.round().max(0.0) as u32;
    let y = r.min.y.round().max(0.0) as u32;
    let right = r.max.x.round().max(0.0) as u32;
    let bottom = r.max.y.round().max(0.0) as u32;
    PixelRect { x, y, width: right.saturating_sub(x).max(1), height: bottom.saturating_sub(y).max(1) }
}

/// The crop tool's state while it is open.
#[derive(Debug, Clone)]
pub struct CropState {
    /// The rectangle in image pixels.
    pub rect: Rect,
    /// The drag in progress: handle and rectangle at its start.
    pub drag: Option<(Handle, Rect)>,
    /// Asking before the original is overwritten.
    pub confirming: bool,
}

impl CropState {
    pub fn new(w: f32, h: f32) -> CropState {
        CropState { rect: Rect::from_min_size(Pos2::ZERO, vec2(w, h)), drag: None, confirming: false }
    }
}

impl App {
    /// Draws the crop rectangle over the image at `shown` and handles its
    /// drags. Returns true while the tool is open, so the viewer doesn't pan.
    pub(super) fn crop_overlay(&mut self, ui: &mut Ui, item: &Item, shown: Rect, area: Rect) -> bool {
        let natural = vec2(item.width as f32, item.height as f32);
        let Some(crop) = self.ws.tools.crop.as_mut() else { return false };
        let p = ui.painter().clone().with_clip_rect(area);
        let screen = to_screen(crop.rect, shown, natural);
        // Dim what will be cut away.
        let dim = Color32::from_black_alpha(150);
        for r in [
            Rect::from_min_max(shown.min, Pos2::new(shown.right(), screen.top())),
            Rect::from_min_max(Pos2::new(shown.left(), screen.bottom()), shown.max),
            Rect::from_min_max(Pos2::new(shown.left(), screen.top()), Pos2::new(screen.left(), screen.bottom())),
            Rect::from_min_max(Pos2::new(screen.right(), screen.top()), Pos2::new(shown.right(), screen.bottom())),
        ] {
            p.rect_filled(r, 0.0, dim);
        }
        p.rect_stroke(screen, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
        for k in 1..3 {
            let f = k as f32 / 3.0;
            let x = screen.left() + screen.width() * f;
            let y = screen.top() + screen.height() * f;
            p.vline(x, screen.y_range(), Stroke::new(1.0, Color32::from_white_alpha(70)));
            p.hline(screen.x_range(), y, Stroke::new(1.0, Color32::from_white_alpha(70)));
        }
        for c in [screen.left_top(), screen.right_top(), screen.left_bottom(), screen.right_bottom()] {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(9.0)), 1.0, Color32::WHITE);
        }

        let resp = ui.interact(area, ui.scope_id().with("crop-canvas"), Sense::drag());
        let pos = ui.input(|i| i.pointer.interact_pos());
        if let Some(h) = pos.and_then(|p| hit(screen, p, 8.0)) {
            ui.ctx().set_cursor_icon(h.cursor());
        }
        if resp.drag_started()
            && let Some(h) = ui.input(|i| i.pointer.press_origin()).and_then(|p| hit(screen, p, 8.0))
        {
            crop.drag = Some((h, crop.rect));
        }
        if let Some((h, start)) = crop.drag {
            ui.ctx().set_cursor_icon(h.cursor());
            let total = resp.total_drag_delta().unwrap_or_default();
            let s = shown.width() / natural.x.max(1.0);
            crop.rect = drag(start, h, total / s.max(f32::EPSILON), natural);
        }
        if resp.drag_stopped() {
            crop.drag = None;
        }

        // Size readout under the rectangle.
        let px = pixel_rect(crop.rect);
        let label = format!("{} × {}", px.width, px.height);
        let at = Pos2::new(screen.center().x, (screen.bottom() + 16.0).min(area.bottom() - 12.0));
        let g = ui.painter().layout_no_wrap(label, mono(12.0), t::p().text);
        let bg = Rect::from_center_size(at, g.size() + vec2(12.0, 6.0));
        p.rect_filled(bg, CornerRadius::same(4), Color32::from_black_alpha(180));
        p.galley(bg.min + vec2(6.0, 3.0), g, t::p().text);
        true
    }

    /// The crop tool's controls in the viewer bar, right-aligned at `right`.
    pub(super) fn crop_bar(&mut self, ui: &mut Ui, item: &Item, right: f32, cy: f32) {
        let Some(crop) = self.ws.tools.crop.as_mut() else { return };
        let button = |ui: &mut Ui, x: f32, w: f32, label: &str, primary: bool| {
            let r = Rect::from_center_size(Pos2::new(x - w / 2.0, cy), vec2(w, 26.0));
            let b = egui::Button::new(egui::RichText::new(label).color(t::p().text)).fill(if primary { t::p().primary } else { t::p().button });
            ui.put(r, b)
        };
        if crop.confirming {
            if button(ui, right, 130.0, "Replace Original", true).clicked() {
                let rect = pixel_rect(crop.rect);
                self.ws.actions.push(Action::Edit(Op::Crop(item.id.clone(), rect)));
                self.ws.tools.crop = None;
                return;
            }
            if button(ui, right - 138.0, 70.0, "Back", false).clicked() {
                crop.confirming = false;
            }
            let g = ui.painter().layout_no_wrap("Cropping overwrites the original file and can't be undone".into(), font(12.0), t::p().text_muted);
            ui.painter().galley(Pos2::new(right - 216.0 - g.size().x, cy - g.size().y / 2.0), g, t::p().text_muted);
        } else {
            if button(ui, right, 70.0, "Save", true).on_hover_text("Save (Enter)").clicked() {
                crop.confirming = true;
            }
            if button(ui, right - 78.0, 70.0, "Cancel", false).on_hover_text("Cancel (Esc)").clicked() {
                self.ws.tools.crop = None;
            }
        }
    }

    /// Rotate and crop buttons for the viewer bar, laid out leftwards from
    /// `x`. Returns the x where the next control can go.
    pub(super) fn rotate_crop_buttons(&mut self, ui: &mut Ui, item: &Item, mut x: f32, cy: f32) -> f32 {
        if transform::can_crop(&item.ext) {
            let open = self.ws.tools.crop.is_some();
            if icon_button(ui, Pos2::new(x, cy), ic::CROP, 17.0, !open, "Crop").clicked() && !open {
                self.start_crop(item);
            }
            x -= 30.0;
        }
        if transform::can_rotate(&item.ext) {
            if icon_button(ui, Pos2::new(x, cy), ic::ARROW_CLOCKWISE, 17.0, true, "Rotate Right").clicked() {
                self.ws.actions.push(Action::Edit(Op::Rotate(vec![item.id.clone()], 1)));
            }
            x -= 30.0;
            if icon_button(ui, Pos2::new(x, cy), ic::ARROW_COUNTER_CLOCKWISE, 17.0, true, "Rotate Left (⇧R)").clicked() {
                self.ws.actions.push(Action::Edit(Op::Rotate(vec![item.id.clone()], -1)));
            }
            x -= 30.0;
        }
        x
    }

    /// Opens the crop tool on the whole image.
    pub(super) fn start_crop(&mut self, item: &Item) {
        if item.width > 0 && item.height > 0 && transform::can_crop(&item.ext) {
            self.ws.tools.comment_mode = false;
            self.ws.tools.crop = Some(CropState::new(item.width as f32, item.height as f32));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), vec2(w, h))
    }

    #[test]
    fn screen_mapping_round_trips() {
        let shown = r(100.0, 50.0, 400.0, 200.0);
        let natural = vec2(2000.0, 1000.0);
        let s = to_screen(r(500.0, 250.0, 1000.0, 500.0), shown, natural);
        assert_eq!(s, r(200.0, 100.0, 200.0, 100.0));
        assert_eq!(to_image(Pos2::new(200.0, 100.0), shown, natural), Pos2::new(500.0, 250.0));
    }

    #[test]
    fn handles_are_found_by_distance() {
        let s = r(100.0, 100.0, 200.0, 100.0);
        assert_eq!(hit(s, Pos2::new(101.0, 99.0), 8.0), Some(Handle::NW));
        assert_eq!(hit(s, Pos2::new(305.0, 205.0), 8.0), Some(Handle::SE));
        assert_eq!(hit(s, Pos2::new(200.0, 96.0), 8.0), Some(Handle::N));
        assert_eq!(hit(s, Pos2::new(296.0, 150.0), 8.0), Some(Handle::E));
        assert_eq!(hit(s, Pos2::new(200.0, 150.0), 8.0), Some(Handle::Move));
        assert_eq!(hit(s, Pos2::new(50.0, 150.0), 8.0), None);
    }

    #[test]
    fn drags_stay_inside_the_image() {
        let bounds = vec2(1000.0, 500.0);
        let start = r(100.0, 100.0, 300.0, 200.0);
        // Moving keeps the size and stops at the edges.
        assert_eq!(drag(start, Handle::Move, vec2(-500.0, 50.0), bounds), r(0.0, 150.0, 300.0, 200.0));
        assert_eq!(drag(start, Handle::Move, vec2(900.0, 900.0), bounds), r(700.0, 300.0, 300.0, 200.0));
        // Edges move alone and can't pass the opposite edge or the image.
        assert_eq!(drag(start, Handle::E, vec2(50.0, 30.0), bounds), r(100.0, 100.0, 350.0, 200.0));
        assert_eq!(drag(start, Handle::W, vec2(-400.0, 0.0), bounds), r(0.0, 100.0, 400.0, 200.0));
        assert_eq!(drag(start, Handle::W, vec2(1000.0, 0.0), bounds).width(), MIN_SIDE);
        assert_eq!(drag(start, Handle::SE, vec2(5000.0, 5000.0), bounds), r(100.0, 100.0, 900.0, 400.0));
        assert_eq!(drag(start, Handle::NW, vec2(-10.0, -10.0), bounds), r(90.0, 90.0, 310.0, 210.0));
    }

    #[test]
    fn pixel_rects_round_edges() {
        assert_eq!(pixel_rect(r(10.4, 0.6, 99.8, 50.0)), PixelRect { x: 10, y: 1, width: 100, height: 50 });
        assert_eq!(pixel_rect(r(5.0, 5.0, 0.2, 0.2)).width, 1);
    }
}
