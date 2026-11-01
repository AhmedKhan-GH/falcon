//! The menu bar: what each command needs to act, the menus themselves
//! (a tree of [`Entry`]), and on macOS the AppKit menu built from them.
//!
//! A key press and a menu click both go through `App::run_command`, which
//! asks [`enabled`] first, so a command does the same thing (or nothing)
//! however it was given. The menu items show the live chords from the
//! shortcut table, but keys stay with egui: each menu has a delegate that
//! tells AppKit it has no key equivalents, so a key press never fires an
//! item and egui (text fields included) gets it as before. Should AppKit
//! fire an item from a key anyway, the app hands the key back to egui
//! instead of running the item (see `Click::by_key`).

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

use crate::logic::Layout;
use crate::shortcuts::{Command, Page, Scroll, Shortcuts};

/// What kind of view is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScopeKind {
    #[default]
    Other,
    Folder,
    Smart,
    Trash,
    Random,
}

/// The app's state as far as commands care. Built each frame by
/// `App::menu_state`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct State {
    /// A library is open.
    pub library: bool,
    /// Preferences is open.
    pub modal: bool,
    /// A text field has keyboard focus.
    pub text: bool,
    pub viewer: bool,
    pub selected: usize,
    pub scope: ScopeKind,
    /// The sidebar page that is open, if one is.
    pub page: Option<Page>,
    pub back: bool,
    pub forward: bool,
    /// The open folder or smart folder has a parent.
    pub parent: bool,
    /// Folders, or smart folders, picked together in the sidebar.
    pub picked_folders: usize,
    pub picked_smart: usize,
    pub undo: bool,
    pub redo: bool,
    pub layout: Layout,
    pub sidebar: bool,
    pub inspector: bool,
    pub filter: bool,
    /// The layout panel's name, info and comment count switches.
    pub show_name: bool,
    pub show_info: bool,
    pub show_comments: bool,
    /// Show Subfolder Content is on.
    pub subfolders: bool,
    pub fullscreen: bool,
    /// Open libraries.
    pub libraries: usize,
    /// Some of the items acted on can be flipped or rotated; the one item
    /// acted on can be cropped.
    pub flippable: bool,
    pub rotatable: bool,
    pub croppable: bool,
    /// The book in `docs/` is on this machine.
    pub docs: bool,
    /// An app password is set, so falcon can be locked.
    pub lockable: bool,
    /// Tags have been copied (Copy Tags), so Paste Tags can act.
    pub tag_clipboard: bool,
    /// Presentation mode is on.
    pub presenting: bool,
    /// The main window floats above other apps' windows.
    pub on_top: bool,
    /// The grayscale preview is on.
    pub grayscale: bool,
    /// The image viewer's navigator shows when zoomed in.
    pub navigator: bool,
}

impl State {
    /// A library is open and the grid shows (no viewer over it).
    fn grid(&self) -> bool {
        self.library && !self.viewer
    }
}

