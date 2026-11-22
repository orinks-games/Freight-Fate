//! The truck router: Valhalla's route and elevation services.
//!
//! The public FOSSGIS instance answers by default; `FF_VALHALLA_URL` and
//! `FF_VALHALLA_ELEVATION_URL` point either at a local build.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use crate::http;
use crate::util::Result;

const DEFAULT_URL: &str = "https://valhalla1.openstreetmap.de";
const USER_AGENT: &str = "Freight-Fate rerouting (https://github.com/orinks-games/Freight-Fate)";
/// A free community service; do not hammer it.
const DELAY: Duration = Duration::from_millis(400);
const FT_PER_M: f64 = 3.280_839_895;

fn base(var: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// What the truck is: a loaded US semi at the federal Interstate maxima
/// (80,000 lb gross and 34,000 lb on the tandem, 23 CFR 658.17; 13 ft 6 in
/// tall, 8 ft 6 in wide, a 53 ft trailer). Valhalla's own default is about
/// 48,000 lb, which clears limits a loaded truck does not.
///
/// `use_tolls` is 1.0: tolls cost the route nothing. A dispatcher sends a
/// truck up the Tri-State and the Pennsylvania Turnpike; the old router left
/// this at its 0.5 default and put legs on parallel surface roads to dodge
/// them (Orinks, 2026-11-05).
fn truck_options() -> serde_json::Value {
    serde_json::json!({
        "height": 4.11,
        "width": 2.59,
        "length": 21.64,
        "weight": 36.29,
        "axle_load": 15.42,
        "hazmat": false,
        "use_tolls": 1.0,
    })
}

fn post(base: &str, path: &str, body: &serde_json::Value) -> Result<Option<serde_json::Value>> {
    let agent = http::agent(Some(Duration::from_secs(180)));
    let payload = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let url = format!("{base}{path}");
    let mut last = String::new();
    for attempt in 0..4u64 {
        match http::request(
            &agent,
            "POST",
            &url,
            &[
                ("Content-Type", "application/json"),
                ("User-Agent", USER_AGENT),
            ],
            Some(&payload),
        ) {
            Ok(resp) if resp.status == 200 => {
                std::thread::sleep(DELAY);
                return serde_json::from_slice(&resp.body)
                    .map(Some)
                    .map_err(|e| format!("{url}: {e}"));
            }
            // 400 is the router saying no route exists: an answer, not a fault.
            Ok(resp) if resp.status == 400 => {
                eprintln!("  {url}: {}", String::from_utf8_lossy(&resp.body));
                return Ok(None);
            }
            Ok(resp) => last = format!("status {}", resp.status),
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_secs(2 * (attempt + 1)));
    }
    Err(format!("{url}: {last}"))
}

/// Valhalla's encoded polyline (precision 6) to `(lon, lat)`.
pub fn decode_shape(encoded: &str) -> Vec<(f64, f64)> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    while i < bytes.len() {
        let mut vals = [0i64; 2];
        for v in &mut vals {
            let (mut shift, mut result) = (0u32, 0i64);
            while let Some(&b) = bytes.get(i) {
                let byte = i64::from(b) - 63;
                i += 1;
                result |= (byte & 0x1f) << shift;
                shift += 5;
                if byte < 0x20 {
                    break;
                }
            }
            *v = if result & 1 == 1 {
                !(result >> 1)
            } else {
                result >> 1
            };
        }
        lat += vals[0];
        lon += vals[1];
        out.push((lon as f64 * 1e-6, lat as f64 * 1e-6));
    }
    out
}

/// One truck route.
pub struct Route {
    /// `(lon, lat)` vertices.
    pub shape: Vec<(f64, f64)>,
    pub miles: f64,
    pub has_toll: bool,
    /// `(street names, miles)` per maneuver, for asking which road it rides.
    pub maneuvers: Vec<(Vec<String>, f64)>,
}

