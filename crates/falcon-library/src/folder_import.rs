//! Importing a folder from disk with its structure kept, after Eagle's
//! `uploadFolderToSidebar`: the folder becomes an Eagle folder of the same
//! name, each subfolder a child folder, and every file is imported into
//! the folder that mirrors its parent directory.
//!
//! [`walk`] reads the tree, [`plan`] turns it into folders to create and
//! files to import, and [`Library::create_folders`] adds the folders in one
//! `metadata.json` write. Importing the files is left to the caller, which
//! stages them on another thread.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::Folder;
use crate::search::natural_cmp;

/// A directory as walked from disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    /// The directory itself.
    pub dir: PathBuf,
    /// Files directly inside it, in name order.
    pub files: Vec<PathBuf>,
    /// Its subdirectories, in name order.
    pub folders: Vec<Tree>,
}

/// macOS packages Eagle treats as single files rather than folders to walk
/// into (Keynote, Pixelmator and MindNode documents).
const PACKAGES: [&str; 3] = ["key", "pxd", "mindnode"];

/// Junk files that operating systems leave in folders; never imported.
/// Names starting with a dot are skipped as hidden files as well.
const JUNK: [&str; 4] = ["Thumbs.db", "ehthumbs.db", "Desktop.ini", "Icon\r"];

/// Whether a walk leaves `name` out: hidden files and OS junk.
pub fn is_skipped(name: &str) -> bool {
    name.starts_with('.') || JUNK.iter().any(|j| j.eq_ignore_ascii_case(name))
}

fn is_package(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| PACKAGES.iter().any(|p| p.eq_ignore_ascii_case(e)))
}

fn by_name(a: &Path, b: &Path) -> Ordering {
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    natural_cmp(&name(a), &name(b)).then_with(|| a.cmp(b))
}

/// Reads `dir` and everything under it. Hidden files and junk are left
/// out; packages count as files. Symbolic links to folders are not
/// followed, so a link cycle can't make the walk endless. Unreadable
/// subfolders come back empty.
pub fn walk(dir: &Path) -> Tree {
    let mut tree = Tree { dir: dir.to_path_buf(), ..Default::default() };
    let Ok(entries) = fs::read_dir(dir) else { return tree };
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if is_skipped(&name.to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() && !is_package(&path) {
            subdirs.push(path);
        } else if ft.is_symlink() && path.is_dir() {
            continue;
        } else {
            tree.files.push(path);
        }
    }
    tree.files.sort_by(|a, b| by_name(a, b));
    subdirs.sort_by(|a, b| by_name(a, b));
    tree.folders = subdirs.iter().map(|d| walk(d)).collect();
    tree
}

impl Tree {
    /// How deep the tree goes and how many files it holds.
    pub fn stats(&self) -> Stats {
        let mut out = Stats { depth: 0, files: 0 };
        let mut stack = vec![(self, 1)];
        while let Some((node, depth)) = stack.pop() {
            out.depth = out.depth.max(depth);
            out.files += node.files.len();
            stack.extend(node.folders.iter().map(|c| (c, depth + 1)));
        }
        out
    }

    /// Every file in the tree, folder by folder.
    pub fn all_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            out.extend(node.files.iter().cloned());
            stack.extend(node.folders.iter().rev());
        }
        out
    }
}

/// The size of a walked tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Levels of folders, counting the walked folder itself as 1.
    pub depth: usize,
    /// Files at every level.
    pub files: usize,
}

/// From this many files Eagle asks before importing a folder.
pub const WARN_FILES: usize = 2_000;
/// Eagle refuses a folder with this many files or more...
pub const MAX_FILES: usize = 100_000;
/// ...or this many levels of folders or more.
pub const MAX_DEPTH: usize = 15;

/// Whether a folder may be imported, after Eagle's limits in
/// `uploadFolderToSidebar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Import it.
    Import,
    /// Ask first: it holds this many files.
    Confirm(usize),
    /// Too deep or too many files to import.
    Refuse,
}

/// Eagle's limits: 15 levels or more, or 100,000 files or more, are
/// refused; 2,000 files or more need a confirmation.
pub fn verdict(stats: Stats) -> Verdict {
    if stats.depth >= MAX_DEPTH || stats.files >= MAX_FILES {
        Verdict::Refuse
    } else if stats.files >= WARN_FILES {
        Verdict::Confirm(stats.files)
    } else {
        Verdict::Import
    }
}

/// Pixave libraries, which folder import skips as Eagle's Import Folders
/// does.
pub fn is_pixave_library(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pxvlibrary"))
}

