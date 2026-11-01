//! Telling Eagle library folders apart from importable paths.

/// Whether `path` names an Eagle library folder (`*.library`). Such a path
/// is opened, never imported, so its `metadata.json`, item folders and
/// thumbnails don't turn into items.
pub fn is_library_path(path: &std::path::Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        let n = n.as_bytes();
        n.len() > ".library".len() && n[n.len() - ".library".len()..].eq_ignore_ascii_case(b".library")
    })
}

/// Splits dropped, pasted or picked paths into libraries to open and paths
/// to import, keeping their order.
pub fn split_libraries(paths: Vec<std::path::PathBuf>) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    paths.into_iter().partition(|p| is_library_path(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libraries_are_split_from_importables() {
        use std::path::PathBuf;
        let paths: Vec<PathBuf> = ["/a/Photos.library", "/a/shot.png", "/a/Folder", "/a/Old.LIBRARY/", "/a/library", "/a/.library", "/a/notes.library.txt", "/a/B.library"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let (libraries, rest) = split_libraries(paths);
        let names = |v: &[PathBuf]| v.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(&libraries), ["/a/Photos.library", "/a/Old.LIBRARY/", "/a/B.library"]);
        assert_eq!(names(&rest), ["/a/shot.png", "/a/Folder", "/a/library", "/a/.library", "/a/notes.library.txt"]);
        assert_eq!(split_libraries(Vec::new()), (Vec::new(), Vec::new()));
    }
}
