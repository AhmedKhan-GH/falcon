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

    /// Renames a tag on every item, merging it into `to` where an item has
    /// both. Tag history, starred tags and tag groups follow, as in Eagle.
    /// Returns how many items changed.
    pub fn rename_tag(&mut self, from: &str, to: &str) -> Result<usize> {
        let to = to.trim();
        if to.is_empty() || from == to {
            return Ok(0);
        }
        let affected: Vec<Item> = self.items().iter().filter(|i| i.tags.iter().any(|t| t == from)).cloned().collect();
        let n = affected.len();
        for mut item in affected {
            let mut tags = Vec::with_capacity(item.tags.len());
            for t in item.tags.drain(..) {
                let t = if t == from { to.to_string() } else { t };
                if !tags.contains(&t) {
                    tags.push(t);
                }
            }
            item.tags = tags;
            self.update_item(item)?;
        }
        let swap = |list: &mut Vec<String>| {
            for t in list.iter_mut() {
                if t == from {
                    *t = to.to_string();
                }
            }
            let mut seen = std::collections::HashSet::new();
            list.retain(|t| seen.insert(t.clone()));
        };
        swap(&mut self.tags.history_tags);
        swap(&mut self.tags.starred_tags);
        for g in &mut self.metadata.tags_groups {
            if let Some(serde_json::Value::Array(tags)) = g.get_mut("tags") {
                for t in tags.iter_mut() {
                    if t.as_str() == Some(from) {
                        *t = serde_json::Value::String(to.to_string());
                    }
                }
            }
        }
        self.save_metadata()?;
        write_json(&self.root().join("tags.json"), &self.tags)?;
        Ok(n)
    }

    /// Removes a tag from every item, the history, starred tags and groups.
    pub fn delete_tag(&mut self, tag: &str) -> Result<usize> {
        let affected: Vec<Item> = self.items().iter().filter(|i| i.tags.iter().any(|t| t == tag)).cloned().collect();
        let n = affected.len();
        for mut item in affected {
            item.tags.retain(|t| t != tag);
            self.update_item(item)?;
        }
        self.tags.history_tags.retain(|t| t != tag);
        self.tags.starred_tags.retain(|t| t != tag);
        for g in &mut self.metadata.tags_groups {
            if let Some(serde_json::Value::Array(tags)) = g.get_mut("tags") {
                tags.retain(|t| t.as_str() != Some(tag));
            }
        }
        self.save_metadata()?;
        write_json(&self.root().join("tags.json"), &self.tags)?;
        Ok(n)
    }

    /// Stars or unstars a tag (most recent first, as Eagle keeps them).
    pub fn set_tag_starred(&mut self, tag: &str, starred: bool) -> Result<()> {
        self.tags.starred_tags.retain(|t| t != tag);
        if starred {
            self.tags.starred_tags.insert(0, tag.to_string());
        }
        write_json(&self.root().join("tags.json"), &self.tags)
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
        self.metadata
            .quick_access
            .retain(|q| q.get("id").and_then(|v| v.as_str()).is_none_or(|i| !gone.iter().any(|g| g == i)));
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

    /// Moves a folder under `parent` (or to the root), at the end of its new
    /// siblings as Eagle appends. Moving a folder into itself or one of its
    /// descendants is refused.
    pub fn move_folder(&mut self, id: &str, parent: Option<&str>) -> Result<()> {
        if let Some(p) = parent {
            let inside = find_mut(&mut self.metadata.folders, id).is_some_and(|f| {
                let mut hit = false;
                f.walk(&mut |x| hit |= x.id == p);
                hit
            });
            if inside {
                return Err(Error::Import { path: self.root().to_path_buf(), reason: "cannot move a folder into itself".into() });
            }
        }
        let Some(folder) = remove(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        match parent {
            None => self.metadata.folders.push(folder),
            Some(p) => match find_mut(&mut self.metadata.folders, p) {
                Some(f) => f.children.push(folder),
                None => {
                    self.metadata.folders.push(folder);
                    return Err(self.missing(p));
                }
            },
        }
        self.save_metadata()
    }

    /// Pins a folder (`"folder"`) or smart folder (`"smartFolder"`) to Quick
    /// Access, at the end as Eagle does.
    pub fn add_quick_access(&mut self, kind: &str, id: &str) -> Result<()> {
        if !self.metadata.quick_access.iter().any(|q| q.get("id").and_then(|v| v.as_str()) == Some(id)) {
            self.metadata.quick_access.push(serde_json::json!({"type": kind, "id": id}));
        }
        self.save_metadata()
    }

    /// Unpins an id from Quick Access.
    pub fn remove_quick_access(&mut self, id: &str) -> Result<()> {
        self.metadata.quick_access.retain(|q| q.get("id").and_then(|v| v.as_str()) != Some(id));
        self.save_metadata()
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
