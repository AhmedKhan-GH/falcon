//! The toolbar's layout panel, after Eagle's: the layout and the grid's
//! fill mode, the sort, the caption and badge switches, the panel switches
//! and Refresh. The options live in `logic::Display`.

use super::*;
use super::organize_ui::OrderTarget;
use crate::organize;
use falcon_library::model::FolderOrder;

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
    pub(super) fn layout_panel(&mut self, ui: &mut Ui, own: OwnSort) {
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
}
