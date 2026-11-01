//! Several folders or smart folders selected in the sidebar: the ⌘ and ⇧
//! clicks that pick them, the combined view they open, their context menu
//! and its dialogs (batch rename, Move Folder, delete). The selection rules
//! live in `logic::TreeSel`, the edits in `falcon_library::edit`.

use std::collections::HashSet;

use eframe::egui::{self, Key, Modifiers, Pos2, Rect, RichText, Sense, TextEdit, Ui, vec2};
use falcon_library::model::Folder;
use falcon_library::Library;

use crate::app::{Action, App, Confirm};
use crate::icons as ic;
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley, icon};
use crate::ui::organize::{color_row, icon_grid, sort_entries};
use crate::folder_icons;
use crate::logic::TreeSel;
use falcon_library::edit::FolderPlace;
use falcon_library::model::FolderOrder;
use falcon_library::rename::{Method, Rename, Subject, TextCase};

/// Folders and smart folders picked in the sidebar, and the rows each
/// tree showed, top to bottom, for ⇧-click ranges.
#[derive(Default)]
pub(crate) struct FolderUi {
    pub sel: TreeSel,
    pub smart: TreeSel,
    /// Rows drawn in the last complete frame, and those drawn so far in
    /// this one.
    rows: Vec<String>,
    rows_next: Vec<String>,
    smart_rows: Vec<String>,
    smart_rows_next: Vec<String>,
    /// The batch rename and Move Folder dialogs, when open.
    rename: Option<RenameDraft>,
    mover: Option<MoveDraft>,
    /// The delete confirmation's "trash the items too" box.
    pub trash_items: bool,
    /// New Folder with Selection: the items, and the name typed so far.
    pub new_with: Option<(Vec<String>, String)>,
    /// The Add to Folders picker (`ui/dialogs/add_to_folders.rs`), and its
    /// "take them out of the open folder" box, kept between openings.
    pub picker: Option<crate::folder_picker::Picker>,
    pub picker_remove: bool,
    /// The open folder's password state: `None` without one, else whether
    /// it is shut (locked and not unlocked). Kept for the menu bar.
    pub open_lock: Option<bool>,
}

/// The batch rename dialog as filled in so far.
struct RenameDraft {
    tree: Tree,
    ids: Vec<String>,
    replace: bool,
    template: String,
    start: u32,
    find: String,
    with: String,
    case: TextCase,
    /// Asking "rename this many?" before a large batch, as Eagle does.
    confirming: bool,
}

impl RenameDraft {
    fn rename(&self) -> Rename {
        let method = if self.replace {
            Method::Replace { find: self.find.clone(), with: self.with.clone() }
        } else {
            Method::Format { template: self.template.clone(), start: self.start }
        };
        Rename { method, case: self.case }
    }
}

/// From this many on, a batch rename asks first (as Eagle does).
const RENAME_ASK: usize = 10;

/// The Move Folder dialog: the folders moving, the search text, the rows
/// collapsed in its tree, and the destination waiting to be confirmed.
struct MoveDraft {
    ids: Vec<String>,
    search: String,
    collapsed: HashSet<String>,
    ask: Option<(String, FolderPlace)>,
}

