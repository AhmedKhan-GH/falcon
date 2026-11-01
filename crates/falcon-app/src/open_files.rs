//! Files opened with falcon from Finder (Open With, or dropped on the Dock
//! icon), after Eagle's "open-file" handling: the bundle declares the
//! document types (`scripts/bundle.sh`), and macOS hands the files to the
//! running app as an "open documents" Apple event, which [`install`]
//! catches. The app takes them with [`take`] each frame and treats them
//! as Eagle does: an `.eaglepack` is imported as a package, a `.library`
//! is opened (through the sandbox guard, like a dropped one) and anything
//! else is imported into the shown library.
//!
//! AppKit installs its own handler for this event while the app finishes
//! launching, so falcon's handler goes in when AppKit announces that
//! (`NSApplicationWillFinishLaunchingNotification`), as Apple advises,
//! which is still before a launch's own documents are delivered.

use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub use mac::{install, set_repaint, take};

/// Files can only arrive this way on macOS.
#[cfg(not(target_os = "macos"))]
pub fn install() {}
#[cfg(not(target_os = "macos"))]
pub fn set_repaint(_ctx: &eframe::egui::Context) {}
#[cfg(not(target_os = "macos"))]
pub fn take() -> Vec<PathBuf> {
    Vec::new()
}

/// Keeps the paths of one event that name something, in order, without
/// repeats.
pub fn gather(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        if !p.as_os_str().is_empty() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter};

    /// `kCoreEventClass`, 'aevt'.
    const CORE_EVENT: u32 = u32::from_be_bytes(*b"aevt");
    /// `kAEOpenDocuments`, 'odoc'.
    const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
    /// `keyDirectObject`, '----': the list of files inside the event.
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    static FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static REPAINT: OnceLock<eframe::egui::Context> = OnceLock::new();

    thread_local! {
        static HANDLER: RefCell<Option<Retained<Handler>>> = const { RefCell::new(None) };
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and Handler has
        // no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FalconOpenFilesHandler"]
        struct Handler;

        impl Handler {
            #[unsafe(method(appWillFinishLaunching:))]
            fn will_finish_launching(&self, _note: &NSNotification) {
                let manager = NSAppleEventManager::sharedAppleEventManager();
                let target: &AnyObject = self;
                // SAFETY: the handler implements the selector with the
                // signature the Apple event manager calls, and stays alive
                // in HANDLER for the life of the main thread.
                unsafe {
                    let _: () = msg_send![
                        &*manager,
                        setEventHandler: target,
                        andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                        forEventClass: CORE_EVENT,
                        andEventID: OPEN_DOCUMENTS
                    ];
                }
            }

            #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
            fn open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                // SAFETY: paramDescriptorForKeyword: takes an AEKeyword
                // (a FourCharCode, u32) and returns a descriptor or nil.
                let param: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
                let Some(param) = param else { return };
                // A list of files (1-based), or a single one.
                let n = param.numberOfItems();
                let items: Vec<Retained<NSAppleEventDescriptor>> = if n > 0 { (1..=n).filter_map(|i| param.descriptorAtIndex(i)).collect() } else { vec![param] };
                let paths = super::gather(items.iter().filter_map(|d| d.fileURLValue()).filter_map(|u| u.path()).map(|p| PathBuf::from(p.to_string())));
                if paths.is_empty() {
                    return;
                }
                FILES.lock().unwrap_or_else(|p| p.into_inner()).extend(paths);
                if let Some(ctx) = REPAINT.get() {
                    ctx.request_repaint();
                }
            }
        }

        unsafe impl NSObjectProtocol for Handler {}
    );

    /// Starts listening for opened files. Call it on the main thread
    /// before the event loop runs.
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        HANDLER.with(|h| {
            let mut h = h.borrow_mut();
            if h.is_some() {
                return;
            }
            let this = Handler::alloc(mtm).set_ivars(());
            let handler: Retained<Handler> = unsafe { msg_send![super(this), init] };
            let observer: &AnyObject = &handler;
            // SAFETY: the observer implements the selector, taking the
            // notification, and outlives the registration (HANDLER keeps
            // it for the life of the main thread); the name is AppKit's.
            unsafe {
                NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                    observer,
                    sel!(appWillFinishLaunching:),
                    Some(objc2_app_kit::NSApplicationWillFinishLaunchingNotification),
                    None,
                );
            }
            *h = Some(handler);
        });
    }

    /// Lets opened files wake the app to handle them.
    pub fn set_repaint(ctx: &eframe::egui::Context) {
        let _ = REPAINT.set(ctx.clone());
    }

    /// The files that arrived since the last call, oldest first.
    pub fn take() -> Vec<PathBuf> {
        std::mem::take(&mut *FILES.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gathers_each_path_once_in_order() {
        let got = gather(["/a.png", "", "/b.eaglepack", "/a.png"].map(PathBuf::from));
        assert_eq!(got, ["/a.png", "/b.eaglepack"].map(PathBuf::from));
    }
}
