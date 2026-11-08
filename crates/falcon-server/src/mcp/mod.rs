//! A Model Context Protocol server on localhost, so AI assistants on the same
//! Mac can use the library falcon has open.
//!
//! It speaks Streamable HTTP on `POST /mcp`, and answers every request with a
//! single JSON object (no SSE stream, no sessions). [`rpc`] holds the JSON-RPC
//! layer and [`tools`] the tools.
//!
//! Falcon's own choices:
//! - it listens on 127.0.0.1 only and never binds Eagle's port or 8000;
//! - a request is served only when its `Host` is the server's own address and
//!   its `Origin`, if it has one, is on the loopback address. There are no CORS
//!   headers, so a web page can't read an answer even from another local port;
//! - [`Options::token`] can also ask for `Authorization: Bearer <token>`;
//! - it serves whatever library [`Handle::set_library`] was last given and has
//!   no way to open another one;
//! - write tools exist only while [`Options::allow_writes`] is on.

mod rpc;
mod tools;

use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use falcon_library::undo::Transaction;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::{EAGLE_PORT, Shared};

/// The first port tried. The HTTP API tries the five from
/// [`crate::DEFAULT_PORT`], so this stays clear of them.
pub const DEFAULT_PORT: u16 = 41610;
/// A port the MCP server never binds, besides Eagle's: other MCP servers on
/// this Mac commonly sit there.
pub const RESERVED_PORT: u16 = 8000;
/// The MCP endpoint.
pub const PATH: &str = "/mcp";

/// Requests carry JSON-RPC text, so this is far more than any of them needs.
const MAX_BODY: u64 = 4 * 1024 * 1024;
/// Requests served at once, so a slow import doesn't hold up reads.
const WORKERS: usize = 4;
/// Undo steps kept for a host that has not taken them yet.
const MAX_PENDING_UNDO: usize = 100;

/// What the app can change on a running server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Whether the write tools are listed and callable.
    pub allow_writes: bool,
    /// A token every request must send as `Authorization: Bearer`. An empty
    /// one matches nothing, so no request is served.
    pub token: Option<String>,
}

/// Which write tool made an [`UndoStep`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    AddItem,
    UpdateItem,
    CreateFolder,
    MoveToTrash,
}

/// A write a tool made, for the app's undo history.
#[derive(Debug)]
pub struct UndoStep {
    /// The library it changed.
    pub root: PathBuf,
    pub kind: WriteKind,
    pub transaction: Transaction,
}

/// What the workers share.
struct Context {
    /// The library being served; [`Handle::set_library`] swaps it.
    library: Mutex<Shared>,
    options: Mutex<Options>,
    undo: Mutex<Vec<UndoStep>>,
    port: u16,
}

