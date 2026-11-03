//! The client against a local HTTP server that answers as each provider
//! does. Nothing here leaves the Mac.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use falcon_ai::*;
use serde_json::{Value, json};

/// One request as the server saw it.
#[derive(Clone, Debug)]
struct Seen {
    head: String,
    body: String,
}

impl Seen {
    fn body(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }

    fn line(&self) -> &str {
        self.head.lines().next().unwrap()
    }
}

/// A server handing out `reply` to every request and recording them. Each
/// answer is `(status, content type, chunks)`, with `pause` before each chunk.
struct Mock {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).ok()?;
    Some(Seen { head, body: String::from_utf8_lossy(&body).into_owned() })
}

fn mock(status: u16, content_type: &'static str, chunks: Vec<String>, pause: Duration) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let (log, chunks) = (log.clone(), chunks.clone());
            std::thread::spawn(move || {
                let Some(req) = read_request(&mut stream) else { return };
                log.lock().unwrap().push(req);
                let head = format!("HTTP/1.1 {status} X\r\ncontent-type: {content_type}\r\nconnection: close\r\ntransfer-encoding: chunked\r\n\r\n");
                if stream.write_all(head.as_bytes()).is_err() {
                    return;
                }
                for c in &chunks {
                    std::thread::sleep(pause);
                    let framed = format!("{:x}\r\n{c}\r\n", c.len());
                    if stream.write_all(framed.as_bytes()).is_err() || stream.flush().is_err() {
                        return;
                    }
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            });
        }
    });
    Mock { base, seen }
}

fn sse(values: &[Value]) -> Vec<String> {
    let mut v: Vec<String> = values.iter().map(|x| format!("data: {x}\n\n")).collect();
    v.push("data: [DONE]\n\n".into());
    v
}

fn openai_text(pieces: &[&str]) -> Vec<String> {
    sse(&pieces.iter().map(|p| json!({"choices":[{"delta":{"content":p}}]})).collect::<Vec<_>>())
}

fn client_for(id: ProviderId, m: &Mock, keys: Arc<MemoryKeys>) -> Client {
    let mut s = Settings::default();
    s.add(id);
    s.providers[0].base_url = m.base.clone();
    Client::new(s, keys).with_limits(Limits { connect: Duration::from_secs(2), first_byte: Duration::from_secs(5), idle: Duration::from_secs(5), total: Duration::from_secs(10) })
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn hello() -> Request {
    Request { messages: vec![Message::text(Role::User, "hi")], ..Default::default() }
}

fn model(id: ProviderId) -> ModelRef {
    ModelRef { provider: id, model: "m".into() }
}

fn tags_schema() -> Value {
    json!({"type":"object","properties":{"tags":{"type":"array","items":{"type":"string"}}},"required":["tags"],"additionalProperties":false})
}

fn keys_with(id: ProviderId, key: &str) -> Arc<MemoryKeys> {
    let k = MemoryKeys::default();
    k.set(id, &Secret::new(key)).unwrap();
    Arc::new(k)
}

#[test]
fn every_openai_style_provider_generates_and_streams() {
    for id in [ProviderId::OpenAi, ProviderId::DeepSeek, ProviderId::Tongyi, ProviderId::Ollama, ProviderId::LmStudio, ProviderId::OpenAiCompatible] {
        let m = mock(200, "text/event-stream", openai_text(&["Hel", "lo"]), Duration::ZERO);
        let c = client_for(id, &m, keys_with(id, "sk-test"));
        let mut pieces = Vec::new();
        let text = c.stream_text(&model(id), &hello(), &no_cancel(), &mut |p| pieces.push(p.to_string())).unwrap();
        assert_eq!(text, "Hello", "{id:?}");
        assert_eq!(pieces, ["Hel", "lo"]);
        assert_eq!(c.generate_text(&model(id), &hello(), &no_cancel()).unwrap(), "Hello");
        let seen = m.seen.lock().unwrap()[0].clone();
        assert!(seen.line().starts_with("POST /v1/chat/completions"), "{}", seen.line());
        assert_eq!(seen.body()["stream"], true);
        assert_eq!(seen.head.to_lowercase().contains("authorization: bearer sk-test"), id != ProviderId::Ollama && id != ProviderId::LmStudio, "{id:?}");
    }
}

#[test]
fn anthropic_key_header_and_tool_output() {
    let events = sse(&[
        json!({"type":"message_start","message":{}}),
        json!({"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"tags\":"}}),
        json!({"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"[\"a\"]}"}}),
        json!({"type":"message_stop"}),
    ]);
    let m = mock(200, "text/event-stream", events, Duration::ZERO);
    let c = client_for(ProviderId::Anthropic, &m, keys_with(ProviderId::Anthropic, "ant-key"));
    let v = c.generate_object(&model(ProviderId::Anthropic), &hello(), &tags_schema(), &no_cancel()).unwrap();
    assert_eq!(v, json!({"tags":["a"]}));
    let seen = m.seen.lock().unwrap()[0].clone();
    assert!(seen.line().starts_with("POST /v1/messages"));
    let head = seen.head.to_lowercase();
    assert!(head.contains("x-api-key: ant-key") && head.contains("anthropic-version:"));
    assert!(!head.contains("authorization"));
}

