//! The progress modal for long operations (Eagle shows one for emptying
//! the Trash, importing and exporting .eaglepack files and refreshing
//! thumbnails): a title, a bar, "N of M", the item being worked on and,
//! where the work can stop between items, a Cancel button.
//!
//! A worker thread holds a [`Handle`] and reports through it; the UI thread
//! draws the modal (`App::progress_modal`) and, once the worker has set an
//! [`Outcome`], does what needs the UI (toast, forgetting thumbnails, undo
//! history, revealing a file). One operation runs at a time.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::app::App;
use crate::theme as t;
use crate::ui::dialogs::library_notices::Alert;

/// What a worker reports while it runs.
#[derive(Default)]
pub(crate) struct ProgressState {
    pub title: String,
    pub done: usize,
    /// 0 while the size of the job isn't known yet: the bar then moves
    /// without a fraction.
    pub total: usize,
    pub current: String,
    pub finished: Option<Outcome>,
}

/// How an operation ended, and what the UI thread does about it.
#[derive(Default)]
pub(crate) struct Outcome {
    /// The toast.
    pub message: String,
    /// Shown as an error toast instead of `message`.
    pub error: Option<String>,
    /// Shown in an alert instead of `message`.
    pub alert: Option<Alert>,
    /// Items whose cached pictures are dropped.
    pub forget: Vec<String>,
    /// Items were permanently deleted: the undo history no longer applies.
    pub deleted: bool,
    /// The library changed on disk (schedules a metadata backup).
    pub changed: bool,
    /// Files to show in Finder.
    pub reveal: Vec<PathBuf>,
}

/// The worker's side: a cheap clone that goes into the thread.
#[derive(Clone)]
pub(crate) struct Handle {
    state: Arc<Mutex<ProgressState>>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl Handle {
    fn state(&self) -> std::sync::MutexGuard<'_, ProgressState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Records progress and asks for a frame.
    pub fn step(&self, done: usize, current: impl Into<String>) {
        let mut s = self.state();
        s.done = done;
        s.current = current.into();
        drop(s);
        self.ctx.request_repaint();
    }

    /// Sets the number of steps once the worker knows it.
    pub fn set_total(&self, total: usize) {
        self.state().total = total;
        self.ctx.request_repaint();
    }

    /// Whether Cancel was pressed; workers check it between items.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Ends the operation; the UI thread closes the modal and applies the outcome.
    pub fn finish(&self, outcome: Outcome) {
        self.state().finished = Some(outcome);
        self.ctx.request_repaint();
    }
}

struct Run {
    state: Arc<Mutex<ProgressState>>,
    cancel: Arc<AtomicBool>,
    cancellable: bool,
}

/// The modal's state, kept per workspace.
#[derive(Default)]
pub(crate) struct ProgressModal {
    run: Option<Run>,
}

impl ProgressModal {
    /// Whether an operation is on.
    pub fn busy(&self) -> bool {
        self.run.is_some()
    }

