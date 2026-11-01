//! The toolbar's layout panel, after Eagle's: the layout and the grid's
//! fill mode, the sort, the caption and badge switches, the panel switches
//! and Refresh. The options live in `logic::Display`.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Ui, vec2};
use crate::app::{Action, App};
use falcon_library::Library;
use crate::icons as ic;
use falcon_library::search::OrderBy;
use crate::logic::{self, Layout};
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley, icon};
use crate::ui::organize::OrderTarget;
use crate::organize;
use falcon_library::model::FolderOrder;
use crate::ui::grid::{GRID_PAD_X, list_span};

/// The sort part of the panel: the view's own sort while a folder or smart
/// folder owns it, else the global one.
type OwnSort = Option<(OrderTarget, Option<(FolderOrder, bool)>)>;

/// A row with `label` on the left and a switch on the right; the whole row
/// flips `on`. Returns whether it changed.
fn switch_row(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, t::RADIUS, t::p().row_hover);
    }
    let g = galley(ui, label, font(13.0), t::p().text, rect.width() - 50.0);
    p.galley(Pos2::new(rect.left() + 6.0, rect.center().y - g.size().y / 2.0), g, t::p().text);
    switch(ui, Pos2::new(rect.right() - 22.0, rect.center().y), *on);
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
}

/// A small on/off switch centred at `c`.
fn switch(ui: &Ui, c: Pos2, on: bool) {
    let track = Rect::from_center_size(c, vec2(28.0, 16.0));
    let p = ui.painter();
    p.rect_filled(track, CornerRadius::same(8), if on { t::p().primary } else { t::p().button });
    let x = if on { track.right() - 8.0 } else { track.left() + 8.0 };
    p.circle_filled(Pos2::new(x, c.y), 6.0, Color32::WHITE);
}

/// A row of mutually exclusive buttons; returns the index clicked.
fn segments(ui: &mut Ui, labels: &[&str], current: usize) -> Option<usize> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, label) in labels.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == current, *label)).clicked() {
                picked = Some(i);
            }
        }
    });
    picked
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(t::p().text_muted).size(12.0));
}

