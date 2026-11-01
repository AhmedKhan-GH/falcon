//! The window's single-page PDF reader. A worker thread opens the document
//! (a `Document` stays on the thread that opened it) and renders the page
//! asked for; the window shows the current page fitted, with a page
//! stepper. Documents that fail to open (password-protected ones too) leave
//! the thumbnail showing.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, Align2, Color32, ColorImage, Pos2, Rect, TextureHandle, TextureOptions, Ui, vec2};
use falcon_pdf::{Bitmap, Document};

use super::kind::{key_scale, render_key, step_page};
use crate::locale::tr;
use crate::theme::{self as t, font};

/// Height of the page stepper along the bottom.
const BAR: f32 = 32.0;

struct Req {
    page: usize,
    key: i32,
}

enum Reply {
    Opened(Vec<(f64, f64)>),
    Failed,
    Page { page: usize, key: i32, bitmap: Option<Bitmap> },
}

fn work(path: PathBuf, rx: Receiver<Req>, tx: Sender<Reply>, ctx: egui::Context, max_side: usize) {
    let send = |r: Reply| {
        let ok = tx.send(r).is_ok();
        ctx.request_repaint();
        ok
    };
    let doc = match Document::open(&path) {
        Ok(d) => d,
        Err(_) => {
            send(Reply::Failed);
            return;
        }
    };
    let sizes = doc.page_sizes();
    if sizes.is_empty() {
        send(Reply::Failed);
        return;
    }
    if !send(Reply::Opened(sizes.clone())) {
        return;
    }
    // Only the newest request is rendered: a resize can ask many times.
    while let Ok(mut req) = rx.recv() {
        while let Ok(next) = rx.try_recv() {
            req = next;
        }
        let size = sizes.get(req.page).copied().unwrap_or((612.0, 792.0));
        let bitmap = doc.render(req.page, key_scale(req.key, size), max_side).ok();
        if !send(Reply::Page { page: req.page, key: req.key, bitmap }) {
            return;
        }
    }
}

/// A rendered page and the texture made from it.
struct Shown {
    page: usize,
    key: i32,
    bitmap: Bitmap,
    tex: TextureHandle,
    gray: bool,
}

pub(super) struct PdfPane {
    tx: Sender<Req>,
    rx: Receiver<Reply>,
    sizes: Vec<(f64, f64)>,
    failed: bool,
    page: usize,
    shown: Option<Shown>,
    asked: Option<(usize, i32)>,
}

impl PdfPane {
    pub fn open(ctx: &egui::Context, path: PathBuf) -> PdfPane {
        let (tx, wrx) = channel();
        let (wtx, rx) = channel();
        let max_side = ctx.input(|i| i.max_texture_side).clamp(1024, 8192);
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("falcon-preview-pdf".into()).spawn(move || work(path, wrx, wtx, ctx, max_side));
        PdfPane { tx, rx, sizes: Vec::new(), failed: spawned.is_err(), page: 0, shown: None, asked: None }
    }

    fn upload(ctx: &egui::Context, bitmap: &Bitmap, gray: bool, old: Option<TextureHandle>) -> TextureHandle {
        let mut rgba = bitmap.rgba.clone();
        if gray {
            crate::logic::grayscale_rgba(&mut rgba);
        }
        let img = ColorImage::from_rgba_unmultiplied([bitmap.width, bitmap.height], &rgba);
        match old {
            Some(mut tex) => {
                tex.set(img, TextureOptions::LINEAR);
                tex
            }
            None => ctx.load_texture("preview-pdf", img, TextureOptions::LINEAR),
        }
    }

    /// Takes what the worker has rendered.
    fn poll(&mut self, ctx: &egui::Context, gray: bool) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Opened(sizes) => self.sizes = sizes,
                Reply::Failed => self.failed = true,
                Reply::Page { page, key, bitmap } => {
                    if self.asked == Some((page, key)) {
                        self.asked = None;
                    }
                    match bitmap {
                        Some(bitmap) if page == self.page => {
                            let old = self.shown.take().map(|s| s.tex);
                            let tex = Self::upload(ctx, &bitmap, gray, old);
                            self.shown = Some(Shown { page, key, bitmap, tex, gray });
                        }
                        Some(_) => {}
                        None => self.failed = true,
                    }
                }
            }
        }
        if let Some(s) = &mut self.shown
            && s.gray != gray
        {
            s.tex = Self::upload(ctx, &s.bitmap, gray, Some(s.tex.clone()));
            s.gray = gray;
        }
    }

    /// Draws the page in `full`; false while the document is opening or
    /// when it can't be shown, so the thumbnail stands in.
    pub fn show(&mut self, ui: &mut Ui, full: Rect, gray: bool) -> bool {
        let ctx = ui.ctx().clone();
        self.poll(&ctx, gray);
        if self.failed || self.sizes.is_empty() {
            return false;
        }
        let count = self.sizes.len();
        let step = ctx.input(|i| {
            use egui::Key::{ArrowLeft, ArrowRight, PageDown, PageUp};
            let mut d = 0isize;
            for (key, by) in [(ArrowLeft, -1), (PageUp, -1), (ArrowRight, 1), (PageDown, 1)] {
                if i.key_pressed(key) {
                    d += by;
                }
            }
            d
        });
        let (mut prev, mut next) = (false, false);

        // The stepper along the bottom.
        let bar = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - BAR), full.max);
        ui.painter().rect_filled(bar, 0.0, Color32::from_black_alpha(90));
        let button = |ui: &mut Ui, x: f32, label: &str, enabled: bool| {
            ui.put(Rect::from_center_size(Pos2::new(x, bar.center().y), vec2(28.0, 22.0)), egui::Button::new(label).small()).clicked() && enabled
        };
        let label = format!("{} / {count}", self.page + 1);
        ui.painter().text(bar.center(), Align2::CENTER_CENTER, label, font(12.0), if gray { Color32::from_gray(200) } else { t::p().text_value });
        prev |= button(ui, bar.center().x - 60.0, "<", self.page > 0);
        next |= button(ui, bar.center().x + 60.0, ">", self.page + 1 < count);
        let delta = step + if prev { -1 } else { 0 } + if next { 1 } else { 0 };
        if delta != 0 {
            self.page = step_page(self.page, delta, count);
            self.asked = None;
            // Keep the old page up (stretched) only for the same page.
            if self.shown.as_ref().is_some_and(|s| s.page != self.page) {
                self.shown = None;
            }
        }

        // The page, fitted to the room above the stepper.
        let room = Rect::from_min_max(full.min, Pos2::new(full.right(), bar.top())).shrink(8.0);
        let (w, h) = self.sizes[self.page];
        let fit = (room.width() / w.max(1.0) as f32).min(room.height() / h.max(1.0) as f32).max(0.01);
        let rect = Rect::from_center_size(room.center(), vec2(w as f32 * fit, h as f32 * fit));
        let key = render_key(fit, ctx.pixels_per_point());
        let have = self.shown.as_ref().is_some_and(|s| s.page == self.page && s.key == key);
        if !have && self.asked != Some((self.page, key)) {
            self.asked = Some((self.page, key));
            if self.tx.send(Req { page: self.page, key }).is_err() {
                self.failed = true;
            }
        }
        match &self.shown {
            Some(s) => {
                ui.painter().image(s.tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, tr("Rendering…"), font(13.0), Color32::from_gray(120));
            }
        }
        true
    }
}
