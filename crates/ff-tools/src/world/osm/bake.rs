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
use super::region::LegRegions;
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
/// number is mph, as in the United States, unless `bare_kmh` (Canada).
#[cfg(test)]
pub fn parse_maxspeed_in(raw: &str, bare_kmh: bool) -> Option<f64> {
    read_maxspeed(raw, bare_kmh).map(|(mph, _)| mph)
}

/// `parse_maxspeed_in`, plus the number on the sign when it is in km/h.
fn read_maxspeed(raw: &str, bare_kmh: bool) -> Option<(f64, Option<i64>)> {
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
        let (mph, kmh) = if token.contains("mph") {
            (value, None)
        } else if bare_kmh || ["km/h", "kmh", "kph"].iter().any(|u| token.contains(u)) {
            (value * KMH_TO_MPH, Some(value.round() as i64))
        } else {
            (value, None)
        };
        let mph = (mph / 5.0).round_ties_even() * 5.0;
        if mph < 5.0 {
            continue;
        }
        return Some((mph.min(85.0), kmh));
    }
    None
}

/// A way's limit: the truck tag when it parses, else the general one;
/// bare numbers are km/h when `bare_kmh`.
#[cfg(test)]
pub fn way_limit_in(
    maxspeed: Option<&str>,
    maxspeed_hgv: Option<&str>,
    bare_kmh: bool,
) -> Option<(f64, bool)> {
    way_reading_in(maxspeed, maxspeed_hgv, bare_kmh).map(|(mph, hgv, _)| (mph, hgv))
}

/// `way_limit_in`, plus the km/h on the sign when the tag read is in km/h.
pub fn way_reading_in(
    maxspeed: Option<&str>,
    maxspeed_hgv: Option<&str>,
    bare_kmh: bool,
) -> Option<(f64, bool, Option<i64>)> {
    let parse = |raw: &str| read_maxspeed(raw, bare_kmh);
    if let Some((mph, kmh)) = maxspeed_hgv.and_then(parse) {
        return Some((mph, true, kmh));
    }
    maxspeed.and_then(parse).map(|(mph, kmh)| (mph, false, kmh))
}

fn digit_groups(text: &str) -> Vec<&str> {
    DIGITS.find_iter(text).map(|m| m.as_str()).collect()
}

/// Whether any number in an OSM `ref` ("US 60;SR 77") is one of the leg's.
fn ref_matches_shield(route_ref: &str, shield: &[&str]) -> bool {
    !shield.is_empty() && digit_groups(route_ref).iter().any(|d| shield.contains(d))
}

/// The roads a label names: "I-55 / MS-7" is two, "I-80" one.
fn label_parts(highway: &str) -> impl Iterator<Item = &str> {
    highway.split('/').map(str::trim).filter(|p| !p.is_empty())
}

fn part_is_interstate(part: &str) -> bool {
    part.to_uppercase().starts_with("I-")
}

/// Whether the whole leg is an interstate: every road its label names is
/// one. A combined label ("I-55 / MS-7", "US-4 / I-89") rides other roads
/// too, so its interstate miles are found per sample instead
/// ([`interstate_numbers`]).
pub fn is_interstate(highway: &str) -> bool {
    let mut parts = label_parts(highway).peekable();
    parts.peek().is_some() && parts.all(part_is_interstate)
}

/// The interstate numbers in a label ("I-55 / MS-7" -> ["55"]).
fn interstate_numbers(highway: &str) -> Vec<&str> {
    label_parts(highway)
        .filter(|p| part_is_interstate(p))
        .flat_map(digit_groups)
        .collect()
}

/// Whether an OSM `ref` ("I 55;US 51") carries one of these interstate
/// numbers on an interstate shield, not just the same digits.
fn ref_carries_interstate(route_ref: &str, numbers: &[&str]) -> bool {
    route_ref.split(';').any(|r| {
        let r = r.trim().to_uppercase();
        (r.starts_with("I ") || r.starts_with("I-"))
            && digit_groups(&r).iter().any(|d| numbers.contains(d))
    })
}

/// One step of the profile. `mph` of `None` is a coverage gap; `assumed`
/// is the source of a jurisdiction's unposted default (Alaska's, a
/// province's), used where no posted way was found.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub at_mi: f64,
    pub mph: Option<i64>,
    pub hgv: bool,
    pub assumed: Option<&'static str>,
    /// The number on a km/h sign, as read (Canada); `mph` is its rounded
    /// conversion, kept for readers that want one.
    pub kmh: Option<i64>,
}

