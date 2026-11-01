//! The filter bar's Date, Duration, BPM, Camera and Font chips (see the Search
//! and filters chapter of the docs). Everything goes through
//! [`Filter`], so `logic::visible` applies it.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, RichText, TextEdit, Ui};
use falcon_library::Library;
use falcon_library::search::{self, Clock, DateField, DateFilter, DatePreset, DurationUnit, FontFilter, Shape, TextFilter};

use super::filter_chip;
use crate::app::App;
use crate::search_state::FilterKind;
use crate::logic;
use crate::theme as t;

/// Text typed into the new chips, kept between frames.
#[derive(Default)]
pub(crate) struct MediaFilterUi {
    /// A filter whose menu should open next frame (Find ▸ Open Filter and its keys).
    pub(crate) open_chip: Option<crate::search_state::FilterKind>,
    /// A chip shown because its menu was opened from the + menu or a key,
    /// though it is not pinned; it goes when the menu closes.
    pub(crate) showing: Option<crate::search_state::FilterKind>,
    /// The bar's Lock: keep the filter and search when another view opens.
    pub(crate) locked: bool,
    /// Text typed into the Tags, Folders and Type chips' search boxes.
    pub(crate) tag_search: String,
    pub(crate) folder_search: String,
    pub(crate) type_search: String,
    /// The Tags chip's sidebar: all tags, the selected ones, or a group.
    pub(crate) tag_view: super::TagView,
    /// The Date chip's tab: Imported, Modified or Created.
    date_tab: usize,
    /// Range fields per tab, as typed.
    from: [String; 3],
    to: [String; 3],
    dur_min: String,
    dur_max: String,
    dur_unit: DurationUnit,
    bpm_min: String,
    bpm_max: String,
    /// Width and height bounds as typed: min and max for each.
    w_min: String,
    w_max: String,
    h_min: String,
    h_max: String,
    size_min: String,
    size_max: String,
    /// The Size chip's unit is MB (otherwise KB).
    size_mb: bool,
    /// Keywords typed into the Comment chip.
    pub(crate) comment_text: String,
    /// The custom shape's W and H last used, restored when it is ticked again.
    custom_last: Option<(f64, f64)>,
    /// Keys of the fonts activated through Eagle, read once.
    font_keys: Option<HashSet<String>>,
}

impl MediaFilterUi {
    /// Reads Eagle's font folder again next time (after falcon activated
    /// or deactivated a font).
    pub(crate) fn forget_font_keys(&mut self) {
        self.font_keys = None;
    }
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
    ui.label(RichText::new(text).color(t::p().text_muted).size(12.0));
}

