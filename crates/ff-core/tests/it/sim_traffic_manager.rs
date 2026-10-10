//! Traffic bubble manager tests (port of `tests/test_traffic_manager.py`).

use std::collections::HashSet;

use crate::sim_support::*;
use ff_core::data::world_models::{Leg, Route};
use ff_core::pyrandom::PyRandom;
use ff_core::sim::traffic_manager::{
    climb_speed_mph, governed_band, BrakingZone, TrafficManager, TrafficVehicle,
    CLIMB_MIN_GRADE_PCT, GOVERNED_BOX_TRUCK_BAND_MPH, GOVERNED_CLASSES, GOVERNED_TRUCK_BAND_MPH,
    MAX_BUBBLE_VEHICLES, MERGE_FREE_START_MI, MERGE_WINDOW_MI, NO_SPAWN_AHEAD_MI,
    NO_SPAWN_BEHIND_MI, SPAWN_CELL_MI,
};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{hourly_volume_fraction, leg_lane_count, DIRECTIONAL_SPLIT};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{effects, WeatherKind};

/// Route miles just past each on-ramp -- where a merge can come from.
fn ramp_miles(manager: &TrafficManager) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for (start, leg) in manager.leg_starts.iter().zip(manager.route.legs.iter()) {
        for interchange in leg.interchanges() {
            out.push(start + interchange.at_mi + 0.1);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

fn manager(seed: i64) -> TrafficManager {
    let route = route_from_cities(world(), &["Chicago", "Indianapolis"]);
    manager_for_route(&route, seed)
}

fn manager_for_route(route: &Route, seed: i64) -> TrafficManager {
    let mut leg_starts = Vec::new();
    let mut at_mi = 0.0;
    for leg in &route.legs {
        leg_starts.push(at_mi);
        at_mi += leg.miles;
    }
    // TruckState() at rest, WeatherSystem("great_lakes", seed=1).
    TrafficManager::new(
        route,
        &leg_starts,
        Some(seed),
        8.0,
        1.0,
        true,
        0.0,
        weather("great_lakes", 1).effects(),
    )
}

fn v(key: &str, pos: f64, speed: f64, rel: i64, intent: &str, class: &str) -> TrafficVehicle {
    TrafficVehicle::new(key, pos, speed, speed, rel, intent, class)
}

#[test]
fn test_traffic_vehicle_keeps_npc_compatibility_properties() {
    let vehicle = TrafficVehicle::new("traffic:test", 12.5, 44.0, 40.0, 1, "merging", "car");
    assert_eq!(vehicle.at_mi(), 12.5);
    assert!(vehicle.end_mi() > vehicle.at_mi());
    assert_eq!(vehicle.lane_text(), "right lane");
    assert_eq!(vehicle.behavior(), "merging_vehicle");
    assert_eq!(vehicle.reason(), "merging traffic");
}

#[test]
fn test_state_trooper_vehicle_has_clear_status_reason() {
    let vehicle = v("trooper:test", 12.5, 62.0, 0, "cruising", "state trooper");
    assert_eq!(vehicle.reason(), "state trooper ahead");
}

#[test]
fn test_traffic_vehicle_maps_new_intents_to_legacy_behavior_and_reason() {
    let expected = [
        ("cruising", "steady_truck", "steady truck traffic"),
        ("following", "slow_car", "slow car ahead"),
        ("merging", "merging_vehicle", "merging traffic"),
        ("braking", "braking_traffic", "brake lights ahead"),
        ("passing", "passing_vehicle", "passing traffic"),
    ];
    for (intent, behavior, reason) in expected {
        let vehicle = v(&format!("traffic:{intent}"), 10.0, 45.0, 0, intent, "car");
        assert_eq!(vehicle.behavior(), behavior);
        assert_eq!(vehicle.reason(), reason);
    }
}

#[test]
fn test_lead_vehicle_selects_nearest_vehicle_in_player_lane() {
    let mut manager = manager(1);
    manager.vehicles = vec![
        v("left", 5.1, 55.0, -1, "passing", "car").with_lane(1),
        v("far", 6.0, 45.0, 0, "following", "semi"),
        v("near", 5.3, 42.0, 0, "braking", "car"),
    ];
    let context = manager.lead_vehicle(5.0, 60.0).expect("a lead");
    assert_eq!(context.lead.key, "near");
    assert_eq!(context.closing_mph, 18.0);
}

#[test]
fn test_lead_vehicle_follows_the_player_into_the_left_lane() {
    let mut manager = manager(1);
    manager.vehicles = vec![
        v("left", 5.1, 55.0, -1, "passing", "car").with_lane(1),
        v("right", 5.3, 42.0, 0, "braking", "car"),
    ];
    manager.player_lane = 1;
    let context = manager.lead_vehicle(5.0, 60.0).expect("a lead");
    assert_eq!(context.lead.key, "left");
}

#[test]
fn test_lead_vehicle_ignores_the_origin_lane_mid_change() {
    // A lane change underway reasons about the lane being entered, not the
    // one being left.
    let mut manager = manager(1);
    manager.vehicles = vec![v("origin", 5.3, 42.0, 0, "braking", "car")];
    manager.player_lane = 0;
    manager.player_lane_target = Some(1); // changing into the left lane
    assert!(manager.lead_vehicle(5.0, 60.0).is_none());
}

#[test]
fn test_lead_vehicle_finds_a_lead_already_in_the_destination_lane() {
    let mut manager = manager(1);
    manager.vehicles = vec![
        v("origin", 5.3, 42.0, 0, "braking", "car"),
        v("dest", 5.4, 40.0, -1, "braking", "car").with_lane(1),
    ];
    manager.player_lane = 0;
    manager.player_lane_target = Some(1);
    let context = manager.lead_vehicle(5.0, 60.0).expect("a lead");
    assert_eq!(context.lead.key, "dest");
}

#[test]
fn test_lead_vehicle_reverts_to_origin_lane_once_the_change_target_clears() {
    let mut manager = manager(1);
    manager.vehicles = vec![v("origin", 5.3, 42.0, 0, "braking", "car")];
    manager.player_lane = 0;
    manager.player_lane_target = Some(1);
    assert!(manager.lead_vehicle(5.0, 60.0).is_none());

    manager.player_lane_target = None; // aborted
    let context = manager.lead_vehicle(5.0, 60.0).expect("a lead");
    assert_eq!(context.lead.key, "origin");
}

#[test]
fn test_lead_vehicle_keeps_overlapping_vehicle_in_player_lane() {
    let mut manager = manager(1);
    manager.vehicles = vec![v("overlap", 4.9, 20.0, 0, "braking", "semi")];
    let context = manager.lead_vehicle(5.0, 10.0).expect("a lead");
    assert_eq!(context.lead.key, "overlap");
    assert_eq!(context.gap_mi, 0.0);
}

#[test]
fn test_update_moves_and_prunes_vehicles_outside_bubble() {
    let mut manager = manager(1);
    manager.vehicles = vec![
        v("behind", -3.0, 55.0, 0, "cruising", "semi"),
        v("ahead", 2.0, 55.0, 0, "cruising", "semi"),
    ];
    manager.update(1.0, 0.0, 20.0, None, None);
    // Only the two seeded here are the subject; the rolling bubble also tops
    // the window up around the truck.
    let seeded: Vec<&TrafficVehicle> = manager
        .vehicles
        .iter()
        .filter(|v| v.key == "behind" || v.key == "ahead")
        .collect();
    assert_eq!(seeded.len(), 1);
    assert_eq!(seeded[0].key, "ahead");
    assert!(seeded[0].position_mi > 2.2);
}

#[test]
fn test_update_keeps_future_route_traffic_until_reached() {
    let route = supported(world(), "Seattle", "New York");
    let mut manager = manager_for_route(&route, 7);
    manager.spawn_initial_traffic();
    let initial_keys: HashSet<String> = manager.vehicles.iter().map(|v| v.key.clone()).collect();

    manager.update(0.0, 0.0, 20.0, None, None);

    let survivors: HashSet<String> = manager.vehicles.iter().map(|v| v.key.clone()).collect();
    assert!(initial_keys.len() > 1);
    assert!(initial_keys.is_subset(&survivors));
    assert!(manager.vehicles.iter().any(|v| v.position_mi > 10.0));
}

#[test]
fn test_roving_posts_add_state_trooper_traffic() {
    let mut manager = manager(1);
    manager.vehicles = vec![v("traffic:existing", 2.0, 55.0, 0, "cruising", "semi")];
    let posts = vec![
        always_observing_post(10.0, "roving_patrol", 1.0, 1.0, 0),
        always_observing_post(22.0, "roving_patrol", 1.0, 1.0, 0),
        // Parked kinds belong to the enforcement cues, not the traffic bubble.
        always_observing_post(30.0, "median_post", 1.0, 1.0, 0),
    ];

    manager.add_enforcement_traffic(&posts);
    manager.add_enforcement_traffic(&posts);

    let troopers: Vec<&TrafficVehicle> = manager
        .vehicles
        .iter()
        .filter(|v| v.vehicle_class == "state trooper")
        .collect();
    assert_eq!(troopers.len(), 2);
    let positions: Vec<f64> = manager.vehicles.iter().map(|v| v.position_mi).collect();
    let mut sorted = positions.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(positions, sorted);
    assert!(troopers.iter().all(|v| v.relative_lane == 0));
}

#[test]
fn test_faster_merging_vehicle_with_a_safe_gap_moves_into_player_lane() {
    let mut manager = manager(1);
    manager.sync_environment(42.0, weather("great_lakes", 1).effects());
    manager.vehicles = vec![v("merge", 0.8, 45.0, 1, "merging", "car").with_lane(-1)];

    manager.update(0.0, 0.0, 20.0, None, None);
    let situation = manager.next_situation(0.0, 55.0).expect("a situation");

    let merging = manager.vehicles.iter().find(|v| v.key == "merge").unwrap();
    assert_eq!(merging.relative_lane, 0);
    assert_eq!(situation.kind, "merging");
    assert!(situation.message.normal.contains("Merging"));
}

#[test]
fn test_slower_merging_vehicle_yields_on_the_ramp_until_the_truck_passes() {
    let mut manager = manager(1);
    manager.sync_environment(55.0, weather("great_lakes", 1).effects());
    manager.vehicles = vec![v("merge", 0.3, 42.0, 1, "merging", "car").with_lane(-1)];

    manager.update(0.0, 0.0, 20.0, None, None);

    let merging = manager.vehicles.iter().find(|v| v.key == "merge").unwrap();
    assert_eq!(merging.lane, -1, "the slower car must remain on the ramp");
    assert!(
        manager.next_situation(0.0, 55.0).is_none(),
        "ramp traffic must not become a lead hazard in the truck's lane"
    );
}

#[test]
fn test_yielding_merging_vehicle_joins_behind_after_the_truck_clears() {
    let mut manager = manager(1);
    manager.sync_environment(55.0, weather("great_lakes", 1).effects());
    manager.vehicles = vec![v("merge", 0.0, 42.0, 1, "merging", "car").with_lane(-1)];

    manager.update(0.0, 0.5, 20.0, None, None);

    let merged = manager.vehicles.iter().find(|v| v.key == "merge").unwrap();
    assert_eq!(merged.lane, 0);
    assert_eq!(merged.intent, "cruising");
    assert!(manager.next_situation(0.5, 55.0).is_none());
}

#[test]
fn test_ramp_traffic_joins_the_right_lane_not_the_players_left_lane() {
    let mut manager = manager(1);
    manager.player_lane = 1;
    manager.sync_environment(55.0, weather("great_lakes", 1).effects());
    manager.vehicles = vec![v("merge", 0.0, 42.0, 1, "merging", "car").with_lane(-1)];

    manager.update(0.0, 0.5, 20.0, None, None);

    let merged = manager.vehicles.iter().find(|v| v.key == "merge").unwrap();
    assert_eq!(merged.lane, 0);
    assert!(manager.next_situation(0.5, 55.0).is_none());
}

#[test]
fn test_yielding_ramp_traffic_does_not_block_a_lane_change_to_the_right() {
    let mut manager = manager(1);
    manager.player_lane = 1;
    manager.player_lane_target = Some(0);
    manager.sync_environment(55.0, weather("great_lakes", 1).effects());
    manager.vehicles = vec![v("merge", 0.3, 42.0, 1, "merging", "car").with_lane(-1)];

    manager.update(0.0, 0.0, 20.0, None, None);

    assert_eq!(manager.vehicles[0].lane, -1);
    assert!(manager.next_situation(0.0, 55.0).is_none());
}

#[test]
fn test_slow_lead_is_called_only_once_it_can_be_heard() {
    let mut manager = manager(1);
    manager.vehicles = vec![v("slow", 1.5, 41.0, 0, "following", "box truck")];
    assert!(
        manager.next_situation(0.0, 45.0).is_none(),
        "a slow lead a mile and a half out is out of earshot, so it is not called yet"
    );
    let situation = manager
        .next_situation(1.1, 45.0)
        .expect("called once it is within half a mile");
    assert_eq!(situation.kind, "following");
    assert!(situation.message.normal.starts_with("Slow box truck"));
}

#[test]
fn test_braking_vehicle_slows_and_creates_lead_situation() {
    let mut manager = manager(1);
    // The congestion it is braking for. The fixture used to name none, which
    // is a car with a wrong label rather than a braking car -- and now that
    // the label ends with its reason, it is a cruising car by the first tick
    // and raises no situation at all. No pace on the zone, so the ratchet to
    // the generic floor is what this test still watches.
    manager.braking_zones = vec![BrakingZone::new(0.0, 2.0, "heavy traffic", None)];
    manager.vehicles = vec![v("brake", 0.7, 45.0, 0, "braking", "car")];

    manager.update(1.0, 0.0, 20.0, None, None);
    let situation = manager.next_situation(0.0, 60.0).expect("a situation");

    let braking = manager.vehicles.iter().find(|v| v.key == "brake").unwrap();
    assert!(braking.target_speed_mph < 45.0);
    assert_eq!(situation.kind, "braking");
    assert!(situation.message.normal.contains("Brake lights"));
}

#[test]
fn test_braking_vehicle_in_a_zone_paces_the_zone_speed() {
    // Inside a handed-over zone, braking traffic settles at the zone's own
    // prevailing speed instead of ratcheting to the generic floor
    // (Brandon, 2026-08-20).
    let mut manager = manager(1);
    manager.rolling_bubble = false;
    // In a real merge window, because the second half below is the
    // merge-window case and has to actually be one. It used to sit at mile
    // 5.5, which is clear of every ramp on this leg: once the zones were
    // taken away the car had no reason of any kind left to be braking, so
    // what the second half really pinned was a label outliving its cause.
    let route = route_from_cities(world(), &["Chicago", "Indianapolis"]);
    let at_mi = route.legs[0].interchanges()[0].at_mi + 0.1;
    assert!(
        manager.merge_plausible_at(at_mi),
        "the fixture has to sit in a merge window"
    );
    manager.braking_zones = vec![BrakingZone::new(
        at_mi - 1.5,
        at_mi + 2.5,
        "heavy traffic",
        Some(45.0),
    )];
    manager.vehicles = vec![v("brake", at_mi, 49.0, 0, "braking", "car")];

    for _ in 0..8 {
        manager.update(1.0, at_mi - 0.5, 0.0, None, None);
    }
    assert_eq!(manager.vehicles[0].target_speed_mph, 45.0);
    // Outside any zone the old floor still governs: the merge-window case.
    // The ramp is still beside it, so it still has a reason to be on the
    // brakes; what it no longer has is a zone pace to settle at.
    manager.braking_zones = Vec::new();
    for _ in 0..8 {
        manager.update(1.0, at_mi - 0.5, 0.0, None, None);
    }
    assert_eq!(manager.vehicles[0].intent, "braking");
    let floor = manager.floor_speed(manager.posted_limit_at(at_mi));
    assert_eq!(manager.vehicles[0].target_speed_mph, floor);
}

#[test]
fn test_next_situation_only_announces_vehicle_once() {
    let mut manager = manager(1);
    manager.vehicles = vec![v("lead", 0.4, 42.0, 0, "following", "semi")];
    let first = manager.next_situation(0.0, 55.0);
    let second = manager.next_situation(0.0, 55.0);
    assert_eq!(first.expect("a situation").kind, "following");
    assert!(second.is_none());
}

#[test]
fn test_next_situation_speaks_speed_units() {
    let mut manager = manager(1);
    manager.vehicles = vec![v("lead", 0.4, 42.0, 0, "following", "semi")];
    let situation = manager.next_situation(0.0, 55.0).expect("a situation");
    assert!(situation.message.normal.contains("42 miles per hour"));
}

#[test]
fn test_manager_copies_leg_starts() {
    let route = route_from_cities(world(), &["Chicago", "Indianapolis"]);
    let mut leg_starts = vec![0.0];
    let manager = TrafficManager::new(
        &route,
        &leg_starts,
        Some(1),
        8.0,
        1.0,
        true,
        0.0,
        weather("great_lakes", 1).effects(),
    );
    leg_starts.push(12.0);
    assert_eq!(manager.leg_starts, vec![0.0]);
}

fn signature(manager: &TrafficManager) -> Vec<(f64, f64, i64, String, String)> {
    manager
        .vehicles
        .iter()
        .map(|v| {
            (
                (v.position_mi * 100.0).round() / 100.0,
                (v.speed_mph * 10.0).round() / 10.0,
                v.relative_lane,
                v.intent.clone(),
                v.vehicle_class.clone(),
            )
        })
        .collect()
}

fn placement_signature(manager: &TrafficManager) -> Vec<(f64, i64, String, String)> {
    manager
        .vehicles
        .iter()
        .map(|v| {
            (
                (v.position_mi * 100.0).round() / 100.0,
                v.relative_lane,
                v.intent.clone(),
                v.vehicle_class.clone(),
            )
        })
        .collect()
}

#[test]
fn test_spawn_is_deterministic_for_same_route_and_seed() {
    let mut first = manager(1);
    let mut second = manager(1);
    first.spawn_initial_traffic();
    second.spawn_initial_traffic();
    assert!(!signature(&first).is_empty());
    assert_eq!(signature(&first), signature(&second));
}

#[test]
fn test_bad_weather_slows_spawned_traffic_without_moving_it() {
    let mut clear = manager(1);
    let mut rain = manager(1);
    rain.weather_effects = effects(WeatherKind::HeavyRain);

    clear.spawn_initial_traffic();
    rain.spawn_initial_traffic();

    assert!(!signature(&clear).is_empty());
    let clear_pos: Vec<f64> = clear.vehicles.iter().map(|v| v.position_mi).collect();
    let rain_pos: Vec<f64> = rain.vehicles.iter().map(|v| v.position_mi).collect();
    assert_eq!(rain_pos, clear_pos);
    let min = |m: &TrafficManager| {
        m.vehicles
            .iter()
            .map(|v| v.speed_mph)
            .fold(f64::MAX, f64::min)
    };
    assert!(min(&rain) < min(&clear));
}

#[test]
fn test_long_route_bad_weather_preserves_spawned_traffic_positions() {
    let route = supported(world(), "Seattle", "New York");
    let mut clear = manager_for_route(&route, 7);
    let mut rain = manager_for_route(&route, 7);
    rain.weather_effects = effects(WeatherKind::HeavyRain);

    clear.spawn_initial_traffic();
    rain.spawn_initial_traffic();

    assert!(!clear.vehicles.is_empty());
    assert_eq!(rain.vehicles.len(), clear.vehicles.len());
    assert_eq!(placement_signature(&rain), placement_signature(&clear));
    let speeds = |m: &TrafficManager| m.vehicles.iter().map(|v| v.speed_mph).collect::<Vec<_>>();
    assert_ne!(speeds(&rain), speeds(&clear));
}

// -- the rolling bubble ------------------------------------------------------

#[test]
fn test_the_bubble_fills_as_the_truck_drives() {
    let mut manager = manager(1);
    manager.update(0.0, 20.0, 1.0, None, None);
    assert!(manager.vehicles.len() >= 4);
}

#[test]
fn test_the_bubble_does_not_drain_over_a_long_run() {
    // Vehicles are retired behind the truck, so something must replace them.
    let mut manager = manager(1);
    manager.update(0.0, 10.0, 1.0, None, None);
    let early = manager.vehicles.len();

    let mut position = 10.0;
    while position < 70.0 {
        position += 0.25;
        manager.update(1.0, position, 1.0, None, None);
    }
    assert!(early >= 1);
    assert!(manager.vehicles.len() >= early);
}

#[test]
fn test_traffic_appears_behind_the_truck_so_it_can_be_overtaken() {
    // The old model placed everything ahead, so nobody could ever pass.
    let mut manager = manager(1);
    let mut behind_seen: Vec<f64> = Vec::new();
    let mut position = 30.0;
    while position < 45.0 {
        position += 0.25;
        manager.update(1.0, position, 1.0, None, None);
        behind_seen.extend(
            manager
                .vehicles
                .iter()
                .filter(|v| v.position_mi < position)
                .map(|v| v.speed_mph),
        );
    }
    assert!(
        !behind_seen.is_empty(),
        "nothing was ever spawned behind the truck"
    );
    let limit = manager.posted_limit_at(40.0);
    assert!(
        behind_seen.iter().any(|mph| *mph > limit),
        "nothing behind is fast enough to pass"
    );
}

#[test]
fn test_nothing_is_created_alongside_the_truck() {
    // A vehicle that materialises next to the cab appeared out of nowhere.
    let mut manager = manager(1);
    manager.update(0.0, 40.0, 1.0, None, None);
    for vehicle in &manager.vehicles {
        let gap = vehicle.position_mi - 40.0;
        assert!(
            !(-NO_SPAWN_BEHIND_MI < gap && gap < NO_SPAWN_AHEAD_MI),
            "{}",
            vehicle.key
        );
    }
}

#[test]
fn test_a_passed_cell_never_spawns_again() {
    // Backing up or slowing must not repopulate road already driven.
    let mut manager = manager(1);
    manager.update(0.0, 50.0, 1.0, None, None);
    let keys: HashSet<String> = manager.vehicles.iter().map(|v| v.key.clone()).collect();

    manager.vehicles = Vec::new();
    manager.update(0.0, 50.0, 1.0, None, None);

    let again: HashSet<String> = manager.vehicles.iter().map(|v| v.key.clone()).collect();
    assert!(again.is_disjoint(&keys));
}

#[test]
fn test_the_bubble_is_deterministic_for_the_same_seed_and_position() {
    let (mut first, mut second) = (manager(4), manager(4));
    first.update(0.0, 25.0, 1.0, None, None);
    second.update(0.0, 25.0, 1.0, None, None);
    assert!(!first.vehicles.is_empty());
    let sig = |m: &TrafficManager| {
        m.vehicles
            .iter()
            .map(|v| {
                (
                    v.key.clone(),
                    (v.position_mi * 1e6).round() / 1e6,
                    (v.speed_mph * 1e6).round() / 1e6,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(sig(&first), sig(&second));
}

#[test]
fn test_density_ignores_the_difficulty_and_compression_knobs() {
    // Presence is not difficulty -- the same rule the police already follow.
    let mut busy = manager(3);
    let mut quiet = manager(3);
    quiet.hazard_scale = 0.11;
    busy.update(0.0, 30.0, 1.0, None, None);
    quiet.update(0.0, 30.0, 1.0, None, None);
    assert!(!busy.vehicles.is_empty());
    assert_eq!(busy.vehicles.len(), quiet.vehicles.len());
}

#[test]
fn test_density_follows_the_clock_not_the_departure_hour() {
    // A run that leaves at 04:00 drives into the morning rush.
    let mut manager = manager(1);
    let leg = manager.route.legs[0].clone();
    manager.hour = 3.0;
    let quiet = manager.leg_density(&leg, true, None);
    manager.hour = 8.0;
    let rush = manager.leg_density(&leg, false, None);
    assert!(rush > quiet);
}

#[test]
fn test_traffic_runs_at_the_speed_of_the_road_it_is_on() {
    // Highway traffic must not crawl because the map got faster (owner
    // playtest, 2026-08-15).
    let route = route_from_cities(world(), &["Dallas", "Houston"]);
    let mut manager = manager_for_route(&route, 4);
    // A dry-road claim.
    manager.weather_effects = effects(WeatherKind::Clear);

    let limit = manager.posted_limit_at(180.0);
    assert!(
        limit >= 70.0,
        "the fixture stretch is meant to be a fast one, got {limit}"
    );

    let mut seen: Vec<(f64, f64)> = Vec::new();
    let mut counted: HashSet<String> = HashSet::new();
    let mut position = 175.0;
    while position < 190.0 {
        position += 0.25;
        manager.update(1.0, position, 1.0, None, None);
        for v in &manager.vehicles {
            if counted.insert(v.key.clone()) {
                seen.push((v.speed_mph, v.position_mi));
            }
        }
    }
    assert!(
        !seen.is_empty(),
        "nothing was ever spawned on the fixture stretch"
    );
    for (mph, at_mi) in &seen {
        let floor = manager.floor_speed(manager.posted_limit_at(*at_mi));
        assert!(
            *mph >= floor - 0.01,
            "{mph:.1} mph at mile {at_mi:.1}, floor {floor:.1}"
        );
    }
    let fast: Vec<f64> = seen
        .iter()
        .filter(|(_, at_mi)| manager.posted_limit_at(*at_mi) >= 70.0)
        .map(|(mph, _)| *mph)
        .collect();
    assert!(!fast.is_empty(), "no vehicle was ever on the fast stretch");
    let max = fast.iter().cloned().fold(f64::MIN, f64::max);
    let min = fast.iter().cloned().fold(f64::MAX, f64::min);
    assert!(max >= limit, "{max}");
    assert!(min >= limit - 25.0, "{min}");
    let near_the_limit = fast.iter().filter(|mph| **mph >= limit - 8.0).count();
    assert!(
        near_the_limit >= fast.len() / 4,
        "{near_the_limit} of {}",
        fast.len()
    );
}

#[test]
fn test_traffic_scales_down_where_the_road_is_slow() {
    // The same draw on a 45 mph posting must not put interstate speeds in a
    // town: relative bands have to cut both ways.
    let route = route_from_cities(world(), &["Chicago", "Indianapolis"]);
    let mut manager = manager_for_route(&route, 4);

    let slow = manager.posted_limit_at(5.0);
    assert!(
        slow <= 55.0,
        "the fixture stretch is meant to be a slow one, got {slow}"
    );

    let mut seen: Vec<f64> = Vec::new();
    let mut position = 3.0;
    while position < 15.0 {
        position += 0.25;
        manager.update(1.0, position, 1.0, None, None);
        // Judge a vehicle by the stretch it is ON, not by the point it happens
        // to occupy. A car drawn legitimately for the 55 zone can drift a
        // tenth of a mile past the boundary, and this test is about the DRAW.
        // Requiring the whole neighbourhood to be slow keeps the question
        // ("were interstate speeds drawn in a town?") and drops the bleed --
        // which a 0.09 mile shift in the boundary is otherwise enough to trip.
        seen.extend(
            manager
                .vehicles
                .iter()
                .filter(|v| {
                    [-0.3, 0.0, 0.3]
                        .into_iter()
                        .map(|offset| manager.posted_limit_at(v.position_mi + offset))
                        .fold(f64::MIN, f64::max)
                        <= slow
                })
                .map(|v| v.speed_mph),
        );
    }
    assert!(
        !seen.is_empty(),
        "nothing was ever spawned on the fixture stretch"
    );
    let max = seen.iter().cloned().fold(f64::MIN, f64::max);
    assert!(max <= slow + 12.0, "{max}");
}

#[test]
fn test_the_opening_miles_of_a_run_spawn_nobody_merging() {
    // Pulling out of a gate must not open with a merge warning (owner
    // report, 2026-08-16).
    let mut opening: Vec<String> = Vec::new();
    let mut later: Vec<String> = Vec::new();
    for seed in 0..40 {
        let mut manager = manager(seed);
        manager.replenish(0.0);
        opening.extend(
            manager
                .vehicles
                .iter()
                .filter(|v| v.position_mi < MERGE_FREE_START_MI)
                .map(|v| v.intent.clone()),
        );
        later.extend(
            manager
                .vehicles
                .iter()
                .filter(|v| v.position_mi >= MERGE_FREE_START_MI)
                .map(|v| v.intent.clone()),
        );
    }
    assert!(
        !opening.is_empty(),
        "the sweep must actually place vehicles in the window"
    );
    assert!(!opening.iter().any(|i| i == "merging"));
    // The intent is withheld at the start, not removed from the game -- but a
    // merge now needs an on-ramp to come from.
    for seed in 0..40 {
        let mut manager = manager(seed);
        let ramps: Vec<f64> = ramp_miles(&manager).into_iter().take(6).collect();
        for ramp_mile in ramps {
            if ramp_mile < MERGE_FREE_START_MI {
                continue;
            }
            manager.spawned_cells.clear();
            manager.vehicles.clear();
            manager.replenish(ramp_mile);
            later.extend(
                manager
                    .vehicles
                    .iter()
                    .filter(|v| v.position_mi >= MERGE_FREE_START_MI)
                    .map(|v| v.intent.clone()),
            );
        }
    }
    assert!(later.iter().any(|i| i == "merging"));
}

#[test]
fn test_the_merge_free_window_only_covers_the_start_of_the_route() {
    // Mid-route the start-of-run rule stops applying -- but a merge still
    // needs a ramp to come from.
    let mut intents: Vec<String> = Vec::new();
    for seed in 0..40 {
        let mut manager = manager(seed);
        let ramps: Vec<f64> = ramp_miles(&manager).into_iter().take(6).collect();
        for ramp_mile in ramps {
            if ramp_mile < MERGE_FREE_START_MI {
                continue;
            }
            manager.spawned_cells.clear();
            manager.vehicles.clear();
            manager.replenish(ramp_mile);
            intents.extend(
                manager
                    .vehicles
                    .iter()
                    .filter(|v| v.position_mi >= MERGE_FREE_START_MI)
                    .map(|v| v.intent.clone()),
            );
        }
    }
    assert!(
        intents.iter().any(|i| i == "merging"),
        "no merge anywhere near an on-ramp"
    );
}

#[test]
fn test_traffic_density_reads_the_road_s_real_volume() {
    // Owner, 2026-08-19: the vehicle count reads the baked HPMS volume under
    // the truck, through the same chain congestion uses.
    let density = |aadt: f64, hour: f64| {
        let mph = 60.0;
        let lam =
            aadt * hourly_volume_fraction(hour, false) * DIRECTIONAL_SPLIT / mph * SPAWN_CELL_MI;
        0.86_f64.min(0.05_f64.max(1.0 - (-lam).exp()))
    };
    // A quiet rural highway empties out overnight and fills at rush.
    assert!(density(2500.0, 3.0) < 0.15);
    assert!(density(2500.0, 17.0) > density(2500.0, 3.0) * 3.0);
    // And a busy road is busier than a quiet one at the same hour.
    assert!(density(45000.0, 12.0) > density(2500.0, 12.0));
}

#[test]
#[ignore = "Python asserted on the module's source text (inspect.getsource); no Rust equivalent"]
fn test_a_leg_with_no_baked_volume_drives_exactly_as_before() {}

#[test]
fn test_merging_only_happens_where_a_ramp_feeds_in() {
    // Owner, 2026-08-19: "why do we have to clear every single car?"
    // Merging is positional: it happens at interchanges.
    let w = world();
    let leg = w
        .legs
        .iter()
        .find(|leg| {
            leg.highway.starts_with("I-") && leg.miles > 100.0 && !leg.interchanges().is_empty()
        })
        .expect("an interstate leg with interchanges");
    let ramps: Vec<f64> = leg.interchanges().iter().map(|i| i.at_mi).collect();
    assert!(!ramps.is_empty());

    let route = Route::new(vec![leg.a.clone(), leg.b.clone()], vec![leg.clone()]);
    let manager = TrafficManager::bare(&route, &[0.0]);

    // Right after a ramp: a merge is plausible.
    assert!(manager.merge_plausible_at(ramps[0] + 0.1));
    // Well clear of every ramp: it is not.
    let candidate = ramps[0] + MERGE_WINDOW_MI + 0.5;
    let clear = ramps
        .iter()
        .all(|r| !(0.0 <= candidate - r && candidate - r <= MERGE_WINDOW_MI));
    if clear {
        assert!(!manager.merge_plausible_at(candidate));
    }
}

#[test]
fn test_hard_braking_follows_the_congestion_not_the_dice() {
    let mut manager = TrafficManager::bare(&Route::default(), &[]);
    manager.braking_zones = vec![BrakingZone::span(10.0, 14.0)];
    assert!(manager.braking_plausible_at(12.0));
    assert!(!manager.braking_plausible_at(40.0));
}

#[test]
fn test_merging_and_braking_are_transient_not_careers() {
    // A merger runs ramp speed only until the lane change is done; a braking
    // car brakes for a moment (Brandon, 2026-08-20).
    let mut manager = manager(1);
    let at_mi = 8.0;
    let limit = manager.posted_limit_at(at_mi);
    let spawn_mph = limit - 15.0; // the merging draw's midpoint: a real deficit
                                  // The jam the braking car is braking FOR. The fixture used to name no
                                  // reason at all: a vehicle labelled braking on a stretch with no
                                  // congestion and no ramp under it, which is not a braking car, it is a
                                  // car with a wrong label. The label is read off the road now, so the road
                                  // has to carry the thing. It ends at 8.2 so the car drives clear of it
                                  // and has to prove it comes back up.
    manager.braking_zones = vec![BrakingZone::new(
        7.0,
        8.2,
        "rush hour congestion",
        Some(30.0),
    )];
    // Both carry a speed draw, as every vehicle the spawner makes does. A
    // vehicle placed with no draw is documented as one the road is not
    // allowed to move (an injected jam, a marked unit), so asking it to
    // recover to road speed was asking for the one thing it is built not to
    // do -- and it "passed" only while a target drawn once was kept for life.
    let drawn = |key: &str, intent: &str| {
        v(key, at_mi, spawn_mph, 0, intent, "semi").with_speed_draw(0.0, None, 0.0)
    };
    manager.vehicles = vec![
        drawn("traffic:0:99:merging", "merging"),
        drawn("traffic:0:99:braking", "braking"),
    ];
    for _ in 0..120 {
        manager.update(1.0, at_mi - 1.5, 1.0, None, None);
    }
    for key in ["traffic:0:99:merging", "traffic:0:99:braking"] {
        let vehicle = manager
            .vehicles
            .iter()
            .find(|v| v.key == key)
            .expect("the slowpoke is still in the bubble");
        // Recovered means back up to the speed THIS road asks of THIS
        // vehicle where it now is -- read fresh off the posting under it,
        // not the number it was carrying two miles ago. The road climbs from
        // 45 to 55 across the stretch it covers in two minutes, so a vehicle
        // frozen at the cruise it recovered to back at the merge fails this.
        let road = manager
            .vehicle_road_speed_mph(vehicle)
            .expect("a drawn vehicle reads the road");
        assert!(
            road > spawn_mph + 5.0,
            "the fixture has to leave a deficit to recover from: road {road:.0}, spawned {spawn_mph:.0}"
        );
        assert_eq!(
            vehicle.intent, "cruising",
            "{key} is still labelled {}",
            vehicle.intent
        );
        assert!(
            vehicle.speed_mph >= road - 3.0,
            "{} vehicle never recovered: {:.0} mph against a road speed of {road:.0}",
            vehicle.intent,
            vehicle.speed_mph
        );
    }
}

#[test]
fn test_a_semi_out_there_is_governed_like_a_real_one() {
    let mut rng = PyRandom::new_from_i64(11);
    let top = GOVERNED_TRUCK_BAND_MPH.1;
    for limit in [65.0, 70.0, 75.0, 80.0] {
        for intent in ["cruising", "passing", "following"] {
            for vehicle_class in GOVERNED_CLASSES {
                for _ in 0..60 {
                    let speed =
                        TrafficManager::intent_speed(intent, limit, &mut rng, vehicle_class);
                    assert!(speed <= top, "{vehicle_class} {intent} {limit} {speed}");
                }
            }
        }
    }
    let fast: Vec<f64> = (0..60)
        .map(|_| TrafficManager::intent_speed("passing", 75.0, &mut rng, "car"))
        .collect();
    assert!(fast.iter().cloned().fold(f64::MIN, f64::max) > top);
    let governed: Vec<f64> = (0..200)
        .map(|_| TrafficManager::intent_speed("cruising", 80.0, &mut rng, "semi"))
        .collect();
    let lo = governed.iter().cloned().fold(f64::MAX, f64::min);
    let hi = governed.iter().cloned().fold(f64::MIN, f64::max);
    assert!(lo >= GOVERNED_TRUCK_BAND_MPH.0);
    assert!(hi - lo > 2.0);
}

#[test]
fn test_a_box_truck_is_not_governed_like_a_tractor_trailer() {
    // Brandon, 2026-08-22: "I'm in the left lane and have been for quite a
    // while and this box truck is still in the right lane and has not
    // cleared."
    //
    // Box trucks were drawn from the semi band, which is a provenance fault
    // as much as a gameplay one: ATRI surveys for-hire fleets running class 8
    // tractors, and a straight truck is not one. On a 65 road that put the box
    // truck within about a mile an hour of a player running the limit, so the
    // overtake never finished -- the driver moved left and simply stayed level
    // with it.
    assert_eq!(
        governed_band("box truck"),
        Some(GOVERNED_BOX_TRUCK_BAND_MPH)
    );
    assert_eq!(governed_band("semi"), Some(GOVERNED_TRUCK_BAND_MPH));
    // The whole point: a box truck's limiter sits below a tractor's.
    assert!(GOVERNED_BOX_TRUCK_BAND_MPH.1 < GOVERNED_TRUCK_BAND_MPH.1);

    let mut rng = PyRandom::new_from_i64(23);

    // On the road the report came from, a player holding the posted limit
    // gets past a typical box truck instead of pacing it.
    let limit = 65.0;
    let speeds: Vec<f64> = (0..400)
        .map(|_| TrafficManager::intent_speed("cruising", limit, &mut rng, "box truck"))
        .collect();
    let mean = speeds.iter().sum::<f64>() / speeds.len() as f64;
    assert!(mean < limit - 3.0, "mean {mean}");
    let passable =
        speeds.iter().filter(|s| **s <= limit - 3.0).count() as f64 / speeds.len() as f64;
    assert!(
        passable > 0.5,
        "only {:.0}% of box trucks are passable at the limit",
        passable * 100.0
    );

    // Still a band, not one number, and still a governor: never above its top.
    let hi = speeds.iter().cloned().fold(f64::MIN, f64::max);
    let lo = speeds.iter().cloned().fold(f64::MAX, f64::min);
    assert!(hi <= GOVERNED_BOX_TRUCK_BAND_MPH.1);
    assert!(hi - lo > 2.0);

    // And the semi is untouched -- its band is sourced separately and this
    // change is not an excuse to move it.
    let semis: Vec<f64> = (0..200)
        .map(|_| TrafficManager::intent_speed("cruising", 80.0, &mut rng, "semi"))
        .collect();
    assert!(semis.iter().cloned().fold(f64::MAX, f64::min) >= GOVERNED_TRUCK_BAND_MPH.0);
    assert!(semis.iter().cloned().fold(f64::MIN, f64::max) <= GOVERNED_TRUCK_BAND_MPH.1);
}

#[test]
fn test_heavy_trucks_lose_the_hill_and_string_out_on_it() {
    // What actually lets a driver past a truck is the terrain.
    //
    // A limiter is a ceiling, so on the flat every governed truck sits on its
    // ceiling and nothing overtakes anything -- the elephant race, and what
    // Brandon met behind a box truck. On a climb the ceiling stops deciding: a
    // loaded tractor has a fixed amount of power to spend lifting 80,000
    // pounds, so it falls to whatever that buys and the lighter trucks climb
    // past it. Modelled from the same physics as the player's own truck rather
    // than a fitted table, and checked against what a driver would expect to
    // see on a mountain grade.

    // Flat and downhill: the hill has nothing to say, the limiter rules.
    assert_eq!(climb_speed_mph("semi", 0.0), f64::INFINITY);
    assert_eq!(climb_speed_mph("semi", -6.0), f64::INFINITY);
    assert_eq!(
        climb_speed_mph("semi", CLIMB_MIN_GRADE_PCT - 0.1),
        f64::INFINITY
    );
    // A car is not modelled here at all; it climbs as it likes.
    assert_eq!(climb_speed_mph("car", 6.0), f64::INFINITY);

    // The number a driver would recognise: a loaded semi on a sustained six
    // percent crawls in the twenties-to-thirties, not at highway speed.
    let six = climb_speed_mph("semi", 6.0);
    assert!(20.0 < six && six < 35.0, "{six}");

    // Steeper is always slower, and never negative or absurd.
    let by_grade: Vec<f64> = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
        .into_iter()
        .map(|g| climb_speed_mph("semi", g))
        .collect();
    let mut descending = by_grade.clone();
    descending.sort_by(|a, b| b.partial_cmp(a).unwrap());
    assert_eq!(by_grade, descending);
    assert!(by_grade.iter().all(|speed| 5.0 < *speed && *speed < 80.0));

    // And weight is what separates them: the light straight truck climbs the
    // same hill faster than the loaded tractor, which is the overtake.
    for grade in [2.0, 4.0, 6.0] {
        assert!(climb_speed_mph("box truck", grade) > climb_speed_mph("semi", grade) + 5.0);
    }
}

#[test]
fn test_npc_trucks_actually_slow_down_on_a_real_climb() {
    // The model reaches the road: a mountain leg really does hold trucks
    // below their limiter, and a flat one really does not.
    let Some(route) = world()
        .supported_route("Denver", "Grand Junction", None)
        .expect("route lookup")
    else {
        // Python skips here; route data is pinned elsewhere.
        return;
    };
    let trip = Trip::new(
        route,
        TruckState::default(),
        weather("rockies", 3),
        TripOptions {
            seed: Some(3),
            world: Some(world()),
            ..Default::default()
        },
    );
    let manager = &trip.traffic_manager;

    let grades: Vec<f64> = (0..trip.total_miles() as i64)
        .step_by(3)
        .map(|mile| manager.grade_pct_at(mile as f64))
        .collect();
    let climbing: Vec<f64> = grades.iter().copied().filter(|g| *g >= 1.0).collect();
    assert!(
        !climbing.is_empty(),
        "I-70 west of Denver has to climb somewhere"
    );
    // Signed: the same road read the other way descends.
    assert!(grades.iter().any(|g| *g <= -1.0));

    let steepest = climbing.iter().cloned().fold(f64::MIN, f64::max);
    assert!(
        climb_speed_mph("semi", steepest) < GOVERNED_TRUCK_BAND_MPH.0,
        "a truck on the steepest part of this route must be held below its limiter"
    );
}

#[test]
fn test_only_a_freeway_has_distant_traffic_to_hear() {
    // The cab's traffic bed reads this: an interstate carries its real
    // presence, an undivided two-lane road and a city street carry none.
    let w = world();
    let presence_on = |leg: &std::sync::Arc<Leg>| {
        let route = Route::new(vec![leg.a.clone(), leg.b.clone()], vec![leg.clone()]);
        let manager = TrafficManager::bare(&route, &[0.0]);
        manager.freeway_presence_at(leg.miles / 2.0)
    };
    let interstate = w
        .legs
        .iter()
        .find(|leg| leg.highway.starts_with("I-") && leg.miles > 50.0 && leg.local_cue.is_empty())
        .expect("an interstate leg");
    let busy = presence_on(interstate);
    assert!(busy > 0.0 && busy <= 1.0, "{busy}");
    let two_lane = w
        .legs
        .iter()
        .find(|leg| {
            !leg.highway.starts_with("I-") && leg.divided == Some(false) && leg.local_cue.is_empty()
        })
        .expect("an undivided highway leg");
    assert_eq!(presence_on(two_lane), 0.0);
    // The same interstate, but as the city street a facility chain bakes.
    let mut street = (**interstate).clone();
    street.local_cue = "Turn right onto Main Street".to_string();
    assert_eq!(presence_on(&std::sync::Arc::new(street)), 0.0);
}

#[test]
fn test_a_busy_freeway_carries_company_in_the_lanes_beside_the_truck() {
    // Every cell is drawn three miles ahead, so a truck at road speed only
    // ever met traffic slower than it, and the cab heard nobody (owner
    // drive, 2026-10-09). New freeway cells now send passers up the lanes
    // beside the truck from just out of earshot behind it; the truck's own
    // right lane never gets one.
    let w = world();
    let bubble_on = |leg: &std::sync::Arc<Leg>| {
        let route = Route::new(vec![leg.a.clone(), leg.b.clone()], vec![leg.clone()]);
        let mut manager = TrafficManager::bare(&route, &[0.0]);
        manager.rolling_bubble = true;
        manager.replenish(leg.miles / 2.0);
        manager.vehicles
    };
    let interstate = w
        .legs
        .iter()
        .find(|leg| {
            leg.highway.starts_with("I-")
                && leg.miles > 50.0
                && leg.local_cue.is_empty()
                && leg_lane_count(Some(leg)) >= 2
        })
        .expect("a multi-lane interstate leg");
    let middle = interstate.miles / 2.0;
    let vehicles = bubble_on(interstate);
    let beside: Vec<_> = vehicles
        .iter()
        .filter(|v| v.key.matches(':').count() == 2)
        .collect();
    assert!(!beside.is_empty(), "{vehicles:?}");
    assert!(beside.iter().all(|v| v.lane >= 1), "{beside:?}");
    assert!(
        beside.iter().all(|v| v.intent == "passing"
            && (NO_SPAWN_BEHIND_MI..=NO_SPAWN_BEHIND_MI + 0.3).contains(&(middle - v.position_mi))),
        "{beside:?}"
    );
    assert!(vehicles.len() <= MAX_BUBBLE_VEHICLES);

    let two_lane = w
        .legs
        .iter()
        .find(|leg| {
            !leg.highway.starts_with("I-")
                && leg.divided == Some(false)
                && leg.local_cue.is_empty()
                && leg.miles > 20.0
        })
        .expect("an undivided highway leg");
    assert!(bubble_on(two_lane)
        .iter()
        .all(|v| v.key.matches(':').count() == 1));
}
