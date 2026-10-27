//! `states/driving_facility_gate.rs`, `states/driving_pickup.rs`,
//! `states/driving_liquid.rs` and `states/driving_stops.rs`.
//!
//! Ported from `tests/test_facility_overshoot.py`, the drive-cue half of
//! `tests/test_tanker_surge.py`, and the pickup-gate half of
//! `tests/test_pickup_loading.py`. Cases that need a mixin this task did not
//! port keep their bodies behind `#[ignore]`.

use std::cell::RefCell;
use std::rc::Rc;

use ff_core::data::world::{get_world, World};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::surge::LiquidLoad;
use ff_core::sim::trip_models::FACILITY_GATE_LIMIT_MPH;
use ff_core::sim::vehicle::TruckState;

use freight_fate::app::testing::{FakeClock, TestApp};
use freight_fate::audio::{Audio, AudioError, SustainLoopSpec, VolumeUpdate, CH_SURGE};
use freight_fate::states::base::TimedMessageState;
use freight_fate::states::city_pickup::PickupFacilityState;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_facility_gate::GATE_MISS_LOOP_MIN;
use freight_fate::states::driving_menu_states::FacilityArrivalState;
use freight_fate::states::driving_stops::{
    assist_full_decel_mps2, assist_servo_brake, bar_solid_zone_mi, bar_tick_range_mi,
};

use crate::states_driving_approach_sweep::{arrive, arrive_over, destinations, TRUCK_LENGTH_FT};

/// How many facilities the single-chain cases below drive. Not one: the first
/// chain in the world is among the gentlest, and a case that only ever drives
/// it is measuring that chain rather than the assist.
const FEW: usize = 5;

// -- rigging -------------------------------------------------------------------------

/// `_driving(app)`: a Buffalo to Rochester delivery.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Gates", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

/// The same run as [`a_drive`], still deadheading to the shipper.
fn a_pickup_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Gates", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_PICKUP,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

/// `(city, location_name)` of a facility with a tier-1 street chain, or None
/// when the shipped data has none.
fn a_turn_level_facility(world: &'static World) -> Option<(String, String)> {
    let mut keys: Vec<&String> = world.cities.keys().collect();
    keys.sort();
    for city in keys {
        let Some(entry) = world.cities.get(city) else {
            continue;
        };
        for location in &entry.locations {
            let Ok(route) = world.facility_approach_route(city, &location.name) else {
                continue;
            };
            if route.legs.len() >= 2 && route.legs.iter().any(|leg| leg.local_speed_mph > 0.0) {
                return Some((city.clone(), location.name.clone()));
            }
        }
    }
    None
}

/// A facility whose street chain turns a corner inside its first tenth of a
/// mile, or None when the shipped data has none.
fn a_facility_with_an_early_corner(world: &'static World) -> Option<(String, String)> {
    let mut keys: Vec<&String> = world.cities.keys().collect();
    keys.sort();
    for city in keys {
        let Some(entry) = world.cities.get(city) else {
            continue;
        };
        for location in &entry.locations {
            let Ok(route) = world.facility_approach_route(city, &location.name) else {
                continue;
            };
            if route.legs.len() > 1
                && route.legs[0].miles <= 0.1
                && route.legs.iter().any(|leg| leg.local_speed_mph > 0.0)
            {
                return Some((city.clone(), location.name.clone()));
            }
        }
    }
    None
}

#[test]
fn test_the_route_readout_at_a_ramp_with_streets_after_it_counts_to_the_gate() {
    // Agent drive, exit 286A into Abilene, 2026-09-23: stopped at the ramp's
    // stop sign, R said "50 feet to the Abilene metro freight market" with
    // five miles of streets still to drive to the gate.
    let world = get_world();
    let Some((city, location)) = a_facility_with_an_early_corner(world) else {
        return; // no baked chain facility to stand in for Abilene
    };
    let streets = world
        .facility_approach_route(&city, &location)
        .expect("a chain route")
        .miles();
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.job.destination = city;
    d.job.destination_location = location;
    d.destination_chain_ahead = None;
    d.destination_exit_taken = true;
    let mut stop = ff_core::sim::trip_models::RoadStop::new(
        "destination",
        d.trip.position_mi,
        "delivery_destination",
    );
    stop.interchange_mi = None;
    d.ramp_stop = Some(stop);
    d.ramp_mi = Some(0.01);
    app.clear_speech();

    d.speak_route_status(&mut app.ctx);

    let line = app.main_lines().pop().expect("R said nothing");
    assert!(line.contains("to the gate at"), "{line}");
    assert!(
        !line.contains("feet") || streets < 0.1,
        "the ramp's end read as the destination: {line}"
    );
}

/// `_at_gate`: put the truck right at the finished route end, rolling at
/// `mph`.
fn at_gate(d: &mut DrivingState, mph: f64, warned: bool) {
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles();
    d.trip.finished = true;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = mph / 2.23694;
    d.gate_speed_warned = warned;
    d.gate_grace_s = 0.0;
}

/// Every event line so far that carries `needle`, newest last.
///
/// Python read `spoken[-1]` off a stubbed `say_event`. Here the real pacer is
/// in the path: an interrupting line purges the channel and hands the ROUTE
/// line it cut back to be requeued, so the rescued line legitimately lands
/// after the one that cut it. What each assertion is about is the line
/// itself, not its position behind a rescue.
fn lines_with(app: &TestApp, needle: &str) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| line.contains(needle))
        .collect()
}

fn last_with(app: &TestApp, needle: &str) -> String {
    lines_with(app, needle)
        .pop()
        .unwrap_or_else(|| panic!("no event line carried {needle:?}: {:?}", app.event_lines()))
}

// -- the facility gate (tests/test_facility_overshoot.py) -----------------------------

#[test]
fn test_fast_crossing_misses_the_gate_and_loops_back() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    let minutes = d.trip.game_minutes;
    at_gate(&mut d, 70.0, true);
    d.handle_arrival_gate(&mut app.ctx);
    assert!(!d.trip.finished);
    assert!(d.trip.position_mi < d.trip.total_miles());
    assert_eq!(d.gate_miss_count, 1);
    assert_eq!(d.trip.game_minutes, minutes + GATE_MISS_LOOP_MIN);
    let text = last_with(&app, "You turned around");
    assert!(app
        .event_calls()
        .iter()
        .any(|(line, interrupt)| *line == text && *interrupt));
    assert!(text.to_lowercase().contains("slow to"));
}

#[test]
fn test_the_speed_keeper_hands_off_at_the_gate_instead_of_missing_it() {
    // The keeper held the gate zone's own 15 to the gate and the miss rule
    // read its 15.4 as "too fast": four loop-backs on one delivery (agent
    // playtest, 2026-09-02). An assist holding the number it was given is
    // handed off at the gate with a fresh window, never looped.
    let mut app = TestApp::new();
    app.ctx.settings.speed_keeper = true;
    let mut d = a_drive(&mut app);
    app.clear_speech();
    at_gate(&mut d, FACILITY_GATE_LIMIT_MPH + 0.4, true);
    d.speed_control_armed = true;
    d.keeper_mph = Some(FACILITY_GATE_LIMIT_MPH);
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.trip.finished, "arrived, not looped");
    assert_eq!(d.gate_miss_count, 0);
    assert!(d.keeper_mph.is_none(), "the gate hands the pedals back");
    assert!(d.gate_grace_s > 0.0, "with a reaction window of its own");
    let line = last_with(&app, "Destination ahead");
    assert!(line.contains("Speed keeper off"), "{line}");
    // Inside that window the next frame is still an arrival.
    d.handle_arrival_gate(&mut app.ctx);
    assert_eq!(d.gate_miss_count, 0);
}

#[test]
fn test_slowing_on_the_warning_earns_a_fresh_window_at_the_gate() {
    // Braked to a stop short of the gate on the pre-gate warning, as told;
    // the window ran out while parked; the last fifty feet rolled in a few
    // over -- and that was an instant loop-back (agent playtest,
    // 2026-09-02). Obeying the warning retires it; a gate reached over the
    // number afterwards gets the gate's own line and window.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles() - 0.3;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 40.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    let grace = d.gate_grace_s;
    assert!(grace > 0.0);
    // Stopped short, window spent.
    d.trip.truck.velocity_mps = 0.0;
    d.check_gate_approach_warning(&mut app.ctx, grace + 1.0);
    assert!(!d.gate_speed_warned, "the obeyed warning is retired");
    // Rolls the last stretch a few over.
    at_gate(&mut d, FACILITY_GATE_LIMIT_MPH + 5.0, false);
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.trip.finished, "first contact is never a miss");
    assert_eq!(d.gate_miss_count, 0);
    assert!(d.gate_grace_s > 0.0);
    assert!(!lines_with(&app, "Destination ahead").is_empty());
}

