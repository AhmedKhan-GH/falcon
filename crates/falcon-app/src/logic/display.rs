//! Display options (Eagle's layout panel): captions, list columns, thumbnail
//! backgrounds and subfolder cards.

use crate::locale::tr;
use std::collections::HashSet;
use falcon_library::model::{Folder, Item};
use falcon_library::search::{self, OrderBy};
use super::format::{format_ms, format_size};
use super::layout::Layout;
use super::filters::format_duration;
use super::folders::find_folder;

/// What the second caption line under a card shows (Eagle's item info).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum InfoType {
    /// Width × height, or the length of a video or audio file, or the file
    /// size of an item without a preview.
    #[default]
    Dimensions,
    FileSize,
    Tags,
    Rating,
    Modified,
    Created,
}

impl InfoType {
    pub const ALL: [InfoType; 6] = [InfoType::Dimensions, InfoType::FileSize, InfoType::Tags, InfoType::Rating, InfoType::Modified, InfoType::Created];

    pub fn label(self) -> &'static str {
        match self {
            InfoType::Dimensions => tr("Dimensions"),
            InfoType::FileSize => tr("File Size"),
            InfoType::Tags => tr("Tags"),
            InfoType::Rating => tr("Rating"),
            InfoType::Modified => tr("Date Modified"),
            InfoType::Created => tr("Date Created"),
        }
    }
}

/// The grid display options, kept for the whole app as Eagle keeps them
/// (its `eagle.list.*` local storage), with Eagle's defaults.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Display {
    /// The name line under waterfall and grid cards.
    pub show_name: bool,
    /// The info line under waterfall and grid cards, and what it shows.
    pub show_info: bool,
    pub info: InfoType,
    /// The extension at the end of the name.
    pub show_ext: bool,
    /// The extension label at a card's top left.
    pub ext_badge: bool,
    /// The comment count at a card's top right.
    pub comment_badge: bool,
    /// Grid layout: the image fills its square (cropped) instead of fitting.
    pub fill: bool,
    /// A folder also shows the items of all its subfolders.
    pub subfolders: bool,
    /// The list layout's optional columns.
    pub columns: ListColumns,
    /// The subfolder cards above a folder's items are folded away.
    pub strip_collapsed: bool,
    /// The layout panel's Thumbnail option: Quality (Eagle's default,
    /// `eagle.list.show.originalImageWhenLarge`) shows large cards from
    /// their originals; Speed keeps the thumbnails.
    pub quality: bool,
    /// Hovered audio cards start playing (Eagle's `listAudioAutoPlay`,
    /// switched by the play button on the card).
    pub audio_autoplay: bool,
    /// Hovered video cards play without sound (Eagle's
    /// `eagle.list.video.muted`, default true, switched by the speaker
    /// button on the preview).
    pub video_muted: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display { show_name: true, show_info: true, info: InfoType::Dimensions, show_ext: true, ext_badge: true, comment_badge: true, fill: false, subfolders: false, columns: ListColumns::default(), strip_collapsed: false, quality: true, audio_autoplay: true, video_muted: true }
    }
}

/// Height of the name and info lines under a card in `layout`.
pub fn caption_height(layout: Layout, d: &Display) -> f32 {
    if !layout.has_caption() {
        return 0.0;
    }
    match (d.show_name, d.show_info) {
        (true, true) => 46.0,
        (false, false) => 0.0,
        _ => 28.0,
    }
}

/// The name under a card: with its extension, unless that is hidden.
pub fn caption_name(item: &Item, show_ext: bool) -> String {
    if show_ext && !item.ext.is_empty() { format!("{}.{}", item.name, item.ext) } else { item.name.clone() }
}

/// The info line under a card.
#[derive(Debug, Clone, PartialEq)]
pub enum Info {
    Text(String),
    /// Five stars, this many lit.
    Stars(u8),
}

