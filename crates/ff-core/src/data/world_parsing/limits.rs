//! The baked maxspeed profile: parse, order, and the real-seconds dwell
//! filter that drops way-boundary slivers (the speed-limit half of
//! `world_parsing.py`).

use std::collections::HashMap;

use serde_json::Value;

use super::records::object;
use super::{get_bool, get_str, parse_at_mi, py_float};
use crate::data::world_constants::{
    LIMIT_ARTIFACT_MAX_RUN_MI, LIMIT_ARTIFACT_PLACE_GUARD_MI, LIMIT_DWELL_FALLBACK_MPH,
    LIMIT_DWELL_FULL_COMPRESSION_MPH, LIMIT_DWELL_LOW_SPEED_SCALE, LIMIT_DWELL_REAL_S,
    LIMIT_DWELL_REFERENCE_SCALE, LIMIT_PLACE_DWELL_REAL_S, LIMIT_PLACE_NEAR_MI,
    LIMIT_PLACE_TOWN_MPH,
};
use crate::data::world_models::{DataError, SpeedLimitSample};
use crate::pyfmt::py_str_float;

pub fn parse_speed_limit(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<SpeedLimitSample, DataError> {
    let raw = object(raw, from_city, to_city, "speed limit")?;
    let at_mi = parse_at_mi(raw, leg_miles, from_city, to_city, "speed limit", true)?;
    let mph = match raw.get("mph") {
        None => return Err(DataError::key("'mph'")),
        // Coverage-gap marker: OSM tagging ends here; the runtime reverts to
        // the highway/region heuristic instead of holding the last posting.
        Some(Value::Null) => None,
        Some(v) => {
            let mph = py_float(v)?;
            if !(5.0..=85.0).contains(&mph) {
                return Err(DataError::value(format!(
                    "{from_city} to {to_city} speed limit has unrealistic mph {}",
                    py_str_float(mph)
                )));
            }
            Some(mph)
        }
    };
    Ok(SpeedLimitSample {
        at_mi,
        mph,
        source: get_str(raw, "source"),
        hgv: get_bool(raw, "hgv", false),
    })
}

/// Parse the baked maxspeed profile, ordered along the leg.
///
/// Sorting by `at_mi` lets the runtime treat it as a step function without
/// trusting the order the samples happen to be stored in. The dwell filter
/// then drops the postings that are way boundaries rather than signs.
pub fn parse_speed_limits(
    raw_samples: &[Value],
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
    places: &[f64],
) -> Result<Vec<SpeedLimitSample>, DataError> {
    let mut samples: Vec<SpeedLimitSample> = raw_samples
        .iter()
        .map(|s| parse_speed_limit(s, leg_miles, from_city, to_city))
        .collect::<Result<_, _>>()?;
    // Python's sort is stable; so is `sort_by`.
    samples.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite at_mi"));
    Ok(dwell_filter_speed_limits(&samples, leg_miles, places))
}

/// How much road `seconds` of real time is, at a posting's own speed.
///
/// Compression is not one number: the trip's clock ramps from crawling pace up
/// to the configured pacing at highway speed, so slow road already runs closer
/// to real time. That is what makes a seconds-sized bar the right one here --
/// it asks for two and a quarter miles of a 70 and two thirds of a mile of a
/// 30, which is very close to the difference between a way boundary and a
/// village main street.
pub fn limit_dwell_floor_mi(seconds: f64, mph: Option<f64>) -> f64 {
    let speed = mph.unwrap_or(LIMIT_DWELL_FALLBACK_MPH).max(5.0);
    let ramp = (speed / LIMIT_DWELL_FULL_COMPRESSION_MPH).min(1.0);
    let scale = LIMIT_DWELL_LOW_SPEED_SCALE
        + (LIMIT_DWELL_REFERENCE_SCALE - LIMIT_DWELL_LOW_SPEED_SCALE) * ramp;
    seconds * speed * scale / 3600.0
}

/// Drop postings too short to be signage, so the limit stops flickering.
///
/// OSM splits a way wherever any tag changes, so the baked profile carries
/// postings that hold for a few hundred feet -- an 80 that drops to 45 and
/// back over four tenths of a mile is a way boundary, not a sign, and no
/// agency posts one. Real driving hides them: a tenth of a mile is seconds of
/// a real hour. Time compression does not, and the player hears the limit
/// reduce and normalize with nothing on the road to explain it (reported
/// 2026-08-11, and again on 2026-08-12 after the first attempt at this).
///
/// That first attempt asked for a fixed MILE, and a mile is not one
/// experience: at 70 the truck is through it in under three real seconds and
/// at 30 it takes over ten. So the bar is real seconds now -- the same law
/// the keeper ease, the turn call and the zone warning already follow -- and
/// converting it back to miles at each posting's own speed does for free what
/// the mile bar needed an exception to do, because slow road is barely
/// compressed. A 70 has to hold for over two miles; a 30 needs two thirds of
/// one.
///
/// The exception survives for the case seconds alone still cannot judge: a
/// genuinely short main street. A place within `LIMIT_PLACE_NEAR_MI` halves
/// the bar, but only for a drop to a speed a place actually posts. Shaving
/// five off a highway limit beside a village is not the village's doing, and
/// the old unconditional pass is what kept 763 sub-three-second postings,
/// including quarter-mile interstate trims.
///
/// Sibling of the timezone dwell filter and the state-crossing sanitizer: the
/// same "a boundary that does not last is not a boundary" rule, applied to
/// the third profile baked out of way geometry. The run is measured against
/// the last posting KEPT, so a chain of slivers collapses whole rather than
/// each one surviving by measuring only its neighbour.
pub fn dwell_filter_speed_limits(
    samples: &[SpeedLimitSample],
    leg_miles: f64,
    places: &[f64],
) -> Vec<SpeedLimitSample> {
    let Some(first) = samples.first() else {
        return Vec::new();
    };
    let mut kept: Vec<SpeedLimitSample> = vec![first.clone()];
    for (i, sample) in samples.iter().enumerate().skip(1) {
        let run_end = samples.get(i + 1).map(|s| s.at_mi).unwrap_or(leg_miles);
        // A place explains a REDUCTION TO A TOWN SPEED and nothing else.
        // Passing through Strawberry is why the number drops to 35; it is never
        // why the road briefly allows more, and never why an 80 becomes a 75.
        let last = kept.last().expect("kept starts non-empty");
        let lowers = matches!((sample.mph, last.mph), (Some(s), Some(k)) if s < k);
        let town = lowers && sample.mph.is_some_and(|m| m <= LIMIT_PLACE_TOWN_MPH);
        let explained = town
            && places
                .iter()
                .any(|at| (sample.at_mi - at).abs() <= LIMIT_PLACE_NEAR_MI);
        let dwell_s = if explained {
            LIMIT_PLACE_DWELL_REAL_S
        } else {
            LIMIT_DWELL_REAL_S
        };
        if run_end - sample.at_mi < limit_dwell_floor_mi(dwell_s, sample.mph) {
            continue; // gone before it can be a sign; the last one kept carries on
        }
        if sample.mph == last.mph {
            continue; // a way boundary that changed nothing else
        }
        kept.push(sample.clone());
    }
    kept
}

/// One posting `tools/screen_limit_artifacts.py` named: the route geometry
/// there is not on the leg's own highway, so the limit it carries is a ramp's
/// or a side street's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlaggedLimit {
    pub at_mi: f64,
    pub mph: f64,
}

