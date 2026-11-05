//! Keyword search, filter-panel rules, colour matching and sorting, following
//! Eagle's behaviour (see the Search and filters chapter of the docs).
//!
//! Like Eagle, everything is a linear pass over the items; there is no index.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, TimeZone};

use crate::model::{Folder, Item};

// ---------------------------------------------------------------------------
// Keyword query

/// A parsed search-box query.
///
/// Eagle flattens the expression tree into three lists: every AND term must
/// match, every OR group must have one member match, and no NOT term may
/// match. Nested groups are flattened the same lossy way, so
/// `dog || (cat black)` is the OR of `dog` and the literal `cat black`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    pub all: Vec<String>,
    pub any: Vec<Vec<String>>,
    pub none: Vec<String>,
    /// The whole query, lower-cased; matched as one substring if the terms
    /// fail.
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Open,
    Close,
    Or,
}

fn tokenize(q: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = q.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '(' => {
                chars.next();
                out.push(Token::Open);
            }
            ')' => {
                chars.next();
                out.push(Token::Close);
            }
            '"' | '\'' => {
                chars.next();
                let mut s = String::new();
                for d in chars.by_ref() {
                    if d == c {
                        break;
                    }
                    s.push(d);
                }
                if !s.is_empty() {
                    out.push(Token::Word(s));
                }
            }
            _ => {
                let mut s = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_whitespace() || d == '(' || d == ')' {
                        break;
                    }
                    s.push(d);
                    chars.next();
                }
                if s == "||" || s.eq_ignore_ascii_case("or") {
                    out.push(Token::Or);
                } else {
                    out.push(Token::Word(s));
                }
            }
        }
    }
    out
}

/// One element of a flattened sequence: a term, or a parenthesised group
/// collapsed to its words joined by spaces.
fn flatten_group(tokens: &[Token], i: &mut usize) -> String {
    let mut words = Vec::new();
    while *i < tokens.len() {
        match &tokens[*i] {
            Token::Close => {
                *i += 1;
                break;
            }
            Token::Open => {
                *i += 1;
                words.push(flatten_group(tokens, i));
            }
            Token::Word(w) => {
                words.push(w.clone());
                *i += 1;
            }
            Token::Or => *i += 1,
        }
    }
    words.join(" ")
}

impl Query {
    pub fn parse(q: &str) -> Query {
        let tokens = tokenize(q);
        let mut terms: Vec<String> = Vec::new();
        let mut ors: Vec<bool> = Vec::new(); // ors[k]: an OR sits before terms[k]
        let mut pending_or = false;
        let mut i = 0;
        while i < tokens.len() {
            match &tokens[i] {
                Token::Or => {
                    pending_or = true;
                    i += 1;
                    continue;
                }
                Token::Open => {
                    i += 1;
                    let g = flatten_group(&tokens, &mut i);
                    if !g.is_empty() {
                        terms.push(g);
                        ors.push(pending_or);
                    }
                }
                Token::Close => i += 1,
                Token::Word(w) => {
                    terms.push(w.clone());
                    ors.push(pending_or);
                    i += 1;
                }
            }
            pending_or = false;
        }

        let mut query = Query { raw: q.trim().to_lowercase(), ..Default::default() };
        let mut k = 0;
        while k < terms.len() {
            // Collect a run joined by ORs.
            let mut run = vec![terms[k].to_lowercase()];
            while k + 1 < terms.len() && ors[k + 1] {
                k += 1;
                run.push(terms[k].to_lowercase());
            }
            k += 1;
            if run.len() > 1 {
                query.any.push(run);
            } else {
                let t = run.pop().unwrap();
                match t.strip_prefix('-') {
                    Some(neg) if !neg.is_empty() => query.none.push(neg.to_string()),
                    _ => query.all.push(t),
                }
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.all.is_empty() && self.any.is_empty() && self.none.is_empty()
    }

    /// Whether `text` (already lower-cased) satisfies the query.
    pub fn matches(&self, text: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        if self.none.iter().any(|n| text.contains(n.as_str())) {
            return false;
        }
        let terms = self.all.iter().all(|t| text.contains(t.as_str()))
            && self.any.iter().all(|g| g.iter().any(|t| text.contains(t.as_str())));
        terms || (!self.raw.is_empty() && text.contains(&self.raw))
    }

    /// Typo-tolerant match (falcon's own, not Eagle's). `Some(0)` when
    /// [`Query::matches`] passes; otherwise `Some(cost)`, the total number of
    /// edits needed for every AND term and one member of every OR group to
    /// match the start of some word in `text`, or `None` if that's too many.
    /// NOT terms stay exact. See the Search chapter for the rules.
    pub fn fuzzy_score(&self, text: &str) -> Option<usize> {
        if self.matches(text) {
            return Some(0);
        }
        if self.none.iter().any(|n| text.contains(n.as_str())) {
            return None;
        }
        let words = || text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty());
        let mut buf: Vec<char> = Vec::new();
        let mut cost = |t: &String| -> Option<usize> {
            if text.contains(t.as_str()) {
                return Some(0);
            }
            let budget = typo_budget(t);
            if budget == 0 {
                return None;
            }
            let t: Vec<char> = t.chars().map(fold).collect();
            let mut best: Option<usize> = None;
            for w in words() {
                // Each term letter the word lacks entirely costs at least one
                // edit, which rules out most words before the full distance.
                let mask = w.chars().fold(0, |m, c| m | letter_bit(fold(c)));
                if t.iter().filter(|&&c| mask & letter_bit(c) == 0).count() > budget {
                    continue;
                }
                buf.clear();
                buf.extend(w.chars().map(fold));
                if let Some(d) = prefix_distance(&t, &buf, best.map_or(budget, |b| b.saturating_sub(1)).min(budget)) {
                    best = Some(best.map_or(d, |b| b.min(d)));
                    if d == 0 {
                        break;
                    }
                }
            }
            best
        };
        let mut total = 0;
        for t in &self.all {
            total += cost(t)?;
        }
        for g in &self.any {
            total += g.iter().filter_map(&mut cost).min()?;
        }
        Some(total.max(1))
    }
}

/// Edits a term may need under fuzzy matching: none for terms under 4
/// characters, phrases and terms with digits (a typo in a number usually
/// means a different number), 1 for 4–7 characters, 2 from 8.
fn typo_budget(term: &str) -> usize {
    if term.chars().any(|c| c.is_whitespace() || c.is_ascii_digit()) {
        return 0;
    }
    match term.chars().count() {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

/// A bit per letter (non-ASCII letters share bits, which only weakens the
/// filter that uses it, never makes it wrong).
fn letter_bit(c: char) -> u128 {
    1u128 << (c as u32 % 128)
}

/// Folds common Latin accents so `cafe` and `café` compare equal.
fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ś' | 'š' => 's',
        'ź' | 'ż' | 'ž' => 'z',
        'ł' => 'l',
        _ => c,
    }
}

/// The smallest optimal-string-alignment distance (Levenshtein plus adjacent
/// swaps) between `term` and either the whole `word` or a prefix of it at
/// least as long as `term`, if it is at most `max`. Shorter prefixes are
/// left out so that deleting letters from the term can't match unrelated
/// words (`tigr` would otherwise match `tiramisu` through `tir`).
fn prefix_distance(term: &[char], word: &[char], max: usize) -> Option<usize> {
    let (m, n) = (term.len(), word.len());
    if n + max < m {
        return None;
    }
    let mut prev2: Vec<usize> = Vec::new();
    let mut prev: Vec<usize> = (0..=n).collect();
    for i in 1..=m {
        let mut cur = vec![i; n + 1];
        for j in 1..=n {
            let sub = prev[j - 1] + usize::from(term[i - 1] != word[j - 1]);
            let mut d = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
            if i > 1 && j > 1 && term[i - 1] == word[j - 2] && term[i - 2] == word[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            cur[j] = d;
        }
        if cur.iter().all(|&d| d > max) {
            return None;
        }
        prev2 = std::mem::replace(&mut prev, cur);
    }
    let best = (m.min(n)..=n).filter(|&j| j >= m || j == n).map(|j| prev[j]).min()?;
    (best <= max).then_some(best)
}

/// Which parts of an item the search box looks at: Eagle's Search Scope
/// menu. Everything is on by default. An item's text content, camera and
/// font names are always searched, whatever the scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SearchScope {
    pub name: bool,
    pub folder_name: bool,
    pub folder_desc: bool,
    pub ext: bool,
    pub tags: bool,
    pub url: bool,
    /// Region and video comments.
    pub comments: bool,
    /// The item's note (`annotation`).
    pub note: bool,
}

impl Default for SearchScope {
    fn default() -> Self {
        SearchScope { name: true, folder_name: true, folder_desc: true, ext: true, tags: true, url: true, comments: true, note: true }
    }
}

/// [`search_text_in`] with every part of the scope on.
pub fn search_text(item: &Item, folders: &FolderIndex, query_len: usize) -> String {
    search_text_in(item, folders, query_len, &SearchScope::default())
}

/// The lower-cased text Eagle searches for an item: its text content
/// (`text`, kept for text files) and camera (`rawMetas.camera`), then, as
/// `scope` allows, the name, `.ext`, url (for queries of two or more
/// characters), note and comments; a font's full name (the `zh` entry
/// first, else `en`) and, for queries of three or more characters, its
/// whole `fontMetas` as JSON; then tags and the name and description of
/// each folder the item is in.
pub fn search_text_in(item: &Item, folders: &FolderIndex, query_len: usize, scope: &SearchScope) -> String {
    let mut s = String::with_capacity(128);
    let mut push = |x: &str| {
        s.push(' ');
        s.push_str(x);
    };
    if let Some(t) = item.extra.get("text").and_then(|v| v.as_str()) {
        push(t);
    }
    if let Some(c) = camera(item) {
        push(c);
    }
    if scope.name {
        push(&item.name);
    }
    if scope.ext && !item.ext.is_empty() {
        push(&format!(".{}", item.ext));
    }
    if scope.url && query_len >= 2 {
        push(&item.url);
    }
    if scope.note {
        push(&item.annotation);
    }
    if scope.comments {
        for t in comment_texts(item) {
            push(t);
        }
    }
    if type_group(&item.ext) == Some("font")
        && let Some(metas) = item.extra.get("fontMetas")
    {
        let full = |lang: &str| metas.get("fullName").and_then(|m| m.get(lang)).and_then(|v| v.as_str()).filter(|n| !n.is_empty());
        push(full("zh").or_else(|| full("en")).unwrap_or(""));
        if query_len > 2 && metas.get("postScriptName").is_some_and(|v| !v.is_null()) {
            push(&metas.to_string());
        }
    }
    if scope.tags {
        for t in &item.tags {
            push(t);
        }
    }
    if scope.folder_name || scope.folder_desc {
        for f in &item.folders {
            if let Some((name, desc)) = folders.get(f) {
                if scope.folder_name {
                    push(name);
                }
                if scope.folder_desc {
                    push(desc);
                }
            }
        }
    }
    s.to_lowercase()
}

/// The text of each of an item's comments (`comments[].annotation`).
pub fn comment_texts(item: &Item) -> impl Iterator<Item = &str> {
    item.extra.get("comments").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|c| c.get("annotation").and_then(|a| a.as_str()))
}

