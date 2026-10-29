//! United States time zones and the local wall clock.
//!
//! The career clock (`profile.game_hours`) and the trip clock
//! (`Trip.current_hour`) are a single absolute timeline defined as Eastern
//! Time; time compression only changes how fast that timeline advances. A time
//! zone is a pure display layer over it: the local wall clock at any place is
//! the absolute clock plus that place's fixed offset. Nothing that measures
//! durations -- hours of service, deadlines, seasons, market days -- ever
//! shifts; only what the player hears spoken as "the time" does.
//!
//! Zones are derived offline and deterministically from coordinates the world
//! data already carries (cities and baked route points both have lat/lon plus a
//! state). Whole states resolve through a table; the states a zone boundary
//! splits (Tennessee, Kentucky, Indiana, the Florida panhandle, west Texas, and
//! friends) use curated longitude/latitude rules that track the real boundary at
//! game fidelity. Daylight saving time is deliberately not modeled: the career
//! calendar is abstract, and fixed standard offsets keep the spoken clock
//! predictable for screen reader players.
//!
//! Port of `freight_fate/sim/timezones.py`.

use super::hos::clock_text;

/// A fixed-offset zone; `name` is the spoken form ("Central Time") and
/// `offset_h` is hours relative to Eastern, the game's reference clock.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TimeZone {
    pub key: &'static str,
    pub name: &'static str,
    pub offset_h: f64,
}

pub const EASTERN: TimeZone = TimeZone {
    key: "eastern",
    name: "Eastern Time",
    offset_h: 0.0,
};
pub const CENTRAL: TimeZone = TimeZone {
    key: "central",
    name: "Central Time",
    offset_h: -1.0,
};
pub const MOUNTAIN: TimeZone = TimeZone {
    key: "mountain",
    name: "Mountain Time",
    offset_h: -2.0,
};
pub const PACIFIC: TimeZone = TimeZone {
    key: "pacific",
    name: "Pacific Time",
    offset_h: -3.0,
};
pub const ALASKA: TimeZone = TimeZone {
    key: "alaska",
    name: "Alaska Time",
    offset_h: -4.0,
};
pub const HAWAII: TimeZone = TimeZone {
    key: "hawaii",
    name: "Hawaii Time",
    offset_h: -5.0,
};

/// Every zone, in the order the Python `ZONES` dict listed them.
pub const ZONES: [TimeZone; 6] = [EASTERN, CENTRAL, MOUNTAIN, PACIFIC, ALASKA, HAWAII];

/// `ZONES[key]`.
pub fn zone_by_key(key: &str) -> Option<TimeZone> {
    ZONES.iter().copied().find(|zone| zone.key == key)
}

/// States that sit entirely inside one zone, keyed by the full state name the
/// world data uses. Arizona skips daylight saving in reality; with no DST in
/// the model it is plain Mountain here.
fn state_zone(state: &str) -> Option<TimeZone> {
    Some(match state {
        "Connecticut"
        | "Delaware"
        | "District of Columbia"
        | "Georgia"
        | "Maine"
        | "Maryland"
        | "Massachusetts"
        | "New Hampshire"
        | "New Jersey"
        | "New York"
        | "North Carolina"
        | "Ohio"
        | "Pennsylvania"
        | "Rhode Island"
        | "South Carolina"
        | "Vermont"
        | "Virginia"
        | "West Virginia" => EASTERN,
        "Alabama" | "Arkansas" | "Illinois" | "Iowa" | "Louisiana" | "Minnesota"
        | "Mississippi" | "Missouri" | "Oklahoma" | "Wisconsin" => CENTRAL,
        "Arizona" | "Colorado" | "Montana" | "New Mexico" | "Utah" | "Wyoming" => MOUNTAIN,
        "California" | "Washington" => PACIFIC,
        "Alaska" => ALASKA,
        "Hawaii" => HAWAII,
        _ => return None,
    })
}

