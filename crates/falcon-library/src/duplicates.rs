//! Finding duplicate items: exact copies by content hash, and near copies
//! by a perceptual hash of their thumbnails. See the Import chapter of the
//! docs; Eagle stores no hash in item JSON, so nothing here writes to items.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use image::DynamicImage;
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};

use crate::library::Library;
use crate::model::Item;
use crate::thumbnail;

/// BLAKE3 hash of a file's bytes, as 64 lowercase hex digits.
pub fn content_hash(path: &Path) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(path)?)?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Cached hashes of one item's original, valid while its size and mtime
/// stay the same.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub size: u64,
    /// The original's modification time, ms.
    pub mtime: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perceptual: Option<u64>,
}

/// Hashes keyed by item id, which the caller can keep between runs (it is
/// plain JSON; falcon doesn't store it in the library).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HashCache {
    pub entries: HashMap<String, CacheEntry>,
}

impl HashCache {
    /// Loads a cache saved with [`HashCache::save`]; empty if missing or
    /// unreadable.
    pub fn load(path: &Path) -> HashCache {
        fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        fs::write(path, serde_json::to_vec(self).map_err(io::Error::other)?)
    }

    /// The entry for `id` if it still matches the file's size and mtime;
    /// otherwise a fresh one replaces it.
    fn entry(&mut self, id: &str, size: u64, mtime: u64) -> &mut CacheEntry {
        let e = self.entries.entry(id.to_string()).or_insert(CacheEntry { size, mtime, content: None, perceptual: None });
        if e.size != size || e.mtime != mtime {
            *e = CacheEntry { size, mtime, content: None, perceptual: None };
        }
        e
    }
}

/// Size and mtime (ms) of a file.
fn stamp(path: &Path) -> Option<(u64, u64)> {
    let m = fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
    Some((m.len(), mtime))
}

/// Puts each group's ids in library order and orders the groups by their
/// first member.
fn tidy(mut groups: Vec<Vec<String>>, order: &HashMap<&str, usize>) -> Vec<Vec<String>> {
    for g in &mut groups {
        g.sort_by_key(|id| order.get(id.as_str()).copied().unwrap_or(usize::MAX));
    }
    groups.retain(|g| g.len() > 1);
    groups.sort_by_key(|g| order.get(g[0].as_str()).copied().unwrap_or(usize::MAX));
    groups
}

/// Items not in the trash, and among `only` when given.
fn live<'a>(lib: &'a Library, only: Option<&HashSet<String>>) -> Vec<&'a Item> {
    lib.items().iter().filter(|i| !i.is_deleted && only.is_none_or(|o| o.contains(&i.id))).collect()
}

/// Groups of items (not in the trash) whose originals are byte-identical.
pub fn find_exact_duplicates(lib: &Library) -> Vec<Vec<String>> {
    find_exact_duplicates_cached(lib, &mut HashCache::default())
}

/// [`find_exact_duplicates`] with a cache. Only files that share their size
/// with another file are hashed.
pub fn find_exact_duplicates_cached(lib: &Library, cache: &mut HashCache) -> Vec<Vec<String>> {
    find_exact_duplicates_in(lib, None, cache)
}

/// [`find_exact_duplicates_cached`] among the items in `only` (all live
/// items when `None`), as Eagle scans the selection or the current list.
pub fn find_exact_duplicates_in(lib: &Library, only: Option<&HashSet<String>>, cache: &mut HashCache) -> Vec<Vec<String>> {
    let items = live(lib, only);
    let order: HashMap<&str, usize> = items.iter().enumerate().map(|(n, i)| (i.id.as_str(), n)).collect();
    let mut by_size: HashMap<u64, Vec<(&Item, u64)>> = HashMap::new();
    for item in &items {
        if let Some((size, mtime)) = stamp(&item.original_path(lib.root())) {
            by_size.entry(size).or_default().push((item, mtime));
        }
    }
    let mut by_hash: HashMap<String, Vec<String>> = HashMap::new();
    for (size, group) in by_size.into_iter().filter(|(_, g)| g.len() > 1) {
        for (item, mtime) in group {
            let e = cache.entry(&item.id, size, mtime);
            if e.content.is_none() {
                e.content = content_hash(&item.original_path(lib.root())).ok();
            }
            if let Some(h) = e.content.clone() {
                by_hash.entry(h).or_default().push(item.id.clone());
            }
        }
    }
    tidy(by_hash.into_values().collect(), &order)
}