/// Library changes queued by the commands for several folders, and by the
/// Move Folder and delete dialogs.
pub(crate) enum FolderAction {
    Rename(Vec<(String, String)>),
    RenameSmart(Vec<(String, String)>),
    Move(Vec<String>, String, FolderPlace),
    /// New icon and/or colour for every folder (`None` keeps each one's).
    Restyle(Vec<String>, Option<Option<String>>, Option<Option<String>>),
    RestyleSmart(Vec<String>, Option<Option<String>>, Option<Option<String>>),
    /// A shared item sort; each folder keeps its direction.
    Order(Vec<String>, Option<FolderOrder>),
    Increase(Vec<String>, bool),
    SmartOrder(Vec<String>, Option<FolderOrder>),
    SmartIncrease(Vec<String>, bool),
    /// Folders to delete, and whether items left in no folder go to the
    /// Trash.
    Delete(Vec<String>, bool),
    DeleteSmart(Vec<String>),
    QuickAdd(&'static str, Vec<String>),
    QuickRemove(Vec<String>),
    Duplicate(String),
    DuplicateSmart(String),
    /// A new top-level folder with this name, holding these items.
    CreateWith(String, Vec<String>),
    /// The Add to Folders picker's save: these items join the first
    /// folders and leave the second.
    Categorize(Vec<String>, Vec<String>, Vec<String>),
    /// A folder made in the picker, and whether it starts ticked.
    PickerCreate(String, crate::folder_picker::NewPlace, bool),
}

impl FolderAction {
    /// The undo name.
    pub fn label(&self) -> String {
        let n = |v: &Vec<String>| if v.len() == 1 { "folder".to_string() } else { format!("{} folders", v.len()) };
        match self {
            FolderAction::Rename(v) | FolderAction::RenameSmart(v) => format!("Rename {} folders", v.len()),
            FolderAction::Move(v, ..) => format!("Move {}", n(v)),
            FolderAction::Restyle(v, ..) | FolderAction::RestyleSmart(v, ..) => format!("Change icon of {}", n(v)),
            FolderAction::Order(v, _) | FolderAction::Increase(v, _) | FolderAction::SmartOrder(v, _) | FolderAction::SmartIncrease(v, _) => {
                format!("Change sort of {}", n(v))
            }
            FolderAction::Delete(v, _) => format!("Delete {}", n(v)),
            FolderAction::DeleteSmart(v) => format!("Delete {}", n(v).replace("folder", "smart folder")),
            FolderAction::QuickAdd(_, v) => format!("Add {} to Quick Access", n(v)),
            FolderAction::QuickRemove(v) => format!("Remove {} from Quick Access", n(v)),
            FolderAction::Duplicate(_) => "Duplicate folder".into(),
            FolderAction::DuplicateSmart(_) => "Duplicate smart folder".into(),
            FolderAction::CreateWith(_, items) => format!("New folder with {} items", items.len()),
            FolderAction::Categorize(items, ..) => format!("Add {} to folders", items.len()),
            FolderAction::PickerCreate(..) => "New folder".into(),
        }
    }
}

fn act(a: FolderAction) -> Action {
    Action::Folders(a)
}

/// Which sidebar tree a row belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tree {
    Folders,
    Smart,
}

impl FolderUi {
    /// Start of a sidebar frame: the rows drawn last frame become the
    /// rows ranges are taken from.
    pub fn begin_rows(&mut self) {
        self.rows = std::mem::take(&mut self.rows_next);
        self.smart_rows = std::mem::take(&mut self.smart_rows_next);
    }

    /// Notes a row as drawn this frame.
    pub fn row(&mut self, tree: Tree, id: &str) {
        match tree {
            Tree::Folders => self.rows_next.push(id.to_string()),
            Tree::Smart => self.smart_rows_next.push(id.to_string()),
        }
    }

    /// Forgets both selections (any change of view clears them, as in
    /// Eagle).
    pub fn clear(&mut self) {
        self.sel.clear();
        self.smart.clear();
    }

    /// Whether the view shows the combined items of picked folders or
    /// smart folders rather than the open scope alone.
    pub fn combined(&self) -> bool {
        !self.sel.ids.is_empty() || !self.smart.ids.is_empty()
    }
}

impl App {
    /// A click on a folder or smart folder row with ⌘ or ⇧ held. Returns
    /// false when neither is held, leaving the plain click to the caller.
    pub(crate) fn tree_click(&mut self, tree: Tree, id: &str, m: Modifiers) -> bool {
        if !m.command && !m.shift {
            return false;
        }
        let open = match (&self.ws.scope, tree) {
            (Scope::Folder(o), Tree::Folders) | (Scope::Smart(o), Tree::Smart) => Some(o.clone()),
            _ => None,
        };
        let scope = |id: &str| match tree {
            Tree::Folders => Scope::Folder(id.to_string()),
            Tree::Smart => Scope::Smart(id.to_string()),
        };
        if m.command && open.is_none() {
            // Nothing of this tree is open: the click opens the row, which
            // is the same view as picking it alone.
            self.go(scope(id));
            return true;
        }
        let ui = &mut self.ws.folders;
        let (sel, rows) = match tree {
            Tree::Folders => (&mut ui.sel, &ui.rows),
            Tree::Smart => (&mut ui.smart, &ui.smart_rows),
        };
        if m.command {
            sel.toggle(open.as_deref(), id);
        } else if !sel.extend(rows, open.as_deref(), id) {
            return true;
        }
        // Picking in one tree drops the other tree's picks.
        match tree {
            Tree::Folders => ui.smart.clear(),
            Tree::Smart => ui.sel.clear(),
        }
        self.ws.selected.clear();
        self.ws.anchor = None;
        true
    }

    /// Whether a sidebar row shows as selected: picked, or, with nothing
    /// picked, the open scope.
    pub(crate) fn tree_active(&self, tree: Tree, id: &str) -> bool {
        let sel = match tree {
            Tree::Folders => &self.ws.folders.sel,
            Tree::Smart => &self.ws.folders.smart,
        };
        if sel.ids.is_empty() {
            match tree {
                Tree::Folders => self.ws.scope == Scope::Folder(id.to_string()),
                Tree::Smart => self.ws.scope == Scope::Smart(id.to_string()),
            }
        } else {
            sel.contains(id)
        }
    }

