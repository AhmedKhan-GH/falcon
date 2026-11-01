//! The window's parts, drawn with egui painters. Each module draws one
//! region (sidebar, toolbar, filter bar, grid, inspector, viewer) or one
//! family of dialogs or menus, as `impl App` blocks over the state in
//! `crate::app` and the feature modules at the crate root.

pub(crate) mod widgets;
pub(crate) mod toast;
