//! Reading a Pixave library (`.pxvlibrary`) for import.
//!
//! Pixave (LittleHJ, discontinued) keeps each library in a macOS package
//! whose name ends in `.pxvlibrary`, and each imported file in a
//! `.pxvitem` package inside it. That much is public; the layout of the
//! library's database is not (see `docs/src/library-format.md`, "Pixave
//! libraries", for the sources). So this module does two things only:
//!
//! - [`probe`] walks the package without writing to it: it finds every
//!   SQLite database (by its file header), reads its tables, columns and
//!   row counts through a read-only, immutable connection, and lists the
//!   `.pxvitem` packages.
//! - [`Probe::files`] gives the item files that can be told apart without
//!   knowing the schema: a `.pxvitem` folder holding exactly one regular
//!   file. Everything else is counted, not guessed at.
//!
//! [`report`] turns a probe into plain text (table and column names and
//! counts, never row contents), so the database layout can be documented
//! from a real library and folders, tags, ratings, labels, notes and links
//! mapped later.

use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::error::{Error, Result};

/// The library package's extension.
pub const EXTENSION: &str = "pxvlibrary";
/// Each item's package extension.
pub const ITEM_EXTENSION: &str = "pxvitem";

/// The first 16 bytes of every SQLite 3 database file.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
/// How deep the walk goes; Pixave's package is shallow.
const MAX_DEPTH: usize = 16;

/// What a probe found in a `.pxvlibrary` package.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Probe {
    /// The package.
    pub root: PathBuf,
    /// Every SQLite database in it, outside the item packages.
    pub databases: Vec<Database>,
    /// Every `.pxvitem` package, with the files in it.
    pub items: Vec<PxvItem>,
    /// `.pxvitem` entries that are plain files, not folders.
    pub plain_items: usize,
}

/// One SQLite database in the package.
#[derive(Debug, Clone, PartialEq)]
pub struct Database {
    /// Path relative to the package.
    pub path: PathBuf,
    /// Whether a `-wal` file sits beside it: changes Pixave hadn't folded
    /// into the database yet, which the immutable read doesn't see.
    pub wal: bool,
    /// Its tables, or why they couldn't be read.
    pub tables: std::result::Result<Vec<Table>, String>,
}

/// One table of a database.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    /// Column names with their declared types.
    pub columns: Vec<(String, String)>,
    /// The number of rows, or `None` when counting failed.
    pub rows: Option<u64>,
}

/// One `.pxvitem` package.
#[derive(Debug, Clone, PartialEq)]
pub struct PxvItem {
    pub package: PathBuf,
    /// The regular files inside, at any depth (hidden files and links
    /// left out), sorted.
    pub files: Vec<PathBuf>,
}

impl Probe {
    /// The files that can be imported: those of `.pxvitem` folders holding
    /// exactly one file whose extension the importer takes.
    pub fn files(&self) -> Vec<PathBuf> {
        self.items.iter().filter_map(PxvItem::single_file).cloned().collect()
    }

    /// `.pxvitem` entries left out by [`Probe::files`]: plain files, and
    /// folders with no file or several.
    pub fn left_out(&self) -> usize {
        self.plain_items + self.items.iter().filter(|i| i.single_file().is_none()).count()
    }
}

