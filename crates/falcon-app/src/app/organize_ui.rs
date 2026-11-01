//! The app side of organising: folder and smart folder icons, colours,
//! sorts and passwords, drag reordering in the sidebar trees, nested smart
//! folders, tag groups and saved filters. Decisions live in
//! `crate::organize`; this module draws and queues library changes.

use super::*;
use crate::folder_icons;
use crate::organize::{self, DropZone};
use falcon_library::edit::SubfolderSort;
use falcon_library::model::{FolderOrder, SavedFilter};
use falcon_library::smart::SmartFolder;
use falcon_library::smart_draft::{Kind, PROPERTIES, RATINGS, RuleDraft, RuleFields, SHAPES};

/// State kept by the organising features.
#[derive(Default)]
pub(super) struct OrgState {
    /// Locked folders opened with their password this session.
    pub unlocked: HashSet<String>,
    unlock_text: String,
    unlock_error: bool,
    password: Option<PasswordDraft>,
    /// The scope a random folder sort was shuffled for, and its seed.
    seed: Option<(Scope, u64)>,
    /// All Tags shows tag groups instead of A-Z.
    by_group: bool,
    /// Tag group being renamed, with the text so far.
    group_rename: Option<(String, String)>,
    /// Tag group waiting for its delete to be confirmed.
    group_delete: Option<String>,
    /// Saved filter name dialog: `None` saves a new one, `Some(i)` renames.
    filter_name: Option<(Option<usize>, String)>,
    /// Open the rename field of the next tag group created.
    rename_new_group: bool,
    /// Smart folder being renamed in the sidebar, with the text so far.
    smart_rename: Option<(String, String)>,
    /// A sort of folders by title waiting for its confirmation: the
    /// parent whose children are sorted (`None`: the top level).
    sort_confirm: Option<(Option<String>, SubfolderSort)>,
    /// Tag group whose description is being edited, with the text so far.
    group_describe: Option<(String, String)>,
    /// The folder settings (auto-tag) dialog.
    auto_tag: Option<AutoTagDraft>,
    /// ⇧⌘R asked for the open folder's settings; opened once the library
    /// is at hand.
    pub settings_request: Option<String>,
}

/// The folder settings dialog: the folder's name and auto-tags as edited
/// so far, and the text in the tag field.
struct AutoTagDraft {
    id: String,
    name: String,
    tags: Vec<String>,
    typed: String,
}

/// A tag dragged on the All Tags page.
#[derive(Clone)]
struct DragTag(String);

/// A tag group heading dragged on the All Tags page.
#[derive(Clone)]
struct DragGroup(String);

/// A smart folder dragged in the sidebar.
#[derive(Clone)]
struct DragSmart(String);

#[derive(Clone, Copy, PartialEq)]
enum PasswordMode {
    New,
    Change,
    Remove,
}

struct PasswordDraft {
    id: String,
    mode: PasswordMode,
    old: String,
    new: String,
    again: String,
    hint: String,
    error: Option<&'static str>,
}

/// Library changes queued by the organising features.
pub(super) enum OrgAction {
    FolderOrder(String, Option<(FolderOrder, bool)>),
    SmartOrder(String, Option<(FolderOrder, bool)>),
    FolderIcon(String, Option<String>, Option<String>),
    SmartIcon(String, Option<String>, Option<String>),
    ReorderFolder(String, Option<String>, usize),
    MoveSmart(String, Option<String>, usize),
    FolderPassword(String, Option<(String, String)>),
    CreateSmart { parent: Option<String>, name: String, conditions: Vec<serde_json::Value>, icon: Option<String>, color: Option<String> },
    CreateGroup(Vec<String>),
    RenameGroup(String, String),
    GroupColor(String, Option<String>),
    DeleteGroup(String),
    AddToGroup(String, Vec<String>),
    RemoveFromGroup(String, Vec<String>),
    SaveFilter(String, serde_json::Value),
    RenameFilter(usize, String),
    DeleteFilter(usize),
    MoveFilter(usize, usize),
    /// Items dropped at a new place in a folder's hand-made order:
    /// folder, items, and the item they now sit before (`None`: the end).
    MoveManual(String, Vec<String>, Option<String>),
    /// A new smart folder group, "Untitled", in a parent or at the top.
    CreateSmartGroup(Option<String>),
    RenameSmart(String, String),
    /// Sorts the children of a folder (`None`: the top level) by title.
    SortFolders(Option<String>, SubfolderSort),
    /// Moves a tag group to an index (counted without it).
    MoveGroup(String, usize),
    GroupDescription(String, String),
    /// The folder settings dialog's Save: folder, name, auto-tags.
    FolderSettings(String, String, Vec<String>),
}

/// Which stored sort the toolbar's sort menu edits.
#[derive(Clone)]
pub(super) enum OrderTarget {
    Folder(String),
    Smart(String),
}

fn org(a: OrgAction) -> Action {
    Action::Organize(a)
}

// ---------------------------------------------------------------------------
// Small widgets

/// Eagle's colour row: a "no colour" ring, then the eight colours. Returns
/// the pick (`Some(None)` clears the colour).
pub(super) fn color_row(ui: &mut Ui, current: Option<&str>) -> Option<Option<String>> {
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let swatches = std::iter::once((None, t::p().icon_none)).chain(folder_icons::colors().map(|(n, c)| (Some(n), c)));
        for (name, c) in swatches {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
            let p = ui.painter();
            if name.is_some() {
                p.circle_filled(rect.center(), 7.0, c);
            } else {
                p.circle_stroke(rect.center(), 6.5, Stroke::new(1.5, c));
            }
            if name == current {
                p.circle_stroke(rect.center(), 9.5, Stroke::new(1.5, t::p().primary));
            } else if resp.hovered() {
                p.circle_stroke(rect.center(), 9.5, Stroke::new(1.0, t::p().text_muted));
            }
            if resp.clicked() {
                pick = Some(name.map(str::to_string));
            }
            resp.on_hover_text(name.unwrap_or("No colour"));
        }
    });
    pick
}

/// The icon picker: the default icon, then every icon Eagle offers, in a
/// scrolling grid drawn in the current colour. Returns the pick
/// (`Some(None)` is the default icon).
pub(super) fn icon_grid(ui: &mut Ui, current: Option<&str>, tint: Color32, smart: bool) -> Option<Option<String>> {
    let mut pick = None;
    let default = if smart { ic::FOLDER_STAR } else { ic::FOLDER };
    egui::ScrollArea::vertical().id_salt("icon-grid").max_height(320.0).show(ui, |ui| {
        ui.set_width(10.0 * 28.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (name, g) in std::iter::once((None, default)).chain(folder_icons::all().map(|(n, g)| (Some(n), g))) {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                if name == current {
                    ui.painter().rect_filled(rect, t::RADIUS, t::p().row_selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, t::RADIUS, t::p().row_hover);
                }
                let size = if folder_icons::is_text(g) { 11.0 } else { 17.0 };
                icon(ui, rect.center(), g, size, tint);
                if resp.clicked() {
                    pick = Some(name.map(str::to_string));
                }
                resp.on_hover_text(name.unwrap_or("Default"));
            }
        });
    });
    pick
}

/// The item sort entries for a folder or smart folder: "Global Settings"
/// (no own sort), the keys, then the direction. Returns the new setting.
pub(super) fn sort_entries(ui: &mut Ui, current: Option<(FolderOrder, bool)>, manual: bool, global: (OrderBy, bool)) -> Option<Option<(FolderOrder, bool)>> {
    let mut pick = None;
    let mark = |on: bool| if on { ic::CHECK } else { " " };
    if ui.button(format!("{}  Global Settings", mark(current.is_none()))).clicked() {
        pick = Some(None);
    }
    let keys = organize::FOLDER_ORDERS.into_iter().chain(manual.then_some(FolderOrder::Manual));
    for k in keys {
        if ui.button(format!("{}  {}", mark(current.is_some_and(|c| c.0 == k)), organize::order_label(k))).clicked() {
            pick = Some(Some((k, current.is_none_or(|c| c.1))));
        }
    }
    ui.separator();
    let shown = current.unwrap_or((FolderOrder::By(global.0), global.1));
    let labels = organize::direction_labels(shown.0);
    for (inc, label) in [(true, labels[0]), (false, labels[1])] {
        if ui.add_enabled(current.is_some(), egui::Button::new(format!("{}  {label}", mark(current.is_some() && shown.1 == inc)))).clicked() {
            pick = Some(Some((shown.0, inc)));
        }
    }
    pick
}

