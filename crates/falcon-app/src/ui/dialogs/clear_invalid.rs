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
use crate::locale::{tr, tr_n, tr_nf, trf};
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
        parts.push(tr_n(c.trashed, "Moved {n} item with missing files to the Trash.", "Moved {n} items with missing files to the Trash."));
    }
    if c.removed > 0 {
        parts.push(tr_n(c.removed, "Moved {n} empty item folder to the macOS Trash.", "Moved {n} empty item folders to the macOS Trash."));
    }
    if c.forgotten > 0 {
        parts.push(tr_n(c.forgotten, "Dropped {n} item whose folder was gone.", "Dropped {n} items whose folders were gone."));
    }
    if parts.is_empty() {
        parts.push(tr("Nothing needed cleaning up.").to_string());
    }
    if let Some((p, why)) = c.failed.first() {
        parts.push(tr_nf(
            c.failed.len(),
            "{n} folder couldn't be moved to the Trash ({path}: {reason}).",
            "{n} folders couldn't be moved to the Trash ({path}: {reason}).",
            &[("path", &p.display()), ("reason", why)],
        ));
    }
    parts.push(tr("The library was reloaded.").to_string());
    parts.join(" ")
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
                    self.show_alert(Alert::new(tr("Nothing invalid"), tr_n(scan.scanned, "Looked at {n} item folder; none is empty or broken.", "Looked at {n} item folders; none is empty or broken.")));
                }
                Ok(scan) => self.tools.clear = State::Found(scan),
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => {
                    egui::Modal::new(egui::Id::unique("clear-invalid-scan")).show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr("Looking for empty or broken item folders…"));
                        });
                    });
                    self.tools.clear = State::Scanning(rx);
                }
            },
            State::Found(scan) => {
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::unique("clear-invalid-confirm")).show(ctx, |ui| {
                    ui.set_width(360.0);
                    ui.label(RichText::new(tr_n(scan.total(), "Found {n} invalid entry", "Found {n} invalid entries")).size(15.0));
                    ui.add_space(4.0);
                    let mut lines = Vec::new();
                    if !scan.broken.is_empty() {
                        lines.push(tr_n(scan.broken.len(), "{n} item with a missing or empty file goes to the Trash.", "{n} items with a missing or empty file go to the Trash."));
                    }
                    if !scan.orphans.is_empty() {
                        lines.push(tr_n(scan.orphans.len(), "{n} folder that belongs to no item goes to the macOS Trash.", "{n} folders that belong to no item go to the macOS Trash."));
                    }
                    if !scan.gone.is_empty() {
                        lines.push(tr_n(scan.gone.len(), "{n} item whose folder is gone is dropped.", "{n} items whose folder is gone are dropped."));
                    }
                    ui.label(lines.join(" "));
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr("If the library is synced by a cloud service, let the sync finish first: files still downloading look missing.")).color(t::p().text_muted));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new(tr("Clean Up")).color(t::p().icon_red)).clicked() {
                            answer = Some(true);
                        }
                        if ui.button(tr("Cancel")).clicked() {
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
                self.show_alert(Alert::new(tr("Cleanup finished"), finished_text(&done)));
            }
            Err(e) => self.flash_error(trf("Could not save: {error}", &[("error", &e)])),
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
        assert_eq!(finished_text(&c), "Moved 2 items with missing files to the Trash. Moved 1 empty item folder to the macOS Trash. The library was reloaded.");
        let none = repair::Cleared::default();
        assert_eq!(finished_text(&none), "Nothing needed cleaning up. The library was reloaded.");
    }
}