/// Folder id to (name, description).
pub type FolderIndex = HashMap<String, (String, String)>;

pub fn folder_index(roots: &[Folder]) -> FolderIndex {
    let mut out = HashMap::new();
    for r in roots {
        r.walk(&mut |f| {
            out.insert(f.id.clone(), (f.name.clone(), f.description.clone()));
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Name ranking (filter searches and Go to Folder)

/// How well `word` matches `text` by Eagle's quick-search score (the
/// string_score method): 1 for identical strings, 0 when the letters of
/// `word` do not all appear in order, otherwise a score rewarding letters
/// that follow each other, start a word or keep their case, and a shared
/// first letter. Lengths and positions are in UTF-16 units, as in
/// JavaScript.
pub fn string_score(text: &str, word: &str) -> f64 {
    if text == word {
        return 1.0;
    }
    if word.is_empty() {
        return 0.0;
    }
    let s: Vec<u16> = text.encode_utf16().collect();
    let ls: Vec<u16> = text.to_lowercase().encode_utf16().collect();
    let w: Vec<u16> = word.encode_utf16().collect();
    let lw: Vec<u16> = word.to_lowercase().encode_utf16().collect();
    let mut running = 0.0;
    let mut start = 0usize;
    for (i, &c) in lw.iter().enumerate() {
        let Some(found) = ls.get(start..).and_then(|rest| rest.iter().position(|&x| x == c)).map(|p| p + start) else {
            return 0.0;
        };
        let mut score = if found == start {
            0.7
        } else {
            let after_space = found > 0 && s.get(found - 1) == Some(&(' ' as u16));
            if after_space { 0.9 } else { 0.1 }
        };
        if s.get(found).is_some() && s.get(found) == w.get(i) {
            score += 0.1;
        }
        running += score;
        start = found + 1;
    }
    let mut total = 0.5 * (running / s.len() as f64 + running / w.len() as f64);
    if lw.first() == ls.first() && total < 0.85 {
        total += 0.15;
    }
    total
}

/// A name as Eagle compares it when ranking: decomposed, without
/// combining accents, lower-cased.
pub fn rank_key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfkd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase()
}

/// Ranks `names` against a typed keyword as Eagle's tag and folder filter
/// searches and Go to Folder do: the keyword loses its spaces, both sides
/// go through [`rank_key`], names scoring 0 are dropped and the rest are
/// ordered best first (ties keep their order). Returns indices into
/// `names`. (Eagle also scores the pinyin of Chinese names; falcon does
/// not.)
pub fn rank_names<'a>(names: impl IntoIterator<Item = &'a str>, keyword: &str) -> Vec<usize> {
    let key = rank_key(keyword).replace(' ', "");
    let mut scored: Vec<(usize, f64)> =
        names.into_iter().enumerate().map(|(n, name)| (n, string_score(&rank_key(name), &key))).filter(|(_, sc)| *sc > 0.0).collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.into_iter().map(|(n, _)| n).collect()
}

// ---------------------------------------------------------------------------
// Filter panel

/// How the tag and folder filters combine their lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Logic {
    #[default]
    Or,
    And,
    Equal,
}

/// Include/exclude lists for tags or folder ids.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetFilter {
    pub logic: Logic,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    /// Also match items with an empty list (`tag.no` / `NoFolders`).
    pub empty: bool,
    /// Exclude items with an empty list (the folder filter's excluded
    /// Uncategorized, `folder.excludes.NoFolders`).
    pub exclude_empty: bool,
}

impl SetFilter {
    pub fn is_active(&self) -> bool {
        !self.includes.is_empty() || !self.excludes.is_empty() || self.empty || self.exclude_empty
    }

    /// Eagle's semantics, including its OR quirk: under OR an item with any
    /// included value passes even if it also has an excluded one.
    pub fn matches(&self, values: &[String]) -> bool {
        if !self.is_active() {
            return true;
        }
        let has = |v: &String| values.contains(v);
        let excluding = !self.excludes.is_empty() || self.exclude_empty;
        let excluded = self.excludes.iter().any(has) || (self.exclude_empty && values.is_empty());
        let base = match self.logic {
            Logic::Or => self.includes.iter().any(has) || (excluding && !excluded),
            Logic::And | Logic::Equal => {
                let ok = self.includes.iter().all(has) && !excluded;
                let ok = ok && (self.logic != Logic::Equal || values.len() == self.includes.len());
                ok && (!self.includes.is_empty() || excluding)
            }
        };
        match (self.empty, self.logic) {
            (true, Logic::Or) => base || values.is_empty(),
            (true, _) if self.includes.is_empty() && !excluding => values.is_empty(),
            (true, _) => base && values.is_empty(),
            (false, _) => base,
        }
    }
}

/// `has` / `no` / keywords for the url and note filters.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum TextFilter {
    #[default]
    Off,
    Has,
    No,
    /// Non-empty and containing every keyword (case-insensitive).
    Keywords(Vec<String>),
}

