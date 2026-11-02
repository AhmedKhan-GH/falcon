//! The preview picture inside the binary files of EPS, Illustrator (PostScript
//! flavour), InDesign, Blender, SketchUp and AutoCAD. Used at import when
//! QuickLook has no render (no generator for the format is installed).
//!
//! Every file here is untrusted. Reads go through the helpers below: a bounded
//! prefix, a checked range, or a linear scan that stops after [`MAX_SCAN`]
//! bytes and [`MAX_HITS`] pictures. Pictures are decoded with size and memory
//! limits, and anything malformed means "no preview" (`None`), never a panic.

mod blend;
mod dwg;
mod eps;
mod indesign;
mod skp;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::ControlFlow;
use std::path::Path;

use image::DynamicImage;

#[allow(unused_imports)]
pub(crate) use crate::zip_preview::decode;

/// The most bytes of one embedded picture or text block read.
#[allow(dead_code)]
pub(crate) const MAX_ITEM: usize = 16 << 20;
/// The most bytes a scan for an embedded picture reads from one file.
#[allow(dead_code)]
pub(crate) const MAX_SCAN: u64 = 1 << 30;
/// The most embedded pictures a scan offers before it stops.
#[allow(dead_code)]
pub(crate) const MAX_HITS: usize = 16;
/// The most bytes of the head of a PostScript or Blender file read.
#[allow(dead_code)]
pub(crate) const HEAD: u64 = 16 << 20;

/// The extensions this module reads a preview from.
pub fn handles(ext: &str) -> bool {
    matches!(ext, "eps" | "ai" | "ait" | "indd" | "indt" | "blend" | "skp" | "dwg")
}

/// The preview picture of the file at `path`; `None` when the format has none
/// or the file is malformed.
pub fn preview(path: &Path, ext: &str) -> Option<DynamicImage> {
    let file = File::open(path).ok()?;
    match ext {
        "eps" | "ai" | "ait" => eps::read(file, path),
        "indd" | "indt" => indesign::read(file, path),
        "blend" => blend::read(file, path),
        "skp" => skp::read(file, path),
        "dwg" => dwg::read(file, path),
        _ => None,
    }
}

/// Up to `max` bytes from the current position of `r`.
#[allow(dead_code)]
pub(crate) fn read_head<R: Read>(r: &mut R, max: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    r.take(max).read_to_end(&mut out).ok()?;
    Some(out)
}