/// Whether `cmd` can act in state `s`.
pub fn enabled(cmd: Command, s: &State) -> bool {
    use Command as C;
    if s.modal {
        return false;
    }
    let grid = s.grid();
    let any = s.library && s.selected > 0;
    match cmd {
        C::Viewer => s.library && (s.viewer || s.selected == 1),
        // The clipboard commands and undo also serve a focused text field.
        C::SelectAll => s.text || s.library,
        C::Copy => s.text || (grid && s.selected > 0),
        C::Paste => s.text || grid,
        C::Cut => s.text,
        C::Undo => s.text || (s.library && s.undo),
        C::Redo => s.text || (s.library && s.redo),
        C::Trash => any,
        C::RemoveFromFolder => any && s.scope == ScopeKind::Folder,
        C::OpenDefault | C::RevealInFinder | C::RefreshThumbnail | C::Duplicate => any || (s.library && s.viewer),
        C::Rename => grid && s.selected == 1,
        C::Back => s.viewer || (s.library && s.back),
        C::Forward => grid && s.forward,
        C::Find | C::ToggleFilter | C::NewFolder | C::NewSmartFolder => grid,
        C::NewSubfolder => grid && s.scope == ScopeKind::Folder,
        C::NewTextFile => s.library,
        // The panels can be toggled anywhere, the viewer included.
        C::ToggleSidebar | C::ToggleInspector | C::ToggleAll => true,
        // In the viewer they zoom the image, else the grid.
        C::ZoomIn | C::ZoomOut => s.library,
        C::ZoomActual | C::ZoomFit => s.library && s.viewer,
        C::FolderSettings => grid && s.scope == ScopeKind::Folder,
        C::ExportComputer | C::ExportPack | C::ExportFormat => any,
        C::CopyPath | C::CopyFolderPath | C::CopyName | C::CopyLink | C::CopyThumbnail | C::CopyBase64 => any,
        // The selection, or else every item shown.
        C::ExportCsv => s.library,
        C::LayoutGrid | C::LayoutJustified | C::LayoutWaterfall | C::LayoutList => grid,
        // As in Eagle, not while several folders are picked.
        C::FolderUp | C::FolderDown | C::FolderTop | C::FolderBottom => {
            grid && matches!(s.scope, ScopeKind::Folder | ScopeKind::Smart) && s.picked_folders < 2 && s.picked_smart < 2
        }
        C::MoveFolder => grid && (s.scope == ScopeKind::Folder || s.picked_folders > 0),
        C::LockApp => s.lockable,
        C::SwitchLibrary | C::CollectWindow | C::Preferences | C::CloseWindow | C::Minimize | C::FullScreen | C::WindowZoom => true,
        C::NextLibrary | C::PreviousLibrary => s.libraries > 1,
        C::Page(_) | C::Import | C::ImportFolders | C::FindDuplicates => s.library,
        C::Scroll(_) | C::SelectFirst | C::SelectLast | C::ToggleFolders => grid,
        C::ParentFolder => grid && s.parent,
        C::Reshuffle => grid && s.scope == ScopeKind::Random,
        C::Rate(_) => any || (s.library && s.viewer),
        C::AddTags | C::TagPanel | C::CopyTags | C::ClearTags => any,
        C::PasteTags => any && s.tag_clipboard,
        C::FlipHorizontal => s.library && s.flippable,
        C::RotateLeft => s.library && s.rotatable,
        C::Crop => s.library && s.croppable,
        C::Documentation => s.docs,
        C::ToggleName | C::ToggleInfo | C::ToggleComments | C::ToggleSubfolders => s.library,
        C::AlwaysOnTop => true,
        C::Grayscale | C::Navigator => s.library,
        C::OpenNewWindow => s.library && (s.viewer || s.selected > 0),
        C::Presentation => s.library && (s.viewer || s.selected > 0 || s.presenting),
        C::Player(_) => false,
    }
}

/// What puts a checkmark on an item.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Check {
    None,
    Layout(Layout),
    Page(Page),
    Sidebar,
    Inspector,
    Filter,
    Viewer,
    FullScreen,
    Name,
    Info,
    Comments,
    Subfolders,
    Presentation,
    AlwaysOnTop,
    Grayscale,
    Navigator,
}

impl Check {
    pub fn on(self, s: &State) -> bool {
        match self {
            Check::None => false,
            Check::Layout(l) => s.library && s.layout == l,
            Check::Page(p) => s.library && s.page == Some(p),
            Check::Sidebar => s.sidebar,
            Check::Inspector => s.inspector,
            Check::Filter => s.filter,
            Check::Viewer => s.viewer,
            Check::FullScreen => s.fullscreen,
            Check::Name => s.show_name,
            Check::Info => s.show_info,
            Check::Comments => s.show_comments,
            Check::Subfolders => s.subfolders,
            Check::Presentation => s.presenting,
            Check::AlwaysOnTop => s.on_top,
            Check::Grayscale => s.grayscale,
            Check::Navigator => s.navigator,
        }
    }
}

/// One menu item.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub label: &'static str,
    pub cmd: Command,
    pub check: Check,
}

/// An entry of a menu.
pub enum Entry {
    Item(Item),
    Sub(&'static str, &'static [Entry]),
    Separator,
}

const fn item(label: &'static str, cmd: Command) -> Entry {
    Entry::Item(Item { label, cmd, check: Check::None })
}

const fn checked(label: &'static str, cmd: Command, check: Check) -> Entry {
    Entry::Item(Item { label, cmd, check })
}

const SEP: Entry = Entry::Separator;

/// Added to the application menu, after About.
pub static APP: &[Entry] = &[item("Preferences…", Command::Preferences), item("Lock Falcon", Command::LockApp)];

