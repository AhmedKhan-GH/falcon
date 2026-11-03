//! One library's embeddings, kept in falcon's app data under a name made
//! from the library's path. Nothing is written inside the `.library` folder,
//! so Eagle-format libraries stay byte-identical.
//!
//! The file is a short header followed by records appended as they are
//! made, so indexing resumes where it stopped. A record cut short by a crash
//! is dropped on load. Later records replace earlier ones; [`Index::compact`]
//! rewrites the file when enough of it is dead.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"FALCONAI";
const VERSION: u32 = 1;
const VECTOR: u8 = 1;
const SKIPPED: u8 = 2;
const REMOVED: u8 = 3;
/// Rewrite the file once this many records are dead, and they outnumber the
/// live ones.
const COMPACT_AFTER: usize = 256;

/// A search result: the item and its cosine similarity to the query.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub score: f32,
}

/// The file an index of the library at `root` lives in.
pub fn path_for(dir: &Path, root: &Path) -> PathBuf {
    let hash = blake3::hash(root.as_os_str().as_encoded_bytes()).to_hex();
    dir.join(format!("{}.index", &hash.as_str()[..32]))
}

pub struct Index {
    path: PathBuf,
    root: String,
    model: String,
    dim: usize,
    ids: Vec<String>,
    stamps: Vec<u64>,
    /// `dim` floats per entry of `ids`.
    vectors: Vec<f32>,
    by_id: HashMap<String, usize>,
    /// Items that have no picture to read, with the stamp they were tried
    /// at, so they aren't tried again until they change.
    skipped: HashMap<String, u64>,
    file: Option<BufWriter<File>>,
    /// Records in the file that a later record replaced.
    dead: usize,
}

impl Index {
    /// Opens the index of the library at `root`, reading what is there. An
    /// index from another model, or one that can't be read, starts over.
    pub fn open(dir: &Path, root: &Path, model: &str, dim: usize) -> io::Result<Index> {
        fs::create_dir_all(dir)?;
        let mut index = Index {
            path: path_for(dir, root),
            root: root.display().to_string(),
            model: model.to_string(),
            dim,
            ids: Vec::new(),
            stamps: Vec::new(),
            vectors: Vec::new(),
            by_id: HashMap::new(),
            skipped: HashMap::new(),
            file: None,
            dead: 0,
        };
        match index.read().unwrap_or(None) {
            Some(end) => {
                // Drop a record cut short, so appending goes on after the
                // last good one.
                OpenOptions::new().write(true).open(&index.path)?.set_len(end)?;
                index.file = Some(BufWriter::new(OpenOptions::new().append(true).open(&index.path)?));
            }
            None => index.start_file()?,
        }
        Ok(index)
    }

