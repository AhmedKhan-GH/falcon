//! The HTTP API end to end: a server on a free port (41650 and up, never
//! Eagle's 41595) over a small library built in a temp folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use falcon_library::Library;
use falcon_library::import::ImportOptions;
use falcon_server::Handle;
use image::{Rgb, RgbImage};
use serde_json::{Value, json};

struct Env {
    tmp: tempfile::TempDir,
    root: PathBuf,
    lib: falcon_server::Shared,
    server: Handle,
    agent: ureq::Agent,
    /// Tagged `red`, `blue`, in `folder`.
    a: String,
    /// Tagged `blue`.
    b: String,
    folder: String,
    sub: String,
}

fn png(path: &Path, w: u32, h: u32) {
    RgbImage::from_fn(w, h, |x, y| Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 90])).save(path).unwrap();
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Test.library");
    let mut lib = Library::create(&root).unwrap();
    let folder = lib.create_folder(None, "Logos").unwrap();
    let sub = lib.create_folder(Some(&folder), "Old").unwrap();
    let (pa, pb) = (tmp.path().join("a.png"), tmp.path().join("b.png"));
    png(&pa, 40, 20);
    png(&pb, 20, 40);
    let opts = |tags: &[&str], folders: Vec<String>| ImportOptions {
        folders,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        palette: false,
        annotation: String::new(),
        url: String::new(),
        ..Default::default()
    };
    let a = lib.import_file(&pa, &opts(&["red", "blue"], vec![folder.clone()])).unwrap();
    let b = lib.import_file(&pb, &opts(&["blue"], vec![])).unwrap();
    lib.record_tag_use(&["blue".into()]).unwrap();
    lib.set_tag_starred("red", true).unwrap();
    let lib: falcon_server::Shared = Arc::new(Mutex::new(lib));
    let server = falcon_server::start(lib.clone(), 41650..41750, || {}).unwrap();
    assert_ne!(server.port, falcon_server::EAGLE_PORT);
    let agent = ureq::Agent::config_builder().http_status_as_error(false).build().new_agent();
    Env { tmp, root, lib, server, agent, a, b, folder, sub }
}

impl Env {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.server.port)
    }

    fn get(&self, path: &str) -> (u16, Value) {
        let mut resp = self.agent.get(&self.url(path)).call().unwrap();
        let code = resp.status().as_u16();
        (code, serde_json::from_str(&resp.body_mut().read_to_string().unwrap()).unwrap())
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let mut resp = self.agent.post(&self.url(path)).header("Content-Type", "application/json").send(body.to_string()).unwrap();
        let code = resp.status().as_u16();
        (code, serde_json::from_str(&resp.body_mut().read_to_string().unwrap()).unwrap())
    }

    /// The `data` of a successful reply.
    fn ok_get(&self, path: &str) -> Value {
        let (code, v) = self.get(path);
        assert_eq!(code, 200, "{path}: {v}");
        v["data"].clone()
    }

    fn ok_post(&self, path: &str, body: Value) -> Value {
        let (code, v) = self.post(path, body);
        assert_eq!(code, 200, "{path}: {v}");
        assert_eq!(v["status"], "success");
        v["data"].clone()
    }

    /// A thrown-error reply: 500 with the text in `message`.
    fn thrown(&self, path: &str, body: Value) -> String {
        let (code, v) = self.post(path, body);
        assert_eq!(code, 500, "{path}: {v}");
        assert_eq!(v["data"], json!({}));
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["status", "message", "data"]);
        v["message"].as_str().unwrap().to_string()
    }

    fn reopen(&self) -> Library {
        Library::open(&self.root).unwrap()
    }
}

#[test]
fn comments_round_trip() {
    let e = env();
    assert_eq!(e.ok_get(&format!("/api/v2/item/getComments?id={}", e.a)), json!([]));
    let (code, v) = e.get("/api/v2/item/getComments?id=NOPE");
    assert_eq!((code, v["message"].as_str()), (500, Some("item not found: NOPE")));
    assert_eq!(e.get("/api/v2/item/getComments").1["message"], "id is required");

    let c = e.ok_post("/api/v2/item/addComment", json!({"id": e.a, "x": 1.4, "y": 2.6, "width": 10, "height": 5, "annotation": "hi"}));
    let keys: Vec<&String> = c.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["id", "annotation", "lastModified", "x", "y", "width", "height"]);
    assert_eq!((c["x"].clone(), c["y"].clone(), c["annotation"].clone()), (json!(1), json!(3), json!("hi")));
    let cid = c["id"].as_str().unwrap().to_string();

    let msg = |body| e.thrown("/api/v2/item/addComment", body);
    assert_eq!(msg(json!({"id": e.a})), "must provide either duration (video) or x/y/width/height (image)");
    assert_eq!(msg(json!({"id": e.a, "duration": 1, "x": 1})), "cannot have both duration and x/y/width/height");
    assert_eq!(msg(json!({"id": e.a, "x": 1})), "x, y, width, height must all be numbers");
    assert_eq!(msg(json!({"id": e.a, "x": 1, "y": 1, "width": 0, "height": 1})), "width and height must be > 0");
    assert_eq!(msg(json!({"id": e.b, "duration": -1})), "duration must be >= 0");

    // Timed notes stay sorted by time.
    e.ok_post("/api/v2/item/addComment", json!({"id": e.b, "duration": 5.5}));
    let early = e.ok_post("/api/v2/item/addComment", json!({"id": e.b, "duration": 1}));
    let list = e.ok_get(&format!("/api/v2/item/getComments?id={}", e.b));
    assert_eq!(list[0]["id"], early["id"]);
    assert_eq!(list[1]["duration"], 5.5);

    let u = e.ok_post("/api/v2/item/updateComment", json!({"id": e.a, "commentId": cid, "annotation": "edited", "width": 20.2}));
    assert_eq!((u["annotation"].clone(), u["width"].clone()), (json!("edited"), json!(20)));
    let up = |body| e.thrown("/api/v2/item/updateComment", body);
    assert_eq!(up(json!({"id": e.a})), "commentId is required");
    assert_eq!(up(json!({"id": e.a, "commentId": "X"})), "comment not found: X");
    assert_eq!(up(json!({"id": e.a, "commentId": cid, "duration": 3})), "cannot set duration on an image comment");
    assert_eq!(up(json!({"id": e.a, "commentId": cid, "height": -1})), "height must be > 0");
    assert_eq!(up(json!({"id": e.b, "commentId": early["id"], "x": 3})), "cannot set x/y/width/height on a video comment");

    // On disk.
    let lib = e.reopen();
    let saved = lib.item(&e.a).unwrap().comments();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].annotation, "edited");

    assert_eq!(e.ok_post("/api/v2/item/removeComment", json!({"id": e.a, "commentId": cid})), json!(true));
    assert_eq!(e.thrown("/api/v2/item/removeComment", json!({"id": e.a, "commentId": cid})), "no comments found");
    assert!(e.reopen().item(&e.a).unwrap().comments().is_empty());
}

