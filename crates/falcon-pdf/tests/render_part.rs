//! Drawing part of a page: it must land where the whole page's render has
//! it, for a crop box and for every `/Rotate`.

#![cfg(target_os = "macos")]

use std::path::Path;

use falcon_pdf::{Document, Rect};

/// A one-page PDF with the page entries `page` and the content stream `content`.
fn write_pdf(path: &Path, page: &str, content: &str) {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R {page} /Contents 4 0 R >>"),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (k, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", k + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    std::fs::write(path, out).unwrap();
}

/// Four coloured quarters of a 400 x 300 media box.
const QUARTERS: &str = "1 0 0 rg 0 150 200 150 re f 0 1 0 rg 200 150 200 150 re f 0 0 1 rg 0 0 200 150 re f 1 1 0 rg 200 0 200 150 re f";

/// The whole page and a sample of parts agree, pixel for pixel, away from
/// the colour edges.
fn parts_match_the_whole(page: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.ai");
    write_pdf(&path, page, QUARTERS);
    let doc = Document::open(&path).unwrap();
    let size = doc.page_size(0).unwrap();
    let whole = doc.render(0, 1.0, 4096).unwrap();
    for (fx, fy) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.6, 0.55), (0.3, 0.7)] {
        let part = Rect { x: fx * size.0, y: fy * size.1, w: 0.25 * size.0, h: 0.25 * size.1 };
        let bmp = doc.render_part(0, part, 80, 60).unwrap();
        assert_eq!((bmp.width, bmp.height), (80, 60));
        for (ux, uy) in [(0.1, 0.1), (0.9, 0.1), (0.1, 0.9), (0.9, 0.9)] {
            let (x, y) = (part.x + ux * part.w, part.y + uy * part.h);
            assert_eq!(bmp.pixel((ux * 80.0) as usize, (uy * 60.0) as usize), whole.pixel(x as usize, y as usize), "{page}: part at {fx},{fy}, sample {ux},{uy}");
        }
    }
}

#[test]
fn parts_land_where_the_whole_page_has_them() {
    parts_match_the_whole("/MediaBox [0 0 400 300]");
    // The crop box is what shows, and it needn't start at the media's origin.
    parts_match_the_whole("/MediaBox [0 0 400 300] /CropBox [50 20 370 280]");
    parts_match_the_whole("/MediaBox [-50 -50 450 350]");
    for turn in [90, 180, 270] {
        parts_match_the_whole(&format!("/MediaBox [0 0 400 300] /Rotate {turn}"));
        parts_match_the_whole(&format!("/MediaBox [0 0 400 300] /CropBox [50 20 370 280] /Rotate {turn}"));
    }
}

/// Far past any whole-page render: an edge stays one pixel wide.
#[test]
fn a_tiny_part_is_sharp_at_any_scale() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.ai");
    write_pdf(&path, "/MediaBox [0 0 4000 3000]", "0 0 0 rg 2000 0 2000 3000 re f");
    let doc = Document::open(&path).unwrap();
    // A hundredth of a point per pixel: 400,000 px across the whole page.
    let bmp = doc.render_part(0, Rect { x: 1999.0, y: 1500.0, w: 2.0, h: 2.0 }, 200, 200).unwrap();
    assert_eq!(bmp.pixel(98, 100), [255, 255, 255, 255]);
    assert_eq!(bmp.pixel(102, 100), [0, 0, 0, 255]);
}

/// Degenerate requests give a picture, not a panic.
#[test]
fn empty_parts_are_white() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.ai");
    write_pdf(&path, "/MediaBox [0 0 40 30]", "0 0 0 rg 0 0 40 30 re f");
    let doc = Document::open(&path).unwrap();
    let bmp = doc.render_part(0, Rect::default(), 0, 0).unwrap();
    assert_eq!((bmp.width, bmp.height, bmp.pixel(0, 0)), (1, 1, [255, 255, 255, 255]));
    assert!(doc.render_part(3, Rect::default(), 4, 4).is_err());
}