    fn header(&self) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(MAGIC);
        h.extend_from_slice(&VERSION.to_le_bytes());
        h.extend_from_slice(&(self.dim as u32).to_le_bytes());
        for text in [&self.model, &self.root] {
            h.extend_from_slice(&(text.len() as u32).to_le_bytes());
            h.extend_from_slice(text.as_bytes());
        }
        h
    }

    fn start_file(&mut self) -> io::Result<()> {
        self.ids.clear();
        self.stamps.clear();
        self.vectors.clear();
        self.by_id.clear();
        self.skipped.clear();
        self.dead = 0;
        let mut file = BufWriter::new(File::create(&self.path)?);
        file.write_all(&self.header())?;
        file.flush()?;
        self.file = Some(file);
        Ok(())
    }

    /// Reads the file; the offset where the good records end, or `None` when
    /// it has to start over.
    fn read(&mut self) -> io::Result<Option<u64>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let mut r = BufReader::new(file);
        let expected = self.header();
        let mut head = vec![0; expected.len()];
        if r.read_exact(&mut head).is_err() || head != expected {
            return Ok(None);
        }
        let mut end = expected.len() as u64;
        let mut vector = vec![0; self.dim * 4];
        while let Some((tag, id, stamp)) = read_record_head(&mut r) {
            match tag {
                VECTOR => {
                    if r.read_exact(&mut vector).is_err() {
                        break;
                    }
                    let floats: Vec<f32> = vector.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
                    self.set(&id, stamp, &floats);
                }
                SKIPPED => self.set_skipped(&id, stamp),
                REMOVED => self.drop_entry(&id),
                _ => break,
            }
            end += 1 + 2 + id.len() as u64 + 8 + if tag == VECTOR { vector.len() as u64 } else { 0 };
        }
        Ok(Some(end))
    }

    fn set(&mut self, id: &str, stamp: u64, vector: &[f32]) {
        if self.skipped.remove(id).is_some() {
            self.dead += 1;
        }
        match self.by_id.get(id) {
            Some(&at) => {
                self.dead += 1;
                self.stamps[at] = stamp;
                self.vectors[at * self.dim..(at + 1) * self.dim].copy_from_slice(vector);
            }
            None => {
                self.by_id.insert(id.to_string(), self.ids.len());
                self.ids.push(id.to_string());
                self.stamps.push(stamp);
                self.vectors.extend_from_slice(vector);
            }
        }
    }

    fn set_skipped(&mut self, id: &str, stamp: u64) {
        self.drop_entry(id);
        if self.skipped.insert(id.to_string(), stamp).is_some() {
            self.dead += 1;
        }
    }

    fn drop_entry(&mut self, id: &str) {
        if self.skipped.remove(id).is_some() {
            self.dead += 2;
        }
        if let Some(at) = self.by_id.remove(id) {
            self.dead += 2;
            self.ids.swap_remove(at);
            self.stamps.swap_remove(at);
            let last = self.ids.len();
            if at != last {
                self.by_id.insert(self.ids[at].clone(), at);
                self.vectors.copy_within(last * self.dim..(last + 1) * self.dim, at * self.dim);
            }
            self.vectors.truncate(last * self.dim);
        }
    }

    fn append(&mut self, tag: u8, id: &str, stamp: u64, vector: Option<&[f32]>) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => write_record(file, tag, id, stamp, vector),
            None => Ok(()),
        }
    }

    /// Stores an item's vector.
    pub fn put(&mut self, id: &str, stamp: u64, vector: &[f32]) -> io::Result<()> {
        if vector.len() != self.dim {
            return Err(io::Error::other("wrong vector length"));
        }
        self.set(id, stamp, vector);
        self.append(VECTOR, id, stamp, Some(vector))
    }

    /// Notes an item with nothing to read.
    pub fn skip(&mut self, id: &str, stamp: u64) -> io::Result<()> {
        self.set_skipped(id, stamp);
        self.append(SKIPPED, id, stamp, None)
    }

    /// Forgets an item.
    pub fn remove(&mut self, id: &str) -> io::Result<()> {
        if !self.by_id.contains_key(id) && !self.skipped.contains_key(id) {
            return Ok(());
        }
        self.drop_entry(id);
        self.append(REMOVED, id, 0, None)
    }

    /// Writes out what was added since the last call.
    pub fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }

    /// The stamp an item was indexed or skipped at.
    pub fn stamp(&self, id: &str) -> Option<u64> {
        self.by_id.get(id).map(|&at| self.stamps[at]).or_else(|| self.skipped.get(id).copied())
    }

    pub fn vector(&self, id: &str) -> Option<&[f32]> {
        self.by_id.get(id).map(|&at| &self.vectors[at * self.dim..(at + 1) * self.dim])
    }

    /// How many items have a vector.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Every item known, with a vector or skipped.
    pub fn known_ids(&self) -> Vec<String> {
        self.ids.iter().chain(self.skipped.keys()).cloned().collect()
    }

    /// The best `limit` items scoring at least `min`, best first. Vectors
    /// are unit length, so the dot product is the cosine similarity.
    pub fn search(&self, query: &[f32], limit: usize, min: f32) -> Vec<Hit> {
        if query.len() != self.dim || limit == 0 || self.dim == 0 {
            return Vec::new();
        }
        let mut scored: Vec<(f32, usize)> = self
            .vectors
            .chunks_exact(self.dim)
            .enumerate()
            .map(|(i, v)| (v.iter().zip(query).map(|(a, b)| a * b).sum::<f32>(), i))
            .filter(|(score, _)| *score >= min)
            .collect();
        let order = |a: &(f32, usize), b: &(f32, usize)| b.0.total_cmp(&a.0).then_with(|| self.ids[a.1].cmp(&self.ids[b.1]));
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit - 1, order);
            scored.truncate(limit);
        }
        scored.sort_unstable_by(order);
        scored.into_iter().map(|(score, i)| Hit { id: self.ids[i].clone(), score }).collect()
    }

    /// Rewrites the file when enough of it is dead.
    pub fn compact(&mut self) -> io::Result<bool> {
        if self.dead < COMPACT_AFTER || self.dead < self.ids.len() + self.skipped.len() {
            return Ok(false);
        }
        self.flush()?;
        self.file = None;
        let temp = self.path.with_extension("index.new");
        {
            let mut out = BufWriter::new(File::create(&temp)?);
            out.write_all(&self.header())?;
            for (i, id) in self.ids.iter().enumerate() {
                write_record(&mut out, VECTOR, id, self.stamps[i], Some(&self.vectors[i * self.dim..(i + 1) * self.dim]))?;
            }
            for (id, stamp) in &self.skipped {
                write_record(&mut out, SKIPPED, id, *stamp, None)?;
            }
            out.flush()?;
        }
        fs::rename(&temp, &self.path)?;
        self.file = Some(BufWriter::new(OpenOptions::new().append(true).open(&self.path)?));
        self.dead = 0;
        Ok(true)
    }

    /// Forgets everything, on disk too.
    pub fn reset(&mut self) -> io::Result<()> {
        self.start_file()
    }
}

