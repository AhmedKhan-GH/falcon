//! Import entry points beyond a plain drop on the grid: folders dropped on
//! the sidebar (their tree recreated as Eagle folders), files dropped on a
//! sidebar folder, and the background import every path ends in.

use std::path::PathBuf;

use eframe::egui::{Pos2, Rect};
use falcon_library::folder_import::{self, Verdict};
use falcon_library::import::{Failure, ImportOptions};

use super::{App, lock};
use crate::drop_pos::{self, Target};
use crate::locale::{tr, tr_n, tr_nf, trf};
use crate::logic::{self, Scope};
use crate::theme as t;

/// A path's last component, for messages.
fn display_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// A failed import, with the options to retry it with.
#[derive(Debug, Clone)]
pub(crate) struct ImportError {
    pub failure: Failure,
    pub opts: ImportOptions,
}

/// One file to import and the options to import it with.
pub(crate) struct Job {
    pub path: PathBuf,
    pub opts: ImportOptions,
}

/// Import state that belongs to the window rather than a library.
#[derive(Default)]
pub(crate) struct Imports {
    /// The sidebar's rect last frame (None while hidden), and its visible
    /// folder rows, to tell where a drop from Finder landed.
    pub sidebar: Option<Rect>,
    pub folder_rows: Vec<(Rect, String)>,
    /// A file dialog to show once the frame is done and the library lock
    /// is released.
    pub request: Option<Request>,
    /// Large folders waiting for the user to confirm their import.
    pub pending: Vec<Pending>,
    /// Names of folders refused for their size, to be told about.
    pub refused: Vec<String>,
    /// The failed-imports panel is open.
    pub errors_open: bool,
    /// Clear All is waiting for confirmation.
    pub confirm_clear: bool,
    /// Items a finished batch added, to select next frame.
    pub select_after: Option<Vec<String>>,
}

/// A walked folder with 2,000 files or more, imported only once confirmed.
pub(crate) struct Pending {
    pub tree: folder_import::Tree,
    pub parent: Option<String>,
    pub files: usize,
}

/// An import file dialog, asked for during the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    /// Pick files; they go into `folders`.
    Files { folders: Vec<String> },
    /// Pick folders; each is imported with its tree inside `parent`.
    Folders { parent: Option<String> },
}

impl Imports {
    /// Remembers a sidebar folder row as a drop target, clipped to what
    /// the sidebar's scroll area shows.
    pub fn note_row(&mut self, ui: &eframe::egui::Ui, rect: Rect, id: &str) {
        let shown = rect.intersect(ui.clip_rect());
        if shown.is_positive() {
            self.folder_rows.push((shown, id.to_string()));
        }
    }
}

impl App {
    /// File ▸ Import Folders…: a dialog for one or more folders, each
    /// imported with its structure kept as a new top-level Eagle folder,
    /// as Eagle's menu item does. Safe to call at any point in a frame;
    /// the dialog opens once the frame is done.
    pub fn import_folders_dialog(&mut self) {
        self.imports.request = Some(Request::Folders { parent: None });
    }

    /// Import Files…: a dialog for files to import into the open folder,
    /// as if dropped on the grid. Eagle has no such menu item; it is
    /// falcon's own. The dialog opens once the frame is done.
    pub fn import_files_dialog(&mut self) {
        let folders = self.scope_folders();
        self.imports.request = Some(Request::Files { folders });
    }

    /// Under the empty message in All or an empty folder, an Import Folders…
    /// button, as on Eagle's empty views: from All the folders go to the
    /// top level, from a folder inside it.
    pub(crate) fn empty_import_button(&mut self, ui: &mut eframe::egui::Ui, centre: Pos2) {
        let parent = match &self.ws.scope {
            Scope::All => None,
            Scope::Folder(id) => Some(id.clone()),
            _ => return,
        };
        if !self.ws.query.trim().is_empty() {
            return;
        }
        let at = Rect::from_center_size(centre, eframe::egui::vec2(160.0, 26.0));
        if ui.put(at, eframe::egui::Button::new(tr("Import Folders…"))).clicked() {
            self.imports.request = Some(Request::Folders { parent });
        }
    }

