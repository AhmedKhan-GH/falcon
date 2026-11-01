//! The tag picker: Eagle's tag select panel, a floating panel beside the
//! inspector for the selected items. Typing searches; Return adds the
//! highlighted tag (or creates the typed ones), clicking a tag the items
//! have takes it off. The decisions live in `crate::tags`.

use std::collections::HashSet;

use eframe::egui::{self, Color32, CornerRadius, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, TextEdit, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::{Action, App};
use crate::locale::{tr, trf};
use crate::logic;
use crate::icons as ic;
use crate::theme::{self as t, font};
use crate::ui::widgets::{galley, icon};
use crate::tags::{self, Dir, Kind, Pick, Side};

/// The text of one of `tags::COLUMNS`' size names in the current language.
fn column_label(label: &str) -> &str {
    match label {
        "Extra Small" => tr("Extra Small"),
        "Small" => tr("Small"),
        "Medium" => tr("Medium"),
        "Large" => tr("Large"),
        other => other,
    }
}

/// The panel's size, as Eagle opens it.
const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 480.0;
const SIDEBAR_W: f32 = 112.0;
const CELL_H: f32 = 26.0;
const GAP: f32 = 2.0;

/// The picker's state while it is open.
#[derive(Default)]
pub(crate) struct TagPicker {
    pub open: bool,
    keyword: String,
    /// The keyword the highlight was last chosen for.
    seen: Option<String>,
    highlight: Option<(usize, usize)>,
    /// Scroll the highlighted cell into view next frame.
    reveal: bool,
    side: Side,
    pinned: bool,
    collapsed: HashSet<Kind>,
    /// Where the pointer was when it opened.
    anchor: Option<Pos2>,
    /// Put the cursor in the search field next frame.
    focus: bool,
    /// The frame it opened in: the click that opened it is not a click
    /// outside it.
    opened: u64,
}

/// What a click or Return asked for.
enum Act {
    Add(Vec<String>),
    Remove(String),
}

impl App {
    /// Opens the picker for the selection (or the item in the viewer),
    /// near `at` (the pointer when `None`). Opening it again keeps it.
    pub(crate) fn open_tag_picker(&mut self, ctx: &egui::Context, at: Option<Pos2>) {
        let p = &mut self.ws.picker;
        if !p.open {
            p.keyword.clear();
            p.seen = None;
            p.highlight = None;
            p.side = Side::All;
        }
        p.open = true;
        p.focus = true;
        p.opened = ctx.cumulative_frame_nr();
        p.anchor = at.or_else(|| ctx.pointer_latest_pos());
    }

    fn close_tag_picker(&mut self) {
        let p = &mut self.ws.picker;
        p.open = false;
        p.keyword.clear();
        p.highlight = None;
    }

    pub(crate) fn tag_picker_ui(&mut self, ctx: &egui::Context, lib: &Library) {
        if !self.ws.picker.open {
            return;
        }
        let targets = self.acted_on();
        let items: Vec<&Item> = targets.iter().filter_map(|id| lib.item(id)).collect();
        let selected = tags::common_tags(&items);
        let prefs = self.prefs.tag_picker;
        let counts = logic::tag_counts(lib.items());
        let groups = &lib.metadata.tags_groups;
        let suggestions = if prefs.show.suggested {
            let known: HashSet<&str> = counts.iter().map(|(t, _)| t.as_str()).collect();
            tags::suggestions(&items, &lib.metadata.folders, &known, &selected)
        } else {
            Vec::new()
        };
        let entries = tags::entries(&counts, groups, &lib.tags.history_tags, &lib.tags.starred_tags, &suggestions, &selected);
        let side = match self.ws.picker.side {
            Side::Group(g) if g >= groups.len() => Side::All,
            s => s,
        };
        let sidebar = prefs.sidebar && !groups.is_empty();
        let side = if sidebar { side } else { Side::All };
        let (sections, side_counts) = tags::sections(&entries, groups, &self.ws.picker.keyword, side, prefs.show);
        let p = &mut self.ws.picker;
        if p.seen.as_deref() != Some(p.keyword.as_str()) {
            p.highlight = tags::default_highlight(&entries, &sections, &p.keyword);
            p.seen = Some(p.keyword.clone());
            p.reveal = true;
        }
        if p.highlight.is_some_and(|(s, i)| sections.get(s).is_none_or(|x| i >= x.picks.len())) {
            p.highlight = None;
        }

        let screen = ctx.content_rect();
        let size = vec2(WIDTH.min(screen.width() - 20.0), HEIGHT.min(screen.height() - 60.0));
        // At the pointer, then moved left of the inspector, as Eagle does.
        let anchor = p.anchor.unwrap_or(Pos2::new(screen.right() - t::INSPECTOR_WIDTH, screen.top() + 80.0));
        let mut x = anchor.x - size.x / 2.0;
        if self.show_inspector {
            x = x.min(screen.right() - t::INSPECTOR_WIDTH - size.x + 5.0);
        }
        let x = x.clamp(screen.left() + 10.0, (screen.right() - size.x - 10.0).max(screen.left() + 10.0));
        let y = (anchor.y - 20.0).clamp(screen.top() + 40.0, (screen.bottom() - size.y - 10.0).max(screen.top() + 40.0));
        let list_w = size.x - 24.0 - if sidebar { SIDEBAR_W } else { 0.0 };
        let cols = tags::columns(list_w, prefs.column, prefs.list);
        let lens: Vec<usize> = sections.iter().map(|s| if p.collapsed.contains(&s.kind) && s.kind != Kind::Results { 0 } else { s.picks.len() }).collect();

        // Keys, taken before the search field sees them.
        let search_id = egui::Id::unique("tag-picker-search");
        let focused = ctx.memory(|m| m.has_focus(search_id));
        let mut act: Option<(Pick, bool)> = None;
        let mut close = false;
        let mut pasted: Option<Vec<String>> = None;
        if focused {
            ctx.input_mut(|i| {
                let cmd = i.modifiers.command;
                for (key, dir) in [(Key::ArrowUp, Dir::Up), (Key::ArrowDown, Dir::Down)] {
                    if i.consume_key(Modifiers::NONE, key) {
                        p.highlight = tags::step(&lens, cols, p.highlight, dir);
                        p.reveal = true;
                    }
                }
                // Left and Right also move the text cursor, as in Eagle.
                for (key, dir) in [(Key::ArrowLeft, Dir::Left), (Key::ArrowRight, Dir::Right)] {
                    if i.key_pressed(key) && !i.modifiers.any() {
                        p.highlight = tags::step(&lens, cols, p.highlight, dir);
                        p.reveal = true;
                    }
                }
                if (i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::COMMAND, Key::Enter))
                    && let Some((s, k)) = p.highlight
                    && let Some(pick) = sections.get(s).and_then(|x| x.picks.get(k))
                {
                    act = Some((pick.clone(), cmd));
                }
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    close = true;
                }
                let back = i.consume_key(Modifiers::SHIFT, Key::Tab);
                if (back || i.consume_key(Modifiers::NONE, Key::Tab)) && sidebar {
                    p.side = side.cycle(groups.len(), back);
                    p.seen = None;
                }
                // Pasting several tags adds them all at once (reversed, as Eagle does).
                i.events.retain(|e| match e {
                    egui::Event::Paste(text) => {
                        let mut list = tags::parse_tags(text);
                        if list.len() > 1 {
                            list.truncate(tags::MAX_ADDED);
                            list.reverse();
                            pasted = Some(list);
                            false
                        } else {
                            true
                        }
                    }
                    _ => true,
                });
            });
        }

        let mut set_side: Option<Side> = None;
        let mut toggle: Option<Kind> = None;
        let mut hover: Option<(usize, usize)> = None;
        let mut new_prefs = prefs;
        let mut pin = p.pinned;
        let area = egui::Area::new(egui::Id::unique("tag-picker")).order(egui::Order::Foreground).fixed_pos(Pos2::new(x, y)).show(ctx, |ui| {
            let frame = egui::Frame::NONE
                .fill(t::p().panel)
                .stroke(Stroke::new(1.0, t::p().field_border))
                .corner_radius(CornerRadius::same(8))
                .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) })
                .inner_margin(egui::Margin::same(8));
            frame.show(ui, |ui| {
                ui.set_min_size(size - vec2(16.0, 16.0));
                ui.set_max_size(size - vec2(16.0, 16.0));
                // Header: search, sidebar toggle, settings, pin, close.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let buttons = if groups.is_empty() { 3.0 } else { 4.0 };
                    let te = ui.add(
                        TextEdit::singleline(&mut p.keyword)
                            .id(search_id)
                            .hint_text(RichText::new(tr("Search or create tags")).color(t::p().text_muted))
                            .font(font(13.0))
                            .char_limit(128)
                            .desired_width(ui.available_width() - buttons * 26.0),
                    );
                    if std::mem::take(&mut p.focus) {
                        te.request_focus();
                    }
                    let btn = |ui: &mut Ui, glyph: &str, on: bool, tip: &str| {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        if on || resp.hovered() {
                            ui.painter().rect_filled(r, t::RADIUS, if on { t::p().row_selected } else { t::p().row_hover });
                        }
                        icon(ui, r.center(), glyph, 14.0, if on { t::p().text } else { t::p().text_value });
                        resp.on_hover_text(tip)
                    };
                    if !groups.is_empty() && btn(ui, ic::SIDEBAR_SIMPLE, sidebar, tr("Show tag groups (Tab moves through them)")).clicked() {
                        new_prefs.sidebar = !prefs.sidebar;
                    }
                    let gear = btn(ui, ic::GEAR, false, tr("Settings"));
                    egui::Popup::menu(&gear).show(|ui| {
                        ui.set_min_width(190.0);
                        ui.checkbox(&mut new_prefs.show.starred, tr("Starred Tags"));
                        ui.checkbox(&mut new_prefs.show.recent, tr("Recent Tags"));
                        ui.checkbox(&mut new_prefs.show.suggested, tr("Recommended Tags"));
                        ui.checkbox(&mut new_prefs.counts, tr("Show Counts"));
                        ui.separator();
                        ui.checkbox(&mut new_prefs.list, tr("One Tag per Row"));
                        ui.add_enabled_ui(!new_prefs.list, |ui| {
                            ui.label(RichText::new(tr("Column size")).color(t::p().text_muted).size(12.0));
                            for (w, label) in tags::COLUMNS {
                                let mark = if new_prefs.column == w { ic::CHECK } else { " " };
                                if ui.button(format!("{mark}  {}", column_label(label))).clicked() {
                                    new_prefs.column = w;
                                }
                            }
                        });
                    });
                    if btn(ui, ic::PUSH_PIN, pin, if pin { tr("Unpin (close when clicking elsewhere)") } else { tr("Keep open") }).clicked() {
                        pin = !pin;
                    }
                    if btn(ui, ic::X, false, tr("Close (Esc)")).clicked() {
                        close = true;
                    }
                });
                ui.add_space(6.0);
                ui.horizontal_top(|ui| {
                    if sidebar {
                        ui.vertical(|ui| {
                            ui.set_width(SIDEBAR_W - 8.0);
                            let mut row = |ui: &mut Ui, s: Side, dot: Option<Color32>, name: &str| {
                                let (r, resp) = ui.allocate_exact_size(vec2(SIDEBAR_W - 8.0, 22.0), Sense::click());
                                if side == s || resp.hovered() {
                                    ui.painter().rect_filled(r, t::RADIUS, if side == s { t::p().row_selected } else { t::p().row_hover });
                                }
                                let mut tx = r.left() + 6.0;
                                if let Some(c) = dot {
                                    ui.painter().circle_filled(Pos2::new(tx + 4.0, r.center().y), 4.0, c);
                                    tx += 13.0;
                                }
                                let n = side_counts.get(&s).copied().unwrap_or(0);
                                let ng = galley(ui, &n.to_string(), font(11.0), t::p().text_muted, 40.0);
                                let g = galley(ui, name, font(12.0), t::p().text, r.right() - tx - ng.size().x - 10.0);
                                ui.painter().galley(Pos2::new(tx, r.center().y - g.size().y / 2.0), g, t::p().text);
                                ui.painter().galley(Pos2::new(r.right() - 5.0 - ng.size().x, r.center().y - ng.size().y / 2.0), ng, t::p().text_muted);
                                if resp.clicked() {
                                    set_side = Some(s);
                                }
                            };
                            row(ui, Side::All, None, tr("tags::All"));
                            for (gi, g) in groups.iter().enumerate() {
                                let c = crate::folder_icons::color(g.color.as_deref()).unwrap_or(t::p().icon_none);
                                row(ui, Side::Group(gi), Some(c), &g.name);
                            }
                            row(ui, Side::Ungrouped, None, tr("Ungrouped"));
                        });
                    }
                    egui::ScrollArea::vertical().id_salt("tag-picker-list").auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_width(list_w);
                        if targets.is_empty() {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| ui.label(RichText::new(tr("No items selected")).color(t::p().text_muted)));
                            return;
                        }
                        if sections.is_empty() {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| ui.label(RichText::new(tr("No tags yet. Type one to create it.")).color(t::p().text_muted)));
                        }
                        let cell_w = ((list_w - GAP * (cols as f32 - 1.0)) / cols as f32).floor();
                        for (si, sec) in sections.iter().enumerate() {
                            if sec.kind != Kind::Results {
                                let collapsed = p.collapsed.contains(&sec.kind);
                                let title = match sec.kind {
                                    Kind::Starred => tr("Starred").to_string(),
                                    Kind::Recent => tr("Recent").to_string(),
                                    Kind::Suggested => tr("Recommended").to_string(),
                                    Kind::Group(g) => groups.get(g).map_or_else(String::new, |g| g.name.clone()),
                                    Kind::Others | Kind::Results => tr("Others").to_string(),
                                };
                                let (r, resp) = ui.allocate_exact_size(vec2(list_w, 26.0), Sense::click());
                                let caret = if collapsed { ic::CARET_RIGHT } else { ic::CARET_DOWN };
                                icon(ui, Pos2::new(r.left() + 6.0, r.center().y), caret, 10.0, t::p().text_muted);
                                let g = galley(ui, &format!("{title}  ({})", sec.picks.len()), font(12.0), t::p().text_muted, list_w - 20.0);
                                ui.painter().galley(Pos2::new(r.left() + 16.0, r.center().y - g.size().y / 2.0), g, t::p().text_muted);
                                if resp.on_hover_text(tr("Click to fold or unfold")).clicked() {
                                    toggle = Some(sec.kind);
                                }
                                if collapsed {
                                    continue;
                                }
                            }
                            for (row, chunk) in sec.picks.chunks(cols).enumerate() {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = GAP;
                                    for (c, pick) in chunk.iter().enumerate() {
                                        let k = row * cols + c;
                                        let (r, resp) = ui.allocate_exact_size(vec2(cell_w, CELL_H), Sense::click());
                                        let lit = p.highlight == Some((si, k));
                                        if lit {
                                            ui.painter().rect_filled(r, t::RADIUS, t::p().row_selected);
                                            if std::mem::take(&mut p.reveal) {
                                                ui.scroll_to_rect(r, None);
                                            }
                                        }
                                        // Hovering moves the highlight once the pointer moves, as in Eagle.
                                        if resp.hovered() && ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
                                            hover = Some((si, k));
                                        }
                                        draw_cell(ui, r, pick, &entries, groups, &selected, prefs.counts, side);
                                        if resp.clicked() {
                                            act = Some((pick.clone(), ui.input(|i| i.modifiers.command)));
                                        }
                                    }
                                });
                                ui.add_space(GAP);
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
            });
        });

        let p = &mut self.ws.picker;
        p.pinned = pin;
        if let Some(h) = hover {
            p.highlight = Some(h);
        }
        if let Some(s) = set_side {
            p.side = s;
            p.seen = None;
        }
        if let Some(k) = toggle
            && !p.collapsed.remove(&k)
        {
            p.collapsed.insert(k);
        }
        if new_prefs != prefs {
            self.prefs.tag_picker = new_prefs;
            if !new_prefs.sidebar {
                self.ws.picker.side = Side::All;
            }
        }
        // Clicks inside keep the cursor in the search field (Eagle refocuses it).
        let inside = area.response.rect;
        let clicked_inside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|pos| inside.contains(pos)));
        if clicked_inside {
            self.ws.picker.focus = true;
        }
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pos| !inside.contains(pos)));
        // A popup of the panel (the settings menu) is outside its rect.
        let menu_open = egui::Popup::is_any_open(ctx);
        let just_opened = self.ws.picker.opened == ctx.cumulative_frame_nr();
        if clicked_outside && !self.ws.picker.pinned && !menu_open && !just_opened {
            close = true;
        }

        let group_for_new = match side {
            Side::Group(g) => groups.get(g).map(|g| g.id.clone()),
            _ => None,
        };
        let act = act.map(|(pick, keep)| {
            match pick {
                Pick::Create(text) => {
                    let mut list = tags::parse_tags(&text);
                    list.truncate(tags::MAX_ADDED);
                    self.ws.picker.keyword.clear();
                    Act::Add(list)
                }
                Pick::Tag(i) => {
                    let name = entries[i].name.clone();
                    if !self.ws.picker.keyword.is_empty() && !keep {
                        self.ws.picker.keyword.clear();
                    }
                    if selected.contains(&name) { Act::Remove(name) } else { Act::Add(vec![name]) }
                }
            }
        });
        if !targets.is_empty() {
            match act {
                Some(Act::Add(list)) if !list.is_empty() => {
                    let new: Vec<String> = list.iter().filter(|t| !counts.iter().any(|(c, _)| c == *t)).cloned().collect();
                    self.ws.actions.push(Action::AddTags(targets.clone(), list, group_for_new.clone().map(|g| (g, new))));
                }
                Some(Act::Remove(name)) => self.ws.actions.push(Action::RemoveTags(targets.clone(), vec![name])),
                _ => {}
            }
            if let Some(list) = pasted {
                let new: Vec<String> = list.iter().filter(|t| !counts.iter().any(|(c, _)| c == *t)).cloned().collect();
                self.ws.actions.push(Action::AddTags(targets.clone(), list, group_for_new.map(|g| (g, new))));
            }
        }
        if close {
            self.close_tag_picker();
        }
    }
}

