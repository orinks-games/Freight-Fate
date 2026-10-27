//! Baked OSM maxspeed: the leg step-function and the runtime preference of
//! a real posted limit over the highway/region heuristic (the pure parts of
//! `tests/test_maxspeed.py`).

use crate::sim_support::*;
use ff_core::data::world_models::{CorridorDetail, Leg, Route, SpeedLimitSample, StateMileage};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{corridor_speed_limit, leg_speed_limit_at};
use ff_core::sim::vehicle::TruckState;

fn sample(at_mi: f64, mph: Option<f64>) -> SpeedLimitSample {
    SpeedLimitSample {
        at_mi,
        mph,
        source: String::new(),
        hgv: false,
    }
}

fn hgv_sample(at_mi: f64, mph: f64) -> SpeedLimitSample {
    SpeedLimitSample {
        at_mi,
        mph: Some(mph),
        source: String::new(),
        hgv: true,
    }
}

fn leg(speed_limits: Vec<SpeedLimitSample>) -> Leg {
    Leg::new("A", "B", 100.0, "I-95", "flat", Vec::new()).with_detail(CorridorDetail {
        speed_limits,
        ..Default::default()
    })
}

// --- leg step function ------------------------------------------------------

#[test]
fn test_unbaked_leg_returns_none() {
    assert!(leg_speed_limit_at(&leg(Vec::new()), 50.0).is_none());
}

#[test]
fn test_single_sample_applies_everywhere() {
    let leg = leg(vec![sample(0.0, Some(65.0))]);
    assert_eq!(leg_speed_limit_at(&leg, 0.0), Some(65.0));
    assert_eq!(leg_speed_limit_at(&leg, 99.0), Some(65.0));
}

#[test]
fn test_step_function_picks_last_sample_at_or_before_offset() {
    let leg = leg(vec![
        sample(0.0, Some(65.0)),
        sample(40.0, Some(70.0)),
        sample(80.0, Some(55.0)),
    ]);
    assert_eq!(leg_speed_limit_at(&leg, 10.0), Some(65.0));
    assert_eq!(leg_speed_limit_at(&leg, 40.0), Some(70.0));
    assert_eq!(leg_speed_limit_at(&leg, 79.9), Some(70.0));
    assert_eq!(leg_speed_limit_at(&leg, 90.0), Some(55.0));
}

#[test]
fn test_offset_before_first_sample_uses_first() {
    let leg = leg(vec![sample(10.0, Some(60.0)), sample(50.0, Some(70.0))]);
    assert_eq!(leg_speed_limit_at(&leg, 0.0), Some(60.0));
}

// --- coverage-gap markers ---------------------------------------------------

#[test]
fn test_gap_marker_answers_none_instead_of_holding_the_last_posting() {
    // The NY-12 lesson: a village 30 whose tagging ends must not rule the
    // untagged miles that follow.
    let leg = leg(vec![
        sample(0.0, Some(30.0)),
        sample(1.2, None),
        sample(40.0, Some(55.0)),
    ]);
    assert_eq!(leg_speed_limit_at(&leg, 0.5), Some(30.0));
    assert_eq!(leg_speed_limit_at(&leg, 20.0), None);
    assert_eq!(leg_speed_limit_at(&leg, 45.0), Some(55.0));
}

// --- runtime preference and fallback ---------------------------------------

/// A mile out on the open road, away from the urban-reduction radius.
fn open_road_mile(trip: &Trip) -> f64 {
    trip.total_miles() / 2.0
}

fn trip_for(route: Route, region: &str) -> Trip {
    Trip::new(
        route,
        TruckState::default(),
        weather(region, 1),
        TripOptions {
            seed: Some(2),
            world: Some(world()),
            ..Default::default()
        },
    )
}

fn state_leg(highway: &str, state: &str, speed_limits: Vec<SpeedLimitSample>) -> Leg {
    Leg::new("A", "B", 100.0, highway, "flat", Vec::new()).with_detail(CorridorDetail {
        state_miles: vec![StateMileage::new(state, 100.0)],
        speed_limits,
        ..Default::default()
    })
}

