//! What each provider's API looks like on the wire: request bodies, model
//! lists and the events of a streamed reply. Six of the eight speak the
//! OpenAI chat format; Anthropic and Google have their own.

use base64::Engine as _;
use serde_json::{Map, Value, json};

use crate::client::{AiError, Part, Request, Role};
use crate::provider::ProviderId;

/// Anthropic requires a limit on the reply; the others take none by default.
const ANTHROPIC_MAX_TOKENS: u32 = 4096;
/// The name of the forced output tool on Anthropic.
const OUTPUT_TOOL: &str = "output";

/// Which family of API a provider speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Dialect {
    OpenAi,
    Anthropic,
    Google,
}

pub(crate) fn dialect(p: ProviderId) -> Dialect {
    match p {
        ProviderId::Anthropic => Dialect::Anthropic,
        ProviderId::Google => Dialect::Google,
        _ => Dialect::OpenAi,
    }
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Percent-encodes a model id for use in a URL path.
fn path_part(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The path and body of a streamed chat request.
pub(crate) fn chat_request(provider: ProviderId, model: &str, req: &Request, schema: Option<&Value>) -> (String, Value) {
    match dialect(provider) {
        Dialect::OpenAi => ("/chat/completions".to_string(), openai_body(provider, model, req, schema)),
        Dialect::Anthropic => ("/messages".to_string(), anthropic_body(model, req, schema)),
        Dialect::Google => (format!("/models/{}:streamGenerateContent?alt=sse", path_part(model)), google_body(req, schema)),
    }
}

/// DeepSeek and Tongyi's OpenAI-compatible modes take `json_object`, with
/// the schema in the instructions; the reply is checked against it either way.
fn wants_json_object(p: ProviderId) -> bool {
    matches!(p, ProviderId::DeepSeek | ProviderId::Tongyi)
}

fn openai_body(provider: ProviderId, model: &str, req: &Request, schema: Option<&Value>) -> Value {
    let mut messages = Vec::new();
    let mut system = req.system.clone().unwrap_or_default();
    if let Some(s) = schema.filter(|_| wants_json_object(provider)) {
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(&format!("Reply with one JSON object that fits this JSON schema, and nothing else: {s}"));
    }
    if !system.is_empty() {
        messages.push(json!({"role":"system","content":system}));
    }
    for m in &req.messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let content = match m.parts.as_slice() {
            [Part::Text(t)] => Value::String(t.clone()),
            parts => Value::Array(
                parts
                    .iter()
                    .map(|p| match p {
                        Part::Text(t) => json!({"type":"text","text":t}),
                        Part::Image { mime, data } => json!({"type":"image_url","image_url":{"url":format!("data:{mime};base64,{}", b64(data))}}),
                    })
                    .collect(),
            ),
        };
        messages.push(json!({"role":role,"content":content}));
    }
    let mut body = Map::new();
    body.insert("model".into(), model.into());
    body.insert("stream".into(), true.into());
    body.insert("messages".into(), messages.into());
    if let Some(n) = req.max_tokens {
        let key = if provider == ProviderId::OpenAi { "max_completion_tokens" } else { "max_tokens" };
        body.insert(key.into(), n.into());
    }
    if let Some(t) = req.temperature {
        body.insert("temperature".into(), t.into());
    }
    // A thinking model spends minutes on what it won't show; Ollama turns
    // that off through this field and ignores it for other models.
    if provider == ProviderId::Ollama {
        body.insert("reasoning_effort".into(), "none".into());
    }
    if let Some(s) = schema {
        let format = if wants_json_object(provider) { json!({"type":"json_object"}) } else { json!({"type":"json_schema","json_schema":{"name":OUTPUT_TOOL,"strict":true,"schema":s}}) };
        body.insert("response_format".into(), format);
    }
    Value::Object(body)
}

fn anthropic_body(model: &str, req: &Request, schema: Option<&Value>) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let content: Vec<Value> = m
                .parts
                .iter()
                .map(|p| match p {
                    Part::Text(t) => json!({"type":"text","text":t}),
                    Part::Image { mime, data } => json!({"type":"image","source":{"type":"base64","media_type":mime,"data":b64(data)}}),
                })
                .collect();
            json!({"role":role,"content":content})
        })
        .collect();
    let mut body = Map::new();
    body.insert("model".into(), model.into());
    body.insert("stream".into(), true.into());
    body.insert("max_tokens".into(), req.max_tokens.unwrap_or(ANTHROPIC_MAX_TOKENS).into());
    body.insert("messages".into(), messages.into());
    if let Some(s) = &req.system {
        body.insert("system".into(), s.clone().into());
    }
    if let Some(t) = req.temperature {
        body.insert("temperature".into(), t.into());
    }
    if let Some(s) = schema {
        // The only tool, forced: its input is the structured reply. No other
        // tool exists, and nothing runs it.
        body.insert("tools".into(), json!([{"name":OUTPUT_TOOL,"description":"Records the reply.","input_schema":s}]));
        body.insert("tool_choice".into(), json!({"type":"tool","name":OUTPUT_TOOL}));
    }
    Value::Object(body)
}

