//! Rebindable keyboard shortcuts, after Eagle's `shortcuts.keybinds`
//! preference: a table of commands with default chords, user overrides
//! stored as Eagle-style accelerator strings ("Command + Shift + N"),
//! conflict checks and Reset. The Player group's keys act only while a
//! video plays in the viewer, so, as in Eagle, they conflict only with
//! each other.

use std::collections::BTreeMap;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

/// What a shortcut does. The app dispatches these in `App::keyboard`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Command {
    Viewer,
    SelectAll,
    Trash,
    Rename,
    Find,
    Back,
    Forward,
    ZoomIn,
    ZoomOut,
    /// The viewer's actual size and zoom to fit.
    ZoomActual,
    ZoomFit,
    NewFolder,
    Copy,
    Paste,
    ToggleSidebar,
    ToggleInspector,
    /// Eagle's View ▸ Sidebar / Inspector: both panels at once.
    ToggleAll,
    ToggleFilter,
    LayoutGrid,
    LayoutJustified,
    LayoutWaterfall,
    LayoutList,
    OpenDefault,
    RevealInFinder,
    Preferences,
    CollectWindow,
    SwitchLibrary,
    NextLibrary,
    PreviousLibrary,
    /// Edit ▸ Move Folder: the open folder or smart folder among its
    /// siblings.
    FolderUp,
    FolderDown,
    FolderTop,
    FolderBottom,
    /// Eagle's Set Current Folder: the open folder's name and auto-tags.
    FolderSettings,
    /// File ▸ Export (`App::export_keys`).
    ExportComputer,
    ExportPack,
    ExportCsv,
    ExportFormat,
    /// Edit ▸ Copy variants (`App::export_keys`).
    CopyPath,
    CopyFolderPath,
    CopyName,
    CopyLink,
    CopyThumbnail,
    CopyBase64,
    /// A key of the video player (`App::video_keys` reads these).
    Player(PlayerKey),
    /// Show a sidebar page (Eagle's View ▸ All … Trash, ⌘1–⌘7).
    Page(Page),
    /// Scroll the grid (Home, End, Page Up, Page Down).
    Scroll(Scroll),
    /// Select the first or last item (⌘↑, ⌘↓).
    SelectFirst,
    SelectLast,
    /// Set the rating of the selection (0 removes it).
    Rate(u8),
}

/// How a scroll command moves the grid.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Scroll {
    Top,
    Bottom,
    PageUp,
    PageDown,
}

impl Scroll {
    /// The new offset from the current one, for a viewport `view` high
    /// over content `content` high. A page is the viewport less a little
    /// overlap, as in Eagle (its window height less 72 px).
    pub fn offset(self, current: f32, view: f32, content: f32) -> f32 {
        let page = (view - 72.0).max(view / 2.0);
        let to = match self {
            Scroll::Top => 0.0,
            Scroll::Bottom => content,
            Scroll::PageUp => current - page,
            Scroll::PageDown => current + page,
        };
        to.clamp(0.0, (content - view).max(0.0))
    }
}

/// The sidebar pages ⌘1–⌘7 open, in the order of Eagle's View menu.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Page {
    All,
    Uncategorized,
    Untagged,
    Recent,
    Random,
    AllTags,
    Trash,
}

impl Page {
    pub fn scope(self) -> crate::logic::Scope {
        use crate::logic::Scope;
        match self {
            Page::All => Scope::All,
            Page::Uncategorized => Scope::Uncategorized,
            Page::Untagged => Scope::Untagged,
            Page::Recent => Scope::Recent,
            Page::Random => Scope::Random(0),
            Page::AllTags => Scope::AllTags,
            Page::Trash => Scope::Trash,
        }
    }
}

/// What a video player key does (Eagle's `player.*` keybinds).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PlayerKey {
    Toggle,
    /// Frames to step (negative: back).
    Frames(i32),
    /// Skip back (false) or forward (true) 2% of the clip.
    Skip(bool),
    Volume(bool),
    Speed(bool),
    AddComment,
}

/// The group whose keys only act in the video player.
pub const PLAYER: &str = "Player";

/// Whether two rows can conflict: Eagle checks a chord only against the
/// rows of the same conflict group (`player` or `all`).
fn same_scope(a: &Spec, b: &Spec) -> bool {
    (a.group == PLAYER) == (b.group == PLAYER)
}

/// One row of the shortcut table.
pub struct Spec {
    pub cmd: Command,
    /// Stable key for the saved overrides; Eagle's keybind name where it
    /// has one.
    pub id: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub default: Option<KeyboardShortcut>,
    /// More chords that also work while the binding is the default
    /// (Enter for the viewer, ⌘+ for zoom in, ⌘← and ⌥← for Back).
    pub aliases: &'static [KeyboardShortcut],
}

const fn k(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const NONE: Modifiers = Modifiers::NONE;
const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };
const CMD_ALT: Modifiers = Modifiers { alt: true, ..Modifiers::COMMAND };
const CMD_ALT_SHIFT: Modifiers = Modifiers { alt: true, shift: true, ..Modifiers::COMMAND };
const ALT: Modifiers = Modifiers::ALT;
const SHIFT: Modifiers = Modifiers::SHIFT;
/// The Mac's ⌃ key (on other systems ⌘ chords already use Ctrl).
const CTRL: Modifiers = Modifiers::CTRL;
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::CTRL };

