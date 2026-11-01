//! Library-level notices and recovery: the alert box for libraries falcon
//! can't use (made by a newer Eagle, changed elsewhere while open), and
//! reloading a library from disk after a sync conflict, and taking in
//! changes other apps make to open libraries (`watch.rs`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;

use eframe::egui::{self, RichText};
use falcon_library::library_merge::{self as merge, MergePlan};
use falcon_library::pack::{self, PackPreview};
use falcon_library::{Error, Library};

use crate::app::{App, Workspace, import, lock};
use crate::locale::{tr, tr_n, tr_nf, trf};
use crate::watch::Change;
use crate::theme as t;
use crate::ui::widgets::window_drag;

/// A message that needs acknowledging, shown in a modal.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Alert {
    pub title: String,
    pub body: String,
}

impl Alert {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Alert { title: title.into(), body: body.into() }
    }
}

/// The alert for an error that has one, else `None` (shown as a toast).
pub(crate) fn alert_for(e: &Error) -> Option<Alert> {
    match e {
        Error::NewerLibrary { path, version } => Some(Alert::new(
            tr("Made by a newer Eagle"),
            trf(
                "{name} was last saved by Eagle {version}. Falcon reads libraries up to Eagle {supported}, so it won't open or change this one. Open it in Eagle, or use a newer falcon.",
                &[("name", &name(path)), ("version", version), ("supported", &falcon_library::library::APP_VERSION)],
            ),
        )),
        Error::SyncConflict(path) => Some(Alert::new(
            tr("Library changed elsewhere"),
            trf(
                "Another app or a sync service changed {name} while it was open here, so falcon didn't save over it. The library was reloaded from disk; the last change made here may need doing again.",
                &[("name", &name(path))],
            ),
        )),
        _ => None,
    }
}

fn name(path: &std::path::Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

impl App {
    /// Shows `alert` (replacing any shown).
    pub(crate) fn show_alert(&mut self, alert: Alert) {
        self.alert = Some(alert);
    }

    /// The alert modal, if one is up.
    pub(crate) fn alert_modal(&mut self, ctx: &egui::Context) {
        let Some(a) = self.alert.clone() else { return };
        let modal = egui::Modal::new(egui::Id::unique("library-alert")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(&a.title).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(&a.body).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.button(tr("OK")).clicked()).inner
        });
        if modal.inner || modal.should_close() {
            self.alert = None;
        }
    }

    /// After a frame's edits: if a `metadata.json` write was refused, as
    /// Eagle does, say so and reload the library from disk next frame.
    pub(crate) fn check_conflict(&mut self, lib: &mut Library) {
        if lib.take_conflict() {
            self.conflict_reload = true;
        }
    }

    /// Reloads the shown library from disk after a refused write. Pending
    /// writes other than the refused metadata go out first; a library that
    /// can no longer be opened (now made by a newer Eagle, or gone) closes.
    pub(crate) fn reload_after_conflict(&mut self) {
        let (Some(shared), Some(root)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        let _ = lock(&shared).flush();
        match falcon_library::sandbox::guard(&root).and_then(|()| Library::open(&root)) {
            Ok(fresh) => {
                let ids: Vec<String> = {
                    let mut lib = lock(&shared);
                    let ids = lib.items().iter().map(|i| i.id.clone()).collect();
                    *lib = fresh;
                    ids
                };
                for id in &ids {
                    self.ws.loader.forget(id);
                }
                // The undo history describes the state that was replaced.
                self.ws.undo.clear();
                self.ws.redo.clear();
                self.ws.edit = None;
                self.ws.folder_edit = None;
                if let Some(a) = alert_for(&Error::SyncConflict(root)) {
                    self.show_alert(a);
                }
            }
            Err(e) => {
                let alert = alert_for(&e).unwrap_or_else(|| Alert::new(tr("Library closed"), trf("Falcon couldn't read the library again: {error}", &[("error", &e)])));
                self.close_workspace(self.others.slot());
                self.show_alert(alert);
            }
        }
    }
}

