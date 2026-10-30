//! The posted-limit profile of one leg, read off OpenStreetMap.
//!
//! A port of the speed-limit half of the deleted `bake_curve_geometry.py`
//! (the sweep that wrote today's `corridor.speed_limits`), reading ways from
//! a local extract instead of Overpass and the road from the geometry
//! archive instead of a router. Every quarter mile along the archived line
//! it takes the limit of the nearest posted way within 90 m, preferring one
//! that carries the leg's own route number. A run longer than four miles
//! with no posted way becomes an explicit gap (`mph: null`), so the game
//! falls back to its highway heuristic there instead of holding the last
//! posting for miles. Then the old anchor repair runs, so the result is
//! clean by construction.

use ff_core::pyfmt::round_py_n;
use regex::Regex;
use std::sync::LazyLock;

use super::geo::{LegLine, WayIndex, M_PER_MI};
use crate::pyjson::{Object, Value};

/// How near a way must be to govern a sample point.
pub const MATCH_CORRIDOR_M: f64 = 90.0;
/// One sample per this much road (a quarter mile).
pub const SAMPLE_M: f64 = 402.0;
/// A posting-free run longer than this becomes an explicit gap.
pub const SPEED_GAP_MI: f64 = 4.0;
/// No US interstate mainline posts below this, so a lower match on an
/// interstate leg is a frontage road or a ramp.
pub const INTERSTATE_MIN_PLAUSIBLE_MPH: f64 = 45.0;

const KMH_TO_MPH: f64 = 0.621371;
const NON_NUMERIC: &[&str] = &["none", "signals", "walk", "variable", "no", "unknown"];

static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)").unwrap());
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

/// An OSM `maxspeed` value in mph, rounded to the nearest 5 (half to even,
/// as Python's `round`), or `None` when it carries no posted number. A bare
/// number is mph: this map is the United States.
pub fn parse_maxspeed(raw: &str) -> Option<f64> {
    for token in raw.split([';', ',']) {
        let token = token.trim().to_lowercase();
        if token.is_empty() || NON_NUMERIC.contains(&token.as_str()) || token.contains("knots") {
            continue;
        }
        let Some(m) = NUMBER.captures(&token) else {
            continue;
        };
        let Ok(value) = m[1].parse::<f64>() else {
            continue;
        };
        let mph = if token.contains("mph") {
            value
        } else if ["km/h", "kmh", "kph"].iter().any(|u| token.contains(u)) {
            value * KMH_TO_MPH
        } else {
            value
        };
        let mph = (mph / 5.0).round_ties_even() * 5.0;
        if mph < 5.0 {
            continue;
        }
        return Some(mph.min(85.0));
    }
    None
}

/// A way's limit: the truck tag when it parses, else the general one.
pub fn way_limit(maxspeed: Option<&str>, maxspeed_hgv: Option<&str>) -> Option<(f64, bool)> {
    if let Some(mph) = maxspeed_hgv.and_then(parse_maxspeed) {
        return Some((mph, true));
    }
    maxspeed.and_then(parse_maxspeed).map(|mph| (mph, false))
}

fn digit_groups(text: &str) -> Vec<&str> {
    DIGITS.find_iter(text).map(|m| m.as_str()).collect()
}

/// Whether any number in an OSM `ref` ("US 60;SR 77") is one of the leg's.
fn ref_matches_shield(route_ref: &str, shield: &[&str]) -> bool {
    !shield.is_empty() && digit_groups(route_ref).iter().any(|d| shield.contains(d))
}

pub fn is_interstate(highway: &str) -> bool {
    highway.trim().to_uppercase().starts_with("I-")
}

/// One step of the profile. `mph` of `None` is a coverage gap.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub at_mi: f64,
    pub mph: Option<i64>,
    pub hgv: bool,
}