/// The menus after the application menu, after Eagle's (without its
/// Library and Actions menus), with only what falcon can do.
pub static MENUS: &[(&str, &[Entry])] = &[
    (
        "File",
        &[
            Entry::Sub("New", &[item("Text File", Command::NewTextFile)]),
            SEP,
            item("New Folder", Command::NewFolder),
            item("New Subfolder", Command::NewSubfolder),
            item("New Smart Folder…", Command::NewSmartFolder),
            SEP,
            item("Import Folders…", Command::ImportFolders),
            item("Import Files…", Command::Import),
            Entry::Sub(
                "Export",
                &[
                    item("To Computer…", Command::ExportComputer),
                    item("As .eaglepack…", Command::ExportPack),
                    item("In Another Format…", Command::ExportFormat),
                    item("To CSV…", Command::ExportCsv),
                ],
            ),
            SEP,
            item("Open Library…", Command::SwitchLibrary),
            SEP,
            item("Close Window", Command::CloseWindow),
        ],
    ),
    (
        "Edit",
        &[
            item("Undo", Command::Undo),
            item("Redo", Command::Redo),
            SEP,
            item("Cut", Command::Cut),
            item("Copy", Command::Copy),
            item("Paste", Command::Paste),
            item("Select All", Command::SelectAll),
            SEP,
            item("Rename", Command::Rename),
            item("Duplicate", Command::Duplicate),
            Entry::Sub(
                "Copy Details",
                &[
                    item("File Paths", Command::CopyPath),
                    item("Folder Paths", Command::CopyFolderPath),
                    item("Names", Command::CopyName),
                    item("Source URLs", Command::CopyLink),
                    item("Thumbnail", Command::CopyThumbnail),
                    item("Base64", Command::CopyBase64),
                ],
            ),
            item("Refresh Thumbnail", Command::RefreshThumbnail),
            SEP,
            Entry::Sub(
                "Move Folder",
                &[
                    item("Up", Command::FolderUp),
                    item("Down", Command::FolderDown),
                    SEP,
                    item("To the Top", Command::FolderTop),
                    item("To the Bottom", Command::FolderBottom),
                ],
            ),
            Entry::Sub(
                "Edit Image",
                &[item("Flip Horizontally", Command::FlipHorizontal), item("Rotate Left", Command::RotateLeft), item("Crop…", Command::Crop)],
            ),
            item("Folder Settings…", Command::FolderSettings),
            item("Move Folder…", Command::MoveFolder),
            SEP,
            item("Remove from Folder", Command::RemoveFromFolder),
            item("Move to Trash", Command::Trash),
        ],
    ),
    (
        "Find",
        &[
            item("Search", Command::Find),
            checked("Filter Bar", Command::ToggleFilter, Check::Filter),
            SEP,
            item("Find Duplicates", Command::FindDuplicates),
        ],
    ),
    (
        "Organize",
        &[
            item("Add Tags…", Command::AddTags),
            SEP,
            item("No Rating", Command::Rate(0)),
            item("1 Star", Command::Rate(1)),
            item("2 Stars", Command::Rate(2)),
            item("3 Stars", Command::Rate(3)),
            item("4 Stars", Command::Rate(4)),
            item("5 Stars", Command::Rate(5)),
            SEP,
            item("Copy Tags", Command::CopyTags),
            item("Paste Tags", Command::PasteTags),
            item("Clear Tags", Command::ClearTags),
        ],
    ),
    (
        "View",
        &[
            checked("Always on Top", Command::AlwaysOnTop, Check::AlwaysOnTop),
            SEP,
            checked("All Items", Command::Page(Page::All), Check::Page(Page::All)),
            checked("Uncategorized", Command::Page(Page::Uncategorized), Check::Page(Page::Uncategorized)),
            checked("Untagged", Command::Page(Page::Untagged), Check::Page(Page::Untagged)),
            checked("Recently Used", Command::Page(Page::Recent), Check::Page(Page::Recent)),
            checked("Random", Command::Page(Page::Random), Check::Page(Page::Random)),
            checked("All Tags", Command::Page(Page::AllTags), Check::Page(Page::AllTags)),
            checked("Trash", Command::Page(Page::Trash), Check::Page(Page::Trash)),
            SEP,
            checked("Waterfall", Command::LayoutWaterfall, Check::Layout(Layout::Waterfall)),
            checked("Justified", Command::LayoutJustified, Check::Layout(Layout::Justified)),
            checked("Grid", Command::LayoutGrid, Check::Layout(Layout::Grid)),
            checked("List", Command::LayoutList, Check::Layout(Layout::List)),
            SEP,
            checked("Show Name", Command::ToggleName, Check::Name),
            checked("Show Item Info", Command::ToggleInfo, Check::Info),
            checked("Show Comment Count", Command::ToggleComments, Check::Comments),
            checked("Show Subfolder Content", Command::ToggleSubfolders, Check::Subfolders),
            SEP,
            item("Scroll to Top", Command::Scroll(Scroll::Top)),
            item("Scroll to Bottom", Command::Scroll(Scroll::Bottom)),
            item("Page Up", Command::Scroll(Scroll::PageUp)),
            item("Page Down", Command::Scroll(Scroll::PageDown)),
            item("First Item", Command::SelectFirst),
            item("Last Item", Command::SelectLast),
            SEP,
            item("Zoom In", Command::ZoomIn),
            item("Zoom Out", Command::ZoomOut),
            item("Actual Size", Command::ZoomActual),
            item("Zoom to Fit", Command::ZoomFit),
            checked("Grayscale", Command::Grayscale, Check::Grayscale),
            checked("Navigator", Command::Navigator, Check::Navigator),
            SEP,
            checked("Viewer", Command::Viewer, Check::Viewer),
            item("Open in New Window", Command::OpenNewWindow),
            item("Open With Default App", Command::OpenDefault),
            item("Reveal in Finder", Command::RevealInFinder),
            SEP,
            checked("Sidebar", Command::ToggleSidebar, Check::Sidebar),
            checked("Inspector", Command::ToggleInspector, Check::Inspector),
            item("Show or Hide Both Panels", Command::ToggleAll),
            checked("Presentation Mode", Command::Presentation, Check::Presentation),
            SEP,
            item("Back", Command::Back),
            item("Forward", Command::Forward),
            item("Parent Folder", Command::ParentFolder),
            item("Shuffle Again", Command::Reshuffle),
            item("Expand or Collapse All Folders", Command::ToggleFolders),
        ],
    ),
    (
        "Window",
        &[
            item("Minimize", Command::Minimize),
            item("Zoom", Command::WindowZoom),
            checked("Full Screen", Command::FullScreen, Check::FullScreen),
            SEP,
            item("Collect Window", Command::CollectWindow),
            item("Next Library", Command::NextLibrary),
            item("Previous Library", Command::PreviousLibrary),
            SEP,
            item("Close Window", Command::CloseWindow),
        ],
    ),
    ("Help", &[item("Falcon Documentation", Command::Documentation)]),
];

