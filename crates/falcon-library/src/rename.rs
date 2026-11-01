//! Batch renaming, worked out the way Eagle's Rename dialog does: either a
//! name template with counters, dates and the original name, or a find
//! and replace (plain text or `/regex/flags`), then an optional change of
//! case. Only names are computed here; callers save them.

use chrono::{DateTime, Local, TimeZone};
use regex::{Regex, RegexBuilder};

/// How the new names are made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// A template. Tokens (any case): `%N` to `%NNNNNNNNN` a counter padded
    /// to that many digits, `*` the original name, `%D` / `%DD` / `%DDD` /
    /// `%DHM` / `%DHMS` today's date, `%M…` the modification date and
    /// `%B…` the creation date in the same five forms, `%T` the tags.
    /// Each token is replaced once, at its first place, as in Eagle.
    Format { template: String, start: u32 },
    /// Every `find` becomes `with`. A `find` written `/pattern/flags` is a
    /// regular expression: only its first match is replaced unless the
    /// flags hold `g`, and `i` ignores case.
    Replace { find: String, with: String },
}

/// The case change applied to every new name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextCase {
    #[default]
    Keep,
    Upper,
    Lower,
    /// Each word with a capital first letter and the rest lower case.
    Capitalize,
}

/// What a template can draw on for one thing being renamed.
#[derive(Debug, Clone, Default)]
pub struct Subject<'a> {
    pub name: &'a str,
    /// Modification and creation times, ms since the epoch.
    pub modified: u64,
    pub created: u64,
    pub tags: &'a [String],
    /// Names of the folders it is in (empty for a folder).
    pub folders: &'a [String],
}

/// A batch rename: the method and the case change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub method: Method,
    pub case: TextCase,
}

/// Longest name Eagle's dialog produces.
const MAX_CHARS: usize = 255;

impl Rename {
    /// Whether the dialog can run: a template, or something to find.
    pub fn is_ready(&self) -> bool {
        match &self.method {
            Method::Format { template, .. } => !template.is_empty(),
            Method::Replace { find, .. } => !find.is_empty(),
        }
    }

    /// The new name of the `index`th subject (from 0), with `now` for the
    /// date tokens. An empty result keeps the original name. Names are cut
    /// to 255 characters.
    pub fn apply(&self, s: &Subject, index: usize, now: DateTime<Local>) -> String {
        let name = match &self.method {
            Method::Format { template, start } => format_name(template, s, *start as usize + index, now),
            Method::Replace { find, with } => replace_name(s.name, find, with),
        };
        // Eagle falls back to the original before changing the case. (A
        // replace that empties the name would leave it empty in Eagle.)
        let name = if name.is_empty() { s.name.to_string() } else { name };
        change_case(&name, self.case).chars().take(MAX_CHARS).collect()
    }
}

/// `/pattern/flags` as a regex and whether it replaces every match.
fn parse_regex(find: &str) -> Option<(Regex, bool)> {
    let rest = find.strip_prefix('/')?;
    let end = rest.rfind('/')?;
    let (pattern, flags) = (&rest[..end], &rest[end + 1..]);
    if pattern.is_empty() || !flags.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    let re = RegexBuilder::new(pattern).case_insensitive(flags.contains('i')).multi_line(flags.contains('m')).build().ok()?;
    Some((re, flags.contains('g')))
}

fn replace_name(name: &str, find: &str, with: &str) -> String {
    match parse_regex(find) {
        Some((re, true)) => re.replace_all(name, with).into_owned(),
        Some((re, false)) => re.replace(name, with).into_owned(),
        None if find.is_empty() => name.to_string(),
        None => name.replace(find, with),
    }
}

/// Replaces the first case-insensitive `%` + `letters` (with at least one
/// more of the last letter when `more`), consuming the longest run in
/// that case.
fn replace_token(text: &mut String, letters: &str, more: bool, with: &str) {
    let lower = text.to_ascii_lowercase();
    let want = format!("%{}", letters.to_ascii_lowercase());
    let mut from = 0;
    while let Some(hit) = lower[from..].find(&want) {
        let at = from + hit;
        let mut end = at + want.len();
        if more {
            let last = want.as_bytes()[want.len() - 1];
            let extra = lower.as_bytes()[end..].iter().take_while(|&&b| b == last).count();
            if extra == 0 {
                from = at + 1;
                continue;
            }
            end += extra;
        }
        text.replace_range(at..end, with);
        return;
    }
}

