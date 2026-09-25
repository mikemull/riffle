use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::client::normalize_site_id;

const URL: &str = "https://api.waterdata.usgs.gov/ogcapi/v1/collections/combined-metadata/items";

#[derive(Debug, Deserialize)]
struct FeatureCollection {
    features: Vec<Feature>,
    #[serde(default)]
    links: Vec<Link>,
}

#[derive(Debug, Deserialize)]
struct Feature {
    properties: Properties,
}

#[derive(Debug, Deserialize)]
struct Link {
    rel: String,
    href: String,
}

#[derive(Debug, Deserialize)]
struct Properties {
    monitoring_location_id: String,
    #[serde(default)]
    monitoring_location_name: Option<String>,
    #[serde(default)]
    state_name: Option<String>,
    #[serde(default)]
    county_name: Option<String>,
    #[serde(default)]
    hydrologic_unit_code: Option<String>,
    #[serde(default)]
    drainage_area: Option<f64>,
    #[serde(default)]
    altitude: Option<f64>,
    #[serde(default)]
    parameter_code: Option<String>,
    #[serde(default)]
    parameter_name: Option<String>,
    #[serde(default)]
    statistic_id: Option<String>,
    #[serde(default)]
    begin: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    primary: Option<String>,
}

/// One parameter+statistic time series a site records, with its period of
/// record. `primary` is `None` for provisional series (USGS retains those
/// only 120 days; see the API's own field description).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesInfo {
    pub parameter_code: String,
    pub parameter_name: String,
    pub statistic_id: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    pub primary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteMetadata {
    pub site_no: String,
    pub name: String,
    pub state_name: Option<String>,
    pub county_name: Option<String>,
    pub hydrologic_unit_code: Option<String>,
    pub drainage_area: Option<f64>,
    pub altitude: Option<f64>,
    pub series: Vec<TimeSeriesInfo>,
}

/// Fetch site + time-series metadata for the given comma-separated site
/// numbers (optionally restricted to one parameter code) from USGS's
/// `combined-metadata` collection, grouped one entry per site with its
/// series listed underneath -- the API itself returns one denormalized row
/// per (site, parameter, statistic) series, repeating the site-level fields.
pub fn fetch(sites: &str, param: Option<&str>, api_key: Option<&str>) -> Result<Vec<SiteMetadata>> {
    let client = reqwest::blocking::Client::new();
    let sites_param = sites.split(',').map(normalize_site_id).collect::<Vec<_>>().join(",");

    let mut next_url = Some(URL.to_string());
    let mut first_request = true;
    let mut rows = Vec::new();

    while let Some(url) = next_url.take() {
        let mut req = client.get(&url);
        if first_request {
            let mut query = vec![("monitoring_location_id", sites_param.as_str()), ("f", "json"), ("limit", "2000")];
            if let Some(p) = param {
                query.push(("parameter_code", p));
            }
            req = req.query(&query);
        }
        if let Some(key) = api_key {
            req = req.header("X-Api-Key", key);
        }

        let resp = req.send().with_context(|| format!("failed to send request to {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            bail!("USGS combined-metadata request failed with status {status}: {body}");
        }

        let page: FeatureCollection = resp.json().context("failed to parse combined-metadata JSON response")?;
        rows.extend(page.features.into_iter().map(|f| f.properties));

        next_url = page.links.into_iter().find(|l| l.rel == "next").map(|l| l.href);
        first_request = false;
    }

    if rows.is_empty() {
        bail!("no metadata found for site(s) {sites}");
    }

    // Group by site, preserving the order sites first appear in.
    let mut order = Vec::new();
    let mut by_site: HashMap<String, SiteMetadata> = HashMap::new();

    for row in rows {
        let entry = by_site.entry(row.monitoring_location_id.clone()).or_insert_with(|| {
            order.push(row.monitoring_location_id.clone());
            SiteMetadata {
                site_no: row.monitoring_location_id.clone(),
                name: row.monitoring_location_name.clone().unwrap_or_default(),
                state_name: row.state_name.clone(),
                county_name: row.county_name.clone(),
                hydrologic_unit_code: row.hydrologic_unit_code.clone(),
                drainage_area: row.drainage_area,
                altitude: row.altitude,
                series: Vec::new(),
            }
        });

        if let (Some(parameter_code), Some(parameter_name)) = (row.parameter_code, row.parameter_name) {
            entry.series.push(TimeSeriesInfo {
                parameter_code,
                parameter_name,
                statistic_id: row.statistic_id,
                begin: row.begin,
                end: row.end,
                primary: row.primary,
            });
        }
    }

    Ok(order.into_iter().filter_map(|id| by_site.remove(&id)).collect())
}