/// Every rebindable command with its default. Arrow keys, Escape and the
/// held Z of the hover preview stay fixed (see `FIXED`).
pub const TABLE: &[Spec] = &[
    Spec { cmd: Command::Viewer, id: "view.viewer", group: "View", label: "Open or close the viewer", default: k(NONE, Key::Space), aliases: &[KeyboardShortcut::new(NONE, Key::Enter)] },
    Spec { cmd: Command::ZoomIn, id: "view.zoom.in", group: "View", label: "Zoom in", default: k(CMD, Key::Equals), aliases: &[KeyboardShortcut::new(CMD, Key::Plus)] },
    Spec { cmd: Command::ZoomOut, id: "view.zoom.out", group: "View", label: "Zoom out", default: k(CMD, Key::Minus), aliases: &[] },
    Spec { cmd: Command::ZoomActual, id: "view.zoom.actual", group: "View", label: "Actual size (viewer)", default: k(CMD, Key::Num0), aliases: &[] },
    Spec { cmd: Command::ZoomFit, id: "view.zoom.fit", group: "View", label: "Zoom to fit (viewer)", default: k(CMD, Key::Num9), aliases: &[] },
    Spec { cmd: Command::ToggleSidebar, id: "view.toggle.sidebar", group: "View", label: "Show or hide the sidebar", default: k(CMD_ALT, Key::Num1), aliases: &[] },
    Spec { cmd: Command::ToggleInspector, id: "view.toggle.inspector", group: "View", label: "Show or hide the inspector", default: k(CMD_ALT, Key::Num2), aliases: &[] },
    Spec { cmd: Command::ToggleAll, id: "view.toggle.all", group: "View", label: "Show or hide the sidebar and inspector", default: k(CMD_ALT, Key::Num3), aliases: &[] },
    // Eagle's ⌥1 is Waterfall and ⌥3 the square Grid (its keybind names say the opposite).
    Spec { cmd: Command::LayoutWaterfall, id: "view.layout.waterfall", group: "View", label: "Waterfall layout", default: k(ALT, Key::Num1), aliases: &[] },
    Spec { cmd: Command::LayoutJustified, id: "view.layout.justified", group: "View", label: "Justified layout", default: k(ALT, Key::Num2), aliases: &[] },
    Spec { cmd: Command::LayoutGrid, id: "view.layout.grid", group: "View", label: "Grid layout", default: k(ALT, Key::Num3), aliases: &[] },
    Spec { cmd: Command::LayoutList, id: "view.layout.list", group: "View", label: "List layout", default: k(ALT, Key::Num4), aliases: &[] },
    Spec { cmd: Command::OpenDefault, id: "view.file.opendefault", group: "View", label: "Open in the default app", default: k(SHIFT, Key::Enter), aliases: &[] },
    Spec { cmd: Command::RevealInFinder, id: "view.file.openfinder", group: "View", label: "Show in Finder", default: k(CMD, Key::Enter), aliases: &[] },
    // Eagle: ⌫ goes back (or leaves the viewer), and ⌘←/⌘→ and ⌥←/⌥→
    // step through the history.
    Spec { cmd: Command::Back, id: "go.back", group: "Navigate", label: "Back", default: k(NONE, Key::Backspace), aliases: &[KeyboardShortcut::new(CMD, Key::ArrowLeft), KeyboardShortcut::new(ALT, Key::ArrowLeft)] },
    Spec { cmd: Command::Forward, id: "go.forward", group: "Navigate", label: "Forward", default: k(CMD, Key::ArrowRight), aliases: &[KeyboardShortcut::new(ALT, Key::ArrowRight)] },
    Spec { cmd: Command::Find, id: "find.search.current", group: "Find", label: "Focus search", default: k(CMD, Key::F), aliases: &[] },
    Spec { cmd: Command::ToggleFilter, id: "find.filter.toggle", group: "Find", label: "Show or hide the filter bar", default: k(CMD_SHIFT, Key::F), aliases: &[] },
    Spec { cmd: Command::SelectAll, id: "edit.select.all", group: "Edit", label: "Select all", default: k(CMD, Key::A), aliases: &[] },
    Spec { cmd: Command::Rename, id: "edit.rename", group: "Edit", label: "Rename the selected item", default: k(CMD, Key::R), aliases: &[KeyboardShortcut::new(NONE, Key::F2)] },
    Spec { cmd: Command::Copy, id: "edit.copy", group: "Edit", label: "Copy the selected files", default: k(CMD, Key::C), aliases: &[] },
    Spec { cmd: Command::Paste, id: "edit.paste", group: "Edit", label: "Paste files or an image", default: k(CMD, Key::V), aliases: &[] },
    Spec { cmd: Command::Trash, id: "edit.remove.trash", group: "Edit", label: "Move to the Trash", default: k(CMD, Key::Backspace), aliases: &[] },
    // Eagle's Edit ▸ Move Folder accelerators. ⇧ may or may not turn the
    // bracket into a brace before it reaches us.
    Spec { cmd: Command::FolderUp, id: "edit.folder.arrange.up", group: "Edit", label: "Move folder up", default: k(CMD, Key::CloseBracket), aliases: &[] },
    Spec { cmd: Command::FolderDown, id: "edit.folder.arrange.down", group: "Edit", label: "Move folder down", default: k(CMD, Key::OpenBracket), aliases: &[] },
    Spec { cmd: Command::FolderTop, id: "edit.folder.arrange.top", group: "Edit", label: "Move folder to the top", default: k(CMD_SHIFT, Key::CloseBracket), aliases: &[KeyboardShortcut::new(CMD_SHIFT, Key::CloseCurlyBracket)] },
    Spec { cmd: Command::FolderBottom, id: "edit.folder.arrange.bottom", group: "Edit", label: "Move folder to the bottom", default: k(CMD_SHIFT, Key::OpenBracket), aliases: &[KeyboardShortcut::new(CMD_SHIFT, Key::OpenCurlyBracket)] },
    Spec { cmd: Command::FolderSettings, id: "edit.folder.setting", group: "Edit", label: "Folder settings (name and auto-tags)", default: k(CMD_SHIFT, Key::R), aliases: &[] },
    Spec { cmd: Command::NewFolder, id: "file.create.folder", group: "File", label: "New folder", default: k(CMD_SHIFT, Key::N), aliases: &[] },
    Spec { cmd: Command::CollectWindow, id: "file.collect.window", group: "File", label: "Show or hide the collect window", default: k(CMD_SHIFT, Key::K), aliases: &[] },
    Spec { cmd: Command::SwitchLibrary, id: "library.switch", group: "Library", label: "Open the library switcher", default: k(CMD, Key::L), aliases: &[] },
    // falcon's own: Eagle shows one library at a time.
    Spec { cmd: Command::NextLibrary, id: "library.next", group: "Library", label: "Next open library", default: k(CTRL, Key::Tab), aliases: &[] },
    Spec { cmd: Command::PreviousLibrary, id: "library.previous", group: "Library", label: "Previous open library", default: k(CTRL_SHIFT, Key::Tab), aliases: &[] },
    Spec { cmd: Command::Preferences, id: "app.preferences", group: "App", label: "Preferences", default: k(CMD, Key::Comma), aliases: &[] },
    // Eagle's View ▸ All … Trash.
    Spec { cmd: Command::Page(Page::All), id: "view.all", group: "Go", label: "Show All", default: k(CMD, Key::Num1), aliases: &[] },
    Spec { cmd: Command::Page(Page::Uncategorized), id: "view.unfiled", group: "Go", label: "Show Uncategorized", default: k(CMD, Key::Num2), aliases: &[] },
    Spec { cmd: Command::Page(Page::Untagged), id: "view.untagged", group: "Go", label: "Show Untagged", default: k(CMD, Key::Num3), aliases: &[] },
    Spec { cmd: Command::Page(Page::Recent), id: "view.recent", group: "Go", label: "Show Recently Used", default: k(CMD, Key::Num4), aliases: &[] },
    Spec { cmd: Command::Page(Page::Random), id: "view.random", group: "Go", label: "Show Random", default: k(CMD, Key::Num5), aliases: &[] },
    Spec { cmd: Command::Page(Page::AllTags), id: "view.alltags", group: "Go", label: "Show All Tags", default: k(CMD, Key::Num6), aliases: &[] },
    Spec { cmd: Command::Page(Page::Trash), id: "view.trash", group: "Go", label: "Show the Trash", default: k(CMD, Key::Num7), aliases: &[] },
    // Eagle's View ▸ Scroll; ⌘↑ and ⌘↓ select the first and last item.
    Spec { cmd: Command::Scroll(Scroll::Top), id: "view.scroll.home", group: "Go", label: "Scroll to the top", default: k(NONE, Key::Home), aliases: &[] },
    Spec { cmd: Command::Scroll(Scroll::Bottom), id: "view.scroll.end", group: "Go", label: "Scroll to the bottom", default: k(NONE, Key::End), aliases: &[] },
    Spec { cmd: Command::Scroll(Scroll::PageUp), id: "view.scroll.prevpage", group: "Go", label: "Scroll up a page", default: k(NONE, Key::PageUp), aliases: &[] },
    Spec { cmd: Command::Scroll(Scroll::PageDown), id: "view.scroll.nextpage", group: "Go", label: "Scroll down a page", default: k(NONE, Key::PageDown), aliases: &[] },
    Spec { cmd: Command::SelectFirst, id: "view.select.first", group: "Go", label: "Select the first item", default: k(CMD, Key::ArrowUp), aliases: &[] },
    Spec { cmd: Command::SelectLast, id: "view.select.last", group: "Go", label: "Select the last item", default: k(CMD, Key::ArrowDown), aliases: &[] },
    // Eagle binds 0–5 outside its keybind table (`organize.rating.*` have no default).
    Spec { cmd: Command::Rate(0), id: "organize.rating.0", group: "Organize", label: "Remove the rating", default: k(NONE, Key::Num0), aliases: &[] },
    Spec { cmd: Command::Rate(1), id: "organize.rating.1", group: "Organize", label: "Rate 1 star", default: k(NONE, Key::Num1), aliases: &[] },
    Spec { cmd: Command::Rate(2), id: "organize.rating.2", group: "Organize", label: "Rate 2 stars", default: k(NONE, Key::Num2), aliases: &[] },
    Spec { cmd: Command::Rate(3), id: "organize.rating.3", group: "Organize", label: "Rate 3 stars", default: k(NONE, Key::Num3), aliases: &[] },
    Spec { cmd: Command::Rate(4), id: "organize.rating.4", group: "Organize", label: "Rate 4 stars", default: k(NONE, Key::Num4), aliases: &[] },
    Spec { cmd: Command::Rate(5), id: "organize.rating.5", group: "Organize", label: "Rate 5 stars", default: k(NONE, Key::Num5), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Toggle), id: "player.playAndPause", group: PLAYER, label: "Play or pause", default: k(NONE, Key::Space), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Frames(-1)), id: "player.prev1frame", group: PLAYER, label: "Previous frame", default: k(NONE, Key::OpenBracket), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Frames(1)), id: "player.next1frame", group: PLAYER, label: "Next frame", default: k(NONE, Key::CloseBracket), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Frames(-10)), id: "player.prev10frame", group: PLAYER, label: "Back 10 frames", default: k(SHIFT, Key::OpenBracket), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Frames(10)), id: "player.next10frame", group: PLAYER, label: "Forward 10 frames", default: k(SHIFT, Key::CloseBracket), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Volume(true)), id: "player.volume.increase", group: PLAYER, label: "Volume up", default: k(CMD, Key::ArrowUp), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Volume(false)), id: "player.volume.decrease", group: PLAYER, label: "Volume down", default: k(CMD, Key::ArrowDown), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Skip(true)), id: "player.step.forward", group: PLAYER, label: "Skip forward", default: k(CMD, Key::ArrowRight), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Skip(false)), id: "player.step.backward", group: PLAYER, label: "Skip back", default: k(CMD, Key::ArrowLeft), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Speed(true)), id: "player.speed.up", group: PLAYER, label: "Faster", default: k(SHIFT, Key::Period), aliases: &[] },
    Spec { cmd: Command::Player(PlayerKey::Speed(false)), id: "player.speed.down", group: PLAYER, label: "Slower", default: k(SHIFT, Key::Comma), aliases: &[] },
    // falcon's own id: Eagle handles N outside its keybind table.
    Spec { cmd: Command::Player(PlayerKey::AddComment), id: "player.comment.add", group: PLAYER, label: "Add a comment at the current time", default: k(NONE, Key::N), aliases: &[] },
    // Export and copy-out (`App::export_keys`).
    Spec { cmd: Command::ExportComputer, id: "file.export.item.computer", group: "Export", label: "Export to Computer", default: k(CMD, Key::E), aliases: &[] },
    Spec { cmd: Command::ExportPack, id: "file.export.item.eaglepack", group: "Export", label: "Export as .eaglepack", default: k(CMD_SHIFT, Key::E), aliases: &[] },
    Spec { cmd: Command::ExportFormat, id: "file.export.item.as", group: "Export", label: "Export in a chosen format and size", default: k(SHIFT, Key::E), aliases: &[] },
    // Unbound by default, as in Eagle.
    Spec { cmd: Command::ExportCsv, id: "file.export.csv", group: "Export", label: "Export to CSV", default: None, aliases: &[] },
    Spec { cmd: Command::CopyPath, id: "edit.copy.path", group: "Copy", label: "Copy file paths", default: k(CMD_ALT, Key::C), aliases: &[] },
    Spec { cmd: Command::CopyFolderPath, id: "edit.copy.folderpath", group: "Copy", label: "Copy item folder paths", default: k(CMD_ALT_SHIFT, Key::C), aliases: &[] },
    Spec { cmd: Command::CopyName, id: "edit.copy.name", group: "Copy", label: "Copy names", default: None, aliases: &[] },
    // falcon's own id: Eagle's Copy Link entry copies its local item link.
    Spec { cmd: Command::CopyLink, id: "edit.copy.sourceurl", group: "Copy", label: "Copy source URLs", default: None, aliases: &[] },
    Spec { cmd: Command::CopyThumbnail, id: "edit.copy.thumbnail", group: "Copy", label: "Copy thumbnail", default: None, aliases: &[] },
    Spec { cmd: Command::CopyBase64, id: "edit.copy.base64", group: "Copy", label: "Copy as Base64", default: None, aliases: &[] },
];

