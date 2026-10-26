mod app;
mod loader;
mod logic;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Falcon")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native("Falcon", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, initial)))))
}
