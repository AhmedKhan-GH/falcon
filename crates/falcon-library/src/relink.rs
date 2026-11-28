//! Missing media and relink engine for referenced assets.
//!
//! Provides matching candidates, batch scanning in folders, relative hint matching,
//! fingerprint/size verification, and atomic commit of updated source references.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::linked::{
    self, AssetStatus, LinkRecord,
    compute_fast_fingerprint, evaluate_status, is_linked_item, load_validated_sidecar,
    validate_link_source, write_sidecar,
};
use crate::model::Item;
use crate::storage_mode::{FalconConfig, atomic_write_json};

/// Match confidence and suitability for reconnecting a missing asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchConfidence {
    /// Exact match: same filename and identical size + content fingerprint.
    Exact,
    /// Verified content match: different filename, but matching size and content fingerprint.
    ContentMatch,
    /// Structure/path match: identical relative path inside a relocated source root.
    RelativePathMatch,
    /// Ambiguous match: same filename, but size or content fingerprint differs or multiple candidates exist.
    Ambiguous,
    /// Incompatible candidate: directory, unsupported format, inside library, or invalid file.
    Incompatible,
}

impl MatchConfidence {
    pub fn is_acceptable(&self) -> bool {
        matches!(self, Self::Exact | Self::ContentMatch | Self::RelativePathMatch)
    }
}

/// A prospective candidate file for relinking an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelinkCandidate {
    pub candidate_path: PathBuf,
    pub confidence: MatchConfidence,
    pub candidate_size: u64,
    pub candidate_mtime_ms: u64,
    pub content_fingerprint: Option<String>,
    pub reason: String,
}

/// Status of an item within a batch relink operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRelinkStatus {
    pub item_id: String,
    pub item_name: String,
    pub ext: String,
    pub previous_source: Option<PathBuf>,
    pub candidate: Option<RelinkCandidate>,
    pub candidates_count: usize,
    pub error: Option<String>,
}

/// Outcome of inspecting or proposing relink targets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelinkPreview {
    pub matched: Vec<ItemRelinkStatus>,
    pub ambiguous: Vec<ItemRelinkStatus>,
    pub unmatched: Vec<ItemRelinkStatus>,
    pub incompatible: Vec<ItemRelinkStatus>,
}

impl RelinkPreview {
    pub fn is_empty(&self) -> bool {
        self.matched.is_empty()
            && self.ambiguous.is_empty()
            && self.unmatched.is_empty()
            && self.incompatible.is_empty()
    }

    pub fn total_count(&self) -> usize {
        self.matched.len() + self.ambiguous.len() + self.unmatched.len() + self.incompatible.len()
    }
}

/// Relink an individual missing asset to a chosen target file.
///
/// Refuses invalid paths, containment in the library, or incompatible formats.
/// Preserves the existing item ID, cached thumbnails, tags, ratings, and folder memberships.
pub fn relink_item_to_file(
    library_root: &Path,
    item: &Item,
    target_path: &Path,
    allow_ambiguous: bool,
) -> Result<LinkRecord> {
    let config = FalconConfig::load_or_default(library_root)?;
    if !is_linked_item(&config, item, library_root) {
        return Err(Error::Refused(format!("item {} is not a linked asset", item.id)));
    }

    let canonical_target = validate_link_source(target_path, library_root)?;
    let target_meta = fs::metadata(&canonical_target).map_err(|e| Error::io(&canonical_target, e))?;
    if !target_meta.is_file() {
        return Err(Error::Import {
            path: canonical_target,
            reason: "relink target must be a regular file".into(),
        });
    }

    let _existing_record = load_validated_sidecar(&config, item, library_root)?.ok_or_else(|| {
        Error::Refused(format!("missing link sidecar for item {}", item.id))
    })?;

    // Compare file formats: extension must be compatible (case-insensitive)
    let target_ext = canonical_target
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    if !item.ext.eq_ignore_ascii_case(&target_ext) && !allow_ambiguous {
        return Err(Error::Import {
            path: canonical_target,
            reason: format!(
                "incompatible format: target has extension '{}' but asset requires '{}'",
                target_ext, item.ext
            ),
        });
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);

    let draft = linked::build_draft(&canonical_target, library_root, &target_meta, now_ms);
    let new_record = draft.into_record(item.id.clone());

    // Atomically write updated falcon-link.json
    write_sidecar(item, library_root, &new_record)?;

    Ok(new_record)
}

