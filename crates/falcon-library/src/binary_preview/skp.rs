//! SketchUp models. Older files are a proprietary stream with PNG images
//! embedded in it (the model thumbnail first, then material textures);
//! newer ones are zip archives with a thumbnail entry.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::ops::ControlFlow;
use std::path::Path;

use image::DynamicImage;

use super::{decode, scan};

const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
const IEND: &[u8] = b"IEND\xAE\x42\x60\x82";
/// A picture smaller than this on its short side is only a fallback.
const MIN_SIDE: u32 = 64;

/// A zip goes to the zip reader. Otherwise the first complete PNG at least
/// `MIN_SIDE` on its short side wins; failing that, the first smaller one.
pub(super) fn read(mut file: File, path: &Path) -> Option<DynamicImage> {
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    if &magic == b"PK\x03\x04" {
        return crate::zip_preview::preview(path, "skp");
    }
    let mut small = None;
    let mut found = None;
    scan(BufReader::new(file), SIG, IEND, |hit| {
        let Some(img) = decode(hit) else {
            return ControlFlow::Continue(());
        };
        if img.width().min(img.height()) >= MIN_SIDE {
            found = Some(img);
            ControlFlow::Break(())
        } else {
            small.get_or_insert(img);
            ControlFlow::Continue(())
        }
    });
    found.or(small)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    fn size(dir: &Path, name: &str, data: &[u8]) -> Option<(u32, u32)> {
        let p = dir.join(name);
        std::fs::write(&p, data).unwrap();
        read(File::open(&p).unwrap(), &p).map(|i| (i.width(), i.height()))
    }

    #[test]
    fn first_complete_png_wins() {
        let d = tempfile::tempdir().unwrap();
        let mut a = b"\xff\xfe\xff\x0eSketchUp Model".to_vec();
        a.extend(vec![1u8; 100]);
        a.extend(png(72, 68));
        a.extend(png(90, 90));
        assert_eq!(size(d.path(), "a.skp", &a), Some((72, 68)));
    }

    #[test]
    fn cut_png_then_whole_one() {
        let d = tempfile::tempdir().unwrap();
        let cut = png(80, 80);
        let mut a = vec![0u8; 20];
        a.extend(&cut[..cut.len() - 20]);
        a.extend(png(70, 66));
        assert_eq!(size(d.path(), "a.skp", &a), Some((70, 66)));
    }

    #[test]
    fn tiny_only_and_tiny_then_large() {
        let d = tempfile::tempdir().unwrap();
        let mut t = vec![0u8; 50];
        t.extend(png(8, 8));
        assert_eq!(size(d.path(), "t.skp", &t), Some((8, 8)));
        t.extend(png(100, 100));
        assert_eq!(size(d.path(), "m.skp", &t), Some((100, 100)));
        // Long and thin counts by its short side.
        let mut s = png(500, 10);
        s.extend(png(20, 20));
        assert_eq!(size(d.path(), "s.skp", &s), Some((500, 10)));
    }

    #[test]
    fn malformed_input_gives_none() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(size(d.path(), "e.skp", b""), None);
        assert_eq!(size(d.path(), "n.skp", &[0u8; 3000]), None);
        let mut junk = SIG.to_vec();
        junk.extend(b"junk junk");
        junk.extend(IEND);
        assert_eq!(size(d.path(), "j.skp", &junk), None);
        assert_eq!(size(d.path(), "pk.skp", b"PK\x03\x04garbage"), None);
        assert_eq!(size(d.path(), "pk2.skp", b"PK"), None);
    }

    #[test]
    fn zip_layout_uses_the_thumbnail_entry() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("z.skp");
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        z.start_file("Thumbnails/thumbnail.png", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(&png(30, 20)).unwrap();
        z.finish().unwrap();
        let img = read(File::open(&p).unwrap(), &p).unwrap();
        assert_eq!((img.width(), img.height()), (30, 20));
    }
}
