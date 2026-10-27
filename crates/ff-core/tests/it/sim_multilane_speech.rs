//! Track D: multilane speech -- lane counts spoken from baked lane_segments
//! (port of `tests/test_multilane_speech.py`).
//!
//! Speech wiring only: the road status and route briefing say the lane count,
//! and a callout fires when the count changes mid-leg. No traffic, no
//! lane-position mechanics. Honest absence: legs with no baked lane data say
//! nothing.

use crate::sim_support::*;
use ff_core::data::world_models::{CorridorDetail, LaneSegment, Leg, Route};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{
    congestion_ratio, leg_lane_count, TripEventKind, MAX_DRIVABLE_LANES,
};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherSystem;

/// `tests/test_multilane_speech.py::_leg`.
fn leg(a: &str, b: &str, miles: f64, segs: Vec<LaneSegment>) -> Leg {
    Leg::new(a, b, miles, "US-1", "flat", Vec::new()).with_detail(CorridorDetail {
        lane_segments: segs,
        ..Default::default()
    })
}

// -- LaneSegment.your_side ---------------------------------------------------

#[test]
fn test_your_side_oneway_is_the_tagged_count() {
    let seg = LaneSegment {
        start_mi: 0.0,
        end_mi: 5.0,
        lanes: 3,
        oneway: true,
        ..Default::default()
    };
    assert_eq!(seg.your_side(), 3); // symmetric carriageway assumption
    assert!(seg.divided());
}

#[test]
fn test_your_side_undivided_splits_the_total() {
    let seg = LaneSegment {
        start_mi: 0.0,
        end_mi: 5.0,
        lanes: 4,
        oneway: false,
        ..Default::default()
    };
    assert_eq!(seg.your_side(), 2);
    assert!(!seg.divided());
    // a two-lane road is one lane your side, never zero
    let two = LaneSegment {
        start_mi: 0.0,
        end_mi: 1.0,
        lanes: 2,
        ..Default::default()
    };
    assert_eq!(two.your_side(), 1);
}

/// OSM's `lanes:forward` follows the way's drawing direction, which the
/// bake never compared with the leg's, so an uneven split cannot say which
/// side is ours. A two-lane road with a passing lane one way handed the truck
/// the oncoming passing lane on about half of those stretches, and full lane
/// keeping pulled out into it to pass (player report, 2026-10-13). The truck
/// gets the smaller side, whichever way it drives.
#[test]
fn test_your_side_never_takes_the_wider_side_of_an_uneven_split() {
    let passing_lane = LaneSegment {
        start_mi: 0.0,
        end_mi: 5.0,
        lanes: 3,
        lanes_forward: 2,
        lanes_backward: 1,
        ..Default::default()
    };
    assert_eq!(passing_lane.your_side(), 1);
    // The same road tagged from the other end, and with one side tagged.
    for (forward, backward) in [(1, 2), (2, 0), (0, 2)] {
        let seg = LaneSegment {
            lanes_forward: forward,
            lanes_backward: backward,
            ..passing_lane.clone()
        };
        assert_eq!(seg.your_side(), 1, "forward {forward} backward {backward}");
    }
    // An even split with a centre turn lane in the total is still two a side.
    let five = LaneSegment {
        lanes: 5,
        lanes_forward: 2,
        lanes_backward: 2,
        ..passing_lane.clone()
    };
    assert_eq!(five.your_side(), 2);
    // A tag that claims the whole road for one side still leaves one lane.
    let mistagged = LaneSegment {
        lanes: 2,
        lanes_forward: 2,
        lanes_backward: 0,
        ..passing_lane
    };
    assert_eq!(mistagged.your_side(), 1);
}

// -- Route.lane_summary (briefing) -------------------------------------------

#[test]
fn test_route_lane_summary_reports_dominant_and_divided() {
    let route = Route::from_legs(
        vec!["a".to_string(), "b".to_string()],
        vec![leg(
            "a",
            "b",
            10.0,
            vec![LaneSegment {
                start_mi: 0.0,
                end_mi: 10.0,
                lanes: 2,
                oneway: true,
                ..Default::default()
            }],
        )],
    );
    assert_eq!(route.lane_summary(), "mostly divided, two lanes your side");
    assert!(route.describe("").contains("two lanes your side"));
}

