//! Font metadata stored at import as `fontMetas`, in the shape Eagle's font
//! parser produces (see the Import chapter of the docs).
//!
//! Each `name` table entry becomes a map from language code to text, keyed
//! by the entry's name (`fontFamily`, `postScriptName`, ...), followed by
//! `numGlyphs`, `support` and `weight`.

use std::borrow::Cow;

use serde_json::{Map, Value, json};
use ttf_parser::{Face, PlatformId};

/// Names of the `name` table ids 0..=22, as Eagle's parser keys them.
const NAME_KEYS: [&str; 23] = [
    "copyright",
    "fontFamily",
    "fontSubfamily",
    "uniqueID",
    "fullName",
    "version",
    "postScriptName",
    "trademark",
    "manufacturer",
    "designer",
    "description",
    "manufacturerURL",
    "designerURL",
    "license",
    "licenseURL",
    "reserved",
    "preferredFamily",
    "preferredSubfamily",
    "compatibleFullName",
    "sampleText",
    "postScriptFindFontName",
    "wwsFamily",
    "wwsSubfamily",
];

/// Weights Eagle accepts, in its order of testing against the PostScript name.
const WEIGHT_WORDS: [(&str, &str); 16] = [
    ("thin", "Thin"),
    ("hairline", "Hairline"),
    ("extralight", "ExtraLight"),
    ("ultralight", "UltraLight"),
    ("light", "Light"),
    ("regular", "Regular"),
    ("normal", "Regular"),
    ("medium", "Medium"),
    ("demibold", "DemiBold"),
    ("semibold", "SemiBold"),
    ("bold", "Bold"),
    ("extrabold", "ExtraBold"),
    ("ultrabold", "UltraBold"),
    ("black", "Black"),
    ("extrablack", "ExtraBlack"),
    ("heavy", "Heavy"),
];

/// Language code for a Windows language id (a common subset of the codes
/// Eagle's parser knows).
fn windows_language(id: u16) -> Option<&'static str> {
    Some(match id {
        0x0409 => "en",
        0x0809 => "en-GB",
        0x0C09 => "en-AU",
        0x1009 => "en-CA",
        0x0804 => "zh",
        0x0404 => "zh-TW",
        0x0C04 => "zh-HK",
        0x1004 => "zh-SG",
        0x1404 => "zh-MO",
        0x0411 => "ja",
        0x0412 => "ko",
        0x040C => "fr",
        0x0C0C => "fr-CA",
        0x0407 => "de",
        0x0410 => "it",
        0x0C0A => "es",
        0x040A => "es",
        0x0416 => "pt",
        0x0816 => "pt-PT",
        0x0413 => "nl",
        0x0419 => "ru",
        0x041D => "sv",
        0x0406 => "da",
        0x0414 => "nb",
        0x040B => "fi",
        0x0415 => "pl",
        0x0405 => "cs",
        0x040E => "hu",
        0x0408 => "el",
        0x041F => "tr",
        0x040D => "he",
        0x0401 => "ar",
        0x041E => "th",
        0x042A => "vi",
        0x0421 => "id",
        0x0422 => "uk",
        0x0418 => "ro",
        0x041B => "sk",
        0x0424 => "sl",
        0x041A => "hr",
        0x0402 => "bg",
        0x0403 => "ca",
        0x042D => "eu",
        _ => return None,
    })
}

/// Language code for a Macintosh language id written in the Roman script.
fn mac_language(id: u16) -> Option<&'static str> {
    Some(match id {
        0 => "en",
        1 => "fr",
        2 => "de",
        3 => "it",
        4 => "nl",
        5 => "sv",
        6 => "es",
        7 => "da",
        8 => "pt",
        9 => "no",
        13 => "fi",
        _ => return None,
    })
}

/// The upper half of the Mac OS Roman code page.
const MAC_ROMAN_HIGH: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{a0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{f8ff}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

fn mac_roman(bytes: &[u8]) -> String {
    let high: Vec<char> = MAC_ROMAN_HIGH.chars().collect();
    bytes.iter().map(|&b| if b < 0x80 { b as char } else { high.get(usize::from(b - 0x80)).copied().unwrap_or('\u{fffd}') }).collect()
}

fn utf16_be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
    String::from_utf16_lossy(&units)
}

