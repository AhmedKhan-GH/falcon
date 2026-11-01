//! Thumbnail rules and rendering, following Eagle's `thumbs/` modules.
//!
//! Eagle stores thumbnails as lossy WebP (quality 75) under the name
//! `<name>_thumbnail.png`; sides above 16383 px fall back to JPEG at 0.7.
//! See the thumbnails chapter of the docs for what is observed and inferred.

use std::fs;
use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader, RgbaImage};

/// Short-side limit for ordinary images.
pub const DEFAULT_MAX_SIDE: u32 = 320;
/// An image up to this many pixels per side may be shown without a thumbnail.
pub const NO_THUMBNAIL_SIZE: u32 = 960;
/// ... as long as its file is no larger than this many bytes.
pub const NO_THUMBNAIL_FILE_SIZE: u64 = 15_000_000;
/// A PNG this large always gets a thumbnail, whatever its dimensions.
pub const PNG_FORCE_THUMBNAIL_SIZE: u64 = 1024 * 1024;
/// SVGs smaller than this (bytes) are displayed directly.
pub const SVG_INLINE_LIMIT: u64 = 100_000;
/// Largest side WebP can hold; beyond it Eagle writes JPEG.
pub const MAX_WEBP_SIDE: u32 = 16383;
/// WebP quality used by Eagle.
pub const WEBP_QUALITY: f32 = 75.0;
/// JPEG quality (0.7) used past [`MAX_WEBP_SIDE`].
pub const JPEG_QUALITY: u8 = 70;

/// Failure while decoding, rendering or encoding an image.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ThumbError(pub String);

impl ThumbError {
    fn new(e: impl std::fmt::Display) -> Self {
        ThumbError(e.to_string())
    }
}

/// Whether falcon can decode and thumbnail this extension (lowercase, no dot).
pub fn is_supported(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg")
}

/// The short-side limit for `ext`: 320 for ordinary images, 512 for icon
/// formats, 480 for documents, design files and other large-canvas formats.
/// 3D models are not scaled down: Eagle keeps its 1080 render (plus the
/// padding) as is.
pub fn max_side(ext: &str) -> u32 {
    match ext {
        e if crate::model3d::EXTENSIONS.contains(&e) => crate::model3d::raster::CANVAS + 2 * crate::model3d::raster::PADDING,
        "ico" | "icns" => 512,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "psd" | "ai" | "eps" | "hdr" | "exr" | "dds"
        | "cr3" => 480,
        _ => DEFAULT_MAX_SIDE,
    }
}

/// What to do about an item's thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Whether a thumbnail file is generated at all.
    pub thumbnail: bool,
    /// Value for the item's `noThumbnail` field (`None` = field absent).
    pub no_thumbnail: Option<bool>,
    /// Value for the item's `removeThumbnail` field (`None` = field absent).
    pub remove_thumbnail: Option<bool>,
}

impl Plan {
    /// Whether the palette is computed from the original file rather than the
    /// thumbnail (`noThumbnail && !removeThumbnail`).
    pub fn analyse_original(&self) -> bool {
        self.no_thumbnail == Some(true) && self.remove_thumbnail != Some(true)
    }
}

/// Plan for a raster image of `w` x `h` pixels and `file_size` bytes.
///
/// The original is shown directly when it is at most 960 px on both sides and
/// at most 15 MB; a PNG over 1 MiB is the exception and still gets a thumbnail.
pub fn plan_raster(ext: &str, w: u32, h: u32, file_size: u64) -> Plan {
    let small = w <= NO_THUMBNAIL_SIZE && h <= NO_THUMBNAIL_SIZE && file_size <= NO_THUMBNAIL_FILE_SIZE;
    let forced = ext == "png" && file_size > PNG_FORCE_THUMBNAIL_SIZE;
    if small && !forced {
        Plan { thumbnail: false, no_thumbnail: Some(true), remove_thumbnail: None }
    } else {
        Plan { thumbnail: true, no_thumbnail: None, remove_thumbnail: None }
    }
}

