//! The list of open libraries. The app draws one workspace (the active
//! one, kept outside the list so the drawing code can borrow it directly);
//! the others wait here in switcher order with all their state. Pure list
//! logic, generic over the workspace type so it can be tested alone.

use std::path::{Path, PathBuf};

/// The workspaces that are not being drawn, and where the active one sits
/// among them. Logical order is `list` with the active workspace inserted
/// at `slot`.
pub struct Parked<T> {
    list: Vec<T>,
    slot: usize,
}

impl<T> Default for Parked<T> {
    fn default() -> Self {
        Parked { list: Vec::new(), slot: 0 }
    }
}

impl<T> Parked<T> {
    /// How many workspaces are open, the active one included.
    pub fn len(&self) -> usize {
        self.list.len() + 1
    }

    /// The active workspace's position in the switcher.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// Every workspace in switcher order.
    pub fn iter<'a>(&'a self, active: &'a T) -> impl Iterator<Item = &'a T> + 'a {
        let (before, after) = self.list.split_at(self.slot);
        before.iter().chain(std::iter::once(active)).chain(after.iter())
    }

    /// Every parked workspace, mutably (to flush them on quit).
    pub fn parked_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.list.iter_mut()
    }

    /// Makes the workspace at position `to` the active one; the old active
    /// one keeps its place. Out-of-range positions change nothing.
    pub fn focus(&mut self, active: &mut T, to: usize) {
        if to == self.slot || to >= self.len() {
            return;
        }
        let next = self.list.remove(if to < self.slot { to } else { to - 1 });
        let old = std::mem::replace(active, next);
        self.list.insert(if self.slot < to { self.slot } else { self.slot - 1 }, old);
        self.slot = to;
    }

    /// Adds `new` at the end of the switcher and makes it active.
    pub fn push(&mut self, active: &mut T, new: T) {
        let old = std::mem::replace(active, new);
        self.list.insert(self.slot, old);
        self.slot = self.list.len();
    }

    /// Removes the workspace at position `at` and returns it. Closing the
    /// active one activates its right neighbour, or the left one at the
    /// end. The last workspace can't be removed (None); the caller replaces
    /// it instead.
    pub fn close(&mut self, active: &mut T, at: usize) -> Option<T> {
        if self.list.is_empty() || at >= self.len() {
            return None;
        }
        let at = if at == self.slot {
            let old = self.slot;
            self.focus(active, if old + 1 < self.len() { old + 1 } else { old - 1 });
            old
        } else {
            at
        };
        let removed = self.list.remove(if at < self.slot { at } else { at - 1 });
        if at < self.slot {
            self.slot -= 1;
        }
        Some(removed)
    }

    /// The position after (or before) the active one, wrapping around.
    pub fn step(&self, forward: bool) -> usize {
        step(self.slot, self.len(), forward)
    }
}

/// `i` moved one place forward or back in a ring of `n`.
pub fn step(i: usize, n: usize, forward: bool) -> usize {
    if n == 0 {
        return 0;
    }
    if forward { (i + 1) % n } else { (i + n - 1) % n }
}

/// The libraries to reopen on launch and which one to show, from what was
/// saved: duplicates and those `exists` rejects are dropped, and the active
/// index follows its library (or is clamped). State saved before several
/// libraries could be open has only `library`.
pub fn restore(libraries: &[PathBuf], active: Option<usize>, library: Option<&Path>, exists: impl Fn(&Path) -> bool) -> (Vec<PathBuf>, usize) {
    let (list, want): (Vec<PathBuf>, Option<&Path>) = if libraries.is_empty() {
        (library.map(Path::to_path_buf).into_iter().collect(), library)
    } else {
        (libraries.to_vec(), active.and_then(|i| libraries.get(i)).map(PathBuf::as_path))
    };
    let mut kept: Vec<PathBuf> = Vec::new();
    for p in list {
        if !kept.contains(&p) && exists(&p) {
            kept.push(p);
        }
    }
    let index = want.and_then(|w| kept.iter().position(|p| p == w)).unwrap_or(0);
    (kept, index)
}

/// Adds `path` to the front of the recent list (at most `cap` long).
pub fn remember(recent: &mut Vec<PathBuf>, path: &Path, cap: usize) {
    recent.retain(|p| p != path);
    recent.insert(0, path.to_path_buf());
    recent.truncate(cap);
}

/// The recent libraries that aren't open, in recent order.
pub fn not_open(recent: &[PathBuf], open: &[PathBuf]) -> Vec<PathBuf> {
    recent.iter().filter(|p| !open.contains(p)).cloned().collect()
}

/// A folder path for display, with the home folder shown as `~`.
pub fn tilde(path: &Path) -> String {
    tilde_in(path, std::env::var_os("HOME").map(PathBuf::from).as_deref())
}

