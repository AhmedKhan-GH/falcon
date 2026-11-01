//! Opening originals in other apps, as Eagle's "Open in Default App" and
//! "Open With" do.
//!
//! Files open through `/usr/bin/open`, as Eagle itself does for "Open
//! With" (`open -a APP FILE`): it needs no permission, returns at once, and
//! hands the file to LaunchServices, so a dataless iCloud original is
//! fetched by the app that opens it, never read here. `open` runs on a
//! short-lived thread so the UI never waits and no zombie is left.
//!
//! The apps that can open a file come from `NSWorkspace
//! URLsForApplicationsToOpenURL:` (macOS 12 and later), with the default
//! from `URLForApplicationToOpenURL:`. LaunchServices answers from the
//! file's type, so it is asked about an empty stand-in with the same
//! extension (never the original) and the list is cached per extension.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::locale::trf;

/// Eagle opens at most this many files at once.
pub const MAX_OPEN: usize = 40;
/// Files shown at once in Finder: each one can open its own Finder window,
/// so a large selection would bury the desktop in windows.
pub const MAX_REVEAL: usize = 31;

/// An app that can open a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    /// Shown name, without `.app`.
    pub name: String,
    /// The app bundle.
    pub path: PathBuf,
}

/// One row of the Open With menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    pub app: AppInfo,
    pub default: bool,
}

impl AppEntry {
    /// The menu label: the default app is marked, as in Eagle.
    pub fn label(&self) -> String {
        default_label(&self.app.name, self.default)
    }
}

pub fn default_label(name: &str, default: bool) -> String {
    if default { trf("{name} (Default)", &[("name", &name)]) } else { name.to_string() }
}

/// The shown name of an app bundle: its file name without `.app`.
pub fn app_name(bundle: &Path) -> String {
    let name = bundle.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name.strip_suffix(".app").map(String::from).unwrap_or(name)
}

/// The Open With list: the default app first, then the others by name,
/// each bundle once, and the app named by `skip` (falcon itself) left out.
/// Two bundles with the same name (an app in /Applications and a copy
/// elsewhere) keep only the first, which is the default when it is one.
pub fn compose(default: Option<AppInfo>, all: Vec<AppInfo>, skip: Option<&Path>) -> Vec<AppEntry> {
    let mut rest: Vec<AppInfo> = all.into_iter().filter(|a| Some(a.path.as_path()) != default.as_ref().map(|d| d.path.as_path())).collect();
    rest.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.path.cmp(&b.path)));
    let mut out: Vec<AppEntry> = Vec::new();
    let entries = default.into_iter().map(|app| AppEntry { app, default: true }).chain(rest.into_iter().map(|app| AppEntry { app, default: false }));
    for e in entries {
        if Some(e.app.path.as_path()) == skip || out.iter().any(|o| o.app.path == e.app.path || o.app.name == e.app.name) {
            continue;
        }
        out.push(e);
    }
    out
}

/// The `open` arguments for `files`, in `app` or each in its default app;
/// at most `MAX_OPEN` files.
pub fn open_args(files: &[PathBuf], app: Option<&Path>) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    if let Some(a) = app {
        args.push("-a".into());
        args.push(a.into());
    }
    // `--` so a file name starting with '-' is never taken for an option.
    args.push("--".into());
    args.extend(files.iter().take(MAX_OPEN).map(|f| f.as_os_str().to_owned()));
    args
}

/// The `open` arguments that show `files` selected in Finder.
pub fn reveal_args(files: &[PathBuf]) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec!["-R".into(), "--".into()];
    args.extend(files.iter().take(MAX_REVEAL).map(|f| f.as_os_str().to_owned()));
    args
}

fn run_open(args: Vec<std::ffi::OsString>) {
    std::thread::spawn(move || {
        let _ = std::process::Command::new("/usr/bin/open").args(args).status();
    });
}

