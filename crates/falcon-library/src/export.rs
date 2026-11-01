//! Export to Computer: copying originals out of the library into a folder
//! the user picked.
//!
//! A [`Plan`] is worked out first (where each original goes, which
//! directories to make), so the app can warn about files it would replace
//! before anything is written, and run the copy on another thread. Names
//! follow Eagle's export: an item keeps `<name>.<ext>`; names that clash
//! within one directory (ignoring case, as the Mac's file system does) are
//! numbered `<name>-1.<ext>`, `<name>-2.<ext>`, with the number padded to
//! the width of the largest one.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use filetime::FileTime;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::Item;

/// What to do when a file of the same name is already in the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collision {
    /// Overwrite it.
    Replace,
    /// Leave it and give the exported file a numbered name.
    KeepBoth,
}

/// One original to copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Everything an export will write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Directories to create, parents first.
    pub dirs: Vec<PathBuf>,
    pub steps: Vec<Step>,
    /// Where the export lands: the chosen folder, or the top of the
    /// exported folder tree.
    pub top: PathBuf,
    /// Whether steps may overwrite files (see [`Collision`]).
    pub overwrite: bool,
}

impl Plan {
    /// Steps whose destination already exists, which a `Replace` plan
    /// overwrites.
    pub fn clashes(&self) -> usize {
        self.steps.iter().filter(|s| s.to.exists()).count()
    }
}

/// An item's file name, `<name>.<ext>`, with any `/` (which no file name
/// can hold) turned into a space.
pub fn file_name(item: &Item) -> String {
    let name = item.name.replace(['/', '\0'], " ");
    if item.ext.is_empty() { name } else { format!("{name}.{}", item.ext) }
}

/// Splits a file name at its last dot (none for a leading dot).
fn stem_ext(file: &str) -> (&str, Option<&str>) {
    match file.rfind('.') {
        Some(i) if i > 0 => (&file[..i], Some(&file[i + 1..])),
        _ => (file, None),
    }
}

/// Chooses the names for `files` copied into one directory that already
/// holds `existing` (lower-cased). Names are compared ignoring case. A name
/// used once, and free, is kept. Otherwise every file of that name is
/// numbered, as Eagle does: `a.png` twice becomes `a-1.png` and `a-2.png`;
/// with `KeepBoth`, an `a.png` already there counts as the first, so a new
/// one becomes `a-2.png`. A numbered name that is taken is skipped.
pub fn name_files(files: &[String], existing: &HashSet<String>, how: Collision) -> Vec<String> {
    let keep = how == Collision::KeepBoth;
    let mut count: HashMap<String, usize> = HashMap::new();
    for f in files {
        *count.entry(f.to_lowercase()).or_default() += 1;
    }
    let mut taken: HashSet<String> = if keep { existing.clone() } else { HashSet::new() };
    // Names that stay as they are, reserved before any numbering so that
    // `a-1.png` in the batch is not handed to a second `a.png`.
    let plain: HashSet<String> = count.iter().filter(|(k, n)| **n == 1 && !taken.contains(*k)).map(|(k, _)| k.clone()).collect();
    taken.extend(plain.iter().cloned());
    let mut next: HashMap<String, usize> = HashMap::new();
    files
        .iter()
        .map(|f| {
            let lower = f.to_lowercase();
            if plain.contains(&lower) {
                return f.clone();
            }
            let had = usize::from(keep && existing.contains(&lower));
            let width = (count[&lower] + had).to_string().len();
            let n = next.entry(lower).or_insert(had);
            let (stem, ext) = stem_ext(f);
            loop {
                *n += 1;
                let candidate = match ext {
                    Some(e) => format!("{stem}-{:0width$}.{e}", *n),
                    None => format!("{stem}-{:0width$}", *n),
                };
                if taken.insert(candidate.to_lowercase()) {
                    return candidate;
                }
            }
        })
        .collect()
}

/// Lower-cased names of the entries in `dir` (none if it doesn't exist).
fn listing(dir: &Path) -> HashSet<String> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default()
}

