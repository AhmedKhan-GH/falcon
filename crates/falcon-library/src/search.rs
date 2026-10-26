//! Keyword search, filter-panel rules, colour matching and sorting, following
//! Eagle's behaviour (see the Search and filters chapter of the docs).
//!
//! Like Eagle, everything is a linear pass over the items; there is no index.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

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
}

/// The lower-cased text Eagle searches for an item: name, `.ext`, url (for
/// queries of two or more characters), notes, tags, and the name and
/// description of each folder the item is in.
pub fn search_text(item: &Item, folders: &FolderIndex, query_len: usize) -> String {
    let mut s = String::with_capacity(128);
    let mut push = |x: &str| {
        s.push(' ');
        s.push_str(x);
    };
    push(&item.name);
    push(&format!(".{}", item.ext));
    if query_len >= 2 {
        push(&item.url);
    }
    push(&item.annotation);
    for t in &item.tags {
        push(t);
    }
    for f in &item.folders {
        if let Some((name, desc)) = folders.get(f) {
            push(name);
            push(desc);
        }
    }
    s.to_lowercase()
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
}

impl SetFilter {
    pub fn is_active(&self) -> bool {
        !self.includes.is_empty() || !self.excludes.is_empty() || self.empty
    }

    /// Eagle's semantics, including its OR quirk: under OR an item with any
    /// included value passes even if it also has an excluded one.
    pub fn matches(&self, values: &[String]) -> bool {
        if !self.is_active() {
            return true;
        }
        let has = |v: &String| values.contains(v);
        let base = match self.logic {
            Logic::Or => {
                if self.includes.iter().any(has) {
                    true
                } else if !self.excludes.is_empty() {
                    !self.excludes.iter().any(has)
                } else {
                    false
                }
            }
            Logic::And | Logic::Equal => {
                let ok = self.includes.iter().all(has) && !self.excludes.iter().any(has);
                let ok = ok && (self.logic != Logic::Equal || values.len() == self.includes.len());
                ok && (!self.includes.is_empty() || !self.excludes.is_empty())
            }
        };
        match (self.empty, self.logic) {
            (true, Logic::Or) => base || values.is_empty(),
            (true, _) if self.includes.is_empty() && self.excludes.is_empty() => values.is_empty(),
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
}

impl Shape {
    fn matches(self, w: u64, h: u64) -> bool {
        let (w, h) = (w as f64, h as f64);
        match self {
            Shape::Landscape => w > h,
            Shape::Portrait => h > w,
            Shape::Square => w == h,
            Shape::PanoramicLandscape => w > h && w / h >= 2.5,
            Shape::PanoramicPortrait => w < h && h / w >= 2.5,
        }
    }
}

/// Eagle's file-type groups for the type filter.
pub fn type_group(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "ts" | "3gp" | "360" | "afx" | "vap" | "eva" | "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" | "wmv"
        | "mpg" | "mts" | "flv" | "m2ts" | "f4v" => "video",
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" => "audio",
        "ttf" | "otf" | "ttc" | "woff" | "woff2" => "font",
        "ppt" | "pptx" | "potx" => "powerpoint",
        "doc" | "docx" => "word",
        "xls" | "xlsx" => "excel",
        _ => return None,
    })
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
    pub url: TextFilter,
    pub note: TextFilter,
    pub color: Option<ColorQuery>,
    pub grayscale: bool,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        *self != Filter::default()
    }

    /// Whether `item` passes every rule; for a colour rule also returns the
    /// distance Eagle sorts by.
    pub fn check(&self, item: &Item) -> Option<f64> {
        let ext = item.ext.as_str();
        let typed = |list: &[String]| list.iter().any(|t| t == ext || type_group(ext) == Some(t.as_str()));
        if !self.types.is_empty() && !typed(&self.types) {
            return None;
        }
        if typed(&self.exclude_types) {
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
        if !self.shapes.is_empty() && !self.shapes.iter().any(|s| s.matches(item.width, item.height)) {
            return None;
        }
        if !self.url.matches(&item.url) || !self.note.matches(&item.annotation) {
            return None;
        }
        if self.grayscale && !is_grayscale(item) {
            return None;
        }
        if !self.tags.matches(&item.tags) || !self.folders.matches(&item.folders) {
            return None;
        }
        match &self.color {
            Some(c) => c.distance(item),
            None => Some(0.0),
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

    /// Eagle's match on the two dominant palette colours; `Some(distance)`
    /// when the item matches.
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
        None
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
}

impl OrderBy {
    pub const ALL: [OrderBy; 9] = [
        OrderBy::Import,
        OrderBy::Btime,
        OrderBy::Mtime,
        OrderBy::Name,
        OrderBy::Ext,
        OrderBy::FileSize,
        OrderBy::Resolution,
        OrderBy::Rating,
        OrderBy::Tags,
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
    });
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
    fn color_math_matches_references() {
        // Reference values from the CIEDE2000 paper (Sharma et al., pair 1).
        let d = delta_e00([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485]);
        assert!((d - 2.0425).abs() < 1e-4, "{d}");
        assert_eq!(lab([255, 255, 255]), [100.0, 0.0, 0.0]);
        assert_eq!(lab([255, 0, 0]), [53.0, 80.0, 67.0]);
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("img2", "img10"), Ordering::Less);
        assert_eq!(natural_cmp("Apple", "apple"), Ordering::Equal);
        assert_eq!(natural_cmp("b", "A"), Ordering::Greater);
    }
}