fn florida(_lat: f64, lon: f64) -> TimeZone {
    // The panhandle west of the Apalachicola River keeps Central time.
    if lon < -85.1 {
        CENTRAL
    } else {
        EASTERN
    }
}

fn indiana(_lat: f64, lon: f64) -> TimeZone {
    // The Gary and Evansville corners follow Chicago; the rest is Eastern.
    if lon < -87.25 {
        CENTRAL
    } else {
        EASTERN
    }
}

fn kentucky(_lat: f64, lon: f64) -> TimeZone {
    // Western Kentucky (Bowling Green, Paducah) is Central; Louisville and
    // Lexington are Eastern.
    if lon < -86.0 {
        CENTRAL
    } else {
        EASTERN
    }
}

fn tennessee(_lat: f64, lon: f64) -> TimeZone {
    // East Tennessee (Knoxville, Chattanooga) is Eastern; the boundary runs
    // just west of Chattanooga, so Nashville and Memphis are Central.
    if lon < -85.5 {
        CENTRAL
    } else {
        EASTERN
    }
}

fn michigan(lat: f64, lon: f64) -> TimeZone {
    // Four western Upper Peninsula counties border Wisconsin's clock.
    if lon < -89.5 && lat > 45.5 {
        CENTRAL
    } else {
        EASTERN
    }
}

fn north_dakota(lat: f64, lon: f64) -> TimeZone {
    // The southwest corner below the Missouri River is Mountain.
    if lon < -102.25 && lat < 47.5 {
        MOUNTAIN
    } else {
        CENTRAL
    }
}

fn south_dakota(_lat: f64, lon: f64) -> TimeZone {
    // West River (Rapid City) is Mountain; Pierre and eastward are Central.
    if lon < -101.0 {
        MOUNTAIN
    } else {
        CENTRAL
    }
}

fn nebraska(_lat: f64, lon: f64) -> TimeZone {
    // The panhandle from about Ogallala west is Mountain.
    if lon < -101.4 {
        MOUNTAIN
    } else {
        CENTRAL
    }
}

fn kansas(_lat: f64, lon: f64) -> TimeZone {
    // The four westernmost border counties (Goodland) are Mountain.
    if lon < -101.5 {
        MOUNTAIN
    } else {
        CENTRAL
    }
}

fn texas(_lat: f64, lon: f64) -> TimeZone {
    // Only El Paso and Hudspeth counties, in the far western wedge.
    if lon < -104.9 {
        MOUNTAIN
    } else {
        CENTRAL
    }
}

fn idaho(lat: f64, _lon: f64) -> TimeZone {
    // The panhandle north of the Salmon River follows Spokane's clock.
    if lat > 45.5 {
        PACIFIC
    } else {
        MOUNTAIN
    }
}

fn oregon(lat: f64, lon: f64) -> TimeZone {
    // Ontario and most of Malheur County, on the Boise side of the desert.
    if lon > -117.3 && lat < 44.5 {
        MOUNTAIN
    } else {
        PACIFIC
    }
}

fn nevada(_lat: f64, lon: f64) -> TimeZone {
    // The West Wendover sliver on the Utah line runs on Mountain time.
    if lon > -114.1 {
        MOUNTAIN
    } else {
        PACIFIC
    }
}

fn split_state_zone(state: &str) -> Option<fn(f64, f64) -> TimeZone> {
    Some(match state {
        "Florida" => florida,
        "Indiana" => indiana,
        "Kentucky" => kentucky,
        "Tennessee" => tennessee,
        "Michigan" => michigan,
        "North Dakota" => north_dakota,
        "South Dakota" => south_dakota,
        "Nebraska" => nebraska,
        "Kansas" => kansas,
        "Texas" => texas,
        "Idaho" => idaho,
        "Oregon" => oregon,
        "Nevada" => nevada,
        _ => return None,
    })
}

