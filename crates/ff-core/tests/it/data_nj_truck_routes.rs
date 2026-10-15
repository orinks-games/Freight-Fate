//! Toms River to Newark keeps a loaded semi on roads it may legally drive.
//!
//! The leg used to be labelled the Garden State Parkway. The Parkway bans
//! commercial vehicles over 10,000 lb GVWR north of Interchange 105
//! (N.J.A.C. 19:9-1.9(b)), and a 102-inch-wide truck has to finish its trip
//! on the National Network once it can reach it (N.J.A.C. 16:32-1.5). The
//! leg now runs US 9 from Parkway Interchange 83 to I-195, I-195 west to
//! Turnpike Interchange 7A, and the New Jersey Turnpike north to 13A. These
//! tests read the checked-in tree, so a re-bake that drifts back onto the
//! Parkway, or a reroute that leaves the paid miles behind, fails here.

use std::sync::OnceLock;

use crate::data_support::data_dir;
use serde_json::Value;

const LEG: &str = "toms_river_nj_us:newark_nj_us";
const EARTH_RADIUS_MILES: f64 = 3958.7613;

/// The Garden State Parkway northbound mainline from the Interchange 105
/// split in Tinton Falls to the Interchange 148 diverge in Bloomfield: the
/// whole stretch where trucks over 10,000 lb are banned, as far north as a
/// Newark leg could reach. Read 2026-10-06 from a Valhalla auto route over
/// OpenStreetMap held to the Parkway (edges named "GSP; Garden State
/// Parkway"), then Douglas-Peucker simplified at 30 m, so every point is on
/// the pavement and no chord strays more than 30 m from it.
const PARKWAY_NORTH_OF_105: &[(f64, f64)] = &[
    (40.24681, -74.08262),
    (40.25099, -74.08190),
    (40.25544, -74.08004),
    (40.26530, -74.07818),
    (40.27028, -74.07826),
    (40.27691, -74.08110),
    (40.28501, -74.08773),
    (40.28955, -74.09009),
    (40.30636, -74.09363),
    (40.31880, -74.09773),
    (40.33296, -74.09856),
    (40.33597, -74.09954),
    (40.33935, -74.10149),
    (40.34226, -74.10495),
    (40.34466, -74.11123),
    (40.34607, -74.11365),
    (40.35205, -74.11855),
    (40.35395, -74.12091),
    (40.35871, -74.13181),
    (40.36187, -74.13680),
    (40.36552, -74.14049),
    (40.37392, -74.14612),
    (40.37674, -74.14935),
    (40.38233, -74.16226),
    (40.38575, -74.17348),
    (40.38770, -74.17695),
    (40.39077, -74.18013),
    (40.39826, -74.18541),
    (40.40442, -74.19502),
    (40.40614, -74.19696),
    (40.40817, -74.19836),
    (40.41388, -74.20083),
    (40.41736, -74.20445),
    (40.42194, -74.21459),
    (40.42793, -74.22564),
    (40.42855, -74.22852),
    (40.42946, -74.23918),
    (40.43071, -74.24441),
    (40.43670, -74.25383),
    (40.44252, -74.26441),
    (40.44481, -74.26696),
    (40.45159, -74.27117),
    (40.45851, -74.27725),
    (40.46117, -74.28047),
    (40.46924, -74.29255),
    (40.47219, -74.29583),
    (40.47903, -74.30064),
    (40.48597, -74.30239),
    (40.49488, -74.30018),
    (40.50224, -74.30145),
    (40.51271, -74.30085),
    (40.51820, -74.30112),
    (40.52154, -74.30052),
    (40.52773, -74.29810),
    (40.53078, -74.29829),
    (40.53320, -74.29986),
    (40.53949, -74.30715),
    (40.55104, -74.31598),
    (40.56567, -74.32429),
    (40.57253, -74.32940),
    (40.57598, -74.33081),
    (40.57990, -74.33090),
    (40.58360, -74.32947),
    (40.60641, -74.31354),
    (40.61843, -74.31064),
    (40.62206, -74.30892),
    (40.62535, -74.30606),
    (40.63592, -74.29356),
    (40.64243, -74.28859),
    (40.64725, -74.28723),
    (40.66067, -74.28708),
    (40.66349, -74.28662),
    (40.66698, -74.28507),
    (40.67460, -74.27972),
    (40.68207, -74.27710),
    (40.68672, -74.27386),
    (40.68928, -74.27096),
    (40.70206, -74.24775),
    (40.70441, -74.24592),
    (40.71222, -74.24377),
    (40.71582, -74.24119),
    (40.71755, -74.23905),
    (40.71995, -74.23325),
    (40.73085, -74.22228),
    (40.74286, -74.21437),
    (40.75859, -74.20906),
    (40.76593, -74.20452),
    (40.77285, -74.20453),
    (40.77780, -74.20104),
    (40.78202, -74.20125),
];