/// Draws where a dragged tree row would land on the row `resp` and
/// returns the zone, while a `P` payload from another row hovers it.
fn tree_drop_feedback<P: Clone + Send + Sync + 'static>(ui: &Ui, resp: &Response, depth: usize, can_nest: bool, is_self: impl Fn(&P) -> bool) -> Option<DropZone> {
    let payload = resp.dnd_hover_payload::<P>()?;
    if is_self(&payload) {
        return None;
    }
    let y = ui.ctx().pointer_latest_pos()?.y;
    let r = resp.rect;
    let zone = organize::drop_zone(r.top(), r.height(), y, can_nest);
    let x0 = r.left() + icon_x(depth) - 14.0;
    let p = ui.painter();
    match zone {
        DropZone::Into => {
            let bg = Rect::from_min_max(Pos2::new(x0, r.top() + 0.5), Pos2::new(r.right() - 11.0, r.bottom() - 0.5));
            p.rect_filled(bg, t::RADIUS, t::p().primary.gamma_multiply(0.3));
            p.rect_stroke(bg, t::RADIUS, Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        }
        DropZone::Before | DropZone::After => {
            let y = if zone == DropZone::Before { r.top() + 0.5 } else { r.bottom() - 0.5 };
            p.hline(x0..=r.right() - 11.0, y, Stroke::new(1.5, t::p().primary));
            p.circle_stroke(Pos2::new(x0, y), 2.5, Stroke::new(1.5, t::p().primary));
        }
    }
    Some(zone)
}

/// A lock on a sidebar row of a folder with a password: closed while
/// locked, open once unlocked this session.
pub(super) fn lock_badge(ui: &Ui, row: Rect, shut: bool, has_count: bool) {
    let x = row.right() - if has_count { 52.0 } else { 24.0 };
    let glyph = if shut { ic::LOCK_SIMPLE } else { ic::LOCK_SIMPLE_OPEN };
    icon(ui, Pos2::new(x, row.center().y), glyph, 13.0, t::p().text_muted);
}

/// A smart folder's sidebar glyph: its own icon, else the grid for a group
/// (Eagle gives new groups the `grid` icon) or the smart folder icon.
pub(super) fn smart_look(sf: &SmartFolder) -> &'static str {
    folder_icons::smart_glyph(organize::smart_icon(sf))
}

// ---------------------------------------------------------------------------
// Folder rows

impl App {
    /// The folder's icon glyph and tint for its sidebar row.
    pub(super) fn folder_look(f: &Folder, open: bool) -> (&'static str, Color32) {
        (folder_icons::glyph(f.icon.as_deref(), open), folder_icons::color(f.icon_color.as_deref()).unwrap_or(t::p().text))
    }

    /// Reorder and nest by dragging a folder row: an insertion line in the
    /// top or bottom quarter, a highlight in the middle. Queues the move on
    /// release.
    pub(super) fn folder_drop(&mut self, ui: &Ui, lib: &Library, resp: &Response, f: &Folder, depth: usize) {
        let zone = tree_drop_feedback::<DragFolder>(ui, resp, depth, true, |d| d.0 == f.id);
        if let (Some(zone), Some(d)) = (zone, resp.dnd_release_payload::<DragFolder>())
            && let Some((parent, index)) = organize::tree_drop(&lib.metadata.folders, &d.0, &f.id, zone)
        {
            if let Some(p) = &parent {
                self.ws.collapsed.remove(p);
            }
            self.ws.actions.push(org(OrgAction::ReorderFolder(d.0.clone(), parent, index)));
        }
    }

    /// The folder context menu entries falcon adds: password, item sort,
    /// icon and colour.
    pub(super) fn folder_menu_extras(&mut self, ui: &mut Ui, lib: &Library, f: &Folder) {
        let shut = organize::is_shut(f, &self.ws.org.unlocked);
        // Eagle disables its auto-tag entry while the folder is locked.
        if ui.add_enabled(!shut, egui::Button::new("Auto-Tags…")).on_hover_text("Name and auto-tags (⇧⌘R)").clicked() {
            self.open_folder_settings(f);
        }
        ui.menu_button("Password", |ui| {
            let mut open = |mode| {
                self.ws.org.password = Some(PasswordDraft {
                    id: f.id.clone(),
                    mode,
                    old: String::new(),
                    new: String::new(),
                    again: String::new(),
                    hint: if mode == PasswordMode::Change { f.password_tips.clone() } else { String::new() },
                    error: None,
                });
            };
            if !f.is_locked() {
                if ui.button("Create Password…").clicked() {
                    open(PasswordMode::New);
                }
            } else {
                if ui.button("Change Password…").clicked() {
                    open(PasswordMode::Change);
                }
                if ui.button("Remove Password…").clicked() {
                    open(PasswordMode::Remove);
                }
                if ui.add_enabled(!shut, egui::Button::new("Lock Now")).clicked() {
                    self.ws.org.unlocked.remove(&f.id);
                }
            }
        });
        let parent = organize::locate(&lib.metadata.folders, &f.id).and_then(|(p, _)| p).map(str::to_string);
        self.folder_title_sort_menu(ui, parent.as_deref());
        ui.menu_button("Sort Items By", |ui| {
            ui.set_min_width(180.0);
            let cur = organize::own_order(f.order_by.as_deref(), f.sort_increase);
            if let Some(v) = sort_entries(ui, cur, true, (self.ws.sort, self.ws.increase)) {
                self.ws.actions.push(org(OrgAction::FolderOrder(f.id.clone(), v)));
            }
        });
        ui.separator();
        let tint = folder_icons::color(f.icon_color.as_deref()).unwrap_or(t::p().text);
        ui.menu_button("Change Icon", |ui| {
            if let Some(icon) = icon_grid(ui, f.icon.as_deref(), tint, false) {
                self.ws.actions.push(org(OrgAction::FolderIcon(f.id.clone(), icon, f.icon_color.clone())));
                ui.close();
            }
        });
        if let Some(color) = color_row(ui, f.icon_color.as_deref()) {
            self.ws.actions.push(org(OrgAction::FolderIcon(f.id.clone(), f.icon.clone(), color)));
            ui.close();
        }
    }
}

