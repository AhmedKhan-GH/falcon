//! The viewer for video items: the picture from `falcon-video`'s player,
//! a control bar under it, Eagle's player keys, and timed comments as
//! markers on the seek bar (see the desktop app chapter of the docs).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

use eframe::egui::{self, Align2, Color32, CornerRadius, Event, Key, Pos2, Rect, Sense, Stroke, TextEdit, TextureHandle, Ui, Vec2, vec2};
use falcon_library::model::{Comment, Item};
use falcon_library::{Library, media};
use falcon_video::Video;

use crate::ui::viewer::comments::{plain_text, time_label as comment_time};
use crate::app::image_edit::Op;
use crate::app::{Action, App};
use crate::ui::widgets::icon_button;
use crate::icons as ic;
use crate::shortcuts::{Command, FrameTool, PlayerKey};
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
/// Width of the seek bar's hover thumbnail, in points.
const PREVIEW_WIDTH: f32 = 160.0;

/// The seek bar's hover thumbnail: a worker thread with its own image
/// generator decodes small pictures of the clip, always the latest time
/// asked for (older requests waiting in the channel are skipped).
struct SeekPreview {
    tx: mpsc::Sender<f64>,
    /// The newest picture decoded and the time it shows.
    got: Arc<Mutex<Option<(f64, falcon_video::Frame)>>>,
    texture: Option<TextureHandle>,
    /// The last time sent to the worker.
    asked: Option<f64>,
}

impl SeekPreview {
    fn start(ctx: &egui::Context, path: PathBuf) -> SeekPreview {
        let (tx, rx) = mpsc::channel::<f64>();
        let got = Arc::new(Mutex::new(None));
        let (out, ctx) = (got.clone(), ctx.clone());
        std::thread::spawn(move || {
            // Twice the drawn width, for Retina screens.
            let Ok(grabber) = falcon_video::Grabber::open(&path, (PREVIEW_WIDTH * 2.0) as u32) else { return };
            // Ends when the viewer drops the sender.
            while let Ok(mut t) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    t = newer;
                }
                if let Ok(f) = grabber.grab(t) {
                    *out.lock().unwrap_or_else(|p| p.into_inner()) = Some((t, f));
                    ctx.request_repaint();
                }
            }
        });
        SeekPreview { tx, got, texture: None, asked: None }
    }

    /// Asks for the picture at `t` unless the last request was close by.
    fn ask(&mut self, t: f64, step: f64) {
        if needs_request(t, self.asked, step) {
            self.asked = Some(t);
            let _ = self.tx.send(t);
        }
    }

    /// The newest picture as a texture, uploading it if it is new.
    fn texture(&mut self, ctx: &egui::Context) -> Option<&TextureHandle> {
        if let Some((_, f)) = self.got.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let img = color_image(&f);
            match &mut self.texture {
                Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("video-seek-preview", img, egui::TextureOptions::LINEAR)),
            }
        }
        self.texture.as_ref()
    }
}

/// How long the volume tip stays up after the last change, in seconds.
pub(crate) const TIP_SECS: f64 = 1.0;

