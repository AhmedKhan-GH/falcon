//! Eagle's App menu ▸ Color Space: how the window's pixels are matched to
//! the display. Eagle (Electron) offers sRGB, Display P3 and Unmanaged,
//! with Unmanaged the default, and its main process applies the choice to
//! the window.
//!
//! Falcon draws through wgpu's Metal surface, whose `CAMetalLayer` carries
//! the colour space. The choice sets the layer's `colorspace` to
//! `kCGColorSpaceSRGB`, `kCGColorSpaceDisplayP3`, or nil for Unmanaged
//! (the colours go to the display as they are). wgpu sets the layer's
//! colour space to nil each time it configures the surface (at start and
//! on every resize), so [`sync`] checks it every frame and sets it again
//! when it differs.

/// What the Color Space menu offers.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
    /// Eagle's default.
    #[default]
    Unmanaged,
}

impl ColorSpace {
    /// The name of the CoreGraphics colour space the layer is set to, or
    /// `None` for Unmanaged (no colour space at all).
    pub fn cg_name(self) -> Option<&'static str> {
        match self {
            ColorSpace::Srgb => Some("kCGColorSpaceSRGB"),
            ColorSpace::DisplayP3 => Some("kCGColorSpaceDisplayP3"),
            ColorSpace::Unmanaged => None,
        }
    }
}

/// Sets the window's colour space to `cs` when its layer isn't already
/// there; true once it is. Call it every frame (see the module note).
#[cfg(target_os = "macos")]
pub fn sync(frame: &eframe::Frame, cs: ColorSpace) -> bool {
    use objc2::runtime::AnyObject;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = frame.window_handle() else { return false };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return false };
    let view = h.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: `ns_view` is winit's live content view, and this runs on the
    // main thread inside `update`.
    unsafe { imp::apply_to_view(view, cs) }
}

