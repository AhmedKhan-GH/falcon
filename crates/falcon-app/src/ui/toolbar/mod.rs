//! The toolbar: navigation buttons, the layout panel button, the search field, the zoom slider and the import progress strip under it.

pub(crate) mod layout_panel;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::FolderOrder;

use crate::app::{App, Progress, TILE_MAX, TILE_MIN};
use crate::icons as ic;
use crate::locale::{tr, trf};
use crate::logic::{self, Scope};
use crate::shortcuts::Command;
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley, icon, icon_button, window_drag};
use crate::ui::grid::GRID_PAD_X;
use crate::ui::sidebar::smart_name;
/// Whether the toolbar title adds "Search results (N)": while a search or
/// a filter narrows the view, except on the All Tags page (whose search
/// filters tags, not items), as in Eagle.
fn shows_results(scope: &Scope, query: &str, filtered: bool) -> bool {
    *scope != Scope::AllTags && (filtered || !query.trim().is_empty())
}

impl App {
    pub(crate) fn toolbar(&mut self, ui: &mut Ui, lib: &Library) {
        let bar = ui.max_rect();
        window_drag(ui, bar, "toolbar");
        let cy = bar.center().y;
        let mut x = bar.left() + logic::bar_inset(!self.show_sidebar);
        self.panels_button(ui, Pos2::new(x, cy));
        // With the sidebar hidden, ⌘N's menu opens under the toolbar's left end.
        if !self.show_sidebar {
            self.new_menu(ui, None, Pos2::new(x - 13.0, bar.bottom()));
        }
        x += 30.0;
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 17.0, !self.ws.back.is_empty(), "").clicked() {
            self.go_back();
        }
        if icon_button(ui, Pos2::new(x + 27.0, cy), ic::CARET_RIGHT, 17.0, !self.ws.forward.is_empty(), "").clicked() {
            self.go_forward();
        }
        // Eagle's breadcrumb: the page, folder or smart folder (its full path
        // on hover, its ancestors on right-click), then "Search results (N)"
        // while a search or a filter narrows it. A click on the first part
        // clears both.
        let title = if let Some(t) = self.combined_title() {
            t
        } else if let Scope::Smart(id) = &self.ws.scope {
            smart_name(lib, id)
        } else {
            self.ws.scope.title(&lib.metadata.folders)
        };
        let narrowed = shows_results(&self.ws.scope, &self.ws.query, self.ws.filter.is_active());
        let g = galley(ui, &title, font(15.0), t::p().text, if narrowed { 160.0 } else { 220.0 });
        let title_rect = Rect::from_min_size(Pos2::new(x + 45.0, cy - g.size().y / 2.0), g.size());
        ui.painter().galley(title_rect.min, g, t::p().text);
        let mut title_end = title_rect.right();
        let title_resp = ui.interact(title_rect, ui.scope_id().with("title"), Sense::click());
        if narrowed {
            let sep = Pos2::new(title_end + 12.0, cy);
            icon(ui, sep, ic::CARET_RIGHT, 12.0, t::p().text_muted);
            let text = format!("{} ({})", tr("Search results"), logic::group_digits(self.ws.order.len()));
            let g = galley(ui, &text, font(15.0), t::p().text, 200.0);
            let at = Pos2::new(sep.x + 12.0, cy - g.size().y / 2.0);
            title_end = at.x + g.size().x;
            ui.painter().galley(at, g, t::p().text);
            if title_resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.ws.query.clear();
                self.reset_filter();
            }
        }
        self.title_menu(lib, title_resp);

        // Right side: search field, then the icon row.
        let search = Rect::from_min_size(Pos2::new(bar.right() - 18.0 - 178.0, cy - 15.0), vec2(178.0, 30.0));
        self.search_field(ui, search);
        let mut ix = search.left() - 20.0;
        let own = self.own_sort(lib);
        let filter = icon_button(ui, Pos2::new(ix, cy), ic::FUNNEL, 18.0, true, tr("Filter"));
        ix -= 26.0;
        // Random, and a folder sorted at random, offer a new shuffle.
        let refresh = matches!(self.ws.scope, Scope::Random(_)) || matches!(own, Some((_, Some((FolderOrder::Random, _)))));
        if refresh {
            let tip = self.shortcuts.tooltip(ui.ctx(), tr("Refresh"), Command::Reshuffle);
            if icon_button(ui, Pos2::new(ix, cy), ic::ARROW_CLOCKWISE, 18.0, true, &tip).clicked() {
                let ctx = ui.ctx().clone();
                self.run_command(&ctx, Command::Reshuffle);
            }
            ix -= 26.0;
        }
        let layout = icon_button(ui, Pos2::new(ix, cy), ic::LAYOUT, 18.0, true, tr("Layout and sort"));
        ix -= 26.0;
        icon_button(ui, Pos2::new(ix, cy), ic::PUZZLE_PIECE, 18.0, false, tr("Plugins"));
        let icons_left = ix - 20.0;

        egui::Popup::menu(&layout).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| self.layout_panel(ui, own));
        if filter.clicked() {
            self.ws.show_filter = !self.ws.show_filter;
        }
        if self.ws.show_filter || self.ws.filter.is_active() {
            let r = Rect::from_center_size(filter.rect.center(), Vec2::splat(26.0));
            ui.painter().rect_stroke(r, t::RADIUS, Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        }

        // Zoom slider between the title and the icons.
        let zx = (bar.left() + 237.0).max(title_end + 30.0);
        if zx + 130.0 < icons_left {
            self.zoom_slider(ui, Pos2::new(zx, cy));
        }
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, t::p().main));
    }

    /// Right-clicking the title of an open subfolder lists it and its
    /// ancestors; a pick opens that folder. Hovering shows the full path.
    fn title_menu(&mut self, lib: &Library, resp: egui::Response) {
        let Scope::Folder(id) = &self.ws.scope else { return };
        if self.ws.folders.combined() {
            return;
        }
        let chain = logic::ancestor_chain(&lib.metadata.folders, id);
        if chain.len() < 2 {
            return;
        }
        let path = chain.iter().rev().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(" / ");
        resp.clone().on_hover_text(path);
        let mut open = None;
        resp.context_menu(|ui| {
            for (i, (fid, name)) in chain.iter().enumerate() {
                if ui.add(egui::Button::selectable(i == 0, name)).clicked() {
                    open = Some(fid.clone());
                    ui.close();
                }
            }
        });
        if let Some(fid) = open {
            self.go(Scope::Folder(fid));
        }
    }

    /// Shows or hides the sidebar and inspector together (Eagle's
    /// `toggleAll`).
    pub(crate) fn toggle_panels(&mut self) {
        (self.show_sidebar, self.show_inspector) = logic::toggle_all(self.show_sidebar, self.show_inspector);
    }

    /// The panels button at the left of the toolbar and the viewer bar:
    /// a click shows or hides both panels; its menu toggles each one.
    pub(crate) fn panels_button(&mut self, ui: &mut Ui, center: Pos2) {
        use crate::shortcuts::{Command as C, display};
        let key = |cmd: C| self.shortcuts.binding(cmd).map(|c| display(ui.ctx(), &c)).unwrap_or_default();
        let (all, side, insp) = (key(C::ToggleAll), key(C::ToggleSidebar), key(C::ToggleInspector));
        let tip = if all.is_empty() { tr("Sidebar and Inspector (Tab)").to_string() } else { trf("Sidebar and Inspector (Tab, {keys})", &[("keys", &all)]) };
        let resp = icon_button(ui, center, ic::SIDEBAR_SIMPLE, 18.0, true, &tip);
        if resp.clicked() {
            self.toggle_panels();
        }
        resp.context_menu(|ui| {
            ui.set_min_width(220.0);
            if ui.add(egui::Button::new(tr("Sidebar and Inspector")).shortcut_text("Tab")).clicked() {
                self.toggle_panels();
            }
            if ui.add(egui::Button::selectable(self.show_sidebar, tr("Sidebar")).shortcut_text(side)).clicked() {
                self.show_sidebar = !self.show_sidebar;
            }
            if ui.add(egui::Button::selectable(self.show_inspector, tr("Inspector")).shortcut_text(insp)).clicked() {
                self.show_inspector = !self.show_inspector;
            }
        });
    }

    pub(crate) fn search_field(&mut self, ui: &mut Ui, rect: Rect) {
        let p = ui.painter();
        p.rect_filled(rect, t::FIELD_RADIUS, t::p().search);
        p.rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
        icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 16.0, t::p().text);
        icon(ui, Pos2::new(rect.left() + 28.0, rect.center().y + 1.0), ic::CARET_DOWN, 9.0, t::p().text_muted);
        let caret = Rect::from_min_max(Pos2::new(rect.left() + 2.0, rect.top()), Pos2::new(rect.left() + 38.0, rect.bottom()));
        let scope_resp = ui.interact(caret, ui.scope_id().with("search-scope"), Sense::click()).on_hover_text(tr("Search Scope"));
        egui::Popup::menu(&scope_resp).show(|ui| self.search_scope_menu(ui));
        icon(ui, Pos2::new(rect.right() - 17.0, rect.center().y), ic::CAMERA, 17.0, t::p().text_disabled);
        let inner = Rect::from_min_max(Pos2::new(rect.left() + 40.0, rect.top()), Pos2::new(rect.right() - 32.0, rect.bottom()));
        let resp = ui.put(
            inner,
            TextEdit::singleline(&mut self.ws.query)
                .id_salt("search")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new(tr("Search")).color(t::p().text_muted))
                .font(font(14.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width()),
        );
        if std::mem::take(&mut self.focus_search) {
            resp.request_focus();
        }
    }

    pub(crate) fn zoom_slider(&mut self, ui: &mut Ui, left_center: Pos2) {
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
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(track.right(), y)], Stroke::new(2.0, t::p().divider));
        let kx = track.left() + f * track.width();
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(kx, y)], Stroke::new(2.0, t::p().text_muted));
        p.circle_filled(Pos2::new(kx, y), 7.0, Color32::WHITE);
    }

    /// The import progress bar; clears itself a moment after finishing.
    pub(crate) fn progress_bar(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let snapshot = {
            let pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            (pr.total, pr.done, pr.failed, pr.current.clone(), pr.paused, pr.started)
        };
        let (total, done, failed, current, paused, started) = snapshot;
        if total == 0 {
            return false;
        }
        if done >= total {
            let mut pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            let errors = std::mem::take(&mut pr.errors);
            let added = std::mem::take(&mut pr.added);
            *pr = Progress { errors, ..Progress::default() };
            drop(pr);
            self.imports.select_after = Some(added);
            self.import_done_notice(total, done - failed);
            if failed > 0 {
                self.play_sound(crate::sounds::Sound::ImportFailed);
                self.flash_error(trf("Added {count}, {failed} failed", &[("count", &(done - failed)), ("failed", &failed)]));
            } else if self.prefs.tells_import_done() {
                self.flash(trf("Added {count}", &[("count", &done)]));
            }
            return false;
        }
        let p = ui.painter();
        p.rect_filled(rect, 0.0, t::p().main);
        let track = Rect::from_min_max(Pos2::new(rect.left() + GRID_PAD_X, rect.bottom() - 6.0), Pos2::new(rect.right() - GRID_PAD_X, rect.bottom() - 3.0));
        p.rect_filled(track, CornerRadius::same(2), t::p().button);
        let f = done as f32 / total as f32;
        let fill = Rect::from_min_max(track.min, Pos2::new(track.left() + track.width() * f, track.bottom()));
        p.rect_filled(fill, CornerRadius::same(2), t::p().primary);
        // The estimate (not shown while paused) changes with time, not only
        // when an item finishes.
        let left = if paused { None } else { started.and_then(|s| crate::progress_eta::remaining(s.elapsed(), done, total)).map(crate::progress_eta::format) };
        let text = match (paused, left) {
            (true, _) => trf("Paused at {done} of {total}", &[("done", &done), ("total", &total)]),
            (false, Some(left)) => trf("Importing {done} of {total}  ·  {left}  ·  {current}", &[("done", &done), ("total", &total), ("left", &left), ("current", &current)]),
            (false, None) => trf("Importing {done} of {total}  ·  {current}", &[("done", &done), ("total", &total), ("current", &current)]),
        };
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        let g = galley(ui, &text, font(12.0), t::p().text_muted, rect.width() - 40.0);
        p.galley(Pos2::new(rect.left() + GRID_PAD_X, rect.top() + 3.0), g, t::p().text_muted);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_show_while_a_search_or_filter_narrows_the_view() {
        assert!(!shows_results(&Scope::All, "", false));
        assert!(!shows_results(&Scope::All, "   ", false), "blank text is no search");
        assert!(shows_results(&Scope::All, "cat", false));
        assert!(shows_results(&Scope::Folder("F".into()), "", true), "a filter alone counts");
        assert!(!shows_results(&Scope::AllTags, "cat", true), "the tag page filters tags");
    }
}
