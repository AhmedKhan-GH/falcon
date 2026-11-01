//! The lock screen shown over the window when the app is locked.

use eframe::egui::{self, Color32, Key, Rect, RichText, TextEdit, Ui, vec2};

use crate::app::App;
use crate::icons as ic;
use crate::theme::{self as t, font};
use crate::ui::widgets::window_drag;

impl App {
    /// The lock screen: the password field, its hint and Unlock.
    pub(crate) fn lock_screen(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let ctx = ui.ctx().clone();
        if ctx.cumulative_frame_nr() == 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        t::follow_system(&ctx);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t::p().main)).show(ui, |ui| {
            window_drag(ui, Rect::from_min_size(rect.min, vec2(rect.width(), t::BAR_HEIGHT)), "lock");
            let card = Rect::from_center_size(rect.center(), vec2(300.0, 220.0));
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(card).layout(egui::Layout::top_down(egui::Align::Center)));
            c.label(RichText::new(ic::LOCK_SIMPLE).font(font(40.0)).color(t::p().text_muted));
            c.add_space(8.0);
            c.label(RichText::new("Falcon is locked").size(16.0).color(t::p().text));
            c.add_space(12.0);
            let field = c.add(
                TextEdit::singleline(&mut self.lock.typed)
                    .password(true)
                    .hint_text(RichText::new("Password").color(t::p().text_muted))
                    .desired_width(220.0)
                    .id_salt("lock-password"),
            );
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            let enter = field.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter));
            c.add_space(8.0);
            let unlock = c.add(egui::Button::new(RichText::new("Unlock").color(Color32::WHITE)).fill(t::p().primary).min_size(vec2(220.0, 0.0))).clicked();
            if (enter || unlock) && self.lock.try_unlock(self.prefs.app_lock.as_ref()) {
                return;
            }
            if self.lock.failed {
                c.label(RichText::new("Wrong password").size(12.0).color(t::p().icon_red));
            }
            let hint = self.prefs.app_lock.as_ref().map(|s| s.hint.clone()).unwrap_or_default();
            if !hint.is_empty() {
                if self.lock.show_hint {
                    c.label(RichText::new(hint).size(12.0).color(t::p().text_muted));
                } else if c.link(RichText::new("Show hint").size(12.0)).clicked() {
                    self.lock.show_hint = true;
                }
            }
        });
    }
}
