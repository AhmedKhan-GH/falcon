//! Progress on the Dock icon while a long operation runs (the progress
//! modal's bar, mirrored). The tile's content view is the app icon with a
//! progress bar along its bottom edge; clearing it puts the plain icon back.

/// What the Dock tile shows: `None` clears it, `Some(None)` is an operation
/// whose size isn't known yet (an empty bar), `Some(Some(f))` is a fraction
/// from 0 to 1. Call from the main thread only; other threads do nothing.
pub(crate) fn set(progress: Option<Option<f32>>) {
    imp::set(progress);
}

/// The bar's value, 0 to 100, for a fraction.
fn percent(fraction: f32) -> f64 {
    f64::from(fraction.clamp(0.0, 1.0)) * 100.0
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImageView, NSProgressIndicator, NSProgressIndicatorStyle, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    struct Tile {
        bar: Retained<NSProgressIndicator>,
        shown: f64,
    }

    thread_local! {
        static TILE: RefCell<Option<Tile>> = const { RefCell::new(None) };
    }

    pub(super) fn set(progress: Option<Option<f32>>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let dock = NSApplication::sharedApplication(mtm).dockTile();
        TILE.with(|cell| {
            let mut tile = cell.borrow_mut();
            let Some(progress) = progress else {
                if tile.take().is_some() {
                    dock.setContentView(None);
                    dock.display();
                }
                return;
            };
            let value = super::percent(progress.unwrap_or(0.0));
            let tile = tile.get_or_insert_with(|| {
                let size = dock.size();
                let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
                let icon = NSImageView::initWithFrame(mtm.alloc(), frame);
                icon.setImage(NSApplication::sharedApplication(mtm).applicationIconImage().as_deref());
                let bar_frame = NSRect::new(NSPoint::new(size.width * 0.08, size.height * 0.06), NSSize::new(size.width * 0.84, size.height * 0.1));
                let bar = NSProgressIndicator::initWithFrame(mtm.alloc(), bar_frame);
                bar.setStyle(NSProgressIndicatorStyle::Bar);
                bar.setIndeterminate(false);
                bar.setMinValue(0.0);
                bar.setMaxValue(100.0);
                icon.addSubview(&bar);
                let view: Retained<NSView> = Retained::into_super(Retained::into_super(icon));
                dock.setContentView(Some(&view));
                Tile { bar, shown: -1.0 }
            });
            // The Dock redraws on display(), so only when the bar moved a notch.
            if (value - tile.shown).abs() >= 0.5 || tile.shown < 0.0 {
                tile.shown = value;
                tile.bar.setDoubleValue(value);
                dock.display();
            }
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn set(_progress: Option<Option<f32>>) {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_is_clamped() {
        assert_eq!(super::percent(0.5), 50.0);
        assert_eq!(super::percent(2.0), 100.0);
        assert_eq!(super::percent(-1.0), 0.0);
    }
}
