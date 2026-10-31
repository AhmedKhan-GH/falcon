//! The Duplicates page: exact copies by content hash and similar pictures
//! by perceptual hash, found on a background thread (see the desktop app
//! chapter of the docs).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use eframe::egui::{self, Align2, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use falcon_library::duplicates::{self, CacheEntry, HashCache};
use falcon_library::model::Item;
use falcon_library::{Library, thumbnail};

use super::{Action, App, GRID_PAD_X};
use crate::loader::{Tier, job_for};
use crate::logic::{self, DupGroup};
use crate::theme::{self as t, font, mono};

/// Phosphor's copy glyph, for the sidebar entry.
pub(super) const ICON: &str = "\u{E1CA}";
/// Default similarity threshold in differing hash bits (of 64).
const THRESHOLD: u32 = 10;
const THUMB: f32 = 120.0;

#[derive(Default)]
struct Progress {
    phase: &'static str,
    done: usize,
    total: usize,
    result: Option<Vec<DupGroup>>,
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
}

impl Default for Duplicates {
    fn default() -> Self {
        Duplicates { cache: Arc::default(), root: None, progress: None, groups: None, threshold: THRESHOLD }
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
/// library group the items from the cached hashes.
fn scan(lib: Library, threshold: u32, cache: &Mutex<HashCache>, progress: &Mutex<Progress>, ctx: &egui::Context) -> Vec<DupGroup> {
    let Ok(mut cache) = cache.lock() else { return Vec::new() };
    let root = lib.root().to_path_buf();
    let live: Vec<(&Item, (u64, u64))> = lib.items().iter().filter(|i| !i.is_deleted).filter_map(|i| Some((i, stamp(&i.original_path(&root))?))).collect();
    let set = |phase, done, total| {
        if let Ok(mut p) = progress.lock() {
            (p.phase, p.done, p.total) = (phase, done, total);
        }
        ctx.request_repaint();
    };
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
    for (n, (item, st)) in live.iter().enumerate() {
        set("Comparing pictures", n, live.len());
        let e = entry(&mut cache, &item.id, *st);
        if e.perceptual.is_none() {
            e.perceptual = picture(item, &root).and_then(|p| thumbnail::decode(&p).ok()).map(|img| duplicates::perceptual_hash(&img));
        }
    }
    set("Grouping", live.len(), live.len());
    let exact = duplicates::find_exact_duplicates_cached(&lib, &mut cache);
    let near = duplicates::find_near_duplicates_cached(&lib, threshold, &mut cache);
    logic::dup_groups(exact, near, |id| cache.entries.get(id).and_then(|e| e.perceptual))
}

impl App {
    /// Starts a scan of the library on a background thread.
    fn start_duplicate_scan(&mut self, lib: &Library) {
        let d = &mut self.duplicates;
        if d.root.as_deref() != Some(lib.root()) {
            *d = Duplicates { threshold: d.threshold, ..Default::default() };
            d.root = Some(lib.root().to_path_buf());
        }
        let progress = Arc::new(Mutex::new(Progress { phase: "Starting", ..Default::default() }));
        d.progress = Some(progress.clone());
        let (snapshot, threshold, cache, ctx) = (lib.clone(), d.threshold, d.cache.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let groups = scan(snapshot, threshold, &cache, &progress, &ctx);
            if let Ok(mut p) = progress.lock() {
                p.result = Some(groups);
            }
            ctx.request_repaint();
        });
    }

    pub(super) fn duplicates_page(&mut self, ui: &mut Ui, lib: &Library) {
        if self.duplicates.root.as_deref().is_some_and(|r| r != lib.root()) {
            self.duplicates = Duplicates { threshold: self.duplicates.threshold, ..Default::default() };
        }
        // A finished scan hands over its groups.
        let finished = self.duplicates.progress.as_ref().and_then(|p| p.lock().ok().and_then(|mut p| p.result.take()));
        if let Some(g) = finished {
            self.duplicates.groups = Some(g);
            self.duplicates.progress = None;
        }
        let running = self.duplicates.progress.as_ref().and_then(|p| p.lock().ok().map(|p| (p.phase, p.done, p.total)));
        let groups = self.duplicates.groups.as_ref().map(|g| logic::live_groups(g, |id| lib.item(id).is_some_and(|i| !i.is_deleted)));
        // Selection and arrow keys walk the members in page order.
        let mut seen = HashSet::new();
        self.order = groups.iter().flatten().flat_map(|g| g.ids.iter()).filter(|id| seen.insert(id.as_str())).cloned().collect();

        let area = ui.max_rect();
        let mut rescan = false;
        let mut pick: Option<Vec<String>> = None;
        egui::ScrollArea::vertical().id_salt("duplicates-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(GRID_PAD_X);
                ui.spacing_mut().item_spacing.x = 8.0;
                let label = if groups.is_some() { "Scan Again" } else { "Find Duplicates" };
                if ui.add_enabled(running.is_none(), egui::Button::new(label)).clicked() {
                    rescan = true;
                }
                ui.label(RichText::new("Similarity").color(t::TEXT_MUTED));
                // Right is stricter, as in Eagle; the value is the bits allowed to differ.
                let mut strict = 20 - self.duplicates.threshold.min(20) as i32;
                let s = ui.add_enabled(running.is_none(), egui::Slider::new(&mut strict, 0..=20).show_value(false));
                if s.on_hover_text("How alike similar pictures must be").drag_stopped() {
                    rescan = groups.is_some();
                }
                self.duplicates.threshold = (20 - strict) as u32;
                if let Some(g) = &groups
                    && !g.is_empty()
                {
                    let marked = logic::all_but_first(g);
                    if ui.button("Select All but First").on_hover_text("Selects every copy, keeping the first of each group").clicked() {
                        pick = Some(marked);
                    }
                    let sel = self.selected.len();
                    if ui.add_enabled(sel > 0, egui::Button::new(format!("Move {sel} to Trash"))).clicked() {
                        self.actions.push(Action::Trash(self.selected.clone(), true));
                    }
                }
            });
            ui.add_space(10.0);
            let x0 = area.left() + GRID_PAD_X;
            let p = ui.painter().clone();
            if let Some((phase, done, total)) = running {
                let (r, _) = ui.allocate_exact_size(vec2(area.width(), 40.0), Sense::hover());
                let bar = Rect::from_min_size(Pos2::new(x0, r.top() + 4.0), vec2(320.0, 6.0));
                p.rect_filled(bar, 3.0, t::BUTTON);
                let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height())), 3.0, t::PRIMARY);
                p.text(Pos2::new(x0, bar.bottom() + 14.0), Align2::LEFT_CENTER, format!("{phase}… {done}/{total}"), font(12.0), t::TEXT_MUTED);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            let Some(groups) = &groups else {
                let (r, _) = ui.allocate_exact_size(vec2(area.width(), 60.0), Sense::hover());
                p.text(
                    Pos2::new(x0, r.center().y),
                    Align2::LEFT_CENTER,
                    "Finds byte-identical files and similar-looking pictures. Nothing is changed until you trash items.",
                    font(13.0),
                    t::TEXT_MUTED,
                );
                return;
            };
            if groups.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(area.width(), 60.0), Sense::hover());
                p.text(Pos2::new(x0, r.center().y), Align2::LEFT_CENTER, "No duplicates found", font(14.0), t::TEXT_MUTED);
                return;
            }
            let n_items: usize = groups.iter().map(|g| g.ids.len()).sum();
            ui.horizontal(|ui| {
                ui.add_space(GRID_PAD_X);
                ui.label(RichText::new(format!("{} groups, {n_items} items", groups.len())).color(t::TEXT_MUTED));
            });
            ui.add_space(6.0);
            let mut jobs = Vec::new();
            let mut clicked: Option<(String, egui::Modifiers)> = None;
            let mut opened: Option<String> = None;
            for (gn, g) in groups.iter().enumerate() {
                let kind = if g.exact { "Identical files".to_string() } else { format!("Similar pictures (within {} of 64 bits)", g.distances.iter().max().unwrap_or(&0)) };
                ui.horizontal(|ui| {
                    ui.add_space(GRID_PAD_X);
                    ui.label(RichText::new(kind).color(t::TEXT_VALUE).size(12.0));
                });
                let per_row = (((area.width() - 2.0 * GRID_PAD_X) + 10.0) / (THUMB + 10.0)).floor().max(1.0) as usize;
                let rows = g.ids.len().div_ceil(per_row);
                let (block, _) = ui.allocate_exact_size(vec2(area.width(), rows as f32 * (THUMB + 44.0)), Sense::hover());
                for (n, id) in g.ids.iter().enumerate() {
                    let Some(item) = lib.item(id) else { continue };
                    let (row, col) = (n / per_row, n % per_row);
                    let cell = Rect::from_min_size(Pos2::new(x0 + col as f32 * (THUMB + 10.0), block.top() + row as f32 * (THUMB + 44.0)), vec2(THUMB, THUMB));
                    if !ui.is_rect_visible(cell.expand(THUMB)) {
                        continue;
                    }
                    let selected = self.selected.contains(id);
                    let resp = ui.interact(cell, ui.scope_id().with(("dup", gn, id.as_str())), Sense::click());
                    p.rect_filled(cell, 6.0, if resp.hovered() { t::CARD_HOVER } else { t::CARD_BORDER });
                    if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                        jobs.push(j);
                    }
                    self.draw_image(ui, lib, item, cell.shrink(4.0), false);
                    if selected {
                        p.rect_stroke(cell, 6.0, Stroke::new(2.0, t::PRIMARY), StrokeKind::Outside);
                    }
                    let first = if n == 0 { "Kept · " } else { "" };
                    let meta = if g.exact { format!("{first}{}", logic::format_size(item.size)) } else { format!("{first}{} bits", g.distances[n]) };
                    let name = super::galley(ui, &format!("{}.{}", item.name, item.ext), font(11.0), t::TEXT, THUMB);
                    p.galley(Pos2::new(cell.left(), cell.bottom() + 4.0), name, t::TEXT);
                    p.text(Pos2::new(cell.left(), cell.bottom() + 28.0), Align2::LEFT_CENTER, meta, mono(10.0), t::TEXT_MUTED);
                    if resp.double_clicked() {
                        opened = Some(id.clone());
                    } else if resp.clicked() {
                        clicked = Some((id.clone(), ui.input(|i| i.modifiers)));
                    }
                }
                ui.add_space(8.0);
            }
            self.frame_jobs.extend(jobs);
            if let Some((id, m)) = clicked {
                self.click(&id, m);
            }
            if let Some(id) = opened {
                self.selected = vec![id.clone()];
                self.viewer = Some(id);
            }
        });
        if let Some(ids) = pick {
            self.selected = ids;
        }
        if rescan {
            self.start_duplicate_scan(lib);
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
        let a = import("a.png", scene(1200, 900, false));
        let b = import("b.png", scene(1200, 900, false));
        let c = import("c.png", scene(1000, 750, false));
        let d = import("d.png", scene(1200, 900, true));
        let cache = Mutex::new(HashCache::default());
        let progress = Mutex::new(Progress::default());
        let ctx = egui::Context::default();
        let groups = scan(lib.clone(), THRESHOLD, &cache, &progress, &ctx);
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert!(groups[0].exact);
        assert_eq!(groups[0].ids, [a.clone(), b.clone()]);
        assert!(!groups[1].exact);
        assert_eq!(groups[1].ids, [a.clone(), b.clone(), c.clone()]);
        assert!(!groups.iter().any(|g| g.ids.contains(&d)));
        assert_eq!(logic::all_but_first(&groups), [b.clone(), c.clone()]);
        let p = progress.lock().unwrap();
        assert_eq!((p.done, p.total), (4, 4));
        // Every item was hashed once and kept in memory for the next scan.
        let cached = cache.lock().unwrap().clone();
        assert!(cached.entries.values().all(|e| e.perceptual.is_some()));
        drop(p);
        let again = scan(lib, THRESHOLD, &cache, &progress, &ctx);
        assert_eq!(again, groups);
        assert_eq!(*cache.lock().unwrap(), cached);
    }
}
