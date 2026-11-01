//! The IFC rows of the item inspector's Properties (`model3d/ifc_facts.rs`).

use eframe::egui::Ui;
use falcon_library::Library;
use falcon_library::model::Item;

use crate::app::App;
use crate::locale::tr;
use crate::model3d::ifc_facts::fact_rows;
use crate::ui::widgets::prop;

impl App {
    /// Products shown and skipped, triangles and schema for an `.ifc` item,
    /// or why it has no 3D preview.
    pub(crate) fn ifc_props(&mut self, ui: &mut Ui, lib: &Library, item: &Item) {
        if !item.ext.eq_ignore_ascii_case("ifc") {
            return;
        }
        let key = format!("{}:{}", item.id, item.size);
        let facts = self.model3d.ifc.get(ui.ctx(), &key, item.original_path(lib.root()));
        for (label, value) in fact_rows(facts) {
            prop(ui, tr(label), &value);
        }
    }
}
