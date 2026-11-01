//! The image viewer's canvas: the picture with zoom and pan, the zoom
//! controls in the bar, and the full-resolution original drawn in tiles
//! when zoomed in past the full tier (see the desktop app chapter).

use std::collections::HashMap;
use std::time::Instant;

use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2, vec2};
use falcon_library::{Library, thumbnail};
use falcon_library::model::Item;

use super::{App, ext_placeholder, media_view};
use crate::loader::{Tier, job_for, original_job};
use crate::logic;
use crate::theme::{self as t, mono};

/// The image viewer's state that outlives one item.
#[derive(Default)]
pub(super) struct ImageState {
    /// "Fit to window edge" (backtick): every image opens filling the view,
    /// enlarged if small, until switched off or Zoom to Fit is chosen.
    pub edge: bool,
    /// The item the opening zoom was last applied to.
    opened: String,
    /// Zoom (`None`: fitted) and pan each image was left at, this session
    /// (Eagle's `lastItemStates`, also kept in memory only).
    views: HashMap<String, (Option<f32>, Vec2)>,
    /// A wheel move in paging mode, stepped once the wheel rests.
    page: Option<(isize, Instant)>,
    /// The transparency checkerboard, made on first use.
    checker: Option<TextureHandle>,
    /// The visible part of a zoomed-in SVG and since when it has held still.
    svg_view: Option<([f32; 4], [u32; 2], Instant)>,
    /// Where in the navigator's frame a drag took hold, from its centre.
    nav_grab: Vec2,
}

