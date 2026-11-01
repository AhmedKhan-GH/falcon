//! Export to Computer: copying originals out of the library into a folder
//! the user picked.
//!
//! A [`Plan`] is worked out first (where each original goes, which
//! directories to make), so the app can warn about files it would replace
//! before anything is written, and run the copy on another thread. Names
//! follow Eagle's export: an item keeps `<name>.<ext>`; names that clash
//! within one directory (ignoring case, as the Mac's file system does) are
//! numbered `<name>-1.<ext>`, `<name>-2.<ext>`, with the number padded to
//! the width of the largest one.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use filetime::FileTime;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::{Folder, Item};
use crate::smart::SmartFolder;

/// What to do when a file of the same name is already in the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collision {
    /// Overwrite it.
    Replace,
    /// Leave it and give the exported file a numbered name.
    KeepBoth,
}

/// One original to copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Everything an export will write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Directories to create, parents first.
    pub dirs: Vec<PathBuf>,
    pub steps: Vec<Step>,
    /// Where the export lands: the chosen folder, or the top of the
    /// exported folder tree.
    pub top: PathBuf,
    /// Whether steps may overwrite files (see [`Collision`]).
    pub overwrite: bool,
}

impl Plan {
    /// Steps whose destination already exists, which a `Replace` plan
    /// overwrites.
    pub fn clashes(&self) -> usize {
        self.steps.iter().filter(|s| s.to.exists()).count()
    }
}

/// An item's file name, `<name>.<ext>`, with any `/` (which no file name
/// can hold) turned into a space.
pub fn file_name(item: &Item) -> String {
    let name = item.name.replace(['/', '\0'], " ");
    if item.ext.is_empty() { name } else { format!("{name}.{}", item.ext) }
}

/// Splits a file name at its last dot (none for a leading dot).
fn stem_ext(file: &str) -> (&str, Option<&str>) {
    match file.rfind('.') {
        Some(i) if i > 0 => (&file[..i], Some(&file[i + 1..])),
        _ => (file, None),
    }
}

/// Chooses the names for `files` copied into one directory that already
/// holds `existing` (lower-cased). Names are compared ignoring case. A name
/// used once, and free, is kept. Otherwise every file of that name is
/// numbered, as Eagle does: `a.png` twice becomes `a-1.png` and `a-2.png`;
/// with `KeepBoth`, an `a.png` already there counts as the first, so a new
/// one becomes `a-2.png`. A numbered name that is taken is skipped.
pub fn name_files(files: &[String], existing: &HashSet<String>, how: Collision) -> Vec<String> {
    let keep = how == Collision::KeepBoth;
    let mut count: HashMap<String, usize> = HashMap::new();
    for f in files {
        *count.entry(f.to_lowercase()).or_default() += 1;
    }
    let mut taken: HashSet<String> = if keep { existing.clone() } else { HashSet::new() };
    // Names that stay as they are, reserved before any numbering so that
    // `a-1.png` in the batch is not handed to a second `a.png`.
    let plain: HashSet<String> = count.iter().filter(|(k, n)| **n == 1 && !taken.contains(*k)).map(|(k, _)| k.clone()).collect();
    taken.extend(plain.iter().cloned());
    let mut next: HashMap<String, usize> = HashMap::new();
    files
        .iter()
        .map(|f| {
            let lower = f.to_lowercase();
            if plain.contains(&lower) {
                return f.clone();
            }
            let had = usize::from(keep && existing.contains(&lower));
            let width = (count[&lower] + had).to_string().len();
            let n = next.entry(lower).or_insert(had);
            let (stem, ext) = stem_ext(f);
            loop {
                *n += 1;
                let candidate = match ext {
                    Some(e) => format!("{stem}-{:0width$}.{e}", *n),
                    None => format!("{stem}-{:0width$}", *n),
                };
                if taken.insert(candidate.to_lowercase()) {
                    return candidate;
                }
            }
        })
        .collect()
}

/// Lower-cased names of the entries in `dir` (none if it doesn't exist).
fn listing(dir: &Path) -> HashSet<String> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default()
}

/// Plans copying the originals of `ids` (in order, each once) straight
/// into `dest`. Unknown ids are skipped.
pub fn plan_items(lib: &Library, ids: &[String], dest: &Path, how: Collision) -> Plan {
    let mut seen = HashSet::new();
    let items: Vec<&Item> = ids.iter().filter(|id| seen.insert(id.as_str())).filter_map(|id| lib.item(id)).collect();
    let mut plan = Plan { dirs: vec![dest.to_path_buf()], steps: Vec::new(), top: dest.to_path_buf(), overwrite: how == Collision::Replace };
    add_steps(&mut plan, lib.root(), &items, dest, &listing(dest), how);
    plan
}

