//! The road a leg drives, for legs the geometry archive does not hold.
//!
//! The Alaska, Yukon and British Columbia legs have only their
//! `route_points` (one every 30 miles or so), and a straight line between
//! them cuts every bend by more than the 90 m a way must sit within. So the
//! road is asked of Valhalla's truck routing: its own route between the
//! leg's ends first, and, where that takes another road, the route forced
//! through every route point (a few sit just off the road, and forcing them
//! sent Anchorage to Palmer 62 miles instead of 43). Either is kept only if
//! it agrees with the leg: within 3 percent of its miles, and passing within
//! half a mile of every route point. Answers are cached outside the
//! repository, keyed by the leg and its route points, so a re-run needs no
//! network and a reroute is a cache miss.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::geo::{haversine_m, LegLine, M_PER_MI};
use crate::http;
use crate::pyjson::Value;
use crate::util::Result;

/// The public FOSSGIS instance unless `FF_VALHALLA_URL` names another.
const DEFAULT_VALHALLA: &str = "https://valhalla1.openstreetmap.de";
const USER_AGENT: &str = "Freight-Fate map tools (https://github.com/orinks-games/freight-fate)";
/// A free community service: one request at a time, with a pause between.
const DELAY: Duration = Duration::from_millis(400);
const CACHE_VERSION: i64 = 1;
/// The scaffold legs' miles were set by hand, some a few miles off the
/// road; a route that passes every route point and is this close is the
/// leg's road, and the difference is reported.
const MAX_LENGTH_ERROR: f64 = 0.08;
const REPORT_LENGTH_ERROR: f64 = 0.03;
const MAX_POINT_OFF_MI: f64 = 0.5;

/// What the source notes call this road.
pub const SOURCE: &str = "a public Valhalla route through the leg's route points";

/// The default cache file, beside the other OpenStreetMap downloads.
pub fn default_cache() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".cache/freight-fate-osm/valhalla-route-shapes.json")
}

/// The leg's route points as `(lat, lon)`, in mile order.
pub fn route_points(leg: &Value) -> Vec<(f64, f64)> {
    let mut pts: Vec<(f64, f64, f64)> = leg
        .get("corridor")
        .and_then(|c| c.get("route_points"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0),
                p.get("lat").and_then(Value::as_f64)?,
                p.get("lon").and_then(Value::as_f64)?,
            ))
        })
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.into_iter().map(|(_, lat, lon)| (lat, lon)).collect()
}

fn cache_key(leg_id: &str, points: &[(f64, f64)]) -> String {
    let text: Vec<String> = points.iter().map(|(a, b)| format!("{a},{b}")).collect();
    let digest = Sha256::digest(text.join(";").as_bytes());
    format!("{leg_id}:{}", &hex::encode(digest)[..16])
}

/// Whether `shape` is the leg's road: its length within 8 percent of the
/// leg's miles and every route point within half a mile of it.
pub fn agrees(shape: &[(f64, f64)], points: &[(f64, f64)], leg_miles: f64) -> bool {
    if shape.len() < 2 || leg_miles <= 0.0 {
        return false;
    }
    let length_mi = LegLine::new(shape.to_vec()).length_m() / M_PER_MI;
    if (length_mi - leg_miles).abs() / leg_miles > MAX_LENGTH_ERROR {
        return false;
    }
    points.iter().all(|&(lat, lon)| {
        shape
            .iter()
            .map(|&(a, b)| haversine_m(lat, lon, a, b))
            .fold(f64::INFINITY, f64::min)
            <= MAX_POINT_OFF_MI * M_PER_MI
    })
}