impl PxvItem {
    fn single_file(&self) -> Option<&PathBuf> {
        match self.files.as_slice() {
            [one] if crate::import::is_recognized(&ext_of(one)) => Some(one),
            _ => None,
        }
    }
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn hidden(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

fn refuse(path: &Path, reason: impl Into<String>) -> Error {
    Error::Pixave { path: path.to_path_buf(), reason: reason.into() }
}

/// Whether `path` is a folder named `*.pxvlibrary`.
pub fn is_library(path: &Path) -> bool {
    has_ext(path, EXTENSION) && path.is_dir()
}

/// Walks the package at `root`, reading only. Refuses a path that isn't a
/// `.pxvlibrary` folder, and one with neither a database nor an item.
pub fn probe(root: &Path) -> Result<Probe> {
    if !has_ext(root, EXTENSION) {
        return Err(refuse(root, format!("not a Pixave library: the folder name must end in .{EXTENSION}")));
    }
    let meta = fs::metadata(root).map_err(|e| Error::io(root, e))?;
    if !meta.is_dir() {
        return Err(refuse(root, "not a Pixave library: it is a file, not a folder"));
    }
    let mut out = Probe { root: root.to_path_buf(), ..Default::default() };
    let mut dbs = Vec::new();
    walk(root, 0, &mut |path, is_dir| {
        if has_ext(path, ITEM_EXTENSION) {
            if is_dir {
                let mut files = Vec::new();
                collect_files(path, 0, &mut files);
                files.sort();
                out.items.push(PxvItem { package: path.to_path_buf(), files });
            } else {
                out.plain_items += 1;
            }
            return false;
        }
        if !is_dir && is_sqlite(path) {
            dbs.push(path.to_path_buf());
        }
        true
    })?;
    if dbs.is_empty() && out.items.is_empty() && out.plain_items == 0 {
        return Err(refuse(root, "no Pixave database or items were found in the package"));
    }
    dbs.sort();
    out.items.sort_by(|a, b| a.package.cmp(&b.package));
    for db in dbs {
        let mut wal = db.clone().into_os_string();
        wal.push("-wal");
        out.databases.push(Database {
            path: db.strip_prefix(root).unwrap_or(&db).to_path_buf(),
            wal: Path::new(&wal).exists(),
            tables: read_schema(&db).map_err(|e| e.to_string()),
        });
    }
    Ok(out)
}

/// Visits every entry under `dir` (links not followed); `visit` returns
/// whether to go into a folder.
fn walk(dir: &Path, depth: usize, visit: &mut dyn FnMut(&Path, bool) -> bool) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|e| Error::io(dir, e))?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        if visit(&path, kind.is_dir()) && kind.is_dir() {
            // An unreadable subfolder is skipped, not fatal.
            let _ = walk(&path, depth + 1, visit);
        }
    }
    Ok(())
}

fn collect_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let _ = walk(dir, depth, &mut |p, is_dir| {
        if hidden(p) {
            return false;
        }
        if !is_dir {
            out.push(p.to_path_buf());
        }
        true
    });
}

fn is_sqlite(path: &Path) -> bool {
    let mut head = [0u8; 16];
    fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == SQLITE_HEADER
}

/// An SQLite URI for `path`, read-only and immutable: SQLite then takes no
/// locks and writes nothing beside the file (no `-shm`, no `-journal`),
/// even while Pixave has it open.
fn immutable_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            uri.push(b as char);
        } else {
            let _ = write!(uri, "%{b:02X}");
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    uri
}

/// Opens `path` read-only and immutable.
pub fn open_read_only(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
    Connection::open_with_flags(immutable_uri(path), flags).map_err(|e| refuse(path, format!("can't open the database: {e}")))
}

/// The tables of the database at `path`, with their columns and row
/// counts. A file that isn't a readable database, or has no tables, is an
/// error.
pub fn read_schema(path: &Path) -> Result<Vec<Table>> {
    let conn = open_read_only(path)?;
    let bad = |e: rusqlite::Error| refuse(path, format!("can't read the database: {e}"));
    let names: Vec<String> = {
        let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").map_err(bad)?;
        let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(bad)?;
        rows.collect::<std::result::Result<_, _>>().map_err(bad)?
    };
    if names.is_empty() {
        return Err(refuse(path, "the database has no tables"));
    }
    let mut tables = Vec::new();
    for name in names {
        let quoted = format!("\"{}\"", name.replace('"', "\"\""));
        let mut st = conn.prepare(&format!("PRAGMA table_info({quoted})")).map_err(bad)?;
        let columns = st
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default())))
            .map_err(bad)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(bad)?;
        let rows = conn.query_row(&format!("SELECT count(*) FROM {quoted}"), [], |r| r.get::<_, i64>(0)).ok().and_then(|n| u64::try_from(n).ok());
        tables.push(Table { name, columns, rows });
    }
    Ok(tables)
}

