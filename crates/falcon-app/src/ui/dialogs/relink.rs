//! Dialog for Missing Media & Relink.
//!
//! Allows reconnecting missing referenced assets via single file or folder scan,
//! showing matched, ambiguous, unmatched, and incompatible candidate previews
//! with explicit confirmation before committing changes.

use std::path::PathBuf;

use eframe::egui::{self, Frame, RichText};
use falcon_library::Library;
use falcon_library::relink::{ItemRelinkStatus, RelinkPreview};

use crate::app::App;
use crate::locale::{tr, trf};
use crate::theme::{self as t};

/// In-flight state of the Relink dialog.
#[derive(Debug, Clone, Default)]
pub struct RelinkDialogState {
    pub open: bool,
    pub target_ids: Vec<String>,
    pub search_folder: Option<PathBuf>,
    pub preview: Option<RelinkPreview>,
    pub accepted_pairs: Vec<(String, PathBuf)>,
    pub scanning: bool,
    pub status_message: Option<String>,
}

impl RelinkDialogState {
    pub fn open_for(&mut self, lib: &Library, ids: Vec<String>) {
        self.open = true;
        self.target_ids = ids;
        self.search_folder = None;
        self.accepted_pairs.clear();
        self.scanning = false;
        self.status_message = None;

        // Populate initial preview with items
        let mut preview = RelinkPreview::default();
        for id in &self.target_ids {
            if let Some(item) = lib.item(id) {
                let prev_src = lib.last_known_source_path(id);
                preview.unmatched.push(ItemRelinkStatus {
                    item_id: item.id.clone(),
                    item_name: item.name.clone(),
                    ext: item.ext.clone(),
                    previous_source: prev_src,
                    candidate: None,
                    candidates_count: 0,
                    error: None,
                });
            }
        }
        self.preview = Some(preview);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.target_ids.clear();
        self.search_folder = None;
        self.preview = None;
        self.accepted_pairs.clear();
        self.status_message = None;
    }
}

impl App {
    pub(crate) fn open_relink_dialog(&mut self, lib: &Library, ids: Vec<String>) {
        self.ws.relink_dialog.open_for(lib, ids);
    }

    pub(crate) fn relink_dialog(&mut self, ctx: &egui::Context, lib: &mut Library) {
        if !self.ws.relink_dialog.open {
            return;
        }

        let mut close_requested = false;
        let mut locate_file_clicked = false;
        let mut locate_folder_clicked = false;
        let mut confirm_apply = false;

        let total_items = self.ws.relink_dialog.target_ids.len();

        let modal = egui::Modal::new(egui::Id::unique("relink-missing-media")).show(ctx, |ui| {
            ui.set_width(580.0);
            ui.label(RichText::new(tr("Relink Missing Media")).size(16.0));
            ui.label(
                RichText::new(trf(
                    "Reconnect missing original files for {n} selected asset(s). Metadata, tags, notes, and ratings are preserved.",
                    &[("n", &total_items.to_string())],
                ))
                .size(12.0)
                .color(t::p().text_muted),
            );
            ui.add_space(8.0);

            // Action Bar: Locate File / Locate Folder
            ui.horizontal(|ui| {
                if total_items == 1 && ui.button(tr("Locate File…")).clicked() {
                    locate_file_clicked = true;
                }
                if ui.button(tr("Locate Folder…")).clicked() {
                    locate_folder_clicked = true;
                }
                if let Some(folder) = &self.ws.relink_dialog.search_folder {
                    ui.label(
                        RichText::new(format!("{}: {}", tr("Folder"), folder.display()))
                            .size(11.5)
                            .color(t::p().icon_blue),
                    );
                }
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // Candidate Review Section
            if let Some(preview) = &self.ws.relink_dialog.preview {
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        // 1. Matched candidates
                        if !preview.matched.is_empty() {
                            ui.label(
                                RichText::new(format!("{} ({})", tr("Matched"), preview.matched.len()))
                                    .size(13.0)
                                    .color(t::p().icon_green),
                            );
                            for item in &preview.matched {
                                Frame::default()
                                    .fill(t::p().row_hover)
                                    .corner_radius(4.0)
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&item.item_name).strong());
                                            ui.label(format!(".{}", item.ext));
                                        });
                                        if let Some(cand) = &item.candidate {
                                            ui.label(
                                                RichText::new(format!("➔ {}", cand.candidate_path.display()))
                                                    .size(11.5)
                                                    .color(t::p().text_muted),
                                            );
                                            ui.label(
                                                RichText::new(format!("{} ({} bytes)", cand.reason, cand.candidate_size))
                                                    .size(11.0)
                                                    .color(t::p().icon_green),
                                            );
                                        }
                                    });
                                ui.add_space(2.0);
                            }
                            ui.add_space(6.0);
                        }

