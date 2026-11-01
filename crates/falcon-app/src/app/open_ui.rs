//! "Open in Default App", "Open With" and "Show in Finder" in the card
//! menu, the viewer bar and the inspector (see `open_with.rs`).

use std::path::{Path, PathBuf};

use eframe::egui::{self, Pos2, Rect, Sense, Ui, vec2};
use falcon_library::Library;

use super::{App, icon, icon_button};
use crate::icons as ic;
use crate::open_with;
use crate::shortcuts::{self, Command};
use crate::theme as t;

const CARET: &str = ic::CARET_DOWN;

/// The originals of `ids`, in order, skipping ids not in the library.
pub(super) fn originals(lib: &Library, ids: &[String]) -> Vec<PathBuf> {
    ids.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect()
}

impl App {
    fn shortcut_text(&self, ctx: &egui::Context, cmd: Command) -> String {
        self.shortcuts.binding(cmd).map(|c| shortcuts::display(ctx, &c)).unwrap_or_default()
    }

    /// Menu entries for `files`: Open in Default App (every file), Open
    /// With (one file, as in Eagle) and Show in Finder.
    pub(super) fn open_menu(&mut self, ui: &mut Ui, files: &[PathBuf]) {
        let def = self.shortcut_text(ui.ctx(), Command::OpenDefault);
        if ui.add(egui::Button::new("Open in Default App").shortcut_text(def)).clicked() {
            open_with::open_default(files);
        }
        if let [file] = files {
            ui.menu_button("Open With", |ui| self.open_with_list(ui, file));
        }
        let reveal = self.shortcut_text(ui.ctx(), Command::RevealInFinder);
        if ui.add(egui::Button::new("Show in Finder").shortcut_text(reveal)).clicked() {
            open_with::reveal(files);
        }
    }

    /// ⇧↩ and ⌘↩: open the viewer's item, or the selection, in its default
    /// app (`default`) or show it in Finder.
    pub(super) fn open_keys(&mut self, default: bool) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let files = {
            let lib = super::lock(&shared);
            match &self.ws.viewer {
                Some(id) => originals(&lib, std::slice::from_ref(id)),
                None => originals(&lib, &self.ws.selected),
            }
        };
        if default {
            open_with::open_default(&files);
        } else {
            open_with::reveal(&files);
        }
    }

    /// The apps that open `file`, the default first.
    fn open_with_list(&mut self, ui: &mut Ui, file: &Path) {
        ui.set_min_width(200.0);
        let apps = self.open_apps.get(file).to_vec();
        if apps.is_empty() {
            ui.label(egui::RichText::new("No apps found").color(t::p().text_muted));
        }
        for e in &apps {
            if ui.button(e.label()).on_hover_text(e.app.path.display().to_string()).clicked() {
                open_with::open_in(&e.app.path, &[file.to_path_buf()]);
            }
            if e.default && apps.len() > 1 {
                ui.separator();
            }
        }
    }

    /// The viewer bar's open button, the one hook every viewer calls
    /// (image, audio, video, font, PDF): the icon opens `file` in its
    /// default app and the caret beside it lists Open With. `right` is the
    /// button's right edge; returns the left edge of what it drew.
    pub(crate) fn viewer_open_button(&mut self, ui: &mut Ui, right: f32, cy: f32, file: &Path) -> f32 {
        let def = self.shortcut_text(ui.ctx(), Command::OpenDefault);
        let caret_x = right - 9.0;
        let caret = ui.interact(Rect::from_center_size(Pos2::new(caret_x, cy), vec2(16.0, 26.0)), ui.scope_id().with("viewer-open-with"), Sense::click());
        let color = if caret.hovered() { t::p().text } else { t::p().text_value };
        icon(ui, Pos2::new(caret_x, cy), CARET, 11.0, color);
        let caret = caret.on_hover_text("Open With");
        egui::Popup::menu(&caret).show(|ui| self.open_with_list(ui, file));
        let x = caret_x - 20.0;
        let tip = if def.is_empty() { "Open in Default App".to_string() } else { format!("Open in Default App ({def})") };
        if icon_button(ui, Pos2::new(x, cy), ic::ARROW_SQUARE_OUT, 16.0, true, &tip).clicked() {
            open_with::open_default(&[file.to_path_buf()]);
        }
        x - 13.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_icon_is_in_the_icon_font() {
        use skrifa::MetadataProvider;
        let font = skrifa::FontRef::new(include_bytes!("../../assets/Phosphor-Light.ttf")).unwrap();
        let ch = ic::ARROW_SQUARE_OUT.chars().next().unwrap();
        let gid = font.charmap().map(ch).expect("glyph");
        assert_ne!(gid, skrifa::GlyphId::NOTDEF);
    }

    #[test]
    fn originals_follow_the_ids() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
        let Ok(lib) = Library::open(&fixture) else { return };
        let ids: Vec<String> = lib.items().iter().take(2).map(|i| i.id.clone()).chain(["missing".to_string()]).collect();
        let files = originals(&lib, &ids);
        assert_eq!(files.len(), ids.len() - 1);
        assert!(files.iter().all(|f| f.starts_with(lib.root())));
    }
}
