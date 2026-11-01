//! On-disk types for an Eagle library. Field order mirrors Eagle's own key
//! order so that compact serialization is byte-identical. Unknown keys are
//! kept in `extra` and written after the known ones.

use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Number, Value};

/// Unknown JSON keys, preserved verbatim.
pub type Extra = Map<String, Value>;

/// Top-level `metadata.json`: the folder tree and library-wide settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryMetadata {
    /// Root folders.
    pub folders: Vec<Folder>,
    /// Smart folders (shape not yet observed; kept as raw JSON).
    pub smart_folders: Vec<Value>,
    /// Quick-access entries (raw JSON).
    pub quick_access: Vec<Value>,
    /// Tag groups, in sidebar order.
    pub tags_groups: Vec<TagGroup>,
    /// Last modification time, ms since epoch.
    pub modification_time: u64,
    /// Version of Eagle that wrote the library.
    pub application_version: String,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A node in the folder tree.
///
/// Eagle writes the keys in this order: `id`, `name`, `description`,
/// `children`, `modificationTime`, `tags`, `extendTags`, `icon`, `iconColor`,
/// `pinyin`, `password`, `passwordTips`, `coverId`, then `orderBy` and
/// `sortIncrease` when the folder has its own sort. Optional keys are left
/// out when unset, as `JSON.stringify` drops `undefined`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Folder id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Free-form description.
    pub description: String,
    /// Child folders.
    pub children: Vec<Folder>,
    /// Creation time, ms since epoch. Eagle sets it when the folder is
    /// created or duplicated and leaves it alone on later edits.
    pub modification_time: u64,
    /// Tags attached to the folder.
    pub tags: Vec<String>,
    /// The folder's tags plus its ancestors'. A runtime cache Eagle's writer
    /// strips; kept only so a file holding it round-trips.
    pub extend_tags: Option<Vec<String>>,
    /// Icon name from [`FOLDER_ICONS`]; absent for the default folder icon.
    pub icon: Option<String>,
    /// Icon colour from [`ICON_COLORS`]; absent for the default colour.
    pub icon_color: Option<String>,
    /// Romanised name for search. Also a runtime cache Eagle's writer strips.
    pub pinyin: Option<String>,
    /// Base64 of the password's Latin-1 bytes, or empty when unset. See
    /// [`Folder::is_locked`].
    pub password: String,
    /// Password hint (empty when unset).
    pub password_tips: String,
    /// Id of the item shown as the folder's cover.
    pub cover_id: Option<String>,
    /// The folder's own sort key (see [`FolderOrder`]); absent to follow the
    /// global sort.
    pub order_by: Option<String>,
    /// Ascending when true; written alongside `orderBy`.
    pub sort_increase: Option<bool>,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Serialize for Folder {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("id", &self.id)?;
        m.serialize_entry("name", &self.name)?;
        m.serialize_entry("description", &self.description)?;
        m.serialize_entry("children", &self.children)?;
        m.serialize_entry("modificationTime", &self.modification_time)?;
        m.serialize_entry("tags", &self.tags)?;
        if let Some(v) = &self.extend_tags {
            m.serialize_entry("extendTags", v)?;
        }
        if let Some(v) = &self.icon {
            m.serialize_entry("icon", v)?;
        }
        if let Some(v) = &self.icon_color {
            m.serialize_entry("iconColor", v)?;
        }
        if let Some(v) = &self.pinyin {
            m.serialize_entry("pinyin", v)?;
        }
        m.serialize_entry("password", &self.password)?;
        m.serialize_entry("passwordTips", &self.password_tips)?;
        if let Some(v) = &self.cover_id {
            m.serialize_entry("coverId", v)?;
        }
        if let Some(v) = &self.order_by {
            m.serialize_entry("orderBy", v)?;
        }
        if let Some(v) = &self.sort_increase {
            m.serialize_entry("sortIncrease", v)?;
        }
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl Folder {
    /// A new, empty folder in the shape Eagle writes.
    pub fn new(id: impl Into<String>, name: impl Into<String>, modification_time: u64) -> Folder {
        Folder {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            children: Vec::new(),
            modification_time,
            tags: Vec::new(),
            extend_tags: None,
            icon: None,
            icon_color: None,
            pinyin: None,
            password: String::new(),
            password_tips: String::new(),
            cover_id: None,
            order_by: None,
            sort_increase: None,
            extra: Extra::new(),
        }
    }

    /// Visits this folder and all descendants, depth first.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Folder)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }

    /// Whether the folder has a password.
    pub fn is_locked(&self) -> bool {
        !self.password.is_empty()
    }

    /// The folder's own sort, if it has one Eagle knows.
    pub fn order(&self) -> Option<FolderOrder> {
        self.order_by.as_deref().and_then(FolderOrder::parse)
    }
}

/// Icon colours Eagle offers for folders, smart folders and tag groups, in
/// menu order. No colour key means the default (grey).
pub const ICON_COLORS: [&str; 8] = ["red", "orange", "yellow", "green", "aqua", "blue", "purple", "pink"];

/// Icon names Eagle's folder and smart-folder icon picker offers, in picker
/// order (spellings as Eagle stores them). No `icon` key means the default
/// folder icon.
pub const FOLDER_ICONS: &[&str] = &[
    "library", "box", "grid", "layer", "briefcase", "photo", "photos", "video", "film", "film2", "film3", "music",
    "book", "book2", "bookshelf", "keynote", "camera", "aperture", "attachment", "scissors", "palette", "wrench",
    "helmet", "life-buoy", "graph", "graph2", "tableware", "cog", "bachelor-cap", "cones", "dribbble", "email",
    "business-card", "coffee", "cart", "lightbulb", "inspiration", "thumb-up", "thumb-down", "like", "unlike", "star",
    "hot", "upload", "download", "paid", "free", "medical", "shield", "search", "shortcuts", "recycle", "excalmation",
    "question", "coin1", "coin2", "coin3", "coin4", "wallet", "watch", "clock", "calendar-week", "calendar-month",
    "website", "phone", "tablet", "desktop", "tv", "cpu", "safari", "chrome", "pie", "bar-chart", "line-chart", "2d",
    "3d", "contrast", "texture", "transition", "animation", "spectrogram", "font-sans", "font-sans-serif",
    "font-handwritten", "flag", "earth", "pin", "pin-check", "map", "road", "motor", "rocket", "airplan", "ship",
    "train", "car", "truck", "water-drop", "sun", "moon", "tree", "mountain", "cloud", "bathtub", "door", "bed",
    "cabinet", "sofa", "home", "store", "house", "game", "weapon", "armor", "award2", "award", "award3", "crown",
    "rophy", "gift", "facebook", "twitter", "instagram", "eye", "bear", "dog", "cat", "man", "woman", "group", "smile",
    "meh", "frown", "die", "baby", "kid", "angel", "demon", "hand", "brain", "number0", "number1", "number2", "number3",
    "number4", "number5", "number6", "number7", "number8", "number9", "number10", "number11", "number12", "number13",
    "number14", "number15", "number16", "number17", "number18", "number19", "number20",
];

/// A tag group from `tagsGroups` in the library `metadata.json`. Eagle
/// writes `id`, `name`, `tags`, then `color` when set and `description` when
/// defined.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TagGroup {
    /// Group id, in the same format as item ids.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Member tags, in the group's order.
    pub tags: Vec<String>,
    /// Colour from [`ICON_COLORS`].
    pub color: Option<String>,
    /// Description (may be present and empty).
    pub description: Option<String>,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Serialize for TagGroup {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("id", &self.id)?;
        m.serialize_entry("name", &self.name)?;
        m.serialize_entry("tags", &self.tags)?;
        if let Some(v) = &self.color {
            m.serialize_entry("color", v)?;
        }
        if let Some(v) = &self.description {
            m.serialize_entry("description", v)?;
        }
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

/// A folder's or smart folder's own sort key, stored as `orderBy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderOrder {
    /// One of the keys the main list also sorts by.
    By(crate::search::OrderBy),
    /// Video and audio length.
    Duration,
    /// Shuffled on every visit.
    Random,
    /// Hand-arranged order, kept per folder in each item's `order` map.
    Manual,
}

impl From<crate::search::OrderBy> for FolderOrder {
    fn from(o: crate::search::OrderBy) -> Self {
        match o {
            crate::search::OrderBy::Duration => FolderOrder::Duration,
            crate::search::OrderBy::Random => FolderOrder::Random,
            o => FolderOrder::By(o),
        }
    }
}

impl FolderOrder {
    /// Eagle's string for this key.
    pub fn as_str(self) -> &'static str {
        use crate::search::OrderBy as O;
        match self {
            FolderOrder::By(O::Import) => "IMPORT",
            FolderOrder::By(O::Name) => "NAME",
            FolderOrder::By(O::Ext) => "EXT",
            FolderOrder::By(O::Resolution) => "RESOLUTION",
            FolderOrder::By(O::FileSize) => "FILESIZE",
            FolderOrder::By(O::Rating) => "RATING",
            FolderOrder::By(O::Btime) => "BTIME",
            FolderOrder::By(O::Mtime) => "MTIME",
            FolderOrder::By(O::Tags) => "TAGS",
            FolderOrder::Duration | FolderOrder::By(O::Duration) => "DURATION",
            FolderOrder::Random | FolderOrder::By(O::Random) => "RANDOM",
            FolderOrder::Manual => "MANUAL",
        }
    }

    /// Parses Eagle's string; `None` for anything unknown.
    pub fn parse(s: &str) -> Option<FolderOrder> {
        use crate::search::OrderBy as O;
        Some(match s {
            "IMPORT" => O::Import.into(),
            "NAME" => O::Name.into(),
            "EXT" => O::Ext.into(),
            "RESOLUTION" => O::Resolution.into(),
            "FILESIZE" => O::FileSize.into(),
            "RATING" => O::Rating.into(),
            "BTIME" => O::Btime.into(),
            "MTIME" => O::Mtime.into(),
            "TAGS" => O::Tags.into(),
            "DURATION" => FolderOrder::Duration,
            "RANDOM" => FolderOrder::Random,
            "MANUAL" => FolderOrder::Manual,
            _ => return None,
        })
    }
}

