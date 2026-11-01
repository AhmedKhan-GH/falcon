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
//! - it serves library routes only (no script injection, plugins or AI);
//! - imports finish before the reply, so returned ids always exist;
//! - imports read their source without holding the library lock;
//! - library history is what the app hands over ([`Handle::set_history`]),
//!   or else the libraries this server has served, newest first.

mod rules;
mod v2;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use base64::Engine;
use falcon_library::import::ImportOptions;
use falcon_library::model::{Folder, Item};
use falcon_library::search::{self, Query};
use falcon_library::Library;
use serde_json::{Map, Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

/// Eagle's default port; falcon defaults to the next one so both can run.
pub const EAGLE_PORT: u16 = 41595;
pub const DEFAULT_PORT: u16 = 41596;

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
}

impl State {
    fn new(lib: &Shared) -> State {
        State {
            history: Mutex::new(vec![root_of(lib)]),
            token: Mutex::new(String::new()),
            preferences: Mutex::new(default_preferences()),
            preferences_changed: AtomicBool::new(false),
            unlocked: Mutex::new(HashSet::new()),
        }
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

    /// Whether `given` is the Developer token. No token set never matches.
    fn token_matches(&self, given: &str) -> bool {
        let token = self.token.lock().unwrap_or_else(|p| p.into_inner());
        !token.is_empty() && *token == given
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
        handle(&lib, &state, port, request);
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

    /// The preferences, if a request changed them since the last call or
    /// [`Handle::set_preferences`] (`preferences/collect/on` and `off` set
    /// `general.showCollectModal`).
    pub fn take_preferences(&self) -> Option<Value> {
        self.state.preferences_changed.swap(false, Ordering::SeqCst).then(|| self.state.preferences())
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
                        handle(&lib, &st, port, request);
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
}

fn handle(lib: &Shared, state: &State, port: u16, mut req: Request) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.trim_end_matches('/').to_string();
    let path = if path.is_empty() { "/".to_string() } else { path };
    let mut args = parse_query(query);
    let method = req.method().clone();
    if matches!(method, Method::Post | Method::Put) {
        let mut body = String::new();
        let _ = req.as_reader().read_to_string(&mut body);
        if !body.trim().is_empty() {
            match serde_json::from_str::<Value>(&body) {
                Ok(Value::Object(m)) => args.extend(m),
                _ => return send(req, Reply::BadRequest),
            }
        }
    }

    // Eagle serves a request whose Host is its own address, or one that
    // carries the Developer token (a POST may also put it in the query).
    let host = req.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str().to_string());
    let allowed_hosts = [format!("localhost:{port}"), format!("127.0.0.1:{port}"), format!("0.0.0.0:{port}")];
    let host_ok = host.as_ref().is_some_and(|h| allowed_hosts.contains(h));
    let token = match args.get("token") {
        Some(Value::String(t)) => Some(t.clone()),
        _ => parse_query(query).get("token").and_then(Value::as_str).map(String::from),
    };
    if !host_ok && !token.is_some_and(|t| state.token_matches(&t)) {
        return send(req, Reply::Unauthorized);
    }

    let reply = route(lib, state, &method, &path, &args);
    send(req, reply);
}

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

fn send(req: Request, reply: Reply) {
    let (code, body) = match reply {
        Reply::Data(d) => (200, json!({"status": "success", "data": d})),
        Reply::Empty => (200, json!({"status": "success"})),
        Reply::Fail(m) => (500, json!({"status": "error", "data": m})),
        Reply::Error(m) => (500, json!({"status": "error", "message": m, "data": {}})),
        Reply::BadRequest => (400, json!({"status": "error", "code": 400, "message": "Bad Request"})),
        Reply::NotFound => (404, json!({"status": "error", "code": 404, "message": "method not allowed"})),
        Reply::WrongMethod => (405, json!({"status": "error", "code": 405, "message": "method not allowed"})),
        Reply::Unauthorized => (
            401,
            json!({"status": "error", "code": 401, "message": "Unauthorized: Access is denied due to invalid token."}),
        ),
    };
    let resp = Response::from_string(body.to_string())
        .with_status_code(code)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
        .with_header(Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap());
    let _ = req.respond(resp);
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
    ];
    let Some((_, methods)) = routes.iter().find(|(p, _)| *p == path) else {
        return Reply::NotFound;
    };
    if !methods.contains(method) {
        return Reply::WrongMethod;
    }
    if let Some(reply) = import_route(lib, path, args) {
        return reply;
    }
    let mut lib = lock(lib);
    let lib = &mut *lib;

