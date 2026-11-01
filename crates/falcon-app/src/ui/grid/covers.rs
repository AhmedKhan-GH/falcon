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

impl App {
    /// Cards for the open folder's subfolders, above its items, as in
    /// Eagle's folder view, under a heading with the Show Subfolder Content
    /// switch. With that on, every folder under it gets a card. Clicking
    /// the heading folds the cards away (for every folder, as in Eagle).
    /// Takes no space outside a folder with children.
    pub(crate) fn subfolder_strip(&mut self, ui: &mut Ui, lib: &Library) {
        let Scope::Folder(id) = &self.ws.scope else { return };
        let Some(folder) = logic::find_folder(&lib.metadata.folders, id) else { return };
        if folder.children.is_empty() || self.ws.layout == logic::Layout::List {
            return;
        }
        let locked = locked_folders(&lib.metadata.folders);
        let mut open = None;
        let width = ui.available_width();
        ui.allocate_ui(vec2(width, HEADING_H), |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(17.0);
                let caret = if self.display.strip_collapsed { crate::icons::CARET_RIGHT } else { crate::icons::CARET_DOWN };
                let heading = egui::RichText::new(format!("Subfolders ({})  {caret}", folder.children.len())).color(t::p().text_muted).size(12.5);
                if ui.add(egui::Label::new(heading).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.display.strip_collapsed = !self.display.strip_collapsed;
                }
                ui.add_space(12.0);
                ui.checkbox(&mut self.display.subfolders, egui::RichText::new("Show subfolder contents").size(12.5));
            });
        });
        if self.display.strip_collapsed {
            return;
        }
        let cards = logic::subfolder_cards(folder, self.display.subfolders);
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
                        if child.is_locked() {
                            p.text(thumb.center(), Align2::CENTER_CENTER, crate::icons::FOLDER, font(30.0), t::p().text_disabled);
                        } else if let Some(item) = cover(lib.items(), child, self.ws.sort, self.ws.increase, &locked) {
                            match self.ws.loader.get(&item.id, Tier::Grid).cloned() {
                                Some(tex) => {
                                    let size = tex.size_vec2();
                                    let s = (thumb.width() / size.x).max(thumb.height() / size.y);
                                    let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), vec2(thumb.width() / (size.x * s), thumb.height() / (size.y * s)));
                                    egui::Image::new((tex.id(), thumb.size())).uv(uv).corner_radius(CornerRadius::same(6)).paint_at(ui, thumb.shrink(1.0));
                                }
                                None if job_for(item, lib.root(), Tier::Grid).is_none() => ext_placeholder(ui, thumb, &item.ext),
                                None => {}
                            }
                            if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                                self.ws.frame_jobs.push(j);
                            }
                        } else {
                            p.text(thumb.center(), Align2::CENTER_CENTER, crate::icons::FOLDER, font(30.0), t::p().text_disabled);
                        }
                        // The name: a double-click renames the folder, as in Eagle.
                        let name_rect = Rect::from_min_size(Pos2::new(rect.left(), thumb.bottom() + 4.0), vec2(CARD_W, 19.0));
                        let renaming = self.ws.strip_rename.as_ref().is_some_and(|(id, _)| *id == child.id);
                        if renaming {
                            ui.painter().rect_filled(name_rect, CornerRadius::same(4), t::p().field);
                            ui.painter().rect_stroke(name_rect, CornerRadius::same(4), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
                            let (_, buf) = self.ws.strip_rename.as_mut().unwrap();
                            let te = ui.put(
                                name_rect,
                                TextEdit::singleline(buf).id_salt(("strip-rename", &child.id)).frame(egui::Frame::NONE).margin(egui::Margin::symmetric(4, 0)).font(font(13.0)),
                            );
                            if !te.has_focus() && !te.lost_focus() {
                                te.request_focus();
                            }
                            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
                            if te.lost_focus() || cancel {
                                let (id, name) = self.ws.strip_rename.take().unwrap();
                                let name = name.trim().to_string();
                                if !cancel && !name.is_empty() && name != child.name {
                                    self.ws.actions.push(Action::RenameFolder(id, name));
                                }
                            }
                        } else {
                            let g = galley(ui, &child.name, font(13.0), t::p().text, CARD_W);
                            ui.painter().galley(Pos2::new(rect.left(), thumb.bottom() + 6.0), g, t::p().text);
                            let name_resp = ui.interact(name_rect, ui.scope_id().with(("strip-name", &child.id)), Sense::click());
                            if name_resp.double_clicked() {
                                self.ws.strip_rename = Some((child.id.clone(), child.name.clone()));
                            }
                        }
                        let count = lib.items().iter().filter(|i| !i.is_deleted && i.folders.contains(&child.id)).count();
                        let mut meta = format!("{count} items");
                        if !child.children.is_empty() {
                            meta.push_str(&format!(" · {} subfolders", child.children.len()));
                        }
                        let g = galley(ui, &meta, font(11.5), t::p().text_muted, CARD_W);
                        ui.painter().galley(Pos2::new(rect.left(), thumb.bottom() + 25.0), g, t::p().text_muted);
                        if resp.double_clicked() || resp.clicked() {
                            open = Some(child.id.clone());
                        }
                        // Items dragged from the grid drop onto the card, as on a sidebar row.
                        if resp.dnd_hover_payload::<crate::app::DragItems>().is_some() {
                            ui.painter().rect_filled(thumb, t::RADIUS, t::p().primary.gamma_multiply(0.25));
                            ui.painter().rect_stroke(thumb, t::RADIUS, Stroke::new(1.5, t::p().primary), StrokeKind::Inside);
                        }
                        if let Some(d) = resp.dnd_release_payload::<crate::app::DragItems>()
                            && let Some(op) = logic::folder_drop(&self.ws.scope, &child.id, ui.input(|i| i.modifiers.alt))
                        {
                            self.push_bulk(d.0.len(), Action::DropOnFolder(d.0.clone(), child.id.clone(), op));
                        }
                        let shut = crate::organize::is_shut(child, &self.ws.org.unlocked);
                        resp.context_menu(|ui| {
                            ui.set_min_width(180.0);
                            if ui.add_enabled(!shut, egui::Button::new("Rename")).clicked() {
                                self.ws.strip_rename = Some((child.id.clone(), child.name.clone()));
                            }
                            if ui.add_enabled(!shut, egui::Button::new("Auto-Tags…")).clicked() {
                                self.open_folder_settings(child);
                            }
                            self.folder_password_menu(ui, child);
                            if !shut {
                                ui.separator();
                                self.tree_export_menu(ui, crate::ui::dialogs::export::Target::Folder(child.id.clone()), true);
                            }
                            ui.separator();
                            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::Trash);
                            let delete = egui::Button::new(egui::RichText::new("Delete Folder").color(t::p().icon_red)).shortcut_text(keys);
                            if ui.add_enabled(!shut, delete).clicked() {
                                self.ask_delete_folders(lib, vec![child.id.clone()]);
                            }
                        });
                        ui.add_space(4.0);
                    }
                });
            });
        });
        if let Some(id) = open {
            self.go(Scope::Folder(id));
        }
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
}
