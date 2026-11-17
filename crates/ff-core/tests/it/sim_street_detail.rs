//! A facility chain with the street detail: each street posts its own limit,
//! the yard starts at the driveway, and the deadline plans the same numbers
//! the drive posts.

use ff_core::data::world::get_world;
use ff_core::data::world_models::{Leg, Route, StreetControl, StreetLimit};
use ff_core::models::jobs::route_drive_hours;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{Zone, YARD_LIMIT_MPH};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherSystem;

fn limit(mph: f64, source: &str) -> Option<StreetLimit> {
    Some(StreetLimit {
        mph,
        source: source.to_string(),
        ..Default::default()
    })
}

/// Three streets -- 30 statutory, 30 read, 45 read -- and the yard.
fn chain_legs(city: &str) -> Vec<Leg> {
    vec![
        Leg::local(city, 0.5, "Main Street", "Start on Main Street.", 30.0)
            .with_street(limit(30.0, "statutory"), Vec::new()),
        Leg::local(city, 0.4, "Oak Street", "Continue onto Oak Street.", 30.0)
            .with_street(limit(30.0, "read"), Vec::new()),
        Leg::local(
            city,
            1.5,
            "Industrial Parkway",
            "Turn left onto Industrial Parkway.",
            45.0,
        )
        .with_turn_deg(90.0)
        .with_street(
            limit(45.0, "read"),
            vec![StreetControl {
                at_mi: 0.0,
                kind: "signal".to_string(),
            }],
        ),
        Leg::local(
            city,
            0.1,
            "a service road",
            "Turn right onto a service road.",
            15.0,
        )
        .with_turn_deg(90.0)
        .with_street(limit(15.0, "assumed"), Vec::new())
        .with_yard(true),
    ]
}

fn trip_on(legs: Vec<Leg>, outbound: bool) -> Trip {
    let city = legs[0].a.clone();
    let route = Route::from_legs(vec![city; legs.len() + 1], legs);
    Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("", Some(3), None, None, false),
        TripOptions {
            seed: Some(3),
            outbound,
            ..Default::default()
        },
    )
}

fn zones(trip: &Trip) -> Vec<(String, f64, f64, f64)> {
    trip.zones
        .iter()
        .map(|z: &Zone| (z.reason.clone(), z.start_mi, z.end_mi, z.limit_mph))
        .collect()
}

#[test]
fn test_each_street_posts_its_own_limit_joined_where_neighbours_agree() {
    let trip = trip_on(chain_legs("abilene_tx_us"), false);
    let got = zones(&trip);
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got[0].0, "facility access road");
    assert!(
        (got[0].1, got[0].3) == (0.0, 30.0) && (got[0].2 - 0.9).abs() < 1e-9,
        "{got:?}"
    );
    assert_eq!(got[1].0, "facility access road");
    assert_eq!(got[1].3, 45.0);
    assert_eq!(got[2].0, "yard");
    assert_eq!(got[2].3, YARD_LIMIT_MPH);
    assert!((got[2].1 - 2.4).abs() < 1e-9 && (got[2].2 - 2.5).abs() < 1e-9);
    assert_eq!(trip.driveway_mi(), Some(2.4));
    // No 15 anywhere on the public street.
    assert!(!trip.zones.iter().any(|z| z.reason == "facility gate"));
    // The provenance stays in the data, never in a line.
    assert_eq!(
        trip.street_limit_at(0.2).map(|l| l.source.as_str()),
        Some("statutory")
    );
    assert_eq!(
        trip.street_controls_between(0.0, 3.0),
        vec![(0.9, "signal")]
    );
}

#[test]
fn test_street_zone_lines_say_the_change_and_the_yard() {
    let trip = trip_on(chain_legs("abilene_tx_us"), false);
    assert_eq!(
        trip.zone_entry_message(&trip.zones[0]),
        // A street is not a zone: the line onto the streets named it.
        "Speed limit 30."
    );
    assert_eq!(
        trip.zone_entry_message(&trip.zones[1]),
        "Speed limit raised to 45."
    );
    assert_eq!(
        trip.zone_entry_message(&trip.zones[2]),
        "Into the yard. Yard limit 15."
    );
}

#[test]
fn test_a_chain_that_ends_on_the_street_posts_no_gate_zone() {
    let mut legs = chain_legs("abilene_tx_us");
    legs.pop();
    let trip = trip_on(legs, false);
    assert!(trip.gate_zone().is_none(), "{:?}", zones(&trip));
    assert_eq!(trip.driveway_mi(), None);
}

