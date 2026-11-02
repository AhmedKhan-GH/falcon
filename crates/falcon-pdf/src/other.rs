//! Platforms without PDFKit: opening fails cleanly, so the viewer shows
//! the item's preview instead.

use std::path::Path;

use crate::{Attachment, Bitmap, Error, Hit, Info, Layer, Link, OutlineEntry, Rect, Result, SearchOptions, Selection};

/// Printing needs macOS's PDFKit.
pub fn print(_path: &Path, _password: Option<&str>, _title: &str) -> Result<bool> {
    Err(Error("printing needs macOS".into()))
}

/// Never constructed off macOS.
pub struct Document {
    _private: (),
}

impl Document {
    pub fn open(_path: &Path) -> Result<Document> {
        Err(Error("the PDF reader needs macOS".into()))
    }
    pub fn open_with_password(_path: &Path, _password: Option<&str>) -> Result<Document> {
        Err(Error("the PDF reader needs macOS".into()))
    }
    pub fn info(&self) -> Info {
        Info::default()
    }
    pub fn page_count(&self) -> usize {
        0
    }
    pub fn title(&self) -> Option<String> {
        None
    }
    pub fn page_size(&self, _i: usize) -> Option<(f64, f64)> {
        None
    }
    pub fn page_sizes(&self) -> Vec<(f64, f64)> {
        Vec::new()
    }
    pub fn render(&self, _i: usize, _scale: f64, _max_px: usize) -> Result<Bitmap> {
        Err(Error("the PDF reader needs macOS".into()))
    }
    pub fn render_part(&self, _i: usize, _part: Rect, _w: usize, _h: usize) -> Result<Bitmap> {
        Err(Error("the PDF reader needs macOS".into()))
    }
    pub fn thumbnail(&self, _i: usize, _max_px: usize) -> Result<Bitmap> {
        Err(Error("the PDF reader needs macOS".into()))
    }
    pub fn text(&self, _i: usize) -> String {
        String::new()
    }
    pub fn search(&self, _text: &str, _options: SearchOptions) -> Vec<Hit> {
        Vec::new()
    }
    pub fn selection(&self, _page: usize, _from: (f64, f64), _to: (f64, f64)) -> Selection {
        Selection::default()
    }
    pub fn outline(&self) -> Vec<OutlineEntry> {
        Vec::new()
    }
    pub fn links(&self, _i: usize) -> Vec<Link> {
        Vec::new()
    }
    pub fn attachments(&self) -> Vec<Attachment> {
        Vec::new()
    }
    pub fn attachment_data(&self, _i: usize) -> Result<Vec<u8>> {
        Err(Error("PDF needs macOS".into()))
    }
    pub fn layers(&self) -> Vec<Layer> {
        Vec::new()
    }
}