#[test]
fn google_key_stays_out_of_the_url() {
    let events = sse(&[json!({"candidates":[{"content":{"parts":[{"text":"Hi"}]}}]})]);
    let m = mock(200, "text/event-stream", events, Duration::ZERO);
    let c = client_for(ProviderId::Google, &m, keys_with(ProviderId::Google, "g-key"));
    assert_eq!(c.generate_text(&model(ProviderId::Google), &hello(), &no_cancel()).unwrap(), "Hi");
    let seen = m.seen.lock().unwrap()[0].clone();
    assert!(seen.line().starts_with("POST /v1/models/m:streamGenerateContent?alt=sse"), "{}", seen.line());
    assert!(!seen.line().contains("g-key"));
    assert!(seen.head.to_lowercase().contains("x-goog-api-key: g-key"));
}

#[test]
fn a_server_that_ignores_stream_still_works() {
    let whole = json!({"choices":[{"message":{"content":"{\"tags\":[\"x\"]}"}}]}).to_string();
    let m = mock(200, "application/json", vec![whole], Duration::ZERO);
    let c = client_for(ProviderId::OpenAiCompatible, &m, Arc::new(MemoryKeys::default()));
    assert_eq!(c.generate_object(&model(ProviderId::OpenAiCompatible), &hello(), &tags_schema(), &no_cancel()).unwrap(), json!({"tags":["x"]}));
}

#[test]
fn images_go_in_the_message() {
    let m = mock(200, "text/event-stream", openai_text(&["ok"]), Duration::ZERO);
    let c = client_for(ProviderId::Ollama, &m, Arc::new(MemoryKeys::default()));
    let req = Request { messages: vec![Message::user(vec![Part::Text("what".into()), Part::Image { mime: "image/jpeg".into(), data: Arc::new(vec![255, 0]) }])], ..Default::default() };
    c.generate_text(&model(ProviderId::Ollama), &req, &no_cancel()).unwrap();
    let body = m.seen.lock().unwrap()[0].body();
    assert_eq!(body["messages"][0]["content"][1]["image_url"]["url"], "data:image/jpeg;base64,/wA=");
}

#[test]
fn bad_structured_output_is_refused() {
    let cases: [(&[&str], BadOutput); 4] = [
        (&["not json"], BadOutput::NotJson),
        (&["{\"tags\":[1]}"], BadOutput::Schema("$.tags[0]: expected string, got number".into())),
        (&["{\"tags\":[],\"run\":\"delete everything\"}"], BadOutput::Schema("$: unexpected field run".into())),
        (&["  "], BadOutput::Empty),
    ];
    for (pieces, want) in cases {
        let m = mock(200, "text/event-stream", openai_text(pieces), Duration::ZERO);
        let c = client_for(ProviderId::Ollama, &m, Arc::new(MemoryKeys::default()));
        assert_eq!(c.generate_object(&model(ProviderId::Ollama), &hello(), &tags_schema(), &no_cancel()), Err(AiError::Bad(want)));
    }
}

#[test]
fn a_cut_reply_says_so() {
    let events = sse(&[json!({"choices":[{"delta":{"content":"{\"tags\":[\"a"}}]}), json!({"choices":[{"delta":{},"finish_reason":"length"}]})]);
    let m = mock(200, "text/event-stream", events, Duration::ZERO);
    let c = client_for(ProviderId::Ollama, &m, Arc::new(MemoryKeys::default()));
    assert_eq!(c.generate_object(&model(ProviderId::Ollama), &hello(), &tags_schema(), &no_cancel()), Err(AiError::Bad(BadOutput::Cut)));
}

