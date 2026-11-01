//! Eagle's frame-by-frame GIF player: a bar under the picture with play
//! and pause, frame steps, the frame counter, a scrub bar with a loop range
//! and frame thumbnails, the speed menu and a menu to set the frame as the
//! thumbnail. It shows for GIFs when the toolbar button or the "advanced
//! GIF viewer" preference turns it on. Also here: GIF and animated WebP
//! cards playing in the grid while hovered (or always, with Eagle's
//! "always play" preference), and in the hover preview. Frames come from
//! `anim.rs`.

use eframe::egui::{self, Align2, Color32, CornerRadius, Event, Pos2, Rect, Response, RichText, Sense, Stroke, TextureHandle, Ui, Vec2, vec2};
use falcon_library::Library;
use falcon_library::model::Item;

use super::image_edit::Op;
use super::{Action, App, icon_button};
use crate::anim::{self, State};
use crate::icons as ic;
use crate::shortcuts::{Command, PlayerKey};
use crate::theme::{self as t, font, mono};

const PLAY: &str = "\u{E3D0}";
const PAUSE: &str = "\u{E39E}";
const FILM_STRIP: &str = "\u{E792}";
const DOTS_THREE: &str = "\u{E1FE}";
/// Height of the player's bar under the picture.
pub(super) const BAR: f32 = 48.0;
/// Scroll distance over the scrub bar that steps one frame.
const WHEEL_STEP: f32 = 24.0;

/// What is being dragged on the scrub bar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Seek,
    First,
    Last,
}

/// The player bar's state for the open item.
#[derive(Default)]
pub(super) struct GifUi {
    drag: Option<Drag>,
    /// Whether to play again when the drag ends.
    resume: bool,
    wheel: f32,
}

fn is_gif(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("gif")
}

impl App {
    /// Whether the frame player shows for a file of this extension.
    fn gif_player_on(&self, ext: &str) -> bool {
        is_gif(ext) && (self.prefs.gif_player || self.ws.gif_player)
    }

    /// The viewer's area split into the picture's part and, when the player
    /// shows, its bar along the bottom.
    pub(super) fn gif_split(&self, item: &Item, area: Rect) -> (Rect, Option<Rect>) {
        if !self.gif_player_on(&item.ext) {
            return (area, None);
        }
        let bar = Rect::from_min_max(Pos2::new(area.left(), area.bottom() - BAR), area.max);
        (Rect::from_min_max(area.min, Pos2::new(area.right(), bar.top())), Some(bar))
    }

    /// The viewer toolbar's button for the frame player (GIFs only), drawn
    /// at `x`; returns where the next tool goes.
    pub(super) fn gif_toggle_button(&mut self, ui: &mut Ui, item: &Item, x: f32, cy: f32) -> f32 {
        if !is_gif(&item.ext) {
            return x;
        }
        let on = self.gif_player_on(&item.ext);
        let c = Pos2::new(x, cy);
        if on {
            ui.painter().rect_filled(Rect::from_center_size(c, Vec2::splat(26.0)), t::RADIUS, t::p().primary.gamma_multiply(0.35));
        }
        if icon_button(ui, c, FILM_STRIP, 16.0, true, "Frame-by-Frame Player").clicked() {
            self.ws.gif_player = !self.ws.gif_player;
        }
        x - 34.0
    }

