//! The filter bar's Date, Duration, Camera and Font chips (see the Search
//! and filters chapter of the docs). Everything goes through
//! [`Filter`], so `logic::visible` applies it.

use std::collections::HashSet;

use eframe::egui::{self, RichText, TextEdit, Ui};
use falcon_library::Library;
use falcon_library::search::{self, Clock, DateField, DateFilter, DatePreset, DurationUnit, FontFilter};

use super::{App, filter_chip};
use crate::logic;
use crate::theme as t;

/// Text typed into the new chips, kept between frames.
#[derive(Default)]
pub(super) struct MediaFilterUi {
    /// The Date chip's tab: Imported, Modified or Created.
    date_tab: usize,
    /// Range fields per tab, as typed.
    from: [String; 3],
    to: [String; 3],
    dur_min: String,
    dur_max: String,
    dur_unit: DurationUnit,
    /// Keys of the fonts activated through Eagle, read once.
    font_keys: Option<HashSet<String>>,
}

const DATE_TABS: [(DateField, &str); 3] = [(DateField::Imported, "Imported"), (DateField::Modified, "Modified"), (DateField::Created, "Created")];

fn date_rule(f: &mut search::Filter, field: DateField) -> &mut DateFilter {
    match field {
        DateField::Imported => &mut f.imported,
        DateField::Modified => &mut f.modified,
        DateField::Created => &mut f.created,
    }
}

fn menu(resp: &egui::Response, add: impl FnOnce(&mut Ui)) {
    egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(add);
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(t::TEXT_MUTED).size(12.0));
}

