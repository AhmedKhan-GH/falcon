//! Date, duration, camera and font filters: labels, parsing, summaries and counts.

use std::collections::{HashMap, HashSet};
use falcon_library::model::Item;
use falcon_library::search::{self, DateFilter, DatePreset, DurationUnit};

/// Menu label of a date preset.
pub fn preset_label(p: DatePreset) -> &'static str {
    match p {
        DatePreset::Today => "Today",
        DatePreset::Yesterday => "Yesterday",
        DatePreset::Last7Days => "Last 7 Days",
        DatePreset::Last30Days => "Last 30 Days",
        DatePreset::Last90Days => "Last 90 Days",
        DatePreset::Last365Days => "Last 365 Days",
    }
}

/// A month of the month list: `2026-10`.
pub fn month_label((y, m): (i32, u32)) -> String {
    format!("{y}-{m:02}")
}

/// A day typed as `2026-10-31` (or `2026/10/31`), as `(year, month, day)`.
pub fn parse_day(text: &str) -> Option<(i32, u32, u32)> {
    let mut parts = text.trim().split(['-', '/', '.']);
    let y = parts.next()?.trim().parse().ok()?;
    let m = parts.next()?.trim().parse().ok()?;
    let d = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(y, m, d).map(|_| (y, m, d))
}

/// A range end (local midnight, ms) as `YYYY-MM-DD`, the form
/// [`day_range`] reads; empty for 0, an end Eagle leaves unset.
pub fn day_text(t: u64) -> String {
    use chrono::TimeZone;
    match i64::try_from(t).ok().filter(|t| *t > 0).and_then(|t| chrono::Local.timestamp_millis_opt(t).single()) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        None => String::new(),
    }
}

