//! Preferences ▸ General ▸ transparency effects, Eagle's
//! `general.enableVibrancy`: menus and boxes go translucent while the main
//! window stays opaque. Off by default here, since falcon can only do part
//! of it (below).
//!
//! What each surface can do differs, which is what [`Surface`] writes down:
//!
//! - A window of its own that is a box (Preferences, Collect) gets a real
//!   blur of the desktop. An `NSVisualEffectView` sits behind wgpu's
//!   `CAMetalLayer` and the window is filled at 75%, the alpha of Eagle's
//!   `--color-theme-75`.
//! - Menus, popups, tooltips and dialogs inside a window are filled at 94%,
//!   with no blur. egui can't leave its window, so they can only show that
//!   window's content, and blurring it would take a second render of the
//!   frame (Eagle's CSS `backdrop-filter` gets that from the browser). A
//!   native view can't help either: it would have to sit above the Metal
//!   layer, and would cover the text the same layer draws.
//! - The main window and preview windows stay opaque. egui's blending never
//!   lowers the alpha already in the frame and the frame starts from an
//!   opaque clear ([`clear_color`]), so screenshots of the window are as they
//!   are with the preference off.
//! - The menu bar is AppKit's (`menu.rs`) and the system makes it vibrant
//!   whatever this preference says.
//!
//! eframe picks the surfaces' alpha mode once, from the main window, so that
//! window is always created transparent (`main.rs`) and
//! [`sync_main_window`] makes it opaque. Separate windows are created transparent too and follow the
//! preference through [`sync_window`], so a change applies at once, without a
//! relaunch.

use eframe::egui::{self, Color32};

/// The opacity of a window of its own over the blur, 75%: Eagle's
/// `--color-theme-75`, the translucent background of its Preferences window
/// on macOS.
pub const WINDOW_ALPHA: u8 = 191;

/// The opacity of a box inside a window, 94%. With no blur to soften what is
/// behind it, the captions of the grid read through a menu's text at 90%.
pub const BOX_ALPHA: u8 = 240;

/// Whether separate windows can be blurred. Only macOS has the blur.
pub const SUPPORTED: bool = cfg!(target_os = "macos");

/// What the preference treats differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// The main window with its panels and grid. Preview windows, whose
    /// surface it also describes, are simply never touched.
    MainWindow,
    /// A context menu, popup, tooltip or dialog, in any window.
    Box,
    /// A box that is a window of its own: Preferences, Collect.
    BoxWindow,
}

impl Surface {
    /// The surface's opacity under the preference.
    pub fn alpha(self, enabled: bool) -> u8 {
        match self {
            Surface::Box if enabled => BOX_ALPHA,
            Surface::BoxWindow if enabled => WINDOW_ALPHA,
            _ => u8::MAX,
        }
    }

    /// Whether the desktop shows blurred behind the surface.
    pub fn blurred(self, enabled: bool) -> bool {
        enabled && SUPPORTED && self == Surface::BoxWindow
    }

    /// `base` at the surface's opacity.
    pub fn fill(self, base: Color32, enabled: bool) -> Color32 {
        let alpha = self.alpha(enabled);
        if alpha == u8::MAX {
            return base;
        }
        let [r, g, b, _] = base.to_srgba_unmultiplied();
        Color32::from_rgba_unmultiplied(r, g, b, alpha)
    }
}

/// What the main window is cleared to: its opaque main colour, whatever the
/// preference says. Separate windows are cleared to nothing by eframe and
/// fill themselves.
pub fn clear_color(main: Color32, enabled: bool) -> [f32; 4] {
    Surface::MainWindow.fill(main, enabled).to_normalized_gamma_f32()
}

/// Brings the frame's style in line with the preference: egui draws menus,
/// popups, tooltips and dialogs with `window_fill`. Call it every frame, as a
/// theme change sets the fill back.
pub fn sync_boxes(ctx: &egui::Context, panel: Color32, enabled: bool) {
    let fill = Surface::Box.fill(panel, enabled);
    if ctx.global_style().visuals.window_fill != fill {
        ctx.global_style_mut(|s| s.visuals.window_fill = fill);
    }
}