impl App {
    /// Applies what the libraries' watchers saw: a newer `metadata.json`
    /// replaces the folder tree (and the undo history, which no longer
    /// fits it), items added, edited or deleted elsewhere appear, change
    /// or go, and a library whose folder vanished closes.
    pub(crate) fn apply_outside_changes(&mut self) {
        let mut vanished = Vec::new();
        let mut storage = Vec::new();
        for ws in std::iter::once(&mut self.ws).chain(self.others.parked_mut()) {
            if let Some(rx) = &ws.health
                && let Ok(warnings) = rx.try_recv()
            {
                ws.health = None;
                if !warnings.is_empty()
                    && let Some(root) = &ws.root
                {
                    storage.push((root.clone(), warnings));
                }
            }
            let changes = ws.watch.as_ref().map(|w| w.drain()).unwrap_or_default();
            if changes.is_empty() {
                continue;
            }
            let Some(shared) = ws.lib.clone() else { continue };
            let mut lib = lock(&shared);
            for change in changes {
                match change {
                    Change::Metadata(m) => {
                        if lib.take_external_metadata(*m) {
                            ws.undo.clear();
                            ws.redo.clear();
                            ws.folder_edit = None;
                        }
                    }
                    Change::Mtime(index) => {
                        let got = lib.sync_items(&index);
                        for id in got.changed.iter().chain(&got.removed) {
                            ws.loader.forget(id);
                        }
                        if ws.edit.as_ref().is_some_and(|e| got.changed.contains(&e.id) || got.removed.contains(&e.id)) {
                            ws.edit = None;
                        }
                    }
                    Change::Missing => vanished.extend(ws.root.clone()),
                }
            }
        }
        for (root, warnings) in storage {
            let text = warnings.iter().map(crate::storage::Warning::message).collect::<Vec<_>>().join(" ");
            if warnings.iter().any(crate::storage::Warning::is_serious) {
                self.show_alert(Alert::new(trf("{name} may not save changes", &[("name", &name(&root))]), text));
            } else {
                self.flash(trf("{name}: {text}", &[("name", &name(&root)), ("text", &text)]));
            }
        }
        for root in vanished {
            if let Some(i) = self.position_of(&root) {
                self.close_vanished(i);
            }
            self.library_gone(root);
        }
    }

    /// Closes the workspace at switcher position `i` whose library folder
    /// is gone, without the flush and backup a normal close does (their
    /// writes would fail, or recreate files in a folder that moved).
    fn close_vanished(&mut self, i: usize) {
        let closed = match self.others.close(&mut self.ws, i) {
            Some(w) => w,
            None if i == self.others.slot() => {
                let mut empty = Workspace::new(&self.ctx, None);
                (empty.sort, empty.increase, empty.layout) = (self.ws.sort, self.ws.increase, self.ws.layout);
                std::mem::replace(&mut self.ws, empty)
            }
            None => return,
        };
        if let Some(shared) = &closed.lib {
            // Nothing pending may be written to where the library was.
            lock(shared).discard_pending_writes();
        }
        drop(closed);
        self.serve_active();
    }

    /// The library at `root` can no longer be found: the missing-library
    /// screen takes over (a dialog over another open library).
    pub(crate) fn library_gone(&mut self, root: std::path::PathBuf) {
        self.missing = Some(root);
    }

