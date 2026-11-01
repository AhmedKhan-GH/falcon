//! Pictures macOS decodes through ImageIO: AVIF, HEIC and HEIF, JPEG XL
//! (macOS 14 and later) and their animated forms (AVIF and HEIF
//! sequences). The `image` crate reads none of them, so on macOS these
//! decode here at full resolution, turned by their orientation, and
//! animated files hand out their frames and delays. Elsewhere every call
//! fails and callers fall back to QuickLook or a placeholder.
//!
//! Which formats the running system reads is asked of ImageIO
//! (`CGImageSourceCopyTypeIdentifiers`), so JPEG XL on macOS 13 reports
//! itself unsupported instead of failing every decode.

use image::metadata::Orientation;
use image::{DynamicImage, RgbaImage};
use std::path::Path;

use crate::thumbnail::ThumbError;

/// The extensions this module may decode, with the type ImageIO knows
/// each as.
const TYPES: [(&str, &str); 7] = [
    ("avif", "public.avif"),
    ("avifs", "public.avis"),
    ("heic", "public.heic"),
    ("hif", "public.heic"),
    ("heif", "public.heif"),
    ("heics", "public.heics"),
    ("jxl", "public.jpeg-xl"),
];

/// The ImageIO type of a (lower-case) extension this module handles.
pub fn type_of(ext: &str) -> Option<&'static str> {
    TYPES.iter().find(|(e, _)| *e == ext).map(|(_, t)| *t)
}

/// Whether `ext` is one of the formats this module decodes on a system that
/// supports it (see [`available`] for whether this one does).
pub fn handles(ext: &str) -> bool {
    type_of(&ext.to_lowercase()).is_some()
}

/// Whether the running system's ImageIO reads `ext` (JPEG XL needs
/// macOS 14). Always false off macOS.
pub fn available(ext: &str) -> bool {
    type_of(&ext.to_lowercase()).is_some_and(|t| readable_types().iter().any(|r| r == t))
}

/// The orientation an ImageIO `Orientation` property (the EXIF values 1–8)
/// stands for; anything else leaves the picture as stored.
pub fn orientation(value: i64) -> Orientation {
    u8::try_from(value).ok().and_then(Orientation::from_exif).unwrap_or(Orientation::NoTransforms)
}

/// A frame's delay in milliseconds from its ImageIO properties: the
/// unclamped delay when the file gives one, else the clamped one, in
/// seconds. Browsers' rule for tiny delays is applied by the player.
pub fn delay_ms(unclamped: Option<f64>, clamped: Option<f64>) -> u32 {
    let secs = unclamped.filter(|s| s.is_finite() && *s > 0.0).or(clamped).unwrap_or(0.0);
    if secs.is_finite() && secs > 0.0 { (secs * 1000.0).round().min(f64::from(u32::MAX)) as u32 } else { 0 }
}

/// Whether a file of `count` images is an animation: several images, the
/// first carrying a frame delay. An image collection (a HEIF burst, or a
/// HEIC with its depth map) has several images but no timing, and stays a
/// still.
pub fn is_animated(count: usize, first_has_delay: bool) -> bool {
    count >= 2 && first_has_delay
}

/// The size a picture of `w` x `h` stored pixels shows at once turned.
pub fn oriented_size(w: u32, h: u32, o: Orientation) -> (u32, u32) {
    match o {
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH => (h, w),
        _ => (w, h),
    }
}

/// Straight alpha from premultiplied RGBA, in place.
fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
            }
        }
    }
}

/// Decodes the file's main picture at full resolution, turned upright.
pub fn decode(path: &Path) -> Result<DynamicImage, ThumbError> {
    imp::decode(path)
}

/// An animated AVIF or HEIF sequence, read frame by frame. Each frame is
/// a whole picture (sequence frames don't need compositing).
pub struct Animation {
    inner: imp::Source,
    size: (u32, u32),
    delays: Vec<u32>,
}

impl Animation {
    /// Opens `path`; `Ok(None)` when it holds a single picture.
    pub fn open(path: &Path) -> Result<Option<Animation>, String> {
        let Some((inner, delays)) = imp::Source::open(path)? else { return Ok(None) };
        let size = inner.size(0);
        Ok(Some(Animation { inner, size, delays }))
    }

    /// The first frame's size, upright.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Each frame's delay in milliseconds.
    pub fn delays(&self) -> &[u32] {
        &self.delays
    }

    /// Frame `i`, upright.
    pub fn frame(&mut self, i: usize) -> Result<RgbaImage, String> {
        self.inner.frame(i)
    }
}

