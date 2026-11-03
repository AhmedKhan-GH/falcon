//! AI Search: semantic text search and similar-image search on the Mac, with
//! a CLIP model the user downloads in Preferences. [`Engine`] is the app's
//! handle on the worker in `falcon-clip`, [`AiView`] is one library's search,
//! and [`pane`] draws Preferences ▸ AI Search. The `impl App` part is
//! `app/ai_search.rs`.
//!
//! Everything lives in falcon's app data (`ai-search/` under the storage
//! folder), never inside the `.library` folder. Only the model download uses
//! the network.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use eframe::egui::{self, Rect, RichText};
use falcon_clip::clip::ClipEmbedder;
use falcon_clip::model::{self, Download, DownloadState};
use falcon_clip::service::Options;
use falcon_clip::{Embedder, Hit, Phase, Query, SearchError, Service, Status};
use falcon_server::{AiQuery, AiStatus};

use crate::locale::{tr, tr_key, trf};
use crate::logic::format_size;
use crate::prefs::{Draft, Page};
use crate::theme as t;

/// The most results a search shows.
pub const SEARCH_LIMIT: usize = 500;

/// How long an API search waits, with room for the model to load.
const API_WAIT: Duration = Duration::from_secs(120);

/// Where AI Search keeps its files.
fn data_dir() -> Option<PathBuf> {
    eframe::storage_dir("Falcon").map(|d| d.join("ai-search"))
}

/// The worker, the download and what they share.
pub struct Engine {
    service: Service,
    download: Download,
    model_base: PathBuf,
    installed: AtomicBool,
    paused: AtomicBool,
    /// Remove Model is asking for confirmation.
    confirm_remove: AtomicBool,
    notify: egui::Context,
}

impl Engine {
    /// Starts the worker, which does nothing until it is given a library and
    /// turned on. No model is loaded and nothing is downloaded here.
    pub fn start(ctx: &egui::Context) -> Option<Arc<Engine>> {
        let base = data_dir()?;
        let model_base = base.join("model");
        let dir = model::dir_in(&model_base);
        let (for_load, for_check) = (dir.clone(), dir.clone());
        let repaint = ctx.clone();
        let service = Service::start(Options {
            index_dir: base.join("indexes"),
            model_id: model::ID.to_string(),
            dim: 512,
            throttle: true,
            is_installed: Box::new(move || model::is_installed(&for_check)),
            loader: Box::new(move || ClipEmbedder::load(&for_load).map(|e| Box::new(e) as Box<dyn Embedder>)),
            notify: Box::new(move || repaint.request_repaint()),
        });
        Some(Arc::new(Engine {
            service,
            download: Download::default(),
            installed: AtomicBool::new(model::is_installed(&dir)),
            model_base,
            paused: AtomicBool::new(false),
            confirm_remove: AtomicBool::new(false),
            notify: ctx.clone(),
        }))
    }

    pub fn status(&self) -> Status {
        self.service.status()
    }

    pub fn set_library(&self, library: Option<falcon_server::Shared>) {
        self.service.set_library(library);
    }

    pub fn set_enabled(&self, on: bool) {
        self.service.set_enabled(on);
    }

    /// Starts a search; the answer arrives on the returned channel.
    pub fn search(&self, query: Query) -> Receiver<Result<Vec<Hit>, SearchError>> {
        self.service.search(query, SEARCH_LIMIT)
    }

    /// Notices a download that has finished.
    pub fn tick(&self) {
        if self.download.state() == DownloadState::Finished {
            self.download.reset();
            self.installed.store(true, Ordering::Relaxed);
            self.service.model_changed();
        }
    }

    fn start_download(&self) {
        let repaint = self.notify.clone();
        self.download.start(model::dir_in(&self.model_base), move || repaint.request_repaint());
    }

