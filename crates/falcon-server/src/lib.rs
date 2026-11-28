//! Eagle's local HTTP API, served from a falcon
//! library.
//!
//! Responses use Eagle's envelope: `{"status":"success","data":...}`, string
//! errors as HTTP 500 `{"status":"error","data":"..."}`, unknown paths as 404
//! and wrong verbs as 405 (both with Eagle's "method not allowed" text).
//!
//! A request is served when its `Host` is the server's own address or when
//! it carries the Developer token ([`Handle::set_token`]) as `token`.
//!
//! Falcon's own choices:
//! - it listens on 127.0.0.1 only;
//! - it serves library routes and AI Search only (no script injection or
//!   plugins);
//! - imports and thumbnail work finish before the reply, so returned ids
//!   exist and new thumbnails are in place;
//! - routes that read or write files do so without holding the library lock;
//! - library history is what the app hands over ([`Handle::set_history`]),
//!   or else the libraries this server has served, newest first.

mod ai;
pub mod mcp;
pub mod policy;
mod rules;
pub mod safe_fetch;
mod v2;

pub use ai::{AiQuery, AiSearch, AiStatus};
pub use policy::normalize_origin;

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use falcon_library::import::ImportOptions;
use falcon_library::model::{Folder, Item};
use falcon_library::search::{self, Query};
use falcon_library::Library;
use serde_json::{Map, Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

/// Eagle's default port; falcon defaults to the next one so both can run.
pub const EAGLE_PORT: u16 = 41595;
pub const DEFAULT_PORT: u16 = 41596;
/// The port the Eagle browser extension's older API (and its save requests) uses.
pub const LEGACY_PORT: u16 = 41593;

/// The version reported to clients, which check it against Eagle's.
const API_VERSION: &str = "4.0.0";

pub type Shared = Arc<Mutex<Library>>;

/// Server state beside the library: what `library/history` reports, the
/// Developer token, the preferences `application/info` reports, and the
/// folders unlocked over the API this session.
struct State {
    history: Mutex<Vec<String>>,
    token: Mutex<String>,
    preferences: Mutex<Value>,
    /// Set when a request changed `preferences` (`preferences/collect/*`).
    preferences_changed: AtomicBool,
    /// `(library root, folder id)` pairs opened with `folder/unlock`.
    unlocked: Mutex<HashSet<(String, String)>>,
    /// What answers `aiSearch/*`; see [`Handle::set_ai_search`].
    ai: Mutex<Option<Arc<dyn AiSearch>>>,
    /// Browser extension origins the user approved ([`Handle::set_approved_origins`]).
    approved_origins: Mutex<HashSet<String>>,
    /// The last extension origin refused for not being approved, so the app can offer it for approval.
    blocked_origin: Mutex<Option<String>>,
    /// The most recent requests, metadata only (see [`TraceRow`]).
    trace: Mutex<std::collections::VecDeque<TraceRow>>,
    /// The user-started browser import session, if any.
    session: Mutex<Option<Session>>,
    /// Imports waiting for the user's decision in the app.
    pending: Mutex<Vec<PendingEntry>>,
    next_pending: std::sync::atomic::AtomicU64,
    /// Wakes the app when something needs its attention.
    notify: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Times of recent requests from browsers, for the rate limit.
    browser_hits: Mutex<std::collections::VecDeque<std::time::Instant>>,
}

/// Requests a browser client may make in [`RATE_WINDOW`] before it is told to slow down.
const RATE_LIMIT: usize = 240;
const RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);
/// Imports waiting for a decision at once, and entries in one browser import.
const MAX_PENDING: usize = 3;
const MAX_BROWSER_BATCH: usize = 50;

/// A browser import session: time-limited, bound to one library, and the
/// only thing that lets an opaque browser request read folder and tag names.
struct Session {
    expires: std::time::Instant,
    root: String,
    origins: HashSet<String>,
}

/// An import from a browser waiting for the user to confirm it in Falcon.
#[derive(Clone, Debug)]
pub struct PendingImport {
    pub id: u64,
    /// Where it came from, as the browser labelled it (an origin, or "browser").
    pub site: String,
    pub count: usize,
    /// Up to five source hosts.
    pub hosts: Vec<String>,
    /// The destination folder's name, or empty for the library root.
    pub folder: String,
}

struct PendingEntry {
    info: PendingImport,
    decision: Arc<Mutex<Option<bool>>>,
}

/// How long an import waits for a decision before it is refused.
const CONFIRM_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// What is kept of a request for diagnosing browser-integration problems:
/// who sent it (as the browser labelled it), what it asked, and what was
/// answered. Never the query string, body, cookies, credentials or paths.
#[derive(Clone, Debug)]
pub struct TraceRow {
    pub secs: u64,
    pub port: u16,
    pub method: String,
    pub path: String,
    /// The `Origin` header reduced to scheme://host[:port], or empty.
    pub origin: String,
    /// `Sec-Fetch-Site` / `-Mode` / `-Dest`.
    pub fetch: String,
    /// The `Referer` reduced to its origin, or empty.
    pub referer: String,
    /// Whether the request carried `Cookie`, `Authorization`, or a CORS preflight header.
    pub flags: String,
    pub caller: String,
    pub tier: String,
    pub status: u16,
    pub why: String,
}

const TRACE_KEEP: usize = 60;

/// `scheme://host[:port]` of a URL-ish header value; empty when it is not one.
fn origin_only(v: &str) -> String {
    let v: String = v.chars().take(200).collect();
    match v.split_once("://") {
        Some((scheme, rest)) if scheme.len() <= 32 && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '+' || c == '.') => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            format!("{scheme}://{host}")
        }
        _ => if v == "null" { "null".into() } else { String::new() },
    }
}

impl State {
    fn new(lib: &Shared) -> State {
        State {
            history: Mutex::new(vec![root_of(lib)]),
            token: Mutex::new(String::new()),
            preferences: Mutex::new(default_preferences()),
            preferences_changed: AtomicBool::new(false),
            unlocked: Mutex::new(HashSet::new()),
            ai: Mutex::new(None),
            approved_origins: Mutex::new(HashSet::new()),
            blocked_origin: Mutex::new(None),
            trace: Mutex::new(std::collections::VecDeque::new()),
            session: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            next_pending: std::sync::atomic::AtomicU64::new(1),
            notify: Mutex::new(None),
            browser_hits: Mutex::new(std::collections::VecDeque::new()),
        }
    }

    /// Counts a browser request; false when the client is over the limit.
    fn rate_ok(&self) -> bool {
        let now = std::time::Instant::now();
        let mut h = self.browser_hits.lock().unwrap_or_else(|p| p.into_inner());
        while h.front().is_some_and(|t| now.duration_since(*t) > RATE_WINDOW) {
            h.pop_front();
        }
        if h.len() >= RATE_LIMIT {
            return false;
        }
        h.push_back(now);
        true
    }

    /// Whether the session is active for the library at `root` (an expired
    /// one, or one for another library, is dropped), and its extension origins.
    fn session_snapshot(&self, root: &str) -> (bool, HashSet<String>) {
        let mut s = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if s.as_ref().is_some_and(|x| x.expires <= std::time::Instant::now() || x.root != root) {
            *s = None;
        }
        match s.as_ref() {
            Some(x) => (true, x.origins.clone()),
            None => (false, HashSet::new()),
        }
    }

    fn wake(&self) {
        if let Some(f) = self.notify.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            f();
        }
    }

    fn record(&self, row: TraceRow) {
        let mut t = self.trace.lock().unwrap_or_else(|p| p.into_inner());
        if t.len() >= TRACE_KEEP {
            t.pop_front();
        }
        t.push_back(row);
    }

    fn token_valid(&self, given: Option<&str>) -> bool {
        let token = self.token.lock().unwrap_or_else(|p| p.into_inner());
        !token.trim().is_empty() && given == Some(token.as_str())
    }

    fn history(&self) -> Vec<String> {
        self.history.lock().map(|h| h.clone()).unwrap_or_default()
    }

    fn remember(&self, path: String) {
        if let Ok(mut h) = self.history.lock() {
            h.retain(|p| *p != path);
            h.insert(0, path);
        }
    }

    /// Whether a request is let in: see [`authorized`].
    fn allows(&self, host_ok: bool, given: Option<&str>) -> bool {
        let token = self.token.lock().unwrap_or_else(|p| p.into_inner());
        authorized(host_ok, &token, given)
    }

    fn preferences(&self) -> Value {
        self.preferences.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The folder ids of the library at `root` unlocked this session.
    fn unlocked_in(&self, root: &str) -> HashSet<String> {
        let u = self.unlocked.lock().unwrap_or_else(|p| p.into_inner());
        u.iter().filter(|(r, _)| r == root).map(|(_, id)| id.clone()).collect()
    }
}

/// The preferences reported before the app hands over its own: the keys
/// the server itself reads, at Eagle's defaults.
fn default_preferences() -> Value {
    json!({"general": {"showCollectModal": "false"}, "developer": {"apiToken": ""}})
}

/// Serves the API until the process exits.
pub fn serve(lib: Shared, port: u16) -> std::io::Result<()> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)).is_ok() {
        return Err(std::io::Error::other(format!("port {port} is in use (is Eagle running?)")));
    }
    let server = Server::http(("127.0.0.1", port)).map_err(std::io::Error::other)?;
    let state = State::new(&lib);
    for request in server.incoming_requests() {
        handle(&lib, &state, port, false, request);
    }
    Ok(())
}

