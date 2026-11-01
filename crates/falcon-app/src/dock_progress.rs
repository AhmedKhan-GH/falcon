//! Progress on the Dock icon while a long operation runs (the progress
//! modal's bar, mirrored). The tile's content view is the app icon with a
//! progress bar along its bottom edge; clearing it puts the plain icon back.

/// What the Dock tile shows: `None` clears it, `Some(None)` is an operation
/// whose size isn't known yet (an empty bar), `Some(Some(f))` is a fraction
/// from 0 to 1. This is the progress modal's slot; the background import has
/// its own (`set_import`) and the tile shows both combined. Call from the
/// main thread only; other threads do nothing.
pub(crate) fn set(progress: Option<Option<f32>>) {
    slots::with(|s| s.modal = progress);
    slots::apply();
}

/// The background import's share of the tile: `Some((done, total))` while
/// files are being imported, `None` when the queue is empty. The progress
/// modal (`set`) wins while it is shown; otherwise the tile shows this
/// fraction. Call from the main thread only.
pub(crate) fn set_import(progress: Option<(usize, usize)>) {
    slots::with(|s| s.import = import_fraction(progress));
    slots::apply();
}

/// Eagle hides the Dock bar once an import is 97% done (and when it ends).
const IMPORT_HIDE_AT: f32 = 0.97;

/// The fraction the Dock tile shows for an import of `done` of `total`
/// files, or `None` for no bar (an empty queue, or nearly finished).
fn import_fraction(progress: Option<(usize, usize)>) -> Option<f32> {
    let (done, total) = progress?;
    if total == 0 || done >= total {
        return None;
    }
    let f = done as f32 / total as f32;
    (f < IMPORT_HIDE_AT).then_some(f)
}

/// What the tile shows given the progress modal's state and the import's:
/// the modal while it is shown, else the import, else nothing.
fn combine(modal: Option<Option<f32>>, import: Option<f32>) -> Option<Option<f32>> {
    modal.or(import.map(Some))
}

mod slots {
    use std::cell::RefCell;

    #[derive(Default)]
    pub(super) struct Slots {
        pub modal: Option<Option<f32>>,
        pub import: Option<f32>,
        /// What was last handed to AppKit, to skip repeats.
        pub applied: Option<Option<f32>>,
    }

    thread_local! {
        static SLOTS: RefCell<Slots> = RefCell::new(Slots::default());
    }

    pub(super) fn with(f: impl FnOnce(&mut Slots)) {
        SLOTS.with(|s| f(&mut s.borrow_mut()));
    }

    /// Shows the combined state if it changed since last time.
    pub(super) fn apply() {
        let shown = SLOTS.with(|s| {
            let mut s = s.borrow_mut();
            let c = super::combine(s.modal, s.import);
            (c != s.applied).then(|| {
                s.applied = c;
                c
            })
        });
        if let Some(c) = shown {
            super::imp::set(c);
        }
    }
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
    fn the_modal_wins_over_the_import() {
        assert_eq!(super::combine(Some(Some(0.2)), Some(0.8)), Some(Some(0.2)));
        assert_eq!(super::combine(Some(None), Some(0.8)), Some(None));
        assert_eq!(super::combine(None, Some(0.8)), Some(Some(0.8)));
        assert_eq!(super::combine(None, None), None);
    }

    #[test]
    fn the_import_bar_shows_until_it_is_nearly_done() {
        assert_eq!(super::import_fraction(None), None);
        assert_eq!(super::import_fraction(Some((0, 0))), None);
        assert_eq!(super::import_fraction(Some((5, 10))), Some(0.5));
        assert_eq!(super::import_fraction(Some((0, 4))), Some(0.0));
        assert_eq!(super::import_fraction(Some((97, 100))), None);
        assert_eq!(super::import_fraction(Some((96, 100))), Some(0.96));
        assert_eq!(super::import_fraction(Some((10, 10))), None);
    }

    #[test]
    fn percent_is_clamped() {
        assert_eq!(super::percent(0.5), 50.0);
        assert_eq!(super::percent(2.0), 100.0);
        assert_eq!(super::percent(-1.0), 0.0);
    }
}
