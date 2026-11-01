//! Localisation: falcon's interface in eight languages.
//!
//! Strings are keyed by their English text, which stays in the code:
//!
//! - `tr("Move to Trash")` gives the text in the current language,
//! - `trf("Moved to {folder}", &[("folder", &name)])` fills `{name}`
//!   placeholders after translating,
//! - `tr_n(n, "{n} item", "{n} items")` picks the plural form for `n` and
//!   fills `{n}`; `tr_nf` also takes other placeholders,
//! - `tr_key("Folders")` marks a string kept in a `const` or `static`
//!   table, translated later with `tr(label)` where it is shown.
//!
//! Each language has a file in `crates/falcon-app/locales/`, built into
//! the binary: `en.json` lists every key (generated from the code by
//! `FALCON_UPDATE_LOCALES=1 cargo test -p falcon-app locale`), and the
//! other files map keys to translations. A plural key (the English
//! "other" form) maps to an object of the language's plural forms. A key
//! missing from a file shows in English. See `docs/src/localisation.md`.

mod fonts;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

pub use fonts::load_fonts;

/// A language falcon is translated into.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Lang {
    #[default]
    En,
    De,
    Es,
    Ja,
    Ko,
    Ru,
    ZhCn,
    ZhTw,
}

impl Lang {
    /// Every language, in the order the preference lists them.
    pub const ALL: [Lang; 8] = [Lang::En, Lang::ZhCn, Lang::ZhTw, Lang::Ja, Lang::Ko, Lang::De, Lang::Es, Lang::Ru];

    /// The file name (without `.json`) and the saved value.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Es => "es",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Ru => "ru",
            Lang::ZhCn => "zh_CN",
            Lang::ZhTw => "zh_TW",
        }
    }

    /// The language's own name for itself, shown untranslated so anyone
    /// can find theirs.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
            Lang::Ru => "Русский",
            Lang::ZhCn => "简体中文",
            Lang::ZhTw => "繁體中文",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The language for a system locale tag such as `en-US`, `de_AT`,
    /// `zh-Hans-CN`, `zh-Hant` or `zh-HK`, if falcon has it.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let tag = tag.replace('_', "-").to_ascii_lowercase();
        let mut parts = tag.split(['-', '.', '@']);
        let lang = match parts.next()? {
            "en" => Lang::En,
            "de" => Lang::De,
            "es" => Lang::Es,
            "ja" => Lang::Ja,
            "ko" => Lang::Ko,
            "ru" => Lang::Ru,
            "zh" => {
                let rest: Vec<&str> = parts.collect();
                let traditional = rest.iter().any(|p| matches!(*p, "hant" | "tw" | "hk" | "mo"));
                let simplified = rest.contains(&"hans");
                return Some(if traditional && !simplified { Lang::ZhTw } else { Lang::ZhCn });
            }
            _ => return None,
        };
        Some(lang)
    }

    /// The translations, or nothing for English (the keys themselves).
    fn source(self) -> &'static str {
        match self {
            Lang::En => "{}",
            Lang::De => include_str!("../../locales/de.json"),
            Lang::Es => include_str!("../../locales/es.json"),
            Lang::Ja => include_str!("../../locales/ja.json"),
            Lang::Ko => include_str!("../../locales/ko.json"),
            Lang::Ru => include_str!("../../locales/ru.json"),
            Lang::ZhCn => include_str!("../../locales/zh_CN.json"),
            Lang::ZhTw => include_str!("../../locales/zh_TW.json"),
        }
    }

    fn index(self) -> usize {
        Lang::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    /// The plural form `n` takes (CLDR's categories, for whole numbers).
    pub fn plural(self, n: u64) -> &'static str {
        match self {
            Lang::En | Lang::De | Lang::Es => {
                if n == 1 {
                    "one"
                } else {
                    "other"
                }
            }
            Lang::Ru => {
                let (d, h) = (n % 10, n % 100);
                if d == 1 && h != 11 {
                    "one"
                } else if (2..=4).contains(&d) && !(12..=14).contains(&h) {
                    "few"
                } else {
                    "many"
                }
            }
            Lang::Ja | Lang::Ko | Lang::ZhCn | Lang::ZhTw => "other",
        }
    }

    /// The plural forms a translation into this language must give.
    pub fn plural_forms(self) -> &'static [&'static str] {
        match self {
            Lang::En | Lang::De | Lang::Es => &["one", "other"],
            Lang::Ru => &["one", "few", "many"],
            Lang::Ja | Lang::Ko | Lang::ZhCn | Lang::ZhTw => &["other"],
        }
    }
}

/// The language preference: the system's, or a chosen one.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(from = "String", into = "String")]
pub enum Language {
    #[default]
    Auto,
    Fixed(Lang),
}

impl From<String> for Language {
    /// An unknown saved value falls back to Auto.
    fn from(s: String) -> Self {
        Lang::from_code(&s).map_or(Language::Auto, Language::Fixed)
    }
}

