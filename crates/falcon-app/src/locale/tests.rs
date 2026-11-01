//! The catalogue checks: `en.json` lists exactly the keys the code uses,
//! and every translation file covers them without losing a placeholder.
//! `FALCON_UPDATE_LOCALES=1 cargo test -p falcon-app locale` rewrites
//! `en.json` from the code and drops keys the code no longer uses from
//! the other files (sorting them), then reports what is still missing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;

fn locales_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("locales")
}

fn updating() -> bool {
    std::env::var_os("FALCON_UPDATE_LOCALES").is_some()
}

/// A key as the code gives it: its text, or a count's two forms.
#[derive(Debug, Clone, PartialEq)]
enum Found {
    Text,
    Plural { one: String },
}

/// Reads the next Rust string literal at the start of `s` (after
/// whitespace and comments), returning its value and the rest.
fn literal(s: &str) -> Option<(String, &str)> {
    let s = skip_space(s);
    let (raw, body) = if let Some(r) = s.strip_prefix('r') {
        let hashes = r.len() - r.trim_start_matches('#').len();
        (Some(hashes), r[hashes..].strip_prefix('"')?)
    } else {
        (None, s.strip_prefix('"')?)
    };
    if let Some(h) = raw {
        let end = format!("\"{}", "#".repeat(h));
        let i = body.find(&end)?;
        return Some((body[..i].to_string(), &body[i + end.len()..]));
    }
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &body[i + 1..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    let rest = &body[i + 2..];
                    let close = rest.find('}')?;
                    let code = u32::from_str_radix(&rest[1..close], 16).ok()?;
                    out.push(char::from_u32(code)?);
                    for _ in 0..=close {
                        chars.next();
                    }
                }
                '\n' => {
                    // A line continuation drops the newline and the next
                    // line's leading whitespace.
                    while chars.clone().next().is_some_and(|(_, c)| c.is_whitespace()) {
                        chars.next();
                    }
                }
                'x' => {
                    let hex: String = (0..2).filter_map(|_| chars.next().map(|(_, c)| c)).collect();
                    out.push(char::from(u8::from_str_radix(&hex, 16).ok()?));
                }
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            c => out.push(c),
        }
    }
    None
}

fn skip_space(mut s: &str) -> &str {
    loop {
        let t = s.trim_start();
        if let Some(r) = t.strip_prefix("//") {
            s = r.find('\n').map_or("", |i| &r[i..]);
        } else if let Some(r) = t.strip_prefix("/*") {
            s = r.find("*/").map_or("", |i| &r[i + 2..]);
        } else {
            return t;
        }
    }
}

/// Skips one argument: everything up to the next comma outside brackets.
fn skip_arg(s: &str) -> Option<&str> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => return Some(&s[i + 1..]),
            '"' => return None,
            _ => {}
        }
        if depth < 0 {
            return None;
        }
    }
    None
}

/// The keys used in one source file: each `tr`, `trf` and `tr_key` whose
/// first argument is a literal, and each `tr_n` and `tr_nf` with literal
/// forms. Code inside string literals and comments is skipped.
fn keys_in(src: &str, file: &str, out: &mut BTreeMap<String, Found>) {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut prev = b' ';
    // The word before, so `fn tr(` (a definition) is not a call.
    let mut last_word = "";
    while i < bytes.len() {
        let rest = &src[i..];
        let c = bytes[i];
        if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("/*") {
            i += rest.find("*/").map_or(rest.len(), |e| e + 2);
            continue;
        }
        let ident_before = prev.is_ascii_alphanumeric() || prev == b'_';
        let raw = c == b'r' && !ident_before && (rest.starts_with("r\"") || rest.starts_with("r#"));
        if (c == b'"' || raw)
            && let Some((_, after)) = literal(rest)
        {
            i = src.len() - after.len();
            prev = b'"';
            continue;
        }
        if c == b'\'' {
            // A char literal ('x', '\n', '"'), else a lifetime.
            let r = &rest[1..];
            let mut it = r.chars();
            let first = it.next();
            let skip = match first {
                Some('\\') => r.find('\'').map(|e| e + 2),
                Some(ch) if it.next() == Some('\'') => Some(1 + ch.len_utf8() + 1),
                _ => None,
            };
            if let Some(n) = skip {
                i += n;
                prev = b'\'';
                continue;
            }
        }
        if !ident_before && prev != b'.' && (c.is_ascii_alphabetic() || c == b'_') {
            let end = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')).unwrap_or(rest.len());
            let word = &rest[..end];
            let after = skip_space(&rest[end..]);
            if let ("tr" | "trf" | "tr_key" | "tr_n" | "tr_nf", Some(args), false) = (word, after.strip_prefix('('), last_word == "fn") {
                let line = src[..i].lines().count();
                if word.starts_with("tr_n") {
                    let forms = skip_arg(args).and_then(literal).and_then(|(one, r)| Some((one, skip_space(r).strip_prefix(',')?))).and_then(|(one, r)| Some((one, literal(r)?.0)));
                    match forms {
                        Some((one, other)) => add(out, other, Found::Plural { one }, file, line),
                        // The module's own wrappers pass their arguments on.
                        None if file.starts_with("locale") => {}
                        None => panic!("{file}:{line}: {word} needs literal forms"),
                    }
                } else if let Some((key, _)) = literal(args) {
                    add(out, key, Found::Text, file, line);
                }
            }
            i += end;
            prev = b'a';
            last_word = word;
            continue;
        }
        if !c.is_ascii_whitespace() {
            last_word = "";
        }
        prev = c;
        i += 1;
    }
}