/// Gemini's `responseSchema` is a subset of JSON Schema with upper-case
/// types; other keywords are dropped (the reply is checked locally anyway).
fn google_schema(s: &Value) -> Value {
    let Some(map) = s.as_object() else { return s.clone() };
    let mut out = Map::new();
    for (k, v) in map {
        match k.as_str() {
            "type" => {
                out.insert(k.clone(), v.as_str().map_or_else(|| v.clone(), |t| t.to_uppercase().into()));
            }
            "properties" => {
                let props = v.as_object().map(|p| p.iter().map(|(n, sub)| (n.clone(), google_schema(sub))).collect::<Map<_, _>>()).unwrap_or_default();
                out.insert(k.clone(), Value::Object(props));
            }
            "items" => {
                out.insert(k.clone(), google_schema(v));
            }
            "required" | "enum" | "minItems" | "maxItems" | "description" | "nullable" => {
                out.insert(k.clone(), v.clone());
            }
            _ => {}
        }
    }
    Value::Object(out)
}

fn google_body(req: &Request, schema: Option<&Value>) -> Value {
    let contents: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "model",
            };
            let parts: Vec<Value> = m
                .parts
                .iter()
                .map(|p| match p {
                    Part::Text(t) => json!({"text":t}),
                    Part::Image { mime, data } => json!({"inlineData":{"mimeType":mime,"data":b64(data)}}),
                })
                .collect();
            json!({"role":role,"parts":parts})
        })
        .collect();
    let mut body = Map::new();
    body.insert("contents".into(), contents.into());
    if let Some(s) = &req.system {
        body.insert("systemInstruction".into(), json!({"parts":[{"text":s}]}));
    }
    let mut config = Map::new();
    if let Some(n) = req.max_tokens {
        config.insert("maxOutputTokens".into(), n.into());
    }
    if let Some(t) = req.temperature {
        config.insert("temperature".into(), t.into());
    }
    if let Some(s) = schema {
        config.insert("responseMimeType".into(), "application/json".into());
        config.insert("responseSchema".into(), google_schema(s));
    }
    if !config.is_empty() {
        body.insert("generationConfig".into(), Value::Object(config));
    }
    Value::Object(body)
}

/// The path of the provider's model list.
pub(crate) fn models_path(provider: ProviderId) -> &'static str {
    match dialect(provider) {
        Dialect::OpenAi => "/models",
        Dialect::Anthropic => "/models?limit=1000",
        Dialect::Google => "/models?pageSize=1000",
    }
}

/// The model ids in a list reply, sorted. `None` when it isn't a list.
pub(crate) fn parse_models(provider: ProviderId, v: &Value) -> Option<Vec<String>> {
    let mut ids: Vec<String> = match dialect(provider) {
        Dialect::OpenAi | Dialect::Anthropic => v.get("data")?.as_array()?.iter().filter_map(|m| m.get("id")?.as_str().map(str::to_string)).collect(),
        Dialect::Google => v
            .get("models")?
            .as_array()?
            .iter()
            .filter(|m| m.get("supportedGenerationMethods").and_then(Value::as_array).is_none_or(|a| a.iter().any(|x| x == "generateContent")))
            .filter_map(|m| m.get("name")?.as_str().map(|n| n.strip_prefix("models/").unwrap_or(n).to_string()))
            .collect(),
    };
    ids.sort();
    ids.dedup();
    Some(ids)
}

