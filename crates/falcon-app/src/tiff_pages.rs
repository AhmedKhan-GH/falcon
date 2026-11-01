//! Pages of multi-page TIFFs: counting them, decoding one, and the viewer's
//! page state. Eagle's own TIFF viewer decodes only the first page; falcon
//! steps through the rest. Page 0 is shown as for any image (see `loader`).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{TextureHandle, Vec2};
use image::{RgbaImage, imageops};
use tiff::ColorType;
use tiff::decoder::{Decoder, DecodingResult};

/// Pages counted at most, so a damaged chain of directories ends.
const MAX_PAGES: usize = 10_000;

/// How many images (IFDs) the TIFF at `path` holds; `None` if it can't be read.
pub fn page_count(path: &Path) -> Option<usize> {
    let mut d = Decoder::new(BufReader::new(File::open(path).ok()?)).ok()?;
    let mut n = 1;
    while n < MAX_PAGES && d.more_images() {
        if d.next_image().is_err() {
            break;
        }
        n += 1;
    }
    Some(n)
}

/// Page `page` (0-based) as RGBA8, shrunk so neither side exceeds `max_side`.
/// `None` for a colour type that isn't handled or a page that can't be read.
pub fn decode_page(path: &Path, page: usize, max_side: u32) -> Option<RgbaImage> {
    let mut d = Decoder::new(BufReader::new(File::open(path).ok()?)).ok()?;
    d.seek_to_image(page).ok()?;
    let (w, h) = d.dimensions().ok()?;
    let color = d.colortype().ok()?;
    let data = match d.read_image().ok()? {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.into_iter().map(|x| (x >> 8) as u8).collect(),
        _ => return None,
    };
    let rgba = to_rgba(color, &data, (w * h) as usize)?;
    let img = RgbaImage::from_raw(w, h, rgba)?;
    if w > max_side || h > max_side {
        let (nw, nh) = if w >= h {
            (max_side, ((h as u64 * max_side as u64) / w as u64).max(1) as u32)
        } else {
            (((w as u64 * max_side as u64) / h as u64).max(1) as u32, max_side)
        };
        return Some(imageops::resize(&img, nw, nh, imageops::FilterType::Triangle));
    }
    Some(img)
}

/// Samples of 8 bits (16-bit already narrowed) to RGBA8 for `pixels` pixels.
fn to_rgba(color: ColorType, d: &[u8], pixels: usize) -> Option<Vec<u8>> {
    let (channels, bits) = match color {
        ColorType::Gray(b) => (1, b),
        ColorType::GrayA(b) => (2, b),
        ColorType::RGB(b) => (3, b),
        ColorType::RGBA(b) => (4, b),
        ColorType::CMYK(b) => (4, b),
        _ => return None,
    };
    if !matches!(bits, 8 | 16) || (matches!(color, ColorType::CMYK(_)) && bits != 8) || d.len() < pixels * channels {
        return None;
    }
    let mut out = Vec::with_capacity(pixels * 4);
    for px in d.chunks_exact(channels).take(pixels) {
        match color {
            ColorType::Gray(_) => out.extend_from_slice(&[px[0], px[0], px[0], 255]),
            ColorType::GrayA(_) => out.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
            ColorType::RGB(_) => out.extend_from_slice(&[px[0], px[1], px[2], 255]),
            ColorType::RGBA(_) => out.extend_from_slice(px),
            _ => {
                let k = 255 - px[3] as u32;
                let c = |v: u8| ((255 - v as u32) * k / 255) as u8;
                out.extend_from_slice(&[c(px[0]), c(px[1]), c(px[2]), 255]);
            }
        }
    }
    Some(out)
}

/// The page control's state for the item in the viewer.
#[derive(Default)]
pub struct TiffPages {
    /// The item the rest belongs to.
    pub id: String,
    /// Pages in the file, once counted.
    pub count: Option<usize>,
    /// The page shown, from 0.
    pub page: usize,
    /// The size of the page picture drawn while a page above 0 is shown.
    pub natural: Option<Vec2>,
    /// The last page picture drawn, kept up while the next one loads.
    pub shown: Option<TextureHandle>,
    counting: Option<Receiver<Option<usize>>>,
}

