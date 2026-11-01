//! Screen capture into the library, after Eagle's: an area or a window
//! picked with the system's own crosshair (`/usr/sbin/screencapture -i`
//! and `-w`), or the whole screen under the pointer. The picture is
//! saved as PNG, then turned into the format and quality chosen in
//! Preferences ▸ Screenshot (and halved when @2x pictures are off on a
//! Retina screen), named "Screenshot - <date> <time>", and handed to the
//! app, which imports it (tagged "Screenshot" unless that is off) or asks
//! where to save it in the collect window first (`App::capture_tick`).
//!
//! The capture runs on a worker thread through a [`Shooter`], which runs
//! `screencapture` in the app and is replaced by a fake in tests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use falcon_library::export;

/// What to capture: Eagle's modes 1 (area), 2 (window) and 3 (full screen).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    Area,
    Window,
    Full,
}

impl Mode {
    /// The menu bar icon's item for this mode.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Area => "Screenshot of an Area",
            Mode::Window => "Screenshot of a Window",
            Mode::Full => "Screenshot of the Screen",
        }
    }
}

/// The format a screenshot is saved in (Eagle's `screencapture.format`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Png,
    Jpg,
    Webp,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Png, Format::Jpg, Format::Webp];

    pub fn label(self) -> &'static str {
        self.export().label()
    }

    pub fn ext(self) -> &'static str {
        self.export().ext()
    }

    fn export(self) -> export::Format {
        match self {
            Format::Png => export::Format::Png,
            Format::Jpg => export::Format::Jpeg,
            Format::Webp => export::Format::Webp,
        }
    }
}

/// Eagle's three qualities for JPG and WebP screenshots: High, Medium and
/// Low.
pub const QUALITIES: [(u8, &str); 3] = [(90, crate::locale::tr_key("High")), (80, crate::locale::tr_key("Medium")), (70, crate::locale::tr_key("Low"))];
/// Eagle's default quality (High).
pub const DEFAULT_QUALITY: u8 = 90;

/// The tag Eagle gives screenshots.
pub const TAG: &str = "Screenshot";

/// What a capture does with its picture, from the Screenshot preferences.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Options {
    pub format: Format,
    /// 1 to 100, for JPG and WebP.
    pub quality: u8,
    /// Keep the full pixel size on a Retina screen (Eagle's `useRetina`).
    pub retina: bool,
    /// Also put the picture on the clipboard (Eagle's
    /// `autoWriteClipboard`); done by the worker after [`take`].
    pub clipboard: bool,
}

/// The arguments for `screencapture` writing a PNG to `path`. The area
/// and window modes are silent and leave out the window shadow, as
/// Eagle's are; the full-screen capture keeps the system's shutter sound
/// (Eagle plays its own sound there and shows no notice) and takes the
/// screen numbered `display` (1 is the main one).
pub fn args(mode: Mode, path: &Path, display: usize) -> Vec<OsString> {
    let mut a: Vec<OsString> = match mode {
        Mode::Area => vec!["-x".into(), "-i".into(), "-o".into()],
        Mode::Window => vec!["-x".into(), "-w".into(), "-o".into()],
        Mode::Full => vec!["-D".into(), display.max(1).to_string().into()],
    };
    a.extend(["-t".into(), "png".into(), path.as_os_str().to_owned()]);
    a
}

/// The item name Eagle gives a screenshot: "Screenshot - 2026-11-01 09.05.07".
pub fn item_name(now: chrono::NaiveDateTime) -> String {
    format!("{TAG} - {}", now.format("%Y-%m-%d %H.%M.%S"))
}

/// The tags a screenshot is imported with.
pub fn tags(auto_tag: bool) -> Vec<String> {
    if auto_tag { vec![TAG.to_string()] } else { Vec::new() }
}

/// Whether the picture is halved: @2x pictures are off and the screen is
/// a Retina one (Eagle checks for a scale of 2).
pub fn halves(opts: &Options, scale: f32) -> bool {
    !opts.retina && scale >= 1.5
}

/// A path in `dir` for `stem.ext` that no file has yet (" 2", " 3"… are
/// added, so two screenshots in one second both survive).
pub fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} {n}.{ext}"));
        n += 1;
    }
    p
}

