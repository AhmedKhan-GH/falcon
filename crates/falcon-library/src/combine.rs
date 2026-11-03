//! Combine Images: several pictures laid out on one canvas and saved as a
//! new PNG. In Eagle this is a plugin (`combine-images`); this is falcon's
//! own small version, not a copy of it.
//!
//! [`plan`] is the layout maths: a row scales every picture to the tallest
//! one's height, a column to the widest one's width, and a grid fits each
//! inside one cell (the largest width by the largest height), centred. The
//! canvas is kept to [`MAX_SIDE`] and [`MAX_PIXELS`]: past them everything,
//! gaps included, is scaled down together.
//!
//! [`stage`] does the work without touching the library, so it can run on
//! another thread: one picture is decoded at a time, in sRGB and turned by
//! its EXIF orientation ([`thumbnail::decode_for_display`]), an animation
//! gives its first frame, and the result is 8 bits per channel. The sources
//! are only read. The caller commits the [`Staged`] import.

use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgba, RgbaImage, imageops};

use crate::import::{ImportOptions, Staged};
use crate::thumbnail;

/// The longest side of a result. Past it the picture is scaled down.
pub const MAX_SIDE: u32 = 16_384;
/// The most pixels in a result: 400 MB in memory, which `image`'s default
/// 512 MB limit can still read back for the thumbnail and the viewer.
pub const MAX_PIXELS: u64 = 100_000_000;
/// Scaling passes before a layout counts as impossible. Each one lands just
/// inside the limits unless rounding (no picture under 1 px) keeps it over.
const PLAN_PASSES: usize = 24;

/// How the pictures are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One row, every picture the same height.
    Horizontal,
    /// One column, every picture the same width.
    Vertical,
    /// Rows of this many equal cells; the last row may be short.
    Grid(u32),
}

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub layout: Layout,
    /// Pixels between pictures, before any scaling down.
    pub gap: u32,
    /// The canvas colour; `None` is transparent.
    pub background: Option<[u8; 3]>,
}

/// Where one picture goes on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A worked-out layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub width: u32,
    pub height: u32,
    /// One per source, in order.
    pub cells: Vec<Cell>,
    /// The limits made it smaller than the sources' own size.
    pub scaled_down: bool,
}

/// A finished combine, ready for [`crate::Library::commit_import`].
#[derive(Debug)]
pub struct Combined {
    pub staged: Staged,
    pub width: u32,
    pub height: u32,
    pub scaled_down: bool,
}

/// Formats [`stage`] can read (lower-case, no dot): what the `image` crate
/// decodes among falcon's picture formats. SVG, RAW and QuickLook formats
/// aren't here.
pub fn can_combine(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "jfif" | "jpe" | "insp" | "webp" | "gif" | "bmp" | "tif" | "tiff")
}

fn scale_side(side: u32, scale: f64) -> u64 {
    ((side as f64 * scale).round() as u64).max(1)
}

/// `side` times `to / from`, at least 1 px.
fn ratio(side: u32, to: u64, from: u32) -> u64 {
    ((side as f64 * to as f64 / from as f64).round() as u64).max(1)
}

/// The canvas size and each picture's box (x, y, width, height) at `scale`.
fn arrange(sizes: &[(u32, u32)], layout: Layout, gap: u32, scale: f64) -> (u64, u64, Vec<[u64; 4]>) {
    let gap = (gap as f64 * scale).round() as u64;
    let max_w = sizes.iter().map(|s| s.0).max().unwrap_or(1);
    let max_h = sizes.iter().map(|s| s.1).max().unwrap_or(1);
    let mut cells = Vec::with_capacity(sizes.len());
    match layout {
        Layout::Horizontal => {
            let height = scale_side(max_h, scale);
            let mut x = 0;
            for &(w, h) in sizes {
                let width = ratio(w, height, h);
                cells.push([x, 0, width, height]);
                x += width + gap;
            }
            (x.saturating_sub(gap), height, cells)
        }
        Layout::Vertical => {
            let width = scale_side(max_w, scale);
            let mut y = 0;
            for &(w, h) in sizes {
                let height = ratio(h, width, w);
                cells.push([0, y, width, height]);
                y += height + gap;
            }
            (width, y.saturating_sub(gap), cells)
        }
        Layout::Grid(columns) => {
            let columns = (columns.max(1) as usize).min(sizes.len().max(1)) as u64;
            let rows = (sizes.len() as u64).div_ceil(columns);
            let (cell_w, cell_h) = (scale_side(max_w, scale), scale_side(max_h, scale));
            for (n, &(w, h)) in sizes.iter().enumerate() {
                let (column, row) = (n as u64 % columns, n as u64 / columns);
                let fit = (cell_w as f64 / w as f64).min(cell_h as f64 / h as f64);
                let width = ((w as f64 * fit).round() as u64).clamp(1, cell_w);
                let height = ((h as f64 * fit).round() as u64).clamp(1, cell_h);
                let x = column * (cell_w + gap) + (cell_w - width) / 2;
                let y = row * (cell_h + gap) + (cell_h - height) / 2;
                cells.push([x, y, width, height]);
            }
            (columns * cell_w + (columns - 1) * gap, rows * cell_h + (rows - 1) * gap, cells)
        }
    }
}

