//! The Add to Folders picker (Find ▸ Add to Folders…, ⇧⌘J, the item
//! menu and the inspector's +): a panel beside the pointer with a search
//! field and the All, Recent and Selected tabs, ticks for the folders the
//! items go in, and folder creation. It has no Save: closing it (a click
//! outside or Esc) applies the ticks as one undo step, or nothing if none
//! changed. The rules live in `crate::folder_picker`.

use eframe::egui::{self, Align2, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, TextEdit, Ui, vec2};
use falcon_library::Library;

use crate::app::{Action, App};
use crate::folder_icons;
use crate::folder_picker::{self, NewPlace, Picker, Row, Tab};
use crate::icons as ic;
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::sidebar::folders::FolderAction;
use crate::ui::widgets::{galley, icon};

/// The panel's width, and the height of its list (window points).
const PANEL_W: f32 = 300.0;
const LIST_H: f32 = 330.0;
/// The panel's height with its search row, for keeping it on screen.
const PANEL_H: f32 = LIST_H + 96.0;

impl App {
    /// Opens the picker for `items`, with the folders they all share
    /// ticked.
    pub(crate) fn open_add_to_folders(&mut self, lib: &Library, items: Vec<String>) {
        // Asked again while open, it stays open with its ticks.
        if items.is_empty() || self.ws.scope == Scope::Trash || self.ws.folders.picker.is_some() {
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

    /// Where the panel's top left goes: beside the pointer (10 points
    /// right and 10 up, as Eagle's), flipped to its left or lifted when it
    /// would run off the window.
    fn picker_corner(at: (f32, f32), screen: Rect) -> Pos2 {
        let (mut x, mut y) = (at.0 + 10.0, at.1 - 10.0);
        if at.0 + PANEL_W > screen.right() {
            x = at.0 - PANEL_W - 20.0;
        }
        if at.1 + PANEL_H > screen.bottom() - 20.0 {
            y = (screen.bottom() - PANEL_H - 20.0).max(20.0);
        }
        Pos2::new(x.max(screen.left()), y.max(screen.top()))
    }

    /// The picker, when open.
    pub(crate) fn add_to_folders_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut p) = self.ws.folders.picker.take() else { return };
        let roots = &lib.metadata.folders;
        let unlocked = self.ws.org.unlocked.clone();
        let shut = |f: &falcon_library::model::Folder| crate::organize::is_shut(f, &unlocked);
        let screen = ctx.content_rect();
        let at = *p.anchor.get_or_insert_with(|| {
            let pos = ctx.pointer_latest_pos().unwrap_or_else(|| screen.center());
            (pos.x, pos.y)
        });
        let corner = Self::picker_corner(at, screen);
        p.refresh(roots, &self.recent_folders, shut);
        let naming_open = p.naming.is_some();
        let area = egui::Modal::default_area(egui::Id::unique("add-to-folders")).anchor(Align2::LEFT_TOP, corner.to_vec2());
        let modal = egui::Modal::new(egui::Id::unique("add-to-folders")).area(area).backdrop_color(Color32::TRANSPARENT).show(ctx, |ui| {
            ui.set_width(PANEL_W);
            if p.naming.is_some() {
                self.picker_name_prompt(ui, &mut p);
                return;
            }
            // Keys while the search field has focus, as in Eagle.
            let mut toggle = None;
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::Tab) {
                    let next = p.tab.next();
                    p.set_tab(next);
                }
            });
            ui.horizontal(|ui| {
                let tabs_w = 3.0 * 28.0;
                let te = ui.add(TextEdit::singleline(&mut p.search).hint_text(crate::locale::tr("Search")).desired_width(ui.available_width() - tabs_w));
                if te.changed() {
                    p.dirty = true;
                }
                if !te.has_focus() && ui.memory(|m| m.focused().is_none()) {
                    te.request_focus();
                }
                if te.has_focus() || te.lost_focus() {
                    ui.input_mut(|i| {
                        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                            p.step(true);
                            p.scroll = true;
                        }
                        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                            p.step(false);
                            p.scroll = true;
                        }
                        if p.search.is_empty()
                            && let Some(Row::Folder { id, .. }) = p.highlight.and_then(|h| p.listed.get(h)).cloned()
                        {
                            if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                                p.set_expanded(&id, false);
                            }
                            if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                                p.set_expanded(&id, true);
                            }
                        }
                        if i.key_pressed(Key::Enter) {
                            toggle = Some(i.modifiers.command);
                        }
                    });
                }
                if te.lost_focus() {
                    te.request_focus();
                }
                for (tab, glyph, name) in [(Tab::All, ic::FOLDER, crate::locale::tr("All")), (Tab::Recent, ic::CLOCK, crate::locale::tr("Recent")), (Tab::Selected, ic::CHECK, crate::locale::tr("Selected"))] {
                    let b = egui::Button::new(RichText::new(glyph).size(14.0)).selected(p.tab == tab).min_size(vec2(24.0, 0.0));
                    if ui.add(b).on_hover_text(name).clicked() {
                        p.set_tab(tab);
                    }
                }
            });
            // A key or tab change in this frame may have made the rows stale.
            p.refresh(roots, &self.recent_folders, shut);
            if let Some(keep_search) = toggle
                && let Some(row) = p.highlight.and_then(|h| p.listed.get(h)).cloned()
            {
                self.picker_pick(&mut p, &row, keep_search);
            }
            ui.add_space(4.0);
            let rows = p.listed.clone();
            egui::ScrollArea::vertical().max_height(LIST_H).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if rows.is_empty() {
                    let none = match p.tab {
                        Tab::All => crate::locale::tr("No folders yet"),
                        Tab::Recent => crate::locale::tr("No folder used yet"),
                        Tab::Selected => crate::locale::tr("No folder chosen"),
                    };
                    ui.add_space(12.0);
                    ui.label(RichText::new(none).color(t::p().text_muted));
                }
                for (i, row) in rows.iter().enumerate() {
                    self.picker_row(ui, lib, &mut p, i, row);
                }
            });
            p.scroll = false;
            if p.current.is_some() {
                ui.add_space(6.0);
                ui.checkbox(&mut p.remove_from_current, crate::locale::tr("Take them out of the open folder"));
            }
        });
        self.ws.folders.picker_remove = p.remove_from_current;
        if modal.should_close() && !naming_open {
            // Closing applies what was ticked, as one undo step; nothing
            // changed, nothing to apply.
            let (add, remove) = p.changes(roots);
            if !add.is_empty() || !remove.is_empty() {
                self.ws.actions.push(Action::Folders(FolderAction::Categorize(p.items.clone(), add, remove)));
            }
            return;
        }
        self.ws.folders.picker = Some(p);
    }

    /// A row clicked or chosen with Enter; `keep_search` (⌘ held) leaves
    /// the search typed in place.
    fn picker_pick(&mut self, p: &mut Picker, row: &Row, keep_search: bool) {
        match row {
            Row::Recent(id) | Row::Folder { id, .. } => p.pick(id, keep_search),
            Row::Create => {
                let name = logic::clean_folder_name(&p.search);
                if !name.is_empty() {
                    self.ws.actions.push(Action::Folders(FolderAction::PickerCreate(name, NewPlace::Top, true)));
                    p.search.clear();
                    p.dirty = true;
                }
            }
            Row::Heading | Row::Separator => {}
        }
    }

    /// One row of the picker.
    fn picker_row(&mut self, ui: &mut Ui, lib: &Library, p: &mut Picker, i: usize, row: &Row) {
        let roots = &lib.metadata.folders;
        if matches!(row, Row::Separator | Row::Heading) {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, t::p().text_muted.gamma_multiply(0.3)));
            return;
        }
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 27.0), Sense::click());
        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            p.highlight = Some(i);
        }
        if p.highlight == Some(i) {
            ui.painter().rect_filled(rect, t::RADIUS, t::p().row_hover);
            if p.scroll {
                resp.scroll_to_me(None);
            }
        }
        let command = ui.input(|i| i.modifiers.command);
        let cy = rect.center().y;
        match row {
            Row::Create => {
                icon(ui, Pos2::new(rect.left() + 16.0, cy), ic::PLUS, 14.0, t::p().text_muted);
                let g = galley(ui, &crate::locale::trf("Create “{text}”", &[("text", &p.search.trim())]), font(13.0), t::p().text, rect.width() - 40.0);
                ui.painter().galley(Pos2::new(rect.left() + 32.0, cy - g.size().y / 2.0), g, t::p().text);
                if resp.clicked() {
                    self.picker_pick(p, row, command);
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
                        p.set_expanded(id, !open);
                    }
                }
                let (glyph, tint) = if recent { (ic::CLOCK, t::p().text_muted) } else { App::folder_look(f, false) };
                let size = if folder_icons::is_text(glyph) { 10.0 } else { 15.0 };
                icon(ui, Pos2::new(x + 2.0, cy), glyph, size, tint);
                let label = if recent { logic::folder_path(roots, id).unwrap_or_else(|| f.name.clone()) } else { f.name.clone() };
                // Flat lists (a search, the Selected tab) say where each folder is.
                let flat = !recent && (p.tab == Tab::Selected || !p.search.trim().is_empty());
                let hint = if flat { folder_picker::parent_name(roots, id) } else { None };
                let label_w = rect.right() - x - 44.0 - if hint.is_some() { 90.0 } else { 0.0 };
                let g = galley(ui, &label, font(13.0), t::p().text, label_w);
                ui.painter().galley(Pos2::new(x + 16.0, cy - g.size().y / 2.0), g, t::p().text);
                if let Some(parent) = hint {
                    let g = galley(ui, parent, font(11.5), t::p().text_muted, 84.0);
                    ui.painter().galley(Pos2::new(rect.right() - 30.0 - g.size().x, cy - g.size().y / 2.0), g, t::p().text_muted);
                }
                if p.checked.contains(id) {
                    icon(ui, Pos2::new(rect.right() - 14.0, cy), ic::CHECK, 14.0, t::p().primary);
                }
                if resp.clicked() && !on_control {
                    self.picker_pick(p, row, command);
                }
                if recent {
                    // A ticked folder can sit in this block without being
                    // recent; there is nothing to forget then.
                    if self.recent_folders.contains(id) {
                        resp.context_menu(|ui| {
                            if ui.button(crate::locale::tr("Remove")).clicked() {
                                folder_picker::forget_recent(&mut self.recent_folders, id);
                                p.dirty = true;
                            }
                        });
                    }
                } else {
                    resp.context_menu(|ui| {
                        if ui.button(crate::locale::tr("New Subfolder")).clicked() {
                            p.naming = Some((NewPlace::Inside(id.clone()), String::new()));
                        }
                        if ui.button(crate::locale::tr("New Folder Below")).clicked() {
                            p.naming = Some((NewPlace::After(id.clone()), String::new()));
                        }
                    });
                }
            }
            Row::Heading | Row::Separator => {}
        }
    }

    /// Names a folder made from a row's menu.
    fn picker_name_prompt(&mut self, ui: &mut Ui, p: &mut Picker) {
        let Some((place, mut name)) = p.naming.take() else { return };
        ui.label(RichText::new(crate::locale::tr("New Folder")).color(t::p().text_muted));
        let te = ui.add(TextEdit::singleline(&mut name).hint_text(crate::locale::tr("Folder name")).desired_width(f32::INFINITY));
        if !te.has_focus() && ui.memory(|m| m.focused().is_none()) {
            te.request_focus();
        }
        let enter = te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        ui.add_space(8.0);
        let mut done = cancel;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clean = logic::clean_folder_name(&name);
            if (ui.add_enabled(!clean.is_empty(), egui::Button::new(crate::locale::tr("Create"))).clicked() || (enter && !clean.is_empty())) && !done {
                self.ws.actions.push(Action::Folders(FolderAction::PickerCreate(clean, place.clone(), false)));
                done = true;
            }
            if ui.button(crate::locale::tr("Cancel")).clicked() {
                done = true;
            }
        });
        if !done {
            p.naming = Some((place, name));
        }
    }
}
