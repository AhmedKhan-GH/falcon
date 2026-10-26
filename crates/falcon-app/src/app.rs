//! The main window, laid out after Eagle's: sidebar, toolbar, card grid and
//! inspector, with Eagle's dark theme.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Galley, Key, Modifiers, Pos2, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, text::LayoutJob, vec2,
};
use falcon_library::import::ImportOptions;
use falcon_library::model::{Folder, Item};
use falcon_library::{Library, thumbnail};

use crate::icons as ic;
use crate::loader::{Loader, Tier, job_for};
use crate::logic::{self, NavCounts, Scope, SortKey};
use crate::theme::{self as t, font, mono};

/// Grid geometry, measured from Eagle at 1280 x 800.
const GRID_PAD_X: f32 = 17.0;
const GRID_PAD_TOP: f32 = 4.0;
const GRID_GAP: f32 = 8.0;
const CARD_TEXT: f32 = 46.0;
const ROW_GAP: f32 = 10.0;
const TILE_MIN: f32 = 80.0;
const TILE_MAX: f32 = 400.0;

/// Ids being dragged from the grid.
#[derive(Clone)]
struct DragItems(Vec<String>);

/// Library changes requested while drawing, applied at the end of a frame.
enum Action {
    CreateFolder { parent: Option<String> },
    RenameFolder(String, String),
    DeleteFolder(String),
    AddToFolder(Vec<String>, String),
    Trash(Vec<String>, bool),
}

/// Uncommitted inspector text for one item.
struct EditBuf {
    id: String,
    name: String,
    notes: String,
    url: String,
    new_tag: String,
}

pub struct App {
    lib: Option<Library>,
    error: Option<String>,
    loader: Loader,
    scope: Scope,
    back: Vec<Scope>,
    forward: Vec<Scope>,
    query: String,
    sort: SortKey,
    ascending: bool,
    tile: f32,
    selected: Vec<String>,
    anchor: Option<String>,
    viewer: Option<String>,
    collapsed: HashSet<String>,
    folder_filter: String,
    show_sidebar: bool,
    show_inspector: bool,
    edit: Option<EditBuf>,
    status: Option<(String, Instant)>,
    /// Ids in grid order, refreshed every frame.
    order: Vec<String>,
    columns: usize,
    scroll_to: Option<String>,
    /// Images needed this frame, handed to the loader once at the end.
    frame_jobs: Vec<crate::loader::Job>,
    actions: Vec<Action>,
    /// Folder being renamed in the sidebar, with the text typed so far.
    renaming: Option<(String, String)>,
    /// Folder awaiting delete confirmation.
    confirm_delete: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        t::install(&cc.egui_ctx);
        let mut app = App {
            lib: None,
            error: None,
            loader: Loader::new(&cc.egui_ctx),
            scope: Scope::All,
            back: Vec::new(),
            forward: Vec::new(),
            query: String::new(),
            sort: SortKey::DateAdded,
            ascending: false,
            tile: 139.0,
            selected: Vec::new(),
            anchor: None,
            viewer: None,
            collapsed: HashSet::new(),
            folder_filter: String::new(),
            show_sidebar: true,
            show_inspector: true,
            edit: None,
            status: None,
            order: Vec::new(),
            columns: 1,
            scroll_to: None,
            frame_jobs: Vec::new(),
            actions: Vec::new(),
            renaming: None,
            confirm_delete: None,
        };
        if let Some(p) = initial {
            app.open(p);
        }
        app
    }

    fn open(&mut self, path: PathBuf) {
        match Library::open(&path) {
            Ok(lib) => {
                self.lib = Some(lib);
                self.error = None;
                self.scope = Scope::All;
                self.back.clear();
                self.forward.clear();
                self.selected.clear();
                self.edit = None;
                self.loader.clear_all();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn pick_library(&mut self) {
        if let Some(p) = rfd::FileDialog::new().set_title("Open Eagle Library").pick_folder() {
            self.open(p);
        }
    }

    fn go(&mut self, scope: Scope) {
        let scope = match scope {
            Scope::Random(_) => Scope::Random(fastrand_seed()),
            s => s,
        };
        if scope == self.scope {
            return;
        }
        self.back.push(std::mem::replace(&mut self.scope, scope));
        self.forward.clear();
        self.selected.clear();
        self.anchor = None;
    }

    fn go_back(&mut self) {
        if let Some(s) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.scope, s));
            self.selected.clear();
        }
    }

    fn go_forward(&mut self) {
        if let Some(s) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.scope, s));
            self.selected.clear();
        }
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    /// Applies a click on the card at `id` with the given modifiers.
    fn click(&mut self, id: &str, m: Modifiers) {
        if m.command {
            if let Some(p) = self.selected.iter().position(|s| s == id) {
                self.selected.remove(p);
            } else {
                self.selected.push(id.to_string());
            }
            self.anchor = Some(id.to_string());
        } else if m.shift
            && let Some(a) = self.anchor.as_ref().and_then(|a| self.order.iter().position(|x| x == a))
            && let Some(b) = self.order.iter().position(|x| x == id)
        {
            let (lo, hi) = (a.min(b), a.max(b));
            self.selected = self.order[lo..=hi].to_vec();
        } else {
            self.selected = vec![id.to_string()];
            self.anchor = Some(id.to_string());
        }
    }

    fn import(&mut self, paths: Vec<PathBuf>) {
        let Some(lib) = self.lib.as_mut() else { return };
        let folders = match &self.scope {
            Scope::Folder(id) => vec![id.clone()],
            _ => Vec::new(),
        };
        let opts = ImportOptions { folders, ..Default::default() };
        let (mut ok, mut failed) = (0, 0);
        let mut stack = paths;
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&p) {
                    stack.extend(rd.flatten().map(|e| e.path()));
                }
                continue;
            }
            match lib.import_file(&p, &opts) {
                Ok(_) => ok += 1,
                Err(_) => failed += 1,
            }
        }
        let msg = if failed > 0 { format!("Added {ok}, {failed} failed") } else { format!("Added {ok}") };
        self.flash(msg);
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (left, right, up, down, space, esc, all, del) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Space) || i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.modifiers.command && i.key_pressed(Key::A),
                i.modifiers.command && i.key_pressed(Key::Backspace),
            )
        });
        if esc {
            if self.viewer.is_some() {
                self.viewer = None;
            } else {
                self.selected.clear();
            }
        }
        if all {
            self.selected = self.order.clone();
        }
        if space && self.viewer.is_none() && self.selected.len() == 1 {
            self.viewer = self.selected.first().cloned();
            return;
        }
        if del && !self.selected.is_empty() {
            let trash = self.scope != Scope::Trash;
            let ids = self.selected.clone();
            if let Some(lib) = self.lib.as_mut()
                && lib.set_trashed(&ids, trash).is_ok()
            {
                self.flash(if trash { "Moved to Trash" } else { "Restored" });
                self.selected.clear();
            }
        }
        let step: isize = if self.viewer.is_some() {
            match (left || up, right || down) {
                (true, _) => -1,
                (_, true) => 1,
                _ => 0,
            }
        } else {
            let c = self.columns as isize;
            match (left, right, up, down) {
                (true, ..) => -1,
                (_, true, ..) => 1,
                (_, _, true, _) => -c,
                (.., true) => c,
                _ => 0,
            }
        };
        if step != 0 && !self.order.is_empty() {
            let cur = self.viewer.as_ref().or(self.selected.last());
            let pos = cur.and_then(|c| self.order.iter().position(|x| x == c));
            let next = match pos {
                Some(p) => (p as isize + step).clamp(0, self.order.len() as isize - 1) as usize,
                None => 0,
            };
            let id = self.order[next].clone();
            if self.viewer.is_some() {
                self.viewer = Some(id.clone());
            }
            self.selected = vec![id.clone()];
            self.anchor = Some(id.clone());
            self.scroll_to = Some(id);
        }
    }
}

