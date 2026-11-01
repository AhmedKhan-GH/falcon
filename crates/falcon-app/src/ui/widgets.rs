//! Small painting helpers shared by every part of the window: text
//! galleys, icons and icon buttons, fields, chips, badges, buttons and
//! thumbnail backgrounds.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Galley, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
    text::LayoutJob, vec2,
};

use crate::icons as ic;
use crate::locale::{tr, trf};
use crate::logic;
use crate::theme::{self as t, font, mono};

pub(crate) fn galley(ui: &Ui, text: &str, f: egui::FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), f, color);
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

pub(crate) fn icon(ui: &Ui, center: Pos2, glyph: &str, size: f32, color: Color32) {
    ui.painter().text(center, Align2::CENTER_CENTER, glyph, font(size), color);
}

/// A bare icon button, as in Eagle's toolbars.
pub(crate) fn icon_button(ui: &mut Ui, center: Pos2, glyph: &str, size: f32, enabled: bool, tip: &str) -> Response {
    let rect = Rect::from_center_size(center, Vec2::splat(26.0));
    let resp = ui.interact(rect, ui.scope_id().with(("icon", glyph, center.x as i32)), Sense::click());
    let color = if !enabled {
        t::p().text_disabled
    } else if resp.hovered() {
        t::p().text
    } else {
        t::p().text_value
    };
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, t::RADIUS, t::p().row_hover);
    }
    icon(ui, center, glyph, size, color);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// Makes `rect` drag the window, and double-click zoom it, like a title bar.
/// The layer a viewer draws in: a child of the central panel covering
/// `screen` and clipped to it, so the viewer never paints over or takes
/// input from the sidebar and inspector beside it.
pub(crate) fn viewer_ui(parent: &mut Ui, screen: Rect, salt: &str) -> Ui {
    let mut ui = parent.new_child(egui::UiBuilder::new().max_rect(screen).id_salt(salt));
    ui.set_clip_rect(screen.intersect(parent.clip_rect()));
    ui
}

pub(crate) fn window_drag(ui: &Ui, rect: Rect, id: &str) {
    let resp = ui.interact(rect, ui.scope_id().with(("drag", id)), Sense::click_and_drag());
    if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
    }
}

/// A rounded input box with Eagle's field styling.
pub(crate) fn field(ui: &mut Ui, value: &mut String, hint: &str, height: f32, multiline: bool, id: &str) -> Response {
    // A frame around the editor, so the box always contains its text:
    // single-line fields clip at the edge, multi-line ones grow to fit.
    egui::Frame::new()
        .fill(t::p().field)
        .stroke(Stroke::new(1.0, t::p().field_border))
        .corner_radius(t::FIELD_RADIUS)
        .inner_margin(egui::Margin::symmetric(11, 8))
        .show(ui, |ui| {
            ui.set_min_height(height - 16.0);
            let width = ui.available_width();
            let edit = if multiline {
                TextEdit::multiline(value).desired_rows(2)
            } else {
                TextEdit::singleline(value).clip_text(true)
            };
            ui.add(
                edit.id_salt(id)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .hint_text(RichText::new(hint).color(t::p().text_muted))
                    .font(font(14.0))
                    .text_color(t::p().text)
                    .desired_width(width)
                    .min_size(vec2(width, height - 16.0)),
            )
        })
        .inner
}

/// A file-type label like Eagle's: translucent type colour, uppercase.
pub(crate) fn badge(ui: &Ui, top_left: Pos2, ext: &str) {
    let Some(c) = t::ext_color(ext) else { return };
    let g = galley(ui, &ext.to_uppercase(), font(10.0), Color32::from_white_alpha(204), 60.0);
    let rect = Rect::from_min_size(top_left, vec2(g.size().x + 6.0, 18.0));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), c.gamma_multiply(0.6));
    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, c.gamma_multiply(0.5)), StrokeKind::Inside);
    p.galley(Pos2::new(rect.left() + 3.0, rect.center().y - g.size().y / 2.0), g, Color32::WHITE);
}

/// Side of the square pin badge.
pub(crate) const PIN_BADGE: f32 = 18.0;