impl TextFilter {
    /// The rule over a list of texts, as Eagle's comment filter reads an
    /// item's comments: `Has` needs one, `No` none, and every keyword must
    /// occur (case-insensitively) in one of them.
    pub fn matches_any<'a>(&self, values: impl Iterator<Item = &'a str>) -> bool {
        match self {
            TextFilter::Off => true,
            TextFilter::Has => values.count() > 0,
            TextFilter::No => values.count() == 0,
            TextFilter::Keywords(k) => {
                let v: Vec<String> = values.map(str::to_lowercase).collect();
                k.iter().all(|w| {
                    let w = w.to_lowercase();
                    v.iter().any(|t| t.contains(&w))
                })
            }
        }
    }

    fn matches(&self, value: &str) -> bool {
        match self {
            TextFilter::Off => true,
            TextFilter::Has => !value.is_empty(),
            TextFilter::No => value.is_empty(),
            TextFilter::Keywords(k) => {
                let v = value.to_lowercase();
                !value.is_empty() && k.iter().all(|w| v.contains(&w.to_lowercase()))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    Landscape,
    Portrait,
    Square,
    PanoramicLandscape,
    PanoramicPortrait,
    /// Exactly 4:3, 3:4, 16:9 or 9:16 (width over height compared as
    /// floating point, as Eagle does, so 1366 × 768 is not 16:9).
    Ratio4x3,
    Ratio3x4,
    Ratio16x9,
    Ratio9x16,
}

impl Shape {
    /// Every shape in the order of Eagle's Shape filter.
    pub const ALL: [Shape; 9] = [
        Shape::Landscape,
        Shape::Portrait,
        Shape::Square,
        Shape::PanoramicLandscape,
        Shape::PanoramicPortrait,
        Shape::Ratio4x3,
        Shape::Ratio3x4,
        Shape::Ratio16x9,
        Shape::Ratio9x16,
    ];

    pub fn matches(self, w: u64, h: u64) -> bool {
        let (w, h) = (w as f64, h as f64);
        match self {
            Shape::Landscape => w > h,
            Shape::Portrait => h > w,
            Shape::Square => w == h,
            Shape::PanoramicLandscape => w > h && w / h >= 2.5,
            Shape::PanoramicPortrait => w < h && h / w >= 2.5,
            Shape::Ratio4x3 => w / h == 4.0 / 3.0,
            Shape::Ratio3x4 => w / h == 3.0 / 4.0,
            Shape::Ratio16x9 => w / h == 16.0 / 9.0,
            Shape::Ratio9x16 => w / h == 9.0 / 16.0,
        }
    }
}

/// Eagle's custom W:H shape: the exact ratio, or every item while either
/// side is unset (0).
pub fn custom_shape_matches((rw, rh): (f64, f64), w: u64, h: u64) -> bool {
    if rw == 0.0 || rh == 0.0 || rw.is_nan() || rh.is_nan() {
        return true;
    }
    w as f64 / h as f64 == rw / rh
}

/// How many items the Shape filter's options would find, as Eagle counts
/// them: only items with a width that are not audio or fonts; each falls
/// in one of the five orientations (panoramic ones are not also counted
/// as horizontal or vertical) and at most one of the four ratios.
pub fn shape_counts<'a>(items: impl IntoIterator<Item = &'a Item>) -> HashMap<Shape, usize> {
    let mut out: HashMap<Shape, usize> = HashMap::new();
    for i in items {
        if i.width == 0 || matches!(type_group(&i.ext), Some("audio" | "font")) {
            continue;
        }
        let (w, h) = (i.width as f64, i.height as f64);
        let orientation = if w > h {
            if w / h >= 2.5 { Shape::PanoramicLandscape } else { Shape::Landscape }
        } else if w < h {
            if h / w >= 2.5 { Shape::PanoramicPortrait } else { Shape::Portrait }
        } else {
            Shape::Square
        };
        *out.entry(orientation).or_default() += 1;
        if let Some(r) = [Shape::Ratio4x3, Shape::Ratio3x4, Shape::Ratio16x9, Shape::Ratio9x16].into_iter().find(|r| r.matches(i.width, i.height)) {
            *out.entry(r).or_default() += 1;
        }
    }
    out
}

/// Eagle's file-type groups for the type filter.
pub fn type_group(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "ts" | "3gp" | "360" | "afx" | "vap" | "eva" | "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" | "wmv"
        | "mpg" | "mts" | "flv" | "m2ts" | "f4v" => "video",
        e if crate::media::is_audio(e) => "audio",
        "ttf" | "otf" | "ttc" | "woff" | "woff2" => "font",
        "ppt" | "pptx" | "potx" => "powerpoint",
        "doc" | "docx" => "word",
        "xls" | "xlsx" => "excel",
        _ => return None,
    })
}

/// The web-video site of a bookmark (`medium`: youtube, vimeo, bilibili).
pub fn medium(item: &Item) -> Option<&str> {
    item.extra.get("medium").and_then(|v| v.as_str()).filter(|m| !m.is_empty())
}

/// The type filter's built-in groups, after the extensions in its list.
pub const TYPE_GROUPS: [&str; 6] = ["font", "video", "audio", "youtube", "vimeo", "bilibili"];

/// Whether an item passes the type filter's included types, in Eagle's
/// order and with its early answers: its own extension wins; otherwise
/// the first included `video` or `audio` group decides by itself (so
/// including both video and audio lets only videos through), a `url`
/// entry takes bookmarks without a site, a site name its bookmarks, and
/// the Office groups and `font` follow.
pub fn type_included(includes: &[String], item: &Item) -> bool {
    let has = |t: &str| includes.iter().any(|x| x == t);
    let ext = item.ext.as_str();
    let group = type_group(ext);
    if has(ext) {
        return true;
    }
    if has("video") {
        return group == Some("video");
    }
    let site = medium(item);
    if has("url") && ext == "url" && site.is_none() {
        return true;
    }
    for s in ["youtube", "vimeo", "bilibili"] {
        if has(s) && ext == "url" && site == Some(s) {
            return true;
        }
    }
    if has("audio") {
        return group == Some("audio");
    }
    for g in ["powerpoint", "word", "excel"] {
        if has(g) && group == Some(g) {
            return true;
        }
    }
    has("font") && group == Some("font")
}

/// Whether one of the type filter's excluded types covers an item.
pub fn type_excluded(excludes: &[String], item: &Item) -> bool {
    let has = |t: &str| excludes.iter().any(|x| x == t);
    let ext = item.ext.as_str();
    let site = medium(item);
    has(ext)
        || type_group(ext).is_some_and(has)
        || (ext == "url" && site.is_none() && has("url"))
        || (ext == "url" && site.is_some_and(has))
}

/// How many items each option of the type filter covers: by extension, or
/// by site for web-video bookmarks, as Eagle counts them, plus the font,
/// video and audio groups (which Eagle lists but never counts, so its
/// panel hides them).
pub fn type_counts<'a>(items: impl IntoIterator<Item = &'a Item>) -> HashMap<String, usize> {
    let mut out: HashMap<String, usize> = HashMap::new();
    for i in items {
        *out.entry(medium(i).unwrap_or(&i.ext).to_string()).or_default() += 1;
        if let Some(g @ ("font" | "video" | "audio")) = type_group(&i.ext) {
            *out.entry(g.to_string()).or_default() += 1;
        }
    }
    out
}

/// The type filter's options: the extensions present, in code-unit order,
/// then the built-in groups not already listed.
pub fn type_options<'a>(items: impl IntoIterator<Item = &'a Item>) -> Vec<String> {
    let exts: HashSet<&str> = items.into_iter().map(|i| i.ext.as_str()).collect();
    let mut out: Vec<String> = exts.into_iter().map(String::from).collect();
    out.sort_by(|a, b| js_cmp(a, b));
    for g in TYPE_GROUPS {
        if !out.iter().any(|x| x == g) {
            out.push(g.to_string());
        }
    }
    out
}

/// The filter panel's rules. Every active rule must pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub tags: SetFilter,
    pub folders: SetFilter,
    /// Extensions or group names to include / exclude.
    pub types: Vec<String>,
    pub exclude_types: Vec<String>,
    /// Inclusive byte range.
    pub size: (Option<u64>, Option<u64>),
    /// Inclusive pixel ranges.
    pub width: (Option<u64>, Option<u64>),
    pub height: (Option<u64>, Option<u64>),
    /// Ratings to match (0 = unrated), OR-ed.
    pub ratings: HashSet<u8>,
    pub shapes: HashSet<Shape>,
    /// The custom W:H shape when it is ticked (see [`custom_shape_matches`]);
    /// OR-ed with `shapes`.
    pub custom_shape: Option<(f64, f64)>,
    pub url: TextFilter,
    pub note: TextFilter,
    /// Region and video comments (Eagle's `annotation` rule).
    pub comments: TextFilter,
    pub color: Option<ColorQuery>,
    pub grayscale: bool,
    /// Date Imported (`modificationTime`).
    pub imported: DateFilter,
    /// Date Created (`btime`); not in Eagle's panel, only in its smart folders.
    pub created: DateFilter,
    /// Date Modified (`mtime`).
    pub modified: DateFilter,
    /// Inclusive range in seconds; each bound is truncated to whole seconds
    /// as Eagle's `parseInt` does. Items without a `duration` fail an active
    /// range.
    pub duration: (Option<f64>, Option<f64>),
    /// Inclusive tempo range in BPM; each bound is truncated to a whole
    /// number as Eagle's `parseInt` does. Items without a `bpm` (or with
    /// `0`) fail an active range.
    pub bpm: (Option<f64>, Option<f64>),
    /// Camera names (`rawMetas.camera`), matched exactly, OR-ed.
    pub cameras: HashSet<String>,
    pub font: FontFilter,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        *self != Filter::default()
    }

    /// Whether `item` passes every rule; for a colour rule also returns the
    /// distance Eagle sorts by.
    pub fn check(&self, item: &Item) -> Option<f64> {
        if !self.types.is_empty() && !type_included(&self.types, item) {
            return None;
        }
        if type_excluded(&self.exclude_types, item) {
            return None;
        }
        let within = |r: (Option<u64>, Option<u64>), v: u64| r.0.is_none_or(|a| v >= a) && r.1.is_none_or(|b| v <= b);
        if !within(self.size, item.size) || !within(self.width, item.width) || !within(self.height, item.height) {
            return None;
        }
        if !self.ratings.is_empty() {
            let star = item.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            if !self.ratings.contains(&star) {
                return None;
            }
        }
        if (!self.shapes.is_empty() || self.custom_shape.is_some())
            && !self.shapes.iter().any(|s| s.matches(item.width, item.height))
            && !self.custom_shape.is_some_and(|r| custom_shape_matches(r, item.width, item.height))
        {
            return None;
        }
        if !self.url.matches(&item.url) || !self.note.matches(&item.annotation) || !self.comments.matches_any(comment_texts(item)) {
            return None;
        }
        if self.grayscale && !is_grayscale(item) {
            return None;
        }
        if !self.tags.matches(&item.tags) || !self.folders.matches(&item.folders) {
            return None;
        }
        if !self.imported.matches(DateField::Imported.value(item))
            || !self.created.matches(DateField::Created.value(item))
            || !self.modified.matches(DateField::Modified.value(item))
        {
            return None;
        }
        if self.duration != (None, None) {
            let d = duration(item)?;
            if self.duration.0.is_some_and(|a| d < a.trunc()) || self.duration.1.is_some_and(|b| d > b.trunc()) {
                return None;
            }
        }
        if self.bpm != (None, None) {
            let b = bpm(item)?;
            if self.bpm.0.is_some_and(|a| b < a.trunc()) || self.bpm.1.is_some_and(|z| b > z.trunc()) {
                return None;
            }
        }
        if !self.cameras.is_empty() && !camera(item).is_some_and(|c| self.cameras.contains(c)) {
            return None;
        }
        if !self.font.matches(item) {
            return None;
        }
        match &self.color {
            Some(c) => c.distance(item),
            None => Some(0.0),
        }
    }
}