/// Lays out pictures of these sizes (width, height), keeping the canvas
/// within [`MAX_SIDE`] and [`MAX_PIXELS`]. Fails when there are no pictures,
/// one has no pixels, or they can't fit even at 1 px each.
pub fn plan(sizes: &[(u32, u32)], layout: Layout, gap: u32) -> Result<Plan, String> {
    if sizes.is_empty() {
        return Err("there are no images to combine".into());
    }
    if sizes.iter().any(|&(w, h)| w == 0 || h == 0) {
        return Err("an image has no pixels".into());
    }
    let mut scale = 1.0_f64;
    for _ in 0..PLAN_PASSES {
        let (width, height, cells) = arrange(sizes, layout, gap, scale);
        let side = width.max(height) as f64 / MAX_SIDE as f64;
        let area = (width as f64 * height as f64 / MAX_PIXELS as f64).sqrt();
        let over = side.max(area);
        if over <= 1.0 {
            let cells = cells.into_iter().map(|[x, y, width, height]| Cell { x: x as u32, y: y as u32, width: width as u32, height: height as u32 }).collect();
            return Ok(Plan { width: width as u32, height: height as u32, cells, scaled_down: scale < 1.0 });
        }
        // A little under, so rounding up doesn't need another pass.
        scale = scale / over * 0.999;
    }
    Err(format!("{} images can't fit in a picture of {MAX_SIDE} px a side", sizes.len()))
}

/// A picture's size as it will be shown, from its header alone: an EXIF
/// quarter turn swaps the sides.
pub fn display_size(path: &Path) -> Result<(u32, u32), String> {
    let name = file_name(path);
    let fail = |e: &dyn std::fmt::Display| format!("{name}: {e}");
    let reader = ImageReader::open(path).map_err(|e| fail(&e))?.with_guessed_format().map_err(|e| fail(&e))?;
    let mut decoder = reader.into_decoder().map_err(|e| fail(&e))?;
    let (w, h) = decoder.dimensions();
    Ok(match decoder.orientation().map_err(|e| fail(&e))? {
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH => (h, w),
        _ => (w, h),
    })
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// `img` at exactly this size. Transparent pixels' colours are hidden while
/// it is resized, so they don't bleed into the edges.
fn fit(img: DynamicImage, width: u32, height: u32) -> RgbaImage {
    let mut rgba = img.into_rgba8();
    if rgba.dimensions() == (width, height) {
        return rgba;
    }
    let filter = imageops::FilterType::Lanczos3;
    if rgba.pixels().all(|p| p[3] == 255) {
        return imageops::resize(&rgba, width, height, filter);
    }
    for p in rgba.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * a + 127) / 255) as u8;
        }
    }
    let mut out = imageops::resize(&rgba, width, height, filter);
    for p in out.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = (p[c] as u32 * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255)) as u8;
        }
    }
    out
}

/// Draws the pictures at `paths` on one canvas. `keep_going` hears of each
/// one before it is read, and returning false stops with `Ok(None)`.
pub fn compose(paths: &[impl AsRef<Path>], options: Options, keep_going: &mut impl FnMut(usize, &Path) -> bool) -> Result<Option<(RgbaImage, bool)>, String> {
    let sizes = paths.iter().map(|p| display_size(p.as_ref())).collect::<Result<Vec<_>, _>>()?;
    let plan = plan(&sizes, options.layout, options.gap)?;
    let background = options.background.map_or(Rgba([0, 0, 0, 0]), |[r, g, b]| Rgba([r, g, b, 255]));
    let mut canvas = RgbaImage::from_pixel(plan.width, plan.height, background);
    for (n, (path, cell)) in paths.iter().zip(&plan.cells).enumerate() {
        let path = path.as_ref();
        if !keep_going(n, path) {
            return Ok(None);
        }
        let img = thumbnail::decode_for_display(path).map_err(|e| format!("{}: {e}", file_name(path)))?;
        imageops::overlay(&mut canvas, &fit(img, cell.width, cell.height), cell.x as i64, cell.y as i64);
    }
    Ok(Some((canvas, plan.scaled_down)))
}

