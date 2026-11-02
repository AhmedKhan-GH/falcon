//! PDF documents for falcon's viewer, through the operating system's own
//! PDF engine: PDFKit on macOS. No third-party PDF library is linked, so
//! falcon keeps its MIT/Apache-2.0 licence.
//!
//! [`Document`] opens a file and hands out what a reader needs: page
//! sizes, pages or parts of them rendered to RGBA pixels at any scale, each page's text,
//! search hits, the text and rectangles of a drag selection, and the table
//! of contents.
//!
//! A `Document` may be opened and used on any thread, but only on the
//! thread that opened it (it is not `Send`). The viewer opens it on a
//! worker thread so a slow or not-yet-downloaded file never blocks the UI.
//!
//! All coordinates in this API are **display points**: PDF points (1/72
//! inch) with the origin at the top-left of the page as shown, after the
//! page's own rotation. The private `geom` module converts to and from
//! PDFKit's page space (origin bottom-left, unrotated).
//!
//! All Objective-C interop sits in the private `mac` module.

mod geom;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::{Document, print};

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
pub use other::{Document, print};

/// An RGBA picture, 8 bits per channel, rows top to bottom, opaque.
#[derive(Clone, Debug, PartialEq)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Bitmap {
    /// The pixel at (x, y), as `[r, g, b, a]`.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

/// A rectangle in display points (origin top-left of the page as shown).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where a search matched: one page and one rectangle per line of the match.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// 0-based page index.
    pub page: usize,
    pub rects: Vec<Rect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchOptions {
    /// Off by default, as in most readers: "Bravo" finds "bravo".
    pub match_case: bool,
    /// Off by default: "e" finds "é".
    pub match_diacritics: bool,
    /// Only matches with no letter or digit right before or after them.
    pub whole_words: bool,
}

/// Whether the match `start..start + len` (UTF-16 units, as PDFKit counts)
/// in `text` stands as whole words: no letter, digit or underscore touches
/// either end.
pub fn is_whole_word(text: &[u16], start: usize, len: usize) -> bool {
    let wordy = |i: usize| text.get(i).and_then(|&u| char::from_u32(u32::from(u))).is_some_and(|c| c.is_alphanumeric() || c == '_');
    let end = start + len;
    if len == 0 || end > text.len() {
        return false;
    }
    !(start > 0 && wordy(start - 1) && wordy(start)) && !(end < text.len() && wordy(end) && wordy(end - 1))
}

/// The text under a drag selection and the rectangles to highlight.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub text: String,
    pub rects: Vec<Rect>,
}

/// One entry of the table of contents, flattened in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineEntry {
    pub title: String,
    /// 0 for top-level entries.
    pub depth: usize,
    /// The page it points to, if it points into this document.
    pub page: Option<usize>,
}

/// Where a link in a page goes.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    /// A page of this document (0-based), and the display point on it to
    /// show, when the link names one.
    Page { page: usize, point: Option<(f64, f64)> },
    /// A web address or other URL.
    Url(String),
}

/// A link annotation: the area to click, in display points, and where it
/// goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub rect: Rect,
    pub target: LinkTarget,
}

/// A file embedded in the document, as pdf.js's attachments list shows
/// them: from the catalog's EmbeddedFiles name tree, then the pages'
/// file attachment annotations.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// The file name it was embedded under.
    pub name: String,
    /// The size its parameters state, in bytes, if any.
    pub size: Option<u64>,
    /// The description it carries, if any.
    pub description: Option<String>,
}

/// An optional content group ("layer"), as pdf.js's layers list shows
/// them, with whether the document's default configuration shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
}

/// A document's metadata, for a Document Properties dialog. Dates are
/// seconds since 1970.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    /// The application that made the original document.
    pub creator: Option<String>,
    /// The software that wrote the PDF.
    pub producer: Option<String>,
    pub created: Option<f64>,
    pub modified: Option<f64>,
    /// The PDF version, as (major, minor).
    pub version: (i64, i64),
    pub pages: usize,
    pub encrypted: bool,
}

/// The message of the error [`Document::open`] returns for a PDF that needs
/// a password, and of the one for a password that doesn't open it.
pub const NEEDS_PASSWORD: &str = "the PDF is password-protected";
pub const WRONG_PASSWORD: &str = "the password does not open this PDF";

/// Why a document could not be opened or read.
#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub String);

impl Error {
    /// Whether opening failed only for want of the right password.
    pub fn needs_password(&self) -> bool {
        self.0 == NEEDS_PASSWORD || self.0 == WRONG_PASSWORD
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The pixel size a page of `size` display points renders to at `scale`
/// pixels per point, shrunk so neither side exceeds `max_px`; never 0.
pub fn render_size(size: (f64, f64), scale: f64, max_px: usize) -> (usize, usize) {
    let (w, h) = (size.0.max(0.0) * scale.max(0.0), size.1.max(0.0) * scale.max(0.0));
    let longest = w.max(h);
    let shrink = if longest > max_px as f64 && longest > 0.0 { max_px as f64 / longest } else { 1.0 };
    (((w * shrink).round() as usize).max(1), ((h * shrink).round() as usize).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_sizes_scale_and_cap() {
        assert_eq!(render_size((612.0, 792.0), 1.0, 4096), (612, 792));
        assert_eq!(render_size((612.0, 792.0), 2.0, 4096), (1224, 1584));
        assert_eq!(render_size((612.0, 792.0), 10.0, 1000), (773, 1000));
        assert_eq!(render_size((792.0, 612.0), 10.0, 1000), (1000, 773));
        // Degenerate pages still give a 1×1 picture rather than nothing.
        assert_eq!(render_size((0.0, 0.0), 2.0, 100), (1, 1));
        assert_eq!(render_size((10.0, 10.0), -1.0, 100), (1, 1));
    }

    #[test]
    fn whole_words() {
        let t: Vec<u16> = "cat concat cat_ x-cat".encode_utf16().collect();
        assert!(is_whole_word(&t, 0, 3));
        assert!(!is_whole_word(&t, 7, 3), "inside concat");
        assert!(!is_whole_word(&t, 11, 3), "before an underscore");
        assert!(is_whole_word(&t, 18, 3), "after a hyphen, at the end");
        assert!(!is_whole_word(&t, 18, 9));
        assert!(!is_whole_word(&t, 0, 0));
        // A match that starts or ends with a non-word character stands alone.
        assert!(is_whole_word(&t, 17, 4));
    }

    #[test]
    fn pixels_read_back() {
        let b = Bitmap { width: 2, height: 1, rgba: vec![1, 2, 3, 4, 5, 6, 7, 8] };
        assert_eq!(b.pixel(1, 0), [5, 6, 7, 8]);
    }
}