#[test]
fn tag_groups_round_trip() {
    let e = env();
    assert_eq!(e.ok_get("/api/v2/tagGroup/get"), json!({"data": [], "total": 0, "offset": 0, "limit": 50}));
    assert_eq!(e.ok_post("/api/v2/tagGroup/create", json!({})), json!(false));
    let g = e.ok_post("/api/v2/tagGroup/create", json!({"name": "Colours", "tags": ["red"], "color": "red", "description": "d"}));
    assert_eq!((g["name"].clone(), g["tags"].clone(), g["color"].clone(), g["description"].clone()), (json!("Colours"), json!(["red"]), json!("red"), json!("d")));
    let id = g["id"].as_str().unwrap().to_string();
    let other = e.ok_post("/api/v2/tagGroup/create", json!({"name": "Other", "tags": ["blue"]}))["id"].as_str().unwrap().to_string();

    // Without removeFromSource a tag may sit in two groups; with it, it moves.
    let g = e.ok_post("/api/v2/tagGroup/addTags", json!({"groupId": id, "tags": ["blue"]}));
    assert_eq!(g["tags"], json!(["red", "blue"]));
    assert_eq!(e.reopen().metadata.tags_groups[1].tags, ["blue"]);
    e.ok_post("/api/v2/tagGroup/removeTags", json!({"groupId": id, "tags": ["blue"]}));
    e.ok_post("/api/v2/tagGroup/addTags", json!({"groupId": id, "tags": ["blue"], "removeFromSource": true}));
    assert!(e.reopen().metadata.tags_groups[1].tags.is_empty());
    assert_eq!(e.thrown("/api/v2/tagGroup/addTags", json!({"groupId": id})), "tagGroup.addTags requires \"tags\" array");
    assert_eq!(e.thrown("/api/v2/tagGroup/removeTags", json!({"groupId": id, "tags": "x"})), "tagGroup.removeTags requires \"tags\" array");
    let (_, v) = e.post("/api/v2/tagGroup/addTags", json!({"groupId": "NOPE", "tags": []}));
    assert_eq!(v, json!({"status": "success"}));

    let g = e.ok_post("/api/v2/tagGroup/update", json!({"id": id, "name": "Renamed", "tags": ["x", "y"], "color": "blue"}));
    assert_eq!((g["name"].clone(), g["tags"].clone(), g["color"].clone()), (json!("Renamed"), json!(["x", "y"]), json!("blue")));
    assert_eq!(e.post("/api/v2/tagGroup/update", json!({"id": "NOPE"})).1, json!({"status": "success"}));
    assert!(e.thrown("/api/v2/tagGroup/update", json!({"id": id, "color": "beige"})).contains("unknown colour"));

    assert_eq!(e.ok_post("/api/v2/tagGroup/remove", json!({"id": other})), json!(true));
    assert_eq!(e.ok_post("/api/v2/tagGroup/remove", json!({"id": other})), json!(false));
    let lib = e.reopen();
    assert_eq!(lib.metadata.tags_groups.len(), 1);
    assert_eq!(lib.metadata.tags_groups[0].name, "Renamed");
    // Removing a group leaves its tags on items.
    assert!(lib.item(&e.b).unwrap().tags.contains(&"blue".to_string()));
}

