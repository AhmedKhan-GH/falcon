//! Switching away from a library while tasks run in it. Eagle warns before
//! a switch and offers Force Switch, since its window holds one library.
//! Falcon's background libraries keep their imports and operations going, so
//! nothing stops here (closing stops them, `force_close.rs`) and the warning
//! only says where the tasks went.
//!
//! Every way of leaving the shown library asks: the switcher's rows and the
//! next and previous library commands, a recent library, Open Library… and a
//! dropped library (once the folder is known), New Library… (before its
//! dialog, so Cancel leaves no new library behind) and the HTTP API's
//! `library/switch`. The API's library goes back out of the shown slot before
//! the question, so the running tasks never write to the other one; Cancel
//! leaves the API on the shown library. Eagle's block during an AI action has
//! no counterpart, as falcon has no AI actions.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use falcon_library::Library;

use super::force_close::tasks_running;
use crate::app::App;
use crate::locale::{tr, trf};
use crate::theme as t;

/// Where a switch goes.
pub(crate) enum Target {
    /// An open library, by folder (a switcher position could shift).
    Focus(PathBuf),
    /// A library to read, or to show if it is open.
    Open(PathBuf),
    /// The Create a Library dialog, whose library opens when it is made.
    Create,
    /// The API's `library/switch` to the library at `root`: `library` is its
    /// copy, `None` when a workspace has it open already.
    Api { root: PathBuf, library: Option<Box<Library>>, note: Option<String> },
}

/// What to do with a request to switch.
#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    /// Switch now.
    Switch,
    /// Ask first.
    Ask,
}

/// The rule: asking needs tasks running in the shown library and a switch
/// that really leaves it (not showing the library already shown).
pub(crate) fn step(leaves: bool, running: bool) -> Step {
    if leaves && running { Step::Ask } else { Step::Switch }
}

/// The dialog's text.
pub(crate) fn body(library: &str) -> String {
    trf("Tasks are running in {library}. They keep running in the background after you switch.", &[("library", &library)])
}

fn stem(root: &Path) -> String {
    root.file_stem().map_or_else(|| root.display().to_string(), |s| s.to_string_lossy().into_owned())
}

impl App {
    /// The switcher's rows and the next and previous library commands: shows
    /// the open library at switcher position `i`, or asks first.
    pub(crate) fn request_focus(&mut self, i: usize) {
        let root = self.others.iter(&self.ws).nth(i).and_then(|w| w.root.clone());
        match root {
            Some(root) => self.request_switch(Target::Focus(root)),
            None => self.focus(i),
        }
    }

    /// Switches to `target`, or asks first when tasks run in the shown library.
    pub(crate) fn request_switch(&mut self, target: Target) {
        let leaves = match &target {
            Target::Focus(p) | Target::Open(p) | Target::Api { root: p, .. } => self.position_of(p) != Some(self.others.slot()),
            Target::Create => true,
        };
        match step(leaves, tasks_running(&self.ws)) {
            Step::Switch => self.run_switch(target),
            Step::Ask => self.switch_warning = Some(target),
        }
    }

    fn run_switch(&mut self, target: Target) {
        match target {
            Target::Focus(root) => {
                if let Some(i) = self.position_of(&root) {
                    self.focus(i);
                }
            }
            Target::Open(path) => self.queue_open(path),
            Target::Create => self.dialogs.start_create(),
            Target::Api { root, library, note } => {
                match (library, self.position_of(&root)) {
                    (Some(library), _) => self.add_workspace(Arc::new(Mutex::new(*library))),
                    (None, Some(i)) => self.focus(i),
                    (None, None) => {}
                }
                self.api_switched(&root, note);
            }
        }
    }

    /// Draws the question. A switch waiting for an answer goes ahead by
    /// itself when the tasks finish, as a close waiting for them does.
    pub(crate) fn switch_warning_dialog(&mut self, ctx: &egui::Context) {
        if self.switch_warning.is_none() {
            return;
        }
        if step(true, tasks_running(&self.ws)) == Step::Switch {
            if let Some(target) = self.switch_warning.take() {
                self.run_switch(target);
            }
            return;
        }
        let name = self.ws.root.as_deref().map(stem).unwrap_or_default();
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("switch-warning")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(tr("Switch Library While Tasks Run")).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(body(&name)).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Force Switch")).clicked() {
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
        match (answer, self.switch_warning.take()) {
            (Some(true), Some(target)) => self.run_switch(target),
            (Some(false), Some(Target::Api { .. })) => self.flash(tr("Library switch over the API canceled")),
            (Some(false), _) => {}
            (None, target) => {
                self.switch_warning = target;
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            (Some(true), None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_asks_only_when_it_leaves_a_library_with_tasks_running() {
        assert_eq!(step(true, true), Step::Ask);
        assert_eq!(step(true, false), Step::Switch);
        assert_eq!(step(false, true), Step::Switch, "showing the library already shown");
        assert_eq!(step(false, false), Step::Switch);
    }

    #[test]
    fn the_body_names_the_library_and_says_the_tasks_go_on() {
        let text = body("Photos");
        assert!(text.contains("running in Photos."));
        assert!(text.contains("keep running in the background"));
    }

    #[test]
    fn a_library_is_named_by_its_folder_stem() {
        assert_eq!(stem(Path::new("/s/Photos.library")), "Photos");
    }
}