// ---------------------------------------------------------------------------
// Dates

/// One day in milliseconds; Eagle steps days by this, ignoring DST.
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Which timestamp a date rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateField {
    /// `modificationTime`, the import time.
    Imported,
    /// `btime`, falling back to `modificationTime` when 0.
    Created,
    /// `mtime`, falling back to `modificationTime` when 0.
    Modified,
}

impl DateField {
    pub fn value(self, item: &Item) -> u64 {
        let or_import = |t: u64| if t == 0 { item.modification_time } else { t };
        match self {
            DateField::Imported => item.modification_time,
            DateField::Created => or_import(item.btime),
            DateField::Modified => or_import(item.mtime),
        }
    }
}

/// Eagle's date presets. Several selected presets are OR-ed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatePreset {
    /// After local midnight today.
    Today,
    /// Between local midnight yesterday and today (both exclusive).
    Yesterday,
    /// Less than N × 24 h before now.
    Last7Days,
    Last30Days,
    Last90Days,
    Last365Days,
}

impl DatePreset {
    pub const ALL: [DatePreset; 6] = [
        DatePreset::Today,
        DatePreset::Yesterday,
        DatePreset::Last7Days,
        DatePreset::Last30Days,
        DatePreset::Last90Days,
        DatePreset::Last365Days,
    ];

    pub fn matches(self, t: i64, clock: Clock) -> bool {
        let within = |days: i64| clock.now - t < days * DAY_MS;
        match self {
            DatePreset::Today => t > clock.today_start,
            DatePreset::Yesterday => t < clock.today_start && t > clock.today_start - DAY_MS,
            DatePreset::Last7Days => within(7),
            DatePreset::Last30Days => within(30),
            DatePreset::Last90Days => within(90),
            DatePreset::Last365Days => within(365),
        }
    }
}

/// The current time and the start of the local day, in ms since the epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub now: i64,
    pub today_start: i64,
}

impl Clock {
    /// Reads the system clock and time zone.
    pub fn local() -> Clock {
        let now = Local::now();
        let today_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|m| Local.from_local_datetime(&m).earliest())
            .map_or(now.timestamp_millis(), |d| d.timestamp_millis());
        Clock { now: now.timestamp_millis(), today_start }
    }
}

/// Local midnight at the start of `year-month-day`, in ms; for building
/// [`DateFilter::range`] from a date picker.
pub fn local_day_start(year: i32, month: u32, day: u32) -> Option<u64> {
    let d = chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(0, 0, 0)?;
    Local.from_local_datetime(&d).earliest().and_then(|d| u64::try_from(d.timestamp_millis()).ok())
}

/// The local `(year, month)` of a timestamp in ms.
pub fn local_month(t: u64) -> Option<(i32, u32)> {
    let d = Local.timestamp_millis_opt(i64::try_from(t).ok()?).single()?;
    Some((d.year(), d.month()))
}

/// One date rule of the filter panel (Date Imported, Date Modified).
///
/// Presets and the range are OR-ed with each other; the month list is a
/// separate rule that must also pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DateFilter {
    pub presets: HashSet<DatePreset>,
    /// Days picked in the calendar: the local midnights starting the first
    /// and the last day. The last day is included whole (Eagle adds 24 h).
    pub range: Option<(u64, u64)>,
    /// Calendar months in local time, `(year, month 1..=12)`.
    pub months: HashSet<(i32, u32)>,
    /// The clock the presets compare with; `None` reads the system clock.
    pub clock: Option<Clock>,
}

impl DateFilter {
    pub fn is_active(&self) -> bool {
        !self.presets.is_empty() || self.range.is_some() || !self.months.is_empty()
    }

    /// Whether a timestamp (ms) passes this rule.
    pub fn matches(&self, t: u64) -> bool {
        if !self.months.is_empty() && !local_month(t).is_some_and(|m| self.months.contains(&m)) {
            return false;
        }
        if self.presets.is_empty() && self.range.is_none() {
            return true;
        }
        let t = t as i64;
        // Eagle ignores a range unless both ends are set (non-zero).
        let in_range = self.range.is_some_and(|(a, b)| a > 0 && b > 0 && a as i64 <= t && t <= b as i64 + DAY_MS);
        if in_range {
            return true;
        }
        if self.presets.is_empty() {
            return false;
        }
        let clock = self.clock.unwrap_or_else(Clock::local);
        self.presets.iter().any(|p| p.matches(t, clock))
    }
}

/// How many items fall in each local month, newest month first; the options
/// of a date rule's month list.
pub fn month_counts<'a>(items: impl IntoIterator<Item = &'a Item>, field: DateField) -> Vec<((i32, u32), usize)> {
    let mut counts: HashMap<(i32, u32), usize> = HashMap::new();
    for i in items {
        if let Some(m) = local_month(field.value(i)) {
            *counts.entry(m).or_default() += 1;
        }
    }
    let mut out: Vec<_> = counts.into_iter().collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out
}

// ---------------------------------------------------------------------------
// Duration

/// Units of Eagle's duration fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DurationUnit {
    #[default]
    Seconds,
    Minutes,
    Hours,
}

impl DurationUnit {
    pub fn to_seconds(self, v: f64) -> f64 {
        match self {
            DurationUnit::Seconds => v,
            DurationUnit::Minutes => v * 60.0,
            DurationUnit::Hours => v * 3600.0,
        }
    }
}

/// An item's `duration` in seconds (audio and video).
pub fn duration(item: &Item) -> Option<f64> {
    item.extra.get("duration").and_then(|v| v.as_f64())
}

/// An item's `bpm` (audio tempo), when it has a non-zero one.
pub fn bpm(item: &Item) -> Option<f64> {
    item.extra.get("bpm").and_then(|v| v.as_f64()).filter(|b| *b != 0.0)
}

/// The shortest and longest duration among `items`, for the duration
/// fields' placeholders.
pub fn duration_range<'a>(items: impl IntoIterator<Item = &'a Item>) -> Option<(f64, f64)> {
    items.into_iter().filter_map(duration).fold(None, |acc, d| match acc {
        None => Some((d, d)),
        Some((a, b)) => Some((a.min(d), b.max(d))),
    })
}

// ---------------------------------------------------------------------------
// Camera and font

/// The camera Eagle stored for a RAW photo (`rawMetas.camera`).
pub fn camera(item: &Item) -> Option<&str> {
    item.extra.get("rawMetas")?.get("camera")?.as_str().filter(|c| !c.is_empty())
}

/// JavaScript's default string order (UTF-16 code units).
fn js_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// The camera filter's options: every distinct camera, sorted as Eagle sorts
/// them (plain code-unit order, so case-sensitive).
pub fn distinct_cameras<'a>(items: impl IntoIterator<Item = &'a Item>) -> Vec<String> {
    let set: HashSet<&str> = items.into_iter().filter_map(camera).collect();
    let mut out: Vec<String> = set.into_iter().map(String::from).collect();
    out.sort_by(|a, b| js_cmp(a, b));
    out
}

/// First value of one of a font's `fontMetas` name maps, such as
/// `postScriptName` or `fontFamily` (Eagle takes the first language key).
pub fn font_name<'a>(item: &'a Item, key: &str) -> Option<&'a str> {
    let m = item.extra.get("fontMetas")?.get(key)?.as_object()?;
    m.values().next()?.as_str()
}

/// The font's family name (`fontMetas.fontFamily`).
pub fn font_family(item: &Item) -> Option<&str> {
    font_name(item, "fontFamily")
}

/// Every distinct font family, in natural order.
pub fn distinct_font_families<'a>(items: impl IntoIterator<Item = &'a Item>) -> Vec<String> {
    let set: HashSet<&str> = items.into_iter().filter_map(font_family).collect();
    let mut out: Vec<String> = set.into_iter().map(String::from).collect();
    out.sort_by(|a, b| natural_cmp(a, b));
    out
}

/// The key Eagle looks up to tell whether a font is activated:
/// `<postScriptName>_.<ext>`.
pub fn font_key(item: &Item) -> Option<String> {
    Some(format!("{}_.{}", font_name(item, "postScriptName")?, item.ext))
}

/// The folder Eagle copies activated fonts into (macOS:
/// `~/Library/Fonts/EagleApp/`).
pub fn eagle_font_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    crate::sandbox::home_dir().map(|h| h.join("Library/Fonts/EagleApp"))
}

