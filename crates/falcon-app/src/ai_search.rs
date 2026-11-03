//! AI Search: semantic text search and similar-image search on the Mac, with
//! a CLIP model the user downloads in Preferences. [`Engine`] is the app's
//! handle on the worker in `falcon-clip`, [`AiView`] is one library's search,
//! and [`pane`] draws Preferences ▸ AI Search. The `impl App` part is
//! `app/ai_search.rs`.
//!
//! Everything lives in falcon's app data (`ai-search/` under the storage
//! folder), never inside the `.library` folder. Only the model download uses
//! the network.

use std::collections::{HashMap, HashSet, VecDeque};
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
use falcon_library::model::SavedAiRules;
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
    /// The search as a saved filter holds it: what's shown, or what a saved
    /// filter asked for and can't be shown yet. A save writes these.
    pub rules: SavedAiRules,
    /// `rules` came from a saved filter.
    restored: bool,
    /// `rules` are held until the index is ready.
    pub waiting: bool,
    /// Why `rules` aren't applied, when waiting won't help.
    problem: Option<String>,
    /// A filter with both rules: the searches left, and what the first found.
    queue: VecDeque<Query>,
    so_far: Option<Vec<Hit>>,
}

/// What to do with a search's answer.
pub enum Answer {
    /// The grid has the results.
    Shown,
    /// One more search to run.
    Next(Query),
    /// No results, and this to tell the user, if anything.
    Failed(Option<String>),
}

/// The searches a filter's rules ask for: the description first, whose order
/// the results keep.
fn rule_queries(rules: &SavedAiRules) -> VecDeque<Query> {
    let text = rules.semantic_text().map(|text| Query::Text(text.to_string()));
    let item = rules.image_item().map(|id| Query::Item(id.to_string()));
    text.into_iter().chain(item).collect()
}

/// `first`, cut down to what `second` also found: rules narrow each other, as
/// every other filter rule does.
fn in_both(first: Vec<Hit>, second: &[Hit]) -> Vec<Hit> {
    let keep: HashSet<&str> = second.iter().map(|hit| hit.id.as_str()).collect();
    first.into_iter().filter(|hit| keep.contains(hit.id.as_str())).collect()
}

