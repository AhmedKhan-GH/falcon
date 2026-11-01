//! Eagle's New… menu (File ▸ New…, ⌘N and the + button in the sidebar
//! header): a searchable list of what can be made or brought in. falcon
//! offers the entries it can do (folders, smart folders, imports, an Auto
//! Import submenu that opens its Preferences pane and shows the watched
//! folder, a Find Duplicates submenu for all items, the current list or
//! the selection, and a blank text document). Eagle's other blank
//! documents come from template files it ships; falcon has none, so those
//! entries are listed disabled.

use eframe::egui::{self, Id, Pos2, Rect, Ui};

use crate::app::App;
use crate::locale::{tr, tr_key, trf};
use crate::logic::Scope;
use crate::shortcuts::Command;
use crate::ui::grid::duplicates::Range;

/// What a click in the menu asks for.
enum Pick {
    Command(Command),
    Duplicates(Range),
    AutoImportSettings,
}

/// Whether the Find Duplicates scopes are on: the current list needs items
/// in the view, the selection needs more than one item.
fn duplicate_scopes(view_items: usize, selected: usize) -> (bool, bool) {
    (view_items > 0, selected > 1)
}

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
        if let Some(pick) = chosen {
            egui::Popup::close_id(&ctx, id);
            match pick {
                Pick::Command(cmd) => self.run_command(&ctx, cmd),
                Pick::Duplicates(range) => self.find_duplicates_in(range),
                Pick::AutoImportSettings => {
                    self.open_preferences();
                    // After `show_with`, which resets the pane.
                    self.prefs_window.set_pane(crate::prefs::Section::AutoImport);
                }
            }
        }
    }

    /// The entries; returns what was picked.
    fn new_menu_body(&mut self, ui: &mut Ui) -> Option<Pick> {
        ui.set_min_width(230.0);
        let mut chosen = None;
        let in_folder = matches!(self.ws.scope, Scope::Folder(_));
        let entry = |app: &App, ui: &mut Ui, chosen: &mut Option<Pick>, label: &str, cmd: Command, enabled: bool| {
            let keys = app.shortcut_text(ui.ctx(), cmd);
            if ui.add_enabled(enabled, egui::Button::new(tr(label)).shortcut_text(keys)).clicked() {
                *chosen = Some(Pick::Command(cmd));
            }
        };
        entry(self, ui, &mut chosen, tr_key("New Folder"), Command::NewFolder, true);
        entry(self, ui, &mut chosen, tr_key("New Subfolder"), Command::NewSubfolder, in_folder);
        entry(self, ui, &mut chosen, tr_key("New Smart Folder…"), Command::NewSmartFolder, true);
        ui.separator();
        entry(self, ui, &mut chosen, tr_key("Import Folders…"), Command::ImportFolders, true);
        entry(self, ui, &mut chosen, tr_key("Import Files…"), Command::Import, true);
        entry(self, ui, &mut chosen, tr_key("Import Eagle Package…"), Command::ImportPack, true);
        crate::ui::menus::search::submenu(ui, tr("Auto-Import"), |ui| {
            if ui.button(tr("Settings…")).clicked() {
                chosen = Some(Pick::AutoImportSettings);
            }
            if let (true, Some(path)) = (self.prefs.auto_import, &self.prefs.auto_import_path) {
                ui.add_enabled(false, egui::Button::new(path.display().to_string()));
            }
        });
        let (list_on, selection_on) = duplicate_scopes(self.ws.order.len(), self.ws.selected.len());
        crate::ui::menus::search::submenu(ui, tr("Find Duplicates"), |ui| {
            for (label, range, on) in [(tr_key("All Items"), Range::All, true), (tr_key("Current List"), Range::View, list_on), (tr_key("Selected"), Range::Selected, selection_on)] {
                if ui.add_enabled(on, egui::Button::new(tr(label))).clicked() {
                    chosen = Some(Pick::Duplicates(range));
                }
            }
        });
        ui.separator();
        entry(self, ui, &mut chosen, tr_key("Text Document"), Command::NewTextFile, true);
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
    fn duplicate_scopes_follow_the_counts() {
        assert_eq!(duplicate_scopes(0, 0), (false, false));
        assert_eq!(duplicate_scopes(5, 1), (true, false));
        assert_eq!(duplicate_scopes(5, 2), (true, true));
    }

    #[test]
    fn the_unavailable_templates_are_eagles_twelve() {
        assert_eq!(NO_TEMPLATE.len(), 12);
        assert!(!NO_TEMPLATE.contains(&"Text"), "the text document is offered");
    }
}
