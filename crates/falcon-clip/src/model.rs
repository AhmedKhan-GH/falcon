//! The model AI Search uses: which files, from where, and how they get here.
//!
//! Everything is pinned: a commit of the model's repository and the SHA-256
//! of each file. Only safetensors weights are read, never pickle. Files are
//! checked when downloaded and again each time they are loaded.
//!
//! The one network call in AI Search is [`Download`]. It sends a plain `GET`
//! with no identifiers, and only runs when the user starts it.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// How Preferences names the model.
pub const NAME: &str = "CLIP ViT-B/32 (LAION-2B)";
pub const REPOSITORY: &str = "laion/CLIP-ViT-B-32-laion2B-s34B-b79K";
/// The commit downloaded.
pub const REVISION: &str = "1a25a446712ba5ee05982a381eed697ef9b435cf";
/// The licence the model's card declares.
pub const LICENCE: &str = "MIT";
/// Says which model an index's vectors came from: an index made by anything
/// else is thrown away.
pub const ID: &str = "laion-clip-vit-b-32@1a25a446";

pub const WEIGHTS: &str = "model.safetensors";
pub const TOKENIZER: &str = "tokenizer.json";

pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub const FILES: [ModelFile; 2] = [
    ModelFile { name: TOKENIZER, size: 2_224_041, sha256: "b556ac8c99757ffb677208af34bc8c6721572114111a6e0aaf5fa69ff0b8d842" },
    ModelFile { name: WEIGHTS, size: 605_157_884, sha256: "74813fbcdc750f235c9784c367ca1394d2a5c25eb0aac92761752ac239db7cff" },
];

/// The model's page, shown in Preferences.
pub fn page() -> String {
    format!("https://huggingface.co/{REPOSITORY}")
}

/// What the download fetches, in bytes.
pub fn download_size() -> u64 {
    FILES.iter().map(|f| f.size).sum()
}

fn url(file: &ModelFile) -> String {
    format!("https://huggingface.co/{REPOSITORY}/resolve/{REVISION}/{}", file.name)
}

/// Where this revision goes under `base`. A new revision gets a new folder.
pub fn dir_in(base: &Path) -> PathBuf {
    base.join(REVISION)
}

/// Whether every file is there at the right size. The cheap check, for
/// showing state; loading checks the hashes.
pub fn is_installed(dir: &Path) -> bool {
    FILES.iter().all(|f| fs::metadata(dir.join(f.name)).is_ok_and(|m| m.is_file() && m.len() == f.size))
}

fn sha256_hex(mut reader: impl Read, cancel: Option<&AtomicBool>) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn check(path: &Path, file: &ModelFile, cancel: Option<&AtomicBool>) -> Result<(), String> {
    let got = File::open(path).and_then(|f| sha256_hex(f, cancel)).map_err(|err| format!("{}: {err}", file.name))?;
    if got == file.sha256 { Ok(()) } else { Err(format!("{} failed its SHA-256 check", file.name)) }
}

/// Checks every file against its pinned SHA-256.
pub fn verify(dir: &Path) -> Result<(), String> {
    FILES.iter().try_for_each(|f| check(&dir.join(f.name), f, None))
}

/// Removes everything [`Download`] put under `base`.
pub fn remove(base: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(base) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// How a download stands.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum DownloadState {
    #[default]
    Idle,
    Running { done: u64, total: u64 },
    Checking,
    Finished,
    Cancelled,
    Failed(String),
}

/// One download at a time, on its own thread. A partial file is continued
/// next time.
#[derive(Default)]
pub struct Download {
    state: Arc<Mutex<DownloadState>>,
    cancel: Arc<AtomicBool>,
}

