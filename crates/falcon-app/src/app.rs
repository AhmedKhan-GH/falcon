//! The egui application: sidebar, toolbar, grid, inspector and viewer.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
};
use falcon_library::import::ImportOptions;
use falcon_library::model::{Folder, Item};
use falcon_library::Library;

use crate::loader::{Job, Loader, Tier, job_for};
use crate::logic::{self, Scope, SortKey};

const ACCENT: Color32 = Color32::from_rgb(88, 140, 255);
const GAP: f32 = 6.0;

type OrderKey = (Scope, String, SortKey, bool, u64);

pub struct App {
    lib: Option<Arc<Library>>,
    error: Option<String>,
    status: String,
    loader: Loader,
    wanted: Vec<Job>,

    scope: Scope,
    query: String,
    sort: SortKey,
    ascending: bool,
    thumb_h: f32,
    square: bool,

    selected: HashSet<String>,
    anchor: Option<String>,
    active: Option<String>,
    viewer: Option<String>,

    generation: u64,
    order_key: Option<OrderKey>,
    order: Vec<usize>,
    folder_counts: HashMap<String, usize>,
    folder_names: HashMap<String, String>,
    tags: Vec<(String, usize)>,
    totals: (usize, usize, usize),
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let mut app = App {
            lib: None,
            error: None,
            status: String::new(),
            loader: Loader::new(&cc.egui_ctx),
            wanted: Vec::new(),
            scope: Scope::All,
            query: String::new(),
            sort: SortKey::DateAdded,
            ascending: false,
            thumb_h: 180.0,
            square: false,
            selected: HashSet::new(),
            anchor: None,
            active: None,
            viewer: None,
            generation: 0,
            order_key: None,
            order: Vec::new(),
            folder_counts: HashMap::new(),
            folder_names: HashMap::new(),
            tags: Vec::new(),
            totals: (0, 0, 0),
        };
        if let Some(p) = initial {
            app.open(&p);
        }
        app
    }

    fn open(&mut self, path: &Path) {
        match Library::open(path) {
            Ok(lib) => {
                self.lib = Some(Arc::new(lib));
                self.error = None;
                self.scope = Scope::All;
                self.selected.clear();
                self.active = None;
                self.viewer = None;
                self.loader.clear_all();
                self.refresh();
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    /// Recomputes everything derived from the library contents.
    fn refresh(&mut self) {
        self.generation += 1;
        self.order_key = None;
        let Some(lib) = &self.lib else { return };
        self.folder_counts = logic::folder_counts(&lib.metadata.folders, lib.items());
        self.folder_names = lib.all_folders().into_iter().map(|f| (f.id.clone(), f.name.clone())).collect();
        self.tags = logic::tag_counts(lib.items());
        let live = lib.items().iter().filter(|i| !i.is_deleted).count();
        let loose = lib.items().iter().filter(|i| !i.is_deleted && i.folders.is_empty()).count();
        self.totals = (live, loose, lib.items().len() - live);
    }

    fn refresh_order(&mut self, lib: &Library) {
        let key = (self.scope.clone(), self.query.clone(), self.sort, self.ascending, self.generation);
        if self.order_key.as_ref() != Some(&key) {
            self.order = logic::visible(
                lib.items(),
                &lib.metadata.folders,
                &self.scope,
                &self.query,
                self.sort,
                self.ascending,
            );
            self.order_key = Some(key);
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if dropped.is_empty() || self.lib.is_none() {
            return;
        }
        let mut files = Vec::new();
        for p in dropped {
            collect_files(&p, &mut files);
        }
        let opts = ImportOptions {
            folders: match &self.scope {
                Scope::Folder(id) => vec![id.clone()],
                _ => Vec::new(),
            },
            ..Default::default()
        };
        let lib = Arc::make_mut(self.lib.as_mut().unwrap());
        let (mut ok, mut failed, mut first_err) = (0, 0, None);
        for f in &files {
            match lib.import_file(f, &opts) {
                Ok(_) => ok += 1,
                Err(e) => {
                    failed += 1;
                    first_err.get_or_insert(e.to_string());
                }
            }
        }
        self.status = format!("Imported {ok} file(s)");
        if failed > 0 {
            self.status += &format!(", {failed} failed: {}", first_err.unwrap_or_default());
        }
        self.refresh();
    }

    // ---- selection -------------------------------------------------------

    fn click_item(&mut self, lib: &Library, pos: usize, mods: egui::Modifiers) {
        let id = lib.items()[self.order[pos]].id.clone();
        if mods.shift
            && let Some(anchor) = self.anchor.clone()
            && let Some(a) = self.order.iter().position(|&i| lib.items()[i].id == anchor)
        {
            let (lo, hi) = (a.min(pos), a.max(pos));
            if !mods.command {
                self.selected.clear();
            }
            for &i in &self.order[lo..=hi] {
                self.selected.insert(lib.items()[i].id.clone());
            }
        } else if mods.command {
            if !self.selected.remove(&id) {
                self.selected.insert(id.clone());
            }
            self.anchor = Some(id.clone());
        } else {
            self.selected.clear();
            self.selected.insert(id.clone());
            self.anchor = Some(id.clone());
        }
        self.active = Some(id);
    }

    // ---- panels ----------------------------------------------------------

    fn choose_screen(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.heading("Falcon");
                ui.add_space(12.0);
                if ui.button("Choose library...").clicked()
                    && let Some(dir) = rfd::FileDialog::new().set_title("Choose an Eagle library").pick_folder()
                {
                    self.open(&dir);
                }
                if let Some(e) = &self.error {
                    ui.add_space(12.0);
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, lib: &Library) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let (all, loose, trash) = self.totals;
            self.scope_row(ui, "All", all, Scope::All);
            self.scope_row(ui, "Uncategorized", loose, Scope::Uncategorized);
            self.scope_row(ui, "Trash", trash, Scope::Trash);
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new("FOLDERS").small().weak());
            for f in &lib.metadata.folders {
                self.folder_row(ui, f);
            }
            ui.add_space(8.0);
            ui.separator();
            egui::CollapsingHeader::new(RichText::new(format!("TAGS ({})", self.tags.len())).small().weak())
                .default_open(true)
                .show(ui, |ui| {
                    let tags = std::mem::take(&mut self.tags);
                    for (t, n) in &tags {
                        self.scope_row(ui, t, *n, Scope::Tag(t.clone()));
                    }
                    self.tags = tags;
                });
        });
    }

    fn scope_row(&mut self, ui: &mut egui::Ui, label: &str, count: usize, scope: Scope) {
        ui.horizontal(|ui| {
            if ui.selectable_label(self.scope == scope, label).clicked() {
                self.scope = scope;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.weak(count.to_string()));
        });
    }

    fn folder_row(&mut self, ui: &mut egui::Ui, f: &Folder) {
        let count = self.folder_counts.get(&f.id).copied().unwrap_or(0);
        let scope = Scope::Folder(f.id.clone());
        if f.children.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(ui.spacing().indent);
                self.scope_row_inner(ui, &f.name, count, scope);
            });
            return;
        }
        let id = ui.make_persistent_id(("folder", &f.id));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
            .show_header(ui, |ui| self.scope_row_inner(ui, &f.name, count, scope))
            .body(|ui| {
                for c in &f.children {
                    self.folder_row(ui, c);
                }
            });
    }

    fn scope_row_inner(&mut self, ui: &mut egui::Ui, label: &str, count: usize, scope: Scope) {
        if ui.selectable_label(self.scope == scope, label).clicked() {
            self.scope = scope;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.weak(count.to_string()));
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search name, tags, notes, type")
                    .desired_width(200.0),
            );
            if !self.query.is_empty() && ui.small_button("Clear").clicked() {
                self.query.clear();
            }
            ui.separator();
            egui::ComboBox::from_id_salt("sort").selected_text(self.sort.label()).show_ui(ui, |ui| {
                for k in SortKey::ALL {
                    ui.selectable_value(&mut self.sort, k, k.label());
                }
            });
            egui::ComboBox::from_id_salt("dir")
                .selected_text(if self.ascending { "Ascending" } else { "Descending" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.ascending, true, "Ascending");
                    ui.selectable_value(&mut self.ascending, false, "Descending");
                });
            ui.separator();
            ui.checkbox(&mut self.square, "Square");
            ui.label("Size");
            ui.add(egui::Slider::new(&mut self.thumb_h, 80.0..=420.0).show_value(false));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Open...").clicked()
                    && let Some(dir) = rfd::FileDialog::new().set_title("Choose an Eagle library").pick_folder()
                {
                    self.open(&dir);
                }
                ui.weak(format!("{} items ({} sel.)", self.order.len(), self.selected.len()));
            });
        });
    }

    fn grid(&mut self, ui: &mut egui::Ui, lib: &Library) {
        let width = ui.available_width() - 14.0;
        let aspects: Vec<f32> = self
            .order
            .iter()
            .map(|&i| {
                let it = &lib.items()[i];
                if self.square || it.width == 0 || it.height == 0 {
                    1.0
                } else {
                    (it.width as f32 / it.height as f32).clamp(0.25, 4.0)
                }
            })
            .collect();
        let rows = if self.square {
            let cols = ((width + GAP) / (self.thumb_h + GAP)).floor().max(1.0) as usize;
            let side = (width - GAP * (cols as f32 - 1.0)) / cols as f32;
            (0..aspects.len().div_ceil(cols))
                .map(|r| logic::Row { start: r * cols, end: ((r + 1) * cols).min(aspects.len()), height: side })
                .collect()
        } else {
            logic::justify(&aspects, width, self.thumb_h, GAP)
        };
        let mut tops = Vec::with_capacity(rows.len());
        let mut y = 0.0;
        for r in &rows {
            tops.push(y);
            y += r.height + GAP;
        }
        let total = y;
        let mods = ui.input(|i| i.modifiers);
        let mut clicked = None;
        let mut opened = None;

        egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            ui.set_min_height(total);
            let origin = ui.max_rect().min;
            let (top, bottom) = (viewport.min.y - self.thumb_h, viewport.max.y + self.thumb_h);
            let first = tops.partition_point(|&t| t < top).saturating_sub(1);
            for (ri, row) in rows.iter().enumerate().skip(first) {
                if tops[ri] > bottom {
                    break;
                }
                let visible = tops[ri] + row.height >= viewport.min.y && tops[ri] <= viewport.max.y;
                let mut x = 0.0;
                for (pos, &aspect) in aspects.iter().enumerate().take(row.end).skip(row.start) {
                    let w = if self.square { row.height } else { aspect * row.height };
                    let rect = Rect::from_min_size(origin + Vec2::new(x, tops[ri]), Vec2::new(w, row.height));
                    x += w + GAP;
                    let item = &lib.items()[self.order[pos]];
                    let resp = ui.interact(rect, ui.scope_id().with(("cell", pos)), Sense::click());
                    if resp.clicked() {
                        clicked = Some(pos);
                    }
                    if resp.double_clicked() {
                        opened = Some(item.id.clone());
                    }
                    if visible {
                        self.paint_cell(ui, lib, item, rect, self.selected.contains(&item.id), resp.hovered());
                    }
                }
            }
        });
        if let Some(pos) = clicked {
            self.click_item(lib, pos, mods);
        }
        if let Some(id) = opened {
            self.selected = HashSet::from([id.clone()]);
            self.active = Some(id.clone());
            self.viewer = Some(id);
        }
    }

    fn paint_cell(&mut self, ui: &egui::Ui, lib: &Library, item: &Item, rect: Rect, selected: bool, hovered: bool) {
        let p = ui.painter_at(rect.expand(3.0));
        let r = CornerRadius::same(4);
        p.rect_filled(rect, r, Color32::from_gray(30));
        let in_view = ui.clip_rect().intersects(rect);
        match job_for(item, lib.root(), Tier::Grid) {
            Some(job) => {
                if let Some(tex) = self.loader.get(&item.id, Tier::Grid) {
                    let size = tex.size_vec2();
                    let fit = if self.square {
                        let s = (rect.width() / size.x).min(rect.height() / size.y);
                        Rect::from_center_size(rect.center(), size * s)
                    } else {
                        rect
                    };
                    p.image(tex.id(), fit, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                } else if self.loader.has_failed(&item.id, Tier::Grid) {
                    badge(&p, rect, &item.ext);
                } else if in_view {
                    self.wanted.push(job);
                }
            }
            None => badge(&p, rect, &item.ext),
        }
        if selected {
            p.rect_stroke(rect, r, Stroke::new(3.0, ACCENT), StrokeKind::Inside);
        } else if hovered {
            p.rect_stroke(rect, r, Stroke::new(1.5, Color32::from_gray(150)), StrokeKind::Inside);
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui, lib: &Library) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let Some(item) = self.active.as_ref().and_then(|id| lib.item(id)) else {
                ui.add_space(20.0);
                ui.weak("Select an item to see its details.");
                return;
            };
            if self.selected.len() > 1 {
                let total: u64 = lib.items().iter().filter(|i| self.selected.contains(&i.id)).map(|i| i.size).sum();
                ui.strong(format!("{} items selected, {}", self.selected.len(), logic::format_size(total)));
                ui.separator();
            }
            // Preview
            let w = ui.available_width();
            let box_h = 300.0f32.min(w);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, box_h), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, Color32::from_gray(24));
            let mut tier = Tier::Preview;
            if let Some(job) = job_for(item, lib.root(), tier) {
                if self.loader.get(&item.id, tier).is_none() {
                    self.wanted.push(job);
                    if self.loader.get(&item.id, Tier::Grid).is_some() {
                        tier = Tier::Grid;
                    }
                }
                if let Some(tex) = self.loader.get(&item.id, tier) {
                    let s = tex.size_vec2();
                    let k = (rect.width() / s.x).min(rect.height() / s.y);
                    let fit = Rect::from_center_size(rect.center(), s * k);
                    ui.painter().image(
                        tex.id(),
                        fit,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else {
                badge(ui.painter(), rect, &item.ext);
            }
            ui.add_space(8.0);
            ui.heading(&item.name);

            egui::Grid::new("meta").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.weak(k);
                    ui.label(v);
                    ui.end_row();
                };
                row("Type", item.ext.to_uppercase());
                row("Size", logic::format_size(item.size));
                if item.width > 0 {
                    row("Dimensions", format!("{} x {}", item.width, item.height));
                }
                row("Added", logic::format_ms(item.modification_time));
                row("Created", logic::format_ms(item.btime));
                row("Modified", logic::format_ms(item.mtime));
            });

            if !item.tags.is_empty() {
                ui.add_space(8.0);
                ui.weak("Tags");
                ui.horizontal_wrapped(|ui| {
                    for t in &item.tags {
                        chip(ui, t);
                    }
                });
            }
            if !item.annotation.is_empty() {
                ui.add_space(8.0);
                ui.weak("Annotation");
                ui.label(&item.annotation);
            }
            if !item.url.is_empty() {
                ui.add_space(8.0);
                ui.weak("URL");
                ui.hyperlink(&item.url);
            }
            if !item.palettes.is_empty() {
                ui.add_space(8.0);
                ui.weak("Palette");
                palette_bar(ui, item);
            }
            ui.add_space(8.0);
            ui.weak("Folders");
            if item.folders.is_empty() {
                ui.label("None");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for f in &item.folders {
                        chip(ui, self.folder_names.get(f).map_or(f.as_str(), String::as_str));
                    }
                });
            }
        });
    }

    fn viewer_ui(&mut self, ui: &mut egui::Ui, lib: &Library, id: &str) {
        let Some(item) = lib.item(id) else {
            self.viewer = None;
            return;
        };
        let pos = self.order.iter().position(|&i| lib.items()[i].id == id);
        let step = ui.input(|i| {
            i.key_pressed(egui::Key::ArrowRight) as isize - i.key_pressed(egui::Key::ArrowLeft) as isize
        });
        if let (Some(p), true) = (pos, step != 0) {
            let n = self.order.len() as isize;
            let next = &lib.items()[self.order[(p as isize + step).rem_euclid(n) as usize]].id;
            self.viewer = Some(next.clone());
            self.selected = HashSet::from([next.clone()]);
            self.active = Some(next.clone());
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(8))).show(ui, |ui| {
            let rect = ui.max_rect();
            let mut tier = Tier::Full;
            match job_for(item, lib.root(), tier) {
                Some(job) => {
                    if self.loader.get(&item.id, tier).is_none() {
                        self.wanted.push(job);
                        tier = [Tier::Preview, Tier::Grid]
                            .into_iter()
                            .find(|t| self.loader.get(&item.id, *t).is_some())
                            .unwrap_or(Tier::Full);
                    }
                    if let Some(tex) = self.loader.get(&item.id, tier) {
                        let s = tex.size_vec2();
                        let area = rect.shrink(24.0);
                        let k = (area.width() / s.x).min(area.height() / s.y);
                        let fit = Rect::from_center_size(rect.center(), s * k);
                        ui.painter().image(
                            tex.id(),
                            fit,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Loading...",
                            FontId::proportional(16.0),
                            Color32::GRAY,
                        );
                    }
                }
                None => badge(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(240.0)), &item.ext),
            }
            ui.painter().text(
                rect.left_top() + Vec2::new(16.0, 14.0),
                egui::Align2::LEFT_TOP,
                format!("{}.{}    (Esc to close)", item.name, item.ext),
                FontId::proportional(14.0),
                Color32::from_gray(190),
            );
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.loader.poll(&ctx);
        self.handle_drops(&ctx);
        self.wanted.clear();

        let Some(lib) = self.lib.clone() else {
            self.choose_screen(ui);
            return;
        };
        self.refresh_order(&lib);

        if let Some(id) = self.viewer.clone() {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.viewer = None;
            } else {
                self.viewer_ui(ui, &lib, &id);
                let jobs = std::mem::take(&mut self.wanted);
                self.loader.want(jobs);
                return;
            }
        }

        egui::Panel::left("sidebar").default_size(250.0).size_range(180.0..=420.0).show(ui, |ui| {
            self.sidebar(ui, &lib);
        });
        egui::Panel::right("inspector").default_size(320.0).size_range(240.0..=480.0).show(ui, |ui| {
            self.inspector(ui, &lib);
        });
        egui::Panel::top("toolbar").exact_size(40.0).show(ui, |ui| self.toolbar(ui));
        if !self.status.is_empty() {
            egui::Panel::bottom("status").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(&self.status);
                    if ui.small_button("Dismiss").clicked() {
                        self.status.clear();
                    }
                });
            });
        }
        egui::CentralPanel::default().show(ui, |ui| self.grid(ui, &lib));

        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let r = ctx.content_rect();
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::NULL.with("drop")));
            p.rect_filled(r, 0.0, Color32::from_black_alpha(170));
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to import",
                FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
        let jobs = std::mem::take(&mut self.wanted);
        self.loader.want(jobs);
    }
}

