//! Combine Images (`falcon_library::combine`): the dialog with the layout,
//! gap and background and a live preview from the items' saved sizes and
//! grid thumbnails, then the combine itself behind the progress modal. The
//! new PNG joins the open folder and is selected; the sources stay as they
//! are, and one undo step takes the new item back to the trash.

use eframe::egui::{self, Color32, Rect, RichText, Sense, TextureHandle, Ui, pos2, vec2};
use falcon_library::Library;
use falcon_library::combine::{self, Layout, Options, Plan};
use std::path::PathBuf;

use crate::app::{App, image_edit, lock};
use crate::loader::Tier;
use crate::locale::{tr, tr_n, trf};
use crate::logic::{self, Scope};
use crate::theme as t;
use crate::ui::dialogs::progress::Outcome;

const PREVIEW: egui::Vec2 = vec2(360.0, 180.0);
const MAX_GAP: u32 = 1000;
const MAX_COLUMNS: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrange {
    Horizontal,
    Vertical,
    Grid,
}

/// The dialog's settings, kept for the next time it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Choice {
    arrange: Arrange,
    columns: u32,
    gap: u32,
    transparent: bool,
    colour: [u8; 3],
}

impl Default for Choice {
    fn default() -> Self {
        Choice { arrange: Arrange::Horizontal, columns: 3, gap: 0, transparent: false, colour: [255, 255, 255] }
    }
}

impl Choice {
    fn options(self) -> Options {
        let layout = match self.arrange {
            Arrange::Horizontal => Layout::Horizontal,
            Arrange::Vertical => Layout::Vertical,
            Arrange::Grid => Layout::Grid(self.columns),
        };
        Options { layout, gap: self.gap, background: (!self.transparent).then_some(self.colour) }
    }
}

/// One item to combine and its saved size, for the preview.
#[derive(Clone)]
struct Source {
    id: String,
    size: (u32, u32),
}

/// The dialog's state: the items while it is open, and the last settings.
#[derive(Default)]
pub(crate) struct State {
    sources: Option<Vec<Source>>,
    choice: Choice,
}

impl State {
    pub(super) fn is_open(&self) -> bool {
        self.sources.is_some()
    }
}

/// The items among `targets` falcon can combine.
fn combinable(lib: &Library, targets: &[String]) -> Vec<String> {
    image_edit::editable(targets, |id| lib.item(id).map(|i| i.ext.clone()), combine::can_combine)
}

/// The side of a saved size, at least 1 so a missing size can't stop the preview.
fn side(px: u64) -> u32 {
    px.clamp(1, u32::MAX as u64) as u32
}

