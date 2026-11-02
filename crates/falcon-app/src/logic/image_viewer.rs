//! The image viewer: zoom and the zoom slider, resolution tiers and tiling, fitting,
//! the mouse wheel, panning, grayscale preview and the navigator.


/// A zoom step in the image viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewZoom {
    In,
    Out,
    Actual,
    Fit,
    /// Toggle "fit to window edge" (Eagle's backtick key).
    Edge,
}

/// The viewer's zoom after `cmd` from `scale` (`None`: fit the window).
pub fn view_zoom(cmd: ViewZoom, scale: f32) -> Option<f32> {
    match cmd {
        ViewZoom::In => Some((scale * 1.25).min(ZOOM_MAX)),
        ViewZoom::Out => Some((scale / 1.25).max(ZOOM_MIN)),
        ViewZoom::Actual => Some(1.0),
        ViewZoom::Fit | ViewZoom::Edge => None,
    }
}

/// The image viewer's zoom range, Eagle's 5% to 800% (`zoom_MIN`, `zoom_MAX`).
pub const ZOOM_MIN: f32 = 0.05;

pub const ZOOM_MAX: f32 = 8.0;

/// The zoom from which the "pixelated" preference draws sharp pixels:
/// Eagle adds its pixelated rendering only from 200% of the image's size.
pub const PIXELATED_FROM: f32 = 2.0;

/// Whether the viewer samples nearest at `zoom` (points per image pixel)
/// with the preference `on`.
pub fn pixelated_at(on: bool, zoom: f32) -> bool {
    on && zoom >= PIXELATED_FROM - 1e-4
}

/// The zoom menu's sizes, in percent (Eagle's `openRatioContextMenu`).
pub const ZOOM_PRESETS: [u32; 11] = [5, 10, 25, 50, 100, 125, 150, 200, 300, 400, 800];

/// The zoom slider's range. Its positions run 5 to 200; above 100 each
/// step is 7%, so the right end is 800% (Eagle's `getRatioExp`).
pub const SLIDER_RANGE: (f32, f32) = (5.0, 200.0);

/// Zoom (1.0 = 100%) at slider position `v`.
pub fn slider_zoom(v: f32) -> f32 {
    let v = v.clamp(SLIDER_RANGE.0, SLIDER_RANGE.1);
    (if v > 100.0 { 100.0 + (v - 100.0) * 7.0 } else { v }) / 100.0
}

/// Slider position of `zoom`.
pub fn zoom_slider_pos(zoom: f32) -> f32 {
    let pct = zoom * 100.0;
    (if pct > 100.0 { (pct - 100.0) / 7.0 + 100.0 } else { pct }).clamp(SLIDER_RANGE.0, SLIDER_RANGE.1)
}

/// The zoom at slider position `v`, snapped to 25, 50, 100, 125, 150, 175
/// or 200% when within 3% of one, as Eagle's slider does.
pub fn slider_zoom_snapped(v: f32) -> f32 {
    let pct = slider_zoom(v) * 100.0;
    [25.0, 50.0, 100.0, 125.0, 150.0, 175.0, 200.0].into_iter().find(|s: &f32| (pct - s).abs() <= 3.0).unwrap_or(pct) / 100.0
}

/// "Fit to window edge" (Eagle's `zoomFitEdge`): the whole image fills the
/// view, enlarged if it is smaller.
pub fn edge_fit(natural: (f32, f32), view: (f32, f32)) -> f32 {
    (view.0 / natural.0.max(1.0)).min(view.1 / natural.1.max(1.0)).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Long side of the loader's full tier, the render the viewer shows first.
pub const FULL_TIER_SIDE: u32 = 3072;

/// Whether the viewer decodes `ext` at its full resolution when zoomed in
/// past the full tier. Eagle's bitmap viewer decodes jpg, png, webp, bmp,
/// avif, jxl, tiff and heic originals in full (`BitmapViewer.isSupportFormat`);
/// falcon decodes the ones its image decoder reads (TIFF included, falling
/// back to the QuickLook render when a TIFF's compression isn't supported),
/// and AVIF, HEIC/HEIF and JPEG XL through ImageIO on macOS (keeping the
/// QuickLook render where the system can't read them, as JPEG XL before
/// macOS 14).
/// GIFs belong to the animation player; SVGs are re-rendered instead.
/// RAW photos show their embedded JPEG preview at its full size, as
/// Eagle's RAW viewer does. TGA is decoded by falcon; HDR and EXR are tone
/// mapped at the viewer's exposure. PSD and PSB show their merged image, in
/// tiles up to [`COMPOSITE_TILED_PIXELS`] ([`composite_tiled`]).
pub fn full_resolution(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "jfif" | "jpe" | "png" | "webp" | "bmp" | "tif" | "tiff" | "tga" | "hdr" | "exr")
        || falcon_library::exif::is_raw(ext)
        || falcon_library::psd::handles(ext)
        || (cfg!(target_os = "macos") && falcon_library::imageio::handles(ext))
}