#[test]
fn tags_rename_merge_and_lists() {
    let e = env();
    assert_eq!(e.ok_get("/api/v2/tag/getRecentTags")["data"], json!([{"name": "blue", "imageCount": 2}]));
    assert_eq!(e.ok_get("/api/v2/tag/getStarredTags")["data"], json!([{"name": "red", "imageCount": 1}]));
    assert_eq!(e.ok_post("/api/v2/tag/get", json!({"name": "RE"}))["data"], json!([{"name": "red", "imageCount": 1}]));
    assert_eq!(e.ok_get("/api/v2/tag/get?name=lu")["total"], 1);

    let t = e.ok_post("/api/v2/tag/update", json!({"originalName": "red", "name": "crimson"}));
    assert_eq!(t, json!({"name": "crimson", "imageCount": 1}));
    assert_eq!(e.post("/api/v2/tag/update", json!({"originalName": "nope", "name": "x"})).1, json!({"status": "success"}));
    assert_eq!(e.reopen().item(&e.a).unwrap().tags, ["crimson", "blue"]);

    assert_eq!(e.thrown("/api/v2/tag/merge", json!({"source": "blue"})), "tag.merge requires \"source\" and \"target\"");
    assert_eq!(e.thrown("/api/v2/tag/merge", json!({"source": "blue", "target": "blue"})), "tag.merge: source and target must be different");
    assert_eq!(e.post("/api/v2/tag/merge", json!({"source": "blue", "target": "nope"})).1, json!({"status": "success"}));
    let m = e.ok_post("/api/v2/tag/merge", json!({"source": "crimson", "target": "blue"}));
    assert_eq!(m, json!({"affectedItems": 1, "sourceRemoved": true}));
    assert_eq!(e.reopen().item(&e.a).unwrap().tags, ["blue"]);
}

#[test]
fn smart_folders_round_trip() {
    let e = env();
    let rules = e.ok_get("/api/v2/smartFolder/getRules");
    assert_eq!(rules["tags"]["category"], "SET");

    let cond = json!([{"match": "AND", "boolean": "TRUE", "rules": [{"property": "tags", "method": "union", "value": ["red"]}]}]);
    assert_eq!(e.thrown("/api/v2/smartFolder/create", json!({"conditions": cond})), "name is required");
    assert_eq!(e.thrown("/api/v2/smartFolder/create", json!({"name": "R"})), "conditions is required");
    assert_eq!(e.thrown("/api/v2/smartFolder/create", json!({"name": "R", "conditions": []})), "conditions must be a non-empty array");
    assert_eq!(e.thrown("/api/v2/smartFolder/create", json!({"name": "R", "conditions": cond, "parent": "P"})), "parent smart folder not found: P");

    let sf = e.ok_post("/api/v2/smartFolder/create", json!({"name": "Red", "conditions": cond, "iconColor": "red"}));
    let keys: Vec<&String> = sf.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["id", "name", "conditions", "description", "icon", "iconColor", "children", "modificationTime", "imageCount"]);
    assert_eq!(sf["imageCount"], 1);
    let id = sf["id"].as_str().unwrap().to_string();
    let any = json!([{"rules": [{"property": "tags", "method": "not-empty"}]}]);
    let child = e.ok_post("/api/v2/smartFolder/create", json!({"name": "Child", "conditions": any, "parent": id}));
    let cid = child["id"].as_str().unwrap().to_string();

    let tree = e.ok_get("/api/v2/smartFolder/get");
    assert_eq!(tree["total"], 1);
    assert_eq!(tree["data"][0]["children"][0]["id"], cid.as_str());
    assert_eq!(tree["data"][0]["children"][0]["imageCount"], 1, "a child narrows its parent");
    let flat = e.ok_post("/api/v2/smartFolder/get", json!({"ids": [cid]}));
    assert_eq!(flat["data"][0]["id"], cid.as_str());
    assert_eq!(e.ok_get(&format!("/api/v2/smartFolder/get?id={id}"))["total"], 1);

    let items = e.ok_get(&format!("/api/v2/smartFolder/getItems?smartFolderId={id}&fields=name,id"));
    assert_eq!(items["data"][0]["id"], e.a.as_str());
    let keys: Vec<&String> = items["data"][0].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["name", "id"]);
    assert_eq!(e.get("/api/v2/smartFolder/getItems").1["message"], "smartFolderId is required");
    assert_eq!(e.get("/api/v2/smartFolder/getItems?smartFolderId=X").1["message"], "smart folder not found: X");

    // item/get narrows to smart folders.
    let got = e.ok_post("/api/v2/item/get", json!({"smartFolders": [id]}));
    assert_eq!(got["total"], 1);
    assert_eq!(e.thrown("/api/v2/item/get", json!({"smartFolders": ["X"]})), "smart folder not found: X");

    let blue = json!([{"rules": [{"property": "tags", "method": "union", "value": ["blue"]}]}]);
    let u = e.ok_post("/api/v2/smartFolder/update", json!({"id": id, "name": "Blue", "conditions": blue, "description": "all blue"}));
    assert_eq!((u["name"].clone(), u["description"].clone(), u["imageCount"].clone()), (json!("Blue"), json!("all blue"), json!(2)));
    assert_eq!(u["iconColor"], "red", "colour is kept when not given");
    assert_eq!(e.thrown("/api/v2/smartFolder/update", json!({"id": id, "name": ""})), "name cannot be empty");
    assert_eq!(e.thrown("/api/v2/smartFolder/update", json!({"id": "X"})), "smart folder not found: X");
    assert_eq!(e.thrown("/api/v2/smartFolder/update", json!({})), "id is required");

    let lib = e.reopen();
    let saved = lib.smart_folders();
    assert_eq!((saved[0].name.as_str(), saved[0].description.as_str(), saved[0].icon_color.as_deref()), ("Blue", "all blue", Some("red")));
    assert_eq!(saved[0].children[0].name, "Child");

    assert_eq!(e.ok_post("/api/v2/smartFolder/remove", json!({"id": id})), json!(true));
    assert_eq!(e.thrown("/api/v2/smartFolder/remove", json!({"id": id})), format!("smart folder not found: {id}"));
    assert!(e.reopen().metadata.smart_folders.is_empty());
}

