//! Moving a library folder to another location (such as an external SSD),
//! preserving its recent entry, layout settings, folder emoji, recently-used
//! items, Actions, app-data backup and AI Search index.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::app::{App, lock};
use crate::library_move::{
    MoveOutcome, Refusal, Stats, check_destination, execute_move, measure_library, migrate_layouts,
    migrate_sidecar_file, replace_recent_path,
};
use crate::locale::{tr, trf};
use crate::theme as t;
use crate::ui::dialogs::force_close::tasks_running;
use crate::ui::dialogs::library::risky_library_folder;
use crate::ui::dialogs::library_notices::Alert;

pub(crate) enum Size {
    Calculating,
    Known(Stats),
    Unknown,
}

pub(crate) struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub(crate) enum State {
    #[default]
    Idle,
    Confirm {
        src: PathBuf,
        dest: PathBuf,
        name: String,
        size: Size,
        rx: Receiver<io::Result<Stats>>,
        _stop: StopOnDrop,
        ntfs: bool,
        risky: bool,
    },
    Moving {
        src: PathBuf,
        dest: PathBuf,
        name: String,
        total_bytes: u64,
        total_files: usize,
        done_bytes: Arc<AtomicU64>,
        done_files: Arc<AtomicUsize>,
        current: Arc<Mutex<String>>,
        cancel: Arc<AtomicBool>,
        rx: Receiver<Result<MoveOutcome, String>>,
        old_actions: Option<PathBuf>,
        was_active: bool,
    },
}

impl State {
    pub(crate) fn is_open(&self) -> bool {
        !matches!(self, State::Idle)
    }
}