    /// Removes the model and every library's index.
    fn remove(&self) -> Result<(), String> {
        model::remove(&self.model_base).map_err(|e| e.to_string())?;
        self.installed.store(false, Ordering::Relaxed);
        self.service.forget_indexes();
        self.service.model_changed();
        Ok(())
    }
}

impl falcon_server::AiSearch for Engine {
    fn status(&self) -> AiStatus {
        api_status(&self.service.status(), self.installed.load(Ordering::Relaxed))
    }

    fn search(&self, query: AiQuery, limit: usize) -> Result<Vec<(String, f32)>, String> {
        let query = match query {
            AiQuery::Text(text) => Query::Text(text),
            AiQuery::Image(bytes) => Query::ImageBytes(bytes),
            AiQuery::Item(id) => Query::Item(id),
        };
        let hits = self.service.search_blocking(query, limit, API_WAIT).map_err(|e| e.to_string())?;
        Ok(hits.into_iter().map(|h| (h.id, h.score)).collect())
    }
}

/// How the API reports a status.
fn api_status(status: &Status, installed: bool) -> AiStatus {
    AiStatus {
        installed,
        ready: status.searchable() && status.phase != Phase::Loading,
        starting: status.phase == Phase::Loading,
        syncing: status.phase == Phase::Indexing,
        done: status.done,
        total: status.total,
        error: status.error.clone(),
    }
}

/// One library's AI search: what the grid shows and what is under way.
#[derive(Default)]
pub struct AiView {
    /// The search box reads what is typed as a description of a picture.
    pub mode: bool,
    /// What an image search is by, while one is shown.
    pub image: Option<String>,
    /// Each result's place, best first. `None` when no AI search is shown.
    pub ranks: Option<HashMap<String, usize>>,
    /// The search under way. A newer search replaces it.
    pub pending: Option<Receiver<Result<Vec<Hit>, SearchError>>>,
    /// Where the search box is, for images dropped on it.
    pub search_rect: Option<Rect>,
}

impl AiView {
    pub fn active(&self) -> bool {
        self.ranks.is_some()
    }

    /// Back to no AI search. The box stays in AI mode if it was.
    pub fn clear_results(&mut self) {
        self.image = None;
        self.ranks = None;
        self.pending = None;
    }

    pub fn show(&mut self, hits: &[Hit]) {
        self.ranks = Some(ranks(hits));
    }

    /// `only`, the ids the view is limited to, cut down to the results.
    pub fn narrow(&self, only: Option<HashSet<String>>) -> Option<HashSet<String>> {
        let Some(ranks) = &self.ranks else { return only };
        Some(match only {
            Some(only) => only.into_iter().filter(|id| ranks.contains_key(id)).collect(),
            None => ranks.keys().cloned().collect(),
        })
    }

    /// Sorts the grid's items best result first, whatever the view's sort.
    pub fn sort(&self, order: &mut [usize], id_of: impl Fn(usize) -> String) {
        if let Some(ranks) = &self.ranks {
            order.sort_by_key(|&i| ranks.get(&id_of(i)).copied().unwrap_or(usize::MAX));
        }
    }
}

fn ranks(hits: &[Hit]) -> HashMap<String, usize> {
    hits.iter().enumerate().map(|(place, hit)| (hit.id.clone(), place)).collect()
}

/// How indexing stands, in a line.
pub fn status_line(status: &Status) -> String {
    let progress = trf("Indexed {done} of {total} items", &[("done", &status.done), ("total", &status.total)]);
    match status.phase {
        Phase::Off => tr("AI Search is turned off").to_string(),
        Phase::NoModel => tr("The model isn't installed").to_string(),
        Phase::Loading => tr("Loading the model").to_string(),
        Phase::Indexing => progress,
        Phase::Paused => trf("Paused at {done} of {total} items", &[("done", &status.done), ("total", &status.total)]),
        Phase::Ready => progress,
        Phase::Failed => trf("AI Search failed: {error}", &[("error", &status.error.clone().unwrap_or_default())]),
    }
}

