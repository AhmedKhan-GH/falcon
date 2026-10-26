//! Importing a local file into a library, following Eagle's `processImage`.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use filetime::FileTime;
use unicode_normalization::UnicodeNormalization;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::Item;
use crate::{ids, palette, thumbnail};

/// Longest file name Eagle allows inside an `.info` folder, in bytes.
const MAX_FILE_NAME: usize = 255;
/// `_thumbnail.png`, which is appended to the name.
const THUMB_SUFFIX: &str = "_thumbnail.png";

/// Settings for [`Library::import_file`].
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Ids of folders to put the item in; each must exist.
    pub folders: Vec<String>,
    /// Tags to attach.
    pub tags: Vec<String>,
    /// Whether to extract a palette (default true).
    pub palette: bool,
    /// Annotation text.
    pub annotation: String,
    /// Source URL.
    pub url: String,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            folders: Vec::new(),
            tags: Vec::new(),
            palette: true,
            annotation: String::new(),
            url: String::new(),
        }
    }
}

/// A file copied, thumbnailed and analysed in a temporary folder, ready to
/// be moved into a library with [`Library::commit_import`]. Staging needs no
/// access to the library, so it can run on another thread.
#[derive(Debug)]
pub struct Staged {
    item: Item,
    dir: tempfile::TempDir,
}

impl Staged {
    /// The item as it will be stored (its id is assigned at commit).
    pub fn item(&self) -> &Item {
        &self.item
    }
}

/// Copies `path` into a staging folder with its mtime kept, then makes the
/// thumbnail and palette, following Eagle's import steps.
pub fn stage(path: impl AsRef<Path>, opts: &ImportOptions) -> Result<Staged> {
    let src = path.as_ref();
    let refuse = |reason: &str| Error::Import { path: src.to_path_buf(), reason: reason.into() };

    let meta = fs::metadata(src).map_err(|e| Error::io(src, e))?;
    if !meta.is_file() {
        return Err(refuse("not a regular file"));
    }
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if meta.len() == 0 && ext != "txt" {
        return Err(refuse("file is empty"));
    }

    let mtime_ms = to_ms(meta.modified().ok());
    // Not every filesystem records a birth time; fall back to the mtime.
    let btime_ms = meta.created().ok().map_or(mtime_ms, |t| to_ms(Some(t)));
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = sanitize_name(&stem, &ext);

    let mut item = Item {
        id: String::new(),
        name,
        size: meta.len(),
        btime: btime_ms,
        mtime: mtime_ms,
        ext,
        tags: opts.tags.clone(),
        folders: opts.folders.clone(),
        is_deleted: false,
        url: opts.url.clone(),
        annotation: opts.annotation.clone(),
        modification_time: 0,
        remove_thumbnail: None,
        no_thumbnail: None,
        height: 0,
        width: 0,
        last_modified: 0,
        palettes: Vec::new(),
        width_first: false,
        extra: Default::default(),
    };

    let dir = tempfile::Builder::new().prefix("falcon-import").tempdir().map_err(|e| Error::io(src, e))?;
    let file = if item.ext.is_empty() { item.name.clone() } else { format!("{}.{}", item.name, item.ext) };
    let raw = dir.path().join(file);
    fs::copy(src, &raw).map_err(|e| Error::io(&raw, e))?;
    filetime::set_file_times(&raw, FileTime::from_last_access_time(&meta), FileTime::from_last_modification_time(&meta))
        .map_err(|e| Error::io(&raw, e))?;

    let thumb = dir.path().join(format!("{}{THUMB_SUFFIX}", item.name));
    let analysis = generate(&mut item, &raw, &thumb);
    if opts.palette
        && let Some(img) = analysis.and_then(|a| match a {
            Analysis::Original => thumbnail::decode(&raw).ok(),
            Analysis::Thumbnail => thumbnail::decode(&thumb).ok(),
        })
    {
        item.palettes = palette::extract(&img);
    }
    if item.remove_thumbnail == Some(true) {
        let _ = fs::remove_file(&thumb);
    }
    Ok(Staged { item, dir })
}

impl Library {
    /// Imports one file and returns the new item's id.
    ///
    /// The original is copied into `images/<ID>.info/` with its mtime kept, a
    /// thumbnail and palette are produced where the format is supported, and
    /// the item's `metadata.json` and the library's `mtime.json` are written.
    /// Formats falcon cannot decode import with no thumbnail and no palette.
    pub fn import_file(&mut self, path: impl AsRef<Path>, opts: &ImportOptions) -> Result<String> {
        self.check_folders(path.as_ref(), opts)?;
        let staged = stage(path, opts)?;
        self.commit_import(staged)
    }

    /// Fails if `opts` names folders this library doesn't have.
    pub fn check_folders(&self, path: &Path, opts: &ImportOptions) -> Result<()> {
        let known = self.folder_ids();
        match opts.folders.iter().find(|f| !known.contains(f.as_str())) {
            Some(bad) => Err(Error::Import { path: path.to_path_buf(), reason: format!("unknown folder {bad}") }),
            None => Ok(()),
        }
    }

