//! The interface every AI feature calls: generate text, stream text,
//! structured output, with images as parts of a message.
//!
//! Every chat call is streamed, whatever the caller asked for: the reply
//! arrives piece by piece, so an idle timeout can tell a slow model from a
//! dead one. The request runs on a thread of its own and the calling thread
//! only waits on it, so Cancel and the timeouts return at once even while
//! the connection is blocked; that thread drops the connection when its next
//! piece has no reader, and the whole-call timeout bounds it either way.

use std::io::{BufRead, BufReader, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::keys::{KeyStore, Secret};
use crate::provider::{BaseError, Kind, ModelRef, ProviderId, Settings, is_loopback};
use crate::wire::{self, Dialect, Reply};

/// How often a waiting call looks at the cancel flag.
const POLL: Duration = Duration::from_millis(50);
/// No reply this long is a broken or hostile server.
const MAX_REPLY_BYTES: u64 = 64 * 1024 * 1024;
/// Error texts from providers are cut to this many characters.
const MAX_ERROR_CHARS: usize = 300;

/// Timeouts of one call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// To open the connection.
    pub connect: Duration,
    /// From sending the request to the first piece of the reply. A local
    /// model may have to load first.
    pub first_byte: Duration,
    /// Between pieces of the reply.
    pub idle: Duration,
    /// The whole call.
    pub total: Duration,
}

impl Limits {
    /// Long enough for a large local model to load and answer.
    pub fn for_provider(p: ProviderId) -> Limits {
        match p.kind() {
            Kind::Cloud => Limits { connect: Duration::from_secs(15), first_byte: Duration::from_secs(120), idle: Duration::from_secs(60), total: Duration::from_secs(300) },
            Kind::Local | Kind::Custom => Limits { connect: Duration::from_secs(15), first_byte: Duration::from_secs(600), idle: Duration::from_secs(180), total: Duration::from_secs(1200) },
        }
    }

