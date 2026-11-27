//! Internal thread-local fault-injection seam for crash-consistency and
//! external-SSD I/O failure tests. Zero overhead in production.

use std::cell::RefCell;
use std::io;
use std::path::Path;

use crate::error::{Error, Result};

/// Persistence boundary hooks checked across multi-file library mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    ImportAfterStageMetadata,
    ImportBeforeLinkedIds,
    ImportAfterLinkedIds,
    ImportBeforePublishRename,
    ImportAfterPublishRename,
    DuplicateAfterStageMetadata,
    DuplicateBeforeLinkedIds,
    DuplicateAfterLinkedIds,
    DuplicateBeforePublishRename,
    DuplicateAfterPublishRename,
    MergeCopyItem,
    MergeBeforeLinkedIdsBatch,
    MergeAfterLinkedIdsBatch,
    MergeBeforeMetadataSave,
    MergeBeforeMtimeWrite,
    MetadataWrite,
    MtimeWrite,
    FlushDirtyItem,
    JournalWrite,
}

type Hook = Box<dyn FnMut(FaultPoint) -> io::Result<()>>;

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

/// Installs a thread-local fault-injection hook for the lifetime of the guard.
pub struct FaultGuard {
    _private: (),
}

impl Drop for FaultGuard {
    fn drop(&mut self) {
        HOOK.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

/// Registers `hook` on the current thread until the returned guard is dropped.
pub fn install(hook: impl FnMut(FaultPoint) -> io::Result<()> + 'static) -> FaultGuard {
    HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    FaultGuard { _private: () }
}

/// Checks whether an active fault hook fails at `point`.
pub(crate) fn check(point: FaultPoint, path: &Path) -> Result<()> {
    HOOK.with(|slot| {
        let mut guard = slot.borrow_mut();
        if let Some(f) = guard.as_mut() {
            f(point).map_err(|e| Error::io(path, e))
        } else {
            Ok(())
        }
    })
}