#[test]
fn test_route_lane_summary_empty_without_data() {
    let route = Route::from_legs(
        vec!["a".to_string(), "b".to_string()],
        vec![leg("a", "b", 10.0, Vec::new())],
    );
    assert_eq!(route.lane_summary(), "");
    assert!(!route.describe("").contains("lanes your side"));
}

#[test]
fn test_route_lane_summary_gives_an_uneven_split_its_smaller_side_both_ways() {
    // The bake's forward is the OSM way's, not the leg's, so the briefing
    // cannot promise three lanes either way; see
    // `test_your_side_never_takes_the_wider_side_of_an_uneven_split`.
    let seg = LaneSegment {
        start_mi: 0.0,
        end_mi: 10.0,
        lanes: 5,
        lanes_forward: 3,
        lanes_backward: 2,
        ..Default::default()
    };
    let fwd = Route::from_legs(
        vec!["a".to_string(), "b".to_string()],
        vec![leg("a", "b", 10.0, vec![seg.clone()])],
    );
    let rev = Route::from_legs(
        vec!["b".to_string(), "a".to_string()],
        vec![leg("a", "b", 10.0, vec![seg])],
    );
    assert!(fwd.lane_summary().contains("two lanes your side"));
    assert!(rev.lane_summary().contains("two lanes your side"));
}

// -- Trip: status readout ----------------------------------------------------

/// `tests/test_multilane_speech.py::_trip`.
fn city_trip(cities: &[&str]) -> Trip {
    let route = route_from_cities(world(), cities);
    Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("heartland", Some(1), None, None, true),
        TripOptions {
            seed: Some(1),
            world: Some(world()),
            ..Default::default()
        },
    )
}

#[test]
fn test_status_readout_speaks_lane_count() {
    let mut trip = city_trip(&["Albuquerque", "Gallup"]);
    trip.position_mi = 20.0; // rural I-40, divided two lanes
    assert_eq!(trip.current_lane_text(), "divided, two lanes your side");
    assert!(trip.progress_summary(true).contains("lanes your side"));
}

#[test]
fn test_status_readout_silent_without_lane_data() {
    let mut trip = city_trip(&["Albuquerque", "Gallup"]);
    trip.position_mi = 0.5; // leg start, before lane coverage begins
    assert!(trip.lanes_at(None).is_none());
    assert_eq!(trip.current_lane_text(), "");
}

// -- Trip: mid-leg change callout --------------------------------------------

/// `tests/test_multilane_speech.py::_lane_callouts`.
fn lane_callouts(trip: &mut Trip, mileposts: &[f64]) -> Vec<String> {
    let mut out = Vec::new();
    trip.position_mi = 0.0;
    trip.check_lane_changes(); // seed
    for mp in mileposts {
        trip.position_mi = *mp;
        trip.events.clear();
        trip.check_lane_changes();
        out.extend(
            trip.events
                .iter()
                .filter(|e| e.kind == TripEventKind::Lane)
                .map(|e| e.text().to_string()),
        );
    }
    out
}

#[test]
fn test_lane_change_callouts_widen_and_narrow() {
    // Albuquerque->Gallup baked 4 -> 3 -> 4 -> 2 lanes, and this asserted all
    // three transitions.
    //
    // MAX_DRIVABLE_LANES caps the spoken count at three, so the first three
    // runs collapse to one and only the narrowing to two survives. That is the
    // point of the cap rather than a loss: "widens to four lanes" named a lane
    // the driver cannot be placed in, because lane_label has three names. The
    // remaining callout is the one a driver can act on.
    let mut trip = city_trip(&["Albuquerque", "Gallup"]); // capped: 3 -> 2
    let msgs = lane_callouts(&mut trip, &[4.0, 6.0, 11.0, 15.0, 40.0]);
    assert!(
        msgs.iter().any(|m| m.contains("Down to two lanes")),
        "{msgs:?}"
    );
    // Nothing may name a fourth or fifth lane any more.
    let named: Vec<&String> = msgs
        .iter()
        .filter(|m| m.contains("four lanes") || m.contains("five lanes"))
        .collect();
    assert!(named.is_empty(), "{msgs:?}");
}

#[test]
fn test_short_runs_collapse_no_spam() {
    // ABQ has 0.3-0.5 mi 5-lane slivers; none should become a callout.
    let mut trip = city_trip(&["Albuquerque", "Gallup"]);
    let mileposts: Vec<f64> = (0..300).map(|p| p as f64 / 2.0).collect();
    let msgs = lane_callouts(&mut trip, &mileposts);
    assert!(!msgs.iter().any(|m| m.contains("five lanes")), "{msgs:?}");
}