    /// For Verify and the model list.
    fn for_list() -> Limits {
        Limits { connect: Duration::from_secs(10), first_byte: Duration::from_secs(30), idle: Duration::from_secs(30), total: Duration::from_secs(60) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    User,
    Assistant,
}

/// A piece of a message.
#[derive(Clone, PartialEq, Debug)]
pub enum Part {
    Text(String),
    /// Image bytes in a format the provider takes, such as JPEG.
    Image { mime: String, data: Arc<Vec<u8>> },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn user(parts: Vec<Part>) -> Message {
        Message { role: Role::User, parts }
    }

    pub fn text(role: Role, text: impl Into<String>) -> Message {
        Message { role, parts: vec![Part::Text(text.into())] }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Request {
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// How a model's reply failed the checks.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BadOutput {
    Empty,
    /// The provider stopped at its length limit before the reply was whole.
    Cut,
    NotJson,
    /// Valid JSON that doesn't fit the schema: where and how.
    Schema(String),
}

/// Why a call failed. Holds neither a key nor anything sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AiError {
    /// The provider isn't in the settings.
    NotAdded(ProviderId),
    BaseUrl(ProviderId, BaseError),
    /// A key may only cross plain http to this Mac.
    PlainHttpKey(ProviderId),
    NoKey(ProviderId),
    /// The Keychain's own message.
    KeyStore(String),
    Cancelled,
    Timeout,
    /// No connection, or it broke.
    Network(String),
    /// A refusal: the status and the provider's message.
    Http { status: u16, message: String },
    /// An error the provider reported in the reply.
    Provider(String),
    /// The reply isn't what was asked for.
    Bad(BadOutput),
}

impl AiError {
    /// Whether the provider refused the key.
    pub fn is_auth(&self) -> bool {
        matches!(self, AiError::Http { status: 401 | 403, .. })
    }
}

/// The address and key of one provider. `Debug` shows no key.
#[derive(Clone, Debug)]
pub struct Connection {
    provider: ProviderId,
    base: String,
    key: Option<Secret>,
}

impl Connection {
    /// A connection with this key. A local provider gets none, a cloud
    /// provider can't go without, and a key doesn't cross plain http unless
    /// the server is on this Mac.
    pub fn new(provider: ProviderId, base: String, key: Option<Secret>) -> Result<Connection, AiError> {
        let key = if provider.takes_key() { key } else { None };
        if provider.needs_key() && key.is_none() {
            return Err(AiError::NoKey(provider));
        }
        if key.is_some() && base.starts_with("http://") && !is_loopback(&base) {
            return Err(AiError::PlainHttpKey(provider));
        }
        Ok(Connection { provider, base, key })
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        let Some(key) = &self.key else { return Vec::new() };
        match wire::dialect(self.provider) {
            Dialect::OpenAi => vec![("authorization", format!("Bearer {}", key.expose()))],
            Dialect::Anthropic => vec![("x-api-key", key.expose().to_string()), ("anthropic-version", "2023-06-01".to_string())],
            Dialect::Google => vec![("x-goog-api-key", key.expose().to_string())],
        }
    }

    /// Takes the key out of a text from the network, and cuts it short.
    fn scrub(&self, text: &str) -> String {
        let text = match &self.key {
            Some(k) if !k.expose().is_empty() => text.replace(k.expose(), "[key]"),
            _ => text.to_string(),
        };
        text.chars().take(MAX_ERROR_CHARS).collect()
    }

    /// The provider's model ids. This is what Verify does: it needs a key
    /// the provider accepts.
    pub fn list_models(&self, cancel: &AtomicBool) -> Result<Vec<String>, AiError> {
        let mut list = None;
        self.exchange(wire::models_path(self.provider), None, Limits::for_list(), cancel, &mut |v| {
            list = wire::parse_models(self.provider, v);
            Ok(())
        })?;
        list.ok_or(AiError::Bad(BadOutput::NotJson))
    }

    /// One streamed chat call.
    fn chat(&self, model: &str, req: &Request, schema: Option<&Value>, limits: Limits, cancel: &AtomicBool, on_text: &mut dyn FnMut(&str)) -> Result<Reply, AiError> {
        let (path, body) = wire::chat_request(self.provider, model, req, schema);
        let mut reply = Reply::default();
        self.exchange(&path, Some(body.to_string()), limits, cancel, &mut |v| {
            let new = wire::feed(self.provider, v, &mut reply)?;
            if !new.is_empty() {
                on_text(&new);
            }
            Ok(())
        })?;
        Ok(reply)
    }

    /// Sends a request (POST with a body, else GET) and hands every JSON
    /// value of the reply to `on_value`: each `data:` line of an event
    /// stream, or the whole body otherwise. Redirects are refused, so
    /// neither the key nor the items can be sent on to another host.
    fn exchange(&self, path: &str, body: Option<String>, limits: Limits, cancel: &AtomicBool, on_value: &mut dyn FnMut(&Value) -> Result<(), AiError>) -> Result<(), AiError> {
        let url = format!("{}{}", self.base, path);
        let headers = self.headers();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || transfer(&url, &headers, body, limits, &tx));

        let started = Instant::now();
        let mut last = started;
        let mut head = None;
        let mut got_piece = false;
        let mut whole = String::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            let allowed = if got_piece { limits.idle } else { limits.first_byte };
            if last.elapsed() > allowed || started.elapsed() > limits.total {
                return Err(AiError::Timeout);
            }
            let piece = match rx.recv_timeout(POLL) {
                Ok(p) => p,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(AiError::Network("the connection closed".to_string())),
            };
            last = Instant::now();
            match piece {
                Piece::Head { status, stream } => head = Some((status, stream)),
                Piece::Line(line) => {
                    got_piece = true;
                    match head {
                        Some((200..=299, true)) => {
                            let Some(data) = line.strip_prefix("data:") else { continue };
                            let data = data.trim();
                            if data.is_empty() || data == "[DONE]" {
                                continue;
                            }
                            let v: Value = serde_json::from_str(data).map_err(|_| AiError::Bad(BadOutput::NotJson))?;
                            on_value(&v)?;
                        }
                        _ => whole.push_str(&line),
                    }
                }
                Piece::Failed { timeout: true, .. } => return Err(AiError::Timeout),
                Piece::Failed { message, .. } => return Err(AiError::Network(self.scrub(&message))),
                Piece::End => break,
            }
        }
        match head {
            Some((200..=299, true)) => Ok(()),
            Some((200..=299, false)) => {
                let v: Value = serde_json::from_str(&whole).map_err(|_| AiError::Bad(BadOutput::NotJson))?;
                on_value(&v)
            }
            Some((status, _)) => Err(AiError::Http { status, message: self.scrub(&error_message(&whole)) }),
            None => Err(AiError::Network("the connection closed".to_string())),
        }
    }
}

/// What the transfer thread reports.
enum Piece {
    Head { status: u16, stream: bool },
    Line(String),
    Failed { timeout: bool, message: String },
    End,
}

/// The request itself, on its own thread. Ends when the reply does, when
/// the caller is gone, or at the whole-call timeout.
fn transfer(url: &str, headers: &[(&'static str, String)], body: Option<String>, limits: Limits, tx: &mpsc::Sender<Piece>) {
    let failed = |e: &ureq::Error| Piece::Failed { timeout: matches!(e, ureq::Error::Timeout(_)), message: e.to_string() };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(limits.connect))
        .timeout_global(Some(limits.total))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let sent = match body {
        Some(body) => {
            let mut r = agent.post(url).content_type("application/json");
            for (k, v) in headers {
                r = r.header(*k, v);
            }
            r.send(body)
        }
        None => {
            let mut r = agent.get(url);
            for (k, v) in headers {
                r = r.header(*k, v);
            }
            r.call()
        }
    };
    let response = match sent {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(failed(&e));
            return;
        }
    };
    let status = response.status().as_u16();
    let stream = response.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|c| c.to_ascii_lowercase().contains("text/event-stream"));
    if tx.send(Piece::Head { status, stream }).is_err() {
        return;
    }
    let reader = response.into_body().into_reader().take(MAX_REPLY_BYTES);
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => {
                let _ = tx.send(Piece::End);
                return;
            }
            Ok(_) => {
                if tx.send(Piece::Line(String::from_utf8_lossy(&line).into_owned())).is_err() {
                    return;
                }
            }
            Err(e) => {
                let timeout = e.kind() == std::io::ErrorKind::TimedOut;
                let _ = tx.send(Piece::Failed { timeout, message: e.to_string() });
                return;
            }
        }
    }
}