/// A 64-bit perceptual hash (pHash): the image is reduced to 32×32 grey,
/// transformed with a 2-D DCT, and each of the 8×8 lowest frequencies
/// becomes a bit, set when it is above their median (the DC term excluded
/// from the median).
pub fn perceptual_hash(img: &DynamicImage) -> u64 {
    const N: usize = 32;
    let grey = img.resize_exact(N as u32, N as u32, FilterType::Triangle).to_luma8();
    let px: Vec<f64> = grey.pixels().map(|p| f64::from(p.0[0])).collect();
    let cos: Vec<f64> = (0..8 * N)
        .map(|i| {
            let (u, x) = (i / N, i % N);
            ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / (2 * N) as f64).cos()
        })
        .collect();
    // Rows first, keeping only the 8 lowest frequencies, then columns.
    let mut rows = vec![0.0; N * 8];
    for y in 0..N {
        for u in 0..8 {
            rows[y * 8 + u] = (0..N).map(|x| px[y * N + x] * cos[u * N + x]).sum();
        }
    }
    let mut coeffs = [0.0f64; 64];
    for v in 0..8 {
        for u in 0..8 {
            coeffs[v * 8 + u] = (0..N).map(|y| rows[y * 8 + u] * cos[v * N + y]).sum();
        }
    }
    let mut ac: Vec<f64> = coeffs[1..].to_vec();
    ac.sort_by(f64::total_cmp);
    let median = (ac[31] + ac[32]) / 2.0;
    coeffs.iter().enumerate().fold(0u64, |h, (i, &c)| if c > median { h | (1 << i) } else { h })
}

/// Number of differing bits.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// The picture a perceptual hash is taken from: the thumbnail, or the
/// original for small images shown without one.
fn picture(item: &Item, root: &Path) -> Option<(std::path::PathBuf, DynamicImage)> {
    let path = item.thumbnail_path(root).or_else(|| {
        let o = item.original_path(root);
        (thumbnail::is_supported(&item.ext) && item.ext != "svg").then_some(o)
    })?;
    let img = thumbnail::decode(&path).ok()?;
    Some((path, img))
}

/// Groups of items whose pictures' perceptual hashes are at most
/// `threshold` bits apart (about 10 suits resized or recompressed copies).
/// Groups are linked transitively.
pub fn find_near_duplicates(lib: &Library, threshold: u32) -> Vec<Vec<String>> {
    find_near_duplicates_cached(lib, threshold, &mut HashCache::default())
}

/// [`find_near_duplicates`] with a cache. Entries are keyed on the
/// original's size and mtime, like the content hash, since a thumbnail
/// only changes with its original.
pub fn find_near_duplicates_cached(lib: &Library, threshold: u32, cache: &mut HashCache) -> Vec<Vec<String>> {
    find_near_duplicates_in(lib, None, threshold, cache)
}