/// Whether the original is worth decoding: the image is larger than the
/// full tier and is drawn (in physical pixels, `shown_px` on its long side)
/// larger than the full tier's render, which would otherwise be magnified.
/// Decoding only then keeps flipping through large photos cheap.
pub fn wants_original(ext: &str, natural_long: u32, shown_px: f32) -> bool {
    full_resolution(ext) && natural_long > FULL_TIER_SIDE && shown_px > FULL_TIER_SIDE as f32
}

/// Formats Eagle draws again natively at the size shown when zoomed in
/// (its native viewer over the thumbnail); falcon asks QuickLook for the
/// page again at the zoom shown once the view holds still and keeps the
/// part on screen ([`quicklook_region`]).
pub fn hires_quicklook(ext: &str) -> bool {
    matches!(ext, "psd" | "psdt" | "psb" | "ai" | "ppt" | "pptx" | "potx")
}

/// Most pixels of a PSD's merged image held in tiles: 1 GiB in memory while
/// they are cut and again as textures. A larger one is decoded only for the
/// part on screen ([`composite_region`]).
pub const COMPOSITE_TILED_PIXELS: u64 = 16384 * 16384;

/// Whether a `w` x `h` merged image is drawn in tiles.
pub fn composite_tiled(w: u32, h: u32) -> bool {
    u64::from(w) * u64::from(h) <= COMPOSITE_TILED_PIXELS
}

/// The part on screen of a merged image too large for tiles, as
/// [`quicklook_region`] gives it: the visible part as fractions and the long
/// side to scale the whole picture to, rounded up to a step. Only the part
/// is decoded, so the size shown and the picture's own `long` side are the
/// only limits.
pub fn composite_region(image: [f32; 4], view: [f32; 4], ppp: f32, long: u32) -> Option<([f32; 4], u32)> {
    let (rect, _) = svg_region(image, view, ppp, u32::MAX)?;
    let shown = (image[2] - image[0]).max(image[3] - image[1]) * ppp;
    let side = ((shown / QUICKLOOK_STEP).ceil() * QUICKLOOK_STEP).min(long as f32);
    Some((rect, side.max(1.0) as u32))
}

/// Most pixels a zoomed QuickLook render of the whole page may have (a
/// 256 MB picture, held on a worker while the part on screen is cut out).
pub const QUICKLOOK_RENDER_PIXELS: f32 = 8192.0 * 8192.0;

/// The longest side asked of QuickLook.
pub const QUICKLOOK_MAX_SIDE: u32 = 16384;

/// Renders are asked in steps of this many pixels on the long side, so
/// small zoom changes reuse the last render.
const QUICKLOOK_STEP: f32 = 512.0;

/// The QuickLook render for a zoomed special format: `image` is where the
/// whole page is on screen and `view` the viewer's rect (both `[x0, y0,
/// x1, y1]` in points), `ppp` pixels per point. Gives the visible part as
/// fractions of the page and the long side to render the whole page at:
/// the size shown, rounded up to a step, within
/// [`QUICKLOOK_RENDER_PIXELS`], [`QUICKLOOK_MAX_SIDE`] and, for a raster
/// format, its own size (`raster_long`), past which a render adds nothing.
/// `None` when none of it is visible.
pub fn quicklook_region(image: [f32; 4], view: [f32; 4], ppp: f32, raster_long: Option<u32>) -> Option<([f32; 4], u32)> {
    let (rect, _) = svg_region(image, view, ppp, u32::MAX)?;
    let (iw, ih) = (image[2] - image[0], image[3] - image[1]);
    let long = (iw.max(ih) * ppp / QUICKLOOK_STEP).ceil() * QUICKLOOK_STEP;
    let aspect = (iw.min(ih) / iw.max(ih)).max(1e-3);
    let budget = (QUICKLOOK_RENDER_PIXELS / aspect).sqrt();
    let side = long.min(budget).min(QUICKLOOK_MAX_SIDE as f32).min(raster_long.map_or(f32::MAX, |n| n as f32));
    Some((rect, side.max(1.0) as u32))
}

/// The pixels of `rect` (fractions `[x0, y0, x1, y1]`) in a `w` × `h`
/// picture, as `(x, y, width, height)`: at least one pixel, inside it.
pub fn crop_span(rect: [f32; 4], w: u32, h: u32) -> (u32, u32, u32, u32) {
    let at = |f: f32, len: u32| ((f.clamp(0.0, 1.0) * len as f32).round() as u32).min(len.saturating_sub(1));
    let (x0, y0) = (at(rect[0], w), at(rect[1], h));
    let end = |f: f32, len: u32, start: u32| ((f.clamp(0.0, 1.0) * len as f32).round() as u32).clamp(start + 1, len.max(start + 1));
    let (x1, y1) = (end(rect[2], w, x0), end(rect[3], h, y0));
    (x0, y0, x1 - x0, y1 - y0)
}