/// One colour of an item's palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    /// RGB triple.
    pub color: [u8; 3],
    /// Share of the image; Eagle writes integers or decimals, so the exact
    /// JSON number is kept.
    pub ratio: Number,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// An item's `images/<ID>.info/metadata.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", from = "ItemRepr")]
pub struct Item {
    /// Item id.
    pub id: String,
    /// Name without extension.
    pub name: String,
    /// Original file size in bytes.
    pub size: u64,
    /// Creation time, ms since epoch.
    pub btime: u64,
    /// File modification time, ms since epoch.
    pub mtime: u64,
    /// File extension without the dot.
    pub ext: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Ids of the folders containing the item.
    pub folders: Vec<String>,
    /// Whether the item is in the trash.
    pub is_deleted: bool,
    /// Source URL.
    pub url: String,
    /// Annotation text.
    pub annotation: String,
    /// Last modification time of the record, ms since epoch.
    pub modification_time: u64,
    /// Present only on some items.
    pub remove_thumbnail: Option<bool>,
    /// Present only on some items.
    pub no_thumbnail: Option<bool>,
    /// Pixel height.
    pub height: u64,
    /// Pixel width.
    pub width: u64,
    /// Last modification time of the content, ms since epoch.
    pub last_modified: u64,
    /// Dominant colours.
    pub palettes: Vec<Palette>,
    /// The keys of the `metadata.json` this item was read from, in file
    /// order (empty for new items). Writing follows this order, and a field
    /// the file didn't have is left out while it still holds its default,
    /// so items round-trip byte for byte. Eagle leaves out `width`,
    /// `height` and `palettes` for files it can't preview, and writes keys
    /// such as `noPreview` and `deletedTime` mid-record.
    pub key_order: KeyOrder,
    /// Unknown keys.
    pub extra: Extra,
}

/// An item's original key order. Only formatting, so it never makes two
/// items unequal.
#[derive(Debug, Clone, Default)]
pub struct KeyOrder(pub Vec<String>);

impl PartialEq for KeyOrder {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::ops::Deref for KeyOrder {
    type Target = Vec<String>;
    fn deref(&self) -> &Vec<String> {
        &self.0
    }
}

impl std::ops::DerefMut for KeyOrder {
    fn deref_mut(&mut self) -> &mut Vec<String> {
        &mut self.0
    }
}

/// Deserialization shape for [`Item`]; `key_order` is filled in afterwards.
/// Every field but `id` may be missing.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemRepr {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    btime: u64,
    #[serde(default)]
    mtime: u64,
    #[serde(default)]
    ext: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    folders: Vec<String>,
    #[serde(default)]
    is_deleted: bool,
    #[serde(default)]
    url: String,
    #[serde(default)]
    annotation: String,
    #[serde(default)]
    modification_time: u64,
    remove_thumbnail: Option<bool>,
    no_thumbnail: Option<bool>,
    #[serde(default)]
    height: u64,
    #[serde(default)]
    width: u64,
    #[serde(default)]
    last_modified: u64,
    #[serde(default)]
    palettes: Vec<Palette>,
    #[serde(flatten)]
    extra: Extra,
}

impl From<ItemRepr> for Item {
    fn from(r: ItemRepr) -> Self {
        Item {
            id: r.id,
            name: r.name,
            size: r.size,
            btime: r.btime,
            mtime: r.mtime,
            ext: r.ext,
            tags: r.tags,
            folders: r.folders,
            is_deleted: r.is_deleted,
            url: r.url,
            annotation: r.annotation,
            modification_time: r.modification_time,
            remove_thumbnail: r.remove_thumbnail,
            no_thumbnail: r.no_thumbnail,
            height: r.height,
            width: r.width,
            last_modified: r.last_modified,
            palettes: r.palettes,
            key_order: KeyOrder::default(),
            extra: r.extra,
        }
    }
}

