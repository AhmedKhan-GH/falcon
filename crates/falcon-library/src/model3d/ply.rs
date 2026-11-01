//! PLY, ASCII and binary little-endian, with vertex colours.

use super::math::V3;
use super::{Mesh, Model, ParseError, Result};

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Ty {
    fn from_name(s: &str) -> Option<Ty> {
        Some(match s {
            "char" | "int8" => Ty::I8,
            "uchar" | "uint8" => Ty::U8,
            "short" | "int16" => Ty::I16,
            "ushort" | "uint16" => Ty::U16,
            "int" | "int32" => Ty::I32,
            "uint" | "uint32" => Ty::U32,
            "float" | "float32" => Ty::F32,
            "double" | "float64" => Ty::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::F64 => 8,
        }
    }

    /// What a colour channel of this type divides by to reach 0..1.
    fn color_scale(self) -> f64 {
        match self {
            Ty::U8 | Ty::I8 => 255.0,
            Ty::U16 | Ty::I16 => 65535.0,
            Ty::U32 | Ty::I32 => 4294967295.0,
            Ty::F32 | Ty::F64 => 1.0,
        }
    }
}

struct Prop {
    name: String,
    ty: Ty,
    /// The type of the count when this is a list.
    list: Option<Ty>,
}

struct Element {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Ascii,
    Little,
    Big,
}

struct Header {
    format: Format,
    elements: Vec<Element>,
    /// Where the data starts.
    body: usize,
}

fn parse_header(bytes: &[u8]) -> Result<Header> {
    let mut pos = 0;
    let mut format = None;
    let mut elements: Vec<Element> = Vec::new();
    let mut first = true;
    loop {
        if pos >= bytes.len() {
            return Err(ParseError::new("PLY header has no end_header"));
        }
        let end = bytes[pos..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| pos + i);
        let line = String::from_utf8_lossy(&bytes[pos..end]).into_owned();
        pos = (end + 1).min(bytes.len());
        let mut w = line.split_whitespace();
        let Some(key) = w.next() else { continue };
        if first {
            if key != "ply" {
                return Err(ParseError::new("not a PLY file"));
            }
            first = false;
            continue;
        }
        let bad = || ParseError(format!("bad PLY header line: {}", line.trim()));
        match key {
            "format" => {
                format = Some(match w.next() {
                    Some("ascii") => Format::Ascii,
                    Some("binary_little_endian") => Format::Little,
                    Some("binary_big_endian") => Format::Big,
                    _ => return Err(bad()),
                });
            }
            "element" => {
                let name = w.next().ok_or_else(bad)?.to_string();
                let count = w.next().and_then(|c| c.parse().ok()).ok_or_else(bad)?;
                elements.push(Element { name, count, props: Vec::new() });
            }
            "property" => {
                let el = elements.last_mut().ok_or_else(bad)?;
                let t = w.next().ok_or_else(bad)?;
                if t == "list" {
                    let ct = w.next().and_then(Ty::from_name).ok_or_else(bad)?;
                    let it = w.next().and_then(Ty::from_name).ok_or_else(bad)?;
                    let name = w.next().ok_or_else(bad)?.to_string();
                    el.props.push(Prop { name, ty: it, list: Some(ct) });
                } else {
                    let ty = Ty::from_name(t).ok_or_else(bad)?;
                    let name = w.next().ok_or_else(bad)?.to_string();
                    el.props.push(Prop { name, ty, list: None });
                }
            }
            "end_header" => break,
            _ => {} // comment, obj_info
        }
    }
    let format = format.ok_or_else(|| ParseError::new("PLY header has no format"))?;
    Ok(Header { format, elements, body: pos })
}

/// Reads values one at a time from the data section.
struct Src<'a> {
    data: &'a [u8],
    pos: usize,
    format: Format,
}

impl Src<'_> {
    #[inline]
    fn read(&mut self, ty: Ty) -> Option<f64> {
        if self.format == Format::Ascii {
            return self.read_ascii();
        }
        let n = ty.size();
        let s = self.data.get(self.pos..self.pos + n)?;
        self.pos += n;
        let big = self.format == Format::Big;
        macro_rules! num {
            ($t:ty) => {{
                let a = s.try_into().ok()?;
                (if big { <$t>::from_be_bytes(a) } else { <$t>::from_le_bytes(a) }) as f64
            }};
        }
        Some(match ty {
            Ty::I8 => s[0] as i8 as f64,
            Ty::U8 => s[0] as f64,
            Ty::I16 => num!(i16),
            Ty::U16 => num!(u16),
            Ty::I32 => num!(i32),
            Ty::U32 => num!(u32),
            Ty::F32 => num!(f32),
            Ty::F64 => num!(f64),
        })
    }

    fn read_ascii(&mut self) -> Option<f64> {
        let d = self.data;
        let mut p = self.pos;
        while p < d.len() && d[p].is_ascii_whitespace() {
            p += 1;
        }
        let start = p;
        while p < d.len() && !d[p].is_ascii_whitespace() {
            p += 1;
        }
        self.pos = p;
        std::str::from_utf8(&d[start..p]).ok()?.parse().ok()
    }
}

