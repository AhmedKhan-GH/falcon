//! The pop-up shown for a screenshot: a card with the screenshot's
//! thumbnail, a title and a hint line, in a borderless window of its own
//! at the top right of the screen under the pointer, above every other
//! app's windows, as Eagle's is. It doesn't take the focus: the window is
//! ordered front without activating falcon, can't become the key window and
//! asks not to activate falcon when clicked (`make_panel`), so it shows and
//! answers the pointer whichever app is in front. A hidden app can't show a
//! window, so then the screenshot gets the macOS notification (`notify.rs`)
//! instead.
//! It can be dragged away towards the screen edge it came from, as Eagle's
//! can. Drawing lives in `ui/capture_notice.rs`; this module holds the
//! rules, the state, the thumbnail load and the AppKit glue.

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use eframe::egui::{self, Pos2, Rect, Vec2, vec2};

pub use platform::{make_panel, pointer_on_screen, visible_area};

/// How the pop-up's window title starts. It is never shown; it is how
/// the window is found and told apart.
pub const WINDOW_TITLE: &str = "Falcon screenshot notice";

/// Which notification a screenshot gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// falcon's own pop-up window.
    Window,
    /// The macOS notification.
    System,
    /// The notification switches are off.
    Nothing,
}

/// Exactly one of the two notifications fires, and only when the
/// preference switches (`Prefs::tells_capture`) allow it. Whichever app is
/// in front the pop-up can show, except while falcon is hidden.
pub fn route(tells: bool, app_hidden: bool) -> Route {
    match (tells, app_hidden) {
        (false, _) => Route::Nothing,
        (true, false) => Route::Window,
        (true, true) => Route::System,
    }
}

/// Seconds the pop-up takes to slide in from the right edge.
pub const SLIDE_IN: f32 = 0.2;
/// Seconds it stays once in (Eagle passes 1500 ms for a screenshot).
pub const HOLD: f32 = 1.5;
/// Side of the square thumbnail, in points.
pub const THUMB: f32 = 44.0;
/// Space around the thumbnail, in points.
pub const PAD: f32 = 8.0;
/// Width of the card, in points.
pub const CARD_WIDTH: f32 = 300.0;
/// Height of the card, in points.
pub const CARD_HEIGHT: f32 = THUMB + PAD * 2.0;
/// Gap from the card to the top of the usable screen, in points.
const TOP: f32 = 12.0;
/// Gap from the card to the right of the usable screen, in points.
const RIGHT: f32 = 20.0;
/// Room left of and below the card for its shadow, in points.
const SHADOW: f32 = 18.0;
/// How far the card moves from its place to be out of the window.
pub const TRAVEL: f32 = CARD_WIDTH + RIGHT + 1.0;

/// Pointer movement, in points, that makes a press a drag and not a click.
pub const CLICK_SLOP: f32 = 4.0;
/// Part of the card's width it must be dragged to go on release.
pub const DISMISS_FRACTION: f32 = 0.4;
/// Speed, in points per second, that flicks the card away from any distance.
pub const FLICK_SPEED: f32 = 600.0;
/// Seconds the card takes to return when let go too soon.
pub const SPRING_BACK: f32 = 0.18;
/// Seconds the card takes to leave once dismissed.
pub const SLIDE_OUT: f32 = 0.15;

/// The size of the pop-up's window: the card with its gaps and its shadow.
/// The card is drawn inside it, so sliding and dragging don't move the
/// window, and the window's right edge at the screen's clips the card.
pub fn window_size() -> Vec2 {
    vec2(SHADOW + CARD_WIDTH + RIGHT, TOP + CARD_HEIGHT + SHADOW)
}

/// Where the window goes in egui points, which the window system takes
/// multiplied by `zoom`: at the top right of `area`, the usable part of
/// the screen in the system's top-left coordinates (below the menu bar,
/// clear of a Dock on the right).
pub fn window_pos(area: Rect, zoom: f32) -> Pos2 {
    Pos2::new(area.right() / zoom - window_size().x, area.top() / zoom)
}

