//! Per-library Storage settings modal (`Library Settings…`), opened from the
//! library switcher / menu. Only functional Phase A Storage controls are shown.

use eframe::egui::{self, RichText};
use falcon_library::Library;
use falcon_library::linked::AssetStatus;
use falcon_library::storage_mode::StorageMode;

use crate::app::App;
use crate::locale::{tr, trf};
use crate::theme as t;
use crate::ui::widgets::{divider, prop};

/// Modal state for the active library's settings dialog.
#[derive(Default)]
pub(crate) struct LibrarySettings {
    pub open: bool,
}

impl App {
    /// Opens the per-library Settings dialog.
    pub(crate) fn open_library_settings(&mut self) {
        if self.ws.lib.is_some() {
            self.ws.library_settings.open = true;
        }
    }

    /// Draws the per-library Settings dialog while open.
    pub(crate) fn library_settings_dialog(&mut self, ctx: &egui::Context, lib: &mut Library) {
        if !self.ws.library_settings.open {
            return;
        }
        let mut keep = true;
        let lib_name = lib
            .root()
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| lib.root().display().to_string());
        let mut mode = lib.storage_mode();
        let initial_mode = mode;

        let mut managed_count = 0usize;
        let mut linked_count = 0usize;
        let mut unavailable_count = 0usize;
        for item in lib.items() {
            match lib.asset_status(&item.id).unwrap_or(AssetStatus::Managed) {
                AssetStatus::Managed => managed_count += 1,
                AssetStatus::Linked => linked_count += 1,
                AssetStatus::Offline | AssetStatus::Missing | AssetStatus::InvalidReference => {
                    linked_count += 1;
                    unavailable_count += 1;
                }
            }
        }

        let modal = egui::Modal::new(egui::Id::unique("library-settings")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new(tr("Library Settings")).size(16.0));
            ui.label(RichText::new(lib_name).size(12.0).color(t::p().text_muted));
            ui.add_space(8.0);
            divider(ui);
            ui.add_space(6.0);

            ui.label(RichText::new(tr("Storage")).size(13.0));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Default Import Mode")).color(t::p().text_value));
            ui.add_space(4.0);
            ui.radio_value(&mut mode, StorageMode::Copy, tr("Copy to Library"))
                .on_hover_text(tr("Copies imported files into the library folder."));
            ui.indent("copy-desc", |ui| {
                ui.label(
                    RichText::new(tr("Copies imported files into the library folder."))
                        .size(11.5)
                        .color(t::p().text_muted),
                );
            });
            ui.add_space(4.0);
            ui.radio_value(&mut mode, StorageMode::Link, tr("Link (Place)"))
                .on_hover_text(tr(
                    "Keeps original files at their existing paths without copying payloads.",
                ));
            ui.indent("link-desc", |ui| {
                ui.label(
                    RichText::new(tr(
                        "Keeps original files at their existing paths without copying payloads.",
                    ))
                    .size(11.5)
                    .color(t::p().text_muted),
                );
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr(
                    "Changes apply to future imports only. Existing items stay as they are.",
                ))
                .size(11.5)
                .color(t::p().text_muted),
            );

            ui.add_space(8.0);
            divider(ui);
            ui.add_space(4.0);
            let loc = lib.root().display().to_string();
            prop(ui, tr("Location"), &loc).on_hover_text(&loc);
            prop(ui, tr("Managed Assets"), &managed_count.to_string());
            prop(ui, tr("Linked Assets"), &linked_count.to_string());
            prop(
                ui,
                tr("Unavailable Originals"),
                &unavailable_count.to_string(),
            );

            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Done")).clicked() {
                    keep = false;
                }
            });
        });

        if mode != initial_mode
            && let Err(e) = lib.set_storage_mode(mode)
        {
            self.flash_error(trf(
                "Could not save Library Settings: {error}",
                &[("error", &e)],
            ));
        }

        if modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        self.ws.library_settings.open = keep;
    }
}
