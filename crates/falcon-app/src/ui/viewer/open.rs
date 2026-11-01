//! "Open in Default App", "Open With" and "Show in Finder" in the card
//! menu, the viewer bar and the inspector, and the viewer's waiting screen
//! for iCloud originals that aren't on disk yet (see `open_with.rs` and
//! `icloud.rs`).

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Ui, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::{Action, App};
use crate::loader::Tier;
use crate::ui::widgets::{icon, icon_button};
use crate::icloud::FetchState;
use crate::icons as ic;
use crate::open_with;
use crate::shortcuts::{self, Command};
use crate::theme::{self as t, font};

const CARET: &str = ic::CARET_DOWN;

/// The originals of `ids`, in order, skipping ids not in the library.
pub(crate) fn originals(lib: &Library, ids: &[String]) -> Vec<PathBuf> {
    ids.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect()
}

impl App {
    pub(crate) fn shortcut_text(&self, ctx: &egui::Context, cmd: Command) -> String {
        self.shortcuts.binding(cmd).map(|c| shortcuts::display(ctx, &c)).unwrap_or_default()
    }

    /// Menu entries for `files`: Open in Default App (every file), Open
    /// With (one file, as in Eagle) and Show in Finder.
    pub(crate) fn open_menu(&mut self, ui: &mut Ui, files: &[PathBuf]) {
        let def = self.shortcut_text(ui.ctx(), Command::OpenDefault);
        if ui.add(egui::Button::new("Open in Default App").shortcut_text(def)).clicked() {
            self.open_files(files, true);
        }
        if let [file] = files {
            crate::ui::menus::search::submenu(ui, "Open With", |ui| self.open_with_list(ui, file));
        }
        let reveal = self.shortcut_text(ui.ctx(), Command::RevealInFinder);
        if ui.add(egui::Button::new("Show in Finder").shortcut_text(reveal)).clicked() {
            self.open_files(files, false);
        }
    }

    /// Opens (`default`) or reveals `files`; several ask first when there are many.
    fn open_files(&mut self, files: &[PathBuf], default: bool) {
        if let [_] = files {
            if default {
                open_with::open_default(files);
                self.used(files);
            } else {
                open_with::reveal(files);
            }
        } else {
            self.push_bulk(files.len(), Action::OpenFiles(files.to_vec(), default));
        }
    }

    /// ⇧↩ and ⌘↩: open the viewer's item, or the selection, in its default
    /// app (`default`) or show it in Finder.
    /// Shows the selected items' originals in Quick Look (Space, when
    /// Preferences says so).
    pub(crate) fn quick_look(&mut self) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let files = originals(&crate::app::lock(&shared), &self.ws.selected);
        open_with::quick_look(&files);
    }

    pub(crate) fn open_keys(&mut self, default: bool) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let files = {
            let lib = crate::app::lock(&shared);
            match &self.ws.viewer {
                Some(id) => originals(&lib, std::slice::from_ref(id)),
                None => originals(&lib, &self.ws.selected),
            }
        };
        self.open_files(&files, default);
    }

    /// Actions that read originals or thumbnails on this thread (rotate,
    /// flip, crop, refresh) skip items still in iCloud: their download
    /// starts and the status line says so. Returns the ids that can go ahead.
    pub(crate) fn local_only(&mut self, lib: &Library, ids: &[String]) -> Vec<String> {
        let ctx = self.ctx.clone();
        let mut waiting = 0;
        let ok: Vec<String> = ids
            .iter()
            .filter(|id| {
                let Some(item) = lib.item(id) else { return false };
                let files = std::iter::once(item.original_path(lib.root())).chain(item.thumbnail_path(lib.root()));
                let local = files.map(|f| self.fetches.prefetch(&ctx, &f).is_local()).filter(|l| !l).count() == 0;
                waiting += usize::from(!local);
                local
            })
            .cloned()
            .collect();
        if waiting > 0 {
            self.flash(waiting_message(waiting));
        }
        ok
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
                self.used(&[file.to_path_buf()]);
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
            self.used(&[file.to_path_buf()]);
        }
        x - 13.0
    }

    /// The viewer's screen while `item`'s original is still in iCloud: the
    /// preview (if cached) dimmed, a line of state and a bar. Returns true
    /// while it shows, so nothing reads the original on this thread.
    pub(crate) fn icloud_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        let ctx = &parent.ctx().clone();
        let path = item.original_path(lib.root());
        let state = self.fetches.ensure(ctx, &path);
        if state.is_local() {
            return false;
        }
        let preview = self.ws.loader.get(&item.id, Tier::Preview).cloned();
        {
            let ui = &mut crate::ui::widgets::viewer_ui(parent, screen, "icloud-viewer");
            let p = ui.painter().clone();
            p.rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            ui.interact(area, ui.scope_id().with("icloud-bg"), Sense::click());
            if let Some(tex) = preview {
                let size = tex.size_vec2();
                let fit = ((area.width() - 80.0) / size.x).min((area.height() - 80.0) / size.y).clamp(0.01, 1.0);
                let r = Rect::from_center_size(area.center(), size * fit);
                egui::Image::new((tex.id(), r.size())).tint(egui::Color32::from_white_alpha(70)).paint_at(ui, r);
            }
            let c = area.center();
            p.text(c - vec2(0.0, 14.0), Align2::CENTER_CENTER, state.message(), font(14.0), t::p().text);
            let track = Rect::from_center_size(c + vec2(0.0, 12.0), vec2(220.0, 4.0));
            match &state {
                FetchState::Downloading { progress } => {
                    p.rect_filled(track, CornerRadius::same(2), t::p().divider);
                    // Unknown progress: a segment sweeping the track.
                    let (a, b) = match progress {
                        Some(f) => (0.0, f.clamp(0.0, 1.0)),
                        None => {
                            let s = (ctx.input(|i| i.time) * 0.6).fract() as f32;
                            (s * 0.75, s * 0.75 + 0.25)
                        }
                    };
                    let seg = Rect::from_min_max(Pos2::new(track.left() + track.width() * a, track.top()), Pos2::new(track.left() + track.width() * b, track.bottom()));
                    p.rect_filled(seg, CornerRadius::same(2), t::p().primary);
                    ctx.request_repaint_after(std::time::Duration::from_millis(33));
                }
                FetchState::Failed(_) => {
                    let mut b = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(c + vec2(0.0, 22.0), vec2(120.0, 26.0))));
                    if b.button("Try Again").clicked() {
                        self.fetches.retry(&path);
                    }
                }
                FetchState::Local => {}
            }
            crate::ui::viewer::audio::top_bar(self, ui, item, bar);
        }
        true
    }
}

/// The status line when an action skipped items still in iCloud.
pub(crate) fn waiting_message(n: usize) -> String {
    if n == 1 {
        "Downloading 1 item from iCloud; try again once it is local".into()
    } else {
        format!("Downloading {n} items from iCloud; try again once they are local")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_messages() {
        assert_eq!(waiting_message(1), "Downloading 1 item from iCloud; try again once it is local");
        assert_eq!(waiting_message(3), "Downloading 3 items from iCloud; try again once they are local");
    }

    #[test]
    fn the_open_icon_is_in_the_icon_font() {
        use skrifa::MetadataProvider;
        let font = skrifa::FontRef::new(include_bytes!("../../../assets/Phosphor-Light.ttf")).unwrap();
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