/// The card in the window, moved `offset` points to the right of its place.
pub fn card_rect(offset: f32) -> Rect {
    Rect::from_min_size(Pos2::new(SHADOW + offset, TOP), vec2(CARD_WIDTH, CARD_HEIGHT))
}

/// A point on the screen (top-left coordinates) in the window at `pos`.
pub fn screen_to_window(at: Pos2, pos: Pos2, zoom: f32) -> Pos2 {
    Pos2::new(at.x / zoom - pos.x, at.y / zoom - pos.y)
}

/// A rectangle in AppKit's coordinates (origin at the bottom left of the
/// primary screen, `primary` points tall) in the top-left ones the window
/// system takes.
pub fn flip_rect(x: f64, y: f64, w: f64, h: f64, primary: f64) -> Rect {
    Rect::from_min_size(Pos2::new(x as f32, (primary - y - h) as f32), vec2(w as f32, h as f32))
}

/// How far a card grabbed at `grab_x` in the window has to go to be
/// dismissed. The pointer stops at the screen edge, so a card held near
/// its right end asks for less than the usual distance: what is left to
/// the edge, less a little.
pub fn dismiss_distance(grab_x: f32) -> f32 {
    (CARD_WIDTH * DISMISS_FRACTION).min((window_size().x - grab_x) * 0.8)
}

/// Whether a card let go at `offset` while moving at `speed` (points per
/// second, positive to the right) leaves. Far enough leaves unless thrown
/// back; a flick to the right leaves from any distance.
pub fn should_dismiss(offset: f32, speed: f32, distance: f32) -> bool {
    if speed <= -FLICK_SPEED {
        return false;
    }
    offset >= distance || (offset > 0.0 && speed >= FLICK_SPEED)
}

/// The card's offset while dragged `dx` points from where a drag took hold
/// with the card at `base`: it follows the pointer towards the edge and
/// stops at its place going the other way.
pub fn drag_offset(base: f32, dx: f32) -> f32 {
    (base + dx).max(0.0)
}

/// `from` easing to `to` over `duration` seconds, `t` seconds in.
fn ease(from: f32, to: f32, t: f32, duration: f32) -> f32 {
    let k = (t / duration).clamp(0.0, 1.0);
    from + (to - from) * (1.0 - (1.0 - k) * (1.0 - k))
}

/// A press on the card and where it has gone since.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    start: Pos2,
    /// The card's offset when pressed (it may have been springing back).
    base: f32,
    /// Decided once the pointer has moved [`CLICK_SLOP`]: whether this is
    /// a sideways drag. Until then it may still be a click.
    sideways: Option<bool>,
    offset: f32,
}

/// What the card is doing besides sliding in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Motion {
    /// In its place.
    #[default]
    Rest,
    Dragged(Drag),
    /// Returning to its place from `from`, since `at`.
    Spring { from: f32, at: f32 },
    /// Leaving from `from`, since `at`.
    Out { from: f32, at: f32 },
}

impl Motion {
    /// How far right of its place the card is, `now` being seconds since
    /// the pop-up showed.
    pub fn offset(&self, now: f32) -> f32 {
        match *self {
            Motion::Rest => 0.0,
            Motion::Dragged(d) => d.offset,
            Motion::Spring { from, at } => ease(from, 0.0, now - at, SPRING_BACK),
            Motion::Out { from, at } => ease(from, TRAVEL, now - at, SLIDE_OUT),
        }
    }

    pub fn dragging(&self) -> bool {
        matches!(self, Motion::Dragged(_))
    }

    /// Whether it needs frames now, for an animation.
    pub fn animating(&self) -> bool {
        !matches!(self, Motion::Rest)
    }

    /// Whether it has finished leaving.
    pub fn gone(&self, now: f32) -> bool {
        matches!(*self, Motion::Out { at, .. } if now - at >= SLIDE_OUT)
    }

