//! Navigation-cue wiring for curated interchanges, grounded exits/onramps and
//! metric navigation distances (the trip half of
//! `tests/test_interchanges.py`; phrasing and parsing live in
//! `data_interchanges.rs`).

use crate::sim_support::*;
use ff_core::data::world::World;
use ff_core::data::world_models::{Interchange, Leg, Route, Stop};
use ff_core::data::world_parsing::parse_interchange;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{
    ramp_speed_mph, NavigationCue, RampAdvisorySpeed, TripEventKind, Zone,
};
use ff_core::sim::trip_route_helpers::{leg_heading, nearest_exit_label};
use ff_core::sim::vehicle::TruckState;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn trip_on(route: Route, imperial: bool) -> Trip {
    Trip::new(
        route,
        TruckState::default(),
        weather("great_lakes", 1),
        TripOptions {
            seed: Some(2),
            imperial,
            world: Some(world()),
            ..Default::default()
        },
    )
}

/// A running automatic truck, as the drive-loop cases build one.
fn rolling_truck() -> TruckState {
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.start_engine();
    truck
}

/// One frame of the Python drive loop: shift, integrate, then step the trip.
fn drive_frame(trip: &mut Trip) -> Vec<ff_core::sim::trip_models::TripEvent> {
    trip.truck.auto_shift();
    trip.truck.update(1.0 / 60.0);
    trip.update(1.0 / 60.0)
}