fn ab(leg: Leg) -> Route {
    Route::from_legs(vec!["A".to_string(), "B".to_string()], vec![leg])
}

#[test]
fn test_runtime_prefers_baked_maxspeed_over_heuristic() {
    let route = first_route_option(world(), "Chicago", "St. Louis");
    let heuristic = corridor_speed_limit(&route.legs[0].highway, "heartland");
    let baked = heuristic + 5.0; // a value the heuristic would never produce here
                                 // Bake the value onto every leg -- the sampled open-road mile can land
                                 // on any of them.
    let legs: Vec<Leg> = route
        .legs
        .iter()
        .map(|leg| with_corridor(leg, |d| d.speed_limits = vec![sample(0.0, Some(baked))]))
        .collect();
    let route = Route::from_legs(route.cities.clone(), legs);
    let trip = trip_for(route, "great_lakes");
    assert_eq!(trip.corridor_limit_at(open_road_mile(&trip)), baked);
}

#[test]
fn test_runtime_caps_general_baked_limit_to_state_truck_limit() {
    let leg = state_leg("I-5", "California", vec![sample(0.0, Some(65.0))]);
    let trip = trip_for(ab(leg), "california");
    assert_eq!(trip.corridor_limit_at(50.0), 55.0);
}

#[test]
fn test_runtime_keeps_truck_specific_baked_limit() {
    let leg = state_leg("I-5", "California", vec![hgv_sample(0.0, 50.0)]);
    let trip = trip_for(ab(leg), "california");
    assert_eq!(trip.corridor_limit_at(50.0), 50.0);
}

#[test]
fn test_runtime_caps_oregon_and_arizona_truck_limits() {
    // Updated by the 2026-07-19 statute audit.
    for (state, highway, baked, expected) in [
        ("Oregon", "I-5", 65.0, 55.0),
        ("Arizona", "I-40", 75.0, 65.0),
    ] {
        let leg = state_leg(highway, state, vec![sample(0.0, Some(baked))]);
        let trip = trip_for(ab(leg), "pacific_northwest");
        assert_eq!(trip.corridor_limit_at(50.0), expected);
    }
}

#[test]
fn test_idaho_nevada_north_dakota_no_longer_capped() {
    for (state, baked) in [("Idaho", 75.0), ("Nevada", 80.0), ("North Dakota", 80.0)] {
        let leg = state_leg("I-84", state, vec![sample(0.0, Some(baked))]);
        let trip = trip_for(ab(leg), "mountain_west");
        assert_eq!(trip.corridor_limit_at(50.0), baked);
    }
}

#[test]
fn test_montana_split_is_scoped_to_road_class() {
    for (highway, expected) in [("I-90", 70.0), ("US-2", 65.0)] {
        let leg = state_leg(highway, "Montana", vec![sample(0.0, Some(80.0))]);
        let trip = trip_for(ab(leg), "mountain_west");
        assert_eq!(trip.corridor_limit_at(50.0), expected);
    }
}

#[test]
fn test_hgv_tag_is_trusted_only_as_far_as_the_statute_allows() {
    let trip = |state: &str, highway: &str, mph: f64| {
        trip_for(
            ab(state_leg(highway, state, vec![hgv_sample(0.0, mph)])),
            "pacific_northwest",
        )
    };
    // Oregon declares a corridor maximum of 65, so a tagged 65 survives.
    assert_eq!(trip("Oregon", "I-84", 65.0).corridor_limit_at(50.0), 65.0);
    // ...but not beyond it.
    assert_eq!(trip("Oregon", "I-84", 75.0).corridor_limit_at(50.0), 65.0);
    // California permits no corridor exception: the stray tag is clamped.
    assert_eq!(
        trip("California", "I-5", 60.0).corridor_limit_at(50.0),
        55.0
    );
    // A class-scoped split must not let a tag borrow the interstate number
    // for a back highway (Montana: 70 interstate, 65 elsewhere).
    assert_eq!(trip("Montana", "US-2", 70.0).corridor_limit_at(50.0), 65.0);
}

