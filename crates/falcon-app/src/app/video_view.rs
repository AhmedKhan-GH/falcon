//! The viewer for video items: the picture from `falcon-video`'s player,
//! a control bar under it, Eagle's player keys, and timed comments as
//! markers on the seek bar (see the desktop app chapter of the docs).

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align2, Color32, CornerRadius, Event, Key, Pos2, Rect, Sense, Stroke, TextEdit, TextureHandle, Ui, Vec2, vec2};
use falcon_library::model::{Comment, Item};
use falcon_library::{Library, media};
use falcon_video::Video;

use super::comments::{plain_text, time_label as comment_time};
use super::image_edit::Op;
use super::{Action, App, icon_button};
use crate::icons as ic;
use crate::shortcuts::PlayerKey;
use crate::logic;
use crate::theme::{self as t, font, mono};

const PLAY: &str = "\u{E3D0}";
const PAUSE: &str = "\u{E39E}";
const REPEAT: &str = "\u{E3F6}";
const SPEAKER: &str = "\u{E44A}";
/// Height of the control bar under the picture.
const CONTROLS: f32 = 84.0;
/// Eagle loops clips this short (in seconds) whatever the loop setting.
const SHORT_CLIP: f64 = 30.0;

/// A timed comment being written or edited in the viewer.
struct Draft {
    item: String,
    /// The comment being edited; `None` for a new one.
    id: Option<String>,
    /// Seconds into the clip.
    at: f64,
    text: String,
    focus: bool,
}

/// The viewer's video player and what it remembers between items.
pub(super) struct VideoState {
    /// The item the player belongs to.
    id: String,
    player: Option<Video>,
    /// Items AVFoundation can't play; the image viewer shows them instead.
    failed: HashSet<String>,
    texture: Option<TextureHandle>,
    /// 0..=1, saved between launches, as Eagle saves its player volume.
    pub volume: f32,
    /// The saved loop setting for clips over 30 s (Eagle's loop button).
    pub looping: bool,
    /// Saved with the volume and the loop setting.
    pub muted: bool,
    /// Where each item was left this session, to resume there.
    resume: HashMap<String, f64>,
    /// Whether to play again once a seek-bar drag ends.
    scrub_resume: Option<bool>,
    draft: Option<Draft>,
    /// A time to jump to once the item's player is open.
    pending: Option<(String, f64)>,
}

impl Default for VideoState {
    fn default() -> Self {
        VideoState {
            id: String::new(),
            player: None,
            failed: HashSet::new(),
            texture: None,
            volume: 1.0,
            looping: false,
            muted: false,
            resume: HashMap::new(),
            scrub_resume: None,
            draft: None,
            pending: None,
        }
    }
}

impl VideoState {
    /// The item whose player is open.
    pub fn loaded(&self) -> Option<&str> {
        self.player.as_ref().map(|_| self.id.as_str())
    }

    /// Stops and drops the player unless it belongs to `viewer`.
    pub fn keep_only(&mut self, viewer: Option<&str>) {
        if self.player.is_some() && viewer != Some(self.id.as_str()) {
            self.close();
        }
    }

    /// Jump to `secs` in item `id` once its player is open (now, if it is).
    pub fn seek_when_open(&mut self, id: &str, secs: f64) {
        match self.player.as_mut() {
            Some(p) if self.id == id => p.seek(secs),
            _ => self.pending = Some((id.to_string(), secs)),
        }
    }

    fn close(&mut self) {
        if let Some(p) = self.player.take() {
            self.resume.insert(std::mem::take(&mut self.id), resume_point(p.current_time(), p.info().duration));
        }
        self.id.clear();
        self.texture = None;
        self.draft = None;
        self.scrub_resume = None;
    }

    /// Opens `item`'s player (autoplaying, as Eagle does by default); false
    /// if AVFoundation can't play it.
    fn open(&mut self, lib: &Library, item: &Item) -> bool {
        if self.player.is_some() && self.id == item.id {
            return true;
        }
        self.close();
        let mut v = match Video::open(&item.original_path(lib.root())) {
            Ok(v) => v,
            Err(_) => {
                self.failed.insert(item.id.clone());
                return false;
            }
        };
        v.set_volume(self.volume);
        v.set_muted(self.muted);
        v.set_looping(starts_looping(v.info().duration, self.looping));
        let start = match self.pending.take() {
            Some((id, secs)) if id == item.id => Some(secs),
            other => {
                self.pending = other;
                self.resume.get(&item.id).copied()
            }
        };
        if let Some(s) = start.filter(|&s| s > 0.0) {
            v.seek(s);
        }
        v.play();
        self.id = item.id.clone();
        self.player = Some(v);
        true
    }
}

