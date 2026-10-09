use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use datafusion::arrow::array::{Array, AsArray, Float64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Date32Type, TimeUnit, TimestampMicrosecondType};
use datafusion::arrow::record_batch::RecordBatch;

use crate::types::{Service, SiteReadingRow, SiteSummary};

fn string_col<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .with_context(|| format!("missing column '{name}'"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("column '{name}' is not the expected Utf8 type"))
}

fn float_col<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a Float64Array> {
    batch
        .column_by_name(name)
        .with_context(|| format!("missing column '{name}'"))?
        .as_any()
        .downcast_ref::<Float64Array>()
        .with_context(|| format!("column '{name}' is not the expected Float64 type"))
}

/// Format a UTC instant for the wire. Fixed-width (always microseconds,
/// always `Z`) when `micros` is set, so formatted values sort correctly as
/// strings -- `dedup_keep_latest` relies on that for `last_modified`.
fn format_instant(dt: DateTime<Utc>, micros: bool) -> String {
    dt.to_rfc3339_opts(if micros { SecondsFormat::Micros } else { SecondsFormat::Secs }, true)
}

/// Format every value of a `Date32` or UTC microsecond-timestamp column as
/// an ISO 8601 string (see `SiteReadingRow`); nulls become `None`.
fn temporal_strings(batch: &RecordBatch, name: &str, micros: bool) -> Result<Vec<Option<String>>> {
    let array = batch.column_by_name(name).with_context(|| format!("missing column '{name}'"))?;
    Ok(match array.data_type() {
        DataType::Date32 => array
            .as_primitive::<Date32Type>()
            .iter()
            .map(|v| v.map(|d| Date32Type::to_naive_date(d).to_string()))
            .collect(),
        DataType::Timestamp(TimeUnit::Microsecond, _) => {
            let ts = array.as_primitive::<TimestampMicrosecondType>();
            (0..ts.len())
                .map(|i| {
                    if ts.is_null(i) {
                        return None;
                    }
                    ts.value_as_datetime(i).map(|dt| format_instant(dt.and_utc(), micros))
                })
                .collect()
        }
        other => bail!("column '{name}' has unexpected type {other}"),
    })
}

/// Convert query result batches back into `SiteReadingRow`s, downcasting
/// each named Arrow column. `service` is the table the batches came from;
/// it's not a column, since each service is its own table.
pub fn batches_to_rows(batches: &[RecordBatch], service: Service) -> Result<Vec<SiteReadingRow>> {
    let mut rows = Vec::new();

    for batch in batches {
        let site_no = string_col(batch, "site_no")?;
        let param_cd = string_col(batch, "param_cd")?;
        let year = string_col(batch, "year")?;
        let datetime = temporal_strings(batch, "datetime", false)?;
        let value = float_col(batch, "value")?;
        let qualifiers = string_col(batch, "qualifiers")?;
        let latitude = float_col(batch, "latitude")?;
        let longitude = float_col(batch, "longitude")?;
        let approval_status = string_col(batch, "approval_status")?;
        let last_modified = temporal_strings(batch, "last_modified", true)?;

        for i in 0..batch.num_rows() {
            rows.push(SiteReadingRow {
                site_no: site_no.value(i).to_string(),
                param_cd: param_cd.value(i).to_string(),
                service: service.as_str().to_string(),
                year: year.value(i).to_string(),
                datetime: datetime[i].clone().context("null datetime")?,
                value: value.value(i),
                qualifiers: (!qualifiers.is_null(i)).then(|| qualifiers.value(i).to_string()),
                latitude: latitude.value(i),
                longitude: longitude.value(i),
                approval_status: approval_status.value(i).to_string(),
                last_modified: last_modified[i].clone().unwrap_or_default(),
            });
        }
    }

    Ok(rows)
}

/// Extract a single string column across all batches, e.g. for
/// `SELECT DISTINCT site_no FROM daily`.
pub fn single_string_column(batches: &[RecordBatch], name: &str) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for batch in batches {
        let col = string_col(batch, name)?;
        for i in 0..batch.num_rows() {
            if !col.is_null(i) {
                values.push(col.value(i).to_string());
            }
        }
    }
    Ok(values)
}

