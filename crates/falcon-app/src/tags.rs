//! The tag picker's decisions, after Eagle's tag select panel: which tags
//! it lists and in which sections, how typed text finds and ranks them,
//! where the arrow keys go, and the tags it recommends. `ui/inspector/tag_picker.rs`
//! draws it.

use std::collections::{HashMap, HashSet};

use falcon_library::model::{Item, TagGroup};

use crate::locale::{tr, tr_key};

/// How many recently used tags the Recent section shows.
pub const RECENT_SHOWN: usize = 12;
/// How many tags one paste or one typed entry may add.
pub const MAX_ADDED: usize = 200;

/// Splits typed or pasted text into tags at commas (also the full-width
/// and ideographic ones), semicolons and line breaks, as Eagle does. Parts
/// are trimmed; empty ones are dropped.
pub fn parse_tags(text: &str) -> Vec<String> {
    text.split(['，', ',', ';', '、', '\n', '\r']).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// One tag the picker can list.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    /// Items outside the Trash that have it.
    pub count: usize,
    /// Indices of the groups it is filed in, in the library's order; the
    /// first one decides its section and colour.
    pub groups: Vec<usize>,
    pub starred: bool,
    /// Its place among the recently used tags (0 is the latest).
    pub recent: Option<usize>,
    /// Recommended for the items picked.
    pub suggested: bool,
}

impl Entry {
    pub fn group(&self) -> Option<usize> {
        self.groups.first().copied()
    }
}

/// The tags the picker lists: every tag in use, tags on the picked items,
/// starred tags no item has, and recommended tags, each once.
pub fn entries(counts: &[(String, usize)], groups: &[TagGroup], history: &[String], starred: &[String], suggestions: &[String], selected: &[String]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut add = |name: &str, count: usize, out: &mut Vec<Entry>| {
        if !index.contains_key(name) {
            index.insert(name.to_string(), out.len());
            out.push(Entry { name: name.to_string(), count, groups: Vec::new(), starred: false, recent: None, suggested: false });
        }
    };
    for (t, n) in counts {
        add(t, *n, &mut out);
    }
    for t in selected.iter().chain(starred).chain(suggestions) {
        add(t, 0, &mut out);
    }
    for (gi, g) in groups.iter().enumerate() {
        for t in &g.tags {
            if let Some(&i) = index.get(t)
                && !out[i].groups.contains(&gi)
            {
                out[i].groups.push(gi);
            }
        }
    }
    // Recent: the latest tags still in use (or picked), as Eagle counts them.
    let mut rank = 0;
    for h in history {
        if rank == RECENT_SHOWN {
            break;
        }
        if let Some(&i) = index.get(h)
            && out[i].recent.is_none()
            && (out[i].count > 0 || selected.contains(h))
        {
            out[i].recent = Some(rank);
            rank += 1;
        }
    }
    for t in starred {
        if let Some(&i) = index.get(t) {
            out[i].starred = true;
        }
    }
    for t in suggestions {
        if let Some(&i) = index.get(t) {
            out[i].suggested = true;
        }
    }
    out
}

/// Which of the optional sections show (the picker's settings menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Show {
    pub starred: bool,
    pub recent: bool,
    pub suggested: bool,
}

impl Default for Show {
    fn default() -> Self {
        Show { starred: true, recent: true, suggested: true }
    }
}

/// The tag picker's settings, after Eagle's tag panel: which optional
/// sections show, tag counts, the column width (Eagle's 100, 120, 140 or
/// 160 px, 140 unless changed), one tag per row, and the group sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PickerPrefs {
    pub show: Show,
    pub counts: bool,
    pub column: u16,
    pub list: bool,
    pub sidebar: bool,
}

/// The column widths the picker offers.
pub const COLUMNS: [(u16, &str); 4] = [(100, tr_key("Extra Small")), (120, tr_key("Small")), (140, tr_key("Medium")), (160, tr_key("Large"))];