/// Side of a checkerboard square, in points.
const CHECKER: f32 = 8.0;

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
        let ext = item.ext.to_lowercase();
        if logic::too_large_to_preview(&ext, item.width, item.height, thumbnail::has_custom_thumbnail(item)) {
            self.too_large(ui, lib, item, bar, area);
            return;
        }
        let natural = if item.width > 0 && item.height > 0 {
            vec2(item.width as f32, item.height as f32)
        } else {
            tex.as_ref().map_or(Vec2::splat(512.0), |t| t.size_vec2())
        };
        let (fit, fit_top) = if self.image.edge {
            (logic::edge_fit((natural.x, natural.y), (area.width(), area.height())), 0.0)
        } else {
            logic::smart_fit((natural.x, natural.y), (area.width(), area.height()), 40.0, &item.name)
        };
        self.ws.loader.set_pixelated(self.prefs.pixelated);
        self.ws.loader.set_grayscale(self.modes.grayscale);
        // A newly opened image starts where it was left, at actual size, or
        // fitted, by the preferences; "fit to window edge" overrides both,
        // as in Eagle.
        if self.image.opened != id {
            self.image.opened = id.clone();
            let remembered = self.prefs.remember_zoom.then(|| self.image.views.get(&id).copied()).flatten();
            match remembered {
                _ if self.image.edge => {}
                Some((zoom, pan)) => (self.ws.view.zoom, self.ws.view.pan) = (zoom, pan),
                None if self.prefs.open_actual_size => (self.ws.view.zoom, self.ws.view.pan) = (Some(1.0), Vec2::ZERO),
                None => {}
            }
        }
        let scale = self.ws.view.zoom.unwrap_or(fit);

        // The wheel scrolls, zooms or pages by the preference; pinching (or
        // the wheel with ⌘, ⌃ or ⌥) zooms around the pointer; dragging pans.
        let resp = ui.interact(area, ui.scope_id().with("viewer-canvas"), Sense::click_and_drag());
        let (scroll, pinch, alt, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt, i.pointer.hover_pos()));
        let action =
            if resp.hovered() { logic::wheel_action(self.prefs.viewer_wheel, (scroll.x, scroll.y), pinch, alt) } else { logic::WheelAction::None };
        match action {
            logic::WheelAction::Zoom(factor) => {
                let new = (scale * factor).clamp(logic::ZOOM_MIN, logic::ZOOM_MAX);
                if let Some(pos) = pointer {
                    let center = area.center() + self.ws.view.pan;
                    let off = pos - center;
                    self.ws.view.pan += off - off * (new / scale);
                }
                self.ws.view.zoom = Some(new);
            }
            logic::WheelAction::Pan(dx, dy) => {
                let pan = self.ws.view.pan + vec2(dx, dy);
                let (px, py) = logic::clamp_pan((pan.x, pan.y), (natural.x * scale, natural.y * scale), (area.width(), area.height()));
                self.ws.view.pan = vec2(px, py);
                self.ws.view.zoom = Some(scale);
            }
            logic::WheelAction::Page(step) => self.image.page = Some((step, Instant::now())),
            logic::WheelAction::None => {}
        }
        if let Some((step, at)) = self.image.page {
            if at.elapsed().as_secs_f32() >= logic::PAGE_REST {
                self.image.page = None;
                self.viewer_step(step);
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32(logic::PAGE_REST));
            }
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
        // A middle click enters or leaves presentation mode, as Eagle's
        // `onDetailClick`.
        if resp.middle_clicked() {
            self.toggle_presentation(ui.ctx());
        }
        // Double-clicking the picture or the space around it leaves the
        // viewer, as in Eagle (not while cropping or marking regions).
        if resp.double_clicked() && self.ws.tools.crop.is_none() && !self.ws.tools.comment_mode {
            self.ws.viewer = None;
        }
        let scale = self.ws.view.zoom.unwrap_or(fit);
        if self.ws.view.zoom.is_none() {
            self.ws.view.pan = vec2(0.0, fit_top);
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
            && logic::wants_original(&ext, natural.x.max(natural.y) as u32, shown_px)
            && let Some(j) = original_job(item, lib.root(), tile)
        {
            self.ws.frame_jobs.push(j);
        }
        // An SVG enlarged past its render is drawn again, sharp, for the part
        // on screen once the view holds still.
        let mut patch = None;
        if ext == "svg" && tex.as_ref().is_some_and(|t| shown_px > t.size_vec2().max_elem()) {
            let max = ui.ctx().input(|i| i.max_texture_side).min(8192) as u32;
            let region = logic::svg_region([r.min.x, r.min.y, r.max.x, r.max.y], [area.min.x, area.min.y, area.max.x, area.max.y], ui.ctx().pixels_per_point(), max);
            if let Some((rect, size)) = region {
                let same = self.image.svg_view.is_some_and(|(r0, s0, _)| r0 == rect && s0 == size);
                if !same {
                    self.image.svg_view = Some((rect, size, Instant::now()));
                }
                let settled = self.image.svg_view.is_some_and(|(_, _, at)| at.elapsed().as_secs_f32() >= logic::SVG_SETTLE);
                if settled {
                    if let Some(j) = job_for(item, lib.root(), Tier::Full) {
                        self.ws.loader.want_svg_region(j, rect, size);
                    }
                } else {
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32(logic::SVG_SETTLE));
                }
            }
            patch = self.ws.loader.svg_patch(&id).map(|p| {
                (p.texture.id(), Rect::from_min_max(r.min + p.rect.min.to_vec2() * r.size(), r.min + p.rect.max.to_vec2() * r.size()))
            });
        }
        let full_px = tex.as_ref().map_or(0.0, |t| t.size_vec2().max_elem());
        let original = self.ws.loader.original(&id).filter(|o| !animated && shown_px > full_px && o.size[0].max(o.size[1]) as f32 > full_px);

        // Without a full render the thumbnail stands in; with neither, the
        // format can't be previewed.
        let full_failed = self.ws.loader.has_failed(&id, Tier::Full) || job_for(item, lib.root(), Tier::Full).is_none();
        let preview_job = if full_failed { job_for(item, lib.root(), Tier::Preview) } else { None };
        let no_preview = full_failed && (preview_job.is_none() || self.ws.loader.has_failed(&id, Tier::Preview));
        let has_original = original.is_some();
        // The item's own thumbnail background wins over the preference.
        let item_bg = crate::logic::ThumbBg::of(item);
        if let Some(bg) = item_bg
            && (tex.is_some() || has_original)
            && r.intersects(area)
        {
            super::paint_thumb_bg(clip.painter(), r, bg);
        } else if self.prefs.transparency_grid && (tex.is_some() || has_original) && r.intersects(area) {
            let checker = self.image.checker.get_or_insert_with(|| {
                let (a, b) = (Color32::WHITE, Color32::from_gray(0xcc));
                ui.ctx().load_texture("viewer-checker", ColorImage::new([2, 2], vec![a, b, b, a]), TextureOptions::NEAREST_REPEAT)
            });
            let shown = r.intersect(area);
            let uv = |p: Pos2| ((p - r.min) / (2.0 * CHECKER)).to_pos2();
            clip.painter().image(checker.id(), shown, Rect::from_min_max(uv(shown.min), uv(shown.max)), Color32::WHITE);
        }
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
            (Some(tex), None) => match patch {
                // Under the sharp part of an SVG the render isn't drawn, so
                // semi-transparent shapes aren't laid on twice.
                Some((patch_tex, hole)) => {
                    paint_except(clip.painter(), tex.id(), r, hole);
                    clip.painter().image(patch_tex, hole, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => egui::Image::new((tex.id(), r.size())).paint_at(&clip, r),
            },
            (None, None) if no_preview => self.cannot_preview(&mut clip, item, area),
            (None, None) => ext_placeholder(&clip, Rect::from_center_size(area.center(), Vec2::splat(260.0)), &item.ext),
        }
        if tex.is_none()
            && let Some(j) = preview_job
        {
            self.ws.frame_jobs.push(j);
        }
        self.edit_overlay(ui, item, r, area);
        self.navigator(ui, tex.as_ref(), natural, r, area);
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
        let (t_zoom, t_actual, t_edge) = (
            "Zoom sizes. Right-click switches between actual size and fit".to_string(),
            tip("Actual Size", "1", C::ZoomActual),
            "Fit to Window Edge (`)".to_string(),
        );
        let mut zoom = self.ws.view.zoom_key.take();
        // Eagle's detail toolbar: the zoom slider and its % label (a menu of
        // sizes; right-click toggles actual size and fit), Actual Size, and
        // the fit-to-edge toggle.
        let mut x = right - 30.0;
        if ui.put(Rect::from_center_size(Pos2::new(x, cy), vec2(40.0, 24.0)), egui::Button::new("Fit").selected(self.image.edge)).on_hover_text(t_edge).clicked() {
            zoom = Some(logic::ViewZoom::Edge);
        }
        x -= 48.0;
        if ui.put(Rect::from_center_size(Pos2::new(x, cy), vec2(40.0, 24.0)), egui::Button::new("1:1")).on_hover_text(t_actual).clicked() {
            zoom = Some(logic::ViewZoom::Actual);
        }
        x -= 50.0;
        let label = ui.put(
            Rect::from_center_size(Pos2::new(x, cy), vec2(50.0, 24.0)),
            egui::Button::new(egui::RichText::new(format!("{:.0}%", scale * 100.0)).font(mono(12.0)).color(t::p().text_value)).frame(false),
        );
        let mut picked = None;
        egui::Popup::menu(&label).show(|ui| {
            ui.set_min_width(150.0);
            for pct in logic::ZOOM_PRESETS {
                if ui.add(egui::Button::new(format!("{pct}%")).selected((scale * 100.0).round() as u32 == pct)).clicked() {
                    picked = Some(pct as f32 / 100.0);
                }
            }
            ui.separator();
            if ui.button("Actual Size").clicked() {
                zoom = Some(logic::ViewZoom::Actual);
            }
            if ui.button("Zoom to Fit").clicked() {
                zoom = Some(logic::ViewZoom::Fit);
            }
        });
        let label = label.on_hover_text(t_zoom);
        if label.secondary_clicked() {
            zoom = Some(if (scale - 1.0).abs() > 1e-3 { logic::ViewZoom::Actual } else { logic::ViewZoom::Fit });
        }
        x -= 25.0;
        let track = Rect::from_min_max(Pos2::new(x - 110.0, cy - 9.0), Pos2::new(x, cy + 9.0));
        if let Some(v) = zoom_slider(ui, track, logic::zoom_slider_pos(scale)) {
            picked = Some(logic::slider_zoom_snapped(v));
        }
        // The GIF frame player's toggle sits left of the slider.
        x = track.left() - 20.0;
        // Sizes from the menu and slider zoom about the view's centre.
        if let Some(new) = picked {
            self.ws.view.pan *= new / scale;
            self.ws.view.zoom = Some(new);
        }
        let tools_x = self.gif_toggle_button(ui, item, x, cy);
        let tools_left = self.edit_bar(ui, item, tools_x, cy);
        media_view::bar_name(ui, item, bar, name_x, tools_left);

        if let Some(z) = zoom {
            self.ws.view.zoom = logic::view_zoom(z, scale);
            match z {
                logic::ViewZoom::Edge => self.image.edge = !self.image.edge,
                logic::ViewZoom::Fit => self.image.edge = false,
                _ => {}
            }
            if matches!(z, logic::ViewZoom::Actual | logic::ViewZoom::Fit | logic::ViewZoom::Edge) {
                self.ws.view.pan = Vec2::ZERO;
            }
        }
        if let Some(j) = job_for(item, lib.root(), Tier::Full) {
            self.ws.frame_jobs.push(j);
        }
        if self.prefs.remember_zoom && !self.image.edge && self.ws.viewer.as_deref() == Some(id.as_str()) {
            self.image.views.insert(id, (self.ws.view.zoom, self.ws.view.pan));
        }
    }

    /// Eagle's navigator: while the picture overflows the view, a small
    /// copy of it in the bottom-right corner with a frame around the part
    /// on screen. Clicking it centres the view there; dragging moves the
    /// frame (and the view) along. Hidden by the Navigator toggle (⌥⌘8).
    fn navigator(&mut self, ui: &Ui, tex: Option<&TextureHandle>, natural: Vec2, r: Rect, area: Rect) {
        let Some(tex) = tex else { return };
        if self.modes.hide_navigator || !logic::navigator_shown((r.width(), r.height()), (area.width(), area.height())) {
            return;
        }
        let side = logic::NAVIGATOR_SIDE;
        let inset = Vec2::splat(logic::NAVIGATOR_INSET + side);
        let boxr = Rect::from_min_size(area.max - inset, Vec2::splat(side));
        let [px, py, pw, ph] = logic::navigator_picture((natural.x, natural.y), side);
        let pic = Rect::from_min_size(boxr.min + vec2(px, py), vec2(pw, ph));
        let [x0, y0, x1, y1] = logic::visible_part([r.min.x, r.min.y, r.max.x, r.max.y], [area.min.x, area.min.y, area.max.x, area.max.y]);
        let frame = Rect::from_min_max(pic.lerp_inside(vec2(x0, y0)), pic.lerp_inside(vec2(x1, y1)));
        let frame = Rect::from_center_size(frame.center(), frame.size().max(Vec2::splat(logic::NAVIGATOR_MIN_FRAME)));

        let resp = ui.interact(boxr, ui.scope_id().with("viewer-navigator"), Sense::click_and_drag());
        if resp.drag_started() || resp.clicked() {
            let at = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            self.image.nav_grab = at.filter(|p| frame.contains(*p)).map_or(Vec2::ZERO, |p| p - frame.center());
        }
        if (resp.dragged() || resp.clicked())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let f = (p - self.image.nav_grab - pic.min) / pic.size();
            let pan = logic::pan_to_show(f.x, f.y, (r.width(), r.height()));
            let (px, py) = logic::clamp_pan(pan, (r.width(), r.height()), (area.width(), area.height()));
            self.ws.view.pan = vec2(px, py);
            self.ws.view.zoom = Some(r.width() / natural.x.max(1.0));
            ui.ctx().request_repaint();
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if frame.contains(resp.hover_pos().unwrap_or_default()) { egui::CursorIcon::Move } else { egui::CursorIcon::Default });
        }

        let p = ui.painter().with_clip_rect(area);
        let round = egui::CornerRadius::same(6);
        p.rect_filled(boxr, round, Color32::from_black_alpha(102));
        p.image(tex.id(), pic, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha(230));
        // The picture outside the frame is dimmed.
        let dim = Color32::from_black_alpha(153);
        for part in [
            Rect::from_min_max(pic.min, Pos2::new(pic.max.x, frame.min.y)),
            Rect::from_min_max(Pos2::new(pic.min.x, frame.max.y), pic.max),
            Rect::from_min_max(Pos2::new(pic.min.x, frame.min.y), Pos2::new(frame.min.x, frame.max.y)),
            Rect::from_min_max(Pos2::new(frame.max.x, frame.min.y), Pos2::new(pic.max.x, frame.max.y)),
        ] {
            let part = part.intersect(pic);
            if part.is_positive() {
                p.rect_filled(part, 0.0, dim);
            }
        }
        let frame_round = (frame.size().min_elem() / 2.0).min(6.0);
        p.rect_stroke(frame, frame_round, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
        p.rect_stroke(boxr, round, egui::Stroke::new(1.0, Color32::from_white_alpha(38)), egui::StrokeKind::Inside);
    }

    /// Eagle's "too large to preview" page for TIFF, HEIC and TGA images
    /// over its pixel limit: the thumbnail, a line giving the limit, and
    /// the Open With button. No full render is attempted.
    fn too_large(&mut self, ui: &mut Ui, lib: &Library, item: &Item, bar: Rect, area: Rect) {
        let resp = ui.interact(area, ui.scope_id().with("viewer-canvas"), Sense::click());
        resp.context_menu(|ui| {
            self.item_menu(ui, lib, item, std::slice::from_ref(&item.id));
        });
        if resp.double_clicked() {
            self.ws.viewer = None;
        }
        let thumb = Rect::from_center_size(area.center() - vec2(0.0, 70.0), Vec2::splat(150.0));
        match self.ws.loader.get(&item.id, Tier::Preview).cloned() {
            Some(tex) => {
                let size = tex.size_vec2() * (thumb.width() / tex.size_vec2().max_elem());
                egui::Image::new((tex.id(), size)).paint_at(ui, Rect::from_center_size(thumb.center(), size));
            }
            None => ext_placeholder(ui, thumb, &item.ext),
        }
        if let Some(j) = job_for(item, lib.root(), Tier::Preview) {
            self.ws.frame_jobs.push(j);
        }
        let megapixels = item.width.saturating_mul(item.height) / 1_000_000;
        let line = format!(
            "Falcon previews TIFF, HEIC and TGA images of up to {} megapixels; this one has {megapixels}.",
            logic::TOO_LARGE_PIXELS / 1_000_000
        );
        self.notice_lines(ui, area, "Too large to preview", &line);
        let (name_x, right) = media_view::bar_shell(self, ui, item, bar);
        media_view::bar_name(ui, item, bar, name_x, right);
    }
}