/// Plan for an SVG of `file_size` bytes whose render succeeded. A thumbnail is
/// always rendered (it feeds the palette); small SVGs delete it afterwards.
pub fn plan_svg(file_size: u64) -> Plan {
    if file_size < SVG_INLINE_LIMIT {
        Plan { thumbnail: true, no_thumbnail: Some(true), remove_thumbnail: Some(true) }
    } else {
        Plan { thumbnail: true, no_thumbnail: None, remove_thumbnail: None }
    }
}

/// Plan for an SVG that failed to render: no thumbnail, `noThumbnail` only.
pub fn plan_svg_failed() -> Plan {
    Plan { thumbnail: false, no_thumbnail: Some(true), remove_thumbnail: None }
}

/// Target size for a `w` x `h` image with short-side limit `max`.
///
/// Nothing changes unless the long side exceeds `max`. Then the short side
/// becomes `max` (landscape fixes the height, everything else the width) and
/// the other side follows with truncation. A wide image whose short side is
/// already under `max` is therefore scaled up, as in Eagle.
pub fn target_size(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w.max(h) <= max || w == 0 || h == 0 {
        return (w, h);
    }
    let scale = |long: u32, short: u32| (u64::from(max) * u64::from(long) / u64::from(short)) as u32;
    if w > h { (scale(w, h).max(1), max) } else { (max, scale(h, w).max(1)) }
}

/// Decodes a raster image, applying its EXIF orientation. Sniffs the format
/// from content, so it also reads Eagle's WebP-in-`.png` thumbnails.
pub fn decode(path: &Path) -> Result<DynamicImage, ThumbError> {
    let reader = ImageReader::open(path)
        .map_err(ThumbError::new)?
        .with_guessed_format()
        .map_err(ThumbError::new)?;
    let mut decoder = reader.into_decoder().map_err(ThumbError::new)?;
    let orientation = decoder.orientation().map_err(ThumbError::new)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(ThumbError::new)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// The largest JPEG preview embedded in a RAW photo, turned by the file's
/// EXIF orientation when it has one (from its TIFF IFDs, RAF's preview
/// EXIF or CR3's `CMT1`; see [`crate::exif::orientation`]). Eagle's RAW
/// viewer shows the preview dcraw extracts; falcon finds the same kind of picture by scanning for
/// baseline or progressive 8-bit JPEG streams and keeping the one with the
/// most pixels (the camera's full-size preview rather than its thumbnail).
pub fn raw_preview(data: &[u8]) -> Result<DynamicImage, ThumbError> {
    let (start, _) = embedded_jpegs(data).into_iter().max_by_key(|&(_, px)| px).ok_or_else(|| ThumbError("no embedded preview".into()))?;
    let mut img = image::load_from_memory_with_format(&data[start..], image::ImageFormat::Jpeg).map_err(ThumbError::new)?;
    let orientation = crate::exif::orientation(data).and_then(|v| image::metadata::Orientation::from_exif(v as u8));
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

/// Start offsets and pixel counts of the JPEG streams in `data` whose
/// headers parse up to a baseline or progressive 8-bit frame.
fn embedded_jpegs(data: &[u8]) -> Vec<(usize, u64)> {
    data.windows(3)
        .enumerate()
        .filter(|(_, w)| *w == [0xFF, 0xD8, 0xFF])
        .filter_map(|(i, _)| jpeg_frame_pixels(&data[i..]).map(|px| (i, px)))
        .collect()
}

/// Walks a JPEG's marker segments from its SOI to the frame header and
/// returns width x height for an 8-bit SOF0, SOF1 or SOF2 frame.
fn jpeg_frame_pixels(jpeg: &[u8]) -> Option<u64> {
    let mut at = 2;
    for _ in 0..64 {
        if *jpeg.get(at)? != 0xFF {
            return None;
        }
        let marker = *jpeg.get(at + 1)?;
        match marker {
            0xFF => at += 1,
            0x01 | 0xD0..=0xD7 => at += 2,
            0xC0..=0xC2 => {
                let f = jpeg.get(at + 4..at + 9)?;
                let (h, w) = (u16::from_be_bytes([f[1], f[2]]), u16::from_be_bytes([f[3], f[4]]));
                return (f[0] == 8 && w > 0 && h > 0).then_some(u64::from(w) * u64::from(h));
            }
            // Lossless, hierarchical or arithmetic frames, or the image data
            // before any frame: not a preview picture.
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF | 0xD8 | 0xD9 | 0xDA => return None,
            _ => {
                let len = u16::from_be_bytes([*jpeg.get(at + 2)?, *jpeg.get(at + 3)?]) as usize;
                if len < 2 {
                    return None;
                }
                at += 2 + len;
            }
        }
    }
    None
}

/// Long side of QuickLook renders, matching the size Eagle reports for
/// documents such as PDFs.
pub const QUICKLOOK_SIZE: u32 = 1024;
/// How long a QuickLook render may take.
pub const QUICKLOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Renders a preview of any file macOS QuickLook understands, with its long
/// side at most `size`.
pub fn quicklook(path: &Path, size: u32) -> Result<DynamicImage, ThumbError> {
    if !cfg!(target_os = "macos") {
        return Err(ThumbError("QuickLook is only available on macOS".into()));
    }
    let out = tempfile::tempdir().map_err(ThumbError::new)?;
    let mut child = std::process::Command::new("qlmanage")
        .arg("-t")
        .arg("-s")
        .arg(size.to_string())
        .arg("-o")
        .arg(out.path())
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(ThumbError::new)?;
    // qlmanage can hang on a malformed file; give up after a while.
    let deadline = std::time::Instant::now() + QUICKLOOK_TIMEOUT;
    let status = loop {
        if let Some(s) = child.try_wait().map_err(ThumbError::new)? {
            break s;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ThumbError("QuickLook timed out".into()));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let name = format!("{}.png", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    let png = out.path().join(name);
    if !status.success() || !png.is_file() {
        return Err(ThumbError("QuickLook made no preview".into()));
    }
    decode(&png)
}

/// What Spotlight knows about a file's pixel size and duration.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Spotlight {
    pub size: Option<(u32, u32)>,
    pub duration: Option<f64>,
}

/// Reads pixel size and duration from Spotlight (`mdls`). Files Spotlight
/// hasn't indexed, such as ones in temporary folders, give nothing.
pub fn spotlight(path: &Path) -> Spotlight {
    let out = std::process::Command::new("mdls")
        .args(["-raw", "-name", "kMDItemPixelWidth", "-name", "kMDItemPixelHeight", "-name", "kMDItemDurationSeconds"])
        .arg(path)
        .output();
    let Ok(out) = out else { return Spotlight::default() };
    let text = String::from_utf8_lossy(&out.stdout);
    let vals: Vec<Option<f64>> = text.split('\0').map(|v| v.trim().parse().ok()).collect();
    let get = |i: usize| vals.get(i).copied().flatten();
    Spotlight {
        size: match (get(0), get(1)) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w as u32, h as u32)),
            _ => None,
        },
        duration: get(2),
    }
}

