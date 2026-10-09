use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::ListingOptions;
use datafusion::prelude::SessionContext;

use crate::types::Service;

/// streamview's service enum -> headwater's (which streamview can't share
/// directly; see `types::Service`).
pub fn headwater_service(service: Service) -> headwater::cli::Service {
    match service {
        Service::Daily => headwater::cli::Service::Daily,
        Service::Continuous => headwater::cli::Service::Continuous,
    }
}

/// Register `headwater`'s partitioned Parquet lake as two DataFusion tables,
/// `daily` and `continuous`, one per `service=` subtree -- they can't be one
/// table because their `datetime` columns have different types (`Date32`
/// vs. UTC timestamp). See `headwater::lake::PARTITION_COLUMNS` for why
/// `service` is the top partition level.
///
/// Ported from `dfquery/src/main.rs`, which is where this was originally
/// worked out the hard way: DataFusion's Hive partition parser requires the
/// declared partition columns to exactly name-match every `key=value`
/// directory level below the table root (site_no/param_cd/year), but it
/// *also* refuses a partition column name that collides with a physical
/// in-file column -- and site_no/param_cd are both. So the file schema
/// passed here (`headwater::lake::file_schema_for_listing()`) deliberately
/// omits them, sourcing those two columns purely from the partition path
/// instead, while `headwater::lake::PARTITION_COLUMNS` still declares all
/// the levels by exact name. DuckDB/Polars tolerate this duplication
/// silently; DataFusion does not.
pub async fn register_lake(ctx: &SessionContext, lake_root: &str) -> Result<()> {
    for service in [Service::Daily, Service::Continuous] {
        let hw_service = headwater_service(service);
        let file_schema = headwater::lake::file_schema_for_listing(hw_service);

        let partition_cols = headwater::lake::PARTITION_COLUMNS
            .iter()
            .map(|(name, dtype)| (name.to_string(), dtype.clone()))
            .collect();

        let options = ListingOptions::new(Arc::new(ParquetFormat::default()))
            .with_file_extension(".parquet")
            .with_table_partition_cols(partition_cols);

        // Trailing slash: DataFusion treats the path as a directory (a
        // table) rather than a single file only when it ends in `/`.
        let root = headwater::lake::service_root(Path::new(lake_root), hw_service);
        let table_path = format!("{}/", root.display());

        ctx.register_listing_table(service.as_str(), &table_path, options, Some(file_schema), None)
            .await
            .with_context(|| format!("failed to register {} table at {table_path}", service.as_str()))?;
    }

    Ok(())
}
