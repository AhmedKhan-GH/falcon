//! UI-independent logic: filtering, sorting, counting and formatting.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use falcon_library::model::{Folder, Item};
use falcon_library::search::{self, Filter, OrderBy, Query};

/// Which sidebar entry is active.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    Uncategorized,
    Untagged,
    /// Most recently changed items first.
    Recent,
    /// A shuffled view; the seed changes each time the entry is opened.
    Random(u64),
    /// The tag overview page.
    AllTags,
    /// The duplicates page.
    Duplicates,
    Trash,
    /// Items placed directly in a folder (Eagle hides subfolder content by
    /// default).
    Folder(String),
    Tag(String),
    /// A smart folder's matches.
    Smart(String),
}

impl Scope {
    /// Title shown in the toolbar and inspector.
    pub fn title(&self, roots: &[Folder]) -> String {
        match self {
            Scope::All => "All".into(),
            Scope::Uncategorized => "Uncategorized".into(),
            Scope::Untagged => "Untagged".into(),
            Scope::Recent => "Recently Used".into(),
            Scope::Random(_) => "Random".into(),
            Scope::AllTags => "All Tags".into(),
            Scope::Duplicates => "Duplicates".into(),
            Scope::Trash => "Trash".into(),
            Scope::Folder(id) => find_folder(roots, id).map_or_else(|| "Folder".into(), |f| f.name.clone()),
            Scope::Tag(t) => t.clone(),
            Scope::Smart(_) => "Smart Folder".into(),
        }
    }
}

/// Finds a folder by id anywhere in the tree.
pub fn find_folder<'a>(roots: &'a [Folder], id: &str) -> Option<&'a Folder> {
    let mut found = None;
    for r in roots {
        r.walk(&mut |f| {
            if f.id == id {
                found = Some(f);
            }
        });
    }
    found
}

/// A folder's names from the top level down, joined with ` / ` (the
/// folder chip's tooltip, as Eagle titles it).
pub fn folder_path(roots: &[Folder], id: &str) -> Option<String> {
    fn down<'a>(list: &'a [Folder], id: &str, path: &mut Vec<&'a str>) -> bool {
        for f in list {
            path.push(&f.name);
            if f.id == id || down(&f.children, id, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    down(roots, id, &mut path).then(|| path.join(" / "))
}

/// Number of live items placed directly in each folder, as Eagle's sidebar
/// shows them. Folders with no direct items are absent.
pub fn folder_counts(items: &[Item]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for i in items.iter().filter(|i| !i.is_deleted) {
        let unique: HashSet<&String> = i.folders.iter().collect();
        for f in unique {
            *out.entry(f.clone()).or_default() += 1;
        }
    }
    out
}

/// Counts for the fixed sidebar entries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NavCounts {
    pub all: usize,
    pub uncategorized: usize,
    pub untagged: usize,
    pub trash: usize,
    pub tags: usize,
}

pub fn nav_counts(items: &[Item]) -> NavCounts {
    let mut c = NavCounts::default();
    let mut tags = HashSet::new();
    for i in items {
        if i.is_deleted {
            c.trash += 1;
            continue;
        }
        c.all += 1;
        c.uncategorized += usize::from(i.folders.is_empty());
        c.untagged += usize::from(i.tags.is_empty());
        tags.extend(i.tags.iter());
    }
    c.tags = tags.len();
    c
}

/// Live tags with their item counts, sorted by name.
pub fn tag_counts(items: &[Item]) -> Vec<(String, usize)> {
    let mut map: HashMap<&str, usize> = HashMap::new();
    for i in items.iter().filter(|i| !i.is_deleted) {
        for t in &i.tags {
            *map.entry(t).or_default() += 1;
        }
    }
    let mut v: Vec<_> = map.into_iter().map(|(t, n)| (t.to_string(), n)).collect();
    v.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then_with(|| a.0.cmp(&b.0)));
    v
}

fn in_scope(item: &Item, scope: &Scope) -> bool {
    match scope {
        Scope::Trash => item.is_deleted,
        _ if item.is_deleted => false,
        Scope::All | Scope::Recent | Scope::Random(_) | Scope::AllTags | Scope::Duplicates => true,
        Scope::Uncategorized => item.folders.is_empty(),
        Scope::Untagged => item.tags.is_empty(),
        Scope::Folder(id) => item.folders.iter().any(|f| f == id),
        Scope::Tag(t) => item.tags.iter().any(|x| x == t),
        // Membership is decided by the caller's `only` set.
        Scope::Smart(_) => true,
    }
}

fn shuffle_key(seed: u64, id: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    id.hash(&mut h);
    h.finish()
}

#[allow(clippy::too_many_arguments)]
/// Indices into `items` that pass the scope, the search query and the filter
/// panel, in Eagle's order. A colour filter sorts by colour distance. The
/// query is matched with typo tolerance; exact matches come first, then
/// typo matches by number of edits, each group in that order.
pub fn visible(
    items: &[Item],
    roots: &[Folder],
    scope: &Scope,
    query: &str,
    filter: &Filter,
    order: OrderBy,
    increase: bool,
    only: Option<&HashSet<String>>,
) -> Vec<usize> {
    let index = search::folder_index(roots);
    let q = Query::parse(query);
    let qlen = query.trim().chars().count();
    let hits: Vec<(usize, f64, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| in_scope(i, scope) && only.is_none_or(|o| o.contains(&i.id)))
        .filter_map(|(n, i)| {
            let cost = if q.is_empty() { 0 } else { q.fuzzy_score(&search::search_text(i, &index, qlen))? };
            filter.check(i).map(|d| (n, d, cost))
        })
        .collect();
    let cost: HashMap<usize, usize> = hits.iter().map(|&(n, _, c)| (n, c)).collect();
    let mut hits: Vec<(usize, f64)> = hits.into_iter().map(|(n, d, _)| (n, d)).collect();
    match scope {
        Scope::Random(seed) => hits.sort_by_key(|&(i, _)| shuffle_key(*seed, &items[i].id)),
        Scope::Recent => hits.sort_by(|a, b| items[b.0].last_modified.cmp(&items[a.0].last_modified)),
        Scope::Trash => hits.sort_by_key(|&(i, _)| {
            let it = &items[i];
            std::cmp::Reverse(it.extra.get("deletedTime").and_then(|v| v.as_u64()).unwrap_or(it.modification_time))
        }),
        _ if filter.color.is_some() => hits.sort_by(|a, b| a.1.total_cmp(&b.1)),
        _ => {
            let mut refs: Vec<&Item> = hits.iter().map(|&(i, _)| &items[i]).collect();
            search::sort(&mut refs, order, increase);
            let pos: HashMap<&str, usize> = hits.iter().map(|&(i, _)| (items[i].id.as_str(), i)).collect();
            let mut out: Vec<usize> = refs.iter().map(|it| pos[it.id.as_str()]).collect();
            out.sort_by_key(|i| cost[i]);
            return out;
        }
    }
    // Exact matches keep their order ahead of typo matches, fewest edits first.
    let mut out: Vec<usize> = hits.into_iter().map(|(i, _)| i).collect();
    out.sort_by_key(|i| cost[i]);
    out
}

/// Byte size the way Eagle prints it: `512 B`, `5.34 KB`, `17.19 KB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} {}", UNITS[u])
}

/// Local date and time the way Eagle prints it: `2026/10/26 2:15`.
pub fn format_ms(ms: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(t) => t.format("%Y/%m/%d %-H:%M").to_string(),
        None => "-".into(),
    }
}

/// `#RRGGBB`.
pub fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// Eagle's item layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Layout {
    /// Columns of equal width; each card keeps its image's shape.
    #[default]
    Waterfall,
    /// Rows of equal height filling the width.
    Justified,
    /// Square cards.
    Grid,
    /// One row per item with its details.
    List,
}

impl Layout {
    pub const ALL: [Layout; 4] = [Layout::Waterfall, Layout::Justified, Layout::Grid, Layout::List];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Waterfall => "Waterfall",
            Layout::Justified => "Justified",
            Layout::Grid => "Grid",
            Layout::List => "List",
        }
    }

    /// Whether cards show the name and size under the image.
    pub fn has_caption(self) -> bool {
        matches!(self, Layout::Waterfall | Layout::Grid)
    }
}

/// Where one card goes: `x, y, w, h` of the image box, in content space.
/// A caption, when the layout has one, sits below it.
pub type Place = [f32; 4];

