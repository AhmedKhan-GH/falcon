//! Libraries waiting to be opened behind the loading screen, one after
//! another: the ones restored at launch, Try Again and Select Another
//! Path on the missing-library screen, and a library just created. Each
//! is read on a worker by `App::open_in_background`
//! (`Library::open_or_restore_with_progress`), so the window keeps
//! drawing while it loads. What was asked on the command line, and links
//! and files opened with falcon meanwhile, wait until the queue is done.

use std::collections::VecDeque;
use std::path::PathBuf;

use super::{App, Launch};
use crate::locale::tr;

/// Something to do once the queue has drained.
#[derive(Debug, PartialEq)]
pub(crate) enum Then {
    /// Show this library (the one shown before quitting, or the one given
    /// on the command line).
    Show(PathBuf),
    /// The missing library at `old` was found at `new`: if it opened, the
    /// old place leaves the recent list; if not, say it's still missing.
    Replace { old: PathBuf, new: PathBuf },
}

#[derive(Default)]
pub(crate) struct OpenQueue {
    paths: VecDeque<PathBuf>,
    then: Vec<Then>,
    /// The command line's starting view, applied once everything is open.
    launch: Option<Launch>,
}

impl OpenQueue {
    pub(crate) fn push(&mut self, path: PathBuf) {
        if !self.paths.contains(&path) {
            self.paths.push_back(path);
        }
    }

    pub(crate) fn then(&mut self, then: Then) {
        self.then.push(then);
    }

    /// Libraries still to open, or follow-ups not yet run.
    pub(crate) fn busy(&self) -> bool {
        !self.paths.is_empty() || !self.then.is_empty() || self.launch.is_some()
    }

    /// The next library to start reading, once the last one is read.
    fn next(&mut self, loading: bool) -> Option<PathBuf> {
        if loading { None } else { self.paths.pop_front() }
    }

    /// The follow-ups, once every library has been read.
    fn drained(&mut self, loading: bool) -> Option<(Vec<Then>, Option<Launch>)> {
        (!loading && self.paths.is_empty() && (!self.then.is_empty() || self.launch.is_some())).then(|| (std::mem::take(&mut self.then), self.launch.take()))
    }
}

impl App {
    /// Launch: reopens `libraries` in order behind the loading screen,
    /// then shows `show` and applies `launch`.
    pub(crate) fn queue_launch(&mut self, libraries: Vec<PathBuf>, show: Option<PathBuf>, launch: Launch) {
        for p in libraries {
            self.opens.push(p);
        }
        if let Some(p) = show {
            self.opens.push(p.clone());
            self.opens.then(Then::Show(p));
        }
        self.opens.launch = Some(launch);
        self.pump_opens();
    }

    /// Opens `path` behind the loading screen, after any already queued.
    pub(crate) fn queue_open(&mut self, path: PathBuf) {
        self.opens.push(path);
        self.pump_opens();
    }

    /// The missing-library screen's Try Again: a folder that is still not
    /// there is reported at once, else it is read behind the loading screen.
    pub(crate) fn retry_missing(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.flash(tr("The library is still missing"));
            return;
        }
        self.queue_open(path);
    }

    /// Select Another Path…: reads `new` in place of the missing `old`.
    pub(crate) fn replace_missing(&mut self, old: PathBuf, new: PathBuf) {
        self.opens.then(Then::Replace { old, new: new.clone() });
        self.queue_open(new);
    }

    /// Starts the next queued read when none is running, and runs the
    /// follow-ups once all are read. Called every frame.
    pub(crate) fn pump_opens(&mut self) {
        while let Some(p) = self.opens.next(self.loading.is_some()) {
            // An open library is just shown, without a load.
            self.open_in_background(p);
        }
        let Some((then, launch)) = self.opens.drained(self.loading.is_some()) else { return };
        for t in then {
            match t {
                Then::Show(p) => {
                    if let Some(i) = self.position_of(&p) {
                        self.focus(i);
                    }
                }
                Then::Replace { old, new } => {
                    if self.position_of(&new).is_some() {
                        crate::workspaces::remove_from_list(&mut self.recent, &old);
                    }
                }
            }
        }
        if let Some(launch) = launch {
            self.apply_launch(launch);
        }
    }

    /// The command line's starting view, on the library shown.
    fn apply_launch(&mut self, launch: Launch) {
        if !launch.import.is_empty() {
            self.import(launch.import);
        }
        // One item opens in the viewer (unless --select); several are selected.
        if let [id] = launch.items.as_slice()
            && !launch.select
        {
            self.ws.viewer = Some(id.clone());
        }
        if let Some(first) = launch.items.first() {
            self.ws.scroll_to = Some(first.clone());
        }
        if !launch.items.is_empty() {
            self.ws.selected = launch.items;
        }
        if let Some(q) = launch.search {
            self.ws.set_query(q);
        }
        if let Some(rgb) = launch.color.as_deref().and_then(crate::ui::filter_bar::parse_hex) {
            self.ws.filter.color = Some(falcon_library::search::ColorQuery::new(rgb));
            self.ws.color_text = crate::logic::hex(rgb);
            self.ws.show_filter = true;
        }
        if launch.tags {
            self.ws.scope = crate::logic::Scope::AllTags;
        }
        for link in launch.links {
            self.follow_link(link);
        }
    }

    /// Whether links and files opened with falcon must wait: libraries are
    /// still being read for the launch.
    pub(crate) fn opens_pending(&self) -> bool {
        self.opens.busy() || self.loading.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_read_at_a_time_in_order() {
        let mut q = OpenQueue::default();
        q.push("/a".into());
        q.push("/b".into());
        q.push("/a".into());
        assert_eq!(q.paths.len(), 2);
        // A read is running: nothing new starts.
        assert_eq!(q.next(true), None);
        assert_eq!(q.next(false), Some(PathBuf::from("/a")));
        assert_eq!(q.next(false), Some(PathBuf::from("/b")));
        assert_eq!(q.next(false), None);
    }

    #[test]
    fn follow_ups_wait_until_everything_is_read() {
        let mut q = OpenQueue::default();
        q.push("/a".into());
        q.then(Then::Show("/a".into()));
        q.launch = Some(Launch { tags: true, ..Launch::default() });
        assert!(q.busy());
        assert!(q.drained(false).is_none(), "a library is still queued");
        assert_eq!(q.next(false), Some(PathBuf::from("/a")));
        assert!(q.drained(true).is_none(), "it is still being read");
        let (then, launch) = q.drained(false).unwrap();
        assert_eq!(then, [Then::Show("/a".into())]);
        assert!(launch.is_some_and(|l| l.tags));
        assert!(!q.busy());
        assert!(q.drained(false).is_none(), "the follow-ups run once");
    }
}
