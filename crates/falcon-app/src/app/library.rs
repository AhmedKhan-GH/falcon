//! The open libraries: opening, focusing and closing workspaces, following
//! `eagle://` links and the HTTP API's library switch, the local server,
//! preferences and the app lock, and the recent-libraries list.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;
use falcon_library::Library;

use super::navigation::fastrand_seed;
use super::{App, Shared, Workspace, lock};
use crate::locale::{tr, tr_n, trf};
use crate::logic::{self, Scope};
use crate::ui::dialogs::library_notices::Alert;
use crate::ui::sidebar::smart_has;

/// Most libraries kept in the recent list.
pub(crate) const RECENT_LIMIT: usize = 10;

/// Whether two paths name the same library folder (exact, or after
/// resolving symlinks and `..`).
pub(crate) fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// What the toast says when opening a library restored its damaged
/// `metadata.json` from `backup` (see `Library::open_or_restore`).
pub(crate) fn restored_note(backup: &std::path::Path) -> String {
    let lib = backup.parent().and_then(std::path::Path::parent).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = backup.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    trf("{lib}'s metadata.json was damaged and was restored from backup/{name}; the damaged file was kept in backup/", &[("lib", &lib), ("name", &name)])
}

impl App {
    /// Opens an `eagle://` link in the shown library, as Eagle does: an
    /// item opens in the viewer inside its first folder (or All), a folder
    /// or smart folder is shown. Ids the library doesn't have are ignored.
    pub(crate) fn follow_link(&mut self, link: crate::links::Link) {
        use crate::links::Link;
        let Some(shared) = self.ws.lib.clone() else { return };
        let target = {
            let lib = lock(&shared);
            match &link {
                Link::Item(id) => lib.item(id).map(|i| match i.folders.first().filter(|f| logic::find_folder(&lib.metadata.folders, f).is_some()) {
                    Some(f) => Scope::Folder(f.clone()),
                    None => Scope::All,
                }),
                Link::Folder(id) => logic::find_folder(&lib.metadata.folders, id).map(|_| Scope::Folder(id.clone())),
                Link::Smart(id) => lib.smart_folders().iter().any(|f| smart_has(f, id)).then(|| Scope::Smart(id.clone())),
            }
        };
        let Some(scope) = target else { return };
        self.go(scope);
        if let Link::Item(id) = link {
            self.ws.selected = vec![id.clone()];
            self.ws.anchor = Some(id.clone());
            self.ws.scroll_to = Some(id.clone());
            self.ws.viewer = Some(id);
        }
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Opens the library at `path` in a workspace of its own and shows it,
    /// or shows it if it is already open.
    pub(crate) fn open(&mut self, path: PathBuf) {
        if let Some(i) = self.position_of(&path) {
            self.focus(i);
            crate::workspaces::remember(&mut self.recent, &path, RECENT_LIMIT);
            return;
        }
        let result = crate::install_dir::guard(&path).and_then(|()| Library::open_or_restore(&path));
        self.opened(path, result);
    }

    /// Shows a library [`App::open`] or a background load
    /// (`crate::ui::dialogs::library_notices::Loading`) read, or reports why it couldn't be read.
    pub(crate) fn opened(&mut self, path: PathBuf, result: falcon_library::Result<(Library, Option<PathBuf>)>) {
        match result {
            Ok((lib, restored)) => {
                let skipped = lib.skipped.len();
                self.add_workspace(Arc::new(Mutex::new(lib)));
                crate::workspaces::remember(&mut self.recent, &path, RECENT_LIMIT);
                self.error = None;
                self.welcomed = true;
                self.missing = None;
                let mut notes = Vec::new();
                if let Some(backup) = restored {
                    notes.push(restored_note(&backup));
                }
                if skipped > 0 {
                    notes.push(tr_n(skipped, "{n} item couldn't be read and is hidden; its file is untouched", "{n} items couldn't be read and are hidden; their files are untouched"));
                }
                if !notes.is_empty() {
                    self.flash(notes.join(". "));
                }
            }
            Err(_) if !path.exists() => self.library_gone(path),
            Err(e) => {
                if let Some(alert) = crate::ui::dialogs::library_notices::alert_for(&e) {
                    crate::errlog::error(&format!("Could not open the library: {e}"));
                    self.show_alert(alert);
                } else if self.ws.lib.is_some() {
                    self.flash_error(trf("Could not open the library: {e}", &[("e", &e)]));
                } else {
                    crate::errlog::error(&format!("Could not open the library: {e}"));
                }
                self.error = Some(e.to_string());
            }
        }
    }

    /// Shows `lib` in a new workspace, after the open ones (or in place of
    /// the empty window). It starts with the shown library's sort and
    /// layout.
    pub(crate) fn add_workspace(&mut self, lib: Shared) {
        let mut ws = Workspace::new(&self.ctx, Some(lib));
        ws.sort = self.ws.sort;
        ws.increase = self.ws.increase;
        ws.layout = self.ws.layout;
        if self.ws.lib.is_none() {
            // The empty window holds the folder state restored on launch.
            ws.collapsed = std::mem::take(&mut self.ws.collapsed);
            self.ws = ws;
        } else {
            self.others.push(&mut self.ws, ws);
        }
        self.serve_active();
    }

    /// Where the library at `path` sits in the switcher, if it is open.
    pub(crate) fn position_of(&self, path: &std::path::Path) -> Option<usize> {
        self.others.iter(&self.ws).position(|w| w.root.as_deref().is_some_and(|r| same_path(r, path)))
    }

    /// Shows the open library at switcher position `i`.
    pub(crate) fn focus(&mut self, i: usize) {
        if i == self.others.slot() {
            return;
        }
        self.others.focus(&mut self.ws, i);
        self.focus_search = false;
        self.serve_active();
    }

    /// Closes the open library at switcher position `i`, writing anything
    /// pending first. Closing the last one leaves the empty window.
    pub(crate) fn close_workspace(&mut self, i: usize) {
        let mut closed = match self.others.close(&mut self.ws, i) {
            Some(w) => w,
            None if i == self.others.slot() => {
                let mut empty = Workspace::new(&self.ctx, None);
                (empty.sort, empty.increase, empty.layout) = (self.ws.sort, self.ws.increase, self.ws.layout);
                std::mem::replace(&mut self.ws, empty)
            }
            None => return,
        };
        if let Err(e) = closed.flush_and_backup() {
            self.flash_error(trf("Could not save: {e}", &[("e", &e)]));
        }
        drop(closed);
        self.serve_active();
    }

    /// Points the HTTP API at the shown library: swaps it into the running
    /// server, starts one if none runs, or stops it when nothing is open.
    pub(crate) fn serve_active(&mut self) {
        match (&self.server, self.ws.lib.clone()) {
            (Some(h), Some(lib)) => {
                h.set_library(lib);
                self.share_with_server();
            }
            (None, Some(_)) => self.start_server(),
            (_, None) => self.server = None,
        }
    }

    /// `library/switch` over the API replaces the library inside the shown
    /// workspace's slot. Give that slot its own library back and show the
    /// new one in a workspace of its own (an existing one if it is open),
    /// with fresh state and no undo history.
    pub(crate) fn follow_api_switch(&mut self) {
        let (Some(shared), Some(was)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        let now = lock(&shared).root().to_path_buf();
        if now == was {
            return;
        }
        let existing = self.position_of(&now);
        let mut note = None;
        match Library::open_or_restore(&was) {
            Ok((back, restored)) => {
                note = restored.as_deref().map(restored_note);
                let new = std::mem::replace(&mut *lock(&shared), back);
                match existing {
                    // The open workspace's copy is the one with its edits.
                    Some(i) => {
                        drop(new);
                        self.focus(i);
                    }
                    None => self.add_workspace(Arc::new(Mutex::new(new))),
                }
            }
            Err(_) => {
                // The old library can't be reopened: this workspace becomes
                // the new one, starting over.
                let mut ws = Workspace::new(&self.ctx, Some(shared));
                (ws.sort, ws.increase, ws.layout) = (self.ws.sort, self.ws.increase, self.ws.layout);
                self.ws = ws;
                if let Some(i) = existing {
                    self.close_workspace(i);
                }
            }
        }
        crate::workspaces::remember(&mut self.recent, &now, RECENT_LIMIT);
        let name = now.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match note {
            Some(note) => self.flash(trf("Switched to {name} over the API. {note}", &[("name", &name), ("note", &note)])),
            None => self.flash(trf("Switched to {name} over the API", &[("name", &name)])),
        }
        self.serve_active();
    }

    /// (Re)starts the HTTP API for the shown library on the ports set in
    /// Preferences. Never Eagle's own port: clients of a running (or later
    /// started) Eagle must keep reaching Eagle.
    pub(crate) fn start_server(&mut self) {
        self.server = None;
        self.api_check.cancel();
        self.clear_api_warning();
        let (Some(shared), Some(ports)) = (self.ws.lib.clone(), self.prefs.api_ports()) else { return };
        let ctx = self.ctx.clone();
        match falcon_server::start(shared, ports.clone(), move || ctx.request_repaint()) {
            Ok(h) => {
                self.flash(trf("API on http://localhost:{port}", &[("port", &h.port)]));
                let ctx = self.ctx.clone();
                self.api_check.start(h.port, move || ctx.request_repaint());
                self.server = Some(h);
                self.share_with_server();
            }
            Err(e) => {
                crate::errlog::error(&format!("API not started: {e}"));
                self.show_alert(Alert::new(crate::api_check::TITLE, crate::api_check::start_failed_message(&ports)));
            }
        }
    }

    /// Drops the API warning when it is the alert on screen.
    fn clear_api_warning(&mut self) {
        if self.alert.as_ref().is_some_and(|a| a.title == crate::api_check::TITLE) {
            self.alert = None;
        }
    }

    /// Takes the result of the localhost check of the running API and warns
    /// when extensions couldn't reach it.
    pub(crate) fn poll_api_check(&mut self) {
        let Some((port, reached)) = self.api_check.poll() else { return };
        if !reached && self.server.as_ref().is_some_and(|h| h.port == port) {
            self.show_alert(Alert::new(crate::api_check::TITLE, crate::api_check::unreachable_message(port)));
        }
    }

    /// Hands the API what Eagle's reads from the app: the Developer token,
    /// the preferences `application/info` reports, and the recent
    /// libraries `library/history` lists.
    pub(crate) fn share_with_server(&self) {
        let Some(h) = &self.server else { return };
        h.set_token(&self.prefs.api_token);
        h.set_preferences(self.prefs.eagle_preferences());
        h.set_history(self.recent.iter().map(|p| p.display().to_string()).collect());
    }

    /// Keeps what the API changed in the preferences (`preferences/collect/*`).
    pub(crate) fn follow_api_preferences(&mut self) {
        if let Some(v) = self.server.as_ref().and_then(|h| h.take_preferences()) {
            self.prefs.take_eagle_preferences(&v);
        }
    }

    pub(crate) fn open_preferences(&mut self) {
        let panels = crate::prefs::Panels { sidebar: self.show_sidebar, inspector: self.show_inspector };
        self.prefs_window.show_with(crate::prefs::Draft { prefs: self.prefs.clone(), panels, keys: self.shortcuts.clone(), launch_at_login: crate::login_item::status().is_on() });
        self.prefs_window.library = self.ws.root.clone();
    }

    /// Lock Now: hides everything behind the lock screen, stopping playback
    /// and closing Preferences and the collect window.
    pub(crate) fn lock_now(&mut self) {
        if !self.lock.lock(self.prefs.app_lock.as_ref()) {
            return;
        }
        self.video.keep_only(None);
        self.audio.stop();
        self.prefs_window.close();
        if self.collect.open {
            self.collect.toggle();
        }
    }

    /// Takes the Preferences window's Save or Apply.
    pub(crate) fn apply_preferences(&mut self, d: crate::prefs::Draft) {
        let restart = self.prefs.api_changed(&d.prefs);
        // The collect folder is chosen in the collect window, which may
        // have changed it while Preferences was open.
        let folder = self.prefs.collect_folder.take();
        let chosen = std::mem::take(&mut self.prefs.collect_folders);
        let recent = std::mem::take(&mut self.prefs.collect_recent);
        self.prefs = d.prefs;
        self.prefs.collect_folder = folder;
        self.prefs.collect_folders = chosen;
        self.prefs.collect_recent = recent;
        self.ctx.set_zoom_factor(crate::prefs::zoom_factor(self.prefs.ui_zoom));
        let lang = self.prefs.language.resolve();
        crate::locale::load_fonts(&self.ctx, lang);
        crate::locale::set(lang);
        self.show_sidebar = d.panels.sidebar;
        self.show_inspector = d.panels.inspector;
        self.shortcuts = d.keys;
        if d.launch_at_login != crate::login_item::status().is_on() {
            if let Err(e) = crate::login_item::set(d.launch_at_login) {
                self.flash_error(format!("Could not change opening at login: {e}"));
            }
            self.prefs_window.refresh_login();
        }
        // A folder refused before is checked again.
        self.auto_import.refused = None;
        self.share_with_server();
        if restart {
            self.start_server();
            if self.ws.lib.is_some() && !self.prefs.api_enabled {
                self.flash(tr("API stopped"));
            }
        }
    }

    /// Opens the create-library dialog (`library_dialogs.rs`).
    pub(crate) fn new_library(&mut self) {
        self.dialogs.start_create();
    }

    /// Asks for a library and opens it in the background, with a loading
    /// screen.
    pub(crate) fn pick_library(&mut self) {
        if let Some(p) = rfd::FileDialog::new().set_title(tr("Open Eagle Library")).set_directory(falcon_library::sandbox::default_root()).pick_folder() {
            self.open_in_background(p);
        }
    }

    /// The layout panel's Refresh: writes anything pending, reads the shown
    /// library again from disk, decodes its thumbnails again and reshuffles
    /// random orders, keeping the view and selection.
    pub(crate) fn refresh_library(&mut self) {
        let (Some(shared), Some(root)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        if let Err(e) = self.ws.flush_and_backup() {
            self.flash_error(trf("Could not save: {e}", &[("e", &e)]));
            return;
        }
        match crate::install_dir::guard(&root).and_then(|()| Library::open_or_restore(&root)) {
            Ok((fresh, _)) => {
                let ids: Vec<String> = {
                    let mut lib = lock(&shared);
                    let ids = lib.items().iter().map(|i| i.id.clone()).collect();
                    *lib = fresh;
                    ids
                };
                for id in &ids {
                    self.ws.loader.forget(id);
                }
                self.ws.org.reshuffle();
                if let Scope::Random(_) = self.ws.scope {
                    self.ws.scope = Scope::Random(fastrand_seed());
                }
                self.flash_brief(tr("Refreshed"));
            }
            Err(e) => self.flash_error(trf("Could not refresh: {e}", &[("e", &e)])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::restored_note;
    use std::path::Path;

    #[test]
    fn restore_note_names_the_library_and_backup() {
        let note = restored_note(Path::new("/s/Photos.library/backup/backup-2026-10-26 02.15.25.310.json"));
        assert_eq!(note, "Photos's metadata.json was damaged and was restored from backup/backup-2026-10-26 02.15.25.310.json; the damaged file was kept in backup/");
    }
}