/// A folder to create for an imported tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFolder {
    /// The new folder's id.
    pub id: String,
    /// Its name: the directory's own name.
    pub name: String,
    /// The folder it goes in; `None` for the top level.
    pub parent: Option<String>,
}

/// What importing a [`Tree`] does to a library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Folders to create, each after its parent, siblings in name order.
    pub folders: Vec<NewFolder>,
    /// Every file with the id of the folder it is imported into.
    pub files: Vec<(PathBuf, String)>,
}

impl Plan {
    /// The id of the folder made for the walked directory itself.
    pub fn root(&self) -> Option<&str> {
        self.folders.first().map(|f| f.id.as_str())
    }
}

/// Maps a walked tree to the folders to create and the file imports, with
/// the top folder placed in `parent` (or at the top level). `new_id` gives
/// each folder its id.
pub fn plan(tree: &Tree, parent: Option<&str>, mut new_id: impl FnMut() -> String) -> Plan {
    let mut out = Plan { folders: Vec::new(), files: Vec::new() };
    let mut stack = vec![(tree, parent.map(String::from))];
    // Breadth-first keeps parents before children and siblings together.
    let mut i = 0;
    while i < stack.len() {
        let (node, parent) = (stack[i].0, stack[i].1.clone());
        let id = new_id();
        let name = node.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| node.dir.to_string_lossy().into_owned());
        out.folders.push(NewFolder { id: id.clone(), name, parent });
        out.files.extend(node.files.iter().map(|f| (f.clone(), id.clone())));
        stack.extend(node.folders.iter().map(|c| (c, Some(id.clone()))));
        i += 1;
    }
    out
}

impl Library {
    /// Creates the folders of a [`Plan`] and writes `metadata.json` once.
    /// Each parent must exist already or come earlier in `folders`. As in
    /// Eagle, the new folders take no icon from their parent. Nothing is
    /// changed when a parent is missing.
    pub fn create_folders(&mut self, folders: &[NewFolder]) -> Result<()> {
        let mut known: std::collections::HashSet<String> = self.folder_ids().into_iter().map(String::from).collect();
        if let Some(bad) = folders.iter().find(|f| {
            let ok = f.parent.as_ref().is_none_or(|p| known.contains(p));
            known.insert(f.id.clone());
            !ok
        }) {
            let parent = bad.parent.clone().unwrap_or_default();
            return Err(Error::Import { path: self.root().to_path_buf(), reason: format!("no folder {parent}") });
        }
        let now = crate::edit::now_ms();
        for f in folders {
            let folder = Folder::new(f.id.clone(), f.name.clone(), now);
            match &f.parent {
                None => self.metadata.folders.push(folder),
                Some(p) => {
                    if let Some(parent) = find_mut(&mut self.metadata.folders, p) {
                        parent.children.push(folder);
                    }
                }
            }
        }
        self.save_metadata()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    /// `Photos/` with `a.png`, `b.png`, `.hidden`, `Thumbs.db`, a package
    /// `deck.key/`, and subfolders `Trip 10/x.jpg`, `Trip 2/y.jpg`,
    /// `Trip 2/Day 1/z.jpg` and an empty `Empty/`.
    fn sample(root: &Path) -> PathBuf {
        let top = root.join("Photos");
        for f in ["b.png", "a.png", ".hidden", "Thumbs.db", "deck.key/Index.zip", "Trip 10/x.jpg", "Trip 2/y.jpg", "Trip 2/Day 1/z.jpg"] {
            touch(&top.join(f));
        }
        fs::create_dir_all(top.join("Empty")).unwrap();
        top
    }

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn walk_keeps_structure_and_skips_junk() {
        let tmp = tempfile::tempdir().unwrap();
        let tree = walk(&sample(tmp.path()));
        assert_eq!(names(&tree.files), ["a.png", "b.png", "deck.key"], "hidden files and junk are skipped; packages are files");
        let subs: Vec<String> = tree.folders.iter().map(|t| t.dir.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(subs, ["Empty", "Trip 2", "Trip 10"], "numbers sort by value");
        assert_eq!(names(&tree.folders[1].files), ["y.jpg"]);
        assert_eq!(names(&tree.folders[1].folders[0].files), ["z.jpg"]);
    }

    #[cfg(unix)]
    #[test]
    fn walk_does_not_follow_folder_links() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().join("Loop");
        touch(&top.join("a.png"));
        std::os::unix::fs::symlink(&top, top.join("again")).unwrap();
        let tree = walk(&top);
        assert_eq!(names(&tree.files), ["a.png"]);
        assert!(tree.folders.is_empty());
    }

    #[test]
    fn stats_count_levels_and_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tree = walk(&sample(tmp.path()));
        assert_eq!(tree.stats(), Stats { depth: 3, files: 6 }, "Photos / Trip 2 / Day 1; junk and hidden files don't count");
        assert_eq!(names(&tree.all_files()), ["a.png", "b.png", "deck.key", "y.jpg", "z.jpg", "x.jpg"]);
        assert_eq!(walk(&tmp.path().join("missing")).stats(), Stats { depth: 1, files: 0 });
    }

