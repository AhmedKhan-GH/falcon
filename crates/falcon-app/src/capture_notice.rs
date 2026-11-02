//! The pop-up shown for a screenshot while falcon is the active app, with
//! the screenshot's thumbnail and name. Eagle shows its own pop-up
//! whatever has focus; macOS only shows a system notification while the
//! app is in the background, so falcon draws this one itself when it is
//! in front and posts the system one (`notify.rs`) when it is not.
//! Drawing lives in `ui/capture_notice.rs`; this module holds the rules,
//! the state and the thumbnail load.

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use eframe::egui;

/// Which notification a screenshot gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// falcon's own pop-up.
    InApp,
    /// The macOS notification.
    System,
    /// The notification switches are off.
    Nothing,
}

/// Exactly one of the two notifications fires, and only when the
/// preference switches (`Prefs::tells_capture`) allow it.
pub fn route(tells: bool, app_active: bool) -> Route {
    match (tells, app_active) {
        (false, _) => Route::Nothing,
        (true, true) => Route::InApp,
        (true, false) => Route::System,
    }
}

/// Seconds the pop-up takes to slide in from the right edge.
pub const SLIDE_IN: f32 = 0.2;
/// Seconds it stays once in (Eagle passes 1500 ms for a screenshot).
pub const HOLD: f32 = 1.5;
/// Side of the square thumbnail, in points.
pub const THUMB: f32 = 44.0;

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
}

/// Only one pop-up is up at a time: a new screenshot replaces the one
/// showing, as Eagle's single notification window does.
#[derive(Default)]
pub struct CaptureNotice {
    pub current: Option<Shown>,
}

impl CaptureNotice {
    /// Shows `name`, replacing any pop-up up now. `thumb` delivers the
    /// picture when it has loaded (None when there is none to load).
    pub fn show(&mut self, name: String, thumb: Option<Receiver<Option<egui::ColorImage>>>) {
        self.current = Some(Shown { name, at: Instant::now(), texture: None, size: [1, 1], loading: thumb, hovered: false, clicked: false });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_picks_exactly_one() {
        assert_eq!(route(true, true), Route::InApp);
        assert_eq!(route(true, false), Route::System);
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
}