impl App {
    /// The chips after the existing ones. Duration, Camera and Font only
    /// show when the library has something they can match or they are on,
    /// as Eagle hides filters that are neither pinned nor active.
    pub(super) fn media_filter_chips(&mut self, ui: &mut Ui, lib: &Library) {
        let items = lib.items();
        let live = || items.iter().filter(|i| !i.is_deleted);
        let fui = &mut self.filter_ui;
        let f = &mut self.filter;

        // Date: one chip with a tab per timestamp, to keep the bar short.
        let rules = [&f.imported, &f.modified, &f.created];
        let active: Vec<usize> = (0..3).filter(|&n| rules[n].is_active()).collect();
        let value = match active.as_slice() {
            [] => None,
            [n] => logic::date_summary(rules[*n]).map(|s| format!("{} {s}", DATE_TABS[*n].1)),
            many => Some(many.len().to_string()),
        };
        let resp = filter_chip(ui, "Date", value);
        menu(&resp, |ui| {
            ui.set_min_width(240.0);
            ui.horizontal(|ui| {
                for (n, (_, name)) in DATE_TABS.iter().enumerate() {
                    let dot = if date_rule(f, DATE_TABS[n].0).is_active() { " •" } else { "" };
                    if ui.selectable_label(fui.date_tab == n, format!("{name}{dot}")).clicked() {
                        fui.date_tab = n;
                    }
                }
            });
            ui.separator();
            let tab = fui.date_tab.min(2);
            let field = DATE_TABS[tab].0;
            let clock = Clock::local();
            let rule = date_rule(f, field);
            for p in DatePreset::ALL {
                let n = live().filter(|i| p.matches(field.value(i) as i64, clock)).count();
                let mut on = rule.presets.contains(&p);
                if ui.checkbox(&mut on, format!("{}  ({n})", logic::preset_label(p))).changed() {
                    if on {
                        rule.presets.insert(p);
                    } else {
                        rule.presets.remove(&p);
                    }
                }
            }
            ui.separator();
            muted(ui, "Range");
            ui.horizontal(|ui| {
                let a = ui.add(TextEdit::singleline(&mut fui.from[tab]).desired_width(92.0).hint_text("2026-01-01"));
                ui.label("–");
                let b = ui.add(TextEdit::singleline(&mut fui.to[tab]).desired_width(92.0).hint_text("2026-12-31"));
                if a.changed() || b.changed() {
                    rule.range = logic::day_range(&fui.from[tab], &fui.to[tab]);
                }
            });
            let months = search::month_counts(live(), field);
            if !months.is_empty() {
                ui.separator();
                muted(ui, "Months");
                egui::ScrollArea::vertical().id_salt("date-months").max_height(220.0).show(ui, |ui| {
                    for (m, n) in months {
                        let mut on = rule.months.contains(&m);
                        if ui.checkbox(&mut on, RichText::new(format!("{}  ({n})", logic::month_label(m))).monospace()).changed() {
                            if on {
                                rule.months.insert(m);
                            } else {
                                rule.months.remove(&m);
                            }
                        }
                    }
                });
            }
            if rule.is_active() && ui.button("Clear").clicked() {
                *rule = DateFilter::default();
                fui.from[tab].clear();
                fui.to[tab].clear();
            }
        });

        // Duration: min and max with a unit, like Eagle's size field.
        let range = search::duration_range(live());
        if range.is_some() || f.duration != (None, None) {
            let resp = filter_chip(ui, "Duration", logic::duration_summary(f.duration));
            menu(&resp, |ui| {
                ui.set_min_width(240.0);
                let mut changed = false;
                ui.horizontal(|ui| {
                    let (lo, hi) = range.map_or((String::new(), String::new()), |(a, b)| (logic::format_duration(a), logic::format_duration(b)));
                    changed |= ui.add(TextEdit::singleline(&mut fui.dur_min).desired_width(64.0).hint_text(lo)).changed();
                    ui.label("–");
                    changed |= ui.add(TextEdit::singleline(&mut fui.dur_max).desired_width(64.0).hint_text(hi)).changed();
                    let before = fui.dur_unit;
                    egui::ComboBox::from_id_salt("duration-unit")
                        .width(56.0)
                        .selected_text(unit_label(fui.dur_unit))
                        .show_ui(ui, |ui| {
                            for u in [DurationUnit::Seconds, DurationUnit::Minutes, DurationUnit::Hours] {
                                ui.selectable_value(&mut fui.dur_unit, u, unit_label(u));
                            }
                        });
                    changed |= before != fui.dur_unit;
                });
                muted(ui, "A number in the unit, or a time such as 1:30");
                if changed {
                    f.duration = (logic::parse_duration(&fui.dur_min, fui.dur_unit), logic::parse_duration(&fui.dur_max, fui.dur_unit));
                }
                if f.duration != (None, None) && ui.button("Clear").clicked() {
                    f.duration = (None, None);
                    fui.dur_min.clear();
                    fui.dur_max.clear();
                }
            });
        }

        // Camera: every distinct camera with its count, multi-select.
        let cameras = logic::camera_counts(items);
        if !cameras.is_empty() || !f.cameras.is_empty() {
            let value = match f.cameras.len() {
                0 => None,
                1 => f.cameras.iter().next().cloned(),
                n => Some(n.to_string()),
            };
            let resp = filter_chip(ui, "Camera", value);
            menu(&resp, |ui| {
                ui.set_min_width(220.0);
                egui::ScrollArea::vertical().id_salt("cameras").max_height(300.0).show(ui, |ui| {
                    for (c, n) in &cameras {
                        let mut on = f.cameras.contains(c);
                        if ui.checkbox(&mut on, format!("{c}  ({n})")).changed() {
                            if on {
                                f.cameras.insert(c.clone());
                            } else {
                                f.cameras.remove(c);
                            }
                        }
                    }
                });
            });
        }

        // Font: activated (installed through Eagle) or not; one at a time.
        let has_fonts = live().any(|i| search::font_key(i).is_some());
        if has_fonts || f.font != FontFilter::Off {
            let keys = fui.font_keys.get_or_insert_with(|| search::eagle_font_dir().map(|d| search::installed_font_keys(&d)).unwrap_or_default());
            let value = match &f.font {
                FontFilter::Off => None,
                FontFilter::Activated(_) => Some("Activated".to_string()),
                FontFilter::Deactivated(_) => Some("Deactivated".to_string()),
            };
            let resp = filter_chip(ui, "Font", value);
            menu(&resp, |ui| {
                ui.set_min_width(200.0);
                let (on, off) = logic::font_counts(items, keys);
                let mut act = matches!(f.font, FontFilter::Activated(_));
                let mut deact = matches!(f.font, FontFilter::Deactivated(_));
                if ui.checkbox(&mut act, format!("Activated  ({on})")).changed() {
                    f.font = if act { FontFilter::Activated(keys.clone()) } else { FontFilter::Off };
                }
                if ui.checkbox(&mut deact, format!("Deactivated  ({off})")).changed() {
                    f.font = if deact { FontFilter::Deactivated(keys.clone()) } else { FontFilter::Off };
                }
                muted(ui, "Activated: installed by Eagle in ~/Library/Fonts/EagleApp");
            });
        }
    }

    /// Clears the text typed into the new chips (the bar's Clear).
    pub(super) fn clear_media_filter_text(&mut self) {
        let keys = self.filter_ui.font_keys.take();
        self.filter_ui = MediaFilterUi { font_keys: keys, ..Default::default() };
    }
}

fn unit_label(u: DurationUnit) -> &'static str {
    match u {
        DurationUnit::Seconds => "s",
        DurationUnit::Minutes => "min",
        DurationUnit::Hours => "h",
    }
}