/// The truck route between two points through the curated `via` pins.
pub fn route(start: (f64, f64), end: (f64, f64), via: &[(f64, f64)]) -> Result<Option<Route>> {
    if via.len() + 2 > 10 {
        return Err("Valhalla routes at most 10 locations, endpoints included".into());
    }
    let mut locations = vec![serde_json::json!({"lat": start.0, "lon": start.1})];
    for &(lat, lon) in via {
        locations.push(serde_json::json!({
            "lat": lat, "lon": lon, "type": "through",
            "search_filter": {"min_road_class": "primary", "exclude_ramp": true},
        }));
    }
    locations.push(serde_json::json!({"lat": end.0, "lon": end.1}));
    let body = serde_json::json!({
        "locations": locations,
        "costing": "truck",
        "costing_options": {"truck": truck_options()},
        "directions_options": {"units": "miles"},
    });
    let Some(result) = post(&base("FF_VALHALLA_URL"), "/route", &body)? else {
        return Ok(None);
    };
    let Some(trip) = result.get("trip") else {
        return Ok(None);
    };
    let mut shape: Vec<(f64, f64)> = Vec::new();
    let mut maneuvers = Vec::new();
    let mut has_toll = trip
        .pointer("/summary/has_toll")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    for leg in trip
        .get("legs")
        .and_then(|l| l.as_array())
        .into_iter()
        .flatten()
    {
        let mut piece = decode_shape(leg.get("shape").and_then(|s| s.as_str()).unwrap_or(""));
        if let (Some(last), Some(first)) = (shape.last(), piece.first()) {
            if last == first {
                piece.remove(0);
            }
        }
        shape.extend(piece);
        has_toll |= leg
            .pointer("/summary/has_toll")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        for m in leg
            .get("maneuvers")
            .and_then(|m| m.as_array())
            .into_iter()
            .flatten()
        {
            // The road's number often rides only in `begin_street_names`
            // (Valhalla writes "BC 97" there and "Cariboo Highway" in
            // `street_names`), so both lists name the maneuver's road.
            let mut names: Vec<String> = Vec::new();
            for key in ["street_names", "begin_street_names"] {
                for n in m
                    .get(key)
                    .and_then(|n| n.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n.as_str())
                {
                    if !names.iter().any(|have| have == n) {
                        names.push(n.to_string());
                    }
                }
            }
            let miles = m.get("length").and_then(|l| l.as_f64()).unwrap_or(0.0);
            maneuvers.push((names, miles));
        }
    }
    let miles = trip
        .pointer("/summary/length")
        .and_then(serde_json::Value::as_f64)
        .ok_or("the route has no length")?;
    Ok(Some(Route {
        shape,
        miles,
        has_toll,
        maneuvers,
    }))
}

/// Ground height in feet at every point, from Valhalla `/height`. A tileset
/// built without elevation answers 200 with nulls; that is a refusal, and is
/// reported as one.
pub fn heights_ft(points: &[(f64, f64)]) -> Result<Vec<f64>> {
    let url = base("FF_VALHALLA_ELEVATION_URL");
    let mut out = Vec::with_capacity(points.len());
    for chunk in points.chunks(500) {
        let shape: Vec<serde_json::Value> = chunk
            .iter()
            .map(|&(lon, lat)| serde_json::json!({"lat": lat, "lon": lon}))
            .collect();
        let body = serde_json::json!({"shape": shape, "range": false});
        let result = post(&url, "/height", &body)?
            .ok_or_else(|| format!("{url}/height refused the shape"))?;
        let heights = result
            .get("height")
            .and_then(|h| h.as_array())
            .ok_or_else(|| format!("{url}/height answered without heights"))?;
        let empty = heights.iter().filter(|h| h.is_null()).count();
        if empty > 0 {
            return Err(format!(
                "{url} returned no elevation for {empty} of {} points; point \
                 FF_VALHALLA_ELEVATION_URL at an instance built with elevation tiles",
                heights.len()
            ));
        }
        out.extend(
            heights
                .iter()
                .filter_map(|h| h.as_f64())
                .map(|m| m * FT_PER_M),
        );
    }
    if out.len() != points.len() {
        return Err("Valhalla /height returned the wrong number of readings".into());
    }
    Ok(out)
}

static SHIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z ]*?)\s*[-\s]\s*(\d+)").unwrap());

