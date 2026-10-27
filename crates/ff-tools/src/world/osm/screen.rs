//! Name the short speed-limit postings that are not on the leg's own highway.
//!
//! A port of the deleted `screen_limit_artifacts.py`. Where the route runs
//! over a ramp or turns onto a street for a few hundred yards, the bake picks
//! up that road's limit, and the driver hears the limit drop and come back
//! with nothing on the highway to explain it. This screen looks at the
//! drop-and-return postings the game would still play (after its dwell
//! filter) that last under [`MAX_RUN_MI`], and walks each one: every
//! [`STEP_MI`] it finds the nearest posted way within [`NEAR_M`] and asks
//! whether that way carries the leg's route number and posts the dropped
//! limit (`on_b`), carries it and posts another (`other_limit`), is another
//! road (`off_route`), or is missing (`no_road`, mostly ramps).
//!
//! A posting is on its highway when `on_b` covers [`ON_HIGHWAY_MIN`] of it;
//! otherwise it is named when one reason covers [`VERDICT_MIN`]. The bake is
//! never edited: the loader folds a named posting unless a village or town
//! is within `LIMIT_ARTIFACT_PLACE_GUARD_MI`. The thresholds were calibrated
//! on a hand-checked sample of 100 postings (2026-10-17).

use std::collections::BTreeMap;

use ff_core::data::world_constants::{
    LIMIT_ARTIFACT_MAX_RUN_MI, LIMIT_ARTIFACT_PLACE_CHECKPOINTS, LIMIT_ARTIFACT_PLACE_GUARD_MI,
    LIMIT_EXPLAINING_CATEGORIES,
};
use ff_core::data::world_models::SpeedLimitSample;
use ff_core::data::world_parsing::dwell_filter_speed_limits;
use ff_core::pyfmt::round_py_n;
use regex::Regex;
use std::sync::LazyLock;

use super::geo::{LegLine, WayIndex};
use crate::pyjson::{Object, Value};

pub const MAX_RUN_MI: f64 = LIMIT_ARTIFACT_MAX_RUN_MI;
pub const STEP_MI: f64 = 0.02;
/// Skip the posting's ends, where the bake's own sampling blurs.
const EDGE_MI: f64 = 0.05;
pub const NEAR_M: f64 = 40.0;
pub const ON_HIGHWAY_MIN: f64 = 0.3;
pub const VERDICT_MIN: f64 = 0.5;

pub const REASONS: &[(&str, &str)] = &[
    (
        "off_route",
        "the route there runs on a different road than the leg's highway",
    ),
    (
        "other_limit",
        "the leg's highway posts a different limit there",
    ),
    (
        "no_road",
        "no mainline way within 40 m of the route there (an interchange ramp)",
    ),
];

static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
static REF_PARTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[;/]").unwrap());

fn route_number(highway: &str) -> Option<&str> {
    DIGITS.find(highway).map(|m| m.as_str())
}

/// Whether an OSM `ref` ("US 19;GA 300") names this route number.
fn ref_carries(route_ref: &str, number: Option<&str>) -> bool {
    let Some(number) = number else { return false };
    REF_PARTS
        .split(route_ref)
        .any(|part| DIGITS.find(part).is_some_and(|m| m.as_str() == number))
}

/// One leg as the screen needs it, read from the indexed world data.
pub struct LegView<'a> {
    pub id: String,
    pub highway: String,
    pub miles: f64,
    pub corridor: &'a Value,
}

fn floats<'a>(
    corridor: &'a Value,
    list: &str,
    key: &'a str,
    keep: impl Fn(&str) -> bool + 'a,
) -> impl Iterator<Item = f64> + 'a {
    corridor
        .get(list)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(move |row| keep(&row.get(key).map(Value::py_str).unwrap_or_default()))
        .filter_map(|row| row.get("at_mi").and_then(Value::as_f64))
}

fn village_miles(corridor: &Value) -> Vec<f64> {
    floats(corridor, "landmarks", "category", |c| {
        LIMIT_EXPLAINING_CATEGORIES.contains(&c)
    })
    .collect()
}

