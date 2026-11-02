//! The All Tags page shown in place of the grid.

use std::sync::Arc;

use eframe::egui::{self, Align2, CornerRadius, Galley, Key, Label, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::search::Logic;

use crate::app::{Action, App};
use crate::icons as ic;
use crate::locale::{tr, tr_n, trf};
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::tag_view::{self, TagView};
use crate::ui::widgets::{galley, icon};
use super::GRID_PAD_X;
use super::tag_sidebar::TAG_SIDEBAR_WIDTH;

/// The narrowest the tag list gets while the page's sidebar is shown, and
/// the narrowest that sidebar is worth drawing.
const MIN_LIST_WIDTH: f32 = 150.0;
const MIN_SIDEBAR_WIDTH: f32 = 100.0;

/// The width of the page's sidebar on a page this wide: it gives way to the
/// list, and folds away (0) when too little of it would be left.
fn tag_sidebar_width(page: f32) -> f32 {
    let width = TAG_SIDEBAR_WIDTH.min(page - MIN_LIST_WIDTH);
    if width < MIN_SIDEBAR_WIDTH { 0.0 } else { width }
}

/// Part of the list inside the page's side margins, plus `extra_left`. The
/// margin narrows the `Ui` itself, so it holds for every row wrapped in it,
/// where an `add_space` would only move the first.
pub(crate) fn tag_margin<R>(ui: &mut Ui, extra_left: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let margin = egui::Margin { left: (GRID_PAD_X + extra_left) as i8, right: GRID_PAD_X as i8, top: 0, bottom: 0 };
    egui::Frame::NONE.inner_margin(margin).show(ui, add).inner
}

/// A wrapping run of chips or controls inside the page's margins. What goes
/// in must fit a row (`ui.max_rect().width()`): anything wider, or an
/// `add_space` at the row's end, stretches the `Ui` and every row after it.
pub(crate) fn tag_flow<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    tag_margin(ui, 0.0, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            add(ui)
        })
        .inner
    })
}

fn tag_heading(ui: &mut Ui, text: RichText) {
    tag_margin(ui, 2.0, |ui| ui.horizontal(|ui| ui.add(Label::new(text).truncate())));
}

/// What was pressed on the toolbar above the list.
#[derive(Default)]
pub(crate) struct TagToolbarPressed {
    pub new_group: bool,
    pub clear_history: bool,
}

/// The toolbar above the list: filter, count, grouping (the All list only;
/// the others are one list each), New Group, sort and Clear Tag History. Its
/// controls wrap to further rows, and on a row too narrow for one its text
/// is cut short, so none reaches past the list.
pub(crate) fn tag_toolbar(ui: &mut Ui, filter: &mut String, count: usize, all: bool, look: &mut crate::tags::TagLook) -> TagToolbarPressed {
    let mut pressed = TagToolbarPressed::default();
    tag_flow(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        tag_filter_field(ui, filter);
        ui.add(Label::new(RichText::new(tr_n(count, "{n} tag", "{n} tags")).color(t::p().text_muted)).truncate());
        ui.spacing_mut().item_spacing.x = 8.0;
        if all {
            ui.add(Label::new(RichText::new(tr("Group by")).color(t::p().text_muted)).truncate());
            if ui.add(egui::Button::selectable(!look.by_group, tr("Alphabet")).truncate()).clicked() {
                look.by_group = false;
            }
            if ui.add(egui::Button::selectable(look.by_group, tr("Tag Group")).truncate()).clicked() {
                look.by_group = true;
            }
        }
        pressed.new_group = ui.add(egui::Button::new(format!("{}  {}", ic::PLUS, tr("New Group"))).truncate()).clicked();
        let sort = egui::Button::new(trf("Sort: {sort}", &[("sort", &look.sort.label())])).truncate();
        egui::containers::menu::MenuButton::from_button(sort).ui(ui, |ui| {
            ui.set_min_width(180.0);
            for s in crate::tags::TagSort::ALL {
                let mark = if s == look.sort { ic::CHECK } else { " " };
                if ui.button(format!("{mark}  {}", s.label())).clicked() {
                    look.sort = s;
                }
            }
        });
        pressed.clear_history = ui.add(egui::Button::new(tr("Clear Tag History…")).truncate()).on_hover_text(tr("Start the tag picker's Recent tags over")).clicked();
    });
    pressed
}

