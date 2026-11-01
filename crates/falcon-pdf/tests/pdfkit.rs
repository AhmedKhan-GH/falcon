//! PDFKit tests on small PDFs written here by hand (plain PDF syntax with
//! the standard Helvetica font, so no other library is needed), plus a
//! read-only check of a real PDF from the fixture library if one is there.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use falcon_pdf::{Document, LinkTarget, SearchOptions};

const WORDS: [&str; 5] = ["alpha", "bravo", "charlie", "delta", "echo"];
const COLORS: [[u8; 3]; 5] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0], [255, 0, 255]];

/// One page: its `/Rotate` value and its content stream.
struct Page {
    rotate: i32,
    content: String,
}

/// Writes a PDF of `pages` (612 × 792 points each) with an outline entry
/// "Chapter Three" pointing at the third page when there are three or more.
fn write_pdf(path: &Path, pages: &[Page], title: &str) {
    let n = pages.len();
    // Objects: 1 catalog, 2 pages, 3 font, 4 info, 5 outlines, 6 outline
    // item, then a page and a content stream per page.
    let page_obj = |i: usize| 7 + 2 * i;
    let mut objs: Vec<String> = Vec::new();
    let outlines = n >= 3;
    objs.push(format!("<< /Type /Catalog /Pages 2 0 R{} >>", if outlines { " /Outlines 5 0 R" } else { "" }));
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_obj(i))).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objs.push(format!("<< /Title ({title}) >>"));
    objs.push("<< /Type /Outlines /First 6 0 R /Last 6 0 R /Count 1 >>".into());
    objs.push(format!("<< /Title (Chapter Three) /Parent 5 0 R /Dest [{} 0 R /Fit] >>", page_obj(2.min(n.saturating_sub(1)))));
    for (i, p) in pages.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate {} /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            p.rotate,
            page_obj(i) + 1
        ));
        objs.push(format!("<< /Length {} >>\nstream\n{}\nendstream", p.content.len(), p.content));
    }
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
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    std::fs::write(path, out).unwrap();
}

/// Five pages, each a coloured block inside white margins with the line
/// "Page N <word>" on it.
fn five_pages(dir: &Path) -> PathBuf {
    let pages: Vec<Page> = (0..5)
        .map(|i| {
            let [r, g, b] = COLORS[i].map(|c| c as f32 / 255.0);
            Page { rotate: 0, content: format!("{r} {g} {b} rg 72 72 468 648 re f 0 g BT /F1 24 Tf 100 680 Td (Page {} {}) Tj ET", i + 1, WORDS[i]) }
        })
        .collect();
    let path = dir.join("five.pdf");
    write_pdf(&path, &pages, "Falcon Test");
    path
}

fn close(a: [u8; 4], b: [u8; 3]) -> bool {
    (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= 12)
}

#[test]
fn opens_and_measures() {
    let dir = tempfile::tempdir().unwrap();
    let doc = Document::open(&five_pages(dir.path())).unwrap();
    assert_eq!(doc.page_count(), 5);
    assert_eq!(doc.page_size(0), Some((612.0, 792.0)));
    assert_eq!(doc.page_size(5), None);
    assert_eq!(doc.page_sizes().len(), 5);
    assert_eq!(doc.title().as_deref(), Some("Falcon Test"));
}

#[test]
fn renders_the_page_colour_inside_white_margins() {
    let dir = tempfile::tempdir().unwrap();
    let doc = Document::open(&five_pages(dir.path())).unwrap();
    let b = doc.render(2, 0.5, 4096).unwrap();
    assert_eq!((b.width, b.height), (306, 396));
    assert!(close(b.pixel(153, 198), COLORS[2]), "centre {:?}", b.pixel(153, 198));
    assert_eq!(b.pixel(5, 5), [255, 255, 255, 255]);
    assert_eq!(b.pixel(300, 390), [255, 255, 255, 255]);
    // The cap shrinks the picture, keeping its shape.
    let small = doc.render(2, 4.0, 400).unwrap();
    assert_eq!((small.width, small.height), (309, 400));
    assert!(doc.render(9, 1.0, 100).is_err());
}

