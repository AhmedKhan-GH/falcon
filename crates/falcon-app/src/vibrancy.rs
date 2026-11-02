//! Preferences ▸ General ▸ transparency effects: a translucent sidebar and
//! inspector over the macOS blur, on by default as Eagle's
//! `general.enableVibrancy` is.
//!
//! The window is always created transparent (`main.rs`), since eframe picks
//! the surface's alpha mode once, at creation. An `NSVisualEffectView` sits
//! behind wgpu's `CAMetalLayer`, and the theme draws the two side panels with
//! alpha. Everything else stays opaque: egui's blending never lowers the
//! alpha already in the frame, so [`Vibrancy::end_frame`] lays one opaque
//! rectangle under the frame, everywhere but the side panels, and text and
//! pictures can't show the desktop through.
//!
//! With the preference off the view is hidden and the layer and window are
//! opaque again, so the change applies at once, without a relaunch.

use eframe::egui::{self, Color32, Rect, Shape, layers::ShapeIdx};

/// The side panels' opacity with transparency on, 75%: Eagle's
/// `--color-theme-75`, the translucent background of its own Preferences
/// window on macOS.
pub const SIDE_ALPHA: u8 = 191;

/// Whether the window is created transparent. Only macOS has the blur.
pub const SUPPORTED: bool = cfg!(target_os = "macos");

/// A side panel's fill: `panel`, translucent while the blur is live.
pub fn side_fill(panel: Color32, live: bool) -> Color32 {
    if live {
        let [r, g, b, _] = panel.to_srgba_unmultiplied();
        Color32::from_rgba_unmultiplied(r, g, b, SIDE_ALPHA)
    } else {
        panel
    }
}

/// What the frame is cleared to: nothing while the blur is live, so that the
/// side panels' alpha reaches the window, else the opaque main colour.
pub fn clear_color(live: bool, main: Color32) -> [f32; 4] {
    if live { [0.0; 4] } else { main.to_normalized_gamma_f32() }
}

/// The part of the window that must stay opaque: all of it between the side
/// panels shown this frame.
pub fn opaque_rect(window: Rect, sidebar_right: Option<f32>, inspector_left: Option<f32>) -> Rect {
    let mut r = window;
    if let Some(x) = sidebar_right {
        r.min.x = x.clamp(window.left(), window.right());
    }
    if let Some(x) = inspector_left {
        r.max.x = x.clamp(r.left(), window.right());
    }
    r
}

/// What the app keeps from frame to frame.
#[derive(Default)]
pub struct Vibrancy {
    /// The blur is in place, so the side panels may be translucent.
    pub live: bool,
    /// The side panels' inner edges this frame, where they are shown.
    pub sidebar_right: Option<f32>,
    pub inspector_left: Option<f32>,
    base: Option<ShapeIdx>,
    #[cfg(target_os = "macos")]
    appearance: Option<bool>,
}

impl Vibrancy {
    /// Keeps a place at the bottom of the frame for the opaque rectangle.
    pub fn begin_frame(&mut self, ui: &egui::Ui) {
        self.sidebar_right = None;
        self.inspector_left = None;
        self.base = Some(ui.painter().add(Shape::Noop));
    }

    /// Fills the place kept by [`Self::begin_frame`].
    pub fn end_frame(&mut self, ui: &egui::Ui, main: Color32) {
        if let Some(idx) = self.base.take()
            && self.live
        {
            let rect = opaque_rect(ui.ctx().content_rect(), self.sidebar_right, self.inspector_left);
            ui.painter().set(idx, Shape::rect_filled(rect, 0.0, main));
        }
    }

    /// Brings the window in line with the preference. Call it every frame:
    /// wgpu makes the layer non-opaque again whenever it configures the
    /// surface. A change of `live` asks for another frame to draw it.
    pub fn sync(&mut self, ctx: &egui::Context, frame: &eframe::Frame, enabled: bool) {
        let live = self.apply(ctx, frame, enabled);
        if live != self.live {
            self.live = live;
            ctx.request_repaint();
        }
    }

