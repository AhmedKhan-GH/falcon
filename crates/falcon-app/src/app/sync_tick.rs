//! The app side of background folder synchronization: starts and stops the
//! scheduler with the library, keeps the watchers of automatic folders in
//! line with the library's list, hands finished passes to the thumbnail
//! worker and the toast, and carries out what Preferences asks.
//!
//! Nothing here touches the disk or holds the library lock for more than a
//! read of the folder list; the passes run in `sync_worker.rs`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use falcon_library::SyncMode;

use super::App;
use crate::locale::{tr, trf};
use crate::prefs::{SyncCommand, SyncRow};
use crate::sync_worker::{Finished, SyncScheduler, SyncWatchers};
use crate::thumb_worker::ThumbPriority;

/// How often the library's folder list is read again.
const REREAD: Duration = Duration::from_secs(2);

impl App {
    /// Called every frame.
    pub(crate) fn sync_tick(&mut self, ctx: &egui::Context) {
        let (Some(shared), Some(root)) = (self.ws.lib.clone(), self.ws.root.clone()) else {
            self.ws.sync = None;
            self.ws.sync_watchers = SyncWatchers::default();
            return;
        };
        // A library switched or moved under the workspace: the old scheduler
        // is stopped (its worker joined) before a new one is made.
        if self.ws.sync.as_ref().is_none_or(|s| s.root() != root) {
            self.ws.sync_watchers = SyncWatchers::default();
            self.ws.sync = None;
            let ctx = ctx.clone();
            self.ws.sync = Some(Arc::new(SyncScheduler::new(Arc::clone(&shared), root, move || ctx.request_repaint())));
            self.ws.sync_read = None;
        }
        let Some(sched) = self.ws.sync.clone() else { return };

        for done in sched.take_finished() {
            self.sync_finished(done);
        }

        if self.ws.sync_read.is_none_or(|t| t.elapsed() >= REREAD) {
            self.ws.sync_read = Some(Instant::now());
            self.ws.sync_folders = crate::app::lock(&shared).watched_folders();
            let wanted: Vec<_> =
                self.ws.sync_folders.iter().filter(|w| w.sync_mode == SyncMode::Auto).map(|w| (w.id.clone(), w.source_path.clone())).collect();
            self.ws.sync_watchers.reconcile(&sched, &wanted);
            // Pausing a folder also stops a pass already running over it.
            for wf in self.ws.sync_folders.iter().filter(|w| w.sync_mode == SyncMode::Disabled) {
                if sched.is_active(&wf.id) {
                    sched.cancel(&wf.id);
                }
            }
        }

        for command in std::mem::take(&mut self.prefs_window.sync_commands) {
            match command {
                SyncCommand::Now(id) => self.sync_folder_now(&id),
                SyncCommand::Stop(id) => sched.cancel(&id),
                SyncCommand::Remove(id) => {
                    sched.cancel(&id);
                    let _ = crate::app::lock(&shared).remove_watched_folder(&id);
                    self.ws.sync_read = None;
                }
            }
        }

        if self.prefs_window.open {
            let running = sched.progress();
            self.prefs_window.sync_rows = self
                .ws
                .sync_folders
                .iter()
                .map(|wf| SyncRow {
                    wf: wf.clone(),
                    running: running.as_ref().filter(|(id, _)| *id == wf.id).map(|(_, p)| (p.done, p.total)),
                    active: sched.is_active(&wf.id),
                })
                .collect();
        }
        // The next re-read, and progress while a pass runs.
        let waiting = !self.ws.sync_folders.is_empty();
        if sched.progress().is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        } else if waiting {
            ctx.request_repaint_after(REREAD);
        }
    }

    /// Asks for a pass over a watched folder; it runs in the background.
    pub(crate) fn sync_folder_now(&mut self, wf_id: &str) {
        let Some(sched) = self.ws.sync.clone() else { return };
        self.ws.sync_manual.insert(wf_id.to_string());
        sched.request(wf_id);
        self.ws.sync_read = None;
    }

    fn sync_finished(&mut self, done: Finished) {
        let manual = self.ws.sync_manual.remove(&done.wf_id);
        match done.result {
            Ok(stats) => {
                if !stats.pending_thumbnail_ids.is_empty()
                    && let Some(worker) = &self.ws.thumb_worker
                {
                    worker.enqueue_batch(&stats.pending_thumbnail_ids, ThumbPriority::Normal);
                }
                if stats.added + stats.modified > 0 {
                    self.refresh = true;
                }
                if stats.offline {
                    if manual {
                        self.flash_error(tr("Folder sync is offline: source folder unreachable"));
                    }
                } else if manual || stats.added + stats.modified + stats.conflicts > 0 {
                    let (n, m) = (stats.added, stats.modified);
                    if stats.conflicts > 0 {
                        let c = stats.conflicts;
                        self.flash(trf(
                            "Folder synced: {n} added, {m} modified, {c} changed in the source and left as they were",
                            &[("n", &n), ("m", &m), ("c", &c)],
                        ));
                    } else if n == 0 && m == 0 {
                        self.flash_brief(tr("Folder is up to date"));
                    } else {
                        self.flash(trf("Folder synced: {n} added, {m} modified", &[("n", &n), ("m", &m)]));
                    }
                }
            }
            Err(e) => self.flash_error(trf("Folder sync failed: {error}", &[("error", &e)])),
        }
        // A library whose folders changed on disk is read again.
        self.ws.sync_read = None;
    }
}