/// `tests/test_interchanges.py::_route_with_interchange`.
fn route_with_interchange(w: &'static World, start: &str, end: &str) -> (Route, f64) {
    let route = first_route_option(w, start, end);
    let leg = route.legs[0].clone();
    let at = leg.miles / 2.0;
    let ix = Interchange {
        at_mi: at,
        exit_ref: "21".to_string(),
        destinations: strings(&["Lafayette"]),
        via: "US 52 West".to_string(),
        highway: leg.highway.clone(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    let edited = with_corridor(&leg, |detail| detail.interchanges = vec![ix]);
    (replace_leg(&route, 0, edited), at)
}

/// `tests/test_interchanges.py::_leg0_curated_stop`: the first curated stop of
/// leg 0 that applies in the travel direction.
fn leg0_curated_stop(route: &Route) -> (Leg, Stop) {
    let leg = route.legs[0].clone();
    let forward = route.cities[0] == leg.a;
    let stop = leg
        .stops
        .iter()
        .find(|s| s.curated() && s.applies_to_direction(forward))
        .expect("leg 0 has a curated stop in this direction")
        .clone();
    ((*leg).clone(), stop)
}

// --- cue wiring -------------------------------------------------------------

#[test]
fn test_interchange_produces_navigation_cue() {
    let (route, _) = route_with_interchange(world(), "Chicago", "Indianapolis");
    let trip = trip_on(route, true);
    let cues: Vec<&NavigationCue> = trip
        .navigation_cues
        .iter()
        .filter(|c| c.kind == "interchange")
        .collect();
    assert_eq!(cues.len(), 1);
    assert!(cues[0].text.contains("exit 21"), "{}", cues[0].text);
    assert!(cues[0].text.contains("Lafayette"), "{}", cues[0].text);
}

#[test]
fn test_interchange_cue_stays_silent_during_drive() {
    let (route, _) = route_with_interchange(world(), "Chicago", "Indianapolis");
    let mut trip = Trip::new(
        route,
        rolling_truck(),
        weather("great_lakes", 1),
        TripOptions {
            seed: Some(2),
            world: Some(world()),
            ..Default::default()
        },
    );
    trip.truck.throttle = 0.85;
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..(60 * 60 * 10) {
        for ev in drive_frame(&mut trip) {
            if ev
                .data
                .cue
                .as_ref()
                .is_some_and(|c| c.kind == "interchange")
            {
                seen.push(ev.text().to_string());
            }
        }
        if trip.finished {
            break;
        }
    }
    assert!(seen.is_empty(), "{seen:?}");
}

#[test]
fn test_reverse_direction_mirrors_interchange_position() {
    let w = world();
    let forward = first_route_option(w, "Chicago", "Indianapolis");
    let leg = forward.legs[0].clone();
    let at = leg.miles / 2.0 - 10.0;
    let ix = Interchange {
        at_mi: at,
        exit_ref: "21".to_string(),
        destinations: strings(&["Lafayette"]),
        via: "US 52 West".to_string(),
        highway: leg.highway.clone(),
        source: "OSM".to_string(),
        ..Default::default()
    };

    let forward = replace_leg(
        &forward,
        0,
        with_corridor(&leg, |detail| detail.interchanges = vec![ix.clone()]),
    );
    let fwd_trip = trip_on(forward, true);
    let fwd_cue = fwd_trip
        .navigation_cues
        .iter()
        .find(|c| c.kind == "interchange")
        .expect("forward interchange cue");

    let reverse = first_route_option(w, "Indianapolis", "Chicago");
    let rev_leg = reverse.legs[0].clone();
    let rev_miles = rev_leg.miles;
    let mirrored = Interchange {
        highway: rev_leg.highway.clone(),
        ..ix
    };
    let reverse = replace_leg(
        &reverse,
        0,
        with_corridor(&rev_leg, |detail| detail.interchanges = vec![mirrored]),
    );
    let rev_trip = trip_on(reverse, true);
    let rev_cue = rev_trip
        .navigation_cues
        .iter()
        .find(|c| c.kind == "interchange")
        .expect("reverse interchange cue");

    // Same physical exit, mirrored mileage from the opposite travel direction.
    assert!(
        (fwd_cue.at_mi - (rev_miles - rev_cue.at_mi)).abs() < 0.2,
        "{} vs {}",
        fwd_cue.at_mi,
        rev_miles - rev_cue.at_mi
    );
}

#[test]
fn observed_ramp_advisory_wins_by_travel_direction_and_missing_uses_calculation() {
    let w = world();
    let forward = first_route_option(w, "Chicago", "Indianapolis");
    let leg = forward.legs[0].clone();
    let at = leg.miles / 2.0;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ramp_advisory_harvest.json"
    )))
    .unwrap();
    let mut raw = fixture["expected_interchange"].clone();
    raw["at_mi"] = serde_json::json!(at);
    let ix = parse_interchange(&raw, leg.miles, "Chicago", "Indianapolis", &leg.highway).unwrap();
    let forward = replace_leg(
        &forward,
        0,
        with_corridor(&leg, |detail| detail.interchanges = vec![ix.clone()]),
    );
    assert_eq!(
        trip_on(forward, true).ramp_advisory_at(at),
        RampAdvisorySpeed::Observed {
            posted_mph: 25.0,
            truck_target_mph: 25.0,
        }
    );

    let reverse = first_route_option(w, "Indianapolis", "Chicago");
    let rev_leg = reverse.legs[0].clone();
    let route_mile = rev_leg.miles - at;
    let reverse = replace_leg(
        &reverse,
        0,
        with_corridor(&rev_leg, |detail| detail.interchanges = vec![ix]),
    );
    assert_eq!(
        trip_on(reverse, true).ramp_advisory_at(route_mile),
        RampAdvisorySpeed::Observed {
            posted_mph: 30.0,
            truck_target_mph: 30.0,
        }
    );

    let no_observation = first_route_option(w, "Chicago", "Indianapolis");
    assert!(matches!(
        trip_on(no_observation, true).ramp_advisory_at(at),
        RampAdvisorySpeed::Calculated { .. }
    ));
}

#[test]
fn passenger_vehicle_ramp_sign_never_raises_the_truck_target() {
    let w = world();
    let route = first_route_option(w, "Chicago", "Indianapolis");
    let leg = route.legs[0].clone();
    let at = leg.miles / 2.0;
    let raw = serde_json::json!({
        "at_mi": at,
        "exit_ref": "fixture",
        "source": "test fixture",
        "ramp_far_end": "surface",
        "ramp_advisory_forward": 70.0,
        "ramp_advisory_source": "OpenStreetMap maxspeed:advisory (read)",
    });
    let ix = parse_interchange(&raw, leg.miles, "Chicago", "Indianapolis", &leg.highway).unwrap();
    let route = replace_leg(
        &route,
        0,
        with_corridor(&leg, |detail| detail.interchanges = vec![ix]),
    );
    let trip = trip_on(route, true);
    let calculated = ramp_speed_mph(trip.corridor_limit_at(at), false);
    assert_eq!(
        trip.ramp_advisory_at(at),
        RampAdvisorySpeed::Observed {
            posted_mph: 70.0,
            truck_target_mph: calculated,
        }
    );
    assert_eq!(trip.ramp_speed_at(at), calculated);
}

