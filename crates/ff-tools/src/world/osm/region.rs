//! Which jurisdiction each mile of a leg lies in, for the rules that depend
//! on it.
//!
//! * A bare `maxspeed` number ("90") is km/h in Canada and mph in the United
//!   States. A way whose own tags say which (`maxspeed:type` "CA:...",
//!   a `ref` like "BC 97") is read that way wherever it is; only an unmarked
//!   number takes the unit of the stretch the sample lies in.
//! * Alaska's unposted roads default to 55 mph (13 AAC 02.275), so an Alaska
//!   stretch with no posted way along it takes 55, labelled assumed, instead
//!   of falling back to the game's highway heuristic.
//! * A posting that changes at a country border starts at the border.
//!
//! The stretches come from the leg's own `state_crossings` (their `at_mi`),
//! else its `state_miles`. Where the road crosses the 49th parallel or the
//! 141st meridian, the border mile is measured on the road itself instead:
//! the corridor's crossing miles were placed by hand from port coordinates,
//! some a few miles out.

use std::collections::HashSet;

use super::geo::{LegLine, M_PER_MI};
use crate::pyjson::{Object, Value};

pub const ALASKA: &str = "Alaska";
/// The unposted default on Alaska's roads, for every vehicle.
pub const ALASKA_DEFAULT_MPH: i64 = 55;
/// Samples this close to a country border are not read: the way beside
/// them may be the other country's, in the other country's unit.
pub const BORDER_GUARD_MI: f64 = 0.1;

/// One stretch of a leg, from `start_mi` to the next stretch.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start_mi: f64,
    pub state: String,
    pub canada: bool,
}

/// The stretches of one leg, in order, and the miles its country borders sit at.
#[derive(Debug, Clone, Default)]
pub struct LegRegions {
    pub spans: Vec<Span>,
    /// Each country border on the leg: the corridor's mile and, where the
    /// road crosses a straight border line, the mile measured on the road.
    pub borders: Vec<Border>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Border {
    pub corridor_mi: f64,
    pub measured_mi: Option<f64>,
}

impl Border {
    pub fn mile(&self) -> f64 {
        self.measured_mi.unwrap_or(self.corridor_mi)
    }
}

/// The spoken names of the Canadian provinces and territories in `geo`.
pub fn canadian_states(world: &Object) -> HashSet<String> {
    world
        .get("geo")
        .and_then(|g| g.get("countries"))
        .and_then(|c| c.get("CA"))
        .and_then(|ca| ca.get("states"))
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Object::iter)
        .map(|(_, name)| name.py_str())
        .collect()
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).map(Value::py_str).unwrap_or_default()
}

/// The straight border line a crossing between these two places runs
/// along, if there is one: `(is_meridian, degrees)`.
fn border_line(a: &str, b: &str) -> Option<(bool, f64)> {
    const PARALLEL_49: &[&str] = &[
        "Washington",
        "Idaho",
        "Montana",
        "North Dakota",
        "Minnesota",
        "British Columbia",
        "Alberta",
        "Saskatchewan",
        "Manitoba",
    ];
    let pair = |x: &str, y: &str| (a == x && b == y) || (a == y && b == x);
    if pair(ALASKA, "Yukon") {
        return Some((true, -141.0));
    }
    if PARALLEL_49.contains(&a) && PARALLEL_49.contains(&b) {
        return Some((false, 49.0));
    }
    None
}

/// The mile, on the leg's own scale, where `line` crosses the border line
/// nearest `near_mi`.
fn measure_border(
    line: &LegLine,
    leg_miles: f64,
    meridian: bool,
    deg: f64,
    near_mi: f64,
) -> Option<f64> {
    let raw_mi = line.length_m() / M_PER_MI;
    if raw_mi <= 0.0 || leg_miles <= 0.0 {
        return None;
    }
    let scale = leg_miles / raw_mi;
    let coord = |p: (f64, f64)| if meridian { p.1 } else { p.0 };
    let mut best: Option<f64> = None;
    for (i, pair) in line.points.windows(2).enumerate() {
        let (c0, c1) = (coord(pair[0]) - deg, coord(pair[1]) - deg);
        if c0 == c1 || (c0 < 0.0) == (c1 < 0.0) {
            continue;
        }
        let t = c0 / (c0 - c1);
        let m0 = line.cum_m[i];
        let m1 = line.cum_m[i + 1];
        let mile = (m0 + t * (m1 - m0)) / M_PER_MI * scale;
        if best.is_none_or(|b| (mile - near_mi).abs() < (b - near_mi).abs()) {
            best = Some(mile);
        }
    }
    best
}

