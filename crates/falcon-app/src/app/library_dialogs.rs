//! Creating a library: a dialog that asks for a name, then a folder to keep
//! it in, with a warning for folders where a library is easily deleted. Also
//! the welcome flow shown on first launch, which ends in the same steps.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Key, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use falcon_library::Library;

use super::App;
use crate::theme::{self as t, Theme, font};

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
    /// The welcome flow's step, name field and message.
    welcome: Welcome,
}

/// The steps of the welcome flow.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    #[default]
    Welcome,
    Look,
    Library,
}

impl Step {
    const ALL: [Step; 3] = [Step::Welcome, Step::Look, Step::Library];

    fn index(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// The step after this one; the last step stays.
    pub fn next(self) -> Step {
        Step::ALL[(self.index() + 1).min(Step::ALL.len() - 1)]
    }

    /// The step before this one; the first step stays.
    pub fn back(self) -> Step {
        Step::ALL[self.index().saturating_sub(1)]
    }
}

#[derive(Default)]
struct Welcome {
    step: Step,
    name: String,
    error: Option<String>,
}

/// Whether the welcome flow is skipped for the saved state: it was finished,
/// or libraries were open or used before it existed.
pub fn already_welcomed(welcomed: bool, recent: usize, open: usize, last: bool) -> bool {
    welcomed || recent > 0 || open > 0 || last
}

const EXAMPLES: [&str; 4] = ["Design References", "Photography", "UI Patterns", "Illustration"];

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

impl App {
    /// The screen shown when no library is open: the welcome flow until it
    /// has been finished, the plain empty state otherwise or after an error.
    pub(super) fn empty_or_welcome(&mut self, ui: &mut Ui) {
        if self.welcomed || self.error.is_some() {
            self.empty_state(ui);
        } else {
            self.welcome(ui);
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let r = ui.max_rect();
        super::window_drag(ui, egui::Rect::from_min_size(r.min, vec2(r.width(), t::BAR_HEIGHT)), "welcome");
        ui.painter().rect_filled(r, 0.0, t::p().main);
        let w = 440.0_f32.min(r.width() - 32.0);
        let body = egui::Rect::from_center_size(r.center(), vec2(w, 340.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Center)));
        let ui = &mut ui;
        let step = self.dialogs.welcome.step;
        match step {
            Step::Welcome => {
                ui.add_space(70.0);
                ui.label(RichText::new("Falcon").size(32.0).color(t::p().text));
                ui.add_space(12.0);
                ui.label(RichText::new("Collect, organise and find images and other files in an Eagle-format library.").size(14.0).color(t::p().text_muted));
            }
            Step::Look => {
                ui.add_space(30.0);
                ui.label(RichText::new("Choose a look").size(20.0).color(t::p().text));
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(10.0, 12.0);
                    for theme in Theme::all() {
                        if swatch(ui, theme).clicked() {
                            self.prefs.theme = theme;
                            t::set_theme(ui.ctx(), theme);
                        }
                    }
                });
                ui.add_space(10.0);
                ui.label(RichText::new("You can change this later in Preferences.").size(12.0).color(t::p().text_muted));
            }
            Step::Library => {
                ui.add_space(20.0);
                ui.label(RichText::new("Create your library").size(20.0).color(t::p().text));
                ui.add_space(12.0);
                let field = ui.add(egui::TextEdit::singleline(&mut self.dialogs.welcome.name).hint_text("Library name").desired_width(w));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if let Some(e) = &self.dialogs.welcome.error {
                    ui.label(RichText::new(e).size(12.0).color(t::p().icon_red));
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for example in EXAMPLES {
                        if ui.small_button(example).clicked() {
                            self.dialogs.welcome.name = example.to_string();
                            self.dialogs.welcome.error = None;
                        }
                    }
                });
                ui.add_space(10.0);
                let create = ui.add(egui::Button::new(RichText::new("Create Library").color(egui::Color32::WHITE)).fill(t::p().primary)).clicked();
                if create || enter {
                    match validate_library_name(&self.dialogs.welcome.name) {
                        Ok(clean) => {
                            self.dialogs.welcome.error = None;
                            self.dialogs.create = Some(Create { name: clean.clone(), pick: Some(clean), from_welcome: true, ..Create::default() });
                            ui.ctx().request_repaint();
                        }
                        Err(e) => self.dialogs.welcome.error = Some(e.to_string()),
                    }
                }
                ui.add_space(8.0);
                ui.label(RichText::new("The library is a folder stored on this Mac. Back it up as you would any other files.").size(12.0).color(t::p().text_muted));
                ui.add_space(6.0);
                if ui.link("Open Existing Library…").clicked() {
                    self.pick_library();
                }
            }
        }
        // Step dots and Back / Next along the bottom of the block.
        let foot = egui::Rect::from_min_size(egui::pos2(body.left(), body.bottom() + 8.0), vec2(w, 28.0));
        let dots = foot.center();
        for (i, s) in Step::ALL.iter().enumerate() {
            let c = dots + vec2((i as f32 - 1.0) * 16.0, 0.0);
            let color = if *s == step { t::p().primary } else { t::p().divider };
            ui.painter().circle_filled(c, 4.0, color);
        }
        let mut f = ui.new_child(egui::UiBuilder::new().max_rect(foot).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if step != Step::Welcome && f.button("Back").clicked() {
            self.dialogs.welcome.step = step.back();
        }
        let mut n = ui.new_child(egui::UiBuilder::new().max_rect(foot).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if step != Step::Library && n.button("Next").clicked() {
            self.dialogs.welcome.step = step.next();
        }
    }
}