/// Keys that cannot be rebound, listed in the Shortcuts section.
pub const FIXED: &[(&str, &str)] = &[
    ("← → ↑ ↓", "Move the selection, or step through the viewer"),
    ("Esc", "Close the viewer, or clear the selection"),
    ("0 / 1 / + / −", "Viewer: zoom to fit, actual size, zoom in, zoom out"),
    ("Tab", "Show or hide the sidebar and inspector (no text field focused)"),
    ("Hold Z", "Hover preview of the card under the pointer"),
];

pub fn spec(cmd: Command) -> &'static Spec {
    TABLE.iter().find(|s| s.cmd == cmd).expect("every command is in TABLE")
}

/// The user's changes to the defaults, by `Spec::id`. An empty string
/// unbinds the command. Saved with the app state.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(transparent)]
pub struct Shortcuts(pub BTreeMap<String, String>);

impl Shortcuts {
    /// The chord bound to a command now, if any.
    pub fn binding(&self, cmd: Command) -> Option<KeyboardShortcut> {
        let s = spec(cmd);
        match self.0.get(s.id) {
            Some(text) => parse(text),
            None => s.default.filter(|c| !self.taken(s, *c)),
        }
    }

    /// Whether another command of the same conflict group was explicitly
    /// bound to `chord`. Recording refuses such clashes, but a default
    /// that changed in a newer falcon can land on a chord the user chose
    /// earlier; the user's choice then wins and the default gives way.
    fn taken(&self, s: &Spec, chord: KeyboardShortcut) -> bool {
        TABLE.iter().filter(|o| o.cmd != s.cmd && same_scope(o, s)).any(|o| self.0.get(o.id).and_then(|t| parse(t)).is_some_and(|c| same(c, Some(chord))))
    }

    pub fn is_default(&self, cmd: Command) -> bool {
        !self.0.contains_key(spec(cmd).id)
    }

    /// Every chord that triggers a command (the binding, plus the alias
    /// while the binding is the default).
    pub fn chords(&self, cmd: Command) -> Vec<KeyboardShortcut> {
        let s = spec(cmd);
        let mut v: Vec<_> = self.binding(cmd).into_iter().collect();
        if self.is_default(cmd) {
            v.extend(s.aliases.iter().copied().filter(|c| !self.taken(s, *c)));
        }
        v
    }

