//! Which states a rerouted leg runs through, and where it crosses each line:
//! the archived road intersected with OpenStreetMap's state boundaries.

use ff_core::pyfmt::round_py_n;

use super::super::osm::geo::M_PER_MI;
use super::roads::{Boundary, Network};

/// How far a reroute may move a line and still keep the name the leg
/// carried for it. A new road seldom moves a crossing more than a few
/// miles; past this, a same-named pair is a different crossing (the British
/// Columbia and Yukon line is crossed five times near Lower Post).
const CARRY_PLACE_MI: f64 = 10.0;

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
pub fn state_context(
    coords: &[(f64, f64)],
    cum_m: &[f64],
    leg_miles: f64,
    start_state: &str,
    network: &Network,
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
    let mut used: Vec<usize> = Vec::new();
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
        // A name the leg already carried for this line (often hand-cited,
        // such as a port of entry) outlives the reroute: the nearest one of
        // the same pair within `CARRY_PLACE_MI`, each once.
        let carried = old_places
            .iter()
            .enumerate()
            .filter(|(k, c)| {
                !used.contains(k)
                    && c.from_state == current
                    && &c.state == next
                    && (c.at_mi - at_mi).abs() <= CARRY_PLACE_MI
            })
            .min_by(|(_, a), (_, b)| (a.at_mi - at_mi).abs().total_cmp(&(b.at_mi - at_mi).abs()))
            .map(|(k, c)| (k, c.place.clone()));
        // With no cited name the place stays empty and the game says the
        // short "Crossing into {state}." over the crossing sound; the line's
        // own name is never written as its place.
        let place = match carried {
            Some((k, place)) => {
                used.push(k);
                place
            }
            None => String::new(),
        };
        crossings.push(Crossing {
            at_mi,
            from_state: current.clone(),
            state: next.clone(),
            place,
        });
        current = next.clone();
    }
    let crossings = fold_short_excursions(crossings);
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

/// A dip into the next jurisdiction and straight back, shorter than this,
/// is not spoken: the Alaska Highway rides the 60th parallel past Lower Post
/// and leaves Yukon for 0.2 mile at a time, which would be two lines a few
/// seconds apart for no change the driver can act on.
pub const MIN_EXCURSION_MI: f64 = 0.5;

/// Drop each crossing whose next one returns to where it came from within
/// [`MIN_EXCURSION_MI`]; the miles stay with the state either side.
pub fn fold_short_excursions(crossings: Vec<Crossing>) -> Vec<Crossing> {
    let mut out: Vec<Crossing> = Vec::with_capacity(crossings.len());
    for c in crossings {
        if let Some(last) = out.last() {
            if c.state == last.from_state
                && c.from_state == last.state
                && c.at_mi - last.at_mi < MIN_EXCURSION_MI
            {
                out.pop();
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_dip_across_a_line_and_back_is_not_a_crossing() {
        let c = |at: f64, from: &str, to: &str| Crossing {
            at_mi: at,
            from_state: from.into(),
            state: to.into(),
            place: String::new(),
        };
        let folded = fold_short_excursions(vec![
            c(274.1, "British Columbia", "Yukon"),
            c(274.3, "Yukon", "British Columbia"),
            c(276.2, "British Columbia", "Yukon"),
            c(281.0, "Yukon", "British Columbia"),
            c(311.7, "British Columbia", "Yukon"),
        ]);
        let at: Vec<f64> = folded.iter().map(|c| c.at_mi).collect();
        assert_eq!(at, vec![276.2, 281.0, 311.7]);
    }

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

    #[test]
    fn a_name_the_leg_carried_for_a_line_outlives_the_reroute() {
        let coords = vec![(-87.9, 42.6), (-87.9, 42.4)];
        let cum = super::super::shape::cumulative_m(&coords);
        let network = Network {
            index: Default::default(),
            roads: Default::default(),
            boundaries: vec![Boundary {
                coords: vec![(42.4924, -88.5), (42.4924, -87.0)],
                states: vec!["Wisconsin".into(), "Illinois".into()],
            }],
        };
        let cited = Crossing {
            at_mi: 7.0,
            from_state: "Wisconsin".into(),
            state: "Illinois".into(),
            place: "the I-94 state line at Russell".into(),
        };
        let miles = cum[cum.len() - 1] / M_PER_MI;
        let (_, kept) = state_context(&coords, &cum, miles, "Wisconsin", &network, &[cited]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].place, "the I-94 state line at Russell");
        let (_, fresh) = state_context(&coords, &cum, miles, "Wisconsin", &network, &[]);
        assert_eq!(fresh[0].place, "");
        let far = Crossing {
            at_mi: 40.0,
            ..kept[0].clone()
        };
        let (_, elsewhere) = state_context(&coords, &cum, miles, "Wisconsin", &network, &[far]);
        assert_eq!(elsewhere[0].place, "");
    }
}
