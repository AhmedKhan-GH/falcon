//! Items dragged out of the grid: past the window edge they become a macOS
//! file drag (for Finder and other apps), and while dragging a count
//! follows the pointer.

use std::path::PathBuf;

use eframe::egui::{self, Color32, CornerRadius, Rect, vec2};
use falcon_library::{Library, thumbnail};

use crate::app::{App, DragItems};
use crate::locale::{tr_n, trf};
use crate::theme::{self as t, font};

impl App {
    /// Starts the native drag once grid items leave the window, and draws
    /// the item count beside the pointer.
    pub(crate) fn drag_out(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame, lib: &Library) {
        // Items dragged past the window edge become a macOS file drag, so
        // they can be dropped into Finder or other apps, as from Eagle.
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(ctx)
            && ctx.pointer_latest_pos().is_some_and(|p| !ctx.content_rect().contains(p))
            && !self.native_drag.load(std::sync::atomic::Ordering::Relaxed)
        {
            let files: Vec<PathBuf> = d.0.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect();
            let preview = d.0.first().and_then(|id| lib.item(id)).map(|i| {
                i.thumbnail_path(lib.root()).unwrap_or_else(|| i.original_path(lib.root()))
            });
            egui::DragAndDrop::clear_payload(ctx);
            if let Some(image) = preview.filter(|_| !files.is_empty()).map(|p| drag_image(&p)) {
                let flag = self.native_drag.clone();
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                let done = flag.clone();
                let ctx2 = ctx.clone();
                let started = drag::start_drag(
                    &*frame,
                    drag::DragItem::Files(files),
                    drag::Image::Raw(image),
                    move |_, _| {
                        done.store(false, std::sync::atomic::Ordering::Relaxed);
                        ctx2.request_repaint();
                    },
                    drag::Options::default(),
                );
                if let Err(e) = started {
                    flag.store(false, std::sync::atomic::Ordering::Relaxed);
                    self.flash_error(trf("Could not drag out: {error}", &[("error", &e)]));
                }
            }
        }
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(ctx)
            && let Some(pos) = ctx.pointer_latest_pos()
        {
            let text = tr_n(d.0.len(), "{n} item", "{n} items");
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag-ghost")));
            let g = painter.layout_no_wrap(text, font(12.0), Color32::WHITE);
            let r = Rect::from_min_size(pos + vec2(12.0, 12.0), g.size() + vec2(16.0, 8.0));
            painter.rect_filled(r, CornerRadius::same(10), t::p().primary);
            painter.galley(r.min + vec2(8.0, 4.0), g, Color32::WHITE);
        }
    }
}

/// The picture shown under the pointer while dragging out: a 128 px PNG of
/// `path`, or a plain tile when it can't be decoded. Always valid PNG, since
/// the drag crate panics on an image AppKit can't load.
pub(crate) fn drag_image(path: &std::path::Path) -> Vec<u8> {
    // A picture still in iCloud would hold the drag until it downloads.
    let decoded = if crate::icloud::is_dataless(path) { None } else { thumbnail::decode(path).ok() };
    let img = decoded
        .map(|i| i.thumbnail(128, 128))
        .unwrap_or_else(|| image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(96, 96, image::Rgba([60, 61, 64, 255]))));
    let mut out = std::io::Cursor::new(Vec::new());
    if img.to_rgba8().write_to(&mut out, image::ImageFormat::Png).is_err() {
        return Vec::new();
    }
    out.into_inner()
}

#[cfg(test)]
mod tests {
    use super::drag_image;

    fn is_png(bytes: &[u8]) -> bool {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Png).is_ok()
    }

    #[test]
    fn drag_images_are_always_valid_png() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library");
        if let Ok(lib) = falcon_library::Library::open(&fixture)
            && let Some(thumb) = lib.items().iter().find_map(|i| i.thumbnail_path(lib.root()))
        {
            assert!(is_png(&drag_image(&thumb)), "WebP thumbnail");
        }
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(is_png(&drag_image(&txt)), "undecodable file");
        assert!(is_png(&drag_image(&dir.path().join("missing.png"))), "missing file");
    }
}
