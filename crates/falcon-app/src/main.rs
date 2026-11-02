mod actions;
mod anim;
mod api_check;
mod app;
mod applock;
mod backups;
mod capture;
mod clip_image;
mod collect;
mod colorspace;
mod colors;
mod dock;
mod dock_progress;
mod drop_pos;
mod debug_report;
mod errlog;
mod folder_icons;
mod font_install;
mod gesture;
mod audio;
mod auto_import;
mod fsevents;
mod hotkeys;
mod hover;
mod hover_media;
mod urls;
mod link_import;
mod icloud;
mod icons;
mod install_dir;
mod links;
mod login_item;
mod folder_picker;
mod import_dupes;
mod loader;
mod locale;
mod logic;
mod menu;
mod notify;
mod open_files;
mod model3d;
mod open_with;
mod prefs;
mod progress_eta;
mod share;
mod quick_switch;
mod search_state;
mod recent;
mod shell;
mod shortcuts;
mod sounds;
mod smart_count;
mod subtitles;
mod storage;
mod tag_view;
mod tags;
mod tasks;
mod organize;
mod text_cards;
mod textdoc;
mod psd_status;
mod tiff_pages;
mod toast;
mod touch_id;
mod tray;
mod theme;
mod ui;
mod library_icons;
mod watch;
mod web_archive;
mod webview;
mod workspaces;

use std::path::PathBuf;

fn main() -> eframe::Result {
    errlog::install_panic_hook();
    // Only libraries in the sandbox folder open, never real ones.
    falcon_library::sandbox::enforce(falcon_library::sandbox::default_root());
    // Before the event loop, so the link that launched the app arrives.
    links::install();
    // Files opened with the app from Finder or dropped on its Dock icon.
    open_files::install();
    // `falcon-app [LIBRARY] [ITEM]`, where ITEM is an id or Eagle's
    // `eagle://item/<id>` link, opened in the viewer.
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    // Later arguments are `eagle://item/<id>` links (or bare ids) to open,
    // or files to import, as when files are opened with the app. Options
    // set the starting view, for scripted screenshots:
    //   --select        select the given items instead of opening them
    //   --search TEXT   start with this search
    //   --color HEX     start with this colour filter
    //   --tags          start on the tag manager
    let mut launch = app::Launch::default();
    let mut args = std::env::args().skip(2);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--select" => launch.select = true,
            "--tags" => launch.tags = true,
            "--search" => launch.search = args.next(),
            "--color" => launch.color = args.next(),
            _ if std::path::Path::new(&a).exists() => launch.import.push(PathBuf::from(a)),
            _ => match links::parse(&a) {
                Some(links::Link::Item(id)) => launch.items.push(id),
                Some(link) => launch.links.push(link),
                None => launch.items.push(a),
            },
        }
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Falcon")
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "Falcon",
        options,
        Box::new(move |cc| {
            let app = app::App::new(cc, initial, launch);
            // After `App::new` has installed the fonts and default visuals.
            theme::set_theme(&cc.egui_ctx, prefs::saved_theme(cc.storage));
            Ok(Box::new(app))
        }),
    )
}