/// The probe as plain text: package name, item counts, and for each
/// database its tables with columns and row counts. No row contents.
pub fn report(p: &Probe) -> String {
    let mut s = String::new();
    let name = p.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let _ = writeln!(s, "Pixave library: {name}");
    let _ = writeln!(s, "Item packages (.{ITEM_EXTENSION} folders): {}", p.items.len());
    let _ = writeln!(s, "Items that are plain files: {}", p.plain_items);
    let _ = writeln!(s, "Items with exactly one file: {}", p.files().len());
    let mut shapes: std::collections::BTreeMap<String, usize> = Default::default();
    for item in &p.items {
        let mut exts: Vec<String> = item.files.iter().map(|f| ext_of(f)).map(|e| if e.is_empty() { "(none)".into() } else { e }).collect();
        exts.sort();
        *shapes.entry(if exts.is_empty() { "(empty)".into() } else { exts.join(" + ") }).or_default() += 1;
    }
    if !shapes.is_empty() {
        let _ = writeln!(s, "Files per item package, by extension:");
        for (shape, n) in &shapes {
            let _ = writeln!(s, "  {shape}: {n}");
        }
    }
    let _ = writeln!(s, "Databases: {}", p.databases.len());
    for db in &p.databases {
        let _ = writeln!(s, "\n{}{}", db.path.display(), if db.wal { " (has a -wal file; quit Pixave for a complete read)" } else { "" });
        match &db.tables {
            Err(e) => {
                let _ = writeln!(s, "  error: {e}");
            }
            Ok(tables) => {
                for t in tables {
                    let rows = t.rows.map_or_else(|| "? rows".to_string(), |n| format!("{n} rows"));
                    let _ = writeln!(s, "  {} ({rows})", t.name);
                    for (c, ty) in &t.columns {
                        let _ = writeln!(s, "    {c} {ty}");
                    }
                }
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_tree(root: &Path) -> Vec<(PathBuf, blake3::Hash)> {
        let mut out = Vec::new();
        let _ = walk(root, 0, &mut |p, is_dir| {
            if !is_dir {
                out.push((p.to_path_buf(), blake3::hash(&fs::read(p).unwrap())));
            }
            true
        });
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// A Pixave-shaped package: a database at the root and three items,
    /// one with a single image, one with two files, one empty.
    fn package(dir: &Path) -> PathBuf {
        let root = dir.join("My Pics.pxvlibrary");
        fs::create_dir_all(root.join("Items/A1.pxvitem")).unwrap();
        fs::create_dir_all(root.join("Items/B2.pxvitem")).unwrap();
        fs::create_dir_all(root.join("Items/C3.pxvitem")).unwrap();
        fs::write(root.join("Items/A1.pxvitem/photo.png"), b"png").unwrap();
        fs::write(root.join("Items/A1.pxvitem/.DS_Store"), b"x").unwrap();
        fs::write(root.join("Items/B2.pxvitem/a.jpg"), b"a").unwrap();
        fs::write(root.join("Items/B2.pxvitem/b.jpg"), b"b").unwrap();
        fs::write(root.join("Items/D4.pxvitem"), b"plain").unwrap();
        let conn = Connection::open(root.join("library.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE images (id INTEGER PRIMARY KEY, name TEXT, rating INTEGER);
             CREATE TABLE \"odd \"\"name\"\"\" (x);
             INSERT INTO images (name, rating) VALUES ('secret-name', 5), ('b', 3);",
        )
        .unwrap();
        drop(conn);
        fs::write(root.join("notes.txt"), b"SQLite format 2 nope").unwrap();
        root
    }

    #[test]
    fn probes_a_package() {
        let d = tempfile::tempdir().unwrap();
        let root = package(d.path());
        let before = hash_tree(&root);
        let p = probe(&root).unwrap();
        assert_eq!(hash_tree(&root), before, "the package is left untouched");
        assert!(!root.join("library.db-shm").exists() && !root.join("library.db-journal").exists());

        assert_eq!(p.items.len(), 3);
        assert_eq!(p.plain_items, 1);
        assert_eq!(p.files(), vec![root.join("Items/A1.pxvitem/photo.png")]);
        assert_eq!(p.left_out(), 3);
        assert_eq!(p.databases.len(), 1);
        let db = &p.databases[0];
        assert_eq!((db.path.as_path(), db.wal), (Path::new("library.db"), false));
        let tables = db.tables.as_ref().unwrap();
        assert_eq!(tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["images", "odd \"name\""]);
        assert_eq!(tables[0].rows, Some(2));
        assert_eq!(tables[0].columns[2], ("rating".to_string(), "INTEGER".to_string()));

        let text = report(&p);
        assert!(text.contains("images (2 rows)") && text.contains("    rating INTEGER"), "{text}");
        assert!(text.contains("  jpg + jpg: 1") && text.contains("  (empty): 1"), "{text}");
        assert!(!text.contains("secret-name"), "row contents stay out of the report");
    }

    #[test]
    fn refuses_what_is_not_a_library() {
        let d = tempfile::tempdir().unwrap();
        let plain = d.path().join("Pics");
        fs::create_dir_all(&plain).unwrap();
        let e = probe(&plain).unwrap_err().to_string();
        assert!(e.contains(".pxvlibrary"), "{e}");
        let empty = d.path().join("Empty.pxvlibrary");
        fs::create_dir_all(&empty).unwrap();
        assert!(probe(&empty).unwrap_err().to_string().contains("no Pixave database"));
        let file = d.path().join("File.pxvlibrary");
        fs::write(&file, b"x").unwrap();
        assert!(probe(&file).is_err());
        assert!(probe(&d.path().join("Gone.pxvlibrary")).is_err());
        assert!(!is_library(&plain) && !is_library(&file) && is_library(&empty));
    }

    #[test]
    fn a_damaged_or_empty_database_is_reported_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Bad.pxvlibrary");
        fs::create_dir_all(&root).unwrap();
        // A valid header followed by garbage.
        let mut bytes = SQLITE_HEADER.to_vec();
        bytes.extend(std::iter::repeat_n(0xAB, 4096));
        fs::write(root.join("broken.db"), &bytes).unwrap();
        Connection::open(root.join("empty.db")).unwrap().execute_batch("CREATE TABLE t (x); DROP TABLE t;").unwrap();
        let before = hash_tree(&root);
        let p = probe(&root).unwrap();
        assert_eq!(hash_tree(&root), before);
        assert_eq!(p.databases.len(), 2);
        let broken = p.databases.iter().find(|db| db.path == Path::new("broken.db")).unwrap();
        assert!(broken.tables.as_ref().unwrap_err().contains("can't"), "{:?}", broken.tables);
        let empty = p.databases.iter().find(|db| db.path == Path::new("empty.db")).unwrap();
        assert!(empty.tables.as_ref().unwrap_err().contains("no tables"));
        assert!(report(&p).contains("error:"));
    }

    #[test]
    fn reads_a_database_another_connection_holds_locked() {
        let d = tempfile::tempdir().unwrap();
        let root = package(d.path());
        let db = root.join("library.db");
        let holder = Connection::open(&db).unwrap();
        holder.execute_batch("BEGIN EXCLUSIVE; INSERT INTO images (name) VALUES ('c');").unwrap();
        let tables = read_schema(&db).unwrap();
        assert_eq!(tables[0].name, "images");
        holder.execute_batch("ROLLBACK;").unwrap();
    }

    #[test]
    fn uri_escapes_the_path() {
        assert_eq!(immutable_uri(Path::new("/a b/c?#%.db")), "file:/a%20b/c%3F%23%25.db?mode=ro&immutable=1");
    }
}
