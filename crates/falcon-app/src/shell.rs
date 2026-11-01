//! State of the main window's smaller pieces that have no module of their
//! own in `app/`: the text cards' file cache and the New… menu request.

/// Held by `App` as `shell`.
#[derive(Default)]
pub(crate) struct Shell {
    /// Text read for `.txt` cards whose record has none (`text_cards.rs`).
    pub(crate) text_cards: crate::text_cards::TextCards,
    /// Open the New… menu next frame (⌘N, File ▸ New…; `ui/menus/new.rs`).
    pub(crate) new_menu: bool,
}