impl LegRegions {
    /// The stretches of `leg`. `line` is the road it drives, used to measure
    /// country borders; `canadian` names the Canadian provinces.
    pub fn of(leg: &Value, line: &LegLine, canadian: &HashSet<String>) -> Self {
        let corridor = leg.get("corridor");
        let leg_miles = leg.get("miles").and_then(Value::as_f64).unwrap_or(0.0);
        let mut raw: Vec<(f64, String)> = Vec::new();
        let crossings: Vec<&Value> = corridor
            .and_then(|c| c.get("state_crossings"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|c| c.get("at_mi").and_then(Value::as_f64).is_some())
            .collect();
        if let Some(first) = crossings.first() {
            raw.push((0.0, text(first, "from_state")));
            for c in &crossings {
                raw.push((
                    c.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0),
                    text(c, "state"),
                ));
            }
        } else {
            let mut run = 0.0;
            for entry in corridor
                .and_then(|c| c.get("state_miles"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                raw.push((run, text(entry, "state")));
                run += entry.get("miles").and_then(Value::as_f64).unwrap_or(0.0);
            }
        }
        let mut regions = LegRegions::default();
        for (start_mi, state) in raw {
            if state.is_empty() || regions.spans.last().is_some_and(|s| s.state == state) {
                continue;
            }
            let canada = canadian.contains(&state);
            if let Some(prev) = regions.spans.last() {
                if prev.canada != canada {
                    let measured = border_line(&prev.state, &state).and_then(|(meridian, deg)| {
                        measure_border(line, leg_miles, meridian, deg, start_mi)
                    });
                    regions.borders.push(Border {
                        corridor_mi: start_mi,
                        measured_mi: measured,
                    });
                }
            }
            regions.spans.push(Span {
                start_mi,
                state,
                canada,
            });
        }
        // A measured border moves the stretch boundary with it.
        let mut b = 0;
        for i in 1..regions.spans.len() {
            if regions.spans[i - 1].canada != regions.spans[i].canada {
                if let Some(border) = regions.borders.get(b) {
                    regions.spans[i].start_mi = border.mile();
                }
                b += 1;
            }
        }
        regions
    }

    fn span_at(&self, mile: f64) -> Option<&Span> {
        self.spans
            .iter()
            .rev()
            .find(|s| s.start_mi <= mile)
            .or(self.spans.first())
    }

    pub fn in_canada(&self, mile: f64) -> bool {
        self.span_at(mile).is_some_and(|s| s.canada)
    }

    pub fn in_alaska(&self, mile: f64) -> bool {
        self.span_at(mile).is_some_and(|s| s.state == ALASKA)
    }

    /// Whether `mile` is too close to a country border to read.
    pub fn near_border(&self, mile: f64) -> bool {
        self.borders
            .iter()
            .any(|b| (mile - b.mile()).abs() < BORDER_GUARD_MI)
    }

    /// The country border between two miles, if one lies between them.
    pub fn border_between(&self, from_mi: f64, to_mi: f64) -> Option<f64> {
        self.borders
            .iter()
            .map(Border::mile)
            .find(|m| from_mi < *m && *m <= to_mi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pyjson;

    fn canadian() -> HashSet<String> {
        ["British Columbia", "Yukon"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn crossings_set_the_stretches_and_the_road_sets_the_border() {
        // Ten miles due east along latitude 62, crossing 141 W near mile 2.7;
        // the corridor puts the line at mile 6.
        let line = LegLine::new(
            (0..=100)
                .map(|i| (62.0, -141.08 + f64::from(i) * 0.003))
                .collect(),
        );
        let leg = pyjson::loads(
            r#"{"miles": 10, "corridor": {"state_crossings": [
                {"at_mi": 6.0, "from_state": "Alaska", "state": "Yukon"}]}}"#,
        )
        .unwrap();
        let regions = LegRegions::of(&leg, &line, &canadian());
        let border = regions.borders[0].mile();
        let expected = 0.08 / 0.3 * 10.0;
        assert!((border - expected).abs() < 0.05, "{border} vs {expected}");
        assert!(regions.in_alaska(1.0) && !regions.in_canada(1.0));
        assert!(regions.in_canada(5.0) && !regions.in_alaska(5.0));
        assert!(regions.near_border(border + 0.05));
        assert_eq!(regions.border_between(1.0, 5.0), Some(border));
    }

    #[test]
    fn state_miles_stand_in_when_there_are_no_crossings() {
        let line = LegLine::new(vec![(56.0, -121.0), (56.5, -121.0)]);
        let leg = pyjson::loads(
            r#"{"miles": 45, "corridor": {"state_miles": [
                {"state": "Yukon", "miles": 12.0}, {"state": "British Columbia", "miles": 33.0}]}}"#,
        )
        .unwrap();
        let regions = LegRegions::of(&leg, &line, &canadian());
        assert_eq!(regions.spans.len(), 2);
        assert_eq!(regions.spans[1].start_mi, 12.0);
        assert!(regions.borders.is_empty());
        assert!(regions.in_canada(0.0) && regions.in_canada(40.0));
    }
}