/// Places items with the given aspect ratios (width / height) in `width`.
/// `size` is the zoom: column width for waterfall and grid, row height for
/// justified. Returns the image boxes, the content height and the number of
/// columns (for keyboard navigation).
pub fn place(layout: Layout, aspects: &[f32], width: f32, size: f32, gap: f32, caption: f32, row_gap: f32) -> (Vec<Place>, f32, usize) {
    let aspect = |a: f32| if a.is_finite() && a > 0.0 { a } else { 1.0 };
    match layout {
        Layout::Grid => {
            let cols = columns(width, size, gap);
            let row_h = size + caption + row_gap;
            let out = (0..aspects.len())
                .map(|n| [(n % cols) as f32 * (size + gap), (n / cols) as f32 * row_h, size, size])
                .collect();
            (out, aspects.len().div_ceil(cols) as f32 * row_h, cols)
        }
        Layout::Waterfall => {
            let cols = columns(width, size, gap);
            let col_w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).max(1.0);
            let mut heights = vec![0.0f32; cols];
            let mut out = Vec::with_capacity(aspects.len());
            for &a in aspects {
                let (c, &y) = heights.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).unwrap();
                // Very tall or wide images are clamped so a column stays usable.
                let h = (col_w / aspect(a)).clamp(col_w * 0.25, col_w * 3.0);
                out.push([c as f32 * (col_w + gap), y, col_w, h]);
                heights[c] = y + h + caption + row_gap;
            }
            (out, heights.into_iter().fold(0.0, f32::max), cols)
        }
        Layout::Justified => {
            let mut out = Vec::with_capacity(aspects.len());
            let (mut y, mut start, mut sum) = (0.0f32, 0usize, 0.0f32);
            let mut rows = 0;
            for (i, &a) in aspects.iter().enumerate() {
                sum += aspect(a);
                let n = (i + 1 - start) as f32;
                let avail = width - gap * (n - 1.0);
                let last = i + 1 == aspects.len();
                if sum * size >= avail || last {
                    // Full rows are scaled to fill; the last keeps `size`.
                    let h = if sum * size >= avail { avail / sum } else { size };
                    let mut x = 0.0;
                    for &b in &aspects[start..=i] {
                        let w = aspect(b) * h;
                        out.push([x, y, w, h]);
                        x += w + gap;
                    }
                    y += h + gap;
                    rows += 1;
                    start = i + 1;
                    sum = 0.0;
                }
            }
            let cols = aspects.len().div_ceil(rows.max(1));
            (out, y, cols.max(1))
        }
        Layout::List => {
            let row = 44.0;
            let out = (0..aspects.len()).map(|n| [0.0, n as f32 * row, width, row]).collect();
            (out, aspects.len() as f32 * row, 1)
        }
    }
}

/// Grid layout: how many square cards of `size` fit in `width`.
pub fn columns(width: f32, size: f32, gap: f32) -> usize {
    (((width + gap) / (size + gap)).floor() as usize).max(1)
}

// ---------------------------------------------------------------------------
// Date, duration, camera and font filters

use falcon_library::search::{DateFilter, DatePreset, DurationUnit};

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

// ---------------------------------------------------------------------------
// Audio playback

/// The fraction of a seek bar at pointer `x`, clamped to 0..=1.
pub fn seek_fraction(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 { 0.0 } else { ((x - left) / width).clamp(0.0, 1.0) }
}

/// The time a click at `x` on a seek bar seeks to.
pub fn seek_time(x: f32, left: f32, width: f32, duration: f64) -> f64 {
    f64::from(seek_fraction(x, left, width)) * duration.max(0.0)
}

/// Where the playhead sits on a seek bar for position `pos` (seconds).
pub fn playhead_x(pos: f64, duration: f64, left: f32, width: f32) -> f32 {
    let f = if duration > 0.0 { (pos / duration).clamp(0.0, 1.0) } else { 0.0 };
    left + f as f32 * width
}

/// How many of `n` waveform peaks are already played at `pos`.
pub fn played_peaks(pos: f64, duration: f64, n: usize) -> usize {
    if duration <= 0.0 { 0 } else { ((pos / duration).clamp(0.0, 1.0) * n as f64).round() as usize }
}

/// The elapsed / total text beside the waveform: `0:12 / 3:04`.
pub fn time_label(pos: f64, duration: f64) -> String {
    format!("{} / {}", format_duration(pos.min(duration.max(0.0))), format_duration(duration))
}

// ---------------------------------------------------------------------------
// Duplicates

/// One group of the duplicates page.
#[derive(Debug, Clone, PartialEq)]
pub struct DupGroup {
    /// Byte-identical originals (true) or similar pictures (false).
    pub exact: bool,
    pub ids: Vec<String>,
    /// Bits each member's perceptual hash differs from the first member's
    /// (0 for exact groups and for members without a hash).
    pub distances: Vec<u32>,
}

/// Builds the page's groups: every exact group, then every similar group
/// that is not just an exact group again (all its members in one exact
/// group).
pub fn dup_groups(exact: Vec<Vec<String>>, near: Vec<Vec<String>>, hash: impl Fn(&str) -> Option<u64>) -> Vec<DupGroup> {
    let mut out: Vec<DupGroup> = exact.iter().map(|ids| DupGroup { exact: true, distances: vec![0; ids.len()], ids: ids.clone() }).collect();
    for ids in near {
        let covered = exact.iter().any(|e| ids.iter().all(|id| e.contains(id)));
        if covered || ids.len() < 2 {
            continue;
        }
        let first = hash(&ids[0]);
        let distances = ids
            .iter()
            .map(|id| match (first, hash(id)) {
                (Some(a), Some(b)) => (a ^ b).count_ones(),
                _ => 0,
            })
            .collect();
        out.push(DupGroup { exact: false, ids, distances });
    }
    out
}

/// The groups with only the members `alive` keeps; a group left with one
/// member drops out.
pub fn live_groups(groups: &[DupGroup], alive: impl Fn(&str) -> bool) -> Vec<DupGroup> {
    groups
        .iter()
        .filter_map(|g| {
            let keep: Vec<usize> = (0..g.ids.len()).filter(|&n| alive(&g.ids[n])).collect();
            (keep.len() > 1).then(|| DupGroup {
                exact: g.exact,
                ids: keep.iter().map(|&n| g.ids[n].clone()).collect(),
                distances: keep.iter().map(|&n| g.distances[n]).collect(),
            })
        })
        .collect()
}

/// Every member but the first of each group, each id once. The first
/// member is the one kept, and an item kept by one group is never picked
/// from another.
pub fn all_but_first(groups: &[DupGroup]) -> Vec<String> {
    let keep: HashSet<&str> = groups.iter().filter_map(|g| g.ids.first().map(String::as_str)).collect();
    let mut seen = HashSet::new();
    groups
        .iter()
        .flat_map(|g| g.ids.iter().skip(1))
        .filter(|id| !keep.contains(id.as_str()) && seen.insert(id.as_str()))
        .cloned()
        .collect()
}

/// Whether `path` names an Eagle library folder (`*.library`). Such a path
/// is opened, never imported, so its `metadata.json`, item folders and
/// thumbnails don't turn into items.
pub fn is_library_path(path: &std::path::Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        let n = n.as_bytes();
        n.len() > ".library".len() && n[n.len() - ".library".len()..].eq_ignore_ascii_case(b".library")
    })
}

/// Splits dropped, pasted or picked paths into libraries to open and paths
/// to import, keeping their order.
pub fn split_libraries(paths: Vec<std::path::PathBuf>) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    paths.into_iter().partition(|p| is_library_path(p))
}

/// What dropping grid items onto a sidebar folder does, after Eagle's
/// `classifySelected`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderDrop {
    /// Take the items out of this folder (the open one) and put them in
    /// the target.
    Move(String),
    /// Put the items in the target as well, keeping their other folders.
    Add,
}

/// The drop's effect for the open `scope`, with ⌥ held or not. Inside a
/// folder a drop moves the items to the target and ⌥ adds them instead;
/// from every other view (All, a tag, a smart folder, the Trash…) it adds.
/// Dropping on the folder that is open changes nothing (`None`).
pub fn folder_drop(scope: &Scope, target: &str, alt: bool) -> Option<FolderDrop> {
    match scope {
        Scope::Folder(open) if open == target => None,
        Scope::Folder(open) if !alt => Some(FolderDrop::Move(open.clone())),
        _ => Some(FolderDrop::Add),
    }
}

