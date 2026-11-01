//! The inspector's Type row opens this dialog to change a file's extension
//! (the file is not converted).

use eframe::egui::{self, Key, RichText, TextEdit};
use falcon_library::item_ops::check_extension;

use super::{Action, App};
use crate::theme as t;

/// The dialog as typed so far.
pub(super) struct ExtDialog {
    pub id: String,
    pub current: String,
    pub text: String,
    /// Stable across frames, so the field keeps its focus and cursor.
    pub uid: egui::Id,
    /// Focus and select-all happen on the first frame only.
    pub focused: bool,
}

impl App {
    pub(super) fn ext_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.ws.ext_dialog.take() else { return };
        let mut keep = true;
        let modal = egui::Modal::new(d.uid).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(RichText::new("Change Extension").size(15.0));
            ui.add_space(2.0);
            ui.label(RichText::new("This does not convert the file, and it may become unreadable.").color(t::p().text_muted));
            ui.add_space(6.0);
            let te = ui.add(TextEdit::singleline(&mut d.text).id(d.uid.with("field")).hint_text("Extension").desired_width(f32::INFINITY));
            if !d.focused {
                d.focused = true;
                te.request_focus();
                if let Some(mut st) = TextEdit::load_state(ui.ctx(), te.id) {
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(d.text.chars().count()))));
                    st.store(ui.ctx(), te.id);
                }
            }
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let checked = check_extension(&d.text);
            if let Err(e) = &checked {
                ui.add_space(4.0);
                ui.label(RichText::new(e.to_string()).color(t::p().text_muted));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = checked.is_ok();
                if (ui.add_enabled(ok, egui::Button::new("Change")).clicked() || (enter && ok)) && keep {
                    if let Ok(ext) = &checked
                        && *ext != d.current
                    {
                        self.ws.actions.push(Action::SetExtension(d.id.clone(), ext.clone()));
                    }
                    keep = false;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.ws.ext_dialog = Some(d);
        }
    }
}
