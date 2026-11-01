//! The tag context menu.

use eframe::egui::{self, RichText, Ui};
use falcon_library::Library;

use crate::app::{Action, App, Confirm};
use crate::locale::{tr, tr_n};
use crate::logic::{Scope};
use crate::theme::{self as t};

impl App {
    /// The menu of picked tags on the All Tags page, after Eagle's.
    pub(crate) fn tags_menu(&mut self, ui: &mut Ui, lib: &Library, tags: &[String]) {
        ui.set_min_width(200.0);
        let n = tags.len();
        if ui.button(if n == 1 { tr("Show Items").to_string() } else { tr_n(n, "Show Items with Any of {n} Tag", "Show Items with Any of {n} Tags") }).clicked() {
            match tags {
                [one] => self.go(Scope::Tag(one.clone())),
                _ => self.filter_any_tags(tags.to_vec()),
            }
        }
        ui.separator();
        let all_starred = tags.iter().all(|t| lib.tags.starred_tags.contains(t));
        if ui.button(if all_starred { tr("Unstar") } else { tr("Star") }).clicked() {
            self.ws.actions.push(Action::StarTags(tags.to_vec(), !all_starred));
        }
        if let [one] = tags
            && ui.button(tr("Rename…")).clicked()
        {
            self.ws.tag_rename = Some((one.clone(), one.clone()));
        }
        if n > 1 {
            let merge = ui.add_enabled(crate::tags::can_merge(n), egui::Button::new(tr_n(n, "Merge {n} Tag…", "Merge {n} Tags…")));
            if merge.on_disabled_hover_text(tr("Eagle merges two to five tags at a time")).clicked() {
                self.ws.org.merge = Some((tags.to_vec(), tags[0].clone()));
            }
        }
        ui.separator();
        self.tag_group_menu(ui, lib, tags);
        ui.separator();
        let label = if n == 1 { tr("Delete Tag").to_string() } else { tr_n(n, "Delete {n} Tag", "Delete {n} Tags") };
        if ui.button(RichText::new(label).color(t::p().icon_red)).clicked() {
            self.ws.confirm = Some(Confirm::DeleteTags(tags.to_vec()));
        }
    }
}
