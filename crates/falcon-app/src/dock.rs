//! Closing the main window keeps falcon running, as Eagle does on macOS:
//! the window goes away but the app stays in the Dock, and clicking its
//! Dock icon (or opening it again from Finder) brings the window back.
//!
//! The window is not closed but the app is hidden, which AppKit undoes by
//! itself whenever the app is activated again, so no reopen handler is
//! needed. Quit (⌘Q) never asks the window to close and still quits.

/// Hides the app in answer to a close request. Returns whether it did;
/// where it can't (other platforms), the window closes as before.
pub fn keep_running_hidden() -> bool {
    platform::hide()
}

/// Brings falcon to the front from another app: unhides it (after the
/// window was closed) and activates it. Used by the menu bar icon and the
/// global keys.
pub fn bring_to_front() {
    platform::bring_to_front();
}

/// Quits falcon as ⌘Q does (the menu bar icon's Quit).
pub fn quit() {
    platform::quit();
}

/// Calls `f` whenever the app is hidden (⌘H, the red button) or the window
/// is minimized, on the main thread. These come as AppKit notifications
/// because eframe draws no frames for a hidden window, so nothing in the
/// frame loop would notice.
pub fn on_hidden(f: fn()) {
    platform::on_hidden(f);
}

#[cfg(target_os = "macos")]
mod platform {
    use block2::RcBlock;
    use objc2_app_kit::{NSApplicationDidHideNotification, NSWindowDidMiniaturizeNotification};
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use std::ptr::NonNull;

    pub fn on_hidden(f: fn()) {
        let center = NSNotificationCenter::defaultCenter();
        for name in [unsafe { NSApplicationDidHideNotification }, unsafe { NSWindowDidMiniaturizeNotification }] {
            let block = RcBlock::new(move |_: NonNull<NSNotification>| f());
            // The observer lives as long as the app.
            let observer = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) };
            std::mem::forget(observer);
        }
    }

    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    pub fn hide() -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        NSApplication::sharedApplication(mtm).hide(None);
        true
    }

    pub fn bring_to_front() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        app.unhide(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }

    pub fn quit() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        NSApplication::sharedApplication(mtm).terminate(None);
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn on_hidden(_: fn()) {}

    pub fn hide() -> bool {
        false
    }

    pub fn bring_to_front() {}

    pub fn quit() {}
}