    /// The items of every picked folder or smart folder, for the combined
    /// view; `None` when nothing is picked.
    pub(crate) fn combined_items(&self, lib: &Library) -> Option<HashSet<String>> {
        let f = &self.ws.folders;
        if !f.sel.ids.is_empty() {
            let picked: HashSet<&str> = f.sel.ids.iter().map(String::as_str).collect();
            return Some(
                lib.items().iter().filter(|i| !i.is_deleted && i.folders.iter().any(|x| picked.contains(x.as_str()))).map(|i| i.id.clone()).collect(),
            );
        }
        if !f.smart.ids.is_empty() {
            return Some(f.smart.ids.iter().flat_map(|id| lib.smart_folder_items(id)).map(|i| i.id.clone()).collect());
        }
        None
    }

    /// The toolbar title while several folders or smart folders are picked.
    pub(crate) fn combined_title(&self) -> Option<String> {
        let f = &self.ws.folders;
        if f.sel.is_multi() {
            Some(format!("{} folders selected", f.sel.ids.len()))
        } else if f.smart.is_multi() {
            Some(format!("{} smart folders selected", f.smart.ids.len()))
        } else {
            None
        }
    }
}

/// The modification time Eagle stores on a smart folder entry (0 if none).
fn smart_mtime(list: &[serde_json::Value], id: &str) -> u64 {
    list.iter()
        .find_map(|v| {
            if v.get("id").and_then(|x| x.as_str()) == Some(id) {
                Some(v.get("modificationTime").and_then(|x| x.as_u64()).unwrap_or(0))
            } else {
                v.get("children").and_then(|c| c.as_array()).map(|c| smart_mtime(c, id)).filter(|&t| t > 0)
            }
        })
        .unwrap_or(0)
}

/// The sort the folders share: `None` unless every one has the same
/// setting.
fn common_sort<'a>(mut sorts: impl Iterator<Item = Option<(FolderOrder, bool)>> + 'a) -> Option<(FolderOrder, bool)> {
    let first = sorts.next()?;
    sorts.all(|s| s == first).then_some(first).flatten()
}

impl App {
    /// The context menu of a folder row while several folders are picked
    /// and it is one of them: Eagle's commands for the whole selection.
    pub(crate) fn multi_folder_menu(&mut self, ui: &mut Ui, lib: &Library, f: &Folder, pinned: bool) {
        ui.set_min_width(200.0);
        let ids = self.ws.folders.sel.ids.clone();
        let folders: Vec<&Folder> = ids.iter().filter_map(|id| logic::find_folder(&lib.metadata.folders, id)).collect();
        if pinned {
            if ui.button("Remove from Quick Access").clicked() {
                self.ws.actions.push(act(FolderAction::QuickRemove(ids.clone())));
            }
        } else if ui.button("Add to Quick Access").clicked() {
            self.ws.actions.push(act(FolderAction::QuickAdd("folder", ids.clone())));
        }
        ui.separator();
        if ui.button("Move Folders…").clicked() {
            self.open_mover(ids.clone());
        }
        if ui.button(format!("Rename {} Folders…", ids.len())).clicked() {
            self.open_rename(Tree::Folders, ids.clone());
        }
        crate::ui::menus::search::submenu(ui, "Sort Items By", |ui| {
            ui.set_min_width(180.0);
            let cur = common_sort(folders.iter().map(|f| crate::organize::own_order(f.order_by.as_deref(), f.sort_increase)));
            if let Some(v) = sort_entries(ui, cur, false, (self.ws.sort, self.ws.increase)) {
                self.ws.actions.push(act(match v {
                    None => FolderAction::Order(ids.clone(), None),
                    Some((k, inc)) if cur.is_some_and(|c| c.0 == k) => FolderAction::Increase(ids.clone(), inc),
                    Some((k, _)) => FolderAction::Order(ids.clone(), Some(k)),
                }));
            }
        });
        // Eagle offers only Export to Computer here, and not from a locked row.
        let shut = crate::organize::is_shut(f, &self.ws.org.unlocked);
        ui.add_enabled_ui(!shut, |ui| {
            crate::ui::menus::search::submenu(ui, "Export", |ui| {
                if ui.button("Export to Computer…").clicked() {
                    self.ws.export.request = Some(crate::ui::dialogs::export::Request::Trees(ids.clone()));
                }
            });
        });
        ui.separator();
        let color = common_value(folders.iter().map(|f| f.icon_color.as_deref()));
        let icon = common_value(folders.iter().map(|f| f.icon.as_deref()));
        let tint = folder_icons::color(color.flatten()).unwrap_or(t::p().text);
        ui.menu_button("Change Icon", |ui| {
            if let Some(pick) = icon_grid(ui, icon.flatten(), tint, false) {
                self.ws.actions.push(act(FolderAction::Restyle(ids.clone(), Some(pick), None)));
                ui.close();
            }
        });
        if let Some(pick) = color_row(ui, color.flatten()) {
            self.ws.actions.push(act(FolderAction::Restyle(ids.clone(), None, Some(pick))));
            ui.close();
        }
        ui.separator();
        if ui.button(RichText::new(format!("Delete {} Folders", ids.len())).color(t::p().icon_red)).clicked() {
            self.ask_delete_folders(lib, ids);
        }
    }