fn eof() -> ParseError {
    ParseError::new("PLY data ends early or is malformed")
}

pub fn parse(bytes: &[u8]) -> Result<Model> {
    let h = parse_header(bytes)?;
    let mut src = Src { data: bytes, pos: h.body, format: h.format };
    let mut mesh = Mesh::default();
    let mut have_vertex = false;
    for el in &h.elements {
        match el.name.as_str() {
            "vertex" if !have_vertex => {
                have_vertex = true;
                read_vertices(&mut src, el, &mut mesh)?;
            }
            "face" => read_faces(&mut src, el, &mut mesh)?,
            _ => skip_element(&mut src, el)?,
        }
    }
    Model { meshes: vec![mesh] }.finish()
}

fn skip_element(src: &mut Src, el: &Element) -> Result<()> {
    if src.format != Format::Ascii && el.props.iter().all(|p| p.list.is_none()) {
        let row: usize = el.props.iter().map(|p| p.ty.size()).sum();
        src.pos = row.checked_mul(el.count).and_then(|n| src.pos.checked_add(n)).filter(|&e| e <= src.data.len()).ok_or_else(eof)?;
        return Ok(());
    }
    for _ in 0..el.count {
        for p in &el.props {
            skip_prop(src, p)?;
        }
    }
    Ok(())
}

fn skip_prop(src: &mut Src, p: &Prop) -> Result<()> {
    match p.list {
        None => {
            src.read(p.ty).ok_or_else(eof)?;
        }
        Some(ct) => {
            let n = src.read(ct).ok_or_else(eof)? as usize;
            if src.format == Format::Ascii {
                for _ in 0..n {
                    src.read(p.ty).ok_or_else(eof)?;
                }
            } else {
                src.pos = src.pos.saturating_add(n.saturating_mul(p.ty.size()));
                if src.pos > src.data.len() {
                    return Err(eof());
                }
            }
        }
    }
    Ok(())
}

fn read_vertices(src: &mut Src, el: &Element, mesh: &mut Mesh) -> Result<()> {
    let find = |n: &str| el.props.iter().position(|p| p.name == n && p.list.is_none());
    let (Some(x), Some(y), Some(z)) = (find("x"), find("y"), find("z")) else {
        return Err(ParseError::new("PLY vertices have no x, y, z"));
    };
    let normal = find("nx").zip(find("ny")).zip(find("nz")).map(|((a, b), c)| [a, b, c]);
    let color = find("red").zip(find("green")).zip(find("blue")).map(|((a, b), c)| [a, b, c]);
    let cap = el.count.min(src.data.len());
    mesh.positions.reserve(cap);
    let mut normals: Vec<V3> = Vec::with_capacity(if normal.is_some() { cap } else { 0 });
    let mut colors: Vec<V3> = Vec::with_capacity(if color.is_some() { cap } else { 0 });
    let mut row = vec![0.0f64; el.props.len()];
    for _ in 0..el.count {
        for (i, p) in el.props.iter().enumerate() {
            if p.list.is_some() {
                skip_prop(src, p)?;
            } else {
                row[i] = src.read(p.ty).ok_or_else(eof)?;
            }
        }
        mesh.positions.push([row[x] as f32, row[y] as f32, row[z] as f32]);
        if let Some(n) = normal {
            normals.push(n.map(|i| row[i] as f32));
        }
        if let Some(c) = color {
            colors.push(c.map(|i| (row[i] / el.props[i].ty.color_scale()) as f32));
        }
    }
    mesh.normals = normal.map(|_| normals);
    mesh.colors = color.map(|_| colors);
    Ok(())
}