/// The time zone at a coordinate, using the state when one is known.
///
/// With no usable state, rough boundary meridians decide -- good enough for
/// the open road between known places. Missing geometry (0, 0) resolves to
/// Eastern, the reference zone, so a synthetic or incomplete leg keeps the
/// clock it always had.
pub fn zone_for(lat: f64, lon: f64, state: &str) -> TimeZone {
    if let Some(split) = split_state_zone(state) {
        return split(lat, lon);
    }
    if let Some(zone) = state_zone(state) {
        return zone;
    }
    if lat == 0.0 && lon == 0.0 {
        return EASTERN;
    }
    if lon >= -85.5 {
        return EASTERN;
    }
    if lon >= -102.0 {
        return CENTRAL;
    }
    if lon >= -114.5 {
        return MOUNTAIN;
    }
    PACIFIC
}

/// Anything with a coordinate and a state: the duck-typed `city` argument
/// of the Python `city_zone` (a `City`, a route point, a test namespace).
pub trait HasLocation {
    fn lat(&self) -> f64;
    fn lon(&self) -> f64;
    fn state(&self) -> &str;
}

/// The zone a City (or anything with lat, lon, and state) lives in.
pub fn city_zone(city: &dyn HasLocation) -> TimeZone {
    zone_for(city.lat(), city.lon(), city.state())
}

/// Absolute (Eastern-reference) hours shifted onto a zone's wall clock.
///
/// Not wrapped to a day: callers doing clock math keep the full timeline,
/// and `clock_text` wraps for speech on its own.
pub fn to_local(game_hours: f64, zone: TimeZone) -> f64 {
    game_hours + zone.offset_h
}

/// Spoken local wall clock, optionally naming the zone: '2:15 PM Central Time'.
pub fn local_clock_text(game_hours: f64, zone: TimeZone, with_zone: bool) -> String {
    let text = clock_text(to_local(game_hours, zone));
    if with_zone {
        format!("{text} {}", zone.name)
    } else {
        text
    }
}

/// Python `a // b` for floats: the floor of the quotient, computed the way
/// CPython does it (from the remainder, so a large quotient keeps its
/// integer exactly).
fn py_floordiv(a: f64, b: f64) -> f64 {
    let modulus = a % b;
    let mut div = (a - modulus) / b;
    if modulus != 0.0 && ((b < 0.0) != (modulus < 0.0)) {
        div -= 1.0;
    }
    if div != 0.0 {
        let floordiv = div.floor();
        if div - floordiv > 0.5 {
            floordiv + 1.0
        } else {
            floordiv
        }
    } else {
        0.0_f64.copysign(a / b)
    }
}

