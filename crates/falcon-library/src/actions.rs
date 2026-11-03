//! Actions: named, ordered lists of organising steps with an optional
//! shortcut, run on the selected items. AI steps get their results from
//! the app, which asks the model; here they only become tags or notes.
//!
//! In Eagle 4.0.0 actions are a plugin (`ai-action`) that keeps its list
//! in `actions.config.json` at the library root. That file belongs to the
//! plugin: falcon never reads, creates, changes or deletes it. Falcon's
//! own actions are its implementation of the feature and live in falcon's
//! app data, one file per library (see [`store_path`]); the app picks the
//! folder (`<app data>/actions/`).
//!
//! The file is a JSON array of objects, each with `id`, `name`,
//! `shortcut` (an accelerator such as `"Command + Shift + 1"`, empty for
//! none) and `steps`, one object per step tagged by `type`. Falcon keeps
//! each entry as the JSON it read, so fields it doesn't know and key
//! order survive a save byte for byte. It is written only after an edit,
//! atomically (temp file and rename), compact, or indented with two
//! spaces when the file it replaces was.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::library::Library;

/// The key naming a library's actions file: Eagle's `metadata.json`
/// carries no library id, so it is the 64-bit FNV-1a hash of the
/// library's canonical path (symlinks resolved, no trailing slash) as 16
/// lowercase hex digits. A path that can't be resolved (the library is
/// gone) is hashed as given. Moving or renaming a library therefore
/// starts it with no actions.
pub fn library_key(root: &Path) -> String {
    let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let text = canonical.to_string_lossy();
    let text = text.trim_end_matches(['/', '\\']);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The file holding the actions of the library at `root`, inside `dir`
/// (falcon's `<app data>/actions/`): `<dir>/<library_key>.json`.
pub fn store_path(dir: &Path, root: &Path) -> PathBuf {
    dir.join(format!("{}.json", library_key(root)))
}

/// One organising step. The `type` strings follow the rule names Eagle's
/// translations still carry from its earlier built-in Actions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Step {
    AddTags { tags: Vec<String> },
    RemoveTags { tags: Vec<String> },
    /// Takes every tag off.
    #[serde(rename = "clearTag")]
    ClearTags,
    /// Folder ids.
    AddFolders { folders: Vec<String> },
    RemoveFolders { folders: Vec<String> },
    /// Takes the items out of every folder.
    #[serde(rename = "clearFolder")]
    ClearFolders,
    /// 0 (none) to 5 stars.
    Rating { rating: u8 },
    /// Tags suggested by the default model. Falcon's own: the app asks the
    /// model and hands the result to [`Library::run_steps_with`].
    AutoTag,
    /// A description from the default model, written to the item's notes.
    /// An item that has notes keeps them unless `replace` is set.
    Describe {
        #[serde(default)]
        replace: bool,
    },
}

impl Step {
    /// Every kind, with empty parameters, in the order the editor offers them.
    pub fn kinds() -> [Step; 9] {
        [
            Step::AddTags { tags: Vec::new() },
            Step::RemoveTags { tags: Vec::new() },
            Step::ClearTags,
            Step::AddFolders { folders: Vec::new() },
            Step::RemoveFolders { folders: Vec::new() },
            Step::ClearFolders,
            Step::Rating { rating: 0 },
            Step::AutoTag,
            Step::Describe { replace: false },
        ]
    }

    /// Whether the step asks a model, so the run needs AI Models.
    pub fn is_ai(&self) -> bool {
        matches!(self, Step::AutoTag | Step::Describe { .. })
    }

    /// Whether two steps are of the same kind, parameters aside.
    pub fn same_kind(&self, other: &Step) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// An action as falcon edits it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ActionDef {
    pub id: String,
    pub name: String,
    /// The accelerator text, as Eagle stores it; `None` when unset.
    pub shortcut: Option<String>,
    pub steps: Vec<Step>,
    /// Falcon understood every step. An entry with fields or steps falcon
    /// doesn't know (written by a newer falcon, or by hand) lists and keeps
    /// its shortcut but cannot run or be edited here.
    pub runnable: bool,
}

