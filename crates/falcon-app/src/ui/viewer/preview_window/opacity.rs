//! The window's opacity. eframe has no command for it, so on macOS the
//! NSWindow with the viewport's title gets its `alphaValue` set.

/// Sets the opacity (percent) of the window titled `title`. True when the
/// window was found (always true where there is nothing to set).
#[cfg(target_os = "macos")]
pub fn set(title: &str, percent: u8) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows().iter() {
        if window.title().to_string() == title {
            window.setAlphaValue(f64::from(percent.clamp(10, 100)) / 100.0);
            return true;
        }
    }
    false
}

#[cfg(not(target_os = "macos"))]
pub fn set(_title: &str, _percent: u8) -> bool {
    true
}