/// A server running on its own thread; dropping it stops the server.
pub struct Handle {
    server: Arc<Server>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// The library being served; [`Handle::set_library`] swaps it.
    slot: Arc<Mutex<Shared>>,
    state: Arc<State>,
    pub port: u16,
}

impl Handle {
    /// Serves `lib` from the next request on, keeping the port. The old
    /// library is left as it is (its owner still holds it), and the new
    /// one joins `library/history`.
    pub fn set_library(&self, lib: Shared) {
        self.state.remember(root_of(&lib));
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = lib;
    }

    /// The library being served now.
    pub fn library(&self) -> Shared {
        self.slot.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Replaces what `library/history` reports (Eagle reports its recent
    /// libraries list), newest first. Duplicates are dropped.
    pub fn set_history(&self, paths: Vec<String>) {
        let mut seen = HashSet::new();
        let paths = paths.into_iter().filter(|p| seen.insert(p.clone())).collect();
        *self.state.history.lock().unwrap_or_else(|p| p.into_inner()) = paths;
    }

    /// Sets the Developer token. A request carrying it as `token` (in the
    /// query string, or in a POST body) is served whatever its `Host`
    /// header says. An empty token turns this off.
    pub fn set_token(&self, token: &str) {
        *self.state.token.lock().unwrap_or_else(|p| p.into_inner()) = token.to_string();
    }

    /// Sets the preferences `application/info` reports, in Eagle's shape
    /// (`{"general": {...}, "developer": {...}, ...}`). Values are strings
    /// where Eagle uses strings (`"true"`, `"false"`).
    pub fn set_preferences(&self, preferences: Value) {
        *self.state.preferences.lock().unwrap_or_else(|p| p.into_inner()) = preferences;
        self.state.preferences_changed.store(false, Ordering::SeqCst);
    }

    /// Hands over what answers the `aiSearch/*` routes. Without one they
    /// answer that AI Search isn't installed.
    pub fn set_ai_search(&self, ai: Arc<dyn AiSearch>) {
        *self.state.ai.lock().unwrap_or_else(|p| p.into_inner()) = Some(ai);
    }

    /// Sets the browser extension origins that may use the API (normalized
    /// `chrome-extension://…`, `moz-extension://…`, `safari-web-extension://…`
    /// origins; anything else is ignored).
    pub fn set_approved_origins(&self, origins: &[String]) {
        let set = origins.iter().filter_map(|o| policy::normalize_origin(o)).collect();
        *self.state.approved_origins.lock().unwrap_or_else(|p| p.into_inner()) = set;
    }

    /// Starts a browser import session for the library being served, for
    /// `secs` seconds. It ends by itself, when stopped, when another library
    /// is served, and with the process.
    pub fn start_session(&self, secs: u64) {
        let root = root_of(&self.library());
        *self.state.session.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(Session { expires: std::time::Instant::now() + std::time::Duration::from_secs(secs), root, origins: HashSet::new() });
    }

    pub fn stop_session(&self) {
        *self.state.session.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// Seconds left in the session, or None when there is none.
    pub fn session_remaining(&self) -> Option<u64> {
        let root = root_of(&self.library());
        self.state.session_snapshot(&root).0.then(|| {
            let s = self.state.session.lock().unwrap_or_else(|p| p.into_inner());
            s.as_ref().map_or(0, |x| x.expires.saturating_duration_since(std::time::Instant::now()).as_secs())
        })
    }

    /// Lets one browser extension origin use the running session (and only
    /// it). Ignored when there is no session or the origin isn't an extension's.
    pub fn approve_session_origin(&self, origin: &str) -> bool {
        let Some(o) = policy::normalize_origin(origin) else { return false };
        match self.state.session.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            Some(s) => {
                s.origins.insert(o);
                true
            }
            None => false,
        }
    }

    /// Imports waiting for the user's decision.
    pub fn pending_imports(&self) -> Vec<PendingImport> {
        self.state.pending.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|e| e.info.clone()).collect()
    }

    /// Records the user's decision on a waiting import.
    pub fn resolve_import(&self, id: u64, accept: bool) {
        if let Some(e) = self.state.pending.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|e| e.info.id == id) {
            *e.decision.lock().unwrap_or_else(|p| p.into_inner()) = Some(accept);
        }
    }

    /// Hands over what wakes the app when an import is waiting.
    pub fn set_notify(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.state.notify.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(f));
    }

    /// The most recent requests (newest last), metadata only, for diagnosing
    /// what a browser client actually sends.
    pub fn recent_requests(&self) -> Vec<TraceRow> {
        self.state.trace.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned().collect()
    }

    /// The last browser extension origin that was refused because it is not
    /// approved, for the app to offer in Preferences.
    pub fn blocked_origin(&self) -> Option<String> {
        self.state.blocked_origin.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Forgets the blocked origin (after it was approved or dismissed).
    pub fn clear_blocked_origin(&self) {
        *self.state.blocked_origin.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// Starts a second listener, on `port`, for clients that only look at
    /// Eagle's port. It shares this server's library, rules and approvals
    /// (it is the same API, not another one) and is refused when anything
    /// already answers on that port, on IPv4 or IPv6, so a running Eagle (or
    /// any other program) is never displaced or shadowed. Dropping the
    /// returned listener closes it.
    pub fn start_compat(&self, port: u16, on_request: impl Fn() + Send + 'static) -> Result<CompatListener, CompatError> {
        self.listen_extra(port, false, on_request)
    }

    /// Like [`Handle::start_compat`], for the older Eagle extension API on
    /// its own port: `GET /` says whether the app is there and `POST /` saves
    /// one image (a form with `src`, `title`, `url`, `folderID`…), which is
    /// handled exactly like an import from a browser session (an approved
    /// extension origin, a confirmation in the app, the safe download).
    pub fn start_legacy(&self, port: u16, on_request: impl Fn() + Send + 'static) -> Result<CompatListener, CompatError> {
        self.listen_extra(port, true, on_request)
    }

    fn listen_extra(&self, port: u16, legacy: bool, on_request: impl Fn() + Send + 'static) -> Result<CompatListener, CompatError> {
        if port == self.port {
            return Err(CompatError::InUse(format!("port {port} is the API's own port")));
        }
        let timeout = std::time::Duration::from_millis(200);
        for addr in [std::net::SocketAddr::from(([127, 0, 0, 1], port)), std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port))] {
            if std::net::TcpStream::connect_timeout(&addr, timeout).is_ok() {
                return Err(CompatError::InUse(format!("another program is already listening on port {port}")));
            }
        }
        let server = Arc::new(Server::http(("127.0.0.1", port)).map_err(|e| CompatError::Failed(e.to_string()))?);
        let (s, slot, state) = (server.clone(), self.slot.clone(), self.state.clone());
        let thread = std::thread::spawn(move || {
            for request in s.incoming_requests() {
                let lib = slot.lock().unwrap_or_else(|p| p.into_inner()).clone();
                handle(&lib, &state, port, legacy, request);
                on_request();
            }
        });
        Ok(CompatListener { server, thread: Some(thread), port })
    }

    /// The preferences, if a request changed them since the last call or
    /// [`Handle::set_preferences`] (`preferences/collect/on` and `off` set
    /// `general.showCollectModal`).
    pub fn take_preferences(&self) -> Option<Value> {
        self.state.preferences_changed.swap(false, Ordering::SeqCst).then(|| self.state.preferences())
    }
}

/// A second listener for Eagle's port (see [`Handle::start_compat`]).
pub struct CompatListener {
    server: Arc<Server>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub port: u16,
}

impl Drop for CompatListener {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Why the compatibility listener did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatError {
    /// Something else already answers on the port; nothing was changed.
    InUse(String),
    Failed(String),
}

impl std::fmt::Display for CompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompatError::InUse(m) | CompatError::Failed(m) => f.write_str(m),
        }
    }
}

fn root_of(lib: &Shared) -> String {
    match lib.lock() {
        Ok(l) => l.root().display().to_string(),
        Err(p) => p.into_inner().root().display().to_string(),
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Starts serving on the first free port of `ports`, calling `on_request`
/// after each handled request (for example to repaint a UI).
pub fn start(
    lib: Shared,
    ports: impl IntoIterator<Item = u16>,
    on_request: impl Fn() + Send + 'static,
) -> std::io::Result<Handle> {
    let mut last = None;
    for port in ports {
        // macOS lets a 127.0.0.1 bind succeed beside a wildcard listener
        // (Eagle listens on *:41595), which would capture that server's
        // localhost traffic. Skip any port where something already answers.
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)).is_ok() {
            last = Some(format!("port {port} is in use"));
            continue;
        }
        match Server::http(("127.0.0.1", port)) {
            Ok(server) => {
                let server = Arc::new(server);
                let s = server.clone();
                let state = Arc::new(State::new(&lib));
                let slot = Arc::new(Mutex::new(lib));
                let (st, sl) = (state.clone(), slot.clone());
                let thread = std::thread::spawn(move || {
                    for request in s.incoming_requests() {
                        let lib = sl.lock().unwrap_or_else(|p| p.into_inner()).clone();
                        handle(&lib, &st, port, false, request);
                        on_request();
                    }
                });
                return Ok(Handle { server, thread: Some(thread), slot, state, port });
            }
            Err(e) => last = Some(e.to_string()),
        }
    }
    Err(std::io::Error::other(last.unwrap_or_else(|| "no ports given".to_string())))
}

