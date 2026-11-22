//! The named bridges a leg drives over.
//!
//! `corridor.bridges` is written by `cargo tools osm-bridges`: one record
//! per named bridge on the road the leg drives, in the leg's native frame.
//! The name is read from OpenStreetMap (`bridge:name`, the name of a
//! `man_made=bridge` outline or of a `type=bridge` relation) or from the
//! curated sheet in `docs/bridges/`; the mileage is derived by projecting
//! the bridge's ways onto the leg's archived road. Each record's `source`
//! says which.
//!
//! A record reads `{"from_mi", "to_mi", "name", "over", "ways", "osm",
//! "curated", "source"}`: `name` is the spoken form ("Confederation
//! Bridge"), `over` the water or thing it spans when known, `ways` the
//! directions it holds for when the two carriageways differ (`both` by
//! default), `osm` the elements it was read from.
//!
//! Screened on the way in, never edited in the bake (see `CLAUDE.md`,
//! "Provenance"): a span that ends before it starts, runs off the leg, or
//! is longer than [`MAX_BRIDGE_MI`] is dropped and counted in
//! [`Bridges::screened`].

use serde::Deserialize;
use serde_json::Value;

use crate::data::pavement::Ways;
use crate::data::world_models::DataError;

/// Longer than any bridge on the map's roads, with room for the builder's
/// projection: the Manchac Swamp Bridge on I-55 is 22.8 mi, the Lake
/// Pontchartrain Causeway 23.8 mi (neither is on a truck leg today; the
/// longest on the map is the Confederation Bridge, 8.0 mi).
pub const MAX_BRIDGE_MI: f64 = 25.0;

/// How far past the leg's end a span may run: the archive's projection
/// rounds to the hundredth.
const END_SLACK_MI: f64 = 0.01;

/// One named bridge, native frame.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedBridge {
    pub from_mi: f64,
    pub to_mi: f64,
    /// The spoken name, "Confederation Bridge".
    pub name: String,
    /// What it spans, "Northumberland Strait", when known.
    pub over: String,
    /// The span is over water, so the river bed plays for its length.
    pub water: bool,
    pub ways: Ways,
    /// From the curated sheet rather than the length bar alone.
    pub curated: bool,
    pub osm: String,
    pub source: String,
}

impl NamedBridge {
    pub fn length_mi(&self) -> f64 {
        self.to_mi - self.from_mi
    }
}

/// A leg's whole layer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bridges {
    pub bridges: Vec<NamedBridge>,
    /// Records the load screens dropped.
    pub screened: usize,
}

impl Bridges {
    /// The bridges a truck heading `forward` crosses, in mile order.
    pub fn for_direction(&self, forward: bool) -> impl Iterator<Item = &NamedBridge> {
        self.bridges.iter().filter(move |b| b.ways.holds(forward))
    }
}

#[derive(Deserialize)]
struct Raw {
    from_mi: f64,
    to_mi: f64,
    name: String,
    #[serde(default)]
    over: String,
    #[serde(default)]
    water: bool,
    #[serde(default)]
    ways: Option<String>,
    #[serde(default)]
    curated: bool,
    #[serde(default)]
    osm: String,
    #[serde(default)]
    source: String,
}

/// Parse `corridor.bridges`; empty when the leg has none.
pub fn parse_bridges(
    raw: Option<&Value>,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Bridges, DataError> {
    let Some(items) = raw.filter(|v| !v.is_null()) else {
        return Ok(Bridges::default());
    };
    let items = items.as_array().ok_or_else(|| {
        DataError::value(format!("{from_city} to {to_city} bridges must be a list"))
    })?;
    let mut out = Bridges::default();
    for item in items {
        let r: Raw = serde_json::from_value(item.clone()).map_err(|e| {
            DataError::value(format!("{from_city} to {to_city} bridge {item}: {e}"))
        })?;
        let ways = match r.ways.as_deref() {
            None => Ways::Both,
            Some(w) => Ways::from_name(w).ok_or_else(|| {
                DataError::value(format!(
                    "{from_city} to {to_city} bridge {:?} holds for {w:?}, not both, forward or \
                     reverse",
                    r.name
                ))
            })?,
        };
        if r.name.trim().is_empty() {
            return Err(DataError::value(format!(
                "{from_city} to {to_city} bridge at {} has no name",
                r.from_mi
            )));
        }
        let sane = r.from_mi.is_finite()
            && r.to_mi.is_finite()
            && r.from_mi >= 0.0
            && r.to_mi > r.from_mi
            && r.to_mi <= leg_miles + END_SLACK_MI
            && r.to_mi - r.from_mi <= MAX_BRIDGE_MI;
        if !sane {
            out.screened += 1;
            continue;
        }
        out.bridges.push(NamedBridge {
            from_mi: r.from_mi,
            to_mi: r.to_mi,
            name: r.name.trim().to_string(),
            over: r.over,
            water: r.water,
            ways,
            curated: r.curated,
            osm: r.osm,
            source: r.source,
        });
    }
    out.bridges.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(raw: Value, miles: f64) -> Bridges {
        parse_bridges(Some(&raw), miles, "a", "b").unwrap()
    }

    #[test]
    fn a_bridge_resolves_by_direction() {
        let b = parse(
            json!([
                {"from_mi": 5.0, "to_mi": 6.2, "name": "Blue Water Bridge", "ways": "forward",
                 "over": "St. Clair River", "water": true},
                {"from_mi": 5.1, "to_mi": 6.3, "name": "Blue Water Bridge", "ways": "reverse"},
                {"from_mi": 1.0, "to_mi": 1.5, "name": "Garden City Skyway"},
            ]),
            10.0,
        );
        let fwd: Vec<f64> = b.for_direction(true).map(|x| x.from_mi).collect();
        let rev: Vec<f64> = b.for_direction(false).map(|x| x.from_mi).collect();
        assert_eq!(fwd, vec![1.0, 5.0]);
        assert_eq!(rev, vec![1.0, 5.1]);
        assert!(b.bridges[1].water);
        assert_eq!(b.bridges[0].name, "Garden City Skyway");
    }

    #[test]
    fn implausible_spans_are_screened_and_counted() {
        let b = parse(
            json!([
                {"from_mi": 3.0, "to_mi": 2.0, "name": "Backwards Bridge"},
                {"from_mi": 9.0, "to_mi": 10.5, "name": "Off The End Bridge"},
                {"from_mi": 0.0, "to_mi": 30.0, "name": "Too Long Bridge"},
                {"from_mi": 9.0, "to_mi": 10.005, "name": "Kept Bridge"},
            ]),
            10.0,
        );
        assert_eq!(b.screened, 3);
        assert_eq!(b.bridges.len(), 1);
        assert_eq!(b.bridges[0].name, "Kept Bridge");
    }

    #[test]
    fn a_malformed_record_fails_the_load() {
        for raw in [
            json!({"from_mi": 1.0}),
            json!([{"from_mi": 1.0, "to_mi": 2.0}]),
            json!([{"from_mi": 1.0, "to_mi": 2.0, "name": " "}]),
            json!([{"from_mi": 1.0, "to_mi": 2.0, "name": "X Bridge", "ways": "up"}]),
        ] {
            assert!(parse_bridges(Some(&raw), 10.0, "a", "b").is_err(), "{raw}");
        }
        assert!(parse_bridges(None, 10.0, "a", "b")
            .unwrap()
            .bridges
            .is_empty());
    }
}
