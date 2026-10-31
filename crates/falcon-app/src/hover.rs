//! Eagle's hover preview: an enlarged copy of a card's image that floats over
//! the window while the pointer rests on the card's zoom button, or while Z is
//! held over a card. Placement and timing are pure functions so they can be
//! tested; `paint` draws the result.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, TextureId, Vec2, vec2};

/// How long the pointer must rest on the zoom button before the preview
/// shows (Eagle's default delay is 200 ms).
pub const DELAY: f64 = 0.2;
/// Gap kept between the preview and the window edge.
pub const MARGIN: f32 = 10.0;
/// A side-placed preview starts this far inside the card's edge, so it grows
/// out of the card.
const INSET: f32 = 15.0;
/// Eagle skips loading originals above this many pixels.
const ORIGINAL_PIXEL_LIMIT: u64 = 56_000_000;
/// Longest side the loader decodes grid thumbnails at (`Tier::Grid`), in
/// pixels; a preview larger than this asks for the original.
pub const GRID_SIDE: f32 = 512.0;
/// Width of the preview's frame.
const FRAME: f32 = 3.0;

/// Types Eagle loads the original of for the preview; the rest keep their
/// thumbnail.
const RASTER: [&str; 9] = ["avif", "gif", "webp", "png", "jpg", "jpeg", "bmp", "ico", "jfif"];
/// Types whose cards have no zoom button (they have their own hover
/// behaviour in Eagle, or nothing to enlarge).
const NO_BUTTON: [&str; 21] = [
    "ts", "mp4", "mov", "m4v", "mkv", "avi", "wmv", "mpg", "webm", "mp3", "wav", "flac", "ogg", "aac", "m4a", "ttf", "otf",
    "ttc", "woff", "woff2", "url",
];

/// Whether cards of this type show a zoom button.
pub fn has_zoom_button(ext: &str) -> bool {
    !NO_BUTTON.contains(&ext.to_lowercase().as_str())
}

/// Side of the zoom button for a given card size: 24 pt, 20 pt on small
/// cards, growing with large ones, always 20–32 pt.
pub fn zoom_button_side(tile: f32) -> f32 {
    let side = if tile < 151.0 {
        20.0
    } else if tile > 241.0 {
        tile / 20.0 + 1.0
    } else {
        24.0
    };
    side.clamp(20.0, 32.0)
}

/// The zoom button's hot area: the bottom-right corner of the card's image,
/// or the whole thumbnail in the list layout.
pub fn zoom_button_rect(thumb: Rect, tile: f32, list: bool) -> Rect {
    if list {
        return thumb;
    }
    let side = zoom_button_side(tile).min(thumb.width()).min(thumb.height());
    Rect::from_min_max(thumb.max - Vec2::splat(side), thumb.max)
}

/// Whether to load the full-size original for a preview shown at `display`
/// size, rather than keep the grid thumbnail (decoded at up to `thumb_side`).
pub fn wants_original(ext: &str, width: u64, height: u64, display: Vec2, thumb_side: f32) -> bool {
    RASTER.contains(&ext.to_lowercase().as_str()) && width * height < ORIGINAL_PIXEL_LIMIT && display.max_elem() > thumb_side
}