impl Default for PickerPrefs {
    fn default() -> Self {
        PickerPrefs { show: Show::default(), counts: true, column: 140, list: false, sidebar: false }
    }
}

/// How many columns of `column` px fit in `width` (2 px gaps), at least one.
pub fn columns(width: f32, column: u16, list: bool) -> usize {
    if list {
        return 1;
    }
    (((width + 2.0) / (f32::from(column.clamp(80, 400)) + 2.0)).floor() as usize).max(1)
}

/// The picker's sidebar choice: every tag, one group's, or those in none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Side {
    #[default]
    All,
    Group(usize),
    Ungrouped,
}

impl Side {
    fn admits(self, e: &Entry) -> bool {
        match self {
            Side::All => true,
            Side::Group(g) => e.groups.contains(&g),
            Side::Ungrouped => e.groups.is_empty(),
        }
    }

    /// Tab moves through All, each group, then Ungrouped (⇧Tab back).
    pub fn cycle(self, groups: usize, back: bool) -> Side {
        let order: Vec<Side> = std::iter::once(Side::All).chain((0..groups).map(Side::Group)).chain(std::iter::once(Side::Ungrouped)).collect();
        let at = order.iter().position(|s| *s == self).unwrap_or(0);
        let n = order.len();
        order[if back { (at + n - 1) % n } else { (at + 1) % n }]
    }
}

/// A section of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Starred,
    Recent,
    Suggested,
    /// A tag group, by its index.
    Group(usize),
    /// Tags in no group.
    Others,
    /// While typing: one list of matches.
    Results,
}

/// A cell of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    /// Create (and add) the typed tags.
    Create(String),
    /// An existing tag, by its index in the entries.
    Tag(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub kind: Kind,
    pub picks: Vec<Pick>,
}

/// What the list shows for `keyword`: without one, Starred, Recent,
/// Recommended, each group and the rest; with one, the matching tags
/// ranked (see [`rank`]), led by a Create cell unless the text names an
/// existing tag exactly. Also returns each sidebar entry's count of tags
/// left by the keyword, keyed by [`Side`].
pub fn sections(entries: &[Entry], groups: &[TagGroup], keyword: &str, side: Side, show: Show) -> (Vec<Section>, HashMap<Side, usize>) {
    let keyword_t = keyword.trim();
    // The order Eagle sorts the list in before splitting it up.
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| {
        let e = &entries[i];
        (
            !(show.starred && e.starred),
            if show.recent { e.recent.unwrap_or(usize::MAX) } else { usize::MAX },
            !(show.suggested && e.suggested),
            e.group().unwrap_or(usize::MAX),
            e.name.clone(),
        )
    });
    // Recommended tags only show while that section is on.
    order.retain(|&i| show.suggested || !entries[i].suggested || entries[i].count > 0 || entries[i].starred);
    if !keyword_t.is_empty() {
        let wanted = fold(keyword_t).replace(' ', "");
        let lower = keyword_t.to_lowercase();
        order.retain(|&i| {
            let e = &entries[i];
            subsequence(&wanted, &fold(&e.name))
                || (lower.chars().count() >= 2 && e.group().and_then(|g| groups.get(g)).is_some_and(|g| g.name.to_lowercase().contains(&lower)))
        });
    }
    let mut counts: HashMap<Side, usize> = HashMap::new();
    counts.insert(Side::All, order.len());
    for &i in &order {
        let e = &entries[i];
        if e.groups.is_empty() {
            *counts.entry(Side::Ungrouped).or_default() += 1;
        }
        for &g in &e.groups {
            *counts.entry(Side::Group(g)).or_default() += 1;
        }
    }
    order.retain(|&i| side.admits(&entries[i]));

    // Each tag goes to one section: starred, else recent, else recommended,
    // else its first group, else the rest.
    let kind_of = |e: &Entry| {
        if show.starred && e.starred {
            Kind::Starred
        } else if show.recent && e.recent.is_some() {
            Kind::Recent
        } else if show.suggested && e.suggested {
            Kind::Suggested
        } else if let Some(g) = e.group().filter(|g| *g < groups.len()) {
            Kind::Group(g)
        } else {
            Kind::Others
        }
    };
    let mut kinds: Vec<Kind> = vec![Kind::Starred, Kind::Recent, Kind::Suggested];
    kinds.extend((0..groups.len()).map(Kind::Group));
    kinds.push(Kind::Others);
    let mut out: Vec<Section> = Vec::new();
    for k in kinds {
        let picks: Vec<Pick> = order.iter().filter(|&&i| kind_of(&entries[i]) == k).map(|&i| Pick::Tag(i)).collect();
        if !picks.is_empty() {
            out.push(Section { kind: k, picks });
        }
    }
    if keyword_t.is_empty() {
        return (out, counts);
    }
    let mut picks: Vec<Pick> = out.into_iter().flat_map(|s| s.picks).collect();
    picks = rank(entries, picks, keyword_t);
    if !entries.iter().any(|e| e.name == keyword_t && (e.count > 0 || e.starred)) {
        picks.insert(0, Pick::Create(keyword_t.to_string()));
    }
    (vec![Section { kind: Kind::Results, picks }], counts)
}