/// One picture to write with [`write`].
#[derive(Clone, Copy)]
pub struct WriteFrame<'a> {
    pub image: &'a RgbaImage,
    /// The EXIF orientation to record (1 leaves it upright).
    pub orientation: u8,
    /// For a HEIF sequence: how long the frame shows, in seconds.
    pub delay: Option<f64>,
}

/// Writes `frames` to `path` as the ImageIO format of `ext` (macOS can
/// write AVIF, HEIC and HEIF sequences, not AVIF sequences or JPEG XL).
/// Several frames with delays make a HEIF sequence.
pub fn write(path: &Path, ext: &str, frames: &[WriteFrame]) -> Result<(), String> {
    let uti = type_of(&ext.to_lowercase()).ok_or_else(|| format!("ImageIO doesn't handle .{ext}"))?;
    imp::write(path, uti, frames)
}

fn readable_types() -> &'static [String] {
    static TYPES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    TYPES.get_or_init(imp::readable_types)
}

#[cfg(target_os = "macos")]
mod imp {
    //! Every call below follows the same two rules: receivers and
    //! arguments are live `CFRetained`/`Retained` values, and the
    //! dictionaries ImageIO returns are read through their toll-free
    //! bridge to `NSDictionary` (a `CFDictionary` *is* an `NSDictionary`).