/// Turns the PNG `screencapture` wrote into the file to import, named
/// `name` beside it: converted to the chosen format and quality, and
/// halved by [`halves`]. A PNG kept at full size is only renamed.
pub fn finish_file(png: &Path, opts: &Options, scale: f32, name: &str) -> Result<PathBuf, String> {
    let dir = png.parent().ok_or("no folder")?;
    let out = free_path(dir, name, opts.format.ext());
    let half = halves(opts, scale);
    if opts.format == Format::Png && !half {
        std::fs::rename(png, &out).map_err(|e| e.to_string())?;
        return Ok(out);
    }
    let longest = if half {
        let (w, h) = image::image_dimensions(png).map_err(|e| e.to_string())?;
        Some((w.max(h) / 2).max(1))
    } else {
        None
    };
    let bytes = export::convert_bytes(png, &export::Convert { format: opts.format.export(), quality: opts.quality.clamp(1, 100), longest })?;
    std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(png);
    Ok(out)
}

/// A finished screenshot, ready to import.
#[derive(Clone, PartialEq, Debug)]
pub struct Shot {
    pub mode: Mode,
    pub path: PathBuf,
    /// The item's name ([`item_name`]).
    pub name: String,
}

/// Runs `screencapture` with the given arguments and waits for it.
pub type Shooter = Arc<dyn Fn(&[OsString]) -> std::io::Result<()> + Send + Sync>;

/// The real [`Shooter`].
pub fn system_shooter() -> Shooter {
    Arc::new(|args: &[OsString]| {
        let status = std::process::Command::new("/usr/sbin/screencapture").args(args).status()?;
        // Esc while picking exits non-zero (or zero, depending on the
        // macOS version) without a file; the missing file says it.
        let _ = status;
        Ok(())
    })
}

/// Where the screen is, for one capture: its scale (2 on Retina) and its
/// number for `screencapture -D`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Screen {
    pub scale: f32,
    pub display: usize,
}

impl Default for Screen {
    fn default() -> Self {
        Screen { scale: 1.0, display: 1 }
    }
}

/// Takes one screenshot into `dir`: `Ok(None)` when it was cancelled
/// (Esc while picking leaves no file).
pub fn take(mode: Mode, opts: &Options, screen: Screen, dir: &Path, now: chrono::NaiveDateTime, shoot: &Shooter) -> Result<Option<Shot>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp = now.and_utc().timestamp_nanos_opt().unwrap_or_default();
    let raw = dir.join(format!(".capture-{}-{stamp}.png", std::process::id()));
    shoot(&args(mode, &raw, screen.display)).map_err(|e| format!("Couldn't run screencapture: {e}"))?;
    if !raw.is_file() {
        return Ok(None);
    }
    let name = item_name(now);
    let path = finish_file(&raw, opts, screen.scale, &name).inspect_err(|_| {
        let _ = std::fs::remove_file(&raw);
    })?;
    Ok(Some(Shot { mode, path, name }))
}

/// The capture worker: one capture at a time, as in Eagle (a second
/// request while one is picking is ignored); results wait in `done` for
/// the app's next frame.
#[derive(Default)]
pub struct Captures {
    busy: Arc<AtomicBool>,
    done: Arc<Mutex<Vec<Result<Shot, String>>>>,
    /// Replaces `screencapture` (tests).
    pub shooter: Option<Shooter>,
}

impl Captures {
    /// Starts a capture on a worker thread; `wake` runs when it ends.
    /// Returns false when one is already running.
    pub fn start(&self, mode: Mode, opts: Options, screen: Screen, dir: PathBuf, wake: impl Fn() + Send + 'static) -> bool {
        if self.busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        let shoot = self.shooter.clone().unwrap_or_else(system_shooter);
        let (busy, done) = (self.busy.clone(), self.done.clone());
        std::thread::spawn(move || {
            let now = chrono::Local::now().naive_local();
            let result = take(mode, &opts, screen, &dir, now, &shoot);
            if opts.clipboard
                && let Ok(Some(shot)) = &result
                && let Err(e) = copy_to_clipboard(&shot.path)
            {
                crate::errlog::error(&format!("screenshot: couldn't copy it to the clipboard: {e}"));
            }
            match result {
                Ok(Some(shot)) => done.lock().unwrap_or_else(|p| p.into_inner()).push(Ok(shot)),
                Ok(None) => {}
                Err(e) => done.lock().unwrap_or_else(|p| p.into_inner()).push(Err(e)),
            }
            busy.store(false, Ordering::SeqCst);
            wake();
        });
        true
    }

