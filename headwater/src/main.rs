use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Parser;
use headwater::{cli, lake, parquet_writer, usgs};

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Command::Fetch(args) => run_fetch(&args),
        cli::Command::Sites(args) => run_sites(&args),
        cli::Command::Basins(args) => run_basins(&args),
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

/// NLDI and NWIS areas further apart than this (as a ratio) usually mean
/// NLDI snapped the gauge to the wrong NHDPlus flowline.
const AREA_MISMATCH_TOLERANCE: f64 = 0.10;

const SQ_MI_TO_KM2: f64 = 2.589_988;

fn run_basins(args: &cli::BasinsArgs) -> Result<()> {
    let dir = &args.output_dir;
    std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;

    // Published drainage areas, for the sanity check. Not fatal if the
    // lookup fails -- the polygons are still worth having.
    let drainage_km2: HashMap<String, f64> =
        match usgs::combined_metadata::fetch(&args.sites, None, args.api_key.as_deref()) {
            Ok(sites) => sites
                .into_iter()
                .filter_map(|s| s.drainage_area.map(|da| (s.site_no, da * SQ_MI_TO_KM2)))
                .collect(),
            Err(e) => {
                eprintln!("warning: drainage-area lookup failed, skipping area check: {e:#}");
                HashMap::new()
            }
        };

    let mut csv = String::from("site_no,nldi_area_km2,nwis_drainage_area_km2,area_ratio\n");
    let mut failures = Vec::new();

    for site in args.sites.split(',').map(usgs::client::normalize_site_id) {
        let basin = match usgs::nldi::fetch_basin(&site, !args.full_resolution) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{site}: {e:#}");
                failures.push(site);
                continue;
            }
        };

        let path = dir.join(format!("{site}.geojson"));
        std::fs::write(&path, serde_json::to_string(&basin)?)
            .with_context(|| format!("failed to write {}", path.display()))?;

        let nldi_km2 = usgs::nldi::area_km2(&basin)?;
        let nwis_km2 = drainage_km2.get(&site).copied();
        let ratio = nwis_km2.map(|n| nldi_km2 / n);

        let check = match ratio {
            Some(r) if (r - 1.0).abs() > AREA_MISMATCH_TOLERANCE => {
                format!("  ** MISMATCH: NWIS drainage area {:.0} km² (ratio {r:.2})", nwis_km2.unwrap())
            }
            Some(r) => format!("  (NWIS {:.0} km², ratio {r:.3})", nwis_km2.unwrap()),
            None => "  (no NWIS drainage area to compare)".to_string(),
        };
        println!("{site}: {nldi_km2:.1} km² -> {}{check}", path.display());

        let fmt = |v: Option<f64>, prec: usize| v.map(|x| format!("{x:.prec$}")).unwrap_or_default();
        csv.push_str(&format!(
            "{site},{nldi_km2:.3},{},{}\n",
            fmt(nwis_km2, 3),
            fmt(ratio, 4)
        ));
    }

    let summary = dir.join("basins.csv");
    std::fs::write(&summary, csv).with_context(|| format!("failed to write {}", summary.display()))?;
    println!("wrote summary to {}", summary.display());

    if !failures.is_empty() {
        bail!("no basin for {} site(s): {}", failures.len(), failures.join(", "));
    }
    Ok(())
}

fn date_only(timestamp: &str) -> &str {
    &timestamp[..10.min(timestamp.len())]
}

fn today() -> String {
    chrono::Utc::now().date_naive().to_string()
}