/// Eagle's ranking of matches for typed text: the exact name first, then
/// names containing the text (where it starts earlier first), then the
/// rest by edit distance (names sharing no letter with the text last),
/// existing tags before merely recommended ones. Ties keep their order.
pub fn rank(entries: &[Entry], mut picks: Vec<Pick>, keyword: &str) -> Vec<Pick> {
    let kw = keyword.to_lowercase();
    let key = |p: &Pick| match p {
        Pick::Create(_) => (0u8, 0usize, 0usize, false),
        Pick::Tag(i) => {
            let e = &entries[*i];
            let name = e.name.to_lowercase();
            let suggested_only = e.suggested && e.count == 0;
            if name == kw {
                (1, 0, 0, suggested_only)
            } else if let Some(at) = name.find(&kw) {
                (2, name[..at].chars().count(), 0, suggested_only)
            } else {
                (3, 0, distance(&kw, &name), suggested_only)
            }
        }
    };
    picks.sort_by_cached_key(key);
    picks
}

/// Levenshtein distance, or `usize::MAX` when the strings share no
/// character (Eagle ranks those last).
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let set: HashSet<char> = a.iter().copied().collect();
    if !b.iter().any(|c| set.contains(c)) {
        return usize::MAX;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] { prev[j - 1] } else { 1 + prev[j - 1].min(prev[j]).min(cur[j - 1]) };
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Lower case with common Latin accents folded, so `cafe` finds `Café`.
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
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
        })
        .collect()
}

/// Whether the letters of `needle` appear in `hay` in order (Eagle's fuzzy
/// match: `mchn` finds `machine`).
pub fn subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

/// The cell highlighted after the list changes: with typed text, the first
/// tag whose name starts with it, else the first cell; with none, nothing.
pub fn default_highlight(entries: &[Entry], sections: &[Section], keyword: &str) -> Option<(usize, usize)> {
    let keyword = keyword.trim().to_lowercase();
    if keyword.is_empty() {
        return None;
    }
    let s = sections.first()?;
    let first_tag = s.picks.iter().position(|p| matches!(p, Pick::Tag(_)));
    if let Some(at) = first_tag
        && let Pick::Tag(i) = &s.picks[at]
        && entries[*i].name.to_lowercase().starts_with(&keyword)
    {
        return Some((0, at));
    }
    (!s.picks.is_empty()).then_some((0, 0))
}