    #[cfg(test)]
    fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// The captures finished since the last call.
    pub fn take_done(&self) -> Vec<Result<Shot, String>> {
        std::mem::take(&mut *self.done.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// The screen under the pointer.
pub fn pointer_screen() -> Screen {
    platform::pointer_screen()
}

/// Puts the screenshot on the clipboard as a picture (Eagle's "Also copy
/// the screenshot to clipboard").
pub fn copy_to_clipboard(path: &Path) -> Result<(), String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let data = arboard::ImageData { width: w as usize, height: h as usize, bytes: std::borrow::Cow::Owned(img.into_raw()) };
    arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};

    use super::Screen;

    pub fn pointer_screen() -> Screen {
        let Some(mtm) = MainThreadMarker::new() else { return Screen::default() };
        let at = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        for (i, s) in screens.iter().enumerate() {
            let f = s.frame();
            let inside = at.x >= f.origin.x && at.x < f.origin.x + f.size.width && at.y >= f.origin.y && at.y < f.origin.y + f.size.height;
            if inside {
                return Screen { scale: s.backingScaleFactor() as f32, display: i + 1 };
            }
        }
        NSScreen::mainScreen(mtm).map_or_else(Screen::default, |s| Screen { scale: s.backingScaleFactor() as f32, display: 1 })
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn pointer_screen() -> super::Screen {
        super::Screen::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(path: &Path, w: u32, h: u32) {
        image::RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255])).save(path).unwrap();
    }

