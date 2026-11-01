//! The lock screen shown over the window when the app is locked.

use eframe::egui::{self, Color32, Key, Rect, RichText, TextEdit, Ui, vec2};

use crate::app::App;
use crate::icons as ic;
use crate::locale::tr;
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
            c.label(RichText::new(tr("Falcon is locked")).size(16.0).color(t::p().text));
            c.add_space(12.0);
            let field = c.add(
                TextEdit::singleline(&mut self.lock.typed)
                    .password(true)
                    .hint_text(RichText::new(tr("Password")).color(t::p().text_muted))
                    .desired_width(220.0)
                    .id_salt("lock-password"),
            );
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            let enter = field.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter));
            c.add_space(8.0);
            let unlock = c.add(egui::Button::new(RichText::new(tr("Unlock")).color(Color32::WHITE)).fill(t::p().primary).min_size(vec2(220.0, 0.0))).clicked();
            if (enter || unlock) && self.lock.try_unlock(self.prefs.app_lock.as_ref()) {
                return;
            }
            if self.lock.failed {
                c.label(RichText::new(tr("Wrong password")).size(12.0).color(t::p().icon_red));
            }
            if self.touch_id_button(&mut c) {
                return;
            }
            let hint = self.prefs.app_lock.as_ref().map(|s| s.hint.clone()).unwrap_or_default();
            if !hint.is_empty() {
                if self.lock.show_hint {
                    c.label(RichText::new(hint).size(12.0).color(t::p().text_muted));
                } else if c.link(RichText::new(tr("Show hint")).size(12.0)).clicked() {
                    self.lock.show_hint = true;
                }
            }
        });
    }

    /// Eagle's Touch ID button on the lock screen, when it is turned on in
    /// Preferences and the Mac can use it; the password still works.
    /// Returns whether a fingerprint unlocked the app.
    fn touch_id_button(&mut self, ui: &mut Ui) -> bool {
        if let Some(ticket) = self.lock.touch_ticket
            && let Some(ok) = crate::touch_id::poll(ticket)
        {
            return self.lock.touch_answer(ok);
        }
        if !self.prefs.touch_id || self.prefs.app_lock.is_none() || !crate::touch_id::available() {
            return false;
        }
        ui.add_space(4.0);
        let waiting = self.lock.touch_ticket.is_some();
        let label = RichText::new(format!("{}  Use Touch ID", ic::LOCK_SIMPLE_OPEN)).color(t::p().text);
        if ui.add_enabled(!waiting, egui::Button::new(label).min_size(vec2(220.0, 0.0))).clicked() {
            self.lock.touch_failed = false;
            self.lock.touch_ticket = crate::touch_id::start("unlock Falcon", ui.ctx());
        }
        if self.lock.touch_failed {
            ui.label(RichText::new("Touch ID didn't unlock Falcon").size(12.0).color(t::p().icon_red));
        }
        false
    }
}