/// One entry of an item's `comments`: a region note on an image or a
/// timestamped note on a video or audio file. Eagle writes `id`, then `x`,
/// `y`, `width`, `height` (image) or `duration` (video), then `annotation`
/// and `lastModified`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    /// Comment id, in the item id format.
    #[serde(default)]
    pub id: String,
    /// Region left edge in the original image's pixels (may be fractional
    /// after a drag).
    pub x: Option<Number>,
    /// Region top edge in original-image pixels.
    pub y: Option<Number>,
    /// Region width in original-image pixels.
    pub width: Option<Number>,
    /// Region height in original-image pixels.
    pub height: Option<Number>,
    /// Playback position in seconds, for video and audio notes.
    pub duration: Option<Number>,
    /// The note as HTML from Eagle's editable box (`<br>` for line breaks).
    #[serde(default)]
    pub annotation: String,
    /// Creation time, ms since epoch. Eagle does not bump it on edits.
    pub last_modified: Option<u64>,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Serialize for Comment {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("id", &self.id)?;
        for (k, v) in [("x", &self.x), ("y", &self.y), ("width", &self.width), ("height", &self.height), ("duration", &self.duration)] {
            if let Some(v) = v {
                m.serialize_entry(k, v)?;
            }
        }
        m.serialize_entry("annotation", &self.annotation)?;
        if let Some(v) = &self.last_modified {
            m.serialize_entry("lastModified", v)?;
        }
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl Comment {
    /// A region note on an image, in original-image pixels.
    pub fn region(x: u64, y: u64, width: u64, height: u64, annotation: &str) -> Comment {
        Comment { x: Some(x.into()), y: Some(y.into()), width: Some(width.into()), height: Some(height.into()), annotation: annotation.into(), ..Default::default() }
    }

    /// A note at `seconds` into a video or audio file.
    pub fn at_time(seconds: f64, annotation: &str) -> Comment {
        Comment { duration: Number::from_f64(seconds), annotation: annotation.into(), ..Default::default() }
    }
}

impl Item {
    /// The item's comments (`comments` in `extra`), empty when it has none.
    /// Entries that are not objects are skipped.
    pub fn comments(&self) -> Vec<Comment> {
        let Some(Value::Array(list)) = self.extra.get("comments") else { return Vec::new() };
        list.iter().filter_map(|c| serde_json::from_value(c.clone()).ok()).collect()
    }

    /// Replaces the item's comments. The `comments` key keeps its place in
    /// `extra`, or is added at the end, as Eagle adds it.
    pub fn set_comments(&mut self, comments: &[Comment]) {
        let value = serde_json::to_value(comments).unwrap_or_else(|_| Value::Array(Vec::new()));
        match self.extra.get_mut("comments") {
            Some(slot) => *slot = value,
            None => {
                self.extra.insert("comments".into(), value);
            }
        }
    }

    /// Parses an item's `metadata.json`, recording its key order.
    pub fn from_json_str(s: &str) -> serde_json::Result<Item> {
        let value: Value = serde_json::from_str(s)?;
        let key_order = value.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let mut item: Item = serde_json::from_value(value)?;
        item.key_order = KeyOrder(key_order);
        Ok(item)
    }

    /// The fields to write, in Eagle's order for new items: known fields,
    /// then unknown keys. Known fields the source file lacked are dropped
    /// while they hold their default.
    fn entries(&self) -> Vec<(String, Value)> {
        let v = |x: &dyn erased::ToValue| x.to_value();
        let mut out: Vec<(String, Value, bool)> = vec![
            ("id".into(), v(&self.id), false),
            ("name".into(), v(&self.name), self.name.is_empty()),
            ("size".into(), v(&self.size), self.size == 0),
            ("btime".into(), v(&self.btime), self.btime == 0),
            ("mtime".into(), v(&self.mtime), self.mtime == 0),
            ("ext".into(), v(&self.ext), self.ext.is_empty()),
            ("tags".into(), v(&self.tags), self.tags.is_empty()),
            ("folders".into(), v(&self.folders), self.folders.is_empty()),
            ("isDeleted".into(), v(&self.is_deleted), !self.is_deleted),
            ("url".into(), v(&self.url), self.url.is_empty()),
            ("annotation".into(), v(&self.annotation), self.annotation.is_empty()),
            ("modificationTime".into(), v(&self.modification_time), self.modification_time == 0),
        ];
        if let Some(x) = &self.remove_thumbnail {
            out.push(("removeThumbnail".into(), v(x), false));
        }
        if let Some(x) = &self.no_thumbnail {
            out.push(("noThumbnail".into(), v(x), false));
        }
        out.push(("height".into(), v(&self.height), self.height == 0));
        out.push(("width".into(), v(&self.width), self.width == 0));
        out.push(("lastModified".into(), v(&self.last_modified), self.last_modified == 0));
        out.push(("palettes".into(), v(&self.palettes), self.palettes.is_empty()));
        let fresh = self.key_order.is_empty();
        let mut out: Vec<(String, Value)> = out
            .into_iter()
            .filter(|(k, _, default)| fresh || !default || self.key_order.contains(k))
            .map(|(k, val, _)| (k, val))
            .collect();
        out.extend(self.extra.iter().map(|(k, val)| (k.clone(), val.clone())));
        if !fresh {
            // Keys from the file first, in file order; anything new after.
            let rank = |k: &str| self.key_order.iter().position(|x| x == k).unwrap_or(usize::MAX);
            out.sort_by_key(|(k, _)| rank(k)); // stable: new keys keep their order
        }
        out
    }
}

impl Serialize for Item {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        for (k, v) in self.entries() {
            m.serialize_entry(&k, &v)?;
        }
        m.end()
    }
}

/// Field-to-JSON conversion used by [`Item::entries`].
mod erased {
    pub trait ToValue {
        fn to_value(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> ToValue for T {
        fn to_value(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
        }
    }
}

/// One entry of `saved-filters.json`: a name and a snapshot of the filter
/// panel's rules. Entries have no id; Eagle finds them by position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedFilter {
    /// Display name.
    pub name: String,
    /// The panel's rule object at save time (see [`SavedFilter::to_filter`]),
    /// plus `keyword` when a search was active. Kept as raw JSON so every
    /// rule Eagle knows survives.
    pub rule: Value,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A saved filter read into falcon's search types.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavedFilterQuery {
    /// The rules falcon's filter can express.
    pub filter: crate::search::Filter,
    /// The search text saved with the filter.
    pub keyword: Option<String>,
    /// Active rules falcon cannot express, by their rule key (`image` and
    /// `semantic`, Eagle's AI searches).
    pub unsupported: Vec<String>,
}

impl SavedFilter {
    /// Reads the saved rules into a [`crate::search::Filter`]. Eagle does not
    /// save whether tags and folders combine with OR, AND or EQUAL (that
    /// setting lives in the panel), so they come back as OR, the panel's
    /// starting value. A font rule reads the activated fonts from Eagle's
    /// font folder and falcon's ([`crate::search::font_dirs`]).
    pub fn to_filter(&self) -> SavedFilterQuery {
        use crate::search::{font_dirs, installed_font_keys_in};
        self.to_filter_with_fonts(|| installed_font_keys_in(&font_dirs()))
    }