#[test]
fn test_a_truck_that_never_slowed_keeps_its_spent_window() {
    // The complement: at 40 the whole way, the warning's window runs out and
    // stays out, so the gate at 40 is the miss it always was.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles() - 0.3;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 40.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    let grace = d.gate_grace_s;
    d.check_gate_approach_warning(&mut app.ctx, grace + 1.0);
    assert!(d.gate_speed_warned);
    at_gate(&mut d, 40.0, true);
    d.handle_arrival_gate(&mut app.ctx);
    assert_eq!(d.gate_miss_count, 1);
}

#[test]
fn test_the_rest_key_short_of_the_gate_names_the_gate() {
    // Stopped and parked fifty feet short of the gate, T opened the
    // emergency shoulder-sleep dialog (agent playtest, 2026-09-02, twice).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles() - 0.01;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 0.0;
    d.trip.stops.clear();
    app.clear_speech();
    let hint = d
        .gate_short_hint(&app.ctx)
        .expect("a gate is a truck length ahead");
    assert!(hint.starts_with("Gate at "), "{hint}");
    assert!(hint.ends_with(" ahead."), "{hint}");
    let parked_before = d.trip.truck.parking_brake;
    d.try_rest_stop(&mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(said.contains("Gate at"), "{said}");
    assert!(!said.contains("shoulder sleep"), "{said}");
    assert_eq!(
        d.trip.truck.parking_brake, parked_before,
        "no dialog opened, so the truck was not secured for one"
    );
    // A mile out it is the ordinary rest key.
    d.trip.position_mi = d.trip.total_miles() - 1.0;
    assert!(d.gate_short_hint(&app.ctx).is_none());
}

#[test]
fn test_the_gate_stop_line_retires_the_ahead_line() {
    // "Destination ahead ... slow down" cut by "At <facility>. Stop
    // completely..." came back BEHIND it, telling a truck at the gate to
    // slow down for the gate (agent playtest, 2026-09-02). The stop line
    // stamps the live fact the ahead line's rescue gate reads.
    use freight_fate::states::driving_updates::live;
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    at_gate(&mut d, 10.0, false);
    live::set_gate_stop_prompted(false);
    d.handle_arrival_gate(&mut app.ctx);
    assert!(
        !live::gate_stop_prompted(),
        "the ahead line is still worth rescuing"
    );
    d.trip.truck.velocity_mps = 2.0 / 2.23694;
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.arrival_full_stop_said);
    assert!(live::gate_stop_prompted(), "and now it is not");
}

#[test]
fn test_slow_crossing_arrives_normally() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    at_gate(&mut d, 10.0, false);
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.trip.finished);
    assert_eq!(d.gate_miss_count, 0);
    assert!(!lines_with(&app, "Destination ahead").is_empty());
    d.trip.truck.velocity_mps = 0.3 / 2.23694;
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.arrival_menu_open);
}

#[test]
fn test_pre_gate_warning_names_a_target_speed() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles() - 0.3;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 40.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    let spoken = app.event_lines();
    assert_eq!(spoken.len(), 1);
    assert!(spoken[0].contains("Facility gate in"));
    assert!(spoken[0].contains("15 miles per hour"));
    assert!(d.gate_grace_s > 0.0);
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    assert_eq!(app.event_lines().len(), 1); // said once, not every frame
}

/// Agent drive A (2026-09-24): "Facility gate in 0.2 miles. Slow to 15
/// miles per hour." and the warning tone, while the keeper already held 15
/// and facility stopping assistance was on. An assist doing the job is not
/// told to do it.
#[test]
fn test_no_pre_gate_warning_when_an_assist_has_the_gate() {
    for assist in [true, false] {
        let mut app = TestApp::new();
        let mut d = a_drive(&mut app);
        app.ctx.settings.destination_approach_assist = assist;
        if !assist {
            // The keeper, already holding the gate's number.
            d.keeper_mph = Some(25.0);
            d.keeper_held_mph = Some(15.0);
        }
        app.clear_speech();
        d.destination_exit_taken = true;
        d.trip.position_mi = d.trip.total_miles() - 0.3;
        d.trip.truck.engine_on = true;
        d.trip.truck.velocity_mps = 16.0 / 2.23694;
        d.check_gate_approach_warning(&mut app.ctx, 0.016);
        assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
        drop(d);
        drop(app);
    }
}

#[test]
fn test_no_instant_miss_inside_the_reaction_window() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles() - 0.3;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 40.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016); // warning spoken, window opens
    let grace = d.gate_grace_s;
    assert!(grace > 0.0);
    d.trip.position_mi = d.trip.total_miles();
    d.trip.finished = true;
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.trip.finished); // still inside the reaction window
    assert_eq!(d.gate_miss_count, 0);
    assert!(!lines_with(&app, "Destination ahead").is_empty());
    d.check_gate_approach_warning(&mut app.ctx, grace + 1.0); // the window expires
    d.handle_arrival_gate(&mut app.ctx);
    assert!(!d.trip.finished);
    assert_eq!(d.gate_miss_count, 1);
}

#[test]
fn test_first_gate_contact_without_a_warning_still_gets_a_window() {
    // A resumed save can arrive at the gate cold: the miss clock must start
    // with the gate's own stop line, never latch on first contact.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    at_gate(&mut d, 40.0, false);
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.trip.finished);
    assert_eq!(d.gate_miss_count, 0);
    assert!(!lines_with(&app, "Destination ahead").is_empty());
    assert!(d.gate_speed_warned);
    assert!(d.gate_grace_s > 0.0);
}

#[test]
fn test_destination_approach_assist_never_misses() {
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = true;
    let mut d = a_drive(&mut app);
    at_gate(&mut d, 40.0, true);
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.trip.finished);
    assert_eq!(d.gate_miss_count, 0);
    assert_eq!(d.trip.truck.brake, 1.0); // the assist is braking the truck itself
}

#[test]
fn test_hazard_braking_never_misses() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    at_gate(&mut d, 40.0, true);
    d.hazard_deadline = Some(5.0); // mid-hazard: braking hard is the right move
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.trip.finished);
    assert_eq!(d.gate_miss_count, 0);
}

#[test]
fn test_repeat_miss_appends_the_help_clause() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    at_gate(&mut d, 70.0, true);
    d.handle_arrival_gate(&mut app.ctx);
    let first = last_with(&app, "Missed the gate at");
    assert!(!first.contains("Settings"));
    at_gate(&mut d, 70.0, true);
    d.handle_arrival_gate(&mut app.ctx);
    let second = lines_with(&app, "Missed the gate at")
        .into_iter()
        .find(|line| line.contains("Brake with"))
        .expect("the repeat miss appends help");
    assert!(second.contains(&first)); // the core line stays identical, help is appended
    assert!(second.contains("Down arrow"));
    assert!(second.contains("Facility stopping assistance"));
}

#[test]
fn test_miss_resets_the_gate_latches() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    at_gate(&mut d, 70.0, true);
    d.arrival_stop_said = true;
    d.arrival_full_stop_said = true;
    d.gate_reminder_s = 5.0;
    d.handle_arrival_gate(&mut app.ctx);
    assert!(!d.arrival_stop_said);
    assert!(!d.arrival_full_stop_said);
    assert_eq!(d.gate_reminder_s, 0.0);
    assert!(!d.gate_speed_warned); // the next approach warns fresh
}

#[test]
fn test_reapproach_after_a_miss_arrives_normally() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    at_gate(&mut d, 70.0, true);
    d.handle_arrival_gate(&mut app.ctx); // the miss
                                         // Back at the gate at a sane speed this time.
    d.trip.position_mi = d.trip.total_miles();
    d.trip.finished = true;
    d.trip.truck.parking_brake = false;
    d.trip.truck.velocity_mps = 2.0 / 2.23694;
    d.handle_arrival_gate(&mut app.ctx);
    assert!(!lines_with(&app, "Stop and set the parking brake.").is_empty());
    d.trip.truck.velocity_mps = 0.3 / 2.23694;
    d.trip.truck.set_parking_brake();
    d.handle_arrival_gate(&mut app.ctx);
    assert!(d.arrival_menu_open);
}

