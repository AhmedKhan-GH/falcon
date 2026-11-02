//! Web pages in the viewer: which page an item opens, and (on macOS) a
//! `WKWebView` laid over the viewer's area of the window.
//!
//! The web view is a native AppKit view added on top of the window's
//! content view, so egui can't draw over it. Each frame the viewer asks for
//! it with [`WebView::want`] and the app places it with [`WebView::sync`]
//! once the frame is drawn: a frame that doesn't ask hides it and unloads
//! the page (so a video stops playing), and a frame where an egui window,
//! menu, popup or modal overlaps its area hides it until that closes. When
//! hidden it hands the keyboard back to egui. Tests never create one.
//!
//! MHTML archives are unpacked first (`crate::web_archive`); the web view
//! owns the unpacked folder, which goes when no frame asks for it.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Rect};

/// What an item opens in the web viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// A web address (`.url` bookmarks).
    Remote(String),
    /// A local HTML file, read from its own folder.
    Local(PathBuf),
    /// An MHTML archive, the file itself: unpacked with
    /// [`WebView::archive`], then shown as the `Local` page it unpacks to.
    Archive(PathBuf),
    /// Shown as a still: a bookmark whose address isn't a web address, or
    /// an archive that can't be unpacked (or with no WebKit).
    Still { url: Option<String> },
}

/// Whether the web viewer shows items of this extension.
pub fn is_web(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "url" | "html" | "mhtml")
}

pub use crate::page_shot::is_web_address;

/// The page for an item of `ext` with the stored `url`, whose file is
/// `file`: bookmarks open their stored address, else the one in the file
/// (older imports didn't store it); HTML opens the file itself.
pub fn page_for(ext: &str, url: &str, file: &Path) -> Option<Page> {
    match ext.to_lowercase().as_str() {
        "url" => {
            let url = Some(url.trim().to_string()).filter(|u| !u.is_empty()).or_else(|| falcon_library::weblink::read(file));
            Some(match url {
                Some(u) if is_web_address(&u) => Page::Remote(u.trim().to_string()),
                url => Page::Still { url },
            })
        }
        "html" => Some(Page::Local(file.to_path_buf())),
        "mhtml" => Some(Page::Archive(file.to_path_buf())),
        _ => None,
    }
}

/// Where a view goes in AppKit's coordinates (`[x, y, width, height]` in
/// points) for `rect` in egui points, given egui's zoom factor, the height
/// of the content view in points and whether that view's y axis points
/// down (`isFlipped`).
pub fn appkit_frame(rect: Rect, zoom: f32, view_height: f64, flipped: bool) -> [f64; 4] {
    let z = f64::from(zoom);
    let (x, w, h) = (f64::from(rect.left()) * z, f64::from(rect.width().max(0.0)) * z, f64::from(rect.height().max(0.0)) * z);
    let y = if flipped { f64::from(rect.top()) * z } else { view_height - f64::from(rect.bottom()) * z };
    [x, y, w, h]
}

/// Whether anything egui draws above the page overlaps `rect`: a modal,
/// or a window, menu or popup area (tooltips and the page's own layer
/// don't count).
pub fn covered(rect: Rect, modal: bool, overlays: impl IntoIterator<Item = Rect>) -> bool {
    modal || overlays.into_iter().any(|r| r.intersects(rect))
}

/// The areas egui draws above the central panel this frame.
fn overlays(ctx: &egui::Context) -> (bool, Vec<Rect>) {
    ctx.memory(|m| {
        let rects = m
            .areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| matches!(l.order, egui::Order::Middle | egui::Order::Foreground))
            .filter_map(|l| m.area_rect(l.id))
            .collect();
        (m.top_modal_layer().is_some(), rects)
    })
}

/// What the viewer asks of the web view this frame.
pub struct Want {
    /// The item shown; a new one loads its page.
    pub key: String,
    pub page: Page,
    /// Where the page goes, in egui points.
    pub rect: Rect,
}

/// Where the page is, for the viewer's bar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nav {
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    /// The address shown now (after following links), and the page's title.
    pub url: Option<String>,
    pub title: Option<String>,
}

