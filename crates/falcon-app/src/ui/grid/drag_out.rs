//! Items dragged out of the grid: inside the window a stacked thumbnail ghost
//! and count follow the pointer, and past the window edge they become a macOS
//! file drag (for Finder and other apps).

#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::path::Path;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;
use image::{DynamicImage, Rgba, RgbaImage};

use crate::app::{App, DragItems};
use crate::loader::{Tier, job_for};
use crate::locale::tr_n;
#[cfg(target_os = "macos")]
use crate::locale::trf;
use crate::theme::{self as t, font};

const GHOST_MAX_SIDE: f32 = 84.0;
const GHOST_PAD: f32 = 4.0;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const NATIVE_MAX_SIDE: u32 = 112;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const NATIVE_PAD: u32 = 6;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const STACK_SHIFT: u32 = 6;

impl App {
    /// Starts the native drag once grid items leave the window (macOS only:
    /// elsewhere items stay inside the window), and draws the dragged preview
    /// and item count beside the pointer.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code, unused_variables))]
    pub(crate) fn drag_out(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame, lib: &Library) {
        #[cfg(target_os = "macos")]
        self.start_native_drag(ctx, frame, lib);
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(ctx)
            && let Some(pos) = ctx.pointer_latest_pos()
        {
            self.draw_drag_ghost(ctx, lib, &d.0, pos);
        }
    }

