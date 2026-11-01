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
use crate::{exif, fontmeta, fontpreview, ids, keywords, media, palette, tempo, thumbnail, waveform};

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
        key_order: Default::default(),
        extra: Default::default(),
    };

    if opts.keywords_as_tags {
        add_keyword_tags(&mut item.tags, keywords::read(src, &item.ext));
    }

    let dir = tempfile::Builder::new().prefix("falcon-import").tempdir().map_err(|e| Error::io(src, e))?;
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

/// Redraws the thumbnail of an item already in a library and recomputes its
/// palette, as Eagle's Refresh Thumbnail does: width, height and the
/// thumbnail flags are measured again from `raw`, and the thumbnail at
/// `thumb` is replaced only once the new one has been drawn. When the item no
/// longer needs a thumbnail, the old file is deleted so it can't be shown in
/// place of the original. Audio keeps its palette, since Eagle's analyser
/// skips audio. Returns false, with `item` unchanged, when nothing could be
/// drawn.
pub(crate) fn redraw(item: &mut Item, raw: &Path, thumb: &Path) -> bool {
    let tmp = thumb.with_file_name(".falcon-redraw_thumbnail.png");
    let _ = fs::remove_file(&tmp);
    let mut next = item.clone();
    let Some(analysis) = generate(&mut next, raw, &tmp) else {
        let _ = fs::remove_file(&tmp);
        return false;
    };
    if !media::is_audio(&next.ext) {
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

/// Format-specific keys Eagle's thumbnail modules add, appended to the item
/// in the order Eagle assigns them: `rawMetas` for RAW photos,
/// `resolutionWidth`, `resolutionHeight` and `duration` for video,
/// `duration` then `bpm` (when a beat is found) for audio, and `fontMetas`
/// for fonts.
fn file_metadata(item: &mut Item, raw: &Path, opts: &ImportOptions) {
    let ext = item.ext.clone();
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
pub const FONT_EXTENSIONS: [&str; 6] = ["ttf", "otf", "ttc", "otc", "woff", "woff2"];

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