/// Combines the pictures and stages the PNG for import with `import`. See
/// [`compose`] for `keep_going`; it also hears of the save, as step
/// `paths.len()`.
pub fn stage(paths: &[impl AsRef<Path>], options: Options, import: &ImportOptions, mut keep_going: impl FnMut(usize, &Path) -> bool) -> Result<Option<Combined>, String> {
    let Some((canvas, scaled_down)) = compose(paths, options, &mut keep_going)? else { return Ok(None) };
    let dir = tempfile::Builder::new().prefix("falcon-combine").tempdir().map_err(|e| e.to_string())?;
    let file = dir.path().join("combined.png");
    if !keep_going(paths.len(), &file) {
        return Ok(None);
    }
    canvas.save_with_format(&file, ImageFormat::Png).map_err(|e| e.to_string())?;
    let (width, height) = canvas.dimensions();
    drop(canvas);
    let staged = crate::import::stage(&file, import).map_err(|e| e.to_string())?;
    Ok(Some(Combined { staged, width, height, scaled_down }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(x: u32, y: u32, width: u32, height: u32) -> Cell {
        Cell { x, y, width, height }
    }

    #[test]
    fn a_row_scales_to_the_tallest_height_with_gaps_between() {
        let p = plan(&[(100, 50), (40, 100), (200, 100)], Layout::Horizontal, 10).unwrap();
        assert_eq!(p.cells, [cell(0, 0, 200, 100), cell(210, 0, 40, 100), cell(260, 0, 200, 100)]);
        assert_eq!((p.width, p.height, p.scaled_down), (460, 100, false));
    }

    #[test]
    fn a_column_scales_to_the_widest_width() {
        let p = plan(&[(100, 50), (50, 100)], Layout::Vertical, 4).unwrap();
        assert_eq!(p.cells, [cell(0, 0, 100, 50), cell(0, 54, 100, 200)]);
        assert_eq!((p.width, p.height), (100, 254));
    }

    #[test]
    fn a_grid_centres_each_picture_in_equal_cells() {
        let p = plan(&[(100, 50), (50, 100), (100, 100)], Layout::Grid(2), 10).unwrap();
        assert_eq!(p.cells, [cell(0, 25, 100, 50), cell(135, 0, 50, 100), cell(0, 110, 100, 100)]);
        assert_eq!((p.width, p.height), (210, 210));
    }

    #[test]
    fn a_grid_has_no_empty_columns_and_at_least_one() {
        let sizes = [(10, 10), (10, 10)];
        assert_eq!(plan(&sizes, Layout::Grid(5), 2).unwrap().width, 22);
        let one = plan(&sizes, Layout::Grid(0), 2).unwrap();
        assert_eq!((one.width, one.height), (10, 22));
    }

    #[test]
    fn past_the_side_limit_everything_scales_together() {
        let p = plan(&[(10_000, 1000), (10_000, 1000)], Layout::Horizontal, 1000).unwrap();
        assert!(p.scaled_down);
        assert!(p.width <= MAX_SIDE && p.width > MAX_SIDE - 100, "{}", p.width);
        // The gap shrinks with the pictures.
        let gap = p.cells[1].x - p.cells[0].width;
        assert!((775..=785).contains(&gap), "{gap}");
        assert_eq!(p.cells[0].width, p.cells[1].width);
        assert!(p.cells[1].x + p.cells[1].width <= p.width);
    }

    #[test]
    fn past_the_pixel_limit_the_area_comes_down() {
        let p = plan(&[(16_000, 16_000)], Layout::Horizontal, 0).unwrap();
        assert!(p.scaled_down);
        assert!(p.width as u64 * p.height as u64 <= MAX_PIXELS);
        assert!(p.width > 9_900, "{}", p.width);
    }

    #[test]
    fn what_cant_fit_or_has_no_pixels_is_refused() {
        assert!(plan(&[(1, 1); 20_000], Layout::Horizontal, 0).is_err());
        assert!(plan(&[], Layout::Vertical, 0).is_err());
        assert!(plan(&[(10, 0), (10, 10)], Layout::Vertical, 0).is_err());
        // The limits themselves are allowed.
        assert!(!plan(&[(MAX_SIDE, 10)], Layout::Horizontal, 0).unwrap().scaled_down);
    }

    #[test]
    fn extreme_shapes_keep_a_pixel_and_stay_in_their_cell() {
        let p = plan(&[(10_000, 1), (1, 10_000)], Layout::Grid(2), 0).unwrap();
        for c in &p.cells {
            assert!(c.width >= 1 && c.height >= 1);
            assert!(c.x + c.width <= p.width && c.y + c.height <= p.height);
        }
    }

    #[test]
    fn only_formats_the_decoder_reads_can_combine() {
        for ext in ["png", "JPG", "webp", "gif", "bmp", "tiff"] {
            assert!(can_combine(ext), "{ext}");
        }
        for ext in ["svg", "mp4", "heic", "psd", ""] {
            assert!(!can_combine(ext), "{ext}");
        }
    }

    #[test]
    fn resizing_keeps_transparent_colours_out_of_the_edges() {
        // Half opaque red, half fully transparent green.
        let img = RgbaImage::from_fn(8, 8, |x, _| if x < 4 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 255, 0, 0]) });
        let out = fit(DynamicImage::ImageRgba8(img), 4, 4);
        for p in out.pixels().filter(|p| p[3] > 8) {
            assert!(p[1] < 16, "green showed through: {p:?}");
        }
    }
}
