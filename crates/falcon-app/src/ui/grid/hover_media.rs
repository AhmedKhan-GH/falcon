//! Drawing the grid's video and audio hover previews (`crate::hover_media`)
//! over the hovered card's picture.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::media;
use falcon_library::model::Item;

use crate::app::App;
use crate::hover_media::{self as hm, Strip};
use crate::logic::{self, Layout};
use crate::theme::{self as t, mono};

/// Height of the progress bar along the picture's bottom edge.
const BAR: f32 = 3.0;

impl App {
    /// Where a card's picture is drawn: the whole tile in waterfall and
    /// justified, the tile's inner square when the grid fills, else the
    /// picture fitted into it without enlarging (as `draw_image` does).
    fn card_picture(&self, item: &Item, card: Rect, caption: f32) -> Rect {
        let tile = Rect::from_min_max(card.min, Pos2::new(card.right(), card.bottom() - caption));
        if self.ws.layout != Layout::Grid {
            return tile;
        }
        if self.display.fill {
            return tile.shrink(1.0);
        }
        let r = tile.shrink(6.0);
        if item.width == 0 || item.height == 0 {
            return r;
        }
        let natural = vec2(item.width as f32, item.height as f32);
        let scale = (r.width() / natural.x).min(r.height() / natural.y).min(1.0);
        Rect::from_center_size(r.center(), natural * scale)
    }

    /// The hover preview of the hovered card, if it is a video or audio
    /// card: called once a frame for that card, after it is drawn.
    pub(crate) fn card_media(&mut self, ui: &Ui, lib: &Library, item: &Item, card: Rect, caption: f32, resp: &egui::Response) {
        if self.ws.layout == Layout::List || self.ws.marquee.is_some() || ui.input(|i| i.pointer.primary_down()) && !resp.is_pointer_button_down_on() {
            return;
        }
        let ext = item.ext.to_lowercase();
        let pic = self.card_picture(item, card, caption);
        if media::is_video(&ext) && self.prefs.video_hover {
            self.video_hover(ui, lib, item, pic);
        } else if media::is_audio(&ext) {
            self.audio_hover(ui, lib, item, pic, resp);
        }
    }

