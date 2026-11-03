//! Graying a web page, as Eagle's grayscale preview grays its iframes: CSS
//! `filter` on the page's root element, which covers its frames. Core Image
//! filters on the native view render in this process, and WebKit composites
//! its content in another. A user script sets the filter on each new document
//! and a call sets it on the one shown ([`PageGray`]). Neither loads anything;
//! off removes both. `tests/web_gray.rs` runs this against real WebKit.

/// The script that grays (or restores) the document it runs in. It goes
/// through the style object, which a page's content security policy allows.
pub fn gray_script(on: bool) -> &'static str {
    if on {
        "var e=document.documentElement;if(e&&e.style)e.style.setProperty('filter','grayscale(1)','important');"
    } else {
        "var e=document.documentElement;if(e&&e.style)e.style.removeProperty('filter');"
    }
}

/// What the web view must do to follow the grayscale preview.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct GrayWork {
    /// Add or remove the user script, for documents to come.
    pub script: bool,
    /// Run [`gray_script`] on the document shown.
    pub apply: bool,
}

/// The grayscale last given to a web view. The user script handles new
/// documents, but a page coming back from the back-forward cache keeps the
/// filter it left with, so a change of address applies the flag again. A view
/// that was never gray runs nothing.
#[derive(Default)]
pub struct PageGray {
    on: bool,
    ever: bool,
    url: Option<String>,
}

impl PageGray {
    pub fn step(&mut self, gray: bool, url: Option<&str>) -> GrayWork {
        let script = gray != self.on;
        let moved = self.url.as_deref() != url;
        self.on = gray;
        self.ever |= gray;
        if moved {
            self.url = url.map(str::to_owned);
        }
        GrayWork { script, apply: script || (moved && self.ever) }
    }
}

#[cfg(target_os = "macos")]
use objc2::{msg_send, rc::{Allocated, Retained}, runtime::{AnyClass, AnyObject}};

/// Adds the graying script to `view` for documents to come, or removes it.
///
/// # Safety
/// `view` must be a live `WKWebView`, on the main thread.
#[cfg(target_os = "macos")]
pub unsafe fn set_script(view: &AnyObject, on: bool) {
    // WKUserScriptInjectionTimeAtDocumentStart: the document element exists,
    // nothing has painted.
    const AT_DOCUMENT_START: isize = 0;
    // SAFETY: WKWebView's configuration and its content controller, checked
    // for nil, and alloc/init of a WebKit class on the main thread. This is
    // the only user script a view holds, so removing all removes this one.
    unsafe {
        let conf: Option<Retained<AnyObject>> = msg_send![view, configuration];
        let Some(conf) = conf else { return };
        let scripts: Option<Retained<AnyObject>> = msg_send![&*conf, userContentController];
        let Some(scripts) = scripts else { return };
        if !on {
            let _: () = msg_send![&*scripts, removeAllUserScripts];
            return;
        }
        let Some(cls) = AnyClass::get(c"WKUserScript") else { return };
        let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
        let source = objc2_foundation::NSString::from_str(gray_script(true));
        let script: Option<Retained<AnyObject>> = msg_send![alloc, initWithSource: &*source, injectionTime: AT_DOCUMENT_START, forMainFrameOnly: true];
        if let Some(script) = script {
            let _: () = msg_send![&*scripts, addUserScript: &*script];
        }
    }
}

/// Grays or restores the document `view` shows.
///
/// # Safety
/// `view` must be a live `WKWebView`, on the main thread.
#[cfg(target_os = "macos")]
pub unsafe fn apply(view: &AnyObject, on: bool) {
    let done: Option<&block2::DynBlock<dyn Fn(*mut AnyObject, *mut AnyObject)>> = None;
    let source = objc2_foundation::NSString::from_str(gray_script(on));
    // SAFETY: a WKWebView method on the live view; its handler may be nil.
    unsafe {
        let _: () = msg_send![view, evaluateJavaScript: &*source, completionHandler: done];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_follows_the_grayscale_flag() {
        let mut g = PageGray::default();
        let (none, script, apply) = (
            GrayWork { script: false, apply: false },
            GrayWork { script: true, apply: true },
            GrayWork { script: false, apply: true },
        );
        // Never gray: no script runs, wherever the page goes.
        assert_eq!(g.step(false, None), none);
        assert_eq!(g.step(false, Some("https://a.example/")), none);
        // On, and off again: the script and the document shown.
        assert_eq!(g.step(true, Some("https://a.example/")), script);
        assert_eq!(g.step(true, Some("https://a.example/")), none);
        // A new address gets the flag again, gray or not, once it was gray:
        // the page may come from the back-forward cache.
        assert_eq!(g.step(true, Some("https://b.example/")), apply);
        assert_eq!(g.step(false, Some("https://b.example/")), script);
        assert_eq!(g.step(false, Some("https://a.example/")), apply);
        assert_eq!(g.step(false, Some("https://a.example/")), none);
    }

    #[test]
    fn the_gray_script_is_reversible_and_loads_nothing() {
        assert!(gray_script(true).contains("setProperty('filter','grayscale(1)'"));
        assert!(gray_script(false).contains("removeProperty('filter')"));
        for s in [gray_script(true), gray_script(false)] {
            assert!(!s.contains("url(") && !s.contains("http") && !s.contains("@import"), "{s}");
        }
    }
}