#[test]
fn item_and_folder_edits() {
    let e = env();
    assert_eq!(e.ok_post("/api/v2/item/update", json!({"id": "NOPE"})), json!(false));
    let i = e.ok_post("/api/v2/item/update", json!({"id": e.b, "name": "renamed", "tags": ["x"], "star": 0, "annotation": "n"}));
    assert_eq!((i["name"].clone(), i["tags"].clone(), i["star"].clone()), (json!("renamed"), json!(["x"]), json!(0)));
    let lib = e.reopen();
    let item = lib.item(&e.b).unwrap();
    assert!(item.original_path(lib.root()).is_file(), "the file follows the rename");
    assert_eq!(item.annotation, "n");
    let long = "x".repeat(10001);
    assert_eq!(e.thrown("/api/v2/item/update", json!({"id": e.b, "name": long})), "name too long (max 10000)");

    // item/add: single and batch.
    let p = e.tmp.path().join("c.png");
    png(&p, 10, 10);
    let added = e.ok_post("/api/v2/item/add", json!({"path": p, "name": "cee", "folders": [e.folder], "tags": ["t"]}));
    let new_id = added["id"].as_str().unwrap().to_string();
    let lib = e.reopen();
    let item = lib.item(&new_id).unwrap();
    assert_eq!((item.name.as_str(), item.folders.clone(), item.tags.clone()), ("cee", vec![e.folder.clone()], vec!["t".to_string()]));
    let batch = e.ok_post("/api/v2/item/add", json!({"items": [{"path": p}, {"path": "/no/such/file.png"}]}));
    assert_eq!(batch["ids"].as_array().unwrap().len(), 1);
    assert_eq!(e.thrown("/api/v2/item/add", json!({"name": "x"})), "item.add requires at least one of: url, path, base64, bookmarkURL");

    // folder/create and folder/update.
    assert_eq!(e.ok_post("/api/v2/folder/create", json!({})), json!(false));
    let f = e.ok_post("/api/v2/folder/create", json!({"name": "New", "parent": e.folder, "iconColor": "green", "description": "d"}));
    let fid = f["id"].as_str().unwrap().to_string();
    assert_eq!((f["iconColor"].clone(), f["description"].clone()), (json!("green"), json!("d")));
    assert_eq!(e.post("/api/v2/folder/update", json!({"id": "NOPE"})).1, json!({"status": "success"}));
    // Eagle's quirk: no `parent` moves a subfolder to the root, and no
    // `iconColor` clears the colour.
    let f = e.ok_post("/api/v2/folder/update", json!({"id": fid, "name": "Moved", "tags": ["k"]}));
    assert_eq!((f["name"].clone(), f["tags"].clone(), f["iconColor"].clone()), (json!("Moved"), json!(["k"]), json!("")));
    let lib = e.reopen();
    assert!(lib.metadata.folders.iter().any(|x| x.id == fid), "moved to the root");
    // A move into its own subtree is skipped.
    e.ok_post("/api/v2/folder/update", json!({"id": e.folder, "parent": e.sub}));
    assert!(e.reopen().metadata.folders.iter().any(|x| x.id == e.folder));
    e.ok_post("/api/v2/folder/update", json!({"id": fid, "parent": e.folder}));
    let lib = e.reopen();
    assert!(lib.metadata.folders.iter().find(|x| x.id == e.folder).unwrap().children.iter().any(|x| x.id == fid));

    // v2 folder/get with no filter is the top-level tree.
    let tree = e.ok_get("/api/v2/folder/get");
    assert_eq!(tree["total"], 1);
}

#[test]
fn library_switch_and_history() {
    let e = env();
    let info = e.ok_get("/api/v2/library/info");
    assert_eq!(info["name"], "Test");
    assert_eq!(info["path"], e.root.display().to_string());
    let hist = e.ok_get("/api/v2/library/history");
    assert_eq!(hist["data"], json!([e.root.display().to_string()]));

    // Refusals leave the current library and create nothing.
    let not_lib = e.tmp.path().join("Plain");
    std::fs::create_dir(&not_lib).unwrap();
    assert_eq!(e.thrown("/api/v2/library/switch", json!({"libraryPath": e.tmp.path().join("Gone.library")})), "Library does not exist.");
    assert_eq!(e.thrown("/api/v2/library/switch", json!({"libraryPath": not_lib})), "Not an Eagle library.");
    assert_eq!(e.thrown("/api/v2/library/switch", json!({})), "Library does not exist.");
    assert_eq!(std::fs::read_dir(&not_lib).unwrap().count(), 0);
    let (code, v) = e.post("/api/library/switch", json!({"libraryPath": "/no/such.library"}));
    assert_eq!((code, v), (500, json!({"status": "error", "data": "Library does not exist."})));

    let other = e.tmp.path().join("Other.library");
    drop(Library::create(&other).unwrap());
    let before: Vec<_> = walk(&other);
    assert_eq!(e.ok_post("/api/v2/library/switch", json!({"libraryPath": other})), json!(true));
    assert_eq!(walk(&other), before, "switching writes nothing");
    assert_eq!(e.ok_get("/api/v2/library/info")["name"], "Other");
    assert_eq!(e.ok_get("/api/v2/item/countAll"), json!(0));
    assert_eq!(e.lib.lock().unwrap().root(), other.as_path(), "the shared slot holds the new library");
    assert_eq!(e.ok_get("/api/library/history"), json!([other.display().to_string(), e.root.display().to_string()]));
    assert_eq!(e.post("/api/library/switch", json!({"libraryPath": e.root})).1, json!({"status": "success"}));
    assert_eq!(e.ok_get("/api/v2/item/countAll"), json!(2));

    // library/icon.
    let (code, v) = e.get(&format!("/api/v2/library/icon?libraryPath={}", e.root.display()));
    assert_eq!((code, v["message"].as_str()), (500, Some("Library icon does not exist.")));
    std::fs::write(other.join("icon.png"), [1u8, 2, 3]).unwrap();
    assert_eq!(e.ok_get(&format!("/api/v2/library/icon?libraryPath={}", other.display())), json!({"type": "Buffer", "data": [1, 2, 3]}));
}

