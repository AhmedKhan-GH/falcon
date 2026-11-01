//! The filter bar: the chips the user pinned (or that are on), the + menu
//! that pins them, Clear, the Lock and the saved filters (see the Search
//! and filters chapter of the docs). The Color, Tags, Folders, Rating,
//! Type, Notes and URL chips live here; the others in `ui/filter_bar/media.rs`.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::{Folder, Item, TagGroup};
use falcon_library::search::{self, ColorQuery, Filter, FontFilter, Logic, SetFilter, TextFilter};

pub(crate) mod media;
mod reorder;

use crate::app::App;
use crate::icons as ic;
use crate::locale::{tr, trf};
use crate::logic;
use crate::search_state::FilterKind;
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley, icon};
use crate::ui::grid::GRID_PAD_X;

/// What the Tags chip's sidebar shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum TagView {
    #[default]
    All,
    Selected,
    /// One tag group, by id.
    Group(String),
}

/// Whether a filter has a rule on.
pub fn kind_active(f: &Filter, kind: FilterKind) -> bool {
    match kind {
        FilterKind::Color => f.color.is_some() || f.grayscale,
        FilterKind::Tags => f.tags.is_active(),
        FilterKind::Folders => f.folders.is_active(),
        FilterKind::Shape => !f.shapes.is_empty() || f.custom_shape.is_some(),
        FilterKind::Rating => !f.ratings.is_empty(),
        FilterKind::Types => !f.types.is_empty() || !f.exclude_types.is_empty(),
        FilterKind::Dimensions => f.width != (None, None) || f.height != (None, None),
        FilterKind::Duration => f.duration != (None, None),
        FilterKind::Size => f.size != (None, None),
        FilterKind::Comment => f.comments != TextFilter::Off,
        FilterKind::Note => f.note != TextFilter::Off,
        FilterKind::Url => f.url != TextFilter::Off,
        FilterKind::Date => f.imported.is_active() || f.modified.is_active() || f.created.is_active(),
        FilterKind::Font => f.font != FontFilter::Off,
        FilterKind::Bpm => f.bpm != (None, None),
        FilterKind::Camera => !f.cameras.is_empty(),
    }
}

/// Whether a filter can match anything in the library: Duration, BPM,
/// Camera and Font need items that have one, as Eagle only offers them
/// then.
pub fn kind_applicable<'a>(kind: FilterKind, mut live: impl Iterator<Item = &'a Item>) -> bool {
    match kind {
        FilterKind::Duration => live.any(|i| search::duration(i).is_some()),
        FilterKind::Bpm => live.any(|i| search::bpm(i).is_some()),
        FilterKind::Camera => live.any(|i| search::camera(i).is_some()),
        FilterKind::Font => live.any(|i| search::font_key(i).is_some()),
        _ => true,
    }
}

/// How a row of an include/exclude list stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Off,
    In,
    Out,
}

/// Where a click on a row moves it: a click includes or un-includes, a
/// right-click excludes or un-excludes, as in Eagle's lists.
pub fn next_pick(now: Pick, secondary: bool) -> Pick {
    match (now, secondary) {
        (Pick::In, false) | (Pick::Out, true) => Pick::Off,
        (_, false) => Pick::In,
        (_, true) => Pick::Out,
    }
}

fn set_pick(includes: &mut Vec<String>, excludes: &mut Vec<String>, value: &str, pick: Pick) {
    includes.retain(|v| v != value);
    excludes.retain(|v| v != value);
    match pick {
        Pick::In => includes.push(value.to_string()),
        Pick::Out => excludes.push(value.to_string()),
        Pick::Off => {}
    }
}

fn pick_of(set: &SetFilter, value: &str) -> Pick {
    if set.includes.iter().any(|v| v == value) {
        Pick::In
    } else if set.excludes.iter().any(|v| v == value) {
        Pick::Out
    } else {
        Pick::Off
    }
}

/// A chip's value as Eagle writes it: the included names, then the
/// excluded ones with a leading minus, joined by commas.
pub fn pick_summary<'a>(includes: impl IntoIterator<Item = &'a str>, excludes: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let parts: Vec<String> = includes.into_iter().map(str::to_string).chain(excludes.into_iter().map(|e| format!("-{e}"))).collect();
    (!parts.is_empty()).then(|| parts.join(","))
}

