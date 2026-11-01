//! Item layouts: the grid, waterfall, justified and list placement maths, and the
//! per-folder layout choice.

use crate::locale::tr;
use std::collections::HashMap;
use super::scope::Scope;

/// Eagle's item layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Layout {
    /// Columns of equal width; each card keeps its image's shape.
    Waterfall,
    /// Rows of equal height filling the width (Eagle's default).
    #[default]
    Justified,
    /// Square cards.
    Grid,
    /// One row per item with its details.
    List,
}

impl Layout {
    pub const ALL: [Layout; 4] = [Layout::Waterfall, Layout::Justified, Layout::Grid, Layout::List];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Waterfall => tr("Waterfall"),
            Layout::Justified => tr("Justified"),
            Layout::Grid => tr("Grid"),
            Layout::List => tr("List"),
        }
    }

    /// Whether cards show the name and info lines under the image. Every
    /// card layout does, as Eagle's do; list rows show them as columns.
    pub fn has_caption(self) -> bool {
        !matches!(self, Layout::List)
    }
}

/// Where one card goes: `x, y, w, h` of the image box, in content space.
/// A caption, when the layout has one, sits below it.
pub type Place = [f32; 4];

/// Places items with the given aspect ratios (width / height) in `width`.
/// `size` is the zoom: column width for waterfall and grid, row height for
/// justified. `caption` is the band under each image (0 when the Display
/// switches hide both lines); rows with a caption are `row_gap` apart,
/// justified rows without one only `gap`, as Eagle packs them. Returns the
/// image boxes, the content height and the number of columns (for keyboard
/// navigation).
pub fn place(layout: Layout, aspects: &[f32], width: f32, size: f32, gap: f32, caption: f32, row_gap: f32) -> (Vec<Place>, f32, usize) {
    let aspect = |a: f32| if a.is_finite() && a > 0.0 { a } else { 1.0 };
    match layout {
        Layout::Grid => {
            let cols = columns(width, size, gap);
            let row_h = size + caption + row_gap;
            let out = (0..aspects.len())
                .map(|n| [(n % cols) as f32 * (size + gap), (n / cols) as f32 * row_h, size, size])
                .collect();
            (out, aspects.len().div_ceil(cols) as f32 * row_h, cols)
        }
        Layout::Waterfall => {
            let cols = columns(width, size, gap);
            let col_w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).max(1.0);
            let mut heights = vec![0.0f32; cols];
            let mut out = Vec::with_capacity(aspects.len());
            for &a in aspects {
                let (c, &y) = heights.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).unwrap();
                // Very tall or wide images are clamped so a column stays usable.
                let h = (col_w / aspect(a)).clamp(col_w * 0.25, col_w * 3.0);
                out.push([c as f32 * (col_w + gap), y, col_w, h]);
                heights[c] = y + h + caption + row_gap;
            }
            (out, heights.into_iter().fold(0.0, f32::max), cols)
        }
        Layout::Justified => {
            let mut out = Vec::with_capacity(aspects.len());
            let (mut y, mut start, mut sum) = (0.0f32, 0usize, 0.0f32);
            let mut rows = 0;
            for (i, &a) in aspects.iter().enumerate() {
                sum += aspect(a);
                let n = (i + 1 - start) as f32;
                let avail = width - gap * (n - 1.0);
                let last = i + 1 == aspects.len();
                if sum * size >= avail || last {
                    // Full rows are scaled to fill; the last keeps `size`.
                    let h = if sum * size >= avail { avail / sum } else { size };
                    let mut x = 0.0;
                    for &b in &aspects[start..=i] {
                        let w = aspect(b) * h;
                        out.push([x, y, w, h]);
                        x += w + gap;
                    }
                    // The caption band sits under the row's images, so the
                    // widths (from the images' shapes) don't change.
                    y += h + if caption > 0.0 { caption + row_gap } else { gap };
                    rows += 1;
                    start = i + 1;
                    sum = 0.0;
                }
            }
            let cols = aspects.len().div_ceil(rows.max(1));
            (out, y, cols.max(1))
        }
        Layout::List => {
            let row = 44.0;
            let out = (0..aspects.len()).map(|n| [0.0, n as f32 * row, width, row]).collect();
            (out, aspects.len() as f32 * row, 1)
        }
    }
}

