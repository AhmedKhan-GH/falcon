//! Full IFC geometry through web-ifc (MPL-2.0, <https://github.com/ThatOpen/engine_web-ifc>),
//! the engine Eagle's Online 3D Viewer uses. `scripts/fetch-web-ifc.sh`
//! fetches its source at a pinned tag; `build.rs` compiles it with falcon's C
//! interface (`cpp/shim.cpp`) when the source is there. Without it
//! [`available`] is false and [`read`] returns [`Error::Unavailable`], and
//! falcon falls back to its own tessellated reader.
//!
//! The file is read as Online 3D Viewer reads it: every product except
//! openings and spaces, coordinates moved to the origin, lengths in metres
//! and Y up, one colour per placed geometry.

use std::sync::Mutex;

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
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unavailable => write!(f, "web-ifc is not built into falcon"),
            Error::Open => write!(f, "web-ifc could not open the file"),
            Error::Failed(m) => write!(f, "web-ifc failed: {m}"),
            Error::TooLarge => write!(f, "the model has too many triangles"),
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
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_triangles: 20_000_000, circle_segments: 12 }
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

/// Whether falcon was built with web-ifc.
pub fn available() -> bool {
    cfg!(web_ifc)
}

/// The web-ifc release built in, when it is.
pub const WEB_IFC_VERSION: &str = "0.0.78";

/// web-ifc keeps global state (tolerances, counters), so one file is read
/// at a time.
static ENGINE: Mutex<()> = Mutex::new(());

/// The stack web-ifc runs on: its recursion over deep instance graphs
/// needs more than a default thread's.
const STACK: usize = 256 << 20;

/// Reads `bytes` (IFC STEP text) into placed geometry.
pub fn read(bytes: &[u8], limits: Limits) -> Result<Scene, Error> {
    if !available() {
        return Err(Error::Unavailable);
    }
    let _guard = ENGINE.lock().unwrap_or_else(|p| p.into_inner());
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .name("falcon-web-ifc".into())
            .stack_size(STACK)
            .spawn_scoped(s, || native::read(bytes, limits))
            .map_err(|e| Error::Failed(e.to_string()))?
            .join()
            .unwrap_or_else(|_| Err(Error::Failed("the reader stopped".into())))
    })
}

#[cfg(web_ifc)]
mod native {
    use std::ffi::{CStr, c_char, c_int, c_void};

    use super::{Error, Geometry, Limits, Product, Scene};

    type GeometryFn =
        unsafe extern "C" fn(*mut c_void, u32, *const f32, u32, *const u32, u32, *const f64, *const f64) -> c_int;
    type ProductFn = unsafe extern "C" fn(*mut c_void, u32, *const c_char, u32);

    unsafe extern "C" {
        fn falcon_ifc_read(
            data: *const u8,
            len: usize,
            circle_segments: u16,
            ctx: *mut c_void,
            on_product: ProductFn,
            on_geometry: GeometryFn,
            err: *mut c_char,
            err_len: usize,
        ) -> c_int;
    }

    struct Sink {
        scene: Scene,
        triangles: usize,
        limits: Limits,
        too_large: bool,
    }

    /// Applies the column-major 4×4 `m` to `p`.
    fn place(m: &[f64; 16], p: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
    }

    unsafe extern "C" fn on_geometry(
        ctx: *mut c_void,
        product: u32,
        vertices: *const f32,
        vertex_floats: u32,
        indices: *const u32,
        index_count: u32,
        color: *const f64,
        matrix: *const f64,
    ) -> c_int {
        // SAFETY: `ctx` is the `Sink` passed to `falcon_ifc_read` below, and
        // the slices are web-ifc's buffers, alive for this call.
        let sink = unsafe { &mut *(ctx as *mut Sink) };
        let (verts, idx, color, m) = unsafe {
            (
                std::slice::from_raw_parts(vertices, vertex_floats as usize),
                std::slice::from_raw_parts(indices, index_count as usize),
                std::slice::from_raw_parts(color, 4),
                &*(matrix as *const [f64; 16]),
            )
        };
        let tris = idx.len() / 3;
        if sink.triangles + tris > sink.limits.max_triangles {
            sink.too_large = true;
            return 1;
        }
        let n = verts.len() / 6;
        let positions: Vec<[f64; 3]> = verts
            .as_chunks::<6>()
            .0
            .iter()
            .map(|v| place(m, [v[0] as f64, v[1] as f64, v[2] as f64]))
            .collect();
        let mut out = Vec::with_capacity(tris * 3);
        for t in idx.as_chunks::<3>().0 {
            if t.iter().any(|&i| i as usize >= n) || t.iter().any(|&i| !positions[i as usize].iter().all(|c| c.is_finite())) {
                continue;
            }
            out.extend_from_slice(t);
        }
        if out.is_empty() {
            return 0;
        }
        sink.triangles += out.len() / 3;
        let c = |k: usize| if color[k].is_finite() { color[k].clamp(0.0, 1.0) } else { 1.0 };
        sink.scene.geometries.push(Geometry { product, positions, indices: out, color: [c(0), c(1), c(2), c(3)] });
        0
    }

    unsafe extern "C" fn on_product(ctx: *mut c_void, product: u32, type_name: *const c_char, _geometries: u32) {
        // SAFETY: as above; `type_name` is a NUL-terminated string alive for
        // this call.
        let sink = unsafe { &mut *(ctx as *mut Sink) };
        let type_name = if type_name.is_null() { String::new() } else { unsafe { CStr::from_ptr(type_name) }.to_string_lossy().into_owned() };
        // Count what was kept (triangles that survived the checks above).
        let kept = sink.scene.geometries.iter().rev().take_while(|g| g.product == product).count() as u32;
        sink.scene.products.push(Product { id: product, type_name, geometries: kept });
    }

    pub(super) fn read(bytes: &[u8], limits: Limits) -> Result<Scene, Error> {
        let mut sink = Sink { scene: Scene::default(), triangles: 0, limits, too_large: false };
        let mut err = [0 as c_char; 512];
        // SAFETY: the callbacks only touch `sink` through `ctx`; `bytes` and
        // `err` outlive the call.
        let code = unsafe {
            falcon_ifc_read(
                bytes.as_ptr(),
                bytes.len(),
                limits.circle_segments,
                &mut sink as *mut Sink as *mut c_void,
                on_product,
                on_geometry,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        match code {
            0 => Ok(sink.scene),
            1 => Err(Error::Open),
            3 if sink.too_large => Err(Error::TooLarge),
            _ => {
                // SAFETY: the shim always NUL-terminates `err`.
                let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy().into_owned();
                Err(Error::Failed(msg))
            }
        }
    }
}

#[cfg(not(web_ifc))]
mod native {
    use super::{Error, Limits, Scene};

    pub(super) fn read(_bytes: &[u8], _limits: Limits) -> Result<Scene, Error> {
        Err(Error::Unavailable)
    }
}