/// One row of an include/exclude list. Returns the row's new state when it
/// was clicked.
fn pick_row(ui: &mut Ui, indent: f32, label: &str, count: Option<usize>, pick: Pick) -> Option<Pick> {
    let h = 22.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), h), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, t::RADIUS, t::p().row_hover);
    }
    let x = rect.left() + 6.0 + indent;
    let mark = egui::Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), Vec2::splat(13.0));
    match pick {
        Pick::Off => {
            ui.painter().rect_stroke(mark, 3.0, Stroke::new(1.0, t::p().text_muted), egui::StrokeKind::Inside);
        }
        Pick::In => {
            ui.painter().rect_filled(mark, 3.0, t::p().primary);
            icon(ui, mark.center(), ic::CHECK, 10.0, Color32::WHITE);
        }
        Pick::Out => {
            ui.painter().rect_filled(mark, 3.0, t::p().icon_red);
            icon(ui, mark.center(), ic::X, 10.0, Color32::WHITE);
        }
    }
    let p = ui.painter();
    let color = if pick == Pick::Out { t::p().text_muted } else { t::p().text };
    p.text(egui::pos2(x + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), color);
    if let Some(n) = count {
        p.text(egui::pos2(rect.right() - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, n.to_string(), egui::FontId::proportional(12.0), t::p().text_muted);
    }
    let resp = resp.on_hover_text(tr("Click to include, right-click to exclude"));
    if resp.clicked() {
        Some(next_pick(pick, false))
    } else if resp.secondary_clicked() {
        Some(next_pick(pick, true))
    } else {
        None
    }
}

fn search_box(ui: &mut Ui, text: &mut String) {
    ui.add(TextEdit::singleline(text).hint_text(format!("{}  {}", ic::MAGNIFYING_GLASS, tr("Search"))).desired_width(f32::INFINITY));
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(t::p().text_muted).size(12.0));
}

/// Tag names in the Tags chip's order without a search: tags in a group
/// first, by the position of their first group, then the others; names in
/// plain code-unit order within each.
pub fn tag_order(tags: &[(String, usize)], groups: &[TagGroup]) -> Vec<usize> {
    let group_of = |name: &str| groups.iter().position(|g| g.tags.iter().any(|t| t == name));
    let mut idx: Vec<usize> = (0..tags.len()).collect();
    idx.sort_by(|&a, &b| {
        let (ga, gb) = (group_of(&tags[a].0), group_of(&tags[b].0));
        let key = |g: Option<usize>| g.unwrap_or(usize::MAX);
        key(ga).cmp(&key(gb)).then_with(|| tags[a].0.encode_utf16().cmp(tags[b].0.encode_utf16()))
    });
    idx
}