    match path {
        "/" | "/api/application/info" => {
            let preferences = state.preferences();
            Reply::Data(json!({
                "version": API_VERSION,
                "prereleaseVersion": null,
                "buildVersion": concat!("falcon-", env!("CARGO_PKG_VERSION")),
                "showCollectModal": preferences["general"]["showCollectModal"] == "true",
                "platform": std::env::consts::OS,
                "preferences": preferences,
            }))
        }
        "/api/v2/app/info" => Reply::Data(json!({
            "version": API_VERSION,
            "prereleaseVersion": null,
            "buildVersion": concat!("falcon-", env!("CARGO_PKG_VERSION")),
            "platform": std::env::consts::OS,
        })),
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
        "/api/library/switch" | "/api/v2/library/switch" => {
            let v2 = path.starts_with("/api/v2/");
            match v2::open_library(s(args, "libraryPath").unwrap_or("")) {
                Ok(new) => {
                    state.remember(new.root().display().to_string());
                    *lib = new;
                    if v2 { Reply::Data(json!(true)) } else { Reply::Empty }
                }
                Err(e) if v2 => Reply::Error(e),
                Err(e) => Reply::Fail(e),
            }
        }
        "/api/library/icon" => v2::library_icon(s(args, "libraryPath")).map_or_else(Reply::Fail, Reply::Data),
        "/api/v2/library/icon" => v2::library_icon(s(args, "libraryPath")).map_or_else(Reply::Error, Reply::Data),

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
fn import_route(lib: &Shared, path: &str, args: &Map<String, Value>) -> Option<Reply> {
    Some(match path {
        "/api/item/addFromPath" => {
            let Some(p) = s(args, "path") else { return Some(Reply::Fail("Missing required parameters.".into())) };
            match add_path(lib, PathBuf::from(p), args, args) {
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
                    && let Ok(id) = add_path(lib, PathBuf::from(p), &e, args)
                {
                    ids.push(id);
                }
            }
            Reply::Data(json!(ids))
        }
        "/api/item/addFromURL" => {
            let Some(url) = s(args, "url") else { return Some(Reply::Fail("Missing required parameters.".into())) };
            match add_url(lib, url, args, args) {
                Ok(_) => Reply::Empty,
                Err(e) => Reply::Fail(e),
            }
        }
        "/api/item/addFromURLs" => {
            let Some(Value::Array(items)) = args.get("items") else { return Some(Reply::Fail("Missing required parameters.".into())) };
            for e in items.iter().filter_map(Value::as_object) {
                if let Some(url) = s(e, "url") {
                    let _ = add_url(lib, url, e, args);
                }
            }
            Reply::Empty
        }
        "/api/v2/item/add" => v2::add_item(lib, args),
        _ => return None,
    })
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
    let path = match item.thumbnail_path(lib.root()) {
        Some(t) => t,
        None => item.original_path(lib.root()),
    };
    path.display().to_string().replace(' ', "%20").replace('#', "%23")
}

/// Imports a file with the metadata Eagle's add routes accept.
///
/// The library is locked twice, briefly: to look up the folders, then to
/// commit. The source is read, thumbnailed and analysed in between, unlocked,
/// because the app locks the same library every frame and a dataless iCloud
/// source blocks until it has downloaded.
fn add_path(lib: &Shared, path: PathBuf, item: &Map<String, Value>, outer: &Map<String, Value>) -> Result<String, String> {
    let folders = {
        let lib = lock(lib);
        let mut folders = folder_ids(&lib, item);
        if folders.is_empty() {
            folders = folder_ids(&lib, outer);
        }
        folders
    };
    let opts = ImportOptions {
        folders,
        tags: strings(item, "tags"),
        palette: true,
        annotation: s(item, "annotation").unwrap_or("").to_string(),
        url: s(item, "website").unwrap_or("").to_string(),
        ..Default::default()
    };
    let staged = falcon_library::import::stage(&path, &opts).map_err(|e| e.to_string())?;
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

/// Downloads an http(s) or data URL to a temporary file and imports it,
/// all without holding the library lock until the commit.
fn add_url(lib: &Shared, url: &str, item: &Map<String, Value>, outer: &Map<String, Value>) -> Result<String, String> {
    let (bytes, ext) = if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',').ok_or("Invalid data URL.")?;
        let ext = meta.split(';').next().and_then(|m| m.split('/').nth(1)).unwrap_or("png");
        let ext = if ext == "svg+xml" { "svg" } else if ext == "jpeg" { "jpg" } else { ext };
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string())?;
        (bytes, ext.to_string())
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let mut resp = ureq::get(url).call().map_err(|e| e.to_string())?;
        let bytes = resp.body_mut().read_to_vec().map_err(|e| e.to_string())?;
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let ext = path.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map_or("jpg", |(_, e)| e).to_lowercase();
        (bytes, ext)
    } else {
        return Err("Unsupported URL.".into());
    };
    let name = s(item, "name").map(String::from).unwrap_or_else(|| {
        url.split(['?', '#']).next().and_then(|p| p.rsplit('/').next()).and_then(|f| f.rsplit_once('.')).map_or("image".into(), |(n, _)| n.to_string())
    });
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let file = dir.path().join(format!("{name}.{ext}"));
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    let mut entry = item.clone();
    entry.entry("website").or_insert_with(|| json!(if url.starts_with("data:") { "" } else { url }));
    add_path(lib, file, &entry, outer)
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
}