    /// Shows the dialog asked for during the frame, if any, and starts the
    /// import. Called with the library lock released.
    pub(crate) fn run_import_request(&mut self) {
        let Some(request) = self.imports.request.take() else { return };
        if self.ws.lib.is_none() {
            return;
        }
        match request {
            Request::Files { folders } => {
                if let Some(files) = rfd::FileDialog::new().set_title(tr("Import Files")).pick_files() {
                    self.import_into(files, folders);
                }
            }
            Request::Folders { parent } => {
                let Some(picked) = rfd::FileDialog::new().set_title(tr("Import Folders")).pick_folders() else { return };
                // As in Eagle, a library is opened rather than imported,
                // and only when it is the only folder picked.
                let (libraries, dirs) = logic::split_libraries(picked);
                if dirs.is_empty() && libraries.len() == 1 {
                    self.import_into(libraries, Vec::new());
                }
                self.import_folder_trees(dirs, parent);
            }
        }
    }

    /// Files dropped from Finder at `at`: on the grid they go into the open
    /// folder, flattened (Eagle's `onDropContainer`); on a sidebar folder,
    /// files go into it and folders are ignored (`onDropFolder`); on the
    /// rest of the sidebar, folders are imported with their trees at the
    /// top level and loose files are ignored (`onDropSidebar`). A library
    /// among them is opened, wherever it lands.
    pub(crate) fn drop_files(&mut self, paths: Vec<PathBuf>, at: Option<Pos2>) {
        let target = drop_pos::target(at, self.show_sidebar.then_some(self.imports.sidebar).flatten(), &self.imports.folder_rows);
        match target {
            Target::Grid => self.import(paths),
            Target::Folder(id) => {
                let (libraries, rest) = logic::split_libraries(paths);
                let (packs, rest) = split_packs(rest);
                let files: Vec<PathBuf> = rest.into_iter().filter(|p| !p.is_dir()).collect();
                self.import_into(libraries, Vec::new());
                if let Some(first) = packs.into_iter().next() {
                    self.preview_pack_import(first, Some(id.clone()));
                }
                self.import_files(files, vec![id]);
            }
            Target::Sidebar => {
                let (libraries, rest) = logic::split_libraries(paths);
                // Eagle's `onDropSidebar` sends a pack to the open folder.
                let (packs, rest) = split_packs(rest);
                if let Some(first) = packs.into_iter().next() {
                    let folder = self.scope_folders().into_iter().next();
                    self.preview_pack_import(first, folder);
                }
                let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) = rest.into_iter().partition(|p| p.is_dir());
                self.import_into(libraries, Vec::new());
                if dirs.is_empty() && !files.is_empty() {
                    self.flash(tr("Drop folders on the sidebar, or files on the grid or a folder"));
                }
                self.import_folder_trees(dirs, None);
            }
        }
    }

    /// Imports each directory as an Eagle folder of the same name inside
    /// `parent` (or at the top level), with its subfolders recreated and
    /// every file in the folder that mirrors its own, as Eagle's
    /// `uploadFolderToSidebar` does. Eagle's limits apply to each folder:
    /// 15 levels or 100,000 files refuse it, 2,000 files ask first.
    /// Pixave libraries are skipped.
    pub(crate) fn import_folder_trees(&mut self, mut dirs: Vec<PathBuf>, parent: Option<String>) {
        let pixave = dirs.iter().filter(|d| folder_import::is_pixave_library(d)).count();
        dirs.retain(|d| !folder_import::is_pixave_library(d));
        if pixave > 0 {
            self.flash(tr_n(pixave, "Skipped {n} Pixave library", "Skipped {n} Pixave libraries"));
        }
        dirs.sort_by(|a, b| falcon_library::search::natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
        let mut ready = Vec::new();
        for dir in dirs {
            let tree = folder_import::walk(&dir);
            match folder_import::verdict(tree.stats()) {
                Verdict::Import => ready.push(tree),
                Verdict::Confirm(files) => self.imports.pending.push(Pending { tree, parent: parent.clone(), files }),
                Verdict::Refuse => self.imports.refused.push(display_name(&dir)),
            }
        }
        self.import_trees(ready, parent);
    }

    /// Creates the folders for walked trees and imports their files; the
    /// last new folder is opened.
    fn import_trees(&mut self, trees: Vec<folder_import::Tree>, parent: Option<String>) {
        if trees.is_empty() {
            return;
        }
        let Some(shared) = self.ws.lib.clone() else { return };
        let mut jobs = Vec::new();
        let mut opened = None;
        for tree in trees {
            let plan = folder_import::plan(&tree, parent.as_deref(), falcon_library::ids::new_id);
            if let Err(e) = lock(&shared).create_folders(&plan.folders) {
                self.flash_error(trf("Couldn't import {path}: {error}", &[("path", &tree.dir.display()), ("error", &e)]));
                continue;
            }
            opened = plan.root().map(String::from);
            jobs.extend(plan.files.into_iter().map(|(path, folder)| Job { path, opts: self.prefs.import_options(vec![folder]) }));
        }
        if let Some(id) = opened {
            self.go(Scope::Folder(id));
        }
        self.spawn_import(jobs);
    }

    /// The failed imports so far.
    fn import_errors(&self) -> Vec<ImportError> {
        self.ws.progress.lock().unwrap_or_else(|p| p.into_inner()).errors.clone()
    }

    fn set_import_errors(&mut self, errors: Vec<ImportError>) {
        self.ws.progress.lock().unwrap_or_else(|p| p.into_inner()).errors = errors;
    }

    /// While imports have failed, a notice under the toolbar ("3 files
    /// failed to import"), as Eagle's error toast: clicking it opens the
    /// list, its × clears the list after a confirmation.
    pub(crate) fn error_toast(&mut self, ui: &mut eframe::egui::Ui, top_center: Pos2) {
        use eframe::egui::{Align2, CornerRadius, Sense, Stroke, StrokeKind, vec2};
        let n = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner()).errors.len();
        if n == 0 {
            return;
        }
        let text = tr_n(n, "{n} file failed to import. Click for details.", "{n} files failed to import. Click for details.");
        let g = super::galley(ui, &text, t::font(13.0), t::p().text, 420.0);
        let size = vec2(g.size().x + 60.0, 32.0);
        let r = Rect::from_center_size(top_center + vec2(0.0, size.y / 2.0), size);
        let p = ui.painter();
        p.rect(r, CornerRadius::same(8), t::p().panel, Stroke::new(1.0, t::p().divider), StrokeKind::Inside);
        p.circle_filled(Pos2::new(r.left() + 16.0, r.center().y), 5.0, t::p().icon_red);
        p.galley(Pos2::new(r.left() + 28.0, r.center().y - g.size().y / 2.0), g, t::p().text);
        // The toast first, so the × on top of it takes its own clicks.
        let body = ui.interact(r, ui.scope_id().with("import-errors-toast"), Sense::click());
        let x = Rect::from_center_size(Pos2::new(r.right() - 16.0, r.center().y), vec2(20.0, 20.0));
        let xr = ui.interact(x, ui.scope_id().with("import-errors-clear"), Sense::click());
        if xr.hovered() {
            ui.painter().rect_filled(x, CornerRadius::same(4), t::p().button_hover);
        }
        ui.painter().text(x.center(), Align2::CENTER_CENTER, crate::icons::X, t::font(12.0), t::p().text_muted);
        if xr.clicked() {
            self.imports.confirm_clear = true;
        } else if body.clicked() {
            self.imports.errors_open = true;
        }
    }

    /// The failed-imports panel, after Eagle's error modal: name, source
    /// and reason for each file, with Clear All, Copy to Clipboard, Retry
    /// All and Close.
    fn error_panel(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui::{self, RichText, vec2};
        let errors = self.import_errors();
        if errors.is_empty() {
            self.imports.errors_open = false;
            return;
        }
        let mut remove = None;
        let (mut close, mut retry, mut copy) = (false, false, false);
        let modal = egui::Modal::new(egui::Id::unique("import-errors")).show(ctx, |ui| {
            ui.set_width(680.0);
            ui.label(RichText::new(trf("Failed imports ({n})", &[("n", &errors.len())])).size(15.0));
            ui.add_space(6.0);
            let cols = [190.0, 250.0, 200.0];
            ui.horizontal(|ui| {
                for (w, h) in cols.iter().zip([tr("Name"), tr("Source"), tr("Reason")]) {
                    ui.add_sized(vec2(*w, 18.0), egui::Label::new(RichText::new(h).size(12.0).color(t::p().text_muted)).truncate());
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, e) in errors.iter().enumerate() {
                    let f = &e.failure;
                    ui.horizontal(|ui| {
                        let name = display_name(&f.path);
                        ui.add_sized(vec2(cols[0], 22.0), egui::Label::new(&name).truncate()).on_hover_text(&name);
                        let src = f.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        let link = ui.add_sized(vec2(cols[1], 22.0), egui::Label::new(RichText::new(&src).color(t::p().text_muted)).truncate().sense(egui::Sense::click()));
                        if link.on_hover_text(trf("{path}\nClick to show in Finder", &[("path", &f.path.display())])).clicked() {
                            let _ = std::process::Command::new("open").arg("-R").arg(&f.path).spawn();
                        }
                        ui.add_sized(vec2(cols[2], 22.0), egui::Label::new(f.kind.describe()).truncate()).on_hover_text(&f.detail);
                        if ui.small_button(crate::icons::X).on_hover_text(tr("Remove from the list")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(tr("Clear All")).clicked() {
                    self.imports.confirm_clear = true;
                }
                copy = ui.button(tr("Copy to Clipboard")).on_hover_text(tr("Copies the files, to paste in Finder")).clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(tr("Close")).clicked();
                    retry = ui.add(egui::Button::new(RichText::new(tr("Retry All")).color(eframe::egui::Color32::WHITE)).fill(t::p().primary)).clicked();
                });
            });
        });
        if let Some(i) = remove {
            let mut errors = errors.clone();
            errors.remove(i);
            self.set_import_errors(errors);
        }
        if copy {
            let paths: Vec<PathBuf> = errors.iter().map(|e| e.failure.path.clone()).collect();
            if let Ok(mut clip) = arboard::Clipboard::new()
                && clip.set().file_list(&paths).is_ok()
            {
                self.flash_brief(tr_n(paths.len(), "Copied {n} file", "Copied {n} files"));
            }
        }
        if retry {
            // As in Eagle, the list empties and each file is tried again
            // with the folders and tags it had; failures come back.
            self.set_import_errors(Vec::new());
            self.spawn_import(errors.into_iter().map(|e| Job { path: e.failure.path, opts: e.opts }).collect());
            close = true;
        }
        if close || (modal.should_close() && !self.imports.confirm_clear) {
            self.imports.errors_open = false;
        }
    }

    /// Clear All's confirmation, from the panel or the toast's ×.
    fn clear_errors_confirm(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui::{self, RichText};
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("import-errors-clear")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(RichText::new(tr("Clear the failed imports?")).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("The list can't be brought back. The files themselves are not touched.")).color(t::p().text_muted));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Clear All")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        if let Some(yes) = answer {
            self.imports.confirm_clear = false;
            if yes {
                self.set_import_errors(Vec::new());
                self.imports.errors_open = false;
            }
        }
    }

    /// Most items Eagle selects after an import (`MAX_AUTO_SELECT`).
    const MAX_AUTO_SELECT: usize = 1000;

    /// After a batch, as Eagle's `autoSelectUploadedItems`: with the
    /// preference on, the new items shown in the current view are
    /// selected (not in the viewer or Random, and not more than 1,000),
    /// and a single one is scrolled to. With it off, a single new item is
    /// only scrolled to.
    fn select_imported(&mut self, added: Vec<String>) {
        if self.ws.viewer.is_some() || matches!(self.ws.scope, Scope::Random(_)) {
            return;
        }
        let shown: std::collections::HashSet<&String> = self.ws.order.iter().collect();
        let visible: Vec<String> = added.into_iter().filter(|id| shown.contains(id)).collect();
        if visible.is_empty() {
            return;
        }
        if visible.len() == 1 {
            self.ws.scroll_to = Some(visible[0].clone());
        }
        if self.prefs.auto_select && visible.len() <= Self::MAX_AUTO_SELECT {
            self.ws.anchor = visible.first().cloned();
            self.ws.selected = visible;
        }
    }

    /// The large-folder confirmation and the too-large notice, one at a
    /// time. Drawn before the library is locked for the frame, since
    /// confirming creates folders.
    pub(crate) fn import_dialogs(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui::{self, RichText};
        if let Some(added) = self.imports.select_after.take() {
            self.select_imported(added);
        }
        if self.imports.errors_open {
            self.error_panel(ctx);
        }
        if self.imports.confirm_clear {
            self.clear_errors_confirm(ctx);
        }
        if let Some(p) = self.imports.pending.first() {
            let name = display_name(&p.tree.dir);
            let files = p.files;
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::unique("import-large-folder")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new(tr("Import a large folder?")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(trf("“{name}” holds {files} files. Importing them all may take a while.", &[("name", &name), ("files", &files)])).color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Import")).clicked() {
                        choice = Some(true);
                    }
                    if ui.button(tr("Cancel")).clicked() {
                        choice = Some(false);
                    }
                });
            });
            if modal.should_close() && choice.is_none() {
                choice = Some(false);
            }
            if let Some(go) = choice {
                let p = self.imports.pending.remove(0);
                if go {
                    self.import_trees(vec![p.tree], p.parent);
                }
            }
            return;
        }
        if let Some(name) = self.imports.refused.first().cloned() {
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::unique("import-refused-folder")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new(tr("This folder is too large to import")).size(15.0));
                ui.add_space(4.0);
                let detail = trf(
                    "“{name}” has {depth} or more levels of folders, or {files} or more files. Import its subfolders one at a time instead.",
                    &[("name", &name), ("depth", &folder_import::MAX_DEPTH), ("files", &folder_import::MAX_FILES)],
                );
                ui.label(RichText::new(detail).color(t::p().text_muted));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(tr("Close")).clicked();
                });
            });
            if close || modal.should_close() {
                self.imports.refused.remove(0);
            }
        }
    }

    /// Stages each file without the library lock and commits it under it,
    /// on a background thread, so the window stays responsive. Progress
    /// shows under the toolbar.
    pub(crate) fn spawn_import(&mut self, jobs: Vec<Job>) {
        self.spawn_import_then(jobs, None);
    }

    /// [`App::spawn_import`], then, for files from the auto-import folder
    /// with deleting on (`removal`), each file whose import committed is
    /// moved to the Trash (`auto_import::Removal`).
    fn spawn_import_then(&mut self, jobs: Vec<Job>, removal: Option<crate::auto_import::Removal>) {
        if jobs.is_empty() {
            return;
        }
        let Some(shared) = self.ws.lib.clone() else { return };
        let progress = self.ws.progress.clone();
        {
            let mut pr = progress.lock().unwrap_or_else(|p| p.into_inner());
            if pr.total == 0 {
                pr.started = Some(std::time::Instant::now());
            }
            pr.total += jobs.len();
        }
        let ctx = self.ctx.clone();
        let scratch = tempfile_dir().ok();
        let dupes = self.prefs.asks_duplicates().then(|| self.ws.dupes.clone());
        std::thread::spawn(move || {
            for Job { path: f, opts } in jobs {
                // Paused from the sidebar: wait before the next file (a
                // stop request ends the wait).
                while {
                    let pr = progress.lock().unwrap_or_else(|p| p.into_inner());
                    pr.paused && !pr.cancel
                } {
                    std::thread::sleep(crate::tasks::PAUSE_POLL);
                }
                if progress.lock().unwrap_or_else(|p| p.into_inner()).cancel {
                    // Stopped: skip the file, but still clear a pasted copy.
                    if scratch.as_ref().is_some_and(|d| f.starts_with(d)) {
                        let _ = std::fs::remove_file(&f);
                    }
                    progress.lock().unwrap_or_else(|p| p.into_inner()).done += 1;
                    continue;
                }
                // A file already in the library is held for the duplicate
                // dialog (`import_dupes`) instead of being committed.
                let result = crate::import_dupes::import_one(&f, &opts, &shared, dupes.as_ref());
                if let Some(removal) = &removal {
                    // The item's original as the library now has it; `None`
                    // when the import failed or the file is held for the
                    // duplicate dialog, which still needs it.
                    let committed = match &result {
                        Ok(Some(id)) => {
                            let lib = lock(&shared);
                            lib.item(id).map(|item| item.original_path(lib.root()))
                        }
                        _ => None,
                    };
                    if removal.finish(&f, committed.as_deref(), crate::auto_import::trash) == crate::auto_import::After::TrashFailed {
                        crate::errlog::error(&format!("auto-import: couldn't move {} to the Trash", f.display()));
                    }
                }
                // Pasted images are staged in falcon's own temp folder; they
                // are removed only now, after the import has read them.
                if scratch.as_ref().is_some_and(|d| f.starts_with(d)) {
                    let _ = std::fs::remove_file(&f);
                }
                let mut pr = progress.lock().unwrap_or_else(|p| p.into_inner());
                pr.done += 1;
                // `Ok(None)` is a duplicate held for the dialog, not added.
                if let Ok(Some(id)) = &result {
                    pr.added.push(id.clone());
                }
                if let Err(e) = &result {
                    pr.failed += 1;
                    pr.errors.push(ImportError { failure: Failure::new(&f, e), opts: opts.clone() });
                }
                pr.current = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                drop(pr);
                ctx.request_repaint();
            }
        });
    }

    /// Keeps the auto-import watcher on the shown library's folder (or
    /// stops it when auto-import is off or no library is open) and imports
    /// what it found, as Eagle's auto-import does: into the library's root,
    /// with no folders or tags, through the usual import so the duplicate
    /// check and the failed-imports list apply. With "Delete files after
    /// importing them" on, each file is moved to the Trash once its import
    /// committed. A folder that can't be watched (gone, or on a network
    /// drive) is reported once.
    pub(crate) fn auto_import_tick(&mut self) {
        let want = match (&self.ws.lib, &self.ws.root) {
            (Some(_), Some(root)) => self.prefs.auto_import_dir(root),
            _ => None,
        };
        if self.auto_import.watcher.as_ref().map(|w| &w.dir) != want.as_ref() && self.auto_import.refused != want {
            self.auto_import.watcher = None;
            if let Some(dir) = want {
                let chosen = self.prefs.auto_import_path.clone().unwrap_or_default();
                match crate::prefs::auto_import_refusal(&chosen) {
                    Some(why) => {
                        self.flash(trf("Auto-import is paused. {reason}", &[("reason", &why)]));
                        self.auto_import.refused = Some(dir);
                    }
                    None => {
                        let ctx = self.ctx.clone();
                        self.auto_import.watcher = crate::auto_import::Watcher::start(dir, self.auto_import.imported.clone(), move || ctx.request_repaint());
                    }
                }
            }
        }
        let batches = self.auto_import.watcher.as_ref().map(|w| w.take()).unwrap_or_default();
        for files in batches {
            if self.prefs.tells_auto_import() {
                let n = files.len();
                self.flash(tr_n(n, "Importing {n} file from the auto-import folder", "Importing {n} files from the auto-import folder"));
            }
            let opts = self.prefs.import_options(Vec::new());
            let removal = self.auto_import.watcher.as_ref().filter(|_| self.prefs.auto_import_delete).map(|w| crate::auto_import::Removal { watched: w.dir.clone(), imported: self.auto_import.imported.clone() });
            self.spawn_import_then(files.into_iter().map(|path| Job { path, opts: opts.clone() }).collect(), removal);
        }
    }
}