                        // 2. Ambiguous candidates
                        if !preview.ambiguous.is_empty() {
                            ui.label(
                                RichText::new(format!("{} ({})", tr("Ambiguous"), preview.ambiguous.len()))
                                    .size(13.0)
                                    .color(t::p().icon_yellow),
                            );
                            for item in &preview.ambiguous {
                                Frame::default()
                                    .fill(t::p().row_hover)
                                    .corner_radius(4.0)
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&item.item_name).strong());
                                            ui.label(format!(".{}", item.ext));
                                            ui.label(RichText::new(tr("Requires Confirmation")).color(t::p().icon_yellow));
                                        });
                                        if let Some(cand) = &item.candidate {
                                            ui.label(
                                                RichText::new(format!("➔ {}", cand.candidate_path.display()))
                                                    .size(11.5)
                                                    .color(t::p().text_muted),
                                            );
                                            ui.label(
                                                RichText::new(&cand.reason)
                                                    .size(11.0)
                                                    .color(t::p().icon_yellow),
                                            );
                                        }
                                    });
                                ui.add_space(2.0);
                            }
                            ui.add_space(6.0);
                        }

                        // 3. Unmatched candidates
                        if !preview.unmatched.is_empty() {
                            ui.label(
                                RichText::new(format!("{} ({})", tr("Unmatched"), preview.unmatched.len()))
                                    .size(13.0)
                                    .color(t::p().text_muted),
                            );
                            for item in &preview.unmatched {
                                Frame::default()
                                    .fill(t::p().row_hover)
                                    .corner_radius(4.0)
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&item.item_name).strong());
                                            ui.label(format!(".{}", item.ext));
                                        });
                                        if let Some(src) = &item.previous_source {
                                            ui.label(
                                                RichText::new(format!("{}: {}", tr("Last Path"), src.display()))
                                                    .size(11.0)
                                                    .color(t::p().text_muted),
                                            );
                                        }
                                    });
                                ui.add_space(2.0);
                            }
                            ui.add_space(6.0);
                        }

                        // 4. Incompatible candidates
                        if !preview.incompatible.is_empty() {
                            ui.label(
                                RichText::new(format!("{} ({})", tr("Incompatible"), preview.incompatible.len()))
                                    .size(13.0)
                                    .color(t::p().icon_red),
                            );
                            for item in &preview.incompatible {
                                Frame::default()
                                    .fill(t::p().row_hover)
                                    .corner_radius(4.0)
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&item.item_name).strong());
                                            ui.label(format!(".{}", item.ext));
                                        });
                                        if let Some(err) = &item.error {
                                            ui.label(RichText::new(err).size(11.0).color(t::p().icon_red));
                                        }
                                    });
                                ui.add_space(2.0);
                            }
                        }
                    });
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // Bottom button controls
            ui.horizontal(|ui| {
                let matched_count = self
                    .ws
                    .relink_dialog
                    .preview
                    .as_ref()
                    .map(|p| p.matched.len())
                    .unwrap_or(0);

                let apply_btn = ui.add_enabled(
                    matched_count > 0,
                    egui::Button::new(RichText::new(format!("{} ({})", tr("Relink"), matched_count)).strong()),
                );
                if apply_btn.clicked() {
                    confirm_apply = true;
                }

                if ui.button(tr("Cancel")).clicked() {
                    close_requested = true;
                }
            });
        });

        if modal.should_close() {
            close_requested = true;
        }

        if locate_file_clicked
            && let Some(item_id) = self.ws.relink_dialog.target_ids.first()
            && let Some(item) = lib.item(item_id).cloned()
        {
            let title = tr("Locate Original File");
            let mut dialog = crate::dialog_owner::file_dialog().set_title(title);
            if let Some(prev) = lib.last_known_source_path(&item.id)
                && let Some(parent) = prev.parent()
            {
                dialog = dialog.set_directory(parent);
            }
            if let Some(chosen) = dialog.pick_file() {
                match lib.relink_item_to_file(&item.id, &chosen) {
                    Ok(_) => {
                        self.flash(trf(
                            "Successfully relinked '{name}'",
                            &[("name", &item.name)],
                        ));
                        close_requested = true;
                    }
                    Err(e) => {
                        self.flash_error(trf("Relink failed: {e}", &[("e", &e.to_string())]));
                    }
                }
            }
        }

        if locate_folder_clicked {
            let title = tr("Locate Folder Containing Missing Files");
            let dialog = crate::dialog_owner::file_dialog().set_title(title);
            if let Some(chosen) = dialog.pick_folder() {
                self.ws.relink_dialog.search_folder = Some(chosen.clone());
                let missing_items: Vec<_> = self
                    .ws
                    .relink_dialog
                    .target_ids
                    .iter()
                    .filter_map(|id| lib.item(id).cloned())
                    .collect();
                let preview = lib.preview_relink_folder(&missing_items, &chosen);
                self.ws.relink_dialog.preview = Some(preview);
            }
        }

        if confirm_apply
            && let Some(preview) = &self.ws.relink_dialog.preview
        {
            let pairs: Vec<(String, PathBuf)> = preview
                .matched
                .iter()
                .filter_map(|s| {
                    s.candidate.as_ref().map(|c| (s.item_id.clone(), c.candidate_path.clone()))
                })
                .collect();

            let count = pairs.len();
            match lib.apply_relink_batch(&pairs) {
                Ok(_) => {
                    self.flash(trf(
                        "Relinked {n} missing asset(s)",
                        &[("n", &count.to_string())],
                    ));
                    close_requested = true;
                }
                Err(e) => {
                    self.flash_error(trf("Relink batch failed: {e}", &[("e", &e.to_string())]));
                }
            }
        }

        if close_requested {
            self.ws.relink_dialog.close();
        }
    }
}