#[test]
fn test_the_gate_warning_names_a_limit_that_is_really_posted() {
    // "Slow to 15" has to be the number in force, not a number nothing posts.
    //
    // The arrival zones are dropped at trip start so no silent low limit writes
    // speeding fines under a spoken 65 on the final freeway miles. That left the
    // pre-gate warning naming 15 while the last half mile still read the
    // corridor's own limit, so every assist held the corridor number straight
    // through the entrance and into the loop-back (owner playtest, 2026-08-21).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // While the truck is still on the highway, nothing has changed: the
    // arrival zones stay off the map.
    assert!(!d.trip.zones.iter().any(|z| z.reason == "facility gate"));
    let end = d.trip.total_miles() - 0.3;
    let (corridor, _) = d.trip.speed_limit_at(end);
    assert!(corridor > FACILITY_GATE_LIMIT_MPH);

    // Taking the destination exit puts the driveway's own limit back.
    // This is the NO-CHAIN case: a facility whose own streets become the
    // trip posts its gate from that chain instead, and posting one here as
    // well would announce the same gate twice. (Python monkeypatched
    // `_surface_chain_route` to None; naming a location the world has no
    // approach for is the same thing without the patch.)
    d.job.destination_location = "no such facility".to_string();
    assert!(d.surface_chain_route(&app.ctx).is_none());
    d.destination_exit_taken = true;
    d.post_gate_zone(&app.ctx);
    let (posted, reason) = d.trip.speed_limit_at(end);
    assert_eq!(reason.as_deref(), Some("facility gate"));
    assert_eq!(posted, FACILITY_GATE_LIMIT_MPH);

    app.clear_speech();
    d.trip.position_mi = end;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 40.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    // The spoken target and the posted limit are the same number.
    let target = app.ctx.settings.speed_text(posted);
    assert!(app.event_lines()[0].contains(&target));
    // Posting it twice would stack two gate zones on one driveway.
    d.post_gate_zone(&app.ctx);
    assert_eq!(
        d.trip
            .zones
            .iter()
            .filter(|z| z.reason == "facility gate")
            .count(),
        1
    );
}

#[test]
fn test_terse_mode_hears_the_essential_cues() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles() - 0.3;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 40.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    let said = last_with(&app, "Gate in");
    assert!(said.contains("Slow to"));
    at_gate(&mut d, 70.0, true);
    d.handle_arrival_gate(&mut app.ctx);
    let said = last_with(&app, "Missed the gate");
    assert_eq!(
        said,
        "Missed the gate, too fast. Slow to 15 miles per hour."
    );
    assert!(said.to_lowercase().contains("slow to"));
}

#[test]
fn test_time_is_charged_each_loop() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let minutes = d.trip.game_minutes;
    for _ in 0..2 {
        at_gate(&mut d, 70.0, true);
        d.handle_arrival_gate(&mut app.ctx);
    }
    assert_eq!(d.trip.game_minutes, minutes + 2.0 * GATE_MISS_LOOP_MIN);
}

#[test]
fn test_missed_gate_loop_charges_hos_fatigue_and_fuel() {
    // The time a miss costs must be real: a gate
    // miss's scripted loop-back costs real HOS, fatigue, and fuel, not just
    // the game clock -- otherwise the loop is a free-time exploit.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let driving_before = hos_of(&app.ctx).driving_min;
    let fatigue_before = profile_of(&app.ctx).fatigue;
    d.trip.truck.rpm = d.trip.truck.specs.idle_rpm;
    let fuel_before = d.trip.truck.fuel_gal;

    at_gate(&mut d, 70.0, true);
    d.handle_arrival_gate(&mut app.ctx);

    assert!((hos_of(&app.ctx).driving_min - (driving_before + GATE_MISS_LOOP_MIN)).abs() < 1e-6);
    assert!(profile_of(&app.ctx).fatigue > fatigue_before);
    assert!(d.trip.truck.fuel_gal < fuel_before);
    // Idle-rate honesty: ~0.8 gal/h floor, so twenty minutes is a small,
    // bounded sip, not a fraction of a highway-cruise burn.
    assert!(fuel_before - d.trip.truck.fuel_gal < 1.0);
}

#[test]
fn test_snapshot_round_trips_the_miss_count() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.gate_miss_count = 2;
    let snapshot = d.snapshot(&app.ctx);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).expect("the drive resumes");
    assert_eq!(resumed.gate_miss_count, 2);
}

#[test]
fn test_a_facility_with_its_own_streets_does_not_get_a_second_gate_zone() {
    // One gate, one announcement.
    //
    // A facility whose approach is a real street chain drives those streets as a
    // trip of their own, and that trip builds a gate zone at its end. Posting one
    // on the highway trip as well put the same gate on the map twice, so the
    // driver heard it announced coming off the ramp and again on the streets
    // (owner, 2026-08-21).
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return; // no turn-level facility approaches in the shipped data
    };
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.job.destination = city;
    d.job.destination_location = location;
    assert!(d.surface_chain_route(&app.ctx).is_some()); // this facility has streets
    d.destination_exit_taken = true;
    d.post_gate_zone(&app.ctx);
    assert!(!d.trip.zones.iter().any(|z| z.reason == "facility gate"));
}

#[test]
fn test_the_hold_prompt_does_not_come_back_once_the_menu_is_open() {
    // "Press Enter to continue" must not be handed back after Enter.
    //
    // The prompt speaks once -- the say-once flag sees to that, and a real
    // roll-in produces exactly one.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.destination_approach_assist = true;
    app.clear_speech();
    at_gate(&mut d, 0.2, false);
    d.handle_arrival_gate(&mut app.ctx);
    let holds = |app: &TestApp| {
        app.event_lines()
            .into_iter()
            .filter(|line| line.contains("Holding at the entrance"))
            .count()
    };
    assert_eq!(holds(&app), 1);
    let prompt = last_with(&app, "Holding at the entrance");
    assert!(prompt.contains("Press Enter"), "{prompt}");
    assert!(!prompt.contains("controller A"), "{prompt}");

    // Said once and only once, however long the driver sits there.
    for _ in 0..50 {
        d.handle_arrival_gate(&mut app.ctx);
    }
    assert_eq!(holds(&app), 1);
}

#[test]
fn test_a_half_full_tank_is_eased_and_stopped_earlier_than_a_dry_van() {
    // Every CDL manual says the same thing about a liquid load: brake earlier
    // and more smoothly, because the surge gives back some of the rate at the
    // worst moment. The truck has known how much since
    // `surge_decel_penalty_mps2` was written, but only the ramp bar asked --
    // the facility arrival, the curve servo and the speed keeper all priced
    // their shed at the dry-van rate (owner, 2026-09-20).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.cargo_kg = CARGO_KG;
    d.trip.truck.velocity_mps = 20.0;

    let dry_ease = d.keeper_ease_mi(15.0, 1.0);
    assert_eq!(
        d.trip.truck.surge_decel_penalty_mps2(),
        0.0,
        "a dry van has no surge to give back"
    );

    // A half-full smooth bore: the worst thing on the roster to stop.
    d.trip.truck.liquid = Some(LiquidLoad::new(0.5, false));
    let wet_penalty = d.trip.truck.surge_decel_penalty_mps2();
    assert!(
        wet_penalty > 0.0,
        "a part-filled tank takes deceleration away: {wet_penalty}"
    );
    let wet_ease = d.keeper_ease_mi(15.0, 1.0);
    assert!(
        wet_ease > dry_ease,
        "the keeper eases further out with liquid aboard: {wet_ease} vs {dry_ease}"
    );
}

#[test]
fn test_the_assist_stops_the_truck_at_a_pickup_gate() {
    // The setting promises to stop at the selected facility arrival point,
    // and it did that at a dock and not at a pickup: the pickup gate had no
    // assist branch at all, so it only ever OPENED the check-in once the
    // truck was already slow. The owner drove into Oshkosh Dry Warehouse
    // with every assist on and stopped the truck himself (2026-09-20).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.destination_approach_assist = true;
    app.clear_speech();
    at_gate(&mut d, 8.0, false);
    d.handle_pickup_gate(&mut app.ctx);
    assert_eq!(d.trip.truck.brake, 1.0, "the assist has to take the brake");
    assert_eq!(d.trip.truck.throttle, 0.0);
    assert!(
        !d.arrival_menu_open,
        "still rolling, nothing to check into yet"
    );

    // Stopped, it holds at the entrance and hands the driver the key --
    // the same line and the same control as the dock.
    d.trip.truck.velocity_mps = 0.0;
    d.handle_pickup_gate(&mut app.ctx);
    let prompt = last_with(&app, "Holding at the entrance");
    assert!(prompt.contains("Press Enter"), "{prompt}");
    assert!(d.trip.truck.parking_brake);
}

