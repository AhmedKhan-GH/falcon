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
    /// Active rules falcon cannot express yet, by their rule key (`import`,
    /// `mtime`, `duration`, `bpm`, `font`, `camera`, `annotation`, `image`,
    /// `semantic`, `shape.43`, ...).
    pub unsupported: Vec<String>,
}

impl SavedFilter {
    /// Reads the saved rules into a [`crate::search::Filter`]. Eagle does not
    /// save whether tags and folders combine with OR, AND or EQUAL (that
    /// setting lives in the panel), so they come back as OR, the panel's
    /// starting value.
    pub fn to_filter(&self) -> SavedFilterQuery {
        use crate::search::{ColorQuery, Filter, SetFilter, Shape, TextFilter};
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
        if exc.iter().any(|k| k == "NoFolders") {
            skip("folder.excludes.NoFolders");
        }
        f.folders = SetFilter {
            empty: inc.iter().any(|k| k == "NoFolders"),
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
        ] {
            if truthy(&["shape", key]) {
                f.shapes.insert(shape);
            }
        }
        for key in ["43", "34", "169", "916", "custom"] {
            if truthy(&["shape", key]) {
                skip(&format!("shape.{key}"));
            }
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

        // Rules with no counterpart in falcon's filter yet.
        if truthy(&["annotation", "has"]) || truthy(&["annotation", "no"]) {
            skip("annotation");
        }
        for k in ["import", "mtime"] {
            let on = ["today", "yesterday", "last7day", "last30day", "last90day", "last365day", "usingRange"].iter().any(|d| truthy(&[k, d]))
                || get(&[k, "selectedMonths"]).and_then(Value::as_object).is_some_and(|m| !m.is_empty());
            if on {
                skip(k);
            }
        }
        for k in ["duration", "bpm"] {
            if number(&[k, "min"]).is_some() || number(&[k, "max"]).is_some() {
                skip(k);
            }
        }
        if truthy(&["font", "activated"]) || truthy(&["font", "deactivated"]) {
            skip("font");
        }
        if get(&["camera"]).and_then(Value::as_object).is_some_and(|m| !m.is_empty()) {
            skip("camera");
        }
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
        use crate::search::{Shape, TextFilter};
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
        r["folder"]["excludes"] = Value::Object(filter.folders.excludes.iter().map(|id| (id.clone(), folder(id))).collect());
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
            };
            r["shape"][key] = true.into();
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
        text(&mut r, "url", &filter.url);
        text(&mut r, "note", &filter.note);
        if let Some(k) = keyword.filter(|k| !k.is_empty()) {
            r["keyword"] = k.into();
        }
        r
    }
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
        assert_eq!(q.unsupported, ["shape.169", "import"]);
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
        let back = SavedFilter { name: "n".into(), rule, extra: Extra::new() }.to_filter();
        assert_eq!(back.filter, f);
        assert_eq!(back.keyword.as_deref(), Some("cat"));
        assert!(back.unsupported.is_empty());
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
