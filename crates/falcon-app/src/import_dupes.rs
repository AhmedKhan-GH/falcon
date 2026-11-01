//! Files found to be already in the library while importing.
//!
//! The import thread stages each file without the library lock, looks up
//! items of the same size and extension under a brief lock, and compares
//! the bytes with the lock released again (`import_check`). A duplicate is
//! not committed: it waits, staged, in a [`Queue`] and the thread moves on
//! to the next file. Once the batch has finished, the window takes the
//! queue into a [`Dialog`] and asks, one file at a time or for all at once,
//! whether to use the existing item, keep both or skip the file. The
//! answer is carried out by [`resolve`] on the UI thread, inside the
//! frame's undo step. Nothing here waits on the UI while holding the lock.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use falcon_library::import::{ImportOptions, Staged};
use falcon_library::{Library, Result, import_check};

/// A staged file with the same bytes as an existing item.
pub struct Pending {
    pub staged: Staged,
    /// The id of the item already in the library.
    pub existing: String,
}

/// Duplicates waiting for the window, filled by the import thread.
pub type Queue = Arc<Mutex<Vec<Pending>>>;

/// What to do with a duplicate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    /// Don't import; put the existing item in the import's folders.
    UseExisting,
    /// Import the file as a new item anyway.
    KeepBoth,
    /// Don't import, and change nothing.
    Skip,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Imports one file on the import thread. With a `queue`, a file whose
/// bytes match an item already in the library is held there instead of
/// being committed, and `Ok(None)` is returned. The library is locked
/// only to list candidates and to commit, never while files are read.
pub fn import_one(path: &Path, opts: &ImportOptions, lib: &Mutex<Library>, queue: Option<&Queue>) -> Result<Option<String>> {
    let staged = falcon_library::import::stage(path, opts)?;
    if let Some(q) = queue {
        let candidates = lock(lib).duplicate_candidates(staged.item().size, &staged.item().ext);
        if let Some(existing) = import_check::same_file(&staged.original(), &candidates) {
            lock(q).push(Pending { staged, existing });
            return Ok(None);
        }
    }
    lock(lib).commit_import(staged).map(Some)
}

/// The duplicate dialog's state: the files still to decide, the choice
/// shown (Use existing first, as in Eagle) and the apply-to-all box.
pub struct Dialog {
    pub pending: VecDeque<Pending>,
    pub choice: Choice,
    pub apply_all: bool,
}

impl Dialog {
    pub fn new(pending: Vec<Pending>) -> Dialog {
        Dialog { pending: pending.into(), choice: Choice::UseExisting, apply_all: false }
    }

    /// Adds files found by a later batch.
    pub fn extend(&mut self, more: Vec<Pending>) {
        self.pending.extend(more);
    }

    /// Takes the files an answer covers: the first one, or every one when
    /// apply-to-all is ticked (it is only offered with more than one).
    pub fn take(&mut self, all: bool) -> Vec<Pending> {
        let n = if all { self.pending.len() } else { self.pending.len().min(1) };
        self.pending.drain(..n).collect()
    }
}

/// Carries out `choice` for one held file and returns the id of the item
/// that now stands for it, if any. When the existing item has since gone
/// (deleted or trashed), Use existing imports the file instead.
pub fn resolve(lib: &mut Library, p: Pending, choice: Choice) -> Result<Option<String>> {
    let alive = lib.item(&p.existing).is_some_and(|i| !i.is_deleted);
    match choice {
        Choice::UseExisting if alive => {
            let folders = p.staged.item().folders.clone();
            lib.use_existing_for_import(&p.existing, &folders).map(|()| Some(p.existing))
        }
        Choice::UseExisting | Choice::KeepBoth => lib.commit_import(p.staged).map(Some),
        // Dropping the staged file removes its temporary folder.
        Choice::Skip => Ok(None),
    }
}

