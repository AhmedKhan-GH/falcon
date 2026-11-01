//! System notifications, where Eagle shows its own pop-ups: a screenshot
//! taken, an import or an export finished. Each follows its switch in
//! Preferences ▸ Notification (and the master switch), checked by the
//! caller (`Prefs::tells_*`).
//!
//! On macOS they go through the UserNotifications framework, which needs
//! the app to have a bundle identifier: in `Falcon.app` (made by
//! `scripts/bundle.sh`) they work; run unbundled (`cargo run`) they are
//! dropped, since the framework refuses to start without one. macOS
//! shows them only while falcon is not the active app (it has no
//! delegate asking otherwise), so they never double the in-window notes.
//! The first one asks the user for permission.
//!
//! The UserNotifications crate in the local cache binds an older objc2
//! than the app's, so the few calls needed are sent by name instead.

/// A notification: a title and a line of text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

impl Notice {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Notice { title: title.into(), body: body.into() }
    }

    /// A screenshot went into the library (Eagle's "Screenshot captured").
    pub fn captured(name: &str) -> Self {
        Notice::new("Screenshot captured", name)
    }

    /// An import finished with `added` new items.
    pub fn imported(added: usize) -> Self {
        Notice::new("Import finished", format!("{added} item{} added to the library", if added == 1 { "" } else { "s" }))
    }

    /// An export finished; `message` is the in-window note ("Exported 3 items").
    pub fn exported(message: &str) -> Self {
        Notice::new("Export finished", message)
    }
}

/// Posts a notification, or does nothing where it can't (no bundle
/// identifier, another system).
pub fn post(n: &Notice) {
    platform::post(n);
}

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::OnceLock;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::msg_send;
    use objc2_foundation::{NSBundle, NSString};

    use super::Notice;

    #[link(name = "UserNotifications", kind = "framework")]
    unsafe extern "C" {}

    /// `UNAuthorizationOptionSound | UNAuthorizationOptionAlert`.
    const OPTIONS: usize = (1 << 1) | (1 << 2);

    /// Whether this process can use the framework at all.
    fn bundled() -> bool {
        static BUNDLED: OnceLock<bool> = OnceLock::new();
        *BUNDLED.get_or_init(|| NSBundle::mainBundle().bundleIdentifier().is_some_and(|id| !id.to_string().is_empty()))
    }

    fn center() -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"UNUserNotificationCenter")?;
        // SAFETY: a class method returning the shared center, which needs
        // a bundle identifier (checked by the caller).
        unsafe { msg_send![cls, currentNotificationCenter] }
    }

    pub fn post(n: &Notice) {
        if !bundled() {
            return;
        }
        let Some(center) = center() else { return };
        let (title, body) = (n.title.clone(), n.body.clone());
        // Permission is asked once by the system; later calls answer at
        // once with the user's choice. The notice is added when allowed.
        let handler = RcBlock::new(move |granted: Bool, _error: *mut AnyObject| {
            if granted.as_bool() {
                add(&title, &body);
            }
        });
        // SAFETY: the options are UNAuthorizationOptions bits and the block
        // has the (BOOL, NSError *) signature the method takes.
        unsafe {
            let _: () = msg_send![&center, requestAuthorizationWithOptions: OPTIONS, completionHandler: &*handler];
        }
    }

    fn add(title: &str, body: &str) {
        let Some(center) = center() else { return };
        let (Some(content_cls), Some(request_cls)) = (AnyClass::get(c"UNMutableNotificationContent"), AnyClass::get(c"UNNotificationRequest")) else { return };
        // SAFETY: plain allocation and property setters of the framework's
        // classes, with NSString arguments; a nil trigger delivers at once
        // and a nil completion handler is allowed.
        unsafe {
            let content: Option<Retained<AnyObject>> = msg_send![content_cls, new];
            let Some(content) = content else { return };
            let _: () = msg_send![&content, setTitle: &*NSString::from_str(title)];
            let _: () = msg_send![&content, setBody: &*NSString::from_str(body)];
            let id = NSString::from_str(&format!("falcon-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default()));
            let request: Option<Retained<AnyObject>> = msg_send![request_cls, requestWithIdentifier: &*id, content: &*content, trigger: std::ptr::null::<AnyObject>()];
            let Some(request) = request else { return };
            let _: () = msg_send![&center, addNotificationRequest: &*request, withCompletionHandler: std::ptr::null::<AnyObject>()];
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn post(_n: &super::Notice) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wording() {
        assert_eq!(Notice::imported(1).body, "1 item added to the library");
        assert_eq!(Notice::imported(3).body, "3 items added to the library");
        assert_eq!(Notice::captured("Screenshot - 2026-11-01 09.05.07").title, "Screenshot captured");
        assert_eq!(Notice::exported("Exported 2 items").body, "Exported 2 items");
    }
}
