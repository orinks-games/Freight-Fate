//! Badges earned at a stop land on the row that earns them: the break
//! taken, the sleep committed, the repair or the fuel paid for, the trailer
//! dropped at the receiver. Each screen sits over a live drive on the
//! playtest harness, the way the game opens it.

use ff_core::models::trailer_yard::preloaded_trailer;
use ff_core::sim::hos::FATIGUE_SEVERE;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::{DriveRef, FacilityArrivalState};
use freight_fate::states::driving_rest_states::RestStopState;
use serde_json::json;

/// A quiet delivery drive on the harness.
fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
    });
    harness.clear_speech();
    harness
}

fn holds(harness: &PlaytestHarness, id: &str) -> bool {
    harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements
        .iter()
        .any(|a| a == id)
}

/// A stop right where the truck is, offering `actions`, its screen open.
fn stop_here(harness: &mut PlaytestHarness, actions: &[&str]) -> RestStopState {
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Test Travel Center", at, "travel_center");
    stop.actions = actions.iter().map(|a| a.to_string()).collect();
    stop.services = vec!["parking".to_string(), "diesel".to_string()];
    stop.parking = "confirmed".to_string();
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
    RestStopState::with_drive(handle, stop, false)
}

/// Activate the row whose label starts with `label`.
fn activate(state: &mut RestStopState, harness: &mut PlaytestHarness, label: &str) {
    let items = state.build_items(&mut harness.app.ctx);
    let found = items
        .iter()
        .find(|item| item.text(state, &harness.app.ctx).starts_with(label))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "no {label:?} row: {:?}",
                items
                    .iter()
                    .map(|item| item.text(state, &harness.app.ctx))
                    .collect::<Vec<_>>()
            )
        });
    (found.action)(state, &mut harness.app.ctx);
}

/// Hours on the clock and `fatigue`, so a sleep has something to reset.
fn tired(harness: &mut PlaytestHarness, fatigue: f64) {
    let profile = harness.app.ctx.profile.as_mut().expect("a career");
    profile.hos.driving_min = 600.0;
    profile.hos.duty_min = 660.0;
    profile.fatigue = fatigue;
}

// -- rest ----------------------------------------------------------------------------

#[test]
fn break_taken_lands_on_the_thirty_minute_break() {
    let mut harness = a_drive("Break Badge");
    let mut stop = stop_here(&mut harness, &["park", "food", "break"]);
    // Coffee eases fatigue but is not the break the rule asks for.
    activate(&mut stop, &mut harness, "Food and coffee break");
    assert!(!holds(&harness, "break_taken"));
    activate(&mut stop, &mut harness, "Take a 30-minute break");
    assert!(holds(&harness, "break_taken"));
}

#[test]
fn coffee_regular_lands_on_the_twenty_fifth_proper_break() {
    let mut harness = a_drive("Coffee Badge");
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .achievement_stats
        .insert("breaks_taken".to_string(), json!(23));
    let mut stop = stop_here(&mut harness, &["park", "break"]);
    activate(&mut stop, &mut harness, "Take a 30-minute break");
    assert!(!holds(&harness, "coffee_regular"), "twenty-four is not it");
    activate(&mut stop, &mut harness, "Take a 30-minute break");
    assert!(holds(&harness, "coffee_regular"));
}

#[test]
fn sleep_before_exhaustion_lands_on_a_sleep_taken_before_severe_fatigue() {
    let mut harness = a_drive("Sleep Badge");
    // Asleep on your feet: the sleep counts, this badge does not.
    tired(&mut harness, FATIGUE_SEVERE + 5.0);
    let mut stop = stop_here(&mut harness, &["park", "sleep"]);
    activate(&mut stop, &mut harness, "Sleep 10 hours");
    activate(&mut stop, &mut harness, "Sleep 10 hours");
    assert!(holds(&harness, "slept_on_route"));
    assert!(!holds(&harness, "sleep_before_exhaustion"));

    tired(&mut harness, FATIGUE_SEVERE - 20.0);
    let mut stop = stop_here(&mut harness, &["park", "sleep"]);
    // The first press only previews the rest.
    activate(&mut stop, &mut harness, "Sleep 10 hours");
    assert!(!holds(&harness, "sleep_before_exhaustion"));
    activate(&mut stop, &mut harness, "Sleep 10 hours");
    assert!(holds(&harness, "sleep_before_exhaustion"));
}

