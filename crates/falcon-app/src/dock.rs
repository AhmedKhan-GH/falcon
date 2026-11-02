//! Closing the main window keeps falcon running, as Eagle does on macOS:
//! the window goes away but the app stays in the Dock, and clicking its
//! Dock icon (or opening it again from Finder) brings the window back.
//!
//! The window is not closed but the app is hidden, which AppKit undoes by
//! itself whenever the app is activated again, so no reopen handler is
//! needed. Quit (⌘Q) never asks the window to close and still quits.
//! Preferences ▸ General can turn this off (`Prefs::keep_dock_icon`, on by
//! default as in Eagle). Then, while the menu bar icon is shown, closing
//! the window also takes falcon out of the Dock (it becomes an accessory
//! app) and the menu bar icon brings it back; without the menu bar icon
//! nothing could, so closing the window quits instead ([`close_action`]).

/// What closing the main window does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseAction {
    /// Hide the app; it stays in the Dock.
    Hide,
    /// Hide the app and take it out of the Dock (the menu bar icon stays).
    HideFromDock,
    /// Let the close go through, which quits.
    Quit,
}

/// The close rule: `keep_dock` and `menu_icon` are the preferences,
/// `can_hide` whether this platform can hide the app.
pub fn close_action(keep_dock: bool, menu_icon: bool, can_hide: bool) -> CloseAction {
    match (can_hide, keep_dock, menu_icon) {
        (false, ..) => CloseAction::Quit,
        (true, true, _) => CloseAction::Hide,
        (true, false, true) => CloseAction::HideFromDock,
        (true, false, false) => CloseAction::Quit,
    }
}

/// Puts falcon in the Dock or takes it out (an accessory app keeps its
/// menu bar icon and windows). Remembered, so [`in_dock`] can tell.
pub fn set_in_dock(shown: bool) {
    if platform::set_in_dock(shown) {
        OUT_OF_DOCK.store(!shown, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Whether falcon is in the Dock (false only after [`set_in_dock`] took
/// it out).
pub fn in_dock() -> bool {
    !OUT_OF_DOCK.load(std::sync::atomic::Ordering::Relaxed)
}

static OUT_OF_DOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether this platform can hide the app instead of closing its window.
pub const CAN_HIDE: bool = cfg!(target_os = "macos");

/// Hides the app in answer to a close request. Returns whether it did;
/// where it can't (other platforms), the window closes as before.
pub fn keep_running_hidden() -> bool {
    platform::hide()
}

/// Brings falcon to the front from another app: unhides it (after the
/// window was closed) and activates it. Used by the menu bar icon and the
/// global keys.
pub fn bring_to_front() {
    if !in_dock() {
        set_in_dock(true);
    }
    platform::bring_to_front();
}

/// Whether falcon is the active app (the one in front and receiving keys),
/// whichever of its windows has focus. False while hidden.
pub fn is_active() -> bool {
    platform::is_active()
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

    pub fn is_active() -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        NSApplication::sharedApplication(mtm).isActive()
    }

    pub fn quit() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        NSApplication::sharedApplication(mtm).terminate(None);
    }

    /// `setActivationPolicy:` with Regular (0) or Accessory (1).
    pub fn set_in_dock(shown: bool) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let app = NSApplication::sharedApplication(mtm);
        let policy: isize = if shown { 0 } else { 1 };
        // SAFETY: NSApplication's setActivationPolicy: takes an NSInteger
        // enum value and returns BOOL.
        let done: bool = unsafe { objc2::msg_send![&*app, setActivationPolicy: policy] };
        done
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn on_hidden(_: fn()) {}

    pub fn hide() -> bool {
        false
    }

    pub fn bring_to_front() {}

    pub fn is_active() -> bool {
        false
    }

    pub fn quit() {}

    pub fn set_in_dock(_shown: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_follows_the_preferences() {
        assert_eq!(close_action(true, false, true), CloseAction::Hide);
        assert_eq!(close_action(true, true, true), CloseAction::Hide);
        assert_eq!(close_action(false, true, true), CloseAction::HideFromDock);
        assert_eq!(close_action(false, false, true), CloseAction::Quit);
        assert_eq!(close_action(true, true, false), CloseAction::Quit);
    }
}