/// Unpacks a WOFF or WOFF 2 file into the plain sfnt it wraps; other data
/// is returned as is. From WOFF 2, tables stored with a transform (the
/// packed `glyf`/`loca` and `hmtx`) are left out: metadata needs only the
/// names, `maxp`, `cmap` and the other plain tables.
pub fn sfnt(data: &[u8]) -> Option<Cow<'_, [u8]>> {
    match data.get(..4) {
        Some(b"wOFF") => woff1_tables(data).map(|(flavor, t)| Cow::Owned(build_sfnt(flavor, &t))),
        Some(b"wOF2") => woff2_tables(data).map(|(flavor, t)| Cow::Owned(build_sfnt(flavor, &t))),
        _ => Some(Cow::Borrowed(data)),
    }
}

fn be16(data: &[u8], o: usize) -> Option<u16> {
    data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], o: usize) -> Option<u32> {
    data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// One table for [`build_sfnt`]: tag, checksum, bytes.
type Table = (u32, u32, Vec<u8>);

fn woff1_tables(data: &[u8]) -> Option<(u32, Vec<Table>)> {
    let flavor = be32(data, 4)?;
    let num = be16(data, 12)?;
    let mut tables = Vec::with_capacity(num.into());
    for i in 0..usize::from(num) {
        let e = 44 + i * 20;
        let tag = be32(data, e)?;
        let (off, comp, orig, sum) = (be32(data, e + 4)? as usize, be32(data, e + 8)? as usize, be32(data, e + 12)? as usize, be32(data, e + 16)?);
        let raw = data.get(off..off.checked_add(comp)?)?;
        let bytes = if comp < orig { miniz_oxide::inflate::decompress_to_vec_zlib(raw).ok()? } else { raw.to_vec() };
        if bytes.len() != orig {
            return None;
        }
        tables.push((tag, sum, bytes));
    }
    Some((flavor, tables))
}

/// Tags a WOFF 2 directory entry can name by index (from the WOFF 2
/// specification).
const WOFF2_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm", b"glyf", b"loca", b"prep", b"CFF ", b"VORG",
    b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern", b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar", b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc",
    b"feat", b"fmtx", b"fvar", b"gvar", b"hsty", b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

/// A WOFF 2 `UIntBase128`: big-endian groups of 7 bits, at most five bytes,
/// no leading zero group.
fn base128(data: &[u8], at: &mut usize) -> Option<u32> {
    let mut v: u32 = 0;
    for i in 0..5 {
        let b = *data.get(*at)?;
        *at += 1;
        if i == 0 && b == 0x80 {
            return None;
        }
        if v & 0xFE00_0000 != 0 {
            return None;
        }
        v = (v << 7) | u32::from(b & 0x7F);
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Reads a single-font WOFF 2 file's untransformed tables. Collections are
/// not read.
fn woff2_tables(data: &[u8]) -> Option<(u32, Vec<Table>)> {
    use std::io::Read;
    let flavor = be32(data, 4)?;
    if &flavor.to_be_bytes() == b"ttcf" {
        return None;
    }
    let num = usize::from(be16(data, 12)?);
    let compressed_len = be32(data, 20)? as usize;
    let mut at = 48;
    // (tag, stored length, keep)
    let mut entries = Vec::with_capacity(num);
    for _ in 0..num {
        let flags = *data.get(at)?;
        at += 1;
        let tag = match usize::from(flags & 0x3F) {
            63 => {
                let t = be32(data, at)?;
                at += 4;
                t
            }
            i => u32::from_be_bytes(*WOFF2_TAGS[i]),
        };
        let version = flags >> 6;
        let orig = base128(data, &mut at)?;
        let glyf_or_loca = &tag.to_be_bytes() == b"glyf" || &tag.to_be_bytes() == b"loca";
        // Version 0 is the transform for glyf and loca, the null transform
        // for every other table; glyf and loca use 3 for "none".
        let transformed = if glyf_or_loca { version != 3 } else { version != 0 };
        let stored = if transformed { base128(data, &mut at)? } else { orig };
        entries.push((tag, stored as usize, !transformed));
    }
    let total: usize = entries.iter().try_fold(0usize, |acc, e| acc.checked_add(e.1))?;
    let stream = data.get(at..at.checked_add(compressed_len)?)?;
    let mut raw = Vec::with_capacity(total.min(64 << 20));
    brotli_decompressor::Decompressor::new(stream, 4096).take(total as u64).read_to_end(&mut raw).ok()?;
    if raw.len() != total {
        return None;
    }
    let mut tables = Vec::new();
    let mut off = 0;
    for (tag, len, keep) in entries {
        let bytes = &raw[off..off + len];
        off += len;
        if keep {
            tables.push((tag, checksum(bytes), bytes.to_vec()));
        }
    }
    tables.sort_by_key(|t| t.0);
    Some((flavor, tables))
}

/// The sfnt table checksum: the sum of big-endian words, zero-padded.
fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// Lays out an sfnt file: the offset table, the directory, then each table
/// padded to four bytes.
fn build_sfnt(flavor: u32, tables: &[Table]) -> Vec<u8> {
    let num = tables.len() as u16;
    let mut pow = 1u16;
    let mut log = 0u16;
    while pow * 2 <= num {
        pow *= 2;
        log += 1;
    }
    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    for v in [num, pow * 16, log, (num * 16).saturating_sub(pow * 16)] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 12 + tables.len() * 16;
    for (tag, sum, bytes) in tables {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&sum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        offset += bytes.len().div_ceil(4) * 4;
    }
    for (_, _, bytes) in tables {
        out.extend_from_slice(bytes);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    out
}

/// Parses a font file (ttf, otf, or the first face of a ttc/otc); unpack
/// WOFF and WOFF 2 with [`sfnt`] first.
pub fn parse(data: &[u8]) -> Option<Face<'_>> {
    Face::parse(data, 0).ok()
}

/// The `name` table as Eagle's parser reads it: entries in table order, each
/// a map from language code to text. A later record for the same entry and
/// language replaces the earlier text but keeps its position.
pub fn names(face: &Face) -> Map<String, Value> {
    let mut out = Map::new();
    for n in face.names() {
        let (lang, text) = match n.platform_id {
            PlatformId::Windows if matches!(n.encoding_id, 1 | 10) => (windows_language(n.language_id), utf16_be(n.name)),
            PlatformId::Macintosh if n.encoding_id == 0 => (mac_language(n.language_id), mac_roman(n.name)),
            PlatformId::Unicode if n.language_id == 0xFFFF => (Some("und"), utf16_be(n.name)),
            _ => continue,
        };
        let Some(lang) = lang else { continue };
        if text.is_empty() {
            continue;
        }
        let key = NAME_KEYS.get(usize::from(n.name_id)).map_or_else(|| n.name_id.to_string(), |k| k.to_string());
        let entry = out.entry(key).or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = entry {
            m.insert(lang.to_string(), Value::String(text));
        }
    }
    out
}

fn first_value<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    m.get(key)?.as_object()?.values().next()?.as_str()
}

fn en_value<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    m.get(key)?.get("en")?.as_str()
}

/// Eagle's weight label, guessed from the PostScript name, then from the
/// subfamily, with " Italic" added when the PostScript name says so.
pub fn weight(metas: &Map<String, Value>) -> String {
    let ps = first_value(metas, "postScriptName").unwrap_or("").to_lowercase();
    if ps.is_empty() {
        return "Regular".into();
    }
    let mut w = WEIGHT_WORDS.iter().find(|(word, _)| ps.contains(word)).map(|(_, w)| w.to_string());
    if w.is_none() {
        w = Some(en_value(metas, "preferredSubfamily").or_else(|| en_value(metas, "fontSubfamily")).unwrap_or("Regular").to_string());
    }
    let mut w = w.filter(|w| WEIGHT_WORDS.iter().any(|(_, x)| x == w)).unwrap_or_else(|| "Regular".into());
    if !w.to_lowercase().contains("italic") && ps.contains("italic") {
        w.push_str(" Italic");
    }
    w
}

/// The font viewer's information panel, read from `fontMetas` as Eagle's
/// viewer reads it: each name in English when the font has it, else its
/// first language. Missing names are empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontInfo {
    pub postscript: String,
    pub full_name: String,
    pub family: String,
    /// `weight`, the style label Eagle stores.
    pub style: String,
    pub version: String,
    pub designer: String,
    pub manufacturer: String,
    pub description: String,
    /// The copyright notice, or the licence text when there is none.
    pub copyright: String,
    pub trademark: String,
    pub glyphs: Option<u64>,
}

