pub mod lake_table;
pub mod rows;

use anyhow::Result;
use datafusion::prelude::{SessionContext, col, lit};

use crate::types::{FilterState, SiteReadingRow};

/// Apply `FilterState` to the lake via DataFusion's `DataFrame` builder API
/// (not string-interpolated SQL) and return deduplicated rows. Shared by
/// `query_readings` (display) and `publish_dataset` (materializing a
/// published version), so both filter identically.
pub async fn run_filtered_query(lake_root: &str, filter: &FilterState) -> Result<Vec<SiteReadingRow>> {
    let ctx = SessionContext::new();
    lake_table::register_lake(&ctx, lake_root).await?;

    let mut df = ctx.table("streamflow").await?.filter(col("param_cd").eq(lit(filter.param_cd.clone())))?;

    if !filter.sites.is_empty() {
        let exprs = filter.sites.iter().map(|s| lit(s.clone())).collect();
        df = df.filter(col("site_no").in_list(exprs, false))?;
    }
    if let Some(start) = filter.start.as_deref().filter(|s| !s.is_empty()) {
        df = df.filter(col("datetime").gt_eq(lit(start.to_string())))?;
    }
    if let Some(end) = filter.end.as_deref().filter(|s| !s.is_empty()) {
        df = df.filter(col("datetime").lt_eq(lit(end.to_string())))?;
    }

    let batches = df.collect().await?;
    let rows = rows::batches_to_rows(&batches)?;
    Ok(rows::dedup_keep_latest(rows))
}
