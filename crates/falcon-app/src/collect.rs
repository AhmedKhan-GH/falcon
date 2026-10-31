//! The collect window: a small always-on-top window that takes files
//! dropped on it (or pasted into it) and adds them to a chosen folder,
//! so items can be collected while the main window is behind other apps.
//! It is falcon's take on Eagle's collect window, which asks where to save
//! an item arriving from the browser extension or a screenshot.

use std::path::PathBuf;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, vec2};
use falcon_library::model::Folder;

use crate::shortcuts::{Command, Shortcuts};
use crate::theme::{self as t, font};

/// Size of the window, in points.
pub const SIZE: egui::Vec2 = vec2(280.0, 360.0);

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

/// The chosen folder, if it still exists in this library.
pub fn target(rows: &[FolderRow], chosen: Option<&str>) -> Option<String> {
    let id = chosen?;
    rows.iter().any(|r| r.id == id).then(|| id.to_string())
}

/// What the window received this frame.
#[derive(PartialEq, Debug)]
pub enum Event {
    Files(Vec<PathBuf>),
    /// ⌘V: import what is on the clipboard.
    Paste,
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
}

impl Collect {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.open_at = self.pos;
    }

    /// Shows the window as its own always-on-top, borderless OS window.
    /// Returns what was dropped or pasted on it.
    pub fn show(&mut self, ctx: &egui::Context, folders: Option<&[FolderRow]>, chosen: &mut Option<String>, keys: &Shortcuts) -> Option<Event> {
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Collect")
            .with_inner_size(SIZE)
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top();
        if let Some(p) = self.open_at {
            builder = builder.with_position(p);
        }
        let id = egui::ViewportId::from_hash_of("falcon-collect");
        ctx.show_viewport_immediate(id, builder, |ui, _class| self.contents(ui, folders, chosen, keys))
    }

    fn contents(&mut self, ui: &mut Ui, folders: Option<&[FolderRow]>, chosen: &mut Option<String>, keys: &Shortcuts) -> Option<Event> {
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
        p.text(header.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, "Collect", font(13.0), t::p().text);
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
        let zone = Rect::from_min_size(Pos2::new(full.left() + 12.0, header.bottom()), vec2(full.width() - 24.0, 110.0));
        let (fill, edge) = if hovering { (t::p().primary.gamma_multiply(0.15), t::p().primary) } else { (t::p().main, t::p().guide) };
        p.rect(zone, CornerRadius::same(8), fill, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        let enabled = folders.is_some();
        let (line1, line2) = if enabled { ("Drop files here", "or paste with ⌘V") } else { ("No library open", "") };
        p.text(zone.center() - vec2(0.0, 22.0), Align2::CENTER_CENTER, crate::icons::TRAY_ARROW_DOWN, font(26.0), if hovering { t::p().primary } else { t::p().text_muted });
        p.text(zone.center() + vec2(0.0, 10.0), Align2::CENTER_CENTER, line1, font(13.0), t::p().text);
        p.text(zone.center() + vec2(0.0, 28.0), Align2::CENTER_CENTER, line2, font(11.5), t::p().text_muted);

        // Folder chooser.
        let list_top = zone.bottom() + 12.0;
        let status_h = 24.0;
        let area = Rect::from_min_max(Pos2::new(full.left() + 12.0, list_top), Pos2::new(full.right() - 12.0, full.bottom() - status_h));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(area));
        c.label(egui::RichText::new("Save to").size(12.0).color(t::p().text_muted));
        if let Some(rows) = folders {
            c.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search folders").desired_width(f32::INFINITY));
            c.add_space(4.0);
            let current = target(rows, chosen.as_deref());
            egui::ScrollArea::vertical().auto_shrink(false).show(&mut c, |ui| {
                if self.filter.trim().is_empty() && folder_row(ui, "No folder", 0, current.is_none()) {
                    *chosen = None;
                }
                for r in rows.iter().filter(|r| matches(r, &self.filter)) {
                    let label = if self.filter.trim().is_empty() { r.name.as_str() } else { r.path.as_str() };
                    let depth = if self.filter.trim().is_empty() { r.depth } else { 0 };
                    if folder_row(ui, label, depth, current.as_deref() == Some(r.id.as_str())) {
                        *chosen = Some(r.id.clone());
                    }
                }
            });
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
        let name = target(rows, chosen.as_deref()).and_then(|id| rows.iter().find(|r| r.id == id)).map_or("no folder".to_string(), |r| r.name.clone());
        if !dropped.is_empty() {
            let n = dropped.len();
            self.status = Some((format!("Adding {n} item{} to {name}", if n == 1 { "" } else { "s" }), now));
            return Some(Event::Files(dropped));
        }
        if pasted && !typing {
            self.status = Some((format!("Pasting into {name}"), now));
            return Some(Event::Paste);
        }
        None
    }
}

/// One selectable folder row; true when clicked.
fn folder_row(ui: &mut Ui, label: &str, depth: usize, selected: bool) -> bool {
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
    let g = ui.painter().layout(label.to_string(), font(13.0), t::p().text, rect.right() - x - 24.0);
    ui.painter().galley(Pos2::new(x + 20.0, rect.center().y - g.size().y / 2.0), g, t::p().text);
    resp.clicked()
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

    #[test]
    fn target_needs_the_folder_to_exist() {
        let rows = flatten(&tree());
        assert_eq!(target(&rows, Some("C")), Some("C".to_string()));
        assert_eq!(target(&rows, Some("gone")), None, "a folder from another library");
        assert_eq!(target(&rows, None), None);
    }
}