/// A handler outcome before it is wrapped in Eagle's envelope.
enum Reply {
    Data(Value),
    Empty,
    /// A string rejection (v1): HTTP 500 with the message in `data`.
    Fail(String),
    /// A thrown `Error` (v2): HTTP 500 with the text in `message` and the
    /// error's enumerable fields (none) as `data: {}`.
    Error(String),
    BadRequest,
    NotFound,
    WrongMethod,
    Unauthorized,
    /// A 302 to this location (the `eagle://` link routes).
    Redirect(String),
    /// A link route's 404: `{"error": "..."}`.
    Missing(&'static str),
    /// An HTML page.
    Page(&'static str),
    /// A refusal with its own status (403 policy, 413 size).
    Status(u16, &'static str),
    /// A JSON body as is, without Eagle's envelope (the older API's discovery).
    Raw(Value),
}

/// The largest request body read (data URLs for imports are the big ones).
const MAX_BODY: u64 = 48 * 1024 * 1024;

fn header(req: &Request, name: &'static str) -> Option<String> {
    req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string())
}

fn handle(lib: &Shared, state: &State, port: u16, legacy: bool, mut req: Request) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.trim_end_matches('/').to_string();
    let path = if path.is_empty() { "/".to_string() } else { path };
    let mut args = parse_query(query);
    let method = req.method().clone();
    let (host, origin, site) = (header(&req, "Host"), header(&req, "Origin"), header(&req, "Sec-Fetch-Site"));
    let (mode, dest) = (header(&req, "Sec-Fetch-Mode"), header(&req, "Sec-Fetch-Dest"));
    let mut row = TraceRow {
        secs: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        port,
        method: method.to_string(),
        path: path.chars().take(80).collect(),
        origin: origin.as_deref().map(origin_only).unwrap_or_default(),
        fetch: format!("{}/{}/{}", site.as_deref().unwrap_or("-"), mode.as_deref().unwrap_or("-"), dest.as_deref().unwrap_or("-")),
        referer: header(&req, "Referer").as_deref().map(origin_only).unwrap_or_default(),
        flags: [("Cookie", "cookie"), ("Authorization", "auth"), ("Access-Control-Request-Headers", "acrh"), ("Access-Control-Request-Method", "acrm")]
            .iter()
            .filter(|(h, _)| header(&req, h).is_some())
            .map(|(_, f)| *f)
            .collect::<Vec<_>>()
            .join(","),
        caller: String::new(),
        tier: String::new(),
        status: 0,
        why: String::new(),
    };
    let host_ok = host.as_deref().is_some_and(|h| policy::host_allowed(h, port));
    let kind = policy::kind(origin.as_deref(), policy::Fetch { site: site.as_deref(), mode: mode.as_deref(), dest: dest.as_deref() }, port);
    let approved = state.approved_origins.lock().unwrap_or_else(|p| p.into_inner()).clone();
    row.caller = match &kind {
        policy::Kind::Local => "local".into(),
        policy::Kind::SameOrigin => "same-origin".into(),
        policy::Kind::Extension(_) => "extension".into(),
        policy::Kind::Opaque => "opaque".into(),
        policy::Kind::Website => "website".into(),
    };

    // CORS preflight: answered only for a caller that could then be served.
    if method == Method::Options {
        let tier = policy::tier(false, &path, &Map::new());
        let ok = host_ok
            && match &kind {
                policy::Kind::Extension(o) => approved.contains(o) || tier == policy::Tier::Discovery,
                policy::Kind::SameOrigin => true,
                _ => tier == policy::Tier::Discovery,
            };
        row.tier = format!("{tier:?}");
        row.status = preflight(req, ok, origin.as_deref(), tier == policy::Tier::Discovery);
        row.why = if ok { "preflight allowed" } else { "preflight refused" }.into();
        return state.record(row);
    }

    let form = legacy && header(&req, "Content-Type").is_some_and(|c| c.to_ascii_lowercase().starts_with("application/x-www-form-urlencoded"));
    if matches!(method, Method::Post | Method::Put) {
        let mut body = String::new();
        let n = req.as_reader().take(MAX_BODY + 1).read_to_string(&mut body);
        if n.is_err() || body.len() as u64 > MAX_BODY {
            row.status = send(req, Reply::Status(413, "Request too large."), None);
            row.why = "body too large".into();
            return state.record(row);
        }
        if form {
            args.extend(parse_query(&body));
        } else if !body.trim().is_empty() {
            match serde_json::from_str::<Value>(&body) {
                Ok(Value::Object(m)) => args.extend(m),
                _ => {
                    row.status = send(req, Reply::BadRequest, None);
                    row.why = "body is not a JSON object".into();
                    return state.record(row);
                }
            }
        }
    }

    // Eagle serves a request whose Host is its own address, or one that
    // carries the Developer token (a POST may also put it in the query).
    let token = match args.get("token") {
        Some(Value::String(t)) => Some(t.clone()),
        _ => parse_query(query).get("token").and_then(Value::as_str).map(String::from),
    };
    if !state.allows(host_ok, token.as_deref()) {
        row.status = send(req, Reply::Unauthorized, None);
        row.why = "host not allowed and no valid token".into();
        return state.record(row);
    }
    let token_ok = state.token_valid(token.as_deref());
    if !token_ok && !matches!(kind, policy::Kind::Local | policy::Kind::SameOrigin) && !state.rate_ok() {
        row.status = send(req, Reply::Status(429, "Too many requests."), None);
        row.why = "rate limited".into();
        return state.record(row);
    }

    // Then the origin policy (BEC-1B): a web page gets discovery only, a
    // browser extension only what the user approved, never filesystem paths.
    let post = matches!(method, Method::Post | Method::Put);
    let tier = if legacy && path == "/" {
        if post { policy::Tier::Import } else { policy::Tier::Discovery }
    } else {
        policy::tier(post, &path, &args)
    };
    let (session_on, session_origins) = state.session_snapshot(&root_of(lib));
    let session = policy::SessionView { active: session_on, origins: Some(&session_origins) };
    let mut verdict = policy::decide(tier, &kind, &approved, token_ok, &path, &session);
    if verdict == policy::Verdict::Allow
        && !token_ok
        && policy::is_switch(&path)
        && matches!(kind, policy::Kind::Extension(_))
        && !s(&args, "libraryPath").is_some_and(|p| state.history().iter().any(|h| h == p))
    {
        verdict = policy::Verdict::Forbidden("Only a library from the recent libraries list can be opened from a browser extension.");
    }
    row.tier = format!("{tier:?}");
    let cors = policy::cors_origin(tier, &kind, &verdict, origin.as_deref());
    if let policy::Verdict::Forbidden(why) = verdict {
        if let policy::Kind::Extension(o) = &kind
            && tier != policy::Tier::Discovery
        {
            *state.blocked_origin.lock().unwrap_or_else(|p| p.into_inner()) = Some(o.clone());
        }
        row.status = send(req, Reply::Status(403, why), cors);
        row.why = why.to_string();
        return state.record(row);
    }

    let mut why = "served";
    if verdict == policy::Verdict::Confirm {
        let site = match &kind {
            policy::Kind::Extension(o) => o.clone(),
            _ => "browser".to_string(),
        };
        let referer = if row.referer.is_empty() { String::new() } else { format!(" (page {})", row.referer) };
        if !confirm_import(lib, state, &format!("{site}{referer}"), &path, &args) {
            row.status = send(req, Reply::Status(403, "The import was not confirmed in Falcon."), cors);
            row.why = "import not confirmed".into();
            return state.record(row);
        }
        why = "import confirmed";
    }
    let reply = match path.as_str() {
        _ if verdict == policy::Verdict::Session => {
            why = "session read";
            session_reply(lib, state, &path)
        }
        "/" if legacy && method == Method::Get => Reply::Raw(json!({"status": "success", "showCollectModal": false})),
        "/" if legacy && method == Method::Post => legacy_save(lib, &args, &state.unlocked_in(&root_of(lib))),
        _ if legacy => Reply::NotFound,
        "/" | "/api/application/info" | "/api/v2/app/info" if method == Method::Get => discovery(state, &path, verdict == policy::Verdict::Allow),
        "/item" | "/folder" | "/smart-folder" => link_redirect(&lock(lib), &path, s(&args, "id")),
        "/api/v2/playground" => Reply::Page(PLAYGROUND),
        _ => route(lib, state, &method, &path, &args),
    };
    row.status = send(req, reply, cors);
    row.why = why.into();
    state.record(row);
}

/// The names of the folders a browser session may show: no password-protected
/// folder and nothing inside one, and only the fields a picker needs.
fn session_folder(f: &Folder) -> Option<Value> {
    if f.is_locked() {
        return None;
    }
    let kids: Vec<Value> = f.children.iter().filter_map(session_folder).collect();
    Some(json!({"id": f.id, "name": f.name, "description": "", "children": kids, "iconColor": f.icon_color, "icon": f.icon}))
}

fn blocked_folder_ids(folders: &[Folder], out: &mut HashSet<String>, under_locked: bool) {
    for f in folders {
        let locked = under_locked || f.is_locked();
        if locked {
            out.insert(f.id.clone());
        }
        blocked_folder_ids(&f.children, out, locked);
    }
}

/// What a browser import session gets for its allowed read routes: folder
/// and tag names with opaque ids, the library's name instead of its path.
fn session_reply(lib: &Shared, state: &State, path: &str) -> Reply {
    if path.starts_with("/api/tag/") {
        return route(lib, state, &Method::Get, path, &Map::new());
    }
    let l = lock(lib);
    let name = library_name(&l);
    match path {
        "/api/folder/list" => Reply::Data(Value::Array(l.metadata.folders.iter().filter_map(session_folder).collect())),
        "/api/folder/listRecent" => {
            let mut blocked = HashSet::new();
            blocked_folder_ids(&l.metadata.folders, &mut blocked, false);
            let flat: Vec<Value> = l
                .all_folders()
                .into_iter()
                .filter(|f| !blocked.contains(&f.id))
                .take(16)
                .map(|f| json!({"id": f.id, "name": f.name, "description": "", "children": [], "iconColor": f.icon_color, "icon": f.icon}))
                .collect();
            Reply::Data(Value::Array(flat))
        }
        "/api/library/info" => Reply::Data(json!({"library": {"path": name, "name": name}})),
        "/api/library/history" => Reply::Data(json!([name])),
        _ => Reply::Status(403, "Not available in a browser import session."),
    }
}

/// The older extension API's save: one image by URL (`src`), with the page
/// it came from (`url`), its title and an optional folder. Everything goes
/// through [`add_url`], so the safe download and the locked-folder rule apply.
fn legacy_save(lib: &Shared, args: &Map<String, Value>, unlocked: &HashSet<String>) -> Reply {
    let Some(src) = s(args, "src") else { return Reply::Fail("Missing required parameters.".into()) };
    let mut item = Map::new();
    if let Some(t) = s(args, "title") {
        item.insert("name".into(), json!(t));
    }
    if let Some(page) = s(args, "url").filter(|u| (u.starts_with("https://") || u.starts_with("http://")) && u.len() <= 1000) {
        item.insert("website".into(), json!(page));
    }
    if let Some(f) = s(args, "folderID") {
        item.insert("folderId".into(), json!(f));
    }
    match add_url(lib, src, &item, &Map::new(), unlocked) {
        Ok(_) => Reply::Empty,
        Err(e) => Reply::Fail(e),
    }
}

/// Holds an import from a browser until the user decides in the app.
/// Nothing is downloaded or committed before that. No decision within
/// [`CONFIRM_WAIT`] counts as no.
fn confirm_import(lib: &Shared, state: &State, site: &str, path: &str, args: &Map<String, Value>) -> bool {
    let items: Vec<&Map<String, Value>> = match args.get("items") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_object).collect(),
        _ => vec![args],
    };
    if items.len() > MAX_BROWSER_BATCH || state.pending.lock().unwrap_or_else(|p| p.into_inner()).len() >= MAX_PENDING {
        return false;
    }
    let mut hosts: Vec<String> = Vec::new();
    for e in &items {
        let host = match (s(e, "src").or_else(|| s(e, "url")), e.get("base64")) {
            (Some(u), _) if u.starts_with("data:") => "(embedded image)".to_string(),
            (Some(u), _) => u.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("?").chars().take(80).collect(),
            (None, Some(_)) => "(embedded image)".to_string(),
            _ => "?".to_string(),
        };
        if !hosts.contains(&host) && hosts.len() < 5 {
            hosts.push(host);
        }
    }
    let folder = {
        let l = lock(lib);
        let mut ids = folder_ids(&l, args);
        if ids.is_empty() {
            ids = items.iter().flat_map(|e| folder_ids(&l, e)).collect();
        }
        ids.first().and_then(|id| l.all_folders().into_iter().find(|f| f.id == *id).map(|f| f.name.clone())).unwrap_or_default()
    };
    let _ = path;
    let decision = Arc::new(Mutex::new(None));
    let id = state.next_pending.fetch_add(1, Ordering::SeqCst);
    state.pending.lock().unwrap_or_else(|p| p.into_inner()).push(PendingEntry {
        info: PendingImport { id, site: site.chars().take(120).collect(), count: items.len().max(1), hosts, folder },
        decision: decision.clone(),
    });
    state.wake();
    let start = std::time::Instant::now();
    let answer = loop {
        if let Some(a) = *decision.lock().unwrap_or_else(|p| p.into_inner()) {
            break a;
        }
        if start.elapsed() >= CONFIRM_WAIT {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    state.pending.lock().unwrap_or_else(|p| p.into_inner()).retain(|e| e.info.id != id);
    answer
}

fn preflight(req: Request, ok: bool, origin: Option<&str>, discovery: bool) -> u16 {
    if !ok {
        let _ = req.respond(Response::empty(403));
        return 403;
    }
    let allow = if discovery { "*".to_string() } else { origin.unwrap_or("").to_string() };
    let h = |k: &str, v: &str| Header::from_bytes(k, v).unwrap();
    let methods = if discovery { "GET, OPTIONS" } else { "GET, POST, OPTIONS" };
    let _ = req.respond(
        Response::empty(204)
            .with_header(h("Access-Control-Allow-Origin", &allow))
            .with_header(h("Access-Control-Allow-Methods", methods))
            .with_header(h("Access-Control-Allow-Headers", "Content-Type"))
            .with_header(h("Access-Control-Max-Age", "600"))
            .with_header(h("Vary", "Origin")),
    );
    204
}

/// The discovery answer. A caller that is not otherwise authorized (a web
/// page) gets only what a client needs to see that the app is there; the
/// rest never includes the Developer token or filesystem paths.
fn discovery(state: &State, path: &str, full: bool) -> Reply {
    let prefs = state.preferences();
    let show = prefs["general"]["showCollectModal"].as_str().unwrap_or("false").to_string();
    let preferences = if full {
        let mut p = prefs;
        if let Some(o) = p.as_object_mut() {
            o.remove("developer");
        }
        if let Some(o) = p.get_mut("autoImport").and_then(Value::as_object_mut) {
            o.remove("path");
        }
        p
    } else {
        json!({"general": {"showCollectModal": show}})
    };
    let identity = json!({"application": "Falcon", "falconVersion": env!("CARGO_PKG_VERSION")});
    let mut data = if path == "/api/v2/app/info" {
        json!({
            "version": API_VERSION,
            "prereleaseVersion": null,
            "buildVersion": concat!("falcon-", env!("CARGO_PKG_VERSION")),
            "platform": std::env::consts::OS,
        })
    } else {
        json!({
            "version": API_VERSION,
            "prereleaseVersion": null,
            "buildVersion": concat!("falcon-", env!("CARGO_PKG_VERSION")),
            "showCollectModal": show == "true",
            "platform": std::env::consts::OS,
            "preferences": preferences,
        })
    };
    if let (Some(d), Some(i)) = (data.as_object_mut(), identity.as_object()) {
        d.extend(i.clone());
    }
    Reply::Data(data)
}

/// `/item?id=`, `/folder?id=` and `/smart-folder?id=`: a redirect to the
/// matching `eagle://` link, or 404 when there is no such object.
fn link_redirect(lib: &Library, path: &str, id: Option<&str>) -> Reply {
    let id = id.unwrap_or("");
    let (found, missing) = match path {
        "/item" => (lib.item(id).is_some(), "File does not exist."),
        "/folder" => (lib.all_folders().iter().any(|f| f.id == id), "Folder does not exist."),
        _ => (find_smart(&lib.smart_folders(), id), "Smart Folder does not exist."),
    };
    if found && !id.is_empty() { Reply::Redirect(format!("eagle:/{path}/{id}")) } else { Reply::Missing(missing) }
}

fn find_smart(list: &[falcon_library::smart::SmartFolder], id: &str) -> bool {
    list.iter().any(|f| f.id == id || find_smart(&f.children, id))
}

/// The interactive API page served at `/api/v2/playground`.
const PLAYGROUND: &str = include_str!("playground.html");

fn parse_query(q: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for pair in q.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(decode(k), Value::String(decode(v)));
    }
    out
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn send(req: Request, reply: Reply, cors: Option<String>) -> u16 {
    let (code, body) = match reply {
        Reply::Redirect(to) => {
            let resp = match Header::from_bytes("Location", to.as_bytes()) {
                Ok(h) => Response::empty(302).with_header(h),
                Err(()) => Response::empty(404),
            };
            let _ = req.respond(resp);
            return 302;
        }
        Reply::Missing(m) => {
            let resp = Response::from_string(json!({"error": m}).to_string())
                .with_status_code(404)
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(resp);
            return 404;
        }
        Reply::Page(html) => {
            let resp = Response::from_string(html).with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap());
            let _ = req.respond(resp);
            return 200;
        }
        Reply::Data(d) => (200, json!({"status": "success", "data": d})),
        Reply::Empty => (200, json!({"status": "success"})),
        Reply::Fail(m) => (500, json!({"status": "error", "data": m})),
        Reply::Error(m) => (500, json!({"status": "error", "message": m, "data": {}})),
        Reply::BadRequest => (400, json!({"status": "error", "code": 400, "message": "Bad Request"})),
        Reply::NotFound => (404, json!({"status": "error", "code": 404, "message": "method not allowed"})),
        Reply::WrongMethod => (405, json!({"status": "error", "code": 405, "message": "method not allowed"})),
        Reply::Status(code, msg) => (code, json!({"status": "error", "code": code, "message": msg})),
        Reply::Raw(v) => (200, v),
        Reply::Unauthorized => (
            401,
            json!({"status": "error", "code": 401, "message": "Unauthorized: Access is denied due to invalid token."}),
        ),
    };
    let mut resp = Response::from_string(body.to_string())
        .with_status_code(code)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap());
    if let Some(origin) = cors
        && let Ok(h) = Header::from_bytes("Access-Control-Allow-Origin", origin.as_bytes())
    {
        resp = resp.with_header(h).with_header(Header::from_bytes("Vary", "Origin").unwrap());
    }
    let _ = req.respond(resp);
    code
}