/// Natural pixel size of an SVG file's contents.
pub fn svg_size(data: &[u8]) -> Result<(u32, u32), ThumbError> {
    let tree = parse_svg(data)?;
    let s = tree.size().to_int_size();
    Ok((s.width(), s.height()))
}

fn parse_svg(data: &[u8]) -> Result<resvg::usvg::Tree, ThumbError> {
    let tree = resvg::usvg::Tree::from_data(data, &resvg::usvg::Options::default()).map_err(ThumbError::new)?;
    let s = tree.size().to_int_size();
    if s.width() == 0 || s.height() == 0 {
        return Err(ThumbError("SVG has no size".into()));
    }
    Ok(tree)
}

/// Renders an SVG. With `max`, it is drawn straight at [`target_size`];
/// without, at its natural size.
pub fn render_svg(data: &[u8], max: Option<u32>) -> Result<DynamicImage, ThumbError> {
    use resvg::tiny_skia::{Pixmap, Transform};
    let tree = parse_svg(data)?;
    let natural = tree.size().to_int_size();
    let (w, h) = match max {
        Some(m) => target_size(natural.width(), natural.height(), m),
        None => (natural.width(), natural.height()),
    };
    if w > MAX_WEBP_SIDE * 2 || h > MAX_WEBP_SIDE * 2 {
        return Err(ThumbError("SVG too large to render".into()));
    }
    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| ThumbError("cannot allocate pixmap".into()))?;
    let transform = Transform::from_scale(w as f32 / tree.size().width(), h as f32 / tree.size().height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut raw = Vec::with_capacity(pixmap.pixels().len() * 4);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        raw.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    RgbaImage::from_raw(w, h, raw).map(DynamicImage::ImageRgba8).ok_or_else(|| ThumbError("bad pixmap".into()))
}