    /// Binds `chord` (or nothing) to `cmd`, unless another command already
    /// uses it; then returns that command and changes nothing, as Eagle's
    /// recorder does.
    pub fn set(&mut self, cmd: Command, chord: Option<KeyboardShortcut>) -> Result<(), Command> {
        if let Some(c) = chord
            && let Some(other) = self.conflict(cmd, c)
        {
            return Err(other);
        }
        let s = spec(cmd);
        if chord.map(|c| same(c, s.default)) == Some(true) || (chord.is_none() && s.default.is_none()) {
            self.0.remove(s.id);
        } else {
            self.0.insert(s.id.to_string(), chord.map(|c| format(&c)).unwrap_or_default());
        }
        Ok(())
    }

    pub fn reset(&mut self, cmd: Command) {
        self.0.remove(spec(cmd).id);
    }

    /// Another command of the same conflict group whose chords include
    /// `chord`.
    pub fn conflict(&self, cmd: Command, chord: KeyboardShortcut) -> Option<Command> {
        let me = spec(cmd);
        TABLE.iter().filter(|s| s.cmd != cmd && same_scope(s, me)).map(|s| s.cmd).find(|&c| self.chords(c).iter().any(|x| same(*x, Some(chord))))
    }

    /// The command a key press triggers, outside the Player group. When
    /// several match (⌘F and ⌘⇧F for ⌘⇧F), the chord with the most
    /// modifiers wins.
    pub fn resolve(&self, key: Key, pressed: Modifiers) -> Option<Command> {
        let mut best: Option<(Command, u8)> = None;
        for s in TABLE.iter().filter(|s| s.group != PLAYER) {
            for c in self.chords(s.cmd) {
                // A bare key (a digit, R, /) is not also its ⌥ chord.
                let bare_with_alt = c.modifiers.is_none() && pressed.alt;
                if c.logical_key == key && pressed.matches_logically(c.modifiers) && !bare_with_alt {
                    let n = weight(c.modifiers);
                    if best.is_none_or(|(_, b)| n > b) {
                        best = Some((s.cmd, n));
                    }
                }
            }
        }
        best.map(|b| b.0)
    }

    /// The commands pressed this frame.
    pub fn pressed(&self, input: &egui::InputState) -> Vec<Command> {
        input.events.iter().filter_map(press).filter_map(|(key, m)| self.resolve(key, m)).collect()
    }

    /// The player key a press triggers. `keys` holds the logical key and,
    /// when known, the physical one, so ⇧[ works whether the keyboard
    /// reports `[` or `{`. Modifiers must match exactly, as Electron's
    /// accelerators do, so ⇧[ is not also [.
    pub fn player(&self, keys: &[Key], pressed: Modifiers) -> Option<PlayerKey> {
        let p = normalize(pressed);
        for &key in keys {
            for s in TABLE.iter().filter(|s| s.group == PLAYER) {
                if self.chords(s.cmd).iter().any(|c| c.logical_key == key && canon(c.modifiers) == p)
                    && let Command::Player(k) = s.cmd
                {
                    return Some(k);
                }
            }
        }
        None
    }
}

fn weight(m: Modifiers) -> u8 {
    m.command as u8 + m.ctrl as u8 + m.shift as u8 + m.alt as u8
}

fn same(a: KeyboardShortcut, b: Option<KeyboardShortcut>) -> bool {
    b.is_some_and(|b| a.logical_key == b.logical_key && canon(a.modifiers) == canon(b.modifiers))
}

/// Pressed modifiers as falcon stores them: `command` is ⌘ on macOS and
/// Ctrl elsewhere; `ctrl` is only the Mac's separate ⌃ key.
pub fn normalize(m: Modifiers) -> Modifiers {
    let command = m.command || m.mac_cmd;
    Modifiers { alt: m.alt, shift: m.shift, ctrl: m.ctrl && (!m.command || m.mac_cmd), command, mac_cmd: false }
}

/// A stored chord's modifiers with `mac_cmd` folded into `command`.
fn canon(m: Modifiers) -> Modifiers {
    Modifiers { command: m.command || m.mac_cmd, mac_cmd: false, ..m }
}

/// A key press from an input event. ⌘C, ⌘X and ⌘V arrive as Copy, Cut and
/// Paste events instead of key presses, so they are turned back into keys.
pub fn press(e: &egui::Event) -> Option<(Key, Modifiers)> {
    match e {
        egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
        egui::Event::Copy => Some((Key::C, Modifiers::COMMAND)),
        egui::Event::Cut => Some((Key::X, Modifiers::COMMAND)),
        egui::Event::Paste(_) | egui::Event::PasteImage(_) => Some((Key::V, Modifiers::COMMAND)),
        _ => None,
    }
}

/// What the recorder makes of a key press.
#[derive(PartialEq, Debug)]
pub enum Recorded {
    /// Bind this chord.
    Chord(KeyboardShortcut),
    /// Backspace alone clears the binding (Eagle).
    Clear,
    /// Escape alone stops recording.
    Cancel,
    /// Not a usable chord (a bare letter or a lone modifier); keep waiting.
    Ignore,
}

/// Eagle's recorder rules: letters, digits and most symbols need a
/// modifier; function keys, Space, Enter, arrows, Home/End, Page Up/Down,
/// Delete and brackets may be bound alone.
pub fn record(key: Key, m: Modifiers) -> Recorded {
    let m = normalize(m);
    let any = m.command || m.ctrl || m.alt || m.shift;
    match key {
        Key::Escape if !any => Recorded::Cancel,
        Key::Backspace if !any => Recorded::Clear,
        _ if !any && !can_stand_alone(key) => Recorded::Ignore,
        _ => Recorded::Chord(KeyboardShortcut::new(m, key)),
    }
}

fn can_stand_alone(key: Key) -> bool {
    use Key::*;
    matches!(
        key,
        F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12 | Space | Enter | ArrowUp | ArrowDown | ArrowLeft | ArrowRight
            | Home | End | PageUp | PageDown | Delete | OpenBracket | CloseBracket
    )
}

/// Eagle's accelerator text: modifiers in the order Ctrl, Command, Shift,
/// Alt, then the key, joined by " + ".
pub fn format(c: &KeyboardShortcut) -> String {
    let m = canon(c.modifiers);
    let mut parts: Vec<&str> = Vec::new();
    if m.ctrl {
        parts.push("Ctrl");
    }
    if m.command {
        parts.push("Command");
    }
    if m.shift {
        parts.push("Shift");
    }
    if m.alt {
        parts.push("Alt");
    }
    parts.push(key_name(c.logical_key));
    parts.join(" + ")
}

/// Parses `format`'s text, and Eagle's spellings (`CmdOrCtrl`, `Option`,
/// `Control`, `Up`, no spaces around `+`).
pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // The key may itself be "+": split on '+' but keep a trailing one.
    let (mods, key) = match text.strip_suffix('+') {
        Some(rest) if rest.trim_end().ends_with('+') || rest.trim().is_empty() => (rest.trim_end().trim_end_matches('+'), "+"),
        _ => match text.rsplit_once('+') {
            Some((m, k)) => (m, k.trim()),
            None => ("", text),
        },
    };
    let mut m = Modifiers::NONE;
    for part in mods.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part {
            "Command" | "Cmd" | "CmdOrCtrl" | "CommandOrControl" | "Super" | "Meta" => m.command = true,
            "Ctrl" | "Control" => m.ctrl = true,
            "Shift" => m.shift = true,
            "Alt" | "Option" => m.alt = true,
            _ => return None,
        }
    }
    let key = Key::from_name(key)?;
    Some(KeyboardShortcut::new(m, key))
}