impl TiffPages {
    /// Starts over for item `id`: page 0, the count asked for on a worker
    /// (`path` is read there, so a slow disk never blocks the UI).
    pub fn open(&mut self, id: &str, path: std::path::PathBuf, ctx: &eframe::egui::Context) {
        *self = TiffPages { id: id.to_string(), ..Default::default() };
        let (tx, rx) = channel();
        self.counting = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(page_count(&path));
            ctx.request_repaint();
        });
    }

    /// Takes the count in when the worker has it.
    pub fn poll(&mut self) {
        if let Some(rx) = &self.counting
            && let Ok(n) = rx.try_recv()
        {
            self.count = Some(n.unwrap_or(1));
            self.counting = None;
        }
    }

    /// The page count if there is more than one page.
    pub fn pages(&self) -> Option<usize> {
        self.count.filter(|n| *n > 1)
    }

    /// Moves `step` pages, stopping at the ends; true if the page changed.
    pub fn step(&mut self, step: isize) -> bool {
        let Some(n) = self.pages() else { return false };
        let to = (self.page as isize + step).clamp(0, n as isize - 1) as usize;
        let changed = to != self.page;
        self.page = to;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::encoder::{TiffEncoder, colortype};

    fn write(dir: &Path, pages: &[(u32, u32, Option<u8>)]) -> std::path::PathBuf {
        let path = dir.join("t.tif");
        let mut f = std::io::BufWriter::new(File::create(&path).unwrap());
        let mut enc = TiffEncoder::new(&mut f).unwrap();
        for &(w, h, gray) in pages {
            match gray {
                Some(g) => enc.write_image::<colortype::Gray8>(w, h, &vec![g; (w * h) as usize]).unwrap(),
                None => {
                    let data: Vec<u8> = (0..w * h).flat_map(|i| [(i % w) as u8, (i / w) as u8, 7]).collect();
                    enc.write_image::<colortype::RGB8>(w, h, &data).unwrap()
                }
            }
        }
        path
    }

    #[test]
    fn counts_and_decodes_pages() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &[(4, 3, None), (6, 2, Some(90)), (2, 2, None)]);
        assert_eq!(page_count(&path), Some(3));
        let p0 = decode_page(&path, 0, 100).unwrap();
        assert_eq!((p0.width(), p0.height()), (4, 3));
        assert_eq!(p0.get_pixel(2, 1).0, [2, 1, 7, 255]);
        let p1 = decode_page(&path, 1, 100).unwrap();
        assert_eq!((p1.width(), p1.height()), (6, 2));
        assert_eq!(p1.get_pixel(5, 1).0, [90, 90, 90, 255]);
        let p2 = decode_page(&path, 2, 100).unwrap();
        assert_eq!((p2.width(), p2.height()), (2, 2));
        assert_eq!(p2.get_pixel(1, 1).0, [1, 1, 7, 255]);
        assert!(decode_page(&path, 3, 100).is_none());
    }

    #[test]
    fn shrinks_to_max_side() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &[(2, 2, None), (40, 20, Some(200))]);
        let p = decode_page(&path, 1, 10).unwrap();
        assert_eq!((p.width(), p.height()), (10, 5));
        assert_eq!(p.get_pixel(3, 3).0, [200, 200, 200, 255]);
        let tall = write(dir.path(), &[(20, 40, Some(1))]);
        let p = decode_page(&tall, 0, 10).unwrap();
        assert_eq!((p.width(), p.height()), (5, 10));
    }

    #[test]
    fn single_page_and_non_tiff() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &[(3, 3, None)]);
        assert_eq!(page_count(&path), Some(1));
        let other = dir.path().join("x.txt");
        std::fs::write(&other, "not a tiff at all").unwrap();
        assert_eq!(page_count(&other), None);
        assert!(decode_page(&other, 0, 10).is_none());
        assert_eq!(page_count(&dir.path().join("missing.tif")), None);
    }

    #[test]
    fn converts_colour_types() {
        assert_eq!(to_rgba(ColorType::GrayA(8), &[10, 20], 1), Some(vec![10, 10, 10, 20]));
        assert_eq!(to_rgba(ColorType::CMYK(8), &[0, 0, 0, 0], 1), Some(vec![255, 255, 255, 255]));
        assert_eq!(to_rgba(ColorType::CMYK(8), &[0, 0, 0, 255], 1), Some(vec![0, 0, 0, 255]));
        assert!(to_rgba(ColorType::Gray(1), &[1], 1).is_none());
    }

    #[test]
    fn steps_stay_in_range() {
        let mut t = TiffPages { count: Some(3), ..Default::default() };
        assert!(!t.step(-1));
        assert!(t.step(1) && t.step(1));
        assert!(!t.step(1));
        assert_eq!(t.page, 2);
        let mut single = TiffPages { count: Some(1), ..Default::default() };
        assert!(!single.step(1));
    }
}
