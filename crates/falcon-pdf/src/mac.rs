//! The PDFKit side: every Objective-C and Core Graphics call in falcon's
//! PDF support is in this file.
//!
//! Most framework methods are `unsafe` in objc2 only because their
//! bindings cannot prove more than the types say. The invariants the calls
//! here rely on are the same throughout, so each block names which apply:
//!
//! - **owned**: every receiver and argument is a live `Retained`/`CFRetained`
//!   (or a reference borrowed from one) of the class the binding expects.
//! - **plain**: arguments are plain values (numbers, points, rects, option
//!   flags, a page index checked against `pageCount`).
//! - **one thread**: the `PDFDocument` and everything taken from it are
//!   only touched from the thread that opened it. `Document` holds
//!   `Retained` Objective-C objects, so it is neither `Send` nor `Sync`,
//!   and the compiler enforces this. PDFKit documents and pages may be
//!   created, searched and drawn off the main thread as long as one thread
//!   uses them at a time (the crate's tests exercise this).
//!
//! Every public method runs inside an autorelease pool, since the viewer
//! calls them on a worker thread that has none of its own.

use std::path::Path;

use objc2::AnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo, CGInterpolationQuality};
use objc2_foundation::{NSPoint, NSString, NSStringCompareOptions, NSURL};
use objc2_pdf_kit::{PDFDisplayBox, PDFDocument, PDFDocumentTitleAttribute, PDFOutline, PDFPage, PDFSelection};

use crate::geom::{self, PageGeom};
use crate::{Bitmap, Error, Hit, OutlineEntry, Rect, Result, SearchOptions, Selection, render_size};

/// Search stops collecting after this many matches.
const MAX_HITS: usize = 10_000;

fn err(msg: impl Into<String>) -> Error {
    Error(msg.into())
}

/// An open PDF file. Usable only on the thread that opened it.
pub struct Document {
    doc: Retained<PDFDocument>,
    count: usize,
}