    fn at() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap().and_hms_opt(9, 5, 7).unwrap()
    }

    #[test]
    fn arguments_follow_the_mode() {
        let p = Path::new("/tmp/x.png");
        let s = |m, d| args(m, p, d).into_iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        assert_eq!(s(Mode::Area, 1), "-x -i -o -t png /tmp/x.png");
        assert_eq!(s(Mode::Window, 1), "-x -w -o -t png /tmp/x.png");
        assert_eq!(s(Mode::Full, 2), "-D 2 -t png /tmp/x.png");
        assert_eq!(s(Mode::Full, 0), "-D 1 -t png /tmp/x.png");
    }

    #[test]
    fn names_and_tags() {
        assert_eq!(item_name(at()), "Screenshot - 2026-11-01 09.05.07");
        assert_eq!(tags(true), ["Screenshot"]);
        assert!(tags(false).is_empty());
    }

    #[test]
    fn converts_and_halves() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw.png");
        png(&raw, 40, 20);
        let opts = Options { format: Format::Jpg, quality: 80, retina: false, clipboard: false };
        let out = finish_file(&raw, &opts, 2.0, "Shot").unwrap();
        assert_eq!(out, dir.path().join("Shot.jpg"));
        assert!(!raw.exists());
        assert_eq!(image::image_dimensions(&out).unwrap(), (20, 10));
        // A PNG at full size is only renamed; a second one gets " 2".
        png(&raw, 40, 20);
        let keep = Options { format: Format::Png, quality: 90, retina: true, clipboard: false };
        std::fs::write(dir.path().join("Shot.png"), b"x").unwrap();
        let out = finish_file(&raw, &keep, 2.0, "Shot").unwrap();
        assert_eq!(out, dir.path().join("Shot 2.png"));
        assert_eq!(image::image_dimensions(&out).unwrap(), (40, 20));
    }

    #[test]
    fn take_runs_the_shooter_and_reports_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let opts = Options { format: Format::Png, quality: 90, retina: true, clipboard: false };
        let writes: Shooter = Arc::new(|a: &[OsString]| {
            png(Path::new(a.last().unwrap()), 4, 4);
            Ok(())
        });
        let shot = take(Mode::Window, &opts, Screen::default(), dir.path(), at(), &writes).unwrap().unwrap();
        assert_eq!(shot.name, "Screenshot - 2026-11-01 09.05.07");
        assert_eq!(shot.path, dir.path().join("Screenshot - 2026-11-01 09.05.07.png"));
        let cancels: Shooter = Arc::new(|_: &[OsString]| Ok(()));
        assert_eq!(take(Mode::Area, &opts, Screen::default(), dir.path(), at(), &cancels), Ok(None));
    }

    #[test]
    fn one_capture_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let rx = Mutex::new(rx);
        let slow: Shooter = Arc::new(move |a: &[OsString]| {
            let _ = rx.lock().unwrap().recv();
            png(Path::new(a.last().unwrap()), 2, 2);
            Ok(())
        });
        let c = Captures { shooter: Some(slow), ..Default::default() };
        let opts = Options { format: Format::Png, quality: 90, retina: true, clipboard: false };
        let (wtx, wrx) = std::sync::mpsc::channel();
        assert!(c.start(Mode::Area, opts, Screen::default(), dir.path().into(), move || wtx.send(()).unwrap()));
        assert!(!c.start(Mode::Area, opts, Screen::default(), dir.path().into(), || {}));
        tx.send(()).unwrap();
        wrx.recv().unwrap();
        let done = c.take_done();
        assert_eq!(done.len(), 1);
        assert!(done[0].as_ref().is_ok_and(|s| s.path.is_file()));
        assert!(!c.busy());
    }

    #[test]
    fn halves_only_without_retina_on_a_retina_screen() {
        let on = Options { format: Format::Png, quality: 90, retina: true, clipboard: false };
        let off = Options { retina: false, ..on };
        assert!(!halves(&on, 2.0));
        assert!(!halves(&on, 1.0));
        assert!(halves(&off, 2.0));
        assert!(!halves(&off, 1.0));
    }

    #[test]
    fn free_path_numbers_taken_names() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_path(dir.path(), "S", "png"), dir.path().join("S.png"));
        std::fs::write(dir.path().join("S.png"), b"x").unwrap();
        assert_eq!(free_path(dir.path(), "S", "png"), dir.path().join("S 2.png"));
        std::fs::write(dir.path().join("S 2.png"), b"x").unwrap();
        assert_eq!(free_path(dir.path(), "S", "png"), dir.path().join("S 3.png"));
        assert_eq!(free_path(dir.path(), "S", "jpg"), dir.path().join("S.jpg"));
    }

    #[test]
    fn format_extensions() {
        assert_eq!(Format::Png.ext(), "png");
        assert_eq!(Format::Jpg.ext(), "jpg");
        assert_eq!(Format::Webp.ext(), "webp");
    }

    #[test]
    fn webp_conversion_keeps_the_size() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw.png");
        png(&raw, 40, 20);
        let opts = Options { format: Format::Webp, quality: 80, retina: true, clipboard: false };
        let out = finish_file(&raw, &opts, 2.0, "Shot").unwrap();
        assert_eq!(out, dir.path().join("Shot.webp"));
        assert!(!raw.exists());
        assert_eq!(image::image_dimensions(&out).unwrap(), (40, 20));
    }

    #[test]
    fn jpg_keeps_the_size_with_retina_on() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw.png");
        png(&raw, 40, 20);
        let opts = Options { format: Format::Jpg, quality: 90, retina: true, clipboard: false };
        let out = finish_file(&raw, &opts, 2.0, "Shot").unwrap();
        assert_eq!(out, dir.path().join("Shot.jpg"));
        assert_eq!(image::image_dimensions(&out).unwrap(), (40, 20));
    }

    #[test]
    fn a_shooter_error_is_an_error_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let opts = Options { format: Format::Png, quality: 90, retina: true, clipboard: false };
        let fails: Shooter = Arc::new(|_: &[OsString]| Err(std::io::Error::other("no screencapture")));
        let r = take(Mode::Area, &opts, Screen::default(), dir.path(), at(), &fails);
        assert!(r.is_err_and(|e| e.contains("no screencapture")));
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(left.is_empty(), "stray files: {left:?}");
    }

    #[test]
    fn qualities_are_eagles_three() {
        assert_eq!(QUALITIES.map(|q| q.0), [90, 80, 70]);
        assert_eq!(DEFAULT_QUALITY, QUALITIES[0].0);
    }
}