fn read_faces(src: &mut Src, el: &Element, mesh: &mut Mesh) -> Result<()> {
    mesh.indices.reserve(el.count.min(src.data.len()) * 3);
    let mut poly: Vec<u32> = Vec::new();
    for _ in 0..el.count {
        for p in &el.props {
            let is_idx = p.list.is_some() && (p.name == "vertex_indices" || p.name == "vertex_index");
            if !is_idx {
                skip_prop(src, p)?;
                continue;
            }
            let n = src.read(p.list.unwrap_or(Ty::U8)).ok_or_else(eof)? as usize;
            poly.clear();
            let mut ok = true;
            for _ in 0..n {
                let v = src.read(p.ty).ok_or_else(eof)?;
                if v < 0.0 || v > u32::MAX as f64 {
                    ok = false;
                }
                poly.push(v as u32);
            }
            if ok {
                for k in 1..poly.len().saturating_sub(1) {
                    mesh.indices.extend([poly[0], poly[k], poly[k + 1]]);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = "ply\nformat ascii 1.0\ncomment square\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\n\
                          element face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";

    #[test]
    fn ascii_quad_is_two_triangles() {
        let m = parse(SQUARE.as_bytes()).unwrap();
        assert_eq!(m.meshes.len(), 1);
        assert_eq!(m.triangle_count(), 2);
        assert_eq!(m.vertex_count(), 4);
        assert_eq!(m.meshes[0].indices, vec![0, 1, 2, 0, 2, 3]);
        assert!(m.meshes[0].colors.is_none() && m.meshes[0].normals.is_none());
    }

    #[test]
    fn crlf_header() {
        let m = parse(SQUARE.replace('\n', "\r\n").as_bytes()).unwrap();
        assert_eq!(m.triangle_count(), 2);
    }

    fn binary_with_unknown_element(big: bool) -> Vec<u8> {
        let fmt = if big { "binary_big_endian" } else { "binary_little_endian" };
        let mut out = format!(
            "ply\nformat {fmt} 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\n\
             property uchar red\nproperty uchar green\nproperty uchar blue\nproperty uchar alpha\n\
             element junk 2\nproperty uchar tag\nproperty list uchar short things\n\
             element face 1\nproperty list uchar uint vertex_indices\nproperty float quality\nend_header\n"
        )
        .into_bytes();
        let f = |v: f32| if big { v.to_be_bytes() } else { v.to_le_bytes() };
        let pts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        for (i, p) in pts.iter().enumerate() {
            for c in p {
                out.extend(f(*c));
            }
            for c in [0.0, 0.0, 1.0] {
                out.extend(f(c));
            }
            out.extend([255, 0, 51 * i as u8, 255]);
        }
        out.extend([9, 3, 1, 0, 2, 0, 3, 0]); // tag 9, list of 3 shorts (LE)
        out.extend([8, 0]); // tag 8, empty list
        out.push(3);
        for i in [0u32, 1, 2] {
            out.extend(if big { i.to_be_bytes() } else { i.to_le_bytes() });
        }
        out.extend(f(0.5));
        out
    }

    #[test]
    fn binary_little_endian_colours_normals_and_skipped_element() {
        let bytes = binary_with_unknown_element(false);
        // The short list payload is little endian; fine for the LE file.
        let m = parse(&bytes).unwrap();
        let mesh = &m.meshes[0];
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.normals.as_ref().unwrap()[2], [0.0, 0.0, 1.0]);
        let c = mesh.colors.as_ref().unwrap();
        assert_eq!(c[0], [1.0, 0.0, 0.0]);
        assert!((c[2][2] - 102.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn binary_big_endian() {
        let mut bytes = binary_with_unknown_element(true);
        // Make the short-list payload big endian too (values 1, 2, 3).
        let at = bytes.windows(8).position(|w| w == [9, 3, 1, 0, 2, 0, 3, 0]).unwrap();
        bytes[at + 1..at + 8].copy_from_slice(&[3, 0, 1, 0, 2, 0, 3]);
        let m = parse(&bytes).unwrap();
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.meshes[0].positions[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn ascii_float_colours() {
        let src = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\n\
                   property float red\nproperty float green\nproperty float blue\nelement face 1\nproperty list uchar int vertex_index\nend_header\n\
                   0 0 0 1 0 0\n1 0 0 0 0.5 0\n0 1 0 0 0 1\n3 0 1 2\n";
        let m = parse(src.as_bytes()).unwrap();
        let c = m.meshes[0].colors.as_ref().unwrap();
        assert_eq!(c[0], [1.0, 0.0, 0.0]);
        assert_eq!(c[1], [0.0, 0.5, 0.0]);
    }

    #[test]
    fn point_cloud_is_an_error() {
        let src = "ply\nformat ascii 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nend_header\n0 0 0\n1 1 1\n";
        assert!(parse(src.as_bytes()).is_err());
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(parse(b"hello").is_err());
        assert!(parse(b"ply\nformat ascii 1.0\nelement vertex 1\n").is_err());
        // Truncated body.
        let b = binary_with_unknown_element(false);
        assert!(parse(&b[..b.len() - 10]).is_err());
    }

    #[test]
    fn out_of_range_faces_are_dropped() {
        let src = SQUARE.replace("4 0 1 2 3", "3 0 1 9");
        assert!(parse(src.as_bytes()).is_err());
    }
}