/// What the events of a reply add up to.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Reply {
    pub text: String,
    /// The provider stopped at its length limit.
    pub cut: bool,
}

fn stream_error(v: &Value) -> Option<String> {
    let e = v.get("error")?;
    Some(e.get("message").and_then(Value::as_str).or(e.as_str()).unwrap_or("error").to_string())
}

/// Takes one event of a streamed reply (or a whole reply, from a server
/// that ignored `stream`), adds its text to `reply` and returns the new text.
pub(crate) fn feed(provider: ProviderId, v: &Value, reply: &mut Reply) -> Result<String, AiError> {
    if let Some(e) = stream_error(v) {
        return Err(AiError::Provider(e));
    }
    let mut new = String::new();
    match dialect(provider) {
        Dialect::OpenAi => {
            if let Some(choice) = v.get("choices").and_then(|c| c.get(0)) {
                // `delta` when streamed, `message` from a whole reply. Reasoning
                // fields are not part of the reply.
                if let Some(t) = choice.get("delta").or(choice.get("message")).and_then(|d| d.get("content")).and_then(Value::as_str) {
                    new.push_str(t);
                }
                if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
                    reply.cut = true;
                }
            }
        }
        Dialect::Anthropic => match v.get("type").and_then(Value::as_str) {
            Some("content_block_delta") => {
                let d = v.get("delta");
                for key in ["text", "partial_json"] {
                    if let Some(t) = d.and_then(|d| d.get(key)).and_then(Value::as_str) {
                        new.push_str(t);
                    }
                }
            }
            Some("message_delta") => {
                if v.get("delta").and_then(|d| d.get("stop_reason")).and_then(Value::as_str) == Some("max_tokens") {
                    reply.cut = true;
                }
            }
            Some("message") => {
                for block in v.get("content").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        new.push_str(t);
                    } else if let Some(input) = block.get("input") {
                        new.push_str(&input.to_string());
                    }
                }
                if v.get("stop_reason").and_then(Value::as_str) == Some("max_tokens") {
                    reply.cut = true;
                }
            }
            _ => {}
        },
        Dialect::Google => {
            if let Some(why) = v.get("promptFeedback").and_then(|f| f.get("blockReason")).and_then(Value::as_str) {
                return Err(AiError::Provider(format!("The request was blocked: {why}")));
            }
            if let Some(c) = v.get("candidates").and_then(|c| c.get(0)) {
                for part in c.get("content").and_then(|c| c.get("parts")).and_then(Value::as_array).into_iter().flatten() {
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        new.push_str(t);
                    }
                }
                if c.get("finishReason").and_then(Value::as_str) == Some("MAX_TOKENS") {
                    reply.cut = true;
                }
            }
        }
    }
    reply.text.push_str(&new);
    Ok(new)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::client::Message;

    fn request() -> Request {
        Request {
            system: Some("sys".into()),
            messages: vec![Message::user(vec![Part::Text("hi".into()), Part::Image { mime: "image/jpeg".into(), data: Arc::new(vec![1, 2, 3]) }])],
            max_tokens: Some(10),
            temperature: None,
        }
    }

    fn schema() -> Value {
        json!({"type":"object","properties":{"tags":{"type":"array","items":{"type":"string"}}},"required":["tags"],"additionalProperties":false})
    }

    #[test]
    fn openai_shape() {
        let (path, b) = chat_request(ProviderId::OpenAi, "gpt", &request(), Some(&schema()));
        assert_eq!(path, "/chat/completions");
        assert_eq!(b["messages"][0], json!({"role":"system","content":"sys"}));
        assert_eq!(b["messages"][1]["content"][1]["image_url"]["url"], "data:image/jpeg;base64,AQID");
        assert_eq!(b["max_completion_tokens"], 10);
        assert_eq!(b["response_format"]["json_schema"]["schema"], schema());
        assert!(b.get("reasoning_effort").is_none());
    }

    #[test]
    fn deepseek_and_tongyi_ask_for_json_objects_with_the_schema_in_the_instructions() {
        for p in [ProviderId::DeepSeek, ProviderId::Tongyi] {
            let (_, b) = chat_request(p, "m", &request(), Some(&schema()));
            assert_eq!(b["response_format"], json!({"type":"json_object"}));
            assert!(b["messages"][0]["content"].as_str().unwrap().contains("JSON schema"));
            assert_eq!(b["max_tokens"], 10);
        }
    }

    #[test]
    fn ollama_skips_thinking() {
        let (_, b) = chat_request(ProviderId::Ollama, "qwen3.8:27b", &request(), None);
        assert_eq!(b["reasoning_effort"], "none");
        assert_eq!(b["model"], "qwen3.8:27b");
    }

    #[test]
    fn anthropic_shape() {
        let mut r = request();
        r.max_tokens = None;
        let (path, b) = chat_request(ProviderId::Anthropic, "c", &r, Some(&schema()));
        assert_eq!(path, "/messages");
        assert_eq!(b["system"], "sys");
        assert_eq!(b["max_tokens"], 4096);
        assert_eq!(b["messages"][0]["content"][1]["source"]["data"], "AQID");
        assert_eq!(b["tools"].as_array().unwrap().len(), 1, "the output tool only");
        assert_eq!(b["tool_choice"]["name"], "output");
    }

    #[test]
    fn google_shape() {
        let (path, b) = chat_request(ProviderId::Google, "gemini x", &request(), Some(&schema()));
        assert_eq!(path, "/models/gemini%20x:streamGenerateContent?alt=sse");
        assert!(!path.to_lowercase().contains("key="), "the key goes in a header");
        assert_eq!(b["systemInstruction"]["parts"][0]["text"], "sys");
        assert_eq!(b["contents"][0]["parts"][1]["inlineData"]["data"], "AQID");
        let rs = &b["generationConfig"]["responseSchema"];
        assert_eq!(rs["type"], "OBJECT");
        assert_eq!(rs["properties"]["tags"]["items"]["type"], "STRING");
        assert!(rs.get("additionalProperties").is_none());
    }

    #[test]
    fn model_lists() {
        assert_eq!(parse_models(ProviderId::Ollama, &json!({"data":[{"id":"b"},{"id":"a"},{"id":"a"}]})).unwrap(), ["a", "b"]);
        assert_eq!(parse_models(ProviderId::Anthropic, &json!({"data":[{"id":"c"}]})).unwrap(), ["c"]);
        let g = json!({"models":[{"name":"models/g1","supportedGenerationMethods":["generateContent"]},{"name":"models/e","supportedGenerationMethods":["embedContent"]}]});
        assert_eq!(parse_models(ProviderId::Google, &g).unwrap(), ["g1"]);
        assert_eq!(parse_models(ProviderId::OpenAi, &json!({"nope":1})), None);
    }

    #[test]
    fn events_add_up() {
        let mut r = Reply::default();
        assert_eq!(feed(ProviderId::Ollama, &json!({"choices":[{"delta":{"reasoning":"hmm"}}]}), &mut r).unwrap(), "");
        feed(ProviderId::Ollama, &json!({"choices":[{"delta":{"content":"a"}}]}), &mut r).unwrap();
        feed(ProviderId::Ollama, &json!({"choices":[{"delta":{},"finish_reason":"length"}]}), &mut r).unwrap();
        assert_eq!(r, Reply { text: "a".into(), cut: true });
        let mut r = Reply::default();
        feed(ProviderId::Anthropic, &json!({"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"a\""}}), &mut r).unwrap();
        feed(ProviderId::Anthropic, &json!({"type":"content_block_delta","delta":{"type":"text_delta","text":":1}"}}), &mut r).unwrap();
        assert_eq!(r.text, "{\"a\":1}");
        let mut r = Reply::default();
        feed(ProviderId::Google, &json!({"candidates":[{"content":{"parts":[{"text":"x"},{"text":"y"}]},"finishReason":"MAX_TOKENS"}]}), &mut r).unwrap();
        assert_eq!(r, Reply { text: "xy".into(), cut: true });
    }

    #[test]
    fn errors_inside_a_stream_stop_it() {
        let mut r = Reply::default();
        assert_eq!(feed(ProviderId::OpenAi, &json!({"error":{"message":"overloaded"}}), &mut r), Err(AiError::Provider("overloaded".into())));
        assert!(matches!(feed(ProviderId::Google, &json!({"promptFeedback":{"blockReason":"SAFETY"}}), &mut r), Err(AiError::Provider(_))));
    }
}
