//! Actions: the open library's named lists of organising steps, each
//! with an optional shortcut, run on the selection from the Actions menu,
//! the Actions panel (toolbar button, G) or the shortcut.
//!
//! The list lives in the library's `actions.config.json`
//! (`falcon_library::actions`), as in Eagle 4.0.0, and is read again
//! when the file changes on disk (checked every four seconds, as Eagle
//! polls it). This module holds the state and the rules; `app.rs` runs
//! actions and routes the menu and keys, `panel.rs` and `editor.rs` draw.

pub(crate) mod app;
pub(crate) mod editor;
pub(crate) mod panel;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use falcon_library::actions::{ActionDef, ActionsFile, Step};

use crate::shortcuts::{self, Shortcuts};

/// How often the file is checked for changes made elsewhere, in seconds.
pub const POLL_SECS: f64 = 4.0;

/// Running an action on this many items or more asks first, as Eagle's
/// Actions menu does.
pub const RUN_CONFIRM: usize = 30;

/// A run queued for the reducer: the action's name and steps, and the
/// items.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub name: String,
    pub ids: Vec<String>,
    pub steps: Vec<Step>,
}

/// The Actions panel's own state.
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    pub query: String,
    /// Where it opened (under the toolbar button), or centred when `None`.
    pub anchor: Option<eframe::egui::Pos2>,
    /// The frame it opened in: the click that opened it is not a click
    /// outside it.
    pub opened: u64,
    pub focus: bool,
    /// An action whose removal waits for a yes.
    pub confirm_remove: Option<String>,
}

