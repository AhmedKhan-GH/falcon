//! Help ▸ Export Debug Report: a zip with a plain text summary of the app,
//! the machine and the open library, the library's index files and the
//! error log. Facts are gathered into [`Info`] and written by [`render`],
//! which is pure so it can be tested.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The library files copied into the report, and listed in its Library
/// section.
pub const LIBRARY_FILES: [&str; 4] = ["metadata.json", "mtime.json", "tags.json", "saved-filters.json"];

/// What the report calls the system's version line.
const OS_NAME: &str = if cfg!(windows) { "Windows" } else { "macOS" };

/// What the report says about the machine.
#[derive(Debug, Clone, PartialEq)]
pub struct System {
    /// The system's version.
    pub macos: String,
    pub build: String,
    pub model: String,
    pub cpu: String,
    pub cpus: usize,
    pub memory_gb: String,
}

/// One of the library's JSON files.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub name: String,
    /// Size in KB, or `None` when the file is missing.
    pub kb: Option<u64>,
    pub writable: bool,
}

/// The open library.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryInfo {
    pub path: String,
    pub items: usize,
    pub folders: usize,
    pub tags: usize,
    pub files: Vec<FileInfo>,
}

/// Everything the report states.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub created: String,
    pub version: String,
    pub system: System,
    /// Window size in points, and egui's pixels per point.
    pub window: (f32, f32),
    pub pixels_per_point: f32,
    pub library: Option<LibraryInfo>,
    pub other_libraries: Vec<String>,
    pub recent: Vec<String>,
}

/// The report text: sections with a title line and `key: value` lines.
pub fn render(info: &Info) -> String {
    let mut out = String::new();
    let mut section = |title: &str, lines: Vec<String>| {
        out.push_str(title);
        out.push('\n');
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
        out.push('\n');
    };
    section("Falcon Debug Report", vec![format!("created: {}", info.created)]);
    section("App", vec![format!("version: {}", info.version)]);
    let s = &info.system;
    section(
        "System",
        vec![
            format!("{OS_NAME}: {}", s.macos),
            format!("build: {}", s.build),
            format!("model: {}", s.model),
            format!("cpu: {}", s.cpu),
            format!("logical cpus: {}", s.cpus),
            format!("memory: {} GB", s.memory_gb),
        ],
    );
    section(
        "Display",
        vec![format!("window: {:.0} x {:.0} points", info.window.0, info.window.1), format!("pixels per point: {}", info.pixels_per_point)],
    );
    let mut lib = Vec::new();
    match &info.library {
        None => lib.push("no library is open".to_string()),
        Some(l) => {
            lib.push(format!("path: {}", l.path));
            lib.push(format!("items: {}", l.items));
            lib.push(format!("folders: {}", l.folders));
            lib.push(format!("tags: {}", l.tags));
            for f in &l.files {
                lib.push(match f.kb {
                    Some(kb) => format!("{}: {kb} KB, {}", f.name, if f.writable { "writable" } else { "read-only" }),
                    None => format!("{}: missing", f.name),
                });
            }
        }
    }
    for p in &info.other_libraries {
        lib.push(format!("also open: {p}"));
    }
    for p in &info.recent {
        lib.push(format!("recent: {p}"));
    }
    section("Library", lib);
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// A command's trimmed output, or "unknown" if it fails.
fn run(program: &str, args: &[&str]) -> String {
    let mut command = Command::new(program);
    command.args(args);
    // Without this a console flashes up for the command in a release build.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The folder a report is offered to, the Desktop.
#[cfg(not(windows))]
pub fn desktop() -> Option<PathBuf> {
    Some(falcon_library::paths::home_dir()?.join("Desktop"))
}

/// The folder a report is offered to, the Desktop. Windows can keep it away
/// from the home folder, as OneDrive does, so the system is asked.
#[cfg(windows)]
pub fn desktop() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    let mut path = std::ptr::null_mut();
    // SAFETY: the out-pointer is filled with a NUL-terminated string on
    // success, freed here with the allocator the call documents.
    let found = unsafe {
        let result = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT as u32, std::ptr::null_mut(), &mut path);
        let found = (result >= 0).then(|| {
            let len = (0..).take_while(|&i| *path.add(i) != 0).count();
            PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(path, len)))
        });
        CoTaskMemFree(path.cast());
        found
    };
    found.or_else(|| Some(falcon_library::paths::home_dir()?.join("Desktop")))
}

