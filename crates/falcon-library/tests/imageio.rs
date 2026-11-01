//! ImageIO round trips: pictures encoded by macOS's own HEIC, HEIF
//! sequence and AVIF encoders decode through `falcon_library::imageio`
//! upright, at full size, with their frame timing.
#![cfg(target_os = "macos")]

use falcon_library::imageio;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFDictionary, CFString, CFURL};
use objc2_core_graphics::{CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_image_io::CGImageDestination;
use std::path::Path;

/// A picture and the properties to write it with.
type Frame = (objc2_core_foundation::CFRetained<CGImage>, Retained<NSDictionary<NSString, AnyObject>>);

/// A `w` x `h` opaque picture: red rising left to right, `blue` everywhere.
fn picture(w: usize, h: usize, blue: u8) -> objc2_core_foundation::CFRetained<CGImage> {
    let mut px = vec![255u8; w * h * 4];
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p[0] = ((i % w) * 255 / (w - 1)) as u8;
        p[1] = 0;
        p[2] = blue;
    }
    let space = CGColorSpace::new_device_rgb().unwrap();
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    // SAFETY: the buffer is w*h*4 bytes and outlives the context.
    let ctx = unsafe { CGBitmapContextCreate(px.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), info) }.unwrap();
    CGBitmapContextCreateImage(Some(&ctx)).unwrap()
}

fn dict(pairs: &[(&str, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| NSString::from_str(k)).collect();
    let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| &**v).collect();
    NSDictionary::from_slices(&keys, &values)
}

/// Any object as an `AnyObject`, to sit among a dictionary's values.
fn obj<T: objc2::Message>(r: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an AnyObject.
    unsafe { Retained::cast_unchecked(r) }
}

fn num(v: f64) -> Retained<AnyObject> {
    obj(NSNumber::new_f64(v))
}

/// Writes `frames` (each with its properties) as a file of type `uti`.
/// `false` when this system has no encoder for it.
fn encode(path: &Path, uti: &str, frames: &[Frame]) -> bool {
    let url = CFURL::from_file_path(path).unwrap();
    // SAFETY: owned arguments; no options.
    let Some(dest) = (unsafe { CGImageDestination::with_url(&url, &CFString::from_str(uti), frames.len(), None) }) else { return false };
    for (img, props) in frames {
        // SAFETY: NSDictionary is toll-free bridged to CFDictionary.
        let props: &CFDictionary = unsafe { &*(Retained::as_ptr(props).cast::<CFDictionary>()) };
        unsafe { dest.add_image(img, Some(props)) };
    }
    unsafe { dest.finalize() }
}

#[test]
fn heic_decodes_upright_at_full_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("turned.heic");
    let props = dict(&[("Orientation", num(6.0))]);
    assert!(encode(&path, "public.heic", &[(picture(64, 32, 0), props)]), "macOS writes HEIC");
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
    if !encode(&path, "public.avif", &[(picture(48, 24, 200), dict(&[]))]) {
        eprintln!("skipped: this macOS has no AVIF encoder");
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
    let frame = |delay: f64| dict(&[("{HEICS}", obj(dict(&[("DelayTime", num(delay))])))]);
    let frames = [(picture(40, 20, 0), frame(0.1)), (picture(40, 20, 120), frame(0.3)), (picture(40, 20, 240), frame(0.3))];
    assert!(encode(&path, "public.heics", &frames), "macOS writes HEIF sequences");
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
