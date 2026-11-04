//! Files under Eagle's alias extensions (`.jfif`, `.psdt`, ...) import with
//! a thumbnail and their dimensions, as the same bytes do under the main name.
// Code only macOS calls so far; macOS builds keep the full check.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use falcon_library::import::ImportOptions;
use falcon_library::library::Library;
use falcon_library::thumbnail;
use image::{ImageEncoder, RgbImage};
use std::path::Path;

const W: u32 = 1200;
const H: u32 = 800;

fn ramp() -> RgbImage {
    RgbImage::from_fn(W, H, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 40]))
}

fn jpeg() -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&ramp()).unwrap();
    out
}

fn tiff() -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::tiff::TiffEncoder::new(std::io::Cursor::new(&mut out))
        .write_image(ramp().as_raw(), W, H, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// A raw RGB PSD holding the ramp.
fn psd() -> Vec<u8> {
    let mut f = b"8BPS\0\x01\0\0\0\0\0\0\0\x03".to_vec();
    f.extend(H.to_be_bytes());
    f.extend(W.to_be_bytes());
    f.extend([0, 8, 0, 3]);
    f.extend([0; 12]);
    f.extend([0, 0]);
    let img = ramp();
    for c in 0..3 {
        f.extend(img.pixels().map(|p| p.0[c]));
    }
    f
}

/// Imports `bytes` as `name` into a new library and returns the item's size
/// and its thumbnail's size.
fn import(name: &str, bytes: &[u8]) -> ((u64, u64), Option<(u32, u32)>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
    let src = tmp.path().join(name);
    std::fs::write(&src, bytes).unwrap();
    let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
    let item = lib.item(&id).unwrap().clone();
    let thumb = item.thumbnail_path(lib.root()).and_then(|p| thumbnail::decode(&p).ok()).map(|i| (i.width(), i.height()));
    ((item.width, item.height), thumb)
}

fn check(name: &str, bytes: &[u8]) {
    let (size, thumb) = import(name, bytes);
    assert_eq!(size, (u64::from(W), u64::from(H)), "{name} dimensions");
    let thumb = thumb.unwrap_or_else(|| panic!("{name} has no thumbnail"));
    assert!(thumb.0.max(thumb.1) > 0 && thumb.0 > thumb.1, "{name} thumbnail {thumb:?}");
}

#[test]
fn jfif_is_a_jpeg() {
    check("a.jfif", &jpeg());
}

#[test]
fn jpe_is_a_jpeg() {
    check("a.jpe", &jpeg());
}

#[test]
fn insp_is_a_jpeg() {
    check("a.insp", &jpeg());
}

/// The alias imports exactly as the main name does (both take the size of
/// QuickLook's render when Spotlight has none).
fn same_as(alias: &str, main: &str, bytes: &[u8]) {
    let (size, thumb) = import(main, bytes);
    assert!(size.0 > 0 && size.1 > 0 && thumb.is_some(), "{main} is the reference: {size:?} {thumb:?}");
    assert_eq!(import(alias, bytes), (size, thumb), "{alias} against {main}");
}

#[cfg(target_os = "macos")]
#[test]
fn tif_is_a_tiff() {
    same_as("a.tif", "a.tiff", &tiff());
}

#[cfg(target_os = "macos")]
#[test]
fn psdt_is_a_psd() {
    check("a.psdt", &psd());
}

#[cfg(target_os = "macos")]
#[test]
fn hif_is_a_heic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.heic");
    falcon_library::imageio::write(&path, "heic", &[falcon_library::imageio::WriteFrame { image: &image::DynamicImage::ImageRgb8(ramp()).to_rgba8(), orientation: 1, delay: None }]).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    same_as("a.hif", "a.heic", &bytes);
}

/// The Adobe formats' alias names have no generator to draw a made-up file,
/// so this checks the name QuickLook is given instead.
#[test]
fn ait_and_indt_reach_quicklook_under_their_main_names() {
    let tmp = tempfile::tempdir().unwrap();
    for (alias, main) in [("a.ait", "a.ai"), ("a.indt", "a.indd"), ("a.psdt", "a.psd"), ("a.jfif", "a.jpg"), ("a.m2ts", "a.mts")] {
        let src: &Path = &tmp.path().join(alias);
        std::fs::write(src, b"x").unwrap();
        let named = falcon_library::alias::renamed(src).unwrap();
        assert_eq!(named.path().file_name().unwrap(), main);
    }
}

/// A transport stream from `FALCON_M2TS_SAMPLE` imports under both names.
#[test]
fn m2ts_is_an_mts() {
    let Ok(sample) = std::env::var("FALCON_M2TS_SAMPLE") else { return };
    let bytes = std::fs::read(sample).unwrap();
    for name in ["a.mts", "a.m2ts"] {
        let (size, thumb) = import(name, &bytes);
        assert!(size.0 > 0 && size.1 > 0 && thumb.is_some(), "{name}: {size:?} {thumb:?}");
    }
}