/// Five-by-Two is a full ten hours asleep in the lot. Finishing a reset that
/// a break already started, or sleeping a split, is less than that.
#[test]
fn five_by_two_needs_the_full_ten_hours_asleep() {
    let mut harness = a_drive("Bunk Badge");
    tired(&mut harness, FATIGUE_SEVERE - 20.0);
    let mut stop = stop_here(&mut harness, &["park", "break", "sleep"]);

    activate(&mut stop, &mut harness, "Sleep 8 hours in sleeper berth");
    activate(&mut stop, &mut harness, "Sleep 8 hours in sleeper berth");
    assert!(
        !holds(&harness, "slept_on_route"),
        "a split is not the night"
    );

    tired(&mut harness, FATIGUE_SEVERE - 20.0);
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(30.0);
    let mut stop = stop_here(&mut harness, &["park", "break", "sleep"]);
    activate(&mut stop, &mut harness, "Take a 30-minute break");
    activate(&mut stop, &mut harness, "Sleep 9.5 hours more");
    activate(&mut stop, &mut harness, "Sleep 9.5 hours more");
    assert!(
        !holds(&harness, "slept_on_route"),
        "nine and a half hours after a break is not ten in the bunk"
    );

    tired(&mut harness, FATIGUE_SEVERE - 20.0);
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(30.0);
    let mut stop = stop_here(&mut harness, &["park", "sleep"]);
    activate(&mut stop, &mut harness, "Sleep 10 hours");
    activate(&mut stop, &mut harness, "Sleep 10 hours");
    assert!(holds(&harness, "slept_on_route"));
}

// -- repairs and fuel ------------------------------------------------------------------

fn damage(harness: &mut PlaytestHarness, pct: f64) {
    harness.with_drive(move |d, _| d.trip.truck.damage_pct = pct);
}

#[test]
fn garage_repair_lands_on_a_shop_repair_and_deep_repair_needs_seventy_five() {
    let mut harness = a_drive("Shop Badge");
    let mut stop = stop_here(&mut harness, &["park", "repair"]);
    activate(&mut stop, &mut harness, "Use repair service");
    assert!(!holds(&harness, "garage_repair"), "nothing to repair");
    damage(&mut harness, 70.0);
    activate(&mut stop, &mut harness, "Use repair service");
    assert!(holds(&harness, "garage_repair"));
    assert!(!holds(&harness, "deep_repair"), "seventy is not deep");
    damage(&mut harness, 80.0);
    activate(&mut stop, &mut harness, "Use repair service");
    assert!(holds(&harness, "deep_repair"));
}

#[test]
fn roadside_fix_lands_when_the_mechanic_patches_the_truck() {
    let mut harness = a_drive("Roadside Badge");
    let mut stop = stop_here(&mut harness, &["park", "roadside_assistance"]);
    activate(&mut stop, &mut harness, "Call roadside assistance");
    assert!(!holds(&harness, "roadside_fix"), "nothing to patch");
    damage(&mut harness, 40.0);
    activate(&mut stop, &mut harness, "Call roadside assistance");
    assert!(holds(&harness, "roadside_fix"));
}

#[test]
fn route_refuel_lands_on_fuel_bought_on_the_road() {
    let mut harness = a_drive("Fuel Badge");
    harness.with_drive(|d, _| {
        d.trip.truck.stop_engine();
        d.trip.truck.fuel_gal = d.trip.truck.specs.fuel_tank_gal;
    });
    let mut stop = stop_here(&mut harness, &["park", "fuel"]);
    activate(&mut stop, &mut harness, "Fuel: tank is full");
    assert!(!holds(&harness, "route_refuel"));
    harness.with_drive(|d, _| d.trip.truck.fuel_gal = d.trip.truck.specs.fuel_tank_gal * 0.3);
    activate(&mut stop, &mut harness, "Refuel");
    assert!(holds(&harness, "route_refuel"));
}

