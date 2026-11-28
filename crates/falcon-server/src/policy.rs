//! Who may call which route (BEC-1B).
//!
//! The API listens on loopback only, but browsers can reach loopback too, so
//! a web page must not be able to use it. The rules here are pure so they can
//! be tested without a socket:
//!
//! * [`host_allowed`]: the `Host` header must name the server itself (blocks
//!   DNS rebinding, where a page's own hostname resolves to 127.0.0.1).
//! * [`kind`]: who is calling, from the `Origin` and `Sec-Fetch-Site`
//!   headers browsers always add and scripts cannot forge. A request with
//!   neither header is a local non-browser program (a script, the CLI, an MCP
//!   client). That is a deliberate, documented trust decision, not proof of
//!   authorization: any local process can call the API, as with Eagle's.
//! * [`tier`]: how risky a route is.
//! * [`decide`]: the verdict. A web page (any `http(s)` origin, `null`, or a
//!   cross-site request without an origin) gets discovery only. A browser
//!   extension gets the API only when its origin was approved by the user,
//!   and never the routes that take filesystem paths.

use std::collections::HashSet;

use serde_json::{Map, Value};

/// How sensitive a route is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// Whether the app is there, its version: no library data.
    Discovery,
    /// Library, folder, tag and asset information.
    Read,
    /// Adds an asset from a URL or data (the safe fetch pipeline).
    Import,
    /// Any other change.
    Mutate,
    /// Takes a filesystem path from the request.
    FsPath,
}

/// Who is calling.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    /// No browser headers: a local program.
    Local,
    /// A page the server itself served (its playground) or a user typing the URL.
    SameOrigin,
    /// A browser extension, by its normalized origin.
    Extension(String),
    /// Any web page or an unidentifiable cross-site request.
    Website,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Serve everything the route offers.
    Allow,
    /// Serve discovery routes with the minimal answer only.
    DiscoveryOnly,
    Forbidden(&'static str),
}

const EXTENSION_SCHEMES: [&str; 3] = ["chrome-extension://", "moz-extension://", "safari-web-extension://"];

/// The `Host` values the server answers to on `port`. `0.0.0.0` is left out:
/// no legitimate client names it, and some browsers send it for pages.
pub fn host_allowed(host: &str, port: u16) -> bool {
    let h = host.to_ascii_lowercase();
    h == format!("localhost:{port}") || h == format!("127.0.0.1:{port}") || h == format!("[::1]:{port}")
}

/// A browser extension origin in canonical form, or None for anything else.
/// Web origins are never accepted here: a page can't prove it is an extension.
pub fn normalize_origin(origin: &str) -> Option<String> {
    let o = origin.trim().trim_end_matches('/').to_ascii_lowercase();
    let scheme = EXTENSION_SCHEMES.iter().find(|s| o.starts_with(*s))?;
    let host = &o[scheme.len()..];
    let ok = !host.is_empty() && host.len() <= 64 && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    ok.then_some(o)
}

fn is_own_origin(origin: &str, port: u16) -> bool {
    let o = origin.to_ascii_lowercase();
    o == format!("http://localhost:{port}") || o == format!("http://127.0.0.1:{port}") || o == format!("http://[::1]:{port}")
}

pub fn kind(origin: Option<&str>, fetch_site: Option<&str>, port: u16) -> Kind {
    match (origin, fetch_site) {
        (None, None) => Kind::Local,
        (Some(o), site) => {
            if let Some(ext) = normalize_origin(o) {
                Kind::Extension(ext)
            } else if is_own_origin(o, port) && matches!(site, None | Some("same-origin")) {
                Kind::SameOrigin
            } else {
                Kind::Website
            }
        }
        // A browser request without an Origin: user navigation or same-origin
        // is fine; a cross-site one (an image tag, a no-cors fetch) is not.
        (None, Some("none" | "same-origin")) => Kind::SameOrigin,
        (None, Some(_)) => Kind::Website,
    }
}

const DISCOVERY: [&str; 4] = ["/", "/api/application/info", "/api/v2/app/info", "/api/v2/playground"];
const FS_PATHS: [&str; 6] = [
    "/api/item/addFromPath",
    "/api/item/addFromPaths",
    "/api/item/setCustomThumbnail",
    "/api/v2/item/setCustomThumbnail",
    "/api/library/icon",
    "/api/v2/library/icon",
];
const SWITCH: [&str; 2] = ["/api/library/switch", "/api/v2/library/switch"];
const IMPORT: [&str; 3] = ["/api/item/addFromURL", "/api/item/addFromURLs", "/api/v2/item/add"];