pub(crate) fn is_hidden(p: &std::path::Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// A private folder for clipboard images before they are imported.
pub(crate) fn tempfile_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("falcon-paste");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Splits the `.eaglepack` files off `paths`.
fn split_packs(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    paths.into_iter().partition(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("eaglepack")))
}

impl App {
    /// Imports files and folders on a background thread: each file is
    /// staged without the library lock, then committed under it, so the
    /// window stays responsive. Progress shows under the toolbar.
    pub(crate) fn import(&mut self, paths: Vec<PathBuf>) {
        let folders = self.scope_folders();
        self.import_into(paths, folders);
    }

    /// The folder new items join: the one being viewed, if any.
    pub(crate) fn scope_folders(&self) -> Vec<String> {
        match &self.ws.scope {
            Scope::Folder(id) => vec![id.clone()],
            _ => Vec::new(),
        }
    }

    /// Imports `paths` into the shown library. A `*.library` folder among
    /// them is opened instead, never imported (only the first, if several).
    pub(crate) fn import_into(&mut self, paths: Vec<PathBuf>, folders: Vec<String>) {
        let (libraries, paths) = logic::split_libraries(paths);
        // A dropped or opened .eaglepack is imported as a package, not as
        // a file (the first one; Eagle takes one at a time).
        let (packs, paths) = split_packs(paths);
        if let Some(first) = packs.into_iter().next() {
            self.preview_pack_import(first, folders.first().cloned());
        }
        self.import_files(paths, folders);
        if let Some(first) = libraries.first() {
            // Behind the loading screen, as the switcher's opens are.
            self.open_in_background(first.clone());
            let others = libraries.len() - 1;
            if others > 0 {
                let name = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                self.flash(tr_nf(
                    others,
                    "Opening {name}; {n} other library was left closed (one opens at a time)",
                    "Opening {name}; {n} other libraries were left closed (one opens at a time)",
                    &[("name", &name)],
                ));
            }
        }
    }

