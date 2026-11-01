//! The macOS share sheet (Eagle's Share in the card menu): AppKit's
//! `NSSharingServicePicker` with the items' originals, shown at the
//! pointer. Elsewhere there is no share sheet.

use std::path::PathBuf;

/// Whether this platform has a share sheet.
pub const AVAILABLE: bool = cfg!(target_os = "macos");

/// Shows the share sheet for `files` at `at` (window points, from the top
/// left), anchored to the key window. Does nothing off the main thread or
/// without a window.
#[cfg(target_os = "macos")]
pub fn share(files: &[PathBuf], at: (f32, f32)) {
    use std::cell::RefCell;

    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{MainThreadMarker, msg_send};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, NSURL};

    thread_local! {
        // The picker shown last, kept alive while its sheet is open.
        static PICKER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    }

    if files.is_empty() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(class) = AnyClass::get(c"NSSharingServicePicker") else { return };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: keyWindow and contentView return an NSWindow and an NSView
    // (or nil); bounds and isFlipped are plain NSView getters.
    unsafe {
        let window: Option<Retained<AnyObject>> = msg_send![&*app, keyWindow];
        let Some(window) = window else { return };
        let view: Option<Retained<AnyObject>> = msg_send![&*window, contentView];
        let Some(view) = view else { return };
        let urls: Vec<Retained<NSURL>> = files.iter().map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))).collect();
        let items = NSArray::from_retained_slice(&urls);
        let alloc: Allocated<AnyObject> = msg_send![class, alloc];
        let picker: Retained<AnyObject> = msg_send![alloc, initWithItems: &*items];
        let bounds: NSRect = msg_send![&*view, bounds];
        let flipped: bool = msg_send![&*view, isFlipped];
        let (x, y) = (f64::from(at.0), f64::from(at.1));
        let y = if flipped { y } else { bounds.size.height - y };
        let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(1.0, 1.0));
        // NSRectEdge: 1 is the minimum-y edge.
        let _: () = msg_send![&*picker, showRelativeToRect: rect, ofView: &*view, preferredEdge: 1usize];
        PICKER.with(|p| *p.borrow_mut() = Some(picker));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn share(_: &[PathBuf], _: (f32, f32)) {}
