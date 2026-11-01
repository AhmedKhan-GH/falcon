//! Textures for the libraries' `icon.png` files, cached per library path and
//! reloaded when the file changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui;

#[derive(Default)]
pub struct LibraryIcons {
    /// Per library: the icon file's modification time when it was loaded
    /// (None when there was no icon), and the texture.
    cache: HashMap<PathBuf, (Option<SystemTime>, Option<egui::TextureHandle>)>,
}

impl LibraryIcons {
    /// The library's icon, loading it when it is new or has changed.
    pub fn get(&mut self, ctx: &egui::Context, root: &Path) -> Option<egui::TextureHandle> {
        let file = falcon_library::library_icon::icon_path(root);
        let stamp = file.as_ref().and_then(|f| std::fs::metadata(f).ok()).and_then(|m| m.modified().ok());
        if let Some((seen, tex)) = self.cache.get(root)
            && *seen == stamp
        {
            return tex.clone();
        }
        let tex = file.and_then(|f| load(&f)).map(|img| {
            ctx.load_texture(format!("library-icon-{}", root.display()), img, egui::TextureOptions::LINEAR)
        });
        self.cache.insert(root.to_path_buf(), (stamp, tex.clone()));
        tex
    }

    /// Forgets a library's cached icon (after it was changed or removed).
    pub fn forget(&mut self, root: &Path) {
        self.cache.remove(root);
    }
}

fn load(file: &Path) -> Option<egui::ColorImage> {
    let img = image::open(file).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}