fn write_record(out: &mut impl Write, tag: u8, id: &str, stamp: u64, vector: Option<&[f32]>) -> io::Result<()> {
    let Ok(len) = u16::try_from(id.len()) else { return Err(io::Error::other("item id too long")) };
    out.write_all(&[tag])?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(id.as_bytes())?;
    out.write_all(&stamp.to_le_bytes())?;
    for x in vector.unwrap_or_default() {
        out.write_all(&x.to_le_bytes())?;
    }
    Ok(())
}

/// The tag, id and stamp of the next record; `None` at the end or when the
/// record is cut short.
fn read_record_head(r: &mut impl Read) -> Option<(u8, String, u64)> {
    let mut head = [0; 3];
    r.read_exact(&mut head).ok()?;
    let mut id = vec![0; usize::from(u16::from_le_bytes([head[1], head[2]]))];
    let mut stamp = [0; 8];
    r.read_exact(&mut id).ok()?;
    r.read_exact(&mut stamp).ok()?;
    Some((head[0], String::from_utf8(id).ok()?, u64::from_le_bytes(stamp)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/tmp/Books.library";

    fn open(dir: &Path) -> Index {
        Index::open(dir, Path::new(ROOT), "m1", 2).unwrap()
    }

    #[test]
    fn what_was_stored_is_read_back_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut ix = open(dir.path());
        ix.put("a", 1, &[1.0, 0.0]).unwrap();
        ix.put("b", 2, &[0.0, 1.0]).unwrap();
        ix.skip("c", 3).unwrap();
        ix.put("a", 9, &[0.6, 0.8]).unwrap();
        ix.remove("b").unwrap();
        ix.flush().unwrap();
        drop(ix);
        let ix = open(dir.path());
        assert_eq!(ix.len(), 1);
        assert_eq!(ix.stamp("a"), Some(9));
        assert_eq!(ix.vector("a"), Some(&[0.6, 0.8][..]));
        assert_eq!(ix.stamp("b"), None);
        assert_eq!(ix.stamp("c"), Some(3), "skipped items aren't tried again");
    }

    #[test]
    fn a_record_cut_short_is_dropped_and_appending_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut ix = open(dir.path());
        ix.put("a", 1, &[1.0, 0.0]).unwrap();
        ix.put("b", 2, &[0.0, 1.0]).unwrap();
        ix.flush().unwrap();
        let path = ix.path.clone();
        drop(ix);
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();
        let mut ix = open(dir.path());
        assert_eq!(ix.len(), 1);
        assert_eq!(ix.stamp("b"), None);
        ix.put("c", 3, &[0.0, 1.0]).unwrap();
        ix.flush().unwrap();
        drop(ix);
        let ix = open(dir.path());
        assert_eq!(ix.len(), 2);
        assert_eq!(ix.stamp("c"), Some(3));
    }

    #[test]
    fn another_model_or_a_damaged_file_starts_over() {
        let dir = tempfile::tempdir().unwrap();
        let mut ix = open(dir.path());
        ix.put("a", 1, &[1.0, 0.0]).unwrap();
        ix.flush().unwrap();
        let path = ix.path.clone();
        drop(ix);
        let other = Index::open(dir.path(), Path::new(ROOT), "m2", 2).unwrap();
        assert_eq!(other.len(), 0);
        drop(other);
        fs::write(&path, b"not an index at all").unwrap();
        assert_eq!(open(dir.path()).len(), 0);
    }

    #[test]
    fn libraries_have_their_own_files() {
        assert_ne!(path_for(Path::new("/d"), Path::new("/a.library")), path_for(Path::new("/d"), Path::new("/b.library")));
        assert_eq!(path_for(Path::new("/d"), Path::new("/a.library")), path_for(Path::new("/d"), Path::new("/a.library")));
    }

    #[test]
    fn search_ranks_by_cosine_and_cuts_off() {
        let dir = tempfile::tempdir().unwrap();
        let mut ix = open(dir.path());
        ix.put("near", 1, &[1.0, 0.0]).unwrap();
        ix.put("mid", 1, &[0.8, 0.6]).unwrap();
        ix.put("far", 1, &[0.0, 1.0]).unwrap();
        let hits = ix.search(&[1.0, 0.0], 10, 0.5);
        let ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["near", "mid"]);
        assert_eq!(ix.search(&[1.0, 0.0], 1, -1.0).len(), 1);
        assert!(ix.search(&[1.0], 10, -1.0).is_empty(), "wrong length");
    }

    #[test]
    fn removing_keeps_the_other_vectors_right() {
        let dir = tempfile::tempdir().unwrap();
        let mut ix = open(dir.path());
        ix.put("a", 1, &[1.0, 0.0]).unwrap();
        ix.put("b", 2, &[0.0, 1.0]).unwrap();
        ix.put("c", 3, &[0.6, 0.8]).unwrap();
        ix.remove("a").unwrap();
        assert_eq!(ix.vector("c"), Some(&[0.6, 0.8][..]));
        assert_eq!(ix.vector("b"), Some(&[0.0, 1.0][..]));
        assert_eq!(ix.len(), 2);
    }

    #[test]
    fn compacting_shrinks_the_file_and_keeps_the_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut ix = open(dir.path());
        for i in 0..400 {
            ix.put("a", i, &[1.0, 0.0]).unwrap();
        }
        ix.skip("s", 7).unwrap();
        ix.flush().unwrap();
        let before = fs::metadata(&ix.path).unwrap().len();
        assert!(ix.compact().unwrap());
        assert!(fs::metadata(&ix.path).unwrap().len() < before / 10);
        ix.put("b", 5, &[0.0, 1.0]).unwrap();
        ix.flush().unwrap();
        drop(ix);
        let ix = open(dir.path());
        assert_eq!(ix.stamp("a"), Some(399));
        assert_eq!(ix.stamp("b"), Some(5));
        assert_eq!(ix.stamp("s"), Some(7));
    }
}