fn key_name(key: Key) -> &'static str {
    match key {
        Key::Equals => "=",
        Key::Minus => "-",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::Semicolon => ";",
        Key::Quote => "'",
        Key::Backtick => "`",
        other => other.name(),
    }
}

/// How a chord looks in the UI (⌘⇧N on a Mac).
pub fn display(ctx: &egui::Context, c: &KeyboardShortcut) -> String {
    ctx.format_shortcut(&KeyboardShortcut::new(canon(c.modifiers), c.logical_key))
}

/// How long a rejected chord's "Used by" note stays, as in Eagle.
const CONFLICT_SECS: f64 = 1.5;

/// The Shortcuts section of Preferences: a row per command whose chord
/// button records the next key press.
#[derive(Default)]
pub struct Recorder {
    recording: Option<Command>,
    /// A rejected chord: the command being set, the one using it, and when.
    conflict: Option<(Command, Command, f64)>,
    confirm_reset: bool,
}

impl Recorder {
    pub fn stop(&mut self) {
        *self = Recorder::default();
    }

    /// Handles one key press while recording.
    pub fn feed(&mut self, keys: &mut Shortcuts, key: Key, m: Modifiers, now: f64) {
        let Some(cmd) = self.recording else { return };
        match record(key, m) {
            Recorded::Cancel => self.recording = None,
            Recorded::Clear => {
                let _ = keys.set(cmd, None);
                self.recording = None;
            }
            Recorded::Ignore => {}
            Recorded::Chord(c) => match keys.set(cmd, Some(c)) {
                Ok(()) => {
                    self.recording = None;
                    self.conflict = None;
                }
                Err(other) => self.conflict = Some((cmd, other, now)),
            },
        }
    }