/// Renders part of an SVG at `size` pixels: `region` is the part to draw,
/// as fractions of the drawing (`[x0, y0, x1, y1]`, 0..1). The viewer uses
/// it to stay sharp when zoomed in, drawing only what is on screen.
pub fn render_svg_region(data: &[u8], region: [f32; 4], size: (u32, u32)) -> Result<DynamicImage, ThumbError> {
    use resvg::tiny_skia::{Pixmap, Transform};
    let tree = parse_svg(data)?;
    let (w, h) = size;
    let [x0, y0, x1, y1] = region;
    if w == 0 || h == 0 || w > MAX_WEBP_SIDE || h > MAX_WEBP_SIDE || x1 <= x0 || y1 <= y0 {
        return Err(ThumbError("bad SVG region".into()));
    }
    let (tw, th) = (tree.size().width(), tree.size().height());
    let (sx, sy) = (w as f32 / ((x1 - x0) * tw), h as f32 / ((y1 - y0) * th));
    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| ThumbError("cannot allocate pixmap".into()))?;
    resvg::render(&tree, Transform::from_row(sx, 0.0, 0.0, sy, -x0 * tw * sx, -y0 * th * sy), &mut pixmap.as_mut());
    let mut raw = Vec::with_capacity(pixmap.pixels().len() * 4);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        raw.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    RgbaImage::from_raw(w, h, raw).map(DynamicImage::ImageRgba8).ok_or_else(|| ThumbError("bad pixmap".into()))
}

/// Scales `img` per [`target_size`]; returns it unchanged when it fits.
pub fn resize(img: DynamicImage, max: u32) -> DynamicImage {
    resize_with(img, max, RESIZE_FILTER)
}

/// Filter for each resampling step. Chromium draws with
/// `imageSmoothingQuality = 'high'`, a cubic filter in Skia.
const RESIZE_FILTER: FilterType = FilterType::CatmullRom;

/// [`resize`] with an explicit filter.
///
/// Like Eagle, an image more than twice the target size is first halved
/// repeatedly (never below the target) while it is over 1.5 times the target,
/// and only then drawn at the final size.
pub fn resize_with(img: DynamicImage, max: u32, filter: FilterType) -> DynamicImage {
    let (tw, th) = target_size(img.width(), img.height(), max);
    let (w, h) = (img.width(), img.height());
    if (tw, th) == (w, h) {
        return img;
    }
    if w <= tw * 2 && h <= th * 2 {
        return img.resize_exact(tw, th, filter);
    }
    let (mut cw, mut ch) = (w, h);
    let mut cur = img;
    while f64::from(cw) > f64::from(tw) * 1.5 || f64::from(ch) > f64::from(th) * 1.5 {
        cw = tw.max(cw / 2);
        ch = th.max(ch / 2);
        cur = cur.resize_exact(cw, ch, filter);
    }
    cur.resize_exact(tw, th, filter)
}

/// Encodes `img` as Eagle does: lossy WebP at quality 75, or JPEG at 0.7 when
/// a side exceeds 16383 px.
pub fn encode(img: &DynamicImage) -> Result<Vec<u8>, ThumbError> {
    if img.width() > MAX_WEBP_SIDE || img.height() > MAX_WEBP_SIDE {
        let mut out = Vec::new();
        let rgb = img.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
            .encode_image(&rgb)
            .map_err(ThumbError::new)?;
        return Ok(out);
    }
    let rgba = img.to_rgba8();
    let mem = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()).encode(WEBP_QUALITY);
    Ok(mem.to_vec())
}

/// Resizes `img` to the limit `max` and writes the encoded thumbnail to `dest`.
pub fn write_thumbnail(img: DynamicImage, max: u32, dest: &Path) -> Result<(), ThumbError> {
    write_encoded(&resize(img, max), dest)
}

