//! Eagle's "Open in New Window" (⌘O): the item in a window of its own,
//! a second eframe viewport like the collect window, with zoom, pan, Always
//! on Top and the grayscale preview (see the desktop app chapter).

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use super::{App, ext_placeholder};
use crate::loader::{Tier, job_for};
use crate::logic;
use crate::theme as t;

/// The preview window's state.
pub(crate) struct PreviewWin {
    pub id: String,
    /// Scale relative to the image's pixels; `None` fits the window.
    zoom: Option<f32>,
    pan: Vec2,
    on_top: bool,
    /// The window is being closed this frame.
    close: bool,
}

impl PreviewWin {
    pub fn new(id: String) -> PreviewWin {
        PreviewWin { id, zoom: None, pan: Vec2::ZERO, on_top: false, close: false }
    }
}

/// The window's first size for a picture `natural` big: the picture at
/// most 1000 × 760 points, never under 320 × 240.
pub fn window_size(natural: (f32, f32)) -> Vec2 {
    let (w, h) = (natural.0.max(1.0), natural.1.max(1.0));
    let k = (1000.0 / w).min(760.0 / h).min(1.0);
    vec2((w * k).max(320.0), (h * k).max(240.0))
}

impl App {
    /// Opens the item acted on (the viewer's, or the first selected) in
    /// the preview window, replacing what it showed.
    pub(crate) fn open_preview_window(&mut self) {
        if let Some(id) = self.acted_on().into_iter().next() {
            self.used_item(&id);
            self.preview = Some(PreviewWin::new(id));
        }
    }

    /// Shows the preview window, if open, as its own OS window.
    pub(crate) fn preview_window(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(id) = self.preview.as_ref().map(|p| p.id.clone()) else { return };
        let Some(item) = lib.item(&id).cloned() else {
            self.preview = None;
            return;
        };
        let natural = (item.width as f32, item.height as f32);
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("{}.{}", item.name, item.ext))
            .with_inner_size(window_size(natural))
            .with_min_inner_size([200.0, 150.0]);
        let vid = egui::ViewportId::from_hash_of("falcon-preview");
        ctx.show_viewport_immediate(vid, builder, |ui, _class| self.preview_contents(ui, lib, &item));
        if self.preview.as_ref().is_some_and(|p| p.close) {
            self.preview = None;
        }
    }

    fn preview_contents(&mut self, ui: &mut Ui, lib: &Library, item: &Item) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t::p().main);
        let (close, esc, cmd_w) = ctx.input(|i| (i.viewport().close_requested(), i.key_pressed(egui::Key::Escape), i.modifiers.command && i.key_pressed(egui::Key::W)));
        let Some(mut pw) = self.preview.take() else { return };
        if close || esc || cmd_w {
            pw.close = true;
        }

        // The picture: the full render, else the preview, else the type.
        for tier in [Tier::Full, Tier::Preview] {
            if let Some(j) = job_for(item, lib.root(), tier) {
                self.ws.frame_jobs.push(j);
            }
        }
        let tex = self.ws.loader.get(&item.id, Tier::Full).cloned().or_else(|| self.ws.loader.get(&item.id, Tier::Preview).cloned());
        let natural = if item.width > 0 && item.height > 0 {
            vec2(item.width as f32, item.height as f32)
        } else {
            tex.as_ref().map_or(Vec2::splat(512.0), |t| t.size_vec2())
        };
        let fit = (full.width() / natural.x).min(full.height() / natural.y).min(1.0);
        let scale = pw.zoom.unwrap_or(fit);

        let resp = ui.interact(full, ui.scope_id().with("preview-canvas"), Sense::click_and_drag());
        let (scroll, pinch, pointer) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
        let factor = if pinch != 1.0 { pinch } else { (scroll / 200.0).exp() };
        if resp.hovered() && factor != 1.0 {
            let new = (scale * factor).clamp(logic::ZOOM_MIN, logic::ZOOM_MAX);
            if let Some(pos) = pointer {
                let off = pos - (full.center() + pw.pan);
                pw.pan += off - off * (new / scale);
            }
            pw.zoom = Some(new);
        }
        if resp.dragged() {
            pw.pan += resp.drag_delta();
            pw.zoom.get_or_insert(scale);
        }
        if resp.double_clicked() {
            pw.zoom = None;
        }
        let scale = pw.zoom.unwrap_or(fit);
        if pw.zoom.is_none() {
            pw.pan = Vec2::ZERO;
        }
        let r = Rect::from_center_size(full.center() + pw.pan, natural * scale);
        match &tex {
            Some(tex) => {
                ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            None => ext_placeholder(ui, Rect::from_center_size(full.center(), Vec2::splat(180.0)), &item.ext),
        }

        // Eagle's window menu, from a right-click.
        let file = item.original_path(lib.root());
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            if ui.add(egui::Button::new("Always on Top").selected(pw.on_top)).clicked() {
                pw.on_top = !pw.on_top;
                let level = if pw.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                ui.close();
            }
            if ui.add(egui::Button::new("Grayscale").selected(self.modes.grayscale)).clicked() {
                self.modes.grayscale = !self.modes.grayscale;
                ui.close();
            }
            ui.separator();
            if ui.button("Actual Size").clicked() {
                pw.zoom = Some(1.0);
                pw.pan = Vec2::ZERO;
                ui.close();
            }
            if ui.button("Zoom to Fit").clicked() {
                pw.zoom = None;
                ui.close();
            }
            ui.separator();
            if ui.button("Open in Default App").clicked() {
                crate::open_with::open_default(std::slice::from_ref(&file));
                ui.close();
            }
            if ui.button("Show in Finder").clicked() {
                crate::open_with::reveal(std::slice::from_ref(&file));
                ui.close();
            }
            if ui.button("Show in Falcon").clicked() {
                // Eagle's Reveal in Eagle: the item selected in the main window.
                self.ws.viewer = None;
                self.ws.selected = vec![item.id.clone()];
                self.ws.anchor = Some(item.id.clone());
                self.ws.scroll_to = Some(item.id.clone());
                ui.ctx().send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                ui.close();
            }
            ui.separator();
            if ui.button("Close Window").clicked() {
                pw.close = true;
                ui.close();
            }
        });
        self.ws.loader.set_grayscale(self.modes.grayscale);
        self.preview = Some(pw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_fits_the_picture() {
        assert_eq!(window_size((800.0, 600.0)), vec2(800.0, 600.0));
        assert_eq!(window_size((4000.0, 2000.0)), vec2(1000.0, 500.0));
        assert_eq!(window_size((100.0, 50.0)), vec2(320.0, 240.0));
        assert_eq!(window_size((0.0, 0.0)), vec2(320.0, 240.0));
    }
}
