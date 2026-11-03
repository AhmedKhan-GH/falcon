//! Against the real Ollama on this Mac, with the model the user chose. Run
//! with `cargo test -p falcon-ai --test ollama -- --ignored --nocapture`.
//! The model is large, so a first answer can take minutes.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use falcon_ai::*;
use serde_json::json;

const MODEL: &str = "ollama::qwen3.8:27b";

fn client() -> Client {
    let mut s = Settings::default();
    s.add(ProviderId::Ollama);
    Client::new(s, Arc::new(MemoryKeys::default()))
}

/// A 16×16 solid red PNG.
fn red_png() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAF0lEQVR4nGP4z8BAEiJN9aiGUQ1DSgMAkPn/Afnh+ngAAAAASUVORK5CYII=")
        .unwrap()
}

#[test]
#[ignore = "needs Ollama with qwen3.8:27b"]
fn real_ollama_text_stream_object_and_image() {
    let c = client();
    let model = ModelRef::parse(MODEL).unwrap();
    let no = AtomicBool::new(false);

    let at = Instant::now();
    let list = c.list_models(ProviderId::Ollama, &no).unwrap();
    assert!(list.iter().any(|m| m == "qwen3.8:27b"), "{list:?}");
    println!("models: {} in {:?}", list.len(), at.elapsed());

    let at = Instant::now();
    let hi = Request { messages: vec![Message::text(Role::User, "Reply with the single word: ready")], ..Default::default() };
    let text = c.generate_text(&model, &hi, &no).unwrap();
    println!("text in {:?}: {text}", at.elapsed());
    assert!(text.to_lowercase().contains("ready"), "{text}");

    let at = Instant::now();
    let mut pieces = 0;
    let streamed = c.stream_text(&model, &hi, &no, &mut |_| pieces += 1).unwrap();
    println!("stream in {:?}: {pieces} pieces: {streamed}", at.elapsed());
    assert!(pieces >= 1);

    let schema = json!({"type":"object","properties":{"tags":{"type":"array","items":{"type":"string"}}},"required":["tags"],"additionalProperties":false});
    let at = Instant::now();
    let ask = Request { system: Some("Reply with JSON only.".into()), messages: vec![Message::text(Role::User, "Give three tags for a photo of a cat on a sofa.")], ..Default::default() };
    let object = c.generate_object(&model, &ask, &schema, &no).unwrap();
    println!("object in {:?}: {object}", at.elapsed());
    assert!(!object["tags"].as_array().unwrap().is_empty());

    let at = Instant::now();
    let see = Request {
        messages: vec![Message::user(vec![Part::Text("What colour fills this image? Reply with one word.".into()), Part::Image { mime: "image/png".into(), data: Arc::new(red_png()) }])],
        ..Default::default()
    };
    let colour = c.generate_text(&model, &see, &no).unwrap();
    println!("image in {:?}: {colour}", at.elapsed());
    assert!(colour.to_lowercase().contains("red"), "{colour}");
}
