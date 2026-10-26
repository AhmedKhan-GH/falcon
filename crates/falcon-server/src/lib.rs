//! Eagle's local HTTP API, served from a falcon
//! library.
//!
//! Responses use Eagle's envelope: `{"status":"success","data":...}`, string
//! errors as HTTP 500 `{"status":"error","data":"..."}`, unknown paths as 404
//! and wrong verbs as 405 (both with Eagle's "method not allowed" text).
//!
//! Falcon's own choices:
//! - it listens on 127.0.0.1 only;
//! - it serves library routes only;
//! - application info reports version and platform only;
//! - imports finish before the reply, so returned ids always exist.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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

/// Serves the API until the process exits.
pub fn serve(lib: Shared, port: u16) -> std::io::Result<()> {
    let server = Server::http(("127.0.0.1", port)).map_err(std::io::Error::other)?;
    for request in server.incoming_requests() {
        handle(&lib, port, request);
    }
    Ok(())
}

/// A handler outcome before it is wrapped in Eagle's envelope.
enum Reply {
    Data(Value),
    Empty,
    /// A string rejection: HTTP 500 with the message in `data`.
    Fail(String),
    BadRequest,
    NotFound,
    WrongMethod,
    Unauthorized,
}

fn handle(lib: &Shared, port: u16, mut req: Request) {
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

    let host = req.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str().to_string());
    let allowed_hosts = [format!("localhost:{port}"), format!("127.0.0.1:{port}"), format!("0.0.0.0:{port}")];
    if !host.as_ref().is_some_and(|h| allowed_hosts.contains(h)) {
        return send(req, Reply::Unauthorized);
    }

    let reply = route(lib, &method, &path, &args);
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
    let mut v = serde_json::to_value(f).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("children".into(), Value::Array(f.children.iter().map(folder_json).collect()));
        for (k, d) in [("extendTags", json!([])), ("icon", json!("")), ("iconColor", json!("")), ("coverId", json!(""))] {
            m.entry(k).or_insert(d);
        }
        m.insert("isUnLock".into(), json!(f.password.is_empty()));
        if !f.password.is_empty() {
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

fn library_info(lib: &Library) -> Value {
    let mut v = serde_json::to_value(&lib.metadata).unwrap_or(json!({}));
    let name = lib.root().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if let Value::Object(m) = &mut v {
        m.insert("library".into(), json!({"path": lib.root().display().to_string(), "name": name}));
    }
    v
}

fn paginate(items: Vec<Value>, args: &Map<String, Value>) -> Value {
    let offset = number(args, "offset").unwrap_or(0) as usize;
    let limit = (number(args, "limit").unwrap_or(50) as usize).clamp(1, 1000);
    let total = items.len();
    let page: Vec<Value> = items.into_iter().skip(offset).take(limit).collect();
    json!({"data": page, "total": total, "offset": offset, "limit": limit})
}

// ---------------------------------------------------------------------------
// Routing

fn route(lib: &Shared, method: &Method, path: &str, args: &Map<String, Value>) -> Reply {
    use Method::{Get, Post};
    let routes: &[(&str, &[Method])] = &[
        ("/", &[Get]),
        ("/api/application/info", &[Get]),
        ("/api/library/info", &[Get]),
        ("/api/library/history", &[Get]),
        ("/api/folder/list", &[Get]),
        ("/api/folder/listRecent", &[Get]),
        ("/api/folder/create", &[Post]),
        ("/api/folder/rename", &[Post]),
        ("/api/folder/update", &[Post]),
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
        ("/api/v2/item/get", &[Get, Post]),
        ("/api/v2/item/query", &[Post]),
        ("/api/v2/item/countAll", &[Get]),
        ("/api/v2/folder/get", &[Get, Post]),
        ("/api/v2/tag/get", &[Get, Post]),
    ];
    let Some((_, methods)) = routes.iter().find(|(p, _)| *p == path) else {
        return Reply::NotFound;
    };
    if !methods.contains(method) {
        return Reply::WrongMethod;
    }
    let mut lib = match lib.lock() {
        Ok(l) => l,
        Err(p) => p.into_inner(),
    };
    let lib = &mut *lib;

    match path {
        "/" | "/api/application/info" => Reply::Data(json!({
            "version": API_VERSION,
            "prereleaseVersion": null,
            "buildVersion": concat!("falcon-", env!("CARGO_PKG_VERSION")),
            "platform": std::env::consts::OS,
        })),
        "/api/v2/app/info" => Reply::Data(json!({
            "version": API_VERSION,
            "prereleaseVersion": null,
            "buildVersion": concat!("falcon-", env!("CARGO_PKG_VERSION")),
            "platform": std::env::consts::OS,
        })),
        "/api/library/info" | "/api/v2/library/info" => Reply::Data(library_info(lib)),
        "/api/library/history" => Reply::Data(json!([lib.root().display().to_string()])),

        "/api/folder/list" => Reply::Data(Value::Array(lib.metadata.folders.iter().map(folder_json).collect())),
        "/api/folder/listRecent" => {
            Reply::Data(Value::Array(lib.all_folders().into_iter().take(16).map(folder_json).collect()))
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
        "/api/tag/groups" => Reply::Data(Value::Array(lib.metadata.tags_groups.clone())),
        "/api/v2/tag/get" => Reply::Data(paginate(tag_objects(lib), args)),

        "/api/item/addFromPath" => {
            let Some(p) = s(args, "path") else { return Reply::Fail("Missing required parameters.".into()) };
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
                _ => return Reply::BadRequest,
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
            let Some(url) = s(args, "url") else { return Reply::Fail("Missing required parameters.".into()) };
            match add_url(lib, url, args, args) {
                Ok(_) => Reply::Empty,
                Err(e) => Reply::Fail(e),
            }
        }
        "/api/item/addFromURLs" => {
            let Some(Value::Array(items)) = args.get("items") else { return Reply::Fail("Missing required parameters.".into()) };
            for e in items.iter().filter_map(Value::as_object) {
                if let Some(url) = s(e, "url") {
                    let _ = add_url(lib, url, e, args);
                }
            }
            Reply::Empty
        }
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

        "/api/v2/item/get" => {
            let items = v2_get(lib, args);
            Reply::Data(paginate(project(items, args), args))
        }
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
            let folders: Vec<Value> = lib
                .all_folders()
                .into_iter()
                .filter(|f| (ids.is_empty() && one.is_none()) || ids.contains(&f.id) || one == Some(f.id.as_str()))
                .map(folder_json)
                .collect();
            Reply::Data(paginate(folders, args))
        }
        _ => Reply::NotFound,
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
    let path = match item.thumbnail_path(lib.root()) {
        Some(t) => t,
        None => item.original_path(lib.root()),
    };
    path.display().to_string().replace(' ', "%20").replace('#', "%23")
}

/// Imports a file with the metadata Eagle's add routes accept.
fn add_path(lib: &mut Library, path: PathBuf, item: &Map<String, Value>, outer: &Map<String, Value>) -> Result<String, String> {
    let mut folders = folder_ids(lib, item);
    if folders.is_empty() {
        folders = folder_ids(lib, outer);
    }
    let opts = ImportOptions {
        folders,
        tags: strings(item, "tags"),
        palette: true,
        annotation: s(item, "annotation").unwrap_or("").to_string(),
        url: s(item, "website").unwrap_or("").to_string(),
    };
    let id = lib.import_file(&path, &opts).map_err(|e| e.to_string())?;
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

/// Downloads an http(s) or data URL to a temporary file and imports it.
fn add_url(lib: &mut Library, url: &str, item: &Map<String, Value>, outer: &Map<String, Value>) -> Result<String, String> {
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

    let order = s(args, "orderBy").unwrap_or("CREATEDATE");
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
    let limit = number(args, "limit").map_or(200, |l| l as usize);
    let page = number(args, "offset").unwrap_or(0) as usize;
    let mut page: Vec<&Item> = items.into_iter().skip(limit * page).take(limit).collect();
    page.sort_by_key(|i| std::cmp::Reverse(i.modification_time));
    page.into_iter().map(item_json).collect()
}

/// `/api/v2/item/get` filters, AND-ed; trash excluded; newest first.
fn v2_get(lib: &Library, args: &Map<String, Value>) -> Vec<Value> {
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