/// Folders in tree order with their depth.
pub fn folders_with_depth(roots: &[Folder]) -> Vec<(&Folder, usize)> {
    fn walk<'a>(f: &'a Folder, depth: usize, out: &mut Vec<(&'a Folder, usize)>) {
        out.push((f, depth));
        for c in &f.children {
            walk(c, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for r in roots {
        walk(r, 0, &mut out);
    }
    out
}

/// A type option's name: the group names spelled out, extensions in
/// capitals.
pub fn type_label(t: &str) -> String {
    match t {
        "font" => tr("Font").into(),
        "video" => tr("Video").into(),
        "audio" => tr("Audio").into(),
        "youtube" => "YouTube".into(),
        "vimeo" => "Vimeo".into(),
        "bilibili" => "Bilibili".into(),
        "url" => tr("Bookmark").into(),
        ext => ext.to_uppercase(),
    }
}

impl App {
    pub(crate) fn filter_bar(&mut self, ui: &mut Ui, lib: &Library) {
        let r = ui.max_rect();
        ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t::p().divider));
        let inner = r.shrink2(vec2(GRID_PAD_X, 7.0));
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let ui = &mut row;
        ui.spacing_mut().item_spacing.x = 6.0;
        let live = || lib.items().iter().filter(|i| !i.is_deleted);
        let request = self.ws.filter_ui.open_chip.take();
        if request.is_some() {
            self.ws.filter_ui.showing = request;
        }
        let mut drawn: Vec<(FilterKind, egui::Rect)> = Vec::new();
        for kind in self.search.ordered() {
            let shown = kind_active(&self.ws.filter, kind)
                || self.ws.filter_ui.showing == Some(kind)
                || (self.search.is_pinned(kind) && kind_applicable(kind, live()));
            if !shown {
                continue;
            }
            let drawn_chip = ui.push_id(kind.id(), |ui| {
                let popup = ui.next_auto_id().with("popup");
                if request == Some(kind) {
                    egui::Popup::open_id(ui.ctx(), popup);
                }
                self.chip(ui, lib, kind);
                egui::Popup::is_id_open(ui.ctx(), popup)
            });
            drawn.push((kind, drawn_chip.response.rect));
            let open = drawn_chip.inner;
            if !open && request != Some(kind) && self.ws.filter_ui.showing == Some(kind) {
                self.ws.filter_ui.showing = None;
            }
        }
        self.reorder_chips(ui, &drawn);
        self.add_filter_button(ui, lib);

        if self.ws.filter.is_active() {
            let clear = ui.add(egui::Button::new(RichText::new(format!("{}  {}", ic::X, tr("Clear"))).color(t::p().text_muted)).frame(false));
            if clear.clicked() {
                self.reset_filter();
            }
        }
        if self.ws.filter.is_active() || !self.ws.query.trim().is_empty() {
            let locked = self.ws.filter_ui.locked;
            let (glyph, color) = if locked { (ic::LOCK_SIMPLE, t::p().primary) } else { (ic::LOCK_SIMPLE_OPEN, t::p().text_muted) };
            let tip = if locked { tr("Locked: the filter and search stay when you open another view") } else { tr("Lock the filter and search so they stay when you open another view") };
            if ui.add(egui::Button::new(RichText::new(glyph).color(color)).frame(false)).on_hover_text(tip).clicked() {
                self.ws.filter_ui.locked = !locked;
            }
        }
        self.saved_filters_menu(ui, lib);
    }

    /// The + button: every filter the library can use, each with a pin
    /// that keeps it in the bar; clicking a name opens that filter.
    fn add_filter_button(&mut self, ui: &mut Ui, lib: &Library) {
        let resp = ui.add(egui::Button::new(RichText::new(ic::PLUS).color(t::p().text_muted)).frame(false)).on_hover_text(tr("Add a filter to the bar"));
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(200.0);
            for kind in self.search.ordered() {
                if !kind_applicable(kind, lib.items().iter().filter(|i| !i.is_deleted)) {
                    continue;
                }
                ui.horizontal(|ui| {
                    let pinned = self.search.is_pinned(kind);
                    let pin = RichText::new(ic::PUSH_PIN).color(if pinned { t::p().primary } else { t::p().text_disabled });
                    if ui.add(egui::Button::new(pin).frame(false)).on_hover_text(if pinned { tr("Unpin from the bar") } else { tr("Pin to the bar") }).clicked() {
                        self.search.set_pinned(kind, !pinned);
                    }
                    if ui.add(egui::Button::new(tr(kind.label())).frame(false).min_size(vec2(140.0, 0.0))).clicked() {
                        self.ws.filter_ui.open_chip = Some(kind);
                        ui.close();
                    }
                });
            }
        });
    }

    fn chip(&mut self, ui: &mut Ui, lib: &Library, kind: FilterKind) {
        match kind {
            FilterKind::Color => self.color_chip(ui),
            FilterKind::Tags => self.tags_chip(ui, lib),
            FilterKind::Folders => self.folders_chip(ui, lib),
            FilterKind::Shape => {
                let counts = search::shape_counts(lib.items().iter().filter(|i| !i.is_deleted));
                Self::shape_chip(ui, &mut self.ws.filter, &mut self.ws.filter_ui, &counts);
            }
            FilterKind::Rating => self.rating_chip(ui, lib),
            FilterKind::Types => self.types_chip(ui, lib),
            FilterKind::Note | FilterKind::Url => {
                let (f, text) = if kind == FilterKind::Note { (&mut self.ws.filter.note, &mut self.ws.note_text) } else { (&mut self.ws.filter.url, &mut self.ws.url_text) };
                let resp = filter_chip(ui, tr(kind.label()), media::text_filter_summary(f));
                egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| text_filter_menu(ui, f, text));
            }
            _ => self.media_chip(ui, lib, kind),
        }
    }

    fn color_chip(&mut self, ui: &mut Ui) {
        let f = &mut self.ws.filter;
        let value = match (f.color, f.grayscale) {
            (Some(c), _) => Some(logic::hex(c.rgb)),
            (None, true) => Some(tr("Grayscale").to_string()),
            (None, false) => None,
        };
        let resp = filter_chip(ui, tr("Color"), value);
        let color_text = &mut self.ws.color_text;
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(230.0);
            let mut rgb = f.color.map_or(DEFAULT_PICKER_RGB, |q| q.rgb);
            if super::color_picker::sv_hue_picker(ui, ui.make_persistent_id("filter_color_picker"), &mut rgb) {
                let acc = f.color.map_or(20.0, |q| q.accuracy);
                f.color = Some(ColorQuery { rgb, accuracy: acc });
                *color_text = logic::hex(rgb);
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for c in PRESET_COLORS {
                    let (rect, hit) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                    ui.painter().circle_filled(rect.center(), 10.0, Color32::from_rgb(c[0], c[1], c[2]));
                    ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(1.0, t::p().divider));
                    if f.color.is_some_and(|q| q.rgb == c) {
                        ui.painter().circle_stroke(rect.center(), 12.5, Stroke::new(1.5, t::p().primary));
                    }
                    if hit.clicked() {
                        let acc = f.color.map_or(20.0, |q| q.accuracy);
                        f.color = Some(ColorQuery { rgb: c, accuracy: acc });
                        *color_text = logic::hex(c);
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tr("Hex"));
                if ui.add(TextEdit::singleline(color_text).desired_width(90.0).hint_text("#FF0000")).changed()
                    && let Some(rgb) = parse_hex(color_text)
                {
                    let acc = f.color.map_or(20.0, |q| q.accuracy);
                    f.color = Some(ColorQuery { rgb, accuracy: acc });
                }
                let (sw, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                match f.color {
                    Some(q) => ui.painter().rect_filled(sw, 4.0, Color32::from_rgb(q.rgb[0], q.rgb[1], q.rgb[2])),
                    None => ui.painter().rect_stroke(sw, 4.0, Stroke::new(1.0, t::p().text_disabled), egui::StrokeKind::Inside),
                }
            });
            if let Some(q) = f.color.as_mut() {
                ui.horizontal(|ui| {
                    ui.label(tr("Accuracy"));
                    // Eagle draws the slider reversed: right is stricter.
                    let mut strict = 45.0 - q.accuracy;
                    ui.add(egui::Slider::new(&mut strict, 5.0..=40.0).show_value(false));
                    q.accuracy = 45.0 - strict;
                });
            }
            ui.checkbox(&mut f.grayscale, tr("Grayscale only"));
            if ui.button(tr("Clear")).clicked() {
                f.color = None;
                f.grayscale = false;
                color_text.clear();
            }
        });
    }

    /// Tags: a search box, a sidebar (all tags, the selected ones, each
    /// group that has some) and the list; click includes, right-click
    /// excludes. A group's page can select all of its tags.
    fn tags_chip(&mut self, ui: &mut Ui, lib: &Library) {
        let tags = logic::tag_counts(lib.items());
        let untagged = lib.items().iter().filter(|i| !i.is_deleted && i.tags.is_empty()).count();
        let groups = &lib.metadata.tags_groups;
        let f = &mut self.ws.filter;
        let fui = &mut self.ws.filter_ui;
        let mut inc: Vec<&str> = f.tags.includes.iter().map(String::as_str).collect();
        if f.tags.empty {
            inc.insert(0, tr("Untagged"));
        }
        let resp = filter_chip(ui, tr("Tags"), pick_summary(inc, f.tags.excludes.iter().map(String::as_str)));
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(380.0);
            ui.horizontal(|ui| {
                logic_picker(ui, &mut f.tags.logic);
            });
            search_box(ui, &mut fui.tag_search);
            ui.separator();
            // The tags the search leaves, in their order.
            let keyword = fui.tag_search.trim();
            let order: Vec<usize> =
                if keyword.is_empty() { tag_order(&tags, groups) } else { search::rank_names(tags.iter().map(|(n, _)| n.as_str()), keyword) };
            let picked: std::collections::HashSet<String> = f.tags.includes.iter().chain(&f.tags.excludes).cloned().collect();
            let selected = |name: &str| picked.contains(name);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(120.0);
                    let row = |ui: &mut Ui, on: bool, label: String| ui.add(egui::Button::selectable(on, label).min_size(vec2(118.0, 0.0)));
                    if row(ui, fui.tag_view == TagView::All, trf("All Tags  {n}", &[("n", &order.len())])).clicked() {
                        fui.tag_view = TagView::All;
                    }
                    let n_sel = f.tags.includes.len() + f.tags.excludes.len();
                    if n_sel > 0 && row(ui, fui.tag_view == TagView::Selected, trf("Selected  {n}", &[("n", &n_sel)])).clicked() {
                        fui.tag_view = TagView::Selected;
                    }
                    for g in groups {
                        let n = order.iter().filter(|&&i| g.tags.contains(&tags[i].0)).count();
                        if n > 0 && row(ui, fui.tag_view == TagView::Group(g.id.clone()), format!("{}  {n}", g.name)).clicked() {
                            fui.tag_view = TagView::Group(g.id.clone());
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(240.0);
                    let group = match &fui.tag_view {
                        TagView::Group(id) => groups.iter().find(|g| &g.id == id),
                        _ => None,
                    };
                    let list: Vec<usize> = order
                        .iter()
                        .copied()
                        .filter(|&i| match &fui.tag_view {
                            TagView::All => true,
                            TagView::Selected => selected(&tags[i].0),
                            TagView::Group(_) => group.is_some_and(|g| g.tags.contains(&tags[i].0)),
                        })
                        .collect();
                    egui::ScrollArea::vertical().id_salt("filter-tags").max_height(300.0).show(ui, |ui| {
                        if fui.tag_view == TagView::All && keyword.is_empty() && (untagged > 0 || f.tags.empty) {
                            let now = if f.tags.empty { Pick::In } else { Pick::Off };
                            if let Some(p) = pick_row(ui, 0.0, tr("Untagged"), Some(untagged), now) {
                                f.tags.empty = p == Pick::In;
                            }
                        }
                        if list.is_empty() {
                            muted(ui, tr("No tags"));
                        }
                        for &i in &list {
                            let (name, n) = &tags[i];
                            if let Some(p) = pick_row(ui, 0.0, name, Some(*n), pick_of(&f.tags, name)) {
                                set_pick(&mut f.tags.includes, &mut f.tags.excludes, name, p);
                            }
                        }
                    });
                    if group.is_some() && !list.is_empty() {
                        let all = list.iter().all(|&i| f.tags.includes.contains(&tags[i].0));
                        let mut on = all;
                        if ui.checkbox(&mut on, tr("Select all")).changed() {
                            for &i in &list {
                                let name = &tags[i].0;
                                set_pick(&mut f.tags.includes, &mut f.tags.excludes, name, if on { Pick::In } else { Pick::Off });
                            }
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                muted(ui, tr("Click to include, right-click to exclude"));
                if f.tags.is_active() && ui.button(tr("Clear")).clicked() {
                    f.tags.includes.clear();
                    f.tags.excludes.clear();
                    f.tags.empty = false;
                }
            });
        });
    }

    /// Folders: a search box and the folders that hold items, in tree
    /// order with their counts (Uncategorized first); click includes,
    /// right-click excludes.
    fn folders_chip(&mut self, ui: &mut Ui, lib: &Library) {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        let mut loose = 0;
        for i in lib.items().iter().filter(|i| !i.is_deleted) {
            if i.folders.is_empty() {
                loose += 1;
            }
            for fid in &i.folders {
                *counts.entry(fid.as_str()).or_default() += 1;
            }
        }
        let tree = folders_with_depth(&lib.metadata.folders);
        let name_of = |id: &str| tree.iter().find(|(f, _)| f.id == id).map_or(id.to_string(), |(f, _)| f.name.clone());
        let f = &mut self.ws.filter;
        let fui = &mut self.ws.filter_ui;
        let mut inc: Vec<String> = f.folders.includes.iter().map(|id| name_of(id)).collect();
        let mut exc: Vec<String> = f.folders.excludes.iter().map(|id| name_of(id)).collect();
        if f.folders.empty {
            inc.insert(0, tr("Uncategorized").into());
        }
        if f.folders.exclude_empty {
            exc.insert(0, tr("Uncategorized").into());
        }
        let resp = filter_chip(ui, tr("Folders"), pick_summary(inc.iter().map(String::as_str), exc.iter().map(String::as_str)));
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(260.0);
            logic_picker(ui, &mut f.folders.logic);
            search_box(ui, &mut fui.folder_search);
            ui.separator();
            let listed: Vec<(&Folder, usize)> =
                tree.iter().copied().filter(|(fo, _)| counts.contains_key(fo.id.as_str()) || pick_of(&f.folders, &fo.id) != Pick::Off).collect();
            let keyword = fui.folder_search.trim();
            let rows: Vec<(&Folder, usize)> = if keyword.is_empty() {
                listed.clone()
            } else {
                search::rank_names(listed.iter().map(|(fo, _)| fo.name.as_str()), keyword).into_iter().map(|i| (listed[i].0, 0)).collect()
            };
            egui::ScrollArea::vertical().id_salt("filter-folders").max_height(320.0).show(ui, |ui| {
                if keyword.is_empty() && (loose > 0 || f.folders.empty || f.folders.exclude_empty) {
                    let now = if f.folders.empty {
                        Pick::In
                    } else if f.folders.exclude_empty {
                        Pick::Out
                    } else {
                        Pick::Off
                    };
                    if let Some(p) = pick_row(ui, 0.0, tr("Uncategorized"), Some(loose), now) {
                        f.folders.empty = p == Pick::In;
                        f.folders.exclude_empty = p == Pick::Out;
                    }
                }
                if rows.is_empty() {
                    muted(ui, tr("Nothing found"));
                }
                for (fo, depth) in rows {
                    let n = counts.get(fo.id.as_str()).copied().unwrap_or(0);
                    if let Some(p) = pick_row(ui, depth as f32 * 14.0, &fo.name, Some(n), pick_of(&f.folders, &fo.id)) {
                        set_pick(&mut f.folders.includes, &mut f.folders.excludes, &fo.id, p);
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                muted(ui, tr("Click to include, right-click to exclude"));
                if f.folders.is_active() && ui.button(tr("Clear")).clicked() {
                    f.folders = SetFilter { logic: f.folders.logic, ..Default::default() };
                }
            });
        });
    }

    /// Rating: 5 to 1 stars and None, with counts; empty options hide, as
    /// in Eagle.
    fn rating_chip(&mut self, ui: &mut Ui, lib: &Library) {
        let mut counts = [0usize; 6];
        for i in lib.items().iter().filter(|i| !i.is_deleted) {
            let star = i.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0).min(5) as usize;
            counts[star] += 1;
        }
        let f = &mut self.ws.filter;
        let mut stars: Vec<u8> = f.ratings.iter().copied().collect();
        stars.sort_unstable_by(|a, b| b.cmp(a));
        let value = (!stars.is_empty()).then(|| stars.iter().map(|&n| if n == 0 { tr("None").to_string() } else { "★".repeat(n as usize) }).collect::<Vec<_>>().join(", "));
        let resp = filter_chip(ui, tr("Rating"), value);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(160.0);
            let mut any = false;
            for n in (0..=5u8).rev() {
                let mut on = f.ratings.contains(&n);
                if counts[n as usize] == 0 && !on {
                    continue;
                }
                any = true;
                let label = if n == 0 { tr("None").to_string() } else { "★".repeat(n as usize) };
                if ui.checkbox(&mut on, format!("{label}  ({})", counts[n as usize])).changed() {
                    if on {
                        f.ratings.insert(n);
                    } else {
                        f.ratings.remove(&n);
                    }
                }
            }
            if !any {
                muted(ui, tr("Nothing found"));
            }
        });
    }

    /// Type: a search box and every extension and built-in group with its
    /// count; click includes, right-click excludes.
    fn types_chip(&mut self, ui: &mut Ui, lib: &Library) {
        let live: Vec<&Item> = lib.items().iter().filter(|i| !i.is_deleted).collect();
        let counts = search::type_counts(live.iter().copied());
        let options = search::type_options(live.iter().copied());
        let f = &mut self.ws.filter;
        let fui = &mut self.ws.filter_ui;
        let value = pick_summary(f.types.iter().map(String::as_str), f.exclude_types.iter().map(String::as_str))
            .map(|_| {
                let inc = f.types.iter().map(|t| type_label(t));
                let exc = f.exclude_types.iter().map(|t| format!("-{}", type_label(t)));
                inc.chain(exc).collect::<Vec<_>>().join(",")
            });
        let resp = filter_chip(ui, tr("Type"), value);
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            search_box(ui, &mut fui.type_search);
            ui.separator();
            let keyword = fui.type_search.trim().to_lowercase();
            let mut shown = 0;
            egui::ScrollArea::vertical().id_salt("filter-types").max_height(320.0).show(ui, |ui| {
                for t in &options {
                    let n = counts.get(t).copied().unwrap_or(0);
                    let pick = if f.types.contains(t) {
                        Pick::In
                    } else if f.exclude_types.contains(t) {
                        Pick::Out
                    } else {
                        Pick::Off
                    };
                    if (n == 0 && pick == Pick::Off) || (!keyword.is_empty() && !t.to_lowercase().contains(&keyword) && !type_label(t).to_lowercase().contains(&keyword)) {
                        continue;
                    }
                    shown += 1;
                    if let Some(p) = pick_row(ui, 0.0, &type_label(t), Some(n), pick) {
                        set_pick(&mut f.types, &mut f.exclude_types, t, p);
                    }
                }
                if shown == 0 {
                    muted(ui, tr("Nothing found"));
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                muted(ui, tr("Click to include, right-click to exclude"));
                if (!f.types.is_empty() || !f.exclude_types.is_empty()) && ui.button(tr("Clear")).clicked() {
                    f.types.clear();
                    f.exclude_types.clear();
                }
            });
        });
    }

    /// Opening another view clears the filter and the search, unless the
    /// bar's Lock is on (Eagle's `resetPage`).
    pub(crate) fn view_changed(&mut self) {
        if !self.ws.filter_ui.locked {
            self.reset_filter();
            self.ws.query.clear();
        }
    }
}

