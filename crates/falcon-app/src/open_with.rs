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

/// Opens each file in its default app. Never reads the files.
pub fn open_default(files: &[PathBuf]) {
    if !files.is_empty() {
        platform::open_default(files);
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
        platform::open_url(args);
    }
}

#[cfg(windows)]
pub use platform::shell_open;

/// Opens the files in `app`.
pub fn open_in(app: &Path, files: &[PathBuf]) {
    if !files.is_empty() {
        platform::open_in(app, files);
    }
}

/// Lets the system ask which app opens `file`, and open it there, where it
/// has its own window for that. False where it hasn't, for the caller to ask.
pub fn choose_app(file: &Path) -> bool {
    platform::choose_app(file)
}

/// Whether [`choose_app`] does anything here.
pub const SYSTEM_CHOOSER: bool = cfg!(windows);

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
        platform::reveal(files);
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

/// `path` as a `file://` address, with everything but unreserved characters
/// and `/` percent-encoded, so it is also safe inside a quoted D-Bus string.
#[cfg(unix)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::{Path, PathBuf};

    use objc2::runtime::NSObjectProtocol;
    use objc2::sel;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    use super::{AppInfo, app_name, open_args, reveal_args};

    fn run_open(args: Vec<std::ffi::OsString>) {
        std::thread::spawn(move || {
            let _ = std::process::Command::new("/usr/bin/open").args(args).status();
        });
    }

    pub fn open_default(files: &[PathBuf]) {
        run_open(open_args(files, None));
    }

    pub fn open_url(args: Vec<std::ffi::OsString>) {
        run_open(args);
    }

    pub fn open_in(app: &Path, files: &[PathBuf]) {
        run_open(open_args(files, Some(app)));
    }

    pub fn reveal(files: &[PathBuf]) {
        run_open(reveal_args(files));
    }

    pub fn choose_app(_: &Path) -> bool {
        false
    }

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

