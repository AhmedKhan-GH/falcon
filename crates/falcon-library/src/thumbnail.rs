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
pub fn max_side(ext: &str) -> u32 {
    match ext {
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

/// Long side of QuickLook renders, matching the size Eagle reports for
/// documents such as PDFs.
pub const QUICKLOOK_SIZE: u32 = 1024;

/// Renders a preview of any file macOS QuickLook understands, with its long
/// side at most `size`.
pub fn quicklook(path: &Path, size: u32) -> Result<DynamicImage, ThumbError> {
    if !cfg!(target_os = "macos") {
        return Err(ThumbError("QuickLook is only available on macOS".into()));
    }
    let out = tempfile::tempdir().map_err(ThumbError::new)?;
    let status = std::process::Command::new("qlmanage")
        .arg("-t")
        .arg("-s")
        .arg(size.to_string())
        .arg("-o")
        .arg(out.path())
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(ThumbError::new)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn plans() {
        assert_eq!(plan_raster("jpg", 960, 960, 15_000_000).no_thumbnail, Some(true));
        assert!(plan_raster("jpg", 961, 10, 10).thumbnail);
        assert!(plan_raster("png", 100, 100, 2 << 20).thumbnail);
        assert!(!plan_raster("jpg", 100, 100, 2 << 20).thumbnail);
        assert!(plan_svg(99_999).remove_thumbnail == Some(true));
        assert!(plan_svg(100_000).no_thumbnail.is_none());
    }
}
