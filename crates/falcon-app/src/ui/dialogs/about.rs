//! The About window (Falcon ▸ About Falcon).

use eframe::egui::{self, Align, ColorImage, Layout, RichText, TextureHandle, TextureOptions, vec2};

use crate::app::App;
use crate::locale::{tr, trf};
use crate::theme as t;

/// The About window: whether it is open, and the icon once loaded.
#[derive(Default)]
pub(crate) struct AboutWindow {
    pub(crate) open: bool,
    pub(crate) icon: Option<TextureHandle>,
}

const ICON: &[u8] = include_bytes!("../../../assets/icon-256.png");

/// The app icon as a texture, or `None` if the PNG can't be read.
fn load_icon(ctx: &egui::Context) -> Option<TextureHandle> {
    let img = image::load_from_memory_with_format(ICON, image::ImageFormat::Png).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ctx.load_texture("about-icon", ColorImage::from_rgba_unmultiplied(size, img.as_raw()), TextureOptions::LINEAR))
}

impl App {
    /// Draws the About window while `self.about.open` is set. Esc and a click
    /// outside close it, like the OK button.
    pub(crate) fn about_ui(&mut self, ctx: &egui::Context) {
        if !self.about.open {
            return;
        }
        if self.about.icon.is_none() {
            self.about.icon = load_icon(ctx);
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(8.0);
                if let Some(tex) = &self.about.icon {
                    ui.image((tex.id(), vec2(80.0, 80.0)));
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Falcon").size(20.0).color(t::p().text));
                ui.label(RichText::new(tr("Desktop asset manager for Eagle libraries")).color(t::p().text_muted));
                ui.label(RichText::new(trf("Version {version}", &[("version", &env!("CARGO_PKG_VERSION"))])).color(t::p().text_muted));
                ui.add_space(14.0);
                if ui.button(tr("OK")).clicked() {
                    close = true;
                }
                ui.add_space(18.0);
                ui.label(RichText::new(tr("Licensed under MIT or Apache-2.0")).size(11.0).color(t::p().text_muted));
            });
        });
        if close || modal.should_close() {
            self.about.open = false;
        }
    }
}
