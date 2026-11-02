//! Blank documents falcon writes itself, for File ▸ New….
//!
//! Each [`Template`] builds the bytes of a minimal, valid empty file in
//! code: a text file, the Office Open XML formats (Word, Excel, PowerPoint),
//! an XMind mind map, a Photoshop document (from Adobe's public PSD file
//! format specification) and a PDF-compatible Illustrator document (a plain
//! PDF with one blank page). Nothing is copied from another application's
//! template files. The caller writes the bytes to a temporary file named
//! [`Template::file_name`] and imports it like any other file.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

/// A blank document falcon can make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Text,
    Word,
    Excel,
    PowerPoint,
    XMind,
    Psd,
    Ai,
}

impl Template {
    /// Every template.
    pub const ALL: [Template; 7] = [Template::Text, Template::Word, Template::Excel, Template::PowerPoint, Template::XMind, Template::Psd, Template::Ai];

    /// The file extension.
    pub fn ext(self) -> &'static str {
        match self {
            Template::Text => "txt",
            Template::Word => "docx",
            Template::Excel => "xlsx",
            Template::PowerPoint => "pptx",
            Template::XMind => "xmind",
            Template::Psd => "psd",
            Template::Ai => "ai",
        }
    }

    /// The name the new file is imported under: "Untitled" plus the extension.
    pub fn file_name(self) -> String {
        format!("Untitled.{}", self.ext())
    }

    /// The bytes of the blank document.
    pub fn bytes(self) -> Vec<u8> {
        match self {
            Template::Text => Vec::new(),
            Template::Word => zip_of(&docx_parts()),
            Template::Excel => zip_of(&xlsx_parts()),
            Template::PowerPoint => zip_of(&pptx_parts()),
            Template::XMind => zip_of(&xmind_parts()),
            Template::Psd => psd_bytes(),
            Template::Ai => ai_bytes(),
        }
    }
}