impl Download {
    pub fn state(&self) -> DownloadState {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state(), DownloadState::Running { .. } | DownloadState::Checking)
    }

    /// Clears a finished, cancelled or failed state.
    pub fn reset(&self) {
        if !self.is_running() {
            *self.state.lock().unwrap_or_else(|p| p.into_inner()) = DownloadState::Idle;
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Starts fetching the model into `dir`; `notify` runs whenever the
    /// state changes.
    pub fn start(&self, dir: PathBuf, notify: impl Fn() + Send + 'static) {
        if self.is_running() {
            return;
        }
        self.cancel.store(false, Ordering::Relaxed);
        let set = {
            let state = self.state.clone();
            move |s: DownloadState| *state.lock().unwrap_or_else(|p| p.into_inner()) = s
        };
        set(DownloadState::Running { done: 0, total: download_size() });
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            let sources: Vec<(String, &ModelFile)> = FILES.iter().map(|f| (url(f), f)).collect();
            let result = fetch_all(&sources, &dir, &cancel, &|s| {
                set(s);
                notify();
            });
            set(match result {
                Ok(()) => DownloadState::Finished,
                Err(_) if cancel.load(Ordering::Relaxed) => DownloadState::Cancelled,
                Err(err) => DownloadState::Failed(err),
            });
            notify();
        });
    }
}

/// Fetches each file not already in place, then checks it.
fn fetch_all(sources: &[(String, &ModelFile)], dir: &Path, cancel: &AtomicBool, report: &dyn Fn(DownloadState)) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let total: u64 = sources.iter().map(|(_, f)| f.size).sum();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent("falcon")
        .build()
        .into();
    let mut before = 0;
    for (url, file) in sources {
        let path = dir.join(file.name);
        if !fs::metadata(&path).is_ok_and(|m| m.len() == file.size) || check(&path, file, Some(cancel)).is_err() {
            let part = dir.join(format!("{}.part", file.name));
            fetch_one(&agent, url, file, &part, cancel, &|done| report(DownloadState::Running { done: before + done, total }))?;
            report(DownloadState::Checking);
            if let Err(err) = check(&part, file, Some(cancel)) {
                if !cancel.load(Ordering::Relaxed) {
                    let _ = fs::remove_file(&part);
                }
                return Err(err);
            }
            fs::rename(&part, &path).map_err(|err| err.to_string())?;
        }
        before += file.size;
        report(DownloadState::Running { done: before, total });
    }
    Ok(())
}