impl App {
    /// The Sort By submenu that orders folders by title, for a folder whose
    /// parent is `parent` (`None`: on the top level, or the Folders
    /// heading). Asks first, as Eagle does.
    pub(super) fn folder_title_sort_menu(&mut self, ui: &mut Ui, parent: Option<&str>) {
        ui.menu_button("Sort By", |ui| {
            ui.set_min_width(200.0);
            for (label, sort) in organize::FOLDER_TITLE_SORTS {
                if ui.button(label).clicked() {
                    self.ws.org.sort_confirm = Some((organize::title_sort_parent(parent, sort).map(str::to_string), sort));
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Smart folder tree

impl App {
    /// One smart folder row and, unless collapsed, its children.
    pub(super) fn smart_row(&mut self, ui: &mut Ui, lib: &Library, roots: &[SmartFolder], sf: &SmartFolder, depth: usize, quick: &HashSet<String>) -> Option<Rect> {
        let has_children = !sf.children.is_empty();
        let open = has_children && !self.ws.collapsed.contains(&sf.id);
        let group = sf.is_group();
        // A group shows no count, as in Eagle.
        let count = if group || !self.prefs.sidebar_counts { 0 } else { lib.smart_folder_items(&sf.id).len() };
        let active = self.tree_active(super::folders_ui::Tree::Smart, &sf.id);
        self.ws.folders.row(super::folders_ui::Tree::Smart, &sf.id);
        let glyph = smart_look(sf);
        let tint = folder_icons::color(sf.icon_color.as_deref()).unwrap_or(t::p().text);
        let renaming = self.ws.org.smart_rename.as_ref().is_some_and(|(id, _)| *id == sf.id);
        let label = if renaming { "" } else { sf.name.as_str() };
        let resp = sidebar_row_tinted(ui, depth, glyph, tint, label, self.badge(count), active, has_children.then_some(open));
        let row = resp.rect;
        if renaming {
            self.smart_rename_field(ui, row, depth, sf);
        }
        if resp.drag_started() {
            resp.dnd_set_drag_payload(DragSmart(sf.id.clone()));
        }
        let zone = tree_drop_feedback::<DragSmart>(ui, &resp, depth, true, |d| d.0 == sf.id);
        if let (Some(zone), Some(d)) = (zone, resp.dnd_release_payload::<DragSmart>())
            && let Some((parent, index)) = organize::tree_drop(roots, &d.0, &sf.id, zone)
        {
            if let Some(p) = &parent {
                self.ws.collapsed.remove(p);
            }
            self.ws.actions.push(org(OrgAction::MoveSmart(d.0.clone(), parent, index)));
        }
        let pinned = quick.contains(&sf.id);
        resp.context_menu(|ui| {
            if self.ws.folders.smart.is_multi() && self.ws.folders.smart.contains(&sf.id) {
                self.multi_smart_menu(ui, lib, pinned);
                return;
            }
            ui.set_min_width(180.0);
            if ui.button("New Subfolder…").clicked() {
                // A group's children start with the folder icon, not the grid.
                let (icon, color) = if group { (None, None) } else { (sf.icon.clone(), sf.icon_color.clone()) };
                self.ws.smart_dialog = Some(SmartDraft { parent: Some(sf.id.clone()), orig_parent: Some(sf.id.clone()), icon, color, ..Default::default() });
            }
            if ui.button("Rename").clicked() {
                self.ws.org.smart_rename = Some((sf.id.clone(), sf.name.clone()));
            }
            // A group has no rules to edit.
            if !group && ui.button("Edit Rules…").clicked() {
                let parent = organize::locate(roots, &sf.id).and_then(|(p, _)| p).map(str::to_string);
                let mut d = SmartDraft::from_folder(sf);
                d.parent.clone_from(&parent);
                d.orig_parent = parent;
                self.ws.smart_dialog = Some(d);
            }
            if pinned {
                if ui.button("Remove from Quick Access").clicked() {
                    self.ws.actions.push(Action::QuickRemove(sf.id.clone()));
                }
            } else if ui.button("Add to Quick Access").clicked() {
                self.ws.actions.push(Action::QuickAdd("smartFolder", sf.id.clone()));
            }
            if depth > 0 && ui.button("Move to Top Level").clicked() {
                self.ws.actions.push(org(OrgAction::MoveSmart(sf.id.clone(), None, usize::MAX)));
            }
            ui.menu_button("Sort Items By", |ui| {
                ui.set_min_width(180.0);
                let cur = organize::own_order(sf.order_by.as_deref(), sf.sort_increase);
                if let Some(v) = sort_entries(ui, cur, false, (self.ws.sort, self.ws.increase)) {
                    self.ws.actions.push(org(OrgAction::SmartOrder(sf.id.clone(), v)));
                }
            });
            if ui.button("Duplicate").clicked() {
                self.ws.actions.push(Action::Folders(super::folders_ui::FolderAction::DuplicateSmart(sf.id.clone())));
            }
            self.tree_export_menu(ui, super::export_ui::Target::Smart(sf.id.clone()), true);
            ui.separator();
            ui.menu_button("Change Icon", |ui| {
                if let Some(icon) = icon_grid(ui, sf.icon.as_deref(), tint, true) {
                    self.ws.actions.push(org(OrgAction::SmartIcon(sf.id.clone(), icon, sf.icon_color.clone())));
                    ui.close();
                }
            });
            if let Some(color) = color_row(ui, sf.icon_color.as_deref()) {
                self.ws.actions.push(org(OrgAction::SmartIcon(sf.id.clone(), sf.icon.clone(), color)));
                ui.close();
            }
            ui.separator();
            let delete = if group { "Delete Smart Folder Group" } else { "Delete Smart Folder" };
            if ui.button(RichText::new(delete).color(t::p().icon_red)).clicked() {
                self.ws.actions.push(Action::DeleteSmart(sf.id.clone()));
            }
        });
        if resp.clicked() {
            let caret = caret_rect(row, depth);
            if has_children && resp.interact_pointer_pos().is_some_and(|p| caret.contains(p)) {
                if !self.ws.collapsed.remove(&sf.id) {
                    self.ws.collapsed.insert(sf.id.clone());
                }
            } else if !self.tree_click(super::folders_ui::Tree::Smart, &sf.id, ui.input(|i| i.modifiers)) {
                self.go(Scope::Smart(sf.id.clone()));
            }
        }
        if resp.double_clicked() {
            match self.prefs.sidebar_double_click {
                crate::prefs::SidebarDoubleClick::Rename => self.ws.org.smart_rename = Some((sf.id.clone(), sf.name.clone())),
                crate::prefs::SidebarDoubleClick::Collapse => {
                    if has_children && !self.ws.collapsed.remove(&sf.id) {
                        self.ws.collapsed.insert(sf.id.clone());
                    }
                }
            }
        }
        if open {
            let mut last = None;
            for c in &sf.children {
                if let Some(r) = self.smart_row(ui, lib, roots, c, depth + 1, quick) {
                    last = Some(r.center().y);
                }
            }
            if let Some(end) = last {
                let x = row.left() + icon_x(depth);
                let p = ui.painter();
                p.line_segment([Pos2::new(x, row.bottom() - 3.0), Pos2::new(x, end - 5.0)], Stroke::new(1.0, t::p().guide));
                p.line_segment([Pos2::new(x, end - 5.0), Pos2::new(x + 5.0, end)], Stroke::new(1.0, t::p().guide));
            }
        }
        Some(row)
    }

    /// The inline rename field over a smart folder's sidebar row. Enter or
    /// leaving the field saves, Escape cancels.
    fn smart_rename_field(&mut self, ui: &mut Ui, row: Rect, depth: usize, sf: &SmartFolder) {
        let r = Rect::from_min_max(Pos2::new(row.left() + icon_x(depth) + 12.0, row.top() + 3.0), Pos2::new(row.right() - 14.0, row.bottom() - 3.0));
        ui.painter().rect_filled(r, CornerRadius::same(4), t::p().field);
        ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        let Some((_, buf)) = self.ws.org.smart_rename.as_mut() else { return };
        let te = ui.put(
            r.shrink2(vec2(4.0, 0.0)),
            TextEdit::singleline(buf).id_salt(("smart-rename", &sf.id)).frame(egui::Frame::NONE).margin(egui::Margin::ZERO).font(font(13.5)).align(Align2::LEFT_CENTER),
        );
        if !te.has_focus() && !te.lost_focus() {
            te.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if (te.lost_focus() || cancel)
            && let Some((id, name)) = self.ws.org.smart_rename.take()
        {
            let name = name.trim().to_string();
            if !cancel && !name.is_empty() && name != sf.name {
                self.ws.actions.push(org(OrgAction::RenameSmart(id, name)));
            }
        }
    }

    /// The Smart Folders heading. Its + button (shown on hover) opens
    /// Eagle's menu: a new smart folder, or a new smart folder group.
    pub(super) fn smart_section(&mut self, ui: &mut Ui) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
        ui.painter().text(Pos2::new(rect.left() + 20.0, rect.center().y), Align2::LEFT_CENTER, "Smart Folders", font(13.0), t::p().text_muted);
        let menu_id = ui.make_persistent_id("smart-add-menu");
        let menu_open = egui::Popup::is_id_open(ui.ctx(), menu_id);
        if !(menu_open || resp.hovered() || ui.rect_contains_pointer(rect)) {
            return;
        }
        let btn = icon_button(ui, Pos2::new(rect.right() - 24.0, rect.center().y), ic::PLUS, 15.0, true, "New Smart Folder");
        egui::Popup::menu(&btn).id(menu_id).show(|ui| {
            ui.set_min_width(200.0);
            if ui.button(format!("{}  New Smart Folder…", ic::FOLDER_STAR)).clicked() {
                self.ws.smart_dialog = Some(SmartDraft::default());
            }
            if ui.button(format!("{}  New Smart Folder Group", folder_icons::glyph(Some("grid"), false))).clicked() {
                self.ws.actions.push(org(OrgAction::CreateSmartGroup(None)));
            }
        });
    }

    /// The smart folder dialog's parent, icon and colour rows.
    pub(super) fn smart_dialog_extras(ui: &mut Ui, d: &mut SmartDraft, lib: &Library) {
        let roots = lib.smart_folders();
        let choices = organize::flatten(&roots, d.editing.as_deref());
        ui.horizontal(|ui| {
            ui.label("Inside");
            let current = d.parent.as_deref().and_then(|p| organize::find(&roots, p)).map_or("Top level".to_string(), |f| f.name.clone());
            egui::ComboBox::from_id_salt("smart-parent").selected_text(current).width(220.0).show_ui(ui, |ui| {
                if ui.selectable_label(d.parent.is_none(), "Top level").clicked() {
                    d.parent = None;
                }
                for (f, depth) in &choices {
                    let label = format!("{}{}  {}", "    ".repeat(*depth), smart_look(f), f.name);
                    if ui.selectable_label(d.parent.as_deref() == Some(f.id.as_str()), label).clicked() {
                        d.parent = Some(f.id.clone());
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Icon");
            let tint = folder_icons::color(d.color.as_deref()).unwrap_or(t::p().text);
            let glyph = folder_icons::smart_glyph(d.icon.as_deref());
            ui.menu_button(RichText::new(glyph).size(16.0).color(tint), |ui| {
                if let Some(icon) = icon_grid(ui, d.icon.as_deref(), tint, true) {
                    d.icon = icon;
                    ui.close();
                }
            });
            ui.add_space(8.0);
            if let Some(c) = color_row(ui, d.color.as_deref()) {
                d.color = c;
            }
        });
    }

    /// Queues the dialog's save: a new smart folder in its parent, or the
    /// edit plus any icon and parent change.
    pub(super) fn save_smart_draft(&mut self, d: &SmartDraft, name: String, conditions: Vec<serde_json::Value>) {
        match d.editing.clone() {
            Some(id) => {
                self.ws.actions.push(Action::UpdateSmart(id.clone(), name, conditions));
                self.ws.actions.push(org(OrgAction::SmartIcon(id.clone(), d.icon.clone(), d.color.clone())));
                if d.parent != d.orig_parent {
                    self.ws.actions.push(org(OrgAction::MoveSmart(id, d.parent.clone(), usize::MAX)));
                }
            }
            None if d.parent.is_none() && d.icon.is_none() && d.color.is_none() => self.ws.actions.push(Action::CreateSmart(name, conditions)),
            None => self.ws.actions.push(org(OrgAction::CreateSmart {
                parent: d.parent.clone(),
                name,
                conditions,
                icon: d.icon.clone(),
                color: d.color.clone(),
            })),
        }
    }
}

/// Eagle's limits on conditions per smart folder and rules per condition.
const MAX_CONDITIONS: usize = 30;
const MAX_RULES: usize = 30;

/// Every folder as `(id, name)`, in tree order, for the folders rule.
pub(super) fn folder_choices(roots: &[Folder]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for r in roots {
        r.walk(&mut |f| out.push((f.id.clone(), f.name.clone())));
    }
    out
}

/// The smart folder dialog's conditions: each with its must/must-not and
/// all/any choice and its rules, AND-ed together.
pub(super) fn smart_conditions_ui(ui: &mut Ui, conditions: &mut Vec<ConditionDraft>, folders: &[(String, String)]) {
    let mut remove = None;
    let count = conditions.len();
    for (ci, c) in conditions.iter_mut().enumerate() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(if ci == 0 { "Items" } else { "And items" });
                egui::ComboBox::from_id_salt(("smart-negate", ci)).selected_text(if c.negate { "must not match" } else { "must match" }).width(120.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut c.negate, false, "must match");
                    ui.selectable_value(&mut c.negate, true, "must not match");
                });
                egui::ComboBox::from_id_salt(("smart-any", ci)).selected_text(if c.any { "any" } else { "all" }).width(60.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut c.any, false, "all");
                    ui.selectable_value(&mut c.any, true, "any");
                });
                ui.label("of these rules");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if count > 1 && ui.button(ic::X).on_hover_text("Remove this condition").clicked() {
                        remove = Some(ci);
                    }
                });
            });
            let mut gone = None;
            let only = c.rules.len() == 1;
            for (ri, r) in c.rules.iter_mut().enumerate() {
                if smart_rule_ui(ui, (ci, ri), r, folders, !only) {
                    gone = Some(ri);
                }
            }
            if let Some(ri) = gone {
                c.rules.remove(ri);
            }
            if c.rules.len() < MAX_RULES && ui.button(format!("{}  Add Rule", ic::PLUS)).clicked() {
                c.rules.push(RuleDraft::default());
            }
        });
        ui.add_space(4.0);
    }
    if let Some(ci) = remove {
        conditions.remove(ci);
    }
    if conditions.len() < MAX_CONDITIONS && ui.button(format!("{}  Add Condition", ic::PLUS)).on_hover_text("Items must also pass every other condition").clicked() {
        conditions.push(ConditionDraft::default());
    }
}

