//! The All Tags page shown in place of the grid.

use eframe::egui::{self, Align2, CornerRadius, Key, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, vec2};
use falcon_library::Library;
use falcon_library::search::Logic;

use crate::app::{Action, App};
use crate::icons as ic;
use crate::locale::{tr, tr_n};
use crate::logic::{self, Scope};
use crate::theme::{self as t, font};
use crate::tag_view::{self, TagView};
use crate::ui::widgets::{galley, icon};
use super::GRID_PAD_X;
use super::tag_sidebar::TAG_SIDEBAR_WIDTH;

impl App {
    pub(crate) fn tag_page(&mut self, ui: &mut Ui, lib: &Library) {
        let mut counts = logic::tag_counts(lib.items());
        self.ws.org.tag_sort.apply(&mut counts);
        let full = ui.available_rect_before_wrap();
        let side = Rect::from_min_size(full.min, vec2(TAG_SIDEBAR_WIDTH, full.height()));
        self.tag_sidebar(ui, lib, side, &counts);
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
        let sort = self.ws.org.tag_sort;
        // The order the chips were drawn in last frame is what ⇧-click spans.
        self.ws.org.tag_order_shown = std::mem::take(&mut self.ws.org.tag_order);
        let all_view = view == TagView::All;
        self.ws.org.tag_sel.retain(|t| tags.iter().any(|(x, _)| x == t) || (all_view && lib.tags.starred_tags.contains(t)));
        let starred: Vec<(String, usize)> = if all_view {
            lib.tags.starred_tags.iter().filter_map(|s| tags.iter().find(|(t, _)| t == s).cloned()).collect()
        } else {
            Vec::new()
        };
        egui::ScrollArea::vertical().id_salt("tags-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(GRID_PAD_X);
                let (rect, _) = ui.allocate_exact_size(vec2(240.0, 30.0), Sense::hover());
                ui.painter().rect_filled(rect, t::FIELD_RADIUS, t::p().search);
                ui.painter().rect_stroke(rect, t::FIELD_RADIUS, Stroke::new(1.0, t::p().field_border), StrokeKind::Inside);
                icon(ui, Pos2::new(rect.left() + 14.0, rect.center().y), ic::MAGNIFYING_GLASS, 15.0, t::p().text_muted);
                let inner = Rect::from_min_max(Pos2::new(rect.left() + 30.0, rect.top()), rect.max - vec2(8.0, 0.0));
                ui.put(
                    inner,
                    TextEdit::singleline(&mut self.ws.tag_filter)
                        .id_salt("tag-filter")
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::ZERO)
                        .hint_text(RichText::new(tr("Filter tags")).color(t::p().text_muted))
                        .font(font(13.0))
                        .align(Align2::LEFT_CENTER),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(tr_n(tags.len(), "{n} tag", "{n} tags")).color(t::p().text_muted));
                self.tag_page_controls(ui, all_view);
            });
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
                ui.horizontal(|ui| {
                    ui.add_space(GRID_PAD_X + 2.0);
                    ui.label(RichText::new(format!("{}  {}  ({})", ic::STAR, tr("Starred"), tags.len())).size(14.0).color(t::p().text_muted));
                });
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
                ui.horizontal(|ui| {
                    ui.add_space(GRID_PAD_X + 2.0);
                    ui.label(RichText::new(heading).size(14.0).color(t::p().text_muted));
                });
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(GRID_PAD_X);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for (tag, n) in group {
                        self.tag_chip(ui, lib, &tag, n);
                    }
                });
                ui.add_space(16.0);
            }
        });
    }

    pub(crate) fn tag_chip(&mut self, ui: &mut Ui, lib: &Library, tag: &str, n: usize) {
        let renaming = self.ws.tag_rename.as_ref().is_some_and(|(t, _)| t == tag);
        if renaming {
            let (_, buf) = self.ws.tag_rename.as_mut().unwrap();
            let te = ui.add(TextEdit::singleline(buf).id_salt(("tag-rename", tag)).desired_width(140.0).font(font(13.0)));
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
        let name_g = galley(ui, tag, font(13.0), t::p().text, 300.0);
        let count_g = galley(ui, &n.to_string(), font(12.0), t::p().text_muted, 80.0);
        // Tags filed in a group carry the group's colour dot.
        let dot = App::tag_dot(lib, tag);
        let dot_w = if dot.is_some() { 14.0 } else { 0.0 };
        let size = vec2(name_g.size().x + count_g.size().x + 30.0 + dot_w, 28.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        self.tag_chip_drag(&resp, tag);
        self.ws.org.tag_order.push(tag.to_string());
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
