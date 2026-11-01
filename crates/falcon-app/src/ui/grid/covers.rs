//! Folder covers and the subfolder cards Eagle shows above a folder's
//! items. A folder's cover is its `coverId` item; without one, Eagle uses
//! the first of the folder's own items in the current sort order (skipping
//! trashed, preview-less and text items and items in locked folders), and
//! a folder with neither borrows the cover of its first subfolder that has
//! one.

use std::collections::HashSet;

use eframe::egui::{self, Align2, CornerRadius, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, vec2};
use falcon_library::Library;
use falcon_library::model::{Folder, Item};
use falcon_library::search::{self, OrderBy};

use crate::app::{Action, App};
use crate::locale::{tr, tr_n, trf};
use crate::ui::widgets::{ext_placeholder, galley};
use crate::loader::{Tier, job_for};
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};

/// Whether an item may stand in as a folder's default cover.
fn can_cover(item: &Item, locked: &HashSet<&str>) -> bool {
    !item.is_deleted
        && item.ext != "txt"
        && item.extra.get("noPreview").and_then(|v| v.as_bool()) != Some(true)
        && !item.folders.iter().any(|f| locked.contains(f.as_str()))
}

/// Ids of password-protected folders.
pub fn locked_folders(roots: &[Folder]) -> HashSet<&str> {
    let mut out = HashSet::new();
    for r in roots {
        r.walk(&mut |f| {
            if f.is_locked() {
                out.insert(f.id.as_str());
            }
        });
    }
    out
}

/// The item shown on `folder`'s card: its `coverId`, else the first usable
/// item in it under `order`, else the cover of its first subfolder that has
/// one.
pub fn cover<'a>(items: &'a [Item], folder: &Folder, order: OrderBy, increase: bool, locked: &HashSet<&str>) -> Option<&'a Item> {
    if let Some(id) = &folder.cover_id
        && let Some(item) = items.iter().find(|i| i.id == *id)
    {
        return Some(item);
    }
    let mut own: Vec<&Item> = items.iter().filter(|i| i.folders.contains(&folder.id) && can_cover(i, locked)).collect();
    if !own.is_empty() {
        search::sort(&mut own, order, increase);
        return own.first().copied();
    }
    folder.children.iter().find_map(|c| cover(items, c, order, increase, locked))
}

const CARD_W: f32 = 150.0;
const THUMB_H: f32 = 100.0;
const STRIP_H: f32 = THUMB_H + 56.0;
const HEADING_H: f32 = 24.0;
/// The List layout's subfolder rows: at least this tall, else a third of
/// the zoom, as Eagle sizes them.
const LIST_ROW_MIN: f32 = 30.0;
/// The modification date column at a subfolder row's right.
const LIST_DATE_W: f32 = 110.0;

/// The height of a subfolder row in the List layout for zoom `tile`.
pub(crate) fn list_row_height(tile: f32) -> f32 {
    (tile / 3.0).max(LIST_ROW_MIN)
}

/// The most the List layout's subfolder rows take of the grid's height
/// before they scroll on their own.
fn list_rows_cap(rows: usize, row_h: f32, avail: f32) -> f32 {
    (rows as f32 * row_h).min((avail * 0.4).max(row_h * 2.0))
}

/// "N items · M subfolders" under a subfolder card, or at its row's right.
fn folder_meta(lib: &Library, child: &Folder) -> String {
    let count = lib.items().iter().filter(|i| !i.is_deleted && i.folders.contains(&child.id)).count();
    let mut meta = tr_n(count, "{n} item", "{n} items");
    if !child.children.is_empty() {
        meta.push_str(&format!(" · {}", tr_n(child.children.len(), "{n} subfolder", "{n} subfolders")));
    }
    meta
}

