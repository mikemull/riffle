use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use arrow::array::{Array, ArrayRef, AsArray};
use arrow::datatypes::{
    DataType, Date32Type, Field, Schema, SchemaRef, TimeUnit, TimestampMicrosecondType,
};
use chrono::NaiveDate;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::arrow_reader::statistics::StatisticsConverter;

use crate::cli::Service;
use crate::parquet_writer;
use crate::usgs::model::SiteReading;

fn service_dir_name(service: Service) -> &'static str {
    match service {
        Service::Daily => "daily",
        Service::Continuous => "continuous",
    }
}

/// The Hive partition-path levels *below* a service root, by exact
/// directory-key name. The full layout is
/// `<root>/service=.../site_no=.../param_cd=.../year=.../part-*.parquet`.
///
/// `service` is deliberately the top level, outside these columns: daily and
/// continuous files have different `datetime` types, so each service is
/// registered as its own DataFusion table rooted at [`service_root`]. (A
/// glob can't select one service from a deeper `service=` level instead --
/// DataFusion's `ListingTableUrl` strips `key=value` segments before glob
/// matching.) A `ListingTable` over a service root must declare exactly
/// these columns, by these exact names, in
/// `ListingOptions::with_table_partition_cols` -- DataFusion requires every
/// `key=value` path segment to name-match a declared partition column,
/// unlike DuckDB/Polars which infer more loosely.
pub const PARTITION_COLUMNS: [(&str, DataType); 3] = [
    ("site_no", DataType::Utf8),
    ("param_cd", DataType::Utf8),
    ("year", DataType::Utf8),
];

/// The physical file schema for a DataFusion `ListingTable` registered over
/// one service root: `parquet_writer::schema()` minus `site_no`/`param_cd`,
/// which must be omitted here because they are *also* partition-path
/// segments (see `PARTITION_COLUMNS`) -- DataFusion refuses to register a
/// table whose declared partition columns collide with a physical column
/// name, so the two column sets must be disjoint even though the underlying
/// Parquet files physically contain `site_no`/`param_cd` too (kept there so
/// single-file mode and any non-partition-aware reader still see complete
/// rows).
pub fn file_schema_for_listing(service: Service) -> SchemaRef {
    let omit = ["site_no", "param_cd"];
    let fields: Vec<Field> = parquet_writer::schema(service)
        .fields()
        .iter()
        .filter(|f| !omit.contains(&f.name().as_str()))
        .map(|f| f.as_ref().clone())
        .collect();
    Arc::new(Schema::new(fields))
}

/// Root of one service's subtree, e.g. `<root>/service=daily` -- the path a
/// per-service `ListingTable` is registered at.
pub fn service_root(root: &Path, service: Service) -> PathBuf {
    root.join(format!("service={}", service_dir_name(service)))
}

/// Partition directory for one site/param/service, e.g.
/// `<root>/service=daily/site_no=USGS-01646500/param_cd=00060`
fn partition_dir(root: &Path, site_no: &str, param_cd: &str, service: Service) -> PathBuf {
    service_root(root, service)
        .join(format!("site_no={site_no}"))
        .join(format!("param_cd={param_cd}"))
}

fn walk_parquet_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)
            .with_context(|| format!("failed to read directory {}", current.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "parquet") {
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// The date of the latest reading already written for this
/// site/param/service (the UTC date, for continuous data), or `None` if the
/// partition doesn't exist yet (first sync for this site).
///
/// Reads only each file's footer: the Parquet writer records min/max
/// statistics per row group, so the max of the per-row-group maxes is the
/// answer without decoding any data pages.
pub fn max_date(
    root: &Path,
    site_no: &str,
    param_cd: &str,
    service: Service,
) -> Result<Option<NaiveDate>> {
    let dir = partition_dir(root, site_no, param_cd, service);
    if !dir.exists() {
        return Ok(None);
    }

    let mut max: Option<NaiveDate> = None;
    for path in walk_parquet_files(&dir)? {
        let file =
            File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)
            .with_context(|| format!("failed to read Parquet metadata from {}", path.display()))?;

        let converter =
            StatisticsConverter::try_new("datetime", builder.schema(), builder.parquet_schema())
                .with_context(|| format!("{} has no 'datetime' column", path.display()))?;
        let maxes = converter
            .row_group_maxes(builder.metadata().row_groups())
            .with_context(|| format!("failed to read statistics from {}", path.display()))?;
        if maxes.null_count() > 0 {
            bail!("{} has row groups without 'datetime' statistics", path.display());
        }

        for date in dates_of(&maxes)? {
            max = max.max(Some(date));
        }
    }

    Ok(max)
}

/// Calendar dates from a `Date32` or UTC microsecond-timestamp array.
fn dates_of(array: &ArrayRef) -> Result<Vec<NaiveDate>> {
    match array.data_type() {
        DataType::Date32 => Ok(array
            .as_primitive::<Date32Type>()
            .iter()
            .flatten()
            .map(Date32Type::to_naive_date)
            .collect()),
        DataType::Timestamp(TimeUnit::Microsecond, _) => {
            let ts = array.as_primitive::<TimestampMicrosecondType>();
            Ok((0..ts.len())
                .filter_map(|i| ts.value_as_datetime(i))
                .map(|dt| dt.date())
                .collect())
        }
        other => bail!("unexpected 'datetime' column type {other}"),
    }
}

/// Write readings into the partitioned lake layout, splitting by site and
/// year (param and service are constant across one fetch). Each call adds a
/// new file to the relevant partition rather than merging into existing
/// files, so re-running an overlapping date range produces duplicate rows in
/// that partition -- acceptable for this prototype; dedupe downstream if
/// needed (e.g. keep the row with the greatest `last_modified` per
/// site/param/datetime).
pub fn write_partitioned(
    root: &Path,
    readings: &[SiteReading],
    param_cd: &str,
    service: Service,
) -> Result<usize> {
    let mut groups: BTreeMap<(String, i32), Vec<SiteReading>> = BTreeMap::new();
    for reading in readings {
        let year = reading.datetime.year();
        groups
            .entry((reading.site_no.clone(), year))
            .or_default()
            .push(reading.clone());
    }

    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut total = 0;
    for ((site_no, year), group) in groups {
        let dir = partition_dir(root, &site_no, param_cd, service).join(format!("year={year}"));
        let path = dir.join(format!("part-{run_id}.parquet"));
        let batch = parquet_writer::build_batch(&group, service)?;
        parquet_writer::write_batch(&batch, &path)?;
        total += group.len();
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_schema_omits_partition_columns() {
        let schema = file_schema_for_listing(Service::Daily);
        assert!(schema.field_with_name("site_no").is_err());
        assert!(schema.field_with_name("param_cd").is_err());
        // Everything else from parquet_writer::schema() should survive.
        for name in ["datetime", "value", "qualifiers", "latitude", "longitude", "approval_status", "last_modified"] {
            assert!(schema.field_with_name(name).is_ok(), "missing column {name}");
        }
    }

    #[test]
    fn partition_columns_match_directory_layout() {
        let names: Vec<&str> = PARTITION_COLUMNS.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["site_no", "param_cd", "year"]);
    }

    #[test]
    fn service_is_the_top_partition_level() {
        let dir = partition_dir(Path::new("lake"), "USGS-1", "00060", Service::Continuous);
        assert_eq!(dir, Path::new("lake/service=continuous/site_no=USGS-1/param_cd=00060"));
    }
}