/// Where the preview of an image of `natural` size goes for a card at `card`
/// inside `window`. The image keeps its shape, is never enlarged past its
/// own size or `max_side`, and stays `MARGIN` inside the window. It goes on
/// whichever side of the card (above or below, left or right) lets it be
/// largest; when both are equal, beside the card unless the card is partly
/// scrolled out of view.
pub fn preview_rect(card: Rect, natural: Vec2, window: Rect, max_side: f32) -> Rect {
    let bounds = window.shrink(MARGIN);
    if natural.x <= 0.0 || natural.y <= 0.0 || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return Rect::from_center_size(card.center(), Vec2::ZERO);
    }
    let cap = 1.0f32.min(max_side / natural.max_elem()).min(bounds.width() / natural.x).min(bounds.height() / natural.y);
    let fit = |avail: Vec2| (avail.x / natural.x).min(avail.y / natural.y).clamp(0.0, cap);
    let (above, below) = (card.top() - bounds.top(), bounds.bottom() - card.bottom());
    let (left, right) = (card.left() - bounds.left(), bounds.right() - card.right());
    let v_scale = fit(vec2(bounds.width(), above.max(below)));
    // Beside the card, the preview may cover the card itself, all but the
    // inset strip it grows from.
    let h_scale = fit(vec2(left.max(right) + card.width() - INSET, bounds.height()));
    let vertical = if v_scale == h_scale {
        card.top() < bounds.top() || card.bottom() > bounds.bottom()
    } else {
        v_scale > h_scale
    };
    let size = natural * if vertical { v_scale } else { h_scale };
    // The preview's top sits a little below the card's top (a quarter of the
    // card, at most 20 pt), as in Eagle.
    let lip = (card.height() / 4.0).min(20.0);
    let min = if vertical {
        let y = if above > below { card.top() + lip - size.y } else { card.top() + lip };
        Pos2::new(card.left(), y)
    } else {
        let x = if right >= left { card.left() + INSET } else { card.right() - INSET - size.x };
        Pos2::new(x, card.top() + lip)
    };
    let r = Rect::from_min_size(min, size);
    let shift = |lo: f32, hi: f32, a: f32, b: f32| if hi > b { b - hi } else if lo < a { a - lo } else { 0.0 };
    let dx = shift(r.left(), r.right(), bounds.left(), bounds.right());
    let dy = shift(r.top(), r.bottom(), bounds.top(), bounds.bottom());
    r.translate(vec2(dx, dy))
}

/// What the pointer and keyboard are doing this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct Input<'a> {
    /// The card under the pointer.
    pub card: Option<&'a str>,
    /// Whether the pointer is on that card's zoom button.
    pub on_button: bool,
    /// Z is held (with no ⌘, ⌃ or ⇧) and no text field has focus.
    pub key: bool,
    /// The grid scrolled this frame.
    pub scrolled: bool,
    /// Something that always closes the preview: a click or drag, the viewer
    /// opening, or the visible items changing.
    pub cancel: bool,
    /// Seconds, from `egui::InputState::time`.
    pub now: f64,
}

/// The preview's state between frames.
#[derive(Debug, Default)]
pub struct Hover {
    target: Option<String>,
    since: f64,
    shown: bool,
    by_key: bool,
    /// Z was held when the preview was cancelled; it must be released first.
    key_blocked: bool,
    /// The pointer was on a zoom button when the preview was cancelled; it
    /// must leave the button first.
    button_blocked: bool,
}

impl Hover {
    /// Advances the state. Returns how long until the preview is due, when
    /// it is waiting on the delay, so the caller can schedule a repaint.
    pub fn update(&mut self, i: Input) -> Option<f64> {
        if !i.key {
            self.key_blocked = false;
        }
        if !i.on_button {
            self.button_blocked = false;
        }
        if i.cancel {
            self.key_blocked = i.key;
            self.button_blocked = i.on_button;
            self.reset();
            return None;
        }
        let Some(card) = i.card else {
            self.reset();
            return None;
        };
        if i.key && !self.key_blocked {
            self.target = Some(card.to_string());
            self.shown = true;
            self.by_key = true;
            return None;
        }
        if !i.on_button || i.scrolled || self.button_blocked {
            self.reset();
            return None;
        }
        if self.by_key || self.target.as_deref() != Some(card) {
            self.reset();
            self.target = Some(card.to_string());
            self.since = i.now;
        }
        let left = DELAY - (i.now - self.since);
        if left <= 0.0 {
            self.shown = true;
            None
        } else {
            Some(left)
        }
    }