impl Context {
    fn library(&self) -> Shared {
        self.library.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn options(&self) -> Options {
        self.options.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn push_undo(&self, step: UndoStep) {
        let mut undo = self.undo.lock().unwrap_or_else(|p| p.into_inner());
        undo.push(step);
        if undo.len() > MAX_PENDING_UNDO {
            undo.remove(0);
        }
    }
}

/// A server running on its own threads; dropping it stops the server.
pub struct Handle {
    server: Arc<Server>,
    threads: Vec<JoinHandle<()>>,
    context: Arc<Context>,
    pub port: u16,
}

impl Handle {
    /// Serves `lib` from the next request on, keeping the port.
    pub fn set_library(&self, lib: Shared) {
        *self.context.library.lock().unwrap_or_else(|p| p.into_inner()) = lib;
    }

    /// Applies new options from the next request on, keeping the port.
    pub fn set_options(&self, options: Options) {
        *self.context.options.lock().unwrap_or_else(|p| p.into_inner()) = options;
    }

    /// The writes made since the last call, oldest first.
    pub fn take_undo(&self) -> Vec<UndoStep> {
        std::mem::take(&mut *self.context.undo.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // Each call wakes one blocked worker.
        for _ in &self.threads {
            self.server.unblock();
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Starts serving on the first free port of `ports`, calling `on_request`
/// after each handled request (for example to repaint a UI). Port 0 asks the
/// system for a free port.
pub fn start(
    lib: Shared,
    ports: impl IntoIterator<Item = u16>,
    options: Options,
    on_request: impl Fn() + Send + Sync + 'static,
) -> std::io::Result<Handle> {
    let mut last = None;
    let on_request = Arc::new(on_request);
    for port in ports {
        if port == EAGLE_PORT || port == RESERVED_PORT {
            last = Some(format!("port {port} is not used"));
            continue;
        }
        // As for the HTTP API: a 127.0.0.1 bind can succeed beside a wildcard
        // listener on macOS, so skip any port where something already answers.
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if port != 0 && std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)).is_ok() {
            last = Some(format!("port {port} is in use"));
            continue;
        }
        let server = match Server::http(("127.0.0.1", port)) {
            Ok(server) => Arc::new(server),
            Err(e) => {
                last = Some(e.to_string());
                continue;
            }
        };
        let port = server.server_addr().to_ip().map_or(port, |a| a.port());
        let context = Arc::new(Context { library: Mutex::new(lib), options: Mutex::new(options), undo: Mutex::new(Vec::new()), port });
        let threads = (0..WORKERS)
            .map(|_| {
                let (server, context, on_request) = (server.clone(), context.clone(), on_request.clone());
                std::thread::spawn(move || {
                    for request in server.incoming_requests() {
                        handle(&context, request);
                        on_request();
                    }
                })
            })
            .collect();
        return Ok(Handle { server, threads, context, port });
    }
    Err(std::io::Error::other(last.unwrap_or_else(|| "no ports given".to_string())))
}

/// The JSON an MCP client is configured with to reach this server.
pub fn client_config(port: u16, token: Option<&str>) -> String {
    let mut server = json!({"type": "http", "url": format!("http://127.0.0.1:{port}{PATH}")});
    if let Some(t) = token {
        server["headers"] = json!({"Authorization": format!("Bearer {t}")});
    }
    let config = json!({"mcpServers": {"falcon": server}});
    serde_json::to_string_pretty(&config).unwrap_or_default()
}

/// What goes back over HTTP.
struct Reply {
    status: u16,
    body: Option<Value>,
    /// `(name, value)` of one more header.
    header: Option<(&'static str, &'static str)>,
}

impl Reply {
    /// A refusal, as a JSON-RPC error with no id.
    fn refused(status: u16, message: &str) -> Reply {
        Reply { status, body: Some(rpc::error_body(None, rpc::INVALID_REQUEST, message, None)), header: None }
    }
}

fn handle(context: &Context, mut request: Request) {
    let reply = respond(context, &mut request);
    let has_body = reply.body.is_some();
    let body = reply.body.map_or_else(String::new, |b| b.to_string());
    let mut response = Response::from_string(body).with_status_code(reply.status);
    // `from_string` starts as text/plain.
    let kind = if has_body { "application/json" } else { "text/plain" };
    if let Ok(h) = Header::from_bytes("Content-Type", kind) {
        response.add_header(h);
    }
    if let Some(h) = reply.header.and_then(|(k, v)| Header::from_bytes(k, v).ok()) {
        response.add_header(h);
    }
    let _ = request.respond(response);
}

fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str())
}

fn respond(context: &Context, request: &mut Request) -> Reply {
    let options = context.options();
    let origins_ok = request.headers().iter().filter(|h| h.field.equiv("Origin")).all(|h| origin_allowed(h.value.as_str()));
    if !origins_ok || !host_allowed(header(request, "Host"), context.port) {
        return Reply::refused(403, "This address or origin is not allowed.");
    }
    if !token_allowed(options.token.as_deref(), header(request, "Authorization")) {
        return Reply { header: Some(("WWW-Authenticate", "Bearer")), ..Reply::refused(401, "A valid bearer token is required.") };
    }
    let url = request.url();
    let path = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches('/');
    if path != PATH {
        return Reply::refused(404, "The MCP endpoint is /mcp.");
    }
    if *request.method() != Method::Post {
        return Reply { header: Some(("Allow", "POST")), ..Reply::refused(405, "Only POST is served.") };
    }
    if !is_json(header(request, "Content-Type")) {
        return Reply::refused(415, "The body must be application/json.");
    }
    if !accepts_json(header(request, "Accept")) {
        return Reply::refused(406, "Replies are application/json.");
    }
    let headers = rpc::Headers {
        protocol: header(request, "MCP-Protocol-Version").map(String::from),
        method: header(request, "Mcp-Method").map(String::from),
        name: header(request, "Mcp-Name").map(String::from),
    };
    let mut body = Vec::new();
    if request.as_reader().take(MAX_BODY + 1).read_to_end(&mut body).is_err() {
        return Reply::refused(400, "The body could not be read.");
    }
    if body.len() as u64 > MAX_BODY {
        return Reply::refused(413, "The body is too large.");
    }
    // A decoder's panic on a bad file must not take a worker with it.
    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rpc::dispatch(context, &options, &headers, &body)));
    let (status, body) = done.unwrap_or_else(|_| (500, Some(rpc::error_body(None, rpc::INTERNAL_ERROR, "The request failed.", None))));
    Reply { status, body, header: None }
}