#[test]
fn ramp_length_reads_the_travel_direction_and_is_none_when_absent() {
    let w = world();
    let route = first_route_option(w, "Chicago", "Indianapolis");
    let leg = route.legs[0].clone();
    let at = leg.miles / 2.0;
    let raw = serde_json::json!({
        "at_mi": at,
        "exit_ref": "fixture",
        "source": "test fixture",
        "ramp_length_ft_forward": 1320.0,
        "ramp_length_source": "derived from OpenStreetMap geometry (fixture)",
    });
    let ix = parse_interchange(&raw, leg.miles, "Chicago", "Indianapolis", &leg.highway).unwrap();
    let forward = replace_leg(
        &route,
        0,
        with_corridor(&leg, |detail| detail.interchanges = vec![ix.clone()]),
    );
    assert_eq!(trip_on(forward, true).ramp_length_mi_at(at), Some(0.25));

    // The same exit driven B->A has no length that way.
    let reverse = first_route_option(w, "Indianapolis", "Chicago");
    let rev_leg = reverse.legs[0].clone();
    let reverse = replace_leg(
        &reverse,
        0,
        with_corridor(&rev_leg, |detail| detail.interchanges = vec![ix]),
    );
    assert_eq!(
        trip_on(reverse, true).ramp_length_mi_at(rev_leg.miles - at),
        None
    );

    // No exit at all nearby.
    let empty = replace_leg(
        &route,
        0,
        with_corridor(&leg, |detail| detail.interchanges.clear()),
    );
    assert_eq!(trip_on(empty, true).ramp_length_mi_at(at), None);

    // A length without a source is refused at load.
    let mut unsourced = raw.clone();
    unsourced
        .as_object_mut()
        .unwrap()
        .remove("ramp_length_source");
    assert!(parse_interchange(
        &unsourced,
        leg.miles,
        "Chicago",
        "Indianapolis",
        &leg.highway
    )
    .is_err());
}

#[test]
fn baked_ramp_length_is_read_for_a_known_exit() {
    // I-65 Chicago to Indianapolis, its exits re-derived on its polyline
    // 2026-09-25 (OSM of 2026-06-22): exit 240 has a southbound ramp only;
    // exit 74A-B matched no gore either way.
    let w = world();
    let route = first_route_option(w, "Chicago", "Indianapolis");
    let trip = trip_on(route.clone(), true);
    let exit_240 = trip.ramp_length_mi_at(57.4).expect("exit 240 has a length");
    assert!((exit_240 - 1727.3 / 5280.0).abs() < 1e-9, "{exit_240}");
    assert_eq!(trip.ramp_length_mi_at(23.1), None);
    let reverse = first_route_option(w, "Indianapolis", "Chicago");
    let miles = reverse.legs[0].miles;
    assert_eq!(trip_on(reverse, true).ramp_length_mi_at(miles - 57.4), None);
}

#[test]
fn test_next_exit_context_mentions_flavor_exit() {
    let (route, _) = route_with_interchange(world(), "Chicago", "Indianapolis");
    let mut trip = trip_on(route, true);
    trip.navigation_cues = vec![NavigationCue::new(
        "interchange:0:50:21",
        "interchange",
        50.0,
        "exit 21 for US-52 West toward Lafayette",
        "",
    )];
    trip.position_mi = 40.0;
    assert_eq!(
        trip.next_navigation_context(true),
        "Destination Indianapolis ahead."
    );
    assert_eq!(
        trip.next_exit_context(),
        "Next listed exit in 10 miles: exit 21 for US-52 West toward Lafayette."
    );
}

#[test]
fn test_next_navigation_context_prioritizes_actionable_stop_over_exit() {
    let (route, _) = route_with_interchange(world(), "Chicago", "Indianapolis");
    let mut trip = trip_on(route, true);
    trip.navigation_cues = vec![
        NavigationCue::new(
            "interchange:0:40:21",
            "interchange",
            40.0,
            "exit 21 for US-52 West toward Lafayette",
            "",
        ),
        NavigationCue::new(
            "rest_stop:0:50:pilot",
            "rest_stop",
            50.0,
            "travel center ahead at exit 26",
            "",
        ),
    ];
    trip.position_mi = 30.0;
    assert_eq!(
        trip.next_navigation_context(true),
        "Next stop in 20 miles: travel center ahead at exit 26."
    );
    assert_eq!(
        trip.next_exit_context(),
        "Next listed exit in 10 miles: exit 21 for US-52 West toward Lafayette."
    );
}

