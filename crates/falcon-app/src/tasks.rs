//! The background task status beside the library name, after Eagle's
//! sidebar spinner: while files are still being imported (each one copied,
//! thumbnailed and colour-analysed on the import thread; falcon has no
//! separate palette queue), a spinner shows how many are left, and a click
//! pauses the queue between files or resumes it.

/// What the status shows, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Files not imported yet.
    pub left: usize,
    pub paused: bool,
}

impl Status {
    /// The tooltip: the state and the number of files left.
    pub fn label(&self) -> String {
        let what = if self.paused { "Paused" } else { "Processing" };
        format!("{what} ({})", self.left)
    }

    /// The hint for a click.
    pub fn action(&self) -> &'static str {
        if self.paused { "Click to resume" } else { "Click to pause" }
    }
}

/// The status for an import batch of `total` files with `done` finished;
/// none once every file is done.
pub fn status(total: usize, done: usize, paused: bool) -> Option<Status> {
    (done < total).then(|| Status { left: total - done, paused })
}

/// How long the import thread sleeps between checks while paused.
pub const PAUSE_POLL: std::time::Duration = std::time::Duration::from_millis(100);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_while_files_are_left() {
        assert_eq!(status(0, 0, false), None);
        assert_eq!(status(5, 5, true), None);
        let s = status(5, 2, false).unwrap();
        assert_eq!((s.left, s.label().as_str(), s.action()), (3, "Processing (3)", "Click to pause"));
        let p = status(5, 2, true).unwrap();
        assert_eq!((p.label().as_str(), p.action()), ("Paused (3)", "Click to resume"));
    }
}
