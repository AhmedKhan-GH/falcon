//! The viewer for 3D models (obj, stl, ply, glb, gltf, 3mf), after the
//! model viewer Eagle embeds: the model lit from the camera on the
//! viewer's background, turned with the mouse (left drag orbits, right or
//! middle drag or ⇧ pans, ⌃ drag and the wheel zoom), fitted when it
//! opens, with buttons in the bar to fit it again, show edges, a grid and
//! axes, the details panel and a menu of display settings. The scene is
//! drawn with wgpu (`model3d/gpu.rs`); the camera maths are in
//! `logic/viewer3d.rs`. Models falcon can't read, and other 3D formats
//! (fbx, 3ds, dae, ifc), show their still.

use eframe::egui::{self, Align2, Color32, CornerRadius, PointerButton, Pos2, Rect, Sense, Stroke, Ui, vec2};
use falcon_library::Library;
use falcon_library::model3d::{self as m3d, math};
use falcon_library::model3d::geom::Stats;
use falcon_library::model::Item;

use crate::app::App;
use crate::icons as ic;
use crate::locale::{tr, tr_key};
use crate::logic::{self, DragAction, Projection, UpAxis, ViewZoom};
use crate::model3d::gpu::SceneCallback;
use crate::model3d::{ModelView, Settings, Shading, ViewerTheme};
use crate::theme::{self as t, font, mono};
use crate::ui::viewer::audio::{bar_name, bar_shell};
use crate::ui::widgets::{galley, icon_button};

const PANEL_WIDTH: f32 = 230.0;

/// A word in the viewer bar that toggles a setting: the accent colour
/// while on. Returns the response and the x left of it.
fn bar_toggle(ui: &mut Ui, right: f32, cy: f32, label: &str, on: bool, tip: &str) -> (egui::Response, f32) {
    let g = galley(ui, label, font(12.5), t::p().text, 120.0);
    let w = g.size().x + 12.0;
    let rect = Rect::from_center_size(Pos2::new(right - w / 2.0, cy), vec2(w, 22.0));
    let resp = ui.interact(rect, ui.scope_id().with(("model-toggle", label)), Sense::click()).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), t::p().button_hover);
    }
    let color = if on { t::p().primary } else if resp.hovered() { t::p().text } else { t::p().text_value };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font(12.5), color);
    (resp, rect.left() - 4.0)
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The details Eagle's viewer lists, as (label, value) rows.
pub(crate) fn detail_rows(s: &Stats) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        (tr_key("Vertices"), s.vertices.to_string()),
        (tr_key("Triangles"), s.triangles.to_string()),
        (tr_key("Size X"), logic::detail_number(f64::from(s.size[0]))),
        (tr_key("Size Y"), logic::detail_number(f64::from(s.size[1]))),
        (tr_key("Size Z"), logic::detail_number(f64::from(s.size[2]))),
    ];
    if let Some(v) = s.volume {
        rows.push((tr_key("Volume"), logic::detail_number(v)));
    }
    rows.push((tr_key("Surface"), logic::detail_number(s.surface)));
    rows
}