/// Encodes `img` as is and writes it to `dest`.
pub fn write_encoded(img: &DynamicImage, dest: &Path) -> Result<(), ThumbError> {
    let bytes = encode(img)?;
    fs::write(dest, bytes).map_err(ThumbError::new)
}

/// Largest file Eagle accepts as a custom thumbnail, in bytes.
pub const CUSTOM_THUMBNAIL_MAX_BYTES: u64 = 10_000_000;
/// Item key Eagle sets when the thumbnail was chosen by the user.
pub const CUSTOM_THUMBNAIL_KEY: &str = "customThumbnail";

/// Whether an item of this extension may get a custom thumbnail. Eagle hides
/// the command for the image formats it shows as themselves (tif, jpg, png,
/// bmp, webp); jpeg and tiff are falcon's addition, as the same formats.
pub fn custom_thumbnail_allowed(ext: &str) -> bool {
    !matches!(ext.to_lowercase().as_str(), "tif" | "tiff" | "jpg" | "jpeg" | "png" | "bmp" | "webp")
}

/// Whether the item's thumbnail was set by hand (`customThumbnail: true`).
pub fn has_custom_thumbnail(item: &crate::model::Item) -> bool {
    item.extra.get(CUSTOM_THUMBNAIL_KEY).and_then(|v| v.as_bool()) == Some(true)
}

/// Short side a frame chosen in the GIF player is brought down to.
pub const FRAME_THUMBNAIL_SIDE: u32 = 480;

/// The size Eagle draws a GIF frame thumbnail at: the short side brought
/// down to [`FRAME_THUMBNAIL_SIDE`], never enlarged.
pub fn frame_thumbnail_size(w: u32, h: u32) -> (u32, u32) {
    let short = w.min(h);
    if short <= FRAME_THUMBNAIL_SIDE {
        return (w, h);
    }
    let scale = |side: u32| ((u64::from(side) * u64::from(FRAME_THUMBNAIL_SIDE) / u64::from(short)) as u32).max(1);
    (scale(w), scale(h))
}

/// Width Eagle draws a video's thumbnail at, whatever the video's size.
pub const VIDEO_THUMBNAIL_WIDTH: u32 = 480;
/// Item key Eagle sets to the second a video's thumbnail was taken at.
pub const THUMBNAIL_AT_KEY: &str = "thumbnailAt";

/// The size of Eagle's video thumbnail for a `w` × `h` frame: 480 wide,
/// the height scaled to match and cut to whole pixels (a canvas size).
pub fn video_thumbnail_size(w: u32, h: u32) -> (u32, u32) {
    let h = (u64::from(VIDEO_THUMBNAIL_WIDTH) * u64::from(h) / u64::from(w.max(1))) as u32;
    (VIDEO_THUMBNAIL_WIDTH, h.max(1))
}

/// Encodes a video frame the way Eagle's thumbnailer does: drawn at
/// [`video_thumbnail_size`] and saved as a JPEG at full quality. Eagle
/// writes those JPEG bytes under the usual `_thumbnail.png` name.
pub fn video_thumbnail_jpeg(frame: &RgbaImage) -> Result<Vec<u8>, ThumbError> {
    let (w, h) = video_thumbnail_size(frame.width(), frame.height());
    let img = DynamicImage::ImageRgba8(frame.clone());
    let img = if (w, h) == (frame.width(), frame.height()) { img } else { img.resize_exact(w, h, RESIZE_FILTER) };
    jpeg(&img, 100)
}

/// `img` as a JPEG at `quality` (1..=100), alpha dropped.
pub fn jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, ThumbError> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(&img.to_rgb8()).map_err(ThumbError::new)?;
    Ok(out)
}

