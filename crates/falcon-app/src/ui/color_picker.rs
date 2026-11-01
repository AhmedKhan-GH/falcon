//! A flat colour picker: a saturation and brightness square for the
//! current hue, with a vertical hue strip beside it.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2, pos2, vec2};

use crate::logic::{hsv_to_rgb, rgb_to_hsv};
use crate::theme as t;

const SQUARE_W: f32 = 180.0;
const HEIGHT: f32 = 130.0;
const GAP: f32 = 8.0;
const STRIP_W: f32 = 16.0;
const SQUARE_CELLS: usize = 16;
const STRIP_SEGMENTS: usize = 12;
const RING_RADIUS: f32 = 5.0;

fn c32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Draws the picker for `rgb` and returns true when the user changed it this
/// frame. The hue is kept in egui memory under `id`, so dragging to black or
/// grey does not lose it; it is only re-read from `rgb` when that changed
/// from outside.
pub(crate) fn sv_hue_picker(ui: &mut egui::Ui, id: egui::Id, rgb: &mut [u8; 3]) -> bool {
    let mut hsv = ui.data_mut(|d| d.get_temp::<[f32; 3]>(id)).filter(|h| hsv_to_rgb(*h) == *rgb).unwrap_or_else(|| rgb_to_hsv(*rgb));
    let (rect, _) = ui.allocate_exact_size(vec2(SQUARE_W + GAP + STRIP_W, HEIGHT), Sense::hover());
    let sq = Rect::from_min_size(rect.min, vec2(SQUARE_W, HEIGHT));
    let strip = Rect::from_min_size(pos2(sq.right() + GAP, rect.top()), vec2(STRIP_W, HEIGHT));
    let sq_resp = ui.interact(sq, id.with("sv"), Sense::click_and_drag());
    let strip_resp = ui.interact(strip, id.with("hue"), Sense::click_and_drag());

    let mut changed = false;
    if let Some(p) = sq_resp.interact_pointer_pos().filter(|_| sq_resp.is_pointer_button_down_on() || sq_resp.clicked()) {
        hsv[1] = ((p.x - sq.left()) / sq.width()).clamp(0.0, 1.0);
        hsv[2] = 1.0 - ((p.y - sq.top()) / sq.height()).clamp(0.0, 1.0);
        changed = true;
    }
    if let Some(p) = strip_resp.interact_pointer_pos().filter(|_| strip_resp.is_pointer_button_down_on() || strip_resp.clicked()) {
        hsv[0] = ((p.y - strip.top()) / strip.height()).clamp(0.0, 1.0) * 360.0;
        changed = true;
    }
    if changed {
        *rgb = hsv_to_rgb(hsv);
    }
    ui.data_mut(|d| d.insert_temp(id, hsv));

    let painter = ui.painter();
    let mut mesh = Mesh::default();
    let n = SQUARE_CELLS;
    for j in 0..=n {
        for i in 0..=n {
            let (s, v) = (i as f32 / n as f32, 1.0 - j as f32 / n as f32);
            let pos = pos2(sq.left() + s * sq.width(), sq.top() + (1.0 - v) * sq.height());
            mesh.colored_vertex(pos, c32(hsv_to_rgb([hsv[0], s, v])));
        }
    }
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = j * (n as u32 + 1) + i;
            let b = a + 1;
            let c = a + n as u32 + 1;
            mesh.add_triangle(a, b, c);
            mesh.add_triangle(b, c + 1, c);
        }
    }
    painter.add(Shape::mesh(mesh));

    let mut hue_mesh = Mesh::default();
    for k in 0..=STRIP_SEGMENTS {
        let f = k as f32 / STRIP_SEGMENTS as f32;
        let y = strip.top() + f * strip.height();
        let col = c32(hsv_to_rgb([f * 360.0, 1.0, 1.0]));
        hue_mesh.colored_vertex(pos2(strip.left(), y), col);
        hue_mesh.colored_vertex(pos2(strip.right(), y), col);
    }
    for k in 0..STRIP_SEGMENTS as u32 {
        let a = k * 2;
        hue_mesh.add_triangle(a, a + 1, a + 2);
        hue_mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    painter.add(Shape::mesh(hue_mesh));

    let border = Stroke::new(1.0, t::p().divider);
    painter.rect_stroke(sq, 0.0, border, egui::StrokeKind::Outside);
    painter.rect_stroke(strip, 0.0, border, egui::StrokeKind::Outside);

    let at: Pos2 = pos2(sq.left() + hsv[1] * sq.width(), sq.top() + (1.0 - hsv[2]) * sq.height());
    painter.circle_stroke(at, RING_RADIUS, Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.7)));
    painter.circle_stroke(at, RING_RADIUS + 1.0, Stroke::new(1.5, Color32::WHITE));
    let y = strip.top() + hsv[0] / 360.0 * strip.height();
    let bar = Rect::from_center_size(pos2(strip.center().x, y), Vec2::new(STRIP_W + 4.0, 4.0));
    painter.rect_stroke(bar, 1.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Middle);
    painter.rect_stroke(bar.shrink(1.5), 0.5, Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.7)), egui::StrokeKind::Middle);
    changed
}
