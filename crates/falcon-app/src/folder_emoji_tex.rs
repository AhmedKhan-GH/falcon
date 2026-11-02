//! Colour emoji textures for folder icons.
//!
//! egui draws no colour emoji and falcon's font stack has none, so the
//! picture of each curated emoji is read from macOS's emoji font
//! (`Apple Color Emoji.ttc`, an sbix bitmap font of about 180 MB). The file
//! is read once, on a worker thread, the first time an emoji is needed; the
//! curated list is decoded to images, the font bytes are dropped and the
//! images come back through a channel. Until they arrive (or if the font is
//! missing) callers draw the folder's normal icon.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use skrifa::{
    FontRef, MetadataProvider,
    bitmap::{BitmapData, BitmapStrikes},
    instance::Size,
};

use crate::folder_emoji::{CURATED, FolderEmoji};

const FONT: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";

/// The strike read from the font, in pixels per em. The sidebar draws the
/// pictures at 16 points, so this is the nearest strike that is not
/// much larger (no mipmaps are made).
const STRIKE: f32 = 40.0;

/// The PNG of `c` in the font's strike nearest `STRIKE` or above.
pub fn png_for<'a>(font: &FontRef<'a>, c: char) -> Option<&'a [u8]> {
    let gid = font.charmap().map(c)?;
    match BitmapStrikes::new(font).glyph_for_size(Size::new(STRIKE), gid)?.data {
        BitmapData::Png(png) => Some(png),
        _ => None,
    }
}

/// Decodes `c` from the font bytes.
pub fn image_for(bytes: &[u8], c: char) -> Option<ColorImage> {
    let font = FontRef::from_index(bytes, 0).ok()?;
    let png = png_for(&font, c)?;
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

/// Reads the font and decodes the curated list. The font bytes are gone
/// when this returns.
fn load_all() -> Vec<(char, ColorImage)> {
    let Ok(bytes) = std::fs::read(FONT) else { return Vec::new() };
    CURATED.iter().filter_map(|(c, _)| image_for(&bytes, *c).map(|i| (*c, i))).collect()
}

enum Load {
    NotStarted,
    Loading(Receiver<Vec<(char, ColorImage)>>),
    Done,
}

/// The emoji of folders and smart folders, and their textures.
pub struct FolderEmojis {
    /// Which folder has which emoji (saved in app data).
    pub store: FolderEmoji,
    load: Load,
    textures: HashMap<char, TextureHandle>,
}

impl Default for FolderEmojis {
    fn default() -> Self {
        Self { store: FolderEmoji::default(), load: Load::NotStarted, textures: HashMap::new() }
    }
}

impl FolderEmojis {
    /// Starts reading the font if that has not happened, and takes in what
    /// the worker has finished. Cheap once loaded.
    pub fn poll(&mut self, ctx: &egui::Context) {
        match &self.load {
            Load::NotStarted => {
                let (tx, rx) = channel();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(load_all());
                    ctx.request_repaint();
                });
                self.load = Load::Loading(rx);
            }
            Load::Loading(rx) => match rx.try_recv() {
                Ok(images) => {
                    for (c, img) in images {
                        let tex = ctx.load_texture(format!("folder-emoji-{}", c as u32), img, TextureOptions::LINEAR);
                        self.textures.insert(c, tex);
                    }
                    self.load = Load::Done;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.load = Load::Done,
            },
            Load::Done => {}
        }
    }

    /// The texture of a curated emoji, once loaded. Asking starts loading.
    pub fn texture(&mut self, ctx: &egui::Context, c: char) -> Option<TextureHandle> {
        self.poll(ctx);
        self.textures.get(&c).cloned()
    }

    /// The texture of the folder's emoji, if it has one that is loaded.
    /// Folders without one never start the font loading.
    pub fn for_folder(&mut self, ctx: &egui::Context, id: &str) -> Option<TextureHandle> {
        let c = self.store.get(id)?;
        self.texture(ctx, c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curated_emoji_is_one_codepoint_with_a_png_strike() {
        for (c, name) in CURATED {
            assert_eq!(c.to_string().chars().count(), 1, "{name}");
        }
        let Ok(bytes) = std::fs::read(FONT) else {
            eprintln!("skipped: {FONT} not found");
            return;
        };
        let font = FontRef::from_index(&bytes, 0).unwrap();
        for (c, name) in CURATED {
            let png = png_for(&font, *c).unwrap_or_else(|| panic!("{name} ({c}) has no PNG strike"));
            assert!(png.starts_with(b"\x89PNG"), "{name}");
            let img = image_for(&bytes, *c).unwrap_or_else(|| panic!("{name} does not decode"));
            assert!(img.size[0] >= 16 && img.size[1] >= 16, "{name}");
        }
    }

    #[test]
    fn a_missing_glyph_is_none() {
        let Ok(bytes) = std::fs::read(FONT) else { return };
        let font = FontRef::from_index(&bytes, 0).unwrap();
        assert!(png_for(&font, '\u{10FFFF}').is_none());
    }
}
