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
    /// A cross-site `no-cors` fetch with no `Origin` and an empty destination.
    /// This is how Safari's extension background page labels its own API
    /// calls (observed), but a web page can send exactly the same request.
    /// The page cannot *read* the answer (it is opaque), which is why a
    /// narrow set of read-only routes may be served to it during an explicit
    /// browser import session, and nothing else.
    Opaque,
    /// Any web page or an unidentifiable cross-site request.
    Website,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Serve everything the route offers.
    Allow,
    /// During a browser import session: serve the route's session-limited
    /// answer (folder and tag names, no paths, no locked folders).
    Session,
    /// During a browser import session: an import that is committed only
    /// after the user confirms it in Falcon.
    Confirm,
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

/// The browser's own labels for a request: `Sec-Fetch-Site`, `-Mode`, `-Dest`.
#[derive(Clone, Copy, Default)]
pub struct Fetch<'a> {
    pub site: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub dest: Option<&'a str>,
}

pub fn kind(origin: Option<&str>, f: Fetch, port: u16) -> Kind {
    let fetch_site = f.site;
    if origin.is_none() && matches!(fetch_site, Some("cross-site" | "same-site")) && f.mode == Some("no-cors") && f.dest == Some("empty") {
        return Kind::Opaque;
    }
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
/// The read-only routes a browser import session may serve (answers are
/// limited, see `session_reply` in `lib.rs`).
pub const SESSION_READS: [&str; 7] = [
    "/api/folder/list",
    "/api/folder/listRecent",
    "/api/tag/all",
    "/api/tag/list",
    "/api/tag/listRecent",
    "/api/library/info",
    "/api/library/history",
];

/// The state of the user-started browser import session, as the policy sees it.
#[derive(Default)]
pub struct SessionView<'a> {
    pub active: bool,
    /// Extension origins the user approved for this session only.
    pub origins: Option<&'a HashSet<String>>,
}

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
pub fn decide(tier: Tier, kind: &Kind, approved: &HashSet<String>, token_ok: bool, path: &str, session: &SessionView) -> Verdict {
    if token_ok {
        return Verdict::Allow;
    }
    let refused = match tier {
        Tier::Discovery => Verdict::DiscoveryOnly,
        _ => Verdict::Forbidden("This origin is not allowed to use the API."),
    };
    let session_read = session.active && tier == Tier::Read && SESSION_READS.contains(&path);
    match kind {
        Kind::Local | Kind::SameOrigin => Verdict::Allow,
        Kind::Extension(o) if approved.contains(o) => match tier {
            Tier::FsPath => Verdict::Forbidden("Filesystem paths are not available to browser extensions."),
            _ => Verdict::Allow,
        },
        Kind::Extension(o) if session.active && session.origins.is_some_and(|s| s.contains(o)) => match tier {
            Tier::Discovery => Verdict::DiscoveryOnly,
            _ if session_read => Verdict::Session,
            Tier::Import => Verdict::Confirm,
            _ => Verdict::Forbidden("Not available in a browser import session."),
        },
        // During a session an unknown extension's import is not refused: it is held for the
        // user's confirmation, which shows who is asking (an extension origin can't be forged by a page).
        Kind::Extension(_) if session.active && tier == Tier::Import => Verdict::Confirm,
        Kind::Opaque if session_read => Verdict::Session,
        Kind::Opaque | Kind::Extension(_) | Kind::Website => refused,
    }
}