impl App {
    /// One of the chips kept here (Dimensions, Size, Comment, Date,
    /// Duration, BPM, Camera, Font); the filter bar decides which show.
    pub(crate) fn media_chip(&mut self, ui: &mut Ui, lib: &Library, kind: FilterKind) {
        let items = lib.items();
        let live = || items.iter().filter(|i| !i.is_deleted);
        let fui = &mut self.ws.filter_ui;
        let f = &mut self.ws.filter;
        match kind {
            FilterKind::Dimensions => {
                // Dimensions: width and height bounds in pixels.
                let dims = (f.width, f.height);
                if dims_fields_stale(&fui.w_min, &fui.w_max, &fui.h_min, &fui.h_max, dims) {
                    let t = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or_default();
                    (fui.w_min, fui.w_max, fui.h_min, fui.h_max) = (t(f.width.0), t(f.width.1), t(f.height.0), t(f.height.1));
                }
                let resp = filter_chip(ui, "Dimensions", dimensions_summary(f.width, f.height));
                menu(&resp, |ui| {
                    ui.set_min_width(200.0);
                    let mut changed = false;
                    for (name, lo, hi) in [("W", &mut fui.w_min, &mut fui.w_max), ("H", &mut fui.h_min, &mut fui.h_max)] {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            changed |= ui.add(TextEdit::singleline(lo).desired_width(64.0).hint_text("Min")).changed();
                            ui.label("–");
                            changed |= ui.add(TextEdit::singleline(hi).desired_width(64.0).hint_text("Max")).changed();
                            muted(ui, "px");
                        });
                    }
                    if changed {
                        f.width = (parse_whole(&fui.w_min), parse_whole(&fui.w_max));
                        f.height = (parse_whole(&fui.h_min), parse_whole(&fui.h_max));
                    }
                    if (f.width, f.height) != ((None, None), (None, None)) && ui.button("Clear").clicked() {
                        f.width = (None, None);
                        f.height = (None, None);
                        (fui.w_min, fui.w_max, fui.h_min, fui.h_max) = Default::default();
                    }
                });
            }
            FilterKind::Size => {
                // Size: file size in KB or MB.
                if size_fields_stale(&fui.size_min, &fui.size_max, fui.size_mb, f.size) {
                    // A saved filter set the rule: show its bounds in KB.
                    let kb = |v: Option<u64>| v.map(|b| format!("{}", b as f64 / 1024.0)).unwrap_or_default();
                    fui.size_min = kb(f.size.0);
                    fui.size_max = kb(f.size.1);
                    fui.size_mb = false;
                }
                let resp = filter_chip(ui, "Size", size_summary(f.size, fui.size_mb));
                menu(&resp, |ui| {
                    ui.set_min_width(220.0);
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        changed |= ui.add(TextEdit::singleline(&mut fui.size_min).desired_width(64.0).hint_text("Min")).changed();
                        ui.label("–");
                        changed |= ui.add(TextEdit::singleline(&mut fui.size_max).desired_width(64.0).hint_text("Max")).changed();
                        let before = fui.size_mb;
                        egui::ComboBox::from_id_salt("size-unit").width(56.0).selected_text(if fui.size_mb { "MB" } else { "KB" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut fui.size_mb, false, "KB");
                            ui.selectable_value(&mut fui.size_mb, true, "MB");
                        });
                        changed |= before != fui.size_mb;
                    });
                    if changed {
                        f.size = (parse_size(&fui.size_min, fui.size_mb), parse_size(&fui.size_max, fui.size_mb));
                    }
                    if f.size != (None, None) && ui.button("Clear").clicked() {
                        f.size = (None, None);
                        fui.size_min.clear();
                        fui.size_max.clear();
                    }
                });
            }
            FilterKind::Comment => {
                // Comment: the same menu as Notes and URL, over an item's comments.
                let resp = filter_chip(ui, "Comment", text_filter_summary(&f.comments));
                menu(&resp, |ui| super::text_filter_menu(ui, &mut f.comments, &mut fui.comment_text));
            }
            FilterKind::Date => {
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
            }
            FilterKind::Duration => {
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
            }
            FilterKind::Bpm => {
                // BPM: min and max, whole numbers as Eagle's parseInt reads them.
                let tempo = live().filter_map(search::bpm).fold(None, |r: Option<(f64, f64)>, b| Some(r.map_or((b, b), |(lo, hi)| (lo.min(b), hi.max(b)))));
                if tempo.is_some() || f.bpm != (None, None) {
                    if bpm_fields_stale(&fui.bpm_min, &fui.bpm_max, f.bpm) {
                        // A saved filter set the rule: show its bounds.
                        fui.bpm_min = f.bpm.0.map(|b| b.trunc().to_string()).unwrap_or_default();
                        fui.bpm_max = f.bpm.1.map(|b| b.trunc().to_string()).unwrap_or_default();
                    }
                    let resp = filter_chip(ui, "BPM", bpm_summary(f.bpm));
                    menu(&resp, |ui| {
                        ui.set_min_width(160.0);
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            let (lo, hi) = tempo.map_or(("Min".to_string(), "Max".to_string()), |(a, b)| (a.trunc().to_string(), b.trunc().to_string()));
                            changed |= ui.add(TextEdit::singleline(&mut fui.bpm_min).desired_width(56.0).hint_text(lo)).changed();
                            ui.label("–");
                            changed |= ui.add(TextEdit::singleline(&mut fui.bpm_max).desired_width(56.0).hint_text(hi)).changed();
                        });
                        if changed {
                            f.bpm = (parse_bpm(&fui.bpm_min), parse_bpm(&fui.bpm_max));
                        }
                        if f.bpm != (None, None) && ui.button("Clear").clicked() {
                            f.bpm = (None, None);
                            fui.bpm_min.clear();
                            fui.bpm_max.clear();
                        }
                    });
                }
            }
            FilterKind::Camera => {
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
            }
            FilterKind::Font => {
                // Font: activated (through Eagle or falcon) or not; one at a time.
                let has_fonts = live().any(|i| search::font_key(i).is_some());
                if has_fonts || f.font != FontFilter::Off {
                    let keys = fui.font_keys.get_or_insert_with(|| search::installed_font_keys_in(&search::font_dirs()));
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
            _ => {}
        }
    }

    /// Clears the text typed into the chips (the bar's Clear), keeping the
    /// Lock and any menu being opened.
    pub(crate) fn clear_media_filter_text(&mut self) {
        let old = std::mem::take(&mut self.ws.filter_ui);
        self.ws.filter_ui = MediaFilterUi { font_keys: old.font_keys, locked: old.locked, open_chip: old.open_chip, showing: old.showing, ..Default::default() };
    }

    /// Refills the typed fields from the filter after a saved filter
    /// replaced it: date ranges as days, durations in seconds.
    pub(crate) fn sync_media_filter_text(&mut self) {
        self.clear_media_filter_text();
        let f = &self.ws.filter;
        let fui = &mut self.ws.filter_ui;
        for (n, (field, _)) in DATE_TABS.iter().enumerate() {
            let rule = match field {
                DateField::Imported => &f.imported,
                DateField::Modified => &f.modified,
                DateField::Created => &f.created,
            };
            if let Some((a, b)) = rule.range {
                fui.from[n] = logic::day_text(a);
                fui.to[n] = logic::day_text(b);
            }
        }
        let secs = |v: Option<f64>| v.map(|s| s.to_string()).unwrap_or_default();
        fui.dur_min = secs(f.duration.0);
        fui.dur_max = secs(f.duration.1);
        if let TextFilter::Keywords(k) = &f.comments {
            fui.comment_text = k.join(", ");
        }
    }

    /// The Shape chip: every shape the library has (or that is ticked)
    /// with its count, plus a Custom W:H option.
    pub(crate) fn shape_chip(ui: &mut Ui, f: &mut search::Filter, fui: &mut MediaFilterUi, counts: &HashMap<Shape, usize>) {
        let resp = filter_chip(ui, "Shape", shape_summary(&f.shapes, f.custom_shape));
        menu(&resp, |ui| {
            ui.set_min_width(200.0);
            for shape in Shape::ALL {
                let n = counts.get(&shape).copied().unwrap_or(0);
                let mut on = f.shapes.contains(&shape);
                if (n > 0 || on) && ui.checkbox(&mut on, format!("{}  ({n})", shape_label(shape))).changed() {
                    if on {
                        f.shapes.insert(shape);
                    } else {
                        f.shapes.remove(&shape);
                    }
                }
            }
            let mut custom = f.custom_shape.is_some();
            if ui.checkbox(&mut custom, "Custom").changed() {
                f.custom_shape = custom.then(|| fui.custom_last.unwrap_or((4.0, 3.0)));
            }
            if let Some((mut w, mut h)) = f.custom_shape {
                ui.horizontal(|ui| {
                    ui.label("W");
                    let a = ui.add(egui::DragValue::new(&mut w).range(0.0..=f64::MAX).speed(0.1));
                    ui.label("H");
                    let b = ui.add(egui::DragValue::new(&mut h).range(0.0..=f64::MAX).speed(0.1));
                    if a.changed() || b.changed() {
                        f.custom_shape = Some((w, h));
                        fui.custom_last = Some((w, h));
                    }
                });
                muted(ui, "A side left at 0 is not used");
            }
            if (!f.shapes.is_empty() || f.custom_shape.is_some()) && ui.button("Clear").clicked() {
                f.shapes.clear();
                f.custom_shape = None;
            }
        });
    }
}

