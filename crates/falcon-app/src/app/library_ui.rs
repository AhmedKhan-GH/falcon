//! Library-level notices and recovery: the alert box for libraries falcon
//! can't use (made by a newer Eagle, changed elsewhere while open), and
//! reloading a library from disk after a sync conflict, and taking in
//! changes other apps make to open libraries (`watch.rs`).

use eframe::egui::{self, RichText};
use falcon_library::{Error, Library};

use super::{App, Workspace, lock};
use crate::watch::Change;
use crate::theme as t;

/// A message that needs acknowledging, shown in a modal.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Alert {
    pub title: String,
    pub body: String,
}

impl Alert {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Alert { title: title.into(), body: body.into() }
    }
}

/// The alert for an error that has one, else `None` (shown as a toast).
pub(super) fn alert_for(e: &Error) -> Option<Alert> {
    match e {
        Error::NewerLibrary { path, version } => Some(Alert::new(
            "Made by a newer Eagle",
            format!(
                "{} was last saved by Eagle {version}. Falcon reads libraries up to Eagle {}, so it won't open or change this one. Open it in Eagle, or use a newer falcon.",
                name(path),
                falcon_library::library::APP_VERSION
            ),
        )),
        Error::SyncConflict(path) => Some(Alert::new(
            "Library changed elsewhere",
            format!(
                "Another app or a sync service changed {} while it was open here, so falcon didn't save over it. The library was reloaded from disk; the last change made here may need doing again.",
                name(path)
            ),
        )),
        _ => None,
    }
}

fn name(path: &std::path::Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

impl App {
    /// Shows `alert` (replacing any shown).
    pub(super) fn show_alert(&mut self, alert: Alert) {
        self.alert = Some(alert);
    }

    /// The alert modal, if one is up.
    pub(super) fn alert_modal(&mut self, ctx: &egui::Context) {
        let Some(a) = self.alert.clone() else { return };
        let modal = egui::Modal::new(egui::Id::unique("library-alert")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(&a.title).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(&a.body).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.button("OK").clicked()).inner
        });
        if modal.inner || modal.should_close() {
            self.alert = None;
        }
    }

    /// After a frame's edits: if a `metadata.json` write was refused, as
    /// Eagle does, say so and reload the library from disk next frame.
    pub(super) fn check_conflict(&mut self, lib: &mut Library) {
        if lib.take_conflict() {
            self.conflict_reload = true;
        }
    }

    /// Reloads the shown library from disk after a refused write. Pending
    /// writes other than the refused metadata go out first; a library that
    /// can no longer be opened (now made by a newer Eagle, or gone) closes.
    pub(super) fn reload_after_conflict(&mut self) {
        let (Some(shared), Some(root)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        let _ = lock(&shared).flush();
        match falcon_library::sandbox::guard(&root).and_then(|()| Library::open(&root)) {
            Ok(fresh) => {
                let ids: Vec<String> = {
                    let mut lib = lock(&shared);
                    let ids = lib.items().iter().map(|i| i.id.clone()).collect();
                    *lib = fresh;
                    ids
                };
                for id in &ids {
                    self.ws.loader.forget(id);
                }
                // The undo history describes the state that was replaced.
                self.ws.undo.clear();
                self.ws.redo.clear();
                self.ws.edit = None;
                self.ws.folder_edit = None;
                if let Some(a) = alert_for(&Error::SyncConflict(root)) {
                    self.show_alert(a);
                }
            }
            Err(e) => {
                let alert = alert_for(&e).unwrap_or_else(|| Alert::new("Library closed", format!("Falcon couldn't read the library again: {e}")));
                self.close_workspace(self.others.slot());
                self.show_alert(alert);
            }
        }
    }
}

impl App {
    /// Applies what the libraries' watchers saw: a newer `metadata.json`
    /// replaces the folder tree (and the undo history, which no longer
    /// fits it), items added, edited or deleted elsewhere appear, change
    /// or go, and a library whose folder vanished closes.
    pub(super) fn apply_outside_changes(&mut self) {
        let mut vanished = Vec::new();
        for ws in std::iter::once(&mut self.ws).chain(self.others.parked_mut()) {
            let changes = ws.watch.as_ref().map(|w| w.drain()).unwrap_or_default();
            if changes.is_empty() {
                continue;
            }
            let Some(shared) = ws.lib.clone() else { continue };
            let mut lib = lock(&shared);
            for change in changes {
                match change {
                    Change::Metadata(m) => {
                        if lib.take_external_metadata(*m) {
                            ws.undo.clear();
                            ws.redo.clear();
                            ws.folder_edit = None;
                        }
                    }
                    Change::Mtime(index) => {
                        let got = lib.sync_items(&index);
                        for id in got.changed.iter().chain(&got.removed) {
                            ws.loader.forget(id);
                        }
                        if ws.edit.as_ref().is_some_and(|e| got.changed.contains(&e.id) || got.removed.contains(&e.id)) {
                            ws.edit = None;
                        }
                    }
                    Change::Missing => vanished.extend(ws.root.clone()),
                }
            }
        }
        for root in vanished {
            if let Some(i) = self.position_of(&root) {
                self.close_vanished(i);
            }
            self.library_gone(root);
        }
    }

    /// Closes the workspace at switcher position `i` whose library folder
    /// is gone, without the flush and backup a normal close does (their
    /// writes would fail, or recreate files in a folder that moved).
    fn close_vanished(&mut self, i: usize) {
        let closed = match self.others.close(&mut self.ws, i) {
            Some(w) => w,
            None if i == self.others.slot() => {
                let mut empty = Workspace::new(&self.ctx, None);
                (empty.sort, empty.increase, empty.layout) = (self.ws.sort, self.ws.increase, self.ws.layout);
                std::mem::replace(&mut self.ws, empty)
            }
            None => return,
        };
        if let Some(shared) = &closed.lib {
            // Nothing pending may be written to where the library was.
            lock(shared).discard_pending_writes();
        }
        drop(closed);
        self.serve_active();
    }

    /// Says that the library at `root` can no longer be found.
    pub(super) fn library_gone(&mut self, root: std::path::PathBuf) {
        self.show_alert(Alert::new(
            "Library missing",
            format!("{} can't be found at {}. It may have been moved, renamed or deleted.", name(&root), crate::workspaces::tilde(&root)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn alerts_name_the_library() {
        let a = alert_for(&Error::NewerLibrary { path: PathBuf::from("/x/Books.library"), version: "4.2.0".into() }).unwrap();
        assert!(a.body.starts_with("Books was last saved by Eagle 4.2.0."), "{}", a.body);
        assert!(a.body.contains("up to Eagle 4.0.0"));
        let a = alert_for(&Error::SyncConflict(PathBuf::from("/x/Books.library"))).unwrap();
        assert!(a.body.contains("changed Books while"));
        assert_eq!(alert_for(&Error::NotALibrary(PathBuf::from("/x"))), None);
    }
}