/// Every item, the application menu's first, in menu order. An item's
/// index here (found by address, which is why the menus are statics)
/// gives its tag in the AppKit menu.
pub fn items() -> &'static [&'static Item] {
    fn walk(entries: &'static [Entry], out: &mut Vec<&'static Item>) {
        for e in entries {
            match e {
                Entry::Item(i) => out.push(i),
                Entry::Sub(_, sub) => walk(sub, out),
                Entry::Separator => {}
            }
        }
    }
    static ITEMS: std::sync::OnceLock<Vec<&'static Item>> = std::sync::OnceLock::new();
    ITEMS.get_or_init(|| {
        let mut out = Vec::new();
        walk(APP, &mut out);
        for (_, entries) in MENUS {
            walk(entries, &mut out);
        }
        out
    })
}

/// The chord an item shows: the live binding, or the fixed keys of the
/// commands egui handles itself.
pub fn chord(cmd: Command, keys: &Shortcuts) -> Option<KeyboardShortcut> {
    let cmd_shift = Modifiers { shift: true, ..Modifiers::COMMAND };
    match cmd {
        Command::Undo => Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)),
        Command::Redo => Some(KeyboardShortcut::new(cmd_shift, Key::Z)),
        Command::Cut => Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::X)),
        _ => keys.binding(cmd),
    }
}

/// A chord as an `NSMenuItem` key equivalent: the key's character
/// (lowercase for letters, AppKit's function-key characters for arrows
/// and the like) and the modifier keys.
#[derive(Clone, PartialEq, Debug)]
pub struct KeyEquivalent {
    pub key: String,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// The key equivalent for `c`, or `None` for a key a menu can't show.
pub fn key_equivalent(c: &KeyboardShortcut) -> Option<KeyEquivalent> {
    let name = c.logical_key.name();
    let key: String = match c.logical_key {
        Key::Enter => "\r".into(),
        Key::Tab => "\t".into(),
        Key::Space => " ".into(),
        Key::Backspace => "\u{8}".into(),
        Key::Delete => "\u{7f}".into(),
        Key::Escape => "\u{1b}".into(),
        Key::ArrowUp => "\u{f700}".into(),
        Key::ArrowDown => "\u{f701}".into(),
        Key::ArrowLeft => "\u{f702}".into(),
        Key::ArrowRight => "\u{f703}".into(),
        Key::Home => "\u{f729}".into(),
        Key::End => "\u{f72b}".into(),
        Key::PageUp => "\u{f72c}".into(),
        Key::PageDown => "\u{f72d}".into(),
        Key::Equals => "=".into(),
        Key::Plus => "+".into(),
        Key::Minus => "-".into(),
        Key::OpenBracket => "[".into(),
        Key::CloseBracket => "]".into(),
        Key::OpenCurlyBracket => "{".into(),
        Key::CloseCurlyBracket => "}".into(),
        Key::Comma => ",".into(),
        Key::Period => ".".into(),
        Key::Slash => "/".into(),
        Key::Backslash => "\\".into(),
        Key::Semicolon => ";".into(),
        Key::Quote => "'".into(),
        Key::Backtick => "`".into(),
        // F1 is U+F704, and so on.
        _ if name.len() > 1 && name.starts_with('F') && name[1..].parse::<u32>().is_ok_and(|n| (1..=35).contains(&n)) => {
            char::from_u32(0xf703 + name[1..].parse::<u32>().ok()?)?.to_string()
        }
        // Letters and digits.
        _ if name.len() == 1 && name.chars().all(|ch| ch.is_ascii_alphanumeric()) => name.to_ascii_lowercase(),
        _ => return None,
    };
    let m = c.modifiers;
    Some(KeyEquivalent { key, command: m.command || m.mac_cmd, shift: m.shift, alt: m.alt, ctrl: m.ctrl })
}

/// A menu item chosen. `by_key` means AppKit matched a key press to the
/// item despite the delegate; the key then goes back to egui.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Click {
    pub cmd: Command,
    pub by_key: bool,
}