    /// [`SavedFilter::to_filter`] with the activated font keys supplied;
    /// `font_keys` is only called when the font rule is on.
    pub fn to_filter_with_fonts(&self, font_keys: impl FnOnce() -> std::collections::HashSet<String>) -> SavedFilterQuery {
        use crate::search::{ColorQuery, DateFilter, Filter, FontFilter, SetFilter, Shape, TextFilter};
        let r = &self.rule;
        let get = |path: &[&str]| path.iter().try_fold(r, |v, k| v.get(*k));
        let truthy = |path: &[&str]| get(path).is_some_and(|v| v.as_bool() == Some(true));
        let keys = |path: &[&str]| -> Vec<String> {
            get(path).and_then(Value::as_object).map(|m| m.iter().filter(|(_, v)| !v.is_null() && v.as_bool() != Some(false)).map(|(k, _)| k.clone()).collect()).unwrap_or_default()
        };
        let strings = |path: &[&str]| -> Vec<String> {
            get(path).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
        };
        // `$.isNumeric` accepts numbers and numeric strings.
        let number = |path: &[&str]| -> Option<f64> {
            get(path).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        };
        let mut out = SavedFilterQuery::default();
        let mut f = Filter::default();
        let mut skip = |k: &str| out.unsupported.push(k.to_string());

        f.types = keys(&["type", "includes"]);
        f.exclude_types = keys(&["type", "excludes"]);
        f.tags = SetFilter { includes: strings(&["tag", "includes"]), excludes: strings(&["tag", "excludes"]), empty: truthy(&["tag", "no"]), ..Default::default() };
        let (inc, exc) = (keys(&["folder", "includes"]), keys(&["folder", "excludes"]));
        f.folders = SetFilter {
            empty: inc.iter().any(|k| k == "NoFolders"),
            exclude_empty: exc.iter().any(|k| k == "NoFolders"),
            includes: inc.into_iter().filter(|k| k != "NoFolders").collect(),
            excludes: exc.into_iter().filter(|k| k != "NoFolders").collect(),
            ..Default::default()
        };

        let unit = if get(&["file", "unit"]).and_then(Value::as_str) == Some("mb") { 1_048_576.0 } else { 1024.0 };
        f.size = (number(&["file", "min"]).map(|v| (v * unit) as u64), number(&["file", "max"]).map(|v| (v * unit) as u64));
        let px = |k: &str| number(&["resolution", k]).map(|v| v as u64);
        f.width = (px("minW"), px("maxW"));
        f.height = (px("minH"), px("maxH"));

        for star in 0..=5u8 {
            if truthy(&["rating", &star.to_string()]) {
                f.ratings.insert(star);
            }
        }
        for (key, shape) in [
            ("portrait", Shape::Portrait),
            ("landscape", Shape::Landscape),
            ("square", Shape::Square),
            ("panoramicPortrait", Shape::PanoramicPortrait),
            ("panoramicLandscape", Shape::PanoramicLandscape),
            ("43", Shape::Ratio4x3),
            ("34", Shape::Ratio3x4),
            ("169", Shape::Ratio16x9),
            ("916", Shape::Ratio9x16),
        ] {
            if truthy(&["shape", key]) {
                f.shapes.insert(shape);
            }
        }
        if truthy(&["shape", "custom"]) {
            f.custom_shape = Some((number(&["shape", "width"]).unwrap_or(0.0), number(&["shape", "height"]).unwrap_or(0.0)));
        }

        if let Some(rgb) = get(&["color", "value"]).and_then(Value::as_array).filter(|a| a.len() >= 3) {
            let c = |i: usize| rgb[i].as_f64().unwrap_or(0.0).clamp(0.0, 255.0) as u8;
            f.color = Some(ColorQuery { rgb: [c(0), c(1), c(2)], accuracy: number(&["color", "accuracy"]).unwrap_or(20.0) });
        }
        f.grayscale = truthy(&["color", "gray"]);

        let text = |k: &str| -> TextFilter {
            if truthy(&[k, "has"]) {
                match get(&[k, "keywords"]).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    Some(words) => TextFilter::Keywords(words.split(',').map(str::to_string).collect()),
                    None => TextFilter::Has,
                }
            } else if truthy(&[k, "no"]) {
                TextFilter::No
            } else {
                TextFilter::Off
            }
        };
        f.url = text("url");
        f.note = text("note");
        f.comments = text("annotation");

        // Dates. Eagle's Date Modified range reads `import.range` (its panel
        // has one picker for both); falcon writes `mtime.range` only when
        // the two ranges differ. `btime` (Date Created) is falcon's own key.
        let date = |k: &str, range_keys: &[&str]| -> DateFilter {
            let mut d = DateFilter::default();
            for (key, p) in DATE_PRESET_KEYS {
                if truthy(&[k, key]) {
                    d.presets.insert(p);
                }
            }
            if truthy(&[k, "usingRange"]) {
                // Eagle passes nothing through a range that lacks an end,
                // which `DateFilter` does for a zero end.
                let ends = range_keys.iter().find_map(|rk| get(&[rk, "range"]).and_then(Value::as_array)).filter(|a| a.len() >= 2);
                let end = |v: &Value| v.as_f64().filter(|t| *t > 0.0).map_or(0, |t| t as u64);
                d.range = Some(ends.map_or((0, 0), |a| (end(&a[0]), end(&a[1]))));
            }
            d.months = keys(&[k, "selectedMonths"]).iter().filter_map(|m| parse_month_key(m)).collect();
            d
        };
        f.imported = date("import", &["import"]);
        f.modified = date("mtime", &["mtime", "import"]);
        f.created = date("btime", &["btime"]);

        // Eagle multiplies each bound by the unit, then truncates (as the
        // filter does).
        let per = match get(&["duration", "unit"]).and_then(Value::as_str) {
            Some("h") => 3600.0,
            Some("m") => 60.0,
            _ => 1.0,
        };
        f.duration = (number(&["duration", "min"]).map(|v| v * per), number(&["duration", "max"]).map(|v| v * per));
        f.bpm = (number(&["bpm", "min"]), number(&["bpm", "max"]));
        f.cameras = keys(&["camera"]).into_iter().collect();
        // Eagle tests `activated` first.
        if truthy(&["font", "activated"]) {
            f.font = FontFilter::Activated(font_keys());
        } else if truthy(&["font", "deactivated"]) {
            f.font = FontFilter::Deactivated(font_keys());
        }

        // Rules with no counterpart in falcon's filter yet.
        if get(&["image", "itemId"]).is_some_and(|v| !v.is_null()) {
            skip("image");
        }
        if get(&["semantic", "value"]).is_some_and(|v| !v.is_null()) {
            skip("semantic");
        }

        out.filter = f;
        out.keyword = r.get("keyword").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        out
    }

