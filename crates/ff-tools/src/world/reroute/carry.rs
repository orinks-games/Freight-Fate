//! Carrying a leg's records across a reroute.
//!
//! A reroute usually changes some of a leg's road and keeps the rest: the
//! Tri-State fix moves 25 of Kenosha to Chicago's 62 miles and leaves the
//! I-94 at either end alone. A record read along the old road (an exit, a
//! river, a lane count, a traffic count) is still true where the new road is
//! the same pavement, and is a description of a road the truck no longer
//! drives where it is not. So each record is found on the OLD road, by its
//! own coordinates where it has them and by its mile where it does not, and
//! kept only if the NEW road runs within [`SAME_ROAD_M`] of that spot; its
//! mile is then re-read on the new road. Everything else is dropped, and the
//! layer builders put back what the new road carries.

use ff_core::pyfmt::round_py_n;

use super::super::osm::geo::M_PER_MI;
use super::shape;
use crate::pyjson::Value;

/// How near the new road must pass for a record to be on the same road.
/// The archive may sit up to 90 m off its road on a long straight; the
/// detours this exists for run a kilometre or more from the old line.
pub const SAME_ROAD_M: f64 = 150.0;

/// A road with its leg mileage, to find and place records on.
pub struct Line<'a> {
    pub coords: &'a [(f64, f64)],
    pub cum_m: Vec<f64>,
    pub miles: f64,
}

impl<'a> Line<'a> {
    pub fn new(coords: &'a [(f64, f64)], miles: f64) -> Self {
        Self {
            coords,
            cum_m: shape::cumulative_m(coords),
            miles,
        }
    }

    fn scale(&self) -> f64 {
        let raw = self.cum_m[self.cum_m.len() - 1] / M_PER_MI;
        if raw > 0.0 && self.miles > 0.0 {
            self.miles / raw
        } else {
            1.0
        }
    }