/// Eagle's volume tip: the level's speaker and the percentage in a dark
/// box whose top right corner is at `corner`.
pub(crate) fn volume_tip(p: &egui::Painter, corner: Pos2, percent: u32) {
    let glyph = [ic::SPEAKER_X, ic::SPEAKER_NONE, ic::SPEAKER_LOW, ic::SPEAKER_HIGH][usize::from(volume_level(percent))];
    let g = p.layout_no_wrap(format!("{percent}%"), font(12.0), Color32::WHITE);
    let size = vec2(8.0 + 22.0 + 2.0 + g.size().x + 12.0, 30.0);
    let r = Rect::from_min_size(Pos2::new(corner.x - size.x, corner.y), size);
    p.rect_filled(r, CornerRadius::same(8), Color32::from_black_alpha(178));
    p.text(Pos2::new(r.left() + 19.0, r.center().y), Align2::CENTER_CENTER, glyph, font(18.0), Color32::WHITE);
    p.galley(Pos2::new(r.left() + 32.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
}
/// How often a held skip button repeats, in seconds (Eagle's 100 ms).
const REPEAT_SECS: f64 = 0.1;

/// A button that acts once when pressed and again every [`REPEAT_SECS`]
/// while held, as Eagle's skip buttons do.
#[derive(Default)]
pub struct Repeat {
    next: Option<f64>,
}

impl Repeat {
    /// Whether to act this frame, given whether the button is held at time
    /// `now` (seconds).
    pub fn fire(&mut self, held: bool, now: f64) -> bool {
        match (held, self.next) {
            (false, _) => {
                self.next = None;
                false
            }
            (true, None) => {
                self.next = Some(now + REPEAT_SECS);
                true
            }
            (true, Some(at)) if now >= at => {
                self.next = Some(at + REPEAT_SECS);
                true
            }
            _ => false,
        }
    }

    pub fn held(&self) -> bool {
        self.next.is_some()
    }
}

/// Formats Eagle hands to its bundled mpv player because Chromium can't
/// play them; macOS can't either, and falcon has no second player.
const NEEDS_OTHER_PLAYER: [&str; 6] = ["mkv", "webm", "avi", "wmv", "flv", "360"];

/// The line under the title of the "can't play" page.
pub fn cannot_play_line(ext: &str) -> String {
    let ext = ext.to_lowercase();
    if NEEDS_OTHER_PLAYER.contains(&ext.as_str()) {
        format!("macOS can't play .{ext} videos, and Falcon has no player of its own for them. Try opening it in another app.")
    } else {
        "macOS couldn't play this video. Try opening it in another app.".to_string()
    }
}

/// The quarter turns after a press of the rotate button or ⇧R: Eagle turns
/// the picture a quarter anticlockwise, or clockwise on a ⇧-click.
pub fn rotate_step(turns: u8, clockwise: bool) -> u8 {
    (turns + if clockwise { 1 } else { 3 }) % 4
}

/// Texture coordinates for the shown picture's corners (top left, top
/// right, bottom right, bottom left) when it is mirrored left to right if
/// `flip` and then turned `turns` quarter turns clockwise, as Eagle's CSS
/// `rotate(…) scaleX(-1)` does.
pub fn corner_uvs(turns: u8, flip: bool) -> [Pos2; 4] {
    let base = if flip { [(1.0, 0.0), (0.0, 0.0), (0.0, 1.0), (1.0, 1.0)] } else { [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] };
    std::array::from_fn(|i| {
        let (u, v) = base[(i + 4 - usize::from(turns % 4)) % 4];
        Pos2::new(u, v)
    })
}

/// Paints `tex` over `rect` with the corners' texture coordinates `uvs`.
fn paint_turned(ui: &Ui, tex: &TextureHandle, rect: Rect, uvs: [Pos2; 4]) {
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (pos, uv) in [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Eagle's volume tip icon for a level in percent: 0 off, 1 under 30%,
/// 2 under 100%, 3 at full volume.
pub fn volume_level(percent: u32) -> u8 {
    match percent {
        0 => 0,
        1..=29 => 1,
        30..=99 => 2,
        _ => 3,
    }
}

/// Whether the hover thumbnail should be decoded again for time `t`: it
/// moved at least `step` seconds from the last request, or there was none.
pub fn needs_request(t: f64, asked: Option<f64>, step: f64) -> bool {
    asked.is_none_or(|a| (t - a).abs() >= step)
}

/// The left edge of a box `w` wide centred on `x`, kept between `left` and
/// `right`.
pub fn bubble_left(x: f32, w: f32, left: f32, right: f32) -> f32 {
    (x - w / 2.0).min(right - w).max(left)
}

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
pub(crate) struct VideoState {
    /// The item the player belongs to.
    id: String,
    player: Option<Video>,
    /// Items AVFoundation can't play; the "can't play" page shows instead.
    failed: HashSet<String>,
    texture: Option<TextureHandle>,
    /// 0..=1, saved between launches, as Eagle saves its player volume.
    pub volume: f32,
    /// The saved loop setting for clips over 30 s (Eagle's loop button).
    pub looping: bool,
    /// Saved with the volume and the loop setting.
    pub muted: bool,
    /// Where items were left, oldest first, to resume there; saved between
    /// launches as Eagle saves each item's position (`Positions`).
    pub resume: Positions,
    /// Whether to play again once a seek-bar drag ends.
    scrub_resume: Option<bool>,
    draft: Option<Draft>,
    /// A time to jump to once the item's player is open.
    pending: Option<(String, f64)>,
    /// A frame tool asked for by a key, run while the viewer draws.
    tool: Option<FrameTool>,
    /// The seek bar's hover thumbnail, started on first hover.
    preview: Option<SeekPreview>,
    /// The back and forward buttons, which repeat while held.
    back: Repeat,
    forward: Repeat,
    /// The volume and mute last drawn, to notice changes for the tip.
    tip_seen: Option<(f32, bool)>,
    /// Until when (egui time) the volume tip shows.
    tip_until: f64,
    /// The picture's view turn (clockwise quarter turns) and mirroring,
    /// from the viewer bar's rotate and flip buttons; the file is untouched.
    turns: u8,
    flipped: bool,
    /// The open clip's subtitles, from a `.vtt` or `.srt` beside it.
    subs: Vec<crate::subtitles::Cue>,
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
            resume: Positions::default(),
            scrub_resume: None,
            draft: None,
            pending: None,
            tool: None,
            preview: None,
            back: Repeat::default(),
            forward: Repeat::default(),
            tip_seen: None,
            tip_until: 0.0,
            turns: 0,
            flipped: false,
            subs: Vec::new(),
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
            self.resume.set(std::mem::take(&mut self.id), resume_point(p.current_time(), p.info().duration));
        }
        self.id.clear();
        self.texture = None;
        self.draft = None;
        self.scrub_resume = None;
        self.tool = None;
        self.preview = None;
        self.back = Repeat::default();
        self.forward = Repeat::default();
        self.tip_seen = None;
        self.tip_until = 0.0;
        self.turns = 0;
        self.flipped = false;
        self.subs.clear();
    }

    /// The positions to save, with the open clip's current one.
    pub fn positions(&self) -> Positions {
        let mut all = self.resume.clone();
        if let Some(p) = &self.player {
            all.set(self.id.clone(), resume_point(p.current_time(), p.info().duration));
        }
        all
    }

    /// Opens `item`'s player, following the video preferences (autoplay,
    /// resuming, looping short clips); false if AVFoundation can't play it.
    fn open(&mut self, lib: &Library, item: &Item, prefs: &crate::prefs::Prefs) -> bool {
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
        v.set_looping(starts_looping(v.info().duration, self.looping, prefs.video_loop_short));
        let start = match self.pending.take() {
            Some((id, secs)) if id == item.id => Some(secs),
            other => {
                self.pending = other;
                self.resume.get(&item.id).filter(|_| prefs.video_resume)
            }
        };
        if let Some(s) = start.filter(|&s| s > 0.0) {
            v.seek(s);
        }
        if prefs.video_autoplay {
            v.play();
        }
        self.id = item.id.clone();
        self.player = Some(v);
        // Eagle shows the first of `<name>.vtt` and `<name>.srt` it finds.
        let original = item.original_path(lib.root());
        self.subs = crate::subtitles::sidecars(&original).iter().find_map(|p| crate::subtitles::load(p)).unwrap_or_default();
        true
    }
}

/// Whether a clip starts out looping: always when it is 30 s or shorter
/// and Preferences loops short clips (Eagle's default), otherwise as last
/// set.
pub fn starts_looping(duration: f64, saved: bool, loop_short: bool) -> bool {
    (loop_short && duration <= SHORT_CLIP) || saved
}

/// How many clips' positions are remembered; the oldest go first.
const MAX_POSITIONS: usize = 500;

/// Where each clip was left, by item id, oldest first.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(transparent)]
pub struct Positions(Vec<(String, f64)>);

impl Positions {
    pub fn get(&self, id: &str) -> Option<f64> {
        self.0.iter().find(|(i, _)| i == id).map(|(_, s)| *s)
    }

    /// Records where `id` was left; a clip left at the start is forgotten.
    pub fn set(&mut self, id: String, secs: f64) {
        self.0.retain(|(i, _)| *i != id);
        if secs > 0.0 && secs.is_finite() {
            self.0.push((id, secs));
        }
        let extra = self.0.len().saturating_sub(MAX_POSITIONS);
        self.0.drain(..extra);
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Where to resume a clip left at `pos`: from the top if it was at the end.
pub fn resume_point(pos: f64, duration: f64) -> f64 {
    if pos >= duration - 0.5 { 0.0 } else { pos.max(0.0) }
}

/// The volume after a scroll of `delta` points over the picture, when
/// Preferences has the wheel set the volume: up or right is louder, 5% a
/// notch, small trackpad deltas a tenth of that.
pub fn scroll_volume(delta: Vec2, volume: f32) -> f32 {
    let d = if delta.y.abs() >= delta.x.abs() { delta.y } else { delta.x };
    let step = 0.05 * (0.1 + 0.9 * (d.abs() / 100.0).min(1.0));
    (volume + step * d.signum() * f32::from(u8::from(d != 0.0))).clamp(0.0, 1.0)
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

/// The name Eagle gives a frame saved from a video: the video's name and
/// the second it was taken at, to two decimals (`toFixed(2)`, which
/// rounds an exact half up).
pub fn frame_item_name(name: &str, at: f64) -> String {
    format!("{name} - {:.2}", (at.max(0.0) * 100.0).round() / 100.0)
}

/// A decoded frame as an image the library and clipboard code take.
fn rgba_image(f: falcon_video::Frame) -> Option<image::RgbaImage> {
    image::RgbaImage::from_raw(f.width as u32, f.height as u32, f.rgba)
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
    /// Player keys while a video or an audio item is in the viewer, read
    /// before the app's own keys so Space and ⌘-arrows act on the player.
    pub(crate) fn video_keys(&mut self, ctx: &egui::Context) {
        let viewer = self.ws.viewer.as_deref();
        let (video, audio) = (viewer.is_some() && self.video.loaded() == viewer, viewer.is_some() && self.audio.loaded() == viewer);
        if !(video || audio) || self.prefs_window.open || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        if audio {
            let pressed = self.take_player_keys(ctx);
            for k in pressed {
                self.audio_action(k);
            }
            return;
        }
        // ⇧R and ⇧F turn and mirror the picture, as Eagle's image keys do
        // for a video; they never touch the file.
        let (rotate, flip) = ctx.input_mut(|i| (i.consume_key(egui::Modifiers::SHIFT, Key::R), i.consume_key(egui::Modifiers::SHIFT, Key::F)));
        if rotate {
            self.video.turns = rotate_step(self.video.turns, false);
        }
        if flip {
            self.video.flipped = !self.video.flipped;
        }
        let pressed = self.take_player_keys(ctx);
        for k in pressed {
            self.player_action(k);
        }
    }

    /// Takes this frame's presses of the player keys out of the input.
    fn take_player_keys(&self, ctx: &egui::Context) -> Vec<PlayerKey> {
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
        pressed
    }

    /// A player key with audio in the viewer, as Eagle's audio player
    /// binds them: volume 5%, skips of 2% (at most 10 s), the speed steps,
    /// and frame keys moving 1/24 s (its default frame time).
    fn audio_action(&mut self, k: PlayerKey) {
        let a = &mut self.audio;
        match k {
            PlayerKey::Toggle => a.toggle(),
            PlayerKey::Volume(up) => a.set_volume(a.volume + if up { 0.05 } else { -0.05 }),
            PlayerKey::Skip(fwd) => {
                let s = skip_step(a.duration, 10.0);
                a.seek(a.position() + if fwd { s } else { -s });
            }
            PlayerKey::Speed(up) => a.set_speed(falcon_video::step_rate(a.speed(), up)),
            PlayerKey::Frames(n) => a.seek(a.position() + f64::from(n) / 24.0),
            PlayerKey::AddComment | PlayerKey::Frame(_) => {}
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
            PlayerKey::Frame(t) => self.video.tool = Some(t),
        }
    }

    /// Eagle's "can't play this file" page: the thumbnail (or the type's
    /// placeholder), the name, a line saying why, and Open With.
    fn cannot_play(&mut self, ui: &mut Ui, lib: &Library, item: &Item, area: Rect) {
        let resp = ui.interact(area, ui.scope_id().with("video-cannot-play"), Sense::click());
        resp.context_menu(|ui| {
            self.item_menu(ui, lib, item, std::slice::from_ref(&item.id));
        });
        let thumb = Rect::from_center_size(area.center() - vec2(0.0, 70.0), Vec2::splat(150.0));
        match self.ws.loader.get(&item.id, crate::loader::Tier::Preview).cloned() {
            Some(tex) => {
                let size = tex.size_vec2() * (thumb.width() / tex.size_vec2().max_elem());
                egui::Image::new((tex.id(), size)).paint_at(ui, Rect::from_center_size(thumb.center(), size));
            }
            None => crate::ui::widgets::ext_placeholder(ui, thumb, &item.ext),
        }
        if let Some(j) = crate::loader::job_for(item, lib.root(), crate::loader::Tier::Preview) {
            self.ws.frame_jobs.push(j);
        }
        let name = format!("{}.{}", item.name, item.ext);
        self.notice_lines(ui, area, &name, &cannot_play_line(&item.ext));
    }

    /// Eagle's frame tools on the picture at the playhead, decoded at the
    /// clip's full size: make it the thumbnail, copy it, or add it to the
    /// library as a new JPEG item named after the video and the time, in
    /// the video's folders and with its URL.
    fn frame_tool(&mut self, item: &Item, tool: FrameTool) {
        let Some(v) = self.video.player.as_ref() else { return };
        let at = v.current_time();
        let img = match v.current_frame().map(rgba_image) {
            Ok(Some(img)) => img,
            Ok(None) => return,
            Err(e) => {
                self.flash(format!("Couldn't read the frame: {e}"));
                return;
            }
        };
        match tool {
            FrameTool::Thumbnail => self.ws.actions.push(Action::Edit(Op::VideoThumbnail(item.id.clone(), img, at))),
            FrameTool::Copy => {
                let (w, h) = img.dimensions();
                let data = arboard::ImageData { width: w as usize, height: h as usize, bytes: img.into_raw().into() };
                if arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).is_ok() {
                    self.flash("Frame copied");
                }
            }
            FrameTool::Save => {
                let jpeg = falcon_library::thumbnail::jpeg(&image::DynamicImage::ImageRgba8(img), 95);
                let Ok(dir) = crate::app::import::tempfile_dir() else { return };
                let path = dir.join(format!("{}.jpg", frame_item_name(&item.name, at)));
                // The import thread deletes the staged file once it has it.
                if jpeg.is_ok_and(|b| std::fs::write(&path, b).is_ok()) {
                    let mut opts = self.prefs.import_options(item.folders.clone());
                    opts.url = item.url.clone();
                    self.spawn_import(vec![crate::app::import::Job { path, opts }]);
                }
            }
        }
    }

    /// The frame tools at the top of the video's right-click menu, as in
    /// Eagle's viewer, above the item's own menu.
    fn video_menu(&mut self, ui: &mut Ui, lib: &Library, item: &Item) {
        let entries = [
            ("Set Current Frame as Thumbnail", FrameTool::Thumbnail),
            ("Copy Current Frame", FrameTool::Copy),
            ("Save Current Frame to Library", FrameTool::Save),
        ];
        for (label, tool) in entries {
            let keys = self.shortcut_text(ui.ctx(), Command::Player(PlayerKey::Frame(tool)));
            if ui.add(egui::Button::new(label).shortcut_text(keys)).clicked() {
                self.frame_tool(item, tool);
                ui.close();
            }
        }
        if ui.button("Load Subtitles…").clicked() {
            ui.close();
            self.load_subtitles(lib, item);
        }
        ui.separator();
        self.item_menu(ui, lib, item, std::slice::from_ref(&item.id));
    }

    /// Eagle's "Load Subtitles": a chosen `.srt` or `.vtt` is copied into
    /// the item's folder as `<name>.<ext>`, where the viewer finds it next
    /// time, and shown at once.
    fn load_subtitles(&mut self, lib: &Library, item: &Item) {
        let picked = rfd::FileDialog::new().set_title("Load Subtitles").add_filter("Subtitles", &["srt", "vtt"]).pick_file();
        let Some(src) = picked else { return };
        let Some(cues) = crate::subtitles::load(&src) else {
            self.flash("No subtitles found in that file");
            return;
        };
        let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let dest = item.original_path(lib.root()).with_extension(&ext);
        if src != dest
            && let Err(e) = std::fs::copy(&src, &dest)
        {
            self.flash(format!("Couldn't copy the subtitles: {e}"));
            return;
        }
        if self.video.loaded() == Some(item.id.as_str()) {
            self.video.subs = cues;
        }
    }

    /// Draws the viewer for a video and returns true: the player, or
    /// Eagle's "can't play" page when AVFoundation refuses the file. False
    /// for anything that isn't a video.
    pub(crate) fn video_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        let ctx = &parent.ctx().clone();
        if !media::is_video(&item.ext.to_lowercase()) {
            return false;
        }
        // A file that opened but then failed to load gives up the player.
        if self.video.loaded() == Some(item.id.as_str()) && self.video.player.as_ref().is_some_and(|v| v.error().is_some()) {
            self.video.failed.insert(item.id.clone());
            self.video.close();
        }
        if self.video.failed.contains(&item.id) || !self.video.open(lib, item, &self.prefs) {
            let ui = &mut crate::ui::widgets::viewer_ui(parent, screen, "video-viewer");
            ui.painter().rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            self.cannot_play(ui, lib, item, area);
            crate::ui::viewer::audio::top_bar(self, ui, item, bar);
            return true;
        }
        {
            let ui = &mut crate::ui::widgets::viewer_ui(parent, screen, "video-viewer");
            ui.painter().rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            ui.interact(area, ui.scope_id().with("video-bg"), Sense::click());
            self.video_view(ctx, ui, lib, item, area);
            // Flip and rotate in the bar, as in Eagle's viewer toolbar.
            let (name_x, mut right) = crate::ui::viewer::audio::bar_shell(self, ui, item, bar);
            let cy = bar.center().y;
            let tip = "Rotate Left (⇧R)\n⇧-click: Rotate Right";
            if icon_button(ui, Pos2::new(right - 4.0, cy), ic::ARROW_COUNTER_CLOCKWISE, 17.0, true, tip).clicked() {
                self.video.turns = rotate_step(self.video.turns, ui.input(|i| i.modifiers.shift));
            }
            right -= 30.0;
            if icon_button(ui, Pos2::new(right - 4.0, cy), ic::FLIP_HORIZONTAL, 17.0, true, "Flip (⇧F)").clicked() {
                self.video.flipped = !self.video.flipped;
            }
            right -= 30.0;
            crate::ui::viewer::audio::bar_name(ui, item, bar, name_x, right);
        }
        true
    }

    fn video_view(&mut self, ctx: &egui::Context, ui: &mut Ui, lib: &Library, item: &Item, area: Rect) {
        if let Some(tool) = self.video.tool.take() {
            self.frame_tool(item, tool);
        }
        let (fill, wheel_volume) = (self.prefs.video_fill, self.prefs.video_scroll == crate::prefs::VideoScroll::Volume);
        let presenting = self.modes.presenting;
        let mut present = false;
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
        // default "fit" mode), or enlarged to fill the stage when
        // Preferences stretches videos. Records without a size take the
        // file's.
        let stage = Rect::from_min_max(area.min, Pos2::new(area.right(), area.bottom() - CONTROLS));
        let natural = match (&vs.texture, info.width, info.height) {
            (_, w, h) if w > 0 && h > 0 => vec2(w as f32, h as f32),
            (Some(tex), _, _) => tex.size_vec2(),
            _ => Vec2::splat(1.0),
        };
        // A quarter turn swaps the picture's sides.
        let natural = if vs.turns % 2 == 1 { vec2(natural.y, natural.x) } else { natural };
        let fit = ((stage.width() - 80.0) / natural.x).min((stage.height() - 40.0) / natural.y).clamp(0.01, if fill { f32::INFINITY } else { 1.0 });
        let shown = Rect::from_center_size(stage.center(), natural * fit);
        match &vs.texture {
            Some(tex) => paint_turned(ui, tex, shown, corner_uvs(vs.turns, vs.flipped)),
            None => {
                p.text(stage.center(), Align2::CENTER_CENTER, "Loading…", font(13.0), t::p().text_muted);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        // Subtitles over the bottom of the picture, white on a dark band.
        if let Some(text) = crate::subtitles::showing(&vs.subs, pos) {
            // Each line centred on its own band, the last one near the bottom.
            let size = (shown.height() / 18.0).clamp(14.0, 30.0);
            let wrap = (shown.width() - 40.0).max(80.0);
            let galleys: Vec<_> = text.lines().map(|l| p.layout(l.to_string(), font(size), Color32::WHITE, wrap)).collect();
            let mut bottom = shown.bottom().min(stage.bottom()) - size * 0.8;
            for g in galleys.into_iter().rev() {
                let r = Rect::from_center_size(Pos2::new(shown.center().x, bottom - g.size().y / 2.0 - 3.0), g.size() + vec2(16.0, 6.0));
                p.rect_filled(r, CornerRadius::same(4), Color32::from_black_alpha(170));
                p.galley(r.min + vec2(8.0, 3.0), g, Color32::WHITE);
                bottom = r.top();
            }
        }
        // Clicking the picture plays or pauses; scrolling seeks, or sets
        // the volume.
        let canvas = ui.interact(stage, ui.scope_id().with("video-canvas"), Sense::click());
        if canvas.clicked() {
            v.toggle();
        }
        if canvas.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if wheel_volume {
                if delta != Vec2::ZERO {
                    vs.volume = scroll_volume(delta, vs.volume);
                    vs.muted = false;
                    v.set_muted(false);
                    v.set_volume(vs.volume);
                }
            } else {
                let d = scroll_seek(delta, duration);
                if d != 0.0 {
                    v.seek(pos + d);
                }
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

        // Hovering or dragging on the bar shows the picture at that moment
        // above it, with the time, as Eagle's seek bar does.
        if let Some(px) = pointer.filter(|_| (hot && near.is_none()) || seek.dragged())
            && duration > 0.0
        {
            let at = logic::seek_time(px.x, left, width, duration);
            let path = item.original_path(lib.root());
            let preview = vs.preview.get_or_insert_with(|| SeekPreview::start(ctx, path));
            let frame = if info.fps > 0.0 { 1.0 / f64::from(info.fps) } else { 0.04 };
            preview.ask(at, (duration / 400.0).max(frame));
            let aspect = if info.width > 0 && info.height > 0 { info.height as f32 / info.width as f32 } else { 9.0 / 16.0 };
            let size = vec2(PREVIEW_WIDTH, (PREVIEW_WIDTH * aspect).min(PREVIEW_WIDTH * 1.5));
            let x0 = bubble_left(px.x, size.x, area.left() + 8.0, area.right() - 8.0);
            let pic = Rect::from_min_size(Pos2::new(x0, track_y - 22.0 - size.y), size);
            let top = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("video-seek-preview")));
            top.rect_filled(pic.expand(2.0), CornerRadius::same(4), Color32::from_black_alpha(220));
            if let Some(tex) = preview.texture(ctx) {
                top.image(tex.id(), pic, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            let label = logic::format_duration(at);
            let g = top.layout_no_wrap(label, mono(11.5), Color32::WHITE);
            let lr = Rect::from_center_size(Pos2::new(pic.center().x, pic.bottom() - 10.0), g.size() + vec2(8.0, 2.0));
            top.rect_filled(lr, CornerRadius::same(3), Color32::from_black_alpha(170));
            top.galley(lr.min + vec2(4.0, 1.0), g, Color32::WHITE);
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
        // Back and forward act on press and repeat while held (Eagle: 2% of
        // the clip, at most 10 s back and 20 s forward).
        let now = ctx.input(|i| i.time);
        let mut x = play.right() + 20.0;
        let held = icon_button(ui, Pos2::new(x, cy), ic::ARROW_COUNTER_CLOCKWISE, 16.0, true, "Back (⌘←)").is_pointer_button_down_on();
        if vs.back.fire(held, now) {
            v.seek(v.current_time() - skip_step(duration, 10.0));
        }
        x += 28.0;
        let held = icon_button(ui, Pos2::new(x, cy), ic::ARROW_CLOCKWISE, 16.0, true, "Forward (⌘→)").is_pointer_button_down_on();
        if vs.forward.fire(held, now) {
            v.seek(v.current_time() + skip_step(duration, 20.0));
        }
        if vs.back.held() || vs.forward.held() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
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

        // Full screen at the right end: as Eagle's, it enters (or leaves)
        // presentation mode.
        let (glyph, tip) = if presenting { (ic::CORNERS_IN, "Exit Full Screen") } else { (ic::CORNERS_OUT, "Full Screen") };
        if icon_button(ui, Pos2::new(right - 6.0, cy), glyph, 17.0, true, tip).clicked() {
            present = true;
        }
        let vol = Rect::from_min_size(Pos2::new(right - 34.0 - 90.0, cy - 10.0), vec2(90.0, 20.0));
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

        // The volume tip at the stage's top right for a second after any
        // change (keys, wheel, slider, mute), as Eagle's player shows it.
        let level = (vs.volume, vs.muted);
        if vs.tip_seen.is_some_and(|seen| seen != level) {
            vs.tip_until = now + TIP_SECS;
        }
        vs.tip_seen = Some(level);
        if now < vs.tip_until {
            let percent = if vs.muted { 0 } else { (vs.volume * 100.0).round() as u32 };
            volume_tip(&p, Pos2::new(stage.right() - 12.0, stage.top() + 12.0), percent);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((vs.tip_until - now).max(0.01)));
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
        canvas.context_menu(|ui| self.video_menu(ui, lib, item));
        if present {
            self.toggle_presentation(ctx);
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
    fn positions_are_kept_per_item() {
        let mut p = Positions::default();
        p.set("A".into(), 12.5);
        p.set("B".into(), 3.0);
        assert_eq!((p.get("A"), p.get("B"), p.get("C")), (Some(12.5), Some(3.0), None));
        p.set("A".into(), 20.0);
        assert_eq!(p.get("A"), Some(20.0));
        assert_eq!(p.0.last().map(|x| x.0.as_str()), Some("A"), "the latest moves to the end");
        p.set("B".into(), 0.0);
        assert_eq!(p.get("B"), None, "a clip left at the start is forgotten");
        p.set("N".into(), f64::NAN);
        assert_eq!(p.get("N"), None);
        for n in 0..MAX_POSITIONS + 10 {
            p.set(format!("x{n}"), 1.0);
        }
        assert_eq!(p.0.len(), MAX_POSITIONS);
        assert_eq!(p.get("A"), None, "the oldest go first");
        assert_eq!(p.get(&format!("x{}", MAX_POSITIONS + 9)), Some(1.0));
        let json = serde_json::to_string(&Positions(vec![("A".into(), 1.5)])).unwrap();
        assert_eq!(json, r#"[["A",1.5]]"#);
    }

    #[test]
    fn wheel_volume() {
        assert_eq!(scroll_volume(Vec2::ZERO, 0.5), 0.5);
        assert!((scroll_volume(vec2(0.0, 200.0), 0.5) - 0.55).abs() < 1e-6, "scrolling up is louder");
        assert!((scroll_volume(vec2(0.0, -100.0), 0.5) - 0.45).abs() < 1e-6);
        assert_eq!(scroll_volume(vec2(0.0, 500.0), 0.98), 1.0);
        assert_eq!(scroll_volume(vec2(0.0, -500.0), 0.01), 0.0);
        let small = scroll_volume(vec2(0.0, 1.0), 0.5) - 0.5;
        assert!(small > 0.0 && small < 0.01);
    }

    #[test]
    fn short_clips_always_loop() {
        assert!(starts_looping(12.0, false, true));
        assert!(starts_looping(30.0, false, true));
        assert!(!starts_looping(30.5, false, true));
        assert!(starts_looping(600.0, true, true));
        assert!(starts_looping(0.0, false, true));
        assert!(!starts_looping(12.0, false, false), "short clips loop only when Preferences says so");
        assert!(starts_looping(12.0, true, false));
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
    fn cannot_play_lines_name_the_format() {
        assert!(cannot_play_line("MKV").contains(".mkv videos"));
        assert!(cannot_play_line("avi").contains("no player of its own"));
        assert!(cannot_play_line("mp4").starts_with("macOS couldn't play this video"));
    }

    #[test]
    fn rotate_turns_anticlockwise_unless_shifted() {
        assert_eq!(rotate_step(0, false), 3);
        assert_eq!(rotate_step(3, false), 2);
        assert_eq!(rotate_step(0, true), 1);
        assert_eq!(rotate_step(3, true), 0);
    }

    #[test]
    fn turned_and_mirrored_corners() {
        let p = |u: f32, v: f32| Pos2::new(u, v);
        assert_eq!(corner_uvs(0, false), [p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)]);
        // A clockwise quarter turn puts the picture's bottom left at the top left.
        assert_eq!(corner_uvs(1, false)[0], p(0.0, 1.0));
        assert_eq!(corner_uvs(2, false)[0], p(1.0, 1.0));
        assert_eq!(corner_uvs(3, false)[0], p(1.0, 0.0));
        // Mirroring swaps left and right.
        assert_eq!(corner_uvs(0, true), [p(1.0, 0.0), p(0.0, 0.0), p(0.0, 1.0), p(1.0, 1.0)]);
        // Mirrored then turned: what was the top right ends at the top right
        // after a half turn of the mirrored picture (rotate(180) scaleX(-1)
        // is a vertical flip).
        assert_eq!(corner_uvs(2, true), [p(0.0, 1.0), p(1.0, 1.0), p(1.0, 0.0), p(0.0, 0.0)]);
    }

    #[test]
    fn held_buttons_repeat_every_tenth_of_a_second() {
        let mut r = Repeat::default();
        assert!(!r.fire(false, 0.0));
        assert!(r.fire(true, 1.0), "acts on press");
        assert!(!r.fire(true, 1.05));
        assert!(r.fire(true, 1.1));
        assert!(!r.fire(true, 1.15));
        assert!(r.fire(true, 1.21));
        assert!(r.held());
        assert!(!r.fire(false, 1.3), "release stops it");
        assert!(!r.held());
        assert!(r.fire(true, 5.0), "a new press acts at once");
    }

    #[test]
    fn volume_tip_levels() {
        assert_eq!([0, 1, 29, 30, 99, 100].map(volume_level), [0, 1, 1, 2, 2, 3]);
    }

    #[test]
    fn hover_thumbnails_are_asked_for_sparingly() {
        assert!(needs_request(3.0, None, 0.5));
        assert!(!needs_request(3.2, Some(3.0), 0.5));
        assert!(needs_request(3.5, Some(3.0), 0.5));
        assert!(needs_request(2.4, Some(3.0), 0.5), "moving back counts too");
    }

    #[test]
    fn hover_bubbles_stay_on_screen() {
        assert_eq!(bubble_left(500.0, 160.0, 0.0, 1000.0), 420.0);
        assert_eq!(bubble_left(10.0, 160.0, 0.0, 1000.0), 0.0);
        assert_eq!(bubble_left(990.0, 160.0, 0.0, 1000.0), 840.0);
        // Wider than the room: pinned to the left edge.
        assert_eq!(bubble_left(50.0, 160.0, 0.0, 100.0), 0.0);
    }

    #[test]
    fn saved_frames_are_named_like_eagles() {
        assert_eq!(frame_item_name("Clip", 12.0), "Clip - 12.00");
        assert_eq!(frame_item_name("Clip", 7.4567), "Clip - 7.46");
        assert_eq!(frame_item_name("Clip", 0.125), "Clip - 0.13", "an exact half rounds up, as toFixed does");
        assert_eq!(frame_item_name("Clip", 1.005), "Clip - 1.00", "1.005 is just under in binary");
        assert_eq!(frame_item_name("Clip", -1.0), "Clip - 0.00");
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
