//! Draws the screenshot pop-up (`crate::capture_notice`): a window of its
//! own at the screen's top right, holding a card with the thumbnail, a
//! title and a hint line. The card slides in, stays a moment (longer while
//! the pointer is on it), has a close button while hovered, can be dragged
//! away to the right, and clicking it shows the screenshot in the library.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, vec2};

use crate::app::App;
use crate::capture_notice::{self as cn, PAD, Shown, THUMB};
use crate::locale::tr;
use crate::theme::{self as t, font};
use crate::ui::widgets::galley;

/// Side of the close button.
const CLOSE: f32 = 16.0;

/// The close button of a card at `card`.
fn close_rect(card: Rect) -> Rect {
    Rect::from_center_size(Pos2::new(card.right() - PAD - 6.0, card.top() + PAD + 6.0), vec2(CLOSE, CLOSE))
}

impl App {
    pub(crate) fn capture_notice_ui(&mut self, ctx: &egui::Context) {
        let Some(shown) = self.system.notice.current.as_mut() else { return };
        let elapsed = shown.at.elapsed().as_secs_f32();
        shown.motion.settle(elapsed);
        if shown.motion.gone(elapsed) || cn::expired(elapsed, shown.hovered || shown.motion.dragging()) {
            // Not asking for the viewport closes its window.
            self.system.notice.current = None;
            return;
        }
        shown.poll(ctx);
        // Frames while it moves, then often enough to see the pointer come
        // onto it (`pointer_on_screen`) and to notice its time is up. The
        // viewport draws within this one's frame, so they are asked of this.
        let moving = elapsed < cn::SLIDE_IN || shown.motion.animating();
        ctx.request_repaint_after(std::time::Duration::from_millis(if moving { 16 } else { 50 }));
        let zoom = ctx.zoom_factor();
        let pos = *shown.place.get_or_insert_with(|| {
            let area = cn::visible_area().unwrap_or_else(|| {
                let size = ctx.input(|i| i.viewport().monitor_size).unwrap_or(vec2(1280.0, 800.0));
                Rect::from_min_size(Pos2::ZERO, size * zoom)
            });
            cn::window_pos(area, zoom)
        });
        let title = format!("{} {}", cn::WINDOW_TITLE, shown.serial);
        let builder = egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size(cn::window_size())
            .with_position(pos)
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_active(false);
        let id = egui::ViewportId::from_hash_of(("falcon-capture-notice", shown.serial));
        let closed = ctx.show_viewport_immediate(id, builder, |ui, _class| {
            if !shown.panel {
                shown.panel = cn::make_panel(&title);
                // Seen on macOS: winit's place at creation is 32 pt too low.
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
            card(ui, shown, elapsed, pos, zoom)
        });
        if closed {
            self.system.notice.current = None;
        }
    }
}

/// Follows the pointer and draws the card. True when its close button was
/// clicked.
fn card(ui: &mut Ui, shown: &mut Shown, elapsed: f32, pos: Pos2, zoom: f32) -> bool {
    let slide = (1.0 - cn::slide(elapsed)) * cn::TRAVEL;
    let (pressed, down, speed, latest) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.velocity().x, i.pointer.latest_pos().or(i.pointer.interact_pos())));
    let mouse = cn::pointer_on_screen().map(|at| cn::screen_to_window(at, pos, zoom)).or(latest);
    let held = cn::card_rect(slide + shown.motion.offset(elapsed));
    let mut closed = false;
    if pressed && let Some(p) = latest.filter(|p| held.contains(*p)) {
        if shown.hovered && close_rect(held).contains(p) {
            shown.on_close = true;
        } else {
            shown.motion.press(p, elapsed);
        }
    }
    if let Some(p) = latest {
        if shown.motion.dragging() {
            if down {
                shown.motion.track(p);
            } else if shown.motion.release(p, speed, elapsed) {
                shown.clicked = true;
            }
        }
        if shown.on_close && !down {
            shown.on_close = false;
            closed = close_rect(held).contains(p);
        }
    }

    let rect = cn::card_rect(slide + shown.motion.offset(elapsed));
    shown.hovered = mouse.is_some_and(|m| rect.contains(m));
    if shown.hovered {
        let icon = if shown.motion.dragging() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::PointingHand };
        ui.ctx().set_cursor_icon(icon);
    }

    let p = t::p();
    let pt = ui.painter();
    let round = CornerRadius::same(8);
    pt.add(egui::Shadow { offset: [0, 3], blur: 14, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(rect, round));
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
    // The title leaves room for the close button; the hint is below it.
    let title = galley(ui, tr("Screenshot added"), font(14.0), p.text, rect.right() - PAD * 2.0 - CLOSE - x);
    let hint = galley(ui, tr("Click to show, drag to dismiss"), font(12.0), p.text_muted, rect.right() - PAD - x);
    let block = title.size().y + 4.0 + hint.size().y;
    let y = rect.center().y - block / 2.0;
    pt.galley(Pos2::new(x, y), title.clone(), p.text);
    pt.galley(Pos2::new(x, y + title.size().y + 4.0), hint, p.text_muted);
    if shown.hovered {
        let c = close_rect(rect);
        let col = if mouse.is_some_and(|m| c.contains(m)) { p.text } else { p.text_muted };
        let m = c.shrink(4.0);
        pt.line_segment([m.left_top(), m.right_bottom()], Stroke::new(1.5, col));
        pt.line_segment([m.right_top(), m.left_bottom()], Stroke::new(1.5, col));
    }
    closed
}