#[test]
fn errors_never_hold_the_key() {
    let key = "sk-very-secret-123";
    let reply = json!({"error":{"message":format!("Incorrect API key provided: {key}")}}).to_string();
    let m = mock(401, "application/json", vec![reply], Duration::ZERO);
    let c = client_for(ProviderId::OpenAi, &m, keys_with(ProviderId::OpenAi, key));
    let e = c.list_models(ProviderId::OpenAi, &no_cancel()).unwrap_err();
    assert!(e.is_auth(), "{e:?}");
    let text = format!("{e:?}");
    assert!(!text.contains(key), "{text}");
    assert!(text.contains("[key]"), "{text}");
    // Nor does a broken connection's error.
    let c = {
        let mut s = Settings::default();
        s.add(ProviderId::OpenAi);
        s.providers[0].base_url = "https://127.0.0.1:1/v1".into();
        Client::new(s, keys_with(ProviderId::OpenAi, key))
    };
    let e = c.generate_text(&model(ProviderId::OpenAi), &hello(), &no_cancel()).unwrap_err();
    assert!(matches!(e, AiError::Network(_)), "{e:?}");
    assert!(!format!("{e:?}").contains(key));
}

#[test]
fn redirects_are_not_followed() {
    let m = mock(307, "text/plain", vec!["moved".into()], Duration::ZERO);
    let c = client_for(ProviderId::Ollama, &m, Arc::new(MemoryKeys::default()));
    assert!(matches!(c.generate_text(&model(ProviderId::Ollama), &hello(), &no_cancel()), Err(AiError::Http { status: 307, .. })));
    assert_eq!(m.seen.lock().unwrap().len(), 1);
}

#[test]
fn model_lists_and_verify() {
    let m = mock(200, "application/json", vec![json!({"data":[{"id":"qwen3.8:27b"},{"id":"a"}]}).to_string()], Duration::ZERO);
    let c = client_for(ProviderId::Ollama, &m, Arc::new(MemoryKeys::default()));
    assert_eq!(c.list_models(ProviderId::Ollama, &no_cancel()).unwrap(), ["a", "qwen3.8:27b"]);
    assert!(m.seen.lock().unwrap()[0].line().starts_with("GET /v1/models"));
}

#[test]
fn a_stalled_reply_times_out() {
    let m = mock(200, "text/event-stream", openai_text(&["slow"]), Duration::from_secs(5));
    let mut s = Settings::default();
    s.add(ProviderId::Ollama);
    s.providers[0].base_url = m.base.clone();
    let c = Client::new(s, Arc::new(MemoryKeys::default())).with_limits(Limits { connect: Duration::from_secs(1), first_byte: Duration::from_millis(200), idle: Duration::from_millis(200), total: Duration::from_secs(3) });
    let at = Instant::now();
    assert_eq!(c.generate_text(&model(ProviderId::Ollama), &hello(), &no_cancel()), Err(AiError::Timeout));
    assert!(at.elapsed() < Duration::from_secs(2));
}

#[test]
fn cancel_returns_at_once() {
    let m = mock(200, "text/event-stream", openai_text(&["slow"]), Duration::from_secs(5));
    let c = client_for(ProviderId::Ollama, &m, Arc::new(MemoryKeys::default()));
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        flag.store(true, Ordering::Relaxed);
    });
    let at = Instant::now();
    assert_eq!(c.generate_text(&model(ProviderId::Ollama), &hello(), &cancel), Err(AiError::Cancelled));
    assert!(at.elapsed() < Duration::from_secs(2));
}

#[test]
fn nothing_goes_out_for_a_provider_that_was_not_added() {
    // The default base URLs would reach the real services; none is contacted.
    let c = Client::new(Settings::default(), Arc::new(MemoryKeys::default()));
    for id in ProviderId::ALL {
        assert_eq!(c.generate_text(&model(id), &hello(), &no_cancel()), Err(AiError::NotAdded(id)));
        assert_eq!(c.list_models(id, &no_cancel()), Err(AiError::NotAdded(id)));
    }
}

#[test]
fn a_cloud_provider_without_a_key_sends_nothing() {
    let mut s = Settings::default();
    s.add(ProviderId::Anthropic);
    let c = Client::new(s, Arc::new(MemoryKeys::default()));
    assert_eq!(c.generate_text(&model(ProviderId::Anthropic), &hello(), &no_cancel()), Err(AiError::NoKey(ProviderId::Anthropic)));
}
