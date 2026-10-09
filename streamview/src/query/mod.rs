pub mod lake_table;
pub mod rows;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use datafusion::prelude::{SessionContext, col, lit};
use datafusion::scalar::ScalarValue;

use crate::types::{FilterState, Service, SiteReadingRow};

/// A filter-panel date (`YYYY-MM-DD`), if one was given.
fn parse_filter_date(value: Option<&str>, label: &str) -> Result<Option<NaiveDate>> {
    value
        .filter(|s| !s.is_empty())
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| format!("invalid {label} date '{s}'")))
        .transpose()
}

/// A literal matching the service's `datetime` column type, for midnight
/// at the start of `date` (UTC, for continuous).
fn date_literal(service: Service, date: NaiveDate) -> ScalarValue {
    match service {
        Service::Daily => ScalarValue::Date32(Some(datafusion::arrow::datatypes::Date32Type::from_naive_date(date))),
        Service::Continuous => {
            let micros = date.and_hms_opt(0, 0, 0).expect("midnight is valid").and_utc().timestamp_micros();
            ScalarValue::TimestampMicrosecond(Some(micros), Some("UTC".into()))
        }
    }
}

/// Apply `FilterState` to the lake via DataFusion's `DataFrame` builder API
/// (not string-interpolated SQL) and return deduplicated rows. Shared by
/// `query_readings` (display) and `publish_dataset` (materializing a
/// published version), so both filter identically.
pub async fn run_filtered_query(lake_root: &str, filter: &FilterState) -> Result<Vec<SiteReadingRow>> {
    let ctx = SessionContext::new();
    lake_table::register_lake(&ctx, lake_root).await?;

    let service = filter.service;
    let mut df = ctx.table(service.as_str()).await?.filter(col("param_cd").eq(lit(filter.param_cd.clone())))?;

    if !filter.sites.is_empty() {
        let exprs = filter.sites.iter().map(|s| lit(s.clone())).collect();
        df = df.filter(col("site_no").in_list(exprs, false))?;
    }
    if let Some(start) = parse_filter_date(filter.start.as_deref(), "start")? {
        df = df.filter(col("datetime").gt_eq(lit(date_literal(service, start))))?;
    }
    if let Some(end) = parse_filter_date(filter.end.as_deref(), "end")? {
        // The end date is inclusive. Expressed as "before the start of the
        // next day" so continuous readings *during* the end date are kept --
        // `datetime <= midnight` would drop all but the first.
        let next_day = end.succ_opt().context("end date out of range")?;
        df = df.filter(col("datetime").lt(lit(date_literal(service, next_day))))?;
    }

    let batches = df.collect().await?;
    let rows = rows::batches_to_rows(&batches, service)?;
    Ok(rows::dedup_keep_latest(rows))
}