/// Evaluates a prospective candidate file against an item's current link metadata.
pub fn evaluate_candidate(
    library_root: &Path,
    item: &Item,
    record: &LinkRecord,
    candidate_path: &Path,
) -> RelinkCandidate {
    let canonical = match validate_link_source(candidate_path, library_root) {
        Ok(c) => c,
        Err(e) => {
            return RelinkCandidate {
                candidate_path: candidate_path.to_path_buf(),
                confidence: MatchConfidence::Incompatible,
                candidate_size: 0,
                candidate_mtime_ms: 0,
                content_fingerprint: None,
                reason: format!("invalid target: {e}"),
            };
        }
    };

    let meta = match fs::metadata(&canonical) {
        Ok(m) if m.is_file() => m,
        _ => {
            return RelinkCandidate {
                candidate_path: canonical,
                confidence: MatchConfidence::Incompatible,
                candidate_size: 0,
                candidate_mtime_ms: 0,
                content_fingerprint: None,
                reason: "candidate is not a readable regular file".into(),
            };
        }
    };

    let cand_size = meta.len();
    let cand_mtime = meta.modified().ok().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
    }).unwrap_or(0);
    let cand_fingerprint = compute_fast_fingerprint(&canonical, cand_size).ok();

    let orig_file_name = record
        .source_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cand_file_name = canonical
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let names_match = orig_file_name.eq_ignore_ascii_case(&cand_file_name);
    let size_matches = cand_size == record.identity.size;
    let fingerprint_matches = record.identity.content_fingerprint.is_some()
        && record.identity.content_fingerprint == cand_fingerprint;

    let cand_ext = canonical
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    if !item.ext.eq_ignore_ascii_case(&cand_ext) {
        return RelinkCandidate {
            candidate_path: canonical,
            confidence: MatchConfidence::Incompatible,
            candidate_size: cand_size,
            candidate_mtime_ms: cand_mtime,
            content_fingerprint: cand_fingerprint,
            reason: format!("incompatible extension '{}'", cand_ext),
        };
    }

    if names_match && size_matches && (fingerprint_matches || record.identity.content_fingerprint.is_none()) {
        RelinkCandidate {
            candidate_path: canonical,
            confidence: MatchConfidence::Exact,
            candidate_size: cand_size,
            candidate_mtime_ms: cand_mtime,
            content_fingerprint: cand_fingerprint,
            reason: "exact filename and content match".into(),
        }
    } else if fingerprint_matches && size_matches {
        RelinkCandidate {
            candidate_path: canonical,
            confidence: MatchConfidence::ContentMatch,
            candidate_size: cand_size,
            candidate_mtime_ms: cand_mtime,
            content_fingerprint: cand_fingerprint,
            reason: "content fingerprint matches (renamed original)".into(),
        }
    } else if names_match {
        RelinkCandidate {
            candidate_path: canonical,
            confidence: MatchConfidence::Ambiguous,
            candidate_size: cand_size,
            candidate_mtime_ms: cand_mtime,
            content_fingerprint: cand_fingerprint,
            reason: "filename matches but size or content differs".into(),
        }
    } else {
        RelinkCandidate {
            candidate_path: canonical,
            confidence: MatchConfidence::Ambiguous,
            candidate_size: cand_size,
            candidate_mtime_ms: cand_mtime,
            content_fingerprint: cand_fingerprint,
            reason: "candidate content or identity does not match".into(),
        }
    }
}

/// Recursively scans `search_dir` (up to max depth) and indexes candidate files by filename and size.
pub fn scan_folder_index(
    search_dir: &Path,
    max_depth: usize,
) -> HashMap<String, Vec<PathBuf>> {
    let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![(search_dir.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    if depth < max_depth {
                        stack.push((path, depth + 1));
                    }
                } else if file_type.is_file()
                    && let Some(name) = path.file_name().and_then(|s| s.to_str())
                {
                    index.entry(name.to_lowercase()).or_default().push(path);
                }
            }
        }
    }
    index
}