/// An arrow key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Where an arrow key moves the highlight in a list of sections laid out
/// `cols` cells wide; `lens` holds each section's cell count (0 for a
/// collapsed one, which is skipped). Up and Down keep the column and cross
/// into the neighbouring section; Left and Right step through the cells.
/// With nothing highlighted, Down, Right and Left take the first cell.
pub fn step(lens: &[usize], cols: usize, cur: Option<(usize, usize)>, dir: Dir) -> Option<(usize, usize)> {
    let cols = cols.max(1);
    let first = lens.iter().position(|&n| n > 0);
    let Some((s, i)) = cur.filter(|(s, i)| lens.get(*s).is_some_and(|n| i < n)) else {
        return match dir {
            Dir::Up => cur,
            _ => first.map(|s| (s, 0)),
        };
    };
    let next_sec = (s + 1..lens.len()).find(|&k| lens[k] > 0);
    let prev_sec = (0..s).rev().find(|&k| lens[k] > 0);
    let (row, col) = (i / cols, i % cols);
    let len = lens[s];
    Some(match dir {
        Dir::Right if i + 1 < len => (s, i + 1),
        Dir::Right => next_sec.map_or((s, i), |k| (k, 0)),
        Dir::Left if i > 0 => (s, i - 1),
        Dir::Left => prev_sec.map_or((s, i), |k| (k, lens[k] - 1)),
        Dir::Down => {
            let below = (row + 1) * cols + col;
            if below < len {
                (s, below)
            } else if row + 1 < len.div_ceil(cols) {
                (s, len - 1)
            } else {
                next_sec.map_or((s, i), |k| (k, if col < lens[k] { col } else { 0 }))
            }
        }
        Dir::Up if row > 0 => (s, (row - 1) * cols + col),
        Dir::Up => prev_sec.map_or((s, i), |k| {
            let last_row = (lens[k] - 1) / cols;
            let at = last_row * cols + col;
            (k, if at < lens[k] { at } else { lens[k] - 1 })
        }),
    })
}

/// Words Eagle won't recommend.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "has", "have", "in", "into", "is", "it", "its", "of", "on", "or", "that", "the",
    "this", "to", "was", "were", "will", "with", "copy", "new", "untitled",
];

/// Recommended tags for the picked items, after Eagle: words from the one
/// item's name and from the names of the folders the items are in (and
/// their parents), at least two characters long with at most one digit,
/// capitalised, without stop words or tags the items all have already.
/// Existing tags come first. Nothing for more than 500 items.
pub fn suggestions(items: &[&Item], folders: &[falcon_library::model::Folder], known: &HashSet<&str>, common: &[String]) -> Vec<String> {
    if items.is_empty() || items.len() > 500 {
        return Vec::new();
    }
    let mut text = String::new();
    if let [one] = items {
        text.push_str(&one.name);
    }
    for item in items {
        for fid in &item.folders {
            for name in folder_chain(folders, fid) {
                text.push(' ');
                text.push_str(name);
            }
        }
    }
    let common: HashSet<String> = common.iter().map(|t| t.to_lowercase()).collect();
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if word.chars().count() < 2 || word.split(|c: char| c.is_ascii_digit()).count() >= 3 || word.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let lower = word.to_lowercase();
        if STOPWORDS.contains(&lower.as_str()) || common.contains(&lower) || !seen.insert(lower) {
            continue;
        }
        let mut c = word.chars();
        let cap: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
        out.push(cap);
    }
    // Existing tags first, the rest in the order found.
    out.sort_by_key(|t| !known.contains(t.as_str()));
    out
}