/// Applies a folder drop to one item: it leaves the open folder on a move,
/// joins the target, gets the target's `auto_tags` it lacks (even when it
/// was in the target already, as in Eagle) and comes out of the Trash.
/// Returns whether the item changed.
pub fn apply_folder_drop(item: &mut Item, target: &str, drop: &FolderDrop, auto_tags: &[String]) -> bool {
    let before = (item.folders.clone(), item.is_deleted, item.tags.len());
    falcon_library::autotag::append_new(&mut item.tags, auto_tags);
    if let FolderDrop::Move(open) = drop {
        item.folders.retain(|f| f != open);
    }
    if !item.folders.iter().any(|f| f == target) {
        item.folders.push(target.to_string());
    }
    item.is_deleted = false;
    (item.folders.clone(), item.is_deleted, item.tags.len()) != before
}

/// What to select once `removed` leave the grid (`order`): the first
/// item after the first removed one that stays, else the nearest one
/// before it, as Eagle does after Remove from Folder.
pub fn next_after_removal(order: &[String], removed: &[String]) -> Option<String> {
    let first = order.iter().position(|id| removed.contains(id))?;
    let stays = |id: &&String| !removed.contains(id);
    order[first..].iter().find(stays).or_else(|| order[..first].iter().rev().find(stays)).cloned()
}

/// The notice after a folder drop: "Moved 3 items to Logos".
pub fn folder_drop_notice(drop: &FolderDrop, n: usize, folder: &str) -> String {
    let verb = match drop {
        FolderDrop::Move(_) => "Moved",
        FolderDrop::Add => "Added",
    };
    let noun = if n == 1 { "item" } else { "items" };
    format!("{verb} {n} {noun} to {folder}")
}

#[cfg(test)]
mod folder_drop_tests {
    use super::*;
    use serde_json::json;

    fn item(folders: &[&str], deleted: bool) -> Item {
        let v = json!({
            "id": "I1", "name": "a", "size": 1, "btime": 0, "mtime": 0, "ext": "png",
            "tags": [], "folders": folders, "isDeleted": deleted, "url": "",
            "annotation": "", "modificationTime": 0, "height": 10, "width": 10,
            "lastModified": 0, "palettes": []
        });
        Item::from_json_str(&v.to_string()).unwrap()
    }

    #[test]
    fn a_folder_moves_and_alt_adds() {
        let open = Scope::Folder("A".into());
        assert_eq!(folder_drop(&open, "B", false), Some(FolderDrop::Move("A".into())));
        assert_eq!(folder_drop(&open, "B", true), Some(FolderDrop::Add));
    }

    #[test]
    fn other_views_add() {
        for scope in [Scope::All, Scope::Uncategorized, Scope::Untagged, Scope::Recent, Scope::Random(3), Scope::Trash, Scope::Tag("t".into()), Scope::Smart("S".into())] {
            assert_eq!(folder_drop(&scope, "B", false), Some(FolderDrop::Add), "{scope:?}");
            assert_eq!(folder_drop(&scope, "B", true), Some(FolderDrop::Add), "{scope:?}");
        }
    }

    #[test]
    fn dropping_on_the_open_folder_does_nothing() {
        let open = Scope::Folder("A".into());
        assert_eq!(folder_drop(&open, "A", false), None);
        assert_eq!(folder_drop(&open, "A", true), None);
    }

    #[test]
    fn applying_a_move() {
        let mut it = item(&["A", "C"], false);
        assert!(apply_folder_drop(&mut it, "B", &FolderDrop::Move("A".into()), &[]));
        assert_eq!(it.folders, ["C", "B"]);
        // Already there and not in the open folder: nothing to write.
        assert!(!apply_folder_drop(&mut it, "B", &FolderDrop::Move("A".into()), &[]));
        assert_eq!(it.folders, ["C", "B"]);
    }

    #[test]
    fn applying_an_add() {
        let mut it = item(&["A"], false);
        assert!(apply_folder_drop(&mut it, "B", &FolderDrop::Add, &[]));
        assert_eq!(it.folders, ["A", "B"]);
        assert!(!apply_folder_drop(&mut it, "B", &FolderDrop::Add, &[]));
        // A trashed item already in the folder still comes back.
        let mut gone = item(&["B"], true);
        assert!(apply_folder_drop(&mut gone, "B", &FolderDrop::Add, &[]));
        assert!(!gone.is_deleted);
        assert_eq!(gone.folders, ["B"]);
    }

    #[test]
    fn a_drop_adds_auto_tags_even_when_already_filed() {
        let tags = ["k".to_string(), "new".to_string()];
        let mut it = item(&["B"], false);
        it.tags = vec!["k".into()];
        // Nothing moves, but the missing auto-tag is still added.
        assert!(apply_folder_drop(&mut it, "B", &FolderDrop::Add, &tags));
        assert_eq!((it.folders.as_slice(), it.tags.as_slice()), (["B".to_string()].as_slice(), tags.as_slice()));
        assert!(!apply_folder_drop(&mut it, "B", &FolderDrop::Add, &tags));
    }