    /// The card whose preview is showing.
    pub fn shown(&self) -> Option<&str> {
        self.target.as_deref().filter(|_| self.shown)
    }

    fn reset(&mut self) {
        self.target = None;
        self.shown = false;
        self.by_key = false;
    }
}

/// Draws the faint zoom-in mark on a hovered card's zoom button: half
/// strength while the card is hovered, full while the button is.
pub fn paint_button(painter: &egui::Painter, rect: Rect, list: bool, hot: bool, font: egui::FontId, glyph: &str) {
    let alpha = if hot { 1.0 } else { 0.5 };
    if list {
        painter.rect_filled(rect, CornerRadius::same(4), Color32::from_black_alpha((90.0 * alpha) as u8));
    }
    let c = rect.center();
    painter.circle_filled(c, rect.width().min(rect.height()).min(24.0) * 0.45, Color32::from_black_alpha((140.0 * alpha) as u8));
    painter.text(c, egui::Align2::CENTER_CENTER, glyph, font, Color32::WHITE.gamma_multiply(alpha));
}

/// Draws the preview: the window dimmed behind it, then the image in a
/// rounded frame with a soft shadow. `fade` runs from 0 to 1 as it appears.
pub fn paint(ctx: &egui::Context, window: Rect, rect: Rect, tex: TextureId, background: Color32, fade: f32) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("hover-preview")));
    painter.rect_filled(window, CornerRadius::ZERO, Color32::from_black_alpha((128.0 * fade) as u8));
    let frame = rect.expand(FRAME);
    let radius = CornerRadius::same(6);
    let shadow = egui::Shadow { offset: [0, 12], blur: 24, spread: 0, color: Color32::from_black_alpha((77.0 * fade) as u8) };
    painter.add(shadow.as_shape(frame, radius));
    painter.rect_filled(frame, radius, background.gamma_multiply(fade));
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.add(egui::epaint::RectShape::filled(rect, CornerRadius::same(3), Color32::WHITE.gamma_multiply(fade)).with_texture(tex, uv));
    painter.rect_stroke(frame, radius, Stroke::new(FRAME, Color32::from_white_alpha((38.0 * fade) as u8)), StrokeKind::Inside);
    painter.rect_stroke(frame, radius, Stroke::new(1.0, Color32::from_black_alpha((128.0 * fade) as u8)), StrokeKind::Outside);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))
    }

    fn inside(r: Rect, w: Rect) -> bool {
        let b = w.shrink(MARGIN).expand(0.01);
        b.contains_rect(r)
    }

    fn aspect(v: Vec2) -> f32 {
        v.x / v.y
    }

    #[test]
    fn small_image_keeps_its_size_and_goes_beside_the_card() {
        let card = Rect::from_min_size(Pos2::new(300.0, 300.0), vec2(140.0, 140.0));
        let r = preview_rect(card, vec2(200.0, 100.0), window(), f32::INFINITY);
        assert_eq!(r.size(), vec2(200.0, 100.0));
        // Room is larger on the right, so it grows rightwards out of the card.
        assert_eq!(r.left(), card.left() + INSET);
        assert_eq!(r.top(), card.top() + 20.0);
    }

    #[test]
    fn grows_towards_the_side_with_room() {
        let card = Rect::from_min_size(Pos2::new(1000.0, 300.0), vec2(140.0, 140.0));
        let r = preview_rect(card, vec2(4000.0, 3000.0), window(), f32::INFINITY);
        assert!(r.right() <= card.right() - INSET + 0.01);
        assert!(inside(r, window()));
        let card = Rect::from_min_size(Pos2::new(40.0, 300.0), vec2(140.0, 140.0));
        let r = preview_rect(card, vec2(4000.0, 3000.0), window(), f32::INFINITY);
        assert!(r.left() >= card.left());
        assert!(inside(r, window()));
    }

    #[test]
    fn wide_image_in_a_tall_window_goes_above_or_below() {
        let w = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 1200.0));
        let card = Rect::from_min_size(Pos2::new(230.0, 100.0), vec2(140.0, 140.0));
        let r = preview_rect(card, vec2(3000.0, 1000.0), w, f32::INFINITY);
        // Fills the window's width, below the card.
        assert!((r.width() - (600.0 - 2.0 * MARGIN)).abs() < 0.01);
        assert!(r.top() >= card.top());
        assert!(inside(r, w));
        let card = Rect::from_min_size(Pos2::new(230.0, 1000.0), vec2(140.0, 140.0));
        let r = preview_rect(card, vec2(3000.0, 1000.0), w, f32::INFINITY);
        assert!(r.bottom() <= card.top() + 20.0 + 0.01);
    }

    #[test]
    fn keeps_shape_and_respects_max_side() {
        let card = Rect::from_min_size(Pos2::new(500.0, 300.0), vec2(140.0, 200.0));
        for natural in [vec2(5000.0, 2000.0), vec2(800.0, 2400.0), vec2(300.0, 300.0)] {
            let r = preview_rect(card, natural, window(), 480.0);
            assert!(r.width().max(r.height()) <= 480.01);
            assert!((aspect(r.size()) - aspect(natural)).abs() < 0.001);
            assert!(r.width() <= natural.x && r.height() <= natural.y);
        }
    }

    #[test]
    fn stays_inside_the_window_everywhere() {
        let w = window();
        for x in (-100..1300).step_by(70) {
            for y in (-100..900).step_by(70) {
                for natural in [vec2(4000.0, 3000.0), vec2(300.0, 2000.0), vec2(120.0, 90.0)] {
                    let card = Rect::from_min_size(Pos2::new(x as f32, y as f32), vec2(150.0, 110.0));
                    let r = preview_rect(card, natural, w, f32::INFINITY);
                    assert!(inside(r, w), "{card:?} {natural:?} -> {r:?}");
                }
            }
        }
    }

    #[test]
    fn small_image_on_a_card_scrolled_half_out_goes_above_or_below() {
        let card = Rect::from_min_size(Pos2::new(500.0, -60.0), vec2(140.0, 140.0));
        let r = preview_rect(card, vec2(100.0, 100.0), window(), f32::INFINITY);
        assert_eq!(r.left(), card.left());
        assert!(inside(r, window()));
    }

    #[test]
    fn degenerate_sizes_give_an_empty_rect() {
        let card = Rect::from_min_size(Pos2::new(10.0, 10.0), vec2(100.0, 100.0));
        assert_eq!(preview_rect(card, Vec2::ZERO, window(), 100.0).size(), Vec2::ZERO);
    }

    #[test]
    fn button_size_follows_the_card_size() {
        assert_eq!(zoom_button_side(100.0), 20.0);
        assert_eq!(zoom_button_side(200.0), 24.0);
        assert_eq!(zoom_button_side(300.0), 20.0);
        assert_eq!(zoom_button_side(400.0), 21.0);
        assert_eq!(zoom_button_side(1000.0), 32.0);
        let thumb = Rect::from_min_size(Pos2::new(0.0, 0.0), vec2(200.0, 100.0));
        assert_eq!(zoom_button_rect(thumb, 200.0, false), Rect::from_min_max(Pos2::new(176.0, 76.0), Pos2::new(200.0, 100.0)));
        assert_eq!(zoom_button_rect(thumb, 200.0, true), thumb);
    }

    #[test]
    fn button_and_original_depend_on_type() {
        assert!(has_zoom_button("JPG"));
        assert!(has_zoom_button("pdf"));
        assert!(!has_zoom_button("mp4"));
        assert!(!has_zoom_button("woff2"));
        assert!(wants_original("png", 4000, 3000, vec2(900.0, 600.0), 512.0));
        assert!(!wants_original("png", 4000, 3000, vec2(400.0, 300.0), 512.0));
        assert!(!wants_original("pdf", 4000, 3000, vec2(900.0, 600.0), 512.0));
        assert!(!wants_original("jpg", 10000, 6000, vec2(900.0, 600.0), 512.0));
    }

    fn at(card: Option<&str>, on_button: bool, now: f64) -> Input<'_> {
        Input { card, on_button, now, ..Input::default() }
    }

    #[test]
    fn shows_after_the_delay_on_the_button() {
        let mut h = Hover::default();
        assert_eq!(h.update(at(Some("a"), false, 0.0)), None);
        assert_eq!(h.shown(), None);
        let wait = h.update(at(Some("a"), true, 1.0)).unwrap();
        assert!((wait - DELAY).abs() < 1e-9);
        assert!(h.update(at(Some("a"), true, 1.1)).is_some());
        assert_eq!(h.shown(), None);
        assert_eq!(h.update(at(Some("a"), true, 1.21)), None);
        assert_eq!(h.shown(), Some("a"));
        // Moving to another card's button starts over.
        h.update(at(Some("b"), true, 1.3));
        assert_eq!(h.shown(), None);
        h.update(at(Some("b"), true, 1.6));
        assert_eq!(h.shown(), Some("b"));
    }

    #[test]
    fn leaving_the_button_scrolling_or_cancelling_hides_it() {
        let mut h = Hover::default();
        h.update(at(Some("a"), true, 0.0));
        h.update(at(Some("a"), true, 0.3));
        assert_eq!(h.shown(), Some("a"));
        h.update(at(Some("a"), false, 0.4));
        assert_eq!(h.shown(), None);
        h.update(at(Some("a"), true, 0.5));
        h.update(at(Some("a"), true, 0.8));
        h.update(Input { scrolled: true, ..at(Some("a"), true, 0.9) });
        assert_eq!(h.shown(), None);
        h.update(at(Some("a"), true, 1.0));
        h.update(at(Some("a"), true, 1.3));
        h.update(Input { cancel: true, ..at(Some("a"), true, 1.4) });
        assert_eq!(h.shown(), None);
    }

    #[test]
    fn a_click_on_the_button_needs_the_pointer_to_leave_first() {
        let mut h = Hover::default();
        h.update(at(Some("a"), true, 0.0));
        h.update(Input { cancel: true, ..at(Some("a"), true, 0.1) });
        h.update(at(Some("a"), true, 0.2));
        h.update(at(Some("a"), true, 0.9));
        assert_eq!(h.shown(), None);
        h.update(at(Some("a"), false, 1.0));
        h.update(at(Some("a"), true, 1.1));
        h.update(at(Some("a"), true, 1.4));
        assert_eq!(h.shown(), Some("a"));
    }

    #[test]
    fn holding_z_shows_at_once_and_follows_the_pointer() {
        let mut h = Hover::default();
        let key = |card, now| Input { key: true, ..at(card, false, now) };
        h.update(key(Some("a"), 0.0));
        assert_eq!(h.shown(), Some("a"));
        h.update(Input { scrolled: true, ..key(Some("b"), 0.1) });
        assert_eq!(h.shown(), Some("b"));
        h.update(key(None, 0.2));
        assert_eq!(h.shown(), None);
        h.update(key(Some("c"), 0.3));
        assert_eq!(h.shown(), Some("c"));
        // Releasing Z hides it.
        h.update(at(Some("c"), false, 0.4));
        assert_eq!(h.shown(), None);
    }

    #[test]
    fn a_click_while_holding_z_needs_z_pressed_again() {
        let mut h = Hover::default();
        let key = |now| Input { key: true, ..at(Some("a"), false, now) };
        h.update(key(0.0));
        h.update(Input { cancel: true, ..key(0.1) });
        h.update(key(0.2));
        assert_eq!(h.shown(), None);
        h.update(at(Some("a"), false, 0.3));
        h.update(key(0.4));
        assert_eq!(h.shown(), Some("a"));
    }
}
