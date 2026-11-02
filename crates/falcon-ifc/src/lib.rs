//! Full IFC geometry through web-ifc (MPL-2.0, <https://github.com/ThatOpen/engine_web-ifc>),
//! the engine Eagle's Online 3D Viewer uses. `scripts/fetch-web-ifc.sh`
//! fetches its source at a pinned tag; `build.rs` compiles it into an
//! executable, `falcon-ifc-worker`, when the source is there, and each read
//! runs in one such process. Without it [`available`] is false and [`read`]
//! returns [`Error::Unavailable`], and falcon falls back to its own
//! tessellated reader.
//!
//! The file is read as Online 3D Viewer reads it: every product except
//! openings and spaces, coordinates moved to the origin, lengths in metres
//! and Y up, one colour per placed geometry.

use std::sync::Mutex;
use std::time::Duration;

mod worker;

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// falcon was built without the web-ifc sources.
    Unavailable,
    /// web-ifc could not open the file.
    Open,
    /// web-ifc failed while reading (its message).
    Failed(String),
    /// The model needs more triangles than [`Limits::max_triangles`].
    TooLarge,
    /// web-ifc did not finish within [`Limits::timeout`].
    TimedOut,
    /// web-ifc went past [`Limits::max_memory`].
    OutOfMemory,
    /// web-ifc crashed; only its worker process ended.
    Crashed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unavailable => write!(f, "web-ifc is not built into falcon"),
            Error::Open => write!(f, "web-ifc could not open the file"),
            Error::Failed(m) => write!(f, "web-ifc failed: {m}"),
            Error::TooLarge => write!(f, "the model has too many triangles"),
            Error::TimedOut => write!(f, "web-ifc did not finish in time"),
            Error::OutOfMemory => write!(f, "web-ifc needed too much memory"),
            Error::Crashed => write!(f, "web-ifc crashed on the file"),
        }
    }
}

impl std::error::Error for Error {}

/// Bounds on one read.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Reading stops with [`Error::TooLarge`] past this many triangles.
    pub max_triangles: usize,
    /// Segments per full circle for curved profiles (web-ifc's default is 12).
    pub circle_segments: u16,
    /// How long web-ifc may run before its worker is ended.
    pub timeout: Duration,
    /// The memory web-ifc's worker may use, in bytes, before it is ended.
    pub max_memory: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_triangles: 20_000_000, circle_segments: 12, timeout: Duration::from_secs(120), max_memory: 16 << 30 }
    }
}

/// One placed geometry: triangles in world coordinates (metres, Y up).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geometry {
    /// The express id (`#n`) of the product it belongs to.
    pub product: u32,
    pub positions: Vec<[f64; 3]>,
    /// Three indices into `positions` per triangle (web-ifc already
    /// reverses the faces of mirrored placements).
    pub indices: Vec<u32>,
    /// RGBA, each 0..1.
    pub color: [f64; 4],
}

/// One product web-ifc looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: u32,
    /// The entity name as the file writes it, e.g. `IFCWALL`.
    pub type_name: String,
    /// Placed geometries with triangles it produced.
    pub geometries: u32,
}

/// Everything read from one file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub products: Vec<Product>,
    pub geometries: Vec<Geometry>,
}

impl Scene {
    pub fn triangle_count(&self) -> usize {
        self.geometries.iter().map(|g| g.indices.len() / 3).sum()
    }
}

/// Whether the web-ifc worker is there to run.
pub fn available() -> bool {
    worker::path().is_some()
}

/// The web-ifc release built in, when it is.
pub const WEB_IFC_VERSION: &str = "0.0.78";

/// One file is read at a time, so a batch import doesn't hold several
/// models' memory at once.
static ENGINE: Mutex<()> = Mutex::new(());

/// Reads `bytes` (IFC STEP text) into placed geometry.
///
/// web-ifc runs in a worker process, so a crash, hang or runaway allocation
/// on a malformed file comes back as an [`Error`].
pub fn read(bytes: &[u8], limits: Limits) -> Result<Scene, Error> {
    // Waiting for another read happens here, before the timeout starts.
    let _guard = ENGINE.lock().unwrap_or_else(|p| p.into_inner());
    worker::read(bytes, limits)
}