/// The filter box: 240 wide, or a whole row where that is less.
fn tag_filter_field(ui: &mut Ui, filter: &mut String) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.max_rect().width().min(240.0), 30.0), Sense::click());
    ui.painter().rect_filled(rect, t::FIELD_RADIUS, t::p().search);
    ui.painter().rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
    icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 15.0, t::p().text_muted);
    let right = rect.right() - 8.0;
    let inner = Rect::from_min_max(Pos2::new((rect.left() + 30.0).min(right), rect.top()), Pos2::new(right, rect.bottom()));
    // A child, not `ui.put`, which would move the row's cursor back to the
    // text's right edge.
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)));
    let text = field.add(
        TextEdit::singleline(filter)
            .id_salt("tag-filter")
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .hint_text(RichText::new(tr("Filter tags")).color(t::p().text_muted))
            .font(font(13.0))
            .align(Align2::LEFT_CENTER),
    );
    if resp.clicked() {
        text.request_focus();
    }
}

/// The list's scrolling: by bar and wheel only, so a drag on the list is
/// the rubber band's. `jump` moves it there this frame.
fn tag_scroll_area(jump: Option<f32>) -> egui::ScrollArea {
    let mut area = egui::ScrollArea::vertical();
    if let Some(y) = jump {
        area = area.vertical_scroll_offset(y);
    }
    area.id_salt("tags-page").auto_shrink([false, false]).scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
}

/// A screen position as a position in the list, which scrolling can't move.
fn to_list(view_rect: Rect, offset: Vec2, screen: Pos2) -> Vec2 {
    screen - view_rect.min + offset
}

/// The rubber band on screen, from where it started in the list to the
/// pointer. Chips are recorded in screen positions, so this is what they
/// are compared with.
fn band_on_screen(view_rect: Rect, offset: Vec2, start: Vec2, pointer: Pos2) -> Rect {
    Rect::from_two_pos(view_rect.min + (start - offset), pointer)
}

/// A chip's text and size. Long names are cut to 300, or less when the
/// whole chip wouldn't fit a row.
struct ChipLook {
    name: Arc<Galley>,
    count: Arc<Galley>,
    dot_w: f32,
    size: Vec2,
}

fn chip_look(ui: &Ui, tag: &str, n: usize, has_dot: bool) -> ChipLook {
    let count = galley(ui, &n.to_string(), font(12.0), t::p().text_muted, 80.0);
    let dot_w = if has_dot { 14.0 } else { 0.0 };
    let row = ui.max_rect().width();
    let name = galley(ui, tag, font(13.0), t::p().text, (row - count.size().x - 30.0 - dot_w).min(300.0));
    let size = vec2((name.size().x + count.size().x + 30.0 + dot_w).min(row), 28.0);
    ChipLook { name, count, dot_w, size }
}

fn allocate_chip(ui: &mut Ui, tag: &str, n: usize, has_dot: bool) -> (ChipLook, Rect, Response) {
    let look = chip_look(ui, tag, n, has_dot);
    let (rect, resp) = ui.allocate_exact_size(look.size, Sense::click_and_drag());
    (look, rect, resp)
}

