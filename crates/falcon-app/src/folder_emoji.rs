//! Emoji as folder and smart folder icons (a falcon addition; Eagle has
//! none).
//!
//! Eagle builds a CSS class from a folder's `icon` value, so an emoji
//! there would have no artwork. The emoji is therefore never written to
//! `metadata.json`: falcon keeps a map from folder or smart folder id to
//! emoji in its own app data (`Saved`). Ids are unique per folder, so the
//! map survives moving or renaming a library, and entries of deleted
//! folders stay so that undoing a delete brings the emoji back. The
//! folder's own `icon` and `iconColor` are left alone, so Eagle still
//! shows the named icon.
//!
//! Nothing here can write to a library: the model is only a map and has no
//! path to one, so leaving `metadata.json` alone holds by construction.
//!
//! The picker offers a fixed list of single-codepoint emoji, because
//! falcon draws them from the system emoji font's bitmaps without text
//! shaping (`folder_emoji_tex.rs`).

use std::collections::BTreeMap;

use crate::locale::tr_key;

/// The emoji the picker offers, each with a short name (a catalogue key in
/// the `emoji::` context, shown with `tr`) that the menu search matches.
pub const CURATED: &[(char, &str)] = &[
    ('📁', tr_key("emoji::Folder")),
    ('📂', tr_key("emoji::Open Folder")),
    ('📷', tr_key("emoji::Camera")),
    ('🎨', tr_key("emoji::Palette")),
    ('🖼', tr_key("emoji::Picture")),
    ('🎬', tr_key("emoji::Clapper")),
    ('🎞', tr_key("emoji::Film")),
    ('🎵', tr_key("emoji::Music")),
    ('🎧', tr_key("emoji::Headphones")),
    ('🎤', tr_key("emoji::Microphone")),
    ('📚', tr_key("emoji::Books")),
    ('📖', tr_key("emoji::Book")),
    ('📝', tr_key("emoji::Memo")),
    ('📌', tr_key("emoji::Pin")),
    ('📎', tr_key("emoji::Paperclip")),
    ('✂', tr_key("emoji::Scissors")),
    ('🔧', tr_key("emoji::Wrench")),
    ('🔨', tr_key("emoji::Hammer")),
    ('⚙', tr_key("emoji::Gear")),
    ('💡', tr_key("emoji::Light Bulb")),
    ('🔒', tr_key("emoji::Lock")),
    ('🔑', tr_key("emoji::Key")),
    ('📦', tr_key("emoji::Package")),
    ('🛒', tr_key("emoji::Cart")),
    ('💼', tr_key("emoji::Briefcase")),
    ('🎒', tr_key("emoji::Backpack")),
    ('💻', tr_key("emoji::Laptop")),
    ('📱', tr_key("emoji::Phone")),
    ('🖨', tr_key("emoji::Printer")),
    ('📺', tr_key("emoji::Television")),
    ('🎮', tr_key("emoji::Game")),
    ('🧩', tr_key("emoji::Puzzle")),
    ('🎁', tr_key("emoji::Gift")),
    ('🏆', tr_key("emoji::Trophy")),
    ('🎓', tr_key("emoji::Graduation Cap")),
    ('🧪', tr_key("emoji::Test Tube")),
    ('🔬', tr_key("emoji::Microscope")),
    ('🔭', tr_key("emoji::Telescope")),
    ('📅', tr_key("emoji::Calendar")),
    ('📊', tr_key("emoji::Bar Chart")),
    ('📈', tr_key("emoji::Rising Chart")),
    ('🗺', tr_key("emoji::Map")),
    ('🧭', tr_key("emoji::Compass")),
    ('⏰', tr_key("emoji::Alarm Clock")),
    ('💰', tr_key("emoji::Money Bag")),
    ('🏠', tr_key("emoji::House")),
    ('🏢', tr_key("emoji::Office")),
    ('✈', tr_key("emoji::Airplane")),
    ('🚗', tr_key("emoji::Car")),
    ('🚲', tr_key("emoji::Bicycle")),
    ('⛵', tr_key("emoji::Sailboat")),
    ('🚀', tr_key("emoji::Rocket")),
    ('🌱', tr_key("emoji::Seedling")),
    ('🌳', tr_key("emoji::Tree")),
    ('🌸', tr_key("emoji::Blossom")),
    ('🌻', tr_key("emoji::Sunflower")),
    ('🍀', tr_key("emoji::Clover")),
    ('🍁', tr_key("emoji::Maple Leaf")),
    ('🌵', tr_key("emoji::Cactus")),
    ('🌙', tr_key("emoji::Moon")),
    ('☀', tr_key("emoji::Sun")),
    ('⭐', tr_key("emoji::Star")),
    ('⚡', tr_key("emoji::Lightning")),
    ('🔥', tr_key("emoji::Fire")),
    ('💧', tr_key("emoji::Droplet")),
    ('❄', tr_key("emoji::Snowflake")),
    ('🌈', tr_key("emoji::Rainbow")),
    ('☁', tr_key("emoji::Cloud")),
    ('🌊', tr_key("emoji::Wave")),
    ('🏔', tr_key("emoji::Mountain")),
    ('🌍', tr_key("emoji::Globe")),
    ('🐶', tr_key("emoji::Dog")),
    ('🐱', tr_key("emoji::Cat")),
    ('🐻', tr_key("emoji::Bear")),
    ('🦊', tr_key("emoji::Fox")),
    ('🐦', tr_key("emoji::Bird")),
    ('🦋', tr_key("emoji::Butterfly")),
    ('🐟', tr_key("emoji::Fish")),
    ('🐢', tr_key("emoji::Turtle")),
    ('☕', tr_key("emoji::Coffee")),
    ('🍎', tr_key("emoji::Apple")),
    ('🍕', tr_key("emoji::Pizza")),
    ('🍰', tr_key("emoji::Cake")),
    ('🍷', tr_key("emoji::Wine")),
    ('🍔', tr_key("emoji::Burger")),
    ('❤', tr_key("emoji::Heart")),
    ('🧡', tr_key("emoji::Orange Heart")),
    ('💛', tr_key("emoji::Yellow Heart")),
    ('💚', tr_key("emoji::Green Heart")),
    ('💙', tr_key("emoji::Blue Heart")),
    ('💜', tr_key("emoji::Purple Heart")),
    ('🖤', tr_key("emoji::Black Heart")),
    ('✅', tr_key("emoji::Check Mark")),
    ('❌', tr_key("emoji::Cross Mark")),
    ('❓', tr_key("emoji::Question Mark")),
    ('❗', tr_key("emoji::Exclamation Mark")),
    ('🚩', tr_key("emoji::Flag")),
    ('🔔', tr_key("emoji::Bell")),
    ('🔖', tr_key("emoji::Bookmark")),
    ('🏷', tr_key("emoji::Label")),
    ('💬', tr_key("emoji::Speech Bubble")),
    ('💎', tr_key("emoji::Gem")),
    ('👑', tr_key("emoji::Crown")),
    ('🎯', tr_key("emoji::Target")),
    ('🔴', tr_key("emoji::Red Circle")),
    ('🟠', tr_key("emoji::Orange Circle")),
    ('🟡', tr_key("emoji::Yellow Circle")),
    ('🟢', tr_key("emoji::Green Circle")),
    ('🔵', tr_key("emoji::Blue Circle")),
    ('🟣', tr_key("emoji::Purple Circle")),
    ('⚫', tr_key("emoji::Black Circle")),
    ('⚪', tr_key("emoji::White Circle")),
];

