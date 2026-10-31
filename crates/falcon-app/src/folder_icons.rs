//! Eagle's folder icon names and colours mapped onto falcon's look: each
//! name in `model::FOLDER_ICONS` gets the closest Phosphor Light glyph
//! (Eagle's own artwork is not copied), and each colour key its dark-theme
//! value from `theme`.

use eframe::egui::Color32;
use falcon_library::model::{FOLDER_ICONS, ICON_COLORS};

use crate::icons as ic;
use crate::theme as t;

/// Eagle icon name → Phosphor Light glyph. Numbers past nine have no glyph
/// and are drawn as plain digits (see [`glyph`]).
const GLYPHS: &[(&str, &str)] = &[
    ("library", "\u{E758}"), // books
    ("box", "\u{E390}"), // package
    ("grid", "\u{E464}"), // squares-four
    ("layer", "\u{E466}"), // stack
    ("briefcase", "\u{E0EE}"), // briefcase
    ("photo", "\u{E2CA}"), // image
    ("photos", "\u{E836}"), // images
    ("video", "\u{E4DA}"), // video-camera
    ("film", "\u{E792}"), // film-strip
    ("film2", "\u{E8C2}"), // film-slate
    ("film3", "\u{E8C0}"), // film-reel
    ("music", "\u{E340}"), // music-notes
    ("book", "\u{E0E2}"), // book
    ("book2", "\u{E0E6}"), // book-open
    ("bookshelf", "\u{E0E4}"), // book-bookmark
    ("keynote", "\u{E654}"), // presentation
    ("camera", "\u{E10E}"), // camera
    ("aperture", "\u{E00A}"), // aperture
    ("attachment", "\u{E39A}"), // paperclip
    ("scissors", "\u{EAE0}"), // scissors
    ("palette", "\u{E6C8}"), // palette
    ("wrench", "\u{E5D4}"), // wrench
    ("helmet", "\u{ED46}"), // hard-hat
    ("life-buoy", "\u{E63A}"), // lifebuoy
    ("graph", "\u{EB58}"), // graph
    ("graph2", "\u{E67C}"), // tree-structure
    ("tableware", "\u{E262}"), // fork-knife
    ("cog", "\u{E270}"), // gear
    ("bachelor-cap", "\u{E62C}"), // graduation-cap
    ("cones", "\u{E9A8}"), // traffic-cone
    ("dribbble", "\u{E20E}"), // dribbble-logo
    ("email", "\u{E214}"), // envelope
    ("business-card", "\u{E2C8}"), // identification-card
    ("coffee", "\u{E1C2}"), // coffee
    ("cart", "\u{E41E}"), // shopping-cart
    ("lightbulb", "\u{E2DC}"), // lightbulb
    ("inspiration", "\u{E63C}"), // lightbulb-filament
    ("thumb-up", "\u{E48E}"), // thumbs-up
    ("thumb-down", "\u{E48C}"), // thumbs-down
    ("like", "\u{E2A8}"), // heart
    ("unlike", "\u{EBE8}"), // heart-break
    ("star", "\u{E46A}"), // star
    ("hot", "\u{E242}"), // fire
    ("upload", "\u{E4C0}"), // upload-simple
    ("download", "\u{E20C}"), // download-simple
    ("paid", "\u{E550}"), // currency-dollar
    ("free", "\u{E478}"), // tag
    ("medical", "\u{E56E}"), // first-aid
    ("shield", "\u{E40A}"), // shield
    ("search", "\u{E30C}"), // magnifying-glass
    ("shortcuts", "\u{E2DE}"), // lightning
    ("recycle", "\u{E75A}"), // recycle
    ("excalmation", "\u{E4E2}"), // warning-circle
    ("question", "\u{E3E8}"), // question
    ("coin1", "\u{E60E}"), // coin
    ("coin2", "\u{E78E}"), // coins
    ("coin3", "\u{E554}"), // currency-eur
    ("coin4", "\u{E55A}"), // currency-jpy
    ("wallet", "\u{E68A}"), // wallet
    ("watch", "\u{E4E6}"), // watch
    ("clock", "\u{E19A}"), // clock
    ("calendar-week", "\u{E10A}"), // calendar-blank
    ("calendar-month", "\u{E108}"), // calendar
    ("website", "\u{E288}"), // globe
    ("phone", "\u{E1E0}"), // device-mobile
    ("tablet", "\u{E1E6}"), // device-tablet
    ("desktop", "\u{E560}"), // desktop
    ("tv", "\u{E754}"), // television
    ("cpu", "\u{E610}"), // cpu
    ("safari", "\u{E1C8}"), // compass
    ("chrome", "\u{E976}"), // google-chrome-logo
    ("pie", "\u{E158}"), // chart-pie
    ("bar-chart", "\u{E150}"), // chart-bar
    ("line-chart", "\u{E154}"), // chart-line
    ("2d", "\u{E45E}"), // square
    ("3d", "\u{E1DA}"), // cube
    ("contrast", "\u{E18C}"), // circle-half
    ("texture", "\u{E8C4}"), // checkerboard
    ("transition", "\u{E0A0}"), // arrows-left-right
    ("animation", "\u{E9FA}"), // spiral
    ("spectrogram", "\u{E802}"), // waveform
    ("font-sans", "\u{E6EE}"), // text-aa
    ("font-sans-serif", "\u{E48A}"), // text-t
    ("font-handwritten", "\u{E3AC}"), // pen-nib
    ("flag", "\u{E244}"), // flag
    ("earth", "\u{E28C}"), // globe-hemisphere-west
    ("pin", "\u{E316}"), // map-pin
    ("pin-check", "\u{E318}"), // map-pin-line
    ("map", "\u{E31A}"), // map-trifold
    ("road", "\u{E838}"), // road-horizon
    ("motor", "\u{E80A}"), // motorcycle
    ("rocket", "\u{E3FC}"), // rocket
    ("airplan", "\u{E002}"), // airplane
    ("ship", "\u{E786}"), // boat
    ("train", "\u{E496}"), // train
    ("car", "\u{E112}"), // car
    ("truck", "\u{E4B4}"), // truck
    ("water-drop", "\u{E210}"), // drop
    ("sun", "\u{E472}"), // sun
    ("moon", "\u{E330}"), // moon
    ("tree", "\u{E6DA}"), // tree
    ("mountain", "\u{E7AE}"), // mountains
    ("cloud", "\u{E1AA}"), // cloud
    ("bathtub", "\u{E81E}"), // bathtub
    ("door", "\u{E61C}"), // door
    ("bed", "\u{E0CC}"), // bed
    ("cabinet", "\u{E00C}"), // archive
    ("sofa", "\u{E012}"), // armchair
    ("home", "\u{E2C2}"), // house
    ("store", "\u{E470}"), // storefront
    ("house", "\u{E2C4}"), // house-line
    ("game", "\u{E26E}"), // game-controller
    ("weapon", "\u{E5BA}"), // sword
    ("armor", "\u{E40E}"), // shield-chevron
    ("award2", "\u{E320}"), // medal
    ("award", "\u{ECFC}"), // medal-military
    ("award3", "\u{E766}"), // certificate
    ("crown", "\u{E614}"), // crown
    ("rophy", "\u{E67E}"), // trophy
    ("gift", "\u{E276}"), // gift
    ("facebook", "\u{E226}"), // facebook-logo
    ("twitter", "\u{E4BA}"), // twitter-logo
    ("instagram", "\u{E2D0}"), // instagram-logo
    ("eye", "\u{E220}"), // eye
    ("bear", "\u{E648}"), // paw-print
    ("dog", "\u{E74A}"), // dog
    ("cat", "\u{E748}"), // cat
    ("man", "\u{E6E2}"), // gender-male
    ("woman", "\u{E6E0}"), // gender-female
    ("group", "\u{E68E}"), // users-three
    ("smile", "\u{E436}"), // smiley
    ("meh", "\u{E43A}"), // smiley-meh
    ("frown", "\u{E43E}"), // smiley-sad
    ("die", "\u{E916}"), // skull
    ("baby", "\u{E774}"), // baby
    ("kid", "\u{E72E}"), // person-simple
    ("angel", "\u{E6A2}"), // sparkle
    ("demon", "\u{EC62}"), // smiley-angry
    ("hand", "\u{E298}"), // hand
    ("brain", "\u{E74E}"), // brain
    ("number0", "\u{E388}"), // number-circle-zero
    ("number1", "\u{E36A}"), // number-circle-one
    ("number2", "\u{E382}"), // number-circle-two
    ("number3", "\u{E37C}"), // number-circle-three
    ("number4", "\u{E35E}"), // number-circle-four
    ("number5", "\u{E358}"), // number-circle-five
    ("number6", "\u{E376}"), // number-circle-six
    ("number7", "\u{E370}"), // number-circle-seven
    ("number8", "\u{E352}"), // number-circle-eight
    ("number9", "\u{E364}"), // number-circle-nine
];