    /// `(lat, lon)` at a leg mile.
    pub fn point_at(&self, mile: f64) -> (f64, f64) {
        let m = mile / self.scale() * M_PER_MI;
        let i = self
            .cum_m
            .partition_point(|c| *c < m)
            .clamp(1, self.coords.len() - 1);
        let span = self.cum_m[i] - self.cum_m[i - 1];
        let t = if span > 0.0 {
            ((m - self.cum_m[i - 1]) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (a, b) = (self.coords[i - 1], self.coords[i]);
        (a.1 + t * (b.1 - a.1), a.0 + t * (b.0 - a.0))
    }

    /// `(metres off, leg mile)` of a point projected onto this road.
    pub fn place(&self, lat: f64, lon: f64) -> (f64, f64) {
        let (off, run) = shape::project(self.coords, &self.cum_m, lat, lon);
        (off, (run / M_PER_MI * self.scale()).clamp(0.0, self.miles))
    }
}

fn coords_of(record: &Value) -> Option<(f64, f64)> {
    Some((record.get("lat")?.as_f64()?, record.get("lon")?.as_f64()?))
}

fn set(record: &mut Value, key: &str, mile: f64) {
    if let Some(o) = record.as_object_mut() {
        o.insert(key, Value::Float(round_py_n(mile, 1)));
    }
}

/// The new mile of one position on the old road, if the new road is there.
fn carry_mile(old: &Line, new: &Line, mile: f64) -> Option<f64> {
    let (lat, lon) = old.point_at(mile);
    let (off, at) = new.place(lat, lon);
    (off <= SAME_ROAD_M).then_some(at)
}

/// What became of a layer.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Carried {
    pub kept: usize,
    pub dropped: usize,
}

/// Carry one corridor layer of point records (`at_mi`) or spans
/// (`start_mi`/`end_mi`). A span is kept when both ends are on the new road
/// and it is still about as long, so a lane count does not stretch across a
/// detour that left the road between its ends.
pub fn carry_layer(rows: &[Value], old: &Line, new: &Line) -> (Vec<Value>, Carried) {
    let mut out = Vec::new();
    let mut tally = Carried::default();
    for row in rows {
        let mut row = row.clone();
        let kept = if let (Some(s), Some(e)) = (
            row.get("start_mi").and_then(Value::as_f64),
            row.get("end_mi").and_then(Value::as_f64),
        ) {
            match (carry_mile(old, new, s), carry_mile(old, new, e)) {
                (Some(ns), Some(ne))
                    if ne > ns && ((ne - ns) - (e - s)).abs() <= 0.2 + 0.1 * (e - s) =>
                {
                    set(&mut row, "start_mi", ns);
                    set(&mut row, "end_mi", ne);
                    round_py_n(ne, 1) > round_py_n(ns, 1)
                }
                _ => false,
            }
        } else if let Some(at) = row.get("at_mi").and_then(Value::as_f64) {
            let placed = match coords_of(&row) {
                Some((lat, lon)) => {
                    let (off, mile) = new.place(lat, lon);
                    // A record with its own coordinates is judged by them,
                    // with the same allowance the old road had for it.
                    let (old_off, _) = old.place(lat, lon);
                    (off <= old_off + SAME_ROAD_M).then_some(mile)
                }
                None => carry_mile(old, new, at),
            };
            match placed {
                Some(mile) => {
                    set(&mut row, "at_mi", mile);
                    true
                }
                None => false,
            }
        } else {
            false
        };
        if kept {
            tally.kept += 1;
            out.push(row);
        } else {
            tally.dropped += 1;
        }
    }
    let key = |r: &Value| {
        r.get("at_mi")
            .or_else(|| r.get("start_mi"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    out.sort_by(|a, b| key(a).total_cmp(&key(b)));
    (out, tally)
}

/// Re-place records that carry their own coordinates (checkpoints, stops)
/// on the new road: kept within `max_off_mi`, with the distance noted.
/// A record and how far (miles) it sits from the new road.
pub type Placed = (Value, f64);

pub fn reposition(rows: &[Value], new: &Line, max_off_mi: f64) -> (Vec<Placed>, Vec<Placed>) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for row in rows {
        let Some((lat, lon)) = coords_of(row) else {
            dropped.push((row.clone(), f64::INFINITY));
            continue;
        };
        let (off_m, mile) = new.place(lat, lon);
        let off_mi = off_m / M_PER_MI;
        if off_mi > max_off_mi {
            dropped.push((row.clone(), off_mi));
            continue;
        }
        let mut row = row.clone();
        set(&mut row, "at_mi", mile);
        kept.push((row, off_mi));
    }
    kept.sort_by(|a, b| {
        let at = |r: &Value| r.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
        at(&a.0).total_cmp(&at(&b.0))
    });
    (kept, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: &[(&str, f64)]) -> Value {
        let mut o = crate::pyjson::Object::new();
        for (k, v) in pairs {
            o.insert(*k, Value::Float(*v));
        }
        Value::Object(o)
    }

    /// Old road: due south 0.2 deg. New road: the same, but bowing 0.05 deg
    /// (about 4 km) east over its middle half.
    type Road = Vec<(f64, f64)>;

    fn roads() -> (Road, Road) {
        let old: Vec<(f64, f64)> = (0..=40).map(|i| (-88.0, 42.6 - i as f64 * 0.005)).collect();
        let new: Vec<(f64, f64)> = (0..=40)
            .map(|i| {
                let east = if (10..=30).contains(&i) { 0.05 } else { 0.0 };
                (-88.0 + east, 42.6 - i as f64 * 0.005)
            })
            .collect();
        (old, new)
    }

    #[test]
    fn records_on_shared_road_stay_and_records_on_the_detour_go() {
        let (old_c, new_c) = roads();
        let old = Line::new(&old_c, 13.8);
        let raw = shape::cumulative_m(&new_c);
        let new = Line::new(&new_c, (raw[raw.len() - 1] / M_PER_MI).round());
        let rows = vec![
            obj(&[("at_mi", 1.0)]),
            obj(&[("at_mi", 7.0)]),
            obj(&[("at_mi", 13.0)]),
            obj(&[("start_mi", 0.2), ("end_mi", 2.0)]),
            obj(&[("start_mi", 2.0), ("end_mi", 12.0)]),
        ];
        let (kept, tally) = carry_layer(&rows, &old, &new);
        assert_eq!(
            tally,
            Carried {
                kept: 3,
                dropped: 2
            }
        );
        let points: Vec<f64> = kept
            .iter()
            .filter_map(|r| r.get("at_mi").and_then(Value::as_f64))
            .collect();
        assert_eq!(points.len(), 2, "{kept:?}");
        let (first, last) = (points[0], points[1]);
        assert!((first - 1.0).abs() < 0.2, "{first}");
        assert!(
            last > 13.0,
            "the last mile moves with the longer road: {last}"
        );
    }

    #[test]
    fn a_town_too_far_from_the_new_road_is_dropped() {
        let (_, new_c) = roads();
        let new = Line::new(&new_c, 14.0);
        let near = obj(&[("lat", 42.58), ("lon", -88.001)]);
        let far = obj(&[("lat", 42.5), ("lon", -88.3)]);
        let (kept, dropped) = reposition(&[near, far], &new, 3.0);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped.len(), 1);
    }
}
