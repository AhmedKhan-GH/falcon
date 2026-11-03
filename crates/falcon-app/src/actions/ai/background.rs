//! A run with AI steps as a background task. The worker reports here; the
//! window stays usable, a small panel (and the spinner beside the library
//! name) shows the progress with Cancel, and `force_close::tasks_running`
//! counts the run, so a library switch, a close and a quit treat it as they
//! do imports. It belongs to the workspace it started in and works on that
//! library whichever is shown.
//!
//! One AI run goes at a time across all open libraries: a local model answers
//! one request at a time anyway, and the report of one run would otherwise
//! interleave with the next. A second is refused with a message. Rule-only
//! actions and long operations don't wait for it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, RichText, vec2};
use falcon_library::undo::Transaction;

use super::{Ended, Place, Plan, Report, execute, outcome};
use crate::actions::Run;
use crate::app::{App, Shared, Workspace, lock};
use crate::locale::{tr, trf};
use crate::theme as t;
use crate::ui::dialogs::progress::{count_text, fraction, shorten};

/// How a worker ended.
pub(crate) enum Finished {
    Report(Box<Report>),
    /// The worker panicked; nothing more is known.
    Crashed,
}

/// What the worker reports.
#[derive(Default)]
struct Live {
    done: usize,
    current: String,
    finished: Option<Finished>,
}

/// A run in a workspace: the UI's side of the worker.
pub(crate) struct BackgroundRun {
    name: String,
    total: usize,
    started: Instant,
    live: Arc<Mutex<Live>>,
    cancel: Arc<AtomicBool>,
}