/// Draws the plan at a size that fits, with thumbnails where they are loaded.
fn preview(ui: &mut Ui, plan: &Plan, background: Option<[u8; 3]>, textures: &[Option<TextureHandle>]) {
    let (outer, _) = ui.allocate_exact_size(PREVIEW, Sense::hover());
    let k = (PREVIEW.x / plan.width as f32).min(PREVIEW.y / plan.height as f32);
    let rect = Rect::from_center_size(outer.center(), vec2(plan.width as f32 * k, plan.height as f32 * k));
    let p = ui.painter_at(outer);
    match background {
        Some([r, g, b]) => {
            p.rect_filled(rect, 0.0, Color32::from_rgb(r, g, b));
        }
        None => crate::ui::widgets::paint_thumb_bg(&p, rect, logic::ThumbBg::Grid),
    }
    for (cell, texture) in plan.cells.iter().zip(textures) {
        let min = rect.min + vec2(cell.x as f32 * k, cell.y as f32 * k);
        let r = Rect::from_min_size(min, vec2(cell.width as f32 * k, cell.height as f32 * k));
        match texture {
            Some(texture) => {
                p.image(texture.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                p.rect_filled(r, 0.0, t::p().text_disabled);
            }
        }
    }
}

impl App {
    /// Opens the dialog for the items among `targets` it can use, in the
    /// order they have on screen.
    pub(crate) fn open_combine(&mut self, lib: &Library, targets: &[String]) {
        let ids = logic::on_screen_order(&self.ws.order, &combinable(lib, targets));
        if ids.len() < 2 {
            return;
        }
        let sources = ids.into_iter().filter_map(|id| lib.item(&id).map(|i| (side(i.width), side(i.height))).map(|size| Source { id, size })).collect();
        self.tools.combine.sources = Some(sources);
    }

    /// The item menu's entry, with two or more images among the targets.
    pub(crate) fn combine_menu(&mut self, ui: &mut Ui, lib: &Library, targets: &[String]) {
        if combinable(lib, targets).len() < 2 {
            return;
        }
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::CombineImages);
        if ui.add(egui::Button::new(tr("Combine Images…")).shortcut_text(keys)).clicked() {
            self.open_combine(lib, targets);
        }
    }

    pub(crate) fn combine_dialog(&mut self, ctx: &egui::Context) {
        let Some(sources) = self.tools.combine.sources.clone() else { return };
        let textures: Vec<Option<TextureHandle>> = sources.iter().map(|s| self.ws.loader.get(&s.id, Tier::Grid).cloned()).collect();
        let sizes: Vec<(u32, u32)> = sources.iter().map(|s| s.size).collect();
        let mut choice = self.tools.combine.choice;
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("combine-images")).show(ctx, |ui| {
            ui.set_width(PREVIEW.x);
            ui.label(RichText::new(tr_n(sources.len(), "Combine {n} image", "Combine {n} images")).size(15.0));
            ui.add_space(8.0);
            egui::Grid::new("combine-images-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(tr("Layout"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut choice.arrange, Arrange::Horizontal, tr("combine::Horizontal"));
                    ui.selectable_value(&mut choice.arrange, Arrange::Vertical, tr("combine::Vertical"));
                    ui.selectable_value(&mut choice.arrange, Arrange::Grid, tr("Grid"));
                });
                ui.end_row();
                if choice.arrange == Arrange::Grid {
                    ui.label(tr("Columns"));
                    ui.add(egui::DragValue::new(&mut choice.columns).range(1..=MAX_COLUMNS));
                    ui.end_row();
                }
                ui.label(tr("Gap"));
                ui.add(egui::DragValue::new(&mut choice.gap).range(0..=MAX_GAP).suffix(" px"));
                ui.end_row();
                ui.label(tr("Background"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut choice.transparent, false, tr("Color"));
                    ui.selectable_value(&mut choice.transparent, true, tr("Transparent"));
                });
                ui.end_row();
                if !choice.transparent {
                    ui.label("");
                    crate::ui::color_picker::sv_hue_picker(ui, ui.make_persistent_id("combine-images-colour"), &mut choice.colour);
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            let options = choice.options();
            let plan = combine::plan(&sizes, options.layout, options.gap);
            match &plan {
                Ok(plan) => {
                    preview(ui, plan, options.background, &textures);
                    ui.add_space(4.0);
                    ui.label(RichText::new(trf("Result: {width} × {height} px", &[("width", &plan.width), ("height", &plan.height)])).color(t::p().text_muted));
                    if plan.scaled_down {
                        let megapixels = combine::MAX_PIXELS / 1_000_000;
                        ui.label(RichText::new(trf("Scaled down to fit the limits of {side} px a side and {megapixels} megapixels.", &[("side", &combine::MAX_SIDE), ("megapixels", &megapixels)])).color(t::p().text_muted));
                    }
                }
                Err(e) => {
                    ui.label(RichText::new(e.as_str()).color(t::p().text_muted));
                }
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(plan.is_ok(), egui::Button::new(tr("Combine"))).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        self.tools.combine.choice = choice;
        if let Some(go) = answer {
            self.tools.combine.sources = None;
            if go {
                self.start_combine(sources.into_iter().map(|s| s.id).collect(), choice.options());
            }
        }
    }

    /// The folders a new item joins: those of the open folder view, every
    /// picked one when several are shown together.
    fn combine_folders(&self) -> Vec<String> {
        let Scope::Folder(open) = &self.ws.scope else { return Vec::new() };
        let picked = &self.ws.folders.sel.ids;
        if picked.is_empty() { vec![open.clone()] } else { picked.clone() }
    }

    /// Reads and saves on a worker, with the library locked only to add the
    /// finished item.
    fn start_combine(&mut self, ids: Vec<String>, options: Options) {
        let Some(shared) = self.ws.lib.clone() else { return };
        if self.operation_running() {
            return;
        }
        let paths: Vec<PathBuf> = {
            let lib = lock(&shared);
            // Files still in iCloud are being fetched; try again after that.
            if self.local_only(&lib, &ids).len() < ids.len() {
                return;
            }
            ids.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect()
        };
        let mut import = self.prefs.import_options(self.combine_folders());
        import.name = Some(tr("Combined Image").to_string());
        let h = self.ws.op.start(&self.ctx, tr("Combining Images"), paths.len() + 1, true);
        std::thread::spawn(move || {
            let stage = || {
                combine::stage(&paths, options, &import, |n, path| {
                    h.step(n, path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
                    !h.cancelled()
                })
            };
            // A decoder's panic must not leave the progress modal up for good.
            let staged = std::panic::catch_unwind(std::panic::AssertUnwindSafe(stage)).unwrap_or_else(|_| Err("the image decoder failed".to_string()));
            let added = staged.and_then(|done| {
                let Some(done) = done else { return Ok(None) };
                let (width, height, scaled_down) = (done.width, done.height, done.scaled_down);
                let mut lib = lock(&shared);
                lib.begin_transaction(tr("Combine images"));
                let id = lib.commit_import(done.staged).map_err(|e| e.to_string());
                let undo = lib.end_transaction();
                id.map(|id| Some((id, undo, width, height, scaled_down)))
            });
            h.finish(match added {
                Ok(Some((id, undo, width, height, scaled_down))) => {
                    let mut message = tr_n(ids.len(), "Combined {n} image", "Combined {n} images");
                    if scaled_down {
                        message = trf("{message}; scaled down to {width} × {height} px to fit the size limit", &[("message", &message), ("width", &width), ("height", &height)]);
                    }
                    Outcome { message, changed: true, undo, select: Some(id), ..Default::default() }
                }
                Ok(None) => Outcome { message: tr("Stopped combining images").to_string(), ..Default::default() },
                Err(e) => {
                    crate::errlog::error(&format!("Combine Images: {e}"));
                    Outcome { error: Some(trf("Couldn't combine the images: {reason}", &[("reason", &e)])), ..Default::default() }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_makes_the_options() {
        let mut c = Choice::default();
        assert_eq!(c.options(), Options { layout: Layout::Horizontal, gap: 0, background: Some([255, 255, 255]) });
        c.arrange = Arrange::Grid;
        c.columns = 4;
        c.gap = 12;
        c.transparent = true;
        assert_eq!(c.options(), Options { layout: Layout::Grid(4), gap: 12, background: None });
    }

    #[test]
    fn only_decodable_images_count() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
        let mut add = |name: &str| {
            let path = tmp.path().join(name);
            if name.ends_with("png") {
                image::RgbImage::new(4, 4).save(&path).unwrap();
            } else {
                std::fs::write(&path, b"text").unwrap();
            }
            lib.import_file(&path, &Default::default()).unwrap()
        };
        let (a, b, c) = (add("a.png"), add("b.txt"), add("c.png"));
        let ids = [a.clone(), b, c.clone()];
        assert_eq!(combinable(&lib, &ids), [a, c]);
        assert_eq!(side(0), 1);
    }
}