impl App {
    /// The search box's Search Scope menu: which parts of each item the
    /// search looks at, remembered across launches.
    pub(crate) fn search_scope_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(190.0);
        muted(ui, "Search in");
        let s = &mut self.search.scope;
        for (on, label) in [
            (&mut s.name, "Name"),
            (&mut s.folder_name, "Folder Name"),
            (&mut s.folder_desc, "Folder Description"),
            (&mut s.ext, "Extension"),
            (&mut s.tags, "Tags"),
            (&mut s.url, "URL"),
            (&mut s.comments, "Comments"),
            (&mut s.note, "Notes"),
        ] {
            ui.checkbox(on, label);
        }
    }
}

fn unit_label(u: DurationUnit) -> &'static str {
    match u {
        DurationUnit::Seconds => "s",
        DurationUnit::Minutes => "min",
        DurationUnit::Hours => "h",
    }
}

/// A BPM bound as typed, read the way Eagle's `parseInt` reads it: leading
/// spaces, an optional sign, then digits up to the first other character
/// ("120.5" and "120 bpm" give 120). `None` when there are no digits.
pub fn parse_bpm(text: &str) -> Option<f64> {
    let t = text.trim_start();
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1.0, &t[1..]),
        Some(b'+') => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<f64>().ok().map(|n| sign * n)
}

