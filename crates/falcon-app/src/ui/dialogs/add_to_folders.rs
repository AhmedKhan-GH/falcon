//! The Add to Folders picker (Find ▸ Add to Folders…, ⇧⌘J, the item
//! menu and the inspector's +): a searchable folder tree with the recently
//! used folders on top, ticks for the folders the items go in, and folder
//! creation. The rules live in `crate::folder_picker`.

use eframe::egui::{self, Key, Modifiers, Pos2, Rect, RichText, Sense, TextEdit, Ui, vec2};
use falcon_library::Library;

use crate::app::{Action, App};
use crate::folder_icons;
use crate::folder_picker::{self, NewPlace, Picker, Row};
use crate::icons as ic;
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::sidebar::folders::FolderAction;
use crate::ui::widgets::{galley, icon};

impl App {
    /// Opens the picker for `items`, with the folders they all share
    /// ticked.
    pub(crate) fn open_add_to_folders(&mut self, lib: &Library, items: Vec<String>) {
        if items.is_empty() || self.ws.scope == Scope::Trash {
            return;
        }
        let list: Vec<_> = items.iter().filter_map(|id| lib.item(id)).collect();
        let existing = logic::shared_folders(&list);
        let current = match &self.ws.scope {
            Scope::Folder(id) => Some(id.clone()),
            _ => None,
        };
        let mut p = Picker::new(items, current, existing, &lib.metadata.folders);
        p.remove_from_current = self.ws.folders.picker_remove;
        self.ws.folders.picker = Some(p);
    }

