//! Editing items and tag history, with Eagle's on-disk effects.
//!
//! Every item edit rewrites the item's whole `metadata.json`, bumps
//! `lastModified` (never `modificationTime`) and updates `mtime.json`.
//! Renaming also renames the original and its thumbnail inside the `.info`
//! folder; the folder itself keeps its ID name.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::import::sanitize_name;
use crate::library::{Library, write_json};
use crate::ids;
use crate::model::{Folder, Item};

/// How many recent tags Eagle keeps in `historyTags`.
const HISTORY_LIMIT: usize = 120;

impl Library {
    /// Saves `item` over the stored item with the same id.
    pub fn update_item(&mut self, mut item: Item) -> Result<()> {
        let old = self.item(&item.id).cloned().ok_or_else(|| Error::Import {
            path: self.root().to_path_buf(),
            reason: format!("no item {}", item.id),
        })?;
        if item.name != old.name {
            item.name = sanitize_name(&item.name, &item.ext);
        }
        if item.name != old.name {
            let root = self.root().to_path_buf();
            let (from, to) = (old.original_path(&root), item.original_path(&root));
            fs::rename(&from, &to).map_err(|e| Error::io(&from, e))?;
            if let Some(thumb) = old.thumbnail_path(&root) {
                let dest = thumb.with_file_name(format!("{}_thumbnail.png", item.name));
                fs::rename(&thumb, &dest).map_err(|e| Error::io(&thumb, e))?;
            }
        }
        item.last_modified = now_ms().max(old.last_modified + 1);
        self.register_item(item)
    }

    /// Moves items to the trash (`isDeleted` plus `deletedTime`) or restores
    /// them.
    pub fn set_trashed(&mut self, ids: &[String], trashed: bool) -> Result<()> {
        for id in ids {
            let Some(mut item) = self.item(id).cloned() else { continue };
            item.is_deleted = trashed;
            if trashed {
                item.extra.insert("deletedTime".into(), now_ms().into());
            }
            self.update_item(item)?;
        }
        Ok(())
    }

    /// Deletes items for good, as Eagle's permanent delete does: the whole
    /// `.info` folder is removed and the id leaves `mtime.json`.
    pub fn delete_permanently(&mut self, ids: &[String]) -> Result<()> {
        for id in ids {
            let Some(item) = self.item(id).cloned() else { continue };
            let dir = item.info_dir(self.root());
            if dir.is_dir() {
                fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
            }
            self.forget_item(id)?;
        }
        Ok(())
    }

    /// Permanently deletes everything in the trash and returns how many
    /// items went.
    pub fn empty_trash(&mut self) -> Result<usize> {
        let ids: Vec<String> = self.items().iter().filter(|i| i.is_deleted).map(|i| i.id.clone()).collect();
        self.delete_permanently(&ids)?;
        Ok(ids.len())
    }

    /// Records tags as just used: moved to the front of `historyTags`, which
    /// is capped at 120, then writes `tags.json`.
    pub fn record_tag_use(&mut self, tags: &[String]) -> Result<()> {
        for t in tags.iter().rev() {
            self.tags.history_tags.retain(|h| h != t);
            self.tags.history_tags.insert(0, t.clone());
        }
        self.tags.history_tags.truncate(HISTORY_LIMIT);
        write_json(&self.root().join("tags.json"), &self.tags)
    }
}

impl Library {
    /// Creates a folder at the end of `parent`'s children (or at the root)
    /// and returns its id.
    pub fn create_folder(&mut self, parent: Option<&str>, name: &str) -> Result<String> {
        let id = ids::new_id();
        let folder = Folder {
            id: id.clone(),
            name: name.to_string(),
            description: String::new(),
            children: Vec::new(),
            modification_time: now_ms(),
            tags: Vec::new(),
            password: String::new(),
            password_tips: String::new(),
            extra: Default::default(),
        };
        match parent {
            None => self.metadata.folders.push(folder),
            Some(p) => match find_mut(&mut self.metadata.folders, p) {
                Some(f) => f.children.push(folder),
                None => return Err(self.missing(p)),
            },
        }
        self.save_metadata()?;
        Ok(id)
    }

    /// Renames a folder. Eagle leaves its `modificationTime` alone.
    pub fn rename_folder(&mut self, id: &str, name: &str) -> Result<()> {
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        f.name = name.to_string();
        self.save_metadata()
    }

    /// Sets a folder's description.
    pub fn set_folder_description(&mut self, id: &str, description: &str) -> Result<()> {
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        f.description = description.to_string();
        self.save_metadata()
    }

    /// Deletes a folder and its subfolders. Items that were in them lose
    /// those folder ids; items left in no folder go to the trash, as Eagle
    /// does (without a `deletedTime`).
    pub fn delete_folder(&mut self, id: &str) -> Result<()> {
        let removed = remove(&mut self.metadata.folders, id).ok_or_else(|| self.missing(id))?;
        let mut gone = Vec::new();
        removed.walk(&mut |f| gone.push(f.id.clone()));
        self.save_metadata()?;
        let affected: Vec<Item> = self
            .items()
            .iter()
            .filter(|i| i.folders.iter().any(|f| gone.contains(f)))
            .cloned()
            .collect();
        for mut item in affected {
            item.folders.retain(|f| !gone.contains(f));
            if item.folders.is_empty() {
                item.is_deleted = true;
            }
            self.update_item(item)?;
        }
        Ok(())
    }

    /// Writes the library `metadata.json`, stamping its `modificationTime`.
    pub fn save_metadata(&mut self) -> Result<()> {
        self.metadata.modification_time = now_ms();
        write_json(&self.root().join("metadata.json"), &self.metadata)
    }

    fn missing(&self, id: &str) -> Error {
        Error::Import { path: self.root().to_path_buf(), reason: format!("no folder {id}") }
    }
}

fn find_mut<'a>(folders: &'a mut [Folder], id: &str) -> Option<&'a mut Folder> {
    for f in folders {
        if f.id == id {
            return Some(f);
        }
        if let Some(found) = find_mut(&mut f.children, id) {
            return Some(found);
        }
    }
    None
}

fn remove(folders: &mut Vec<Folder>, id: &str) -> Option<Folder> {
    if let Some(pos) = folders.iter().position(|f| f.id == id) {
        return Some(folders.remove(pos));
    }
    folders.iter_mut().find_map(|f| remove(&mut f.children, id))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