/// One rule row; returns whether its remove button was clicked.
fn smart_rule_ui(ui: &mut Ui, salt: (usize, usize), rule: &mut RuleDraft, folders: &[(String, String)], removable: bool) -> bool {
    let mut remove = false;
    ui.horizontal(|ui| {
        match rule {
            RuleDraft::Unsupported(v) => {
                let text = v.to_string();
                let short: String = if text.chars().count() > 60 { text.chars().take(59).chain(['…']).collect() } else { text.clone() };
                ui.label(RichText::new("Unsupported rule").italics().color(t::p().text_muted))
                    .on_hover_text("Falcon can't show or edit this rule. Saving keeps it exactly as it is; remove it to drop it.");
                ui.label(RichText::new(short).monospace().small().color(t::p().text_muted)).on_hover_text(text);
            }
            RuleDraft::Edit { fields, .. } => rule_fields_ui(ui, salt, fields, folders),
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(removable, egui::Button::new(ic::X)).on_hover_text("Remove this rule").clicked() {
                remove = true;
            }
        });
    });
    remove
}

fn rule_fields_ui(ui: &mut Ui, salt: (usize, usize), f: &mut RuleFields, folders: &[(String, String)]) {
    let kind = f.kind();
    let label = PROPERTIES.iter().find(|p| p.key == f.property).map_or(f.property, |p| p.label);
    egui::ComboBox::from_id_salt(("smart-prop", salt)).selected_text(label).width(130.0).height(400.0).show_ui(ui, |ui| {
        for p in PROPERTIES {
            if ui.selectable_label(f.property == p.key, p.label).clicked() {
                f.set_property(p.key);
            }
        }
    });
    let kind_now = f.kind();
    let methods = PROPERTIES.iter().find(|p| p.key == f.property).map_or(&[][..], |p| p.methods);
    egui::ComboBox::from_id_salt(("smart-method", salt)).selected_text(smart_draft::method_label(kind_now, f.method)).width(110.0).show_ui(ui, |ui| {
        for m in methods {
            if ui.selectable_label(f.method == *m, smart_draft::method_label(kind_now, m)).clicked() {
                f.set_method(m);
            }
        }
    });
    if kind != kind_now || smart_draft::no_value(kind_now, f.method) {
        return;
    }
    let field = |ui: &mut Ui, s: &mut String, hint: &str, w: f32| {
        ui.add(TextEdit::singleline(s).hint_text(hint).desired_width(w));
    };
    let between = f.method == "between";
    match kind {
        Kind::Text => field(ui, &mut f.value, if f.method == "regex" { "pattern" } else { "text" }, 220.0),
        Kind::Type => field(ui, &mut f.value, "png, video, font…", 220.0),
        Kind::Tags => field(ui, &mut f.value, "tag, tag", 220.0),
        Kind::Number | Kind::Size | Kind::Duration => {
            field(ui, &mut f.value, "number", 64.0);
            if between {
                ui.label("and");
                field(ui, &mut f.value2, "number", 64.0);
            }
            let units = smart_draft::units(kind);
            if !units.is_empty() {
                let name = |u: &str| match u {
                    "mb" => "MB",
                    "kb" => "KB",
                    "s" => "seconds",
                    "m" => "minutes",
                    "h" => "hours",
                    _ => "?",
                };
                egui::ComboBox::from_id_salt(("smart-unit", salt)).selected_text(name(f.unit)).width(80.0).show_ui(ui, |ui| {
                    for u in units {
                        if ui.selectable_label(f.unit == *u, name(u)).clicked() {
                            f.unit = u;
                        }
                    }
                });
            }
        }
        Kind::Date if f.method == "within" => {
            field(ui, &mut f.value, "number", 64.0);
            ui.label("days");
        }
        Kind::Date => {
            field(ui, &mut f.value, "YYYY-MM-DD", 96.0);
            if between {
                ui.label("and");
                field(ui, &mut f.value2, "YYYY-MM-DD", 96.0);
            }
        }
        Kind::Folders => {
            let mut ids: Vec<String> = f.value.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            let names: Vec<&str> = ids.iter().map(|id| folders.iter().find(|(i, _)| i == id).map_or(id.as_str(), |(_, n)| n.as_str())).collect();
            let shown = if names.is_empty() { "Choose folders…".to_string() } else { names.join(", ") };
            let mut changed = false;
            ui.menu_button(shown, |ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (id, name) in folders {
                        let mut on = ids.contains(id);
                        if ui.checkbox(&mut on, name).changed() {
                            if on {
                                ids.push(id.clone());
                            } else {
                                ids.retain(|x| x != id);
                            }
                            changed = true;
                        }
                    }
                });
            });
            if changed {
                f.value = ids.join(", ");
            }
        }
        Kind::Rating if f.method == "contain" => field(ui, &mut f.value, "stars, e.g. 35", 120.0),
        Kind::Rating => {
            let shown = RATINGS.iter().find(|r| r.0 == f.value).map_or(f.value.as_str(), |r| r.1).to_string();
            egui::ComboBox::from_id_salt(("smart-rating", salt)).selected_text(shown).width(100.0).show_ui(ui, |ui| {
                for (v, name) in RATINGS {
                    if ui.selectable_label(f.value == *v, *name).clicked() {
                        f.value = v.to_string();
                    }
                }
            });
        }
        Kind::Shape => {
            let shown = SHAPES.iter().find(|s| s.0 == f.value).map_or(f.value.as_str(), |s| s.1).to_string();
            egui::ComboBox::from_id_salt(("smart-shape", salt)).selected_text(shown).width(120.0).show_ui(ui, |ui| {
                for (v, name) in SHAPES {
                    if ui.selectable_label(f.value == *v, *name).clicked() {
                        f.value = v.to_string();
                    }
                }
            });
            if f.value == "custom" {
                field(ui, &mut f.width, "W", 40.0);
                ui.label(":");
                field(ui, &mut f.height, "H", 40.0);
            }
        }
        Kind::Color => {
            field(ui, &mut f.value, "#FF0000", 80.0);
            let hex = f.value.trim().trim_start_matches('#');
            let mut rgb = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6).map_or([0, 0, 0], |v| [(v >> 16) as u8, (v >> 8) as u8, v as u8]);
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                f.value = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
            }
        }
        Kind::Font => {}
    }
}

