use anyhow::{Context, Result, bail};
use serde_json::Value;

const BASE_URL: &str = "https://api.water.usgs.gov/nldi/linked-data/nwissite";

/// Mean Earth radius (m), as used by turf/d3 for spherical area.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Fetch the upstream drainage basin for one site from the USGS Network
/// Linked Data Index. Returns the raw GeoJSON FeatureCollection (one Polygon
/// or MultiPolygon feature). `simplified = false` asks for the full-resolution
/// boundary, which is several times larger.
pub fn fetch_basin(site: &str, simplified: bool) -> Result<Value> {
    let url = format!("{BASE_URL}/{site}/basin");
    let resp = reqwest::blocking::Client::new()
        .get(&url)
        .query(&[("simplified", simplified.to_string()), ("splitCatchment", "false".to_string())])
        .send()
        .with_context(|| format!("failed to send request to {url}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        // NLDI errors are problem+json; surface its `detail` when present.
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("detail").and_then(Value::as_str).map(str::to_string))
            .unwrap_or(body);
        bail!("NLDI basin request for {site} failed with status {status}: {detail}");
    }

    resp.json().context("failed to parse NLDI basin GeoJSON")
}

/// Spherical area (km²) of every Polygon/MultiPolygon geometry in a GeoJSON
/// FeatureCollection. Ring orientation is ignored: the first ring of each
/// polygon counts as the exterior and the rest are subtracted as holes.
pub fn area_km2(collection: &Value) -> Result<f64> {
    let features = collection
        .get("features")
        .and_then(Value::as_array)
        .context("GeoJSON has no features array")?;

    let mut total_m2 = 0.0;
    for feature in features {
        let geometry = feature.get("geometry").context("feature has no geometry")?;
        let coords = geometry.get("coordinates").context("geometry has no coordinates")?;
        match geometry.get("type").and_then(Value::as_str) {
            Some("Polygon") => total_m2 += polygon_area_m2(coords)?,
            Some("MultiPolygon") => {
                for polygon in coords.as_array().context("MultiPolygon coordinates not an array")? {
                    total_m2 += polygon_area_m2(polygon)?;
                }
            }
            other => bail!("unexpected basin geometry type {other:?}"),
        }
    }
    Ok(total_m2 / 1e6)
}

fn polygon_area_m2(rings: &Value) -> Result<f64> {
    let rings = rings.as_array().context("polygon coordinates not an array")?;
    let mut area = 0.0;
    for (i, ring) in rings.iter().enumerate() {
        let a = ring_area_m2(ring)?.abs();
        area += if i == 0 { a } else { -a };
    }
    Ok(area)
}

/// Signed area of a lon/lat ring on a sphere (Chamberlain & Duquette 2007,
/// the same formula turf.js uses).
fn ring_area_m2(ring: &Value) -> Result<f64> {
    let points = ring
        .as_array()
        .context("ring not an array")?
        .iter()
        .map(|p| {
            let lon = p.get(0).and_then(Value::as_f64);
            let lat = p.get(1).and_then(Value::as_f64);
            lon.zip(lat).context("ring position is not [lon, lat]")
        })
        .collect::<Result<Vec<_>>>()?;

    let n = points.len();
    if n < 3 {
        return Ok(0.0);
    }
    let mut sum = 0.0;
    for i in 0..n {
        let (lon_lo, _) = points[i];
        let (_, lat_mid) = points[(i + 1) % n];
        let (lon_hi, _) = points[(i + 2) % n];
        sum += (lon_hi.to_radians() - lon_lo.to_radians()) * lat_mid.to_radians().sin();
    }
    Ok(sum * EARTH_RADIUS_M * EARTH_RADIUS_M / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn one_degree_square_at_equator() {
        // ~12,364 km² for a 1°x1° cell at the equator.
        let fc = json!({"type": "FeatureCollection", "features": [{
            "type": "Feature", "properties": {},
            "geometry": {"type": "Polygon", "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]]}
        }]});
        let a = area_km2(&fc).unwrap();
        assert!((a - 12_364.0).abs() < 10.0, "got {a}");
    }

    #[test]
    fn hole_is_subtracted() {
        let fc = json!({"type": "FeatureCollection", "features": [{
            "type": "Feature", "properties": {},
            "geometry": {"type": "Polygon", "coordinates": [
                [[0,0],[2,0],[2,2],[0,2],[0,0]],
                [[0.5,0.5],[1.5,0.5],[1.5,1.5],[0.5,1.5],[0.5,0.5]]
            ]}
        }]});
        let a = area_km2(&fc).unwrap();
        assert!((a - 3.0 * 12_364.0).abs() < 50.0, "got {a}");
    }
}
