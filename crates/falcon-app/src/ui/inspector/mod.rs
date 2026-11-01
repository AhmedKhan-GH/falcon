//! The inspector: the open item's or items' properties, tags, folders and comments, or the open folder's.

use std::collections::HashSet;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, Ui, Vec2, vec2,
};
use falcon_library::model::Item;
use falcon_library::search::ColorQuery;
use falcon_library::{Library, thumbnail};

use crate::app::{Action, App, EditBuf, Fold, MultiEditBuf};
use crate::colors;
use crate::icons as ic;
use crate::loader::{Tier, job_for};
use crate::toast::{Kind as ToastKind, Life, Toast};
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::widgets::{
    ChipClick, divider, ext_placeholder, field, folder_chip, icon, icon_button, prop,
    rating, tag_chip_removable, wide_button, window_drag,
};

pub(crate) mod tag_picker;

/// The saved name and description of a folder, or of a smart folder.
pub(crate) fn folder_texts(lib: &Library, id: &str, smart: bool) -> Option<(String, String)> {
    if !smart {
        return logic::find_folder(&lib.metadata.folders, id).map(|f| (f.name.clone(), f.description.clone()));
    }
    fn find(list: &[falcon_library::smart::SmartFolder], id: &str) -> Option<(String, String)> {
        list.iter().find_map(|f| if f.id == id { Some((f.name.clone(), f.description.clone())) } else { find(&f.children, id) })
    }
    find(&lib.smart_folders(), id)
}

/// The action that renames a folder or a smart folder from the inspector.
pub(crate) fn rename_action(id: &str, name: &str, smart: bool) -> Action {
    if smart { Action::Organize(crate::app::organize_ui::OrgAction::RenameSmart(id.to_string(), name.to_string())) } else { Action::RenameFolder(id.to_string(), name.to_string()) }
}

