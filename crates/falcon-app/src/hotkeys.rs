//! Global shortcuts, after Eagle's `shortcuts.global.*`: keys that act
//! from any app. The chords live in the shortcut table's Global group
//! (`shortcuts::GLOBAL`); this module registers them with the system
//! through Carbon's `RegisterEventHotKey` (still the only system API for
//! app-wide hot keys) and queues the presses for the app's next frame.
//!
//! The capture keys are registered only while Preferences ▸ Screenshot ▸
//! "Use the screenshot keys" is on, which is off by default (Eagle has it
//! on, but Eagle itself usually holds ⌥⌘E on the same Mac); the area key
//! keeps Eagle's ⌥⌘E, so turning the switch on gives Eagle's layout. Show
//! falcon and Show falcon and focus the search are registered once bound
//! (they have no default). A chord another app holds is refused by the
//! system; Preferences shows it on the key's row (`Recorder::refused`)
//! and tries again when the switch or the chord changes.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

use crate::shortcuts::{Command, GLOBAL, Shortcuts, TABLE};

/// The chords to register: every bound Global row, the captures only when
/// `captures` is on. Chords the system can't take (no key code) are left
/// out.
pub fn wanted(keys: &Shortcuts, captures: bool) -> Vec<(Command, KeyboardShortcut)> {
    TABLE
        .iter()
        .filter(|s| s.group == GLOBAL)
        .filter(|s| captures || !matches!(s.cmd, Command::Capture(_)))
        .filter_map(|s| keys.binding(s.cmd).map(|c| (s.cmd, c)))
        .filter(|(_, c)| key_code(c.logical_key).is_some())
        .collect()
}

/// Carbon's modifier bits: `cmdKey`, `shiftKey`, `optionKey` and
/// `controlKey`.
pub fn carbon_modifiers(m: Modifiers) -> u32 {
    let mut bits = 0;
    if m.command || m.mac_cmd {
        bits |= 1 << 8;
    }
    if m.shift {
        bits |= 1 << 9;
    }
    if m.alt {
        bits |= 1 << 11;
    }
    if m.ctrl {
        bits |= 1 << 12;
    }
    bits
}

/// The virtual key code (`kVK_*`, the ANSI layout's positions) for a key.
pub fn key_code(key: Key) -> Option<u32> {
    use Key::*;
    Some(match key {
        A => 0x00,
        S => 0x01,
        D => 0x02,
        F => 0x03,
        H => 0x04,
        G => 0x05,
        Z => 0x06,
        X => 0x07,
        C => 0x08,
        V => 0x09,
        B => 0x0B,
        Q => 0x0C,
        W => 0x0D,
        E => 0x0E,
        R => 0x0F,
        Y => 0x10,
        T => 0x11,
        Num1 => 0x12,
        Num2 => 0x13,
        Num3 => 0x14,
        Num4 => 0x15,
        Num6 => 0x16,
        Num5 => 0x17,
        Equals => 0x18,
        Num9 => 0x19,
        Num7 => 0x1A,
        Minus => 0x1B,
        Num8 => 0x1C,
        Num0 => 0x1D,
        CloseBracket => 0x1E,
        O => 0x1F,
        U => 0x20,
        OpenBracket => 0x21,
        I => 0x22,
        P => 0x23,
        Enter => 0x24,
        L => 0x25,
        J => 0x26,
        Quote => 0x27,
        K => 0x28,
        Semicolon => 0x29,
        Backslash => 0x2A,
        Comma => 0x2B,
        Slash => 0x2C,
        N => 0x2D,
        M => 0x2E,
        Period => 0x2F,
        Tab => 0x30,
        Space => 0x31,
        Backtick => 0x32,
        Backspace => 0x33,
        Escape => 0x35,
        F17 => 0x40,
        F18 => 0x4F,
        F19 => 0x50,
        F20 => 0x5A,
        F5 => 0x60,
        F6 => 0x61,
        F7 => 0x62,
        F3 => 0x63,
        F8 => 0x64,
        F9 => 0x65,
        F11 => 0x67,
        F13 => 0x69,
        F16 => 0x6A,
        F14 => 0x6B,
        F10 => 0x6D,
        F12 => 0x6F,
        F15 => 0x71,
        Home => 0x73,
        PageUp => 0x74,
        Delete => 0x75,
        F4 => 0x76,
        End => 0x77,
        F2 => 0x78,
        PageDown => 0x79,
        F1 => 0x7A,
        ArrowLeft => 0x7B,
        ArrowRight => 0x7C,
        ArrowDown => 0x7D,
        ArrowUp => 0x7E,
        _ => return None,
    })
}

