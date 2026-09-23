//! Content-addressed, linearly-versioned dataset manifests.
//!
//! Each published dataset version gets a `content_hash` (a Merkle-root-style
//! hash over its files' individual hashes) and a `parent_hash` pointing at
//! the previous version's `content_hash` (or `None` for the first version) --
//! a simple hash-linked chain, not a branching/merging model. Two
//! collaborators can independently verify they have byte-identical data by
//! recomputing `content_hash` from their local files and comparing, with no
//! shared server involved.
//!
//! Deliberately has no Arrow/DataFusion/Parquet dependency: verifying a
//! dataset's integrity shouldn't require pulling in a query engine.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQuery {
    pub collection: String,
    pub sites: Vec<String>,
    pub param_cd: String,
    pub start: String,
    pub end: String,
}

/// Snapshot of USGS's own revision state at publish time -- meaningful
/// because USGS revises historical data after the fact (Provisional ->
/// Approved), so a manifest can honestly say which upstream revision it
/// reflects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamState {
    pub max_last_modified: String,
    pub approval_status_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub content_hash: String,
    pub parent_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub generator: String,
    pub source: SourceQuery,
    pub upstream_state: UpstreamState,
    pub row_count: usize,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    pub ok: bool,
    pub expected_hash: String,
    pub actual_hash: String,
    pub mismatched_files: Vec<String>,
}

/// SHA-256 of a file's contents, as a lowercase hex string.
pub fn hash_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).with_context(|| format!("failed to read {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// A single top-level hash over a set of files: sorts by path (so file
/// write order never affects the result), then hashes the newline-joined
/// "path:sha256" lines. This is what two collaborators actually compare to
/// confirm they have identical data.
pub fn compute_content_hash(files: &[FileEntry]) -> String {
    let mut sorted: Vec<&FileEntry> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    let mut hasher = Sha256::new();
    for entry in sorted {
        hasher.update(entry.path.as_bytes());
        hasher.update(b":");
        hasher.update(entry.sha256.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

pub fn build_manifest(
    files: Vec<FileEntry>,
    source: SourceQuery,
    upstream_state: UpstreamState,
    parent: Option<&Manifest>,
    generator: &str,
) -> Manifest {
    let content_hash = compute_content_hash(&files);
    let row_count = files.iter().map(|f| f.row_count).sum();
    Manifest {
        content_hash,
        parent_hash: parent.map(|p| p.content_hash.clone()),
        created_at: Utc::now(),
        generator: generator.to_string(),
        source,
        upstream_state,
        row_count,
        files,
    }
}

/// Writes `<dir>/manifest.json` (pretty-printed) and returns its path.
pub fn write_manifest(manifest: &Manifest, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join("manifest.json");
    let json = serde_json::to_string_pretty(manifest).context("failed to serialize manifest")?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

/// Loads a manifest from an exact file path (typically `.../manifest.json`).
pub fn load_manifest(path: &Path) -> Result<Manifest> {
    let json = std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("failed to parse manifest at {}", path.display()))
}

/// Recomputes each file's hash from disk (relative to `base_dir`) and
/// compares against what the manifest declares -- the mechanical basis for
/// "two collaborators have the same data." A missing file counts as a
/// mismatch for that path.
pub fn verify_manifest(manifest: &Manifest, base_dir: &Path) -> Result<VerifyReport> {
    let mut mismatched = Vec::new();
    let mut actual_files = Vec::with_capacity(manifest.files.len());

    for entry in &manifest.files {
        let full_path = base_dir.join(&entry.path);
        let actual_hash = hash_file(&full_path).unwrap_or_default();
        if actual_hash.is_empty() || actual_hash != entry.sha256 {
            mismatched.push(entry.path.clone());
        }
        actual_files.push(FileEntry {
            path: entry.path.clone(),
            sha256: actual_hash,
            row_count: entry.row_count,
        });
    }

    let actual_hash = compute_content_hash(&actual_files);
    Ok(VerifyReport {
        ok: mismatched.is_empty(),
        expected_hash: manifest.content_hash.clone(),
        actual_hash,
        mismatched_files: mismatched,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        path
    }

    #[test]
    fn hash_is_stable_and_order_independent() {
        let a = FileEntry { path: "b.parquet".into(), sha256: "hash-b".into(), row_count: 1 };
        let b = FileEntry { path: "a.parquet".into(), sha256: "hash-a".into(), row_count: 1 };

        let hash1 = compute_content_hash(&[a.clone(), b.clone()]);
        let hash2 = compute_content_hash(&[b, a]);
        assert_eq!(hash1, hash2, "file order must not affect the content hash");
    }

    #[test]
    fn build_manifest_chains_parent_hash_to_content_hash() {
        let files = vec![FileEntry { path: "data.parquet".into(), sha256: "abc".into(), row_count: 10 }];
        let source = SourceQuery {
            collection: "daily".into(),
            sites: vec!["USGS-1".into()],
            param_cd: "00060".into(),
            start: "2024-01-01".into(),
            end: "2024-01-31".into(),
        };
        let upstream = UpstreamState { max_last_modified: "2025-01-01T00:00:00Z".into(), approval_status_counts: BTreeMap::new() };

        let v1 = build_manifest(files.clone(), source.clone(), upstream.clone(), None, "test");
        assert!(v1.parent_hash.is_none());

        let v2 = build_manifest(files, source, upstream, Some(&v1), "test");
        assert_eq!(v2.parent_hash, Some(v1.content_hash));
    }

    #[test]
    fn verify_detects_corruption_and_passes_on_untouched_files() {
        let dir = std::env::temp_dir().join(format!("usgs-manifest-test-{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();

        let file_path = write_temp_file(&dir, "data.bin", b"hello world");
        let real_hash = hash_file(&file_path).unwrap();

        let entry = FileEntry { path: "data.bin".into(), sha256: real_hash.clone(), row_count: 5 };
        let manifest = build_manifest(
            vec![entry],
            SourceQuery { collection: "x".into(), sites: vec![], param_cd: "x".into(), start: "".into(), end: "".into() },
            UpstreamState { max_last_modified: "".into(), approval_status_counts: BTreeMap::new() },
            None,
            "test",
        );

        let report = verify_manifest(&manifest, &dir).unwrap();
        assert!(report.ok, "unmodified file should verify clean");
        assert_eq!(report.actual_hash, report.expected_hash);

        // Corrupt the file and verify again.
        write_temp_file(&dir, "data.bin", b"corrupted!!");
        let report2 = verify_manifest(&manifest, &dir).unwrap();
        assert!(!report2.ok);
        assert_eq!(report2.mismatched_files, vec!["data.bin".to_string()]);

        std::fs::remove_dir_all(&dir).ok();
    }
}