    pub(crate) fn import_files(&mut self, paths: Vec<PathBuf>, folders: Vec<String>) {
        let extra = self.collect_extra.clone();
        if paths.is_empty() {
            return;
        }
        if self.ws.lib.is_none() {
            return;
        }
        let mut files = Vec::new();
        let mut stack = paths;
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                // Flattened, as Eagle's grid drop does. The walk skips
                // junk and won't follow a folder link into a loop.
                if !falcon_library::folder_import::is_pixave_library(&p) {
                    files.extend(falcon_library::folder_import::walk(&p).all_files());
                }
            } else if !is_hidden(&p) {
                files.push(p);
            }
        }
        if files.is_empty() {
            return;
        }
        files.sort();
        let mut opts = self.prefs.import_options(folders);
        if let Some(meta) = &extra {
            meta.apply(&mut opts, files.len());
        }
        self.spawn_import(files.into_iter().map(|path| Job { path, opts: opts.clone() }).collect());
    }

    /// ⌘C: puts the selected originals on the clipboard as files.
    pub(crate) fn copy_selection(&mut self) {
        let ids = self.ws.selected.clone();
        self.copy_items(&ids);
    }

    /// Puts the originals of `ids` on the clipboard as files; one jpg, png or
    /// bmp also as its pixels (`clip_image`).
    pub(crate) fn copy_items(&mut self, ids: &[String]) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let paths: Vec<PathBuf> = {
            let lib = lock(&shared);
            ids.iter().filter_map(|id| lib.item(id)).map(|i| i.original_path(lib.root())).collect()
        };
        // One jpg, png or bmp goes as pixels and a file reference together.
        // Never for an original still in iCloud: copying the pixels reads it.
        let pixels = match paths.as_slice() {
            [p] => {
                let kind = p.extension().and_then(|e| e.to_str()).and_then(crate::clip_image::pixel_type);
                kind.is_some() && p.is_file() && !crate::icloud::is_dataless(p) && crate::clip_image::copy(p).is_ok()
            }
            _ => false,
        };
        if pixels
            || arboard::Clipboard::new().is_ok_and(|mut clip| clip.set().file_list(&paths).is_ok())
        {
            self.used(&paths);
            self.flash_brief(tr_n(paths.len(), "Copied {n} file", "Copied {n} files"));
        }
    }

    /// ⌘V in the grid: imports copied files, or a copied image as a PNG.
    pub(crate) fn paste(&mut self) {
        let folders = self.scope_folders();
        self.paste_into(folders);
    }

    pub(crate) fn paste_into(&mut self, folders: Vec<String>) {
        let Ok(mut clip) = arboard::Clipboard::new() else { return };
        if let Ok(files) = clip.get().file_list()
            && !files.is_empty()
        {
            self.import_into(files, folders);
            return;
        }
        if let Ok(img) = clip.get_image() {
            let Some(buf) = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()) else { return };
            let Ok(dir) = tempfile_dir() else { return };
            let stamp = chrono::Local::now().format("%Y-%m-%d at %H.%M.%S");
            let path = dir.join(format!("Pasted image {stamp}.png"));
            // The import thread deletes the file once it has copied it in.
            if buf.save(&path).is_ok() {
                self.import_into(vec![path], folders);
            }
        }
    }
}
