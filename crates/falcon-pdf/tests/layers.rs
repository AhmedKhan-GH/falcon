//! Switching optional content groups: each test writes a page with a red
//! square in the layer "Red" and a blue square in "Blue", in the forms
//! real files come in, and checks the pixels after each switch.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use falcon_pdf::Document;
use lopdf::{EncryptionState, EncryptionVersion, Permissions};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// A one-page PDF with a classic cross-reference table. `properties` is the
/// catalog's /OCProperties value; `extra` more catalog entries. Object 4 is
/// the red group, 5 the blue one and 6 is free for `properties` to refer to.
fn pdf(properties: &str, config: &str, extra: &str) -> Vec<u8> {
    let content = "/OC /A BDC 1 0 0 rg 50 50 100 100 re f EMC /OC /B BDC 0 0 1 rg 250 50 100 100 re f EMC";
    let objs = [
        format!("<< /Type /Catalog /Pages 2 0 R /OCProperties {properties} {extra} >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Resources << /Properties << /A 4 0 R /B 5 0 R >> >> /Contents 7 0 R >>".to_string(),
        "<< /Type /OCG /Name (Red) >>".to_string(),
        "<< /Type /OCG /Name (Blue) >>".to_string(),
        config.to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (k, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", k + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// Both layers on, the configuration in an object of its own.
fn plain() -> Vec<u8> {
    pdf("6 0 R", "<< /OCGs [4 0 R 5 0 R] /D << /Name (Default) >> >>", "")
}

fn write(dir: &Path, bytes: &[u8]) -> PathBuf {
    let path = dir.join("layers.pdf");
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The two squares' colours.
fn squares(doc: &Document) -> ([u8; 4], [u8; 4]) {
    let b = doc.render(0, 1.0, 1000).unwrap();
    (b.pixel(100, 100), b.pixel(300, 100))
}

/// Every combination, then back to the default.
fn switches_all_ways(path: &Path, password: Option<&str>) {
    let mut doc = Document::open_with_password(path, password).unwrap();
    let names: Vec<_> = doc.layers().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Red", "Blue"]);
    assert_eq!(squares(&doc), (RED, BLUE));
    doc.set_layers(&[false, true]).unwrap();
    assert_eq!(squares(&doc), (WHITE, BLUE));
    doc.set_layers(&[true, false]).unwrap();
    assert_eq!(squares(&doc), (RED, WHITE));
    doc.set_layers(&[false, false]).unwrap();
    assert_eq!(squares(&doc), (WHITE, WHITE));
    doc.set_layers(&[true, true]).unwrap();
    assert_eq!(squares(&doc), (RED, BLUE));
}

#[test]
fn a_table_file_switches() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &plain());
    let before = std::fs::read(&path).unwrap();
    switches_all_ways(&path, None);
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file on disk is never written");
}

#[test]
fn switching_changes_only_the_pixels_of_the_layer() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &plain());
    let mut doc = Document::open(&path).unwrap();
    let first = doc.render(0, 1.0, 1000).unwrap();
    doc.set_layers(&[false, true]).unwrap();
    let off = doc.render(0, 1.0, 1000).unwrap();
    assert_ne!(first, off);
    for y in 0..first.height {
        for x in 0..first.width {
            if first.pixel(x, y) != off.pixel(x, y) {
                assert!(x < 200, "only the left side has the red layer, not ({x}, {y})");
            }
        }
    }
    assert_eq!(doc.thumbnail(0, 400).unwrap().pixel(100, 100), WHITE, "thumbnails follow");
    assert!(doc.layers()[0].visible, "the list keeps saying what the default is");
    doc.set_layers(&[true, true]).unwrap();
    assert_eq!(doc.render(0, 1.0, 1000).unwrap(), first, "the default is the original drawing");
}

#[test]
fn a_direct_configuration_in_the_catalog_switches() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &pdf("<< /OCGs [4 0 R 5 0 R] /D << >> >>", "null", "/Lang (en)"));
    switches_all_ways(&path, None);
}

#[test]
fn a_default_of_off_can_be_turned_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &pdf("6 0 R", "<< /OCGs [4 0 R 5 0 R] /D << /BaseState /OFF /ON [5 0 R] >> >>", ""));
    let mut doc = Document::open(&path).unwrap();
    assert_eq!(squares(&doc), (WHITE, BLUE));
    doc.set_layers(&[true, true]).unwrap();
    assert_eq!(squares(&doc), (RED, BLUE));
    doc.set_layers(&[true, false]).unwrap();
    assert_eq!(squares(&doc), (RED, WHITE));
}

#[test]
fn an_automatic_state_doesnt_override_the_choice() {
    let dir = tempfile::tempdir().unwrap();
    let auto = "<< /OCGs [4 0 R 5 0 R] /D << /AS [<< /Event /View /Category [/View] /OCGs [4 0 R 5 0 R] >>] >> >>";
    let path = write(dir.path(), &pdf("6 0 R", auto, ""));
    let mut doc = Document::open(&path).unwrap();
    doc.set_layers(&[false, true]).unwrap();
    assert_eq!(squares(&doc), (WHITE, BLUE));
}

#[test]
fn compressed_objects_and_a_stream_cross_reference_switch() {
    let dir = tempfile::tempdir().unwrap();
    let mut modern = Vec::new();
    lopdf::Document::load_mem(&plain()).unwrap().save_modern(&mut modern).unwrap();
    assert!(modern.windows(6).any(|w| w == b"ObjStm"), "the catalog and groups are in an object stream");
    let path = write(dir.path(), &modern);
    switches_all_ways(&path, None);
}

#[test]
fn bytes_before_the_header_are_allowed_for() {
    let dir = tempfile::tempdir().unwrap();
    let mut shifted = b"junk before the header\n".to_vec();
    shifted.extend_from_slice(&plain());
    let path = write(dir.path(), &shifted);
    switches_all_ways(&path, None);
}

#[test]
fn an_encrypted_file_switches_with_its_password() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = lopdf::Document::load_mem(&plain()).unwrap();
    doc.trailer.set("ID", lopdf::Object::Array(vec![lopdf::Object::string_literal("0123456789abcdef"), lopdf::Object::string_literal("0123456789abcdef")]));
    let version = EncryptionVersion::V2 { document: &doc, owner_password: "owner", user_password: "secret", key_length: 128, permissions: Permissions::all() };
    let state = EncryptionState::try_from(version).unwrap();
    doc.encrypt(&state).unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let path = write(dir.path(), &bytes);
    assert!(Document::open(&path).err().unwrap().needs_password());
    switches_all_ways(&path, Some("secret"));
}

#[test]
fn a_wrong_list_or_no_layers_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &plain());
    let mut doc = Document::open(&path).unwrap();
    assert!(doc.set_layers(&[true]).is_err());
    assert_eq!(squares(&doc), (RED, BLUE));

    let none = pdf("null", "null", "");
    let path = write(dir.path(), &none);
    let mut doc = Document::open(&path).unwrap();
    assert!(doc.layers().is_empty());
    doc.set_layers(&[]).unwrap();
}
