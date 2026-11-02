//! The tips overlay (Help ▸ Show Tips, and once on first launch). A falcon
//! addition. Rules and texts are in `crate::tips`.

use eframe::egui::{self, Align, Layout, RichText};

use crate::app::App;
use crate::locale::{tr, trf};
use crate::theme as t;
use crate::tips::{TIPS, should_show_on_launch};

impl App {
    /// Opens the tips on their own once (see `should_show_on_launch`), then
    /// draws them while open. Esc, the close button and Done all dismiss them
    /// and remember it; a click outside does not.
    pub(crate) fn tips_ui(&mut self, ctx: &egui::Context) {
        if !self.tips.open && should_show_on_launch(self.tips_seen, self.welcomed, self.ws.lib.is_some(), self.any_modal_open()) {
            self.tips.show();
        }
        if !self.tips.open {
            return;
        }
        let tip = &TIPS[self.tips.index.min(TIPS.len() - 1)];
        let body = match tip.with_key {
            Some((cmd, keyed)) => match self.shortcut_text(ctx, cmd) {
                keys if keys.is_empty() => tr(tip.body).to_string(),
                keys => trf(keyed, &[("key", &keys)]),
            },
            None => tr(tip.body).to_string(),
        };
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Modal::new(egui::Id::unique("tips")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr(tip.title)).size(18.0).color(t::p().text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(crate::icons::X).on_hover_text(tr("Close")).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            ui.label(RichText::new(body).color(t::p().text));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(trf("{n} of {total}", &[("n", &(self.tips.index + 1)), ("total", &TIPS.len())])).color(t::p().text_muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.tips.is_last() {
                        if ui.button(tr("Done")).clicked() {
                            close = true;
                        }
                    } else if ui.button(tr("Next")).clicked() {
                        self.tips.next();
                    }
                    if ui.add_enabled(!self.tips.is_first(), egui::Button::new(tr("Back"))).clicked() {
                        self.tips.back();
                    }
                });
            });
        });
        if close {
            self.tips.open = false;
            self.tips_seen = true;
        }
    }
}
