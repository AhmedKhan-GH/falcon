//! The message in the middle of a grid with nothing to show, and on the
//! All Tags page when there are no tags. What it says depends on the view:
//! a drop hint where files can be added, "No Matches" while searching or
//! filtering, and a note for each smart view.

use eframe::egui::text::LayoutJob;
use eframe::egui::{self, Align, Rect, TextFormat, Ui};
use falcon_library::Library;

use super::{App, icon};
use crate::icons as ic;
use crate::logic::Scope;
use crate::theme::{self as t, font};

/// Which view is empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    All,
    Recent,
    Random,
    Folder { has_children: bool, locked: bool },
    Smart,
    Uncategorized,
    Untagged,
    Trash,
    Other,
}

/// What to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Empty {
    pub icon: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    /// Show the Import Folders… button.
    pub import: bool,
}

const fn say(icon: &'static str, title: &'static str, body: &'static str, import: bool) -> Option<Empty> {
    Some(Empty { icon, title, body, import })
}

/// The message for an empty view, or none when the view has its own
/// content. `searching` is a search query or any active filter;
/// `library_items` counts the items outside the Trash.
pub(crate) fn message(view: View, searching: bool, library_items: usize) -> Option<Empty> {
    if searching {
        return say(ic::MAGNIFYING_GLASS, "No Matches", "Try another keyword or loosen the filters.", false);
    }
    match view {
        View::All | View::Recent | View::Random => {
            say(ic::IMAGES, "Drop Files Here", "Drag files or folders onto this window to add them to the library.", true)
        }
        View::Folder { has_children: false, locked: false } => {
            say(ic::FOLDER, "Drop Files Here", "Drag files onto this window to add them to this folder.", true)
        }
        View::Folder { .. } => None,
        View::Smart => say(ic::FUNNEL, "No Matches", "No items match this smart folder's rules.", false),
        View::Uncategorized if library_items == 0 => {
            say(ic::FOLDER_DASHED, "Uncategorized Items", "Items that aren't in any folder gather here, ready to be sorted.", false)
        }
        View::Uncategorized => say(ic::FOLDER_DASHED, "All Sorted", "Every item is in a folder.", false),
        View::Untagged if library_items == 0 => {
            say(ic::TAG_SIMPLE, "Untagged Items", "Items without tags gather here, ready to be tagged.", false)
        }
        View::Untagged => say(ic::TAG_SIMPLE, "All Tagged", "Every item has a tag.", false),
        View::Trash => say(
            ic::TRASH,
            "Trash Is Empty",
            "Deleted items wait here until you restore them or delete them for good.",
            false,
        ),
        View::Other => say(ic::IMAGES, "No Items", "", false),
    }
}

const ICON_SIZE: f32 = 44.0;
const BODY_WIDTH: f32 = 360.0;
const BUTTON_SIZE: egui::Vec2 = egui::Vec2::new(160.0, 26.0);

/// Draws the icon, title and body centred in `area`, a little above its
/// middle. Returns the centre for a button under the text.
fn paint(ui: &Ui, area: Rect, e: &Empty) -> egui::Pos2 {
    let title = ui.painter().layout_no_wrap(e.title.to_string(), font(17.0), t::p().text);
    let body = (!e.body.is_empty()).then(|| {
        let mut job = LayoutJob::default();
        job.wrap.max_width = BODY_WIDTH;
        job.halign = Align::Center;
        job.append(e.body, 0.0, TextFormat::simple(font(13.0), t::p().text_muted));
        ui.painter().layout_job(job)
    });
    let button = if e.import { 16.0 + BUTTON_SIZE.y } else { 0.0 };
    let total = ICON_SIZE + 14.0 + title.size().y + body.as_ref().map_or(0.0, |b| 6.0 + b.size().y) + button;
    let mut y = area.center().y - total / 2.0 - 20.0;
    let x = area.center().x;
    icon(ui, egui::pos2(x, y + ICON_SIZE / 2.0), e.icon, ICON_SIZE, t::p().text_muted.gamma_multiply(0.6));
    y += ICON_SIZE + 14.0;
    ui.painter().galley(egui::pos2(x - title.size().x / 2.0, y), title.clone(), t::p().text);
    y += title.size().y;
    if let Some(b) = body {
        y += 6.0;
        ui.painter().galley(egui::pos2(x - b.size().x / 2.0, y), b.clone(), t::p().text_muted);
        y += b.size().y;
    }
    egui::pos2(x, y + 16.0 + BUTTON_SIZE.y / 2.0)
}