    use super::{Orientation, Path, RgbaImage, ThumbError, delay_ms, is_animated, orientation, unpremultiply};
    use image::DynamicImage;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_core_foundation::{CFArray, CFDictionary, CFRetained, CFString, CFURL, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
        kCGColorSpaceSRGB,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
    use objc2_image_io::{CGImageDestination, CGImageSource};

    fn as_ns(d: &CFDictionary) -> &NSDictionary<AnyObject, AnyObject> {
        // SAFETY: CFDictionary and NSDictionary are toll-free bridged; the
        // reference keeps the lifetime of `d`.
        unsafe { &*(d as *const CFDictionary).cast::<NSDictionary<AnyObject, AnyObject>>() }
    }

    fn get(d: &NSDictionary<AnyObject, AnyObject>, key: &str) -> Option<Retained<AnyObject>> {
        let key = NSString::from_str(key);
        d.objectForKey(&key)
    }

    fn number(d: &NSDictionary<AnyObject, AnyObject>, key: &str) -> Option<f64> {
        get(d, key)?.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
    }

    /// The frame delay found in any of a frame's per-format dictionaries
    /// (`{AVIS}`, `{HEICS}`, `{GIF}` and so on all use the same keys).
    fn frame_delay(props: &NSDictionary<AnyObject, AnyObject>) -> Option<u32> {
        let mut found = None;
        for key in props.allKeys() {
            let Some(value) = props.objectForKey(&key) else { continue };
            let Ok(sub) = value.downcast::<NSDictionary>() else { continue };
            // SAFETY: an NSDictionary's keys and values are objects.
            let sub: &NSDictionary<AnyObject, AnyObject> = unsafe { &*(Retained::as_ptr(&sub)).cast() };
            let (unclamped, clamped) = (number(sub, "UnclampedDelayTime"), number(sub, "DelayTime"));
            if unclamped.is_some() || clamped.is_some() {
                found = Some(delay_ms(unclamped, clamped));
                break;
            }
        }
        found
    }

    fn open_source(path: &Path) -> Result<CFRetained<CGImageSource>, String> {
        let url = CFURL::from_file_path(path).ok_or("not a file path")?;
        // SAFETY: owned; no options.
        unsafe { CGImageSource::with_url(&url, None) }.ok_or_else(|| "ImageIO can't read the file".to_string())
    }

    fn properties(src: &CGImageSource, i: usize) -> Option<CFRetained<CFDictionary>> {
        // SAFETY: owned; no options.
        unsafe { src.properties_at_index(i, None) }
    }

    fn orientation_at(src: &CGImageSource, i: usize) -> Orientation {
        properties(src, i).and_then(|p| number(as_ns(&p), "Orientation")).map_or(Orientation::NoTransforms, |v| orientation(v as i64))
    }

    /// Image `i` of `src` as straight-alpha RGBA in sRGB, upright.
    fn decode_at(src: &CGImageSource, i: usize) -> Result<RgbaImage, String> {
        // SAFETY: owned; no options.
        let image = unsafe { src.image_at_index(i, None) }.ok_or("ImageIO decoded no picture")?;
        let mut img = draw(&image)?;
        let o = orientation_at(src, i);
        if o != Orientation::NoTransforms {
            let mut d = DynamicImage::ImageRgba8(img);
            d.apply_orientation(o);
            img = d.into_rgba8();
        }
        Ok(img)
    }

    /// Draws a CGImage into an RGBA buffer of its own size.
    fn draw(image: &CGImage) -> Result<RgbaImage, String> {
        let (w, h) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
        if w == 0 || h == 0 || w.saturating_mul(h) > 1 << 30 {
            return Err(format!("a {w} x {h} picture can't be decoded"));
        }
        let mut rgba = vec![0u8; w * h * 4];
        // SAFETY: kCGColorSpaceSRGB is a constant CFString.
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).or_else(CGColorSpace::new_device_rgb).ok_or("no RGB colour space")?;
        let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
        // SAFETY: `rgba` is `w * h * 4` bytes and outlives the context,
        // dropped below; 8 bits per component with `w * 4` bytes per row
        // matches that buffer exactly.
        let ctx = unsafe { CGBitmapContextCreate(rgba.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), info) }.ok_or("no bitmap context")?;
        CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(image));
        drop(ctx);
        unpremultiply(&mut rgba);
        RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or_else(|| "bad buffer".to_string())
    }

    pub fn decode(path: &Path) -> Result<DynamicImage, ThumbError> {
        let src = open_source(path).map_err(ThumbError)?;
        // SAFETY: owned.
        let primary = unsafe { src.primary_image_index() };
        decode_at(&src, primary).map(DynamicImage::ImageRgba8).map_err(ThumbError)
    }

    pub fn readable_types() -> Vec<String> {
        // SAFETY: no arguments; returns a retained CFArray of CFStrings.
        let types: CFRetained<CFArray> = unsafe { CGImageSource::type_identifiers() };
        // SAFETY: toll-free bridged, holding only strings.
        let types: &NSArray<NSString> = unsafe { &*(CFRetained::as_ptr(&types).as_ptr() as *const NSArray<NSString>) };
        types.iter().map(|s| s.to_string()).collect()
    }

    /// Any object as an `AnyObject`, to sit among a dictionary's values.
    fn obj<T: objc2::Message>(r: Retained<T>) -> Retained<AnyObject> {
        // SAFETY: every Objective-C object is an AnyObject.
        unsafe { Retained::cast_unchecked(r) }
    }

    fn dict(pairs: Vec<(&str, Retained<AnyObject>)>) -> Retained<NSDictionary<NSString, AnyObject>> {
        let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| NSString::from_str(k)).collect();
        let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| &**v).collect();
        NSDictionary::from_slices(&keys, &values)
    }

    /// A CGImage holding a copy of `img`.
    fn cg_image(img: &RgbaImage) -> Result<CFRetained<CGImage>, String> {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let mut px = img.as_raw().clone();
        // Premultiply, as the bitmap context stores it.
        for p in px.as_chunks_mut::<4>().0 {
            let a = u32::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * a + 127) / 255) as u8;
            }
        }
        // SAFETY: kCGColorSpaceSRGB is a constant CFString.
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or("no sRGB colour space")?;
        let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
        // SAFETY: `px` is `w * h * 4` bytes and outlives the context; the
        // image made from it is a copy.
        let ctx = unsafe { CGBitmapContextCreate(px.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), info) }.ok_or("no bitmap context")?;
        CGBitmapContextCreateImage(Some(&ctx)).ok_or_else(|| "no image".to_string())
    }

    pub fn write(path: &Path, uti: &str, frames: &[super::WriteFrame]) -> Result<(), String> {
        let url = CFURL::from_file_path(path).ok_or("not a file path")?;
        // SAFETY: owned arguments; no options.
        let dest = unsafe { CGImageDestination::with_url(&url, &CFString::from_str(uti), frames.len(), None) }
            .ok_or_else(|| format!("this system can't write {uti}"))?;
        for f in frames {
            let mut props = vec![("Orientation", obj(NSNumber::new_u8(f.orientation)))];
            if let Some(d) = f.delay {
                props.push(("{HEICS}", obj(dict(vec![("DelayTime", obj(NSNumber::new_f64(d)))]))));
            }
            let props = dict(props);
            let image = cg_image(f.image)?;
            // SAFETY: NSDictionary is toll-free bridged to CFDictionary;
            // owned arguments.
            unsafe { dest.add_image(&image, Some(&*Retained::as_ptr(&props).cast::<CFDictionary>())) };
        }
        // SAFETY: owned.
        if unsafe { dest.finalize() } { Ok(()) } else { Err(format!("writing {uti} failed")) }
    }

    pub struct Source {
        src: CFRetained<CGImageSource>,
    }

    impl Source {
        pub fn open(path: &Path) -> Result<Option<(Source, Vec<u32>)>, String> {
            let src = open_source(path)?;
            // SAFETY: owned.
            let count = unsafe { src.count() };
            let first = properties(&src, 0).and_then(|p| frame_delay(as_ns(&p)));
            if !is_animated(count, first.is_some()) {
                return Ok(None);
            }
            let delays = (0..count).map(|i| properties(&src, i).and_then(|p| frame_delay(as_ns(&p))).unwrap_or(0)).collect();
            Ok(Some((Source { src }, delays)))
        }

        pub fn size(&self, i: usize) -> (u32, u32) {
            let Some(p) = properties(&self.src, i) else { return (0, 0) };
            let p = as_ns(&p);
            let (w, h) = (number(p, "PixelWidth").unwrap_or(0.0) as u32, number(p, "PixelHeight").unwrap_or(0.0) as u32);
            super::oriented_size(w, h, orientation_at(&self.src, i))
        }

        pub fn frame(&mut self, i: usize) -> Result<RgbaImage, String> {
            decode_at(&self.src, i)
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::{DynamicImage, Path, RgbaImage, ThumbError};

    pub fn decode(_: &Path) -> Result<DynamicImage, ThumbError> {
        Err(ThumbError("ImageIO is only available on macOS".into()))
    }

    pub fn readable_types() -> Vec<String> {
        Vec::new()
    }

    pub fn write(_: &Path, _: &str, _: &[super::WriteFrame]) -> Result<(), String> {
        Err("ImageIO is only available on macOS".into())
    }

    pub enum Source {}

    impl Source {
        pub fn open(_: &Path) -> Result<Option<(Source, Vec<u32>)>, String> {
            Ok(None)
        }

        pub fn size(&self, _: usize) -> (u32, u32) {
            match *self {}
        }

        pub fn frame(&mut self, _: usize) -> Result<RgbaImage, String> {
            match *self {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_to_imageio_types() {
        assert_eq!(type_of("avif"), Some("public.avif"));
        assert_eq!(type_of("hif"), Some("public.heic"));
        assert_eq!(type_of("jxl"), Some("public.jpeg-xl"));
        assert!(handles("HEIC") && handles("heif") && handles("avifs") && handles("heics"));
        assert!(!handles("jpg") && !handles("png") && !handles(""));
    }

    #[test]
    fn exif_orientations_map_and_turn_sizes() {
        assert_eq!(orientation(1), Orientation::NoTransforms);
        assert_eq!(orientation(3), Orientation::Rotate180);
        assert_eq!(orientation(6), Orientation::Rotate90);
        assert_eq!(orientation(8), Orientation::Rotate270);
        assert_eq!(orientation(2), Orientation::FlipHorizontal);
        // Out of range values leave the picture alone.
        assert_eq!(orientation(0), Orientation::NoTransforms);
        assert_eq!(orientation(9), Orientation::NoTransforms);
        assert_eq!(orientation(-1), Orientation::NoTransforms);
        assert_eq!(orientation(300), Orientation::NoTransforms);
        assert_eq!(oriented_size(40, 20, orientation(6)), (20, 40));
        assert_eq!(oriented_size(40, 20, orientation(5)), (20, 40));
        assert_eq!(oriented_size(40, 20, orientation(3)), (40, 20));
    }

    #[test]
    fn delays_prefer_the_unclamped_value() {
        assert_eq!(delay_ms(Some(0.02), Some(0.1)), 20);
        assert_eq!(delay_ms(None, Some(0.25)), 250);
        assert_eq!(delay_ms(Some(0.0), Some(0.1)), 100);
        assert_eq!(delay_ms(Some(f64::NAN), None), 0);
        assert_eq!(delay_ms(None, None), 0);
        assert_eq!(delay_ms(None, Some(-1.0)), 0);
        assert_eq!(delay_ms(Some(1.0 / 30.0), None), 33);
    }

    #[test]
    fn only_timed_sequences_animate() {
        assert!(is_animated(3, true));
        assert!(!is_animated(3, false), "an image collection is a still");
        assert!(!is_animated(1, true));
    }

    #[test]
    fn unpremultiplies_partial_alpha() {
        let mut px = [64, 32, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0];
        unpremultiply(&mut px);
        assert_eq!(px, [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }
}
