use leptos::prelude::*;
use leptos::server_fn::codec::Json;

use crate::types::{FilterState, ManifestSummary, VerifyReport};

#[cfg(feature = "ssr")]
fn parse_version_number(dir_name: &str) -> Option<usize> {
    let rest = dir_name.strip_prefix('v')?;
    let (n_str, _hash) = rest.split_once('-')?;
    n_str.parse().ok()
}

/// Every `v<n>-<hash8>` version directory under a dataset, sorted oldest
/// first -- oldest-to-newest is also parent-to-child order in the
/// `parent_hash` chain, since versions are only ever appended.
#[cfg(feature = "ssr")]
fn list_version_dirs(dataset_dir: &std::path::Path) -> std::io::Result<Vec<(usize, std::path::PathBuf)>> {
    let mut versions = Vec::new();
    if dataset_dir.exists() {
        for entry in std::fs::read_dir(dataset_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(n) = parse_version_number(&name) {
                versions.push((n, entry.path()));
            }
        }
    }
    versions.sort_by_key(|(n, _)| *n);
    Ok(versions)
}

#[cfg(feature = "ssr")]
fn to_summary(dir_name: String, m: &usgs_manifest::Manifest) -> ManifestSummary {
    ManifestSummary {
        dir_name,
        content_hash: m.content_hash.clone(),
        parent_hash: m.parent_hash.clone(),
        created_at: m.created_at.to_rfc3339(),
        row_count: m.row_count,
        sites: m.source.sites.clone(),
        param_cd: m.source.param_cd.clone(),
        start: m.source.start.clone(),
        end: m.source.end.clone(),
        max_last_modified: m.upstream_state.max_last_modified.clone(),
        approval_status_counts: m.upstream_state.approval_status_counts.clone(),
    }
}

/// Re-runs the given filter, materializes the result as a new published
/// dataset version (Parquet file + provenance manifest), and chains it onto
/// the dataset's existing version history via `parent_hash`. See
/// `usgs-manifest` for the hashing/chaining scheme.
#[server(endpoint = "/publish_dataset", input = Json)]
pub async fn publish_dataset(filter: FilterState, dataset_name: String) -> Result<ManifestSummary, ServerFnError> {
    use std::collections::{BTreeMap, BTreeSet};

    use usgs_manifest::{FileEntry, SourceQuery, UpstreamState, build_manifest, hash_file, load_manifest, write_manifest};

    let name = dataset_name.trim();
    if name.is_empty() {
        return Err(ServerFnError::new("dataset name is required"));
    }

    let rows = crate::query::run_filtered_query(&super::lake_root(), &filter)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if rows.is_empty() {
        return Err(ServerFnError::new("no rows matched this filter; nothing to publish"));
    }

    let dataset_dir = std::path::Path::new(&super::publish_root()).join(name);
    let existing_versions = list_version_dirs(&dataset_dir).map_err(|e| ServerFnError::new(e.to_string()))?;

    let parent_manifest = match existing_versions.last() {
        Some((_, path)) => Some(load_manifest(&path.join("manifest.json")).map_err(|e| ServerFnError::new(e.to_string()))?),
        None => None,
    };
    let next_version = existing_versions.last().map(|(n, _)| n + 1).unwrap_or(1);

    // Write to a provisional directory first -- the final directory name
    // embeds the data's content hash, which we only know once it's written.
    let readings = crate::query::rows::to_site_readings(&rows);
    let batch = headwater::parquet_writer::build_batch(&readings).map_err(|e| ServerFnError::new(e.to_string()))?;

    let provisional_dir = dataset_dir.join(format!("v{next_version}-pending"));
    std::fs::create_dir_all(&provisional_dir).map_err(|e| ServerFnError::new(e.to_string()))?;
    let file_path = provisional_dir.join("data.parquet");
    headwater::parquet_writer::write_batch(&batch, &file_path).map_err(|e| ServerFnError::new(e.to_string()))?;

    let file_hash = hash_file(&file_path).map_err(|e| ServerFnError::new(e.to_string()))?;
    let hash_prefix = &file_hash[..8.min(file_hash.len())];
    let final_dir_name = format!("v{next_version}-{hash_prefix}");
    let version_dir = dataset_dir.join(&final_dir_name);
    std::fs::rename(&provisional_dir, &version_dir).map_err(|e| ServerFnError::new(e.to_string()))?;

    let file_entry = FileEntry { path: "data.parquet".to_string(), sha256: file_hash, row_count: rows.len() };

    // Provenance reflects what was *actually* published, not just the raw
    // filter -- e.g. an empty `filter.sites` means "all sites", so we record
    // the distinct sites actually present in the data instead.
    let distinct_sites: Vec<String> = rows.iter().map(|r| r.site_no.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    let distinct_services: Vec<String> = rows.iter().map(|r| r.service.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    let mut approval_counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        *approval_counts.entry(r.approval_status.clone()).or_insert(0) += 1;
    }
    let max_last_modified = rows.iter().map(|r| r.last_modified.as_str()).max().unwrap_or("").to_string();

    let source = SourceQuery {
        collection: distinct_services.join(","),
        sites: distinct_sites,
        param_cd: filter.param_cd.clone(),
        start: filter.start.clone().unwrap_or_default(),
        end: filter.end.clone().unwrap_or_default(),
    };
    let upstream_state = UpstreamState { max_last_modified, approval_status_counts: approval_counts };

    let manifest = build_manifest(vec![file_entry], source, upstream_state, parent_manifest.as_ref(), "streamview/0.1.0");
    write_manifest(&manifest, &version_dir).map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(to_summary(final_dir_name, &manifest))
}

/// All published versions of a dataset, oldest first.
#[server(endpoint = "/list_published_versions")]
pub async fn list_published_versions(dataset_name: String) -> Result<Vec<ManifestSummary>, ServerFnError> {
    use usgs_manifest::load_manifest;

    let dataset_dir = std::path::Path::new(&super::publish_root()).join(dataset_name.trim());
    let versions = list_version_dirs(&dataset_dir).map_err(|e| ServerFnError::new(e.to_string()))?;

    let mut summaries = Vec::new();
    for (_, path) in versions {
        let manifest = load_manifest(&path.join("manifest.json")).map_err(|e| ServerFnError::new(e.to_string()))?;
        let dir_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        summaries.push(to_summary(dir_name, &manifest));
    }
    Ok(summaries)
}

/// Recomputes hashes from disk and compares against a published version's
/// manifest -- the check a collaborator runs to confirm their copy is
/// byte-identical to what was published.
#[server(endpoint = "/verify_published_version", input = Json)]
pub async fn verify_published_version(dataset_name: String, dir_name: String) -> Result<VerifyReport, ServerFnError> {
    use usgs_manifest::{load_manifest, verify_manifest};

    let version_dir = std::path::Path::new(&super::publish_root()).join(dataset_name.trim()).join(&dir_name);
    let manifest = load_manifest(&version_dir.join("manifest.json")).map_err(|e| ServerFnError::new(e.to_string()))?;
    let report = verify_manifest(&manifest, &version_dir).map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(VerifyReport {
        ok: report.ok,
        expected_hash: report.expected_hash,
        actual_hash: report.actual_hash,
        mismatched_files: report.mismatched_files,
    })
}
