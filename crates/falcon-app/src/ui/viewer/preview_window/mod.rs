//! Eagle's "Open in New Window" (⌘O): the selected items in a window of
//! their own, stepped with ← and →, a further eframe viewport like the
//! collect window; each open makes another window. Pictures zoom and pan
//! (with the navigator and preview-only rotate and flip); video, audio,
//! text and PDF files get a viewer of the window's own; the window can stay
//! on top, be made translucent, show in grayscale and copy the item (see
//! the desktop app chapter).

mod kind;
mod opacity;
mod pdf;
mod players;

use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::App;
use crate::loader::{Tier, job_for};
use crate::logic;
use crate::theme as t;
use crate::ui::dialogs::export::{CopyKind, Request};
use crate::ui::viewer::crop::ViewTransform;
use crate::ui::viewer::image::paint_transformed;
use crate::ui::widgets::ext_placeholder;
use kind::{Kind, OPACITIES, kind_of};

/// The viewer an item got in the window.
enum Pane {
    /// Not chosen yet (the window just opened).
    Pending,
    /// The zoomable picture: images, and the thumbnail of anything else.
    Picture,
    Video(players::VideoPane),
    Audio(players::AudioPane),
    Text(players::TextPane),
    Pdf(pdf::PdfPane),
}

impl Pane {
    fn open(ctx: &egui::Context, lib: &Library, item: &Item, autoplay: bool) -> Pane {
        let file = item.original_path(lib.root());
        match kind_of(&item.ext) {
            Kind::Video => players::VideoPane::open(&file, autoplay).map_or(Pane::Picture, Pane::Video),
            Kind::Audio => players::AudioPane::open(&file, item).map_or(Pane::Picture, Pane::Audio),
            Kind::Text => Pane::Text(players::TextPane::open(&file)),
            Kind::Pdf => Pane::Pdf(pdf::PdfPane::open(ctx, file)),
            Kind::Picture => Pane::Picture,
        }
    }
}

/// The preview window's state. It owns its players, which stop when the
/// item changes or the window closes.
pub(crate) struct PreviewWin {
    /// The item shown, one of `ids`.
    pub id: String,
    /// The items the window steps through, and where `id` is among them.
    ids: Vec<String>,
    index: usize,
    /// Tells this window's viewport apart from the others.
    serial: u64,
    /// Scale relative to the image's pixels; `None` fits the window.
    zoom: Option<f32>,
    pan: Vec2,
    on_top: bool,
    /// The window is being closed this frame.
    close: bool,
    /// Preview-only rotation and flip of the picture.
    tf: ViewTransform,
    /// Window opacity in percent.
    opacity: u8,
    /// The title and opacity last set on the window.
    applied: Option<(String, u8)>,
    /// Where in the navigator's frame a drag took hold, from its centre.
    nav_grab: Vec2,
    pane: Pane,
    /// A viewer of the window's own is drawing (not the picture).
    custom: bool,
}

impl PreviewWin {
    /// A window over `ids`, which must not be empty, showing the first.
    pub fn new(ids: Vec<String>, serial: u64) -> PreviewWin {
        PreviewWin {
            id: ids[0].clone(),
            ids,
            index: 0,
            serial,
            zoom: None,
            pan: Vec2::ZERO,
            on_top: false,
            close: false,
            tf: ViewTransform::default(),
            opacity: 100,
            applied: None,
            nav_grab: Vec2::ZERO,
            pane: Pane::Pending,
            custom: false,
        }
    }

    /// Moves `by` items along, stopping at either end as Eagle does. The
    /// new item starts with a fresh view and viewer; Always on Top and the
    /// opacity stay.
    fn step(&mut self, by: isize) {
        let next = self.index as isize + by;
        if next < 0 || next as usize >= self.ids.len() || next as usize == self.index {
            return;
        }
        let keep = PreviewWin { on_top: self.on_top, opacity: self.opacity, applied: self.applied.take(), ..PreviewWin::new(self.ids.clone(), self.serial) };
        *self = PreviewWin { id: keep.ids[next as usize].clone(), index: next as usize, ..keep };
    }
}

/// The items Eagle opens in a new window: every one given except audio.
pub fn window_items(items: &[(String, String)]) -> Vec<String> {
    items.iter().filter(|(_, ext)| !falcon_library::media::is_audio(&ext.to_lowercase())).map(|(id, _)| id.clone()).collect()
}