/// `len` bytes at `offset`, when that range is inside a file of `file_len`
/// bytes and no bigger than `cap`. Nothing is allocated before those checks.
#[allow(dead_code)]
pub(crate) fn read_at(file: &mut File, file_len: u64, offset: u64, len: u64, cap: usize) -> Option<Vec<u8>> {
    if len == 0 || len > cap as u64 || offset.checked_add(len)? > file_len {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut out = vec![0u8; len as usize];
    file.read_exact(&mut out).ok()?;
    Some(out)
}

/// The little-endian `u32` at `at`, when it is inside `d`.
#[allow(dead_code)]
pub(crate) fn le32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// Where `needle` first starts in `hay`.
#[allow(dead_code)]
pub(crate) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Offers `visit` each stretch of `r` that starts with `open` and ends with
/// the next `close`, markers included, in order. When an opening comes again
/// before the close (a cut-off picture), the stretch starts at the last one.
///
/// Linear in the bytes read: at most [`MAX_SCAN`] are read, at most
/// [`MAX_ITEM`] held, a stretch that outgrows that is dropped, and the scan
/// ends after [`MAX_HITS`] offers or when `visit` breaks.
#[allow(dead_code)]
pub(crate) fn scan<R: Read>(mut r: R, open: &[u8], close: &[u8], mut visit: impl FnMut(&[u8]) -> ControlFlow<()>) {
    if open.is_empty() || close.is_empty() {
        return;
    }
    let mut chunk = vec![0u8; 1 << 20];
    let mut window: Vec<u8> = Vec::new();
    // `window` starts with `open`, and has no `close` before `searched`.
    let mut opened = false;
    let mut searched = 0;
    let mut hits = 0;
    let mut total = 0u64;
    loop {
        loop {
            if !opened {
                let Some(s) = find(&window, open) else {
                    let keep = open.len() - 1;
                    if window.len() > keep {
                        window.drain(..window.len() - keep);
                    }
                    break;
                };
                window.drain(..s);
                opened = true;
                searched = open.len();
            }
            let Some(e) = find(&window[searched..], close) else {
                if window.len() > MAX_ITEM + close.len() {
                    // Too long to be a picture: look for the next opening.
                    window.drain(..window.len() - (open.len() - 1));
                    opened = false;
                } else {
                    searched = window.len().saturating_sub(close.len() - 1).max(open.len());
                }
                break;
            };
            let end = searched + e + close.len();
            let start = window[..end].windows(open.len()).rposition(|w| w == open).unwrap_or(0);
            if end - start <= MAX_ITEM {
                hits += 1;
                if visit(&window[start..end]).is_break() || hits >= MAX_HITS {
                    return;
                }
            }
            window.drain(..end);
            opened = false;
        }
        if total >= MAX_SCAN {
            return;
        }
        match r.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                window.extend_from_slice(&chunk[..n]);
                total += n as u64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(data: &[u8], open: &[u8], close: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        scan(std::io::Cursor::new(data), open, close, |h| {
            out.push(h.to_vec());
            ControlFlow::Continue(())
        });
        out
    }

    #[test]
    fn scan_finds_stretches_with_their_markers() {
        assert_eq!(hits(b"xx<a>1</a>yy<a>22</a>", b"<a>", b"</a>"), vec![b"<a>1</a>".to_vec(), b"<a>22</a>".to_vec()]);
        assert!(hits(b"", b"<a>", b"</a>").is_empty());
        assert!(hits(b"<a>no close", b"<a>", b"</a>").is_empty());
        assert!(hits(b"</a>no open", b"<a>", b"</a>").is_empty());
    }

    #[test]
    fn scan_starts_a_cut_off_stretch_at_the_last_opening() {
        assert_eq!(hits(b"<a>cut<a>whole</a>", b"<a>", b"</a>"), vec![b"<a>whole</a>".to_vec()]);
    }

    #[test]
    fn scan_finds_markers_across_read_chunks() {
        let mut d = vec![b'.'; (1 << 20) - 2];
        d.extend(b"<a>");
        d.extend(vec![b'x'; (1 << 20) - 3]);
        d.extend(b"</a>");
        let got = hits(&d, b"<a>", b"</a>");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].len(), 3 + (1 << 20) - 3 + 4);
    }

    #[test]
    fn scan_drops_an_overlong_stretch_and_finds_the_next() {
        let mut d = b"<a>".to_vec();
        d.resize(MAX_ITEM + (3 << 20), b'x');
        d.extend(b"<a>ok</a>");
        assert_eq!(hits(&d, b"<a>", b"</a>"), vec![b"<a>ok</a>".to_vec()]);
    }

    #[test]
    fn scan_stops_at_the_hit_limit_and_when_told() {
        let d = b"<a></a>".repeat(MAX_HITS * 3);
        assert_eq!(hits(&d, b"<a>", b"</a>").len(), MAX_HITS);
        let mut n = 0;
        scan(std::io::Cursor::new(&d), b"<a>", b"</a>", |_| {
            n += 1;
            ControlFlow::Break(())
        });
        assert_eq!(n, 1);
    }

    #[test]
    fn scan_of_endless_openings_finishes_quickly() {
        // Would be quadratic if each opening searched the window again.
        let d = b"<a>".repeat(8 << 20);
        let t = std::time::Instant::now();
        assert!(hits(&d, b"<a>", b"</a>").is_empty());
        assert!(t.elapsed().as_secs() < 30, "took {:?}", t.elapsed());
    }

    #[test]
    fn read_at_refuses_ranges_outside_the_file_or_over_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"0123456789").unwrap();
        let mut f = File::open(&p).unwrap();
        assert_eq!(read_at(&mut f, 10, 2, 3, 100).unwrap(), b"234");
        assert!(read_at(&mut f, 10, 8, 3, 100).is_none());
        assert!(read_at(&mut f, 10, u64::MAX, 3, 100).is_none());
        assert!(read_at(&mut f, 10, 0, 0, 100).is_none());
        assert!(read_at(&mut f, 10, 0, 5, 4).is_none());
        // A stated length near the top of memory allocates nothing.
        assert!(read_at(&mut f, u64::MAX, 0, u64::MAX, MAX_ITEM).is_none());
    }

    #[test]
    fn unknown_extensions_are_not_handled() {
        assert!(!handles("pdf") && !handles("mrw") && handles("dwg") && handles("blend"));
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, b"x").unwrap();
        assert!(preview(&p, "txt").is_none());
        assert!(preview(&dir.path().join("absent.eps"), "eps").is_none());
    }
}
