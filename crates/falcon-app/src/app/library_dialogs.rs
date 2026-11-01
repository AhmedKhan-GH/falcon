//! Creating a library: a dialog that asks for a name, then a folder to keep
//! it in, with a warning for folders where a library is easily deleted.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, RichText};
use falcon_library::Library;

use super::App;
use crate::theme as t;

/// Characters a library name may not contain.
const FORBIDDEN: &[char] = &['@', '#', '$', '%', '^', '&', '*', '<', '>', ':', '\'', '"', '/', '\\', '|', '?'];
/// The longest library name kept.
const NAME_LIMIT: usize = 127;

/// Why a library name was refused.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NameError {
    Empty,
    Symbols,
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NameError::Empty => "The name can't be empty.",
            NameError::Symbols => "The name can't contain symbols such as % ^ & * # @ or slashes.",
        })
    }
}

/// Checks a name typed by the user and returns the name to use: stripped of
/// forbidden characters (none are expected after the check), trimmed and cut
/// to 127 characters.
pub fn validate_library_name(name: &str) -> Result<String, NameError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(NameError::Empty);
    }
    if trimmed.contains(FORBIDDEN) {
        return Err(NameError::Symbols);
    }
    let clean: String = trimmed.chars().filter(|c| !FORBIDDEN.contains(c)).collect();
    let clean: String = clean.trim().chars().take(NAME_LIMIT).collect();
    let clean = clean.trim().to_string();
    if clean.is_empty() { Err(NameError::Empty) } else { Ok(clean) }
}

/// Whether a library kept in `folder` is at risk of being deleted by
/// accident: any path containing "Desktop" or "Downloads", as Eagle checks.
pub fn risky_library_folder(folder: &Path) -> bool {
    let s = folder.to_string_lossy();
    s.contains("Desktop") || s.contains("Downloads")
}

/// The create dialog and the steps after it.
#[derive(Default)]
pub struct Create {
    /// What the name field holds.
    pub name: String,
    /// Shown under the name field.
    error: Option<String>,
    /// A folder waiting for the user to accept the risk: the name and folder.
    warn: Option<(String, PathBuf)>,
    /// A valid name whose folder is still to be chosen.
    pick: Option<String>,
    /// Started from the welcome flow, which has no dialog of its own to
    /// return to when the folder choice is cancelled.
    pub from_welcome: bool,
    focus: bool,
}

/// What the dialogs are doing, kept by the app.
#[derive(Default)]
pub struct State {
    pub create: Option<Create>,
}

impl State {
    /// Opens the dialog with an empty name.
    pub fn start_create(&mut self) {
        self.create = Some(Create { focus: true, ..Create::default() });
    }
}

impl App {
    /// Draws whichever create step is showing. Called once a frame.
    pub(super) fn library_dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.dialogs.create.take() else { return };
        let mut keep = true;
        let mut pick = dlg.pick.take();
        if pick.is_none() {
            if let Some((name, folder)) = dlg.warn.clone() {
                match warning_modal(ctx, &folder) {
                    Some(Warning::Another) => {
                        dlg.warn = None;
                        pick = Some(name);
                    }
                    Some(Warning::Anyway) => keep = self.finish_create(&mut dlg, name, folder),
                    Some(Warning::Close) => keep = false,
                    None => {}
                }
            } else {
                match name_modal(ctx, &mut dlg) {
                    Some(true) => match validate_library_name(&dlg.name) {
                        Ok(clean) => {
                            dlg.error = None;
                            pick = Some(clean);
                        }
                        Err(e) => dlg.error = Some(e.to_string()),
                    },
                    Some(false) => keep = false,
                    None => {}
                }
            }
        }
        if let Some(name) = pick {
            keep = self.pick_folder_for(&mut dlg, name);
        }
        if keep {
            self.dialogs.create = Some(dlg);
        }
    }

    /// Asks for the folder (a blocking system dialog) and carries on.
    /// Returns whether the dialog stays open.
    fn pick_folder_for(&mut self, dlg: &mut Create, name: String) -> bool {
        let folder = rfd::FileDialog::new()
            .set_title("Choose where to keep the library")
            .set_directory(falcon_library::sandbox::default_root())
            .pick_folder();
        let Some(folder) = folder else {
            // Cancelled: back to the name, unless there is no dialog to go back to.
            return !dlg.from_welcome;
        };
        if risky_library_folder(&folder) {
            dlg.name = name.clone();
            dlg.warn = Some((name, folder));
            return true;
        }
        self.finish_create(dlg, name, folder)
    }

    /// Creates `<folder>/<name>.library` and opens it. Returns whether the
    /// dialog stays open (an existing library there asks for another folder).
    fn finish_create(&mut self, dlg: &mut Create, name: String, folder: PathBuf) -> bool {
        let dest = folder.join(format!("{name}.library"));
        if dest.exists() {
            dlg.warn = None;
            dlg.name = name;
            dlg.error = Some("A library already exists there. Create again and pick another folder, or change the name.".to_string());
            return true;
        }
        match falcon_library::sandbox::guard(&dest).and_then(|()| Library::create(&dest)) {
            Ok(_) => self.open(dest),
            Err(e) => self.flash(format!("Could not create the library: {e}")),
        }
        false
    }
}