/// The badge on an item pinned to the top of the open folder: a pin on a
/// dark translucent square.
pub(crate) fn pin_badge(ui: &Ui, top_left: Pos2) {
    let rect = Rect::from_min_size(top_left, Vec2::splat(PIN_BADGE));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), Color32::from_black_alpha(128));
    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, Color32::from_white_alpha(38)), StrokeKind::Inside);
    icon(ui, rect.center(), ic::PUSH_PIN, 12.0, Color32::WHITE);
}

pub(crate) fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, t::p().divider));
}

pub(crate) fn prop(ui: &mut Ui, key: &str, value: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
    let p = ui.painter();
    p.text(rect.left_center(), Align2::LEFT_CENTER, key, font(12.0), t::p().text);
    let g = galley(ui, value, mono(12.0), t::p().text_value, rect.width() - 90.0);
    p.galley(Pos2::new(rect.left() + 90.0, rect.center().y - g.size().y / 2.0), g, t::p().text_value);
    resp
}

/// Rating row: five stars; clicking the current rating clears it.
pub(crate) fn rating(ui: &mut Ui, star: u64) -> Option<u64> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, tr("Rating"), font(12.0), t::p().text);
    let mut out = None;
    for n in 1..=5u64 {
        let c = Pos2::new(rect.left() + 90.0 + 7.0 + (n - 1) as f32 * 17.0, rect.center().y);
        let hit = ui.interact(Rect::from_center_size(c, Vec2::splat(16.0)), ui.scope_id().with(("star", n)), Sense::click());
        let on = n <= star;
        let fam = egui::FontId::new(13.0, t::fill_family());
        let color = if on { Color32::from_rgb(0xff, 0xd6, 0x0a) } else if hit.hovered() { t::p().text_value } else { t::p().text_disabled };
        ui.painter().text(c, Align2::CENTER_CENTER, ic::STAR, fam, color);
        if hit.clicked() {
            out = Some(if star == n { 0 } else { n });
        }
    }
    out
}

pub(crate) fn chip(ui: &mut Ui, text: &str, removable: bool) -> Response {
    let g = galley(ui, text, font(12.0), t::p().text, 180.0);
    let extra = if removable { 14.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0 + extra, 22.0), Sense::click());
    let fill = if resp.hovered() { t::p().button_hover } else { t::p().button };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().galley(Pos2::new(rect.left() + 8.0, rect.center().y - g.size().y / 2.0), g, t::p().text);
    if removable && resp.hovered() {
        icon(ui, Pos2::new(rect.right() - 10.0, rect.center().y), ic::X, 10.0, t::p().text_muted);
    }
    resp.on_hover_text(if removable { tr("Click to remove") } else { "" })
}

/// An inspector tag chip with the colour dot of its group; clicking it
/// takes the tag off.
pub(crate) fn tag_chip_removable(ui: &mut Ui, text: &str, dot: Option<Color32>) -> Response {
    let g = galley(ui, text, font(12.0), t::p().text, 180.0);
    let dot_w = if dot.is_some() { 11.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0 + dot_w, 22.0), Sense::click());
    let fill = if resp.hovered() { t::p().button_hover } else { t::p().button };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    if let Some(c) = dot {
        ui.painter().circle_filled(Pos2::new(rect.left() + 10.0, rect.center().y), 3.5, c);
    }
    ui.painter().galley(Pos2::new(rect.left() + 8.0 + dot_w, rect.center().y - g.size().y / 2.0), g, t::p().text);
    if resp.hovered() {
        icon(ui, Pos2::new(rect.right() - 10.0, rect.center().y), ic::X, 10.0, t::p().text_muted);
    }
    resp.on_hover_text(tr("Click to remove"))
}

/// What a click on a folder chip asked for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipClick {
    Open,
    Remove,
}

