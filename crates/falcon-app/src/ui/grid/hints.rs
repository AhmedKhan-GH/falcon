//! What the cards need besides the item: whether to show the pin badge in
//! the open folder, worked out once per frame.

use std::collections::HashMap;

use eframe::egui::Ui;
use falcon_library::model::{FolderOrder, Item};

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
    /// Tooltips for cards whose thumbnail failed, with when they were
    /// worked out; the disk is looked at again after `RECHECK` seconds.
    broken: HashMap<String, (f64, Option<&'static str>)>,
}

/// Seconds a broken thumbnail's hint is kept before the files are checked again.
const RECHECK: f64 = 3.0;

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
    pub(crate) fn shows_pin(&self, item: &Item) -> bool {
        crate::logic::pinned_here(item, self.ws.card_hints.pin_scope.as_deref(), self.ws.card_hints.random_sort)
    }

    /// The tooltip for an item whose thumbnail failed to load. The files
    /// are only looked at for failed cards, and not again for a few seconds.
    pub(crate) fn broken_hint(&mut self, ui: &Ui, lib: &Library, item: &Item) -> Option<&'static str> {
        if !self.ws.loader.has_failed(&item.id, crate::loader::Tier::Grid) {
            return None;
        }
        let now = ui.input(|i| i.time);
        if let Some((at, hint)) = self.ws.card_hints.broken.get(&item.id)
            && now - at < RECHECK
        {
            return *hint;
        }
        let root = lib.root();
        let thumb = item.info_dir(root).join(item.thumbnail_name());
        let hint = crate::logic::broken_thumb_hint(true, thumb.to_string_lossy().chars().count(), item.original_path(root).is_file(), thumb.is_file());
        self.ws.card_hints.broken.insert(item.id.clone(), (now, hint));
        hint
    }
}
