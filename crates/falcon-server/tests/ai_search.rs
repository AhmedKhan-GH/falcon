//! The `aiSearch/*` routes, with a stand-in engine.

use std::sync::{Arc, Mutex};

use base64::Engine;
use falcon_library::Library;
use falcon_library::import::ImportOptions;
use falcon_server::{AiQuery, AiSearch, AiStatus};
use image::{Rgb, RgbImage};
use serde_json::{Value, json};

struct Env {
    _tmp: tempfile::TempDir,
    server: falcon_server::Handle,
    agent: ureq::Agent,
    id: String,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::create(tmp.path().join("Test.library")).unwrap();
    let path = tmp.path().join("a.png");
    RgbImage::from_pixel(20, 20, Rgb([1, 2, 3])).save(&path).unwrap();
    let id = lib.import_file(&path, &ImportOptions::default()).unwrap();
    let lib: falcon_server::Shared = Arc::new(Mutex::new(lib));
    let server = falcon_server::start(lib, 41760..41860, || {}).unwrap();
    assert_ne!(server.port, falcon_server::EAGLE_PORT);
    let agent = ureq::Agent::config_builder().http_status_as_error(false).build().new_agent();
    Env { _tmp: tmp, server, agent, id }
}

impl Env {
    fn get(&self, path: &str) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}/api/v2/aiSearch/{path}", self.server.port);
        let mut resp = self.agent.get(&url).call().unwrap();
        (resp.status().as_u16(), serde_json::from_str(&resp.body_mut().read_to_string().unwrap()).unwrap())
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}/api/v2/aiSearch/{path}", self.server.port);
        let mut resp = self.agent.post(&url).header("Content-Type", "application/json").send(body.to_string()).unwrap();
        (resp.status().as_u16(), serde_json::from_str(&resp.body_mut().read_to_string().unwrap()).unwrap())
    }
}

/// Answers with the item it was built with, and notes what it was asked.
struct Fake {
    status: AiStatus,
    id: String,
    asked: Mutex<Vec<String>>,
}

impl AiSearch for Fake {
    fn status(&self) -> AiStatus {
        self.status.clone()
    }

    fn search(&self, query: AiQuery, limit: usize) -> Result<Vec<(String, f32)>, String> {
        let what = match query {
            AiQuery::Text(t) => format!("text:{t}"),
            AiQuery::Image(b) => format!("image:{}", String::from_utf8_lossy(&b)),
            AiQuery::Item(i) => format!("item:{i}"),
        };
        self.asked.lock().unwrap().push(format!("{what}@{limit}"));
        Ok(vec![(self.id.clone(), 0.5), ("gone".to_string(), 0.25)])
    }
}

#[test]
fn without_a_model_every_route_says_so() {
    let e = env();
    for route in ["isInstalled", "isReady", "isStarting", "isSyncing"] {
        assert_eq!(e.get(route), (200, json!({"status": "success", "data": false})), "{route}");
    }
    let (code, v) = e.get("getSyncStatus");
    assert_eq!(code, 200);
    assert_eq!(v["data"], json!({"isSyncing": false, "done": 0, "total": 0, "pending": 0}));
    let (_, v) = e.get("checkServiceHealth");
    assert_eq!(v["data"], json!({"healthy": false, "status": "notInstalled", "error": null}));
    let (code, v) = e.post("searchByText", json!({"query": "a cat"}));
    assert_eq!(code, 500);
    assert!(v["message"].as_str().unwrap().contains("not installed"), "{v}");
    let (code, _) = e.get("searchByItemId?itemId=X");
    assert_eq!(code, 500);
    let (code, v) = e.post("searchByText", json!({}));
    assert_eq!(code, 500);
    assert!(v["message"].as_str().unwrap().contains("missing"), "{v}");
    assert_eq!(e.post("isReady", json!({})).0, 405);
    assert_eq!(e.get("nothingLikeThis").0, 404);
}

#[test]
fn while_loading_searches_say_not_ready() {
    let e = env();
    let status = AiStatus { installed: true, starting: true, total: 3, ..Default::default() };
    e.server.set_ai_search(Arc::new(Fake { status, id: e.id.clone(), asked: Mutex::default() }));
    assert_eq!(e.get("isInstalled").1["data"], true);
    assert_eq!(e.get("isStarting").1["data"], true);
    assert_eq!(e.get("isReady").1["data"], false);
    assert_eq!(e.get("checkServiceHealth").1["data"]["status"], "starting");
    assert_eq!(e.get("getSyncStatus").1["data"]["pending"], 3);
    let (code, v) = e.post("searchByText", json!({"query": "a cat"}));
    assert_eq!((code, v["message"].as_str()), (500, Some("AI Search is not ready")));
}

#[test]
fn searches_answer_items_with_scores_and_pass_the_query_on() {
    let e = env();
    let status = AiStatus { installed: true, ready: true, syncing: true, done: 1, total: 2, ..Default::default() };
    let fake = Arc::new(Fake { status, id: e.id.clone(), asked: Mutex::default() });
    e.server.set_ai_search(fake.clone());
    assert_eq!(e.get("checkServiceHealth").1["data"], json!({"healthy": true, "status": "syncing", "error": null}));
    assert_eq!(e.get("isSyncing").1["data"], true);

    let (code, v) = e.post("searchByText", json!({"query": "a cat", "limit": 7}));
    assert_eq!(code, 200, "{v}");
    let list = &v["data"];
    assert_eq!(list["total"], 1, "an id with no item is left out");
    assert_eq!(list["data"][0]["id"], e.id.as_str());
    assert_eq!(list["data"][0]["score"], 0.5);
    assert_eq!(list["data"][0]["name"], "a");

    let encoded = base64::engine::general_purpose::STANDARD.encode(b"picture");
    assert_eq!(e.post("searchByBase64", json!({"base64": format!("data:image/png;base64,{encoded}")})).0, 200);
    assert_eq!(e.post("searchByBase64", json!({"base64": "%%%"})).0, 500);
    assert_eq!(e.get(&format!("searchByItemId?itemId={}", e.id)).0, 200);
    let asked = fake.asked.lock().unwrap();
    assert_eq!(asked[0], "text:a cat@7");
    assert_eq!(asked[1], "image:picture@50");
    assert_eq!(asked[2], format!("item:{}@50", e.id));
}
