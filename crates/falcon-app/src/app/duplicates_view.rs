//! The Duplicates page, after Eagle's duplicate panel: pick a method
//! (identical files by content hash, or similar pictures by perceptual
//! hash) and which items to scan (all, the view the page was opened from,
//! or the selection then), scan on a background thread, adjust the
//! groups, then merge them: each group keeps one item, which takes the
//! chosen name, link, note, folders, tags and the best rating, and the
//! rest go to the Trash, as one undo step (see the desktop app chapter).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use eframe::egui::{self, Align2, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use falcon_library::duplicates::{self, CacheEntry, HashCache};
use falcon_library::merge::{self, Choices, Merge, MergeReport};
use falcon_library::model::Item;
use falcon_library::{Library, thumbnail};

use super::{Action, App, GRID_PAD_X, Scope, export_ui::plural};
use crate::loader::{Tier, job_for};
use crate::logic::{self, DupGroup};
use crate::theme::{self as t, font, mono};

/// Phosphor's copy glyph, for the sidebar entry.
pub(super) const ICON: &str = "\u{E1CA}";
/// Default similarity threshold in differing hash bits (of 64).
const THRESHOLD: u32 = 10;
const THUMB: f32 = 120.0;

/// What a scan looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Method {
    /// Byte-identical originals.
    #[default]
    Same,
    /// Pictures that look alike.
    Similar,
}

/// Which items a scan looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Range {
    #[default]
    All,
    /// The list the page was opened from.
    View,
    /// What was selected when the page was opened.
    Selected,
}

#[derive(Default)]
struct Progress {
    phase: &'static str,
    done: usize,
    total: usize,
    result: Option<Vec<DupGroup>>,
}

/// One group in the merge review: what its items offer, and the merge as
/// chosen so far.
struct ReviewGroup {
    choices: Choices,
    merge: Merge,
    /// Use the note joining every note (Eagle's "merge all notes").
    joined: bool,
}

/// The merge review dialog.
struct Review {
    groups: Vec<ReviewGroup>,
    current: usize,
    /// Asking "merge these?" before doing it, as Eagle always does.
    confirming: bool,
}

/// State of the page, kept while the app runs.
pub(super) struct Duplicates {
    /// Hashes by item id, valid while the original's size and mtime stay
    /// the same. Kept in memory only, like Eagle, which stores none.
    cache: Arc<Mutex<HashCache>>,
    /// The library the cache and results belong to.
    root: Option<PathBuf>,
    progress: Option<Arc<Mutex<Progress>>>,
    groups: Option<Vec<DupGroup>>,
    threshold: u32,
    method: Method,
    range: Range,
    /// The view the page was opened from, its items in order, and the
    /// selection then.
    came_from: Option<(Scope, Vec<String>, Vec<String>)>,
    /// The method and items of the last scan, for rescanning.
    scanned: Option<(Method, Option<HashSet<String>>)>,
    review: Option<Review>,
    /// The last merge, for the summary.
    report: Option<MergeReport>,
}

impl Default for Duplicates {
    fn default() -> Self {
        Duplicates {
            cache: Arc::default(),
            root: None,
            progress: None,
            groups: None,
            threshold: THRESHOLD,
            method: Method::default(),
            range: Range::default(),
            came_from: None,
            scanned: None,
            review: None,
            report: None,
        }
    }
}

impl Duplicates {
    /// Notes the view the page is being opened from, so a scan can cover
    /// just its items or just the selection, as Eagle's panel can.
    pub(super) fn opened_from(&mut self, scope: Scope, order: Vec<String>, selected: Vec<String>) {
        self.came_from = Some((scope, order, selected));
        // A range that no longer has items falls back to everything.
        if self.range_ids().is_some_and(|ids| ids.is_empty()) {
            self.range = Range::All;
        }
    }

    /// The ids the current range covers (`None` for every item).
    fn range_ids(&self) -> Option<HashSet<String>> {
        let (_, order, selected) = self.came_from.as_ref()?;
        match self.range {
            Range::All => None,
            Range::View => Some(order.iter().cloned().collect()),
            Range::Selected => Some(selected.iter().cloned().collect()),
        }
    }

