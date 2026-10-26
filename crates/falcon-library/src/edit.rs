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
use crate::model::Item;

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

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
