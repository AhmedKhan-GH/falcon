//! Commands: the dispatcher shared by the keyboard and the menu bar
//! (`run_command`), the keys read each frame (`keyboard`), the menu bar's
//! state and clicks, and the commands of the Find menu (`find.rs`) and the
//! item commands that need more than one action (`items.rs`).

mod find;
pub(crate) mod items;

use std::path::PathBuf;

use eframe::egui::{self, Key, Modifiers};
use falcon_library::model::{FolderOrder, Item};
use falcon_library::{Library, thumbnail};

use super::{
    Action, App, BULK_CONFIRM, Confirm, SmartDraft, Switch, TILE_MAX, TILE_MIN, image_edit, lock,
};
use super::navigation::fastrand_seed;
use crate::locale::{tr, tr_n};
use crate::logic::{self, Layout, Scope};

/// The input events a key press of `chord` makes, as egui-winit sends
/// them: ⌘C, ⌘X and ⌘V become clipboard events, and a bare printable key
/// also types its character.
pub(crate) fn key_events(chord: &egui::KeyboardShortcut) -> Vec<egui::Event> {
    let m = chord.modifiers;
    let modifiers = Modifiers { mac_cmd: m.command && cfg!(target_os = "macos"), ..m };
    let command_only = m.command && !m.shift && !m.alt && !m.ctrl;
    match chord.logical_key {
        Key::C if command_only => return vec![egui::Event::Copy],
        Key::X if command_only => return vec![egui::Event::Cut],
        Key::V if command_only => {
            let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
            return vec![egui::Event::Paste(text)];
        }
        _ => {}
    }
    let mut events = vec![egui::Event::Key { key: chord.logical_key, physical_key: None, pressed: true, repeat: false, modifiers }];
    let name = if chord.logical_key == Key::Space { " " } else { chord.logical_key.symbol_or_name() };
    if !m.command && !m.ctrl && !m.alt && name.chars().count() == 1 {
        let text = if m.shift { name.to_uppercase() } else { name.to_lowercase() };
        events.push(egui::Event::Text(text));
    }
    events
}

/// Whether some of `ids` can be flipped, some rotated, and (one item)
/// cropped.
pub(crate) fn editable_now(ids: &[String], lib: &Library) -> [bool; 3] {
    use falcon_library::transform::{can_crop, can_flip, can_rotate};
    let ext = |id: &String| lib.item(id).map(|i| i.ext.as_str());
    [
        ids.iter().filter_map(ext).any(can_flip),
        ids.iter().filter_map(ext).any(can_rotate),
        ids.len() == 1 && ids.iter().filter_map(ext).any(can_crop),
    ]
}

/// The book in `docs/` next to the sources: the built one if there is
/// one, else its source folder. `None` when falcon runs elsewhere.
pub(crate) fn docs_path() -> Option<&'static std::path::Path> {
    static DOCS: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DOCS.get_or_init(|| {
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        [docs.join("book/index.html"), docs.join("src")].into_iter().find(|p| p.exists())
    })
    .as_deref()
}

/// The toast after a rating key or menu item.
pub(crate) fn rated_note(stars: u8) -> String {
    match stars {
        0 => tr("Removed the rating").into(),
        n => tr_n(n as usize, "Rated {n} star", "Rated {n} stars"),
    }
}

