//! Remove from Disk (a falcon addition; Eagle has no such command): moves
//! a library folder to the Trash after a confirmation naming the library
//! and its size. The rule is in `library_removal.rs`; this is the dialog.
//! It is drawn with or without a library open, since the switcher's menu
//! can reach it either way.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, RichText};

use crate::app::App;
use crate::library_removal::{self as rule, Refusal, TrashError};
use crate::locale::{tr, trf};
use crate::theme as t;

/// What is known of the folder's size.
enum Size {
    Calculating,
    Known(u64),
    Unknown,
}

/// The open question. Dropping it (the dialog closed) stops the size walk.
pub(crate) struct RemoveLibrary {
    path: PathBuf,
    name: String,
    size: Size,
    rx: Receiver<std::io::Result<u64>>,
    stop: Arc<AtomicBool>,
}

impl Drop for RemoveLibrary {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn library_name(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

impl App {
    /// Every library that is open or being read.
    fn open_library_paths(&self) -> Vec<PathBuf> {
        let mut open: Vec<PathBuf> = self.others.iter(&self.ws).filter_map(|w| w.root.clone()).collect();
        open.extend(self.loading.as_ref().map(|l| l.path.clone()));
        open
    }

    /// Whether the switcher offers Remove from Disk for `root`: only for a
    /// library inside the enforced sandbox.
    pub(crate) fn can_offer_removal(&self, root: &Path) -> bool {
        falcon_library::sandbox::root().is_some() && falcon_library::sandbox::check(root).is_ok()
    }

    /// Whether `root` is open, so Remove from Disk is disabled for it.
    pub(crate) fn is_library_open(&self, root: &Path) -> bool {
        rule::is_open(root, &self.open_library_paths())
    }

    /// Runs the rule once more, then asks. The size is counted on a worker.
    pub(crate) fn ask_remove_library(&mut self, root: &Path) {
        if let Err(r) = self.removal_check(root) {
            self.flash_error(refusal_text(root, r));
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (path, flag, ctx) = (root.to_path_buf(), stop.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(rule::folder_size(&path, &flag));
            ctx.request_repaint();
        });
        self.remove_library = Some(RemoveLibrary { path: root.to_path_buf(), name: library_name(root), size: Size::Calculating, rx, stop });
    }

    fn removal_check(&self, root: &Path) -> Result<(), Refusal> {
        rule::check(root, falcon_library::sandbox::root(), &falcon_library::sandbox::check, &self.open_library_paths())
    }

    /// Draws the question and, on Move to Trash, trashes the folder.
    pub(crate) fn remove_library_dialog(&mut self, ctx: &egui::Context) {
        let Some(q) = &mut self.remove_library else { return };
        match q.rx.try_recv() {
            Ok(Ok(n)) => q.size = Size::Known(n),
            Ok(Err(_)) => q.size = Size::Unknown,
            Err(TryRecvError::Disconnected) if matches!(q.size, Size::Calculating) => q.size = Size::Unknown,
            Err(_) => {}
        }
        let (path, name) = (q.path.clone(), q.name.clone());
        let size_line = match q.size {
            Size::Calculating => tr("Calculating…").to_string(),
            Size::Known(n) => crate::logic::format_size(n),
            Size::Unknown => tr("Size unknown").to_string(),
        };
        let ready = !matches!(q.size, Size::Calculating);
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("remove-library")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(trf("Move “{name}” to the Trash?", &[("name", &name)])).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(path.display().to_string()).color(t::p().text_muted));
            ui.label(RichText::new(trf("Size: {size}", &[("size", &size_line)])).color(t::p().text_muted));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Everything in the folder goes to the Trash. You can put it back from Finder.")).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(ready, egui::Button::new(RichText::new(tr("Move to Trash")).color(t::p().icon_red))).clicked() {
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
        if !ready {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        match answer {
            Some(true) => {
                self.remove_library = None;
                self.remove_library_now(&path, &name);
            }
            Some(false) => self.remove_library = None,
            None => {}
        }
    }

    /// The trash step: the whole rule runs again first. A failure shows an
    /// error and changes nothing; there is no fallback to deleting.
    fn remove_library_now(&mut self, path: &Path, name: &str) {
        let open = self.open_library_paths();
        let done = rule::trash_library(path, falcon_library::sandbox::root(), &falcon_library::sandbox::check, &open, crate::auto_import::trash);
        match done {
            Ok(()) => {
                crate::workspaces::remove_from_list(&mut self.recent, path);
                self.lib_icons.forget(path);
                self.layouts.remove(path.to_string_lossy().as_ref());
                self.flash(trf("Moved “{name}” to the Trash", &[("name", &name)]));
            }
            Err(TrashError::Refused(r)) => self.flash_error(refusal_text(path, r)),
            Err(TrashError::Failed(e)) => self.flash_error(trf("Couldn't move “{name}” to the Trash: {error}", &[("name", &name), ("error", &e)])),
        }
    }
}

/// Which of the three sentences a refusal gets.
#[derive(Debug, PartialEq)]
enum Why {
    Open,
    Sandbox,
    NotPlain,
}

fn why(r: Refusal) -> Why {
    match r {
        Refusal::Open | Refusal::InsideOpen | Refusal::ContainsOpen => Why::Open,
        Refusal::NoSandbox | Refusal::OutsideSandbox | Refusal::IsSandboxRoot | Refusal::ContainsSandbox => Why::Sandbox,
        Refusal::Missing | Refusal::Symlink | Refusal::NotDirectory | Refusal::NotLibrary | Refusal::NoMetadata => Why::NotPlain,
    }
}

/// The error toast for a refusal: one whole sentence per kind.
fn refusal_text(path: &Path, r: Refusal) -> String {
    let name = library_name(path);
    match why(r) {
        Why::Open => trf("“{name}” is open, so it can't be moved to the Trash.", &[("name", &name)]),
        Why::Sandbox => trf("“{name}” isn't in the sandbox folder, so it can't be moved to the Trash.", &[("name", &name)]),
        Why::NotPlain => trf("“{name}” isn't a plain library folder, so it can't be moved to the Trash.", &[("name", &name)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_gets_one_of_three_sentences() {
        use Refusal::*;
        for r in [NoSandbox, OutsideSandbox, IsSandboxRoot, ContainsSandbox, Missing, Symlink, NotDirectory, NotLibrary, NoMetadata, Open, InsideOpen, ContainsOpen] {
            let text = refusal_text(Path::new("/a/B.library"), r);
            assert!(text.contains('B') && text.contains("Trash"), "{r:?}: {text}");
        }
        assert_eq!(why(Refusal::InsideOpen), Why::Open);
        assert_eq!(why(Refusal::IsSandboxRoot), Why::Sandbox);
        assert_eq!(why(Refusal::NoMetadata), Why::NotPlain);
    }
}