fn zip_of(parts: &[(&str, String)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in parts {
        // Writing into memory cannot fail.
        zip.start_file(*name, opts).expect("zip entry");
        zip.write_all(body.as_bytes()).expect("zip data");
    }
    zip.finish().expect("zip end").into_inner()
}

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_DOC: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

fn root_rels(target: &str) -> String {
    format!(r#"{XML}<Relationships xmlns="{NS_REL}"><Relationship Id="rId1" Type="{REL_DOC}" Target="{target}"/></Relationships>"#)
}

fn docx_parts() -> Vec<(&'static str, String)> {
    vec![
        (
            "[Content_Types].xml",
            format!(
                r#"{XML}<Types xmlns="{NS_CT}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#
            ),
        ),
        ("_rels/.rels", root_rels("word/document.xml")),
        (
            "word/document.xml",
            format!(
                r#"{XML}<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/><w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#
            ),
        ),
    ]
}

fn xlsx_parts() -> Vec<(&'static str, String)> {
    vec![
        (
            "[Content_Types].xml",
            format!(
                r#"{XML}<Types xmlns="{NS_CT}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#
            ),
        ),
        ("_rels/.rels", root_rels("xl/workbook.xml")),
        (
            "xl/workbook.xml",
            format!(r#"{XML}<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="{NS_R}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            format!(r#"{XML}<Relationships xmlns="{NS_REL}"><Relationship Id="rId1" Type="{NS_R}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
        ),
        ("xl/worksheets/sheet1.xml", format!(r#"{XML}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#)),
    ]
}

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

fn pptx_theme() -> String {
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let line = r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;
    let colour = |tag: &str, v: &str| format!(r#"<a:{tag}><a:srgbClr val="{v}"/></a:{tag}>"#);
    let colours: String = [
        ("dk2", "44546A"),
        ("lt2", "E7E6E6"),
        ("accent1", "4472C4"),
        ("accent2", "ED7D31"),
        ("accent3", "A5A5A5"),
        ("accent4", "FFC000"),
        ("accent5", "5B9BD5"),
        ("accent6", "70AD47"),
        ("hlink", "0563C1"),
        ("folHlink", "954F72"),
    ]
    .iter()
    .map(|(t, v)| colour(t, v))
    .collect();
    format!(
        r#"{XML}<a:theme xmlns:a="{NS_A}" name="Falcon"><a:themeElements><a:clrScheme name="Falcon"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1>{colours}</a:clrScheme><a:fontScheme name="Falcon"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Falcon"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst><a:lnStyleLst>{line}{line}{line}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

fn pptx_parts() -> Vec<(&'static str, String)> {
    let ct = "application/vnd.openxmlformats-officedocument.presentationml";
    let tree = r#"<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld>"#;
    let rel = |id: &str, kind: &str, target: &str| format!(r#"<Relationship Id="{id}" Type="{NS_R}/{kind}" Target="{target}"/>"#);
    vec![
        (
            "[Content_Types].xml",
            format!(
                r#"{XML}<Types xmlns="{NS_CT}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="{ct}.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="{ct}.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="{ct}.slideLayout+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="{ct}.slide+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>"#
            ),
        ),
        ("_rels/.rels", root_rels("ppt/presentation.xml")),
        (
            "ppt/presentation.xml",
            format!(
                r#"{XML}<p:presentation xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:p="{NS_P}"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
            ),
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            format!(r#"{XML}<Relationships xmlns="{NS_REL}">{}{}{}</Relationships>"#, rel("rId1", "slideMaster", "slideMasters/slideMaster1.xml"), rel("rId2", "slide", "slides/slide1.xml"), rel("rId3", "theme", "theme/theme1.xml")),
        ),
        (
            "ppt/slideMasters/slideMaster1.xml",
            format!(
                r#"{XML}<p:sldMaster xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:p="{NS_P}">{tree}<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
            ),
        ),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            format!(r#"{XML}<Relationships xmlns="{NS_REL}">{}{}</Relationships>"#, rel("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), rel("rId2", "theme", "../theme/theme1.xml")),
        ),
        ("ppt/slideLayouts/slideLayout1.xml", format!(r#"{XML}<p:sldLayout xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:p="{NS_P}" type="blank">{tree}</p:sldLayout>"#)),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            format!(r#"{XML}<Relationships xmlns="{NS_REL}">{}</Relationships>"#, rel("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")),
        ),
        ("ppt/slides/slide1.xml", format!(r#"{XML}<p:sld xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:p="{NS_P}">{tree}</p:sld>"#)),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            format!(r#"{XML}<Relationships xmlns="{NS_REL}">{}</Relationships>"#, rel("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")),
        ),
        ("ppt/theme/theme1.xml", pptx_theme()),
    ]
}

/// XMind's current format (XMind 8 "Zen" and later): `content.json` is a
/// list of sheets, each with a root topic.
fn xmind_parts() -> Vec<(&'static str, String)> {
    let id = || (0..26).map(|_| char::from(b"0123456789abcdefghijklmnopqrstuv"[fastrand::usize(..32)])).collect::<String>();
    let content = serde_json::json!([{
        "id": id(),
        "revisionId": id(),
        "class": "sheet",
        "title": "Map 1",
        "rootTopic": {
            "id": id(),
            "class": "topic",
            "title": "Central Topic",
            "structureClass": "org.xmind.ui.map.unbalanced"
        }
    }]);
    let metadata = serde_json::json!({"creator": {"name": "falcon", "version": env!("CARGO_PKG_VERSION")}});
    let manifest = serde_json::json!({"file-entries": {"content.json": {}, "metadata.json": {}}});
    vec![("content.json", content.to_string()), ("metadata.json", metadata.to_string()), ("manifest.json", manifest.to_string())]
}

/// Size of the blank Photoshop canvas, in pixels.
const PSD_SIZE: (u32, u32) = (1920, 1080);
/// Page size of the blank Illustrator document, in points.
const AI_PAGE: (u32, u32) = (1366, 768);

/// PackBits (the PSD "RLE" scheme) of a row of `n` white bytes: runs of at
/// most 128 copies, each a count byte `257 - run` and the value.
fn white_row_rle(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut left = n;
    while left > 0 {
        let run = left.min(128);
        if run == 1 {
            out.extend([0, 0xFF]);
        } else {
            out.extend([(257 - run) as u8, 0xFF]);
        }
        left -= run;
    }
    out
}

/// A white, opaque, 8-bit RGB Photoshop document with no layers, laid out
/// as the spec's five sections: header, colour mode data (empty), image
/// resources (the resolution, 72 dpi), layer and mask information (no
/// layers, no global mask) and the merged image, RLE compressed.
fn psd_bytes() -> Vec<u8> {
    let (w, h) = PSD_SIZE;
    let mut out = Vec::new();
    out.extend(b"8BPS");
    out.extend(1u16.to_be_bytes()); // version
    out.extend([0u8; 6]); // reserved
    out.extend(3u16.to_be_bytes()); // channels
    out.extend(h.to_be_bytes());
    out.extend(w.to_be_bytes());
    out.extend(8u16.to_be_bytes()); // depth
    out.extend(3u16.to_be_bytes()); // colour mode: RGB
    out.extend(0u32.to_be_bytes()); // colour mode data
    // Resolution info (resource 0x03ED), 16 bytes: 72.00 dpi as 16.16 fixed
    // point, in inches, both ways.
    let mut res = Vec::new();
    res.extend(b"8BIM");
    res.extend(0x03EDu16.to_be_bytes());
    res.extend([0u8, 0]); // empty pascal name, padded to even
    res.extend(16u32.to_be_bytes());
    for _ in 0..2 {
        res.extend(0x0048_0000u32.to_be_bytes());
        res.extend(1u16.to_be_bytes()); // inches
        res.extend(1u16.to_be_bytes()); // inches
    }
    out.extend((res.len() as u32).to_be_bytes());
    out.extend(res);
    // Layer and mask information: layer info length 0, global mask length 0.
    out.extend(8u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    // Image data: RLE, a table of byte counts for every row of every
    // channel, then the rows channel by channel.
    out.extend(1u16.to_be_bytes());
    let row = white_row_rle(w as usize);
    for _ in 0..3 * h {
        out.extend((row.len() as u16).to_be_bytes());
    }
    for _ in 0..3 * h {
        out.extend(&row);
    }
    out
}

/// A one-page PDF 1.5 file: Illustrator opens a PDF saved with an `.ai`
/// extension, and falcon reads PDF-compatible Illustrator files by their
/// `%PDF` header.
fn ai_bytes() -> Vec<u8> {
    let (w, h) = AI_PAGE;
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << >> /Contents 4 0 R >>"),
        "<< /Length 0 >>\nstream\n\nendstream".to_string(),
    ];
    let mut out = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn open(t: Template) -> zip::ZipArchive<Cursor<Vec<u8>>> {
        zip::ZipArchive::new(Cursor::new(t.bytes())).expect("a valid zip")
    }

    fn entry(z: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str) -> String {
        let mut s = String::new();
        z.by_name(name).unwrap_or_else(|_| panic!("missing {name}")).read_to_string(&mut s).unwrap();
        s
    }

    /// Every part parses as XML, and every relationship target and content
    /// type override points at a part that exists.
    fn check_package(t: Template, required: &[&str]) {
        let mut z = open(t);
        let names: Vec<String> = (0..z.len()).map(|i| z.name_for_index(i).unwrap().unwrap().to_string()).collect();
        assert_eq!(names[0], "[Content_Types].xml");
        for r in required {
            assert!(names.iter().any(|n| n == r), "{t:?} lacks {r}");
        }
        for n in &names {
            let xml = entry(&mut z, n);
            let doc = roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{n}: {e}"));
            if let Some(base) = n.strip_suffix(".rels") {
                // `dir/_rels/part.xml.rels` describes `dir/part.xml`.
                let dir = base.rsplit_once("_rels/").map(|(d, _)| d).unwrap_or("");
                for rel in doc.descendants().filter(|e| e.has_tag_name("Relationship")) {
                    let target = rel.attribute("Target").unwrap();
                    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
                    for seg in target.split('/') {
                        if seg == ".." {
                            parts.pop();
                        } else {
                            parts.push(seg);
                        }
                    }
                    assert!(names.contains(&parts.join("/")), "{n} points at missing {target}");
                }
            }
            if n == "[Content_Types].xml" {
                for o in doc.descendants().filter(|e| e.has_tag_name("Override")) {
                    let part = o.attribute("PartName").unwrap().trim_start_matches('/');
                    assert!(names.iter().any(|x| x == part), "override for missing {part}");
                }
            }
        }
    }

    #[test]
    fn names_are_untitled_with_the_extension() {
        let names: Vec<String> = Template::ALL.iter().map(|t| t.file_name()).collect();
        assert_eq!(names, ["Untitled.txt", "Untitled.docx", "Untitled.xlsx", "Untitled.pptx", "Untitled.xmind", "Untitled.psd", "Untitled.ai"]);
    }

    #[test]
    fn text_is_empty() {
        assert!(Template::Text.bytes().is_empty());
    }

    #[test]
    fn docx_is_a_valid_package() {
        check_package(Template::Word, &["_rels/.rels", "word/document.xml"]);
    }

    #[test]
    fn xlsx_is_a_valid_package() {
        check_package(Template::Excel, &["_rels/.rels", "xl/workbook.xml", "xl/worksheets/sheet1.xml"]);
    }

    #[test]
    fn pptx_is_a_valid_package() {
        check_package(Template::PowerPoint, &["ppt/presentation.xml", "ppt/slideMasters/slideMaster1.xml", "ppt/slideLayouts/slideLayout1.xml", "ppt/slides/slide1.xml", "ppt/theme/theme1.xml"]);
    }

    #[test]
    fn xmind_has_a_sheet_with_a_root_topic() {
        let mut z = open(Template::XMind);
        let content: serde_json::Value = serde_json::from_str(&entry(&mut z, "content.json")).unwrap();
        let sheet = &content[0];
        assert_eq!(sheet["class"], "sheet");
        assert_eq!(sheet["rootTopic"]["class"], "topic");
        assert!(sheet["id"].as_str().is_some_and(|s| !s.is_empty()));
        let meta: serde_json::Value = serde_json::from_str(&entry(&mut z, "metadata.json")).unwrap();
        assert_eq!(meta["creator"]["name"], "falcon");
        let manifest: serde_json::Value = serde_json::from_str(&entry(&mut z, "manifest.json")).unwrap();
        assert!(manifest["file-entries"]["content.json"].is_object());
    }

    fn be32(b: &[u8], at: usize) -> usize {
        u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize
    }

    #[test]
    fn psd_sections_are_consistent() {
        let b = Template::Psd.bytes();
        assert_eq!(&b[..4], b"8BPS");
        assert_eq!(&b[4..6], [0, 1]);
        assert_eq!((be32(&b, 14), be32(&b, 18)), (1080, 1920));
        assert_eq!(&b[22..26], [0, 8, 0, 3]);
        // Each section's length leads to the next one, and the last ends the file.
        let mut at = 26;
        let colour = be32(&b, at);
        at += 4 + colour;
        let resources = be32(&b, at);
        assert!(b[at + 4..at + 4 + resources].starts_with(b"8BIM"));
        at += 4 + resources;
        let layers = be32(&b, at);
        at += 4 + layers;
        assert_eq!(&b[at..at + 2], [0, 1], "RLE");
        at += 2;
        let table = 3 * 1080 * 2;
        let data: usize = (0..3 * 1080).map(|i| u16::from_be_bytes([b[at + 2 * i], b[at + 2 * i + 1]]) as usize).sum();
        assert_eq!(at + table + data, b.len());
        assert!(b.len() < 120_000);
    }

    #[test]
    fn psd_decodes_as_white() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Untitled.psd");
        std::fs::write(&file, Template::Psd.bytes()).unwrap();
        assert_eq!(crate::psd::dimensions(&file), Some((1920, 1080)));
        let mut c = crate::psd::Composite::open(&file).unwrap();
        assert_eq!(c.size(), (1920, 1080));
        let img = c.read_rects(&[[0, 0, 1920, 1080]]).unwrap().remove(0);
        assert!(img.pixels().all(|p| p.0 == [255, 255, 255, 255]));
    }

    /// macOS's image reader accepts the blank Photoshop file.
    #[cfg(target_os = "macos")]
    #[test]
    fn sips_reads_the_psd() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Untitled.psd");
        std::fs::write(&file, Template::Psd.bytes()).unwrap();
        let Ok(out) = std::process::Command::new("/usr/bin/sips").args(["-g", "pixelWidth", "-g", "pixelHeight"]).arg(&file).output() else { return };
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("pixelWidth: 1920") && text.contains("pixelHeight: 1080"), "{text}");
    }

    #[test]
    fn ai_is_a_pdf_with_correct_offsets() {
        let b = Template::Ai.bytes();
        assert!(b.starts_with(b"%PDF-"));
        assert!(crate::import::is_recognized("ai"));
        let find = |needle: &[u8]| b.windows(needle.len()).rposition(|w| w == needle).unwrap();
        let tail = String::from_utf8(b[find(b"startxref\n")..].to_vec()).unwrap();
        let start: usize = tail.lines().nth(1).unwrap().parse().unwrap();
        assert!(b[start..].starts_with(b"xref\n0 5\n"));
        let table = String::from_utf8(b[start..find(b"trailer")].to_vec()).unwrap();
        let entries: Vec<&str> = table.lines().skip(2).collect();
        assert_eq!(entries.len(), 5);
        assert!(entries[0].ends_with(" f "));
        for (i, e) in entries.iter().enumerate().skip(1) {
            let off: usize = e[..10].parse().unwrap();
            assert!(e.ends_with(" n "));
            assert!(b[off..].starts_with(format!("{i} 0 obj\n").as_bytes()), "object {i}");
        }
        let text = String::from_utf8_lossy(&b);
        assert!(text.contains("/MediaBox [0 0 1366 768]") && text.contains("/Count 1"));
        assert!(text.trim_end().ends_with("%%EOF"));
    }

    /// macOS's image reader opens the blank document as a PDF page.
    #[cfg(target_os = "macos")]
    #[test]
    fn sips_reads_the_ai_as_a_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Untitled.pdf");
        std::fs::write(&file, Template::Ai.bytes()).unwrap();
        let Ok(out) = std::process::Command::new("/usr/bin/sips").args(["-g", "pixelWidth", "-g", "pixelHeight"]).arg(&file).output() else { return };
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("pixelWidth: 1366") && text.contains("pixelHeight: 768"), "{text}");
    }

    /// macOS's own converter reads the blank Word file.
    #[cfg(target_os = "macos")]
    #[test]
    fn textutil_reads_the_docx() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Untitled.docx");
        std::fs::write(&file, Template::Word.bytes()).unwrap();
        let Ok(out) = std::process::Command::new("/usr/bin/textutil").args(["-convert", "txt", "-stdout"]).arg(&file).output() else { return };
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// They import as the item types the extensions give.
    #[test]
    fn every_template_is_recognised_by_import() {
        for t in Template::ALL {
            assert!(crate::import::is_recognized(t.ext()), "{}", t.ext());
        }
    }
}
