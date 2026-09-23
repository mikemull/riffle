use anyhow::{Result, bail};

use super::client;
use super::model::SiteReading;

/// Historical daily values, via the modernized `daily` collection
/// (replaces the legacy NWIS `/dv` service).
pub fn fetch(
    sites: &str,
    param: &str,
    start: &str,
    end: &str,
    api_key: Option<&str>,
) -> Result<Vec<SiteReading>> {
    let datetime_range = format!("{start}/{end}");
    let readings = client::fetch_collection("daily", sites, param, &datetime_range, api_key)?;

    if readings.is_empty() {
        bail!("no data returned for site(s) {sites} (param {param}) in range {start}..{end}");
    }

    Ok(readings)
}