/// What to tell the user when a search fails.
pub fn search_failed(error: &SearchError) -> String {
    match error {
        SearchError::BadImage(_) => tr("That picture can't be read").to_string(),
        SearchError::NotReady => tr("AI Search isn't ready").to_string(),
        SearchError::NoItem => tr("That item is no longer in this library").to_string(),
        other => trf("AI Search failed: {error}", &[("error", other)]),
    }
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
        self.begin(SavedAiRules::default());
    }

    /// A new search takes over: `rules` is how a save would write it.
    pub fn begin(&mut self, rules: SavedAiRules) {
        self.rules = rules;
        self.restored = false;
        self.waiting = false;
        self.problem = None;
        self.queue.clear();
        self.so_far = None;
    }

    /// Takes a saved filter's rules. They stay here, and go back in a save,
    /// whether or not they ever run.
    pub fn restore(&mut self, rules: SavedAiRules) {
        self.begin(rules);
        self.restored = true;
        self.waiting = !rule_queries(&self.rules).is_empty();
        if self.rules.has_unreadable() {
            self.problem = Some(tr("This saved search can't be read, so it isn't applied. Saving the filter again keeps it.").to_string());
        }
    }

    /// The held rules are going to run: the first search to start.
    pub fn release(&mut self) -> Option<Query> {
        self.waiting = false;
        self.so_far = None;
        self.queue = rule_queries(&self.rules);
        self.queue.pop_front()
    }

    /// Takes a search's answer. A failure keeps the rules.
    pub fn answer(&mut self, answer: Result<Vec<Hit>, SearchError>) -> Answer {
        self.pending = None;
        match answer {
            Ok(hits) => {
                let hits = match self.so_far.take() {
                    Some(first) => in_both(first, &hits),
                    None => hits,
                };
                if let Some(next) = self.queue.pop_front() {
                    self.so_far = Some(hits);
                    return Answer::Next(next);
                }
                self.show(&hits);
                Answer::Shown
            }
            Err(error) => {
                let message = search_failed(&error);
                if !self.rules.is_active() {
                    self.clear_results();
                    return Answer::Failed(Some(message));
                }
                self.ranks = None;
                self.queue.clear();
                self.so_far = None;
                if error == SearchError::NotReady {
                    self.waiting = true;
                    return Answer::Failed(None);
                }
                self.problem = Some(message.clone());
                Answer::Failed(Some(message))
            }
        }
    }

    /// The filter bar shows the rules: the box can't say all of a saved
    /// filter's, and anything not applied has to be said.
    pub fn shows_chip(&self) -> bool {
        self.rules.is_active() && (self.restored || self.waiting || self.problem.is_some())
    }

    /// What the chip says the search is.
    pub fn chip_value(&self) -> String {
        let mut parts = Vec::new();
        if self.rules.semantic_on() {
            parts.push(self.rules.semantic_text().map_or_else(|| tr("Unreadable").to_string(), str::to_string));
        }
        if self.rules.image_on() {
            parts.push(match (&self.image, self.rules.image_item()) {
                (Some(label), Some(_)) => trf("Like {name}", &[("name", label)]),
                _ => tr("Unreadable").to_string(),
            });
        }
        parts.join(", ")
    }

    /// Why the rules aren't applied, in a line. `None` once they are.
    pub fn note(&self, status: &Status) -> Option<String> {
        if let Some(problem) = &self.problem {
            return Some(problem.clone());
        }
        self.waiting.then(|| trf("Can't apply yet. {reason}", &[("reason", &status_line(status))]))
    }

    /// An image search shown by a picture that isn't an item: the saved
    /// format holds only an item's id, so a save leaves it out.
    pub fn picture_not_saved(&self) -> bool {
        self.image.is_some() && self.rules.image.is_none()
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

    #[test]
    fn failures_are_said_plainly() {
        assert_eq!(search_failed(&SearchError::BadImage("x".into())), "That picture can't be read");
        assert_eq!(search_failed(&SearchError::Failed("out of memory".into())), "AI Search failed: out of memory");
    }

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

    fn write(view: &AiView) -> String {
        let mut rule = falcon_library::model::SavedFilter::rule_from_filter(&Default::default(), None, |_| None);
        view.rules.write_to(&mut rule);
        rule.to_string()
    }

    fn rules(rule: serde_json::Value) -> SavedAiRules {
        SavedAiRules::from_rule(&rule)
    }

    #[test]
    fn rules_that_cant_run_yet_are_held_said_and_saved() {
        let saved = rules(serde_json::json!({"semantic": {"keep": 1, "value": "black cat"}}));
        let mut view = AiView::default();
        view.restore(saved.clone());
        assert!(view.waiting && view.shows_chip() && !view.active());
        assert_eq!(view.chip_value(), "black cat");
        let no_model = Status { phase: Phase::NoModel, ..Default::default() };
        assert_eq!(view.note(&no_model).as_deref(), Some("Can't apply yet. The model isn't installed"));
        let indexing = Status { phase: Phase::Indexing, done: 1, total: 4, ..Default::default() };
        assert_eq!(view.note(&indexing).as_deref(), Some("Can't apply yet. Indexed 1 of 4 items"));
        assert_eq!(view.rules, saved);
        assert!(write(&view).contains(r#""semantic":{"keep":1,"value":"black cat"}"#));

        // Told not ready after all: held again, not dropped and not an error.
        assert!(matches!(view.release(), Some(Query::Text(text)) if text == "black cat"));
        assert!(!view.waiting);
        assert!(matches!(view.answer(Err(SearchError::NotReady)), Answer::Failed(None)));
        assert!(view.waiting);
        assert_eq!(view.rules, saved);

        // Ending the search is what drops them.
        view.clear_results();
        assert!(!view.waiting && !view.rules.is_active() && !view.shows_chip());
    }

    #[test]
    fn a_failed_search_keeps_its_rule_and_says_why() {
        let mut view = AiView::default();
        let saved = SavedAiRules::for_item("GONE");
        view.image = Some("GONE".into());
        view.restore(saved.clone());
        assert!(matches!(view.release(), Some(Query::Item(id)) if id == "GONE"));
        assert!(matches!(view.answer(Err(SearchError::NoItem)), Answer::Failed(Some(_))));
        assert_eq!(view.rules, saved);
        assert!(!view.waiting && view.shows_chip());
        assert_eq!(view.note(&Status::default()).as_deref(), Some("That item is no longer in this library"));
    }

    #[test]
    fn a_failed_search_with_no_rule_ends_as_before() {
        let mut view = AiView { image: Some("x.png".into()), ..Default::default() };
        assert!(view.picture_not_saved());
        assert!(matches!(view.answer(Err(SearchError::BadImage("x".into()))), Answer::Failed(Some(_))));
        assert_eq!(view.image, None);
        assert!(!view.shows_chip());
    }

    #[test]
    fn a_rule_that_cant_be_read_is_kept_and_not_run() {
        let saved = rules(serde_json::json!({"image": {"itemId": 7}}));
        let mut view = AiView::default();
        view.restore(saved.clone());
        assert!(!view.waiting && view.release().is_none());
        assert!(view.shows_chip());
        assert_eq!(view.chip_value(), "Unreadable");
        assert!(view.note(&Status::default()).unwrap().contains("can't be read"));
        assert_eq!(view.rules, saved);
        assert!(!view.picture_not_saved());
    }

    #[test]
    fn rules_that_are_off_show_nothing_and_still_save() {
        let saved = rules(serde_json::json!({"semantic": {"value": "", "keep": 1}}));
        let mut view = AiView::default();
        view.restore(saved);
        assert!(!view.waiting && !view.shows_chip());
        assert!(write(&view).contains(r#""semantic":{"value":"","keep":1}"#));
    }

    #[test]
    fn a_search_shown_from_the_box_saves_without_a_chip() {
        let mut view = AiView::default();
        view.begin(SavedAiRules::for_text("black cat"));
        assert!(!view.shows_chip());
        assert!(write(&view).contains(r#""semantic":{"value":"black cat"}"#));
        view.begin(SavedAiRules::for_item("ABC"));
        view.image = Some("a.png".into());
        assert!(!view.picture_not_saved());
        assert!(write(&view).contains(r#""image":{"itemId":"ABC"}"#));
    }

    /// The stand-in embedder falcon-clip's tests use: a picture's vector is
    /// its average colour and "red" and "green" point along theirs.
    struct Colours;

    impl Embedder for Colours {
        fn embed_images(&mut self, images: &[image::DynamicImage]) -> Result<Vec<Vec<f32>>, String> {
            Ok(images
                .iter()
                .map(|image| {
                    let rgb = image.to_rgb8();
                    let p = rgb.get_pixel(0, 0);
                    let v: Vec<f32> = p.0.iter().map(|c| f32::from(*c)).collect();
                    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                    v.iter().map(|x| x / n).collect()
                })
                .collect())
        }

        fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
            Ok(match text {
                "red" => vec![1.0, 0.0, 0.0],
                "green" => vec![0.0, 1.0, 0.0],
                _ => return Err("unknown word".to_string()),
            })
        }

        fn dim(&self) -> usize {
            3
        }

        fn device(&self) -> String {
            "Test".to_string()
        }
    }

    /// Runs what `ai_tick` runs, until the view has no search left.
    fn settle(view: &mut AiView, service: &Service) -> Answer {
        let mut query = view.release();
        loop {
            let answer = service.search_blocking(query.take().expect("a search"), SEARCH_LIMIT, Duration::from_secs(20));
            match view.answer(answer) {
                Answer::Next(next) => query = Some(next),
                done => return done,
            }
        }
    }

    /// From the JSON a library holds to the grid's results, through the real
    /// worker, with nothing held under the library lock meanwhile.
    #[test]
    fn saved_rules_apply_through_the_worker_once_it_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let mut library = falcon_library::Library::create(dir.path().join("T.library")).unwrap();
        let mut add = |name: &str, colour: [u8; 3]| {
            let path = dir.path().join(format!("{name}.png"));
            image::RgbImage::from_pixel(32, 32, image::Rgb(colour)).save(&path).unwrap();
            library.import_file(&path, &falcon_library::import::ImportOptions::default()).unwrap()
        };
        let red = add("red", [250, 10, 10]);
        let reddish = add("reddish", [220, 40, 30]);
        let green = add("green", [10, 250, 10]);
        let root = library.root().to_path_buf();
        let installed = Arc::new(AtomicBool::new(false));
        let check = installed.clone();
        let service = Service::start(Options {
            index_dir: dir.path().join("indexes"),
            model_id: "test-model".to_string(),
            dim: 3,
            throttle: false,
            is_installed: Box::new(move || check.load(Ordering::Relaxed)),
            loader: Box::new(|| Ok(Box::new(Colours))),
            notify: Box::new(|| {}),
        });
        service.set_library(Some(Arc::new(std::sync::Mutex::new(library))));
        service.set_enabled(true);

        // No model: the rule waits, and a search would say so.
        let wait = |ok: &dyn Fn(&Status) -> bool| {
            let start = std::time::Instant::now();
            while !ok(&service.status()) {
                assert!(start.elapsed() < Duration::from_secs(20), "{:?}", service.status());
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&|s| s.phase == Phase::NoModel);
        let mut view = AiView::default();
        view.restore(SavedAiRules::for_text("red"));
        assert!(!service.status().index_ready(&root));
        let answer = service.search_blocking(Query::Text("red".into()), SEARCH_LIMIT, Duration::from_secs(20));
        assert_eq!(answer.unwrap_err(), SearchError::NotReady);
        assert!(view.waiting && !view.active());

        // The model arrives: the rule runs once the library has been read.
        installed.store(true, Ordering::Relaxed);
        service.model_changed();
        wait(&|s| s.index_ready(&root) && s.done == 3);
        assert!(matches!(settle(&mut view, &service), Answer::Shown));
        let shown = view.narrow(None).unwrap();
        assert!(shown.contains(&red) && !shown.contains(&green));
        assert_eq!(view.rules.semantic_text(), Some("red"), "still what a save writes");

        // Visual Match from a saved rule.
        view.restore(SavedAiRules::for_item(&green));
        assert!(matches!(settle(&mut view, &service), Answer::Shown));
        assert_eq!(view.narrow(None).unwrap().into_iter().collect::<Vec<_>>(), std::slice::from_ref(&green));

        // Both rules narrow each other, in the description's order.
        let both = SavedAiRules { image: SavedAiRules::for_item(&reddish).image, semantic: SavedAiRules::for_text("red").semantic };
        view.restore(both);
        assert!(matches!(settle(&mut view, &service), Answer::Shown));
        let shown = view.narrow(None).unwrap();
        assert!(shown.contains(&reddish) && !shown.contains(&green));
        let none = SavedAiRules { image: SavedAiRules::for_item(&green).image, semantic: SavedAiRules::for_text("red").semantic };
        view.restore(none);
        assert!(matches!(settle(&mut view, &service), Answer::Shown));
        assert_eq!(view.narrow(None).unwrap().len(), 0);

        // An item that's gone says so and keeps its rule.
        view.restore(SavedAiRules::for_item("GONE"));
        assert!(matches!(settle(&mut view, &service), Answer::Failed(Some(_))));
        assert_eq!(view.rules.image_item(), Some("GONE"));
        assert!(!view.active());
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
