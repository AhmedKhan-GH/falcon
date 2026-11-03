//! Whether a modal dialog is up. Behind one, key presses and menu items
//! must not run commands (see `App::keyboard` and `menu::enabled`).

use super::App;

impl App {
    /// Whether a dialog that takes the whole window is up: the confirmations
    /// and prompts, the folder, tag, export and tool dialogs, Preferences,
    /// About, Go to Folder, the import prompts and the progress modal.
    pub(crate) fn blocking_dialog_open(&self) -> bool {
        let ws = &self.ws;
        let lib = ws.lib.is_some();
        self.prefs_window.open
            || self.about.open
            || self.tips.open
            || self.quick.is_open()
            || self.item_rename.is_some()
            || self.pack_ask.is_some()
            || self.merge.is_some()
            || self.alert.is_some()
            || (lib && (self.missing.is_some() || self.loading.is_some()))
            || self.confirm_reload
            || self.clear_history_open
            || self.remove_library.is_some()
            || self.force_close.is_some()
            || self.switch_warning.is_some()
            || self.quit.asking.is_some()
            || self.link_import.open
            || self.dialogs.create.as_ref().is_some_and(|c| !c.from_welcome)
            || !self.imports.pending.is_empty()
            || self.imports.confirm_clear
            || self.tools.dialog_open()
            || ws.dup_dialog.is_some()
            || ws.ext_dialog.is_some()
            || ws.smart_dialog.is_some()
            || ws.confirm.is_some()
            || ws.bulk.is_some()
            || ws.op.busy()
            || ws.export.dialog_open()
            || ws.org.dialog_open()
            || ws.folders.dialog_open()
    }

    /// Whether any modal surface is up: a blocking dialog, the tag picker,
    /// the Actions panel or its editor. The picker and the panel close with
    /// Esc, which they read themselves.
    pub(crate) fn any_modal_open(&self) -> bool {
        modal_open(self.blocking_dialog_open(), self.ws.picker.open, self.user_actions.panel.open, self.user_actions.editor.is_some())
    }
}

/// The union rule behind `App::any_modal_open`.
pub(crate) fn modal_open(dialog: bool, tag_picker: bool, actions_panel: bool, action_editor: bool) -> bool {
    dialog || tag_picker || actions_panel || action_editor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dialogs::{export::ExportState, tools::Tools};
    use crate::ui::organize::OrgState;
    use crate::ui::sidebar::folders::FolderUi;

    #[test]
    fn any_surface_counts() {
        assert!(!modal_open(false, false, false, false));
        assert!(modal_open(true, false, false, false));
        assert!(modal_open(false, true, false, false));
        assert!(modal_open(false, false, true, false));
        assert!(modal_open(false, false, false, true));
    }

    #[test]
    fn fresh_state_has_no_dialog() {
        assert!(!Tools::default().dialog_open());
        assert!(!ExportState::default().dialog_open());
        assert!(!OrgState::default().dialog_open());
        assert!(!FolderUi::default().dialog_open());
    }
}