impl From<Language> for String {
    fn from(l: Language) -> Self {
        match l {
            Language::Auto => "auto".into(),
            Language::Fixed(l) => l.code().into(),
        }
    }
}

impl Language {
    /// Auto, then every language.
    pub fn all() -> impl Iterator<Item = Language> {
        std::iter::once(Language::Auto).chain(Lang::ALL.into_iter().map(Language::Fixed))
    }

    /// The language to show: the chosen one, else `$FALCON_LANG` or the
    /// first of the system's preferred languages falcon has, else English.
    pub fn resolve(self) -> Lang {
        match self {
            Language::Fixed(l) => l,
            Language::Auto => system_lang(),
        }
    }

    /// How the preference lists it.
    pub fn label(self) -> String {
        match self {
            Language::Auto => trf("Automatic ({language})", &[("language", &system_lang().native_name())]),
            Language::Fixed(l) => l.native_name().to_string(),
        }
    }
}

/// The first of the system's preferred languages that falcon has.
pub fn system_lang() -> Lang {
    std::env::var("FALCON_LANG")
        .ok()
        .and_then(|t| Lang::from_tag(&t))
        .or_else(|| sys_locale::get_locales().find_map(|t| Lang::from_tag(&t)))
        .unwrap_or_default()
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Switches the interface language; text drawn from the next frame on
/// follows it (the menu bar rebuilds itself, see `menu::Bar`).
pub fn set(lang: Lang) {
    CURRENT.store(lang.index() as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    Lang::ALL.get(usize::from(CURRENT.load(Ordering::Relaxed))).copied().unwrap_or_default()
}

/// One translation: a text, or a plural key's forms.
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum Entry {
    Text(String),
    Plural(std::collections::BTreeMap<String, String>),
}

type Catalogue = HashMap<String, Entry>;

/// A language's translations, read once from the built-in file. A file
/// that fails to parse (which the tests rule out) leaves it in English.
fn catalogue(lang: Lang) -> &'static Catalogue {
    static LOADED: [OnceLock<Catalogue>; 8] = [const { OnceLock::new() }; 8];
    LOADED[lang.index()].get_or_init(|| serde_json::from_str(lang.source()).unwrap_or_default())
}

/// The text for `key` in `lang`, if it has one.
fn lookup(lang: Lang, key: &str) -> Option<&'static Entry> {
    if lang == Lang::En {
        return None;
    }
    catalogue(lang).get(key)
}

/// `key` (falcon's English text) in the current language.
pub fn tr(key: &str) -> &str {
    match lookup(current(), key) {
        Some(Entry::Text(t)) => t,
        Some(Entry::Plural(forms)) => forms.get("other").map_or(key, |s| s.as_str()),
        None => key,
    }
}

/// Marks a string in a `const` or `static` table as a key, so the
/// catalogue lists it; it is translated where it is shown, with `tr`.
pub const fn tr_key(key: &'static str) -> &'static str {
    key
}

/// `key` translated, with its `{name}` placeholders filled from `args`.
pub fn trf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    fill(tr(key), args)
}

/// The form of a count for `n`: `one` and `other` are the English forms,
/// and `other` is the key. `{n}` is filled with `n`.
pub fn tr_n(n: usize, one: &str, other: &str) -> String {
    tr_nf(n, one, other, &[])
}

/// `tr_n` with more placeholders filled from `args`.
pub fn tr_nf(n: usize, one: &str, other: &str, args: &[(&str, &dyn Display)]) -> String {
    let lang = current();
    let count = n as u64;
    let text = match lookup(lang, other) {
        Some(Entry::Plural(forms)) => forms.get(lang.plural(count)).or_else(|| forms.get("other")).map_or(other, |s| s.as_str()),
        Some(Entry::Text(t)) => t.as_str(),
        None if Lang::En.plural(count) == "one" => one,
        None => other,
    };
    let mut all: Vec<(&str, &dyn Display)> = vec![("n", &n)];
    all.extend_from_slice(args);
    fill(text, &all)
}

/// Replaces each `{name}` in `template` with its value from `args`.
/// `{{` and `}}` stand for braces; an unknown name is left as it is.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let Some(inner) = tail.strip_prefix('{')
            && let Some(end) = inner.find('}')
            && let Some((_, value)) = args.iter().find(|(name, _)| *name == &inner[..end])
        {
            let _ = write!(out, "{value}");
            rest = &inner[end + 1..];
            continue;
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// The `{name}` placeholders in a text, sorted and without repeats.
#[cfg(test)]
pub fn placeholders(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        let tail = &rest[i + 1..];
        if let Some(t) = tail.strip_prefix('{') {
            rest = t;
            continue;
        }
        match tail.find('}') {
            Some(end) if tail[..end].chars().all(|c| c.is_alphanumeric() || c == '_') && end > 0 => {
                found.push(tail[..end].to_string());
                rest = &tail[end + 1..];
            }
            _ => rest = tail,
        }
    }
    found.sort();
    found.dedup();
    found
}