/// The info line of `item` for `info`, as Eagle fills it: tags joined (a
/// dash when none), dates as the inspector prints them, and for
/// dimensions the length of a video or audio file, or the size of a file
/// without a preview.
pub fn caption_info(item: &Item, info: InfoType) -> Info {
    let stamp = |t: u64| format_ms(if t > 0 { t } else { item.modification_time });
    Info::Text(match info {
        InfoType::Dimensions => {
            if let Some(d) = search::duration(item).filter(|d| *d > 0.0) {
                format_duration(d)
            } else if item.width > 0 && item.height > 0 && item.extra.get("noPreview").and_then(|v| v.as_bool()) != Some(true) {
                format!("{} × {}", item.width, item.height)
            } else {
                format_size(item.size)
            }
        }
        InfoType::FileSize => format_size(item.size),
        InfoType::Tags if item.tags.is_empty() => "-".into(),
        InfoType::Tags => item.tags.join(", "),
        InfoType::Rating => return Info::Stars(item.extra.get("star").and_then(|v| v.as_u64()).unwrap_or(0).min(5) as u8),
        InfoType::Modified => stamp(item.mtime),
        InfoType::Created => stamp(item.btime),
    })
}

/// `id` and the ids of every folder under it; empty when there is no
/// such folder.
pub fn folder_tree(roots: &[Folder], id: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(f) = find_folder(roots, id) {
        f.walk(&mut |c| {
            out.insert(c.id.clone());
        });
    }
    out
}

/// Ids of the live items a folder view shows: those placed in the folder,
/// and with `subfolders` (Eagle's Show Subfolder Content) also those in
/// any folder under it.
pub fn folder_members(items: &[Item], roots: &[Folder], id: &str, subfolders: bool) -> HashSet<String> {
    let tree = if subfolders { folder_tree(roots, id) } else { HashSet::from([id.to_string()]) };
    items.iter().filter(|i| !i.is_deleted && i.folders.iter().any(|f| tree.contains(f))).map(|i| i.id.clone()).collect()
}

/// The subfolders a folder view shows as cards: its children, or with
/// `subfolders` every folder under it, depth first, as Eagle lists them.
pub fn subfolder_cards(folder: &Folder, subfolders: bool) -> Vec<&Folder> {
    if !subfolders {
        return folder.children.iter().collect();
    }
    let mut out = Vec::new();
    for c in &folder.children {
        c.walk(&mut |f| out.push(f));
    }
    out
}

/// A column of the list layout, in Eagle's header order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListColumn {
    Name,
    Tags,
    Dimensions,
    Rating,
    Type,
    Size,
    Imported,
}

impl ListColumn {
    pub const ALL: [ListColumn; 7] =
        [ListColumn::Name, ListColumn::Tags, ListColumn::Dimensions, ListColumn::Rating, ListColumn::Type, ListColumn::Size, ListColumn::Imported];
    /// The columns that can be hidden, in the order of Eagle's header menu.
    pub const OPTIONAL: [ListColumn; 6] = [ListColumn::Tags, ListColumn::Dimensions, ListColumn::Rating, ListColumn::Type, ListColumn::Size, ListColumn::Imported];

    pub fn label(self) -> &'static str {
        match self {
            ListColumn::Name => tr("Name"),
            ListColumn::Tags => tr("Tags"),
            ListColumn::Dimensions => tr("Dimensions"),
            ListColumn::Rating => tr("Rating"),
            ListColumn::Type => tr("Type"),
            ListColumn::Size => tr("File Size"),
            ListColumn::Imported => tr("Date Imported"),
        }
    }

    /// The sort a click on the column's header picks.
    pub fn sort_key(self) -> OrderBy {
        match self {
            ListColumn::Name => OrderBy::Name,
            ListColumn::Tags => OrderBy::Tags,
            ListColumn::Dimensions => OrderBy::Resolution,
            ListColumn::Rating => OrderBy::Rating,
            ListColumn::Type => OrderBy::Ext,
            ListColumn::Size => OrderBy::FileSize,
            ListColumn::Imported => OrderBy::Import,
        }
    }

    /// The column's share of the row's width.
    fn weight(self) -> f32 {
        match self {
            ListColumn::Name => 3.0,
            ListColumn::Tags => 2.0,
            ListColumn::Imported => 1.4,
            _ => 1.0,
        }
    }
}