/// The folder falcon copies the fonts it activates into (macOS:
/// `~/Library/Fonts/Falcon/`; on Windows `Falcon\Fonts` in `%LOCALAPPDATA%`;
/// elsewhere `fonts/Falcon` in the user's data folder, which fontconfig
/// reads). falcon never writes to Eagle's folder, which the user's Eagle
/// manages; it only reads it.
pub fn falcon_font_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return crate::sandbox::home_dir().map(|h| h.join("Library/Fonts/Falcon"));
    }
    #[cfg(windows)]
    return Some(crate::sandbox::local_data_dir()?.join("Falcon").join("Fonts"));
    #[cfg(not(windows))]
    {
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| Some(crate::sandbox::home_dir()?.join(".local/share")))?;
        Some(data.join("fonts/Falcon"))
    }
}

/// The folders whose fonts count as activated: Eagle's and falcon's.
pub fn font_dirs() -> Vec<PathBuf> {
    eagle_font_dir().into_iter().chain(falcon_font_dir()).collect()
}

/// Keys of the activated fonts across `dirs` (see [`installed_font_keys`]):
/// a font in any of them counts as activated.
pub fn installed_font_keys_in(dirs: &[PathBuf]) -> HashSet<String> {
    dirs.iter().flat_map(|d| installed_font_keys(d)).collect()
}

/// Keys of the activated fonts in `dir`, built from file names the way Eagle
/// does: `<base>_<.ext lower-cased>`. The base only loses the extension when
/// the file name ends with the lower-cased form, as with Node's `basename`.
pub fn installed_font_keys(dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return HashSet::new() };
    entries.flatten().map(|e| font_file_key(&e.file_name().to_string_lossy())).collect()
}

fn font_file_key(file: &str) -> String {
    let ext = Path::new(file).extension().map(|x| format!(".{}", x.to_string_lossy().to_lowercase())).unwrap_or_default();
    let base = file.strip_suffix(ext.as_str()).filter(|b| !b.is_empty()).unwrap_or(file);
    format!("{base}_{ext}")
}

/// The font filter: activated (installed through Eagle) or not. The sets
/// hold [`installed_font_keys`]. Items that are not fonts fail both.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum FontFilter {
    #[default]
    Off,
    Activated(HashSet<String>),
    Deactivated(HashSet<String>),
}

impl FontFilter {
    pub fn matches(&self, item: &Item) -> bool {
        let installed = |set: &HashSet<String>| font_key(item).map(|k| set.contains(&k));
        match self {
            FontFilter::Off => true,
            FontFilter::Activated(set) => installed(set) == Some(true),
            FontFilter::Deactivated(set) => installed(set) == Some(false),
        }
    }
}

// ---------------------------------------------------------------------------
// Colour

/// A colour-search query: the colour and Eagle's accuracy (5..=40, default
/// 20; lower is stricter).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorQuery {
    pub rgb: [u8; 3],
    pub accuracy: f64,
}

impl ColorQuery {
    pub fn new(rgb: [u8; 3]) -> ColorQuery {
        ColorQuery { rgb, accuracy: 20.0 }
    }

    /// Eagle's match on the two dominant palette colours, then on any
    /// colour marked as primary; `Some(distance)` when the item matches.
    pub fn distance(&self, item: &Item) -> Option<f64> {
        let p = &item.palettes;
        let ratio = |i: usize| p.get(i).and_then(|c| c.ratio.as_f64()).unwrap_or(0.0);
        if p.is_empty() || ratio(0) < 25.0 {
            return None;
        }
        if p.iter().take(2).any(|c| c.color == self.rgb) {
            return Some(0.01);
        }
        let q = lab(self.rgb);
        let acc = self.accuracy;
        let near = |c: [u8; 3]| {
            let l = lab(c);
            (delta_e00(q, l) < acc && delta_e76(q, l) < acc + 50.0).then(|| delta_e76(q, l))
        };
        if ratio(0) > 33.0 {
            if let Some(d) = near(p[0].color) {
                return Some(d);
            }
            // A mostly white background: try the second colour instead.
            // (Inferred nesting; read flat, Eagle's branch is unreachable.)
            if ratio(0) > 50.0
                && p.len() > 1
                && delta_e00(lab([255, 255, 255]), lab(p[0].color)) < 5.0
                && ratio(1) > 8.0
                && let Some(d) = near(p[1].color)
            {
                return Some(d + 5.0);
            }
        }
        if ratio(1) > 33.0
            && p.len() > 1
            && let Some(d) = near(p[1].color)
        {
            return Some(d);
        }
        // A colour marked as the item's primary one (`marked`, set from the
        // inspector's palette) matches within a fixed ΔE2000 of 30, whatever
        // its share or the accuracy, as in Eagle.
        p.iter().filter(|c| c.extra.get(crate::item_ops::MARKED_KEY).and_then(serde_json::Value::as_bool) == Some(true)).find_map(|c| {
            let l = lab(c.color);
            (delta_e00(q, l) < 30.0).then(|| delta_e76(q, l))
        })
    }
}

fn is_grayscale(item: &Item) -> bool {
    !item.palettes.is_empty()
        && item.palettes.iter().filter(|c| c.ratio.as_f64().unwrap_or(0.0) >= 0.02).all(|c| {
            let [r, g, b] = c.color.map(i32::from);
            (r - g).abs() < 8 && (r - b).abs() < 8 && (g - b).abs() < 8
        })
}

/// sRGB to CIELAB (D65), rounded to integers like the `color-convert`
/// package Eagle uses.
pub fn lab(rgb: [u8; 3]) -> [f64; 3] {
    let lin = |c: u8| {
        let c = c as f64 / 255.0;
        if c > 0.04045 { ((c + 0.055) / 1.055).powf(2.4) } else { c / 12.92 }
    };
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (r * 0.4124 + g * 0.3576 + b * 0.1805) * 100.0 / 95.047;
    let y = (r * 0.2126 + g * 0.7152 + b * 0.0722) * 100.0 / 100.0;
    let z = (r * 0.0193 + g * 0.1192 + b * 0.9505) * 100.0 / 108.883;
    let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [(116.0 * fy - 16.0).round(), (500.0 * (fx - fy)).round(), (200.0 * (fy - fz)).round()]
}