    /// A finished spring is at rest.
    pub fn settle(&mut self, now: f32) {
        if matches!(*self, Motion::Spring { at, .. } if now - at >= SPRING_BACK) {
            *self = Motion::Rest;
        }
    }

    /// The button went down on the card, at `pos` in the window. A card
    /// on its way out stays on its way.
    pub fn press(&mut self, pos: Pos2, now: f32) {
        if !matches!(self, Motion::Out { .. }) {
            let base = self.offset(now);
            *self = Motion::Dragged(Drag { start: pos, base, sideways: None, offset: base });
        }
    }

    /// The pointer is at `pos` with the button down. Only a sideways drag
    /// moves the card; one that sets off up or down leaves it alone.
    pub fn track(&mut self, pos: Pos2) {
        let Motion::Dragged(d) = self else { return };
        let delta = pos - d.start;
        if d.sideways.is_none() && delta.length() >= CLICK_SLOP {
            d.sideways = Some(delta.x.abs() >= delta.y.abs());
        }
        if d.sideways == Some(true) {
            d.offset = drag_offset(d.base, delta.x);
        }
    }

    /// The button came up at `pos`, moving at `speed` across. True when
    /// that made a click, which opens the item. A card dragged far enough
    /// or flicked leaves; any other returns to its place.
    pub fn release(&mut self, pos: Pos2, speed: f32, now: f32) -> bool {
        self.track(pos);
        let Motion::Dragged(d) = *self else { return false };
        let leaves = d.sideways == Some(true) && should_dismiss(d.offset, speed, dismiss_distance(d.start.x));
        *self = if leaves {
            Motion::Out { from: d.offset, at: now }
        } else if d.offset > 0.0 {
            Motion::Spring { from: d.offset, at: now }
        } else {
            Motion::Rest
        };
        d.sideways.is_none()
    }
}

/// Whether the pop-up is over. It stays while the pointer is on it, as
/// Eagle's does, and goes once the pointer leaves after its time is up.
pub fn expired(elapsed: f32, hovered: bool) -> bool {
    elapsed >= SLIDE_IN + HOLD && !hovered
}

/// How far in the pop-up has slid, from 0 (off screen) to 1 (in place).
pub fn slide(elapsed: f32) -> f32 {
    let t = (elapsed / SLIDE_IN).clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// Pixel size to decode the thumbnail at: the shorter side at twice the
/// box (sharp on a Retina screen), the longer side held to 512, never
/// larger than the picture.
pub fn thumb_size(w: u32, h: u32) -> [u32; 2] {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let want = THUMB * 2.0;
    let mut scale = (want / w.min(h)).min(1.0);
    if w.max(h) * scale > 512.0 {
        scale = 512.0 / w.max(h);
    }
    [(w * scale).round().max(1.0) as u32, (h * scale).round().max(1.0) as u32]
}

/// The part of a `w` by `h` picture the square box shows, as texture
/// coordinates `[u0, v0, u1, v1]`: it fills the box, centred across and
/// kept to the top down, as Eagle's thumbnail is.
pub fn cover_uv(w: u32, h: u32) -> [f32; 4] {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    if w > h {
        let u0 = (1.0 - h / w) / 2.0;
        [u0, 0.0, 1.0 - u0, 1.0]
    } else {
        [0.0, 0.0, 1.0, w / h]
    }
}

/// The pop-up on screen.
pub struct Shown {
    pub name: String,
    pub at: Instant,
    pub texture: Option<egui::TextureHandle>,
    /// Pixel size of the texture, for [`cover_uv`].
    pub size: [u32; 2],
    loading: Option<Receiver<Option<egui::ColorImage>>>,
    /// The pointer was on it last frame.
    pub hovered: bool,
    /// It was clicked; the app reveals the item.
    pub clicked: bool,
    /// Sliding away or springing back under a drag.
    pub motion: Motion,
    /// The button went down on the close button.
    pub on_close: bool,
    /// Where its window is, chosen when it first draws.
    pub place: Option<Pos2>,
    /// The window has been kept from taking the focus (`make_panel`) and
    /// put in its place.
    pub panel: bool,
    /// Tells this pop-up's window from the one it replaces.
    pub serial: u64,
}

/// Only one pop-up is up at a time: a new screenshot replaces the one
/// showing, as Eagle's single notification window does.
#[derive(Default)]
pub struct CaptureNotice {
    pub current: Option<Shown>,
    shown: u64,
}

impl CaptureNotice {
    /// Shows `name`, replacing any pop-up up now. `thumb` delivers the
    /// picture when it has loaded (None when there is none to load).
    pub fn show(&mut self, name: String, thumb: Option<Receiver<Option<egui::ColorImage>>>) {
        self.shown += 1;
        self.current = Some(Shown {
            name,
            at: Instant::now(),
            texture: None,
            size: [1, 1],
            loading: thumb,
            hovered: false,
            clicked: false,
            motion: Motion::default(),
            on_close: false,
            place: None,
            panel: false,
            serial: self.shown,
        });
    }

    /// The name of the screenshot whose pop-up was clicked, closing it.
    pub fn take_click(&mut self) -> Option<String> {
        if self.current.as_ref().is_some_and(|s| s.clicked) {
            return self.current.take().map(|s| s.name);
        }
        None
    }
}

impl Shown {
    /// Uploads the thumbnail once its load has finished. A failed load
    /// leaves the pop-up without a picture.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.loading else { return };
        match rx.try_recv() {
            Ok(Some(img)) => {
                self.size = [img.size[0] as u32, img.size[1] as u32];
                self.texture = Some(ctx.load_texture("capture-notice", img, egui::TextureOptions::LINEAR));
                self.loading = None;
            }
            Ok(None) | Err(TryRecvError::Disconnected) => self.loading = None,
            Err(TryRecvError::Empty) => {}
        }
    }
}