/// Which optional list columns show, with Eagle's defaults (its
/// `eagle.list.layout.settings`): tags and rating hidden.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ListColumns {
    pub tags: bool,
    pub dimensions: bool,
    pub rating: bool,
    pub ext: bool,
    pub size: bool,
    pub imported: bool,
}

impl Default for ListColumns {
    fn default() -> Self {
        ListColumns { tags: false, dimensions: true, rating: false, ext: true, size: true, imported: true }
    }
}

impl ListColumns {
    /// The switch for an optional column (the name always shows).
    pub fn flag(&mut self, c: ListColumn) -> Option<&mut bool> {
        match c {
            ListColumn::Name => None,
            ListColumn::Tags => Some(&mut self.tags),
            ListColumn::Dimensions => Some(&mut self.dimensions),
            ListColumn::Rating => Some(&mut self.rating),
            ListColumn::Type => Some(&mut self.ext),
            ListColumn::Size => Some(&mut self.size),
            ListColumn::Imported => Some(&mut self.imported),
        }
    }

    /// The shown columns with each one's left edge and width as fractions
    /// of the row.
    pub fn spans(&self) -> Vec<(ListColumn, f32, f32)> {
        let mut c = self.clone();
        let shown: Vec<ListColumn> = ListColumn::ALL.into_iter().filter(|&k| c.flag(k).is_none_or(|on| *on)).collect();
        let total: f32 = shown.iter().map(|k| k.weight()).sum();
        let mut x = 0.0;
        shown
            .into_iter()
            .map(|k| {
                let w = k.weight() / total;
                let span = (k, x, w);
                x += w;
                span
            })
            .collect()
    }
}

/// A list cell's content.
pub fn list_cell(item: &Item, col: ListColumn, show_ext: bool) -> Info {
    match col {
        ListColumn::Name => Info::Text(caption_name(item, show_ext)),
        ListColumn::Tags => caption_info(item, InfoType::Tags),
        ListColumn::Dimensions => Info::Text(if item.width > 0 && item.height > 0 { format!("{} × {}", item.width, item.height) } else { "-".into() }),
        ListColumn::Rating => caption_info(item, InfoType::Rating),
        ListColumn::Type => Info::Text(item.ext.to_uppercase()),
        ListColumn::Size => Info::Text(format_size(item.size)),
        ListColumn::Imported => Info::Text(format_ms(item.modification_time)),
    }
}

/// The sort after a click on a list header, as Eagle's: the column already
/// sorted by flips its direction, any other sorts by it, increasing.
/// `current` is the view's sort key and direction (`None`: a folder that
/// follows the global sort).
pub fn header_sort(current: Option<(OrderBy, bool)>, clicked: OrderBy) -> (OrderBy, bool) {
    match current {
        Some((k, inc)) if k == clicked => (k, !inc),
        _ => (clicked, true),
    }
}

/// What is drawn behind an item's image in the grid and the viewer
/// (Eagle's per-item `background`; no key means none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbBg {
    Dark,
    Gray,
    Light,
    /// A checkerboard, for transparency.
    Grid,
}

impl ThumbBg {
    pub const ALL: [ThumbBg; 4] = [ThumbBg::Dark, ThumbBg::Gray, ThumbBg::Light, ThumbBg::Grid];