    /// A finished merge, for the summary.
    pub(super) fn merged(&mut self, report: MergeReport) {
        self.report = Some(report);
    }
}

/// Size and mtime (ms) of a file, as the library's cache keys them.
fn stamp(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
    Some((m.len(), mtime))
}

/// The cache entry for an item, replaced if its file changed.
fn entry<'a>(cache: &'a mut HashCache, id: &str, (size, mtime): (u64, u64)) -> &'a mut CacheEntry {
    let e = cache.entries.entry(id.to_string()).or_insert(CacheEntry { size, mtime, content: None, perceptual: None });
    if e.size != size || e.mtime != mtime {
        *e = CacheEntry { size, mtime, content: None, perceptual: None };
    }
    e
}

/// The picture a perceptual hash is taken from, as the library picks it:
/// the thumbnail, or a small original shown without one.
fn picture(item: &Item, root: &Path) -> Option<PathBuf> {
    item.thumbnail_path(root).or_else(|| (thumbnail::is_supported(&item.ext) && item.ext != "svg").then(|| item.original_path(root)))
}

/// Fills the cache item by item, so progress can be shown, then lets the
/// library group the items (among `only`, when given) from the cached
/// hashes. Similar groups put the best copy first, as Eagle does.
fn scan(lib: Library, method: Method, only: Option<&HashSet<String>>, threshold: u32, cache: &Mutex<HashCache>, progress: &Mutex<Progress>, ctx: &egui::Context) -> Vec<DupGroup> {
    let Ok(mut cache) = cache.lock() else { return Vec::new() };
    let root = lib.root().to_path_buf();
    let live: Vec<(&Item, (u64, u64))> = lib
        .items()
        .iter()
        .filter(|i| !i.is_deleted && only.is_none_or(|o| o.contains(&i.id)))
        .filter_map(|i| Some((i, stamp(&i.original_path(&root))?)))
        .collect();
    let set = |phase, done, total| {
        if let Ok(mut p) = progress.lock() {
            (p.phase, p.done, p.total) = (phase, done, total);
        }
        ctx.request_repaint();
    };
    match method {
        Method::Same => {
            // Only files sharing their size with another can be byte-identical.
            let mut sizes: HashMap<u64, usize> = HashMap::new();
            for (_, (size, _)) in &live {
                *sizes.entry(*size).or_default() += 1;
            }
            let candidates: Vec<_> = live.iter().filter(|(_, s)| sizes[&s.0] > 1).collect();
            for (n, (item, st)) in candidates.iter().enumerate() {
                set("Comparing files", n, candidates.len());
                let e = entry(&mut cache, &item.id, *st);
                if e.content.is_none() {
                    e.content = duplicates::content_hash(&item.original_path(&root)).ok();
                }
            }
            set("Grouping", candidates.len(), candidates.len());
            duplicates::find_exact_duplicates_in(&lib, only, &mut cache).into_iter().map(|ids| logic::dup_group(true, ids, |_| None)).collect()
        }
        Method::Similar => {
            for (n, (item, st)) in live.iter().enumerate() {
                set("Comparing pictures", n, live.len());
                let e = entry(&mut cache, &item.id, *st);
                if e.perceptual.is_none() {
                    e.perceptual = picture(item, &root).and_then(|p| thumbnail::decode(&p).ok()).map(|img| duplicates::perceptual_hash(&img));
                }
            }
            set("Grouping", live.len(), live.len());
            let near = duplicates::find_near_duplicates_in(&lib, only, threshold, &mut cache);
            near.into_iter()
                .map(|ids| {
                    let mut items: Vec<&Item> = ids.iter().filter_map(|id| lib.item(id)).collect();
                    merge::best_first(&mut items);
                    let ids = items.iter().map(|i| i.id.clone()).collect();
                    logic::dup_group(false, ids, |id| cache.entries.get(id).and_then(|e| e.perceptual))
                })
                .collect()
        }
    }
}

