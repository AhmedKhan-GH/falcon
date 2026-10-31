//! Comments on a region of an image: markers and drawing in the viewer, the
//! inspector's list and the count badge on cards. Region coordinates are in
//! the image's displayed pixels (`width` × `height`), as Eagle stores them.
//!
//! Eagle keeps a comment's text as the HTML of an editable box (`<br>` for
//! line breaks). Falcon writes plain text and shows Eagle's HTML with the
//! tags taken out.

use eframe::egui::{self, Align2, Color32, CornerRadius, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, text::LayoutJob, vec2};
use falcon_library::model::{Comment, Item};

use super::image_edit::{Draft, Op};
use super::{Action, App, heading, icon_button};
use super::crop::{to_image, to_screen};
use crate::icons as ic;
use crate::theme::{self as t, font, mono};

/// Smallest region Eagle keeps: both sides over 10 image pixels.
pub const MIN_REGION: f32 = 10.0;

/// A comment's text without HTML: `<br>` and block ends become line
/// breaks, other tags are dropped and the common entities decoded.
pub fn plain_text(annotation: &str) -> String {
    let mut out = String::new();
    let mut rest = annotation;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = rest[start + 1..start + end].trim().to_ascii_lowercase();
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let closing = tag.starts_with('/');
        if name == "br" || (closing && matches!(name.as_str(), "div" | "p" | "li")) || (!closing && name == "div" && !out.is_empty() && !out.ends_with('\n')) {
            out.push('\n');
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    let out = out.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&");
    let lines: Vec<&str> = out.lines().collect();
    let mut s = lines.join("\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s.trim_start_matches('\n').to_string()
}

/// `m:ss` (or `h:mm:ss`) for a timed comment.
pub fn time_label(seconds: f64) -> String {
    let s = seconds.max(0.0).floor() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// A region comment's rectangle in image pixels; `None` for timed ones.
pub fn region(c: &Comment) -> Option<Rect> {
    let n = |v: &Option<serde_json::Number>| v.as_ref().and_then(|n| n.as_f64()).map(|f| f as f32);
    let (x, y, w, h) = (n(&c.x)?, n(&c.y)?, n(&c.width)?, n(&c.height)?);
    Some(Rect::from_min_size(Pos2::new(x, y), vec2(w, h)))
}

/// The region dragged from `a` to `b` (image pixels), clipped to the image
/// and rounded to whole pixels as Eagle rounds it. `None` unless both
/// sides are over [`MIN_REGION`].
pub fn new_region(a: Pos2, b: Pos2, bounds: Vec2) -> Option<Rect> {
    let clamp = |p: Pos2| Pos2::new(p.x.clamp(0.0, bounds.x).round(), p.y.clamp(0.0, bounds.y).round());
    let r = Rect::from_two_pos(clamp(a), clamp(b));
    (r.width() > MIN_REGION && r.height() > MIN_REGION).then_some(r)
}

/// Whether the viewer offers comment mode for this item. Eagle hides it for
/// video, audio, fonts, 3D models, PDFs and text.
pub fn supports_regions(item: &Item) -> bool {
    let ext = item.ext.to_lowercase();
    item.width > 0
        && item.height > 0
        && !falcon_library::media::is_video(&ext)
        && !falcon_library::media::is_audio(&ext)
        && !falcon_library::import::FONT_EXTENSIONS.contains(&ext.as_str())
        && !matches!(ext.as_str(), "pdf" | "txt" | "woff2" | "glb" | "gltf" | "obj" | "fbx" | "stl" | "url")
}

/// Eagle's comment count: a blue disc with the number at a card's top right.
pub fn count_badge(ui: &Ui, tile: Rect, item: &Item) {
    let Some(serde_json::Value::Array(list)) = item.extra.get("comments") else { return };
    if list.is_empty() {
        return;
    }
    let c = Pos2::new(tile.right() - 15.0, tile.top() + 15.0);
    let p = ui.painter();
    p.circle_filled(c, 10.0, t::p().primary);
    p.circle_stroke(c, 10.0, Stroke::new(1.0, Color32::WHITE));
    p.text(c, Align2::CENTER_CENTER, list.len().to_string(), mono(11.0), Color32::WHITE);
}

fn wrapped(ui: &Ui, text: &str, size: f32, color: Color32, width: f32, rows: usize) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text.to_string(), font(size), color, width.max(1.0));
    job.wrap.max_rows = rows;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

impl App {
    /// The comment button for the viewer bar at `x`; returns the next free x.
    pub(super) fn comment_button(&mut self, ui: &mut Ui, item: &Item, x: f32, cy: f32) -> f32 {
        if !supports_regions(item) {
            return x;
        }
        let on = self.ws.tools.comment_mode;
        let r = Rect::from_center_size(Pos2::new(x, cy), Vec2::splat(26.0));
        if on {
            ui.painter().rect_filled(r, t::RADIUS, t::p().primary.gamma_multiply(0.5));
        }
        if icon_button(ui, Pos2::new(x, cy), ic::CHAT_CIRCLE, 17.0, true, "Comment Mode (C)").clicked() {
            self.ws.tools.comment_mode = !on;
            self.ws.tools.crop = None;
        }
        let n = item.comments().len();
        if n > 0 {
            let c = Pos2::new(x + 9.0, cy - 8.0);
            ui.painter().circle_filled(c, 7.0, t::p().primary);
            ui.painter().text(c, Align2::CENTER_CENTER, n.to_string(), mono(9.0), Color32::WHITE);
        }
        x - 30.0
    }

    /// Region markers over the image at `shown`, and in comment mode the
    /// drawing of new regions and the note editor. Returns true while
    /// comment mode is on, so the viewer doesn't pan.
    pub(super) fn comment_overlay(&mut self, ui: &mut Ui, item: &Item, shown: Rect, area: Rect) -> bool {
        if !supports_regions(item) {
            return false;
        }
        let natural = vec2(item.width as f32, item.height as f32);
        let mode = self.ws.tools.comment_mode && self.ws.tools.crop.is_none();
        let p = ui.painter().clone().with_clip_rect(area);

        if mode {
            let canvas = ui.interact(area, ui.scope_id().with("comment-canvas"), Sense::drag());
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if canvas.drag_started() {
                self.ws.tools.draw_start = ui.input(|i| i.pointer.press_origin()).map(|o| to_image(o, shown, natural));
                self.ws.tools.draft = None;
            }
            if let (Some(a), Some(now)) = (self.ws.tools.draw_start, canvas.interact_pointer_pos()) {
                let b = to_image(now, shown, natural);
                let r = to_screen(Rect::from_two_pos(a, b), shown, natural);
                p.rect_filled(r, 0.0, t::p().primary.gamma_multiply(0.15));
                p.rect_stroke(r, 0.0, Stroke::new(1.5, t::p().primary), StrokeKind::Middle);
                if canvas.drag_stopped() {
                    self.ws.tools.draw_start = None;
                    if let Some(rect) = new_region(a, b, natural) {
                        self.ws.tools.draft = Some(Draft { item: item.id.clone(), id: None, rect, text: String::new(), focus: true });
                    }
                }
            }
            if canvas.drag_stopped() {
                self.ws.tools.draw_start = None;
            }
        }

        let comments = item.comments();
        for (n, c) in comments.iter().enumerate() {
            let Some(rect) = region(c) else { continue };
            let s = to_screen(rect, shown, natural);
            let editing = self.ws.tools.draft.as_ref().is_some_and(|d| d.id.as_deref() == Some(c.id.as_str()));
            let resp = ui.interact(s, ui.scope_id().with(("comment", &c.id)), if mode { Sense::click() } else { Sense::hover() });
            let hot = resp.hovered() || editing;
            p.rect_filled(s, 0.0, Color32::from_black_alpha(if hot { 40 } else { 20 }));
            p.rect_stroke(s, 0.0, Stroke::new(if hot { 2.0 } else { 1.5 }, t::p().primary), StrokeKind::Middle);
            let badge = s.left_top() + vec2(0.0, 0.0);
            p.circle_filled(badge, 10.0, t::p().primary);
            p.circle_stroke(badge, 10.0, Stroke::new(1.0, Color32::WHITE));
            p.text(badge, Align2::CENTER_CENTER, (n + 1).to_string(), mono(11.0), Color32::WHITE);
            let text = plain_text(&c.annotation);
            if !editing {
                let tip = if text.is_empty() { "(no text)".to_string() } else { text };
                resp.clone().on_hover_text(tip);
            }
            if mode && resp.clicked() {
                self.ws.tools.draft = Some(Draft { item: item.id.clone(), id: Some(c.id.clone()), rect, text: plain_text(&c.annotation), focus: true });
            }
            if mode && hot {
                let x = Rect::from_center_size(s.right_top(), Vec2::splat(18.0));
                let xr = ui.interact(x, ui.scope_id().with(("comment-x", &c.id)), Sense::click());
                p.circle_filled(x.center(), 9.0, if xr.hovered() { t::p().icon_red } else { t::p().button });
                p.text(x.center(), Align2::CENTER_CENTER, ic::X, font(10.0), Color32::WHITE);
                if xr.on_hover_text("Delete comment").clicked() {
                    self.ws.actions.push(Action::Edit(Op::DeleteComment(item.id.clone(), c.id.clone())));
                    self.ws.tools.draft = None;
                }
            }
        }

        // A new region not saved yet.
        if let Some(d) = self.ws.tools.draft.as_ref().filter(|d| d.id.is_none() && d.item == item.id) {
            let s = to_screen(d.rect, shown, natural);
            p.rect_filled(s, 0.0, t::p().primary.gamma_multiply(0.15));
            p.rect_stroke(s, 0.0, Stroke::new(2.0, t::p().primary), StrokeKind::Middle);
        }
        if self.ws.tools.draft.as_ref().is_some_and(|d| d.item == item.id) {
            let s = to_screen(self.ws.tools.draft.as_ref().unwrap().rect, shown, natural);
            self.comment_editor(ui, item, s, area);
        }
        mode
    }

    /// The small note editor under a region being written.
    fn comment_editor(&mut self, ui: &mut Ui, item: &Item, region: Rect, area: Rect) {
        let width = 260.0;
        let x = region.left().clamp(area.left() + 8.0, (area.right() - width - 8.0).max(area.left() + 8.0));
        let y = (region.bottom() + 8.0).min(area.bottom() - 130.0);
        let mut save = false;
        let mut cancel = false;
        let mut delete = false;
        let draft = self.ws.tools.draft.as_mut().unwrap();
        egui::Area::new(ui.scope_id().with("comment-editor")).order(egui::Order::Tooltip).fixed_pos(Pos2::new(x, y)).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(t::p().panel).stroke(Stroke::new(1.0, t::p().divider)).corner_radius(CornerRadius::same(8)).inner_margin(10).show(ui, |ui| {
                ui.set_width(width - 20.0);
                let te = ui.add(
                    TextEdit::multiline(&mut draft.text)
                        .id_salt("comment-text")
                        .hint_text("Write a comment…")
                        .font(font(13.0))
                        .desired_rows(3)
                        .desired_width(width - 20.0),
                );
                if std::mem::take(&mut draft.focus) {
                    te.request_focus();
                }
                if te.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                    save = true;
                }
                // The text box gives up focus on Esc itself.
                if (te.has_focus() || te.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
                    cancel = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").on_hover_text("⌘↩").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if draft.id.is_some() && ui.button("Delete").clicked() {
                        delete = true;
                    }
                });
            });
        });
        if save {
            let d = self.ws.tools.draft.take().unwrap();
            let r = d.rect;
            let text = d.text.trim_end().to_string();
            match d.id {
                None => {
                    let c = Comment::region(r.min.x as u64, r.min.y as u64, r.width() as u64, r.height() as u64, &text);
                    self.ws.actions.push(Action::Edit(Op::AddComment(item.id.clone(), c)));
                }
                Some(id) => {
                    if let Some(mut c) = item.comments().into_iter().find(|c| c.id == id)
                        && plain_text(&c.annotation) != text
                    {
                        c.annotation = text;
                        self.ws.actions.push(Action::Edit(Op::UpdateComment(item.id.clone(), c)));
                    }
                }
            }
        } else if delete {
            let d = self.ws.tools.draft.take().unwrap();
            if let Some(id) = d.id {
                self.ws.actions.push(Action::Edit(Op::DeleteComment(item.id.clone(), id)));
            }
        } else if cancel {
            self.ws.tools.draft = None;
        }
    }

    /// The inspector's comment list with edit and delete.
    pub(super) fn comment_list(&mut self, ui: &mut Ui, item: &Item) {
        let comments = item.comments();
        if comments.is_empty() {
            return;
        }
        heading(ui, &format!("Comments ({})", comments.len()));
        let w = ui.available_width();
        for (n, c) in comments.iter().enumerate() {
            let editing = self.ws.tools.list_edit.as_ref().is_some_and(|(i, id, _)| *i == item.id && *id == c.id);
            if editing {
                let (_, _, text) = self.ws.tools.list_edit.as_mut().unwrap();
                let te = ui.add(TextEdit::multiline(text).id_salt(("comment-edit", &c.id)).font(font(13.0)).desired_rows(2).desired_width(w));
                let mut done = None;
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() || (te.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter))) {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        done = Some(false);
                    }
                });
                if let Some(save) = done {
                    let (_, _, text) = self.ws.tools.list_edit.take().unwrap();
                    let text = text.trim_end().to_string();
                    if save && text != plain_text(&c.annotation) {
                        let mut c = c.clone();
                        c.annotation = text;
                        self.ws.actions.push(Action::Edit(Op::UpdateComment(item.id.clone(), c)));
                    }
                }
                continue;
            }
            let text = plain_text(&c.annotation);
            let label = match c.duration.as_ref().and_then(|d| d.as_f64()) {
                Some(s) => format!("{}  {}", time_label(s), text),
                None => text,
            };
            let g = wrapped(ui, if label.is_empty() { "(no text)" } else { &label }, 12.5, t::p().text, w - 64.0, 3);
            let (row, resp) = ui.allocate_exact_size(vec2(w, g.size().y.max(16.0) + 8.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(row, CornerRadius::same(4), t::p().row_hover);
            }
            let badge = Pos2::new(row.left() + 9.0, row.top() + 12.0);
            ui.painter().circle_filled(badge, 8.0, t::p().primary);
            ui.painter().text(badge, Align2::CENTER_CENTER, (n + 1).to_string(), mono(10.0), Color32::WHITE);
            ui.painter().galley(Pos2::new(row.left() + 24.0, row.top() + 4.0), g, t::p().text);
            if resp.hovered() || ui.rect_contains_pointer(row) {
                let edit = icon_button(ui, Pos2::new(row.right() - 34.0, row.top() + 12.0), ic::PENCIL, 13.0, true, "Edit");
                let del = icon_button(ui, Pos2::new(row.right() - 10.0, row.top() + 12.0), ic::TRASH, 13.0, true, "Delete");
                if edit.clicked() {
                    self.ws.tools.list_edit = Some((item.id.clone(), c.id.clone(), plain_text(&c.annotation)));
                }
                if del.clicked() {
                    self.ws.actions.push(Action::Edit(Op::DeleteComment(item.id.clone(), c.id.clone())));
                }
            }
            if resp.clicked() && region(c).is_some() {
                // Show it on the image, in comment mode, as Eagle does.
                self.ws.viewer = Some(item.id.clone());
                self.ws.tools.comment_mode = true;
            }
        }
        ui.add_space(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_reduced_to_text() {
        assert_eq!(plain_text("plain"), "plain");
        assert_eq!(plain_text("one<br>two<br/>three"), "one\ntwo\nthree");
        assert_eq!(plain_text("first<div>second</div><div>third</div>"), "first\nsecond\nthird");
        assert_eq!(plain_text("<b>bold</b> &amp; <i>it</i>&nbsp;x &lt;tag&gt;"), "bold & it x <tag>");
        assert_eq!(plain_text("a < b"), "a < b");
        assert_eq!(plain_text("trailing<br>"), "trailing");
        assert_eq!(plain_text(""), "");
    }

    #[test]
    fn times() {
        assert_eq!(time_label(0.4), "0:00");
        assert_eq!(time_label(75.9), "1:15");
        assert_eq!(time_label(3725.0), "1:02:05");
    }

    #[test]
    fn regions_need_more_than_ten_pixels() {
        let b = vec2(1000.0, 800.0);
        assert_eq!(new_region(Pos2::new(110.4, 50.0), Pos2::new(20.0, 200.6), b), Some(Rect::from_min_max(Pos2::new(20.0, 50.0), Pos2::new(110.0, 201.0))));
        assert_eq!(new_region(Pos2::new(0.0, 0.0), Pos2::new(10.0, 300.0), b), None);
        assert_eq!(new_region(Pos2::new(-50.0, -50.0), Pos2::new(2000.0, 20.0), b), Some(Rect::from_min_max(Pos2::ZERO, Pos2::new(1000.0, 20.0))));
        let c = Comment::region(20, 50, 90, 151, "x");
        assert_eq!(region(&c), Some(Rect::from_min_size(Pos2::new(20.0, 50.0), vec2(90.0, 151.0))));
        assert_eq!(region(&Comment::at_time(3.0, "x")), None);
    }

    #[test]
    fn region_support_follows_eagle() {
        let mut item: Item = serde_json::from_value(serde_json::json!({
            "id": "A", "name": "a", "size": 1, "btime": 0, "mtime": 0, "ext": "png", "tags": [], "folders": [],
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": 0, "height": 10, "width": 10,
            "lastModified": 0, "palettes": []
        }))
        .unwrap();
        assert!(supports_regions(&item));
        item.ext = "mp4".into();
        assert!(!supports_regions(&item));
        item.ext = "ttf".into();
        assert!(!supports_regions(&item));
        item.ext = "png".into();
        item.width = 0;
        assert!(!supports_regions(&item));
    }
}