    /// The missing-library screen, when no library is open.
    pub(crate) fn missing_page(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        window_drag(ui, egui::Rect::from_min_size(r.min, egui::vec2(r.width(), t::BAR_HEIGHT)), "missing");
        ui.painter().rect_filled(r, 0.0, t::p().main);
        let card = egui::Rect::from_center_size(r.center(), egui::vec2(420.0, 240.0));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(card).layout(egui::Layout::top_down(egui::Align::Center)));
        c.label(RichText::new(crate::icons::FOLDER_OPEN).font(t::font(40.0)).color(t::p().text_muted));
        c.add_space(6.0);
        self.missing_contents(&mut c, false);
    }

    /// The same, as a dialog while other libraries are open.
    pub(crate) fn missing_modal(&mut self, ctx: &egui::Context) {
        if self.missing.is_none() {
            return;
        }
        let modal = egui::Modal::new(egui::Id::unique("library-missing")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.vertical_centered(|ui| self.missing_contents(ui, true));
        });
        if modal.should_close() {
            self.missing = None;
        }
    }

    /// Title, path and the ways out: Try Again, Select Another Path…, and
    /// Create a New Library (plus Close in the dialog).
    fn missing_contents(&mut self, ui: &mut egui::Ui, dialog: bool) {
        let Some(path) = self.missing.clone() else { return };
        ui.label(RichText::new(tr("Library not found")).size(17.0).color(t::p().text));
        ui.add_space(4.0);
        ui.label(RichText::new(trf("There is no library folder at {path}. It may have been moved, renamed or deleted, or its drive isn't connected.", &[("path", &crate::workspaces::tilde(&path))])).color(t::p().text_muted));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Try Again")).clicked() {
                self.open(path.clone());
                if self.missing.is_some() {
                    self.flash(tr("The library is still missing"));
                }
            }
            if ui.button(tr("Select Another Path…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title(tr("Open Eagle Library")).set_directory(falcon_library::sandbox::default_root()).pick_folder() {
                    self.open(p);
                }
                if self.missing.is_none() {
                    // Found elsewhere: the old place leaves the history.
                    crate::workspaces::remove_from_list(&mut self.recent, &path);
                }
            }
            if dialog && ui.button(tr("Close")).clicked() {
                self.missing = None;
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Can't find it?")).size(12.0).color(t::p().text_muted));
            if ui.link(RichText::new(tr("Create a new library")).size(12.0)).clicked() {
                self.new_library();
            }
        });
    }
}

/// Merging another library into the shown one (`falcon_library::library_merge`).
pub(crate) enum Merge {
    /// Asking, with what would come in.
    Confirm { into: PathBuf, plan: Box<MergePlan> },
    /// Copying item folders on a background thread.
    Copying { into: PathBuf, plan: Arc<MergePlan>, done: Arc<AtomicUsize>, cancel: Arc<AtomicBool>, rx: Receiver<Copied> },
}

type Copied = (Vec<String>, Vec<(String, String)>);

impl App {
    /// Library ▸ Merge Another Library…: pick a library and ask.
    pub(crate) fn merge_library(&mut self) {
        let (Some(shared), Some(into)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        if self.merge.is_some() {
            return;
        }
        let Some(source) = rfd::FileDialog::new().set_title(tr("Choose a Library to Merge In")).set_directory(falcon_library::sandbox::default_root()).pick_folder() else {
            return;
        };
        if let Err(e) = falcon_library::sandbox::guard(&source) {
            self.show_alert(Alert::new(tr("Can't merge that library"), e.to_string()));
            return;
        }
        let planned = merge::plan(&lock(&shared), &source);
        match planned {
            Ok(plan) => self.merge = Some(Merge::Confirm { into, plan: Box::new(plan) }),
            Err(e) => self.show_alert(Alert::new(tr("Can't merge that library"), e.to_string())),
        }
    }

    /// The merge's confirmation and progress dialogs, and its last step
    /// once the copy is done.
    pub(crate) fn merge_dialogs(&mut self, ctx: &egui::Context) {
        let Some(state) = self.merge.take() else { return };
        self.merge = match state {
            Merge::Confirm { into, plan } => {
                let (folders, items) = plan.counts();
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::unique("merge-confirm")).show(ctx, |ui| {
                    ui.set_width(360.0);
                    ui.label(RichText::new(trf("Merge {source} into {library}?", &[("source", &name(&plan.source)), ("library", &name(&into))])).size(15.0));
                    ui.add_space(4.0);
                    let body = trf(
                        "{folders} and {items} from {path} will be added. Folders, smart folders and tag groups already here are kept as they are, and so are items already here.",
                        &[
                            ("folders", &tr_n(folders, "{n} top-level folder", "{n} top-level folders")),
                            ("items", &tr_n(items, "{n} item", "{n} items")),
                            ("path", &crate::workspaces::tilde(&plan.source)),
                        ],
                    );
                    ui.label(RichText::new(body).color(t::p().text_muted));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr("Merge")).clicked() {
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
                match answer {
                    None => Some(Merge::Confirm { into, plan }),
                    Some(false) => None,
                    Some(true) => Some(start_copy(ctx, into, *plan)),
                }
            }
            Merge::Copying { into, plan, done, cancel, rx } => match rx.try_recv() {
                Ok((copied, failed)) => {
                    self.finish_merge(&into, &plan, &copied, failed.len());
                    None
                }
                Err(_) => {
                    let total = plan.item_dirs.len().max(1);
                    let n = done.load(Ordering::Relaxed);
                    egui::Modal::new(egui::Id::unique("merge-progress")).show(ctx, |ui| {
                        ui.set_width(320.0);
                        ui.label(RichText::new(trf("Merging {name}", &[("name", &name(&plan.source))])).size(15.0));
                        ui.add_space(6.0);
                        ui.add(egui::ProgressBar::new(n as f32 / total as f32).text(trf("{done} of {total}", &[("done", &n), ("total", &plan.item_dirs.len())])));
                        ui.add_space(8.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!cancel.load(Ordering::Relaxed), egui::Button::new(tr("Stop"))).clicked() {
                                cancel.store(true, Ordering::Relaxed);
                            }
                        });
                    });
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    Some(Merge::Copying { into, plan, done, cancel, rx })
                }
            },
        };
    }

