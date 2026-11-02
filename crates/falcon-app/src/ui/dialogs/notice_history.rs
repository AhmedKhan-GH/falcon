//! The Notification Center (Window ▸ Notification Center): a window listing
//! the toasts and notices shown, newest first. A falcon addition; Eagle has
//! no such panel. The list is `crate::notice_history`.

use eframe::egui::{self, Align, Layout, RichText, ScrollArea, Sense, vec2};

use crate::app::App;
use crate::locale::tr;
use crate::notice_history::{Kind, format_time, now};
use crate::theme as t;

impl App {
    /// Draws the window while `notice_window_open` is set. It does not block
    /// the app, and opens with or without a library.
    pub(crate) fn notice_history_ui(&mut self, ctx: &egui::Context) {
        if !self.notice_window_open {
            return;
        }
        let mut open = true;
        let mut clear = false;
        let offset = {
            use chrono::Offset;
            chrono::Local::now().offset().fix().local_minus_utc()
        };
        let now = now();
        egui::Window::new(tr("Notification Center")).open(&mut open).default_size(vec2(380.0, 420.0)).collapsible(false).show(ctx, |ui| {
            let p = t::p();
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!self.notices.is_empty(), egui::Button::new(tr("Clear"))).clicked() {
                        clear = true;
                    }
                });
            });
            ui.separator();
            if self.notices.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(tr("No notifications")).color(p.text_muted)));
                return;
            }
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for e in self.notices.newest_first() {
                    ui.horizontal_top(|ui| {
                        ui.add_sized(vec2(84.0, 16.0), egui::Label::new(RichText::new(format_time(e.at, now, offset)).size(11.0).color(p.text_muted)).sense(Sense::hover()));
                        if e.kind == Kind::Error {
                            ui.label(RichText::new("●").color(p.icon_red));
                        }
                        if e.count > 1 {
                            ui.label(RichText::new(format!("×{}", e.count)).size(11.0).color(p.text_muted));
                        }
                        ui.add(egui::Label::new(RichText::new(&e.text).color(p.text)).wrap().selectable(true));
                    });
                    ui.add_space(4.0);
                }
            });
        });
        if clear {
            self.notices.clear();
        }
        self.notice_window_open = open;
    }
}