// ---------------------------------------------------------------------------
// Argument helpers

fn s<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|v| !v.is_empty())
}

fn strings(args: &Map<String, Value>, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        Some(Value::String(v)) if !v.is_empty() => v.split(',').map(|x| x.trim().to_string()).collect(),
        _ => Vec::new(),
    }
}

fn number(args: &Map<String, Value>, key: &str) -> Option<u64> {
    match args.get(key)? {
        Value::Number(n) => n.as_u64(),
        Value::String(v) => v.parse().ok(),
        _ => None,
    }
}

/// `folderId` / `folderID` / `folderIds` / `folderIDs`, keeping only ids
/// that exist (Eagle drops unknown ones).
fn folder_ids(lib: &Library, args: &Map<String, Value>) -> Vec<String> {
    let known = lib.folder_ids();
    let mut ids = Vec::new();
    for key in ["folderId", "folderID"] {
        if let Some(v) = s(args, key) {
            ids.push(v.to_string());
        }
    }
    for key in ["folderIds", "folderIDs"] {
        ids.extend(strings(args, key));
    }
    ids.retain(|i| known.contains(i.as_str()));
    ids.dedup();
    ids
}

fn item_json(item: &Item) -> Value {
    serde_json::to_value(item).unwrap_or(Value::Null)
}