impl BackgroundRun {
    /// Starts the worker. It holds its own handle on the library.
    pub fn start(ctx: &egui::Context, shared: Shared, run: Run, plan: Plan) -> BackgroundRun {
        let live = Arc::new(Mutex::new(Live::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let this = BackgroundRun { name: run.name.clone(), total: run.ids.len(), started: Instant::now(), live: live.clone(), cancel: cancel.clone() };
        let ctx = ctx.clone();
        let label = crate::actions::app::run_label(&run);
        std::thread::spawn(move || {
            let set = |update: &mut dyn FnMut(&mut Live)| {
                update(&mut live.lock().unwrap_or_else(|p| p.into_inner()));
                ctx.request_repaint();
            };
            let mut progress = |n: usize, item: &str| {
                set(&mut |l| {
                    l.done = n;
                    l.current = item.to_string();
                });
            };
            // A panic must not leave the run showing for good.
            let work = std::panic::AssertUnwindSafe(|| execute(&shared, &run, &plan, &label, &cancel, &mut progress));
            let mut end = Some(std::panic::catch_unwind(work).map_or(Finished::Crashed, |r| Finished::Report(Box::new(r))));
            set(&mut |l| l.finished = end.take());
        });
        this
    }

    fn live(&self) -> std::sync::MutexGuard<'_, Live> {
        self.live.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether the worker is still going.
    pub fn working(&self) -> bool {
        self.live().finished.is_none()
    }

    /// Asks the worker to stop. Finished items are still applied.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn take_finished(&self) -> Option<Finished> {
        self.live().finished.take()
    }

    /// The spinner's tooltip.
    pub fn tooltip(&self) -> String {
        format!("{}\n{}", trf("Running {name}", &[("name", &self.name)]), count_text(self.live().done, self.total))
    }

    /// The items done and in all.
    fn counts(&self) -> (usize, usize) {
        (self.live().done, self.total)
    }
}

/// The name a workspace's library goes by in messages.
fn library_name(ws: &Workspace) -> String {
    ws.root.as_deref().and_then(|r| r.file_stem()).map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

/// What refuses another AI run while one is on in any of the `open` libraries.
fn second_run_message<'a>(mut open: impl Iterator<Item = &'a Workspace>) -> Option<String> {
    open.find_map(|ws| {
        let run = ws.ai_run.as_ref()?;
        Some(trf("{name} is still running in {library}. Wait for it to finish or cancel it before starting another AI run.", &[("name", &run.name), ("library", &library_name(ws))]))
    })
}

/// The inspector's text for an item a run just changed. What the user typed
/// and hasn't committed stays, and goes in when the field loses focus; a field
/// left alone follows the item, or its old text would be written back then.
fn follow_notes(ws: &mut Workspace, tx: &Transaction) {
    let Some(edit) = &mut ws.edit else { return };
    let Some(old) = tx.old_item(&edit.id) else { return };
    if edit.notes == old.annotation
        && let Some(lib) = &ws.lib
        && let Some(item) = lock(lib).item(&edit.id)
    {
        edit.notes.clone_from(&item.annotation);
    }
}

/// Files a finished run in its workspace: the undo history gets its step
/// and the inspector follows. `None` while it runs or when there is none.
pub(crate) fn settle(ws: &mut Workspace, place: &Place) -> Option<Ended> {
    let run = ws.ai_run.as_ref()?;
    let finished = run.take_finished()?;
    let name = run.name.clone();
    ws.ai_run = None;
    let mut ended = match finished {
        Finished::Report(report) => outcome(&name, *report, place),
        Finished::Crashed => Ended { error: Some(trf("{name} stopped on an internal error.", &[("name", &name)])), ..Default::default() },
    };
    if let Some(tx) = ended.undo.take() {
        follow_notes(ws, &tx);
        crate::app::actions::push_capped(&mut ws.undo, tx);
        ws.redo.clear();
    }
    if ended.changed {
        ws.backup.changed(Instant::now());
    }
    Some(ended)
}

/// A workspace is closing: files a finished run, whose undo step goes with
/// the library. A close that didn't wait (its folder vanished) stops the run.
pub(crate) fn settle_closing(ws: &mut Workspace) -> Option<Ended> {
    if let Some(run) = ws.ai_run.as_ref().filter(|r| r.working()) {
        run.cancel();
        return None;
    }
    let place = Place::Closed(library_name(ws));
    settle(ws, &place)
}

impl App {
    /// Starts a run with AI steps in the shown library. The reducer calls
    /// this; the run's undo step comes back when it ends (`ai_runs_tick`).
    pub(crate) fn start_ai_run(&mut self, run: Run) {
        let Some(shared) = self.ws.lib.clone() else { return };
        // The 30-item question can outlast another run's start.
        if self.refuse_second_ai_run() {
            return;
        }
        let Some(plan) = Plan::new(crate::ai_models::client(&self.prefs.ai_models)) else {
            if let Some(problem) = crate::ai_models::setup_problem(&self.prefs.ai_models) {
                self.flash_error(tr(problem));
            }
            return;
        };
        self.ws.ai_run = Some(BackgroundRun::start(&self.ctx, shared, run, plan));
    }

    /// Says no, with the reason, while an AI run is on in any open library.
    pub(crate) fn refuse_second_ai_run(&mut self) -> bool {
        let Some(message) = second_run_message(self.others.iter(&self.ws)) else { return false };
        self.flash(message);
        true
    }

    /// Once a frame, with no library locked: files finished runs and tells
    /// the user. A library with a close pending waits for `close_workspace`,
    /// so its report doesn't offer an undo that is about to leave.
    pub(crate) fn ai_runs_tick(&mut self) {
        let closing = self.force_close.as_ref().map(|p| p.root.clone());
        let closes = |ws: &Workspace| closing.as_deref().is_some_and(|c| ws.root.as_deref().is_some_and(|r| crate::app::library::same_path(r, c)));
        let mut ended = Vec::new();
        if !closes(&self.ws) {
            ended.extend(settle(&mut self.ws, &Place::Shown));
        }
        for ws in self.others.parked_mut() {
            if !closes(ws) {
                let place = Place::Parked(library_name(ws));
                ended.extend(settle(ws, &place));
            }
        }
        for e in ended {
            self.present_ai_run(e);
        }
        if self.alert.is_none()
            && let Some(alert) = self.user_actions.ai_reports.pop_front()
        {
            self.show_alert(alert);
        }
    }

