//! Where files dropped from Finder landed in the window.
//!
//! While a drag from another app is over the window, macOS sends no mouse
//! moves, so egui's pointer position is stale when the drop arrives. The
//! system's mouse location is read instead and turned into egui points.

use eframe::egui::{Context, Pos2, Rect};

/// Where a drop from Finder lands, which decides what it does (after
/// Eagle's three drop handlers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The grid (or anywhere outside the sidebar): files, and the files in
    /// dropped folders, go into the open folder, flattened.
    Grid,
    /// A folder row in the sidebar: files go into that folder; folders are
    /// left alone.
    Folder(String),
    /// The rest of the sidebar: each folder is imported with its tree
    /// recreated at the top level; loose files are left alone.
    Sidebar,
}

/// The drop target at `pos`, given the sidebar's rect and its visible
/// folder rows from the last frame.
pub fn target(pos: Option<Pos2>, sidebar: Option<Rect>, rows: &[(Rect, String)]) -> Target {
    let Some(pos) = pos else { return Target::Grid };
    if !sidebar.is_some_and(|s| s.contains(pos)) {
        return Target::Grid;
    }
    match rows.iter().find(|(r, _)| r.contains(pos)) {
        Some((_, id)) => Target::Folder(id.clone()),
        None => Target::Sidebar,
    }
}

/// The drop position in egui points, if it can be told.
pub fn at_drop(ctx: &Context) -> Option<Pos2> {
    platform::mouse_in_window(ctx).or_else(|| ctx.pointer_latest_pos())
}

/// Turns a mouse location in screen points, measured from the top left of
/// the main display, into a window position in egui points. `inner_min` is
/// the window's content origin as egui reports it (screen points divided
/// by `zoom`).
pub fn to_window(mouse: Pos2, inner_min: Pos2, zoom: f32) -> Pos2 {
    Pos2::new(mouse.x / zoom - inner_min.x, mouse.y / zoom - inner_min.y)
}

#[cfg(target_os = "macos")]
mod platform {
    use eframe::egui::{Context, Pos2};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};

    pub fn mouse_in_window(ctx: &Context) -> Option<Pos2> {
        let mtm = MainThreadMarker::new()?;
        // AppKit measures from the bottom left of the main display; winit
        // (and so egui) from its top left.
        let main_height = NSScreen::screens(mtm).firstObject()?.frame().size.height;
        let p = NSEvent::mouseLocation();
        let inner = ctx.input(|i| i.viewport().inner_rect)?;
        let mouse = Pos2::new(p.x as f32, (main_height - p.y) as f32);
        Some(super::to_window(mouse, inner.min, ctx.zoom_factor()))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use eframe::egui::{Context, Pos2};

    pub fn mouse_in_window(_ctx: &Context) -> Option<Pos2> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        let side = Rect::from_min_max(Pos2::ZERO, Pos2::new(200.0, 600.0));
        let rows = vec![(Rect::from_min_max(Pos2::new(0.0, 100.0), Pos2::new(200.0, 128.0)), "F1".to_string())];
        assert_eq!(target(Some(Pos2::new(50.0, 110.0)), Some(side), &rows), Target::Folder("F1".into()));
        assert_eq!(target(Some(Pos2::new(50.0, 300.0)), Some(side), &rows), Target::Sidebar);
        assert_eq!(target(Some(Pos2::new(400.0, 110.0)), Some(side), &rows), Target::Grid);
        assert_eq!(target(Some(Pos2::new(50.0, 110.0)), None, &rows), Target::Grid, "a hidden sidebar takes nothing");
        assert_eq!(target(None, Some(side), &rows), Target::Grid);
    }

    #[test]
    fn screen_to_window_points() {
        assert_eq!(to_window(Pos2::new(300.0, 200.0), Pos2::new(100.0, 50.0), 1.0), Pos2::new(200.0, 150.0));
        // At 2× zoom egui points are twice as large.
        assert_eq!(to_window(Pos2::new(300.0, 200.0), Pos2::new(50.0, 25.0), 2.0), Pos2::new(100.0, 75.0));
    }
}