    /// Merges the folder tree and registers the copied items in the
    /// library they were copied into (wherever it sits in the switcher).
    fn finish_merge(&mut self, into: &std::path::Path, plan: &MergePlan, copied: &[String], failed: usize) {
        let Some(shared) = self.others.iter(&self.ws).find(|w| w.root.as_deref() == Some(into)).and_then(|w| w.lib.clone()) else { return };
        let result = lock(&shared).finish_merge(plan, copied);
        match result {
            Ok(r) => {
                let mut body = trf(
                    "Added {items}, {folders} and {others} from {name}.",
                    &[
                        ("items", &tr_n(r.items, "{n} item", "{n} items")),
                        ("folders", &tr_n(r.folders, "{n} folder", "{n} folders")),
                        ("others", &tr_n(r.smart_folders + r.tag_groups, "{n} smart folder or tag group", "{n} smart folders or tag groups")),
                        ("name", &name(&plan.source)),
                    ],
                );
                if failed > 0 {
                    body.push(' ');
                    body.push_str(&tr_n(failed, "{n} item could not be copied.", "{n} items could not be copied."));
                }
                if copied.len() < plan.item_dirs.len() && failed == 0 {
                    body.push(' ');
                    body.push_str(tr("Items already in this library were left as they are."));
                }
                self.show_alert(Alert::new(tr("Merge finished"), body));
            }
            Err(e) => self.show_alert(Alert::new(tr("Merge not finished"), trf("The items were copied, but the folders couldn't be saved: {error}", &[("error", &e)]))),
        }
    }
}

fn start_copy(ctx: &egui::Context, into: PathBuf, plan: MergePlan) -> Merge {
    let plan = Arc::new(plan);
    let (done, cancel) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
    let (tx, rx) = std::sync::mpsc::channel();
    let (p, d, c, root, ctx2) = (plan.clone(), done.clone(), cancel.clone(), into.clone(), ctx.clone());
    std::thread::spawn(move || {
        let out = merge::copy_items(&root, &p, &c, |n, _| {
            d.store(n, Ordering::Relaxed);
            ctx2.request_repaint();
        });
        let _ = tx.send(out);
        ctx2.request_repaint();
    });
    Merge::Copying { into, plan, done, cancel, rx }
}

/// Importing an `.eaglepack`: Eagle confirms the disk space it takes,
/// then asks about items the library already has.
pub(crate) enum PackAsk {
    Space { path: PathBuf, folder: Option<String>, preview: PackPreview },
    Duplicates { path: PathBuf, folder: Option<String>, preview: PackPreview },
}

