//! The collect window: a small always-on-top window that takes files
//! dropped on it (or pasted into it) and adds them to a chosen folder,
//! so items can be collected while the main window is behind other apps.
//! It is falcon's take on Eagle's collect window, which asks where to save
//! an item arriving from the browser extension or a screenshot.

use std::path::PathBuf;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, vec2};
use falcon_library::import::ImportOptions;
use falcon_library::model::Folder;

use crate::locale::{tr, tr_nf, trf};
use crate::shortcuts::{Command, Shortcuts};
use crate::theme::{self as t, font};

/// Size of the window, in points.
pub const SIZE: egui::Vec2 = vec2(300.0, 600.0);

/// A folder as the collect window lists it.
#[derive(Clone, PartialEq, Debug)]
pub struct FolderRow {
    pub id: String,
    pub name: String,
    /// Names from the top level down, joined by " / ".
    pub path: String,
    pub depth: usize,
}

/// Every folder in tree order.
pub fn flatten(folders: &[Folder]) -> Vec<FolderRow> {
    fn walk(fs: &[Folder], prefix: &str, depth: usize, out: &mut Vec<FolderRow>) {
        for f in fs {
            let path = if prefix.is_empty() { f.name.clone() } else { format!("{prefix} / {}", f.name) };
            out.push(FolderRow { id: f.id.clone(), name: f.name.clone(), path: path.clone(), depth });
            walk(&f.children, &path, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(folders, "", 0, &mut out);
    out
}

/// Whether a row matches the search text (any part of its path, ignoring
/// case).
pub fn matches(row: &FolderRow, filter: &str) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty() || row.path.to_lowercase().contains(&f)
}

/// How many recent folders are remembered.
pub const RECENT_MAX: usize = 20;
/// How many recent folders show, besides the selected ones.
pub const RECENT_SHOWN: usize = 5;
/// How many tag suggestions show.
pub const SUGGESTIONS: usize = 8;

/// Moves an old single `collect_folder` into the list of folders, once.
pub fn migrate(folders: &mut Vec<String>, old: &mut Option<String>) {
    if let Some(id) = old.take()
        && folders.is_empty()
    {
        folders.push(id);
    }
}

/// The chosen folders that still exist in this library, in the order
/// they were chosen.
pub fn target(rows: &[FolderRow], chosen: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in chosen {
        if rows.iter().any(|r| r.id == *id) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// A click on a folder: plain selects only it, ⌘-click toggles it.
pub fn toggle_folder(selected: &mut Vec<String>, id: &str, additive: bool) {
    if additive {
        if let Some(at) = selected.iter().position(|s| s == id) {
            selected.remove(at);
        } else {
            selected.push(id.to_string());
        }
    } else {
        selected.clear();
        selected.push(id.to_string());
    }
}

/// Puts the folders just collected into at the front of the recent list.
pub fn push_recent(recent: &mut Vec<String>, used: &[String]) {
    let mut next: Vec<String> = Vec::new();
    for id in used {
        if !next.contains(id) {
            next.push(id.clone());
        }
    }
    next.extend(recent.iter().filter(|r| !used.contains(r)).cloned());
    next.truncate(RECENT_MAX);
    *recent = next;
}

/// One line of the folder list.
#[derive(Clone, PartialEq, Debug)]
pub enum Line {
    /// The row that clears the selection (the library root).
    NoFolder,
    /// A folder in the recent block: shown by full path, at depth 0.
    Recent(FolderRow),
    Separator,
    /// A folder in the tree. While searching it shows its path at depth 0.
    Tree(FolderRow),
    /// Make a new top-level folder with this name.
    Create(String),
}

/// Which lines the list shows. `selected` and `recent` may name folders
/// that are gone; they are ignored.
pub fn lines(rows: &[FolderRow], selected: &[String], recent: &[String], filter: &str) -> Vec<Line> {
    let find = |id: &String| rows.iter().find(|r| r.id == *id);
    let searching = !filter.trim().is_empty();
    let chosen = target(rows, selected);
    // Selected folders first, then the recent ones in recent order.
    let mut block: Vec<&FolderRow> = chosen.iter().filter_map(find).collect();
    let chosen_n = block.len();
    let recents: Vec<&FolderRow> = recent.iter().filter(|id| !chosen.contains(id)).filter_map(find).collect();
    block.extend(recents);
    // The block is worth showing only when the tree has more than it.
    let worthwhile = !block.is_empty() && rows.len() > block.len().min(chosen_n + RECENT_SHOWN);
    let mut shown: Vec<&FolderRow> = if worthwhile { block } else { Vec::new() };
    shown.retain(|r| matches(r, filter));
    shown.truncate(chosen_n + RECENT_SHOWN);

    let mut out = Vec::new();
    if !searching {
        out.push(Line::NoFolder);
    }
    out.extend(shown.iter().map(|r| Line::Recent((*r).clone())));
    let rest: Vec<&FolderRow> = rows.iter().filter(|r| matches(r, filter) && !(searching && shown.iter().any(|s| s.id == r.id))).collect();
    if !shown.is_empty() && !rest.is_empty() {
        out.push(Line::Separator);
    }
    out.extend(rest.into_iter().map(|r| Line::Tree(r.clone())));
    let text = filter.trim();
    if searching && !rows.iter().any(|r| r.name == text) {
        out.push(Line::Create(text.to_string()));
    }
    out
}

/// The name to create when Enter is pressed: only when the create row is
/// the one thing the search found.
pub fn enter_creates(lines: &[Line]) -> Option<&str> {
    match lines {
        [Line::Create(name)] => Some(name),
        _ => None,
    }
}

/// Adds a typed tag unless it is empty or already there (any case).
pub fn add_tag(tags: &mut Vec<String>, raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() || tags.iter().any(|x| x.to_lowercase() == t.to_lowercase()) {
        return false;
    }
    tags.push(t.to_string());
    true
}

/// Tag suggestions for the typed text: starred tags first, then recent
/// ones, then every other tag; only those containing the text (any case)
/// and not already chosen.
pub fn suggest(typed: &str, starred: &[String], history: &[String], all: &[String], have: &[String]) -> Vec<String> {
    let q = typed.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for t in starred.iter().chain(history).chain(all) {
        let l = t.to_lowercase();
        if l.contains(&q) && !have.iter().any(|h| h.to_lowercase() == l) && !out.iter().any(|o| o.to_lowercase() == l) {
            out.push(t.clone());
        }
        if out.len() == SUGGESTIONS {
            break;
        }
    }
    out
}

/// The library's tags a suggestion can come from.
#[derive(Default, Clone)]
pub struct TagPool {
    pub starred: Vec<String>,
    pub history: Vec<String>,
    pub all: Vec<String>,
}

/// What the person typed above the folder list.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Meta {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
}

impl Meta {
    /// Puts the tags, description and (for a single file) name into the
    /// import options, keeping tags the options already have.
    pub fn apply(&self, opts: &mut ImportOptions, files: usize) {
        for t in &self.tags {
            add_tag(&mut opts.tags, t);
        }
        if !self.description.trim().is_empty() {
            opts.annotation = self.description.trim().to_string();
        }
        if files == 1 && !self.name.trim().is_empty() {
            opts.name = Some(self.name.trim().to_string());
        }
    }
}

/// What the window received this frame.
#[derive(PartialEq, Debug)]
pub enum Event {
    Files(Vec<PathBuf>),
    /// ⌘V: import what is on the clipboard.
    Paste,
    /// Make a top-level folder with this name.
    CreateFolder(String),
}

#[derive(Default)]
pub struct Collect {
    pub open: bool,
    /// The window's last position on screen, remembered between launches.
    pub pos: Option<Pos2>,
    /// Where it opened this time; kept fixed while open so moving the
    /// window by hand is not fought.
    open_at: Option<Pos2>,
    filter: String,
    status: Option<(String, f64)>,
    /// Name, description and tags for the next collect.
    pub meta: Meta,
    tag_input: String,
}

impl Collect {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.open_at = self.pos;
    }

    /// Clears name, description and tags after a collect.
    pub fn clear_meta(&mut self) {
        self.meta = Meta::default();
        self.tag_input.clear();
    }

    /// Shows the window as its own always-on-top, borderless OS window.
    /// Returns what was dropped or pasted on it.
    pub fn show(&mut self, ctx: &egui::Context, folders: Option<&[FolderRow]>, state: &mut State, pool: &TagPool, keys: &Shortcuts) -> Option<Event> {
        let mut builder = egui::ViewportBuilder::default()
            .with_title(tr("Collect"))
            .with_inner_size(SIZE)
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top();
        if let Some(p) = self.open_at {
            builder = builder.with_position(p);
        }
        let id = egui::ViewportId::from_hash_of("falcon-collect");
        ctx.show_viewport_immediate(id, builder, |ui, _class| self.contents(ui, folders, state, pool, keys))
    }

    fn contents(&mut self, ui: &mut Ui, folders: Option<&[FolderRow]>, state: &mut State, pool: &TagPool, keys: &Shortcuts) -> Option<Event> {
        let ctx = ui.ctx().clone();
        let (outer, close_requested, hovering, dropped, pasted, esc, toggle, now) = ctx.input(|i| {
            let dropped: Vec<PathBuf> = i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect();
            let pasted = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_)));
            (
                i.viewport().outer_rect,
                i.viewport().close_requested(),
                !i.raw.hovered_files.is_empty(),
                dropped,
                pasted,
                i.key_pressed(egui::Key::Escape),
                keys.pressed(i).contains(&Command::CollectWindow),
                i.time,
            )
        });
        if let Some(r) = outer {
            self.pos = Some(r.min);
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        if close_requested || toggle || (esc && !typing) {
            self.open = false;
        }

        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, t::p().panel);
        p.rect_stroke(full, 0.0, Stroke::new(1.0, t::p().divider), egui::StrokeKind::Inside);

        // Header: drag to move, × to close.
        let header = Rect::from_min_size(full.min, vec2(full.width(), 36.0));
        let drag = ui.interact(header, ui.scope_id().with("drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        p.text(header.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, tr("Collect"), font(13.0), t::p().text);
        let x = Rect::from_center_size(header.right_center() - vec2(20.0, 0.0), vec2(22.0, 22.0));
        let xr = ui.interact(x, ui.scope_id().with("close"), Sense::click());
        if xr.hovered() {
            p.rect_filled(x, CornerRadius::same(5), t::p().button_hover);
        }
        p.text(x.center(), Align2::CENTER_CENTER, crate::icons::X, font(13.0), t::p().text_muted);
        if xr.clicked() {
            self.open = false;
        }

        // Drop zone.
        let zone = Rect::from_min_size(Pos2::new(full.left() + 12.0, header.bottom()), vec2(full.width() - 24.0, 84.0));
        let (fill, edge) = if hovering { (t::p().primary.gamma_multiply(0.15), t::p().primary) } else { (t::p().main, t::p().guide) };
        p.rect(zone, CornerRadius::same(8), fill, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        let enabled = folders.is_some();
        let (line1, line2) = if enabled { (tr("Drop files here"), tr("or paste with ⌘V")) } else { (tr("No library open"), "") };
        p.text(zone.center() - vec2(0.0, 20.0), Align2::CENTER_CENTER, crate::icons::TRAY_ARROW_DOWN, font(22.0), if hovering { t::p().primary } else { t::p().text_muted });
        p.text(zone.center() + vec2(0.0, 6.0), Align2::CENTER_CENTER, line1, font(13.0), t::p().text);
        p.text(zone.center() + vec2(0.0, 24.0), Align2::CENTER_CENTER, line2, font(11.5), t::p().text_muted);

        // Name, description, tags, then the folder chooser.
        let status_h = 24.0;
        let area = Rect::from_min_max(Pos2::new(full.left() + 12.0, zone.bottom() + 8.0), Pos2::new(full.right() - 12.0, full.bottom() - status_h));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let muted = |s: &str| egui::RichText::new(s.to_string()).size(12.0).color(t::p().text_muted);
        c.label(muted(tr("Name")));
        c.add(egui::TextEdit::singleline(&mut self.meta.name).hint_text(tr("Used when one file is collected")).desired_width(f32::INFINITY));
        c.add_space(4.0);
        c.label(muted(tr("Description")));
        c.add(egui::TextEdit::multiline(&mut self.meta.description).desired_rows(2).desired_width(f32::INFINITY));
        c.add_space(4.0);
        c.label(muted(tr("Tags")));
        self.tag_editor(&mut c, pool);
        c.add_space(6.0);

        let mut event = None;
        c.label(muted(tr("Save to")));
        if let Some(rows) = folders {
            let search = c.add(egui::TextEdit::singleline(&mut self.filter).hint_text(tr("Search folders")).desired_width(f32::INFINITY));
            c.add_space(4.0);
            let list = lines(rows, state.selected, state.recent, &self.filter);
            if search.lost_focus() && c.input(|i| i.key_pressed(egui::Key::Enter)) {
                event = enter_creates(&list).map(|n| Event::CreateFolder(n.to_string()));
            }
            let additive = c.input(|i| i.modifiers.command);
            let mut remove: Option<String> = None;
            let searching = !self.filter.trim().is_empty();
            egui::ScrollArea::vertical().auto_shrink(false).show(&mut c, |ui| {
                for line in &list {
                    match line {
                        Line::NoFolder => {
                            if folder_row(ui, tr("No folder"), 0, state.selected.is_empty(), false).clicked() {
                                state.selected.clear();
                            }
                        }
                        Line::Recent(r) => {
                            let on = state.selected.contains(&r.id);
                            let resp = folder_row(ui, &r.path, 0, on, on);
                            if resp.clicked() {
                                toggle_folder(state.selected, &r.id, additive);
                            }
                            resp.context_menu(|ui| {
                                if ui.button(tr("Remove from Recent")).clicked() {
                                    remove = Some(r.id.clone());
                                    ui.close();
                                }
                            });
                        }
                        Line::Separator => {
                            ui.add_space(3.0);
                            ui.separator();
                            ui.add_space(3.0);
                        }
                        Line::Tree(r) => {
                            let on = state.selected.contains(&r.id);
                            let (label, depth) = if searching { (r.path.as_str(), 0) } else { (r.name.as_str(), r.depth) };
                            if folder_row(ui, label, depth, on, on).clicked() {
                                toggle_folder(state.selected, &r.id, additive);
                            }
                        }
                        Line::Create(name) => {
                            if folder_row(ui, &trf("New Folder “{name}”", &[("name", name)]), 0, false, false).clicked() {
                                event = Some(Event::CreateFolder(name.clone()));
                            }
                        }
                    }
                }
            });
            if let Some(id) = remove {
                state.recent.retain(|r| *r != id);
            }
            if matches!(event, Some(Event::CreateFolder(_))) {
                self.filter.clear();
            }
        }

        // Status line.
        if self.status.as_ref().is_some_and(|s| now - s.1 > 4.0) {
            self.status = None;
        }
        if let Some((msg, _)) = &self.status {
            let r = Rect::from_min_max(Pos2::new(full.left() + 14.0, full.bottom() - status_h), full.max);
            p.text(r.left_center(), Align2::LEFT_CENTER, msg, font(11.5), t::p().text_muted);
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        let rows = folders?;
        if event.is_some() {
            return event;
        }
        let chosen = target(rows, state.selected);
        enum Target {
            Library,
            Folder(String),
            Folders(usize),
        }
        let dest = match chosen.as_slice() {
            [] => Target::Library,
            [one] => Target::Folder(rows.iter().find(|r| r.id == *one).map_or_else(String::new, |r| r.name.clone())),
            many => Target::Folders(many.len()),
        };
        if !dropped.is_empty() {
            let n = dropped.len();
            let msg = match &dest {
                Target::Library => tr_nf(n, "Adding {n} item to the library", "Adding {n} items to the library", &[]),
                Target::Folder(f) => tr_nf(n, "Adding {n} item to {folder}", "Adding {n} items to {folder}", &[("folder", f)]),
                Target::Folders(c) => tr_nf(n, "Adding {n} item to {count} folders", "Adding {n} items to {count} folders", &[("count", c)]),
            };
            self.status = Some((msg, now));
            return Some(Event::Files(dropped));
        }
        if pasted && !typing {
            let msg = match &dest {
                Target::Library => tr("Pasting into the library").to_string(),
                Target::Folder(f) => trf("Pasting into {folder}", &[("folder", f)]),
                Target::Folders(c) => trf("Pasting into {count} folders", &[("count", c)]),
            };
            self.status = Some((msg, now));
            return Some(Event::Paste);
        }
        None
    }

    /// The tag field: chips with ×, an input that takes Enter or a comma,
    /// and suggestions from the library while typing.
    fn tag_editor(&mut self, ui: &mut Ui, pool: &TagPool) {
        let mut gone = None;
        ui.horizontal_wrapped(|ui| {
            for (i, tag) in self.meta.tags.iter().enumerate() {
                let chip = format!("{tag}  {}", crate::icons::X);
                if ui.add(egui::Button::new(egui::RichText::new(chip).size(12.0)).corner_radius(CornerRadius::same(10))).clicked() {
                    gone = Some(i);
                }
            }
        });
        if let Some(i) = gone {
            self.meta.tags.remove(i);
        }
        let resp = ui.add(egui::TextEdit::singleline(&mut self.tag_input).hint_text(tr("Add a tag")).desired_width(f32::INFINITY));
        let comma = self.tag_input.contains(',');
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if comma || enter {
            for part in std::mem::take(&mut self.tag_input).split(',') {
                add_tag(&mut self.meta.tags, part);
            }
            if enter {
                resp.request_focus();
            }
        }
        let picks = suggest(&self.tag_input, &pool.starred, &pool.history, &pool.all, &self.meta.tags);
        if !picks.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for tag in &picks {
                    if ui.add(egui::Button::new(egui::RichText::new(tag).size(11.5).color(t::p().text_muted)).corner_radius(CornerRadius::same(10))).clicked() {
                        add_tag(&mut self.meta.tags, tag);
                        self.tag_input.clear();
                    }
                }
            });
        }
    }
}

