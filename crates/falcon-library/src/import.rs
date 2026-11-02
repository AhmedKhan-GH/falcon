//! Importing a local file into a library, following Eagle's `processImage`.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use filetime::FileTime;
use image::DynamicImage;
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::model::Item;
use crate::{exif, fontmeta, fontpreview, ids, keywords, media, palette, tempo, thumbnail, waveform, weblink};

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
    /// Add a font's weight (`fontMetas.weight`) as a tag, like Eagle's
    /// "font auto tag" preference (default true, as in Eagle).
    pub font_weight_tag: bool,
    /// Also store `rawMetas` for JPEG, TIFF, HEIC, PNG and WebP photos.
    /// Eagle only does it for RAW files (default false).
    pub camera_metadata_all_photos: bool,
    /// Add embedded IPTC keywords and XMP `dc:subject` entries as tags, like
    /// Eagle's IPTC preference (default false, as in Eagle).
    pub keywords_as_tags: bool,
    /// Add the auto-tags of the folders in `folders` (and their parents'),
    /// as Eagle does for items imported or pasted into a folder in its
    /// window (default false: Eagle's API leaves them off). See
    /// [`crate::autotag`].
    pub folder_auto_tags: bool,
    /// A name for the item, in place of the file's own. The file's
    /// extension is kept. Meant for a single file (default None).
    pub name: Option<String>,
    /// Import the item straight into the trash (Inboard's trashed items).
    pub trashed: bool,
    /// The item's date added (`modificationTime`, ms) in place of the
    /// import time, for items brought over from another app.
    pub added_at: Option<u64>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            folders: Vec::new(),
            tags: Vec::new(),
            palette: true,
            annotation: String::new(),
            url: String::new(),
            font_weight_tag: true,
            camera_metadata_all_photos: false,
            keywords_as_tags: false,
            folder_auto_tags: false,
            name: None,
            trashed: false,
            added_at: None,
        }
    }
}

/// Refusal reasons [`stage`] gives, which [`Failure::new`] recognises.
const NOT_A_FILE: &str = "not a regular file";
const EMPTY_FILE: &str = "file is empty";
const UNRECOGNIZED: &str = "file format is not recognised";

/// Browser download-in-progress extensions (Chrome, Firefox, Safari),
/// which Eagle's `getExt` treats as having no format.
const PARTIAL_DOWNLOADS: [&str; 3] = ["crdownload", "part", "download"];

/// Whether Eagle would import a file with this (lower-cased) extension:
/// any extension at all, except unfinished browser downloads. A file with
/// no extension is refused as "not recognized"; every other format is
/// imported, with a thumbnail when one can be made and as an icon
/// otherwise.
pub fn is_recognized(ext: &str) -> bool {
    !ext.is_empty() && !PARTIAL_DOWNLOADS.contains(&ext)
}

/// Why a file could not be imported, for the app's list of failed imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// The file is gone.
    Missing,
    /// A folder or package, not a regular file.
    NotAFile,
    /// Empty (only `.txt` files may be).
    Empty,
    /// Its format can't be told.
    Unrecognized,
    /// It can't be read (permissions, a disk error).
    Unreadable,
    /// The disk is full.
    NoSpace,
    /// Copying it into the staging folder failed.
    CopyFailed,
    /// Writing it into the library failed.
    LibraryWrite,
    /// A folder it should go in no longer exists.
    UnknownFolder,
    /// Anything else.
    Other,
}

impl FailKind {
    /// A short reason for the user.
    pub fn describe(self) -> &'static str {
        match self {
            FailKind::Missing => "The file no longer exists",
            FailKind::NotAFile => "Folders and packages can't be imported as items",
            FailKind::Empty => "The file is empty",
            FailKind::Unrecognized => "The file format isn't recognised",
            FailKind::Unreadable => "The file can't be read",
            FailKind::NoSpace => "Not enough disk space",
            FailKind::CopyFailed => "Copying the file failed",
            FailKind::LibraryWrite => "Writing to the library failed",
            FailKind::UnknownFolder => "The folder to import into no longer exists",
            FailKind::Other => "The file couldn't be imported",
        }
    }
}

/// One file that failed to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The file.
    pub path: std::path::PathBuf,
    /// Why, in general.
    pub kind: FailKind,
    /// The underlying error, for copying out.
    pub detail: String,
}