/// The BPM chip's value: `90 – 128`, `≥ 90` or `≤ 128`, with each bound
/// truncated as the search applies it.
pub fn bpm_summary(range: (Option<f64>, Option<f64>)) -> Option<String> {
    let n = |b: f64| b.trunc().to_string();
    match range {
        (None, None) => None,
        (Some(a), None) => Some(format!("≥ {}", n(a))),
        (None, Some(b)) => Some(format!("≤ {}", n(b))),
        (Some(a), Some(b)) => Some(format!("{} – {}", n(a), n(b))),
    }
}

/// Whether the typed fields no longer describe the rule (a saved filter or
/// the bar's Clear changed it), so they should be refilled from it.
fn bpm_fields_stale(min: &str, max: &str, range: (Option<f64>, Option<f64>)) -> bool {
    let t = |b: Option<f64>| b.map(f64::trunc);
    (parse_bpm(min), parse_bpm(max)) != (t(range.0), t(range.1))
}

/// A whole-number bound as typed, read like `$.isNumeric(text)` then
/// `parseInt(text)`: the trimmed text must be a finite number, which is cut
/// toward zero; negatives become 0.
pub fn parse_whole(text: &str) -> Option<u64> {
    let v: f64 = text.trim().parse().ok().filter(|v: &f64| v.is_finite())?;
    Some(v.trunc().max(0.0) as u64)
}

/// A file size bound as typed in KB or MB (fractions allowed), in bytes cut
/// toward zero. `None` for text that is not a finite non-negative number.
pub fn parse_size(text: &str, mb: bool) -> Option<u64> {
    let v: f64 = text.trim().parse().ok().filter(|v: &f64| v.is_finite() && *v >= 0.0)?;
    let unit = if mb { 1_048_576.0 } else { 1024.0 };
    Some((v * unit).trunc() as u64)
}

fn range_text(prefix: &str, (lo, hi): (Option<String>, Option<String>), sep: &str) -> Option<String> {
    let p = |s: String| if prefix.is_empty() { s } else { format!("{prefix} {s}") };
    match (lo, hi) {
        (None, None) => None,
        (Some(a), None) => Some(p(format!("\u{2265} {a}"))),
        (None, Some(b)) => Some(p(format!("\u{2264} {b}"))),
        (Some(a), Some(b)) => Some(p(format!("{a}{sep}{b}"))),
    }
}