    /// Tells the user how a run ended. An alert waits for one already up.
    pub(crate) fn present_ai_run(&mut self, e: Ended) {
        if self.prefs.notifications {
            let text = e.alert.as_ref().map(|a| a.title.as_str()).or(e.error.as_deref()).unwrap_or(&e.message);
            crate::notify::post(&crate::notify::Notice::action_finished(text));
        }
        if let Some(alert) = e.alert {
            self.user_actions.ai_reports.push_back(alert);
        } else if let Some(error) = e.error {
            self.flash_error(error);
        } else if e.undo_toast {
            self.flash_undo(e.message);
        } else {
            self.flash(e.message);
        }
    }

    /// The running AI step's items done and in all, for the Dock.
    pub(crate) fn ai_run_progress(&self) -> Option<(usize, usize)> {
        self.others.iter(&self.ws).find_map(|ws| ws.ai_run.as_ref().filter(|r| r.working())).map(BackgroundRun::counts)
    }

    /// The panel with the progress and Cancel. It floats over any page, the
    /// viewer included, and shows a run in another library too, so Cancel is
    /// always in reach. Modal dialogs cover it.
    pub(crate) fn ai_run_panel(&mut self, ctx: &egui::Context) {
        if self.modes.presenting {
            return;
        }
        let shown = self.ws.ai_run.is_some();
        let Some(ws) = self.others.iter(&self.ws).find(|ws| ws.ai_run.as_ref().is_some_and(BackgroundRun::working)) else { return };
        let Some(run) = &ws.ai_run else { return };
        let title = if shown { trf("Running {name}", &[("name", &run.name)]) } else { trf("Running {name} in {library}", &[("name", &run.name), ("library", &library_name(ws))]) };
        draw_panel(ctx, run, &title);
    }
}

/// Draws the panel, and returns where its Cancel button is, if showing.
fn draw_panel(ctx: &egui::Context, run: &BackgroundRun, title: &str) -> Option<egui::Rect> {
    let (done, current) = {
        let l = run.live();
        (l.done, l.current.clone())
    };
    let count = count_text(done, run.total);
    let status = match crate::progress_eta::remaining(run.started.elapsed(), done, run.total).map(crate::progress_eta::format) {
        Some(left) => format!("{count}  ·  {left}"),
        None => count,
    };
    let p = t::p();
    let mut button = None;
    egui::Area::new(egui::Id::unique("ai-run-panel")).anchor(egui::Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0)).order(egui::Order::Middle).show(ctx, |ui| {
        egui::Frame::new().fill(p.panel).stroke(egui::Stroke::new(1.0, p.divider)).corner_radius(8).inner_margin(10).show(ui, |ui| {
            ui.set_width(280.0);
            ui.label(RichText::new(shorten(title, 44)).size(13.0).color(p.text));
            ui.add_space(4.0);
            let bar = match fraction(done, run.total) {
                Some(f) => egui::ProgressBar::new(f),
                None => egui::ProgressBar::new(0.0).animate(true),
            };
            ui.add(bar.desired_height(4.0));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    ui.label(RichText::new(status).size(12.0).color(p.text_muted));
                    ui.label(RichText::new(shorten(&current, 30)).size(12.0).color(p.text_muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if run.cancelling() {
                        ui.label(RichText::new(tr("Stopping tasks…")).size(12.0).color(p.text_muted));
                    } else {
                        let cancel = ui.button(tr("Cancel"));
                        button = Some(cancel.rect);
                        if cancel.clicked() {
                            run.cancel();
                        }
                    }
                });
            });
        });
    });
    ctx.request_repaint_after(std::time::Duration::from_secs(1));
    button
}

#[cfg(test)]
mod tests;