// --- Scope A: grounded exits, ramps, onramps --------------------------------

#[test]
fn test_leg_heading_follows_route_numbering() {
    let w = world();
    // Odd routes are signed N/S even where the geometry runs diagonally
    // (I-95 NY->Philadelphia trends southwest but is signed South).
    assert_eq!(
        leg_heading(w, "I-95", "new_york_ny_us", "philadelphia_pa_us"),
        "South"
    );
    assert_eq!(
        leg_heading(w, "I-95", "philadelphia_pa_us", "new_york_ny_us"),
        "North"
    );
    // Even routes are signed E/W.
    assert_eq!(
        leg_heading(w, "I-80", "chicago_il_us", "cleveland_oh_us"),
        "East"
    );
    assert_eq!(
        leg_heading(w, "I-80", "cleveland_oh_us", "chicago_il_us"),
        "West"
    );
    // No route number -> no heading.
    assert_eq!(
        leg_heading(w, "Local Road", "chicago_il_us", "cleveland_oh_us"),
        ""
    );
}

#[test]
fn test_nearest_exit_label_respects_tolerance() {
    let base = first_route_option(world(), "Chicago", "Indianapolis").legs[0].clone();
    let leg = with_corridor(&base, |detail| {
        detail.interchanges = vec![
            Interchange {
                at_mi: 50.0,
                exit_ref: "7".to_string(),
                source: "x".to_string(),
                ..Default::default()
            },
            Interchange {
                at_mi: 80.0,
                destinations: strings(&["Town"]),
                source: "x".to_string(),
                ..Default::default()
            },
        ]
    });
    assert_eq!(nearest_exit_label(&leg, 51.0, 2.0), "exit 7"); // within 2.0 mi
    assert_eq!(nearest_exit_label(&leg, 55.0, 2.0), ""); // too far
    assert_eq!(nearest_exit_label(&leg, 80.0, 2.0), ""); // nearest has no ref
}