// ---------------------------------------------------------------------------
// Sorting and locks in the grid

impl App {
    /// The view's own sort target and setting, when browsing a folder or
    /// smart folder (which then owns the sort menu, as in Eagle).
    pub(super) fn own_sort(&self, lib: &Library) -> Option<(OrderTarget, Option<(FolderOrder, bool)>)> {
        // Several picked folders follow the global sort, as in Eagle.
        if self.ws.folders.combined() {
            return None;
        }
        match &self.ws.scope {
            Scope::Folder(id) => {
                let f = logic::find_folder(&lib.metadata.folders, id)?;
                Some((OrderTarget::Folder(id.clone()), organize::own_order(f.order_by.as_deref(), f.sort_increase)))
            }
            Scope::Smart(id) => {
                let roots = lib.smart_folders();
                let f = organize::find(&roots, id)?;
                Some((OrderTarget::Smart(id.clone()), organize::own_order(f.order_by.as_deref(), f.sort_increase)))
            }
            _ => None,
        }
    }

    /// Visible item indices for the grid: the scope, search and filter in
    /// the view's own sort (or the global one), without items in locked
    /// folders.
    pub(super) fn grid_order(&mut self, lib: &Library, only: Option<&HashSet<String>>) -> Vec<usize> {
        let own = self.own_sort(lib).and_then(|o| o.1);
        let (key, inc) = organize::base_key(own, (self.ws.sort, self.ws.increase));
        // Picked folders show their items together: everything the caller's
        // `only` set lets through.
        let scope = if self.ws.folders.combined() { Scope::All } else { self.ws.scope.clone() };
        let mut order = logic::visible(lib.items(), &lib.metadata.folders, &scope, &self.ws.query, &self.ws.filter, key, inc, only);
        // Random and duration re-sort only a plain listing (no search or
        // colour ranking to keep).
        if self.ws.query.trim().is_empty() && self.ws.filter.color.is_none() {
            let seed = match &self.ws.org.seed {
                Some((s, seed)) if *s == self.ws.scope => *seed,
                _ => {
                    let seed = fastrand_seed();
                    self.ws.org.seed = Some((self.ws.scope.clone(), seed));
                    seed
                }
            };
            let folder = if let Scope::Folder(id) = &scope { Some(id.as_str()) } else { None };
            organize::finish_order(&mut order, lib.items(), own, folder, seed);
        }
        let hidden = organize::hidden_folders(&lib.metadata.folders, &self.ws.org.unlocked);
        organize::without_hidden(order, lib.items(), &hidden)
    }

    /// The folder whose items can be dragged into a new order right now
    /// (see [`organize::manual_sortable`]).
    pub(super) fn manual_folder(&self, lib: &Library) -> Option<String> {
        let Scope::Folder(id) = &self.ws.scope else { return None };
        if self.ws.folders.combined() {
            return None;
        }
        let own = self.own_sort(lib).and_then(|o| o.1);
        organize::manual_sortable(true, own, self.ws.sort, self.ws.filter.is_active(), !self.ws.query.trim().is_empty()).then(|| id.clone())
    }

    /// While grid items are dragged over the grid of a hand-sortable
    /// folder: shows where they would land, as Eagle's drop strip beside a
    /// card (a bar between list rows), and queues the move on release.
    /// `places` are the cards in content space; `origin` is the content's
    /// top-left on screen.
    pub(super) fn manual_drag(&mut self, ui: &Ui, folder: &str, places: &[logic::Place], caption: f32, origin: Pos2) {
        let Some(d) = egui::DragAndDrop::payload::<DragItems>(ui.ctx()) else { return };
        let Some(pos) = ui.ctx().pointer_latest_pos().filter(|p| ui.clip_rect().contains(*p)) else { return };
        let order = &self.ws.order;
        let moving: Vec<String> = d.0.iter().filter(|id| order.contains(id)).cloned().collect();
        if moving.is_empty() {
            return;
        }
        let base = origin + vec2(GRID_PAD_X, GRID_PAD_TOP);
        let skip = |n: usize| order.get(n).is_none_or(|id| moving.contains(id));
        let Some(hit) = logic::manual_drop(self.ws.layout, places, caption, GRID_GAP, skip, pos.x - base.x, pos.y - base.y) else { return };
        let [x, y, w, h] = hit.bar;
        let bar = Rect::from_min_size(base + vec2(x, y), vec2(w, h));
        let p = ui.painter();
        p.rect_filled(bar, CornerRadius::same(3), t::p().primary.gamma_multiply(0.3));
        p.rect_stroke(bar, CornerRadius::same(3), Stroke::new(2.0, t::p().primary), StrokeKind::Inside);
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            let before = organize::manual_before(order, &moving, hit.target, hit.after);
            if !organize::manual_noop(order, &moving, before.as_deref()) {
                self.ws.actions.push(org(OrgAction::MoveManual(folder.to_string(), moving, before)));
            }
        }
    }

    /// The sort part of the toolbar's layout menu while a folder or smart
    /// folder owns the sort: changes go to it. With "Global Settings" the
    /// direction still changes the global sort.
    pub(super) fn own_sort_menu(&mut self, ui: &mut Ui, target: OrderTarget, cur: Option<(FolderOrder, bool)>) {
        ui.label(RichText::new("Sort by (this folder)").color(t::p().text_muted).size(12.0));
        let manual = matches!(target, OrderTarget::Folder(_));
        let pick = sort_entries(ui, cur, manual, (self.ws.sort, self.ws.increase));
        if cur.is_none() {
            let labels = if self.ws.sort.is_time() { ["Newest First", "Oldest First"] } else { ["Ascending", "Descending"] };
            ui.label(RichText::new(format!("Global: {}", self.ws.sort.label())).color(t::p().text_muted).size(12.0));
            for (inc, label) in [(true, labels[0]), (false, labels[1])] {
                let mark = if inc == self.ws.increase { ic::CHECK } else { " " };
                if ui.button(format!("{mark}  {label}")).clicked() {
                    self.ws.increase = inc;
                }
            }
        }
        if let Some(v) = pick {
            self.ws.actions.push(org(match target {
                OrderTarget::Folder(id) => OrgAction::FolderOrder(id, v),
                OrderTarget::Smart(id) => OrgAction::SmartOrder(id, v),
            }));
        }
    }

    /// The locked folder that must be opened before the current scope
    /// shows anything.
    pub(super) fn scope_gate<'a>(&self, lib: &'a Library) -> Option<&'a Folder> {
        match &self.ws.scope {
            Scope::Folder(id) => organize::lock_gate(&lib.metadata.folders, id, &self.ws.org.unlocked),
            _ => None,
        }
    }

    /// Eagle's unlock screen, shown in place of a locked folder's items.
    pub(super) fn unlock_screen(&mut self, ui: &mut Ui, gate: &Folder, lib: &Library) {
        let area = ui.max_rect();
        let c = area.center() - vec2(0.0, 60.0);
        icon(ui, c, ic::LOCK_SIMPLE, 48.0, t::p().text_muted);
        ui.painter().text(c + vec2(0.0, 48.0), Align2::CENTER_CENTER, "Unlock to view contents", font(16.0), t::p().text);
        let mut y = c.y + 74.0;
        if !gate.password_tips.is_empty() {
            ui.painter().text(Pos2::new(c.x, y), Align2::CENTER_CENTER, format!("Hint: {}", gate.password_tips), font(13.0), t::p().text_muted);
            y += 24.0;
        }
        let r = Rect::from_center_size(Pos2::new(c.x, y + 14.0), vec2(220.0, 30.0));
        let border = if self.ws.org.unlock_error { t::p().icon_red } else { t::p().field_border };
        ui.painter().rect_filled(r, t::FIELD_RADIUS, t::p().field);
        ui.painter().rect_stroke(r, t::FIELD_RADIUS, Stroke::new(1.0, border), StrokeKind::Inside);
        let te = ui.put(
            r.shrink2(vec2(10.0, 0.0)),
            TextEdit::singleline(&mut self.ws.org.unlock_text)
                .id_salt(("unlock", &gate.id))
                .password(true)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Enter password").color(t::p().text_muted))
                .font(font(14.0))
                .align(Align2::LEFT_CENTER),
        );
        if te.changed() {
            self.ws.org.unlock_error = false;
        }
        if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if lib.verify_folder_password(&gate.id, &self.ws.org.unlock_text) {
                self.ws.org.unlocked.insert(gate.id.clone());
                self.ws.org.unlock_error = false;
            } else {
                self.ws.org.unlock_error = true;
                te.request_focus();
            }
            self.ws.org.unlock_text.clear();
        }
    }
}

// ---------------------------------------------------------------------------
// Tag groups