impl Failure {
    /// Classifies an error from [`stage`], [`Library::check_folders`] or
    /// [`Library::commit_import`] for the file at `path`.
    pub fn new(path: &Path, err: &Error) -> Failure {
        use std::io::ErrorKind as K;
        let kind = match err {
            Error::Import { reason, .. } if reason == NOT_A_FILE => FailKind::NotAFile,
            Error::Import { reason, .. } if reason == EMPTY_FILE => FailKind::Empty,
            Error::Import { reason, .. } if reason == UNRECOGNIZED => FailKind::Unrecognized,
            Error::Import { reason, .. } if reason.starts_with("unknown folder") => FailKind::UnknownFolder,
            Error::Io { source, .. } if source.kind() == K::StorageFull => FailKind::NoSpace,
            Error::Io { path: at, source } if at == path => match source.kind() {
                K::NotFound => FailKind::Missing,
                _ => FailKind::Unreadable,
            },
            // Reading the source fails inside the staging copy too.
            Error::Io { path: at, source } if is_staging(at) => match source.kind() {
                K::NotFound if !path.exists() => FailKind::Missing,
                K::PermissionDenied => FailKind::Unreadable,
                _ => FailKind::CopyFailed,
            },
            Error::Io { .. } | Error::Json { .. } => FailKind::LibraryWrite,
            _ => FailKind::Other,
        };
        Failure { path: path.to_path_buf(), kind, detail: err.to_string() }
    }
}

/// Whether `path` is inside one of [`stage`]'s temporary folders.
fn is_staging(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str().to_string_lossy().starts_with(STAGING_PREFIX))
}

const STAGING_PREFIX: &str = "falcon-import";

/// A file copied, thumbnailed and analysed in a temporary folder, ready to
/// be moved into a library with [`Library::commit_import`]. Staging needs no
/// access to the library, so it can run on another thread.
#[derive(Debug)]
pub struct Staged {
    item: Item,
    dir: tempfile::TempDir,
    /// [`ImportOptions::folder_auto_tags`], applied at commit.
    auto_tags: bool,
    /// [`ImportOptions::added_at`].
    added_at: Option<u64>,
}

impl Staged {
    /// The item as it will be stored (its id is assigned at commit).
    pub fn item(&self) -> &Item {
        &self.item
    }

    /// The staged copy of the file.
    pub fn original(&self) -> std::path::PathBuf {
        let item = &self.item;
        self.dir.path().join(if item.ext.is_empty() { item.name.clone() } else { format!("{}.{}", item.name, item.ext) })
    }
}