const NUMBERS: [&str; 11] = ["10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20"];

/// The glyph for a stored `icon` name. No name, or one falcon doesn't
/// know, is the default folder icon (`open` picks its opened form).
pub fn glyph(name: Option<&str>, open: bool) -> &'static str {
    let default = if open { ic::FOLDER_OPEN } else { ic::FOLDER };
    let Some(name) = name.filter(|n| !n.is_empty()) else { return default };
    if let Some((_, g)) = GLYPHS.iter().find(|(n, _)| *n == name) {
        return g;
    }
    match name.strip_prefix("number").and_then(|n| n.parse::<usize>().ok()) {
        Some(n @ 10..=20) => NUMBERS[n - 10],
        _ => default,
    }
}

/// The glyph for a smart folder's `icon` name; the default is the smart
/// folder icon.
pub fn smart_glyph(name: Option<&str>) -> &'static str {
    match name.filter(|n| !n.is_empty()) {
        Some(n) if glyph(Some(n), false) != ic::FOLDER => glyph(Some(n), false),
        _ => ic::FOLDER_STAR,
    }
}

/// Whether a glyph is plain digits rather than an icon (drawn smaller).
pub fn is_text(glyph: &str) -> bool {
    glyph.chars().all(|c| c.is_ascii_digit())
}