/// A folder in `/api/folder/list`'s shape, with Eagle's extra display keys.
fn folder_json(f: &Folder) -> Value {
    folder_json_with(f, &HashSet::new())
}

/// [`folder_json`], where the folders in `unlocked` (opened with
/// `folder/unlock`) report `isUnLock: true` and keep their children.
fn folder_json_with(f: &Folder, unlocked: &HashSet<String>) -> Value {
    let mut v = serde_json::to_value(f).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("children".into(), Value::Array(f.children.iter().map(|c| folder_json_with(c, unlocked)).collect()));
        for (k, d) in [("extendTags", json!([])), ("icon", json!("")), ("iconColor", json!("")), ("coverId", json!(""))] {
            m.entry(k).or_insert(d);
        }
        let open = f.password.is_empty() || unlocked.contains(&f.id);
        m.insert("isUnLock".into(), json!(open));
        if !open {
            m.insert("children".into(), json!([]));
        }
    }
    v
}

fn tag_objects(lib: &Library) -> Vec<Value> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for i in lib.items().iter().filter(|i| !i.is_deleted) {
        for t in &i.tags {
            *counts.entry(t).or_default() += 1;
        }
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| search::natural_cmp(a.0, b.0));
    v.into_iter().map(|(name, n)| json!({"name": name, "imageCount": n})).collect()
}

