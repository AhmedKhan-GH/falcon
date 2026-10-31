//! Rebindable keyboard shortcuts, after Eagle's `shortcuts.keybinds`
//! preference: a table of commands with default chords, user overrides
//! stored as Eagle-style accelerator strings ("Command + Shift + N"),
//! conflict checks and Reset.

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
    NewFolder,
    Copy,
    Paste,
    ToggleSidebar,
    ToggleInspector,
    ToggleFilter,
    LayoutGrid,
    LayoutJustified,
    LayoutWaterfall,
    LayoutList,
    Preferences,
    CollectWindow,
    SwitchLibrary,
    NextLibrary,
    PreviousLibrary,
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
    /// A second chord that also works while the binding is the default
    /// (Enter for the viewer, ⌘+ for zoom in).
    pub alias: Option<KeyboardShortcut>,
}

const fn k(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const NONE: Modifiers = Modifiers::NONE;
const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };
const CMD_ALT: Modifiers = Modifiers { alt: true, ..Modifiers::COMMAND };
const ALT: Modifiers = Modifiers::ALT;

/// Every rebindable command with its default. Arrow keys, Escape and the
/// held Z of the hover preview stay fixed (see `FIXED`).
pub const TABLE: &[Spec] = &[
    Spec { cmd: Command::Viewer, id: "view.viewer", group: "View", label: "Open or close the viewer", default: k(NONE, Key::Space), alias: k(NONE, Key::Enter) },
    Spec { cmd: Command::ZoomIn, id: "view.zoom.in", group: "View", label: "Zoom in", default: k(CMD, Key::Equals), alias: k(CMD, Key::Plus) },
    Spec { cmd: Command::ZoomOut, id: "view.zoom.out", group: "View", label: "Zoom out", default: k(CMD, Key::Minus), alias: None },
    Spec { cmd: Command::ToggleSidebar, id: "view.toggle.sidebar", group: "View", label: "Show or hide the sidebar", default: k(CMD_ALT, Key::Num1), alias: None },
    Spec { cmd: Command::ToggleInspector, id: "view.toggle.inspector", group: "View", label: "Show or hide the inspector", default: k(CMD_ALT, Key::Num2), alias: None },
    Spec { cmd: Command::LayoutGrid, id: "view.layout.grid", group: "View", label: "Grid layout", default: k(ALT, Key::Num1), alias: None },
    Spec { cmd: Command::LayoutJustified, id: "view.layout.justified", group: "View", label: "Justified layout", default: k(ALT, Key::Num2), alias: None },
    Spec { cmd: Command::LayoutWaterfall, id: "view.layout.waterfall", group: "View", label: "Waterfall layout", default: k(ALT, Key::Num3), alias: None },
    Spec { cmd: Command::LayoutList, id: "view.layout.list", group: "View", label: "List layout", default: k(ALT, Key::Num4), alias: None },
    Spec { cmd: Command::Back, id: "go.back", group: "Navigate", label: "Back", default: k(CMD, Key::OpenBracket), alias: None },
    Spec { cmd: Command::Forward, id: "go.forward", group: "Navigate", label: "Forward", default: k(CMD, Key::CloseBracket), alias: None },
    Spec { cmd: Command::Find, id: "find.search.current", group: "Find", label: "Focus search", default: k(CMD, Key::F), alias: None },
    Spec { cmd: Command::ToggleFilter, id: "find.filter.toggle", group: "Find", label: "Show or hide the filter bar", default: k(CMD_SHIFT, Key::F), alias: None },
    Spec { cmd: Command::SelectAll, id: "edit.select.all", group: "Edit", label: "Select all", default: k(CMD, Key::A), alias: None },
    Spec { cmd: Command::Rename, id: "edit.rename", group: "Edit", label: "Rename the selected item", default: k(NONE, Key::F2), alias: None },
    Spec { cmd: Command::Copy, id: "edit.copy", group: "Edit", label: "Copy the selected files", default: k(CMD, Key::C), alias: None },
    Spec { cmd: Command::Paste, id: "edit.paste", group: "Edit", label: "Paste files or an image", default: k(CMD, Key::V), alias: None },
    Spec { cmd: Command::Trash, id: "edit.remove.trash", group: "Edit", label: "Move to the Trash", default: k(CMD, Key::Backspace), alias: None },
    Spec { cmd: Command::NewFolder, id: "file.create.folder", group: "File", label: "New folder", default: k(CMD_SHIFT, Key::N), alias: None },
    Spec { cmd: Command::CollectWindow, id: "file.collect.window", group: "File", label: "Show or hide the collect window", default: k(CMD_SHIFT, Key::K), alias: None },
    Spec { cmd: Command::SwitchLibrary, id: "library.switch", group: "Library", label: "Open the library switcher", default: k(CMD, Key::L), alias: None },
    // falcon's own: Eagle shows one library at a time.
    Spec { cmd: Command::NextLibrary, id: "library.next", group: "Library", label: "Next open library", default: k(CMD_SHIFT, Key::CloseBracket), alias: k(CMD_SHIFT, Key::CloseCurlyBracket) },
    Spec { cmd: Command::PreviousLibrary, id: "library.previous", group: "Library", label: "Previous open library", default: k(CMD_SHIFT, Key::OpenBracket), alias: k(CMD_SHIFT, Key::OpenCurlyBracket) },
    Spec { cmd: Command::Preferences, id: "app.preferences", group: "App", label: "Preferences", default: k(CMD, Key::Comma), alias: None },
];