/// Whether a library switch takes the library path from the request.
pub fn is_switch(path: &str) -> bool {
    SWITCH.contains(&path)
}

fn has_path_key(args: &Map<String, Value>) -> bool {
    args.contains_key("path") || args.get("items").and_then(Value::as_array).is_some_and(|a| a.iter().any(|e| e.get("path").is_some()))
}

/// The risk of a route. `post` is whether the method changes state by
/// convention; two GET routes that change state are listed by name.
pub fn tier(post: bool, path: &str, args: &Map<String, Value>) -> Tier {
    if DISCOVERY.contains(&path) {
        Tier::Discovery
    } else if FS_PATHS.contains(&path) || SWITCH.contains(&path) || (path == "/api/v2/item/add" && has_path_key(args)) {
        Tier::FsPath
    } else if IMPORT.contains(&path) {
        Tier::Import
    } else if post || path.starts_with("/api/preferences/") {
        Tier::Mutate
    } else {
        Tier::Read
    }
}

/// The verdict for a request that already passed the Host / token check.
/// `token_ok` is a valid Developer token: explicit authorization.
pub fn decide(tier: Tier, kind: &Kind, approved: &HashSet<String>, token_ok: bool) -> Verdict {
    if token_ok {
        return Verdict::Allow;
    }
    match kind {
        Kind::Local | Kind::SameOrigin => Verdict::Allow,
        Kind::Extension(o) if approved.contains(o) => match tier {
            Tier::FsPath => Verdict::Forbidden("Filesystem paths are not available to browser extensions."),
            _ => Verdict::Allow,
        },
        Kind::Extension(_) | Kind::Website => match tier {
            Tier::Discovery => Verdict::DiscoveryOnly,
            _ => Verdict::Forbidden("This origin is not allowed to use the API."),
        },
    }
}