impl FontInfo {
    pub fn from_metas(metas: &Map<String, Value>) -> FontInfo {
        let name = |k: &str| en_value(metas, k).or_else(|| first_value(metas, k)).unwrap_or("").trim().to_string();
        let copyright = name("copyright");
        FontInfo {
            postscript: name("postScriptName"),
            full_name: name("fullName"),
            family: name("fontFamily"),
            style: metas.get("weight").and_then(Value::as_str).unwrap_or("").to_string(),
            version: name("version"),
            designer: name("designer"),
            manufacturer: name("manufacturer"),
            description: name("description"),
            copyright: if copyright.is_empty() { name("license") } else { copyright },
            trademark: name("trademark"),
            glyphs: metas.get("numGlyphs").and_then(Value::as_u64),
        }
    }
}

/// The sample languages Eagle's "change language" offers, as stored in
/// `fontMetas.preferLng`, with their names.
pub const SAMPLE_LANGUAGES: [(&str, &str); 5] =
    [("en", "English"), ("jp", "日本語"), ("kr", "한국어"), ("zh_CN", "简体中文"), ("zh_TW", "繁體中文")];

/// Builds `fontMetas` for a font file's bytes, or `None` if it can't be
/// parsed. `fallback_id` names a font whose table has no names at all.
pub fn font_metas(data: &[u8], fallback_id: &str) -> Option<Map<String, Value>> {
    let data = sfnt(data)?;
    let face = parse(&data)?;
    let mut metas = names(&face);
    if metas.is_empty() {
        metas.insert("postScriptName".into(), json!({ "en": fallback_id }));
        metas.insert("fullName".into(), json!({ "en": fallback_id }));
    }
    if !metas.contains_key("postScriptName") {
        let id = en_value(&metas, "uniqueID").unwrap_or(fallback_id).to_string();
        metas.insert("postScriptName".into(), json!({ "en": id }));
    }
    metas.insert("numGlyphs".into(), json!(face.number_of_glyphs()));
    let has = |s: &str| s.chars().all(|c| face.glyph_index(c).is_some_and(|g| g.0 > 0));
    let mut support = Map::new();
    if has("龙岁") {
        support.insert("zh_CN".into(), json!(true));
    }
    if has("実匁図") {
        support.insert("jp".into(), json!(true));
    }
    if has("陳鑿發") {
        support.insert("zh_TW".into(), json!(true));
    }
    if has("별았") {
        support.insert("kr".into(), json!(true));
    }
    metas.insert("support".into(), Value::Object(support));
    let w = weight(&metas);
    metas.insert("weight".into(), json!(w));
    Some(metas)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metas(ps: &str, sub: Option<&str>) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("postScriptName".into(), json!({ "en": ps }));
        if let Some(s) = sub {
            m.insert("fontSubfamily".into(), json!({ "en": s }));
        }
        m
    }

    #[test]
    fn weights() {
        assert_eq!(weight(&metas("Inter-Thin", None)), "Thin");
        assert_eq!(weight(&metas("Inter-SemiBoldItalic", None)), "SemiBold Italic");
        // "bold" is tested before "extrabold", as in Eagle.
        assert_eq!(weight(&metas("Inter-ExtraBold", None)), "Bold");
        assert_eq!(weight(&metas("Foo", Some("Medium"))), "Medium");
        assert_eq!(weight(&metas("Foo", Some("Oblique"))), "Regular");
        assert_eq!(weight(&metas("Foo-Italic", None)), "Regular Italic");
        assert_eq!(weight(&Map::new()), "Regular");
    }

    #[test]
    fn info_prefers_english_names() {
        let m = json!({
            "copyright": {"en": "(c) Someone"},
            "fontFamily": {"zh": "字体", "en": "Ziti"},
            "fullName": {"ja": "フル"},
            "postScriptName": {"en": " Ziti-Bold "},
            "license": {"en": "OFL"},
            "numGlyphs": 1234,
            "weight": "Bold",
        });
        let info = FontInfo::from_metas(m.as_object().unwrap());
        assert_eq!(info.family, "Ziti");
        assert_eq!(info.full_name, "フル", "the first language when there is no English");
        assert_eq!(info.postscript, "Ziti-Bold", "trimmed");
        assert_eq!(info.copyright, "(c) Someone");
        assert_eq!((info.style.as_str(), info.glyphs), ("Bold", Some(1234)));
        assert_eq!(info.designer, "");
        let no_copyright = FontInfo::from_metas(json!({"license": {"en": "OFL"}}).as_object().unwrap());
        assert_eq!(no_copyright.copyright, "OFL", "the licence stands in for a missing copyright");
        assert_eq!(FontInfo::from_metas(&Map::new()), FontInfo::default());
    }

    #[test]
    fn mac_roman_text() {
        assert_eq!(MAC_ROMAN_HIGH.chars().count(), 128);
        assert_eq!(mac_roman(b"Caf\x8e \xa9 \xff"), "Café © ˇ");
    }

    #[test]
    fn system_font() {
        let path = ["/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Helvetica.ttc"]
            .into_iter()
            .find(|p| std::path::Path::new(p).is_file());
        let Some(path) = path else { return };
        let data = std::fs::read(path).unwrap();
        let m = font_metas(&data, "X").unwrap();
        assert!(first_value(&m, "fontFamily").is_some());
        assert!(first_value(&m, "postScriptName").is_some());
        let keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
        assert_eq!(&keys[keys.len() - 3..], ["numGlyphs", "support", "weight"]);
        assert!(m["numGlyphs"].as_u64().unwrap() > 100);
    }

    #[test]
    fn woff_round_trip() {
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") else { return };
        // Wrap the font's tables in a WOFF container, compressing each.
        let face = ttf_parser::RawFace::parse(&data, 0).unwrap();
        let records: Vec<_> = face.table_records.into_iter().collect();
        let mut dir = Vec::new();
        let mut blobs = Vec::new();
        let mut offset = 44 + records.len() * 20;
        for r in &records {
            let raw = &data[r.offset as usize..(r.offset + r.length) as usize];
            let comp = miniz_oxide::deflate::compress_to_vec_zlib(raw, 6);
            let body = if comp.len() < raw.len() { comp } else { raw.to_vec() };
            dir.extend_from_slice(&r.tag.as_u32().to_be_bytes());
            dir.extend_from_slice(&(offset as u32).to_be_bytes());
            dir.extend_from_slice(&(body.len() as u32).to_be_bytes());
            dir.extend_from_slice(&r.length.to_be_bytes());
            dir.extend_from_slice(&r.check_sum.to_be_bytes());
            offset += body.len().div_ceil(4) * 4;
            let mut body = body;
            body.resize(body.len().div_ceil(4) * 4, 0);
            blobs.push(body);
        }
        let mut woff = b"wOFF".to_vec();
        woff.extend_from_slice(&data[..4]);
        woff.extend_from_slice(&(offset as u32).to_be_bytes());
        woff.extend_from_slice(&(records.len() as u16).to_be_bytes());
        woff.resize(44, 0);
        woff.extend(dir);
        for b in blobs {
            woff.extend(b);
        }
        let a = font_metas(&data, "X").unwrap();
        let b = font_metas(&woff, "X").unwrap();
        assert_eq!(a, b);
    }

    /// A Brotli stream of stored (uncompressed) meta-blocks, which every
    /// decoder must accept: a one-bit window header, then per block ISLAST=0,
    /// four nibbles of length - 1, ISUNCOMPRESSED=1, padding and the bytes,
    /// then an empty last block.
    fn brotli_stored(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut first = true;
        for chunk in data.chunks(65536) {
            let len = (chunk.len() - 1) as u32;
            // Bits, least significant first.
            let (bits, n) = if first { ((len << 4) | (1 << 20), 21) } else { ((len << 3) | (1 << 19), 20) };
            first = false;
            for i in 0..(n as usize).div_ceil(8) {
                out.push((bits >> (8 * i)) as u8);
            }
            out.extend_from_slice(chunk);
        }
        // ISLAST=1, ISLASTEMPTY=1 (after the window bit if nothing came first).
        out.push(if first { 0b110 } else { 0b11 });
        out
    }

    fn base128_bytes(mut v: u32) -> Vec<u8> {
        let mut groups = vec![(v & 0x7F) as u8];
        v >>= 7;
        while v > 0 {
            groups.push((v & 0x7F) as u8 | 0x80);
            v >>= 7;
        }
        groups.reverse();
        groups
    }

    /// Wraps a font's tables in WOFF 2. `glyf` and `loca` are stored either
    /// plainly (transform 3) or as if transformed (version 0, with junk data
    /// the reader must skip).
    fn woff2(data: &[u8], transform_glyf: bool) -> Vec<u8> {
        let face = ttf_parser::RawFace::parse(data, 0).unwrap();
        let mut dir = Vec::new();
        let mut stream = Vec::new();
        for r in face.table_records {
            let raw = &data[r.offset as usize..(r.offset + r.length) as usize];
            let tag = r.tag.to_bytes();
            let index = [b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name"].iter().position(|t| **t == tag);
            let glyf_or_loca = &tag == b"glyf" || &tag == b"loca";
            let version: u8 = if glyf_or_loca && !transform_glyf { 3 } else { 0 };
            dir.push((version << 6) | index.map_or(63, |i| i as u8));
            if index.is_none() {
                dir.extend_from_slice(&tag);
            }
            dir.extend(base128_bytes(r.length));
            if glyf_or_loca && transform_glyf {
                let junk = if &tag == b"glyf" { vec![7u8; 100] } else { Vec::new() };
                dir.extend(base128_bytes(junk.len() as u32));
                stream.extend(junk);
            } else {
                stream.extend_from_slice(raw);
            }
        }
        let comp = brotli_stored(&stream);
        let mut out = b"wOF2".to_vec();
        out.extend_from_slice(&data[..4]);
        out.extend_from_slice(&0u32.to_be_bytes()); // length, patched below
        out.extend_from_slice(&face.table_records.len().to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(&(comp.len() as u32).to_be_bytes());
        out.resize(48, 0);
        out.extend(dir);
        out.extend(comp);
        let n = out.len() as u32;
        out[8..12].copy_from_slice(&n.to_be_bytes());
        out
    }

    #[test]
    fn base128_reads_the_spec_examples() {
        let read = |b: &[u8]| base128(b, &mut 0);
        assert_eq!(read(&[0x3F]), Some(63));
        assert_eq!(read(&[0x81, 0x00]), Some(128));
        assert_eq!(read(&base128_bytes(1_234_567)), Some(1_234_567));
        assert_eq!(read(&[0x80, 0x01]), None, "leading zero group");
        assert_eq!(read(&[0xFF; 6]), None, "too long");
    }

    #[test]
    fn woff2_names_match_the_plain_font() {
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") else { return };
        let plain = font_metas(&data, "X").unwrap();
        for transform in [false, true] {
            let packed = woff2(&data, transform);
            assert_eq!(font_metas(&packed, "X").as_ref(), Some(&plain), "transformed glyf: {transform}");
        }
        // A broken stream is refused rather than misread.
        let mut bad = woff2(&data, false);
        let len = bad.len();
        bad.truncate(len - 1000);
        assert!(font_metas(&bad, "X").is_none());
    }

    #[test]
    fn woff2_imports_with_font_metas() {
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") else { return };
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = crate::Library::create(tmp.path().join("F.library")).unwrap();
        let src = tmp.path().join("Arial.woff2");
        std::fs::write(&src, woff2(&data, false)).unwrap();
        let id = lib.import_file(&src, &crate::import::ImportOptions::default()).unwrap();
        let item = lib.item(&id).unwrap();
        let metas = item.extra.get("fontMetas").and_then(Value::as_object).unwrap();
        assert_eq!(first_value(metas, "fontFamily"), Some("Arial"));
        assert!(item.tags.iter().any(|t| t == "Regular"), "{:?}", item.tags);
        assert_eq!((item.width, item.height), (600, 600), "the specimen was drawn");
        // Packed glyphs: names still read, but there is nothing to draw.
        let packed = woff2(&data, true);
        let m = font_metas(&packed, "X").unwrap();
        assert!(crate::fontpreview::render(&packed, &m).is_none());
    }
}