    #[test]
    fn notices() {
        assert_eq!(folder_drop_notice(&FolderDrop::Move("A".into()), 3, "Logos"), "Moved 3 items to Logos");
        assert_eq!(folder_drop_notice(&FolderDrop::Add, 1, "Logos"), "Added 1 item to Logos");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selection_after_removal() {
        let order: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(next_after_removal(&order, &ids(&["b"])).as_deref(), Some("c"));
        assert_eq!(next_after_removal(&order, &ids(&["b", "c"])).as_deref(), Some("d"));
        assert_eq!(next_after_removal(&order, &ids(&["c", "d"])).as_deref(), Some("b"));
        assert_eq!(next_after_removal(&order, &ids(&["a", "b", "c", "d"])), None);
        assert_eq!(next_after_removal(&order, &ids(&["x"])), None);
    }

    #[test]
    fn libraries_are_split_from_importables() {
        use std::path::PathBuf;
        let paths: Vec<PathBuf> = ["/a/Photos.library", "/a/shot.png", "/a/Folder", "/a/Old.LIBRARY/", "/a/library", "/a/.library", "/a/notes.library.txt", "/a/B.library"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let (libraries, rest) = split_libraries(paths);
        let names = |v: &[PathBuf]| v.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(&libraries), ["/a/Photos.library", "/a/Old.LIBRARY/", "/a/B.library"]);
        assert_eq!(names(&rest), ["/a/shot.png", "/a/Folder", "/a/library", "/a/.library", "/a/notes.library.txt"]);
        assert_eq!(split_libraries(Vec::new()), (Vec::new(), Vec::new()));
    }

    #[allow(clippy::too_many_arguments)]
    fn item(id: &str, name: &str, folders: &[&str], tags: &[&str], deleted: bool, t: u64, size: u64, w: u64) -> Item {
        let v = json!({
            "id": id, "name": name, "size": size, "btime": 0, "mtime": 0, "ext": "png",
            "tags": tags, "folders": folders, "isDeleted": deleted, "url": "",
            "annotation": "Hello Note", "modificationTime": t, "height": 10, "width": w,
            "lastModified": t, "palettes": []
        });
        Item::from_json_str(&v.to_string()).unwrap()
    }

    fn folder(id: &str, children: Vec<Folder>) -> Folder {
        let v = json!({
            "id": id, "name": format!("{id}-name"), "description": "", "children": children,
            "modificationTime": 0, "tags": [], "password": "", "passwordTips": ""
        });
        serde_json::from_value(v).unwrap()
    }

    fn fixture() -> (Vec<Folder>, Vec<Item>) {
        let roots = vec![folder("A", vec![folder("B", vec![])]), folder("C", vec![])];
        let items = vec![
            item("1", "Zebra", &["A"], &["red", "Big"], false, 30, 100, 5),
            item("2", "apple", &["B"], &["red"], false, 10, 300, 50),
            item("3", "Mango", &["A", "B"], &[], false, 20, 200, 20),
            item("4", "gone", &["C"], &["red"], true, 40, 1, 1),
            item("5", "loose", &[], &["blue"], false, 50, 2, 2),
        ];
        (roots, items)
    }

    fn names(items: &[Item], v: &[usize]) -> Vec<String> {
        v.iter().map(|&i| items[i].name.clone()).collect()
    }

    #[test]
    fn folder_paths_join_ancestors() {
        let (roots, _) = fixture();
        assert_eq!(folder_path(&roots, "B").as_deref(), Some("A-name / B-name"));
        assert_eq!(folder_path(&roots, "C").as_deref(), Some("C-name"));
        assert_eq!(folder_path(&roots, "Z"), None);
    }

    #[test]
    fn folder_counts_are_direct_and_skip_trash() {
        let (_, items) = fixture();
        let c = folder_counts(&items);
        assert_eq!(c["A"], 2);
        assert_eq!(c["B"], 2);
        assert!(!c.contains_key("C"));
    }

    #[test]
    fn nav_counts_cover_fixed_entries() {
        let (_, items) = fixture();
        assert_eq!(nav_counts(&items), NavCounts { all: 4, uncategorized: 1, untagged: 1, trash: 1, tags: 3 });
    }

    #[test]
    fn scopes() {
        let (roots, items) = fixture();
        let v = |s: Scope| names(&items, &visible(&items, &roots, &s, "", &Filter::default(), OrderBy::Name, true, None));
        assert_eq!(v(Scope::All), ["apple", "loose", "Mango", "Zebra"]);
        assert_eq!(v(Scope::Uncategorized), ["loose"]);
        assert_eq!(v(Scope::Untagged), ["Mango"]);
        assert_eq!(v(Scope::Trash), ["gone"]);
        assert_eq!(v(Scope::Folder("A".into())), ["Mango", "Zebra"]);
        assert_eq!(v(Scope::Tag("red".into())), ["apple", "Zebra"]);
        assert_eq!(v(Scope::Recent), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(v(Scope::Random(7)).len(), 4);
    }

    #[test]
    fn search_terms_and_negation() {
        let (roots, items) = fixture();
        let q = |s: &str| names(&items, &visible(&items, &roots, &Scope::All, s, &Filter::default(), OrderBy::Name, true, None));
        assert_eq!(q("ZEB"), ["Zebra"]);
        assert_eq!(q("red big"), ["Zebra"]);
        assert_eq!(q("red -big"), ["apple"]);
        assert_eq!(q(".png").len(), 4);
        assert_eq!(q("B-name"), ["apple", "Mango"]); // folder name
        assert_eq!(q("zebra OR mango"), ["Mango", "Zebra"]);
        assert_eq!(q("  ").len(), 4);
    }

    #[test]
    fn search_tolerates_typos_after_exact_hits() {
        let (roots, items) = fixture();
        let q = |s: &str| names(&items, &visible(&items, &roots, &Scope::All, s, &Filter::default(), OrderBy::Name, true, None));
        assert_eq!(q("zebar"), ["Zebra"]);
        assert_eq!(q("mangp"), ["Mango"]);
        assert!(q("qqqqq").is_empty());
    }

    #[test]
    fn sorting_follows_eagle_directions() {
        let (roots, items) = fixture();
        let s = |k, inc| names(&items, &visible(&items, &roots, &Scope::All, "", &Filter::default(), k, inc, None));
        // Time keys are newest first when "increase" is on.
        assert_eq!(s(OrderBy::Import, true), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(OrderBy::Import, false), ["apple", "Mango", "Zebra", "loose"]);
        assert_eq!(s(OrderBy::FileSize, true), ["loose", "Zebra", "Mango", "apple"]);
        assert_eq!(s(OrderBy::Resolution, false), ["apple", "Mango", "Zebra", "loose"]);
    }

    #[test]
    fn layouts_place_every_item_inside_the_width() {
        let aspects = [1.0, 2.0, 0.5, 1.5, 1.0, 3.0, 0.75];
        for layout in Layout::ALL {
            let (places, height, cols) = place(layout, &aspects, 600.0, 140.0, 8.0, 46.0, 10.0);
            assert_eq!(places.len(), aspects.len(), "{layout:?}");
            assert!(cols >= 1 && height > 0.0);
            for [x, y, w, h] in places {
                assert!(x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0, "{layout:?}");
                assert!(x + w <= 600.0 + 0.01, "{layout:?} overflows: {x} + {w}");
            }
        }
    }

    #[test]
    fn justified_rows_fill_the_width() {
        let (places, _, _) = place(Layout::Justified, &[1.0; 9], 400.0, 100.0, 0.0, 0.0, 0.0);
        // Four squares of 100 fill a 400 row exactly.
        let first_row: f32 = places.iter().filter(|p| p[1] == 0.0).map(|p| p[2]).sum();
        assert!((first_row - 400.0).abs() < 0.01);
        assert!(places.iter().all(|p| (p[3] - 100.0).abs() < 0.01));
    }

    #[test]
    fn waterfall_fills_the_shortest_column() {
        let (places, _, cols) = place(Layout::Waterfall, &[0.5, 2.0, 2.0], 300.0, 140.0, 20.0, 0.0, 0.0);
        assert_eq!(cols, 2);
        // The tall first item fills column 0; the next two stack in column 1.
        assert_eq!(places[1][0], places[2][0]);
        assert!(places[2][1] > places[1][1]);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(5465), "5.34 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(hex([255, 0, 16]), "#FF0010");
        assert_eq!(columns(300.0, 100.0, 8.0), 2);
    }
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

    #[test]
    fn seek_bar_mapping() {
        assert_eq!(seek_fraction(50.0, 0.0, 200.0), 0.25);
        assert_eq!(seek_fraction(-10.0, 0.0, 200.0), 0.0);
        assert_eq!(seek_fraction(500.0, 0.0, 200.0), 1.0);
        assert_eq!(seek_fraction(5.0, 0.0, 0.0), 0.0);
        assert_eq!(seek_time(150.0, 100.0, 100.0, 60.0), 30.0);
        assert_eq!(playhead_x(30.0, 60.0, 100.0, 100.0), 150.0);
        assert_eq!(playhead_x(90.0, 60.0, 100.0, 100.0), 200.0);
        assert_eq!(playhead_x(5.0, 0.0, 100.0, 100.0), 100.0);
        // A click puts the playhead where it was clicked.
        let t = seek_time(173.0, 100.0, 400.0, 245.0);
        assert!((playhead_x(t, 245.0, 100.0, 400.0) - 173.0).abs() < 1e-3);
        assert_eq!(played_peaks(30.0, 60.0, 1000), 500);
        assert_eq!(played_peaks(1.0, 0.0, 1000), 0);
        assert_eq!(time_label(12.4, 184.0), "0:12 / 3:04");
        assert_eq!(time_label(200.0, 184.0), "3:04 / 3:04");
    }

    #[test]
    fn duplicate_groups() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let hashes: HashMap<&str, u64> = [("a", 0b0000), ("b", 0b0000), ("c", 0b0111), ("d", 0b0001)].into();
        let exact = vec![s(&["a", "b"])];
        // [a, b] again is dropped; [c, d] stays, with distances from c.
        let near = vec![s(&["a", "b"]), s(&["c", "d"])];
        let g = dup_groups(exact, near, |id| hashes.get(id).copied());
        assert_eq!(g.len(), 2);
        assert!(g[0].exact);
        assert_eq!(g[0].distances, vec![0, 0]);
        assert!(!g[1].exact);
        assert_eq!(g[1].distances, vec![0, 2]);
        assert_eq!(all_but_first(&g), s(&["b", "d"]));
        // An item kept by one group is never picked from another.
        let overlap = vec![
            DupGroup { exact: true, ids: s(&["a", "b"]), distances: vec![0, 0] },
            DupGroup { exact: false, ids: s(&["b", "a", "c"]), distances: vec![0, 0, 3] },
        ];
        assert_eq!(all_but_first(&overlap), s(&["c"]));
        // Trashed members leave; a group of one disappears.
        let live = live_groups(&g, |id| id != "d");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].ids, s(&["a", "b"]));
    }
}

// ---------------------------------------------------------------------------
// Reordering items by hand

/// Width of the drop strip Eagle puts just outside each card's left and
/// right edges while items can be reordered by hand.
pub const SORT_STRIP: f32 = 16.0;

/// Height of the drop bar at the top or bottom of a list row.
pub const SORT_BAR: f32 = 4.0;

/// Where dragged items would land in a hand-sorted grid: before or after
/// the card `target`, and the bar to draw (`x, y, w, h` in content space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManualDrop {
    pub target: usize,
    pub after: bool,
    pub bar: [f32; 4],
}

