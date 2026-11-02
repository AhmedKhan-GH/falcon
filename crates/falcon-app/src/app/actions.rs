//! Library changes requested while drawing (`Action`) and the reducer that
//! applies them once a frame, under the library lock, as one undoable
//! transaction; with the undo history labels, the bulk-change confirmation
//! and the per-frame edit bracket.

use std::time::Instant;

use falcon_library::Library;
use falcon_library::model::Item;
use falcon_library::undo::{Transaction, WriteMode};

use super::{App, Confirm, image_edit};
use crate::locale::{tr, tr_n, tr_nf, trf};
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
    /// File ▸ New…: a blank document from a template, into these folders.
    NewDocument(falcon_library::templates::Template, Vec<String>),
    /// The Duplicates page's merge: each kept item takes its group's
    /// values and the rest go to the Trash (`falcon_library::merge`).
    MergeDuplicates(Vec<falcon_library::merge::Merge>),
    /// The duplicate dialog's answer for these held files.
    ResolveDuplicates(Vec<crate::import_dupes::Pending>, crate::import_dupes::Choice),
    /// An action's steps run on items (`actions/`).
    RunSteps(crate::actions::Run),
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
        Action::CreateFolder { .. } | Action::CreateSibling(_) => tr("New folder").into(),
        Action::RemoveFromFolder(ids, _) => trf("Remove {n} from folder", &[("n", &ids.len())]),
        Action::RenameFolder(..) => tr("Rename folder").into(),
        Action::SetExtension(..) => tr("Change extension").into(),
        Action::SetFolderDescription(..) => tr("Edit folder description").into(),
        Action::SetSmartDescription(..) => tr("Edit smart folder description").into(),
        Action::AddToFolder(ids, _) => trf("Add {n} to folder", &[("n", &ids.len())]),
        Action::OpenFiles(files, _) => tr_n(files.len(), "Open {n} file", "Open {n} files"),
        Action::DropOnFolder(ids, _, logic::FolderDrop::Move(_)) => trf("Move {n} to folder", &[("n", &ids.len())]),
        Action::DropOnFolder(ids, _, logic::FolderDrop::Add) => trf("Add {n} to folder", &[("n", &ids.len())]),
        Action::Trash(ids, true) => trf("Move {n} to Trash", &[("n", &ids.len())]),
        Action::Background(ids, _) => trf("Set the thumbnail background of {n}", &[("n", &ids.len())]),
        Action::Trash(ids, false) => trf("Restore {n}", &[("n", &ids.len())]),
        Action::TrashAsk(ids) | Action::TrashInFolder(ids, _, false) => trf("Move {n} to Trash", &[("n", &ids.len())]),
        Action::TrashInFolder(ids, _, true) => trf("Remove {n} from folder", &[("n", &ids.len())]),
        Action::DeletePermanently(ids) => trf("Delete {n}", &[("n", &ids.len())]),
        Action::EmptyTrash => tr("Empty Trash").into(),
        Action::CreateSmart(..) => tr("New smart folder").into(),
        Action::UpdateSmart(..) => tr("Edit smart folder").into(),
        Action::DeleteSmart(_) => tr("Delete smart folder").into(),
        Action::Arrange(_) => tr("Move folder").into(),
        Action::RenameTag(..) => tr("Rename tag").into(),
        Action::DeleteTags(t) => tr_n(t.len(), "Delete {n} tag", "Delete {n} tags"),
        Action::StarTags(_, true) => tr("Star tags").into(),
        Action::StarTags(_, false) => tr("Unstar tags").into(),
        Action::MergeTags(t, _) => tr_n(t.len(), "Merge {n} tag", "Merge {n} tags"),
        Action::ClearTagHistory => tr("Clear tag history").into(),
        Action::MoveFolder(..) => tr("Move folder").into(),
        Action::AddTags(ids, ..) => trf("Tag {n}", &[("n", &ids.len())]),
        Action::RemoveTags(ids, _) => trf("Untag {n}", &[("n", &ids.len())]),
        Action::PasteTags(ids, _) => trf("Paste tags to {n}", &[("n", &ids.len())]),
        Action::ClearTags(ids) => trf("Clear tags of {n}", &[("n", &ids.len())]),
        Action::SetNotes(ids, _) => trf("Edit the notes of {n}", &[("n", &ids.len())]),
        Action::SetUrls(ids, _) => trf("Edit the URLs of {n}", &[("n", &ids.len())]),
        Action::BatchRating(ids, _) => trf("Rate {n}", &[("n", &ids.len())]),
        Action::QuickAdd(..) => tr("Add to Quick Access").into(),
        Action::QuickRemove(_) => tr("Remove from Quick Access").into(),
        Action::PlaceQuick(..) => tr("Move in Quick Access").into(),
        Action::Organize(a) => organize_label(a),
        Action::MergeDuplicates(m) => tr_n(m.len(), "Merge {n} duplicate", "Merge {n} duplicates"),
        Action::ResolveDuplicates(files, crate::import_dupes::Choice::UseExisting) => trf("Use existing for {n}", &[("n", &files.len())]),
        Action::ResolveDuplicates(files, _) => trf("Import {n}", &[("n", &files.len())]),
        Action::Edit(op) => op.label(),
        Action::Folders(a) => a.label(),
        Action::RenameItem(..) => tr("Rename").into(),
        Action::NewDocument(..) => tr("New document").into(),
        Action::RunSteps(run) => crate::actions::app::run_label(run),
    }
}

