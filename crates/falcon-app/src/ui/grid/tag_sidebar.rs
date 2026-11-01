//! The All Tags page's sidebar, after Eagle's tag manager: All,
//! Uncategorized (tags in no group) and Starred, then the tag groups with
//! a + to make one. A click opens a list and gives the arrow keys to the
//! "tags" focus zone. Tags dragged from the page drop on Starred (they are
//! starred) or on a group (they move into it; with ⌥ they are filed there
//! as well). Groups can be dragged to reorder them, renamed by
//! double-clicking and have their own menu. The rules are in `tag_view.rs`.

use eframe::egui::{Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, vec2};
use falcon_library::Library;

use crate::app::App;
use crate::folder_icons;
use crate::icons as ic;
use crate::locale::{tr, trf};
use crate::logic;
use crate::organize::{self, DropZone};
use crate::ui::organize::{OrgAction, org};
use crate::tag_view::{self, Drop, TagView};
use crate::theme::{self as t, font};
use crate::ui::organize::{DragGroup, DragTag};
use crate::ui::sidebar::sidebar_row_tinted;
use crate::ui::widgets::icon_button;

/// The sidebar's width on the page.
pub(crate) const TAG_SIDEBAR_WIDTH: f32 = 190.0;

impl App {
    /// Opens a list on the All Tags page, as a click on its sidebar row
    /// does: another list drops the picked tags and the page's filter.
    pub(crate) fn open_tag_view(&mut self, view: TagView) {
        if self.ws.org.tag_view != view {
            self.ws.org.tag_sel.clear();
            self.ws.org.tag_anchor = None;
            self.ws.tag_filter.clear();
        }
        self.ws.org.tag_view = view;
        self.zone = logic::Zone::Tags;
    }