#[test]
fn rotated_pages_render_turned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rotated.pdf");
    // Top half red, bottom half blue, turned a quarter clockwise: red ends
    // up on the right.
    let content = "1 0 0 rg 0 396 612 396 re f 0 0 1 rg 0 0 612 396 re f".to_string();
    write_pdf(&path, &[Page { rotate: 90, content }], "Rotated");
    let doc = Document::open(&path).unwrap();
    assert_eq!(doc.page_size(0), Some((792.0, 612.0)));
    let b = doc.render(0, 0.25, 4096).unwrap();
    assert_eq!((b.width, b.height), (198, 153));
    assert!(close(b.pixel(170, 76), [255, 0, 0]), "right {:?}", b.pixel(170, 76));
    assert!(close(b.pixel(28, 76), [0, 0, 255]), "left {:?}", b.pixel(28, 76));
}

#[test]
fn searches_text_and_selections() {
    let dir = tempfile::tempdir().unwrap();
    let doc = Document::open(&five_pages(dir.path())).unwrap();
    assert!(doc.text(0).contains("Page 1"), "{:?}", doc.text(0));
    assert!(doc.text(4).contains("echo"));
    assert_eq!(doc.text(7), "");

    let hits = doc.search("bravo", SearchOptions::default());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 1);
    let r = hits[0].rects[0];
    assert!(r.w > 0.0 && r.h > 0.0);
    // The text sits 680 points up a 792-point page: about 112 down from the top.
    assert!(r.y > 80.0 && r.y < 115.0 && r.x > 100.0, "{r:?}");
    assert_eq!(doc.search("BRAVO", SearchOptions::default()).len(), 1);
    assert!(doc.search("BRAVO", SearchOptions { match_case: true }).is_empty());
    assert_eq!(doc.search("Page", SearchOptions::default()).iter().map(|h| h.page).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
    assert!(doc.search("", SearchOptions::default()).is_empty());

    // Dragging across the line selects it.
    let sel = doc.selection(0, (90.0, 100.0), (500.0, 118.0));
    assert!(sel.text.contains("alpha"), "{sel:?}");
    assert!(!sel.rects.is_empty());
    assert!(doc.selection(0, (10.0, 700.0), (20.0, 710.0)).text.is_empty());
}

#[test]
fn outline_and_thumbnails() {
    let dir = tempfile::tempdir().unwrap();
    let doc = Document::open(&five_pages(dir.path())).unwrap();
    let outline = doc.outline();
    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].title, "Chapter Three");
    assert_eq!(outline[0].page, Some(2));
    assert_eq!(outline[0].depth, 0);
    let t = doc.thumbnail(0, 128).unwrap();
    assert_eq!((t.width, t.height), (99, 128));
}

/// Writes a PDF from its numbered objects (1 is the catalog).
fn write_objects(path: &Path, objs: &[String]) {
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

#[test]
fn reads_links() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.pdf");
    // Page 1 (object 3) has three links: to page 2 at a point, to page 2
    // through a GoTo action, and to a web address; page 2 is object 4.
    let objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R 6 0 R 7 0 R] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_string(),
        "<< /Type /Annot /Subtype /Link /Rect [72 700 272 720] /Border [0 0 0] /Dest [4 0 R /XYZ 0 692 0] >>".to_string(),
        "<< /Type /Annot /Subtype /Link /Rect [72 600 272 620] /Border [0 0 0] /A << /S /GoTo /D [4 0 R /Fit] >> >>".to_string(),
        "<< /Type /Annot /Subtype /Link /Rect [72 500 272 520] /Border [0 0 0] /A << /S /URI /URI (https://example.com/a) >> >>".to_string(),
    ];
    write_objects(&path, &objs);
    let doc = Document::open(&path).unwrap();
    let links = doc.links(0);
    assert_eq!(links.len(), 3, "{links:?}");
    // Display points: 720 up a 792-point page is 72 down from the top.
    let r = links[0].rect;
    assert!((r.x - 72.0).abs() < 0.5 && (r.y - 72.0).abs() < 0.5 && (r.w - 200.0).abs() < 0.5 && (r.h - 20.0).abs() < 0.5, "{r:?}");
    match &links[0].target {
        LinkTarget::Page { page: 1, point: Some((_, y)) } => assert!((y - 100.0).abs() < 0.5, "{y}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(links[1].target, LinkTarget::Page { page: 1, .. }), "{:?}", links[1].target);
    assert_eq!(links[2].target, LinkTarget::Url("https://example.com/a".into()));
    assert!(doc.links(1).is_empty());
    assert!(doc.links(9).is_empty());
}

#[test]
fn bad_files_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Document::open(&dir.path().join("missing.pdf")).is_err());
    let text = dir.path().join("not.pdf");
    std::fs::write(&text, "hello, not a pdf").unwrap();
    assert!(Document::open(&text).is_err());
    let empty = dir.path().join("empty.pdf");
    std::fs::write(&empty, "").unwrap();
    assert!(Document::open(&empty).is_err());
    assert!(Document::open(dir.path()).is_err());
}

