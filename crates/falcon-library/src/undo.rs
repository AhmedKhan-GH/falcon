//! Undo transactions and batched writes.
//!
//! Every library write goes through a few functions (`register_item`,
//! `forget_item`, `save_metadata`, `save_tags`, `save_filters`). While a
//! transaction is open they record what they replace: the first pre-edit
//! copy of each item, the ids of items that did not exist before, and the
//! metadata, tags and saved filters as last saved. [`Library::revert`] puts
//! that state back and returns the inverse transaction, for redo.
//!
//! In [`WriteMode::Batched`] the same functions only mark what is dirty, and
//! [`Library::flush`] writes each dirty file once. The bytes are the same as
//! [`WriteMode::Immediate`] would have left: every writer serialises the
//! whole in-memory value, so the last write wins either way.

use std::collections::BTreeSet;

use crate::error::Result;
use crate::library::{Library, write_json};
use crate::model::{Item, LibraryMetadata, SavedFilter, Tags};

/// A group of edits that can be reverted together.
#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    /// What the edit was, for "Undid: …" messages.
    pub label: String,
    /// Pre-edit copies of changed items, the first one per id.
    items: Vec<Item>,
    /// Items registered during the transaction that did not exist before.
    created: Vec<String>,
    metadata: Option<LibraryMetadata>,
    tags: Option<Tags>,
    saved_filters: Option<Vec<SavedFilter>>,
    /// Set when items were deleted for good during the transaction: their
    /// files are gone, so the edit can't be undone and an undo history from
    /// before it no longer applies.
    pub irreversible: bool,
}

impl Transaction {
    fn new(label: String) -> Transaction {
        Transaction { label, items: Vec::new(), created: Vec::new(), metadata: None, tags: None, saved_filters: None, irreversible: false }
    }

    /// True when nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.created.is_empty() && self.metadata.is_none() && self.tags.is_none() && self.saved_filters.is_none()
    }

    /// Ids of items the transaction changed or created.
    pub fn item_ids(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|i| i.id.as_str()).chain(self.created.iter().map(String::as_str))
    }
}

/// When library writes reach the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WriteMode {
    /// Each edit writes its files at once (the default; the CLI and server
    /// use it).
    #[default]
    Immediate,
    /// Edits mark files dirty and [`Library::flush`] writes them.
    Batched,
}

/// What a [`Library::flush`] wrote.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlushReport {
    /// Item `metadata.json` files written.
    pub items: usize,
    pub mtime: bool,
    pub metadata: bool,
    pub tags: bool,
    pub saved_filters: bool,
}

impl FlushReport {
    /// Whether `mtime.json` or `metadata.json` was written. Eagle backs
    /// the metadata up after it rewrites `mtime.json`, so callers that keep
    /// backups (the app) schedule one when this is true.
    pub fn wants_backup(&self) -> bool {
        self.mtime || self.metadata
    }
}

/// Files waiting for a flush in batched mode.
#[derive(Debug, Clone, Default)]
pub(crate) struct Dirty {
    pub(crate) items: BTreeSet<String>,
    pub(crate) mtime: bool,
    pub(crate) metadata: bool,
    pub(crate) tags: bool,
    pub(crate) saved_filters: bool,
}

impl Dirty {
    fn any(&self) -> bool {
        !self.items.is_empty() || self.mtime || self.metadata || self.tags || self.saved_filters
    }
}

/// The library-wide values as last saved.
#[derive(Debug, Clone)]
pub(crate) struct Committed {
    pub(crate) metadata: LibraryMetadata,
    pub(crate) tags: Tags,
    pub(crate) saved_filters: Vec<SavedFilter>,
}

impl Library {
    /// Starts recording edits. An already open transaction is ended and
    /// dropped first, so a transaction never spans more than one caller's
    /// unit of work.
    pub fn begin_transaction(&mut self, label: impl Into<String>) {
        self.journal = Some(Transaction::new(label.into()));
    }

