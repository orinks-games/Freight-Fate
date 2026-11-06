//! Route records and mile markers agree with the road the truck drives.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::data_support::data_dir;
use serde_json::Value;

const BUFFALO_NY_TO_NEW_YORK: &str = "buffalo_ny_us:new_york_ny_us";
const ROCHESTER_NY_TO_NEW_YORK: &str = "rochester_ny_us:new_york_ny_us";
const DALLAS_TX_TO_ST_LOUIS: &str = "dallas_tx_us:st_louis_mo_us";
const WASHINGTON_DC_TO_CHARLOTTESVILLE: &str = "washington_dc_us:charlottesville_va_us";
const HARRISBURG_PA_TO_WILMINGTON: &str = "harrisburg_pa_us:wilmington_de_us";
const NORFOLK_VA_TO_PETERSBURG: &str = "norfolk_va_us:petersburg_va_us";
const GREEN_BAY_WI_TO_GRAND_RAPIDS: &str = "green_bay_wi_us:grand_rapids_mi_us";
const BINGHAMTON_NY_TO_UTICA: &str = "binghamton_ny_us:utica_ny_us";
const INDIANAPOLIS_IN_TO_NASHVILLE: &str = "indianapolis_in_us:nashville_tn_us";
const MEMPHIS_TN_TO_NASHVILLE: &str = "memphis_tn_us:nashville_tn_us";
const MEMPHIS_TN_TO_JACKSON: &str = "memphis_tn_us:jackson_tn_us";
const GALESBURG_IL_TO_DAVENPORT: &str = "galesburg_il_us:davenport_ia_us";
const DES_MOINES_IA_TO_CHICAGO: &str = "des_moines_ia_us:chicago_il_us";
const CHICAGO_IL_TO_ST_LOUIS: &str = "chicago_il_us:st_louis_mo_us";
const MINNEAPOLIS_MN_TO_DES_MOINES: &str = "minneapolis_mn_us:des_moines_ia_us";
const MIAMI_FL_TO_KEY_WEST: &str = "miami_fl_us:key_west_fl_us";
const DURANGO_CO_TO_MONTROSE: &str = "durango_co_us:montrose_co_us";
const DURANGO_CO_TO_MOAB: &str = "durango_co_us:moab_ut_us";

const REPAIRED_LEGS: &[&str] = &[
    BUFFALO_NY_TO_NEW_YORK,
    ROCHESTER_NY_TO_NEW_YORK,
    DALLAS_TX_TO_ST_LOUIS,
    WASHINGTON_DC_TO_CHARLOTTESVILLE,
    HARRISBURG_PA_TO_WILMINGTON,
    NORFOLK_VA_TO_PETERSBURG,
    GREEN_BAY_WI_TO_GRAND_RAPIDS,
    BINGHAMTON_NY_TO_UTICA,
    INDIANAPOLIS_IN_TO_NASHVILLE,
];

const EARTH_RADIUS_MILES: f64 = 3958.7613;

#[derive(Debug)]
struct LegData {
    raw: Value,
    miles: f64,
}

#[derive(Debug)]
struct RouteGeometry {
    points: Vec<(f64, f64)>,
    cumulative_miles: Vec<f64>,
}

#[derive(Debug)]
struct Dataset {
    legs: BTreeMap<String, LegData>,
    geometry: BTreeMap<String, RouteGeometry>,
}

static DATASET: OnceLock<Dataset> = OnceLock::new();

fn data() -> &'static Dataset {
    DATASET.get_or_init(load_dataset)
}

