//! Draws the screenshot pop-up (`crate::capture_notice`): a card at the
//! window's top right with the thumbnail, a title and the screenshot's
//! name. It slides in, stays a moment (longer while the pointer is on it),
//! has a close button while hovered, and clicking it shows the screenshot
//! in the library.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, vec2};

use crate::app::App;
use crate::capture_notice::{self as cn, THUMB};
use crate::locale::tr;
use crate::theme::{self as t, font};
use crate::ui::widgets::galley;

const WIDTH: f32 = 300.0;
const PAD: f32 = 8.0;
const TOP: f32 = 12.0;
const RIGHT: f32 = 20.0;

impl App {
    pub(crate) fn capture_notice_ui(&mut self, ctx: &egui::Context) {
        let Some(shown) = self.system.notice.current.as_mut() else { return };
        let elapsed = shown.at.elapsed().as_secs_f32();
        if cn::expired(elapsed, shown.hovered) {
            self.system.notice.current = None;
            return;
        }
        shown.poll(ctx);
        // Frames while it slides, then a check now and then for expiry.
        ctx.request_repaint_after(std::time::Duration::from_millis(if elapsed < cn::SLIDE_IN { 16 } else { 100 }));
        let p = t::p();
        let off = (1.0 - cn::slide(elapsed)) * (WIDTH + RIGHT);
        let mut closed = false;
        egui::Area::new(egui::Id::NULL.with("capture-notice")).order(egui::Order::Tooltip).anchor(Align2::RIGHT_TOP, vec2(-RIGHT + off, TOP)).show(ctx, |ui| {
            let size = vec2(WIDTH, THUMB + PAD * 2.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let pt = ui.painter();
            let round = CornerRadius::same(8);
            pt.rect_filled(rect.translate(vec2(0.0, 2.0)).expand(1.0), round, egui::Color32::from_black_alpha(60));
            pt.rect_filled(rect, round, p.panel);
            pt.rect_stroke(rect, round, Stroke::new(1.0, p.card_border), StrokeKind::Inside);
            let thumb = Rect::from_min_size(rect.min + vec2(PAD, PAD), vec2(THUMB, THUMB));
            let tr_round = CornerRadius::same(6);
            match &shown.texture {
                Some(tex) => {
                    let [u0, v0, u1, v1] = cn::cover_uv(shown.size[0], shown.size[1]);
                    egui::Image::new(tex).uv(Rect::from_min_max(Pos2::new(u0, v0), Pos2::new(u1, v1))).corner_radius(tr_round).paint_at(ui, thumb);
                }
                None => {
                    pt.rect_filled(thumb, tr_round, p.button);
                }
            }
            pt.rect_stroke(thumb, tr_round, Stroke::new(1.0, p.divider), StrokeKind::Inside);
            let x = thumb.right() + PAD;
            let room = rect.right() - PAD * 2.0 - 16.0 - x;
            let title = galley(ui, tr("Screenshot added"), font(14.0), p.text, room);
            let name = galley(ui, &shown.name, font(13.0), p.text_muted, room);
            let block = title.size().y + 4.0 + name.size().y;
            let y = rect.center().y - block / 2.0;
            pt.galley(Pos2::new(x, y), title.clone(), p.text);
            pt.galley(Pos2::new(x, y + title.size().y + 4.0), name, p.text_muted);
            // `contains_pointer`, not `hovered`: the close button on top of
            // the card takes the hover, and must not make itself vanish.
            shown.hovered = resp.contains_pointer();
            if shown.hovered {
                let c = Rect::from_center_size(Pos2::new(rect.right() - PAD - 6.0, rect.top() + PAD + 6.0), vec2(16.0, 16.0));
                let close = ui.interact(c, ui.scope_id().with("close"), Sense::click());
                let col = if close.hovered() { p.text } else { p.text_muted };
                let m = c.shrink(4.0);
                ui.painter().line_segment([m.left_top(), m.right_bottom()], Stroke::new(1.5, col));
                ui.painter().line_segment([m.right_top(), m.left_bottom()], Stroke::new(1.5, col));
                closed = close.clicked();
            }
            if resp.clicked() && !closed {
                shown.clicked = true;
            }
        });
        if closed {
            self.system.notice.current = None;
        }
    }
}