impl App {
    /// File ▸ Import ▸ Eagle Package…
    pub(crate) fn import_pack_dialog(&mut self) {
        if self.ws.lib.is_none() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().set_title(tr("Import Eagle Package")).add_filter("Eagle Package", &["eaglepack"]).pick_file() {
            let folder = self.scope_folders().into_iter().next();
            self.preview_pack_import(path, folder);
        }
    }

    /// Reads the pack and asks; `folder` takes every item of an items pack
    /// (a dropped pack goes into the folder it was dropped on).
    pub(crate) fn preview_pack_import(&mut self, path: PathBuf, folder: Option<String>) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let preview = pack::preview_pack(&lock(&shared), &path);
        match preview {
            Ok(preview) => self.pack_ask = Some(PackAsk::Space { path, folder, preview }),
            Err(e) => self.show_alert(Alert::new(tr("Can't import the package"), trf("It may be incomplete or damaged; export or download it again. ({error})", &[("error", &e)]))),
        }
    }

    pub(crate) fn pack_dialogs(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.pack_ask.take() else { return };
        let (title, body, yes, no) = match &ask {
            PackAsk::Space { path, preview, .. } => (
                trf("Import {name}?", &[("name", &name(path))]),
                tr_nf(
                    preview.items,
                    "{n} item will take up {size} of disk space in this library.",
                    "{n} items will take up {size} of disk space in this library.",
                    &[("size", &crate::logic::format_size(preview.bytes))],
                ),
                tr("Import"),
                tr("Cancel"),
            ),
            PackAsk::Duplicates { preview, .. } => (
                tr_n(preview.duplicates, "{n} item already in the library", "{n} items already in the library"),
                tr("They look like items already here. Import them anyway, or leave them out and import the rest.").to_string(),
                tr("Import"),
                tr("Leave Them Out"),
            ),
        };
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("pack-import")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(&title).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(&body).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(yes).clicked() {
                    answer = Some(true);
                }
                if ui.button(no).clicked() {
                    answer = Some(false);
                }
            });
        });
        let closed = modal.should_close();
        match (ask, answer) {
            (PackAsk::Space { .. }, Some(false)) | (PackAsk::Space { .. }, None) if closed => {}
            (PackAsk::Space { path, folder, preview }, Some(true)) if preview.duplicates > 0 => self.pack_ask = Some(PackAsk::Duplicates { path, folder, preview }),
            (PackAsk::Space { path, folder, .. }, Some(true)) => self.start_pack_import(path, folder, false),
            (PackAsk::Space { .. }, Some(false)) => {}
            (PackAsk::Duplicates { path, folder, .. }, Some(keep)) => self.start_pack_import(path, folder, !keep),
            // Dismissing Eagle's duplicates question (Esc) imports everything.
            (PackAsk::Duplicates { path, folder, .. }, None) if closed => self.start_pack_import(path, folder, false),
            (ask, None) => self.pack_ask = Some(ask),
        }
    }
}