/// The window's first size for a picture `natural` big: the picture at
/// most 1000 × 760 points, never under 320 × 240.
pub fn window_size(natural: (f32, f32)) -> Vec2 {
    let (w, h) = (natural.0.max(1.0), natural.1.max(1.0));
    let k = (1000.0 / w).min(760.0 / h).min(1.0);
    vec2((w * k).max(320.0), (h * k).max(240.0))
}

/// What a menu entry asks for once the menu is closed.
enum Act {
    CopyFile,
    CopyFrame,
    SaveFrame,
}

impl App {
    /// Opens the items acted on (the viewer's, or every selected one) in a
    /// new preview window.
    pub(crate) fn open_preview_window(&mut self) {
        let ids = self.acted_on();
        let Some(shared) = self.ws.lib.clone() else { return };
        let lib = crate::app::lock(&shared);
        self.open_preview_items(&lib, &ids);
    }

    /// Opens `ids`, less audio, in a new preview window, as Eagle's
    /// `openInPreviewWindow` does, and records them as Recently Used.
    pub(crate) fn open_preview_items(&mut self, lib: &Library, ids: &[String]) {
        let items: Vec<(String, String)> = ids.iter().filter_map(|id| lib.item(id).map(|i| (id.clone(), i.ext.clone()))).collect();
        let ids = window_items(&items);
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            self.used_item(id);
        }
        self.preview_serial += 1;
        self.preview.push(PreviewWin::new(ids, self.preview_serial));
    }

    /// Shows each open preview window as its own OS window.
    pub(crate) fn preview_window(&mut self, ctx: &egui::Context, lib: &Library) {
        let mut wins = std::mem::take(&mut self.preview);
        wins.retain_mut(|pw| {
            let Some(item) = lib.item(&pw.id).cloned() else { return false };
            let natural = (item.width as f32, item.height as f32);
            let builder = egui::ViewportBuilder::default()
                .with_title(pw.title(&item))
                .with_inner_size(window_size(natural))
                .with_min_inner_size([200.0, 150.0]);
            let vid = egui::ViewportId::from_hash_of(("falcon-preview", pw.serial));
            ctx.show_viewport_immediate(vid, builder, |ui, _class| self.preview_contents(ui, lib, &item, pw));
            !pw.close
        });
        // Windows opened while these were drawn come after them.
        wins.append(&mut self.preview);
        self.preview = wins;
    }

    fn preview_contents(&mut self, ui: &mut Ui, lib: &Library, item: &Item, pw: &mut PreviewWin) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t::p().main);
        let (close, esc, cmd_w) = ctx.input(|i| (i.viewport().close_requested(), i.key_pressed(egui::Key::Escape), i.modifiers.command && i.key_pressed(egui::Key::W)));
        if close || esc || cmd_w {
            pw.close = true;
        }
        // ← and → step through the window's items (not while typing, nor
        // in the text and PDF viewers, which use the arrows); the next
        // frame shows the new item.
        let typing = ctx.egui_wants_keyboard_input() || matches!(pw.pane, Pane::Text(_) | Pane::Pdf(_));
        let (left, right) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight)));
        if pw.ids.len() > 1 && !typing && (left || right) {
            pw.step(if right { 1 } else { -1 });
            ctx.request_repaint();
            return;
        }
        if matches!(pw.pane, Pane::Pending) {
            pw.pane = Pane::open(&ctx, lib, item, self.prefs.video_autoplay);
        }
        // The opacity belongs to the NSWindow, found by its title.
        let want = (pw.title(item), pw.opacity);
        if pw.applied.as_ref() != Some(&want) {
            if opacity::set(&want.0, want.1) {
                pw.applied = Some(want);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

        // The picture: the full render, else the preview, else the type.
        for tier in [Tier::Full, Tier::Preview] {
            if let Some(j) = job_for(item, lib.root(), tier) {
                self.ws.frame_jobs.push(j);
            }
        }
        let tex = self.ws.loader.get(&item.id, Tier::Full).cloned().or_else(|| self.ws.loader.get(&item.id, Tier::Preview).cloned());
        let gray = self.modes.grayscale;
        let file = item.original_path(lib.root());

        let resp = ui.interact(full, ui.scope_id().with("preview-canvas"), Sense::click_and_drag());
        pw.custom = match &mut pw.pane {
            Pane::Video(v) => {
                v.show(ui, full, &resp, gray);
                true
            }
            Pane::Audio(a) => {
                a.show(ui, full, &mut self.ws.waveforms, item, &file, gray);
                true
            }
            Pane::Text(x) => {
                x.show(ui, full, gray);
                true
            }
            Pane::Pdf(p) => p.show(ui, full, gray),
            Pane::Pending | Pane::Picture => false,
        };
        if !pw.custom {
            pw.picture(ui, &resp, tex, item, full, self.modes.hide_navigator);
        }

        // Eagle's window menu, from a right-click.
        let mut act = None;
        resp.context_menu(|ui| act = self.window_menu(ui, pw, item, &file));
        match act {
            Some(Act::CopyFile) => self.copy_preview_file(item, &file),
            Some(Act::CopyFrame) => {
                if let Pane::Video(v) = &pw.pane
                    && let Some((img, _)) = v.current_frame()
                {
                    let (w, h) = img.dimensions();
                    let data = arboard::ImageData { width: w as usize, height: h as usize, bytes: img.into_raw().into() };
                    if arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).is_ok() {
                        self.flash_brief("Frame copied");
                    }
                }
            }
            Some(Act::SaveFrame) => {
                if let Pane::Video(v) = &pw.pane
                    && let Some((img, at)) = v.current_frame()
                {
                    let name = format!("{}.png", crate::ui::viewer::video::frame_item_name(&item.name, at));
                    if let Some(dest) = rfd::FileDialog::new().set_file_name(name).add_filter("PNG", &["png"]).save_file() {
                        match img.save(&dest) {
                            Ok(()) => self.flash_brief("Frame saved"),
                            Err(e) => self.flash_error(format!("Couldn't save the frame: {e}")),
                        }
                    }
                }
            }
            None => {}
        }
        self.ws.loader.set_grayscale(gray);
    }

    /// The window's right-click menu; returns what to do after it closes.
    fn window_menu(&mut self, ui: &mut Ui, pw: &mut PreviewWin, item: &Item, file: &std::path::Path) -> Option<Act> {
        let mut act = None;
        ui.set_min_width(200.0);
        if ui.add(egui::Button::new("Always on Top").selected(pw.on_top)).clicked() {
            pw.on_top = !pw.on_top;
            let level = if pw.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            ui.close();
        }
        ui.menu_button("Opacity", |ui| {
            for pct in OPACITIES {
                if ui.add(egui::Button::new(format!("{pct}%")).selected(pw.opacity == pct)).clicked() {
                    pw.opacity = pct;
                    ui.close();
                }
            }
        });
        if ui.add(egui::Button::new("Grayscale").selected(self.modes.grayscale)).clicked() {
            self.modes.grayscale = !self.modes.grayscale;
            ui.close();
        }
        if !pw.custom {
            ui.separator();
            if ui.button("Actual Size").clicked() {
                pw.zoom = Some(1.0);
                pw.pan = Vec2::ZERO;
                ui.close();
            }
            if ui.button("Zoom to Fit").clicked() {
                pw.zoom = None;
                ui.close();
            }
            if ui.add(egui::Button::new("Navigator").selected(!self.modes.hide_navigator)).clicked() {
                self.modes.hide_navigator = !self.modes.hide_navigator;
                ui.close();
            }
            ui.separator();
            for (label, to) in [
                ("Rotate Left", pw.tf.rotated(-1)),
                ("Rotate Right", pw.tf.rotated(1)),
                ("Flip Horizontal", pw.tf.flipped_h()),
                ("Flip Vertical", pw.tf.flipped_v()),
            ] {
                if ui.button(label).clicked() {
                    pw.tf = to;
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.button("Copy").clicked() {
            act = Some(Act::CopyFile);
            ui.close();
        }
        if ui.button("Copy File Path").clicked() {
            self.ws.export.request = Some(Request::Copy(CopyKind::Path, vec![item.id.clone()]));
            ui.close();
        }
        // The links as the item menu copies them: Link is the local API
        // link, Eagle Link the `eagle://item/<id>` one.
        ui.menu_button("Copy", |ui| {
            for (label, kind) in [("Link", CopyKind::Link), ("Eagle Link", CopyKind::EagleLink)] {
                if ui.button(label).clicked() {
                    self.ws.export.request = Some(Request::Copy(kind, vec![item.id.clone()]));
                    ui.close();
                }
            }
        });
        if matches!(pw.pane, Pane::Video(_)) {
            ui.separator();
            if ui.button("Copy Frame").clicked() {
                act = Some(Act::CopyFrame);
                ui.close();
            }
            if ui.button("Save Frame…").clicked() {
                act = Some(Act::SaveFrame);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Open in Default App").clicked() {
            crate::open_with::open_default(std::slice::from_ref(&file.to_path_buf()));
            ui.close();
        }
        if ui.button("Show in Finder").clicked() {
            crate::open_with::reveal(std::slice::from_ref(&file.to_path_buf()));
            ui.close();
        }
        if ui.button("Show in Falcon").clicked() {
            // Eagle's Reveal in Eagle: the item selected in the main window.
            self.ws.viewer = None;
            self.ws.selected = vec![item.id.clone()];
            self.ws.anchor = Some(item.id.clone());
            self.ws.scroll_to = Some(item.id.clone());
            ui.ctx().send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            ui.close();
        }
        ui.separator();
        if ui.button("Close Window").clicked() {
            pw.close = true;
            ui.close();
        }
        act
    }

    /// Copy: the original on the clipboard as a file, as ⌘C does in the grid.
    fn copy_preview_file(&mut self, item: &Item, file: &std::path::Path) {
        let files = [file.to_path_buf()];
        if let Ok(mut clip) = arboard::Clipboard::new()
            && clip.set().file_list(&files).is_ok()
        {
            self.used(&files);
            self.flash_brief(format!("Copied {}.{}", item.name, item.ext));
        }
    }
}

fn window_title(item: &Item) -> String {
    format!("{}.{}", item.name, item.ext)
}

impl PreviewWin {
    /// The window's title: the file name, with the position when the
    /// window holds several items.
    fn title(&self, item: &Item) -> String {
        if self.ids.len() > 1 { format!("{} ({}/{})", window_title(item), self.index + 1, self.ids.len()) } else { window_title(item) }
    }
}

impl PreviewWin {
    /// The zoomable picture (or the thumbnail of a type with no viewer of
    /// its own) with its navigator.
    fn picture(&mut self, ui: &mut Ui, resp: &egui::Response, tex: Option<TextureHandle>, item: &Item, full: Rect, hide_navigator: bool) {
        let ctx = ui.ctx().clone();
        let tf = self.tf;
        let natural = if item.width > 0 && item.height > 0 {
            tf.size(item.width as f32, item.height as f32)
        } else {
            tex.as_ref().map_or(Vec2::splat(512.0), |t| tf.size(t.size_vec2().x, t.size_vec2().y))
        };
        let fit = (full.width() / natural.x).min(full.height() / natural.y).min(1.0);
        let scale = self.zoom.unwrap_or(fit);

        let (scroll, pinch, pointer) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
        let factor = if pinch != 1.0 { pinch } else { (scroll / 200.0).exp() };
        if resp.hovered() && factor != 1.0 {
            let new = (scale * factor).clamp(logic::ZOOM_MIN, logic::ZOOM_MAX);
            if let Some(pos) = pointer {
                let off = pos - (full.center() + self.pan);
                self.pan += off - off * (new / scale);
            }
            self.zoom = Some(new);
        }
        if resp.dragged() {
            self.pan += resp.drag_delta();
            self.zoom.get_or_insert(scale);
        }
        if resp.double_clicked() {
            self.zoom = None;
        }
        let scale = self.zoom.unwrap_or(fit);
        if self.zoom.is_none() {
            self.pan = Vec2::ZERO;
        }
        let r = Rect::from_center_size(full.center() + self.pan, natural * scale);
        match &tex {
            Some(tex) if tf.is_identity() => {
                ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            Some(tex) => paint_transformed(ui.painter(), tex.id(), r, tf),
            None => ext_placeholder(ui, Rect::from_center_size(full.center(), Vec2::splat(180.0)), &item.ext),
        }
        if !hide_navigator {
            self.navigator(ui, tex.as_ref(), natural, r, full);
        }
    }

    /// The navigator of the image viewer: while the picture overflows the
    /// window, a small copy of it at the bottom right with a frame around
    /// the part on screen. Clicking it centres the view there; dragging
    /// moves the view along.
    fn navigator(&mut self, ui: &Ui, tex: Option<&TextureHandle>, natural: Vec2, r: Rect, area: Rect) {
        let Some(tex) = tex else { return };
        if !logic::navigator_shown((r.width(), r.height()), (area.width(), area.height())) {
            return;
        }
        let side = logic::NAVIGATOR_SIDE;
        let inset = Vec2::splat(logic::NAVIGATOR_INSET + side);
        let boxr = Rect::from_min_size(area.max - inset, Vec2::splat(side));
        let [px, py, pw, ph] = logic::navigator_picture((natural.x, natural.y), side);
        let pic = Rect::from_min_size(boxr.min + vec2(px, py), vec2(pw, ph));
        let [x0, y0, x1, y1] = logic::visible_part([r.min.x, r.min.y, r.max.x, r.max.y], [area.min.x, area.min.y, area.max.x, area.max.y]);
        let frame = Rect::from_min_max(pic.lerp_inside(vec2(x0, y0)), pic.lerp_inside(vec2(x1, y1)));
        let frame = Rect::from_center_size(frame.center(), frame.size().max(Vec2::splat(logic::NAVIGATOR_MIN_FRAME)));

        let resp = ui.interact(boxr, ui.scope_id().with("preview-navigator"), Sense::click_and_drag());
        if resp.drag_started() || resp.clicked() {
            let at = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            self.nav_grab = at.filter(|p| frame.contains(*p)).map_or(Vec2::ZERO, |p| p - frame.center());
        }
        if (resp.dragged() || resp.clicked())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let f = (p - self.nav_grab - pic.min) / pic.size();
            let pan = logic::pan_to_show(f.x, f.y, (r.width(), r.height()));
            let (px, py) = logic::clamp_pan(pan, (r.width(), r.height()), (area.width(), area.height()));
            self.pan = vec2(px, py);
            self.zoom = Some(r.width() / natural.x.max(1.0));
            ui.ctx().request_repaint();
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if frame.contains(resp.hover_pos().unwrap_or_default()) { egui::CursorIcon::Move } else { egui::CursorIcon::Default });
        }

        let p = ui.painter().with_clip_rect(area);
        let round = egui::CornerRadius::same(6);
        p.rect_filled(boxr, round, Color32::from_black_alpha(102));
        if self.tf.is_identity() {
            p.image(tex.id(), pic, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha(230));
        } else {
            paint_transformed(&p, tex.id(), pic, self.tf);
        }
        // The picture outside the frame is dimmed.
        let dim = Color32::from_black_alpha(153);
        for part in [
            Rect::from_min_max(pic.min, Pos2::new(pic.max.x, frame.min.y)),
            Rect::from_min_max(Pos2::new(pic.min.x, frame.max.y), pic.max),
            Rect::from_min_max(Pos2::new(pic.min.x, frame.min.y), Pos2::new(frame.min.x, frame.max.y)),
            Rect::from_min_max(Pos2::new(frame.max.x, frame.min.y), Pos2::new(pic.max.x, frame.max.y)),
        ] {
            let part = part.intersect(pic);
            if part.is_positive() {
                p.rect_filled(part, 0.0, dim);
            }
        }
        let frame_round = (frame.size().min_elem() / 2.0).min(6.0);
        p.rect_stroke(frame, frame_round, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
        p.rect_stroke(boxr, round, egui::Stroke::new(1.0, Color32::from_white_alpha(38)), egui::StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_fits_the_picture() {
        assert_eq!(window_size((800.0, 600.0)), vec2(800.0, 600.0));
        assert_eq!(window_size((4000.0, 2000.0)), vec2(1000.0, 500.0));
        assert_eq!(window_size((100.0, 50.0)), vec2(320.0, 240.0));
        assert_eq!(window_size((0.0, 0.0)), vec2(320.0, 240.0));
    }

    #[test]
    fn audio_stays_out_and_steps_stop_at_the_ends() {
        let items = [("a".to_string(), "png".to_string()), ("b".to_string(), "MP3".to_string()), ("c".to_string(), "mp4".to_string())];
        let ids = window_items(&items);
        assert_eq!(ids, ["a", "c"]);
        let mut w = PreviewWin::new(ids, 1);
        w.step(-1);
        assert_eq!(w.id, "a");
        (w.zoom, w.on_top, w.opacity) = (Some(2.0), true, 50);
        w.step(1);
        assert_eq!((w.id.as_str(), w.index, w.zoom, w.on_top, w.opacity), ("c", 1, None, true, 50));
        w.step(1);
        assert_eq!(w.id, "c");
    }
}
