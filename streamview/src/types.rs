use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One row from the `streamflow` DataFusion table (headwater's lake), shared
/// between the ssr query layer and the csr/wasm UI -- the whole point of
/// keeping this in one place is that both sides use the exact same type,
/// no separate TypeScript-ish DTO to keep in sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteReadingRow {
    pub site_no: String,
    pub param_cd: String,
    pub service: String,
    pub year: String,
    pub datetime: String,
    pub value: f64,
    pub qualifiers: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub approval_status: String,
    pub last_modified: String,
}

/// Filter criteria for `query_readings`, shared between the filter panel UI
/// and the ssr query layer. An empty `sites` means "all sites in the lake".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterState {
    pub sites: Vec<String>,
    pub param_cd: String,
    pub start: Option<String>,
    pub end: Option<String>,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            sites: Vec::new(),
            param_cd: "00060".to_string(),
            start: None,
            end: None,
        }
    }
}

/// One site's identity and location, for the filter panel's checkbox list
/// and the map's markers -- both driven from the same `list_sites()` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSummary {
    pub site_no: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// A published dataset version, for the publish panel's history list. This
/// is a display-oriented mirror of `usgs_manifest::Manifest` (plus the
/// directory name, which the manifest itself doesn't carry) rather than
/// sharing that type directly -- `usgs-manifest` stays an ssr-only
/// dependency, matching `headwater`/`datafusion`, rather than pulling its file
/// I/O-oriented API into the wasm/csr build for no benefit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSummary {
    pub dir_name: String,
    pub content_hash: String,
    pub parent_hash: Option<String>,
    pub created_at: String,
    pub row_count: usize,
    pub sites: Vec<String>,
    pub param_cd: String,
    pub start: String,
    pub end: String,
    pub max_last_modified: String,
    pub approval_status_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    pub ok: bool,
    pub expected_hash: String,
    pub actual_hash: String,
    pub mismatched_files: Vec<String>,
}

/// USGS descriptive metadata for a site (name, location, period of record),
/// read from the JSON file `headwater sites --output` produces -- not
/// fetched live, and not part of the reading-level lake data. Field-for-field
/// mirror of `headwater::usgs::combined_metadata::SiteMetadata`/
/// `TimeSeriesInfo`, kept as a separate local type for the same reason as
/// `ManifestSummary`: `headwater` is an ssr-only dependency, not available
/// to the wasm/csr build these types also need to compile for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteMetadataInfo {
    pub site_no: String,
    pub name: String,
    pub state_name: Option<String>,
    pub county_name: Option<String>,
    pub hydrologic_unit_code: Option<String>,
    pub drainage_area: Option<f64>,
    pub altitude: Option<f64>,
    pub series: Vec<SiteSeriesInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSeriesInfo {
    pub parameter_code: String,
    pub parameter_name: String,
    pub statistic_id: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    pub primary: Option<String>,
}