/// How far past a country border the first posting read can sit and still
/// be moved back to the border: four samples. The guard either side of the
/// line and a way that starts short of it leave a sample or two unread; a
/// longer stretch with nothing read is coverage missing, not the border.
const BORDER_SNAP_MI: f64 = 1.0;

/// The coverage-aware step function for one leg, before the anchor repair.
/// `regions` says which samples are in Canada (bare numbers are km/h),
/// which are in Alaska (no posted way means the 55 mph default), and where
/// the country borders are (a posting that changes there starts there).
pub fn bake_leg(
    highway: &str,
    line: &LegLine,
    leg_miles: f64,
    index: &WayIndex,
    regions: &LegRegions,
) -> Vec<Sample> {
    let raw_mi = line.length_m() / M_PER_MI;
    let scale = if leg_miles > 0.0 && raw_mi > 0.0 {
        leg_miles / raw_mi
    } else {
        1.0
    };
    let mile = |m: f64| round_py_n(m / M_PER_MI * scale, 1);
    let shield = digit_groups(highway);
    let interstate = is_interstate(highway);
    // A combined label's interstate numbers: there the street floor holds
    // only where one of its interstates is in the corridor.
    let mixed_interstates = if interstate {
        Vec::new()
    } else {
        interstate_numbers(highway)
    };
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
        if regions.near_border(at_mi) {
            continue;
        }
        let canada = regions.in_canada(at_mi);
        // The nearest way that carries the leg's route number, else the
        // nearest of any; ways whose tag has no number do not count.
        // mph, hgv, on_shield, dist, sign km/h
        let mut best: Option<(f64, bool, bool, f64, Option<i64>)> = None;
        let mut on_interstate = interstate;
        for (way, d) in index.near(lat, lon, MATCH_CORRIDOR_M + 1e-9, coslat) {
            if !mixed_interstates.is_empty()
                && ref_carries_interstate(&way.route_ref, &mixed_interstates)
            {
                on_interstate = true;
            }
            let Some((mph, hgv)) = way.limit(canada) else {
                continue;
            };
            let on_shield = ref_matches_shield(&way.route_ref, &shield);
            let better = match best {
                None => true,
                Some((_, _, b_on, b_d, _)) => {
                    (on_shield && !b_on) || (on_shield == b_on && d < b_d)
                }
            };
            if better {
                best = Some((mph, hgv, on_shield, d, way.sign_kmh(canada)));
            }
        }
        let read = best
            .filter(|b| !(on_interstate && b.0 < INTERSTATE_MIN_PLAUSIBLE_MPH))
            .map(|(mph, hgv, _, _, kmh)| (mph as i64, hgv, None, kmh));
        let unposted = || {
            regions
                .unposted_default(at_mi)
                .map(|(mph, source)| (mph, false, Some(source), None))
        };
        let Some((mph, hgv, assumed, kmh)) = read.or_else(unposted) else {
            hole_start.get_or_insert(at_mi);
            continue;
        };
        if let Some(start) = hole_start.take() {
            if samples.last().is_some_and(|s| s.mph.is_some()) && at_mi - start > SPEED_GAP_MI {
                samples.push(Sample {
                    at_mi: start,
                    mph: None,
                    hgv: false,
                    assumed: None,
                    kmh: None,
                });
            }
        }
        if samples.last().is_some_and(|s| {
            s.mph == Some(mph) && s.hgv == hgv && s.assumed == assumed && s.kmh == kmh
        }) {
            continue;
        }
        // The new limit is posted at the border, not where the next sample
        // past it happened to fall.
        let at_mi = samples
            .last()
            .and_then(|prev| regions.border_between(prev.at_mi, at_mi))
            .filter(|border| at_mi - border <= BORDER_SNAP_MI)
            .map_or(at_mi, |border| round_py_n(border, 1));
        let row = Sample {
            at_mi,
            mph: Some(mph),
            hgv,
            assumed,
            kmh,
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
                assumed: None,
                kmh: None,
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
/// A read posting carries `source`; an unposted default carries its own.
pub fn world_rows(samples: &[Sample], source: &str) -> Vec<Value> {
    samples
        .iter()
        .map(|s| {
            let mut row = Object::new();
            row.insert("at_mi", Value::Float(s.at_mi));
            match s.mph {
                Some(mph) => {
                    row.insert("mph", Value::Int(i128::from(mph)));
                    if let Some(kmh) = s.kmh {
                        row.insert("kmh", Value::Int(i128::from(kmh)));
                    }
                    let source = s.assumed.unwrap_or(source);
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

    fn parse_maxspeed(raw: &str) -> Option<f64> {
        parse_maxspeed_in(raw, false)
    }

    fn way_limit(maxspeed: Option<&str>, hgv: Option<&str>) -> Option<(f64, bool)> {
        way_limit_in(maxspeed, hgv, false)
    }

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
        assert_eq!(parse_maxspeed_in("90", true), Some(55.0));
        assert_eq!(parse_maxspeed_in("50 mph", true), Some(50.0));
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
            ca_mph: Some(mph),
            ca_hgv: false,
            ca_kmh: None,
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
        bake_in(ways, highway, "")
    }

    /// Bake the test leg with these `state_crossings` (JSON array body).
    fn bake_in(ways: Vec<OsmWay>, highway: &str, crossings: &str) -> Vec<Sample> {
        let line = leg_line();
        let index = WayIndex::build(ways, &corridor_cells(std::iter::once(&line)));
        let miles = line.length_m() / M_PER_MI;
        let leg = crate::pyjson::loads(&format!(
            r#"{{"miles": {miles}, "corridor": {{"state_crossings": [{crossings}]}}}}"#
        ))
        .unwrap();
        let canadian = ["Yukon".to_string(), "British Columbia".to_string()]
            .into_iter()
            .collect();
        let regions = LegRegions::of(&leg, &line, &canadian);
        bake_leg(highway, &line, miles, &index, &regions)
    }

    /// A way whose bare "90" reads 85 mph in the US and 55 in Canada.
    fn bare_ninety(id: i64, lon0: f64, lon1: f64) -> OsmWay {
        OsmWay {
            ca_mph: Some(55.0),
            ca_kmh: Some(90),
            ..way(id, "1", 85.0, 40.0, lon0, lon1)
        }
    }

    #[test]
    fn a_bare_number_is_kmh_in_canada() {
        let yukon = r#"{"at_mi": 0.0, "from_state": "Yukon", "state": "Yukon"}"#;
        let samples = bake_in(vec![bare_ninety(1, -100.1, -99.5)], "AK-2", yukon);
        assert_eq!(samples.len(), 1, "{samples:?}");
        assert_eq!(samples[0].mph, Some(55));
        // The sign's own number is kept beside its rounded conversion.
        assert_eq!(samples[0].kmh, Some(90));
        assert!(samples[0].assumed.is_none());
    }

    #[test]
    fn a_kmh_reading_keeps_the_number_on_the_sign() {
        assert_eq!(read_maxspeed("100", true), Some((60.0, Some(100))));
        assert_eq!(read_maxspeed("70 km/h", false), Some((45.0, Some(70))));
        assert_eq!(read_maxspeed("55 mph", true), Some((55.0, None)));
        assert_eq!(read_maxspeed("55", false), Some((55.0, None)));
        assert_eq!(
            way_reading_in(Some("100"), Some("90"), true),
            Some((55.0, true, Some(90)))
        );
    }

    #[test]
    fn alaska_with_no_posted_way_takes_the_55_default() {
        let alaska = r#"{"at_mi": 0.0, "from_state": "Alaska", "state": "Alaska"}"#;
        let samples = bake_in(Vec::new(), "AK-2", alaska);
        assert_eq!(samples.len(), 1, "{samples:?}");
        assert_eq!((samples[0].at_mi, samples[0].mph), (0.0, Some(55)));
        assert!(samples[0].assumed.is_some());
    }

    #[test]
    fn british_columbia_with_no_posted_way_stays_a_gap() {
        // An untagged Hart Highway is a posted road OpenStreetMap is silent
        // on, not an unposted one: no statutory default is filled in.
        let bc = r#"{"at_mi": 0.0, "from_state": "British Columbia", "state": "British Columbia"}"#;
        assert!(bake_in(Vec::new(), "Alaska Highway", bc).is_empty());
    }

    #[test]
    fn yukon_has_no_fixed_default_so_an_unposted_road_stays_a_gap() {
        let yukon = r#"{"at_mi": 0.0, "from_state": "Yukon", "state": "Yukon"}"#;
        assert!(bake_in(Vec::new(), "Alaska Highway", yukon).is_empty());
    }

    #[test]
    fn a_limit_that_changes_at_the_border_starts_at_the_border() {
        // Alaska's 65 to mile ~9.7, a bare Yukon "90" from ~10.3; the line
        // between them at mile 10.
        let border = r#"{"at_mi": 10.0, "from_state": "Alaska", "state": "Yukon"}"#;
        let samples = bake_in(
            vec![
                way(1, "AK 2", 65.0, 40.0, -100.01, -99.815),
                bare_ninety(2, -99.805, -99.5),
            ],
            "AK-2",
            border,
        );
        let posted: Vec<(f64, Option<i64>)> = samples.iter().map(|s| (s.at_mi, s.mph)).collect();
        assert_eq!(posted, vec![(0.0, Some(65)), (10.0, Some(55))]);
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
    fn a_combined_label_keeps_town_limits_off_its_interstate() {
        // The first half is I-55 at 70; the second half is a town street at
        // 30 with no interstate in the corridor.
        let ways = vec![
            way(1, "I 55", 70.0, 40.0, -100.1, -99.81),
            way(2, "", 30.0, 40.0, -99.81, -99.5),
        ];
        let samples = bake(ways.clone(), "I-55 / MS-7");
        let mph: Vec<_> = samples.iter().map(|s| s.mph).collect();
        assert_eq!(mph, [Some(70), Some(30)], "{samples:?}");
        // A whole-interstate label still refuses the street: past the
        // interstate there is only a gap.
        let samples = bake(ways, "I-55");
        let mph: Vec<_> = samples.iter().map(|s| s.mph).collect();
        assert_eq!(mph, [Some(70), None], "{samples:?}");
    }

    #[test]
    fn a_combined_label_keeps_the_floor_beside_its_interstate() {
        // A 30 mph frontage road nearer the line than I-89, which posts no
        // limit: on the interstate stretch the street reading is refused.
        let frontage = way(1, "", 30.0, 40.0, -100.1, -99.5);
        let mut interstate = way(2, "I 89", 65.0, 40.0005, -100.1, -99.5);
        interstate.mph = None;
        interstate.ca_mph = None;
        let samples = bake(vec![frontage, interstate], "US-4 / I-89");
        assert!(samples.is_empty(), "{samples:?}");
    }

    #[test]
    fn interstate_means_every_road_in_the_label() {
        assert!(is_interstate("I-80"));
        assert!(is_interstate("I-95 / I-295"));
        assert!(!is_interstate("I-55 / MS-7"));
        assert!(!is_interstate("RI-138 / I-95"));
        assert!(!is_interstate("US-60"));
        assert_eq!(interstate_numbers("RI-138 / I-95"), ["95"]);
        assert!(ref_carries_interstate("US 51;I 55", &["55"]));
        assert!(!ref_carries_interstate("MS 55", &["55"]));
    }

    #[test]
    fn repair_drops_a_town_anchor_the_road_contradicts() {
        let s = |at_mi, mph| Sample {
            at_mi,
            mph: Some(mph),
            hgv: false,
            assumed: None,
            kmh: None,
        };
        let mut samples = vec![s(0.0, 25), s(22.0, 65)];
        assert_eq!(repair("US-60", 40.0, &mut samples), 1);
        assert_eq!(samples, vec![s(22.0, 65)]);
        let mut town = vec![s(0.0, 25), s(2.0, 65)];
        assert_eq!(repair("US-60", 40.0, &mut town), 0);
        let mut interstate = vec![s(0.0, 70), s(3.0, 40), s(4.0, 70)];
        assert_eq!(repair("I-55", 40.0, &mut interstate), 1);
        // A combined label is not an interstate leg: its town limits stay.
        let mut mixed = vec![s(0.0, 70), s(50.0, 55), s(58.7, 30)];
        assert_eq!(repair("I-55 / MS-7", 59.0, &mut mixed), 0);
    }

    #[test]
    fn world_rows_keep_the_existing_key_order() {
        let rows = world_rows(
            &[
                Sample {
                    at_mi: 0.0,
                    mph: Some(65),
                    hgv: true,
                    assumed: None,
                    kmh: None,
                },
                Sample {
                    at_mi: 3.0,
                    mph: Some(60),
                    hgv: false,
                    assumed: None,
                    kmh: Some(100),
                },
                Sample {
                    at_mi: 6.5,
                    mph: None,
                    hgv: false,
                    assumed: None,
                    kmh: None,
                },
                Sample {
                    at_mi: 9.0,
                    mph: Some(55),
                    hgv: false,
                    assumed: Some("assumed"),
                    kmh: None,
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
                r#"{"at_mi": 3.0, "mph": 60, "kmh": 100, "source": "src", "hgv": false}"#,
                r#"{"at_mi": 6.5, "mph": null}"#,
                r#"{"at_mi": 9.0, "mph": 55, "source": "assumed", "hgv": false}"#,
            ]
        );
    }
}