#[test]
fn test_runtime_reads_baked_profile_in_reverse_direction() {
    let leg = state_leg(
        "I-65",
        "Indiana",
        vec![sample(0.0, Some(55.0)), sample(80.0, Some(70.0))],
    );
    let route = Route::from_legs(vec!["B".to_string(), "A".to_string()], vec![leg]);
    let trip = trip_for(route, "great_lakes");
    assert_eq!(trip.corridor_limit_at(10.0), 65.0);
    assert_eq!(trip.corridor_limit_at(90.0), 55.0);
}

#[test]
fn test_runtime_falls_back_to_heuristic_without_a_profile() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let stripped = with_corridor(&route.legs[0], |d| d.speed_limits.clear());
    let route = replace_leg(&route, 0, stripped);
    let trip = trip_for(route.clone(), "great_lakes");
    let mile = open_road_mile(&trip);
    let (leg_i, _) = trip.leg_at_mile(mile);
    let expected = corridor_speed_limit(&route.legs[leg_i].highway, &trip.region_at(mile));
    assert_eq!(trip.corridor_limit_at(mile), expected);
}

#[test]
fn test_baked_limit_wins_near_city() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let baked = with_corridor(&route.legs[0], |d| {
        d.speed_limits = vec![sample(0.0, Some(75.0))]
    });
    let route = replace_leg(&route, 0, baked);
    let trip = trip_for(route, "great_lakes");
    // Real posted data is authoritative; the city cap is only a fallback
    // when the route lacks baked speed samples.
    assert_eq!(trip.corridor_limit_at(0.0), 75.0);
}

fn split_limit_trip(state: &str, mph: f64, hgv: bool, highway: &str) -> Trip {
    let sample = SpeedLimitSample {
        at_mi: 0.0,
        mph: Some(mph),
        source: String::new(),
        hgv,
    };
    trip_for(ab(state_leg(highway, state, vec![sample])), "california")
}

#[test]
fn test_split_limit_reported_whether_the_cap_or_the_tag_produced_it() {
    // A California 55 arrives two ways and the driver must not be able to
    // tell them apart (player report, 2026-07-19).
    let mut tagged = split_limit_trip("California", 55.0, true, "US-395");
    let mut capped = split_limit_trip("California", 65.0, false, "I-80");
    assert_eq!(tagged.speed_limit_at(50.0).0, 55.0);
    assert_eq!(capped.speed_limit_at(50.0).0, 55.0);
    assert_eq!(
        tagged.truck_limit_at(50.0),
        (true, Some("California".to_string()))
    );
    assert_eq!(
        capped.truck_limit_at(50.0),
        (true, Some("California".to_string()))
    );
}

#[test]
fn test_plain_posting_is_not_reported_as_a_truck_limit() {
    assert_eq!(
        split_limit_trip("Nevada", 80.0, false, "I-80").truck_limit_at(50.0),
        (false, None)
    );
    assert_eq!(
        split_limit_trip("Texas", 75.0, false, "I-80").truck_limit_at(50.0),
        (false, None)
    );
}

#[test]
fn test_zone_owns_the_reason_over_a_split_limit() {
    // Inside construction the cone is why the number dropped, not the state
    // line.
    let mut trip = split_limit_trip("California", 65.0, false, "I-80");
    let Some(zone) = trip.zones.first().cloned() else {
        return; // no zone generated for this seed
    };
    assert_eq!(
        trip.truck_limit_at((zone.start_mi + zone.end_mi) / 2.0),
        (false, None)
    );
}

#[test]
fn test_local_truck_posting_is_a_truck_limit_without_crediting_the_state() {
    // A truck-tagged posting in a state with no statutory split is still a
    // truck limit, but no state law explains it.
    assert_eq!(
        split_limit_trip("Texas", 45.0, true, "I-80").truck_limit_at(50.0),
        (true, None)
    );
}
