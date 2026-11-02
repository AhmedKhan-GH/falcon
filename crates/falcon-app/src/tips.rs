//! The tips shown on first launch and from Help ▸ Show Tips. A falcon
//! addition: Eagle has no such overlay. The list, the open/step state and the
//! rule for showing it on launch live here; `ui/dialogs/tips.rs` draws it.

use crate::locale::tr_key;
use crate::shortcuts::Command;

/// One tip: a short title and a sentence. A tip that names a key carries the
/// command whose live binding fills `{key}` in its second text; it uses the
/// plain text when that command has no key.
pub(crate) struct Tip {
    pub title: &'static str,
    pub body: &'static str,
    pub with_key: Option<(Command, &'static str)>,
}

pub(crate) const TIPS: &[Tip] = &[
    Tip {
        title: tr_key("Add files"),
        body: tr_key("Drag files or folders from Finder onto the window to import them. Pasting copied files or images imports them too."),
        with_key: None,
    },
    Tip {
        title: tr_key("Look at an item"),
        body: tr_key("Select an item and open the viewer to see it at full size. Open it again to go back to the grid."),
        with_key: Some((Command::Viewer, tr_key("Select an item and press {key} to see it at full size. Press it again to go back to the grid."))),
    },
    Tip {
        title: tr_key("Search"),
        body: tr_key("Type in the search field at the top to find items by name, tag, note and more."),
        with_key: Some((Command::Find, tr_key("Press {key} to focus the search field, then type to find items by name, tag, note and more."))),
    },
    Tip {
        title: tr_key("Filters"),
        body: tr_key("The filter bar narrows the list by colour, tags, folders, shape, rating, type and more. Several filters can be on at once."),
        with_key: Some((Command::ToggleFilter, tr_key("Press {key} to show or hide the filter bar. It narrows the list by colour, tags, folders, shape, rating, type and more, and several filters can be on at once."))),
    },
    Tip {
        title: tr_key("Tags and smart folders"),
        body: tr_key("Tag selected items from the tag picker. A smart folder, made with File ▸ New Smart Folder, lists the items that match its rules."),
        with_key: Some((Command::TagPanel, tr_key("Press {key} to tag the selected items from the tag picker. A smart folder, made with File ▸ New Smart Folder, lists the items that match its rules."))),
    },
    Tip {
        title: tr_key("Jump to a folder"),
        body: tr_key("Jump to any folder, tag or smart folder by typing part of its name."),
        with_key: Some((Command::QuickSwitch, tr_key("Press {key} and type part of a name to jump to a folder, tag or smart folder."))),
    },
];

/// Whether the tips are open and which one is showing.
#[derive(Default)]
pub(crate) struct TipsState {
    pub open: bool,
    pub index: usize,
}

impl TipsState {
    /// Opens at the first tip.
    pub fn show(&mut self) {
        self.open = true;
        self.index = 0;
    }

    pub fn next(&mut self) {
        self.index = (self.index + 1).min(TIPS.len() - 1);
    }

    pub fn back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn is_first(&self) -> bool {
        self.index == 0
    }

    pub fn is_last(&self) -> bool {
        self.index + 1 >= TIPS.len()
    }
}

/// Whether the tips should open by themselves: never seen, the welcome flow
/// finished, a library open, and no dialog up.
pub(crate) fn should_show_on_launch(tips_seen: bool, welcomed: bool, library_open: bool, a_modal_is_open: bool) -> bool {
    !tips_seen && welcomed && library_open && !a_modal_is_open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_rule() {
        // (tips_seen, welcomed, library_open, modal) -> shown
        let table = [
            (false, true, true, false, true),
            (true, true, true, false, false),
            (false, false, true, false, false),
            (false, true, false, false, false),
            (false, true, true, true, false),
            (true, false, false, true, false),
        ];
        for (seen, welcomed, lib, modal, want) in table {
            assert_eq!(should_show_on_launch(seen, welcomed, lib, modal), want, "{seen} {welcomed} {lib} {modal}");
        }
    }

    #[test]
    fn next_and_back_stay_in_range() {
        let mut s = TipsState::default();
        s.show();
        s.back();
        assert_eq!(s.index, 0);
        assert!(s.is_first());
        for _ in 0..TIPS.len() + 3 {
            s.next();
        }
        assert_eq!(s.index, TIPS.len() - 1);
        assert!(s.is_last());
        s.back();
        assert_eq!(s.index, TIPS.len() - 2);
    }

    #[test]
    fn show_restarts_at_the_first_tip() {
        let mut s = TipsState { open: false, index: 4 };
        s.show();
        assert!(s.open);
        assert_eq!(s.index, 0);
    }

    #[test]
    fn old_saved_state_has_no_tips_seen() {
        let saved: crate::app::Saved = serde_json::from_str(r#"{"welcomed":true}"#).unwrap();
        assert!(!saved.tips_seen);
        assert!(saved.welcomed);
    }

    #[test]
    fn every_tip_has_text() {
        assert!((5..=6).contains(&TIPS.len()));
        for t in TIPS {
            assert!(!t.title.trim().is_empty());
            assert!(!t.body.trim().is_empty());
            assert!(!t.body.contains("{key}"));
            if let Some((_, keyed)) = t.with_key {
                assert!(keyed.contains("{key}"), "{}", t.title);
            }
        }
    }
}