fn library_name(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

fn refusal_text(r: &Refusal) -> String {
    match r {
        Refusal::Missing => tr("That library folder could not be found.").to_string(),
        Refusal::Symlink => tr("Falcon won't move a library through a symbolic link.").to_string(),
        Refusal::NotLibrary => tr("That folder doesn't look like an Eagle library.").to_string(),
        Refusal::DestMissing => tr("The chosen destination folder doesn't exist.").to_string(),
        Refusal::SameFolder => tr("The library is already in that folder.").to_string(),
        Refusal::InsideSelf => tr("A library can't be moved inside itself.").to_string(),
        Refusal::AlreadyExists => tr("A folder with that name already exists in the chosen location.").to_string(),
        Refusal::InstallDir(why) => why.clone(),
        Refusal::OpenConflict => tr("Another open library conflicts with that path.").to_string(),
    }
}

impl App {
    /// Asks where to move `src`, validates the destination, counts the
    /// library's size in the background and opens the confirmation dialog.
    pub(crate) fn start_move_library(&mut self, src: PathBuf) {
        if self.move_library.is_open()
            || self.merge.is_some()
            || self.operation_running()
            || self.loading.is_some()
            || self.remove_library.is_some()
        {
            return;
        }
        let name = library_name(&src);
        let running = self.position_of(&src).is_some_and(|pos| self.others.iter(&self.ws).nth(pos).is_some_and(tasks_running));
        if running {
            self.show_alert(Alert::new(
                tr("Can't move the library"),
                trf("Finish or stop running tasks in {name} before moving it.", &[("name", &name)]),
            ));
            return;
        }
        let title = trf("Choose where to move “{name}”", &[("name", &name)]);
        let Some(dest_parent) = self.library_folder_dialog(&title).pick_folder() else {
            return;
        };
        let mut other_open: Vec<PathBuf> = self
            .others
            .iter(&self.ws)
            .filter_map(|w| w.root.clone())
            .filter(|p| !crate::app::library::same_path(p, &src))
            .collect();
        if let Some(l) = &self.loading
            && !crate::app::library::same_path(&l.path, &src)
        {
            other_open.push(l.path.clone());
        }
        let plan = match check_destination(&src, &dest_parent, &other_open) {
            Ok(plan) => plan,
            Err(r) => {
                self.show_alert(Alert::new(tr("Can't move the library"), refusal_text(&r)));
                return;
            }
        };
        let ntfs = crate::storage::is_ntfs_volume(&dest_parent);
        let risky = risky_library_folder(&dest_parent);
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (path, flag, ctx) = (plan.src.clone(), stop.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(measure_library(&path, &flag));
            ctx.request_repaint();
        });
        self.move_library = State::Confirm {
            src: plan.src,
            dest: plan.dest,
            name: plan.name,
            size: Size::Calculating,
            rx,
            _stop: StopOnDrop(stop),
            ntfs,
            risky,
        };
    }

    /// Draws the move confirmation or progress modal. Called once a frame.
    pub(crate) fn move_library_dialog(&mut self, ctx: &egui::Context) {
        let state = std::mem::take(&mut self.move_library);
        self.move_library = match state {
            State::Idle => State::Idle,
            State::Confirm { src, dest, name, mut size, rx, _stop, ntfs, risky } => {
                match rx.try_recv() {
                    Ok(Ok(stats)) => size = Size::Known(stats),
                    Ok(Err(_)) => size = Size::Unknown,
                    Err(TryRecvError::Disconnected) if matches!(size, Size::Calculating) => size = Size::Unknown,
                    Err(_) => {}
                }
                let size_line = match size {
                    Size::Calculating => tr("Calculating…").to_string(),
                    Size::Known(s) => crate::logic::format_size(s.bytes),
                    Size::Unknown => tr("Size unknown").to_string(),
                };
                let ready = !matches!(size, Size::Calculating);
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::unique("move-library-confirm")).show(ctx, |ui| {
                    ui.set_width(400.0);
                    ui.label(RichText::new(trf("Move “{name}”?", &[("name", &name)])).size(15.0));
                    ui.add_space(6.0);
                    ui.label(RichText::new(trf("From: {path}", &[("path", &crate::workspaces::tilde(&src))])).color(t::p().text_muted));
                    ui.label(RichText::new(trf("To: {path}", &[("path", &crate::workspaces::tilde(&dest))])).color(t::p().text_muted));
                    ui.label(RichText::new(trf("Size: {size}", &[("size", &size_line)])).color(t::p().text_muted));
                    if ntfs {
                        ui.add_space(6.0);
                        ui.label(RichText::new(tr("The destination drive is formatted as NTFS. NTFS drivers on macOS are slow and less reliable; APFS or exFAT works better.")).color(t::p().icon_red));
                    }
                    if risky {
                        ui.add_space(6.0);
                        ui.label(RichText::new(tr("The destination is in Desktop or Downloads, where files are often cleared out by hand or by the system.")).color(t::p().icon_red));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("All files, thumbnails and metadata will be moved to the new location. On cross-drive moves, the original folder goes to the Trash once copying finishes.")).color(t::p().text_muted));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(ready, egui::Button::new(RichText::new(tr("Move Library")).color(egui::Color32::WHITE)).fill(t::p().primary)).clicked() {
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
                if !ready {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                match answer {
                    Some(true) => {
                        let stats = match size {
                            Size::Known(s) => s,
                            _ => Stats::default(),
                        };
                        if let Some(worker) = self.ws.thumb_worker.take() {
                            worker.shutdown();
                        }
                        self.begin_move(src, dest, name, stats)
                    }
                    Some(false) => State::Idle,
                    None => State::Confirm { src, dest, name, size, rx, _stop, ntfs, risky },
                }
            }
            State::Moving { src, dest, name, total_bytes, total_files, done_bytes, done_files, current, cancel, rx, old_actions, was_active } => {
                match rx.try_recv() {
                    Ok(result) => {
                        self.finish_move(src, dest, name, old_actions, was_active, result);
                        State::Idle
                    }
                    Err(TryRecvError::Disconnected) => {
                        self.finish_move(src, dest, name, old_actions, was_active, Err(tr("Move worker stopped unexpectedly.").to_string()));
                        State::Idle
                    }
                    Err(TryRecvError::Empty) => {
                        let bytes = done_bytes.load(Ordering::Relaxed);
                        let files = done_files.load(Ordering::Relaxed);
                        let frac = if total_bytes > 0 {
                            (bytes as f32 / total_bytes as f32).clamp(0.0, 1.0)
                        } else if total_files > 0 {
                            (files as f32 / total_files as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let label = if total_bytes > 0 {
                            format!("{} / {}", crate::logic::format_size(bytes), crate::logic::format_size(total_bytes))
                        } else {
                            trf("{done} of {total}", &[("done", &files), ("total", &total_files)])
                        };
                        let cur = current.lock().unwrap_or_else(|p| p.into_inner()).clone();
                        egui::Modal::new(egui::Id::unique("move-library-progress")).show(ctx, |ui| {
                            ui.set_width(360.0);
                            ui.label(RichText::new(trf("Moving “{name}”", &[("name", &name)])).size(15.0));
                            ui.add_space(6.0);
                            let bar = if total_bytes == 0 && total_files == 0 {
                                egui::ProgressBar::new(0.0).animate(true)
                            } else {
                                egui::ProgressBar::new(frac).text(label)
                            };
                            ui.add(bar);
                            if !cur.is_empty() {
                                ui.add_space(4.0);
                                ui.label(RichText::new(cur).size(12.0).color(t::p().text_muted));
                            }
                            ui.add_space(8.0);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(!cancel.load(Ordering::Relaxed), egui::Button::new(tr("Cancel"))).clicked() {
                                    cancel.store(true, Ordering::Relaxed);
                                }
                            });
                        });
                        ctx.request_repaint_after(std::time::Duration::from_millis(100));
                        State::Moving { src, dest, name, total_bytes, total_files, done_bytes, done_files, current, cancel, rx, old_actions, was_active }
                    }
                }
            }
        };
    }

    fn begin_move(&mut self, src: PathBuf, dest: PathBuf, name: String, stats: Stats) -> State {
        let old_actions = crate::actions::store_dir().map(|d| falcon_library::actions::store_path(&d, &src));
        let pos = self.position_of(&src);
        let was_active = pos == Some(self.others.slot());

        if let Some(ws) = self.workspace_for_mut(&src)
            && let Err(e) = ws.flush_and_backup()
        {
            self.show_alert(Alert::new(tr("Could not move the library"), trf("Could not save pending changes: {error}", &[("error", &e)])));
            return State::Idle;
        }
        if let Some(ws) = self.workspace_for_mut(&src) {
            ws.watch = None;
        }
        if was_active {
            self.audio.stop();
            self.video.keep_only(None);
            self.pdf.keep_only(None);
            self.hover_media.stop_all();
            self.preview.clear();
            self.ws.viewer = None;
            self.server = None;
            self.api_check.cancel();
            self.mcp = Default::default();
            if let Some(engine) = &self.ai.engine {
                engine.set_library(None);
            }
        }

        let done_bytes = Arc::new(AtomicU64::new(0));
        let done_files = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(Mutex::new(String::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();

        let (s, d, db, df, cur, c, ctx) = (
            src.clone(),
            dest.clone(),
            done_bytes.clone(),
            done_files.clone(),
            current.clone(),
            cancel.clone(),
            self.ctx.clone(),
        );
        std::thread::spawn(move || {
            let res = execute_move(
                &s,
                &d,
                &c,
                |files, bytes, file| {
                    df.store(files, Ordering::Relaxed);
                    db.store(bytes, Ordering::Relaxed);
                    *cur.lock().unwrap_or_else(|p| p.into_inner()) = file.to_string();
                    ctx.request_repaint();
                },
                crate::auto_import::trash,
            );
            let _ = tx.send(res);
            ctx.request_repaint();
        });

        State::Moving {
            src,
            dest,
            name,
            total_bytes: stats.bytes,
            total_files: stats.files,
            done_bytes,
            done_files,
            current,
            cancel,
            rx,
            old_actions,
            was_active,
        }
    }

    fn finish_move(
        &mut self,
        src: PathBuf,
        dest: PathBuf,
        name: String,
        old_actions: Option<PathBuf>,
        was_active: bool,
        result: Result<MoveOutcome, String>,
    ) {
        match result {
            Ok(outcome) => {
                let ctx = self.ctx.clone();
                if let Some(ws) = self.workspace_for_mut(&src) {
                    if let Some(shared) = ws.lib.clone() {
                        lock(&shared).set_root(dest.clone());
                    }
                    ws.root = Some(dest.clone());
                    if ws.undo_root.is_some() {
                        ws.undo_root = Some(dest.clone());
                    }
                    let c1 = ctx.clone();
                    ws.watch = Some(crate::watch::Watcher::start(dest.clone(), move || c1.request_repaint()));
                    let c2 = ctx.clone();
                    ws.health = Some(crate::storage::spawn_check(dest.clone(), move || c2.request_repaint()));
                }
                self.migrate_library_references(&src, &dest, old_actions.as_deref());
                if self.missing.as_deref().is_some_and(|m| crate::app::library::same_path(m, &src)) {
                    self.missing = None;
                }
                if was_active {
                    self.user_actions.follow(self.ws.root.as_deref());
                    self.folder_emoji.store.set_library(self.ws.root.as_deref());
                    self.serve_active();
                }
                if let Some(err) = outcome.trash_error {
                    self.show_alert(Alert::new(
                        tr("Library moved"),
                        trf(
                            "Moved “{name}” to {dest}, but couldn't move the original folder to the Trash ({error}). You can remove the old folder manually.",
                            &[("name", &name), ("dest", &crate::workspaces::tilde(&dest)), ("error", &err)],
                        ),
                    ));
                } else {
                    self.flash(trf("Moved “{name}” to {dest}", &[("name", &name), ("dest", &crate::workspaces::tilde(&dest))]));
                }
            }
            Err(msg) => {
                let ctx = self.ctx.clone();
                if let Some(ws) = self.workspace_for_mut(&src)
                    && ws.watch.is_none()
                    && src.is_dir()
                {
                    ws.watch = Some(crate::watch::Watcher::start(src.clone(), move || ctx.request_repaint()));
                }
                if was_active {
                    self.serve_active();
                }
                if msg == "cancelled" {
                    self.flash(tr("Move cancelled"));
                } else {
                    self.show_alert(Alert::new(tr("Could not move the library"), msg));
                }
            }
        }
    }

    /// Migrates every path-keyed piece of app data and state from `old` to
    /// `new` (`recent`, `layouts`, `recent_items`, `folder_emoji`, `actions`,
    /// `eagle-backup` and the AI Search index).
    pub(crate) fn migrate_library_references(&mut self, old: &Path, new: &Path, old_actions: Option<&Path>) {
        replace_recent_path(&mut self.recent, old, new);
        migrate_layouts(&mut self.layouts, old, new);
        self.recent_items.rename_library(old, new);
        self.folder_emoji.store.rename_library(old, new);
        self.lib_icons.forget(old);

        let new_actions = crate::actions::store_dir().map(|d| falcon_library::actions::store_path(&d, new));
        let computed_old = crate::actions::store_dir().map(|d| falcon_library::actions::store_path(&d, old));
        migrate_sidecar_file(old_actions.or(computed_old.as_deref()), new_actions.as_deref());

        let old_backup = crate::backups::app_backup_dir().map(|d| d.join(falcon_library::backup::app_data_backup_name(old)));
        let new_backup = crate::backups::app_backup_dir().map(|d| d.join(falcon_library::backup::app_data_backup_name(new)));
        migrate_sidecar_file(old_backup.as_deref(), new_backup.as_deref());

        if let Some(engine) = &self.ai.engine {
            engine.relocate_index(old.to_path_buf(), new.to_path_buf());
        }
    }

    fn workspace_for_mut(&mut self, root: &Path) -> Option<&mut crate::app::Workspace> {
        if self.ws.root.as_deref().is_some_and(|r| crate::app::library::same_path(r, root)) {
            return Some(&mut self.ws);
        }
        self.others.parked_mut().find(|w| w.root.as_deref().is_some_and(|r| crate::app::library::same_path(r, root)))
    }
}
