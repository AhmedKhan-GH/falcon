//! The [`Engine`] on macOS: each page gets a new `WKWebView` in a window of
//! its own, placed off every screen.
//!
//! Each page has a new non-persistent data store, so no cookie or storage
//! reaches disk or another page, and the only request headers are WebKit's
//! own. A content rule list blocks third-party loads and strips cookies. There is no UI
//! delegate, so WebKit answers JavaScript dialogs itself and refuses new
//! windows, and media needs a user gesture. Navigation is allowed only to
//! http and https; a frame may also use `about:`, `data:` and `blob:`.
//!
//! Every message is sent on the main thread (`prepare` and `open` check) to
//! live objects we retain or objects AppKit returned, checked for nil.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use block2::{Block, RcBlock};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::{NSData, NSError, NSNumber, NSPoint, NSRect, NSSize, NSString, NSURL};

use super::{Engine, Page, PageEvent, Readiness, scheme_allowed};

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

/// Every load from another site than the page's is refused, and no request
/// carries a cookie, even one the page set for itself a moment ago.
const RULES: &str = r#"[{"trigger":{"url-filter":".*"},"action":{"type":"block-cookies"}},{"trigger":{"url-filter":".*","load-type":["third-party"]},"action":{"type":"block"}}]"#;
/// Where the off-screen windows go, far from any display.
const OFF_SCREEN: f64 = -30_000.0;
/// The picture's width in points: enough for a thumbnail, and half of
/// [`super::PAGE_SIZE`] keeps the copy on the UI thread short.
const SHOT_WIDTH: f64 = 720.0;
/// `NSURLErrorCancelled`: one navigation gave way to the next, as a
/// redirecting page does. The page isn't over.
const CANCELLED: isize = -999;
/// `WKNavigationActionPolicyCancel` and `...Allow`.
const POLICY_CANCEL: isize = 0;
const POLICY_ALLOW: isize = 1;
/// `WKAudiovisualMediaTypeAll`.
const ALL_MEDIA: usize = usize::MAX;

type Wake = Rc<dyn Fn()>;

/// What a page's navigation delegate and picture callback leave behind.
struct Shared {
    events: RefCell<VecDeque<PageEvent>>,
    wake: Wake,
}

impl Shared {
    fn push(&self, event: PageEvent) {
        self.events.borrow_mut().push_back(event);
        (self.wake)();
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the ivars need no
    // particular Drop order.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FalconPageShotDelegate"]
    #[ivars = Rc<Shared>]
    struct Delegate;

    impl Delegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _web: &AnyObject, _nav: Option<&AnyObject>) {
            self.ivars().push(PageEvent::Finished);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _web: &AnyObject, _nav: Option<&AnyObject>, error: &NSError) {
            self.failed(error);
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(&self, _web: &AnyObject, _nav: Option<&AnyObject>, error: &NSError) {
            self.failed(error);
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, _web: &AnyObject) {
            self.ivars().push(PageEvent::Failed);
        }

        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(&self, _web: &AnyObject, action: &AnyObject, handler: &Block<dyn Fn(isize)>) {
            handler.call((if navigation_allowed(action) { POLICY_ALLOW } else { POLICY_CANCEL },));
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, shared: Rc<Shared>) -> Retained<Delegate> {
        let this = Self::alloc(mtm).set_ivars(shared);
        // SAFETY: NSObject's init on a new instance.
        unsafe { msg_send![super(this), init] }
    }

    fn failed(&self, error: &NSError) {
        if error.code() != CANCELLED {
            self.ivars().push(PageEvent::Failed);
        }
    }
}

/// Whether a navigation goes ahead, by its scheme and frame. A request for
/// a new window has no target frame and is refused.
fn navigation_allowed(action: &AnyObject) -> bool {
    // SAFETY: WKNavigationAction, WKFrameInfo, NSURLRequest and NSURL
    // getters on the live action WebKit passed; nils are checked.
    unsafe {
        let frame: Option<Retained<AnyObject>> = msg_send![action, targetFrame];
        let Some(frame) = frame else { return false };
        let main: bool = msg_send![&*frame, isMainFrame];
        let request: Option<Retained<AnyObject>> = msg_send![action, request];
        let Some(request) = request else { return false };
        let url: Option<Retained<NSURL>> = msg_send![&*request, URL];
        let scheme = url.and_then(|u| u.scheme());
        scheme.is_some_and(|s| scheme_allowed(&s.to_string(), main))
    }
}

#[derive(Default)]
enum Rules {
    #[default]
    NotStarted,
    Compiling,
    Ready(Retained<AnyObject>),
    Failed,
}

/// Opens pages in WebKit. `wake` is called whenever something happens that
/// the next tick should see.
pub struct WebKit {
    wake: Wake,
    rules: Rc<RefCell<Rules>>,
}

impl WebKit {
    pub fn new(wake: impl Fn() + 'static) -> WebKit {
        WebKit { wake: Rc::new(wake), rules: Rc::default() }
    }