    #[test]
    fn limits_follow_eagle() {
        let v = |depth, files| verdict(Stats { depth, files });
        assert_eq!(v(14, 1_999), Verdict::Import);
        assert_eq!(v(1, 2_000), Verdict::Confirm(2_000));
        assert_eq!(v(14, 99_999), Verdict::Confirm(99_999));
        assert_eq!(v(15, 1), Verdict::Refuse);
        assert_eq!(v(1, 100_000), Verdict::Refuse);
    }

    #[test]
    fn deep_trees_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut dir = tmp.path().join("L1");
        for i in 2..=15 {
            dir = dir.join(format!("L{i}"));
        }
        touch(&dir.join("deep.png"));
        let tree = walk(&tmp.path().join("L1"));
        assert_eq!(tree.stats(), Stats { depth: 15, files: 1 });
        assert_eq!(verdict(tree.stats()), Verdict::Refuse);
        assert_eq!(verdict(walk(&tmp.path().join("L1/L2")).stats()), Verdict::Import, "14 levels are fine");
    }

    #[test]
    fn pixave_libraries_are_recognised() {
        assert!(is_pixave_library(Path::new("/x/Mine.pxvlibrary")));
        assert!(is_pixave_library(Path::new("/x/Mine.PXVLibrary")));
        assert!(!is_pixave_library(Path::new("/x/Mine.library")));
    }

    #[test]
    fn plan_maps_folders_and_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tree = walk(&sample(tmp.path()));
        let mut n = 0;
        let plan = plan(&tree, Some("PARENT"), || {
            n += 1;
            format!("F{n}")
        });
        let folders: Vec<(&str, &str, Option<&str>)> = plan.folders.iter().map(|f| (f.id.as_str(), f.name.as_str(), f.parent.as_deref())).collect();
        assert_eq!(
            folders,
            [
                ("F1", "Photos", Some("PARENT")),
                ("F2", "Empty", Some("F1")),
                ("F3", "Trip 2", Some("F1")),
                ("F4", "Trip 10", Some("F1")),
                ("F5", "Day 1", Some("F3")),
            ]
        );
        assert_eq!(plan.root(), Some("F1"));
        let files: Vec<(String, &str)> = plan.files.iter().map(|(p, id)| (p.file_name().unwrap().to_string_lossy().into_owned(), id.as_str())).collect();
        let expect = [("a.png", "F1"), ("b.png", "F1"), ("deck.key", "F1"), ("y.jpg", "F3"), ("x.jpg", "F4"), ("z.jpg", "F5")];
        assert_eq!(files, expect.map(|(a, b)| (a.to_string(), b)));
    }

    #[test]
    fn create_folders_builds_the_tree_in_one_write() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("T.library");
        let mut lib = Library::create(&root).unwrap();
        let parent = lib.create_folder(None, "Inbox").unwrap();
        let tree = walk(&sample(tmp.path()));
        let plan = plan(&tree, Some(&parent), crate::ids::new_id);
        lib.create_folders(&plan.folders).unwrap();
        let lib = Library::open(&root).unwrap();
        let inbox = &lib.metadata.folders[0];
        assert_eq!(inbox.children.len(), 1);
        let photos = &inbox.children[0];
        assert_eq!(photos.name, "Photos");
        let kids: Vec<&str> = photos.children.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(kids, ["Empty", "Trip 2", "Trip 10"]);
        assert_eq!(photos.children[1].children[0].name, "Day 1");

        // A plan at the top level; a missing parent changes nothing.
        let mut lib = Library::open(&root).unwrap();
        let top = super::plan(&walk(&tmp.path().join("Photos/Trip 2")), None, crate::ids::new_id);
        lib.create_folders(&top.folders).unwrap();
        assert_eq!(lib.metadata.folders.last().unwrap().name, "Trip 2");
        let before = lib.metadata.folders.len();
        let bad = super::plan(&walk(&tmp.path().join("Photos")), Some("NOPE"), crate::ids::new_id);
        assert!(lib.create_folders(&bad.folders).is_err());
        assert_eq!(Library::open(&root).unwrap().metadata.folders.len(), before);
    }
}
