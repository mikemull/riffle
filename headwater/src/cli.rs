use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "headwater", about = "Fetch USGS streamflow data and write it to Parquet")]
pub struct Cli {
    /// Comma-separated USGS site numbers, e.g. 01646500,03339000
    #[arg(long)]
    pub sites: String,

    /// USGS Water Data API collection to query
    #[arg(long, value_enum, default_value_t = Service::Daily)]
    pub service: Service,

    /// USGS parameter code (default 00060 = discharge, cubic feet/second)
    #[arg(long, default_value = "00060")]
    pub param: String,

    /// Start date, YYYY-MM-DD. Required unless --incremental finds existing
    /// data for a site (in which case it resumes from there); still required
    /// as the initial-sync fallback for a site with no existing data yet.
    #[arg(long, value_parser = parse_date)]
    pub start: Option<String>,

    /// End date, YYYY-MM-DD. Defaults to today when --incremental is set.
    #[arg(long, value_parser = parse_date)]
    pub end: Option<String>,

    /// Output Parquet file path (single-file mode). Mutually exclusive with
    /// --output-dir.
    #[arg(long, conflicts_with = "output_dir")]
    pub output: Option<PathBuf>,

    /// Output directory for a partitioned Parquet lake, laid out as
    /// site_no=.../param_cd=.../service=.../year=.../part-*.parquet.
    /// Mutually exclusive with --output.
    #[arg(long, conflicts_with = "output")]
    pub output_dir: Option<PathBuf>,

    /// Only fetch data newer than what's already in --output-dir for each
    /// site (scans existing Parquet files for the max timestamp). Requires
    /// --output-dir.
    #[arg(long, requires = "output_dir")]
    pub incremental: bool,

    /// USGS Water Data API key (optional; raises rate limits). Falls back to
    /// the USGS_API_KEY environment variable. Get one at
    /// https://api.waterdata.usgs.gov/signup/
    #[arg(long, env = "USGS_API_KEY")]
    pub api_key: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Service {
    /// Historical daily values (summarized mean/min/max per day)
    Daily,
    /// Continuous high-frequency sensor readings
    Continuous,
}

fn parse_date(s: &str) -> Result<String, String> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|_| s.to_string())
        .map_err(|e| format!("invalid date '{s}': expected YYYY-MM-DD ({e})"))
}