    /// Moves a staged import into `images/<ID>.info/`, stamps its times and
    /// writes its `metadata.json` and `mtime.json`.
    pub fn commit_import(&mut self, staged: Staged) -> Result<String> {
        let Staged { mut item, dir } = staged;
        let known = self.folder_ids();
        item.folders.retain(|f| known.contains(f.as_str()));
        let mut id = ids::new_id();
        while self.root().join("images").join(format!("{id}.info")).exists() {
            id = ids::new_id();
        }
        item.id = id;
        let dest = item.info_dir(self.root());
        fs::create_dir_all(&dest).map_err(|e| Error::io(&dest, e))?;
        let moved = (|| -> Result<()> {
            for entry in fs::read_dir(dir.path()).map_err(|e| Error::io(dir.path(), e))? {
                let entry = entry.map_err(|e| Error::io(dir.path(), e))?;
                let to = dest.join(entry.file_name());
                // A rename can't cross volumes; copy then.
                if fs::rename(entry.path(), &to).is_err() {
                    fs::copy(entry.path(), &to).map_err(|e| Error::io(&to, e))?;
                    if let Ok(m) = fs::metadata(entry.path()) {
                        let _ = filetime::set_file_times(&to, FileTime::from_last_access_time(&m), FileTime::from_last_modification_time(&m));
                    }
                }
            }
            item.modification_time = self.next_import_time(now_ms());
            item.last_modified = now_ms().max(item.modification_time);
            self.register_item(item.clone())
        })();
        match moved {
            Ok(()) => Ok(item.id),
            Err(e) => {
                let _ = fs::remove_dir_all(&dest);
                Err(e)
            }
        }
    }
}

/// Where the palette should be computed from.
enum Analysis {
    Original,
    Thumbnail,
}

/// Sets width, height and the thumbnail flags on `item` and writes the
/// thumbnail. A decode or render failure leaves the item icon-only: no flags,
/// no thumbnail, no palette (the `None` result).
fn generate(item: &mut Item, raw: &Path, thumb: &Path) -> Option<Analysis> {
    let max = thumbnail::max_side(&item.ext);
    let apply = |item: &mut Item, plan: thumbnail::Plan| {
        item.no_thumbnail = plan.no_thumbnail;
        item.remove_thumbnail = plan.remove_thumbnail;
    };
    if item.ext == "svg" {
        let data = fs::read(raw).ok()?;
        let rendered = thumbnail::svg_size(&data).and_then(|(w, h)| {
            let plan = thumbnail::plan_svg(item.size);
            // A small SVG's thumbnail is temporary, so it is kept at full size.
            let limit = (plan.remove_thumbnail != Some(true)).then_some(max);
            let img = thumbnail::render_svg(&data, limit)?;
            thumbnail::write_encoded(&img, thumb)?;
            Ok((w, h, plan))
        });
        return match rendered {
            Ok((w, h, plan)) => {
                item.width = w.into();
                item.height = h.into();
                apply(item, plan);
                Some(Analysis::Thumbnail)
            }
            Err(_) => {
                apply(item, thumbnail::plan_svg_failed());
                None
            }
        };
    }
    if !thumbnail::is_supported(&item.ext) {
        return quicklook_thumbnail(item, raw, thumb, max);
    }
    let img = thumbnail::decode(raw).ok()?;
    item.width = img.width().into();
    item.height = img.height().into();
    let plan = thumbnail::plan_raster(&item.ext, img.width(), img.height(), item.size);
    if !plan.thumbnail {
        apply(item, plan);
        return Some(Analysis::Original);
    }
    if thumbnail::write_thumbnail(img, max, thumb).is_err() {
        let _ = fs::remove_file(thumb);
        return None;
    }
    apply(item, plan);
    Some(Analysis::Thumbnail)
}

/// Formats falcon can't decode itself (PDF, video, PSD, AI, HEIC, RAW, ...)
/// get a QuickLook render as their thumbnail, as Eagle renders them with its
/// own viewers. Sizes come from Spotlight when it knows them, else from the
/// render (Eagle likewise reports a PDF's render size); videos also get a
/// `duration` in seconds.
fn quicklook_thumbnail(item: &mut Item, raw: &Path, thumb: &Path, max: u32) -> Option<Analysis> {
    // Eagle shows text files as text and never thumbnails them.
    if item.ext == "txt" {
        return None;
    }
    let img = thumbnail::quicklook(raw, thumbnail::QUICKLOOK_SIZE).ok()?;
    let meta = thumbnail::spotlight(raw);
    let (w, h) = meta.size.unwrap_or((img.width(), img.height()));
    item.width = w.into();
    item.height = h.into();
    if let Some(d) = meta.duration {
        item.extra.insert("duration".into(), serde_json::json!(d));
    }
    thumbnail::write_thumbnail(img, max, thumb).ok()?;
    Some(Analysis::Thumbnail)
}

/// Turns a file stem into an item name: forbidden characters removed, trimmed,
/// NFC-normalised, shortened to fit the file system, `_` if nothing is left.
pub(crate) fn sanitize_name(stem: &str, ext: &str) -> String {
    let cleaned: String = stem
        .chars()
        .filter(|c| !c.is_control() && !"/?<>\\:*|\"%".contains(*c))
        .collect();
    let cleaned: String = cleaned.nfc().collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    let room = MAX_FILE_NAME.saturating_sub(THUMB_SUFFIX.len().max(ext.len() + 1));
    let mut out = String::new();
    for c in cleaned.chars() {
        if out.len() + c.len_utf8() > room {
            break;
        }
        out.push(c);
    }
    let out = out.trim().to_string();
    if out.is_empty() || out == "." || out == ".." { "_".into() } else { out }
}

fn to_ms(t: Option<SystemTime>) -> u64 {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

fn now_ms() -> u64 {
    to_ms(Some(SystemTime::now()))
}
