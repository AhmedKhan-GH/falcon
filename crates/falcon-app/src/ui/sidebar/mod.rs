//! The sidebar: the library switcher, the navigation rows, Quick Access and the folder and smart folder trees.

pub(crate) mod folders;
mod task_status;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use eframe::egui::{self, Align2, Color32, CornerRadius, Key, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, text::LayoutJob, vec2};
use falcon_library::model::Folder;
use falcon_library::smart::SmartFolder;
use falcon_library::Library;

use crate::app::{Action, App, Confirm, DragFolder, DragItems, DragQuick, Switch};
use crate::icons as ic;
use crate::logic::{self, NavCounts, Scope};
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley, icon, icon_button, window_drag};

/// The sidebar's rows for the arrow keys.
#[derive(Default)]
pub(crate) struct SidebarKeys {
    /// The views of the sidebar's visible rows this frame, top to bottom,
    /// for ↑ and ↓ while the sidebar has the keys.
    pub(crate) rows: Vec<Scope>,
    /// The row the keys last opened, and whether to scroll it into view.
    pub(crate) cursor: Option<usize>,
    pub(crate) reveal: bool,
}

pub(crate) struct SidebarCtx<'a> {
    pub(crate) folder_counts: &'a HashMap<String, usize>,
    pub(crate) nav: &'a NavCounts,
    /// Ids pinned to Quick Access.
    pub(crate) quick: &'a HashSet<String>,
}

/// Outlines a sidebar row while grid items are dragged over it (folders
/// dragged in the tree get insertion lines instead, see `ui/organize.rs`).
pub(crate) fn drop_highlight(ui: &Ui, resp: &Response, depth: usize) {
    if resp.dnd_hover_payload::<DragItems>().is_some() {
        let r = resp.rect;
        let bg = Rect::from_min_max(Pos2::new(r.left() + icon_x(depth) - 14.0, r.top() + 0.5), Pos2::new(r.right() - 11.0, r.bottom() - 0.5));
        ui.painter().rect_filled(bg, t::RADIUS, t::p().primary.gamma_multiply(0.25));
        ui.painter().rect_stroke(bg, t::RADIUS, Stroke::new(1.5, t::p().primary), StrokeKind::Inside);
    }
}

pub(crate) fn subtree_matches(f: &Folder, filter: &str) -> bool {
    f.name.to_lowercase().contains(filter) || f.children.iter().any(|c| subtree_matches(c, filter))
}

/// Horizontal centre of a row's icon at `depth`.
pub(crate) fn icon_x(depth: usize) -> f32 {
    29.0 + 20.0 * depth as f32
}

pub(crate) fn caret_rect(row: Rect, depth: usize) -> Rect {
    Rect::from_center_size(Pos2::new(row.left() + icon_x(depth) - 19.0, row.center().y), vec2(16.0, row.height()))
}

pub(crate) fn smart_name(lib: &Library, id: &str) -> String {
    let mut name = String::from("Smart Folder");
    for sf in lib.smart_folders() {
        sf.walk(&mut Vec::new(), &mut |f, _| {
            if f.id == id {
                name = f.name.clone();
            }
        });
    }
    name
}

/// A section header with a + button that shows on hover, like Eagle's.
/// A sidebar section heading. It senses clicks so it can carry a menu.
/// A caret after the title shows whether the section is `folded`.
pub(crate) fn section(ui: &mut Ui, title: &str, folded: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::click());
    let text = ui.painter().text(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        font(13.0),
        t::p().text_muted,
    );
    let caret = if folded { ic::CARET_RIGHT } else { ic::CARET_DOWN };
    ui.painter().text(Pos2::new(text.right() + 5.0, rect.center().y), Align2::LEFT_CENTER, caret, font(11.0), t::p().text_muted);
    resp
}

/// Which sidebar sections are folded away to their headers.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(crate) struct SectionFold {
    pub quick: bool,
    pub smart: bool,
    pub folders: bool,
}

/// How many smart folders (groups and nested ones included) are in `roots`.
pub(crate) fn smart_count(roots: &[SmartFolder]) -> usize {
    roots.iter().map(|s| 1 + smart_count(&s.children)).sum()
}

/// Whether a smart folder or any of its descendants has `filter` (already
/// lowercased) in its name.
pub(crate) fn smart_subtree_matches(s: &SmartFolder, filter: &str) -> bool {
    s.name.to_lowercase().contains(filter) || s.children.iter().any(|c| smart_subtree_matches(c, filter))
}

/// A heading's title with its count, shown only when there is something.
fn counted(title: &str, n: usize) -> String {
    if n > 0 { format!("{title} ({n})") } else { title.to_string() }
}

