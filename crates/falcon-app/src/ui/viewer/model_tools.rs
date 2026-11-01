//! The 3D viewer's tools after the ones in Eagle's model viewer: the model
//! tree navigator (the file, its meshes with hide, isolate and fit, and its
//! materials), the measure tool (two points picked on the surface, their
//! distance, the angle between the faces and the distance between parallel
//! faces), Export (GLB, glTF, OBJ, PLY or STL of the visible meshes) and Create Snapshot (a
//! PNG of the current view). The scene itself is `model.rs`; the geometry
//! (writers, picking, the snapshot rasterizer) is `falcon_library::model3d`.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, vec2};
use falcon_library::model::Item;
use falcon_library::model3d::export::{self, Format};
use falcon_library::model3d::{math, raster};

use crate::app::App;
use crate::icons as ic;
use crate::locale::{tr, trf};
use crate::logic;
use crate::model3d::{ModelView, Settings, ViewerTheme};
use crate::theme::{self as t, font, mono};
use crate::ui::widgets::{galley, icon};

const TREE_WIDTH: f32 = 240.0;
const ROW: f32 = 22.0;

/// The colours of the viewer's panels for its theme: (panel, text, muted,
/// divider).
pub(crate) fn panel_colors(theme: ViewerTheme) -> (Color32, Color32, Color32, Color32) {
    match theme {
        ViewerTheme::Auto => (t::p().panel.gamma_multiply(0.92), t::p().text_value, t::p().text_muted, t::p().divider),
        ViewerTheme::Light => (Color32::from_rgba_unmultiplied(250, 250, 250, 235), Color32::from_gray(40), Color32::from_gray(120), Color32::from_gray(220)),
        ViewerTheme::Dark => (Color32::from_rgba_unmultiplied(36, 37, 40, 235), Color32::from_gray(225), Color32::from_gray(150), Color32::from_gray(64)),
    }
}

/// A small icon button at `center` with its own id (rows share x).
fn row_button(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, center: Pos2, glyph: &str, color: Color32, tip: &str) -> egui::Response {
    let rect = Rect::from_center_size(center, vec2(20.0, 20.0));
    let resp = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), color.gamma_multiply(0.15));
    }
    icon(ui, center, glyph, 13.0, if resp.hovered() { color } else { color.gamma_multiply(0.8) });
    resp.on_hover_text(tip)
}

/// What the tree asked for, applied once it is drawn.
enum TreeAct {
    Toggle(usize),
    Isolate(usize),
    Fit(usize),
    Material(String),
    ShowAll,
}

