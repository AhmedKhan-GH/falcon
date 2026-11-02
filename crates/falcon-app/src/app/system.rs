//! What reaches falcon from outside its window: the global keys
//! (`hotkeys.rs`), the menu bar icon (`tray.rs`) and screen capture
//! (`capture.rs`), with the system notifications they and imports and
//! exports post (`notify.rs`).

use eframe::egui;

use super::App;
use super::import::{Job, tempfile_dir};
use crate::capture::{self, Mode};
use crate::locale::{tr, trf};
use crate::shortcuts::Command;
use crate::tray::Choice;

/// The state behind it.
#[derive(Default)]
pub(crate) struct System {
    pub(crate) captures: capture::Captures,
    pub(crate) hotkeys: crate::hotkeys::HotKeys,
    pub(crate) tray: crate::tray::Tray,
    /// Screenshot imports in the running import batch, so the batch that
    /// only holds them doesn't also say "Import finished" (the screenshot
    /// had its own notice).
    pub(crate) capture_imports: usize,
}

impl App {
    /// Runs every frame, locked or not: keeps the global keys and the
    /// menu bar icon in step with the preferences, acts on what they
    /// queued, and brings in finished screenshots.
    pub(crate) fn system_tick(&mut self, ctx: &egui::Context) {
        // A 3D model the viewer has left is let go of (`model3d`).
        self.model3d.release_unseen(ctx);
        let captures_on = self.prefs.screenshot_shortcuts;
        // While Preferences is open the keys follow what is being edited,
        // so a chord another app holds shows on its row at once and is
        // tried again when the switch or the chord changes; Cancel brings
        // back the saved ones. A row waiting for a key press gets every
        // key, so they are let go meanwhile.
        let wanted = if self.prefs_window.recording() {
            Vec::new()
        } else {
            match self.prefs_window.live_keys() {
                Some((keys, on)) => crate::hotkeys::wanted(keys, on),
                None => crate::hotkeys::wanted(&self.shortcuts, captures_on),
            }
        };
        let outcome = self.system.hotkeys.sync(ctx, wanted);
        self.prefs_window.set_refused(self.system.hotkeys.refused());
        // Outside Preferences (at launch, or after Save) a note says so;
        // inside, the row does.
        if !self.prefs_window.open
            && let Some(out) = outcome
            && let Some((cmd, chord)) = out.refused.first()
        {
            let label = crate::shortcuts::spec(*cmd).label;
            let chord = crate::shortcuts::display(ctx, chord);
            self.flash_error(trf("{chord} can't be used for “{command}”: another app has it", &[("chord", &chord), ("command", &tr(label))]));
        }
        self.system.tray.sync(ctx, self.prefs.menu_icon, &self.shortcuts, captures_on);
        for cmd in crate::hotkeys::take_pressed() {
            self.outside_command(ctx, cmd);
        }
        for choice in crate::tray::take_choices() {
            match choice {
                Choice::Quit => self.request_quit(ctx),
                other => {
                    if let Some(cmd) = other.command() {
                        self.outside_command(ctx, cmd);
                    }
                }
            }
        }
        self.capture_tick();
        for shot in self.collect.take_abandoned() {
            let _ = std::fs::remove_file(&shot.path);
        }
    }

    /// A global key or a menu bar icon item. While falcon is locked only
    /// Show acts (it shows the lock screen).
    fn outside_command(&mut self, ctx: &egui::Context, cmd: Command) {
        match cmd {
            Command::ShowFalcon => self.show_falcon(ctx, false),
            Command::ShowSearch => self.show_falcon(ctx, !self.lock.is_locked()),
            Command::Capture(mode) if !self.lock.is_locked() => self.start_capture(mode),
            _ => {}
        }
    }

    /// Brings the main window to the front, with the search box focused
    /// when `search` (Eagle's `global.show.search`).
    pub(crate) fn show_falcon(&mut self, ctx: &egui::Context, search: bool) {
        crate::dock::bring_to_front();
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        if search && self.ws.lib.is_some() {
            self.ws.viewer = None;
            self.focus_search = true;
        }
    }

    /// Starts a screenshot of `mode`. As in Eagle it needs an open
    /// library, and a request while one is being taken is ignored.
    pub(crate) fn start_capture(&mut self, mode: Mode) {
        if self.ws.lib.is_none() {
            self.flash(tr("Open a library to take screenshots into"));
            return;
        }
        let Ok(dir) = tempfile_dir() else {
            self.flash_error(tr("Couldn't make a folder for the screenshot"));
            return;
        };
        let ctx = self.ctx.clone();
        self.system.captures.start(mode, self.prefs.capture_options(), capture::pointer_screen(), dir, move || ctx.request_repaint());
    }

    /// Brings in the screenshots taken since the last frame: into the
    /// collect window when it should ask where they go, else straight into
    /// the library (its root, as in Eagle) with a notification.
    fn capture_tick(&mut self) {
        for result in self.system.captures.take_done() {
            let shot = match result {
                Ok(shot) => shot,
                Err(e) => {
                    crate::errlog::error(&format!("screenshot: {e}"));
                    self.flash_error(trf("The screenshot failed: {error}", &[("error", &e)]));
                    continue;
                }
            };
            if self.ws.lib.is_none() {
                let _ = std::fs::remove_file(&shot.path);
                continue;
            }
            let tags = capture::tags(self.prefs.screenshot_tag);
            if self.prefs.asks_capture(shot.mode) {
                crate::dock::bring_to_front();
                self.collect.ask(crate::collect::Pending { path: shot.path, name: shot.name, tags });
                continue;
            }
            if self.prefs.tells_capture(shot.mode) {
                crate::notify::post(&crate::notify::Notice::captured(&shot.name));
            }
            if self.prefs.sounds_capture(shot.mode) {
                self.play_sound(crate::sounds::Sound::Capture);
            }
            let mut opts = self.prefs.import_options(Vec::new());
            opts.name = Some(shot.name);
            for t in tags {
                crate::collect::add_tag(&mut opts.tags, &t);
            }
            self.system.capture_imports += 1;
            // The import deletes the file once it has copied it in (it is
            // in falcon's temp folder).
            self.spawn_import(vec![Job { path: shot.path, opts }]);
        }
    }

    /// When an import batch finishes: posts "Import finished" unless the
    /// batch held only screenshots, and starts counting afresh.
    pub(crate) fn import_done_notice(&mut self, total: usize, added: usize) {
        let only_shots = self.system.capture_imports >= total;
        self.system.capture_imports = 0;
        if !only_shots && added > 0 && self.prefs.tells_import_done() {
            crate::notify::post(&crate::notify::Notice::imported(added));
        }
    }

    /// When an export finishes with `message` ("Exported 3 items").
    pub(crate) fn export_done_notice(&self, message: &str) {
        if self.prefs.tells_export_done() {
            crate::notify::post(&crate::notify::Notice::exported(message));
        }
    }
}
