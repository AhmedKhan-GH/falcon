//! Quitting while tasks run. Imports, a progress-modal operation or an
//! export in any open library would be cut off by a quit, so the first
//! request asks, as closing one library does (`force_close.rs`). Stopping
//! waits for the workers, then the window closes for real, so the usual
//! save and backup on quit still run.
//!
//! Quit arrives three ways: the window's close request (the red button
//! and ⌘W when they would quit rather than hide), the app menu's Quit
//! (⌘Q, replaced in `menu.rs` so it comes here instead of ending the
//! process) and the menu bar icon's Quit.

use eframe::egui::{self, RichText};

use super::force_close::{Step, step, stop_tasks, tasks_running};
use crate::app::App;
use crate::locale::tr;
use crate::theme as t;

/// The quit dialog's state.
#[derive(Default)]
pub(crate) struct QuitConfirm {
    /// The question is showing; true once the tasks were told to stop.
    pub asking: Option<bool>,
    /// The quit was answered: the next close request goes through.
    pub allowed: bool,
}

/// What to do with a close request of the window.
#[derive(Debug, PartialEq)]
pub(crate) enum WindowClose {
    /// Let it go through (it quits).
    Allow,
    /// It only hides the app: the usual hide, no question.
    Hide,
    /// Cancel it and ask first.
    Ask,
}

/// The rule for a close request. `allowed`: the quit was already answered;
/// `would_quit`: the close would quit rather than hide; `running`: tasks
/// run in some open library.
pub(crate) fn window_close(allowed: bool, would_quit: bool, running: bool) -> WindowClose {
    match (allowed, would_quit, running) {
        (true, ..) => WindowClose::Allow,
        (false, false, _) => WindowClose::Hide,
        (false, true, true) => WindowClose::Ask,
        (false, true, false) => WindowClose::Allow,
    }
}

/// What the menu bar's Quit does: quits now, or asks first.
#[derive(Debug, PartialEq)]
pub(crate) enum MenuQuit {
    Now,
    Ask,
}

pub(crate) fn menu_quit(running: bool) -> MenuQuit {
    if running { MenuQuit::Ask } else { MenuQuit::Now }
}

impl App {
    fn any_tasks_running(&self) -> bool {
        self.others.iter(&self.ws).any(tasks_running)
    }

    /// A close request of the window. Returns whether the hide-instead rule
    /// of `app/mod.rs` applies to it.
    pub(crate) fn quit_on_close_request(&mut self, ctx: &egui::Context) -> bool {
        let would_quit = crate::dock::close_action(self.prefs.keep_dock_icon, self.prefs.menu_icon, crate::dock::CAN_HIDE) == crate::dock::CloseAction::Quit;
        match window_close(self.quit.allowed, would_quit, self.any_tasks_running()) {
            WindowClose::Allow => false,
            WindowClose::Hide => true,
            WindowClose::Ask => {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.quit.asking.get_or_insert(false);
                false
            }
        }
    }

    /// A quit from the menu bar or its icon.
    pub(crate) fn request_quit(&mut self, _ctx: &egui::Context) {
        match menu_quit(self.any_tasks_running()) {
            MenuQuit::Now => crate::dock::quit(),
            MenuQuit::Ask => {
                // The question must be seen, also when the app is hidden.
                crate::dock::bring_to_front();
                self.quit.asking.get_or_insert(false);
            }
        }
    }

    /// Takes the menu's Quit, draws the question, and quits once the tasks
    /// have stopped. Runs every frame, also while locked.
    pub(crate) fn quit_dialog(&mut self, ctx: &egui::Context) {
        if crate::menu::take_quit_request() {
            self.request_quit(ctx);
        }
        let Some(stopping) = self.quit.asking else { return };
        match step(stopping, self.any_tasks_running()) {
            Step::Close => {
                self.quit.asking = None;
                self.quit.allowed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Step::Wait => {
                egui::Modal::new(egui::Id::unique("quit-wait")).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new(tr("Quit While Tasks Run")).size(15.0));
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr("Stopping tasks…")).color(t::p().text_muted));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Step::Ask => {
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::unique("quit-ask")).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new(tr("Quit While Tasks Run")).size(15.0));
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr("Tasks are still running. Quitting stops them. Work already done is kept.")).color(t::p().text_muted));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new(tr("Stop Tasks and Quit")).color(t::p().icon_red)).clicked() {
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
                        for ws in self.others.iter(&self.ws).filter(|w| tasks_running(w)) {
                            stop_tasks(ws);
                        }
                        self.quit.asking = Some(true);
                        ctx.request_repaint();
                    }
                    Some(false) => self.quit.asking = None,
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
    fn a_quitting_close_asks_only_while_tasks_run() {
        assert_eq!(window_close(false, true, true), WindowClose::Ask);
        assert_eq!(window_close(false, true, false), WindowClose::Allow);
    }

    #[test]
    fn a_hiding_close_never_asks() {
        assert_eq!(window_close(false, false, true), WindowClose::Hide);
        assert_eq!(window_close(false, false, false), WindowClose::Hide);
    }

    #[test]
    fn an_answered_quit_goes_through() {
        assert_eq!(window_close(true, true, true), WindowClose::Allow);
        assert_eq!(window_close(true, false, true), WindowClose::Allow);
    }

    #[test]
    fn the_menu_quit_asks_only_while_tasks_run() {
        assert_eq!(menu_quit(true), MenuQuit::Ask);
        assert_eq!(menu_quit(false), MenuQuit::Now);
    }
}