/// The tint for a stored colour key; `None` (or an unknown key) is the
/// default text colour.
pub fn color(name: Option<&str>) -> Option<Color32> {
    Some(match name? {
        "red" => t::p().icon_red,
        "orange" => t::p().icon_orange,
        "yellow" => t::p().icon_yellow,
        "green" => t::p().icon_green,
        "aqua" => t::p().icon_aqua,
        "blue" => t::p().icon_blue,
        "purple" => t::p().icon_purple,
        "pink" => t::p().icon_pink,
        _ => return None,
    })
}

/// Every icon name in Eagle's picker order with its glyph.
pub fn all() -> impl Iterator<Item = (&'static str, &'static str)> {
    FOLDER_ICONS.iter().map(|n| (*n, glyph(Some(n), false)))
}

/// The colour keys in menu order with their tints.
pub fn colors() -> impl Iterator<Item = (&'static str, Color32)> {
    ICON_COLORS.iter().map(|c| (*c, color(Some(c)).unwrap_or(t::p().icon_none)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picker_icon_has_a_glyph() {
        for name in FOLDER_ICONS {
            let g = glyph(Some(name), false);
            assert!(!g.is_empty(), "{name}");
            assert_ne!(g, ic::FOLDER, "{name} falls back to the folder glyph");
        }
        assert_eq!(all().count(), FOLDER_ICONS.len());
        assert!(GLYPHS.iter().all(|(n, _)| FOLDER_ICONS.contains(n)), "table names must be Eagle's");
    }

    #[test]
    fn unknown_names_fall_back_to_the_folder() {
        assert_eq!(glyph(None, false), ic::FOLDER);
        assert_eq!(glyph(Some(""), true), ic::FOLDER_OPEN);
        assert_eq!(glyph(Some("no-such-icon"), false), ic::FOLDER);
        assert_eq!(glyph(Some("number21"), false), ic::FOLDER);
        assert_eq!(glyph(Some("number12"), false), "12");
        assert!(is_text("12") && !is_text(ic::FOLDER));
        assert_eq!(smart_glyph(None), ic::FOLDER_STAR);
        assert_eq!(smart_glyph(Some("rocket")), glyph(Some("rocket"), false));
    }

    #[test]
    fn colours_cover_every_key() {
        for c in ICON_COLORS {
            assert!(color(Some(c)).is_some(), "{c}");
        }
        assert_eq!(color(Some("teal")), None);
        assert_eq!(color(None), None);
        assert_eq!(colors().count(), 8);
    }
}
