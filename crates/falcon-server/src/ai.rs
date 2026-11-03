//! Eagle's `/api/v2/aiSearch/*` routes. The work is done by whatever the app
//! hands over ([`crate::Handle::set_ai_search`]), so this crate doesn't build
//! the model; with nothing handed over (the `falcon` command line, or the
//! app before it starts) they answer that AI Search isn't installed.
//!
//! PARITY.md knows these routes by name only, so the shapes are falcon's
//! reading of the names: the four `is*` routes answer a boolean, and searches
//! answer the v2 list shape (`data`, `total`, `offset`, `limit`), each item
//! with a `score`. Searching runs without the library lock.

use std::sync::{Arc, PoisonError};

use base64::Engine;
use serde_json::{Map, Value, json};

use crate::{Reply, Shared, State, item_json, lock, number, paginate, s};

/// How AI Search stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiStatus {
    /// The model is on this machine and turned on.
    pub installed: bool,
    /// Searches can be answered now.
    pub ready: bool,
    /// The model is loading.
    pub starting: bool,
    /// Items are being indexed.
    pub syncing: bool,
    /// Items in the library that have been looked at, and how many there are.
    pub done: usize,
    pub total: usize,
    /// The model failed.
    pub error: Option<String>,
}

/// What to search by.
pub enum AiQuery {
    Text(String),
    /// The bytes of a picture file.
    Image(Vec<u8>),
    Item(String),
}

/// AI Search as the app provides it.
pub trait AiSearch: Send + Sync {
    fn status(&self) -> AiStatus;
    /// Item ids and scores, best first.
    fn search(&self, query: AiQuery, limit: usize) -> Result<Vec<(String, f32)>, String>;
}

const NOT_INSTALLED: &str = "AI Search is not installed. Install its model in Preferences > AI Search.";
const NOT_READY: &str = "AI Search is not ready";

/// Answers one `aiSearch` route.
pub(crate) fn route(lib: &Shared, state: &State, path: &str, args: &Map<String, Value>) -> Reply {
    let engine: Option<Arc<dyn AiSearch>> = state.ai.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let status = engine.as_ref().map(|e| e.status()).unwrap_or_default();
    let name = path.rsplit('/').next().unwrap_or("");
    match name {
        "isInstalled" => Reply::Data(json!(status.installed)),
        "isReady" => Reply::Data(json!(status.ready)),
        "isStarting" => Reply::Data(json!(status.starting)),
        "isSyncing" => Reply::Data(json!(status.syncing)),
        "getSyncStatus" => Reply::Data(json!({
            "isSyncing": status.syncing,
            "done": status.done,
            "total": status.total,
            "pending": status.total.saturating_sub(status.done),
        })),
        "checkServiceHealth" => {
            let word = match (&status.error, status.installed, status.starting, status.syncing) {
                (Some(_), ..) => "error",
                (None, false, ..) => "notInstalled",
                (None, true, true, _) => "starting",
                (None, true, false, true) => "syncing",
                (None, true, false, false) => "ready",
            };
            Reply::Data(json!({"healthy": status.ready && status.error.is_none(), "status": word, "error": status.error}))
        }
        "searchByText" | "searchByBase64" | "searchByItemId" => {
            let query = match name {
                "searchByText" => s(args, "query").or_else(|| s(args, "text")).map(|t| AiQuery::Text(t.to_string())),
                "searchByItemId" => s(args, "itemId").or_else(|| s(args, "id")).map(|i| AiQuery::Item(i.to_string())),
                _ => match s(args, "base64").or_else(|| s(args, "image")).map(decode_base64) {
                    Some(Ok(bytes)) => Some(AiQuery::Image(bytes)),
                    Some(Err(message)) => return Reply::Error(message),
                    None => None,
                },
            };
            let Some(query) = query else { return Reply::Error(format!("{name}: missing what to search by")) };
            let Some(engine) = engine.filter(|_| status.installed) else { return Reply::Error(NOT_INSTALLED.to_string()) };
            if !status.ready {
                return Reply::Error(NOT_READY.to_string());
            }
            let limit = (number(args, "limit").filter(|l| *l > 0).unwrap_or(50) as usize).min(1000);
            match engine.search(query, limit) {
                Ok(hits) => {
                    let lib = lock(lib);
                    let items = hits.into_iter().filter_map(|(id, score)| {
                        let mut value = item_json(lib.item(&id)?);
                        if let Value::Object(m) = &mut value {
                            m.insert("score".into(), json!(score));
                        }
                        Some(value)
                    });
                    Reply::Data(paginate(items.collect(), args))
                }
                Err(message) => Reply::Error(message),
            }
        }
        _ => Reply::NotFound,
    }
}

/// The bytes of a picture sent as Base64, with or without a `data:` prefix.
fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let body = if text.starts_with("data:") { text.split_once(',').map_or("", |(_, b)| b) } else { text };
    base64::engine::general_purpose::STANDARD.decode(body.trim()).map_err(|e| format!("base64: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_comes_with_or_without_a_data_prefix() {
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi");
        assert_eq!(decode_base64("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert!(decode_base64("not base64!").is_err());
    }
}
