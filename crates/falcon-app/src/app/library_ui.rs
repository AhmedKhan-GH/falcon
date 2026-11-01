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

use super::{App, Workspace, import_ui, lock};
use crate::watch::Change;
use crate::theme as t;

/// A message that needs acknowledging, shown in a modal.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Alert {
    pub title: String,
    pub body: String,
}

impl Alert {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Alert { title: title.into(), body: body.into() }
    }
}

/// The alert for an error that has one, else `None` (shown as a toast).
pub(super) fn alert_for(e: &Error) -> Option<Alert> {
    match e {
        Error::NewerLibrary { path, version } => Some(Alert::new(
            "Made by a newer Eagle",
            format!(
                "{} was last saved by Eagle {version}. Falcon reads libraries up to Eagle {}, so it won't open or change this one. Open it in Eagle, or use a newer falcon.",
                name(path),
                falcon_library::library::APP_VERSION
            ),
        )),
        Error::SyncConflict(path) => Some(Alert::new(
            "Library changed elsewhere",
            format!(
                "Another app or a sync service changed {} while it was open here, so falcon didn't save over it. The library was reloaded from disk; the last change made here may need doing again.",
                name(path)
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
    pub(super) fn show_alert(&mut self, alert: Alert) {
        self.alert = Some(alert);
    }

    /// The alert modal, if one is up.
    pub(super) fn alert_modal(&mut self, ctx: &egui::Context) {
        let Some(a) = self.alert.clone() else { return };
        let modal = egui::Modal::new(egui::Id::unique("library-alert")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(&a.title).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(&a.body).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.button("OK").clicked()).inner
        });
        if modal.inner || modal.should_close() {
            self.alert = None;
        }
    }

    /// After a frame's edits: if a `metadata.json` write was refused, as
    /// Eagle does, say so and reload the library from disk next frame.
    pub(super) fn check_conflict(&mut self, lib: &mut Library) {
        if lib.take_conflict() {
            self.conflict_reload = true;
        }
    }

    /// Reloads the shown library from disk after a refused write. Pending
    /// writes other than the refused metadata go out first; a library that
    /// can no longer be opened (now made by a newer Eagle, or gone) closes.
    pub(super) fn reload_after_conflict(&mut self) {
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
                let alert = alert_for(&e).unwrap_or_else(|| Alert::new("Library closed", format!("Falcon couldn't read the library again: {e}")));
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
    pub(super) fn apply_outside_changes(&mut self) {
        let mut vanished = Vec::new();
        for ws in std::iter::once(&mut self.ws).chain(self.others.parked_mut()) {
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
    pub(super) fn library_gone(&mut self, root: std::path::PathBuf) {
        self.missing = Some(root);
    }

    /// The missing-library screen, when no library is open.
    pub(super) fn missing_page(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        super::window_drag(ui, egui::Rect::from_min_size(r.min, egui::vec2(r.width(), t::BAR_HEIGHT)), "missing");
        ui.painter().rect_filled(r, 0.0, t::p().main);
        let card = egui::Rect::from_center_size(r.center(), egui::vec2(420.0, 240.0));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(card).layout(egui::Layout::top_down(egui::Align::Center)));
        c.label(RichText::new(crate::icons::FOLDER_OPEN).font(t::font(40.0)).color(t::p().text_muted));
        c.add_space(6.0);
        self.missing_contents(&mut c, false);
    }

    /// The same, as a dialog while other libraries are open.
    pub(super) fn missing_modal(&mut self, ctx: &egui::Context) {
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
        ui.label(RichText::new("Library not found").size(17.0).color(t::p().text));
        ui.add_space(4.0);
        ui.label(RichText::new(format!("There is no library folder at {}. It may have been moved, renamed or deleted, or its drive isn't connected.", crate::workspaces::tilde(&path))).color(t::p().text_muted));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Try Again").clicked() {
                self.open(path.clone());
                if self.missing.is_some() {
                    self.flash("The library is still missing");
                }
            }
            if ui.button("Select Another Path…").clicked() {
                self.pick_library();
                if self.missing.is_none() {
                    // Found elsewhere: the old place leaves the history.
                    crate::workspaces::remove_from_list(&mut self.recent, &path);
                }
            }
            if dialog && ui.button("Close").clicked() {
                self.missing = None;
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Can't find it?").size(12.0).color(t::p().text_muted));
            if ui.link(RichText::new("Create a new library").size(12.0)).clicked() {
                self.new_library();
            }
        });
    }
}

/// Merging another library into the shown one (`falcon_library::library_merge`).
pub(super) enum Merge {
    /// Asking, with what would come in.
    Confirm { into: PathBuf, plan: Box<MergePlan> },
    /// Copying item folders on a background thread.
    Copying { into: PathBuf, plan: Arc<MergePlan>, done: Arc<AtomicUsize>, cancel: Arc<AtomicBool>, rx: Receiver<Copied> },
}

type Copied = (Vec<String>, Vec<(String, String)>);

impl App {
    /// Library ▸ Merge Another Library…: pick a library and ask.
    pub(super) fn merge_library(&mut self) {
        let (Some(shared), Some(into)) = (self.ws.lib.clone(), self.ws.root.clone()) else { return };
        if self.merge.is_some() {
            return;
        }
        let Some(source) = rfd::FileDialog::new().set_title("Choose a Library to Merge In").set_directory(falcon_library::sandbox::default_root()).pick_folder() else {
            return;
        };
        if let Err(e) = falcon_library::sandbox::guard(&source) {
            self.show_alert(Alert::new("Can't merge that library", e.to_string()));
            return;
        }
        let planned = merge::plan(&lock(&shared), &source);
        match planned {
            Ok(plan) => self.merge = Some(Merge::Confirm { into, plan: Box::new(plan) }),
            Err(e) => self.show_alert(Alert::new("Can't merge that library", e.to_string())),
        }
    }

    /// The merge's confirmation and progress dialogs, and its last step
    /// once the copy is done.
    pub(super) fn merge_dialogs(&mut self, ctx: &egui::Context) {
        let Some(state) = self.merge.take() else { return };
        self.merge = match state {
            Merge::Confirm { into, plan } => {
                let (folders, items) = plan.counts();
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::unique("merge-confirm")).show(ctx, |ui| {
                    ui.set_width(360.0);
                    ui.label(RichText::new(format!("Merge {} into {}?", name(&plan.source), name(&into))).size(15.0));
                    ui.add_space(4.0);
                    let body = format!(
                        "{} and {} from {} will be added. Folders, smart folders and tag groups already here are kept as they are, and so are items already here.",
                        plural(folders, "top-level folder"),
                        plural(items, "item"),
                        crate::workspaces::tilde(&plan.source)
                    );
                    ui.label(RichText::new(body).color(t::p().text_muted));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Merge").clicked() {
                            answer = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
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
                        ui.label(RichText::new(format!("Merging {}", name(&plan.source))).size(15.0));
                        ui.add_space(6.0);
                        ui.add(egui::ProgressBar::new(n as f32 / total as f32).text(format!("{n} of {}", plan.item_dirs.len())));
                        ui.add_space(8.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!cancel.load(Ordering::Relaxed), egui::Button::new("Stop")).clicked() {
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
                let mut body = format!("Added {}, {} and {} from {}.", plural(r.items, "item"), plural(r.folders, "folder"), plural(r.smart_folders + r.tag_groups, "smart folder or tag group"), name(&plan.source));
                if failed > 0 {
                    body.push_str(&format!(" {} could not be copied.", plural(failed, "item")));
                }
                if copied.len() < plan.item_dirs.len() && failed == 0 {
                    body.push_str(" Items already in this library were left as they are.");
                }
                self.show_alert(Alert::new("Merge finished", body));
            }
            Err(e) => self.show_alert(Alert::new("Merge not finished", format!("The items were copied, but the folders couldn't be saved: {e}"))),
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

fn plural(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

/// Importing an `.eaglepack`: Eagle confirms the disk space it takes,
/// then asks about items the library already has.
pub(super) enum PackAsk {
    Space { path: PathBuf, folder: Option<String>, preview: PackPreview },
    Duplicates { path: PathBuf, folder: Option<String>, preview: PackPreview },
}

impl App {
    /// File ▸ Import ▸ Eagle Package…
    pub(super) fn import_pack_dialog(&mut self) {
        if self.ws.lib.is_none() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().set_title("Import Eagle Package").add_filter("Eagle Package", &["eaglepack"]).pick_file() {
            let folder = self.scope_folders().into_iter().next();
            self.start_pack_import(path, folder);
        }
    }

    /// Reads the pack and asks; `folder` takes every item of an items pack
    /// (a dropped pack goes into the folder it was dropped on).
    pub(super) fn start_pack_import(&mut self, path: PathBuf, folder: Option<String>) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let preview = pack::preview_pack(&lock(&shared), &path);
        match preview {
            Ok(preview) => self.pack_ask = Some(PackAsk::Space { path, folder, preview }),
            Err(e) => self.show_alert(Alert::new("Can't import the package", format!("It may be incomplete or damaged; export or download it again. ({e})"))),
        }
    }

    pub(super) fn pack_dialogs(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.pack_ask.take() else { return };
        let (title, body, yes, no) = match &ask {
            PackAsk::Space { path, preview, .. } => (
                format!("Import {}?", name(path)),
                format!("{} will take up {} of disk space in this library.", plural(preview.items, "item"), crate::logic::format_size(preview.bytes)),
                "Import",
                "Cancel",
            ),
            PackAsk::Duplicates { preview, .. } => (
                "Already in the library".to_string(),
                format!("{} in the package look like items already here. Import them anyway?", plural(preview.duplicates, "item")),
                "Import",
                "Don't Import Them",
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
            (PackAsk::Space { path, folder, .. }, Some(true)) => self.run_pack_import(&path, folder, false),
            (PackAsk::Space { .. }, Some(false)) => {}
            (PackAsk::Duplicates { path, folder, .. }, Some(keep)) => self.run_pack_import(&path, folder, !keep),
            // Closing the duplicates question cancels, as Esc does in Eagle.
            (PackAsk::Duplicates { .. }, None) if closed => {}
            (ask, None) => self.pack_ask = Some(ask),
        }
    }

    fn run_pack_import(&mut self, path: &std::path::Path, folder: Option<String>, skip_duplicates: bool) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let opts = pack::PackImportOptions { folder, skip_duplicates };
        let result = pack::import_pack(&mut lock(&shared), path, &opts);
        match result {
            Ok(ids) => {
                self.flash(format!("Imported {} from {}", plural(ids.len(), "item"), name(path)));
                self.ws.backup.changed(std::time::Instant::now());
            }
            Err(e) => self.show_alert(Alert::new("Can't import the package", format!("It may be incomplete or damaged; export or download it again. ({e})"))),
        }
    }
}

impl App {
    /// File ▸ Import ▸ Inboard Backup…: one folder per Inboard folder, then
    /// the images, with their names, tags, links, trashed state and dates.
    pub(super) fn import_inboard_dialog(&mut self) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let Some(dir) = rfd::FileDialog::new().set_title("Choose an Inboard Backup Folder").pick_folder() else { return };
        let backup = match falcon_library::inboard::read(&dir) {
            Ok(b) => b,
            Err(_) if !falcon_library::inboard::is_backup(&dir) => {
                self.show_alert(Alert::new("Not an Inboard backup", format!("{} has no {}.", crate::workspaces::tilde(&dir), falcon_library::inboard::INDEX)));
                return;
            }
            Err(e) => {
                self.show_alert(Alert::new("Can't read the Inboard backup", e.to_string()));
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
                        self.show_alert(Alert::new("Can't import the Inboard backup", format!("A folder couldn't be created: {e}")));
                        return;
                    }
                }
            }
        }
        let map = falcon_library::inboard::folder_map(&backup.folders, &ids);
        let jobs: Vec<import_ui::Job> = backup
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
                import_ui::Job { path: it.file.clone(), opts }
            })
            .collect();
        let mut note = format!("Importing {} and {} from Inboard", plural(jobs.len(), "image"), plural(ids.len(), "folder"));
        if backup.missing > 0 {
            note.push_str(&format!("; {} missing from the backup", plural(backup.missing, "file")));
        }
        self.flash(note);
        self.spawn_import(jobs);
    }
}

impl App {
    /// Library ▸ Empty Cache and Reload: asks first, as Eagle does.
    pub(super) fn reload_confirm(&mut self, ctx: &egui::Context) {
        if !self.confirm_reload {
            return;
        }
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("empty-cache")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new("Empty cache and reload?").size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new("Falcon reads the whole library from disk again, rebuilds mtime.json from the items and redraws every thumbnail. A large library takes a while.").color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Empty Cache and Reload").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
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
            self.flash(format!("Could not save: {e}"));
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
                self.flash("Library reloaded");
            }
            Err(e) => match alert_for(&e) {
                Some(a) => self.show_alert(a),
                None => self.flash(format!("Could not reload: {e}")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "item"), "1 item");
        assert_eq!(plural(0, "folder"), "0 folders");
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