/// Whether a clip starts out looping: always when it is 30 s or shorter
/// (Eagle's "loop short videos"), otherwise as last set.
pub fn starts_looping(duration: f64, saved: bool) -> bool {
    duration <= SHORT_CLIP || saved
}

/// Where to resume a clip left at `pos`: from the top if it was at the end.
pub fn resume_point(pos: f64, duration: f64) -> f64 {
    if pos >= duration - 0.5 { 0.0 } else { pos.max(0.0) }
}

/// Eagle's skip: 2% of the clip, at most `cap` seconds (10 for the keys
/// and the back button, 20 for the forward button).
pub fn skip_step(duration: f64, cap: f64) -> f64 {
    (duration.max(0.0) * 0.02).min(cap)
}

/// Seconds to move for a scroll of `delta` points over the picture (the
/// larger axis wins), as Eagle's default "scroll seeks" habit: scrolling up
/// or right goes back. Small trackpad deltas move a tenth of a step.
pub fn scroll_seek(delta: Vec2, duration: f64) -> f64 {
    let d = if delta.y.abs() >= delta.x.abs() { delta.y } else { delta.x };
    if d == 0.0 {
        return 0.0;
    }
    let step = skip_step(duration, 10.0) * (0.1 + 0.9 * f64::from((d.abs() / 100.0).min(1.0)));
    if d > 0.0 { -step } else { step }
}

/// The comment marker under `x` on a seek bar, if any: the closest of the
/// `times` within `slop` points.
pub fn marker_at(x: f32, times: &[f64], duration: f64, left: f32, width: f32, slop: f32) -> Option<usize> {
    times
        .iter()
        .enumerate()
        .map(|(n, &s)| (n, (logic::playhead_x(s, duration, left, width) - x).abs()))
        .filter(|&(_, d)| d <= slop)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(n, _)| n)
}

/// Builds the egui picture for a decoded frame.
fn color_image(f: &falcon_video::Frame) -> egui::ColorImage {
    let pixels = f.rgba.as_chunks::<4>().0.iter().map(|p| Color32::from_rgb(p[0], p[1], p[2])).collect();
    egui::ColorImage { size: [f.width, f.height], source_size: vec2(f.width as f32, f.height as f32), pixels }
}

/// The timed comments of an item: (index in the list, seconds, comment).
fn timed(item: &Item) -> Vec<(usize, f64, Comment)> {
    item.comments().into_iter().enumerate().filter_map(|(n, c)| Some((n, c.duration.as_ref()?.as_f64()?, c))).collect()
}