impl App {
    fn apply_actions(&mut self, lib: &mut Library) {
        for action in std::mem::take(&mut self.actions) {
            let result = match action {
                Action::CreateFolder { parent } => lib.create_folder(parent.as_deref(), "New Folder").map(|id| {
                    if let Some(p) = parent {
                        self.collapsed.remove(&p);
                    }
                    self.renaming = Some((id, "New Folder".into()));
                }),
                Action::RenameFolder(id, name) => lib.rename_folder(&id, &name),
                Action::DeleteFolder(id) => {
                    if self.scope == Scope::Folder(id.clone()) {
                        self.scope = Scope::All;
                    }
                    lib.delete_folder(&id)
                }
                Action::AddToFolder(ids, fid) => {
                    let n = ids.len();
                    let mut r = Ok(());
                    for id in ids {
                        if let Some(mut item) = lib.item(&id).cloned()
                            && !item.folders.contains(&fid)
                        {
                            item.folders.push(fid.clone());
                            item.is_deleted = false;
                            r = r.and(lib.update_item(item));
                        }
                    }
                    let name = logic::find_folder(&lib.metadata.folders, &fid).map_or_else(String::new, |f| f.name.clone());
                    self.flash(format!("Added {n} to {name}"));
                    r
                }
                Action::Trash(ids, trash) => {
                    self.selected.retain(|s| !ids.contains(s));
                    let r = lib.set_trashed(&ids, trash);
                    self.flash(if trash { format!("Moved {} to Trash", ids.len()) } else { format!("Restored {}", ids.len()) });
                    r
                }
            };
            if let Err(e) = result {
                self.flash(format!("Could not save: {e}"));
            }
        }
    }
}

fn fastrand_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

// ---------------------------------------------------------------------------
// Small painting helpers

fn galley(ui: &Ui, text: &str, f: egui::FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), f, color);
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

fn icon(ui: &Ui, center: Pos2, glyph: &str, size: f32, color: Color32) {
    ui.painter().text(center, Align2::CENTER_CENTER, glyph, font(size), color);
}

/// A bare icon button, as in Eagle's toolbars.
fn icon_button(ui: &mut Ui, center: Pos2, glyph: &str, size: f32, enabled: bool, tip: &str) -> Response {
    let rect = Rect::from_center_size(center, Vec2::splat(26.0));
    let resp = ui.interact(rect, ui.scope_id().with(("icon", glyph, center.x as i32)), Sense::click());
    let color = if !enabled {
        t::TEXT_DISABLED
    } else if resp.hovered() {
        t::TEXT
    } else {
        Color32::from_gray(222)
    };
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, t::RADIUS, t::ROW_HOVER);
    }
    icon(ui, center, glyph, size, color);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// Makes `rect` drag the window, and double-click zoom it, like a title bar.
fn window_drag(ui: &Ui, rect: Rect, id: &str) {
    let resp = ui.interact(rect, ui.scope_id().with(("drag", id)), Sense::click_and_drag());
    if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
    }
}

/// A rounded input box with Eagle's field styling.
fn field(ui: &mut Ui, value: &mut String, hint: &str, height: f32, multiline: bool, id: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, t::FIELD_RADIUS, t::FIELD);
    p.rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
    let inner = rect.shrink2(vec2(11.0, if multiline { 8.0 } else { 0.0 }));
    let edit = if multiline { TextEdit::multiline(value) } else { TextEdit::singleline(value) };
    let edit = edit
        .id_salt(id)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .hint_text(RichText::new(hint).color(t::TEXT_MUTED))
        .font(font(14.0))
        .text_color(t::TEXT)
        .desired_width(inner.width())
        .align(if multiline { Align2::LEFT_TOP } else { Align2::LEFT_CENTER });
    ui.put(inner, edit)
}

/// A file-type label like Eagle's: translucent type colour, uppercase.
fn badge(ui: &Ui, top_left: Pos2, ext: &str) {
    let Some(c) = t::ext_color(ext) else { return };
    let g = galley(ui, &ext.to_uppercase(), font(10.0), Color32::from_white_alpha(204), 60.0);
    let rect = Rect::from_min_size(top_left, vec2(g.size().x + 6.0, 18.0));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), c.gamma_multiply(0.6));
    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, c.gamma_multiply(0.5)), StrokeKind::Inside);
    p.galley(Pos2::new(rect.left() + 3.0, rect.center().y - g.size().y / 2.0), g, Color32::WHITE);
}

// ---------------------------------------------------------------------------
// Sidebar

struct SidebarCtx<'a> {
    folder_counts: &'a HashMap<String, usize>,
    nav: &'a NavCounts,
}