/// Raster formats among [`hires_quicklook`]: a render past their own size
/// only magnifies.
pub fn quicklook_raster(ext: &str) -> bool {
    falcon_library::psd::handles(ext)
}

/// The formats Eagle shows as a picture of their own (its special-format
/// viewer list): for these a custom thumbnail doesn't replace the render.
pub fn special_picture_format(ext: &str) -> bool {
    matches!(
        ext,
        "hdr" | "exr" | "ico" | "graffle" | "xmind" | "mindnode" | "psd" | "psdt" | "indd" | "idml" | "indt" | "psb" | "xd" | "c4d" | "blend"
            | "clip" | "skp" | "dwg" | "dds" | "prd" | "pxd" | "sketch" | "fig" | "afdesign" | "af" | "afpub" | "afphoto" | "ai" | "sketchraw"
            | "icns" | "skt" | "eps" | "key" | "potx" | "pptx" | "ppt" | "docx" | "doc" | "pages" | "xlsx" | "xls" | "eddx" | "emmx" | "numbers"
            | "cdr"
    )
}

/// Whether the viewer shows the item's custom thumbnail as its picture:
/// as in Eagle, when it has one and the format has no viewer or render of
/// its own (not an image falcon decodes, a RAW, TGA, or a special format).
pub fn custom_thumbnail_is_picture(ext: &str, has_custom: bool) -> bool {
    has_custom
        && !falcon_library::thumbnail::is_supported(ext)
        && !full_resolution(ext)
        && !special_picture_format(ext)
}

/// Side of the textures a full-resolution image is cut into. egui can't
/// hold a texture larger than the GPU's limit (`max_texture_side`, 8192 or
/// 16384 on Macs), and a smaller tile also keeps each upload short enough
/// to spread over frames.
pub fn tile_side(max_texture_side: usize) -> usize {
    max_texture_side.clamp(64, 2048)
}

/// One tile of a full-resolution image, in image pixels. `core` is the part
/// the tile draws; `padded` adds a pixel on every inner edge, so linear
/// filtering at the seams blends with the neighbour instead of clamping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileSpan {
    pub core: [usize; 4],
    pub padded: [usize; 4],
}

/// What the mouse wheel does over a picture in the viewer (Eagle's
/// `habits.scrollBehavior`; scroll is its default).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WheelMode {
    /// Pan the picture; with ⌘, ⌃ or ⌥ the wheel zooms.
    #[default]
    Scroll,
    /// Zoom around the pointer.
    Zoom,
    /// Go to the previous or next item.
    Paging,
}

/// What one frame of wheel input does in the image viewer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WheelAction {
    None,
    /// Move the picture by this many points.
    Pan(f32, f32),
    /// Multiply the zoom by this factor, around the pointer.
    Zoom(f32),
    /// Wheel moved in paging mode: -1 previous, 1 next (stepped once the
    /// wheel has rested, see [`PAGE_REST`]).
    Page(isize),
}

/// How long the wheel must rest before paging steps, as Eagle's 50 ms
/// debounce: one gesture, momentum included, moves one item.
pub const PAGE_REST: f32 = 0.05;

/// Turns the frame's scroll (`scroll`, egui's sign: up and left are
/// positive), pinch factor and the zoom modifier (⌥; egui already turns
/// ⌘ and ⌃ scrolling into a pinch) into what the viewer does.
pub fn wheel_action(mode: WheelMode, scroll: (f32, f32), pinch: f32, zoom_modifier: bool) -> WheelAction {
    if pinch != 1.0 {
        return WheelAction::Zoom(pinch);
    }
    let (dx, dy) = scroll;
    if dx == 0.0 && dy == 0.0 {
        return WheelAction::None;
    }
    let zoom = WheelAction::Zoom(((if dy != 0.0 { dy } else { dx }) / 300.0).exp());
    match mode {
        _ if zoom_modifier => zoom,
        WheelMode::Zoom => zoom,
        WheelMode::Scroll => WheelAction::Pan(dx, dy),
        WheelMode::Paging if dy != 0.0 => WheelAction::Page(if dy < 0.0 { 1 } else { -1 }),
        WheelMode::Paging => WheelAction::Page(if dx < 0.0 { 1 } else { -1 }),
    }
}

/// Keeps a scrolled picture in view: on each axis, a picture larger than
/// the view can't leave a gap at either edge, and a smaller one stays
/// inside the view. `pan` is the offset of the picture's centre from the
/// view's centre.
pub fn clamp_pan(pan: (f32, f32), shown: (f32, f32), view: (f32, f32)) -> (f32, f32) {
    let axis = |p: f32, s: f32, v: f32| {
        let limit = ((s - v) / 2.0).abs();
        p.clamp(-limit, limit)
    };
    (axis(pan.0, shown.0, view.0), axis(pan.1, shown.1, view.1))
}