impl App {
    /// Player keys while a video plays in the viewer, read before the app's
    /// own keys so Space and ⌘-arrows act on the player.
    pub(super) fn video_keys(&mut self, ctx: &egui::Context) {
        if self.ws.viewer.is_none() || self.video.loaded() != self.ws.viewer.as_deref() || self.prefs_window.open || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let mut pressed = Vec::new();
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                    let keys: Vec<Key> = std::iter::once(*key).chain(*physical_key).collect();
                    match self.shortcuts.player(&keys, *modifiers) {
                        Some(k) => {
                            pressed.push(k);
                            false
                        }
                        None => true,
                    }
                }
                // Releases of keys the player took stay harmless.
                _ => true,
            })
        });
        for k in pressed {
            self.player_action(k);
        }
    }

    fn player_action(&mut self, k: PlayerKey) {
        let id = self.video.id.clone();
        let Some(v) = self.video.player.as_mut() else { return };
        match k {
            PlayerKey::Toggle => v.toggle(),
            PlayerKey::Frames(n) => v.step(n),
            PlayerKey::Skip(fwd) => {
                let s = skip_step(v.info().duration, 10.0);
                v.seek(v.current_time() + if fwd { s } else { -s });
            }
            PlayerKey::Volume(up) => {
                self.video.volume = (self.video.volume + if up { 0.05 } else { -0.05 }).clamp(0.0, 1.0);
                self.video.muted = false;
                v.set_volume(self.video.volume);
                v.set_muted(false);
            }
            PlayerKey::Speed(up) => v.set_rate(falcon_video::step_rate(v.rate(), up)),
            PlayerKey::AddComment => {
                v.pause();
                self.video.draft = Some(Draft { item: id, id: None, at: v.current_time(), text: String::new(), focus: true });
            }
        }
    }

    /// Draws the viewer for a video AVFoundation can play and returns true;
    /// false for anything else (the image viewer shows it).
    pub(super) fn video_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        let ctx = &parent.ctx().clone();
        if !media::is_video(&item.ext.to_lowercase()) || self.video.failed.contains(&item.id) || !self.video.open(lib, item) {
            return false;
        }
        {
            let ui = &mut super::viewer_ui(parent, screen, "video-viewer");
            ui.painter().rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            ui.interact(area, ui.scope_id().with("video-bg"), Sense::click());
            self.video_view(ctx, ui, item, area);
            super::media_view::top_bar(self, ui, item, bar);
        }
        true
    }

    fn video_view(&mut self, ctx: &egui::Context, ui: &mut Ui, item: &Item, area: Rect) {
        let vs = &mut self.video;
        let Some(v) = vs.player.as_mut() else { return };
        v.tick();
        if let Some(f) = v.frame() {
            let img = color_image(&f);
            match &mut vs.texture {
                Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                None => vs.texture = Some(ctx.load_texture("video-frame", img, egui::TextureOptions::LINEAR)),
            }
        }
        let info = v.info().clone();
        let duration = info.duration;
        let pos = v.current_time();
        let playing = v.is_playing();
        let p = ui.painter().clone();

        // The picture, fitted like an image but never enlarged (Eagle's
        // default "fit" mode). Records without a size take the file's.
        let stage = Rect::from_min_max(area.min, Pos2::new(area.right(), area.bottom() - CONTROLS));
        let natural = match (&vs.texture, info.width, info.height) {
            (_, w, h) if w > 0 && h > 0 => vec2(w as f32, h as f32),
            (Some(tex), _, _) => tex.size_vec2(),
            _ => Vec2::splat(1.0),
        };
        let fit = ((stage.width() - 80.0) / natural.x).min((stage.height() - 40.0) / natural.y).clamp(0.01, 1.0);
        let shown = Rect::from_center_size(stage.center(), natural * fit);
        match (&vs.texture, v.error()) {
            (_, Some(e)) => {
                p.text(stage.center(), Align2::CENTER_CENTER, format!("Can't play this video: {e}"), font(13.0), t::p().text_muted);
            }
            (Some(tex), None) => egui::Image::new((tex.id(), shown.size())).paint_at(ui, shown),
            (None, None) => {
                p.text(stage.center(), Align2::CENTER_CENTER, "Loading…", font(13.0), t::p().text_muted);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        // Clicking the picture plays or pauses; scrolling seeks.
        let canvas = ui.interact(stage, ui.scope_id().with("video-canvas"), Sense::click());
        if canvas.clicked() {
            v.toggle();
        }
        if canvas.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            let d = scroll_seek(delta, duration);
            if d != 0.0 {
                v.seek(pos + d);
            }
        }

        // Seek bar with the timed comments on it.
        let controls = Rect::from_min_max(Pos2::new(area.left(), stage.bottom()), area.max);
        let (left, right) = (controls.left() + 28.0, controls.right() - 28.0);
        let width = (right - left).max(1.0);
        let track_y = controls.top() + 18.0;
        let hit = Rect::from_min_max(Pos2::new(left - 6.0, track_y - 9.0), Pos2::new(right + 6.0, track_y + 9.0));
        let seek = ui.interact(hit, ui.scope_id().with("video-seek"), Sense::click_and_drag());
        if seek.drag_started() {
            vs.scrub_resume = Some(playing);
            v.pause();
        }
        if (seek.clicked() || seek.dragged())
            && let Some(at) = seek.interact_pointer_pos()
        {
            v.seek(logic::seek_time(at.x, left, width, duration));
        }
        if seek.drag_stopped()
            && let Some(resume) = vs.scrub_resume.take()
            && resume
        {
            v.play();
        }
        if seek.hovered() || seek.dragged() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let hot = seek.hovered() || seek.dragged();
        let thick = if hot { 6.0 } else { 4.0 };
        let track = Rect::from_min_max(Pos2::new(left, track_y - thick / 2.0), Pos2::new(right, track_y + thick / 2.0));
        p.rect_filled(track, CornerRadius::same(3), t::p().divider);
        let head = logic::playhead_x(pos, duration, left, width);
        p.rect_filled(Rect::from_min_max(track.min, Pos2::new(head, track.bottom())), CornerRadius::same(3), t::p().primary);
        p.circle_filled(Pos2::new(head, track_y), if hot { 7.0 } else { 5.0 }, Color32::WHITE);

        let marks = timed(item);
        let times: Vec<f64> = marks.iter().map(|m| m.1).collect();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let near = pointer.filter(|p| hit.expand(4.0).contains(*p)).and_then(|p| marker_at(p.x, &times, duration, left, width, 5.0));
        for (k, (_, s, c)) in marks.iter().enumerate() {
            let x = logic::playhead_x(*s, duration, left, width);
            let r = Rect::from_center_size(Pos2::new(x, track_y), vec2(4.0, thick + 6.0));
            let color = if near == Some(k) { Color32::WHITE } else { Color32::from_white_alpha(170) };
            p.rect_filled(r, CornerRadius::same(1), color);
            let resp = ui.interact(r.expand2(vec2(3.0, 4.0)), ui.scope_id().with(("video-mark", &c.id)), Sense::click());
            let text = plain_text(&c.annotation);
            let resp = resp.on_hover_text(format!("{}  {}", comment_time(*s), if text.is_empty() { "(no text)" } else { &text }));
            if resp.clicked() {
                v.seek(*s);
            }
            resp.context_menu(|ui| {
                if ui.button("Edit Comment…").clicked() {
                    v.pause();
                    v.seek(*s);
                    vs.draft = Some(Draft { item: item.id.clone(), id: Some(c.id.clone()), at: *s, text: text.clone(), focus: true });
                    ui.close();
                }
                if ui.button("Delete Comment").clicked() {
                    self.ws.actions.push(Action::Edit(Op::DeleteComment(item.id.clone(), c.id.clone())));
                    ui.close();
                }
            });
        }

        // Buttons: play, skips, frame steps and time on the left; comment,
        // speed, loop and volume on the right.
        let cy = controls.top() + 54.0;
        let play = Rect::from_center_size(Pos2::new(left + 16.0, cy), Vec2::splat(34.0));
        let presp = ui.interact(play, ui.scope_id().with("video-play"), Sense::click());
        p.circle_filled(play.center(), 17.0, if presp.hovered() { t::p().primary } else { t::p().brand });
        p.text(play.center(), Align2::CENTER_CENTER, if playing { PAUSE } else { PLAY }, egui::FontId::new(16.0, t::fill_family()), Color32::WHITE);
        if presp.on_hover_text("Play/Pause (Space)").clicked() {
            v.toggle();
        }
        let mut x = play.right() + 20.0;
        if icon_button(ui, Pos2::new(x, cy), ic::ARROW_COUNTER_CLOCKWISE, 16.0, true, "Back (⌘←)").clicked() {
            v.seek(pos - skip_step(duration, 10.0));
        }
        x += 28.0;
        if icon_button(ui, Pos2::new(x, cy), ic::ARROW_CLOCKWISE, 16.0, true, "Forward (⌘→)").clicked() {
            v.seek(pos + skip_step(duration, 20.0));
        }
        x += 28.0;
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 15.0, true, "Previous Frame ([)").clicked() {
            v.step(-1);
        }
        x += 26.0;
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_RIGHT, 15.0, true, "Next Frame (])").clicked() {
            v.step(1);
        }
        x += 22.0;
        p.text(Pos2::new(x, cy), Align2::LEFT_CENTER, logic::time_label(pos, duration), mono(12.5), t::p().text_value);

        let vol = Rect::from_min_size(Pos2::new(right - 90.0, cy - 10.0), vec2(90.0, 20.0));
        let mut level = if vs.muted { 0.0 } else { vs.volume };
        let mut vui = ui.new_child(egui::UiBuilder::new().max_rect(vol));
        vui.spacing_mut().slider_width = 90.0;
        if vui.add(egui::Slider::new(&mut level, 0.0..=1.0).show_value(false)).on_hover_text("Volume (⌘↑ ⌘↓)").changed() {
            vs.volume = level;
            vs.muted = false;
            v.set_volume(level);
            v.set_muted(false);
        }
        let spk = Pos2::new(vol.left() - 18.0, cy);
        let tip = if vs.muted { "Unmute" } else { "Mute" };
        let sresp = icon_button(ui, spk, SPEAKER, 16.0, true, tip);
        if vs.muted || !info.has_audio {
            p.line_segment([spk + vec2(-8.0, 7.0), spk + vec2(8.0, -7.0)], Stroke::new(1.5, t::p().text_muted));
        }
        if sresp.clicked() {
            vs.muted = !vs.muted;
            v.set_muted(vs.muted);
        }
        let lp = Pos2::new(spk.x - 32.0, cy);
        let looping = v.looping();
        if looping {
            p.rect_filled(Rect::from_center_size(lp, Vec2::splat(26.0)), t::RADIUS, t::p().primary.gamma_multiply(0.35));
        }
        if icon_button(ui, lp, REPEAT, 16.0, true, if looping { "Loop: On" } else { "Loop: Off" }).clicked() {
            v.set_looping(!looping);
            vs.looping = !looping;
        }
        let sp = Rect::from_center_size(Pos2::new(lp.x - 40.0, cy), vec2(44.0, 24.0));
        let rate = v.rate();
        let label = format!("{}×", format!("{rate:.2}").trim_end_matches('0').trim_end_matches('.'));
        let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(sp));
        egui::containers::menu::MenuButton::new(egui::RichText::new(label).font(mono(12.0))).ui(&mut sui, |ui| {
            for r in falcon_video::RATES.iter().rev() {
                let name = format!("{}×", format!("{r:.2}").trim_end_matches('0').trim_end_matches('.'));
                if ui.selectable_label((r - rate).abs() < 1e-3, name).clicked() {
                    v.set_rate(*r);
                    ui.close();
                }
            }
        });
        let cp = Pos2::new(sp.left() - 22.0, cy);
        if icon_button(ui, cp, ic::CHAT_CIRCLE, 16.0, true, "Add Comment at Current Time (N)").clicked() {
            v.pause();
            vs.draft = Some(Draft { item: item.id.clone(), id: None, at: pos, text: String::new(), focus: true });
        }

        if vs.draft.as_ref().is_some_and(|d| d.item == item.id) {
            self.timed_comment_editor(ui, item, Pos2::new(controls.center().x, controls.top() - 8.0));
        }
        if playing {
            ctx.request_repaint();
        } else {
            // A seek or frame step delivers its picture a moment later.
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    /// The small box for writing a timed comment, above the control bar.
    fn timed_comment_editor(&mut self, ui: &mut Ui, item: &Item, bottom: Pos2) {
        let width = 340.0;
        let (mut save, mut cancel, mut delete) = (false, false, false);
        let Some(draft) = self.video.draft.as_mut() else { return };
        let at = Pos2::new(bottom.x - width / 2.0, bottom.y - 150.0);
        egui::Area::new(ui.scope_id().with("timed-comment")).order(egui::Order::Tooltip).fixed_pos(at).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(t::p().panel).stroke(Stroke::new(1.0, t::p().divider)).corner_radius(CornerRadius::same(8)).inner_margin(10).show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.label(egui::RichText::new(format!("Comment at {}", comment_time(draft.at))).font(font(12.0)).color(t::p().text_muted));
                let te = ui.add(TextEdit::multiline(&mut draft.text).id_salt("timed-comment-text").hint_text("Write a comment…").font(font(13.0)).desired_rows(3).desired_width(width - 20.0));
                if std::mem::take(&mut draft.focus) {
                    te.request_focus();
                }
                if te.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                    save = true;
                }
                if (te.has_focus() || te.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
                    cancel = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").on_hover_text("⌘↩").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if draft.id.is_some() && ui.button("Delete").clicked() {
                        delete = true;
                    }
                });
            });
        });
        if save {
            let d = self.video.draft.take().unwrap();
            let text = d.text.trim_end().to_string();
            match d.id {
                // Eagle drops a new comment left empty.
                None if text.trim().is_empty() => {}
                None => self.ws.actions.push(Action::Edit(Op::AddComment(item.id.clone(), Comment::at_time(d.at, &text)))),
                Some(id) => {
                    if let Some(mut c) = item.comments().into_iter().find(|c| c.id == id)
                        && plain_text(&c.annotation) != text
                    {
                        c.annotation = text;
                        self.ws.actions.push(Action::Edit(Op::UpdateComment(item.id.clone(), c)));
                    }
                }
            }
        } else if delete {
            if let Some(id) = self.video.draft.take().and_then(|d| d.id) {
                self.ws.actions.push(Action::Edit(Op::DeleteComment(item.id.clone(), id)));
            }
        } else if cancel {
            self.video.draft = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_clips_always_loop() {
        assert!(starts_looping(12.0, false));
        assert!(starts_looping(30.0, false));
        assert!(!starts_looping(30.5, false));
        assert!(starts_looping(600.0, true));
        assert!(starts_looping(0.0, false));
    }

    #[test]
    fn resume_points() {
        assert_eq!(resume_point(12.5, 60.0), 12.5);
        assert_eq!(resume_point(59.8, 60.0), 0.0);
        assert_eq!(resume_point(-1.0, 60.0), 0.0);
        assert_eq!(resume_point(0.0, 0.0), 0.0);
    }

    #[test]
    fn skips_are_two_percent_capped() {
        assert_eq!(skip_step(100.0, 10.0), 2.0);
        assert_eq!(skip_step(3600.0, 10.0), 10.0);
        assert_eq!(skip_step(3600.0, 20.0), 20.0);
        assert_eq!(skip_step(0.0, 10.0), 0.0);
        assert_eq!(skip_step(-5.0, 10.0), 0.0);
    }

    #[test]
    fn scrolling_up_goes_back() {
        assert_eq!(scroll_seek(Vec2::ZERO, 100.0), 0.0);
        assert_eq!(scroll_seek(vec2(0.0, 200.0), 100.0), -2.0);
        assert_eq!(scroll_seek(vec2(0.0, -200.0), 100.0), 2.0);
        // A small trackpad nudge moves a fraction of the step.
        let small = scroll_seek(vec2(0.0, -1.0), 100.0);
        assert!(small > 0.2 && small < 0.25, "{small}");
        // Horizontal swipes count when they dominate.
        assert_eq!(scroll_seek(vec2(-150.0, 10.0), 100.0), 2.0);
        assert_eq!(scroll_seek(vec2(0.0, 50.0), 0.0), 0.0);
    }

    #[test]
    fn markers_are_hit_within_slop() {
        let times = [10.0, 50.0, 52.0];
        // A 100 s clip on a bar from x=0 to x=1000: 10 points per second.
        assert_eq!(marker_at(102.0, &times, 100.0, 0.0, 1000.0, 5.0), Some(0));
        assert_eq!(marker_at(503.0, &times, 100.0, 0.0, 1000.0, 5.0), Some(1));
        // With a wide slop the closest of two neighbours wins.
        assert_eq!(marker_at(509.0, &times, 100.0, 0.0, 1000.0, 15.0), Some(1));
        assert_eq!(marker_at(512.0, &times, 100.0, 0.0, 1000.0, 15.0), Some(2));
        assert_eq!(marker_at(518.0, &times, 100.0, 0.0, 1000.0, 5.0), Some(2));
        assert_eq!(marker_at(300.0, &times, 100.0, 0.0, 1000.0, 5.0), None);
        assert_eq!(marker_at(0.0, &[], 100.0, 0.0, 1000.0, 5.0), None);
        // A clip of unknown length puts every marker at the start.
        assert_eq!(marker_at(1.0, &times, 0.0, 0.0, 1000.0, 5.0), Some(0));
    }

    #[test]
    fn frames_become_opaque_pictures() {
        let f = falcon_video::Frame { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] };
        let img = color_image(&f);
        assert_eq!(img.size, [2, 1]);
        assert_eq!(img.pixels, vec![Color32::RED, Color32::BLUE]);
    }

    #[test]
    fn closing_remembers_nothing_without_a_player() {
        let mut s = VideoState::default();
        assert_eq!(s.loaded(), None);
        s.keep_only(None);
        s.seek_when_open("A", 4.0);
        assert_eq!(s.pending, Some(("A".to_string(), 4.0)));
        s.close();
        assert!(s.resume.is_empty());
        assert_eq!(s.volume, 1.0);
    }
}
