use leptos::prelude::*;
use leptos::server_fn::codec::Json;

use crate::types::{FilterState, SiteMetadataInfo, SiteReadingRow, SiteSummary};

/// Every distinct site in the lake, with its location -- for the filter
/// panel's checkbox list and the map's markers.
#[server(endpoint = "/list_sites")]
pub async fn list_sites() -> Result<Vec<SiteSummary>, ServerFnError> {
    use datafusion::prelude::SessionContext;

    let ctx = SessionContext::new();
    crate::query::lake_table::register_lake(&ctx, &super::lake_root())
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // MIN() rather than DISTINCT: a site's lat/lon should be constant across
    // its readings, but this guarantees exactly one row per site regardless.
    let df = ctx
        .sql(
            "SELECT site_no, MIN(latitude) AS latitude, MIN(longitude) AS longitude \
             FROM streamflow GROUP BY site_no ORDER BY site_no",
        )
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let batches = df.collect().await.map_err(|e| ServerFnError::new(e.to_string()))?;

    crate::query::rows::batches_to_site_summaries(&batches).map_err(|e| ServerFnError::new(e.to_string()))
}

/// USGS descriptive metadata (name, location, period of record) for the
/// sites in the lake, read from a static JSON file `headwater sites
/// --output` produces -- deliberately not fetched live (see the design
/// discussion this followed: streamview stays local-first, headwater is the
/// only thing that talks to the live USGS API). Returns an empty list,
/// rather than an error, if the file hasn't been generated yet -- this is
/// enhancement data, not something the rest of the UI should break without.
#[server(endpoint = "/site_metadata")]
pub async fn site_metadata() -> Result<Vec<SiteMetadataInfo>, ServerFnError> {
    let path = std::env::var("SITE_METADATA_PATH")
        .unwrap_or_else(|_| "../headwater/maumee_lake/site_metadata.json".to_string());

    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|e| ServerFnError::new(e.to_string())),
        Err(e) => {
            // Not returned to the client (this is enhancement data, not
            // worth breaking the page over) -- but silent failure here is
            // indistinguishable from "nothing to show", so at least log it
            // server-side. `path` is relative to the ssr binary's current
            // working directory, not the crate root -- a common way to
            // land here is running the binary from somewhere other than
            // `streamview/`.
            eprintln!("site_metadata: could not read {path} ({e}); returning empty list");
            Ok(Vec::new())
        }
    }
}

/// Query the lake with real filter criteria. See
/// `query::run_filtered_query` (shared with `publish_dataset`) and
/// `query::rows::dedup_keep_latest` for why results are deduplicated.
#[server(endpoint = "/query_readings", input = Json)]
pub async fn query_readings(filter: FilterState) -> Result<Vec<SiteReadingRow>, ServerFnError> {
    crate::query::run_filtered_query(&super::lake_root(), &filter)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}
