//! The menu bar icon, after Eagle's tray icon on macOS (shown while
//! Preferences ▸ General ▸ "Show menu bar icon" is on): its menu has Show
//! falcon, the three screen captures (with their global keys beside
//! them) and Quit. A choice is queued for the app's next frame, like the
//! menu bar's (`menu.rs`).

use eframe::egui::KeyboardShortcut;

use crate::capture::Mode;
use crate::shortcuts::{Command, Shortcuts};

/// One line of the icon's menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    Item(Choice),
    Separator,
}

/// What the icon's menu can do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Show,
    Capture(Mode),
    Quit,
}

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Choice::Show => "Show Falcon",
            Choice::Capture(m) => m.label(),
            Choice::Quit => "Quit Falcon",
        }
    }

    /// The command whose global key the item shows.
    pub fn command(self) -> Option<Command> {
        match self {
            Choice::Show => Some(Command::ShowFalcon),
            Choice::Capture(m) => Some(Command::Capture(m)),
            Choice::Quit => None,
        }
    }
}

/// The menu, in Eagle's order.
pub const MENU: [Entry; 7] = [
    Entry::Item(Choice::Show),
    Entry::Separator,
    Entry::Item(Choice::Capture(Mode::Area)),
    Entry::Item(Choice::Capture(Mode::Window)),
    Entry::Item(Choice::Capture(Mode::Full)),
    Entry::Separator,
    Entry::Item(Choice::Quit),
];

/// The key each item shows: its global key, when registered (the capture
/// keys only while they are switched on).
pub fn shown_key(choice: Choice, keys: &Shortcuts, captures: bool) -> Option<KeyboardShortcut> {
    let cmd = choice.command()?;
    if matches!(cmd, Command::Capture(_)) && !captures {
        return None;
    }
    keys.binding(cmd)
}

/// The icon as last put in the menu bar.
#[derive(Default)]
pub struct Tray {
    /// The keys shown in the menu when it was built; `None` while hidden.
    built: Option<Vec<Option<KeyboardShortcut>>>,
}

impl Tray {
    /// Shows or hides the icon, rebuilding its menu when the keys it shows
    /// change. Call on the main thread.
    pub fn sync(&mut self, ctx: &eframe::egui::Context, show: bool, keys: &Shortcuts, captures: bool) {
        let want = show.then(|| MENU.iter().map(|e| if let Entry::Item(c) = e { shown_key(*c, keys, captures) } else { None }).collect::<Vec<_>>());
        if want == self.built {
            return;
        }
        match &want {
            Some(k) => platform::show(ctx, k),
            None => platform::hide(),
        }
        self.built = want;
    }
}

/// The items chosen since the last call, oldest first.
pub fn take_choices() -> Vec<Choice> {
    platform::take()
}

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::RefCell;
    use std::sync::{Mutex, OnceLock};

    use eframe::egui::KeyboardShortcut;
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
    use objc2_foundation::NSString;

    use super::{Choice, Entry, MENU};
    use crate::menu::key_equivalent;

    static CHOICES: Mutex<Vec<Choice>> = Mutex::new(Vec::new());
    static REPAINT: OnceLock<eframe::egui::Context> = OnceLock::new();

    thread_local! {
        static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) };
        static ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and Target has
        // no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FalconTrayTarget"]
        struct Target;

        impl Target {
            #[unsafe(method(falconTrayItem:))]
            fn clicked(&self, item: &NSMenuItem) {
                let Some(Entry::Item(choice)) = usize::try_from(item.tag()).ok().and_then(|i| MENU.get(i)).copied() else { return };
                CHOICES.lock().unwrap_or_else(|p| p.into_inner()).push(choice);
                if let Some(ctx) = REPAINT.get() {
                    ctx.request_repaint();
                }
            }
        }

        unsafe impl NSObjectProtocol for Target {}
    );

    impl Target {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn show(ctx: &eframe::egui::Context, keys: &[Option<KeyboardShortcut>]) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let _ = REPAINT.set(ctx.clone());
        let target = TARGET.with(|t| t.borrow_mut().get_or_insert_with(|| Target::new(mtm)).clone());
        let item = ITEM.with(|slot| {
            slot.borrow_mut()
                .get_or_insert_with(|| {
                    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
                    if let Some(button) = item.button(mtm) {
                        // A template symbol follows the menu bar's colours.
                        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("bird"), Some(&NSString::from_str("Falcon")));
                        match image {
                            Some(img) => {
                                img.setTemplate(true);
                                button.setImage(Some(&img));
                            }
                            None => button.setTitle(&NSString::from_str("Falcon")),
                        }
                    }
                    item
                })
                .clone()
        });
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        for (i, (entry, key)) in MENU.iter().zip(keys).enumerate() {
            match entry {
                Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                Entry::Item(choice) => {
                    let ke = key.as_ref().and_then(key_equivalent);
                    let k = NSString::from_str(ke.as_ref().map_or("", |k| k.key.as_str()));
                    // SAFETY: the action is a method of `target`, which takes the item.
                    let it = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(choice.label()), Some(sel!(falconTrayItem:)), &k) };
                    if let Some(k) = ke {
                        let mut mask = NSEventModifierFlags::empty();
                        for (on, flag) in [(k.command, NSEventModifierFlags::Command), (k.shift, NSEventModifierFlags::Shift), (k.alt, NSEventModifierFlags::Option), (k.ctrl, NSEventModifierFlags::Control)] {
                            if on {
                                mask |= flag;
                            }
                        }
                        it.setKeyEquivalentModifierMask(mask);
                    }
                    // SAFETY: `target` lives in TARGET for the rest of the process.
                    unsafe { it.setTarget(Some(&target)) };
                    it.setTag(i as isize);
                    menu.addItem(&it);
                }
            }
        }
        item.setMenu(Some(&menu));
    }

    pub fn hide() {
        ITEM.with(|slot| {
            if let Some(item) = slot.borrow_mut().take() {
                NSStatusBar::systemStatusBar().removeStatusItem(&item);
            }
        });
    }

    pub fn take() -> Vec<Choice> {
        std::mem::take(&mut *CHOICES.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use eframe::egui::KeyboardShortcut;

    pub fn show(_ctx: &eframe::egui::Context, _keys: &[Option<KeyboardShortcut>]) {}
    pub fn hide() {}
    pub fn take() -> Vec<super::Choice> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_follows_eagle() {
        let labels: Vec<_> = MENU.iter().filter_map(|e| if let Entry::Item(c) = e { Some(c.label()) } else { None }).collect();
        assert_eq!(labels, ["Show Falcon", "Capture Area", "Capture Window", "Capture Full Screen", "Quit Falcon"]);
    }

    #[test]
    fn items_show_registered_keys_only() {
        let keys = Shortcuts::default();
        let area = Choice::Capture(Mode::Area);
        assert_eq!(shown_key(area, &keys, true).map(|c| crate::shortcuts::format(&c)).as_deref(), Some("Command + Alt + E"));
        assert_eq!(shown_key(area, &keys, false), None, "capture keys switched off");
        assert_eq!(shown_key(Choice::Show, &keys, true), None, "no default");
        assert_eq!(shown_key(Choice::Quit, &keys, true), None);
    }
}
