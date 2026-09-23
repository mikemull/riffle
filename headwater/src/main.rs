use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Parser;
use headwater::{cli, lake, parquet_writer, usgs};

fn main() -> Result<()> {
    let args = cli::Cli::parse();

    match (&args.output, &args.output_dir) {
        (None, None) => bail!("one of --output or --output-dir is required"),
        _ => {}
    }
    // clap's `requires = "output_dir"` on --incremental doesn't fire when
    // --output is also present (it conflicts with --output-dir), so check
    // explicitly here too.
    if args.incremental && args.output_dir.is_none() {
        bail!("--incremental requires --output-dir");
    }

    match &args.output_dir {
        Some(dir) => run_lake(&args, dir),
        None => run_single_file(&args),
    }
}

fn run_single_file(args: &cli::Cli) -> Result<()> {
    let start = args
        .start
        .as_deref()
        .context("--start is required in single-file mode")?;
    let end = args
        .end
        .as_deref()
        .context("--end is required in single-file mode")?;
    let output = args.output.as_ref().expect("checked in main");

    let readings = usgs::fetch(
        args.service,
        &args.sites,
        &args.param,
        start,
        end,
        args.api_key.as_deref(),
    )?;
    let row_count = readings.len();

    parquet_writer::write(&readings, output)?;

    println!("wrote {row_count} rows to {}", output.display());

    Ok(())
}

fn run_lake(args: &cli::Cli, dir: &Path) -> Result<()> {
    let end = args.end.clone().unwrap_or_else(today);

    let mut total = 0;
    for site in args.sites.split(',').map(str::trim) {
        let normalized_site = usgs::client::normalize_site_id(site);

        let start = if args.incremental {
            match lake::max_datetime(dir, &normalized_site, &args.param, args.service)? {
                // Resume from the start of the day of the last known
                // reading, so late-arriving data within that day isn't
                // missed (this re-fetches -- and duplicates -- that day's
                // existing rows; see lake::write_partitioned).
                Some(max) => max[..10.min(max.len())].to_string(),
                None => args.start.clone().with_context(|| {
                    format!("no existing data for site {site}; provide --start for its initial sync")
                })?,
            }
        } else {
            args.start
                .clone()
                .context("--start is required unless --incremental finds existing data")?
        };

        let readings =
            usgs::fetch(args.service, site, &args.param, &start, &end, args.api_key.as_deref())?;
        let written = lake::write_partitioned(dir, &readings, &args.param, args.service)?;
        println!("{site}: wrote {written} rows ({start}..{end}) to {}", dir.display());
        total += written;
    }

    println!("total: wrote {total} rows to {}", dir.display());
    Ok(())
}

fn today() -> String {
    chrono::Utc::now().date_naive().to_string()
}