    /// The player keys while the frame player shows, read before the app's
    /// own keys so Space plays and pauses instead of closing the viewer.
    pub(super) fn gif_keys(&mut self, ctx: &egui::Context) {
        if self.ws.viewer.is_none() {
            // Eagle turns the player off when the viewer closes.
            self.ws.gif_player = false;
            return;
        }
        // The grid is hidden: its cards stop.
        self.ws.cards.clear();
        let ext = self.ws.view.file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let ready = self.ws.view.anim.as_ref().is_some_and(|a| a.state == State::Ready);
        if !ready || !self.gif_player_on(&ext) || self.prefs_window.open || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let mut pressed = Vec::new();
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                    let keys: Vec<egui::Key> = std::iter::once(*key).chain(*physical_key).collect();
                    match self.shortcuts.player(&keys, *modifiers) {
                        Some(k @ (PlayerKey::Toggle | PlayerKey::Frames(_) | PlayerKey::Speed(_))) => {
                            pressed.push(k);
                            false
                        }
                        _ => true,
                    }
                }
                _ => true,
            })
        });
        let Some(p) = self.ws.view.anim.as_mut() else { return };
        for k in pressed {
            match k {
                PlayerKey::Toggle => p.toggle(),
                PlayerKey::Frames(n) => p.step(n.into()),
                PlayerKey::Speed(up) => p.speed = anim::step_speed(p.speed, up),
                _ => {}
            }
        }
    }

    /// Draws the player's bar in `bar`. A click on the picture (`canvas`)
    /// plays or pauses, as in Eagle.
    pub(super) fn gif_toolbar(&mut self, ui: &mut Ui, item: &Item, bar: Rect, canvas: &Response) {
        let tips = [
            ("Previous Frame", self.shortcut_text(ui.ctx(), Command::Player(PlayerKey::Frames(-1)))),
            ("Next Frame", self.shortcut_text(ui.ctx(), Command::Player(PlayerKey::Frames(1)))),
            ("Play/Pause", self.shortcut_text(ui.ctx(), Command::Player(PlayerKey::Toggle))),
        ]
        .map(|(l, k)| if k.is_empty() { l.to_string() } else { format!("{l} ({k})") });
        let editing = self.ws.tools.crop.is_some() || self.ws.tools.comment_mode;
        let p = ui.painter().clone();
        p.rect_filled(bar, 0.0, t::p().panel);
        p.hline(bar.x_range(), bar.top(), Stroke::new(1.0, t::p().divider));
        let cy = bar.center().y;
        let Some(player) = self.ws.view.anim.as_mut() else { return };
        if player.state != State::Ready {
            let text = match &player.state {
                State::Failed(e) => format!("Can't play this GIF: {e}"),
                State::Still => "This GIF has one frame".to_string(),
                _ => "Reading frames…".to_string(),
            };
            p.text(Pos2::new(bar.left() + 20.0, cy), Align2::LEFT_CENTER, text, font(12.5), t::p().text_muted);
            return;
        }
        // Under the controls, so they keep their clicks.
        let bar_resp = ui.interact(bar, ui.scope_id().with("gif-bar"), Sense::click());
        let g = &mut self.ws.view.gif;
        if canvas.clicked() && !editing {
            player.toggle();
        }
        let total = player.count();

        // Play, previous and next frame, the counter.
        let mut x = bar.left() + 22.0;
        if icon_button(ui, Pos2::new(x, cy), if player.playing { PAUSE } else { PLAY }, 17.0, true, &tips[2]).clicked() {
            player.toggle();
        }
        x += 30.0;
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_LEFT, 15.0, true, &tips[0]).clicked() {
            player.step(-1);
        }
        x += 26.0;
        if icon_button(ui, Pos2::new(x, cy), ic::CARET_RIGHT, 15.0, true, &tips[1]).clicked() {
            player.step(1);
        }
        x += 22.0;
        let (cur, of) = anim::counter_label(player.frame(), total);
        let r = p.text(Pos2::new(x, cy), Align2::LEFT_CENTER, cur, mono(12.5), t::p().text_value);
        let r = p.text(Pos2::new(r.right() + 5.0, cy), Align2::LEFT_CENTER, of, mono(12.5), t::p().text_muted);

        // The speed and more menus on the right.
        let more = Pos2::new(bar.right() - 22.0, cy);
        let sp = Rect::from_center_size(Pos2::new(more.x - 42.0, cy), vec2(48.0, 24.0));
        let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(sp));
        egui::containers::menu::MenuButton::new(RichText::new(anim::speed_label(player.speed)).font(mono(12.0))).ui(&mut sui, |ui| {
            for s in anim::SPEEDS.iter().rev() {
                if ui.selectable_label((s - player.speed).abs() < 1e-3, anim::speed_label(*s)).clicked() {
                    player.speed = *s;
                    ui.close();
                }
            }
        });
        let mut set_thumb = false;
        let mut reset = false;
        let menu = |ui: &mut Ui, has_range: bool, set_thumb: &mut bool, reset: &mut bool| {
            if ui.button("Set Frame as Thumbnail").clicked() {
                *set_thumb = true;
                ui.close();
            }
            if ui.add_enabled(has_range, egui::Button::new("Reset Loop Range")).clicked() {
                *reset = true;
                ui.close();
            }
        };
        let has_range = player.range.is_some();
        let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(more, Vec2::splat(26.0))));
        egui::containers::menu::MenuButton::new(RichText::new(DOTS_THREE).font(font(17.0))).ui(&mut mui, |ui| {
            menu(ui, has_range, &mut set_thumb, &mut reset);
        });

        // The scrub bar between them.
        let left = r.right() + 18.0;
        let right = sp.left() - 16.0;
        let width = (right - left).max(1.0);
        let hit = Rect::from_min_max(Pos2::new(left - 6.0, cy - 12.0), Pos2::new(right + 6.0, cy + 12.0));
        let seek = ui.interact(hit, ui.scope_id().with("gif-scrub"), Sense::click_and_drag());
        let range = player.range.unwrap_or(anim::LoopRange { first: 0, last: total - 1 });
        let handle = |at: usize| Rect::from_center_size(Pos2::new(anim::frame_x(at, total, left, width), cy), vec2(10.0, 22.0));
        let first = ui.interact(handle(range.first), ui.scope_id().with("gif-first"), Sense::drag());
        let last = ui.interact(handle(range.last), ui.scope_id().with("gif-last"), Sense::drag());
        let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
        let under = pointer.map(|p| anim::frame_at_x(p.x, left, width, total));

        // Right-click on the bar opens the same menu, pausing.
        if bar_resp.secondary_clicked() || seek.secondary_clicked() {
            player.playing = false;
        }
        for resp in [&bar_resp, &seek] {
            resp.context_menu(|ui| menu(ui, has_range, &mut set_thumb, &mut reset));
        }

        for (resp, kind) in [(&first, Drag::First), (&last, Drag::Last), (&seek, Drag::Seek)] {
            if resp.drag_started() {
                // Eagle only seeks inside the loop range.
                if kind == Drag::Seek && under.is_some_and(|f| f < range.first || f > range.last) {
                    continue;
                }
                g.drag = Some(kind);
                g.resume = player.playing;
                player.playing = false;
            }
        }
        if let Some(d) = g.drag {
            if let Some(f) = under {
                player.seek(if d == Drag::Seek { f.clamp(range.first, range.last) } else { f });
            }
            // Dropped: a handle sets its end of the range at the frame shown.
            if !(first.dragged() || last.dragged() || seek.dragged()) {
                let f = player.frame();
                match d {
                    Drag::First => player.set_range(Some(anim::range_with_first(player.range, f, total))),
                    Drag::Last => player.set_range(Some(anim::range_with_last(player.range, f))),
                    Drag::Seek => {}
                }
                player.playing = g.resume;
                g.drag = None;
            }
        }
        if seek.clicked()
            && let Some(f) = under.filter(|f| (range.first..=range.last).contains(f))
        {
            player.seek(f);
        }
        // Scrolling over the bar steps frames (up is forward).
        if seek.hovered() {
            g.wheel += ui.input(|i| i.smooth_scroll_delta.y + i.smooth_scroll_delta.x);
            if g.wheel.abs() >= WHEEL_STEP {
                player.step(if g.wheel > 0.0 { 1 } else { -1 });
                g.wheel = 0.0;
            }
        } else {
            g.wheel = 0.0;
        }

        // Track, loop range, handles and playhead.
        let hot = seek.hovered() || g.drag.is_some();
        let thick = if hot { 6.0 } else { 4.0 };
        let track = Rect::from_min_max(Pos2::new(left, cy - thick / 2.0), Pos2::new(right, cy + thick / 2.0));
        p.rect_filled(track, CornerRadius::same(3), t::p().divider);
        let (mut a, mut b) = (range.first, range.last);
        match (g.drag, under) {
            (Some(Drag::First), Some(f)) => a = f,
            (Some(Drag::Last), Some(f)) => b = f,
            _ => {}
        }
        let (a, b) = (a.min(b), a.max(b));
        let span = Rect::from_min_max(Pos2::new(anim::frame_x(a, total, left, width), track.top()), Pos2::new(anim::frame_x(b, total, left, width), track.bottom()));
        p.rect_filled(span, CornerRadius::same(3), t::p().primary.gamma_multiply(if player.range.is_some() { 0.8 } else { 0.35 }));
        for (at, resp) in [(a, &first), (b, &last)] {
            let r = Rect::from_center_size(Pos2::new(anim::frame_x(at, total, left, width), cy), vec2(4.0, if hot { 16.0 } else { 12.0 }));
            let c = if resp.hovered() || resp.dragged() { Color32::WHITE } else { t::p().text_value };
            p.rect_filled(r, CornerRadius::same(2), c);
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        let head = anim::frame_x(player.frame(), total, left, width);
        p.circle_filled(Pos2::new(head, cy), if hot { 6.0 } else { 4.5 }, Color32::WHITE);
        if seek.hovered() && !first.hovered() && !last.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // The frame under the pointer, above the bar.
        let show_thumb = (hit.expand2(vec2(0.0, 4.0)).contains(pointer.unwrap_or(Pos2::ZERO)) && g.drag != Some(Drag::Seek)) || matches!(g.drag, Some(Drag::First | Drag::Last));
        if show_thumb
            && let (Some(f), Some(pos)) = (under, pointer)
            && let Some(tex) = player.thumb(f)
        {
            let ppp = ui.ctx().pixels_per_point();
            let size = tex.size_vec2() / ppp;
            let size = size * (120.0 / size.max_elem()).min(1.0);
            let x = (pos.x - size.x / 2.0).clamp(left, (right - size.x).max(left));
            let r = Rect::from_min_size(Pos2::new(x, bar.top() - size.y - 22.0), size);
            let lp = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("gif-thumb")));
            lp.rect_filled(r.expand(3.0), CornerRadius::same(5), t::p().panel);
            lp.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            lp.text(Pos2::new(r.center().x, r.bottom() + 11.0), Align2::CENTER_CENTER, format!("{}", f + 1), mono(11.5), t::p().text);
        }

        if reset {
            player.set_range(None);
            player.seek(0);
        }
        if set_thumb {
            player.request_export(player.frame());
        }
        if let Some((_, img)) = player.take_export() {
            self.ws.actions.push(Action::Edit(Op::FrameThumbnail(item.id.clone(), img)));
        }
    }

    /// Keeps a GIF or WebP card playing while it is hovered (⌥ keeps it
    /// still), or always with the preference on, as in Eagle.
    pub(super) fn card_anim(&mut self, ui: &Ui, lib: &Library, item: &Item, card: Rect, hovered: bool) {
        let Some(kind) = anim::card_kind(&item.ext) else { return };
        let alt = ui.input(|i| i.modifiers.alt);
        if !(self.prefs.always_play_animated || hovered && !alt) || !self.ws.cards.may_play(&item.id) {
            return;
        }
        let path = item.original_path(lib.root());
        // An original still in iCloud stays a still rather than download.
        if !self.ws.cards.playing(&item.id) && crate::icloud::is_dataless(&path) {
            return;
        }
        let ctx = ui.ctx();
        let side = ((card.width().max(card.height()) * ctx.pixels_per_point()).ceil() as u32).next_multiple_of(64).min(1024);
        let (frame, now) = (ctx.cumulative_frame_nr(), ctx.input(|i| i.time));
        self.ws.cards.play(ctx, &item.id, path, kind, side, frame, now);
    }

    /// The frame of a GIF or animated WebP shown in the hover preview at
    /// `rect`, while it plays.
    pub(super) fn hover_anim(&mut self, ctx: &egui::Context, lib: &Library, item: &Item, rect: Rect) -> Option<TextureHandle> {
        let kind = anim::card_kind(&item.ext)?;
        let path = item.original_path(lib.root());
        if crate::icloud::is_dataless(&path) {
            return None;
        }
        let side = ((rect.size().max_elem() * ctx.pixels_per_point()).ceil() as u32).min(2048);
        self.ws.cards.popup(ctx, &item.id, path, kind, side, ctx.input(|i| i.time)).cloned()
    }
}