#[test]
fn set_library_keeps_the_port() {
    let e = env();
    let other = e.tmp.path().join("Second.library");
    let second: falcon_server::Shared = Arc::new(Mutex::new(Library::create(&other).unwrap()));
    e.server.set_library(second.clone());
    assert!(Arc::ptr_eq(&e.server.library(), &second));
    assert_eq!(e.ok_get("/api/v2/library/info")["name"], "Second");
    assert_eq!(e.ok_get("/api/v2/item/countAll"), json!(0));
    assert_eq!(e.ok_get("/api/library/history"), json!([other.display().to_string(), e.root.display().to_string()]));
    // A switch over the API now replaces what is inside the new slot.
    assert_eq!(e.ok_post("/api/v2/library/switch", json!({"libraryPath": e.root})), json!(true));
    assert_eq!(second.lock().unwrap().root(), e.root.as_path());
    assert_eq!(e.lib.lock().unwrap().root(), e.root.as_path(), "the first library is untouched");
    e.server.set_library(e.lib.clone());
    assert_eq!(e.ok_get("/api/v2/item/countAll"), json!(2));
}

/// Every file under `dir` with its size and modification time.
fn walk(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let meta = entry.metadata().unwrap();
        if meta.is_dir() {
            out.extend(walk(&entry.path()));
        } else {
            out.push((entry.path(), meta.len(), meta.modified().unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn wrong_method_and_unknown_route() {
    let e = env();
    let (code, v) = e.get("/api/v2/tagGroup/create");
    assert_eq!((code, v["message"].as_str()), (405, Some("method not allowed")));
    let (code, _) = e.get("/api/v2/aiSearch/isReady");
    assert_eq!(code, 404);
    // limit 0 falls back to the default page size.
    assert_eq!(e.ok_get("/api/v2/tag/get?limit=0")["limit"], 50);
}

/// A PNG of noise, which compresses badly and so is slow to decode.
fn noisy_png(path: &Path, side: u32) {
    let mut seed = 0x2545_f491_u32 ^ side;
    RgbImage::from_fn(side, side, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let [r, g, b, _] = seed.to_le_bytes();
        Rgb([r, g, b])
    })
    .save(path)
    .unwrap();
}

#[test]
fn imports_leave_the_library_unlocked_while_staging() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    let e = env();
    let paths: Vec<String> = (0..4)
        .map(|n| {
            let p = e.tmp.path().join(format!("big{n}.png"));
            noisy_png(&p, 600);
            p.display().to_string()
        })
        .collect();

    // Stands in for the app, which locks the library every frame.
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (lib, stop) = (e.lib.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut longest = Duration::ZERO;
            while !stop.load(Ordering::Relaxed) {
                let t = Instant::now();
                drop(lib.lock().unwrap_or_else(|p| p.into_inner()));
                longest = longest.max(t.elapsed());
                std::thread::sleep(Duration::from_millis(2));
            }
            longest
        })
    };
    let t = Instant::now();
    let ids = e.ok_post("/api/item/addFromPaths", json!({"paths": paths}));
    let took = t.elapsed();
    stop.store(true, Ordering::Relaxed);
    let longest = watcher.join().unwrap();

    assert_eq!(ids.as_array().unwrap().len(), 4, "{ids}");
    // Holding the lock across the request would make one wait as long as
    // the whole request; brief commits keep every wait a small part of it.
    assert!(took > Duration::from_millis(50), "staging too fast to tell: {took:?}");
    assert!(longest * 4 < took, "waited {longest:?} for the lock during a {took:?} import");
}

// ---------------------------------------------------------------------------
// Token auth, preferences, links and the add/refresh routes

impl Env {
    /// A raw HTTP/1.1 exchange with an explicit `Host` header. Returns the
    /// status, the header block (lower-cased) and the body text.
    fn raw(&self, method: &str, path: &str, host: Option<&str>, body: Option<&str>) -> (u16, String, String) {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.server.port)).unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\n");
        if let Some(h) = host {
            req.push_str(&format!("Host: {h}\r\n"));
        }
        req.push_str("Connection: close\r\n");
        if let Some(b) = body {
            req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", b.len()));
        } else {
            req.push_str("\r\n");
        }
        s.write_all(req.as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let text = String::from_utf8_lossy(&out).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let code = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (code, head.to_lowercase(), body.to_string())
    }

    fn own_host(&self) -> String {
        format!("127.0.0.1:{}", self.server.port)
    }

    /// Raw GET with the server's own Host.
    fn get_raw(&self, path: &str) -> (u16, String, String) {
        self.raw("GET", path, Some(&self.own_host()), None)
    }

    fn foreign(&self, method: &str, path: &str, body: Option<&str>) -> (u16, Value) {
        let (code, _, text) = self.raw(method, path, Some("example.com"), body);
        (code, serde_json::from_str(&text).unwrap())
    }
}