/// The Dimensions chip's value: `W ≥ 100, H 200–800`.
pub fn dimensions_summary(w: (Option<u64>, Option<u64>), h: (Option<u64>, Option<u64>)) -> Option<String> {
    let part = |name: &str, r: (Option<u64>, Option<u64>)| range_text(name, (r.0.map(|n| n.to_string()), r.1.map(|n| n.to_string())), "\u{2013}");
    let parts: Vec<String> = [part("W", w), part("H", h)].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// The Size chip's value in the unit the user picked: `≥ 2 MB`, `10–500 KB`.
pub fn size_summary(r: (Option<u64>, Option<u64>), mb: bool) -> Option<String> {
    let (div, unit) = if mb { (1_048_576.0, "MB") } else { (1024.0, "KB") };
    let n = |b: u64| format!("{}", b as f64 / div);
    range_text("", (r.0.map(n), r.1.map(n)), "\u{2013}").map(|s| format!("{s} {unit}"))
}

/// A Notes/URL/Comment chip's value: Has, None or the keywords.
pub fn text_filter_summary(t: &TextFilter) -> Option<String> {
    match t {
        TextFilter::Off => None,
        TextFilter::Has => Some("Has".into()),
        TextFilter::No => Some("None".into()),
        TextFilter::Keywords(k) => Some(k.join(", ")),
    }
}

/// The name of a shape option in the Shape chip.
pub fn shape_label(s: Shape) -> &'static str {
    match s {
        Shape::Landscape => "Horizontal",
        Shape::Portrait => "Vertical",
        Shape::Square => "Square",
        Shape::PanoramicLandscape => "Panoramic Horizontal",
        Shape::PanoramicPortrait => "Panoramic Vertical",
        Shape::Ratio4x3 => "4:3",
        Shape::Ratio3x4 => "3:4",
        Shape::Ratio16x9 => "16:9",
        Shape::Ratio9x16 => "9:16",
    }
}

