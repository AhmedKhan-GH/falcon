//! The Notification Center's list: the toasts and notices shown, newest
//! last in memory. A falcon addition (Eagle has no such panel). Pure, so
//! the rules can be tested; drawing is `ui/dialogs/notice_history.rs`.

use serde::{Deserialize, Serialize};

/// The most entries kept; older ones are dropped.
pub const CAP: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Info,
    Error,
    /// A dialog notice (`Alert`).
    Notice,
}

/// One note as it was shown. Only the text is kept, not its Undo button or
/// folder link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub text: String,
    pub kind: Kind,
    /// Seconds since the Unix epoch, when it was last shown.
    pub at: i64,
    /// How many times in a row it was shown.
    pub count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    /// Oldest first.
    entries: Vec<Entry>,
}

impl History {
    /// Adds a note. The same text as the latest entry counts up that entry
    /// (and refreshes its time) instead of adding another.
    pub fn record(&mut self, text: &str, kind: Kind, at: i64) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.entries.last_mut()
            && last.text == text
            && last.kind == kind
        {
            last.count = last.count.saturating_add(1);
            last.at = at;
            return;
        }
        self.entries.push(Entry { text: text.to_string(), kind, at, count: 1 });
        self.trim();
    }

    /// Drops the oldest entries past the cap. Also run after loading.
    pub fn trim(&mut self) {
        if self.entries.len() > CAP {
            let extra = self.entries.len() - CAP;
            self.entries.drain(..extra);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Newest first.
    pub fn newest_first(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev()
    }
}

/// Now, in seconds since the Unix epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The time shown beside an entry: "14:05" for today, "2026-10-31 14:05"
/// otherwise. `offset` is the local time zone's offset from UTC in seconds.
pub fn format_time(at: i64, now: i64, offset: i32) -> String {
    use chrono::DateTime;
    let local = |t: i64| DateTime::from_timestamp(t + i64::from(offset), 0);
    let (Some(then), Some(today)) = (local(at), local(now)) else { return String::new() };
    if then.date_naive() == today.date_naive() { then.format("%H:%M").to_string() } else { then.format("%Y-%m-%d %H:%M").to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(h: &History) -> Vec<&str> {
        h.newest_first().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn newest_comes_first() {
        let mut h = History::default();
        h.record("a", Kind::Info, 1);
        h.record("b", Kind::Error, 2);
        h.record("c", Kind::Notice, 3);
        assert_eq!(texts(&h), ["c", "b", "a"]);
    }

    #[test]
    fn same_text_in_a_row_collapses_with_a_count() {
        let mut h = History::default();
        h.record("Copied", Kind::Info, 1);
        h.record("Copied", Kind::Info, 5);
        h.record("Copied", Kind::Info, 9);
        assert_eq!(h.len(), 1);
        let e = h.newest_first().next().unwrap();
        assert_eq!((e.count, e.at), (3, 9));
    }

    #[test]
    fn same_text_not_in_a_row_stays_separate() {
        let mut h = History::default();
        h.record("a", Kind::Info, 1);
        h.record("b", Kind::Info, 2);
        h.record("a", Kind::Info, 3);
        assert_eq!(texts(&h), ["a", "b", "a"]);
    }

    #[test]
    fn same_text_of_another_kind_is_another_entry() {
        let mut h = History::default();
        h.record("a", Kind::Info, 1);
        h.record("a", Kind::Error, 2);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn empty_text_is_not_recorded() {
        let mut h = History::default();
        h.record("", Kind::Info, 1);
        assert!(h.is_empty());
    }

    #[test]
    fn cap_keeps_the_newest() {
        let mut h = History::default();
        for i in 0..CAP + 25 {
            h.record(&format!("n{i}"), Kind::Info, i as i64);
        }
        assert_eq!(h.len(), CAP);
        assert_eq!(h.newest_first().next().unwrap().text, format!("n{}", CAP + 24));
        assert_eq!(h.newest_first().last().unwrap().text, "n25");
    }

    #[test]
    fn clear_empties() {
        let mut h = History::default();
        h.record("a", Kind::Info, 1);
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn round_trips_through_serde() {
        let mut h = History::default();
        h.record("a", Kind::Info, 1);
        h.record("a", Kind::Info, 2);
        h.record("b", Kind::Error, 3);
        let ron = serde_json::to_string(&h).unwrap();
        let back: History = serde_json::from_str(&ron).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn over_cap_state_is_trimmed_on_load() {
        let h = History { entries: (0..CAP + 10).map(|i| Entry { text: format!("n{i}"), kind: Kind::Info, at: 0, count: 1 }).collect() };
        let ron = serde_json::to_string(&h).unwrap();
        let mut back: History = serde_json::from_str(&ron).unwrap();
        assert_eq!(back.len(), CAP + 10);
        back.trim();
        assert_eq!(back.len(), CAP);
        assert_eq!(back.newest_first().last().unwrap().text, "n10");
    }

    #[test]
    fn times_show_the_clock_today_and_the_date_before() {
        // 2026-11-02 12:00:00 UTC.
        let now = 1_793_620_800;
        assert_eq!(format_time(now - 3600, now, 0), "11:00");
        assert_eq!(format_time(now - 86_400, now, 0), "2026-11-01 12:00");
        // Five hours ahead of UTC: 17:00 local.
        assert_eq!(format_time(now, now, 5 * 3600), "17:00");
        // 22:00 UTC is already the next day at +5, so 12:00 UTC is yesterday.
        assert_eq!(format_time(now, now + 10 * 3600, 5 * 3600), "2026-11-02 17:00");
    }
}
