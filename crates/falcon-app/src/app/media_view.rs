//! The viewer for audio and font items: a waveform with playback controls,
//! and a font specimen drawn with the font itself (see the desktop app
//! chapter of the docs).

use eframe::egui::{
    self, Align2, Color32, FontData, FontFamily, FontId, FontInsert, InsertFontFamily, Pos2, Rect, RichText, Sense, Stroke,
    TextEdit, Ui, Vec2, vec2,
};
use falcon_library::fontpreview::Specimen;
use falcon_library::model::Item;
use falcon_library::{Library, fontmeta, media, search, waveform};

use super::{App, galley, icon_button, window_drag};
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
/// Sizes of the font viewer's waterfall, in points.
const WATERFALL: [f32; 9] = [72.0, 60.0, 48.0, 36.0, 30.0, 24.0, 18.0, 14.0, 12.0];
const CHARSET: [&str; 4] = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz", "0123456789", "!?&@#%*()[]{}.,:;'\"/-–—"];

/// The font viewer's state for the font on screen.
#[derive(Default)]
pub(super) struct FontView {
    id: String,
    /// The family the font is registered under, or `None` if egui can't
    /// read it.
    family: Option<FontFamily>,
    text: String,
    /// The text was typed by the user, so it stays when another font opens.
    edited: bool,
    size: f32,
}

/// Whether egui can parse a font, checked with the same parser it uses, as
/// it panics on fonts it can't read.
fn egui_can_read(data: &[u8]) -> bool {
    skrifa::FontRef::from_index(data, 0).is_ok()
}

/// Sample text in a script the font covers.
fn sample(item: &Item) -> String {
    let metas = item.extra.get("fontMetas").and_then(|m| m.as_object()).cloned().unwrap_or_default();
    match Specimen::for_metas(&metas) {
        Specimen::Latin => "Sphinx of black quartz, judge my vow",
        Specimen::SimplifiedChinese => "字体样张 汉字之美 Aa 0123",
        Specimen::TraditionalChinese => "字體樣張 漢字之美 Aa 0123",
        Specimen::Japanese => "あいうえお 日本語文 Aa 0123",
        Specimen::Korean => "가나다라마 Aa 0123",
    }
    .to_string()
}

/// The bar every viewer shares: back and the item's name at the left, the
/// open button at the right. Returns where the name starts and the x left
/// of what was drawn on the right, so a viewer can add its own controls
/// there before `bar_name` fits the name into what is left.
pub(super) fn bar_shell(app: &mut App, ui: &mut Ui, item: &Item, bar: Rect) -> (f32, f32) {
    let p = ui.painter().clone();
    p.rect_filled(bar, 0.0, t::p().main);
    window_drag(ui, bar, "viewer");
    let cy = bar.center().y;
    // Leave room for the window buttons only when the bar starts at the
    // window's left edge (no sidebar beside it).
    let x = bar.left() + logic::bar_inset(bar.left() <= ui.ctx().content_rect().left() + 1.0);
    if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 17.0, true, "Back (Esc)").clicked() {
        app.ws.viewer = None;
    }
    let mut right = bar.right() - 12.0;
    if app.ws.view.id == item.id {
        let file = app.ws.view.file.clone();
        right = app.viewer_open_button(ui, right, cy, &file);
    }
    (x + 22.0, right)
}

/// The item's name in the viewer bar, between `left` and `right`.
pub(super) fn bar_name(ui: &Ui, item: &Item, bar: Rect, left: f32, right: f32) {
    let name = format!("{}.{}", item.name, item.ext);
    let g = galley(ui, &name, font(14.0), t::p().text, (right - left - 12.0).clamp(0.0, 360.0));
    ui.painter().galley(Pos2::new(left, bar.center().y - g.size().y / 2.0), g, t::p().text);
}

/// The bar for viewers with no controls of their own in it (audio, font,
/// video, the iCloud wait). Returns the x left of its right-hand buttons.
pub(super) fn top_bar(app: &mut App, ui: &mut Ui, item: &Item, bar: Rect) -> f32 {
    let (left, right) = bar_shell(app, ui, item, bar);
    bar_name(ui, item, bar, left, right);
    right
}