    /// Opens the modal and returns the worker's handle. `total` is 0 when
    /// not yet known; `cancellable` shows the Cancel button.
    pub fn start(&mut self, ctx: &egui::Context, title: impl Into<String>, total: usize, cancellable: bool) -> Handle {
        let state = Arc::new(Mutex::new(ProgressState { title: title.into(), total, ..Default::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        self.run = Some(Run { state: state.clone(), cancel: cancel.clone(), cancellable });
        ctx.request_repaint();
        Handle { state, cancel, ctx: ctx.clone() }
    }
}

/// The share of the job done, kept between 0 and 1. `None` while the total
/// isn't known.
pub(crate) fn fraction(done: usize, total: usize) -> Option<f32> {
    (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0))
}

/// "3 of 10", or empty while the total isn't known. `done` never reads
/// past `total`.
pub(crate) fn count_text(done: usize, total: usize) -> String {
    if total == 0 { String::new() } else { crate::locale::trf("{done} of {total}", &[("done", &done.min(total)), ("total", &total)]) }
}

/// `name` cut to `max` characters with an ellipsis in the middle, keeping
/// the end (the extension).
pub(crate) fn shorten(name: &str, max: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max || max < 4 {
        return name.to_string();
    }
    let tail = (max - 1) / 2;
    let head = max - 1 - tail;
    format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>())
}

impl App {
    /// Draws the progress modal while an operation runs, and applies the
    /// outcome on the frame after it ends.
    pub(crate) fn progress_modal(&mut self, ctx: &egui::Context) {
        let Some(run) = &self.ws.op.run else { return };
        let (title, done, total, current, outcome) = {
            let mut s = run.state.lock().unwrap_or_else(|p| p.into_inner());
            (s.title.clone(), s.done, s.total, s.current.clone(), s.finished.take())
        };
        if let Some(o) = outcome {
            self.ws.op.run = None;
            crate::dock_progress::set(None);
            self.finish_operation(o);
            return;
        }
        crate::dock_progress::set(Some(fraction(done, total)));
        let cancellable = run.cancellable;
        let cancel = run.cancel.clone();
        let modal = egui::Modal::new(egui::Id::unique("progress-modal")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(title).size(15.0));
            ui.add_space(8.0);
            let bar = match fraction(done, total) {
                Some(f) => egui::ProgressBar::new(f),
                None => egui::ProgressBar::new(0.0).animate(true),
            };
            ui.add(bar);
            ui.add_space(4.0);
            ui.label(RichText::new(count_text(done, total)).color(t::p().text_muted));
            ui.label(RichText::new(shorten(&current, 44)).size(12.0).color(t::p().text_muted));
            if cancellable {
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::locale::tr("Cancel")).clicked() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                });
            }
        });
        if cancellable && modal.should_close() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Whether a long operation is on; a second one waits for the first
    /// (Eagle runs one at a time).
    pub(crate) fn operation_running(&mut self) -> bool {
        let busy = self.ws.op.busy();
        if busy {
            self.flash(crate::locale::tr("Wait for the current operation to finish"));
        }
        busy
    }

    fn finish_operation(&mut self, o: Outcome) {
        for id in &o.forget {
            self.ws.loader.forget(id);
        }
        if o.deleted {
            self.ws.selected.clear();
            self.ws.undo.clear();
            self.ws.redo.clear();
        }
        if o.changed {
            self.ws.backup.changed(std::time::Instant::now());
        }
        if let Some(alert) = o.alert {
            self.show_alert(alert);
        } else if let Some(e) = o.error {
            self.flash_error(e);
        } else {
            self.flash(o.message);
        }
        if !o.reveal.is_empty() {
            crate::open_with::reveal(&o.reveal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_is_clamped_and_needs_a_total() {
        assert_eq!(fraction(0, 0), None);
        assert_eq!(fraction(1, 4), Some(0.25));
        assert_eq!(fraction(9, 4), Some(1.0));
    }

    #[test]
    fn count_reads_n_of_m() {
        assert_eq!(count_text(3, 10), "3 of 10");
        assert_eq!(count_text(12, 10), "10 of 10");
        assert_eq!(count_text(0, 0), "");
    }

    #[test]
    fn long_names_keep_their_ends() {
        assert_eq!(shorten("photo.png", 20), "photo.png");
        let s = shorten("a-very-long-file-name-indeed.jpeg", 12);
        assert_eq!(s.chars().count(), 12);
        assert!(s.starts_with("a-ver") && s.ends_with(".jpeg") && s.contains('…'));
        assert_eq!(shorten("ab", 1), "ab");
    }

    #[test]
    fn a_handle_reports_and_cancels() {
        let mut modal = ProgressModal::default();
        let h = modal.start(&egui::Context::default(), "Working", 2, true);
        assert!(modal.busy() && !h.cancelled());
        h.step(1, "a.png");
        h.set_total(3);
        let run = modal.run.as_ref().unwrap();
        run.cancel.store(true, Ordering::Relaxed);
        assert!(h.cancelled());
        let s = run.state.lock().unwrap();
        assert_eq!((s.done, s.total, s.current.as_str()), (1, 3, "a.png"));
    }
}
