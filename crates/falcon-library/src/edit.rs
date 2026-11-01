//! Editing items and tag history, with Eagle's on-disk effects.
//!
//! Every item edit rewrites the item's whole `metadata.json`, bumps
//! `lastModified` (never `modificationTime`) and updates `mtime.json`.
//! Renaming also renames the original and its thumbnail inside the `.info`
//! folder; the folder itself keeps its ID name.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::import::sanitize_name;
use crate::library::{Library, write_json};
use crate::ids;
use crate::model::{Comment, Folder, FolderOrder, ICON_COLORS, Item, SavedFilter, TagGroup};

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

    /// Adds a comment to an item and returns its id. A missing id or
    /// `lastModified` is filled in. Timed notes (`duration`) are kept sorted
    /// by time, a new one after any at the same time; region notes are
    /// appended. The item is saved like any other edit.
    pub fn add_comment(&mut self, item_id: &str, mut comment: Comment) -> Result<String> {
        let mut item = self.item(item_id).cloned().ok_or_else(|| self.invalid(format!("no item {item_id}")))?;
        if comment.id.is_empty() {
            comment.id = ids::new_id();
        }
        comment.last_modified.get_or_insert_with(now_ms);
        let id = comment.id.clone();
        let mut list = item.comments();
        match comment.duration.as_ref().and_then(|d| d.as_f64()) {
            Some(t) => {
                let at = list.iter().position(|c| c.duration.as_ref().and_then(|d| d.as_f64()).is_some_and(|d| d > t)).unwrap_or(list.len());
                list.insert(at, comment);
            }
            None => list.push(comment),
        }
        item.set_comments(&list);
        self.update_item(item)?;
        Ok(id)
    }

    /// Replaces the comment with `comment.id` on an item (text, region or
    /// time). Eagle leaves the comment's `lastModified` alone on edits, so
    /// whatever `comment` carries is written.
    pub fn update_comment(&mut self, item_id: &str, comment: Comment) -> Result<()> {
        let mut item = self.item(item_id).cloned().ok_or_else(|| self.invalid(format!("no item {item_id}")))?;
        let mut list = item.comments();
        let slot = list.iter_mut().find(|c| c.id == comment.id).ok_or_else(|| self.invalid(format!("no comment {}", comment.id)))?;
        *slot = comment;
        item.set_comments(&list);
        self.update_item(item)
    }

    /// Removes a comment from an item. Eagle keeps an empty `comments: []`.
    pub fn delete_comment(&mut self, item_id: &str, comment_id: &str) -> Result<()> {
        let mut item = self.item(item_id).cloned().ok_or_else(|| self.invalid(format!("no item {item_id}")))?;
        let mut list = item.comments();
        let before = list.len();
        list.retain(|c| c.id != comment_id);
        if list.len() == before {
            return Err(self.invalid(format!("no comment {comment_id}")));
        }
        item.set_comments(&list);
        self.update_item(item)
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
            swap(&mut g.tags);
        }
        self.save_metadata()?;
        self.save_tags()?;
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
            g.tags.retain(|t| t != tag);
        }
        self.save_metadata()?;
        self.save_tags()?;
        Ok(n)
    }

    /// Stars or unstars a tag (most recent first, as Eagle keeps them).
    pub fn set_tag_starred(&mut self, tag: &str, starred: bool) -> Result<()> {
        self.tags.starred_tags.retain(|t| t != tag);
        if starred {
            self.tags.starred_tags.insert(0, tag.to_string());
        }
        self.save_tags()
    }

    /// Records tags as just used: moved to the front of `historyTags`, which
    /// is capped at 120, then writes `tags.json`.
    pub fn record_tag_use(&mut self, tags: &[String]) -> Result<()> {
        for t in tags.iter().rev() {
            self.tags.history_tags.retain(|h| h != t);
            self.tags.history_tags.insert(0, t.clone());
        }
        self.tags.history_tags.truncate(HISTORY_LIMIT);
        self.save_tags()
    }
}

/// Tag groups, kept in `tagsGroups` of the library `metadata.json`. Group
/// edits only rewrite that file; items are untouched unless a delete asks
/// for its tags to go too.
impl Library {
    /// Adds a tag group at the end of the list and returns its id. Tags are
    /// kept in the given order without duplicates; like Eagle's "create
    /// group from tags", they are not taken out of other groups.
    pub fn create_tag_group(&mut self, name: &str, tags: &[String], color: Option<&str>) -> Result<String> {
        self.check_color(color)?;
        let id = ids::new_id();
        let mut list = Vec::new();
        for t in tags.iter().map(|t| clean_tag(t)) {
            if !list.contains(&t) {
                list.push(t);
            }
        }
        self.metadata.tags_groups.push(TagGroup {
            id: id.clone(),
            name: display_name(name),
            tags: list,
            color: color.map(str::to_string),
            description: None,
            extra: Default::default(),
        });
        self.save_metadata()?;
        Ok(id)
    }

    /// Renames a tag group (cut to 1024 characters, as Eagle does).
    pub fn rename_tag_group(&mut self, id: &str, name: &str) -> Result<()> {
        self.tag_group_mut(id)?.name = display_name(name);
        self.save_metadata()
    }

    /// Replaces a group's tags (order kept, duplicates dropped). Other
    /// groups are left alone.
    pub fn set_tag_group_tags(&mut self, id: &str, tags: &[String]) -> Result<()> {
        let g = self.tag_group_mut(id)?;
        g.tags.clear();
        for t in tags.iter().map(|t| clean_tag(t)) {
            if !g.tags.contains(&t) {
                g.tags.push(t);
            }
        }
        self.save_metadata()
    }

