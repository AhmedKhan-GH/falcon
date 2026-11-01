//! Eagle's New… menu (File ▸ New…, ⌘N and the + button in the sidebar
//! header): a searchable list of what can be made or brought in. falcon
//! offers the entries it can do (folders, smart folders, imports, the
//! duplicate finder and a blank text document). Eagle's other blank
//! documents come from template files it ships; falcon has none, so those
//! entries are listed disabled.

use eframe::egui::{self, Id, Pos2, Rect, Ui};

use crate::app::App;
use crate::locale::{tr, tr_key, trf};
use crate::logic::Scope;
use crate::shortcuts::Command;

/// The apps of Eagle's blank documents that need a template file, in its
/// order.
pub const NO_TEMPLATE: [&str; 12] = ["Sketch", "Photoshop", "Illustrator", "XD", "Word", "PowerPoint", "Excel", "Keynote", "Pages", "Numbers", "MindNode", "XMind"];

/// The menu's popup, shared by every way of opening it.
fn popup_id() -> Id {
    Id::unique("falcon-new-menu")
}

impl App {
    /// Opens the New… menu at the next chance (`Command::NewMenu`).
    pub(crate) fn request_new_menu(&mut self) {
        self.shell.new_menu = true;
        self.ctx.request_repaint();
    }

    /// The + button: opens the menu, or closes it when open.
    pub(crate) fn toggle_new_menu(&mut self, ctx: &egui::Context) {
        if egui::Popup::is_id_open(ctx, popup_id()) {
            egui::Popup::close_id(ctx, popup_id());
        } else {
            self.request_new_menu();
        }
    }

    /// Draws the New… menu below `anchor` (the sidebar's + button), or at
    /// `at` when the sidebar is hidden, while it is open; a pending
    /// request opens it.
    pub(crate) fn new_menu(&mut self, ui: &Ui, anchor: Option<Rect>, at: Pos2) {
        let ctx = ui.ctx().clone();
        let id = popup_id();
        if std::mem::take(&mut self.shell.new_menu) {
            egui::Popup::open_id(&ctx, id);
        }
        if !egui::Popup::is_id_open(&ctx, id) {
            return;
        }
        let anchor = anchor.unwrap_or_else(|| Rect::from_min_size(at, egui::Vec2::ZERO));
        let mut chosen = None;
        egui::Popup::new(id, ctx.clone(), anchor, ui.layer_id())
            .kind(egui::PopupKind::Menu)
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .style(egui::containers::menu::menu_style)
            .open_memory(None)
            .show(|ui| crate::ui::menus::search::show(ui, |ui| chosen = self.new_menu_body(ui)));
        if let Some(cmd) = chosen {
            egui::Popup::close_id(&ctx, id);
            self.run_command(&ctx, cmd);
        }
    }

    /// The entries; returns the command picked.
    fn new_menu_body(&mut self, ui: &mut Ui) -> Option<Command> {
        ui.set_min_width(230.0);
        let mut chosen = None;
        let in_folder = matches!(self.ws.scope, Scope::Folder(_));
        let mut entry = |app: &App, ui: &mut Ui, label: &str, cmd: Command, enabled: bool| {
            let keys = app.shortcut_text(ui.ctx(), cmd);
            if ui.add_enabled(enabled, egui::Button::new(tr(label)).shortcut_text(keys)).clicked() {
                chosen = Some(cmd);
            }
        };
        entry(self, ui, tr_key("New Folder"), Command::NewFolder, true);
        entry(self, ui, tr_key("New Subfolder"), Command::NewSubfolder, in_folder);
        entry(self, ui, tr_key("New Smart Folder…"), Command::NewSmartFolder, true);
        ui.separator();
        entry(self, ui, tr_key("Import Folders…"), Command::ImportFolders, true);
        entry(self, ui, tr_key("Import Files…"), Command::Import, true);
        entry(self, ui, tr_key("Import Eagle Package…"), Command::ImportPack, true);
        entry(self, ui, tr_key("Find Duplicates"), Command::FindDuplicates, true);
        ui.separator();
        entry(self, ui, tr_key("Text Document"), Command::NewTextFile, true);
        crate::ui::menus::search::submenu(ui, tr("Other Documents"), |ui| {
            for app in NO_TEMPLATE {
                ui.add_enabled(false, egui::Button::new(trf("{app} Document", &[("app", &app)]))).on_disabled_hover_text(tr("Not available: falcon has no template file for this format"));
            }
        });
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unavailable_templates_are_eagles_twelve() {
        assert_eq!(NO_TEMPLATE.len(), 12);
        assert!(!NO_TEMPLATE.contains(&"Text"), "the text document is offered");
    }
}
