//! A calendar popup for date fields: a button that opens a month grid and
//! writes the clicked day into the field as `YYYY-MM-DD`.

use chrono::{Datelike, Local, NaiveDate};
use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::icons as ic;
use crate::locale::{tr, tr_key, trf};
use crate::logic::{add_months, format_ymd, month_grid, parse_ymd};
use crate::theme::{self as t, font};

const MONTHS: [&str; 12] = [
    tr_key("January"),
    tr_key("February"),
    tr_key("March"),
    tr_key("April"),
    tr_key("May"),
    tr_key("June"),
    tr_key("July"),
    tr_key("August"),
    tr_key("September"),
    tr_key("October"),
    tr_key("November"),
    tr_key("December"),
];
/// Weekday abbreviations, Monday first.
const WEEKDAYS: [&str; 7] = [tr_key("Mo"), tr_key("Tu"), tr_key("We"), tr_key("Th"), tr_key("Fr"), tr_key("Sa"), tr_key("Su")];
const CELL: egui::Vec2 = vec2(26.0, 22.0);

/// Draws the calendar button for the date in `value`; true when a day was picked.
pub(crate) fn date_button(ui: &mut Ui, id: egui::Id, value: &mut String) -> bool {
    let button = ui.add(egui::Button::new(RichText::new(ic::CARET_DOWN).color(t::p().text_muted)).small()).on_hover_text(tr("Pick a date"));
    let month_id = id.with("date-picker-month");
    if button.clicked() {
        // Start from the field's date, else today, each time the popup opens.
        let start = parse_ymd(value).unwrap_or_else(|| Local::now().date_naive());
        ui.data_mut(|d| d.insert_temp(month_id, (start.year(), start.month())));
    }
    let mut picked = false;
    egui::Popup::menu(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let today = Local::now().date_naive();
        let selected = parse_ymd(value);
        let (mut year, mut month) = ui.data(|d| d.get_temp::<(i32, u32)>(month_id)).unwrap_or((today.year(), today.month()));
        ui.horizontal(|ui| {
            if ui.small_button(ic::CARET_LEFT).clicked() {
                (year, month) = add_months(year, month, -1);
            }
            ui.add_sized([CELL.x * 5.0, CELL.y], egui::Label::new(RichText::new(trf("{month} {year}", &[("month", &tr(MONTHS[month as usize - 1])), ("year", &year)])).color(t::p().text)));
            if ui.small_button(ic::CARET_RIGHT).clicked() {
                (year, month) = add_months(year, month, 1);
            }
        });
        ui.data_mut(|d| d.insert_temp(month_id, (year, month)));
        let mut chosen: Option<NaiveDate> = None;
        egui::Grid::new(id.with("date-picker-grid")).spacing(vec2(2.0, 2.0)).show(ui, |ui| {
            for w in WEEKDAYS {
                ui.add_sized(CELL, egui::Label::new(RichText::new(tr(w)).color(t::p().text_muted)));
            }
            ui.end_row();
            for (i, cell) in month_grid(year, month).into_iter().enumerate() {
                match cell {
                    Some(date) => {
                        if day_cell(ui, date, date == today, Some(date) == selected) {
                            chosen = Some(date);
                        }
                    }
                    None => {
                        ui.allocate_space(CELL);
                    }
                }
                if i % 7 == 6 {
                    ui.end_row();
                }
            }
        });
        if let Some(date) = chosen {
            *value = format_ymd(date);
            picked = true;
            ui.close();
        }
    });
    picked
}

fn day_cell(ui: &mut Ui, date: NaiveDate, is_today: bool, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(CELL, Sense::click());
    let radius = CornerRadius::same(4);
    if selected {
        ui.painter().rect_filled(rect, radius, t::p().primary);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, radius, t::p().button_hover);
    }
    if is_today {
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
    }
    let color = if selected { t::p().text } else { t::p().text_value };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, date.day().to_string(), font(12.0), color);
    resp.clicked()
}
