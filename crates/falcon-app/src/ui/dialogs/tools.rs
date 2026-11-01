//! State and drawing for the library tools' dialogs: Add to Other Library
//! (`add_to_library.rs`) Convert WebP (`webp_convert.rs`) and Clear Invalid Content
//! (`clear_invalid.rs`).

use eframe::egui;

use crate::app::App;

/// Each tool's dialog state.
#[derive(Default)]
pub(crate) struct Tools {
    pub add: super::add_to_library::State,
    pub webp: super::webp_convert::State,
    pub clear: super::clear_invalid::State,
}

impl App {
    /// Draws whichever tool dialog is up.
    pub(crate) fn tools_dialogs(&mut self, ctx: &egui::Context) {
        self.add_to_library_dialog(ctx);
        self.webp_dialog(ctx);
        self.clear_invalid_dialog(ctx);
    }
}