/// The names of a folder's parents, from the top, then its own.
fn folder_chain<'a>(roots: &'a [falcon_library::model::Folder], id: &str) -> Vec<&'a str> {
    fn walk<'a>(fs: &'a [falcon_library::model::Folder], id: &str, path: &mut Vec<&'a str>) -> bool {
        for f in fs {
            path.push(&f.name);
            if f.id == id || walk(&f.children, id, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    walk(roots, id, &mut path);
    path
}

/// The tags every one of `items` has, in the first item's order (Eagle's
/// multi-selection inspector shows these).
pub fn common_tags(items: &[&Item]) -> Vec<String> {
    let Some(first) = items.first() else { return Vec::new() };
    first.tags.iter().filter(|t| items.iter().all(|i| i.tags.contains(t))).cloned().collect()
}

// ---------------------------------------------------------------------------
// The All Tags page

/// A click on a tag of the All Tags page, after Eagle's tag manager: a
/// plain click picks only that tag, ⌘ adds or drops it, ⇧ adds the run
/// from the last clicked tag to it in page order (keeping the rest).
/// Returns the picked tags and the new anchor.
pub fn select_click(picked: &[String], anchor: Option<&str>, order: &[String], tag: &str, shift: bool, cmd: bool) -> (Vec<String>, Option<String>) {
    let mut out = picked.to_vec();
    if shift && let (Some(a), Some(b)) = (anchor.and_then(|a| order.iter().position(|t| t == a)), order.iter().position(|t| t == tag)) {
        for t in &order[a.min(b)..=a.max(b)] {
            if !out.contains(t) {
                out.push(t.clone());
            }
        }
    } else if cmd || shift {
        if let Some(at) = out.iter().position(|t| t == tag) {
            out.remove(at);
        } else {
            out.push(tag.to_string());
        }
    } else {
        out = vec![tag.to_string()];
    }
    (out, Some(tag.to_string()))
}

/// How many tags Eagle merges at once: two to five.
pub fn can_merge(n: usize) -> bool {
    (2..=5).contains(&n)
}

/// How the All Tags page lists tags: its sort and whether it groups by tag
/// group. Eagle keeps both in app-wide settings (`TAGS_SORT_BY`,
/// `TAGS_SORT_INCREASE`, `TAGS_GROUP_BY`), so they follow the user across
/// libraries and launches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TagLook {
    pub sort: TagSort,
    pub by_group: bool,
}

/// The All Tags page's sort (Eagle's tag list menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TagSort {
    #[default]
    NameUp,
    NameDown,
    CountDown,
    CountUp,
}

impl TagSort {
    pub const ALL: [TagSort; 4] = [TagSort::NameUp, TagSort::NameDown, TagSort::CountDown, TagSort::CountUp];