/// Opens each file in its default app. Never reads the files.
pub fn open_default(files: &[PathBuf]) {
    if !files.is_empty() {
        run_open(open_args(files, None));
    }
}

/// The `open` arguments for a web link, or None unless it is an http or
/// https address (so a link in a note can't open a file or run an app).
pub fn url_args(url: &str) -> Option<Vec<std::ffi::OsString>> {
    let lower = url.to_ascii_lowercase();
    let web = (lower.starts_with("http://") || lower.starts_with("https://")) && !url.chars().any(char::is_whitespace);
    web.then(|| vec!["--".into(), url.into()])
}

/// Opens a web link in the default browser.
pub fn open_url(url: &str) {
    if let Some(args) = url_args(url) {
        run_open(args);
    }
}

/// Opens the files in `app`.
pub fn open_in(app: &Path, files: &[PathBuf]) {
    if !files.is_empty() {
        run_open(open_args(files, Some(app)));
    }
}

/// Shows the files in macOS Quick Look (`qlmanage -p`), which stays open
/// until closed. Never reads the files itself.
pub fn quick_look(files: &[PathBuf]) {
    if files.is_empty() || !cfg!(target_os = "macos") {
        return;
    }
    let mut args: Vec<std::ffi::OsString> = vec!["-p".into()];
    args.extend(files.iter().take(MAX_OPEN).map(|f| f.as_os_str().to_owned()));
    std::thread::spawn(move || {
        use std::process::Stdio;
        let _ = std::process::Command::new("/usr/bin/qlmanage").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

/// Shows the files in Finder.
pub fn reveal(files: &[PathBuf]) {
    if !files.is_empty() {
        run_open(reveal_args(files));
    }
}

/// The app bundle falcon runs from, if it runs from one.
fn own_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// An empty stand-in with `path`'s extension in a private temp folder.
/// LaunchServices answers nothing for a missing file, and asking about the
/// original could touch an iCloud file that isn't local (Eagle also asks
/// about an empty temp file).
fn probe(path: &Path) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("falcon-open-with");
    std::fs::create_dir_all(&dir).ok()?;
    let ext: String = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default().chars().filter(|c| c.is_alphanumeric()).collect();
    let p = dir.join(if ext.is_empty() { "probe".to_string() } else { format!("probe.{ext}") });
    if !p.exists() {
        std::fs::write(&p, b"").ok()?;
    }
    Some(p)
}

/// The apps for `path`'s file type, uncached.
pub fn apps_for(path: &Path) -> Vec<AppEntry> {
    let Some(p) = probe(path) else { return Vec::new() };
    let (default, all) = platform::apps(&p);
    compose(default, all, own_bundle().as_deref())
}

/// Open With lists by lower-cased extension, so menus redrawn every frame
/// ask LaunchServices once.
#[derive(Default)]
pub struct AppCache {
    by_ext: HashMap<String, Vec<AppEntry>>,
}

impl AppCache {
    pub fn get(&mut self, path: &Path) -> &[AppEntry] {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        self.by_ext.entry(ext).or_insert_with(|| apps_for(path))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::{Path, PathBuf};

    use objc2::runtime::NSObjectProtocol;
    use objc2::sel;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    use super::{AppInfo, app_name};

    fn info(url: &NSURL) -> Option<AppInfo> {
        let path = PathBuf::from(url.path()?.to_string());
        Some(AppInfo { name: app_name(&path), path })
    }

    /// The default app and every app that can open `path`. Only the file
    /// name's type matters; the file is not read.
    pub fn apps(path: &Path) -> (Option<AppInfo>, Vec<AppInfo>) {
        let ws = NSWorkspace::sharedWorkspace();
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let default = ws.URLForApplicationToOpenURL(&url).and_then(|u| info(&u));
        // URLsForApplicationsToOpenURL: arrived in macOS 12; before that only
        // the default is offered.
        let all = if ws.respondsToSelector(sel!(URLsForApplicationsToOpenURL:)) {
            ws.URLsForApplicationsToOpenURL(&url).iter().filter_map(|u| info(&u)).collect()
        } else {
            Vec::new()
        };
        (default, all)
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    use super::AppInfo;

    pub fn apps(_: &Path) -> (Option<AppInfo>, Vec<AppInfo>) {
        (None, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_links_open() {
        assert_eq!(url_args("https://a.b/c"), Some(vec!["--".into(), "https://a.b/c".into()]));
        assert!(url_args("HTTP://a.b").is_some());
        assert!(url_args("file:///etc/passwd").is_none());
        assert!(url_args("/Applications/Calculator.app").is_none());
        assert!(url_args("https://a b").is_none());
    }

    fn app(name: &str, path: &str) -> AppInfo {
        AppInfo { name: name.into(), path: path.into() }
    }

    #[test]
    fn default_first_then_by_name_each_once() {
        let all = vec![
            app("zed", "/Applications/Zed.app"),
            app("Preview", "/System/Applications/Preview.app"),
            app("Acrobat", "/Applications/Acrobat.app"),
            app("Preview", "/System/Applications/Preview.app"),
            app("Falcon", "/Applications/Falcon.app"),
            app("Acrobat", "/Volumes/Old/Acrobat.app"),
        ];
        let out = compose(Some(app("Preview", "/System/Applications/Preview.app")), all, Some(Path::new("/Applications/Falcon.app")));
        let labels: Vec<String> = out.iter().map(AppEntry::label).collect();
        assert_eq!(labels, ["Preview (Default)", "Acrobat", "zed"]);
        assert_eq!(out[1].app.path, Path::new("/Applications/Acrobat.app"));
        // Without a default the list is just sorted.
        let out = compose(None, vec![app("b", "/b.app"), app("A", "/a.app")], None);
        assert_eq!(out.iter().map(|e| e.label()).collect::<Vec<_>>(), ["A", "b"]);
        assert!(compose(None, Vec::new(), None).is_empty());
    }

    #[test]
    fn names_and_labels() {
        assert_eq!(app_name(Path::new("/Applications/Visual Studio Code.app")), "Visual Studio Code");
        assert_eq!(app_name(Path::new("/opt/tool")), "tool");
        assert_eq!(default_label("Preview", true), "Preview (Default)");
        assert_eq!(default_label("Preview", false), "Preview");
    }

    #[test]
    fn open_arguments() {
        let files: Vec<PathBuf> = vec!["/l/images/A.info/-odd.png".into(), "/l/images/B.info/b.pdf".into()];
        let s = |v: Vec<std::ffi::OsString>| v.into_iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(s(open_args(&files, None)), ["--", "/l/images/A.info/-odd.png", "/l/images/B.info/b.pdf"]);
        assert_eq!(s(open_args(&files[1..], Some(Path::new("/Applications/Preview.app")))), ["-a", "/Applications/Preview.app", "--", "/l/images/B.info/b.pdf"]);
        assert_eq!(s(reveal_args(&files[..1])), ["-R", "--", "/l/images/A.info/-odd.png"]);
        let many: Vec<PathBuf> = (0..100).map(|n| PathBuf::from(format!("/f{n}"))).collect();
        assert_eq!(open_args(&many, None).len(), MAX_OPEN + 1);
        assert_eq!(reveal_args(&many).len(), MAX_REVEAL + 2);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_services_lists_apps_by_type_without_the_file() {
        // The original need not exist: an empty stand-in is asked about.
        let out = apps_for(Path::new("/nonexistent/falcon-test/notes.txt"));
        assert!(!out.is_empty(), "every Mac has an app for text");
        assert!(out.iter().filter(|e| e.default).count() <= 1);
        if let Some(first) = out.first() {
            assert!(first.default || out.iter().all(|e| !e.default));
        }
        let mut seen = std::collections::HashSet::new();
        assert!(out.iter().all(|e| seen.insert(e.app.path.clone())));
    }
}