#[test]
fn test_place_stops_attaches_exit_label() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let (leg, target) = leg0_curated_stop(&route);
    let ix = Interchange {
        at_mi: target.at_mi,
        exit_ref: "21".to_string(),
        destinations: strings(&["Lafayette"]),
        via: "US 52 West".to_string(),
        highway: leg.highway.clone(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    let route = replace_leg(
        &route,
        0,
        with_corridor(&leg, |detail| detail.interchanges = vec![ix]),
    );
    let trip = trip_on(route, true);
    let placed = trip
        .stops
        .iter()
        .find(|s| s.name == target.name)
        .expect("the curated stop was placed");
    assert_eq!(placed.exit_label, "exit 21");
}

#[test]
fn test_a_matched_stop_takes_its_own_exit_not_the_nearest() {
    // The bake matched the stop to the interchange three miles on; the exit
    // half a mile away by mile marker is a neighbour, and used to win.
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let (mut leg, target) = leg0_curated_stop(&route);
    let exit = |at_mi: f64, exit_ref: &str| Interchange {
        at_mi,
        exit_ref: exit_ref.to_string(),
        highway: leg.highway.clone(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    let exits = vec![exit(target.at_mi + 0.5, "7"), exit(target.at_mi + 3.0, "9")];
    for stop in leg.stops.iter_mut().filter(|s| s.name == target.name) {
        stop.exit_ref = "9".to_string();
        stop.interchange_mi = Some(target.at_mi + 3.0);
    }
    let matched = with_corridor(&leg, |detail| detail.interchanges = exits.clone());
    let trip = trip_on(replace_leg(&route, 0, matched), true);
    let placed = trip
        .stops
        .iter()
        .find(|s| s.name == target.name)
        .expect("the curated stop was placed");
    assert_eq!(placed.exit_label, "exit 9");
    let served = trip
        .interchange_at(placed.interchange_mi.expect("the match rides along"), 1e-6)
        .expect("the match names a record on the route");
    assert_eq!(served.exit_ref, "9");

    // The leg records no such exit: the stop's own number still speaks.
    let unrecorded = with_corridor(&leg, |detail| detail.interchanges = vec![exits[0].clone()]);
    let trip = trip_on(replace_leg(&route, 0, unrecorded), true);
    let placed = trip
        .stops
        .iter()
        .find(|s| s.name == target.name)
        .expect("the curated stop was placed");
    assert_eq!(placed.exit_label, "exit 9");
    assert_eq!(placed.interchange_mi, None);
}

#[test]
fn test_rest_stop_cue_names_exit_when_linked() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let (leg, target) = leg0_curated_stop(&route);
    let ix = Interchange {
        at_mi: target.at_mi,
        exit_ref: "21".to_string(),
        destinations: strings(&["Lafayette"]),
        via: "US 52 West".to_string(),
        highway: leg.highway.clone(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    let route = replace_leg(
        &route,
        0,
        with_corridor(&leg, |detail| detail.interchanges = vec![ix]),
    );
    let trip = trip_on(route, true);
    let cue = trip
        .navigation_cues
        .iter()
        .find(|c| c.kind == "rest_stop" && c.key.contains(&target.name))
        .expect("a rest-stop cue for the curated stop");
    assert!(cue.text.contains("at exit 21"), "{}", cue.text);
    assert_eq!(cue.near_text, "");
}

#[test]
fn test_rest_stop_cue_generic_without_linked_exit() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let (leg, target) = leg0_curated_stop(&route);
    let route = replace_leg(
        &route,
        0,
        with_corridor(&leg, |detail| detail.interchanges = Vec::new()),
    );
    let trip = trip_on(route, true);
    let cue = trip
        .navigation_cues
        .iter()
        .find(|c| c.kind == "rest_stop" && c.key.contains(&target.name))
        .expect("a rest-stop cue for the curated stop");
    assert!(!cue.text.contains("at exit"), "{}", cue.text); // no fabricated exit number
    assert_eq!(cue.near_text, "");
}

#[test]
fn test_first_leg_has_onramp_cue() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let highway = route.legs[0].highway.clone();
    let trip = trip_on(route, true);
    let onramps: Vec<&NavigationCue> = trip
        .navigation_cues
        .iter()
        .filter(|c| c.kind == "onramp")
        .collect();
    assert_eq!(onramps.len(), 1);
    let text = &onramps[0].near_text;
    assert!(
        text.starts_with(&format!("Merge onto {highway} South toward ")),
        "{text}"
    );
    assert!(
        text.contains("Indianapolis") && text.contains("miles on it."),
        "{text}"
    );
}

#[test]
fn test_onramp_cue_fires_at_drive_start() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let mut trip = Trip::new(
        route,
        rolling_truck(),
        weather("great_lakes", 1),
        TripOptions {
            seed: Some(2),
            world: Some(world()),
            ..Default::default()
        },
    );
    trip.truck.throttle = 0.85;
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..(60 * 120) {
        for ev in drive_frame(&mut trip) {
            if ev.text().contains("Merge onto") {
                seen.push(ev.text().to_string());
            }
        }
        if !seen.is_empty() {
            break;
        }
    }
    assert!(
        seen.first().is_some_and(|m| m.starts_with("Merge onto")),
        "{seen:?}"
    );
}

// --- metric navigation distances --------------------------------------------

#[test]
fn test_metric_navigation_cues_use_kilometers() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let metric = trip_on(route.clone(), false);
    let blob = metric
        .navigation_cues
        .iter()
        .map(|c| format!("{} {}", c.text, c.near_text))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(blob.contains("kilometers"));
    assert!(!blob.contains("mile"), "{blob}");

    // The default (imperial) trip keeps miles, so existing drives are unchanged.
    let imperial = trip_on(route, true);
    let blob_i = imperial
        .navigation_cues
        .iter()
        .map(|c| format!("{} {}", c.text, c.near_text))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(blob_i.contains("miles"));
}

#[test]
fn test_metric_drive_speaks_distances_in_kilometers() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let mut trip = Trip::new(
        route,
        rolling_truck(),
        weather("great_lakes", 1),
        TripOptions {
            seed: Some(2),
            imperial: false,
            world: Some(world()),
            ..Default::default()
        },
    );
    trip.truck.throttle = 0.85;
    let mut nav: Vec<String> = Vec::new();
    for _ in 0..(60 * 60 * 12) {
        for ev in drive_frame(&mut trip) {
            if matches!(ev.kind, TripEventKind::GpsCue | TripEventKind::StopAhead) {
                nav.push(ev.text().to_string());
            }
        }
        if trip.finished {
            break;
        }
    }
    assert!(
        !nav.is_empty(),
        "expected navigation announcements on a long metric drive"
    );
    let blob = nav.join(" ");
    assert!(blob.contains("kilometers"));
    assert!(!blob.contains("mile"), "{blob}");
}