/// Brings the main window in line with the rules, which keep it opaque. Call
/// it every frame: wgpu makes the layer non-opaque again whenever it configures
/// the surface.
#[cfg(target_os = "macos")]
pub fn sync_main_window(frame: &eframe::Frame, enabled: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = frame.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    // SAFETY: `ns_view` is winit's live content view, used on the main
    // thread inside `ui`.
    unsafe { imp::apply(h.ns_view.as_ptr().cast(), Surface::MainWindow.blurred(enabled), false, mtm) };
}

#[cfg(not(target_os = "macos"))]
pub fn sync_main_window(_: &eframe::Frame, _: bool) {}

/// Brings the separate window titled `title` in line with the preference:
/// blurred or opaque, in falcon's light or dark look. Call it every frame
/// from inside the window's viewport. eframe has no handle for a window
/// besides the main one, so the NSWindow is found by its title, as for a
/// preview window's opacity.
#[cfg(target_os = "macos")]
pub fn sync_window(ctx: &egui::Context, title: &str, enabled: bool) {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let dark = ctx.theme() == egui::Theme::Dark;
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    for window in app.windows().iter() {
        if window.title().to_string() == title {
            if let Some(view) = window.contentView() {
                // SAFETY: the window's live content view, on the main thread.
                unsafe { imp::apply(objc2::rc::Retained::as_ptr(&view).cast_mut(), Surface::BoxWindow.blurred(enabled), dark, mtm) };
            }
            return;
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn sync_window(_: &egui::Context, _: &str, _: bool) {}

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
        // Collect floats over other apps' windows, so it is seldom the active one.
        backdrop.setState(NSVisualEffectState::Active);
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

    /// Gives the blur falcon's light or dark look rather than the system's,
    /// when it differs.
    fn set_appearance(view: &NSView, dark: bool) {
        let Some(class) = AnyClass::get(c"NSAppearance") else { return };
        let name = NSString::from_str(if dark { "NSAppearanceNameDarkAqua" } else { "NSAppearanceNameAqua" });
        // SAFETY: NSAppearance's class method returns a live appearance or
        // nil, NSView's accessors take and give either, and the same
        // appearance object is handed out for a name.
        unsafe {
            let appearance: *mut AnyObject = msg_send![class, appearanceNamed: &*name];
            let now: *mut AnyObject = msg_send![view, appearance];
            if now != appearance {
                let _: () = msg_send![view, setAppearance: appearance];
            }
        }
    }

    /// Shows the blur behind a transparent layer, or makes the window opaque.
    ///
    /// # Safety
    /// `view` must be a live NSView, used on the main thread.
    pub unsafe fn apply(view: *mut NSView, enabled: bool, dark: bool, mtm: MainThreadMarker) {
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
            return;
        }
        let backdrop = existing.unwrap_or_else(|| install(view, mtm));
        backdrop.setHidden(false);
        set_appearance(&backdrop, dark);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Color32 = Color32::from_rgb(0x1f, 0x20, 0x23);

    #[test]
    fn with_the_preference_off_every_surface_is_opaque_and_unblurred() {
        for s in [Surface::MainWindow, Surface::Box, Surface::BoxWindow] {
            assert_eq!(s.alpha(false), u8::MAX, "{s:?}");
            assert_eq!(s.fill(PANEL, false), PANEL, "{s:?}");
            assert!(!s.blurred(false), "{s:?}");
        }
    }

    #[test]
    fn with_the_preference_on_only_boxes_are_translucent() {
        assert_eq!(Surface::MainWindow.fill(PANEL, true), PANEL);
        assert_eq!(Surface::Box.alpha(true), BOX_ALPHA);
        assert_eq!(Surface::BoxWindow.alpha(true), WINDOW_ALPHA);
    }

    #[test]
    fn only_a_window_of_its_own_can_blur_the_desktop() {
        assert_eq!(Surface::BoxWindow.blurred(true), SUPPORTED);
        // A box inside a window can't leave it.
        assert!(!Surface::Box.blurred(true));
        // So the main window never needs the effect view.
        assert!(!Surface::MainWindow.blurred(true));
    }

    #[test]
    fn a_translucent_fill_keeps_the_colour() {
        let fill = Surface::BoxWindow.fill(PANEL, true);
        assert_eq!(fill.a(), WINDOW_ALPHA);
        // Same colour, within rounding of the premultiplied store.
        let [r, g, b, _] = fill.to_srgba_unmultiplied();
        for (got, want) in [(r, 0x1f), (g, 0x20), (b, 0x23)] {
            assert!(got.abs_diff(want) <= 1, "{got} against {want}");
        }
    }

    #[test]
    fn the_main_window_is_cleared_opaque() {
        assert_eq!(clear_color(PANEL, false)[3], 1.0);
        assert_eq!(clear_color(PANEL, true)[3], 1.0);
    }

    #[test]
    fn boxes_follow_the_preference_and_a_theme_change() {
        let ctx = egui::Context::default();
        sync_boxes(&ctx, PANEL, true);
        assert_eq!(ctx.global_style().visuals.window_fill, Surface::Box.fill(PANEL, true));
        // A theme change puts the opaque fill back.
        ctx.global_style_mut(|s| s.visuals.window_fill = Color32::WHITE);
        sync_boxes(&ctx, PANEL, true);
        assert_eq!(ctx.global_style().visuals.window_fill.a(), BOX_ALPHA);
        // Off restores the panel colour exactly.
        sync_boxes(&ctx, PANEL, false);
        assert_eq!(ctx.global_style().visuals.window_fill, PANEL);
    }

    /// egui's blending, from `egui_wgpu::renderer`: colour is premultiplied
    /// "over", alpha `src * (1 - dst) + dst`.
    fn over(dst: [f32; 2], src: [f32; 2]) -> [f32; 2] {
        [src[0] + dst[0] * (1.0 - src[1]), src[1] * (1.0 - dst[1]) + dst[1]]
    }

    #[test]
    fn a_translucent_box_never_lowers_the_alpha_of_the_opaque_main_window() {
        let a = f32::from(BOX_ALPHA) / 255.0;
        let mut px = [0.1, 1.0];
        for src in [[0.0, 0.0], [0.1 * a, a], [0.5, 0.5], [1.0, 1.0]] {
            px = over(px, src);
            assert_eq!(px[1], 1.0);
        }
    }

    #[test]
    fn text_on_a_blurred_window_is_more_opaque_than_the_window() {
        let a = f32::from(WINDOW_ALPHA) / 255.0;
        // Over a cleared pixel, then a whole glyph pixel and a faint edge.
        let panel = over([0.0, 0.0], [0.1 * a, a]);
        assert!((panel[1] - a).abs() < 1e-6);
        assert_eq!(over(panel, [1.0, 1.0])[1], 1.0);
        assert!(over(panel, [0.3, 0.3])[1] > a);
    }

    #[test]
    fn the_switch_shows_while_preferences_is_open_and_cancel_takes_it_back() {
        use crate::prefs::{Draft, Panels, Prefs, Window};
        let saved = Prefs::default();
        let mut w = Window::default();
        assert_eq!(w.live_vibrancy(), None, "closed, the saved value counts");
        let prefs = Prefs { vibrancy: true, ..Prefs::default() };
        w.show_with(Draft { prefs, panels: Panels { sidebar: true, inspector: true }, keys: Default::default(), launch_at_login: false });
        assert_eq!(w.live_vibrancy(), Some(true));
        w.close();
        assert!(!w.live_vibrancy().unwrap_or(saved.vibrancy));
    }

    #[test]
    fn off_is_the_default_and_the_choice_is_kept() {
        let mut p = crate::prefs::Prefs::default();
        assert!(!p.vibrancy, "falcon's boxes can't blur as Eagle's do, so it is off by default");
        p.vibrancy = true;
        let text = serde_json::to_string(&p).unwrap();
        let back: crate::prefs::Prefs = serde_json::from_str(&text).unwrap();
        assert!(back.vibrancy);
        // Saved state from before the setting has none and gets the default.
        let old: crate::prefs::Prefs = serde_json::from_str(r#"{"hover_zoom":false}"#).unwrap();
        assert!(!old.vibrancy);
    }
}