    /// The context menu of a smart folder row while several are picked and
    /// it is one of them.
    pub(crate) fn multi_smart_menu(&mut self, ui: &mut Ui, lib: &Library, pinned: bool) {
        ui.set_min_width(200.0);
        let ids = self.ws.folders.smart.ids.clone();
        let roots = lib.smart_folders();
        let picked: Vec<&falcon_library::smart::SmartFolder> = ids.iter().filter_map(|id| crate::organize::find(&roots, id)).collect();
        crate::ui::menus::search::submenu(ui, "Sort Items By", |ui| {
            ui.set_min_width(180.0);
            let cur = common_sort(picked.iter().map(|f| crate::organize::own_order(f.order_by.as_deref(), f.sort_increase)));
            if let Some(v) = sort_entries(ui, cur, false, (self.ws.sort, self.ws.increase)) {
                self.ws.actions.push(act(match v {
                    None => FolderAction::SmartOrder(ids.clone(), None),
                    Some((k, inc)) if cur.is_some_and(|c| c.0 == k) => FolderAction::SmartIncrease(ids.clone(), inc),
                    Some((k, _)) => FolderAction::SmartOrder(ids.clone(), Some(k)),
                }));
            }
        });
        ui.separator();
        if pinned {
            if ui.button("Remove from Quick Access").clicked() {
                self.ws.actions.push(act(FolderAction::QuickRemove(ids.clone())));
            }
        } else if ui.button("Add to Quick Access").clicked() {
            self.ws.actions.push(act(FolderAction::QuickAdd("smartFolder", ids.clone())));
        }
        if ui.button(format!("Rename {} Smart Folders…", ids.len())).clicked() {
            self.open_rename(Tree::Smart, ids.clone());
        }
        ui.separator();
        let color = common_value(picked.iter().map(|f| f.icon_color.as_deref()));
        let icon = common_value(picked.iter().map(|f| f.icon.as_deref()));
        let tint = folder_icons::color(color.flatten()).unwrap_or(t::p().text);
        ui.menu_button("Change Icon", |ui| {
            if let Some(pick) = icon_grid(ui, icon.flatten(), tint, true) {
                self.ws.actions.push(act(FolderAction::RestyleSmart(ids.clone(), Some(pick), None)));
                ui.close();
            }
        });
        if let Some(pick) = color_row(ui, color.flatten()) {
            self.ws.actions.push(act(FolderAction::RestyleSmart(ids.clone(), None, Some(pick))));
            ui.close();
        }
        ui.separator();
        if ui.button(RichText::new(format!("Delete {} Smart Folders", ids.len())).color(t::p().icon_red)).clicked() {
            self.ws.confirm = Some(Confirm::DeleteSmarts(ids));
        }
    }

