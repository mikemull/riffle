use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Parser;
use headwater::{cli, lake, parquet_writer, usgs};

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Command::Fetch(args) => run_fetch(&args),
        cli::Command::Sites(args) => run_sites(&args),
    }
}

fn run_fetch(args: &cli::FetchArgs) -> Result<()> {
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
        Some(dir) => run_lake(args, dir),
        None => run_single_file(args),
    }
}

fn run_single_file(args: &cli::FetchArgs) -> Result<()> {
    let start = args
        .start
        .as_deref()
        .context("--start is required in single-file mode")?;
    let end = args
        .end
        .as_deref()
        .context("--end is required in single-file mode")?;
    let output = args.output.as_ref().expect("checked in run_fetch");

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

fn run_lake(args: &cli::FetchArgs, dir: &Path) -> Result<()> {
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

fn run_sites(args: &cli::SitesArgs) -> Result<()> {
    let sites = usgs::combined_metadata::fetch(&args.sites, args.param.as_deref(), args.api_key.as_deref())?;

    for site in &sites {
        println!("{} -- {}", site.site_no, site.name);

        let mut details = Vec::new();
        let location = [&site.county_name, &site.state_name]
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if !location.is_empty() {
            details.push(location);
        }
        if let Some(da) = site.drainage_area {
            details.push(format!("drainage area: {da} sq mi"));
        }
        if let Some(huc) = &site.hydrologic_unit_code {
            details.push(format!("HUC {huc}"));
        }
        if !details.is_empty() {
            println!("  {}", details.join(" | "));
        }

        if site.series.is_empty() {
            println!("  (no matching parameter/statistic series)");
        }
        for series in &site.series {
            let stat = series.statistic_id.as_deref().unwrap_or("-");
            let begin = date_only(series.begin.as_deref().unwrap_or("?"));
            let end = date_only(series.end.as_deref().unwrap_or("?"));
            let status = if series.primary.is_some() { "Primary" } else { "Provisional" };
            println!(
                "  {} ({})  stat {stat}  {begin} .. {end}  [{status}]",
                series.parameter_name, series.parameter_code
            );
        }
        println!();
    }

    if let Some(path) = &args.output {
        let json = serde_json::to_string_pretty(&sites).context("failed to serialize site metadata")?;
        std::fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))?;
        println!("wrote metadata for {} site(s) to {}", sites.len(), path.display());
    }

    Ok(())
}

fn date_only(timestamp: &str) -> &str {
    &timestamp[..10.min(timestamp.len())]
}

fn today() -> String {
    chrono::Utc::now().date_naive().to_string()
}