    /// Starts compiling the rule list; on failure the rules stay unusable.
    fn compile(&mut self) {
        *self.rules.borrow_mut() = Rules::Failed;
        let Some(store) = AnyClass::get(c"WKContentRuleListStore") else { return };
        let dir = std::env::temp_dir().join(format!("falcon-page-rules-{}", std::process::id()));
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
        let (rules, wake) = (self.rules.clone(), self.wake.clone());
        let done = RcBlock::new(move |list: *mut AnyObject, _error: *mut NSError| {
            // SAFETY: WebKit hands a live rule list, or nil.
            let list = unsafe { Retained::retain(list) };
            // The list is held in memory now; its file isn't needed.
            let _ = std::fs::remove_dir_all(&dir);
            *rules.borrow_mut() = list.map_or(Rules::Failed, Rules::Ready);
            wake();
        });
        // SAFETY: the store class method takes a file URL and returns the
        // store or nil; the compile method copies its block and calls it
        // once on the main thread.
        unsafe {
            let store: Option<Retained<AnyObject>> = msg_send![store, storeWithURL: &*url];
            let Some(store) = store else { return };
            *self.rules.borrow_mut() = Rules::Compiling;
            let _: () = msg_send![
                &*store,
                compileContentRuleListForIdentifier: &*NSString::from_str("falcon-page-shot"),
                encodedContentRuleList: &*NSString::from_str(RULES),
                completionHandler: &*done
            ];
        }
    }

    fn configuration(rules: &AnyObject) -> Option<Retained<AnyObject>> {
        let (conf, store) = (AnyClass::get(c"WKWebViewConfiguration")?, AnyClass::get(c"WKWebsiteDataStore")?);
        // SAFETY: WebKit's configuration setters on new objects, with the
        // argument types they declare; getters are checked for nil.
        unsafe {
            let conf: Retained<AnyObject> = msg_send![conf, new];
            let store: Retained<AnyObject> = msg_send![store, nonPersistentDataStore];
            let _: () = msg_send![&*conf, setWebsiteDataStore: &*store];
            let _: () = msg_send![&*conf, setMediaTypesRequiringUserActionForPlayback: ALL_MEDIA];
            let prefs: Option<Retained<AnyObject>> = msg_send![&*conf, preferences];
            let _: () = msg_send![&*prefs?, setJavaScriptCanOpenWindowsAutomatically: false];
            let content: Option<Retained<AnyObject>> = msg_send![&*conf, userContentController];
            let _: () = msg_send![&*content?, addContentRuleList: rules];
            Some(conf)
        }
    }
}

impl Engine for WebKit {
    type Page = WebPage;

    fn prepare(&mut self) -> Readiness {
        if MainThreadMarker::new().is_none() || AnyClass::get(c"WKWebView").is_none() {
            return Readiness::Unavailable;
        }
        if matches!(*self.rules.borrow(), Rules::NotStarted) {
            self.compile();
        }
        match &*self.rules.borrow() {
            Rules::Ready(_) => Readiness::Ready,
            Rules::Failed => Readiness::Unavailable,
            Rules::NotStarted | Rules::Compiling => Readiness::Waiting,
        }
    }