/// The notice after answering: "Used the existing item for 2 files".
pub fn notice(choice: Choice, n: usize) -> String {
    let files = if n == 1 { "1 file".to_string() } else { format!("{n} files") };
    match choice {
        Choice::UseExisting => format!("Used the existing items for {files}"),
        Choice::KeepBoth => format!("Imported {files} as new items"),
        Choice::Skip => format!("Skipped {files}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Library, std::path::PathBuf, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("Dup.library")).unwrap();
        let folder = lib.create_folder(None, "F").unwrap();
        let src = tmp.path().join("a.png");
        image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3])).save(&src).unwrap();
        let id = lib.import_file(&src, &ImportOptions { palette: false, ..Default::default() }).unwrap();
        (tmp, lib, src, folder, id)
    }

    fn opts(folder: &str) -> ImportOptions {
        ImportOptions { palette: false, folders: vec![folder.to_string()], folder_auto_tags: true, ..Default::default() }
    }

    #[test]
    fn duplicates_are_held_only_when_asked() {
        let (tmp, lib, src, folder, id) = setup();
        let shared = Mutex::new(lib);
        let queue: Queue = Default::default();
        assert_eq!(import_one(&src, &opts(&folder), &shared, Some(&queue)).unwrap(), None);
        assert_eq!(lock(&queue).len(), 1);
        assert_eq!(lock(&queue)[0].existing, id);
        assert_eq!(lock(&shared).items().len(), 1, "nothing committed");
        // A different file goes straight in.
        let other = tmp.path().join("b.png");
        image::RgbImage::from_pixel(9, 9, image::Rgb([9, 9, 9])).save(&other).unwrap();
        assert!(import_one(&other, &opts(&folder), &shared, Some(&queue)).unwrap().is_some());
        // Without a queue (the preference off) duplicates import silently.
        assert!(import_one(&src, &opts(&folder), &shared, None).unwrap().is_some());
        assert_eq!(lock(&shared).items().len(), 3);
        assert_eq!(lock(&queue).len(), 1);
    }

    #[test]
    fn taking_one_or_all() {
        let (_tmp, lib, src, folder, _) = setup();
        let shared = Mutex::new(lib);
        let queue: Queue = Default::default();
        for _ in 0..3 {
            import_one(&src, &opts(&folder), &shared, Some(&queue)).unwrap();
        }
        let mut d = Dialog::new(std::mem::take(&mut *lock(&queue)));
        assert_eq!((d.choice, d.apply_all), (Choice::UseExisting, false));
        assert_eq!(d.take(false).len(), 1);
        assert_eq!(d.pending.len(), 2);
        assert_eq!(d.take(true).len(), 2);
        assert!(d.take(false).is_empty() && d.take(true).is_empty());
    }

    #[test]
    fn each_choice() {
        let (_tmp, lib, src, folder, id) = setup();
        let shared = Mutex::new(lib);
        let queue: Queue = Default::default();
        for _ in 0..2 {
            import_one(&src, &opts(&folder), &shared, Some(&queue)).unwrap();
        }
        let mut lib = shared.into_inner().unwrap();
        let mut held = std::mem::take(&mut *lock(&queue)).into_iter();

        assert_eq!(resolve(&mut lib, held.next().unwrap(), Choice::UseExisting).unwrap(), Some(id.clone()));
        assert_eq!(lib.items().len(), 1);
        assert_eq!(lib.item(&id).unwrap().folders, std::slice::from_ref(&folder));

        let new = resolve(&mut lib, held.next().unwrap(), Choice::KeepBoth).unwrap().unwrap();
        assert_ne!(new, id);
        assert_eq!(lib.items().len(), 2);

        // Use existing when the existing item is in the Trash imports.
        let queue: Queue = Default::default();
        let shared = Mutex::new(lib);
        import_one(&src, &opts(&folder), &shared, Some(&queue)).unwrap();
        let mut lib = shared.into_inner().unwrap();
        let p = lock(&queue).pop().unwrap();
        lib.set_trashed(std::slice::from_ref(&p.existing), true).unwrap();
        let got = resolve(&mut lib, p, Choice::UseExisting).unwrap().unwrap();
        assert!(lib.item(&got).is_some_and(|i| !i.is_deleted));

        // Skip changes nothing and removes the staged copy.
        let shared = Mutex::new(lib);
        import_one(&src, &opts(&folder), &shared, Some(&queue)).unwrap();
        let mut lib = shared.into_inner().unwrap();
        let p = lock(&queue).pop().unwrap();
        let dir = p.staged.original().parent().unwrap().to_path_buf();
        let before = lib.items().len();
        assert!(dir.exists());
        assert_eq!(resolve(&mut lib, p, Choice::Skip).unwrap(), None);
        assert!(!dir.exists());
        assert_eq!(lib.items().len(), before);
    }

    #[test]
    fn notices() {
        assert_eq!(notice(Choice::UseExisting, 1), "Used the existing items for 1 file");
        assert_eq!(notice(Choice::KeepBoth, 2), "Imported 2 files as new items");
        assert_eq!(notice(Choice::Skip, 3), "Skipped 3 files");
    }
}