/// Copies `path` into a staging folder with its mtime kept, then makes the
/// thumbnail and palette, following Eagle's import steps.
pub fn stage(path: impl AsRef<Path>, opts: &ImportOptions) -> Result<Staged> {
    let src = path.as_ref();
    let refuse = |reason: &str| Error::Import { path: src.to_path_buf(), reason: reason.into() };

    let meta = fs::metadata(src).map_err(|e| Error::io(src, e))?;
    if !meta.is_file() {
        return Err(refuse(NOT_A_FILE));
    }
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !is_recognized(&ext) {
        return Err(refuse(UNRECOGNIZED));
    }
    if meta.len() == 0 && ext != "txt" {
        return Err(refuse(EMPTY_FILE));
    }

    let mtime_ms = to_ms(meta.modified().ok());
    // Not every filesystem records a birth time; fall back to the mtime.
    let btime_ms = meta.created().ok().map_or(mtime_ms, |t| to_ms(Some(t)));
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match opts.name.as_deref().map(|n| sanitize_name(n, &ext)) {
        Some(n) if !n.is_empty() => n,
        _ => sanitize_name(&stem, &ext),
    };

    let mut item = Item {
        id: String::new(),
        name,
        size: meta.len(),
        btime: btime_ms,
        mtime: mtime_ms,
        ext,
        tags: opts.tags.clone(),
        folders: opts.folders.clone(),
        is_deleted: opts.trashed,
        url: opts.url.clone(),
        annotation: opts.annotation.clone(),
        modification_time: 0,
        remove_thumbnail: None,
        no_thumbnail: None,
        height: 0,
        width: 0,
        last_modified: 0,
        palettes: Vec::new(),
        key_order: Default::default(),
        extra: Default::default(),
    };

    if opts.keywords_as_tags {
        add_keyword_tags(&mut item.tags, keywords::read(src, &item.ext));
    }

    let dir = tempfile::Builder::new().prefix(STAGING_PREFIX).tempdir().map_err(|e| Error::io(src, e))?;
    let file = if item.ext.is_empty() { item.name.clone() } else { format!("{}.{}", item.name, item.ext) };
    let raw = dir.path().join(file);
    fs::copy(src, &raw).map_err(|e| Error::io(&raw, e))?;
    filetime::set_file_times(&raw, FileTime::from_last_access_time(&meta), FileTime::from_last_modification_time(&meta))
        .map_err(|e| Error::io(&raw, e))?;

    let thumb = dir.path().join(format!("{}{THUMB_SUFFIX}", item.name));
    let analysis = generate(&mut item, &raw, &thumb);
    file_metadata(&mut item, &raw, opts);
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
    if item.ext == "txt" {
        set_text(&mut item, &raw);
        item.key_order = crate::model::KeyOrder(crate::text::RECORD_KEYS.iter().map(|k| k.to_string()).collect());
    }
    Ok(Staged { item, dir, auto_tags: opts.folder_auto_tags, added_at: opts.added_at })
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
        let Staged { mut item, dir, auto_tags, added_at } = staged;
        let known = self.folder_ids();
        item.folders.retain(|f| known.contains(f.as_str()));
        if auto_tags {
            crate::autotag::append_new(&mut item.tags, &crate::autotag::auto_tags_for(&self.metadata.folders, &item.folders));
        }
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
            item.modification_time = match added_at {
                Some(t) => t,
                None => self.next_import_time(now_ms()),
            };
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

/// Redraws the thumbnail of an item already in a library and recomputes its
/// palette, as Eagle's Refresh Thumbnail does: width, height and the
/// thumbnail flags are measured again from `raw`, and the thumbnail at
/// `thumb` is replaced only once the new one has been drawn. When the item no
/// longer needs a thumbnail, the old file is deleted so it can't be shown in
/// place of the original. Audio keeps its palette, since Eagle's analyser
/// skips audio. Returns false, with `item` unchanged, when nothing could be
/// drawn.
pub(crate) fn redraw(item: &mut Item, raw: &Path, thumb: &Path) -> bool {
    // A text item has no thumbnail; its card is drawn from its text.
    if item.ext == "txt" {
        return set_text(item, raw);
    }
    let tmp = thumb.with_file_name(".falcon-redraw_thumbnail.png");
    let _ = fs::remove_file(&tmp);
    let mut next = item.clone();
    let Some(analysis) = generate(&mut next, raw, &tmp) else {
        let _ = fs::remove_file(&tmp);
        return false;
    };
    if media::is_audio(&next.ext) {
        // Eagle's refresh takes the tempo again (`generated.bpm`) and drops
        // the key when no beat is found. An existing key keeps its place.
        match tempo::bpm(raw, &next.ext) {
            Some(bpm) => {
                next.extra.insert("bpm".into(), tempo::json_value(bpm));
            }
            None => {
                next.extra.shift_remove("bpm");
            }
        }
    } else {
        let img = match analysis {
            Analysis::Original => thumbnail::decode(raw).ok(),
            Analysis::Thumbnail => thumbnail::decode(&tmp).ok(),
        };
        next.palettes = img.map(|i| palette::extract(&i)).unwrap_or_default();
    }
    let keep = tmp.is_file() && next.remove_thumbnail != Some(true);
    if keep {
        if fs::rename(&tmp, thumb).is_err() {
            let _ = fs::remove_file(&tmp);
            return false;
        }
    } else {
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(thumb);
    }
    *item = next;
    true
}

/// Reads the start of a text item's file into its record (see
/// [`crate::text`]). False, with `item` unchanged, when the file can't be
/// read.
fn set_text(item: &mut Item, raw: &Path) -> bool {
    match crate::text::preview(raw) {
        Ok(text) => {
            item.extra.insert(crate::text::TEXT_KEY.into(), text.into());
            true
        }
        Err(_) => false,
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
    if let Some((img, w, h)) = drawn_thumbnail(&item.ext, raw) {
        thumbnail::write_thumbnail(img, max, thumb).ok()?;
        item.width = w.into();
        item.height = h.into();
        return Some(Analysis::Thumbnail);
    }
    if item.ext == "mhtml" {
        return mhtml_thumbnail(item, raw, thumb, max);
    }
    if !thumbnail::is_supported(&item.ext) {
        // Video macOS can't read goes to FFmpeg first; other video only
        // when QuickLook fails, as Eagle tries its own player before mpv.
        if media::is_video(&item.ext) && falcon_ffmpeg::handles(&item.ext) {
            return ffmpeg_thumbnail(item, raw, thumb).or_else(|| quicklook_thumbnail(item, raw, thumb, max));
        }
        if media::is_video(&item.ext) {
            return quicklook_thumbnail(item, raw, thumb, max).or_else(|| ffmpeg_thumbnail(item, raw, thumb));
        }
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

/// Thumbnails falcon draws itself, with the size Eagle records for them:
/// a 280×140 waveform for audio and a 600×600 specimen for fonts. `None`
/// for other formats, or when drawing fails (QuickLook is tried next).
fn drawn_thumbnail(ext: &str, raw: &Path) -> Option<(DynamicImage, u32, u32)> {
    if media::is_audio(ext) {
        let img = waveform::thumbnail(raw, ext)?;
        return Some((DynamicImage::ImageRgba8(img), waveform::WIDTH, waveform::HEIGHT));
    }
    if FONT_EXTENSIONS.contains(&ext) {
        let data = fs::read(raw).ok()?;
        let metas = fontmeta::font_metas(&data, "")?;
        let img = fontpreview::render(&data, &metas)?;
        return Some((img, fontpreview::SIZE, fontpreview::SIZE));
    }
    if crate::model3d::is_supported(ext) {
        // Eagle draws models itself; on failure QuickLook gets its turn.
        let img = crate::model3d::raster::thumbnail(raw, ext)?;
        let (w, h) = img.dimensions();
        return Some((DynamicImage::ImageRgba8(img), w, h));
    }
    None
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
    thumbnail::write_thumbnail(img, max, thumb).ok()?;
    Some(Analysis::Thumbnail)
}

/// The size Eagle records for a web archive: the 1440 × 900 window its
/// thumbnailer captures the page in.
pub const MHTML_SIZE: (u32, u32) = (1440, 900);

/// An MHTML archive's thumbnail. QuickLook has no renderer for MHTML, so
/// the archive is unpacked into a temporary folder (removed afterwards)
/// and QuickLook renders the page there as it renders an HTML file; the
/// top of the render, cut to Eagle's 16:10, is the thumbnail, and the item
/// gets Eagle's 1440 × 900.
fn mhtml_thumbnail(item: &mut Item, raw: &Path, thumb: &Path, max: u32) -> Option<Analysis> {
    let dir = tempfile::Builder::new().prefix("falcon-mhtml-").tempdir().ok()?;
    let unpacked = crate::mhtml::unpack_file(raw, dir.path(), crate::mhtml::Limits::default(), crate::mhtml::Policy::None).ok()?;
    let img = thumbnail::quicklook(&unpacked.root, thumbnail::QUICKLOOK_SIZE).ok()?;
    let (w, h) = (img.width(), img.height());
    let (ew, eh) = MHTML_SIZE;
    let cut = (u64::from(w) * u64::from(eh) / u64::from(ew)) as u32;
    let img = if cut > 0 && cut < h { img.crop_imm(0, 0, w, cut) } else { img };
    thumbnail::write_thumbnail(img, max, thumb).ok()?;
    item.width = ew.into();
    item.height = eh.into();
    Some(Analysis::Thumbnail)
}

/// A video's thumbnail through the FFmpeg fallback, as Eagle's
/// thumbnailer makes one through its bundled player: the picture ten
/// seconds in (a third of the way through a shorter clip), or a later one
/// when that is black, written per [`thumbnail::video_thumbnail_jpeg`]
/// (a 480-wide JPEG). `None` without FFmpeg or when it can't decode the file.
fn ffmpeg_thumbnail(item: &mut Item, raw: &Path, thumb: &Path) -> Option<Analysis> {
    if !falcon_ffmpeg::available() {
        return None;
    }
    let (info, frame, _) = falcon_ffmpeg::thumbnail(raw).ok()?;
    let img = image::RgbaImage::from_raw(frame.width as u32, frame.height as u32, frame.rgba)?;
    let bytes = thumbnail::video_thumbnail_jpeg(&img).ok()?;
    fs::write(thumb, bytes).ok()?;
    item.width = info.width.into();
    item.height = info.height.into();
    Some(Analysis::Thumbnail)
}

/// Format-specific keys Eagle's thumbnail modules add, appended to the item
/// in the order Eagle assigns them: `rawMetas` for RAW photos,
/// `resolutionWidth`, `resolutionHeight` and `duration` for video,
/// `duration` then `bpm` (when a beat is found) for audio, and `fontMetas`
/// for fonts. A `.url` file's address becomes the item's URL so
/// the viewer can open it; a URL given in the options wins.
fn file_metadata(item: &mut Item, raw: &Path, opts: &ImportOptions) {
    let ext = item.ext.clone();
    if item.url.is_empty() && weblink::is_link_file(&ext) {
        item.url = weblink::read(raw).unwrap_or_default();
    }
    if (exif::is_raw(&ext) || (opts.camera_metadata_all_photos && exif::EXIF_EXTENSIONS.contains(&ext.as_str())))
        && let Some(meta) = exif::read(raw)
    {
        item.extra.insert("rawMetas".into(), Value::Object(meta.to_raw_metas()));
    }
    if media::is_video(&ext) {
        item.extra.insert("resolutionWidth".into(), json!(item.width));
        item.extra.insert("resolutionHeight".into(), json!(item.height));
    }
    if (media::is_video(&ext) || media::is_audio(&ext))
        && let Some(d) = media::duration(raw, &ext).or_else(|| thumbnail::spotlight(raw).duration)
    {
        item.extra.insert("duration".into(), json!(d));
    }
    if media::is_audio(&ext)
        && let Some(bpm) = tempo::bpm(raw, &ext)
    {
        item.extra.insert("bpm".into(), tempo::json_value(bpm));
    }
    if FONT_EXTENSIONS.contains(&ext.as_str())
        && let Some(metas) = fs::read(raw).ok().and_then(|data| fontmeta::font_metas(&data, &ids::new_id()))
    {
        if opts.font_weight_tag
            && let Some(w) = metas.get("weight").and_then(|w| w.as_str())
        {
            item.tags.push(w.to_string());
        }
        item.extra.insert("fontMetas".into(), Value::Object(metas));
    }
}

/// Appends keywords to the tags. As in Eagle, when there are keywords the
/// whole list is then de-duplicated, keeping first occurrences.
fn add_keyword_tags(tags: &mut Vec<String>, keywords: Vec<String>) {
    if keywords.is_empty() {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    let all: Vec<String> = tags.drain(..).chain(keywords).filter(|t| seen.insert(t.clone())).collect();
    *tags = all;
}

/// Font formats whose metadata falcon reads.
pub const FONT_EXTENSIONS: [&str; 5] = ["ttf", "otf", "ttc", "woff", "woff2"];

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

/// Copies a file's size, birth time and mtime (ms) onto `item`, as Eagle
/// re-reads them before refreshing a thumbnail.
pub(crate) fn restat(item: &mut Item, path: &Path) -> Result<()> {
    let meta = fs::metadata(path).map_err(|e| Error::io(path, e))?;
    item.size = meta.len();
    item.mtime = to_ms(meta.modified().ok());
    item.btime = meta.created().ok().map_or(item.mtime, |t| to_ms(Some(t)));
    Ok(())
}

fn to_ms(t: Option<SystemTime>) -> u64 {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

fn now_ms() -> u64 {
    to_ms(Some(SystemTime::now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import(file: &str, bytes: &[u8], opts: &ImportOptions) -> Item {
        import_with_thumb(file, bytes, opts).0
    }

    #[test]
    fn a_given_name_replaces_the_file_name_and_keeps_the_extension() {
        let named = ImportOptions { name: Some(" My: note ".into()), ..Default::default() };
        let item = import("raw.txt", b"hi", &named);
        assert_eq!((item.name.as_str(), item.ext.as_str()), ("My note", "txt"));
        assert_eq!(import("raw.txt", b"hi", &ImportOptions::default()).name, "raw");
    }

    #[test]
    fn items_from_another_app_keep_their_trash_state_and_date() {
        let opts = ImportOptions { trashed: true, added_at: Some(1_520_139_967_000), ..Default::default() };
        let item = import("old.txt", b"hi", &opts);
        assert!(item.is_deleted);
        assert_eq!(item.modification_time, 1_520_139_967_000);
        assert!(!import("new.txt", b"hi", &ImportOptions::default()).is_deleted);
    }

    /// Imports into a fresh library; also returns the thumbnail's size.
    fn import_with_thumb(file: &str, bytes: &[u8], opts: &ImportOptions) -> (Item, Option<(u32, u32)>) {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
        let src = tmp.path().join(file);
        fs::write(&src, bytes).unwrap();
        let id = lib.import_file(&src, opts).unwrap();
        let item = lib.item(&id).unwrap().clone();
        let thumb = item.thumbnail_path(lib.root()).and_then(|p| thumbnail::decode(&p).ok()).map(|i| (i.width(), i.height()));
        (item, thumb)
    }

    fn keys(item: &Item) -> Vec<&str> {
        item.extra.keys().map(|k| k.as_str()).collect()
    }

    /// Runs only the metadata step: QuickLook can stall on the tiny made-up
    /// TIFF, and the thumbnail doesn't matter here.
    fn metadata_only(file: &str, bytes: &[u8], opts: &ImportOptions) -> Item {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join(file);
        fs::write(&src, bytes).unwrap();
        let mut item: Item = serde_json::from_value(json!({
            "id": "", "name": "shot", "size": bytes.len(), "btime": 0, "mtime": 0, "ext": src.extension().unwrap().to_str(),
            "tags": [], "folders": [], "isDeleted": false, "url": "", "annotation": "", "modificationTime": 0,
            "height": 0, "width": 0, "lastModified": 0, "palettes": []
        }))
        .unwrap();
        file_metadata(&mut item, &src, opts);
        item
    }

    #[test]
    fn link_files_set_the_item_url() {
        let none = ImportOptions::default();
        let url = metadata_only("go.url", b"[InternetShortcut]\r\nURL=https://example.com/a\r\n", &none);
        assert_eq!(url.url, "https://example.com/a");
        let xml = b"<plist><dict><key>URL</key><string>https://example.com/w</string></dict></plist>";
        assert_eq!(metadata_only("go.webloc", xml, &none).url, "", "a .webloc is a plain item, as in Eagle");
        assert_eq!(metadata_only("empty.url", b"[InternetShortcut]\n", &none).url, "");
        assert_eq!(metadata_only("note.txt", b"[InternetShortcut]\nURL=https://example.com/\n", &none).url, "");

        // An item that already has a URL (from `opts.url`) keeps it.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("go.url");
        fs::write(&src, "[InternetShortcut]\nURL=https://example.com/a\n").unwrap();
        let mut item = metadata_only("x.txt", b"hi", &none);
        item.ext = "url".into();
        item.url = "https://given.example/".into();
        let given = ImportOptions { url: item.url.clone(), ..Default::default() };
        file_metadata(&mut item, &src, &given);
        assert_eq!(item.url, "https://given.example/");
    }

    #[test]
    fn failures_are_classified() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = ImportOptions { palette: false, ..Default::default() };
        let fail = |p: &Path| Failure::new(p, &stage(p, &opts).unwrap_err());

        let missing = tmp.path().join("gone.png");
        assert_eq!(fail(&missing).kind, FailKind::Missing);
        assert_eq!(fail(tmp.path()).kind, FailKind::NotAFile);
        let empty = tmp.path().join("empty.png");
        fs::write(&empty, b"").unwrap();
        let f = fail(&empty);
        assert_eq!((f.kind, f.path.as_path()), (FailKind::Empty, empty.as_path()));
        assert!(f.detail.contains("empty"), "{}", f.detail);

        for name in ["README", "movie.mp4.crdownload", "song.part", "page.download"] {
            let p = tmp.path().join(name);
            fs::write(&p, b"data").unwrap();
            assert_eq!(fail(&p).kind, FailKind::Unrecognized, "{name}");
        }
        // Unknown but present extensions import, as icon-only items.
        let odd = tmp.path().join("notes.xyz123");
        fs::write(&odd, b"data").unwrap();
        assert!(stage(&odd, &opts).is_ok());

        let mut lib = Library::create(tmp.path().join("T.library")).unwrap();
        let ok = tmp.path().join("a.txt");
        fs::write(&ok, b"hi").unwrap();
        let bad = ImportOptions { folders: vec!["NOPE".into()], ..opts.clone() };
        let err = lib.import_file(&ok, &bad).unwrap_err();
        assert_eq!(Failure::new(&ok, &err).kind, FailKind::UnknownFolder);

        let full = Error::io(tmp.path().join("falcon-importXYZ/a.png"), std::io::Error::from(std::io::ErrorKind::StorageFull));
        assert_eq!(Failure::new(&ok, &full).kind, FailKind::NoSpace);
        let copy = Error::io(tmp.path().join("falcon-importXYZ/a.png"), std::io::Error::other("boom"));
        assert_eq!(Failure::new(&ok, &copy).kind, FailKind::CopyFailed);
        let write = Error::io(tmp.path().join("T.library/images/X.info"), std::io::Error::other("boom"));
        assert_eq!(Failure::new(&ok, &write).kind, FailKind::LibraryWrite);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = tmp.path().join("locked.png");
            fs::write(&locked, b"x").unwrap();
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
            // Root can read anything; only check when the file is unreadable.
            if fs::read(&locked).is_err() {
                assert_eq!(fail(&locked).kind, FailKind::Unreadable);
            }
        }
    }

    #[test]
    fn raw_photo_gets_raw_metas() {
        let opts = ImportOptions { palette: false, ..Default::default() };
        let item = metadata_only("shot.nef", &crate::exif::tests::tiff_with_exif(), &opts);
        let m = &item.extra["rawMetas"];
        assert_eq!(m["camera"], "Nikon D850");
        assert_eq!(m["isoSpeed"], 400);
        assert_eq!(m["shutter"], "1/250.0 sec");
        assert_eq!(m["aperture"], "f/2.8");
        assert_eq!(m["focalLength"], "50.0 mm");
        // A TIFF only gets them when asked.
        let item = metadata_only("shot.tif", &crate::exif::tests::tiff_with_exif(), &opts);
        assert!(!item.extra.contains_key("rawMetas"));
        let all = ImportOptions { camera_metadata_all_photos: true, ..opts };
        let item = metadata_only("shot.tif", &crate::exif::tests::tiff_with_exif(), &all);
        assert!(item.extra.contains_key("rawMetas"));
    }

    #[test]
    fn keywords_become_tags_when_asked() {
        let png = crate::keywords::tests::png_with_xmp(true);
        let opts = ImportOptions { palette: false, tags: vec!["beach".into(), "mine".into()], ..Default::default() };
        assert_eq!(import("k.png", &png, &opts).tags, ["beach", "mine"]);
        let on = ImportOptions { keywords_as_tags: true, ..opts };
        assert_eq!(import("k.png", &png, &on).tags, ["beach", "mine", "sunset", "sea & sky"]);
    }

    #[test]
    fn audio_gets_duration() {
        let opts = ImportOptions { palette: false, ..Default::default() };
        let (item, thumb) = import_with_thumb("tone.wav", &crate::media::tests::wav(8000, 1.25, |t| (t * 300.0).sin()), &opts);
        assert_eq!(item.extra["duration"], 1.25);
        assert_eq!((item.width, item.height), (280, 140));
        assert_eq!(thumb, Some((280, 140)), "the waveform is the thumbnail");
        assert!(!item.extra.contains_key("bpm"), "a steady tone has no beat");
    }

    #[test]
    fn video_macos_cannot_read_is_thumbnailed_through_ffmpeg() {
        if !falcon_ffmpeg::available() {
            eprintln!("skipped: FFmpeg is not installed (scripts/build-ffmpeg.sh)");
            return;
        }
        // Two seconds of 64×48 frames, black for the first one: Eagle's
        // spot (a third of the way in) is black, so a later frame is used.
        let clip = falcon_ffmpeg::testing::Clip { width: 64, height: 48, frames: 50, fps: 25, audio_rate: Some(8000) };
        let avi = clip.avi(|i, _| if i < 25 { [0, 0, 0] } else { [200, 40, 40] });
        let (item, thumb) = import_with_thumb("clip.avi", &avi, &ImportOptions::default());
        assert_eq!((item.width, item.height), (64, 48));
        assert_eq!(thumb, Some((480, 360)), "a 480-wide thumbnail as Eagle draws");
        assert!((item.extra["duration"].as_f64().unwrap() - 2.0).abs() < 0.05, "{}", item.extra["duration"]);
        assert_eq!((item.extra["resolutionWidth"].clone(), item.extra["resolutionHeight"].clone()), (json!(64), json!(48)));
        let reddish = item.palettes.first().is_some_and(|p| p.color[0] > 150 && p.color[1] < 90);
        assert!(reddish, "the thumbnail is the first non-black frame: {:?}", item.palettes.first().map(|p| p.color));
    }

    #[test]
    fn av1_video_is_thumbnailed_through_dav1d() {
        if !falcon_ffmpeg::ffi::api().is_ok_and(|api| falcon_ffmpeg::ffi::has_decoder(api, "libdav1d")) {
            eprintln!("skipped: FFmpeg with dav1d is not installed (scripts/build-ffmpeg.sh)");
            return;
        }
        // falcon-ffmpeg's AV1 fixture (a second of flat 64×48 frames, frame
        // `i` coloured (10 + 7i, 100, 200)) rewrapped in each container.
        // WebM and Matroska go to FFmpeg first; MP4 to QuickLook first and
        // FFmpeg when QuickLook can't read AV1.
        let ivf = include_bytes!("../../falcon-ffmpeg/testdata/av1.ivf");
        let clip = falcon_ffmpeg::testing::av1::Av1Clip::from_ivf(ivf).unwrap();
        for (name, bytes) in [("clip.webm", clip.matroska(true)), ("clip.mkv", clip.matroska(false)), ("clip.mp4", clip.mp4())] {
            let (item, thumb) = import_with_thumb(name, &bytes, &ImportOptions::default());
            assert_eq!((item.width, item.height), (64, 48), "{name}");
            assert!((item.extra["duration"].as_f64().unwrap() - 1.0).abs() < 0.05, "{name}: {}", item.extra["duration"]);
            let c = item.palettes.first().map(|p| p.color).unwrap();
            assert!(c[1].abs_diff(100) < 16 && c[2].abs_diff(200) < 16, "{name}: a frame of the clip, not black or an icon: {c:?}");
            if name.ends_with(".mp4") && thumb != Some((480, 360)) {
                // QuickLook read it (macOS 14+ decodes AV1 on Apple silicon
                // with an AV1 decoder) and chose its own frame and size.
                assert!(thumb.is_some(), "{name}");
                continue;
            }
            assert_eq!(thumb, Some((480, 360)), "{name}: a 480-wide thumbnail as Eagle draws");
            // Eagle's spot, a third of the way in, is frame 8: (66, 100, 200).
            assert!(c[0].abs_diff(66) < 16, "{name}: {c:?}");
        }
    }

    #[test]
    fn audio_with_a_beat_gets_bpm_after_duration() {
        let opts = ImportOptions { palette: false, ..Default::default() };
        let click = |t: f64| { let s = t % 0.5; if s < 0.004 { 0.8 * (s * 9000.0).sin() } else { 0.0 } };
        let item = metadata_only("beat.wav", &crate::media::tests::wav(22050, 12.0, click), &opts);
        let bpm = item.extra["bpm"].as_f64().unwrap();
        assert!((bpm - 120.0).abs() <= 1.0, "{bpm}");
        let keys: Vec<&str> = item.extra.keys().map(String::as_str).collect();
        assert_eq!(keys, ["duration", "bpm"]);
    }

    #[test]
    fn text_files_keep_their_text_in_eagles_record_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("T.library");
        let mut lib = Library::create(&root).unwrap();
        let src = tmp.path().join("Notes.txt");
        fs::write(&src, "Line one\nLine two\n").unwrap();
        let id = lib.import_file(&src, &ImportOptions::default()).unwrap();
        let item = lib.item(&id).unwrap().clone();
        assert_eq!(item.extra["text"], "Line one\nLine two\n");
        assert!(item.thumbnail_path(lib.root()).is_none_or(|p| !p.exists()), "no thumbnail file");
        let json: Value = serde_json::from_slice(&fs::read(item.info_dir(lib.root()).join("metadata.json")).unwrap()).unwrap();
        let written: Vec<&str> = json.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        assert_eq!(written, crate::text::RECORD_KEYS);

        // An empty file (File ▸ New ▸ Text File) gets an empty text.
        let empty = tmp.path().join("Untitled.txt");
        fs::write(&empty, "").unwrap();
        let id2 = lib.import_file(&empty, &ImportOptions::default()).unwrap();
        assert_eq!(lib.item(&id2).unwrap().extra["text"], "");

        // Refresh Thumbnail reads the text again, also for an item that
        // had none (imported by an older falcon).
        let raw = item.original_path(lib.root());
        fs::write(&raw, "Rewritten").unwrap();
        let mut stale = lib.item(&id).unwrap().clone();
        stale.extra.shift_remove("text");
        lib.update_item(stale).unwrap();
        assert!(lib.refresh_thumbnail(&id).unwrap());
        assert_eq!(Library::open(&root).unwrap().item(&id).unwrap().extra["text"], "Rewritten");
    }

    #[test]
    fn refresh_thumbnail_takes_the_tempo_again() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("T.library");
        let mut lib = Library::create(&root).unwrap();
        let src = tmp.path().join("beat.wav");
        let click = |t: f64| { let s = t % 0.5; if s < 0.004 { 0.8 * (s * 9000.0).sin() } else { 0.0 } };
        fs::write(&src, crate::media::tests::wav(22050, 12.0, click)).unwrap();
        let opts = ImportOptions { palette: false, ..Default::default() };
        let id = lib.import_file(&src, &opts).unwrap();
        // A stale tempo, with a key after it whose place must not change.
        let mut item = lib.item(&id).unwrap().clone();
        item.extra.insert("bpm".into(), json!(90.5));
        item.extra.insert("star".into(), json!(3));
        lib.update_item(item).unwrap();
        let mut lib = Library::open(&root).unwrap();
        assert!(lib.refresh_thumbnail(&id).unwrap());
        let lib = Library::open(&root).unwrap();
        let item = lib.item(&id).unwrap();
        let bpm = item.extra["bpm"].as_f64().unwrap();
        assert!((bpm - 120.0).abs() <= 1.0, "{bpm}");
        assert_eq!(keys(item), ["duration", "bpm", "star"]);

        // Without a beat the key goes, as Eagle sets it to undefined.
        let raw = item.original_path(lib.root());
        fs::write(&raw, crate::media::tests::wav(8000, 1.25, |t| (t * 300.0).sin())).unwrap();
        let mut lib = Library::open(&root).unwrap();
        assert!(lib.refresh_thumbnail(&id).unwrap());
        assert_eq!(keys(Library::open(&root).unwrap().item(&id).unwrap()), ["duration", "star"]);
    }

    #[test]
    fn font_gets_font_metas_and_weight_tag() {
        let Ok(data) = fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") else { return };
        let opts = ImportOptions { palette: false, tags: vec!["mine".into()], ..Default::default() };
        let (item, thumb) = import_with_thumb("Arial.ttf", &data, &opts);
        assert_eq!((item.width, item.height), (600, 600));
        assert_eq!(thumb, Some((320, 320)), "the specimen is scaled to the thumbnail limit");
        assert_eq!(item.extra["fontMetas"]["postScriptName"]["en"], "ArialMT");
        assert_eq!(item.tags, ["mine", "Regular"]);
        assert!(keys(&item).contains(&"fontMetas"));
        let item = import("Arial.ttf", &data, &ImportOptions { font_weight_tag: false, ..opts });
        assert_eq!(item.tags, ["mine"]);
    }
}