impl App {
    fn sidebar(&mut self, ui: &mut Ui, lib: &Library) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::PANEL);
        p.vline(full.right() - 0.5, full.y_range(), Stroke::new(1.0, t::DIVIDER));

        // Header: traffic lights on the left, actions on the right.
        let header = Rect::from_min_size(full.min, vec2(full.width(), t::BAR_HEIGHT));
        window_drag(ui, header, "sidebar");
        let cy = header.center().y;
        let r = full.right();
        if icon_button(ui, Pos2::new(r - 24.0, cy), ic::SIDEBAR_SIMPLE, 18.0, true, "Hide sidebar").clicked() {
            self.show_sidebar = false;
        }
        icon_button(ui, Pos2::new(r - 51.0, cy), ic::ARROWS_LEFT_RIGHT, 18.0, true, "Switch Library")
            .clicked()
            .then(|| self.pick_library());
        if icon_button(ui, Pos2::new(r - 79.0, cy), ic::PLUS, 18.0, true, "Create Folder").clicked() {
            self.actions.push(Action::CreateFolder { parent: None });
        }

        // Library switcher.
        let lib_row = Rect::from_min_size(Pos2::new(full.left(), header.bottom()), vec2(full.width(), 30.0));
        let resp = ui.interact(lib_row, ui.scope_id().with("library"), Sense::click());
        let ic_rect = Rect::from_center_size(Pos2::new(full.left() + 29.0, lib_row.center().y), Vec2::splat(22.0));
        p.rect_filled(ic_rect, CornerRadius::same(5), t::BRAND);
        icon(ui, ic_rect.center(), ic::FOLDER, 14.0, Color32::WHITE);
        let name = lib.root().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let g = galley(ui, &name, font(15.0), t::TEXT, full.width() - 80.0);
        let gw = g.size().x;
        p.galley(Pos2::new(full.left() + 44.0, lib_row.center().y - g.size().y / 2.0), g, t::TEXT);
        icon(ui, Pos2::new(full.left() + 44.0 + gw + 10.0, lib_row.center().y), ic::CARET_UP_DOWN, 12.0, t::TEXT);
        if resp.on_hover_text("Switch Library").clicked() {
            self.pick_library();
        }

        // Filter box at the bottom.
        let filter_rect = Rect::from_min_max(
            Pos2::new(full.left() + 11.0, full.bottom() - 37.0),
            Pos2::new(full.right() - 11.0, full.bottom() - 11.0),
        );

        let list_rect = Rect::from_min_max(Pos2::new(full.left(), lib_row.bottom() + 6.0), Pos2::new(full.right() - 1.0, filter_rect.top() - 8.0));
        let folder_counts = logic::folder_counts(lib.items());
        let nav = logic::nav_counts(lib.items());
        let cx = SidebarCtx { folder_counts: &folder_counts, nav: &nav };
        let mut list = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
        egui::ScrollArea::vertical().id_salt("sidebar-list").auto_shrink([false, false]).show(&mut list, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.nav_rows(ui, &cx);
            ui.add_space(12.0);
            section(ui, "Smart Folders");
            let total = lib.all_folders().len();
            section(ui, &format!("Folders ({total})"));
            let filter = self.folder_filter.trim().to_lowercase();
            for f in &lib.metadata.folders {
                self.folder_row(ui, f, 0, &cx, &filter);
            }
            ui.add_space(8.0);
        });

        p.rect_filled(filter_rect, t::RADIUS, t::FIELD);
        p.rect_stroke(filter_rect, t::RADIUS, Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
        icon(ui, Pos2::new(filter_rect.left() + 15.0, filter_rect.center().y), ic::FUNNEL, 15.0, t::TEXT_MUTED);
        let inner = Rect::from_min_max(Pos2::new(filter_rect.left() + 30.0, filter_rect.top()), filter_rect.max);
        ui.put(
            inner,
            TextEdit::singleline(&mut self.folder_filter)
                .id_salt("folder-filter")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Filter").color(t::TEXT_MUTED))
                .font(font(13.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width() - 8.0),
        );
    }

    fn nav_rows(&mut self, ui: &mut Ui, cx: &SidebarCtx) {
        let n = cx.nav;
        let entries: [(&str, &str, Scope, usize); 7] = [
            (ic::IMAGES, "All", Scope::All, n.all),
            (ic::FOLDER_DASHED, "Uncategorized", Scope::Uncategorized, n.uncategorized),
            (ic::TAG_SIMPLE, "Untagged", Scope::Untagged, n.untagged),
            (ic::CLOCK, "Recently Used", Scope::Recent, 0),
            (ic::SHUFFLE, "Random", Scope::Random(0), 0),
            (ic::BOOKMARK_SIMPLE, "All Tags", Scope::AllTags, n.tags),
            (ic::TRASH, "Trash", Scope::Trash, n.trash),
        ];
        for (glyph, label, scope, count) in entries {
            let active = match (&scope, &self.scope) {
                (Scope::Random(_), Scope::Random(_)) => true,
                (a, b) => a == b,
            };
            let resp = sidebar_row(ui, 0, glyph, label, (count > 0).then_some(count), active, None);
            if scope == Scope::Trash {
                drop_highlight(ui, &resp, 0);
                if let Some(d) = resp.dnd_release_payload::<DragItems>() {
                    self.actions.push(Action::Trash(d.0.clone(), true));
                }
            }
            if resp.clicked() {
                self.go(scope);
            }
        }
    }

    /// One folder and, unless collapsed, its children. Returns whether
    /// anything was drawn (filtered folders are hidden with their subtree
    /// unless a descendant matches).
    fn folder_row(&mut self, ui: &mut Ui, f: &Folder, depth: usize, cx: &SidebarCtx, filter: &str) -> Option<Rect> {
        if !filter.is_empty() && !subtree_matches(f, filter) {
            return None;
        }
        let has_children = !f.children.is_empty();
        let open = has_children && (!filter.is_empty() || !self.collapsed.contains(&f.id));
        let active = self.scope == Scope::Folder(f.id.clone());
        let count = cx.folder_counts.get(&f.id).copied();
        let glyph = if open { ic::FOLDER_OPEN } else { ic::FOLDER };
        let renaming = self.renaming.as_ref().is_some_and(|(id, _)| *id == f.id);
        let label = if renaming { "" } else { f.name.as_str() };
        let resp = sidebar_row(ui, depth, glyph, label, count, active, has_children.then_some(open));
        let row_rect = resp.rect;
        if renaming {
            let r = Rect::from_min_max(
                Pos2::new(row_rect.left() + icon_x(depth) + 12.0, row_rect.top() + 3.0),
                Pos2::new(row_rect.right() - 14.0, row_rect.bottom() - 3.0),
            );
            ui.painter().rect_filled(r, CornerRadius::same(4), t::FIELD);
            ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t::PRIMARY), StrokeKind::Inside);
            let (_, buf) = self.renaming.as_mut().unwrap();
            let te = ui.put(
                r.shrink2(vec2(4.0, 0.0)),
                TextEdit::singleline(buf)
                    .id_salt(("rename", &f.id))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .font(font(13.5))
                    .align(Align2::LEFT_CENTER),
            );
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if te.lost_focus() || cancel {
                let (id, name) = self.renaming.take().unwrap();
                let name = name.trim().to_string();
                if !cancel && !name.is_empty() && name != f.name {
                    self.actions.push(Action::RenameFolder(id, name));
                }
            }
        }
        drop_highlight(ui, &resp, depth);
        if let Some(d) = resp.dnd_release_payload::<DragItems>() {
            self.actions.push(Action::AddToFolder(d.0.clone(), f.id.clone()));
        }
        resp.context_menu(|ui| {
            ui.set_min_width(180.0);
            if ui.button("New Subfolder").clicked() {
                self.actions.push(Action::CreateFolder { parent: Some(f.id.clone()) });
            }
            if ui.button("Rename").clicked() {
                self.renaming = Some((f.id.clone(), f.name.clone()));
            }
            ui.separator();
            if ui.button(RichText::new("Delete Folder").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                self.confirm_delete = Some(f.id.clone());
            }
        });
        if resp.clicked() {
            let caret = caret_rect(row_rect, depth);
            if has_children && resp.interact_pointer_pos().is_some_and(|p| caret.contains(p)) {
                if !self.collapsed.remove(&f.id) {
                    self.collapsed.insert(f.id.clone());
                }
            } else {
                self.go(Scope::Folder(f.id.clone()));
            }
        }
        if resp.double_clicked() && has_children && !self.collapsed.remove(&f.id) {
            self.collapsed.insert(f.id.clone());
        }
        if open {
            let mut last_center = None;
            for c in &f.children {
                if let Some(r) = self.folder_row(ui, c, depth + 1, cx, filter) {
                    last_center = Some(r.center().y);
                }
            }
            if let Some(end) = last_center {
                // Guide line from this folder's icon down to its last child.
                let x = row_rect.left() + icon_x(depth);
                let p = ui.painter();
                let top = row_rect.bottom() - 3.0;
                p.line_segment([Pos2::new(x, top), Pos2::new(x, end - 5.0)], Stroke::new(1.0, t::GUIDE));
                p.line_segment([Pos2::new(x, end - 5.0), Pos2::new(x + 5.0, end)], Stroke::new(1.0, t::GUIDE));
            }
        }
        Some(row_rect)
    }
}

