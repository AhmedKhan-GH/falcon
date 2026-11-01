//! The confirmation for destructive actions (`Confirm`) and the bulk-change
//! question for actions on many items (`App::push_bulk`).

use eframe::egui::{self, RichText};
use falcon_library::Library;

use crate::app::{Action, App, Confirm};
use crate::locale::{tr, tr_n, trf};
use crate::logic;
use crate::theme as t;

/// The live items in `ids` and all their subfolders, each counted once.
fn folder_items(lib: &Library, ids: &[String]) -> usize {
    let tree: std::collections::HashSet<String> = ids.iter().flat_map(|id| logic::folder_tree(&lib.metadata.folders, id)).collect();
    lib.items().iter().filter(|i| !i.is_deleted && i.folders.iter().any(|f| tree.contains(f))).count()
}

impl App {
    /// The open `Confirm` dialog; its button queues the action.
    pub(crate) fn confirm_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        if let Some(c) = self.ws.confirm.clone() {
            let (title, detail, label) = match &c {
                Confirm::DeleteFolders(ids) => {
                    let title = match ids.as_slice() {
                        [one] => {
                            let name = logic::find_folder(&lib.metadata.folders, one).map_or("", |f| f.name.as_str());
                            trf("Delete “{name}”?", &[("name", &name)])
                        }
                        _ => tr_n(ids.len(), "Delete {n} folder?", "Delete {n} folders?"),
                    };
                    (title, tr("Subfolders are deleted too."), tr("Delete"))
                }
                Confirm::DeleteSmarts(ids) => (
                    tr_n(ids.len(), "Delete {n} smart folder?", "Delete {n} smart folders?"),
                    tr("Only the smart folders go; the items they show stay."),
                    tr("Delete"),
                ),
                Confirm::DeleteItems(ids) => (
                    tr_n(ids.len(), "Delete {n} item permanently?", "Delete {n} items permanently?"),
                    tr("The files are removed from the library and cannot be restored."),
                    tr("Delete"),
                ),
                Confirm::DeleteTags(tags) => (
                    if let [one] = tags.as_slice() {
                        trf("Delete the tag “{tag}”?", &[("tag", one)])
                    } else {
                        tr_n(tags.len(), "Delete {n} tag?", "Delete {n} tags?")
                    },
                    tr("They are removed from every item. The items themselves stay."),
                    if tags.len() == 1 { tr("Delete Tag") } else { tr("Delete Tags") },
                ),
                Confirm::ClearTagHistory => (
                    tr("Clear the tag history?").to_string(),
                    tr("The tag picker's Recent tags start over. Tags stay on their items."),
                    tr("Clear"),
                ),
                Confirm::EmptyTrash => {
                    let n = lib.items().iter().filter(|i| i.is_deleted).count();
                    (trf("Empty the Trash ({n})?", &[("n", &n)]), tr("The files are removed from the library and cannot be restored."), tr("Empty Trash"))
                }
                Confirm::TrashInFolder(ids, _) => (
                    tr_n(ids.len(), "Move {n} item to Trash?", "Move {n} items to Trash?"),
                    tr("Some of them are in other folders as well."),
                    if self.trash_folder_only { tr("Remove") } else { tr("Move to Trash") },
                ),
                Confirm::RestoreAll => {
                    let n = lib.items().iter().filter(|i| i.is_deleted).count();
                    (
                        tr_n(n, "Restore all {n} item in the Trash?", "Restore all {n} items in the Trash?"),
                        tr("They go back to their folders. ⌘Z puts them back in the Trash."),
                        tr("Restore All"),
                    )
                }
            };
            let modal = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(title).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(detail).color(t::p().text_muted));
                if matches!(c, Confirm::TrashInFolder(..)) {
                    ui.add_space(6.0);
                    ui.radio_value(&mut self.trash_folder_only, true, tr("Only take them out of this folder"))
                        .on_hover_text(tr("Items in no other folder still go to the Trash"));
                    ui.radio_value(&mut self.trash_folder_only, false, tr("Move them all to the Trash"));
                }
                if matches!(c, Confirm::DeleteFolders(_)) {
                    ui.add_space(6.0);
                    ui.checkbox(&mut self.ws.folders.trash_items, tr("Move the items to the Trash as well"))
                        .on_hover_text(tr("Items that are also in another folder stay. Untick to keep every item."));
                }
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new(label).color(t::p().icon_red)).clicked() {
                        if let Confirm::DeleteFolders(ids) = &c {
                            let n = folder_items(lib, ids);
                            self.push_bulk_at(n, 50, Action::Folders(crate::ui::sidebar::folders::FolderAction::Delete(ids.clone(), self.ws.folders.trash_items)));
                        } else {
                            self.ws.actions.push(match c.clone() {
                            Confirm::DeleteSmarts(ids) => Action::Folders(crate::ui::sidebar::folders::FolderAction::DeleteSmart(ids)),
                            Confirm::DeleteItems(ids) => Action::DeletePermanently(ids),
                            Confirm::EmptyTrash => Action::EmptyTrash,
                            Confirm::TrashInFolder(ids, fid) => Action::TrashInFolder(ids, fid, self.trash_folder_only),
                            Confirm::RestoreAll => Action::Trash(lib.items().iter().filter(|i| i.is_deleted).map(|i| i.id.clone()).collect(), false),
                            Confirm::DeleteTags(t) => Action::DeleteTags(t),
                            Confirm::ClearTagHistory => Action::ClearTagHistory,
                            Confirm::DeleteFolders(_) => return,
                            });
                        }
                        self.ws.confirm = None;
                    }
                    if ui.button(tr("Cancel")).clicked() {
                        self.ws.confirm = None;
                    }
                });
            });
            if modal.should_close() {
                self.ws.confirm = None;
            }
        }
    }

    /// Asks before applying the actions held in `ws.bulk`.
    pub(crate) fn bulk_dialog(&mut self, ctx: &egui::Context) {
        if let Some((n, _)) = &self.ws.bulk {
            let n = *n;
            let mut answer = None;
            let modal = egui::Modal::new(egui::Id::unique("bulk")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(tr("Many items at once")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(tr_n(n, "{n} item will be affected. Go ahead?", "{n} items will be affected. Go ahead?")).color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("OK")).clicked() {
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
            match answer {
                Some(true) => {
                    if let Some((_, actions)) = self.ws.bulk.take() {
                        self.ws.actions.extend(actions);
                    }
                }
                Some(false) => self.ws.bulk = None,
                None => {}
            }
        }
    }
}
