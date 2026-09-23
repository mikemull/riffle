use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::model::SiteReading;

const BASE_URL: &str = "https://api.waterdata.usgs.gov/ogcapi/v1/collections";

#[derive(Debug, Deserialize)]
struct FeatureCollection {
    features: Vec<Feature>,
    #[serde(default)]
    links: Vec<Link>,
}

#[derive(Debug, Deserialize)]
struct Feature {
    properties: Properties,
    geometry: Geometry,
}

#[derive(Debug, Deserialize)]
struct Geometry {
    coordinates: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct Properties {
    monitoring_location_id: String,
    parameter_code: String,
    time: String,
    // Can be JSON null (e.g. ice-affected periods with no numeric reading).
    value: Option<String>,
    #[serde(default, deserialize_with = "qualifier_as_string")]
    qualifier: Option<String>,
    #[serde(default)]
    approval_status: Option<String>,
    #[serde(default)]
    last_modified: Option<String>,
}

/// `qualifier` is sometimes a single nullable string and sometimes a list of
/// strings (e.g. `["ESTIMATED"]`) depending on the collection/site; normalize
/// both shapes to a single comma-joined string.
fn qualifier_as_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum QualifierValue {
        Single(Option<String>),
        Many(Vec<String>),
    }

    Ok(match QualifierValue::deserialize(deserializer)? {
        QualifierValue::Single(v) => v,
        QualifierValue::Many(v) if v.is_empty() => None,
        QualifierValue::Many(v) => Some(v.join(",")),
    })
}

#[derive(Debug, Deserialize)]
struct Link {
    rel: String,
    href: String,
}

/// Normalize a single USGS site number to the `USGS-`-prefixed monitoring
/// location ID the modernized API expects. Leaves an already-prefixed ID
/// (e.g. from another agency) untouched.
pub fn normalize_site_id(site: &str) -> String {
    let site = site.trim();
    if site.contains('-') {
        site.to_string()
    } else {
        format!("USGS-{site}")
    }
}

/// Normalize a comma-separated list of USGS site numbers; see
/// [`normalize_site_id`].
fn normalize_sites(sites: &str) -> String {
    sites.split(',').map(normalize_site_id).collect::<Vec<_>>().join(",")
}

/// Fetch every feature from a collection matching the given filters,
/// following `next` pagination links until the result set is exhausted.
pub fn fetch_collection(
    collection: &str,
    sites: &str,
    param: &str,
    datetime_range: &str,
    api_key: Option<&str>,
) -> Result<Vec<SiteReading>> {
    let client = reqwest::blocking::Client::new();
    let sites = normalize_sites(sites);

    let mut next_url = Some(format!("{BASE_URL}/{collection}/items"));
    let mut first_request = true;
    let mut readings = Vec::new();

    while let Some(url) = next_url.take() {
        let mut req = client.get(&url);
        if first_request {
            req = req.query(&[
                ("monitoring_location_id", sites.as_str()),
                ("parameter_code", param),
                ("datetime", datetime_range),
                ("f", "json"),
                ("limit", "2000"),
            ]);
        }
        if let Some(key) = api_key {
            req = req.header("X-Api-Key", key);
        }

        let resp = req
            .send()
            .with_context(|| format!("failed to send request to {url}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            bail!("USGS Water Data API request failed with status {status}: {body}");
        }

        let page: FeatureCollection = resp
            .json()
            .context("failed to parse USGS Water Data API JSON response")?;

        for feature in page.features {
            let p = feature.properties;
            let Some(Ok(value)) = p.value.map(|v| v.parse::<f64>()) else {
                // Missing (null) or non-numeric sentinel value -- skip
                continue;
            };
            // GeoJSON coordinate order is [longitude, latitude]
            let longitude = feature.geometry.coordinates.first().copied().unwrap_or(0.0);
            let latitude = feature.geometry.coordinates.get(1).copied().unwrap_or(0.0);

            readings.push(SiteReading {
                site_no: p.monitoring_location_id,
                param_cd: p.parameter_code,
                datetime: p.time,
                value,
                qualifiers: p.qualifier.unwrap_or_default(),
                latitude,
                longitude,
                approval_status: p.approval_status.unwrap_or_default(),
                last_modified: p.last_modified.unwrap_or_default(),
            });
        }

        next_url = page.links.into_iter().find(|l| l.rel == "next").map(|l| l.href);
        first_request = false;
    }

    Ok(readings)
}
