//! The long operations that run on a worker thread behind the progress
//! modal (`ui/dialogs/progress.rs`): refreshing thumbnails, emptying the
//! Trash and importing an .eaglepack. The worker locks the library per item,
//! so the window keeps drawing, and checks for Cancel between items; what
//! was done stays done.

use std::path::PathBuf;

use falcon_library::pack::{self, PackImport};
use falcon_library::Library;

use crate::app::{lock, App};
use crate::ui::dialogs::library_notices::Alert;
use crate::ui::dialogs::progress::Outcome;

/// The toast for a finished thumbnail refresh.
fn refreshed_text(done: usize, custom: usize) -> String {
    if custom > 0 {
        crate::locale::tr_nf(done, "Refreshed {n} thumbnail, kept {custom} custom", "Refreshed {n} thumbnails, kept {custom} custom", &[("custom", &custom)])
    } else {
        crate::locale::tr_n(done, "Refreshed {n} thumbnail", "Refreshed {n} thumbnails")
    }
}

impl App {
    /// Refresh Thumbnail for several items, off the UI thread. Cancel stops
    /// before the next item.
    pub(crate) fn start_refresh_thumbnails(&mut self, ids: Vec<String>) {
        let Some(shared) = self.ws.lib.clone() else { return };
        if self.operation_running() {
            return;
        }
        let h = self.ws.op.start(&self.ctx, crate::locale::tr("Refreshing Thumbnails"), ids.len(), true);
        std::thread::spawn(move || {
            let (mut done, mut custom, mut failure) = (0, 0, None);
            let mut forget = Vec::new();
            for (n, id) in ids.iter().enumerate() {
                if h.cancelled() {
                    break;
                }
                let name = lock(&shared).item(id).map(|i| i.name.clone()).unwrap_or_default();
                h.step(n, name);
                let result = lock(&shared).refresh_thumbnail(id);
                match result {
                    Ok(true) => done += 1,
                    Ok(false) => custom += 1,
                    Err(e) => failure = Some(e),
                }
                forget.push(id.clone());
            }
            h.finish(Outcome { message: refreshed_text(done, custom), error: failure.map(|e| crate::locale::trf("Could not save: {error}", &[("error", &e)])), forget, changed: done > 0, ..Default::default() });
        });
    }

    /// Empty Trash: each item is deleted for good in turn (what
    /// `Library::empty_trash` does in one call), so Cancel leaves the rest
    /// in the Trash. Permanent, so the undo history goes (`Outcome::deleted`).
    pub(crate) fn start_empty_trash(&mut self, lib: &Library) {
        let ids: Vec<String> = lib.items().iter().filter(|i| i.is_deleted).map(|i| i.id.clone()).collect();
        if ids.is_empty() {
            self.flash(crate::locale::trf("Deleted {n}", &[("n", &0)]));
            return;
        }
        let Some(shared) = self.ws.lib.clone() else { return };
        if self.operation_running() {
            return;
        }
        let h = self.ws.op.start(&self.ctx, crate::locale::tr("Emptying the Trash"), ids.len(), true);
        let names: Vec<String> = ids.iter().map(|id| lib.item(id).map(|i| i.name.clone()).unwrap_or_default()).collect();
        std::thread::spawn(move || {
            let mut deleted = 0;
            let mut failure = None;
            for (n, id) in ids.iter().enumerate() {
                if h.cancelled() {
                    break;
                }
                h.step(n, names[n].clone());
                let result = lock(&shared).delete_permanently(std::slice::from_ref(id));
                if let Err(e) = result {
                    failure = Some(crate::locale::trf("Could not save: {error}", &[("error", &e)]));
                    break;
                }
                deleted += 1;
            }
            h.finish(Outcome { message: crate::locale::trf("Deleted {n}", &[("n", &deleted)]), error: failure, deleted: true, changed: deleted > 0, ..Default::default() });
        });
    }

    /// Imports a pack behind the progress modal: unpacking first (without
    /// the library), then one item at a time. Cancel stops between items;
    /// the items already in stay.
    pub(crate) fn start_pack_import(&mut self, path: PathBuf, folder: Option<String>, skip_duplicates: bool) {
        let Some(shared) = self.ws.lib.clone() else { return };
        if self.operation_running() {
            return;
        }
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let h = self.ws.op.start(&self.ctx, crate::locale::tr("Importing Eagle Package"), 0, true);
        h.step(0, crate::locale::trf("Reading {file}", &[("file", &file)]));
        std::thread::spawn(move || {
            let failed = |e: falcon_library::Error| Outcome {
                alert: Some(Alert::new(crate::locale::tr("Can't import the package"), crate::locale::trf("It may be incomplete or damaged; export or download it again. ({error})", &[("error", &e)]))),
                ..Default::default()
            };
            let opts = pack::PackImportOptions { folder, skip_duplicates };
            let prepared = pack::extract_pack(&path).and_then(|dir| {
                let mut run = PackImport::prepare(&lock(&shared), &path, dir, &opts)?;
                run.start(&mut lock(&shared))?;
                Ok(run)
            });
            let mut run = match prepared {
                Ok(r) => r,
                Err(e) => return h.finish(failed(e)),
            };
            let total = run.total();
            h.set_total(total);
            let mut added = 0;
            let mut stopped = false;
            loop {
                if h.cancelled() {
                    stopped = true;
                    break;
                }
                h.step(added, file.clone());
                let step = run.step(&mut lock(&shared));
                match step {
                    Ok(Some(_)) => added += 1,
                    Ok(None) => break,
                    Err(e) => {
                        let mut o = failed(e);
                        o.changed = added > 0;
                        return h.finish(o);
                    }
                }
            }
            let message = if stopped {
                crate::locale::tr_nf(total, "Stopped after {added} of {n} item from {name}", "Stopped after {added} of {n} items from {name}", &[("added", &added), ("name", &file)])
            } else {
                crate::locale::tr_nf(added, "Imported {n} item from {name}", "Imported {n} items from {name}", &[("name", &file)])
            };
            h.finish(Outcome { message, changed: true, ..Default::default() });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_summary() {
        assert_eq!(refreshed_text(1, 0), "Refreshed 1 thumbnail");
        assert_eq!(refreshed_text(3, 2), "Refreshed 3 thumbnails, kept 2 custom");
    }
}