/// Outlines a sidebar row while grid items are dragged over it.
fn drop_highlight(ui: &Ui, resp: &Response, depth: usize) {
    if resp.dnd_hover_payload::<DragItems>().is_some() {
        let r = resp.rect;
        let bg = Rect::from_min_max(Pos2::new(r.left() + icon_x(depth) - 14.0, r.top() + 0.5), Pos2::new(r.right() - 11.0, r.bottom() - 0.5));
        ui.painter().rect_filled(bg, t::RADIUS, t::PRIMARY.gamma_multiply(0.25));
        ui.painter().rect_stroke(bg, t::RADIUS, Stroke::new(1.5, t::PRIMARY), StrokeKind::Inside);
    }
}

fn subtree_matches(f: &Folder, filter: &str) -> bool {
    f.name.to_lowercase().contains(filter) || f.children.iter().any(|c| subtree_matches(c, filter))
}

/// Horizontal centre of a row's icon at `depth`.
fn icon_x(depth: usize) -> f32 {
    29.0 + 20.0 * depth as f32
}

fn caret_rect(row: Rect, depth: usize) -> Rect {
    Rect::from_center_size(Pos2::new(row.left() + icon_x(depth) - 19.0, row.center().y), vec2(16.0, row.height()))
}

fn section(ui: &mut Ui, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
    ui.painter().text(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        font(13.0),
        t::TEXT_MUTED,
    );
}

fn sidebar_row(
    ui: &mut Ui,
    depth: usize,
    glyph: &str,
    label: &str,
    count: Option<usize>,
    active: bool,
    caret: Option<bool>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t::ROW_HEIGHT), Sense::click());
    let ix = rect.left() + icon_x(depth);
    let bg = Rect::from_min_max(Pos2::new(ix - 14.0, rect.top() + 0.5), Pos2::new(rect.right() - 11.0, rect.bottom() - 0.5));
    let p = ui.painter();
    if active {
        p.rect_filled(bg, t::RADIUS, t::ROW_SELECTED);
    } else if resp.hovered() {
        p.rect_filled(bg, t::RADIUS, t::ROW_HOVER);
    }
    if let Some(open) = caret {
        // A small solid triangle, pointing down when open.
        let c = caret_rect(rect, depth).center();
        let pts = if open {
            vec![c + vec2(-4.5, -2.5), c + vec2(4.5, -2.5), c + vec2(0.0, 3.0)]
        } else {
            vec![c + vec2(-2.5, -4.5), c + vec2(3.0, 0.0), c + vec2(-2.5, 4.5)]
        };
        p.add(egui::Shape::convex_polygon(pts, t::TEXT_MUTED, Stroke::NONE));
    }
    p.text(Pos2::new(ix, rect.center().y), Align2::CENTER_CENTER, glyph, font(18.0), t::TEXT);
    let count_w = if count.is_some() { 30.0 } else { 0.0 };
    let g = galley(ui, label, font(13.5), t::TEXT, rect.right() - 20.0 - count_w - (ix + 15.0));
    p.galley(Pos2::new(ix + 15.0, rect.center().y - g.size().y / 2.0), g, t::TEXT);
    if let Some(n) = count {
        p.text(Pos2::new(rect.right() - 19.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), font(12.0), t::TEXT_MUTED);
    }
    resp
}

// ---------------------------------------------------------------------------
// Toolbar

impl App {
    fn toolbar(&mut self, ui: &mut Ui, lib: &Library) {
        let bar = ui.max_rect();
        window_drag(ui, bar, "toolbar");
        let cy = bar.center().y;
        let mut x = bar.left() + 22.0;
        if !self.show_sidebar {
            x += 72.0; // room for the traffic lights
            if icon_button(ui, Pos2::new(x, cy), ic::SIDEBAR_SIMPLE, 18.0, true, "Show sidebar").clicked() {
                self.show_sidebar = true;
            }
            x += 30.0;
        }
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 17.0, !self.back.is_empty(), "").clicked() {
            self.go_back();
        }
        if icon_button(ui, Pos2::new(x + 27.0, cy), ic::CARET_RIGHT, 17.0, !self.forward.is_empty(), "").clicked() {
            self.go_forward();
        }
        let title = if self.query.trim().is_empty() {
            self.scope.title(&lib.metadata.folders)
        } else {
            "Search results".into()
        };
        let g = galley(ui, &title, font(15.0), t::TEXT, 220.0);
        let title_end = x + 45.0 + g.size().x;
        ui.painter().galley(Pos2::new(x + 45.0, cy - g.size().y / 2.0), g, t::TEXT);

        // Right side: search field, then the icon row.
        let search = Rect::from_min_size(Pos2::new(bar.right() - 18.0 - 178.0, cy - 15.0), vec2(178.0, 30.0));
        self.search_field(ui, search);
        let mut ix = search.left() - 20.0;
        let filter = icon_button(ui, Pos2::new(ix, cy), ic::FUNNEL, 18.0, true, "Filter");
        ix -= 26.0;
        let layout = icon_button(ui, Pos2::new(ix, cy), ic::LAYOUT, 18.0, true, "Layout and sort");
        ix -= 26.0;
        icon_button(ui, Pos2::new(ix, cy), ic::LIGHTNING, 18.0, false, "Actions");
        ix -= 26.0;
        icon_button(ui, Pos2::new(ix, cy), ic::PUZZLE_PIECE, 18.0, false, "Plugins");
        let icons_left = ix - 20.0;

        egui::Popup::menu(&layout).show(|ui| self.layout_menu(ui));
        egui::Popup::menu(&filter).show(|ui| {
            ui.set_min_width(180.0);
            ui.label(RichText::new("Filter by type").color(t::TEXT_MUTED).size(12.0));
            for ext in ["svg", "png", "jpg", "pdf", "gif"] {
                if ui.button(format!(".{ext}")).clicked() {
                    self.query = format!(".{ext}");
                }
            }
        });

