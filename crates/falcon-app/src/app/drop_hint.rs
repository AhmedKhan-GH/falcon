//! While files from Finder are dragged over the window, shows where they
//! would go, after Eagle's drop areas: the grid ("drop files to import"),
//! the sidebar ("drop folders to import"), or one folder row (outlined).

use eframe::egui::{self, Color32, CornerRadius, LayerId, Order, Rect, Stroke, StrokeKind, vec2};

use super::App;
use crate::drop_pos::{self, Target};
use crate::theme::{self as t, font};

impl App {
    pub(crate) fn drop_hint(&mut self, ctx: &egui::Context) {
        let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let ours = self.native_drag.load(std::sync::atomic::Ordering::Relaxed);
        if !dragging || ours || self.ws.lib.is_none() {
            return;
        }
        // macOS sends no mouse moves during the drag; poll the mouse.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        let sidebar = self.show_sidebar.then_some(self.imports.sidebar).flatten();
        let target = drop_pos::target(drop_pos::at_drop(ctx), sidebar, &self.imports.folder_rows);
        let window = ctx.content_rect();
        let p = ctx.layer_painter(LayerId::new(Order::Foreground, egui::Id::unique("drop-hint")));
        let accent = t::p().primary;
        let shade = |r: Rect, text: &str| {
            let r = r.shrink(8.0);
            p.rect_filled(r, CornerRadius::same(10), t::p().main.gamma_multiply(0.85));
            p.rect_stroke(r, CornerRadius::same(10), Stroke::new(2.0, accent), StrokeKind::Inside);
            let g = p.layout(text.to_string(), font(14.0), t::p().text, (r.width() - 32.0).max(80.0));
            let at = r.center() - g.size() / 2.0;
            p.galley(at, g, Color32::PLACEHOLDER);
        };
        match target {
            Target::Grid => {
                let left = sidebar.map_or(window.left(), |s| s.right());
                let right = if self.show_inspector { window.right() - self.inspector_width } else { window.right() };
                let grid = Rect::from_x_y_ranges(left..=right, window.y_range());
                let text = match &self.ws.scope {
                    crate::logic::Scope::Folder(_) => "Drop files to import them into this folder",
                    _ => "Drop files to import them",
                };
                shade(grid, text);
            }
            Target::Sidebar => {
                if let Some(s) = sidebar {
                    shade(s, "Drop folders to import them with their subfolders");
                }
            }
            Target::Folder(id) => {
                if let Some((r, _)) = self.imports.folder_rows.iter().find(|(_, f)| *f == id) {
                    let r = r.shrink2(vec2(6.0, 0.0));
                    p.rect_stroke(r, t::RADIUS, Stroke::new(2.0, accent), StrokeKind::Inside);
                }
            }
        }
    }
}