enum Warning {
    Another,
    Anyway,
    Close,
}

fn warning_modal(ctx: &egui::Context, folder: &Path) -> Option<Warning> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("library-folder-warning")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.label(RichText::new("Keep the library here?").size(15.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{} is a folder where files are often cleared out by hand or by the system. A library kept there could be deleted by accident.",
                folder.display()
            ))
            .color(t::p().text_muted),
        );
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("Save Anyway").color(t::p().icon_red)).clicked() {
                out = Some(Warning::Anyway);
            }
            if ui.button("Choose Another").clicked() {
                out = Some(Warning::Another);
            }
        });
    });
    if out.is_none() && modal.should_close() {
        out = Some(Warning::Close);
    }
    out
}

/// The name dialog: `Some(true)` for Create, `Some(false)` for Cancel.
fn name_modal(ctx: &egui::Context, dlg: &mut Create) -> Option<bool> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("create-library")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.label(RichText::new("Create a Library").size(15.0));
        ui.add_space(4.0);
        ui.label(RichText::new("Enter a name for the library, then pick the folder to keep it in.").color(t::p().text_muted));
        ui.add_space(8.0);
        let field = ui.add(egui::TextEdit::singleline(&mut dlg.name).hint_text("Library name").desired_width(f32::INFINITY));
        if dlg.focus {
            field.request_focus();
            dlg.focus = false;
        }
        if let Some(e) = &dlg.error {
            ui.add_space(4.0);
            ui.label(RichText::new(e).size(12.0).color(t::p().icon_red));
        }
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("Create").color(egui::Color32::WHITE)).clicked() {
                out = Some(true);
            }
            if ui.button("Cancel").clicked() {
                out = Some(false);
            }
        });
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            out = Some(true);
        }
    });
    if out.is_none() && modal.should_close() {
        out = Some(false);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(validate_library_name(""), Err(NameError::Empty));
        assert_eq!(validate_library_name("   "), Err(NameError::Empty));
        for c in FORBIDDEN {
            assert_eq!(validate_library_name(&format!("a{c}b")), Err(NameError::Symbols), "{c}");
        }
        assert_eq!(validate_library_name("  My Pictures  "), Ok("My Pictures".to_string()));
        assert_eq!(validate_library_name("Café 2026-1_a"), Ok("Café 2026-1_a".to_string()));
    }

    #[test]
    fn long_names_are_cut() {
        let long = "x".repeat(300);
        assert_eq!(validate_library_name(&long).unwrap().chars().count(), 127);
        let spaced = format!("{} y", "x".repeat(126));
        assert_eq!(validate_library_name(&spaced).unwrap(), "x".repeat(126));
    }

    #[test]
    fn risky_folders() {
        assert!(risky_library_folder(Path::new("/Users/a/Desktop")));
        assert!(risky_library_folder(Path::new("/Users/a/Downloads/x")));
        assert!(risky_library_folder(Path::new("/Users/a/Desktop Stuff")));
        assert!(!risky_library_folder(Path::new("/Users/a/Falcon Sandbox")));
    }
}
