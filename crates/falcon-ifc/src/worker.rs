//! Talks to `falcon-ifc-worker`, the executable with web-ifc in it
//! (`cpp/worker.cpp` describes the protocol).
//!
//! web-ifc is C++ and malformed files can make it overflow its stack, spin
//! forever or read out of bounds, none of which a thread could survive, so
//! it never runs in falcon's own process. A crash ends only the worker, the
//! parent kills one that outlives the timeout, and the worker ends itself
//! past its timeout or memory limit, so it can't outlive a parent that has
//! gone.

use std::io::{BufReader, ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use super::{Error, Geometry, Limits, Product, Scene};

/// How much longer than the worker's own limit the parent waits.
const GRACE: Duration = Duration::from_secs(2);

const EXIT_TIMEOUT: i32 = 71;
const EXIT_MEMORY: i32 = 72;

/// The worker beside the running executable (an app bundle's) or where this
/// build made it.
pub(super) fn path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        let beside = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("falcon-ifc-worker")));
        let built = option_env!("FALCON_IFC_WORKER").map(PathBuf::from);
        [beside, built].into_iter().flatten().find(|p| p.is_file())
    })
    .as_ref()
}

fn request(limits: Limits) -> Vec<u8> {
    let mut out = Vec::with_capacity(18);
    out.extend_from_slice(&limits.circle_segments.to_le_bytes());
    out.extend_from_slice(&(limits.timeout.as_millis() as u64).to_le_bytes());
    out.extend_from_slice(&limits.max_memory.to_le_bytes());
    out
}

/// Reads `bytes` in a worker process.
pub(super) fn read(bytes: &[u8], limits: Limits) -> Result<Scene, Error> {
    let exe = path().ok_or(Error::Unavailable)?;
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Failed(e.to_string()))?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::Failed("the worker has no pipes".into()));
    };
    std::thread::scope(|s| {
        // A worker that dies early ends this with a broken pipe, which its
        // exit status explains.
        s.spawn(move || {
            let _ = stdin.write_all(&request(limits)).and_then(|()| stdin.write_all(bytes));
        });
        let (tx, rx) = std::sync::mpsc::channel();
        s.spawn(move || {
            let mut stream = BufReader::with_capacity(1 << 20, stdout);
            let _ = tx.send(read_stream(&mut stream, limits));
        });
        let reply = rx.recv_timeout(limits.timeout + GRACE);
        // A worker that hasn't ended by now is stuck or was told to stop.
        if matches!(reply, Err(_) | Ok(Ok(Err(Error::TooLarge)))) {
            let _ = child.kill();
        }
        let status = child.wait().map_err(|e| Error::Failed(e.to_string()))?;
        match reply {
            Err(_) => Err(Error::TimedOut),
            Ok(Ok(result)) => result,
            // The stream was cut short: the worker died.
            Ok(Err(_)) => Err(match status.code() {
                Some(EXIT_TIMEOUT) => Error::TimedOut,
                Some(EXIT_MEMORY) => Error::OutOfMemory,
                _ => Error::Crashed,
            }),
        }
    })
}

fn bad_stream() -> std::io::Error {
    std::io::Error::new(ErrorKind::InvalidData, "bad stream from the web-ifc worker")
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

/// Reads `count` items of `N` bytes each. The buffer grows as bytes arrive,
/// so a wrong count can't reserve more than the stream really holds.
fn read_items<const N: usize, T>(r: &mut impl Read, count: u32, item: impl Fn([u8; N]) -> T) -> std::io::Result<Vec<T>> {
    let wanted = count as u64 * N as u64;
    let mut bytes = Vec::new();
    if r.take(wanted).read_to_end(&mut bytes)? as u64 != wanted {
        return Err(bad_stream());
    }
    Ok(bytes.as_chunks::<N>().0.iter().map(|c| item(*c)).collect())
}

/// `Err` when the stream ends early or is malformed: the worker died. The
/// worker's own answer is the `Ok`.
fn read_stream(r: &mut impl Read, limits: Limits) -> std::io::Result<Result<Scene, Error>> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"FIFC" {
        return Err(bad_stream());
    }
    let mut scene = Scene::default();
    let mut triangles = 0usize;
    loop {
        let mut tag = [0u8; 1];
        r.read_exact(&mut tag)?;
        match tag[0] {
            b'G' => {
                let product = read_u32(r)?;
                let color = read_items(r, 4, f64::from_le_bytes)?;
                let matrix = read_items(r, 16, f64::from_le_bytes)?;
                let vertex_floats = read_u32(r)?;
                let index_count = read_u32(r)?;
                let vertices = read_items(r, vertex_floats, f32::from_le_bytes)?;
                let indices = read_items(r, index_count, u32::from_le_bytes)?;
                if triangles + indices.len() / 3 > limits.max_triangles {
                    return Ok(Err(Error::TooLarge));
                }
                if let Some(g) = place(product, &color, &matrix, &vertices, &indices) {
                    triangles += g.indices.len() / 3;
                    scene.geometries.push(g);
                }
            }
            b'P' => {
                let id = read_u32(r)?;
                let name_len = read_u32(r)?;
                let name = read_items(r, name_len, u8::from_le_bytes)?;
                // Count what was kept (triangles that survived the checks in `place`).
                let geometries = scene.geometries.iter().rev().take_while(|g| g.product == id).count() as u32;
                scene.products.push(Product { id, type_name: String::from_utf8_lossy(&name).into_owned(), geometries });
            }
            b'E' => return Ok(Ok(scene)),
            b'O' => return Ok(Err(Error::Open)),
            b'F' => {
                let len = read_u32(r)?;
                let message = read_items(r, len, u8::from_le_bytes)?;
                return Ok(Err(Error::Failed(String::from_utf8_lossy(&message).into_owned())));
            }
            _ => return Err(bad_stream()),
        }
    }
}

/// One placed geometry in world coordinates, keeping the triangles whose
/// indices are in range and whose corners are finite; `None` when none is.
fn place(product: u32, color: &[f64], m: &[f64], vertices: &[f32], indices: &[u32]) -> Option<Geometry> {
    let positions: Vec<[f64; 3]> = vertices
        .as_chunks::<6>()
        .0
        .iter()
        .map(|v| {
            let p = [v[0] as f64, v[1] as f64, v[2] as f64];
            [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
        })
        .collect();
    let mut kept = Vec::with_capacity(indices.len());
    for t in indices.as_chunks::<3>().0 {
        let good = |i: &u32| positions.get(*i as usize).is_some_and(|p| p.iter().all(|c| c.is_finite()));
        if t.iter().all(good) {
            kept.extend_from_slice(t);
        }
    }
    if kept.is_empty() {
        return None;
    }
    let c = |k: usize| if color[k].is_finite() { color[k].clamp(0.0, 1.0) } else { 1.0 };
    Some(Geometry { product, positions, indices: kept, color: [c(0), c(1), c(2), c(3)] })
}