#[test]
fn test_callouts_seeded_on_resume_do_not_replay() {
    // Starting already past a boundary must not announce it.
    let mut trip = city_trip(&["Albuquerque", "Gallup"]);
    trip.position_mi = 40.0; // past every ABQ-metro change
    trip.check_lane_changes(); // seeds silently
    trip.events.clear();
    trip.position_mi = 45.0;
    trip.check_lane_changes();
    assert!(!trip.events.iter().any(|e| e.kind == TripEventKind::Lane));
}

#[test]
fn test_synthetic_change_message_direction() {
    assert_eq!(
        Trip::lane_change_message(2, 3),
        "Road widens to three lanes your side."
    );
    assert_eq!(
        Trip::lane_change_message(3, 1),
        "Down to one lane your side."
    );
}

#[test]
fn test_the_driver_is_never_offered_more_lanes_than_speech_can_name() {
    // Owner, 2026-08-19: "We don't support five lane highways yet, just three."
    //
    // It is a speech limit before it is a driving one. `lane_label` has three
    // names -- right, left, middle -- so on a road of four or more, every
    // interior lane is announced as "the middle lane" and a player working by
    // ear cannot tell which one they are in, nor which one just came open.
    //
    // The HPMS bake records real per-direction counts and they run to six on
    // urban freeways. That record stays true; the cap is on what the DRIVER is
    // offered.
    assert_eq!(MAX_DRIVABLE_LANES, 3);

    let w = world();
    let route = route_from_cities(w, &["Chicago", "Indianapolis"]);
    let make = |route: Route| {
        Trip::new(
            route,
            TruckState::default(),
            weather("midwest", 1),
            TripOptions {
                seed: Some(2),
                world: Some(w),
                ..Default::default()
            },
        )
    };
    let trip = make(route.clone());

    let mut mile = 0;
    while (mile as f64) < trip.total_miles() {
        assert!(trip.lane_count_at(Some(mile as f64)) <= MAX_DRIVABLE_LANES);
        mile += 25;
    }

    // And the cap is real even when the road really does carry more.
    let mut wide = with_corridor(&route.legs[0], |detail| detail.lane_segments = Vec::new());
    wide.lanes = 6;
    let route = replace_leg(&route, 0, wide);
    let trip = make(route.clone());
    assert_eq!(leg_lane_count(Some(&route.legs[0])), 6); // the record is untouched
    assert_eq!(trip.lane_count_at(Some(0.0)), MAX_DRIVABLE_LANES);
}

#[test]
fn test_traffic_capacity_still_uses_the_real_lane_count() {
    // The other half: clamping capacity too would invent jams.
    //
    // A six-lane urban freeway carrying its real volume flows. Divide that
    // volume by three lanes because three is all the driver can be put in, and
    // the congestion model reports stop-and-go on a road that is moving --
    // which is why the cap lives in `lane_count_at` and not in the bake or in
    // `leg_aadt_at`.
    let six = congestion_ratio(250_000.0, 8.0, 6, false);
    let three = congestion_ratio(250_000.0, 8.0, 3, false);
    assert!(three > six);
    // The real count is what keeps this freeway out of the jam band.
    assert!(six < 1.0 && 1.0 < three);
}

#[test]
fn test_no_lane_callout_names_a_lane_the_driver_cannot_be_in() {
    // Owner playtest, Denver->Silverthorne, 2026-08-19: the transcript said
    // "Down to five lanes your side".
    //
    // MAX_DRIVABLE_LANES was applied in Trip.lane_count_at, but the lane-count
    // CALLOUT builds its runs straight from leg.lane_segments, so it read the
    // raw OSM count and named a road the driver cannot be placed on. Capping
    // one path is not capping the concept.
    let w = world();
    let route = route_from_cities(w, &["Denver", "Silverthorne"]);
    let trip = Trip::new(
        route,
        TruckState::default(),
        weather("mountain", 1),
        TripOptions {
            seed: Some(2),
            world: Some(w),
            ..Default::default()
        },
    );
    let runs = trip.build_lane_runs();
    assert!(
        !runs.is_empty(),
        "the route has no baked lane data to check"
    );
    let worst = runs.iter().map(|r| r.lanes).max().expect("runs");
    assert!(
        worst <= MAX_DRIVABLE_LANES,
        "a run claims {worst} lanes your side"
    );
}