    /// Renames the open transaction (the app names it once it knows what
    /// the frame did).
    pub fn set_transaction_label(&mut self, label: impl Into<String>) {
        if let Some(j) = &mut self.journal {
            j.label = label.into();
        }
    }

    /// Stops recording and returns what was recorded, or `None` when
    /// nothing changed.
    pub fn end_transaction(&mut self) -> Option<Transaction> {
        self.journal.take().filter(|t| !t.is_empty() || t.irreversible)
    }

    /// Puts back the state `tx` recorded and returns the inverse
    /// transaction (reverting that redoes `tx`). Items are saved through
    /// [`Library::update_item`], so renamed files get their old names back
    /// and `lastModified` moves forward, as with any edit (Eagle's own undo
    /// also saves the item anew). Items `tx` created go to the trash; their
    /// files stay. Metadata, tags and saved filters are restored and
    /// written (metadata gets a fresh `modificationTime`). Items deleted for
    /// good since are skipped. A transaction open around the call is set
    /// aside meanwhile, so the revert is not recorded in it.
    pub fn revert(&mut self, tx: Transaction) -> Result<Transaction> {
        let outer = self.journal.take();
        self.begin_transaction(tx.label.clone());
        let result = self.apply(&tx);
        let inverse = self.journal.take().unwrap_or_else(|| Transaction::new(tx.label.clone()));
        self.journal = outer;
        result.map(|()| inverse)
    }

    fn apply(&mut self, tx: &Transaction) -> Result<()> {
        if let Some(m) = &tx.metadata {
            self.metadata = m.clone();
            self.save_metadata()?;
        }
        if let Some(t) = &tx.tags {
            self.tags = t.clone();
            self.save_tags()?;
        }
        if let Some(f) = &tx.saved_filters {
            self.saved_filters = f.clone();
            self.save_filters()?;
        }
        for old in &tx.items {
            if self.item(&old.id).is_some() {
                self.update_item(old.clone())?;
            }
        }
        let created: Vec<String> = tx.created.iter().filter(|id| self.item(id).is_some_and(|i| !i.is_deleted)).cloned().collect();
        self.set_trashed(&created, true)
    }

    /// Records `id` in an open transaction: its current copy the first time
    /// it changes, or as created if it doesn't exist yet.
    pub(crate) fn journal_item(&mut self, id: &str) {
        let Some(j) = &self.journal else { return };
        if j.items.iter().any(|i| i.id == id) || j.created.iter().any(|c| c == id) {
            return;
        }
        let old = self.item(id).cloned();
        let j = self.journal.as_mut().expect("checked above");
        match old {
            Some(item) => j.items.push(item),
            None => j.created.push(id.to_string()),
        }
    }

    /// Notes that `metadata` is being saved: the copy saved before goes into
    /// an open transaction (once), and the current one becomes the saved one.
    pub(crate) fn journal_metadata(&mut self) {
        let prev = std::mem::replace(&mut self.committed.metadata, self.metadata.clone());
        if let Some(j) = &mut self.journal {
            j.metadata.get_or_insert(prev);
        }
    }

    pub(crate) fn journal_tags(&mut self) {
        let prev = std::mem::replace(&mut self.committed.tags, self.tags.clone());
        if let Some(j) = &mut self.journal {
            j.tags.get_or_insert(prev);
        }
    }

    pub(crate) fn journal_saved_filters(&mut self) {
        let prev = std::mem::replace(&mut self.committed.saved_filters, self.saved_filters.clone());
        if let Some(j) = &mut self.journal {
            j.saved_filters.get_or_insert(prev);
        }
    }

    /// In batched mode, marks something dirty and returns true (the caller
    /// skips its write); in immediate mode returns false.
    pub(crate) fn batching(&mut self, mark: impl FnOnce(&mut Dirty)) -> bool {
        match self.write_mode {
            WriteMode::Immediate => false,
            WriteMode::Batched => {
                mark(&mut self.dirty);
                true
            }
        }
    }