/// The coverage-aware step function for one leg, before the anchor repair.
pub fn bake_leg(highway: &str, line: &LegLine, leg_miles: f64, index: &WayIndex) -> Vec<Sample> {
    let raw_mi = line.length_m() / M_PER_MI;
    let scale = if leg_miles > 0.0 && raw_mi > 0.0 {
        leg_miles / raw_mi
    } else {
        1.0
    };
    let mile = |m: f64| round_py_n(m / M_PER_MI * scale, 1);
    let shield = digit_groups(highway);
    let interstate = is_interstate(highway);
    let mean_lat = line.points.iter().map(|p| p.0).sum::<f64>() / line.points.len() as f64;
    let coslat = mean_lat.to_radians().cos();
    let total_mi = mile(line.length_m());

    let mut samples: Vec<Sample> = Vec::new();
    let mut last_m = -1e9;
    let mut hole_start: Option<f64> = None;
    let n = line.points.len();
    for (i, &(lat, lon)) in line.points.iter().enumerate() {
        let is_last = i == n - 1;
        if line.cum_m[i] - last_m < SAMPLE_M && !is_last {
            continue;
        }
        last_m = line.cum_m[i];
        let at_mi = mile(line.cum_m[i]);
        // The nearest way that carries the leg's route number, else the
        // nearest of any; ways whose tag has no number do not count.
        let mut best: Option<(f64, bool, bool, f64)> = None; // mph, hgv, on_shield, dist
        for (way, d) in index.near(lat, lon, MATCH_CORRIDOR_M + 1e-9, coslat) {
            let Some(mph) = way.mph else { continue };
            let on_shield = ref_matches_shield(&way.route_ref, &shield);
            let better = match best {
                None => true,
                Some((_, _, b_on, b_d)) => (on_shield && !b_on) || (on_shield == b_on && d < b_d),
            };
            if better {
                best = Some((mph, way.hgv, on_shield, d));
            }
        }
        let Some((mph, hgv, _, _)) =
            best.filter(|b| !(interstate && b.0 < INTERSTATE_MIN_PLAUSIBLE_MPH))
        else {
            hole_start.get_or_insert(at_mi);
            continue;
        };
        if let Some(start) = hole_start.take() {
            if samples.last().is_some_and(|s| s.mph.is_some()) && at_mi - start > SPEED_GAP_MI {
                samples.push(Sample {
                    at_mi: start,
                    mph: None,
                    hgv: false,
                });
            }
        }
        let mph = mph as i64;
        if samples
            .last()
            .is_some_and(|s| s.mph == Some(mph) && s.hgv == hgv)
        {
            continue;
        }
        let row = Sample {
            at_mi,
            mph: Some(mph),
            hgv,
        };
        match samples.last_mut() {
            Some(last) if last.at_mi == at_mi => *last = row,
            _ => samples.push(row),
        }
    }
    if let Some(start) = hole_start {
        if samples.last().is_some_and(|s| s.mph.is_some()) && total_mi - start > SPEED_GAP_MI {
            samples.push(Sample {
                at_mi: start,
                mph: None,
                hgv: false,
            });
        }
    }
    samples
}

const SURFACE_ANCHOR_MAX_MPH: f64 = 45.0;
const SURFACE_NEXT_MIN_MPH: f64 = 55.0;
const SURFACE_MIN_SPAN_MI: f64 = 5.0;
const SURFACE_SOLE_SAMPLE_MIN_LEG_MI: f64 = 15.0;

/// The anchor repair (`repair_interstate_anchor_limits.py`), applied to one
/// fresh profile: interstate legs lose every sub-45 posting; a surface leg
/// loses a city-street mile-0 anchor that the road's own character
/// contradicts, or a lone street posting owning a long leg. Returns how
/// many postings it dropped.
pub fn repair(highway: &str, leg_miles: f64, samples: &mut Vec<Sample>) -> usize {
    let before = samples.len();
    if is_interstate(highway) {
        samples.retain(|s| {
            s.mph
                .is_none_or(|m| m as f64 >= INTERSTATE_MIN_PLAUSIBLE_MPH)
        });
        return before - samples.len();
    }
    let numeric: Vec<&Sample> = samples.iter().filter(|s| s.mph.is_some()).collect();
    let mph = |s: &Sample| s.mph.map(|m| m as f64).unwrap_or(0.0);
    let first_is_numeric = samples.first().is_some_and(|s| s.mph.is_some());
    let anchor = numeric.len() >= 2
        && first_is_numeric
        && numeric[0].at_mi == 0.0
        && mph(numeric[0]) < SURFACE_ANCHOR_MAX_MPH
        && mph(numeric[1]) >= SURFACE_NEXT_MIN_MPH
        && numeric[1].at_mi >= SURFACE_MIN_SPAN_MI
        && !samples
            .iter()
            .any(|s| s.mph.is_none() && s.at_mi < numeric[1].at_mi);
    let sole = numeric.len() == 1
        && first_is_numeric
        && samples.len() == 1
        && leg_miles >= SURFACE_SOLE_SAMPLE_MIN_LEG_MI
        && mph(numeric[0]) < SURFACE_ANCHOR_MAX_MPH
        && (numeric[0].at_mi == 0.0 || numeric[0].at_mi == leg_miles);
    if anchor || sole {
        samples.remove(0);
    }
    before - samples.len()
}

