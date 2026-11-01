//! The Find menu's commands: Search All, Filter Sidebar, Clear Filter and
//! opening one filter of the filter bar.
use super::App;
use crate::search_state::FilterKind;
use falcon_library::search::Filter;

impl App {
    /// Search All: open the All page (leaving the viewer) and focus the search box.
    pub(crate) fn search_all(&mut self) {
        self.ws.viewer = None;
        self.go(crate::logic::Scope::All);
        self.focus_search = true;
    }

    /// Filter Sidebar: show the sidebar and focus its folder filter box.
    pub(crate) fn focus_filter_sidebar(&mut self) {
        self.show_sidebar = true;
        self.focus_folder_filter = true;
    }

    /// Clear Filter: every filter rule off and the Lock released, as
    /// Eagle's reset does (the search text stays).
    pub(crate) fn reset_filter(&mut self) {
        self.ws.filter_ui.locked = false;
        self.ws.filter = Filter::default();
        self.clear_media_filter_text();
        self.ws.color_text.clear();
        self.ws.note_text.clear();
        self.ws.url_text.clear();
    }

    /// Shows the filter bar with one filter's menu open.
    pub(crate) fn open_filter(&mut self, kind: FilterKind) {
        self.ws.show_filter = true;
        self.ws.filter_ui.open_chip = Some(kind);
    }
}