/// Everything the loader's guard counts as a reason: village landmarks and
/// town checkpoints.
fn place_miles(corridor: &Value) -> Vec<f64> {
    let mut out = village_miles(corridor);
    out.extend(floats(corridor, "checkpoints", "type", |t| {
        LIMIT_ARTIFACT_PLACE_CHECKPOINTS.contains(&t)
    }));
    out
}

/// The postings the game keeps after its dwell filter.
fn played_profile(leg: &LegView) -> Vec<SpeedLimitSample> {
    let mut samples: Vec<SpeedLimitSample> = leg
        .corridor
        .get("speed_limits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            Some(SpeedLimitSample {
                at_mi: s.get("at_mi")?.as_f64()?,
                mph: s.get("mph").and_then(Value::as_f64),
                source: String::new(),
                hgv: false,
            })
        })
        .collect();
    samples.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    dwell_filter_speed_limits(&samples, leg.miles, &village_miles(leg.corridor))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub at_mi: f64,
    pub mph: f64,
    pub from_mph: f64,
    pub run_mi: f64,
}

/// Played drop-and-return postings shorter than [`MAX_RUN_MI`].
pub fn candidates_of(leg: &LegView) -> Vec<Candidate> {
    let profile = played_profile(leg);
    let mut out = Vec::new();
    for w in profile.windows(3) {
        let (Some(a), Some(b), Some(c)) = (w[0].mph, w[1].mph, w[2].mph) else {
            continue;
        };
        if a != c || b >= a {
            continue;
        }
        let run = w[2].at_mi - w[1].at_mi;
        if run < MAX_RUN_MI {
            out.push(Candidate {
                at_mi: w[1].at_mi,
                mph: b,
                from_mph: a,
                run_mi: round_py_n(run, 2),
            });
        }
    }
    out
}

fn place_guarded(leg: &LegView, cand: &Candidate) -> bool {
    let lo = cand.at_mi - LIMIT_ARTIFACT_PLACE_GUARD_MI;
    let hi = cand.at_mi + cand.run_mi + LIMIT_ARTIFACT_PLACE_GUARD_MI;
    place_miles(leg.corridor)
        .iter()
        .any(|at| *at >= lo && *at <= hi)
}

/// `(reason, fractions)` for one candidate; no reason means leave it.
fn classify(
    leg: &LegView,
    line: &LegLine,
    cand: &Candidate,
    index: &WayIndex,
) -> (Option<&'static str>, BTreeMap<&'static str, f64>) {
    let number = route_number(&leg.highway);
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut mile = cand.at_mi + EDGE_MI;
    let end = cand.at_mi + cand.run_mi - EDGE_MI;
    let mut steps = 0usize;
    while mile <= end + 1e-9 {
        let (lat, lon) = line.point_at(mile, leg.miles);
        let nearest = index
            .near(lat, lon, NEAR_M, lat.to_radians().cos())
            .into_iter()
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let kind = match nearest {
            None => "no_road",
            Some((way, _)) if ref_carries(&way.route_ref, number) => {
                if way.mph == Some(cand.mph) {
                    "on_b"
                } else {
                    "other_limit"
                }
            }
            Some(_) => "off_route",
        };
        *counts.entry(kind).or_default() += 1;
        steps += 1;
        mile += STEP_MI;
    }
    if steps == 0 {
        return (None, BTreeMap::new());
    }
    let fractions: BTreeMap<&'static str, f64> = counts
        .into_iter()
        .map(|(k, v)| (k, round_py_n(v as f64 / steps as f64, 3)))
        .collect();
    let share = |k: &str| fractions.get(k).copied().unwrap_or(0.0);
    if share("on_b") >= ON_HIGHWAY_MIN {
        return (None, fractions);
    }
    let reason = REASONS
        .iter()
        .map(|(k, _)| *k)
        .find(|k| share(k) >= VERDICT_MIN);
    (reason, fractions)
}

/// What the screen found, for the meta line and the run report.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Counts {
    pub candidates: usize,
    pub on_highway: usize,
    pub mixed: usize,
    pub named: usize,
    pub place_guarded: usize,
    pub folds: usize,
    /// Legs with candidates but no archived road to walk.
    pub skipped_candidates: usize,
}

