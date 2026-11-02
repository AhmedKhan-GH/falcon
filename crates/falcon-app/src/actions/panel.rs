//! The Actions panel, opened by the toolbar's Actions button and G: the
//! library's actions with a search; a click runs one on the selection,
//! and each can be edited, duplicated or removed.

use eframe::egui::{self, Align2, Key, Pos2, RichText, Sense, TextEdit, vec2};
use falcon_library::Library;

use super::editor::Editor;
use crate::app::App;
use crate::icons as ic;
use crate::locale::{tr, tr_n, trf};
use crate::theme::{self as t, font};

const WIDTH: f32 = 340.0;
const ROW_H: f32 = 30.0;

/// What a click in the panel asked for.
enum Pick {
    Run(String),
    Edit(String),
    Duplicate(String),
    Remove(String),
    New,
}

impl App {
    /// Opens the panel under `anchor` (the toolbar button), or centred.
    pub(crate) fn open_actions_panel(&mut self, anchor: Option<Pos2>) {
        let p = &mut self.user_actions.panel;
        p.open = true;
        p.anchor = anchor;
        p.opened = self.ctx.cumulative_frame_nr();
        p.focus = true;
        p.confirm_remove = None;
    }

    pub(crate) fn actions_panel_ui(&mut self, ctx: &egui::Context, lib: &Library) {
        if !self.user_actions.panel.open || self.user_actions.editor.is_some() {
            return;
        }
        let selected = self.acted_on().iter().filter(|id| lib.item(id).is_some_and(|i| !i.is_deleted)).count();
        let screen = ctx.content_rect();
        let at = match self.user_actions.panel.anchor {
            Some(a) => Pos2::new((a.x - WIDTH + 20.0).clamp(screen.left() + 10.0, screen.right() - WIDTH - 10.0), a.y + 6.0),
            None => Pos2::new(screen.center().x - WIDTH / 2.0, screen.top() + 90.0),
        };
        let mut pick = None;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let area = egui::Area::new(egui::Id::unique("actions-panel")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(WIDTH);
                ui.label(RichText::new(tr("Actions")).size(15.0));
                ui.add_space(4.0);
                let panel = &mut self.user_actions.panel;
                let search = ui.add(
                    TextEdit::singleline(&mut panel.query)
                        .id_salt("actions-panel-search")
                        .hint_text(RichText::new(format!("{}  {}", ic::MAGNIFYING_GLASS, tr("Search actions"))).color(t::p().text_muted))
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut panel.focus) {
                    search.request_focus();
                }
                let query = panel.query.clone();
                let confirm = panel.confirm_remove.clone();
                ui.add_space(6.0);
                let defs = self.user_actions.defs().to_vec();
                let defs = &defs;
                if let Some(e) = &self.user_actions.broken {
                    ui.label(RichText::new(trf("The library's actions file could not be read: {e}", &[("e", e)])).color(t::p().icon_red));
                } else if defs.is_empty() {
                    ui.label(RichText::new(tr("No actions yet")).strong());
                    ui.label(
                        RichText::new(tr("An action is a list of organising steps, such as adding tags, filing into folders and rating, that runs on the selected items with one click or its own shortcut."))
                            .color(t::p().text_muted),
                    );
                } else {
                    let shown = super::filter(defs, &query);
                    if shown.is_empty() {
                        ui.label(RichText::new(tr("No actions match")).color(t::p().text_muted));
                    }
                    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                        for d in shown {
                            if confirm.as_deref() == Some(d.id.as_str()) {
                                ui.horizontal(|ui| {
                                    ui.label(trf("Remove {name}?", &[("name", &d.name)]));
                                    if ui.button(RichText::new(tr("Remove")).color(t::p().icon_red)).clicked() {
                                        pick = Some(Pick::Remove(d.id.clone()));
                                    }
                                    if ui.button(tr("Cancel")).clicked() {
                                        self.user_actions.panel.confirm_remove = None;
                                    }
                                });
                                continue;
                            }
                            let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                            let can_run = d.runnable && selected > 0;
                            if resp.hovered() {
                                ui.painter().rect_filled(row, t::RADIUS, t::p().button_hover);
                            }
                            let color = if d.runnable { t::p().text } else { t::p().text_disabled };
                            ui.painter().text(row.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, format!("{}  {}", ic::LIGHTNING, d.name), font(13.0), color);
                            if let Some(c) = super::chord_of(d) {
                                ui.painter().text(row.right_center() - vec2(34.0, 0.0), Align2::RIGHT_CENTER, crate::shortcuts::display(ui.ctx(), &c), font(12.0), t::p().text_muted);
                            }
                            let more = egui::Rect::from_center_size(row.right_center() - vec2(14.0, 0.0), egui::Vec2::splat(22.0));
                            let more_resp = ui.interact(more, ui.scope_id().with(("action-more", &d.id)), Sense::click());
                            ui.painter().text(more.center(), Align2::CENTER_CENTER, ic::DOTS_THREE, font(14.0), t::p().text_muted);
                            let resp = if !d.runnable {
                                resp.on_hover_text(tr("This action has steps Falcon can't run"))
                            } else if selected == 0 {
                                resp.on_hover_text(tr("Select items to run the action on"))
                            } else {
                                resp.on_hover_text(tr_n(selected, "Run on {n} item", "Run on {n} items"))
                            };
                            if resp.clicked() && !more_resp.hovered() && can_run {
                                pick = Some(Pick::Run(d.id.clone()));
                            }
                            egui::Popup::menu(&more_resp).show(|ui| {
                                if ui.add_enabled(d.runnable, egui::Button::new(tr("Edit…"))).clicked() {
                                    pick = Some(Pick::Edit(d.id.clone()));
                                }
                                if ui.add_enabled(d.runnable, egui::Button::new(tr("Duplicate"))).clicked() {
                                    pick = Some(Pick::Duplicate(d.id.clone()));
                                }
                                if ui.button(RichText::new(tr("Remove…")).color(t::p().icon_red)).clicked() {
                                    self.user_actions.panel.confirm_remove = Some(d.id.clone());
                                }
                            });
                        }
                    });
                }
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.user_actions.broken.is_none() && self.ws.lib.is_some(), egui::Button::new(format!("{}  {}", ic::PLUS, tr("New Action")))).clicked() {
                        pick = Some(Pick::New);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let note = if selected == 0 { tr("Nothing selected").to_string() } else { tr_n(selected, "{n} item selected", "{n} items selected") };
                        ui.label(RichText::new(note).color(t::p().text_muted).size(12.0));
                    });
                });
            });
        });
        let opened_now = self.user_actions.panel.opened == ctx.cumulative_frame_nr();
        // A click outside closes it, unless it opened a popup of the panel.
        if !opened_now && ctx.input(|i| i.pointer.any_pressed()) {
            let pos = ctx.input(|i| i.pointer.press_origin());
            let inside = pos.is_some_and(|p| area.response.rect.contains(p));
            if !inside && !egui::Popup::is_any_open(ctx) {
                close = true;
            }
        }
        match pick {
            Some(Pick::Run(id)) => {
                self.run_user_action(&id);
                close = true;
            }
            Some(Pick::Edit(id)) => {
                if let Some(d) = self.user_actions.find(&id) {
                    self.user_actions.editor = Some(Editor::edit(d));
                }
            }
            Some(Pick::New) => self.user_actions.editor = Some(Editor::new_action()),
            Some(Pick::Duplicate(id)) => {
                let taken: Vec<String> = self.user_actions.defs().iter().map(|d| d.name.clone()).collect();
                let name = self.user_actions.find(&id).map(|d| super::copy_name(&d.name, &taken.iter().map(String::as_str).collect::<Vec<_>>())).unwrap_or_default();
                if let Err(e) = self.user_actions.commit(|f| {
                    f.duplicate(&id, &name);
                }) {
                    self.flash_error(trf("Could not save: {e}", &[("e", &e)]));
                }
            }
            Some(Pick::Remove(id)) => {
                self.user_actions.panel.confirm_remove = None;
                if let Err(e) = self.user_actions.commit(|f| f.remove(&id)) {
                    self.flash_error(trf("Could not save: {e}", &[("e", &e)]));
                }
            }
            None => {}
        }
        if close {
            self.user_actions.panel.open = false;
            self.user_actions.panel.confirm_remove = None;
        }
    }
}
