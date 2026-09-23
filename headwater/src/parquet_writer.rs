use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow::array::{Float64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;

use crate::usgs::model::SiteReading;

pub fn schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("site_no", DataType::Utf8, false),
        Field::new("param_cd", DataType::Utf8, false),
        Field::new("datetime", DataType::Utf8, false),
        Field::new("value", DataType::Float64, false),
        Field::new("qualifiers", DataType::Utf8, true),
        Field::new("latitude", DataType::Float64, false),
        Field::new("longitude", DataType::Float64, false),
        Field::new("approval_status", DataType::Utf8, false),
        Field::new("last_modified", DataType::Utf8, false),
    ]))
}

pub fn build_batch(readings: &[SiteReading]) -> Result<RecordBatch> {
    let site_no: StringArray = readings.iter().map(|r| Some(r.site_no.as_str())).collect();
    let param_cd: StringArray = readings.iter().map(|r| Some(r.param_cd.as_str())).collect();
    let datetime: StringArray = readings.iter().map(|r| Some(r.datetime.as_str())).collect();
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
    let last_modified: StringArray = readings
        .iter()
        .map(|r| Some(r.last_modified.as_str()))
        .collect();

    RecordBatch::try_new(
        schema(),
        vec![
            Arc::new(site_no),
            Arc::new(param_cd),
            Arc::new(datetime),
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
    let mut writer = ArrowWriter::try_new(file, schema(), Some(props))
        .context("failed to create Parquet writer")?;
    writer
        .write(batch)
        .context("failed to write RecordBatch to Parquet")?;
    writer.close().context("failed to finalize Parquet file")?;

    Ok(())
}

pub fn write(readings: &[SiteReading], output: &Path) -> Result<()> {
    let batch = build_batch(readings)?;
    write_batch(&batch, output)
}
