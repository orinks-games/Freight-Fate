//! Which states a rerouted leg runs through, and where it crosses each line:
//! the archived road intersected with OpenStreetMap's state boundaries.

use ff_core::pyfmt::round_py_n;

use super::super::osm::geo::M_PER_MI;
use super::roads::{Boundary, Network};
use super::valhalla::matches_shield;

/// A crossing's road is read within this distance of the line.
const PLACE_ROAD_M: f64 = 150.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub at_mi: f64,
    pub from_state: String,
    pub state: String,
    pub place: String,
}

/// The parameter along `p1 -> p2` where it crosses `q1 -> q2`, if it does.
fn intersect(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> Option<f64> {
    let r = (p2.0 - p1.0, p2.1 - p1.1);
    let s = (q2.0 - q1.0, q2.1 - q1.1);
    let denom = r.0 * s.1 - r.1 * s.0;
    if denom.abs() < 1e-15 {
        return None;
    }
    let qp = (q1.0 - p1.0, q1.1 - p1.1);
    let t = (qp.0 * s.1 - qp.1 * s.0) / denom;
    let u = (qp.0 * r.1 - qp.1 * r.0) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// Every place the road crosses a line between two states, in order, as
/// `(metres along, the two states)`. Boundaries are `(lat, lon)`; the road
/// is `(lon, lat)`.
fn events(coords: &[(f64, f64)], cum_m: &[f64], borders: &[Boundary]) -> Vec<(f64, Vec<String>)> {
    let (mut lo_lat, mut hi_lat, mut lo_lon, mut hi_lon) = (90.0, -90.0, 180.0, -180.0);
    for &(lon, lat) in coords {
        lo_lat = f64::min(lo_lat, lat);
        hi_lat = f64::max(hi_lat, lat);
        lo_lon = f64::min(lo_lon, lon);
        hi_lon = f64::max(hi_lon, lon);
    }
    let mut out = Vec::new();
    for border in borders.iter().filter(|b| b.states.len() == 2) {
        for q in border.coords.windows(2) {
            let (q1, q2) = ((q[0].1, q[0].0), (q[1].1, q[1].0));
            if q1.0.max(q2.0) < lo_lon
                || q1.0.min(q2.0) > hi_lon
                || q1.1.max(q2.1) < lo_lat
                || q1.1.min(q2.1) > hi_lat
            {
                continue;
            }
            for i in 1..coords.len() {
                if let Some(t) = intersect(coords[i - 1], coords[i], q1, q2) {
                    let m = cum_m[i - 1] + t * (cum_m[i] - cum_m[i - 1]);
                    out.push((m, border.states.clone()));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// `(state miles, crossings)` for the road, starting in `start_state`.
#[allow(clippy::too_many_arguments)]
pub fn state_context(
    coords: &[(f64, f64)],
    cum_m: &[f64],
    leg_miles: f64,
    start_state: &str,
    network: &Network,
    highway: &str,
    old_places: &[Crossing],
) -> (Vec<(String, f64)>, Vec<Crossing>) {
    let raw_mi = cum_m[cum_m.len() - 1] / M_PER_MI;
    let scale = if raw_mi > 0.0 {
        leg_miles / raw_mi
    } else {
        1.0
    };
    let mut current = start_state.to_string();
    let mut crossings: Vec<Crossing> = Vec::new();
    for (m, states) in events(coords, cum_m, &network.boundaries) {
        let Some(next) = states.iter().find(|s| **s != current) else {
            continue;
        };
        if !states.contains(&current) {
            continue;
        }
        let at_mi = round_py_n(m / M_PER_MI * scale, 1);
        // A road riding the line itself crosses it back and forth within a
        // few metres; one crossing and its immediate return are dropped.
        if let Some(last) = crossings.last() {
            if &last.from_state == next && at_mi - last.at_mi < 0.05 {
                crossings.pop();
                current = next.clone();
                continue;
            }
        }
        let (lon, lat) = point_at(coords, cum_m, m);
        let on_label = network.any_ref_near(lat, lon, PLACE_ROAD_M, |r| matches_shield(r, highway));
        let place = if on_label {
            format!("{current}-{next} line on {highway}")
        } else {
            old_places
                .iter()
                .find(|c| c.from_state == current && &c.state == next)
                .map(|c| c.place.clone())
                .unwrap_or_else(|| format!("{current}-{next} line"))
        };
        crossings.push(Crossing {
            at_mi,
            from_state: current.clone(),
            state: next.clone(),
            place,
        });
        current = next.clone();
    }
    let mut miles = Vec::new();
    let mut state = start_state.to_string();
    let mut from = 0.0;
    for c in &crossings {
        miles.push((state.clone(), round_py_n(c.at_mi - from, 1)));
        state = c.state.clone();
        from = c.at_mi;
    }
    miles.push((state, round_py_n(leg_miles - from, 1)));
    (miles, crossings)
}

fn point_at(coords: &[(f64, f64)], cum_m: &[f64], m: f64) -> (f64, f64) {
    let i = cum_m.partition_point(|c| *c < m).clamp(1, coords.len() - 1);
    let span = cum_m[i] - cum_m[i - 1];
    let t = if span > 0.0 {
        ((m - cum_m[i - 1]) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (a, b) = (coords[i - 1], coords[i]);
    (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_that_cross_report_where() {
        let t = intersect((0.0, 0.0), (2.0, 0.0), (1.0, -1.0), (1.0, 1.0)).unwrap();
        assert!((t - 0.5).abs() < 1e-12);
        assert!(intersect((0.0, 0.0), (0.5, 0.0), (1.0, -1.0), (1.0, 1.0)).is_none());
    }

    #[test]
    fn a_road_south_across_a_parallel_line_changes_state_once() {
        let coords = vec![(-87.9, 42.6), (-87.9, 42.4)];
        let cum = super::super::shape::cumulative_m(&coords);
        let border = Boundary {
            coords: vec![(42.4924, -88.5), (42.4924, -87.0)],
            states: vec!["Wisconsin".into(), "Illinois".into()],
        };
        let found = events(&coords, &cum, &[border]);
        assert_eq!(found.len(), 1);
        let mi = found[0].0 / M_PER_MI;
        assert!((mi - 7.4).abs() < 0.1, "{mi}");
    }
}