/// `"South US Highway 181"` to `("US", "181")`; a street name to `None`.
/// Every state route collapses to one class, so "State Highway 6" and
/// "TX 6" compare equal: right for asking whether a road IS the leg's road.
pub fn shield_key(name: &str) -> Option<(&'static str, String)> {
    const DIRECTIONS: &[&str] = &["N", "S", "E", "W", "NORTH", "SOUTH", "EAST", "WEST"];
    let text = name.replace('.', " ").replace('-', " - ");
    let mut words: Vec<&str> = text.split_whitespace().collect();
    while words
        .first()
        .is_some_and(|w| DIRECTIONS.contains(&w.to_uppercase().as_str()))
    {
        words.remove(0);
    }
    while words
        .last()
        .is_some_and(|w| DIRECTIONS.contains(&w.to_uppercase().as_str()))
    {
        words.pop();
    }
    let joined = words.join(" ");
    let caps = SHIELD.captures(&joined)?;
    let prefix = caps[1]
        .to_uppercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let number = caps[2].to_string();
    let class = match prefix.as_str() {
        "I" | "INTERSTATE" => "I",
        "US" | "US HIGHWAY" | "US HWY" | "US ROUTE" => "US",
        "SR" | "STATE HIGHWAY" | "STATE ROUTE" | "STATE ROAD" | "HIGHWAY" | "HWY" | "ROUTE" => {
            "STATE"
        }
        // Canada's shields as OpenStreetMap writes them: the Trans-Canada
        // ("TCH 1") and the Yellowhead ("YH 16") are their provinces' routes
        // of the same number.
        "TCH" | "YH" => "STATE",
        "CR" | "COUNTY ROAD" | "COUNTY ROUTE" => "CR",
        p if p.ends_with(" ROUTE") || p.ends_with(" HIGHWAY") || p.ends_with(" ROAD") => "STATE",
        p if p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()) => "STATE",
        _ => return None,
    };
    Some((class, number))
}

/// Is `name` (as OpenStreetMap writes it) the road `highway` (as the world
/// writes it)? Class and number both, so US 95 is not I-95. A label naming
/// several roads ("Hwy 15 / Pacific Highway") matches any of them, and a
/// road known by name rather than number (the Alaska Highway, the Coquihalla)
/// matches that name: Canadian legs are labelled that way, and every
/// province's route collapses to the state class, so "BC 97" is "Hwy 97".
pub fn matches_shield(name: &str, highway: &str) -> bool {
    highway
        .split('/')
        .flat_map(|part| part.split(" and "))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| match (shield_key(part), shield_key(name)) {
            (Some(a), Some(b)) => a == b,
            (None, Some(b)) => same_name(name, part) || named_route_is(part, &b),
            (None, None) => same_name(name, part),
            _ => false,
        })
}

/// Alaska and Yukon highways are known by name, and the world labels its
/// legs that way, but the truck router often names a stretch only by its
/// route number ("AK 1" for the Seward Highway), so a named label also
/// matches the numbers its road carries. Read from the `ref` tags on the
/// OpenStreetMap ways of each name (planet of 2026-11-16): one road can
/// carry two numbers along its length (the Seward Highway is AK 1 to Tern
/// Lake and AK 9 on to Seward; the Richardson is AK 4 from Valdez and
/// AK 2 from Delta Junction).
const NAMED_ROUTES: &[(&str, &[&str])] = &[
    ("Alaska Highway", &["AK 2", "YK 1"]),
    ("Dalton Highway", &["AK 11"]),
    ("Elliott Highway", &["AK 2"]),
    ("George Parks Highway", &["AK 3"]),
    ("Parks Highway", &["AK 3"]),
    ("Glenn Highway", &["AK 1"]),
    ("Haines Highway", &["AK 7", "YK 3"]),
    ("Haines Road", &["YK 3"]),
    ("Klondike Highway", &["AK 98", "YK 2"]),
    ("Richardson Highway", &["AK 4", "AK 2"]),
    ("Seward Highway", &["AK 1", "AK 9"]),
    ("Steese Highway", &["AK 2", "AK 6"]),
    ("Sterling Highway", &["AK 1"]),
    ("Tok Cutoff", &["AK 1"]),
];