/// What registering a set of chords did.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Outcome {
    /// Chords the system refused, usually because another app holds them.
    pub refused: Vec<(Command, KeyboardShortcut)>,
}

/// Registers a set of chords in place of the last one and returns the
/// ones the system refused. The app uses Carbon (`platform::register`);
/// tests inject their own.
pub type Registrar = Box<dyn FnMut(&eframe::egui::Context, &[(Command, KeyboardShortcut)]) -> Vec<(Command, KeyboardShortcut)>>;

/// The registered keys. Keeps what it registered and changes the system's
/// set only when the wanted set changes (a switched-on capture key, a new
/// chord), which is also when a refused key is tried again.
#[derive(Default)]
pub struct HotKeys {
    current: Vec<(Command, KeyboardShortcut)>,
    /// The chords the last registration refused, until the next one.
    refused: Vec<(Command, KeyboardShortcut)>,
    registrar: Option<Registrar>,
}

impl HotKeys {
    /// Keys registered through `registrar` instead of the system.
    #[cfg(test)]
    pub fn with_registrar(registrar: Registrar) -> Self {
        HotKeys { registrar: Some(registrar), ..Default::default() }
    }

    /// Registers `wanted` in place of the current set when they differ;
    /// returns what happened then, or `None` when nothing changed. Call on
    /// the main thread.
    pub fn sync(&mut self, ctx: &eframe::egui::Context, wanted: Vec<(Command, KeyboardShortcut)>) -> Option<Outcome> {
        if wanted == self.current {
            return None;
        }
        let refused = match &mut self.registrar {
            Some(r) => r(ctx, &wanted),
            None => platform::register(ctx, &wanted),
        };
        self.current = wanted;
        self.refused.clone_from(&refused);
        Some(Outcome { refused })
    }

    /// The chords another app holds, from the last registration.
    pub fn refused(&self) -> &[(Command, KeyboardShortcut)] {
        &self.refused
    }
}

/// The global keys pressed since the last call, oldest first.
pub fn take_pressed() -> Vec<Command> {
    platform::take()
}

#[cfg(target_os = "macos")]
#[allow(non_upper_case_globals)]
mod platform {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::{Mutex, OnceLock};

    use eframe::egui::KeyboardShortcut;

    use super::{carbon_modifiers, key_code};
    use crate::shortcuts::Command;