#[test]
fn test_manual_delivery_stop_names_parking_and_facility_controls() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.destination_approach_assist = false;
    app.clear_speech();
    at_gate(&mut d, 0.2, false);
    d.arrival_full_stop_said = false;
    d.trip.truck.parking_brake = false;

    d.handle_arrival_gate(&mut app.ctx);

    // Parked, the facility opens on its own: the prompt names the stop and
    // nothing else.
    let prompt = last_with(&app, "Stop and set the parking brake.");
    assert!(!prompt.contains("opens the facility"), "{prompt}");
}

#[test]
fn test_the_approach_assist_stops_the_truck_on_a_facility_street_chain() {
    // Owner, Spokane, 2026-08-21: "it did not automatically stop at the
    // destination; I had to stop." Drives the whole frame loop with the
    // driver's hands off from the moment the assist speaks, and measures the
    // speed the truck was doing when the gate went under its wheels.
    //
    // The rigging is the approach sweep's, because this case and that sweep
    // want the identical drive -- see `states_driving_approach_sweep.rs`,
    // which runs it over fifty destinations in fifty states. A handful of
    // chains here, because ONE is what let this through: the chains differ in
    // length, in how many corners they turn and in where the gate sits, and
    // the first one in the world happens to be among the gentlest.
    let (chain, _) = destinations(get_world(), FEW);
    for destination in &chain {
        let arrival = arrive(destination);
        assert!(
            arrival.on_chain,
            "{}",
            arrival.report(destination) // never reached the facility's own streets
        );
        assert!(arrival.ready, "{}", arrival.report(destination));
        assert!(
            arrival.speed_at_point_mph.unwrap_or(0.0) <= FACILITY_GATE_LIMIT_MPH,
            "{}",
            arrival.report(destination)
        );
    }
}

#[test]
fn test_the_approach_assist_still_has_its_air_at_the_gate() {
    // A facility chain is a string of corners, and the speed keeper works
    // them with the service brakes. The air system charges a whole
    // application every time the pedal RISES and only leakage while it is
    // held, so the assist can only afford the chain if each snub is ONE
    // application.
    //
    // It was not. `update_keeper` returns early on a frame the driver is on
    // the accelerator or the automatic is mid-shift, and the input pass at
    // the top of the frame had already bled the pedal the keeper left behind
    // -- so the same snub was re-made, and re-charged, nine times a second.
    // A hundred and twenty-five psi to the spring-brake trip in twenty
    // seconds, and the truck set its own parking brakes half a mile short of
    // Aberdeen Company Yard with the delivery unfinishable (2026-09-18).
    //
    // Arriving is not enough to catch that: the truck can arrive on the last
    // of its air, or be stopped by the spring brakes ON the gate and read as
    // parked there. This case measures the tanks the whole way in.
    // The bar is the truck's OWN low-air warning, not a number chosen here:
    // an approach the driver never even hears an air warning on is an
    // approach that never spent the air, and the warning sits a long way
    // above the spring-brake trip, so the case fails while the truck is
    // still drivable rather than only once it has already stranded itself.
    let (chain, _) = destinations(get_world(), FEW);
    for destination in &chain {
        let arrival = arrive(destination);
        assert!(arrival.on_chain, "{}", arrival.report(destination));
        // A run that sampled nothing leaves the floor at infinity and would
        // pass the real bar below without measuring anything at all.
        assert!(
            arrival.min_air_psi.is_finite(),
            "{}",
            arrival.report(destination)
        );
        assert!(
            arrival.min_air_psi > arrival.air_low_warning_psi,
            "{}",
            arrival.report(destination)
        );
    }
}

#[test]
fn test_the_approach_assist_stops_within_a_truck_length_of_the_gate() {
    // The other half of the Spokane report: stopping means stopping AT the
    // gate, not a city block past it.
    let (chain, _) = destinations(get_world(), FEW);
    for destination in &chain {
        let arrival = arrive(destination);
        assert!(
            arrival.past_the_point_ft <= TRUCK_LENGTH_FT,
            "stopped {:.0} feet past the gate\n{}",
            arrival.past_the_point_ft,
            arrival.report(destination)
        );
    }
}

#[test]
fn test_the_ramp_is_not_the_arrival_when_a_street_chain_follows_it() {
    // Owner, Spokane, 2026-08-22: "it didn't stop where I can pull it in."
    //
    // With the pedal finally reaching the truck, the assist stopped it dead at
    // the bottom of the destination ramp -- a mile of city streets short of the
    // gate -- and left automatic speed control paused the arrival way for the
    // whole chain. The ramp's end is a driving continuation when a chain
    // follows; the arrival is the chain's own, a mile on.
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return; // no baked facility street chain in the shipped data
    };
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = true;
    let mut d = a_drive(&mut app);
    d.job.destination = city;
    d.job.destination_location = location;
    d.destination_chain_ahead = None;
    assert!(d.destination_street_chain_ahead(&app.ctx));

    // On the destination ramp, well inside the distance a 30 mph truck needs
    // to stop: exactly where the ramp-as-gate branch latched.
    let destination = d
        .destination_exit_stop(&mut app.ctx)
        .expect("a destination exit");
    d.ramp_stop = Some(destination);
    d.ramp_mi = Some(0.05);
    d.trip.truck.start_engine();
    d.trip.truck.transmission.automatic = true;
    d.trip.truck.transmission.gear = 6;
    d.trip.truck.velocity_mps = 30.0 / 2.23694;
    d.trip.truck.brake = 0.0;
    d.update_destination_approach_assist(&mut app.ctx);

    assert!(!d.destination_arrival_active);
    assert_eq!(d.trip.truck.brake, 0.0);
    assert_eq!(d.destination_assist_brake, 0.0);

    // And the same ramp IS the arrival when nothing follows it: the
    // ramp-to-dock delivery the 2026-08-19 fix was made for still stops.
    d.destination_chain_ahead = Some(false);
    d.ramp_mi = Some(0.05);
    d.update_destination_approach_assist(&mut app.ctx);
    assert!(d.destination_arrival_active);
    assert!(d.trip.truck.brake > 0.0);
}

#[test]
fn test_a_blown_gate_loop_back_lets_the_arrival_latch_go() {
    // The loop-back promised "a fresh approach: the assist announces itself
    // again", but left the latch set -- so it never did, and held the retry
    // at the facility-lane walk from the turnaround on (owner, 2026-09-03).
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = true;
    let mut d = a_drive(&mut app);
    d.destination_chain_ahead = Some(false);
    let destination = d
        .destination_exit_stop(&mut app.ctx)
        .expect("a destination exit");
    d.ramp_stop = Some(destination.clone());
    d.ramp_mi = Some(0.0);
    d.ramp_terminal_done = true;
    d.destination_arrival_active = true;
    d.destination_assist_brake = 0.6;

    d.loop_back_to_destination_terminal(&mut app.ctx, &destination);

    assert!(!d.destination_arrival_active);
    assert_eq!(d.destination_assist_brake, 0.0);
    assert!(!d.approach_pull_ahead);
    assert!(d.ramp_mi.is_some_and(|mi| mi > 0.0));
}

#[test]
fn test_a_stop_menu_lets_go_of_every_assist_brake() {
    // Shane, Jerry and Jessie, 2026-09-21: facility stopping assistance
    // brought the truck into a stop, and back on the road it would not move
    // -- parking brake off, engine revving, 0 mph. The arrival's brake was
    // still latched under the menu, and the brake ramp now holds any pedal an
    // assist is holding, so the truck sat on it for good.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = 0.0;
    d.destination_arrival_active = true;
    d.destination_assist_brake = 1.0;
    d.keeper_snub = 0.4;
    d.aeb_brake = 0.5;

    assert!(secure_truck_for_stopped_menu(&mut d, &mut app.ctx));
    assert!(!d.destination_arrival_active);
    assert_eq!(d.destination_assist_brake, 0.0);
    assert_eq!(d.keeper_snub, 0.0);
    assert_eq!(d.aeb_brake, 0.0);
    assert!(d.curve_servo.is_none());

    // Back on the road: release the parking brake and the service brake
    // comes off on its own.
    d.trip.truck.release_parking_brake();
    for _ in 0..120 {
        d.update_frame(&mut app.ctx, 1.0 / 60.0);
    }
    assert_eq!(d.trip.truck.brake, 0.0);
}

