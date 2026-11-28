//! Speech generation clocks: places fire by mile, chatter by sitting time.
//!
//! Joshua's split (2026-08-28): road events (exits, curves, scales, work
//! zones, lane-count changes, town names) fire once per place -- same count
//! at 20x or 1:1 over the same miles. Chatter (CB, weather color, billboards,
//! flavor landmarks) uses a wall-clock / sitting budget, so 20x must not
//! spawn 20x pokes over the same real sitting time. Night, fuel, and HOS
//! stay on drive time and are not retuned here.

use crate::sim_support::*;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::sim::hos::{REALISTIC_LIMITS, RELAXED_LIMITS};
use ff_core::sim::road_event_pacing::CHATTER_GAP_REAL_S;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{RoadsideCallout, TripEvent, TripEventKind, CB_CALLS_PER_RUN};
use ff_core::sim::vehicle::TruckState;

fn highway_trip(time_scale: f64) -> Trip {
    let world = world();
    let mut trip = make_trip_with(
        world,
        "Chicago",
        "Indianapolis",
        TripOptions {
            seed: Some(7),
            time_scale,
            start_hour: 12.0,
            hazard_scale: 0.0,
            world: Some(world),
            ..Default::default()
        },
    );
    trip.traffic_manager.rolling_bubble = false;
    trip.truck.transmission.automatic = true;
    trip.truck.start_engine();
    trip.truck.parking_brake = false;
    trip.truck.velocity_mps = 65.0 / 2.23694;
    trip
}

fn drive_sitting(trip: &mut Trip, sitting_s: f64, dt: f64) -> Vec<TripEvent> {
    let mut all = Vec::new();
    let start = trip.sitting_s;
    let mut guard = 0;
    while trip.sitting_s - start < sitting_s - 1e-9 && !trip.finished && guard < 200_000 {
        all.extend(trip.update(dt));
        guard += 1;
    }
    all
}

fn drive_miles(trip: &mut Trip, end_mi: f64, dt: f64) -> Vec<TripEvent> {
    let mut all = Vec::new();
    let mut guard = 0;
    while trip.position_mi < end_mi && !trip.finished && guard < 400_000 {
        all.extend(trip.update(dt));
        guard += 1;
    }
    all
}

fn chatter_events(events: &[TripEvent]) -> usize {
    events
        .iter()
        .filter(|e| match e.kind {
            TripEventKind::Billboard => true,
            TripEventKind::Landmark => {
                !e.data.explains_limit.unwrap_or(false)
                    && e.data.category.as_deref() != Some("village")
            }
            TripEventKind::WeatherChange => true,
            TripEventKind::GpsCue => e.data.cb_patrol.is_some(),
            _ => false,
        })
        .count()
}

fn geo_lane_events(events: &[TripEvent]) -> usize {
    events
        .iter()
        .filter(|e| e.kind == TripEventKind::Lane)
        .count()
}

fn village_events(events: &[TripEvent]) -> usize {
    events
        .iter()
        .filter(|e| {
            e.kind == TripEventKind::Landmark && e.data.category.as_deref() == Some("village")
        })
        .count()
}

#[test]
fn test_default_clock_is_still_compressed_standard() {
    assert_eq!(TripOptions::default().time_scale, 20.0);
    assert_eq!(tuning_for_time_scale(20.0).name, "standard");
    assert_eq!(tuning_for_time_scale(1.0).name, "real time");
}

#[test]
fn test_hos_numbers_unchanged_by_the_speech_clock_split() {
    assert_eq!(REALISTIC_LIMITS, (11.0 * 60.0, 14.0 * 60.0, 8.0 * 60.0));
    assert_eq!(RELAXED_LIMITS, REALISTIC_LIMITS);
    assert_eq!(CB_CALLS_PER_RUN, 2);
    assert_eq!(CHATTER_GAP_REAL_S, 90.0);
}

