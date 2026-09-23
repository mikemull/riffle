use anyhow::{Context, Result, bail};
use chrono::NaiveDate;

use super::client;
use super::model::SiteReading;

/// The modernized `continuous` collection caps each request to a 3-year
/// window, so requests spanning longer ranges are split and stitched together.
const MAX_WINDOW_DAYS: i64 = 3 * 365;

/// High-frequency sensor readings, via the modernized `continuous` collection
/// (replaces the legacy NWIS `/iv` service).
pub fn fetch(
    sites: &str,
    param: &str,
    start: &str,
    end: &str,
    api_key: Option<&str>,
) -> Result<Vec<SiteReading>> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .with_context(|| format!("invalid start date '{start}'"))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .with_context(|| format!("invalid end date '{end}'"))?;

    if end_date < start_date {
        bail!("end date {end} is before start date {start}");
    }

    let mut readings = Vec::new();
    let mut chunk_start = start_date;

    while chunk_start <= end_date {
        let chunk_end = std::cmp::min(
            chunk_start + chrono::Duration::days(MAX_WINDOW_DAYS - 1),
            end_date,
        );
        // The API treats a bare date as an exact midnight instant, not a
        // whole-day span, so pin explicit start-of-day/end-of-day times to
        // get full coverage of the end date.
        let datetime_range = format!("{chunk_start}T00:00:00Z/{chunk_end}T23:59:59Z");
        let mut chunk =
            client::fetch_collection("continuous", sites, param, &datetime_range, api_key)?;
        readings.append(&mut chunk);

        chunk_start = chunk_end + chrono::Duration::days(1);
    }

    if readings.is_empty() {
        bail!("no data returned for site(s) {sites} (param {param}) in range {start}..{end}");
    }

    Ok(readings)
}