impl crate::Library {
    /// Makes the video frame shown at `at` seconds the item's thumbnail, as
    /// Eagle's "Set Current Frame as Thumbnail" does: Eagle regenerates the
    /// video thumbnail from that time, so the frame is written per
    /// [`video_thumbnail_jpeg`] to `<name>_thumbnail.png`, the palette is
    /// computed from it and `thumbnailAt` records the time (Eagle keeps it
    /// and draws later thumbnails from there). The item is not marked as a
    /// custom thumbnail and keeps its size.
    pub fn set_video_thumbnail(&mut self, id: &str, frame: &RgbaImage, at: f64) -> crate::Result<()> {
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let bytes = video_thumbnail_jpeg(frame).map_err(|e| self.invalid(e.0))?;
        let dest = self.info_dir(&item).join(format!("{}_thumbnail.png", item.name));
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).map_err(|e| self.invalid(e.to_string()))?;
        fs::write(&dest, &bytes).map_err(|e| crate::Error::io(&dest, e))?;
        item.palettes = crate::palette::extract(&img);
        let at = serde_json::Number::from_f64(at.max(0.0)).unwrap_or_else(|| 0.into());
        item.extra.insert(THUMBNAIL_AT_KEY.into(), serde_json::Value::Number(at));
        self.update_item(item)
    }

    /// Makes a frame of an animated item its thumbnail, as Eagle's GIF
    /// player does: the frame is scaled per [`frame_thumbnail_size`],
    /// written as a PNG to `<name>_thumbnail.png`, and the palette is
    /// computed from it. Nothing else changes: the item keeps its size and
    /// is not marked as a custom thumbnail, so Refresh Thumbnail draws the
    /// first frame again.
    pub fn set_frame_thumbnail(&mut self, id: &str, frame: &RgbaImage) -> crate::Result<()> {
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let (w, h) = frame_thumbnail_size(frame.width(), frame.height());
        let img = DynamicImage::ImageRgba8(frame.clone());
        let img = if (w, h) == (frame.width(), frame.height()) { img } else { img.resize_exact(w, h, RESIZE_FILTER) };
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| self.invalid(e.to_string()))?;
        let dest = self.info_dir(&item).join(format!("{}_thumbnail.png", item.name));
        fs::write(&dest, png).map_err(|e| crate::Error::io(&dest, e))?;
        item.palettes = crate::palette::extract(&img);
        self.update_item(item)
    }

    /// Makes the image at `image` the item's thumbnail, as Eagle's
    /// "Custom Thumbnail" does: the file is copied byte for byte to
    /// `<name>_thumbnail.png` (not resized or re-encoded), `width` and
    /// `height` become the picture's size, the palette is computed from it,
    /// `noThumbnail` and `noPreview` are removed and `customThumbnail: true`
    /// is set. Refuses files over 10 MB, pictures falcon can't decode, and
    /// items whose format is its own thumbnail.
    pub fn set_custom_thumbnail(&mut self, id: &str, image: &Path) -> crate::Result<()> {
        let item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        let item = custom_thumbnailed(self.root(), item, image)?;
        self.update_item(item)
    }

    /// Drops a custom thumbnail and draws the item's own again (Eagle's
    /// "Reset Thumbnail"). Returns false when the item had none.
    pub fn reset_custom_thumbnail(&mut self, id: &str) -> crate::Result<bool> {
        let mut item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        if !has_custom_thumbnail(&item) {
            return Ok(false);
        }
        item.extra.shift_remove(CUSTOM_THUMBNAIL_KEY);
        self.redraw_item(item).map(|()| true)
    }

    /// Redraws an item's thumbnail from its original and recomputes the
    /// palette (Eagle's "Refresh Thumbnail"). Size, birth time and mtime are
    /// read from the file again. As in Eagle, an item with a custom thumbnail
    /// is left alone; the result is then false.
    pub fn refresh_thumbnail(&mut self, id: &str) -> crate::Result<bool> {
        let item = self.item(id).cloned().ok_or_else(|| self.invalid(format!("no item {id}")))?;
        if has_custom_thumbnail(&item) {
            return Ok(false);
        }
        self.redraw_item(item).map(|()| true)
    }

    /// Re-reads the original's file times and redraws thumbnail and palette,
    /// then saves the item.
    pub(crate) fn redraw_item(&mut self, item: crate::model::Item) -> crate::Result<()> {
        let item = redrawn(self.root(), item)?;
        self.update_item(item)
    }
}

fn refused(root: &Path, reason: impl Into<String>) -> crate::Error {
    crate::Error::Import { path: root.to_path_buf(), reason: reason.into() }
}