/// [`find_near_duplicates_cached`] among the items in `only`; items
/// outside it never link two groups together.
pub fn find_near_duplicates_in(lib: &Library, only: Option<&HashSet<String>>, threshold: u32, cache: &mut HashCache) -> Vec<Vec<String>> {
    let items = live(lib, only);
    let order: HashMap<&str, usize> = items.iter().enumerate().map(|(n, i)| (i.id.as_str(), n)).collect();
    let mut hashes: Vec<(&str, u64)> = Vec::new();
    for item in &items {
        let Some((size, mtime)) = stamp(&item.original_path(lib.root())) else { continue };
        let e = cache.entry(&item.id, size, mtime);
        if e.perceptual.is_none() {
            e.perceptual = picture(item, lib.root()).map(|(_, img)| perceptual_hash(&img));
        }
        if let Some(h) = e.perceptual {
            hashes.push((item.id.as_str(), h));
        }
    }
    tidy(group_close(&hashes, threshold), &order)
}

/// Links every pair within `threshold` and returns the connected groups.
pub fn group_close(hashes: &[(&str, u64)], threshold: u32) -> Vec<Vec<String>> {
    let mut parent: Vec<usize> = (0..hashes.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..hashes.len() {
        for j in i + 1..hashes.len() {
            if hamming(hashes[i].1, hashes[j].1) <= threshold {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut groups: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, (id, _)) in hashes.iter().enumerate() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(id.to_string());
    }
    groups.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportOptions;
    use image::{Rgb, RgbImage};

    fn scene(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let disc = ((fx - 0.3).powi(2) + (fy - 0.4).powi(2)).sqrt() < 0.2;
            if disc { Rgb([240, 200, 40]) } else { Rgb([(fx * 120.0) as u8, 60, (fy * 200.0) as u8]) }
        }))
    }

    fn other(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            let stripe = (x / 16 + y / 24) % 2 == 0;
            if stripe { Rgb([20, 20, 20]) } else { Rgb([230, 240, 250]) }
        }))
    }

    #[test]
    fn perceptual_distances() {
        let a = perceptual_hash(&scene(400, 300));
        assert_eq!(hamming(a, perceptual_hash(&scene(400, 300))), 0);
        let resized = perceptual_hash(&scene(400, 300).resize_exact(173, 130, FilterType::CatmullRom));
        assert!(hamming(a, resized) <= 4, "resized copy: {}", hamming(a, resized));
        let far = hamming(a, perceptual_hash(&other(400, 300)));
        assert!(far >= 20, "unrelated: {far}");
    }

    #[test]
    fn grouping() {
        let g = group_close(&[("a", 0), ("b", 0b11), ("c", 0b1111), ("d", u64::MAX)], 2);
        let mut g: Vec<Vec<String>> = g.into_iter().map(|mut x| { x.sort(); x }).collect();
        g.sort();
        assert_eq!(g, [vec!["a", "b", "c"], vec!["d"]]);
    }

    #[test]
    fn finds_duplicates_in_a_library() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lib = Library::create(tmp.path().join("D.library")).unwrap();
        let opts = ImportOptions { palette: false, ..Default::default() };
        let save = |name: &str, img: &DynamicImage| {
            let p = tmp.path().join(name);
            img.save(&p).unwrap();
            p
        };
        let a = lib.import_file(save("a.png", &scene(1200, 900)), &opts).unwrap();
        let b = lib.import_file(save("b.png", &scene(1200, 900)), &opts).unwrap();
        let c = lib.import_file(save("c.png", &scene(1000, 750)), &opts).unwrap();
        let d = lib.import_file(save("d.png", &other(1200, 900)), &opts).unwrap();

        assert_eq!(find_exact_duplicates(&lib), [vec![a.clone(), b.clone()]]);
        let near = find_near_duplicates(&lib, 10);
        assert_eq!(near, [vec![a.clone(), b.clone(), c.clone()]]);
        assert!(!near[0].contains(&d));

        let mut cache = HashCache::default();
        find_exact_duplicates_cached(&lib, &mut cache);
        find_near_duplicates_cached(&lib, 10, &mut cache);
        assert!(cache.entries[&a].perceptual.is_some());
        let path = tmp.path().join("cache.json");
        cache.save(&path).unwrap();
        assert_eq!(HashCache::load(&path), cache);
    }
}