#[test]
fn test_20x_does_not_multiply_ambient_or_cb_over_the_same_sitting_time() {
    // Dense flavor so 20x would pile up without a sitting budget: a billboard
    // every two miles, starting immediately.
    fn plant(trip: &mut Trip) {
        trip.billboards = (0..40)
            .map(|i| {
                RoadsideCallout::new(
                    &format!("billboard:plant:{i}"),
                    0.4 + i as f64 * 2.0,
                    "billboard",
                    "Billboard: planted scenery.",
                )
            })
            .collect();
        trip.landmarks = Vec::new();
        // A river's name comes with its water and never waits on the
        // budget, so the route's own rivers would count here.
        trip.rivers = Vec::new();
        trip.announced_billboards.clear();
        trip.announced_landmarks.clear();
        trip.last_chatter_s = None;
        trip.posts = Vec::new();
    }

    let sitting = 120.0;
    let dt = 0.25;

    let mut real = highway_trip(1.0);
    plant(&mut real);
    let real_events = drive_sitting(&mut real, sitting, dt);
    let real_chatter = chatter_events(&real_events);

    let mut compressed = highway_trip(20.0);
    plant(&mut compressed);
    let compressed_events = drive_sitting(&mut compressed, sitting, dt);
    let compressed_chatter = chatter_events(&compressed_events);

    // Same wheel time: 20x covers ~20x the miles, but flavor is budgeted.
    assert!(
        compressed.position_mi > real.position_mi * 8.0,
        "20x should have burned far more road ({:.1} mi vs {:.1} mi)",
        compressed.position_mi,
        real.position_mi
    );
    assert!(
        compressed_chatter <= real_chatter + 1,
        "20x multiplied ambient/CB over sitting time: 1x={real_chatter} 20x={compressed_chatter}"
    );
    // And 20x must not be a 20x pile-up of the planted billboards.
    let uncompressed_would_be = compressed
        .billboards
        .iter()
        .filter(|c| c.at_mi <= compressed.position_mi)
        .count();
    assert!(
        uncompressed_would_be >= 8,
        "the plant should have given 20x many places to poke, got {uncompressed_would_be}"
    );
    assert!(
        compressed_chatter * 4 < uncompressed_would_be,
        "sitting budget did not skip extras: spoke {compressed_chatter} of {uncompressed_would_be} places"
    );
}

#[test]
fn a_placed_attraction_sign_is_read_even_right_after_a_joke_board() {
    // Kingston Penitentiary, 2026-11-22: a pool board a mile before the
    // placed sign spent the chatter budget and the approved board was lost.
    let mut trip = highway_trip(4.0);
    trip.billboards = vec![RoadsideCallout::new(
        "billboard:2.0",
        2.0,
        "billboard",
        "Billboard: planted joke.",
    )];
    trip.landmarks = vec![RoadsideCallout::new(
        "landmark:0:3.0:Planted Museum",
        3.0,
        "billboard_sign",
        "Billboard: The Planted Museum, ahead.",
    )];
    trip.announced_billboards.clear();
    trip.announced_landmarks.clear();
    trip.last_chatter_s = None;
    trip.posts = Vec::new();
    let events = drive_miles(&mut trip, 5.0, 0.25);
    let spoken: Vec<String> = events
        .iter()
        .filter(|e| matches!(e.kind, TripEventKind::Billboard | TripEventKind::Landmark))
        .map(|e| e.text().to_string())
        .collect();
    assert!(
        spoken.iter().any(|t| t.contains("planted joke")),
        "the joke board should play first: {spoken:?}"
    );
    assert!(
        spoken.iter().any(|t| t.contains("Planted Museum")),
        "the placed sign was silenced by the joke board's budget: {spoken:?}"
    );
}

#[test]
fn kerrville_rivers_survive_nearby_towns_and_a_spent_chatter_budget() {
    // A river's name comes with its water (`Trip::check_rivers`): never
    // spaced away by a town, never waiting on the chatter budget.
    let world = world();
    let mut trip = make_trip_with(world, "Kerrville", "Uvalde", TripOptions::default());
    assert!(
        !trip.landmarks.iter().any(|c| c.category == "river"),
        "rivers are not scenery callouts"
    );
    let crossings: Vec<_> = trip
        .rivers
        .iter()
        .map(|r| (r.at_mi, r.name.clone()))
        .collect();
    for name in ["Medina River", "Frio River", "Dry Frio River"] {
        let (at_mi, _) = crossings
            .iter()
            .find(|(_, river)| river == name)
            .unwrap_or_else(|| panic!("missing {name} from the route: {crossings:?}"));
        // Clear of the last river's water, then onto this one.
        trip.river = Default::default();
        trip.position_mi = *at_mi;
        trip.note_chatter_spoke();
        trip.check_rivers(0.1);
        assert!(
            std::mem::take(&mut trip.events).iter().any(|e| {
                e.kind == TripEventKind::Landmark
                    && e.data.category.as_deref() == Some("river")
                    && e.text() == format!("Crossing the {name}.")
            }),
            "{name} was not emitted with a spent chatter budget"
        );
        trip.check_rivers(0.1);
        assert!(trip.events.is_empty(), "{name} was emitted again");
    }
}

