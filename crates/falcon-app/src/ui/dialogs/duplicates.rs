//! The dialog for files an import found already in the library (see
//! `crate::import_dupes`), after Eagle's Duplicate File Alert.

use eframe::egui::{self, CornerRadius, Key, Rect, RichText, Sense, Ui, Vec2};
use falcon_library::Library;

use crate::app::{Action, App};
use crate::locale::{tr, trf};
use crate::loader::{Tier, job_for};
use crate::logic;
use crate::theme as t;
use crate::import_dupes::{Choice, Dialog};

impl App {
    /// Moves held duplicates into the dialog once the running import has
    /// finished, so the dialog can count them all, as Eagle waits for its
    /// upload queue.
    pub(crate) fn collect_duplicates(&mut self) {
        let idle = {
            let pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            pr.total == 0 || pr.done >= pr.total
        };
        if !idle {
            return;
        }
        let held = std::mem::take(&mut *self.ws.dupes.lock().unwrap_or_else(|p| p.into_inner()));
        if held.is_empty() {
            return;
        }
        match &mut self.ws.dup_dialog {
            Some(d) => d.extend(held),
            None => self.ws.dup_dialog = Some(Dialog::new(held)),
        }
    }

    /// Draws the duplicate dialog. Return imports with the chosen option,
    /// Esc skips the file; with ⌘, either answers for every file.
    pub(crate) fn duplicate_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.dup_dialog.take() else { return };
        let Some(first) = d.pending.front() else { return };
        let n = d.pending.len();
        let new = first.staged.item().clone();
        let existing = lib.item(&first.existing).cloned();
        let folder_name = |id: Option<&String>| match id {
            Some(id) => logic::find_folder(&lib.metadata.folders, id).map_or_else(|| tr("Uncategorized").to_string(), |f| f.name.clone()),
            None => tr("Uncategorized").to_string(),
        };
        let mut answer: Option<(Choice, bool)> = None;
        let modal = egui::Modal::new(egui::Id::unique("import-duplicate")).show(ctx, |ui| {
            ui.set_width(420.0);
            let title = if n > 1 { trf("Already in the library ({n})", &[("n", &n)]) } else { tr("Already in the library").to_string() };
            ui.label(RichText::new(title).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(trf("“{name}” has the same contents as an item you already have.", &[("name", &format!("{}.{}", new.name, new.ext))])).color(t::p().text_muted));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(6), t::p().field);
                if let Some(item) = &existing {
                    if let Some(tex) = self.ws.loader.get(&item.id, Tier::Grid).cloned() {
                        let ts = tex.size_vec2();
                        let sc = ((r.width() - 8.0) / ts.x).min((r.height() - 8.0) / ts.y);
                        let ir = Rect::from_center_size(r.center(), ts * sc);
                        egui::Image::new((tex.id(), ir.size())).corner_radius(4).paint_at(ui, ir);
                    }
                    if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                        self.ws.frame_jobs.push(j);
                    }
                }
                ui.vertical(|ui| {
                    let line = |ui: &mut Ui, label: &str, text: String| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(label).size(12.0).color(t::p().text_muted));
                            ui.label(RichText::new(text).size(12.0));
                        });
                    };
                    if let Some(item) = &existing {
                        line(ui, tr("In the library:"), format!("{}.{}", item.name, item.ext));
                        line(ui, tr("Folder:"), folder_name(item.folders.first()));
                    }
                    let dims = if new.width > 0 { format!("{} × {}, ", new.width, new.height) } else { String::new() };
                    line(ui, tr("Size:"), format!("{dims}{} KB", new.size.div_ceil(1024)));
                    line(ui, tr("Importing into:"), folder_name(new.folders.first()));
                });
            });
            ui.add_space(8.0);
            ui.radio_value(&mut d.choice, Choice::UseExisting, tr("Use the existing item (it joins the folder and takes its auto-tags)"));
            ui.radio_value(&mut d.choice, Choice::KeepBoth, tr("Keep both (import this copy as a new item)"));
            if n > 1 {
                ui.checkbox(&mut d.apply_all, trf("Do the same for all {n} files", &[("n", &n)]));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Import")).clicked() {
                    answer = Some((d.choice, d.apply_all));
                }
                if ui.button(tr("Don't Import")).clicked() {
                    answer = Some((Choice::Skip, d.apply_all));
                }
            });
            let (enter, cmd) = ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.command));
            if enter {
                answer = Some((d.choice, d.apply_all || cmd));
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                answer = Some((Choice::Skip, d.apply_all || cmd));
            }
        });
        // As in Eagle, a click outside the dialog doesn't answer it.
        let _ = modal;
        if let Some((choice, all)) = answer {
            let files = d.take(all);
            self.ws.actions.push(Action::ResolveDuplicates(files, choice));
        }
        if !d.pending.is_empty() {
            self.ws.dup_dialog = Some(d);
        }
    }
}
