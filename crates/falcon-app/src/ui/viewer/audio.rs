//! The viewer for audio and font items: a waveform with playback controls
//! here, and the font viewer in `ui/viewer/font.rs` (see the desktop app chapter
//! of the docs).

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2, vec2,
};
use falcon_library::model::Item;
use falcon_library::{Library, fontmeta, media, search, waveform};

use crate::app::App;
use crate::ui::widgets::{galley, icon_button, window_drag};
use crate::icons as ic;
use crate::logic;
use crate::theme::{self as t, font, mono};

const PLAY: &str = "\u{E3D0}";
const PAUSE: &str = "\u{E39E}";
const REPEAT: &str = "\u{E3F6}";
const SPEAKER: &str = "\u{E44A}";
/// Eagle's waveform colours: unplayed grey, played in the brand blue.
const WAVE: Color32 = Color32::from_rgb(0x7c, 0x7c, 0x7c);
const PLAYED: Color32 = Color32::from_rgb(waveform::COLOR[0], waveform::COLOR[1], waveform::COLOR[2]);
/// The bar every viewer shares: the panels button, back, the counter and
/// the item's name at the left; open, previous and next at the right.
/// Returns where the name starts and the x left of what was drawn on the
/// right, so a viewer can add its own controls there before `bar_name`
/// fits the name into what is left.
pub(crate) fn bar_shell(app: &mut App, ui: &mut Ui, item: &Item, bar: Rect) -> (f32, f32) {
    let p = ui.painter().clone();
    p.rect_filled(bar, 0.0, t::p().main);
    window_drag(ui, bar, "viewer");
    let cy = bar.center().y;
    // Leave room for the window buttons only when the bar starts at the
    // window's left edge (no sidebar beside it).
    let counter = logic::viewer_counter(&app.ws.order, &item.id);
    let mut x = bar.left() + logic::bar_inset(bar.left() <= ui.ctx().content_rect().left() + 1.0);
    app.panels_button(ui, Pos2::new(x, cy));
    x += 28.0;
    if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 17.0, true, "Back (Esc)").clicked() {
        app.ws.viewer = None;
    }
    x += 22.0;
    // Eagle's "n / total" counter.
    if let Some((n, total)) = counter {
        let g = ui.painter().layout_no_wrap(format!("{n} / {total}"), mono(12.0), t::p().text_value);
        let w = g.size().x;
        p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, t::p().text_value);
        x += w + 14.0;
    }
    // Previous and next at the right end, as in Eagle.
    let (first, last) = counter.map_or((true, true), |(n, total)| (n == 1, n == total));
    if icon_button(ui, Pos2::new(bar.right() - 22.0, cy), ic::CARET_RIGHT, 16.0, !last, "Next (→)").clicked() {
        app.viewer_step(1);
    }
    if icon_button(ui, Pos2::new(bar.right() - 48.0, cy), ic::CARET_LEFT, 16.0, !first, "Previous (←)").clicked() {
        app.viewer_step(-1);
    }
    let mut right = bar.right() - 68.0;
    if app.ws.view.id == item.id {
        let file = app.ws.view.file.clone();
        right = app.viewer_open_button(ui, right, cy, &file);
    }
    (x, right)
}

/// The item's name in the viewer bar, between `left` and `right`.
pub(crate) fn bar_name(ui: &Ui, item: &Item, bar: Rect, left: f32, right: f32) {
    let name = format!("{}.{}", item.name, item.ext);
    let g = galley(ui, &name, font(14.0), t::p().text, (right - left - 12.0).clamp(0.0, 360.0));
    ui.painter().galley(Pos2::new(left, bar.center().y - g.size().y / 2.0), g, t::p().text);
}

/// The bar for viewers with no controls of their own in it (audio, font,
/// video, the iCloud wait). Returns the x left of its right-hand buttons.
pub(crate) fn top_bar(app: &mut App, ui: &mut Ui, item: &Item, bar: Rect) -> f32 {
    let (left, right) = bar_shell(app, ui, item, bar);
    bar_name(ui, item, bar, left, right);
    right
}

