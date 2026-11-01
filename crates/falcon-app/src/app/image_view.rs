//! The image viewer's canvas: the picture with zoom and pan, the zoom
//! controls in the bar, and the full-resolution original drawn in tiles
//! when zoomed in past the full tier (see the desktop app chapter).

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, TextureHandle, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use super::{App, ext_placeholder, icon_button, media_view};
use crate::icons as ic;
use crate::loader::{Tier, job_for, original_job};
use crate::logic;
use crate::theme::{self as t, mono};

impl App {
    /// Draws the picture `tex` (a frame of an animation, the full render or
    /// the preview) in `area` under the viewer's `bar`, with its zoom
    /// controls, and the GIF frame player's bar in `gif_bar` when it is on.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn image_canvas(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        item: &Item,
        tex: Option<TextureHandle>,
        bar: Rect,
        area: Rect,
        gif_bar: Option<Rect>,
    ) {
        let id = item.id.clone();
        let p = ui.painter().clone();
        let natural = if item.width > 0 && item.height > 0 {
            vec2(item.width as f32, item.height as f32)
        } else {
            tex.as_ref().map_or(Vec2::splat(512.0), |t| t.size_vec2())
        };
        let fit = ((area.width() - 80.0) / natural.x).min((area.height() - 80.0) / natural.y).min(1.0);
        let scale = self.ws.view.zoom.unwrap_or(fit);

        // Scroll or pinch over the picture zooms around the pointer;
        // dragging pans.
        let resp = ui.interact(area, ui.scope_id().with("viewer-canvas"), Sense::click_and_drag());
        let (scroll, pinch, pointer) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
        let factor = if !resp.hovered() {
            1.0
        } else if pinch != 1.0 {
            pinch
        } else if scroll != 0.0 {
            (scroll / 300.0).exp()
        } else {
            1.0
        };
        if factor != 1.0 {
            let new = (scale * factor).clamp(0.02, 32.0);
            if let Some(pos) = pointer {
                let center = area.center() + self.ws.view.pan;
                let off = pos - center;
                self.ws.view.pan += off - off * (new / scale);
            }
            self.ws.view.zoom = Some(new);
        }
        if resp.dragged() {
            self.ws.view.pan += resp.drag_delta();
            if self.ws.view.zoom.is_none() {
                self.ws.view.zoom = Some(scale);
            }
        }
        // Right-click opens the item's menu, as on its card.
        resp.context_menu(|ui| {
            self.item_menu(ui, lib, item, std::slice::from_ref(&id));
        });
        // Double-clicking the picture or the space around it leaves the
        // viewer, as in Eagle (not while cropping or marking regions).
        if resp.double_clicked() && self.ws.tools.crop.is_none() && !self.ws.tools.comment_mode {
            self.ws.viewer = None;
        }
        let scale = self.ws.view.zoom.unwrap_or(fit);
        if self.ws.view.zoom.is_none() {
            self.ws.view.pan = Vec2::ZERO;
        }
        let r = Rect::from_center_size(area.center() + self.ws.view.pan, natural * scale);
        let mut clip = ui.new_child(egui::UiBuilder::new().max_rect(area));
        clip.set_clip_rect(area);

        // Zoomed in past the full tier's render, the original is decoded at
        // full resolution (on a worker, in tiles) and drawn instead; never
        // for an animation, whose player owns the picture.
        let animated = self.ws.view.anim.as_ref().is_some_and(|a| a.texture().is_some());
        let shown_px = r.width().max(r.height()) * ui.ctx().pixels_per_point();
        let tile = logic::tile_side(ui.ctx().input(|i| i.max_texture_side));
        if !animated
            && logic::wants_original(&item.ext.to_lowercase(), natural.x.max(natural.y) as u32, shown_px)
            && let Some(j) = original_job(item, lib.root(), tile)
        {
            self.ws.frame_jobs.push(j);
        }
        let full_px = tex.as_ref().map_or(0.0, |t| t.size_vec2().max_elem());
        let original = self.ws.loader.original(&id).filter(|o| !animated && shown_px > full_px && o.size[0].max(o.size[1]) as f32 > full_px);

        // Without a full render the thumbnail stands in; with neither, the
        // format can't be previewed.
        let full_failed = self.ws.loader.has_failed(&id, Tier::Full) || job_for(item, lib.root(), Tier::Full).is_none();
        let preview_job = if full_failed { job_for(item, lib.root(), Tier::Preview) } else { None };
        let no_preview = full_failed && (preview_job.is_none() || self.ws.loader.has_failed(&id, Tier::Preview));
        match (&tex, original) {
            (_, Some(o)) => {
                let painter = clip.painter();
                for t in &o.tiles {
                    let rect = Rect::from_min_max(r.min + t.rect.min.to_vec2() * r.size(), r.min + t.rect.max.to_vec2() * r.size());
                    if rect.intersects(area) {
                        painter.image(t.texture.id(), rect, t.uv, Color32::WHITE);
                    }
                }
            }
            (Some(tex), None) => egui::Image::new((tex.id(), r.size())).paint_at(&clip, r),
            (None, None) if no_preview => self.cannot_preview(&mut clip, item, area),
            (None, None) => ext_placeholder(&clip, Rect::from_center_size(area.center(), Vec2::splat(260.0)), &item.ext),
        }
        if tex.is_none()
            && let Some(j) = preview_job
        {
            self.ws.frame_jobs.push(j);
        }
        self.edit_overlay(ui, item, r, area);
        if let Some(gif_bar) = gif_bar {
            self.gif_toolbar(ui, item, gif_bar, &resp);
        }

        // The bar on top: back and name, then zoom and the tools.
        let (name_x, right) = media_view::bar_shell(self, ui, item, bar);
        let cy = bar.center().y;
        use crate::shortcuts::Command as C;
        let tip = |label: &str, fixed: &str, cmd: C| match self.shortcuts.binding(cmd) {
            Some(c) => format!("{label} ({fixed}, {})", crate::shortcuts::display(ui.ctx(), &c)),
            None => format!("{label} ({fixed})"),
        };
        let (t_in, t_out, t_actual, t_fit) =
            (tip("Zoom In", "+", C::ZoomIn), tip("Zoom Out", "−", C::ZoomOut), tip("Actual Size", "1", C::ZoomActual), tip("Zoom to Fit", "0", C::ZoomFit));
        let mut zoom = self.ws.view.zoom_key.take();
        let mut x = right - 14.0;
        if icon_button(ui, Pos2::new(x, cy), ic::PLUS, 15.0, true, &t_in).clicked() {
            zoom = Some(logic::ViewZoom::In);
        }
        x -= 28.0;
        p.text(Pos2::new(x, cy), Align2::CENTER_CENTER, format!("{:.0}%", scale * 100.0), mono(12.0), t::p().text_value);
        x -= 30.0;
        if icon_button(ui, Pos2::new(x, cy), ic::MINUS, 15.0, true, &t_out).clicked() {
            zoom = Some(logic::ViewZoom::Out);
        }
        x -= 40.0;
        let actual = ui.put(Rect::from_center_size(Pos2::new(x, cy), vec2(46.0, 24.0)), egui::Button::new("1:1"));
        if actual.on_hover_text(t_actual).clicked() {
            zoom = Some(logic::ViewZoom::Actual);
        }
        x -= 50.0;
        if ui.put(Rect::from_center_size(Pos2::new(x, cy), vec2(46.0, 24.0)), egui::Button::new("Fit")).on_hover_text(t_fit).clicked() {
            zoom = Some(logic::ViewZoom::Fit);
        }
        let tools_x = self.gif_toggle_button(ui, item, x - 50.0, cy);
        let tools_left = self.edit_bar(ui, item, tools_x, cy);
        media_view::bar_name(ui, item, bar, name_x, tools_left);

        if let Some(z) = zoom {
            self.ws.view.zoom = logic::view_zoom(z, scale);
            if matches!(z, logic::ViewZoom::Actual | logic::ViewZoom::Fit) {
                self.ws.view.pan = Vec2::ZERO;
            }
        }
        if let Some(j) = job_for(item, lib.root(), Tier::Full) {
            self.ws.frame_jobs.push(j);
        }
    }
}