fn load_dataset() -> Dataset {
    let root = data_dir().join("world_data").join("us");
    let leg_dir = root.join("legs");
    let mut legs = BTreeMap::new();
    for path in files_with_extension(&leg_dir, "json") {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let shard: Value = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for raw in shard["legs"]
            .as_array()
            .expect("leg shard has a legs array")
        {
            let from = raw["from"].as_str().expect("leg has a from city");
            let to = raw["to"].as_str().expect("leg has a to city");
            let id = format!("{from}:{to}");
            let miles = raw["miles"].as_f64().expect("leg has mileage");
            legs.insert(
                id,
                LegData {
                    raw: raw.clone(),
                    miles,
                },
            );
        }
    }

    let mut geometry = BTreeMap::new();
    let geometry_dir = root.join("geometry");
    for path in files_with_extension(&geometry_dir, "jsonl") {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for (line_number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let row: Value = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{}:{}: {error}", path.display(), line_number + 1));
            if row.get("meta").is_some() {
                continue;
            }
            let Some(id) = row.get("leg").and_then(Value::as_str) else {
                continue;
            };
            let Some(leg) = legs.get(id) else {
                continue;
            };
            let points = decode_geometry(&row["geom"]).unwrap_or_else(|| {
                panic!(
                    "{}:{}: invalid geometry for {id}",
                    path.display(),
                    line_number + 1
                )
            });
            let mut cumulative_miles = vec![0.0];
            for pair in points.windows(2) {
                let distance = haversine_miles(pair[0].0, pair[0].1, pair[1].0, pair[1].1);
                cumulative_miles.push(cumulative_miles.last().copied().unwrap() + distance);
            }
            let total_miles = *cumulative_miles.last().unwrap_or(&0.0);
            if points.len() < 2 || total_miles <= 0.0 {
                continue;
            }
            let scale = leg.miles / total_miles;
            for mile in &mut cumulative_miles {
                *mile *= scale;
            }
            geometry.insert(
                id.to_string(),
                RouteGeometry {
                    points,
                    cumulative_miles,
                },
            );
        }
    }

    Dataset { legs, geometry }
}

fn files_with_extension(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|value| value == extension))
        .collect();
    paths.sort();
    paths
}

fn decode_geometry(raw: &Value) -> Option<Vec<(f64, f64)>> {
    let exponent = raw.get("q")?.as_i64()? as i32;
    let scale = 10_f64.powi(exponent);
    let mut lat = raw.get("lat0")?.as_f64()?;
    let mut lon = raw.get("lon0")?.as_f64()?;
    let dlat = raw.get("dlat")?.as_array()?;
    let dlon = raw.get("dlon")?.as_array()?;
    let mut points = vec![(lat / scale, lon / scale)];
    for (delta_lat, delta_lon) in dlat.iter().zip(dlon) {
        lat += delta_lat.as_f64()?;
        lon += delta_lon.as_f64()?;
        points.push((lat / scale, lon / scale));
    }
    Some(points)
}

fn haversine_miles(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let delta_lat = lat2 - lat1;
    let delta_lon = (lon2 - lon1).to_radians();
    let a =
        (delta_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_MILES * a.sqrt().atan2((1.0 - a).max(0.0).sqrt())
}

fn project(geometry: &RouteGeometry, lat: f64, lon: f64) -> (f64, f64) {
    let mut best = (0.0, f64::INFINITY);
    for (index, pair) in geometry.points.windows(2).enumerate() {
        let (lat1, lon1) = pair[0];
        let (lat2, lon2) = pair[1];
        let latitude_scale = EARTH_RADIUS_MILES * ((lat1 + lat2 + lat) / 3.0).to_radians().cos();
        let x_scale = latitude_scale * PI / 180.0;
        let y_scale = EARTH_RADIUS_MILES * PI / 180.0;
        let dx = (lon2 - lon1) * x_scale;
        let dy = (lat2 - lat1) * y_scale;
        let px = (lon - lon1) * x_scale;
        let py = (lat - lat1) * y_scale;
        let length_squared = dx * dx + dy * dy;
        let t = if length_squared == 0.0 {
            0.0
        } else {
            ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
        };
        let off_miles = ((px - t * dx).powi(2) + (py - t * dy).powi(2)).sqrt();
        if off_miles < best.1 {
            let at_mi = geometry.cumulative_miles[index]
                + t * (geometry.cumulative_miles[index + 1] - geometry.cumulative_miles[index]);
            best = (at_mi, off_miles);
        }
    }
    best
}

fn leg(id: &str) -> &'static LegData {
    data()
        .legs
        .get(id)
        .unwrap_or_else(|| panic!("missing leg {id}"))
}

fn geometry(id: &str) -> &'static RouteGeometry {
    data()
        .geometry
        .get(id)
        .unwrap_or_else(|| panic!("missing archived geometry for {id}"))
}

fn corridor_records<'a>(leg: &'a LegData, key: &str) -> &'a [Value] {
    leg.raw
        .get("corridor")
        .and_then(|corridor| corridor.get(key))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn top_level_records<'a>(leg: &'a LegData, key: &str) -> &'a [Value] {
    leg.raw
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn number(record: &Value, key: &str) -> Option<f64> {
    record.get(key).and_then(Value::as_f64)
}

fn text<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