/// The undo name of an organising action.
pub(crate) fn organize_label(a: &crate::ui::organize::OrgAction) -> String {
    use crate::ui::organize::OrgAction as O;
    match a {
        O::FolderOrder(..) | O::SmartOrder(..) => tr("Change sort").into(),
        O::FolderIcon(..) | O::SmartIcon(..) => tr("Change icon").into(),
        O::ReorderFolder(..) => tr("Move folder").into(),
        O::MoveSmart(..) => tr("Move smart folder").into(),
        O::FolderPassword(_, Some(_)) => tr("Set password").into(),
        O::FolderPassword(_, None) => tr("Remove password").into(),
        O::CreateSmart { .. } => tr("New smart folder").into(),
        O::CreateGroup(_) => tr("New tag group").into(),
        O::RenameGroup(..) => tr("Rename tag group").into(),
        O::GroupColor(..) => tr("Recolour tag group").into(),
        O::DeleteGroup(_) => tr("Delete tag group").into(),
        O::AddToGroup(_, tags) => tr_n(tags.len(), "Group {n} tag", "Group {n} tags"),
        O::AddToGroupToo(_, tags) => tr_n(tags.len(), "Group {n} tag", "Group {n} tags"),
        O::RemoveFromGroup(_, tags) => tr_n(tags.len(), "Ungroup {n} tag", "Ungroup {n} tags"),
        O::SaveFilter(..) => tr("Save filter").into(),
        O::RenameFilter(..) => tr("Rename saved filter").into(),
        O::DeleteFilter(_) => tr("Remove saved filter").into(),
        O::MoveFilter(..) => tr("Move saved filter").into(),
        O::MoveManual(_, ids, _) => trf("Reorder {n}", &[("n", &ids.len())]),
        O::CreateSmartGroup(_) => tr("New smart folder group").into(),
        O::RenameSmart(..) => tr("Rename smart folder").into(),
        O::SortFolders(..) => tr("Sort folders by title").into(),
        O::MoveGroup(..) => tr("Move tag group").into(),
        O::GroupDescription(..) => tr("Describe tag group").into(),
        O::FolderSettings(..) => tr("Change folder settings").into(),
    }
}