    /// Draws the sidebar in `rect`. `counts` are the tags in use.
    pub(crate) fn tag_sidebar(&mut self, ui: &mut Ui, lib: &Library, rect: Rect, counts: &[(String, usize)]) {
        self.ws.org.tag_sidebar = Some(rect);
        let groups = &lib.metadata.tags_groups;
        let view = tag_view::valid(&self.ws.org.tag_view, groups);
        if view != self.ws.org.tag_view {
            self.ws.org.tag_view = view.clone();
        }
        ui.painter().vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, t::p().divider));
        let mut child = ui.new_child(eframe::egui::UiBuilder::new().max_rect(rect.shrink2(vec2(0.0, 10.0))));
        let ui = &mut child;
        ui.spacing_mut().item_spacing.y = 0.0;
        let fixed = [
            (TagView::All, ic::TAG_SIMPLE, tr("tags::All"), counts.len()),
            (TagView::Uncategorized, ic::FOLDER_DASHED, tr("tags::Uncategorized"), tag_view::uncategorized(counts, groups).len()),
            (TagView::Starred, ic::STAR, tr("Starred"), lib.tags.starred_tags.len()),
        ];
        for (v, glyph, label, n) in fixed {
            let resp = sidebar_row_tinted(ui, 0, glyph, t::p().text, label, self.badge(n), view == v, None);
            if view == v && std::mem::take(&mut self.ws.org.tag_reveal) {
                resp.scroll_to_me(None);
            }
            if resp.clicked() {
                self.open_tag_view(v.clone());
            }
            if v == TagView::Starred {
                self.tag_row_drop(ui, lib, &resp, &v);
            }
        }
        ui.add_space(10.0);
        // The Groups heading, with a + that makes a group and opens it.
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let label = if groups.is_empty() { tr("Groups").to_string() } else { trf("Groups ({n})", &[("n", &groups.len())]) };
        ui.painter().text(Pos2::new(head.left() + 14.0, head.center().y), eframe::egui::Align2::LEFT_CENTER, label, font(12.0), t::p().text_muted);
        if icon_button(ui, Pos2::new(head.right() - 24.0, head.center().y), ic::PLUS, 14.0, true, tr("New tag group")).clicked() {
            self.ws.org.rename_new_group();
            self.ws.actions.push(org(OrgAction::CreateGroup(Vec::new())));
            self.zone = logic::Zone::Tags;
        }
        eframe::egui::ScrollArea::vertical().id_salt("tag-sidebar-groups").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for g in groups {
                let v = TagView::Group(g.id.clone());
                let tint = folder_icons::color(g.color.as_deref()).unwrap_or(t::p().icon_none);
                let renaming = self.ws.org.group_rename.as_ref().is_some_and(|(id, _)| *id == g.id);
                let name = if renaming { "" } else { g.name.as_str() };
                let resp = sidebar_row_tinted(ui, 0, ic::TAG_SIMPLE, tint, name, if renaming { None } else { Some(g.tags.len()) }, view == v, None);
                if renaming {
                    self.group_rename_field(ui, resp.rect, &g.id, &g.name);
                }
                if view == v && std::mem::take(&mut self.ws.org.tag_reveal) {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    self.open_tag_view(v.clone());
                }
                if resp.double_clicked() {
                    self.ws.org.group_rename = Some((g.id.clone(), g.name.clone()));
                }
                resp.context_menu(|ui| self.tag_group_context_menu(ui, lib, &g.id));
                self.tag_row_drop(ui, lib, &resp, &v);
                self.group_row_drag(ui, lib, &resp, &g.id);
            }
        });
    }

    /// The inline field renaming a group over its sidebar row. Return or
    /// leaving the field saves a changed, non-empty name; Esc cancels.
    fn group_rename_field(&mut self, ui: &mut Ui, row: Rect, id: &str, old: &str) {
        let Some((_, buf)) = self.ws.org.group_rename.as_mut() else { return };
        let r = Rect::from_min_max(Pos2::new(row.left() + 40.0, row.top() + 2.0), Pos2::new(row.right() - 12.0, row.bottom() - 2.0));
        let te = ui.put(r, TextEdit::singleline(buf).id_salt(("group-rename", id)).font(font(13.0)));
        if !te.has_focus() && !te.lost_focus() {
            te.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if te.lost_focus() || cancel {
            let (id, name) = self.ws.org.group_rename.take().unwrap_or_default();
            if !cancel && !name.trim().is_empty() && name.trim() != old {
                self.ws.actions.push(org(OrgAction::RenameGroup(id, name.trim().to_string())));
            }
        }
    }

    /// Dragged tags over a Starred or group row: highlighted when the row
    /// takes them; dropped, they are starred or filed (`tag_view::drop_on`).
    fn tag_row_drop(&mut self, ui: &Ui, lib: &Library, resp: &eframe::egui::Response, target: &TagView) {
        let copy = ui.input(|i| i.modifiers.alt);
        let Some(d) = resp.dnd_hover_payload::<DragTag>() else { return };
        let Some(drop) = tag_view::drop_on(target, &d.0, &lib.metadata.tags_groups, copy) else { return };
        let bg = Rect::from_min_max(Pos2::new(resp.rect.left() + 8.0, resp.rect.top() + 0.5), Pos2::new(resp.rect.right() - 11.0, resp.rect.bottom() - 0.5));
        ui.painter().rect_stroke(bg, t::RADIUS, Stroke::new(1.5, t::p().primary), StrokeKind::Inside);
        if resp.dnd_release_payload::<DragTag>().is_none() {
            return;
        }
        let action = match drop {
            Drop::Star(tags) => crate::app::Action::StarTags(tags, true),
            Drop::Group { id, tags, copy: true } => org(OrgAction::AddToGroupToo(id, tags)),
            Drop::Group { id, tags, copy: false } => org(OrgAction::AddToGroup(id, tags)),
        };
        self.ws.actions.push(action);
    }

    /// Groups reorder by dragging their rows: the top half of another row
    /// drops before it, the bottom half after, shown by a line.
    fn group_row_drag(&mut self, ui: &Ui, lib: &Library, resp: &eframe::egui::Response, id: &str) {
        if resp.drag_started() {
            resp.dnd_set_drag_payload(DragGroup(id.to_string()));
        }
        let Some(d) = resp.dnd_hover_payload::<DragGroup>().filter(|d| d.0 != id) else { return };
        let Some(y) = ui.ctx().pointer_latest_pos().map(|p| p.y) else { return };
        let r = resp.rect;
        let after = organize::drop_zone(r.top(), r.height(), y, false) == DropZone::After;
        let line_y = if after { r.bottom() - 0.5 } else { r.top() + 0.5 };
        ui.painter().hline(r.left() + 12.0..=r.right() - 12.0, line_y, Stroke::new(1.5, t::p().primary));
        if resp.dnd_release_payload::<DragGroup>().is_some()
            && let Some(index) = organize::group_drop(&lib.metadata.tags_groups, &d.0, id, after)
        {
            self.ws.actions.push(org(OrgAction::MoveGroup(d.0.clone(), index)));
        }
    }
}