#[cfg(windows)]
mod platform {
    use std::ffi::{OsStr, OsString};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::ptr;

    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize};
    use windows_sys::Win32::UI::Shell::{OAIF_ALLOW_REGISTRATION, OAIF_EXEC, OPENASINFO, SHOpenWithDialog, ShellExecuteW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::{AppInfo, MAX_OPEN, MAX_REVEAL};

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// `path` as the shell wants it: no `\\?\` prefix, only backslashes.
    fn native(path: &Path) -> PathBuf {
        falcon_library::sandbox::shown(path).components().collect()
    }

    /// Runs `work` on a short-lived thread with COM set up the way the shell
    /// functions ask, so the UI never waits on them.
    fn on_shell_thread(work: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            // SAFETY: COM is initialised and released on this thread alone.
            let started = unsafe { CoInitializeEx(ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) };
            work();
            if started >= 0 {
                // SAFETY: paired with the call above.
                unsafe { CoUninitialize() };
            }
        });
    }

    /// Hands `target`, a file or an address, to the shell, which opens it in
    /// the app registered for it. No command line is involved, so nothing in
    /// the name can be read as anything else.
    pub fn shell_open(target: OsString) {
        on_shell_thread(move || {
            let (verb, target) = (wide(OsStr::new("open")), wide(&target));
            // SAFETY: both strings are NUL-terminated and outlive the call.
            unsafe { ShellExecuteW(ptr::null_mut(), verb.as_ptr(), target.as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL) };
        });
    }

    pub fn open_default(files: &[PathBuf]) {
        for f in files.iter().take(MAX_OPEN) {
            shell_open(native(f).into());
        }
    }

    pub fn open_url(args: Vec<OsString>) {
        if let Some(url) = args.into_iter().last() {
            shell_open(url);
        }
    }

    pub fn open_in(app: &Path, files: &[PathBuf]) {
        let (app, files) = (native(app), files.iter().take(MAX_OPEN).map(|f| native(f)).collect::<Vec<_>>());
        std::thread::spawn(move || {
            let _ = Command::new(app).args(files).stdout(Stdio::null()).stderr(Stdio::null()).status();
        });
    }

    /// Selects a file in File Explorer. Explorer takes one file and opens a
    /// window for it, so a selection shows its first file in each folder.
    pub fn reveal(files: &[PathBuf]) {
        let explorer = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("explorer.exe"), |r| PathBuf::from(r).join("explorer.exe"));
        let mut folders = Vec::new();
        for f in files.iter().take(MAX_REVEAL) {
            let f = native(f);
            if folders.contains(&f.parent().map(Path::to_path_buf)) {
                continue;
            }
            folders.push(f.parent().map(Path::to_path_buf));
            // Explorer reads its own command line, and only understands the
            // quotes around the path, not around the whole argument.
            let mut arg = OsString::from("/select,\"");
            arg.push(&f);
            arg.push("\"");
            let _ = Command::new(&explorer).raw_arg(arg).spawn();
        }
    }

    pub fn apps(_: &Path) -> (Option<AppInfo>, Vec<AppInfo>) {
        (None, Vec::new())
    }

    /// Asks with the system's own Open With window, which does the opening.
    pub fn choose_app(file: &Path) -> bool {
        let file = wide(native(file).as_os_str());
        on_shell_thread(move || {
            let info = OPENASINFO { pcszFile: file.as_ptr(), pcszClass: ptr::null(), oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_EXEC };
            // SAFETY: the file name is NUL-terminated and outlives the call,
            // which returns when the window closes.
            unsafe { SHOpenWithDialog(ptr::null_mut(), &info) };
        });
        true
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::{AppInfo, MAX_OPEN, MAX_REVEAL, file_uri};

    /// `xdg-open` takes one target at a time, unlike macOS's `open`.
    fn xdg_open(target: OsString) {
        std::thread::spawn(move || {
            let _ = Command::new("xdg-open").arg(target).stdout(Stdio::null()).stderr(Stdio::null()).status();
        });
    }

    pub fn open_default(files: &[PathBuf]) {
        for f in files.iter().take(MAX_OPEN) {
            xdg_open(f.into());
        }
    }

    pub fn open_url(args: Vec<OsString>) {
        if let Some(url) = args.into_iter().last() {
            xdg_open(url);
        }
    }

    pub fn open_in(app: &Path, files: &[PathBuf]) {
        let (app, files) = (app.to_path_buf(), files.iter().take(MAX_OPEN).cloned().collect::<Vec<_>>());
        std::thread::spawn(move || {
            let _ = Command::new(app).args(files).stdout(Stdio::null()).stderr(Stdio::null()).status();
        });
    }

    /// Selects the files in the file manager through the freedesktop
    /// FileManager1 interface, or opens their folder where nothing answers it.
    pub fn reveal(files: &[PathBuf]) {
        let files: Vec<PathBuf> = files.iter().take(MAX_REVEAL).cloned().collect();
        std::thread::spawn(move || {
            let uris = files.iter().map(|f| format!("'{}'", file_uri(f))).collect::<Vec<_>>().join(",");
            let shown = Command::new("gdbus")
                .args(["call", "--session", "--dest", "org.freedesktop.FileManager1", "--object-path", "/org/freedesktop/FileManager1"])
                .args(["--method", "org.freedesktop.FileManager1.ShowItems", &format!("[{uris}]"), ""])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !shown && let Some(parent) = files.first().and_then(|f| f.parent()) {
                xdg_open(parent.into());
            }
        });
    }

    pub fn apps(_: &Path) -> (Option<AppInfo>, Vec<AppInfo>) {
        (None, Vec::new())
    }

    pub fn choose_app(_: &Path) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn file_addresses_encode_what_could_break_out() {
        assert_eq!(file_uri(Path::new("/l/a.png")), "file:///l/a.png");
        assert_eq!(file_uri(Path::new("/l/it's a é.png")), "file:///l/it%27s%20a%20%C3%A9.png");
    }

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