/// A future moment as a local appointment: '6 PM Central Time tomorrow'.
///
/// The day qualifier counts local midnights between now and the moment, so
/// "tomorrow" means what a driver parked at the receiver would mean by it.
pub fn appointment_text(now_game_hours: f64, hours_from_now: f64, zone: TimeZone) -> String {
    let local_now = to_local(now_game_hours, zone);
    let local_due = local_now
        + if hours_from_now > 0.0 {
            hours_from_now
        } else {
            0.0
        };
    let days_ahead = py_floordiv(local_due, 24.0) as i64 - py_floordiv(local_now, 24.0) as i64;
    let base = format!("{} {}", clock_text(local_due), zone.name);
    if days_ahead <= 0 {
        return base;
    }
    if days_ahead == 1 {
        return format!("{base} tomorrow");
    }
    format!("{base} in {days_ahead} days")
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_timezones.py`. The trip-crossing, world-data
    //! and dispatch-board cases need `sim::trip`, `data::world` and the app
    //! shell and are ignored until those land.
    use super::*;

    // --- zone derivation ----------------------------------------------------------

    #[test]
    fn test_whole_states_resolve_from_the_table() {
        assert_eq!(zone_for(33.75, -84.39, "Georgia"), EASTERN); // Atlanta
        assert_eq!(zone_for(41.88, -87.63, "Illinois"), CENTRAL); // Chicago
        assert_eq!(zone_for(39.74, -104.99, "Colorado"), MOUNTAIN); // Denver
        assert_eq!(zone_for(47.61, -122.33, "Washington"), PACIFIC); // Seattle
    }

    #[test]
    fn test_split_states_follow_the_real_boundary() {
        let cases: [(&str, f64, f64, &str, TimeZone); 18] = [
            ("Knoxville", 35.96, -83.92, "Tennessee", EASTERN),
            ("Chattanooga", 35.05, -85.31, "Tennessee", EASTERN),
            ("Nashville", 36.16, -86.78, "Tennessee", CENTRAL),
            ("Memphis", 35.15, -90.05, "Tennessee", CENTRAL),
            ("Miami", 25.76, -80.19, "Florida", EASTERN),
            ("Pensacola", 30.42, -87.22, "Florida", CENTRAL),
            ("Louisville", 38.25, -85.76, "Kentucky", EASTERN),
            ("Bowling Green", 36.99, -86.44, "Kentucky", CENTRAL),
            ("Indianapolis", 39.77, -86.16, "Indiana", EASTERN),
            ("Evansville", 37.97, -87.57, "Indiana", CENTRAL),
            ("Dallas", 32.78, -96.80, "Texas", CENTRAL),
            ("El Paso", 31.76, -106.49, "Texas", MOUNTAIN),
            ("Sioux Falls", 43.55, -96.73, "South Dakota", CENTRAL),
            ("Rapid City", 44.08, -103.23, "South Dakota", MOUNTAIN),
            ("Boise", 43.62, -116.20, "Idaho", MOUNTAIN),
            ("Coeur d'Alene", 47.68, -116.78, "Idaho", PACIFIC),
            ("Portland", 45.52, -122.68, "Oregon", PACIFIC),
            ("Ontario", 44.03, -116.96, "Oregon", MOUNTAIN),
        ];
        for (place, lat, lon, state, expected) in cases {
            assert_eq!(zone_for(lat, lon, state), expected, "{place}");
        }
    }

    #[test]
    fn test_unknown_state_falls_back_to_longitude() {
        assert_eq!(zone_for(40.0, -75.0, ""), EASTERN);
        assert_eq!(zone_for(40.0, -95.0, ""), CENTRAL);
        assert_eq!(zone_for(40.0, -108.0, ""), MOUNTAIN);
        assert_eq!(zone_for(40.0, -120.0, ""), PACIFIC);
    }

    #[test]
    fn test_missing_geometry_keeps_the_reference_clock() {
        // Synthetic legs and incomplete data carry (0, 0); the clock must not move.
        assert_eq!(zone_for(0.0, 0.0, ""), EASTERN);
        assert_eq!(zone_for(0.0, 0.0, "Atlantis"), EASTERN);
    }

    #[test]
    fn test_every_world_city_resolves_to_a_conus_zone() {
        let world = crate::data::world::World::load().expect("the shipped world loads");
        for (key, city) in world.cities.iter() {
            let zone = zone_for(city.lat, city.lon, &city.state);
            assert!(
                [EASTERN, CENTRAL, MOUNTAIN, PACIFIC].contains(&zone),
                "{key} in {} landed outside the CONUS zones",
                city.state
            );
        }
    }

    struct Place {
        lat: f64,
        lon: f64,
        state: &'static str,
    }

    impl HasLocation for Place {
        fn lat(&self) -> f64 {
            self.lat
        }
        fn lon(&self) -> f64 {
            self.lon
        }
        fn state(&self) -> &str {
            self.state
        }
    }

    #[test]
    fn city_zone_reads_the_location_trait() {
        let denver = Place {
            lat: 39.74,
            lon: -104.99,
            state: "Colorado",
        };
        assert_eq!(city_zone(&denver), MOUNTAIN);
        assert_eq!(zone_by_key("central"), Some(CENTRAL));
        assert_eq!(zone_by_key("lunar"), None);
    }

    // --- the local wall clock -------------------------------------------------------

    #[test]
    fn test_local_clock_wraps_backwards_across_midnight() {
        // 1 AM on the Eastern reference clock is 10 PM Pacific the night before.
        assert_eq!(to_local(1.0, PACIFIC), -2.0);
        assert_eq!(local_clock_text(1.0, PACIFIC, false), "10 PM");
    }

    #[test]
    fn test_local_clock_can_name_its_zone() {
        assert_eq!(local_clock_text(12.0, CENTRAL, true), "11 AM Central Time");
    }

    // --- appointments ---------------------------------------------------------------

    #[test]
    fn test_appointment_same_local_day() {
        assert_eq!(appointment_text(6.0, 4.0, EASTERN), "10 AM Eastern Time");
    }

    #[test]
    fn test_appointment_tomorrow_counts_local_midnights() {
        assert_eq!(
            appointment_text(20.0, 10.0, EASTERN),
            "6 AM Eastern Time tomorrow"
        );
    }

    #[test]
    fn test_appointment_days_ahead() {
        assert_eq!(
            appointment_text(6.0, 44.0, EASTERN),
            "2 AM Eastern Time in 2 days"
        );
    }

    #[test]
    fn test_appointment_tomorrow_judged_in_the_destination_zone() {
        // 10 PM on the reference clock is 9 PM Central; five hours later is past
        // the receiver's midnight even though it is not past Eastern's.
        assert_eq!(
            appointment_text(22.0, 5.0, CENTRAL),
            "2 AM Central Time tomorrow"
        );
    }

    #[test]
    fn py_floordiv_matches_python_on_negative_hours() {
        assert_eq!(py_floordiv(-2.0, 24.0), -1.0);
        assert_eq!(py_floordiv(23.9, 24.0), 0.0);
        assert_eq!(py_floordiv(48.0, 24.0), 2.0);
        assert_eq!(py_floordiv(-24.0, 24.0), -1.0);
    }

    // --- trip crossings: helpers ------------------------------------------------------

    use crate::data::world::get_world;
    use crate::data::world_models::{
        CorridorDetail, Leg, Route, RoutePoint, StateCrossing, StateMileage,
    };
    use crate::sim::trip::{Trip, TripOptions};
    use crate::sim::trip_models::TripEventKind;
    use crate::sim::vehicle::TruckState;
    use crate::sim::weather::test_support::new_system;

    fn rp(at_mi: f64, lat: f64, lon: f64) -> RoutePoint {
        RoutePoint { at_mi, lat, lon }
    }

    /// A stylized east-to-middle Tennessee leg: Eastern until the boundary at
    /// the halfway point, Central beyond it. Kept under 70 miles so the trip
    /// places no traffic or patrols for the synthetic endpoint cities.
    fn tennessee_leg() -> Leg {
        Leg::new("A", "B", 60.0, "I-40", "hills", Vec::new()).with_detail(CorridorDetail {
            state_miles: vec![StateMileage::new("Tennessee", 60.0)],
            route_points: vec![
                rp(0.0, 35.96, -83.92),
                rp(20.0, 35.90, -85.00),
                rp(30.0, 36.00, -85.80),
                rp(40.0, 36.10, -86.30),
                rp(60.0, 36.16, -86.78),
            ],
            ..Default::default()
        })
    }

    /// A stylized Kingman-to-Barstow: sparse route points thirty miles apart
    /// with the Arizona-California line carried as an exact state crossing.
    fn desert_interstate_leg() -> Leg {
        Leg::new("A", "B", 90.0, "I-40", "mountain", Vec::new()).with_detail(CorridorDetail {
            state_miles: vec![
                StateMileage::new("Arizona", 49.3),
                StateMileage::new("California", 40.7),
            ],
            state_crossings: vec![StateCrossing {
                at_mi: 49.3,
                from_state: "Arizona".into(),
                state: "California".into(),
                place: "the Colorado River".into(),
                source: String::new(),
            }],
            route_points: vec![
                rp(0.0, 35.19, -114.05),
                rp(30.0, 34.80, -114.16),
                rp(60.0, 34.80, -114.59),
                rp(90.0, 34.83, -115.03),
            ],
            ..Default::default()
        })
    }

    fn ab(leg: Leg) -> Route {
        Route::from_legs(vec!["A".to_string(), "B".to_string()], vec![leg])
    }

    fn ba(leg: Leg) -> Route {
        Route::from_legs(vec!["B".to_string(), "A".to_string()], vec![leg])
    }

    fn trip(route: Route, start_hour: f64) -> Trip {
        Trip::new(
            route,
            TruckState::default(),
            new_system("mid_south", Some(1), None, None, true),
            TripOptions {
                seed: Some(2),
                start_hour,
                ..Default::default()
            },
        )
    }

    fn crossings(trip: &Trip) -> Vec<(f64, &'static str)> {
        trip.timezone_crossings
            .iter()
            .map(|c| (c.at_mi, c.to_zone.key))
            .collect()
    }

    fn timezone_events(trip: &Trip) -> Vec<String> {
        trip.events
            .iter()
            .filter(|e| e.kind == TripEventKind::TimezoneCrossing)
            .map(|e| e.text().to_string())
            .collect()
    }

    // --- trip crossings -------------------------------------------------------------

    #[test]
    fn test_trip_finds_the_boundary_from_route_geometry() {
        let trip = trip(ab(tennessee_leg()), 12.0);
        assert_eq!(trip.start_timezone, EASTERN);
        assert_eq!(trip.destination_timezone(), CENTRAL);
        assert_eq!(crossings(&trip), vec![(30.0, "central")]);
        assert_eq!(trip.timezone_at(20.0), EASTERN);
        assert_eq!(trip.timezone_at(35.0), CENTRAL);
    }

    #[test]
    fn test_clock_changes_at_the_state_crossing_not_the_next_sparse_point() {
        let trip = trip(ab(desert_interstate_leg()), 12.0);
        assert_eq!(crossings(&trip), vec![(49.3, "pacific")]);
        assert_eq!(trip.timezone_at(48.0).key, "mountain");
        assert_eq!(trip.timezone_at(50.0).key, "pacific");
    }

    #[test]
    fn test_clock_changes_at_the_state_crossing_reversed() {
        let trip = trip(ba(desert_interstate_leg()), 12.0);
        // Traversed the other way the border sits at 90 - 49.3 trip miles.
        let rounded: Vec<(f64, &str)> = crossings(&trip)
            .into_iter()
            .map(|(at, key)| ((at * 10.0).round() / 10.0, key))
            .collect();
        assert_eq!(rounded, vec![(40.7, "mountain")]);
        assert_eq!(trip.timezone_at(39.0).key, "pacific");
        assert_eq!(trip.timezone_at(42.0).key, "mountain");
    }

    #[test]
    fn test_trip_reversed_route_mirrors_the_boundary() {
        let trip = trip(ba(tennessee_leg()), 12.0);
        assert_eq!(trip.start_timezone, CENTRAL);
        assert_eq!(trip.destination_timezone(), EASTERN);
        // The crossing lands on the first sampled point inside the new zone.
        assert_eq!(crossings(&trip), vec![(40.0, "eastern")]);
    }

    #[test]
    fn test_crossing_announces_the_new_local_clock_once() {
        let mut trip = trip(ab(tennessee_leg()), 12.0);
        trip.position_mi = 35.0;
        trip.check_timezone();
        let events = timezone_events(&trip);
        assert_eq!(events.len(), 1);
        // Noon on the Eastern reference clock is 11 AM Central.
        assert_eq!(events[0], "Crossing into Central Time. It is now 11 AM.");
        trip.check_timezone();
        assert_eq!(timezone_events(&trip).len(), 1);
    }

    #[test]
    fn test_crossing_east_announces_the_eastern_clock() {
        let mut trip = trip(ba(tennessee_leg()), 12.0);
        trip.position_mi = 45.0;
        trip.check_timezone();
        let events = timezone_events(&trip);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], "Crossing into Eastern Time. It is now 12 PM.");
    }

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- states::driving_core::timezone_crossing_message"]
    fn test_terse_speech_says_only_the_zone() {
        // TODO(port): terse crossing message is "Central Time."
    }

    #[test]
    fn test_local_hour_follows_the_truck_across_the_boundary() {
        let mut trip = trip(ab(tennessee_leg()), 12.0);
        assert_eq!(trip.local_hour(), 12.0);
        trip.position_mi = 35.0;
        assert_eq!(trip.local_hour(), 11.0);
    }

    #[test]
    fn test_restore_past_the_boundary_does_not_reannounce() {
        // restore() walks real corridor data, so use a real route: resuming a
        // save from beyond the boundary must adopt the zone silently.
        let world = get_world();
        let route = world.route_options("Atlanta", "Dallas", 3, false).unwrap()[0].clone();
        let mut trip = Trip::new(
            route,
            TruckState::default(),
            new_system("atlantic_southeast", Some(1), None, None, true),
            TripOptions::seeded(2),
        );
        let first = trip
            .timezone_crossings
            .iter()
            .find(|c| c.to_zone == CENTRAL)
            .copied()
            .expect("a crossing into Central");
        trip.restore(first.at_mi + 5.0, 120.0);
        trip.check_timezone();
        assert!(timezone_events(&trip).is_empty());
        assert_eq!(trip.current_timezone(), CENTRAL);
    }

    #[test]
    fn test_boundary_zigzag_is_not_a_crossing() {
        // A road that pokes over the line and comes back within the dwell
        // window must not move the clock at all.
        let leg =
            Leg::new("A", "B", 60.0, "I-40", "hills", Vec::new()).with_detail(CorridorDetail {
                state_miles: vec![StateMileage::new("Tennessee", 60.0)],
                route_points: vec![
                    rp(0.0, 35.96, -83.92),
                    rp(25.0, 35.90, -85.60), // briefly over the line
                    rp(30.0, 35.90, -85.20), // and straight back
                    rp(60.0, 35.96, -84.50),
                ],
                ..Default::default()
            });
        let trip = trip(ab(leg), 12.0);
        assert!(trip.timezone_crossings.is_empty());
        assert_eq!(trip.destination_timezone(), EASTERN);
    }

    // --- destination-local deadlines --------------------------------------------------

    #[test]
    fn test_deadline_reads_in_the_destination_zone() {
        let trip = trip(ab(tennessee_leg()), 12.0);
        assert_eq!(trip.deadline_clock_text(10.0, None), "9 PM Central Time");
        assert_eq!(
            trip.deadline_clock_text(20.0, None),
            "7 AM Central Time tomorrow"
        );
    }

    #[test]
    fn test_deadline_clock_is_anchored_at_trip_start() {
        let mut trip = trip(ab(tennessee_leg()), 12.0);
        let before = trip.deadline_clock_text(10.0, None);
        trip.game_minutes = 180.0; // three hours of driving later...
        assert_eq!(trip.deadline_clock_text(10.0, None), before); // ...the appointment holds
    }

    // --- real world data ---------------------------------------------------------------

    #[test]
    fn test_atlanta_to_dallas_crosses_into_central() {
        let world = get_world();
        let route = world.route_options("Atlanta", "Dallas", 3, false).unwrap()[0].clone();
        let trip = Trip::new(
            route,
            TruckState::default(),
            new_system("atlantic_southeast", Some(1), None, None, true),
            TripOptions::seeded(2),
        );
        assert_eq!(trip.start_timezone, EASTERN);
        assert_eq!(trip.destination_timezone(), CENTRAL);
        let keys: Vec<(&str, &str)> = trip
            .timezone_crossings
            .iter()
            .map(|c| (c.from_zone.key, c.to_zone.key))
            .collect();
        assert!(keys.contains(&("eastern", "central")));
    }

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- JobBoardState detail lines"]
    fn test_dispatch_detail_quotes_the_local_appointment() {
        // TODO(port): the F1 detail says "deliver by about" and names a zone.
    }
}