/// Hit-tests a drag at `(x, y)` (content space) against the cards in
/// `places` from [`place`]. Cards for which `skip` is true (the dragged
/// ones) take no drops. In the card layouts, a card's box (image plus
/// `caption`) widened by [`SORT_STRIP`] on each side takes the drop: the
/// left half inserts before it, the right half after. Where the strips of
/// neighbours overlap, the card nearest the pointer wins, and on a tie the
/// later card. In the list layout the top half of a row inserts before it
/// and the bottom half after. The bar sits in the `gap` beside the card
/// (or across the row edge in the list).
pub fn manual_drop(layout: Layout, places: &[Place], caption: f32, gap: f32, skip: impl Fn(usize) -> bool, x: f32, y: f32) -> Option<ManualDrop> {
    if layout == Layout::List {
        let n = places.iter().enumerate().position(|(n, p)| !skip(n) && y >= p[1] && y < p[1] + p[3])?;
        let [px, py, w, h] = places[n];
        let after = y >= py + h / 2.0;
        let edge = if after { py + h } else { py };
        return Some(ManualDrop { target: n, after, bar: [px, edge - SORT_BAR / 2.0, w, SORT_BAR] });
    }
    let mut best: Option<(usize, f32)> = None;
    for (n, &[px, py, w, h]) in places.iter().enumerate() {
        if skip(n) || y < py || y >= py + h + caption || x < px - SORT_STRIP || x >= px + w + SORT_STRIP {
            continue;
        }
        let dist = if x < px { px - x } else if x > px + w { x - px - w } else { 0.0 };
        if best.is_none_or(|(_, d)| dist <= d) {
            best = Some((n, dist));
        }
    }
    let (n, _) = best?;
    let [px, py, w, h] = places[n];
    let after = x >= px + w / 2.0;
    let bw = SORT_STRIP.min(gap.max(SORT_BAR));
    let cx = if after { px + w + gap / 2.0 } else { px - gap / 2.0 };
    Some(ManualDrop { target: n, after, bar: [cx - bw / 2.0, py, bw, h + caption] })
}

#[cfg(test)]
mod manual_drop_tests {
    use super::*;

    const GAP: f32 = 8.0;
    const CAP: f32 = 46.0;

    fn none(_: usize) -> bool {
        false
    }

    #[test]
    fn grid_halves_pick_before_or_after() {
        // Three 100 pt columns in 316 pt: cards at x = 0, 108, 216.
        let (p, _, cols) = place(Layout::Grid, &[1.0; 5], 316.0, 100.0, GAP, CAP, 10.0);
        assert_eq!(cols, 3);
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, none, 20.0, 50.0).unwrap();
        assert_eq!((d.target, d.after), (0, false));
        assert_eq!(d.bar, [-8.0, 0.0, 8.0, 146.0]);
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, none, 190.0, 120.0).unwrap();
        assert_eq!((d.target, d.after), (1, true), "the caption takes drops too");
        assert_eq!(d.bar[0], 208.0 + 4.0 - 4.0);
        // Second row: y starts at 156.
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, none, 120.0, 160.0).unwrap();
        assert_eq!((d.target, d.after), (4, false));
        // Below the last row, or in the row gap, nothing.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 50.0, 150.0), None);
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 50.0, 400.0), None);
    }

    #[test]
    fn grid_gap_goes_to_the_nearer_card() {
        let (p, _, _) = place(Layout::Grid, &[1.0; 3], 316.0, 100.0, GAP, CAP, 10.0);
        // The gap is 100..108; 101 is nearer card 0, 107 nearer card 1.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 101.0, 10.0).map(|d| (d.target, d.after)), Some((0, true)));
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 107.0, 10.0).map(|d| (d.target, d.after)), Some((1, false)));
        // Exactly halfway, the later card wins.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, 104.0, 10.0).map(|d| (d.target, d.after)), Some((1, false)));
        // The strip reaches 16 pt past the outer edge.
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, -15.0, 10.0).map(|d| d.target), Some(0));
        assert_eq!(manual_drop(Layout::Grid, &p, CAP, GAP, none, -17.0, 10.0), None);
    }

    #[test]
    fn dragged_cards_take_no_drop() {
        let (p, _, _) = place(Layout::Grid, &[1.0; 3], 316.0, 100.0, GAP, CAP, 10.0);
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, |n| n == 1, 150.0, 10.0);
        assert_eq!(d, None, "over the dragged card itself");
        let d = manual_drop(Layout::Grid, &p, CAP, GAP, |n| n == 1, 107.0, 10.0).unwrap();
        assert_eq!((d.target, d.after), (0, true), "its neighbour's strip still works");
    }

    #[test]
    fn waterfall_uses_each_cards_own_column_box() {
        // Two columns of 104 pt: a tall card, then two short ones.
        let (p, _, cols) = place(Layout::Waterfall, &[0.5, 2.0, 2.0], 216.0, 100.0, GAP, CAP, 10.0);
        assert_eq!(cols, 2);
        // Card 1 is in column 2 at the top; card 2 goes under it.
        assert_eq!(p[1][0], 112.0);
        assert_eq!(p[2][0], 112.0);
        let below = p[2][1] + 5.0;
        let d = manual_drop(Layout::Waterfall, &p, CAP, GAP, none, 200.0, below).unwrap();
        assert_eq!((d.target, d.after), (2, true));
        assert_eq!(d.bar[3], p[2][3] + CAP);
        // The tall card's lower part, beside card 2.
        let d = manual_drop(Layout::Waterfall, &p, CAP, GAP, none, 10.0, below).unwrap();
        assert_eq!((d.target, d.after), (0, false));
    }

    #[test]
    fn justified_rows_have_no_caption() {
        let (p, _, _) = place(Layout::Justified, &[1.5, 1.5, 1.0], 300.0, 100.0, GAP, 0.0, 10.0);
        let d = manual_drop(Layout::Justified, &p, 0.0, GAP, none, p[1][0] + 1.0, p[1][1] + 1.0).unwrap();
        assert_eq!((d.target, d.after), (1, false));
        assert_eq!(d.bar[3], p[1][3]);
        let last = p[2];
        let d = manual_drop(Layout::Justified, &p, 0.0, GAP, none, last[0] + last[2] + 3.0, last[1] + 1.0).unwrap();
        assert_eq!((d.target, d.after), (2, true));
    }

    #[test]
    fn list_rows_split_top_and_bottom() {
        let (p, _, _) = place(Layout::List, &[1.0; 3], 500.0, 100.0, GAP, 0.0, 10.0);
        let d = manual_drop(Layout::List, &p, 0.0, GAP, none, 300.0, 50.0).unwrap();
        assert_eq!((d.target, d.after), (1, false));
        assert_eq!(d.bar, [0.0, 42.0, 500.0, SORT_BAR]);
        let d = manual_drop(Layout::List, &p, 0.0, GAP, none, 300.0, 80.0).unwrap();
        assert_eq!((d.target, d.after), (1, true));
        assert_eq!(d.bar[1], 86.0);
        assert_eq!(manual_drop(Layout::List, &p, 0.0, GAP, none, 300.0, 200.0), None);
        assert_eq!(manual_drop(Layout::List, &p, 0.0, GAP, |n| n == 1, 300.0, 50.0), None);
    }

    #[test]
    fn zero_sized_items_get_placeholder_cards() {
        // A 0×0 item (a .md or .epub) has no aspect; the grid code passes 0
        // or NaN and `place` falls back to a square placeholder box.
        for layout in Layout::ALL {
            let caption = if layout.has_caption() { CAP } else { 0.0 };
            let (p, _, _) = place(layout, &[0.0, f32::NAN, 1.0], 316.0, 100.0, GAP, caption, 10.0);
            for (n, r) in p.iter().enumerate() {
                assert!(r.iter().all(|v| v.is_finite()) && r[2] > 0.0 && r[3] > 0.0, "{layout:?} card {n}: {r:?}");
                let d = manual_drop(layout, &p, caption, GAP, none, r[0] + 1.0, r[1] + 1.0).unwrap();
                assert_eq!((d.target, d.after), (n, false), "{layout:?}");
            }
        }
        // A degenerate box never panics and never divides by zero.
        let d = manual_drop(Layout::Grid, &[[0.0, 0.0, 0.0, 0.0]], 0.0, 0.0, none, 0.0, 0.0);
        assert_eq!(d, None);
        assert_eq!(manual_drop(Layout::Grid, &[], CAP, GAP, none, 0.0, 0.0), None);
    }
}