/// A clickable preview of a theme's colours, named below.
fn swatch(ui: &mut Ui, theme: Theme) -> egui::Response {
    let pal = theme.resolve(ui.ctx().system_theme()).palette();
    let (rect, resp) = ui.allocate_exact_size(vec2(96.0, 74.0), Sense::click());
    let selected = t::current() == theme;
    let preview = egui::Rect::from_min_size(rect.min, vec2(96.0, 52.0));
    let p = ui.painter();
    p.rect_filled(preview, t::RADIUS, pal.main);
    let side = egui::Rect::from_min_size(preview.min, vec2(28.0, 52.0));
    p.rect_filled(side, egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, pal.panel);
    p.circle_filled(preview.center() + vec2(12.0, -6.0), 7.0, pal.primary);
    p.rect_filled(egui::Rect::from_min_size(preview.center() + vec2(-8.0, 8.0), vec2(36.0, 4.0)), 2.0, pal.text_muted);
    let border = if selected { Stroke::new(2.0, t::p().primary) } else if resp.hovered() { Stroke::new(1.0, t::p().text_muted) } else { Stroke::new(1.0, pal.divider) };
    p.rect_stroke(preview, t::RADIUS, border, StrokeKind::Inside);
    p.text(egui::pos2(rect.center().x, rect.bottom() - 8.0), Align2::CENTER_CENTER, theme.to_string(), font(12.0), if selected { t::p().text } else { t::p().text_muted });
    resp
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

    #[test]
    fn steps_stay_in_bounds() {
        assert_eq!(Step::Welcome.back(), Step::Welcome);
        assert_eq!(Step::Welcome.next(), Step::Look);
        assert_eq!(Step::Look.next(), Step::Library);
        assert_eq!(Step::Library.next(), Step::Library);
        assert_eq!(Step::Library.back(), Step::Look);
    }

    #[test]
    fn existing_users_skip_the_welcome() {
        assert!(!already_welcomed(false, 0, 0, false));
        assert!(already_welcomed(true, 0, 0, false));
        assert!(already_welcomed(false, 1, 0, false));
        assert!(already_welcomed(false, 0, 2, false));
        assert!(already_welcomed(false, 0, 0, true));
    }
}