    /// The value Eagle stores in the item's `background`.
    pub fn key(self) -> &'static str {
        match self {
            ThumbBg::Dark => "dark",
            ThumbBg::Gray => "gray",
            ThumbBg::Light => "light",
            ThumbBg::Grid => "grid",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThumbBg::Dark => tr("Black"),
            ThumbBg::Gray => tr("Gray"),
            ThumbBg::Light => tr("White"),
            ThumbBg::Grid => tr("Checkerboard"),
        }
    }

    /// An item's background; an unknown value is none.
    pub fn of(item: &Item) -> Option<ThumbBg> {
        let v = item.extra.get("background")?.as_str()?;
        ThumbBg::ALL.into_iter().find(|b| b.key() == v)
    }
}

/// Sets (or with `None` removes) an item's `background`, as Eagle does;
/// returns whether it changed.
pub fn set_thumb_bg(item: &mut Item, bg: Option<ThumbBg>) -> bool {
    let before = item.extra.get("background").cloned();
    match bg {
        Some(b) => {
            item.extra.insert("background".into(), b.key().into());
        }
        None => {
            item.extra.remove("background");
        }
    }
    item.extra.get("background") != before.as_ref()
}


/// Largest side of a large card's picture decoded from its original.
pub const CARD_TIER_SIDE: u32 = 1600;

/// The formats whose large cards show the original in Quality mode, as
/// Eagle's `enlargeThumbnails` swaps them (jpg, png, webp, bmp, jfif).
pub fn quality_card_format(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "png" | "webp" | "bmp" | "jfif")
}

/// Whether a card drawn `shown_px` physical pixels on its long side should
/// show its original: Quality is on, the format is one Eagle swaps, and the
/// card is drawn larger than the grid render (`grid_side` pixels), which
/// would otherwise be enlarged. Eagle swaps once its tile height passes
/// 600 px; falcon's tiles stop at 400 points, so it asks the question
/// directly.
pub fn wants_card_original(quality: bool, ext: &str, shown_px: f32, grid_side: f32) -> bool {
    quality && quality_card_format(ext) && shown_px > grid_side
}

/// Whether `item` is pinned to the top of the folder being viewed, which
/// is when Eagle draws the pin badge: only for the one open folder (none
/// when several are picked together) and not while that folder's own sort
/// is Random, which ignores pins.
pub fn pinned_here(item: &Item, scope_folder: Option<&str>, random_sort: bool) -> bool {
    !random_sort && scope_folder.is_some_and(|f| falcon_library::item_ops::pinned_at(item, f).is_some())
}