// ---------------------------------------------------------------------------
// The detail viewer's shell

/// Room the macOS window buttons need at the left of a bar that starts at
/// the window's left edge.
pub const TRAFFIC_LIGHTS: f32 = 72.0;

/// The left and right edges of the viewer, given the window's edges and
/// the widths of the sidebar and inspector when they are shown. As in
/// Eagle's detail mode, the viewer takes the space between the two panels
/// (all of it when both are hidden).
pub fn viewer_span(left: f32, right: f32, sidebar: Option<f32>, inspector: Option<f32>) -> (f32, f32) {
    let l = left + sidebar.unwrap_or(0.0);
    let r = (right - inspector.unwrap_or(0.0)).max(l);
    (l, r)
}

/// Where the first control of a bar starts, measured from the bar's left
/// edge: after the window buttons when nothing is to its left.
pub fn bar_inset(at_window_edge: bool) -> f32 {
    if at_window_edge { 20.0 + TRAFFIC_LIGHTS + 6.0 } else { 20.0 }
}

/// Eagle's toggle for both panels: shows both when the sidebar is hidden,
/// otherwise hides both. Takes and returns (sidebar shown, inspector
/// shown).
pub fn toggle_all(sidebar: bool, _inspector: bool) -> (bool, bool) {
    if sidebar { (false, false) } else { (true, true) }
}

/// The viewer bar's counter: the item's place in the grid order, from 1,
/// and the number of items.
pub fn viewer_counter(order: &[String], id: &str) -> Option<(usize, usize)> {
    order.iter().position(|x| x == id).map(|n| (n + 1, order.len()))
}

/// The end of the grid a viewer step ran into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    First,
    Last,
}

impl Edge {
    /// The note the viewer shows when a step can go no further.
    pub fn note(self) -> &'static str {
        match self {
            Edge::First => "This is the first item",
            Edge::Last => "This is the last item",
        }
    }
}

/// Where a viewer step of `step` from position `at` (none: the item is not
/// in the grid) lands among `len` items, or the end it ran into.
pub fn viewer_step(len: usize, at: Option<usize>, step: isize) -> Result<usize, Edge> {
    if len == 0 {
        return Err(if step < 0 { Edge::First } else { Edge::Last });
    }
    let Some(at) = at else { return Ok(0) };
    let to = at as isize + step;
    if to < 0 {
        Err(Edge::First)
    } else if to >= len as isize {
        Err(Edge::Last)
    } else {
        Ok(to as usize)
    }
}

/// A zoom step in the image viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewZoom {
    In,
    Out,
    Actual,
    Fit,
    /// Toggle "fit to window edge" (Eagle's backtick key).
    Edge,
}

/// The viewer's zoom after `cmd` from `scale` (`None`: fit the window).
pub fn view_zoom(cmd: ViewZoom, scale: f32) -> Option<f32> {
    match cmd {
        ViewZoom::In => Some((scale * 1.25).min(ZOOM_MAX)),
        ViewZoom::Out => Some((scale / 1.25).max(ZOOM_MIN)),
        ViewZoom::Actual => Some(1.0),
        ViewZoom::Fit | ViewZoom::Edge => None,
    }
}

/// The image viewer's zoom range, Eagle's 5% to 800% (`zoom_MIN`, `zoom_MAX`).
pub const ZOOM_MIN: f32 = 0.05;
pub const ZOOM_MAX: f32 = 8.0;

/// The zoom menu's sizes, in percent (Eagle's `openRatioContextMenu`).
pub const ZOOM_PRESETS: [u32; 11] = [5, 10, 25, 50, 100, 125, 150, 200, 300, 400, 800];

/// The zoom slider's range. Its positions run 5 to 200; above 100 each
/// step is 7%, so the right end is 800% (Eagle's `getRatioExp`).
pub const SLIDER_RANGE: (f32, f32) = (5.0, 200.0);

/// Zoom (1.0 = 100%) at slider position `v`.
pub fn slider_zoom(v: f32) -> f32 {
    let v = v.clamp(SLIDER_RANGE.0, SLIDER_RANGE.1);
    (if v > 100.0 { 100.0 + (v - 100.0) * 7.0 } else { v }) / 100.0
}

/// Slider position of `zoom`.
pub fn zoom_slider_pos(zoom: f32) -> f32 {
    let pct = zoom * 100.0;
    (if pct > 100.0 { (pct - 100.0) / 7.0 + 100.0 } else { pct }).clamp(SLIDER_RANGE.0, SLIDER_RANGE.1)
}

/// The zoom at slider position `v`, snapped to 25, 50, 100, 125, 150, 175
/// or 200% when within 3% of one, as Eagle's slider does.
pub fn slider_zoom_snapped(v: f32) -> f32 {
    let pct = slider_zoom(v) * 100.0;
    [25.0, 50.0, 100.0, 125.0, 150.0, 175.0, 200.0].into_iter().find(|s: &f32| (pct - s).abs() <= 3.0).unwrap_or(pct) / 100.0
}

/// "Fit to window edge" (Eagle's `zoomFitEdge`): the whole image fills the
/// view, enlarged if it is smaller.
pub fn edge_fit(natural: (f32, f32), view: (f32, f32)) -> f32 {
    (view.0 / natural.0.max(1.0)).min(view.1 / natural.1.max(1.0)).clamp(ZOOM_MIN, ZOOM_MAX)
}

// ---- Image viewer: resolution, fitting and zoom ----

/// Long side of the loader's full tier, the render the viewer shows first.
pub const FULL_TIER_SIDE: u32 = 3072;

/// Whether the viewer decodes `ext` at its full resolution when zoomed in
/// past the full tier. Eagle's bitmap viewer decodes jpg, png, webp, bmp,
/// avif, jxl, tiff and heic originals in full (`BitmapViewer.isSupportFormat`);
/// falcon decodes the ones its image decoder reads (TIFF included, falling
/// back to the QuickLook render when a TIFF's compression isn't supported).
/// GIFs belong to the animation player; SVGs are re-rendered instead.
pub fn full_resolution(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "jfif" | "jpe" | "png" | "webp" | "bmp" | "tif" | "tiff")
}

/// Whether the original is worth decoding: the image is larger than the
/// full tier and is drawn (in physical pixels, `shown_px` on its long side)
/// larger than the full tier's render, which would otherwise be magnified.
/// Decoding only then keeps flipping through large photos cheap.
pub fn wants_original(ext: &str, natural_long: u32, shown_px: f32) -> bool {
    full_resolution(ext) && natural_long > FULL_TIER_SIDE && shown_px > FULL_TIER_SIDE as f32
}

/// Side of the textures a full-resolution image is cut into. egui can't
/// hold a texture larger than the GPU's limit (`max_texture_side`, 8192 or
/// 16384 on Macs), and a smaller tile also keeps each upload short enough
/// to spread over frames.
pub fn tile_side(max_texture_side: usize) -> usize {
    max_texture_side.clamp(64, 2048)
}

/// One tile of a full-resolution image, in image pixels. `core` is the part
/// the tile draws; `padded` adds a pixel on every inner edge, so linear
/// filtering at the seams blends with the neighbour instead of clamping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileSpan {
    pub core: [usize; 4],
    pub padded: [usize; 4],
}

/// Whether an image is "long" (a tall strip such as a web page capture)
/// by Eagle's `smartZoom` rule: at least 960 px wide and over 1.8 times as
/// tall as wide, or at least 320 px wide and over 2.7 times as tall. Eagle
/// has no such rule for wide images.
pub fn is_long_image(w: f32, h: f32) -> bool {
    (w >= 960.0 && w * 1.8 < h) || (w >= 320.0 && w * 2.7 < h)
}

/// The scale Eagle gives an image named `name` (without extension) for
/// its pixel density: `@2x` shows at 50%, `@3x` at 33%, `@1.5x` at 67%,
/// `@0.5x` at 200%.
pub fn density_scale(name: &str) -> Option<f32> {
    [("@2x", 0.5), ("@3x", 1.0 / 3.0), ("@1.5x", 2.0 / 3.0), ("@0.5x", 2.0)].into_iter().find(|(s, _)| name.ends_with(s)).map(|(_, v)| v)
}

