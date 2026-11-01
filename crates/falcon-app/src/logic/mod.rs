//! UI-independent logic, split by domain and re-exported from here so every item
//! is reachable as `crate::logic::name`. One file per domain:
//! - `scope`: scopes, navigation counts, filtering and sorting.
//! - `format`: size, date and colour formatting.
//! - `layout`: layouts and card placement maths.
//! - `filters`: date, duration, camera and font filters.
//! - `audio`: audio seek bar and time labels.
//! - `duplicates`: duplicate groups.
//! - `libraries`: library folders versus importable paths.
//! - `folders`: folder tree lookups, folder drops, tree selection, folder names.
//! - `selection`: marquee and keyboard selection.
//! - `drops`: manual reordering drops.
//! - `viewer`: the detail viewer's shell.
//! - `image_viewer`: image viewer zoom, fitting, tiling, navigator, grayscale.
//! - `viewer3d`: the 3D model viewer's camera, navigation, grid and axes.
//! - `display`: display options, list columns, thumbnail backgrounds, subfolder cards.

mod scope;
mod format;
mod layout;
mod filters;
mod audio;
mod duplicates;
mod libraries;
mod folders;
mod selection;
mod drops;
mod viewer;
mod image_viewer;
mod display;
mod color;
mod viewer3d;

pub use scope::*;
pub use format::*;
pub use layout::*;
pub use filters::*;
pub use audio::*;
pub use duplicates::*;
pub use libraries::*;
pub use folders::*;
pub use selection::*;
pub use drops::*;
pub use viewer::*;
pub use image_viewer::*;
pub use display::*;
pub use color::*;
pub use viewer3d::*;

#[cfg(test)]
pub(crate) mod test_util {
    use falcon_library::model::{Folder, Item};
    use serde_json::json;

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn item(id: &str, name: &str, folders: &[&str], tags: &[&str], deleted: bool, t: u64, size: u64, w: u64) -> Item {
        let v = json!({
            "id": id, "name": name, "size": size, "btime": 0, "mtime": 0, "ext": "png",
            "tags": tags, "folders": folders, "isDeleted": deleted, "url": "",
            "annotation": "Hello Note", "modificationTime": t, "height": 10, "width": w,
            "lastModified": t, "palettes": []
        });
        Item::from_json_str(&v.to_string()).unwrap()
    }

    pub(crate) fn folder(id: &str, children: Vec<Folder>) -> Folder {
        let v = json!({
            "id": id, "name": format!("{id}-name"), "description": "", "children": children,
            "modificationTime": 0, "tags": [], "password": "", "passwordTips": ""
        });
        serde_json::from_value(v).unwrap()
    }

    pub(crate) fn fixture() -> (Vec<Folder>, Vec<Item>) {
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

    pub(crate) fn names(items: &[Item], v: &[usize]) -> Vec<String> {
        v.iter().map(|&i| items[i].name.clone()).collect()
    }
}