impl App {
    /// Key presses: rebindable commands go through the shortcut table
    /// (`shortcuts.rs`) to `run_command`, as menu clicks do; Tab, the
    /// viewer's plain zoom keys, the arrows and Esc are fixed.
    pub(crate) fn keyboard(&mut self, ctx: &egui::Context) {
        use crate::shortcuts::Command as C;
        let mut pressed = ctx.input(|i| self.shortcuts.pressed(i));
        // A click in the sidebar hands it the arrow keys; a click anywhere
        // else gives them back to the grid, as in Eagle.
        if let Some(pos) = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.press_origin()).flatten()) {
            let side = self.show_sidebar && self.imports.sidebar.is_some_and(|r| r.contains(pos));
            let tags = self.ws.scope == Scope::AllTags && self.ws.org.tag_sidebar.is_some_and(|r| r.contains(pos));
            self.zone = if side {
                logic::Zone::Sidebar
            } else if tags {
                logic::Zone::Tags
            } else {
                logic::Zone::Content
            };
        }
        if pressed.contains(&C::Preferences) && !self.prefs_window.open {
            self.open_preferences();
        }
        // The duplicate dialog and Go to Folder take Return, Esc and the
        // arrows for themselves.
        if self.prefs_window.open || self.about.open || self.ws.dup_dialog.is_some() || self.quick.is_open() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        pressed.retain(|c| *c != C::Preferences);
        // The rotate and flip keys act on the viewed item only, as Eagle's
        // handlers act on its current item; the menu still edits a grid
        // selection.
        if self.ws.viewer.is_none() {
            pressed.retain(|c| !matches!(c, C::RotateLeft | C::FlipHorizontal));
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Z)) {
            self.ws.undo_request = Some(!ctx.input(|i| i.modifiers.shift));
        }
        // Tab is Eagle's fixed key for both panels.
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) && !pressed.contains(&C::ToggleAll) {
            pressed.push(C::ToggleAll);
        }
        let viewer = self.ws.viewer.is_some();
        // The text viewer has no plain zoom keys, and its digits rate the
        // item, as in Eagle's text editor (only reached while neither its
        // text nor its title has focus).
        let text = viewer && self.ws.text.as_ref().is_some_and(|v| self.ws.viewer.as_deref() == Some(v.id.as_str()));
        // There F5 reads the file again (as in Eagle's editor) instead of
        // entering presentation mode.
        if text && ctx.input(|i| i.key_pressed(Key::F5)) {
            pressed.retain(|c| *c != C::Presentation);
        }
        // The viewer's fixed zoom keys: plain + and −, and ` for fit to the
        // window edge. The digits rate the item in every viewer, as in Eagle;
        // zoom to fit and actual size are the table's ⌘9 and ⌘0 (with ⌘= and
        // ⌘−), through `run_command`.
        if viewer && !text {
            use logic::ViewZoom as Z;
            self.ws.view.zoom_key = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers: m, .. } if !m.command && !m.ctrl && !m.alt => match key {
                        Key::Plus | Key::Equals => Some(Z::In),
                        Key::Minus => Some(Z::Out),
                        Key::Backtick => Some(Z::Edge),
                        _ => None,
                    },
                    _ => None,
                })
            });
        }
        // In the viewer, ⌫ (Back) leaves it, as in Eagle; the arrow chords
        // of Back do nothing there.
        if viewer && ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
            pressed.retain(|c| *c != C::Back);
        }
        // With audio in the viewer, Space plays and pauses through the
        // player keys (`video_keys`), as in Eagle; it never closes the viewer.
        let audio = viewer && self.audio.loaded() == self.ws.viewer.as_deref() && ctx.input(|i| i.key_pressed(Key::Space));
        if audio {
            pressed.retain(|c| *c != C::Viewer);
        }
        // ⇧Space pages up, as in Eagle; the table would read it as Space
        // (the viewer key), since a chord without ⇧ also matches with it.
        if !viewer && ctx.input(|i| i.modifiers.shift_only() && i.key_pressed(Key::Space)) {
            pressed.retain(|c| *c != C::Viewer);
            pressed.push(C::Scroll(crate::shortcuts::Scroll::PageUp));
        }
        // These take the whole key press: the fixed keys wait a frame.
        let done = audio || pressed.iter().any(|c| matches!(c, C::Paste | C::OpenDefault | C::RevealInFinder | C::RevealAlt | C::Viewer) || (*c == C::Back && viewer));
        for cmd in pressed {
            self.run_command(ctx, cmd);
        }
        if done {
            return;
        }
        // With ⌘ or ⌥ the arrows are commands (Back, Forward, first and
        // last item, parent folder), not selection moves. Bare W, A, S
        // and D are arrows too, as in Eagle, unless bound to a command.
        let wasd = |i: &egui::InputState, k: Key| i.modifiers.is_none() && i.key_pressed(k) && self.shortcuts.resolve(k, Modifiers::NONE).is_none();
        let (left, right, up, down, esc, shift) = ctx.input(|i| {
            let plain = !i.modifiers.command && !i.modifiers.alt;
            (
                plain && i.key_pressed(Key::ArrowLeft) || wasd(i, Key::A),
                plain && i.key_pressed(Key::ArrowRight) || wasd(i, Key::D),
                plain && i.key_pressed(Key::ArrowUp) || wasd(i, Key::W),
                plain && i.key_pressed(Key::ArrowDown) || wasd(i, Key::S),
                i.key_pressed(Key::Escape),
                i.modifiers.shift,
            )
        });
        if esc {
            if self.ws.picker.open {
                self.ws.picker.open = false;
            } else if self.modes.presenting {
                // Esc leaves presentation mode first, keeping the viewer, as in Eagle.
                self.leave_presentation(ctx);
            } else if self.ws.scope == Scope::AllTags && !self.ws.org.tag_sel.is_empty() {
                self.ws.org.tag_sel.clear();
            } else if self.ws.viewer.is_some() {
                self.ws.viewer = None;
            } else {
                self.ws.selected.clear();
            }
        }
        // On the All Tags page, as in Eagle: → in the sidebar moves to the
        // page's tag sidebar ("tags" zone), ← there moves back, and ↑ and ↓
        // there open the list above or below.
        if self.ws.scope == Scope::AllTags && self.ws.viewer.is_none() && !shift {
            if self.zone == logic::Zone::Tags {
                if left && self.show_sidebar {
                    self.zone = logic::Zone::Sidebar;
                }
                if (up || down)
                    && let Some(l) = self.ws.lib.clone()
                {
                    let next = crate::tag_view::step(&self.ws.org.tag_view, &lock(&l).metadata.tags_groups, down);
                    if let Some(v) = next {
                        self.open_tag_view(v);
                        self.ws.org.tag_reveal = true;
                    }
                }
                if left || right || up || down {
                    return;
                }
            } else if self.zone == logic::Zone::Sidebar && right {
                self.zone = logic::Zone::Tags;
                return;
            }
        }
        // The sidebar has the keys: ↑ and ↓ open the row above or below,
        // ← and → collapse or expand the open folder (or every picked one).
        if self.zone == logic::Zone::Sidebar && self.show_sidebar && self.ws.viewer.is_none() && !shift {
            if up || down {
                if let Some(i) = logic::sidebar_step(&self.side.rows, self.side.cursor, &self.ws.scope, down) {
                    let scope = self.side.rows[i].clone();
                    self.side.cursor = Some(i);
                    self.side.reveal = true;
                    self.go(scope);
                }
                return;
            }
            if left || right {
                let picked = if self.ws.folders.sel.is_multi() {
                    self.ws.folders.sel.ids.clone()
                } else if self.ws.folders.smart.is_multi() {
                    self.ws.folders.smart.ids.clone()
                } else {
                    match &self.ws.scope {
                        Scope::Folder(id) | Scope::Smart(id) => vec![id.clone()],
                        _ => Vec::new(),
                    }
                };
                for id in picked {
                    if right {
                        self.ws.collapsed.remove(&id);
                    } else {
                        self.ws.collapsed.insert(id);
                    }
                }
                return;
            }
        }
        if self.ws.viewer.is_some() {
            match (left || up, right || down) {
                (true, _) => self.viewer_step(-1),
                (_, true) => self.viewer_step(1),
                _ => {}
            }
            return;
        }
        // ⇧← and ⇧→ grow or shrink the selection; ⇧↑ and ⇧↓ do the same
        // in the list layout and nothing in the others, as in Eagle.
        if shift && (left || right || up || down) && !self.ws.selected.is_empty() {
            let list = self.ws.layout == Layout::List;
            let forward = match (left, right, up, down) {
                (true, ..) => Some(false),
                (_, true, ..) => Some(true),
                (_, _, true, _) if list => Some(false),
                (.., true) if list => Some(true),
                _ => None,
            };
            if let Some(forward) = forward
                && let Some((sel, to)) = logic::extend_selection(&self.ws.order, &self.ws.selected, self.ws.anchor.as_deref(), forward)
            {
                self.ws.selected = sel;
                self.ws.scroll_to = Some(to);
            }
            return;
        }
        let step: isize = {
            let c = self.ws.columns as isize;
            match (left, right, up, down) {
                (true, ..) => -1,
                (_, true, ..) => 1,
                (_, _, true, _) => -c,
                (.., true) => c,
                _ => 0,
            }
        };
        if step != 0 && !self.ws.order.is_empty() {
            let cur = self.ws.selected.last();
            let pos = cur.and_then(|c| self.ws.order.iter().position(|x| x == c));
            let next = match pos {
                Some(p) => (p as isize + step).clamp(0, self.ws.order.len() as isize - 1) as usize,
                None => 0,
            };
            let id = self.ws.order[next].clone();
            self.ws.selected = vec![id.clone()];
            self.ws.anchor = Some(id.clone());
            self.ws.scroll_to = Some(id);
        }
    }

    /// The state commands and the menu bar look at (see `menu.rs`).
    pub(crate) fn menu_state(&self, ctx: &egui::Context) -> crate::menu::State {
        use crate::menu::ScopeKind;
        use crate::shortcuts::Page;
        crate::menu::State {
            library: self.ws.lib.is_some(),
            modal: self.prefs_window.open || self.ws.dup_dialog.is_some() || self.lock.is_locked() || self.item_rename.is_some() || self.ws.ext_dialog.is_some() || self.quick.is_open() || self.ws.folders.picker.is_some(),
            lockable: self.prefs.app_lock.is_some(),
            tag_clipboard: self.copied_tags.is_some(),
            last_folder: !self.recent_folders.is_empty(),
            folder_password: self.ws.folders.open_lock.is_some(),
            folder_unlocked: self.ws.folders.open_lock == Some(false),
            text: ctx.egui_wants_keyboard_input(),
            viewer: self.ws.viewer.is_some(),
            selected: self.ws.selected.len(),
            scope: match self.ws.scope {
                Scope::Folder(_) => ScopeKind::Folder,
                Scope::Smart(_) => ScopeKind::Smart,
                Scope::Trash => ScopeKind::Trash,
                Scope::Random(_) => ScopeKind::Random,
                _ => ScopeKind::Other,
            },
            page: Page::ALL.into_iter().find(|p| match (p.scope(), &self.ws.scope) {
                (Scope::Random(_), Scope::Random(_)) => true,
                (a, b) => a == *b,
            }),
            undo: !self.ws.undo.is_empty(),
            redo: !self.ws.redo.is_empty(),
            fullscreen: ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)),
            random_sorted: self.random_sorted(),
            space_scrolls: self.prefs.space == crate::prefs::SpaceKey::Scroll,
            flippable: self.ws.editable[0],
            rotatable: self.ws.editable[1],
            croppable: self.ws.editable[2],
            has_custom_thumb: self.has_custom_thumb(),
            docs: docs_path().is_some(),
            back: !self.ws.back.is_empty(),
            forward: !self.ws.forward.is_empty(),
            parent: self.ws.up.is_some(),
            picked_folders: self.ws.folders.sel.ids.len(),
            picked_smart: self.ws.folders.smart.ids.len(),
            layout: self.ws.layout,
            sidebar: self.show_sidebar,
            inspector: self.show_inspector,
            filter: self.ws.show_filter,
            show_name: self.display.show_name,
            show_info: self.display.show_info,
            show_comments: self.display.comment_badge,
            subfolders: self.display.subfolders,
            libraries: self.others.len(),
            presenting: self.modes.presenting,
            on_top: self.modes.on_top,
            grayscale: self.modes.grayscale,
            navigator: !self.modes.hide_navigator,
        }
    }

    /// Menu bar clicks since the last frame. Runs before `keyboard`, so a
    /// key handed back to egui is seen this frame.
    pub(crate) fn menu_clicks(&mut self, ctx: &egui::Context) {
        for click in crate::menu::take_clicks() {
            if click.by_key {
                // AppKit matched a key to an item; egui should have had it.
                if let Some(chord) = crate::menu::chord(click.cmd, &self.shortcuts) {
                    let events = key_events(&chord);
                    ctx.input_mut(|i| i.events.extend(events));
                }
            } else if !self.text_command(ctx, click.cmd) {
                self.run_command(ctx, click.cmd);
            }
        }
    }

    /// Edit ▸ Undo, Redo, Cut, Copy, Paste and Select All act on a focused
    /// text field: they become the events its keys would make. Returns
    /// whether the command went there.
    pub(crate) fn text_command(&self, ctx: &egui::Context, cmd: crate::shortcuts::Command) -> bool {
        use crate::shortcuts::Command as C;
        if !ctx.egui_wants_keyboard_input() || self.prefs_window.open {
            return false;
        }
        let chord = match cmd {
            C::Undo | C::Redo | C::Cut => crate::menu::chord(cmd, &self.shortcuts),
            C::Copy => Some(egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::C)),
            C::Paste => Some(egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::V)),
            C::SelectAll => Some(egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::A)),
            _ => None,
        };
        let Some(chord) = chord else { return false };
        let events = key_events(&chord);
        ctx.input_mut(|i| i.events.extend(events));
        true
    }

    /// The items a command acts on: the viewer's item, or the selection.
    /// Opens the source links of the acted-on items (Eagle's cap is 50).
    pub(crate) fn open_links(&mut self) {
        let Some(shared) = self.ws.lib.clone() else { return };
        let targets: Vec<_> = {
            let lib = lock(&shared);
            self.acted_on()
                .iter()
                .filter_map(|id| lib.item(id))
                .filter(|it| !it.url.is_empty())
                .take(50)
                .filter_map(|it| crate::urls::link_target(&it.url, |p| std::path::Path::new(p).exists()))
                .collect()
        };
        if targets.is_empty() {
            self.flash(tr("No link to open"));
        }
        for t in &targets {
            crate::urls::open_link(t);
        }
    }

    /// The first item acted on has a custom thumbnail.
    /// The open folder or smart folder has Random as its own sort.
    fn random_sorted(&self) -> bool {
        self.ws.lib.clone().is_some_and(|s| matches!(self.own_sort(&lock(&s)), Some((_, Some((FolderOrder::Random, _))))))
    }

    fn has_custom_thumb(&self) -> bool {
        let Some(id) = self.acted_on().into_iter().next() else { return false };
        self.ws.lib.clone().is_some_and(|s| lock(&s).item(&id).is_some_and(thumbnail::has_custom_thumbnail))
    }

    pub(crate) fn acted_on(&self) -> Vec<String> {
        match &self.ws.viewer {
            Some(id) => vec![id.clone()],
            None => self.ws.selected.clone(),
        }
    }

    /// Runs one command, from a key press or a menu click. A command that
    /// can't act now (`menu::enabled`) does nothing. Called before the
    /// frame locks the library, which some commands lock themselves.
    pub(crate) fn run_command(&mut self, ctx: &egui::Context, cmd: crate::shortcuts::Command) {
        use crate::shortcuts::Command as C;
        if !crate::menu::enabled(cmd, &self.menu_state(ctx)) {
            return;
        }
        let viewer = self.ws.viewer.is_some();
        match cmd {
            C::Preferences => self.open_preferences(),
            C::LockApp => self.lock_now(),
            C::CollectWindow => self.collect.toggle(),
            C::SwitchLibrary => {
                if self.ws.lib.is_some() {
                    self.show_sidebar = true;
                    self.open_switcher = true;
                } else {
                    self.pick_library();
                }
            }
            C::NewLibrary => self.new_library(),
            C::MergeLibrary => self.merge_library(),
            C::ClearLibraryHistory => self.clear_history_open = true,
            C::EmptyCacheReload => self.confirm_reload = true,
            C::ImportPack => self.import_pack_dialog(),
            C::ImportInboard => self.import_inboard_dialog(),
            C::NextLibrary => self.switch_to = Some(Switch::Focus(self.others.step(true))),
            C::PreviousLibrary => self.switch_to = Some(Switch::Focus(self.others.step(false))),
            C::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            C::ToggleInspector => self.show_inspector = !self.show_inspector,
            C::ToggleAll => {
                self.toggle_panels();
                // Tab must not also hand keyboard focus to the first widget.
                ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
            }
            C::ZoomIn if viewer => self.ws.view.zoom_key = Some(logic::ViewZoom::In),
            C::ZoomOut if viewer => self.ws.view.zoom_key = Some(logic::ViewZoom::Out),
            C::ZoomIn => self.tile = (self.tile + 20.0).min(TILE_MAX),
            C::ZoomOut => self.tile = (self.tile - 20.0).max(TILE_MIN),
            C::ZoomActual => self.ws.view.zoom_key = Some(logic::ViewZoom::Actual),
            C::ZoomFit => self.ws.view.zoom_key = Some(logic::ViewZoom::Fit),
            C::Back if viewer => self.ws.viewer = None,
            C::Back => self.go_back(),
            C::Forward => self.go_forward(),
            C::Rename => self.rename_items(self.ws.selected.clone()),
            C::Find => self.focus_search = true,
            C::SearchAll => self.search_all(),
            C::FilterSidebar => self.focus_filter_sidebar(),
            C::ResetFilter => self.reset_filter(),
            C::OpenFilter(kind) => self.open_filter(kind),
            C::QuickSwitch | C::QuickSwitchKey => self.quick.toggle(),
            C::FolderPasswordNew | C::FolderPasswordChange | C::FolderPasswordRemove | C::LockFolder => {
                if let Scope::Folder(id) = &self.ws.scope {
                    self.ws.org.password_request = Some((id.clone(), cmd));
                }
            }
            C::FolderSettings => {
                if let Scope::Folder(id) = &self.ws.scope {
                    self.ws.org.settings_request = Some(id.clone());
                }
            }
            // The first selected item, as in Eagle.
            C::SetFolderCover => {
                if let Scope::Folder(fid) = &self.ws.scope
                    && let Some(id) = self.ws.selected.first()
                {
                    self.ws.actions.push(Action::Edit(image_edit::Op::SetFolderCover(fid.clone(), id.clone())));
                }
            }
            // The picked folders, else the open one, as Eagle's menu does.
            C::MoveFolder => match (&self.ws.scope, self.ws.folders.sel.ids.is_empty()) {
                (_, false) => self.open_mover(self.ws.folders.sel.ids.clone()),
                (Scope::Folder(id), true) => self.open_mover(vec![id.clone()]),
                _ => {}
            },
            C::FolderUp | C::FolderDown | C::FolderTop | C::FolderBottom => {
                use crate::organize::Arrange;
                let how = match cmd {
                    C::FolderUp => Arrange::Up,
                    C::FolderDown => Arrange::Down,
                    C::FolderTop => Arrange::Top,
                    _ => Arrange::Bottom,
                };
                self.ws.actions.push(Action::Arrange(how));
            }
            // As in Eagle: next to the open folder, else at the top level.
            C::NewFolder => self.ws.actions.push(match &self.ws.scope {
                Scope::Folder(id) => Action::CreateSibling(id.clone()),
                _ => Action::CreateFolder { parent: None },
            }),
            C::NewSubfolder => {
                if let Scope::Folder(id) = &self.ws.scope {
                    self.ws.actions.push(Action::CreateFolder { parent: Some(id.clone()) });
                }
            }
            C::NewSmartFolder => self.ws.smart_dialog = Some(SmartDraft::default()),
            C::NewMenu => self.request_new_menu(),
            C::NewTextFile => {
                let folders = self.scope_folders();
                self.ws.actions.push(Action::NewTextFile(folders));
            }
            C::Copy => self.copy_selection(),
            C::Paste => self.paste(),
            C::ToggleFilter => self.ws.show_filter = !self.ws.show_filter,
            C::LayoutGrid => self.set_layout(Layout::Grid),
            C::LayoutJustified => self.set_layout(Layout::Justified),
            C::LayoutWaterfall => self.set_layout(Layout::Waterfall),
            C::LayoutList => self.set_layout(Layout::List),
            C::OpenDefault => self.open_keys(true),
            C::RevealInFinder | C::RevealAlt => self.open_keys(false),
            C::OpenLink => self.open_links(),
            C::SelectAll => self.ws.selected = self.ws.order.clone(),
            C::Viewer if viewer => self.ws.viewer = None,
            C::Viewer => match self.prefs.space {
                // As in Eagle, Preview and Quick Look need a selection.
                crate::prefs::SpaceKey::Preview if !self.ws.selected.is_empty() => self.ws.viewer = self.ws.selected.first().cloned(),
                crate::prefs::SpaceKey::QuickLook if !self.ws.selected.is_empty() => self.quick_look(),
                crate::prefs::SpaceKey::Preview | crate::prefs::SpaceKey::QuickLook => {}
                crate::prefs::SpaceKey::Scroll => self.ws.scroll = Some(crate::shortcuts::Scroll::PageDown),
            },
            // On the All Tags page ⌘⌫ deletes the open tag group from the
            // tag sidebar, or takes the picked tags out of the open group,
            // as Eagle's delete does there.
            C::Trash if self.ws.scope == Scope::AllTags => {
                if let crate::tag_view::TagView::Group(id) = self.ws.org.tag_view.clone()
                    && let Some(l) = self.ws.lib.clone()
                {
                    let lib = lock(&l);
                    if self.zone == logic::Zone::Tags {
                        self.ask_delete_tag_group(&lib, &id);
                    } else if !self.ws.org.tag_sel.is_empty() {
                        let tags = self.ws.org.tag_sel.clone();
                        self.push_bulk(tags.len(), crate::ui::organize::org(crate::ui::organize::OrgAction::RemoveFromGroup(id, tags)));
                    }
                }
            }
            C::Trash => {
                let ids = self.ws.selected.clone();
                if self.ws.scope == Scope::Trash {
                    self.ws.confirm = Some(Confirm::DeleteItems(ids));
                } else {
                    self.leave_viewed(&ids);
                    self.push_bulk(ids.len(), Action::TrashAsk(ids));
                }
            }
            C::ExportComputer | C::ExportPack | C::ExportCsv | C::ExportFormat | C::CopyPath | C::CopyFolderPath | C::CopyName | C::CopyLink | C::CopyEagleLink | C::CopySourceUrl | C::CopyThumbnail | C::CopyBase64 => {
                self.export_keys(&|c| c == cmd);
            }
            C::Scroll(how) => self.ws.scroll = Some(how),
            C::SelectFirst | C::SelectLast => {
                let id = if cmd == C::SelectFirst { self.ws.order.first() } else { self.ws.order.last() };
                if let Some(id) = id.cloned() {
                    self.ws.selected = vec![id.clone()];
                    self.ws.anchor = Some(id.clone());
                    self.ws.scroll_to = Some(id);
                }
            }
            C::AddTags | C::TagPanel => self.open_tag_picker(ctx, None),
            C::CopyTags => {
                if let Some(l) = self.ws.lib.clone() {
                    let ids = self.acted_on();
                    let tags = {
                        let lib = lock(&l);
                        let items: Vec<&Item> = ids.iter().filter_map(|id| lib.item(id)).collect();
                        crate::tags::common_tags(&items)
                    };
                    self.copy_tags(ctx, tags);
                }
            }
            C::PasteTags => self.paste_tags(self.acted_on()),
            C::ClearTags => self.ws.actions.push(Action::ClearTags(self.acted_on())),
            C::Rate(n) => {
                let ids = self.acted_on();
                let asks = ids.len() >= BULK_CONFIRM;
                self.push_bulk(ids.len(), Action::BatchRating(ids, n.into()));
                if !asks {
                    self.flash_brief(rated_note(n));
                }
            }
            C::CloseWindow if self.ws.viewer.is_some() => self.ws.viewer = None,
            C::CloseWindow => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            C::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            C::FullScreen => {
                let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
            }
            C::AddToLastFolder => self.add_to_last_used(self.acted_on()),
            C::ActionsPanel => self.open_actions_panel(None),
            C::AddToFolders | C::FolderPanel => {
                if let Some(l) = self.ws.lib.clone() {
                    let ids = self.acted_on();
                    self.open_add_to_folders(&lock(&l), ids);
                }
            }
            C::CustomThumbnail | C::ClipboardThumbnail => {
                let Some(id) = self.acted_on().into_iter().next() else { return };
                let allowed = self.ws.lib.clone().is_some_and(|s| lock(&s).item(&id).is_some_and(|i| thumbnail::custom_thumbnail_allowed(&i.ext)));
                if !allowed {
                    self.flash(tr("This file is its own thumbnail"));
                } else if cmd == C::CustomThumbnail {
                    self.ws.actions.push(Action::Edit(image_edit::Op::PickCustomThumbnail(id)));
                } else {
                    self.ws.actions.push(Action::Edit(image_edit::Op::ClipboardThumbnail(id)));
                }
            }
            C::ResetThumbnail => {
                // As in Eagle, the first item acted on.
                if let Some(id) = self.acted_on().into_iter().next() {
                    self.ws.actions.push(Action::Edit(image_edit::Op::ResetCustomThumbnail(id)));
                }
            }
            C::Duplicate | C::DuplicateAlt => {
                if let Some(id) = self.acted_on().into_iter().next() {
                    self.ws.actions.push(Action::Edit(image_edit::Op::Duplicate(id)));
                }
            }
            C::RefreshThumbnail => {
                let ids = self.acted_on();
                self.ws.actions.push(Action::Edit(image_edit::Op::RefreshThumbnail(ids)));
            }
            C::RemoveFromFolder => {
                let Scope::Folder(fid) = self.ws.scope.clone() else { return };
                let ids = self.ws.selected.clone();
                let next = logic::next_after_removal(&self.ws.order, &ids);
                if self.ws.viewer.is_some() {
                    self.ws.viewer.clone_from(&next);
                }
                self.ws.selected = next.iter().cloned().collect();
                self.ws.anchor.clone_from(&next);
                self.ws.actions.push(Action::RemoveFromFolder(ids, fid));
            }
            // A new order in place, without a history step.
            C::Reshuffle if self.random_sorted() => {
                // A folder sorted at random: forget its seed, so it shuffles again.
                self.ws.org.reshuffle();
                self.ws.selected.clear();
                self.ws.anchor = None;
            }
            C::Reshuffle => {
                self.ws.folders.clear();
                self.ws.scope = Scope::Random(fastrand_seed());
                self.ws.selected.clear();
                self.ws.anchor = None;
            }
            C::ParentFolder => {
                if let Some(up) = self.ws.up.clone() {
                    self.go(up);
                }
            }
            C::ToggleFolders => {
                let Some(shared) = self.ws.lib.clone() else { return };
                let lib = lock(&shared);
                match &self.ws.scope {
                    Scope::Smart(id) => crate::organize::toggle_all(&lib.smart_folders(), Some(id), &mut self.ws.collapsed),
                    Scope::Folder(id) => crate::organize::toggle_all(&lib.metadata.folders, Some(id), &mut self.ws.collapsed),
                    _ => crate::organize::toggle_all(&lib.metadata.folders, None, &mut self.ws.collapsed),
                }
            }
            C::Page(p) => {
                self.ws.viewer = None;
                self.go(p.scope());
            }
            C::Undo | C::Redo => self.ws.undo_request = Some(cmd == C::Undo),
            // Only for a focused text field (`text_command`).
            C::Cut => {}
            C::Import => self.import_files_dialog(),
            C::ImportFolders => self.import_folders_dialog(),
            C::FindDuplicates => {
                self.ws.viewer = None;
                self.go(Scope::Duplicates);
            }
            C::FlipHorizontal | C::RotateLeft | C::Crop => {
                let Some(shared) = self.ws.lib.clone() else { return };
                let ids = self.acted_on();
                let lib = lock(&shared);
                let ext_of = |id: &str| lib.item(id).map(|i| i.ext.clone());
                use falcon_library::transform;
                match cmd {
                    C::FlipHorizontal => {
                        let ids = image_edit::editable(&ids, ext_of, transform::can_flip);
                        self.flip_request(ids, transform::Flip::Horizontal);
                    }
                    C::RotateLeft => {
                        let ids = image_edit::editable(&ids, ext_of, transform::can_rotate);
                        self.rotate_request(ids, -1);
                    }
                    _ => {
                        if self.ws.tools.crop.is_some() {
                            // The crop key again saves.
                            if let Some(c) = self.ws.tools.crop.as_mut() {
                                c.confirming = true;
                            }
                        } else if let [id] = image_edit::editable(&ids, ext_of, transform::can_crop).as_slice() {
                            self.ws.viewer = Some(id.clone());
                            self.ws.tools.crop_next = Some(id.clone());
                        }
                    }
                }
            }
            C::WindowZoom => {
                let on = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!on));
            }
            C::Documentation => {
                if let Some(p) = docs_path() {
                    crate::open_with::open_default(&[p.to_path_buf()]);
                }
            }
            C::Presentation => self.toggle_presentation(ctx),
            C::AlwaysOnTop => self.toggle_on_top(ctx),
            C::Grayscale => self.modes.grayscale = !self.modes.grayscale,
            C::Navigator => self.modes.hide_navigator = !self.modes.hide_navigator,
            C::OpenNewWindow => self.open_preview_window(),

            C::About => self.about.open = true,
            C::DebugReport => self.export_debug_report(ctx),
            C::ErrorLogs => {
                if let Some(p) = crate::errlog::ensure() {
                    crate::open_with::open_default(&[p]);
                }
            }
            C::Player(_) => {}
            C::ShowFalcon => self.show_falcon(ctx, false),
            C::ShowSearch => self.show_falcon(ctx, true),
            C::Capture(mode) => self.start_capture(mode),
            C::ToggleName => self.display.show_name = !self.display.show_name,
            C::ToggleInfo => self.display.show_info = !self.display.show_info,
            C::ToggleComments => self.display.comment_badge = !self.display.comment_badge,
            C::ToggleSubfolders => self.display.subfolders = !self.display.subfolders,
        }
    }

    /// Shows the previous (`-1`) or next (`1`) item of the grid in the
    /// viewer; the selection follows. Past either end a note says so, as
    /// Eagle's viewer does.
    pub(crate) fn viewer_step(&mut self, step: isize) {
        let Some(cur) = self.ws.viewer.clone() else { return };
        let at = self.ws.order.iter().position(|x| *x == cur);
        let next = match logic::viewer_step(self.ws.order.len(), at, step) {
            Ok(n) => n,
            Err(edge) => {
                self.flash(edge.note());
                return;
            }
        };
        let id = self.ws.order[next].clone();
        self.ws.viewer = Some(id.clone());
        self.ws.selected = vec![id.clone()];
        self.ws.anchor = Some(id.clone());
        self.ws.scroll_to = Some(id);
    }
}

