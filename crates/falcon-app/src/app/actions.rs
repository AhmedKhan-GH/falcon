//! Library changes requested while drawing (`Action`) and the reducer that
//! applies them once a frame, under the library lock, as one undoable
//! transaction; with the undo history labels, the bulk-change confirmation
//! and the per-frame edit bracket.

use std::time::Instant;

use falcon_library::Library;
use falcon_library::model::Item;
use falcon_library::undo::{Transaction, WriteMode};

use super::{App, Confirm, image_edit};
use crate::logic::{self, Scope};

/// An action on this many items or more asks first (Eagle's bulk confirm).
pub(crate) const BULK_CONFIRM: usize = 100;

/// Whether an action on `n` items asks first.
pub(crate) fn needs_confirm(n: usize, threshold: usize) -> bool {
    n >= threshold
}

/// Library changes requested while drawing, applied at the end of a frame.
pub(crate) enum Action {
    CreateFolder { parent: Option<String> },
    /// A new folder right after this one, among its siblings.
    CreateSibling(String),
    /// Items taken out of a folder (Edit ▸ Remove from Folder).
    RemoveFromFolder(Vec<String>, String),
    RenameFolder(String, String),
    /// Item id and new extension (the inspector's Type row).
    SetExtension(String, String),
    SetFolderDescription(String, String),
    SetSmartDescription(String, String),
    AddToFolder(Vec<String>, String),
    /// Original files to open in the default app (true) or show in Finder
    /// (false); queued through `push_bulk` when there are several.
    OpenFiles(Vec<std::path::PathBuf>, bool),
    /// Items dropped on a sidebar folder: moved out of the open folder, or
    /// added (`logic::folder_drop`).
    DropOnFolder(Vec<String>, String, logic::FolderDrop),
    Trash(Vec<String>, bool),
    /// Move to Trash from a view: inside a folder, when some of the items
    /// are in other folders too, ask first (`Confirm::TrashInFolder`).
    TrashAsk(Vec<String>),
    /// The answer: take the items out of this folder, trashing only those
    /// in no other folder (`true`), or trash them all.
    TrashInFolder(Vec<String>, String, bool),
    DeletePermanently(Vec<String>),
    EmptyTrash,
    CreateSmart(String, Vec<serde_json::Value>),
    UpdateSmart(String, String, Vec<serde_json::Value>),
    RenameTag(String, String),
    MoveFolder(String, Option<String>),
    /// Tags added to items by the tag picker (`Library::add_tags_to_items`);
    /// new tags created while the picker shows a group join it:
    /// `(group id, new tags)`.
    AddTags(Vec<String>, Vec<String>, Option<(String, Vec<String>)>),
    /// Tags taken off items (`Library::remove_tags_from_items`).
    RemoveTags(Vec<String>, Vec<String>),
    /// The note or URL of several items (`Library::set_notes`, `set_urls`).
    SetNotes(Vec<String>, String),
    SetUrls(Vec<String>, String),
    /// Paste Tags and Clear Tags (`Library::paste_tags`, `clear_tags`).
    PasteTags(Vec<String>, Vec<String>),
    ClearTags(Vec<String>),
    BatchRating(Vec<String>, u64),
    QuickAdd(&'static str, String),
    QuickRemove(String),
    /// Pins or moves `(kind, id)` beside `target`; below it when true.
    PlaceQuick(&'static str, String, String, bool),
    /// Tags deleted, starred or unstarred, or merged into one name, from
    /// the All Tags page.
    DeleteTags(Vec<String>),
    StarTags(Vec<String>, bool),
    MergeTags(Vec<String>, String),
    ClearTagHistory,
    DeleteSmart(String),
    /// Edit ▸ Move Folder on the open folder or smart folder.
    Arrange(crate::organize::Arrange),
    /// Folder icons, sorts, passwords and reordering, nested smart
    /// folders, tag groups and saved filters (see `ui/organize.rs`).
    Organize(crate::ui::organize::OrgAction),
    /// Thumbnails, palettes, covers, rotate, crop and comments (`app/image_edit.rs`).
    Edit(image_edit::Op),
    /// Several folders at once, Move Folder and folder deletion (`ui/sidebar/folders.rs`).
    Folders(crate::ui::sidebar::folders::FolderAction),
    /// Sets or removes the thumbnail background of these items.
    Background(Vec<String>, Option<logic::ThumbBg>),
    /// An item renamed from the text viewer's title.
    RenameItem(String, String),
    /// File ▸ New ▸ Text File, into these folders.
    NewTextFile(Vec<String>),
    /// The Duplicates page's merge: each kept item takes its group's
    /// values and the rest go to the Trash (`falcon_library::merge`).
    MergeDuplicates(Vec<falcon_library::merge::Merge>),
    /// The duplicate dialog's answer for these held files.
    ResolveDuplicates(Vec<crate::import_dupes::Pending>, crate::import_dupes::Choice),
}

/// Most undo steps kept.
pub(crate) const UNDO_LIMIT: usize = 100;

pub(crate) fn push_capped(stack: &mut Vec<Transaction>, tx: Transaction) {
    stack.push(tx);
    if stack.len() > UNDO_LIMIT {
        stack.remove(0);
    }
}

/// The undo name of an action, as "Undid: …" shows it.
pub(crate) fn action_label(action: &Action) -> String {
    match action {
        Action::CreateFolder { .. } | Action::CreateSibling(_) => "New folder".into(),
        Action::RemoveFromFolder(ids, _) => format!("Remove {} from folder", ids.len()),
        Action::RenameFolder(..) => "Rename folder".into(),
        Action::SetExtension(..) => "Change extension".into(),
        Action::SetFolderDescription(..) => "Edit folder description".into(),
        Action::SetSmartDescription(..) => "Edit smart folder description".into(),
        Action::AddToFolder(ids, _) => format!("Add {} to folder", ids.len()),
        Action::OpenFiles(files, _) => format!("Open {} files", files.len()),
        Action::DropOnFolder(ids, _, logic::FolderDrop::Move(_)) => format!("Move {} to folder", ids.len()),
        Action::DropOnFolder(ids, _, logic::FolderDrop::Add) => format!("Add {} to folder", ids.len()),
        Action::Trash(ids, true) => format!("Move {} to Trash", ids.len()),
        Action::Background(ids, _) => format!("Set the thumbnail background of {}", ids.len()),
        Action::Trash(ids, false) => format!("Restore {}", ids.len()),
        Action::TrashAsk(ids) | Action::TrashInFolder(ids, _, false) => format!("Move {} to Trash", ids.len()),
        Action::TrashInFolder(ids, _, true) => format!("Remove {} from folder", ids.len()),
        Action::DeletePermanently(ids) => format!("Delete {}", ids.len()),
        Action::EmptyTrash => "Empty Trash".into(),
        Action::CreateSmart(..) => "New smart folder".into(),
        Action::UpdateSmart(..) => "Edit smart folder".into(),
        Action::DeleteSmart(_) => "Delete smart folder".into(),
        Action::Arrange(_) => "Move folder".into(),
        Action::RenameTag(..) => "Rename tag".into(),
        Action::DeleteTags(t) => if t.len() == 1 { "Delete tag".into() } else { format!("Delete {} tags", t.len()) },
        Action::StarTags(_, true) => "Star tags".into(),
        Action::StarTags(_, false) => "Unstar tags".into(),
        Action::MergeTags(t, _) => format!("Merge {} tags", t.len()),
        Action::ClearTagHistory => "Clear tag history".into(),
        Action::MoveFolder(..) => "Move folder".into(),
        Action::AddTags(ids, ..) => format!("Tag {}", ids.len()),
        Action::RemoveTags(ids, _) => format!("Untag {}", ids.len()),
        Action::PasteTags(ids, _) => format!("Paste tags to {}", ids.len()),
        Action::ClearTags(ids) => format!("Clear tags of {}", ids.len()),
        Action::SetNotes(ids, _) => format!("Edit the notes of {}", ids.len()),
        Action::SetUrls(ids, _) => format!("Edit the URLs of {}", ids.len()),
        Action::BatchRating(ids, _) => format!("Rate {}", ids.len()),
        Action::QuickAdd(..) => "Add to Quick Access".into(),
        Action::QuickRemove(_) => "Remove from Quick Access".into(),
        Action::PlaceQuick(..) => "Move in Quick Access".into(),
        Action::Organize(a) => organize_label(a),
        Action::MergeDuplicates(m) => format!("Merge {} duplicates", m.len()),
        Action::ResolveDuplicates(files, crate::import_dupes::Choice::UseExisting) => format!("Use existing for {}", files.len()),
        Action::ResolveDuplicates(files, _) => format!("Import {}", files.len()),
        Action::Edit(op) => op.label(),
        Action::Folders(a) => a.label(),
        Action::RenameItem(..) => "Rename".into(),
        Action::NewTextFile(_) => "New text file".into(),
    }
}

/// The undo name of an organising action.
pub(crate) fn organize_label(a: &crate::ui::organize::OrgAction) -> String {
    use crate::ui::organize::OrgAction as O;
    match a {
        O::FolderOrder(..) | O::SmartOrder(..) => "Change sort".into(),
        O::FolderIcon(..) | O::SmartIcon(..) => "Change icon".into(),
        O::ReorderFolder(..) => "Move folder".into(),
        O::MoveSmart(..) => "Move smart folder".into(),
        O::FolderPassword(_, Some(_)) => "Set password".into(),
        O::FolderPassword(_, None) => "Remove password".into(),
        O::CreateSmart { .. } => "New smart folder".into(),
        O::CreateGroup(_) => "New tag group".into(),
        O::RenameGroup(..) => "Rename tag group".into(),
        O::GroupColor(..) => "Recolour tag group".into(),
        O::DeleteGroup(_) => "Delete tag group".into(),
        O::AddToGroup(_, tags) => format!("Group {} tags", tags.len()),
        O::AddToGroupToo(_, tags) => format!("Group {} tags", tags.len()),
        O::RemoveFromGroup(_, tags) => format!("Ungroup {} tags", tags.len()),
        O::SaveFilter(..) => "Save filter".into(),
        O::RenameFilter(..) => "Rename saved filter".into(),
        O::DeleteFilter(_) => "Remove saved filter".into(),
        O::MoveFilter(..) => "Move saved filter".into(),
        O::MoveManual(_, ids, _) => format!("Reorder {}", ids.len()),
        O::CreateSmartGroup(_) => "New smart folder group".into(),
        O::RenameSmart(..) => "Rename smart folder".into(),
        O::SortFolders(..) => "Sort folders by title".into(),
        O::MoveGroup(..) => "Move tag group".into(),
        O::GroupDescription(..) => "Describe tag group".into(),
        O::FolderSettings(..) => "Change folder settings".into(),
    }
}

/// Names a frame's undo step from its actions; a frame without actions
/// only had inspector edits.
pub(crate) fn undo_label(labels: &[String]) -> String {
    match labels {
        [] => "Edit item".into(),
        [one] => one.clone(),
        [first, rest @ ..] => format!("{first} and {} more", rest.len()),
    }
}

impl App {
    /// Queues `action`, which changes `n` items, as Eagle does: at once for
    /// fewer than `BULK_CONFIRM`, else after the user confirms the dialog.
    pub(crate) fn push_bulk(&mut self, n: usize, action: Action) {
        self.push_bulk_at(n, BULK_CONFIRM, action);
    }

    /// Like `push_bulk` with its own `threshold` (deleting folders asks from
    /// 50 items).
    pub(crate) fn push_bulk_at(&mut self, n: usize, threshold: usize, action: Action) {
        if !needs_confirm(n, threshold) {
            self.ws.actions.push(action);
            return;
        }
        match &mut self.ws.bulk {
            Some((held, actions)) => {
                *held = (*held).max(n);
                actions.push(action);
            }
            None => self.ws.bulk = Some((n, vec![action])),
        }
    }

    pub(crate) fn apply_actions(&mut self, lib: &mut Library) {
        for action in std::mem::take(&mut self.ws.actions) {
            self.ws.frame_labels.push(action_label(&action));
            let result = match action {
                Action::CreateFolder { parent } => lib.create_folder(parent.as_deref(), "New Folder").map(|id| {
                    if let Some(p) = parent {
                        self.ws.collapsed.remove(&p);
                    }
                    self.ws.renaming = Some((id, "New Folder".into()));
                }),
                Action::CreateSibling(of) => match crate::organize::locate(&lib.metadata.folders, &of).map(|(p, at)| (p.map(str::to_string), at)) {
                    Some((parent, at)) => lib.create_folder(parent.as_deref(), "New Folder").and_then(|id| {
                        lib.reorder_folder(&id, parent.as_deref(), at + 1)?;
                        if let Some(p) = &parent {
                            self.ws.collapsed.remove(p);
                        }
                        self.ws.renaming = Some((id, "New Folder".into()));
                        Ok(())
                    }),
                    None => Ok(()),
                },
                Action::RemoveFromFolder(ids, fid) => {
                    let mut r = Ok(());
                    let mut n = 0;
                    for id in &ids {
                        if let Some(mut item) = lib.item(id).cloned()
                            && item.folders.contains(&fid)
                        {
                            item.folders.retain(|f| *f != fid);
                            r = r.and(lib.update_item(item));
                            n += 1;
                        }
                    }
                    let name = logic::find_folder(&lib.metadata.folders, &fid).map_or_else(String::new, |f| f.name.clone());
                    self.flash_undo(format!("Removed {n} from {name}"));
                    r
                }
                Action::RenameFolder(id, name) => lib.rename_folder(&id, &name),
                Action::SetExtension(id, ext) => {
                    let r = lib.set_extension(&id, &ext);
                    if r.is_ok() {
                        self.ws.loader.forget(&id);
                        self.flash(format!("Extension changed to .{ext}"));
                    }
                    r
                }
                Action::SetFolderDescription(id, text) => lib.set_folder_description(&id, &text),
                Action::SetSmartDescription(id, text) => lib.set_smart_folder_description(&id, &text),
                Action::AddToFolder(ids, fid) => {
                    self.note_folder_use(std::slice::from_ref(&fid));
                    let n = ids.len();
                    let mut r = Ok(());
                    for id in ids {
                        if let Some(mut item) = lib.item(&id).cloned()
                            && falcon_library::autotag::join_folder(&mut item, &lib.metadata.folders, &fid)
                        {
                            r = r.and(lib.update_item(item));
                        }
                    }
                    let name = logic::find_folder(&lib.metadata.folders, &fid).map_or_else(String::new, |f| f.name.clone());
                    self.flash_folder(format!("Added {n} to"), &name, &fid, true);
                    r
                }
                Action::DropOnFolder(ids, fid, op) => {
                    self.note_folder_use(std::slice::from_ref(&fid));
                    let mut r = Ok(());
                    let mut moved = Vec::new();
                    let tags = falcon_library::autotag::folder_auto_tags(&lib.metadata.folders, &fid);
                    for id in &ids {
                        if let Some(mut item) = lib.item(id).cloned()
                            && logic::apply_folder_drop(&mut item, &fid, &op, &tags)
                        {
                            r = r.and(lib.update_item(item));
                            moved.push(id);
                        }
                    }
                    // Moved items leave the open folder (unless they went into
                    // a subfolder it shows): the next item is selected.
                    let shown_below = self.display.subfolders
                        && matches!(&self.ws.scope, Scope::Folder(open) if logic::folder_tree(&lib.metadata.folders, open).contains(&fid));
                    if matches!(op, logic::FolderDrop::Move(_)) && !shown_below {
                        let moved: Vec<String> = moved.into_iter().cloned().collect();
                        self.select_after_removal(&moved);
                    }
                    let name = logic::find_folder(&lib.metadata.folders, &fid).map_or_else(String::new, |f| f.name.clone());
                    self.flash_folder(logic::folder_drop_lead(&op, ids.len()), &name, &fid, true);
                    r
                }
                Action::CreateSmart(name, conditions) => lib.create_smart_folder(&name, conditions).map(|id| {
                    self.ws.scope = Scope::Smart(id);
                    self.ws.selected.clear();
                }),
                Action::UpdateSmart(id, name, conditions) => lib.update_smart_folder(&id, &name, conditions),
                Action::DeleteSmart(id) => {
                    if self.ws.scope == Scope::Smart(id.clone()) {
                        self.ws.scope = Scope::All;
                    }
                    lib.delete_smart_folder(&id)
                }
                Action::Arrange(how) => match self.ws.scope.clone() {
                    Scope::Folder(id) => match crate::organize::arrange(&lib.metadata.folders, &id, how) {
                        Some((parent, index)) => lib.reorder_folder(&id, parent.as_deref(), index),
                        None => Ok(()),
                    },
                    Scope::Smart(id) => match crate::organize::arrange(&lib.smart_folders(), &id, how) {
                        Some((parent, index)) => lib.move_smart_folder(&id, parent.as_deref(), index),
                        None => Ok(()),
                    },
                    _ => Ok(()),
                },
                Action::MoveFolder(id, parent) => {
                    if let Some(p) = &parent {
                        self.ws.collapsed.remove(p);
                    }
                    lib.move_folder(&id, parent.as_deref())
                }
                Action::AddTags(ids, tags, group) => lib.add_tags_to_items(&ids, &tags).and_then(|_| match group {
                    Some((g, new)) if !new.is_empty() => lib.add_tags_to_group(&g, &new),
                    _ => Ok(()),
                }),
                Action::RemoveTags(ids, tags) => lib.remove_tags_from_items(&ids, &tags).map(|_| ()),
                Action::PasteTags(ids, tags) => lib.paste_tags(&ids, &tags).map(|n| self.flash_undo(format!("Pasted {} to {}", crate::ui::dialogs::export::plural(tags.len(), "tag"), crate::ui::dialogs::export::plural(n, "item")))),
                Action::SetNotes(ids, note) => lib.set_notes(&ids, &note).map(|_| ()),
                Action::SetUrls(ids, url) => lib.set_urls(&ids, &url).map(|_| ()),
                Action::ClearTags(ids) => lib.clear_tags(&ids).map(|n| self.flash_undo(format!("Cleared the tags of {}", crate::ui::dialogs::export::plural(n, "item")))),
                Action::BatchRating(ids, star) => {
                    let mut r = Ok(());
                    for id in &ids {
                        let Some(mut item) = lib.item(id).cloned() else { continue };
                        if star == 0 {
                            item.extra.remove("star");
                        } else {
                            item.extra.insert("star".into(), star.into());
                        }
                        r = r.and(lib.update_item(item));
                    }
                    r
                }
                Action::QuickAdd(kind, id) => lib.add_quick_access(kind, &id),
                Action::QuickRemove(id) => lib.remove_quick_access(&id),
                Action::PlaceQuick(kind, id, target, below) => lib.place_quick_access(kind, &id, &target, below),
                Action::RenameTag(from, to) => lib.rename_tag(&from, &to).map(|n| {
                    if self.ws.scope == Scope::Tag(from.clone()) {
                        self.ws.scope = Scope::Tag(to.clone());
                    }
                    self.flash_undo(format!("Renamed “{from}” to “{to}” on {n} items"));
                }),
                Action::DeleteTags(tags) => lib.delete_tags(&tags).map(|n| {
                    self.ws.org.tag_sel.retain(|t| !tags.contains(t));
                    let what = if let [one] = tags.as_slice() { format!("“{one}”") } else { crate::ui::dialogs::export::plural(tags.len(), "tag") };
                    self.flash_undo(format!("Removed {what} from {}", crate::ui::dialogs::export::plural(n, "item")));
                }),
                Action::StarTags(tags, on) => lib.star_tags(&tags, on),
                Action::MergeTags(tags, into) => lib.merge_tags(&tags, &into).map(|n| {
                    let into = into.trim().to_string();
                    if let Scope::Tag(t) = &self.ws.scope
                        && tags.contains(t)
                    {
                        self.ws.scope = Scope::Tag(into.clone());
                    }
                    self.ws.org.tag_sel = vec![into.clone()];
                    self.flash_undo(format!("Merged {} into “{into}” on {}", crate::ui::dialogs::export::plural(tags.len(), "tag"), crate::ui::dialogs::export::plural(n, "item")));
                }),
                Action::OpenFiles(files, default) => {
                    if default {
                        crate::open_with::open_default(&files);
                        self.used(&files);
                    } else {
                        crate::open_with::reveal(&files);
                    }
                    Ok(())
                }
                Action::ClearTagHistory => lib.clear_tag_history().map(|_| self.flash("Cleared the tag history")),
                Action::DeletePermanently(ids) => {
                    self.select_after_removal(&ids);
                    let r = lib.delete_permanently(&ids);
                    self.flash(format!("Deleted {}", ids.len()));
                    r
                }
                Action::EmptyTrash => lib.empty_trash().map(|n| {
                    self.ws.selected.clear();
                    self.flash(format!("Deleted {n}"));
                }),
                Action::Trash(ids, trash) => {
                    // The Duplicates page keeps its own selection rules.
                    if self.ws.scope == Scope::Duplicates {
                        self.ws.selected.retain(|s| !ids.contains(s));
                    } else {
                        self.select_after_removal(&ids);
                    }
                    let r = lib.set_trashed(&ids, trash);
                    if trash {
                        self.flash_undo(format!("Moved {} to Trash", ids.len()));
                    } else {
                        self.flash(format!("Restored {}", ids.len()));
                    }
                    r
                }
                Action::TrashAsk(ids) => {
                    let items: Vec<&Item> = ids.iter().filter_map(|id| lib.item(id)).collect();
                    match logic::trash_question(&self.ws.scope, self.ws.folders.combined(), &items) {
                        Some(fid) => {
                            self.ws.confirm = Some(Confirm::TrashInFolder(ids, fid));
                            Ok(())
                        }
                        None => {
                            self.select_after_removal(&ids);
                            let r = lib.set_trashed(&ids, true);
                            self.flash(format!("Moved {} to Trash", ids.len()));
                            r
                        }
                    }
                }
                Action::TrashInFolder(ids, fid, only) => {
                    self.select_after_removal(&ids);
                    let (mut r, mut trash) = (Ok(()), Vec::new());
                    for id in &ids {
                        let Some(mut item) = lib.item(id).cloned() else { continue };
                        if only && logic::leave_folder(&mut item, &fid) {
                            r = r.and(lib.update_item(item));
                        } else {
                            trash.push(id.clone());
                        }
                    }
                    let removed = ids.len() - trash.len();
                    r = r.and(lib.set_trashed(&trash, true));
                    self.flash(match (removed, trash.len()) {
                        (0, t) => format!("Moved {t} to Trash"),
                        (n, 0) => format!("Removed {n} from the folder"),
                        (n, t) => format!("Removed {n} from the folder and moved {t} to Trash"),
                    });
                    r
                }
                Action::MergeDuplicates(merges) => lib.merge_duplicates(&merges).map(|report| {
                    self.ws.selected.retain(|id| lib.item(id).is_some_and(|i| !i.is_deleted));
                    self.flash(format!("Merged {} and moved {} to the Trash", crate::ui::dialogs::export::plural(report.kept, "group"), report.trashed));
                    self.ws.duplicates.merged(report);
                }),
                Action::Organize(a) => self.apply_organize(lib, a),
                Action::ResolveDuplicates(files, choice) => {
                    let n = files.len();
                    let mut r = Ok(());
                    for p in files {
                        r = r.and(crate::import_dupes::resolve(lib, p, choice).map(|_| ()));
                    }
                    self.flash(crate::import_dupes::notice(choice, n));
                    r
                }
                Action::Edit(op) => self.apply_edit(lib, op),
                Action::Folders(a) => self.apply_folder_action(lib, a),
                Action::Background(ids, bg) => {
                    let mut r = Ok(());
                    for id in &ids {
                        let Some(mut item) = lib.item(id).cloned() else { continue };
                        if logic::set_thumb_bg(&mut item, bg) {
                            r = r.and(lib.update_item(item));
                        }
                    }
                    r
                }
                Action::NewTextFile(folders) => self.new_text_file(lib, folders),
                Action::RenameItem(id, name) => match lib.item(&id).cloned() {
                    Some(mut item) => {
                        item.name = name;
                        // The inspector's title field shows the new name.
                        self.ws.edit = None;
                        lib.update_item(item)
                    }
                    None => Ok(()),
                },
            };
            if let Err(e) = result {
                self.flash_error(format!("Could not save: {e}"));
            }
        }
    }

    /// Start of a frame with the library locked: batched writes on, a
    /// pending ⌘Z or ⇧⌘Z applied, and a transaction opened for the frame's
    /// edits.
    pub(crate) fn begin_frame_edits(&mut self, lib: &mut Library) {
        if self.ws.undo_root.as_deref() != Some(lib.root()) {
            self.ws.undo.clear();
            self.ws.redo.clear();
            self.ws.undo_root = Some(lib.root().to_path_buf());
        }
        if lib.write_mode() != WriteMode::Batched
            && let Err(e) = lib.set_write_mode(WriteMode::Batched)
        {
            self.flash_error(format!("Could not save: {e}"));
        }
        if let Some(undo) = self.ws.undo_request.take() {
            let (from, to) = if undo { (&mut self.ws.undo, &mut self.ws.redo) } else { (&mut self.ws.redo, &mut self.ws.undo) };
            match from.pop() {
                None => self.flash(if undo { "Nothing to undo" } else { "Nothing to redo" }),
                Some(tx) => {
                    let label = tx.label.clone();
                    match lib.revert(tx) {
                        Ok(inverse) => {
                            push_capped(to, inverse);
                            // Reload the inspector's text from the restored item.
                            self.ws.edit = None;
                            self.ws.multi_edit = None;
                            self.ws.folder_edit = None;
                            self.flash(format!("{}: {label}", if undo { "Undid" } else { "Redid" }));
                        }
                        Err(e) => self.flash_error(format!("Could not save: {e}")),
                    }
                }
            }
        }
        lib.begin_transaction("Edit item");
    }

    /// End of a frame: the frame's edits become one undo step (a permanent
    /// delete clears the history instead), and pending writes are flushed.
    pub(crate) fn end_frame_edits(&mut self, lib: &mut Library) {
        let labels = std::mem::take(&mut self.ws.frame_labels);
        if let Some(mut tx) = lib.end_transaction() {
            if tx.irreversible {
                self.ws.undo.clear();
                self.ws.redo.clear();
            } else {
                tx.label = undo_label(&labels);
                push_capped(&mut self.ws.undo, tx);
                self.ws.redo.clear();
            }
        }
        if lib.has_pending_writes() {
            match lib.flush() {
                Ok(report) if report.wants_backup() => {
                    self.ws.backup.changed(Instant::now());
                    self.ctx.request_repaint_after(crate::backups::DEBOUNCE);
                }
                Ok(_) => {}
                Err(e) => self.flash_error(format!("Could not save: {e}")),
            }
        }
    }

    /// Starts each open library's metadata backup on a background thread
    /// when it is due (a second after a write, and every 30 minutes), and
    /// asks for a frame when the next one will be.
    pub(crate) fn run_backups(&mut self) {
        let now = Instant::now();
        let mut wait = crate::backups::PERIOD;
        for ws in std::iter::once(&mut self.ws).chain(self.others.parked_mut()) {
            let (Some(_), Some(root)) = (&ws.lib, &ws.root) else { continue };
            if let Some(running) = ws.backup.start(now) {
                crate::backups::spawn(root.clone(), running);
            }
            wait = wait.min(ws.backup.wait(now));
        }
        // A backup that waits for a running one is retried shortly.
        self.ctx.request_repaint_after(wait.max(std::time::Duration::from_millis(100)));
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;

    #[test]
    fn confirm_threshold() {
        assert!(!needs_confirm(99, BULK_CONFIRM));
        assert!(needs_confirm(100, BULK_CONFIRM));
        assert!(!needs_confirm(49, 50));
        assert!(needs_confirm(50, 50));
        assert_eq!(action_label(&Action::OpenFiles(vec!["/a".into(), "/b".into()], false)), "Open 2 files");
    }

    #[test]
    fn undo_labels() {
        assert_eq!(action_label(&Action::Trash(vec!["a".into(), "b".into(), "c".into()], true)), "Move 3 to Trash");
        assert_eq!(action_label(&Action::RenameFolder("f".into(), "x".into())), "Rename folder");
        assert_eq!(action_label(&Action::SetExtension("i".into(), "png".into())), "Change extension");
        assert_eq!(action_label(&Action::SetFolderDescription("f".into(), "x".into())), "Edit folder description");
        assert_eq!(action_label(&Action::SetSmartDescription("f".into(), "x".into())), "Edit smart folder description");
        assert_eq!(undo_label(&[]), "Edit item");
        assert_eq!(undo_label(&["Rename folder".into()]), "Rename folder");
        assert_eq!(undo_label(&["New folder".into(), "Tag 2".into(), "Rate 2".into()]), "New folder and 2 more");
    }

    #[test]
    fn undo_history_is_capped() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("Undo.library")).unwrap();
        let mut stack = Vec::new();
        for n in 0..UNDO_LIMIT + 5 {
            lib.begin_transaction(n.to_string());
            lib.create_folder(None, "F").unwrap();
            push_capped(&mut stack, lib.end_transaction().unwrap());
        }
        assert_eq!(stack.len(), UNDO_LIMIT);
        assert_eq!(stack[0].label, "5");
    }
}