/// Converts rows back into `headwater::usgs::model::SiteReading`, so a
/// published dataset can reuse `headwater::parquet_writer` verbatim rather
/// than re-implementing Parquet writing here. This drops `service`/`year`
/// (partition-derived, not part of headwater's physical file schema -- see
/// `headwater::lake::file_schema_for_listing`) and parses the wire-format
/// timestamp strings back into typed values.
pub fn to_site_readings(rows: &[SiteReadingRow]) -> Result<Vec<headwater::usgs::model::SiteReading>> {
    rows.iter()
        .map(|r| {
            let last_modified = (!r.last_modified.is_empty())
                .then(|| DateTime::parse_from_rfc3339(&r.last_modified).map(|dt| dt.with_timezone(&Utc)))
                .transpose()
                .with_context(|| format!("invalid last_modified '{}'", r.last_modified))?;
            Ok(headwater::usgs::model::SiteReading {
                site_no: r.site_no.clone(),
                param_cd: r.param_cd.clone(),
                datetime: headwater::usgs::model::ReadingTime::parse(&r.datetime)?,
                value: r.value,
                qualifiers: r.qualifiers.clone().unwrap_or_default(),
                latitude: r.latitude,
                longitude: r.longitude,
                approval_status: r.approval_status.clone(),
                last_modified,
            })
        })
        .collect()
}

/// Convert the result of `SELECT site_no, latitude, longitude FROM ...`
/// into `SiteSummary`s, for the filter panel and the map.
pub fn batches_to_site_summaries(batches: &[RecordBatch]) -> Result<Vec<SiteSummary>> {
    let mut summaries = Vec::new();
    for batch in batches {
        let site_no = string_col(batch, "site_no")?;
        let latitude = float_col(batch, "latitude")?;
        let longitude = float_col(batch, "longitude")?;
        for i in 0..batch.num_rows() {
            summaries.push(SiteSummary {
                site_no: site_no.value(i).to_string(),
                latitude: latitude.value(i),
                longitude: longitude.value(i),
            });
        }
    }
    Ok(summaries)
}

/// Collapse rows sharing the same (site_no, param_cd, datetime) down to one,
/// keeping the one with the greatest `last_modified`.
///
/// This exists because `headwater`'s incremental sync deliberately re-fetches
/// the boundary day on every resume (see headwater's `lake.rs`/`main.rs`),
/// which leaves genuine duplicate rows on disk for that day. Lexicographic
/// string comparison matches chronological order here because
/// `batches_to_rows` formats both `datetime` and `last_modified` as
/// fixed-width UTC ISO 8601 strings.
pub fn dedup_keep_latest(rows: Vec<SiteReadingRow>) -> Vec<SiteReadingRow> {
    use std::collections::HashMap;

    let mut best: HashMap<(String, String, String), SiteReadingRow> = HashMap::new();
    for row in rows {
        let key = (row.site_no.clone(), row.param_cd.clone(), row.datetime.clone());
        match best.get(&key) {
            Some(existing) if existing.last_modified >= row.last_modified => {}
            _ => {
                best.insert(key, row);
            }
        }
    }

    let mut result: Vec<SiteReadingRow> = best.into_values().collect();
    result.sort_by(|a, b| (a.site_no.as_str(), a.datetime.as_str()).cmp(&(b.site_no.as_str(), b.datetime.as_str())));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(site: &str, datetime: &str, value: f64, last_modified: &str) -> SiteReadingRow {
        SiteReadingRow {
            site_no: site.to_string(),
            param_cd: "00060".to_string(),
            service: "daily".to_string(),
            year: "2024".to_string(),
            datetime: datetime.to_string(),
            value,
            qualifiers: None,
            latitude: 0.0,
            longitude: 0.0,
            approval_status: "Approved".to_string(),
            last_modified: last_modified.to_string(),
        }
    }

    #[test]
    fn dedup_keeps_the_row_with_max_last_modified() {
        let rows = vec![
            row("USGS-1", "2024-03-31", 100.0, "2025-01-01T00:00:00+00:00"),
            row("USGS-1", "2024-03-31", 999.0, "2025-06-01T00:00:00+00:00"),
            row("USGS-1", "2024-04-01", 50.0, "2025-01-01T00:00:00+00:00"),
        ];

        let deduped = dedup_keep_latest(rows);

        assert_eq!(deduped.len(), 2);
        let march_31 = deduped.iter().find(|r| r.datetime == "2024-03-31").unwrap();
        assert_eq!(march_31.value, 999.0);
    }
}
