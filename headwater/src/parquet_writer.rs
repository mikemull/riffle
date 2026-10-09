use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use arrow::array::{ArrayRef, Date32Array, Float64Array, StringArray, TimestampMicrosecondArray};
use arrow::datatypes::{DataType, Date32Type, Field, Schema, SchemaRef, TimeUnit};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;

use crate::cli::Service;
use crate::usgs::model::{ReadingTime, SiteReading};

/// Timestamps are stored at microsecond precision, normalized to UTC.
fn utc_timestamp() -> DataType {
    DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
}

/// The `datetime` column's type depends on the service: a daily value is a
/// local calendar date (`Date32`), a continuous reading is a UTC instant.
/// See [`ReadingTime`].
pub fn datetime_type(service: Service) -> DataType {
    match service {
        Service::Daily => DataType::Date32,
        Service::Continuous => utc_timestamp(),
    }
}

pub fn schema(service: Service) -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("site_no", DataType::Utf8, false),
        Field::new("param_cd", DataType::Utf8, false),
        Field::new("datetime", datetime_type(service), false),
        Field::new("value", DataType::Float64, false),
        Field::new("qualifiers", DataType::Utf8, true),
        Field::new("latitude", DataType::Float64, false),
        Field::new("longitude", DataType::Float64, false),
        Field::new("approval_status", DataType::Utf8, false),
        Field::new("last_modified", utc_timestamp(), true),
    ]))
}

/// Build the `datetime` column, rejecting any reading whose time doesn't
/// match the service's kind (a date in a continuous batch or vice versa).
fn datetime_column(readings: &[SiteReading], service: Service) -> Result<ArrayRef> {
    let mismatch = |t: &ReadingTime| anyhow!("reading time {t:?} doesn't match the {service:?} service");
    Ok(match service {
        Service::Daily => {
            let days = readings
                .iter()
                .map(|r| match r.datetime {
                    ReadingTime::Date(d) => Ok(Date32Type::from_naive_date(d)),
                    ref t => Err(mismatch(t)),
                })
                .collect::<Result<Vec<_>>>()?;
            Arc::new(Date32Array::from(days))
        }
        Service::Continuous => {
            let micros = readings
                .iter()
                .map(|r| match r.datetime {
                    ReadingTime::Instant(dt) => Ok(dt.timestamp_micros()),
                    ref t => Err(mismatch(t)),
                })
                .collect::<Result<Vec<_>>>()?;
            Arc::new(TimestampMicrosecondArray::from(micros).with_timezone("UTC"))
        }
    })
}

pub fn build_batch(readings: &[SiteReading], service: Service) -> Result<RecordBatch> {
    let site_no: StringArray = readings.iter().map(|r| Some(r.site_no.as_str())).collect();
    let param_cd: StringArray = readings.iter().map(|r| Some(r.param_cd.as_str())).collect();
    let datetime = datetime_column(readings, service)?;
    let value: Float64Array = readings.iter().map(|r| Some(r.value)).collect();
    let qualifiers: StringArray = readings
        .iter()
        .map(|r| (!r.qualifiers.is_empty()).then_some(r.qualifiers.as_str()))
        .collect();
    let latitude: Float64Array = readings.iter().map(|r| Some(r.latitude)).collect();
    let longitude: Float64Array = readings.iter().map(|r| Some(r.longitude)).collect();
    let approval_status: StringArray = readings
        .iter()
        .map(|r| Some(r.approval_status.as_str()))
        .collect();
    let last_modified = TimestampMicrosecondArray::from(
        readings
            .iter()
            .map(|r| r.last_modified.map(|dt| dt.timestamp_micros()))
            .collect::<Vec<_>>(),
    )
    .with_timezone("UTC");

    RecordBatch::try_new(
        schema(service),
        vec![
            Arc::new(site_no),
            Arc::new(param_cd),
            datetime,
            Arc::new(value),
            Arc::new(qualifiers),
            Arc::new(latitude),
            Arc::new(longitude),
            Arc::new(approval_status),
            Arc::new(last_modified),
        ],
    )
    .context("failed to build Arrow RecordBatch")
}

pub fn write_batch(batch: &RecordBatch, output: &Path) -> Result<()> {
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build();

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let file = File::create(output)
        .with_context(|| format!("failed to create output file {}", output.display()))?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))
        .context("failed to create Parquet writer")?;
    writer
        .write(batch)
        .context("failed to write RecordBatch to Parquet")?;
    writer.close().context("failed to finalize Parquet file")?;

    Ok(())
}

pub fn write(readings: &[SiteReading], service: Service, output: &Path) -> Result<()> {
    let batch = build_batch(readings, service)?;
    write_batch(&batch, output)
}