/// The Shape chip's value: the ticked options in menu order, the custom
/// one as `W:H`.
pub fn shape_summary(shapes: &HashSet<Shape>, custom: Option<(f64, f64)>) -> Option<String> {
    let mut parts: Vec<String> = Shape::ALL.into_iter().filter(|s| shapes.contains(s)).map(|s| shape_label(s).to_string()).collect();
    if let Some((w, h)) = custom {
        parts.push(format!("{w}:{h}"));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

type Bound = (Option<u64>, Option<u64>);

fn dims_fields_stale(wl: &str, wh: &str, hl: &str, hh: &str, (w, h): (Bound, Bound)) -> bool {
    ((parse_whole(wl), parse_whole(wh)), (parse_whole(hl), parse_whole(hh))) != (w, h)
}

fn size_fields_stale(min: &str, max: &str, mb: bool, r: (Option<u64>, Option<u64>)) -> bool {
    (parse_size(min, mb), parse_size(max, mb)) != r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpm_fields_parse_like_parse_int() {
        assert_eq!(parse_bpm("120"), Some(120.0));
        assert_eq!(parse_bpm("  98.7"), Some(98.0));
        assert_eq!(parse_bpm("140 bpm"), Some(140.0));
        assert_eq!(parse_bpm("+90"), Some(90.0));
        assert_eq!(parse_bpm("-5"), Some(-5.0));
        assert_eq!(parse_bpm(""), None);
        assert_eq!(parse_bpm("fast"), None);
        assert_eq!(parse_bpm("-"), None);
        assert_eq!(parse_bpm(".5"), None);
    }

    #[test]
    fn bpm_chip_labels() {
        assert_eq!(bpm_summary((None, None)), None);
        assert_eq!(bpm_summary((Some(90.0), None)).as_deref(), Some("≥ 90"));
        assert_eq!(bpm_summary((None, Some(128.5))).as_deref(), Some("≤ 128"));
        assert_eq!(bpm_summary((Some(90.0), Some(128.0))).as_deref(), Some("90 – 128"));
    }

    #[test]
    fn bpm_fields_follow_saved_filters() {
        assert!(!bpm_fields_stale("", "", (None, None)));
        assert!(!bpm_fields_stale("90", "128 ", (Some(90.0), Some(128.0))));
        assert!(!bpm_fields_stale("fast", "", (None, None)), "unreadable text is no rule");
        assert!(!bpm_fields_stale("128", "", (Some(128.5), None)), "a saved fraction is truncated");
        assert!(bpm_fields_stale("", "", (Some(90.0), None)), "a saved filter set a bound");
        assert!(bpm_fields_stale("90", "", (None, None)), "the rule was cleared");
    }

    #[test]
    fn whole_numbers_like_is_numeric_parse_int() {
        assert_eq!(parse_whole("100"), Some(100));
        assert_eq!(parse_whole(" 99.9 "), Some(99));
        assert_eq!(parse_whole("-5"), Some(0));
        assert_eq!(parse_whole("-0.5"), Some(0));
        assert_eq!(parse_whole("1e3"), Some(1000));
        assert_eq!(parse_whole("12px"), None);
        assert_eq!(parse_whole(""), None);
        assert_eq!(parse_whole("inf"), None);
        assert_eq!(parse_whole("NaN"), None);
    }

    #[test]
    fn sizes_in_kb_and_mb() {
        assert_eq!(parse_size("2", false), Some(2048));
        assert_eq!(parse_size("1.5", true), Some(1_572_864));
        assert_eq!(parse_size(" 0.0005 ", false), Some(0));
        assert_eq!(parse_size("1.0009765625", false), Some(1025));
        assert_eq!(parse_size("-1", false), None);
        assert_eq!(parse_size("x", true), None);
        assert_eq!(parse_size("inf", true), None);
    }

    #[test]
    fn dimension_and_size_chip_values() {
        assert_eq!(dimensions_summary((None, None), (None, None)), None);
        assert_eq!(dimensions_summary((Some(100), None), (None, None)).as_deref(), Some("W \u{2265} 100"));
        assert_eq!(dimensions_summary((Some(100), Some(200)), (None, Some(800))).as_deref(), Some("W 100\u{2013}200, H \u{2264} 800"));
        assert_eq!(size_summary((None, None), true), None);
        assert_eq!(size_summary((Some(2_097_152), None), true).as_deref(), Some("\u{2265} 2 MB"));
        assert_eq!(size_summary((Some(10_240), Some(512_000)), false).as_deref(), Some("10\u{2013}500 KB"));
        assert_eq!(size_summary((None, Some(3072)), false).as_deref(), Some("\u{2264} 3 KB"));
        assert_eq!(size_summary((None, Some(1_572_864)), true).as_deref(), Some("\u{2264} 1.5 MB"));
    }

    #[test]
    fn stale_fields_follow_the_rule() {
        assert!(!dims_fields_stale("", "", "", "", ((None, None), (None, None))));
        assert!(!dims_fields_stale("100", "", "", "9.5", ((Some(100), None), (None, Some(9)))));
        assert!(dims_fields_stale("", "", "", "", ((Some(1), None), (None, None))));
        assert!(!size_fields_stale("2", "", true, (Some(2_097_152), None)));
        assert!(size_fields_stale("2", "", false, (Some(2_097_152), None)));
        assert!(size_fields_stale("2", "", false, (None, None)));
    }

    #[test]
    fn text_and_shape_chip_values() {
        assert_eq!(text_filter_summary(&TextFilter::Off), None);
        assert_eq!(text_filter_summary(&TextFilter::Has).as_deref(), Some("Has"));
        assert_eq!(text_filter_summary(&TextFilter::No).as_deref(), Some("None"));
        assert_eq!(text_filter_summary(&TextFilter::Keywords(vec!["a".into(), "b".into()])).as_deref(), Some("a, b"));
        let none = HashSet::new();
        assert_eq!(shape_summary(&none, None), None);
        let two: HashSet<Shape> = [Shape::Ratio16x9, Shape::Landscape].into_iter().collect();
        assert_eq!(shape_summary(&two, None).as_deref(), Some("Horizontal, 16:9"));
        assert_eq!(shape_summary(&none, Some((2.35, 1.0))).as_deref(), Some("2.35:1"));
        assert_eq!(shape_summary(&two, Some((4.0, 3.0))).as_deref(), Some("Horizontal, 16:9, 4:3"));
        assert_eq!(shape_label(Shape::PanoramicPortrait), "Panoramic Vertical");
    }
}