    pub fn label(self) -> &'static str {
        match self {
            TagSort::NameUp => tr("A > Z"),
            TagSort::NameDown => tr("Z > A"),
            TagSort::CountDown => tr("Count, High to Low"),
            TagSort::CountUp => tr("Count, Low to High"),
        }
    }

    /// Sorts `(tag, count)` pairs; names compare without case, then as
    /// written, and break ties between equal counts.
    pub fn apply(self, tags: &mut [(String, usize)]) {
        let name = |a: &(String, usize), b: &(String, usize)| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then_with(|| a.0.cmp(&b.0));
        match self {
            TagSort::NameUp => tags.sort_by(name),
            TagSort::NameDown => tags.sort_by(|a, b| name(b, a)),
            TagSort::CountDown => tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| name(a, b))),
            TagSort::CountUp => tags.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| name(a, b))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn group(name: &str, tags: &[&str]) -> TagGroup {
        TagGroup { id: name.to_uppercase(), name: name.into(), tags: s(tags), color: None, description: None, extra: Default::default() }
    }

    fn counts(v: &[(&str, usize)]) -> Vec<(String, usize)> {
        v.iter().map(|(t, n)| (t.to_string(), *n)).collect()
    }

    fn names(entries: &[Entry], picks: &[Pick]) -> Vec<String> {
        picks
            .iter()
            .map(|p| match p {
                Pick::Create(t) => format!("+{t}"),
                Pick::Tag(i) => entries[*i].name.clone(),
            })
            .collect()
    }

    #[test]
    fn parses_like_eagle() {
        assert_eq!(parse_tags("a, b，c;d、e\nf,, ,g"), s(&["a", "b", "c", "d", "e", "f", "g"]));
        assert!(parse_tags(" , ").is_empty());
    }

    #[test]
    fn sections_without_a_keyword() {
        let groups = [group("Mood", &["calm", "loud"]), group("Style", &["flat", "calm"])];
        let c = counts(&[("apple", 2), ("calm", 1), ("flat", 3), ("loud", 1), ("zebra", 1), ("unused", 0)]);
        let e = entries(&c, &groups, &s(&["zebra", "gone", "loud"]), &s(&["flat", "ghost"]), &s(&["Sky"]), &[]);
        let (secs, counts) = sections(&e, &groups, "", Side::All, Show::default());
        let got: Vec<(Kind, Vec<String>)> = secs.iter().map(|x| (x.kind, names(&e, &x.picks))).collect();
        assert_eq!(
            got,
            [
                (Kind::Starred, s(&["flat", "ghost"])),
                // History order; "gone" is no longer on any item.
                (Kind::Recent, s(&["zebra", "loud"])),
                (Kind::Suggested, s(&["Sky"])),
                (Kind::Group(0), s(&["calm"])),
                (Kind::Others, s(&["apple", "unused"])),
            ]
        );
        // calm counts for both of its groups.
        assert_eq!(counts[&Side::Group(1)], 2);
        assert_eq!(counts[&Side::Ungrouped], 5);

        // Hidden sections fall back to the groups.
        let off = Show { starred: false, recent: false, suggested: false };
        let (secs, _) = sections(&e, &groups, "", Side::All, off);
        let got: Vec<(Kind, Vec<String>)> = secs.iter().map(|x| (x.kind, names(&e, &x.picks))).collect();
        assert_eq!(got, [(Kind::Group(0), s(&["calm", "loud"])), (Kind::Group(1), s(&["flat"])), (Kind::Others, s(&["apple", "ghost", "unused", "zebra"]))]);

        // The sidebar narrows to one group.
        let (secs, _) = sections(&e, &groups, "", Side::Group(1), off);
        assert_eq!(names(&e, &secs[0].picks), s(&["calm"]));
        assert_eq!(names(&e, &secs[1].picks), s(&["flat"]));
    }

    #[test]
    fn recent_shows_twelve_tags_in_use() {
        let c: Vec<(String, usize)> = (0..20).map(|n| (format!("t{n:02}"), 1)).collect();
        let history: Vec<String> = (0..20).rev().map(|n| format!("t{n:02}")).collect();
        let e = entries(&c, &[], &history, &[], &[], &[]);
        let recent: Vec<&str> = {
            let mut v: Vec<&Entry> = e.iter().filter(|x| x.recent.is_some()).collect();
            v.sort_by_key(|x| x.recent);
            v.iter().map(|x| x.name.as_str()).collect()
        };
        assert_eq!(recent.len(), RECENT_SHOWN);
        assert_eq!(recent[0], "t19");
    }

    #[test]
    fn typing_ranks_exact_then_contains_then_fuzzy() {
        let c = counts(&[("cartoon", 1), ("Cat", 1), ("scatter", 1), ("concat", 1), ("act", 1), ("dog", 1), ("c-a-t", 1)]);
        let e = entries(&c, &[], &[], &[], &[], &[]);
        let (secs, _) = sections(&e, &[], "cat", Side::All, Show::default());
        // "cartoon" holds c,a,t in order but not "cat"; "act" doesn't.
        // Names are matched exactly for Create, so "Cat" doesn't stop it.
        assert_eq!(names(&e, &secs[0].picks), s(&["+cat", "Cat", "scatter", "concat", "c-a-t", "cartoon"]));
        assert_eq!(default_highlight(&e, &secs, "cat"), Some((0, 1)));

        // A new word gets a Create cell first; the highlight skips it to a
        // tag starting with the text.
        let (secs, _) = sections(&e, &[], "ca", Side::All, Show::default());
        assert_eq!(names(&e, &secs[0].picks)[..3], s(&["+ca", "Cat", "cartoon"]));
        assert_eq!(default_highlight(&e, &secs, "ca"), Some((0, 1)));
        let (secs, _) = sections(&e, &[], "zz", Side::All, Show::default());
        assert_eq!(names(&e, &secs[0].picks), s(&["+zz"]));
        assert_eq!(default_highlight(&e, &secs, "zz"), Some((0, 0)));
        assert_eq!(default_highlight(&e, &secs, ""), None);
    }

    #[test]
    fn typing_matches_accents_spaces_and_group_names() {
        let groups = [group("Colours", &["teal"])];
        let c = counts(&[("Café", 1), ("machine learning", 1), ("teal", 1)]);
        let e = entries(&c, &groups, &[], &[], &[], &[]);
        let find = |k: &str| names(&e, &sections(&e, &groups, k, Side::All, Show::default()).0[0].picks);
        assert!(find("cafe").contains(&"Café".to_string()));
        assert!(find("mach learn").contains(&"machine learning".to_string()));
        assert!(find("colo").contains(&"teal".to_string()), "a group's name finds its tags");
    }

    #[test]
    fn distance_is_levenshtein_or_unrelated() {
        assert_eq!(distance("kitten", "sitting"), 3);
        assert_eq!(distance("abc", "xyz"), usize::MAX);
    }

    #[test]
    fn arrows_walk_the_grid_and_cross_sections() {
        // Sections of 5, 0 (collapsed) and 3 cells, two columns.
        let lens = [5, 0, 3];
        assert_eq!(step(&lens, 2, None, Dir::Down), Some((0, 0)));
        assert_eq!(step(&lens, 2, None, Dir::Up), None);
        assert_eq!(step(&lens, 2, Some((0, 1)), Dir::Down), Some((0, 3)));
        // From the second column of the last full row, Down takes the last cell.
        assert_eq!(step(&lens, 2, Some((0, 3)), Dir::Down), Some((0, 4)));
        // Past the end, into the next open section in the same column.
        assert_eq!(step(&lens, 2, Some((0, 4)), Dir::Down), Some((2, 0)));
        assert_eq!(step(&lens, 2, Some((2, 1)), Dir::Up), Some((0, 4)));
        assert_eq!(step(&lens, 2, Some((2, 0)), Dir::Up), Some((0, 4)));
        assert_eq!(step(&lens, 2, Some((0, 4)), Dir::Right), Some((2, 0)));
        assert_eq!(step(&lens, 2, Some((2, 0)), Dir::Left), Some((0, 4)));
        assert_eq!(step(&lens, 2, Some((2, 2)), Dir::Right), Some((2, 2)));
        assert_eq!(step(&lens, 2, Some((0, 0)), Dir::Up), Some((0, 0)));
    }

    #[test]
    fn column_count() {
        // Eagle's 320 px panel less its padding holds two 140 px columns.
        assert_eq!(columns(296.0, 140, false), 2);
        assert_eq!(columns(296.0, 100, false), 2);
        assert_eq!(columns(310.0, 100, false), 3);
        assert_eq!(columns(50.0, 140, false), 1);
        assert_eq!(columns(296.0, 140, true), 1);
    }

    #[test]
    fn sidebar_tab_cycles() {
        assert_eq!(Side::All.cycle(2, false), Side::Group(0));
        assert_eq!(Side::Group(1).cycle(2, false), Side::Ungrouped);
        assert_eq!(Side::Ungrouped.cycle(2, false), Side::All);
        assert_eq!(Side::All.cycle(2, true), Side::Ungrouped);
    }

    fn item(name: &str, folders: &[&str], tags: &[&str]) -> Item {
        let mut i: Item = serde_json::from_str(r#"{"id":"I","name":"n","size":1,"btime":0,"mtime":0,"ext":"png","tags":[],"folders":[],"isDeleted":false,"url":"","annotation":"","modificationTime":0,"height":0,"width":0,"lastModified":0,"palettes":[]}"#).unwrap();
        i.name = name.into();
        i.folders = s(folders);
        i.tags = s(tags);
        i
    }

    #[test]
    fn recommends_words_from_names_and_folders() {
        let folders: Vec<falcon_library::model::Folder> = serde_json::from_str(
            r#"[{"id":"P","name":"Travel photos","description":"","children":[{"id":"C","name":"Japan 2024","description":"","children":[],"modificationTime":0,"tags":[],"password":"","passwordTips":""}],"modificationTime":0,"tags":[],"password":"","passwordTips":""}]"#,
        )
        .unwrap();
        let one = item("the kyoto temple_copy v2", &["C"], &["temple"]);
        let known: HashSet<&str> = ["Japan"].into_iter().collect();
        let got = suggestions(&[&one], &folders, &known, &one.tags);
        assert_eq!(got, s(&["Japan", "Kyoto", "V2", "Travel", "Photos"]));
        let two = item("other", &[], &[]);
        assert_eq!(suggestions(&[&one, &two], &folders, &known, &[]), s(&["Japan", "Travel", "Photos"]), "names only for one item");
    }

    #[test]
    fn tag_page_clicks() {
        let order = s(&["a", "b", "c", "d", "e"]);
        let (p, a) = select_click(&[], None, &order, "b", false, false);
        assert_eq!((p.clone(), a.clone()), (s(&["b"]), Some("b".into())));
        let (p, a) = select_click(&p, a.as_deref(), &order, "d", true, false);
        assert_eq!(p, s(&["b", "c", "d"]));
        let (p, a) = select_click(&p, a.as_deref(), &order, "c", false, true);
        assert_eq!(p, s(&["b", "d"]), "⌘ drops a picked tag");
        let (p, a) = select_click(&p, a.as_deref(), &order, "a", false, true);
        assert_eq!(p, s(&["b", "d", "a"]));
        // ⇧ runs from the last clicked tag and keeps the others.
        let (p, _) = select_click(&p, a.as_deref(), &order, "e", true, false);
        assert_eq!(p, s(&["b", "d", "a", "c", "e"]));
        // With no anchor ⇧ toggles just the tag.
        assert_eq!(select_click(&[], None, &order, "c", true, false).0, s(&["c"]));
        let (p, _) = select_click(&order, Some("a"), &order, "c", false, false);
        assert_eq!(p, s(&["c"]));
    }

    #[test]
    fn merge_takes_two_to_five() {
        assert!(!can_merge(1));
        assert!(can_merge(2) && can_merge(5));
        assert!(!can_merge(6));
    }

    #[test]
    fn tag_sorts() {
        let base = counts(&[("b", 2), ("A", 5), ("c", 2), ("a", 1)]);
        let run = |s: TagSort| {
            let mut v = base.clone();
            s.apply(&mut v);
            v.into_iter().map(|x| x.0).collect::<Vec<_>>()
        };
        assert_eq!(run(TagSort::NameUp), s(&["A", "a", "b", "c"]));
        assert_eq!(run(TagSort::NameDown), s(&["c", "b", "a", "A"]));
        assert_eq!(run(TagSort::CountDown), s(&["A", "b", "c", "a"]));
        assert_eq!(run(TagSort::CountUp), s(&["a", "b", "c", "A"]));
    }

    #[test]
    fn common_tags_keep_the_first_items_order() {
        let a = item("a", &[], &["x", "y", "z"]);
        let b = item("b", &[], &["z", "x"]);
        assert_eq!(common_tags(&[&a, &b]), s(&["x", "z"]));
        assert!(common_tags(&[]).is_empty());
    }
}