    type OSStatus = i32;
    type EventTargetRef = *mut c_void;
    type EventRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;
    type EventHandlerRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type Handler = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(target: EventTargetRef, handler: Handler, count: usize, types: *const EventTypeSpec, user_data: *mut c_void, out: *mut EventHandlerRef) -> OSStatus;
        fn RegisterEventHotKey(code: u32, modifiers: u32, id: EventHotKeyID, target: EventTargetRef, options: u32, out: *mut EventHotKeyRef) -> OSStatus;
        fn UnregisterEventHotKey(key: EventHotKeyRef) -> OSStatus;
        fn GetEventParameter(event: EventRef, name: u32, desired: u32, actual: *mut u32, size: usize, actual_size: *mut usize, data: *mut c_void) -> OSStatus;
    }

    const fn four(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    const kEventClassKeyboard: u32 = four(b"keyb");
    const kEventHotKeyPressed: u32 = 5;
    const kEventParamDirectObject: u32 = four(b"----");
    const typeEventHotKeyID: u32 = four(b"hkid");
    /// falcon's hot key signature.
    const SIGNATURE: u32 = four(b"FLCN");

    static PRESSED: Mutex<Vec<Command>> = Mutex::new(Vec::new());
    static REPAINT: OnceLock<eframe::egui::Context> = OnceLock::new();
    /// The commands of the registered keys, by hot key id.
    static IDS: Mutex<Vec<Command>> = Mutex::new(Vec::new());

    thread_local! {
        static INSTALLED: RefCell<bool> = const { RefCell::new(false) };
        static REFS: RefCell<Vec<EventHotKeyRef>> = const { RefCell::new(Vec::new()) };
    }

    extern "C" fn on_hot_key(_call: EventHandlerCallRef, event: EventRef, _data: *mut c_void) -> OSStatus {
        let mut id = EventHotKeyID::default();
        // SAFETY: the event is a hot key event (the handler is installed
        // for those only) and `id` has the size given.
        let status = unsafe {
            GetEventParameter(event, kEventParamDirectObject, typeEventHotKeyID, std::ptr::null_mut(), size_of::<EventHotKeyID>(), std::ptr::null_mut(), (&raw mut id).cast())
        };
        if status == 0 && id.signature == SIGNATURE {
            let cmd = IDS.lock().unwrap_or_else(|p| p.into_inner()).get(id.id as usize).copied();
            if let Some(cmd) = cmd {
                PRESSED.lock().unwrap_or_else(|p| p.into_inner()).push(cmd);
                if let Some(ctx) = REPAINT.get() {
                    ctx.request_repaint();
                }
            }
        }
        0
    }

    pub fn register(ctx: &eframe::egui::Context, wanted: &[(Command, KeyboardShortcut)]) -> Vec<(Command, KeyboardShortcut)> {
        let _ = REPAINT.set(ctx.clone());
        // SAFETY: Carbon calls run on the main thread (the app's frame).
        let target = unsafe { GetApplicationEventTarget() };
        INSTALLED.with(|done| {
            if !*done.borrow() {
                let spec = EventTypeSpec { event_class: kEventClassKeyboard, event_kind: kEventHotKeyPressed };
                let mut handler: EventHandlerRef = std::ptr::null_mut();
                // SAFETY: `spec` outlives the call; the handler is a plain
                // function that stays installed for the life of the app.
                let status = unsafe { InstallEventHandler(target, on_hot_key, 1, &spec, std::ptr::null_mut(), &mut handler) };
                *done.borrow_mut() = status == 0;
            }
        });
        REFS.with(|refs| {
            for r in refs.borrow_mut().drain(..) {
                // SAFETY: each ref came from RegisterEventHotKey and is
                // unregistered once.
                unsafe { UnregisterEventHotKey(r) };
            }
        });
        let mut ids = Vec::new();
        let mut refused = Vec::new();
        for &(cmd, chord) in wanted {
            let Some(code) = key_code(chord.logical_key) else { continue };
            let id = EventHotKeyID { signature: SIGNATURE, id: ids.len() as u32 };
            let mut r: EventHotKeyRef = std::ptr::null_mut();
            // SAFETY: plain values and an out pointer to a local.
            let status = unsafe { RegisterEventHotKey(code, carbon_modifiers(chord.modifiers), id, target, 0, &mut r) };
            ids.push(cmd);
            if status == 0 && !r.is_null() {
                REFS.with(|refs| refs.borrow_mut().push(r));
            } else {
                refused.push((cmd, chord));
            }
        }
        *IDS.lock().unwrap_or_else(|p| p.into_inner()) = ids;
        refused
    }

    pub fn take() -> Vec<Command> {
        std::mem::take(&mut *PRESSED.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use eframe::egui::KeyboardShortcut;

    use crate::shortcuts::Command;

    pub fn register(_ctx: &eframe::egui::Context, _wanted: &[(Command, KeyboardShortcut)]) -> Vec<(Command, KeyboardShortcut)> {
        Vec::new()
    }

    pub fn take() -> Vec<Command> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Mode;

    #[test]
    fn eagles_defaults_register_only_the_area_capture() {
        let keys = Shortcuts::default();
        let w = wanted(&keys, true);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].0, Command::Capture(Mode::Area));
        assert_eq!(crate::shortcuts::format(&w[0].1), "Command + Alt + E");
        assert!(wanted(&keys, false).is_empty(), "the capture switch turns the capture keys off");
    }

    #[test]
    fn show_keys_ignore_the_capture_switch() {
        let mut keys = Shortcuts::default();
        keys.set(Command::ShowFalcon, crate::shortcuts::parse("Ctrl + Alt + Space")).unwrap();
        let w = wanted(&keys, false);
        assert_eq!(w.iter().map(|x| x.0).collect::<Vec<_>>(), [Command::ShowFalcon]);
    }

    #[test]
    fn carbon_codes() {
        assert_eq!(key_code(Key::E), Some(0x0E));
        assert_eq!(key_code(Key::Space), Some(0x31));
        assert_eq!(key_code(Key::F1), Some(0x7A));
        assert_eq!(carbon_modifiers(Modifiers { alt: true, ..Modifiers::COMMAND }), 256 | 2048);
        assert_eq!(carbon_modifiers(Modifiers::NONE), 0);
    }

    #[test]
    fn global_keys_never_reach_the_window() {
        let keys = Shortcuts::default();
        assert_ne!(keys.resolve(Key::E, Modifiers { alt: true, ..Modifiers::COMMAND }), Some(Command::Capture(Mode::Area)));
    }

    #[test]
    fn letters_and_digits_have_distinct_key_codes() {
        use std::collections::HashSet;
        let keys = [
            Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M,
            Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
            Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9,
        ];
        let codes: Vec<_> = keys.iter().map(|k| key_code(*k).unwrap_or_else(|| panic!("{k:?} has no code"))).collect();
        assert_eq!(codes.iter().collect::<HashSet<_>>().len(), keys.len());
    }

    #[test]
    fn each_modifier_has_its_carbon_bit() {
        let none = Modifiers::NONE;
        assert_eq!(carbon_modifiers(Modifiers { command: true, ..none }), 256);
        assert_eq!(carbon_modifiers(Modifiers { shift: true, ..none }), 512);
        assert_eq!(carbon_modifiers(Modifiers { alt: true, ..none }), 2048);
        assert_eq!(carbon_modifiers(Modifiers { ctrl: true, ..none }), 4096);
        assert_eq!(carbon_modifiers(Modifiers { mac_cmd: true, ..none }), 256);
    }

    #[test]
    fn wanted_drops_keys_without_a_code_and_keeps_table_order() {
        let odd = Key::F21;
        assert_eq!(key_code(odd), None);
        let mut keys = Shortcuts::default();
        keys.set(Command::ShowSearch, Some(KeyboardShortcut::new(Modifiers::COMMAND, odd))).unwrap();
        assert!(keys.binding(Command::ShowSearch).is_some(), "the odd key is bound");
        assert!(wanted(&keys, true).iter().all(|w| w.0 != Command::ShowSearch));
        // Bound out of table order, they still come back in the table's.
        keys.set(Command::Capture(Mode::Window), crate::shortcuts::parse("Ctrl + Alt + W")).unwrap();
        keys.set(Command::ShowFalcon, crate::shortcuts::parse("Ctrl + Alt + Space")).unwrap();
        let order: Vec<_> = wanted(&keys, true).iter().map(|w| w.0).collect();
        assert_eq!(order, [Command::ShowFalcon, Command::Capture(Mode::Area), Command::Capture(Mode::Window)]);
    }

    /// A registrar that refuses `taken` and records each call.
    fn fake(taken: Vec<KeyboardShortcut>, calls: std::rc::Rc<std::cell::RefCell<Vec<Vec<Command>>>>) -> HotKeys {
        HotKeys::with_registrar(Box::new(move |_ctx, wanted| {
            calls.borrow_mut().push(wanted.iter().map(|w| w.0).collect());
            wanted.iter().filter(|w| taken.contains(&w.1)).copied().collect()
        }))
    }

    #[test]
    fn a_refused_key_stays_refused_until_the_set_changes() {
        let ctx = eframe::egui::Context::default();
        let calls = std::rc::Rc::default();
        let area = crate::shortcuts::parse("Command + Alt + E").unwrap();
        let mut hk = fake(vec![area], std::rc::Rc::clone(&calls));
        let keys = Shortcuts::default();
        // Off by default: nothing to register, nothing refused.
        assert_eq!(hk.sync(&ctx, wanted(&keys, false)), None);
        assert!(hk.refused().is_empty());
        // Switched on: Eagle holds ⌥⌘E.
        let out = hk.sync(&ctx, wanted(&keys, true)).unwrap();
        assert_eq!(out.refused, [(Command::Capture(Mode::Area), area)]);
        assert_eq!(hk.refused(), out.refused.as_slice());
        // The same set again is not registered again, and stays refused.
        assert_eq!(hk.sync(&ctx, wanted(&keys, true)), None);
        assert_eq!(hk.refused().len(), 1);
        assert_eq!(calls.borrow().len(), 1);
        // A new chord is tried at once and succeeds.
        let mut moved = keys.clone();
        moved.set(Command::Capture(Mode::Area), crate::shortcuts::parse("Ctrl + Alt + E")).unwrap();
        let out = hk.sync(&ctx, wanted(&moved, true)).unwrap();
        assert!(out.refused.is_empty() && hk.refused().is_empty());
        // Switching off and on again retries the old chord.
        hk.sync(&ctx, wanted(&keys, false));
        assert!(!hk.sync(&ctx, wanted(&keys, true)).unwrap().refused.is_empty());
        assert_eq!(calls.borrow().len(), 4);
    }
}