fn library_name(lib: &Library) -> String {
    lib.root().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn library_info(lib: &Library) -> Value {
    let mut v = serde_json::to_value(&lib.metadata).unwrap_or(json!({}));
    let name = library_name(lib);
    if let Value::Object(m) = &mut v {
        m.insert("library".into(), json!({"path": lib.root().display().to_string(), "name": name}));
    }
    v
}

fn paginate(items: Vec<Value>, args: &Map<String, Value>) -> Value {
    let offset = number(args, "offset").unwrap_or(0) as usize;
    let limit = (number(args, "limit").filter(|l| *l > 0).unwrap_or(50) as usize).min(1000);
    let total = items.len();
    let page: Vec<Value> = items.into_iter().skip(offset).take(limit).collect();
    json!({"data": page, "total": total, "offset": offset, "limit": limit})
}

// ---------------------------------------------------------------------------
// Routing

fn route(lib: &Shared, state: &State, method: &Method, path: &str, args: &Map<String, Value>) -> Reply {
    use Method::{Get, Post};
    let routes: &[(&str, &[Method])] = &[
        ("/", &[Get]),
        ("/api/application/info", &[Get]),
        ("/api/library/info", &[Get]),
        ("/api/library/history", &[Get]),
        ("/api/library/switch", &[Post]),
        ("/api/library/icon", &[Get]),
        ("/api/folder/list", &[Get]),
        ("/api/folder/listRecent", &[Get]),
        ("/api/folder/create", &[Post]),
        ("/api/folder/rename", &[Post]),
        ("/api/folder/update", &[Post]),
        ("/api/folder/unlock", &[Post]),
        ("/api/preferences/collect/on", &[Get]),
        ("/api/preferences/collect/off", &[Get]),
        ("/api/tag/all", &[Get]),
        ("/api/tag/list", &[Get]),
        ("/api/tag/listRecent", &[Get]),
        ("/api/tag/groups", &[Get]),
        ("/api/item/addFromPath", &[Post]),
        ("/api/item/addFromPaths", &[Post]),
        ("/api/item/addFromURL", &[Post]),
        ("/api/item/addFromURLs", &[Post]),
        ("/api/item/update", &[Post]),
        ("/api/item/info", &[Get]),
        ("/api/item/thumbnail", &[Get]),
        ("/api/item/moveToTrash", &[Post]),
        ("/api/item/list", &[Get]),
        ("/api/item/setCustomThumbnail", &[Post]),
        ("/api/item/refreshPalette", &[Post]),
        ("/api/item/refreshThumbnail", &[Post]),
        ("/api/v2/app/info", &[Get]),
        ("/api/v2/library/info", &[Get]),
        ("/api/v2/library/history", &[Get]),
        ("/api/v2/library/switch", &[Post]),
        ("/api/v2/library/icon", &[Get]),
        ("/api/v2/item/get", &[Get, Post]),
        ("/api/v2/item/query", &[Post]),
        ("/api/v2/item/countAll", &[Get]),
        ("/api/v2/item/add", &[Post]),
        ("/api/v2/item/update", &[Post]),
        ("/api/v2/item/setCustomThumbnail", &[Post]),
        ("/api/v2/item/refreshThumbnail", &[Post]),
        ("/api/v2/item/getComments", &[Get]),
        ("/api/v2/item/addComment", &[Post]),
        ("/api/v2/item/updateComment", &[Post]),
        ("/api/v2/item/removeComment", &[Post]),
        ("/api/v2/folder/get", &[Get, Post]),
        ("/api/v2/folder/create", &[Post]),
        ("/api/v2/folder/update", &[Post]),
        ("/api/v2/smartFolder/get", &[Get, Post]),
        ("/api/v2/smartFolder/create", &[Post]),
        ("/api/v2/smartFolder/update", &[Post]),
        ("/api/v2/smartFolder/remove", &[Post]),
        ("/api/v2/smartFolder/getItems", &[Get, Post]),
        ("/api/v2/smartFolder/getRules", &[Get]),
        ("/api/v2/tag/get", &[Get, Post]),
        ("/api/v2/tag/getRecentTags", &[Get]),
        ("/api/v2/tag/getStarredTags", &[Get]),
        ("/api/v2/tag/update", &[Post]),
        ("/api/v2/tag/merge", &[Post]),
        ("/api/v2/tagGroup/get", &[Get]),
        ("/api/v2/tagGroup/create", &[Post]),
        ("/api/v2/tagGroup/update", &[Post]),
        ("/api/v2/tagGroup/remove", &[Post]),
        ("/api/v2/tagGroup/addTags", &[Post]),
        ("/api/v2/tagGroup/removeTags", &[Post]),
        ("/api/v2/aiSearch/isInstalled", &[Get]),
        ("/api/v2/aiSearch/isReady", &[Get]),
        ("/api/v2/aiSearch/isStarting", &[Get]),
        ("/api/v2/aiSearch/isSyncing", &[Get]),
        ("/api/v2/aiSearch/getSyncStatus", &[Get]),
        ("/api/v2/aiSearch/checkServiceHealth", &[Get]),
        ("/api/v2/aiSearch/searchByText", &[Get, Post]),
        ("/api/v2/aiSearch/searchByBase64", &[Post]),
        ("/api/v2/aiSearch/searchByItemId", &[Get, Post]),
    ];
    let Some((_, methods)) = routes.iter().find(|(p, _)| *p == path) else {
        return Reply::NotFound;
    };
    if !methods.contains(method) {
        return Reply::WrongMethod;
    }
    if path.starts_with("/api/v2/aiSearch/") {
        return ai::route(lib, state, path, args);
    }
    if let Some(reply) = import_route(lib, state, path, args) {
        return reply;
    }
    if let Some(reply) = unlocked_route(lib, state, path, args) {
        return reply;
    }
    let mut lib = lock(lib);
    let lib = &mut *lib;

    match path {
        // `/`, `application/info` and `v2/app/info` are answered by `discovery`.
        "/" | "/api/application/info" | "/api/v2/app/info" => discovery(state, path, true),
        "/api/library/info" => Reply::Data(library_info(lib)),
        "/api/v2/library/info" => {
            // v2 puts `path` and `name` at the top level instead of under `library`.
            let mut v = serde_json::to_value(&lib.metadata).unwrap_or(json!({}));
            if let Value::Object(m) = &mut v {
                m.insert("path".into(), json!(lib.root().display().to_string()));
                m.insert("name".into(), json!(library_name(lib)));
            }
            Reply::Data(v)
        }
        "/api/library/history" => Reply::Data(json!(state.history())),
        "/api/v2/library/history" => Reply::Data(paginate(state.history().into_iter().map(Value::from).collect(), args)),
        "/api/folder/list" => {
            let open = state.unlocked_in(&lib.root().display().to_string());
            Reply::Data(Value::Array(lib.metadata.folders.iter().map(|f| folder_json_with(f, &open)).collect()))
        }
        "/api/folder/listRecent" => {
            let open = state.unlocked_in(&lib.root().display().to_string());
            Reply::Data(Value::Array(lib.all_folders().into_iter().take(16).map(|f| folder_json_with(f, &open)).collect()))
        }
        "/api/folder/unlock" => {
            let id = s(args, "folderId").unwrap_or("");
            let Some(folder) = lib.all_folders().into_iter().find(|f| f.id == id) else {
                return Reply::Fail("Folder does not exist.".into());
            };
            // Eagle compares the stored value with the encoded password, so
            // an empty password opens a folder that has none.
            let given = args.get("password").and_then(Value::as_str);
            let opens = match given {
                Some(p) if folder.is_locked() => lib.verify_folder_password(id, p),
                Some(p) => p.is_empty(),
                None => false,
            };
            if !opens {
                return Reply::Fail("Password is incorrect.".into());
            }
            let root = lib.root().display().to_string();
            state.unlocked.lock().unwrap_or_else(|p| p.into_inner()).insert((root, id.to_string()));
            Reply::Empty
        }
        "/api/preferences/collect/on" | "/api/preferences/collect/off" => {
            let on = path.ends_with("/on");
            let mut prefs = state.preferences.lock().unwrap_or_else(|p| p.into_inner());
            let Some(general) = prefs.get_mut("general").and_then(Value::as_object_mut) else {
                return Reply::Fail("preferences file not exist.".into());
            };
            general.insert("showCollectModal".into(), json!(if on { "true" } else { "false" }));
            state.preferences_changed.store(true, Ordering::SeqCst);
            Reply::Empty
        }
        "/api/folder/create" => {
            let Some(name) = s(args, "folderName") else { return Reply::Fail("Missing required parameters.".into()) };
            match lib.create_folder(s(args, "parent"), name) {
                Ok(id) => folder_reply(lib, &id),
                Err(e) => Reply::Fail(e.to_string()),
            }
        }
        "/api/folder/rename" => {
            let (Some(id), Some(name)) = (s(args, "folderId").or(s(args, "folderID")), s(args, "newName")) else {
                return Reply::Fail("Missing required parameters.".into());
            };
            match lib.rename_folder(id, name) {
                Ok(()) => folder_reply(lib, id),
                Err(_) => Reply::Fail("Folder does not exist.".into()),
            }
        }
        "/api/folder/update" => {
            let Some(id) = s(args, "folderId").or(s(args, "folderID")) else {
                return Reply::Fail("Missing required parameters.".into());
            };
            if let Some(name) = s(args, "newName")
                && lib.rename_folder(id, name).is_err()
            {
                return Reply::Fail("Folder does not exist.".into());
            }
            if let Some(d) = args.get("newDescription").and_then(Value::as_str)
                && lib.set_folder_description(id, d).is_err()
            {
                return Reply::Fail("Folder does not exist.".into());
            }
            folder_reply(lib, id)
        }

        "/api/tag/all" => {
            let tags = tag_objects(lib);
            Reply::Data(json!({
                "tags": tags,
                "recent": lib.tags.history_tags,
                "groups": lib.metadata.tags_groups,
                "starred": lib.tags.starred_tags,
            }))
        }
        "/api/tag/list" => Reply::Data(Value::Array(tag_objects(lib))),
        "/api/tag/listRecent" => {
            let all = tag_objects(lib);
            let recent: Vec<Value> = lib
                .tags
                .history_tags
                .iter()
                .filter_map(|t| all.iter().find(|o| o["name"] == *t).cloned())
                .collect();
            Reply::Data(Value::Array(recent))
        }
        "/api/tag/groups" => Reply::Data(json!(lib.metadata.tags_groups)),
        "/api/v2/tag/get" => v2::get_tags(lib, args),
        "/api/v2/tag/getRecentTags" => v2::listed_tags(lib, &lib.tags.history_tags, args),
        "/api/v2/tag/getStarredTags" => v2::listed_tags(lib, &lib.tags.starred_tags, args),
        "/api/v2/tag/update" => v2::update_tag(lib, args),
        "/api/v2/tag/merge" => v2::merge_tags(lib, args),
        "/api/v2/tagGroup/get" => v2::get_tag_groups(lib, args),
        "/api/v2/tagGroup/create" => v2::create_tag_group(lib, args),
        "/api/v2/tagGroup/update" => v2::update_tag_group(lib, args),
        "/api/v2/tagGroup/remove" => v2::remove_tag_group(lib, args),
        "/api/v2/tagGroup/addTags" => v2::add_group_tags(lib, args),
        "/api/v2/tagGroup/removeTags" => v2::remove_group_tags(lib, args),
        "/api/v2/smartFolder/get" => v2::get_smart_folders(lib, args),
        "/api/v2/smartFolder/create" => v2::create_smart_folder(lib, args),
        "/api/v2/smartFolder/update" => v2::update_smart_folder(lib, args),
        "/api/v2/smartFolder/remove" => v2::remove_smart_folder(lib, args),
        "/api/v2/smartFolder/getItems" => v2::smart_folder_items(lib, args),
        "/api/v2/smartFolder/getRules" => Reply::Data(rules::schema()),
        "/api/v2/item/update" => v2::update_item(lib, args),
        "/api/v2/item/getComments" => v2::get_comments(lib, args),
        "/api/v2/item/addComment" => v2::add_comment(lib, args),
        "/api/v2/item/updateComment" => v2::update_comment(lib, args),
        "/api/v2/item/removeComment" => v2::remove_comment(lib, args),
        "/api/v2/folder/create" => v2::create_folder(lib, args),
        "/api/v2/folder/update" => v2::update_folder(lib, args),

        "/api/item/update" => {
            let Some(mut item) = s(args, "id").and_then(|id| lib.item(id)).cloned() else {
                return Reply::Fail("File does not exist.".into());
            };
            if let Some(Value::Array(tags)) = args.get("tags") {
                item.tags = tags.iter().filter_map(|t| t.as_str().map(String::from)).collect();
            }
            if let Some(u) = args.get("url").and_then(Value::as_str) {
                item.url = u.to_string();
            }
            if let Some(a) = args.get("annotation").and_then(Value::as_str) {
                item.annotation = a.to_string();
            }
            // Eagle ignores star 0 here.
            if let Some(star) = number(args, "star").filter(|s| (1..=5).contains(s)) {
                item.extra.insert("star".into(), json!(star));
            }
            let id = item.id.clone();
            match lib.update_item(item) {
                Ok(()) => Reply::Data(lib.item(&id).map(item_json).unwrap_or(Value::Null)),
                Err(e) => Reply::Fail(e.to_string()),
            }
        }
        "/api/item/info" => match s(args, "id").and_then(|id| lib.item(id)) {
            Some(i) => Reply::Data(item_json(i)),
            None => Reply::Fail("File does not exist.".into()),
        },
        "/api/item/thumbnail" => match s(args, "id").and_then(|id| lib.item(id)) {
            Some(i) => Reply::Data(json!(thumbnail_path(lib, i))),
            None => Reply::Fail("File does not exist.".into()),
        },
        "/api/item/moveToTrash" => {
            let ids = strings(args, "itemIds");
            if ids.is_empty() {
                return Reply::Fail("Missing required parameters.".into());
            }
            match lib.set_trashed(&ids, true) {
                Ok(()) => Reply::Empty,
                Err(e) => Reply::Fail(e.to_string()),
            }
        }
        "/api/item/list" => Reply::Data(Value::Array(v1_list(lib, args))),

        "/api/v2/item/get" => match v2::smart_filter(lib, args) {
            Ok(only) => {
                let items = v2_get(lib, args, only.as_ref());
                Reply::Data(paginate(project(items, args), args))
            }
            Err(e) => Reply::Error(e),
        },
        "/api/v2/item/query" => {
            let q = Query::parse(s(args, "query").unwrap_or(""));
            let index = search::folder_index(&lib.metadata.folders);
            let qlen = s(args, "query").map_or(0, |q| q.chars().count());
            let mut items: Vec<&Item> =
                lib.items().iter().filter(|i| !i.is_deleted && q.matches(&search::search_text(i, &index, qlen))).collect();
            search::sort(&mut items, search::OrderBy::Import, true);
            let items: Vec<Value> = items.into_iter().map(item_json).collect();
            Reply::Data(paginate(project(items, args), args))
        }
        "/api/v2/item/countAll" => Reply::Data(json!(lib.items().len())),
        "/api/v2/folder/get" => {
            let ids = strings(args, "ids");
            let one = s(args, "id");
            // With no filter Eagle returns the top-level tree, else a flat list.
            let folders: Vec<Value> = if ids.is_empty() && one.is_none() {
                lib.metadata.folders.iter().map(folder_json).collect()
            } else {
                lib.all_folders()
                    .into_iter()
                    .filter(|f| ids.contains(&f.id) || one == Some(f.id.as_str()))
                    .map(folder_json)
                    .collect()
            };
            Reply::Data(paginate(folders, args))
        }
        _ => Reply::NotFound,
    }
}

/// Locks the library, taking it over from a holder that panicked.
fn lock(lib: &Shared) -> MutexGuard<'_, Library> {
    lib.lock().unwrap_or_else(|p| p.into_inner())
}

