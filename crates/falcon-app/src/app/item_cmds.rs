//! Item commands from Eagle's card menu and Edit and Organize menus that
//! need more than one action: Add to Last Used Folder, Open File
//! Location and the Rename dialog for several items. This file is a child
//! module of `app`.

use eframe::egui::{self, RichText, TextEdit, Ui};
use falcon_library::Library;
use falcon_library::model::Item;
use falcon_library::rename::{Method, Rename, Subject, TextCase};

use super::image_edit::Op;
use super::{Action, App};
use crate::logic::{self, Scope};
use crate::theme as t;

/// From this many items on, renaming asks first (as Eagle does).
const RENAME_ASK: usize = 10;

/// The Rename dialog for several items, as filled in so far.
pub struct ItemRename {
    ids: Vec<String>,
    replace: bool,
    template: String,
    start: u32,
    find: String,
    with: String,
    case: TextCase,
    confirming: bool,
}

impl ItemRename {
    fn rename(&self) -> Rename {
        let method = if self.replace {
            Method::Replace { find: self.find.clone(), with: self.with.clone() }
        } else {
            Method::Format { template: self.template.clone(), start: self.start }
        };
        Rename { method, case: self.case }
    }
}

/// Whether Eagle drops `c` from a templated item name: emoji and the
/// characters joining or varying them.
fn is_emoji(c: char) -> bool {
    matches!(u32::from(c), 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE00..=0xFE0F | 0x200D | 0xE0020..=0xE007F)
}

/// A name made from a template, cleaned as Eagle cleans item names: no
/// `/`, `%` or emoji. An emptied name keeps the original.
pub fn clean_item_name(name: &str, original: &str) -> String {
    let out: String = name.chars().filter(|&c| c != '/' && c != '%' && !is_emoji(c)).collect();
    if out.trim().is_empty() { original.to_string() } else { out }
}

/// The new name of each item (id, old name, new name) under `rename`.
pub fn item_renames(rename: &Rename, items: &[&Item], folder_name: impl Fn(&str) -> Option<String>, now: chrono::DateTime<chrono::Local>) -> Vec<(String, String, String)> {
    items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let folders: Vec<String> = it.folders.iter().filter_map(|f| folder_name(f)).collect();
            // Eagle falls back to the date added for missing file times.
            let pick = |t: u64| if t > 0 { t } else { it.modification_time };
            let s = Subject { name: &it.name, modified: pick(it.mtime), created: pick(it.btime), tags: &it.tags, folders: &folders };
            let mut new = rename.apply(&s, i, now);
            if matches!(rename.method, Method::Format { .. }) {
                new = clean_item_name(&new, &it.name);
            }
            (it.id.clone(), it.name.clone(), new)
        })
        .collect()
}

/// Most folders remembered as recently used (Eagle keeps 50).
pub const RECENT_FOLDERS: usize = 50;

/// Puts `ids` at the front of the recently used folders, the last of them
/// first, without repeats, keeping at most [`RECENT_FOLDERS`].
pub fn note_folders(list: &mut Vec<String>, ids: &[String]) {
    for id in ids {
        list.retain(|x| x != id);
        list.insert(0, id.clone());
    }
    list.truncate(RECENT_FOLDERS);
}

/// The most recently used folder that still exists. Like Eagle, only the
/// first eight remembered are looked at.
pub fn last_used(list: &[String], exists: impl Fn(&str) -> bool) -> Option<String> {
    list.iter().take(8).find(|id| exists(id)).cloned()
}

impl App {
    /// Remembers folders items were just added to.
    pub(super) fn note_folder_use(&mut self, ids: &[String]) {
        note_folders(&mut self.recent_folders, ids);
    }