/// A local date and time for `ms` since 1970 ("2024-03-05 14:07"); empty
/// for a time before 1970 or out of range.
pub fn date_time_text(ms: i64) -> String {
    use chrono::TimeZone;
    match Some(ms).filter(|t| *t >= 0).and_then(|t| chrono::Local.timestamp_millis_opt(t).single()) {
        Some(d) => d.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

/// The calendar range of a date rule from the two day fields: both must be
/// valid days, swapped if typed the wrong way round.
pub fn day_range(from: &str, to: &str) -> Option<(u64, u64)> {
    let start = |t: &str| parse_day(t).and_then(|(y, m, d)| search::local_day_start(y, m, d));
    let (a, b) = (start(from)?, start(to)?);
    Some((a.min(b), a.max(b)))
}

/// The chip text of one date rule: the single preset, month or range when
/// only one is set, otherwise how many are set.
pub fn date_summary(f: &DateFilter) -> Option<String> {
    let n = f.presets.len() + f.months.len() + usize::from(f.range.is_some());
    match n {
        0 => None,
        1 => Some(if let Some(p) = DatePreset::ALL.into_iter().find(|p| f.presets.contains(p)) {
            preset_label(p).to_string()
        } else if let Some(m) = f.months.iter().next() {
            month_label(*m)
        } else {
            "Range".to_string()
        }),
        n => Some(n.to_string()),
    }
}

/// A duration as a clock: `0:05`, `1:23`, `1:02:03`. Fractions are dropped.
pub fn format_duration(secs: f64) -> String {
    let s = if secs.is_finite() { secs.max(0.0) as u64 } else { 0 };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// A duration typed as a clock (`1:23`, `1:02:03`) in seconds; a plain
/// number is read in `unit`.
pub fn parse_duration(text: &str, unit: DurationUnit) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if t.contains(':') {
        let parts: Vec<&str> = t.split(':').collect();
        if parts.len() > 3 {
            return None;
        }
        let mut total = 0.0;
        for p in &parts {
            let v: f64 = p.trim().parse().ok().filter(|v: &f64| *v >= 0.0)?;
            total = total * 60.0 + v;
        }
        return Some(total);
    }
    t.parse::<f64>().ok().filter(|v| *v >= 0.0 && v.is_finite()).map(|v| unit.to_seconds(v))
}

/// The duration chip text: `0:30 – 2:00`, `≥ 0:30`, `≤ 2:00`.
pub fn duration_summary(range: (Option<f64>, Option<f64>)) -> Option<String> {
    match range {
        (None, None) => None,
        (Some(a), None) => Some(format!("≥ {}", format_duration(a))),
        (None, Some(b)) => Some(format!("≤ {}", format_duration(b))),
        (Some(a), Some(b)) => Some(format!("{} – {}", format_duration(a), format_duration(b))),
    }
}

/// How many live items each camera has, in [`search::distinct_cameras`]
/// order.
pub fn camera_counts(items: &[Item]) -> Vec<(String, usize)> {
    let live = || items.iter().filter(|i| !i.is_deleted);
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for c in live().filter_map(search::camera) {
        *counts.entry(c).or_default() += 1;
    }
    search::distinct_cameras(live())
        .into_iter()
        .map(|c| {
            let n = counts.get(c.as_str()).copied().unwrap_or(0);
            (c, n)
        })
        .collect()
}

/// Live fonts that are (first) and are not (second) activated, given the
/// installed keys.
pub fn font_counts(items: &[Item], installed: &HashSet<String>) -> (usize, usize) {
    items.iter().filter(|i| !i.is_deleted).filter_map(search::font_key).fold((0, 0), |(a, d), k| {
        if installed.contains(&k) { (a + 1, d) } else { (a, d + 1) }
    })
}

#[cfg(test)]
mod filter_media_tests {
    use super::*;
    use serde_json::json;

    fn item(id: &str, extra: serde_json::Value) -> Item {
        let mut v = json!({
            "id": id, "name": id, "size": 1, "btime": 0, "mtime": 0, "ext": "ttf",
            "tags": [], "folders": [], "isDeleted": false, "url": "",
            "annotation": "", "modificationTime": 0, "height": 0, "width": 0,
            "lastModified": 0, "palettes": []
        });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        Item::from_json_str(&v.to_string()).unwrap()
    }

    #[test]
    fn durations_format_and_parse() {
        assert_eq!(format_duration(5.7), "0:05");
        assert_eq!(format_duration(83.0), "1:23");
        assert_eq!(format_duration(3723.0), "1:02:03");
        assert_eq!(format_duration(f64::NAN), "0:00");
        assert_eq!(parse_duration("1:23", DurationUnit::Seconds), Some(83.0));
        assert_eq!(parse_duration("1:02:03", DurationUnit::Minutes), Some(3723.0));
        assert_eq!(parse_duration("90", DurationUnit::Seconds), Some(90.0));
        assert_eq!(parse_duration("1.5", DurationUnit::Minutes), Some(90.0));
        assert_eq!(parse_duration("2", DurationUnit::Hours), Some(7200.0));
        assert_eq!(parse_duration(" ", DurationUnit::Seconds), None);
        assert_eq!(parse_duration("-3", DurationUnit::Seconds), None);
        assert_eq!(parse_duration("1:x", DurationUnit::Seconds), None);
        assert_eq!(parse_duration("1:2:3:4", DurationUnit::Seconds), None);
        assert_eq!(duration_summary((Some(30.0), Some(120.0))).as_deref(), Some("0:30 – 2:00"));
        assert_eq!(duration_summary((Some(30.0), None)).as_deref(), Some("≥ 0:30"));
        assert_eq!(duration_summary((None, None)), None);
    }

    #[test]
    fn days_and_date_summaries() {
        assert_eq!(parse_day("2026-10-31"), Some((2026, 10, 31)));
        assert_eq!(parse_day("2026/2/3"), Some((2026, 2, 3)));
        assert_eq!(parse_day("2026-02-30"), None);
        assert_eq!(parse_day("2026-10"), None);
        let (a, b) = day_range("2026-10-31", "2026-10-01").unwrap();
        assert!(a < b);
        assert_eq!(Some(a), search::local_day_start(2026, 10, 1));
        assert_eq!(day_range("2026-10-01", ""), None);
        assert_eq!(day_text(a), "2026-10-01");
        assert_eq!(day_text(0), "");
        let mut f = DateFilter::default();
        assert_eq!(date_summary(&f), None);
        f.presets.insert(DatePreset::Last7Days);
        assert_eq!(date_summary(&f).as_deref(), Some("Last 7 Days"));
        f.presets.clear();
        f.months.insert((2026, 3));
        assert_eq!(date_summary(&f).as_deref(), Some("2026-03"));
        f.range = Some((1, 2));
        assert_eq!(date_summary(&f).as_deref(), Some("2"));
        assert_eq!(month_label((2026, 3)), "2026-03");
    }

    #[test]
    fn camera_and_font_counts() {
        let items = vec![
            item("a", json!({"ext": "nef", "rawMetas": {"camera": "Nikon D850"}})),
            item("b", json!({"ext": "nef", "rawMetas": {"camera": "Nikon D850"}})),
            item("c", json!({"ext": "arw", "rawMetas": {"camera": "Sony A7"}})),
            item("d", json!({"ext": "arw", "isDeleted": true, "rawMetas": {"camera": "Gone"}})),
            item("e", json!({"fontMetas": {"postScriptName": {"en": "Inter-Regular"}}})),
            item("f", json!({"fontMetas": {"postScriptName": {"en": "Inter-Bold"}}})),
        ];
        assert_eq!(camera_counts(&items), vec![("Nikon D850".to_string(), 2), ("Sony A7".to_string(), 1)]);
        let installed: HashSet<String> = ["Inter-Bold_.ttf".to_string()].into();
        assert_eq!(font_counts(&items, &installed), (1, 1));
        assert_eq!(font_counts(&items, &HashSet::new()), (0, 2));
    }
}
