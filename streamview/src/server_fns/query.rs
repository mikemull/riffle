use leptos::prelude::*;
use leptos::server_fn::codec::Json;

use crate::types::{FilterState, SiteReadingRow, SiteSummary};

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

/// Query the lake with real filter criteria. See
/// `query::run_filtered_query` (shared with `publish_dataset`) and
/// `query::rows::dedup_keep_latest` for why results are deduplicated.
#[server(endpoint = "/query_readings", input = Json)]
pub async fn query_readings(filter: FilterState) -> Result<Vec<SiteReadingRow>, ServerFnError> {
    crate::query::run_filtered_query(&super::lake_root(), &filter)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}
