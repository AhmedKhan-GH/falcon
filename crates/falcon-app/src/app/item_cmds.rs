//! Item commands from Eagle's card menu and Edit and Organize menus that
//! need more than one action: Add to Last Used Folder. This file is a
//! child module of `app`.

use super::{Action, App};
use crate::logic::Scope;

/// Most folders remembered as recently used (Eagle keeps 50).
pub const RECENT_FOLDERS: usize = 50;

/// Puts `ids` at the front of the recently used folders, the last of them
/// first, without repeats, keeping at most [`RECENT_FOLDERS`].
pub fn note_folders(list: &mut Vec<String>, ids: &[String]) {
    for id in ids {
        list.retain(|x| x != id);
        list.insert(0, id.clone());
    }
    list.truncate(RECENT_FOLDERS);
}

/// The most recently used folder that still exists. Like Eagle, only the
/// first eight remembered are looked at.
pub fn last_used(list: &[String], exists: impl Fn(&str) -> bool) -> Option<String> {
    list.iter().take(8).find(|id| exists(id)).cloned()
}

impl App {
    /// Remembers folders items were just added to.
    pub(super) fn note_folder_use(&mut self, ids: &[String]) {
        note_folders(&mut self.recent_folders, ids);
    }

    /// Eagle's Add to Last Used Folder (⇧D): the items join the folder
    /// they were last added to. On the Uncategorized page they leave the
    /// grid, so the selection moves on.
    pub(super) fn add_to_last_used(&mut self, ids: Vec<String>) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let target = {
            let lib = super::lock(&shared);
            last_used(&self.recent_folders, |id| crate::logic::find_folder(&lib.metadata.folders, id).is_some())
        };
        let Some(fid) = target else {
            self.flash("No folder used yet");
            return;
        };
        if ids.is_empty() {
            return;
        }
        if self.ws.scope == Scope::Uncategorized {
            self.leave_viewed(&ids);
            self.select_after_removal(&ids);
        }
        self.ws.actions.push(Action::AddToFolder(ids, fid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn recent_folders_are_newest_first_without_repeats() {
        let mut list = v(&["a", "b", "c"]);
        note_folders(&mut list, &v(&["c", "d"]));
        assert_eq!(list, v(&["d", "c", "a", "b"]));
        let mut long: Vec<String> = (0..60).map(|i| i.to_string()).collect();
        note_folders(&mut long, &v(&["x"]));
        assert_eq!(long.len(), RECENT_FOLDERS);
        assert_eq!(long[0], "x");
    }

    #[test]
    fn the_last_used_folder_must_still_exist() {
        let list = v(&["gone", "b", "c"]);
        assert_eq!(last_used(&list, |id| id != "gone"), Some("b".into()));
        assert_eq!(last_used(&list, |_| false), None);
        let far: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        assert_eq!(last_used(&far, |id| id == "9"), None, "only the first eight count");
    }
}