pub(crate) fn sidebar_row(
    ui: &mut Ui,
    depth: usize,
    glyph: &str,
    label: &str,
    count: Option<usize>,
    active: bool,
    caret: Option<bool>,
) -> Response {
    sidebar_row_tinted(ui, depth, glyph, t::p().text, label, count, active, caret)
}

/// A sidebar row whose icon is drawn in `tint` (a folder's icon colour).
#[allow(clippy::too_many_arguments)]
pub(crate) fn sidebar_row_tinted(
    ui: &mut Ui,
    depth: usize,
    glyph: &str,
    tint: Color32,
    label: &str,
    count: Option<usize>,
    active: bool,
    caret: Option<bool>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t::ROW_HEIGHT), Sense::click_and_drag());
    let ix = rect.left() + icon_x(depth);
    let bg = Rect::from_min_max(Pos2::new(ix - 14.0, rect.top() + 0.5), Pos2::new(rect.right() - 11.0, rect.bottom() - 0.5));
    let p = ui.painter();
    if active {
        p.rect_filled(bg, t::RADIUS, t::p().row_selected);
    } else if resp.hovered() {
        p.rect_filled(bg, t::RADIUS, t::p().row_hover);
    }
    if let Some(open) = caret {
        // A small solid triangle, pointing down when open.
        let c = caret_rect(rect, depth).center();
        let pts = if open {
            vec![c + vec2(-4.5, -2.5), c + vec2(4.5, -2.5), c + vec2(0.0, 3.0)]
        } else {
            vec![c + vec2(-2.5, -4.5), c + vec2(3.0, 0.0), c + vec2(-2.5, 4.5)]
        };
        p.add(egui::Shape::convex_polygon(pts, t::p().text_muted, Stroke::NONE));
    }
    let size = if crate::folder_icons::is_text(glyph) { 11.0 } else { 18.0 };
    p.text(Pos2::new(ix, rect.center().y), Align2::CENTER_CENTER, glyph, font(size), tint);
    let count_w = if count.is_some() { 30.0 } else { 0.0 };
    let g = galley(ui, label, font(13.5), t::p().text, rect.right() - 20.0 - count_w - (ix + 15.0));
    p.galley(Pos2::new(ix + 15.0, rect.center().y - g.size().y / 2.0), g, t::p().text);
    if let Some(n) = count {
        p.text(Pos2::new(rect.right() - 19.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), font(12.0), t::p().text_muted);
    }
    resp
}

/// A switcher row: the library's name over its folder, dimmed.
pub(crate) fn library_label(path: &std::path::Path, dim: bool) -> LayoutJob {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = path.parent().map(crate::workspaces::tilde).unwrap_or_default();
    let mut job = LayoutJob::default();
    let main = if dim { t::p().text_muted } else { t::p().text };
    job.append(&name, 0.0, egui::TextFormat::simple(font(13.0), main));
    job.append(&format!("\n{dir}"), 0.0, egui::TextFormat::simple(font(11.0), t::p().text_muted));
    job
}

/// Paints a library icon filling `rect` (a centred square crop of the image)
/// with rounded corners.
pub(crate) fn paint_library_icon(ui: &Ui, rect: Rect, tex: &egui::TextureHandle, radius: u8) {
    let [w, h] = tex.size();
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let side = w.min(h);
    let (ux, uy) = ((1.0 - side / w) / 2.0, (1.0 - side / h) / 2.0);
    let uv = Rect::from_min_max(Pos2::new(ux, uy), Pos2::new(1.0 - ux, 1.0 - uy));
    egui::Image::from_texture(tex).uv(uv).corner_radius(radius).paint_at(ui, rect);
}

/// Whether smart folder `f` or one below it has id `id`.
pub(crate) fn smart_has(f: &falcon_library::smart::SmartFolder, id: &str) -> bool {
    f.id == id || f.children.iter().any(|c| smart_has(c, id))
}

impl App {
    /// The small icon at the left of a switcher row: the library's
    /// `icon.png`, or a folder glyph (a warning for a missing folder).
    pub(crate) fn switcher_icon(&mut self, ui: &mut Ui, root: &std::path::Path, missing: bool) {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        if missing {
            icon(ui, rect.center(), ic::WARNING, 16.0, t::p().text_muted);
        } else if let Some(tex) = self.lib_icons.get(ui.ctx(), root) {
            paint_library_icon(ui, rect, &tex, 4);
        } else {
            icon(ui, rect.center(), ic::FOLDER, 16.0, t::p().text_muted);
        }
    }