/// The message in an error reply, whatever shape the provider gave it.
fn error_message(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return body.trim().to_string() };
    let found = v.get("error").and_then(|e| e.get("message").or(Some(e))).and_then(Value::as_str).or(v.get("message").and_then(Value::as_str));
    found.map_or_else(|| body.trim().to_string(), str::to_string)
}

/// A reply with any thinking block and a surrounding code fence taken off.
fn reply_text(text: &str) -> &str {
    let mut t = text.trim();
    if t.starts_with("<think>")
        && let Some(end) = t.find("</think>")
    {
        t = t[end + "</think>".len()..].trim();
    }
    if let Some(rest) = t.strip_prefix("```")
        && let Some(inner) = rest.strip_suffix("```")
    {
        t = inner.split_once('\n').map_or(inner, |(first, body)| if first.trim().chars().all(char::is_alphanumeric) { body } else { inner }).trim();
    }
    t
}

fn not_empty(reply: Reply) -> Result<String, AiError> {
    let text = reply_text(&reply.text);
    if text.is_empty() {
        return Err(AiError::Bad(if reply.cut { BadOutput::Cut } else { BadOutput::Empty }));
    }
    Ok(text.to_string())
}

/// The shared interface: the settings and keys at the time it was made.
#[derive(Clone)]
pub struct Client {
    settings: Settings,
    keys: Arc<dyn KeyStore>,
    limits: Option<Limits>,
}

impl Client {
    pub fn new(settings: Settings, keys: Arc<dyn KeyStore>) -> Client {
        Client { settings, keys, limits: None }
    }