/// Grid layout: how many square cards of `size` fit in `width`.
pub fn columns(width: f32, size: f32, gap: f32) -> usize {
    (((width + gap) / (size + gap)).floor() as usize).max(1)
}

/// The key a view's layout is remembered under, as Eagle keys it: the
/// folder's or smart folder's id, else the library (`root`) for every
/// other page.
pub fn layout_key(scope: &Scope, root: &str) -> String {
    match scope {
        Scope::Folder(id) | Scope::Smart(id) => id.clone(),
        _ => root.to_string(),
    }
}

/// The layout a view opens in: its own, else the library's, else
/// `fallback` (falcon's single layout from before), else Justified.
pub fn layout_for(saved: &HashMap<String, Layout>, scope: &Scope, root: &str, fallback: Option<Layout>) -> Layout {
    saved.get(&layout_key(scope, root)).or_else(|| saved.get(root)).copied().or(fallback).unwrap_or(Layout::Justified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_place_every_item_inside_the_width() {
        let aspects = [1.0, 2.0, 0.5, 1.5, 1.0, 3.0, 0.75];
        for layout in Layout::ALL {
            let (places, height, cols) = place(layout, &aspects, 600.0, 140.0, 8.0, 46.0, 10.0);
            assert_eq!(places.len(), aspects.len(), "{layout:?}");
            assert!(cols >= 1 && height > 0.0);
            for [x, y, w, h] in places {
                assert!(x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0, "{layout:?}");
                assert!(x + w <= 600.0 + 0.01, "{layout:?} overflows: {x} + {w}");
            }
        }
    }

    #[test]
    fn justified_rows_fill_the_width() {
        let (places, _, _) = place(Layout::Justified, &[1.0; 9], 400.0, 100.0, 0.0, 0.0, 0.0);
        // Four squares of 100 fill a 400 row exactly.
        let first_row: f32 = places.iter().filter(|p| p[1] == 0.0).map(|p| p[2]).sum();
        assert!((first_row - 400.0).abs() < 0.01);
        assert!(places.iter().all(|p| (p[3] - 100.0).abs() < 0.01));
    }

    #[test]
    fn justified_rows_leave_room_for_the_caption() {
        let aspects = [1.0; 8];
        let (bare, bare_h, _) = place(Layout::Justified, &aspects, 400.0, 100.0, 0.0, 0.0, 10.0);
        let (cap, cap_h, _) = place(Layout::Justified, &aspects, 400.0, 100.0, 0.0, 46.0, 10.0);
        // The same images in the same places across, each row 46 + 10 lower
        // per row above it.
        for (n, (b, c)) in bare.iter().zip(&cap).enumerate() {
            assert_eq!((b[0], b[2], b[3]), (c[0], c[2], c[3]), "card {n}");
            let row = (n / 4) as f32;
            assert!((c[1] - (row * (100.0 + 46.0 + 10.0))).abs() < 0.01, "card {n} at {}", c[1]);
        }
        assert!((bare_h - 200.0).abs() < 0.01);
        assert!((cap_h - 2.0 * 156.0).abs() < 0.01);
        // A caption never overlaps the next row's images.
        assert!(cap[4][1] >= cap[0][1] + cap[0][3] + 46.0);
    }

    #[test]
    fn justified_without_caption_packs_rows_by_the_gap() {
        let (p, h, _) = place(Layout::Justified, &[1.0; 8], 424.0, 100.0, 8.0, 0.0, 10.0);
        assert!((p[4][1] - 108.0).abs() < 0.01, "rows are gap apart, not row_gap: {}", p[4][1]);
        assert!((h - 216.0).abs() < 0.01);
    }

    #[test]
    fn waterfall_fills_the_shortest_column() {
        let (places, _, cols) = place(Layout::Waterfall, &[0.5, 2.0, 2.0], 300.0, 140.0, 20.0, 0.0, 0.0);
        assert_eq!(cols, 2);
        // The tall first item fills column 0; the next two stack in column 1.
        assert_eq!(places[1][0], places[2][0]);
        assert!(places[2][1] > places[1][1]);
    }
}
