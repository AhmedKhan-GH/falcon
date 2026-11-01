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

    #[test]
    fn passwords_encode_like_btoa() {
        assert_eq!(encode_password("123").as_deref(), Some("MTIz"));
        // btoa takes each UTF-16 unit below 256 as a byte, not UTF-8.
        assert_eq!(encode_password("é").as_deref(), Some("6Q=="));
        assert_eq!(encode_password("密码"), None);
    }
}