/// Starts loading a downscaled thumbnail of the screenshot at `path`
/// off the UI thread. The import deletes the file once it has copied it
/// in, so the thread decodes a hard link made here (instant), which it
/// removes afterwards. None when the link can't be made.
pub fn load_thumb(ctx: &egui::Context, path: &Path) -> Option<Receiver<Option<egui::ColorImage>>> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    let link = path.with_file_name(format!("notice-{stamp}.{}", path.extension()?.to_string_lossy()));
    std::fs::hard_link(path, &link).ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let img = falcon_library::thumbnail::decode(&link).ok().map(|img| {
            let [w, h] = thumb_size(img.width(), img.height());
            let small = if (w, h) == (img.width(), img.height()) { img } else { img.thumbnail(w, h) }.to_rgba8();
            egui::ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw())
        });
        let _ = std::fs::remove_file(&link);
        let _ = tx.send(img);
        ctx.request_repaint();
    });
    Some(rx)
}

#[cfg(target_os = "macos")]
mod platform {
    use eframe::egui::{Pos2, Rect};
    use std::sync::Once;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{MainThreadMarker, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSEvent, NSScreen, NSWindowCollectionBehavior};
    use objc2_foundation::NSString;

    /// The usable part (below the menu bar, clear of the Dock) of the
    /// screen under the pointer, in top-left coordinates.
    pub fn visible_area() -> Option<Rect> {
        let mtm = MainThreadMarker::new()?;
        let at = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let primary = screens.iter().next()?.frame().size.height;
        let inside = |s: &NSScreen| {
            let f = s.frame();
            at.x >= f.origin.x && at.x < f.origin.x + f.size.width && at.y >= f.origin.y && at.y < f.origin.y + f.size.height
        };
        let screen = screens.iter().find(|s| inside(s)).or_else(|| screens.iter().next())?;
        let f = screen.visibleFrame();
        Some(super::flip_rect(f.origin.x, f.origin.y, f.size.width, f.size.height, primary))
    }