impl App {
    /// Draws the viewer for an audio item, or a font egui can read, and
    /// returns true; returns false for anything else, which the image
    /// viewer shows.
    pub(crate) fn media_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        let ctx = &parent.ctx().clone();
        let audio = media::is_audio(&item.ext);
        if !audio && !self.font_ready(ctx, lib, item) {
            return false;
        }
        {
            let ui = &mut crate::ui::widgets::viewer_ui(parent, screen, "media-viewer");
            ui.painter().rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            // The background's right-click opens the item menu.
            let bg = ui.interact(area, ui.scope_id().with("media-bg"), Sense::click());
            bg.context_menu(|ui| {
                self.item_menu(ui, lib, item, std::slice::from_ref(&item.id));
            });
            if audio {
                self.audio_view(ctx, ui, lib, item, area);
            } else {
                self.font_view(ui, lib, item, area);
            }
            top_bar(self, ui, item, bar);
        }
        true
    }

    fn audio_view(&mut self, ctx: &egui::Context, ui: &mut Ui, lib: &Library, item: &Item, area: Rect) {
        let path = item.original_path(lib.root());
        // Eagle starts playing as soon as the item opens.
        self.audio.load(&item.id, &path, &item.ext, true);
        self.audio.tick();
        let duration = search::duration(item).unwrap_or(0.0);
        self.audio.duration = duration;
        let pos = self.audio.position();

        let w = (area.width() - 160.0).clamp(200.0, 1000.0);
        let wave = Rect::from_center_size(area.center() - vec2(0.0, 30.0), vec2(w, 160.0));
        let p = ui.painter().clone();
        match self.ws.waveforms.get(ctx, &item.id, &path, &item.ext) {
            None => {
                p.text(wave.center(), Align2::CENTER_CENTER, "Reading waveform…", font(13.0), t::p().text_muted);
            }
            Some(peaks) => {
                let bars = (w / 3.0) as usize;
                let peaks = peaks.map(|pk| waveform::rebucket(&pk, bars)).unwrap_or_else(|| vec![(0.0, 0.0); bars]);
                let loudest = peaks.iter().map(|&(l, h)| l.abs().max(h.abs())).fold(0.0f32, f32::max);
                let scale = if loudest > 0.0 { 1.0 / loudest } else { 0.0 };
                let played = logic::played_peaks(pos, duration, peaks.len());
                let mid = wave.center().y;
                let half = wave.height() / 2.0;
                for (n, &(l, h)) in peaks.iter().enumerate() {
                    let x = wave.left() + n as f32 * 3.0 + 1.0;
                    let (top, bottom) = (mid - (h * scale * half).max(0.5), mid - (l * scale * half).min(-0.5));
                    let color = if n < played { PLAYED } else { WAVE };
                    p.line_segment([Pos2::new(x, top), Pos2::new(x, bottom)], Stroke::new(2.0, color));
                }
            }
        }
        // The waveform is the seek bar: click or drag to move the playhead.
        let seek = ui.interact(wave, ui.scope_id().with("seek"), Sense::click_and_drag());
        if (seek.clicked() || seek.dragged())
            && let Some(at) = seek.interact_pointer_pos()
        {
            self.audio.seek(logic::seek_time(at.x, wave.left(), wave.width(), duration));
        }
        if seek.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let x = logic::playhead_x(pos, duration, wave.left(), wave.width());
        p.line_segment([Pos2::new(x, wave.top()), Pos2::new(x, wave.bottom())], Stroke::new(1.0, PLAYED));

        // Controls: play/pause, time, loop and volume.
        let cy = wave.bottom() + 40.0;
        let play = Rect::from_center_size(Pos2::new(wave.left() + 20.0, cy), Vec2::splat(40.0));
        let resp = ui.interact(play, ui.scope_id().with("play"), Sense::click());
        p.circle_filled(play.center(), 20.0, if resp.hovered() { t::p().primary } else { t::p().brand });
        let playing = self.audio.is_playing();
        let glyph = if playing { PAUSE } else { PLAY };
        p.text(play.center(), Align2::CENTER_CENTER, glyph, FontId::new(18.0, t::fill_family()), Color32::WHITE);
        if resp.on_hover_text("Play/Pause (Space)").clicked() {
            self.audio.toggle();
        }
        p.text(Pos2::new(play.right() + 14.0, cy), Align2::LEFT_CENTER, logic::time_label(pos, duration), mono(13.0), t::p().text_value);

        let vol = Rect::from_min_size(Pos2::new(wave.right() - 110.0, cy - 10.0), vec2(110.0, 20.0));
        let mut v = self.audio.volume;
        let mut vui = ui.new_child(egui::UiBuilder::new().max_rect(vol));
        vui.spacing_mut().slider_width = 110.0;
        if vui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).on_hover_text("Volume").changed() {
            self.audio.set_volume(v);
        }
        let spk = Pos2::new(vol.left() - 16.0, cy);
        ui.painter().text(spk, Align2::CENTER_CENTER, SPEAKER, font(16.0), t::p().text_muted);
        let lp = Pos2::new(spk.x - 34.0, cy);
        let color = if self.audio.looping { t::p().primary } else { t::p().text_muted };
        let lresp = ui.interact(Rect::from_center_size(lp, Vec2::splat(26.0)), ui.scope_id().with("loop"), Sense::click());
        if lresp.hovered() {
            ui.painter().rect_filled(Rect::from_center_size(lp, Vec2::splat(26.0)), t::RADIUS, t::p().row_hover);
        }
        ui.painter().text(lp, Align2::CENTER_CENTER, REPEAT, font(16.0), color);
        if lresp.on_hover_text("Loop").clicked() {
            self.audio.looping = !self.audio.looping;
        }
        // Speed menu left of loop (⇧. and ⇧, step through 0.25–8×).
        let sp = Rect::from_center_size(Pos2::new(lp.x - 40.0, cy), vec2(44.0, 24.0));
        let rate = self.audio.speed();
        let name = |r: f32| format!("{}×", format!("{r:.2}").trim_end_matches('0').trim_end_matches('.'));
        let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(sp));
        egui::containers::menu::MenuButton::new(RichText::new(name(rate)).font(mono(12.0))).ui(&mut sui, |ui| {
            for r in crate::audio::MENU_RATES.iter().rev() {
                if ui.selectable_label((r - rate).abs() < 1e-3, name(*r)).clicked() {
                    self.audio.set_speed(*r);
                    ui.close();
                }
            }
        });
        // The volume tip, as in the video viewer.
        let now = ctx.input(|i| i.time);
        if self.audio.tip_seen.is_some_and(|seen| seen != self.audio.volume) {
            self.audio.tip_until = now + crate::ui::viewer::video::TIP_SECS;
        }
        self.audio.tip_seen = Some(self.audio.volume);
        if now < self.audio.tip_until {
            crate::ui::viewer::video::volume_tip(&p, Pos2::new(area.right() - 12.0, area.top() + 12.0), (self.audio.volume * 100.0).round() as u32);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((self.audio.tip_until - now).max(0.01)));
        }
        if let Some(e) = &self.audio.error {
            p.text(Pos2::new(wave.center().x, cy + 44.0), Align2::CENTER_CENTER, e, font(12.0), t::p().text_muted);
        }
        if playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

}

