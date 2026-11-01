//! Library-level notices and recovery: the alert box for libraries falcon
//! can't use (made by a newer Eagle, changed elsewhere while open), and
//! reloading a library from disk after a sync conflict.

use eframe::egui::{self, RichText};
use falcon_library::{Error, Library};

use super::{App, lock};
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