/// How close a stretch of the leg must run to the Parkway, and how nearly
/// parallel, to count as riding it. 0.1 mi covers the far carriageway plus
/// the 30 m simplification. It errs strict: US 9 runs within 60 m of the
/// Parkway through Sayreville and counts too (the old geometry shows 2.4 mi
/// of it), so a route drifting back onto either road fails and gets a look.
/// 25 degrees keeps a crossing, such as the Turnpike's at Woodbridge, from
/// counting however close it passes.
const ON_PARKWAY_MI: f64 = 0.1;
const PARALLEL_DEGREES: f64 = 25.0;
/// The most Parkway-north-of-105 mileage the leg may show. A real ride of a
/// single interchange is well over a mile; this leaves room only for the
/// sliver a near-parallel crossing can add.
const MAX_PARKWAY_MI: f64 = 0.5;

struct Leg {
    raw: Value,
    points: Vec<(f64, f64)>,
}

fn leg() -> &'static Leg {
    static LEG_DATA: OnceLock<Leg> = OnceLock::new();
    LEG_DATA.get_or_init(load)
}

fn load() -> Leg {
    let root = data_dir().join("world_data").join("us");
    let shard_path = root.join("legs").join("NJ.json");
    let shard: Value = serde_json::from_str(
        &std::fs::read_to_string(&shard_path)
            .unwrap_or_else(|error| panic!("{}: {error}", shard_path.display())),
    )
    .unwrap_or_else(|error| panic!("{}: {error}", shard_path.display()));
    let raw = shard["legs"]
        .as_array()
        .expect("leg shard has a legs array")
        .iter()
        .find(|leg| {
            format!(
                "{}:{}",
                leg["from"].as_str().unwrap_or_default(),
                leg["to"].as_str().unwrap_or_default()
            ) == LEG
        })
        .unwrap_or_else(|| panic!("missing leg {LEG}"))
        .clone();

    let geometry_path = root.join("geometry").join("nj.jsonl");
    let text = std::fs::read_to_string(&geometry_path)
        .unwrap_or_else(|error| panic!("{}: {error}", geometry_path.display()));
    let row: Value = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("geometry row is JSON"))
        .find(|row| row.get("leg").and_then(Value::as_str) == Some(LEG))
        .unwrap_or_else(|| panic!("missing archived geometry for {LEG}"));
    let points = decode_geometry(&row["geom"]).expect("archived geometry decodes");
    Leg { raw, points }
}

fn decode_geometry(raw: &Value) -> Option<Vec<(f64, f64)>> {
    let scale = 10_f64.powi(raw.get("q")?.as_i64()? as i32);
    let mut lat = raw.get("lat0")?.as_f64()?;
    let mut lon = raw.get("lon0")?.as_f64()?;
    let mut points = vec![(lat / scale, lon / scale)];
    for (delta_lat, delta_lon) in raw
        .get("dlat")?
        .as_array()?
        .iter()
        .zip(raw.get("dlon")?.as_array()?)
    {
        lat += delta_lat.as_f64()?;
        lon += delta_lon.as_f64()?;
        points.push((lat / scale, lon / scale));
    }
    Some(points)
}

fn haversine_miles(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lat2) = (a.0.to_radians(), b.0.to_radians());
    let delta_lat = lat2 - lat1;
    let delta_lon = (b.1 - a.1).to_radians();
    let h =
        (delta_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_MILES * h.sqrt().atan2((1.0 - h).max(0.0).sqrt())
}

fn length_miles(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|pair| haversine_miles(pair[0], pair[1]))
        .sum()
}

/// Local flat projection in miles around `origin`; plenty at New Jersey scale.
fn to_xy(origin: (f64, f64), point: (f64, f64)) -> (f64, f64) {
    let miles_per_degree = EARTH_RADIUS_MILES * std::f64::consts::PI / 180.0;
    (
        (point.1 - origin.1) * miles_per_degree * origin.0.to_radians().cos(),
        (point.0 - origin.0) * miles_per_degree,
    )
}

