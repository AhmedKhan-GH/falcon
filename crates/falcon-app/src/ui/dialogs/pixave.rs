//! File ▸ Import ▸ Pixave Library…: probes the chosen `.pxvlibrary`
//! package (`falcon_library::pixave`), shows what can come over, and
//! imports the files it can tell apart into a new folder named after the
//! library. The schema report can be saved for mapping the rest later.

use std::path::Path;

use eframe::egui::{self, RichText};
use falcon_library::pixave::{self, Probe};

use crate::app::{App, import, lock};
use crate::locale::{tr, tr_n, tr_nf, trf};
use crate::theme as t;
use crate::ui::dialogs::library_notices::Alert;

/// A probed Pixave library waiting for the user's answer.
pub(crate) struct PixaveAsk {
    probe: Probe,
}

enum Answer {
    Import,
    Report,
    Cancel,
}

fn library_name(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

impl App {
    /// Asks for a `.pxvlibrary` package and probes it.
    pub(crate) fn import_pixave_dialog(&mut self) {
        if self.ws.lib.is_none() {
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title(tr("Choose a Pixave Library")).pick_folder() else { return };
        match pixave::probe(&dir) {
            Ok(probe) => self.pixave_ask = Some(PixaveAsk { probe }),
            Err(e) => {
                let body = if pixave::is_library(&dir) {
                    e.to_string()
                } else {
                    trf("Choose a Pixave library: a folder whose name ends in .pxvlibrary. ({path})", &[("path", &crate::workspaces::tilde(&dir))])
                };
                self.show_alert(Alert::new(tr("Can't read the Pixave library"), body));
            }
        }
    }

    pub(crate) fn pixave_dialogs(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.pixave_ask.take() else { return };
        let name = library_name(&ask.probe.root);
        let files = ask.probe.files().len();
        let left_out = ask.probe.left_out();
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("pixave-import")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(trf("Import {name}?", &[("name", &name)])).size(15.0));
            ui.add_space(4.0);
            let muted = t::p().text_muted;
            ui.label(RichText::new(tr_nf(
                files,
                "{n} file can be imported into a new folder named {name}.",
                "{n} files can be imported into a new folder named {name}.",
                &[("name", &name)],
            )));
            if left_out > 0 {
                ui.label(RichText::new(tr_n(left_out, "{n} item was left out because its file couldn't be identified.", "{n} items were left out because their files couldn't be identified.")).color(muted));
            }
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Pixave's catalogue format isn't public, so collections, tags, ratings, colour labels, notes, links and dates are not brought over. A schema report lists the catalogue's tables without their contents.")).color(muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(files > 0, egui::Button::new(tr("Import"))).clicked() {
                    answer = Some(Answer::Import);
                }
                if ui.button(tr("Cancel")).clicked() {
                    answer = Some(Answer::Cancel);
                }
                if ui.button(tr("Save Schema Report…")).clicked() {
                    answer = Some(Answer::Report);
                }
            });
        });
        if answer.is_none() && modal.should_close() {
            answer = Some(Answer::Cancel);
        }
        match answer {
            None => self.pixave_ask = Some(ask),
            Some(Answer::Cancel) => {}
            Some(Answer::Report) => {
                self.save_pixave_report(&ask.probe);
                self.pixave_ask = Some(ask);
            }
            Some(Answer::Import) => self.start_pixave_import(&ask.probe),
        }
    }

    fn save_pixave_report(&mut self, probe: &Probe) {
        let file = format!("{} schema.txt", library_name(&probe.root));
        let Some(dest) = rfd::FileDialog::new().set_title(tr("Save Schema Report")).set_file_name(file).add_filter("Text", &["txt"]).save_file() else { return };
        match write_atomic(&dest, pixave::report(probe).as_bytes()) {
            Ok(()) => self.flash(trf("Saved {file}", &[("file", &crate::workspaces::tilde(&dest))])),
            Err(e) => self.show_alert(Alert::new(tr("Can't save the schema report"), e.to_string())),
        }
    }

    /// One folder named after the library, then the files through the
    /// ordinary import (copied, with progress, Stop and the duplicate
    /// check).
    fn start_pixave_import(&mut self, probe: &Probe) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let files = probe.files();
        let folder = match lock(&shared).create_folder(None, &library_name(&probe.root)) {
            Ok(id) => id,
            Err(e) => {
                self.show_alert(Alert::new(tr("Can't import the Pixave library"), trf("A folder couldn't be created: {error}", &[("error", &e)])));
                return;
            }
        };
        let jobs: Vec<import::Job> = files.into_iter().map(|path| import::Job { path, opts: self.prefs.import_options(vec![folder.clone()]) }).collect();
        self.flash(tr_n(jobs.len(), "Importing {n} file from Pixave", "Importing {n} files from Pixave"));
        self.spawn_import(jobs);
    }
}

/// Writes through a temporary file beside `dest`, renamed into place.
fn write_atomic(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dest.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, dest));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_written_whole() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("r.txt");
        std::fs::write(&dest, b"old").unwrap();
        write_atomic(&dest, b"new report").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new report");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "no temporary file left behind");
        assert_eq!(library_name(Path::new("/x/My Pics.pxvlibrary")), "My Pics");
    }
}
