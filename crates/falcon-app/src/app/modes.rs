//! Window and viewer modes from Eagle's View menu: presentation
//! (slideshow) mode, Always on Top, the grayscale preview and the
//! navigator toggle (see the desktop app chapter of the
//! docs).

use eframe::egui;

use super::App;

/// The modes' state, kept by the app.
#[derive(Default)]
pub(crate) struct Modes {
    /// Presentation mode: the viewer fills the screen, without the panels
    /// or its bar, as Eagle's `isSlideshowMode`.
    pub presenting: bool,
    /// Whether the window was already full screen when presentation began,
    /// so leaving puts it back as it was.
    was_fullscreen: bool,
    /// The window has been seen full screen since presentation began; it
    /// leaving full screen after that (the green button, ⌃⌘F) ends it.
    seen_fullscreen: bool,
    /// The main window floats above other windows (Eagle's
    /// `isAlwaysOnTop`, off at every launch).
    pub on_top: bool,
    /// Grayscale preview (Eagle's `isGrayscaleMode`, off at every launch):
    /// the viewer's pictures are drawn desaturated.
    pub grayscale: bool,
    /// The image viewer's navigator is hidden (Eagle's `isHideNavigator`,
    /// kept between launches as Eagle keeps it in local storage).
    pub hide_navigator: bool,
}

impl App {
    /// Eagle's Enter / Leave Presentation Mode (F5, middle click in the
    /// viewer): the selected item opens in the viewer, the window goes
    /// full screen and the panels and the viewer's bar hide.
    pub(crate) fn toggle_presentation(&mut self, ctx: &egui::Context) {
        if self.modes.presenting {
            self.leave_presentation(ctx);
            return;
        }
        if self.ws.viewer.is_none() {
            let Some(first) = self.ws.selected.first().cloned() else { return };
            self.ws.viewer = Some(first);
        }
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        self.modes = Modes { presenting: true, was_fullscreen: fullscreen, seen_fullscreen: fullscreen, ..std::mem::take(&mut self.modes) };
        if !fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
    }

    /// Ends presentation mode, leaving full screen unless the window was
    /// full screen before. The viewer stays open, as in Eagle.
    pub(crate) fn leave_presentation(&mut self, ctx: &egui::Context) {
        if !self.modes.presenting {
            return;
        }
        self.modes.presenting = false;
        if !self.modes.was_fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    /// Once a frame: presentation ends when the viewer closes or the window
    /// leaves full screen by other means, as Eagle's `leave-full-screen`.
    pub(crate) fn follow_modes(&mut self, ctx: &egui::Context) {
        if !self.modes.presenting {
            return;
        }
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        if self.ws.viewer.is_none() {
            self.leave_presentation(ctx);
        } else if fullscreen {
            self.modes.seen_fullscreen = true;
        } else if self.modes.seen_fullscreen {
            self.modes.presenting = false;
        }
    }

    /// Eagle's Always on Top (⇧T): the main window's level, through
    /// eframe's `ViewportCommand::WindowLevel`.
    pub(crate) fn toggle_on_top(&mut self, ctx: &egui::Context) {
        self.modes.on_top = !self.modes.on_top;
        let level = if self.modes.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
    }

    /// Whether the sidebar and the inspector show now (never while
    /// presenting).
    pub(crate) fn panels_shown(&self) -> (bool, bool) {
        let p = self.modes.presenting;
        (self.show_sidebar && !p, self.show_inspector && !p)
    }
}