/// The add routes. They lock the library only briefly (see [`add_path`]),
/// so the app sharing it stays responsive while a source is read.
fn import_route(lib: &Shared, state: &State, path: &str, args: &Map<String, Value>) -> Option<Reply> {
    let unlocked = state.unlocked_in(&root_of(lib));
    Some(match path {
        "/api/item/addFromPath" => {
            let Some(p) = s(args, "path") else { return Some(Reply::Fail("Missing required parameters.".into())) };
            match add_path(lib, PathBuf::from(p), args, args, false, &unlocked) {
                Ok(id) => Reply::Data(json!(id)),
                Err(e) => Reply::Fail(e),
            }
        }
        "/api/item/addFromPaths" => {
            let entries: Vec<Map<String, Value>> = match (args.get("items"), args.get("paths")) {
                (Some(Value::Array(items)), _) => items.iter().filter_map(|v| v.as_object().cloned()).collect(),
                (_, Some(Value::Array(paths))) => paths
                    .iter()
                    .filter_map(|p| p.as_str())
                    .map(|p| {
                        let mut m = Map::new();
                        m.insert("path".into(), json!(p));
                        m
                    })
                    .collect(),
                _ => return Some(Reply::BadRequest),
            };
            let mut ids = Vec::new();
            for e in entries {
                if let Some(p) = s(&e, "path")
                    && let Ok(id) = add_path(lib, PathBuf::from(p), &e, args, false, &unlocked)
                {
                    ids.push(id);
                }
            }
            Reply::Data(json!(ids))
        }
        "/api/item/addFromURL" => {
            let Some(url) = s(args, "url") else { return Some(Reply::Fail("Missing required parameters.".into())) };
            let added = add_url(lib, url, args, args, &unlocked);
            record_tags(lib, args);
            match added {
                Ok(_) => Reply::Empty,
                Err(e) => Reply::Fail(e),
            }
        }
        "/api/item/addFromURLs" => {
            let Some(Value::Array(items)) = args.get("items") else { return Some(Reply::Fail("Missing required parameters.".into())) };
            for e in items.iter().filter_map(Value::as_object) {
                if let Some(url) = s(e, "url") {
                    let _ = add_url(lib, url, e, args, &unlocked);
                    record_tags(lib, e);
                }
            }
            Reply::Empty
        }
        "/api/v2/item/add" => v2::add_item(lib, args, &unlocked),
        _ => return None,
    })
}

/// Routes that read or write files. Like the add routes, they hold the
/// library lock only to look things up and to commit, never while a file
/// is read, decoded or written.
fn unlocked_route(lib: &Shared, state: &State, path: &str, args: &Map<String, Value>) -> Option<Reply> {
    use falcon_library::{palette, thumbnail};
    Some(match path {
        "/api/library/switch" | "/api/v2/library/switch" => {
            let v2 = path.starts_with("/api/v2/");
            match v2::open_library(s(args, "libraryPath").unwrap_or("")) {
                Ok(new) => {
                    state.remember(new.root().display().to_string());
                    *lock(lib) = new;
                    *state.session.lock().unwrap_or_else(|p| p.into_inner()) = None;
                    if v2 { Reply::Data(json!(true)) } else { Reply::Empty }
                }
                Err(e) if v2 => Reply::Error(e),
                Err(e) => Reply::Fail(e),
            }
        }
        "/api/library/icon" => v2::library_icon(s(args, "libraryPath")).map_or_else(Reply::Fail, Reply::Data),
        "/api/v2/library/icon" => v2::library_icon(s(args, "libraryPath")).map_or_else(Reply::Error, Reply::Data),

        // v1 resolves `true` once the thumbnail is in place; v2 answers
        // `true` or `false` and never throws.
        "/api/item/setCustomThumbnail" | "/api/v2/item/setCustomThumbnail" => {
            let v2 = path.starts_with("/api/v2/");
            let (id, file) = if v2 { (s(args, "itemId"), s(args, "filePath")) } else { (s(args, "id"), s(args, "thumbnailPath")) };
            let file = PathBuf::from(file.unwrap_or(""));
            let done = restage(lib, id.unwrap_or(""), |root, item| {
                thumbnail::custom_thumbnailed(root, item.clone(), &file).map(Some).map_err(|e| e.to_string())
            });
            match done {
                Restaged::Done => Reply::Data(json!(true)),
                _ if v2 => Reply::Data(json!(false)),
                Restaged::NoItem => Reply::Fail("File does not exist.".into()),
                Restaged::Failed => Reply::Fail("Failed to set custom thumbnail.".into()),
            }
        }
        // Eagle queues the work and replies at once, so only an unknown
        // item is an error; falcon finishes the work first.
        "/api/item/refreshPalette" => match restage(lib, s(args, "id").unwrap_or(""), |root, item| {
            if falcon_library::media::is_audio(&item.ext) {
                return Ok(None);
            }
            let img = palette::refresh_source(item, root).ok_or("nothing to analyse")?;
            Ok(Some(Item { palettes: palette::extract(&img), ..item.clone() }))
        }) {
            Restaged::NoItem => Reply::Fail("File does not exist.".into()),
            _ => Reply::Empty,
        },
        "/api/item/refreshThumbnail" | "/api/v2/item/refreshThumbnail" => {
            let v2 = path.starts_with("/api/v2/");
            let id = if v2 { s(args, "itemId") } else { s(args, "id") };
            let done = restage(lib, id.unwrap_or(""), |root, item| {
                // As in the app, a custom thumbnail is left alone.
                if thumbnail::has_custom_thumbnail(item) {
                    return Ok(None);
                }
                thumbnail::redrawn(root, item.clone()).map(Some).map_err(|e| e.to_string())
            });
            match done {
                Restaged::NoItem if v2 => Reply::Data(json!(false)),
                Restaged::NoItem => Reply::Fail("File does not exist.".into()),
                _ if v2 => Reply::Data(json!(true)),
                _ => Reply::Empty,
            }
        }
        _ => return None,
    })
}

enum Restaged {
    NoItem,
    Failed,
    Done,
}

/// Runs `work` on a copy of item `id` without the library lock, then
/// saves what it changed. `work` returns `None` when there is nothing to
/// save. Only the keys `work` changed are written over the item as it is
/// at commit time, so edits made meanwhile (by the app, say) survive.
fn restage(lib: &Shared, id: &str, work: impl FnOnce(&Path, &Item) -> Result<Option<Item>, String>) -> Restaged {
    let (root, before) = {
        let lib = lock(lib);
        match lib.item(id) {
            Some(i) => (lib.root().to_path_buf(), i.clone()),
            None => return Restaged::NoItem,
        }
    };
    let after = match work(&root, &before) {
        Ok(Some(after)) => after,
        Ok(None) => return Restaged::Done,
        Err(_) => return Restaged::Failed,
    };
    let mut lib = lock(lib);
    // A library switched meanwhile has other items, even under the same id.
    if lib.root() != root {
        return Restaged::NoItem;
    }
    let Some(current) = lib.item(id).cloned() else { return Restaged::NoItem };
    let Some(merged) = merge_changes(&before, &after, &current) else { return Restaged::Failed };
    match lib.update_item(merged) {
        Ok(()) => Restaged::Done,
        Err(_) => Restaged::Failed,
    }
}

/// `current` with the keys that differ between `before` and `after` taken
/// from `after` (and the keys `after` dropped removed). Key order is kept.
fn merge_changes(before: &Item, after: &Item, current: &Item) -> Option<Item> {
    let (Value::Object(b), Value::Object(a), Value::Object(mut c)) =
        (serde_json::to_value(before).ok()?, serde_json::to_value(after).ok()?, serde_json::to_value(current).ok()?)
    else {
        return None;
    };
    for (k, v) in &a {
        if b.get(k) != Some(v) {
            c.insert(k.clone(), v.clone());
        }
    }
    for k in b.keys().filter(|k| !a.contains_key(*k)) {
        c.shift_remove(k);
    }
    serde_json::from_value(Value::Object(c)).ok()
}

/// v1's URL routes put the tags they were given at the front of the
/// recent tags, as Eagle's do (its path routes don't).
fn record_tags(lib: &Shared, entry: &Map<String, Value>) {
    let tags: Vec<String> = strings(entry, "tags").into_iter().filter(|t| !t.is_empty()).collect();
    if !tags.is_empty() {
        let _ = lock(lib).record_tag_use(&tags);
    }
}

fn folder_reply(lib: &Library, id: &str) -> Reply {
    match lib.all_folders().into_iter().find(|f| f.id == id) {
        Some(f) => Reply::Data(folder_json(f)),
        None => Reply::Fail("Folder does not exist.".into()),
    }
}

/// `/api/item/thumbnail`: the original when there is no thumbnail, else the
/// thumbnail, URL-encoded the way Eagle does.
fn thumbnail_path(lib: &Library, item: &Item) -> String {
    let path = item
        .thumbnail_path(lib.root())
        .or_else(|| lib.resolved_source_path(&item.id).ok())
        .or_else(|| lib.last_known_source_path(&item.id))
        .unwrap_or_else(|| item.managed_storage_path(lib.root()));
    path.display().to_string().replace(' ', "%20").replace('#', "%23")
}

