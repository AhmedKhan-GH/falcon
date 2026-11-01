//! The smart folder dialog: name and conditions of a smart folder.

use eframe::egui::{self, RichText, TextEdit};
use falcon_library::Library;
use falcon_library::smart_draft;

use crate::app::App;
use crate::theme as t;

impl App {
    pub(crate) fn smart_dialog_ui(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.smart_dialog.take() else { return };
        let mut keep = true;
        let folders = crate::ui::organize::folder_choices(&lib.metadata.folders);
        let max_rules_height = (ctx.content_rect().height() - 320.0).max(160.0);
        let modal = egui::Modal::new(egui::Id::unique("smart-dialog")).show(ctx, |ui| {
            ui.set_width(660.0);
            ui.label(RichText::new(if d.editing.is_some() { "Edit Smart Folder" } else { "New Smart Folder" }).size(16.0));
            ui.add_space(6.0);
            ui.add(TextEdit::singleline(&mut d.name).hint_text("Name").desired_width(f32::INFINITY));
            ui.add_space(6.0);
            App::smart_dialog_extras(ui, &mut d, lib);
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(max_rules_height).auto_shrink([false, true]).show(ui, |ui| {
                crate::ui::organize::smart_conditions_ui(ui, &mut d.conditions, &folders);
            });
            let result = smart_draft::conditions(&d.conditions);
            if let Err(e) = &result {
                ui.add_space(4.0);
                ui.label(RichText::new(e).color(t::p().icon_red));
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = !d.name.trim().is_empty() && !d.conditions.is_empty() && result.is_ok();
                let label = if d.editing.is_some() { "Save" } else { "Create" };
                if ui.add_enabled(ok, egui::Button::new(label)).clicked()
                    && let Ok(conditions) = result
                {
                    let name = d.name.trim().to_string();
                    self.save_smart_draft(&d, name, conditions);
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
            self.ws.smart_dialog = Some(d);
        }
    }
}