/// What the toolbar button says when hovered.
pub fn button_tip(status: &Status) -> String {
    if status.searchable() {
        format!("{}\n{}", tr("AI Search: describe a picture, in English"), status_line(status))
    } else {
        format!("{}\n{}", tr("AI Search"), status_line(status))
    }
}

/// How far along indexing is, while it's running.
pub fn progress(status: &Status) -> Option<f32> {
    (status.phase == Phase::Indexing && status.total > 0).then(|| status.done as f32 / status.total as f32)
}

/// Preferences ▸ AI Search.
pub fn pane(p: &mut Page, d: &mut Draft, engine: Option<&Engine>) {
    let keywords = ["ai", "clip", "model", "download", "semantic", "similar", "visual match", "image search", "licence", "license"];
    p.block(tr_key("Model"), &keywords, |ui| {
        let Some(engine) = engine else {
            ui.label(tr("AI Search needs Falcon's app data folder, which can't be found."));
            return;
        };
        ui.label(RichText::new(model::NAME).color(t::p().text));
        crate::prefs::note(ui, &trf("Licence: {licence}", &[("licence", &model::LICENCE)]));
        crate::prefs::note(ui, &trf("Download size: {size}", &[("size", &format_size(model::download_size()))]));
        crate::prefs::note(ui, &trf("Source: {page}, commit {commit}", &[("page", &model::page()), ("commit", &&model::REVISION[..8])]));
        ui.add_space(6.0);
        model_controls(ui, engine);
        ui.add_space(6.0);
        crate::prefs::note(ui, tr("Nothing is downloaded until you choose to. Only this download uses the network: your images and searches never leave this Mac."));
    });
    let keywords = ["ai", "index", "indexing", "progress", "pause", "rebuild", "english", "search"];
    p.block(tr_key("AI Search"), &keywords, |ui| {
        ui.checkbox(&mut d.prefs.ai_search, tr("Use AI Search"));
        crate::prefs::note(ui, tr("Find pictures by describing them or by showing a similar one. Descriptions should be in English."));
        let Some(engine) = engine else { return };
        if !engine.installed.load(Ordering::Relaxed) {
            return;
        }
        ui.add_space(6.0);
        let status = engine.status();
        ui.label(status_line(&status));
        if status.total > 0 {
            ui.add(egui::ProgressBar::new(status.done as f32 / status.total as f32).desired_width(320.0));
        }
        if status.skipped > 0 {
            crate::prefs::note(ui, &crate::locale::tr_n(status.skipped, "{n} item has no picture to read", "{n} items have no picture to read"));
        }
        if let Some(device) = &status.device {
            let speed = if status.rate > 0.0 && status.phase == Phase::Indexing { format!(", {:.0}/s", status.rate) } else { String::new() };
            crate::prefs::note(ui, &format!("{}{speed}", trf("Running on {device}", &[("device", device)])));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let paused = engine.paused.load(Ordering::Relaxed);
            if ui.add_enabled(status.searchable(), egui::Button::new(if paused { tr("Resume Indexing") } else { tr("Pause Indexing") })).clicked() {
                engine.paused.store(!paused, Ordering::Relaxed);
                engine.service.set_paused(!paused);
            }
            if ui.add_enabled(status.searchable(), egui::Button::new(tr("Rebuild Index"))).on_hover_text(tr("Read every picture in this library again")).clicked() {
                engine.service.rebuild();
            }
        });
    });
}

