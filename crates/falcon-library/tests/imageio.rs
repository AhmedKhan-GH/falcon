//! ImageIO round trips: pictures written by macOS's own HEIC, HEIF
//! sequence and AVIF encoders decode through `falcon_library::imageio`
//! upright, at full size, with their frame timing.
#![cfg(target_os = "macos")]

use falcon_library::imageio::{self, WriteFrame};
use image::RgbaImage;
use std::path::Path;

/// A `w` x `h` opaque picture: red rising left to right, `blue` everywhere.
fn picture(w: u32, h: u32, blue: u8) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, _| image::Rgba([(x * 255 / (w - 1)) as u8, 0, blue, 255]))
}

fn still(image: &RgbaImage, orientation: u8) -> WriteFrame<'_> {
    WriteFrame { image, orientation, delay: None }
}

#[test]
fn heic_decodes_upright_at_full_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("turned.heic");
    imageio::write(&path, "heic", &[still(&picture(64, 32, 0), 6)]).expect("macOS writes HEIC");
    assert!(imageio::available("heic"));
    let img = imageio::decode(&path).unwrap().into_rgba8();
    // Stored 64 x 32, shown turned a quarter clockwise.
    assert_eq!(img.dimensions(), (32, 64));
    // The stored left edge (dark red) is now the top row.
    let top = img.get_pixel(16, 0).0[0];
    let bottom = img.get_pixel(16, 63).0[0];
    assert!(top < 60 && bottom > 195, "top {top}, bottom {bottom}");
    // A still is not an animation.
    assert!(imageio::Animation::open(&path).unwrap().is_none());
}

#[test]
fn avif_decodes_through_imageio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("still.avif");
    if let Err(e) = imageio::write(&path, "avif", &[still(&picture(48, 24, 200), 1)]) {
        eprintln!("skipped: {e}");
        return;
    }
    assert!(imageio::available("avif"));
    let img = imageio::decode(&path).unwrap().into_rgba8();
    assert_eq!(img.dimensions(), (48, 24));
    let p = img.get_pixel(24, 12).0;
    assert!(p[2] > 170 && p[3] == 255, "{p:?}");
}

/// A HEIF sequence goes through the same path as an animated AVIF (the
/// per-frame `{HEICS}` and `{AVIS}` dictionaries share their keys).
#[test]
fn heif_sequences_give_frames_and_delays() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seq.heics");
    let pics = [picture(40, 20, 0), picture(40, 20, 120), picture(40, 20, 240)];
    let frames: Vec<WriteFrame> = pics.iter().zip([0.1, 0.3, 0.3]).map(|(image, d)| WriteFrame { image, orientation: 1, delay: Some(d) }).collect();
    imageio::write(&path, "heics", &frames).expect("macOS writes HEIF sequences");
    let mut anim = imageio::Animation::open(&path).unwrap().expect("animated");
    assert_eq!(anim.size(), (40, 20));
    // macOS's HEICS encoder stores the last frame with the one before's
    // duration, so the last two are given the same delay.
    assert_eq!(anim.delays(), &[100, 300, 300]);
    let blues: Vec<u8> = (0..3).map(|i| anim.frame(i).unwrap().get_pixel(20, 10).0[2]).collect();
    assert!(blues[0] < 30 && (90..150).contains(&blues[1]) && blues[2] > 210, "{blues:?}");
    // Out of range frames fail rather than panic.
    assert!(anim.frame(7).is_err());
}

#[test]
fn unwritable_formats_say_so() {
    let dir = tempfile::tempdir().unwrap();
    assert!(imageio::write(&dir.path().join("x.png"), "png", &[still(&picture(4, 4, 0), 1)]).is_err());
}

/// An animated AVIF (AVIS) plays through the same code; ImageIO can read
/// but not write AVIS and no AVIF encoder is installed, so this needs a
/// real file: `FALCON_AVIS=/path/to/anim.avif cargo test -p falcon-library
/// --test imageio -- --ignored`.
#[test]
#[ignore = "needs an animated AVIF file in $FALCON_AVIS; none can be generated offline"]
fn animated_avif_plays() {
    let path = std::env::var("FALCON_AVIS").expect("set FALCON_AVIS");
    let mut anim = imageio::Animation::open(Path::new(&path)).unwrap().expect("animated");
    assert!(anim.delays().len() >= 2 && anim.delays().iter().all(|&d| d > 0));
    let (w, h) = anim.size();
    assert_eq!(anim.frame(1).unwrap().dimensions(), (w, h));
}