/// The directory name for a folder, cleaned up as Eagle does on the Mac:
/// tabs and runs of spaces become one space, `:` `/` and `\\` become
/// spaces, the ends are trimmed and a trailing dot is dropped. A name left
/// empty becomes "Untitled".
pub fn folder_dir_name(name: &str) -> String {
    let spaced: String = name.chars().map(|c| if matches!(c, ':' | '/' | '\\' | '\0') || c.is_whitespace() { ' ' } else { c }).collect();
    let mut out = spaced.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    while out.ends_with('.') {
        out.pop();
    }
    let out = out.trim_end().to_string();
    if out.is_empty() { "Untitled".into() } else { out }
}

/// Plans exporting the folder `id` as a directory tree under `dest`: a
/// directory per folder and subfolder, each holding the originals of the
/// live items filed directly in it (an item in two folders is copied into
/// both). Items in any folder of `hidden` (locked ones) are left out, as
/// are hidden subfolders. Sibling folders whose names clean up to the same
/// directory share it. `None` if there is no such folder.
pub fn plan_folder(lib: &Library, id: &str, dest: &Path, how: Collision, hidden: &HashSet<String>) -> Option<Plan> {
    let folder = lib.all_folders().into_iter().find(|f| f.id == id)?;
    let top = dest.join(folder_dir_name(&folder.name));
    let mut dirs: Vec<(PathBuf, Vec<&Item>)> = Vec::new();
    let mut by_folder: HashMap<&str, Vec<&Item>> = HashMap::new();
    for item in lib.items().iter().filter(|i| !i.is_deleted && !i.folders.iter().any(|f| hidden.contains(f))) {
        for f in &item.folders {
            by_folder.entry(f.as_str()).or_default().push(item);
        }
    }
    fn walk<'a>(f: &'a Folder, dir: PathBuf, by_folder: &HashMap<&str, Vec<&'a Item>>, hidden: &HashSet<String>, dirs: &mut Vec<(PathBuf, Vec<&'a Item>)>) {
        let items = by_folder.get(f.id.as_str()).cloned().unwrap_or_default();
        match dirs.iter_mut().find(|(d, _)| *d == dir) {
            Some((_, list)) => list.extend(items),
            None => dirs.push((dir.clone(), items)),
        }
        for c in f.children.iter().filter(|c| !hidden.contains(&c.id)) {
            walk(c, dir.join(folder_dir_name(&c.name)), by_folder, hidden, dirs);
        }
    }
    walk(folder, top.clone(), &by_folder, hidden, &mut dirs);
    let mut plan = Plan { dirs: Vec::new(), steps: Vec::new(), top, overwrite: how == Collision::Replace };
    for (dir, mut items) in dirs {
        let mut seen = HashSet::new();
        items.retain(|i| seen.insert(i.id.as_str()));
        add_steps(&mut plan, lib.root(), &items, &dir, &listing(&dir), how);
        plan.dirs.push(dir);
    }
    Some(plan)
}

/// Plans exporting the smart folder `id` as one flat directory under
/// `dest` holding the originals of its items (Eagle exports a smart folder
/// without its subfolders). Items in `hidden` folders are left out. `None`
/// if there is no such smart folder.
pub fn plan_smart(lib: &Library, id: &str, dest: &Path, how: Collision, hidden: &HashSet<String>) -> Option<Plan> {
    let name = find_smart(&lib.smart_folders(), id)?.name.clone();
    let top = dest.join(folder_dir_name(&name));
    let items: Vec<&Item> = lib.smart_folder_items(id).into_iter().filter(|i| !i.folders.iter().any(|f| hidden.contains(f))).collect();
    let mut plan = Plan { dirs: vec![top.clone()], steps: Vec::new(), top: top.clone(), overwrite: how == Collision::Replace };
    add_steps(&mut plan, lib.root(), &items, &top, &listing(&top), how);
    Some(plan)
}

fn find_smart<'a>(list: &'a [SmartFolder], id: &str) -> Option<&'a SmartFolder> {
    list.iter().find_map(|s| if s.id == id { Some(s) } else { find_smart(&s.children, id) })
}

fn add_steps(plan: &mut Plan, root: &Path, items: &[&Item], dir: &Path, existing: &HashSet<String>, how: Collision) {
    add_named_steps(plan, root, items, items.iter().map(|i| file_name(i)).collect(), dir, existing, how);
}

fn add_named_steps(plan: &mut Plan, root: &Path, items: &[&Item], names: Vec<String>, dir: &Path, existing: &HashSet<String>, how: Collision) {
    let names = name_files(&names, existing, how);
    for (item, name) in items.iter().zip(names) {
        plan.steps.push(Step { id: item.id.clone(), from: item.original_path(root), to: dir.join(name) });
    }
}

