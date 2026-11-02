//! Eagle's `eagle://` links: `eagle://item/<id>`, `eagle://folder/<id>`
//! and `eagle://smart-folder/<id>`, as Eagle's Copy Link and the API's
//! `/item?id=` redirects make them, and `eagle://openFolder/<id>`, the
//! form Eagle's "added to folder" notices link with, which opens the
//! folder in the app like a folder link. The bundle registers the scheme
//! (`scripts/bundle.sh`); macOS hands a clicked link to the running app as
//! a "get URL" Apple event, which [`install`] catches. The app takes the
//! links with [`take`] each frame.

/// What a link opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Item(String),
    Folder(String),
    Smart(String),
}

/// Reads an `eagle://` link. Anything after the id (`/`, `?`, `#`) is
/// ignored; other schemes and kinds give `None`.
pub fn parse(url: &str) -> Option<Link> {
    let rest = url.trim().strip_prefix("eagle://")?;
    let (kind, id) = rest.split_once('/')?;
    let id = id.split(['/', '?', '#']).next().unwrap_or("");
    if id.is_empty() {
        return None;
    }
    let id = id.to_string();
    match kind {
        "item" => Some(Link::Item(id)),
        "folder" | "openFolder" => Some(Link::Folder(id)),
        "smart-folder" => Some(Link::Smart(id)),
        _ => None,
    }
}

/// The link Copy ▸ Eagle Link puts on the clipboard for a folder.
pub fn folder_link(id: &str) -> String {
    format!("eagle://folder/{id}")
}

/// The link Copy ▸ Eagle Link puts on the clipboard for a smart folder.
pub fn smart_link(id: &str) -> String {
    format!("eagle://smart-folder/{id}")
}

/// An item's link through the local API on `port`, as Eagle's Copy Eagle
/// Link writes it: the API answers it with a redirect to the item's `eagle://item/<id>` link.
pub fn api_item_link(port: u16, id: &str) -> String {
    format!("http://localhost:{port}/item?id={id}")
}

/// A folder's link through the local API on `port`, as Copy Link writes
/// it: the API answers it with a redirect to [`folder_link`].
pub fn api_folder_link(port: u16, id: &str) -> String {
    format!("http://localhost:{port}/folder?id={id}")
}

/// A smart folder's link through the local API on `port`; the API answers
/// it with a redirect to [`smart_link`].
pub fn api_smart_link(port: u16, id: &str) -> String {
    format!("http://localhost:{port}/smart-folder?id={id}")
}

#[cfg(target_os = "macos")]
pub use mac::{install, set_repaint, take};

/// Links can only arrive on macOS.
#[cfg(not(target_os = "macos"))]
pub fn install() {}
#[cfg(not(target_os = "macos"))]
pub fn set_repaint(_ctx: &eframe::egui::Context) {}
#[cfg(not(target_os = "macos"))]
pub fn take() -> Vec<Link> {
    Vec::new()
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::sync::{Mutex, OnceLock};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};

    use super::{Link, parse};

    /// `kInternetEventClass` and `kAEGetURL`, both 'GURL'.
    const GET_URL: u32 = u32::from_be_bytes(*b"GURL");
    /// `keyDirectObject`, '----': the URL inside the event.
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    static LINKS: Mutex<Vec<Link>> = Mutex::new(Vec::new());
    static REPAINT: OnceLock<eframe::egui::Context> = OnceLock::new();

    thread_local! {
        static HANDLER: RefCell<Option<Retained<Handler>>> = const { RefCell::new(None) };
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and Handler has
        // no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FalconLinkHandler"]
        struct Handler;

        impl Handler {
            #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
            fn get_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                // SAFETY: paramDescriptorForKeyword: takes an AEKeyword
                // (a FourCharCode, u32) and returns a descriptor or nil.
                let param: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
                let Some(url) = param.and_then(|p| p.stringValue()) else { return };
                if let Some(link) = parse(&url.to_string()) {
                    LINKS.lock().unwrap_or_else(|p| p.into_inner()).push(link);
                    if let Some(ctx) = REPAINT.get() {
                        ctx.request_repaint();
                    }
                }
            }
        }

        unsafe impl NSObjectProtocol for Handler {}
    );

    /// Starts listening for links. Call it on the main thread before the
    /// event loop runs, so a link that launched the app is not missed.
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        HANDLER.with(|h| {
            let mut h = h.borrow_mut();
            if h.is_some() {
                return;
            }
            let this = Handler::alloc(mtm).set_ivars(());
            let handler: Retained<Handler> = unsafe { msg_send![super(this), init] };
            let manager = NSAppleEventManager::sharedAppleEventManager();
            let target: &AnyObject = &handler;
            // SAFETY: the handler implements the selector with the
            // signature the Apple event manager calls, and stays alive in
            // HANDLER for the life of the thread (the main thread).
            unsafe {
                let _: () = msg_send![
                    &*manager,
                    setEventHandler: target,
                    andSelector: sel!(handleGetURLEvent:withReplyEvent:),
                    forEventClass: GET_URL,
                    andEventID: GET_URL
                ];
            }
            *h = Some(handler);
        });
    }

    /// Lets a link wake the app to handle it.
    pub fn set_repaint(ctx: &eframe::egui::Context) {
        let _ = REPAINT.set(ctx.clone());
    }

    /// The links that arrived since the last call, oldest first.
    pub fn take() -> Vec<Link> {
        std::mem::take(&mut *LINKS.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_links_parse_back() {
        assert_eq!(parse(&folder_link("L1")), Some(Link::Folder("L1".into())));
        assert_eq!(parse(&smart_link("S1")), Some(Link::Smart("S1".into())));
    }

    #[test]
    fn reads_eagle_links() {
        assert_eq!(parse("eagle://item/M3QSGJNQTC2DG"), Some(Link::Item("M3QSGJNQTC2DG".into())));
        assert_eq!(parse("eagle://folder/L1/"), Some(Link::Folder("L1".into())));
        assert_eq!(parse("eagle://smart-folder/S1?x=1"), Some(Link::Smart("S1".into())));
        assert_eq!(parse("eagle://openFolder/L2"), Some(Link::Folder("L2".into())));
        assert_eq!(parse("eagle://item/"), None);
        assert_eq!(parse("eagle://plugin/X"), None);
        assert_eq!(parse("https://item/X"), None);
    }

    #[test]
    fn writes_item_links() {
        assert_eq!(parse("eagle://item/A1"), Some(Link::Item("A1".into())));
        assert_eq!(api_item_link(41600, "A1"), "http://localhost:41600/item?id=A1");
        assert_eq!(api_folder_link(41600, "L1"), "http://localhost:41600/folder?id=L1");
        assert_eq!(api_smart_link(41600, "S1"), "http://localhost:41600/smart-folder?id=S1");
    }
}