    /// Builds the rule object Eagle's panel would save for `filter`: its
    /// full default rule set (as `JSON.stringify` writes it, integer-like
    /// keys first) with falcon's rules filled in. `folder_name` looks up a
    /// folder's name; Eagle stores whole folder objects under
    /// `folder.includes`, of which it only reads `id` back, so falcon writes
    /// `{id, name}`. The tag and folder logic is not part of the rule.
    pub fn rule_from_filter(filter: &crate::search::Filter, keyword: Option<&str>, folder_name: impl Fn(&str) -> Option<String>) -> Value {
        use crate::search::{DateFilter, FontFilter, Shape, TextFilter};
        let mut r = default_filter_rule();
        let obj = |list: &[String]| Value::Object(list.iter().map(|t| (t.clone(), Value::Bool(true))).collect());
        r["type"]["includes"] = obj(&filter.types);
        r["type"]["excludes"] = obj(&filter.exclude_types);
        r["tag"]["no"] = filter.tags.empty.into();
        r["tag"]["includes"] = filter.tags.includes.clone().into();
        r["tag"]["excludes"] = filter.tags.excludes.clone().into();
        let folder = |id: &str| {
            let mut m = Map::new();
            m.insert("id".into(), id.into());
            if let Some(n) = folder_name(id) {
                m.insert("name".into(), n.into());
            }
            Value::Object(m)
        };
        let mut inc: Map<String, Value> = filter.folders.includes.iter().map(|id| (id.clone(), folder(id))).collect();
        if filter.folders.empty {
            inc.insert("NoFolders".into(), serde_json::json!({"id": "NoFolders"}));
        }
        r["folder"]["includes"] = Value::Object(inc);
        let mut exc: Map<String, Value> = filter.folders.excludes.iter().map(|id| (id.clone(), folder(id))).collect();
        if filter.folders.exclude_empty {
            exc.insert("NoFolders".into(), serde_json::json!({"id": "NoFolders"}));
        }
        r["folder"]["excludes"] = Value::Object(exc);
        for star in &filter.ratings {
            r["rating"][star.to_string()] = true.into();
        }
        for s in &filter.shapes {
            let key = match s {
                Shape::Portrait => "portrait",
                Shape::Landscape => "landscape",
                Shape::Square => "square",
                Shape::PanoramicPortrait => "panoramicPortrait",
                Shape::PanoramicLandscape => "panoramicLandscape",
                Shape::Ratio4x3 => "43",
                Shape::Ratio3x4 => "34",
                Shape::Ratio16x9 => "169",
                Shape::Ratio9x16 => "916",
            };
            r["shape"][key] = true.into();
        }
        if let Some((w, h)) = filter.custom_shape {
            r["shape"]["custom"] = true.into();
            r["shape"]["width"] = js_number(w);
            r["shape"]["height"] = js_number(h);
        }
        if let Some(c) = &filter.color {
            let color = r["color"].as_object_mut().expect("default color rule");
            color.shift_insert(1, "value".into(), serde_json::json!(c.rgb));
            color["accuracy"] = js_number(c.accuracy);
        }
        r["color"]["gray"] = filter.grayscale.into();
        // Sizes go in KB, which Eagle multiplies back by 1024.
        let kb = |b: u64| js_number(b as f64 / 1024.0);
        let file = r["file"].as_object_mut().expect("default file rule");
        if let Some(b) = filter.size.0 {
            file.shift_insert(0, "min".into(), kb(b));
        }
        if let Some(b) = filter.size.1 {
            let at = usize::from(filter.size.0.is_some());
            file.shift_insert(at, "max".into(), kb(b));
        }
        for (k, v) in [("minW", filter.width.0), ("maxW", filter.width.1), ("minH", filter.height.0), ("maxH", filter.height.1)] {
            if let Some(v) = v {
                r["resolution"][k] = v.into();
            }
        }
        let text = |r: &mut Value, k: &str, t: &TextFilter| match t {
            TextFilter::Off => {}
            TextFilter::Has => r[k]["has"] = true.into(),
            TextFilter::No => r[k]["no"] = true.into(),
            TextFilter::Keywords(words) => {
                r[k]["has"] = true.into();
                r[k]["keywords"] = words.join(",").into();
            }
        };
        for (k, v) in [("min", filter.bpm.0), ("max", filter.bpm.1)] {
            if let Some(v) = v {
                r["bpm"][k] = js_number(v);
            }
        }
        text(&mut r, "annotation", &filter.comments);
        text(&mut r, "url", &filter.url);
        text(&mut r, "note", &filter.note);

        // Dates: presets, then the range in the place Eagle's defaults give
        // it, then months.
        let date = |rule: &mut Value, d: &DateFilter, range: Option<(u64, u64)>| {
            for (key, p) in DATE_PRESET_KEYS {
                if d.presets.contains(&p) {
                    rule[key] = true.into();
                }
            }
            if d.range.is_some() {
                rule["usingRange"] = true.into();
            }
            if let Some((a, b)) = range {
                let m = rule.as_object_mut().expect("default date rule");
                let at = m.keys().position(|k| k == "type").unwrap_or(m.len());
                m.shift_insert(at, "range".into(), serde_json::json!([a, b]));
            }
            let mut months: Vec<_> = d.months.iter().copied().collect();
            months.sort_unstable();
            rule["selectedMonths"] = Value::Object(months.into_iter().map(|(y, m)| (format!("{y:04}/{m:02}"), Value::Bool(true))).collect());
        };
        // Eagle's panel stores one picked range, in `import`, and its
        // Date Modified rule reads it from there.
        let shared = filter.imported.range.or(filter.modified.range);
        date(&mut r["import"], &filter.imported, shared);
        if shared.is_some() {
            r["import"]["type"] = "range".into();
        }
        let own = filter.modified.range.filter(|m| Some(*m) != shared);
        date(&mut r["mtime"], &filter.modified, own);

        // Each bound in the largest unit that keeps it whole.
        if filter.duration != (None, None) {
            let bounds = [filter.duration.0, filter.duration.1];
            let whole = |per: f64| bounds.iter().flatten().all(|s| (s / per).fract() == 0.0);
            let (unit, per) = if whole(3600.0) {
                ("h", 3600.0)
            } else if whole(60.0) {
                ("m", 60.0)
            } else {
                ("s", 1.0)
            };
            let d = r["duration"].as_object_mut().expect("default duration rule");
            let mut at = 0;
            for (k, v) in [("min", filter.duration.0), ("max", filter.duration.1)] {
                if let Some(s) = v {
                    d.shift_insert(at, k.into(), js_number(s / per));
                    at += 1;
                }
            }
            d["unit"] = unit.into();
        }
        let mut cameras: Vec<&String> = filter.cameras.iter().collect();
        cameras.sort();
        r["camera"] = Value::Object(cameras.into_iter().map(|c| (c.clone(), Value::Bool(true))).collect());
        match filter.font {
            FontFilter::Off => {}
            FontFilter::Activated(_) => r["font"]["activated"] = true.into(),
            FontFilter::Deactivated(_) => r["font"]["deactivated"] = true.into(),
        }
        if filter.created.is_active() {
            let mut btime = default_filter_rule()["import"].take();
            date(&mut btime, &filter.created, filter.created.range);
            r["btime"] = btime;
        }
        if let Some(k) = keyword.filter(|k| !k.is_empty()) {
            r["keyword"] = k.into();
        }
        r
    }
}

/// The date presets under their rule keys, in Eagle's order.
const DATE_PRESET_KEYS: [(&str, crate::search::DatePreset); 6] = {
    use crate::search::DatePreset as P;
    [("today", P::Today), ("yesterday", P::Yesterday), ("last7day", P::Last7Days), ("last30day", P::Last30Days), ("last90day", P::Last90Days), ("last365day", P::Last365Days)]
};

/// A `selectedMonths` key, `YYYY/MM`.
fn parse_month_key(key: &str) -> Option<(i32, u32)> {
    let (y, m) = key.split_once('/')?;
    let m: u32 = m.parse().ok()?;
    let year = y.parse().ok()?;
    (1..=12).contains(&m).then_some((year, m))
}

/// A number as JavaScript prints it: whole values without a `.0`.
fn js_number(v: f64) -> Value {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9_007_199_254_740_992.0 {
        Value::from(v as i64)
    } else {
        Value::from(v)
    }
}

/// Eagle's default filter rules after `JSON.parse(JSON.stringify(..))`:
/// `undefined` values gone and integer-like keys first.
fn default_filter_rule() -> Value {
    let day = || {
        serde_json::json!({
            "today": false, "yesterday": false, "last7day": false, "last30day": false, "last90day": false,
            "last365day": false, "usingRange": false, "type": "undefined", "selectedMonths": {}
        })
    };
    let has = || serde_json::json!({"has": false, "no": false});
    serde_json::json!({
        "type": {"includes": {}, "excludes": {}},
        "tag": {"no": false, "includes": [], "excludes": []},
        "folder": {"includes": {}, "excludes": {}},
        "font": {"activated": false, "deactivated": false},
        "camera": {},
        "import": day(),
        "mtime": day(),
        "rating": {"0": false, "1": false, "2": false, "3": false, "4": false, "5": false},
        "color": {"gray": false, "accuracy": 20},
        "shape": {
            "34": false, "43": false, "169": false, "916": false, "portrait": false, "landscape": false,
            "square": false, "panoramicPortrait": false, "panoramicLandscape": false, "custom": false,
            "width": 4, "height": 3
        },
        "resolution": {},
        "file": {"unit": "kb"},
        "duration": {"unit": "s"},
        "bpm": {},
        "annotation": has(),
        "note": has(),
        "url": has(),
        "image": {},
        "semantic": {}
    })
}