    /// The picker, when open.
    pub(crate) fn add_to_folders_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut p) = self.ws.folders.picker.take() else { return };
        let roots = &lib.metadata.folders;
        let unlocked = self.ws.org.unlocked.clone();
        let shut = |f: &falcon_library::model::Folder| crate::organize::is_shut(f, &unlocked);
        let mut keep = true;
        let mut save = false;
        let modal = egui::Modal::new(egui::Id::unique("add-to-folders")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new("Add to Folders").size(15.0));
            ui.add_space(6.0);
            if p.naming.is_some() {
                self.picker_name_prompt(ui, &mut p);
                return;
            }
            let rows = folder_picker::rows(&p, roots, &self.recent_folders, shut);
            // Keys while the search field has focus, as in Eagle.
            let (mut toggle, mut save_key) = (false, false);
            let te = ui.add(TextEdit::singleline(&mut p.search).hint_text("Search").desired_width(f32::INFINITY));
            if te.changed() {
                let rows = folder_picker::rows(&p, roots, &self.recent_folders, shut);
                p.highlight = folder_picker::first_pickable(&rows);
            }
            if !te.has_focus() && ui.memory(|m| m.focused().is_none()) {
                te.request_focus();
            }
            if te.has_focus() || te.lost_focus() {
                ui.input_mut(|i| {
                    if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                        p.step(&rows, true);
                        p.scroll = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                        p.step(&rows, false);
                        p.scroll = true;
                    }
                    if p.search.is_empty()
                        && let Some(Row::Folder { id, .. }) = rows.get(p.highlight)
                    {
                        if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                            p.expanded.remove(id);
                        }
                        if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                            p.expanded.insert(id.clone());
                        }
                    }
                    if i.key_pressed(Key::Enter) {
                        if i.modifiers.command {
                            save_key = true;
                        } else {
                            toggle = true;
                        }
                    }
                });
            }
            if te.lost_focus() {
                te.request_focus();
            }
            if toggle && let Some(row) = rows.get(p.highlight).cloned() {
                self.picker_pick(&mut p, &row);
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if rows.iter().all(|r| matches!(r, Row::Heading)) && roots.is_empty() {
                    ui.label(RichText::new("No folders yet").color(t::p().text_muted));
                }
                for (i, row) in rows.iter().enumerate() {
                    self.picker_row(ui, lib, &mut p, i, row);
                }
            });
            p.scroll = false;
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if p.current.is_some() {
                    ui.checkbox(&mut p.remove_from_current, "Take them out of the open folder");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = format!("Add {}", crate::ui::dialogs::export::plural(p.items.len(), "item"));
                    if ui.add_enabled(p.can_save(), egui::Button::new(label)).clicked() || (save_key && p.can_save()) {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        });
        if modal.should_close() && p.naming.is_none() {
            keep = false;
        }
        self.ws.folders.picker_remove = p.remove_from_current;
        if save {
            let (add, remove) = p.changes(roots);
            if !add.is_empty() || !remove.is_empty() {
                self.ws.actions.push(Action::Folders(FolderAction::Categorize(p.items.clone(), add, remove)));
            }
            keep = false;
        }
        if keep {
            self.ws.folders.picker = Some(p);
        }
    }

    /// A row clicked or chosen with Enter.
    fn picker_pick(&mut self, p: &mut Picker, row: &Row) {
        match row {
            Row::Recent(id) | Row::Folder { id, .. } => p.toggle(id),
            Row::Create => {
                let name = logic::clean_folder_name(&p.search);
                if !name.is_empty() {
                    self.ws.actions.push(Action::Folders(FolderAction::PickerCreate(name, NewPlace::Top, true)));
                    p.search.clear();
                }
            }
            Row::Heading => {}
        }
    }

    /// One row of the picker.
    fn picker_row(&mut self, ui: &mut Ui, lib: &Library, p: &mut Picker, i: usize, row: &Row) {
        let roots = &lib.metadata.folders;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 27.0), Sense::click());
        if matches!(row, Row::Heading) {
            let n = lib.all_folders().len();
            ui.painter().text(
                Pos2::new(rect.left() + 6.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!("Folders ({n})"),
                font(12.5),
                t::p().text_muted,
            );
            let plus = Rect::from_center_size(Pos2::new(rect.right() - 14.0, rect.center().y), vec2(20.0, 20.0));
            let r = ui.interact(plus, ui.scope_id().with("picker-new-top"), Sense::click()).on_hover_text("New Folder");
            icon(ui, plus.center(), ic::PLUS, 14.0, if r.hovered() { t::p().text } else { t::p().text_muted });
            if r.clicked() {
                p.naming = Some((NewPlace::Top, String::new()));
            }
            return;
        }
        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            p.highlight = i;
        }
        if p.highlight == i {
            ui.painter().rect_filled(rect, t::RADIUS, t::p().row_hover);
            if p.scroll {
                resp.scroll_to_me(None);
            }
        }
        let cy = rect.center().y;
        match row {
            Row::Create => {
                icon(ui, Pos2::new(rect.left() + 16.0, cy), ic::PLUS, 14.0, t::p().text_muted);
                let g = galley(ui, &format!("Create “{}”", p.search.trim()), font(13.0), t::p().text, rect.width() - 40.0);
                ui.painter().galley(Pos2::new(rect.left() + 32.0, cy - g.size().y / 2.0), g, t::p().text);
                if resp.clicked() {
                    self.picker_pick(p, row);
                }
            }
            Row::Recent(id) | Row::Folder { id, .. } => {
                let Some(f) = logic::find_folder(roots, id) else { return };
                let (depth, kids, recent) = match row {
                    Row::Folder { depth, kids, .. } => (*depth, *kids, false),
                    _ => (0, false, true),
                };
                let x = rect.left() + 18.0 + 18.0 * depth as f32;
                let mut on_control = false;
                if kids {
                    let open = p.expanded.contains(id);
                    let caret = Rect::from_center_size(Pos2::new(x - 12.0, cy), vec2(14.0, rect.height()));
                    icon(ui, caret.center(), if open { ic::CARET_DOWN } else { ic::CARET_RIGHT }, 11.0, t::p().text_muted);
                    if resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| caret.contains(q)) {
                        on_control = true;
                        if !p.expanded.remove(id) {
                            p.expanded.insert(id.clone());
                        }
                    }
                }
                let (glyph, tint) = if recent { (ic::CLOCK, t::p().text_muted) } else { App::folder_look(f, false) };
                let size = if folder_icons::is_text(glyph) { 10.0 } else { 15.0 };
                icon(ui, Pos2::new(x + 2.0, cy), glyph, size, tint);
                let label = if recent { logic::folder_path(roots, id).unwrap_or_else(|| f.name.clone()) } else { f.name.clone() };
                let g = galley(ui, &label, font(13.0), t::p().text, rect.right() - x - 60.0);
                ui.painter().galley(Pos2::new(x + 16.0, cy - g.size().y / 2.0), g, t::p().text);
                if p.checked.contains(id) {
                    icon(ui, Pos2::new(rect.right() - 36.0, cy), ic::CHECK, 14.0, t::p().primary);
                }
                if recent && (resp.hovered() || p.highlight == i) {
                    let x_rect = Rect::from_center_size(Pos2::new(rect.right() - 12.0, cy), vec2(18.0, 18.0));
                    icon(ui, x_rect.center(), ic::X, 12.0, t::p().text_muted);
                    if resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| x_rect.contains(q)) {
                        on_control = true;
                        self.recent_folders.retain(|r| r != id);
                    }
                }
                if resp.clicked() && !on_control {
                    self.picker_pick(p, row);
                }
                if !recent {
                    resp.context_menu(|ui| {
                        if ui.button("New Subfolder").clicked() {
                            p.naming = Some((NewPlace::Inside(id.clone()), String::new()));
                        }
                        if ui.button("New Folder Below").clicked() {
                            p.naming = Some((NewPlace::After(id.clone()), String::new()));
                        }
                    });
                }
            }
            Row::Heading => {}
        }
    }

    /// Names a folder made from the heading's + or a row's menu.
    fn picker_name_prompt(&mut self, ui: &mut Ui, p: &mut Picker) {
        let Some((place, mut name)) = p.naming.take() else { return };
        ui.label(RichText::new("New Folder").color(t::p().text_muted));
        let te = ui.add(TextEdit::singleline(&mut name).hint_text("Folder name").desired_width(f32::INFINITY));
        if !te.has_focus() && ui.memory(|m| m.focused().is_none()) {
            te.request_focus();
        }
        let enter = te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        ui.add_space(8.0);
        let mut done = cancel;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clean = logic::clean_folder_name(&name);
            if (ui.add_enabled(!clean.is_empty(), egui::Button::new("Create")).clicked() || (enter && !clean.is_empty())) && !done {
                self.ws.actions.push(Action::Folders(FolderAction::PickerCreate(clean, place.clone(), false)));
                done = true;
            }
            if ui.button("Cancel").clicked() {
                done = true;
            }
        });
        if !done {
            p.naming = Some((place, name));
        }
    }
}