    /// Asks before deleting folders, as Eagle does: locked folders are left
    /// out, and a single empty folder without subfolders goes at once.
    pub(crate) fn ask_delete_folders(&mut self, lib: &Library, ids: Vec<String>) {
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| logic::find_folder(&lib.metadata.folders, id).is_some_and(|f| !crate::organize::is_shut(f, &self.ws.org.unlocked)))
            .collect();
        match ids.as_slice() {
            [] => {}
            [one] => {
                let f = logic::find_folder(&lib.metadata.folders, one).expect("filtered above");
                let empty = f.children.is_empty() && !lib.items().iter().any(|i| !i.is_deleted && i.folders.contains(one));
                if empty {
                    self.ws.actions.push(act(FolderAction::Delete(ids, true)));
                } else {
                    self.ws.folders.trash_items = true;
                    self.ws.confirm = Some(Confirm::DeleteFolders(ids));
                }
            }
            _ => {
                self.ws.folders.trash_items = true;
                self.ws.confirm = Some(Confirm::DeleteFolders(ids));
            }
        }
    }

    fn open_rename(&mut self, tree: Tree, ids: Vec<String>) {
        let template = match tree {
            Tree::Folders => "Folder %N",
            Tree::Smart => "Smart Folder %N",
        };
        self.ws.folders.rename = Some(RenameDraft {
            tree,
            ids,
            replace: false,
            template: template.into(),
            start: 1,
            find: String::new(),
            with: String::new(),
            case: TextCase::Keep,
            confirming: false,
        });
    }

    /// Opens Move Folder for these folders, with the sidebar's collapsed
    /// rows.
    pub(crate) fn open_mover(&mut self, ids: Vec<String>) {
        self.ws.folders.mover = Some(MoveDraft { ids, search: String::new(), collapsed: self.ws.collapsed.clone(), ask: None });
    }

    /// The batch rename and Move Folder dialogs.
    pub(crate) fn folder_dialogs(&mut self, ctx: &egui::Context, lib: &Library) {
        self.rename_dialog(ctx, lib);
        self.move_dialog(ctx, lib);
        self.new_with_dialog(ctx);
        self.add_to_folders_dialog(ctx, lib);
        self.ws.folders.open_lock = match &self.ws.scope {
            Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id)
                .filter(|f| f.is_locked())
                .map(|f| crate::organize::is_shut(f, &self.ws.org.unlocked)),
            _ => None,
        };
    }

    /// New Folder with Selection: asks for the name of the folder the
    /// selected items go into.
    fn new_with_dialog(&mut self, ctx: &egui::Context) {
        let Some((items, mut name)) = self.ws.folders.new_with.take() else { return };
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("new-folder-with")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(RichText::new("New Folder").size(15.0));
            ui.add_space(2.0);
            ui.label(RichText::new(format!("The {} selected items go into it.", items.len())).color(t::p().text_muted));
            ui.add_space(6.0);
            let te = ui.add(TextEdit::singleline(&mut name).hint_text("Folder name").desired_width(f32::INFINITY));
            if ui.memory(|m| m.focused().is_none()) {
                te.request_focus();
            }
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clean = logic::clean_folder_name(&name);
                let ok = !clean.is_empty();
                if (ui.add_enabled(ok, egui::Button::new("Create")).clicked() || (enter && ok)) && keep {
                    self.ws.actions.push(act(FolderAction::CreateWith(clean, items.clone())));
                    keep = false;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.ws.folders.new_with = Some((items, name));
        }
    }

    /// Eagle's Rename dialog for several folders or smart folders: a name
    /// template or a find and replace, a case change, and a preview.
    fn rename_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.folders.rename.take() else { return };
        let smart = lib.smart_folders();
        // (id, name, modified, tags) for each folder still there.
        let subjects: Vec<(String, String, u64, Vec<String>)> = d
            .ids
            .iter()
            .filter_map(|id| match d.tree {
                Tree::Folders => logic::find_folder(&lib.metadata.folders, id).map(|f| (f.id.clone(), f.name.clone(), f.modification_time, f.tags.clone())),
                Tree::Smart => crate::organize::find(&smart, id).map(|f| (f.id.clone(), f.name.clone(), smart_mtime(&lib.metadata.smart_folders, id), Vec::new())),
            })
            .collect();
        let rename = d.rename();
        let now = chrono::Local::now();
        let previews: Vec<(String, String, String)> = subjects
            .iter()
            .enumerate()
            .map(|(i, (id, name, mtime, tags))| {
                let s = Subject { name, modified: *mtime, created: *mtime, tags, folders: &[] };
                (id.clone(), name.clone(), rename.apply(&s, i, now))
            })
            .collect();
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("batch-rename")).show(ctx, |ui| {
            ui.set_width(460.0);
            let what = if d.tree == Tree::Folders { "Folders" } else { "Smart Folders" };
            ui.label(RichText::new(format!("Rename {} {what}", previews.len())).size(15.0));
            ui.add_space(6.0);
            if d.confirming {
                ui.label(format!("This renames {} {}. Go ahead?", previews.len(), what.to_lowercase()));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Rename").clicked() {
                        self.finish_rename(&d, &previews);
                        keep = false;
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
                egui::Grid::new("rename-replace").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("Find");
                    ui.add(TextEdit::singleline(&mut d.find).hint_text("Text, or /pattern/flags").desired_width(340.0));
                    ui.end_row();
                    ui.label("Replace with");
                    ui.add(TextEdit::singleline(&mut d.with).desired_width(340.0));
                    ui.end_row();
                });
            } else {
                egui::Grid::new("rename-format").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("Name");
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(&mut d.template).desired_width(270.0));
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
                egui::ComboBox::from_id_salt("rename-case")
                    .selected_text(case_label(d.case))
                    .show_ui(ui, |ui| {
                        for c in [TextCase::Keep, TextCase::Upper, TextCase::Lower, TextCase::Capitalize] {
                            ui.selectable_value(&mut d.case, c, case_label(c));
                        }
                    });
            });
            ui.add_space(6.0);
            ui.separator();
            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("rename-preview").num_columns(3).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
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
                let ready = d.rename().is_ready() && !previews.is_empty();
                if ui.add_enabled(ready, egui::Button::new("Rename")).clicked() {
                    if previews.len() >= RENAME_ASK {
                        d.confirming = true;
                    } else {
                        self.finish_rename(&d, &previews);
                        keep = false;
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
        if keep {
            self.ws.folders.rename = Some(d);
        }
    }

    /// Queues the renames that change a name.
    fn finish_rename(&mut self, d: &RenameDraft, previews: &[(String, String, String)]) {
        let changed: Vec<(String, String)> = previews.iter().filter(|(_, a, b)| a != b).map(|(id, _, b)| (id.clone(), b.clone())).collect();
        if changed.is_empty() {
            return;
        }
        self.ws.actions.push(act(match d.tree {
            Tree::Folders => FolderAction::Rename(changed),
            Tree::Smart => FolderAction::RenameSmart(changed),
        }));
    }

    /// Eagle's Move Folder dialog: the folder tree with a search box; each
    /// row moves the folders above, into or below it, after a confirmation.
    /// The folders moving, and what is inside them, can't be picked.
    fn move_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.folders.mover.take() else { return };
        d.ids.retain(|id| logic::find_folder(&lib.metadata.folders, id).is_some());
        if d.ids.is_empty() {
            return;
        }
        let name = |id: &str| logic::find_folder(&lib.metadata.folders, id).map_or_else(String::new, |f| f.name.clone());
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("move-folders")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new("Move Folder").size(15.0));
            ui.add_space(6.0);
            if let Some((target, place)) = d.ask.clone() {
                let first = name(&d.ids[0]);
                let who = match d.ids.len() {
                    1 => format!("“{first}”"),
                    2 => format!("“{first}” and 1 other folder"),
                    n => format!("“{first}” and {} other folders", n - 1),
                };
                let where_ = match place {
                    FolderPlace::Above => "above",
                    FolderPlace::Into => "into",
                    FolderPlace::Below => "below",
                };
                ui.label(format!("Move {who} {where_} “{}”?", name(&target)));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Move").clicked() {
                        if place == FolderPlace::Into {
                            self.ws.collapsed.remove(&target);
                        }
                        self.ws.actions.push(act(FolderAction::Move(d.ids.clone(), target.clone(), place)));
                        keep = false;
                    }
                    if ui.button("Back").clicked() {
                        d.ask = None;
                    }
                });
                return;
            }
            let te = ui.add(TextEdit::singleline(&mut d.search).hint_text("Search folders").desired_width(f32::INFINITY));
            if ui.memory(|m| m.focused().is_none()) {
                te.request_focus();
            }
            ui.add_space(4.0);
            let filter = d.search.trim().to_lowercase();
            let mut rows = Vec::new();
            move_rows(&lib.metadata.folders, 0, &d, &filter, &self.ws.org.unlocked, &mut rows);
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(RichText::new("No folder matches").color(t::p().text_muted));
                }
                for (f, depth, moving) in rows {
                    self.move_row(ui, &mut d, f, depth, moving, filter.is_empty());
                }
            });
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.ws.folders.mover = Some(d);
        }
    }

    /// One row of the Move Folder tree, with its Above, Into and Below
    /// buttons.
    fn move_row(&mut self, ui: &mut Ui, d: &mut MoveDraft, f: &Folder, depth: usize, moving: bool, browsing: bool) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        let p = ui.painter();
        if resp.hovered() && !moving {
            p.rect_filled(rect, t::RADIUS, t::p().row_hover);
        }
        let x = rect.left() + 18.0 + 18.0 * depth as f32;
        let has_kids = !f.children.is_empty() && browsing;
        if has_kids {
            let open = !d.collapsed.contains(&f.id);
            let caret = Rect::from_center_size(Pos2::new(x - 13.0, rect.center().y), vec2(14.0, rect.height()));
            icon(ui, caret.center(), if open { ic::CARET_DOWN } else { ic::CARET_RIGHT }, 11.0, t::p().text_muted);
            if resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| caret.contains(q)) && !d.collapsed.remove(&f.id) {
                d.collapsed.insert(f.id.clone());
            }
        }
        let (glyph, tint) = App::folder_look(f, false);
        let text = if moving { t::p().text_disabled } else { t::p().text };
        let size = if folder_icons::is_text(glyph) { 10.0 } else { 15.0 };
        icon(ui, Pos2::new(x, rect.center().y), glyph, size, if moving { text } else { tint });
        let g = galley(ui, &f.name, font(13.0), text, rect.right() - x - 170.0);
        ui.painter().galley(Pos2::new(x + 13.0, rect.center().y - g.size().y / 2.0), g, text);
        if moving || !(resp.hovered() || ui.rect_contains_pointer(rect)) {
            return;
        }
        let mut right = rect.right() - 4.0;
        for (label, place) in [("Below", FolderPlace::Below), ("Into", FolderPlace::Into), ("Above", FolderPlace::Above)] {
            let w = 48.0;
            let r = Rect::from_min_max(Pos2::new(right - w, rect.top() + 3.0), Pos2::new(right, rect.bottom() - 3.0));
            if ui.put(r, egui::Button::new(RichText::new(label).size(12.0)).small()).clicked() {
                d.ask = Some((f.id.clone(), place));
            }
            right -= w + 4.0;
        }
    }
}