/// Where the picker starts when no colour is set.
const DEFAULT_PICKER_RGB: [u8; 3] = [0x32, 0x97, 0xff];

/// Colours offered in the colour filter, like Eagle's swatch row.
pub(crate) const PRESET_COLORS: [[u8; 3]; 14] = [
    [0xff, 0x3b, 0x30],
    [0xff, 0x95, 0x00],
    [0xff, 0xd6, 0x0a],
    [0x34, 0xc7, 0x59],
    [0x30, 0xb0, 0xc7],
    [0x00, 0x7a, 0xff],
    [0x58, 0x56, 0xd6],
    [0xaf, 0x52, 0xde],
    [0xff, 0x2d, 0x55],
    [0xa2, 0x84, 0x5e],
    [0x00, 0x00, 0x00],
    [0x8e, 0x8e, 0x93],
    [0xff, 0xff, 0xff],
    [0x67, 0x95, 0x3f],
];

pub(crate) fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// A filter chip: muted when off, primary-tinted with its value when on.
pub(crate) fn filter_chip(ui: &mut Ui, label: &str, value: Option<String>) -> Response {
    let text = match &value {
        Some(v) => format!("{label}: {v}"),
        None => label.to_string(),
    };
    let on = value.is_some();
    let g = galley(ui, &text, font(13.0), t::p().text, 220.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0, 26.0), Sense::click());
    let fill = if on {
        t::p().primary.gamma_multiply(0.22)
    } else if resp.hovered() {
        t::p().button_hover
    } else {
        t::p().button
    };
    ui.painter().rect_filled(rect, t::RADIUS, fill);
    if on {
        ui.painter().rect_stroke(rect, t::RADIUS, Stroke::new(1.0, t::p().primary.gamma_multiply(0.7)), StrokeKind::Inside);
    }
    let color = if on { t::p().text } else { t::p().text_value };
    ui.painter().galley(Pos2::new(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, color);
    icon(ui, Pos2::new(rect.right() - 11.0, rect.center().y), ic::CARET_DOWN, 10.0, t::p().text_muted);
    resp
}

