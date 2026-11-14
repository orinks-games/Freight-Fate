//! `osm-villages`: the towns and villages a leg passes, as `village`
//! landmarks the ride-along names.
//!
//! A port of the deleted `tools/bake_villages.py` and its place extractor
//! (`tools/extract_osm_places.py`). Named `place=town|village` nodes (never
//! hamlets: a handful of houses is not somewhere the driver arrives, and
//! never a ghost town or abandoned place, see [`is_gone`]) within
//! 12 miles of the road are projected onto it, nearest first, at most 30 a
//! leg. Each keeps its `off_mi`, so the game can name only the ones the road
//! reaches and still answer "eleven miles ahead" for the rest. Dropped: a
//! place whose name another landmark, a checkpoint or either end city
//! already speaks; a place within 4 miles of a dispatch city (20 miles when
//! the names match), which is that city; a name a screen reader cannot read
//! as one place ("Camp Verde / Matthi:wa"). A village the road runs through
//! (half a mile or closer) that sits just before a speed drop to 45 or less
//! moves to 0.2 mile ahead of the drop, so its name explains the limit.
//! Run after `osm-landmarks`.

use std::collections::{BTreeMap, HashSet};

use super::features::{self, tag, Select};
use super::landmarks::clean_name;
use super::{corridor_mut, leg_id, leg_miles, pbf_paths, round, Grid, Only, Route, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;
use crate::world::osm::geo::{haversine_m, M_PER_MI};

const USAGE: &str = "usage: osm-villages --pbf FILE [--pbf FILE ...] [--only from:to;from:to] \
                     [--extract-date YYYY-MM-DD] [--snapshot NAME] [--write]";

pub const CATCHMENT_MI: f64 = 12.0;
pub const CITY_DEDUPE_MI: f64 = 4.0;
pub const CITY_NAME_DEDUPE_MI: f64 = 20.0;
pub const TOWN_ZONE_MPH: i64 = 45;
pub const ZONE_PAIR_WINDOW_MI: f64 = 1.5;
pub const ZONE_PAIR_LEAD_MI: f64 = 0.2;
pub const MAX_PER_LEG: usize = 30;
pub const ENTER_OFF_MI: f64 = 0.5;
const UNSPEAKABLE: &[char] = &['/', ':', ';', '|', '(', ')', '='];
const CELL_DEG: f64 = 0.25;

#[derive(Debug, Clone)]
pub struct Place {
    pub id: i64,
    pub name: String,
    pub rank: String,
    pub lat: f64,
    pub lon: f64,
}

fn norm(name: &str) -> String {
    name.to_lowercase()
        .replace(['.', '-', '\u{2013}'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn speakable(name: &str) -> bool {
    !name.contains(UNSPEAKABLE)
}

/// Miles where a posted limit of 45 or less steps down.
pub fn zone_starts(leg: &Value) -> Vec<f64> {
    let mut rows: Vec<(f64, Option<i64>)> = leg
        .get("corridor")
        .and_then(|c| c.get("speed_limits"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let at = s.get("at_mi").and_then(Value::as_f64)?;
            let mph = s.get("mph").and_then(Value::as_f64).map(|m| m as i64);
            Some((at, mph))
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut starts = Vec::new();
    let mut previous: Option<i64> = None;
    for (at, mph) in rows {
        let Some(mph) = mph else { continue };
        if mph <= TOWN_ZONE_MPH && previous.is_none_or(|p| mph < p) {
            starts.push(at);
        }
        previous = Some(mph);
    }
    starts
}

/// The mile for a village whose name explains a nearby drop.
pub fn paired_mile(at_mi: f64, starts: &[f64]) -> Option<f64> {
    let start = starts
        .iter()
        .filter(|s| (-0.5..=ZONE_PAIR_WINDOW_MI).contains(&(*s - at_mi)))
        .min_by(|a, b| (*a - at_mi).abs().total_cmp(&(*b - at_mi).abs()))?;
    Some(round(start - ZONE_PAIR_LEAD_MI, 1).max(0.0))
}

pub fn spoken(name: &str, off_mi: f64) -> String {
    if off_mi <= ENTER_OFF_MI {
        format!("Entering {name}")
    } else {
        format!("Passing {name}")
    }
}

/// Dispatch cities: `(lat, lon, normalized spoken name)`.
pub fn city_anchors(world: &Object) -> Vec<(f64, f64, String)> {
    let Some(cities) = world.get("cities").and_then(Value::as_object) else {
        return Vec::new();
    };
    cities
        .iter()
        .filter_map(|(slug, c)| {
            let lat = c.get("lat").and_then(Value::as_f64)?;
            let lon = c.get("lon").and_then(Value::as_f64)?;
            let name = c.get("spoken_city").and_then(Value::as_str).unwrap_or(slug);
            Some((lat, lon, norm(name)))
        })
        .collect()
}

/// Names the leg already speaks: other landmarks, checkpoints, both ends.
fn taken_names(leg: &Value, world: &Object) -> HashSet<String> {
    let corridor = leg.get("corridor");
    let names = |key: &str| -> Vec<String> {
        corridor
            .and_then(|c| c.get(key))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|r| {
                key != "landmarks" || r.get("category").and_then(Value::as_str) != Some("village")
            })
            .filter_map(|r| r.get("name").and_then(Value::as_str).map(norm))
            .collect()
    };
    let mut taken: HashSet<String> = names("landmarks").into_iter().collect();
    taken.extend(names("checkpoints"));
    for end in ["from", "to"] {
        let slug = leg.get(end).and_then(Value::as_str).unwrap_or("");
        let city = world.get("cities").and_then(|c| c.get(slug));
        let name = city
            .and_then(|c| c.get("spoken_city"))
            .and_then(Value::as_str)
            .unwrap_or(slug);
        taken.insert(norm(name));
    }
    taken
}

#[derive(Default)]
pub struct Counts {
    pub paired: usize,
    pub unspeakable: usize,
}

/// The leg's village records, in mile order.
pub fn bake_leg(
    leg: &Value,
    world: &Object,
    route: &Route,
    places: &[Place],
    grid: &Grid,
    anchors: &[(f64, f64, String)],
    counts: &mut Counts,
) -> Vec<Object> {
    let miles = leg_miles(leg);
    let taken = taken_names(leg, world);
    let reach = (CATCHMENT_MI / 69.0 / CELL_DEG).ceil() as i64 + 1;
    let mut best: BTreeMap<String, Object> = BTreeMap::new();
    for i in grid.along(&route.points, reach) {
        let place = &places[i as usize];
        let name = clean_name(&place.name);
        if name.is_empty() {
            continue;
        }
        if !speakable(&name) {
            counts.unspeakable += 1;
            continue;
        }
        let key = norm(&name);
        if taken.contains(&key) {
            continue;
        }
        let (at_mi, off_mi, _, _) = route.project(place.lat, place.lon);
        if off_mi > CATCHMENT_MI {
            continue;
        }
        let is_city = anchors.iter().any(|(lat, lon, city)| {
            let limit = if *city == key {
                CITY_NAME_DEDUPE_MI
            } else {
                CITY_DEDUPE_MI
            };
            haversine_m(place.lat, place.lon, *lat, *lon) / M_PER_MI <= limit
        });
        if is_city {
            continue;
        }
        let off = round(off_mi, 2);
        if best
            .get(&key)
            .and_then(|r| r.get("off_mi"))
            .and_then(Value::as_f64)
            .is_some_and(|prev| prev <= off)
        {
            continue;
        }
        best.insert(
            key,
            crate::pyobj! {
                "name" => name.as_str(),
                "at_mi" => round(at_mi.clamp(0.0, miles), 1),
                "off_mi" => off,
                "category" => "village",
                "kind" => "point",
                "source" => format!(
                    "OpenStreetMap place={} node {}, projected onto the leg's checked-in route geometry",
                    place.rank, place.id
                ),
            },
        );
    }
    let off = |r: &Object| r.get("off_mi").and_then(Value::as_f64).unwrap_or(0.0);
    let at = |r: &Object| r.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
    let mut records: Vec<Object> = best.into_values().collect();
    records.sort_by(|a, b| off(a).total_cmp(&off(b)));
    records.truncate(MAX_PER_LEG);
    let starts = zone_starts(leg);
    for r in &mut records {
        let (o, a) = (off(r), at(r));
        if o <= ENTER_OFF_MI {
            if let Some(adjusted) = paired_mile(a, &starts) {
                r.insert("at_mi", Value::Float(adjusted));
                let source = r.get("source").map(Value::py_str).unwrap_or_default();
                r.insert(
                    "source",
                    Value::Str(format!(
                        "{source}, placed ahead of the speed zone it explains"
                    )),
                );
                counts.paired += 1;
            }
        }
        let name = r.get("name").map(Value::py_str).unwrap_or_default();
        r.insert("spoken", Value::Str(spoken(&name, o)));
    }
    records.sort_by(|a, b| at(a).total_cmp(&at(b)));
    records
}

/// Tags that mark a place nobody lives in any more: a ghost town, a site
/// kept for its history, or a town abandoned or flooded. Mappers often keep
/// the old `place=village` on such a node, so the rank alone lets them in.
const GONE_KEYS: &[&str] = &[
    "abandoned",
    "abandoned:place",
    "disused",
    "disused:place",
    "historic:place",
    "was:place",
    "demolished:place",
    "destroyed:place",
    "end_date",
];

/// `historic=*` values that mean the place itself is gone. Not `district`:
/// a living New England village is often a historic district too.
const GONE_HISTORIC: &[&str] = &[
    "ghost_town",
    "abandoned",
    "deserted_village",
    "ruins",
    "archaeological_site",
    "settlement",
];

/// Whether a place node's tags say it is gone.
pub fn is_gone(t: &[(&str, &str)]) -> bool {
    let has = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
    let no = |v: &str| matches!(v, "no" | "0");
    GONE_KEYS.iter().any(|k| has(k).is_some_and(|v| !no(v)))
        || has("historic").is_some_and(|v| GONE_HISTORIC.contains(&v))
        || has("population").is_some_and(|v| v.trim() == "0")
        || has("name").is_some_and(|n| n.to_lowercase().contains("(historical)"))
}

pub fn read_places(pbfs: &[std::path::PathBuf]) -> Result<Vec<Place>> {
    let keep = |t: &[(&str, &str)]| {
        let has = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
        matches!(has("place"), Some("town" | "village"))
            && has("name").is_some_and(|n| !n.trim().is_empty())
            && !is_gone(t)
    };
    let found = features::read(
        pbfs,
        &Select {
            node: Some(&keep),
            way: None,
            relation: None,
        },
    )?;
    Ok(found
        .nodes
        .into_iter()
        .map(|n| Place {
            id: n.id,
            name: tag(&n.tags, "name").unwrap_or("").trim().to_string(),
            rank: tag(&n.tags, "place").unwrap_or("").to_string(),
            lat: n.lat,
            lon: n.lon,
        })
        .collect())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["pbf", "only", "extract-date", "snapshot"],
            flags: &["write"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let pbfs = pbf_paths(&args, USAGE)?;
    let _ = super::extract_date(&args, &pbfs)?;
    let only = Only::parse(&args);
    let mut run = Run::load()?;
    let places = read_places(&pbfs)?;
    println!("{} named towns and villages in the extracts", places.len());
    let mut grid = Grid::new(CELL_DEG);
    for (i, p) in places.iter().enumerate() {
        grid.insert_point(p.lat, p.lon, i as u32);
    }
    let anchors = city_anchors(&run.world);
    let world = run.world.clone();
    let archive = std::mem::take(&mut run.archive);
    let mut counts = Counts::default();
    let (mut changed, mut same, mut total, mut no_archive) = (0, 0, 0, 0);
    for leg in run.legs_mut()?.iter_mut() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        let Some(line) = archive.get(&id) else {
            no_archive += 1;
            continue;
        };
        let route = Route::new(line, leg_miles(leg));
        let records = bake_leg(leg, &world, &route, &places, &grid, &anchors, &mut counts);
        total += records.len();
        if only.is_some() {
            let list: Vec<String> = records
                .iter()
                .map(|r| {
                    format!(
                        "{} ({})",
                        r.get("spoken").map(Value::py_str).unwrap_or_default(),
                        r.get("at_mi").map(Value::py_str).unwrap_or_default()
                    )
                })
                .collect();
            println!("  {id}: {}", list.join("; "));
        }
        let corridor = corridor_mut(leg)?;
        let mut rows: Vec<Value> = corridor
            .get("landmarks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|lm| lm.get("category").and_then(Value::as_str) != Some("village"))
            .cloned()
            .collect();
        rows.extend(records.into_iter().map(Value::Object));
        let at = |v: &Value| v.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
        rows.sort_by(|a, b| at(a).total_cmp(&at(b)));
        let rows = Value::Array(rows);
        if corridor.get("landmarks") == Some(&rows) {
            same += 1;
        } else {
            changed += 1;
            corridor.insert("landmarks", rows);
        }
    }
    run.archive = archive;
    println!(
        "{total} villages; {} placed ahead of a speed zone, {} names refused as unspeakable; \
         {changed} legs changed, {same} the same, {no_archive} with no archived road kept.",
        counts.paired, counts.unspeakable
    );
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_village_moves_ahead_of_the_drop_it_explains() {
        assert_eq!(paired_mile(10.0, &[10.6, 30.0]), Some(10.4));
        assert_eq!(paired_mile(10.0, &[12.0]), None);
        assert_eq!(paired_mile(0.1, &[0.2]), Some(0.0));
    }

    #[test]
    fn only_steps_down_into_a_town_zone_count() {
        let leg = crate::pyjson::loads(
            r#"{"corridor": {"speed_limits": [
                {"at_mi": 0.0, "mph": 65}, {"at_mi": 5.0, "mph": 45},
                {"at_mi": 6.0, "mph": 35}, {"at_mi": 7.0, "mph": 45},
                {"at_mi": 9.0, "mph": null}, {"at_mi": 12.0, "mph": 40}]}}"#,
        )
        .unwrap();
        assert_eq!(zone_starts(&leg), vec![5.0, 6.0, 12.0]);
    }

    #[test]
    fn ghost_towns_are_not_read_as_places() {
        let live = [
            ("place", "village"),
            ("name", "Savage"),
            ("population", "269"),
        ];
        assert!(!is_gone(&live));
        assert!(!is_gone(&[
            ("place", "town"),
            ("name", "Pine"),
            ("disused", "no")
        ]));
        assert!(!is_gone(&[
            ("place", "village"),
            ("name", "Bean Hill"),
            ("historic", "district")
        ]));
        for gone in [
            ("historic", "ghost_town"),
            ("abandoned", "yes"),
            ("abandoned:place", "village"),
            ("end_date", "1958"),
            ("population", "0"),
        ] {
            assert!(
                is_gone(&[("place", "village"), ("name", "Bodie"), gone]),
                "{gone:?}"
            );
        }
        assert!(is_gone(&[
            ("place", "village"),
            ("name", "Ore'q (historical)")
        ]));
    }

    #[test]
    fn slashed_names_are_not_spoken() {
        assert!(!speakable("Camp Verde / Matthi:wa"));
        assert!(speakable("Pine"));
        assert_eq!(spoken("Pine", 0.2), "Entering Pine");
        assert_eq!(spoken("Pine", 3.0), "Passing Pine");
    }
}
