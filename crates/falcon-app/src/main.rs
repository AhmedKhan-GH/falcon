mod app;
mod icons;
mod loader;
mod logic;
mod theme;

use std::path::PathBuf;

fn main() -> eframe::Result {
    // `falcon-app [LIBRARY] [ITEM]`, where ITEM is an id or Eagle's
    // `eagle://item/<id>` link, opened in the viewer.
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    // Later arguments are an `eagle://item/<id>` link (or bare id) to open,
    // or files to import, as when files are opened with the app.
    let mut open_items = Vec::new();
    let mut import = Vec::new();
    for a in std::env::args().skip(2) {
        if std::path::Path::new(&a).exists() {
            import.push(PathBuf::from(a));
        } else {
            open_items.push(a.trim_start_matches("eagle://item/").to_string());
        }
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Falcon")
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native("Falcon", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, initial, open_items, import)))))
}