impl App {
    /// Draws the viewer for an audio item, or a font egui can read, and
    /// returns true; returns false for anything else, which the image
    /// viewer shows.
    pub(super) fn media_viewer(&mut self, parent: &mut Ui, lib: &Library, item: &Item, screen: Rect) -> bool {
        let ctx = &parent.ctx().clone();
        let audio = media::is_audio(&item.ext);
        if !audio && !self.font_ready(ctx, lib, item) {
            return false;
        }
        {
            let ui = &mut super::viewer_ui(parent, screen, "media-viewer");
            ui.painter().rect_filled(screen, 0.0, t::p().main);
            let bar = Rect::from_min_size(screen.min, vec2(screen.width(), t::BAR_HEIGHT));
            let area = Rect::from_min_max(Pos2::new(screen.left(), bar.bottom()), screen.max);
            // Swallow clicks on the background so the grid below never gets them.
            ui.interact(area, ui.scope_id().with("media-bg"), Sense::click());
            if audio {
                self.audio_view(ctx, ui, lib, item, area);
            } else {
                self.font_view(ui, item, area);
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
        if let Some(e) = &self.audio.error {
            p.text(Pos2::new(wave.center().x, cy + 44.0), Align2::CENTER_CENTER, e, font(12.0), t::p().text_muted);
        }
        if playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    /// Registers the font with egui the first time it is viewed; true once
    /// egui has it (a frame later).
    fn font_ready(&mut self, ctx: &egui::Context, lib: &Library, item: &Item) -> bool {
        if search::type_group(&item.ext) != Some("font") {
            return false;
        }
        if self.ws.font_view.id != item.id {
            let family = FontFamily::Name(format!("falcon-font-{}", item.id).into());
            let data = std::fs::read(item.original_path(lib.root())).ok();
            let sfnt = data.as_deref().and_then(fontmeta::sfnt).map(|c| c.into_owned()).filter(|d| egui_can_read(d));
            let family = sfnt.map(|bytes| {
                // egui keeps a font once added; adding the same name again is a no-op.
                ctx.add_font(FontInsert {
                    name: format!("falcon-font-{}", item.id),
                    data: FontData::from_owned(bytes),
                    families: vec![InsertFontFamily { family: family.clone(), priority: egui::FontPriority::Highest }],
                });
                family
            });
            let edited = self.ws.font_view.edited;
            let text = if edited { std::mem::take(&mut self.ws.font_view.text) } else { sample(item) };
            let size = if self.ws.font_view.size > 0.0 { self.ws.font_view.size } else { 72.0 };
            self.ws.font_view = FontView { id: item.id.clone(), family, text, edited, size };
        }
        match &self.ws.font_view.family {
            Some(f) => {
                let ready = ctx.fonts(|fonts| fonts.definitions().families.contains_key(f));
                if !ready {
                    ctx.request_repaint();
                }
                ready
            }
            None => false,
        }
    }

    fn font_view(&mut self, ui: &mut Ui, item: &Item, area: Rect) {
        let Some(family) = self.ws.font_view.family.clone() else { return };
        let fv = &mut self.ws.font_view;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area.shrink2(vec2(48.0, 16.0))));
        let ui = &mut child;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Size").color(t::p().text_muted));
            ui.spacing_mut().slider_width = 220.0;
            ui.add(egui::Slider::new(&mut fv.size, 12.0..=200.0).suffix(" pt").max_decimals(0));
            if ui.button("Reset Text").clicked() {
                fv.text = sample(item);
                fv.edited = false;
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().id_salt("font-view").auto_shrink([false, false]).show(ui, |ui| {
            // The sample is editable in place, as in Eagle's font viewer.
            let sample_edit = ui.add(
                TextEdit::multiline(&mut fv.text)
                    .id_salt("font-sample")
                    .font(FontId::new(fv.size, family.clone()))
                    .text_color(t::p().text)
                    .frame(egui::Frame::NONE)
                    .desired_rows(1)
                    .desired_width(f32::INFINITY),
            );
            if sample_edit.changed() {
                fv.edited = true;
            }
            ui.add_space(20.0);
            ui.separator();
            for size in WATERFALL {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(44.0, size * 1.2), Sense::hover());
                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, format!("{size:.0}"), mono(11.0), t::p().text_muted);
                    let g = ui.painter().layout_no_wrap(fv.text.replace('\n', " "), FontId::new(size, family.clone()), t::p().text);
                    let (r, _) = ui.allocate_exact_size(vec2(g.size().x, size * 1.2), Sense::hover());
                    ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0), g, t::p().text);
                });
            }
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            for line in CHARSET {
                ui.label(RichText::new(line).font(FontId::new(36.0, family.clone())).color(t::p().text));
            }
            ui.add_space(24.0);
        });
    }
}

