//! What the cards need besides the item: whether to show the pin badge in
//! the open folder, worked out once per frame.

use falcon_library::model::FolderOrder;

use crate::app::App;
use crate::logic::Scope;
use falcon_library::Library;

/// Per-workspace card state.
#[derive(Default)]
pub(crate) struct CardHints {
    /// The folder whose pins the cards show (a single open folder), if any.
    pub pin_scope: Option<String>,
    /// That folder's own sort is Random, which hides the badge.
    pub random_sort: bool,
}

impl App {
    /// Refreshes the frame's card hints from the open view.
    pub(crate) fn update_card_hints(&mut self, lib: &Library) {
        self.ws.card_hints.random_sort = matches!(self.own_sort(lib), Some((_, Some((FolderOrder::Random, _)))));
        self.ws.card_hints.pin_scope = match &self.ws.scope {
            Scope::Folder(id) if !self.ws.folders.combined() => Some(id.clone()),
            _ => None,
        };
    }

    /// Whether `item` shows the pin badge in this view.
    pub(crate) fn shows_pin(&self, item: &falcon_library::model::Item) -> bool {
        crate::logic::pinned_here(item, self.ws.card_hints.pin_scope.as_deref(), self.ws.card_hints.random_sort)
    }
}