#[test]
fn test_the_approach_assist_delivers_the_truck_to_the_dock() {
    // Jerry, Hobbs Food Processing Plant, 2026-08-22: through the ramp light
    // on green, "Destination approach assistance slowing", 8 miles per hour,
    // 2 miles per hour -- and then nothing. He sat, turned the assist off in
    // settings, and the dock opened five seconds after he resumed. The
    // requirement is the DOCK, not a stop: on a facility whose ramp ends at
    // the gate, the menu has to open with nobody touching a pedal.
    let (_, plain) = destinations(get_world(), 1);
    let destination = &plain[0];
    let arrival = arrive(destination);
    assert!(arrival.docked, "{}", arrival.report(destination));
    assert!(
        arrival.assist_spoke,
        "the assist took the pedals without a word\n{}",
        arrival.report(destination)
    );
    // The assist walks the truck over the point at two miles an hour and its
    // own brake lands it; the ramp must not bark "come to a complete stop" at
    // a driver whose truck is already doing exactly that.
    assert!(
        !arrival.said("Come to a stop."),
        "{}",
        arrival.report(destination)
    );
}

#[test]
fn test_the_approach_assist_delivers_the_truck_to_a_street_chain_gate_uphill() {
    // The same promise on a facility street chain with the gate at the top of
    // a grade: the road takes speed off for free there, which is exactly
    // where a brake-only stop profile undershoots -- it converges on a halt
    // SHORT of the gate, brake held, and the dock never opens.
    //
    // The grade is baked, because the shipped chains are laid on local street
    // geometry that carries no grade segments at all: every one of them reads
    // dead level, so a real climb to a gate cannot be found, only built.
    let (chain, _) = destinations(get_world(), 1);
    let destination = &chain[0];
    let arrival = arrive_over(destination, Some(0.03));
    assert!(
        arrival.on_chain && arrival.ready && arrival.speed_mph <= DOCKING_MAX_MPH,
        "{}",
        arrival.report(destination)
    );
    assert!(
        arrival.past_the_point_ft <= TRUCK_LENGTH_FT,
        "stopped {:.0} feet past a gate at the top of a three percent climb\n{}",
        arrival.past_the_point_ft,
        arrival.report(destination)
    );
}

#[test]
fn test_off_the_ramp_carries_the_first_corner_and_hands_speed_control_back() {
    // Owner, Spokane, 2026-08-22: "the assist did not stop and I had to
    // brake to start the turn onto city streets."
    //
    // The chain begins a few hundred feet before its first corner. Raised on
    // its own a frame after the swap, that corner's call queued as a
    // droppable lead behind the off-the-ramp line and the gate warning, went
    // stale, and was dropped -- twice, the second time after the loop-back --
    // so it was never heard. And the truck came off with nothing holding it:
    // the ramp's transit pause was still on, so the keeper that would have
    // eased for the corner had not come back.
    //
    // The first corner is spoken IN the handoff line, and the pause ends at
    // the handoff.
    let world = get_world();
    let Some((city, location)) = a_facility_with_an_early_corner(world) else {
        return; // no baked chain with a corner inside the first tenth of a mile
    };
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = true;
    app.ctx.settings.speed_keeper = true;
    let mut d = a_drive(&mut app);
    d.job.destination = city;
    d.job.destination_location = location;
    d.destination_chain_ahead = None;

    // The ramp's transit pause, exactly as the terminal leaves it.
    d.speed_control_armed = true;
    d.pause_speed_control(&mut app.ctx, true);
    assert!(d.speed_control_paused_at_stop);

    d.trip.position_mi = d.trip.total_miles();
    d.trip.finished = true;
    app.clear_speech();
    assert!(d.begin_surface_chain(&mut app.ctx, true));

    let message = last_with(&app, "to the facility gate.");
    let first_street = d.trip.route.legs[1].highway.clone();
    assert!(message.contains(&first_street), "{message}");
    assert!(message.to_lowercase().contains("turn"), "{message}");
    // Spoken here, so the commitment loop must not raise it again.
    let corner = d.turn_cue_in_play().expect("a corner is in play");
    assert!(d.turn_advised.contains(&corner.key));
    // The clock goes real at the corner's brake point, which a truck still
    // at the stop bar has not reached (states_driving_turns pins that).
    assert_eq!(
        d.trip.controlled_turn,
        corner.at_mi - d.trip.position_mi <= d.turn_brake_point_mi(&corner)
    );
    // And the pause is gone: the next frame may hand the streets to the
    // keeper without waiting on a driver who is off the brake.
    assert!(!d.speed_control_paused_at_stop);
    // "Then turn left now onto North 1st Street" said it: the route's own
    // call at the corner is not said again on top (agent drive, exit 286A,
    // 2026-09-23).
    if message.contains(" now onto ") {
        let before = app.event_lines().len();
        let near = ff_core::sim::trip_models::TripEvent {
            kind: ff_core::sim::trip_models::TripEventKind::GpsCue,
            message: ff_core::speech_text::SpokenMessage::new(corner.near_text.clone()),
            data: ff_core::sim::trip_models::TripEventData {
                cue: Some(corner.clone()),
                ..Default::default()
            },
        };
        d.handle_trip_event(&mut app.ctx, &near);
        assert_eq!(app.event_lines().len(), before, "{:?}", app.event_lines());
    }
}

// -- the pickup gate (tests/test_pickup_loading.py, drive half) -----------------------

#[test]
fn test_the_pickup_gate_asks_for_a_complete_stop_then_a_check_in() {
    let world = get_world();
    let mut app = TestApp::new();
    let mut profile = Profile::named_in("Deadhead", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_PICKUP,
        Some(12.0),
    );
    d.trip.set_npc_vehicles(Vec::new());
    app.clear_speech();

    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 20.0 / 2.23694;
    d.handle_pickup_gate(&mut app.ctx);
    let said = last_with(&app, "Pickup ahead: ");
    assert!(said.contains("Stop at the gate"));
    assert!(d.arrival_stop_said);

    // Creeping in: the gate asks for the check-in rather than repeating.
    d.trip.truck.velocity_mps = 2.0 / 2.23694;
    d.handle_pickup_gate(&mut app.ctx);
    let said = last_with(&app, "Stop and set the parking brake.");
    assert!(said.starts_with("At "));
    assert!(d.arrival_full_stop_said);
}

#[test]
fn test_the_pickup_progress_summary_speaks_the_players_unit() {
    // Spoken distances go through the unit setting; a player on metric must
    // not hear miles here just because this handler moved modules.
    let world = get_world();
    let mut app = TestApp::new();
    let mut profile = Profile::named_in("Deadhead", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_PICKUP,
        Some(12.0),
    );
    let imperial = d.pickup_progress_summary(&app.ctx);
    assert!(imperial.contains("miles remaining of"));
    assert!(imperial.ends_with(&format!(
        "to pickup at {}.",
        d.pickup_facility_text(&app.ctx)
    )));

    app.ctx.settings.imperial_units = false;
    let metric = d.pickup_progress_summary(&app.ctx);
    assert!(metric.contains("kilometers remaining of"));
    assert!(!metric.contains("miles"));
}

// -- the tank load's cue layer (tests/test_tanker_surge.py, drive half) ---------------

/// What a [`SurgeAudio`] was asked to do. Python's `_Audio` recorded exactly
/// these four calls.
#[derive(Debug, Default)]
struct SurgeCalls {
    /// `start_loop(channel, key, volume)`.
    loops: Vec<(u32, String, f64)>,
    /// `set_loop_volume(channel, volume)`.
    volumes: Vec<(u32, f64)>,
    /// `stop_loop(channel)`.
    stops: Vec<u32>,
    /// `play(key, volume)`.
    played: Vec<(String, f64)>,
}

impl SurgeCalls {
    fn is_empty(&self) -> bool {
        self.loops.is_empty()
            && self.volumes.is_empty()
            && self.stops.is_empty()
            && self.played.is_empty()
    }
}

type SurgeLog = Rc<RefCell<SurgeCalls>>;

#[derive(Default)]
struct SurgeAudio {
    log: SurgeLog,
}

