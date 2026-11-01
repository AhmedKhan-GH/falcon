//! Page stepping for multi-page TIFFs in the image viewer: the keys, the
//! page control in the bar and the picture of the page shown. Page 1 is
//! drawn as any image; the other pages come from `loader::tiff_page_job`.

use eframe::egui::{self, Event, Key, Pos2, TextureHandle, vec2};
use falcon_library::model::Item;
use falcon_library::Library;

use crate::app::App;
use crate::icons as ic;
use crate::loader::{Tier, tiff_page_job};
use crate::theme::{self as t, mono};
use crate::ui::widgets::icon_button;

/// Whether `ext` is a TIFF.
fn is_tiff(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "tif" | "tiff")
}

impl App {
    /// PageUp and PageDown step the pages of the TIFF in the image viewer
    /// (taken out of the key events, so they don't scroll the grid behind).
    pub(crate) fn tiff_keys(&mut self, ctx: &egui::Context) {
        let Some(id) = self.ws.viewer.as_deref() else { return };
        if self.tiff.id != id || self.tiff.pages().is_none() || self.prefs_window.open || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let mut step = 0;
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { key: k @ (Key::PageUp | Key::PageDown), pressed: true, modifiers, .. } if modifiers.is_none() => {
                    step += if *k == Key::PageDown { 1 } else { -1 };
                    false
                }
                _ => true,
            });
        });
        self.tiff_step(step);
    }

    /// Moves the page by `step`; the view fits the new page.
    fn tiff_step(&mut self, step: isize) {
        if step != 0 && self.tiff.step(step) {
            self.ws.view.zoom = None;
            self.ws.view.pan = vec2(0.0, 0.0);
        }
    }

    /// Follows the viewer to `item`, takes in the page count and, on a page
    /// above the first, asks for its picture. Returns the picture to show
    /// instead of the item's own (the page once loaded, else the previous
    /// one); `tiff.natural` then holds its size, for fitting.
    pub(crate) fn tiff_view(&mut self, ctx: &egui::Context, lib: &Library, item: &Item) -> Option<TextureHandle> {
        if self.tiff.id != item.id {
            if is_tiff(&item.ext) {
                self.tiff.open(&item.id, item.original_path(lib.root()), ctx);
            } else {
                self.tiff = Default::default();
                self.tiff.id = item.id.clone();
            }
        }
        self.tiff.natural = None;
        self.tiff.poll();
        if self.tiff.pages().is_none() || self.tiff.page == 0 {
            self.tiff.shown = None;
            return None;
        }
        let job = tiff_page_job(item, lib.root(), self.tiff.page);
        let tex = self.ws.loader.get(&job.key.0, Tier::Full).cloned();
        if !self.ws.loader.has_failed(&job.key.0, Tier::Full) {
            self.ws.frame_jobs.push(job);
        }
        if tex.is_some() {
            self.tiff.shown = tex.clone();
        }
        let shown = self.tiff.shown.clone()?;
        self.tiff.natural = Some(shown.size_vec2());
        Some(shown)
    }

    /// The page control in the bar, right of `x` going left: previous, the
    /// `2 / 5` label, next. Returns the x the next tool goes at.
    pub(crate) fn tiff_bar(&mut self, ui: &mut egui::Ui, item: &Item, x: f32, cy: f32) -> f32 {
        let Some(n) = self.tiff.pages().filter(|_| self.tiff.id == item.id) else { return x };
        let mut step = 0;
        let page = self.tiff.page;
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_RIGHT, 14.0, page + 1 < n, "Next Page (Page Down)").clicked() {
            step = 1;
        }
        let label = format!("{} / {n}", page + 1);
        let w = 12.0 + 8.0 * label.chars().count() as f32;
        ui.painter().text(Pos2::new(x - 15.0 - w / 2.0, cy), egui::Align2::CENTER_CENTER, label, mono(12.0), t::p().text_value);
        if icon_button(ui, Pos2::new(x - 30.0 - w, cy), ic::CARET_LEFT, 14.0, page > 0, "Previous Page (Page Up)").clicked() {
            step = -1;
        }
        self.tiff_step(step);
        x - 56.0 - w
    }
}