/// The file work of [`crate::Library::set_custom_thumbnail`], without the
/// library: checks `image`, copies it over the thumbnail of `item` (in the
/// library at `root`) and returns the item as it should be saved. Lets a
/// caller that shares the library read and write files without holding
/// its lock.
pub fn custom_thumbnailed(root: &Path, mut item: crate::model::Item, image: &Path) -> crate::Result<crate::model::Item> {
    if !custom_thumbnail_allowed(&item.ext) {
        return Err(refused(root, format!("a .{} file is its own thumbnail", item.ext)));
    }
    let size = fs::metadata(image).map_err(|e| crate::Error::io(image, e))?.len();
    if size > CUSTOM_THUMBNAIL_MAX_BYTES {
        return Err(refused(root, "a custom thumbnail can be at most 10 MB"));
    }
    let img = decode(image).map_err(|e| refused(root, format!("cannot read {}: {e}", image.display())))?;
    let dest = item.info_dir(root).join(format!("{}_thumbnail.png", item.name));
    fs::copy(image, &dest).map_err(|e| crate::Error::io(&dest, e))?;
    item.width = img.width().into();
    item.height = img.height().into();
    item.palettes = crate::palette::extract(&img);
    item.no_thumbnail = None;
    item.extra.shift_remove("noPreview");
    item.extra.insert(CUSTOM_THUMBNAIL_KEY.into(), true.into());
    Ok(item)
}