impl App {
    /// The All Tags page's mode switch and New Group button.
    pub(super) fn tag_page_controls(&mut self, ui: &mut Ui) {
        ui.add_space(12.0);
        ui.label(RichText::new("Group by").color(t::p().text_muted));
        if ui.selectable_label(!self.ws.org.by_group, "Alphabet").clicked() {
            self.ws.org.by_group = false;
        }
        if ui.selectable_label(self.ws.org.by_group, "Tag Group").clicked() {
            self.ws.org.by_group = true;
        }
        if ui.button(format!("{}  New Group", ic::PLUS)).clicked() {
            self.ws.org.by_group = true;
            self.ws.org.rename_new_group = true;
            self.ws.actions.push(org(OrgAction::CreateGroup(Vec::new())));
        }
    }

    /// Whether the page shows tag groups.
    pub(super) fn tags_by_group(&self) -> bool {
        self.ws.org.by_group
    }

    /// The All Tags page in tag-group mode: one section per group, then
    /// the tags in no group.
    pub(super) fn tag_groups_page(&mut self, ui: &mut Ui, lib: &Library, filter: &str) {
        let counts = logic::tag_counts(lib.items());
        let (sections, rest) = organize::tag_sections(&counts, &lib.metadata.tags_groups, filter);
        for sec in &sections {
            self.tag_group_header(ui, lib, Some(sec), sec.tags.len());
            self.tag_chips(ui, lib, &sec.tags);
        }
        self.tag_group_header(ui, lib, None, rest.len());
        self.tag_chips(ui, lib, &rest);
    }

    fn tag_chips(&mut self, ui: &mut Ui, lib: &Library, tags: &[(String, usize)]) {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(GRID_PAD_X);
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (tag, n) in tags {
                self.tag_chip(ui, lib, tag, *n);
            }
        });
        ui.add_space(16.0);
    }

    /// A group's heading: colour dot, name and count, a drop target for
    /// dragged tags, and a menu to rename, recolour or delete it. `None`
    /// is the ungrouped section; dropping there takes a tag out of its group.
    fn tag_group_header(&mut self, ui: &mut Ui, lib: &Library, sec: Option<&organize::TagSection>, n: usize) {
        let sense = if sec.is_some() { Sense::click_and_drag() } else { Sense::click() };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), sense);
        let x = rect.left() + GRID_PAD_X + 2.0;
        let cy = rect.center().y;
        let dot = sec.and_then(|s| folder_icons::color(s.color.as_deref())).unwrap_or(t::p().icon_none);
        if sec.is_some() {
            ui.painter().circle_filled(Pos2::new(x + 5.0, cy), 5.0, dot);
        }
        let tx = if sec.is_some() { x + 18.0 } else { x };
        let renaming = sec.is_some_and(|s| self.ws.org.group_rename.as_ref().is_some_and(|(id, _)| *id == s.id));
        if let (true, Some(s)) = (renaming, sec) {
            let (_, buf) = self.ws.org.group_rename.as_mut().unwrap();
            let te = ui.put(Rect::from_min_size(Pos2::new(tx, rect.top() + 1.0), vec2(220.0, 22.0)), TextEdit::singleline(buf).id_salt(("group-rename", &s.id)).font(font(14.0)));
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if te.lost_focus() || cancel {
                let (id, name) = self.ws.org.group_rename.take().unwrap();
                if !cancel && !name.trim().is_empty() && name.trim() != s.name {
                    self.ws.actions.push(org(OrgAction::RenameGroup(id, name.trim().to_string())));
                }
            }
        } else {
            let title = sec.map_or("Ungrouped", |s| s.name.as_str());
            ui.painter().text(Pos2::new(tx, cy), Align2::LEFT_CENTER, format!("{title}  ({n})"), font(14.0), t::p().text_muted);
        }
        if resp.dnd_hover_payload::<DragTag>().is_some() {
            ui.painter().rect_stroke(rect.shrink2(vec2(GRID_PAD_X - 6.0, 0.0)), t::RADIUS, Stroke::new(1.5, t::p().primary), StrokeKind::Inside);
        }
        if let Some(d) = resp.dnd_release_payload::<DragTag>() {
            match sec {
                Some(s) => self.ws.actions.push(org(OrgAction::AddToGroup(s.id.clone(), vec![d.0.clone()]))),
                None => {
                    if let Some(g) = organize::group_of(&lib.metadata.tags_groups, &d.0) {
                        self.ws.actions.push(org(OrgAction::RemoveFromGroup(g.id.clone(), vec![d.0.clone()])));
                    }
                }
            }
        }
        let Some(s) = sec else { return };
        self.group_heading_drag(ui, lib, &resp, s);
        self.group_description(ui, s);
        if resp.double_clicked() {
            self.ws.org.group_rename = Some((s.id.clone(), s.name.clone()));
        }
        let groups = &lib.metadata.tags_groups;
        let pos = groups.iter().position(|g| g.id == s.id).unwrap_or(0);
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            if ui.button("Rename").clicked() {
                self.ws.org.group_rename = Some((s.id.clone(), s.name.clone()));
            }
            let label = if s.description.is_empty() { "Add Description…" } else { "Edit Description…" };
            if ui.button(label).clicked() {
                self.ws.org.group_describe = Some((s.id.clone(), s.description.clone()));
            }
            if ui.add_enabled(pos > 0, egui::Button::new("Move Up")).clicked() {
                self.ws.actions.push(org(OrgAction::MoveGroup(s.id.clone(), pos - 1)));
            }
            if ui.add_enabled(pos + 1 < groups.len(), egui::Button::new("Move Down")).clicked() {
                self.ws.actions.push(org(OrgAction::MoveGroup(s.id.clone(), pos + 1)));
            }
            if let Some(c) = color_row(ui, s.color.as_deref()) {
                self.ws.actions.push(org(OrgAction::GroupColor(s.id.clone(), c)));
                ui.close();
            }
            ui.separator();
            if ui.button(RichText::new("Delete Group").color(t::p().icon_red)).clicked() {
                if s.tags.is_empty() {
                    self.ws.actions.push(org(OrgAction::DeleteGroup(s.id.clone())));
                } else {
                    self.ws.org.group_delete = Some(s.id.clone());
                }
            }
        });
    }

    /// Reorders groups by dragging their headings: the top half of another
    /// heading drops before it, the bottom half after, shown by a line.
    fn group_heading_drag(&mut self, ui: &Ui, lib: &Library, resp: &Response, s: &organize::TagSection) {
        if resp.drag_started() {
            resp.dnd_set_drag_payload(DragGroup(s.id.clone()));
        }
        let Some(d) = resp.dnd_hover_payload::<DragGroup>().filter(|d| d.0 != s.id) else { return };
        let Some(y) = ui.ctx().pointer_latest_pos().map(|p| p.y) else { return };
        let r = resp.rect;
        let after = organize::drop_zone(r.top(), r.height(), y, false) == DropZone::After;
        let line_y = if after { r.bottom() - 0.5 } else { r.top() + 0.5 };
        let x0 = r.left() + GRID_PAD_X;
        ui.painter().hline(x0..=r.right() - GRID_PAD_X, line_y, Stroke::new(1.5, t::p().primary));
        ui.painter().circle_stroke(Pos2::new(x0, line_y), 2.5, Stroke::new(1.5, t::p().primary));
        if resp.dnd_release_payload::<DragGroup>().is_some()
            && let Some(index) = organize::group_drop(&lib.metadata.tags_groups, &d.0, &s.id, after)
        {
            self.ws.actions.push(org(OrgAction::MoveGroup(d.0.clone(), index)));
        }
    }

    /// A group's description under its heading in muted text, or the field
    /// editing it. Leaving the field saves a changed text; Escape cancels.
    /// Double-click the text to edit it.
    fn group_description(&mut self, ui: &mut Ui, s: &organize::TagSection) {
        let editing = self.ws.org.group_describe.as_ref().is_some_and(|(id, _)| *id == s.id);
        if !editing && s.description.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.add_space(GRID_PAD_X + 20.0);
            let width = (ui.available_width() - GRID_PAD_X - 20.0).max(80.0);
            if let (true, Some((_, buf))) = (editing, self.ws.org.group_describe.as_mut()) {
                let te = ui.add(TextEdit::multiline(buf).id_salt(("group-describe", &s.id)).hint_text("Add a description").desired_rows(2).desired_width(width).font(font(13.0)));
                if !te.has_focus() && !te.lost_focus() {
                    te.request_focus();
                }
                let cancel = ui.input(|i| i.key_pressed(Key::Escape));
                if (te.lost_focus() || cancel)
                    && let Some((id, text)) = self.ws.org.group_describe.take()
                {
                    let text = organize::clean_description(&text);
                    if !cancel && text != s.description {
                        self.ws.actions.push(org(OrgAction::GroupDescription(id, text)));
                    }
                }
            } else {
                let label = ui.add(egui::Label::new(RichText::new(&s.description).size(13.0).color(t::p().text_muted)).wrap().sense(Sense::click()));
                if label.double_clicked() {
                    self.ws.org.group_describe = Some((s.id.clone(), s.description.clone()));
                }
            }
        });
        ui.add_space(2.0);
    }

    /// Lets a tag chip be dragged onto a group heading.
    pub(super) fn tag_chip_drag(resp: &Response, tag: &str) {
        if resp.drag_started() {
            resp.dnd_set_drag_payload(DragTag(tag.to_string()));
        }
    }

    /// The colour dot of a tag filed in a group (grey for an uncoloured group).
    pub(super) fn tag_dot(lib: &Library, tag: &str) -> Option<Color32> {
        organize::group_of(&lib.metadata.tags_groups, tag).map(|g| folder_icons::color(g.color.as_deref()).unwrap_or(t::p().icon_none))
    }

    /// The tag chip menu's Move to Group submenu.
    pub(super) fn tag_group_menu(&mut self, ui: &mut Ui, lib: &Library, tag: &str) {
        let current = organize::group_of(&lib.metadata.tags_groups, tag).map(|g| g.id.clone());
        ui.menu_button("Move to Group", |ui| {
            ui.set_min_width(180.0);
            for g in &lib.metadata.tags_groups {
                let mark = if current.as_deref() == Some(g.id.as_str()) { ic::CHECK } else { " " };
                if ui.button(format!("{mark}  {}", g.name)).clicked() && current.as_deref() != Some(g.id.as_str()) {
                    self.ws.actions.push(org(OrgAction::AddToGroup(g.id.clone(), vec![tag.to_string()])));
                }
            }
            if !lib.metadata.tags_groups.is_empty() {
                ui.separator();
            }
            if ui.button("New Group…").clicked() {
                self.ws.org.by_group = true;
                self.ws.org.rename_new_group = true;
                self.ws.actions.push(org(OrgAction::CreateGroup(vec![tag.to_string()])));
            }
            if let Some(g) = &current
                && ui.button("Remove from Group").clicked()
            {
                self.ws.actions.push(org(OrgAction::RemoveFromGroup(g.clone(), vec![tag.to_string()])));
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Saved filters

impl App {
    /// The filter bar's saved filter menu: save the current filter, then
    /// the saved ones to apply, rename, move or delete.
    pub(super) fn saved_filters_menu(&mut self, ui: &mut Ui, lib: &Library) {
        let n = lib.saved_filters.len();
        let resp = filter_chip(ui, &format!("{}  Saved", ic::FLOPPY_DISK), None);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            let can_save = self.ws.filter.is_active() || !self.ws.query.trim().is_empty();
            if ui.add_enabled(can_save, egui::Button::new("Save This Filter…")).clicked() {
                let name = organize::fresh_name("Filter", lib.saved_filters.iter().map(|f| f.name.as_str()));
                self.ws.org.filter_name = Some((None, name));
            }
            if n > 0 {
                ui.separator();
            }
            for (i, sf) in lib.saved_filters.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(&sf.name).frame(false).min_size(vec2(160.0, 0.0))).clicked() {
                        self.apply_saved_filter(sf);
                    }
                    ui.menu_button("⋯", |ui| {
                        if ui.button("Rename…").clicked() {
                            self.ws.org.filter_name = Some((Some(i), sf.name.clone()));
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("Move Up")).clicked() {
                            self.ws.actions.push(org(OrgAction::MoveFilter(i, i - 1)));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("Move Down")).clicked() {
                            self.ws.actions.push(org(OrgAction::MoveFilter(i, i + 1)));
                        }
                        if ui.button(RichText::new("Remove").color(t::p().icon_red)).clicked() {
                            self.ws.actions.push(org(OrgAction::DeleteFilter(i)));
                        }
                    });
                });
            }
        });
    }

    /// Replaces the filter and search with a saved filter, as Eagle does,
    /// and says which saved rules falcon can't apply yet.
    fn apply_saved_filter(&mut self, sf: &SavedFilter) {
        let q = sf.to_filter();
        let words = |t: &TextFilter| if let TextFilter::Keywords(k) = t { k.join(", ") } else { String::new() };
        self.ws.note_text = words(&q.filter.note);
        self.ws.url_text = words(&q.filter.url);
        self.ws.color_text = q.filter.color.map(|c| logic::hex(c.rgb)).unwrap_or_default();
        self.ws.filter = q.filter;
        self.sync_media_filter_text();
        self.ws.query = q.keyword.unwrap_or_default();
        self.ws.show_filter = true;
        let skipped = organize::unsupported_names(&q.unsupported);
        if skipped.is_empty() {
            self.flash(format!("Applied “{}”", sf.name));
        } else {
            self.flash(format!("Applied “{}” without: {}", sf.name, skipped.join(", ")));
        }
    }
}