/// Every named posting (rows in the file's shape) and the counts.
pub fn screen(
    legs: &[LegView],
    lines: &std::collections::HashMap<String, LegLine>,
    index: &WayIndex,
) -> (Vec<Value>, Counts) {
    let mut rows: Vec<(String, f64, Value)> = Vec::new();
    let mut counts = Counts::default();
    for leg in legs {
        let cands = candidates_of(leg);
        if cands.is_empty() {
            continue;
        }
        let Some(line) = lines.get(&leg.id) else {
            counts.skipped_candidates += cands.len();
            continue;
        };
        for cand in &cands {
            counts.candidates += 1;
            let (reason, fractions) = classify(leg, line, cand, index);
            let Some(reason) = reason else {
                if fractions.get("on_b").copied().unwrap_or(0.0) >= ON_HIGHWAY_MIN {
                    counts.on_highway += 1;
                } else {
                    counts.mixed += 1;
                }
                continue;
            };
            counts.named += 1;
            if place_guarded(leg, cand) {
                counts.place_guarded += 1;
            } else {
                counts.folds += 1;
            }
            let mut fr = Object::new();
            for (k, v) in &fractions {
                fr.insert(*k, Value::Float(*v));
            }
            let row = crate::pyobj! {
                "leg" => leg.id.as_str(),
                "highway" => leg.highway.as_str(),
                "at_mi" => cand.at_mi,
                "mph" => cand.mph,
                "from_mph" => cand.from_mph,
                "run_mi" => cand.run_mi,
                "why" => reason,
                "fractions" => fr,
            };
            rows.push((leg.id.clone(), cand.at_mi, Value::Object(row)));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    (rows.into_iter().map(|r| r.2).collect(), counts)
}

#[cfg(test)]
mod tests {
    use super::super::geo::{corridor_cells, OsmWay};
    use super::*;

    #[test]
    fn ref_carries_reads_each_concurrency() {
        assert!(ref_carries("US 19;GA 300", Some("300")));
        assert!(ref_carries("I 80/US 6", Some("6")));
        assert!(!ref_carries("US 195", Some("95")));
        assert!(!ref_carries("", None));
    }

    /// A 10-mile leg on latitude 40 posting 65, with a 45 for two miles in
    /// the middle (long enough to survive the dwell filter), read against one posted way of the given ref and limit.
    fn screen_one(way_ref: &str, way_mph: f64) -> (Vec<Value>, Counts) {
        let points = (0..=100)
            .map(|i| (40.0, -100.0 + f64::from(i) * 0.0019))
            .collect();
        let line = LegLine::new(points);
        let miles = line.length_m() / super::super::geo::M_PER_MI;
        let corridor = serde_json::json!({});
        let mut corridor = crate::pyjson::loads(&corridor.to_string()).unwrap();
        let limits = crate::pyjson::loads(
            r#"[{"at_mi": 0.0, "mph": 65}, {"at_mi": 4.0, "mph": 45}, {"at_mi": 6.0, "mph": 65}]"#,
        )
        .unwrap();
        if let Value::Object(o) = &mut corridor {
            o.insert("speed_limits", limits);
        }
        let leg = LegView {
            id: "a:b".into(),
            highway: "US-6".into(),
            miles,
            corridor: &corridor,
        };
        let way = OsmWay {
            id: 1,
            route_ref: way_ref.into(),
            mph: Some(way_mph),
            hgv: false,
            coords: vec![(40.0001, -100.1), (40.0001, -99.7)],
        };
        let index = WayIndex::build(vec![way], &corridor_cells(std::iter::once(&line)));
        let lines = std::iter::once(("a:b".to_string(), line)).collect();
        screen(&[leg], &lines, &index)
    }

    #[test]
    fn a_dip_the_highway_posts_is_left_alone() {
        let (rows, counts) = screen_one("US 6", 45.0);
        assert!(rows.is_empty());
        assert_eq!(counts.on_highway, 1);
    }

    #[test]
    fn a_dip_from_another_road_is_named() {
        let (rows, counts) = screen_one("", 45.0);
        assert_eq!(counts.named, 1);
        assert_eq!(counts.folds, 1);
        assert_eq!(
            rows[0].get("why").map(Value::py_str).as_deref(),
            Some("off_route")
        );
        assert_eq!(rows[0].get("at_mi").and_then(Value::as_f64), Some(4.0));
    }
}
