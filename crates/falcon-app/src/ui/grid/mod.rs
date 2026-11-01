//! The card grid and list view of the current scope, with its geometry,
//! selection and hover preview.

use std::collections::HashSet;

use eframe::egui::{self, Align2, CornerRadius, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::{App, DragItems, Marquee};
use crate::icons as ic;
use crate::loader::{Tier, job_for};
use crate::logic::{self, Layout, Scope};
use crate::theme::{self as t, font};
use crate::ui::widgets::{PIN_BADGE, badge, ext_placeholder, galley, paint_thumb_bg, pin_badge};

pub(crate) mod covers;
pub(crate) mod duplicates;
pub(crate) mod drag_out;
pub(crate) mod empty;
pub(crate) use hints::CardHints;
mod hints;
mod hover_media;
mod tag_sidebar;
mod tags_page;
mod text_card;

/// Grid geometry, measured from Eagle at 1280 x 800.
pub(crate) const GRID_PAD_X: f32 = 17.0;
pub(crate) const GRID_PAD_TOP: f32 = 4.0;
pub(crate) const GRID_GAP: f32 = 8.0;
pub(crate) const ROW_GAP: f32 = 10.0;

/// The width kept free at a list row's right for its comment count.
const LIST_COUNT_GUTTER: f32 = 26.0;

/// Where the list layout's columns start in a row (after the thumbnail)
/// and how wide they are together.
pub(crate) fn list_span(row: Rect) -> (f32, f32) {
    let thumb = row.height() - 10.0;
    (row.left() + 6.0 + thumb + 12.0, row.width() - thumb - 18.0 - LIST_COUNT_GUTTER)
}

impl App {
    /// The scope's items in grid order, as indices into the library.
    pub(crate) fn scope_order(&mut self, lib: &Library) -> Vec<usize> {
        let only: Option<HashSet<String>> = match &self.ws.scope {
            _ if self.ws.folders.combined() => self.combined_items(lib),
            Scope::Smart(id) => Some(lib.smart_folder_items(id).into_iter().map(|i| i.id.clone()).collect()),
            Scope::Recent => {
                if self.recent_shown.as_ref().is_none_or(|(root, _)| root != lib.root()) {
                    self.recent_shown = Some((lib.root().to_path_buf(), self.recent_items.list(lib.root()).to_vec()));
                }
                self.recent_shown.as_ref().map(|(_, ids)| ids.iter().cloned().collect())
            }
            _ => None,
        };
        self.grid_order(lib, only.as_ref())
    }

    /// Keeps `ws.order` current while the viewer hides the grid, so the
    /// viewer's stepping and counter follow edits and deletions.
    pub(crate) fn refresh_order(&mut self, lib: &Library) {
        if self.scope_gate(lib).is_some() {
            self.ws.order.clear();
            return;
        }
        let order = self.scope_order(lib);
        self.ws.order = order.iter().map(|&i| lib.items()[i].id.clone()).collect();
    }

    pub(crate) fn grid(&mut self, ui: &mut Ui, lib: &Library) {
        let area = ui.max_rect();
        self.ws.cards.begin(ui.ctx().cumulative_frame_nr());
        // The grayscale preview grays the cards too, as Eagle's does.
        self.ws.cards.set_gray(self.modes.grayscale);
        self.ws.loader.set_grayscale(self.modes.grayscale);
        let scroll_by = self.ws.scroll.take();
        let order = self.scope_order(lib);
        if let Some(gate) = self.scope_gate(lib) {
            self.ws.order.clear();
            self.unlock_screen(ui, gate, lib);
            return;
        }
        self.sync_layout();
        self.update_card_hints(lib);
        let items: Vec<&Item> = order.iter().map(|&i| &lib.items()[i]).collect();
        let new_order: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        let order_changed = self.ws.hover.shown().is_some() && self.ws.order != new_order;
        self.ws.order = new_order;

        if self.ws.scope == Scope::Duplicates {
            self.duplicates_page(ui, lib);
            return;
        }
        if self.ws.scope == Scope::AllTags && self.ws.query.trim().is_empty() {
            self.tag_page(ui, lib);
            return;
        }
        self.subfolder_strip(ui, lib);
        if items.is_empty() {
            self.empty_view(ui, lib, area);
            return;
        }
        if self.ws.layout == Layout::List {
            self.list_header(ui, lib);
        }

        let layout = self.ws.layout;
        let width = area.width() - 2.0 * GRID_PAD_X;
        let aspects: Vec<f32> = items
            .iter()
            .map(|i| if i.width > 0 && i.height > 0 { i.width as f32 / i.height as f32 } else { 1.0 })
            .collect();
        let caption = logic::caption_height(layout, &self.display);
        let (places, content_h, cols) = logic::place(layout, &aspects, width, self.tile, GRID_GAP, caption, ROW_GAP);
        self.ws.columns = cols;
        let total_h = GRID_PAD_TOP + content_h + 20.0;
        let card_of = |n: usize| {
            let [x, y, w, h] = places[n];
            Rect::from_min_size(Pos2::new(GRID_PAD_X + x, GRID_PAD_TOP + y), vec2(w, h + caption))
        };

        let mut scroll = egui::ScrollArea::vertical().id_salt(("grid", format!("{:?}", self.ws.scope))).auto_shrink([false, false]);
        if let Some(id) = self.ws.scroll_to.take()
            && let Some(pos) = self.ws.order.iter().position(|x| *x == id)
        {
            let r = card_of(pos);
            let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
            let view_h = area.height();
            if r.top() < cur {
                scroll = scroll.vertical_scroll_offset(r.top() - GRID_PAD_TOP);
            } else if r.bottom() > cur + view_h {
                scroll = scroll.vertical_scroll_offset(r.bottom() + ROW_GAP - view_h);
            }
        }
        if let Some(how) = scroll_by {
            let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
            scroll = scroll.vertical_scroll_offset(how.offset(cur, area.height(), total_h));
        }
        // The rubber band ends with the button; near an edge it scrolls.
        let pointer = ui.ctx().pointer_latest_pos();
        if self.ws.marquee.is_some() && !ui.input(|i| i.pointer.primary_down()) {
            self.ws.marquee = None;
        }
        if self.ws.marquee.is_some()
            && let Some(p) = pointer
        {
            let step = logic::edge_scroll(p.y, area.top(), area.bottom());
            if step != 0.0 {
                let cur = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).unwrap_or(0.0);
                scroll = scroll.vertical_scroll_offset((cur + step).clamp(0.0, (total_h - area.height()).max(0.0)));
                ui.ctx().request_repaint();
            }
        }

        let mut jobs = Vec::new();
        let mut clicked: Option<(String, Modifiers)> = None;
        let mut opened: Option<String> = None;
        let mut double_clicked: Option<String> = None;
        let mut middle: Option<String> = None;
        let mut bg_clicked = false;
        let mut hovered: Option<(&Item, Rect, bool)> = None;
        let manual = self.manual_folder(lib);
        let mut band_rect: Option<Rect> = None;
        let out = scroll.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let bg = ui.allocate_rect(Rect::from_min_size(origin, vec2(area.width(), total_h.max(area.height()))), Sense::click_and_drag());
            if bg.clicked() {
                bg_clicked = true;
            }
            self.background_layout_menu(&bg, lib);
            if bg.drag_started_by(egui::PointerButton::Primary)
                && let Some(at) = ui.input(|i| i.pointer.press_origin())
            {
                let m = ui.input(|i| i.modifiers);
                let mode = logic::marquee_mode(m.command, m.shift);
                let base = if mode == logic::MarqueeMode::Replace { Vec::new() } else { self.ws.selected.clone() };
                self.ws.marquee = Some(Marquee { start: at - origin.to_vec2(), base, mode });
            }
            if let Some(mq) = &self.ws.marquee
                && let Some(p) = pointer
            {
                let now = p - origin.to_vec2();
                let band = logic::rect_between([mq.start.x, mq.start.y], [now.x, now.y]);
                let cards: Vec<logic::Place> = (0..items.len()).map(card_of).map(|r| [r.left(), r.top(), r.width(), r.height()]).collect();
                let hits = logic::rect_hits(&cards, band);
                let sel = logic::marquee_selection(&self.ws.order, &mq.base, &hits, mq.mode);
                if let Some(last) = hits.last().and_then(|&n| self.ws.order.get(n)) {
                    self.ws.anchor = Some(last.clone());
                }
                self.ws.selected = sel;
                let [x, y, w, h] = band;
                band_rect = Some(Rect::from_min_size(origin + vec2(x, y), vec2(w, h)));
            }
            let view = viewport.expand(200.0);
            for (n, item) in items.iter().enumerate() {
                let local = card_of(n);
                if local.bottom() < view.top() || local.top() > view.bottom() {
                    continue;
                }
                let card = local.translate(origin.to_vec2());
                let resp = ui.interact(card, ui.scope_id().with(("card", &item.id)), Sense::click_and_drag());
                if resp.drag_started() {
                    let ids = if self.ws.selected.contains(&item.id) { self.ws.selected.clone() } else { vec![item.id.clone()] };
                    resp.dnd_set_drag_payload(DragItems(ids));
                }
                if resp.secondary_clicked() && !self.ws.selected.contains(&item.id) {
                    self.ws.selected = vec![item.id.clone()];
                }
                let targets: Vec<String> = if self.ws.selected.contains(&item.id) { self.ws.selected.clone() } else { vec![item.id.clone()] };
                resp.context_menu(|ui| {
                    if self.item_menu(ui, lib, item, &targets) {
                        opened = Some(item.id.clone());
                    }
                });
                if resp.clicked() {
                    clicked = Some((item.id.clone(), ui.input(|i| i.modifiers)));
                }
                if resp.double_clicked() {
                    double_clicked = Some(item.id.clone());
                }
                // Eagle's wheel-button preference: a new window, never for audio.
                if resp.middle_clicked() && self.prefs.middle_button == crate::prefs::MiddleButton::NewWindow && !falcon_library::media::is_audio(&item.ext.to_lowercase()) {
                    middle = Some(item.id.clone());
                }
                let selected = self.ws.selected.contains(&item.id);
                self.card_anim(ui, lib, item, card, resp.hovered());
                match layout {
                    Layout::List => self.list_row(ui, lib, item, card, selected, resp.hovered(), n),
                    _ => self.card(ui, lib, item, card, caption, selected, resp.hovered()),
                }
                if resp.hovered() || self.hover_media.over_toggle(&item.id, ui.ctx().cumulative_frame_nr()) {
                    hovered = Some((item, card, self.zoom_button(ui, item, card, caption)));
                    self.card_media(ui, lib, item, card, caption, &resp);
                }
                if let Some(j) = job_for(item, lib.root(), Tier::Grid) {
                    jobs.push(j);
                }
            }
            if let Some(folder) = &manual {
                self.manual_drag(ui, folder, &places, caption, origin);
            }
            if let Some(r) = band_rect {
                ui.painter().rect_filled(r, CornerRadius::same(2), t::p().primary.gamma_multiply(0.15));
                ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
            }
        });
        let offset = out.state.clamped_offset().y;
        let scrolled = ui.ctx().data(|d| d.get_temp::<f32>(ui.scope_id().with("grid-offset"))).is_some_and(|o| (o - offset).abs() > 0.5);
        ui.ctx().data_mut(|d| d.insert_temp(ui.scope_id().with("grid-offset"), offset));
        self.hover_preview(ui, lib, hovered, scrolled, order_changed || clicked.is_some() || opened.is_some());
        self.ws.frame_jobs.extend(jobs);
        // Thumbnails first; large cards' originals after them.
        let (cards, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.ws.frame_jobs).into_iter().partition(|j| j.key.1 == Tier::Card);
        self.ws.frame_jobs = rest;
        self.ws.frame_jobs.extend(cards);
        // A middle click opens that one item, as Eagle's does.
        if let Some(id) = middle {
            self.open_preview_items(lib, &[id]);
        }
        if let Some((id, m)) = clicked {
            self.click(&id, m);
        } else if bg_clicked {
            self.ws.selected.clear();
        }
        if let Some(id) = &double_clicked
            && self.prefs.double_click == crate::prefs::DoubleClick::External
        {
            let files = crate::ui::viewer::open::originals(lib, std::slice::from_ref(id));
            crate::open_with::open_default(&files);
            self.used(&files);
        } else if let Some(id) = opened.or(double_clicked) {
            self.ws.viewer = Some(id);
        }
    }

    /// After `ids` leave the grid (trashed, restored, deleted or moved out
    /// of the open folder): when they were the whole selection, selects the
    /// item after them (or before, at the end), as Eagle does; otherwise
    /// just drops them from the selection.
    pub(crate) fn select_after_removal(&mut self, ids: &[String]) {
        if self.ws.selected.is_empty() || self.ws.selected.iter().any(|s| !ids.contains(s)) {
            self.ws.selected.retain(|s| !ids.contains(s));
            return;
        }
        let next = logic::next_after_removal(&self.ws.order, ids);
        self.ws.selected = next.iter().cloned().collect();
        self.ws.anchor.clone_from(&next);
        self.ws.scroll_to = next;
    }

    /// Before `ids` leave the grid (trashed or restored), moves the viewer
    /// off them to the next item, or the previous one at the end, or
    /// closes it when none is left.
    pub(crate) fn leave_viewed(&mut self, ids: &[String]) {
        let Some(cur) = self.ws.viewer.clone() else { return };
        if !ids.contains(&cur) {
            return;
        }
        let at = self.ws.order.iter().position(|x| *x == cur).unwrap_or(0);
        let after = self.ws.order.iter().skip(at + 1).find(|x| !ids.contains(x));
        let before = self.ws.order.iter().take(at).rev().find(|x| !ids.contains(x));
        self.ws.viewer = after.or(before).cloned();
    }

    /// The layout panel's Quality option: a card drawn larger than its grid
    /// render shows its original instead (decoded at up to
    /// `logic::CARD_TIER_SIDE`), once loaded. Asks for it while wanted.
    fn large_card(&mut self, ui: &Ui, lib: &Library, item: &Item, rect: Rect) -> Option<egui::TextureHandle> {
        let shown_px = rect.size().max_elem() * ui.ctx().pixels_per_point();
        // An original no bigger than the grid render has nothing to add.
        if item.width.max(item.height) as f32 <= crate::hover::GRID_SIDE {
            return None;
        }
        if !logic::wants_card_original(self.display.quality, &item.ext.to_lowercase(), shown_px, crate::hover::GRID_SIDE) {
            return None;
        }
        if let Some(tex) = self.ws.loader.get(&item.id, Tier::Card) {
            return Some(tex.clone());
        }
        let path = item.original_path(lib.root());
        // An original still in iCloud isn't downloaded for a card.
        if !self.ws.loader.has_failed(&item.id, Tier::Card)
            && !crate::icloud::is_dataless(&path)
            && let Some(j) = crate::loader::card_job(item, lib.root())
        {
            self.ws.frame_jobs.push(j);
        }
        None
    }

    /// Draws an item's image into `rect`: fitted without enlarging (grid),
    /// or filling it exactly (waterfall and justified boxes already have the
    /// image's shape).
    pub(crate) fn draw_image(&mut self, ui: &Ui, lib: &Library, item: &Item, rect: Rect, fill: bool) {
        if item.ext == "txt" {
            return self.text_card(ui, lib, item, rect);
        }
        let failed = self.ws.loader.has_failed(&item.id, Tier::Grid);
        let playing = self.ws.cards.texture(&item.id).cloned();
        let large = self.large_card(ui, lib, item, rect);
        match playing.or(large).or_else(|| self.ws.loader.get(&item.id, Tier::Grid).cloned()) {
            Some(tex) => {
                let natural = if item.width > 0 && item.height > 0 { vec2(item.width as f32, item.height as f32) } else { tex.size_vec2() };
                let scale = (rect.width() / natural.x).min(rect.height() / natural.y);
                // Eagle's object-fit: scale-down never enlarges a small image.
                let scale = if fill { scale } else { scale.min(1.0) };
                let r = Rect::from_center_size(rect.center(), natural * scale);
                egui::Image::new((tex.id(), r.size())).corner_radius(4).paint_at(ui, r);
            }
            None if failed || job_for(item, lib.root(), Tier::Grid).is_none() => ext_placeholder(ui, rect, &item.ext),
            None => {}
        }
    }

    /// Draws an item's image so it covers `rect`, cropping what overflows
    /// (the grid layout's Fill mode).
    pub(crate) fn draw_image_cover(&mut self, ui: &Ui, lib: &Library, item: &Item, rect: Rect) {
        if item.ext == "txt" {
            return self.text_card(ui, lib, item, rect);
        }
        let failed = self.ws.loader.has_failed(&item.id, Tier::Grid);
        let playing = self.ws.cards.texture(&item.id).cloned();
        let large = self.large_card(ui, lib, item, rect);
        match playing.or(large).or_else(|| self.ws.loader.get(&item.id, Tier::Grid).cloned()) {
            Some(tex) => {
                let size = tex.size_vec2();
                let s = (rect.width() / size.x).max(rect.height() / size.y);
                let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), vec2(rect.width() / (size.x * s), rect.height() / (size.y * s)));
                egui::Image::new((tex.id(), rect.size())).uv(uv).corner_radius(4).paint_at(ui, rect);
            }
            None if failed || job_for(item, lib.root(), Tier::Grid).is_none() => ext_placeholder(ui, rect, &item.ext),
            None => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn card(&mut self, ui: &Ui, lib: &Library, item: &Item, card: Rect, caption: f32, selected: bool, hovered: bool) {
        let tile = Rect::from_min_max(card.min, Pos2::new(card.right(), card.bottom() - caption));
        let grid = self.ws.layout == Layout::Grid;
        let d = self.display.clone();
        let p = ui.painter().clone();
        if grid {
            p.rect_filled(tile, t::RADIUS, t::p().main);
            let border = if hovered { t::p().card_hover } else { t::p().card_border };
            p.rect_stroke(tile, t::RADIUS, Stroke::new(1.0, border), StrokeKind::Inside);
            if let Some(bg) = logic::ThumbBg::of(item) {
                paint_thumb_bg(&p, tile.shrink(1.0), bg);
            }
            if d.fill {
                self.draw_image_cover(ui, lib, item, tile.shrink(1.0));
            } else {
                self.draw_image(ui, lib, item, tile.shrink(6.0), false);
            }
        } else {
            p.rect_filled(tile, CornerRadius::same(4), t::p().field);
            if let Some(bg) = logic::ThumbBg::of(item) {
                paint_thumb_bg(&p, tile, bg);
            }
            self.draw_image(ui, lib, item, tile, true);
            if hovered && !selected {
                p.rect_stroke(tile, CornerRadius::same(4), Stroke::new(1.0, t::p().card_hover), StrokeKind::Inside);
            }
        }
        let mut badge_x = 5.0;
        if self.shows_pin(item) {
            pin_badge(ui, tile.min + vec2(badge_x, 5.0));
            badge_x += PIN_BADGE + 2.0;
        }
        if d.ext_badge {
            badge(ui, tile.min + vec2(badge_x, 5.0), &item.ext);
        }
        if d.comment_badge {
            crate::ui::viewer::comments::count_badge(ui, tile, item);
        }
        if selected {
            p.rect_stroke(tile.expand(1.0), CornerRadius::same(7), Stroke::new(2.5, t::p().primary), StrokeKind::Inside);
        }
        if caption == 0.0 {
            return;
        }
        let mut y = tile.bottom() + 14.0;
        if d.show_name {
            let name = logic::caption_name(item, d.show_ext);
            let g = galley(ui, &name, font(13.0), t::p().text, tile.width());
            if selected {
                let pill = Rect::from_center_size(Pos2::new(tile.center().x, y), vec2(g.size().x + 10.0, 19.0));
                p.rect_filled(pill, CornerRadius::same(4), t::p().primary);
            }
            p.galley(Pos2::new(tile.center().x - g.size().x / 2.0, y - g.size().y / 2.0), g, t::p().text);
            y += 18.0;
        }
        if d.show_info {
            match logic::caption_info(item, d.info) {
                logic::Info::Text(text) => {
                    let g = galley(ui, &text, font(12.0), t::p().text_muted, tile.width());
                    p.galley(Pos2::new(tile.center().x - g.size().x / 2.0, y - g.size().y / 2.0), g, t::p().text_muted);
                }
                logic::Info::Stars(n) => {
                    for i in 0..5u8 {
                        let color = if i < n { t::p().icon_yellow } else { t::p().text_disabled };
                        let x = tile.center().x + (f32::from(i) - 2.0) * 12.0;
                        p.text(Pos2::new(x, y), Align2::CENTER_CENTER, "★", font(11.0), color);
                    }
                }
            }
        }
    }

    /// One row of the list layout: thumbnail, then the shown columns
    /// (`logic::ListColumns`), lined up with the header.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn list_row(&mut self, ui: &Ui, lib: &Library, item: &Item, row: Rect, selected: bool, hovered: bool, n: usize) {
        let p = ui.painter().clone();
        let bg = row.shrink2(vec2(0.0, 2.0));
        if selected {
            p.rect_filled(bg, t::RADIUS, t::p().primary.gamma_multiply(0.35));
        } else if hovered {
            p.rect_filled(bg, t::RADIUS, t::p().row_hover);
        } else if n % 2 == 1 {
            p.rect_filled(bg, t::RADIUS, t::p().row_hover.gamma_multiply(0.3));
        }
        let thumb = Rect::from_min_size(Pos2::new(row.left() + 6.0, row.top() + 5.0), Vec2::splat(row.height() - 10.0));
        p.rect_filled(thumb, CornerRadius::same(4), t::p().field);
        if let Some(bg) = logic::ThumbBg::of(item) {
            paint_thumb_bg(&p, thumb, bg);
        }
        self.draw_image(ui, lib, item, thumb.shrink(2.0), false);
        let cy = row.center().y;
        if self.display.comment_badge {
            crate::ui::viewer::comments::count_disc(ui, Pos2::new(row.right() - 18.0, cy), item);
        }
        let (x0, span) = list_span(row);
        let pinned = self.shows_pin(item);
        for (col, at, w) in self.display.columns.spans() {
            let (mut x, mut cw) = (x0 + span * at, span * w - 8.0);
            if pinned && col == logic::ListColumn::Name {
                pin_badge(ui, Pos2::new(x, cy - PIN_BADGE / 2.0));
                x += PIN_BADGE + 4.0;
                cw -= PIN_BADGE + 4.0;
            }
            let (size, color) = if col == logic::ListColumn::Name { (13.0, t::p().text) } else { (12.0, t::p().text_muted) };
            match logic::list_cell(item, col, self.display.show_ext) {
                logic::Info::Text(text) => {
                    let g = galley(ui, &text, font(size), color, cw);
                    p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, color);
                }
                logic::Info::Stars(lit) => {
                    for i in 0..5u8 {
                        let c = if i < lit { t::p().icon_yellow } else { t::p().text_disabled };
                        p.text(Pos2::new(x + f32::from(i) * 12.0, cy), Align2::LEFT_CENTER, "★", font(11.0), c);
                    }
                }
            }
        }
    }

    /// Draws a hovered card's zoom button, as Eagle does, and says whether
    /// the pointer is on it.
    pub(crate) fn zoom_button(&self, ui: &Ui, item: &Item, card: Rect, caption: f32) -> bool {
        if !self.prefs.hover_zoom || !crate::hover::has_zoom_button(&item.ext) {
            return false;
        }
        let list = self.ws.layout == Layout::List;
        let thumb = if list {
            Rect::from_min_size(Pos2::new(card.left() + 6.0, card.top() + 5.0), Vec2::splat(card.height() - 10.0))
        } else {
            Rect::from_min_max(card.min, Pos2::new(card.right(), card.bottom() - caption))
        };
        let rect = crate::hover::zoom_button_rect(thumb, self.tile, list);
        let hot = ui.rect_contains_pointer(rect);
        crate::hover::paint_button(ui.painter(), rect, list, hot, font(rect.width().min(24.0) * 0.6), ic::MAGNIFYING_GLASS);
        hot
    }

    /// Eagle's hover preview: an enlarged image over the window after the
    /// pointer rests on a card's zoom button, or at once while Z is held.
    pub(crate) fn hover_preview(&mut self, ui: &Ui, lib: &Library, hovered: Option<(&Item, Rect, bool)>, scrolled: bool, cancel: bool) {
        use crate::hover;
        let ctx = ui.ctx().clone();
        let (now, key, down) = ctx.input(|i| {
            let m = i.modifiers;
            (i.time, i.key_down(Key::Z) && !m.command && !m.ctrl && !m.shift, i.pointer.any_down())
        });
        let cancel = cancel
            || down
            || self.ws.viewer.is_some()
            || egui::DragAndDrop::has_any_payload(&ctx)
            || self.native_drag.load(std::sync::atomic::Ordering::Relaxed);
        let input = hover::Input {
            card: hovered.map(|h| h.0.id.as_str()),
            on_button: hovered.is_some_and(|h| h.2),
            key: key && ctx.memory(|m| m.focused().is_none()),
            scrolled,
            cancel,
            now,
        };
        if let Some(wait) = self.ws.hover.update(input) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        let fade = ctx.animate_bool_with_time(egui::Id::unique("hover-preview-fade"), self.ws.hover.shown().is_some(), 0.15);
        let Some((item, card, _)) = hovered.filter(|h| self.ws.hover.shown() == Some(h.0.id.as_str())) else {
            self.ws.cards.close_popup();
            return;
        };
        let grid = self.ws.loader.get(&item.id, Tier::Grid).cloned();
        let natural = match (item.width, item.height, &grid) {
            (w, h, _) if w > 0 && h > 0 => vec2(w as f32, h as f32),
            (_, _, Some(t)) => t.size_vec2(),
            _ => return,
        };
        let window = ctx.content_rect();
        let rect = hover::preview_rect(card, natural, window, f32::INFINITY);
        // GIFs and animated WebPs play in the preview.
        if let Some(tex) = self.hover_anim(&ctx, lib, item, rect) {
            hover::paint(&ctx, window, rect, tex.id(), t::p().panel, fade);
            return;
        }
        let original = hover::wants_original(&item.ext, item.width, item.height, rect.size() * ctx.pixels_per_point(), hover::GRID_SIDE);
        let full = if original {
            if let Some(j) = job_for(item, lib.root(), Tier::Full) {
                self.ws.frame_jobs.insert(0, j);
            }
            self.ws.loader.get(&item.id, Tier::Full).cloned()
        } else {
            None
        };
        if let Some(tex) = full.or(grid) {
            hover::paint(&ctx, window, rect, tex.id(), t::p().panel, fade);
        }
    }
}