impl Document {
    /// Opens a PDF. Fails cleanly for a missing file, a file that is not a
    /// PDF, and a password-protected PDF. Reads the file synchronously, so
    /// callers that must not block (the UI) call it on a worker thread.
    pub fn open(path: &Path) -> Result<Document> {
        if !path.is_file() {
            return Err(err(format!("no file at {}", path.display())));
        }
        let path = path.to_str().ok_or_else(|| err("the path is not valid UTF-8"))?;
        autoreleasepool(|_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            // SAFETY: owned; one thread (the result stays in this `Document`).
            let doc = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }.ok_or_else(|| err("not a PDF this system can read"))?;
            // SAFETY: owned; one thread.
            let (locked, count) = unsafe { (doc.isLocked(), doc.pageCount()) };
            if locked {
                return Err(err("the PDF is password-protected"));
            }
            Ok(Document { doc, count })
        })
    }

    pub fn page_count(&self) -> usize {
        self.count
    }

    /// The document's title from its metadata, if it has one.
    pub fn title(&self) -> Option<String> {
        autoreleasepool(|_| {
            // SAFETY: owned; one thread; the attribute key is a framework
            // constant set at load time.
            let attrs = unsafe { self.doc.documentAttributes() }?;
            let key = unsafe { PDFDocumentTitleAttribute };
            let value = attrs.objectForKey(key)?;
            let s = value.downcast::<NSString>().ok()?.to_string();
            let s = s.trim().to_string();
            (!s.is_empty()).then_some(s)
        })
    }

    fn page(&self, i: usize) -> Result<Retained<PDFPage>> {
        if i >= self.count {
            return Err(err(format!("no page {}", i + 1)));
        }
        // SAFETY: owned, plain (index checked above); one thread.
        unsafe { self.doc.pageAtIndex(i) }.ok_or_else(|| err(format!("page {} can't be read", i + 1)))
    }

    fn geom(page: &PDFPage) -> PageGeom {
        // SAFETY: owned, plain; one thread.
        let (b, rot) = unsafe { (page.boundsForBox(PDFDisplayBox::CropBox), page.rotation()) };
        PageGeom { x0: b.origin.x, y0: b.origin.y, w: b.size.width.abs(), h: b.size.height.abs(), turns: geom::turns(rot as i64) }
    }

    /// Page `i`'s size in points as shown (after its rotation).
    pub fn page_size(&self, i: usize) -> Option<(f64, f64)> {
        autoreleasepool(|_| self.page(i).ok().map(|p| Self::geom(&p).size()))
    }

    /// Every page's size, in order (one call for the viewer's layout).
    pub fn page_sizes(&self) -> Vec<(f64, f64)> {
        (0..self.count).map(|i| self.page_size(i).unwrap_or((612.0, 792.0))).collect()
    }

    /// Draws page `i` at `scale` pixels per point on white, turned as the
    /// page asks, shrunk so neither side exceeds `max_px`.
    pub fn render(&self, i: usize, scale: f64, max_px: usize) -> Result<Bitmap> {
        autoreleasepool(|_| {
            let page = self.page(i)?;
            let g = Self::geom(&page);
            let size = g.size();
            let (w, h) = render_size(size, scale, max_px.max(1));
            let mut rgba = vec![0u8; w * h * 4];
            let space = CGColorSpace::new_device_rgb().ok_or_else(|| err("no RGB colour space"))?;
            let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
            // SAFETY: `rgba` is `w * h * 4` bytes and outlives the context,
            // which is dropped before `rgba` is moved; 8 bits per component
            // with `w * 4` bytes per row matches that buffer exactly.
            let ctx = unsafe { CGBitmapContextCreate(rgba.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), info) }.ok_or_else(|| err("no bitmap context"))?;
            let c = Some(&*ctx);
            CGContext::set_rgb_fill_color(c, 1.0, 1.0, 1.0, 1.0);
            CGContext::fill_rect(c, CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)));
            CGContext::set_interpolation_quality(c, CGInterpolationQuality::High);
            if size.0 > 0.0 && size.1 > 0.0 {
                CGContext::scale_ctm(c, w as f64 / size.0, h as f64 / size.1);
            }
            // drawWithBox:toContext: applies the box's origin and the page's
            // rotation itself, drawing the page as shown into a context of
            // its displayed size (checked by the rotated-page test).
            // SAFETY: owned; one thread; `ctx` is a live bitmap context.
            unsafe { page.drawWithBox_toContext(PDFDisplayBox::CropBox, &ctx) };
            drop(ctx);
            // The background is opaque white, so premultiplied is straight.
            for px in rgba.as_chunks_mut::<4>().0 {
                px[3] = 255;
            }
            Ok(Bitmap { width: w, height: h, rgba })
        })
    }

    /// A small picture of page `i` whose longer side is `max_px`.
    pub fn thumbnail(&self, i: usize, max_px: usize) -> Result<Bitmap> {
        let size = self.page_size(i).ok_or_else(|| err(format!("no page {}", i + 1)))?;
        let longest = size.0.max(size.1).max(1.0);
        self.render(i, max_px as f64 / longest, max_px)
    }

    /// The text of page `i` (empty for a page without a text layer).
    pub fn text(&self, i: usize) -> String {
        autoreleasepool(|_| {
            let Ok(page) = self.page(i) else { return String::new() };
            // SAFETY: owned; one thread.
            unsafe { page.string() }.map(|s| s.to_string()).unwrap_or_default()
        })
    }

    /// Every match of `text` in the document, in reading order, with one
    /// rectangle per line it spans. An empty query finds nothing.
    pub fn search(&self, text: &str, options: SearchOptions) -> Vec<Hit> {
        if text.is_empty() {
            return Vec::new();
        }
        autoreleasepool(|_| {
            let mut flags = NSStringCompareOptions::DiacriticInsensitiveSearch;
            if !options.match_case {
                flags |= NSStringCompareOptions::CaseInsensitiveSearch;
            }
            // SAFETY: owned, plain; one thread.
            let found = unsafe { self.doc.findString_withOptions(&NSString::from_str(text), flags) };
            let mut hits = Vec::new();
            for sel in found.iter().take(MAX_HITS) {
                // SAFETY: owned; one thread.
                let pages = unsafe { sel.pages() };
                for page in pages.iter() {
                    // SAFETY: owned; one thread; the page belongs to this document.
                    let index = unsafe { self.doc.indexForPage(&page) };
                    if index >= self.count {
                        continue;
                    }
                    hits.push(Hit { page: index, rects: line_rects(&sel, &page) });
                }
            }
            hits
        })
    }

    /// The text between two display points on page `page` (as a drag from
    /// `from` to `to` selects it) and the rectangles to highlight.
    pub fn selection(&self, page: usize, from: (f64, f64), to: (f64, f64)) -> Selection {
        autoreleasepool(|_| {
            let Ok(p) = self.page(page) else { return Selection::default() };
            let g = Self::geom(&p);
            let (a, b) = (g.to_page(from.0, from.1), g.to_page(to.0, to.1));
            // SAFETY: owned, plain; one thread.
            let Some(sel) = (unsafe { p.selectionFromPoint_toPoint(NSPoint::new(a.0, a.1), NSPoint::new(b.0, b.1)) }) else {
                return Selection::default();
            };
            // SAFETY: owned; one thread.
            let text = unsafe { sel.string() }.map(|s| s.to_string()).unwrap_or_default();
            let rects = if text.is_empty() { Vec::new() } else { line_rects(&sel, &p) };
            Selection { text, rects }
        })
    }

    /// The table of contents, flattened depth-first; empty if there is none.
    pub fn outline(&self) -> Vec<OutlineEntry> {
        autoreleasepool(|_| {
            let mut out = Vec::new();
            // SAFETY: owned; one thread.
            if let Some(root) = unsafe { self.doc.outlineRoot() } {
                self.walk(&root, 0, &mut out);
            }
            out
        })
    }

    fn walk(&self, node: &PDFOutline, depth: usize, out: &mut Vec<OutlineEntry>) {
        // Deep or huge outlines are cut off rather than walked forever.
        if depth > 16 || out.len() > 5000 {
            return;
        }
        // SAFETY: owned, plain (child index below numberOfChildren); one thread.
        unsafe {
            for n in 0..node.numberOfChildren() {
                let Some(child) = node.childAtIndex(n) else { continue };
                let title = child.label().map(|s| s.to_string()).unwrap_or_default();
                let page = child.destination().and_then(|d| d.page()).map(|p| self.doc.indexForPage(&p)).filter(|&i| i < self.count);
                out.push(OutlineEntry { title, depth, page });
                self.walk(&child, depth + 1, out);
            }
        }
    }
}

/// One display rectangle per line of `sel` on `page`.
fn line_rects(sel: &PDFSelection, page: &PDFPage) -> Vec<Rect> {
    let g = Document::geom(page);
    // SAFETY: owned; one thread (called from `Document` methods).
    let lines = unsafe { sel.selectionsByLine() };
    let mut rects = Vec::new();
    for line in lines.iter() {
        // SAFETY: owned; one thread.
        let b = unsafe { line.boundsForPage(page) };
        if b.size.width > 0.0 && b.size.height > 0.0 {
            rects.push(g.rect_to_display(b.origin.x, b.origin.y, b.size.width, b.size.height));
        }
    }
    rects
}