/// Valhalla's encoded polyline (precision 6) as `(lat, lon)`.
pub fn decode_polyline(encoded: &str) -> Vec<(f64, f64)> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    let next = |i: &mut usize| -> Option<i64> {
        let (mut result, mut shift) = (0i64, 0u32);
        loop {
            let b = i64::from(*bytes.get(*i)?) - 63;
            *i += 1;
            result |= (b & 0x1f) << shift;
            shift += 5;
            if b < 0x20 {
                break;
            }
        }
        Some(if result & 1 == 1 {
            !(result >> 1)
        } else {
            result >> 1
        })
    };
    while i < bytes.len() {
        let (Some(dlat), Some(dlon)) = (next(&mut i), next(&mut i)) else {
            break;
        };
        lat += dlat;
        lon += dlon;
        out.push((lat as f64 / 1e6, lon as f64 / 1e6));
    }
    out
}

/// How far from a route point Valhalla may look for the road: both
/// carriageways of a divided highway, not the next road over.
const SNAP_RADIUS_M: i64 = 150;
/// The public instance's location limit.
const MAX_LOCATIONS: usize = 10;

/// The truck the routes are asked for: 80,000 lb, 13 ft 6 in, a 53 ft
/// trailer (the federal maxima on the Interstate system, 23 CFR 658.17).
fn request_body(points: &[(f64, f64)], costing: &str) -> String {
    let last = points.len().saturating_sub(1);
    let locations: Vec<serde_json::Value> = points
        .iter()
        .enumerate()
        .map(|(i, &(lat, lon))| {
            if i == 0 || i == last {
                serde_json::json!({"lat": lat, "lon": lon})
            } else {
                serde_json::json!({
                    "lat": lat, "lon": lon, "type": "through", "radius": SNAP_RADIUS_M,
                    "search_filter": {"exclude_ramp": true}
                })
            }
        })
        .collect();
    serde_json::json!({
        "locations": locations,
        "costing": costing,
        "costing_options": {"truck": {
            "height": 4.11, "width": 2.59, "length": 21.64,
            "weight": 36.29, "axle_load": 15.42, "hazmat": false
        }},
        "directions_options": {"units": "miles"}
    })
    .to_string()
}

fn fetch_route(
    agent: &ureq::Agent,
    base: &str,
    points: &[(f64, f64)],
    costing: &str,
) -> Result<Vec<(f64, f64)>> {
    let body = request_body(points, costing);
    let mut last = String::new();
    for attempt in 0..4u64 {
        std::thread::sleep(DELAY + Duration::from_secs(2 * attempt));
        let response = http::request(
            agent,
            "POST",
            &format!("{base}/route"),
            &[
                ("Content-Type", "application/json"),
                ("User-Agent", USER_AGENT),
            ],
            Some(body.as_bytes()),
        );
        match response {
            Ok(r) if r.status == 200 => {
                let json: serde_json::Value =
                    serde_json::from_slice(&r.body).map_err(|e| format!("Valhalla answer: {e}"))?;
                let mut shape: Vec<(f64, f64)> = Vec::new();
                for leg in json["trip"]["legs"].as_array().into_iter().flatten() {
                    let piece = decode_polyline(leg["shape"].as_str().unwrap_or(""));
                    let skip = usize::from(shape.last().is_some() && shape.last() == piece.first());
                    shape.extend(piece.into_iter().skip(skip));
                }
                return Ok(shape);
            }
            Ok(r) if r.status == 400 => {
                return Err(format!(
                    "Valhalla found no truck route: {}",
                    String::from_utf8_lossy(&r.body)
                ))
            }
            Ok(r) => last = format!("HTTP {}", r.status),
            Err(e) => last = e,
        }
    }
    Err(format!("Valhalla did not answer: {last}"))
}

/// Shapes already fetched, keyed by leg and route points.
pub struct ShapeCache {
    path: PathBuf,
    shapes: BTreeMap<String, Vec<(f64, f64)>>,
    dirty: bool,
}