        // Zoom slider between the title and the icons.
        let zx = (bar.left() + 237.0).max(title_end + 30.0);
        if zx + 130.0 < icons_left {
            self.zoom_slider(ui, Pos2::new(zx, cy));
        }
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, t::MAIN));
    }

    fn search_field(&mut self, ui: &mut Ui, rect: Rect) {
        let p = ui.painter();
        p.rect_filled(rect, t::FIELD_RADIUS, t::SEARCH);
        p.rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::FIELD_BORDER), StrokeKind::Inside);
        icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 16.0, t::TEXT);
        icon(ui, Pos2::new(rect.left() + 28.0, rect.center().y + 1.0), ic::CARET_DOWN, 9.0, t::TEXT_MUTED);
        icon(ui, Pos2::new(rect.right() - 17.0, rect.center().y), ic::CAMERA, 17.0, t::TEXT_DISABLED);
        let inner = Rect::from_min_max(Pos2::new(rect.left() + 40.0, rect.top()), Pos2::new(rect.right() - 32.0, rect.bottom()));
        ui.put(
            inner,
            TextEdit::singleline(&mut self.query)
                .id_salt("search")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Search").color(t::TEXT_MUTED))
                .font(font(14.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width()),
        );
    }

    fn zoom_slider(&mut self, ui: &mut Ui, left_center: Pos2) {
        let cy = left_center.y;
        let x0 = left_center.x;
        if icon_button(ui, Pos2::new(x0, cy), ic::MINUS, 13.0, true, "").clicked() {
            self.tile = (self.tile - 20.0).max(TILE_MIN);
        }
        let track = Rect::from_min_max(Pos2::new(x0 + 16.0, cy - 8.0), Pos2::new(x0 + 104.0, cy + 8.0));
        if icon_button(ui, Pos2::new(x0 + 120.0, cy), ic::PLUS, 13.0, true, "").clicked() {
            self.tile = (self.tile + 20.0).min(TILE_MAX);
        }
        let resp = ui.interact(track, ui.scope_id().with("zoom"), Sense::click_and_drag());
        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked())
        {
            let f = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            self.tile = TILE_MIN + f * (TILE_MAX - TILE_MIN);
        }
        let f = (self.tile - TILE_MIN) / (TILE_MAX - TILE_MIN);
        let p = ui.painter();
        let y = track.center().y;
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(track.right(), y)], Stroke::new(2.0, Color32::from_gray(70)));
        let kx = track.left() + f * track.width();
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(kx, y)], Stroke::new(2.0, Color32::from_gray(200)));
        p.circle_filled(Pos2::new(kx, y), 7.0, Color32::WHITE);
    }

    fn layout_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(190.0);
        ui.label(RichText::new("Sort by").color(t::TEXT_MUTED).size(12.0));
        for k in SortKey::ALL {
            let mark = if k == self.sort { ic::CHECK } else { " " };
            if ui.button(format!("{mark}  {}", k.label())).clicked() {
                self.sort = k;
            }
        }
        ui.separator();
        for (asc, label) in [(true, "Ascending"), (false, "Descending")] {
            let mark = if asc == self.ascending { ic::CHECK } else { " " };
            if ui.button(format!("{mark}  {label}")).clicked() {
                self.ascending = asc;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Grid

impl App {
    fn grid(&mut self, ui: &mut Ui, lib: &Library) {
        let area = ui.max_rect();
        let order = logic::visible(lib.items(), &lib.metadata.folders, &self.scope, &self.query, self.sort, self.ascending);
        let items: Vec<&Item> = order.iter().map(|&i| &lib.items()[i]).collect();
        self.order = items.iter().map(|i| i.id.clone()).collect();

        if self.scope == Scope::AllTags && self.query.trim().is_empty() {
            self.tag_page(ui, lib);
            return;
        }
        if items.is_empty() {
            ui.painter().text(area.center(), Align2::CENTER_CENTER, "No items", font(15.0), t::TEXT_MUTED);
            return;
        }

        let s = self.tile;
        let width = area.width() - 2.0 * GRID_PAD_X;
        let cols = logic::columns(width, s, GRID_GAP);
        self.columns = cols;
        let row_h = s + CARD_TEXT + ROW_GAP;
        let rows = items.len().div_ceil(cols);
        let total_h = GRID_PAD_TOP + rows as f32 * row_h + 20.0;

        let mut scroll = egui::ScrollArea::vertical().id_salt(("grid", format!("{:?}", self.scope))).auto_shrink([false, false]);
        if let Some(id) = self.scroll_to.take()
            && let Some(pos) = self.order.iter().position(|x| *x == id)
        {
            let row = (pos / cols) as f32;
            let top = GRID_PAD_TOP + row * row_h;
            let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
            let view_h = area.height();
            if top < cur {
                scroll = scroll.vertical_scroll_offset(top);
            } else if top + row_h > cur + view_h {
                scroll = scroll.vertical_scroll_offset(top + row_h - view_h);
            }
        }

        let mut jobs = Vec::new();
        let mut clicked: Option<(String, Modifiers)> = None;
        let mut opened: Option<String> = None;
        let mut bg_clicked = false;
        let out = scroll.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let bg = ui.allocate_rect(Rect::from_min_size(origin, vec2(area.width(), total_h.max(area.height()))), Sense::click());
            if bg.clicked() {
                bg_clicked = true;
            }
            let first = ((viewport.min.y - GRID_PAD_TOP) / row_h).floor().max(0.0) as usize;
            let last = (((viewport.max.y - GRID_PAD_TOP) / row_h).ceil() as usize + 1).min(rows);
            for r in first..last {
                for c in 0..cols {
                    let n = r * cols + c;
                    let Some(item) = items.get(n) else { break };
                    let tl = origin + vec2(GRID_PAD_X + c as f32 * (s + GRID_GAP), GRID_PAD_TOP + r as f32 * row_h);
                    let card = Rect::from_min_size(tl, vec2(s, s + CARD_TEXT));
                    let resp = ui.interact(card, ui.scope_id().with(("card", &item.id)), Sense::click_and_drag());
                    if resp.drag_started() {
                        let ids = if self.selected.contains(&item.id) { self.selected.clone() } else { vec![item.id.clone()] };
                        resp.dnd_set_drag_payload(DragItems(ids));
                    }
                    if resp.secondary_clicked() && !self.selected.contains(&item.id) {
                        self.selected = vec![item.id.clone()];
                    }
                    let scope_is_trash = self.scope == Scope::Trash;
                    let targets: Vec<String> = if self.selected.contains(&item.id) { self.selected.clone() } else { vec![item.id.clone()] };
                    resp.context_menu(|ui| {
                        ui.set_min_width(200.0);
                        if ui.button("Open").clicked() {
                            opened = Some(item.id.clone());
                        }
                        if ui.button("Show in Finder").clicked() {
                            let _ = std::process::Command::new("open").arg("-R").arg(item.original_path(lib.root())).spawn();
                        }
                        if ui.button("Copy File Path").clicked() {
                            ui.ctx().copy_text(item.original_path(lib.root()).display().to_string());
                        }
                        ui.menu_button("Add to Folder", |ui| {
                            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                for f in lib.all_folders() {
                                    if ui.button(&f.name).clicked() {
                                        self.actions.push(Action::AddToFolder(targets.clone(), f.id.clone()));
                                    }
                                }
                            });
                        });
                        ui.separator();
                        if scope_is_trash {
                            if ui.button("Restore").clicked() {
                                self.actions.push(Action::Trash(targets.clone(), false));
                            }
                        } else if ui.button("Move to Trash").clicked() {
                            self.actions.push(Action::Trash(targets.clone(), true));
                        }
                    });
                    if resp.clicked() {
                        clicked = Some((item.id.clone(), ui.input(|i| i.modifiers)));
                    }
                    if resp.double_clicked() {
                        opened = Some(item.id.clone());
                    }
                    let selected = self.selected.contains(&item.id);
                    self.card(ui, lib, item, card, s, selected, resp.hovered());
                    if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                        jobs.push(j);
                    }
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(ui.scope_id().with("grid-offset"), out.state.clamped_offset().y));
        self.frame_jobs.extend(jobs);
        if let Some((id, m)) = clicked {
            self.click(&id, m);
        } else if bg_clicked {
            self.selected.clear();
        }
        if let Some(id) = opened {
            self.viewer = Some(id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &Ui, lib: &Library, item: &Item, card: Rect, s: f32, selected: bool, hovered: bool) {
        let tile = Rect::from_min_size(card.min, Vec2::splat(s));
        let p = ui.painter();
        p.rect_filled(tile, t::RADIUS, t::MAIN);
        let border = if hovered { t::CARD_HOVER } else { t::CARD_BORDER };
        p.rect_stroke(tile, t::RADIUS, Stroke::new(1.0, border), StrokeKind::Inside);

        let inner = tile.shrink(6.0);
        let failed = self.loader.has_failed(&item.id, Tier::Grid);
        match self.loader.get(&item.id, Tier::Grid).cloned() {
            Some(tex) => {
                let tex_size = tex.size_vec2();
                // Eagle's object-fit: scale-down never enlarges a small image.
                let natural = if item.width > 0 && item.height > 0 {
                    vec2(item.width as f32, item.height as f32)
                } else {
                    tex_size
                };
                let scale = (inner.width() / natural.x).min(inner.height() / natural.y).min(1.0);
                let size = natural * scale;
                let r = Rect::from_center_size(inner.center(), size);
                egui::Image::new((tex.id(), size)).corner_radius(4).paint_at(ui, r);
            }
            None if failed || job_for(item, lib.root(), Tier::Grid).is_none() => {
                ext_placeholder(ui, inner, &item.ext);
            }
            None => {}
        }
        badge(ui, tile.min + vec2(5.0, 5.0), &item.ext);
        if selected {
            p.rect_stroke(tile.expand(1.0), CornerRadius::same(7), Stroke::new(2.5, t::PRIMARY), StrokeKind::Inside);
        }

        let name = format!("{}.{}", item.name, item.ext);
        let g = galley(ui, &name, font(13.0), t::TEXT, s);
        let y = tile.bottom() + 14.0;
        if selected {
            let pill = Rect::from_center_size(Pos2::new(tile.center().x, y), vec2(g.size().x + 10.0, 19.0));
            p.rect_filled(pill, CornerRadius::same(4), t::PRIMARY);
        }
        p.galley(Pos2::new(tile.center().x - g.size().x / 2.0, y - g.size().y / 2.0), g, t::TEXT);
        if item.width > 0 && item.height > 0 {
            p.text(
                Pos2::new(tile.center().x, tile.bottom() + 32.0),
                Align2::CENTER_CENTER,
                format!("{} × {}", item.width, item.height),
                font(12.0),
                t::TEXT_MUTED,
            );
        }
    }

    fn tag_page(&mut self, ui: &mut Ui, lib: &Library) {
        let tags = logic::tag_counts(lib.items());
        egui::ScrollArea::vertical().id_salt("tags-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(14.0);
            let mut current: Option<char> = None;
            let mut groups: Vec<(char, Vec<(String, usize)>)> = Vec::new();
            for (tag, n) in tags {
                let first = tag.chars().next().map_or('#', |c| c.to_ascii_uppercase());
                let first = if first.is_alphabetic() { first } else { '#' };
                if current != Some(first) {
                    groups.push((first, Vec::new()));
                    current = Some(first);
                }
                groups.last_mut().unwrap().1.push((tag, n));
            }
            for (letter, group) in groups {
                ui.horizontal(|ui| {
                    ui.add_space(GRID_PAD_X + 4.0);
                    ui.label(RichText::new(letter.to_string()).size(15.0).color(t::TEXT_MUTED));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(GRID_PAD_X);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for (tag, n) in group {
                        let text = format!("{tag}   {n}");
                        let g = galley(ui, &text, font(13.0), t::TEXT, 400.0);
                        let (rect, resp) = ui.allocate_exact_size(g.size() + vec2(22.0, 12.0), Sense::click());
                        let fill = if resp.hovered() { t::BUTTON_HOVER } else { t::BUTTON };
                        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                        ui.painter().galley(rect.min + vec2(11.0, 6.0), g, t::TEXT);
                        if resp.clicked() {
                            self.go(Scope::Tag(tag.clone()));
                        }
                    }
                });
                ui.add_space(14.0);
            }
        });
    }
}

fn ext_placeholder(ui: &Ui, rect: Rect, ext: &str) {
    let s = rect.width().min(rect.height()) * 0.42;
    let r = Rect::from_center_size(rect.center(), vec2(s * 0.8, s));
    icon(ui, r.center(), ic::FILE, s, Color32::from_gray(90));
    ui.painter().text(
        r.center() + vec2(0.0, s * 0.12),
        Align2::CENTER_CENTER,
        ext.to_uppercase(),
        font((s * 0.18).max(9.0)),
        Color32::from_gray(150),
    );
}

// ---------------------------------------------------------------------------
// Inspector

impl App {
    fn inspector(&mut self, ui: &mut Ui, lib: &mut Library) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::PANEL);
        p.vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0, t::DIVIDER));
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
            match self.selected.len() {
                0 => self.scope_inspector(ui, lib),
                1 => {
                    let id = self.selected[0].clone();
                    if let Some(item) = lib.item(&id).cloned() {
                        self.item_inspector(ui, lib, item);
                    }
                }
                n => self.multi_inspector(ui, lib, n),
            }
        });
    }

    fn scope_inspector(&mut self, ui: &mut Ui, lib: &Library) {
        let folder = match &self.scope {
            Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id).cloned(),
            _ => None,
        };
        let mut name = self.scope.title(&lib.metadata.folders);
        let mut desc = folder.as_ref().map(|f| f.description.clone()).unwrap_or_default();
        field(ui, &mut name, "Name", 33.0, false, "scope-name");
        if folder.is_some() {
            field(ui, &mut desc, "Description", 33.0, false, "scope-desc");
        }
        divider(ui);
        let items: Vec<&Item> = self.order.iter().filter_map(|id| lib.item(id)).collect();
        let size: u64 = items.iter().map(|i| i.size).sum();
        heading(ui, "Properties");
        prop(ui, "Items", &items.len().to_string());
        prop(ui, "Size", &logic::format_size(size));
        if let Some(f) = &folder {
            prop(ui, "Date Imported", &logic::format_ms(f.modification_time));
            prop(ui, "Password", if f.password.is_empty() { "None" } else { "Set" });
        }
        ui.add_space(4.0);
        wide_button(ui, ic::EXPORT, "Export", false);
    }

    fn multi_inspector(&mut self, ui: &mut Ui, lib: &Library, n: usize) {
        let size: u64 = self.selected.iter().filter_map(|id| lib.item(id)).map(|i| i.size).sum();
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{n} items selected")).size(15.0));
        divider(ui);
        heading(ui, "Properties");
        prop(ui, "Items", &n.to_string());
        prop(ui, "Size", &logic::format_size(size));
    }

    fn item_inspector(&mut self, ui: &mut Ui, lib: &mut Library, item: Item) {
        if self.edit.as_ref().is_none_or(|e| e.id != item.id) {
            self.edit = Some(EditBuf {
                id: item.id.clone(),
                name: item.name.clone(),
                notes: item.annotation.clone(),
                url: item.url.clone(),
                new_tag: String::new(),
            });
        }
        let w = ui.available_width();

        // Preview.
        let aspect = if item.width > 0 && item.height > 0 { item.height as f32 / item.width as f32 } else { 1.0 };
        let ph = (w * aspect).clamp(80.0, 260.0);
        let (prect, presp) = ui.allocate_exact_size(vec2(w, ph), Sense::click());
        if let Some(tex) = self.loader.get(&item.id, Tier::Preview) {
            let ts = if item.width > 0 { vec2(item.width as f32, item.height as f32) } else { tex.size_vec2() };
            let scale = (prect.width() / ts.x).min(prect.height() / ts.y).min(1.0);
            let r = Rect::from_center_size(prect.center(), ts * scale);
            egui::Image::new((tex.id(), r.size())).corner_radius(4).paint_at(ui, r);
        } else if !thumbnail::is_supported(&item.ext) && item.thumbnail_path(lib.root()).is_none() {
            ext_placeholder(ui, prect, &item.ext);
        }
        if presp.double_clicked() {
            self.viewer = Some(item.id.clone());
        }
        if let Some(j) = job_for(&item, lib.root(), Tier::Preview) {
            self.frame_jobs.push(j);
        }

        // Palette.
        if !item.palettes.is_empty() {
            let d = 16.0;
            let (row, _) = ui.allocate_exact_size(vec2(w, d), Sense::hover());
            for (n, c) in item.palettes.iter().enumerate() {
                let center = Pos2::new(row.left() + d / 2.0 + n as f32 * (d + 6.0), row.center().y);
                if center.x > row.right() {
                    break;
                }
                let color = Color32::from_rgb(c.color[0], c.color[1], c.color[2]);
                ui.painter().circle_filled(center, d / 2.0, color);
                ui.painter().circle_stroke(center, d / 2.0, Stroke::new(1.0, Color32::from_white_alpha(25)));
                let hit = ui.interact(Rect::from_center_size(center, Vec2::splat(d)), ui.scope_id().with(("swatch", n)), Sense::click());
                let hex = logic::hex(c.color);
                if hit.clicked() {
                    ui.ctx().copy_text(hex.clone());
                    self.flash(format!("Copied {hex}"));
                }
                hit.on_hover_text(format!("{hex}  ·  {}%", c.ratio));
            }
        }

        let mut changed: Option<Item> = None;
        let edit = self.edit.as_mut().unwrap();
        let r = field(ui, &mut edit.name, "Title", 33.0, false, "item-name");
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
        if r.lost_focus() && edit.url != item.url {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.url = edit.url.trim().to_string();
            changed = Some(it);
        }

        // Tags.
        heading(ui, "Tags");
        let mut remove_tag = None;
        let mut add_tag = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for tag in &item.tags {
                if chip(ui, tag, true).clicked() {
                    remove_tag = Some(tag.clone());
                }
            }
            let te = TextEdit::singleline(&mut edit.new_tag)
                .id_salt("new-tag")
                .hint_text(RichText::new("New tag").color(t::TEXT_MUTED))
                .font(font(13.0))
                .frame(egui::Frame::NONE)
                .desired_width(90.0);
            let r = ui.add(te);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !edit.new_tag.trim().is_empty() {
                add_tag = Some(edit.new_tag.trim().to_string());
                edit.new_tag.clear();
                r.request_focus();
            }
        });
        if let Some(tag) = remove_tag {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.tags.retain(|x| *x != tag);
            changed = Some(it);
        }
        if let Some(tag) = add_tag.clone()
            && !item.tags.contains(&tag)
        {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.tags.push(tag);
            changed = Some(it);
        }

        // Folders.
        heading(ui, "Folders");
        let mut remove_folder = None;
        let mut add_folder = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for fid in &item.folders {
                let name = logic::find_folder(&lib.metadata.folders, fid).map_or_else(|| fid.clone(), |f| f.name.clone());
                if chip(ui, &name, true).clicked() {
                    remove_folder = Some(fid.clone());
                }
            }
            let add = ui.add(egui::Button::new(RichText::new(format!("{}  Add Category", ic::PLUS)).size(12.0).color(t::TEXT_MUTED)).frame(false));
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
        if let Some(fid) = remove_folder {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.folders.retain(|x| *x != fid);
            changed = Some(it);
        }
        if let Some(fid) = add_folder {
            let mut it = changed.clone().unwrap_or_else(|| item.clone());
            it.folders.push(fid);
            changed = Some(it);
        }

        divider(ui);
        heading(ui, "Properties");
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
        prop(ui, "Type", &item.ext.to_uppercase());
        prop(ui, "Date Imported", &logic::format_ms(item.modification_time));
        prop(ui, "Date Created", &logic::format_ms(item.btime));
        prop(ui, "Date Modified", &logic::format_ms(item.mtime));
        ui.add_space(4.0);
        if wide_button(ui, ic::EXPORT, "Show in Finder", true).clicked() {
            let _ = std::process::Command::new("open").arg("-R").arg(item.original_path(lib.root())).spawn();
        }

        if let Some(it) = changed {
            match lib.update_item(it) {
                Ok(()) => {
                    if let Some(tag) = add_tag {
                        let _ = lib.record_tag_use(&[tag]);
                    }
                    if let Some(e) = self.edit.as_mut() {
                        e.name = lib.item(&item.id).map_or(e.name.clone(), |i| i.name.clone());
                    }
                }
                Err(e) => self.flash(format!("Could not save: {e}")),
            }
        }
    }
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, t::DIVIDER));
}