fn coordinates(record: &Value) -> Option<(f64, f64)> {
    Some((number(record, "lat")?, number(record, "lon")?))
}

fn state_at(leg: &LegData, at_mi: f64) -> Option<&str> {
    let crossings = corridor_records(leg, "state_crossings");
    let mut state = crossings
        .first()
        .and_then(|crossing| text(crossing, "from_state"))
        .or_else(|| {
            corridor_records(leg, "state_miles")
                .first()
                .and_then(|mileage| text(mileage, "state"))
        });
    for crossing in crossings {
        if number(crossing, "at_mi").is_some_and(|mile| mile <= at_mi) {
            state = text(crossing, "state").or(state);
        }
    }
    state
}

fn find_named<'a>(records: &'a [Value], needle: &str) -> Option<&'a Value> {
    let needle = needle.to_lowercase();
    records.iter().find(|record| {
        text(record, "name").is_some_and(|name| name.to_lowercase().contains(&needle))
    })
}

fn named<'a>(records: &'a [Value], needle: &str, id: &str) -> &'a Value {
    find_named(records, needle).unwrap_or_else(|| panic!("{id}: missing {needle}"))
}

fn record_strings(record: &Value, key: &str) -> Vec<String> {
    match record.get(key) {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn is_interstate_65(interchange: &Value, leg: &LegData) -> bool {
    let highway = text(interchange, "highway")
        .or_else(|| text(&leg.raw, "highway"))
        .unwrap_or_default()
        .replace(' ', "-");
    highway.to_ascii_uppercase().starts_with("I-65")
}

/// Drivers hear each town in the state the road actually enters.
#[test]
fn checkpoints_sit_in_the_state_the_game_announces() {
    let mut failures = Vec::new();
    for (id, leg) in &data().legs {
        let crossings = corridor_records(leg, "state_crossings");
        if crossings.is_empty() {
            continue;
        }
        for checkpoint in corridor_records(leg, "checkpoints") {
            let (Some(said), Some(at_mi)) =
                (text(checkpoint, "state"), number(checkpoint, "at_mi"))
            else {
                continue;
            };
            if said.is_empty() {
                continue;
            }
            if crossings.iter().any(|crossing| {
                number(crossing, "at_mi").is_some_and(|mile| (mile - at_mi).abs() <= 3.0)
            }) {
                continue;
            }
            let real = state_at(leg, at_mi).expect("leg with crossings has a state at each mile");
            if said != real {
                failures.push((
                    id.clone(),
                    text(checkpoint, "name").unwrap_or_default().to_string(),
                    at_mi,
                    said.to_string(),
                    real.to_string(),
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "checkpoint state disagreements: {failures:?}"
    );
}

/// Each state's spoken distance adds up to the route's full length.
#[test]
fn state_miles_cover_the_leg() {
    for (id, leg) in &data().legs {
        let state_miles = corridor_records(leg, "state_miles");
        if state_miles.is_empty() {
            continue;
        }
        let miles: f64 = state_miles
            .iter()
            .map(|record| number(record, "miles").unwrap_or_default())
            .sum();
        assert!(
            (miles - leg.miles).abs() <= 1.0,
            "{id}: state miles sum to {miles}, leg is {}",
            leg.miles
        );

        let actual: BTreeSet<String> = state_miles
            .iter()
            .filter_map(|record| text(record, "state"))
            .map(str::to_string)
            .collect();
        let crossings = corridor_records(leg, "state_crossings");
        if let Some(first) = crossings.first() {
            let mut expected = BTreeSet::new();
            if let Some(state) = text(first, "from_state") {
                expected.insert(state.to_string());
            }
            for crossing in crossings {
                if let Some(state) = text(crossing, "state") {
                    expected.insert(state.to_string());
                }
            }
            assert_eq!(actual, expected, "{id}: state mileage sequence differs");
        } else {
            assert_eq!(actual.len(), 1, "{id}: one-state leg has multiple states");
        }
    }
}

/// New York's Thruway legs stay within New York.
#[test]
fn thruway_legs_never_leave_new_york() {
    for id in [BUFFALO_NY_TO_NEW_YORK, ROCHESTER_NY_TO_NEW_YORK] {
        let leg = leg(id);
        assert!(
            corridor_records(leg, "state_crossings").is_empty(),
            "{id}: state crossings {:?}",
            corridor_records(leg, "state_crossings")
        );
        let state_miles = corridor_records(leg, "state_miles");
        assert_eq!(state_miles.len(), 1, "{id}: {state_miles:?}");
        assert_eq!(text(&state_miles[0], "state"), Some("New York"), "{id}");
        let miles = number(&state_miles[0], "miles").expect("state mileage");
        assert!(
            (miles - leg.miles).abs() <= 1.0,
            "{id}: {miles} vs {}",
            leg.miles
        );
    }
}

/// Stops, checkpoints, and route points stay on the road they describe.
#[test]
fn repaired_legs_keep_their_records_on_their_road() {
    for id in REPAIRED_LEGS {
        let leg = leg(id);
        let geometry = geometry(id);
        for route_point in corridor_records(leg, "route_points") {
            let (lat, lon) =
                coordinates(route_point).unwrap_or_else(|| panic!("{id}: route point"));
            let (_, off_mi) = project(geometry, lat, lon);
            assert!(off_mi <= 3.0, "{id}: route point is {off_mi:.2} mi off");
        }
        for checkpoint in corridor_records(leg, "checkpoints") {
            let Some((lat, lon)) = coordinates(checkpoint) else {
                continue;
            };
            let (projected_mi, off_mi) = project(geometry, lat, lon);
            let at_mi = number(checkpoint, "at_mi").expect("checkpoint mile");
            assert!(
                off_mi <= 3.0,
                "{id}: checkpoint {:?} is {off_mi:.2} mi off",
                text(checkpoint, "name")
            );
            assert!(
                (projected_mi - at_mi).abs() <= 3.0,
                "{id}: checkpoint {:?} at {at_mi:.2}, projected {projected_mi:.2}",
                text(checkpoint, "name")
            );
        }
        for stop in top_level_records(leg, "stops") {
            let Some((lat, lon)) = coordinates(stop) else {
                continue;
            };
            let (_, off_mi) = project(geometry, lat, lon);
            assert!(
                off_mi <= 3.0,
                "{id}: stop {:?} is {off_mi:.2} mi off",
                text(stop, "name")
            );
        }
    }
}

/// Every archived leg's route points and coordinate-bearing checkpoints match its geometry.
#[test]
fn all_archived_leg_route_points_and_checkpoints_stay_near_geometry() {
    assert!(
        data().geometry.contains_key(DURANGO_CO_TO_MOAB),
        "Durango–Moab archive is required for alignment coverage"
    );
    let mut route_point_failures = Vec::new();
    let mut checkpoint_failures = Vec::new();
    for (id, leg_data) in &data().legs {
        let Some(geometry) = data().geometry.get(id) else {
            continue;
        };
        let leg = leg_data;
        for route_point in corridor_records(leg, "route_points") {
            let Some((lat, lon)) = coordinates(route_point) else {
                route_point_failures.push(format!("{id}: route point has no coordinates"));
                continue;
            };
            let (_, off_mi) = project(geometry, lat, lon);
            if off_mi > 10.0 {
                route_point_failures.push(format!("{id}: route point {off_mi:.2} mi off"));
            }
        }
        for checkpoint in corridor_records(leg, "checkpoints") {
            let Some((lat, lon)) = coordinates(checkpoint) else {
                continue;
            };
            let (_, off_mi) = project(geometry, lat, lon);
            if off_mi > 3.0 {
                checkpoint_failures.push(format!(
                    "{id}: {:?} is {off_mi:.2} mi off",
                    text(checkpoint, "name")
                ));
            }
        }
    }
    assert!(
        route_point_failures.is_empty(),
        "route points over 10 mi from archived geometry ({}): {route_point_failures:?}",
        route_point_failures.len()
    );
    assert!(
        checkpoint_failures.is_empty(),
        "checkpoints over 3 mi from archived geometry ({}): {checkpoint_failures:?}",
        checkpoint_failures.len()
    );
}

#[test]
fn scenic_billboard_bans_are_complete_and_not_crossing_fragments() {
    for (id, leg_data) in &data().legs {
        for span in corridor_records(leg_data, "billboard_bans") {
            let from_mi = number(span, "from_mi").expect("ban span has a start mile");
            let to_mi = number(span, "to_mi").expect("ban span has an end mile");
            assert!(
                to_mi - from_mi >= 1.0,
                "{id}: scenic ban {:?} is only {:.2} mi",
                text(span, "name"),
                to_mi - from_mi
            );
        }
        let is_i25_leg = text(&leg_data.raw, "highway")
            .unwrap_or_default()
            .split([';', '|', '/', ','])
            .any(|route_ref| {
                route_ref
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .collect::<String>()
                    .eq_ignore_ascii_case("I25")
            });
        if is_i25_leg {
            assert!(
                corridor_records(leg_data, "billboard_bans")
                    .iter()
                    .all(|span| !text(span, "name")
                        .unwrap_or_default()
                        .contains("Cache la Poudre")),
                "{id}: Cache la Poudre is a crossing, not a scenic span"
            );
        }
    }

    let san_juan: Vec<_> = corridor_records(leg(DURANGO_CO_TO_MONTROSE), "billboard_bans")
        .iter()
        .filter(|span| {
            text(span, "name")
                .unwrap_or_default()
                .contains("San Juan Skyway")
        })
        .collect();
    assert_eq!(
        san_juan.len(),
        1,
        "Durango–Montrose should have one contiguous San Juan Skyway span"
    );
    let san_juan = san_juan[0];
    assert!(
        number(san_juan, "to_mi").unwrap() - number(san_juan, "from_mi").unwrap() >= 60.0,
        "Durango–Montrose San Juan Skyway span is too short: {san_juan}"
    );

    let san_juan = corridor_records(leg(DURANGO_CO_TO_MOAB), "billboard_bans")
        .iter()
        .find(|span| {
            text(span, "name")
                .unwrap_or_default()
                .contains("San Juan Skyway")
        })
        .expect("Durango–Moab begins on the San Juan Skyway");
    assert!(
        number(san_juan, "from_mi").unwrap() <= 2.0,
        "Durango–Moab San Juan Skyway span begins too late: {san_juan}"
    );
}

/// The Dallas–St. Louis trip stays on I-44 through Oklahoma, not Arkansas.
#[test]
fn dallas_st_louis_rides_i44() {
    let leg = leg(DALLAS_TX_TO_ST_LOUIS);
    let states: Vec<&str> = corridor_records(leg, "state_miles")
        .iter()
        .filter_map(|record| text(record, "state"))
        .collect();
    assert_eq!(states, ["Texas", "Oklahoma", "Missouri"]);
    let forbidden = ["Caddo Valley", "Malvern", "Arkadelphia", "Poplar Bluff"];
    for interchange in corridor_records(leg, "interchanges") {
        for destination in record_strings(interchange, "destinations") {
            assert!(
                !forbidden
                    .iter()
                    .any(|name| destination.to_lowercase().contains(&name.to_lowercase())),
                "{DALLAS_TX_TO_ST_LOUIS}: forbidden destination {destination:?}"
            );
        }
        for via in record_strings(interchange, "via") {
            assert!(
                !forbidden
                    .iter()
                    .any(|name| via.to_lowercase().contains(&name.to_lowercase())),
                "{DALLAS_TX_TO_ST_LOUIS}: forbidden via {via:?}"
            );
        }
    }
    for stop in top_level_records(leg, "stops") {
        let name = text(stop, "name").unwrap_or_default();
        assert!(
            !forbidden
                .iter()
                .any(|place| name.to_lowercase().contains(&place.to_lowercase())),
            "{DALLAS_TX_TO_ST_LOUIS}: forbidden stop {name:?}"
        );
    }
}

/// River callouts happen where the road crosses the river.
#[test]
fn river_callouts_sit_on_the_crossing() {
    for (id, leg) in &data().legs {
        let Some(geometry) = data().geometry.get(id) else {
            continue;
        };
        for landmark in corridor_records(leg, "landmarks") {
            if text(landmark, "category") != Some("river") {
                continue;
            }
            let source = text(landmark, "source").unwrap_or_default();
            assert!(
                source.starts_with("derived")
                    && ((source.contains("tools/place_river_crossings.py")
                        && source.contains("at the crossing of OSM "))
                        // cargo tools osm-landmarks, which replaced it
                        || source.contains("placed at the crossing of the road with it")),
                "{id}: {} lacks place_river_crossings provenance: {source}",
                text(landmark, "name").unwrap_or_default()
            );
            let Some((lat, lon)) = coordinates(landmark) else {
                continue;
            };
            let (projected_mi, off_mi) = project(geometry, lat, lon);
            let at_mi = number(landmark, "at_mi").expect("river mile");
            assert!(
                off_mi <= 0.3,
                "{id}: {} is {off_mi:.2} mi off the road",
                text(landmark, "name").unwrap_or_default()
            );
            assert!(
                (projected_mi - at_mi).abs() <= 0.5,
                "{id}: {} at {at_mi:.2}, projected {projected_mi:.2}",
                text(landmark, "name").unwrap_or_default()
            );
        }
    }
}

/// Separate rivers get separate callouts along the drive.
#[test]
fn no_two_rivers_share_a_mile() {
    for id in [
        MEMPHIS_TN_TO_NASHVILLE,
        GALESBURG_IL_TO_DAVENPORT,
        CHICAGO_IL_TO_ST_LOUIS,
        INDIANAPOLIS_IN_TO_NASHVILLE,
        DES_MOINES_IA_TO_CHICAGO,
    ] {
        let rivers: Vec<_> = corridor_records(leg(id), "landmarks")
            .iter()
            .filter(|landmark| text(landmark, "category") == Some("river"))
            .collect();
        for (index, first) in rivers.iter().enumerate() {
            for second in rivers.iter().skip(index + 1) {
                if text(first, "name") == text(second, "name") {
                    continue;
                }
                let first_mi = number(first, "at_mi").expect("river mile");
                let second_mi = number(second, "at_mi").expect("river mile");
                assert!(
                    (first_mi - second_mi).abs() >= 0.3,
                    "{id}: {} and {} share mile {first_mi:.1}",
                    text(first, "name").unwrap_or_default(),
                    text(second, "name").unwrap_or_default()
                );
            }
        }
    }
}

/// River callouts along I-40 match the crossings the driver passes.
#[test]
fn i40_rivers_agree() {
    let hatchie_jackson = number(
        named(
            corridor_records(leg(MEMPHIS_TN_TO_JACKSON), "landmarks"),
            "Hatchie River",
            MEMPHIS_TN_TO_JACKSON,
        ),
        "at_mi",
    )
    .expect("Hatchie mile");
    let memphis_nashville = leg(MEMPHIS_TN_TO_NASHVILLE);
    let landmarks = corridor_records(memphis_nashville, "landmarks");
    let hatchie_nashville = number(
        named(landmarks, "Hatchie River", MEMPHIS_TN_TO_NASHVILLE),
        "at_mi",
    )
    .expect("Hatchie mile");
    assert!(
        (hatchie_jackson - hatchie_nashville).abs() <= 1.5,
        "Hatchie River at {hatchie_jackson:.1} and {hatchie_nashville:.1}"
    );
    let tennessee = number(
        named(landmarks, "Tennessee River", MEMPHIS_TN_TO_NASHVILLE),
        "at_mi",
    )
    .expect("Tennessee River mile");
    let buffalo = number(
        named(landmarks, "Buffalo River", MEMPHIS_TN_TO_NASHVILLE),
        "at_mi",
    )
    .expect("Buffalo River mile");
    let duck = number(
        named(landmarks, "Duck River", MEMPHIS_TN_TO_NASHVILLE),
        "at_mi",
    )
    .expect("Duck River mile");
    assert!(
        (130.0..=140.0).contains(&tennessee),
        "Tennessee River at {tennessee}"
    );
    assert!(
        tennessee < buffalo && buffalo < duck,
        "{tennessee}, {buffalo}, {duck}"
    );
}

/// Ohio River callouts stay at the state-line crossing.
#[test]
fn ohio_river_is_the_state_line() {
    for (id, leg) in &data().legs {
        let crossings = corridor_records(leg, "state_crossings");
        for river in corridor_records(leg, "landmarks")
            .iter()
            .filter(|landmark| {
                text(landmark, "category") == Some("river")
                    && text(landmark, "name") == Some("Ohio River")
            })
        {
            let at_mi = number(river, "at_mi").expect("Ohio River mile");
            let nearest = crossings
                .iter()
                .filter_map(|crossing| number(crossing, "at_mi"))
                .map(|crossing_mi| (crossing_mi - at_mi).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest <= 2.5,
                "{id}: Ohio River at {at_mi:.1}, nearest state crossing {nearest:.1} mi away"
            );
        }
    }
}

/// Mississippi River callouts stay by the crossing bridges.
#[test]
fn mississippi_callouts_where_the_bridge_is() {
    for id in [
        CHICAGO_IL_TO_ST_LOUIS,
        DES_MOINES_IA_TO_CHICAGO,
        GALESBURG_IL_TO_DAVENPORT,
    ] {
        let leg = leg(id);
        let crossings = corridor_records(leg, "state_crossings");
        let rivers: Vec<_> = corridor_records(leg, "landmarks")
            .iter()
            .filter(|landmark| {
                text(landmark, "category") == Some("river")
                    && text(landmark, "name") == Some("Mississippi River")
            })
            .collect();
        assert!(!rivers.is_empty(), "{id}: missing Mississippi River");
        for river in rivers {
            let at_mi = number(river, "at_mi").expect("Mississippi mile");
            let nearest = crossings
                .iter()
                .filter_map(|crossing| number(crossing, "at_mi"))
                .map(|crossing_mi| (crossing_mi - at_mi).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest <= 2.5,
                "{id}: Mississippi River at {at_mi:.1}, nearest state crossing {nearest:.1} mi away"
            );
        }
    }
}

fn first_mile_below_latitude(geometry: &RouteGeometry, latitude: f64) -> Option<f64> {
    for (index, pair) in geometry.points.windows(2).enumerate() {
        let (lat1, _) = pair[0];
        let (lat2, _) = pair[1];
        if lat1 >= latitude && lat2 < latitude {
            let t = (latitude - lat1) / (lat2 - lat1);
            return Some(
                geometry.cumulative_miles[index]
                    + t * (geometry.cumulative_miles[index + 1] - geometry.cumulative_miles[index]),
            );
        }
    }
    None
}

/// The Minnesota–Iowa callout lands where the road crosses the parallel.
#[test]
fn minnesota_iowa_line_on_the_43_30_parallel() {
    let leg = leg(MINNEAPOLIS_MN_TO_DES_MOINES);
    let crossing = corridor_records(leg, "state_crossings")
        .iter()
        .find(|crossing| {
            text(crossing, "from_state") == Some("Minnesota")
                && text(crossing, "state") == Some("Iowa")
        })
        .expect("Minnesota-to-Iowa crossing");
    let crossing_mi = number(crossing, "at_mi").expect("crossing mile");
    let parallel_mi = first_mile_below_latitude(geometry(MINNEAPOLIS_MN_TO_DES_MOINES), 43.5)
        .expect("route crosses 43.5 degrees north");
    assert!(
        (crossing_mi - parallel_mi).abs() <= 1.5,
        "state line at {crossing_mi:.1}, 43.5-degree crossing at {parallel_mi:.1}"
    );
}

/// Florida Keys landmarks stay with the islands and highway markers.
#[test]
fn keys_markers_are_in_the_keys() {
    let leg = leg(MIAMI_FL_TO_KEY_WEST);
    let checkpoints = corridor_records(leg, "checkpoints");
    let landmarks = corridor_records(leg, "landmarks");
    let key_largo = number(
        named(checkpoints, "Key Largo", MIAMI_FL_TO_KEY_WEST),
        "at_mi",
    )
    .expect("Key Largo mile");
    let islamorada = number(
        named(checkpoints, "Islamorada", MIAMI_FL_TO_KEY_WEST),
        "at_mi",
    )
    .expect("Islamorada mile");
    let big_pine = number(
        named(checkpoints, "Big Pine Key", MIAMI_FL_TO_KEY_WEST),
        "at_mi",
    )
    .expect("Big Pine Key mile");
    let entering = number(
        named(
            landmarks,
            "Entering the Florida Keys on the Overseas Highway",
            MIAMI_FL_TO_KEY_WEST,
        ),
        "at_mi",
    )
    .expect("Florida Keys entry mile");
    assert!(
        45.0 < entering && entering < key_largo,
        "entry at {entering}, Key Largo at {key_largo}"
    );
    let abyss = number(
        named(landmarks, "Christ of the Abyss", MIAMI_FL_TO_KEY_WEST),
        "at_mi",
    )
    .expect("Christ of the Abyss mile");
    assert!(
        entering < abyss && abyss < islamorada,
        "{entering}, {abyss}, {islamorada}"
    );
    let hurricane = number(
        named(landmarks, "The Hurricane Monument", MIAMI_FL_TO_KEY_WEST),
        "at_mi",
    )
    .expect("Hurricane Monument mile");
    assert!(
        (hurricane - islamorada).abs() <= 3.0,
        "Hurricane Monument at {hurricane}"
    );
    let refuge = number(
        named(landmarks, "National Key Deer Refuge", MIAMI_FL_TO_KEY_WEST),
        "at_mi",
    )
    .expect("Key Deer Refuge mile");
    assert!(
        (refuge - big_pine).abs() <= 1.5,
        "refuge at {refuge}, Big Pine at {big_pine}"
    );

    let route_geometry = geometry(MIAMI_FL_TO_KEY_WEST);
    for marker in landmarks.iter().filter(|landmark| {
        text(landmark, "category") == Some("highway_marker") && coordinates(landmark).is_some()
    }) {
        let (lat, lon) = coordinates(marker).expect("marker coordinates");
        let (projected_mi, _) = project(route_geometry, lat, lon);
        let at_mi = number(marker, "at_mi").expect("marker mile");
        let directions = record_strings(marker, "directions");
        let forward = directions.len() == 1 && directions[0] == "forward";
        if forward {
            assert!(
                at_mi <= projected_mi && projected_mi <= at_mi + 3.5,
                "{} at {at_mi:.1}, projected {projected_mi:.1}",
                text(marker, "name").unwrap_or_default()
            );
        } else {
            assert!(
                (projected_mi - at_mi).abs() <= 1.0,
                "{} at {at_mi:.1}, projected {projected_mi:.1}",
                text(marker, "name").unwrap_or_default()
            );
        }
    }
}

/// Kentucky I-65 exit numbers count down as the truck heads south.
#[test]
fn indianapolis_nashville_kentucky_exits_count_down() {
    let leg = leg(INDIANAPOLIS_IN_TO_NASHVILLE);
    let crossings = corridor_records(leg, "state_crossings");
    let into_kentucky = crossings
        .iter()
        .find(|crossing| {
            text(crossing, "from_state") == Some("Indiana")
                && text(crossing, "state") == Some("Kentucky")
        })
        .and_then(|crossing| number(crossing, "at_mi"))
        .expect("Indiana-to-Kentucky crossing");
    let into_tennessee = crossings
        .iter()
        .find(|crossing| {
            text(crossing, "from_state") == Some("Kentucky")
                && text(crossing, "state") == Some("Tennessee")
        })
        .and_then(|crossing| number(crossing, "at_mi"))
        .expect("Kentucky-to-Tennessee crossing");
    assert!(into_kentucky < into_tennessee);

    // Tennessee's 121/117/112 are legitimate; only Kentucky is checked.
    let mut exits: Vec<_> = corridor_records(leg, "interchanges")
        .iter()
        .filter(|interchange| is_interstate_65(interchange, leg))
        .filter_map(|interchange| {
            let at_mi = number(interchange, "at_mi")?;
            let exit_ref = text(interchange, "exit_ref")?;
            if at_mi <= into_kentucky
                || at_mi >= into_tennessee
                || exit_ref.is_empty()
                || !exit_ref.chars().all(|character| character.is_ascii_digit())
            {
                return None;
            }
            Some((at_mi, exit_ref.parse::<u32>().ok()?))
        })
        .collect();
    exits.sort_by(|left, right| left.0.total_cmp(&right.0));
    assert!(!exits.is_empty(), "no numbered I-65 exits in Kentucky");
    for pair in exits.windows(2) {
        assert!(
            pair[0].1 > pair[1].1,
            "I-65 exits do not count down: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }

    let mut named_exits = Vec::new();
    for (destination, expected_ref) in [
        ("Horse Cave", "58"),
        ("Cave City", "53"),
        ("Park City", "48"),
    ] {
        let interchange = corridor_records(leg, "interchanges")
            .iter()
            .find(|interchange| {
                is_interstate_65(interchange, leg)
                    && number(interchange, "at_mi")
                        .is_some_and(|mile| into_kentucky < mile && mile < into_tennessee)
                    && record_strings(interchange, "destinations")
                        .iter()
                        .any(|name| name.to_lowercase().contains(&destination.to_lowercase()))
            })
            .unwrap_or_else(|| panic!("missing I-65 exit for {destination}"));
        let exit_ref = text(interchange, "exit_ref").expect("destination exit ref");
        assert_eq!(exit_ref, expected_ref, "{destination}");
        named_exits.push((
            number(interchange, "at_mi").expect("destination exit mile"),
            destination,
        ));
    }
    named_exits.sort_by(|left, right| left.0.total_cmp(&right.0));
    assert_eq!(
        named_exits
            .iter()
            .map(|(_, name)| *name)
            .collect::<Vec<_>>(),
        vec!["Horse Cave", "Cave City", "Park City"]
    );
}