impl ShapeCache {
    pub fn load(path: &Path) -> Self {
        let shapes = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .filter(|v| v["version"].as_i64() == Some(CACHE_VERSION))
            .and_then(|v| serde_json::from_value(v["shapes"].clone()).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            shapes,
            dirty: false,
        }
    }

    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let text = serde_json::json!({"version": CACHE_VERSION, "shapes": self.shapes}).to_string();
        std::fs::write(&self.path, text).map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }

    /// The leg's road from the cache, else from Valhalla when `fetch`.
    /// `Ok(None)` when it is not cached and `fetch` is off.
    pub fn line(&mut self, leg_id: &str, leg: &Value, fetch: bool) -> Result<Option<LegLine>> {
        let points = route_points(leg);
        if points.len() < 2 {
            return Ok(None);
        }
        let miles = leg.get("miles").and_then(Value::as_f64).unwrap_or(0.0);
        let key = cache_key(leg_id, &points);
        if let Some(shape) = self.shapes.get(&key).filter(|s| agrees(s, &points, miles)) {
            return Ok(Some(LegLine::new(shape.clone())));
        }
        if !fetch {
            return Ok(None);
        }
        let base = std::env::var("FF_VALHALLA_URL").unwrap_or_else(|_| DEFAULT_VALHALLA.into());
        let agent = http::agent(Some(Duration::from_secs(180)));
        let base = base.trim_end_matches('/');
        // Valhalla's own truck route between the ends first. Where it takes
        // another road, the truck route forced through every route point;
        // where truck costing will not pass one (the Pacific Highway
        // crossing at Blaine), the car route through them.
        let direct = [points[0], points[points.len() - 1]];
        let mut shape = fetch_route(&agent, base, &direct, "truck")?;
        for costing in ["truck", "auto"] {
            if agrees(&shape, &points, miles) {
                break;
            }
            if costing == "truck" && points.len() < 3 {
                continue; // the forced truck route is the direct one
            }
            shape = Vec::new();
            // Chunks of the location limit, each starting where the last ended.
            let mut start = 0;
            while start + 1 < points.len() {
                let end = (start + MAX_LOCATIONS - 1).min(points.len() - 1);
                let piece = fetch_route(&agent, base, &points[start..=end], costing)?;
                let skip = usize::from(shape.last().is_some() && shape.last() == piece.first());
                shape.extend(piece.into_iter().skip(skip));
                start = end;
            }
        }
        let got = LegLine::new(shape.clone()).length_m() / M_PER_MI;
        if !agrees(&shape, &points, miles) {
            return Err(format!(
                "no route through {leg_id}'s route points agrees with it (the last ran \
                 {got:.1} miles; the leg says {miles})"
            ));
        }
        if (got - miles).abs() / miles > REPORT_LENGTH_ERROR {
            println!("  {leg_id}: the road runs {got:.1} miles; the leg says {miles}");
        }
        self.shapes.insert(key, shape.clone());
        self.dirty = true;
        Ok(Some(LegLine::new(shape)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_valhallas_precision_six_polyline() {
        // Google's example polyline, read at precision 6.
        let pts = decode_polyline("_p~iF~ps|U_ulLnnqC_mqNvxq`@");
        assert_eq!(pts.len(), 3);
        assert!((pts[0].0 - 3.85).abs() < 1e-9 && (pts[0].1 + 12.02).abs() < 1e-9);
        assert!((pts[2].0 - 4.3252).abs() < 1e-9 && (pts[2].1 + 12.6453).abs() < 1e-9);
    }

    #[test]
    fn a_route_that_disagrees_with_the_leg_is_refused() {
        // About 6.9 miles due north.
        let shape: Vec<(f64, f64)> = (0..=100)
            .map(|i| (60.0 + f64::from(i) * 0.001, -141.0))
            .collect();
        let points = [(60.0, -141.0), (60.1, -141.0)];
        assert!(agrees(&shape, &points, 6.9));
        assert!(!agrees(&shape, &points, 8.0), "too short for the leg");
        assert!(
            !agrees(&shape, &[(60.0, -141.0), (60.05, -140.9)], 6.9),
            "misses a point"
        );
    }
}
