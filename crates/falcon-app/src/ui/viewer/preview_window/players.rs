//! The window's own players: video, audio and text. Each belongs to the
//! window (never to the main viewer's players) and goes with it.

use std::io::Read;
use std::path::Path;

use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2, vec2};
use falcon_library::model::Item;
use falcon_library::{search, waveform};
use falcon_video::{Frame, Video};

use super::kind::TEXT_LIMIT;
use crate::audio::{Audio, Waveforms};
use crate::logic;
use crate::theme::{self as t, font, mono};
use crate::ui::viewer::audio::{PAUSE, PLAY, PLAYED, WAVE};

/// Height of the video's seek bar.
const BAR: f32 = 24.0;

/// A colour, or its gray when the window is in grayscale.
fn tint(c: Color32, gray: bool) -> Color32 {
    if gray {
        let [y, ..] = logic::grayscale_pixel([c.r(), c.g(), c.b(), 255]);
        Color32::from_rgba_unmultiplied(y, y, y, c.a())
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// Video

pub(super) struct VideoPane {
    video: Video,
    tex: Option<TextureHandle>,
    /// The last frame decoded, kept to upload again when grayscale changes.
    last: Option<Frame>,
    gray: bool,
}

impl VideoPane {
    /// Opens the file; `None` if AVFoundation can't play it.
    pub fn open(path: &Path, autoplay: bool) -> Option<VideoPane> {
        let mut video = Video::open(path).ok()?;
        if autoplay {
            video.play();
        }
        Some(VideoPane { video, tex: None, last: None, gray: false })
    }

    /// The frame at the playhead, at the clip's full size, and its time.
    pub fn current_frame(&self) -> Option<(image::RgbaImage, f64)> {
        let at = self.video.current_time();
        self.video.current_frame().ok().and_then(crate::ui::viewer::video::rgba_image).map(|img| (img, at))
    }

    fn upload(&mut self, ctx: &egui::Context, gray: bool) {
        let Some(frame) = &self.last else { return };
        let img = crate::ui::viewer::video::color_image(frame, gray);
        match &mut self.tex {
            Some(tex) => tex.set(img, TextureOptions::LINEAR),
            None => self.tex = Some(ctx.load_texture("preview-video", img, TextureOptions::LINEAR)),
        }
        self.gray = gray;
    }

    /// Draws the clip in `full`. A click on `canvas` (the window) or Space
    /// plays and pauses; the bar along the bottom seeks.
    pub fn show(&mut self, ui: &mut Ui, full: Rect, canvas: &egui::Response, gray: bool) {
        let ctx = ui.ctx().clone();
        self.video.tick();
        let fresh = self.video.frame();
        let changed = fresh.is_some() || (self.gray != gray && self.last.is_some());
        if let Some(f) = fresh {
            self.last = Some(f);
        }
        if changed {
            self.upload(&ctx, gray);
        }
        let info = self.video.info().clone();
        let (pos, playing) = (self.video.current_time(), self.video.is_playing());

        let stage = Rect::from_min_max(full.min, Pos2::new(full.right(), full.bottom() - BAR));
        let natural = match (&self.tex, info.width, info.height) {
            (_, w, h) if w > 0 && h > 0 => vec2(w as f32, h as f32),
            (Some(tex), ..) => tex.size_vec2(),
            _ => Vec2::splat(1.0),
        };
        // Fitted, never enlarged.
        let fit = (stage.width() / natural.x).min(stage.height() / natural.y).clamp(0.01, 1.0);
        match &self.tex {
            Some(tex) => {
                let r = Rect::from_center_size(stage.center(), natural * fit);
                ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().text(stage.center(), Align2::CENTER_CENTER, "Loading…", font(13.0), t::p().text_muted);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        if canvas.clicked() || ctx.input(|i| i.key_pressed(Key::Space)) {
            self.video.toggle();
        }

        // The seek bar: the time, then a thin track.
        let bar = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - BAR), full.max);
        let p = ui.painter();
        p.rect_filled(bar, 0.0, Color32::from_black_alpha(110));
        p.text(Pos2::new(bar.left() + 10.0, bar.center().y), Align2::LEFT_CENTER, logic::time_label(pos, info.duration), mono(11.0), tint(t::p().text_value, gray));
        let track = Rect::from_min_max(Pos2::new(bar.left() + 118.0, bar.top()), Pos2::new(bar.right() - 12.0, bar.bottom()));
        let seek = ui.interact(track, ui.scope_id().with("preview-seek"), Sense::click_and_drag());
        if (seek.clicked() || seek.dragged())
            && let Some(at) = seek.interact_pointer_pos()
        {
            self.video.seek(logic::seek_time(at.x, track.left(), track.width(), info.duration));
        }
        let y = track.center().y;
        let x = logic::playhead_x(pos, info.duration, track.left(), track.width());
        let p = ui.painter();
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(track.right(), y)], Stroke::new(2.0, Color32::from_white_alpha(60)));
        p.line_segment([Pos2::new(track.left(), y), Pos2::new(x, y)], Stroke::new(2.0, tint(PLAYED, gray)));
        p.circle_filled(Pos2::new(x, y), if seek.hovered() { 5.0 } else { 4.0 }, Color32::WHITE);
        if seek.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

// ---------------------------------------------------------------------------
// Audio

pub(super) struct AudioPane {
    audio: Audio,
    duration: f64,
}

impl AudioPane {
    /// Loads the track and starts it; `None` if it can't play.
    pub fn open(path: &Path, item: &Item) -> Option<AudioPane> {
        let mut audio = Audio::default();
        audio.load(&item.id, path, &item.ext, true);
        audio.loaded()?;
        let duration = search::duration(item).unwrap_or(0.0);
        audio.duration = duration;
        Some(AudioPane { audio, duration })
    }

    /// Draws the waveform as the seek bar, with a play button and the time.
    pub fn show(&mut self, ui: &mut Ui, full: Rect, waveforms: &mut Waveforms, item: &Item, path: &Path, gray: bool) {
        let ctx = ui.ctx().clone();
        self.audio.tick();
        let pos = self.audio.position();
        let duration = self.duration;
        let w = (full.width() - 48.0).clamp(80.0, 1000.0);
        let h = (full.height() - 100.0).clamp(40.0, 160.0);
        let wave = Rect::from_center_size(full.center() - vec2(0.0, 22.0), vec2(w, h));
        let (unplayed, played) = (tint(WAVE, gray), tint(PLAYED, gray));
        match waveforms.get(&ctx, &item.id, path, &item.ext) {
            None => {
                ui.painter().text(wave.center(), Align2::CENTER_CENTER, "Reading waveform…", font(13.0), t::p().text_muted);
            }
            Some(peaks) => {
                let bars = (w / 3.0) as usize;
                let peaks = peaks.map(|pk| waveform::rebucket(&pk, bars)).unwrap_or_else(|| vec![(0.0, 0.0); bars]);
                let loudest = peaks.iter().map(|&(l, h)| l.abs().max(h.abs())).fold(0.0f32, f32::max);
                let scale = if loudest > 0.0 { 1.0 / loudest } else { 0.0 };
                let done = logic::played_peaks(pos, duration, peaks.len());
                let (mid, half) = (wave.center().y, wave.height() / 2.0);
                for (n, &(l, hi)) in peaks.iter().enumerate() {
                    let x = wave.left() + n as f32 * 3.0 + 1.0;
                    let (top, bottom) = (mid - (hi * scale * half).max(0.5), mid - (l * scale * half).min(-0.5));
                    ui.painter().line_segment([Pos2::new(x, top), Pos2::new(x, bottom)], Stroke::new(2.0, if n < done { played } else { unplayed }));
                }
            }
        }
        let seek = ui.interact(wave, ui.scope_id().with("preview-wave"), Sense::click_and_drag());
        if (seek.clicked() || seek.dragged())
            && let Some(at) = seek.interact_pointer_pos()
        {
            self.audio.seek(logic::seek_time(at.x, wave.left(), wave.width(), duration));
        }
        if seek.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let x = logic::playhead_x(pos, duration, wave.left(), wave.width());
        ui.painter().line_segment([Pos2::new(x, wave.top()), Pos2::new(x, wave.bottom())], Stroke::new(1.0, played));

        // Play/pause and the time under the waveform.
        let cy = wave.bottom() + 34.0;
        let play = Rect::from_center_size(Pos2::new(wave.left() + 16.0, cy), Vec2::splat(32.0));
        let resp = ui.interact(play, ui.scope_id().with("preview-play"), Sense::click());
        let fill = tint(if resp.hovered() { t::p().primary } else { t::p().brand }, gray);
        ui.painter().circle_filled(play.center(), 16.0, fill);
        let glyph = if self.audio.is_playing() { PAUSE } else { PLAY };
        ui.painter().text(play.center(), Align2::CENTER_CENTER, glyph, FontId::new(15.0, t::fill_family()), Color32::WHITE);
        if resp.clicked() || ctx.input(|i| i.key_pressed(Key::Space)) {
            self.audio.toggle();
        }
        ui.painter().text(Pos2::new(play.right() + 12.0, cy), Align2::LEFT_CENTER, logic::time_label(pos, duration), mono(12.0), tint(t::p().text_value, gray));
        if self.audio.is_playing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

// ---------------------------------------------------------------------------
// Text

/// Lines laid out together; chunks keep each layout small and cached.
const CHUNK_LINES: usize = 200;

pub(super) struct TextPane {
    chunks: Vec<String>,
    error: Option<String>,
}

impl TextPane {
    /// Reads up to 2 MB of the file as lossy UTF-8.
    pub fn open(path: &Path) -> TextPane {
        let mut bytes = Vec::new();
        let read = std::fs::File::open(path).and_then(|f| f.take(TEXT_LIMIT).read_to_end(&mut bytes));
        match read {
            Ok(_) => TextPane { chunks: chunks(&String::from_utf8_lossy(&bytes)), error: None },
            Err(e) => TextPane { chunks: Vec::new(), error: Some(e.to_string()) },
        }
    }

    pub fn show(&self, ui: &mut Ui, full: Rect, gray: bool) {
        let color = tint(t::p().text, gray);
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(full.shrink(14.0)));
        if let Some(e) = &self.error {
            inner.label(egui::RichText::new(format!("Can't read this file: {e}")).color(tint(t::p().text_muted, gray)));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink(false).show(&mut inner, |ui| {
            ui.set_width(ui.available_width());
            for chunk in &self.chunks {
                ui.add(egui::Label::new(egui::RichText::new(chunk).font(mono(13.0)).color(color)).wrap().selectable(false));
            }
        });
    }
}

/// `text` cut into pieces of whole lines.
fn chunks(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    lines.chunks(CHUNK_LINES).map(|c| c.concat()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cut_into_whole_lines() {
        let text = "a\n".repeat(450);
        let c = chunks(&text);
        assert_eq!(c.len(), 3);
        assert_eq!(c.concat(), text);
        assert!(chunks("").is_empty());
        assert_eq!(chunks("no newline"), vec!["no newline".to_string()]);
    }
}