fn heading(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, text, font(13.0), t::TEXT_MUTED);
}

fn prop(ui: &mut Ui, key: &str, value: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
    let p = ui.painter();
    p.text(rect.left_center(), Align2::LEFT_CENTER, key, font(12.0), t::TEXT);
    let g = galley(ui, value, mono(12.0), t::TEXT_VALUE, rect.width() - 90.0);
    p.galley(Pos2::new(rect.left() + 90.0, rect.center().y - g.size().y / 2.0), g, t::TEXT_VALUE);
}

/// Rating row: five stars; clicking the current rating clears it.
fn rating(ui: &mut Ui, star: u64) -> Option<u64> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, "Rating", font(12.0), t::TEXT);
    let mut out = None;
    for n in 1..=5u64 {
        let c = Pos2::new(rect.left() + 90.0 + 7.0 + (n - 1) as f32 * 17.0, rect.center().y);
        let hit = ui.interact(Rect::from_center_size(c, Vec2::splat(16.0)), ui.scope_id().with(("star", n)), Sense::click());
        let on = n <= star;
        let fam = egui::FontId::new(13.0, t::fill_family());
        let color = if on { Color32::from_rgb(0xff, 0xd6, 0x0a) } else if hit.hovered() { t::TEXT_VALUE } else { t::TEXT_DISABLED };
        ui.painter().text(c, Align2::CENTER_CENTER, ic::STAR, fam, color);
        if hit.clicked() {
            out = Some(if star == n { 0 } else { n });
        }
    }
    out
}

