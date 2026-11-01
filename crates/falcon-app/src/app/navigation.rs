//! Where the user is in a library: going to a view, back and forward,
//! the layout remembered per view, clicks that select, Recently Used.

use std::path::PathBuf;

use eframe::egui::Modifiers;
use falcon_library::Library;

use super::App;
use crate::logic::{self, Layout, Scope};

/// The parent of the open folder or smart folder, as a scope.
pub(crate) fn parent_scope(scope: &Scope, lib: &Library) -> Option<Scope> {
    use crate::organize::locate;
    match scope {
        Scope::Folder(id) => locate(&lib.metadata.folders, id).and_then(|(p, _)| p).map(|p| Scope::Folder(p.to_string())),
        Scope::Smart(id) => locate(&lib.smart_folders(), id).and_then(|(p, _)| p.map(|p| Scope::Smart(p.to_string()))),
        _ => None,
    }
}

pub(crate) fn fastrand_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

impl App {
    pub(crate) fn go(&mut self, scope: Scope) {
        let scope = match scope {
            Scope::Random(_) => Scope::Random(fastrand_seed()),
            s => s,
        };
        // Opening any view, even the one open, drops picked folders and
        // takes a fresh look at Recently Used.
        self.ws.folders.clear();
        self.recent_shown = None;
        if scope == self.ws.scope {
            return;
        }
        // The Duplicates page can scan the view it was opened from.
        if scope == Scope::Duplicates {
            self.ws.duplicates.opened_from(self.ws.scope.clone(), self.ws.order.clone(), self.ws.selected.clone());
        }
        self.ws.back.push(std::mem::replace(&mut self.ws.scope, scope));
        self.ws.forward.clear();
        self.ws.selected.clear();
        self.ws.anchor = None;
        self.view_changed();
    }

    /// The shown library's path, the key its pages share a layout under.
    pub(crate) fn root_key(&self) -> String {
        self.ws.root.as_ref().map(|r| r.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Puts the layout the current view opens in into `ws.layout`.
    pub(crate) fn sync_layout(&mut self) {
        self.ws.layout = logic::layout_for(&self.layouts, &self.ws.scope, &self.root_key(), self.legacy_layout);
    }

    /// Switches the current view to `layout` and remembers it for this
    /// folder or smart folder, or for the library's other pages, as Eagle
    /// does.
    pub(crate) fn set_layout(&mut self, layout: Layout) {
        self.layouts.insert(logic::layout_key(&self.ws.scope, &self.root_key()), layout);
        self.ws.layout = layout;
    }

    pub(crate) fn go_back(&mut self) {
        if let Some(s) = self.ws.back.pop() {
            self.ws.forward.push(std::mem::replace(&mut self.ws.scope, s));
            self.ws.selected.clear();
            self.ws.folders.clear();
            self.view_changed();
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if let Some(s) = self.ws.forward.pop() {
            self.ws.back.push(std::mem::replace(&mut self.ws.scope, s));
            self.ws.selected.clear();
            self.ws.folders.clear();
            self.view_changed();
        }
    }

    /// Applies a click on the card at `id` with the given modifiers.
    pub(crate) fn click(&mut self, id: &str, m: Modifiers) {
        if m.command {
            if let Some(p) = self.ws.selected.iter().position(|s| s == id) {
                self.ws.selected.remove(p);
            } else {
                self.ws.selected.push(id.to_string());
            }
            self.ws.anchor = Some(id.to_string());
        } else if m.shift
            && let Some(a) = self.ws.anchor.as_ref().and_then(|a| self.ws.order.iter().position(|x| x == a))
            && let Some(b) = self.ws.order.iter().position(|x| x == id)
        {
            let (lo, hi) = (a.min(b), a.max(b));
            self.ws.selected = self.ws.order[lo..=hi].to_vec();
        } else {
            self.ws.selected = vec![id.to_string()];
            self.ws.anchor = Some(id.to_string());
        }
    }

    /// Records library originals that were opened or copied as Recently
    /// Used, as Eagle does for its open, Open With, new window and copy
    /// commands. Other files are ignored.
    pub(crate) fn used(&mut self, files: &[PathBuf]) {
        if let Some(root) = self.ws.root.clone() {
            self.recent_items.add_many(&root, &crate::recent::item_ids(files));
        }
    }

    /// Records one item opened in the preview window as Recently Used.
    pub(crate) fn used_item(&mut self, id: &str) {
        if let Some(root) = self.ws.root.clone() {
            self.recent_items.add(&root, id);
        }
    }
}