/// How an image opens ("Smart Zoom"): the scale, and the vertical offset of
/// its centre from the view's centre. It fits inside `view` less `margin`
/// on each side, never enlarged; a long image fills the width instead and
/// opens at its top, for scrolling down; an `@2x`-style name opens at its
/// density's scale when that fits.
pub fn smart_fit(natural: (f32, f32), view: (f32, f32), margin: f32, name: &str) -> (f32, f32) {
    let (w, h) = (natural.0.max(1.0), natural.1.max(1.0));
    let (vw, vh) = ((view.0 - 2.0 * margin).max(1.0), (view.1 - 2.0 * margin).max(1.0));
    let fit = (vw / w).min(vh / h);
    let mut scale = fit.min(1.0);
    if is_long_image(w, h) {
        scale = (view.0 / w).min(1.0);
    }
    if let Some(d) = density_scale(name) {
        scale = if w * d <= view.0 && h * d <= view.1 { d } else { fit };
    }
    let top = ((h * scale - view.1) / 2.0).max(0.0);
    (scale, top)
}

/// Pixel count above which Eagle shows its "too large" page instead of a
/// TIFF, HEIC or TGA (`MAX_DIMENSION` in `index.html`).
pub const TOO_LARGE_PIXELS: u64 = 120_000_000;

/// Whether the viewer shows the "too large to preview" page for an item:
/// a TIFF, HEIC/HEIF or TGA over [`TOO_LARGE_PIXELS`], unless it has a
/// custom thumbnail, as in Eagle. Other formats show at any size.
pub fn too_large_to_preview(ext: &str, w: u64, h: u64, custom_thumbnail: bool) -> bool {
    matches!(ext, "tif" | "tiff" | "heic" | "heif" | "hif" | "tga") && w.saturating_mul(h) > TOO_LARGE_PIXELS && !custom_thumbnail
}

/// Cuts a `w` x `h` image into tiles whose padded size is at most `side`.
pub fn tile_spans(w: usize, h: usize, side: usize) -> Vec<TileSpan> {
    let step = side.saturating_sub(2).max(1);
    let mut out = Vec::new();
    let mut y = 0;
    while y < h {
        let y1 = (y + step).min(h);
        let mut x = 0;
        while x < w {
            let x1 = (x + step).min(w);
            out.push(TileSpan { core: [x, y, x1, y1], padded: [x.saturating_sub(1), y.saturating_sub(1), (x1 + 1).min(w), (y1 + 1).min(h)] });
            x = x1;
        }
        y = y1;
    }
    out
}

#[cfg(test)]
mod image_viewer_tests {
    use super::*;

    #[test]
    fn originals_only_when_the_full_tier_would_be_magnified() {
        assert!(wants_original("jpg", 12000, 3500.0));
        assert!(!wants_original("jpg", 12000, 1500.0), "fitted, the full tier is enough");
        assert!(!wants_original("jpg", 3000, 6000.0), "the full tier already is the original");
        assert!(!wants_original("gif", 12000, 6000.0), "animations are the player's");
        assert!(!wants_original("psd", 12000, 6000.0), "no decoder");
        assert!(wants_original("tiff", 9000, 4000.0));
    }

    #[test]
    fn slider_runs_to_800_percent_and_snaps() {
        assert_eq!(slider_zoom(5.0), 0.05);
        assert_eq!(slider_zoom(100.0), 1.0);
        assert_eq!(slider_zoom(200.0), 8.0);
        assert_eq!(slider_zoom(150.0), 4.5);
        assert_eq!(slider_zoom(0.0), 0.05, "clamped to the range");
        for z in [0.05, 0.5, 1.0, 2.0, 8.0] {
            assert!((slider_zoom(zoom_slider_pos(z)) - z).abs() < 1e-5);
        }
        assert_eq!(zoom_slider_pos(20.0), 200.0);
        // Within 3% of a snap point the slider lands on it.
        assert_eq!(slider_zoom_snapped(52.0), 0.5);
        assert_eq!(slider_zoom_snapped(55.0), 0.55);
        assert_eq!(slider_zoom_snapped(zoom_slider_pos(1.98)), 2.0);
        assert_eq!(ZOOM_PRESETS.first(), Some(&5));
        assert_eq!(ZOOM_PRESETS.last(), Some(&800));
    }

    #[test]
    fn edge_fit_enlarges_to_the_view() {
        assert_eq!(edge_fit((200.0, 100.0), (1000.0, 800.0)), 5.0);
        assert_eq!(edge_fit((4000.0, 1000.0), (1000.0, 800.0)), 0.25);
        assert_eq!(edge_fit((10.0, 10.0), (1000.0, 800.0)), ZOOM_MAX);
    }

    #[test]
    fn long_images_by_eagles_ratios() {
        assert!(is_long_image(1000.0, 1801.0));
        assert!(!is_long_image(1000.0, 1800.0));
        assert!(!is_long_image(959.0, 2000.0), "under 960 wide needs 2.7x");
        assert!(is_long_image(400.0, 1081.0));
        assert!(!is_long_image(319.0, 5000.0));
        assert!(!is_long_image(8000.0, 1000.0), "wide strips aren't long");
    }

    #[test]
    fn smart_fit_fits_long_images_to_the_width_at_the_top() {
        // An ordinary image fits inside the margins, never enlarged.
        assert_eq!(smart_fit((2000.0, 1000.0), (1040.0, 600.0), 20.0, "a"), (0.5, 0.0));
        assert_eq!(smart_fit((200.0, 100.0), (1040.0, 600.0), 20.0, "a"), (1.0, 0.0));
        // A web page capture fills the width and opens at its top: its
        // centre sits half the overflow below the view's centre.
        let (scale, top) = smart_fit((1500.0, 9000.0), (1000.0, 800.0), 20.0, "page");
        assert!((scale - 1000.0 / 1500.0).abs() < 1e-6);
        assert!((top - (6000.0 - 800.0) / 2.0).abs() < 1e-3);
        // Narrower than the view, a long image shows at 100%.
        assert_eq!(smart_fit((400.0, 2000.0), (1000.0, 800.0), 20.0, "x").0, 1.0);
        // @2x opens at half size when that fits.
        assert_eq!(smart_fit((1200.0, 800.0), (1000.0, 800.0), 20.0, "shot@2x").0, 0.5);
        assert_eq!(density_scale("icon@3x"), Some(1.0 / 3.0));
        assert_eq!(density_scale("plain"), None);
    }

    #[test]
    fn too_large_only_for_eagles_formats_over_the_limit() {
        assert!(too_large_to_preview("tiff", 12000, 10001, false));
        assert!(!too_large_to_preview("tiff", 12000, 10000, false), "exactly the limit still shows");
        assert!(too_large_to_preview("heic", 20000, 7000, false));
        assert!(too_large_to_preview("tga", 20000, 7000, false));
        assert!(!too_large_to_preview("tiff", 20000, 7000, true), "a custom thumbnail shows instead");
        assert!(!too_large_to_preview("png", 20000, 7000, false));
        assert!(!too_large_to_preview("jpg", 100_000, 100_000, false));
    }

    #[test]
    fn tiles_fit_the_gpu_and_cover_the_image() {
        assert_eq!(tile_side(8192), 2048);
        assert_eq!(tile_side(1024), 1024);
        let (w, h, side) = (12000, 8000, 2048);
        let spans = tile_spans(w, h, side);
        let mut covered = 0;
        for s in &spans {
            let [x0, y0, x1, y1] = s.padded;
            assert!(x1 - x0 <= side && y1 - y0 <= side);
            let [cx0, cy0, cx1, cy1] = s.core;
            assert!(x0 <= cx0 && cx1 <= x1 && y0 <= cy0 && cy1 <= y1);
            covered += (cx1 - cx0) * (cy1 - cy0);
        }
        assert_eq!(covered, w * h, "cores tile the image exactly");
        // Inner edges carry a pixel of overlap; the image's own edges don't.
        assert_eq!(spans[0].padded, [0, 0, 2047, 2047]);
        assert_eq!(spans[1].padded[0], 2045);
        assert_eq!(tile_spans(10, 10, 2048), vec![TileSpan { core: [0, 0, 10, 10], padded: [0, 0, 10, 10] }]);
    }
}

#[cfg(test)]
mod viewer_shell_tests {
    use super::*;

