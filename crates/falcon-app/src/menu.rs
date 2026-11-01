//! When each command can act. A key press and a menu click both go
//! through `App::run_command`, which asks [`enabled`] first, so a command
//! does the same thing (or nothing) however it was given.

use crate::logic::Layout;
use crate::shortcuts::Command;

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
    pub back: bool,
    pub forward: bool,
    /// The open folder or smart folder has a parent.
    pub parent: bool,
    pub layout: Layout,
    pub sidebar: bool,
    pub inspector: bool,
    pub filter: bool,
    /// Open libraries.
    pub libraries: usize,
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
        C::SelectAll => s.library,
        C::Copy => grid && s.selected > 0,
        C::Paste => grid,
        C::Trash => any,
        C::RemoveFromFolder => any && s.scope == ScopeKind::Folder,
        C::OpenDefault | C::RevealInFinder | C::RefreshThumbnail => any || (s.library && s.viewer),
        C::Rename => grid && s.selected == 1,
        C::Back => s.viewer || (s.library && s.back),
        C::Forward => grid && s.forward,
        C::Find | C::ToggleFilter | C::NewFolder | C::NewSmartFolder => grid,
        C::NewSubfolder => grid && s.scope == ScopeKind::Folder,
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
        C::FolderUp | C::FolderDown | C::FolderTop | C::FolderBottom => grid && matches!(s.scope, ScopeKind::Folder | ScopeKind::Smart),
        C::SwitchLibrary | C::CollectWindow | C::Preferences | C::CloseWindow | C::Minimize | C::FullScreen => true,
        C::NextLibrary | C::PreviousLibrary => s.libraries > 1,
        C::Page(_) => s.library,
        C::Scroll(_) | C::SelectFirst | C::SelectLast => grid,
        C::ParentFolder => grid && s.parent,
        C::Reshuffle => grid && s.scope == ScopeKind::Random,
        C::Rate(_) => any || (s.library && s.viewer),
        C::Player(_) => false,
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
    }

    #[test]
    fn no_library() {
        let s = State::default();
        assert!(enabled(Command::SwitchLibrary, &s));
        assert!(!enabled(Command::NewFolder, &s));
        assert!(!enabled(Command::NextLibrary, &s));
    }
}
