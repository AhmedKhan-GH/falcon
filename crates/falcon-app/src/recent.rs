//! Recently Used: the items opened in each library, newest first, as
//! Eagle keeps them. Eagle stores the list in the app's own storage per
//! library, not in the library, so other apps never see it; falcon keeps
//! it in its saved state, keyed by library path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Most ids kept per library, as in Eagle.
const LIMIT: usize = 5000;
/// Eagle records a batch only below this size (opening or copying a
/// hundred items at once doesn't count as using them).
const BATCH: usize = 20;
/// How long the viewer must stay on an item before it counts, in seconds.
pub const VIEW_DELAY: f64 = 1.0;

/// Each library's list, by library path.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RecentItems(HashMap<String, Vec<String>>);

fn key(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

impl RecentItems {
    /// The library's ids, newest first.
    pub fn list(&self, root: &Path) -> &[String] {
        self.0.get(&key(root)).map_or(&[], Vec::as_slice)
    }

    /// Puts one opened item first.
    pub fn add(&mut self, root: &Path, id: &str) {
        let list = self.0.entry(key(root)).or_default();
        list.retain(|x| x != id);
        list.insert(0, id.to_string());
        list.truncate(LIMIT);
    }

    /// Puts several items first, keeping their order, unless there are
    /// [`BATCH`] or more of them (Eagle skips those).
    pub fn add_many(&mut self, root: &Path, ids: &[String]) {
        if ids.is_empty() || ids.len() >= BATCH {
            return;
        }
        for id in ids.iter().rev() {
            self.add(root, id);
        }
    }

    /// Forgets the library's list.
    pub fn clear(&mut self, root: &Path) {
        self.0.remove(&key(root));
    }
}

/// The item id of a library original (`images/<id>.info/<name>.<ext>`),
/// so file-based open commands can record what they opened.
pub fn item_id(file: &Path) -> Option<String> {
    let dir = file.parent()?.file_name()?.to_str()?;
    dir.strip_suffix(".info").filter(|id| !id.is_empty()).map(str::to_string)
}

/// The ids of those `files` that are library originals.
pub fn item_ids(files: &[PathBuf]) -> Vec<String> {
    files.iter().filter_map(|f| item_id(f)).collect()
}

/// Times how long the viewer stays on one item, so it is recorded once
/// it has been shown for [`VIEW_DELAY`] (Eagle waits a second, so
/// stepping quickly through items doesn't fill the list).
#[derive(Debug, Default)]
pub struct ViewTimer {
    current: Option<(String, f64, bool)>,
}

impl ViewTimer {
    /// Call each frame with the viewer's item and the time; returns the
    /// id once, when it has been shown long enough.
    pub fn tick(&mut self, viewing: Option<&str>, now: f64) -> Option<String> {
        let Some(id) = viewing else {
            self.current = None;
            return None;
        };
        match &mut self.current {
            Some((cur, since, done)) if cur == id => {
                if !*done && now - *since >= VIEW_DELAY {
                    *done = true;
                    return Some(id.to_string());
                }
                None
            }
            _ => {
                self.current = Some((id.to_string(), now, false));
                None
            }
        }
    }

    /// Seconds until the current item counts, if it hasn't yet.
    pub fn wait(&self, now: f64) -> Option<f64> {
        match &self.current {
            Some((_, since, false)) => Some((since + VIEW_DELAY - now).max(0.0)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn newest_first_without_repeats() {
        let root = Path::new("/s/A.library");
        let mut r = RecentItems::default();
        r.add(root, "a");
        r.add(root, "b");
        r.add(root, "a");
        assert_eq!(r.list(root), ids(&["a", "b"]));
        r.add_many(root, &ids(&["c", "b"]));
        assert_eq!(r.list(root), ids(&["c", "b", "a"]));
        assert!(r.list(Path::new("/s/B.library")).is_empty());
        r.clear(root);
        assert!(r.list(root).is_empty());
    }

    #[test]
    fn big_batches_are_not_recorded_and_the_list_is_capped() {
        let root = Path::new("/s/A.library");
        let mut r = RecentItems::default();
        let many: Vec<String> = (0..BATCH).map(|n| n.to_string()).collect();
        r.add_many(root, &many);
        assert!(r.list(root).is_empty());
        r.add_many(root, &many[..BATCH - 1]);
        assert_eq!(r.list(root).len(), BATCH - 1);
        for n in 0..LIMIT + 10 {
            r.add(root, &format!("x{n}"));
        }
        assert_eq!(r.list(root).len(), LIMIT);
        assert_eq!(r.list(root)[0], format!("x{}", LIMIT + 9));
    }

    #[test]
    fn ids_come_from_info_folders() {
        assert_eq!(item_id(Path::new("/s/A.library/images/KAB12.info/cat.png")).as_deref(), Some("KAB12"));
        assert_eq!(item_id(Path::new("/s/docs/index.html")), None);
        assert_eq!(item_id(Path::new("/s/A.library/images/.info/x.png")), None);
    }

    #[test]
    fn the_viewer_counts_after_a_second_once() {
        let mut t = ViewTimer::default();
        assert_eq!(t.tick(Some("a"), 0.0), None);
        assert_eq!(t.wait(0.4), Some(0.6));
        assert_eq!(t.tick(Some("a"), 0.5), None);
        assert_eq!(t.tick(Some("b"), 0.9), None, "switching restarts the wait");
        assert_eq!(t.tick(Some("b"), 1.95), Some("b".into()));
        assert_eq!(t.tick(Some("b"), 3.0), None, "recorded once");
        assert_eq!(t.wait(3.0), None);
        assert_eq!(t.tick(None, 3.1), None);
        assert_eq!(t.tick(Some("b"), 3.2), None, "reopening waits again");
        assert_eq!(t.tick(Some("b"), 4.3), Some("b".into()));
    }
}