/// One cell: a check for tags all the items have, the group's colour dot,
/// the name, and the count.
#[allow(clippy::too_many_arguments)]
fn draw_cell(ui: &Ui, r: Rect, pick: &Pick, entries: &[tags::Entry], groups: &[falcon_library::model::TagGroup], selected: &[String], counts: bool, side: Side) {
    let p = ui.painter();
    let cy = r.center().y;
    match pick {
        Pick::Create(text) => {
            // New tags join the group shown in the sidebar.
            let tint = match side {
                Side::Group(g) => groups.get(g).and_then(|g| crate::folder_icons::color(g.color.as_deref())),
                _ => None,
            };
            icon(ui, Pos2::new(r.left() + 11.0, cy), ic::PLUS, 12.0, tint.unwrap_or(t::p().primary));
            let g = galley(ui, &trf("Create “{text}”", &[("text", &text)]), font(12.0), t::p().text, r.width() - 28.0);
            p.galley(Pos2::new(r.left() + 22.0, cy - g.size().y / 2.0), g, t::p().text);
        }
        Pick::Tag(i) => {
            let e = &entries[*i];
            let on = selected.contains(&e.name);
            let mut x = r.left() + 6.0;
            if on {
                icon(ui, Pos2::new(x + 5.0, cy), ic::CHECK, 11.0, t::p().primary);
            }
            x += 14.0;
            if let Some(g) = e.group().and_then(|g| groups.get(g)) {
                let c = crate::folder_icons::color(g.color.as_deref()).unwrap_or(t::p().icon_none);
                p.circle_filled(Pos2::new(x + 3.0, cy), 3.5, c);
                x += 11.0;
            }
            let ng = (counts && e.count > 0).then(|| galley(ui, &e.count.to_string(), font(11.0), t::p().text_muted, 50.0));
            let room = r.right() - x - 6.0 - ng.as_ref().map_or(0.0, |g| g.size().x + 6.0);
            let color = if on { t::p().text } else { t::p().text_value };
            let g = galley(ui, &e.name, font(12.0), color, room);
            p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, color);
            if let Some(ng) = ng {
                p.galley(Pos2::new(r.right() - 6.0 - ng.size().x, cy - ng.size().y / 2.0), ng, t::p().text_muted);
            }
        }
    }
}
