//! State of the main window's smaller pieces that have no module of their
//! own in `app/`: the text cards' file cache.

/// Held by `App` as `shell`.
#[derive(Default)]
pub(crate) struct Shell {
    /// Text read for `.txt` cards whose record has none (`text_cards.rs`).
    pub(crate) text_cards: crate::text_cards::TextCards,
}
