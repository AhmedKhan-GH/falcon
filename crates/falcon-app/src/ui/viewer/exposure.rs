//! The exposure slider in the image viewer's bar, for Radiance HDR and
//! OpenEXR items: they are tone mapped (Reinhard, sRGB) and the slider moves
//! the exposure in stops. The maths are `falcon_library::hdr`; the loader
//! makes the picture again (`Loader::set_exposure`).

use eframe::egui::{self, Align2, Pos2, Rect, Sense, Ui};
use falcon_library::hdr;
use falcon_library::model::Item;

use crate::app::App;
use crate::theme::{self as t, mono};

/// The slider's range in stops, and its step.
const RANGE: (f32, f32) = (-5.0, 5.0);
const STEP: f32 = 0.1;
const TRACK_W: f32 = 90.0;
const LABEL_W: f32 = 58.0;

/// `+1.0 EV`, `-0.5 EV`, `0.0 EV`.
fn label(stops: f32) -> String {
    if stops.abs() < 0.05 { "0.0 EV".to_string() } else { format!("{stops:+.1} EV") }
}

/// The slider's value for a pointer at fraction `f` (0..1) of the track.
fn stops_at(f: f32) -> f32 {
    let v = RANGE.0 + f.clamp(0.0, 1.0) * (RANGE.1 - RANGE.0);
    (v / STEP).round() * STEP
}

impl App {
    /// The exposure slider and its value, left of `right`, for an HDR or EXR
    /// item. Returns the x the next tool goes at (`right` for other items).
    pub(crate) fn exposure_bar(&mut self, ui: &mut Ui, item: &Item, right: f32, cy: f32) -> f32 {
        if !hdr::is_hdr(&item.ext.to_lowercase()) {
            return right;
        }
        let mut stops = self.image.exposure.clamp(RANGE.0, RANGE.1);
        let text = Rect::from_min_max(Pos2::new(right - LABEL_W, cy - 9.0), Pos2::new(right, cy + 9.0));
        let track = Rect::from_min_max(Pos2::new(text.left() - 8.0 - TRACK_W, cy - 9.0), Pos2::new(text.left() - 8.0, cy + 9.0));
        let resp = ui.interact(track, ui.scope_id().with("viewer-exposure-slider"), Sense::click_and_drag());
        let label_resp = ui.interact(text, ui.scope_id().with("viewer-exposure-label"), Sense::click());
        if resp.double_clicked() || label_resp.double_clicked() {
            stops = 0.0;
        } else if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
            stops = stops_at((p.x - track.left()) / track.width());
        }
        resp.on_hover_text("Exposure (double-click to reset)");
        label_resp.on_hover_text("Exposure in stops (double-click to reset)");
        let painter = ui.painter();
        let y = track.center().y;
        let f = (stops - RANGE.0) / (RANGE.1 - RANGE.0);
        let (zero, knob) = (track.left() + track.width() * (-RANGE.0 / (RANGE.1 - RANGE.0)), track.left() + f * track.width());
        painter.line_segment([Pos2::new(track.left(), y), Pos2::new(track.right(), y)], egui::Stroke::new(2.0, t::p().divider));
        painter.line_segment([Pos2::new(zero, y), Pos2::new(knob, y)], egui::Stroke::new(2.0, t::p().text_muted));
        painter.line_segment([Pos2::new(zero, y - 4.0), Pos2::new(zero, y + 4.0)], egui::Stroke::new(1.0, t::p().text_muted));
        painter.circle_filled(Pos2::new(knob, y), 6.0, egui::Color32::WHITE);
        painter.text(Pos2::new(text.right(), cy), Align2::RIGHT_CENTER, label(stops), mono(12.0), t::p().text_value);
        self.image.exposure = stops;
        self.ws.loader.set_exposure(hdr::stops_to_exposure(stops));
        track.left() - 14.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_steps_and_labels() {
        assert_eq!(stops_at(0.0), -5.0);
        assert_eq!(stops_at(1.0), 5.0);
        assert!(stops_at(0.5).abs() < 1e-4);
        assert!((stops_at(0.6) - 1.0).abs() < 1e-4);
        assert_eq!(label(1.0), "+1.0 EV");
        assert_eq!(label(-0.5), "-0.5 EV");
        assert_eq!(label(0.0), "0.0 EV");
    }
}
