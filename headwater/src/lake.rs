use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use arrow::array::StringArray;
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use crate::cli::Service;
use crate::parquet_writer;
use crate::usgs::model::SiteReading;

fn service_dir_name(service: Service) -> &'static str {
    match service {
        Service::Daily => "daily",
        Service::Continuous => "continuous",
    }
}

/// The lake's four Hive partition-path levels, by exact directory-key name
/// (`site_no=.../param_cd=.../service=.../year=...`). A DataFusion
/// `ListingTable` over this lake must declare exactly these, by these exact
/// names, in `ListingOptions::with_table_partition_cols` -- DataFusion
/// requires every `key=value` path segment to name-match a declared
/// partition column, unlike DuckDB/Polars which infer more loosely.
pub const PARTITION_COLUMNS: [(&str, DataType); 4] = [
    ("site_no", DataType::Utf8),
    ("param_cd", DataType::Utf8),
    ("service", DataType::Utf8),
    ("year", DataType::Utf8),
];

/// The physical file schema for a DataFusion `ListingTable` registered over
/// this lake: `parquet_writer::schema()` minus `site_no`/`param_cd`, which
/// must be omitted here because they are *also* partition-path segments
/// (see `PARTITION_COLUMNS`) -- DataFusion refuses to register a table whose
/// declared partition columns collide with a physical column name, so the
/// two column sets must be disjoint even though the underlying Parquet files
/// physically contain `site_no`/`param_cd` too (kept there so single-file
/// mode and any non-partition-aware reader still see complete rows).
pub fn file_schema_for_listing() -> SchemaRef {
    let omit = ["site_no", "param_cd"];
    let fields: Vec<Field> = parquet_writer::schema()
        .fields()
        .iter()
        .filter(|f| !omit.contains(&f.name().as_str()))
        .map(|f| f.as_ref().clone())
        .collect();
    Arc::new(Schema::new(fields))
}

/// Partition directory for one site/param/service, e.g.
/// `<root>/site_no=USGS-01646500/param_cd=00060/service=daily`
fn partition_dir(root: &Path, site_no: &str, param_cd: &str, service: Service) -> PathBuf {
    root.join(format!("site_no={site_no}"))
        .join(format!("param_cd={param_cd}"))
        .join(format!("service={}", service_dir_name(service)))
}

fn year_of(datetime: &str) -> &str {
    &datetime[..4.min(datetime.len())]
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

/// Scan every Parquet file already written for this site/param/service and
/// return the maximum `datetime` value seen. Lexicographic order matches
/// chronological order here since every timestamp we write is the API's
/// original ISO 8601 string. Returns `None` if the partition doesn't exist
/// yet (first sync for this site).
pub fn max_datetime(
    root: &Path,
    site_no: &str,
    param_cd: &str,
    service: Service,
) -> Result<Option<String>> {
    let dir = partition_dir(root, site_no, param_cd, service);
    if !dir.exists() {
        return Ok(None);
    }

    let mut max: Option<String> = None;
    for path in walk_parquet_files(&dir)? {
        let file =
            File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .with_context(|| format!("failed to read Parquet metadata from {}", path.display()))?
            .build()
            .with_context(|| format!("failed to build Parquet reader for {}", path.display()))?;

        for batch in reader {
            let batch = batch
                .with_context(|| format!("failed to read row group in {}", path.display()))?;
            let col = batch
                .column_by_name("datetime")
                .context("Parquet file is missing a 'datetime' column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("'datetime' column is not the expected string type")?;
            for v in col.iter().flatten() {
                if max.as_deref().map(|m| v > m).unwrap_or(true) {
                    max = Some(v.to_string());
                }
            }
        }
    }

    Ok(max)
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
    let mut groups: BTreeMap<(String, String), Vec<SiteReading>> = BTreeMap::new();
    for reading in readings {
        let year = year_of(&reading.datetime).to_string();
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
        let batch = parquet_writer::build_batch(&group)?;
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
        let schema = file_schema_for_listing();
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
        assert_eq!(names, ["site_no", "param_cd", "service", "year"]);
    }
}