impl Audio for SurgeAudio {
    fn enabled(&self) -> bool {
        false
    }
    fn backend_name(&self) -> &str {
        "surge-recording"
    }
    fn master_volume(&self) -> f64 {
        1.0
    }
    fn sfx_volume(&self) -> f64 {
        1.0
    }
    fn music_volume(&self) -> f64 {
        1.0
    }
    fn weather_volume(&self) -> f64 {
        1.0
    }
    fn engine_volume(&self) -> f64 {
        1.0
    }
    fn ui_volume(&self) -> f64 {
        1.0
    }
    fn engine_running(&self) -> bool {
        false
    }
    fn engine_starting(&self) -> bool {
        false
    }
    fn voice_key(&self, key: &str) -> String {
        key.to_string()
    }
    fn play_with(&mut self, key: &str, volume: f64, _pan: f64) {
        self.log.borrow_mut().played.push((key.to_string(), volume));
    }
    fn play_bank_with(&mut self, base: &str, _fallback: &str, volume: f64, pan: f64) {
        self.play_with(base, volume, pan);
    }
    fn set_engine_duck(&mut self, _duck: f64) {}
    fn set_speech_duck(&mut self, _duck: f64) {}
    fn set_engine_voice(&mut self, _classic: bool) {}
    fn set_jake_voice(&mut self, _classic: bool) {}
    fn has_asset(&mut self, _key: &str) -> bool {
        true
    }
    fn start_loop_with(&mut self, channel: u32, key: &str, volume: f64, _fade_ms: u32) {
        self.log
            .borrow_mut()
            .loops
            .push((channel, key.to_string(), volume));
    }
    fn set_loop_volume(&mut self, channel: u32, volume: f64) {
        self.log.borrow_mut().volumes.push((channel, volume));
    }
    fn set_loop_pan(&mut self, _channel: u32, _pan: f64) {}
    fn stop_loop_with(&mut self, channel: u32, _fade_ms: u32) {
        self.log.borrow_mut().stops.push(channel);
    }
    fn start_sustain_loop_with(
        &mut self,
        _channel: u32,
        _key: &str,
        _spec: SustainLoopSpec,
        _volume: f64,
    ) {
    }
    fn release_sustain_loop_with(&mut self, _channel: u32, _fade_ms: u32) {}
    fn hold_alert_with(&mut self, _key: &str, _volume: f64, _fade_ms: u32) {}
    fn release_alert_with(&mut self, _fade_ms: u32) {}
    fn hold_cue(&mut self, _name: &str) {}
    fn cue_held(&self, _name: &str) -> bool {
        false
    }
    fn release_cue(&mut self, _name: &str) {}
    fn engine_start_with(&mut self, _play_start_sound: bool) {}
    fn engine_stop_with(&mut self, _shutdown_sound: bool) {}
    fn update(&mut self, _dt: f64) {}
    fn set_engine_rpm_with(&mut self, _rpm: f64, _throttle: f64) {}
    fn set_road_noise(&mut self, _speed_mps: f64) {}
    fn set_weather_with(&mut self, _key: Option<&str>, _intensity: f64) {}
    fn set_wind(&mut self, _intensity: f64) {}
    fn set_ambient_with(&mut self, _key: Option<&str>, _volume: f64) {}
    fn horn_start(&mut self) {}
    fn horn_stop(&mut self) {}
    fn reverse_start(&mut self) {}
    fn reverse_stop(&mut self) {}
    fn stop_world(&mut self) {}
    fn play_music_with(&mut self, _track: &str, _fade_ms: u32) {}
    fn play_radio_stream_with(&mut self, _url: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn play_music_file_with(&mut self, _path: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn music_playing(&self) -> bool {
        false
    }
    fn radio_now_playing(&self) -> Option<String> {
        None
    }
    fn stop_music_with(&mut self, _fade_ms: u32) {}
    fn set_volumes(&mut self, _volumes: &VolumeUpdate) {}
    fn shutdown(&mut self) {}
}

const CARGO_KG: f64 = 20_000.0;

/// `_truck(liquid, speed_mps)`.
fn a_tank_truck(liquid: Option<LiquidLoad>, speed_mps: f64) -> TruckState {
    let mut t = TruckState {
        cargo_kg: CARGO_KG,
        velocity_mps: speed_mps,
        liquid,
        ..Default::default()
    };
    t.set_air_ready(false);
    t
}

/// A drive whose truck is the one handed in, with a recording audio in place.
///
/// The event pacer runs on a fake clock the frame loop below advances: sixty
/// simulated seconds pass in microseconds of real time here, and without that
/// the pacer projects the first line as still in the air and drops the
/// settling line as chatter that would start stale.
fn a_tank_drive(
    app: &mut TestApp,
    truck: TruckState,
    terse: bool,
) -> (DrivingState, SurgeLog, FakeClock) {
    if terse {
        app.ctx.settings.driving_speech = "quiet".to_string();
    }
    let mut d = a_drive(app);
    d.trip.truck = truck;
    let audio = SurgeAudio::default();
    let log = Rc::clone(&audio.log);
    app.ctx.audio = Box::new(audio);
    let clock = app.fake_pacer_clock();
    app.clear_speech();
    (d, log, clock)
}

/// `_drive(driver, steps, decel, brake_steps)`.
fn drive_liquid(
    d: &mut DrivingState,
    app: &mut TestApp,
    clock: &FakeClock,
    steps: i32,
    decel: f64,
    brake_steps: i32,
) {
    for step in 0..steps {
        clock.advance(0.02);
        d.trip.truck.brake = if step < brake_steps { 1.0 } else { 0.0 };
        let accel = if step < brake_steps { -decel } else { 0.0 };
        if let Some(liquid) = d.trip.truck.liquid.as_mut() {
            liquid.update(0.02, accel, 0.0);
        }
        d.update_liquid_cues(&mut app.ctx, 0.02);
    }
}

#[test]
fn test_the_cue_layer_is_completely_silent_for_other_freight() {
    let mut app = TestApp::new();
    let (mut d, log, clock) = a_tank_drive(&mut app, a_tank_truck(None, 24.6), false);
    drive_liquid(&mut d, &mut app, &clock, 400, 4.0, 100);
    assert!(log.borrow().is_empty());
    assert!(app.event_lines().is_empty());
}

#[test]
fn test_the_wash_is_silent_on_steady_cruise_and_alive_once_the_load_runs() {
    let mut app = TestApp::new();
    let (mut d, log, clock) = a_tank_drive(
        &mut app,
        a_tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6),
        false,
    );
    // Steady: nothing moving, nothing sounding.
    drive_liquid(&mut d, &mut app, &clock, 50, 0.0, 0);
    assert!(log.borrow().is_empty());
    // Braking: the bed comes up.
    drive_liquid(&mut d, &mut app, &clock, 200, 4.0, 100);
    let log = log.borrow();
    assert!(
        !log.loops.is_empty(),
        "the wash should sound while the liquid is running"
    );
    assert!(log.loops.iter().all(|c| c.1 == "vehicle/liquid_wash"));
    assert!(log.loops.iter().all(|c| c.0 == CH_SURGE));
}

#[test]
fn test_the_hit_fires_when_the_wave_arrives_and_is_the_loudest_thing_here() {
    let mut app = TestApp::new();
    let (mut d, log, clock) = a_tank_drive(
        &mut app,
        a_tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6),
        false,
    );
    drive_liquid(&mut d, &mut app, &clock, 400, 4.0, 100);
    let log = log.borrow();
    assert!(
        !log.played.is_empty(),
        "the arriving wave should be a one-shot hit"
    );
    assert!(log.played.iter().any(|c| c.0 == "vehicle/liquid_hit"));
    let loudest_hit = log.played.iter().map(|c| c.1).fold(f64::MIN, f64::max);
    let loudest_wash = log.loops.iter().map(|c| c.2).fold(0.0, f64::max);
    assert!(loudest_hit > loudest_wash);
}

#[test]
fn test_the_lateral_hit_has_its_own_voice() {
    let mut app = TestApp::new();
    let mut truck = a_tank_truck(Some(LiquidLoad::new(0.5, false)), 20.0);
    truck.corner_advisory_mph = 25.0;
    let (mut d, log, clock) = a_tank_drive(&mut app, truck, false);
    // A reversal, not one steady bend: the pull builds in over a transition,
    // so a single bend held hot sets the liquid against the wall without a
    // slam (NTSB HAR-11/01 2.3.4 -- it is a quick succession of inputs that
    // throws it). A bend and a straight, a second each.
    for step in 0..500 {
        d.trip.truck.corner_advisory_mph = if (step / 50) % 2 == 0 { 25.0 } else { 0.0 };
        clock.advance(0.02);
        d.trip.truck.update(0.02);
        d.update_liquid_cues(&mut app.ctx, 0.02);
    }
    let log = log.borrow();
    assert!(log
        .played
        .iter()
        .any(|c| c.0 == "vehicle/liquid_hit_lateral"));
}

