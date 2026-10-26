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
    /// Tag groups (raw JSON).
    pub tags_groups: Vec<Value>,
    /// Last modification time, ms since epoch.
    pub modification_time: u64,
    /// Version of Eagle that wrote the library.
    pub application_version: String,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A node in the folder tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Last modification time, ms since epoch.
    pub modification_time: u64,
    /// Tags attached to the folder.
    pub tags: Vec<String>,
    /// Password (empty when unset).
    pub password: String,
    /// Password hint.
    pub password_tips: String,
    /// Unknown keys.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Folder {
    /// Visits this folder and all descendants, depth first.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Folder)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
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