impl App {
    /// The model tree navigator over the left of the scene.
    pub(crate) fn model_tree(&mut self, ui: &mut Ui, item: &Item, area: Rect, aspect: f32) {
        let theme = self.model3d.settings.theme;
        let Some(view) = self.model3d.view.as_ref() else { return };
        let Some(model) = view.model.clone() else { return };
        let (panel, text, muted, divider) = panel_colors(theme);
        let rect = Rect::from_min_max(area.min + vec2(12.0, 12.0), Pos2::new((area.left() + 12.0 + TREE_WIDTH).min(area.right() - 12.0), area.bottom() - 12.0));
        if rect.width() < 120.0 || rect.height() < 80.0 {
            return;
        }
        ui.painter().rect_filled(rect, CornerRadius::same(6), panel);
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, divider), egui::StrokeKind::Inside);
        // The panel takes the pointer: nothing behind it orbits.
        let _ = ui.interact(rect, ui.scope_id().with("model-tree-bg"), Sense::click_and_drag());
        let mut materials: Vec<(String, [f32; 3])> = Vec::new();
        for m in &model.meshes {
            if let Some(mat) = &m.material
                && !materials.iter().any(|(n, _)| *n == mat.name)
            {
                materials.push((mat.name.clone(), mat.color));
            }
        }
        let hidden: Vec<bool> = (0..model.meshes.len()).map(|k| view.is_hidden(k)).collect();
        let mut act = None;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)));
        child.set_clip_rect(rect.shrink(4.0));
        egui::ScrollArea::vertical().id_salt("model-tree").auto_shrink([false, false]).show(&mut child, |ui| {
            let w = ui.available_width();
            let heading = |ui: &mut Ui, label: &str| {
                let (r, _) = ui.allocate_exact_size(vec2(w, ROW), Sense::hover());
                ui.painter().text(Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER, label, font(11.5), muted);
            };
            heading(ui, tr("Files"));
            let (r, _) = ui.allocate_exact_size(vec2(w, ROW), Sense::hover());
            icon(ui, Pos2::new(r.left() + 8.0, r.center().y), ic::FILE, 13.0, text);
            let g = galley(ui, &format!("{}.{}", item.name, item.ext), font(12.0), text, w - 24.0);
            ui.painter().galley(Pos2::new(r.left() + 22.0, r.center().y - g.size().y / 2.0), g, text);
            ui.add_space(6.0);

            heading(ui, tr("Meshes"));
            for (k, mesh) in model.meshes.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(w, ROW), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::same(4), text.gamma_multiply(0.08));
                }
                let shown = !hidden[k];
                let eye = row_button(ui, ("tree-eye", k), Pos2::new(r.left() + 10.0, r.center().y), ic::EYE, if shown { text } else { muted.gamma_multiply(0.5) }, if shown { tr("Hide") } else { tr("Show") });
                if eye.clicked() {
                    act = Some(TreeAct::Toggle(k));
                }
                let name = if mesh.name.trim().is_empty() { trf("Mesh {n}", &[("n", &(k + 1))]) } else { mesh.name.clone() };
                let ink = if shown { text } else { muted };
                let g = galley(ui, &name, font(12.0), ink, w - 76.0);
                ui.painter().galley(Pos2::new(r.left() + 24.0, r.center().y - g.size().y / 2.0), g, ink);
                if row_button(ui, ("tree-fit", k), Pos2::new(r.right() - 32.0, r.center().y), ic::CORNERS_IN, text, tr("Fit Mesh to Window")).clicked() {
                    act = Some(TreeAct::Fit(k));
                }
                if row_button(ui, ("tree-iso", k), Pos2::new(r.right() - 10.0, r.center().y), ic::FUNNEL, text, tr("Isolate (again to show all)")).clicked() {
                    act = Some(TreeAct::Isolate(k));
                }
                if resp.double_clicked() {
                    act = Some(TreeAct::Fit(k));
                }
            }
            if hidden.iter().any(|&h| h) && ui.add(egui::Button::new(egui::RichText::new(tr("Show All")).color(text)).small()).clicked() {
                act = Some(TreeAct::ShowAll);
            }
            if !materials.is_empty() {
                ui.add_space(6.0);
                heading(ui, tr("Materials"));
                for (name, c) in &materials {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, ROW), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), text.gamma_multiply(0.08));
                    }
                    let sw = Rect::from_center_size(Pos2::new(r.left() + 10.0, r.center().y), vec2(11.0, 11.0));
                    let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
                    ui.painter().rect_filled(sw, CornerRadius::same(2), col);
                    let label = if name.is_empty() { tr("No Name") } else { name.as_str() };
                    let g = galley(ui, label, font(12.0), text, w - 30.0);
                    ui.painter().galley(Pos2::new(r.left() + 24.0, r.center().y - g.size().y / 2.0), g, text);
                    if resp.on_hover_text(tr("Show only the meshes with this material (again to show all)")).clicked() {
                        act = Some(TreeAct::Material(name.clone()));
                    }
                }
            }
        });
        let Some(act) = act else { return };
        let Some(view) = self.model3d.view.as_mut() else { return };
        match act {
            TreeAct::Toggle(k) => {
                let h = view.is_hidden(k);
                view.set_hidden(k, !h);
            }
            TreeAct::Isolate(k) => view.isolate(|i| i == k),
            TreeAct::Fit(k) => view.fit_meshes(&[k], aspect),
            TreeAct::Material(name) => view.isolate(|i| model.meshes[i].material.as_ref().is_some_and(|m| m.name == name)),
            TreeAct::ShowAll => view.isolate(|_| true),
        }
    }

    /// The measure tool: a click on the model picks a point (a third starts
    /// over, a click off the model clears); the points, the line between
    /// them and the readout are drawn over the scene.
    pub(crate) fn model_measure(&mut self, ui: &mut Ui, resp: &egui::Response, area: Rect, vp: &math::M4, settings: &Settings) {
        let Some(view) = self.model3d.view.as_mut() else { return };
        let aspect = (area.width() / area.height().max(1.0)).max(1e-3);
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let (x, y) = ((p.x - area.left()) / area.width() * 2.0 - 1.0, 1.0 - (p.y - area.top()) / area.height() * 2.0);
            let (o, d) = view.camera.ray(settings.projection, aspect, x, y);
            let hit = view.model.as_ref().and_then(|m| export::pick(&m.meshes, |k| !view.hidden.get(k).copied().unwrap_or(false), o, d));
            match hit {
                Some(h) => {
                    if view.picks.len() >= 2 {
                        view.picks.clear();
                    }
                    view.picks.push(h);
                }
                None => view.picks.clear(),
            }
        }
        let rect = [area.min.x, area.min.y, area.max.x, area.max.y];
        let pts: Vec<Option<Pos2>> = view.picks.iter().map(|h| logic::screen_point(vp, h.point, rect).map(|[x, y]| Pos2::new(x, y))).collect();
        let p = ui.painter().with_clip_rect(area);
        let accent = t::p().primary;
        if let [Some(a), Some(b)] = pts[..] {
            p.line_segment([a, b], Stroke::new(2.0, accent));
        }
        for pt in pts.iter().flatten() {
            p.circle(*pt, 5.0, Color32::WHITE, Stroke::new(2.0, accent));
        }
        // The readout, bottom left.
        let (panel, text, muted, divider) = panel_colors(settings.theme);
        let lines: Vec<(String, String)> = match &view.picks[..] {
            [] => vec![(tr("Measure").into(), tr("Click a point on the model").into())],
            [_] => vec![(tr("Measure").into(), tr("Click a second point").into())],
            [a, b, ..] => {
                let m = export::measure(a, b);
                let mut rows = vec![(tr("Distance").to_string(), logic::detail_number(f64::from(m.distance))), (tr("Angle").to_string(), format!("{:.1}°", m.angle))];
                if let Some(d) = m.parallel {
                    rows.push((tr("Parallel faces").to_string(), logic::detail_number(f64::from(d))));
                }
                rows
            }
        };
        let h = 12.0 + 18.0 * lines.len() as f32;
        let r = Rect::from_min_size(Pos2::new(area.left() + 12.0, area.bottom() - 12.0 - h), vec2(230.0, h));
        p.rect_filled(r, CornerRadius::same(6), panel);
        p.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, divider), egui::StrokeKind::Inside);
        for (i, (k, v)) in lines.iter().enumerate() {
            let y = r.top() + 6.0 + 9.0 + 18.0 * i as f32;
            p.text(Pos2::new(r.left() + 10.0, y), Align2::LEFT_CENTER, k, font(12.0), muted);
            p.text(Pos2::new(r.right() - 10.0, y), Align2::RIGHT_CENTER, v, mono(12.0), text);
        }
    }

    /// Writes the visible meshes in `format` where the user picks.
    pub(crate) fn model_export(&mut self, item: &Item, format: Format) {
        let Some(view) = self.model3d.view.as_ref() else { return };
        let ext = format.ext();
        let name = format!("{}.{ext}", item.name);
        let Some(dest) = rfd::FileDialog::new().set_file_name(name).add_filter(ext.to_uppercase(), &[ext]).save_file() else { return };
        match export::write(view.visible_meshes(), format, &dest) {
            Ok(()) => self.flash_brief(tr("Model exported")),
            Err(e) => self.flash_error(trf("Couldn't export the model: {error}", &[("error", &e)])),
        }
    }

    /// Saves a PNG of the current view at the view's pixel size, drawn by
    /// the software rasterizer (faces only: edges, grid and axes are left
    /// out), on the background when one is set or transparent otherwise.
    pub(crate) fn model_snapshot(&mut self, item: &Item, size: [u32; 2], aspect: f32) {
        let settings = self.model3d.settings.clone();
        let Some(view) = self.model3d.view.as_ref() else { return };
        let Some(dest) = rfd::FileDialog::new().set_file_name(format!("{}.png", item.name)).add_filter("PNG", &["png"]).save_file() else { return };
        let vp = snapshot_matrix(view, &settings, aspect);
        let default = settings.default_color.map(|c| f32::from(c) / 255.0);
        let mut img = raster::render_view(view.visible_meshes(), size[0], size[1], &vp, view.camera.light_direction(), default);
        if let Some(bg) = settings.effective_background() {
            for px in img.pixels_mut() {
                let a = f32::from(px[3]) / 255.0;
                for c in 0..3 {
                    px[c] = (f32::from(px[c]) * a + f32::from(bg[c]) * (1.0 - a)).round() as u8;
                }
                px[3] = 255;
            }
        }
        match img.save(&dest) {
            Ok(()) => self.flash_brief(tr("Snapshot saved")),
            Err(e) => self.flash_error(trf("Couldn't save the snapshot: {error}", &[("error", &e)])),
        }
    }
}

/// The camera matrix of the view as drawn.
fn snapshot_matrix(view: &ModelView, settings: &Settings, aspect: f32) -> math::M4 {
    let proj = view.camera.projection(settings.projection, aspect, view.radius());
    math::mul(&proj, &view.camera.view())
}