/// A store file as read: each entry's JSON, untouched.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ActionsFile {
    entries: Vec<Map<String, Value>>,
    /// The file it was read from was indented.
    pretty: bool,
}

impl ActionsFile {
    /// Parses the file's text. Entries that are not objects are refused,
    /// as is anything but an array.
    pub fn parse(text: &str) -> serde_json::Result<ActionsFile> {
        let entries: Vec<Map<String, Value>> = serde_json::from_str(text)?;
        let pretty = text.trim_start().strip_prefix('[').is_some_and(|rest| rest.starts_with(['\n', '\r']));
        Ok(ActionsFile { entries, pretty })
    }

    /// The text falcon writes.
    pub fn to_text(&self) -> String {
        if self.pretty {
            serde_json::to_string_pretty(&self.entries).unwrap_or_else(|_| "[]".into())
        } else {
            serde_json::to_string(&self.entries).unwrap_or_else(|_| "[]".into())
        }
    }

    /// Reads a store file; a missing file is an empty list.
    pub fn load(path: &Path) -> Result<ActionsFile> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|e| Error::json(path, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ActionsFile::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Writes a store file atomically: a temp file beside it, then a
    /// rename, creating its folder if needed. On a failure the temp file
    /// is removed and the old file, if any, stays as it was.
    pub fn save(&self, path: &Path) -> Result<()> {
        let io = |e: std::io::Error| Error::io(path, e);
        let dir = path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir).map_err(io)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io)?;
        std::io::Write::write_all(&mut tmp, self.to_text().as_bytes()).map_err(io)?;
        tmp.persist(path).map_err(|e| io(e.error))?;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The actions, in file order.
    pub fn actions(&self) -> Vec<ActionDef> {
        self.entries.iter().map(read_entry).collect()
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.get("id").and_then(Value::as_str) == Some(id))
    }

    /// Adds an action at the end, or replaces the entry with its id. Only
    /// `id`, `name`, `shortcut` and `steps` are written; the entry's other
    /// fields and its key order stay.
    pub fn put(&mut self, def: &ActionDef) {
        let entry = match self.index(&def.id) {
            Some(i) => &mut self.entries[i],
            None => {
                self.entries.push(Map::new());
                self.entries.last_mut().expect("just pushed")
            }
        };
        entry.insert("id".into(), def.id.clone().into());
        entry.insert("name".into(), def.name.clone().into());
        entry.insert("shortcut".into(), def.shortcut.clone().unwrap_or_default().into());
        if def.runnable {
            entry.insert("steps".into(), serde_json::to_value(&def.steps).unwrap_or_default());
        }
    }

    /// Changes only an action's shortcut (also for an entry falcon can't run).
    pub fn set_shortcut(&mut self, id: &str, shortcut: Option<String>) {
        if let Some(i) = self.index(id) {
            self.entries[i].insert("shortcut".into(), shortcut.unwrap_or_default().into());
        }
    }

    /// Removes an action.
    pub fn remove(&mut self, id: &str) {
        if let Some(i) = self.index(id) {
            self.entries.remove(i);
        }
    }

    /// A copy of the action right after it, with a new id and `name`.
    /// Returns the new id.
    pub fn duplicate(&mut self, id: &str, name: &str) -> Option<String> {
        let i = self.index(id)?;
        let mut copy = self.entries[i].clone();
        let new_id = crate::ids::new_id();
        copy.insert("id".into(), new_id.clone().into());
        copy.insert("name".into(), name.into());
        // A shortcut belongs to one action.
        copy.insert("shortcut".into(), "".into());
        self.entries.insert(i + 1, copy);
        Some(new_id)
    }

    /// Moves an action to `to` (an index in the list after its removal).
    pub fn reorder(&mut self, id: &str, to: usize) {
        if let Some(i) = self.index(id) {
            let e = self.entries.remove(i);
            self.entries.insert(to.min(self.entries.len()), e);
        }
    }
}

fn read_entry(e: &Map<String, Value>) -> ActionDef {
    let text = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let shortcut = Some(text("shortcut")).filter(|s| !s.trim().is_empty());
    let steps: Option<Vec<Step>> = match e.get("steps") {
        None => Some(Vec::new()),
        Some(v) => serde_json::from_value(v.clone()).ok(),
    };
    // An entry with fields only another program reads runs elsewhere.
    let foreign = e.keys().any(|k| !matches!(k.as_str(), "id" | "name" | "shortcut" | "steps"));
    ActionDef { id: text("id"), name: text("name"), shortcut, runnable: steps.is_some() && !foreign, steps: steps.unwrap_or_default() }
}