impl App {
    pub(crate) fn layout_panel(&mut self, ui: &mut Ui, own: OwnSort) {
        ui.set_min_width(250.0);
        ui.set_max_width(250.0);
        heading(ui, "Layout");
        let labels: Vec<&str> = Layout::ALL.iter().map(|l| l.label()).collect();
        let cur = Layout::ALL.iter().position(|l| *l == self.ws.layout).unwrap_or(0);
        if let Some(i) = segments(ui, &labels, cur) {
            self.set_layout(Layout::ALL[i]);
        }
        if self.ws.layout == Layout::Grid {
            ui.add_space(4.0);
            heading(ui, "Fill mode");
            if let Some(i) = segments(ui, &["Fit", "Fill"], usize::from(self.display.fill)) {
                self.display.fill = i == 1;
            }
        }
        ui.add_space(4.0);
        // Eagle's Thumbnail option: Quality shows large cards from their
        // originals, Speed keeps the thumbnails.
        heading(ui, "Thumbnail");
        if let Some(i) = segments(ui, &["Speed", "Quality"], usize::from(self.display.quality)) {
            self.display.quality = i == 1;
        }
        ui.separator();
        self.panel_sort(ui, own);
        ui.separator();
        let d = &mut self.display;
        switch_row(ui, "Show Name", &mut d.show_name);
        switch_row(ui, "Show Item Info", &mut d.show_info);
        if d.show_info {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.menu_button(format!("{}  {}", d.info.label(), ic::CARET_DOWN), |ui| {
                    for k in logic::InfoType::ALL {
                        if ui.add(egui::Button::selectable(d.info == k, k.label())).clicked() {
                            d.info = k;
                        }
                    }
                });
            });
        }
        switch_row(ui, "Show Extension", &mut d.show_ext);
        switch_row(ui, "Show Extension Label", &mut d.ext_badge);
        switch_row(ui, "Show Comment Count", &mut d.comment_badge);
        switch_row(ui, "Show Subfolder Content", &mut d.subfolders);
        ui.separator();
        switch_row(ui, "Show Sidebar", &mut self.show_sidebar);
        switch_row(ui, "Show Inspector", &mut self.show_inspector);
        ui.separator();
        if ui.add_sized(vec2(ui.available_width(), 24.0), egui::Button::new("Refresh")).clicked() {
            self.refresh = true;
            ui.close();
        }
    }

    /// The panel's sort row: a menu of keys and the two directions.
    fn panel_sort(&mut self, ui: &mut Ui, own: OwnSort) {
        heading(ui, "Sort by");
        if let Some((target, cur)) = own {
            let label = match cur {
                Some((k, _)) => organize::order_label(k).to_string(),
                None => format!("Global ({})", self.ws.sort.label()),
            };
            ui.menu_button(format!("{label}  {}", ic::CARET_DOWN), |ui| self.own_sort_menu(ui, target, cur));
            return;
        }
        ui.menu_button(format!("{}  {}", self.ws.sort.label(), ic::CARET_DOWN), |ui| {
            for k in OrderBy::ALL {
                if ui.add(egui::Button::selectable(k == self.ws.sort, k.label())).clicked() {
                    self.ws.sort = k;
                }
            }
        });
        let labels = if self.ws.sort.is_time() { ["Newest First", "Oldest First"] } else { ["Ascending", "Descending"] };
        if let Some(i) = segments(ui, &labels, usize::from(!self.ws.increase)) {
            self.ws.increase = i == 0;
        }
    }

    /// The list layout's header: a click on a column sorts by it (again
    /// flips the direction), in the open folder's or smart folder's own
    /// sort or else the global one; a right-click picks the columns.
    pub(crate) fn list_header(&mut self, ui: &mut Ui, lib: &Library) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
        let row = Rect::from_min_max(Pos2::new(rect.left() + GRID_PAD_X, rect.top()), Pos2::new(rect.right() - GRID_PAD_X, rect.top() + 44.0));
        let own = self.own_sort(lib);
        let current = match &own {
            Some((_, Some((FolderOrder::By(k), inc)))) => Some((*k, *inc)),
            Some(_) => None,
            None => Some((self.ws.sort, self.ws.increase)),
        };
        let (x0, span) = list_span(row);
        let cy = rect.center().y;
        let mut picked = None;
        for (col, at, w) in self.display.columns.spans() {
            let cell = Rect::from_min_max(Pos2::new(x0 + span * at - 4.0, rect.top()), Pos2::new(x0 + span * (at + w) - 4.0, rect.bottom()));
            let r = ui.interact(cell, ui.scope_id().with(("list-header", col.label())), Sense::click());
            let active = current.is_some_and(|c| c.0 == col.sort_key());
            if r.hovered() {
                ui.painter().rect_filled(cell.shrink2(vec2(0.0, 3.0)), t::RADIUS, t::p().row_hover);
            }
            let color = if active { t::p().text } else { t::p().text_muted };
            let g = galley(ui, col.label(), font(12.0), color, cell.width() - 24.0);
            let gw = g.size().x;
            ui.painter().galley(Pos2::new(cell.left() + 4.0, cy - g.size().y / 2.0), g, color);
            if active {
                let arrow = if current.is_some_and(|c| c.1) { ic::CARET_DOWN } else { ic::CARET_UP };
                icon(ui, Pos2::new(cell.left() + gw + 14.0, cy), arrow, 11.0, color);
            }
            if r.clicked() {
                picked = Some(logic::header_sort(current, col.sort_key()));
            }
            r.context_menu(|ui| self.column_menu(ui));
        }
        resp.context_menu(|ui| self.column_menu(ui));
        ui.painter().hline(rect.x_range().shrink(GRID_PAD_X), rect.bottom() - 0.5, Stroke::new(1.0, t::p().divider));
        let Some((key, inc)) = picked else { return };
        match own {
            Some((OrderTarget::Folder(id), _)) => self.ws.actions.push(Action::Organize(crate::ui::organize::OrgAction::FolderOrder(id, Some((FolderOrder::By(key), inc))))),
            Some((OrderTarget::Smart(id), _)) => self.ws.actions.push(Action::Organize(crate::ui::organize::OrgAction::SmartOrder(id, Some((FolderOrder::By(key), inc))))),
            None => (self.ws.sort, self.ws.increase) = (key, inc),
        }
    }

    /// The header's menu: which optional columns show.
    fn column_menu(&mut self, ui: &mut Ui) {
        for col in logic::ListColumn::OPTIONAL {
            if let Some(on) = self.display.columns.flag(col) {
                ui.checkbox(on, col.label());
            }
        }
    }
}