    fn open(&mut self, url: &str, size: (u32, u32)) -> Option<WebPage> {
        let mtm = MainThreadMarker::new()?;
        if !super::is_web_address(url) {
            return None;
        }
        let rules = self.rules.borrow();
        let Rules::Ready(rules) = &*rules else { return None };
        let conf = Self::configuration(rules)?;
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let shared = Rc::new(Shared { events: RefCell::default(), wake: self.wake.clone() });
        let delegate = Delegate::new(mtm, shared.clone());
        let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h));
        let frame = NSRect::new(NSPoint::new(OFF_SCREEN, OFF_SCREEN), NSSize::new(w, h));
        // SAFETY: plain AppKit window setup and WKWebView alloc/init on the
        // main thread; the view's delegate is weak and `delegate` outlives
        // it in `WebPage`. The window isn't released when closed, since
        // `WebPage` owns it.
        unsafe {
            let web = AnyClass::get(c"WKWebView")?;
            let alloc: Allocated<AnyObject> = msg_send![web, alloc];
            let view: Option<Retained<AnyObject>> = msg_send![alloc, initWithFrame: bounds, configuration: &*conf];
            let view = view?;
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(mtm.alloc(), frame, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false);
            window.setReleasedWhenClosed(false);
            window.setIgnoresMouseEvents(true);
            window.setHasShadow(false);
            window.setCanHide(false);
            window.setExcludedFromWindowsMenu(true);
            window.setCollectionBehavior(NSWindowCollectionBehavior::Transient | NSWindowCollectionBehavior::IgnoresCycle);
            let _: () = msg_send![&*window, setContentView: &*view];
            // Below everything and never made key: on no screen, but
            // WebKit draws for it.
            window.orderBack(None);
            let _: () = msg_send![&*view, setNavigationDelegate: &*delegate];
            let nsurl = NSURL::URLWithString(&NSString::from_str(url))?;
            let class = AnyClass::get(c"NSURLRequest")?;
            let request: Option<Retained<AnyObject>> = msg_send![class, requestWithURL: &*nsurl];
            let _: Option<Retained<AnyObject>> = msg_send![&*view, loadRequest: &*request?];
            Some(WebPage { view, window, _delegate: delegate, shared, asked: Cell::new(false) })
        }
    }
}

/// A page loading off screen.
pub struct WebPage {
    view: Retained<AnyObject>,
    window: Retained<NSWindow>,
    _delegate: Retained<Delegate>,
    shared: Rc<Shared>,
    asked: Cell<bool>,
}

impl Page for WebPage {
    fn next_event(&mut self) -> Option<PageEvent> {
        self.shared.events.borrow_mut().pop_front()
    }

    fn snapshot(&mut self) {
        if self.asked.replace(true) {
            return;
        }
        let shared = self.shared.clone();
        let done = RcBlock::new(move |image: *mut AnyObject, _error: *mut NSError| {
            // SAFETY: WebKit hands a live NSImage or nil; TIFFRepresentation
            // returns NSData or nil.
            let data: Option<Retained<NSData>> = unsafe { image.as_ref().and_then(|image| msg_send![image, TIFFRepresentation]) };
            shared.push(data.map_or(PageEvent::ShotFailed, |d| PageEvent::Shot(d.to_vec())));
        });
        let Some(class) = AnyClass::get(c"WKSnapshotConfiguration") else {
            self.shared.push(PageEvent::ShotFailed);
            return;
        };
        // SAFETY: WKSnapshotConfiguration's setters on a new object; the
        // snapshot method copies its block and calls it once on the main
        // thread.
        unsafe {
            let conf: Retained<AnyObject> = msg_send![class, new];
            let _: () = msg_send![&*conf, setSnapshotWidth: &*NSNumber::new_f64(SHOT_WIDTH)];
            let _: () = msg_send![&*self.view, takeSnapshotWithConfiguration: &*conf, completionHandler: &*done];
        }
    }
}

impl Drop for WebPage {
    fn drop(&mut self) {
        // SAFETY: WKWebView and NSWindow methods on the live objects; nil
        // clears the view's delegate.
        unsafe {
            let _: () = msg_send![&*self.view, setNavigationDelegate: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*self.view, stopLoading];
        }
        self.window.orderOut(None);
        self.window.close();
    }
}