/// Writes one file to `part`, continuing what is there when the server
/// allows it.
fn fetch_one(agent: &ureq::Agent, url: &str, file: &ModelFile, part: &Path, cancel: &AtomicBool, report: &dyn Fn(u64)) -> Result<(), String> {
    let mut have = fs::metadata(part).map_or(0, |m| m.len());
    if have == file.size {
        return Ok(());
    }
    if have > file.size {
        fs::remove_file(part).map_err(|err| err.to_string())?;
        have = 0;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".to_string());
    }
    let mut request = agent.get(url).header("Accept-Encoding", "identity");
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let response = request.call().map_err(|err| err.to_string())?;
    let mut done = if response.status().as_u16() == 206 { have } else { 0 };
    let mut out = OpenOptions::new().create(true).write(true).truncate(false).open(part).map_err(|err| err.to_string())?;
    out.set_len(done).and_then(|()| out.seek(SeekFrom::Start(done))).map_err(|err| err.to_string())?;
    let mut body = response.into_body().into_reader();
    let mut buffer = vec![0; 1 << 18];
    let mut last = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let n = body.read(&mut buffer).map_err(|err| err.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buffer[..n]).map_err(|err| err.to_string())?;
        done += n as u64;
        if done > file.size {
            return Err(format!("{} is larger than expected", file.name));
        }
        if last.elapsed() > Duration::from_millis(200) {
            last = std::time::Instant::now();
            report(done);
        }
    }
    out.flush().map_err(|err| err.to_string())?;
    report(done);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    /// A one-file server that honours `Range`, on a local port, standing in
    /// for the model's host. Returns the address, and the requests' headers.
    fn serve(body: Vec<u8>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}/file", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                head.push_str(&line);
            }
            let from = head.lines().find_map(|l| l.to_lowercase().strip_prefix("range: bytes=").and_then(|r| r.trim().trim_end_matches('-').parse::<usize>().ok()));
            log.lock().unwrap().push(head);
            let (status, part) = match from {
                Some(n) => ("206 Partial Content", &body[n..]),
                None => ("200 OK", &body[..]),
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", part.len()).unwrap();
            stream.write_all(part).unwrap();
        });
        (addr, seen)
    }

    fn file_for(name: &'static str, body: &[u8]) -> &'static ModelFile {
        let sha = sha256_hex(body, None).unwrap();
        Box::leak(Box::new(ModelFile { name, size: body.len() as u64, sha256: Box::leak(sha.into_boxed_str()) }))
    }

    #[test]
    fn a_download_is_checked_and_put_in_place_without_identifiers() {
        let body = b"pretend weights".to_vec();
        let (url, seen) = serve(body.clone());
        let file = file_for("w.safetensors", &body);
        let dir = tempfile::tempdir().unwrap();
        fetch_all(&[(url, file)], dir.path(), &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!(fs::read(dir.path().join("w.safetensors")).unwrap(), body);
        let head = seen.lock().unwrap()[0].to_lowercase();
        assert!(!head.contains('@') && !head.contains("cookie") && !head.contains("authorization"), "{head}");
        assert!(!dir.path().join("w.safetensors.part").exists());
    }

    #[test]
    fn a_partial_file_is_continued() {
        let body = b"0123456789".to_vec();
        let (url, seen) = serve(body.clone());
        let file = file_for("c.safetensors", &body);
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("c.safetensors.part"), b"0123").unwrap();
        fetch_all(&[(url, file)], dir.path(), &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!(fs::read(dir.path().join("c.safetensors")).unwrap(), body);
        assert!(seen.lock().unwrap()[0].to_lowercase().contains("range: bytes=4-"));
    }

    #[test]
    fn a_file_with_the_wrong_hash_is_thrown_away() {
        let (url, _seen) = serve(b"tampered!!".to_vec());
        let file = file_for("t.safetensors", b"0123456789");
        let dir = tempfile::tempdir().unwrap();
        let err = fetch_all(&[(url, file)], dir.path(), &AtomicBool::new(false), &|_| {}).unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert!(!dir.path().join("t.safetensors").exists());
        assert!(!dir.path().join("t.safetensors.part").exists());
    }

    #[test]
    fn cancelling_stops_the_download_and_keeps_nothing_final() {
        let file = file_for("x.safetensors", &[7; 1024]);
        let dir = tempfile::tempdir().unwrap();
        let sources = [("http://127.0.0.1:9/never".to_string(), file)];
        assert!(fetch_all(&sources, dir.path(), &AtomicBool::new(true), &|_| {}).is_err());
        assert!(!dir.path().join("x.safetensors").exists());
    }

    #[test]
    fn verify_and_is_installed_look_at_the_pinned_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_installed(dir.path()));
        assert!(verify(dir.path()).is_err());
        fs::write(dir.path().join(TOKENIZER), b"short").unwrap();
        assert!(!is_installed(dir.path()), "the size is wrong");
    }

    #[test]
    fn the_pin_is_exact_and_only_safetensors_are_fetched() {
        assert_eq!(REVISION.len(), 40);
        for f in &FILES {
            assert_eq!(f.sha256.len(), 64);
            assert!(url(f).contains(REVISION));
            assert!(!f.name.ends_with(".bin") && !f.name.ends_with(".pt"), "never pickle");
        }
    }

    #[test]
    fn remove_takes_the_model_away_and_a_missing_one_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("model");
        fs::create_dir_all(dir_in(&base)).unwrap();
        fs::write(dir_in(&base).join(WEIGHTS), b"x").unwrap();
        remove(&base).unwrap();
        assert!(!base.exists());
        remove(&base).unwrap();
    }
}