impl App {
    pub(crate) fn tag_page(&mut self, ui: &mut Ui, lib: &Library) {
        let mut counts = logic::tag_counts(lib.items());
        self.tag_look.sort.apply(&mut counts);
        let full = ui.available_rect_before_wrap();
        let side = Rect::from_min_size(full.min, vec2(tag_sidebar_width(full.width()), full.height()));
        if side.width() > 0.0 {
            self.tag_sidebar(ui, lib, side, &counts);
        } else {
            self.ws.org.tag_sidebar = None;
        }
        let content = Rect::from_min_max(Pos2::new(side.right(), full.top()), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
        self.tag_list(&mut child, lib, counts);
        ui.advance_cursor_after_rect(full);
    }

    /// The list the sidebar opened: every tag, the ones in no group, the
    /// starred ones or one group's, filtered by the page's box and sorted.
    fn tag_list(&mut self, ui: &mut Ui, lib: &Library, counts: Vec<(String, usize)>) {
        let filter = self.ws.tag_filter.trim().to_lowercase();
        let groups = &lib.metadata.tags_groups;
        let view = tag_view::valid(&self.ws.org.tag_view, groups);
        let group = match &view {
            TagView::Group(id) => groups.iter().find(|g| &g.id == id),
            _ => None,
        };
        let listed = match &view {
            TagView::All => counts.clone(),
            TagView::Uncategorized => tag_view::uncategorized(&counts, groups),
            TagView::Starred => tag_view::starred(&counts, &lib.tags.starred_tags),
            TagView::Group(_) => group.map(|g| tag_view::group_tags(&counts, g)).unwrap_or_default(),
        };
        let mut tags: Vec<(String, usize)> = listed.into_iter().filter(|(t, _)| filter.is_empty() || t.to_lowercase().contains(&filter)).collect();
        let sort = self.tag_look.sort;
        // The order the chips were drawn in last frame is what ⇧-click spans.
        self.ws.org.tag_order_shown = std::mem::take(&mut self.ws.org.tag_order);
        let all_view = view == TagView::All;
        self.ws.org.tag_sel.retain(|t| tags.iter().any(|(x, _)| x == t) || (all_view && lib.tags.starred_tags.contains(t)));
        let starred: Vec<(String, usize)> = if all_view {
            lib.tags.starred_tags.iter().filter_map(|s| tags.iter().find(|(t, _)| t == s).cloned()).collect()
        } else {
            Vec::new()
        };
        // The rubber band's background sits under the list, so chips and
        // fields on top take their own presses.
        let view_rect = ui.max_rect();
        let bg = ui.interact(view_rect, ui.scope_id().with("tag-band"), Sense::click_and_drag());
        self.ws.org.tag_rects.clear();
        let out = tag_scroll_area(self.ws.org.tag_scroll.take()).show(ui, |ui| {
            ui.add_space(12.0);
            self.tag_page_toolbar(ui, tags.len(), all_view);
            ui.add_space(14.0);
            // A group's list keeps its heading (and description) when empty.
            if let Some(g) = group {
                let sec = crate::organize::TagSection {
                    id: g.id.clone(),
                    name: g.name.clone(),
                    color: g.color.clone(),
                    description: g.description.clone().unwrap_or_default(),
                    tags: tags.clone(),
                };
                self.tag_group_header(ui, lib, Some(&sec), tags.len());
                self.tag_chips(ui, lib, &tags);
                return;
            }
            if tags.is_empty() && starred.is_empty() {
                let top = ui.cursor().top();
                let rest = Rect::from_min_max(Pos2::new(ui.max_rect().left(), top), ui.max_rect().right_bottom());
                let all_filed = view == TagView::Uncategorized && !counts.is_empty();
                self.empty_tags(ui, rest, !filter.is_empty(), all_filed);
                return;
            }
            if view == TagView::Starred {
                tag_heading(ui, RichText::new(format!("{}  {}  ({})", ic::STAR, tr("Starred"), tags.len())).size(14.0).color(t::p().text_muted));
                self.tag_chips(ui, lib, &tags);
                return;
            }
            if all_view && self.tags_by_group() {
                self.tag_groups_page(ui, lib, &filter);
                return;
            }

            let mut groups: Vec<(String, Vec<(String, usize)>)> = Vec::new();
            if !starred.is_empty() {
                groups.push((format!("{}  {}", ic::STAR, tr("Starred")), starred.clone()));
            }
            let letter = |t: &str| {
                let first = t.chars().next().map_or('#', |c| c.to_uppercase().next().unwrap_or(c));
                if first.is_alphabetic() { first } else { '#' }
            };
            // A-Z sections (Z-A for Z > A, # last either way), the sort inside each.
            let down = sort == crate::tags::TagSort::NameDown;
            tags.sort_by(|a, b| {
                let (x, y) = (letter(&a.0), letter(&b.0));
                (x == '#').cmp(&(y == '#')).then_with(|| if down { y.cmp(&x) } else { x.cmp(&y) })
            });
            let mut current: Option<char> = None;
            for (tag, n) in tags {
                let first = letter(&tag);
                if current != Some(first) {
                    groups.push((first.to_string(), Vec::new()));
                    current = Some(first);
                }
                groups.last_mut().unwrap().1.push((tag, n));
            }
            for (heading, group) in groups {
                tag_heading(ui, RichText::new(heading).size(14.0).color(t::p().text_muted));
                ui.add_space(2.0);
                tag_flow(ui, |ui| {
                    for (tag, n) in group {
                        self.tag_chip(ui, lib, &tag, n);
                    }
                });
                ui.add_space(16.0);
            }
        });
        self.tag_band(ui, &bg, view_rect, out.state.clamped_offset());
    }

    /// Eagle's rubber band on the tag list: a plain press on empty space
    /// drops the pick, and dragging picks every chip the band touches
    /// (live, replacing the pick); near the top or bottom edge the list
    /// scrolls. A press with ⌘ or ⇧ does nothing there.
    fn tag_band(&mut self, ui: &Ui, bg: &egui::Response, view_rect: Rect, offset: egui::Vec2) {
        let plain = ui.input(|i| !i.modifiers.command && !i.modifiers.shift && !i.modifiers.ctrl);
        if bg.clicked() && plain {
            self.ws.org.tag_sel.clear();
            self.ws.org.tag_anchor = None;
        }
        if bg.drag_started() && plain
            && let Some(p) = bg.interact_pointer_pos()
        {
            self.ws.org.tag_sel.clear();
            self.ws.org.tag_anchor = None;
            self.ws.org.tag_band = Some(to_list(view_rect, offset, p));
        }
        let Some(start) = self.ws.org.tag_band else { return };
        if !bg.dragged() {
            self.ws.org.tag_band = None;
            return;
        }
        let Some(p) = ui.ctx().pointer_latest_pos() else { return };
        let band = band_on_screen(view_rect, offset, start, p);
        let chips: Vec<(String, Rect)> = self.ws.org.tag_rects.clone();
        self.ws.org.tag_sel = tag_view::banded(&chips, band);
        let shown = band.intersect(view_rect);
        ui.painter().rect_filled(shown, 2.0, t::p().primary.gamma_multiply(0.15));
        ui.painter().rect_stroke(shown, 2.0, Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        // Near an edge the list scrolls, as Eagle's does.
        let edge = 24.0;
        let dy = if p.y < view_rect.top() + edge { 12.0 } else if p.y > view_rect.bottom() - edge { -12.0 } else { 0.0 };
        if dy != 0.0 {
            ui.ctx().request_repaint();
            self.ws.org.tag_scroll = Some((offset.y - dy).max(0.0));
        }
    }

    pub(crate) fn tag_chip(&mut self, ui: &mut Ui, lib: &Library, tag: &str, n: usize) {
        let renaming = self.ws.tag_rename.as_ref().is_some_and(|(t, _)| t == tag);
        if renaming {
            let (_, buf) = self.ws.tag_rename.as_mut().unwrap();
            // The field's margin comes on top of its width.
            let width = (ui.max_rect().width() - 8.0).min(140.0);
            if ui.available_size_before_wrap().x < width + 8.0 {
                ui.end_row();
            }
            let te = ui.add(TextEdit::singleline(buf).id_salt(("tag-rename", tag)).desired_width(width).font(font(13.0)));
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if te.lost_focus() || cancel {
                let (from, to) = self.ws.tag_rename.take().unwrap();
                if !cancel && !to.trim().is_empty() && to.trim() != from {
                    self.ws.actions.push(Action::RenameTag(from, to.trim().to_string()));
                }
            }
            return;
        }
        // Tags filed in a group carry the group's colour dot.
        let dot = App::tag_dot(lib, tag);
        let (ChipLook { name: name_g, count: count_g, dot_w, .. }, rect, resp) = allocate_chip(ui, tag, n, dot.is_some());
        self.tag_chip_drag(&resp, tag);
        self.ws.org.tag_order.push(tag.to_string());
        self.ws.org.tag_rects.push((tag.to_string(), rect));
        let picked = self.ws.org.tag_sel.iter().any(|t| t == tag);
        let fill = if picked {
            t::p().primary.gamma_multiply(0.45)
        } else if resp.hovered() {
            t::p().button_hover
        } else {
            t::p().button
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        if picked {
            ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, t::p().primary), StrokeKind::Inside);
        }
        let cy = rect.center().y;
        if let Some(c) = dot {
            ui.painter().circle_filled(Pos2::new(rect.left() + 14.0, cy), 4.0, c);
        }
        ui.painter().galley(Pos2::new(rect.left() + 11.0 + dot_w, cy - name_g.size().y / 2.0), name_g.clone(), t::p().text);
        ui.painter().galley(Pos2::new(rect.right() - 11.0 - count_g.size().x, cy - count_g.size().y / 2.0), count_g, t::p().text_muted);
        // Eagle's tag manager: a click picks (⌘ and ⇧ pick more), a
        // double-click shows the tag's items.
        if resp.double_clicked() {
            self.go(Scope::Tag(tag.to_string()));
        } else if resp.clicked() {
            let m = ui.input(|i| i.modifiers);
            let o = &mut self.ws.org;
            let (sel, anchor) = crate::tags::select_click(&o.tag_sel, o.tag_anchor.as_deref(), &o.tag_order_shown, tag, m.shift, m.command);
            o.tag_sel = sel;
            o.tag_anchor = anchor;
        }
        if resp.secondary_clicked() && !picked {
            self.ws.org.tag_sel = vec![tag.to_string()];
            self.ws.org.tag_anchor = Some(tag.to_string());
        }
        resp.context_menu(|ui| crate::ui::menus::search::show(ui, |ui| {
            let tags = if self.ws.org.tag_sel.is_empty() { vec![tag.to_string()] } else { self.ws.org.tag_sel.clone() };
            self.tags_menu(ui, lib, &tags);
        }));
        resp.on_hover_text(tr("Click to pick, ⌘ or ⇧ to pick more · double-click to show items · right-click for more. Renaming to an existing tag merges them."));
    }

    /// Eagle's Filter with Tags: All, with the tag filter set to any of
    /// `tags`.
    pub(crate) fn filter_any_tags(&mut self, tags: Vec<String>) {
        self.go(Scope::All);
        self.ws.filter.tags.includes = tags;
        self.ws.filter.tags.excludes.clear();
        self.ws.filter.tags.empty = false;
        self.ws.filter.tags.logic = Logic::Or;
        self.ws.show_filter = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::TagLook;
    use egui::{Event, Modifiers, PointerButton, pos2};

    const WINDOW: Vec2 = vec2(1280.0, 800.0);

    /// Tags of many lengths, up to longer than any row.
    fn many_tags() -> Vec<(String, usize)> {
        let names = ["Arnold", "Arthur Wesley Dow", "a", "Art Nouveau", "a very long tag name that never stops going on and on and on and on and on and on and on, past any width", "2024"];
        (0..120).map(|i| (format!("{} {i}", names[i % names.len()]), i * 7)).collect()
    }

    /// What one frame of the list laid out.
    struct ListFrame {
        chips: Vec<(String, Rect)>,
        /// The rubber band background's response.
        band: Response,
        view: Rect,
        offset: Vec2,
        /// Where the heading is: it alone starts a little further in.
        heading: egui::Rangef,
    }

    /// One frame of the list between x 400 and `400 + width`, built as
    /// `tag_page` and `tag_list` build it.
    fn list_frame(ctx: &egui::Context, width: f32, jump: Option<f32>, events: Vec<Event>) -> ListFrame {
        let content = Rect::from_min_size(pos2(400.0, 40.0), vec2(width, 600.0));
        let (mut filter, mut look) = (String::new(), TagLook::default());
        let tags = many_tags();
        let mut chips = Vec::new();
        let mut bg = None;
        let mut offset = Vec2::ZERO;
        let mut heading = egui::Rangef::NOTHING;
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW)), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
            let ui = &mut child;
            bg = Some(ui.interact(content, ui.scope_id().with("band"), Sense::click_and_drag()));
            let scrolled = tag_scroll_area(jump).show(ui, |ui| {
                ui.add_space(12.0);
                tag_toolbar(ui, &mut filter, tags.len(), true, &mut look);
                ui.add_space(14.0);
                let top = ui.cursor().top();
                tag_heading(ui, RichText::new("A heading much too long to fit a narrow list on one line").size(14.0));
                heading = egui::Rangef::new(top, ui.cursor().top());
                for part in tags.chunks(40) {
                    tag_flow(ui, |ui| {
                        for (tag, n) in part {
                            let (_, rect, _) = allocate_chip(ui, tag, *n, n % 2 == 0);
                            chips.push((tag.clone(), rect));
                        }
                    });
                }
                assert_eq!(ui.max_rect().width(), width, "something stretched the list");
            });
            offset = scrolled.state.clamped_offset();
        });
        out.textures_delta.clear();
        ListFrame { chips, band: bg.unwrap(), view: content, offset, heading }
    }

    /// The rects of what can be clicked in the toolbar and chips, this frame.
    fn clickable(ctx: &egui::Context, content: Rect) -> Vec<Rect> {
        ctx.viewport(|v| {
            let widgets = &v.this_pass.widgets;
            let rects: Vec<Rect> = widgets
                .layer_ids()
                .flat_map(|l| widgets.get_layer(l))
                .filter(|w| w.sense.senses_click())
                // The band background and the scroll bar aren't in rows.
                .filter(|w| w.rect.height() < 40.0)
                .map(|w| w.rect)
                .collect();
            assert!(rects.iter().any(|r| content.contains_rect(*r)));
            rects
        })
    }

    /// From the window at its narrowest with wide panels (120: the sidebar
    /// folded away; 150; 210: both panels at their least) up to 1280's 610.
    #[test]
    fn chips_and_toolbar_stay_inside_the_margins_on_rows_with_one_left_edge() {
        for width in [120.0, 150.0, 210.0, 400.0, 610.0] {
            let ctx = egui::Context::default();
            list_frame(&ctx, width, None, vec![]);
            let ListFrame { chips, view: content, heading, .. } = list_frame(&ctx, width, None, vec![]);
            let left = content.left() + GRID_PAD_X;
            let right = content.right() - GRID_PAD_X;
            let controls = clickable(&ctx, content);
            assert!(controls.len() > chips.len(), "{width}: the toolbar's controls are missing");
            for r in &controls {
                assert!(r.left() >= left - 0.5 && r.right() <= right + 0.5, "{width}: {r:?} is outside {left}..{right}");
            }
            // Everything that starts a row starts at the margin, not just the first row.
            let mut rows: Vec<(f32, f32)> = Vec::new();
            for r in controls.iter().filter(|r| !heading.contains(r.center().y)) {
                match rows.iter_mut().find(|(y, _)| (r.center().y - *y).abs() < 10.0) {
                    Some(row) => row.1 = row.1.min(r.left()),
                    None => rows.push((r.center().y, r.left())),
                }
            }
            assert!(rows.len() > 5, "{width}: expected wrapped rows, got {rows:?}");
            for (y, row_left) in rows {
                assert!((row_left - left).abs() < 0.5, "{width}: the row at {y} starts at {row_left}, not {left}");
            }
        }
    }

    /// At 1280 the toolbar takes two rows and no button is cut short.
    #[test]
    fn a_wide_toolbar_wraps_rather_than_cuts_its_buttons() {
        let ctx = egui::Context::default();
        let (mut filter, mut look) = (String::new(), TagLook::default());
        let mut clear = Rect::NOTHING;
        let mut measured = Rect::NOTHING;
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW)), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            let content = Rect::from_min_size(pos2(400.0, 40.0), vec2(610.0, 600.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
            tag_toolbar(&mut child, &mut filter, 12, true, &mut look);
            clear = ctx_last_click_widget(&child);
            let mut probe = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(0.0, 700.0), vec2(800.0, 40.0))));
            measured = probe.add(egui::Button::new(tr("Clear Tag History…"))).rect;
        });
        out.textures_delta.clear();
        assert_eq!(clear.width(), measured.width());
        assert!(clear.right() <= 400.0 + 610.0 - GRID_PAD_X);
        assert!(clear.top() > 40.0 + 30.0, "the last button should be on a second row");
    }

    /// The last clickable widget added so far.
    fn ctx_last_click_widget(ui: &Ui) -> Rect {
        ui.ctx().viewport(|v| {
            let widgets = &v.this_pass.widgets;
            let last = widgets.layer_ids().flat_map(|l| widgets.get_layer(l)).filter(|w| w.sense.senses_click()).last();
            last.map_or(Rect::NOTHING, |w| w.rect)
        })
    }

    /// Rubber band: scrolled or not, a band over two chips' centres picks
    /// them and what's between, and only what it touches.
    #[test]
    fn the_rubber_band_lines_up_with_the_chips_under_a_margin_and_a_scroll() {
        let ctx = egui::Context::default();
        list_frame(&ctx, 400.0, None, vec![]);
        let ListFrame { chips: before, view, offset, .. } = list_frame(&ctx, 400.0, None, vec![]);
        assert_eq!(offset, Vec2::ZERO);
        let start = to_list(view, offset, before[0].1.center());
        list_frame(&ctx, 400.0, Some(60.0), vec![]);
        let ListFrame { chips: after, offset: scrolled, .. } = list_frame(&ctx, 400.0, Some(60.0), vec![]);
        assert_eq!(scrolled.y, 60.0);
        for ((_, was), (_, now)) in before.iter().zip(&after) {
            assert_eq!(*was, now.translate(vec2(0.0, 60.0)), "a chip moved other than by the scroll");
        }
        let band = band_on_screen(view, scrolled, start, after[1].1.center());
        let picked = tag_view::banded(&after, band);
        assert_eq!(picked[0], after[0].0);
        assert!(picked.contains(&after[1].0));
        assert!(!picked.contains(&after[100].0));
        for (tag, rect) in &after {
            assert_eq!(picked.contains(tag), rect.intersects(band), "{tag}");
        }
    }

    /// The margin takes no presses: there the rubber band starts, as it
    /// does in the gaps between chips; a chip keeps its own.
    #[test]
    fn a_press_in_the_margin_reaches_the_rubber_band() {
        let drag_from = |at: Pos2| {
            let ctx = egui::Context::default();
            let chips = list_frame(&ctx, 400.0, None, vec![]).chips;
            list_frame(&ctx, 400.0, None, vec![Event::PointerMoved(at)]);
            list_frame(&ctx, 400.0, None, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
            list_frame(&ctx, 400.0, None, vec![Event::PointerMoved(at + vec2(10.0, 30.0))]);
            (chips, list_frame(&ctx, 400.0, None, vec![Event::PointerMoved(at + vec2(30.0, 40.0))]).band.dragged())
        };
        let (chips, _) = drag_from(pos2(0.0, 0.0));
        let chip = chips[0].1;
        let (_, margin) = drag_from(pos2(405.0, chip.center().y));
        assert!(margin, "the margin should start a band");
        let (_, on_chip) = drag_from(chip.center());
        assert!(!on_chip, "a chip should keep its own press");
    }

    #[test]
    fn the_page_sidebar_gives_way_to_the_list_then_folds_away() {
        assert_eq!(tag_sidebar_width(640.0), TAG_SIDEBAR_WIDTH);
        assert_eq!(tag_sidebar_width(400.0), TAG_SIDEBAR_WIDTH);
        assert_eq!(tag_sidebar_width(320.0), 170.0);
        assert_eq!(tag_sidebar_width(120.0), 0.0);
        assert_eq!(tag_sidebar_width(0.0), 0.0);
    }
}