    #[cfg(target_os = "macos")]
    fn apply(&mut self, ctx: &egui::Context, frame: &eframe::Frame, enabled: bool) -> bool {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = frame.window_handle() else { return false };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return false };
        let Some(mtm) = objc2::MainThreadMarker::new() else { return false };
        let dark = ctx.theme() == egui::Theme::Dark;
        // SAFETY: `ns_view` is winit's live content view, used on the main
        // thread inside `ui`.
        unsafe { imp::apply(h.ns_view.as_ptr().cast(), enabled, dark, &mut self.appearance, mtm) }
    }

    #[cfg(not(target_os = "macos"))]
    fn apply(&mut self, _: &egui::Context, _: &eframe::Frame, _: bool) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{ClassType as _, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use objc2_foundation::{NSObjectProtocol as _, NSPoint, NSRect, NSString};

    define_class!(
        // SAFETY: NSVisualEffectView may be subclassed, and Backdrop has no
        // Drop.
        #[unsafe(super(NSVisualEffectView))]
        #[thread_kind = MainThreadOnly]
        #[name = "FalconWindowBackdrop"]
        struct Backdrop;

        impl Backdrop {
            /// Clicks, scrolling and the cursor go on to winit's view.
            #[unsafe(method(hitTest:))]
            fn hit_test(&self, _point: NSPoint) -> *mut NSView {
                std::ptr::null_mut()
            }
        }
    );

    /// Sits under every sibling, wgpu's layer and a web page's view alike.
    const BACKDROP_Z: f64 = -1.0;

    fn find(view: &NSView) -> Option<Retained<NSView>> {
        view.subviews().iter().find(|v| v.isKindOfClass(Backdrop::class()))
    }

    fn install(view: &NSView, mtm: MainThreadMarker) -> Retained<NSView> {
        let frame: NSRect = view.bounds();
        // SAFETY: NSView's designated initialiser; Backdrop has no ivars.
        let backdrop: Retained<Backdrop> = unsafe { msg_send![Backdrop::alloc(mtm), initWithFrame: frame] };
        backdrop.setMaterial(NSVisualEffectMaterial::Sidebar);
        backdrop.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        backdrop.setState(NSVisualEffectState::FollowsWindowActiveState);
        backdrop.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        backdrop.setWantsLayer(true);
        view.addSubview_positioned_relativeTo(&backdrop, NSWindowOrderingMode::Below, None);
        Retained::into_super(Retained::into_super(backdrop))
    }

    /// Sets a layer's `opaque` when it differs.
    ///
    /// # Safety
    /// `layer` must be a live CALayer.
    unsafe fn set_layer_opaque(layer: *mut AnyObject, opaque: bool) {
        // SAFETY: CALayer accessors.
        unsafe {
            let now: Bool = msg_send![layer, isOpaque];
            if now.as_bool() != opaque {
                let _: () = msg_send![layer, setOpaque: opaque];
            }
        }
    }

    /// Makes the window opaque or lets it show through, when it differs.
    fn set_window_opaque(view: &NSView, opaque: bool) {
        let Some(window) = view.window() else { return };
        if window.isOpaque() == opaque {
            return;
        }
        window.setOpaque(opaque);
        if let Some(class) = AnyClass::get(c"NSColor") {
            // SAFETY: NSColor class methods returning a live colour, and
            // NSWindow's setter, which retains it.
            unsafe {
                let color: *mut AnyObject = if opaque { msg_send![class, windowBackgroundColor] } else { msg_send![class, clearColor] };
                let _: () = msg_send![&*window, setBackgroundColor: color];
            }
        }
        window.invalidateShadow();
    }

    /// Gives the blur falcon's light or dark look rather than the system's.
    fn set_appearance(view: &NSView, dark: bool) {
        let Some(class) = AnyClass::get(c"NSAppearance") else { return };
        let name = NSString::from_str(if dark { "NSAppearanceNameDarkAqua" } else { "NSAppearanceNameAqua" });
        // SAFETY: NSAppearance's class method returns a live appearance or
        // nil, and NSView's setter takes either.
        unsafe {
            let appearance: *mut AnyObject = msg_send![class, appearanceNamed: &*name];
            let _: () = msg_send![view, setAppearance: appearance];
        }
    }

    /// True once the blur is showing behind a transparent layer.
    ///
    /// # Safety
    /// `view` must be a live NSView, used on the main thread.
    pub unsafe fn apply(view: *mut NSView, enabled: bool, dark: bool, appearance: &mut Option<bool>, mtm: MainThreadMarker) -> bool {
        // SAFETY: as the caller ensures.
        let metal = unsafe { crate::colorspace::imp::metal_layer_of_view(view.cast()) };
        // SAFETY: as the caller ensures.
        let view = unsafe { &*view };
        // Without wgpu's layer there is nothing to put the blur behind.
        let on = enabled && !metal.is_null();
        let existing = find(view);
        if !on {
            if let Some(backdrop) = existing {
                backdrop.setHidden(true);
            }
            if !metal.is_null() {
                // SAFETY: a live CALayer.
                unsafe { set_layer_opaque(metal, true) };
            }
            set_window_opaque(view, true);
            return false;
        }
        let backdrop = existing.unwrap_or_else(|| {
            *appearance = None;
            install(view, mtm)
        });
        backdrop.setHidden(false);
        if *appearance != Some(dark) {
            set_appearance(&backdrop, dark);
            *appearance = Some(dark);
        }
        // SAFETY: `metal` is a live CALayer, and the backdrop is layer-backed
        // (`install`), so `layer` returns a live one.
        unsafe {
            let layer: *mut AnyObject = msg_send![&*backdrop, layer];
            if !layer.is_null() {
                let z: f64 = msg_send![layer, zPosition];
                if z != BACKDROP_Z {
                    let _: () = msg_send![layer, setZPosition: BACKDROP_Z];
                }
            }
            set_layer_opaque(metal, false);
        }
        set_window_opaque(view, false);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    const PANEL: Color32 = Color32::from_rgb(0x1f, 0x20, 0x23);

    #[test]
    fn side_panels_are_translucent_only_while_the_blur_is_live() {
        assert_eq!(side_fill(PANEL, false), PANEL);
        let fill = side_fill(PANEL, true);
        assert_eq!(fill.a(), SIDE_ALPHA);
        // Same colour, within rounding of the premultiplied store.
        let [r, g, b, _] = fill.to_srgba_unmultiplied();
        for (got, want) in [(r, 0x1f), (g, 0x20), (b, 0x23)] {
            assert!(got.abs_diff(want) <= 1, "{got} against {want}");
        }
    }

    #[test]
    fn the_frame_is_cleared_to_nothing_only_while_the_blur_is_live() {
        assert_eq!(clear_color(true, PANEL), [0.0; 4]);
        assert_eq!(clear_color(false, PANEL)[3], 1.0);
    }

    #[test]
    fn everything_between_the_side_panels_stays_opaque() {
        let w = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 700.0));
        assert_eq!(opaque_rect(w, None, None), w);
        assert_eq!(opaque_rect(w, Some(200.0), None), Rect::from_min_max(pos2(200.0, 0.0), pos2(1000.0, 700.0)));
        assert_eq!(opaque_rect(w, None, Some(740.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(740.0, 700.0)));
        assert_eq!(opaque_rect(w, Some(200.0), Some(740.0)), Rect::from_min_max(pos2(200.0, 0.0), pos2(740.0, 700.0)));
        // Panels wider than the window can't turn the rectangle inside out.
        let r = opaque_rect(w, Some(1200.0), Some(100.0));
        assert!(r.width() == 0.0 && r.left() == 1000.0);
    }

    /// egui's blending, from `egui_wgpu::renderer`: colour is premultiplied
    /// "over", alpha `src * (1 - dst) + dst`.
    fn over(dst: [f32; 2], src: [f32; 2]) -> [f32; 2] {
        [src[0] + dst[0] * (1.0 - src[1]), src[1] * (1.0 - dst[1]) + dst[1]]
    }

    #[test]
    fn nothing_drawn_on_the_opaque_part_lowers_its_alpha() {
        let mut px = [0.1, 1.0];
        for src in [[0.0, 0.0], [0.2, 0.2], [0.5, 0.5], [1.0, 1.0]] {
            px = over(px, src);
            assert_eq!(px[1], 1.0);
        }
    }

    #[test]
    fn text_on_a_side_panel_is_more_opaque_than_the_panel() {
        let a = f32::from(SIDE_ALPHA) / 255.0;
        // Over a cleared pixel, then a whole glyph pixel and a faint edge.
        let panel = over([0.0, 0.0], [0.1 * a, a]);
        assert!((panel[1] - a).abs() < 1e-6);
        assert_eq!(over(panel, [1.0, 1.0])[1], 1.0);
        assert!(over(panel, [0.3, 0.3])[1] > a);
    }

    #[test]
    fn off_is_the_default_and_the_choice_is_kept() {
        let mut p = crate::prefs::Prefs::default();
        assert!(!p.vibrancy, "Eagle's main window is opaque, so the blur is off by default");
        p.vibrancy = true;
        let text = serde_json::to_string(&p).unwrap();
        let back: crate::prefs::Prefs = serde_json::from_str(&text).unwrap();
        assert!(back.vibrancy);
        // Saved state from before the setting has none and gets the default.
        let old: crate::prefs::Prefs = serde_json::from_str(r#"{"hover_zoom":false}"#).unwrap();
        assert!(!old.vibrancy);
    }
}