// -- the receiver's drop yard ------------------------------------------------------------

/// Point the drive's load at a drop-yard receiver, hauling a preloaded
/// trailer from the origin yard that does (or does not) carry a write-up.
fn drop_yard_run(harness: &mut PlaytestHarness, defective: bool, refused: bool) {
    harness.with_drive(move |d, _| {
        d.job.destination_type = "cross_dock".to_string();
        d.job.origin_type = "cross_dock".to_string();
        let found = (0..400).find_map(|i| {
            d.job.origin_facility_id = format!("badge-cross-dock-{i}");
            preloaded_trailer(&d.job)
                .filter(|t| t.defect().is_some() == defective)
                .map(|_| ())
        });
        assert!(found.is_some(), "no yard staged a {defective} trailer");
        d.trailer_refused = refused;
    });
}

/// Pull up to the receiver, open its menu, and drop the trailer.
fn drop_at_the_receiver(harness: &mut PlaytestHarness) {
    harness.with_drive(|d, _| {
        d.destination_exit_taken = true;
        d.trip.finished = true;
        d.trip.position_mi = d.trip.total_miles();
        d.truck_mut().velocity_mps = 0.0;
        d.truck_mut().set_parking_brake();
    });
    harness.advance_clock(1.0 / 60.0);
    harness.with_drive(|d, ctx| d.update_frame(ctx, 1.0 / 60.0));
    harness.key(key_event(Key::Down, None));
    harness.finish_timed_state();
    assert!(harness.state_is::<FacilityArrivalState>());
    assert!(!holds(harness, "first_delivery_drop"), "before the drop");
    harness.select_menu_item("Drop the loaded trailer and hook an empty");
    harness.finish_timed_state();
    assert!(!harness.state_is::<DrivingState>());
}

#[test]
fn first_delivery_drop_lands_when_the_receiver_keeps_the_trailer() {
    let mut harness = a_drive("Drop Badge");
    drop_yard_run(&mut harness, false, false);
    drop_at_the_receiver(&mut harness);
    assert!(holds(&harness, "first_delivery_drop"));
    assert!(
        !holds(&harness, "dropped_the_bad_one"),
        "the trailer was sound"
    );
}

#[test]
fn dropped_the_bad_one_lands_when_the_written_up_trailer_is_left_behind() {
    let mut harness = a_drive("Bad Drop Badge");
    drop_yard_run(&mut harness, true, false);
    drop_at_the_receiver(&mut harness);
    assert!(holds(&harness, "dropped_the_bad_one"));
}

#[test]
fn a_trailer_refused_at_the_shipper_is_not_the_one_dropped() {
    // The yard swapped the bad box at pickup, so the trailer left in the
    // receiver's yard is the sound one.
    let mut harness = a_drive("Refused Drop Badge");
    drop_yard_run(&mut harness, true, true);
    drop_at_the_receiver(&mut harness);
    assert!(holds(&harness, "first_delivery_drop"));
    assert!(!holds(&harness, "dropped_the_bad_one"));
}

#[test]
fn the_drop_badges_stay_off_a_live_unload() {
    let mut harness = a_drive("Dock Badge");
    harness.with_drive(|d, _| d.job.destination_type = "mine_quarry".to_string());
    harness.with_drive(|d, _| {
        d.destination_exit_taken = true;
        d.trip.finished = true;
        d.trip.position_mi = d.trip.total_miles();
        d.truck_mut().velocity_mps = 0.0;
        d.truck_mut().set_parking_brake();
    });
    harness.advance_clock(1.0 / 60.0);
    harness.with_drive(|d, ctx| d.update_frame(ctx, 1.0 / 60.0));
    harness.key(key_event(Key::Down, None));
    harness.finish_timed_state();
    harness.select_menu_item("Dock and deliver");
    harness.finish_timed_state();
    assert!(!holds(&harness, "first_delivery_drop"));
}