    fn draw_drag_ghost(&mut self, ctx: &egui::Context, lib: &Library, ids: &[String], pos: Pos2) {
        let count = ids.len();
        let first_item = ids.first().and_then(|id| lib.item(id));
        let tex = first_item.and_then(|item| {
            [Tier::Grid, Tier::Preview, Tier::Full].into_iter().find_map(|tier| self.ws.loader.get(&item.id, tier).cloned())
        });
        if tex.is_none()
            && let Some(item) = first_item
            && let Some(j) = job_for(lib, item, Tier::Grid)
        {
            self.ws.frame_jobs.insert(0, j);
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag-ghost")));
        let thumb_size = match &tex {
            Some(tex) => fit_within(tex.size_vec2(), GHOST_MAX_SIDE),
            None => vec2(64.0, 64.0),
        };
        let card = drag_ghost_rect(pos, thumb_size + Vec2::splat(GHOST_PAD * 2.0));
        if count > 1 {
            let back = card.translate(vec2(5.0, 5.0));
            painter.rect_filled(back, CornerRadius::same(8), t::p().field);
            painter.rect_stroke(back, CornerRadius::same(8), Stroke::new(1.0, t::p().card_border), StrokeKind::Inside);
        }
        painter.rect_filled(card, CornerRadius::same(8), t::p().panel);
        painter.rect_stroke(card, CornerRadius::same(8), Stroke::new(1.0, t::p().card_border), StrokeKind::Inside);
        let inner = card.shrink(GHOST_PAD);
        if let Some(tex) = tex {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(tex.id(), inner, uv, Color32::WHITE);
        } else if let Some(item) = first_item {
            let label = if item.ext.is_empty() { "FILE".to_string() } else { item.ext.to_uppercase() };
            painter.text(inner.center(), egui::Align2::CENTER_CENTER, label, font(12.0), t::p().text_muted);
        }
        let text = tr_n(count, "{n} item", "{n} items");
        let g = painter.layout_no_wrap(text, font(11.5), Color32::WHITE);
        let pill_pos = Pos2::new(card.right() - g.size().x - 10.0, card.bottom() - 10.0);
        let pill = Rect::from_min_size(pill_pos, g.size() + vec2(14.0, 6.0));
        painter.rect_filled(pill, CornerRadius::same(9), t::p().primary);
        painter.galley(pill.min + vec2(7.0, 3.0), g, Color32::WHITE);
    }

    #[cfg(target_os = "macos")]
    fn start_native_drag(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame, lib: &Library) {
        // Items dragged past the window edge (either outside content_rect or
        // after CursorLeft clears latest_pos) become a macOS file drag, so
        // they can be dropped into Finder or other apps, as from Eagle.
        let (down, pos) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
        let active_native = self.native_drag.load(std::sync::atomic::Ordering::Relaxed);
        if let Some(d) = egui::DragAndDrop::payload::<DragItems>(ctx)
            && should_start_native_drag(true, down, pos, ctx.content_rect(), active_native)
        {
            let files: Vec<PathBuf> = d.0.iter().filter_map(|id| lib.resolved_source_path(id).ok()).collect();
            let first = d.0.first().and_then(|id| lib.item(id));
            let image = first.filter(|_| !files.is_empty()).map(|item| {
                let src = lib.resolved_source_path(&item.id).ok();
                drag_image_for_source(item, lib.root(), src.as_deref(), files.len())
            });
            egui::DragAndDrop::clear_payload(ctx);
            if let Some(image) = image {
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
    }
}

fn drag_ghost_rect(pos: Pos2, size: Vec2) -> Rect {
    Rect::from_center_size(pos, size)
}

fn fit_within(size: Vec2, max_side: f32) -> Vec2 {
    let long = size.x.max(size.y);
    if long <= 0.0 {
        return vec2(max_side, max_side);
    }
    let scale = (max_side / long).min(1.0);
    vec2((size.x * scale).round().max(16.0), (size.y * scale).round().max(16.0))
}

/// Whether an active grid item drag has left the window and should become a
/// native OS file drag (`PointerGone` sets `latest_pos` to `None` when the
/// pointer crosses out of the window or onto another app's window).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn should_start_native_drag(has_payload: bool, primary_down: bool, latest_pos: Option<Pos2>, content_rect: Rect, native_drag: bool) -> bool {
    has_payload && primary_down && !native_drag && latest_pos.is_none_or(|p| !content_rect.contains(p))
}

/// The picture shown under the pointer while dragging out: a card with the
/// item's decoded preview (and a stacked backing card for multi-item drags),
/// or a neutral document tile when nothing can be decoded. Always valid PNG,
/// since the drag crate panics on an image AppKit can't load.
#[cfg(test)]
pub(crate) fn drag_image_for(item: &Item, root: &Path, count: usize) -> Vec<u8> {
    let orig = item.managed_storage_path(root);
    drag_image_for_source(item, root, Some(&orig), count)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn drag_image_for_source(item: &Item, root: &Path, source: Option<&Path>, count: usize) -> Vec<u8> {
    let thumb = item.thumbnail_path(root);
    let decoded = thumb
        .as_deref()
        .and_then(|t| (!crate::icloud::is_dataless(t)).then(|| falcon_library::thumbnail::decode_for_display(t).ok()).flatten())
        .or_else(|| source.and_then(|orig| decode_original_preview(orig, &item.ext.to_lowercase())));
    encode_drag_card(decoded.as_ref(), count)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn decode_original_preview(path: &Path, ext: &str) -> Option<DynamicImage> {
    if crate::icloud::is_dataless(path) || !path.is_file() {
        return None;
    }
    if ext == "svg" {
        let data = std::fs::read(path).ok()?;
        return falcon_library::thumbnail::render_svg(&data, Some(NATIVE_MAX_SIDE)).ok();
    }
    if falcon_library::binary_preview::handles(ext)
        && let Some(img) = falcon_library::binary_preview::preview(path, ext)
    {
        return Some(img);
    }
    if falcon_library::zip_preview::handles(ext)
        && let Some(img) = falcon_library::zip_preview::preview(path, ext)
    {
        return Some(img);
    }
    if falcon_library::psd::handles(ext)
        && let Ok(mut c) = falcon_library::psd::Composite::open(path)
    {
        let (w, h) = c.size();
        if let Ok(rgba) = c.read_scaled([0, 0, w, h], NATIVE_MAX_SIDE) {
            return Some(DynamicImage::ImageRgba8(rgba));
        }
    }
    if falcon_library::exif::is_raw(ext)
        && let Ok(data) = std::fs::read(path)
        && let Ok(img) = falcon_library::thumbnail::raw_preview(&data)
    {
        return Some(img);
    }
    if ext == "pdf"
        && let Ok(doc) = falcon_pdf::Document::open(path)
        && let Ok(b) = doc.thumbnail(0, NATIVE_MAX_SIDE as usize)
        && let Some(rgba) = RgbaImage::from_raw(u32::try_from(b.width).ok()?, u32::try_from(b.height).ok()?, b.rgba)
    {
        return Some(DynamicImage::ImageRgba8(rgba));
    }
    if falcon_library::imageio::available(ext)
        && let Ok(img) = falcon_library::imageio::decode(path)
    {
        return Some(img);
    }
    falcon_library::thumbnail::decode_for_display(path).ok()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn encode_drag_card(preview: Option<&DynamicImage>, count: usize) -> Vec<u8> {
    let scaled = preview.map(|i| i.thumbnail(NATIVE_MAX_SIDE, NATIVE_MAX_SIDE).to_rgba8());
    let (pw, ph) = scaled.as_ref().map_or((64, 64), |i| (i.width().max(1), i.height().max(1)));
    let card_w = pw + NATIVE_PAD * 2;
    let card_h = ph + NATIVE_PAD * 2;
    let shift = if count > 1 { STACK_SHIFT } else { 0 };
    let mut canvas = RgbaImage::from_pixel(card_w + shift, card_h + shift, Rgba([0, 0, 0, 0]));
    if count > 1 {
        draw_card_box(&mut canvas, shift, shift, card_w, card_h, Rgba([38, 40, 46, 220]), Rgba([255, 255, 255, 36]));
    }
    draw_card_box(&mut canvas, 0, 0, card_w, card_h, Rgba([52, 54, 60, 240]), Rgba([255, 255, 255, 70]));
    match scaled {
        Some(img) => blend_rgba(&mut canvas, &img, NATIVE_PAD, NATIVE_PAD),
        None => draw_doc_placeholder(&mut canvas, NATIVE_PAD, NATIVE_PAD, pw, ph),
    }
    let mut out = std::io::Cursor::new(Vec::new());
    if canvas.write_to(&mut out, image::ImageFormat::Png).is_err() {
        return Vec::new();
    }
    out.into_inner()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn draw_card_box(dst: &mut RgbaImage, x0: u32, y0: u32, w: u32, h: u32, fill: Rgba<u8>, border: Rgba<u8>) {
    for dy in 0..h {
        for dx in 0..w {
            // 2 px chamfer keeps corners clean without complex SDFs.
            let corner = (dx == 0 || dx + 1 == w) && (dy == 0 || dy + 1 == h);
            if corner {
                continue;
            }
            let edge = dx == 0 || dy == 0 || dx + 1 == w || dy + 1 == h || ((dx == 1 || dx + 2 == w) && (dy == 1 || dy + 2 == h));
            let px = dst.get_pixel_mut(x0 + dx, y0 + dy);
            *px = alpha_over(*px, if edge { border } else { fill });
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn draw_doc_placeholder(dst: &mut RgbaImage, x0: u32, y0: u32, w: u32, h: u32) {
    let page = Rgba([215, 220, 228, 255]);
    let fold = Rgba([165, 172, 182, 255]);
    let margin_x = w / 5;
    let margin_y = h / 6;
    let pw = w.saturating_sub(margin_x * 2);
    let ph = h.saturating_sub(margin_y * 2);
    let fold_sz = (pw / 3).max(6);
    for dy in 0..ph {
        for dx in 0..pw {
            if dx >= pw - fold_sz && dy < fold_sz && dx - (pw - fold_sz) > dy {
                continue;
            }
            let in_fold = dx >= pw - fold_sz && dy < fold_sz;
            dst.put_pixel(x0 + margin_x + dx, y0 + margin_y + dy, if in_fold { fold } else { page });
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn blend_rgba(dst: &mut RgbaImage, src: &RgbaImage, x0: u32, y0: u32) {
    for (y, row) in src.rows().enumerate() {
        for (x, &fg) in row.enumerate() {
            let (dx, dy) = (x0 + x as u32, y0 + y as u32);
            if dx < dst.width() && dy < dst.height() {
                let bg = *dst.get_pixel(dx, dy);
                dst.put_pixel(dx, dy, alpha_over(bg, fg));
            }
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn alpha_over(bg: Rgba<u8>, fg: Rgba<u8>) -> Rgba<u8> {
    let a = u16::from(fg.0[3]);
    if a == 255 {
        return fg;
    }
    if a == 0 {
        return bg;
    }
    let inv = 255 - a;
    let ba = u16::from(bg.0[3]);
    let out_a = a + ((ba * inv + 127) / 255);
    if out_a == 0 {
        return Rgba([0, 0, 0, 0]);
    }
    let mix = |f: u8, b: u8| -> u8 {
        let num = u32::from(f) * u32::from(a) + ((u32::from(b) * u32::from(ba) * u32::from(inv) + 127) / 255);
        ((num + u32::from(out_a) / 2) / u32::from(out_a)).min(255) as u8
    };
    Rgba([mix(fg.0[0], bg.0[0]), mix(fg.0[1], bg.0[1]), mix(fg.0[2], bg.0[2]), out_a.min(255) as u8])
}

#[cfg(test)]
mod tests {
    use super::{decode_original_preview, drag_ghost_rect, drag_image_for, encode_drag_card, should_start_native_drag};
    use eframe::egui::{Pos2, Rect, vec2};
    use falcon_library::model::Item;
    use image::GenericImageView;
    use std::path::Path;

    fn drag_image(path: &Path) -> Vec<u8> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
        let decoded = decode_original_preview(path, &ext);
        encode_drag_card(decoded.as_ref(), 1)
    }

    fn is_png(bytes: &[u8]) -> bool {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Png).is_ok()
    }

    #[test]
    fn native_drag_starts_outside_or_after_cursor_left_only_while_held() {
        let win = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));
        assert!(!should_start_native_drag(true, true, Some(Pos2::new(400.0, 300.0)), win, false));
        assert!(should_start_native_drag(true, true, Some(Pos2::new(-5.0, 300.0)), win, false));
        assert!(should_start_native_drag(true, true, None, win, false), "CursorLeft clears latest_pos");
        assert!(!should_start_native_drag(true, false, None, win, false), "must still hold button");
        assert!(!should_start_native_drag(false, true, None, win, false));
        assert!(!should_start_native_drag(true, true, None, win, true));
        assert_eq!(drag_ghost_rect(Pos2::new(120.0, 80.0), vec2(80.0, 60.0)).center(), Pos2::new(120.0, 80.0));
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

    #[test]
    fn svg_and_affinity_originals_without_thumbnails_decode_for_drag_preview() {
        let dir = tempfile::tempdir().unwrap();
        let item: Item = serde_json::from_value(serde_json::json!({
            "id": "SVG1", "name": "logo", "size": 100, "btime": 0, "mtime": 0, "ext": "svg",
            "tags": [], "folders": [], "isDeleted": false, "url": "", "annotation": "", "modificationTime": 0,
            "height": 80, "width": 40, "lastModified": 0, "palettes": []
        }))
        .unwrap();
        std::fs::create_dir_all(item.info_dir(dir.path())).unwrap();
        std::fs::write(
            item.managed_storage_path(dir.path()),
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="80"><rect width="40" height="80" fill="#22aa66"/></svg>"##,
        )
        .unwrap();

        let single = image::load_from_memory(&drag_image_for(&item, dir.path(), 1)).unwrap();
        let stacked = image::load_from_memory(&drag_image_for(&item, dir.path(), 3)).unwrap();
        assert!(single.height() > single.width(), "SVG aspect ratio preserved in drag card");
        assert!(stacked.width() > single.width(), "multi-item drag adds stacked backing card");
        let center = single.get_pixel(single.width() / 2, single.height() / 2).0;
        assert_eq!(center, [0x22, 0xaa, 0x66, 255]);
    }
}
