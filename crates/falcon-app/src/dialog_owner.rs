//! The system's file and message dialogs, owned by the main window on Windows.
//!
//! The dialogs block the frame that opens them. Without an owner, Windows
//! leaves the main window enabled, so a click on it raises it over the
//! dialog: falcon then waits for an answer nobody can see and stops drawing.
//! An owned dialog stays above the window and disables it until answered.
//! macOS and Linux keep their unowned dialogs (on macOS an owner would make
//! the dialog a sheet).

/// Remembers the main window for the dialogs opened after this; called
/// every frame.
pub fn remember(frame: &eframe::Frame) {
    platform::remember(frame);
}

/// An `rfd::FileDialog`, owned by the main window where that matters.
pub fn file_dialog() -> rfd::FileDialog {
    platform::file_dialog()
}

/// An `rfd::MessageDialog`, owned by the main window where that matters.
pub fn message_dialog() -> rfd::MessageDialog {
    platform::message_dialog()
}

#[cfg(windows)]
mod platform {
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
    };
    use std::cell::Cell;
    use std::num::NonZeroIsize;

    thread_local! {
        /// The main window's `HWND`. Dialogs open on the UI thread, which
        /// is the one that remembers it.
        static OWNER: Cell<Option<NonZeroIsize>> = const { Cell::new(None) };
    }

    pub fn remember(frame: &eframe::Frame) {
        if let Ok(handle) = frame.window_handle()
            && let RawWindowHandle::Win32(h) = handle.as_raw()
        {
            OWNER.set(Some(h.hwnd));
        }
    }

    /// The remembered window, handed to rfd as the dialog's parent.
    struct Owner(NonZeroIsize);

    impl HasWindowHandle for Owner {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::Win32(Win32WindowHandle::new(self.0));
            // SAFETY: the main window lives as long as the app, and rfd uses
            // the handle only while the dialog it opens is up.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    impl HasDisplayHandle for Owner {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(DisplayHandle::windows())
        }
    }

    pub fn file_dialog() -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match OWNER.get() {
            Some(hwnd) => dialog.set_parent(&Owner(hwnd)),
            None => dialog,
        }
    }

    pub fn message_dialog() -> rfd::MessageDialog {
        let dialog = rfd::MessageDialog::new();
        match OWNER.get() {
            Some(hwnd) => dialog.set_parent(&Owner(hwnd)),
            None => dialog,
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn remember(_: &eframe::Frame) {}

    pub fn file_dialog() -> rfd::FileDialog {
        rfd::FileDialog::new()
    }

    pub fn message_dialog() -> rfd::MessageDialog {
        rfd::MessageDialog::new()
    }
}
