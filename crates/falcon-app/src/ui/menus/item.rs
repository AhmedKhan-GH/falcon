//! The item context menu: actions on one item or the selection.

use eframe::egui::{self, RichText, Ui};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::{Action, App, Confirm, commands, image_edit};
use crate::locale::{tr, tr_n};
use crate::logic::{self, Scope};
use crate::theme::{self as t};

impl App {
    /// The item context menu, for a grid card or the item in the viewer.
    /// `targets` are the items it acts on. Returns true when Open was
    /// chosen (the grid's menu only; the viewer's has no Open).
    /// The item context menu, with Eagle's search box on top.
    pub(crate) fn item_menu(&mut self, ui: &mut Ui, lib: &Library, item: &Item, targets: &[String]) -> bool {
        crate::ui::menus::search::show(ui, |ui| self.item_menu_body(ui, lib, item, targets))
    }

    pub(crate) fn item_menu_body(&mut self, ui: &mut Ui, lib: &Library, item: &Item, targets: &[String]) -> bool {
        let in_viewer = self.ws.viewer.as_deref() == Some(item.id.as_str());
        let mut opened = false;
        ui.set_min_width(200.0);
        if !in_viewer && ui.button(tr("Open")).clicked() {
            opened = true;
        }
        self.open_menu(ui, &crate::ui::viewer::open::originals(lib, targets));
        if targets.len() == 1 && self.ws.scope != Scope::Trash {
            self.location_menu(ui, lib, item);
        }
        self.item_copy_menu(ui, targets);
        if self.ws.scope != Scope::Trash {
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::AddToFolders);
            if ui.add(egui::Button::new(tr("Add to Folders…")).shortcut_text(keys)).clicked() {
                self.open_add_to_folders(lib, targets.to_vec());
            }
        }
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::AddToLastFolder);
        let last = commands::items::last_used(&self.recent_folders, |id| logic::find_folder(&lib.metadata.folders, id).is_some());
        let tip = last.as_deref().and_then(|id| logic::find_folder(&lib.metadata.folders, id)).map(|f| f.name.clone());
        let b = ui.add_enabled(last.is_some(), egui::Button::new(tr("Add to Last Used Folder")).shortcut_text(keys));
        if b.clicked() {
            self.add_to_last_used(targets.to_vec());
        }
        if let Some(name) = tip {
            b.on_hover_text(name);
        }
        // Eagle's Copy Tags and Paste Tags.
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::CopyTags);
        if ui.add(egui::Button::new(tr("Copy Tags")).shortcut_text(keys)).clicked() {
            let items: Vec<&Item> = targets.iter().filter_map(|id| lib.item(id)).collect();
            let tags = crate::tags::common_tags(&items);
            let ctx = ui.ctx().clone();
            self.copy_tags(&ctx, tags);
        }
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::PasteTags);
        if ui.add_enabled(self.copied_tags.is_some(), egui::Button::new(tr("Paste Tags")).shortcut_text(keys)).clicked() {
            self.paste_tags(targets.to_vec());
        }
        if self.ws.scope != Scope::Trash {
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Rename);
            let label = if targets.len() > 1 { tr_n(targets.len(), "Rename {n} Item…", "Rename {n} Items…") } else { tr("Rename").to_string() };
            if ui.add(egui::Button::new(label).shortcut_text(keys)).clicked() {
                if targets.len() == 1 {
                    self.ws.selected = targets.to_vec();
                }
                self.rename_items(targets.to_vec());
            }
        }
        if targets.len() == 1 {
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Duplicate);
            if ui.add(egui::Button::new(tr("Duplicate")).shortcut_text(keys)).clicked() {
                self.ws.actions.push(Action::Edit(image_edit::Op::Duplicate(item.id.clone())));
            }
        }
        // Eagle offers it for several items.
        if targets.len() > 1 && self.ws.scope != Scope::Trash && ui.button(tr_n(targets.len(), "New Folder with {n} Item…", "New Folder with {n} Items…")).clicked() {
            self.ws.folders.new_with = Some((targets.to_vec(), String::new()));
        }
        self.edit_menu(ui, item, targets, lib);
        if !in_viewer {
            let cur = logic::ThumbBg::of(item);
            crate::ui::menus::search::submenu(ui, tr("Thumbnail Background"), |ui| {
                if ui.add(egui::Button::selectable(cur.is_none(), tr("None"))).clicked() {
                    self.ws.actions.push(Action::Background(targets.to_vec(), None));
                }
                ui.separator();
                for bg in logic::ThumbBg::ALL {
                    if ui.add(egui::Button::selectable(cur == Some(bg), bg.label())).clicked() {
                        self.ws.actions.push(Action::Background(targets.to_vec(), Some(bg)));
                    }
                }
            });
        }
        if self.ws.scope != Scope::Trash {
            self.webp_menu(ui, item, targets);
        }
        self.item_export_menu(ui, targets);
        self.add_to_library_menu(ui, crate::ui::dialogs::add_to_library::AddWhat::Items(targets.to_vec()));
        // Eagle's Open Item in New Window.
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::OpenNewWindow);
        if ui.add(egui::Button::new(tr("Open in New Window")).shortcut_text(keys)).clicked() {
            self.open_preview_items(lib, targets);
        }
        // Eagle's Navigator toggle, in the viewer.
        if in_viewer {
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Navigator);
            if ui.add(egui::Button::new(tr("Navigator")).selected(!self.modes.hide_navigator).shortcut_text(keys)).clicked() {
                self.modes.hide_navigator = !self.modes.hide_navigator;
            }
        }
        // Eagle's grayscale preview toggle; the menu stays open, as Eagle's
        // entry does.
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Grayscale);
        if ui.add(egui::Button::new(tr("Grayscale")).selected(self.modes.grayscale).shortcut_text(keys)).clicked() {
            self.modes.grayscale = !self.modes.grayscale;
        }
        // Eagle's Enter / Leave Presentation Mode.
        let label = if self.modes.presenting { tr("Leave Presentation Mode") } else { tr("Enter Presentation Mode") };
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Presentation);
        if ui.add(egui::Button::new(label).shortcut_text(keys)).clicked() {
            if !in_viewer && !self.modes.presenting {
                self.ws.viewer = Some(item.id.clone());
            }
            self.toggle_presentation(ui.ctx());
        }
        if crate::share::AVAILABLE && self.ws.scope != Scope::Trash && ui.button(tr("Share…")).clicked() {
            let files = crate::ui::viewer::open::originals(lib, targets);
            let at = ui.ctx().input(|i| i.pointer.interact_pos()).unwrap_or_default();
            crate::share::share(&files, (at.x, at.y));
            ui.close();
        }
        ui.separator();
        if self.ws.scope == Scope::Trash {
            if ui.button(tr("Restore")).clicked() {
                self.leave_viewed(targets);
                self.ws.actions.push(Action::Trash(targets.to_vec(), false));
            }
            if ui.button(RichText::new(tr("Delete Permanently")).color(t::p().icon_red)).clicked() {
                self.ws.confirm = Some(Confirm::DeleteItems(targets.to_vec()));
            }
        } else {
            if let Scope::Folder(fid) = self.ws.scope.clone() {
                let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::RemoveFromFolder);
                if ui.add(egui::Button::new(tr("Remove from Folder")).shortcut_text(keys)).clicked() {
                    self.leave_viewed(targets);
                    self.select_after_removal(targets);
                    self.ws.actions.push(Action::RemoveFromFolder(targets.to_vec(), fid));
                }
            }
            if ui.button(tr("Move to Trash")).clicked() {
                self.leave_viewed(targets);
                self.push_bulk(targets.len(), Action::TrashAsk(targets.to_vec()));
            }
        }
        opened
    }
}