fn collect_files(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        if let Ok(rd) = std::fs::read_dir(p) {
            let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            v.sort();
            for c in v {
                if !c.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    collect_files(&c, out);
                }
            }
        }
    } else {
        out.push(p.to_path_buf());
    }
}

fn badge(p: &egui::Painter, rect: Rect, ext: &str) {
    let label = if ext.is_empty() { "FILE".to_string() } else { ext.to_uppercase() };
    let s = (rect.width().min(rect.height()) * 0.5).clamp(36.0, 90.0);
    let b = Rect::from_center_size(rect.center(), Vec2::new(s, s * 0.6));
    p.rect_filled(b, 6.0, Color32::from_gray(55));
    p.text(
        b.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional((s * 0.28).max(11.0)),
        Color32::from_gray(210),
    );
}

fn chip(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(Color32::from_gray(48))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(RichText::new(text).small()));
}

fn palette_bar(ui: &mut egui::Ui, item: &Item) {
    let total: f64 = item.palettes.iter().map(|c| c.ratio.as_f64().unwrap_or(0.0)).sum::<f64>().max(1e-9);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
    let mut x = rect.min.x;
    for (n, c) in item.palettes.iter().enumerate() {
        let w = (c.ratio.as_f64().unwrap_or(0.0) / total) as f32 * rect.width();
        let r = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w, rect.height()));
        x += w;
        let [r8, g8, b8] = c.color;
        ui.painter().rect_filled(r, 0.0, Color32::from_rgb(r8, g8, b8));
        let resp = ui.interact(r, ui.scope_id().with(("sw", n)), Sense::hover());
        resp.on_hover_text(format!("{}  rgb({r8}, {g8}, {b8})", logic::hex(c.color)));
    }
}