/// CIE76 colour difference.
pub fn delta_e76(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// CIEDE2000 colour difference (kL = kC = kH = 1).
pub fn delta_e00(x: [f64; 3], y: [f64; 3]) -> f64 {
    use std::f64::consts::PI;
    let (l1, a1, b1) = (x[0], x[1], x[2]);
    let (l2, a2, b2) = (y[0], y[1], y[2]);
    let rad = |d: f64| d * PI / 180.0;
    let deg = |r: f64| r * 180.0 / PI;
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let cbar = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (cbar.powi(7) / (cbar.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = ((a1p * a1p + b1 * b1).sqrt(), (a2p * a2p + b2 * b2).sqrt());
    let hue = |b: f64, a: f64| {
        if b == 0.0 && a == 0.0 {
            0.0
        } else {
            let h = deg(b.atan2(a));
            if h < 0.0 { h + 360.0 } else { h }
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhh = 2.0 * (c1p * c2p).sqrt() * rad(dh / 2.0).sin();
    let lbar = (l1 + l2) / 2.0;
    let cbarp = (c1p + c2p) / 2.0;
    let hbarp = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * rad(hbarp - 30.0).cos() + 0.24 * rad(2.0 * hbarp).cos() + 0.32 * rad(3.0 * hbarp + 6.0).cos()
        - 0.20 * rad(4.0 * hbarp - 63.0).cos();
    let dtheta = 30.0 * (-((hbarp - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cbarp.powi(7) / (cbarp.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lbar - 50.0).powi(2) / (20.0 + (lbar - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cbarp;
    let sh = 1.0 + 0.015 * cbarp * t;
    let rt = -rad(2.0 * dtheta).sin() * rc;
    ((dl / sl).powi(2) + (dc / sc).powi(2) + (dhh / sh).powi(2) + rt * (dc / sc) * (dhh / sh)).sqrt()
}

// ---------------------------------------------------------------------------
// Sorting

/// Eagle's `orderBy` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum OrderBy {
    #[default]
    Import,
    Name,
    Ext,
    Resolution,
    FileSize,
    Rating,
    Btime,
    Mtime,
    Tags,
    /// Video and audio length in whole seconds; items without one count
    /// as 0.
    Duration,
    /// Shuffled: [`sort`] keeps the incoming order and the caller shuffles.
    Random,
}

impl OrderBy {
    /// Every key of the global sort menu, in Eagle's order.
    pub const ALL: [OrderBy; 11] = [
        OrderBy::Import,
        OrderBy::Btime,
        OrderBy::Mtime,
        OrderBy::Name,
        OrderBy::Ext,
        OrderBy::FileSize,
        OrderBy::Resolution,
        OrderBy::Rating,
        OrderBy::Tags,
        OrderBy::Duration,
        OrderBy::Random,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OrderBy::Import => "Date Imported",
            OrderBy::Btime => "Date Created",
            OrderBy::Mtime => "Date Modified",
            OrderBy::Name => "Title",
            OrderBy::Ext => "Extension",
            OrderBy::FileSize => "File Size",
            OrderBy::Resolution => "Dimensions",
            OrderBy::Rating => "Rating",
            OrderBy::Tags => "Tags",
            OrderBy::Duration => "Duration",
            OrderBy::Random => "Random",
        }
    }

    /// Whether this key sorts newest-first before the direction flag.
    pub fn is_time(self) -> bool {
        matches!(self, OrderBy::Import | OrderBy::Btime | OrderBy::Mtime)
    }
}

/// Case-insensitive comparison with numbers compared by value, like
/// `Intl.Collator(..., {numeric: true, sensitivity: 'base'})`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    n.push(c);
                    x.next();
                }
                let mut m = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    m.push(d);
                    y.next();
                }
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Sorts items as Eagle does: the key's own direction (time keys newest
/// first, others ascending), reversed when `increase` is false. The sort is
/// stable, so ties keep their incoming order.
pub fn sort(items: &mut [&Item], order: OrderBy, increase: bool) {
    let star = |i: &Item| i.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0);
    let or_import = |t: u64, i: &Item| if t == 0 { i.modification_time } else { t };
    items.sort_by(|a, b| match order {
        OrderBy::Import => b.modification_time.cmp(&a.modification_time),
        OrderBy::Btime => or_import(b.btime, b).cmp(&or_import(a.btime, a)),
        OrderBy::Mtime => or_import(b.mtime, b).cmp(&or_import(a.mtime, a)),
        OrderBy::Name => natural_cmp(&a.name, &b.name),
        OrderBy::Ext => natural_cmp(&a.ext, &b.ext),
        OrderBy::Resolution => (a.width * a.height).cmp(&(b.width * b.height)),
        OrderBy::FileSize => a.size.cmp(&b.size),
        OrderBy::Rating => star(a).cmp(&star(b)),
        OrderBy::Tags => natural_cmp(a.tags.first().map_or("", |s| s), b.tags.first().map_or("", |s| s)),
        // Eagle: `parseInt(duration) || 0`, shortest first.
        OrderBy::Duration => {
            let secs = |i: &Item| duration(i).filter(|d| d.is_finite()).map_or(0, |d| d.trunc() as i64);
            secs(a).cmp(&secs(b))
        }
        OrderBy::Random => Ordering::Equal,
    });
    if !increase {
        items.reverse();
    }
}

/// Sorts items by their hand-arranged order in `folder` (a folder's
/// `MANUAL` sort): `order[folder]` compared as text, larger first, with
/// `modificationTime` standing in for items that have no entry, as Eagle
/// does. Reversed when `increase` is false. Stable.
pub fn sort_manual(items: &mut [&Item], folder: &str, increase: bool) {
    let mut keyed: Vec<(String, &Item)> = items.iter().map(|i| (i.manual_sort_key(folder), *i)).collect();
    keyed.sort_by(|a, b| b.0.cmp(&a.0));
    for (slot, (_, item)) in items.iter_mut().zip(keyed) {
        *slot = item;
    }
    if !increase {
        items.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_syntax() {
        let q = Query::parse(r#"cat "black dog" -bird"#);
        assert_eq!(q.all, ["cat", "black dog"]);
        assert_eq!(q.none, ["bird"]);
        let q = Query::parse("red OR blue || green tree");
        assert_eq!(q.any, [vec!["red", "blue", "green"]]);
        assert_eq!(q.all, ["tree"]);
        // Eagle's lossy flattening of nested groups.
        let q = Query::parse("dog || (cat black)");
        assert_eq!(q.any, [vec!["dog", "cat black"]]);
    }

    #[test]
    fn query_matching() {
        let q = Query::parse("cat -bird");
        assert!(q.matches("a cat here"));
        assert!(!q.matches("a cat and a bird"));
        assert!(Query::parse("-bird").matches("nothing"));
        assert!(Query::parse("red || blue").matches("blue sky"));
        assert!(!Query::parse("red || blue").matches("green"));
        assert!(Query::parse("").matches("x"));
    }

    #[test]
    fn fuzzy_matching() {
        let s = |q: &str, t: &str| Query::parse(q).fuzzy_score(t);
        // Exact hits cost nothing.
        assert_eq!(s("tiger", "a tiger here"), Some(0));
        // One typo, a swap, a missing letter, a typo in a prefix.
        assert_eq!(s("tigre", "a tiger here"), Some(1));
        assert_eq!(s("tgier", "a tiger here"), Some(1));
        assert_eq!(s("tigr", "a tiger here"), Some(1));
        assert_eq!(s("photgraph", "photography_01.jpg"), Some(1));
        assert_eq!(s("landscpae mountian", "mountain landscape"), Some(2));
        // Accents fold.
        assert_eq!(s("creme", "crème brûlée"), Some(1));
        // Too many edits, short terms, digits and phrases stay exact.
        assert_eq!(s("tgr", "a tiger here"), None);
        assert_eq!(s("tigr", "tiramisu"), None);
        assert_eq!(s("img_2035", "img_2034"), None);
        assert_eq!(s("\"black dgo\"", "black dog"), None);
        // Every AND term must match; one OR member is enough; NOT is exact.
        assert_eq!(s("tigre zebar", "a tiger here"), None);
        assert_eq!(s("zebar || tigre", "a tiger here"), Some(1));
        assert_eq!(s("tigre -here", "a tiger here"), None);
    }

    #[test]
    fn prefix_distance_rules() {
        let c = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(prefix_distance(&c("abcd"), &c("abcd"), 1), Some(0));
        assert_eq!(prefix_distance(&c("abdc"), &c("abcd"), 1), Some(1));
        assert_eq!(prefix_distance(&c("abcx"), &c("abcdefgh"), 1), Some(1));
        assert_eq!(prefix_distance(&c("abcde"), &c("abcd"), 1), Some(1));
        assert_eq!(prefix_distance(&c("abcdef"), &c("abcd"), 1), None);
        assert_eq!(prefix_distance(&c("xycd"), &c("abcd"), 1), None);
    }

    #[test]
    fn set_filter_or_quirk() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let f = SetFilter { includes: v(&["a"]), excludes: v(&["b"]), ..Default::default() };
        assert!(f.matches(&v(&["a", "b"]))); // included wins under OR
        assert!(f.matches(&v(&["c"]))); // lacks the excluded one
        assert!(!f.matches(&v(&["b"])));
        let f = SetFilter { logic: Logic::And, includes: v(&["a", "b"]), ..Default::default() };
        assert!(f.matches(&v(&["a", "b", "c"])));
        assert!(!f.matches(&v(&["a"])));
        let f = SetFilter { logic: Logic::Equal, includes: v(&["a", "b"]), ..Default::default() };
        assert!(!f.matches(&v(&["a", "b", "c"])));
        let f = SetFilter { empty: true, ..Default::default() };
        assert!(f.matches(&[]));
        assert!(!f.matches(&v(&["a"])));
    }

    #[test]
    fn excluding_the_empty_list() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let f = SetFilter { exclude_empty: true, ..Default::default() };
        assert!(f.is_active());
        assert!(f.matches(&v(&["F1"])) && !f.matches(&[]));
        // Under OR, lacking every excluded value is enough, as in Eagle.
        let f = SetFilter { includes: v(&["F1"]), exclude_empty: true, ..Default::default() };
        assert!(f.matches(&v(&["F1"])) && !f.matches(&[]) && f.matches(&v(&["F2"])));
        let f = SetFilter { logic: Logic::And, excludes: v(&["F2"]), exclude_empty: true, ..Default::default() };
        assert!(f.matches(&v(&["F1"])) && !f.matches(&[]) && !f.matches(&v(&["F2"])));
    }

    #[test]
    fn color_math_matches_references() {
        // Reference values from the CIEDE2000 paper (Sharma et al., pair 1).
        let d = delta_e00([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485]);
        assert!((d - 2.0425).abs() < 1e-4, "{d}");
        assert_eq!(lab([255, 255, 255]), [100.0, 0.0, 0.0]);
        assert_eq!(lab([255, 0, 0]), [53.0, 80.0, 67.0]);
    }

    fn item(extra: serde_json::Value) -> Item {
        let mut v = serde_json::json!({
            "id": "A", "name": "a", "size": 1, "btime": 0, "mtime": 0, "ext": "jpg", "tags": [], "folders": [],
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": 0, "height": 1, "width": 1,
            "lastModified": 0, "palettes": []
        });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_marked_primary_colour_matches_past_the_dominant_ones() {
        let palettes = |marked: bool| {
            item(serde_json::json!({"palettes": [
                {"color": [250, 250, 250], "ratio": 60},
                {"color": [20, 20, 20], "ratio": 30},
                {"color": [200, 30, 30], "ratio": 5, "marked": marked},
            ]}))
        };
        let red = ColorQuery::new([210, 40, 40]);
        assert_eq!(red.distance(&palettes(false)), None);
        let d = red.distance(&palettes(true)).expect("the marked red matches");
        assert!(d > 0.0 && d < 30.0, "{d}");
        // Far colours stay out even when marked; the strict accuracy
        // doesn't apply to the marked colour.
        assert_eq!(ColorQuery::new([30, 30, 220]).distance(&palettes(true)), None);
        assert!(ColorQuery { rgb: [210, 40, 40], accuracy: 5.0 }.distance(&palettes(true)).is_some());
    }

    const NOW: i64 = 1_760_000_000_000;
    const MIDNIGHT: i64 = NOW - 10 * 3_600_000;

    fn dates(presets: &[DatePreset]) -> DateFilter {
        DateFilter {
            presets: presets.iter().copied().collect(),
            clock: Some(Clock { now: NOW, today_start: MIDNIGHT }),
            ..Default::default()
        }
    }

    #[test]
    fn date_presets() {
        let at = |t: i64| t as u64;
        let today = dates(&[DatePreset::Today]);
        assert!(today.matches(at(MIDNIGHT + 1)));
        assert!(!today.matches(at(MIDNIGHT)));
        let yesterday = dates(&[DatePreset::Yesterday]);
        assert!(yesterday.matches(at(MIDNIGHT - 1)));
        assert!(!yesterday.matches(at(MIDNIGHT - DAY_MS)));
        assert!(!yesterday.matches(at(MIDNIGHT + 1)));
        let week = dates(&[DatePreset::Last7Days]);
        assert!(week.matches(at(NOW - 7 * DAY_MS + 1)));
        assert!(!week.matches(at(NOW - 7 * DAY_MS)));
        for (p, n) in [(DatePreset::Last30Days, 30), (DatePreset::Last90Days, 90), (DatePreset::Last365Days, 365)] {
            let f = dates(&[p]);
            assert!(f.matches(at(NOW - n * DAY_MS + 1)) && !f.matches(at(NOW - n * DAY_MS)), "{p:?}");
        }
        // Presets are OR-ed.
        let either = dates(&[DatePreset::Today, DatePreset::Yesterday]);
        assert!(either.matches(at(MIDNIGHT - 5)) && either.matches(at(MIDNIGHT + 5)));
        assert!(!either.matches(at(MIDNIGHT - 2 * DAY_MS)));
    }

    #[test]
    fn date_range_and_months() {
        let a = local_day_start(2024, 3, 1).unwrap();
        let b = local_day_start(2024, 3, 10).unwrap();
        let f = DateFilter { range: Some((a, b)), ..Default::default() };
        assert!(f.matches(a));
        assert!(!f.matches(a - 1));
        assert!(f.matches(b + DAY_MS as u64)); // the last day is whole, end inclusive
        assert!(!f.matches(b + DAY_MS as u64 + 1));
        // An unset end makes Eagle ignore the range.
        assert!(!DateFilter { range: Some((a, 0)), ..Default::default() }.matches(a));

        let mid_march = local_day_start(2024, 3, 15).unwrap() + 12 * 3_600_000;
        let f = DateFilter { months: [(2024, 3)].into(), ..Default::default() };
        assert!(f.matches(mid_march));
        assert!(!f.matches(local_day_start(2024, 4, 15).unwrap()));
        // Months and presets must both pass.
        let f = DateFilter { months: [(2024, 3)].into(), ..dates(&[DatePreset::Today]) };
        assert!(!f.matches(mid_march));
    }

    #[test]
    fn date_fields_in_filter() {
        let i = item(serde_json::json!({"modificationTime": NOW - 1000, "mtime": 0, "btime": 5}));
        assert_eq!(DateField::Modified.value(&i), (NOW - 1000) as u64);
        assert_eq!(DateField::Created.value(&i), 5);
        let f = Filter { imported: dates(&[DatePreset::Today]), ..Default::default() };
        assert!(f.check(&i).is_some());
        let f = Filter { created: dates(&[DatePreset::Today]), ..Default::default() };
        assert!(f.check(&i).is_none());
        let f = Filter { modified: dates(&[DatePreset::Today]), ..Default::default() };
        assert!(f.check(&i).is_some(), "mtime 0 falls back to the import time");
        let counts = month_counts([&i, &i], DateField::Imported);
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0].1, 2);
    }

    #[test]
    fn duration_rule() {
        let clip = item(serde_json::json!({"ext": "mp3", "duration": 90.7}));
        let still = item(serde_json::json!({}));
        let f = |min: Option<f64>, max: Option<f64>| Filter { duration: (min, max), ..Default::default() };
        assert!(f(Some(90.0), None).check(&clip).is_some());
        assert!(f(Some(90.9), None).check(&clip).is_some(), "bounds are truncated");
        assert!(f(None, Some(DurationUnit::Minutes.to_seconds(1.5))).check(&clip).is_none());
        assert!(f(None, Some(DurationUnit::Minutes.to_seconds(2.0))).check(&clip).is_some());
        assert!(f(Some(0.0), None).check(&still).is_none());
        assert_eq!(DurationUnit::Hours.to_seconds(1.0), 3600.0);
        assert_eq!(duration_range([&clip, &still]), Some((90.7, 90.7)));
    }

    #[test]
    fn bpm_rule() {
        let song = item(serde_json::json!({"ext": "mp3", "bpm": 119.985}));
        let still = item(serde_json::json!({}));
        let f = |min: Option<f64>, max: Option<f64>| Filter { bpm: (min, max), ..Default::default() };
        assert!(f(Some(100.0), Some(120.0)).check(&song).is_some());
        assert!(f(None, Some(119.9)).check(&song).is_none(), "119.9 truncates to 119");
        assert!(f(Some(120.7), None).check(&song).is_none(), "120.7 truncates to 120");
        assert!(f(Some(0.0), None).check(&still).is_none(), "no bpm fails an active range");
        assert!(Filter::default().check(&still).is_some());
        assert!(f(Some(1.0), None).check(&item(serde_json::json!({"bpm": 0}))).is_none());
    }

    #[test]
    fn camera_rule_and_options() {
        let nikon = item(serde_json::json!({"ext": "nef", "rawMetas": {"camera": "Nikon D850"}}));
        let canon = item(serde_json::json!({"ext": "cr2", "rawMetas": {"camera": "Canon EOS R5"}}));
        let lower = item(serde_json::json!({"ext": "cr2", "rawMetas": {"camera": "canon eos r5"}}));
        let none = item(serde_json::json!({}));
        let f = Filter { cameras: ["Canon EOS R5".to_string()].into(), ..Default::default() };
        assert!(f.check(&canon).is_some());
        assert!(f.check(&lower).is_none(), "exact, case-sensitive");
        assert!(f.check(&nikon).is_none() && f.check(&none).is_none());
        assert_eq!(distinct_cameras([&nikon, &canon, &lower, &none, &canon]), ["Canon EOS R5", "Nikon D850", "canon eos r5"]);
    }

    #[test]
    fn font_rule_and_options() {
        let font = item(serde_json::json!({"ext": "ttf", "fontMetas": {
            "fontFamily": {"en": "Inter"}, "postScriptName": {"en": "Inter-Regular", "zh": "x"}}}));
        let other = item(serde_json::json!({"ext": "otf", "fontMetas": {"fontFamily": {"en": "Arial"}, "postScriptName": {"en": "ArialMT"}}}));
        let photo = item(serde_json::json!({}));
        assert_eq!(font_key(&font).as_deref(), Some("Inter-Regular_.ttf"));
        let installed: HashSet<String> = [font_file_key("Inter-Regular.TTF"), font_file_key("Inter-Regular.ttf")].into();
        assert!(installed.contains("Inter-Regular_.ttf"));
        assert!(installed.contains("Inter-Regular.TTF_.ttf"), "Node's basename keeps a differently cased extension");
        let on = Filter { font: FontFilter::Activated(installed.clone()), ..Default::default() };
        let off = Filter { font: FontFilter::Deactivated(installed), ..Default::default() };
        assert!(on.check(&font).is_some() && off.check(&font).is_none());
        assert!(on.check(&other).is_none() && off.check(&other).is_some());
        assert!(on.check(&photo).is_none() && off.check(&photo).is_none());
        assert_eq!(distinct_font_families([&other, &font, &photo, &font]), ["Arial", "Inter"]);
    }

    #[test]
    fn fonts_in_either_folder_count_as_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let (eagle, falcon) = (tmp.path().join("EagleApp"), tmp.path().join("Falcon"));
        std::fs::create_dir_all(&eagle).unwrap();
        std::fs::create_dir_all(&falcon).unwrap();
        std::fs::write(eagle.join("ArialMT.otf"), b"").unwrap();
        std::fs::write(falcon.join("Inter-Regular.ttf"), b"").unwrap();
        let keys = installed_font_keys_in(&[eagle.clone(), falcon, tmp.path().join("missing")]);
        assert!(keys.contains("ArialMT_.otf") && keys.contains("Inter-Regular_.ttf"));
        assert_eq!(keys.len(), 2, "a missing folder adds nothing");
        assert_eq!(installed_font_keys_in(&[eagle]).len(), 1);
        assert!(installed_font_keys_in(&[]).is_empty());
        if cfg!(target_os = "macos") {
            let dirs = font_dirs();
            assert!(dirs[0].ends_with("Library/Fonts/EagleApp") && dirs[1].ends_with("Library/Fonts/Falcon"));
        }
    }

    #[test]
    fn search_text_parts_and_scope() {
        let mut folders = FolderIndex::new();
        folders.insert("F".into(), ("Trips".into(), "Summer holidays".into()));
        let photo = item(serde_json::json!({
            "name": "Beach", "url": "https://example.org", "annotation": "sunset note", "tags": ["sand"], "folders": ["F"],
            "comments": [{"id": "c", "annotation": "Look at the Boat", "x": 0}],
            "rawMetas": {"camera": "Nikon Z6"}
        }));
        let text = search_text(&photo, &folders, 5);
        for part in ["beach", ".jpg", "example.org", "sunset note", "look at the boat", "nikon z6", "sand", "trips", "summer holidays"] {
            assert!(text.contains(part), "{part} in {text}");
        }
        assert!(!search_text(&photo, &folders, 1).contains("example.org"), "urls need two characters");
        let off = SearchScope { comments: false, tags: false, folder_desc: false, name: false, ..Default::default() };
        let text = search_text_in(&photo, &folders, 5, &off);
        assert!(!text.contains("boat") && !text.contains("sand") && !text.contains("summer") && !text.contains("beach"));
        assert!(text.contains("trips") && text.contains("nikon z6"), "folder names stay, and the camera is always searched");

        let doc = item(serde_json::json!({"ext": "txt", "text": "Chapter One"}));
        let none = SearchScope { name: false, folder_name: false, folder_desc: false, ext: false, tags: false, url: false, comments: false, note: false };
        assert!(search_text_in(&doc, &folders, 3, &none).contains("chapter one"), "text content is always searched");

        let font = item(serde_json::json!({"ext": "ttf", "fontMetas": {
            "fullName": {"en": "Inter Bold"}, "postScriptName": {"en": "Inter-Bold"}}}));
        assert!(search_text(&font, &folders, 2).contains("inter bold"));
        assert!(!search_text(&font, &folders, 2).contains("inter-bold"), "the metadata needs three characters");
        assert!(search_text(&font, &folders, 3).contains("inter-bold"));
        let zh = item(serde_json::json!({"ext": "otf", "fontMetas": {"fullName": {"en": "Song", "zh": "宋体"}}}));
        let t = search_text(&zh, &folders, 3);
        assert!(t.contains("宋体") && !t.contains("song"), "the zh name wins; no postScriptName, no JSON");
    }

    #[test]
    fn comment_rule() {
        let noted = item(serde_json::json!({"comments": [{"annotation": "Red Car"}, {"annotation": "blue sky"}]}));
        let bare = item(serde_json::json!({}));
        let empty = item(serde_json::json!({"comments": []}));
        let f = |t: TextFilter| Filter { comments: t, ..Default::default() };
        assert!(f(TextFilter::Has).check(&noted).is_some() && f(TextFilter::Has).check(&bare).is_none());
        assert!(f(TextFilter::No).check(&bare).is_some() && f(TextFilter::No).check(&empty).is_some());
        assert!(f(TextFilter::No).check(&noted).is_none());
        let kw = |w: &[&str]| f(TextFilter::Keywords(w.iter().map(|s| s.to_string()).collect()));
        assert!(kw(&["car", "SKY"]).check(&noted).is_some(), "each keyword may sit in a different comment");
        assert!(kw(&["car", "boat"]).check(&noted).is_none());
        assert!(kw(&["car"]).check(&bare).is_none());
    }

    #[test]
    fn ratio_and_custom_shapes() {
        let sized = |w: u64, h: u64| item(serde_json::json!({"width": w, "height": h}));
        let f = |shapes: &[Shape], custom: Option<(f64, f64)>| Filter { shapes: shapes.iter().copied().collect(), custom_shape: custom, ..Default::default() };
        assert!(f(&[Shape::Ratio16x9], None).check(&sized(1920, 1080)).is_some());
        assert!(f(&[Shape::Ratio16x9], None).check(&sized(1366, 768)).is_none(), "exact ratios only");
        assert!(f(&[Shape::Ratio9x16], None).check(&sized(1080, 1920)).is_some());
        assert!(f(&[Shape::Ratio4x3, Shape::Ratio3x4], None).check(&sized(600, 800)).is_some());
        assert!(f(&[], Some((2.0, 1.0))).check(&sized(400, 200)).is_some());
        assert!(f(&[], Some((2.0, 1.0))).check(&sized(400, 201)).is_none());
        assert!(f(&[], Some((0.0, 3.0))).check(&sized(1, 7)).is_some(), "an unset side lets everything through");
        assert!(f(&[Shape::Square], Some((2.0, 1.0))).check(&sized(5, 5)).is_some(), "OR-ed with the other shapes");

        let items = [sized(1920, 1080), sized(3000, 1000), sized(800, 600), sized(10, 10), item(serde_json::json!({"ext": "mp3", "width": 9, "height": 3})), sized(0, 0)];
        let c = shape_counts(items.iter());
        assert_eq!(c.get(&Shape::Landscape), Some(&2));
        assert_eq!(c.get(&Shape::PanoramicLandscape), Some(&1));
        assert_eq!(c.get(&Shape::Square), Some(&1));
        assert_eq!((c.get(&Shape::Ratio16x9), c.get(&Shape::Ratio4x3)), (Some(&1), Some(&1)));
        assert_eq!(c.get(&Shape::Portrait), None);
    }

    #[test]
    fn type_rule_follows_eagles_order() {
        let of = |ext: &str| item(serde_json::json!({"ext": ext}));
        let site = |m: &str| item(serde_json::json!({"ext": "url", "medium": m}));
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(type_included(&v(&["png"]), &of("png")));
        assert!(type_included(&v(&["video"]), &of("mp4")) && !type_included(&v(&["video"]), &of("png")));
        assert!(!type_included(&v(&["video", "audio"]), &of("mp3")), "video decides alone, as in Eagle");
        assert!(type_included(&v(&["video", "mp3"]), &of("mp3")), "an extension still wins");
        assert!(type_included(&v(&["audio", "font"]), &of("wav")) && !type_included(&v(&["audio", "font"]), &of("ttf")));
        assert!(type_included(&v(&["font"]), &of("woff2")));
        assert!(type_included(&v(&["youtube"]), &site("youtube")) && !type_included(&v(&["youtube"]), &site("vimeo")));
        assert!(type_included(&v(&["url"]), &site("vimeo")), "the url extension itself matches");
        assert!(type_included(&v(&["word"]), &of("docx")));
        assert!(type_excluded(&v(&["video"]), &of("mov")) && !type_excluded(&v(&["video"]), &of("png")));
        assert!(type_excluded(&v(&["bilibili"]), &site("bilibili")) && !type_excluded(&v(&["bilibili"]), &of("url")));
        assert!(type_excluded(&v(&["url"]), &of("url")) && type_excluded(&v(&["url"]), &site("youtube")));

        let items = [of("png"), of("png"), of("mp4"), site("youtube"), of("url"), of("TTF")];
        let c = type_counts(items.iter());
        assert_eq!((c["png"], c["mp4"], c["video"], c["youtube"], c["url"]), (2, 1, 1, 1, 1));
        assert!(!c.contains_key("audio"));
        assert_eq!(type_options(items.iter()), ["TTF", "mp4", "png", "url", "font", "video", "audio", "youtube", "vimeo", "bilibili"]);
    }

    #[test]
    fn string_scores() {
        assert_eq!(string_score("hello", "hello"), 1.0);
        assert_eq!(string_score("hello", ""), 0.0);
        assert_eq!(string_score("hello", "hz"), 0.0);
        // "he": 0.7+0.1 each -> running 1.6; 0.5*(1.6/5 + 1.6/2) = 0.56; +0.15.
        assert!((string_score("hello", "he") - 0.71).abs() < 1e-9);
        // An acronym letter after a space scores like two consecutive ones.
        assert!(string_score("hello world", "hw") > string_score("hello xworld", "hw"));
        assert!(string_score("Hello", "h") < string_score("hello", "h"), "same case scores more");
    }

    #[test]
    fn names_rank_best_first() {
        let names = ["Travel Photos", "photos", "Phones", "Café Art", "misc"];
        let r = rank_names(names, "photos");
        assert_eq!(r[0], 1, "the exact name first");
        assert!(r.contains(&0) && !r.contains(&2) && !r.contains(&4));
        assert_eq!(rank_names(names, "cafe"), [3], "accents fold");
        assert_eq!(rank_names(names, "travel ph")[0], 0, "spaces in the keyword are dropped");
        assert!(rank_names(names, "").is_empty());
    }

    #[test]
    fn duration_sort_counts_missing_as_zero() {
        let clip = |name: &str, d: Option<f64>| {
            let mut v = serde_json::json!({"name": name});
            if let Some(d) = d {
                v["duration"] = d.into();
            }
            item(v)
        };
        let items = [clip("a", Some(30.9)), clip("b", None), clip("c", Some(5.5)), clip("d", Some(30.1))];
        let mut refs: Vec<&Item> = items.iter().collect();
        sort(&mut refs, OrderBy::Duration, true);
        assert_eq!(refs.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["b", "c", "a", "d"], "whole seconds, stable");
        sort(&mut refs, OrderBy::Duration, false);
        assert_eq!(refs.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["d", "a", "c", "b"]);
        let mut refs: Vec<&Item> = items.iter().collect();
        sort(&mut refs, OrderBy::Random, true);
        assert_eq!(refs[0].name, "a", "random leaves the order to the caller");
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("img2", "img10"), Ordering::Less);
        assert_eq!(natural_cmp("Apple", "apple"), Ordering::Equal);
        assert_eq!(natural_cmp("b", "A"), Ordering::Greater);
    }
}