/// The saved choices the window edits.
pub struct State<'a> {
    pub selected: &'a mut Vec<String>,
    pub recent: &'a mut Vec<String>,
}

/// One selectable folder row, with a check mark when `checked`.
fn folder_row(ui: &mut Ui, label: &str, depth: usize, selected: bool, checked: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let fill = if selected {
        t::p().row_selected
    } else if resp.hovered() {
        t::p().row_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    let x = rect.left() + 8.0 + depth as f32 * 14.0;
    ui.painter().text(Pos2::new(x + 7.0, rect.center().y), Align2::CENTER_CENTER, crate::icons::FOLDER, font(14.0), t::p().text_muted);
    let g = ui.painter().layout(label.to_string(), font(13.0), t::p().text, rect.right() - x - 44.0);
    ui.painter().galley(Pos2::new(x + 20.0, rect.center().y - g.size().y / 2.0), g, t::p().text);
    if checked {
        ui.painter().text(Pos2::new(rect.right() - 14.0, rect.center().y), Align2::CENTER_CENTER, crate::icons::CHECK, font(13.0), t::p().primary);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, name: &str, children: Vec<Folder>) -> Folder {
        let v = serde_json::json!({
            "id": id, "name": name, "description": "", "children": children,
            "modificationTime": 0, "tags": [], "password": "", "passwordTips": ""
        });
        serde_json::from_value(v).unwrap()
    }

    fn tree() -> Vec<Folder> {
        vec![folder("A", "Art", vec![folder("B", "Brushes", vec![folder("C", "Charcoal", vec![])])]), folder("D", "Design", vec![])]
    }

    #[test]
    fn flattens_in_tree_order_with_paths() {
        let rows = flatten(&tree());
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["A", "B", "C", "D"]);
        assert_eq!(rows[2].path, "Art / Brushes / Charcoal");
        assert_eq!(rows[2].depth, 2);
        assert_eq!(rows[3].depth, 0);
    }

    #[test]
    fn search_matches_any_part_of_the_path() {
        let rows = flatten(&tree());
        let hits: Vec<_> = rows.iter().filter(|r| matches(r, "brush")).map(|r| r.id.as_str()).collect();
        assert_eq!(hits, ["B", "C"]);
        assert_eq!(rows.iter().filter(|r| matches(r, "  ")).count(), 4);
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn target_keeps_surviving_folders_in_order() {
        let rows = flatten(&tree());
        assert_eq!(target(&rows, &ids(&["D", "gone", "A", "D"])), ids(&["D", "A"]));
        assert!(target(&rows, &[]).is_empty());
    }

    #[test]
    fn click_selects_one_and_command_click_toggles() {
        let mut s = ids(&["A", "B"]);
        toggle_folder(&mut s, "C", false);
        assert_eq!(s, ids(&["C"]));
        toggle_folder(&mut s, "A", true);
        assert_eq!(s, ids(&["C", "A"]));
        toggle_folder(&mut s, "C", true);
        assert_eq!(s, ids(&["A"]));
    }

    #[test]
    fn migrates_the_old_single_folder_once() {
        let (mut f, mut o) = (Vec::new(), Some("X".to_string()));
        migrate(&mut f, &mut o);
        assert_eq!((f.clone(), o.clone()), (ids(&["X"]), None));
        let mut o = Some("Y".to_string());
        migrate(&mut f, &mut o);
        assert_eq!((f, o), (ids(&["X"]), None), "an existing selection wins");
    }

    #[test]
    fn recent_moves_used_folders_to_the_front_and_caps() {
        let mut r = ids(&["A", "B", "C"]);
        push_recent(&mut r, &ids(&["C", "D"]));
        assert_eq!(r, ids(&["C", "D", "A", "B"]));
        let mut many: Vec<String> = (0..25).map(|i| format!("f{i}")).collect();
        push_recent(&mut many, &ids(&["new"]));
        assert_eq!((many.len(), many[0].as_str()), (RECENT_MAX, "new"));
    }

    fn kinds(l: &[Line]) -> Vec<String> {
        l.iter()
            .map(|x| match x {
                Line::NoFolder => "none".into(),
                Line::Recent(r) => format!("r:{}", r.id),
                Line::Separator => "-".into(),
                Line::Tree(r) => format!("t:{}", r.id),
                Line::Create(n) => format!("new:{n}"),
            })
            .collect()
    }

    #[test]
    fn plain_tree_without_recent_or_selected() {
        let rows = flatten(&tree());
        assert_eq!(kinds(&lines(&rows, &[], &["gone".into()], "")), ["none", "t:A", "t:B", "t:C", "t:D"]);
    }

    #[test]
    fn recent_block_puts_selected_first_then_a_separator_and_the_tree() {
        let rows = flatten(&tree());
        let l = lines(&rows, &ids(&["D"]), &ids(&["B", "D", "A"]), "");
        assert_eq!(kinds(&l), ["none", "r:D", "r:B", "r:A", "-", "t:A", "t:B", "t:C", "t:D"]);
        let Line::Recent(r) = &l[2] else { panic!() };
        assert_eq!(r.path, "Art / Brushes");
    }

    #[test]
    fn recent_block_is_skipped_when_it_is_everything() {
        let rows = flatten(&[folder("A", "Art", vec![])]);
        assert_eq!(kinds(&lines(&rows, &ids(&["A"]), &[], "")), ["none", "t:A"]);
    }

    #[test]
    fn searching_filters_the_block_and_leaves_its_folders_out_below() {
        let rows = flatten(&tree());
        let l = lines(&rows, &[], &ids(&["B"]), "brush");
        assert_eq!(kinds(&l), ["r:B", "-", "t:C", "new:brush"]);
    }

    #[test]
    fn create_row_unless_a_folder_has_that_exact_name() {
        let rows = flatten(&tree());
        assert!(kinds(&lines(&rows, &[], &[], "Design")).iter().all(|k| !k.starts_with("new")));
        let l = lines(&rows, &[], &[], "  zzz ");
        assert_eq!(kinds(&l), ["new:zzz"]);
        assert_eq!(enter_creates(&l), Some("zzz"));
        assert_eq!(enter_creates(&lines(&rows, &[], &[], "des")), None);
    }

    #[test]
    fn recent_block_is_capped_at_five_plus_selected() {
        let many: Vec<Folder> = (0..12).map(|i| folder(&format!("f{i}"), &format!("F{i}"), vec![])).collect();
        let rows = flatten(&many);
        let recent: Vec<String> = (0..12).map(|i| format!("f{i}")).collect();
        let l = lines(&rows, &ids(&["f11"]), &recent, "");
        assert_eq!(l.iter().filter(|x| matches!(x, Line::Recent(_))).count(), 6);
    }

    #[test]
    fn tags_add_once_and_suggestions_are_ordered() {
        let mut t = Vec::new();
        assert!(add_tag(&mut t, " Sky "));
        assert!(!add_tag(&mut t, "sky"));
        assert!(!add_tag(&mut t, "  "));
        assert_eq!(t, ids(&["Sky"]));
        let s = suggest("s", &ids(&["star"]), &ids(&["Sea", "star"]), &ids(&["sun", "Sea", "Sky", "x"]), &ids(&["Sky"]));
        assert_eq!(s, ids(&["star", "Sea", "sun"]));
        assert!(suggest("", &ids(&["a"]), &[], &[], &[]).is_empty());
        let many: Vec<String> = (0..20).map(|i| format!("t{i}")).collect();
        assert_eq!(suggest("t", &[], &[], &many, &[]).len(), SUGGESTIONS);
    }

    #[test]
    fn meta_goes_into_the_import_options() {
        let m = Meta { name: " Cover ".into(), description: "A note".into(), tags: ids(&["a", "B"]) };
        let mut o = ImportOptions { tags: ids(&["b"]), ..Default::default() };
        m.apply(&mut o, 1);
        assert_eq!((o.tags.clone(), o.annotation.as_str(), o.name.as_deref()), (ids(&["b", "a"]), "A note", Some("Cover")));
        let mut o = ImportOptions::default();
        m.apply(&mut o, 2);
        assert_eq!(o.name, None, "the name is for one file only");
    }
}
