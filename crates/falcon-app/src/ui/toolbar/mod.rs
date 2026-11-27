//! The toolbar: navigation buttons, the layout panel button, the search field, the zoom slider and the import progress strip under it.

pub(crate) mod layout_panel;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::FolderOrder;

use crate::app::{App, Progress, TILE_MAX, TILE_MIN};
use crate::icons as ic;
use crate::locale::{tr, trf};
use crate::logic::{self, Scope};
use crate::search_state::SearchPoll;
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
        let narrowed = shows_results(&self.ws.scope, &self.ws.query, self.ws.filter.is_active() || self.ws.ai.active());
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
                self.ws.set_query("");
                self.reset_filter();
            }
        }
        self.title_menu(lib, title_resp);

        let own = self.own_sort(lib);
        let refresh = matches!(self.ws.scope, Scope::Random(_)) || matches!(own, Some((_, Some((FolderOrder::Random, _)))));

        // Dedicated space for inspector/sidebar panel toggle when closed
        if !self.show_inspector {
            let toggle_center = Pos2::new(bar.right() - 24.0, cy);
            if icon_button(ui, toggle_center, ic::SIDEBAR_SIMPLE, 18.0, true, tr("Show inspector")).clicked() {
                self.show_inspector = true;
            }
        }

        // Right side: dynamic search field with reserved 8px spacing, then the icon row.
        let toggle_reserved = if !self.show_inspector { 44.0 } else { 16.0 };
        let search_right = bar.right() - toggle_reserved;

        let mut num_icons = 3.0; // Filter, Layout, Plugins
        if self.ai_view_allowed() { num_icons += 1.0; }
        if refresh { num_icons += 1.0; }
        if self.ws.scope != Scope::AllTags { num_icons += 1.0; }
        let icons_width = num_icons * 26.0 + 20.0;

        let avail_right = (search_right - (title_end + 30.0)).max(0.0);
        let max_search_w = (avail_right - icons_width).max(90.0);
        let desired_search_w = ((bar.width() - 500.0) * 0.15 + 130.0).clamp(110.0, 190.0);
        let search_w = desired_search_w.min(max_search_w).clamp(90.0, 200.0);

        let search = Rect::from_min_size(Pos2::new(search_right - search_w, cy - 15.0), vec2(search_w, 30.0));
        self.search_field(ui, search);
        let mut ix = search.left() - 20.0;
        let filter = icon_button(ui, Pos2::new(ix, cy), ic::FUNNEL, 18.0, true, tr("Filter"));
        ix -= 26.0;
        if self.ai_view_allowed() {
            self.ai_button(ui, Pos2::new(ix, cy));
            ix -= 26.0;
        }
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
        // Eagle hides its Actions button on the All Tags page.
        if self.ws.scope != Scope::AllTags {
            let tip = self.shortcuts.tooltip(ui.ctx(), tr("Actions"), Command::ActionsPanel);
            let b = icon_button(ui, Pos2::new(ix, cy), ic::LIGHTNING, 18.0, true, &tip);
            if b.clicked() {
                if self.user_actions.panel.open {
                    self.user_actions.panel.open = false;
                } else {
                    self.open_actions_panel(Some(b.rect.center_bottom()));
                }
            }
            if self.user_actions.panel.open {
                let r = Rect::from_center_size(b.rect.center(), Vec2::splat(26.0));
                ui.painter().rect_stroke(r, t::RADIUS, Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
            }
            ix -= 26.0;
        }
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
        let has_zoom = zx + 145.0 < icons_left;
        let slider_dragged = if has_zoom {
            self.zoom_slider(ui, Pos2::new(zx, cy))
        } else {
            false
        };

        // Window drag on empty toolbar regions only, strictly excluding all
        // buttons, title, icons, search, and the zoom slider zone.
        let drag_start = (title_end + 6.0).max(bar.left());
        let zoom_left = (zx - 16.0).max(drag_start);
        let zoom_right = (zx + 140.0).min(icons_left);
        if has_zoom {
            if zoom_left > drag_start {
                window_drag(ui, Rect::from_min_max(Pos2::new(drag_start, bar.top()), Pos2::new(zoom_left, bar.bottom())), "toolbar-left");
            }
            if zoom_right < icons_left {
                window_drag(ui, Rect::from_min_max(Pos2::new(zoom_right, bar.top()), Pos2::new(icons_left, bar.bottom())), "toolbar-right");
            }
        } else if icons_left > drag_start {
            window_drag(ui, Rect::from_min_max(Pos2::new(drag_start, bar.top()), Pos2::new(icons_left, bar.bottom())), "toolbar");
        }

        // Disable native macOS 28pt titlebar dragging whenever the pointer is
        // over interactive toolbar controls (especially the zoom slider) or
        // while the zoom slider is actively being dragged.
        let over_zoom = has_zoom && ui.input(|i| {
            i.pointer.hover_pos().is_some_and(|p| {
                bar.y_range().contains(p.y) && (zoom_left..=zoom_right).contains(&p.x)
            })
        });
        let over_controls = ui.input(|i| {
            i.pointer.hover_pos().is_some_and(|p| {
                bar.contains(p) && (p.x >= icons_left || p.x <= title_rect.left())
            })
        });
        let movable = !(slider_dragged || over_zoom || over_controls);
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::NULL.with("window_movable"), movable));

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

    /// The AI Search button: a highlight while the search box reads
    /// descriptions, and a thin bar while the library is being indexed.
    fn ai_button(&mut self, ui: &mut Ui, center: Pos2) {
        let status = self.ai.engine.as_ref().map(|e| e.status()).unwrap_or_default();
        let b = icon_button(ui, center, ic::SPARKLE, 18.0, true, &crate::ai_search::button_tip(&status));
        if b.clicked() {
            self.ai_button_clicked();
        }
        if self.ws.ai.mode {
            let r = Rect::from_center_size(center, Vec2::splat(26.0));
            ui.painter().rect_stroke(r, t::RADIUS, Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        }
        if let Some(part) = crate::ai_search::progress(&status) {
            let track = Rect::from_center_size(center + vec2(0.0, 12.0), vec2(20.0, 2.0));
            ui.painter().rect_filled(track, 1.0, t::p().button);
            ui.painter().rect_filled(Rect::from_min_size(track.min, vec2(track.width() * part, 2.0)), 1.0, t::p().primary);
        }
    }

    /// The camera at the right of the search box: searches by a picture
    /// once the model is ready, and ends an image search while one is shown.
    fn search_camera(&mut self, ui: &mut Ui, rect: Rect) {
        let center = Pos2::new(rect.right() - 17.0, rect.center().y);
        let spot = Rect::from_center_size(center, Vec2::splat(26.0));
        if self.ws.ai.image.is_some() {
            icon(ui, center, ic::X, 14.0, t::p().text_muted);
            if ui.interact(spot, ui.scope_id().with("search-image-end"), Sense::click()).on_hover_text(tr("End the image search")).clicked() {
                self.ws.ai.clear_results();
            }
            return;
        }
        let ready = self.ai_ready();
        icon(ui, center, ic::CAMERA, 17.0, if ready { t::p().text_value } else { t::p().text_disabled });
        let tip = if ready { tr("Search by image") } else { tr("Install the model in Preferences ▸ AI Search") };
        let resp = ui.interact(spot, ui.scope_id().with("search-camera"), Sense::click()).on_hover_text(tip);
        if !ready {
            if resp.clicked() {
                self.open_ai_preferences();
            }
            return;
        }
        egui::Popup::menu(&resp).show(|ui| {
            if ui.button(tr("Choose Image…")).clicked() {
                self.ai_choose_image();
            }
            if ui.button(tr("Paste Image")).clicked() {
                self.ai_paste_image();
            }
            ui.label(RichText::new(tr("Or drop a picture on the search box")).size(11.0).color(t::p().text_muted));
        });
    }

    pub(crate) fn search_field(&mut self, ui: &mut Ui, rect: Rect) {
        self.ws.ai.search_rect = Some(rect);
        let p = ui.painter();
        p.rect_filled(rect, t::FIELD_RADIUS, t::p().search);
        p.rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
        icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 16.0, t::p().text);
        icon(ui, Pos2::new(rect.left() + 28.0, rect.center().y + 1.0), ic::CARET_DOWN, 9.0, t::p().text_muted);
        let caret = Rect::from_min_max(Pos2::new(rect.left() + 2.0, rect.top()), Pos2::new(rect.left() + 38.0, rect.bottom()));
        let scope_resp = ui.interact(caret, ui.scope_id().with("search-scope"), Sense::click()).on_hover_text(tr("Search Scope"));
        egui::Popup::menu(&scope_resp).show(|ui| self.search_scope_menu(ui));
        self.search_camera(ui, rect);
        let hint = if let Some(label) = &self.ws.ai.image {
            trf("Like {name}", &[("name", label)])
        } else if self.ws.ai.mode && self.ai_view_allowed() {
            tr("Describe a picture").to_string()
        } else {
            tr("Search").to_string()
        };
        let inner = Rect::from_min_max(Pos2::new(rect.left() + 40.0, rect.top()), Pos2::new(rect.right() - 32.0, rect.bottom()));
        let resp = ui.put(
            inner,
            TextEdit::singleline(&mut self.ws.search_typed)
                .id_salt("search")
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .hint_text(RichText::new(hint).color(t::p().text_muted))
                .font(font(14.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(inner.width()),
        );
        if std::mem::take(&mut self.focus_search) {
            resp.request_focus();
        }
        // Eagle applies the search after a short debounce, or at once on Return.
        let submitted = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        match self.ws.search_debounce.poll(resp.changed(), submitted, ui.input(|i| i.time)) {
            SearchPoll::Apply if self.ws.ai.mode && self.ai_view_allowed() => {
                let text = self.ws.search_typed.clone();
                self.ai_text_search(&text);
            }
            SearchPoll::Apply => {
                // Typing ends an image search.
                self.ws.ai.clear_results();
                self.ws.query.clone_from(&self.ws.search_typed);
            }
            SearchPoll::Wait(left) => ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left)),
            SearchPoll::Idle => {}
        }
    }

    pub(crate) fn zoom_slider(&mut self, ui: &mut Ui, left_center: Pos2) -> bool {
        let cy = left_center.y;
        let x0 = left_center.x;
        let can_zoom_out = self.tile > TILE_MIN;
        let can_zoom_in = self.tile < TILE_MAX;

        // Minus button (26x26, centered at x0)
        let minus_center = Pos2::new(x0, cy);
        if icon_button(ui, minus_center, ic::MINUS, 13.0, can_zoom_out, tr("Zoom out (⌘−)")).clicked() {
            self.tile = (self.tile - 20.0).max(TILE_MIN).round();
            if let Some(id) = self.ws.selected.first().cloned() {
                self.ws.scroll_to = Some(id);
            }
        }

        // Track starts safely past minus button (minus button rect is x0-13 .. x0+13)
        // With thumb radius 6.0, track_left = x0 + 22.0 gives thumb left edge at x0 + 16.0 (3px clear gap)
        let track_left = x0 + 22.0;
        let track_width = 80.0;
        let track_right = track_left + track_width; // x0 + 102.0

        // Plus button (26x26, center at track_right + 22.0 = x0 + 124.0)
        // Plus button rect is x0 + 111.0 .. x0 + 137.0
        // When thumb is at max, thumb center is at x0 + 102.0, thumb right edge is at x0 + 108.0 (3px clear gap)
        let plus_center = Pos2::new(track_right + 22.0, cy);
        if icon_button(ui, plus_center, ic::PLUS, 13.0, can_zoom_in, tr("Zoom in (⌘+)")).clicked() {
            self.tile = (self.tile + 20.0).min(TILE_MAX).round();
            if let Some(id) = self.ws.selected.first().cloned() {
                self.ws.scroll_to = Some(id);
            }
        }

        // Expanded interactive hit area covering full track and thumb bounds
        let hit_track = Rect::from_min_max(
            Pos2::new(track_left - 8.0, cy - 15.0),
            Pos2::new(track_right + 8.0, cy + 15.0),
        );
        let resp = ui.interact(hit_track, ui.scope_id().with("zoom-slider"), Sense::click_and_drag());

        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }

        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.drag_started() || resp.dragged() || resp.clicked())
        {
            let f = ((pos.x - track_left) / track_width).clamp(0.0, 1.0);
            let new_tile = (TILE_MIN + f * (TILE_MAX - TILE_MIN)).round();
            if (new_tile - self.tile).abs() >= 1.0 {
                self.tile = new_tile;
                if let Some(id) = self.ws.selected.first().cloned() {
                    self.ws.scroll_to = Some(id);
                }
            }
        }

        let f = ((self.tile - TILE_MIN) / (TILE_MAX - TILE_MIN)).clamp(0.0, 1.0);
        let p = ui.painter();
        let y = cy;

        // Inactive background track
        p.line_segment([Pos2::new(track_left, y), Pos2::new(track_right, y)], Stroke::new(2.0, t::p().divider));

        // Active highlighted track
        let kx = track_left + f * track_width;
        if kx > track_left {
            p.line_segment([Pos2::new(track_left, y), Pos2::new(kx, y)], Stroke::new(2.0, t::p().primary));
        }

        // Thumb styling with visual feedback
        let thumb_r = if resp.dragged() { 7.5 } else if resp.hovered() { 7.0 } else { 6.0 };
        if resp.hovered() || resp.dragged() {
            p.circle_filled(Pos2::new(kx, y), thumb_r + 2.5, t::p().primary.gamma_multiply(0.35));
        }
        p.circle_filled(Pos2::new(kx, y), thumb_r, Color32::WHITE);
        resp.dragged() || resp.drag_started()
    }

    /// The import progress bar; clears itself a moment after finishing.
    pub(crate) fn progress_bar(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let snapshot = {
            let pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            (pr.total, pr.done, pr.failed, pr.current.clone(), pr.paused, pr.started)
        };
        let (total, done, failed, current, paused, started) = snapshot;
        if total == 0 {
            if let Some(worker) = &self.ws.thumb_worker {
                let tp = worker.progress();
                if tp.active && tp.total > 0 {
                    let f = (tp.done as f32 / tp.total as f32).clamp(0.0, 1.0);
                    let bar = Rect::from_min_size(rect.min, vec2(rect.width() * f, 2.0));
                    ui.painter().rect_filled(bar, 0.0, t::p().primary.gamma_multiply(0.8));
                    return true;
                }
            }
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

    #[test]
    fn zoom_slider_geometry_has_no_button_overlap() {
        let x0 = 100.0;
        let minus_rect = Rect::from_center_size(Pos2::new(x0, 20.0), Vec2::splat(26.0));
        let track_left = x0 + 22.0;
        let track_width = 80.0;
        let track_right = track_left + track_width;
        let plus_rect = Rect::from_center_size(Pos2::new(track_right + 22.0, 20.0), Vec2::splat(26.0));

        let thumb_r = 6.0;
        let min_thumb_left = track_left - thumb_r;
        let max_thumb_right = track_right + thumb_r;

        assert!(min_thumb_left > minus_rect.right(), "min thumb edge ({min_thumb_left}) must clear minus button right ({})", minus_rect.right());
        assert!(max_thumb_right < plus_rect.left(), "max thumb edge ({max_thumb_right}) must clear plus button left ({})", plus_rect.left());
    }

    #[test]
    fn search_and_toggle_button_spacing_prevents_overlap() {
        let bar_right = 1000.0;
        let show_inspector = false;
        let toggle_reserved = if !show_inspector { 44.0 } else { 16.0 };
        let search_right = bar_right - toggle_reserved;

        let toggle_center_x = bar_right - 24.0;
        let toggle_left_edge = toggle_center_x - 12.0; // button has 24px width
        let gap = toggle_left_edge - search_right;

        assert!(gap >= 8.0, "gap between search and toggle button must be at least 8px, got {gap}");
    }

    #[test]
    fn zoom_slider_quantizes_to_whole_pixels() {
        for f in [0.0, 0.12345, 0.33333, 0.5, 0.7777, 1.0] {
            let tile = (TILE_MIN + f * (TILE_MAX - TILE_MIN)).round();
            assert_eq!(tile.fract(), 0.0, "tile size must be an integer to prevent layout jitter");
            assert!((TILE_MIN..=TILE_MAX).contains(&tile));
        }
    }
}
