//! Grid items dropped onto a folder: on a sidebar row, a Quick Access row
//! or a subfolder card. As in Eagle's folder drop, a locked folder asks for
//! its password first and takes the items once unlocked (it then stays
//! unlocked this session); a drop out of several picked sidebar folders
//! takes the items out of each of them (`logic::folder_drop_picked`).

use eframe::egui::{self, Key, RichText, TextEdit, Ui};
use falcon_library::Library;

use crate::app::{Action, App};
use crate::locale::{tr, trf};
use crate::logic;
use crate::theme as t;

/// A drop waiting for its locked target's password.
pub(crate) struct DropUnlock {
    folder: String,
    /// How many items, for the bulk confirmation once unlocked.
    count: usize,
    action: Action,
    text: String,
    wrong: bool,
}

impl App {
    /// Drops `ids` onto the folder `target`: moved out of the open folder
    /// (or every picked folder), or added with ⌥, after the password when
    /// the target is locked.
    pub(crate) fn drop_items_on_folder(&mut self, ui: &Ui, lib: &Library, ids: &[String], target: &str) {
        let alt = ui.input(|i| i.modifiers.alt);
        let Some(op) = logic::folder_drop_picked(&self.ws.scope, &self.ws.folders.sel.ids, target, alt) else { return };
        let action = Action::DropOnFolder(ids.to_vec(), target.to_string(), op);
        let locked = logic::find_folder(&lib.metadata.folders, target).is_some_and(|f| crate::organize::is_shut(f, &self.ws.org.unlocked));
        if locked {
            self.ws.org.drop_unlock = Some(DropUnlock { folder: target.to_string(), count: ids.len(), action, text: String::new(), wrong: false });
        } else {
            self.push_bulk(ids.len(), action);
        }
    }

    /// The password prompt of a drop onto a locked folder.
    pub(crate) fn drop_unlock_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.org.drop_unlock.take() else { return };
        let Some(folder) = logic::find_folder(&lib.metadata.folders, &d.folder) else { return };
        let mut keep = true;
        let mut unlock = false;
        let modal = egui::Modal::new(egui::Id::unique("drop-unlock")).show(ctx, |ui: &mut Ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(trf("Enter the password of “{name}” to put the items in it", &[("name", &folder.name)])).size(15.0));
            ui.add_space(6.0);
            if !folder.password_tips.is_empty() {
                ui.label(RichText::new(trf("Hint: {hint}", &[("hint", &folder.password_tips)])).color(t::p().text_muted));
            }
            let te = ui.add(TextEdit::singleline(&mut d.text).password(true).hint_text(tr("Enter password")).desired_width(f32::INFINITY));
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            if te.changed() {
                d.wrong = false;
            }
            if d.wrong {
                ui.label(RichText::new(tr("Wrong password")).color(t::p().icon_red));
            }
            unlock |= te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                unlock |= ui.button(tr("Unlock")).clicked();
                if ui.button(tr("Cancel")).clicked() {
                    keep = false;
                }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep && unlock {
            if lib.verify_folder_password(&d.folder, &d.text) {
                self.ws.org.unlocked.insert(d.folder.clone());
                self.push_bulk(d.count, d.action);
                return;
            }
            d.wrong = true;
            d.text.clear();
        }
        if keep {
            self.ws.org.drop_unlock = Some(d);
        }
    }
}