/// The viewer's web view, created the first time a page is shown.
#[derive(Default)]
pub struct WebView {
    want: Option<Want>,
    /// The item whose page is loaded.
    loaded: Option<String>,
    /// The MHTML archive unpacked for the page.
    archive: crate::web_archive::Archives,
    #[cfg(target_os = "macos")]
    native: Option<mac::Native>,
}

impl WebView {
    /// Whether pages can be shown live (macOS with WebKit).
    pub fn available() -> bool {
        #[cfg(target_os = "macos")]
        {
            mac::available()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// The archive of item `key` (file `file`), unpacked for this view;
    /// asking keeps it for this frame.
    pub fn archive(&mut self, key: &str, file: &Path, ctx: &egui::Context) -> crate::web_archive::State {
        self.archive.get(key, file, ctx)
    }

    /// Asks for the page to be shown this frame.
    pub fn want(&mut self, want: Want) {
        self.want = Some(want);
    }

    /// Places, shows or hides the web view after the frame is drawn.
    pub fn sync(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        self.archive.end_frame();
        #[cfg(target_os = "macos")]
        self.sync_with(ctx, |n| n.attach(frame));
        #[cfg(not(target_os = "macos"))]
        {
            self.want = None;
            let _ = (ctx, frame);
        }
    }

    /// The same for a further window (a preview window), found by its
    /// title: egui hands an immediate viewport no window handle, so the
    /// NSWindow is looked up the way the window's opacity is set. Until
    /// the window exists the page waits.
    pub fn sync_window(&mut self, ctx: &egui::Context, title: &str) {
        self.archive.end_frame();
        #[cfg(target_os = "macos")]
        self.sync_with(ctx, |n| n.attach_titled(title));
        #[cfg(not(target_os = "macos"))]
        {
            self.want = None;
            let _ = (ctx, title);
        }
    }

    #[cfg(target_os = "macos")]
    fn sync_with(&mut self, ctx: &egui::Context, attach: impl FnOnce(&mac::Native) -> bool) {
        let want = self.want.take();
        {
            let Some(want) = want else {
                if let Some(n) = &self.native {
                    n.hide();
                    if self.loaded.take().is_some() {
                        n.unload();
                    }
                }
                return;
            };
            if self.native.is_none() {
                self.native = mac::Native::new();
            }
            let Some(n) = &self.native else { return };
            if !attach(n) {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            if self.loaded.as_deref() != Some(&want.key) {
                n.load(&want.page);
                self.loaded = Some(want.key.clone());
            }
            let (modal, areas) = overlays(ctx);
            if covered(want.rect, modal, areas) || want.rect.width() < 1.0 || want.rect.height() < 1.0 {
                n.hide();
            } else {
                n.place(want.rect, ctx.zoom_factor());
            }
            // The title and the back and forward buttons follow the page.
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    /// The page's address, title and history, when one is loaded.
    pub fn nav(&self) -> Nav {
        #[cfg(target_os = "macos")]
        if self.loaded.is_some()
            && let Some(n) = &self.native
        {
            return n.nav();
        }
        Nav::default()
    }

    pub fn back(&self) {
        self.call(Cmd::Back);
    }

    pub fn forward(&self) {
        self.call(Cmd::Forward);
    }

    pub fn reload(&self) {
        self.call(Cmd::Reload);
    }

    pub fn stop(&self) {
        self.call(Cmd::Stop);
    }

    fn call(&self, cmd: Cmd) {
        #[cfg(target_os = "macos")]
        if self.loaded.is_some()
            && let Some(n) = &self.native
        {
            n.call(cmd);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cmd;
    }
}

#[derive(Clone, Copy)]
enum Cmd {
    Back,
    Forward,
    Reload,
    Stop,
}

#[cfg(target_os = "macos")]
mod mac {
    //! The `WKWebView`. Every message below is sent on the main thread (the
    //! app draws there; `Native::new` checks) to live objects: the web view
    //! we retain, the window's content view from winit's window handle, and
    //! the objects AppKit returns, checked for nil.

    use super::{Cmd, Nav, Page, Rect, appkit_frame};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{MainThreadMarker, msg_send};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[link(name = "WebKit", kind = "framework")]
    unsafe extern "C" {}

    fn class(name: &std::ffi::CStr) -> Option<&'static AnyClass> {
        AnyClass::get(name)
    }

    pub fn available() -> bool {
        class(c"WKWebView").is_some()
    }

    fn ns_string(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }

    fn string(obj: Option<Retained<AnyObject>>) -> Option<String> {
        let s = obj?.downcast::<NSString>().ok()?.to_string();
        (!s.is_empty()).then_some(s)
    }

    pub struct Native {
        view: Retained<AnyObject>,
    }

    impl Native {
        pub fn new() -> Option<Native> {
            MainThreadMarker::new()?;
            let (web, conf) = (class(c"WKWebView")?, class(c"WKWebViewConfiguration")?);
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
            // SAFETY: plain alloc/init of WebKit classes on the main thread.
            let view: Retained<AnyObject> = unsafe {
                let conf: Retained<AnyObject> = msg_send![conf, new];
                let alloc: Allocated<AnyObject> = msg_send![web, alloc];
                let view: Option<Retained<AnyObject>> = msg_send![alloc, initWithFrame: frame, configuration: &*conf];
                let view = view?;
                let _: () = msg_send![&*view, setAllowsBackForwardNavigationGestures: true];
                let _: () = msg_send![&*view, setHidden: true];
                // Files dropped on the page go to falcon's import (the
                // content view below), not to a page load.
                let _: () = msg_send![&*view, unregisterDraggedTypes];
                view
            };
            Some(Native { view })
        }

        /// Puts the web view on the window's content view, if it isn't
        /// there; false when the window can't be reached.
        pub fn attach(&self, frame: &eframe::Frame) -> bool {
            let Ok(handle) = frame.window_handle() else { return false };
            let RawWindowHandle::AppKit(h) = handle.as_raw() else { return false };
            let content = h.ns_view.as_ptr().cast::<AnyObject>();
            // SAFETY: `ns_view` is winit's live content view.
            unsafe { self.attach_to(content) };
            true
        }

        /// Puts the web view on the content view of the window titled
        /// `title`; false while there is no such window.
        pub fn attach_titled(&self, title: &str) -> bool {
            let Some(mtm) = MainThreadMarker::new() else { return false };
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            let Some(window) = app.windows().iter().find(|w| w.title().to_string() == title) else { return false };
            let Some(content) = window.contentView() else { return false };
            let content = Retained::as_ptr(&content).cast::<AnyObject>().cast_mut();
            // SAFETY: the window's live content view, retained by the
            // window for the duration of the call.
            unsafe { self.attach_to(content) };
            true
        }

        /// # Safety
        /// `content` must be a live NSView.
        unsafe fn attach_to(&self, content: *mut AnyObject) {
            // SAFETY: superview and addSubview: are NSView methods on live views.
            unsafe {
                let parent: *mut AnyObject = msg_send![&*self.view, superview];
                if parent != content {
                    let _: () = msg_send![&*content, addSubview: &*self.view];
                }
            }
        }

        fn superview(&self) -> Option<Retained<AnyObject>> {
            // SAFETY: NSView getter.
            unsafe { msg_send![&*self.view, superview] }
        }

        /// Shows the view over `rect` (egui points).
        pub fn place(&self, rect: Rect, zoom: f32) {
            let Some(parent) = self.superview() else { return };
            // SAFETY: NSView getters and setters on live views.
            unsafe {
                let bounds: NSRect = msg_send![&*parent, bounds];
                let flipped: bool = msg_send![&*parent, isFlipped];
                let [x, y, w, h] = appkit_frame(rect, zoom, bounds.size.height, flipped);
                let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
                let old: NSRect = msg_send![&*self.view, frame];
                if old != frame {
                    let _: () = msg_send![&*self.view, setFrame: frame];
                }
                let hidden: bool = msg_send![&*self.view, isHidden];
                if hidden {
                    let _: () = msg_send![&*self.view, setHidden: false];
                }
            }
        }

        /// Hides the view, giving the keyboard back to egui's view if the
        /// page had it.
        pub fn hide(&self) {
            // SAFETY: NSView and NSWindow methods on live objects; the
            // first responder is checked to be an NSView before asking
            // whether it is inside the web view.
            unsafe {
                let hidden: bool = msg_send![&*self.view, isHidden];
                if hidden {
                    return;
                }
                let window: Option<Retained<AnyObject>> = msg_send![&*self.view, window];
                if let (Some(window), Some(parent), Some(nsview)) = (window, self.superview(), class(c"NSView")) {
                    let first: Option<Retained<AnyObject>> = msg_send![&*window, firstResponder];
                    if let Some(first) = first {
                        let is_view: bool = msg_send![&*first, isKindOfClass: nsview];
                        let inside: bool = is_view && msg_send![&*first, isDescendantOf: &*self.view];
                        if inside {
                            let _: Bool = msg_send![&*window, makeFirstResponder: &*parent];
                        }
                    }
                }
                let _: () = msg_send![&*self.view, setHidden: true];
            }
        }

        pub fn load(&self, page: &Page) {
            let Some(nsurl) = class(c"NSURL") else { return };
            // SAFETY: NSURL and NSURLRequest class constructors (nil is
            // checked) and WKWebView's load methods on the live view.
            unsafe {
                match page {
                    Page::Remote(url) => {
                        let Some(req) = class(c"NSURLRequest") else { return };
                        let u: Option<Retained<AnyObject>> = msg_send![nsurl, URLWithString: &*ns_string(url)];
                        let Some(u) = u else { return };
                        let r: Option<Retained<AnyObject>> = msg_send![req, requestWithURL: &*u];
                        if let Some(r) = r {
                            let _: Option<Retained<AnyObject>> = msg_send![&*self.view, loadRequest: &*r];
                        }
                    }
                    Page::Local(path) => {
                        let dir = path.parent().unwrap_or(path);
                        let file: Option<Retained<AnyObject>> = msg_send![nsurl, fileURLWithPath: &*ns_string(&path.to_string_lossy())];
                        let dir: Option<Retained<AnyObject>> = msg_send![nsurl, fileURLWithPath: &*ns_string(&dir.to_string_lossy())];
                        if let (Some(file), Some(dir)) = (file, dir) {
                            let _: Option<Retained<AnyObject>> = msg_send![&*self.view, loadFileURL: &*file, allowingReadAccessToURL: &*dir];
                        }
                    }
                    Page::Archive(_) | Page::Still { .. } => self.unload(),
                }
            }
        }

        /// Stops the page and leaves a blank one (so sound stops).
        pub fn unload(&self) {
            // SAFETY: WKWebView methods on the live view.
            unsafe {
                let _: () = msg_send![&*self.view, stopLoading];
                let _: Option<Retained<AnyObject>> = msg_send![&*self.view, loadHTMLString: &*ns_string(""), baseURL: std::ptr::null::<AnyObject>()];
            }
        }

        pub fn nav(&self) -> Nav {
            // SAFETY: WKWebView getters on the live view; NSURL's
            // absoluteString on a non-nil URL.
            unsafe {
                let url: Option<Retained<AnyObject>> = msg_send![&*self.view, URL];
                let url = url.and_then(|u| {
                    let s: Option<Retained<AnyObject>> = msg_send![&*u, absoluteString];
                    string(s)
                });
                let title: Option<Retained<AnyObject>> = msg_send![&*self.view, title];
                Nav {
                    can_back: msg_send![&*self.view, canGoBack],
                    can_forward: msg_send![&*self.view, canGoForward],
                    loading: msg_send![&*self.view, isLoading],
                    url: url.filter(|u| u != "about:blank"),
                    title: string(title),
                }
            }
        }

        pub fn call(&self, cmd: Cmd) {
            // SAFETY: WKWebView navigation methods on the live view.
            unsafe {
                match cmd {
                    Cmd::Back => {
                        let _: Option<Retained<AnyObject>> = msg_send![&*self.view, goBack];
                    }
                    Cmd::Forward => {
                        let _: Option<Retained<AnyObject>> = msg_send![&*self.view, goForward];
                    }
                    Cmd::Reload => {
                        let _: Option<Retained<AnyObject>> = msg_send![&*self.view, reload];
                    }
                    Cmd::Stop => {
                        let _: () = msg_send![&*self.view, stopLoading];
                    }
                }
            }
        }
    }

    impl Drop for Native {
        fn drop(&mut self) {
            // SAFETY: NSView method on the live view.
            unsafe {
                let _: () = msg_send![&*self.view, removeFromSuperview];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, pos2};

    #[test]
    fn web_extensions() {
        for e in ["url", "URL", "html", "mhtml"] {
            assert!(is_web(e), "{e}");
        }
        assert!(!is_web("txt") && !is_web("pdf") && !is_web("webloc"));
        for e in ["htm", "mht"] {
            assert!(!is_web(e), "{e} is a plain item, as in Eagle");
        }
    }

    #[test]
    fn only_web_addresses_load() {
        assert!(is_web_address("https://example.com/a?b=1"));
        assert!(is_web_address("HTTP://example.com"));
        assert!(!is_web_address("file:///etc/passwd"));
        assert!(!is_web_address("javascript:alert(1)"));
        assert!(!is_web_address("someapp://open"));
        assert!(!is_web_address("https://exa mple.com"));
        assert!(!is_web_address("https://"));
    }

    #[test]
    fn pages_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.url");
        std::fs::write(&file, "[InternetShortcut]\nURL=https://example.com/from-file\n").unwrap();
        // The stored address wins; else the file's.
        assert_eq!(page_for("url", "https://stored.example/", &file), Some(Page::Remote("https://stored.example/".into())));
        assert_eq!(page_for("url", " ", &file), Some(Page::Remote("https://example.com/from-file".into())));
        // Not a web address: a still, keeping the address for Copy.
        assert_eq!(page_for("url", "file:///tmp/x", &file), Some(Page::Still { url: Some("file:///tmp/x".into()) }));
        assert_eq!(page_for("webloc", "https://example.com/", &file), None);
        let missing = dir.path().join("none.url");
        assert_eq!(page_for("url", "", &missing), Some(Page::Still { url: None }));
        let html = dir.path().join("p.html");
        assert_eq!(page_for("HTML", "", &html), Some(Page::Local(html.clone())));
        let archive = dir.path().join("a.mhtml");
        assert_eq!(page_for("MHTML", "https://source.example/", &archive), Some(Page::Archive(archive.clone())));
        assert_eq!(page_for("mht", "https://source.example/", &archive), None, "not in Eagle's formats");
        assert_eq!(page_for("png", "", &html), None);
    }

    #[test]
    fn frames_follow_the_view_axis_and_zoom() {
        let r = Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 350.0));
        // A flipped view counts y from the top.
        assert_eq!(appkit_frame(r, 1.0, 800.0, true), [100.0, 50.0, 400.0, 300.0]);
        // An unflipped one from the bottom.
        assert_eq!(appkit_frame(r, 1.0, 800.0, false), [100.0, 450.0, 400.0, 300.0]);
        // egui zoomed to 125%: its points are larger than AppKit's.
        assert_eq!(appkit_frame(r, 1.25, 1000.0, true), [125.0, 62.5, 500.0, 375.0]);
        // A collapsed rect never gets a negative size.
        let empty = Rect::from_min_max(pos2(10.0, 10.0), Pos2::new(5.0, 5.0));
        assert_eq!(appkit_frame(empty, 1.0, 100.0, true)[2..], [0.0, 0.0]);
    }

    #[test]
    fn overlapping_areas_cover_the_page() {
        let page = Rect::from_min_max(pos2(200.0, 40.0), pos2(1000.0, 800.0));
        let menu = Rect::from_min_max(pos2(900.0, 100.0), pos2(1100.0, 300.0));
        let aside = Rect::from_min_max(pos2(0.0, 40.0), pos2(190.0, 800.0));
        assert!(covered(page, false, [menu]));
        assert!(!covered(page, false, [aside]));
        assert!(covered(page, true, []), "a modal always hides it");
        assert!(!covered(page, false, []));
    }
}