/// The inspector's folder chip, like Eagle's: clicking it opens the folder,
/// and its × (always shown, brighter under the pointer) removes the item.
pub(crate) fn folder_chip(ui: &mut Ui, text: &str, path: &str) -> Option<ChipClick> {
    let g = galley(ui, text, font(12.0), t::p().text, 180.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0 + 16.0, 22.0), Sense::click());
    let x = Rect::from_center_size(Pos2::new(rect.right() - 11.0, rect.center().y), vec2(18.0, 18.0));
    let on_x = resp.hover_pos().is_some_and(|p| x.contains(p));
    let fill = if resp.hovered() { t::p().button_hover } else { t::p().button };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(5), fill);
    if on_x {
        p.rect_filled(x, CornerRadius::same(4), t::p().button);
    }
    p.galley(Pos2::new(rect.left() + 8.0, rect.center().y - g.size().y / 2.0), g, t::p().text);
    let mark = if on_x { t::p().text } else { t::p().text_muted.gamma_multiply(0.7) };
    icon(ui, x.center(), ic::X, 10.0, mark);
    let click = resp.clicked().then(|| if resp.interact_pointer_pos().is_some_and(|p| x.contains(p)) { ChipClick::Remove } else { ChipClick::Open });
    resp.on_hover_text(if on_x { tr("Remove from this folder").to_string() } else { trf("{path}\nClick to open", &[("path", &path)]) });
    click
}

pub(crate) fn wide_button(ui: &mut Ui, glyph: &str, label: &str, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let fill = if enabled && resp.hovered() { t::p().button_hover } else { t::p().button };
    ui.painter().rect_filled(rect, t::RADIUS, fill);
    let color = if enabled { t::p().text } else { t::p().text_muted };
    let g = galley(ui, label, font(13.0), color, rect.width());
    let total = 22.0 + g.size().x;
    let x = rect.center().x - total / 2.0;
    icon(ui, Pos2::new(x + 8.0, rect.center().y), glyph, 16.0, color);
    ui.painter().galley(Pos2::new(x + 22.0, rect.center().y - g.size().y / 2.0), g, color);
    resp
}

/// Paints an item's thumbnail background into `rect`: a plain colour, or
/// a checkerboard clipped to what is visible.
pub(crate) fn paint_thumb_bg(p: &egui::Painter, rect: Rect, bg: logic::ThumbBg) {
    use logic::ThumbBg as B;
    let plain = |c| {
        p.rect_filled(rect, 0.0, c);
    };
    match bg {
        B::Dark => plain(Color32::BLACK),
        // Eagle's box-container styles: #888 at half opacity, and a white
        // checkerboard of #e0e0e0 squares 5 px wide (two 45° gradients on a
        // 10 px tile).
        B::Gray => plain(Color32::from_rgba_unmultiplied(0x88, 0x88, 0x88, 128)),
        B::Light => plain(Color32::WHITE),
        B::Grid => {
            const CELL: f32 = 5.0;
            let vis = rect.intersect(p.clip_rect());
            if !vis.is_positive() {
                return;
            }
            p.rect_filled(vis, 0.0, Color32::WHITE);
            let (c0, r0) = (((vis.left() - rect.left()) / CELL).floor() as i64, ((vis.top() - rect.top()) / CELL).floor() as i64);
            let (c1, r1) = (((vis.right() - rect.left()) / CELL).ceil() as i64, ((vis.bottom() - rect.top()) / CELL).ceil() as i64);
            for row in r0..r1 {
                for col in c0..c1 {
                    if (row + col) % 2 == 0 {
                        continue;
                    }
                    let cell = Rect::from_min_size(rect.min + vec2(col as f32 * CELL, row as f32 * CELL), Vec2::splat(CELL)).intersect(vis);
                    p.rect_filled(cell, 0.0, Color32::from_gray(0xe0));
                }
            }
        }
    }
}

pub(crate) fn ext_placeholder(ui: &Ui, rect: Rect, ext: &str) {
    let s = rect.width().min(rect.height()) * 0.42;
    let r = Rect::from_center_size(rect.center(), vec2(s * 0.8, s));
    icon(ui, r.center(), ic::FILE, s, t::p().text_disabled);
    ui.painter().text(
        r.center() + vec2(0.0, s * 0.12),
        Align2::CENTER_CENTER,
        ext.to_uppercase(),
        font((s * 0.18).max(9.0)),
        t::p().text_muted,
    );
}