#[cfg(test)]
mod display_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn large_cards_show_originals_in_quality_mode() {
        assert!(wants_card_original(true, "jpg", 800.0, 512.0));
        assert!(!wants_card_original(true, "jpg", 400.0, 512.0), "small cards keep the thumbnail");
        assert!(!wants_card_original(false, "jpg", 800.0, 512.0), "Speed keeps thumbnails");
        assert!(!wants_card_original(true, "gif", 800.0, 512.0), "only the formats Eagle swaps");
        assert!(Display::default().quality, "Eagle defaults to Quality");
    }
    use crate::logic::scope::Scope;
    use crate::logic::format::format_ms;
    use crate::logic::layout::{Layout, layout_for, layout_key};

    fn item() -> Item {
        serde_json::from_value(serde_json::json!({
            "id": "A", "name": "photo", "size": 2048, "btime": 0, "mtime": 0, "ext": "png", "tags": [], "folders": [],
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": 1, "height": 20, "width": 30,
            "lastModified": 1, "palettes": []
        }))
        .unwrap()
    }

    #[test]
    fn defaults_are_eagles() {
        let d = Display::default();
        assert!(d.show_name && d.show_info && d.show_ext && d.ext_badge && d.comment_badge && !d.fill);
        assert_eq!(d.info, InfoType::Dimensions);
        // Old saved state without the field reads as the default.
        let d: Display = serde_json::from_str(r#"{"show_name":false}"#).unwrap();
        assert!(!d.show_name && d.show_info);
        assert!(d.video_muted && d.audio_autoplay, "hover previews start muted, audio playing");
        let d = Display { video_muted: false, ..Display::default() };
        let back: Display = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert!(!back.video_muted);
    }

    #[test]
    fn caption_lines_follow_the_toggles() {
        let mut d = Display::default();
        assert_eq!(caption_height(Layout::Grid, &d), 46.0);
        assert_eq!(caption_height(Layout::Justified, &d), 0.0, "justified rows have no caption");
        d.show_info = false;
        assert_eq!(caption_height(Layout::Waterfall, &d), 28.0);
        d.show_name = false;
        assert_eq!(caption_height(Layout::Waterfall, &d), 0.0);
        let it = item();
        assert_eq!(caption_name(&it, true), "photo.png");
        assert_eq!(caption_name(&it, false), "photo");
    }

    #[test]
    fn info_per_type() {
        let mut it = item();
        let text = |it: &Item, t| match caption_info(it, t) {
            Info::Text(s) => s,
            Info::Stars(n) => format!("{n} stars"),
        };
        assert_eq!(text(&it, InfoType::Dimensions), "30 × 20");
        assert_eq!(text(&it, InfoType::FileSize), "2 KB");
        assert_eq!(text(&it, InfoType::Tags), "-");
        it.tags = vec!["a".into(), "b".into()];
        assert_eq!(text(&it, InfoType::Tags), "a, b");
        assert_eq!(text(&it, InfoType::Rating), "0 stars");
        it.extra.insert("star".into(), 4.into());
        assert_eq!(text(&it, InfoType::Rating), "4 stars");
        // No file dates: the import time stands in, as in Eagle.
        assert_eq!(text(&it, InfoType::Modified), format_ms(1));
        it.btime = 5;
        assert_eq!(text(&it, InfoType::Created), format_ms(5));
        it.extra.insert("duration".into(), 83.0.into());
        assert_eq!(text(&it, InfoType::Dimensions), "1:23");
        it.extra.remove("duration");
        it.extra.insert("noPreview".into(), true.into());
        assert_eq!(text(&it, InfoType::Dimensions), "2 KB");
    }

    #[test]
    fn layouts_are_remembered_per_folder_then_per_library() {
        let mut saved = HashMap::new();
        let folder = Scope::Folder("F".into());
        assert_eq!(layout_for(&saved, &Scope::All, "/lib", None), Layout::Justified, "Eagle's default");
        assert_eq!(layout_for(&saved, &Scope::All, "/lib", Some(Layout::Grid)), Layout::Grid);
        saved.insert(layout_key(&Scope::All, "/lib"), Layout::List);
        assert_eq!(layout_for(&saved, &Scope::Trash, "/lib", None), Layout::List, "every page shares the library's");
        assert_eq!(layout_for(&saved, &folder, "/lib", None), Layout::List, "a folder without its own");
        saved.insert(layout_key(&folder, "/lib"), Layout::Waterfall);
        assert_eq!(layout_for(&saved, &folder, "/lib", None), Layout::Waterfall);
        assert_eq!(layout_for(&saved, &Scope::All, "/other", None), Layout::Justified);
    }

    #[test]
    fn subfolder_content_widens_a_folder() {
        let mut top = Folder::new("T", "Top", 0);
        let mut mid = Folder::new("M", "Mid", 0);
        mid.children.push(Folder::new("L", "Leaf", 0));
        top.children.push(mid);
        top.children.push(Folder::new("S", "Side", 0));
        let roots = vec![top, Folder::new("O", "Other", 0)];
        let mk = |id: &str, folders: &[&str], deleted: bool| {
            let mut it = item();
            it.id = id.into();
            it.folders = folders.iter().map(|s| s.to_string()).collect();
            it.is_deleted = deleted;
            it
        };
        let items = vec![mk("a", &["T"], false), mk("b", &["M"], false), mk("c", &["L", "T"], false), mk("d", &["O"], false), mk("e", &["L"], true)];
        let sorted = |set: HashSet<String>| {
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort();
            v
        };
        assert_eq!(sorted(folder_members(&items, &roots, "T", false)), ["a", "c"]);
        assert_eq!(sorted(folder_members(&items, &roots, "T", true)), ["a", "b", "c"], "trashed items stay out");
        assert_eq!(sorted(folder_members(&items, &roots, "M", true)), ["b", "c"]);
        assert_eq!(sorted(folder_tree(&roots, "M")), ["L", "M"]);
        assert!(folder_tree(&roots, "nope").is_empty());
        let names = |v: Vec<&Folder>| v.into_iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(",");
        assert_eq!(names(subfolder_cards(&roots[0], false)), "M,S");
        assert_eq!(names(subfolder_cards(&roots[0], true)), "M,L,S");
    }

    #[test]
    fn list_columns_and_header_sort() {
        let mut cols = ListColumns::default();
        let names = |c: &ListColumns| c.spans().into_iter().map(|s| s.0.label()).collect::<Vec<_>>();
        assert_eq!(names(&cols), ["Name", "Dimensions", "Type", "File Size", "Date Imported"], "Eagle's default columns");
        *cols.flag(ListColumn::Tags).unwrap() = true;
        *cols.flag(ListColumn::Size).unwrap() = false;
        assert_eq!(names(&cols), ["Name", "Tags", "Dimensions", "Type", "Date Imported"]);
        assert!(cols.flag(ListColumn::Name).is_none(), "the name always shows");
        let spans = cols.spans();
        let end = spans.last().map(|s| s.1 + s.2).unwrap();
        assert!((end - 1.0).abs() < 1e-5, "the columns fill the row");
        assert_eq!(header_sort(Some((OrderBy::Name, true)), OrderBy::Name), (OrderBy::Name, false));
        assert_eq!(header_sort(Some((OrderBy::Name, false)), OrderBy::FileSize), (OrderBy::FileSize, true));
        assert_eq!(header_sort(None, OrderBy::Import), (OrderBy::Import, true));
    }

    #[test]
    fn thumbnail_backgrounds_use_eagles_key() {
        let mut it: Item = serde_json::from_value(serde_json::json!({
            "id": "A", "name": "n", "size": 1, "btime": 0, "mtime": 0, "ext": "png", "tags": [], "folders": [],
            "isDeleted": false, "url": "", "annotation": "", "modificationTime": 1, "lastModified": 1
        }))
        .unwrap();
        assert_eq!(ThumbBg::of(&it), None);
        assert!(set_thumb_bg(&mut it, Some(ThumbBg::Grid)));
        assert_eq!(it.extra.get("background").and_then(|v| v.as_str()), Some("grid"));
        assert_eq!(ThumbBg::of(&it), Some(ThumbBg::Grid));
        assert!(!set_thumb_bg(&mut it, Some(ThumbBg::Grid)), "no change");
        assert!(set_thumb_bg(&mut it, None));
        assert!(!it.extra.contains_key("background"), "none removes the key, as Eagle does");
        it.extra.insert("background".into(), "plaid".into());
        assert_eq!(ThumbBg::of(&it), None);
    }

    #[test]
    fn pin_badge_only_in_the_pinned_folder() {
        let item = |pinned: serde_json::Value| -> Item {
            serde_json::from_value(serde_json::json!({"id": "A", "name": "a", "size": 1, "btime": 1, "mtime": 1, "ext": "png", "tags": [], "folders": ["F"], "isDeleted": false, "url": "", "annotation": "", "modificationTime": 1, "height": 1, "width": 1, "lastModified": 1, "palettes": [], "pinned": pinned})).unwrap()
        };
        let it = item(serde_json::json!({"F": 1700000000000.0, "G": 0}));
        assert!(pinned_here(&it, Some("F"), false));
        assert!(!pinned_here(&it, Some("F"), true), "random sort hides it");
        assert!(!pinned_here(&it, Some("G"), false), "a zero stamp is not a pin");
        assert!(!pinned_here(&it, Some("H"), false));
        assert!(!pinned_here(&it, None, false), "other views and combined folders show none");
    }
}