/// The rows of the Move Folder tree: each folder with its depth and
/// whether it is one of those moving. What is inside a moving folder, or
/// a locked one, is left out; collapsed rows hide their children unless a
/// search is typed, which keeps matching rows and their ancestors.
fn move_rows<'a>(list: &'a [Folder], depth: usize, d: &MoveDraft, filter: &str, unlocked: &HashSet<String>, out: &mut Vec<(&'a Folder, usize, bool)>) {
    fn matches(f: &Folder, filter: &str) -> bool {
        f.name.to_lowercase().contains(filter) || f.children.iter().any(|c| matches(c, filter))
    }
    for f in list {
        if !filter.is_empty() && !matches(f, filter) {
            continue;
        }
        let moving = d.ids.contains(&f.id);
        out.push((f, depth, moving));
        let shut = crate::organize::is_shut(f, unlocked);
        if !moving && !shut && (!filter.is_empty() || !d.collapsed.contains(&f.id)) {
            move_rows(&f.children, depth + 1, d, filter, unlocked, out);
        }
    }
}

/// The value every one has, if they agree: `Some(v)`; else `None`.
fn common_value<'a>(mut values: impl Iterator<Item = Option<&'a str>>) -> Option<Option<&'a str>> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn case_label(c: TextCase) -> &'static str {
    match c {
        TextCase::Keep => "As typed",
        TextCase::Upper => "UPPER CASE",
        TextCase::Lower => "lower case",
        TextCase::Capitalize => "Capitalize Words",
    }
}