    /// Eagle's Add to Last Used Folder (⇧D): the items join the folder
    /// they were last added to. On the Uncategorized page they leave the
    /// grid, so the selection moves on.
    pub(super) fn add_to_last_used(&mut self, ids: Vec<String>) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let target = {
            let lib = super::lock(&shared);
            last_used(&self.recent_folders, |id| crate::logic::find_folder(&lib.metadata.folders, id).is_some())
        };
        let Some(fid) = target else {
            self.flash("No folder used yet");
            return;
        };
        if ids.is_empty() {
            return;
        }
        if self.ws.scope == Scope::Uncategorized {
            self.leave_viewed(&ids);
            self.select_after_removal(&ids);
        }
        self.ws.actions.push(Action::AddToFolder(ids, fid));
    }

    /// Eagle's Rename for the selection: one item renames in the
    /// inspector's title field, several open the Rename dialog.
    pub(super) fn rename_items(&mut self, ids: Vec<String>) {
        if ids.len() > 1 {
            self.item_rename = Some(ItemRename {
                ids,
                replace: false,
                template: self.rename_template.clone(),
                start: 1,
                find: String::new(),
                with: String::new(),
                case: TextCase::Keep,
                confirming: false,
            });
        } else if !ids.is_empty() {
            self.show_inspector = true;
            self.ws.focus_title = true;
        }
    }

    /// The Rename dialog for several items: a template with Eagle's tokens
    /// or a find and replace, a case change and a preview.
    pub(super) fn item_rename_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.item_rename.take() else { return };
        let items: Vec<&Item> = d.ids.iter().filter_map(|id| lib.item(id)).collect();
        let rename = d.rename();
        let folder_name = |id: &str| logic::find_folder(&lib.metadata.folders, id).map(|f| f.name.clone());
        let previews = item_renames(&rename, &items, folder_name, chrono::Local::now());
        let mut keep = !previews.is_empty();
        let mut apply = false;
        let modal = egui::Modal::new(egui::Id::unique("item-batch-rename")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(RichText::new(format!("Rename {} Items", previews.len())).size(15.0));
            ui.add_space(6.0);
            if d.confirming {
                ui.label(format!("This renames {} items. Go ahead?", previews.len()));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Rename").clicked() {
                        apply = true;
                    }
                    if ui.button("Back").clicked() {
                        d.confirming = false;
                    }
                });
                return;
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.replace, false, "Format");
                ui.selectable_value(&mut d.replace, true, "Replace Text");
            });
            ui.add_space(4.0);
            if d.replace {
                egui::Grid::new("item-rename-replace").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("Find");
                    ui.add(TextEdit::singleline(&mut d.find).hint_text("Text, or /pattern/flags").desired_width(360.0));
                    ui.end_row();
                    ui.label("Replace with");
                    ui.add(TextEdit::singleline(&mut d.with).desired_width(360.0));
                    ui.end_row();
                });
            } else {
                egui::Grid::new("item-rename-format").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("Name");
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(&mut d.template).desired_width(290.0));
                        ui.menu_button("Insert", |ui| {
                            ui.set_min_width(200.0);
                            let mut pick = |ui: &mut Ui, label: &str, token: &str| {
                                if ui.add(egui::Button::new(label).shortcut_text(token)).clicked() {
                                    d.template.push_str(token);
                                    ui.close();
                                }
                            };
                            ui.menu_button("Number", |ui| {
                                for (label, token) in [("1", "%N"), ("01", "%NN"), ("001", "%NNN"), ("0001", "%NNNN")] {
                                    pick(ui, label, token);
                                }
                            });
                            ui.menu_button("Date", |ui| {
                                for (head, letter) in [("Today", 'D'), ("Modified", 'M'), ("Created", 'B')] {
                                    ui.label(RichText::new(head).color(t::p().text_muted).size(12.0));
                                    for (label, suffix) in [("Date", "1"), ("Month and day", "2"), ("Written out", "3"), ("Date and time", "HM"), ("Date and time with seconds", "HMS")] {
                                        let token = match suffix {
                                            "1" => format!("%{letter}"),
                                            "2" => format!("%{letter}{letter}"),
                                            "3" => format!("%{letter}{letter}{letter}"),
                                            s => format!("%{letter}{s}"),
                                        };
                                        pick(ui, label, &token);
                                    }
                                }
                            });
                            pick(ui, "Tags", "%T");
                            pick(ui, "Folders", "%F");
                            pick(ui, "Original Name", "*");
                        });
                    });
                    ui.end_row();
                    ui.label("Start at");
                    ui.add(egui::DragValue::new(&mut d.start).range(0..=999_999));
                    ui.end_row();
                });
            }
            ui.horizontal(|ui| {
                ui.label("Case");
                egui::ComboBox::from_id_salt("item-rename-case").selected_text(case_label(d.case)).show_ui(ui, |ui| {
                    for c in [TextCase::Keep, TextCase::Upper, TextCase::Lower, TextCase::Capitalize] {
                        ui.selectable_value(&mut d.case, c, case_label(c));
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();
            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("item-rename-preview").num_columns(3).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                    for (_, before, after) in &previews {
                        ui.label(RichText::new(before).color(t::p().text_muted));
                        ui.label("→");
                        ui.label(after);
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = d.rename().is_ready();
                if ui.add_enabled(ready, egui::Button::new("Rename")).clicked() {
                    if previews.len() >= RENAME_ASK {
                        d.confirming = true;
                    } else {
                        apply = true;
                    }
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if apply {
            keep = false;
            if !d.replace {
                self.rename_template.clone_from(&d.template);
            }
            let changed: Vec<(String, String)> = previews.into_iter().filter(|(_, a, b)| a != b).map(|(id, _, b)| (id, b)).collect();
            if !changed.is_empty() {
                self.ws.edit = None;
                self.ws.actions.push(Action::Edit(Op::RenameItems(changed)));
            }
        }
        if keep {
            self.item_rename = Some(d);
        }
    }

    /// Eagle's Open File Location submenu for one item: All, then each
    /// folder the item is in.
    pub(super) fn location_menu(&mut self, ui: &mut Ui, lib: &Library, item: &Item) {
        ui.menu_button("Open File Location", |ui| {
            if ui.button("All").clicked() {
                self.open_item_location(&item.id, None);
            }
            let folders: Vec<(String, String)> =
                item.folders.iter().filter_map(|id| logic::find_folder(&lib.metadata.folders, id).map(|f| (f.id.clone(), f.name.clone()))).collect();
            if !folders.is_empty() {
                ui.separator();
            }
            for (id, name) in folders {
                if ui.button(name).clicked() {
                    self.open_item_location(&item.id, Some(id));
                }
            }
        });
    }

    /// Shows the item in All or in one of its folders, with the search and
    /// filters cleared so it is there, selected and scrolled to.
    pub(super) fn open_item_location(&mut self, id: &str, folder: Option<String>) {
        self.ws.filter = falcon_library::search::Filter::default();
        self.clear_media_filter_text();
        self.ws.color_text.clear();
        self.ws.note_text.clear();
        self.ws.url_text.clear();
        self.ws.query.clear();
        self.ws.viewer = None;
        self.go(folder.map_or(Scope::All, Scope::Folder));
        self.ws.selected = vec![id.to_string()];
        self.ws.anchor = Some(id.to_string());
        self.ws.scroll_to = Some(id.to_string());
    }
}

fn case_label(c: TextCase) -> &'static str {
    match c {
        TextCase::Keep => "As typed",
        TextCase::Upper => "UPPER CASE",
        TextCase::Lower => "lower case",
        TextCase::Capitalize => "Capitalize Words",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    fn item(name: &str, tags: &[&str], folders: &[&str]) -> Item {
        serde_json::from_value(serde_json::json!({"id": name, "name": name, "size": 1, "btime": 0, "mtime": 0, "ext": "png", "tags": tags, "folders": folders, "isDeleted": false, "url": "", "annotation": "", "modificationTime": 1_700_000_000_000u64, "height": 1, "width": 1, "lastModified": 1, "palettes": []})).unwrap()
    }

    #[test]
    fn items_rename_with_tokens_and_lose_slashes_percent_and_emoji() {
        let (a, b) = (item("cat", &["b", "a"], &["F2", "F1"]), item("dog", &[], &[]));
        let names = |id: &str| Some(format!("Folder {id}"));
        let now = chrono::Local::now();
        let fmt = |template: &str| Rename { method: Method::Format { template: template.into(), start: 1 }, case: TextCase::Keep };
        let out = item_renames(&fmt("* %NN %T %F"), &[&a, &b], names, now);
        assert_eq!(out[0], ("cat".into(), "cat".into(), "cat 01 a-b Folder F1-Folder F2".into()));
        assert_eq!(out[1].2, "dog 02  ");
        let out = item_renames(&fmt("a/b 100%🙂 x"), &[&a], names, now);
        assert_eq!(out[0].2, "ab 100 x");
        assert_eq!(item_renames(&fmt("🙂"), &[&a], names, now)[0].2, "cat", "an emptied name keeps the original");
        // Find and replace isn't cleaned (the library sanitises the name).
        let rep = Rename { method: Method::Replace { find: "a".into(), with: "%".into() }, case: TextCase::Upper };
        assert_eq!(item_renames(&rep, &[&a], names, now)[0].2, "C%T");
        // Missing file times fall back to the date added.
        let m = item_renames(&fmt("%M"), &[&a], names, now);
        assert!(m[0].2.starts_with("2023"), "{}", m[0].2);
    }

    #[test]
    fn recent_folders_are_newest_first_without_repeats() {
        let mut list = v(&["a", "b", "c"]);
        note_folders(&mut list, &v(&["c", "d"]));
        assert_eq!(list, v(&["d", "c", "a", "b"]));
        let mut long: Vec<String> = (0..60).map(|i| i.to_string()).collect();
        note_folders(&mut long, &v(&["x"]));
        assert_eq!(long.len(), RECENT_FOLDERS);
        assert_eq!(long[0], "x");
    }

    #[test]
    fn the_last_used_folder_must_still_exist() {
        let list = v(&["gone", "b", "c"]);
        assert_eq!(last_used(&list, |id| id != "gone"), Some("b".into()));
        assert_eq!(last_used(&list, |_| false), None);
        let far: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        assert_eq!(last_used(&far, |id| id == "9"), None, "only the first eight count");
    }
}