/// What a model produced for one item in one AI step. It can only become
/// the item's tags or notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutput {
    Tags(Vec<String>),
    Description(String),
}

/// What running an action did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunReport {
    /// Items at least one step changed.
    pub changed: usize,
    /// Folder ids named by steps that no longer exist (skipped).
    pub missing_folders: Vec<String>,
}

impl Library {
    /// Runs `steps` in order on the items `ids`: each step on every item
    /// before the next step. Items in the Trash are left alone. A step
    /// that changes nothing is no error; a folder that no longer exists
    /// is skipped and reported; a write error stops the run there. AI steps
    /// do nothing here; [`Library::run_steps_with`] applies their results.
    pub fn run_steps(&mut self, ids: &[String], steps: &[Step]) -> Result<RunReport> {
        let ids: Vec<String> = ids.iter().filter(|id| self.item(id).is_some_and(|i| !i.is_deleted)).cloned().collect();
        let mut report = RunReport::default();
        let mut touched = std::collections::HashSet::new();
        let known: std::collections::HashSet<String> = self.folder_ids().into_iter().map(str::to_string).collect();
        for step in steps {
            match step {
                Step::AddTags { tags } => {
                    for id in &ids {
                        if self.item(id).is_some_and(|i| tags.iter().filter_map(|t| crate::tagging::clean(t)).any(|t| !i.tags.contains(&t))) {
                            touched.insert(id.clone());
                        }
                    }
                    self.add_tags_to_items(&ids, tags)?;
                }
                Step::RemoveTags { tags } => {
                    for id in &ids {
                        if self.item(id).is_some_and(|i| i.tags.iter().any(|t| tags.contains(t))) {
                            touched.insert(id.clone());
                        }
                    }
                    self.remove_tags_from_items(&ids, tags)?;
                }
                Step::ClearTags => {
                    for id in &ids {
                        if self.item(id).is_some_and(|i| !i.tags.is_empty()) {
                            touched.insert(id.clone());
                        }
                    }
                    self.clear_tags(&ids)?;
                }
                Step::AddFolders { folders } => {
                    for fid in folders {
                        if !known.contains(fid) {
                            if !report.missing_folders.contains(fid) {
                                report.missing_folders.push(fid.clone());
                            }
                            continue;
                        }
                        for id in &ids {
                            let Some(mut item) = self.item(id).cloned() else { continue };
                            if crate::autotag::join_folder(&mut item, &self.metadata.folders, fid) {
                                self.update_item(item)?;
                                touched.insert(id.clone());
                            }
                        }
                    }
                }
                Step::RemoveFolders { folders } => {
                    for id in &ids {
                        let Some(mut item) = self.item(id).cloned() else { continue };
                        let before = item.folders.len();
                        item.folders.retain(|f| !folders.contains(f));
                        if item.folders.len() != before {
                            self.update_item(item)?;
                            touched.insert(id.clone());
                        }
                    }
                }
                Step::ClearFolders => {
                    for id in &ids {
                        let Some(mut item) = self.item(id).cloned() else { continue };
                        if !item.folders.is_empty() {
                            item.folders.clear();
                            self.update_item(item)?;
                            touched.insert(id.clone());
                        }
                    }
                }
                Step::Rating { rating } => {
                    let rating = (*rating).min(5);
                    for id in &ids {
                        let Some(mut item) = self.item(id).cloned() else { continue };
                        let now = item.extra.get("star").and_then(Value::as_u64).unwrap_or(0);
                        if now == u64::from(rating) {
                            continue;
                        }
                        if rating == 0 {
                            item.extra.remove("star");
                        } else {
                            item.extra.insert("star".into(), rating.into());
                        }
                        self.update_item(item)?;
                        touched.insert(id.clone());
                    }
                }
                Step::AutoTag | Step::Describe { .. } => {}
            }
        }
        report.changed = touched.len();
        Ok(report)
    }