impl App {
    /// Applies a queued multi-folder edit.
    pub(crate) fn apply_folder_action(&mut self, lib: &mut Library, a: FolderAction) -> falcon_library::Result<()> {
        match a {
            FolderAction::Rename(names) => lib.rename_folders(&names),
            FolderAction::RenameSmart(names) => {
                let mut r = Ok(());
                for (id, name) in &names {
                    r = r.and(lib.rename_smart_folder(id, name));
                }
                r
            }
            FolderAction::Move(ids, target, place) => lib.move_folders(&ids, &target, place),
            FolderAction::Restyle(ids, icon, color) => lib.restyle_folders(&ids, icon.as_ref().map(Option::as_deref), color.as_ref().map(Option::as_deref)),
            FolderAction::RestyleSmart(ids, icon, color) => {
                let roots = lib.smart_folders();
                let mut r = Ok(());
                for id in &ids {
                    let Some(sf) = crate::organize::find(&roots, id) else { continue };
                    let i = icon.clone().unwrap_or_else(|| sf.icon.clone());
                    let c = color.clone().unwrap_or_else(|| sf.icon_color.clone());
                    r = r.and(lib.set_smart_folder_icon(id, i.as_deref(), c.as_deref()));
                }
                r
            }
            FolderAction::Order(ids, order) => lib.set_folders_order(&ids, order),
            FolderAction::Increase(ids, inc) => lib.set_folders_increase(&ids, inc),
            FolderAction::SmartOrder(ids, order) => {
                let roots = lib.smart_folders();
                let mut r = Ok(());
                for id in &ids {
                    let inc = crate::organize::find(&roots, id).and_then(|f| f.sort_increase).unwrap_or(true);
                    r = r.and(lib.set_smart_folder_order(id, order, inc));
                }
                r
            }
            FolderAction::SmartIncrease(ids, inc) => {
                let roots = lib.smart_folders();
                let mut r = Ok(());
                for id in &ids {
                    // A smart folder following the global sort has no direction to set.
                    if let Some(order) = crate::organize::find(&roots, id).and_then(|f| f.order_by.as_deref().and_then(FolderOrder::parse)) {
                        r = r.and(lib.set_smart_folder_order(id, Some(order), inc));
                    }
                }
                r
            }
            FolderAction::Delete(ids, trash) => {
                let r = lib.delete_folders(&ids, trash);
                self.ws.folders.clear();
                if let Scope::Folder(open) = &self.ws.scope
                    && logic::find_folder(&lib.metadata.folders, open).is_none()
                {
                    self.ws.scope = Scope::All;
                    self.ws.selected.clear();
                }
                r
            }
            FolderAction::DeleteSmart(ids) => {
                let mut r = Ok(());
                for id in &ids {
                    r = r.and(lib.delete_smart_folder(id));
                }
                self.ws.folders.clear();
                if let Scope::Smart(open) = &self.ws.scope
                    && crate::organize::find(&lib.smart_folders(), open).is_none()
                {
                    self.ws.scope = Scope::All;
                    self.ws.selected.clear();
                }
                r
            }
            FolderAction::QuickAdd(kind, ids) => {
                let mut r = Ok(());
                for id in &ids {
                    r = r.and(lib.add_quick_access(kind, id));
                }
                r
            }
            FolderAction::QuickRemove(ids) => {
                let mut r = Ok(());
                for id in &ids {
                    r = r.and(lib.remove_quick_access(id));
                }
                r
            }
            FolderAction::Duplicate(id) => lib.duplicate_folder(&id).map(drop),
            FolderAction::DuplicateSmart(id) => lib.duplicate_smart_folder(&id).map(drop),
            FolderAction::CreateWith(name, items) => {
                let id = lib.create_folder(None, &name)?;
                let mut r = Ok(());
                for item in &items {
                    if let Some(mut it) = lib.item(item).cloned()
                        && falcon_library::autotag::join_folder(&mut it, &lib.metadata.folders, &id)
                    {
                        r = r.and(lib.update_item(it));
                    }
                }
                // Eagle opens the new folder and makes it the last-used one.
                self.note_folder_use(std::slice::from_ref(&id));
                self.go(Scope::Folder(id));
                r
            }
            FolderAction::Categorize(items, add, remove) => self.categorize(lib, &items, &add, &remove),
            FolderAction::PickerCreate(name, place, check) => {
                use crate::folder_picker::NewPlace;
                let (parent, index) = match &place {
                    NewPlace::Top => (None, Some(0)),
                    NewPlace::Inside(p) => (Some(p.clone()), None),
                    NewPlace::After(of) => match crate::organize::locate(&lib.metadata.folders, of) {
                        Some((p, at)) => (p.map(str::to_string), Some(at + 1)),
                        None => (None, Some(0)),
                    },
                };
                let id = lib.create_folder(parent.as_deref(), &name)?;
                if let Some(at) = index {
                    lib.reorder_folder(&id, parent.as_deref(), at)?;
                }
                if let Some(p) = self.ws.folders.picker.as_mut() {
                    if check {
                        p.checked.insert(id.clone());
                    }
                    if let Some(parent) = parent {
                        p.expanded.insert(parent);
                    }
                }
                Ok(())
            }
        }
    }
}