/// Asks the system about the machine.
#[cfg(windows)]
pub fn system() -> System {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // `ver` prints "Microsoft Windows [Version 10.0.26200.1234]".
    let line = run("cmd", &["/C", "ver"]);
    let version = line.rsplit_once(' ').map_or(line.as_str(), |(_, v)| v).trim_end_matches(']').to_string();
    let mut memory = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: the struct's size is set as the call requires.
    let bytes = (unsafe { GlobalMemoryStatusEx(&mut memory) } != 0).then_some(memory.ullTotalPhys);
    System {
        build: version.split('.').nth(2).unwrap_or("unknown").to_string(),
        macos: version,
        model: "unknown".to_string(),
        cpu: std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".to_string()),
        cpus: std::thread::available_parallelism().map_or(0, |n| n.get()),
        memory_gb: bytes.map_or_else(|| "unknown".to_string(), |b| format!("{:.1}", b as f64 / (1u64 << 30) as f64)),
    }
}

/// Asks the system about the machine.
#[cfg(not(windows))]
pub fn system() -> System {
    let bytes: Option<u64> = run("sysctl", &["-n", "hw.memsize"]).parse().ok();
    System {
        macos: run("sw_vers", &["-productVersion"]),
        build: run("sw_vers", &["-buildVersion"]),
        model: run("sysctl", &["-n", "hw.model"]),
        cpu: run("sysctl", &["-n", "machdep.cpu.brand_string"]),
        cpus: std::thread::available_parallelism().map_or(0, |n| n.get()),
        memory_gb: bytes.map_or_else(|| "unknown".to_string(), |b| format!("{:.1}", b as f64 / (1u64 << 30) as f64)),
    }
}

/// Size and writability of the library's JSON files.
pub fn library_files(root: &Path) -> Vec<FileInfo> {
    LIBRARY_FILES
        .iter()
        .map(|name| {
            let meta = std::fs::metadata(root.join(name)).ok();
            FileInfo { name: name.to_string(), kb: meta.as_ref().map(|m| m.len().div_ceil(1024)), writable: meta.is_some_and(|m| !m.permissions().readonly()) }
        })
        .collect()
}

/// Puts the report's files in `dir`: the text, the library's files that
/// exist and the error log (as `falcon.log`).
pub fn stage(dir: &Path, text: &str, library: Option<&Path>, log: Option<&Path>) -> std::io::Result<()> {
    std::fs::write(dir.join("debug-report.txt"), text)?;
    if let Some(root) = library {
        for name in LIBRARY_FILES {
            let from = root.join(name);
            if from.is_file() {
                std::fs::copy(&from, dir.join(name))?;
            }
        }
    }
    if let Some(log) = log.filter(|l| l.is_file()) {
        std::fs::copy(log, dir.join("falcon.log"))?;
    }
    Ok(())
}

