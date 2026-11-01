//! Plays the sound effects (`crate::sounds`) for the app: which library
//! change sounds, and the call that checks the preferences and the
//! debounce.

use super::App;
use super::actions::Action;
use crate::sounds::Sound;
use crate::ui::organize::OrgAction;
use crate::ui::sidebar::folders::FolderAction;

/// The sound an action plays once applied, as Eagle sounds them: item
/// removals (Trash, Remove from Folder) the delete-item sound; folders,
/// smart folders, tags and permanent deletes the delete-folder one. Move
/// to Trash that may ask first (`TrashAsk`) sounds when it trashes, in
/// the reducer, and Empty Trash when it finishes.
pub(crate) fn action_sound(a: &Action) -> Option<Sound> {
    match a {
        Action::RemoveFromFolder(..) | Action::Trash(_, true) | Action::TrashInFolder(..) => Some(Sound::DeleteItem),
        Action::DeletePermanently(_) | Action::DeleteTags(_) | Action::DeleteSmart(_) => Some(Sound::DeleteFolder),
        Action::Folders(FolderAction::Delete(..) | FolderAction::DeleteSmart(_)) => Some(Sound::DeleteFolder),
        Action::Organize(OrgAction::RemoveFromGroup(..)) => Some(Sound::DeleteFolder),
        _ => None,
    }
}

impl App {
    /// Plays `sound` if Preferences ▸ Notification allows it; never waits.
    pub(crate) fn play_sound(&mut self, sound: Sound) {
        self.sounds.play(sound, &self.prefs.sounds, std::time::Instant::now(), &mut self.audio);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_actions_sound() {
        let ids = || vec!["a".to_string()];
        assert_eq!(action_sound(&Action::Trash(ids(), true)), Some(Sound::DeleteItem));
        assert_eq!(action_sound(&Action::Trash(ids(), false)), None, "a restore is silent");
        assert_eq!(action_sound(&Action::RemoveFromFolder(ids(), "f".into())), Some(Sound::DeleteItem));
        assert_eq!(action_sound(&Action::TrashInFolder(ids(), "f".into(), true)), Some(Sound::DeleteItem));
        assert_eq!(action_sound(&Action::TrashAsk(ids())), None, "sounds in the reducer once it trashes");
        assert_eq!(action_sound(&Action::DeletePermanently(ids())), Some(Sound::DeleteFolder));
        assert_eq!(action_sound(&Action::Folders(FolderAction::Delete(ids(), true))), Some(Sound::DeleteFolder));
        assert_eq!(action_sound(&Action::Folders(FolderAction::DeleteSmart(ids()))), Some(Sound::DeleteFolder));
        assert_eq!(action_sound(&Action::DeleteSmart("s".into())), Some(Sound::DeleteFolder));
        assert_eq!(action_sound(&Action::DeleteTags(ids())), Some(Sound::DeleteFolder));
        assert_eq!(action_sound(&Action::Organize(OrgAction::RemoveFromGroup("g".into(), ids()))), Some(Sound::DeleteFolder));
        assert_eq!(action_sound(&Action::EmptyTrash), None, "sounds when it finishes");
        assert_eq!(action_sound(&Action::RenameFolder("f".into(), "x".into())), None);
    }
}