/// Imports a file with the metadata Eagle's add routes accept.
///
/// The library is locked twice, briefly: to look up the folders, then to
/// commit. The source is read, thumbnailed and analysed in between, unlocked,
/// because the app locks the same library every frame and a dataless iCloud
/// source blocks until it has downloaded.
///
/// With `auto_tags`, the auto-tags of the target folders come first, then
/// the given tags, as Eagle's URL imports do (its path imports add none).
fn add_path(
    lib: &Shared,
    path: PathBuf,
    item: &Map<String, Value>,
    outer: &Map<String, Value>,
    from_download: bool,
    unlocked: &HashSet<String>,
) -> Result<String, String> {
    let auto_tags = from_download;
    let (folders, tags, root, mut mode) = {
        let lib = lock(lib);
        let mut folders = folder_ids(&lib, item);
        if folders.is_empty() {
            folders = folder_ids(&lib, outer);
        }
        // A password-protected folder takes imports only once it was unlocked over the API this session.
        if let Some(f) = lib.all_folders().into_iter().find(|f| folders.contains(&f.id) && f.is_locked() && !unlocked.contains(&f.id)) {
            return Err(format!("The folder \"{}\" is password-protected.", f.name));
        }
        let mut tags = if auto_tags { falcon_library::autotag::auto_tags_for(&lib.metadata.folders, &folders) } else { Vec::new() };
        falcon_library::autotag::append_new(&mut tags, &strings(item, "tags"));
        (folders, tags, lib.root().to_path_buf(), lib.storage_mode())
    };
    let opts = ImportOptions {
        folders,
        tags,
        palette: true,
        annotation: s(item, "annotation").unwrap_or("").to_string(),
        url: s(item, "website").unwrap_or("").to_string(),
        ..Default::default()
    };
    // A download lives in a temp directory that is removed afterwards, so it is always copied in, never linked.
    if from_download {
        mode = falcon_library::StorageMode::Copy;
    }
    let staged = falcon_library::import::stage_for(&path, &opts, Some(&root), mode).map_err(|e| e.to_string())?;
    let mut lib = lock(lib);
    let id = lib.commit_import(staged).map_err(|e| e.to_string())?;
    let mut changed = lib.item(&id).cloned().ok_or("import failed")?;
    let mut dirty = false;
    if let Some(name) = s(item, "name") {
        changed.name = name.chars().take(128).collect();
        dirty = true;
    }
    if let Some(star) = number(item, "star").filter(|s| (1..=5).contains(s)) {
        changed.extra.insert("star".into(), json!(star));
        dirty = true;
    }
    if let Some(t) = number(item, "modificationTime") {
        changed.modification_time = t;
        dirty = true;
    }
    if dirty {
        lib.update_item(changed).map_err(|e| e.to_string())?;
    }
    Ok(id)
}

/// Fetches an http(s) or data URL through the safe pipeline
/// ([`safe_fetch`]) into a private temp file and imports that with the
/// normal import path. The temp file is gone when this returns.
fn add_url(lib: &Shared, url: &str, item: &Map<String, Value>, outer: &Map<String, Value>, unlocked: &HashSet<String>) -> Result<String, String> {
    // Refuse a protected destination before downloading anything.
    {
        let l = lock(lib);
        let mut ids = folder_ids(&l, item);
        if ids.is_empty() {
            ids = folder_ids(&l, outer);
        }
        if let Some(f) = l.all_folders().into_iter().find(|f| ids.contains(&f.id) && f.is_locked() && !unlocked.contains(&f.id)) {
            return Err(format!("The folder \"{}\" is password-protected.", f.name));
        }
    }
    let policy = safe_fetch::FetchPolicy::default();
    let fetched = if url.starts_with("data:") { safe_fetch::from_data_url(url, &policy)? } else { safe_fetch::fetch_url(url, &policy)? };
    let derived = url.split(['?', '#']).next().and_then(|p| p.rsplit('/').next()).map(decode).and_then(|f| f.rsplit_once('.').map(|(n, _)| n.to_string())).unwrap_or_default();
    let name = safe_fetch::sanitize_name(s(item, "name").unwrap_or(&derived), "image");
    let mut entry = item.clone();
    entry.insert("name".into(), json!(name));
    entry.entry("website").or_insert_with(|| json!(if url.starts_with("data:") { "" } else { url }));
    add_path(lib, fetched.file.clone(), &entry, outer, true, unlocked)
}

/// `/api/item/list` with Eagle's quirks: `offset` is a page number, no limit
/// means 200, and the page is re-sorted newest first after being cut.
fn v1_list(lib: &Library, args: &Map<String, Value>) -> Vec<Value> {
    let names = strings(args, "keyword").into_iter().chain(strings(args, "name")).map(|k| k.to_lowercase()).collect::<Vec<_>>();
    let urls = strings(args, "url");
    let tags = strings(args, "tags");
    let folders = strings(args, "folders");
    let ext = s(args, "ext").map(str::to_lowercase);
    let mut items: Vec<&Item> = lib
        .items()
        .iter()
        .filter(|i| !i.is_deleted)
        .filter(|i| names.iter().all(|n| i.name.to_lowercase().contains(n)))
        .filter(|i| ext.as_ref().is_none_or(|e| i.ext == *e))
        .filter(|i| urls.iter().all(|u| i.url.contains(u.as_str())))
        .filter(|i| tags.is_empty() || i.tags.iter().any(|t| tags.contains(t)))
        .filter(|i| folders.is_empty() || i.folders.iter().any(|f| folders.contains(f)))
        .collect();

    sort_v1(&mut items, s(args, "orderBy").unwrap_or("CREATEDATE"));
    let limit = number(args, "limit").map_or(200, |l| l as usize);
    let page = number(args, "offset").unwrap_or(0) as usize;
    let mut page: Vec<&Item> = items.into_iter().skip(limit * page).take(limit).collect();
    page.sort_by_key(|i| std::cmp::Reverse(i.modification_time));
    page.into_iter().map(item_json).collect()
}

/// Sorts by an Eagle `orderBy` key (`CREATEDATE`, `FILESIZE`, `NAME`,
/// `RESOLUTION`); a `-` reverses it.
fn sort_v1(items: &mut [&Item], order: &str) {
    let reverse = order.contains('-');
    let key = order.replace('-', "").to_uppercase();
    match key.as_str() {
        "FILESIZE" => items.sort_by_key(|i| i.size),
        "NAME" => items.sort_by(|a, b| search::natural_cmp(&a.name, &b.name)),
        "RESOLUTION" => items.sort_by_key(|i| i.width * i.height),
        _ => items.sort_by_key(|i| i.modification_time),
    }
    if reverse {
        items.reverse();
    }
}

/// `/api/v2/item/get` filters, AND-ed; trash excluded; newest first.
/// `only`, from `smartFolders`, keeps the items of those smart folders.
fn v2_get(lib: &Library, args: &Map<String, Value>, only: Option<&std::collections::HashSet<String>>) -> Vec<Value> {
    let id = s(args, "id");
    let ids = strings(args, "ids");
    let keywords: Vec<String> = strings(args, "keywords").into_iter().map(|k| k.to_lowercase()).collect();
    let tags = strings(args, "tags");
    let folders = strings(args, "folders");
    let ext = s(args, "ext").map(str::to_lowercase);
    let annotation = s(args, "annotation").map(str::to_lowercase);
    let url = s(args, "url");
    let rating = number(args, "rating");
    let flag = |k: &str| matches!(args.get(k), Some(Value::Bool(true))) || s(args, k) == Some("true");
    let mut items: Vec<&Item> = lib
        .items()
        .iter()
        .filter(|i| !i.is_deleted)
        .filter(|i| id.is_none_or(|x| i.id == x))
        .filter(|i| only.is_none_or(|o| o.contains(&i.id)))
        .filter(|i| ids.is_empty() || ids.contains(&i.id))
        .filter(|i| !flag("isUnfiled") || i.folders.is_empty())
        .filter(|i| !flag("isUntagged") || i.tags.is_empty())
        .filter(|i| keywords.iter().all(|k| i.name.to_lowercase().contains(k)))
        .filter(|i| tags.iter().all(|t| i.tags.contains(t)))
        .filter(|i| folders.iter().all(|f| i.folders.contains(f)))
        .filter(|i| ext.as_ref().is_none_or(|e| i.ext == *e))
        .filter(|i| annotation.as_ref().is_none_or(|a| i.annotation.to_lowercase().contains(a)))
        .filter(|i| url.is_none_or(|u| i.url.contains(u)))
        .filter(|i| rating.is_none_or(|r| i.extra.get("star").and_then(Value::as_u64).unwrap_or(0) == r))
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.modification_time));
    items.into_iter().map(item_json).collect()
}

/// Keeps only `fields`, when given (the v2 projection).
fn project(items: Vec<Value>, args: &Map<String, Value>) -> Vec<Value> {
    let fields = strings(args, "fields");
    if fields.is_empty() {
        return items;
    }
    items
        .into_iter()
        .map(|v| match v {
            Value::Object(m) => Value::Object(m.into_iter().filter(|(k, _)| fields.contains(k)).collect()),
            other => other,
        })
        .collect()
}

/// Eagle's access rule: a request whose `Host` is the server's own address
/// is served; otherwise it must carry the Developer token, compared
/// exactly. An empty Developer token (Eagle's default; one that is only
/// whitespace counts as empty) means token auth is off: only the Host
/// check applies, and no token given, not even an empty one, lets a
/// request in.
fn authorized(host_ok: bool, token: &str, given: Option<&str>) -> bool {
    host_ok || (!token.trim().is_empty() && given == Some(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_strings_decode() {
        let m = parse_query("id=AB%20C&tags=a%2Cb&x=1+2");
        assert_eq!(m["id"], "AB C");
        assert_eq!(m["tags"], "a,b");
        assert_eq!(m["x"], "1 2");
        assert_eq!(decode("100%"), "100%");
    }

    #[test]
    fn empty_token_means_host_check_only() {
        assert!(authorized(true, "", None));
        assert!(authorized(true, "", Some("")));
        assert!(authorized(true, "abc", Some("wrong")));
        assert!(!authorized(false, "", None));
        assert!(!authorized(false, "", Some("")));
        assert!(!authorized(false, "", Some("abc")));
        assert!(!authorized(false, "  ", Some("  ")), "a blank token is no token");
    }

    #[test]
    fn set_token_is_exact() {
        assert!(authorized(false, "abc", Some("abc")));
        assert!(!authorized(false, "abc", Some("ABC")));
        assert!(!authorized(false, "abc", Some("abc ")));
        assert!(!authorized(false, "abc", Some("")));
        assert!(!authorized(false, "abc", None));
    }
}