impl App {
    /// Files `items` into the folders `add` (with their auto-tags) and out
    /// of `remove`, as the Add to Folders picker's save does; items that
    /// leave the open view hand the selection on.
    fn categorize(&mut self, lib: &mut Library, items: &[String], add: &[String], remove: &[String]) -> falcon_library::Result<()> {
        let mut r = Ok(());
        for id in items {
            let Some(mut item) = lib.item(id).cloned() else { continue };
            let mut changed = false;
            for f in add {
                changed |= falcon_library::autotag::join_folder(&mut item, &lib.metadata.folders, f);
            }
            let before = item.folders.len();
            item.folders.retain(|f| !remove.contains(f));
            changed |= item.folders.len() != before;
            if changed {
                r = r.and(lib.update_item(item));
            }
        }
        self.note_folder_use(add);
        let leaves = match &self.ws.scope {
            Scope::Folder(open) => remove.contains(open),
            Scope::Uncategorized => !add.is_empty(),
            _ => false,
        };
        if leaves {
            self.leave_viewed(items);
            self.select_after_removal(items);
        }
        let n = crate::ui::dialogs::export::plural(items.len(), "item");
        match add {
            [one] => {
                let name = logic::find_folder(&lib.metadata.folders, one).map_or_else(String::new, |f| f.name.clone());
                self.flash_folder(format!("Added {n} to"), &name, one, true);
            }
            [] => self.flash_undo(format!("Took {n} out of {}", crate::ui::dialogs::export::plural(remove.len(), "folder"))),
            _ => self.flash_undo(format!("Added {n} to {} folders", add.len())),
        }
        r
    }
}
