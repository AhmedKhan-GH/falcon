//! Opening falcon when the user logs in, through `SMAppService` (macOS 13
//! and later). The setting is not kept in the preferences: the system owns
//! it (the user can switch it off in System Settings ▸ General ▸ Login
//! Items), so it is read from there every time Preferences opens.
//!
//! Only `SMAppService` is used. Before macOS 13 the class is missing and
//! the setting is reported as unavailable; the deprecated
//! `LSSharedFileList` API is deliberately not implemented. Registration
//! also needs a bundled app (`scripts/bundle.sh`): under `cargo run` there
//! is no bundle to register, and the setting is unavailable too.

/// Whether falcon opens at login.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Status {
    #[default]
    Off,
    On,
    /// Registered, but the user still has to allow it in System Settings.
    NeedsApproval,
    /// Can't be used here, with the reason to show.
    Unavailable(String),
}

impl Status {
    /// Whether the checkbox shows as ticked.
    pub fn is_on(&self) -> bool {
        matches!(self, Status::On | Status::NeedsApproval)
    }
}

use crate::locale::{tr, tr_key};

/// Maps `SMAppServiceStatus` (0 not registered, 1 enabled, 2 requires
/// approval, 3 not found).
pub fn status_from_code(code: isize) -> Status {
    match code {
        0 => Status::Off,
        1 => Status::On,
        2 => Status::NeedsApproval,
        3 => Status::Unavailable(tr("The system can't find the app to open at login").into()),
        _ => Status::Unavailable(tr("Unknown login item status").into()),
    }
}

/// Whether a main bundle path is an app bundle that can be registered.
pub fn is_app_bundle(path: &str) -> bool {
    path.trim_end_matches('/').ends_with(".app")
}

const NOT_BUNDLED: &str = tr_key("Only the app bundle can open at login");

/// The current state, read from the system.
pub fn status() -> Status {
    platform::status()
}

/// Registers or unregisters falcon as a login item and returns the state
/// afterwards; the error is the system's own message.
pub fn set(on: bool) -> Result<Status, String> {
    platform::set(on)
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    use objc2_foundation::{NSBundle, NSError};

    use super::{NOT_BUNDLED, Status, is_app_bundle, status_from_code};
    use crate::locale::tr;

    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" {}

    fn bundled() -> bool {
        let bundle = NSBundle::mainBundle();
        bundle.bundleIdentifier().is_some() && is_app_bundle(&bundle.bundlePath().to_string())
    }

    /// `+[SMAppService mainAppService]`, or why there isn't one.
    fn service() -> Result<Retained<AnyObject>, Status> {
        if !bundled() {
            return Err(Status::Unavailable(tr(NOT_BUNDLED).into()));
        }
        let Some(class) = AnyClass::get(c"SMAppService") else {
            return Err(Status::Unavailable(tr("Opening at login needs macOS 13 or later").into()));
        };
        let service: Option<Retained<AnyObject>> = unsafe { msg_send![class, mainAppService] };
        service.ok_or_else(|| Status::Unavailable(tr("The system has no login item for this app").into()))
    }

    fn read(service: &AnyObject) -> Status {
        let code: isize = unsafe { msg_send![service, status] };
        status_from_code(code)
    }

    pub fn status() -> Status {
        match service() {
            Ok(s) => read(&s),
            Err(unavailable) => unavailable,
        }
    }

    pub fn set(on: bool) -> Result<Status, String> {
        let service = service().map_err(|s| match s {
            Status::Unavailable(why) => why,
            _ => String::new(),
        })?;
        let done: Result<(), Retained<NSError>> =
            if on { unsafe { msg_send![&*service, registerAndReturnError: _] } } else { unsafe { msg_send![&*service, unregisterAndReturnError: _] } };
        done.map_err(|e| e.localizedDescription().to_string())?;
        Ok(read(&service))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Status;
    use crate::locale::tr;

    pub fn status() -> Status {
        Status::Unavailable(tr("Opening at login is only available on macOS").into())
    }

    pub fn set(_on: bool) -> Result<Status, String> {
        Err(tr("Opening at login is only available on macOS").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_until_the_user_turns_it_on() {
        // Nothing is registered by default: the window shows the system's
        // state, and only ticking the box calls `set`.
        assert_eq!(Status::default(), Status::Off);
        assert!(!Status::default().is_on());
        assert!(!status_from_code(0).is_on());
    }

    #[test]
    fn codes_map_to_states() {
        assert_eq!(status_from_code(0), Status::Off);
        assert_eq!(status_from_code(1), Status::On);
        assert_eq!(status_from_code(2), Status::NeedsApproval);
        assert!(matches!(status_from_code(3), Status::Unavailable(_)));
        assert!(matches!(status_from_code(9), Status::Unavailable(_)));
    }

    #[test]
    fn only_app_bundles_count() {
        assert!(is_app_bundle("/Applications/Falcon.app"));
        assert!(is_app_bundle("/Applications/Falcon.app/"));
        assert!(!is_app_bundle("/Users/me/falcon/target/debug"));
        assert!(!is_app_bundle("/Users/me/Falcon.app/Contents/MacOS"));
    }

    #[test]
    fn approval_counts_as_ticked() {
        assert!(Status::On.is_on() && Status::NeedsApproval.is_on());
        assert!(!Status::Off.is_on() && !Status::Unavailable("x".into()).is_on());
    }
}