impl App {
    /// Cards for the open folder's subfolders, above its items, as in
    /// Eagle's folder view, under a heading with the Show Subfolder Content
    /// switch. With that on, every folder under it gets a card. Clicking
    /// the heading folds the cards away (for every folder, as in Eagle).
    /// In the List layout they are rows (a small cover, the name, the counts
    /// and the date), as Eagle lists them there. Takes no space outside a
    /// folder with children.
    pub(crate) fn subfolder_strip(&mut self, ui: &mut Ui, lib: &Library) {
        let Scope::Folder(id) = &self.ws.scope else { return };
        let Some(folder) = logic::find_folder(&lib.metadata.folders, id) else { return };
        if folder.children.is_empty() {
            return;
        }
        let locked = locked_folders(&lib.metadata.folders);
        let mut open = None;
        let width = ui.available_width();
        ui.allocate_ui(vec2(width, HEADING_H), |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(17.0);
                let caret = if self.display.strip_collapsed { crate::icons::CARET_RIGHT } else { crate::icons::CARET_DOWN };
                let heading = egui::RichText::new(format!("{}  {caret}", trf("Subfolders ({count})", &[("count", &folder.children.len())]))).color(t::p().text_muted).size(12.5);
                if ui.add(egui::Label::new(heading).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.display.strip_collapsed = !self.display.strip_collapsed;
                }
                ui.add_space(12.0);
                ui.checkbox(&mut self.display.subfolders, egui::RichText::new(tr("Show subfolder contents")).size(12.5));
            });
        });
        if self.display.strip_collapsed {
            return;
        }
        let cards = logic::subfolder_cards(folder, self.display.subfolders);
        if self.ws.layout == logic::Layout::List {
            let row_h = list_row_height(self.tile);
            let h = list_rows_cap(cards.len(), row_h, ui.available_height());
            ui.allocate_ui(vec2(width, h + 6.0), |ui| {
                egui::ScrollArea::vertical().id_salt(("subfolder-rows", &folder.id)).max_height(h).auto_shrink([false, true]).show(ui, |ui| {
                    for (n, child) in cards.into_iter().enumerate() {
                        let (full, resp) = ui.allocate_exact_size(vec2(width, row_h), Sense::click());
                        let row = Rect::from_min_max(Pos2::new(full.left() + 17.0, full.top()), Pos2::new(full.right() - 17.0, full.bottom() - 1.0));
                        let p = ui.painter().clone();
                        if resp.hovered() || resp.context_menu_opened() {
                            p.rect_filled(row, CornerRadius::same(6), t::p().row_hover);
                        } else if n % 2 == 1 {
                            p.rect_filled(row, CornerRadius::same(6), t::p().row_hover.gamma_multiply(0.3));
                        }
                        let side = row.height() - 6.0;
                        let thumb = Rect::from_min_size(Pos2::new(row.left() + 10.0, row.top() + 3.0), vec2(side, side));
                        p.rect_filled(thumb, CornerRadius::same(4), t::p().field);
                        self.strip_cover(ui, lib, child, thumb, &locked, (side * 0.6).max(12.0));
                        let date = Rect::from_min_max(Pos2::new(row.right() - LIST_DATE_W - 10.0, row.top()), Pos2::new(row.right() - 10.0, row.bottom()));
                        p.text(date.right_center(), Align2::RIGHT_CENTER, logic::format_ms(child.modification_time), font(12.0), t::p().text_muted);
                        let meta = galley(ui, &folder_meta(lib, child), font(12.0), t::p().text_muted, (row.width() * 0.3).max(60.0));
                        let meta_x = date.left() - 10.0 - meta.size().x;
                        p.galley(Pos2::new(meta_x, row.center().y - meta.size().y / 2.0), meta, t::p().text_muted);
                        let name_x = thumb.right() + 10.0;
                        let name_rect = Rect::from_min_max(Pos2::new(name_x, row.center().y - 10.0), Pos2::new((meta_x - 10.0).max(name_x + 40.0), row.center().y + 10.0));
                        self.strip_name(ui, child, name_rect, Pos2::new(name_x, row.center().y - 8.0));
                        if resp.clicked() || resp.double_clicked() {
                            open = Some(child.id.clone());
                        }
                        self.strip_drop_and_menu(ui, lib, child, &resp, row, CornerRadius::same(6));
                    }
                });
            });
        } else {
            ui.allocate_ui(vec2(width, STRIP_H), |ui| {
                egui::ScrollArea::horizontal().id_salt(("subfolders", &folder.id)).auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(17.0);
                        for child in cards {
                            let (rect, resp) = ui.allocate_exact_size(vec2(CARD_W, STRIP_H - 8.0), Sense::click());
                            let thumb = Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(CARD_W, THUMB_H));
                            let p = ui.painter();
                            p.rect_filled(thumb, t::RADIUS, t::p().field);
                            let border = if resp.hovered() || resp.context_menu_opened() { t::p().card_hover } else { t::p().card_border };
                            p.rect_stroke(thumb, t::RADIUS, Stroke::new(1.0, border), StrokeKind::Inside);
                            self.strip_cover(ui, lib, child, thumb, &locked, 30.0);
                            // The name: a double-click renames the folder, as in Eagle.
                            let name_rect = Rect::from_min_size(Pos2::new(rect.left(), thumb.bottom() + 4.0), vec2(CARD_W, 19.0));
                            self.strip_name(ui, child, name_rect, Pos2::new(rect.left(), thumb.bottom() + 6.0));
                            let g = galley(ui, &folder_meta(lib, child), font(11.5), t::p().text_muted, CARD_W);
                            ui.painter().galley(Pos2::new(rect.left(), thumb.bottom() + 25.0), g, t::p().text_muted);
                            if resp.double_clicked() || resp.clicked() {
                                open = Some(child.id.clone());
                            }
                            self.strip_drop_and_menu(ui, lib, child, &resp, thumb, t::RADIUS);
                            ui.add_space(4.0);
                        }
                    });
                });
            });
        }
        if let Some(id) = open {
            self.go(Scope::Folder(id));
        }
    }

    /// A subfolder's cover cropped into `thumb`, or a folder icon of
    /// `icon` points when it is locked or has nothing to show.
    fn strip_cover(&mut self, ui: &Ui, lib: &Library, child: &Folder, thumb: Rect, locked: &HashSet<&str>, icon: f32) {
        let p = ui.painter();
        let item = if child.is_locked() { None } else { cover(lib.items(), child, self.ws.sort, self.ws.increase, locked) };
        let Some(item) = item else {
            p.text(thumb.center(), Align2::CENTER_CENTER, crate::icons::FOLDER, font(icon), t::p().text_disabled);
            return;
        };
        match self.ws.loader.get(&item.id, Tier::Grid).cloned() {
            Some(tex) => {
                let size = tex.size_vec2();
                let s = (thumb.width() / size.x).max(thumb.height() / size.y);
                let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), vec2(thumb.width() / (size.x * s), thumb.height() / (size.y * s)));
                let radius = if thumb.height() < 60.0 { 4 } else { 6 };
                egui::Image::new((tex.id(), thumb.size())).uv(uv).corner_radius(CornerRadius::same(radius)).paint_at(ui, thumb.shrink(1.0));
            }
            None if job_for(item, lib.root(), Tier::Grid).is_none() => ext_placeholder(ui, thumb, &item.ext),
            None => {}
        }
        if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
            self.ws.frame_jobs.push(j);
        }
    }

    /// A subfolder's name at `at`; a double-click on `name_rect` renames
    /// it in place, as in Eagle.
    fn strip_name(&mut self, ui: &mut Ui, child: &Folder, name_rect: Rect, at: Pos2) {
        let renaming = self.ws.strip_rename.as_ref().is_some_and(|(id, _)| *id == child.id);
        if !renaming {
            let g = galley(ui, &child.name, font(13.0), t::p().text, name_rect.width());
            ui.painter().galley(at, g, t::p().text);
            let name_resp = ui.interact(name_rect, ui.scope_id().with(("strip-name", &child.id)), Sense::click());
            if name_resp.double_clicked() {
                self.ws.strip_rename = Some((child.id.clone(), child.name.clone()));
            }
            return;
        }
        ui.painter().rect_filled(name_rect, CornerRadius::same(4), t::p().field);
        ui.painter().rect_stroke(name_rect, CornerRadius::same(4), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        let Some((_, buf)) = self.ws.strip_rename.as_mut() else { return };
        let te = ui.put(name_rect, TextEdit::singleline(buf).id_salt(("strip-rename", &child.id)).frame(egui::Frame::NONE).margin(egui::Margin::symmetric(4, 0)).font(font(13.0)));
        if !te.has_focus() && !te.lost_focus() {
            te.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if (te.lost_focus() || cancel)
            && let Some((id, name)) = self.ws.strip_rename.take()
        {
            let name = name.trim().to_string();
            if !cancel && !name.is_empty() && name != child.name {
                self.ws.actions.push(Action::RenameFolder(id, name));
            }
        }
    }

    /// Items dropped on a subfolder card or row go into it, as on a sidebar
    /// row (`target` lights up while they hover), and its right-click menu.
    fn strip_drop_and_menu(&mut self, ui: &Ui, lib: &Library, child: &Folder, resp: &egui::Response, target: Rect, radius: CornerRadius) {
        if resp.dnd_hover_payload::<crate::app::DragItems>().is_some() {
            ui.painter().rect_filled(target, radius, t::p().primary.gamma_multiply(0.25));
            ui.painter().rect_stroke(target, radius, Stroke::new(1.5, t::p().primary), StrokeKind::Inside);
        }
        if let Some(d) = resp.dnd_release_payload::<crate::app::DragItems>() {
            self.drop_items_on_folder(ui, lib, &d.0, &child.id);
        }
        let shut = crate::organize::is_shut(child, &self.ws.org.unlocked);
        resp.context_menu(|ui| {
            ui.set_min_width(180.0);
            if ui.add_enabled(!shut, egui::Button::new(tr("Rename"))).clicked() {
                self.ws.strip_rename = Some((child.id.clone(), child.name.clone()));
            }
            if ui.add_enabled(!shut, egui::Button::new(tr("Auto-Tags…"))).clicked() {
                self.open_folder_settings(child);
            }
            self.folder_password_menu(ui, child);
            if !shut {
                ui.separator();
                self.tree_export_menu(ui, crate::ui::dialogs::export::Target::Folder(child.id.clone()), true);
                self.add_to_library_menu(ui, crate::ui::dialogs::add_to_library::AddWhat::Folder(child.id.clone()));
            }
            ui.separator();
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Trash);
            let delete = egui::Button::new(egui::RichText::new(tr("Delete Folder")).color(t::p().icon_red)).shortcut_text(keys);
            if ui.add_enabled(!shut, delete).clicked() {
                self.ask_delete_folders(lib, vec![child.id.clone()]);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, folder: &str, time: u64) -> Item {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "size": 1, "btime": time, "mtime": time, "ext": "png", "tags": [], "folders": [folder],
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": time, "height": 10, "width": 10,
            "lastModified": time, "palettes": []
        }))
        .unwrap()
    }

    #[test]
    fn cover_falls_back_like_eagle() {
        let mut parent = Folder::new("P", "Parent", 0);
        let mut child = Folder::new("C", "Child", 0);
        let mut items = vec![item("old", "C", 1), item("new", "C", 5), item("txt", "C", 9)];
        items[2].ext = "txt".into();
        child.cover_id = None;
        parent.children.push(child.clone());
        let none = HashSet::new();
        // Newest import first (Eagle's default order); the text file is skipped.
        assert_eq!(cover(&items, &child, OrderBy::Import, true, &none).map(|i| i.id.as_str()), Some("new"));
        // The parent has no items: it borrows the child's cover.
        assert_eq!(cover(&items, &parent, OrderBy::Import, true, &none).map(|i| i.id.as_str()), Some("new"));
        // An explicit cover wins.
        parent.cover_id = Some("old".into());
        assert_eq!(cover(&items, &parent, OrderBy::Import, true, &none).map(|i| i.id.as_str()), Some("old"));
        // Trashed items and items in locked folders don't stand in.
        items[1].is_deleted = true;
        assert_eq!(cover(&items, &child, OrderBy::Import, true, &none).map(|i| i.id.as_str()), Some("old"));
        let locked: HashSet<&str> = ["C"].into_iter().collect();
        assert_eq!(cover(&items, &child, OrderBy::Import, true, &locked), None);
    }

    #[test]
    fn list_rows_follow_the_zoom_and_stop_growing() {
        assert_eq!(list_row_height(60.0), LIST_ROW_MIN, "never below 30");
        assert_eq!(list_row_height(240.0), 80.0, "a third of the zoom");
        assert_eq!(list_rows_cap(3, 40.0, 1000.0), 120.0, "few rows take their own height");
        assert_eq!(list_rows_cap(50, 40.0, 1000.0), 400.0, "many scroll inside 40% of the grid");
        assert_eq!(list_rows_cap(50, 40.0, 100.0), 80.0, "but always show two rows");
    }
}