fn add(out: &mut BTreeMap<String, Found>, key: String, found: Found, file: &str, line: usize) {
    assert!(!key.trim().is_empty(), "{file}:{line}: an empty key");
    match out.get(&key) {
        Some(old) if *old != found => panic!("{file}:{line}: {key:?} is used both as a text and as a count, or with two singulars"),
        _ => {
            out.insert(key, found);
        }
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// Every key the code uses.
fn code_keys() -> BTreeMap<String, Found> {
    let mut files = Vec::new();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    rust_files(&src, &mut files);
    let mut out = BTreeMap::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let name = f.strip_prefix(&src).unwrap().display().to_string();
        keys_in(&text, &name, &mut out);
    }
    out
}

/// A file's text in the one layout the tests accept: sorted keys, two
/// spaces, a final newline.
fn canonical(map: &BTreeMap<String, Entry>) -> String {
    let value: BTreeMap<&String, serde_json::Value> = map
        .iter()
        .map(|(k, e)| {
            let v = match e {
                Entry::Text(t) => serde_json::Value::String(t.clone()),
                Entry::Plural(forms) => {
                    let mut m = serde_json::Map::new();
                    for (f, t) in forms {
                        m.insert(f.clone(), serde_json::Value::String(t.clone()));
                    }
                    serde_json::Value::Object(m)
                }
            };
            (k, v)
        })
        .collect();
    serde_json::to_string_pretty(&value).unwrap() + "\n"
}

fn english() -> BTreeMap<String, Entry> {
    code_keys()
        .into_iter()
        .map(|(k, f)| {
            let e = match f {
                Found::Text => Entry::Text(k.clone()),
                Found::Plural { one } => Entry::Plural(BTreeMap::from([("one".to_string(), one), ("other".to_string(), k.clone())])),
            };
            (k, e)
        })
        .collect()
}

fn read(lang: Lang) -> (String, BTreeMap<String, Entry>) {
    let path = locales_dir().join(format!("{}.json", lang.code()));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let map = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (text, map)
}

#[test]
fn english_catalogue_lists_the_codes_keys() {
    let en = english();
    let text = canonical(&en);
    let path = locales_dir().join("en.json");
    if updating() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(on_disk == text, "locales/en.json is out of date with the code; run FALCON_UPDATE_LOCALES=1 cargo test -p falcon-app locale");
}

#[test]
fn translations_cover_every_key() {
    let en = english();
    let mut problems = Vec::new();
    for lang in Lang::ALL.into_iter().filter(|l| *l != Lang::En) {
        let (text, mut map) = read(lang);
        let stale: Vec<String> = map.keys().filter(|k| !en.contains_key(*k)).cloned().collect();
        if updating() {
            map.retain(|k, _| en.contains_key(k));
            std::fs::write(locales_dir().join(format!("{}.json", lang.code())), canonical(&map)).unwrap();
        } else {
            if text != canonical(&map) {
                problems.push(format!("{}.json is not sorted and laid out as the tests write it (FALCON_UPDATE_LOCALES=1 rewrites it)", lang.code()));
            }
            for k in &stale {
                problems.push(format!("{}.json: {k:?} is not used by the code", lang.code()));
            }
        }
        let mut missing = BTreeMap::new();
        for (key, english) in &en {
            let Some(entry) = map.get(key) else {
                missing.insert(key.clone(), english.clone());
                continue;
            };
            let want = placeholders(key);
            let check = |form: &str, t: &str, problems: &mut Vec<String>| {
                let mut got = placeholders(t);
                // A form used for exactly one may say "one" in words.
                let exactly_one = form == "one" && matches!(lang, Lang::De | Lang::Es);
                if exactly_one && !got.contains(&"n".to_string()) && want.contains(&"n".to_string()) {
                    got.push("n".into());
                    got.sort();
                }
                if got != want {
                    problems.push(format!("{}.json: {key:?} ({form}) has placeholders {got:?}, the English {want:?}", lang.code()));
                }
                if t.trim().is_empty() {
                    problems.push(format!("{}.json: {key:?} is empty", lang.code()));
                }
            };
            match (english, entry) {
                (Entry::Text(_), Entry::Text(t)) => check("text", t, &mut problems),
                (Entry::Plural(_), Entry::Plural(forms)) => {
                    for f in lang.plural_forms() {
                        match forms.get(*f) {
                            Some(t) => check(f, t, &mut problems),
                            None => problems.push(format!("{}.json: {key:?} lacks the {f:?} form", lang.code())),
                        }
                    }
                    for f in forms.keys().filter(|f| !lang.plural_forms().contains(&f.as_str())) {
                        problems.push(format!("{}.json: {key:?} has a {f:?} form the language doesn't use", lang.code()));
                    }
                }
                (Entry::Plural(_), Entry::Text(t)) if lang.plural_forms() == ["other"] => check("other", t, &mut problems),
                _ => problems.push(format!("{}.json: {key:?} should be {}", lang.code(), if matches!(english, Entry::Plural(_)) { "an object of plural forms" } else { "a text" })),
            }
        }
        if !missing.is_empty() {
            problems.push(format!("{}.json lacks {} keys:\n{}", lang.code(), missing.len(), canonical(&missing)));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_file_parses() {
    for lang in Lang::ALL {
        let parsed: Result<HashMap<String, Entry>, _> = serde_json::from_str(lang.source());
        assert!(parsed.is_ok(), "{}: {:?}", lang.code(), parsed.err());
    }
}

#[test]
fn keys_are_found_in_code() {
    let src = r#"
        let a = tr("Plain");
        let b = trf("Moved to {folder}", &[("folder", &f)]);
        // tr("In a comment")
        let s = "tr(\"in a string\")";
        let c = tr_n(items.len(), "{n} item", "{n} items");
        let d = crate::locale::tr_nf(x.min(3), "One {kind}", "{n} {kind}s", &[("kind", &k)]);
        const L: &str = tr_key("Tabled");
        let e = tr(label);
        let f = '"'; let g = tr("After a quote \
                 char");
        fn h<'a>(x: &'a str) { tr("Lifetime") }
        x.tr("method");
    "#;
    let mut out = BTreeMap::new();
    keys_in(src, "t.rs", &mut out);
    let keys: Vec<&str> = out.keys().map(|k| k.as_str()).collect();
    assert_eq!(keys, ["After a quote char", "Lifetime", "Moved to {folder}", "Plain", "Tabled", "{n} items", "{n} {kind}s"]);
    assert_eq!(out["{n} items"], Found::Plural { one: "{n} item".into() });
}

#[test]
fn fill_replaces_named_placeholders() {
    assert_eq!(fill("Moved {n} items to {folder}", &[("n", &3), ("folder", &"Art")]), "Moved 3 items to Art");
    assert_eq!(fill("{unknown} and {{braces}}", &[]), "{unknown} and {braces}");
    assert_eq!(fill("{a}{a}", &[("a", &"x")]), "xx");
    assert_eq!(fill("no placeholders", &[("a", &1)]), "no placeholders");
    assert_eq!(fill("trailing {", &[]), "trailing {");
    assert_eq!(placeholders("{n} of {total}, {n} again {{not}}"), ["n", "total"]);
}

#[test]
fn plural_rules() {
    assert_eq!(Lang::En.plural(1), "one");
    assert_eq!(Lang::En.plural(0), "other");
    assert_eq!(Lang::De.plural(2), "other");
    let ru: Vec<&str> = [1, 2, 5, 11, 12, 21, 22, 25, 101, 111].into_iter().map(|n| Lang::Ru.plural(n)).collect();
    assert_eq!(ru, ["one", "few", "many", "many", "many", "one", "few", "many", "one", "many"]);
    assert_eq!(Lang::Ja.plural(1), "other");
    for lang in Lang::ALL {
        for n in 0..200 {
            assert!(lang.plural_forms().contains(&lang.plural(n)), "{lang:?} {n}");
        }
    }
}

#[test]
fn system_tags_map_to_languages() {
    assert_eq!(Lang::from_tag("en-US"), Some(Lang::En));
    assert_eq!(Lang::from_tag("de_AT.UTF-8"), Some(Lang::De));
    assert_eq!(Lang::from_tag("zh-Hans-CN"), Some(Lang::ZhCn));
    assert_eq!(Lang::from_tag("zh-CN"), Some(Lang::ZhCn));
    assert_eq!(Lang::from_tag("zh-Hant-TW"), Some(Lang::ZhTw));
    assert_eq!(Lang::from_tag("zh-HK"), Some(Lang::ZhTw));
    assert_eq!(Lang::from_tag("zh"), Some(Lang::ZhCn));
    assert_eq!(Lang::from_tag("ja-JP"), Some(Lang::Ja));
    assert_eq!(Lang::from_tag("pt-BR"), None);
    assert_eq!(Lang::from_tag(""), None);
}

#[test]
fn the_preference_saves_as_a_code() {
    for l in Language::all() {
        assert_eq!(Language::from(String::from(l)), l);
    }
    assert_eq!(String::from(Language::Fixed(Lang::ZhTw)), "zh_TW");
    assert_eq!(Language::from("klingon".to_string()), Language::Auto);
    assert_eq!(Language::default(), Language::Auto);
}

#[test]
fn lookups_fall_back_to_english() {
    // Read the catalogue directly; `set` would change what other tests see.
    assert!(lookup(Lang::En, "Move to Trash").is_none());
    assert!(lookup(Lang::De, "a key nobody uses").is_none());
    for lang in Lang::ALL {
        assert_eq!(Lang::from_code(lang.code()), Some(lang));
    }
}