    /// Runs `steps` in order on `ids` as [`Library::run_steps`] does, with
    /// each AI step taking its results from `outputs`, keyed by the step's
    /// index and the item's id. An item with no result for an AI step is
    /// left as it is by that step.
    pub fn run_steps_with(&mut self, ids: &[String], steps: &[Step], outputs: &HashMap<(usize, String), StepOutput>) -> Result<RunReport> {
        let state = |lib: &Library, id: &String| lib.item(id).filter(|i| !i.is_deleted).map(|i| (i.tags.clone(), i.folders.clone(), i.annotation.clone(), i.extra.get("star").cloned()));
        let before: Vec<_> = ids.iter().map(|id| state(self, id)).collect();
        let mut report = RunReport::default();
        for (n, step) in steps.iter().enumerate() {
            if !step.is_ai() {
                let done = self.run_steps(ids, std::slice::from_ref(step))?;
                for f in done.missing_folders {
                    if !report.missing_folders.contains(&f) {
                        report.missing_folders.push(f);
                    }
                }
                continue;
            }
            for id in ids {
                if !self.item(id).is_some_and(|i| !i.is_deleted) {
                    continue;
                }
                match outputs.get(&(n, id.clone())) {
                    Some(StepOutput::Tags(tags)) => {
                        self.add_tags_to_items(std::slice::from_ref(id), tags)?;
                    }
                    Some(StepOutput::Description(text)) => {
                        if let Some(mut item) = self.item(id).cloned()
                            && item.annotation != *text
                        {
                            item.annotation.clone_from(text);
                            self.update_item(item)?;
                        }
                    }
                    None => {}
                }
            }
        }
        report.changed = ids.iter().zip(&before).filter(|(id, was)| state(self, id) != **was).count();
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tagging::tests::{library, s};

    /// The shape documented in `docs/src/actions.md`.
    const SAMPLE: &str = r#"[{"id":"MA1B2C3D4E5F6","name":"File as reference","shortcut":"Command + Shift + 1","steps":[{"type":"addTags","tags":["reference","to sort"]},{"type":"removeTags","tags":["inbox"]},{"type":"addFolders","folders":["MFOLDER000001"]},{"type":"rating","rating":4}]},{"id":"MA1B2C3D4E5F7","name":"Reset","shortcut":"","steps":[{"type":"clearTag"},{"type":"clearFolder"},{"type":"removeFolders","folders":["MFOLDER000002"]},{"type":"rating","rating":0}]}]"#;

    #[test]
    fn sample_parses_and_writes_back_byte_for_byte() {
        let f = ActionsFile::parse(SAMPLE).unwrap();
        assert_eq!(f.to_text(), SAMPLE);
        let a = f.actions();
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|a| a.runnable));
        assert_eq!(a[0].shortcut.as_deref(), Some("Command + Shift + 1"));
        assert_eq!(a[1].shortcut, None, "an empty string is no shortcut");
        assert_eq!(a[0].steps[0], Step::AddTags { tags: s(&["reference", "to sort"]) });
        assert_eq!(a[0].steps[3], Step::Rating { rating: 4 });
        assert_eq!(a[1].steps[..2], [Step::ClearTags, Step::ClearFolders]);
    }

    #[test]
    fn put_round_trips_the_sample() {
        let f = ActionsFile::parse(SAMPLE).unwrap();
        let mut g = ActionsFile::default();
        for a in f.actions() {
            g.put(&a);
        }
        assert_eq!(g.to_text(), SAMPLE);
    }

    #[test]
    fn foreign_entries_survive_edits_untouched() {
        let text = r#"[{"name":"Describe","id":"X1","prompt":"Write a note","model":{"n":1.5},"shortcut":"Command + 7"},{"id":"X2","name":"Mine","shortcut":"","steps":[{"type":"rating","rating":2}]}]"#;
        let mut f = ActionsFile::parse(text).unwrap();
        let a = f.actions();
        assert!(!a[0].runnable, "unknown fields mean another program runs it");
        assert_eq!(a[0].shortcut.as_deref(), Some("Command + 7"));
        assert!(a[1].runnable);
        let mut mine = a[1].clone();
        mine.name = "Mine too".into();
        f.put(&mine);
        let out = f.to_text();
        assert!(out.starts_with(r#"[{"name":"Describe","id":"X1","prompt":"Write a note","model":{"n":1.5},"shortcut":"Command + 7"},"#), "{out}");
        // Steps falcon can't read are no reason to drop the entry.
        let odd = r#"[{"id":"Y","name":"Odd","shortcut":"","steps":[{"type":"teleport"}]}]"#;
        let mut f = ActionsFile::parse(odd).unwrap();
        let y = f.actions().remove(0);
        assert!(!y.runnable);
        f.put(&y);
        assert_eq!(f.to_text(), odd, "an entry falcon can't run keeps its steps");
    }

    #[test]
    fn indented_files_stay_indented() {
        let text = "[\n  {\n    \"id\": \"A\",\n    \"name\": \"N\",\n    \"shortcut\": \"\",\n    \"steps\": []\n  }\n]";
        let f = ActionsFile::parse(text).unwrap();
        assert_eq!(f.to_text(), text);
        assert_eq!(ActionsFile::parse("[]").unwrap().to_text(), "[]");
    }

    #[test]
    fn edits_reorder_duplicate_remove() {
        let mut f = ActionsFile::parse(SAMPLE).unwrap();
        let id = f.duplicate("MA1B2C3D4E5F6", "Copy").unwrap();
        let a = f.actions();
        assert_eq!(a[1].id, id);
        assert_eq!((a[1].name.as_str(), a[1].shortcut.as_deref()), ("Copy", None));
        assert_eq!(a[1].steps, a[0].steps);
        f.reorder(&id, 0);
        assert_eq!(f.actions()[0].id, id);
        f.set_shortcut(&id, Some("Alt + 9".into()));
        assert_eq!(f.actions()[0].shortcut.as_deref(), Some("Alt + 9"));
        f.remove(&id);
        assert_eq!(f.to_text(), SAMPLE);
    }

    #[test]
    fn missing_file_is_empty_and_save_is_atomic() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("actions/k.json");
        assert!(ActionsFile::load(&path).unwrap().is_empty());
        let f = ActionsFile::parse(SAMPLE).unwrap();
        f.save(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), SAMPLE);
        assert_eq!(fs::read_dir(d.path().join("actions")).unwrap().count(), 1, "no temp file left");
        assert_eq!(ActionsFile::load(&path).unwrap(), f);
        fs::write(&path, "{oops").unwrap();
        assert!(ActionsFile::load(&path).is_err());
    }

    #[test]
    fn a_failed_save_leaves_no_partial_file() {
        let d = tempfile::tempdir().unwrap();
        // The rename fails: a folder sits where the file goes.
        let blocked = d.path().join("k.json");
        fs::create_dir(&blocked).unwrap();
        assert!(ActionsFile::parse(SAMPLE).unwrap().save(&blocked).is_err());
        let names: Vec<_> = fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["k.json"], "the temp file is gone");
        assert!(blocked.is_dir());
        // The temp file can't be made: the old file stays whole.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let ro = d.path().join("ro");
            fs::create_dir(&ro).unwrap();
            let path = ro.join("k.json");
            fs::write(&path, "[]").unwrap();
            fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
            let res = ActionsFile::parse(SAMPLE).unwrap().save(&path);
            fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(res.is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), "[]");
            assert_eq!(fs::read_dir(&ro).unwrap().count(), 1);
        }
    }

    #[test]
    fn keys_follow_the_canonical_library_path() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("A.library"), d.path().join("B.library"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        assert_ne!(library_key(&a), library_key(&b));
        assert_eq!(library_key(&a).len(), 16);
        assert_eq!(library_key(&a), library_key(&d.path().join("A.library/")), "a trailing slash is the same library");
        assert_eq!(library_key(&a), library_key(&d.path().join("B.library/../A.library")), "resolved before hashing");
        let dir = Path::new("/app/actions");
        assert_eq!(store_path(dir, &a), dir.join(format!("{}.json", library_key(&a))));
        assert!(!store_path(dir, &a).starts_with(&a), "never inside the library");
    }

    fn folder(lib: &mut Library, name: &str) -> String {
        lib.create_folder(None, name).unwrap()
    }

    fn star(lib: &Library, id: &str) -> u64 {
        lib.item(id).unwrap().extra.get("star").and_then(Value::as_u64).unwrap_or(0)
    }

    #[test]
    fn each_step_kind_on_a_multi_selection() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = library(d.path(), &[&["inbox", "x"], &["y"], &["inbox"]]);
        let (f1, f2) = (folder(&mut lib, "One"), folder(&mut lib, "Two"));
        let ids = s(&["A1", "A2"]);
        let r = lib.run_steps(&ids, &[Step::AddTags { tags: s(&["ref"]) }, Step::RemoveTags { tags: s(&["inbox"]) }]).unwrap();
        assert_eq!(r.changed, 2);
        assert_eq!(lib.item("A1").unwrap().tags, s(&["x", "ref"]));
        assert_eq!(lib.item("A2").unwrap().tags, s(&["y", "ref"]));
        assert_eq!(lib.item("A3").unwrap().tags, s(&["inbox"]), "only the selection");
        lib.run_steps(&ids, &[Step::AddFolders { folders: vec![f1.clone(), f2.clone()] }]).unwrap();
        assert_eq!(lib.item("A1").unwrap().folders, vec![f1.clone(), f2.clone()]);
        lib.run_steps(&ids, &[Step::RemoveFolders { folders: vec![f1.clone()] }]).unwrap();
        assert_eq!(lib.item("A2").unwrap().folders, vec![f2.clone()]);
        lib.run_steps(&ids, &[Step::Rating { rating: 3 }]).unwrap();
        assert_eq!((star(&lib, "A1"), star(&lib, "A2"), star(&lib, "A3")), (3, 3, 0));
        let r = lib.run_steps(&ids, &[Step::ClearTags, Step::ClearFolders, Step::Rating { rating: 0 }]).unwrap();
        assert_eq!(r.changed, 2);
        let a1 = lib.item("A1").unwrap();
        assert!(a1.tags.is_empty() && a1.folders.is_empty() && !a1.extra.contains_key("star"));
        // Written to disk.
        let reopened = Library::open(lib.root()).unwrap();
        assert!(reopened.item("A2").unwrap().tags.is_empty());
        assert_eq!(reopened.item("A3").unwrap().tags, s(&["inbox"]));
    }

    #[test]
    fn steps_run_in_order() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = library(d.path(), &[&["a"]]);
        lib.run_steps(&s(&["A1"]), &[Step::ClearTags, Step::AddTags { tags: s(&["b"]) }]).unwrap();
        assert_eq!(lib.item("A1").unwrap().tags, s(&["b"]));
        lib.run_steps(&s(&["A1"]), &[Step::AddTags { tags: s(&["c"]) }, Step::ClearTags]).unwrap();
        assert!(lib.item("A1").unwrap().tags.is_empty());
    }

    #[test]
    fn missing_folders_are_skipped_and_trash_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = library(d.path(), &[&[], &[]]);
        let f = folder(&mut lib, "Here");
        let mut gone = lib.item("A2").unwrap().clone();
        gone.is_deleted = true;
        lib.update_item(gone).unwrap();
        let r = lib.run_steps(&s(&["A1", "A2"]), &[Step::AddFolders { folders: vec!["NOPE".into(), f.clone()] }, Step::Rating { rating: 9 }]).unwrap();
        assert_eq!(r.missing_folders, s(&["NOPE"]));
        assert_eq!(r.changed, 1);
        assert_eq!(lib.item("A1").unwrap().folders, vec![f]);
        assert_eq!(star(&lib, "A1"), 5, "ratings cap at five");
        let a2 = lib.item("A2").unwrap();
        assert!(a2.is_deleted && a2.folders.is_empty(), "a trashed item is not restored");
    }

    #[test]
    fn ai_steps_round_trip_and_old_entries_stay_readable() {
        let text = r#"[{"id":"A","name":"Label","shortcut":"","steps":[{"type":"autoTag"},{"type":"describe","replace":true},{"type":"describe"}]}]"#;
        let f = ActionsFile::parse(text).unwrap();
        let a = f.actions().remove(0);
        assert!(a.runnable);
        assert_eq!(a.steps, [Step::AutoTag, Step::Describe { replace: true }, Step::Describe { replace: false }]);
        assert!(a.steps.iter().all(Step::is_ai));
        assert!(!Step::Rating { rating: 1 }.is_ai());
        let mut g = ActionsFile::default();
        g.put(&a);
        assert!(g.to_text().contains(r#"{"type":"autoTag"}"#));
    }

    #[test]
    fn outputs_apply_in_step_order_with_the_rule_steps() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = library(d.path(), &[&["old"], &["kept"], &[]]);
        let steps = [Step::ClearTags, Step::AutoTag, Step::Describe { replace: true }, Step::Rating { rating: 2 }];
        let outputs = HashMap::from([
            ((1, "A1".to_string()), StepOutput::Tags(s(&["new", " new ", ""]))),
            ((2, "A1".to_string()), StepOutput::Description("A cat.".into())),
            ((1, "A3".to_string()), StepOutput::Tags(s(&["other"]))),
        ]);
        let r = lib.run_steps_with(&s(&["A1", "A3"]), &steps, &outputs).unwrap();
        assert_eq!(r.changed, 2);
        let a1 = lib.item("A1").unwrap();
        assert_eq!(a1.tags, s(&["new"]), "added after the clear, once");
        assert_eq!(a1.annotation, "A cat.");
        assert_eq!(star(&lib, "A1"), 2);
        let a3 = lib.item("A3").unwrap();
        assert_eq!(a3.tags, s(&["other"]));
        assert_eq!(a3.annotation, "");
        assert_eq!(lib.item("A2").unwrap().tags, s(&["kept"]), "not in the run");
        // run_steps alone ignores the AI steps.
        let r = lib.run_steps(&s(&["A2"]), &[Step::AutoTag, Step::Describe { replace: true }]).unwrap();
        assert_eq!(r.changed, 0);
    }

    #[test]
    fn outputs_undo_as_one_step_and_skip_the_trash() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = library(d.path(), &[&["a"], &[]]);
        let mut gone = lib.item("A2").unwrap().clone();
        gone.is_deleted = true;
        lib.update_item(gone).unwrap();
        let outputs = HashMap::from([
            ((0, "A1".to_string()), StepOutput::Tags(s(&["n"]))),
            ((1, "A1".to_string()), StepOutput::Description("d".into())),
            ((0, "A2".to_string()), StepOutput::Tags(s(&["n"]))),
        ]);
        lib.begin_transaction("Run");
        let r = lib.run_steps_with(&s(&["A1", "A2"]), &[Step::AutoTag, Step::Describe { replace: true }], &outputs).unwrap();
        let tx = lib.end_transaction().unwrap();
        assert_eq!(r.changed, 1);
        assert!(lib.item("A2").unwrap().tags.is_empty(), "a trashed item is left alone");
        assert_eq!((lib.item("A1").unwrap().tags.len(), lib.item("A1").unwrap().annotation.as_str()), (2, "d"));
        lib.revert(tx).unwrap();
        assert_eq!((lib.item("A1").unwrap().tags.clone(), lib.item("A1").unwrap().annotation.as_str()), (s(&["a"]), ""));
    }

    #[test]
    fn a_run_undoes_as_one_step() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = library(d.path(), &[&["a"], &["b"]]);
        let f = folder(&mut lib, "F");
        let snap = |lib: &Library| lib.items().iter().map(|i| (i.tags.clone(), i.folders.clone(), i.extra.get("star").cloned())).collect::<Vec<_>>();
        let before = snap(&lib);
        let tags_before = lib.tags.clone();
        lib.begin_transaction("Run");
        lib.run_steps(&s(&["A1", "A2"]), &[Step::AddTags { tags: s(&["n"]) }, Step::AddFolders { folders: vec![f] }, Step::Rating { rating: 2 }, Step::RemoveTags { tags: s(&["a"]) }])
            .unwrap();
        let tx = lib.end_transaction().expect("recorded");
        assert_ne!(snap(&lib), before);
        lib.revert(tx).unwrap();
        assert_eq!(snap(&lib), before, "every step of the run comes back at once");
        assert_eq!(lib.tags, tags_before);
    }
}