#[test]
fn works_on_a_spawned_thread() {
    let dir = tempfile::tempdir().unwrap();
    let path = five_pages(dir.path());
    let results: Vec<_> = (0..3)
        .map(|k| {
            let path = path.clone();
            std::thread::spawn(move || {
                let doc = Document::open(&path).unwrap();
                let b = doc.render(k, 0.25, 1000).unwrap();
                (b.pixel(b.width / 2, b.height / 2), doc.search(WORDS[k], SearchOptions::default())[0].page)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    for (k, (px, page)) in results.into_iter().enumerate() {
        assert!(close(px, COLORS[k]), "page {k}: {px:?}");
        assert_eq!(page, k);
    }
}

/// Whether the file's contents are in iCloud only (opening it would start
/// a download).
fn dataless(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;
    std::fs::metadata(path).map(|m| m.st_flags() & 0x4000_0000 != 0).unwrap_or(true)
}

/// A real PDF from the fixture library, opened read-only. Skips when the
/// fixture is absent or holds no local PDF.
#[test]
fn reads_a_real_pdf_from_the_fixture() {
    let images = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/Assets.library/images");
    let Ok(dirs) = std::fs::read_dir(&images) else {
        eprintln!("skipped: no fixture library");
        return;
    };
    let pdf = dirs
        .flatten()
        .filter_map(|d| std::fs::read_dir(d.path()).ok())
        .flat_map(|files| files.flatten().map(|f| f.path()))
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) && !dataless(p));
    let Some(pdf) = pdf else {
        eprintln!("skipped: no local PDF in the fixture");
        return;
    };
    let before = std::fs::metadata(&pdf).unwrap().modified().unwrap();
    let doc = Document::open(&pdf).unwrap();
    assert!(doc.page_count() > 0);
    let (w, h) = doc.page_size(0).unwrap();
    assert!(w > 0.0 && h > 0.0);
    let b = doc.render(0, 0.5, 1024).unwrap();
    assert!(b.width > 0 && b.height > 0);
    let _ = doc.outline();
    let _ = doc.text(0);
    drop(doc);
    assert_eq!(std::fs::metadata(&pdf).unwrap().modified().unwrap(), before);
}

/// A large real PDF from the Books library, opened read-only, only when
/// its contents are already on disk (an iCloud-only file is skipped, never
/// downloaded). Times the open and a render of a middle page.
#[test]
fn reads_a_downloaded_books_pdf() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let books = Path::new(&home).join("Library/Mobile Documents/iCloud~com~apple~iBooks/Documents");
    let Ok(entries) = std::fs::read_dir(&books) else {
        eprintln!("skipped: no Books library");
        return;
    };
    let pdf = entries.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) && !dataless(p));
    let Some(pdf) = pdf else {
        eprintln!("skipped: no downloaded PDF in Books");
        return;
    };
    let start = std::time::Instant::now();
    let doc = Document::open(&pdf).unwrap();
    let opened = start.elapsed();
    let mid = doc.page_count() / 2;
    let b = doc.render(mid, 2.0, 4096).unwrap();
    eprintln!("{}: {} pages, opened in {opened:?}, page {} rendered {}×{} in {:?}", pdf.display(), doc.page_count(), mid + 1, b.width, b.height, start.elapsed() - opened);
    assert!(doc.page_count() > 0);
}