    /// Replaces every provider's default timeouts.
    pub fn with_limits(mut self, limits: Limits) -> Client {
        self.limits = Some(limits);
        self
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The connection to a provider the user added, with its stored key.
    pub fn connection(&self, provider: ProviderId) -> Result<Connection, AiError> {
        if self.settings.provider(provider).is_none() {
            return Err(AiError::NotAdded(provider));
        }
        let base = self.settings.base_url(provider).map_err(|e| AiError::BaseUrl(provider, e))?;
        let key = if provider.takes_key() { self.keys.get(provider).map_err(AiError::KeyStore)? } else { None };
        Connection::new(provider, base, key)
    }

    fn limits_for(&self, p: ProviderId) -> Limits {
        self.limits.unwrap_or_else(|| Limits::for_provider(p))
    }

    /// The whole reply as text.
    pub fn generate_text(&self, model: &ModelRef, req: &Request, cancel: &AtomicBool) -> Result<String, AiError> {
        self.stream_text(model, req, cancel, &mut |_| {})
    }

    /// Calls `on_text` with each piece as it arrives; returns the whole reply.
    pub fn stream_text(&self, model: &ModelRef, req: &Request, cancel: &AtomicBool, on_text: &mut dyn FnMut(&str)) -> Result<String, AiError> {
        let reply = self.connection(model.provider)?.chat(&model.model, req, None, self.limits_for(model.provider), cancel, on_text)?;
        not_empty(reply)
    }

    /// A reply that is JSON fitting `schema`, whatever the provider's own
    /// structured output did. Anything else is an error.
    pub fn generate_object(&self, model: &ModelRef, req: &Request, schema: &Value, cancel: &AtomicBool) -> Result<Value, AiError> {
        let reply = self.connection(model.provider)?.chat(&model.model, req, Some(schema), self.limits_for(model.provider), cancel, &mut |_| {})?;
        let cut = reply.cut;
        let text = not_empty(reply)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| AiError::Bad(if cut { BadOutput::Cut } else { BadOutput::NotJson }))?;
        crate::schema::validate(&value, schema).map_err(|e| AiError::Bad(BadOutput::Schema(e)))?;
        Ok(value)
    }

    /// The model ids of a provider the user added.
    pub fn list_models(&self, provider: ProviderId, cancel: &AtomicBool) -> Result<Vec<String>, AiError> {
        self.connection(provider)?.list_models(cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_lose_thinking_and_fences() {
        assert_eq!(reply_text(" {\"a\":1} "), "{\"a\":1}");
        assert_eq!(reply_text("<think>hm</think>\n{\"a\":1}"), "{\"a\":1}");
        assert_eq!(reply_text("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(reply_text("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(reply_text("not {fenced}"), "not {fenced}");
    }

    #[test]
    fn error_messages_of_any_shape() {
        assert_eq!(error_message(r#"{"error":{"message":"bad key"}}"#), "bad key");
        assert_eq!(error_message(r#"{"error":"bad"}"#), "bad");
        assert_eq!(error_message(r#"{"message":"m"}"#), "m");
        assert_eq!(error_message(" plain \n"), "plain");
    }

    #[test]
    fn keys_only_go_where_they_may() {
        let key = || Some(Secret::new("k"));
        assert_eq!(Connection::new(ProviderId::OpenAi, "https://x/v1".into(), None).unwrap_err(), AiError::NoKey(ProviderId::OpenAi));
        assert!(Connection::new(ProviderId::OpenAiCompatible, "http://192.168.1.5/v1".into(), None).is_ok(), "a custom server may need no key");
        assert_eq!(Connection::new(ProviderId::OpenAiCompatible, "http://192.168.1.5/v1".into(), key()).unwrap_err(), AiError::PlainHttpKey(ProviderId::OpenAiCompatible));
        assert!(Connection::new(ProviderId::OpenAiCompatible, "http://localhost:9/v1".into(), key()).is_ok());
        assert!(Connection::new(ProviderId::Ollama, "http://192.168.1.5/v1".into(), key()).unwrap().key.is_none(), "a local provider takes none");
    }

    #[test]
    fn a_connection_prints_no_key() {
        let c = Connection::new(ProviderId::OpenAi, "https://x/v1".into(), Some(Secret::new("sk-secret"))).unwrap();
        assert!(!format!("{c:?}").contains("sk-secret"));
        assert_eq!(c.scrub("bad sk-secret here"), "bad [key] here");
        assert_eq!(c.scrub(&"x".repeat(1000)).len(), MAX_ERROR_CHARS);
    }
}
