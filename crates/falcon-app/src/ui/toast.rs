//! The status toast: drawing the one short message at a time (with its
//! Undo button and folder link) over the grid or the viewer, and the
//! `flash` calls that set it. The model is `crate::toast`.

use std::time::Instant;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, vec2};

use crate::app::App;
use crate::logic::Scope;
use crate::theme::{self as t, font};
use crate::toast::{Kind as ToastKind, Life, Link, Toast};
use crate::ui::widgets::galley;

impl App {
    /// Draws the status toast inside `full`, the central panel: at the
    /// bottom over the grid, in the middle over the viewer.
    pub(crate) fn status_toast(&mut self, ui: &mut Ui, ctx: &egui::Context, full: Rect) {
        if let Some(mut toast) = self.ws.status.clone() {
            let over_viewer = self.ws.viewer.is_some();
            if toast.at.elapsed().as_secs_f32() < crate::toast::seconds(&toast, over_viewer) {
                let p = t::p();
                let f = font(13.0);
                let gal = galley(ui, &toast.msg, f.clone(), p.text, 400.0);
                let link = toast.link.as_ref().map(|l| galley(ui, &l.label, f.clone(), p.primary, 240.0));
                let undo = toast.undo.then(|| galley(ui, "Undo", f.clone(), p.primary, 80.0));
                let mark = if toast.kind == ToastKind::Error { 20.0 } else { 0.0 };
                let gap = 8.0;
                let mut w = 24.0 + mark + gal.size().x;
                for g in link.iter().chain(undo.iter()) {
                    w += gap + g.size().x;
                }
                let h = gal.size().y.max(link.as_ref().map_or(0.0, |g| g.size().y)) + 14.0;
                // Over the viewer the note sits in the middle of the
                // picture, as Eagle's viewer tips do.
                let at = if over_viewer { full.center() } else { Pos2::new(full.center().x, full.bottom() - 30.0) };
                let r = Rect::from_center_size(at, vec2(w, h));
                ui.painter().rect_filled(r, CornerRadius::same(8), p.row_selected);
                let mut x = r.left() + 12.0;
                if mark > 0.0 {
                    let c = Pos2::new(x + 7.0, r.center().y);
                    ui.painter().circle_filled(c, 7.0, p.icon_red);
                    ui.painter().text(c, Align2::CENTER_CENTER, "!", font(11.0), Color32::WHITE);
                    x += mark;
                }
                ui.painter().galley(Pos2::new(x, r.center().y - gal.size().y / 2.0), gal.clone(), p.text);
                x += gal.size().x;
                let mut go_to = None;
                if let (Some(g), Some(l)) = (&link, &toast.link) {
                    x += gap;
                    let lr = Rect::from_min_size(Pos2::new(x, r.center().y - g.size().y / 2.0), g.size());
                    let resp = ui.interact(lr.expand(2.0), ui.scope_id().with("toast_link"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.painter().galley(lr.min, g.clone(), p.primary);
                    if resp.hovered() {
                        let y = lr.bottom() - 1.0;
                        ui.painter().line_segment([Pos2::new(lr.left(), y), Pos2::new(lr.right(), y)], Stroke::new(1.0, p.primary));
                    }
                    if resp.clicked() {
                        go_to = Some(l.folder.clone());
                    }
                    x += g.size().x;
                }
                let mut undo_clicked = false;
                if let Some(g) = &undo {
                    x += gap;
                    let ur = Rect::from_min_size(Pos2::new(x, r.center().y - g.size().y / 2.0), g.size());
                    let resp = ui.interact(ur.expand(2.0), ui.scope_id().with("toast_undo"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.painter().galley(ur.min, g.clone(), p.primary);
                    if resp.hovered() {
                        let y = ur.bottom() - 1.0;
                        ui.painter().line_segment([Pos2::new(ur.left(), y), Pos2::new(ur.right(), y)], Stroke::new(1.0, p.primary));
                    }
                    undo_clicked = resp.clicked();
                }
                if let Some(folder) = go_to {
                    self.ws.status = None;
                    self.go(Scope::Folder(folder));
                } else if undo_clicked {
                    self.ws.status = None;
                    self.ws.undo_request = Some(true);
                } else {
                    // While the pointer rests on the toast it stays up.
                    if ctx.pointer_hover_pos().is_some_and(|pos| r.contains(pos)) {
                        toast.at = Instant::now();
                        self.ws.status = Some(toast);
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
            } else {
                self.ws.status = None;
            }
        }
    }

    /// Shows a note for a few seconds; a new one replaces the old.
    pub(crate) fn flash(&mut self, msg: impl Into<String>) {
        self.ws.status = Some(Toast::new(msg, ToastKind::Info, Life::Normal));
    }

    /// A quick confirmation, such as "Copied 2 tags".
    pub(crate) fn flash_brief(&mut self, msg: impl Into<String>) {
        self.ws.status = Some(Toast::new(msg, ToastKind::Info, Life::Brief));
    }

    /// A failure: a red mark before the text, and it stays longer.
    pub(crate) fn flash_error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        crate::errlog::error(&msg);
        self.ws.status = Some(Toast::new(msg, ToastKind::Error, Life::Normal));
    }

    /// A note with an Undo button, for actions the undo history records.
    pub(crate) fn flash_undo(&mut self, msg: impl Into<String>) {
        let mut t = Toast::new(msg, ToastKind::Info, Life::Undoable);
        t.undo = true;
        self.ws.status = Some(t);
    }

    /// A note that ends in the folder's name as a link that opens it.
    /// `msg` is the text before the name ("Added 3 items to").
    pub(crate) fn flash_folder(&mut self, msg: impl Into<String>, folder_name: &str, folder_id: &str, undo: bool) {
        let mut t = Toast::new(msg, ToastKind::Info, if undo { Life::Undoable } else { Life::Normal });
        t.undo = undo;
        if !folder_name.is_empty() {
            t.link = Some(Link { label: folder_name.to_string(), folder: folder_id.to_string() });
        }
        self.ws.status = Some(t);
    }
}
