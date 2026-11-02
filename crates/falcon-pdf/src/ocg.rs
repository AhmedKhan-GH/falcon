//! Choosing which optional content groups ("layers") a PDF shows. PDFKit
//! draws a document's default configuration and has no way to change it,
//! so this writes another one: an incremental update, as PDF defines them,
//! appended to the file's own bytes and replacing the object that holds the
//! configuration. Nothing already in the file changes, so an encrypted
//! document stays readable with its password and the bytes never reach the
//! disk; the viewer opens them in memory.
//!
//! The update holds no strings, which an encrypted document would need
//! encrypted, apart from a catalog's own when `/OCProperties` sits directly
//! in it (see [`Patcher::new`]). lopdf reads the file; the update's bytes
//! are written here.

use lopdf::{Dictionary, LoadOptions, Object, ObjectId};

use crate::{Error, Result};

fn err(msg: impl Into<String>) -> Error {
    Error(msg.into())
}

fn pdf_err(e: lopdf::Error) -> Error {
    err(format!("the PDF's layers can't be read: {e}"))
}

/// What's needed to write configurations for one file, read once since
/// parsing a large document takes a moment.
pub struct Patcher {
    /// The groups, in `/OCGs` order, matching `Document::layers`.
    groups: Vec<ObjectId>,
    /// The object replaced by each update.
    object: ObjectId,
    /// `/OCProperties` without its configurations.
    properties: Dictionary,
    /// The configuration's `/Intent`, kept since it decides which groups
    /// count at all.
    intent: Option<Object>,
    /// The catalog, when `object` is the catalog (`/OCProperties` is direct).
    catalog: Option<Dictionary>,
    /// The trailer entries an update repeats.
    trailer: Dictionary,
    /// The file's object count, `/Size`.
    size: u32,
    /// Where the file's last cross-reference section is, as the file counts.
    prev: u64,
    /// Whether that section is a stream, not a table; an update matches it.
    xref_stream: bool,
    /// Bytes before `%PDF-`, which the file's offsets don't count.
    header: u64,
}

/// Drops stream contents, which only cost memory here. Object streams stay,
/// since dictionaries are read out of them.
fn keep(id: ObjectId, object: &mut Object) -> Option<(ObjectId, Object)> {
    match object {
        Object::Stream(s) if !s.dict.has_type(b"ObjStm") => None,
        _ => Some((id, object.clone())),
    }
}

impl Patcher {
    /// Reads the configuration's place in the PDF `bytes`, unlocking it with
    /// `password` if it has one. Fails when it can't be replaced: a file
    /// lopdf can't read, a group that isn't an indirect object, or an
    /// encrypted file with strings next to a direct `/OCProperties`.
    pub fn new(bytes: &[u8], password: Option<&str>) -> Result<Patcher> {
        let (prev, xref_stream, header) = last_section(bytes)?;
        let mut options = password.map(LoadOptions::with_password).unwrap_or_default();
        options.filter = Some(keep);
        let doc = lopdf::Document::load_mem_with_options(bytes, options).map_err(pdf_err)?;

        let root = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(pdf_err)?;
        let root_dict = doc.get_dictionary(root).map_err(pdf_err)?;
        let (object, source, catalog) = match root_dict.get(b"OCProperties").map_err(pdf_err)? {
            Object::Reference(id) => (*id, doc.get_dictionary(*id).map_err(pdf_err)?, None),
            Object::Dictionary(d) => (root, d, Some(root_dict.clone())),
            _ => return Err(err("the PDF's layers can't be read")),
        };
        let ocgs = source.get(b"OCGs").map_err(pdf_err)?;
        let (_, list) = doc.dereference(ocgs).map_err(pdf_err)?;
        let groups = list.as_array().map_err(pdf_err)?.iter().map(Object::as_reference).collect::<lopdf::Result<Vec<_>>>().map_err(pdf_err)?;
        let intent = source
            .get(b"D")
            .and_then(|d| doc.dereference(d))
            .and_then(|(_, d)| d.as_dict())
            .and_then(|d| d.get(b"Intent"))
            .ok()
            .filter(|i| !has_string(i))
            .cloned();
        let mut properties = Dictionary::new();
        properties.set("OCGs", ocgs.clone());

        let encrypted = doc.encryption_state.is_some() || doc.trailer.has(b"Encrypt");
        if encrypted && catalog.as_ref().is_some_and(|c| c.iter().any(|(k, v)| k != b"OCProperties" && has_string(v))) {
            return Err(err("the PDF's encryption keeps its layers from being switched"));
        }

        let mut trailer = Dictionary::new();
        trailer.set("Root", Object::Reference(root));
        for key in [&b"Info"[..], b"ID"] {
            if let Ok(v) = doc.trailer.get(key) {
                trailer.set(key, v.clone());
            }
        }
        // lopdf takes /Encrypt out of its trailer when it decrypts.
        let encrypt = doc.encryption_state.as_ref().and_then(|s| s.encrypt_object_id()).map(Object::Reference).or_else(|| doc.trailer.get(b"Encrypt").ok().cloned());
        if let Some(e) = encrypt {
            trailer.set("Encrypt", e);
        }
        let stated = doc.trailer.get(b"Size").and_then(Object::as_i64).unwrap_or(0).max(0) as u32;
        let size = stated.max(doc.max_id + 1).max(object.0 + 1);
        Ok(Patcher { groups, object, properties, intent, catalog, trailer, size, prev, xref_stream, header })
    }

    /// How many groups the file has.
    pub fn groups(&self) -> usize {
        self.groups.len()
    }