    /// Adds tags to the end of a group. As in Eagle's tag manager, a tag
    /// moves: it leaves any other group it was in.
    pub fn add_tags_to_group(&mut self, id: &str, tags: &[String]) -> Result<()> {
        self.tag_group_mut(id)?;
        let tags: Vec<String> = tags.iter().map(|t| clean_tag(t)).collect();
        for g in &mut self.metadata.tags_groups {
            if g.id == id {
                for t in &tags {
                    if !g.tags.contains(t) {
                        g.tags.push(t.clone());
                    }
                }
            } else {
                g.tags.retain(|t| !tags.contains(t));
            }
        }
        self.save_metadata()
    }

    /// Takes tags out of a group; the tags stay on their items.
    pub fn remove_tags_from_group(&mut self, id: &str, tags: &[String]) -> Result<()> {
        self.tag_group_mut(id)?.tags.retain(|t| !tags.contains(t));
        self.save_metadata()
    }

    /// Sets a group's colour (one of [`ICON_COLORS`]) or removes it.
    pub fn set_tag_group_color(&mut self, id: &str, color: Option<&str>) -> Result<()> {
        self.check_color(color)?;
        self.tag_group_mut(id)?.color = color.map(str::to_string);
        self.save_metadata()
    }

    /// Sets a group's description, the text under its name in the tag
    /// manager. Eagle keeps the key once set: clearing the box writes `""`
    /// rather than removing it, and the key goes after `color`.
    pub fn set_tag_group_description(&mut self, id: &str, description: &str) -> Result<()> {
        self.tag_group_mut(id)?.description = Some(description.to_string());
        self.save_metadata()
    }

    /// Deletes a group. With `remove_tags`, its tags are also deleted from
    /// every item, the tag history and starred tags, as Eagle offers.
    pub fn delete_tag_group(&mut self, id: &str, remove_tags: bool) -> Result<()> {
        let pos = self.tag_group_index(id)?;
        let group = self.metadata.tags_groups.remove(pos);
        self.save_metadata()?;
        if remove_tags {
            for t in &group.tags {
                self.delete_tag(t)?;
            }
        }
        Ok(())
    }

    /// Moves a group to `index` in the list (after it has left its old
    /// place; past the end appends), as dragging in the tag manager does.
    pub fn reorder_tag_group(&mut self, id: &str, index: usize) -> Result<()> {
        let pos = self.tag_group_index(id)?;
        let g = self.metadata.tags_groups.remove(pos);
        let at = index.min(self.metadata.tags_groups.len());
        self.metadata.tags_groups.insert(at, g);
        self.save_metadata()
    }

    fn tag_group_index(&self, id: &str) -> Result<usize> {
        self.metadata.tags_groups.iter().position(|g| g.id == id).ok_or_else(|| self.invalid(format!("no tag group {id}")))
    }

    fn tag_group_mut(&mut self, id: &str) -> Result<&mut TagGroup> {
        let pos = self.tag_group_index(id)?;
        Ok(&mut self.metadata.tags_groups[pos])
    }
}

/// Saved filters in `saved-filters.json`. Entries have no id, so they are
/// addressed by position, as Eagle's menu does. Each edit rewrites the file
/// (compact, no trailing newline) and leaves `metadata.json` alone.
impl Library {
    /// Appends a saved filter and returns its position. `rule` is the rule
    /// object (see [`SavedFilter::rule_from_filter`]).
    pub fn create_saved_filter(&mut self, name: &str, rule: serde_json::Value) -> Result<usize> {
        if name.is_empty() {
            return Err(self.invalid("empty saved filter name"));
        }
        self.saved_filters.push(SavedFilter { name: name.to_string(), rule, extra: Default::default() });
        self.save_filters()?;
        Ok(self.saved_filters.len() - 1)
    }

    /// Renames the saved filter at `index`; Eagle ignores empty names.
    pub fn rename_saved_filter(&mut self, index: usize, name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(self.invalid("empty saved filter name"));
        }
        self.saved_filter_mut(index)?.name = name.to_string();
        self.save_filters()
    }

    /// Replaces the rules of the saved filter at `index`.
    pub fn update_saved_filter(&mut self, index: usize, rule: serde_json::Value) -> Result<()> {
        self.saved_filter_mut(index)?.rule = rule;
        self.save_filters()
    }

    /// Deletes the saved filter at `index`.
    pub fn delete_saved_filter(&mut self, index: usize) -> Result<()> {
        self.saved_filter_mut(index)?;
        self.saved_filters.remove(index);
        self.save_filters()
    }

    /// Moves the saved filter at `index` to position `to` (past the end
    /// appends), as dragging in Eagle's menu does.
    pub fn reorder_saved_filter(&mut self, index: usize, to: usize) -> Result<()> {
        self.saved_filter_mut(index)?;
        let f = self.saved_filters.remove(index);
        let at = to.min(self.saved_filters.len());
        self.saved_filters.insert(at, f);
        self.save_filters()
    }

    fn saved_filter_mut(&mut self, index: usize) -> Result<&mut SavedFilter> {
        let err = self.invalid(format!("no saved filter {index}"));
        self.saved_filters.get_mut(index).ok_or(err)
    }

    /// Writes `saved-filters.json` (or marks it dirty in batched mode),
    /// recording the previous list in an open transaction.
    pub(crate) fn save_filters(&mut self) -> Result<()> {
        self.journal_saved_filters();
        if self.batching(|d| d.saved_filters = true) {
            return Ok(());
        }
        write_json(&self.root().join("saved-filters.json"), &self.saved_filters)
    }
}