fn chip(ui: &mut Ui, text: &str, removable: bool) -> Response {
    let g = galley(ui, text, font(12.0), t::TEXT, 180.0);
    let extra = if removable { 14.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0 + extra, 22.0), Sense::click());
    let fill = if resp.hovered() { t::BUTTON_HOVER } else { t::BUTTON };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().galley(Pos2::new(rect.left() + 8.0, rect.center().y - g.size().y / 2.0), g, t::TEXT);
    if removable && resp.hovered() {
        icon(ui, Pos2::new(rect.right() - 10.0, rect.center().y), ic::X, 10.0, t::TEXT_MUTED);
    }
    resp.on_hover_text(if removable { "Click to remove" } else { "" })
}

fn wide_button(ui: &mut Ui, glyph: &str, label: &str, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let fill = if enabled && resp.hovered() { t::BUTTON_HOVER } else { t::BUTTON };
    ui.painter().rect_filled(rect, t::RADIUS, fill);
    let color = if enabled { t::TEXT } else { t::TEXT_MUTED };
    let g = galley(ui, label, font(13.0), color, rect.width());
    let total = 22.0 + g.size().x;
    let x = rect.center().x - total / 2.0;
    icon(ui, Pos2::new(x + 8.0, rect.center().y), glyph, 16.0, color);
    ui.painter().galley(Pos2::new(x + 22.0, rect.center().y - g.size().y / 2.0), g, color);
    resp
}