impl App {
    pub(crate) fn inspector(&mut self, ui: &mut Ui, lib: &mut Library) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::p().panel);
        p.vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0, t::p().divider));
        let header = Rect::from_min_size(full.min, vec2(full.width(), t::BAR_HEIGHT));
        window_drag(ui, header, "inspector");
        if icon_button(ui, Pos2::new(full.right() - 24.0, header.center().y), ic::PUSH_PIN, 17.0, true, "Hide inspector")
            .clicked()
        {
            self.show_inspector = false;
        }
        let body = Rect::from_min_max(Pos2::new(full.left() + 16.0, header.bottom() + 8.0), Pos2::new(full.right() - 16.0, full.bottom()));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(&mut ui, |ui| {
            ui.spacing_mut().item_spacing.y = 9.0;
            if !self.ws.selected.is_empty() {
                self.flush_folder_edit(lib, None);
            }
            match self.ws.selected.len() {
                0 => self.scope_inspector(ui, lib),
                1 => {
                    let id = self.ws.selected[0].clone();
                    if let Some(item) = lib.item(&id).cloned() {
                        self.item_inspector(ui, lib, item);
                    }
                }
                n => self.multi_inspector(ui, lib, n),
            }
        });
    }

    /// A section title that folds the section away when clicked, with an
    /// arrow that points right when it is folded. True while it is shown.
    /// The state is shared by the three inspectors and every library.
    pub(crate) fn fold_heading(&mut self, ui: &mut Ui, which: Fold, text: &str) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click());
        let closed = self.folds.closed(which);
        if resp.clicked() {
            *closed = !*closed;
        }
        let color = if resp.hovered() { t::p().text } else { t::p().text_muted };
        icon(ui, Pos2::new(rect.left() + 6.0, rect.center().y), if *closed { ic::CARET_RIGHT } else { ic::CARET_DOWN }, 10.0, color);
        ui.painter().text(rect.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, text, font(13.0), color);
        !*closed
    }

    /// Saves text typed into the folder inspector when its fields went away
    /// with focus still in them (the scope or the selection changed), unless
    /// they are still shown for folder `shown`.
    pub(crate) fn flush_folder_edit(&mut self, lib: &Library, shown: Option<&str>) {
        let Some((id, name, desc, smart)) = self.ws.folder_edit.take_if(|(id, ..)| shown != Some(id.as_str())) else { return };
        let Some((old_name, old_desc)) = folder_texts(lib, &id, smart) else { return };
        if !name.trim().is_empty() && name.trim() != old_name {
            self.ws.actions.push(rename_action(&id, name.trim(), smart));
        }
        if desc != old_desc {
            self.ws.actions.push(if smart { Action::SetSmartDescription(id, desc) } else { Action::SetFolderDescription(id, desc) });
        }
    }

    pub(crate) fn scope_inspector(&mut self, ui: &mut Ui, lib: &Library) {
        let folder = match &self.ws.scope {
            Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id).cloned(),
            _ => None,
        };
        let smart_id = match &self.ws.scope {
            Scope::Smart(id) => Some(id.clone()),
            _ => None,
        };
        // The smart folder open, as (id, name, description), if it exists.
        let smart = smart_id.and_then(|id| folder_texts(lib, &id, true).map(|(n, d)| (id, n, d)));
        let shown = folder.as_ref().map(|f| (f.id.clone(), f.name.clone(), f.description.clone(), false)).or_else(|| smart.map(|(i, n, d)| (i, n, d, true)));
        self.flush_folder_edit(lib, shown.as_ref().map(|s| s.0.as_str()));
        if let Some((id, saved_name, saved_desc, is_smart)) = &shown {
            // A folder's name and description save when the field loses
            // focus (Return does that for the name), as one undo step each.
            // An emptied name goes back to the saved one, as in Eagle. A
            // smart folder's are edited the same way.
            if self.ws.folder_edit.is_none() {
                self.ws.folder_edit = Some((id.clone(), saved_name.clone(), saved_desc.clone(), *is_smart));
            }
            let (_, name, desc, _) = self.ws.folder_edit.as_mut().expect("set above");
            let r = field(ui, name, "Name", 33.0, false, "scope-name");
            if r.lost_focus() {
                let typed = name.trim();
                if !typed.is_empty() && typed != saved_name {
                    self.ws.actions.push(rename_action(id, typed, *is_smart));
                }
            } else if !r.has_focus() {
                // Follow renames made elsewhere, and undo.
                name.clone_from(saved_name);
            }
            let r = field(ui, desc, "Description", 54.0, true, "scope-desc");
            if r.lost_focus() {
                if desc != saved_desc {
                    let (id, text) = (id.clone(), desc.clone());
                    self.ws.actions.push(if *is_smart { Action::SetSmartDescription(id, text) } else { Action::SetFolderDescription(id, text) });
                }
            } else if !r.has_focus() {
                desc.clone_from(saved_desc);
            }
        } else {
            // Other scopes have fixed names.
            let mut name = self.ws.scope.title(&lib.metadata.folders);
            ui.add_enabled_ui(false, |ui| field(ui, &mut name, "Name", 33.0, false, "scope-name"));
        }
        divider(ui);
        let items: Vec<&Item> = self.ws.order.iter().filter_map(|id| lib.item(id)).collect();
        let size: u64 = items.iter().map(|i| i.size).sum();
        if self.fold_heading(ui, Fold::Properties, "Properties") {
            prop(ui, "Items", &items.len().to_string());
            prop(ui, "Size", &logic::format_size(size));
            if let Some(f) = &folder {
                prop(ui, "Date Imported", &logic::format_ms(f.modification_time));
                prop(ui, "Password", if f.password.is_empty() { "None" } else { "Set" });
            }
        }
        ui.add_space(4.0);
        self.scope_export_button(ui, lib);
    }

    pub(crate) fn multi_inspector(&mut self, ui: &mut Ui, lib: &Library, n: usize) {
        let items: Vec<&Item> = self.ws.selected.iter().filter_map(|id| lib.item(id)).collect();
        let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        let size: u64 = items.iter().map(|i| i.size).sum();

        // A fanned stack of the first three previews, like Eagle's.
        let w = ui.available_width();
        let (stack, _) = ui.allocate_exact_size(vec2(w, 150.0), Sense::hover());
        for (k, item) in items.iter().take(3).enumerate().rev() {
            let off = (k as f32 - 1.0) * 14.0;
            let r = Rect::from_center_size(stack.center() + vec2(off, -off * 0.4), Vec2::splat(112.0));
            ui.painter().rect_filled(r, CornerRadius::same(6), t::p().field);
            ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
            if let Some(tex) = self.ws.loader.get(&item.id, Tier::Grid).cloned() {
                let ts = tex.size_vec2();
                let sc = ((r.width() - 10.0) / ts.x).min((r.height() - 10.0) / ts.y);
                let ir = Rect::from_center_size(r.center(), ts * sc);
                egui::Image::new((tex.id(), ir.size())).corner_radius(4).paint_at(ui, ir);
            }
            if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                self.ws.frame_jobs.push(j);
            }
        }
        ui.vertical_centered(|ui| ui.label(RichText::new(format!("{n} items selected")).size(15.0)));

        // Eagle's multi-selection inspector has no name field, but the note
        // and URL apply to every item: each shows the value they share, or
        // nothing when they differ.
        let shown = (
            logic::shared_text(items.iter().map(|i| i.annotation.as_str())).to_string(),
            logic::shared_text(items.iter().map(|i| i.url.as_str())).to_string(),
        );
        if self.ws.multi_edit.as_ref().is_none_or(|m| m.ids != ids || (m.shown != shown && !ui.ctx().egui_wants_keyboard_input())) {
            self.ws.multi_edit = Some(MultiEditBuf { ids: ids.clone(), notes: shown.0.clone(), url: shown.1.clone(), shown: shown.clone() });
        }
        let m = self.ws.multi_edit.as_mut().unwrap();
        let mut edits = Vec::new();
        let notes_hint = if shown.0.is_empty() && items.iter().any(|i| !i.annotation.is_empty()) { "Multiple notes" } else { "Notes..." };
        let r = field(ui, &mut m.notes, notes_hint, 54.0, true, "multi-notes");
        if r.lost_focus() && m.notes != m.shown.0 {
            let action = Action::SetNotes(ids.clone(), m.notes.clone());
            m.shown.0 = m.notes.clone();
            edits.push(action);
        }
        let url_hint = if shown.1.is_empty() && items.iter().any(|i| !i.url.is_empty()) { "Multiple URLs" } else { "http://" };
        let r = field(ui, &mut m.url, url_hint, 33.0, false, "multi-url");
        if r.lost_focus() && m.url.trim() != m.shown.1 {
            m.url = m.url.trim().to_string();
            let action = Action::SetUrls(ids.clone(), m.url.clone());
            m.shown.1 = m.url.clone();
            edits.push(action);
        }
        for action in edits {
            self.push_bulk(ids.len(), action);
        }

        // Tags every selected item shares; the picker adds to all.
        if self.fold_heading(ui, Fold::Tags, "Tags") {
            let common = crate::tags::common_tags(&items);
            self.tag_field(ui, lib, &common, &ids);
        }

        if self.fold_heading(ui, Fold::Folders, "Folders") {
            // A chip for each folder all of them are in; the × takes all of
            // them out, as for one item.
            let mut remove_folder = None;
            let mut open_folder = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for fid in logic::shared_folders(&items) {
                    let Some(f) = logic::find_folder(&lib.metadata.folders, &fid) else { continue };
                    let path = logic::folder_path(&lib.metadata.folders, &fid).unwrap_or_else(|| f.name.clone());
                    match folder_chip(ui, &f.name, &path) {
                        Some(ChipClick::Remove) => remove_folder = Some(fid),
                        Some(ChipClick::Open) => open_folder = Some(fid),
                        None => {}
                    }
                }
                let add = ui.add(egui::Button::new(RichText::new(format!("{}  Add All to Folder", ic::PLUS)).size(12.0).color(t::p().text_muted)).frame(false));
                egui::Popup::menu(&add).show(|ui| {
                    ui.set_min_width(200.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for f in lib.all_folders() {
                            if ui.button(&f.name).clicked() {
                                self.push_bulk(ids.len(), Action::AddToFolder(ids.clone(), f.id.clone()));
                            }
                        }
                    });
                });
            });
            if let Some(fid) = remove_folder {
                self.ws.actions.push(Action::RemoveFromFolder(ids.clone(), fid));
            }
            if let Some(fid) = open_folder {
                self.go(Scope::Folder(fid));
            }
        }

        divider(ui);
        if self.fold_heading(ui, Fold::Properties, "Properties") {
            let stars: HashSet<u64> = items.iter().map(|i| i.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0)).collect();
            let shown = if stars.len() == 1 { stars.into_iter().next().unwrap_or(0) } else { 0 };
            if let Some(star) = rating(ui, shown) {
                self.push_bulk(ids.len(), Action::BatchRating(ids.clone(), star));
            }
            prop(ui, "Items", &n.to_string());
            prop(ui, "Size", &logic::format_size(size));
        }
        ui.add_space(4.0);
        let trash = self.ws.scope == Scope::Trash;
        let label = if trash { "Restore All" } else { "Move All to Trash" };
        if wide_button(ui, ic::TRASH, label, true).clicked() {
            let n = ids.len();
            if trash {
                self.ws.actions.push(Action::Trash(ids, false));
            } else {
                self.push_bulk(n, Action::TrashAsk(ids));
            }
        }
    }

    /// Copy Tags: keeps `tags` as the tag clipboard and puts them on the
    /// system clipboard one per line, as Eagle does. Nothing to copy keeps
    /// the old clipboard.
    pub(crate) fn copy_tags(&mut self, ctx: &egui::Context, tags: Vec<String>) {
        if tags.is_empty() {
            self.flash_brief("No tags to copy");
            return;
        }
        ctx.copy_text(tags.join("\n"));
        self.flash_brief(format!("Copied {}", crate::app::export_ui::plural(tags.len(), "tag")));
        self.copied_tags = Some(tags);
    }

    /// Paste Tags: the copied tags go onto `ids`, after their own.
    pub(crate) fn paste_tags(&mut self, ids: Vec<String>) {
        if let Some(tags) = self.copied_tags.clone()
            && !ids.is_empty()
        {
            self.ws.actions.push(Action::PasteTags(ids, tags));
        }
    }

    /// The menu of the inspector's tag area (Eagle's tag input menu):
    /// copy, paste and clear the tags of `ids`, or for one chip `tag`,
    /// show its items, copy just it, or paste.
    pub(crate) fn tag_area_menu(&mut self, ui: &mut Ui, tags: &[String], ids: &[String], tag: Option<&str>) {
        use crate::shortcuts::Command as C;
        ui.set_min_width(190.0);
        if let Some(tag) = tag {
            if ui.button("Show Items with This Tag").clicked() {
                self.go(Scope::Tag(tag.to_string()));
            }
            if ui.button("Copy Tag").clicked() {
                let ctx = ui.ctx().clone();
                self.copy_tags(&ctx, vec![tag.to_string()]);
            }
        } else {
            let keys = self.shortcut_text(ui.ctx(), C::CopyTags);
            if ui.add_enabled(!tags.is_empty(), egui::Button::new("Copy Tags").shortcut_text(keys)).clicked() {
                let ctx = ui.ctx().clone();
                self.copy_tags(&ctx, tags.to_vec());
            }
        }
        let keys = self.shortcut_text(ui.ctx(), C::PasteTags);
        if ui.add_enabled(self.copied_tags.is_some(), egui::Button::new("Paste Tags").shortcut_text(keys)).clicked() {
            self.paste_tags(ids.to_vec());
        }
        if tag.is_none() {
            ui.separator();
            let keys = self.shortcut_text(ui.ctx(), C::ClearTags);
            if ui.add_enabled(!tags.is_empty(), egui::Button::new("Clear Tags").shortcut_text(keys)).clicked() {
                self.ws.actions.push(Action::ClearTags(ids.to_vec()));
            }
        }
    }

    /// The inspector's tags: a chip per tag (its × takes the tag off `ids`)
    /// and the button that opens the tag picker, as in Eagle.
    pub(crate) fn tag_field(&mut self, ui: &mut Ui, lib: &Library, tags: &[String], ids: &[String]) {
        let mut remove = None;
        let mut open = None;
        // The area senses clicks behind its chips: right-clicking between
        // them offers copy, paste and clear.
        let area = ui.scope_builder(egui::UiBuilder::new().sense(Sense::click()), |ui| ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for tag in tags {
                let chip = tag_chip_removable(ui, tag, App::tag_dot(lib, tag));
                if chip.clicked() {
                    remove = Some(tag.clone());
                }
                chip.context_menu(|ui| self.tag_area_menu(ui, tags, ids, Some(tag)));
            }
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::TagPanel);
            let label = if tags.is_empty() { format!("{}  Add Tags", ic::PLUS) } else { ic::PLUS.to_string() };
            let b = ui.add(egui::Button::new(RichText::new(label).size(12.0).color(t::p().text_muted)).frame(false));
            let b = b.on_hover_text(if keys.is_empty() { "Add tags".to_string() } else { format!("Add tags  ({keys})") });
            if b.clicked() {
                open = Some(b.rect.left_center());
            }
        }));
        area.response.context_menu(|ui| self.tag_area_menu(ui, tags, ids, None));
        if let Some(tag) = remove {
            self.ws.actions.push(Action::RemoveTags(ids.to_vec(), vec![tag]));
        }
        if let Some(at) = open {
            let ctx = ui.ctx().clone();
            self.open_tag_picker(&ctx, Some(at));
        }
    }

    pub(crate) fn item_inspector(&mut self, ui: &mut Ui, lib: &mut Library, item: Item) {
        if self.ws.edit.as_ref().is_none_or(|e| e.id != item.id) {
            self.ws.edit = Some(EditBuf {
                id: item.id.clone(),
                name: item.name.clone(),
                notes: item.annotation.clone(),
                url: item.url.clone(),
            });
        }
        let w = ui.available_width();

        // Preview.
        let aspect = if item.width > 0 && item.height > 0 { item.height as f32 / item.width as f32 } else { 1.0 };
        let ph = (w * aspect).clamp(80.0, 260.0);
        let (prect, presp) = ui.allocate_exact_size(vec2(w, ph), Sense::click());
        if let Some(tex) = self.ws.loader.get(&item.id, Tier::Preview) {
            let ts = if item.width > 0 { vec2(item.width as f32, item.height as f32) } else { tex.size_vec2() };
            let scale = (prect.width() / ts.x).min(prect.height() / ts.y).min(1.0);
            let r = Rect::from_center_size(prect.center(), ts * scale);
            egui::Image::new((tex.id(), r.size())).corner_radius(4).paint_at(ui, r);
        } else if !thumbnail::is_supported(&item.ext) && item.thumbnail_path(lib.root()).is_none() {
            ext_placeholder(ui, prect, &item.ext);
        }
        if presp.double_clicked() {
            self.ws.viewer = Some(item.id.clone());
        }
        if let Some(j) = job_for(&item, lib.root(), Tier::Preview) {
            self.ws.frame_jobs.push(j);
        }

        // Palette.
        if !item.palettes.is_empty() {
            let d = 16.0;
            let (row, _) = ui.allocate_exact_size(vec2(w, d), Sense::hover());
            let mut toggle_primary: Option<(usize, bool)> = None;
            for (n, c) in item.palettes.iter().enumerate() {
                let center = Pos2::new(row.left() + d / 2.0 + n as f32 * (d + 6.0), row.center().y);
                if center.x + d / 2.0 > row.right() {
                    break;
                }
                let color = Color32::from_rgb(c.color[0], c.color[1], c.color[2]);
                ui.painter().circle_filled(center, d / 2.0, color);
                let marked = c.extra.get(falcon_library::item_ops::MARKED_KEY).and_then(|v| v.as_bool()).unwrap_or(false);
                if marked {
                    ui.painter().circle_stroke(center, d / 2.0 + 1.0, Stroke::new(2.0, t::p().primary));
                } else {
                    ui.painter().circle_stroke(center, d / 2.0, Stroke::new(1.0, t::p().divider));
                }
                let hit = ui.interact(Rect::from_center_size(center, Vec2::splat(d)), ui.scope_id().with(("swatch", n)), Sense::click());
                let hex = logic::hex(c.color);
                if hit.clicked() {
                    self.ws.filter.color = Some(ColorQuery::new(c.color));
                    self.ws.color_text = hex.clone();
                    self.ws.show_filter = true;
                }
                let mut similar = false;
                hit.context_menu(|ui| {
                    if ui.button("Find Similar Colours").clicked() {
                        similar = true;
                        ui.close();
                    }
                    ui.separator();
                    let values = [
                        colors::hex(c.color),
                        colors::rgb(c.color),
                        colors::rgba(c.color),
                        colors::hsl(c.color),
                        colors::hsv(c.color),
                        colors::hwb(c.color),
                        colors::cmyk(c.color),
                    ];
                    for (i, v) in values.iter().enumerate() {
                        if i == 4 {
                            ui.separator();
                        }
                        if ui.button(format!("Copy {v}")).clicked() {
                            ui.ctx().copy_text(v.clone());
                            self.flash_brief(format!("Copied {v}"));
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.checkbox(&mut { marked }, "Primary Colour").clicked() {
                        toggle_primary = Some((n, !marked));
                        ui.close();
                    }
                });
                if similar {
                    self.ws.filter.color = Some(ColorQuery::new(c.color));
                    self.ws.color_text = hex.clone();
                    self.ws.show_filter = true;
                }
                hit.on_hover_text(format!("{hex}  ·  {}%\nClick to find similar colours, right-click for more", c.ratio));
            }
            if let Some((n, on)) = toggle_primary
                && let Err(e) = lib.set_palette_marked(&item.id, n, on)
            {
                self.flash_error(format!("Could not save: {e}"));
            }
        }

        let mut changed: Option<Item> = None;
        let link_keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::OpenLink);
        let edit = self.ws.edit.as_mut().unwrap();
        let r = field(ui, &mut edit.name, "Title", 33.0, false, "item-name");
        if std::mem::take(&mut self.ws.focus_title) {
            r.request_focus();
        }
        if r.lost_focus() && edit.name.trim() != item.name && !edit.name.trim().is_empty() {
            let mut it = item.clone();
            it.name = edit.name.trim().to_string();
            changed = Some(it);
        }
        let r = field(ui, &mut edit.notes, "Notes...", 54.0, true, "item-notes");
        if r.lost_focus() && edit.notes != item.annotation {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.annotation = edit.notes.clone();
            changed = Some(it);
        }
        let r = field(ui, &mut edit.url, "http://", 33.0, false, "item-url");
        let mut open_link = false;
        if !item.url.is_empty() || !edit.url.is_empty() {
            let at = Pos2::new(r.rect.right() - 18.0, r.rect.center().y);
            let tip = format!("Open Link ({link_keys})");
            open_link = icon_button(ui, at, ic::ARROW_SQUARE_OUT, 16.0, true, &tip).clicked();
        }
        if r.lost_focus() && edit.url != item.url {
            if crate::urls::url_accepted(edit.url.trim(), |p| std::path::Path::new(p).exists()) {
                let mut it = changed.clone().unwrap_or_else(|| item.clone());
                it.url = edit.url.trim().to_string();
                changed = Some(it);
            } else {
                edit.url = item.url.clone();
                self.ws.status = Some(Toast::new("Not a valid link", ToastKind::Info, Life::Normal));
            }
        }
        if open_link && let Some(t) = crate::urls::link_target(&item.url, |p| std::path::Path::new(p).exists()) {
            crate::urls::open_link(&t);
        }

        // Tags.
        if self.fold_heading(ui, Fold::Tags, "Tags") {
            self.tag_field(ui, lib, &item.tags, std::slice::from_ref(&item.id));
        }

        // Folders.
        let folders_open = self.fold_heading(ui, Fold::Folders, "Folders");
        let mut remove_folder = None;
        let mut open_folder = None;
        let mut add_folder = None;
        if folders_open {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for fid in &item.folders {
                    let name = logic::find_folder(&lib.metadata.folders, fid).map_or_else(|| fid.clone(), |f| f.name.clone());
                    let path = logic::folder_path(&lib.metadata.folders, fid).unwrap_or_else(|| name.clone());
                    match folder_chip(ui, &name, &path) {
                        Some(ChipClick::Remove) => remove_folder = Some(fid.clone()),
                        Some(ChipClick::Open) => open_folder = Some(fid.clone()),
                        None => {}
                    }
                }
                let add = ui.add(egui::Button::new(RichText::new(format!("{}  Add Category", ic::PLUS)).size(12.0).color(t::p().text_muted)).frame(false));
                egui::Popup::menu(&add).show(|ui| {
                    ui.set_min_width(200.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for f in lib.all_folders() {
                            if !item.folders.contains(&f.id) && ui.button(&f.name).clicked() {
                                add_folder = Some(f.id.clone());
                            }
                        }
                    });
                });
            });
        }
        if let Some(fid) = remove_folder {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.folders.retain(|x| *x != fid);
            changed = Some(it);
        }
        // Eagle's quick open: show the folder with this item still selected.
        if let Some(fid) = open_folder
            && logic::find_folder(&lib.metadata.folders, &fid).is_some()
        {
            self.go(Scope::Folder(fid));
            self.ws.selected = vec![item.id.clone()];
            self.ws.anchor = Some(item.id.clone());
            self.ws.scroll_to = Some(item.id.clone());
        }
        if let Some(fid) = add_folder {
            self.note_folder_use(std::slice::from_ref(&fid));
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            // Eagle's folder picker in the inspector adds auto-tags too.
            if falcon_library::autotag::join_folder(&mut it, &lib.metadata.folders, &fid) {
                changed = Some(it);
            }
        }

        self.comment_list(ui, &item);
        divider(ui);
        if self.fold_heading(ui, Fold::Properties, "Properties") {
            let star = item.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(new_star) = rating(ui, star) {
                let mut it = changed.clone().unwrap_or_else(|| item.clone());
                if new_star == 0 {
                    it.extra.remove("star");
                } else {
                    it.extra.insert("star".into(), new_star.into());
                }
                changed = Some(it);
            }
            if item.width > 0 {
                prop(ui, "Dimensions", &format!("{} × {}", item.width, item.height));
            }
            prop(ui, "Size", &logic::format_size(item.size));
            let type_row = prop(ui, "Type", &item.ext.to_uppercase());
            let type_hit = ui.interact(type_row.rect, ui.scope_id().with("type-row"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Click to change the extension");
            if type_hit.clicked() {
                self.ws.ext_dialog = Some(crate::app::ext_ui::ExtDialog { id: item.id.clone(), current: item.ext.to_lowercase(), text: item.ext.clone(), uid: egui::Id::unique("change-extension"), focused: false });
            }
            prop(ui, "Date Imported", &logic::format_ms(item.modification_time));
            prop(ui, "Date Created", &logic::format_ms(item.btime));
            prop(ui, "Date Modified", &logic::format_ms(item.mtime));
            crate::app::media_view::media_props(ui, &item);
        }
        ui.add_space(4.0);
        if wide_button(ui, ic::ARROW_SQUARE_OUT, "Open in Default App", true).clicked() {
            let files = [item.original_path(lib.root())];
            crate::open_with::open_default(&files);
            self.used(&files);
        }
        ui.add_space(4.0);
        if wide_button(ui, ic::EXPORT, "Show in Finder", true).clicked() {
            let _ = std::process::Command::new("open").arg("-R").arg(item.original_path(lib.root())).spawn();
        }

        if let Some(it) = changed {
            match lib.update_item(it) {
                Ok(()) => {
                    if let Some(e) = self.ws.edit.as_mut() {
                        e.name = lib.item(&item.id).map_or(e.name.clone(), |i| i.name.clone());
                    }
                }
                Err(e) => self.flash_error(format!("Could not save: {e}")),
            }
        }
    }
}