/// The CORS answer for a request: `Some("*")` for discovery (nothing in it
/// is sensitive), the exact origin for an allowed browser caller, else none.
pub fn cors_origin(tier: Tier, kind: &Kind, verdict: &Verdict, origin: Option<&str>) -> Option<String> {
    match verdict {
        Verdict::Forbidden(_) => None,
        _ if tier == Tier::Discovery => Some("*".to_string()),
        Verdict::Allow | Verdict::Session | Verdict::Confirm => match kind {
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

    fn k(origin: Option<&str>, site: Option<&str>) -> Kind {
        kind(origin, Fetch { site, mode: None, dest: None }, 1)
    }

    fn d(t: Tier, kind: &Kind, approved: &HashSet<String>, token: bool) -> Verdict {
        decide(t, kind, approved, token, "/api/folder/list", &SessionView::default())
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
        assert_eq!(k(None, None), Kind::Local);
        assert_eq!(k(Some("https://www.pinterest.com"), Some("cross-site")), Kind::Website);
        assert_eq!(k(Some("null"), None), Kind::Website);
        assert_eq!(k(None, Some("cross-site")), Kind::Website, "no Origin is not proof of anything for a browser");
        assert_eq!(k(None, Some("same-site")), Kind::Website);
        assert_eq!(k(None, Some("none")), Kind::SameOrigin);
        assert_eq!(k(Some("http://localhost:1"), Some("same-origin")), Kind::SameOrigin);
        assert_eq!(k(Some("http://localhost:2"), Some("cross-site")), Kind::Website, "another local port is another site");
        assert_eq!(k(Some("http://localhost:1"), Some("cross-site")), Kind::Website);
        assert_eq!(k(Some("chrome-extension://ABCdef/"), Some("none")), Kind::Extension("chrome-extension://abcdef".into()));
        assert_eq!(k(Some("https://chrome-extension.evil.example"), None), Kind::Website);
    }

    #[test]
    fn opaque_requests_are_a_narrow_class() {
        let f = |site, mode, dest| kind(None, Fetch { site, mode, dest }, 1);
        assert_eq!(f(Some("cross-site"), Some("no-cors"), Some("empty")), Kind::Opaque);
        assert_eq!(f(Some("same-site"), Some("no-cors"), Some("empty")), Kind::Opaque);
        // Anything else without an Origin stays a website.
        assert_eq!(f(Some("cross-site"), Some("cors"), Some("empty")), Kind::Website);
        assert_eq!(f(Some("cross-site"), Some("no-cors"), Some("script")), Kind::Website, "script and image loads are not it");
        assert_eq!(f(Some("cross-site"), Some("no-cors"), Some("image")), Kind::Website);
        assert_eq!(f(Some("cross-site"), Some("navigate"), Some("document")), Kind::Website);
        // With an Origin it is never opaque.
        assert_eq!(kind(Some("https://x.example"), Fetch { site: Some("cross-site"), mode: Some("no-cors"), dest: Some("empty") }, 1), Kind::Website);
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
            assert!(matches!(d(t, &Kind::Website, &approved, false), Verdict::Forbidden(_)), "{t:?}");
        }
        assert_eq!(d(Tier::Discovery, &Kind::Website, &approved, false), Verdict::DiscoveryOnly);
    }

    #[test]
    fn extensions_need_approval_and_never_get_paths() {
        let ext = Kind::Extension("chrome-extension://abc".into());
        let mut approved = HashSet::new();
        assert!(matches!(d(Tier::Import, &ext, &approved, false), Verdict::Forbidden(_)));
        assert_eq!(d(Tier::Discovery, &ext, &approved, false), Verdict::DiscoveryOnly);
        approved.insert("chrome-extension://abc".to_string());
        for t in [Tier::Read, Tier::Import, Tier::Mutate, Tier::Discovery] {
            assert_eq!(d(t, &ext, &approved, false), Verdict::Allow, "{t:?}");
        }
        assert!(matches!(d(Tier::FsPath, &ext, &approved, false), Verdict::Forbidden(_)));
        assert_eq!(d(Tier::FsPath, &ext, &approved, true), Verdict::Allow, "the Developer token is explicit authorization");
        assert!(matches!(d(Tier::Read, &Kind::Extension("chrome-extension://other".into()), &approved, false), Verdict::Forbidden(_)));
    }

    #[test]
    fn local_programs_are_served_as_before() {
        let approved = HashSet::new();
        for t in [Tier::Read, Tier::Import, Tier::Mutate, Tier::FsPath, Tier::Discovery] {
            assert_eq!(d(t, &Kind::Local, &approved, false), Verdict::Allow);
        }
    }

    #[test]
    fn opaque_requests_need_an_active_session_and_an_allowed_read() {
        let approved = HashSet::new();
        let off = SessionView::default();
        let on = SessionView { active: true, origins: None };
        for path in SESSION_READS {
            assert!(matches!(decide(Tier::Read, &Kind::Opaque, &approved, false, path, &off), Verdict::Forbidden(_)), "no session: {path}");
            assert_eq!(decide(Tier::Read, &Kind::Opaque, &approved, false, path, &on), Verdict::Session, "{path}");
        }
        // Even with a session: other reads, writes, imports and paths stay refused.
        for (t, path) in [
            (Tier::Read, "/api/item/list"),
            (Tier::Read, "/api/item/info"),
            (Tier::Read, "/api/item/thumbnail"),
            (Tier::Read, "/api/v2/library/info"),
            (Tier::Mutate, "/api/folder/create"),
            (Tier::Mutate, "/api/preferences/collect/on"),
            (Tier::Import, "/api/item/addFromURL"),
            (Tier::FsPath, "/api/library/switch"),
        ] {
            assert!(matches!(decide(t, &Kind::Opaque, &approved, false, path, &on), Verdict::Forbidden(_)), "{path}");
        }
        // A website (with an Origin) never gets the session, even for an allowed route.
        assert!(matches!(decide(Tier::Read, &Kind::Website, &approved, false, "/api/folder/list", &on), Verdict::Forbidden(_)));
    }

    #[test]
    fn session_origins_read_and_import_with_confirmation_only() {
        let approved = HashSet::new();
        let mut so = HashSet::new();
        so.insert("safari-web-extension://abc".to_string());
        let on = SessionView { active: true, origins: Some(&so) };
        let ext = Kind::Extension("safari-web-extension://abc".into());
        assert_eq!(decide(Tier::Read, &ext, &approved, false, "/api/folder/list", &on), Verdict::Session);
        assert_eq!(decide(Tier::Import, &ext, &approved, false, "/api/item/addFromURL", &on), Verdict::Confirm);
        for (t, p) in [(Tier::Mutate, "/api/folder/create"), (Tier::FsPath, "/api/item/addFromPath"), (Tier::Read, "/api/item/list")] {
            assert!(matches!(decide(t, &ext, &approved, false, p, &on), Verdict::Forbidden(_)), "{p}");
        }
        // An extension not seen before is not refused during a session: its import waits for confirmation.
        let other = Kind::Extension("safari-web-extension://other".into());
        assert_eq!(decide(Tier::Import, &other, &approved, false, "/api/item/addFromURL", &on), Verdict::Confirm);
        assert!(matches!(decide(Tier::Read, &other, &approved, false, "/api/folder/list", &on), Verdict::Forbidden(_)), "but it reads nothing until it was confirmed once");
        assert!(matches!(decide(Tier::Mutate, &other, &approved, false, "/api/folder/create", &on), Verdict::Forbidden(_)));
        let off = SessionView { active: false, origins: Some(&so) };
        assert!(matches!(decide(Tier::Import, &ext, &approved, false, "/api/item/addFromURL", &off), Verdict::Forbidden(_)), "an expired session grants nothing");
        assert!(matches!(decide(Tier::Import, &other, &approved, false, "/api/item/addFromURL", &off), Verdict::Forbidden(_)));
    }

    #[test]
    fn cors_is_never_a_wildcard_for_library_data() {
        let ext = Kind::Extension("chrome-extension://abc".into());
        assert_eq!(cors_origin(Tier::Discovery, &Kind::Website, &Verdict::DiscoveryOnly, Some("https://x.com")), Some("*".into()));
        assert_eq!(cors_origin(Tier::Read, &ext, &Verdict::Allow, Some("chrome-extension://abc")), Some("chrome-extension://abc".into()));
        assert_eq!(cors_origin(Tier::Read, &Kind::Website, &Verdict::Forbidden("x"), Some("https://x.com")), None);
        assert_eq!(cors_origin(Tier::Read, &Kind::Local, &Verdict::Allow, None), None);
        assert_eq!(cors_origin(Tier::Read, &Kind::Opaque, &Verdict::Session, None), None, "an opaque caller is given no CORS grant at all");
    }
}