    /// The current write mode.
    pub fn write_mode(&self) -> WriteMode {
        self.write_mode
    }

    /// Switches between immediate and batched writes. Leaving batched mode
    /// flushes what is pending.
    pub fn set_write_mode(&mut self, mode: WriteMode) -> Result<()> {
        if mode == WriteMode::Immediate && self.write_mode == WriteMode::Batched {
            self.flush()?;
        }
        self.write_mode = mode;
        Ok(())
    }

    /// True when batched writes are waiting for [`Library::flush`].
    pub fn has_pending_writes(&self) -> bool {
        self.dirty.any()
    }

    /// Writes everything batched mode held back: each dirty item once, then
    /// `mtime.json`, `metadata.json`, `tags.json` and `saved-filters.json`
    /// once each. It takes no backup; [`FlushReport::wants_backup`] tells
    /// the caller when to (see [`Library::backup_metadata`]). On an error
    /// the pending set is kept, so the next flush tries again.
    pub fn flush(&mut self) -> Result<FlushReport> {
        let mut report = FlushReport::default();
        if !self.dirty.any() {
            return Ok(report);
        }
        let root = self.root().to_path_buf();
        for id in &self.dirty.items {
            // An item deleted since it was marked has nothing left to write.
            if let Some(item) = self.item(id) {
                write_json(&item.info_dir(&root).join("metadata.json"), item)?;
                report.items += 1;
            }
        }
        if self.dirty.mtime {
            write_json(&root.join("mtime.json"), &self.mtime)?;
            report.mtime = true;
        }
        if self.dirty.metadata {
            write_json(&root.join("metadata.json"), &self.metadata)?;
            report.metadata = true;
        }
        if self.dirty.tags {
            write_json(&root.join("tags.json"), &self.tags)?;
            report.tags = true;
        }
        if self.dirty.saved_filters {
            write_json(&root.join("saved-filters.json"), &self.saved_filters)?;
            report.saved_filters = true;
        }
        self.dirty = Dirty::default();
        Ok(report)
    }
}

