//! Loading and saving whole libraries.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::{Folder, Item, LibraryMetadata, MtimeIndex, Tags};

const METADATA: &str = "metadata.json";
const MTIME: &str = "mtime.json";
const TAGS: &str = "tags.json";
const FILTERS: &str = "saved-filters.json";
const IMAGES: &str = "images";

/// An Eagle library loaded into memory (item payload files stay on disk).
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
    /// `metadata.json`.
    pub metadata: LibraryMetadata,
    /// `mtime.json`.
    pub mtime: MtimeIndex,
    /// `tags.json`.
    pub tags: Tags,
    /// `saved-filters.json` (shape not yet observed; raw JSON).
    pub saved_filters: Vec<Value>,
    items: Vec<Item>,
}

impl Library {
    /// Loads the library at `path`. Items are sorted by id.
    pub fn open(path: impl AsRef<Path>) -> Result<Library> {
        let root = path.as_ref().to_path_buf();
        if !root.join(METADATA).is_file() || !root.join(IMAGES).is_dir() {
            return Err(Error::NotALibrary(root));
        }
        let metadata = read_json(&root.join(METADATA))?;
        let mtime = read_json(&root.join(MTIME))?;
        let tags = read_json(&root.join(TAGS))?;
        let saved_filters = read_json(&root.join(FILTERS))?;

        let images = root.join(IMAGES);
        let mut items = Vec::new();
        for entry in fs::read_dir(&images).map_err(|e| Error::io(&images, e))? {
            let entry = entry.map_err(|e| Error::io(&images, e))?;
            if !entry.file_name().to_string_lossy().ends_with(".info") || !entry.path().is_dir() {
                continue;
            }
            let file = entry.path().join(METADATA);
            let text = fs::read_to_string(&file).map_err(|e| Error::io(&file, e))?;
            items.push(Item::from_json_str(&text).map_err(|e| Error::json(&file, e))?);
        }
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Library { root, metadata, mtime, tags, saved_filters, items })
    }

    /// The library's root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All items (including trashed ones), sorted by id.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Looks up an item by id.
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Mutable access to the items, for edits before [`Library::save_to`].
    pub fn items_mut(&mut self) -> &mut Vec<Item> {
        &mut self.items
    }

    /// Every folder in the tree, depth first.
    pub fn all_folders(&self) -> Vec<&Folder> {
        let mut out = Vec::new();
        for f in &self.metadata.folders {
            f.walk(&mut |x| out.push(x));
        }
        out
    }

    /// Ids of all folders in the tree.
    pub fn folder_ids(&self) -> HashSet<&str> {
        self.all_folders().into_iter().map(|f| f.id.as_str()).collect()
    }

    /// Info directory of `item` in this library.
    pub fn info_dir(&self, item: &Item) -> PathBuf {
        item.info_dir(&self.root)
    }

    /// Writes the library to `dest` (created if needed), copying each item's
    /// files and any other root entries (`backup/`, `Icon`, ...) unchanged,
    /// and regenerating the JSON files from the in-memory model.
    pub fn save_to(&self, dest: impl AsRef<Path>) -> Result<()> {
        let dest = dest.as_ref();
        mkdir(dest)?;
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b);
        if same(dest, &self.root) {
            return Err(Error::SaveOntoSelf(dest.to_path_buf()));
        }

        // Unmodelled root entries are copied verbatim.
        let modelled = [METADATA, MTIME, TAGS, FILTERS, IMAGES];
        for entry in fs::read_dir(&self.root).map_err(|e| Error::io(&self.root, e))? {
            let entry = entry.map_err(|e| Error::io(&self.root, e))?;
            if !modelled.iter().any(|m| entry.file_name() == *m) {
                copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
            }
        }

        write_json(&dest.join(METADATA), &self.metadata)?;
        write_json(&dest.join(MTIME), &self.mtime)?;
        write_json(&dest.join(TAGS), &self.tags)?;
        write_json(&dest.join(FILTERS), &self.saved_filters)?;

        let images = dest.join(IMAGES);
        mkdir(&images)?;
        for item in &self.items {
            let src = self.info_dir(item);
            let out = item.info_dir(dest);
            mkdir(&out)?;
            if src.is_dir() {
                for entry in fs::read_dir(&src).map_err(|e| Error::io(&src, e))? {
                    let entry = entry.map_err(|e| Error::io(&src, e))?;
                    if entry.file_name() != METADATA {
                        copy_recursive(&entry.path(), &out.join(entry.file_name()))?;
                    }
                }
            }
            write_json(&out.join(METADATA), item)?;
        }
        Ok(())
    }
}

impl Item {
    /// `<library>/images/<id>.info`.
    pub fn info_dir(&self, library_root: &Path) -> PathBuf {
        library_root.join(IMAGES).join(format!("{}.info", self.id))
    }

    /// Path of the original file, `<name>.<ext>` inside the info dir.
    pub fn original_path(&self, library_root: &Path) -> PathBuf {
        let file = if self.ext.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.name, self.ext)
        };
        self.info_dir(library_root).join(file)
    }

    /// Path of `<name>_thumbnail.png`, if that file exists.
    pub fn thumbnail_path(&self, library_root: &Path) -> Option<PathBuf> {
        let p = self.info_dir(library_root).join(format!("{}_thumbnail.png", self.name));
        p.is_file().then_some(p)
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::json(path, e))
}

/// Eagle writes compact JSON with no trailing newline.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| Error::json(path, e))?;
    fs::write(path, bytes).map_err(|e| Error::io(path, e))
}

fn mkdir(p: &Path) -> Result<()> {
    fs::create_dir_all(p).map_err(|e| Error::io(p, e))
}

fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        mkdir(dst)?;
        for entry in fs::read_dir(src).map_err(|e| Error::io(src, e))? {
            let entry = entry.map_err(|e| Error::io(src, e))?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        fs::copy(src, dst).map_err(|e| Error::io(src, e))?;
    }
    Ok(())
}
