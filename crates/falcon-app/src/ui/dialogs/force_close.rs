//! Closing a library while tasks run in it. Several libraries stay open at
//! once and a library in the background keeps its imports and operations
//! going, so a switch only says so (`switch_warning.rs`) and closing is what
//! stops them, which needs this warning. The close itself
//! waits until the workers have stopped, so none writes to the library
//! after it was saved.

use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::app::{App, Workspace};
use crate::locale::{tr, trf};
use crate::theme as t;

/// A close waiting for its library's tasks.
pub(crate) struct Pending {
    /// The library's folder (a switcher position would shift).
    pub root: PathBuf,
    /// The tasks were told to stop; the close follows once they have.
    pub stopping: bool,
}

/// What the workspace has going, as counters.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(crate) struct Counts {
    pub import_total: usize,
    pub import_done: usize,
    /// A progress-modal operation, exports included.
    pub op_working: bool,
    /// A run with AI steps, which has no modal.
    pub ai_working: bool,
}

impl Counts {
    pub fn running(&self) -> bool {
        (self.import_total > 0 && self.import_done < self.import_total) || self.op_working || self.ai_working
    }
}

/// What to do on a frame with a close pending.
#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    /// Close the library now.
    Close,
    /// Ask whether to stop the tasks.
    Ask,
    /// The tasks were told to stop; wait for them.
    Wait,
}

/// The next step for a pending close.
pub(crate) fn step(stopping: bool, running: bool) -> Step {
    match (running, stopping) {
        (false, _) => Step::Close,
        (true, false) => Step::Ask,
        (true, true) => Step::Wait,
    }
}

/// The dialog's text.
pub(crate) fn body(library: &str) -> String {
    trf("Tasks are still running in {library}. Closing it stops them. Work already done is kept.", &[("library", &library)])
}

/// Whether imports, a long operation, an export or an AI run are running in `ws`,
/// shown or in the background.
pub(crate) fn tasks_running(ws: &Workspace) -> bool {
    let (import_total, import_done) = {
        let p = ws.progress.lock().unwrap_or_else(|p| p.into_inner());
        (p.total, p.done)
    };
    Counts { import_total, import_done, op_working: ws.op.working(), ai_working: ws.ai_run.as_ref().is_some_and(|r| r.working()) }.running()
}

pub(crate) fn stop_tasks(ws: &Workspace) {
    ws.progress.lock().unwrap_or_else(|p| p.into_inner()).cancel = true;
    ws.op.cancel();
    if let Some(run) = &ws.ai_run {
        run.cancel();
    }
}

impl App {
    /// The sidebar's close button: closes at once, or asks first when the
    /// library has tasks running.
    pub(crate) fn request_close(&mut self, i: usize) {
        let Some(ws) = self.others.iter(&self.ws).nth(i) else { return };
        match (&ws.root, tasks_running(ws)) {
            (Some(root), true) => self.force_close = Some(Pending { root: root.clone(), stopping: false }),
            _ => self.close_workspace(i),
        }
    }

    /// Draws the question, and closes the library once its tasks have stopped.
    pub(crate) fn force_close_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.force_close else { return };
        let (root, stopping) = (pending.root.clone(), pending.stopping);
        let Some(i) = self.position_of(&root) else {
            self.force_close = None;
            return;
        };
        let running = self.others.iter(&self.ws).nth(i).is_some_and(tasks_running);
        let name = root.file_stem().map_or_else(|| root.display().to_string(), |s| s.to_string_lossy().into_owned());
        match step(stopping, running) {
            Step::Close => {
                self.force_close = None;
                self.close_workspace(i);
            }
            Step::Wait => {
                egui::Modal::new(egui::Id::unique("force-close-wait")).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new(tr("Close Library While Tasks Run")).size(15.0));
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr("Stopping tasks…")).color(t::p().text_muted));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Step::Ask => {
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::unique("force-close")).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new(tr("Close Library While Tasks Run")).size(15.0));
                    ui.add_space(4.0);
                    ui.label(RichText::new(body(&name)).color(t::p().text_muted));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new(tr("Stop Tasks and Close")).color(t::p().icon_red)).clicked() {
                            answer = Some(true);
                        }
                        if ui.button(tr("Cancel")).clicked() {
                            answer = Some(false);
                        }
                    });
                });
                if modal.should_close() {
                    answer = Some(false);
                }
                match answer {
                    Some(true) => {
                        if let Some(ws) = self.others.iter(&self.ws).nth(i) {
                            stop_tasks(ws);
                        }
                        if let Some(p) = &mut self.force_close {
                            p.stopping = true;
                        }
                        ctx.request_repaint();
                    }
                    Some(false) => self.force_close = None,
                    None => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_say_whether_anything_runs() {
        assert!(!Counts::default().running());
        assert!(Counts { import_total: 5, import_done: 2, ..Default::default() }.running());
        assert!(!Counts { import_total: 5, import_done: 5, ..Default::default() }.running());
        assert!(Counts { op_working: true, ..Default::default() }.running());
        assert!(Counts { ai_working: true, ..Default::default() }.running());
    }

    #[test]
    fn a_close_asks_only_while_tasks_run_and_waits_after_the_answer() {
        assert_eq!(step(false, false), Step::Close);
        assert_eq!(step(false, true), Step::Ask);
        assert_eq!(step(true, true), Step::Wait);
        assert_eq!(step(true, false), Step::Close);
    }

    #[test]
    fn the_body_names_the_library() {
        assert!(body("Photos").contains("running in Photos."));
    }
}