/// Keys that cannot be rebound, listed in the Shortcuts section.
pub const FIXED: &[(&str, &str)] = &[
    ("← → ↑ ↓", "Move the selection, or step through the viewer"),
    ("Esc", "Close the viewer, or clear the selection"),
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
            None => s.default,
        }
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
            v.extend(s.alias);
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

    /// Another command whose chords include `chord`.
    pub fn conflict(&self, cmd: Command, chord: KeyboardShortcut) -> Option<Command> {
        TABLE.iter().map(|s| s.cmd).filter(|&c| c != cmd).find(|&c| self.chords(c).iter().any(|x| same(*x, Some(chord))))
    }

    /// The command a key press triggers. When several match (⌘F and ⌘⇧F
    /// for ⌘⇧F), the chord with the most modifiers wins.
    pub fn resolve(&self, key: Key, pressed: Modifiers) -> Option<Command> {
        let mut best: Option<(Command, u8)> = None;
        for s in TABLE {
            for c in self.chords(s.cmd) {
                if c.logical_key == key && pressed.matches_logically(c.modifiers) {
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
            p.text(row.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, s.label, font(13.0), t::TEXT);
            let button = egui::Rect::from_min_size(egui::pos2(row.right() - 150.0, row.top() + 3.0), vec2(150.0, 24.0));
            let resp = ui.interact(button, ui.scope_id().with(s.id), Sense::click());
            let active = self.recording == Some(s.cmd);
            let fill = if resp.hovered() || active { t::BUTTON_HOVER } else { t::BUTTON };
            let stroke = if active { Stroke::new(1.0, t::PRIMARY) } else { Stroke::NONE };
            p.rect(button, CornerRadius::same(6), fill, stroke, egui::StrokeKind::Inside);
            let (text, color) = match (active, keys.binding(s.cmd)) {
                (true, _) => ("Type a shortcut".to_string(), t::TEXT_MUTED),
                (false, Some(c)) => (display(ui.ctx(), &c), t::TEXT),
                (false, None) => ("None".to_string(), t::TEXT_DISABLED),
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
                    p.rect_filled(r, CornerRadius::same(5), t::BUTTON_HOVER);
                }
                p.text(r.center(), Align2::CENTER_CENTER, crate::icons::ARROW_COUNTER_CLOCKWISE, font(13.0), t::TEXT_MUTED);
                if reset.clicked() {
                    keys.reset(s.cmd);
                    // The default may now clash with a chord given to
                    // another command; that command gives way.
                    if let Some(c) = keys.binding(s.cmd) {
                        for other in TABLE.iter().filter(|o| o.cmd != s.cmd) {
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
            ui.painter().text(row.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, *what, font(13.0), t::TEXT_MUTED);
            ui.painter().text(row.right_center() - vec2(75.0, 0.0), Align2::CENTER_CENTER, *k, font(12.5), t::TEXT_MUTED);
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
        let mut seen = HashSet::new();
        for s in TABLE {
            for c in s.default.iter().chain(s.alias.iter()) {
                assert!(seen.insert(format(c)), "{} reuses {}", s.id, format(c));
            }
        }
        let ids: HashSet<_> = TABLE.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), TABLE.len(), "ids are unique");
        let cmds: HashSet<_> = TABLE.iter().map(|s| s.cmd).collect();
        assert_eq!(cmds.len(), TABLE.len(), "each command appears once");
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
            for c in s.default.iter().chain(s.alias.iter()) {
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
    fn library_switch_keys() {
        let keys = Shortcuts::default();
        assert_eq!(keys.resolve(Key::L, Modifiers::COMMAND), Some(Command::SwitchLibrary));
        // ⇧ may or may not turn the bracket into a brace before it reaches us.
        for (key, cmd) in [(Key::CloseBracket, Command::NextLibrary), (Key::CloseCurlyBracket, Command::NextLibrary), (Key::OpenBracket, Command::PreviousLibrary), (Key::OpenCurlyBracket, Command::PreviousLibrary)] {
            assert_eq!(keys.resolve(key, CMD_SHIFT), Some(cmd), "{key:?}");
        }
        assert_eq!(keys.resolve(Key::OpenBracket, Modifiers::COMMAND), Some(Command::Back));
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

        let r = parse("Command + R").unwrap();
        assert_eq!(keys.set(Command::Rename, Some(r)), Ok(()));
        assert_eq!(keys.0.get("edit.rename").map(String::as_str), Some("Command + R"));
        assert_eq!(keys.resolve(Key::R, Modifiers::COMMAND), Some(Command::Rename));
        assert_eq!(keys.resolve(Key::F2, Modifiers::NONE), None);

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
        r.feed(&mut keys, Key::R, cmd, 2.0);
        assert!(!r.recording.is_some() && r.conflict.is_none());
        assert_eq!(keys.binding(Command::Rename).map(|c| format(&c)).as_deref(), Some("Command + R"));

        r.recording = Some(Command::Rename);
        r.feed(&mut keys, Key::Escape, Modifiers::NONE, 3.0);
        assert!(!r.recording.is_some());
        assert_eq!(keys.binding(Command::Rename).map(|c| format(&c)).as_deref(), Some("Command + R"), "Escape keeps the chord");

        r.recording = Some(Command::Rename);
        r.feed(&mut keys, Key::Backspace, Modifiers::NONE, 4.0);
        assert_eq!(keys.binding(Command::Rename), None, "Backspace clears");
    }

    #[test]
    fn saved_form_is_a_plain_map() {
        let mut keys = Shortcuts::default();
        keys.set(Command::Rename, parse("Command + R")).unwrap();
        let json = serde_json::to_string(&keys).unwrap();
        assert_eq!(json, r#"{"edit.rename":"Command + R"}"#);
        let back: Shortcuts = serde_json::from_str(&json).unwrap();
        assert_eq!(back, keys);
    }
}