fn format_name(template: &str, s: &Subject, n: usize, now: DateTime<Local>) -> String {
    let mut out = template.to_string();
    let pad = |w: usize| format!("{n:0w$}");
    // Counters, longest first; nine or more letters pad to nine.
    let counters: [(&str, bool, usize); 10] = [
        ("NNNNNNNN", true, 9),
        ("NNNNNNNNN", false, 8),
        ("NNNNNNNN", false, 8),
        ("NNNNNNN", false, 7),
        ("NNNNNN", false, 6),
        ("NNNNN", false, 5),
        ("NNNN", false, 4),
        ("NNN", false, 3),
        ("NN", false, 2),
        ("N", false, 1),
    ];
    // Eagle runs each counter pattern twice (upper and lower case); with
    // case-insensitive matching that replaces up to two tokens of a length.
    for (letters, more, width) in counters {
        for _ in 0..2 {
            replace_token(&mut out, letters, more, &pad(width));
        }
    }
    let local = |ms: u64| Local.timestamp_millis_opt(ms as i64).single().unwrap_or(now);
    let dates = [("D", now, '-'), ("M", local(s.modified), '_'), ("B", local(s.created), '_')];
    for (letter, t, sep) in dates {
        let ymd = format!("%Y{sep}%m{sep}%d");
        let forms = [
            (format!("{letter}{letter}"), true, t.format("%b %-d, %Y").to_string()),
            (format!("{letter}{letter}{letter}"), false, t.format("%b %-d, %Y").to_string()),
            (format!("{letter}{letter}"), false, t.format(&format!("%m{sep}%d")).to_string()),
            (format!("{letter}HMS"), false, t.format(&format!("{ymd} %H_%M_%S")).to_string()),
            (format!("{letter}HM"), false, t.format(&format!("{ymd} %H_%M")).to_string()),
            (letter.to_string(), false, t.format(&ymd).to_string()),
        ];
        for _ in 0..2 {
            for (letters, more, value) in &forms {
                replace_token(&mut out, letters, *more, value);
            }
        }
    }
    if let Some(at) = out.find('*') {
        out.replace_range(at..at + 1, s.name);
    }
    // Eagle looks for an upper-case %T or %F before replacing either case.
    for (token, list) in [("%T", s.tags), ("%F", s.folders)] {
        if out.contains(token) {
            let mut sorted = list.to_vec();
            sorted.sort();
            replace_token(&mut out, &token[1..], false, &sorted.join("-"));
        }
    }
    out
}

