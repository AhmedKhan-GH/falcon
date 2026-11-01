//! The Clear History confirmation dialog.

use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::app::App;
use crate::locale::tr;
use crate::theme as t;

impl App {
    /// Library ▸ Clear Library History: asks first. Shown with or without
    /// a library open.
    pub(crate) fn clear_history_dialog(&mut self, ctx: &egui::Context) {
        if self.clear_history_open {
            let modal = egui::Modal::new(egui::Id::unique("clear-library-history")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(tr("Clear Library History")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(tr("The list of recent libraries is emptied, except for the libraries that are open now. This can't be undone. No library is deleted.")).color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new(tr("Clear All")).color(t::p().icon_red)).clicked() {
                        let open: Vec<PathBuf> = self.others.iter(&self.ws).filter_map(|w| w.root.clone()).collect();
                        self.recent = crate::workspaces::clear_history(&self.recent, &open);
                        self.clear_history_open = false;
                    }
                    if ui.button(tr("Cancel")).clicked() {
                        self.clear_history_open = false;
                    }
                });
            });
            if modal.should_close() {
                self.clear_history_open = false;
            }
        }
    }
}