/// Copies one original, keeping its modification time, as Eagle does. The
/// copy is written beside its destination and renamed into place, so a
/// failed copy never leaves half a file under the real name.
pub fn copy_file(from: &Path, to: &Path, overwrite: bool) -> Result<()> {
    let dir = to.parent().unwrap_or(Path::new("."));
    let tmp = tempfile::Builder::new().prefix(".falcon-export").tempfile_in(dir).map_err(|e| Error::io(dir, e))?;
    fs::copy(from, tmp.path()).map_err(|e| Error::io(from, e))?;
    if let Ok(meta) = fs::metadata(from) {
        let _ = filetime::set_file_mtime(tmp.path(), FileTime::from_last_modification_time(&meta));
    }
    if overwrite {
        tmp.persist(to).map_err(|e| Error::io(to, e.error))?;
    } else {
        tmp.persist_noclobber(to).map_err(|e| Error::io(to, e.error))?;
    }
    Ok(())
}

/// Runs `plan`, calling `on_step` after each copy with whether it worked,
/// and returns the paths written. A step that fails is skipped; the rest
/// still run.
pub fn run(plan: &Plan, mut on_step: impl FnMut(&Step, bool)) -> Result<Vec<PathBuf>> {
    for d in &plan.dirs {
        fs::create_dir_all(d).map_err(|e| Error::io(d, e))?;
    }
    let mut written = Vec::new();
    for step in &plan.steps {
        let ok = copy_file(&step.from, &step.to, plan.overwrite).is_ok();
        if ok {
            written.push(step.to.clone());
        }
        on_step(step, ok);
    }
    Ok(written)
}

// ---------------------------------------------------------------------------
// Export in a chosen format and size

/// An image format to export to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    Webp,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Jpeg, Format::Png, Format::Webp];

    pub fn ext(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Webp => "webp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPG",
            Format::Png => "PNG",
            Format::Webp => "WebP",
        }
    }

    /// Whether the format takes a quality setting.
    pub fn lossy(self) -> bool {
        self != Format::Png
    }
}

/// How to convert each image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Convert {
    pub format: Format,
    /// 1 to 100, for JPG and WebP. WebP at 100 is lossless.
    pub quality: u8,
    /// Scale the image down so its longer side is at most this; never up.
    pub longest: Option<u32>,
}

/// Whether an item's original can be converted: raster images falcon
/// decodes, and SVG.
pub fn convertible(item: &Item) -> bool {
    matches!(item.ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "jfif" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "ico" | "svg")
}

/// Plans exporting `ids` converted to `format` into `dest`: each file is
/// named `<name>.<format ext>`, clashes handled as in [`plan_items`].
/// Items that can't be converted are left out; see [`convertible`].
pub fn plan_converted(lib: &Library, ids: &[String], dest: &Path, format: Format, how: Collision) -> Plan {
    let mut seen = HashSet::new();
    let items: Vec<&Item> = ids.iter().filter(|id| seen.insert(id.as_str())).filter_map(|id| lib.item(id)).filter(|i| convertible(i)).collect();
    let names = items.iter().map(|i| format!("{}.{}", i.name.replace(['/', '\0'], " "), format.ext())).collect();
    let mut plan = Plan { dirs: vec![dest.to_path_buf()], steps: Vec::new(), top: dest.to_path_buf(), overwrite: how == Collision::Replace };
    add_named_steps(&mut plan, lib.root(), &items, names, dest, &listing(dest), how);
    plan
}

/// Decodes an original (SVG rendered at its natural size), scales it down
/// to `longest` if needed, and encodes it as `convert` asks.
pub fn convert_bytes(from: &Path, convert: &Convert) -> std::result::Result<Vec<u8>, String> {
    use image::{DynamicImage, ImageEncoder};
    let is_svg = from.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let mut img = if is_svg {
        crate::thumbnail::render_svg(&fs::read(from).map_err(|e| e.to_string())?, None).map_err(|e| e.0)?
    } else {
        crate::thumbnail::decode(from).map_err(|e| e.0)?
    };
    if let Some(max) = convert.longest
        && img.width().max(img.height()) > max
    {
        img = img.resize(max, max, image::imageops::FilterType::Lanczos3);
    }
    let quality = convert.quality.clamp(1, 100);
    let mut out = Vec::new();
    match convert.format {
        Format::Jpeg => {
            // JPEG has no alpha: transparent parts go on white.
            let rgba = img.to_rgba8();
            let rgb = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                let [r, g, b, a] = rgba.get_pixel(x, y).0;
                let mix = |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
                image::Rgb([mix(r), mix(g), mix(b)])
            });
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(&rgb).map_err(|e| e.to_string())?;
        }
        Format::Png => {
            let img = if img.color().has_alpha() { DynamicImage::ImageRgba8(img.to_rgba8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
            image::codecs::png::PngEncoder::new(&mut out).write_image(img.as_bytes(), img.width(), img.height(), img.color().into()).map_err(|e| e.to_string())?;
        }
        Format::Webp => {
            let rgba = img.to_rgba8();
            let enc = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
            let mem = if quality >= 100 { enc.encode_lossless() } else { enc.encode(f32::from(quality)) };
            out = mem.to_vec();
        }
    }
    Ok(out)
}