/// Bytes a merge of the picked groups frees: every member but the kept one.
fn saving(groups: &[DupGroup], lib: &Library) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    for g in groups.iter().filter(|g| g.picked) {
        files += g.ids.len();
        bytes += g.ids.iter().filter(|id| **id != g.keep).filter_map(|id| lib.item(id)).map(|i| i.size).sum::<u64>();
    }
    (files, bytes)
}

/// A changes made to the results while drawing them.
enum Edit {
    Pick(usize, bool),
    Keep(usize, String),
    RemoveItem(usize, String),
    RemoveGroup(usize),
}

impl App {
    /// Starts a scan of the chosen items on a background thread.
    fn start_duplicate_scan(&mut self, lib: &Library) {
        let d = &mut self.ws.duplicates;
        if d.root.as_deref() != Some(lib.root()) {
            *d = Duplicates { threshold: d.threshold, method: d.method, ..Default::default() };
            d.root = Some(lib.root().to_path_buf());
        }
        let only = d.range_ids();
        d.scanned = Some((d.method, only.clone()));
        d.review = None;
        let progress = Arc::new(Mutex::new(Progress { phase: "Starting", ..Default::default() }));
        d.progress = Some(progress.clone());
        let (snapshot, method, threshold, cache, ctx) = (lib.clone(), d.method, d.threshold, d.cache.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let groups = scan(snapshot, method, only.as_ref(), threshold, &cache, &progress, &ctx);
            if let Ok(mut p) = progress.lock() {
                p.result = Some(groups);
            }
            ctx.request_repaint();
        });
    }

    pub(super) fn duplicates_page(&mut self, ui: &mut Ui, lib: &Library) {
        if self.ws.duplicates.root.as_deref().is_some_and(|r| r != lib.root()) {
            let d = &self.ws.duplicates;
            self.ws.duplicates = Duplicates { threshold: d.threshold, method: d.method, came_from: d.came_from.clone(), ..Default::default() };
        }
        // A finished scan hands over its groups.
        let finished = self.ws.duplicates.progress.as_ref().and_then(|p| p.lock().ok().and_then(|mut p| p.result.take()));
        if let Some(g) = finished {
            self.ws.duplicates.groups = Some(g);
            self.ws.duplicates.progress = None;
        }
        let running = self.ws.duplicates.progress.as_ref().and_then(|p| p.lock().ok().map(|p| (p.phase, p.done, p.total)));
        // Trashed members (merged, or trashed elsewhere) leave the results.
        if let Some(g) = &self.ws.duplicates.groups {
            let live = logic::live_groups(g, |id| lib.item(id).is_some_and(|i| !i.is_deleted));
            self.ws.duplicates.groups = Some(live);
        }
        let groups = self.ws.duplicates.groups.clone();
        // Selection and arrow keys walk the members in page order.
        let mut seen = HashSet::new();
        self.ws.order = groups.iter().flatten().flat_map(|g| g.ids.iter()).filter(|id| seen.insert(id.as_str())).cloned().collect();

        let area = ui.max_rect();
        let mut rescan = false;
        let mut start_review = false;
        let mut edits = Vec::new();
        let similar = self.ws.duplicates.method == Method::Similar;
        egui::ScrollArea::vertical().id_salt("duplicates-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(GRID_PAD_X);
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_enabled_ui(running.is_none(), |ui| self.scan_controls(ui, lib, groups.is_some(), &mut rescan));
            });
            ui.add_space(10.0);
            let x0 = area.left() + GRID_PAD_X;
            let p = ui.painter().clone();
            if let Some((phase, done, total)) = running {
                let (r, _) = ui.allocate_exact_size(vec2(area.width(), 40.0), Sense::hover());
                let bar = Rect::from_min_size(Pos2::new(x0, r.top() + 4.0), vec2(320.0, 6.0));
                p.rect_filled(bar, 3.0, t::p().button);
                let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height())), 3.0, t::p().primary);
                p.text(Pos2::new(x0, bar.bottom() + 14.0), Align2::LEFT_CENTER, format!("{phase}… {done}/{total}"), font(12.0), t::p().text_muted);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            let Some(groups) = &groups else {
                let (r, _) = ui.allocate_exact_size(vec2(area.width(), 60.0), Sense::hover());
                let text = if similar {
                    "Finds pictures that look alike, picks the best copy of each group, and merges the rest into it."
                } else {
                    "Finds byte-identical files and merges each group into one item."
                };
                p.text(Pos2::new(x0, r.center().y), Align2::LEFT_CENTER, format!("{text} Nothing changes until you merge."), font(13.0), t::p().text_muted);
                return;
            };
            if groups.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(area.width(), 60.0), Sense::hover());
                let what = if similar { "No similar pictures found" } else { "No identical files found" };
                p.text(Pos2::new(x0, r.center().y), Align2::LEFT_CENTER, what, font(14.0), t::p().text_muted);
                return;
            }
            let (files, bytes) = saving(groups, lib);
            ui.horizontal(|ui| {
                ui.add_space(GRID_PAD_X);
                ui.spacing_mut().item_spacing.x = 8.0;
                let picked = groups.iter().filter(|g| g.picked).count();
                if ui.add_enabled(picked > 0, egui::Button::new(format!("Merge {}…", plural(picked, "Group")))).clicked() {
                    start_review = true;
                }
                let line = if picked > 0 {
                    format!("{} groups · {} picked, merging saves {}", groups.len(), plural(files, "file"), logic::format_size(bytes))
                } else {
                    format!("{} groups · tick the groups to merge", groups.len())
                };
                ui.label(RichText::new(line).color(t::p().text_muted));
            });
            ui.add_space(6.0);
            let mut jobs = Vec::new();
            let mut clicked: Option<(String, egui::Modifiers)> = None;
            let mut opened: Option<String> = None;
            for (gn, g) in groups.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_space(GRID_PAD_X);
                    let kind = if g.exact {
                        format!("{} identical files", g.ids.len())
                    } else {
                        format!("{} similar pictures, the best copy picked (within {} of 64 bits)", g.ids.len(), g.distances.iter().max().unwrap_or(&0))
                    };
                    let mut picked = g.picked;
                    if ui.checkbox(&mut picked, RichText::new(kind).color(t::p().text_value).size(12.0)).changed() {
                        edits.push(Edit::Pick(gn, picked));
                    }
                    if ui.small_button("Remove from List").on_hover_text("Leaves these items alone").clicked() {
                        edits.push(Edit::RemoveGroup(gn));
                    }
                });
                let per_row = (((area.width() - 2.0 * GRID_PAD_X) + 10.0) / (THUMB + 10.0)).floor().max(1.0) as usize;
                let rows = g.ids.len().div_ceil(per_row);
                let (block, _) = ui.allocate_exact_size(vec2(area.width(), rows as f32 * (THUMB + 58.0)), Sense::hover());
                for (n, id) in g.ids.iter().enumerate() {
                    let Some(item) = lib.item(id) else { continue };
                    let (row, col) = (n / per_row, n % per_row);
                    let cell = Rect::from_min_size(Pos2::new(x0 + col as f32 * (THUMB + 10.0), block.top() + row as f32 * (THUMB + 58.0)), vec2(THUMB, THUMB));
                    if !ui.is_rect_visible(cell.expand(THUMB)) {
                        continue;
                    }
                    let kept = *id == g.keep;
                    let resp = ui.interact(cell, ui.scope_id().with(("dup", gn, id.as_str())), Sense::click());
                    p.rect_filled(cell, 6.0, if resp.hovered() { t::p().card_hover } else { t::p().card_border });
                    if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                        jobs.push(j);
                    }
                    self.draw_image(ui, lib, item, cell.shrink(4.0), false);
                    if kept {
                        p.rect_stroke(cell, 6.0, Stroke::new(2.0, t::p().primary), StrokeKind::Outside);
                    } else if self.ws.selected.contains(id) {
                        p.rect_stroke(cell, 6.0, Stroke::new(1.5, t::p().text_muted), StrokeKind::Outside);
                    }
                    let name = super::galley(ui, &format!("{}{}.{}", if kept { "Kept · " } else { "" }, item.name, item.ext), font(11.0), t::p().text, THUMB);
                    p.galley(Pos2::new(cell.left(), cell.bottom() + 4.0), name, t::p().text);
                    let mut meta = format!("{} · {}", item.ext.to_uppercase(), logic::format_size(item.size));
                    if !g.exact && n > 0 {
                        meta.push_str(&format!(" · {} bits", g.distances[n]));
                    }
                    p.text(Pos2::new(cell.left(), cell.bottom() + 28.0), Align2::LEFT_CENTER, meta, mono(10.0), t::p().text_muted);
                    if item.width > 0 {
                        p.text(Pos2::new(cell.left(), cell.bottom() + 42.0), Align2::LEFT_CENTER, format!("{} × {}", item.width, item.height), mono(10.0), t::p().text_muted);
                    }
                    let resp = resp.on_hover_text(if g.exact { "Right-click to keep this copy instead" } else { "Click to keep this copy" });
                    if resp.double_clicked() {
                        opened = Some(id.clone());
                    } else if resp.clicked() {
                        // A click on a similar picture makes it the copy kept,
                        // as in Eagle; it is also selected for the inspector.
                        if !g.exact {
                            edits.push(Edit::Keep(gn, id.clone()));
                        }
                        clicked = Some((id.clone(), ui.input(|i| i.modifiers)));
                    }
                    resp.context_menu(|ui| {
                        if ui.add_enabled(!kept, egui::Button::new("Keep This Copy")).clicked() {
                            edits.push(Edit::Keep(gn, id.clone()));
                        }
                        if ui.button("Open").clicked() {
                            opened = Some(id.clone());
                        }
                        let files = [item.original_path(lib.root())];
                        if ui.button("Open in Default App").clicked() {
                            crate::open_with::open_default(&files);
                            self.used(&files);
                        }
                        ui.separator();
                        if ui.button("Remove from Group").on_hover_text("Leaves this item alone").clicked() {
                            edits.push(Edit::RemoveItem(gn, id.clone()));
                        }
                    });
                }
                ui.add_space(8.0);
            }
            self.ws.frame_jobs.extend(jobs);
            if let Some((id, m)) = clicked {
                self.click(&id, m);
            }
            if let Some(id) = opened {
                self.ws.selected = vec![id.clone()];
                self.ws.viewer = Some(id);
            }
        });
        if let Some(list) = &mut self.ws.duplicates.groups {
            // Applied from the last edit back, so earlier indices stay valid.
            for e in edits.into_iter().rev() {
                match e {
                    Edit::Pick(at, on) => list[at].picked = on,
                    Edit::Keep(at, id) => list[at].keep = id,
                    Edit::RemoveItem(at, id) => logic::remove_member(list, at, &id),
                    Edit::RemoveGroup(at) => {
                        list.remove(at);
                    }
                }
            }
        }
        if start_review {
            self.open_merge_review(lib);
        }
        if rescan {
            self.start_duplicate_scan(lib);
        }
        let ctx = ui.ctx().clone();
        self.merge_review(&ctx, lib);
        self.merge_summary(&ctx);
    }

    /// The method, the items to scan, the similarity slider and the scan
    /// button.
    fn scan_controls(&mut self, ui: &mut Ui, lib: &Library, has_results: bool, rescan: &mut bool) {
        let d = &mut self.ws.duplicates;
        let before = (d.method, d.range);
        ui.selectable_value(&mut d.method, Method::Same, "Identical Files").on_hover_text("Files with exactly the same bytes");
        ui.selectable_value(&mut d.method, Method::Similar, "Similar Pictures").on_hover_text("Pictures that look alike, at any size or format");
        ui.add_space(6.0);
        let live = lib.items().iter().filter(|i| !i.is_deleted).count();
        let (view, view_n, sel_n) = match &d.came_from {
            Some((scope, order, selected)) => (scope.title(&lib.metadata.folders), order.len(), selected.len()),
            None => (String::new(), 0, 0),
        };
        let label = |r: Range| match r {
            Range::All => format!("All Items ({live})"),
            Range::View => format!("{view} ({view_n})"),
            Range::Selected => format!("Selected Items ({sel_n})"),
        };
        egui::ComboBox::from_id_salt("dup-range").selected_text(label(d.range)).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.range, Range::All, label(Range::All));
            if view_n > 0 {
                ui.selectable_value(&mut d.range, Range::View, label(Range::View));
            }
            if sel_n > 1 {
                ui.selectable_value(&mut d.range, Range::Selected, label(Range::Selected));
            }
        });
        // Changing what to look for starts over, as Eagle's back button does.
        if (d.method, d.range) != before {
            d.groups = None;
        }
        if d.method == Method::Similar {
            ui.label(RichText::new("Similarity").color(t::p().text_muted));
            // Right is stricter, as in Eagle; the value is the bits allowed to differ.
            let mut strict = 20 - d.threshold.min(20) as i32;
            let s = ui.add(egui::Slider::new(&mut strict, 0..=20).show_value(false));
            d.threshold = (20 - strict) as u32;
            if s.on_hover_text("How alike similar pictures must be").drag_stopped() && has_results {
                *rescan = true;
            }
        }
        let label = if has_results && d.groups.is_some() { "Scan Again" } else { "Scan" };
        if ui.button(label).clicked() {
            *rescan = true;
        }
    }

    /// Opens the merge review for the ticked groups, with Eagle's starting
    /// values.
    fn open_merge_review(&mut self, lib: &Library) {
        let Some(groups) = &self.ws.duplicates.groups else { return };
        let review: Vec<ReviewGroup> = groups
            .iter()
            .filter(|g| g.picked)
            .map(|g| {
                let items: Vec<&Item> = g.ids.iter().filter_map(|id| lib.item(id)).collect();
                let choices = Choices::of(&items);
                let trash = g.ids.iter().filter(|id| **id != g.keep).cloned().collect();
                let merge = choices.defaults(&g.keep, trash);
                ReviewGroup { choices, merge, joined: false }
            })
            .collect();
        if !review.is_empty() {
            self.ws.duplicates.review = Some(Review { groups: review, current: 0, confirming: false });
        }
    }

    /// The merge review: for each group, the kept item's name, link and
    /// note (one of the group's, or every note joined), and which of the
    /// group's folders and tags it keeps.
    fn merge_review(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut review) = self.ws.duplicates.review.take() else { return };
        let mut close = false;
        let mut merge = false;
        let folder_name = |id: &str| logic::find_folder(&lib.metadata.folders, id).map_or_else(|| id.to_string(), |f| f.name.clone());
        let n_trash: usize = review.groups.iter().map(|g| g.merge.trash.len()).sum();
        let modal = egui::Modal::new(egui::Id::unique("merge-review")).show(ctx, |ui| {
            ui.set_width(640.0);
            if review.confirming {
                ui.label(RichText::new(format!("Merge {}?", plural(review.groups.len(), "group"))).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{} go to the Trash. ⌘Z undoes the merge.", plural(n_trash, "file"))).color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Merge").clicked() {
                        merge = true;
                    }
                    if ui.button("Back").clicked() {
                        review.confirming = false;
                    }
                });
                return;
            }
            ui.label(RichText::new("Check Before Merging").size(15.0));
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    egui::ScrollArea::vertical().id_salt("merge-groups").max_height(380.0).show(ui, |ui| {
                        for (n, g) in review.groups.iter().enumerate() {
                            let name = lib.item(&g.merge.keep).map_or_else(String::new, |i| i.name.clone());
                            let text = format!("{}\n{}", name, plural(g.merge.trash.len() + 1, "file"));
                            if ui.selectable_label(n == review.current, text).clicked() {
                                review.current = n;
                            }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt("merge-props").max_height(380.0).show(ui, |ui| {
                        let g = &mut review.groups[review.current];
                        if let Some(item) = lib.item(&g.merge.keep) {
                            let (r, _) = ui.allocate_exact_size(vec2(120.0, 90.0), Sense::hover());
                            if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                                self.ws.frame_jobs.push(j);
                            }
                            self.draw_image(ui, lib, item, r, false);
                        }
                        let section = |ui: &mut Ui, title: &str| {
                            ui.add_space(6.0);
                            ui.label(RichText::new(title).color(t::p().text_muted).size(12.0));
                        };
                        let empty = |ui: &mut Ui, text: &str| {
                            ui.label(RichText::new(text).color(t::p().text_muted).italics());
                        };
                        section(ui, "File Name");
                        for name in &g.choices.names {
                            if ui.radio(g.merge.name.as_ref() == Some(name), name).clicked() {
                                g.merge.name = Some(name.clone());
                            }
                        }
                        section(ui, "Link");
                        if g.choices.urls.is_empty() {
                            empty(ui, "No link");
                        }
                        for url in &g.choices.urls {
                            if ui.radio(g.merge.url == *url, url).clicked() {
                                g.merge.url = url.clone();
                            }
                        }
                        section(ui, "Note");
                        if g.choices.notes.is_empty() {
                            empty(ui, "No note");
                        }
                        for note in &g.choices.notes {
                            let short: String = note.lines().next().unwrap_or("").chars().take(60).collect();
                            if ui.radio(!g.joined && g.merge.note == *note, short).on_hover_text(note).clicked() {
                                g.merge.note = note.clone();
                                g.joined = false;
                            }
                        }
                        if g.choices.merged_note.is_some() && ui.radio(g.joined, "Join all notes").clicked() {
                            g.joined = true;
                        }
                        section(ui, "Folders");
                        if g.choices.folders.is_empty() {
                            empty(ui, "No folders");
                        }
                        ui.horizontal_wrapped(|ui| {
                            for f in g.choices.folders.clone() {
                                let on = g.merge.folders.contains(&f);
                                if ui.selectable_label(on, folder_name(&f)).clicked() {
                                    let mut set: HashSet<String> = g.merge.folders.iter().cloned().collect();
                                    if on {
                                        set.remove(&f);
                                    } else {
                                        set.insert(f);
                                    }
                                    g.merge.folders = g.choices.folders.iter().filter(|x| set.contains(*x)).cloned().collect();
                                }
                            }
                        });
                        section(ui, "Tags");
                        if g.choices.tags.is_empty() {
                            empty(ui, "No tags");
                        }
                        ui.horizontal_wrapped(|ui| {
                            for tag in g.choices.tags.clone() {
                                let on = g.merge.tags.contains(&tag);
                                if ui.selectable_label(on, &tag).clicked() {
                                    let mut set: HashSet<String> = g.merge.tags.iter().cloned().collect();
                                    if on {
                                        set.remove(&tag);
                                    } else {
                                        set.insert(tag);
                                    }
                                    g.merge.tags = g.choices.tags.iter().filter(|x| set.contains(*x)).cloned().collect();
                                }
                            }
                        });
                    });
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} · {} go to the Trash", plural(review.groups.len(), "group"), plural(n_trash, "file"))).color(t::p().text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Merge…").clicked() {
                        review.confirming = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        });
        if modal.should_close() {
            close = true;
        }
        if merge {
            let merges: Vec<Merge> = review
                .groups
                .into_iter()
                .map(|g| {
                    let mut m = g.merge;
                    if g.joined
                        && let Some(note) = g.choices.merged_note
                    {
                        m.note = note;
                    }
                    m
                })
                .collect();
            self.ws.actions.push(Action::MergeDuplicates(merges));
        } else if !close {
            self.ws.duplicates.review = Some(review);
        }
    }

    /// What the last merge did, with the choice to go on or start over.
    fn merge_summary(&mut self, ctx: &egui::Context) {
        let Some(report) = self.ws.duplicates.report else { return };
        let mut done = None;
        let modal = egui::Modal::new(egui::Id::unique("merge-summary")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(RichText::new("Merged").size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{} moved to the Trash, saving {}.", plural(report.trashed, "file"), logic::format_size(report.bytes))).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Keep Merging").clicked() {
                    done = Some(false);
                }
                if ui.button("Done").clicked() {
                    done = Some(true);
                }
            });
        });
        if modal.should_close() {
            done = Some(false);
        }
        if let Some(finish) = done {
            self.ws.duplicates.report = None;
            if finish {
                self.ws.duplicates.groups = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_library::import::ImportOptions;
    use image::{DynamicImage, Rgb, RgbImage};

    fn scene(w: u32, h: u32, stripes: bool) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            if stripes {
                if (x / 16 + y / 24) % 2 == 0 { Rgb([20, 20, 20]) } else { Rgb([230, 240, 250]) }
            } else if ((fx - 0.3).powi(2) + (fy - 0.4).powi(2)).sqrt() < 0.2 {
                Rgb([240, 200, 40])
            } else {
                Rgb([(fx * 120.0) as u8, 60, (fy * 200.0) as u8])
            }
        }))
    }

    #[test]
    fn scan_groups_copies_and_reports_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("D.library")).unwrap();
        let opts = ImportOptions { palette: false, ..Default::default() };
        let mut import = |name: &str, img: DynamicImage| {
            let p = tmp.path().join(name);
            img.save(&p).unwrap();
            lib.import_file(p, &opts).unwrap()
        };
        let c = import("c.png", scene(1000, 750, false));
        let a = import("a.png", scene(1200, 900, false));
        let b = import("b.png", scene(1200, 900, false));
        let d = import("d.png", scene(1200, 900, true));
        let cache = Mutex::new(HashCache::default());
        let progress = Mutex::new(Progress::default());
        let ctx = egui::Context::default();

        let same = scan(lib.clone(), Method::Same, None, THRESHOLD, &cache, &progress, &ctx);
        assert_eq!(same.len(), 1, "{same:?}");
        assert!(same[0].exact && same[0].picked);
        assert_eq!(same[0].ids, [a.clone(), b.clone()]);
        assert_eq!(same[0].keep, a);

        let similar = scan(lib.clone(), Method::Similar, None, THRESHOLD, &cache, &progress, &ctx);
        assert_eq!(similar.len(), 1, "{similar:?}");
        assert!(!similar[0].exact);
        // The biggest picture comes first and is kept, though c came first.
        assert_eq!(similar[0].ids, [a.clone(), b.clone(), c.clone()]);
        assert_eq!(similar[0].keep, a);
        assert!(!similar.iter().any(|g| g.ids.contains(&d)));
        let p = progress.lock().unwrap();
        assert_eq!((p.done, p.total), (4, 4));
        drop(p);
        // Every item was hashed once and kept in memory for the next scan.
        let cached = cache.lock().unwrap().clone();
        assert!(cached.entries.values().all(|e| e.perceptual.is_some()));
        let again = scan(lib.clone(), Method::Similar, None, THRESHOLD, &cache, &progress, &ctx);
        assert_eq!(again, similar);
        assert_eq!(*cache.lock().unwrap(), cached);

        // A narrower range only groups its own items.
        let only: HashSet<String> = [c.clone(), b.clone(), d.clone()].into_iter().collect();
        let some = scan(lib.clone(), Method::Similar, Some(&only), THRESHOLD, &cache, &progress, &ctx);
        assert_eq!(some.len(), 1);
        assert_eq!(some[0].ids, [b.clone(), c.clone()]);
        assert!(scan(lib.clone(), Method::Same, Some(&only), THRESHOLD, &cache, &progress, &ctx).is_empty());
        assert_eq!(saving(&some, &lib), (2, lib.item(&c).unwrap().size));
    }

    #[test]
    fn the_range_follows_the_view_the_page_came_from() {
        let mut d = Duplicates::default();
        assert_eq!(d.range_ids(), None);
        d.range = Range::Selected;
        d.opened_from(Scope::All, vec!["a".into(), "b".into()], vec![]);
        assert_eq!(d.range, Range::All, "no selection: falls back to all items");
        d.range = Range::View;
        d.opened_from(Scope::All, vec!["a".into(), "b".into()], vec!["b".into()]);
        assert_eq!(d.range_ids().unwrap().len(), 2);
    }
}