    /// While recording, takes this frame's key presses out of the input so
    /// nothing else reacts to them, and records them.
    pub fn capture(&mut self, ctx: &egui::Context, keys: &mut Shortcuts) {
        if self.recording.is_none() {
            return;
        }
        let (presses, now) = ctx.input_mut(|i| {
            let presses: Vec<_> = i.events.iter().filter_map(press).collect();
            i.events.retain(|e| press(e).is_none());
            (presses, i.time)
        });
        for (key, m) in presses {
            self.feed(keys, key, m, now);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, keys: &mut Shortcuts) {
        use crate::theme::{self as t, font};
        use egui::{Align2, CornerRadius, RichText, Sense, Stroke, vec2};
        let now = ui.input(|i| i.time);
        if self.conflict.is_some_and(|c| now - c.2 > CONFLICT_SECS) {
            self.conflict = None;
        }
        if self.conflict.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut group = "";
        for s in TABLE {
            if s.group != group {
                group = s.group;
                ui.add_space(if s.cmd == TABLE[0].cmd { 4.0 } else { 10.0 });
                ui.label(RichText::new(group).strong().size(13.0));
            }
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
            let p = ui.painter();
            p.text(row.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, s.label, font(13.0), t::p().text);
            let button = egui::Rect::from_min_size(egui::pos2(row.right() - 150.0, row.top() + 3.0), vec2(150.0, 24.0));
            let resp = ui.interact(button, ui.scope_id().with(s.id), Sense::click());
            let active = self.recording == Some(s.cmd);
            let fill = if resp.hovered() || active { t::p().button_hover } else { t::p().button };
            let stroke = if active { Stroke::new(1.0, t::p().primary) } else { Stroke::NONE };
            p.rect(button, CornerRadius::same(6), fill, stroke, egui::StrokeKind::Inside);
            let (text, color) = match (active, keys.binding(s.cmd)) {
                (true, _) => ("Type a shortcut".to_string(), t::p().text_muted),
                (false, Some(c)) => (display(ui.ctx(), &c), t::p().text),
                (false, None) => ("None".to_string(), t::p().text_disabled),
            };
            p.text(button.center(), Align2::CENTER_CENTER, text, font(12.5), color);
            if resp.clicked() {
                self.recording = if active { None } else { Some(s.cmd) };
                self.conflict = None;
            }
            if let Some((_, other, _)) = self.conflict.filter(|c| c.0 == s.cmd) {
                let note = format!("Used by {}", spec(other).label);
                p.text(button.left_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, note, font(11.5), egui::Color32::from_rgb(0xff, 0xb0, 0x40));
            } else if !keys.is_default(s.cmd) {
                let r = egui::Rect::from_center_size(button.left_center() - vec2(16.0, 0.0), vec2(22.0, 22.0));
                let reset = ui.interact(r, ui.scope_id().with((s.id, "reset")), Sense::click()).on_hover_text("Use the default");
                if reset.hovered() {
                    p.rect_filled(r, CornerRadius::same(5), t::p().button_hover);
                }
                p.text(r.center(), Align2::CENTER_CENTER, crate::icons::ARROW_COUNTER_CLOCKWISE, font(13.0), t::p().text_muted);
                if reset.clicked() {
                    keys.reset(s.cmd);
                    // The default may now clash with a chord given to
                    // another command; that command gives way.
                    if let Some(c) = s.default {
                        for other in TABLE.iter().filter(|o| o.cmd != s.cmd && same_scope(o, s)) {
                            if keys.chords(other.cmd).iter().any(|x| same(*x, Some(c))) {
                                let _ = keys.set(other.cmd, None);
                            }
                        }
                    }
                }
            }
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Fixed").strong().size(13.0));
        for (k, what) in FIXED {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
            ui.painter().text(row.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, *what, font(13.0), t::p().text_muted);
            ui.painter().text(row.right_center() - vec2(75.0, 0.0), Align2::CENTER_CENTER, *k, font(12.5), t::p().text_muted);
        }
        ui.add_space(12.0);
        ui.separator();
        ui.horizontal(|ui| {
            if self.confirm_reset {
                ui.label("Restore every shortcut to its default?");
                if ui.button("Restore").clicked() {
                    keys.0.clear();
                    self.stop();
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_reset = false;
                }
            } else if ui.add_enabled(!keys.0.is_empty(), egui::Button::new("Restore Defaults")).clicked() {
                self.confirm_reset = true;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn defaults_are_unique() {
        // Within each conflict group; the player's Space overrides the
        // viewer's while a video plays.
        let mut seen = HashSet::new();
        for s in TABLE {
            for c in s.default.iter().chain(s.aliases.iter()) {
                assert!(seen.insert((s.group == PLAYER, format(c))), "{} reuses {}", s.id, format(c));
            }
        }
        let ids: HashSet<_> = TABLE.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), TABLE.len(), "ids are unique");
        let cmds: HashSet<_> = TABLE.iter().map(|s| s.cmd).collect();
        assert_eq!(cmds.len(), TABLE.len(), "each command appears once");
    }

    #[test]
    fn layout_keys_match_eagle() {
        let keys = Shortcuts::default();
        assert_eq!(keys.resolve(Key::Num1, Modifiers::ALT), Some(Command::LayoutWaterfall));
        assert_eq!(keys.resolve(Key::Num2, Modifiers::ALT), Some(Command::LayoutJustified));
        assert_eq!(keys.resolve(Key::Num3, Modifiers::ALT), Some(Command::LayoutGrid));
        assert_eq!(keys.resolve(Key::Num4, Modifiers::ALT), Some(Command::LayoutList));
    }

    #[test]
    fn no_conflicts_with_defaults() {
        let keys = Shortcuts::default();
        for s in TABLE {
            if let Some(c) = s.default {
                assert_eq!(keys.conflict(s.cmd, c), None, "{}", s.id);
            }
        }
    }

    #[test]
    fn format_parse_round_trips() {
        for s in TABLE {
            for c in s.default.iter().chain(s.aliases.iter()) {
                let text = format(c);
                let back = parse(&text).unwrap_or_else(|| panic!("parse {text}"));
                assert!(same(back, Some(*c)), "{text}");
                assert_eq!(format(&back), text);
            }
        }
        let c = KeyboardShortcut::new(Modifiers { ctrl: true, alt: true, ..Modifiers::COMMAND }, Key::Backslash);
        assert_eq!(format(&c), "Ctrl + Command + Alt + \\");
        assert!(same(parse(&format(&c)).unwrap(), Some(c)));
    }

    #[test]
    fn parses_eagle_spellings() {
        assert_eq!(format(&parse("CmdOrCtrl + Shift + N").unwrap()), "Command + Shift + N");
        assert_eq!(format(&parse("CmdOrCtrl+Shift+C").unwrap()), "Command + Shift + C");
        assert_eq!(format(&parse("Option + Up").unwrap()), "Alt + Up");
        assert_eq!(format(&parse("CmdOrCtrl + =").unwrap()), "Command + =");
        assert_eq!(format(&parse("Command + +").unwrap()), "Command + Plus");
        assert_eq!(format(&parse("F5").unwrap()), "F5");
        assert_eq!(parse(""), None);
        assert_eq!(parse("Hyper + A"), None);
        assert_eq!(parse("Command + Nope"), None);
    }

    #[test]
    fn page_keys_follow_eagles_view_menu() {
        let keys = Shortcuts::default();
        let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7];
        let pages = [Page::All, Page::Uncategorized, Page::Untagged, Page::Recent, Page::Random, Page::AllTags, Page::Trash];
        for (key, page) in digits.into_iter().zip(pages) {
            assert_eq!(keys.resolve(key, CMD), Some(Command::Page(page)), "{key:?}");
        }
        // ⌥1 is still a layout and ⌥⌘1 the sidebar.
        assert_eq!(keys.resolve(Key::Num1, ALT), Some(Command::LayoutWaterfall));
        assert_eq!(keys.resolve(Key::Num1, CMD_ALT), Some(Command::ToggleSidebar));
    }

    #[test]
    fn scroll_keys() {
        let keys = Shortcuts::default();
        assert_eq!(keys.resolve(Key::Home, Modifiers::NONE), Some(Command::Scroll(Scroll::Top)));
        assert_eq!(keys.resolve(Key::End, Modifiers::NONE), Some(Command::Scroll(Scroll::Bottom)));
        assert_eq!(keys.resolve(Key::PageUp, Modifiers::NONE), Some(Command::Scroll(Scroll::PageUp)));
        assert_eq!(keys.resolve(Key::PageDown, Modifiers::NONE), Some(Command::Scroll(Scroll::PageDown)));
        assert_eq!(keys.resolve(Key::ArrowUp, CMD), Some(Command::SelectFirst));
        assert_eq!(keys.resolve(Key::ArrowDown, CMD), Some(Command::SelectLast));
        // Plain ↑ ↓ stay fixed selection moves.
        assert_eq!(keys.resolve(Key::ArrowUp, Modifiers::NONE), None);
    }

    #[test]
    fn scroll_offsets() {
        // A 600 px view over 2000 px of content, now at 500.
        assert_eq!(Scroll::Top.offset(500.0, 600.0, 2000.0), 0.0);
        assert_eq!(Scroll::Bottom.offset(500.0, 600.0, 2000.0), 1400.0);
        assert_eq!(Scroll::PageDown.offset(500.0, 600.0, 2000.0), 1028.0);
        assert_eq!(Scroll::PageUp.offset(500.0, 600.0, 2000.0), 0.0);
        assert_eq!(Scroll::PageDown.offset(1300.0, 600.0, 2000.0), 1400.0);
        // Content shorter than the view doesn't scroll.
        assert_eq!(Scroll::Bottom.offset(0.0, 600.0, 300.0), 0.0);
    }

    #[test]
    fn digits_rate() {
        let keys = Shortcuts::default();
        let digits = [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];
        for (n, key) in digits.into_iter().enumerate() {
            assert_eq!(keys.resolve(key, Modifiers::NONE), Some(Command::Rate(n as u8)));
        }
        // ⌥1 is a layout, and ⌥5 (no binding) is not 5.
        assert_eq!(keys.resolve(Key::Num1, ALT), Some(Command::LayoutWaterfall));
        assert_eq!(keys.resolve(Key::Num5, ALT), None);
        assert_eq!(keys.resolve(Key::Num6, Modifiers::NONE), None);
    }

    #[test]
    fn library_switch_keys() {
        let keys = Shortcuts::default();
        assert_eq!(keys.resolve(Key::L, Modifiers::COMMAND), Some(Command::SwitchLibrary));
        assert_eq!(keys.resolve(Key::Tab, CTRL), Some(Command::NextLibrary));
        assert_eq!(keys.resolve(Key::Tab, CTRL_SHIFT), Some(Command::PreviousLibrary));
        // Plain Tab and ⇧Tab stay with egui's focus moves.
        assert_eq!(keys.resolve(Key::Tab, Modifiers::NONE), None);
        assert_eq!(keys.resolve(Key::Tab, Modifiers::SHIFT), None);
    }

    #[test]
    fn panel_toggle_keys() {
        let keys = Shortcuts::default();
        assert_eq!(keys.resolve(Key::Num1, CMD_ALT), Some(Command::ToggleSidebar));
        assert_eq!(keys.resolve(Key::Num2, CMD_ALT), Some(Command::ToggleInspector));
        assert_eq!(keys.resolve(Key::Num3, CMD_ALT), Some(Command::ToggleAll));
        assert_eq!(spec(Command::ToggleAll).id, "view.toggle.all");
    }

    #[test]
    fn viewer_zoom_keys() {
        let keys = Shortcuts::default();
        assert_eq!(keys.resolve(Key::Equals, CMD), Some(Command::ZoomIn));
        assert_eq!(keys.resolve(Key::Minus, CMD), Some(Command::ZoomOut));
        assert_eq!(keys.resolve(Key::Num0, CMD), Some(Command::ZoomActual));
        assert_eq!(keys.resolve(Key::Num9, CMD), Some(Command::ZoomFit));
        assert_eq!(keys.resolve(Key::Num0, Modifiers::NONE), Some(Command::Rate(0)), "plain 0 rates; in the viewer it stays the fixed fit key (`keyboard` drops Rate there)");
    }

    #[test]
    fn eagle_history_keys() {
        let keys = Shortcuts::default();
        let cmd = Modifiers { command: true, mac_cmd: true, ..Default::default() };
        assert_eq!(keys.resolve(Key::Backspace, Modifiers::NONE), Some(Command::Back));
        assert_eq!(keys.resolve(Key::ArrowLeft, cmd), Some(Command::Back));
        assert_eq!(keys.resolve(Key::ArrowLeft, Modifiers::ALT), Some(Command::Back));
        assert_eq!(keys.resolve(Key::ArrowRight, cmd), Some(Command::Forward));
        assert_eq!(keys.resolve(Key::ArrowRight, Modifiers::ALT), Some(Command::Forward));
        // ⌘⌫ is still the Trash, and plain arrows are no command.
        assert_eq!(keys.resolve(Key::Backspace, cmd), Some(Command::Trash));
        assert_eq!(keys.resolve(Key::ArrowLeft, Modifiers::NONE), None);
        assert_eq!(keys.chords(Command::Back).len(), 3);
    }

    #[test]
    fn eagle_folder_arrange_and_rename_keys() {
        let keys = Shortcuts::default();
        assert_eq!(keys.resolve(Key::CloseBracket, CMD), Some(Command::FolderUp));
        assert_eq!(keys.resolve(Key::OpenBracket, CMD), Some(Command::FolderDown));
        // ⇧ may or may not turn the bracket into a brace before it reaches us.
        for (key, cmd) in [(Key::CloseBracket, Command::FolderTop), (Key::CloseCurlyBracket, Command::FolderTop), (Key::OpenBracket, Command::FolderBottom), (Key::OpenCurlyBracket, Command::FolderBottom)] {
            assert_eq!(keys.resolve(key, CMD_SHIFT), Some(cmd), "{key:?}");
        }
        assert_eq!(keys.resolve(Key::R, CMD), Some(Command::Rename));
        assert_eq!(keys.resolve(Key::F2, Modifiers::NONE), Some(Command::Rename));
    }

    #[test]
    fn saved_choices_beat_changed_defaults() {
        // Saved by an older falcon: ⌘R for Select All (Rename's default is
        // now ⌘R) and ⌃Tab for the filter (now Next Library).
        let keys: Shortcuts = serde_json::from_str(r#"{"edit.select.all":"Command + R","find.filter.toggle":"Ctrl + Tab"}"#).unwrap();
        assert_eq!(keys.resolve(Key::R, CMD), Some(Command::SelectAll));
        assert_eq!(keys.binding(Command::Rename), None);
        assert_eq!(keys.resolve(Key::F2, Modifiers::NONE), Some(Command::Rename), "the alias still works");
        assert_eq!(keys.resolve(Key::Tab, CTRL), Some(Command::ToggleFilter));
        assert_eq!(keys.binding(Command::NextLibrary), None);
        // Recording still refuses a clash with the saved chord.
        assert_eq!(keys.clone().set(Command::Find, parse("Command + R")), Err(Command::SelectAll));
        // Untouched commands keep their new defaults.
        assert_eq!(keys.resolve(Key::Backspace, Modifiers::NONE), Some(Command::Back));
        // A saved id falcon no longer has, or text it can't parse, is ignored.
        let odd: Shortcuts = serde_json::from_str(r#"{"go.nowhere":"Command + B","go.back":"Hyper + ["}"#).unwrap();
        assert_eq!(odd.binding(Command::Back), None);
        assert_eq!(odd.resolve(Key::B, CMD), None);
        assert_eq!(odd.resolve(Key::ArrowRight, CMD), Some(Command::Forward));
    }

    #[test]
    fn open_keys_beat_the_viewer_enter() {
        let s = Shortcuts::default();
        assert_eq!(s.resolve(Key::Enter, Modifiers::NONE), Some(Command::Viewer));
        assert_eq!(s.resolve(Key::Enter, Modifiers::SHIFT), Some(Command::OpenDefault));
        assert_eq!(s.resolve(Key::Enter, Modifiers::COMMAND), Some(Command::RevealInFinder));
        assert_eq!(spec(Command::OpenDefault).id, "view.file.opendefault");
        assert_eq!(format(&s.binding(Command::RevealInFinder).unwrap()), parse("CmdOrCtrl + Enter").map(|c| format(&c)).unwrap());
    }

    #[test]
    fn resolve_prefers_the_most_specific_chord() {
        let keys = Shortcuts::default();
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers { shift: true, ..cmd };
        assert_eq!(keys.resolve(Key::F, cmd), Some(Command::Find));
        assert_eq!(keys.resolve(Key::F, cmd_shift), Some(Command::ToggleFilter));
        assert_eq!(keys.resolve(Key::N, cmd_shift), Some(Command::NewFolder));
        assert_eq!(keys.resolve(Key::N, cmd), None);
        assert_eq!(keys.resolve(Key::Plus, cmd_shift), Some(Command::ZoomIn), "⌘+ needs shift on US keyboards");
        assert_eq!(keys.resolve(Key::Enter, Modifiers::NONE), Some(Command::Viewer));
        assert_eq!(keys.resolve(Key::Space, cmd), None);
        // macOS sends ⌘ as both mac_cmd and command.
        let mac = Modifiers { mac_cmd: true, command: true, ..Default::default() };
        assert_eq!(keys.resolve(Key::A, mac), Some(Command::SelectAll));
    }

    #[test]
    fn clipboard_events_map_to_keys() {
        let keys = Shortcuts::default();
        let (k, m) = press(&egui::Event::Copy).unwrap();
        assert_eq!(keys.resolve(k, m), Some(Command::Copy));
        let (k, m) = press(&egui::Event::Paste("x".into())).unwrap();
        assert_eq!(keys.resolve(k, m), Some(Command::Paste));
    }

    #[test]
    fn set_rejects_conflicts_and_reset_restores() {
        let mut keys = Shortcuts::default();
        let cmd_f = parse("Command + F").unwrap();
        assert_eq!(keys.set(Command::NewFolder, Some(cmd_f)), Err(Command::Find));
        assert!(keys.is_default(Command::NewFolder));
        // The viewer's alias Enter is taken while the viewer is default.
        assert_eq!(keys.set(Command::Rename, Some(parse("Enter").unwrap())), Err(Command::Viewer));

        let r = parse("Command + K").unwrap();
        assert_eq!(keys.set(Command::Rename, Some(r)), Ok(()));
        assert_eq!(keys.0.get("edit.rename").map(String::as_str), Some("Command + K"));
        assert_eq!(keys.resolve(Key::K, Modifiers::COMMAND), Some(Command::Rename));
        assert_eq!(keys.resolve(Key::F2, Modifiers::NONE), None, "a changed binding drops the F2 alias");

        keys.set(Command::Rename, None).unwrap();
        assert_eq!(keys.binding(Command::Rename), None);
        assert_eq!(keys.0.get("edit.rename").map(String::as_str), Some(""));

        keys.reset(Command::Rename);
        assert!(keys.0.is_empty());
        // Setting the default chord leaves no override behind.
        keys.set(Command::Find, Some(cmd_f)).unwrap();
        assert!(keys.0.is_empty());
        // Rebinding the viewer drops its Enter alias.
        keys.set(Command::Viewer, Some(parse("F3").unwrap())).unwrap();
        assert_eq!(keys.resolve(Key::Enter, Modifiers::NONE), None);
    }

    #[test]
    fn recorder_rules() {
        let none = Modifiers::NONE;
        assert_eq!(record(Key::Escape, none), Recorded::Cancel);
        assert_eq!(record(Key::Backspace, none), Recorded::Clear);
        assert_eq!(record(Key::A, none), Recorded::Ignore);
        assert!(matches!(record(Key::F5, none), Recorded::Chord(_)));
        assert!(matches!(record(Key::OpenBracket, none), Recorded::Chord(_)));
        let mac = Modifiers { mac_cmd: true, command: true, shift: true, ..Default::default() };
        let Recorded::Chord(c) = record(Key::Backspace, mac) else { panic!() };
        assert_eq!(format(&c), "Command + Shift + Backspace");
        // Ctrl on Windows/Linux is `command`, not a separate ctrl.
        let pc = Modifiers { ctrl: true, command: true, ..Default::default() };
        let Recorded::Chord(c) = record(Key::K, pc) else { panic!() };
        assert_eq!(format(&c), "Command + K");
        let mac_ctrl = Modifiers { ctrl: true, ..Default::default() };
        let Recorded::Chord(c) = record(Key::K, mac_ctrl) else { panic!() };
        assert_eq!(format(&c), "Ctrl + K");
    }

    #[test]
    fn recorder_feeds_the_draft() {
        let mut keys = Shortcuts::default();
        let mut r = Recorder { recording: Some(Command::Rename), ..Default::default() };
        let cmd = Modifiers::COMMAND;
        r.feed(&mut keys, Key::R, Modifiers::NONE, 0.0);
        assert!(r.recording.is_some(), "a bare letter is ignored");
        r.feed(&mut keys, Key::F, cmd, 1.0);
        assert_eq!(r.conflict.map(|c| c.1), Some(Command::Find));
        assert!(r.recording.is_some() && keys.is_default(Command::Rename), "a conflict changes nothing");
        r.feed(&mut keys, Key::K, cmd, 2.0);
        assert!(!r.recording.is_some() && r.conflict.is_none());
        assert_eq!(keys.binding(Command::Rename).map(|c| format(&c)).as_deref(), Some("Command + K"));

        r.recording = Some(Command::Rename);
        r.feed(&mut keys, Key::Escape, Modifiers::NONE, 3.0);
        assert!(!r.recording.is_some());
        assert_eq!(keys.binding(Command::Rename).map(|c| format(&c)).as_deref(), Some("Command + K"), "Escape keeps the chord");

        r.recording = Some(Command::Rename);
        r.feed(&mut keys, Key::Backspace, Modifiers::NONE, 4.0);
        assert_eq!(keys.binding(Command::Rename), None, "Backspace clears");
    }

    #[test]
    fn saved_form_is_a_plain_map() {
        let mut keys = Shortcuts::default();
        keys.set(Command::Rename, parse("Command + K")).unwrap();
        let json = serde_json::to_string(&keys).unwrap();
        assert_eq!(json, r#"{"edit.rename":"Command + K"}"#);
        let back: Shortcuts = serde_json::from_str(&json).unwrap();
        assert_eq!(back, keys);
    }

    #[test]
    fn player_keys_only_clash_with_the_viewer_key() {
        let keys = Shortcuts::default();
        for s in TABLE.iter().filter(|s| s.group == PLAYER) {
            let c = s.default.unwrap();
            let global = keys.resolve(c.logical_key, c.modifiers);
            if s.cmd == Command::Player(PlayerKey::Toggle) {
                assert_eq!(global, Some(Command::Viewer));
            } else if let Command::Player(PlayerKey::Skip(fwd)) = s.cmd {
                // ⌘← and ⌘→ skip in a playing video and are Back and
                // Forward elsewhere, as in Eagle.
                assert_eq!(global, Some(if fwd { Command::Forward } else { Command::Back }));
            } else if let Command::Player(PlayerKey::Volume(up)) = s.cmd {
                // ⌘↑ and ⌘↓ change the volume in a playing video and
                // select the first or last item elsewhere.
                assert_eq!(global, Some(if up { Command::SelectFirst } else { Command::SelectLast }));
            } else {
                // A global chord that only matches loosely (⌘[ for [) is no clash.
                assert!(global.is_none_or(|g| !keys.chords(g).iter().any(|x| same(*x, Some(c)))), "{} clashes with {global:?}", s.id);
            }
        }
        // ⌘[ moves the folder; the player's [ and ] are bare.
        assert_eq!(keys.resolve(Key::OpenBracket, Modifiers::COMMAND), Some(Command::FolderDown));
        assert_eq!(keys.player(&[Key::OpenBracket], Modifiers::COMMAND), None);
    }

    #[test]
    fn player_conflicts_stay_in_the_group() {
        let mut keys = Shortcuts::default();
        // ⌘F is Find, but the player may use it.
        assert_eq!(keys.set(Command::Player(PlayerKey::Toggle), parse("Command + F")), Ok(()));
        assert_eq!(keys.player(&[Key::F], Modifiers::COMMAND), Some(PlayerKey::Toggle));
        assert_eq!(keys.resolve(Key::F, Modifiers::COMMAND), Some(Command::Find));
        // ⌘↑ is the player's volume, so another player key can't take it,
        assert_eq!(keys.set(Command::Player(PlayerKey::AddComment), parse("Command + Up")), Err(Command::Player(PlayerKey::Volume(true))));
        // …and ] is the player's next frame, which a global command may take.
        assert_eq!(keys.set(Command::Player(PlayerKey::AddComment), parse("]")), Err(Command::Player(PlayerKey::Frames(1))));
        assert_eq!(keys.set(Command::Rename, parse("]")), Ok(()));
        assert_eq!(keys.0.get("player.playAndPause").map(String::as_str), Some("Command + F"));
    }

    #[test]
    fn eagle_player_keys() {
        let keys = Shortcuts::default();
        let p = |k: &[Key], m: Modifiers| keys.player(k, m);
        let none = Modifiers::NONE;
        let shift = Modifiers::SHIFT;
        let cmd = Modifiers { command: true, mac_cmd: true, ..Default::default() };
        assert_eq!(p(&[Key::Space], none), Some(PlayerKey::Toggle));
        assert_eq!(p(&[Key::Space], cmd), None);
        assert_eq!(p(&[Key::N], none), Some(PlayerKey::AddComment));
        assert_eq!(p(&[Key::N], shift), None);
        assert_eq!(p(&[Key::OpenBracket], none), Some(PlayerKey::Frames(-1)));
        assert_eq!(p(&[Key::CloseBracket], none), Some(PlayerKey::Frames(1)));
        assert_eq!(p(&[Key::OpenCurlyBracket, Key::OpenBracket], shift), Some(PlayerKey::Frames(-10)));
        assert_eq!(p(&[Key::CloseBracket], shift), Some(PlayerKey::Frames(10)));
        assert_eq!(p(&[Key::Period], shift), Some(PlayerKey::Speed(true)));
        assert_eq!(p(&[Key::Comma], shift), Some(PlayerKey::Speed(false)));
        assert_eq!(p(&[Key::Period], none), None);
        assert_eq!(p(&[Key::ArrowUp], cmd), Some(PlayerKey::Volume(true)));
        assert_eq!(p(&[Key::ArrowDown], cmd), Some(PlayerKey::Volume(false)));
        assert_eq!(p(&[Key::ArrowRight], cmd), Some(PlayerKey::Skip(true)));
        assert_eq!(p(&[Key::ArrowLeft], cmd), Some(PlayerKey::Skip(false)));
        // Plain arrows stay with the viewer (previous and next item).
        assert_eq!(p(&[Key::ArrowLeft], none), None);
        assert_eq!(p(&[Key::A], none), None);
        // Player keys never come out of the global resolver.
        assert_eq!(keys.resolve(Key::N, none), None);
    }
}
