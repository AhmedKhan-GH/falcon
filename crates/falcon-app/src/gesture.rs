//! The image viewer's mouse gestures, after Eagle's: with the right (or
//! middle) button held, dragging up or down zooms about the point pressed,
//! and a quick sideways swipe moves to the next item (rightwards) or the
//! previous one (leftwards). A right click that doesn't move still opens
//! the item's menu, which egui reports as a click; this module only rules
//! on drags. Drawing and input are in `ui/viewer/image.rs`.

use eframe::egui::Pos2;

/// The lowest and highest zoom a vertical drag reaches (5% to 200%).
pub const ZOOM_RANGE: (f32, f32) = (0.05, 2.0);
/// A sideways swipe must be longer than this, in points.
pub const SWIPE_MIN: f32 = 20.0;
/// And take at most this long, in seconds.
pub const SWIPE_SECS: f64 = 1.0;
/// Points of trail kept for the swipe's feedback.
pub const TRAIL: usize = 24;

/// What a released gesture asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    Next,
    Prev,
}

/// One drag with the right or middle button held.
#[derive(Clone, Debug)]
pub struct Gesture {
    /// Where the button went down.
    pub start: Pos2,
    /// When the button went down (egui time).
    since: f64,
    /// The zoom when the button went down.
    zoom0: f32,
    /// The longest sideways distance reached, so a swipe that turned back
    /// doesn't count.
    max_dx: f32,
    /// Once a drag has zoomed, it stays a zoom.
    pub zooming: bool,
    /// Recent pointer positions, oldest first, for the swipe's trail.
    pub trail: Vec<Pos2>,
}

impl Gesture {
    pub fn press(at: Pos2, time: f64, zoom: f32) -> Gesture {
        Gesture { start: at, since: time, zoom0: zoom, max_dx: 0.0, zooming: false, trail: vec![at] }
    }

    /// The pointer moved to `at`: the new zoom if this drag zooms. The
    /// drag zooms once it has gone further up or down than sideways;
    /// every 100 points up (150 from 100% on) multiplies the zoom by e.
    pub fn moved(&mut self, at: Pos2) -> Option<f32> {
        let (dx, dy) = (at.x - self.start.x, self.start.y - at.y);
        self.max_dx = self.max_dx.max(dx.abs());
        self.trail.push(at);
        if self.trail.len() > TRAIL {
            self.trail.remove(0);
        }
        if !self.zooming && dy.abs() <= dx.abs() {
            return None;
        }
        self.zooming = true;
        let k = if self.zoom0 < 1.0 { 100.0 } else { 150.0 };
        Some((self.zoom0 * (dy / k).exp()).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1))
    }

    /// Whether the swipe's trail should show: a sideways drag past
    /// [`SWIPE_MIN`] that isn't zooming.
    pub fn swiping(&self) -> bool {
        !self.zooming && self.trail.last().is_some_and(|p| (p.x - self.start.x).abs() > SWIPE_MIN)
    }

    /// The button came up at `at`, at `time`, with the zoom now `zoom`. A
    /// mostly sideways drag longer than [`SWIPE_MIN`], made within
    /// [`SWIPE_SECS`] without zooming, and not turned back by more than a
    /// third of its reach, steps to the next (rightwards) or previous item.
    pub fn release(&self, at: Pos2, time: f64, zoom: f32) -> Outcome {
        let (dx, dy) = (at.x - self.start.x, at.y - self.start.y);
        if dy.abs() - 5.0 > dx.abs() || self.zooming || (zoom - self.zoom0).abs() > 1e-4 {
            return Outcome::None;
        }
        if dx.abs() > SWIPE_MIN && time - self.since <= SWIPE_SECS && dx.abs() > self.max_dx * 2.0 / 3.0 {
            return if dx > 0.0 { Outcome::Next } else { Outcome::Prev };
        }
        Outcome::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Pos2 {
        Pos2::new(x, y)
    }

    #[test]
    fn quick_sideways_swipes_step() {
        let mut g = Gesture::press(p(100.0, 100.0), 0.0, 1.0);
        assert_eq!(g.moved(p(160.0, 104.0)), None);
        assert!(g.swiping());
        assert_eq!(g.release(p(160.0, 104.0), 0.4, 1.0), Outcome::Next);
        let mut g = Gesture::press(p(100.0, 100.0), 0.0, 1.0);
        g.moved(p(40.0, 98.0));
        assert_eq!(g.release(p(40.0, 98.0), 0.4, 1.0), Outcome::Prev);
    }

    #[test]
    fn slow_short_or_turned_back_swipes_dont() {
        let mut g = Gesture::press(p(100.0, 100.0), 0.0, 1.0);
        g.moved(p(160.0, 100.0));
        assert_eq!(g.release(p(160.0, 100.0), 1.5, 1.0), Outcome::None, "too slow");
        let mut g = Gesture::press(p(100.0, 100.0), 0.0, 1.0);
        g.moved(p(115.0, 100.0));
        assert!(!g.swiping());
        assert_eq!(g.release(p(115.0, 100.0), 0.2, 1.0), Outcome::None, "too short");
        let mut g = Gesture::press(p(100.0, 100.0), 0.0, 1.0);
        g.moved(p(250.0, 100.0));
        g.moved(p(140.0, 100.0));
        assert_eq!(g.release(p(140.0, 100.0), 0.5, 1.0), Outcome::None, "turned back");
    }

    #[test]
    fn vertical_drags_zoom_and_never_step() {
        let mut g = Gesture::press(p(100.0, 300.0), 0.0, 1.0);
        let z = g.moved(p(102.0, 150.0)).unwrap();
        assert!((z - 1.0f32.exp().min(2.0)).abs() < 1e-4, "150 points up from 100% is ×e, held at 200%");
        assert!(g.zooming);
        // Once zooming, a sideways move still zooms.
        assert!(g.moved(p(300.0, 290.0)).is_some());
        assert_eq!(g.release(p(300.0, 290.0), 0.3, 1.1), Outcome::None);
        // Below 100% it is more sensitive: 100 points down divides by e.
        let mut g = Gesture::press(p(0.0, 0.0), 0.0, 0.5);
        assert!((g.moved(p(0.0, 100.0)).unwrap() - 0.5 / 1.0f32.exp()).abs() < 1e-4);
        let mut g = Gesture::press(p(0.0, 0.0), 0.0, 0.1);
        assert_eq!(g.moved(p(0.0, 1000.0)), Some(ZOOM_RANGE.0));
    }
}