/// Scans a folder to find matching candidate files for a list of missing items.
pub fn preview_relink_folder(
    library_root: &Path,
    items: &[Item],
    search_dir: &Path,
) -> RelinkPreview {
    let mut preview = RelinkPreview::default();
    let config = match FalconConfig::load_or_default(library_root) {
        Ok(c) => c,
        Err(_) => return preview,
    };

    let index = scan_folder_index(search_dir, 6);

    for item in items {
        if !is_linked_item(&config, item, library_root) {
            continue;
        }
        let status = evaluate_status(&config, item, library_root);
        if status == AssetStatus::Linked {
            // Already available and verified; skip unless user wants to relink
            continue;
        }

        let record = match load_validated_sidecar(&config, item, library_root) {
            Ok(Some(r)) => r,
            _ => {
                preview.incompatible.push(ItemRelinkStatus {
                    item_id: item.id.clone(),
                    item_name: item.name.clone(),
                    ext: item.ext.clone(),
                    previous_source: None,
                    candidate: None,
                    candidates_count: 0,
                    error: Some("missing or invalid link record".into()),
                });
                continue;
            }
        };

        let orig_name = record
            .source_path
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let mut item_candidates: Vec<RelinkCandidate> = Vec::new();

        // 1. Try relative hint directly inside search_dir
        if let Some(hint) = &record.relative_hint {
            let relative_candidate = search_dir.join(hint);
            if relative_candidate.is_file() {
                let mut cand = evaluate_candidate(library_root, item, &record, &relative_candidate);
                if cand.confidence != MatchConfidence::Incompatible {
                    cand.confidence = MatchConfidence::RelativePathMatch;
                    cand.reason = "matched relative directory hierarchy".into();
                    item_candidates.push(cand);
                }
            }
        }

        // 2. Try candidates found by filename in folder scan index
        if let Some(paths) = index.get(&orig_name) {
            for p in paths {
                if item_candidates.iter().any(|c| &c.candidate_path == p) {
                    continue;
                }
                item_candidates.push(evaluate_candidate(library_root, item, &record, p));
            }
        }

        let previous_source = Some(record.source_path.clone());
        let count = item_candidates.len();

        if count == 0 {
            preview.unmatched.push(ItemRelinkStatus {
                item_id: item.id.clone(),
                item_name: item.name.clone(),
                ext: item.ext.clone(),
                previous_source,
                candidate: None,
                candidates_count: 0,
                error: None,
            });
        } else if count == 1 {
            let cand = item_candidates.remove(0);
            match cand.confidence {
                MatchConfidence::Exact | MatchConfidence::ContentMatch | MatchConfidence::RelativePathMatch => {
                    preview.matched.push(ItemRelinkStatus {
                        item_id: item.id.clone(),
                        item_name: item.name.clone(),
                        ext: item.ext.clone(),
                        previous_source,
                        candidate: Some(cand),
                        candidates_count: 1,
                        error: None,
                    });
                }
                MatchConfidence::Ambiguous => {
                    preview.ambiguous.push(ItemRelinkStatus {
                        item_id: item.id.clone(),
                        item_name: item.name.clone(),
                        ext: item.ext.clone(),
                        previous_source,
                        candidate: Some(cand),
                        candidates_count: 1,
                        error: None,
                    });
                }
                MatchConfidence::Incompatible => {
                    preview.incompatible.push(ItemRelinkStatus {
                        item_id: item.id.clone(),
                        item_name: item.name.clone(),
                        ext: item.ext.clone(),
                        previous_source,
                        candidate: Some(cand),
                        candidates_count: 1,
                        error: None,
                    });
                }
            }
        } else {
            // Multiple candidates: pick exact match if unique, else treat as ambiguous
            let exact_matches: Vec<_> = item_candidates
                .iter()
                .filter(|c| c.confidence == MatchConfidence::Exact || c.confidence == MatchConfidence::RelativePathMatch)
                .cloned()
                .collect();
            if exact_matches.len() == 1 {
                preview.matched.push(ItemRelinkStatus {
                    item_id: item.id.clone(),
                    item_name: item.name.clone(),
                    ext: item.ext.clone(),
                    previous_source,
                    candidate: Some(exact_matches[0].clone()),
                    candidates_count: count,
                    error: None,
                });
            } else {
                preview.ambiguous.push(ItemRelinkStatus {
                    item_id: item.id.clone(),
                    item_name: item.name.clone(),
                    ext: item.ext.clone(),
                    previous_source,
                    candidate: item_candidates.into_iter().next(),
                    candidates_count: count,
                    error: Some("multiple potential candidates found in folder".into()),
                });
            }
        }
    }

    preview
}

/// Applies accepted relink pairs atomically, with rollback if an error occurs.
/// Returns the list of successfully relinked item IDs.
pub fn apply_relink_batch(
    library_root: &Path,
    items: &[Item],
    pairs: &[(String, PathBuf)],
) -> Result<Vec<String>> {
    let mut committed: Vec<(String, LinkRecord)> = Vec::new();
    let item_map: HashMap<&str, &Item> = items.iter().map(|i| (i.id.as_str(), i)).collect();

    for (item_id, target_path) in pairs {
        let Some(item) = item_map.get(item_id.as_str()) else {
            // Roll back already committed items
            rollback_relink_batch(library_root, &committed);
            return Err(Error::Refused(format!("unknown item {item_id}")));
        };

        // Read previous record for rollback safety
        let config = FalconConfig::load_or_default(library_root)?;
        let prev_record = match load_validated_sidecar(&config, item, library_root)? {
            Some(r) => r,
            None => {
                rollback_relink_batch(library_root, &committed);
                return Err(Error::Refused(format!("item {item_id} has no link sidecar")));
            }
        };

        match relink_item_to_file(library_root, item, target_path, true) {
            Ok(_) => {
                committed.push((item_id.clone(), prev_record));
            }
            Err(e) => {
                rollback_relink_batch(library_root, &committed);
                return Err(e);
            }
        }
    }

    Ok(committed.into_iter().map(|(id, _)| id).collect())
}

/// Rolls back sidecar files to their previous state in case of batch failure.
fn rollback_relink_batch(library_root: &Path, committed: &[(String, LinkRecord)]) {
    for (item_id, prev_record) in committed {
        let sidecar = library_root
            .join("images")
            .join(format!("{item_id}.info"))
            .join(linked::LINK_SIDECAR);
        let _ = atomic_write_json(&sidecar, prev_record);
    }
}