/// The named postings in `limit_artifacts.jsonl`, by leg id (`from:to`).
///
/// A missing or empty file names nothing, the same fail-open the curve
/// artifact list takes. A row that does not parse is skipped: the list is
/// tooling output, and a bad row must not stop the world loading.
pub fn flagged_limits_from(text: &str) -> HashMap<String, Vec<FlaggedLimit>> {
    let mut out: HashMap<String, Vec<FlaggedLimit>> = HashMap::new();
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let (Some(leg), Some(at_mi), Some(mph)) = (
            row.get("leg").and_then(Value::as_str),
            row.get("at_mi").and_then(Value::as_f64),
            row.get("mph").and_then(Value::as_f64),
        ) else {
            continue; // the meta line, or a malformed row
        };
        out.entry(leg.to_string())
            .or_default()
            .push(FlaggedLimit { at_mi, mph });
    }
    out
}

/// Fold named drop-and-return postings back into their neighbours.
///
/// Runs on the profile the dwell filter kept. A posting goes when the limit
/// drops from A to B and returns to A within `LIMIT_ARTIFACT_MAX_RUN_MI`, the
/// offline screen named B (OpenStreetMap shows the route there on a ramp or a
/// different road, not on the leg's highway), and nothing on the map explains
/// it: no village or town within `LIMIT_ARTIFACT_PLACE_GUARD_MI` of either
/// end. The return to A goes with it, so A simply carries on.
///
/// The bake is never edited: the screen names postings and this decides, so
/// the guard can be re-judged without re-reading OpenStreetMap. Returns the
/// profile and how many postings it folded.
pub fn fold_limit_artifacts(
    kept: Vec<SpeedLimitSample>,
    flagged: &[FlaggedLimit],
    places: &[f64],
) -> (Vec<SpeedLimitSample>, usize) {
    if flagged.is_empty() || kept.len() < 3 {
        return (kept, 0);
    }
    let mut out: Vec<SpeedLimitSample> = Vec::with_capacity(kept.len());
    let mut folded = 0;
    let mut i = 0;
    while i < kept.len() {
        let sample = &kept[i];
        if let (Some(before), Some(after)) = (out.last(), kept.get(i + 1)) {
            let named = sample.mph.is_some_and(|mph| {
                flagged
                    .iter()
                    .any(|f| f.mph == mph && (f.at_mi - sample.at_mi).abs() < 0.005)
            });
            let returns = before.mph.is_some()
                && before.mph == after.mph
                && matches!((sample.mph, before.mph), (Some(b), Some(a)) if b < a);
            let short = after.at_mi - sample.at_mi < LIMIT_ARTIFACT_MAX_RUN_MI;
            let explained = places.iter().any(|at| {
                *at >= sample.at_mi - LIMIT_ARTIFACT_PLACE_GUARD_MI
                    && *at <= after.at_mi + LIMIT_ARTIFACT_PLACE_GUARD_MI
            });
            if named && returns && short && !explained {
                folded += 1;
                i += 2; // the posting and the return to the limit before it
                continue;
            }
        }
        out.push(sample.clone());
        i += 1;
    }
    (out, folded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at_mi: f64, mph: Option<f64>) -> SpeedLimitSample {
        SpeedLimitSample {
            at_mi,
            mph,
            source: String::new(),
            hgv: false,
        }
    }

    #[test]
    fn dwell_filter_drops_slivers_and_keeps_a_village_main_street() {
        // An 80 that drops to 45 and back inside four tenths of a mile is a way
        // boundary; with no place to explain it, it goes.
        let kept = dwell_filter_speed_limits(
            &[
                sample(0.0, Some(80.0)),
                sample(10.0, Some(45.0)),
                sample(10.4, Some(80.0)),
            ],
            50.0,
            &[],
        );
        assert_eq!(kept.len(), 1);
        // The same drop beside a village survives the shorter bar: three real
        // seconds of a 35 is 0.44 miles, and this main street holds for half a
        // mile.
        let kept = dwell_filter_speed_limits(
            &[
                sample(0.0, Some(80.0)),
                sample(10.0, Some(35.0)),
                sample(10.5, Some(80.0)),
            ],
            50.0,
            &[10.2],
        );
        assert_eq!(
            kept.iter().map(|s| s.mph).collect::<Vec<_>>(),
            vec![Some(80.0), Some(35.0), Some(80.0)]
        );
    }

    fn flag(at_mi: f64, mph: f64) -> FlaggedLimit {
        FlaggedLimit { at_mi, mph }
    }

    fn mphs(samples: &[SpeedLimitSample]) -> Vec<Option<f64>> {
        samples.iter().map(|s| s.mph).collect()
    }

    fn reno_ramp() -> Vec<SpeedLimitSample> {
        vec![
            sample(0.0, Some(65.0)),
            sample(12.0, Some(45.0)),
            sample(12.9, Some(65.0)),
            sample(30.0, Some(55.0)),
        ]
    }

    #[test]
    fn a_named_ramp_posting_folds_into_the_highway_limit() {
        let (kept, folded) = fold_limit_artifacts(reno_ramp(), &[flag(12.0, 45.0)], &[]);
        assert_eq!(folded, 1);
        assert_eq!(mphs(&kept), vec![Some(65.0), Some(55.0)]);
        assert_eq!(kept[1].at_mi, 30.0);
    }

    #[test]
    fn a_town_beside_the_posting_keeps_it() {
        // The guard is the map's own reason, judged at load: a town within a
        // quarter mile of either end keeps the posting even when it is named.
        for place in [11.8, 12.4, 13.1] {
            let (kept, folded) = fold_limit_artifacts(reno_ramp(), &[flag(12.0, 45.0)], &[place]);
            assert_eq!(folded, 0, "place at {place}");
            assert_eq!(kept.len(), 4);
        }
        let (_, folded) = fold_limit_artifacts(reno_ramp(), &[flag(12.0, 45.0)], &[13.2]);
        assert_eq!(folded, 1, "a town past the guard does not explain it");
    }

    #[test]
    fn only_a_named_short_drop_and_return_folds() {
        // Not named: kept.
        let (_, folded) = fold_limit_artifacts(reno_ramp(), &[flag(12.0, 35.0)], &[]);
        assert_eq!(folded, 0);
        // Named, but the limit does not come back to where it was.
        let steps = vec![
            sample(0.0, Some(65.0)),
            sample(12.0, Some(45.0)),
            sample(12.9, Some(55.0)),
        ];
        assert_eq!(fold_limit_artifacts(steps, &[flag(12.0, 45.0)], &[]).1, 0);
        // Named, but a rise rather than a drop.
        let rise = vec![
            sample(0.0, Some(45.0)),
            sample(12.0, Some(65.0)),
            sample(12.9, Some(45.0)),
        ];
        assert_eq!(fold_limit_artifacts(rise, &[flag(12.0, 65.0)], &[]).1, 0);
        // Named, but it holds for the screen's longest span or more.
        let long = vec![
            sample(0.0, Some(65.0)),
            sample(12.0, Some(45.0)),
            sample(14.5, Some(65.0)),
        ];
        assert_eq!(fold_limit_artifacts(long, &[flag(12.0, 45.0)], &[]).1, 0);
        // The first posting has nothing before it to return to.
        let first = vec![sample(0.0, Some(45.0)), sample(0.9, Some(65.0))];
        assert_eq!(fold_limit_artifacts(first, &[flag(0.0, 45.0)], &[]).1, 0);
    }

    #[test]
    fn the_list_reader_skips_the_meta_line_and_bad_rows() {
        let text = concat!(
            r#"{"meta": {"layer": "limit_artifacts"}}"#,
            "\n",
            r#"{"leg": "reno_nv_us:sacramento_ca_us", "at_mi": 12.0, "mph": 45.0}"#,
            "\n",
            "not json\n",
            r#"{"leg": "reno_nv_us:sacramento_ca_us", "at_mi": 20.5, "mph": 25}"#,
            "\n",
        );
        let flagged = flagged_limits_from(text);
        assert_eq!(flagged.len(), 1);
        assert_eq!(
            flagged["reno_nv_us:sacramento_ca_us"],
            vec![flag(12.0, 45.0), flag(20.5, 25.0)]
        );
    }
}