#[test]
fn test_the_load_running_and_the_load_settling_are_both_spoken() {
    // A downward line is required: silence is otherwise ambiguous between
    // "settled" and "the cue layer stopped working".
    //
    // Python ran this at both verbosities against a stub context. Here the
    // real ladder is in the path, and at `quiet` it retires STATUS and
    // CONFIRMATION to earcons by design (`DRIVING_SPEECH_DISPOSITIONS`), so
    // the terse WORDING is asserted directly rather than through the voice.
    let mut app = TestApp::new();
    let (mut d, _log, clock) = a_tank_drive(
        &mut app,
        a_tank_truck(Some(LiquidLoad::new(0.5, true)), 24.6),
        false,
    );
    drive_liquid(&mut d, &mut app, &clock, 3000, 4.0, 100);
    let said = app.event_lines().join(" ").to_lowercase();
    assert!(said.contains("running forward"), "{said}");
    assert!(said.contains("settled"), "{said}");
}

#[test]
fn test_the_terse_load_lines_still_say_running_forward_and_settled() {
    // The terse half of the pair above, read off the mixin: at `quiet` the
    // ladder carries these two categories as earcons, so the words never
    // reach the capture -- but they still have to BE the short forms.
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let (mut d, _log, clock) = a_tank_drive(
        &mut app,
        a_tank_truck(Some(LiquidLoad::new(0.5, true)), 24.6),
        false,
    );
    assert!(d.terse_speech(&app.ctx));
    drive_liquid(&mut d, &mut app, &clock, 3000, 4.0, 100);
    let review = app
        .app
        .ctx
        .message_log
        .filtered_messages()
        .iter()
        .map(|message| message.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(review.contains("load running forward"), "{review}");
    assert!(review.contains("load settled"), "{review}");
}

#[test]
fn test_the_bed_is_dropped_on_the_way_out() {
    let mut app = TestApp::new();
    let (mut d, log, clock) = a_tank_drive(
        &mut app,
        a_tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6),
        false,
    );
    drive_liquid(&mut d, &mut app, &clock, 200, 4.0, 100);
    log.borrow_mut().stops.clear();
    d.stop_liquid_cues(&mut app.ctx);
    assert!(log.borrow().stops.contains(&CH_SURGE));
}

#[test]
fn test_the_status_screen_can_be_asked_what_the_tank_will_do() {
    // One app for all three: `TestApp` holds the process-wide environment
    // lock, so a second one built while the first is alive would deadlock.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);

    d.trip.truck = a_tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6);
    let clause = d.liquid_status_clause();
    assert!(clause.contains("smooth bore"));
    assert!(clause.contains("half full"));
    assert!(!clause.to_lowercase().contains("outage"));

    d.trip.truck = a_tank_truck(Some(LiquidLoad::new(0.5, true)), 24.6);
    assert!(d.liquid_status_clause().contains("baffled"));

    d.trip.truck = a_tank_truck(None, 24.6);
    assert_eq!(d.liquid_status_clause(), "");
}

// -- the stop cues measure against what the truck can do (driving_stops.rs) ------------

#[test]
fn test_the_stop_bar_tick_range_is_unchanged_for_ordinary_freight() {
    // The floor is the promise: a truck that stops inside the old three
    // hundred feet hears the bar exactly where it always did.
    let truck = a_tank_truck(None, 13.4); // 30 mph, a normal bar approach
    assert!((bar_tick_range_mi(&truck) - RAMP_BAR_TICK_RANGE_MI).abs() < 1e-9);
    assert!((bar_solid_zone_mi(&truck) - RAMP_BAR_SOLID_MI).abs() < 1e-9);
}

#[test]
fn test_the_stop_bar_tick_starts_earlier_when_the_truck_needs_more_road() {
    // A blind driver's whole model of where the bar is, is the tick's rate.
    // It has to be measured against a range this truck can stop inside.
    let dry = a_tank_truck(None, 24.6);
    let wet = a_tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6);
    assert!(bar_tick_range_mi(&wet) > bar_tick_range_mi(&dry));

    // And the same is true of every other reason a truck stops long, which is
    // the point: this was owed work independent of any tank.
    let mut icy = a_tank_truck(None, 24.6);
    icy.grip = 0.3;
    assert!(bar_tick_range_mi(&icy) > bar_tick_range_mi(&dry));

    let mut downhill = a_tank_truck(None, 24.6);
    downhill.grade = -0.06;
    assert!(bar_tick_range_mi(&downhill) > bar_tick_range_mi(&dry));

    let mut faded = a_tank_truck(None, 24.6);
    faded.brake_temp_c = 600.0;
    faded.brake_wear_pct = 90.0;
    assert!(bar_tick_range_mi(&faded) > bar_tick_range_mi(&dry));
}

#[test]
fn test_the_held_tone_comes_in_early_when_sixty_feet_is_not_enough() {
    let close = a_tank_truck(Some(LiquidLoad::new(0.5, false)), 13.4);
    assert!(bar_solid_zone_mi(&close) > RAMP_BAR_SOLID_MI);
}

#[test]
fn test_the_stopping_assist_can_only_ever_press_harder_than_it_used_to() {
    for truck in [
        a_tank_truck(None, 24.6),
        a_tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6),
        a_tank_truck(Some(LiquidLoad::new(0.5, true)), 24.6),
    ] {
        assert!(assist_full_decel_mps2(&truck) <= RAMP_ASSIST_FULL_DECEL_MPS2);
        assert!(assist_full_decel_mps2(&truck) > 0.0);
    }
}

#[test]
fn test_the_servo_rises_at_once_and_releases_only_past_the_band() {
    // Easing off is free; it is coming back on that costs air, so the pedal
    // follows a falling demand only once the fall is worth a release (bench
    // trace, 2026-08-11: 276 applications on one flat approach).
    let truck = a_tank_truck(None, 24.6);
    let full = assist_full_decel_mps2(&truck);
    let floor = RAMP_ASSIST_DECEL_START_MPS2 / full;
    // The floor and the trigger demand agree at first press.
    assert!((assist_servo_brake(0.0, RAMP_ASSIST_DECEL_START_MPS2, &truck) - floor).abs() < 1e-9);
    // A rising demand is taken at once.
    let harder = assist_servo_brake(floor, full * 0.8, &truck);
    assert!(harder > floor);
    // A demand that dips a hair under the pedal costs no application.
    let dip = full * (harder - RAMP_ASSIST_RELEASE_BAND / 2.0);
    assert_eq!(assist_servo_brake(harder, dip, &truck), harder);
    // Past the band it does let go.
    let real_fall = full * (harder - RAMP_ASSIST_RELEASE_BAND * 2.0);
    assert!(assist_servo_brake(harder, real_fall, &truck) < harder);
    // And it never asks for more pedal than there is.
    assert_eq!(assist_servo_brake(0.0, 99.0, &truck), 1.0);
}

// -- the pull-in beat hands the dock menu the drive it belongs to ----------------------

/// Step the state on top of the stack until its countdown ends
/// (`PlaytestHarness::finish_timed_state`, without the harness).
fn finish_timed_state(app: &mut TestApp) {
    for _ in 0..10_000 {
        let Some(state) = app.ctx.state() else { return };
        let remaining = state
            .borrow()
            .as_any()
            .downcast_ref::<TimedMessageState>()
            .map(|timed| timed.remaining)
            .unwrap_or(0.0);
        if remaining <= 0.0 {
            return;
        }
        state.borrow_mut().update(&mut app.ctx, 1.0 / 60.0);
        app.ctx.run_deferred();
    }
    panic!("the pull-in beat never counted down");
}

