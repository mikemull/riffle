use std::sync::Arc;

use anyhow::{Context, Result};
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::ListingOptions;
use datafusion::prelude::SessionContext;

/// Register `headwater`'s partitioned Parquet lake as a DataFusion table
/// named `streamflow`.
///
/// Ported from `dfquery/src/main.rs`, which is where this was originally
/// worked out the hard way: DataFusion's Hive partition parser requires the
/// declared partition columns to exactly name-match every `key=value`
/// directory level (our lake has 4: site_no/param_cd/service/year), but it
/// *also* refuses a partition column name that collides with a physical
/// in-file column -- and site_no/param_cd are both. So the file schema
/// passed here (`headwater::lake::file_schema_for_listing()`) deliberately
/// omits them, sourcing those two columns purely from the partition path
/// instead, while `headwater::lake::PARTITION_COLUMNS` still declares all 4
/// levels by exact name. DuckDB/Polars tolerate this duplication silently;
/// DataFusion does not.
pub async fn register_lake(ctx: &SessionContext, lake_root: &str) -> Result<()> {
    let file_schema = headwater::lake::file_schema_for_listing();

    let partition_cols = headwater::lake::PARTITION_COLUMNS
        .iter()
        .map(|(name, dtype)| (name.to_string(), dtype.clone()))
        .collect();

    let options = ListingOptions::new(Arc::new(ParquetFormat::default()))
        .with_table_partition_cols(partition_cols);

    ctx.register_listing_table("streamflow", lake_root, options, Some(file_schema), None)
        .await
        .with_context(|| format!("failed to register lake at {lake_root}"))?;

    Ok(())
}
