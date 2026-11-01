//! What the search box and filter bar remember between launches: the
//! search scope, which filters are pinned to the bar, and the Go to Folder
//! history. Kept with the app state, like Eagle's `localStorage` entries,
//! so it is shared by every library.

use std::collections::BTreeMap;

use falcon_library::search::SearchScope;

/// One filter of the filter bar, in Eagle's default bar order. falcon's
/// Date chip stands for Eagle's Date Added and Date Modified filters (it
/// has a tab for each, and one for Date Created).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FilterKind {
    Color,
    Tags,
    Folders,
    Shape,
    Rating,
    Types,
    Dimensions,
    Duration,
    Size,
    Comment,
    Note,
    Url,
    Date,
    Font,
    Bpm,
    Camera,
}

#[allow(dead_code)]
impl FilterKind {
    pub const ALL: [FilterKind; 16] = [
        FilterKind::Color,
        FilterKind::Tags,
        FilterKind::Folders,
        FilterKind::Shape,
        FilterKind::Rating,
        FilterKind::Types,
        FilterKind::Dimensions,
        FilterKind::Duration,
        FilterKind::Size,
        FilterKind::Comment,
        FilterKind::Note,
        FilterKind::Url,
        FilterKind::Date,
        FilterKind::Font,
        FilterKind::Bpm,
        FilterKind::Camera,
    ];

    /// The key the pin is saved under (Eagle's filter id).
    pub fn id(self) -> &'static str {
        match self {
            FilterKind::Color => "color",
            FilterKind::Tags => "tags",
            FilterKind::Folders => "folders",
            FilterKind::Shape => "shape",
            FilterKind::Rating => "rating",
            FilterKind::Types => "types",
            FilterKind::Dimensions => "resolution",
            FilterKind::Duration => "duration",
            FilterKind::Size => "size",
            FilterKind::Comment => "annotation",
            FilterKind::Note => "note",
            FilterKind::Url => "url",
            FilterKind::Date => "import",
            FilterKind::Font => "fontActivated",
            FilterKind::Bpm => "bpm",
            FilterKind::Camera => "camera",
        }
    }

    /// The chip's name.
    pub fn label(self) -> &'static str {
        match self {
            FilterKind::Color => "Color",
            FilterKind::Tags => "Tags",
            FilterKind::Folders => "Folders",
            FilterKind::Shape => "Shape",
            FilterKind::Rating => "Rating",
            FilterKind::Types => "Type",
            FilterKind::Dimensions => "Dimensions",
            FilterKind::Duration => "Duration",
            FilterKind::Size => "Size",
            FilterKind::Comment => "Comment",
            FilterKind::Note => "Notes",
            FilterKind::Url => "URL",
            FilterKind::Date => "Date",
            FilterKind::Font => "Font",
            FilterKind::Bpm => "BPM",
            FilterKind::Camera => "Camera",
        }
    }

    /// Whether the chip sits in the bar before the user changes anything:
    /// Eagle pins Color, Tags, Folders, Shape, Rating and Type.
    pub fn pinned_by_default(self) -> bool {
        matches!(self, FilterKind::Color | FilterKind::Tags | FilterKind::Folders | FilterKind::Shape | FilterKind::Rating | FilterKind::Types)
    }
}

/// Remembered search and filter settings.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct SearchPrefs {
    /// The search box's Search Scope menu.
    pub scope: SearchScope,
    /// Pins the user changed, by [`FilterKind::id`]; the others keep
    /// [`FilterKind::pinned_by_default`].
    pub pinned: BTreeMap<String, bool>,
    /// Folders and smart folders opened from Go to Folder, newest first.
    pub folder_history: Vec<String>,
    pub smart_history: Vec<String>,
}

/// How many folders (and smart folders) Go to Folder remembers, as Eagle.
#[allow(dead_code)]
pub const HISTORY_LEN: usize = 200;

#[allow(dead_code)]
impl SearchPrefs {
    pub fn is_pinned(&self, kind: FilterKind) -> bool {
        self.pinned.get(kind.id()).copied().unwrap_or_else(|| kind.pinned_by_default())
    }

    /// Pins or unpins a filter; the default needs no entry.
    pub fn set_pinned(&mut self, kind: FilterKind, on: bool) {
        if on == kind.pinned_by_default() {
            self.pinned.remove(kind.id());
        } else {
            self.pinned.insert(kind.id().to_string(), on);
        }
    }
}

/// Puts `id` first in a Go to Folder history, without repeats, keeping at
/// most [`HISTORY_LEN`].
#[allow(dead_code)]
pub fn push_history(list: &mut Vec<String>, id: &str) {
    list.retain(|x| x != id);
    list.insert(0, id.to_string());
    list.truncate(HISTORY_LEN);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_default_to_eagles_and_store_only_changes() {
        let mut p = SearchPrefs::default();
        let shown: Vec<_> = FilterKind::ALL.into_iter().filter(|k| p.is_pinned(*k)).collect();
        assert_eq!(shown, [FilterKind::Color, FilterKind::Tags, FilterKind::Folders, FilterKind::Shape, FilterKind::Rating, FilterKind::Types]);
        p.set_pinned(FilterKind::Size, true);
        p.set_pinned(FilterKind::Color, false);
        assert!(p.is_pinned(FilterKind::Size) && !p.is_pinned(FilterKind::Color));
        p.set_pinned(FilterKind::Color, true);
        assert_eq!(p.pinned.keys().collect::<Vec<_>>(), ["size"]);
        let ids: std::collections::HashSet<_> = FilterKind::ALL.iter().map(|k| k.id()).collect();
        assert_eq!(ids.len(), FilterKind::ALL.len());
    }

    #[test]
    fn history_is_newest_first_without_repeats() {
        let mut h = vec!["a".to_string(), "b".to_string()];
        push_history(&mut h, "b");
        assert_eq!(h, ["b", "a"]);
        for n in 0..300 {
            push_history(&mut h, &n.to_string());
        }
        assert_eq!(h.len(), HISTORY_LEN);
        assert_eq!(h[0], "299");
    }

    #[test]
    fn prefs_survive_a_round_trip_and_missing_fields() {
        let mut p = SearchPrefs::default();
        p.scope.comments = false;
        p.set_pinned(FilterKind::Bpm, true);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<SearchPrefs>(&json).unwrap(), p);
        let old: SearchPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(old, SearchPrefs::default());
        assert!(old.scope.comments);
    }
}