/// The open library's actions.
#[derive(Default)]
pub struct ActionsState {
    root: Option<PathBuf>,
    file: ActionsFile,
    defs: Vec<ActionDef>,
    /// Why the file couldn't be read; nothing is written over it then.
    pub broken: Option<String>,
    stamp: Option<SystemTime>,
    checked_at: Option<f64>,
    pub panel: Panel,
    pub editor: Option<editor::Editor>,
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl ActionsState {
    /// Follows the open library: reads its file when the library changes
    /// and, every `POLL_SECS`, when the file's modification time moved.
    /// Returns whether the list was read again.
    pub fn follow(&mut self, root: Option<&Path>, now: f64) -> bool {
        if self.root.as_deref() != root {
            self.root = root.map(Path::to_path_buf);
            self.panel = Panel::default();
            self.editor = None;
            self.reload();
            self.checked_at = Some(now);
            return true;
        }
        if self.checked_at.is_some_and(|t| now - t < POLL_SECS) {
            return false;
        }
        self.checked_at = Some(now);
        let Some(root) = &self.root else { return false };
        if modified(&ActionsFile::path(root)) == self.stamp {
            return false;
        }
        self.reload();
        true
    }

    fn reload(&mut self) {
        self.broken = None;
        let Some(root) = &self.root else {
            self.file = ActionsFile::default();
            self.defs.clear();
            self.stamp = None;
            return;
        };
        self.stamp = modified(&ActionsFile::path(root));
        match ActionsFile::load(root) {
            Ok(f) => self.file = f,
            Err(e) => {
                // Eagle lists nothing then; falcon also keeps its hands off.
                self.file = ActionsFile::default();
                self.broken = Some(e.to_string());
            }
        }
        self.defs = self.file.actions();
    }

    /// The actions, in the file's order.
    pub fn defs(&self) -> &[ActionDef] {
        &self.defs
    }

    pub fn find(&self, id: &str) -> Option<&ActionDef> {
        self.defs.iter().find(|d| d.id == id)
    }

    /// The Actions menu's entries.
    pub fn menu_entries(&self) -> Vec<crate::menu::ActionEntry> {
        self.defs.iter().map(|d| crate::menu::ActionEntry { id: d.id.clone(), name: d.name.clone(), chord: chord_of(d), runnable: d.runnable }).collect()
    }

    /// Names and chords, for the Shortcuts pane.
    pub fn shortcut_list(&self) -> Vec<(String, Option<KeyboardShortcut>)> {
        self.defs.iter().map(|d| (d.name.clone(), chord_of(d))).collect()
    }

    /// The action a key press runs. Modifiers must match exactly, as with
    /// Eagle's menu accelerators.
    pub fn by_press(&self, key: Key, pressed: Modifiers) -> Option<&ActionDef> {
        let p = shortcuts::normalize(pressed);
        self.defs.iter().find(|d| chord_of(d).is_some_and(|c| c.logical_key == key && shortcuts::normalize(c.modifiers) == p))
    }

    /// What already uses `chord`, other than action `own`: another
    /// action (its name) or a command (its label).
    pub fn conflict(&self, chord: KeyboardShortcut, own: &str, keys: &Shortcuts) -> Option<String> {
        if let Some(d) = self.defs.iter().find(|d| d.id != own && chord_of(d).is_some_and(|c| shortcuts::same_chord(c, chord))) {
            return Some(d.name.clone());
        }
        keys.used_by(chord).map(|c| crate::locale::tr(shortcuts::spec(c).label).to_string())
    }

    /// Applies an edit to the file and writes it. Refused while no library
    /// is open or the file on disk couldn't be read.
    pub fn commit(&mut self, edit: impl FnOnce(&mut ActionsFile)) -> Result<(), String> {
        let Some(root) = self.root.clone() else { return Err(crate::locale::tr("No library is open").into()) };
        if let Some(e) = &self.broken {
            return Err(e.clone());
        }
        // Someone else may have changed it since the last check.
        if modified(&ActionsFile::path(&root)) != self.stamp {
            self.reload();
            if let Some(e) = &self.broken {
                return Err(e.clone());
            }
        }
        let mut file = self.file.clone();
        edit(&mut file);
        file.save(&root).map_err(|e| e.to_string())?;
        self.file = file;
        self.defs = self.file.actions();
        self.stamp = modified(&ActionsFile::path(&root));
        Ok(())
    }

    /// A run of action `id` on `ids`, if falcon can run it.
    pub fn run_of(&self, id: &str, ids: Vec<String>) -> Option<Run> {
        let d = self.find(id).filter(|d| d.runnable)?;
        Some(Run { name: d.name.clone(), ids, steps: d.steps.clone() })
    }
}

/// An action's shortcut as a chord.
pub fn chord_of(d: &ActionDef) -> Option<KeyboardShortcut> {
    d.shortcut.as_deref().and_then(shortcuts::parse)
}

/// A name for a copy of `name` not yet taken by `taken`.
pub fn copy_name(name: &str, taken: &[&str]) -> String {
    let base = crate::locale::trf("{name} Copy", &[("name", &name)]);
    if !taken.contains(&base.as_str()) {
        return base;
    }
    (2..).map(|n| format!("{base} {n}")).find(|c| !taken.contains(&c.as_str())).unwrap_or(base)
}

/// The actions whose name contains every word of `query`, ignoring case.
pub fn filter<'a>(defs: &'a [ActionDef], query: &str) -> Vec<&'a ActionDef> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    defs.iter().filter(|d| words.iter().all(|w| d.name.to_lowercase().contains(w))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(text: &str) -> (tempfile::TempDir, ActionsState) {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("actions.config.json"), text).unwrap();
        let mut s = ActionsState::default();
        assert!(s.follow(Some(d.path()), 0.0));
        (d, s)
    }

    const TWO: &str = r#"[{"id":"A","name":"Tag as reference","shortcut":"Command + Shift + 1","steps":[{"type":"addTags","tags":["ref"]}]},{"id":"B","name":"Rate five","shortcut":"","steps":[{"type":"rating","rating":5}]}]"#;

    #[test]
    fn follows_the_library_and_polls_the_file() {
        let (d, mut s) = state_with(TWO);
        assert_eq!(s.defs().len(), 2);
        assert!(!s.follow(Some(d.path()), 1.0), "not before the poll interval");
        // Written by Eagle meanwhile (a later modification time).
        std::fs::write(d.path().join("actions.config.json"), "[]").unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(d.path().join("actions.config.json")).unwrap().set_modified(later).unwrap();
        assert!(s.follow(Some(d.path()), 5.0));
        assert!(s.defs().is_empty());
        assert!(s.follow(None, 6.0));
        assert!(s.menu_entries().is_empty());
    }

    #[test]
    fn a_broken_file_lists_nothing_and_is_never_overwritten() {
        let (d, mut s) = state_with("{not json");
        assert!(s.defs().is_empty() && s.broken.is_some());
        assert!(s.commit(|f| f.remove("A")).is_err());
        assert_eq!(std::fs::read_to_string(d.path().join("actions.config.json")).unwrap(), "{not json");
    }

    #[test]
    fn commit_writes_the_file() {
        let (d, mut s) = state_with(TWO);
        let mut b = s.find("B").unwrap().clone();
        b.shortcut = Some("Alt + 5".into());
        s.commit(|f| f.put(&b)).unwrap();
        assert_eq!(s.find("B").unwrap().shortcut.as_deref(), Some("Alt + 5"));
        let disk = std::fs::read_to_string(d.path().join("actions.config.json")).unwrap();
        assert!(disk.ends_with(r#"{"id":"B","name":"Rate five","shortcut":"Alt + 5","steps":[{"type":"rating","rating":5}]}]"#), "{disk}");
        // No library open: nothing to write to.
        let mut none = ActionsState::default();
        assert!(none.commit(|f| f.remove("A")).is_err());
    }

    #[test]
    fn shortcuts_match_exactly() {
        let (_d, s) = state_with(TWO);
        let cmd_shift = Modifiers { shift: true, ..Modifiers::COMMAND };
        assert_eq!(s.by_press(Key::Num1, cmd_shift).map(|d| d.id.as_str()), Some("A"));
        assert_eq!(s.by_press(Key::Num1, Modifiers { shift: true, mac_cmd: true, ..Modifiers::NONE }).map(|d| d.id.as_str()), Some("A"), "⌘ as reported on a Mac");
        assert!(s.by_press(Key::Num1, Modifiers::COMMAND).is_none());
        assert!(s.by_press(Key::Num1, Modifiers { alt: true, ..cmd_shift }).is_none());
    }

    #[test]
    fn conflicts_with_commands_and_other_actions() {
        let (_d, s) = state_with(TWO);
        let keys = Shortcuts::default();
        let c = |text: &str| shortcuts::parse(text).unwrap();
        assert_eq!(s.conflict(c("Command + Shift + 1"), "B", &keys).as_deref(), Some("Tag as reference"));
        assert_eq!(s.conflict(c("Command + Shift + 1"), "A", &keys), None, "its own key");
        // ⌘T adds tags; G opens the Actions panel.
        assert!(s.conflict(c("Command + T"), "B", &keys).is_some());
        assert!(s.conflict(c("G"), "B", &keys).is_some());
        assert_eq!(s.conflict(c("Command + Shift + Alt + 9"), "B", &keys), None);
    }

    #[test]
    fn copies_get_free_names_and_search_matches_words() {
        assert_eq!(copy_name("Sort", &[]), "Sort Copy");
        assert_eq!(copy_name("Sort", &["Sort Copy", "Sort Copy 2"]), "Sort Copy 3");
        let f = ActionsFile::parse(TWO).unwrap().actions();
        assert_eq!(filter(&f, "").len(), 2);
        assert_eq!(filter(&f, "REF tag").iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["A"]);
        assert!(filter(&f, "nothing").is_empty());
    }

    #[test]
    fn runs_only_what_falcon_understands() {
        let (_d, s) = state_with(r#"[{"id":"P","name":"AI","shortcut":"","prompt":"x"},{"id":"B","name":"Rate","shortcut":"","steps":[{"type":"rating","rating":1}]}]"#);
        assert!(s.run_of("P", vec!["I".into()]).is_none());
        let r = s.run_of("B", vec!["I".into()]).unwrap();
        assert_eq!(r.steps, [Step::Rating { rating: 1 }]);
        assert_eq!(s.menu_entries().iter().map(|e| e.runnable).collect::<Vec<_>>(), [false, true]);
    }
}