/// How [`Library::sort_subfolders`] orders folders by name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubfolderSort {
    /// Z to A instead of A to Z.
    pub descending: bool,
    /// Sort every level below as well, not just the direct subfolders.
    pub all_levels: bool,
}

/// Eagle strips line breaks from tags it files into a group.
fn clean_tag(t: &str) -> String {
    t.replace(['\r', '\n'], "")
}

/// A name as Eagle stores it: cut to 1024 characters and NFC-normalised
/// (Eagle normalises folder, smart-folder and tag-group names on save).
pub(crate) fn display_name(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.chars().take(1024).nfc().collect()
}

impl Library {
    /// Creates a folder at the end of `parent`'s children (or at the root)
    /// and returns its id. A subfolder takes its parent's icon and colour,
    /// as Eagle's new-subfolder command does.
    pub fn create_folder(&mut self, parent: Option<&str>, name: &str) -> Result<String> {
        let id = ids::new_id();
        let mut folder = Folder::new(id.clone(), display_name(name), now_ms());
        match parent {
            None => self.metadata.folders.push(folder),
            Some(p) => match find_mut(&mut self.metadata.folders, p) {
                Some(f) => {
                    folder.icon.clone_from(&f.icon);
                    folder.icon_color.clone_from(&f.icon_color);
                    f.children.push(folder)
                }
                None => return Err(self.missing(p)),
            },
        }
        self.save_metadata()?;
        Ok(id)
    }

    /// Renames a folder. Eagle leaves its `modificationTime` alone. A
    /// cached `pinyin` (which Eagle itself strips before writing) would now
    /// be stale, so it goes.
    pub fn rename_folder(&mut self, id: &str, name: &str) -> Result<()> {
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        f.name = display_name(name);
        f.pinyin = None;
        self.save_metadata()
    }

    /// Sets a folder's own sort, or clears it with `None` (both `orderBy`
    /// and `sortIncrease` go). Eagle leaves `modificationTime` alone.
    pub fn set_folder_order(&mut self, id: &str, order: Option<FolderOrder>, increase: bool) -> Result<()> {
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        f.order_by = order.map(|o| o.as_str().to_string());
        f.sort_increase = order.map(|_| increase);
        self.save_metadata()
    }

    /// Sets a folder's icon and icon colour; `None` removes the key (the
    /// default icon or colour). Pass the current value to keep one. The
    /// colour must be one of [`ICON_COLORS`].
    pub fn set_folder_icon(&mut self, id: &str, icon: Option<&str>, color: Option<&str>) -> Result<()> {
        self.check_color(color)?;
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        f.icon = icon.map(str::to_string);
        f.icon_color = color.map(str::to_string);
        self.save_metadata()
    }

    /// Sets the item shown as a folder's cover, or removes `coverId`.
    pub fn set_folder_cover(&mut self, id: &str, item: Option<&str>) -> Result<()> {
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        f.cover_id = item.map(str::to_string);
        self.save_metadata()
    }

    /// Sets a folder's description.
    pub fn set_folder_description(&mut self, id: &str, description: &str) -> Result<()> {
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        f.description = description.to_string();
        self.save_metadata()
    }

    /// Locks a folder with `(password, hint)`, or unlocks it with `None`.
    /// Eagle stores `btoa(password)`, which only accepts characters up to
    /// U+00FF, so other passwords are refused here as Eagle cannot set them
    /// either. Unlocking writes `""` for both keys, as Eagle's save does.
    pub fn set_folder_password(&mut self, id: &str, password: Option<(&str, &str)>) -> Result<()> {
        let encoded = match password {
            Some(("", _)) => return Err(self.invalid("empty password")),
            Some((p, hint)) => Some((encode_password(p).ok_or_else(|| self.invalid("password has characters beyond Latin-1"))?, hint)),
            None => None,
        };
        let Some(f) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        let (password, tips) = encoded.unwrap_or_default();
        f.password = password;
        f.password_tips = tips.to_string();
        self.save_metadata()
    }

    /// Whether `password` opens the folder. An unlocked or unknown folder
    /// never matches.
    pub fn verify_folder_password(&self, id: &str, password: &str) -> bool {
        self.all_folders()
            .into_iter()
            .find(|f| f.id == id)
            .is_some_and(|f| f.is_locked() && encode_password(password).is_some_and(|e| e == f.password))
    }

    /// Deletes a folder and its subfolders, moving items left in no folder
    /// to the trash (see [`Library::delete_folders`]).
    pub fn delete_folder(&mut self, id: &str) -> Result<()> {
        self.delete_folders(&[id.to_string()], true)
    }

