//! Import entry points beyond a plain drop on the grid: folders dropped on
//! the sidebar (their tree recreated as Eagle folders), files dropped on a
//! sidebar folder, and the background import every path ends in.

use std::path::PathBuf;

use eframe::egui::{Pos2, Rect};
use falcon_library::folder_import;
use falcon_library::import::ImportOptions;

use super::{App, lock, tempfile_dir};
use crate::drop_pos::{self, Target};
use crate::logic::{self, Scope};

/// One file to import and the options to import it with.
pub(super) struct Job {
    pub path: PathBuf,
    pub opts: ImportOptions,
}

/// Import state that belongs to the window rather than a library.
#[derive(Default)]
pub(super) struct Imports {
    /// The sidebar's rect last frame (None while hidden), and its visible
    /// folder rows, to tell where a drop from Finder landed.
    pub sidebar: Option<Rect>,
    pub folder_rows: Vec<(Rect, String)>,
    /// A file dialog to show once the frame is done and the library lock
    /// is released.
    pub request: Option<Request>,
}

/// An import file dialog, asked for during the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Request {
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

    /// Under "No items" in All or an empty folder, an Import Folders…
    /// button, as on Eagle's empty views: from All the folders go to the
    /// top level, from a folder inside it.
    pub(super) fn empty_import_button(&mut self, ui: &mut eframe::egui::Ui, area: Rect) {
        let parent = match &self.ws.scope {
            Scope::All => None,
            Scope::Folder(id) => Some(id.clone()),
            _ => return,
        };
        if !self.ws.query.trim().is_empty() {
            return;
        }
        let at = Rect::from_center_size(area.center() + eframe::egui::vec2(0.0, 34.0), eframe::egui::vec2(160.0, 26.0));
        if ui.put(at, eframe::egui::Button::new("Import Folders…")).clicked() {
            self.imports.request = Some(Request::Folders { parent });
        }
    }

    /// Shows the dialog asked for during the frame, if any, and starts the
    /// import. Called with the library lock released.
    pub(super) fn run_import_request(&mut self) {
        let Some(request) = self.imports.request.take() else { return };
        if self.ws.lib.is_none() {
            return;
        }
        match request {
            Request::Files { folders } => {
                if let Some(files) = rfd::FileDialog::new().set_title("Import Files").pick_files() {
                    self.import_into(files, folders);
                }
            }
            Request::Folders { parent } => {
                let Some(picked) = rfd::FileDialog::new().set_title("Import Folders").pick_folders() else { return };
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
    pub(super) fn drop_files(&mut self, paths: Vec<PathBuf>, at: Option<Pos2>) {
        let target = drop_pos::target(at, self.show_sidebar.then_some(self.imports.sidebar).flatten(), &self.imports.folder_rows);
        match target {
            Target::Grid => self.import(paths),
            Target::Folder(id) => {
                let (libraries, rest) = logic::split_libraries(paths);
                let files: Vec<PathBuf> = rest.into_iter().filter(|p| !p.is_dir()).collect();
                self.import_into(libraries, Vec::new());
                self.import_files(files, vec![id]);
            }
            Target::Sidebar => {
                let (libraries, rest) = logic::split_libraries(paths);
                let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) = rest.into_iter().partition(|p| p.is_dir());
                self.import_into(libraries, Vec::new());
                if dirs.is_empty() && !files.is_empty() {
                    self.flash("Drop folders on the sidebar, or files on the grid or a folder");
                }
                self.import_folder_trees(dirs, None);
            }
        }
    }

    /// Imports each directory as an Eagle folder of the same name inside
    /// `parent` (or at the top level), with its subfolders recreated and
    /// every file in the folder that mirrors its own, as Eagle's
    /// `uploadFolderToSidebar` does. The last new folder is opened.
    pub(super) fn import_folder_trees(&mut self, mut dirs: Vec<PathBuf>, parent: Option<String>) {
        if dirs.is_empty() {
            return;
        }
        let Some(shared) = self.ws.lib.clone() else { return };
        dirs.sort_by(|a, b| falcon_library::search::natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
        let mut jobs = Vec::new();
        let mut opened = None;
        for dir in dirs {
            let tree = folder_import::walk(&dir);
            let plan = folder_import::plan(&tree, parent.as_deref(), falcon_library::ids::new_id);
            if let Err(e) = lock(&shared).create_folders(&plan.folders) {
                self.flash(format!("Couldn't import {}: {e}", dir.display()));
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

    /// Stages each file without the library lock and commits it under it,
    /// on a background thread, so the window stays responsive. Progress
    /// shows under the toolbar.
    pub(super) fn spawn_import(&mut self, jobs: Vec<Job>) {
        if jobs.is_empty() {
            return;
        }
        let Some(shared) = self.ws.lib.clone() else { return };
        let progress = self.ws.progress.clone();
        {
            let mut pr = progress.lock().unwrap_or_else(|p| p.into_inner());
            pr.total += jobs.len();
        }
        let ctx = self.ctx.clone();
        let scratch = tempfile_dir().ok();
        let dupes = self.prefs.notify_duplicates.then(|| self.ws.dupes.clone());
        std::thread::spawn(move || {
            for Job { path: f, opts } in jobs {
                // A file already in the library is held for the duplicate
                // dialog (`import_dupes`) instead of being committed.
                let result = crate::import_dupes::import_one(&f, &opts, &shared, dupes.as_ref());
                // Pasted images are staged in falcon's own temp folder; they
                // are removed only now, after the import has read them.
                if scratch.as_ref().is_some_and(|d| f.starts_with(d)) {
                    let _ = std::fs::remove_file(&f);
                }
                let mut pr = progress.lock().unwrap_or_else(|p| p.into_inner());
                pr.done += 1;
                if result.is_err() {
                    pr.failed += 1;
                }
                pr.current = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                drop(pr);
                ctx.request_repaint();
            }
        });
    }
}