/// The file work of Refresh Thumbnail, without the library: re-reads the
/// original's file times, redraws thumbnail and palette, and returns the
/// item as it should be saved.
pub fn redrawn(root: &Path, mut item: crate::model::Item) -> crate::Result<crate::model::Item> {
    let raw = item.original_path(root);
    crate::import::restat(&mut item, &raw)?;
    let thumb = item.info_dir(root).join(format!("{}_thumbnail.png", item.name));
    if !crate::import::redraw(&mut item, &raw, &thumb) {
        return Err(refused(root, format!("cannot draw a thumbnail for {}.{}", item.name, item.ext)));
    }
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tga_decodes_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.tga");
        let img = image::RgbaImage::from_fn(5, 3, |x, y| image::Rgba([x as u8 * 40, y as u8 * 80, 7, 200]));
        img.save(&path).unwrap();
        let back = decode(&path).unwrap().into_rgba8();
        assert_eq!(back.dimensions(), (5, 3));
        assert_eq!(*back.get_pixel(3, 2), image::Rgba([120, 160, 7, 200]));
    }

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&img).unwrap();
        out
    }

    #[test]
    fn raw_previews_are_the_largest_embedded_jpeg() {
        // A stand-in RAW: header bytes, a small thumbnail, sensor data with a
        // stray FF D8 FF, then the full-size preview.
        let mut raw = b"II*\0junk".to_vec();
        raw.extend(jpeg(16, 8));
        raw.extend([0x12, 0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x0B, 0x10, 0x00, 0x40, 0x00, 0x40]);
        raw.extend(vec![7u8; 1000]);
        raw.extend(jpeg(64, 32));
        raw.extend(vec![9u8; 100]);
        assert_eq!(embedded_jpegs(&raw).len(), 2, "the lossless frame is skipped");
        let img = raw_preview(&raw).unwrap();
        assert_eq!((img.width(), img.height()), (64, 32));
        assert!(raw_preview(&[0u8; 100]).is_err());
    }

    /// RAF and CR3 previews are turned by the orientation in their own
    /// containers (the preview's EXIF, the Canon `CMT1` block).
    #[test]
    fn raw_previews_follow_raf_and_cr3_orientation() {
        // A little-endian TIFF whose one IFD entry is Orientation = `o`.
        let tiff = |o: u16| [&b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0"[..], &o.to_le_bytes(), &[0; 6]].concat();
        let mut preview = jpeg(64, 32);
        let t = tiff(6);
        let app1 = [&[0xFF, 0xE1][..], &((t.len() + 8) as u16).to_be_bytes(), b"Exif\0\0", &t].concat();
        preview.splice(2..2, app1);
        let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        raf.resize(100, 0);
        raf[84..88].copy_from_slice(&100u32.to_be_bytes());
        raf[88..92].copy_from_slice(&(preview.len() as u32).to_be_bytes());
        raf.extend_from_slice(&preview);
        let img = raw_preview(&raf).unwrap();
        assert_eq!((img.width(), img.height()), (32, 64));

        let mp4 = |kind: &[u8], payload: &[u8]| [&((payload.len() + 8) as u32).to_be_bytes()[..], kind, payload].concat();
        let uuid = [0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48];
        let canon = [&uuid[..], &mp4(b"CMT1", &tiff(8))].concat();
        let cr3 = [mp4(b"ftyp", b"crx \0\0\0\x01crx isom"), mp4(b"moov", &mp4(b"uuid", &canon)), mp4(b"mdat", &jpeg(64, 32))].concat();
        let img = raw_preview(&cr3).unwrap();
        assert_eq!((img.width(), img.height()), (32, 64));
    }

    #[test]
    fn svg_regions_render_only_the_part_asked_for() {
        // Left half red, right half blue.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="50" height="50" fill="#f00"/><rect x="50" width="50" height="50" fill="#00f"/></svg>"##;
        let right = render_svg_region(svg, [0.5, 0.0, 1.0, 1.0], (400, 400)).unwrap().to_rgba8();
        assert_eq!(right.dimensions(), (400, 400));
        assert_eq!(right.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(right.get_pixel(399, 399).0, [0, 0, 255, 255]);
        // A strip across the middle shows both halves, magnified 20 times.
        let middle = render_svg_region(svg, [0.45, 0.0, 0.55, 0.1], (200, 100)).unwrap().to_rgba8();
        assert_eq!(middle.get_pixel(10, 50).0, [255, 0, 0, 255]);
        assert_eq!(middle.get_pixel(190, 50).0, [0, 0, 255, 255]);
        assert!(render_svg_region(svg, [0.5, 0.0, 0.5, 1.0], (10, 10)).is_err());
    }

    #[test]
    fn sizes() {
        assert_eq!(target_size(2000, 1000, 320), (640, 320));
        assert_eq!(target_size(1000, 2000, 320), (320, 640));
        assert_eq!(target_size(300, 200, 320), (300, 200));
        assert_eq!(target_size(1000, 200, 320), (1600, 320));
        assert_eq!(target_size(400, 400, 320), (320, 320));
        assert_eq!(target_size(330, 100, 320), (1056, 320));
    }

    #[test]
    fn frame_thumbnail_sizes() {
        assert_eq!(frame_thumbnail_size(1200, 900), (640, 480));
        assert_eq!(frame_thumbnail_size(900, 1200), (480, 640));
        assert_eq!(frame_thumbnail_size(2000, 480), (2000, 480), "a short side of 480 is left alone");
        assert_eq!(frame_thumbnail_size(300, 200), (300, 200));
    }

    #[test]
    fn video_thumbnails_are_480_wide_jpegs() {
        assert_eq!(video_thumbnail_size(1920, 1080), (480, 270));
        assert_eq!(video_thumbnail_size(1080, 1920), (480, 853), "cut to whole pixels");
        assert_eq!(video_thumbnail_size(160, 120), (480, 360), "small videos are drawn larger");
        assert_eq!(video_thumbnail_size(0, 0), (480, 1));
        let frame = RgbaImage::from_pixel(640, 360, image::Rgba([200, 30, 40, 255]));
        let bytes = video_thumbnail_jpeg(&frame).unwrap();
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
        let back = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(back.dimensions(), (480, 270));
        let px = back.get_pixel(240, 135).0;
        assert!(px[0].abs_diff(200) < 6 && px[1].abs_diff(30) < 6 && px[2].abs_diff(40) < 6, "{px:?}");
    }

    #[test]
    fn plans() {
        assert_eq!(plan_raster("jpg", 960, 960, 15_000_000).no_thumbnail, Some(true));
        assert!(plan_raster("jpg", 961, 10, 10).thumbnail);
        assert!(plan_raster("png", 100, 100, 2 << 20).thumbnail);
        assert!(!plan_raster("jpg", 100, 100, 2 << 20).thumbnail);
        assert!(plan_svg(99_999).remove_thumbnail == Some(true));
        assert!(plan_svg(100_000).no_thumbnail.is_none());
    }
}