/// Runs a [`plan_converted`] plan: each step's original is converted and
/// written beside its destination, then renamed into place.
pub fn run_converted(plan: &Plan, convert: &Convert, mut on_step: impl FnMut(&Step, bool)) -> Result<Vec<PathBuf>> {
    for d in &plan.dirs {
        fs::create_dir_all(d).map_err(|e| Error::io(d, e))?;
    }
    let mut written = Vec::new();
    for step in &plan.steps {
        let ok = convert_bytes(&step.from, convert).ok().is_some_and(|bytes| write_new(&step.to, &bytes, plan.overwrite).is_ok());
        if ok {
            written.push(step.to.clone());
        }
        on_step(step, ok);
    }
    Ok(written)
}

/// Writes `bytes` to `to` through a temporary file beside it.
fn write_new(to: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    let dir = to.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::Builder::new().prefix(".falcon-export").tempfile_in(dir).map_err(|e| Error::io(dir, e))?;
    std::io::Write::write_all(&mut tmp, bytes).map_err(|e| Error::io(to, e))?;
    if overwrite {
        tmp.persist(to).map_err(|e| Error::io(to, e.error))?;
    } else {
        tmp.persist_noclobber(to).map_err(|e| Error::io(to, e.error))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CSV

/// The CSV columns, in Eagle's order: id, name, extension, width, height,
/// duration, source URL, notes, comments, tags, folders, size, rating,
/// import time, modification time and the original's path. The header
/// text is falcon's own.
pub const CSV_HEADER: [&str; 16] = [
    "Item ID",
    "Name",
    "File Type",
    "Width (px)",
    "Height (px)",
    "Duration (s)",
    "Source URL",
    "Notes",
    "Comments",
    "Tags",
    "Folders",
    "Size (bytes)",
    "Rating",
    "Date Imported (UTC)",
    "Date Modified (UTC)",
    "Original Path",
];

/// One CSV field: quoted, with quotes doubled, when it holds a comma, a
/// quote or a line break; as it is otherwise.
pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// A time in ms since the epoch as Eagle's CSV writes it, `YYYY-MM-DD
/// HH:MM:SS` in UTC; empty for 0.
fn csv_time(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

/// A number field: empty when missing or zero, as Eagle writes it.
fn csv_number(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::Number(n)) if n.as_f64().is_some_and(|f| f != 0.0) => n.to_string(),
        _ => String::new(),
    }
}

/// The CSV row for one item.
pub fn csv_row(lib: &Library, item: &Item, folder_names: &HashMap<&str, &str>) -> Vec<String> {
    let nonzero = |n: u64| if n == 0 { String::new() } else { n.to_string() };
    let comments: Vec<String> = item.comments().into_iter().map(|c| c.annotation).collect();
    let folders: Vec<&str> = item.folders.iter().map(|f| folder_names.get(f.as_str()).copied().unwrap_or(f.as_str())).collect();
    vec![
        item.id.clone(),
        item.name.clone(),
        item.ext.clone(),
        nonzero(item.width),
        nonzero(item.height),
        csv_number(item.extra.get("duration")),
        item.url.clone(),
        item.annotation.clone(),
        comments.join("\n"),
        item.tags.join(", "),
        folders.join(", "),
        nonzero(item.size),
        csv_number(item.extra.get("star")),
        csv_time(item.modification_time),
        csv_time(item.last_modified),
        item.original_path(lib.root()).display().to_string(),
    ]
}

/// The CSV text for `ids` (unknown ids skipped): a header and one row per
/// item, lines joined with `\n`, preceded by a UTF-8 byte order mark so
/// spreadsheet apps read it as UTF-8, as Eagle writes it.
pub fn csv(lib: &Library, ids: &[String]) -> String {
    let all = lib.all_folders();
    let names: HashMap<&str, &str> = all.iter().map(|f| (f.id.as_str(), f.name.as_str())).collect();
    let mut lines = vec![CSV_HEADER.iter().map(|h| csv_field(h)).collect::<Vec<_>>().join(",")];
    for item in ids.iter().filter_map(|id| lib.item(id)) {
        lines.push(csv_row(lib, item, &names).iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
    }
    format!("\u{feff}{}", lines.join("\n"))
}

/// Writes [`csv`] to `dest` (through a temporary file) and returns the
/// number of rows.
pub fn write_csv(lib: &Library, ids: &[String], dest: &Path) -> Result<usize> {
    let text = csv(lib, ids);
    let dir = dest.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| Error::io(dir, e))?;
    std::io::Write::write_all(&mut tmp, text.as_bytes()).map_err(|e| Error::io(dest, e))?;
    tmp.persist(dest).map_err(|e| Error::io(dest, e.error))?;
    Ok(ids.iter().filter(|id| lib.item(id).is_some()).count())
}

// ---------------------------------------------------------------------------
// Copy variants

/// Lines for Copy File Path: each item's original, one per line, as Eagle
/// copies them.
pub fn paths_text(lib: &Library, items: &[&Item]) -> String {
    items.iter().map(|i| i.original_path(lib.root()).display().to_string()).collect::<Vec<_>>().join("\n")
}

/// Lines for Copy Folder Path: each item's `<id>.info` folder with a
/// trailing slash, as Eagle copies them.
pub fn folder_paths_text(lib: &Library, items: &[&Item]) -> String {
    items.iter().map(|i| format!("{}/", i.info_dir(lib.root()).display())).collect::<Vec<_>>().join("\n")
}

/// The MIME subtype Eagle uses for an original it copies as Base64 itself;
/// other kinds are copied as their thumbnail.
fn base64_type(ext: &str) -> Option<&'static str> {
    Some(match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" | "insp" => "jpeg",
        "png" => "png",
        "webp" => "webp",
        "gif" => "gif",
        _ => return None,
    })
}