/// The CORS answer for a request: `Some("*")` for discovery (nothing in it
/// is sensitive), the exact origin for an allowed browser caller, else none.
pub fn cors_origin(tier: Tier, kind: &Kind, verdict: &Verdict, origin: Option<&str>) -> Option<String> {
    match verdict {
        Verdict::Forbidden(_) => None,
        _ if tier == Tier::Discovery => Some("*".to_string()),
        Verdict::Allow => match kind {
            Kind::Extension(_) | Kind::SameOrigin => origin.map(str::to_string),
            _ => None,
        },
        Verdict::DiscoveryOnly => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Map<String, Value> {
        Map::new()
    }

    #[test]
    fn host_header_must_name_the_server() {
        for h in ["localhost:41596", "127.0.0.1:41596", "[::1]:41596", "LOCALHOST:41596"] {
            assert!(host_allowed(h, 41596), "{h}");
        }
        for h in ["evil.example:41596", "localhost:41597", "0.0.0.0:41596", "localhost", "127.0.0.1.evil.example:41596", ""] {
            assert!(!host_allowed(h, 41596), "{h}");
        }
    }

    #[test]
    fn caller_kinds() {
        assert_eq!(kind(None, None, 1), Kind::Local);
        assert_eq!(kind(Some("https://www.pinterest.com"), Some("cross-site"), 1), Kind::Website);
        assert_eq!(kind(Some("null"), None, 1), Kind::Website);
        assert_eq!(kind(None, Some("cross-site"), 1), Kind::Website, "no Origin is not proof of anything for a browser");
        assert_eq!(kind(None, Some("same-site"), 1), Kind::Website);
        assert_eq!(kind(None, Some("none"), 1), Kind::SameOrigin);
        assert_eq!(kind(Some("http://localhost:1"), Some("same-origin"), 1), Kind::SameOrigin);
        assert_eq!(kind(Some("http://localhost:2"), Some("cross-site"), 1), Kind::Website, "another local port is another site");
        assert_eq!(kind(Some("http://localhost:1"), Some("cross-site"), 1), Kind::Website);
        assert_eq!(kind(Some("chrome-extension://ABCdef/"), Some("none"), 1), Kind::Extension("chrome-extension://abcdef".into()));
        assert_eq!(kind(Some("https://chrome-extension.evil.example"), None, 1), Kind::Website);
    }

    #[test]
    fn only_extension_origins_normalize() {
        assert_eq!(normalize_origin(" moz-extension://1234-abcd "), Some("moz-extension://1234-abcd".into()));
        assert_eq!(normalize_origin("safari-web-extension://AB-12/"), Some("safari-web-extension://ab-12".into()));
        for bad in ["https://example.com", "chrome-extension://", "chrome-extension://a/b", "chrome-extension://a b", "ftp://x", ""] {
            assert_eq!(normalize_origin(bad), None, "{bad}");
        }
    }

    #[test]
    fn tiers() {
        assert_eq!(tier(false, "/", &none()), Tier::Discovery);
        assert_eq!(tier(false, "/api/library/info", &none()), Tier::Read);
        assert_eq!(tier(true, "/api/item/addFromURL", &none()), Tier::Import);
        assert_eq!(tier(true, "/api/item/addFromPath", &none()), Tier::FsPath);
        assert_eq!(tier(true, "/api/library/switch", &none()), Tier::FsPath);
        assert_eq!(tier(true, "/api/folder/create", &none()), Tier::Mutate);
        assert_eq!(tier(false, "/api/preferences/collect/on", &none()), Tier::Mutate, "a GET that changes state");
        let mut a = Map::new();
        assert_eq!(tier(true, "/api/v2/item/add", &a), Tier::Import);
        a.insert("path".into(), Value::from("/etc/hosts"));
        assert_eq!(tier(true, "/api/v2/item/add", &a), Tier::FsPath);
        let mut b = Map::new();
        b.insert("items".into(), serde_json::json!([{"url": "https://x"}, {"path": "/etc/hosts"}]));
        assert_eq!(tier(true, "/api/v2/item/add", &b), Tier::FsPath, "a path hidden in a batch");
    }

    #[test]
    fn websites_get_discovery_only() {
        let approved = HashSet::new();
        for t in [Tier::Read, Tier::Import, Tier::Mutate, Tier::FsPath] {
            assert!(matches!(decide(t, &Kind::Website, &approved, false), Verdict::Forbidden(_)), "{t:?}");
        }
        assert_eq!(decide(Tier::Discovery, &Kind::Website, &approved, false), Verdict::DiscoveryOnly);
    }

    #[test]
    fn extensions_need_approval_and_never_get_paths() {
        let ext = Kind::Extension("chrome-extension://abc".into());
        let mut approved = HashSet::new();
        assert!(matches!(decide(Tier::Import, &ext, &approved, false), Verdict::Forbidden(_)));
        assert_eq!(decide(Tier::Discovery, &ext, &approved, false), Verdict::DiscoveryOnly);
        approved.insert("chrome-extension://abc".to_string());
        for t in [Tier::Read, Tier::Import, Tier::Mutate, Tier::Discovery] {
            assert_eq!(decide(t, &ext, &approved, false), Verdict::Allow, "{t:?}");
        }
        assert!(matches!(decide(Tier::FsPath, &ext, &approved, false), Verdict::Forbidden(_)));
        assert_eq!(decide(Tier::FsPath, &ext, &approved, true), Verdict::Allow, "the Developer token is explicit authorization");
        assert!(matches!(decide(Tier::Read, &Kind::Extension("chrome-extension://other".into()), &approved, false), Verdict::Forbidden(_)));
    }

    #[test]
    fn local_programs_are_served_as_before() {
        let approved = HashSet::new();
        for t in [Tier::Read, Tier::Import, Tier::Mutate, Tier::FsPath, Tier::Discovery] {
            assert_eq!(decide(t, &Kind::Local, &approved, false), Verdict::Allow);
        }
    }

    #[test]
    fn cors_is_never_a_wildcard_for_library_data() {
        let ext = Kind::Extension("chrome-extension://abc".into());
        assert_eq!(cors_origin(Tier::Discovery, &Kind::Website, &Verdict::DiscoveryOnly, Some("https://x.com")), Some("*".into()));
        assert_eq!(cors_origin(Tier::Read, &ext, &Verdict::Allow, Some("chrome-extension://abc")), Some("chrome-extension://abc".into()));
        assert_eq!(cors_origin(Tier::Read, &Kind::Website, &Verdict::Forbidden("x"), Some("https://x.com")), None);
        assert_eq!(cors_origin(Tier::Read, &Kind::Local, &Verdict::Allow, None), None);
    }
}
