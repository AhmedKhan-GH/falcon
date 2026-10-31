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
    /// The folder's tags plus its ancestors', which Eagle caches on load.
    pub extend_tags: Option<Vec<String>>,
    /// Icon name from [`FOLDER_ICONS`]; absent for the default folder icon.
    pub icon: Option<String>,
    /// Icon colour from [`ICON_COLORS`]; absent for the default colour.
    pub icon_color: Option<String>,
    /// Romanised name Eagle caches for search (the name itself for Latin text).
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
        FolderOrder::By(o)
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
            FolderOrder::Duration => "DURATION",
            FolderOrder::Random => "RANDOM",
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
    /// Eagle writes `width` before `height` for a few items; remembered so
    /// the output keeps the original key order.
    pub width_first: bool,
    /// Unknown keys.
    pub extra: Extra,
}

/// Deserialization shape for [`Item`]; `width_first` is filled in afterwards.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemRepr {
    id: String,
    name: String,
    size: u64,
    btime: u64,
    mtime: u64,
    ext: String,
    tags: Vec<String>,
    folders: Vec<String>,
    is_deleted: bool,
    url: String,
    annotation: String,
    modification_time: u64,
    remove_thumbnail: Option<bool>,
    no_thumbnail: Option<bool>,
    height: u64,
    width: u64,
    last_modified: u64,
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
            width_first: false,
            extra: r.extra,
        }
    }
}

impl Item {
    /// Parses an item's `metadata.json`, recording the width/height key order.
    pub fn from_json_str(s: &str) -> serde_json::Result<Item> {
        let value: Value = serde_json::from_str(s)?;
        let width_first = value.as_object().is_some_and(|m| {
            let pos = |k: &str| m.keys().position(|x| x == k);
            matches!((pos("width"), pos("height")), (Some(w), Some(h)) if w < h)
        });
        let mut item: Item = serde_json::from_value(value)?;
        item.width_first = width_first;
        Ok(item)
    }
}

impl Serialize for Item {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("id", &self.id)?;
        m.serialize_entry("name", &self.name)?;
        m.serialize_entry("size", &self.size)?;
        m.serialize_entry("btime", &self.btime)?;
        m.serialize_entry("mtime", &self.mtime)?;
        m.serialize_entry("ext", &self.ext)?;
        m.serialize_entry("tags", &self.tags)?;
        m.serialize_entry("folders", &self.folders)?;
        m.serialize_entry("isDeleted", &self.is_deleted)?;
        m.serialize_entry("url", &self.url)?;
        m.serialize_entry("annotation", &self.annotation)?;
        m.serialize_entry("modificationTime", &self.modification_time)?;
        if let Some(v) = &self.remove_thumbnail {
            m.serialize_entry("removeThumbnail", v)?;
        }
        if let Some(v) = &self.no_thumbnail {
            m.serialize_entry("noThumbnail", v)?;
        }
        if self.width_first {
            m.serialize_entry("width", &self.width)?;
            m.serialize_entry("height", &self.height)?;
        } else {
            m.serialize_entry("height", &self.height)?;
            m.serialize_entry("width", &self.width)?;
        }
        m.serialize_entry("lastModified", &self.last_modified)?;
        m.serialize_entry("palettes", &self.palettes)?;
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
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