    /// Deletes folders with their subfolders, and unpins them from Quick
    /// Access. Items that were in them lose those folder ids. With
    /// `trash_items` (Eagle's "move the items to the trash too", on by
    /// default), an item left in no folder goes to the trash, without a
    /// `deletedTime` as Eagle does; an item still in another folder stays.
    /// Without it every item stays. An id inside another deleted folder
    /// goes with it; an unknown id is refused before anything changes.
    pub fn delete_folders(&mut self, ids: &[String], trash_items: bool) -> Result<()> {
        let known = self.folder_ids();
        if let Some(id) = ids.iter().find(|id| !known.contains(id.as_str())) {
            return Err(self.missing(id));
        }
        let mut gone = HashSet::new();
        for id in ids {
            if let Some(removed) = remove(&mut self.metadata.folders, id) {
                removed.walk(&mut |f| {
                    gone.insert(f.id.clone());
                });
            }
        }
        self.metadata
            .quick_access
            .retain(|q| q.get("id").and_then(|v| v.as_str()).is_none_or(|i| !gone.contains(i)));
        self.save_metadata()?;
        let affected: Vec<Item> = self
            .items()
            .iter()
            .filter(|i| i.folders.iter().any(|f| gone.contains(f)))
            .cloned()
            .collect();
        for mut item in affected {
            item.folders.retain(|f| !gone.contains(f));
            if trash_items && item.folders.is_empty() {
                item.is_deleted = true;
            }
            self.update_item(item)?;
        }
        Ok(())
    }

    /// Duplicates a folder with its subfolders, right after it, as Eagle's
    /// Duplicate does, and returns the copy's id. The copies keep the
    /// names, description, auto-tags, icon, colour, cover and item sort;
    /// each gets a new id and a new `modificationTime`, and loses its
    /// password. No items are put in the copies.
    pub fn duplicate_folder(&mut self, id: &str) -> Result<String> {
        fn fresh(f: &mut Folder, now: u64) {
            f.id = ids::new_id();
            f.modification_time = now;
            f.password = String::new();
            f.password_tips = String::new();
            for c in &mut f.children {
                fresh(c, now);
            }
        }
        let parent = parent_map(&self.metadata.folders).get(id).copied().ok_or_else(|| self.missing(id))?.map(str::to_string);
        let mut copy = find_mut(&mut self.metadata.folders, id).expect("found above").clone();
        fresh(&mut copy, now_ms());
        let new_id = copy.id.clone();
        let siblings = match parent {
            None => &mut self.metadata.folders,
            Some(p) => &mut find_mut(&mut self.metadata.folders, &p).expect("parent exists").children,
        };
        let at = siblings.iter().position(|f| f.id == id).expect("in its parent");
        siblings.insert(at + 1, copy);
        self.save_metadata()?;
        Ok(new_id)
    }

    /// Renames several folders, `(id, new name)` each, in one save, as
    /// Eagle's batch rename does. Like [`Library::rename_folder`] it leaves
    /// `modificationTime` alone and drops a stale `pinyin`. An unknown id
    /// is refused before anything changes.
    pub fn rename_folders(&mut self, names: &[(String, String)]) -> Result<()> {
        let known = self.folder_ids();
        if let Some((id, _)) = names.iter().find(|(id, _)| !known.contains(id.as_str())) {
            return Err(self.missing(id));
        }
        for (id, name) in names {
            let f = find_mut(&mut self.metadata.folders, id).expect("checked above");
            f.name = display_name(name);
            f.pinyin = None;
        }
        self.save_metadata()
    }

    /// Gives several folders the same icon and/or colour in one save, as
    /// Eagle's commands for selected folders do. `None` leaves that key as
    /// each folder has it; `Some(None)` removes it.
    pub fn restyle_folders(&mut self, ids: &[String], icon: Option<Option<&str>>, color: Option<Option<&str>>) -> Result<()> {
        self.check_color(color.flatten())?;
        self.expect_folders(ids)?;
        for id in ids {
            let f = find_mut(&mut self.metadata.folders, id).expect("checked above");
            if let Some(i) = icon {
                f.icon = i.map(str::to_string);
            }
            if let Some(c) = color {
                f.icon_color = c.map(str::to_string);
            }
        }
        self.save_metadata()
    }

    /// Gives several folders the same item sort in one save, as Eagle's
    /// menu for selected folders does: `None` removes `orderBy` and
    /// `sortIncrease`; a key keeps each folder's direction, ascending
    /// when it had none.
    pub fn set_folders_order(&mut self, ids: &[String], order: Option<FolderOrder>) -> Result<()> {
        self.expect_folders(ids)?;
        for id in ids {
            let f = find_mut(&mut self.metadata.folders, id).expect("checked above");
            f.order_by = order.map(|o| o.as_str().to_string());
            f.sort_increase = order.map(|_| f.sort_increase.unwrap_or(true));
        }
        self.save_metadata()
    }

    /// Sets the sort direction of several folders in one save.
    pub fn set_folders_increase(&mut self, ids: &[String], increase: bool) -> Result<()> {
        self.expect_folders(ids)?;
        for id in ids {
            find_mut(&mut self.metadata.folders, id).expect("checked above").sort_increase = Some(increase);
        }
        self.save_metadata()
    }

