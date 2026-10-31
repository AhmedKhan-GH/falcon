//! A library with a damaged item record still opens.

use falcon_library::Library;

#[test]
fn a_zeroed_item_record_is_skipped_and_copied_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("T.library");
    Library::create(&root).unwrap();
    let bad = root.join("images/MAF8IT0E64JOT.info");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("metadata.json"), [0u8; 604]).unwrap();
    std::fs::write(bad.join("book.pdf"), b"pdf").unwrap();

    let lib = Library::open(&root).unwrap();
    assert_eq!(lib.items().len(), 0);
    assert_eq!(lib.skipped.len(), 1);
    assert_eq!(lib.skipped[0].0, bad);

    let copy = dir.path().join("Copy.library");
    lib.save_to(&copy).unwrap();
    let copied = copy.join("images/MAF8IT0E64JOT.info");
    assert_eq!(std::fs::read(copied.join("metadata.json")).unwrap(), vec![0u8; 604]);
    assert_eq!(std::fs::read(copied.join("book.pdf")).unwrap(), b"pdf");
    // The damaged original is never written.
    assert_eq!(std::fs::read(bad.join("metadata.json")).unwrap(), vec![0u8; 604]);
}