fn change_case(name: &str, case: TextCase) -> String {
    match case {
        TextCase::Keep => name.to_string(),
        TextCase::Upper => name.to_uppercase(),
        TextCase::Lower => name.to_lowercase(),
        TextCase::Capitalize => {
            // A word starts at an ASCII letter, digit or underscore and runs
            // to the next space, as JavaScript's `\w\S*`.
            let mut out = String::with_capacity(name.len());
            let mut in_word = false;
            for c in name.chars() {
                if c.is_whitespace() {
                    in_word = false;
                    out.push(c);
                } else if in_word {
                    out.extend(c.to_lowercase());
                } else if c.is_ascii_alphanumeric() || c == '_' {
                    in_word = true;
                    out.push(c.to_ascii_uppercase());
                } else {
                    out.push(c);
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 3, 7, 9, 5, 2).unwrap()
    }

    fn fmt(template: &str, start: u32) -> Rename {
        Rename { method: Method::Format { template: template.into(), start }, case: TextCase::Keep }
    }

    fn subject(name: &str) -> Subject<'_> {
        Subject { name, ..Default::default() }
    }

    #[test]
    fn counters_pad_and_count_from_the_start() {
        let r = fmt("Shot %NNN", 1);
        assert_eq!(r.apply(&subject("a"), 0, now()), "Shot 001");
        assert_eq!(r.apply(&subject("a"), 41, now()), "Shot 042");
        assert_eq!(fmt("%n-%NN", 7).apply(&subject("a"), 2, now()), "9-09");
        assert_eq!(fmt("x%NNNNNNNNNNNN", 0).apply(&subject("a"), 5, now()), "x000000005");
        // Past the padding the number is written in full.
        assert_eq!(fmt("%N", 1).apply(&subject("a"), 11, now()), "12");
    }

    #[test]
    fn original_name_dates_and_tags() {
        let tags = vec!["sky".to_string(), "blue".to_string()];
        let modified = Local.with_ymd_and_hms(2024, 12, 31, 23, 59, 58).unwrap().timestamp_millis() as u64;
        let s = Subject { name: "Logo", modified, created: modified, tags: &tags, folders: &[] };
        assert_eq!(fmt("* (%D)", 1).apply(&s, 0, now()), "Logo (2026-03-07)");
        assert_eq!(fmt("%dd %DHM", 1).apply(&s, 0, now()), "03-07 2026-03-07 09_05");
        assert_eq!(fmt("%DHMS", 1).apply(&s, 0, now()), "2026-03-07 09_05_02");
        assert_eq!(fmt("%DDD", 1).apply(&s, 0, now()), "Mar 7, 2026");
        assert_eq!(fmt("%M / %bb / %MHMS", 1).apply(&s, 0, now()), "2024_12_31 / 12_31 / 2024_12_31 23_59_58");
        assert_eq!(fmt("* %T", 1).apply(&s, 0, now()), "Logo blue-sky");
        // A lower-case %t is left as typed, as in Eagle.
        assert_eq!(fmt("* %t", 1).apply(&s, 0, now()), "Logo %t");
        // A folder has no folders of its own.
        assert_eq!(fmt("*%F", 1).apply(&s, 0, now()), "Logo");
        // Only the first * is the name.
        assert_eq!(fmt("**", 1).apply(&s, 0, now()), "Logo*");
        // Tokens in the original name are not expanded.
        assert_eq!(fmt("* %N", 1).apply(&subject("%D"), 0, now()), "%D 1");
    }

    #[test]
    fn find_and_replace_plain_and_regex() {
        let r = |find: &str, with: &str| Rename { method: Method::Replace { find: find.into(), with: with.into() }, case: TextCase::Keep };
        assert_eq!(r("a", "o").apply(&subject("banana"), 0, now()), "bonono");
        assert_eq!(r("/a/", "o").apply(&subject("banana"), 0, now()), "bonana");
        assert_eq!(r("/A/gi", "o").apply(&subject("banana"), 0, now()), "bonono");
        assert_eq!(r("/(\\d+)/g", "#$1").apply(&subject("v1 v22"), 0, now()), "v#1 v#22");
        // Not a valid regex: plain text.
        assert_eq!(r("/(/", "x").apply(&subject("a/(/b"), 0, now()), "axb");
        // Replacing everything away keeps the name.
        assert_eq!(r("banana", "").apply(&subject("banana"), 0, now()), "banana");
        assert!(!r("", "x").is_ready());
        assert!(!fmt("", 1).is_ready());
    }

    #[test]
    fn text_case_and_length() {
        let cased = |case| Rename { method: Method::Format { template: "*".into(), start: 1 }, case };
        let s = subject("hello wORLD (draft) élan");
        assert_eq!(cased(TextCase::Upper).apply(&s, 0, now()), "HELLO WORLD (DRAFT) ÉLAN");
        assert_eq!(cased(TextCase::Lower).apply(&s, 0, now()), "hello world (draft) élan");
        assert_eq!(cased(TextCase::Capitalize).apply(&s, 0, now()), "Hello World (Draft) éLan");
        let long = "x".repeat(300);
        assert_eq!(cased(TextCase::Keep).apply(&subject(&long), 0, now()).chars().count(), 255);
    }
}
