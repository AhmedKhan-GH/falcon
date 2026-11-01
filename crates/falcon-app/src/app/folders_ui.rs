//! Several folders or smart folders selected in the sidebar: the ⌘ and ⇧
//! clicks that pick them and the combined view they open. The selection
//! rules live in `logic::TreeSel`.

use super::*;
use crate::logic::TreeSel;

/// Folders and smart folders picked in the sidebar, and the rows each
/// tree showed, top to bottom, for ⇧-click ranges.
#[derive(Default)]
pub(super) struct FolderUi {
    pub sel: TreeSel,
    pub smart: TreeSel,
    /// Rows drawn in the last complete frame, and those drawn so far in
    /// this one.
    rows: Vec<String>,
    rows_next: Vec<String>,
    smart_rows: Vec<String>,
    smart_rows_next: Vec<String>,
}

/// Which sidebar tree a row belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tree {
    Folders,
    Smart,
}

impl FolderUi {
    /// Start of a sidebar frame: the rows drawn last frame become the
    /// rows ranges are taken from.
    pub fn begin_rows(&mut self) {
        self.rows = std::mem::take(&mut self.rows_next);
        self.smart_rows = std::mem::take(&mut self.smart_rows_next);
    }

    /// Notes a row as drawn this frame.
    pub fn row(&mut self, tree: Tree, id: &str) {
        match tree {
            Tree::Folders => self.rows_next.push(id.to_string()),
            Tree::Smart => self.smart_rows_next.push(id.to_string()),
        }
    }

    /// Forgets both selections (any change of view clears them, as in
    /// Eagle).
    pub fn clear(&mut self) {
        self.sel.clear();
        self.smart.clear();
    }

    /// Whether the view shows the combined items of picked folders or
    /// smart folders rather than the open scope alone.
    pub fn combined(&self) -> bool {
        !self.sel.ids.is_empty() || !self.smart.ids.is_empty()
    }
}

impl App {
    /// A click on a folder or smart folder row with ⌘ or ⇧ held. Returns
    /// false when neither is held, leaving the plain click to the caller.
    pub(super) fn tree_click(&mut self, tree: Tree, id: &str, m: Modifiers) -> bool {
        if !m.command && !m.shift {
            return false;
        }
        let open = match (&self.ws.scope, tree) {
            (Scope::Folder(o), Tree::Folders) | (Scope::Smart(o), Tree::Smart) => Some(o.clone()),
            _ => None,
        };
        let scope = |id: &str| match tree {
            Tree::Folders => Scope::Folder(id.to_string()),
            Tree::Smart => Scope::Smart(id.to_string()),
        };
        if m.command && open.is_none() {
            // Nothing of this tree is open: the click opens the row, which
            // is the same view as picking it alone.
            self.go(scope(id));
            return true;
        }
        let ui = &mut self.ws.folders;
        let (sel, rows) = match tree {
            Tree::Folders => (&mut ui.sel, &ui.rows),
            Tree::Smart => (&mut ui.smart, &ui.smart_rows),
        };
        if m.command {
            sel.toggle(open.as_deref(), id);
        } else if !sel.extend(rows, open.as_deref(), id) {
            return true;
        }
        // Picking in one tree drops the other tree's picks.
        match tree {
            Tree::Folders => ui.smart.clear(),
            Tree::Smart => ui.sel.clear(),
        }
        self.ws.selected.clear();
        self.ws.anchor = None;
        true
    }

    /// Whether a sidebar row shows as selected: picked, or, with nothing
    /// picked, the open scope.
    pub(super) fn tree_active(&self, tree: Tree, id: &str) -> bool {
        let sel = match tree {
            Tree::Folders => &self.ws.folders.sel,
            Tree::Smart => &self.ws.folders.smart,
        };
        if sel.ids.is_empty() {
            match tree {
                Tree::Folders => self.ws.scope == Scope::Folder(id.to_string()),
                Tree::Smart => self.ws.scope == Scope::Smart(id.to_string()),
            }
        } else {
            sel.contains(id)
        }
    }

    /// The items of every picked folder or smart folder, for the combined
    /// view; `None` when nothing is picked.
    pub(super) fn combined_items(&self, lib: &Library) -> Option<HashSet<String>> {
        let f = &self.ws.folders;
        if !f.sel.ids.is_empty() {
            let picked: HashSet<&str> = f.sel.ids.iter().map(String::as_str).collect();
            return Some(
                lib.items().iter().filter(|i| !i.is_deleted && i.folders.iter().any(|x| picked.contains(x.as_str()))).map(|i| i.id.clone()).collect(),
            );
        }
        if !f.smart.ids.is_empty() {
            return Some(f.smart.ids.iter().flat_map(|id| lib.smart_folder_items(id)).map(|i| i.id.clone()).collect());
        }
        None
    }

    /// The toolbar title while several folders or smart folders are picked.
    pub(super) fn combined_title(&self) -> Option<String> {
        let f = &self.ws.folders;
        if f.sel.is_multi() {
            Some(format!("{} folders selected", f.sel.ids.len()))
        } else if f.smart.is_multi() {
            Some(format!("{} smart folders selected", f.smart.ids.len()))
        } else {
            None
        }
    }
}