/// Plans copying the originals of `ids` (in order, each once) straight
/// into `dest`. Unknown ids are skipped.
pub fn plan_items(lib: &Library, ids: &[String], dest: &Path, how: Collision) -> Plan {
    let mut seen = HashSet::new();
    let items: Vec<&Item> = ids.iter().filter(|id| seen.insert(id.as_str())).filter_map(|id| lib.item(id)).collect();
    let mut plan = Plan { dirs: vec![dest.to_path_buf()], steps: Vec::new(), top: dest.to_path_buf(), overwrite: how == Collision::Replace };
    add_steps(&mut plan, lib.root(), &items, dest, &listing(dest), how);
    plan
}

fn add_steps(plan: &mut Plan, root: &Path, items: &[&Item], dir: &Path, existing: &HashSet<String>, how: Collision) {
    let names = name_files(&items.iter().map(|i| file_name(i)).collect::<Vec<_>>(), existing, how);
    for (item, name) in items.iter().zip(names) {
        plan.steps.push(Step { id: item.id.clone(), from: item.original_path(root), to: dir.join(name) });
    }
}

/// Copies one original, keeping its modification time, as Eagle does. The
/// copy is written beside its destination and renamed into place, so a
/// failed copy never leaves half a file under the real name.
pub fn copy_file(from: &Path, to: &Path, overwrite: bool) -> Result<()> {
    let dir = to.parent().unwrap_or(Path::new("."));
    let tmp = tempfile::Builder::new().prefix(".falcon-export").tempfile_in(dir).map_err(|e| Error::io(dir, e))?;
    fs::copy(from, tmp.path()).map_err(|e| Error::io(from, e))?;
    if let Ok(meta) = fs::metadata(from) {
        let _ = filetime::set_file_mtime(tmp.path(), FileTime::from_last_modification_time(&meta));
    }
    if overwrite {
        tmp.persist(to).map_err(|e| Error::io(to, e.error))?;
    } else {
        tmp.persist_noclobber(to).map_err(|e| Error::io(to, e.error))?;
    }
    Ok(())
}