/// The catalogue key of a curated emoji's name; show it with `tr`.
pub fn name(c: char) -> Option<&'static str> {
    CURATED.iter().find(|(e, _)| *e == c).map(|(_, n)| *n)
}

/// Folder and smart folder ids mapped to their emoji. Saved as is in
/// falcon's app data.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(transparent)]
pub struct FolderEmoji(BTreeMap<String, String>);

impl FolderEmoji {
    /// The folder's emoji. A stored value that is not a curated emoji
    /// (from a newer falcon, or hand-edited) is ignored.
    pub fn get(&self, id: &str) -> Option<char> {
        let mut chars = self.0.get(id)?.chars();
        let c = chars.next()?;
        (chars.next().is_none() && name(c).is_some()).then_some(c)
    }

    /// Gives the folder an emoji. Anything outside the curated list is
    /// refused.
    pub fn set(&mut self, id: &str, emoji: char) -> bool {
        if name(emoji).is_none() {
            return false;
        }
        self.0.insert(id.to_string(), emoji.to_string());
        true
    }

    /// Removes the folder's emoji, so its named icon shows again.
    pub fn clear(&mut self, id: &str) {
        self.0.remove(id);
    }

    /// Whether any folder has an emoji.
    #[cfg(test)]
    pub fn any(&self) -> bool {
        !self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_emoji_are_unique_and_named() {
        let mut seen = std::collections::HashSet::new();
        for (c, n) in CURATED {
            assert!(seen.insert(*c), "{c} twice");
            assert!(!n.is_empty());
        }
        let names: std::collections::HashSet<_> = CURATED.iter().map(|(_, n)| n).collect();
        assert_eq!(names.len(), CURATED.len(), "a name is used twice");
        assert!((90..=130).contains(&CURATED.len()));
    }

    #[test]
    fn set_and_clear() {
        let mut m = FolderEmoji::default();
        assert!(m.set("F1", '🔥'));
        assert_eq!(m.get("F1"), Some('🔥'));
        assert!(m.set("F1", '🌱'));
        assert_eq!(m.get("F1"), Some('🌱'));
        m.clear("F1");
        assert_eq!(m.get("F1"), None);
        assert!(!m.any());
        assert!(!m.set("F1", 'a'));
        assert_eq!(m.get("F1"), None);
    }

    #[test]
    fn round_trips_through_serde() {
        let mut m = FolderEmoji::default();
        m.set("F1", '🔥');
        m.set("S1", '📁');
        let json = serde_json::to_string(&m).unwrap();
        let back: FolderEmoji = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.get("S1"), Some('📁'));
    }

    #[test]
    fn saved_state_without_the_field_loads_and_keeps_it_when_present() {
        let saved: crate::app::Saved = serde_json::from_str("{}").unwrap();
        assert!(!saved.folder_emoji.any());
        let saved: crate::app::Saved = serde_json::from_str(r#"{"folder_emoji":{"F1":"🔥"}}"#).unwrap();
        assert_eq!(saved.folder_emoji.get("F1"), Some('🔥'));
    }

    #[test]
    fn unknown_values_are_ignored() {
        let m: FolderEmoji = serde_json::from_str(r#"{"a":"x","b":"","c":"🔥🔥","d":"🫠","e":"🔥"}"#).unwrap();
        assert_eq!(m.get("a"), None);
        assert_eq!(m.get("b"), None);
        assert_eq!(m.get("c"), None);
        assert_eq!(m.get("d"), None);
        assert_eq!(m.get("e"), Some('🔥'));
        assert_eq!(m.get("missing"), None);
    }

    #[test]
    fn clearing_one_folder_leaves_the_others() {
        let mut m = FolderEmoji::default();
        m.set("F1", '🔥');
        m.set("F2", '🌱');
        m.clear("F1");
        assert_eq!(m.get("F1"), None);
        assert_eq!(m.get("F2"), Some('🌱'));
    }
}