#[test]
fn test_unit_toggle_rerenders_baked_navigation_cues() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let mut trip = trip_on(route, true);

    fn cue_blob(trip: &Trip) -> String {
        trip.navigation_cues
            .iter()
            .map(|c| format!("{} {}", c.text, c.near_text))
            .collect::<Vec<_>>()
            .join(" ")
    }

    assert!(cue_blob(&trip).contains("miles") && !cue_blob(&trip).contains("kilometers"));
    // Switching units mid-trip re-renders the distances already on the route.
    trip.set_imperial(false);
    assert!(cue_blob(&trip).contains("kilometers") && !cue_blob(&trip).contains("mile"));
    trip.set_imperial(true);
    assert!(cue_blob(&trip).contains("miles") && !cue_blob(&trip).contains("kilometers"));
}

#[test]
fn test_metric_zone_warning_uses_metric_speed_limit() {
    let route = first_route_option(world(), "Chicago", "Indianapolis");
    let mut trip = trip_on(route, false);
    trip.zones = vec![Zone::new(5.0, 10.0, 45.0, "construction")];
    trip.announced_zone_warnings.clear();
    trip.position_mi = 4.0; // within the 2-mile warning lookahead of the zone

    let msgs: Vec<String> = trip
        .update(0.0)
        .iter()
        .filter(|ev| ev.kind == TripEventKind::GpsCue)
        .map(|ev| ev.text().to_string())
        .collect();
    let blob = msgs.join(" ");
    // 55/45 mph rendered as km/h
    assert!(blob.contains("Limit 89, then 72."), "{blob}");
    assert!(!blob.contains("Limit 55"), "{blob}");
}

#[test]
fn test_mainline_via_does_not_pollute_the_exit_label() {
    // tools/build_interchanges.py merges every ramp near an interchange node,
    // so this exit's record carried the I-35 North entrance ramp's signage.
    // The cue must not send the driver "for I-35 North" toward their own road.
    let w = world();
    let route = route_from_cities(w, &["Oklahoma City", "Ardmore"]);
    assert!(
        route
            .legs
            .iter()
            .any(|leg| { leg.interchanges().iter().any(|ix| ix.exit_ref == "31B") }),
        "exit 31B missing from the Oklahoma City -> Ardmore route"
    );
    let trip = make_trip(w, "Oklahoma City", "Ardmore", 2);
    let cue = trip
        .navigation_cues
        .iter()
        .find(|c| c.kind == "interchange" && c.text.contains("exit 31B"))
        .expect("an interchange cue for exit 31B");
    assert!(cue.text.contains("toward Waurika"), "{}", cue.text);
    assert!(!cue.text.contains("I-35 North"), "{}", cue.text);
    assert!(!cue.text.contains("Oklahoma City"), "{}", cue.text);
    assert!(!cue.near_text.contains("I-35 North"), "{}", cue.near_text);
}

#[test]
fn a_ramp_past_its_signed_curve_is_held_to_its_design_speed() {
    // Cleveland's exit 170A is signed 15 mph for its bend. Held to the sign
    // gore to bar, the 1,500-foot ramp took a minute and a half (owner,
    // 2026-10-24); the run past the bend is held to the ramp's design speed.
    let route = first_route_option(world(), "Chicago", "Cleveland");
    let trip = trip_on(route, true);
    let total = trip.total_miles();
    let at = (0..400)
        .map(|i| total - 10.0 + f64::from(i) * 0.025)
        .find(|&mi| {
            trip.interchange_at(mi, 0.02)
                .is_some_and(|ix| ix.exit_ref == "170A")
        })
        .expect("exit 170A on the route");
    assert_eq!(trip.ramp_speed_at(at), 15.0);
    let mut stop = ff_core::sim::trip_models::RoadStop::new("Cleveland", at, "destination");
    stop.interchange_mi = Some(at);
    let layout = trip.exit_ramp_layout(&stop);
    assert_eq!(layout.curve_mph, 15.0);
    assert_eq!(layout.tangent_mph, trip.ramp_design_mph_at(at));
    assert!(layout.tangent_mph >= 30.0, "{layout:?}");
}