fn png_data_url(w: u32, h: u32) -> String {
    use base64::Engine;
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x * 7) as u8, (y * 5) as u8, 40]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

fn keys(v: &Value) -> Vec<&str> {
    v.as_object().unwrap().keys().map(String::as_str).collect()
}

#[test]
fn token_auth() {
    let e = env();
    let denied = |v: &Value| {
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], 401);
        assert!(v["message"].is_string());
    };
    let (code, v) = e.foreign("GET", "/api/library/info", None);
    assert_eq!(code, 401);
    denied(&v);

    e.server.set_token("abc");
    let (code, v) = e.foreign("GET", "/api/library/info?token=abc", None);
    assert_eq!((code, v["status"].clone()), (200, json!("success")));
    let (code, v) = e.foreign("GET", "/api/library/info?token=wrong", None);
    assert_eq!(code, 401);
    denied(&v);

    let (code, v) = e.foreign("POST", "/api/folder/create", Some(r#"{"token":"abc","folderName":"ByBody"}"#));
    assert_eq!((code, v["status"].clone(), v["data"]["name"].clone()), (200, json!("success"), json!("ByBody")));
    let (code, v) = e.foreign("POST", "/api/folder/create?token=abc", Some(r#"{"folderName":"ByQuery"}"#));
    assert_eq!((code, v["status"].clone(), v["data"]["name"].clone()), (200, json!("success"), json!("ByQuery")));
    let (code, v) = e.foreign("POST", "/api/folder/create", Some(r#"{"token":"nope","folderName":"X"}"#));
    assert_eq!(code, 401);
    denied(&v);

    // The right Host keeps working with or without a token.
    assert_eq!(e.get("/api/library/info").0, 200);
    assert_eq!(e.get("/api/library/info?token=wrong").0, 200);

    // No token set: an empty one matches nothing.
    e.server.set_token("");
    let (code, v) = e.foreign("GET", "/api/library/info?token=", None);
    assert_eq!(code, 401);
    denied(&v);
    assert_eq!(e.foreign("GET", "/api/library/info", None).0, 401);
    assert_eq!(e.get("/api/library/info").0, 200);
}

#[test]
fn empty_token_turns_token_auth_off() {
    let e = env();
    // Initial state: no token, so only the Host check applies.
    for path in ["/api/library/info?token=", "/api/library/info?token=anything"] {
        assert_eq!(e.foreign("GET", path, None).0, 401, "{path}");
    }
    let (code, _) = e.foreign("POST", "/api/folder/create", Some(r#"{"token":"","folderName":"X"}"#));
    assert_eq!(code, 401);
    assert_eq!(e.raw("GET", "/api/library/info", None, None).0, 401);
    assert_eq!(e.get("/api/library/info").0, 200);

    e.server.set_token("abc");
    assert_eq!(e.foreign("GET", "/api/library/info?token=abc", None).0, 200);

    e.server.set_token("");
    assert_eq!(e.foreign("GET", "/api/library/info?token=abc", None).0, 401);
    assert_eq!(e.raw("GET", "/api/library/info", None, None).0, 401);
    assert_eq!(e.get("/api/library/info").0, 200);
}

#[test]
fn application_info_and_preferences() {
    let e = env();
    for path in ["/api/application/info", "/"] {
        let d = e.ok_get(path);
        assert_eq!(keys(&d), ["version", "prereleaseVersion", "buildVersion", "showCollectModal", "platform", "preferences"], "{path}");
        assert_eq!(d["showCollectModal"], false);
        assert_eq!(d["preferences"], json!({"general": {"showCollectModal": "false"}, "developer": {"apiToken": ""}}));
    }
    e.server.set_preferences(json!({"general": {"showCollectModal": "true", "x": "1"}}));
    let d = e.ok_get("/api/application/info");
    assert_eq!(d["showCollectModal"], true);
    assert_eq!(d["preferences"], json!({"general": {"showCollectModal": "true", "x": "1"}}));
    assert_eq!(keys(&e.ok_get("/api/v2/app/info")), ["version", "prereleaseVersion", "buildVersion", "platform"]);
}

#[test]
fn collect_modal_preferences() {
    let e = env();
    assert_eq!(e.server.take_preferences(), None);
    let (code, v) = e.get("/api/preferences/collect/on");
    assert_eq!((code, v), (200, json!({"status": "success"})));
    let d = e.ok_get("/api/application/info");
    assert_eq!(d["showCollectModal"], true);
    assert_eq!(d["preferences"]["general"]["showCollectModal"], "true");
    let prefs = e.server.take_preferences().expect("changed");
    assert_eq!(prefs["general"]["showCollectModal"], "true");
    assert_eq!(e.server.take_preferences(), None);

    let (code, v) = e.get("/api/preferences/collect/off");
    assert_eq!((code, v), (200, json!({"status": "success"})));
    let d = e.ok_get("/api/application/info");
    assert_eq!(d["showCollectModal"], false);
    assert_eq!(d["preferences"]["general"]["showCollectModal"], "false");
    assert!(e.server.take_preferences().is_some());
    assert_eq!(e.server.take_preferences(), None);

    assert_eq!(e.post("/api/preferences/collect/on", json!({})).0, 405);

    e.server.set_preferences(json!({}));
    for p in ["on", "off"] {
        let (code, v) = e.get(&format!("/api/preferences/collect/{p}"));
        assert_eq!((code, v), (500, json!({"status": "error", "data": "preferences file not exist."})));
    }
    assert_eq!(e.server.take_preferences(), None);
}

#[test]
fn library_history_is_set_by_the_app() {
    let e = env();
    let (a, b) = ("/libs/A.library".to_string(), "/libs/B.library".to_string());
    e.server.set_history(vec![a.clone(), b.clone(), a.clone()]);
    assert_eq!(e.ok_get("/api/library/history"), json!([a, b]));
    let d = e.ok_get("/api/v2/library/history");
    assert_eq!(d["data"], json!([a, b]));
    assert_eq!(d["total"], 2);
    assert_eq!(keys(&d), ["data", "total", "offset", "limit"]);
    let d = e.ok_get("/api/v2/library/history?limit=1&offset=1");
    assert_eq!((d["data"].clone(), d["total"].clone(), d["offset"].clone(), d["limit"].clone()), (json!([b]), json!(2), json!(1), json!(1)));
}

#[test]
fn folder_unlock() {
    let e = env();
    e.lib.lock().unwrap().set_folder_password(&e.folder, Some(("pw", "hint"))).unwrap();
    let find = |list: &Value| list.as_array().unwrap().iter().find(|f| f["id"] == e.folder.as_str()).cloned().unwrap();
    let before = find(&e.ok_get("/api/folder/list"));
    assert_eq!(before["isUnLock"], false);
    assert_eq!(before["children"], json!([]));

    let fail = |body: Value| {
        let (code, v) = e.post("/api/folder/unlock", body);
        assert_eq!(code, 500, "{v}");
        assert_eq!(v["status"], "error");
        v["data"].as_str().unwrap().to_string()
    };
    assert_eq!(fail(json!({"folderId": "NOPE", "password": "pw"})), "Folder does not exist.");
    assert_eq!(fail(json!({"folderId": e.folder, "password": "bad"})), "Password is incorrect.");
    assert_eq!(fail(json!({"folderId": e.folder})), "Password is incorrect.");
    assert_eq!(find(&e.ok_get("/api/folder/list"))["isUnLock"], false);

    let (code, v) = e.post("/api/folder/unlock", json!({"folderId": e.folder, "password": "pw"}));
    assert_eq!((code, v), (200, json!({"status": "success"})));
    let after = find(&e.ok_get("/api/folder/list"));
    assert_eq!(after["isUnLock"], true);
    assert_eq!(after["children"].as_array().unwrap().len(), 1);
    assert_eq!(after["children"][0]["id"], e.sub.as_str());

    // A folder without a password opens with an empty one.
    let (code, v) = e.post("/api/folder/unlock", json!({"folderId": e.sub, "password": ""}));
    assert_eq!((code, v), (200, json!({"status": "success"})));

    // Nothing was written: the password is still set on disk.
    assert!(e.reopen().all_folders().into_iter().find(|f| f.id == e.folder).unwrap().is_locked());
}

#[test]
fn refresh_palette() {
    let e = env();
    assert!(e.lib.lock().unwrap().item(&e.a).unwrap().palettes.is_empty());
    let (code, v) = e.post("/api/item/refreshPalette", json!({"id": "NOPE"}));
    assert_eq!((code, v), (500, json!({"status": "error", "data": "File does not exist."})));
    let (code, v) = e.post("/api/item/refreshPalette", json!({"id": e.a}));
    assert_eq!((code, v), (200, json!({"status": "success"})));
    assert!(!e.lib.lock().unwrap().item(&e.a).unwrap().palettes.is_empty());
    assert!(!e.reopen().item(&e.a).unwrap().palettes.is_empty());
}

#[test]
fn refresh_thumbnail() {
    let e = env();
    let (code, v) = e.post("/api/item/refreshThumbnail", json!({"id": "NOPE"}));
    assert_eq!((code, v), (500, json!({"status": "error", "data": "File does not exist."})));
    let (code, v) = e.post("/api/item/refreshThumbnail", json!({"id": e.a}));
    assert_eq!((code, v), (200, json!({"status": "success"})));
    let (code, v) = e.post("/api/v2/item/refreshThumbnail", json!({"itemId": e.a}));
    assert_eq!((code, v), (200, json!({"status": "success", "data": true})));
    let (code, v) = e.post("/api/v2/item/refreshThumbnail", json!({"itemId": "NOPE"}));
    assert_eq!((code, v), (200, json!({"status": "success", "data": false})));
}

#[test]
fn set_custom_thumbnail() {
    let e = env();
    // PNG items are their own thumbnail and refuse a custom one, so use GIFs.
    let gif = |name: &str| {
        let p = e.tmp.path().join(format!("{name}.png"));
        png(&p, 30, 30);
        let img = image::open(&p).unwrap();
        let g = e.tmp.path().join(format!("{name}.gif"));
        let img = img.to_rgba8();
        img.save(&g).unwrap();
        e.ok_post("/api/item/addFromPath", json!({"path": g.display().to_string(), "name": name}));
        let lib = e.lib.lock().unwrap();
        lib.items().iter().find(|i| i.name == name).unwrap().id.clone()
    };
    let (ga, gb) = (gif("ga"), gif("gb"));
    let file = e.tmp.path().join("custom.png");
    png(&file, 64, 48);
    let file = file.display().to_string();

    let (code, v) = e.post("/api/item/setCustomThumbnail", json!({"id": "NOPE", "thumbnailPath": file}));
    assert_eq!((code, v), (500, json!({"status": "error", "data": "File does not exist."})));
    let missing = e.tmp.path().join("missing.png").display().to_string();
    let (code, v) = e.post("/api/item/setCustomThumbnail", json!({"id": ga, "thumbnailPath": missing}));
    assert_eq!((code, v), (500, json!({"status": "error", "data": "Failed to set custom thumbnail."})));

    let (code, v) = e.post("/api/item/setCustomThumbnail", json!({"id": ga, "thumbnailPath": file}));
    assert_eq!((code, v), (200, json!({"status": "success", "data": true})));
    let info = e.ok_get(&format!("/api/item/info?id={}", ga));
    assert_eq!(info["customThumbnail"], true);
    assert_eq!((info["width"].clone(), info["height"].clone()), (json!(64), json!(48)));

    // v2 never errors.
    // A PNG item refuses one, and v2 answers false.
    let (code, v) = e.post("/api/v2/item/setCustomThumbnail", json!({"itemId": e.a, "filePath": file}));
    assert_eq!((code, v), (200, json!({"status": "success", "data": false})));
    let (code, v) = e.post("/api/v2/item/setCustomThumbnail", json!({"itemId": gb, "filePath": file}));
    assert_eq!((code, v), (200, json!({"status": "success", "data": true})));
    assert_eq!(e.ok_get(&format!("/api/item/info?id={}", gb))["customThumbnail"], true);
    for body in [json!({"itemId": "NOPE", "filePath": file}), json!({"itemId": ga, "filePath": missing})] {
        let (code, v) = e.post("/api/v2/item/setCustomThumbnail", body);
        assert_eq!((code, v), (200, json!({"status": "success", "data": false})));
    }
}

#[test]
fn eagle_link_redirects() {
    let e = env();
    let cond = json!([{"match": "AND", "boolean": "TRUE", "rules": [{"property": "tags", "method": "union", "value": ["red"]}]}]);
    let sid = e.ok_post("/api/v2/smartFolder/create", json!({"name": "S", "conditions": cond}))["id"].as_str().unwrap().to_string();

    for (path, id, link) in [("/item", &e.a, "item"), ("/folder", &e.folder, "folder"), ("/smart-folder", &sid, "smart-folder")] {
        let (code, head, body) = e.get_raw(&format!("{path}?id={id}"));
        assert_eq!(code, 302, "{path}");
        assert!(head.contains(&format!("location: eagle://{link}/{}", id.to_lowercase())), "{head}");
        assert_eq!(body, "");
    }
    for (path, msg) in [("/item", "File does not exist."), ("/folder", "Folder does not exist."), ("/smart-folder", "Smart Folder does not exist.")] {
        for q in ["?id=NOPE", ""] {
            let (code, _, body) = e.get_raw(&format!("{path}{q}"));
            assert_eq!(code, 404, "{path}{q}");
            assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"error": msg}));
        }
        let (code, _, body) = e.raw("GET", &format!("{path}?id=x"), Some("example.com"), None);
        assert_eq!(code, 401);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["code"], 401);
    }
}

#[test]
fn playground_page() {
    let e = env();
    let (code, head, body) = e.get_raw("/api/v2/playground");
    assert_eq!(code, 200);
    assert!(head.contains("content-type: text/html"), "{head}");
    assert!(!body.is_empty());
}

#[test]
fn add_from_url_applies_folder_auto_tags() {
    let e = env();
    e.lib.lock().unwrap().set_folder_auto_tags(&e.folder, &["auto".to_string()]).unwrap();
    let url = png_data_url(16, 16);
    let tags_of = |name: &str| -> Vec<String> {
        let lib = e.lib.lock().unwrap();
        let hits: Vec<_> = lib.items().iter().filter(|i| i.name == name).collect();
        assert_eq!(hits.len(), 1, "{name}");
        hits[0].tags.clone()
    };

    let (code, v) = e.post("/api/item/addFromURL", json!({"url": url, "folderId": e.folder, "tags": ["mine", "auto"], "name": "u1"}));
    assert_eq!((code, v), (200, json!({"status": "success"})));
    assert_eq!(tags_of("u1"), ["auto", "mine"]);
    {
        let lib = e.lib.lock().unwrap();
        assert_eq!(&lib.tags.history_tags[..2], ["mine", "auto"]);
    }
    let history = e.lib.lock().unwrap().tags.history_tags.clone();

    let p = e.tmp.path().join("p.png");
    png(&p, 10, 10);
    e.ok_post("/api/item/addFromPath", json!({"path": p.display().to_string(), "name": "p1", "folderId": e.folder, "tags": ["p"]}));
    assert_eq!(tags_of("p1"), ["p"]);
    assert_eq!(e.lib.lock().unwrap().tags.history_tags, history);

    e.ok_post("/api/item/addFromURLs", json!({"folderId": e.folder, "items": [{"url": url, "name": "u2", "tags": ["t2"]}]}));
    assert_eq!(tags_of("u2"), ["auto", "t2"]);
}