    /// The bytes to append to a file of `file_len` bytes so that it shows
    /// the groups `visible` marks. An automatic state (`/AS`) would override
    /// the choice, so the new configuration has none.
    pub fn update(&self, visible: &[bool], file_len: usize) -> Vec<u8> {
        let list = |on: bool| Object::Array(self.groups.iter().zip(visible).filter(|&(_, &v)| v == on).map(|(g, _)| Object::Reference(*g)).collect());
        let mut config = Dictionary::new();
        config.set("BaseState", Object::Name(b"ON".to_vec()));
        config.set("ON", list(true));
        config.set("OFF", list(false));
        if let Some(i) = &self.intent {
            config.set("Intent", i.clone());
        }
        let mut properties = self.properties.clone();
        properties.set("D", Object::Dictionary(config));
        let object = match &self.catalog {
            Some(c) => {
                let mut c = c.clone();
                c.set("OCProperties", Object::Dictionary(properties));
                Object::Dictionary(c)
            }
            None => Object::Dictionary(properties),
        };

        // Offsets in the file don't count what comes before its header.
        let base = file_len as u64 - self.header;
        let mut out = b"\n".to_vec();
        let at = base + out.len() as u64;
        let (n, generation) = self.object;
        out.extend_from_slice(format!("{n} {generation} obj\n").as_bytes());
        write(&mut out, &object);
        out.extend_from_slice(b"\nendobj\n");
        let xref = base + out.len() as u64;
        let mut trailer = self.trailer.clone();
        trailer.set("Prev", Object::Integer(self.prev as i64));
        if self.xref_stream {
            // Uncompressed, as a cross-reference stream is never encrypted.
            let own = self.size;
            trailer.set("Type", Object::Name(b"XRef".to_vec()));
            trailer.set("Size", Object::Integer(i64::from(own) + 1));
            trailer.set("W", Object::Array(vec![1.into(), 8.into(), 2.into()]));
            trailer.set("Index", Object::Array(vec![i64::from(n).into(), 1.into(), i64::from(own).into(), 1.into()]));
            let mut entries = Vec::new();
            for (offset, g) in [(at, generation), (xref, 0)] {
                entries.push(1u8);
                entries.extend_from_slice(&offset.to_be_bytes());
                entries.extend_from_slice(&g.to_be_bytes());
            }
            trailer.set("Length", Object::Integer(entries.len() as i64));
            out.extend_from_slice(format!("{own} 0 obj\n").as_bytes());
            write(&mut out, &Object::Dictionary(trailer));
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&entries);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        } else {
            trailer.set("Size", Object::Integer(i64::from(self.size)));
            // Each table entry is exactly 20 bytes, line end included.
            out.extend_from_slice(format!("xref\n{n} 1\n{at:010} {generation:05} n \ntrailer\n").as_bytes());
            write(&mut out, &Object::Dictionary(trailer));
            out.push(b'\n');
        }
        out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }
}

/// Writes an object in PDF syntax. Strings go out in hex, whatever form they
/// came in. No stream reaches here, as a catalog holds none directly.
fn write(out: &mut Vec<u8>, object: &Object) {
    match object {
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Object::Real(r) => out.extend_from_slice(r.to_string().as_bytes()),
        Object::Name(n) => write_name(out, n),
        Object::String(s, _) => {
            out.push(b'<');
            for b in s {
                out.extend_from_slice(format!("{b:02X}").as_bytes());
            }
            out.push(b'>');
        }
        Object::Array(a) => {
            out.push(b'[');
            for item in a {
                write(out, item);
                out.push(b' ');
            }
            out.push(b']');
        }
        Object::Dictionary(d) => {
            out.extend_from_slice(b"<<");
            for (k, v) in d.iter() {
                write_name(out, k);
                out.push(b' ');
                write(out, v);
                out.push(b' ');
            }
            out.extend_from_slice(b">>");
        }
        Object::Reference((n, g)) => out.extend_from_slice(format!("{n} {g} R").as_bytes()),
        Object::Null | Object::Stream(_) => out.extend_from_slice(b"null"),
    }
}

fn write_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &b in name {
        if b.is_ascii_graphic() && !b"#()<>[]{}/%".contains(&b) {
            out.push(b);
        } else {
            out.extend_from_slice(format!("#{b:02X}").as_bytes());
        }
    }
}

fn has_string(object: &Object) -> bool {
    match object {
        Object::String(..) => true,
        Object::Array(a) => a.iter().any(has_string),
        Object::Dictionary(d) => d.iter().any(|(_, v)| has_string(v)),
        _ => false,
    }
}

/// The last cross-reference section of a file: its offset as the file
/// counts them, whether it is a stream, and the bytes before the header.
fn last_section(bytes: &[u8]) -> Result<(u64, bool, u64)> {
    let bad = || err("the PDF's cross-reference table can't be read");
    let header = bytes.windows(5).take(1024).position(|w| w == b"%PDF-").unwrap_or(0);
    let keyword = bytes.windows(9).rposition(|w| w == b"startxref").ok_or_else(bad)?;
    let digits: Vec<u8> = bytes[keyword + 9..].iter().copied().skip_while(u8::is_ascii_whitespace).take_while(u8::is_ascii_digit).collect();
    let prev: u64 = std::str::from_utf8(&digits).ok().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
    let start = usize::try_from(prev).ok().and_then(|p| p.checked_add(header)).filter(|&s| s < bytes.len()).ok_or_else(bad)?;
    let section = &bytes[start..];
    let skip = section.iter().take_while(|b| b.is_ascii_whitespace()).count();
    let section = &section[skip..];
    if section.starts_with(b"xref") {
        Ok((prev, false, header as u64))
    } else if section.first().is_some_and(u8::is_ascii_digit) {
        Ok((prev, true, header as u64))
    } else {
        Err(bad())
    }
}
