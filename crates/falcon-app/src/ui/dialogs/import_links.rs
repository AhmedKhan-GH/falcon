//! File ▸ Import Links…: a box to paste addresses into, one per line.
//! Each is fetched on a background thread and becomes an item: an image
//! link is downloaded, anything else is saved as a `.url` bookmark, all
//! going into the open folder through the usual import queue.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::app::App;
use crate::app::import::{Job, tempfile_dir};
use crate::link_import::{self, Resolved};
use crate::locale::{tr, tr_n};
use crate::theme as t;

/// Addresses fetched at the same time.
const WORKERS: usize = 4;
/// Invalid lines listed under the box before "…".
const SHOWN_BAD: usize = 8;

/// The box and the fetch in progress.
#[derive(Default)]
pub(crate) struct LinkImport {
    pub open: bool,
    text: String,
    /// The lines the last attempt refused; empty with `refused` set means
    /// the box was blank.
    bad: Vec<String>,
    refused: bool,
    focus: bool,
    /// Folders the addresses in flight go into, and the files arriving.
    folders: Vec<String>,
    rx: Option<Receiver<Option<Resolved>>>,
    expected: usize,
}

impl App {
    /// Opens the box, filled from the clipboard when it holds addresses.
    pub(crate) fn import_links_dialog(&mut self) {
        if self.ws.lib.is_none() {
            return;
        }
        let clip = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
        let l = &mut self.link_import;
        *l = LinkImport { open: true, text: link_import::prefill(&clip), focus: true, rx: l.rx.take(), expected: l.expected, folders: std::mem::take(&mut l.folders), ..Default::default() };
    }

    /// Draws the box and takes in the files the fetch has finished.
    pub(crate) fn import_links_ui(&mut self, ctx: &egui::Context) {
        self.collect_fetched();
        if !self.link_import.open {
            return;
        }
        let l = &mut self.link_import;
        let (mut go, mut cancel) = (false, false);
        let modal = egui::Modal::new(egui::Id::unique("import-links")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(RichText::new(tr("Batch Import Links")).size(15.0));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Paste links, one per line. Images are imported as files and web pages are saved as bookmarks.")).color(t::p().text_muted));
            ui.add_space(8.0);
            let edit = egui::TextEdit::multiline(&mut l.text).desired_rows(7).desired_width(f32::INFINITY).hint_text("https://");
            let r = ui.add(edit);
            if std::mem::take(&mut l.focus) {
                r.request_focus();
            }
            if l.refused {
                ui.add_space(4.0);
                let mut msg = tr("Invalid link format").to_string();
                for b in l.bad.iter().take(SHOWN_BAD) {
                    msg.push('\n');
                    msg.push_str(b);
                }
                if l.bad.len() > SHOWN_BAD {
                    msg.push_str("\n…");
                }
                ui.label(RichText::new(msg).color(t::p().icon_red));
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                go = ui.add(egui::Button::new(RichText::new(tr("Import")).color(egui::Color32::WHITE)).fill(t::p().primary)).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        if cancel || modal.should_close() {
            l.open = false;
        } else if go {
            match link_import::parse(&l.text) {
                Ok(links) => {
                    l.open = false;
                    self.fetch_links(links);
                }
                Err(bad) => {
                    l.bad = bad;
                    l.refused = true;
                }
            }
        }
    }

    /// Starts fetching `links` for the open folder.
    pub(crate) fn fetch_links(&mut self, links: Vec<String>) {
        let Ok(dir) = tempfile_dir() else { return };
        if links.is_empty() || self.link_import.rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let queue = Arc::new(Mutex::new(links.iter().cloned().collect::<VecDeque<_>>()));
        for _ in 0..WORKERS.min(links.len()) {
            let (queue, tx, dir, ctx) = (queue.clone(), tx.clone(), dir.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let next = queue.lock().unwrap_or_else(|p| p.into_inner()).pop_front();
                    let Some(address) = next else { break };
                    // `None` is a file that could not be written; the
                    // address is counted either way.
                    let sent = tx.send(link_import::resolve(&address, &dir).ok());
                    ctx.request_repaint();
                    if sent.is_err() {
                        break;
                    }
                }
            });
        }
        self.link_import.folders = self.scope_folders();
        self.link_import.expected = links.len();
        self.link_import.rx = Some(rx);
        self.flash(tr_n(links.len(), "Fetching {n} link", "Fetching {n} links"));
    }

    /// Queues the files the fetch has produced since last frame.
    fn collect_fetched(&mut self) {
        let Some(rx) = &self.link_import.rx else { return };
        let mut got = Vec::new();
        let finished = loop {
            match rx.try_recv() {
                Ok(r) => got.push(r),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        self.link_import.expected = self.link_import.expected.saturating_sub(got.len());
        if finished || self.link_import.expected == 0 {
            self.link_import.rx = None;
        }
        let (mut jobs, mut failed) = (Vec::new(), 0);
        for r in got {
            let Some(r) = r else {
                failed += 1;
                continue;
            };
            if self.ws.lib.is_none() {
                let _ = std::fs::remove_file(&r.path);
                continue;
            }
            let mut opts = self.prefs.import_options(self.link_import.folders.clone());
            opts.url = r.address;
            jobs.push(Job { path: r.path, opts });
        }
        if failed > 0 {
            self.flash_error(tr_n(failed, "{n} link could not be saved", "{n} links could not be saved"));
        }
        self.spawn_import(jobs);
    }
}