// ---------------------------------------------------------------------------
// Dialogs

impl App {
    /// The password, saved filter name and group delete dialogs.
    fn open_folder_settings(&mut self, f: &Folder) {
        self.ws.org.auto_tag = Some(AutoTagDraft { id: f.id.clone(), name: f.name.clone(), tags: f.tags.clone(), typed: String::new() });
    }

    pub(super) fn organize_dialogs(&mut self, ctx: &egui::Context, lib: &Library) {
        if let Some(id) = self.ws.org.settings_request.take()
            && let Some(f) = logic::find_folder(&lib.metadata.folders, &id)
            && !organize::is_shut(f, &self.ws.org.unlocked)
        {
            self.open_folder_settings(f);
        }
        self.auto_tag_dialog(ctx, lib);
        self.password_dialog(ctx, lib);
        self.filter_name_dialog(ctx, lib);
        if let Some((parent, sort)) = self.ws.org.sort_confirm.clone() {
            let what = match (&parent, sort.all_levels) {
                (_, true) => "every folder".to_string(),
                (None, false) => "the top-level folders".to_string(),
                (Some(p), false) => format!("the folders in “{}”", logic::find_folder(&lib.metadata.folders, p).map_or("", |f| f.name.as_str())),
            };
            let modal = egui::Modal::new(egui::Id::unique("sort-folders")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(format!("Sort {what} by title?")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new("Their hand-made order is replaced.").color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Sort by Title").clicked() {
                        self.ws.actions.push(org(OrgAction::SortFolders(parent.clone(), sort)));
                        self.ws.org.sort_confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.ws.org.sort_confirm = None;
                    }
                });
            });
            if modal.should_close() {
                self.ws.org.sort_confirm = None;
            }
        }
        if let Some(id) = self.ws.org.group_delete.clone() {
            let name = lib.metadata.tags_groups.iter().find(|g| g.id == id).map_or_else(String::new, |g| g.name.clone());
            let modal = egui::Modal::new(egui::Id::unique("group-delete")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(format!("Delete the group “{name}”?")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new("Its tags stay on their items.").color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Delete Group").color(t::p().icon_red)).clicked() {
                        self.ws.actions.push(org(OrgAction::DeleteGroup(id.clone())));
                        self.ws.org.group_delete = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.ws.org.group_delete = None;
                    }
                });
            });
            if modal.should_close() {
                self.ws.org.group_delete = None;
            }
        }
    }

    fn password_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.org.password.take() else { return };
        let mut keep = true;
        let name = logic::find_folder(&lib.metadata.folders, &d.id).map_or_else(String::new, |f| f.name.clone());
        let modal = egui::Modal::new(egui::Id::unique("folder-password")).show(ctx, |ui| {
            ui.set_width(340.0);
            let title = match d.mode {
                PasswordMode::New => "Create Password",
                PasswordMode::Change => "Change Password",
                PasswordMode::Remove => "Remove Password",
            };
            ui.label(RichText::new(format!("{title} for “{name}”")).size(15.0));
            ui.add_space(6.0);
            let pw = |ui: &mut Ui, v: &mut String, hint: &str| {
                ui.add(TextEdit::singleline(v).password(true).hint_text(hint).desired_width(f32::INFINITY));
            };
            match d.mode {
                PasswordMode::New => {
                    pw(ui, &mut d.new, "Password");
                    pw(ui, &mut d.again, "Enter the password again");
                }
                PasswordMode::Change => {
                    pw(ui, &mut d.old, "Old password");
                    pw(ui, &mut d.new, "New password");
                    pw(ui, &mut d.again, "Enter the new password again");
                }
                PasswordMode::Remove => pw(ui, &mut d.old, "Current password"),
            }
            if d.mode != PasswordMode::Remove {
                ui.add(TextEdit::multiline(&mut d.hint).hint_text("Password hint").desired_rows(2).desired_width(f32::INFINITY));
            }
            if let Some(e) = d.error {
                ui.label(RichText::new(e).color(t::p().icon_red));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = match d.mode {
                    PasswordMode::New => "Save Password",
                    PasswordMode::Change => "Change Password",
                    PasswordMode::Remove => "Remove Password",
                };
                if ui.button(label).clicked() {
                    match organize::check_password_form(d.mode == PasswordMode::New, d.mode == PasswordMode::Remove, &d.new, &d.again, || lib.verify_folder_password(&d.id, &d.old)) {
                        Err(e) => d.error = Some(e),
                        Ok(()) => {
                            if d.mode == PasswordMode::Remove {
                                self.ws.org.unlocked.remove(&d.id);
                                self.ws.actions.push(org(OrgAction::FolderPassword(d.id.clone(), None)));
                            } else {
                                // The folder stays open until Lock Now or the next launch.
                                self.ws.org.unlocked.insert(d.id.clone());
                                self.ws.actions.push(org(OrgAction::FolderPassword(d.id.clone(), Some((d.new.clone(), d.hint.trim().to_string())))));
                            }
                            keep = false;
                        }
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
            self.ws.org.password = Some(d);
        }
    }

    /// Folder settings, after Eagle's auto-tag dialog: the folder's name and
    /// the tags added to items put into it. Saving a changed list also adds
    /// the folder's tags to the items already in it or in its subfolders.
    fn auto_tag_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.org.auto_tag.take() else { return };
        let Some(folder) = logic::find_folder(&lib.metadata.folders, &d.id) else { return };
        let inherited: Vec<String> = falcon_library::autotag::folder_auto_tags(&lib.metadata.folders, &d.id).into_iter().filter(|t| !folder.tags.contains(t)).collect();
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("folder-settings")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Folder Settings").size(15.0));
            ui.add_space(6.0);
            ui.label(RichText::new("Name").size(12.0).color(t::p().text_muted));
            let name = ui.add(TextEdit::singleline(&mut d.name).char_limit(1024).desired_width(f32::INFINITY));
            let save_key = |ui: &Ui| ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
            let mut save = name.has_focus() && save_key(ui);
            ui.add_space(8.0);
            ui.label(RichText::new("Tags added to items put in this folder").size(12.0).color(t::p().text_muted));
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (n, tag) in d.tags.iter().enumerate() {
                    if chip(ui, tag, true).clicked() {
                        remove = Some(n);
                    }
                }
            });
            if let Some(n) = remove {
                d.tags.remove(n);
            }
            let field = ui.add(TextEdit::singleline(&mut d.typed).hint_text("Add a tag (Return or comma)").desired_width(f32::INFINITY));
            if field.has_focus() && save_key(ui) {
                save = true;
            }
            if d.typed.contains(',') || (field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                organize::add_typed_tags(&mut d.tags, &d.typed);
                d.typed.clear();
                if !save {
                    field.request_focus();
                }
            }
            if !inherited.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("Also added from the folders above: {}", inherited.join(", "))).size(11.0).color(t::p().text_muted));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Saving a changed list adds these tags to the items already in the folder and its subfolders. Tags you remove stay on those items.").size(11.0).color(t::p().text_muted));
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Save").clicked() || save {
                    organize::add_typed_tags(&mut d.tags, &d.typed);
                    let name = if d.name.trim().is_empty() { folder.name.clone() } else { d.name.clone() };
                    if name != folder.name || d.tags != folder.tags {
                        self.ws.actions.push(org(OrgAction::FolderSettings(d.id.clone(), name, d.tags.clone())));
                    }
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
            self.ws.org.auto_tag = Some(d);
        }
    }

    fn filter_name_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some((index, mut name)) = self.ws.org.filter_name.take() else { return };
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("filter-name")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(RichText::new(if index.is_some() { "Rename Saved Filter" } else { "Save This Filter" }).size(15.0));
            ui.add_space(6.0);
            let te = ui.add(TextEdit::singleline(&mut name).hint_text("Name").desired_width(f32::INFINITY));
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = !name.trim().is_empty();
                if (ui.add_enabled(ok, egui::Button::new("Save")).clicked() || (enter && ok)) && keep {
                    let n = name.trim().to_string();
                    self.ws.actions.push(org(match index {
                        Some(i) => OrgAction::RenameFilter(i, n),
                        None => {
                            let keyword = Some(self.ws.query.trim()).filter(|q| !q.is_empty());
                            let rule = SavedFilter::rule_from_filter(&self.ws.filter, keyword, |id| logic::find_folder(&lib.metadata.folders, id).map(|f| f.name.clone()));
                            OrgAction::SaveFilter(n, rule)
                        }
                    }));
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
            self.ws.org.filter_name = Some((index, name));
        }
    }
}