/// The world-source rows, keyed and ordered as the existing data writes them.
pub fn world_rows(samples: &[Sample], source: &str) -> Vec<Value> {
    samples
        .iter()
        .map(|s| {
            let mut row = Object::new();
            row.insert("at_mi", Value::Float(s.at_mi));
            match s.mph {
                Some(mph) => {
                    row.insert("mph", Value::Int(i128::from(mph)));
                    row.insert("source", Value::Str(source.to_string()));
                    row.insert("hgv", Value::Bool(s.hgv));
                }
                None => row.insert("mph", Value::Null),
            }
            Value::Object(row)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::geo::{corridor_cells, OsmWay};
    use super::*;

    #[test]
    fn maxspeed_values_read_as_python_read_them() {
        assert_eq!(parse_maxspeed("55 mph"), Some(55.0));
        assert_eq!(parse_maxspeed("55"), Some(55.0));
        assert_eq!(parse_maxspeed("100 km/h"), Some(60.0));
        assert_eq!(parse_maxspeed("signals"), None);
        assert_eq!(parse_maxspeed("none; 45 mph"), Some(45.0));
        // 52.5 rounds half to even: 50, as Python's round() does.
        assert_eq!(parse_maxspeed("52.5 mph"), Some(50.0));
        assert_eq!(parse_maxspeed("57.5 mph"), Some(60.0));
        assert_eq!(parse_maxspeed("90 mph"), Some(85.0));
        assert_eq!(
            way_limit(Some("70 mph"), Some("65 mph")),
            Some((65.0, true))
        );
        assert_eq!(
            way_limit(Some("70 mph"), Some("signals")),
            Some((70.0, false))
        );
    }

    fn way(id: i64, route_ref: &str, mph: f64, lat: f64, lon0: f64, lon1: f64) -> OsmWay {
        OsmWay {
            id,
            route_ref: route_ref.to_string(),
            mph: Some(mph),
            hgv: false,
            coords: vec![(lat, lon0), (lat, lon1)],
        }
    }

    /// A 20-mile east-west leg along latitude 40.
    fn leg_line() -> LegLine {
        let points = (0..=200)
            .map(|i| (40.0, -100.0 + f64::from(i) * 0.0019))
            .collect();
        LegLine::new(points)
    }

    fn bake(ways: Vec<OsmWay>, highway: &str) -> Vec<Sample> {
        let line = leg_line();
        let index = WayIndex::build(ways, &corridor_cells(std::iter::once(&line)));
        let miles = line.length_m() / M_PER_MI;
        bake_leg(highway, &line, miles, &index)
    }

    #[test]
    fn the_legs_own_route_wins_over_a_nearer_road() {
        // A 35 street hugging the route and the interstate 60 m off it.
        let samples = bake(
            vec![
                way(1, "", 35.0, 40.0001, -100.1, -99.5),
                way(2, "I 80", 70.0, 40.00055, -100.1, -99.5),
            ],
            "I-80",
        );
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].mph, Some(70));
        assert_eq!(samples[0].at_mi, 0.0);
    }

    #[test]
    fn a_long_untagged_run_becomes_a_gap_and_a_short_one_does_not() {
        // Posted from mile 0 to ~5 and again from ~15; nothing between.
        let samples = bake(
            vec![
                way(1, "US 6", 55.0, 40.0, -100.01, -99.906),
                way(2, "US 6", 55.0, 40.0, -99.72, -99.5),
            ],
            "US-6",
        );
        assert_eq!(samples.len(), 3, "{samples:?}");
        assert_eq!(samples[1].mph, None);
        assert!(samples[1].at_mi > 4.0 && samples[1].at_mi < 6.0);
        assert_eq!(samples[2].mph, Some(55));
    }

    #[test]
    fn interstate_legs_never_take_a_street_limit() {
        let samples = bake(vec![way(1, "", 30.0, 40.0, -100.1, -99.5)], "I-80");
        assert!(samples.is_empty(), "{samples:?}");
    }

    #[test]
    fn repair_drops_a_town_anchor_the_road_contradicts() {
        let s = |at_mi, mph| Sample {
            at_mi,
            mph: Some(mph),
            hgv: false,
        };
        let mut samples = vec![s(0.0, 25), s(22.0, 65)];
        assert_eq!(repair("US-60", 40.0, &mut samples), 1);
        assert_eq!(samples, vec![s(22.0, 65)]);
        let mut town = vec![s(0.0, 25), s(2.0, 65)];
        assert_eq!(repair("US-60", 40.0, &mut town), 0);
        let mut interstate = vec![s(0.0, 70), s(3.0, 40), s(4.0, 70)];
        assert_eq!(repair("I-55", 40.0, &mut interstate), 1);
    }

    #[test]
    fn world_rows_keep_the_existing_key_order() {
        let rows = world_rows(
            &[
                Sample {
                    at_mi: 0.0,
                    mph: Some(65),
                    hgv: true,
                },
                Sample {
                    at_mi: 6.5,
                    mph: None,
                    hgv: false,
                },
            ],
            "src",
        );
        let text: Vec<String> = rows
            .iter()
            .map(|r| crate::pyjson::dumps(r, &crate::pyjson::Options::compact()))
            .collect();
        assert_eq!(
            text,
            vec![
                r#"{"at_mi": 0.0, "mph": 65, "source": "src", "hgv": true}"#,
                r#"{"at_mi": 6.5, "mph": null}"#,
            ]
        );
    }
}