    /// Eagle's video hover preview: after half a second the card plays,
    /// silently and looping; moving the pointer across it scrubs, and it
    /// plays on from there once the pointer rests. A progress bar runs
    /// along the bottom and the time shows for a moment after each move.
    fn video_hover(&mut self, ui: &Ui, lib: &Library, item: &Item, pic: Rect) {
        let ctx = ui.ctx().clone();
        let (now, frame, dt) = ctx.input(|i| (i.time, ctx.cumulative_frame_nr(), f64::from(i.stable_dt.min(0.1))));
        let gray = self.modes.grayscale;
        let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| pic.contains(*p));
        let v = self.hover_media.video_for(&item.id, now, frame);
        if now - v.since < hm::VIDEO_DELAY {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(hm::VIDEO_DELAY - (now - v.since)));
            return;
        }
        if v.strip.as_ref().is_some_and(|s| s.gray() != gray) {
            v.strip = None;
        }
        if v.strip.is_none() {
            let path = item.original_path(lib.root());
            // An original still in iCloud isn't downloaded for a preview.
            if crate::icloud::is_dataless(&path) {
                return;
            }
            let side = ((pic.size().max_elem() * ctx.pixels_per_point()).ceil() as u32).clamp(64, hm::STRIP_MAX_SIDE);
            v.strip = Some(Strip::start(&ctx, path, side, gray));
            v.moved_at = now;
        }
        let Some(strip) = v.strip.as_mut() else { return };
        strip.poll(&ctx);
        if strip.failed {
            return;
        }
        ctx.request_repaint();
        let Some(duration) = strip.duration else { return };
        // The first position seen only arms scrubbing; play starts at 0.
        match (pointer, v.last_x) {
            (Some(p), Some(x)) if p.x != x => {
                v.t = logic::seek_time(p.x, pic.left(), pic.width(), duration);
                v.moved_at = now;
                v.label_until = now + hm::LABEL_SECS;
            }
            _ if now - v.moved_at >= hm::SCRUB_REST => v.t = hm::advance(v.t, dt, duration),
            _ => {}
        }
        v.last_x = pointer.map(|p| p.x).or(v.last_x);
        let (t_now, playing, label_until) = (v.t, now - v.moved_at >= hm::SCRUB_REST, v.label_until);
        let (muted, volume) = (self.display.video_muted, self.video.volume);
        let path = item.original_path(lib.root());
        self.hover_media.video_sound(&item.id, &path, &item.ext.to_lowercase(), muted, playing, t_now, duration, volume);
        let Some(strip) = self.hover_media.video.as_ref().and_then(|v| v.strip.as_ref()) else { return };
        let Some(tex) = strip.frame_at(t_now) else { return };
        let p = ui.painter().with_clip_rect(pic.intersect(ui.clip_rect()));
        let uv = cover_uv(tex.size_vec2(), pic.size());
        p.rect_filled(pic, CornerRadius::same(4), Color32::BLACK);
        p.image(tex.id(), pic, uv, Color32::WHITE);
        progress(&p, pic, t_now / duration);
        if now < label_until {
            time_pill(&p, pic, &logic::time_label(t_now, duration));
        }
        // The speaker button: its click is the button's, not the card's.
        let button = Rect::from_center_size(Pos2::new(pic.right() - 6.0 - 11.0, pic.bottom() - BAR - 6.0 - 11.0), Vec2::splat(22.0));
        let b = ui.interact(button, ui.scope_id().with(("hover-video-mute", &item.id)), Sense::click());
        p.circle_filled(button.center(), 11.0, if b.hovered() { t::p().primary } else { Color32::from_black_alpha(170) });
        let glyph = if muted { crate::icons::SPEAKER_X } else { crate::icons::SPEAKER_HIGH };
        p.text(button.center(), Align2::CENTER_CENTER, glyph, egui::FontId::new(13.0, t::fill_family()), Color32::WHITE);
        let b = b.on_hover_text(if muted { "Unmute" } else { "Mute" });
        if b.clicked() {
            self.display.video_muted = !muted;
        }
        self.hover_media.set_toggle_over(&item.id, b.hovered() || b.is_pointer_button_down_on(), frame);
    }

    /// Eagle's audio hover preview: after a moment the track plays (unless
    /// its play button was last switched off, which is remembered); the
    /// played part is tinted, a line follows the pointer, a click seeks
    /// there, and the button at the top right plays or pauses.
    fn audio_hover(&mut self, ui: &Ui, lib: &Library, item: &Item, pic: Rect, resp: &egui::Response) {
        let ctx = ui.ctx().clone();
        let (now, frame) = ctx.input(|i| (i.time, ctx.cumulative_frame_nr()));
        let (volume, autoplay) = (self.audio.volume, self.display.audio_autoplay);
        let a = self.hover_media.audio_for(&item.id, now, frame);
        let (since, started) = (a.since, std::mem::replace(&mut a.started, true));
        if now - since < hm::AUDIO_DELAY {
            a.started = started;
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(hm::AUDIO_DELAY - (now - since)));
            return;
        }
        let player = &mut self.hover_media.player;
        if !started {
            let path = item.original_path(lib.root());
            if crate::icloud::is_dataless(&path) {
                return;
            }
            player.looping = true;
            player.volume = volume;
            player.load(&item.id, &path, &item.ext.to_lowercase(), autoplay);
        }
        if player.loaded() != Some(item.id.as_str()) {
            return;
        }
        player.tick();
        let duration = falcon_library::search::duration(item).unwrap_or(0.0);
        let pos = player.position();
        let p = ui.painter().with_clip_rect(pic.intersect(ui.clip_rect()));
        if duration > 0.0 {
            let x = pic.left() + pic.width() * (pos / duration).clamp(0.0, 1.0) as f32;
            p.rect_filled(Rect::from_min_max(pic.min, Pos2::new(x, pic.bottom())), CornerRadius::same(4), t::p().primary.gamma_multiply(0.25));
            p.line_segment([Pos2::new(x, pic.top()), Pos2::new(x, pic.bottom())], Stroke::new(1.0, t::p().primary));
            if let Some(hp) = ctx.input(|i| i.pointer.hover_pos()).filter(|q| pic.contains(*q)) {
                p.line_segment([Pos2::new(hp.x, pic.top()), Pos2::new(hp.x, pic.bottom())], Stroke::new(1.0, Color32::from_white_alpha(140)));
            }
        }
        let button = Rect::from_center_size(Pos2::new(pic.right() - 16.0, pic.top() + 16.0), Vec2::splat(24.0));
        let b = ui.interact(button, ui.scope_id().with(("hover-audio-play", &item.id)), Sense::click());
        self.hover_media.set_toggle_over(&item.id, b.hovered() || b.is_pointer_button_down_on(), frame);
        let player = &mut self.hover_media.player;
        let playing = player.is_playing();
        p.circle_filled(button.center(), 12.0, if b.hovered() { t::p().primary } else { Color32::from_black_alpha(170) });
        let glyph = if playing { crate::ui::viewer::audio::PAUSE } else { crate::ui::viewer::audio::PLAY };
        p.text(button.center(), Align2::CENTER_CENTER, glyph, egui::FontId::new(12.0, t::fill_family()), Color32::WHITE);
        if b.clicked() {
            player.toggle();
            self.display.audio_autoplay = player.is_playing();
        } else if resp.clicked()
            && duration > 0.0
            && let Some(at) = resp.interact_pointer_pos().filter(|q| pic.contains(*q))
        {
            player.seek(logic::seek_time(at.x, pic.left(), pic.width(), duration));
        }
        time_pill(&p, pic, &logic::time_label(pos, duration));
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