/// Writes the report zip to `dest`, using a fresh temp folder that is
/// removed afterwards.
pub fn export(info: &Info, library: Option<&Path>, dest: &Path) -> Result<(), String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir: PathBuf = std::env::temp_dir().join(format!("falcon-debug-report-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        stage(&dir, &render(info), library, crate::errlog::path().as_deref()).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(dest);
        zip_folder(&dir, dest)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[cfg(target_os = "macos")]
fn zip_folder(dir: &Path, dest: &Path) -> Result<(), String> {
    let out = Command::new("/usr/bin/ditto").arg("-c").arg("-k").arg(dir).arg(dest).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn zip_folder(dir: &Path, dest: &Path) -> Result<(), String> {
    fn add(zip: &mut zip::ZipWriter<std::fs::File>, root: &Path, dir: &Path) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                add(zip, root, &path)?;
            } else {
                // Names in a zip use forward slashes on every system.
                let name = path.strip_prefix(root).map_err(std::io::Error::other)?.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                zip.start_file(name, zip::write::SimpleFileOptions::default())?;
                std::io::copy(&mut std::fs::File::open(&path)?, zip)?;
            }
        }
        Ok(())
    }
    let mut zip = zip::ZipWriter::new(std::fs::File::create(dest).map_err(|e| e.to_string())?);
    add(&mut zip, dir, dir).map_err(|e| e.to_string())?;
    zip.finish().map(drop).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> Info {
        Info {
            created: "2026-11-01 12:00:00".into(),
            version: "0.1.0".into(),
            system: System { macos: "15.1".into(), build: "24B83".into(), model: "Mac14,2".into(), cpu: "Apple M2".into(), cpus: 8, memory_gb: "16.0".into() },
            window: (1280.0, 800.0),
            pixels_per_point: 2.0,
            library: Some(LibraryInfo {
                path: "/Users/me/Pictures/Test.library".into(),
                items: 120,
                folders: 7,
                tags: 31,
                files: vec![
                    FileInfo { name: "metadata.json".into(), kb: Some(12), writable: true },
                    FileInfo { name: "mtime.json".into(), kb: Some(4), writable: false },
                    FileInfo { name: "tags.json".into(), kb: None, writable: false },
                ],
            }),
            other_libraries: vec!["/Users/me/Pictures/Two.library".into()],
            recent: vec!["/Users/me/Pictures/Old.library".into()],
        }
    }

    #[test]
    fn the_report_has_titled_sections() {
        let text = render(&info());
        let expected = "\
Falcon Debug Report
created: 2026-11-01 12:00:00

App
version: 0.1.0

System
macOS: 15.1
build: 24B83
model: Mac14,2
cpu: Apple M2
logical cpus: 8
memory: 16.0 GB

Display
window: 1280 x 800 points
pixels per point: 2

Library
path: /Users/me/Pictures/Test.library
items: 120
folders: 7
tags: 31
metadata.json: 12 KB, writable
mtime.json: 4 KB, read-only
tags.json: missing
also open: /Users/me/Pictures/Two.library
recent: /Users/me/Pictures/Old.library
";
        assert_eq!(text, expected.replace("macOS:", &format!("{OS_NAME}:")));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_zip_names_its_files_with_forward_slashes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("in");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("a.txt"), "a").unwrap();
        let dest = tmp.path().join("out.zip");
        zip_folder(&dir, &dest).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        assert_eq!(zip.by_index(0).unwrap().name().unwrap(), "sub/a.txt");
    }

    #[cfg(windows)]
    #[test]
    fn the_system_reports_a_version_and_memory() {
        let s = system();
        assert!(s.macos.starts_with(|c: char| c.is_ascii_digit()), "{s:?}");
        assert!(s.build.parse::<u32>().is_ok(), "{s:?}");
        assert!(s.memory_gb.parse::<f64>().is_ok(), "{s:?}");
        assert!(desktop().is_some());
    }

    #[test]
    fn without_a_library_the_section_says_so() {
        let text = render(&Info { library: None, other_libraries: vec![], recent: vec![], ..info() });
        assert!(text.ends_with("Library\nno library is open\n"));
    }

    #[test]
    fn unknown_facts_are_shown_as_unknown() {
        let mut i = info();
        i.system.model = "unknown".into();
        assert!(render(&i).contains("model: unknown\n"));
    }

    #[test]
    fn staging_copies_only_what_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("L.library");
        std::fs::create_dir(&lib).unwrap();
        std::fs::write(lib.join("metadata.json"), "{}").unwrap();
        std::fs::write(lib.join("tags.json"), "[]").unwrap();
        let log = tmp.path().join("log.txt");
        std::fs::write(&log, "Falcon error log\n").unwrap();
        let out = tmp.path().join("out");
        std::fs::create_dir(&out).unwrap();
        stage(&out, "report", Some(&lib), Some(&log)).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["debug-report.txt", "falcon.log", "metadata.json", "tags.json"]);
        assert_eq!(std::fs::read_to_string(out.join("falcon.log")).unwrap(), "Falcon error log\n");
    }

    #[test]
    fn staging_without_a_library_or_log() {
        let tmp = tempfile::tempdir().unwrap();
        stage(tmp.path(), "report", None, Some(&tmp.path().join("missing.log"))).unwrap();
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn library_files_report_size_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("metadata.json"), vec![b'x'; 1500]).unwrap();
        let files = library_files(tmp.path());
        assert_eq!(files.len(), 4);
        assert_eq!(files[0], FileInfo { name: "metadata.json".into(), kb: Some(2), writable: true });
        assert_eq!(files[1].kb, None);
    }
}
