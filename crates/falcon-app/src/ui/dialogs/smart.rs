//! The smart folder dialog: name and conditions of a smart folder.

use eframe::egui::{self, RichText, TextEdit, Ui};
use falcon_library::Library;
use falcon_library::smart_draft;

use crate::app::App;
use crate::locale::tr;
use crate::theme as t;

impl App {
    pub(crate) fn smart_dialog_ui(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut d) = self.ws.smart_dialog.take() else { return };
        let mut keep = true;
        let folders = crate::ui::organize::folder_choices(&lib.metadata.folders);
        let max_rules_height = (ctx.content_rect().height() - 320.0).max(160.0);
        let modal = egui::Modal::new(egui::Id::unique("smart-dialog")).show(ctx, |ui| {
            ui.set_width(660.0);
            ui.label(RichText::new(if d.editing.is_some() { tr("Edit Smart Folder") } else { tr("New Smart Folder") }).size(16.0));
            ui.add_space(6.0);
            ui.add(TextEdit::singleline(&mut d.name).hint_text(tr("Name")).desired_width(f32::INFINITY));
            ui.add_space(6.0);
            App::smart_dialog_extras(ui, &mut d, lib);
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(max_rules_height).auto_shrink([false, true]).show(ui, |ui| {
                crate::ui::organize::smart_conditions_ui(ui, &mut d.conditions, &folders);
            });
            let result = smart_draft::conditions(&d.conditions);
            if let Err(e) = &result {
                ui.add_space(4.0);
                ui.label(RichText::new(crate::ui::organize::smart_error(e)).color(t::p().icon_red));
            }
            App::smart_live_count(ui, &mut d, result.as_ref().ok(), lib);
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = !d.name.trim().is_empty() && !d.conditions.is_empty() && result.is_ok();
                let label = if d.editing.is_some() { tr("Save") } else { tr("Create") };
                if ui.add_enabled(ok, egui::Button::new(label)).clicked()
                    && let Ok(conditions) = result
                {
                    let name = d.name.trim().to_string();
                    self.save_smart_draft(&d, name, conditions);
                    keep = false;
                }
                if ui.button(tr("Cancel")).clicked() {
                    keep = false;
                }
                // The live count fills the rest of the row, at its left.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let text = match d.live.shown() {
                        Some((n, current)) => {
                            let c = if current { t::p().text_muted } else { t::p().text_disabled };
                            Some(RichText::new(crate::smart_count::found_text(n)).color(c))
                        }
                        None if d.live.counting() => Some(RichText::new(crate::locale::tr("Counting…")).color(t::p().text_disabled)),
                        None => None,
                    };
                    if let Some(text) = text {
                        ui.label(text);
                    }
                });
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.ws.smart_dialog = Some(d);
        }
    }

    /// Starts counting the rules shown, once they have settled, and asks
    /// for a redraw when the wait is over (`smart_count.rs`).
    fn smart_live_count(ui: &Ui, d: &mut crate::app::SmartDraft, conditions: Option<&Vec<serde_json::Value>>, lib: &Library) {
        use crate::smart_count::{Snapshot, Step, key};
        d.live.receive();
        let k = conditions.filter(|c| !c.is_empty()).map(|c| key(c, d.parent.as_deref()));
        match d.live.step(k.as_deref(), std::time::Instant::now()) {
            Step::Wait(Some(after)) => ui.ctx().request_repaint_after(after),
            Step::Wait(None) => {}
            Step::Start(k) => {
                let snapshot = || Snapshot {
                    items: lib.items().to_vec(),
                    folders: falcon_library::search::folder_index(&lib.metadata.folders),
                    roots: lib.smart_folders(),
                };
                d.live.start(k, conditions.cloned().unwrap_or_default(), d.parent.clone(), snapshot, ui.ctx());
            }
        }
    }
}
