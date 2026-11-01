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

#[cfg(target_os = "macos")]
mod platform {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    pub fn hide() -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        NSApplication::sharedApplication(mtm).hide(None);
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn hide() -> bool {
        false
    }
}