impl App {
    /// Draws the viewer for a 3D model falcon can read and returns true;
    /// false for anything else, for a model that failed to read, or when
    /// the window doesn't draw with wgpu (the still is shown then).
    pub(crate) fn model_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        if self.model3d.target.is_none() || !m3d::is_supported(&item.ext) {
            return false;
        }
        let ctx = parent.ctx().clone();
        if self.model3d.view.as_ref().is_none_or(|v| v.id != item.id) {
            let path = item.original_path(lib.root());
            self.model3d.view = Some(ModelView::open(&ctx, &item.id, path, &item.ext, self.model3d.settings.up));
        }
        let view = self.model3d.view.as_mut().expect("opened above");
        let loading = view.poll();
        if view.error.is_some() {
            return false;
        }
        let ui = &mut crate::ui::widgets::viewer_ui(parent, screen, "model-viewer");
        let bg = self.model3d.settings.effective_background().map_or(t::p().main, rgb);
        ui.painter().rect_filled(screen, 0.0, bg);
        let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
        let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
        if loading {
            ui.painter().text(area.center(), Align2::CENTER_CENTER, tr("Loading model…"), font(13.0), t::p().text_muted);
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            self.model_scene(ui, lib, item, area);
        }
        self.model_bar(ui, item, bar, area, !loading);
        true
    }

    /// The model, its navigation and the details panel.
    fn model_scene(&mut self, ui: &mut Ui, lib: &Library, item: &Item, area: Rect) {
        let ctx = ui.ctx().clone();
        let Some(target) = self.model3d.target else { return };
        let settings = self.model3d.settings.clone();
        let zoom_key = self.ws.view.zoom_key.take();
        let Some(view) = self.model3d.view.as_mut() else { return };
        let aspect = (area.width() / area.height().max(1.0)).max(1e-3);
        if view.needs_fit {
            view.fit(aspect);
        }
        // ⌘= and ⌘− zoom a wheel step; ⌘0 and ⌘9 fit.
        match zoom_key {
            Some(ViewZoom::In) => view.camera.zoom(logic::WHEEL_STEP),
            Some(ViewZoom::Out) => view.camera.zoom(-logic::WHEEL_STEP),
            Some(ViewZoom::Fit | ViewZoom::Actual) => view.fit(aspect),
            _ => {}
        }

        let resp = ui.interact(area, ui.scope_id().with("model-area"), Sense::click_and_drag());
        if resp.dragged() {
            let d = resp.drag_delta();
            let mods = ctx.input(|i| i.modifiers);
            let primary = resp.dragged_by(PointerButton::Primary);
            let other = resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle);
            match logic::drag_action(primary, other, mods.ctrl, mods.shift) {
                Some(DragAction::Orbit) => view.camera.orbit(d.x, d.y, settings.fixed_up),
                Some(DragAction::Pan) => view.camera.pan(d.x, d.y),
                Some(DragAction::Zoom) => view.camera.zoom(-d.y * logic::ZOOM_PER_POINT),
                None => {}
            }
        }
        if resp.hovered() {
            let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if pinch != 1.0 {
                view.camera.zoom(logic::pinch_zoom(pinch));
            } else if scroll != 0.0 {
                view.camera.zoom(logic::wheel_zoom(scroll));
            }
        }

        // The scene.
        let ppp = ctx.pixels_per_point();
        let size = [(area.width() * ppp).round().max(1.0) as u32, (area.height() * ppp).round().max(1.0) as u32];
        let radius = view.radius();
        let proj = view.camera.projection(settings.projection, aspect, radius);
        let view_proj = math::mul(&proj, &view.camera.view());
        let bg = settings.effective_background().unwrap_or_else(|| {
            let c = t::p().main;
            [c.r(), c.g(), c.b()]
        });
        let clear = settings.effective_background().map_or([0.0; 4], |c| [f64::from(c[0]) / 255.0, f64::from(c[1]) / 255.0, f64::from(c[2]) / 255.0, 1.0]);
        let callback = SceneCallback {
            slot: u64::from(self.preview_drawing.is_some()),
            target,
            size,
            clear,
            view_proj,
            light: view.camera.light_direction(),
            mesh: view.mesh(&settings),
            lines: if settings.edges || settings.grid || settings.axes { view.lines(&settings, bg) } else { None },
            edges: settings.edges,
            grid: settings.grid,
            axes: settings.axes,
        };
        ui.painter().add(callback.shape(area));

        // Right click without a drag opens the item menu, as on the other
        // viewers; in a preview window, the window's own menu (it takes the
        // scene's response).
        if self.preview_drawing.is_some() {
            self.model3d.scene_resp = Some(resp.clone());
        } else {
            resp.context_menu(|ui| {
                self.item_menu(ui, lib, item, std::slice::from_ref(&item.id));
            });
        }
        if settings.measuring {
            self.model_measure(ui, &resp, area, &view_proj, &settings);
        } else if let Some(v) = self.model3d.view.as_mut() {
            v.picks.clear();
        }
        if self.model3d.settings.info {
            self.model_details(ui, area);
        }
        if self.model3d.settings.tree {
            self.model_tree(ui, item, area, aspect);
        }
    }

    /// Eagle's Details panel, over the top right of the scene.
    fn model_details(&self, ui: &mut Ui, area: Rect) {
        let Some(view) = self.model3d.view.as_ref() else { return };
        let Some(stats) = &view.stats else { return };
        let rows = detail_rows(stats);
        // Materials by name, once each, with their colour.
        let mut mats: Vec<(String, [f32; 3])> = Vec::new();
        for m in view.model.iter().flat_map(|m| m.meshes.iter()) {
            if let Some(mat) = &m.material
                && !mats.iter().any(|(n, _)| *n == mat.name)
            {
                mats.push((mat.name.clone(), mat.color));
            }
        }
        let shown = mats.len().min(12);
        let line = 18.0;
        let h = 16.0 + line * (rows.len() as f32 + if shown > 0 { shown as f32 + 1.5 } else { 0.0 });
        let rect = Rect::from_min_size(Pos2::new(area.right() - PANEL_WIDTH - 12.0, area.top() + 12.0), vec2(PANEL_WIDTH, h));
        if rect.left() < area.left() + 12.0 || rect.bottom() > area.bottom() {
            return;
        }
        let (panel, text, muted, divider) = crate::ui::viewer::model_tools::panel_colors(self.model3d.settings.theme);
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(6), panel);
        p.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, divider), egui::StrokeKind::Inside);
        let mut y = rect.top() + 8.0 + line / 2.0;
        for (k, v) in rows {
            p.text(Pos2::new(rect.left() + 12.0, y), Align2::LEFT_CENTER, tr(k), font(12.0), muted);
            p.text(Pos2::new(rect.right() - 12.0, y), Align2::RIGHT_CENTER, v, mono(12.0), text);
            y += line;
        }
        if shown > 0 {
            y += line * 0.5;
            p.text(Pos2::new(rect.left() + 12.0, y), Align2::LEFT_CENTER, tr("Materials"), font(12.0), muted);
            for (name, c) in mats.iter().take(shown) {
                y += line;
                let sw = Rect::from_center_size(Pos2::new(rect.left() + 18.0, y), vec2(10.0, 10.0));
                let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
                p.rect_filled(sw, CornerRadius::same(2), col);
                let name = if name.is_empty() { tr("No Name") } else { name.as_str() };
                let g = galley(ui, name, font(12.0), text, PANEL_WIDTH - 44.0);
                p.galley(Pos2::new(rect.left() + 30.0, y - g.size().y / 2.0), g, text);
            }
        }
    }

    /// The shared viewer bar plus the model's buttons: fit, edges, grid,
    /// axes, details and the settings menu.
    fn model_bar(&mut self, ui: &mut Ui, item: &Item, bar: Rect, area: Rect, ready: bool) {
        let (left, mut right) = bar_shell(self, ui, item, bar);
        let cy = bar.center().y;
        let aspect = (area.width() / area.height().max(1.0)).max(1e-3);
        if ready {
            let gear = icon_button(ui, Pos2::new(right - 13.0, cy), ic::GEAR, 16.0, true, tr("Display Settings"));
            egui::Popup::menu(&gear).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(230.0);
                self.model_settings_menu(ui, aspect);
            });
            right -= 30.0;
            // Create Snapshot and Export, as in Eagle's viewer toolbar.
            if icon_button(ui, Pos2::new(right - 13.0, cy), ic::CAMERA, 16.0, true, tr("Create Snapshot (PNG of the view)")).clicked() {
                let ppp = ui.ctx().pixels_per_point();
                let size = [(area.width() * ppp).round().max(1.0) as u32, (area.height() * ppp).round().max(1.0) as u32];
                self.model_snapshot(item, size, aspect);
            }
            right -= 30.0;
            let export = icon_button(ui, Pos2::new(right - 13.0, cy), ic::EXPORT, 16.0, true, tr("Export Model"));
            let mut format = None;
            egui::Popup::menu(&export).show(|ui| {
                ui.set_min_width(170.0);
                if ui.button(tr("Export as OBJ…")).clicked() {
                    format = Some(m3d::export::Format::Obj);
                }
                if ui.button(tr("Export as STL…")).clicked() {
                    format = Some(m3d::export::Format::Stl);
                }
            });
            if let Some(f) = format {
                self.model_export(item, f);
            }
            right -= 30.0;
            let s = &mut self.model3d.settings;
            let (r, x) = bar_toggle(ui, right, cy, tr("Measure"), s.measuring, tr("Measure: click two points on the model"));
            if r.clicked() {
                s.measuring = !s.measuring;
            }
            let (r, x) = bar_toggle(ui, x, cy, tr("Tree"), s.tree, tr("Show the files, meshes and materials"));
            if r.clicked() {
                s.tree = !s.tree;
            }
            let (r, x) = bar_toggle(ui, x, cy, tr("model::Details"), s.info, tr("Show the model's details"));
            if r.clicked() {
                s.info = !s.info;
            }
            let (r, x) = bar_toggle(ui, x, cy, tr("model::Axes"), s.axes, tr("Show the X, Y and Z axes"));
            if r.clicked() {
                s.axes = !s.axes;
            }
            let (r, x) = bar_toggle(ui, x, cy, tr("model::Grid"), s.grid, tr("Show a grid under the model"));
            if r.clicked() {
                s.grid = !s.grid;
            }
            let (r, x) = bar_toggle(ui, x, cy, tr("model::Edges"), s.edges, tr("Show edges"));
            if r.clicked() {
                s.edges = !s.edges;
            }
            if icon_button(ui, Pos2::new(x - 13.0, cy), ic::CORNERS_IN, 16.0, true, tr("Fit Model to Window (⌘0)")).clicked()
                && let Some(v) = self.model3d.view.as_mut()
            {
                v.fit(aspect);
            }
            right = x - 30.0;
        }
        bar_name(ui, item, bar, left, right);
    }

    /// The settings menu: shading, camera, up vector, navigation, edges and
    /// colours, after Eagle's viewer toolbar and settings panel.
    fn model_settings_menu(&mut self, ui: &mut Ui, aspect: f32) {
        let s = &mut self.model3d.settings;
        let before = s.clone();
        ui.label(egui::RichText::new(tr("Shading")).color(t::p().text_muted));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.shading, Shading::Auto, tr("model::Auto"));
            ui.selectable_value(&mut s.shading, Shading::Flat, tr("model::Flat"));
            ui.selectable_value(&mut s.shading, Shading::Smooth, tr("model::Smooth"));
        });
        ui.label(egui::RichText::new(tr("model::Camera")).color(t::p().text_muted));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.projection, Projection::Perspective, tr("Perspective"));
            ui.selectable_value(&mut s.projection, Projection::Orthographic, tr("Orthographic"));
        });
        ui.label(egui::RichText::new(tr("Up Vector")).color(t::p().text_muted));
        let mut flip = false;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.up, UpAxis::Y, tr("Y Axis"));
            ui.selectable_value(&mut s.up, UpAxis::Z, tr("Z Axis"));
            flip = ui.button(tr("Flip")).clicked();
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.fixed_up, true, tr("Fixed Up Vector"));
            ui.selectable_value(&mut s.fixed_up, false, tr("Free Orbit"));
        });
        ui.separator();
        ui.checkbox(&mut s.edges, tr("Show Edges"));
        ui.add_enabled_ui(s.edges, |ui| {
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut s.edge_color);
                ui.add(egui::Slider::new(&mut s.edge_threshold, 0.0..=90.0).suffix("°").text(tr("Angle")));
            });
        });
        ui.separator();
        let mut custom = s.background.is_some();
        ui.horizontal(|ui| {
            ui.checkbox(&mut custom, tr("Background Colour"));
            let mut c = s.background.unwrap_or([42, 43, 46]);
            if custom && ui.color_edit_button_srgb(&mut c).changed() {
                s.background = Some(c);
            }
            if custom && s.background.is_none() {
                s.background = Some(c);
            }
        });
        if !custom {
            s.background = None;
        }
        ui.horizontal(|ui| {
            ui.color_edit_button_srgb(&mut s.default_color);
            ui.label(tr("Default Colour"));
        });
        ui.label(egui::RichText::new(tr("Theme")).color(t::p().text_muted));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.theme, ViewerTheme::Auto, tr("Auto"));
            ui.selectable_value(&mut s.theme, ViewerTheme::Light, tr("Light"));
            ui.selectable_value(&mut s.theme, ViewerTheme::Dark, tr("Dark"));
        });
        ui.separator();
        let reset = ui.button(tr("Reset to Default")).clicked();
        if reset {
            *s = Settings { info: s.info, tree: s.tree, measuring: s.measuring, ..Settings::default() };
        }
        let up_changed = s.up != before.up;
        if let Some(v) = self.model3d.view.as_mut() {
            if up_changed || (reset && before.up != UpAxis::Y) {
                v.home(self.model3d.settings.up, aspect);
            } else if flip {
                v.camera = v.camera.flipped();
            }
            if before.fixed_up != self.model3d.settings.fixed_up && self.model3d.settings.fixed_up {
                // Back to a fixed up vector: stand the camera up again.
                v.camera.up = self.model3d.settings.up.vector();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_list_volume_only_when_closed() {
        let mut s = Stats { vertices: 8, triangles: 12, size: [1.0, 2.0, 0.5], volume: Some(1.0), surface: 6.0 };
        let rows = detail_rows(&s);
        let labels: Vec<_> = rows.iter().map(|r| r.0).collect();
        assert_eq!(labels, ["Vertices", "Triangles", "Size X", "Size Y", "Size Z", "Volume", "Surface"]);
        assert_eq!(rows[3].1, "2");
        assert_eq!(rows[4].1, "0.5");
        s.volume = None;
        assert!(!detail_rows(&s).iter().any(|r| r.0 == "Volume"));
    }
}