/// The viewer's zoom slider over `track` at position `pos` (see
/// `logic::SLIDER_RANGE`); returns the position dragged or clicked to.
fn zoom_slider(ui: &mut Ui, track: Rect, pos: f32) -> Option<f32> {
    let (lo, hi) = logic::SLIDER_RANGE;
    let resp = ui.interact(track, ui.scope_id().with("viewer-zoom-slider"), Sense::click_and_drag());
    let picked = resp
        .interact_pointer_pos()
        .filter(|_| resp.dragged() || resp.clicked())
        .map(|p| lo + ((p.x - track.left()) / track.width()).clamp(0.0, 1.0) * (hi - lo));
    let f = ((picked.unwrap_or(pos) - lo) / (hi - lo)).clamp(0.0, 1.0);
    let p = ui.painter();
    let y = track.center().y;
    p.line_segment([Pos2::new(track.left(), y), Pos2::new(track.right(), y)], egui::Stroke::new(2.0, t::p().divider));
    let kx = track.left() + f * track.width();
    p.line_segment([Pos2::new(track.left(), y), Pos2::new(kx, y)], egui::Stroke::new(2.0, t::p().text_muted));
    p.circle_filled(Pos2::new(kx, y), 6.0, Color32::WHITE);
    picked
}

/// Draws texture `tex` stretched over `r`, except inside `hole`: the bands
/// above, below, left and right of it, each with its part of the texture.
fn paint_except(painter: &egui::Painter, tex: egui::TextureId, r: Rect, hole: Rect) {
    let hole = hole.intersect(r);
    let bands = [
        Rect::from_min_max(r.min, Pos2::new(r.max.x, hole.min.y)),
        Rect::from_min_max(Pos2::new(r.min.x, hole.max.y), r.max),
        Rect::from_min_max(Pos2::new(r.min.x, hole.min.y), Pos2::new(hole.min.x, hole.max.y)),
        Rect::from_min_max(Pos2::new(hole.max.x, hole.min.y), Pos2::new(r.max.x, hole.max.y)),
    ];
    let uv = |p: Pos2| ((p - r.min) / r.size()).to_pos2();
    for band in bands.into_iter().filter(|b| b.width() > 0.0 && b.height() > 0.0) {
        painter.image(tex, band, Rect::from_min_max(uv(band.min), uv(band.max)), Color32::WHITE);
    }
}