/// Distance in miles from `point` to `polyline`, and the bearing (degrees,
/// direction-free) of the nearest segment.
fn nearest(polyline: &[(f64, f64)], point: (f64, f64)) -> (f64, f64) {
    let mut best = (f64::INFINITY, 0.0);
    for pair in polyline.windows(2) {
        let (ax, ay) = to_xy(point, pair[0]);
        let (bx, by) = to_xy(point, pair[1]);
        let (dx, dy) = (bx - ax, by - ay);
        let length_squared = dx * dx + dy * dy;
        let t = if length_squared == 0.0 {
            0.0
        } else {
            (-(ax * dx + ay * dy) / length_squared).clamp(0.0, 1.0)
        };
        let distance = (ax + t * dx).hypot(ay + t * dy);
        if distance < best.0 {
            best = (distance, dy.atan2(dx).to_degrees());
        }
    }
    best
}

fn angle_between(a: f64, b: f64) -> f64 {
    let difference = (a - b).rem_euclid(180.0);
    difference.min(180.0 - difference)
}

/// Miles of `route` that run along the Parkway north of Interchange 105.
fn miles_riding_the_parkway(route: &[(f64, f64)]) -> f64 {
    const STEP_MI: f64 = 0.02;
    let mut riding = 0.0;
    for pair in route.windows(2) {
        let span = haversine_miles(pair[0], pair[1]);
        if span == 0.0 {
            continue;
        }
        let (dx, dy) = to_xy(pair[0], pair[1]);
        let heading = dy.atan2(dx).to_degrees();
        let steps = (span / STEP_MI).ceil().max(1.0);
        for i in 0..steps as usize {
            let t = (i as f64 + 0.5) / steps;
            let sample = (
                pair[0].0 + (pair[1].0 - pair[0].0) * t,
                pair[0].1 + (pair[1].1 - pair[0].1) * t,
            );
            let (distance, parkway_heading) = nearest(PARKWAY_NORTH_OF_105, sample);
            if distance <= ON_PARKWAY_MI
                && angle_between(heading, parkway_heading) <= PARALLEL_DEGREES
            {
                riding += span / steps;
            }
        }
    }
    riding
}

/// The detector has to see the Parkway when a route does ride it, or the
/// next test proves nothing.
#[test]
fn the_parkway_detector_sees_a_ride_up_the_parkway() {
    let riding = miles_riding_the_parkway(PARKWAY_NORTH_OF_105);
    let length = length_miles(PARKWAY_NORTH_OF_105);
    assert!(
        riding > length - 0.5,
        "detector saw {riding:.1} of {length:.1} Parkway miles"
    );
}

/// No stretch of the leg runs on the Parkway where trucks are banned.
#[test]
fn toms_river_newark_never_rides_the_parkway_north_of_interchange_105() {
    let riding = miles_riding_the_parkway(&leg().points);
    assert!(
        riding <= MAX_PARKWAY_MI,
        "{LEG} rides the Garden State Parkway north of Interchange 105 for {riding:.2} mi"
    );
}

/// The leg is called by the road it drives, not the one it cannot.
#[test]
fn toms_river_newark_is_labelled_for_the_turnpike() {
    let highway = leg().raw["highway"].as_str().unwrap_or_default();
    assert_eq!(highway, "I-95", "{LEG} is labelled {highway:?}");
}

/// Every curated pin -- two on I-195, one on the Turnpike -- is on the line
/// the truck drives, so the reroute is the road the pins describe.
#[test]
fn toms_river_newark_passes_its_i195_and_turnpike_pins() {
    let pins = leg().raw["route_via"]
        .as_array()
        .unwrap_or_else(|| panic!("{LEG} has no route_via pins"));
    assert_eq!(pins.len(), 3, "{LEG}: {pins:?}");
    for pin in pins {
        let point = (
            pin["lat"].as_f64().expect("pin latitude"),
            pin["lon"].as_f64().expect("pin longitude"),
        );
        let (off_mi, _) = nearest(&leg().points, point);
        assert!(
            off_mi <= 0.05,
            "{LEG}: pin {:?} is {off_mi:.2} mi off the route",
            pin["note"].as_str().unwrap_or_default()
        );
    }
}

/// Pay, deadlines and the odometer run on the leg's miles, so they match
/// the line the truck actually drives (the archive is the same source of
/// truth as the drive; whole miles, so within rounding).
#[test]
fn toms_river_newark_pays_the_miles_it_drives() {
    let miles = leg().raw["miles"].as_f64().expect("leg miles");
    let driven = length_miles(&leg().points);
    assert!(
        (miles - driven).abs() <= 0.5,
        "{LEG} pays {miles} mi for a {driven:.2} mi route"
    );
    assert_eq!(
        miles,
        driven.round(),
        "{LEG}: paid miles are not the rounded route"
    );
}