fn tilde_in(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parked letters with `a` active.
    fn ring(others: &[char]) -> (char, Parked<char>) {
        let mut p = Parked::default();
        let mut active = 'a';
        for &c in others {
            p.push(&mut active, c);
        }
        p.focus(&mut active, 0);
        (active, p)
    }

    fn order(active: &char, p: &Parked<char>) -> String {
        p.iter(active).collect()
    }

    #[test]
    fn push_appends_and_activates() {
        let mut p = Parked::default();
        let mut a = 'a';
        assert_eq!((p.len(), p.slot()), (1, 0));
        p.push(&mut a, 'b');
        p.push(&mut a, 'c');
        assert_eq!((a, p.slot(), order(&a, &p)), ('c', 2, "abc".into()));
    }

    #[test]
    fn focus_keeps_the_order() {
        let (mut a, mut p) = ring(&['b', 'c', 'd']);
        assert_eq!(order(&a, &p), "abcd");
        p.focus(&mut a, 2);
        assert_eq!((a, p.slot(), order(&a, &p)), ('c', 2, "abcd".into()));
        p.focus(&mut a, 0);
        assert_eq!((a, p.slot(), order(&a, &p)), ('a', 0, "abcd".into()));
        p.focus(&mut a, 3);
        p.focus(&mut a, 1);
        assert_eq!((a, order(&a, &p)), ('b', "abcd".into()));
        p.focus(&mut a, 9);
        assert_eq!(a, 'b', "out of range is ignored");
    }

    #[test]
    fn close_picks_a_neighbour() {
        let (mut a, mut p) = ring(&['b', 'c', 'd']);
        p.focus(&mut a, 1);
        // Closing the active one moves right.
        assert_eq!(p.close(&mut a, 1), Some('b'));
        assert_eq!((a, p.slot(), order(&a, &p)), ('c', 1, "acd".into()));
        // Closing one before it shifts the slot.
        assert_eq!(p.close(&mut a, 0), Some('a'));
        assert_eq!((a, p.slot(), order(&a, &p)), ('c', 0, "cd".into()));
        // Closing the active one at the end moves left.
        p.focus(&mut a, 1);
        assert_eq!(p.close(&mut a, 1), Some('d'));
        assert_eq!((a, p.slot(), order(&a, &p)), ('c', 0, "c".into()));
        // The last one stays.
        assert_eq!(p.close(&mut a, 0), None);
        assert_eq!(p.close(&mut a, 5), None);
    }

    #[test]
    fn close_after_the_active_one() {
        let (mut a, mut p) = ring(&['b', 'c']);
        assert_eq!(p.close(&mut a, 2), Some('c'));
        assert_eq!((a, p.slot(), order(&a, &p)), ('a', 0, "ab".into()));
    }

    #[test]
    fn step_wraps() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0);
        assert_eq!(step(0, 3, false), 2);
        assert_eq!(step(0, 1, false), 0);
        assert_eq!(step(0, 0, true), 0);
        let (mut a, mut p) = ring(&['b']);
        p.focus(&mut a, p.step(false));
        assert_eq!(a, 'b');
    }

    #[test]
    fn restore_lists_and_clamps() {
        let all = |_: &Path| true;
        let p = |s: &str| PathBuf::from(s);
        let libs = [p("/a"), p("/b"), p("/a"), p("/c")];
        assert_eq!(restore(&libs, Some(3), None, all), (vec![p("/a"), p("/b"), p("/c")], 2));
        assert_eq!(restore(&libs, Some(9), None, all).1, 0);
        // A missing library drops out and the active one is found by path.
        assert_eq!(restore(&libs, Some(3), None, |q: &Path| q != Path::new("/b")), (vec![p("/a"), p("/c")], 1));
        // The active library itself is gone: the first remaining one.
        assert_eq!(restore(&libs, Some(1), None, |q: &Path| q != Path::new("/b")).1, 0);
        // Old saved state: only `library`.
        assert_eq!(restore(&[], None, Some(Path::new("/x")), all), (vec![p("/x")], 0));
        assert_eq!(restore(&[], None, Some(Path::new("/x")), |_: &Path| false), (vec![], 0));
        assert_eq!(restore(&[], None, None, all), (vec![], 0));
    }

    #[test]
    fn recent_minus_open_and_tilde() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(not_open(&[p("/a"), p("/b"), p("/c")], &[p("/b")]), [p("/a"), p("/c")]);
        let home = Some(Path::new("/Users/me"));
        assert_eq!(tilde_in(Path::new("/Users/me/Pictures"), home), "~/Pictures");
        assert_eq!(tilde_in(Path::new("/Users/me"), home), "~");
        assert_eq!(tilde_in(Path::new("/Volumes/X"), home), "/Volumes/X");
        assert_eq!(tilde_in(Path::new("/Users/meme"), home), "/Users/meme");
    }

    #[test]
    fn remember_moves_to_front() {
        let mut r = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        remember(&mut r, Path::new("/b"), 10);
        assert_eq!(r, [PathBuf::from("/b"), PathBuf::from("/a")]);
        remember(&mut r, Path::new("/c"), 2);
        assert_eq!(r, [PathBuf::from("/c"), PathBuf::from("/b")]);
    }
}
