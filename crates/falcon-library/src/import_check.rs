//! The duplicate check on import, after Eagle's Duplicate File Alert.
//!
//! Eagle finds an existing item with the same metadata key (see the Import
//! chapter) and then compares the first 4,096,000 bytes of both files.
//! falcon looks for items, not in the Trash, with the same size and
//! extension, and compares the whole files by BLAKE3 hash, which is at
//! least as strict. Eagle skips svg, tif, tiff and the formats its key
//! leaves empty (bmp, tga, txt, and anything it can't preview); falcon
//! skips the same list and files without an extension.
//!
//! What happens to a duplicate is the caller's choice (the app asks):
//! import it anyway, drop it, or use the existing item instead, which
//! [`Library::use_existing_for_import`] does.

use std::path::{Path, PathBuf};

use crate::duplicates::content_hash;
use crate::error::{Error, Result};
use crate::library::Library;

/// Extensions the check leaves alone.
pub const UNCHECKED: &[&str] = &["svg", "tif", "tiff", "bmp", "tga", "txt"];

/// Whether files with extension `ext` (lowercase) are checked.
pub fn is_checked(ext: &str) -> bool {
    !ext.is_empty() && !UNCHECKED.contains(&ext)
}

/// The id of the first candidate whose file has the same bytes as `path`.
pub fn same_file(path: &Path, candidates: &[(String, PathBuf)]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let hash = content_hash(path).ok()?;
    candidates.iter().find(|(_, p)| content_hash(p).is_ok_and(|h| h == hash)).map(|(id, _)| id.clone())
}

impl Library {
    /// Items, not in the Trash, that a new file of `size` bytes with
    /// extension `ext` could duplicate, with their originals' paths. Empty
    /// for extensions that aren't checked. Cheap: no file is read.
    pub fn duplicate_candidates(&self, size: u64, ext: &str) -> Vec<(String, PathBuf)> {
        if !is_checked(ext) {
            return Vec::new();
        }
        self.items()
            .iter()
            .filter(|i| !i.is_deleted && i.size == size && i.ext.eq_ignore_ascii_case(ext))
            .map(|i| (i.id.clone(), i.original_path(self.root())))
            .collect()
    }

    /// Eagle's "Use existing file": instead of importing a duplicate, the
    /// existing item joins each of `folders` it isn't in yet, taking that
    /// folder's auto-tags (see [`crate::autotag`]), and its
    /// `modificationTime` becomes the import time, so it sorts with the
    /// new items. Other tags and notes of the would-be import are not
    /// carried over, as in Eagle.
    pub fn use_existing_for_import(&mut self, existing: &str, folders: &[String]) -> Result<()> {
        let Some(mut item) = self.item(existing).cloned() else {
            return Err(Error::Import { path: self.root().to_path_buf(), reason: format!("no item {existing}") });
        };
        let known = self.folder_ids();
        for f in folders.iter().filter(|f| known.contains(f.as_str())) {
            crate::autotag::join_folder(&mut item, &self.metadata.folders, f);
        }
        item.modification_time = self.next_import_time(crate::edit::now_ms());
        self.update_item(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{ImportOptions, stage};

    #[test]
    fn unchecked_formats() {
        assert!(is_checked("png") && is_checked("mp4") && is_checked("ttf"));
        for ext in ["svg", "tif", "tiff", "bmp", "tga", "txt", ""] {
            assert!(!is_checked(ext), "{ext}");
        }
    }

    #[test]
    fn finds_and_uses_the_existing_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("D.library")).unwrap();
        let folder = lib.create_folder(None, "F").unwrap();
        lib.set_folder_auto_tags(&folder, &["auto".into()]).unwrap();
        let png = |name: &str, shade: u8| {
            let p = tmp.path().join(name);
            image::RgbImage::from_pixel(8, 8, image::Rgb([shade, 0, 0])).save(&p).unwrap();
            p
        };
        let opts = ImportOptions { palette: false, tags: vec!["mine".into()], ..Default::default() };
        let a = lib.import_file(png("a.png", 10), &opts).unwrap();
        let old_time = lib.item(&a).unwrap().modification_time;

        // Same bytes, other name: a duplicate of `a`.
        let into = ImportOptions { folders: vec![folder.clone()], tags: vec!["new".into()], ..opts.clone() };
        let copy = stage(png("copy.png", 10), &into).unwrap();
        let cands = lib.duplicate_candidates(copy.item().size, &copy.item().ext);
        assert_eq!(cands.len(), 1);
        assert_eq!(same_file(&copy.original(), &cands), Some(a.clone()));

        // Other bytes, even against the same candidates: not a duplicate.
        let other = stage(png("other.png", 200), &into).unwrap();
        assert_eq!(same_file(&other.original(), &cands), None);
        // Another extension or the Trash: no candidates.
        assert!(lib.duplicate_candidates(copy.item().size, "jpg").is_empty());
        assert!(lib.duplicate_candidates(copy.item().size, "svg").is_empty());

        let before = lib.items().len();
        lib.use_existing_for_import(&a, &into.folders).unwrap();
        let item = lib.item(&a).unwrap();
        assert_eq!(item.folders, std::slice::from_ref(&folder));
        assert_eq!(item.tags, ["mine", "auto"], "the folder's auto-tags, not the import's tags");
        assert!(item.modification_time > old_time);
        assert_eq!(lib.items().len(), before);
        drop(copy);

        lib.set_trashed(std::slice::from_ref(&a), true).unwrap();
        assert!(lib.duplicate_candidates(lib.item(&a).unwrap().size, "png").is_empty());
        assert!(lib.use_existing_for_import("nope", &[]).is_err());
    }
}