/// A batched library flushes what is pending when dropped, so nothing is
/// lost when the app closes it. Errors can't be reported here; call
/// [`Library::flush`] first to see them. (A clone shares the pending set at
/// the time of cloning, so drop clones of a batched library before the
/// original makes further edits, or flush first.)
impl Drop for Library {
    fn drop(&mut self) {
        if self.write_mode == WriteMode::Batched && self.dirty.any() {
            let _ = self.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;
    use crate::smart;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A new library with two imported PNGs, `a` and `b`.
    fn setup(dir: &Path) -> (Library, String, String) {
        let mut lib = Library::create(dir.join("Test.library")).unwrap();
        let mut ids = Vec::new();
        for (name, rgb) in [("a", [200, 30, 30]), ("b", [30, 30, 200])] {
            let path = dir.join(format!("{name}.png"));
            image::RgbImage::from_pixel(8, 6, image::Rgb(rgb)).save(&path).unwrap();
            ids.push(lib.import_file(&path, &ImportOptions::default()).unwrap());
        }
        let b = ids.pop().unwrap();
        (lib, ids.pop().unwrap(), b)
    }

    /// Runs `edit` in a transaction that must record something.
    fn tx(lib: &mut Library, edit: impl FnOnce(&mut Library)) -> Transaction {
        lib.begin_transaction("edit");
        edit(lib);
        lib.end_transaction().expect("something changed")
    }

    fn item(lib: &Library, id: &str) -> Item {
        lib.item(id).unwrap().clone()
    }

    /// An item without `lastModified`, which every save bumps.
    fn content(lib: &Library, id: &str) -> Item {
        let mut i = item(lib, id);
        i.last_modified = 0;
        i
    }

    /// Every file under `root`, by relative path, ignoring metadata backups.
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.file_name().is_some_and(|n| n != "backup") {
                        stack.push(p);
                    }
                } else {
                    out.insert(p.strip_prefix(root).unwrap().to_path_buf(), fs::read(&p).unwrap());
                }
            }
        }
        out
    }

    #[test]
    fn rename_undo_and_redo_move_the_files() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, _) = setup(tmp.path());
        let before = content(&lib, &a);
        let t = tx(&mut lib, |lib| {
            let mut i = item(lib, &a);
            i.name = "renamed".into();
            lib.update_item(i).unwrap();
        });
        let root = lib.root().to_path_buf();
        assert!(root.join(format!("images/{a}.info/renamed.png")).is_file());
        let redo = lib.revert(t).unwrap();
        assert_eq!(content(&lib, &a), before);
        assert!(root.join(format!("images/{a}.info/a.png")).is_file());
        assert!(!root.join(format!("images/{a}.info/renamed_thumbnail.png")).exists());
        assert!(!root.join(format!("images/{a}.info/renamed.png")).exists());
        // Disk matches memory after an undo.
        let on_disk = Library::open(&root).unwrap();
        assert_eq!(on_disk.item(&a), lib.item(&a));
        let undo = lib.revert(redo).unwrap();
        assert_eq!(item(&lib, &a).name, "renamed");
        assert!(root.join(format!("images/{a}.info/renamed.png")).is_file());
        lib.revert(undo).unwrap();
        assert_eq!(item(&lib, &a).name, "a");
    }

    #[test]
    fn trash_and_restore_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, b) = setup(tmp.path());
        let before = (content(&lib, &a), content(&lib, &b));
        let t = tx(&mut lib, |lib| lib.set_trashed(&[a.clone(), b.clone()], true).unwrap());
        assert!(item(&lib, &a).is_deleted);
        let redo = lib.revert(t).unwrap();
        assert_eq!((content(&lib, &a), content(&lib, &b)), before);
        assert!(!item(&lib, &a).extra.contains_key("deletedTime"));
        lib.revert(redo).unwrap();
        assert!(item(&lib, &a).is_deleted && item(&lib, &b).is_deleted);
    }

    #[test]
    fn tag_edits_undo_items_and_tags_json() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, _) = setup(tmp.path());
        let tags_before = lib.tags.clone();
        let t = tx(&mut lib, |lib| {
            let mut i = item(lib, &a);
            i.tags.push("red".into());
            lib.update_item(i).unwrap();
            lib.record_tag_use(&["red".into()]).unwrap();
            lib.set_tag_starred("red", true).unwrap();
        });
        let redo = lib.revert(t).unwrap();
        assert!(item(&lib, &a).tags.is_empty());
        assert_eq!(lib.tags, tags_before);
        let on_disk = Library::open(lib.root()).unwrap();
        assert_eq!(on_disk.tags, tags_before);
        lib.revert(redo).unwrap();
        assert_eq!(item(&lib, &a).tags, ["red"]);
        assert_eq!(lib.tags.starred_tags, ["red"]);
    }

    #[test]
    fn folder_edits_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, b) = setup(tmp.path());
        let empty = lib.metadata.folders.clone();
        let mut f = String::new();
        let t = tx(&mut lib, |lib| f = lib.create_folder(None, "One").unwrap());
        lib.revert(t).unwrap();
        assert_eq!(lib.metadata.folders, empty);

        let f = lib.create_folder(None, "One").unwrap();
        let g = lib.create_folder(None, "Two").unwrap();
        let t = tx(&mut lib, |lib| lib.rename_folder(&f, "Uno").unwrap());
        lib.revert(t).unwrap();
        assert_eq!(lib.all_folders()[0].name, "One");

        let tree = lib.metadata.folders.clone();
        let t = tx(&mut lib, |lib| lib.move_folder(&g, Some(&f)).unwrap());
        assert_eq!(lib.metadata.folders.len(), 1);
        lib.revert(t).unwrap();
        assert_eq!(lib.metadata.folders, tree);

        // Items filed in a deleted folder come back to it, out of the trash.
        for (id, folders) in [(&a, vec![f.clone()]), (&b, vec![f.clone(), g.clone()])] {
            let mut i = item(&lib, id);
            i.folders = folders;
            lib.update_item(i).unwrap();
        }
        let before = (content(&lib, &a), content(&lib, &b));
        let t = tx(&mut lib, |lib| lib.delete_folder(&f).unwrap());
        assert!(item(&lib, &a).is_deleted);
        assert_eq!(item(&lib, &b).folders, std::slice::from_ref(&g));
        lib.revert(t).unwrap();
        assert_eq!((content(&lib, &a), content(&lib, &b)), before);
        assert_eq!(lib.metadata.folders, tree);
        let on_disk = Library::open(lib.root()).unwrap();
        assert_eq!(on_disk.metadata.folders, tree);
    }

    /// The folder inspector's name and description edits change only those
    /// two keys of the folder in `metadata.json` (plus the file's own
    /// `modificationTime`, which every save stamps), and one undo step
    /// restores them.
    #[test]
    fn folder_name_and_description_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, _, _) = setup(tmp.path());
        let f = lib.create_folder(None, "One").unwrap();
        let meta = lib.root().join("metadata.json");
        // The file without its top-level save time.
        let read = || {
            let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&meta).unwrap()).unwrap();
            v.as_object_mut().unwrap().shift_remove("modificationTime").expect("save time");
            v.to_string()
        };
        let before = read();
        assert_eq!(before.matches(r#""name":"One","description":"""#).count(), 1, "{before}");
        let t = tx(&mut lib, |lib| {
            lib.rename_folder(&f, "Uno").unwrap();
            lib.set_folder_description(&f, "Line one\nline \"two\"").unwrap();
        });
        let after = read();
        assert_eq!(after, before.replace(r#""name":"One","description":"""#, r#""name":"Uno","description":"Line one\nline \"two\"""#));
        let reopened = Library::open(lib.root()).unwrap();
        let folder = &reopened.all_folders()[0];
        assert_eq!((folder.name.as_str(), folder.description.as_str()), ("Uno", "Line one\nline \"two\""));
        let redo = lib.revert(t).unwrap();
        assert_eq!(read(), before);
        lib.revert(redo).unwrap();
        assert_eq!(read(), after);
        assert!(lib.set_folder_description("missing", "x").is_err());
    }

    #[test]
    fn tag_group_edits_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, _, _) = setup(tmp.path());
        let t = tx(&mut lib, |lib| {
            lib.create_tag_group("Colours", &["red".into()], None).unwrap();
        });
        lib.revert(t).unwrap();
        assert!(lib.metadata.tags_groups.is_empty());
        let id = lib.create_tag_group("Colours", &["red".into()], None).unwrap();
        let before = lib.metadata.tags_groups.clone();
        let t = tx(&mut lib, |lib| {
            lib.rename_tag_group(&id, "Hues").unwrap();
            lib.add_tags_to_group(&id, &["blue".into()]).unwrap();
        });
        let redo = lib.revert(t).unwrap();
        assert_eq!(lib.metadata.tags_groups, before);
        lib.revert(redo).unwrap();
        assert_eq!(lib.metadata.tags_groups[0].name, "Hues");
        assert_eq!(lib.metadata.tags_groups[0].tags, ["red", "blue"]);
    }

    #[test]
    fn smart_folder_and_saved_filter_edits_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, _, _) = setup(tmp.path());
        let rule = serde_json::json!({"conditions": [{"match": "AND", "rules": [{"property": "name", "method": "contain", "value": "a"}]}]});
        let conditions = rule["conditions"].as_array().unwrap().clone();
        let t = tx(&mut lib, |lib| {
            lib.create_smart_folder("Smart", conditions.clone()).unwrap();
        });
        lib.revert(t).unwrap();
        assert!(lib.smart_folders().is_empty());
        let id = lib.create_smart_folder("Smart", conditions.clone()).unwrap();
        let t = tx(&mut lib, |lib| lib.update_smart_folder(&id, "Clever", Vec::new()).unwrap());
        lib.revert(t).unwrap();
        let sf: Vec<smart::SmartFolder> = lib.smart_folders();
        assert_eq!(sf[0].name, "Smart");
        let t = tx(&mut lib, |lib| lib.delete_smart_folder(&id).unwrap());
        lib.revert(t).unwrap();
        assert_eq!(lib.smart_folders().len(), 1);

        let t = tx(&mut lib, |lib| {
            lib.create_saved_filter("Saved", rule.clone()).unwrap();
        });
        lib.revert(t).unwrap();
        assert!(lib.saved_filters.is_empty());
        assert_eq!(fs::read(lib.root().join("saved-filters.json")).unwrap(), b"[]");
    }

    #[test]
    fn imports_undo_to_the_trash() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, _, _) = setup(tmp.path());
        let path = tmp.path().join("c.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 200, 0])).save(&path).unwrap();
        let mut c = String::new();
        let t = tx(&mut lib, |lib| c = lib.import_file(&path, &ImportOptions::default()).unwrap());
        assert_eq!(t.item_ids().collect::<Vec<_>>(), [c.as_str()]);
        let redo = lib.revert(t).unwrap();
        assert!(item(&lib, &c).is_deleted);
        assert!(item(&lib, &c).original_path(lib.root()).is_file());
        lib.revert(redo).unwrap();
        assert!(!item(&lib, &c).is_deleted);
    }

    #[test]
    fn permanent_deletes_are_irreversible() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, _) = setup(tmp.path());
        lib.begin_transaction("Delete");
        lib.delete_permanently(&[a]).unwrap();
        assert!(lib.end_transaction().unwrap().irreversible);
        lib.begin_transaction("Nothing");
        assert_eq!(lib.end_transaction(), None);
    }

    #[test]
    fn revert_inside_an_open_transaction_is_not_recorded_in_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, _) = setup(tmp.path());
        let t = tx(&mut lib, |lib| lib.set_trashed(std::slice::from_ref(&a), true).unwrap());
        lib.begin_transaction("frame");
        lib.revert(t).unwrap();
        assert_eq!(lib.end_transaction(), None);
    }

    /// The same edits, for the batching tests.
    fn edits(lib: &mut Library, a: &str, b: &str) {
        let f = lib.create_folder(None, "F").unwrap();
        lib.rename_folder(&f, "G").unwrap();
        for id in [a, b] {
            let mut i = item(lib, id);
            i.tags.push("x".into());
            i.folders.push(f.clone());
            lib.update_item(i).unwrap();
        }
        let mut i = item(lib, a);
        i.name = "first".into();
        lib.update_item(i).unwrap();
        lib.record_tag_use(&["x".into()]).unwrap();
        lib.create_tag_group("G", &["x".into()], None).unwrap();
        lib.create_saved_filter("S", serde_json::json!({})).unwrap();
        lib.set_trashed(&[b.to_string()], true).unwrap();
    }

    /// Normalises the clock-dependent values so two runs compare equal.
    fn normalise(lib: &mut Library) {
        lib.metadata.modification_time = 0;
        for i in lib.items_mut() {
            i.last_modified = 0;
            i.extra.remove("deletedTime");
        }
        for e in &mut lib.mtime.entries {
            e.1 = 0;
        }
    }

    #[test]
    fn batched_flush_writes_what_immediate_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let (one, two) = (tmp.path().join("one"), tmp.path().join("two"));
        fs::create_dir_all(&one).unwrap();
        fs::create_dir_all(&two).unwrap();
        let (mut imm, a, b) = setup(&one);
        let (mut bat, a2, b2) = setup(&two);
        edits(&mut imm, &a, &b);

        bat.set_write_mode(WriteMode::Batched).unwrap();
        let before = snapshot(bat.root());
        edits(&mut bat, &a2, &b2);
        assert!(bat.has_pending_writes());
        // Only the rename touched the disk: the files moved, no JSON changed.
        let mid = snapshot(bat.root());
        let json = |s: &BTreeMap<PathBuf, Vec<u8>>| s.iter().filter(|(p, _)| p.extension().is_some_and(|e| e == "json")).map(|(p, v)| (p.clone(), v.clone())).collect::<BTreeMap<_, _>>();
        assert_eq!(json(&mid), json(&before));
        let report = bat.flush().unwrap();
        assert_eq!(report.items, 2);
        assert!(report.mtime && report.metadata && report.tags && report.saved_filters);
        assert!(report.wants_backup());
        // Flushing never backs up by itself; the app schedules that.
        assert!(bat.backups().unwrap().is_empty());
        assert!(!bat.has_pending_writes());
        assert_eq!(bat.flush().unwrap(), FlushReport::default());

        // Each file now holds what the in-memory model serialises to, the
        // same as after immediate writes.
        for lib in [&imm, &bat] {
            let root = lib.root();
            assert_eq!(fs::read(root.join("metadata.json")).unwrap(), serde_json::to_vec(&lib.metadata).unwrap());
            assert_eq!(fs::read(root.join("mtime.json")).unwrap(), serde_json::to_vec(&lib.mtime).unwrap());
            assert_eq!(fs::read(root.join("tags.json")).unwrap(), serde_json::to_vec(&lib.tags).unwrap());
            assert_eq!(fs::read(root.join("saved-filters.json")).unwrap(), serde_json::to_vec(&lib.saved_filters).unwrap());
            for i in lib.items() {
                assert_eq!(fs::read(i.info_dir(root).join("metadata.json")).unwrap(), serde_json::to_vec(i).unwrap());
            }
        }
        // And the two libraries agree once ids and clocks are set aside.
        let (mut x, mut y) = (Library::open(imm.root()).unwrap(), Library::open(bat.root()).unwrap());
        normalise(&mut x);
        normalise(&mut y);
        let strip = |l: &Library| {
            let mut m = l.metadata.clone();
            m.folders.iter_mut().for_each(|f| {
                f.id.clear();
                f.modification_time = 0;
            });
            m.tags_groups.iter_mut().for_each(|g| g.id.clear());
            m
        };
        assert_eq!(strip(&x), strip(&y));
        assert_eq!(x.tags, y.tags);
        assert_eq!(x.saved_filters, y.saved_filters);
        let names = |l: &Library| l.items().iter().map(|i| (i.name.clone(), i.tags.clone(), i.is_deleted)).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names(&x), names(&y));
    }

    #[test]
    fn dropping_a_batched_library_flushes() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, _) = setup(tmp.path());
        let root = lib.root().to_path_buf();
        lib.set_write_mode(WriteMode::Batched).unwrap();
        let mut i = item(&lib, &a);
        i.tags.push("kept".into());
        lib.update_item(i).unwrap();
        assert!(Library::open(&root).unwrap().item(&a).unwrap().tags.is_empty());
        drop(lib);
        assert_eq!(Library::open(&root).unwrap().item(&a).unwrap().tags, ["kept"]);
    }

    #[test]
    fn leaving_batched_mode_flushes_and_deleted_items_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, a, b) = setup(tmp.path());
        lib.set_write_mode(WriteMode::Batched).unwrap();
        lib.set_trashed(&[a.clone(), b.clone()], true).unwrap();
        lib.delete_permanently(std::slice::from_ref(&a)).unwrap();
        lib.set_write_mode(WriteMode::Immediate).unwrap();
        let on_disk = Library::open(lib.root()).unwrap();
        assert!(on_disk.item(&a).is_none());
        assert!(on_disk.item(&b).unwrap().is_deleted);
        assert_eq!(on_disk.mtime, lib.mtime);
    }
}
