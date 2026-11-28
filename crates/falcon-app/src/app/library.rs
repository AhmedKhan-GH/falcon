//! The open libraries: opening, focusing and closing workspaces, following
//! `eagle://` links and the HTTP API's library switch, the local server,
//! preferences and the app lock, and the recent-libraries list.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;
use falcon_library::Library;

use super::navigation::fastrand_seed;
use super::{App, Shared, Workspace, lock};
use crate::locale::{tr, tr_n, tr_nf, trf};
use crate::logic::{self, Scope};
use crate::ui::dialogs::library_notices::Alert;
use crate::ui::dialogs::switch_warning::Target;
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
        let startup_scope = if let Some(shared) = &ws.lib {
            self.resolve_startup_scope(&lock(shared))
        } else {
            Scope::Home
        };
        ws.scope = startup_scope;
        if self.ws.lib.is_none() {
            // The empty window holds the folder state restored on launch.
            ws.collapsed = std::mem::take(&mut self.ws.collapsed);
            self.ws = ws;
        } else {
            self.others.push(&mut self.ws, ws);
        }
        self.serve_active();
    }

    /// Resolves the startup destination scope based on preferences and active library.
    pub(crate) fn resolve_startup_scope(&self, lib: &Library) -> Scope {
        resolve_startup_destination(
            self.prefs.startup_destination,
            self.prefs.startup_folder_id.as_deref(),
            self.last_scope.as_ref(),
            lib,
        )
    }
}

/// Resolves which `Scope` Falcon opens into for a given library.
pub(crate) fn resolve_startup_destination(
    destination: crate::prefs::StartupDestination,
    specific_folder_id: Option<&str>,
    last_scope: Option<&Scope>,
    lib: &Library,
) -> Scope {
    use crate::prefs::StartupDestination;
    match destination {
        StartupDestination::Home => Scope::Home,
        StartupDestination::All => Scope::All,
        StartupDestination::LastOpened => {
            if let Some(scope) = last_scope.filter(|s| is_valid_scope(s, lib)) {
                return (*scope).clone();
            }
            Scope::Home
        }
        StartupDestination::SpecificFolder => {
            if let Some(fid) = specific_folder_id.filter(|fid| logic::find_folder(&lib.metadata.folders, fid).is_some()) {
                return Scope::Folder(fid.to_string());
            }
            Scope::Home
        }
    }
}

/// Verifies whether `scope` is valid and present in `lib`.
pub(crate) fn is_valid_scope(scope: &Scope, lib: &Library) -> bool {
    match scope {
        Scope::Home | Scope::All | Scope::Uncategorized | Scope::Untagged | Scope::Recent | Scope::Random(_) | Scope::AllTags | Scope::Duplicates | Scope::Trash => true,
        Scope::Folder(id) => logic::find_folder(&lib.metadata.folders, id).is_some(),
        Scope::Tag(t) => lib.items().iter().any(|item| item.tags.iter().any(|x| x == t)),
        Scope::Smart(id) => lib.smart_folders().iter().any(|s| &s.id == id),
    }
}