/// Whether an image is "long" (a tall strip such as a web page capture)
/// by Eagle's `smartZoom` rule: at least 960 px wide and over 1.8 times as
/// tall as wide, or at least 320 px wide and over 2.7 times as tall. Eagle
/// has no such rule for wide images.
pub fn is_long_image(w: f32, h: f32) -> bool {
    (w >= 960.0 && w * 1.8 < h) || (w >= 320.0 && w * 2.7 < h)
}

/// The scale Eagle gives an image named `name` (without extension) for
/// its pixel density: `@2x` shows at 50%, `@3x` at 33%, `@1.5x` at 67%,
/// `@0.5x` at 200%.
pub fn density_scale(name: &str) -> Option<f32> {
    [("@2x", 0.5), ("@3x", 1.0 / 3.0), ("@1.5x", 2.0 / 3.0), ("@0.5x", 2.0)].into_iter().find(|(s, _)| name.ends_with(s)).map(|(_, v)| v)
}

/// How an image opens ("Smart Zoom"): the scale, and the vertical offset of
/// its centre from the view's centre. It fits inside `view` less `margin`
/// on each side, never enlarged; a long image fills the width instead and
/// opens at its top, for scrolling down; an `@2x`-style name opens at its
/// density's scale when that fits.
pub fn smart_fit(natural: (f32, f32), view: (f32, f32), margin: f32, name: &str) -> (f32, f32) {
    let (w, h) = (natural.0.max(1.0), natural.1.max(1.0));
    let (vw, vh) = ((view.0 - 2.0 * margin).max(1.0), (view.1 - 2.0 * margin).max(1.0));
    let fit = (vw / w).min(vh / h);
    let mut scale = fit.min(1.0);
    if is_long_image(w, h) {
        scale = (view.0 / w).min(1.0);
    }
    if let Some(d) = density_scale(name) {
        scale = if w * d <= view.0 && h * d <= view.1 { d } else { fit };
    }
    let top = ((h * scale - view.1) / 2.0).max(0.0);
    (scale, top)
}

/// How long the view must hold still before a zoomed-in SVG is rendered
/// again at the new size.
pub const SVG_SETTLE: f32 = 0.15;

/// The same for a zoomed QuickLook format, whose render starts a process.
pub const QUICKLOOK_SETTLE: f32 = 0.4;

/// The part of an SVG to render sharp: `image` is where the whole drawing
/// is on screen and `view` the viewer's rect (both `[x0, y0, x1, y1]` in
/// points). Gives the visible part as fractions of the drawing and its
/// size in pixels (`ppp` pixels per point), at most `max_side` on its long
/// side; `None` when none of it is visible.
pub fn svg_region(image: [f32; 4], view: [f32; 4], ppp: f32, max_side: u32) -> Option<([f32; 4], [u32; 2])> {
    let (iw, ih) = (image[2] - image[0], image[3] - image[1]);
    let vis = [image[0].max(view[0]), image[1].max(view[1]), image[2].min(view[2]), image[3].min(view[3])];
    if iw <= 0.0 || ih <= 0.0 || vis[2] <= vis[0] || vis[3] <= vis[1] {
        return None;
    }
    let frac = |v: f32, o: f32, len: f32| ((v - o) / len).clamp(0.0, 1.0);
    let rect = [frac(vis[0], image[0], iw), frac(vis[1], image[1], ih), frac(vis[2], image[0], iw), frac(vis[3], image[1], ih)];
    let (w, h) = ((vis[2] - vis[0]) * ppp, (vis[3] - vis[1]) * ppp);
    let shrink = (max_side as f32 / w.max(h)).min(1.0);
    Some((rect, [((w * shrink).round() as u32).max(1), ((h * shrink).round() as u32).max(1)]))
}

/// Pixel count above which Eagle shows its "too large" page instead of a
/// TIFF, HEIC or TGA (`MAX_DIMENSION` in `index.html`).
pub const TOO_LARGE_PIXELS: u64 = 120_000_000;

/// Whether the viewer shows the "too large to preview" page for an item:
/// a TIFF, HEIC/HEIF or TGA over [`TOO_LARGE_PIXELS`], unless it has a
/// custom thumbnail, as in Eagle. Other formats show at any size.
pub fn too_large_to_preview(ext: &str, w: u64, h: u64, custom_thumbnail: bool) -> bool {
    matches!(ext, "tif" | "tiff" | "heic" | "heif" | "hif" | "tga") && w.saturating_mul(h) > TOO_LARGE_PIXELS && !custom_thumbnail
}