impl App {
    /// Says why the open view is empty, with an Import Folders… button
    /// where files can be added.
    pub(crate) fn empty_view(&mut self, ui: &mut Ui, lib: &Library, area: Rect) {
        let view = match &self.ws.scope {
            Scope::All => View::All,
            Scope::Recent => View::Recent,
            Scope::Random(_) => View::Random,
            Scope::Folder(id) => {
                let found = crate::logic::find_folder(&lib.metadata.folders, id);
                View::Folder {
                    has_children: found.is_some_and(|f| !f.children.is_empty()),
                    locked: found.is_some_and(|f| crate::organize::is_shut(f, &self.ws.org.unlocked)),
                }
            }
            Scope::Smart(_) => View::Smart,
            Scope::Uncategorized => View::Uncategorized,
            Scope::Untagged => View::Untagged,
            Scope::Trash => View::Trash,
            _ => View::Other,
        };
        let searching = !self.ws.query.trim().is_empty() || self.ws.filter.is_active();
        let live = lib.items().iter().filter(|i| !i.is_deleted).count();
        let Some(e) = message(view, searching, live) else { return };
        let at = paint(ui, area, &e);
        if e.import {
            self.empty_import_button(ui, at);
        }
    }

    /// The All Tags page's message when it has no tags to list, or none
    /// match the page's filter.
    pub(crate) fn empty_tags(&self, ui: &Ui, area: Rect, filtering: bool) {
        let e = if filtering {
            Empty { icon: ic::MAGNIFYING_GLASS, title: "No Matches", body: "Try another keyword.", import: false }
        } else {
            Empty {
                icon: ic::TAG_SIMPLE,
                title: "No Tags Yet",
                body: "Tags help you organize items and find them again in search.",
                import: false,
            }
        };
        paint(ui, area, &e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(view: View, searching: bool, n: usize) -> Option<&'static str> {
        message(view, searching, n).map(|e| e.title)
    }

    #[test]
    fn searching_wins_in_every_view() {
        for v in [View::All, View::Trash, View::Smart, View::Uncategorized, View::Other, View::Folder { has_children: true, locked: true }] {
            let e = message(v, true, 5).unwrap();
            assert_eq!(e.title, "No Matches");
            assert_eq!(e.body, "Try another keyword or loosen the filters.");
            assert!(!e.import);
        }
    }

    #[test]
    fn drop_hints_offer_import() {
        for v in [View::All, View::Recent, View::Random] {
            let e = message(v, false, 0).unwrap();
            assert_eq!(e.title, "Drop Files Here");
            assert!(e.import);
        }
        let f = message(View::Folder { has_children: false, locked: false }, false, 0).unwrap();
        assert_eq!(f.body, "Drag files onto this window to add them to this folder.");
        assert!(f.import);
    }

    #[test]
    fn folders_with_subfolders_or_locks_say_nothing() {
        assert_eq!(message(View::Folder { has_children: true, locked: false }, false, 0), None);
        assert_eq!(message(View::Folder { has_children: false, locked: true }, false, 0), None);
    }

    #[test]
    fn smart_views_follow_the_library() {
        assert_eq!(title(View::Smart, false, 3), Some("No Matches"));
        assert_eq!(title(View::Uncategorized, false, 0), Some("Uncategorized Items"));
        assert_eq!(title(View::Uncategorized, false, 2), Some("All Sorted"));
        assert_eq!(title(View::Untagged, false, 0), Some("Untagged Items"));
        assert_eq!(title(View::Untagged, false, 2), Some("All Tagged"));
        assert_eq!(title(View::Trash, false, 2), Some("Trash Is Empty"));
        let o = message(View::Other, false, 0).unwrap();
        assert_eq!((o.title, o.body), ("No Items", ""));
    }
}