impl App {

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
        let ai_run = crate::actions::ai::background::settle_closing(&mut closed);
        if let Err(e) = closed.flush_and_backup() {
            self.flash_error(trf("Could not save: {e}", &[("e", &e)]));
        }
        drop(closed);
        if let Some(ended) = ai_run {
            self.present_ai_run(ended);
        }
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
        self.follow_mcp();
    }

    /// `library/switch` over the API replaces the library inside the shown
    /// workspace's slot. Give that slot its own library back and show the
    /// new one in a workspace of its own (an existing one if it is open),
    /// with fresh state and no undo history. The slot gets its library back
    /// at once; showing the new one asks first while tasks run in this one.
    pub(crate) fn follow_api_switch(&mut self) {
        let (Some(shared), Some(was)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        let now = lock(&shared).root().to_path_buf();
        if now == was {
            return;
        }
        let existing = self.position_of(&now);
        match Library::open_or_restore(&was) {
            Ok((back, restored)) => {
                let note = restored.as_deref().map(restored_note);
                let new = std::mem::replace(&mut *lock(&shared), back);
                // The open workspace's copy is the one with its edits.
                let library = existing.is_none().then(|| Box::new(new));
                self.request_switch(Target::Api { root: now, library, note });
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
                self.api_switched(&now, None);
            }
        }
    }

    /// The end of an API switch to the library at `now`, which is shown.
    pub(crate) fn api_switched(&mut self, now: &std::path::Path, note: Option<String>) {
        crate::workspaces::remember(&mut self.recent, now, RECENT_LIMIT);
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
        self.compat = None;
        self.compat_legacy = None;
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
                self.sync_compat();
            }
            Err(e) => {
                crate::errlog::error(&format!("API not started: {e}"));
                self.sync_compat();
                self.show_alert(Alert::new(crate::api_check::title(), crate::api_check::start_failed_message(&ports)));
            }
        }
    }

    /// Starts or stops the listener on Eagle's port to match the preference
    /// and records what happened. It is a second listener of the same API
    /// (see `falcon_server::Handle::start_compat`); it never replaces another
    /// program's listener, and a conflict leaves compatibility off.
    pub(crate) fn sync_compat(&mut self) {
        self.compat = None;
        self.compat_legacy = None;
        self.prefs_window.compat_note = None;
        self.compat_status = if !self.prefs.eagle_compat {
            // Compatibility off also ends any browser import session.
            if let Some(h) = &self.server {
                h.stop_session();
            }
            crate::prefs::CompatStatus::Off
        } else {
            self.start_compat_listeners()
        };
        self.prefs_window.compat_status = self.compat_status.clone();
    }

    /// Opens the listener on Eagle's port and the one for the extension's
    /// older API, refusing any port something else already uses.
    fn start_compat_listeners(&mut self) -> crate::prefs::CompatStatus {
        let Some(h) = &self.server else { return crate::prefs::CompatStatus::NeedsApi };
        let ctx = self.ctx.clone();
        match h.start_compat(falcon_server::EAGLE_PORT, move || ctx.request_repaint()) {
            Ok(l) => {
                let port = l.port;
                self.compat = Some(l);
                // The extension saves through its older API on its own port.
                let ctx = self.ctx.clone();
                self.prefs_window.compat_note = Some(match h.start_legacy(falcon_server::LEGACY_PORT, move || ctx.request_repaint()) {
                    Ok(l2) => {
                        self.compat_legacy = Some(l2);
                        trf("Older extension API on port {port}: listening.", &[("port", &falcon_server::LEGACY_PORT)])
                    }
                    Err(e) => trf("Older extension API on port {port} not started: {error}", &[("port", &falcon_server::LEGACY_PORT), ("error", &e.to_string())]),
                });
                crate::prefs::CompatStatus::Listening(port)
            }
            Err(falcon_server::CompatError::InUse(m)) => crate::prefs::CompatStatus::Conflict(m),
            Err(falcon_server::CompatError::Failed(m)) => crate::prefs::CompatStatus::Failed(m),
        }
    }

    /// The toolbar's "Save from the browser": opens the extension's ports if
    /// they aren't open and starts a 10-minute session, in one click. Without
    /// the Preferences switch the ports close again when the session ends.
    pub(crate) fn start_browser_session(&mut self) {
        if self.server.is_none() {
            self.flash_error(tr("Turn on the local API in Preferences ▸ Developer first."));
            return;
        }
        if self.compat.is_none() {
            self.compat_status = self.start_compat_listeners();
            self.prefs_window.compat_status = self.compat_status.clone();
        }
        match &self.compat_status {
            crate::prefs::CompatStatus::Conflict(m) | crate::prefs::CompatStatus::Failed(m) => {
                let m = trf("Not started: {error}", &[("error", m)]);
                self.compat = None;
                self.compat_legacy = None;
                self.flash_error(m);
            }
            _ => {
                if let Some(h) = &self.server {
                    h.start_session(600);
                }
                self.flash(tr("Browser import session started"));
            }
        }
    }

    pub(crate) fn stop_browser_session(&mut self) {
        if let Some(h) = &self.server {
            h.stop_session();
        }
        self.flash(tr("Browser import session stopped"));
    }

    /// Carries out what Preferences asked of the browser import session and
    /// tells it how long is left.
    /// Keeps the API's recent folders and the app's the same, both ways.
    fn sync_recent_folders(&mut self) {
        let Some(h) = &self.server else { return };
        if let Some(v) = h.take_recent_folders() {
            self.server_recents = v.clone();
            self.recent_folders = v;
        } else if self.recent_folders != self.server_recents {
            h.set_recent_folders(self.recent_folders.clone());
            self.server_recents = self.recent_folders.clone();
        }
    }

    fn drive_session(&mut self) {
        self.sync_recent_folders();
        match self.prefs_window.session_cmd.take() {
            Some(crate::prefs::SessionCmd::Start) => self.start_browser_session(),
            Some(crate::prefs::SessionCmd::Stop) => self.stop_browser_session(),
            None => {}
        }
        self.prefs_window.session_remaining = self.server.as_ref().and_then(|h| h.session_remaining());
        if self.prefs_window.session_remaining.is_some() {
            // The toolbar and Preferences show a countdown.
            self.ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        // Ports opened only for a session close with it.
        if self.prefs_window.session_remaining.is_none() && !self.prefs.eagle_compat && (self.compat.is_some() || self.compat_legacy.is_some()) {
            self.compat = None;
            self.compat_legacy = None;
            self.compat_status = crate::prefs::CompatStatus::Off;
            self.prefs_window.compat_status = crate::prefs::CompatStatus::Off;
            self.prefs_window.compat_note = None;
        }
    }

    /// Asks before a browser import is committed: where it comes from, what
    /// and how many, and where it would go. Nothing is imported until Import.
    pub(crate) fn import_confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(h) = &self.server else { return };
        let Some(p) = h.pending_imports().into_iter().next() else { return };
        if self.import_attention != Some(p.id) {
            self.import_attention = Some(p.id);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("browser-import-confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(egui::RichText::new(tr("Import from the browser?")).size(15.0));
            ui.add_space(6.0);
            ui.label(trf("Source: {site}", &[("site", &p.site)]));
            ui.label(trf("Items: {count}", &[("count", &p.count)]));
            ui.label(trf("From: {hosts}", &[("hosts", &p.hosts.join(", "))]));
            let folder = if p.folder.is_empty() { tr("Library root").to_string() } else { p.folder.clone() };
            ui.label(trf("Into: {folder}", &[("folder", &folder)]));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Import")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer = Some(false);
        }
        if let Some(a) = answer {
            h.resolve_import(p.id, a);
        }
    }

    /// Hands Preferences the last browser extension origin the API refused.
    pub(crate) fn poll_blocked_origin(&mut self) {
        self.drive_session();
        if self.prefs_window.open {
            self.prefs_window.blocked_origin = self.server.as_ref().and_then(|h| h.blocked_origin());
            self.prefs_window.recent_requests = self
                .server
                .as_ref()
                .map(|h| {
                    h.recent_requests()
                        .iter()
                        .rev()
                        .take(10)
                        .map(|r| {
                            let t = r.secs % 86400;
                            let when = format!("{:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60);
                            format!("{when} :{} {} {} origin={} ref={} fetch={} [{}] {} → {} ({})", r.port, r.method, r.path, if r.origin.is_empty() { "-" } else { &r.origin }, if r.referer.is_empty() { "-" } else { &r.referer }, r.fetch, r.flags, r.caller, r.status, r.why)
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
    }

    /// Drops the API warning when it is the alert on screen.
    fn clear_api_warning(&mut self) {
        if self.alert.as_ref().is_some_and(|a| crate::api_check::is_title(&a.title)) {
            self.alert = None;
        }
    }

    /// Takes the result of the localhost check of the running API and warns
    /// when extensions couldn't reach it.
    pub(crate) fn poll_api_check(&mut self) {
        let Some((port, reached)) = self.api_check.poll() else { return };
        if !reached && self.server.as_ref().is_some_and(|h| h.port == port) {
            self.show_alert(Alert::new(crate::api_check::title(), crate::api_check::unreachable_message(port)));
        }
    }

    /// Hands the API what Eagle's reads from the app: the Developer token,
    /// the preferences `application/info` reports, and the recent
    /// libraries `library/history` lists.
    pub(crate) fn share_with_server(&self) {
        let Some(h) = &self.server else { return };
        h.set_token(&self.prefs.api_token);
        h.set_approved_origins(&self.prefs.approved_extension_origins);
        let ctx = self.ctx.clone();
        h.set_notify(move || ctx.request_repaint());
        h.set_preferences(self.prefs.eagle_preferences());
        h.set_history(self.recent.iter().map(|p| p.display().to_string()).collect());
        if let Some(engine) = &self.ai.engine {
            h.set_ai_search(engine.clone());
        }
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
        self.show_mcp_status();
        self.prefs_window.compat_status = self.compat_status.clone();
        self.prefs_window.blocked_origin = self.server.as_ref().and_then(|h| h.blocked_origin());
    }

    /// Whether menus and boxes are translucent: as being edited in
    /// Preferences, else as saved.
    pub(crate) fn vibrancy_on(&self) -> bool {
        self.prefs_window.live_vibrancy().unwrap_or(self.prefs.vibrancy)
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
        let compat_changed = self.prefs.eagle_compat != d.prefs.eagle_compat;
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
                self.flash_error(trf("Could not change opening at login: {error}", &[("error", &e)]));
            }
            self.prefs_window.refresh_login();
        }
        // A folder refused before is checked again.
        self.auto_import.refused = None;
        self.share_with_server();
        self.follow_mcp();
        if restart {
            self.start_server();
            if self.ws.lib.is_some() && !self.prefs.api_enabled {
                self.flash(tr("API stopped"));
            }
        } else if compat_changed {
            self.sync_compat();
        }
        if let Some(h) = &self.server
            && h.blocked_origin().is_some_and(|o| self.prefs.approved_extension_origins.contains(&o))
        {
            h.clear_blocked_origin();
        }
    }

    /// Opens the create-library dialog (`library_dialogs.rs`), after the
    /// warning when tasks run in the shown library.
    pub(crate) fn new_library(&mut self) {
        self.request_switch(Target::Create);
    }

    /// A folder dialog for choosing a library or where to keep one. It
    /// starts in the folder that holds the shown library; with none shown,
    /// the system picks the starting folder.
    pub(crate) fn library_folder_dialog(&self, title: &str) -> rfd::FileDialog {
        let dialog = crate::dialog_owner::file_dialog().set_title(title);
        match self.ws.root.as_deref().and_then(std::path::Path::parent).filter(|p| !p.as_os_str().is_empty()) {
            Some(folder) => dialog.set_directory(folder),
            None => dialog,
        }
    }

    /// Asks for a library and opens it in the background, with a loading
    /// screen (after the warning when tasks run in the shown library).
    pub(crate) fn pick_library(&mut self) {
        if let Some(p) = self.library_folder_dialog(tr("Open Eagle Library")).pick_folder() {
            self.request_switch(Target::Open(p));
        }
    }

    /// The layout panel's Refresh: writes anything pending, reads the shown
    /// library again from disk, decodes its thumbnails again and reshuffles
    /// random orders, keeping the view and selection.
    pub(crate) fn refresh_library(&mut self) {
        if self.reload_library() {
            self.flash_brief(tr("Refreshed"));
        }
    }

    /// A smart folder's Refresh (a falcon addition): the same reload, then a
    /// toast with the folder's name and its fresh item count. The view and
    /// selection stay as they were.
    pub(crate) fn refresh_smart_folder(&mut self, id: &str) {
        if !self.reload_library() {
            return;
        }
        let found = self.ws.lib.as_ref().and_then(|shared| {
            let lib = lock(shared);
            let mut name = None;
            for root in lib.smart_folders() {
                root.walk(&mut Vec::new(), &mut |f, _| {
                    if f.id == id {
                        name = Some(f.name.clone());
                    }
                });
            }
            Some((name?, lib.smart_folder_items(id).len()))
        });
        match found {
            Some((name, n)) => self.flash(smart_refreshed_text(&name, n)),
            None => self.flash_brief(tr("Refreshed")),
        }
    }

    /// Writes anything pending and reads the library again. False (after an
    /// error toast) when it could not.
    fn reload_library(&mut self) -> bool {
        let (Some(shared), Some(root)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return false };
        if let Err(e) = self.ws.flush_and_backup() {
            self.flash_error(trf("Could not save: {e}", &[("e", &e)]));
            return false;
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
                true
            }
            Err(e) => {
                self.flash_error(trf("Could not refresh: {e}", &[("e", &e)]));
                false
            }
        }
    }
}

/// Deduces the external physical directory for a Falcon folder from its linked items directly from `&Library`.
/// Operates on the already-provided reference without re-acquiring mutexes (preventing UI deadlocks),
/// and samples up to 10 items for fast responsiveness.
pub(crate) fn deduce_source_path_from_lib(lib: &Library, folder_id: &str) -> Option<PathBuf> {
    let mut path_counts: HashMap<PathBuf, usize> = HashMap::new();
    let mut checked = 0;
    for item in lib.items() {
        if !item.is_deleted && item.folders.iter().any(|f| f == folder_id) {
            if let Some(src) = lib.last_known_source_path(&item.id).or_else(|| lib.resolved_source_path(&item.id).ok()) {
                if let Some(parent) = src.parent() {
                    if parent.is_dir() {
                        *path_counts.entry(parent.to_path_buf()).or_default() += 1;
                        checked += 1;
                        if checked >= 10 {
                            break;
                        }
                    }
                }
            }
        }
    }
    path_counts.into_iter().max_by_key(|(_, c)| *c).map(|(p, _)| p)
}

/// The toast after a smart folder's Refresh.
fn smart_refreshed_text(name: &str, n: usize) -> String {
    tr_nf(n, "{name} now has {n} item", "{name} now has {n} items", &[("name", &name)])
}

#[cfg(test)]
mod tests {
    use super::{restored_note, smart_refreshed_text};
    use std::path::Path;

    #[test]
    fn smart_refresh_toast_names_the_folder_and_counts() {
        assert_eq!(smart_refreshed_text("Pictures", 1), "Pictures now has 1 item");
        assert_eq!(smart_refreshed_text("Pictures", 0), "Pictures now has 0 items");
        assert_eq!(smart_refreshed_text("Pictures", 12), "Pictures now has 12 items");
    }

    #[test]
    fn restore_note_names_the_library_and_backup() {
        let note = restored_note(Path::new("/s/Photos.library/backup/backup-2026-10-26 02.15.25.310.json"));
        assert_eq!(note, "Photos's metadata.json was damaged and was restored from backup/backup-2026-10-26 02.15.25.310.json; the damaged file was kept in backup/");
    }

    #[test]
    fn deduce_source_path_empty_folder_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = falcon_library::Library::create(tmp.path().join("L.library")).unwrap();
        assert_eq!(super::deduce_source_path_from_lib(&lib, "nonexistent"), None);
    }
}