/// The inspector rows for audio, video, camera and font metadata, when the
/// item has them: `duration`, `bpm`, `rawMetas` and `fontMetas`.
pub(super) fn media_props(ui: &mut Ui, item: &Item) {
    if let Some(d) = search::duration(item) {
        super::prop(ui, "Duration", &logic::format_duration(d));
    }
    if let Some(b) = search::bpm(item) {
        // As stored (`119.985`), as Eagle's inspector shows it.
        super::prop(ui, "BPM", &b.to_string());
    }
    if let Some(raw) = item.extra.get("rawMetas") {
        let text = |k: &str| raw.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
        if let Some(c) = search::camera(item) {
            super::prop(ui, "Camera", c);
        }
        if let Some(ms) = raw.get("timestamp").and_then(|v| v.as_u64()).filter(|&t| t > 0) {
            super::prop(ui, "Date Taken", &logic::format_ms(ms));
        }
        if let Some(iso) = raw.get("isoSpeed").and_then(|v| v.as_u64()) {
            super::prop(ui, "ISO", &iso.to_string());
        }
        for (k, label) in [("focalLength", "Focal Length"), ("shutter", "Shutter"), ("aperture", "Aperture")] {
            if let Some(v) = text(k) {
                super::prop(ui, label, &v);
            }
        }
    }
    if let Some(metas) = item.extra.get("fontMetas").and_then(|m| m.as_object()) {
        if let Some(f) = search::font_family(item) {
            super::prop(ui, "Family", f);
        }
        let style = metas.get("weight").and_then(|v| v.as_str()).map_or_else(|| fontmeta::weight(metas), String::from);
        super::prop(ui, "Style", &style);
        if let Some(ps) = search::font_name(item, "postScriptName") {
            super::prop(ui, "PostScript", ps);
        }
        if let Some(v) = search::font_name(item, "version") {
            super::prop(ui, "Version", v);
        }
        if let Some(n) = metas.get("numGlyphs").and_then(|v| v.as_u64()) {
            super::prop(ui, "Glyphs", &n.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_egui_cannot_read_are_refused() {
        assert!(!egui_can_read(b"not a font"));
        assert!(!egui_can_read(&[]));
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") else { return };
        assert!(egui_can_read(&data));
        // A registered font lays out text in its own family from the next pass.
        let ctx = egui::Context::default();
        let family = FontFamily::Name("falcon-font-test".into());
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.label("before");
            ui.ctx().add_font(FontInsert {
                name: "falcon-font-test".into(),
                data: FontData::from_owned(data.clone()),
                families: vec![InsertFontFamily { family: family.clone(), priority: egui::FontPriority::Highest }],
            });
        });
        out.textures_delta.clear();
        let mut width = 0.0;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            assert!(ui.ctx().fonts(|f| f.definitions().families.contains_key(&family)));
            width = ui.painter().layout_no_wrap("Sphinx".into(), FontId::new(48.0, family.clone()), Color32::WHITE).size().x;
        });
        out.textures_delta.clear();
        assert!(width > 50.0, "{width}");
    }
}