    #[test]
    fn viewer_zoom_steps_and_limits() {
        assert_eq!(view_zoom(ViewZoom::In, 1.0), Some(1.25));
        assert_eq!(view_zoom(ViewZoom::Out, 1.25), Some(1.0));
        assert_eq!(view_zoom(ViewZoom::In, 7.0), Some(8.0));
        assert_eq!(view_zoom(ViewZoom::Out, 0.051), Some(0.05));
        assert_eq!(view_zoom(ViewZoom::Edge, 2.0), None);
        assert_eq!(view_zoom(ViewZoom::Actual, 0.3), Some(1.0));
        assert_eq!(view_zoom(ViewZoom::Fit, 4.0), None);
    }

    #[test]
    fn steps_stop_at_the_ends() {
        assert_eq!(viewer_step(3, Some(1), 1), Ok(2));
        assert_eq!(viewer_step(3, Some(1), -1), Ok(0));
        assert_eq!(viewer_step(3, Some(2), 1), Err(Edge::Last));
        assert_eq!(viewer_step(3, Some(0), -1), Err(Edge::First));
        assert_eq!(viewer_step(1, Some(0), 1), Err(Edge::Last));
        // An item that left the grid steps to the first one.
        assert_eq!(viewer_step(3, None, 1), Ok(0));
        assert_eq!(viewer_step(0, None, -1), Err(Edge::First));
        assert_ne!(Edge::First.note(), Edge::Last.note());
    }

    #[test]
    fn counter_counts_from_one() {
        let order: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(viewer_counter(&order, "a"), Some((1, 3)));
        assert_eq!(viewer_counter(&order, "c"), Some((3, 3)));
        assert_eq!(viewer_counter(&order, "z"), None);
        assert_eq!(viewer_counter(&[], "a"), None);
    }

    #[test]
    fn toggle_all_follows_the_sidebar() {
        assert_eq!(toggle_all(true, true), (false, false));
        assert_eq!(toggle_all(false, false), (true, true));
        // Mixed states go by the sidebar, as in Eagle.
        assert_eq!(toggle_all(true, false), (false, false));
        assert_eq!(toggle_all(false, true), (true, true));
        // Twice gets back to both shown or both hidden.
        let (s, i) = toggle_all(true, false);
        assert_eq!(toggle_all(s, i), (true, true));
    }

    #[test]
    fn viewer_sits_between_the_panels() {
        let (sw, iw) = (240.0, 260.0);
        assert_eq!(viewer_span(0.0, 1440.0, Some(sw), Some(iw)), (240.0, 1180.0));
        assert_eq!(viewer_span(0.0, 1440.0, None, Some(iw)), (0.0, 1180.0));
        assert_eq!(viewer_span(0.0, 1440.0, Some(sw), None), (240.0, 1440.0));
        assert_eq!(viewer_span(0.0, 1440.0, None, None), (0.0, 1440.0));
        // A window narrower than both panels leaves an empty span, never a
        // negative one.
        assert_eq!(viewer_span(0.0, 400.0, Some(sw), Some(iw)), (240.0, 240.0));
    }

    #[test]
    fn bars_leave_room_for_the_window_buttons() {
        assert_eq!(bar_inset(false), 20.0);
        assert_eq!(bar_inset(true), 98.0);
    }
}

/// Several sidebar folders, or several smart folders, picked with ⌘ and ⇧
/// clicks, as in Eagle. The view then shows the items of all of them, and
/// the context menu acts on all of them. Empty means the plain single
/// view of the open scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeSel {
    /// Picked ids, in the order they were picked.
    pub ids: Vec<String>,
    /// Where a ⇧-click range starts: the last row a ⌘-click added (Eagle's
    /// current row). Without one the range starts at the open folder.
    pub anchor: Option<String>,
}

impl TreeSel {
    /// Two or more picked: the batch menu applies.
    pub fn is_multi(&self) -> bool {
        self.ids.len() > 1
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|x| x == id)
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    /// A ⌘-click on `id` while `open` is the folder shown. A first ⌘-click
    /// picks the open folder too; after that the click toggles `id`, but
    /// the last picked row stays picked. Adding a row makes it the anchor.
    pub fn toggle(&mut self, open: Option<&str>, id: &str) {
        if self.ids.is_empty()
            && let Some(o) = open
        {
            self.ids.push(o.to_string());
        }
        match self.ids.iter().position(|x| x == id) {
            Some(at) if self.ids.len() > 1 => {
                self.ids.remove(at);
            }
            Some(_) => {}
            None => {
                self.ids.push(id.to_string());
                self.anchor = Some(id.to_string());
            }
        }
    }

    /// A ⇧-click on `id`: every row of `rows` (the rows shown, top to
    /// bottom) from the anchor, or else the open folder, through `id` joins
    /// the picked ones; the anchor stays. Returns false, changing nothing,
    /// when there is no start row among `rows`, as Eagle ignores the click.
    pub fn extend(&mut self, rows: &[String], open: Option<&str>, id: &str) -> bool {
        let Some(start) = self.anchor.as_deref().or(open) else { return false };
        let (Some(a), Some(b)) = (rows.iter().position(|r| r == start), rows.iter().position(|r| r == id)) else { return false };
        for r in &rows[a.min(b)..=a.max(b)] {
            if !self.contains(r) {
                self.ids.push(r.clone());
            }
        }
        true
    }
}

#[cfg(test)]
mod tree_sel_tests {
    use super::*;

    fn rows(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_click_picks_the_open_folder_then_toggles() {
        let mut s = TreeSel::default();
        s.toggle(Some("A"), "C");
        assert_eq!(s.ids, ["A", "C"]);
        assert_eq!(s.anchor.as_deref(), Some("C"));
        s.toggle(Some("A"), "B");
        assert_eq!(s.ids, ["A", "C", "B"]);
        // Toggling off keeps the anchor where it was.
        s.toggle(Some("A"), "C");
        assert_eq!(s.ids, ["A", "B"]);
        assert_eq!(s.anchor.as_deref(), Some("B"));
        s.toggle(Some("A"), "A");
        assert_eq!(s.ids, ["B"]);
        // The last one can't be taken away.
        s.toggle(Some("A"), "B");
        assert_eq!(s.ids, ["B"]);
        assert!(!s.is_multi());
        // ⌘-clicking the open folder alone picks only it.
        let mut s = TreeSel::default();
        s.toggle(Some("A"), "A");
        assert_eq!(s.ids, ["A"]);
        assert_eq!(s.anchor, None);
    }

    #[test]
    fn shift_click_adds_the_rows_between() {
        let shown = rows(&["A", "A1", "A2", "B", "C"]);
        let mut s = TreeSel::default();
        assert!(s.extend(&shown, Some("A1"), "B"));
        assert_eq!(s.ids, ["A1", "A2", "B"]);
        // Upwards from the open folder, adding to what is picked.
        assert!(s.extend(&shown, Some("A1"), "A"));
        assert_eq!(s.ids, ["A1", "A2", "B", "A"]);
        // From the ⌘-click anchor rather than the open folder.
        let mut s = TreeSel::default();
        s.toggle(Some("A"), "C");
        assert!(s.extend(&shown, Some("A"), "A2"));
        assert_eq!(s.ids, ["A", "C", "A2", "B"]);
        assert_eq!(s.anchor.as_deref(), Some("C"));
    }

    #[test]
    fn shift_click_without_a_start_row_does_nothing() {
        let shown = rows(&["A", "B"]);
        let mut s = TreeSel::default();
        assert!(!s.extend(&shown, None, "B"));
        // The open folder is hidden (collapsed or filtered out).
        assert!(!s.extend(&shown, Some("Z"), "B"));
        assert!(s.ids.is_empty());
        s.clear();
        assert_eq!(s, TreeSel::default());
    }
}

/// A folder name typed for New Folder with Selection, cleaned as Eagle
/// does on macOS: runs of white space become one space, `:`, `/` and `\`
/// become spaces, and the ends are trimmed.
pub fn clean_folder_name(name: &str) -> String {
    let squeezed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    squeezed.replace([':', '/', '\\'], " ").trim().to_string()
}

#[cfg(test)]
mod clean_name_tests {
    use super::*;

    #[test]
    fn folder_names_are_cleaned_like_eagle() {
        assert_eq!(clean_folder_name("  Mood\tboard   2026 "), "Mood board 2026");
        assert_eq!(clean_folder_name("a/b:c\\d"), "a b c d");
        assert_eq!(clean_folder_name("a / b"), "a   b", "slashes go after the spaces are squeezed");
        assert_eq!(clean_folder_name(" / "), "");
    }
}