impl App {
    /// File ▸ Import ▸ Inboard Backup…: one folder per Inboard folder, then
    /// the images, with their names, tags, links, trashed state and dates.
    pub(crate) fn import_inboard_dialog(&mut self) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let Some(dir) = rfd::FileDialog::new().set_title(tr("Choose an Inboard Backup Folder")).pick_folder() else { return };
        let backup = match falcon_library::inboard::read(&dir) {
            Ok(b) => b,
            Err(_) if !falcon_library::inboard::is_backup(&dir) => {
                self.show_alert(Alert::new(tr("Not an Inboard backup"), trf("{path} has no {file}.", &[("path", &crate::workspaces::tilde(&dir)), ("file", &falcon_library::inboard::INDEX)])));
                return;
            }
            Err(e) => {
                self.show_alert(Alert::new(tr("Can't read the Inboard backup"), e.to_string()));
                return;
            }
        };
        let mut ids = Vec::new();
        {
            let mut lib = lock(&shared);
            for title in &backup.folders {
                match lib.create_folder(None, title) {
                    Ok(id) => ids.push(id),
                    Err(e) => {
                        drop(lib);
                        self.show_alert(Alert::new(tr("Can't import the Inboard backup"), trf("A folder couldn't be created: {error}", &[("error", &e)])));
                        return;
                    }
                }
            }
        }
        let map = falcon_library::inboard::folder_map(&backup.folders, &ids);
        let jobs: Vec<import::Job> = backup
            .items
            .iter()
            .map(|it| {
                let folders: Vec<String> = it.folders.iter().filter_map(|t| map.get(t).cloned()).collect();
                let mut opts = self.prefs.import_options(folders);
                for t in &it.tags {
                    if !opts.tags.contains(t) {
                        opts.tags.push(t.clone());
                    }
                }
                opts.url = it.url.clone();
                opts.name = (!it.name.trim().is_empty()).then(|| it.name.clone());
                opts.trashed = it.trashed;
                opts.added_at = it.added_at;
                import::Job { path: it.file.clone(), opts }
            })
            .collect();
        let mut note = trf(
            "Importing {images} and {folders} from Inboard",
            &[("images", &tr_n(jobs.len(), "{n} image", "{n} images")), ("folders", &tr_n(ids.len(), "{n} folder", "{n} folders"))],
        );
        if backup.missing > 0 {
            note = trf("{note}; {files} missing from the backup", &[("note", &note), ("files", &tr_n(backup.missing, "{n} file", "{n} files"))]);
        }
        self.flash(note);
        self.spawn_import(jobs);
    }
}

impl App {
    /// Library ▸ Empty Cache and Reload: asks first, as Eagle does.
    pub(crate) fn reload_confirm(&mut self, ctx: &egui::Context) {
        if !self.confirm_reload {
            return;
        }
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("empty-cache")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(tr("Empty cache and reload?")).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Falcon reads the whole library from disk again, rebuilds mtime.json from the items and redraws every thumbnail. A large library takes a while.")).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Empty Cache and Reload")).clicked() {
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
        if let Some(go) = answer {
            self.confirm_reload = false;
            if go {
                self.empty_cache_and_reload();
            }
        }
    }

    /// Writes what is pending, reads the shown library again, rebuilds its
    /// `mtime.json` and drops every decoded thumbnail and preview.
    fn empty_cache_and_reload(&mut self) {
        let (Some(shared), Some(root)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        if let Err(e) = self.ws.flush_and_backup() {
            self.flash(trf("Could not save: {error}", &[("error", &e)]));
            return;
        }
        let fresh = falcon_library::sandbox::guard(&root).and_then(|()| Library::open_or_restore(&root)).and_then(|(mut lib, _)| lib.rebuild_mtime().map(|()| lib));
        match fresh {
            Ok(fresh) => {
                let ids: Vec<String> = {
                    let mut lib = lock(&shared);
                    let mut ids: Vec<String> = lib.items().iter().map(|i| i.id.clone()).collect();
                    ids.extend(fresh.items().iter().map(|i| i.id.clone()));
                    *lib = fresh;
                    ids
                };
                for id in &ids {
                    self.ws.loader.forget(id);
                }
                self.ws.duplicates = Default::default();
                self.ws.undo.clear();
                self.ws.redo.clear();
                self.ws.edit = None;
                self.ws.folder_edit = None;
                self.flash(tr("Library reloaded"));
            }
            Err(e) => match alert_for(&e) {
                Some(a) => self.show_alert(a),
                None => self.flash(trf("Could not reload: {error}", &[("error", &e)])),
            },
        }
    }
}

/// A library being read on a background thread, for the loading screen.
pub(crate) struct Loading {
    path: PathBuf,
    /// Item folders read so far, and how many there are (0 until the
    /// folder list is known).
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    rx: Receiver<falcon_library::Result<(Library, Option<PathBuf>)>>,
}