fn named_route_is(label: &str, shield: &(&'static str, String)) -> bool {
    NAMED_ROUTES
        .iter()
        .filter(|(road, _)| same_name(label, road))
        .flat_map(|(_, numbers)| numbers.iter())
        .any(|number| shield_key(number).as_ref() == Some(shield))
}

fn same_name(a: &str, b: &str) -> bool {
    // A street's direction word is not part of its name: OpenStreetMap
    // writes "East Palmer-Wasilla Highway" for the Palmer-Wasilla Highway.
    const DIRECTIONS: &[&str] = &["north", "south", "east", "west"];
    let norm = |s: &str| {
        let mut words: Vec<String> = s.split_whitespace().map(str::to_lowercase).collect();
        if words.len() > 2 && DIRECTIONS.contains(&words[0].as_str()) {
            words.remove(0);
        }
        words.join(" ")
    };
    let (a, b) = (norm(a), norm(b));
    !b.is_empty() && a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_precision_six() {
        fn enc(v: i64, out: &mut String) {
            let mut v = if v < 0 { !(v << 1) } else { v << 1 };
            while v >= 0x20 {
                out.push(char::from((0x20 | (v & 0x1f)) as u8 + 63));
                v >>= 5;
            }
            out.push(char::from(v as u8 + 63));
        }
        let mut s = String::new();
        for (dlat, dlon) in [(38_500_000, -120_200_000), (2_200_000, -750_000)] {
            enc(dlat, &mut s);
            enc(dlon, &mut s);
        }
        let pts = decode_shape(&s);
        assert_eq!(pts.len(), 2);
        assert!((pts[0].1 - 38.5).abs() < 1e-6 && (pts[0].0 + 120.2).abs() < 1e-6);
        assert!((pts[1].1 - 40.7).abs() < 1e-6 && (pts[1].0 + 120.95).abs() < 1e-6);
    }

    #[test]
    fn shields_compare_on_class_and_number() {
        assert!(matches_shield("I 94", "I-94"));
        assert!(matches_shield("Interstate 94 East", "I-94"));
        assert!(!matches_shield("US 41", "I-94"));
        assert!(!matches_shield("US 94", "I-94"));
        assert!(matches_shield("TX 6", "State Highway 6"));
        assert!(matches_shield("NC 16", "NC-16"));
        assert!(!matches_shield("Tri-State Tollway", "I-94"));
    }

    #[test]
    fn canadian_labels_match_by_number_or_by_name() {
        assert!(matches_shield("BC 97", "Hwy 97"));
        assert!(matches_shield("BC 97", "Highway 97"));
        assert!(matches_shield("Alaska Highway", "Alaska Highway"));
        assert!(matches_shield("alaska  highway", "Alaska Highway"));
        assert!(!matches_shield("John Hart Highway", "Alaska Highway"));
        assert!(matches_shield("BC 1", "Hwy 1 / Hwy 97"));
        assert!(matches_shield("BC 97", "Hwy 1 / Hwy 97"));
        assert!(!matches_shield("BC 5", "Hwy 1 / Hwy 97"));
        assert!(matches_shield(
            "Pacific Highway",
            "Hwy 15 / Pacific Highway"
        ));
        assert!(matches_shield("I-5", "I-5 / US-97"));
        assert!(matches_shield("TCH 1 West", "Hwy 1 / Hwy 97"));
        assert!(matches_shield("YH 16", "Hwy 16"));
    }

    #[test]
    fn alaska_named_highways_match_their_route_numbers() {
        assert!(matches_shield("AK 1", "Seward Highway"));
        assert!(matches_shield("AK 9", "Seward Highway"));
        assert!(matches_shield(
            "AK 3 North",
            "George Parks Highway / Glenn Highway"
        ));
        assert!(matches_shield("AK 2", "Richardson Highway"));
        assert!(matches_shield("AK 4", "Richardson Highway"));
        assert!(matches_shield(
            "AK 1",
            "Seward Highway and Portage Glacier Highway"
        ));
        assert!(!matches_shield("AK 3", "Richardson Highway"));
        assert!(!matches_shield("US 1", "Seward Highway"));
        assert!(!matches_shield("AK 2", "Dalton Highway"));
        assert!(matches_shield(
            "East Palmer-Wasilla Highway",
            "Palmer-Wasilla Highway"
        ));
        assert!(!matches_shield("East Street", "Street"));
    }
}