    /// The menu for a library in the switcher (right-click or the ⋯ button).
    pub(crate) fn library_menu(&mut self, ui: &mut Ui, root: &std::path::Path, missing: bool) {
        if !missing {
            if ui.button("Change Icon…").clicked() {
                ui.close();
                self.change_library_icon(root);
            }
            if falcon_library::library_icon::icon_path(root).is_some() {
                ui.menu_button("Remove Icon", |ui| {
                    ui.label("Remove this library's icon?");
                    if ui.button("Remove").clicked() {
                        ui.close();
                        self.remove_library_icon(root);
                    }
                    if ui.button("Cancel").clicked() {
                        ui.close();
                    }
                });
            }
        }
        if ui.add_enabled(!missing, egui::Button::new("Show in Finder")).clicked() {
            ui.close();
            let _ = std::process::Command::new("open").arg("-R").arg(root).spawn();
        }
        ui.separator();
        if ui.button("Remove from List").clicked() {
            ui.close();
            crate::workspaces::remove_from_list(&mut self.recent, root);
        }
    }

    /// Asks for an image and makes it the library's icon.
    pub(crate) fn change_library_icon(&mut self, root: &std::path::Path) {
        let Some(src) = rfd::FileDialog::new().set_title("Choose Library Icon").add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif"]).pick_file() else {
            return;
        };
        let done = falcon_library::sandbox::guard(root).and_then(|()| falcon_library::library_icon::set_icon(root, &src));
        self.lib_icons.forget(root);
        if let Err(e) = done {
            self.flash(format!("Couldn't change the icon: {e}"));
        }
    }

    pub(crate) fn remove_library_icon(&mut self, root: &std::path::Path) {
        let done = falcon_library::sandbox::guard(root).and_then(|()| falcon_library::library_icon::remove_icon(root));
        self.lib_icons.forget(root);
        if let Err(e) = done {
            self.flash(format!("Couldn't remove the icon: {e}"));
        }
    }

    pub(crate) fn sidebar(&mut self, ui: &mut Ui, lib: &Library) {
        let full = ui.max_rect();
        self.imports.sidebar = Some(full);
        self.imports.folder_rows.clear();
        self.side.rows.clear();
        self.ws.folders.begin_rows();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::p().panel);
        p.vline(full.right() - 0.5, full.y_range(), Stroke::new(1.0, t::p().divider));

        // Header: traffic lights on the left, actions on the right.
        let header = Rect::from_min_size(full.min, vec2(full.width(), t::BAR_HEIGHT));
        window_drag(ui, header, "sidebar");
        let cy = header.center().y;
        let r = full.right();
        if icon_button(ui, Pos2::new(r - 24.0, cy), ic::SIDEBAR_SIMPLE, 18.0, true, "Hide sidebar").clicked() {
            self.show_sidebar = false;
        }
        // Go to Folder opens the quick switcher, as Eagle's header button
        // does; the library name below switches libraries.
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::QuickSwitchKey);
        let tip = if keys.is_empty() { "Go to Folder".to_owned() } else { format!("Go to Folder  {keys}") };
        if icon_button(ui, Pos2::new(r - 51.0, cy), ic::ARROWS_LEFT_RIGHT, 18.0, true, &tip).clicked() {
            self.quick.toggle();
        }
        let add_tip = self.shortcuts.tooltip(ui.ctx(), crate::locale::tr("Add"), crate::shortcuts::Command::NewMenu);
        let add = icon_button(ui, Pos2::new(r - 79.0, cy), ic::PLUS, 18.0, true, &add_tip);
        if add.clicked() {
            self.toggle_new_menu(ui.ctx());
        }
        self.new_menu(ui, Some(add.rect), add.rect.left_bottom());

        // Library switcher.
        let lib_row = Rect::from_min_size(Pos2::new(full.left(), header.bottom()), vec2(full.width(), 30.0));
        let resp = ui.interact(lib_row, ui.scope_id().with("library"), Sense::click());
        let ic_rect = Rect::from_center_size(Pos2::new(full.left() + 29.0, lib_row.center().y), Vec2::splat(22.0));
        match self.lib_icons.get(ui.ctx(), lib.root()) {
            Some(tex) => paint_library_icon(ui, ic_rect, &tex, 5),
            None => {
                p.rect_filled(ic_rect, CornerRadius::same(5), t::p().brand);
                icon(ui, ic_rect.center(), ic::FOLDER, 14.0, Color32::WHITE);
            }
        }
        let name = lib.root().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let g = galley(ui, &name, font(15.0), t::p().text, full.width() - 80.0);
        let gw = g.size().x;
        p.galley(Pos2::new(full.left() + 44.0, lib_row.center().y - g.size().y / 2.0), g, t::p().text);
        icon(ui, Pos2::new(full.left() + 44.0 + gw + 10.0, lib_row.center().y), ic::CARET_UP_DOWN, 12.0, t::p().text);
        self.task_status(ui, Pos2::new(full.left() + 44.0 + gw + 32.0, lib_row.center().y));
        let current = lib.root().to_path_buf();
        if std::mem::take(&mut self.open_switcher) {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&resp));
        }
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(260.0);
            ui.label(RichText::new("Open").color(t::p().text_muted).size(12.0));
            let open: Vec<PathBuf> = self.others.iter(&self.ws).map(|w| w.root.clone().unwrap_or_default()).collect();
            let slot = self.others.slot();
            for (i, p) in open.iter().enumerate() {
                ui.horizontal(|ui| {
                    self.switcher_icon(ui, p, false);
                    let row = ui.add(egui::Button::selectable(i == slot, library_label(p, false)).min_size(vec2(200.0, 0.0)));
                    row.context_menu(|ui| self.library_menu(ui, p, false));
                    if row.on_hover_text(p.display().to_string()).clicked() && i != slot {
                        self.switch_to = Some(Switch::Focus(i));
                    }
                    ui.menu_button(ic::DOTS_THREE, |ui| self.library_menu(ui, p, false)).response.on_hover_text("More");
                    if ui.small_button(ic::X).on_hover_text("Close Library").clicked() {
                        self.switch_to = Some(Switch::Close(i));
                    }
                });
            }
            let recent = crate::workspaces::not_open(&self.recent, &open);
            if !recent.is_empty() {
                ui.separator();
                ui.label(RichText::new("Recent").color(t::p().text_muted).size(12.0));
                for p in recent {
                    let missing = !p.is_dir();
                    ui.horizontal(|ui| {
                        self.switcher_icon(ui, &p, missing);
                        let row = ui.add(egui::Button::new(library_label(&p, missing)).min_size(vec2(200.0, 0.0)));
                        row.context_menu(|ui| self.library_menu(ui, &p, missing));
                        let tip = if missing { format!("The folder can't be found: {}", p.display()) } else { p.display().to_string() };
                        if row.on_hover_text(tip).clicked() {
                            self.switch_to = Some(Switch::Open(p.clone()));
                        }
                        ui.menu_button(ic::DOTS_THREE, |ui| self.library_menu(ui, &p, missing)).response.on_hover_text("More");
                    });
                }
                if ui.button("Clear History…").clicked() {
                    ui.close();
                    self.clear_history_open = true;
                }
            }
            ui.separator();
            if ui.button("Open Library…").clicked() {
                self.switch_to = Some(Switch::Pick);
            }
            if ui.button("New Library…").clicked() {
                self.switch_to = Some(Switch::Create);
            }
            if ui.button("Merge Another Library…").clicked() {
                self.switch_to = Some(Switch::Merge);
            }
            if ui.button("Empty Cache and Reload…").clicked() {
                self.confirm_reload = true;
            }
            if ui.button("Show in Finder").clicked() {
                let _ = std::process::Command::new("open").arg("-R").arg(&current).spawn();
            }
            ui.separator();
            if ui.button("Import Files…").clicked() {
                self.import_files_dialog();
            }
            if ui.button("Import Folders…").clicked() {
                self.import_folders_dialog();
            }
            if ui.button("Import Eagle Package…").clicked() {
                self.import_pack_dialog();
            }
            if ui.button("Import Inboard Backup…").clicked() {
                self.import_inboard_dialog();
            }
            ui.separator();
            if ui.button(if self.collect.open { "Hide Collect Window" } else { "Show Collect Window" }).clicked() {
                self.collect.toggle();
            }
            if ui.button("Preferences…").clicked() {
                self.open_preferences();
            }
        });

        // Filter box at the bottom.
        let filter_rect = Rect::from_min_max(
            Pos2::new(full.left() + 11.0, full.bottom() - 37.0),
            Pos2::new(full.right() - 11.0, full.bottom() - 11.0),
        );

        let list_rect = Rect::from_min_max(Pos2::new(full.left(), lib_row.bottom() + 6.0), Pos2::new(full.right() - 1.0, filter_rect.top() - 8.0));
        let folder_counts = logic::folder_counts(lib.items());
        let nav = logic::nav_counts(lib.items(), &lib.metadata.folders);
        let quick: HashSet<String> =
            lib.metadata.quick_access.iter().filter_map(|q| q.get("id").and_then(|v| v.as_str()).map(String::from)).collect();
        let cx = SidebarCtx { folder_counts: &folder_counts, nav: &nav, quick: &quick };
        let mut list = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
        egui::ScrollArea::vertical().id_salt("sidebar-list").auto_shrink([false, false]).show(&mut list, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.nav_rows(ui, &cx);
            ui.add_space(12.0);
            let shown = self.prefs.sidebar.clone();
            if shown.quick_access && !lib.metadata.quick_access.is_empty() {
                let head = section(ui, &format!("Quick Access ({})", lib.metadata.quick_access.len()), self.sections.quick);
                if head.clicked() {
                    self.sections.quick = !self.sections.quick;
                }
                let folded = self.sections.quick;
                let smart = lib.smart_folders();
                for q in lib.metadata.quick_access.iter().filter(|_| !folded) {
                    let (Some(kind), Some(id)) = (q.get("type").and_then(|v| v.as_str()), q.get("id").and_then(|v| v.as_str())) else {
                        continue;
                    };
                    let (glyph, name, scope, count) = if kind == "smartFolder" {
                        let Some(sf) = crate::organize::find(&smart, id) else { continue };
                        let count = if sf.is_group() { 0 } else { lib.smart_folder_items(id).len() };
                        (crate::ui::organize::smart_look(sf), sf.name.clone(), Scope::Smart(id.to_string()), count)
                    } else {
                        let Some(f) = logic::find_folder(&lib.metadata.folders, id) else { continue };
                        (ic::FOLDER, f.name.clone(), Scope::Folder(id.to_string()), cx.folder_counts.get(id).copied().unwrap_or(0))
                    };
                    let active = self.ws.scope == scope;
                    let resp = sidebar_row(ui, 0, glyph, &name, self.badge(count), active, None);
                    self.side_row(scope.clone(), &resp);
                    if kind != "smartFolder" {
                        self.imports.note_row(ui, resp.rect, id);
                    }
                    if resp.drag_started() {
                        resp.dnd_set_drag_payload(DragQuick(id.to_string()));
                    }
                    let kind: &'static str = if kind == "smartFolder" { "smartFolder" } else { "folder" };
                    self.quick_drop(ui, lib, &resp, id);
                    if kind == "folder" {
                        drop_highlight(ui, &resp, 0);
                        if let Some(d) = resp.dnd_release_payload::<DragItems>()
                            && let Some(op) = logic::folder_drop(&self.ws.scope, id, ui.input(|i| i.modifiers.alt))
                        {
                            self.push_bulk(d.0.len(), Action::DropOnFolder(d.0.clone(), id.to_string(), op));
                        }
                    }
                    if resp.clicked() {
                        self.go(scope);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Remove from Quick Access").clicked() {
                            self.ws.actions.push(Action::QuickRemove(id.to_string()));
                        }
                    });
                }
                ui.add_space(8.0);
            }
            let filter = self.ws.folder_filter.trim().to_lowercase();
            if shown.smart_folders {
                let smart = lib.smart_folders();
                if filter.is_empty() || smart.iter().any(|s| smart_subtree_matches(s, &filter)) {
                    let title = counted("Smart Folders", smart_count(&smart));
                    if self.smart_section(ui, &title, self.sections.smart) {
                        self.sections.smart = !self.sections.smart;
                    }
                    if !self.sections.smart {
                        for sf in &smart {
                            self.smart_row(ui, lib, &smart, sf, 0, &quick, &filter);
                        }
                    }
                }
            }
            if !shown.folders {
                return;
            }
            if !filter.is_empty() && !lib.metadata.folders.iter().any(|f| subtree_matches(f, &filter)) {
                return;
            }
            let total = lib.all_folders().len();
            let header = section(ui, &counted("Folders", total), self.sections.folders);
            if header.clicked() {
                self.sections.folders = !self.sections.folders;
            }
            if header.dnd_hover_payload::<DragFolder>().is_some() {
                ui.painter().rect_stroke(header.rect.shrink2(vec2(11.0, 1.0)), t::RADIUS, Stroke::new(1.5, t::p().primary), StrokeKind::Inside);
            }
            if let Some(d) = header.dnd_release_payload::<DragFolder>() {
                self.ws.actions.push(Action::MoveFolder(d.0.clone(), None));
            }
            header.context_menu(|ui| {
                self.folder_title_sort_menu(ui, None);
            });
            if !self.sections.folders {
                for f in &lib.metadata.folders {
                    self.folder_row(ui, lib, f, 0, &cx, &filter);
                }
            }
            ui.add_space(8.0);
        });

        p.rect_filled(filter_rect, t::RADIUS, t::p().field);
        p.rect_stroke(filter_rect, t::RADIUS, Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
        icon(ui, Pos2::new(filter_rect.left() + 15.0, filter_rect.center().y), ic::FUNNEL, 15.0, t::p().text_muted);
        let inner = Rect::from_min_max(Pos2::new(filter_rect.left() + 30.0, filter_rect.top()), filter_rect.max);
        let filter_resp = ui.put(
            inner,
            TextEdit::singleline(&mut self.ws.folder_filter)
                .id_salt("folder-filter")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new("Filter").color(t::p().text_muted))
                .font(font(13.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width() - 8.0),
        );
        if std::mem::take(&mut self.focus_folder_filter) {
            filter_resp.request_focus();
        }
    }

    /// Notes a visible sidebar row for the arrow keys, and scrolls it into
    /// view when the keys just opened it.
    pub(crate) fn side_row(&mut self, scope: Scope, resp: &Response) {
        if self.side.reveal && self.side.cursor == Some(self.side.rows.len()) {
            resp.scroll_to_me(None);
            self.side.reveal = false;
        }
        self.side.rows.push(scope);
    }

    /// The count shown beside a sidebar row: none when zero, or when
    /// Preferences turns counts off.
    pub(crate) fn badge(&self, count: usize) -> Option<usize> {
        (count > 0 && self.prefs.sidebar_counts).then_some(count)
    }

    pub(crate) fn nav_rows(&mut self, ui: &mut Ui, cx: &SidebarCtx) {
        let n = cx.nav;
        let entries: [(&str, &str, Scope, usize); 8] = [
            (ic::IMAGES, "All", Scope::All, n.all),
            (ic::FOLDER_DASHED, "Uncategorized", Scope::Uncategorized, n.uncategorized),
            (ic::TAG_SIMPLE, "Untagged", Scope::Untagged, n.untagged),
            (ic::CLOCK, "Recently Used", Scope::Recent, 0),
            (ic::SHUFFLE, "Random", Scope::Random(0), 0),
            (ic::BOOKMARK_SIMPLE, "All Tags", Scope::AllTags, n.tags),
            (crate::ui::grid::duplicates::ICON, "Duplicates", Scope::Duplicates, 0),
            (ic::TRASH, "Trash", Scope::Trash, n.trash),
        ];
        for (glyph, label, scope, count) in entries {
            if !self.prefs.sidebar.shows(&scope) {
                continue;
            }
            let active = match (&scope, &self.ws.scope) {
                (Scope::Random(_), Scope::Random(_)) => true,
                (a, b) => a == b,
            };
            let resp = sidebar_row(ui, 0, glyph, label, self.badge(count), active, None);
            self.side_row(scope.clone(), &resp);
            if scope == Scope::Recent {
                resp.context_menu(|ui| {
                    let root = self.ws.root.clone().unwrap_or_default();
                    let any = !self.recent_items.list(&root).is_empty();
                    if ui.add_enabled(any, egui::Button::new("Clear Recently Used")).clicked() {
                        self.recent_items.clear(&root);
                        self.recent_shown = None;
                        if self.ws.scope == Scope::Recent {
                            self.ws.selected.clear();
                        }
                    }
                });
            }
            if scope != Scope::Trash && scope != Scope::Recent {
                // As in Eagle, the entries' menu picks which of them show;
                // it stays open while you tick them. Trash and Recently Used
                // have menus of their own.
                egui::Popup::context_menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.set_min_width(180.0);
                    let mut all = true;
                    ui.add_enabled(false, egui::Checkbox::new(&mut all, "All"));
                    let s = &mut self.prefs.sidebar;
                    ui.checkbox(&mut s.uncategorized, "Uncategorized");
                    ui.checkbox(&mut s.untagged, "Untagged");
                    ui.checkbox(&mut s.recent, "Recently Used");
                    ui.checkbox(&mut s.random, "Random");
                });
            }
            if scope == Scope::Trash {
                resp.context_menu(|ui| {
                    if ui.add_enabled(n.trash > 0, egui::Button::new("Restore All Items")).clicked() {
                        self.ws.confirm = Some(Confirm::RestoreAll);
                    }
                    if ui.add_enabled(n.trash > 0, egui::Button::new("Empty Trash")).clicked() {
                        self.ws.confirm = Some(Confirm::EmptyTrash);
                    }
                });
                drop_highlight(ui, &resp, 0);
                if let Some(d) = resp.dnd_release_payload::<DragItems>() {
                    self.ws.actions.push(Action::Trash(d.0.clone(), true));
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
    pub(crate) fn folder_row(&mut self, ui: &mut Ui, lib: &Library, f: &Folder, depth: usize, cx: &SidebarCtx, filter: &str) -> Option<Rect> {
        if !filter.is_empty() && !subtree_matches(f, filter) {
            return None;
        }
        // A locked folder keeps its subfolders hidden until it is unlocked.
        let shut = crate::organize::is_shut(f, &self.ws.org.unlocked);
        let has_children = !f.children.is_empty() && !shut;
        let open = has_children && (!filter.is_empty() || !self.ws.collapsed.contains(&f.id));
        let active = self.tree_active(crate::ui::sidebar::folders::Tree::Folders, &f.id);
        self.ws.folders.row(crate::ui::sidebar::folders::Tree::Folders, &f.id);
        let count = cx.folder_counts.get(&f.id).copied().filter(|_| !shut && self.prefs.sidebar_counts);
        let (glyph, tint) = App::folder_look(f, open);
        let renaming = self.ws.renaming.as_ref().is_some_and(|(id, _)| *id == f.id);
        let label = if renaming { "" } else { f.name.as_str() };
        let resp = sidebar_row_tinted(ui, depth, glyph, tint, label, count, active, has_children.then_some(open));
        self.side_row(Scope::Folder(f.id.clone()), &resp);
        let row_rect = resp.rect;
        self.imports.note_row(ui, row_rect, &f.id);
        if f.is_locked() && !renaming {
            crate::ui::organize::lock_badge(ui, row_rect, shut, count.is_some());
        }
        if renaming {
            let r = Rect::from_min_max(
                Pos2::new(row_rect.left() + icon_x(depth) + 12.0, row_rect.top() + 3.0),
                Pos2::new(row_rect.right() - 14.0, row_rect.bottom() - 3.0),
            );
            ui.painter().rect_filled(r, CornerRadius::same(4), t::p().field);
            ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
            let (_, buf) = self.ws.renaming.as_mut().unwrap();
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
                let (id, name) = self.ws.renaming.take().unwrap();
                let name = name.trim().to_string();
                if !cancel && !name.is_empty() && name != f.name {
                    self.ws.actions.push(Action::RenameFolder(id, name));
                }
            }
        }
        if resp.drag_started() {
            resp.dnd_set_drag_payload(DragFolder(f.id.clone()));
        }
        drop_highlight(ui, &resp, depth);
        if let Some(d) = resp.dnd_release_payload::<DragItems>()
            && let Some(op) = logic::folder_drop(&self.ws.scope, &f.id, ui.input(|i| i.modifiers.alt))
        {
            self.push_bulk(d.0.len(), Action::DropOnFolder(d.0.clone(), f.id.clone(), op));
        }
        self.folder_drop(ui, lib, &resp, f, depth);
        let pinned = cx.quick.contains(&f.id);
        let on_caret = has_children && resp.interact_pointer_pos().is_some_and(|p| caret_rect(row_rect, depth).contains(p));
        let caret_menu = Self::caret_menu(ui, &resp, on_caret);
        resp.context_menu(|ui| {
            if caret_menu {
                self.expand_menu(ui, &lib.metadata.folders, &f.id);
                return;
            }
            // The caret's Expand menu has no search box, as in Eagle.
            crate::ui::menus::search::show(ui, |ui| {
            if self.ws.folders.sel.is_multi() && self.ws.folders.sel.contains(&f.id) {
                self.multi_folder_menu(ui, lib, f, pinned);
                return;
            }
            ui.set_min_width(180.0);
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::NewFolder);
            if ui.add(egui::Button::new("New Folder").shortcut_text(keys)).on_hover_text("A new folder right below this one").clicked() {
                self.ws.actions.push(Action::CreateSibling(f.id.clone()));
            }
            if ui.add_enabled(!shut, egui::Button::new("New Subfolder")).clicked() {
                self.ws.actions.push(Action::CreateFolder { parent: Some(f.id.clone()) });
            }
            if ui.button("Move Folder…").clicked() {
                self.open_mover(vec![f.id.clone()]);
            }
            ui.separator();
            if pinned {
                if ui.button("Remove from Quick Access").clicked() {
                    self.ws.actions.push(Action::QuickRemove(f.id.clone()));
                }
            } else if ui.button("Add to Quick Access").clicked() {
                self.ws.actions.push(Action::QuickAdd("folder", f.id.clone()));
            }
            if depth > 0 && ui.button("Move to Top Level").clicked() {
                self.ws.actions.push(Action::MoveFolder(f.id.clone(), None));
            }
            ui.separator();
            if ui.button("Rename").clicked() {
                self.ws.renaming = Some((f.id.clone(), f.name.clone()));
            }
            if ui.button("Copy Link").clicked() {
                ui.ctx().copy_text(crate::links::api_folder_link(self.api_link_port(), &f.id));
                self.flash_brief("Link copied");
            }
            crate::ui::menus::search::submenu(ui, "Copy", |ui| {
                if ui.button("Eagle Link").clicked() {
                    ui.ctx().copy_text(crate::links::folder_link(&f.id));
                    self.flash_brief("Link copied");
                    ui.close();
                }
            });
            self.folder_menu_extras(ui, lib, f);
            ui.separator();
            self.expand_menu(ui, &lib.metadata.folders, &f.id);
            ui.separator();
            // Eagle disables Duplicate on a locked folder.
            if ui.add_enabled(!shut, egui::Button::new("Duplicate")).clicked() {
                self.ws.actions.push(Action::Folders(crate::ui::sidebar::folders::FolderAction::Duplicate(f.id.clone())));
            }
            self.tree_export_menu(ui, crate::ui::dialogs::export::Target::Folder(f.id.clone()), !shut);
            ui.separator();
            // The same switch as the View menu's (Eagle shows it checked).
            let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::ToggleSubfolders);
            let mark = if self.display.subfolders { crate::icons::CHECK } else { " " };
            if ui.add(egui::Button::new(format!("{mark}  Show Subfolder Content")).shortcut_text(keys)).clicked() {
                self.run_command(&ui.ctx().clone(), crate::shortcuts::Command::ToggleSubfolders);
            }
            ui.separator();
            if ui.add_enabled(!shut, egui::Button::new(RichText::new("Delete Folder").color(t::p().icon_red))).clicked() {
                self.ask_delete_folders(lib, vec![f.id.clone()]);
            }
            })
        });
        if resp.clicked() {
            let caret = caret_rect(row_rect, depth);
            if has_children && resp.interact_pointer_pos().is_some_and(|p| caret.contains(p)) {
                self.caret_click(ui, &lib.metadata.folders, &f.id);
            } else if !self.tree_click(crate::ui::sidebar::folders::Tree::Folders, &f.id, ui.input(|i| i.modifiers)) {
                self.go(Scope::Folder(f.id.clone()));
            }
        }
        if resp.double_clicked() {
            match self.prefs.sidebar_double_click {
                crate::prefs::SidebarDoubleClick::Rename => self.ws.renaming = Some((f.id.clone(), f.name.clone())),
                crate::prefs::SidebarDoubleClick::Collapse => {
                    if has_children && !self.ws.collapsed.remove(&f.id) {
                        self.ws.collapsed.insert(f.id.clone());
                    }
                }
            }
        }
        if open {
            let mut last_center = None;
            for c in &f.children {
                if let Some(r) = self.folder_row(ui, lib, c, depth + 1, cx, filter) {
                    last_center = Some(r.center().y);
                }
            }
            if let Some(end) = last_center {
                // Guide line from this folder's icon down to its last child.
                let x = row_rect.left() + icon_x(depth);
                let p = ui.painter();
                let top = row_rect.bottom() - 3.0;
                // Eagle tints a coloured folder's guide line in its colour.
                let guide = if f.icon_color.is_some() { tint.gamma_multiply(0.25) } else { t::p().guide };
                p.line_segment([Pos2::new(x, top), Pos2::new(x, end - 5.0)], Stroke::new(1.0, guide));
                p.line_segment([Pos2::new(x, end - 5.0), Pos2::new(x + 5.0, end)], Stroke::new(1.0, guide));
            }
        }
        Some(row_rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sf(name: &str, children: Vec<SmartFolder>) -> SmartFolder {
        SmartFolder {
            id: name.to_string(),
            name: name.to_string(),
            conditions: Vec::new(),
            children,
            icon: None,
            icon_color: None,
            description: String::new(),
            order_by: None,
            sort_increase: None,
        }
    }

    #[test]
    fn counts_nested_smart_folders_and_groups() {
        assert_eq!(smart_count(&[]), 0);
        let tree = vec![sf("A", vec![sf("B", vec![sf("C", vec![])])]), sf("D", vec![])];
        assert_eq!(smart_count(&tree), 4);
    }

    #[test]
    fn smart_filter_matches_descendants_case_insensitively() {
        let tree = sf("Work", vec![sf("Red Cars", vec![])]);
        assert!(smart_subtree_matches(&tree, "work"));
        assert!(smart_subtree_matches(&tree, "cars"));
        assert!(!smart_subtree_matches(&tree, "boats"));
    }

    #[test]
    fn heading_count_only_when_nonzero() {
        assert_eq!(counted("Folders", 0), "Folders");
        assert_eq!(counted("Folders", 3), "Folders (3)");
    }
}