/// Runs `plan`, calling `on_step` after each copy with whether it worked,
/// and returns the paths written. A step that fails is skipped; the rest
/// still run.
pub fn run(plan: &Plan, mut on_step: impl FnMut(&Step, bool)) -> Result<Vec<PathBuf>> {
    for d in &plan.dirs {
        fs::create_dir_all(d).map_err(|e| Error::io(d, e))?;
    }
    let mut written = Vec::new();
    for step in &plan.steps {
        let ok = copy_file(&step.from, &step.to, plan.overwrite).is_ok();
        if ok {
            written.push(step.to.clone());
        }
        on_step(step, ok);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_lowercase()).collect()
    }

    fn names(files: &[&str], existing: &[&str], how: Collision) -> Vec<String> {
        name_files(&files.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &set(existing), how)
    }

    #[test]
    fn unique_names_stay() {
        assert_eq!(names(&["a.png", "b.jpg", "noext"], &["c.png"], Collision::KeepBoth), ["a.png", "b.jpg", "noext"]);
    }

    #[test]
    fn clashes_in_the_batch_are_numbered_ignoring_case() {
        assert_eq!(names(&["a.png", "A.PNG", "b.png"], &[], Collision::KeepBoth), ["a-1.png", "A-2.PNG", "b.png"]);
        assert_eq!(names(&["x", "x"], &[], Collision::Replace), ["x-1", "x-2"]);
        // Padded to the widest number.
        let ten = names(&["a.png"; 10], &[], Collision::Replace);
        assert_eq!(ten[0], "a-01.png");
        assert_eq!(ten[9], "a-10.png");
    }

    #[test]
    fn existing_files_are_kept_or_replaced() {
        assert_eq!(names(&["a.png", "b.png"], &["A.png"], Collision::KeepBoth), ["a-2.png", "b.png"]);
        assert_eq!(names(&["a.png", "b.png"], &["a.png"], Collision::Replace), ["a.png", "b.png"]);
        // Taken numbered names are skipped.
        assert_eq!(names(&["a.png"], &["a.png", "a-2.png"], Collision::KeepBoth), ["a-3.png"]);
        // A batch file already called a-1.png keeps that name.
        assert_eq!(names(&["a.png", "a.png", "a-1.png"], &[], Collision::KeepBoth), ["a-2.png", "a-3.png", "a-1.png"]);
    }

    fn png(path: &Path, shade: u8) {
        image::RgbImage::from_fn(8, 8, |_, _| image::Rgb([shade, 0, 0])).save(path).unwrap();
    }

    /// A library with three items, two of them named `pic`.
    fn library(root: &Path) -> (Library, Vec<String>) {
        let mut lib = Library::create(root.join("A.library")).unwrap();
        let mut ids = Vec::new();
        for (i, name) in ["pic", "other", "pic"].iter().enumerate() {
            let dir = root.join(format!("src{i}"));
            fs::create_dir_all(&dir).unwrap();
            let src = dir.join(format!("{name}.png"));
            png(&src, 40 * i as u8);
            filetime::set_file_mtime(&src, FileTime::from_unix_time(1_600_000_000 + i as i64, 0)).unwrap();
            ids.push(lib.import_file(&src, &ImportOptions { palette: false, ..Default::default() }).unwrap());
        }
        (lib, ids)
    }

    #[test]
    fn items_are_copied_with_their_times() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, ids) = library(dir.path());
        let out = dir.path().join("out");
        let plan = plan_items(&lib, &ids, &out, Collision::KeepBoth);
        let to: Vec<_> = plan.steps.iter().map(|s| s.to.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(to, ["pic-1.png", "other.png", "pic-2.png"]);
        assert_eq!(plan.clashes(), 0);
        let mut seen = 0;
        let written = run(&plan, |_, ok| seen += usize::from(ok)).unwrap();
        assert_eq!((written.len(), seen), (3, 3));
        for (step, id) in plan.steps.iter().zip(&ids) {
            let item = lib.item(id).unwrap();
            assert_eq!(fs::read(&step.to).unwrap(), fs::read(item.original_path(lib.root())).unwrap());
            let a = FileTime::from_last_modification_time(&fs::metadata(&step.to).unwrap());
            let b = FileTime::from_last_modification_time(&fs::metadata(item.original_path(lib.root())).unwrap());
            assert_eq!(a.unix_seconds(), b.unix_seconds());
        }
        // No temp files are left behind.
        assert_eq!(fs::read_dir(&out).unwrap().count(), 3);
    }

    #[test]
    fn a_second_export_keeps_both_or_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, ids) = library(dir.path());
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("other.png"), b"mine").unwrap();

        let replace = plan_items(&lib, &ids[1..2], &out, Collision::Replace);
        assert_eq!(replace.clashes(), 1);
        let keep = plan_items(&lib, &ids[1..2], &out, Collision::KeepBoth);
        assert_eq!(keep.steps[0].to, out.join("other-2.png"));
        run(&keep, |_, _| {}).unwrap();
        assert_eq!(fs::read(out.join("other.png")).unwrap(), b"mine");
        run(&replace, |_, _| {}).unwrap();
        assert_ne!(fs::read(out.join("other.png")).unwrap(), b"mine");
        assert_eq!(fs::read(out.join("other.png")).unwrap(), fs::read(out.join("other-2.png")).unwrap());
    }

    #[test]
    fn failed_steps_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, ids) = library(dir.path());
        let out = dir.path().join("out");
        let mut plan = plan_items(&lib, &ids, &out, Collision::KeepBoth);
        plan.steps[0].from = dir.path().join("missing.png");
        let mut failed = 0;
        let written = run(&plan, |_, ok| failed += usize::from(!ok)).unwrap();
        assert_eq!((written.len(), failed), (2, 1));
        assert_eq!(fs::read_dir(&out).unwrap().count(), 2);
    }
}
