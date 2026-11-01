//! File-system events for one folder through FSEvents (CoreServices), for
//! the auto-import watcher (`auto_import.rs`). The stream only says that
//! something under the folder changed; the watcher then lists the folder
//! itself. Events arrive on a dispatch queue of the stream's own and are
//! passed on as `()` over a channel. Dropping the [`Stream`] stops it.

use std::path::Path;
use std::sync::mpsc::Receiver;

/// A running event stream. Dropping it stops and releases the stream.
pub struct Stream {
    /// Kept only to be dropped, which stops the stream.
    _imp: imp::Handle,
}

/// Starts a stream on `dir` (and everything below it): a `()` arrives for
/// each batch of changes, at most every `latency` seconds. `None` when the
/// stream can't be made or started; the caller then lists the folder on a
/// timer instead.
pub fn watch(dir: &Path, latency: f64) -> Option<(Stream, Receiver<()>)> {
    imp::watch(dir, latency).map(|(imp, rx)| (Stream { _imp: imp }, rx))
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CString, c_char, c_void};
    use std::path::Path;
    use std::sync::mpsc::{Receiver, Sender, channel};

    type CFTypeRef = *const c_void;
    type StreamRef = *mut c_void;
    type Callback = extern "C" fn(stream: StreamRef, info: *mut c_void, count: usize, paths: *mut c_void, flags: *const u32, ids: *const u64);

    #[repr(C)]
    struct Context {
        version: isize,
        info: *mut c_void,
        retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
        release: Option<extern "C" fn(*const c_void)>,
        copy_description: Option<extern "C" fn(*const c_void) -> CFTypeRef>,
    }

    /// `kFSEventStreamEventIdSinceNow`: only changes from now on.
    const SINCE_NOW: u64 = u64::MAX;
    /// `kFSEventStreamCreateFlagNoDefer | kFSEventStreamCreateFlagFileEvents`:
    /// the first change of a quiet spell is sent at once, and changes to
    /// files count, not only to folders.
    const FLAGS: u32 = 0x02 | 0x10;
    /// `kCFStringEncodingUTF8`.
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn FSEventStreamCreate(alloc: CFTypeRef, callback: Callback, context: *const Context, paths: CFTypeRef, since: u64, latency: f64, flags: u32) -> StreamRef;
        fn FSEventStreamSetDispatchQueue(stream: StreamRef, queue: *mut c_void);
        fn FSEventStreamStart(stream: StreamRef) -> u8;
        fn FSEventStreamStop(stream: StreamRef);
        fn FSEventStreamInvalidate(stream: StreamRef);
        fn FSEventStreamRelease(stream: StreamRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFTypeRef;
        fn CFArrayCreate(alloc: CFTypeRef, values: *const CFTypeRef, count: isize, callbacks: *const c_void) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
        static kCFTypeArrayCallBacks: [usize; 5];
    }

    // libdispatch is part of libSystem, always linked.
    unsafe extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
        fn dispatch_release(object: *mut c_void);
    }

    pub struct Handle {
        stream: StreamRef,
        queue: *mut c_void,
    }

    // SAFETY: the stream and queue are only touched again to stop and
    // release them, which FSEvents allows from any thread.
    unsafe impl Send for Handle {}

    extern "C" fn on_events(_: StreamRef, info: *mut c_void, _: usize, _: *mut c_void, _: *const u32, _: *const u64) {
        // SAFETY: `info` is the boxed sender, alive until the stream's
        // release callback frees it, after the last event.
        let tx = unsafe { &*(info as *const Sender<()>) };
        let _ = tx.send(());
    }

    extern "C" fn release_sender(info: *const c_void) {
        // SAFETY: made by `Box::into_raw` in `watch`; FSEvents calls this
        // once, when the stream is released.
        drop(unsafe { Box::from_raw(info as *mut Sender<()>) });
    }

    pub fn watch(dir: &Path, latency: f64) -> Option<(Handle, Receiver<()>)> {
        let path = CString::new(dir.to_str()?).ok()?;
        let label = CString::new("falcon.auto-import.events").ok()?;
        let (tx, rx) = channel();
        let info = Box::into_raw(Box::new(tx)) as *mut c_void;
        let context = Context { version: 0, info, retain: None, release: Some(release_sender), copy_description: None };
        // SAFETY: plain CoreFoundation and FSEvents calls with valid
        // arguments; every object made here is released on each path out.
        unsafe {
            let cf_path = CFStringCreateWithCString(std::ptr::null(), path.as_ptr(), UTF8);
            if cf_path.is_null() {
                release_sender(info);
                return None;
            }
            let paths = CFArrayCreate(std::ptr::null(), &cf_path, 1, kCFTypeArrayCallBacks.as_ptr() as *const c_void);
            CFRelease(cf_path);
            if paths.is_null() {
                release_sender(info);
                return None;
            }
            let stream = FSEventStreamCreate(std::ptr::null(), on_events, &context, paths, SINCE_NOW, latency, FLAGS);
            CFRelease(paths);
            if stream.is_null() {
                release_sender(info);
                return None;
            }
            // From here the stream owns `info` and frees it on release.
            let queue = dispatch_queue_create(label.as_ptr(), std::ptr::null());
            if queue.is_null() {
                FSEventStreamRelease(stream);
                return None;
            }
            FSEventStreamSetDispatchQueue(stream, queue);
            let handle = Handle { stream, queue };
            if FSEventStreamStart(stream) == 0 {
                return None;
            }
            Some((handle, rx))
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the stream was created, given its queue and maybe
            // started; stopping a stream that never started is allowed.
            unsafe {
                FSEventStreamStop(self.stream);
                FSEventStreamInvalidate(self.stream);
                FSEventStreamRelease(self.stream);
                dispatch_release(self.queue);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::Path;
    use std::sync::mpsc::Receiver;

    pub struct Handle;

    pub fn watch(_: &Path, _: f64) -> Option<(Handle, Receiver<()>)> {
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// A real stream, on a temporary folder only. Ignored by default so the
    /// test run never starts FSEvents; run it by hand with `--ignored`.
    #[test]
    #[ignore]
    fn a_real_stream_reports_a_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (stream, rx) = super::watch(tmp.path(), 0.1).expect("stream starts");
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(tmp.path().join("a.png"), b"x").unwrap();
        assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
        drop(stream);
        // The sender is freed with the stream: the channel closes.
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(2)) {
                Ok(()) => continue,
                Err(e) => break assert_eq!(e, std::sync::mpsc::RecvTimeoutError::Disconnected),
            }
        }
    }
}