impl App {
    /// Opens the library at `path` on a background thread, showing a
    /// loading screen meanwhile (an open one is just shown).
    pub(crate) fn open_in_background(&mut self, path: PathBuf) {
        if self.position_of(&path).is_some() || self.loading.is_some() {
            if self.loading.is_none() {
                self.open(path);
            }
            return;
        }
        let (done, total) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (tx, rx) = std::sync::mpsc::channel();
        let (d, n, p, ctx) = (done.clone(), total.clone(), path.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let progress = |a: usize, b: usize| {
                d.store(a, Ordering::Relaxed);
                n.store(b, Ordering::Relaxed);
                ctx.request_repaint();
            };
            let result = falcon_library::sandbox::guard(&p).and_then(|()| Library::open_or_restore_with_progress(&p, &progress));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.loading = Some(Loading { path, done, total, rx });
    }

    /// Takes a finished background load.
    pub(crate) fn poll_loading(&mut self) {
        let Some(l) = &self.loading else { return };
        match l.rx.try_recv() {
            Ok(result) => {
                let path = l.path.clone();
                self.loading = None;
                self.opened(path, result);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.loading = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// The loading screen's text: the library, then what is being read.
    fn loading_lines(&self) -> Option<(String, String, Option<f32>)> {
        let l = self.loading.as_ref()?;
        let (done, total) = (l.done.load(Ordering::Relaxed), l.total.load(Ordering::Relaxed));
        let (line, frac) = loading_status(done, total);
        Some((trf("Opening {name}", &[("name", &name(&l.path))]), line, frac))
    }

    /// The loading screen, when no library is open.
    pub(crate) fn loading_page(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        window_drag(ui, egui::Rect::from_min_size(r.min, egui::vec2(r.width(), t::BAR_HEIGHT)), "loading");
        ui.painter().rect_filled(r, 0.0, t::p().main);
        let card = egui::Rect::from_center_size(r.center(), egui::vec2(320.0, 120.0));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(card).layout(egui::Layout::top_down(egui::Align::Center)));
        self.loading_contents(&mut c);
    }

    /// The same, as a dialog over the open library.
    pub(crate) fn loading_modal(&mut self, ctx: &egui::Context) {
        if self.loading.is_none() {
            return;
        }
        egui::Modal::new(egui::Id::unique("library-loading")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.vertical_centered(|ui| self.loading_contents(ui));
        });
    }

    fn loading_contents(&mut self, ui: &mut egui::Ui) {
        let Some((title, line, frac)) = self.loading_lines() else { return };
        ui.label(RichText::new(title).size(15.0).color(t::p().text));
        ui.add_space(6.0);
        let bar = match frac {
            Some(f) => egui::ProgressBar::new(f),
            None => egui::ProgressBar::new(0.0).animate(true),
        };
        ui.add(bar.desired_width(260.0));
        ui.add_space(4.0);
        ui.label(RichText::new(line).size(12.0).color(t::p().text_muted));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// The loading screen's status line and bar fill for `done` of `total`
/// item folders read (`total` 0: still listing them).
fn loading_status(done: usize, total: usize) -> (String, Option<f32>) {
    if total == 0 {
        (tr("Reading the library…").to_string(), None)
    } else if done >= total {
        (tr("Almost there…").to_string(), Some(1.0))
    } else {
        (trf("Reading items: {done} of {total}", &[("done", &done), ("total", &total)]), Some(done as f32 / total as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn loading_lines() {
        assert_eq!(loading_status(0, 0), ("Reading the library…".to_string(), None));
        assert_eq!(loading_status(50, 200), ("Reading items: 50 of 200".to_string(), Some(0.25)));
        assert_eq!(loading_status(200, 200).0, "Almost there…");
    }

    #[test]
    fn alerts_name_the_library() {
        let a = alert_for(&Error::NewerLibrary { path: PathBuf::from("/x/Books.library"), version: "4.2.0".into() }).unwrap();
        assert!(a.body.starts_with("Books was last saved by Eagle 4.2.0."), "{}", a.body);
        assert!(a.body.contains("up to Eagle 4.0.0"));
        let a = alert_for(&Error::SyncConflict(PathBuf::from("/x/Books.library"))).unwrap();
        assert!(a.body.contains("changed Books while"));
        assert_eq!(alert_for(&Error::NotALibrary(PathBuf::from("/x"))), None);
    }
}