/// The download, cancel and remove buttons.
fn model_controls(ui: &mut egui::Ui, engine: &Engine) {
    match engine.download.state() {
        DownloadState::Running { done, total } => {
            ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32).desired_width(320.0).text(format!("{} / {}", format_size(done), format_size(total))));
            if ui.button(tr("Cancel Download")).clicked() {
                engine.download.cancel();
            }
            return;
        }
        DownloadState::Checking => {
            ui.label(tr("Checking the download"));
            return;
        }
        DownloadState::Failed(error) => {
            ui.colored_label(t::p().icon_red, trf("The download failed: {error}", &[("error", &error)]));
        }
        DownloadState::Cancelled => {
            crate::prefs::note(ui, tr("Download cancelled. What was fetched is kept, and the next download continues from there."));
        }
        DownloadState::Idle | DownloadState::Finished => {}
    }
    if !engine.installed.load(Ordering::Relaxed) {
        if ui.button(trf("Download Model ({size})", &[("size", &format_size(model::download_size()))])).clicked() {
            engine.download.reset();
            engine.start_download();
        }
        return;
    }
    ui.label(tr("The model is installed."));
    if engine.confirm_remove.load(Ordering::Relaxed) {
        ui.label(tr("Remove the model and every library's index?"));
        ui.horizontal(|ui| {
            if ui.button(tr("Remove")).clicked() {
                engine.confirm_remove.store(false, Ordering::Relaxed);
                if let Err(error) = engine.remove() {
                    log_removal_failure(&error);
                }
            }
            if ui.button(tr("Cancel")).clicked() {
                engine.confirm_remove.store(false, Ordering::Relaxed);
            }
        });
    } else if ui.button(tr("Remove Model…")).clicked() {
        engine.confirm_remove.store(true, Ordering::Relaxed);
    }
}

fn log_removal_failure(error: &str) {
    crate::errlog::error(&format!("AI Search: removing the model failed: {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, score: f32) -> Hit {
        Hit { id: id.to_string(), score }
    }

    #[test]
    fn results_keep_their_order_and_limit_the_view() {
        let mut view = AiView::default();
        assert!(!view.active());
        assert_eq!(view.narrow(None), None, "no AI search leaves the view alone");
        view.show(&[hit("b", 0.9), hit("a", 0.5)]);
        assert!(view.active());
        let only = view.narrow(None).unwrap();
        assert_eq!(only.len(), 2);
        let scope: HashSet<String> = ["a".to_string(), "c".to_string()].into_iter().collect();
        assert_eq!(view.narrow(Some(scope)).unwrap().into_iter().collect::<Vec<_>>(), ["a"], "the view's own limit still holds");
        let ids = ["a", "b", "c"];
        let mut order = vec![0, 1, 2];
        view.sort(&mut order, |i| ids[i].to_string());
        assert_eq!(order, [1, 0, 2], "best first, others last");
    }

    #[test]
    fn clearing_keeps_the_mode_and_drops_the_rest() {
        let mut view = AiView { mode: true, image: Some("x".into()), ..Default::default() };
        view.show(&[hit("a", 1.0)]);
        view.clear_results();
        assert!(view.mode);
        assert!(!view.active());
        assert_eq!(view.image, None);
    }

    #[test]
    fn progress_shows_only_while_indexing() {
        let mut status = Status { phase: Phase::Indexing, done: 1, total: 4, ..Default::default() };
        assert_eq!(progress(&status), Some(0.25));
        status.phase = Phase::Ready;
        assert_eq!(progress(&status), None);
    }

    #[test]
    fn the_api_is_told_how_things_stand() {
        let status = |phase| Status { phase, done: 1, total: 2, ..Default::default() };
        let off = api_status(&status(Phase::Off), true);
        assert!(off.installed && !off.ready);
        let none = api_status(&status(Phase::NoModel), false);
        assert!(!none.installed && !none.ready);
        let loading = api_status(&status(Phase::Loading), true);
        assert!(loading.starting && !loading.ready);
        let indexing = api_status(&status(Phase::Indexing), true);
        assert!(indexing.ready && indexing.syncing && !indexing.starting);
        assert_eq!((indexing.done, indexing.total), (1, 2));
        let ready = api_status(&status(Phase::Ready), true);
        assert!(ready.ready && !ready.syncing);
    }
}
