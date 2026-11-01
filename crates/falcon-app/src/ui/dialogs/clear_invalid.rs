//! Clear Invalid Content (`falcon_library::repair`), in the library
//! switcher: scans the shown library's item folders on a background
//! thread, says what it found (warning, as Eagle does, that a sync tool
//! must have finished), then moves broken items to the Trash, moves
//! orphaned folders to the macOS Trash and reloads the library.

use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, RichText};
use falcon_library::repair::{self, InvalidScan};

use crate::app::{App, lock};
use crate::theme as t;
use crate::ui::dialogs::export::plural;
use crate::ui::dialogs::library_notices::Alert;

#[derive(Default)]
pub(crate) enum State {
    #[default]
    Idle,
    Scanning(Receiver<InvalidScan>),
    Found(InvalidScan),
}

/// The alert after a cleanup.
pub(crate) fn finished_text(c: &repair::Cleared) -> String {
    let mut parts = Vec::new();
    if c.trashed > 0 {
        parts.push(format!("moved {} with missing files to the Trash", plural(c.trashed, "item")));
    }
    if c.removed > 0 {
        parts.push(format!("moved {} to the macOS Trash", plural(c.removed, "empty item folder")));
    }
    if c.forgotten > 0 {
        parts.push(format!("dropped {} whose folders were gone", plural(c.forgotten, "item")));
    }
    let mut text = if parts.is_empty() { "Nothing needed cleaning up.".to_string() } else { format!("Falcon {}.", parts.join(", ")) };
    if let Some((p, why)) = c.failed.first() {
        text.push_str(&format!(" {} couldn't be moved to the Trash ({}: {why}).", plural(c.failed.len(), "folder"), p.display()));
    }
    text.push_str(" The library was reloaded.");
    text
}

impl App {
    /// Starts the scan of the shown library.
    pub(crate) fn clear_invalid_content(&mut self) {
        let Some(shared) = self.ws.lib.clone() else { return };
        if !matches!(self.tools.clear, State::Idle) {
            return;
        }
        let (root, items) = {
            let lib = lock(&shared);
            (lib.root().to_path_buf(), lib.items().to_vec())
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(repair::scan(&root, &items));
            ctx.request_repaint();
        });
        self.tools.clear = State::Scanning(rx);
    }

    /// The scan's progress note, its result and the cleanup.
    pub(crate) fn clear_invalid_dialog(&mut self, ctx: &egui::Context) {
        match std::mem::take(&mut self.tools.clear) {
            State::Idle => {}
            State::Scanning(rx) => match rx.try_recv() {
                Ok(scan) if scan.total() == 0 => {
                    self.show_alert(Alert::new("Nothing invalid", format!("Looked at {}; none is empty or broken.", plural(scan.scanned, "item folder"))));
                }
                Ok(scan) => self.tools.clear = State::Found(scan),
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => {
                    egui::Modal::new(egui::Id::unique("clear-invalid-scan")).show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Looking for empty or broken item folders…");
                        });
                    });
                    self.tools.clear = State::Scanning(rx);
                }
            },
            State::Found(scan) => {
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::unique("clear-invalid-confirm")).show(ctx, |ui| {
                    ui.set_width(360.0);
                    ui.label(RichText::new(if scan.total() == 1 { "Found 1 invalid entry".to_string() } else { format!("Found {} invalid entries", scan.total()) }).size(15.0));
                    ui.add_space(4.0);
                    let mut lines = Vec::new();
                    if !scan.broken.is_empty() {
                        lines.push(format!("{} with a missing or empty file go to the Trash.", plural(scan.broken.len(), "item")));
                    }
                    if !scan.orphans.is_empty() {
                        lines.push(format!("{} that belong to no item go to the macOS Trash.", plural(scan.orphans.len(), "folder")));
                    }
                    if !scan.gone.is_empty() {
                        lines.push(format!("{} whose folder is gone are dropped.", plural(scan.gone.len(), "item")));
                    }
                    ui.label(lines.join(" "));
                    ui.add_space(4.0);
                    ui.label(RichText::new("If the library is synced by a cloud service, let the sync finish first: files still downloading look missing.").color(t::p().text_muted));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Clean Up").color(t::p().icon_red)).clicked() {
                            answer = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            answer = Some(false);
                        }
                    });
                });
                if modal.should_close() && answer.is_none() {
                    answer = Some(false);
                }
                match answer {
                    Some(true) => self.apply_clear_invalid(&scan),
                    Some(false) => {}
                    None => self.tools.clear = State::Found(scan),
                }
            }
        }
    }

    fn apply_clear_invalid(&mut self, scan: &InvalidScan) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let result = lock(&shared).clear_invalid(scan, crate::auto_import::trash);
        match result {
            Ok(done) => {
                for (p, why) in &done.failed {
                    crate::errlog::error(&format!("Clear Invalid Content: couldn't move {} to the Trash: {why}", p.display()));
                }
                self.ws.selected.retain(|id| !scan.broken.contains(id) && !scan.gone.contains(id));
                self.show_alert(Alert::new("Cleanup finished", finished_text(&done)));
            }
            Err(e) => self.flash_error(format!("Could not save: {e}")),
        }
        // As Eagle does, the library is read again from disk.
        self.refresh = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_texts() {
        let c = repair::Cleared { trashed: 2, removed: 1, forgotten: 0, failed: vec![] };
        assert_eq!(finished_text(&c), "Falcon moved 2 items with missing files to the Trash, moved 1 empty item folder to the macOS Trash. The library was reloaded.");
        let none = repair::Cleared::default();
        assert_eq!(finished_text(&none), "Nothing needed cleaning up. The library was reloaded.");
    }
}
