//! Running actions and routing the Actions menu and the action keys.

use eframe::egui;
use falcon_library::Library;

use super::{RUN_CONFIRM, Run};
use crate::app::{Action, App};
use crate::locale::{tr, tr_nf, trf};
use crate::logic::Scope;

impl App {
    /// Once a frame, before the keyboard: follows the open library's
    /// actions, hands the actions to Preferences, and runs the
    /// actions chosen in the menu bar or by their keys.
    pub(crate) fn actions_tick(&mut self, ctx: &egui::Context) {
        self.user_actions.follow(self.ws.root.as_deref());
        self.prefs_window.set_actions(self.user_actions.shortcut_list());
        for click in crate::menu::take_action_clicks() {
            if click.by_key {
                // AppKit matched a key; it goes back to egui, which runs it below.
                if let Some(c) = self.user_actions.find(&click.id).and_then(super::chord_of) {
                    let events = crate::app::commands::key_events(&c);
                    ctx.input_mut(|i| i.events.extend(events));
                }
            } else {
                self.run_user_action(&click.id);
            }
        }
        if !self.action_keys_live(ctx) {
            return;
        }
        let hits: Vec<String> = ctx.input_mut(|i| {
            let mut hits = Vec::new();
            i.events.retain(|e| match crate::shortcuts::press(e).and_then(|(key, m)| self.user_actions.by_press(key, m)) {
                Some(d) => {
                    hits.push(d.id.clone());
                    false
                }
                None => true,
            });
            hits
        });
        for id in hits {
            self.run_user_action(&id);
        }
    }

    /// Whether key presses can run actions now: a library is open and no
    /// text field, dialog or recorder wants the keys.
    fn action_keys_live(&self, ctx: &egui::Context) -> bool {
        self.ws.lib.is_some()
            && self.user_actions.editor.is_none()
            && !ctx.memory(|m| m.focused().is_some())
            && !self.menu_state(ctx).modal
            && !self.about.open
    }

    /// Runs action `id` on the selection (or the item in the viewer), as
    /// Eagle's Actions menu does: asking first from `RUN_CONFIRM` items.
    pub(crate) fn run_user_action(&mut self, id: &str) {
        let Some(def) = self.user_actions.find(id) else { return };
        if !def.runnable {
            self.flash_error(tr("This action has steps Falcon can't run"));
            return;
        }
        if self.ws.scope == Scope::Trash {
            self.flash(tr("Actions don't run in the Trash"));
            return;
        }
        let ids = self.acted_on();
        if ids.is_empty() {
            self.flash(tr("Select items to run the action on"));
            return;
        }
        let n = ids.len();
        if def.steps.iter().any(|s| s.is_ai())
            && let Some(problem) = crate::ai_models::setup_problem(&self.prefs.ai_models)
        {
            self.flash_error(tr(problem));
            return;
        }
        if def.steps.iter().any(|s| s.is_ai()) && self.refuse_second_ai_run() {
            return;
        }
        if let Some(run) = self.user_actions.run_of(id, ids) {
            self.push_bulk_at(n, RUN_CONFIRM, Action::RunSteps(run));
        }
    }

    /// The reducer's half: runs the steps inside the frame's undo step.
    pub(crate) fn apply_run(&mut self, lib: &mut Library, run: Run) -> falcon_library::Result<()> {
        if run.has_ai() {
            self.start_ai_run(run);
            return Ok(());
        }
        let report = lib.run_steps(&run.ids, &run.steps)?;
        if !report.missing_folders.is_empty() {
            self.flash_error(tr_nf(
                report.missing_folders.len(),
                "{name}: {n} folder no longer exists and was skipped",
                "{name}: {n} folders no longer exist and were skipped",
                &[("name", &run.name)],
            ));
        } else {
            self.flash_undo(tr_nf(report.changed, "{name} changed {n} item", "{name} changed {n} items", &[("name", &run.name)]));
        }
        Ok(())
    }
}

/// The undo name of a run.
pub(crate) fn run_label(run: &Run) -> String {
    trf("Run {name}", &[("name", &run.name)])
}