/// `tags.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tags {
    /// Recently used tags.
    pub history_tags: Vec<String>,
    /// Starred tags.
    pub starred_tags: Vec<String>,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `mtime.json`: item id to modification time, followed by an `"all"` count.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MtimeIndex {
    /// `(item id, mtime)` in file order.
    pub entries: Vec<(String, u64)>,
    /// The trailing `"all"` count, if present.
    pub all: Option<u64>,
}

impl Serialize for MtimeIndex {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        for (k, v) in &self.entries {
            m.serialize_entry(k, v)?;
        }
        if let Some(all) = &self.all {
            m.serialize_entry("all", all)?;
        }
        m.end()
    }
}

impl<'de> Deserialize<'de> for MtimeIndex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let map = Map::<String, Value>::deserialize(d)?;
        let mut out = MtimeIndex::default();
        for (k, v) in map {
            let n = v
                .as_u64()
                .ok_or_else(|| serde::de::Error::custom(format!("mtime for {k} is not an integer")))?;
            if k == "all" {
                out.all = Some(n);
            } else {
                out.entries.push((k, n));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Records shaped like real ones from users' libraries.
    const EPUB: &str = r#"{"id":"MU2PQF5G9ZVDB","name":"book","size":46784743,"btime":1789479226928,"mtime":1789479229513,"ext":"epub","tags":[],"folders":[],"isDeleted":true,"url":"","annotation":"","modificationTime":1789479235636,"noPreview":true,"deletedTime":1789479244672,"lastModified":1789479244751}"#;
    const TEX: &str = r#"{"id":"MPLM58NXPSPTR","name":"main","size":1200,"btime":1,"mtime":2,"ext":"tex","tags":[],"folders":["F1"],"isDeleted":false,"url":"","annotation":"","modificationTime":3,"lastModified":4}"#;
    const WIDTH_FIRST: &str = r#"{"id":"A","name":"n","size":1,"btime":1,"mtime":1,"ext":"png","tags":[],"folders":[],"isDeleted":false,"url":"","annotation":"","modificationTime":1,"width":10,"height":20,"lastModified":1,"palettes":[]}"#;

    #[test]
    fn items_without_dimensions_round_trip() {
        for json in [EPUB, TEX, WIDTH_FIRST] {
            let item = Item::from_json_str(json).unwrap();
            assert_eq!(serde_json::to_string(&item).unwrap(), json);
        }
        let item = Item::from_json_str(EPUB).unwrap();
        assert_eq!((item.width, item.height, item.palettes.len()), (0, 0, 0));
        assert!(item.is_deleted);
    }

    #[test]
    fn values_set_later_are_written_even_if_the_file_lacked_them() {
        let mut item = Item::from_json_str(TEX).unwrap();
        item.width = 640;
        item.tags.push("paper".into());
        let out = serde_json::to_string(&item).unwrap();
        assert!(out.contains(r#""tags":["paper"]"#), "{out}");
        assert!(out.ends_with(r#""lastModified":4,"width":640}"#), "{out}");
    }

    #[test]
    fn new_items_use_eagles_key_order() {
        let mut item = Item::from_json_str(TEX).unwrap();
        item.key_order.clear();
        let out = serde_json::to_string(&item).unwrap();
        assert!(out.ends_with(r#""modificationTime":3,"height":0,"width":0,"lastModified":4,"palettes":[]}"#), "{out}");
    }

    #[test]
    fn only_id_is_required() {
        assert!(Item::from_json_str(r#"{"id":"X"}"#).is_ok());
        assert!(Item::from_json_str(r#"{"name":"X"}"#).is_err());
    }

    #[test]
    fn folder_with_every_optional_key_round_trips() {
        let json = r#"{"id":"MVP1BZ7QBLH3P","name":"Logos","description":"d","children":[{"id":"MVP1BZ7UEGUE3","name":"Red","description":"","children":[],"modificationTime":2,"tags":[],"password":"","passwordTips":""}],"modificationTime":1,"tags":["a"],"extendTags":["a"],"icon":"palette","iconColor":"red","pinyin":"Logos","password":"MTIz","passwordTips":"hint","coverId":"MVP1BZ81EWQ4O","orderBy":"RANDOM","sortIncrease":false}"#;
        let f: Folder = serde_json::from_str(json).unwrap();
        assert!(f.extra.is_empty());
        assert_eq!(f.order(), Some(FolderOrder::Random));
        assert_eq!(f.icon_color.as_deref(), Some("red"));
        assert_eq!(serde_json::to_string(&f).unwrap(), json);
    }

    #[test]
    fn folder_order_names_round_trip() {
        for o in crate::search::OrderBy::ALL {
            let f = FolderOrder::from(o);
            assert_eq!(FolderOrder::parse(f.as_str()), Some(f));
        }
        for s in ["DURATION", "RANDOM", "MANUAL"] {
            assert_eq!(FolderOrder::parse(s).unwrap().as_str(), s);
        }
        assert_eq!(FolderOrder::parse("nope"), None);
        assert_eq!(FOLDER_ICONS.len(), 161);
    }

    /// Eagle's default rules as its save writes them.
    const DEFAULT_RULE: &str = r#"{"type":{"includes":{},"excludes":{}},"tag":{"no":false,"includes":[],"excludes":[]},"folder":{"includes":{},"excludes":{}},"font":{"activated":false,"deactivated":false},"camera":{},"import":{"today":false,"yesterday":false,"last7day":false,"last30day":false,"last90day":false,"last365day":false,"usingRange":false,"type":"undefined","selectedMonths":{}},"mtime":{"today":false,"yesterday":false,"last7day":false,"last30day":false,"last90day":false,"last365day":false,"usingRange":false,"type":"undefined","selectedMonths":{}},"rating":{"0":false,"1":false,"2":false,"3":false,"4":false,"5":false},"color":{"gray":false,"accuracy":20},"shape":{"34":false,"43":false,"169":false,"916":false,"portrait":false,"landscape":false,"square":false,"panoramicPortrait":false,"panoramicLandscape":false,"custom":false,"width":4,"height":3},"resolution":{},"file":{"unit":"kb"},"duration":{"unit":"s"},"bpm":{},"annotation":{"has":false,"no":false},"note":{"has":false,"no":false},"url":{"has":false,"no":false},"image":{},"semantic":{}}"#;

    #[test]
    fn empty_filter_builds_eagles_default_rule() {
        let rule = SavedFilter::rule_from_filter(&crate::search::Filter::default(), None, |_| None);
        assert_eq!(rule.to_string(), DEFAULT_RULE);
        let saved = SavedFilter { name: "x".into(), rule, extra: Extra::new() };
        assert_eq!(saved.to_filter(), SavedFilterQuery::default());
    }

    #[test]
    fn saved_filter_reads_eagle_rules() {
        let rule = DEFAULT_RULE
            .replace(r#""type":{"includes":{}"#, r#""type":{"includes":{"png":true,"video":true}"#)
            .replace(r#""tag":{"no":false,"includes":[]"#, r#""tag":{"no":true,"includes":["red"]"#)
            .replace(r#""folder":{"includes":{}"#, r#""folder":{"includes":{"F1":{"id":"F1","name":"One","images":[]},"NoFolders":{"id":"NoFolders"}}"#)
            .replace(r#""rating":{"0":false,"1":false"#, r#""rating":{"0":true,"1":false"#)
            .replace(r#""color":{"gray":false,"accuracy":20}"#, r#""color":{"gray":false,"value":[255,0,10],"accuracy":10}"#)
            .replace(r#""square":false"#, r#""square":true"#)
            .replace(r#""169":false"#, r#""169":true"#)
            .replace(r#""resolution":{}"#, r#""resolution":{"minW":"100","maxH":800}"#)
            .replace(r#""file":{"unit":"kb"}"#, r#""file":{"min":1.5,"unit":"mb"}"#)
            .replace(r#""url":{"has":false,"no":false}"#, r#""url":{"has":true,"no":false,"keywords":"dribbble,shot"}"#)
            .replace(r#""note":{"has":false,"no":false}"#, r#""note":{"has":false,"no":true}"#)
            .replace(r#""import":{"today":false"#, r#""import":{"today":true"#)
            .replace(r#""semantic":{}}"#, r#""semantic":{},"keyword":"logo"}"#);
        let json = format!(r#"{{"name":"Mine","rule":{rule}}}"#);
        let saved: SavedFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&saved).unwrap(), json, "round trip");

        let q = saved.to_filter();
        let f = &q.filter;
        assert_eq!(q.keyword.as_deref(), Some("logo"));
        assert_eq!(f.types, ["png", "video"]);
        assert_eq!((f.tags.includes.clone(), f.tags.empty), (vec!["red".to_string()], true));
        assert_eq!((f.folders.includes.clone(), f.folders.empty), (vec!["F1".to_string()], true));
        assert!(f.ratings.contains(&0) && f.ratings.len() == 1);
        assert_eq!(f.color.map(|c| (c.rgb, c.accuracy)), Some(([255, 0, 10], 10.0)));
        assert!(f.shapes.contains(&crate::search::Shape::Square));
        assert_eq!((f.width, f.height), ((Some(100), None), (None, Some(800))));
        assert_eq!(f.size, (Some(1_572_864), None));
        assert_eq!(f.url, crate::search::TextFilter::Keywords(vec!["dribbble".into(), "shot".into()]));
        assert_eq!(f.note, crate::search::TextFilter::No);
        assert!(f.shapes.contains(&crate::search::Shape::Ratio16x9));
        assert!(q.unsupported.is_empty(), "{:?}", q.unsupported);
        assert_eq!(f.imported.presets, [crate::search::DatePreset::Today].into());
    }

    fn saved(rule: Value) -> SavedFilter {
        SavedFilter { name: "n".into(), rule, extra: Extra::new() }
    }

    /// Filter -> rule -> Filter for every rule `Filter` has, one at a time.
    #[test]
    fn every_filter_field_survives_a_save() {
        use crate::search::{ColorQuery, DateFilter, DatePreset, Filter, FontFilter, SetFilter, Shape, TextFilter};
        let fonts: std::collections::HashSet<String> = ["Inter-Regular_.ttf".to_string()].into();
        let dates = DateFilter { presets: [DatePreset::Yesterday, DatePreset::Last90Days].into(), range: Some((1_700_000_000_000, 1_700_086_400_000)), months: [(2024, 3), (2025, 11)].into(), clock: None };
        let cases: Vec<(&str, Filter)> = vec![
            ("tags", Filter { tags: SetFilter { includes: vec!["a".into()], excludes: vec!["b".into()], empty: true, ..Default::default() }, ..Default::default() }),
            ("folders", Filter { folders: SetFilter { includes: vec!["F1".into()], excludes: vec!["F2".into()], empty: true, ..Default::default() }, ..Default::default() }),
            ("uncategorized excluded", Filter { folders: SetFilter { excludes: vec!["F2".into()], exclude_empty: true, ..Default::default() }, ..Default::default() }),
            ("types", Filter { types: vec!["png".into(), "video".into()], ..Default::default() }),
            ("exclude_types", Filter { exclude_types: vec!["font".into()], ..Default::default() }),
            ("size", Filter { size: (Some(1024), Some(3 * 1_048_576)), ..Default::default() }),
            ("width", Filter { width: (Some(10), Some(20)), ..Default::default() }),
            ("height", Filter { height: (None, Some(30)), ..Default::default() }),
            ("ratings", Filter { ratings: [0, 4].into(), ..Default::default() }),
            ("shapes", Filter { shapes: [Shape::Portrait, Shape::PanoramicLandscape].into(), ..Default::default() }),
            ("ratios", Filter { shapes: [Shape::Ratio4x3, Shape::Ratio3x4, Shape::Ratio16x9, Shape::Ratio9x16].into(), ..Default::default() }),
            ("custom shape", Filter { custom_shape: Some((2.35, 1.0)), ..Default::default() }),
            ("url", Filter { url: TextFilter::Keywords(vec!["a".into(), "b".into()]), ..Default::default() }),
            ("note", Filter { note: TextFilter::No, ..Default::default() }),
            ("comments", Filter { comments: TextFilter::Has, ..Default::default() }),
            ("comment keywords", Filter { comments: TextFilter::Keywords(vec!["red".into(), "car".into()]), ..Default::default() }),
            ("color", Filter { color: Some(ColorQuery { rgb: [9, 8, 7], accuracy: 12.5 }), ..Default::default() }),
            ("grayscale", Filter { grayscale: true, ..Default::default() }),
            ("imported", Filter { imported: dates.clone(), ..Default::default() }),
            ("modified", Filter { modified: dates.clone(), ..Default::default() }),
            ("created", Filter { created: dates.clone(), ..Default::default() }),
            ("both ranges", Filter { imported: dates.clone(), modified: DateFilter { range: Some((5, 6)), ..Default::default() }, ..Default::default() }),
            ("unfinished range", Filter { modified: DateFilter { range: Some((0, 0)), ..Default::default() }, ..Default::default() }),
            ("duration s", Filter { duration: (Some(90.5), None), ..Default::default() }),
            ("duration m", Filter { duration: (Some(60.0), Some(5400.0)), ..Default::default() }),
            ("duration h", Filter { duration: (None, Some(7200.0)), ..Default::default() }),
            ("bpm", Filter { bpm: (Some(90.0), Some(128.5)), ..Default::default() }),
            ("cameras", Filter { cameras: ["Canon EOS R5".to_string(), "NIKON Z 6".to_string()].into(), ..Default::default() }),
            ("font activated", Filter { font: FontFilter::Activated(fonts.clone()), ..Default::default() }),
            ("font deactivated", Filter { font: FontFilter::Deactivated(fonts.clone()), ..Default::default() }),
        ];
        for (name, f) in cases {
            let rule = SavedFilter::rule_from_filter(&f, None, |_| None);
            let back = saved(rule.clone()).to_filter_with_fonts(|| fonts.clone());
            assert_eq!(back.filter, f, "{name}: {rule}");
            assert!(back.unsupported.is_empty(), "{name}: {:?}", back.unsupported);
        }
    }

    #[test]
    fn new_rules_are_written_in_eagles_shape() {
        use crate::search::{DateFilter, DatePreset, Filter, FontFilter};
        let f = Filter {
            imported: DateFilter { presets: [DatePreset::Today].into(), range: Some((10, 20)), ..Default::default() },
            modified: DateFilter { months: [(2024, 3)].into(), ..Default::default() },
            duration: (Some(60.0), Some(5400.0)),
            cameras: ["X100V".to_string()].into(),
            font: FontFilter::Deactivated(Default::default()),
            ..Default::default()
        };
        let text = SavedFilter::rule_from_filter(&f, None, |_| None).to_string();
        let day = |k: &str, today: bool, using: bool, range: &str, ty: &str, months: &str| {
            format!(r#""{k}":{{"today":{today},"yesterday":false,"last7day":false,"last30day":false,"last90day":false,"last365day":false,"usingRange":{using},{range}"type":"{ty}","selectedMonths":{{{months}}}}}"#)
        };
        for part in [
            r#""font":{"activated":false,"deactivated":true},"camera":{"X100V":true}"#.to_string(),
            day("import", true, true, r#""range":[10,20],"#, "range", ""),
            day("mtime", false, false, "", "undefined", r#""2024/03":true"#),
            r#""duration":{"min":1,"max":90,"unit":"m"}"#.to_string(),
        ] {
            assert!(text.contains(&part), "{part}\n{text}");
        }
        assert!(!text.contains("btime"), "no Date Created rule, no key");
    }

    #[test]
    fn reads_eagles_date_duration_camera_and_font_rules() {
        use crate::search::{DateFilter, DatePreset, FontFilter};
        // As Eagle saves them: text-field strings, the shared picked range in
        // `import` (with `model`), and a Date Modified rule using it.
        let rule = DEFAULT_RULE
            .replace(r#""font":{"activated":false,"deactivated":false}"#, r#""font":{"activated":true,"deactivated":false}"#)
            .replace(r#""camera":{}"#, r#""camera":{"ILCE-7M3":true,"DMC-GH5":true}"#)
            .replace(
                r#""import":{"today":false,"yesterday":false,"last7day":false,"last30day":false,"last90day":false,"last365day":false,"usingRange":false,"type":"undefined","selectedMonths":{}}"#,
                r#""import":{"today":false,"yesterday":false,"last7day":true,"last30day":false,"last90day":false,"last365day":false,"usingRange":false,"type":"range","selectedMonths":{"2023/12":true},"model":"2024-01-01 to 2024-01-31","range":[1704067200000,1706659200000]}"#,
            )
            .replace(r#""mtime":{"today":false"#, r#""mtime":{"today":true"#)
            .replace(r#""usingRange":false,"type":"undefined","selectedMonths":{}},"rating""#, r#""usingRange":true,"type":"undefined","selectedMonths":{}},"rating""#)
            .replace(r#""duration":{"unit":"s"}"#, r#""duration":{"unit":"m","min":"1.5","max":"10"}"#);
        assert_eq!(rule.matches("ILCE").count() + rule.matches("last7day\":true").count() + rule.matches("\"today\":true").count() + rule.matches("\"usingRange\":true").count() + rule.matches("\"min\":\"1.5\"").count() + rule.matches("\"activated\":true").count(), 6, "{rule}");
        let q = saved(serde_json::from_str(&rule).unwrap()).to_filter_with_fonts(|| ["A_.otf".to_string()].into());
        let f = &q.filter;
        assert!(q.unsupported.is_empty(), "{:?}", q.unsupported);
        assert_eq!(f.imported, DateFilter { presets: [DatePreset::Last7Days].into(), months: [(2023, 12)].into(), ..Default::default() });
        assert_eq!(f.modified, DateFilter { presets: [DatePreset::Today].into(), range: Some((1_704_067_200_000, 1_706_659_200_000)), ..Default::default() });
        assert_eq!(f.created, DateFilter::default());
        assert_eq!(f.duration, (Some(90.0), Some(600.0)));
        assert_eq!(f.cameras, ["ILCE-7M3".to_string(), "DMC-GH5".to_string()].into());
        assert_eq!(f.font, FontFilter::Activated(["A_.otf".to_string()].into()));

        let off = saved(serde_json::from_str(DEFAULT_RULE).unwrap()).to_filter_with_fonts(|| panic!("font keys read for an inactive rule"));
        assert_eq!(off.filter, Default::default());
    }

    #[test]
    fn filters_survive_a_save() {
        use crate::search::{ColorQuery, Filter, SetFilter, Shape, TextFilter};
        let mut f = Filter {
            tags: SetFilter { includes: vec!["a".into()], excludes: vec!["b".into()], ..Default::default() },
            folders: SetFilter { includes: vec!["F1".into()], empty: true, ..Default::default() },
            types: vec!["jpg".into()],
            exclude_types: vec!["font".into()],
            size: (Some(2048), Some(1_048_576)),
            width: (Some(10), None),
            height: (None, Some(20)),
            url: TextFilter::Has,
            note: TextFilter::Keywords(vec!["x".into()]),
            color: Some(ColorQuery { rgb: [1, 2, 3], accuracy: 20.0 }),
            grayscale: true,
            bpm: (Some(90.0), Some(128.5)),
            ..Default::default()
        };
        f.ratings.insert(5);
        f.shapes.insert(Shape::PanoramicPortrait);
        let rule = SavedFilter::rule_from_filter(&f, Some("cat"), |id| (id == "F1").then(|| "One".to_string()));
        let text = rule.to_string();
        assert!(text.contains(r#""color":{"gray":true,"value":[1,2,3],"accuracy":20}"#), "{text}");
        assert!(text.contains(r#""file":{"min":2,"max":1024,"unit":"kb"}"#), "{text}");
        assert!(text.contains(r#""folder":{"includes":{"F1":{"id":"F1","name":"One"},"NoFolders":{"id":"NoFolders"}}"#), "{text}");
        assert!(text.ends_with(r#""keyword":"cat"}"#));
        assert!(text.contains(r#""bpm":{"min":90,"max":128.5}"#), "{text}");
        let back = SavedFilter { name: "n".into(), rule, extra: Extra::new() }.to_filter();
        assert_eq!(back.filter, f);
        assert_eq!(back.keyword.as_deref(), Some("cat"));
        assert!(back.unsupported.is_empty());
    }

    #[test]
    fn comments_round_trip_and_keep_their_place() {
        let image = r#"{"id":"M1","x":12,"y":30.5,"width":200,"height":80.25,"annotation":"look<br>here","lastModified":1793005755402}"#;
        let video = r#"{"id":"M2","duration":3.733,"annotation":"cut","lastModified":1793005755403,"extra":true}"#;
        for json in [image, video] {
            let c: Comment = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&c).unwrap(), json);
        }
        let base = r#"{"id":"X","name":"n","size":1,"btime":0,"mtime":0,"ext":"png","tags":[],"folders":[],"isDeleted":false,"url":"","annotation":"","modificationTime":1,"height":1,"width":1,"lastModified":1,"palettes":[]"#;
        let text = format!(r#"{base},"comments":[{image}],"star":3}}"#);
        let mut item = Item::from_json_str(&text).unwrap();
        let mut list = item.comments();
        assert_eq!(list[0].width, Some(200.into()));
        item.set_comments(&list);
        assert_eq!(serde_json::to_string(&item).unwrap(), text, "untouched comments round-trip in place");
        list.push(Comment::at_time(1.5, "x"));
        item.set_comments(&list);
        assert!(serde_json::to_string(&item).unwrap().ends_with(r#""duration":1.5,"annotation":"x"}],"star":3}"#));

        let mut bare = Item::from_json_str(&format!("{base}}}")).unwrap();
        assert!(bare.comments().is_empty());
        bare.set_comments(&[Comment::region(1, 2, 30, 40, "")]);
        assert!(serde_json::to_string(&bare).unwrap().ends_with(r#""palettes":[],"comments":[{"id":"","x":1,"y":2,"width":30,"height":40,"annotation":""}]}"#));
    }

    #[test]
    fn tag_groups_round_trip() {
        for json in [
            r#"{"id":"LZ1Q2W3E4R5TY","name":"Mood","tags":["calm","loud"]}"#,
            r#"{"id":"LZ1Q2W3E4R5TY","name":"Mood","tags":[],"color":"aqua","description":""}"#,
            r#"{"id":"LZ1Q2W3E4R5TY","name":"Mood","tags":[],"description":"x","later":1}"#,
        ] {
            let g: TagGroup = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&g).unwrap(), json);
        }
    }
}