#[cfg(test)]
mod tests {
    use super::{key_events, rated_note};
    use eframe::egui::{Event, Key, KeyboardShortcut, Modifiers};

    #[test]
    fn menu_keys_become_the_events_egui_winit_sends() {
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        assert_eq!(key_events(&cmd(Key::C)), vec![Event::Copy]);
        assert_eq!(key_events(&cmd(Key::X)), vec![Event::Cut]);
        assert!(matches!(key_events(&cmd(Key::V)).as_slice(), [Event::Paste(_)]));
        // ⌥⌘C is an ordinary key press.
        let alt_cmd_c = key_events(&KeyboardShortcut::new(Modifiers { alt: true, ..Modifiers::COMMAND }, Key::C));
        assert!(matches!(alt_cmd_c.as_slice(), [Event::Key { key: Key::C, pressed: true, .. }]));
        // A bare key also types its character.
        let r = key_events(&KeyboardShortcut::new(Modifiers::NONE, Key::R));
        assert!(matches!(r.as_slice(), [Event::Key { key: Key::R, .. }, Event::Text(t)] if t == "r"));
        let space = key_events(&KeyboardShortcut::new(Modifiers::NONE, Key::Space));
        assert!(matches!(space.as_slice(), [Event::Key { .. }, Event::Text(t)] if t == " "));
        assert_eq!(key_events(&cmd(Key::W)).len(), 1);
    }

    #[test]
    fn rating_toasts() {
        assert_eq!(rated_note(0), "Removed the rating");
        assert_eq!(rated_note(1), "Rated 1 star");
        assert_eq!(rated_note(3), "Rated 3 stars");
    }
}
