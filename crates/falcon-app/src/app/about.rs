//! The About window (Falcon ▸ About Falcon).

use eframe::egui::{self, Align, ColorImage, Layout, RichText, TextureHandle, TextureOptions, vec2};

use super::App;
use crate::theme as t;

const ICON: &[u8] = include_bytes!("../../assets/icon-256.png");

/// The app icon as a texture, or `None` if the PNG can't be read.
fn load_icon(ctx: &egui::Context) -> Option<TextureHandle> {
    let img = image::load_from_memory_with_format(ICON, image::ImageFormat::Png).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ctx.load_texture("about-icon", ColorImage::from_rgba_unmultiplied(size, img.as_raw()), TextureOptions::LINEAR))
}

impl App {
    /// Draws the About window while `self.about` is set. Esc and a click
    /// outside close it, like the OK button.
    pub(crate) fn about_ui(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        if self.about_icon.is_none() {
            self.about_icon = load_icon(ctx);
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(8.0);
                if let Some(tex) = &self.about_icon {
                    ui.image((tex.id(), vec2(80.0, 80.0)));
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Falcon").size(20.0).color(t::p().text));
                ui.label(RichText::new("Desktop asset manager for Eagle libraries").color(t::p().text_muted));
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(t::p().text_muted));
                ui.add_space(14.0);
                if ui.button("OK").clicked() {
                    close = true;
                }
                ui.add_space(18.0);
                ui.label(RichText::new("Licensed under MIT or Apache-2.0").size(11.0).color(t::p().text_muted));
            });
        });
        if close || modal.should_close() {
            self.about = false;
        }
    }
}
