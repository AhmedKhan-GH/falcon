//! The confirmation for destructive actions (`Confirm`) and the bulk-change
//! question for actions on many items (`App::push_bulk`).

use eframe::egui::{self, RichText};
use falcon_library::Library;

use crate::app::{Action, App, Confirm};
use crate::logic;
use crate::theme as t;

impl App {
    /// The open `Confirm` dialog; its button queues the action.
    pub(crate) fn confirm_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        if let Some(c) = self.ws.confirm.clone() {
            let (title, detail, label) = match &c {
                Confirm::DeleteFolders(ids) => {
                    let title = match ids.as_slice() {
                        [one] => format!("Delete “{}”?", logic::find_folder(&lib.metadata.folders, one).map_or("", |f| f.name.as_str())),
                        _ => format!("Delete {} folders?", ids.len()),
                    };
                    (title, "Subfolders are deleted too.".to_string(), "Delete")
                }
                Confirm::DeleteSmarts(ids) => (
                    format!("Delete {} smart folders?", ids.len()),
                    "Only the smart folders go; the items they show stay.".to_string(),
                    "Delete",
                ),
                Confirm::DeleteItems(ids) => (
                    format!("Delete {} item{} permanently?", ids.len(), if ids.len() == 1 { "" } else { "s" }),
                    "The files are removed from the library and cannot be restored.".to_string(),
                    "Delete",
                ),
                Confirm::DeleteTags(tags) => (
                    if let [one] = tags.as_slice() { format!("Delete the tag “{one}”?") } else { format!("Delete {} tags?", tags.len()) },
                    "They are removed from every item. The items themselves stay.".to_string(),
                    if tags.len() == 1 { "Delete Tag" } else { "Delete Tags" },
                ),
                Confirm::ClearTagHistory => (
                    "Clear the tag history?".to_string(),
                    "The tag picker's Recent tags start over. Tags stay on their items.".to_string(),
                    "Clear",
                ),
                Confirm::EmptyTrash => {
                    let n = lib.items().iter().filter(|i| i.is_deleted).count();
                    (format!("Empty the Trash ({n})?"), "The files are removed from the library and cannot be restored.".to_string(), "Empty Trash")
                }
                Confirm::TrashInFolder(ids, _) => (
                    format!("Move {} to Trash?", crate::ui::dialogs::export::plural(ids.len(), "item")),
                    "Some of them are in other folders as well.".to_string(),
                    if self.trash_folder_only { "Remove" } else { "Move to Trash" },
                ),
                Confirm::RestoreAll => {
                    let n = lib.items().iter().filter(|i| i.is_deleted).count();
                    (format!("Restore all {n} items in the Trash?"), "They go back to their folders. ⌘Z puts them back in the Trash.".to_string(), "Restore All")
                }
            };
            let modal = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(title).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(detail).color(t::p().text_muted));
                if matches!(c, Confirm::TrashInFolder(..)) {
                    ui.add_space(6.0);
                    ui.radio_value(&mut self.trash_folder_only, true, "Only take them out of this folder")
                        .on_hover_text("Items in no other folder still go to the Trash");
                    ui.radio_value(&mut self.trash_folder_only, false, "Move them all to the Trash");
                }
                if matches!(c, Confirm::DeleteFolders(_)) {
                    ui.add_space(6.0);
                    ui.checkbox(&mut self.ws.folders.trash_items, "Move the items to the Trash as well")
                        .on_hover_text("Items that are also in another folder stay. Untick to keep every item.");
                }
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new(label).color(t::p().icon_red)).clicked() {
                        self.ws.actions.push(match c.clone() {
                            Confirm::DeleteFolders(ids) => Action::Folders(crate::ui::sidebar::folders::FolderAction::Delete(ids, self.ws.folders.trash_items)),
                            Confirm::DeleteSmarts(ids) => Action::Folders(crate::ui::sidebar::folders::FolderAction::DeleteSmart(ids)),
                            Confirm::DeleteItems(ids) => Action::DeletePermanently(ids),
                            Confirm::EmptyTrash => Action::EmptyTrash,
                            Confirm::TrashInFolder(ids, fid) => Action::TrashInFolder(ids, fid, self.trash_folder_only),
                            Confirm::RestoreAll => Action::Trash(lib.items().iter().filter(|i| i.is_deleted).map(|i| i.id.clone()).collect(), false),
                            Confirm::DeleteTags(t) => Action::DeleteTags(t),
                            Confirm::ClearTagHistory => Action::ClearTagHistory,
                        });
                        self.ws.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
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
                ui.label(RichText::new(format!("Change {n} items?")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new("Everything selected is changed at once.").color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
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