#[cfg(not(target_os = "macos"))]
pub fn sync(_: &eframe::Frame, _: ColorSpace) -> bool {
    false
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ColorSpace;
    use objc2::encode::{Encoding, RefEncode};
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use std::ffi::c_void;

    /// `CGColorSpaceRef`'s pointee, for the encoding objc2 checks.
    #[repr(C)]
    pub struct CGColorSpace {
        _private: [u8; 0],
    }

    // SAFETY: matches `^{CGColorSpace=}`, the type of `-[CALayer colorspace]`.
    unsafe impl RefEncode for CGColorSpace {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGColorSpace", &[]));
    }

    type CFStringRef = *const c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGColorSpaceCreateWithName(name: CFStringRef) -> *mut CGColorSpace;
        fn CGColorSpaceCopyName(space: *mut CGColorSpace) -> CFStringRef;
        fn CGColorSpaceRelease(space: *mut CGColorSpace);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFEqual(a: *const c_void, b: *const c_void) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    /// Sets `view`'s `CAMetalLayer` to `cs`.
    ///
    /// # Safety
    /// `view` must be a live NSView (or null), used on the main thread.
    pub unsafe fn apply_to_view(view: *mut AnyObject, cs: ColorSpace) -> bool {
        if view.is_null() {
            return false;
        }
        // SAFETY: NSView getter.
        let layer: *mut AnyObject = unsafe { msg_send![view, layer] };
        // SAFETY: `layer` is a live CALayer or null.
        unsafe { apply_to_layer(layer, cs) }
    }

    /// Sets a `CAMetalLayer` to `cs` unless it already has it; false when
    /// `layer` isn't one.
    ///
    /// # Safety
    /// `layer` must be a live object or null.
    pub unsafe fn apply_to_layer(layer: *mut AnyObject, cs: ColorSpace) -> bool {
        let Some(metal) = AnyClass::get(c"CAMetalLayer") else { return false };
        if layer.is_null() {
            return false;
        }
        // SAFETY: isKindOfClass: is an NSObject method.
        let is_metal: Bool = unsafe { msg_send![layer, isKindOfClass: metal] };
        if !is_metal.as_bool() {
            return false;
        }
        // SAFETY: `colorspace` is a CALayer getter returning a CGColorSpaceRef
        // or nil, not retained for the caller; CGColorSpaceCopyName returns a
        // new string (or null), released below.
        unsafe {
            // The constants' values are their own names, and a CFString is an
            // NSString (toll-free bridged).
            let name = cs.cg_name().map(objc2_foundation::NSString::from_str);
            let wanted: CFStringRef = name.as_ref().map_or(std::ptr::null(), |n| std::ptr::from_ref::<objc2_foundation::NSString>(n).cast());
            let current: *mut CGColorSpace = msg_send![layer, colorspace];
            let current_name = if current.is_null() { std::ptr::null() } else { CGColorSpaceCopyName(current) };
            let same = match (current_name.is_null(), wanted.is_null()) {
                (true, true) => true,
                (false, false) => CFEqual(current_name, wanted) != 0,
                _ => false,
            };
            if !current_name.is_null() {
                CFRelease(current_name);
            }
            if same {
                return true;
            }
            let space = if wanted.is_null() { std::ptr::null_mut() } else { CGColorSpaceCreateWithName(wanted) };
            let _: () = msg_send![layer, setColorspace: space];
            if !space.is_null() {
                CGColorSpaceRelease(space);
            }
        }
        true
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[link(name = "QuartzCore", kind = "framework")]
        unsafe extern "C" {}

        /// The name of the colour space `layer` carries, if any.
        fn name(layer: *mut AnyObject) -> Option<String> {
            // SAFETY: CALayer getter, then the CoreGraphics name functions.
            unsafe {
                let space: *mut CGColorSpace = msg_send![layer, colorspace];
                if space.is_null() {
                    return None;
                }
                let n = CGColorSpaceCopyName(space);
                if n.is_null() {
                    return None;
                }
                // A CFString is an NSString (toll-free bridged).
                let out = (*(n as *const objc2_foundation::NSString)).to_string();
                CFRelease(n);
                Some(out)
            }
        }

        #[test]
        fn a_metal_layer_takes_each_colour_space() {
            let class = AnyClass::get(c"CAMetalLayer").expect("QuartzCore has CAMetalLayer");
            // SAFETY: new on a class returns an owned instance; released at the end.
            let layer: *mut AnyObject = unsafe { msg_send![class, new] };
            assert!(!layer.is_null());
            // SAFETY: `layer` is a live CAMetalLayer.
            unsafe {
                assert!(apply_to_layer(layer, ColorSpace::Srgb));
                assert_eq!(name(layer).as_deref(), Some("kCGColorSpaceSRGB"));
                assert!(apply_to_layer(layer, ColorSpace::DisplayP3));
                assert_eq!(name(layer).as_deref(), Some("kCGColorSpaceDisplayP3"));
                // Already set: nothing changes.
                assert!(apply_to_layer(layer, ColorSpace::DisplayP3));
                assert!(apply_to_layer(layer, ColorSpace::Unmanaged));
                assert_eq!(name(layer), None);
                let _: () = msg_send![layer, release];
            }
        }

        #[test]
        fn other_layers_and_nothing_are_refused() {
            // SAFETY: a plain NSObject is not a layer; null is refused.
            unsafe {
                let obj: *mut AnyObject = msg_send![AnyClass::get(c"NSObject").unwrap(), new];
                assert!(!apply_to_layer(obj, ColorSpace::Srgb));
                assert!(!apply_to_layer(std::ptr::null_mut(), ColorSpace::Srgb));
                assert!(!apply_to_view(std::ptr::null_mut(), ColorSpace::Srgb));
                let _: () = msg_send![obj, release];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmanaged_is_the_default_as_in_eagle() {
        assert_eq!(ColorSpace::default(), ColorSpace::Unmanaged);
    }

    #[test]
    fn each_choice_maps_to_its_core_graphics_name() {
        assert_eq!(ColorSpace::Srgb.cg_name(), Some("kCGColorSpaceSRGB"));
        assert_eq!(ColorSpace::DisplayP3.cg_name(), Some("kCGColorSpaceDisplayP3"));
        assert_eq!(ColorSpace::Unmanaged.cg_name(), None);
    }

    #[test]
    fn the_choice_is_kept_in_the_saved_preferences() {
        let mut p = crate::prefs::Prefs::default();
        assert_eq!(p.color_space, ColorSpace::Unmanaged);
        p.color_space = ColorSpace::DisplayP3;
        let text = serde_json::to_string(&p).unwrap();
        let back: crate::prefs::Prefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back.color_space, ColorSpace::DisplayP3);
        // Saved state from before the menu has none and gets the default.
        let old: crate::prefs::Prefs = serde_json::from_str(r#"{"hover_zoom":false}"#).unwrap();
        assert_eq!(old.color_space, ColorSpace::Unmanaged);
    }
}