/// The inspector rows for audio, video, camera and font metadata, when the
/// item has them: `duration`, `bpm`, `rawMetas` and `fontMetas`.
pub(crate) fn media_props(ui: &mut Ui, item: &Item) {
    if let Some(d) = search::duration(item) {
        crate::ui::widgets::prop(ui, "Duration", &logic::format_duration(d));
    }
    if let Some(b) = search::bpm(item) {
        // As stored (`119.985`), as Eagle's inspector shows it.
        crate::ui::widgets::prop(ui, "BPM", &b.to_string());
    }
    if let Some(raw) = item.extra.get("rawMetas") {
        let text = |k: &str| raw.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
        if let Some(c) = search::camera(item) {
            crate::ui::widgets::prop(ui, "Camera", c);
        }
        if let Some(ms) = raw.get("timestamp").and_then(|v| v.as_u64()).filter(|&t| t > 0) {
            crate::ui::widgets::prop(ui, "Date Taken", &logic::format_ms(ms));
        }
        if let Some(iso) = raw.get("isoSpeed").and_then(|v| v.as_u64()) {
            crate::ui::widgets::prop(ui, "ISO", &iso.to_string());
        }
        for (k, label) in [("focalLength", "Focal Length"), ("shutter", "Shutter"), ("aperture", "Aperture")] {
            if let Some(v) = text(k) {
                crate::ui::widgets::prop(ui, label, &v);
            }
        }
    }
    if let Some(metas) = item.extra.get("fontMetas").and_then(|m| m.as_object()) {
        if let Some(f) = search::font_family(item) {
            crate::ui::widgets::prop(ui, "Family", f);
        }
        let style = metas.get("weight").and_then(|v| v.as_str()).map_or_else(|| fontmeta::weight(metas), String::from);
        crate::ui::widgets::prop(ui, "Style", &style);
        if let Some(ps) = search::font_name(item, "postScriptName") {
            crate::ui::widgets::prop(ui, "PostScript", ps);
        }
        if let Some(v) = search::font_name(item, "version") {
            crate::ui::widgets::prop(ui, "Version", v);
        }
        if let Some(n) = metas.get("numGlyphs").and_then(|v| v.as_u64()) {
            crate::ui::widgets::prop(ui, "Glyphs", &n.to_string());
        }
    }
}