/// Whether `Host` names this server. It stops DNS rebinding even from a
/// client that sends no `Origin`.
fn host_allowed(host: Option<&str>, port: u16) -> bool {
    host.is_some_and(|h| h == format!("127.0.0.1:{port}") || h.eq_ignore_ascii_case(&format!("localhost:{port}")))
}

/// Whether an `Origin` is on the loopback address, at any port. `null` (a
/// sandboxed page or a local file) is not.
fn origin_allowed(origin: &str) -> bool {
    let origin = origin.to_ascii_lowercase();
    let Some(rest) = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")) else { return false };
    ["localhost", "127.0.0.1", "[::1]"].iter().any(|host| {
        rest.strip_prefix(host).is_some_and(|tail| {
            tail.is_empty() || tail.strip_prefix(':').is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
    })
}

/// Whether a request has the token, when one is asked for. An empty token
/// matches nothing.
fn token_allowed(token: Option<&str>, authorization: Option<&str>) -> bool {
    let Some(token) = token else { return true };
    let given = authorization.and_then(|a| a.split_once(' ')).filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer")).map(|(_, t)| t.trim());
    !token.is_empty() && given.is_some_and(|g| same_bytes(g.as_bytes(), token.as_bytes()))
}

/// Compares without stopping at the first difference.
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

fn is_json(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|c| c.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"))
}

/// A client must offer JSON and SSE. Answers are always JSON, so only one
/// that rules JSON out is turned away.
fn accepts_json(accept: Option<&str>) -> bool {
    accept.is_none_or(|a| {
        a.split(',').any(|m| {
            let m = m.split(';').next().unwrap_or("").trim();
            m.eq_ignore_ascii_case("application/json") || m.eq_ignore_ascii_case("application/*") || m == "*/*"
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        for good in ["http://localhost", "http://localhost:3000", "http://127.0.0.1:6274", "https://LOCALHOST:1", "http://[::1]:80"] {
            assert!(origin_allowed(good), "{good}");
        }
        for bad in ["null", "", "http://evil.example", "http://localhost.evil.example", "http://127.0.0.1.evil.example:80", "http://localhost:", "http://localhost:80/x", "http://localhost@evil.example", "file://", "chrome-extension://abc", "http://192.168.1.2"] {
            assert!(!origin_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn hosts() {
        assert!(host_allowed(Some("127.0.0.1:41610"), 41610));
        assert!(host_allowed(Some("localhost:41610"), 41610));
        assert!(!host_allowed(Some("localhost:41611"), 41610));
        assert!(!host_allowed(Some("evil.example:41610"), 41610));
        assert!(!host_allowed(None, 41610));
    }

    #[test]
    fn tokens() {
        assert!(token_allowed(None, None));
        assert!(token_allowed(Some("abc"), Some("Bearer abc")));
        assert!(token_allowed(Some("abc"), Some("bearer abc")));
        assert!(!token_allowed(Some("abc"), None));
        assert!(!token_allowed(Some("abc"), Some("Bearer abd")));
        assert!(!token_allowed(Some("abc"), Some("Basic abc")));
        assert!(!token_allowed(Some(""), Some("Bearer ")), "an empty token matches nothing");
    }

    #[test]
    fn media_types() {
        assert!(is_json(Some("application/json; charset=utf-8")));
        assert!(!is_json(Some("text/plain")));
        assert!(!is_json(None));
        assert!(accepts_json(None));
        assert!(accepts_json(Some("application/json, text/event-stream")));
        assert!(accepts_json(Some("*/*")));
        assert!(!accepts_json(Some("text/event-stream")));
    }

    #[test]
    fn config_names_the_address_and_any_token() {
        let plain: Value = serde_json::from_str(&client_config(41610, None)).unwrap();
        assert_eq!(plain["mcpServers"]["falcon"]["url"], "http://127.0.0.1:41610/mcp");
        assert!(plain["mcpServers"]["falcon"].get("headers").is_none());
        let with: Value = serde_json::from_str(&client_config(41610, Some("abc"))).unwrap();
        assert_eq!(with["mcpServers"]["falcon"]["headers"]["Authorization"], "Bearer abc");
    }
}