    /// Moves several folders above, into or below `target` in one save, as
    /// Eagle's Move Folder dialog does. They keep their order in the tree
    /// (top to bottom); above and below they become the target's siblings
    /// in that order, into it they follow its last child. A folder whose
    /// parent moves too stays inside it. Moving onto one of the moved
    /// folders, or into a moved folder's descendant, is refused, and
    /// nothing changes on any error.
    pub fn move_folders(&mut self, ids: &[String], target: &str, place: FolderPlace) -> Result<()> {
        self.expect_folders(ids)?;
        if !self.folder_ids().contains(target) {
            return Err(self.missing(target));
        }
        let path = folder_path(&self.metadata.folders, target);
        if ids.iter().any(|id| path.contains(&id.as_str())) {
            return Err(self.invalid("cannot move a folder into itself"));
        }
        let picked: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let parents = parent_map(&self.metadata.folders);
        // Tree order, leaving out folders whose parent moves with them.
        let movers: Vec<String> = self
            .all_folders()
            .into_iter()
            .filter(|f| picked.contains(f.id.as_str()) && !parents[f.id.as_str()].is_some_and(|p| picked.contains(p)))
            .map(|f| f.id.clone())
            .collect();
        let moved: Vec<Folder> = movers.iter().map(|id| remove(&mut self.metadata.folders, id).expect("checked above")).collect();
        let (siblings, at) = match place {
            FolderPlace::Into => {
                let kids = &mut find_mut(&mut self.metadata.folders, target).expect("checked above").children;
                let end = kids.len();
                (kids, end)
            }
            FolderPlace::Above | FolderPlace::Below => {
                let parent = parent_map(&self.metadata.folders)[target].map(str::to_string);
                let list = match parent {
                    None => &mut self.metadata.folders,
                    Some(p) => &mut find_mut(&mut self.metadata.folders, &p).expect("parent exists").children,
                };
                let i = list.iter().position(|f| f.id == target).expect("target is in its parent");
                (list, if place == FolderPlace::Below { i + 1 } else { i })
            }
        };
        siblings.splice(at..at, moved);
        self.save_metadata()
    }

    /// Moves a folder under `parent` (or to the root), at the end of its new
    /// siblings as Eagle appends. Moving a folder into itself or one of its
    /// descendants is refused.
    pub fn move_folder(&mut self, id: &str, parent: Option<&str>) -> Result<()> {
        self.reorder_folder(id, parent, usize::MAX)
    }

    /// Moves a folder under `new_parent` (or to the root) at position
    /// `index` among its new siblings, counted after it has left its old
    /// place; an index past the end appends. Use it with the same parent to
    /// reorder siblings. Moving a folder into itself or one of its
    /// descendants is refused, and nothing changes on any error.
    pub fn reorder_folder(&mut self, id: &str, new_parent: Option<&str>, index: usize) -> Result<()> {
        let Some(folder) = find_mut(&mut self.metadata.folders, id) else { return Err(self.missing(id)) };
        if let Some(p) = new_parent {
            let mut inside = false;
            folder.walk(&mut |x| inside |= x.id == p);
            if inside {
                return Err(self.invalid("cannot move a folder into itself"));
            }
            if find_mut(&mut self.metadata.folders, p).is_none() {
                return Err(self.missing(p));
            }
        }
        let folder = remove(&mut self.metadata.folders, id).ok_or_else(|| self.missing(id))?;
        let siblings = match new_parent {
            None => &mut self.metadata.folders,
            Some(p) => &mut find_mut(&mut self.metadata.folders, p).expect("parent checked above").children,
        };
        siblings.insert(index.min(siblings.len()), folder);
        self.save_metadata()
    }

    /// Sorts `parent`'s subfolders (or the root folders) by name, as the
    /// folder menu's "Sort By" commands do, and saves the reordered tree.
    /// With `all_levels`, every folder below is sorted too. Names compare
    /// case-insensitively with numbers by value; descending is the
    /// ascending list reversed. Eagle stores no flag: only `children` (or
    /// the root list) changes order.
    pub fn sort_subfolders(&mut self, parent: Option<&str>, sort: SubfolderSort) -> Result<()> {
        fn sort_level(list: &mut [Folder], sort: SubfolderSort) {
            list.sort_by(|a, b| crate::search::natural_cmp(&a.name, &b.name));
            if sort.descending {
                list.reverse();
            }
            if sort.all_levels {
                for f in list {
                    sort_level(&mut f.children, sort);
                }
            }
        }
        let list = match parent {
            None => &mut self.metadata.folders,
            Some(p) => match find_mut(&mut self.metadata.folders, p) {
                Some(f) => &mut f.children,
                None => return Err(self.missing(p)),
            },
        };
        sort_level(list, sort);
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
    /// An open transaction records the metadata as last saved; in batched
    /// mode the write waits for [`Library::flush`].
    pub fn save_metadata(&mut self) -> Result<()> {
        self.metadata.modification_time = now_ms();
        self.journal_metadata();
        if self.batching(|d| d.metadata = true) {
            return Ok(());
        }
        write_json(&self.root().join("metadata.json"), &self.metadata)
    }

    /// Writes `tags.json` like [`Library::save_metadata`] writes metadata.
    pub(crate) fn save_tags(&mut self) -> Result<()> {
        self.journal_tags();
        if self.batching(|d| d.tags = true) {
            return Ok(());
        }
        write_json(&self.root().join("tags.json"), &self.tags)
    }

    /// Refuses a list naming a folder that doesn't exist.
    fn expect_folders(&self, ids: &[String]) -> Result<()> {
        let known = self.folder_ids();
        match ids.iter().find(|id| !known.contains(id.as_str())) {
            Some(id) => Err(self.missing(id)),
            None => Ok(()),
        }
    }

    fn missing(&self, id: &str) -> Error {
        Error::Import { path: self.root().to_path_buf(), reason: format!("no folder {id}") }
    }

    pub(crate) fn invalid(&self, reason: impl Into<String>) -> Error {
        Error::Import { path: self.root().to_path_buf(), reason: reason.into() }
    }

    /// Refuses a colour outside Eagle's palette.
    pub(crate) fn check_color(&self, color: Option<&str>) -> Result<()> {
        match color {
            Some(c) if !ICON_COLORS.contains(&c) => Err(self.invalid(format!("unknown colour {c}"))),
            _ => Ok(()),
        }
    }
}

/// Where [`Library::move_folders`] puts folders, next to the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderPlace {
    Above,
    Into,
    Below,
}