pub(crate) fn logic_picker(ui: &mut Ui, logic: &mut Logic) {
    ui.horizontal(|ui| {
        for (l, name) in [(Logic::Or, tr("Any")), (Logic::And, tr("All")), (Logic::Equal, tr("Exact"))] {
            if ui.selectable_label(*logic == l, name).clicked() {
                *logic = l;
            }
        }
    });
}

pub(crate) fn text_filter_menu(ui: &mut Ui, f: &mut TextFilter, keywords: &mut String) {
    ui.set_min_width(200.0);
    for (opt, name) in [(TextFilter::Off, tr("Any")), (TextFilter::Has, tr("Has")), (TextFilter::No, tr("None"))] {
        let on = std::mem::discriminant(f) == std::mem::discriminant(&opt);
        if ui.selectable_label(on, name).clicked() {
            *f = opt;
        }
    }
    ui.separator();
    ui.label(RichText::new(tr("Contains")).color(t::p().text_muted).size(12.0));
    if ui.add(TextEdit::singleline(keywords).hint_text(tr("keyword, keyword"))).changed() {
        let words: Vec<String> = keywords.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect();
        *f = if words.is_empty() { TextFilter::Off } else { TextFilter::Keywords(words) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_cycle_like_eagles_lists() {
        assert_eq!(next_pick(Pick::Off, false), Pick::In);
        assert_eq!(next_pick(Pick::In, false), Pick::Off);
        assert_eq!(next_pick(Pick::Off, true), Pick::Out);
        assert_eq!(next_pick(Pick::Out, true), Pick::Off);
        assert_eq!(next_pick(Pick::In, true), Pick::Out);
        assert_eq!(next_pick(Pick::Out, false), Pick::In);
        let (mut inc, mut exc) = (vec!["a".to_string()], vec![]);
        set_pick(&mut inc, &mut exc, "a", Pick::Out);
        assert_eq!((inc.len(), exc), (0, vec!["a".to_string()]));
    }

    #[test]
    fn chip_values_list_includes_then_excludes() {
        assert_eq!(pick_summary(["a", "b"], ["c"]).as_deref(), Some("a,b,-c"));
        assert_eq!(pick_summary([], []), None);
    }

    #[test]
    fn tags_order_by_group_then_name() {
        let tags: Vec<(String, usize)> = ["zeta", "Beta", "alpha", "gamma"].iter().map(|t| (t.to_string(), 1)).collect();
        let group = |name: &str, tags: &[&str]| TagGroup {
            id: name.into(),
            name: name.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            color: None,
            description: None,
            extra: Default::default(),
        };
        let groups = [group("g1", &["gamma"]), group("g2", &["zeta", "alpha"])];
        let names: Vec<&str> = tag_order(&tags, &groups).into_iter().map(|i| tags[i].0.as_str()).collect();
        assert_eq!(names, ["gamma", "alpha", "zeta", "Beta"]);
    }

    #[test]
    fn active_filters_by_kind() {
        let mut f = Filter::default();
        assert!(FilterKind::ALL.iter().all(|k| !kind_active(&f, *k)));
        f.folders.exclude_empty = true;
        f.custom_shape = Some((1.0, 1.0));
        assert!(kind_active(&f, FilterKind::Folders) && kind_active(&f, FilterKind::Shape));
        assert!(!kind_active(&f, FilterKind::Tags));
        assert_eq!(type_label("youtube"), "YouTube");
        assert_eq!(type_label("png"), "PNG");
    }
}