    /// The pointer on the screen, in top-left coordinates. It is asked for
    /// and not taken from the window's events: a window that isn't the key
    /// window is only sent the pointer's moves with a button down.
    pub fn pointer_on_screen() -> Option<Pos2> {
        let mtm = MainThreadMarker::new()?;
        let at = NSEvent::mouseLocation();
        let primary = NSScreen::screens(mtm).iter().next()?.frame().size.height;
        Some(Pos2::new(at.x as f32, (primary - at.y) as f32))
    }

    /// Makes the window titled `title` one that neither activates falcon
    /// when clicked nor becomes the key or main window, on every Space and
    /// over full-screen apps, and out of the Window menu. True once the
    /// window has been found.
    ///
    /// AppKit's way is an `NSPanel` with the non-activating style, but winit
    /// only makes its own `NSWindow` subclass, and giving a live window
    /// another class breaks the key-value observers on it (removing them
    /// when it closes raises an exception). So the window asks not to
    /// activate through `_setPreventsActivation:`, AppKit's private setter
    /// behind that style, where the system has it (else a click activates
    /// falcon and nothing worse), and winit's class answers no for it in
    /// [`keep_from_key`].
    pub fn make_panel(title: &str) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return true };
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        let Some(window) = windows.iter().find(|w| w.title().to_string() == title) else { return false };
        keep_from_key();
        let object: &AnyObject = window.as_ref();
        // SAFETY: the selector is checked to exist and takes one BOOL.
        unsafe {
            let responds: Bool = msg_send![object, respondsToSelector: sel!(_setPreventsActivation:)];
            if responds.as_bool() {
                let _: () = msg_send![object, _setPreventsActivation: Bool::YES];
            }
        }
        window.setExcludedFromWindowsMenu(true);
        window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::FullScreenAuxiliary);
        true
    }

    /// Answers `canBecomeKeyWindow` and `canBecomeMainWindow`: yes, which
    /// is what winit's class says, except for the pop-up's window.
    extern "C-unwind" fn can_become(this: &AnyObject, _cmd: Sel) -> Bool {
        // SAFETY: `this` is a window, and `title` returns a live string.
        let title: Retained<NSString> = unsafe { msg_send![this, title] };
        Bool::new(!title.to_string().starts_with(super::WINDOW_TITLE))
    }

    /// winit's window class answers yes to becoming key and main for every
    /// window, even a borderless one. Its answers are replaced, once.
    fn keep_from_key() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let Some(class) = AnyClass::get(c"WinitWindow") else { return };
            let imp = can_become as extern "C-unwind" fn(&AnyObject, Sel) -> Bool;
            for selector in [sel!(canBecomeKeyWindow), sel!(canBecomeMainWindow)] {
                if let Some(method) = class.instance_method(selector) {
                    // SAFETY: both methods take no arguments and return a
                    // BOOL, as `can_become` does.
                    unsafe { method.set_implementation(std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(imp)) };
                }
            }
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use eframe::egui::{Pos2, Rect};

    pub fn visible_area() -> Option<Rect> {
        None
    }

    pub fn pointer_on_screen() -> Option<Pos2> {
        None
    }

    pub fn make_panel(_title: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_picks_exactly_one() {
        // Whichever app is in front, the pop-up; while hidden, the system's.
        assert_eq!(route(true, false), Route::Window);
        assert_eq!(route(true, true), Route::System);
        assert_eq!(route(false, true), Route::Nothing);
        assert_eq!(route(false, false), Route::Nothing);
    }

    #[test]
    fn expiry() {
        assert!(!expired(0.0, false));
        assert!(!expired(1.69, false));
        assert!(expired(1.7, false));
        // The pointer on it keeps it up, and it goes when it leaves.
        assert!(!expired(5.0, true));
        assert!(expired(5.0, false));
    }

    #[test]
    fn sliding_in() {
        assert_eq!(slide(0.0), 0.0);
        assert_eq!(slide(SLIDE_IN), 1.0);
        assert_eq!(slide(10.0), 1.0);
        assert!(slide(0.1) > 0.5 && slide(0.1) < 1.0);
    }

    #[test]
    fn a_new_screenshot_replaces_the_pop_up() {
        let mut n = CaptureNotice::default();
        n.show("Screenshot - a".into(), None);
        n.show("Screenshot - b".into(), None);
        assert_eq!(n.current.as_ref().map(|s| s.name.as_str()), Some("Screenshot - b"));
        assert_eq!(n.take_click(), None);
        n.current.as_mut().unwrap().clicked = true;
        assert_eq!(n.take_click().as_deref(), Some("Screenshot - b"));
        assert!(n.current.is_none());
    }

    #[test]
    fn thumbnail_sizes() {
        // A big screenshot: shorter side to 88.
        assert_eq!(thumb_size(2880, 1800), [141, 88]);
        // Already small: unchanged.
        assert_eq!(thumb_size(50, 40), [50, 40]);
        // A very wide strip is held to 512 along its length.
        assert_eq!(thumb_size(10000, 1000), [512, 51]);
        assert_eq!(thumb_size(0, 0), [1, 1]);
    }

    #[test]
    fn cover_fills_the_box_from_the_top() {
        assert_eq!(cover_uv(100, 100), [0.0, 0.0, 1.0, 1.0]);
        // Wider than tall: the middle band across.
        assert_eq!(cover_uv(200, 100), [0.25, 0.0, 0.75, 1.0]);
        // Taller than wide: the top part down.
        assert_eq!(cover_uv(100, 400), [0.0, 0.0, 1.0, 0.25]);
    }

    fn at(x: f32, y: f32) -> Pos2 {
        Pos2::new(x, y)
    }

    #[test]
    fn the_window_sits_at_the_top_right_of_the_usable_screen() {
        // A 1440 pt screen with a 25 pt menu bar.
        let area = Rect::from_min_size(at(0.0, 25.0), vec2(1440.0, 875.0));
        assert_eq!(window_pos(area, 1.0), at(1440.0 - window_size().x, 25.0));
        // A second screen to the left has negative coordinates.
        let left = Rect::from_min_size(at(-1920.0, 0.0), vec2(1920.0, 1080.0));
        assert_eq!(window_pos(left, 1.0).x, -window_size().x);
        // With the interface zoomed the right edge stays on the screen's:
        // the window is `window_size * zoom` wide on screen.
        let pos = window_pos(area, 1.25);
        assert!(((pos.x + window_size().x) * 1.25 - 1440.0).abs() < 0.01);
        assert!((pos.y * 1.25 - 25.0).abs() < 0.01);
    }

    #[test]
    fn the_card_fits_the_window_and_leaves_it_at_full_travel() {
        let win = Rect::from_min_size(Pos2::ZERO, window_size());
        assert!(win.contains_rect(card_rect(0.0)));
        assert_eq!(card_rect(0.0).width(), 300.0);
        assert!(card_rect(TRAVEL).left() > win.right());
        assert_eq!(card_rect(10.0).left(), card_rect(0.0).left() + 10.0);
    }

    #[test]
    fn coordinates_convert() {
        // AppKit's y runs up from the primary screen's bottom.
        assert_eq!(flip_rect(0.0, 0.0, 1440.0, 875.0, 900.0), Rect::from_min_size(at(0.0, 25.0), vec2(1440.0, 875.0)));
        assert_eq!(screen_to_window(at(1100.0, 40.0), at(1000.0, 25.0), 1.0), at(100.0, 15.0));
        assert_eq!(screen_to_window(at(1250.0, 50.0), at(800.0, 20.0), 1.25), at(200.0, 20.0));
    }

    #[test]
    fn a_click_is_a_press_that_barely_moves() {
        let mut m = Motion::Rest;
        m.press(at(100.0, 30.0), 1.0);
        assert!(m.dragging());
        m.track(at(102.0, 31.0));
        assert_eq!(m.offset(1.0), 0.0);
        assert!(m.release(at(102.0, 31.0), 0.0, 1.1));
        assert_eq!(m, Motion::Rest);
    }

    #[test]
    fn a_sideways_drag_moves_the_card_and_is_no_click() {
        let mut m = Motion::Rest;
        m.press(at(100.0, 30.0), 1.0);
        m.track(at(150.0, 32.0));
        assert_eq!(m.offset(1.0), 50.0);
        // Back past where it started: it stops at its place.
        m.track(at(60.0, 32.0));
        assert_eq!(m.offset(1.0), 0.0);
        // Returning close to the start doesn't make it a click again.
        assert!(!m.release(at(100.0, 30.0), 0.0, 1.2));
    }

    #[test]
    fn a_vertical_drag_does_nothing() {
        let mut m = Motion::Rest;
        m.press(at(100.0, 30.0), 1.0);
        m.track(at(101.0, 60.0));
        m.track(at(250.0, 60.0));
        assert_eq!(m.offset(1.0), 0.0);
        assert!(!m.release(at(250.0, 60.0), 2000.0, 1.2));
        assert_eq!(m, Motion::Rest);
    }

    #[test]
    fn far_enough_or_a_flick_dismisses() {
        let d = CARD_WIDTH * DISMISS_FRACTION;
        assert!(should_dismiss(d, 0.0, d));
        assert!(!should_dismiss(d - 1.0, 0.0, d));
        assert!(should_dismiss(5.0, FLICK_SPEED, d));
        assert!(!should_dismiss(0.0, FLICK_SPEED, d));
        // Thrown back from far out, it stays.
        assert!(!should_dismiss(d + 50.0, -FLICK_SPEED, d));
    }

    #[test]
    fn near_the_screen_edge_less_distance_is_asked() {
        assert_eq!(dismiss_distance(30.0), CARD_WIDTH * DISMISS_FRACTION);
        // Held 40 pt from the window's edge the pointer can't go further.
        let d = dismiss_distance(window_size().x - 40.0);
        assert_eq!(d, 32.0);
    }

    #[test]
    fn let_go_too_soon_it_springs_back() {
        let mut m = Motion::Rest;
        m.press(at(50.0, 30.0), 1.0);
        assert!(!m.release(at(90.0, 30.0), 0.0, 2.0));
        assert_eq!(m, Motion::Spring { from: 40.0, at: 2.0 });
        assert_eq!(m.offset(2.0), 40.0);
        let mid = m.offset(2.0 + SPRING_BACK / 2.0);
        assert!(mid > 0.0 && mid < 20.0);
        assert_eq!(m.offset(2.0 + SPRING_BACK), 0.0);
        assert!(!m.gone(10.0));
        m.settle(2.0 + SPRING_BACK / 2.0);
        assert!(m.animating());
        m.settle(2.0 + SPRING_BACK);
        assert_eq!(m, Motion::Rest);
    }

    #[test]
    fn past_the_threshold_it_slides_out_and_goes() {
        let mut m = Motion::Rest;
        m.press(at(50.0, 30.0), 1.0);
        assert!(!m.release(at(190.0, 30.0), 0.0, 2.0));
        assert_eq!(m, Motion::Out { from: 140.0, at: 2.0 });
        assert!(!m.gone(2.0));
        assert!(m.offset(2.0 + SLIDE_OUT / 2.0) > 140.0);
        assert_eq!(m.offset(2.0 + SLIDE_OUT), TRAVEL);
        assert!(m.gone(2.0 + SLIDE_OUT));
        // A press can't catch it on its way.
        m.press(at(100.0, 30.0), 2.0 + SLIDE_OUT / 2.0);
        assert!(matches!(m, Motion::Out { .. }));
    }

    #[test]
    fn caught_while_springing_back_it_is_held_where_it_is() {
        let mut m = Motion::Spring { from: 100.0, at: 0.0 };
        let now = SPRING_BACK / 2.0;
        let there = m.offset(now);
        m.press(at(100.0, 30.0), now);
        assert_eq!(m.offset(now), there);
        m.track(at(110.0, 30.0));
        assert_eq!(m.offset(now), there + 10.0);
    }
}
