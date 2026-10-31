//! The app side of organising: folder and smart folder icons, colours,
//! sorts and passwords, drag reordering in the sidebar trees, nested smart
//! folders, tag groups and saved filters. Decisions live in
//! `crate::organize`; this module draws and queues library changes.

use super::*;
use crate::folder_icons;
use crate::organize::{self, DropZone};
use falcon_library::model::{FolderOrder, SavedFilter};
use falcon_library::smart::SmartFolder;

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
}

/// A tag dragged on the All Tags page.
#[derive(Clone)]
struct DragTag(String);

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
fn color_row(ui: &mut Ui, current: Option<&str>) -> Option<Option<String>> {
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let swatches = std::iter::once((None, t::ICON_NONE)).chain(folder_icons::colors().map(|(n, c)| (Some(n), c)));
        for (name, c) in swatches {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
            let p = ui.painter();
            if name.is_some() {
                p.circle_filled(rect.center(), 7.0, c);
            } else {
                p.circle_stroke(rect.center(), 6.5, Stroke::new(1.5, c));
            }
            if name == current {
                p.circle_stroke(rect.center(), 9.5, Stroke::new(1.5, t::PRIMARY));
            } else if resp.hovered() {
                p.circle_stroke(rect.center(), 9.5, Stroke::new(1.0, t::TEXT_MUTED));
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
fn icon_grid(ui: &mut Ui, current: Option<&str>, tint: Color32, smart: bool) -> Option<Option<String>> {
    let mut pick = None;
    let default = if smart { ic::FOLDER_STAR } else { ic::FOLDER };
    egui::ScrollArea::vertical().id_salt("icon-grid").max_height(320.0).show(ui, |ui| {
        ui.set_width(10.0 * 28.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (name, g) in std::iter::once((None, default)).chain(folder_icons::all().map(|(n, g)| (Some(n), g))) {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                if name == current {
                    ui.painter().rect_filled(rect, t::RADIUS, t::ROW_SELECTED);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, t::RADIUS, t::ROW_HOVER);
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
fn sort_entries(ui: &mut Ui, current: Option<(FolderOrder, bool)>, manual: bool, global: (OrderBy, bool)) -> Option<Option<(FolderOrder, bool)>> {
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
            p.rect_filled(bg, t::RADIUS, t::PRIMARY.gamma_multiply(0.3));
            p.rect_stroke(bg, t::RADIUS, Stroke::new(1.0, t::PRIMARY), StrokeKind::Inside);
        }
        DropZone::Before | DropZone::After => {
            let y = if zone == DropZone::Before { r.top() + 0.5 } else { r.bottom() - 0.5 };
            p.hline(x0..=r.right() - 11.0, y, Stroke::new(1.5, t::PRIMARY));
            p.circle_stroke(Pos2::new(x0, y), 2.5, Stroke::new(1.5, t::PRIMARY));
        }
    }
    Some(zone)
}

/// A lock on a sidebar row of a folder with a password: closed while
/// locked, open once unlocked this session.
pub(super) fn lock_badge(ui: &Ui, row: Rect, shut: bool, has_count: bool) {
    let x = row.right() - if has_count { 52.0 } else { 24.0 };
    let glyph = if shut { ic::LOCK_SIMPLE } else { ic::LOCK_SIMPLE_OPEN };
    icon(ui, Pos2::new(x, row.center().y), glyph, 13.0, t::TEXT_MUTED);
}

// ---------------------------------------------------------------------------
// Folder rows

impl App {
    /// The folder's icon glyph and tint for its sidebar row.
    pub(super) fn folder_look(f: &Folder, open: bool) -> (&'static str, Color32) {
        (folder_icons::glyph(f.icon.as_deref(), open), folder_icons::color(f.icon_color.as_deref()).unwrap_or(t::TEXT))
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
    pub(super) fn folder_menu_extras(&mut self, ui: &mut Ui, f: &Folder) {
        let shut = organize::is_shut(f, &self.ws.org.unlocked);
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
        ui.menu_button("Sort Items By", |ui| {
            ui.set_min_width(180.0);
            let cur = organize::own_order(f.order_by.as_deref(), f.sort_increase);
            if let Some(v) = sort_entries(ui, cur, true, (self.ws.sort, self.ws.increase)) {
                self.ws.actions.push(org(OrgAction::FolderOrder(f.id.clone(), v)));
            }
        });
        ui.separator();
        let tint = folder_icons::color(f.icon_color.as_deref()).unwrap_or(t::TEXT);
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

// ---------------------------------------------------------------------------
// Smart folder tree

impl App {
    /// One smart folder row and, unless collapsed, its children.
    pub(super) fn smart_row(&mut self, ui: &mut Ui, lib: &Library, roots: &[SmartFolder], sf: &SmartFolder, depth: usize, quick: &HashSet<String>) -> Option<Rect> {
        let has_children = !sf.children.is_empty();
        let open = has_children && !self.ws.collapsed.contains(&sf.id);
        let count = lib.smart_folder_items(&sf.id).len();
        let active = self.ws.scope == Scope::Smart(sf.id.clone());
        let glyph = folder_icons::smart_glyph(sf.icon.as_deref());
        let tint = folder_icons::color(sf.icon_color.as_deref()).unwrap_or(t::TEXT);
        let resp = sidebar_row_tinted(ui, depth, glyph, tint, &sf.name, (count > 0).then_some(count), active, has_children.then_some(open));
        let row = resp.rect;
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
            ui.set_min_width(180.0);
            if ui.button("New Subfolder…").clicked() {
                self.ws.smart_dialog = Some(SmartDraft { parent: Some(sf.id.clone()), orig_parent: Some(sf.id.clone()), icon: sf.icon.clone(), color: sf.icon_color.clone(), ..Default::default() });
            }
            if ui.button("Edit…").clicked() {
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
            if ui.button(RichText::new("Delete Smart Folder").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                self.ws.actions.push(Action::DeleteSmart(sf.id.clone()));
            }
        });
        if resp.clicked() {
            let caret = caret_rect(row, depth);
            if has_children && resp.interact_pointer_pos().is_some_and(|p| caret.contains(p)) {
                if !self.ws.collapsed.remove(&sf.id) {
                    self.ws.collapsed.insert(sf.id.clone());
                }
            } else {
                self.go(Scope::Smart(sf.id.clone()));
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
                p.line_segment([Pos2::new(x, row.bottom() - 3.0), Pos2::new(x, end - 5.0)], Stroke::new(1.0, t::GUIDE));
                p.line_segment([Pos2::new(x, end - 5.0), Pos2::new(x + 5.0, end)], Stroke::new(1.0, t::GUIDE));
            }
        }
        Some(row)
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
                    let label = format!("{}{}", "    ".repeat(*depth), f.name);
                    if ui.selectable_label(d.parent.as_deref() == Some(f.id.as_str()), label).clicked() {
                        d.parent = Some(f.id.clone());
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Icon");
            let tint = folder_icons::color(d.color.as_deref()).unwrap_or(t::TEXT);
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
    pub(super) fn save_smart_draft(&mut self, d: &SmartDraft, name: String, cond: serde_json::Value) {
        match d.editing.clone() {
            Some(id) => {
                self.ws.actions.push(Action::UpdateSmart(id.clone(), name, vec![cond]));
                self.ws.actions.push(org(OrgAction::SmartIcon(id.clone(), d.icon.clone(), d.color.clone())));
                if d.parent != d.orig_parent {
                    self.ws.actions.push(org(OrgAction::MoveSmart(id, d.parent.clone(), usize::MAX)));
                }
            }
            None if d.parent.is_none() && d.icon.is_none() && d.color.is_none() => self.ws.actions.push(Action::CreateSmart(name, vec![cond])),
            None => self.ws.actions.push(org(OrgAction::CreateSmart {
                parent: d.parent.clone(),
                name,
                conditions: vec![cond],
                icon: d.icon.clone(),
                color: d.color.clone(),
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// Sorting and locks in the grid

impl App {
    /// The view's own sort target and setting, when browsing a folder or
    /// smart folder (which then owns the sort menu, as in Eagle).
    pub(super) fn own_sort(&self, lib: &Library) -> Option<(OrderTarget, Option<(FolderOrder, bool)>)> {
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
        let mut order = logic::visible(lib.items(), &lib.metadata.folders, &self.ws.scope, &self.ws.query, &self.ws.filter, key, inc, only);
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
            organize::finish_order(&mut order, lib.items(), own, seed);
        }
        let hidden = organize::hidden_folders(&lib.metadata.folders, &self.ws.org.unlocked);
        organize::without_hidden(order, lib.items(), &hidden)
    }

    /// The sort part of the toolbar's layout menu while a folder or smart
    /// folder owns the sort: changes go to it. With "Global Settings" the
    /// direction still changes the global sort.
    pub(super) fn own_sort_menu(&mut self, ui: &mut Ui, target: OrderTarget, cur: Option<(FolderOrder, bool)>) {
        ui.label(RichText::new("Sort by (this folder)").color(t::TEXT_MUTED).size(12.0));
        let manual = matches!(target, OrderTarget::Folder(_));
        let pick = sort_entries(ui, cur, manual, (self.ws.sort, self.ws.increase));
        if cur.is_none() {
            let labels = if self.ws.sort.is_time() { ["Newest First", "Oldest First"] } else { ["Ascending", "Descending"] };
            ui.label(RichText::new(format!("Global: {}", self.ws.sort.label())).color(t::TEXT_MUTED).size(12.0));
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
        icon(ui, c, ic::LOCK_SIMPLE, 48.0, t::TEXT_MUTED);
        ui.painter().text(c + vec2(0.0, 48.0), Align2::CENTER_CENTER, "Unlock to view contents", font(16.0), t::TEXT);
        let mut y = c.y + 74.0;
        if !gate.password_tips.is_empty() {
            ui.painter().text(Pos2::new(c.x, y), Align2::CENTER_CENTER, format!("Hint: {}", gate.password_tips), font(13.0), t::TEXT_MUTED);
            y += 24.0;
        }
        let r = Rect::from_center_size(Pos2::new(c.x, y + 14.0), vec2(220.0, 30.0));
        let border = if self.ws.org.unlock_error { Color32::from_rgb(0xff, 0x66, 0x67) } else { t::FIELD_BORDER };
        ui.painter().rect_filled(r, t::FIELD_RADIUS, t::FIELD);
        ui.painter().rect_stroke(r, t::FIELD_RADIUS, Stroke::new(1.0, border), StrokeKind::Inside);
        let te = ui.put(
            r.shrink2(vec2(10.0, 0.0)),
            TextEdit::singleline(&mut self.ws.org.unlock_text)
                .id_salt(("unlock", &gate.id))
                .password(true)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Enter password").color(t::TEXT_MUTED))
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
        ui.label(RichText::new("Group by").color(t::TEXT_MUTED));
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
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        let x = rect.left() + GRID_PAD_X + 2.0;
        let cy = rect.center().y;
        let dot = sec.and_then(|s| folder_icons::color(s.color.as_deref())).unwrap_or(t::ICON_NONE);
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
            ui.painter().text(Pos2::new(tx, cy), Align2::LEFT_CENTER, format!("{title}  ({n})"), font(14.0), t::TEXT_MUTED);
        }
        if resp.dnd_hover_payload::<DragTag>().is_some() {
            ui.painter().rect_stroke(rect.shrink2(vec2(GRID_PAD_X - 6.0, 0.0)), t::RADIUS, Stroke::new(1.5, t::PRIMARY), StrokeKind::Inside);
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
        if resp.double_clicked() {
            self.ws.org.group_rename = Some((s.id.clone(), s.name.clone()));
        }
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            if ui.button("Rename").clicked() {
                self.ws.org.group_rename = Some((s.id.clone(), s.name.clone()));
            }
            if let Some(c) = color_row(ui, s.color.as_deref()) {
                self.ws.actions.push(org(OrgAction::GroupColor(s.id.clone(), c)));
                ui.close();
            }
            ui.separator();
            if ui.button(RichText::new("Delete Group").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                if s.tags.is_empty() {
                    self.ws.actions.push(org(OrgAction::DeleteGroup(s.id.clone())));
                } else {
                    self.ws.org.group_delete = Some(s.id.clone());
                }
            }
        });
    }

    /// Lets a tag chip be dragged onto a group heading.
    pub(super) fn tag_chip_drag(resp: &Response, tag: &str) {
        if resp.drag_started() {
            resp.dnd_set_drag_payload(DragTag(tag.to_string()));
        }
    }

    /// The colour dot of a tag filed in a group (grey for an uncoloured group).
    pub(super) fn tag_dot(lib: &Library, tag: &str) -> Option<Color32> {
        organize::group_of(&lib.metadata.tags_groups, tag).map(|g| folder_icons::color(g.color.as_deref()).unwrap_or(t::ICON_NONE))
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
                        if ui.button(RichText::new("Remove").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
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
    pub(super) fn organize_dialogs(&mut self, ctx: &egui::Context, lib: &Library) {
        self.password_dialog(ctx, lib);
        self.filter_name_dialog(ctx, lib);
        if let Some(id) = self.ws.org.group_delete.clone() {
            let name = lib.metadata.tags_groups.iter().find(|g| g.id == id).map_or_else(String::new, |g| g.name.clone());
            let modal = egui::Modal::new(egui::Id::unique("group-delete")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(format!("Delete the group “{name}”?")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new("Its tags stay on their items.").color(t::TEXT_MUTED));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Delete Group").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
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
                ui.label(RichText::new(e).color(Color32::from_rgb(0xff, 0x66, 0x67)));
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
        }
    }
}