/// The menu bar as last built and refreshed.
#[derive(Default)]
pub struct Bar {
    built: Option<Shortcuts>,
    state: Option<State>,
}

impl Bar {
    /// Builds the menus on the first call and again when the bindings
    /// change, and hands the state to the items' validation when it
    /// changes. Call on the main thread.
    pub fn sync(&mut self, ctx: &eframe::egui::Context, keys: &Shortcuts, state: &State) {
        if self.built.as_ref() != Some(keys) {
            mac::build(ctx, keys);
            self.built = Some(keys.clone());
        }
        if self.state.as_ref() != Some(state) {
            mac::set_state(state.clone());
            self.state = Some(state.clone());
        }
    }
}

/// The items chosen since the last call, oldest first.
pub fn take_clicks() -> Vec<Click> {
    mac::take_clicks()
}

#[cfg(not(target_os = "macos"))]
mod mac {
    use super::{Click, State};
    use crate::shortcuts::Shortcuts;

    pub fn build(_ctx: &eframe::egui::Context, _keys: &Shortcuts) {}
    pub fn set_state(_state: State) {}
    pub fn take_clicks() -> Vec<Click> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::{Mutex, OnceLock};

    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuDelegate, NSMenuItem,
        NSMenuItemValidation,
    };
    use objc2_foundation::{NSString, NSUserDefaults};

    use super::{Click, Entry, State, items, key_equivalent};
    use crate::shortcuts::Shortcuts;

    /// Item tags are this plus the item's index in `items()`.
    const TAG_BASE: isize = 1000;
    /// The separator falcon puts before Preferences in the app menu.
    const TAG_SEPARATOR: isize = 999;

    static CLICKS: Mutex<Vec<Click>> = Mutex::new(Vec::new());
    static STATE: Mutex<Option<State>> = Mutex::new(None);
    static REPAINT: OnceLock<eframe::egui::Context> = OnceLock::new();

    thread_local! {
        static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) };
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and Target has
        // no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FalconMenuTarget"]
        pub(super) struct Target;

        impl Target {
            #[unsafe(method(falconMenuItem:))]
            fn clicked(&self, item: &NSMenuItem) {
                clicked(self.mtm(), item);
            }

            /// NSMenuDelegate: no key equivalents, so key presses reach
            /// the window (and egui) instead of the menu.
            #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
            fn has_key_equivalent(&self, _menu: &NSMenu, _event: &NSEvent, _target: *mut c_void, _action: *mut c_void) -> bool {
                false
            }
        }

        unsafe impl NSObjectProtocol for Target {}

        unsafe impl NSMenuDelegate for Target {}

        unsafe impl NSMenuItemValidation for Target {
            #[unsafe(method(validateMenuItem:))]
            fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
                validate(item)
            }
        }
    );

    /// The model item behind a menu item, by its tag.
    fn model(item: &NSMenuItem) -> Option<&'static super::Item> {
        usize::try_from(item.tag() - TAG_BASE).ok().and_then(|i| items().get(i).copied())
    }

    fn clicked(mtm: MainThreadMarker, item: &NSMenuItem) {
        let Some(it) = model(item) else { return };
        let by_key = NSApplication::sharedApplication(mtm).currentEvent().is_some_and(|e| e.r#type() == NSEventType::KeyDown);
        CLICKS.lock().unwrap_or_else(|p| p.into_inner()).push(Click { cmd: it.cmd, by_key });
        if let Some(ctx) = REPAINT.get() {
            ctx.request_repaint();
        }
    }

    /// Enables the item and sets its checkmark from the last state.
    fn validate(item: &NSMenuItem) -> bool {
        let Some(it) = model(item) else { return true };
        let state = STATE.lock().unwrap_or_else(|p| p.into_inner()).clone().unwrap_or_default();
        item.setState(if it.check.on(&state) { NSControlStateValueOn } else { NSControlStateValueOff });
        super::enabled(it.cmd, &state)
    }

    impl Target {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn set_state(state: State) {
        *STATE.lock().unwrap_or_else(|p| p.into_inner()) = Some(state);
    }

    pub fn take_clicks() -> Vec<Click> {
        std::mem::take(&mut *CLICKS.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Replaces every menu after the application menu (which winit made)
    /// and puts Preferences into it.
    pub fn build(ctx: &eframe::egui::Context, keys: &Shortcuts) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let _ = REPAINT.set(ctx.clone());
        let target = TARGET.with(|t| t.borrow_mut().get_or_insert_with(|| Target::new(mtm)).clone());
        // AppKit would add its own Enter Full Screen item next to ours.
        NSUserDefaults::standardUserDefaults().setBool_forKey(false, &NSString::from_str("NSFullScreenMenuItemEverywhere"));
        let app = NSApplication::sharedApplication(mtm);
        let main = match app.mainMenu() {
            Some(m) => m,
            None => {
                let m = NSMenu::new(mtm);
                let app_item = NSMenuItem::new(mtm);
                app_item.setSubmenu(Some(&NSMenu::new(mtm)));
                m.addItem(&app_item);
                app.setMainMenu(Some(&m));
                m
            }
        };
        while main.numberOfItems() > 1 {
            main.removeItemAtIndex(main.numberOfItems() - 1);
        }
        let all = items();
        if let Some(app_menu) = main.itemAtIndex(0).and_then(|i| i.submenu()) {
            for i in (0..app_menu.numberOfItems()).rev() {
                if app_menu.itemAtIndex(i).is_some_and(|it| it.tag() >= TAG_SEPARATOR) {
                    app_menu.removeItemAtIndex(i);
                }
            }
            // After About.
            let mut at = app_menu.numberOfItems().min(1);
            let sep = NSMenuItem::separatorItem(mtm);
            sep.setTag(TAG_SEPARATOR);
            app_menu.insertItem_atIndex(&sep, at);
            for e in super::APP {
                if let Entry::Item(it) = e {
                    at += 1;
                    app_menu.insertItem_atIndex(&menu_item(mtm, &target, keys, all, it), at);
                }
            }
        }
        for (title, entries) in super::MENUS {
            let menu = submenu(mtm, &target, keys, all, title, entries);
            let top = NSMenuItem::new(mtm);
            top.setTitle(&NSString::from_str(title));
            top.setSubmenu(Some(&menu));
            main.addItem(&top);
            match *title {
                "Window" => app.setWindowsMenu(Some(&menu)),
                "Help" => app.setHelpMenu(Some(&menu)),
                _ => {}
            }
        }
    }

    fn submenu(mtm: MainThreadMarker, target: &Target, keys: &Shortcuts, all: &[&super::Item], title: &str, entries: &'static [Entry]) -> Retained<NSMenu> {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        menu.setDelegate(Some(ProtocolObject::from_ref(target)));
        for e in entries {
            match e {
                Entry::Item(it) => menu.addItem(&menu_item(mtm, target, keys, all, it)),
                Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                Entry::Sub(t, sub) => {
                    let item = NSMenuItem::new(mtm);
                    item.setTitle(&NSString::from_str(t));
                    item.setSubmenu(Some(&submenu(mtm, target, keys, all, t, sub)));
                    menu.addItem(&item);
                }
            }
        }
        menu
    }

    fn menu_item(mtm: MainThreadMarker, target: &Target, keys: &Shortcuts, all: &[&super::Item], it: &super::Item) -> Retained<NSMenuItem> {
        let index = all.iter().position(|x| std::ptr::eq(*x, it)).unwrap_or(0);
        let ke = super::chord(it.cmd, keys).as_ref().and_then(key_equivalent);
        let key = NSString::from_str(ke.as_ref().map_or("", |k| k.key.as_str()));
        // SAFETY: the action is a method of `target`, which takes the item.
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(it.label), Some(sel!(falconMenuItem:)), &key) };
        if let Some(k) = ke {
            let mut mask = NSEventModifierFlags::empty();
            for (on, flag) in [(k.command, NSEventModifierFlags::Command), (k.shift, NSEventModifierFlags::Shift), (k.alt, NSEventModifierFlags::Option), (k.ctrl, NSEventModifierFlags::Control)] {
                if on {
                    mask |= flag;
                }
            }
            item.setKeyEquivalentModifierMask(mask);
        }
        // SAFETY: `target` lives in TARGET for the rest of the process.
        unsafe { item.setTarget(Some(target)) };
        item.setTag(TAG_BASE + index as isize);
        item
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(selected: usize) -> State {
        State { library: true, selected, libraries: 1, ..Default::default() }
    }

    #[test]
    fn nothing_acts_behind_preferences() {
        let s = State { modal: true, ..grid(3) };
        assert!(!enabled(Command::Trash, &s));
        assert!(!enabled(Command::Preferences, &s));
    }

    #[test]
    fn selection_rules() {
        assert!(!enabled(Command::Rename, &grid(0)));
        assert!(enabled(Command::Rename, &grid(1)));
        assert!(!enabled(Command::Rename, &grid(2)));
        assert!(!enabled(Command::Copy, &grid(0)));
        assert!(enabled(Command::Trash, &grid(2)));
        assert!(enabled(Command::Viewer, &grid(1)));
        assert!(!enabled(Command::Viewer, &grid(2)));
    }

    #[test]
    fn the_viewer_blocks_grid_commands() {
        let s = State { viewer: true, ..grid(1) };
        assert!(!enabled(Command::Rename, &s));
        assert!(enabled(Command::ZoomIn, &s), "zooms the image");
        assert!(enabled(Command::ZoomFit, &s));
        assert!(!enabled(Command::ZoomFit, &grid(1)));
        assert!(enabled(Command::ToggleAll, &s));
        assert!(enabled(Command::Back, &s), "Back leaves the viewer");
        assert!(enabled(Command::Viewer, &s));
        assert!(enabled(Command::RevealInFinder, &s));
    }

    #[test]
    fn folder_moves_need_a_folder() {
        assert!(!enabled(Command::FolderUp, &grid(0)));
        assert!(enabled(Command::FolderUp, &State { scope: ScopeKind::Folder, ..grid(0) }));
        assert!(enabled(Command::FolderTop, &State { scope: ScopeKind::Smart, ..grid(0) }));
        // Several picked folders can't be stepped, but can be moved.
        let picked = State { scope: ScopeKind::Folder, picked_folders: 3, ..grid(0) };
        assert!(!enabled(Command::FolderUp, &picked));
        assert!(!enabled(Command::FolderBottom, &State { scope: ScopeKind::Smart, picked_smart: 2, ..grid(0) }));
        assert!(enabled(Command::MoveFolder, &picked));
        assert!(enabled(Command::MoveFolder, &State { scope: ScopeKind::Folder, ..grid(0) }));
        assert!(!enabled(Command::MoveFolder, &State { scope: ScopeKind::Smart, ..grid(0) }));
    }

    #[test]
    fn menus_follow_eagles_order() {
        let titles: Vec<&str> = MENUS.iter().map(|m| m.0).collect();
        assert_eq!(titles, ["File", "Edit", "Find", "Organize", "View", "Window", "Help"]);
        let keys = Shortcuts::default();
        // Always on Top comes first, then the View pages, ⌘1 to ⌘7.
        let view = MENUS.iter().find(|m| m.0 == "View").unwrap().1;
        assert!(matches!(&view[0], Entry::Item(i) if i.cmd == Command::AlwaysOnTop));
        let pages: Vec<String> = view.iter().skip(2).take(7).filter_map(|e| if let Entry::Item(i) = e { chord(i.cmd, &keys).map(|c| crate::shortcuts::format(&c)) } else { None }).collect();
        assert_eq!(pages, (1..=7).map(|n| format!("Command + {n}")).collect::<Vec<_>>());
    }

    #[test]
    fn every_item_has_its_own_tag() {
        let all = items();
        for (n, it) in all.iter().enumerate() {
            assert_eq!(all.iter().position(|x| std::ptr::eq(*x, *it)), Some(n), "{}", it.label);
            assert!(!matches!(it.cmd, Command::Player(_)), "{}", it.label);
        }
        // Labels are unique within a menu (Close Window is in File and Window).
        for (title, entries) in MENUS {
            let mut seen = std::collections::HashSet::new();
            for e in entries.iter() {
                if let Entry::Item(i) = e {
                    assert!(seen.insert(i.label), "{title} repeats {}", i.label);
                }
            }
        }
    }

    #[test]
    fn every_item_can_act_somewhere() {
        // A state where nearly everything applies: a folder with a parent,
        // two selected items, history both ways, two libraries.
        let full = State {
            library: true,
            selected: 1,
            scope: ScopeKind::Folder,
            back: true,
            forward: true,
            parent: true,
            undo: true,
            redo: true,
            libraries: 2,
            flippable: true,
            rotatable: true,
            croppable: true,
            docs: true,
            lockable: true,
            tag_clipboard: true,
            ..Default::default()
        };
        assert!(!enabled(Command::PasteTags, &State { tag_clipboard: false, ..full.clone() }), "Paste Tags needs copied tags");
        assert!(!enabled(Command::LockApp, &State::default()), "Lock needs a password");
        for it in items().iter() {
            let s = match it.cmd {
                Command::Reshuffle => State { scope: ScopeKind::Random, ..full.clone() },
                Command::Cut => State { text: true, ..full.clone() },
                Command::ZoomActual | Command::ZoomFit => State { viewer: true, ..full.clone() },
                _ => full.clone(),
            };
            assert!(enabled(it.cmd, &s), "{} never acts", it.label);
        }
    }

    #[test]
    fn items_show_live_chords() {
        let mut keys = Shortcuts::default();
        let rename = |k: &Shortcuts| chord(Command::Rename, k).and_then(|c| key_equivalent(&c));
        assert_eq!(rename(&keys), Some(KeyEquivalent { key: "r".into(), command: true, shift: false, alt: false, ctrl: false }));
        keys.set(Command::Rename, crate::shortcuts::parse("Command + Shift + Y")).unwrap();
        assert_eq!(rename(&keys).map(|k| (k.key, k.shift)), Some(("y".into(), true)));
        keys.set(Command::Rename, None).unwrap();
        assert_eq!(rename(&keys), None);
        // Undo, Redo and Cut are egui's own keys.
        assert_eq!(chord(Command::Redo, &keys).map(|c| crate::shortcuts::format(&c)).as_deref(), Some("Command + Shift + Z"));
        assert_eq!(chord(Command::Import, &keys), None);
    }

    #[test]
    fn key_equivalents() {
        let ke = |text: &str| key_equivalent(&crate::shortcuts::parse(text).unwrap()).unwrap();
        assert_eq!(ke("Command + Backspace").key, "\u{8}");
        assert_eq!(ke("Command + Shift + Backspace").key, "\u{8}");
        assert_eq!(ke("Alt + Up").key, "\u{f700}");
        assert_eq!(ke("Command + Down").key, "\u{f701}");
        assert_eq!(ke("Home").key, "\u{f729}");
        assert_eq!(ke("PageDown").key, "\u{f72d}");
        assert_eq!(ke("F2").key, "\u{f705}");
        assert_eq!(ke("Shift + Enter").key, "\r");
        assert_eq!(ke("Space").key, " ");
        assert_eq!(ke("Command + =").key, "=");
        assert_eq!(ke("Command + ]").key, "]");
        assert_eq!(ke("/").key, "/");
        assert_eq!(ke("3").key, "3");
        let full = ke("Ctrl + Command + F");
        assert_eq!((full.key.as_str(), full.command, full.ctrl, full.shift, full.alt), ("f", true, true, false, false));
        let n = ke("Command + Shift + Alt + N");
        assert_eq!((n.key.as_str(), n.command, n.shift, n.alt), ("n", true, true, true));
        // macOS reports ⌘ as both flags.
        let mac = KeyboardShortcut::new(Modifiers { mac_cmd: true, command: true, ..Default::default() }, Key::W);
        assert!(key_equivalent(&mac).unwrap().command);
    }

    #[test]
    fn checkmarks() {
        let s = State { library: true, layout: Layout::Grid, page: Some(Page::Trash), sidebar: true, ..Default::default() };
        let on: Vec<&str> = items().iter().filter(|i| i.check.on(&s)).map(|i| i.label).collect();
        assert_eq!(on, ["Trash", "Grid", "Sidebar"]);
    }

    /// Registers the AppKit target class, which checks its method
    /// signatures (the menus themselves need the main thread).
    #[cfg(target_os = "macos")]
    #[test]
    fn target_class_registers() {
        let class = <mac::Target as objc2::ClassType>::class();
        for sel in [objc2::sel!(falconMenuItem:), objc2::sel!(validateMenuItem:), objc2::sel!(menuHasKeyEquivalent:forEvent:target:action:)] {
            assert!(class.instance_method(sel).is_some(), "{sel:?}");
        }
    }

    #[test]
    fn presentation_needs_an_item() {
        assert!(!enabled(Command::Presentation, &grid(0)));
        assert!(enabled(Command::Presentation, &grid(1)));
        assert!(enabled(Command::Presentation, &State { library: true, viewer: true, ..Default::default() }));
        let on = State { library: true, presenting: true, ..Default::default() };
        assert!(enabled(Command::Presentation, &on));
        assert!(Check::Presentation.on(&on));
    }

    #[test]
    fn no_library() {
        let s = State::default();
        assert!(enabled(Command::SwitchLibrary, &s));
        assert!(!enabled(Command::NewFolder, &s));
        assert!(!enabled(Command::NextLibrary, &s));
    }

    #[test]
    fn caption_switches_carry_checkmarks() {
        let s = State { show_name: true, show_info: false, ..grid(0) };
        assert!(Check::Name.on(&s));
        assert!(!Check::Info.on(&s));
        assert!(!Check::Comments.on(&s));
        assert!(enabled(Command::ToggleInfo, &s));
        assert!(!enabled(Command::ToggleInfo, &State::default()), "needs a library");
    }
}