// ---------------------------------------------------------------------------
// Viewer and frame

impl App {
    fn viewer(&mut self, ctx: &egui::Context, ui: &mut Ui, lib: &Library) {
        let Some(id) = self.viewer.clone() else { return };
        let Some(item) = lib.item(&id) else {
            self.viewer = None;
            return;
        };
        let screen = ui.max_rect();
        egui::Area::new(egui::Id::unique("viewer")).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
            let p = ui.painter();
            p.rect_filled(screen, 0.0, t::MAIN);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            window_drag(ui, bar, "viewer");
            let name = format!("{}.{}", item.name, item.ext);
            p.text(bar.center(), Align2::CENTER_CENTER, &name, font(14.0), t::TEXT);
            if icon_button(ui, Pos2::new(bar.right() - 24.0, bar.center().y), ic::X, 17.0, true, "Close").clicked() {
                self.viewer = None;
            }
            let area = Rect::from_min_max(Pos2::new(screen.left() + 40.0, bar.bottom() + 10.0), screen.max - vec2(40.0, 40.0));
            let tex = self.loader.get(&id, Tier::Full).cloned();
            let tex = tex.or_else(|| self.loader.get(&id, Tier::Preview).cloned());
            match tex {
                Some(tex) => {
                    let ts = if item.width > 0 { vec2(item.width as f32, item.height as f32) } else { tex.size_vec2() };
                    let scale = (area.width() / ts.x).min(area.height() / ts.y).min(1.0);
                    let r = Rect::from_center_size(area.center(), ts * scale);
                    egui::Image::new((tex.id(), r.size())).paint_at(ui, r);
                }
                None => ext_placeholder(ui, area, &item.ext),
            }
            if let Some(j) = job_for(item, lib.root(), Tier::Full) {
                self.frame_jobs.push(j);
            }
        });
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let r = ui.max_rect();
        window_drag(ui, Rect::from_min_size(r.min, vec2(r.width(), t::BAR_HEIGHT)), "empty");
        ui.painter().rect_filled(r, 0.0, t::MAIN);
        let c = r.center();
        ui.painter().text(c - vec2(0.0, 40.0), Align2::CENTER_CENTER, "Open an Eagle library", font(18.0), t::TEXT);
        if let Some(e) = &self.error {
            ui.painter().text(c - vec2(0.0, 14.0), Align2::CENTER_CENTER, e, font(12.0), t::TEXT_MUTED);
        }
        let b = Rect::from_center_size(c + vec2(0.0, 24.0), vec2(180.0, 32.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(b));
        if wide_button(&mut child, ic::FOLDER_OPEN, "Choose Library…", true).clicked() {
            self.pick_library();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.cumulative_frame_nr() == 0 {
            // Come to the front on launch, like any Mac app.
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.loader.poll(&ctx);

        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.import(dropped);
        }
        self.keyboard(&ctx);

        let Some(mut lib) = self.lib.take() else {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.empty_state(ui));
            return;
        };

        if self.show_sidebar {
            egui::Panel::left("sidebar")
                .exact_size(t::SIDEBAR_WIDTH)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.sidebar(ui, &lib));
        }
        if self.show_inspector {
            egui::Panel::right("inspector")
                .exact_size(t::INSPECTOR_WIDTH)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.inspector(ui, &mut lib));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t::MAIN)).show(ui, |ui| {
            let full = ui.max_rect();
            let bar = Rect::from_min_size(full.min, vec2(full.width(), t::BAR_HEIGHT));
            let mut top = ui.new_child(egui::UiBuilder::new().max_rect(bar));
            self.toolbar(&mut top, &lib);
            if !self.show_inspector {
                let c = Pos2::new(bar.right() - 24.0, bar.center().y);
                if icon_button(&mut top, c, ic::SIDEBAR_SIMPLE, 18.0, true, "Show inspector").clicked() {
                    self.show_inspector = true;
                }
            }
            let grid = Rect::from_min_max(Pos2::new(full.left(), bar.bottom()), full.max);
            let mut g = ui.new_child(egui::UiBuilder::new().max_rect(grid));
            g.set_clip_rect(grid);
            self.grid(&mut g, &lib);

            if let Some((msg, at)) = &self.status {
                if at.elapsed().as_secs_f32() < 3.0 {
                    let gal = galley(ui, msg, font(13.0), t::TEXT, 400.0);
                    let r = Rect::from_center_size(Pos2::new(full.center().x, full.bottom() - 30.0), gal.size() + vec2(24.0, 14.0));
                    ui.painter().rect_filled(r, CornerRadius::same(8), t::ROW_SELECTED);
                    ui.painter().galley(r.min + vec2(12.0, 7.0), gal, t::TEXT);
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                } else {
                    self.status = None;
                }
            }
        });
        if self.viewer.is_some() {
            let mut v = ui.new_child(egui::UiBuilder::new().max_rect(ctx.content_rect()));
            self.viewer(&ctx, &mut v, &lib);
        }
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(&ctx)
            && let Some(pos) = ctx.pointer_latest_pos()
        {
            let text = if d.0.len() == 1 { "1 item".to_string() } else { format!("{} items", d.0.len()) };
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag-ghost")));
            let g = painter.layout_no_wrap(text, font(12.0), Color32::WHITE);
            let r = Rect::from_min_size(pos + vec2(12.0, 12.0), g.size() + vec2(16.0, 8.0));
            painter.rect_filled(r, CornerRadius::same(10), t::PRIMARY);
            painter.galley(r.min + vec2(8.0, 4.0), g, Color32::WHITE);
        }
        if let Some(id) = self.confirm_delete.clone() {
            let name = logic::find_folder(&lib.metadata.folders, &id).map_or_else(String::new, |f| f.name.clone());
            let modal = egui::Modal::new(egui::Id::unique("confirm-delete")).show(&ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new(format!("Delete “{name}”?")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new("Its subfolders are deleted too. Items left in no folder move to the Trash.").color(t::TEXT_MUTED));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Delete").color(Color32::from_rgb(0xff, 0x66, 0x67))).clicked() {
                        self.actions.push(Action::DeleteFolder(id.clone()));
                        self.confirm_delete = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_delete = None;
                    }
                });
            });
            if modal.should_close() {
                self.confirm_delete = None;
            }
        }
        self.apply_actions(&mut lib);
        self.loader.want(std::mem::take(&mut self.frame_jobs));
        self.lib = Some(lib);
    }
}