/// Each folder's parent id (`None` at the top level).
fn parent_map(roots: &[Folder]) -> HashMap<&str, Option<&str>> {
    fn go<'a>(list: &'a [Folder], parent: Option<&'a str>, out: &mut HashMap<&'a str, Option<&'a str>>) {
        for f in list {
            out.insert(&f.id, parent);
            go(&f.children, Some(&f.id), out);
        }
    }
    let mut out = HashMap::new();
    go(roots, None, &mut out);
    out
}

/// Ids from the top level down to `id`, `id` included.
fn folder_path<'a>(roots: &'a [Folder], id: &str) -> Vec<&'a str> {
    let parents = parent_map(roots);
    let mut path = Vec::new();
    let mut cur = parents.get_key_value(id).map(|(k, _)| *k);
    while let Some(c) = cur {
        path.push(c);
        cur = parents[c];
    }
    path
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

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `btoa`: base64 of each character as one byte; `None` past U+00FF.
fn encode_password(p: &str) -> Option<String> {
    use base64::Engine;
    let bytes: Option<Vec<u8>> = p.chars().map(|c| u8::try_from(u32::from(c)).ok()).collect();
    Some(base64::engine::general_purpose::STANDARD.encode(bytes?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subfolders_sort_by_name_with_numbers_by_value() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("S.library")).unwrap();
        let top = lib.create_folder(None, "Top").unwrap();
        for n in ["folder 10", "Folder 2", "beta", "Alpha", "folder 1"] {
            let id = lib.create_folder(Some(&top), n).unwrap();
            for c in ["z", "B", "a"] {
                lib.create_folder(Some(&id), c).unwrap();
            }
        }
        lib.create_folder(None, "Another").unwrap();
        let names = |l: &[Folder]| l.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
        let stamp = lib.metadata.folders[0].children[0].modification_time;

        lib.sort_subfolders(Some(&top), SubfolderSort::default()).unwrap();
        let reopened = Library::open(lib.root()).unwrap();
        let kids = &reopened.metadata.folders[0].children;
        assert_eq!(names(kids), ["Alpha", "beta", "folder 1", "Folder 2", "folder 10"]);
        assert_eq!(names(&kids[0].children), ["z", "B", "a"], "one level only");
        assert_eq!(names(&reopened.metadata.folders), ["Top", "Another"], "other levels untouched");
        assert!(kids.iter().any(|f| f.modification_time == stamp), "folders keep their times");

        lib.sort_subfolders(None, SubfolderSort { descending: true, all_levels: true }).unwrap();
        assert_eq!(names(&lib.metadata.folders), ["Top", "Another"]);
        let kids = &lib.metadata.folders[0].children;
        assert_eq!(names(kids), ["folder 10", "Folder 2", "folder 1", "beta", "Alpha"]);
        assert_eq!(names(&kids[0].children), ["z", "B", "a"]);
        lib.sort_subfolders(None, SubfolderSort { descending: false, all_levels: true }).unwrap();
        assert_eq!(names(&lib.metadata.folders[1].children[0].children), ["a", "B", "z"]);
        assert!(lib.sort_subfolders(Some("NOPE"), SubfolderSort::default()).is_err());
    }

    #[test]
    fn tag_group_descriptions_and_drag_order() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("G.library")).unwrap();
        let a = lib.create_tag_group("A", &["x".into()], Some("red")).unwrap();
        let b = lib.create_tag_group("B", &[], None).unwrap();
        let c = lib.create_tag_group("C", &[], None).unwrap();
        let path = lib.root().join("metadata.json");
        let text = || std::fs::read_to_string(&path).unwrap();
        assert!(!text().contains("description"), "no key until one is set");
        lib.set_tag_group_description(&a, "Warm & bright").unwrap();
        assert!(text().contains(&format!(r#"{{"id":"{a}","name":"A","tags":["x"],"color":"red","description":"Warm & bright"}}"#)), "{}", text());
        lib.set_tag_group_description(&a, "").unwrap();
        assert!(text().contains(r#""color":"red","description":""}"#), "cleared, the key stays: {}", text());
        assert!(lib.set_tag_group_description("NOPE", "x").is_err());

        // Dragging C to the top, then A below B (index counted after removal).
        lib.reorder_tag_group(&c, 0).unwrap();
        lib.reorder_tag_group(&a, 2).unwrap();
        let order: Vec<&str> = lib.metadata.tags_groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(order, [c.as_str(), b.as_str(), a.as_str()]);
        let reopened = Library::open(lib.root()).unwrap();
        assert_eq!(reopened.metadata.tags_groups[2].description.as_deref(), Some(""));
    }

    /// A library with three imported PNGs.
    fn with_items(dir: &std::path::Path) -> (Library, Vec<String>) {
        let mut lib = Library::create(dir.join("B.library")).unwrap();
        let mut ids = Vec::new();
        for n in 0..3 {
            let path = dir.join(format!("{n}.png"));
            image::RgbImage::from_pixel(4, 4, image::Rgb([n * 80, 20, 20])).save(&path).unwrap();
            ids.push(lib.import_file(&path, &crate::import::ImportOptions::default()).unwrap());
        }
        (lib, ids)
    }

    fn file_into(lib: &mut Library, item: &str, folders: &[&String]) {
        let mut i = lib.item(item).unwrap().clone();
        i.folders = folders.iter().map(|f| f.to_string()).collect();
        lib.update_item(i).unwrap();
    }

    fn names(l: &[Folder]) -> Vec<String> {
        l.iter().map(|f| f.name.clone()).collect()
    }

    #[test]
    fn deleting_several_folders_trashes_or_keeps_items() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, items) = with_items(tmp.path());
        let a = lib.create_folder(None, "A").unwrap();
        let a1 = lib.create_folder(Some(&a), "A1").unwrap();
        let b = lib.create_folder(None, "B").unwrap();
        let c = lib.create_folder(None, "C").unwrap();
        lib.add_quick_access("folder", &a1).unwrap();
        lib.add_quick_access("folder", &c).unwrap();
        file_into(&mut lib, &items[0], &[&a1]);
        file_into(&mut lib, &items[1], &[&a, &b]);
        file_into(&mut lib, &items[2], &[&b, &c]);

        // Keeping the items: they only lose the folders.
        lib.delete_folders(&[a.clone(), b.clone()], false).unwrap();
        let kept = Library::open(lib.root()).unwrap();
        assert_eq!(names(&kept.metadata.folders), ["C"]);
        assert!(kept.item(&items[0]).unwrap().folders.is_empty());
        assert!(kept.items().iter().all(|i| !i.is_deleted));
        assert_eq!(kept.item(&items[2]).unwrap().folders, std::slice::from_ref(&c));
        assert_eq!(kept.metadata.quick_access.len(), 1, "the subfolder is unpinned");

        // Trashing: items left in no folder go, the one still in C stays.
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, items) = with_items(tmp.path());
        let a = lib.create_folder(None, "A").unwrap();
        let a1 = lib.create_folder(Some(&a), "A1").unwrap();
        let b = lib.create_folder(None, "B").unwrap();
        let c = lib.create_folder(None, "C").unwrap();
        file_into(&mut lib, &items[0], &[&a1]);
        file_into(&mut lib, &items[1], &[&a, &b]);
        file_into(&mut lib, &items[2], &[&b, &c]);
        // A1 is inside A: naming both is fine.
        lib.delete_folders(&[a1.clone(), a.clone(), b.clone()], true).unwrap();
        let got = Library::open(lib.root()).unwrap();
        assert!(got.item(&items[0]).unwrap().is_deleted);
        assert!(got.item(&items[1]).unwrap().is_deleted);
        assert!(!got.item(&items[2]).unwrap().is_deleted);
        assert!(got.item(&items[0]).unwrap().extra.get("deletedTime").is_none(), "as Eagle, no deletedTime");

        let before = lib.metadata.folders.clone();
        assert!(lib.delete_folders(&[c.clone(), "NOPE".into()], true).is_err());
        assert_eq!(lib.metadata.folders, before, "nothing changes on an unknown id");
    }

    #[test]
    fn batch_rename_restyle_and_sort() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("R.library")).unwrap();
        let a = lib.create_folder(None, "A").unwrap();
        let b = lib.create_folder(Some(&a), "B").unwrap();
        let c = lib.create_folder(None, "C").unwrap();
        let stamp = lib.metadata.folders[0].modification_time;
        lib.set_folder_order(&c, Some(FolderOrder::By(crate::search::OrderBy::Name)), false).unwrap();

        lib.rename_folders(&[(a.clone(), "Cafe\u{301} 1".into()), (b.clone(), "Two".into())]).unwrap();
        let got = Library::open(lib.root()).unwrap();
        assert_eq!(got.metadata.folders[0].name, "Caf\u{e9} 1", "NFC as Eagle saves");
        assert_eq!(got.metadata.folders[0].children[0].name, "Two");
        assert_eq!(got.metadata.folders[0].modification_time, stamp);
        assert!(lib.rename_folders(&[(c.clone(), "X".into()), ("NOPE".into(), "Y".into())]).is_err());
        assert_eq!(lib.metadata.folders[1].name, "C");

        let all = [a.clone(), b.clone(), c.clone()];
        lib.restyle_folders(&all, None, Some(Some("red"))).unwrap();
        lib.restyle_folders(&all[..2], Some(Some("star")), None).unwrap();
        let got = Library::open(lib.root()).unwrap();
        let f = got.all_folders();
        assert_eq!((f[0].icon.as_deref(), f[0].icon_color.as_deref()), (Some("star"), Some("red")));
        assert_eq!((f[2].icon.as_deref(), f[2].icon_color.as_deref()), (None, Some("red")));
        assert!(lib.restyle_folders(&all, None, Some(Some("mauve"))).is_err());
        lib.restyle_folders(&all, Some(None), Some(None)).unwrap();
        let text = std::fs::read_to_string(lib.root().join("metadata.json")).unwrap();
        assert!(!text.contains("iconColor") && !text.contains("\"icon\""), "keys removed: {text}");

        // A shared key keeps each folder's direction; Global Settings drops both keys.
        lib.set_folders_order(&all, Some(FolderOrder::By(crate::search::OrderBy::FileSize))).unwrap();
        let f = lib.all_folders();
        assert_eq!(f.iter().map(|f| f.sort_increase).collect::<Vec<_>>(), [Some(true), Some(true), Some(false)]);
        assert!(f.iter().all(|f| f.order_by.as_deref() == Some("FILESIZE")), "{:?}", f.iter().map(|f| &f.order_by).collect::<Vec<_>>());
        lib.set_folders_increase(&all, false).unwrap();
        assert!(lib.all_folders().iter().all(|f| f.sort_increase == Some(false)));
        lib.set_folders_order(&all, None).unwrap();
        let text = std::fs::read_to_string(lib.root().join("metadata.json")).unwrap();
        assert!(!text.contains("orderBy") && !text.contains("sortIncrease"));
    }

    #[test]
    fn moving_several_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("M.library")).unwrap();
        let a = lib.create_folder(None, "A").unwrap();
        let a1 = lib.create_folder(Some(&a), "A1").unwrap();
        let a2 = lib.create_folder(Some(&a), "A2").unwrap();
        let b = lib.create_folder(None, "B").unwrap();
        let c = lib.create_folder(None, "C").unwrap();
        let d = lib.create_folder(None, "D").unwrap();
        let tree = |l: &Library| -> Vec<String> {
            fn go(list: &[Folder], depth: usize, out: &mut Vec<String>) {
                for f in list {
                    out.push(format!("{}{}", " ".repeat(depth), f.name));
                    go(&f.children, depth + 1, out);
                }
            }
            let mut out = Vec::new();
            go(&l.metadata.folders, 0, &mut out);
            out
        };

        // One undo step puts everything back.
        let start = tree(&lib);
        lib.begin_transaction("move");
        lib.move_folders(&[c.clone(), d.clone()], &a, FolderPlace::Into).unwrap();
        let t = lib.end_transaction().unwrap();
        lib.revert(t).unwrap();
        assert_eq!(tree(&Library::open(lib.root()).unwrap()), start);

        // Picked in any order, they keep their tree order: A2 then C.
        lib.move_folders(&[c.clone(), a2.clone()], &b, FolderPlace::Above).unwrap();
        assert_eq!(tree(&lib), ["A", " A1", "A2", "C", "B", "D"]);
        lib.move_folders(&[a2.clone(), d.clone()], &a1, FolderPlace::Below).unwrap();
        assert_eq!(tree(&lib), ["A", " A1", " A2", " D", "C", "B"]);
        // Into: after the last child. A moves with its children; A1, whose
        // parent moves too, stays inside it.
        lib.move_folders(&[a1.clone(), a.clone()], &b, FolderPlace::Into).unwrap();
        assert_eq!(tree(&Library::open(lib.root()).unwrap()), ["C", "B", " A", "  A1", "  A2", "  D"]);

        let before = tree(&lib);
        assert!(lib.move_folders(std::slice::from_ref(&b), &a2, FolderPlace::Into).is_err(), "into its own descendant");
        assert!(lib.move_folders(&[b.clone(), c.clone()], &c, FolderPlace::Above).is_err(), "onto a moved folder");
        assert!(lib.move_folders(std::slice::from_ref(&c), "NOPE", FolderPlace::Above).is_err());
        assert_eq!(tree(&lib), before);
    }

    #[test]
    fn duplicating_a_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, items) = with_items(tmp.path());
        let a = lib.create_folder(None, "A").unwrap();
        let a1 = lib.create_folder(Some(&a), "A1").unwrap();
        lib.create_folder(Some(&a1), "A1x").unwrap();
        let b = lib.create_folder(None, "B").unwrap();
        lib.set_folder_icon(&a1, Some("star"), Some("blue")).unwrap();
        lib.set_folder_password(&a1, Some(("pw", "hint"))).unwrap();
        lib.set_folder_description(&a1, "about").unwrap();
        lib.set_folder_cover(&a1, Some(&items[0])).unwrap();
        lib.set_folder_order(&a1, Some(FolderOrder::Random), false).unwrap();
        file_into(&mut lib, &items[0], &[&a1]);
        let original = lib.metadata.folders[0].children[0].clone();

        let copy = lib.duplicate_folder(&a1).unwrap();
        let got = Library::open(lib.root()).unwrap();
        let kids = &got.metadata.folders[0].children;
        assert_eq!(names(kids), ["A1", "A1"], "right after the original, same name");
        assert_eq!(kids[0], original, "the original is untouched");
        let dup = &kids[1];
        assert_eq!(dup.id, copy);
        assert_ne!(dup.children[0].id, original.children[0].id);
        assert_eq!(dup.children[0].name, "A1x");
        assert_eq!((dup.icon.as_deref(), dup.icon_color.as_deref(), dup.description.as_str()), (Some("star"), Some("blue"), "about"));
        assert_eq!((dup.cover_id.as_deref(), dup.order_by.as_deref(), dup.sort_increase), (Some(items[0].as_str()), Some("RANDOM"), Some(false)));
        assert!(!dup.is_locked() && dup.password_tips.is_empty());
        assert!(dup.modification_time >= original.modification_time);
        assert_eq!(got.item(&items[0]).unwrap().folders, std::slice::from_ref(&a1), "items are not copied");

        // A top-level folder lands after itself, before the next one.
        lib.duplicate_folder(&b).unwrap();
        assert_eq!(names(&lib.metadata.folders), ["A", "B", "B"]);
        assert!(lib.duplicate_folder("NOPE").is_err());
    }

    #[test]
    fn passwords_encode_like_btoa() {
        assert_eq!(encode_password("123").as_deref(), Some("MTIz"));
        // btoa takes each UTF-16 unit below 256 as a byte, not UTF-8.
        assert_eq!(encode_password("é").as_deref(), Some("6Q=="));
        assert_eq!(encode_password("密码"), None);
    }
}