/// Copy Base64: a `data:` URL of the item's original when it is a JPEG,
/// PNG, WebP or GIF, else of its thumbnail (whose real format is sniffed:
/// Eagle's thumbnails are WebP inside a `.png` name). `None` when there is
/// neither.
pub fn data_url(lib: &Library, item: &Item) -> Option<String> {
    use base64::Engine;
    let (bytes, kind) = match base64_type(&item.ext) {
        Some(kind) => (fs::read(item.original_path(lib.root())).ok()?, kind),
        None => {
            let bytes = fs::read(item.thumbnail_path(lib.root())?).ok()?;
            let kind = match image::guess_format(&bytes) {
                Ok(image::ImageFormat::WebP) => "webp",
                Ok(image::ImageFormat::Jpeg) => "jpeg",
                _ => "png",
            };
            (bytes, kind)
        }
    };
    Some(format!("data:image/{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Copy Thumbnail: the item's thumbnail decoded, or, for an item without
/// one, its original if that is an image.
pub fn thumbnail_image(lib: &Library, item: &Item) -> Option<image::RgbaImage> {
    let path = item.thumbnail_path(lib.root()).unwrap_or_else(|| item.original_path(lib.root()));
    crate::thumbnail::decode(&path).ok().map(|i| i.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_lowercase()).collect()
    }

    fn names(files: &[&str], existing: &[&str], how: Collision) -> Vec<String> {
        name_files(&files.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &set(existing), how)
    }

    #[test]
    fn unique_names_stay() {
        assert_eq!(names(&["a.png", "b.jpg", "noext"], &["c.png"], Collision::KeepBoth), ["a.png", "b.jpg", "noext"]);
    }

    #[test]
    fn clashes_in_the_batch_are_numbered_ignoring_case() {
        assert_eq!(names(&["a.png", "A.PNG", "b.png"], &[], Collision::KeepBoth), ["a-1.png", "A-2.PNG", "b.png"]);
        assert_eq!(names(&["x", "x"], &[], Collision::Replace), ["x-1", "x-2"]);
        // Padded to the widest number.
        let ten = names(&["a.png"; 10], &[], Collision::Replace);
        assert_eq!(ten[0], "a-01.png");
        assert_eq!(ten[9], "a-10.png");
    }

    #[test]
    fn existing_files_are_kept_or_replaced() {
        assert_eq!(names(&["a.png", "b.png"], &["A.png"], Collision::KeepBoth), ["a-2.png", "b.png"]);
        assert_eq!(names(&["a.png", "b.png"], &["a.png"], Collision::Replace), ["a.png", "b.png"]);
        // Taken numbered names are skipped.
        assert_eq!(names(&["a.png"], &["a.png", "a-2.png"], Collision::KeepBoth), ["a-3.png"]);
        // A batch file already called a-1.png keeps that name.
        assert_eq!(names(&["a.png", "a.png", "a-1.png"], &[], Collision::KeepBoth), ["a-2.png", "a-3.png", "a-1.png"]);
    }

    fn png(path: &Path, shade: u8) {
        image::RgbImage::from_fn(8, 8, |_, _| image::Rgb([shade, 0, 0])).save(path).unwrap();
    }

    /// A library with three items, two of them named `pic`.
    fn library(root: &Path) -> (Library, Vec<String>) {
        let mut lib = Library::create(root.join("A.library")).unwrap();
        let mut ids = Vec::new();
        for (i, name) in ["pic", "other", "pic"].iter().enumerate() {
            let dir = root.join(format!("src{i}"));
            fs::create_dir_all(&dir).unwrap();
            let src = dir.join(format!("{name}.png"));
            png(&src, 40 * i as u8);
            filetime::set_file_mtime(&src, FileTime::from_unix_time(1_600_000_000 + i as i64, 0)).unwrap();
            ids.push(lib.import_file(&src, &ImportOptions { palette: false, ..Default::default() }).unwrap());
        }
        (lib, ids)
    }

    #[test]
    fn items_are_copied_with_their_times() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, ids) = library(dir.path());
        let out = dir.path().join("out");
        let plan = plan_items(&lib, &ids, &out, Collision::KeepBoth);
        let to: Vec<_> = plan.steps.iter().map(|s| s.to.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(to, ["pic-1.png", "other.png", "pic-2.png"]);
        assert_eq!(plan.clashes(), 0);
        let mut seen = 0;
        let written = run(&plan, |_, ok| seen += usize::from(ok)).unwrap();
        assert_eq!((written.len(), seen), (3, 3));
        for (step, id) in plan.steps.iter().zip(&ids) {
            let item = lib.item(id).unwrap();
            assert_eq!(fs::read(&step.to).unwrap(), fs::read(item.original_path(lib.root())).unwrap());
            let a = FileTime::from_last_modification_time(&fs::metadata(&step.to).unwrap());
            let b = FileTime::from_last_modification_time(&fs::metadata(item.original_path(lib.root())).unwrap());
            assert_eq!(a.unix_seconds(), b.unix_seconds());
        }
        // No temp files are left behind.
        assert_eq!(fs::read_dir(&out).unwrap().count(), 3);
    }

    #[test]
    fn a_second_export_keeps_both_or_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, ids) = library(dir.path());
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("other.png"), b"mine").unwrap();

        let replace = plan_items(&lib, &ids[1..2], &out, Collision::Replace);
        assert_eq!(replace.clashes(), 1);
        let keep = plan_items(&lib, &ids[1..2], &out, Collision::KeepBoth);
        assert_eq!(keep.steps[0].to, out.join("other-2.png"));
        run(&keep, |_, _| {}).unwrap();
        assert_eq!(fs::read(out.join("other.png")).unwrap(), b"mine");
        run(&replace, |_, _| {}).unwrap();
        assert_ne!(fs::read(out.join("other.png")).unwrap(), b"mine");
        assert_eq!(fs::read(out.join("other.png")).unwrap(), fs::read(out.join("other-2.png")).unwrap());
    }

    #[test]
    fn folder_names_are_cleaned_like_eagle() {
        assert_eq!(folder_dir_name("  a\tb  c "), "a b c");
        assert_eq!(folder_dir_name("x/y:z\\w"), "x y z w");
        assert_eq!(folder_dir_name("dots..."), "dots");
        assert_eq!(folder_dir_name(" / "), "Untitled");
    }

    #[test]
    fn folders_export_as_a_tree() {
        let dir = tempfile::tempdir().unwrap();
        let (mut lib, ids) = library(dir.path());
        let top = lib.create_folder(None, "Refs: old").unwrap();
        let child = lib.create_folder(Some(&top), "Inner").unwrap();
        let twin = lib.create_folder(Some(&top), "Inner").unwrap();
        let locked = lib.create_folder(Some(&top), "Secret").unwrap();
        let file = |lib: &mut Library, id: &str, folders: &[&String]| {
            let mut item = lib.item(id).unwrap().clone();
            item.folders = folders.iter().map(|f| f.to_string()).collect();
            lib.update_item(item).unwrap();
        };
        file(&mut lib, &ids[0], &[&top, &child]);
        file(&mut lib, &ids[1], &[&locked]);
        file(&mut lib, &ids[2], &[&twin]);
        let out = dir.path().join("out");
        let hidden: HashSet<String> = [locked.clone()].into();
        let plan = plan_folder(&lib, &top, &out, Collision::KeepBoth, &hidden).unwrap();
        let root = out.join("Refs old");
        assert_eq!(plan.top, root);
        assert_eq!(plan.dirs, [root.clone(), root.join("Inner")]);
        let to: Vec<_> = plan.steps.iter().map(|s| s.to.strip_prefix(&out).unwrap().to_path_buf()).collect();
        // The two "Inner" folders share a directory, so their pics are numbered.
        assert_eq!(to, [PathBuf::from("Refs old/pic.png"), PathBuf::from("Refs old/Inner/pic-1.png"), PathBuf::from("Refs old/Inner/pic-2.png")]);
        run(&plan, |_, _| {}).unwrap();
        assert!(root.join("Inner/pic-2.png").is_file());

        // Unlocked, the locked folder comes too.
        let plan = plan_folder(&lib, &top, &out, Collision::Replace, &HashSet::new()).unwrap();
        assert!(plan.dirs.contains(&root.join("Secret")));
        assert_eq!(plan.clashes(), 3);
        assert!(plan_folder(&lib, "NOPE", &out, Collision::Replace, &HashSet::new()).is_none());
    }

    #[test]
    fn smart_folders_export_flat() {
        let dir = tempfile::tempdir().unwrap();
        let (mut lib, ids) = library(dir.path());
        let rule = serde_json::json!({"rules": [{"property": "name", "method": "contain", "value": "pic"}], "match": "AND", "boolean": "TRUE"});
        let id = lib.create_smart_folder("Pics", vec![rule]).unwrap();
        assert_eq!(lib.smart_folder_items(&id).len(), 2);
        let out = dir.path().join("out");
        let plan = plan_smart(&lib, &id, &out, Collision::KeepBoth, &HashSet::new()).unwrap();
        assert_eq!(plan.dirs, [out.join("Pics")]);
        let got: HashSet<_> = plan.steps.iter().map(|s| (s.id.clone(), s.to.parent().unwrap().to_path_buf())).collect();
        assert_eq!(got, [(ids[0].clone(), out.join("Pics")), (ids[2].clone(), out.join("Pics"))].into());
        let names: HashSet<_> = plan.steps.iter().map(|s| s.to.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["pic-1.png".to_string(), "pic-2.png".to_string()].into());
        assert!(plan_smart(&lib, "NOPE", &out, Collision::KeepBoth, &HashSet::new()).is_none());
    }

    #[test]
    fn csv_fields_are_quoted_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field(""), "");
        assert_eq!(csv_field("a, b"), "\"a, b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_field("cr\r"), "\"cr\r\"");
        assert_eq!(csv_field("日本語"), "日本語");
        assert_eq!(csv_time(0), "");
    }

    #[test]
    fn csv_has_a_row_per_item_in_eagles_column_order() {
        let dir = tempfile::tempdir().unwrap();
        let (mut lib, ids) = library(dir.path());
        let folder = lib.create_folder(None, "Refs, old").unwrap();
        let mut item = lib.item(&ids[1]).unwrap().clone();
        item.tags = vec!["blue".into(), "logo".into()];
        item.folders = vec![folder.clone(), "GONE".into()];
        item.url = "https://example.com/a?b=1,2".into();
        item.annotation = "line one\nline \"two\"".into();
        item.modification_time = 1_700_000_000_123;
        item.extra.insert("star".into(), serde_json::json!(4));
        item.set_comments(&[crate::model::Comment::region(1, 2, 3, 4, "first"), crate::model::Comment::region(1, 2, 3, 4, "second")]);
        lib.update_item(item).unwrap();

        let out = dir.path().join("x.csv");
        assert_eq!(write_csv(&lib, &[ids[1].clone(), "NOPE".into()], &out).unwrap(), 1);
        let text = fs::read_to_string(&out).unwrap();
        let body = text.strip_prefix('\u{feff}').expect("starts with a BOM");
        assert!(!body.ends_with('\n'));
        let (header, row) = body.split_once('\n').unwrap();
        assert_eq!(header.split(',').count(), 16);
        assert!(header.starts_with("Item ID,Name,File Type,"));
        let item = lib.item(&ids[1]).unwrap();
        let path = item.original_path(lib.root()).display().to_string();
        let want = format!(
            "{},other,png,8,8,,\"https://example.com/a?b=1,2\",\"line one\nline \"\"two\"\"\",\"first\nsecond\",\"blue, logo\",\"Refs, old, GONE\",{},4,2023-11-14 22:13:20,{},{}",
            ids[1], item.size, csv_time(item.last_modified), csv_field(&path)
        );
        assert_eq!(row, want);
    }

    #[test]
    fn copy_texts_and_data_urls() {
        use base64::Engine;
        let dir = tempfile::tempdir().unwrap();
        let (mut lib, ids) = library(dir.path());
        let items: Vec<&Item> = ids.iter().filter_map(|id| lib.item(id)).collect();
        let paths = paths_text(&lib, &items);
        assert_eq!(paths.lines().count(), 3);
        assert!(paths.lines().next().unwrap().ends_with("/pic.png"));
        let folders = folder_paths_text(&lib, &items[..1]);
        assert_eq!(folders, format!("{}/", items[0].info_dir(lib.root()).display()));

        // A PNG is copied as itself.
        let url = data_url(&lib, items[0]).unwrap();
        let b64 = url.strip_prefix("data:image/png;base64,").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert_eq!(bytes, fs::read(items[0].original_path(lib.root())).unwrap());
        assert_eq!(thumbnail_image(&lib, items[0]).unwrap().dimensions(), (8, 8));

        // Another kind falls back to its thumbnail, or to nothing.
        let mut other = items[1].clone();
        let info = other.info_dir(lib.root());
        fs::remove_file(other.original_path(lib.root())).unwrap();
        fs::write(info.join("other.txt"), b"hello").unwrap();
        other.ext = "txt".into();
        lib.update_item(other.clone()).unwrap();
        let thumb = info.join("other_thumbnail.png");
        let webp = webp::Encoder::from_rgba(&[255; 16], 2, 2).encode(80.0).to_vec();
        fs::write(&thumb, &webp).unwrap();
        let other = lib.item(&ids[1]).unwrap();
        assert!(data_url(&lib, other).unwrap().starts_with("data:image/webp;base64,"));
        assert_eq!(thumbnail_image(&lib, other).unwrap().dimensions(), (2, 2));
        fs::remove_file(&thumb).unwrap();
        assert!(data_url(&lib, other).is_none());
        assert!(thumbnail_image(&lib, other).is_none());
    }

    #[test]
    fn images_convert_to_each_format_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("wide.png");
        image::RgbaImage::from_fn(40, 20, |x, _| image::Rgba([200, 10, 10, if x < 20 { 0 } else { 255 }])).save(&src).unwrap();
        for format in Format::ALL {
            let convert = Convert { format, quality: 80, longest: Some(10) };
            let bytes = convert_bytes(&src, &convert).unwrap();
            let want = match format {
                Format::Jpeg => image::ImageFormat::Jpeg,
                Format::Png => image::ImageFormat::Png,
                Format::Webp => image::ImageFormat::WebP,
            };
            assert_eq!(image::guess_format(&bytes).unwrap(), want);
            let img = image::load_from_memory(&bytes).unwrap();
            assert_eq!((img.width(), img.height()), (10, 5), "{format:?}");
            if format == Format::Jpeg {
                // The transparent half turns white, not black.
                let p = img.to_rgb8().get_pixel(0, 2).0;
                assert!(p.iter().all(|c| *c > 200), "{p:?}");
            }
        }
        // Never enlarged; lossless WebP at 100.
        let bytes = convert_bytes(&src, &Convert { format: Format::Webp, quality: 100, longest: Some(400) }).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (40, 20));
        assert_eq!(img.to_rgba8().get_pixel(30, 3).0, [200, 10, 10, 255]);
        assert!(convert_bytes(&dir.path().join("missing.png"), &Convert { format: Format::Png, quality: 90, longest: None }).is_err());
    }

    #[test]
    fn converted_exports_are_renamed_and_skip_other_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let (mut lib, ids) = library(dir.path());
        let mut other = lib.item(&ids[1]).unwrap().clone();
        other.ext = "mp3".into();
        lib.update_item(other).unwrap();
        let out = dir.path().join("out");
        let plan = plan_converted(&lib, &ids, &out, Format::Jpeg, Collision::KeepBoth);
        let to: Vec<_> = plan.steps.iter().map(|s| s.to.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(to, ["pic-1.jpg", "pic-2.jpg"]);
        let convert = Convert { format: Format::Jpeg, quality: 90, longest: None };
        let written = run_converted(&plan, &convert, |_, _| {}).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(image::guess_format(&fs::read(&written[0]).unwrap()).unwrap(), image::ImageFormat::Jpeg);
        // pic.jpg itself is still free.
        let again = plan_converted(&lib, &ids[..1], &out, Format::Jpeg, Collision::KeepBoth);
        assert_eq!(again.steps[0].to, out.join("pic.jpg"));
    }

    #[test]
    fn failed_steps_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, ids) = library(dir.path());
        let out = dir.path().join("out");
        let mut plan = plan_items(&lib, &ids, &out, Collision::KeepBoth);
        plan.steps[0].from = dir.path().join("missing.png");
        let mut failed = 0;
        let written = run(&plan, |_, ok| failed += usize::from(!ok)).unwrap();
        assert_eq!((written.len(), failed), (2, 1));
        assert_eq!(fs::read_dir(&out).unwrap().count(), 2);
    }
}