/// The part of a picture of `tex` size that covers `rect` size, cropping
/// what overflows (the strip has the clip's shape, the card may not).
fn cover_uv(tex: Vec2, rect: Vec2) -> Rect {
    let (ta, ra) = (tex.x / tex.y.max(1.0), rect.x / rect.y.max(1.0));
    if ta > ra {
        let w = ra / ta;
        Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0))
    } else {
        let h = ta / ra;
        Rect::from_min_max(Pos2::new(0.0, (1.0 - h) / 2.0), Pos2::new(1.0, (1.0 + h) / 2.0))
    }
}

/// The progress bar along the bottom of `pic`, `f` of the way along.
fn progress(p: &egui::Painter, pic: Rect, f: f64) {
    let track = Rect::from_min_max(Pos2::new(pic.left(), pic.bottom() - BAR), pic.max);
    p.rect_filled(track, 0.0, Color32::from_white_alpha(60));
    let x = track.left() + track.width() * f.clamp(0.0, 1.0) as f32;
    p.rect_filled(Rect::from_min_max(track.min, Pos2::new(x, track.bottom())), 0.0, t::p().primary);
}

/// The time in a dark pill at the bottom left of `pic`.
fn time_pill(p: &egui::Painter, pic: Rect, text: &str) {
    let g = p.layout_no_wrap(text.to_string(), mono(11.0), Color32::WHITE);
    let r = Rect::from_min_size(Pos2::new(pic.left() + 6.0, pic.bottom() - BAR - 6.0 - g.size().y - 4.0), g.size() + vec2(10.0, 4.0));
    p.rect_filled(r, CornerRadius::same(4), Color32::from_black_alpha(170));
    p.galley(r.min + vec2(5.0, 2.0), g, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covering_crops_the_long_side() {
        let uv = cover_uv(vec2(200.0, 100.0), vec2(100.0, 100.0));
        assert!((uv.width() - 0.5).abs() < 1e-6 && (uv.height() - 1.0).abs() < 1e-6);
        assert!((uv.center().x - 0.5).abs() < 1e-6);
        let uv = cover_uv(vec2(100.0, 200.0), vec2(200.0, 100.0));
        assert!((uv.height() - 0.25).abs() < 1e-6 && (uv.width() - 1.0).abs() < 1e-6);
        assert_eq!(cover_uv(vec2(160.0, 90.0), vec2(320.0, 180.0)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)));
    }
}
