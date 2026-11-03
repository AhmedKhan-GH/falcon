//! Drawing the background task status (`crate::tasks`) beside the library
//! name: imports, or else a running AI step.

use std::f32::consts::TAU;

use eframe::egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

use crate::app::App;
use crate::theme as t;
use crate::ui::viewer::audio::{PAUSE, PLAY};
use crate::ui::widgets::icon;

impl App {
    /// A spinner centred on `center` while imports are running (a pause
    /// glyph under the pointer), a resume glyph while they are paused; a
    /// click toggles. Nothing while the queue is empty.
    pub(crate) fn task_status(&mut self, ui: &mut Ui, center: Pos2) {
        let status = {
            let pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            crate::tasks::status(pr.total, pr.done, pr.paused)
        };
        let Some(status) = status else {
            if let Some(tip) = self.ws.ai_run.as_ref().filter(|r| r.working()).map(|r| r.tooltip()) {
                let rect = Rect::from_center_size(center, Vec2::splat(20.0));
                ui.interact(rect, ui.scope_id().with("ai-run-status"), Sense::hover()).on_hover_text(tip);
                spinner(ui, center, t::p().text_muted);
            }
            return;
        };
        let rect = Rect::from_center_size(center, Vec2::splat(20.0));
        let resp = ui.interact(rect, ui.scope_id().with("task-status"), Sense::click()).on_hover_text(format!("{}\n{}", status.label(), status.action()));
        if resp.clicked() {
            let mut pr = self.ws.progress.lock().unwrap_or_else(|p| p.into_inner());
            pr.paused = !pr.paused;
        }
        let color = if resp.hovered() { t::p().text } else { t::p().text_muted };
        if status.paused {
            icon(ui, center, PLAY, 13.0, color);
        } else if resp.hovered() {
            icon(ui, center, PAUSE, 13.0, color);
        } else {
            spinner(ui, center, color);
        }
    }
}

/// A three-quarter ring turning once a second.
fn spinner(ui: &mut Ui, center: Pos2, color: Color32) {
    let start = ui.input(|i| i.time) as f32 * TAU;
    let points: Vec<Pos2> = (0..=24).map(|n| start + n as f32 / 24.0 * TAU * 0.75).map(|a| center + 6.0 * Vec2::angled(a)).collect();
    ui.painter().circle_stroke(center, 6.0, Stroke::new(2.0, t::p().divider));
    ui.painter().add(Shape::line(points, Stroke::new(2.0, color)));
    ui.ctx().request_repaint();
}
