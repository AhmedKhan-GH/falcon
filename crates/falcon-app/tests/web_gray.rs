//! The grayscale preview's page filter against real WebKit: the script added
//! for new documents, the call on the one shown, and both taken back. WebKit
//! needs the process's main thread, so this runs without the test harness.
//! The page is an HTML string, so nothing touches the network, and the web
//! view is on no window. It skips, saying so, with no window server session.

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("skipped: WebKit is macOS only");
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/web_gray.rs"]
mod web_gray;

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSDate, NSPoint, NSRect, NSRunLoop, NSSize, NSString};

    use crate::web_gray::{apply, set_script};

    #[link(name = "WebKit", kind = "framework")]
    unsafe extern "C" {}

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGSessionCopyCurrentDictionary() -> *const std::ffi::c_void;
    }

    const READ: &str = "getComputedStyle(document.documentElement).filter + '|' + document.title";

    fn pump() {
        let until = NSDate::dateWithTimeIntervalSinceNow(0.02);
        NSRunLoop::currentRunLoop().runUntilDate(&until);
    }

    /// The root element's computed filter and the page's title, or `None` if
    /// WebKit doesn't answer in time.
    fn read(view: &AnyObject) -> Option<(String, String)> {
        let out: Rc<RefCell<Option<String>>> = Rc::default();
        let sink = out.clone();
        let done = RcBlock::new(move |value: *mut AnyObject, _error: *mut AnyObject| {
            // SAFETY: WebKit hands over a live object or nil; the script's
            // result is a string, checked by the downcast.
            let value = unsafe { Retained::retain(value) };
            *sink.borrow_mut() = value.and_then(|v| v.downcast::<NSString>().ok()).map(|s| s.to_string());
        });
        // SAFETY: a WKWebView method on the live view, with a live handler.
        unsafe {
            let _: () = msg_send![view, evaluateJavaScript: &*NSString::from_str(READ), completionHandler: &*done];
        }
        let start = Instant::now();
        while out.borrow().is_none() && start.elapsed() < Duration::from_secs(5) {
            pump();
        }
        let text = out.borrow().clone()?;
        let (filter, title) = text.split_once('|')?;
        Some((filter.to_owned(), title.to_owned()))
    }

    /// Loads a page titled `title` and waits for it.
    fn load(view: &AnyObject, title: &str) -> bool {
        let html = NSString::from_str(&format!("<!doctype html><title>{title}</title><p>page</p>"));
        // SAFETY: a WKWebView method on the live view; a nil base URL is allowed.
        unsafe {
            let _: Option<Retained<AnyObject>> = msg_send![view, loadHTMLString: &*html, baseURL: std::ptr::null::<AnyObject>()];
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            pump();
            // SAFETY: a WKWebView getter on the live view.
            let loading: bool = unsafe { msg_send![view, isLoading] };
            if !loading && read(view).is_some_and(|(_, t)| t == title) {
                return true;
            }
        }
        false
    }

    pub fn run() {
        // SAFETY: a CoreGraphics query with no arguments; a non-null result
        // is an owned dictionary, leaked for the test's short life.
        if unsafe { CGSessionCopyCurrentDictionary() }.is_null() {
            println!("skipped: no window server session");
            return;
        }
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        // SAFETY: NSApplicationActivationPolicyProhibited: no Dock icon.
        let _: bool = unsafe { msg_send![&*app, setActivationPolicy: 2isize] };
        let (web, conf) = (AnyClass::get(c"WKWebView").expect("WebKit"), AnyClass::get(c"WKWebViewConfiguration").expect("WebKit"));
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
        // SAFETY: plain alloc/init of WebKit classes on the main thread.
        let view: Retained<AnyObject> = unsafe {
            let conf: Retained<AnyObject> = msg_send![conf, new];
            let alloc: Allocated<AnyObject> = msg_send![web, alloc];
            msg_send![alloc, initWithFrame: frame, configuration: &*conf]
        };
        if !load(&view, "plain") {
            println!("skipped: WebKit can't load a page here");
            return;
        }
        let filter = |view: &AnyObject| read(view).expect("WebKit answers").0;
        assert_eq!(filter(&view), "none", "a page starts in colour");
        // SAFETY (all below): `view` is a live WKWebView, on the main thread.
        // The call grays the document shown, and takes it back.
        unsafe { apply(&view, true) };
        assert_eq!(filter(&view), "grayscale(1)");
        unsafe { apply(&view, false) };
        assert_eq!(filter(&view), "none");
        // The script grays the next document, and removing it stops that.
        unsafe { set_script(&view, true) };
        assert!(load(&view, "gray"));
        assert_eq!(filter(&view), "grayscale(1)", "a new document starts gray");
        unsafe { set_script(&view, false) };
        assert!(load(&view, "colour"));
        assert_eq!(filter(&view), "none", "the script is gone");
        println!("ok");
    }
}