/// The dock menu opens after the pull-in wait, and the drive is still alive.
///
/// The wait screen REPLACES the drive on the stack, so its completion has to
/// carry a handle to the drive rather than go looking for one: when it
/// looked, it found nothing and the game sat on "Pulling into destination"
/// with no menu, forever.
#[test]
fn test_pull_in_beat_opens_the_dock_menu_over_the_live_drive() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    at_gate(&mut drive, 0.3, false);
    app.push_state(drive);
    let shared = app.state().expect("the drive is the active state");
    app.clear_speech();

    {
        let mut borrowed = shared.borrow_mut();
        let d = borrowed
            .as_any_mut()
            .downcast_mut::<DrivingState>()
            .expect("the pushed state is the drive");
        d.handle_arrival_gate(&mut app.ctx);
        assert!(d.arrival_menu_open);
    }
    app.ctx.run_deferred();
    // The wait screen took the drive's place, exactly as Python's did.
    assert_eq!(app.ctx.stack_len(), 1);

    finish_timed_state(&mut app);

    let top = app.state().expect("a state is on the stack");
    assert!(
        top.borrow().as_any().is::<FacilityArrivalState>(),
        "the pull-in beat did not open the dock menu; the screen still says {:?}",
        app.visible_lines()
    );
    // The drive it covers is the one that pulled in, still live off the stack.
    let mut borrowed = shared.borrow_mut();
    let d = borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .expect("the drive outlived the wait screen");
    assert_eq!(d.status_text, "Parked at destination. Dock and deliver.");
}

/// The same hand-off on the pickup side: the check-in menu opens.
#[test]
fn test_pull_in_beat_opens_the_check_in_menu_over_the_live_drive() {
    let mut app = TestApp::new();
    let mut drive = a_pickup_drive(&mut app);
    drive.trip.position_mi = drive.trip.total_miles();
    drive.trip.finished = true;
    drive.trip.truck.engine_on = true;
    drive.trip.truck.velocity_mps = 0.0;
    app.push_state(drive);
    let shared = app.state().expect("the drive is the active state");
    app.clear_speech();

    {
        let mut borrowed = shared.borrow_mut();
        let d = borrowed
            .as_any_mut()
            .downcast_mut::<DrivingState>()
            .expect("the pushed state is the drive");
        d.handle_pickup_gate(&mut app.ctx);
    }
    app.ctx.run_deferred();
    assert_eq!(app.ctx.stack_len(), 1);

    finish_timed_state(&mut app);

    let top = app.state().expect("a state is on the stack");
    assert!(
        top.borrow().as_any().is::<PickupFacilityState>(),
        "the pull-in beat did not open the check-in menu; the screen still says {:?}",
        app.visible_lines()
    );
    let mut borrowed = shared.borrow_mut();
    let d = borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .expect("the drive outlived the wait screen");
    assert_eq!(d.status_text, "Parked at pickup. Check in and load.");
}

// -- the pre-gate warning on a street chain -------------------------------------------

/// A delivery to `city` / `location_name` from Dallas, the Tyler run's origin.
fn a_drive_to_facility(app: &mut TestApp, city: &str, location_name: &str) -> DrivingState {
    use ff_core::models::jobs::make_reposition_job;
    let world = get_world();
    let mut profile = Profile::named_in("Gates", "Dallas");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .shortest_route("dallas_tx_us", city, None, false)
        .expect("the world routes")
        .expect("Dallas has a route there");
    let mut job = make_reposition_job(world, "dallas_tx_us", city, false, None)
        .expect("a reposition job exists");
    job.destination_location = location_name.to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

#[test]
fn test_the_gate_warning_speaks_once_as_a_street_gate_comes_up() {
    // Tyler Cross-Dock by ear (agent playtest, 2026-09-03): "Gate in 0.8
    // kilometers" four corners before the yard, and then a second time after
    // every corner's slowdown had retired the first. Tyler Company Yard's
    // streets end at its gate with no driveway mapped, so no stretch of that
    // public street is signed at the gate's 15 any more: the street keeps its
    // own limit to the gate, and the warning is the braking a loaded truck
    // needs to be down to the gate speed there, spoken once per approach;
    // obeying it still earns the gate's own window at contact (the 2026-09-02
    // rule).
    let mut app = TestApp::new();
    let mut d = a_drive_to_facility(&mut app, "tyler_tx_us", "Tyler Company Yard");
    d.destination_exit_taken = true;
    assert!(d.begin_surface_chain(&mut app.ctx, false));
    assert!(
        d.trip.gate_zone().is_none(),
        "no gate zone on a public street: {:?}",
        d.trip.zones
    );
    assert!(
        d.trip
            .zones
            .iter()
            .all(|zone| zone.limit_mph > FACILITY_GATE_LIMIT_MPH),
        "{:?}",
        d.trip.zones
    );
    let total = d.trip.total_miles();
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 30.0 / 2.23694;
    app.clear_speech();

    // Half a mile out at 30: streets and corners still to come.
    d.trip.position_mi = total - 0.5;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    assert!(
        lines_with(&app, "ate in").is_empty(),
        "warned four corners out: {:?}",
        app.event_lines()
    );

    // Inside the braking a loaded truck needs from 30 to 15 (about a tenth
    // of a mile at the normal rate): one warning.
    d.trip.position_mi = total - 0.09;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    assert_eq!(
        lines_with(&app, "ate in").len(),
        1,
        "{:?}",
        app.event_lines()
    );
    assert!(d.gate_speed_warned);
    let grace = d.gate_grace_s;
    assert!(grace > 0.0);

    // Slowed as told, window spent: the warning is obeyed.
    d.trip.position_mi = total - 0.05;
    d.trip.truck.velocity_mps = 8.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, grace + 1.0);
    assert!(
        !d.gate_speed_warned,
        "obeying the warning retires its window"
    );

    // Back up to 30: no second line...
    d.trip.position_mi = total - 0.03;
    d.trip.truck.velocity_mps = 30.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    assert_eq!(
        lines_with(&app, "ate in").len(),
        1,
        "{:?}",
        app.event_lines()
    );
    // ...and the gate itself still opens a fresh window on contact.
    assert!(!d.gate_speed_warned);
}

#[test]
fn test_behind_a_driveway_the_gate_warning_waits_for_the_yard() {
    // Abilene Company Yard's chain leaves the public street at a service
    // road. Up to that driveway the street keeps its own limit and the
    // driveway is a turn with its own call; the yard limit, and the check-in
    // warning, begin past it.
    let mut app = TestApp::new();
    let mut d = a_drive_to_facility(&mut app, "abilene_tx_us", "Abilene Company Yard");
    d.destination_exit_taken = true;
    assert!(d.begin_surface_chain(&mut app.ctx, false));
    let driveway = d.trip.driveway_mi().expect("this chain has a driveway");
    let yard = d.trip.gate_zone().expect("the yard is posted").clone();
    assert_eq!(yard.reason, "yard");
    assert_eq!(yard.limit_mph, FACILITY_GATE_LIMIT_MPH);
    assert!((yard.start_mi - driveway).abs() < 1e-9);
    // The street up to the driveway keeps its own number.
    let (street, reason) = d.trip.speed_limit_at(driveway - 0.02);
    assert_eq!(reason.as_deref(), Some("facility access road"));
    assert!(street > FACILITY_GATE_LIMIT_MPH, "{street}");

    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 30.0 / 2.23694;
    app.clear_speech();
    d.trip.position_mi = driveway - 0.05;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    assert!(
        lines_with(&app, "ate in").is_empty(),
        "the gate warned on the public street: {:?}",
        app.event_lines()
    );
    d.trip.position_mi = driveway + 0.01;
    d.trip.truck.velocity_mps = 20.0 / 2.23694;
    d.check_gate_approach_warning(&mut app.ctx, 0.016);
    assert_eq!(
        lines_with(&app, "ate in").len(),
        1,
        "{:?}",
        app.event_lines()
    );
}

#[test]
fn test_the_gate_warning_window_is_the_zone_plus_braking_room() {
    // On a synthetic approach the gate zone is the signed half mile, so the
    // warning lands where it always did at gate speed, and a faster truck
    // hears it earlier by exactly the distance its brakes need.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.destination_exit_taken = true;
    d.post_gate_zone(&app.ctx);
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = FACILITY_GATE_LIMIT_MPH / 2.23694;
    let at_gate_speed = d.gate_warning_window_mi();
    assert!((at_gate_speed - 0.5).abs() < 1e-6, "{at_gate_speed}");
    d.trip.truck.velocity_mps = 55.0 / 2.23694;
    let at_55 = d.gate_warning_window_mi();
    // (24.6 m/s)^2 - (6.7 m/s)^2 over twice 0.4 m/s^2 is about 700 meters.
    assert!(at_55 > 0.5 + 0.4 && at_55 < 0.5 + 0.5, "{at_55}");
}