/// Cuts a `w` x `h` image into tiles whose padded size is at most `side`.
pub fn tile_spans(w: usize, h: usize, side: usize) -> Vec<TileSpan> {
    let step = side.saturating_sub(2).max(1);
    let mut out = Vec::new();
    let mut y = 0;
    while y < h {
        let y1 = (y + step).min(h);
        let mut x = 0;
        while x < w {
            let x1 = (x + step).min(w);
            out.push(TileSpan { core: [x, y, x1, y1], padded: [x.saturating_sub(1), y.saturating_sub(1), (x1 + 1).min(w), (y1 + 1).min(h)] });
            x = x1;
        }
        y = y1;
    }
    out
}

/// One pixel as CSS's `filter: grayscale(1)` draws it (the filter Eagle's
/// grayscale preview puts on the grid and the viewer): every channel
/// becomes the luminance 0.2126 R + 0.7152 G + 0.0722 B; alpha stays. Works
/// on premultiplied pixels too, since the weights sum to one.
pub fn grayscale_pixel([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let y = ((u32::from(r) * 2126 + u32::from(g) * 7152 + u32::from(b) * 722 + 5000) / 10_000) as u8;
    [y, y, y, a]
}

/// One colour through [`grayscale_pixel`]: what the grayscale preview does
/// to the colours a viewer paints itself (the font specimen's themes).
pub fn gray_color(c: eframe::egui::Color32) -> eframe::egui::Color32 {
    let [r, g, b, a] = grayscale_pixel(c.to_srgba_unmultiplied());
    eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Desaturates RGBA pixels in place with [`grayscale_pixel`] (a trailing
/// partial pixel is left alone). The grid can run it on its cards' pixels.
pub fn grayscale_rgba(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        *px = grayscale_pixel(*px);
    }
}

/// Side of the navigator's square box and its inset from the view's
/// bottom-right corner, in points (Eagle's `.smooth_zoom_navigator`).
pub const NAVIGATOR_SIDE: f32 = 120.0;

pub const NAVIGATOR_INSET: f32 = 16.0;

/// The visible-area frame never gets smaller than this, in points.
pub const NAVIGATOR_MIN_FRAME: f32 = 8.0;

/// Whether the navigator shows for a picture drawn `shown` big in a view
/// `view` big: once it overflows the view, by Eagle's test (97% of its
/// width wider than the view, or taller than it).
pub fn navigator_shown(shown: (f32, f32), view: (f32, f32)) -> bool {
    shown.0 * 0.97 > view.0 || shown.1 > view.1
}

/// Where the picture sits in the navigator's box: fitted inside a square
/// of `side`, centred. Returns `[x, y, w, h]` from the box's top-left.
pub fn navigator_picture(natural: (f32, f32), side: f32) -> [f32; 4] {
    let (w, h) = (natural.0.max(1.0), natural.1.max(1.0));
    let (pw, ph) = if w >= h { (side, side * h / w) } else { (side * w / h, side) };
    [(side - pw) / 2.0, (side - ph) / 2.0, pw, ph]
}

/// The part of the picture on screen, as fractions of it
/// (`[x0, y0, x1, y1]`, within 0..1), for a picture drawn at `picture` and
/// a view at `view` (both `[x0, y0, x1, y1]` on screen).
pub fn visible_part(picture: [f32; 4], view: [f32; 4]) -> [f32; 4] {
    let (w, h) = ((picture[2] - picture[0]).max(1e-6), (picture[3] - picture[1]).max(1e-6));
    let fx = |x: f32| ((x - picture[0]) / w).clamp(0.0, 1.0);
    let fy = |y: f32| ((y - picture[1]) / h).clamp(0.0, 1.0);
    [fx(view[0]), fy(view[1]), fx(view[2]), fy(view[3])]
}

/// The viewer's pan (the picture's centre from the view's centre) that
/// puts the point at fractions `(fx, fy)` of a picture drawn `shown` big in
/// the middle of the view, as a click on Eagle's navigator does.
pub fn pan_to_show(fx: f32, fy: f32, shown: (f32, f32)) -> (f32, f32) {
    (shown.0 * (0.5 - fx.clamp(0.0, 1.0)), shown.1 * (0.5 - fy.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod viewer_mode_tests {
    use super::*;

    #[test]
    fn navigator_maths() {
        // Shows once the picture overflows the view.
        assert!(!navigator_shown((800.0, 600.0), (1000.0, 700.0)));
        assert!(navigator_shown((1040.0, 600.0), (1000.0, 700.0)));
        assert!(!navigator_shown((1020.0, 600.0), (1000.0, 700.0)), "within Eagle's 3%");
        assert!(navigator_shown((800.0, 701.0), (1000.0, 700.0)));
        // A wide picture fills the box's width and sits in the middle.
        assert_eq!(navigator_picture((400.0, 200.0), 120.0), [0.0, 30.0, 120.0, 60.0]);
        assert_eq!(navigator_picture((100.0, 200.0), 120.0), [30.0, 0.0, 60.0, 120.0]);
        // The right half of a picture twice the view's width, top to bottom.
        assert_eq!(visible_part([-1000.0, 0.0, 1000.0, 500.0], [0.0, 0.0, 1000.0, 500.0]), [0.5, 0.0, 1.0, 1.0]);
        assert_eq!(visible_part([0.0, 0.0, 100.0, 100.0], [-50.0, 25.0, 50.0, 200.0]), [0.0, 0.25, 0.5, 1.0]);
        // Centring the middle needs no pan; the left edge pans right.
        assert_eq!(pan_to_show(0.5, 0.5, (2000.0, 1000.0)), (0.0, 0.0));
        assert_eq!(pan_to_show(0.0, 1.0, (2000.0, 1000.0)), (1000.0, -500.0));
        assert_eq!(pan_to_show(-3.0, 0.25, (2000.0, 1000.0)), (1000.0, 250.0));
    }

    #[test]
    fn grayscale_uses_css_luminance() {
        assert_eq!(grayscale_pixel([255, 255, 255, 255]), [255, 255, 255, 255]);
        assert_eq!(grayscale_pixel([0, 0, 0, 7]), [0, 0, 0, 7]);
        assert_eq!(grayscale_pixel([255, 0, 0, 255]), [54, 54, 54, 255]);
        assert_eq!(grayscale_pixel([0, 255, 0, 255]), [182, 182, 182, 255]);
        assert_eq!(grayscale_pixel([0, 0, 255, 128]), [18, 18, 18, 128]);
        let mut px = vec![255, 0, 0, 255, 10, 20, 30, 40, 9];
        grayscale_rgba(&mut px);
        assert_eq!(px, vec![54, 54, 54, 255, 19, 19, 19, 40, 9]);
    }
}

#[cfg(test)]
mod image_viewer_tests {
    use super::*;

    #[test]
    fn originals_only_when_the_full_tier_would_be_magnified() {
        assert!(wants_original("jpg", 12000, 3500.0));
        assert!(!wants_original("jpg", 12000, 1500.0), "fitted, the full tier is enough");
        assert!(!wants_original("jpg", 3000, 6000.0), "the full tier already is the original");
        assert!(!wants_original("gif", 12000, 6000.0), "animations are the player's");
        assert!(wants_original("psd", 12000, 6000.0), "the merged image");
        assert!(wants_original("psb", 12000, 6000.0));
        assert!(!wants_original("ai", 12000, 6000.0), "no decoder");
        assert!(wants_original("tiff", 9000, 4000.0));
        assert!(wants_original("nef", 8256, 4000.0), "a RAW's embedded preview");
        if cfg!(target_os = "macos") {
            for ext in ["avif", "heic", "heif", "hif", "jxl"] {
                assert!(wants_original(ext, 8000, 4000.0), "{ext} through ImageIO");
            }
        }
    }

    #[test]
    fn wheel_follows_the_preference() {
        use WheelMode::*;
        assert_eq!(WheelMode::default(), Scroll, "Eagle's default");
        assert_eq!(wheel_action(Scroll, (0.0, -30.0), 1.0, false), WheelAction::Pan(0.0, -30.0));
        assert_eq!(wheel_action(Scroll, (0.0, 0.0), 1.0, false), WheelAction::None);
        // ⌥ (or a pinch, which is also what ⌘-scrolling becomes) zooms in any mode.
        assert!(matches!(wheel_action(Scroll, (0.0, 30.0), 1.0, true), WheelAction::Zoom(f) if f > 1.0));
        assert_eq!(wheel_action(Paging, (0.0, 0.0), 1.25, false), WheelAction::Zoom(1.25));
        assert!(matches!(wheel_action(Zoom, (0.0, -30.0), 1.0, false), WheelAction::Zoom(f) if f < 1.0));
        // Scrolling down or right goes to the next item.
        assert_eq!(wheel_action(Paging, (0.0, -5.0), 1.0, false), WheelAction::Page(1));
        assert_eq!(wheel_action(Paging, (0.0, 5.0), 1.0, false), WheelAction::Page(-1));
        assert_eq!(wheel_action(Paging, (-5.0, 0.0), 1.0, false), WheelAction::Page(1));
    }

    #[test]
    fn svg_regions_cover_what_is_on_screen() {
        // A drawing four times the view's size, centred: the middle quarter
        // shows, rendered at the view's pixel size.
        let (rect, size) = svg_region([-1000.0, -1000.0, 2000.0, 2000.0], [0.0, 0.0, 1000.0, 1000.0], 2.0, 8192).unwrap();
        assert_eq!(rect, [1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]);
        assert_eq!(size, [2000, 2000]);
        // Capped on the long side.
        assert_eq!(svg_region([0.0, 0.0, 1000.0, 500.0], [0.0, 0.0, 1000.0, 500.0], 2.0, 1000).unwrap().1, [1000, 500]);
        assert_eq!(svg_region([2000.0, 0.0, 3000.0, 10.0], [0.0, 0.0, 1000.0, 1000.0], 2.0, 8192), None);
    }

    #[test]
    fn quicklook_regions_render_the_page_at_the_zoom_shown() {
        // A page drawn 3000 points square at 2× shows its middle third; the
        // whole page is asked for at its 6000 px size on screen (a step).
        let (rect, side) = quicklook_region([-1000.0, -1000.0, 2000.0, 2000.0], [0.0, 0.0, 1000.0, 1000.0], 2.0, None).unwrap();
        assert_eq!(rect, [1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]);
        assert_eq!(side, 6144);
        // A square page is held to 8192² pixels; a wide one may be longer.
        assert_eq!(quicklook_region([0.0, 0.0, 20000.0, 20000.0], [0.0, 0.0, 1000.0, 1000.0], 1.0, None).unwrap().1, 8192);
        assert_eq!(quicklook_region([0.0, 0.0, 40000.0, 10000.0], [0.0, 0.0, 1000.0, 1000.0], 1.0, None).unwrap().1, 16384);
        // A merged image has no pixel cap, only its own size.
        assert_eq!(composite_region([0.0, 0.0, 40000.0, 40000.0], [0.0, 0.0, 1000.0, 1000.0], 1.0, 90000).unwrap().1, 40448);
        assert_eq!(composite_region([0.0, 0.0, 40000.0, 40000.0], [0.0, 0.0, 1000.0, 1000.0], 1.0, 30000).unwrap().1, 30000);
        assert_eq!(composite_region([2000.0, 0.0, 3000.0, 10.0], [0.0, 0.0, 1000.0, 1000.0], 1.0, 30000), None);
        assert!(composite_tiled(16384, 16384) && composite_tiled(30000, 8000));
        assert!(!composite_tiled(30000, 30000));
        // A PSD never past its own pixels.
        assert_eq!(quicklook_region([0.0, 0.0, 9000.0, 6000.0], [0.0, 0.0, 1000.0, 1000.0], 1.0, Some(5000)).unwrap().1, 5000);
        assert_eq!(quicklook_region([2000.0, 0.0, 3000.0, 10.0], [0.0, 0.0, 1000.0, 1000.0], 2.0, None), None);
        assert_eq!(crop_span([0.25, 0.5, 0.75, 1.0], 400, 200), (100, 100, 200, 100));
        assert_eq!(crop_span([1.0, 1.0, 1.0, 1.0], 400, 200), (399, 199, 1, 1));
    }

    #[test]
    fn scrolling_stops_at_the_edges() {
        // A long picture scrolls until its bottom meets the view's bottom.
        assert_eq!(clamp_pan((0.0, -5000.0), (1000.0, 6000.0), (1000.0, 800.0)), (0.0, -2600.0));
        assert_eq!(clamp_pan((0.0, 100.0), (1000.0, 6000.0), (1000.0, 800.0)), (0.0, 100.0));
        // A small one moves only within the view.
        assert_eq!(clamp_pan((900.0, 0.0), (200.0, 100.0), (1000.0, 800.0)), (400.0, 0.0));
    }

    #[test]
    fn slider_runs_to_800_percent_and_snaps() {
        assert_eq!(slider_zoom(5.0), 0.05);
        assert_eq!(slider_zoom(100.0), 1.0);
        assert_eq!(slider_zoom(200.0), 8.0);
        assert_eq!(slider_zoom(150.0), 4.5);
        assert_eq!(slider_zoom(0.0), 0.05, "clamped to the range");
        for z in [0.05, 0.5, 1.0, 2.0, 8.0] {
            assert!((slider_zoom(zoom_slider_pos(z)) - z).abs() < 1e-5);
        }
        assert_eq!(zoom_slider_pos(20.0), 200.0);
        // Within 3% of a snap point the slider lands on it.
        assert_eq!(slider_zoom_snapped(52.0), 0.5);
        assert_eq!(slider_zoom_snapped(55.0), 0.55);
        assert_eq!(slider_zoom_snapped(zoom_slider_pos(1.98)), 2.0);
        assert_eq!(ZOOM_PRESETS.first(), Some(&5));
        assert_eq!(ZOOM_PRESETS.last(), Some(&800));
    }

    #[test]
    fn edge_fit_enlarges_to_the_view() {
        assert_eq!(edge_fit((200.0, 100.0), (1000.0, 800.0)), 5.0);
        assert_eq!(edge_fit((4000.0, 1000.0), (1000.0, 800.0)), 0.25);
        assert_eq!(edge_fit((10.0, 10.0), (1000.0, 800.0)), ZOOM_MAX);
    }

    #[test]
    fn long_images_by_eagles_ratios() {
        assert!(is_long_image(1000.0, 1801.0));
        assert!(!is_long_image(1000.0, 1800.0));
        assert!(!is_long_image(959.0, 2000.0), "under 960 wide needs 2.7x");
        assert!(is_long_image(400.0, 1081.0));
        assert!(!is_long_image(319.0, 5000.0));
        assert!(!is_long_image(8000.0, 1000.0), "wide strips aren't long");
    }

    #[test]
    fn smart_fit_fits_long_images_to_the_width_at_the_top() {
        // An ordinary image fits inside the margins, never enlarged.
        assert_eq!(smart_fit((2000.0, 1000.0), (1040.0, 600.0), 20.0, "a"), (0.5, 0.0));
        assert_eq!(smart_fit((200.0, 100.0), (1040.0, 600.0), 20.0, "a"), (1.0, 0.0));
        // A web page capture fills the width and opens at its top: its
        // centre sits half the overflow below the view's centre.
        let (scale, top) = smart_fit((1500.0, 9000.0), (1000.0, 800.0), 20.0, "page");
        assert!((scale - 1000.0 / 1500.0).abs() < 1e-6);
        assert!((top - (6000.0 - 800.0) / 2.0).abs() < 1e-3);
        // Narrower than the view, a long image shows at 100%.
        assert_eq!(smart_fit((400.0, 2000.0), (1000.0, 800.0), 20.0, "x").0, 1.0);
        // @2x opens at half size when that fits.
        assert_eq!(smart_fit((1200.0, 800.0), (1000.0, 800.0), 20.0, "shot@2x").0, 0.5);
        assert_eq!(density_scale("icon@3x"), Some(1.0 / 3.0));
        assert_eq!(density_scale("plain"), None);
    }

    #[test]
    fn too_large_only_for_eagles_formats_over_the_limit() {
        assert!(too_large_to_preview("tiff", 12000, 10001, false));
        assert!(!too_large_to_preview("tiff", 12000, 10000, false), "exactly the limit still shows");
        assert!(too_large_to_preview("heic", 20000, 7000, false));
        assert!(too_large_to_preview("tga", 20000, 7000, false));
        assert!(!too_large_to_preview("tiff", 20000, 7000, true), "a custom thumbnail shows instead");
        assert!(!too_large_to_preview("png", 20000, 7000, false));
        assert!(!too_large_to_preview("jpg", 100_000, 100_000, false));
    }

    #[test]
    fn tiles_fit_the_gpu_and_cover_the_image() {
        assert_eq!(tile_side(8192), 2048);
        assert_eq!(tile_side(1024), 1024);
        let (w, h, side) = (12000, 8000, 2048);
        let spans = tile_spans(w, h, side);
        let mut covered = 0;
        for s in &spans {
            let [x0, y0, x1, y1] = s.padded;
            assert!(x1 - x0 <= side && y1 - y0 <= side);
            let [cx0, cy0, cx1, cy1] = s.core;
            assert!(x0 <= cx0 && cx1 <= x1 && y0 <= cy0 && cy1 <= y1);
            covered += (cx1 - cx0) * (cy1 - cy0);
        }
        assert_eq!(covered, w * h, "cores tile the image exactly");
        // Inner edges carry a pixel of overlap; the image's own edges don't.
        assert_eq!(spans[0].padded, [0, 0, 2047, 2047]);
        assert_eq!(spans[1].padded[0], 2045);
        assert_eq!(tile_spans(10, 10, 2048), vec![TileSpan { core: [0, 0, 10, 10], padded: [0, 0, 10, 10] }]);
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    #[test]
    fn larger_renders_and_custom_thumbnails() {
        assert!(hires_quicklook("pptx") && !hires_quicklook("sketch"));
        assert!(quicklook_raster("psb") && !quicklook_raster("ai"));
        assert!(custom_thumbnail_is_picture("zip", true));
        assert!(!custom_thumbnail_is_picture("zip", false));
        assert!(!custom_thumbnail_is_picture("psd", true));
        assert!(!custom_thumbnail_is_picture("png", true));
        assert!(!custom_thumbnail_is_picture("cr3", true));
    }

    #[test]
    fn pixelated_from_200_percent() {
        assert!(!pixelated_at(true, 1.5));
        assert!(pixelated_at(true, 2.0));
        assert!(pixelated_at(true, 4.0));
        assert!(!pixelated_at(false, 4.0));
    }

    #[test]
    fn viewer_zoom_steps_and_limits() {
        assert_eq!(view_zoom(ViewZoom::In, 1.0), Some(1.25));
        assert_eq!(view_zoom(ViewZoom::Out, 1.25), Some(1.0));
        assert_eq!(view_zoom(ViewZoom::In, 7.0), Some(8.0));
        assert_eq!(view_zoom(ViewZoom::Out, 0.051), Some(0.05));
        assert_eq!(view_zoom(ViewZoom::Edge, 2.0), None);
        assert_eq!(view_zoom(ViewZoom::Actual, 0.3), Some(1.0));
        assert_eq!(view_zoom(ViewZoom::Fit, 4.0), None);
    }
}