#[test]
fn test_outbound_the_yard_comes_first_and_has_no_driveway_to_find() {
    let world = get_world();
    let Some((city, name)) = world.cities.iter().find_map(|(key, city)| {
        city.locations.iter().find_map(|location| {
            let route = world.facility_approach_route(key, &location.name).ok()?;
            (route.legs.len() >= 2 && route.legs.last()?.local_yard)
                .then(|| (key.clone(), location.name.clone()))
        })
    }) else {
        panic!("no facility chain with a driveway in the shipped data");
    };
    let inbound = world
        .facility_approach_route(&city, &name)
        .expect("inbound");
    let outbound = world
        .facility_departure_route(&city, &name)
        .expect("the world answers")
        .expect("a chain facility departs on its chain");
    assert!(outbound.legs[0].local_yard);
    // The same streets carry the same limits the other way; their controls
    // face the inbound truck and stay behind.
    let last = inbound.legs.len() - 1;
    assert_eq!(outbound.legs[last].local_limit, inbound.legs[0].local_limit);
    assert!(outbound
        .legs
        .iter()
        .all(|leg| leg.local_controls.is_empty()));
    let trip = Trip::new(
        outbound,
        TruckState::default(),
        WeatherSystem::new("", Some(3), None, None, false),
        TripOptions {
            seed: Some(3),
            outbound: true,
            ..Default::default()
        },
    );
    assert_eq!(trip.zones[0].reason, "yard");
    assert_eq!(trip.zones[0].start_mi, 0.0);
    assert_eq!(trip.driveway_mi(), None);
}

#[test]
fn test_the_deadline_plans_each_street_at_its_own_limit() {
    // The same streets planned at the flat 25 an access road used to get are
    // slower than planned at their own numbers.
    let city = "abilene_tx_us";
    let detailed = Route::from_legs(vec![city.to_string(); 5], chain_legs(city));
    let flat_legs: Vec<Leg> = chain_legs(city)
        .into_iter()
        .map(|leg| {
            Leg::local(
                city,
                leg.miles,
                &leg.highway,
                &leg.local_cue,
                leg.local_speed_mph,
            )
            .with_turn_deg(leg.local_turn_deg)
        })
        .collect();
    let flat = Route::from_legs(vec![city.to_string(); 5], flat_legs);
    let detailed_h = route_drive_hours(Some(&detailed), 0.0, None);
    let flat_h = route_drive_hours(Some(&flat), 0.0, None);
    assert!(detailed_h < flat_h, "{detailed_h} vs {flat_h}");
    // 0.9 mi at 30, 1.5 at 45, 0.1 at the yard's 15, before the planning
    // factor: within a hair of that sum over the flat plan's 25-and-15.
    let own = 0.9 / 30.0 + 1.5 / 45.0 + 0.1 / 15.0;
    let ratio = detailed_h / own;
    assert!(
        (0.9..1.4).contains(&ratio),
        "{detailed_h} h against {own} h at the posted limits"
    );
}

#[test]
fn test_the_bake_marks_the_yard_from_the_chains_driveway() {
    let world = get_world();
    let approach = world
        .facility_source_approach("abilene_tx_us", "Abilene Company Yard")
        .expect("the world answers")
        .expect("Abilene Company Yard has a chain");
    let driveway = approach.driveway.clone().expect("it has a driveway");
    let route = world
        .facility_approach_route("abilene_tx_us", "Abilene Company Yard")
        .expect("its chain");
    let mut start = 0.0;
    for leg in route.legs.iter() {
        assert_eq!(
            leg.local_yard,
            start >= driveway.at_mi - 0.015,
            "{} at {start}",
            leg.highway
        );
        start += leg.miles;
    }
}

#[test]
fn test_a_street_that_only_changes_name_is_called_once_at_the_join() {
    // Cleveland agent drive, 2026-10-25: "In a quarter mile, continue onto
    // Columbus Road", then "Continue onto Columbus Road, then 1 mile on it"
    // a few seconds later. A turn keeps its lead.
    let mut trip = trip_on(chain_legs("abilene_tx_us"), false);
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = Vec::new();
    let mut spoken: Vec<String> = Vec::new();
    let mut mi = 0.0;
    while mi < 1.0 {
        trip.position_mi = mi;
        for event in trip.update(0.0) {
            spoken.push(event.text().to_string());
        }
        mi += 0.02;
    }
    let oak: Vec<&String> = spoken.iter().filter(|t| t.contains("Oak Street")).collect();
    assert_eq!(oak.len(), 1, "{spoken:?}");
    assert!(oak[0].starts_with("Continue onto Oak Street"), "{spoken:?}");
    assert!(
        spoken
            .iter()
            .any(|t| t.starts_with("In ") && t.contains("turn left onto Industrial Parkway")),
        "{spoken:?}"
    );
}