/// Names a frame's undo step from its actions; a frame without actions
/// only had inspector edits.
pub(crate) fn undo_label(labels: &[String]) -> String {
    match labels {
        [] => tr("Edit item").into(),
        [one] => one.clone(),
        [first, rest @ ..] => trf("{first} and {n} more", &[("first", first), ("n", &rest.len())]),
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
            let sound = super::sounds::action_sound(&action);
            let result = match action {
                Action::CreateFolder { parent } => lib.create_folder(parent.as_deref(), tr("Untitled")).and_then(|id| {
                    if let Some(p) = parent {
                        // Eagle puts a new subfolder first among its siblings.
                        lib.reorder_folder(&id, Some(&p), 0)?;
                        self.ws.collapsed.remove(&p);
                    }
                    self.ws.renaming = Some((id, tr("Untitled").into()));
                    Ok(())
                }),
                Action::CreateSibling(of) => match crate::organize::locate(&lib.metadata.folders, &of).map(|(p, at)| (p.map(str::to_string), at)) {
                    Some((parent, at)) => lib.create_folder(parent.as_deref(), tr("Untitled")).and_then(|id| {
                        lib.reorder_folder(&id, parent.as_deref(), at + 1)?;
                        if let Some(p) = &parent {
                            self.ws.collapsed.remove(p);
                        }
                        self.ws.renaming = Some((id, tr("Untitled").into()));
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
                    self.flash_undo(trf("Removed {n} from {name}", &[("n", &n), ("name", &name)]));
                    r
                }
                Action::RenameFolder(id, name) => lib.rename_folder(&id, &name),
                Action::SetExtension(id, ext) => {
                    let r = lib.set_extension(&id, &ext);
                    if r.is_ok() {
                        self.ws.loader.forget(&id);
                        self.flash(trf("Extension changed to .{ext}", &[("ext", &ext)]));
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
                    self.flash_folder(trf("Added {n} to", &[("n", &n)]), &name, &fid, true);
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
                Action::PasteTags(ids, tags) => lib.paste_tags(&ids, &tags).map(|n| self.flash_undo(tr_nf(n, "Pasted {tags} to {n} item", "Pasted {tags} to {n} items", &[("tags", &tr_n(tags.len(), "{n} tag", "{n} tags"))]))),
                Action::SetNotes(ids, note) => lib.set_notes(&ids, &note).map(|_| ()),
                Action::SetUrls(ids, url) => lib.set_urls(&ids, &url).map(|_| ()),
                Action::ClearTags(ids) => lib.clear_tags(&ids).map(|n| self.flash_undo(tr_n(n, "Cleared the tags of {n} item", "Cleared the tags of {n} items"))),
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
                    self.flash_undo(tr_nf(n, "Renamed “{from}” to “{to}” on {n} item", "Renamed “{from}” to “{to}” on {n} items", &[("from", &from), ("to", &to)]));
                }),
                Action::DeleteTags(tags) => lib.delete_tags(&tags).map(|n| {
                    self.ws.org.tag_sel.retain(|t| !tags.contains(t));
                    let what = if let [one] = tags.as_slice() { format!("“{one}”") } else { tr_n(tags.len(), "{n} tag", "{n} tags") };
                    self.flash_undo(tr_nf(n, "Removed {what} from {n} item", "Removed {what} from {n} items", &[("what", &what)]));
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
                    self.flash_undo(tr_nf(n, "Merged {tags} into “{into}” on {n} item", "Merged {tags} into “{into}” on {n} items", &[("tags", &tr_n(tags.len(), "{n} tag", "{n} tags")), ("into", &into)]));
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
                Action::ClearTagHistory => lib.clear_tag_history().map(|_| self.flash(tr("Cleared the tag history"))),
                Action::DeletePermanently(ids) => {
                    self.select_after_removal(&ids);
                    let r = lib.delete_permanently(&ids);
                    self.flash(trf("Deleted {n}", &[("n", &ids.len())]));
                    r
                }
                // Runs behind the progress modal; permanent, so the undo
                // history is cleared when it ends.
                Action::EmptyTrash => {
                    self.start_empty_trash(lib);
                    Ok(())
                }
                Action::Trash(ids, trash) => {
                    // The Duplicates page keeps its own selection rules.
                    if self.ws.scope == Scope::Duplicates {
                        self.ws.selected.retain(|s| !ids.contains(s));
                    } else {
                        self.select_after_removal(&ids);
                    }
                    let r = lib.set_trashed(&ids, trash);
                    if trash {
                        self.flash_undo(trf("Moved {n} to Trash", &[("n", &ids.len())]));
                    } else {
                        self.flash(trf("Restored {n}", &[("n", &ids.len())]));
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
                            self.flash(trf("Moved {n} to Trash", &[("n", &ids.len())]));
                            if r.is_ok() {
                                self.play_sound(crate::sounds::Sound::DeleteItem);
                            }
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
                        (0, t) => trf("Moved {t} to Trash", &[("t", &t)]),
                        (n, 0) => trf("Removed {n} from the folder", &[("n", &n)]),
                        (n, t) => trf("Removed {n} from the folder and moved {t} to Trash", &[("n", &n), ("t", &t)]),
                    });
                    r
                }
                Action::MergeDuplicates(merges) => lib.merge_duplicates(&merges).map(|report| {
                    self.ws.selected.retain(|id| lib.item(id).is_some_and(|i| !i.is_deleted));
                    self.flash(tr_nf(report.kept, "Merged {n} group and moved {trashed} to the Trash", "Merged {n} groups and moved {trashed} to the Trash", &[("trashed", &report.trashed)]));
                    self.ws.duplicates.merged(report);
                }),
                Action::Organize(a) => self.apply_organize(lib, a),
                Action::ResolveDuplicates(files, choice) => {
                    let n = files.len();
                    let mut r = Ok(());
                    let mut resolved = Vec::new();
                    for p in files {
                        r = r.and(crate::import_dupes::resolve(lib, p, choice).map(|id| resolved.extend(id)));
                    }
                    self.page_shots_for_links_in(lib, &resolved);
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
                Action::NewDocument(template, folders) => self.new_document(lib, template, folders),
                Action::RunSteps(run) => self.apply_run(lib, run),
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
            match result {
                Ok(()) => {
                    if let Some(s) = sound {
                        self.play_sound(s);
                    }
                }
                Err(e) => self.flash_error(trf("Could not save: {e}", &[("e", &e)])),
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
            self.flash_error(trf("Could not save: {e}", &[("e", &e)]));
        }
        if let Some(undo) = self.ws.undo_request.take() {
            let (from, to) = if undo { (&mut self.ws.undo, &mut self.ws.redo) } else { (&mut self.ws.redo, &mut self.ws.undo) };
            match from.pop() {
                None => self.flash(if undo { tr("Nothing to undo") } else { tr("Nothing to redo") }),
                Some(tx) => {
                    let label = tx.label.clone();
                    match lib.revert(tx) {
                        Ok(inverse) => {
                            push_capped(to, inverse);
                            // Reload the inspector's text from the restored item.
                            self.ws.edit = None;
                            self.ws.multi_edit = None;
                            self.ws.folder_edit = None;
                            self.flash(if undo { trf("Undid: {label}", &[("label", &label)]) } else { trf("Redid: {label}", &[("label", &label)]) });
                        }
                        Err(e) => self.flash_error(trf("Could not save: {e}", &[("e", &e)])),
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
                Err(e) => self.flash_error(trf("Could not save: {e}", &[("e", &e)])),
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