#[test]
fn a_landmark_line_that_names_a_town_takes_the_town_slot() {
    fn at(key: &str, mi: f64, category: &str, place: &str, spoken: &str) -> RoadsideCallout {
        let mut c = RoadsideCallout::new(key, mi, category, spoken);
        c.place = place.to_string();
        c
    }
    // Gravenhurst explains no limit, so its town name is heard only at the
    // loudest place tier; the Bethune line must replace it as a landmark,
    // not be spaced away two miles behind it.
    let towns = vec![
        at("g", 109.9, "village", "Gravenhurst", "Passing Gravenhurst."),
        {
            let mut grimshaw = at("s", 150.0, "village", "Grimshaw", "Entering Grimshaw.");
            grimshaw.explains_limit = true;
            grimshaw
        },
    ];
    let lines = vec![
        at(
            "b",
            109.9,
            "highway_marker",
            "Gravenhurst",
            "Passing Bethune Memorial House, in Gravenhurst.",
        ),
        at(
            "m",
            150.0,
            "highway_marker",
            "Grimshaw",
            "Entering Grimshaw, mile zero of the Mackenzie Highway.",
        ),
        // Same name, another mile: not the town's line.
        at(
            "x",
            105.0,
            "highway_marker",
            "Gravenhurst",
            "Gravenhurst Wharf.",
        ),
    ];
    let placed = Trip::space_landmarks(towns, lines);
    let said: Vec<(&str, &str, bool)> = placed
        .iter()
        .map(|c| (c.spoken.as_str(), c.category.as_str(), c.explains_limit))
        .collect();
    assert_eq!(
        said,
        vec![
            ("Gravenhurst Wharf.", "highway_marker", false),
            (
                "Passing Bethune Memorial House, in Gravenhurst.",
                "highway_marker",
                false
            ),
            (
                "Entering Grimshaw, mile zero of the Mackenzie Highway.",
                "village",
                true
            ),
        ]
    );
}

#[test]
fn test_geo_callouts_fire_once_per_place_at_either_clock() {
    fn plant(trip: &mut Trip) {
        trip.landmarks = vec![
            {
                let mut c =
                    RoadsideCallout::new("village:a", 2.0, "village", "Entering Millville.");
                c.explains_limit = false;
                c
            },
            RoadsideCallout::new("village:b", 8.0, "village", "Passing Hartsford."),
            RoadsideCallout::new("village:c", 14.0, "village", "Entering Colesburg."),
        ];
        trip.billboards = Vec::new();
        trip.announced_landmarks.clear();
        trip.announced_billboards.clear();
        trip.last_chatter_s = None;
    }

    let end_mi = 16.0;
    let dt = 0.25;

    let mut real = highway_trip(1.0);
    plant(&mut real);
    let real_events = drive_miles(&mut real, end_mi, dt);

    let mut compressed = highway_trip(20.0);
    plant(&mut compressed);
    let compressed_events = drive_miles(&mut compressed, end_mi, dt);

    assert_eq!(village_events(&real_events), 3, "1x missed a town name");
    assert_eq!(
        village_events(&compressed_events),
        3,
        "20x must still name each place once, not skip or repeat"
    );
}

#[test]
fn test_lane_count_changes_match_across_clocks_over_the_same_miles() {
    let world = world();
    let route = route_from_cities(world, &["Albuquerque", "Gallup"]);
    let make = |time_scale: f64| {
        let mut truck = TruckState::default();
        truck.transmission.automatic = true;
        truck.start_engine();
        truck.parking_brake = false;
        truck.velocity_mps = 65.0 / 2.23694;
        let mut trip = Trip::new(
            route.clone(),
            truck,
            weather("desert_southwest", 1),
            TripOptions {
                seed: Some(1),
                time_scale,
                hazard_scale: 0.0,
                world: Some(world),
                ..Default::default()
            },
        );
        trip.traffic_manager.rolling_bubble = false;
        trip
    };

    let end_mi = 40.0;
    let real_events = drive_miles(&mut make(1.0), end_mi, 0.25);
    let compressed_events = drive_miles(&mut make(20.0), end_mi, 0.25);
    assert_eq!(
        geo_lane_events(&real_events),
        geo_lane_events(&compressed_events),
        "lane-count changes are places: same miles, same count"
    );
}
