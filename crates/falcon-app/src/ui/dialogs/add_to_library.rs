//! Add to Other Library (`falcon_library::add_to_library`): the submenu in
//! the item, folder, smart folder and tag group menus, Eagle's confirmation
//! for several items, and the copy behind the progress modal.
//!
//! The submenu lists the recent libraries except the shown one, as Eagle's
//! does, then Choose Library… for any other. The other library must pass the
//! sandbox and install-folder guards. When it is open in falcon too, the
//! copied items are registered through its own handle, so its window sees
//! them at once; otherwise it is opened just for the write.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, Ui};
use falcon_library::Library;
use falcon_library::add_to_library::{self as add, AddPlan};

use crate::app::{App, lock};
use crate::theme as t;
use crate::ui::dialogs::export::plural;
use crate::ui::dialogs::library_notices::Alert;
use crate::ui::dialogs::progress::Outcome;

/// What is added to the other library.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AddWhat {
    Items(Vec<String>),
    Folder(String),
    Smart(String),
    TagGroup(String),
}

/// The confirmation waiting for an answer.
#[derive(Default)]
pub(crate) struct State {
    confirm: Option<(AddWhat, PathBuf)>,
}

/// A library's name for messages.
fn name(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

/// Whether Eagle asks first: more than one item.
pub(crate) fn asks_first(what: &AddWhat) -> bool {
    matches!(what, AddWhat::Items(ids) if ids.len() > 1)
}

/// The toast for a finished copy.
pub(crate) fn summary(target: &str, what: &AddWhat, copied: &add::Copied, report: &add::AddReport, total: usize, stopped: bool) -> String {
    let n = report.items;
    let mut msg = match what {
        AddWhat::Items(_) | AddWhat::Folder(_) if stopped => format!("Stopped after adding {n} of {} to {target}", plural(total, "item")),
        AddWhat::Items(_) => format!("Added {} to {target}", plural(n, "item")),
        AddWhat::Folder(_) if report.definition => format!("Added the folder and {} to {target}", plural(n, "item")),
        AddWhat::Folder(_) => format!("Added {} to {target}; it already had the folder", plural(n, "item")),
        AddWhat::Smart(_) => format!("Added the smart folder to {target}"),
        AddWhat::TagGroup(_) => format!("Added the tag group to {target}"),
    };
    if !copied.present.is_empty() {
        let k = copied.present.len();
        msg.push_str(&if k == 1 { "; 1 item was already there".to_string() } else { format!("; {k} items were already there") });
    }
    msg
}

impl App {
    /// The Add to Other Library submenu.
    pub(crate) fn add_to_library_menu(&mut self, ui: &mut Ui, what: AddWhat) {
        let current = self.ws.root.clone();
        crate::ui::menus::search::submenu(ui, "Add to Other Library", |ui| {
            ui.set_min_width(240.0);
            let others: Vec<PathBuf> = self.recent.iter().filter(|p| !current.as_deref().is_some_and(|c| crate::app::library::same_path(c, p))).cloned().collect();
            if others.is_empty() {
                ui.label(RichText::new("No other recent libraries").color(t::p().text_muted));
            }
            for p in others {
                let missing = !p.is_dir();
                let row = ui.add_enabled(!missing, egui::Button::new(crate::ui::sidebar::library_label(&p, missing)));
                if row.on_hover_text(p.display().to_string()).clicked() {
                    self.choose_add_target(what.clone(), p);
                }
            }
            ui.separator();
            if ui.button("Choose Library…").clicked()
                && let Some(p) = rfd::FileDialog::new().set_title("Choose a Library to Add To").pick_folder()
            {
                self.choose_add_target(what.clone(), p);
            }
        });
    }

    /// Checks the chosen library, then asks (several items) or starts.
    fn choose_add_target(&mut self, what: AddWhat, target: PathBuf) {
        let Some(current) = self.ws.root.clone() else { return };
        let checked = crate::install_dir::guard(&target).map_err(add::AddRefusal::from).and_then(|()| add::check_target(&current, &target));
        if let Err(e) = checked {
            self.show_alert(Alert::new("Can't add to that library", e.to_string()));
            return;
        }
        if asks_first(&what) {
            self.tools.add.confirm = Some((what, target));
        } else {
            self.start_add(what, target);
        }
    }

    /// The confirmation for several items.
    pub(crate) fn add_to_library_dialog(&mut self, ctx: &egui::Context) {
        let Some((what, target)) = self.tools.add.confirm.clone() else { return };
        let n = match &what {
            AddWhat::Items(ids) => ids.len(),
            _ => 0,
        };
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("add-to-library-confirm")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(format!("Add {} to {}", plural(n, "item"), name(&target))).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new("Their files and details are copied into the other library. This library keeps them as they are.").color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Add").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        if let Some(go) = answer {
            self.tools.add.confirm = None;
            if go {
                self.start_add(what, target);
            }
        }
    }

    /// Copies behind the progress modal; Cancel stops before the next item.
    fn start_add(&mut self, what: AddWhat, target: PathBuf) {
        let Some(shared) = self.ws.lib.clone() else { return };
        if self.operation_running() {
            return;
        }
        let plan = {
            let lib = lock(&shared);
            match &what {
                AddWhat::Items(ids) => Some(AddPlan::items(&lib, &target, ids.clone())),
                AddWhat::Folder(id) => AddPlan::folder(&lib, &target, id),
                AddWhat::Smart(id) => AddPlan::smart_folder(&lib, &target, id),
                AddWhat::TagGroup(id) => AddPlan::tag_group(&lib, &target, id),
            }
        };
        let Some(plan) = plan else { return };
        // The other library's own handle when it is open here too.
        let open = self.others.iter(&self.ws).find(|w| w.root.as_deref().is_some_and(|r| crate::app::library::same_path(r, &target))).and_then(|w| w.lib.clone());
        let target_name = name(&target);
        let total = plan.items.len();
        let h = self.ws.op.start(&self.ctx, format!("Copying to {target_name}"), total, true);
        std::thread::spawn(move || {
            let names: Vec<String> = {
                let lib = lock(&shared);
                plan.items.iter().map(|id| lib.item(id).map(|i| format!("{}.{}", i.name, i.ext)).unwrap_or_default()).collect()
            };
            let copied = add::copy_items(&plan, h.cancel_flag(), |n, _| h.step(n, names.get(n).cloned().unwrap_or_default()));
            let stopped = h.cancelled() && copied.copied.len() + copied.present.len() + copied.failed.len() < total;
            let finished = match &open {
                Some(other) => lock(other).finish_add(&plan, &copied.copied),
                None => Library::open(&plan.target).and_then(|mut lib| lib.finish_add(&plan, &copied.copied)),
            };
            for (id, why) in &copied.failed {
                crate::errlog::error(&format!("Add to Other Library: couldn't copy {id}: {why}"));
            }
            let outcome = match finished {
                Ok(report) => {
                    let message = summary(&target_name, &what, &copied, &report, total, stopped);
                    let error = (!copied.failed.is_empty()).then(|| format!("{message}; {} couldn't be copied", plural(copied.failed.len(), "item")));
                    Outcome { message, error, ..Default::default() }
                }
                Err(e) => Outcome {
                    alert: Some(Alert::new("Not finished", format!("The files were copied, but {target_name} couldn't be updated: {e}. Opening it again picks the copied items up."))),
                    ..Default::default()
                },
            };
            h.finish(outcome);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_items_ask_first() {
        assert!(!asks_first(&AddWhat::Items(vec!["A".into()])));
        assert!(asks_first(&AddWhat::Items(vec!["A".into(), "B".into()])));
        assert!(!asks_first(&AddWhat::Folder("F".into())));
    }

    #[test]
    fn summaries() {
        let report = |items, definition| add::AddReport { items, definition };
        let none = add::Copied::default();
        let items = AddWhat::Items(vec![]);
        assert_eq!(summary("B", &items, &none, &report(1, false), 1, false), "Added 1 item to B");
        let present = add::Copied { present: vec!["X".into(), "Y".into()], ..Default::default() };
        assert_eq!(summary("B", &items, &present, &report(3, false), 5, false), "Added 3 items to B; 2 items were already there");
        let one = add::Copied { present: vec!["X".into()], ..Default::default() };
        assert_eq!(summary("B", &items, &one, &report(0, false), 1, false), "Added 0 items to B; 1 item was already there");
        assert_eq!(summary("B", &items, &none, &report(2, false), 9, true), "Stopped after adding 2 of 9 items to B");
        let folder = AddWhat::Folder("F".into());
        assert_eq!(summary("B", &folder, &none, &report(4, true), 4, false), "Added the folder and 4 items to B");
        assert_eq!(summary("B", &folder, &none, &report(4, false), 4, false), "Added 4 items to B; it already had the folder");
        assert_eq!(summary("B", &AddWhat::Smart("S".into()), &none, &report(0, true), 0, false), "Added the smart folder to B");
        assert_eq!(summary("B", &AddWhat::TagGroup("G".into()), &none, &report(0, true), 0, false), "Added the tag group to B");
    }
}