// ---------------------------------------------------------------------------
// Applying changes

impl App {
    pub(super) fn apply_organize(&mut self, lib: &mut Library, a: OrgAction) -> falcon_library::Result<()> {
        match a {
            OrgAction::FolderOrder(id, v) => lib.set_folder_order(&id, v.map(|x| x.0), v.is_none_or(|x| x.1)),
            OrgAction::SmartOrder(id, v) => lib.set_smart_folder_order(&id, v.map(|x| x.0), v.is_none_or(|x| x.1)),
            OrgAction::FolderIcon(id, icon, color) => lib.set_folder_icon(&id, icon.as_deref(), color.as_deref()),
            OrgAction::SmartIcon(id, icon, color) => lib.set_smart_folder_icon(&id, icon.as_deref(), color.as_deref()),
            OrgAction::ReorderFolder(id, parent, index) => lib.reorder_folder(&id, parent.as_deref(), index),
            OrgAction::MoveSmart(id, parent, index) => lib.move_smart_folder(&id, parent.as_deref(), index),
            OrgAction::FolderPassword(id, pw) => lib.set_folder_password(&id, pw.as_ref().map(|(p, h)| (p.as_str(), h.as_str()))),
            OrgAction::CreateSmart { parent, name, conditions, icon, color } => {
                let id = lib.create_smart_folder_in(parent.as_deref(), &name, conditions)?;
                if icon.is_some() || color.is_some() {
                    lib.set_smart_folder_icon(&id, icon.as_deref(), color.as_deref())?;
                }
                if let Some(p) = parent {
                    self.ws.collapsed.remove(&p);
                }
                self.ws.scope = Scope::Smart(id);
                self.ws.selected.clear();
                Ok(())
            }
            OrgAction::CreateGroup(tags) => {
                let name = organize::fresh_name("New Group", lib.metadata.tags_groups.iter().map(|g| g.name.as_str()));
                let id = lib.create_tag_group(&name, &[], None)?;
                if !tags.is_empty() {
                    // Filed tags move out of any other group, as a drag does.
                    lib.add_tags_to_group(&id, &tags)?;
                }
                if std::mem::take(&mut self.ws.org.rename_new_group) {
                    self.ws.org.group_rename = Some((id, name));
                }
                Ok(())
            }
            OrgAction::RenameGroup(id, name) => lib.rename_tag_group(&id, &name),
            OrgAction::GroupColor(id, color) => lib.set_tag_group_color(&id, color.as_deref()),
            OrgAction::DeleteGroup(id) => lib.delete_tag_group(&id, false),
            OrgAction::AddToGroup(id, tags) => lib.add_tags_to_group(&id, &tags),
            OrgAction::RemoveFromGroup(id, tags) => lib.remove_tags_from_group(&id, &tags),
            OrgAction::SaveFilter(name, rule) => lib.create_saved_filter(&name, rule).map(|_| self.flash(format!("Saved “{name}”"))),
            OrgAction::RenameFilter(i, name) => lib.rename_saved_filter(i, &name),
            OrgAction::DeleteFilter(i) => lib.delete_saved_filter(i),
            OrgAction::MoveFilter(i, to) => lib.reorder_saved_filter(i, to),
            OrgAction::MoveManual(folder, ids, before) => lib.move_items_manually(&folder, &ids, before.as_deref()),
            OrgAction::CreateSmartGroup(parent) => {
                // Eagle names it "Untitled", opens it and starts a rename.
                let id = lib.create_smart_folder_group(parent.as_deref(), "Untitled")?;
                if let Some(p) = parent {
                    self.ws.collapsed.remove(&p);
                }
                self.ws.org.smart_rename = Some((id.clone(), "Untitled".into()));
                self.ws.scope = Scope::Smart(id);
                self.ws.selected.clear();
                Ok(())
            }
            OrgAction::RenameSmart(id, name) => lib.rename_smart_folder(&id, &name),
            OrgAction::SortFolders(parent, sort) => lib.sort_subfolders(parent.as_deref(), sort),
            OrgAction::MoveGroup(id, index) => lib.reorder_tag_group(&id, index),
            OrgAction::GroupDescription(id, text) => lib.set_tag_group_description(&id, &text),
            OrgAction::FolderSettings(id, name, tags) => {
                if logic::find_folder(&lib.metadata.folders, &id).is_some_and(|f| f.name != name) {
                    lib.rename_folder(&id, &name)?;
                }
                let n = lib.set_folder_auto_tags(&id, &tags)?;
                if n > 0 {
                    self.flash(format!("Added the folder's tags to {n} item{}", if n == 1 { "" } else { "s" }));
                }
                Ok(())
            }
        }
    }
}
