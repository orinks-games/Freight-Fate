//! The cycle restart on a team load (owner ruling 2026-11-28): time in the
//! sleeper berth while the co-driver drives is off duty, so it counts toward
//! the 34-hour restart (36 hours off under Canadian rules), the logbook off
//! the wheel follows the rules of the place the truck is in, and the
//! handback says when the cycle restarted.

use ff_core::sim::hos::{CycleEntry, HosRules};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::Menu;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::RestStopState;
use freight_fate::states::driving_team::TeamMode;

const RESTARTED: &str = "Your cycle restarted.";

/// Blaine to Surrey with the player's cycle spent (70 hours on it) under
/// `rules`, a worked shift, and the wheel handed to a rested co-driver.
fn handed_over(rules: HosRules, mode: TeamMode) -> PlaytestHarness {
    handed_over_with_cycle(rules, mode, 70.0)
}

fn handed_over_with_cycle(rules: HosRules, mode: TeamMode, cycle_h: f64) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_route(
        "blaine_wa_us",
        "surrey_bc_ca",
        RouteSetup::default().cities(&["blaine_wa_us", "surrey_bc_ca"]),
    );
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness.with_drive(move |d, ctx| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.weather.current = WeatherKind::Clear;
        d.job.deadline_game_h = d.trip.game_minutes / 60.0 + 200.0;
        let p = ctx.profile.as_mut().expect("a career");
        p.hos.set_rules(rules);
        p.hos.drive(300.0);
        p.hos.cycle_entries.clear();
        if cycle_h > 0.0 {
            p.hos.cycle_entries.push(CycleEntry {
                end_min: p.hos.cycle_clock_min,
                on_duty_min: cycle_h * 60.0,
            });
        }
        d.hand_wheel_to_co_driver(ctx, mode);
        // The co-driver's own logbook, the profile's while they drive,
        // follows the same rules.
        ctx.profile.as_mut().expect("a career").hos.set_rules(rules);
    });
    harness
}

/// `minutes` in the berth on the player's logbook while the co-driver drives.
fn berth(harness: &mut PlaytestHarness, minutes: f64) {
    harness.with_drive(move |d, _| {
        d.team
            .as_mut()
            .expect("a co-driver")
            .off_wheel_hos
            .sleeper(minutes);
    });
}

fn stop_rows(harness: &mut PlaytestHarness) -> Vec<String> {
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Example Truck Stop", at, "truck_stop");
    stop.actions = ["park", "sleep", "save"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.services = vec!["parking".to_string()];
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive"));
    let mut state = RestStopState::with_drive(handle, stop, false);
    let items = state.build_items(&mut harness.app.ctx);
    items
        .iter()
        .map(|item| item.text(&state, &harness.app.ctx))
        .collect()
}

#[test]
fn berth_hours_on_the_move_count_toward_the_restart_at_the_handback_stop() {
    for (rules, row) in [
        (
            HosRules::Us,
            "Rest 14 hours more to finish a 34-hour restart",
        ),
        (
            HosRules::CanadaSouth60,
            "Rest 16 hours more to finish 36 hours off",
        ),
    ] {
        let mut harness = handed_over(rules, TeamMode::SleepThrough);
        berth(&mut harness, 20.0 * 60.0);
        let log = harness
            .with_drive(|d, ctx| d.take_wheel_back(ctx))
            .expect("the shift's log");
        assert!(!log.cycle_restarted, "{rules:?}");
        let rows = stop_rows(&mut harness);
        assert!(rows.iter().any(|r| r == row), "{rules:?}: {rows:?}");
    }
}

#[test]
fn a_slept_shift_long_enough_restarts_the_cycle_and_says_so() {
    let mut harness = handed_over(HosRules::Us, TeamMode::SleepThrough);
    berth(&mut harness, 34.0 * 60.0);
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.co_driver_summon(ctx, "hours up"));
    let said = harness.transcript_text();
    assert!(said.contains(RESTARTED), "{said}");
    assert!(said.trim_end().ends_with(RESTARTED), "{said}");
    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .hos
            .cycle_used_min(),
        0.0
    );
}

#[test]
fn taking_the_wheel_back_from_a_ride_along_says_it_too() {
    let mut harness = handed_over(HosRules::Us, TeamMode::RideAlong);
    berth(&mut harness, 34.0 * 60.0);
    let log = harness
        .with_drive(|d, ctx| d.take_wheel_back(ctx))
        .expect("the shift's log");
    assert!(log.cycle_restarted);
}

#[test]
fn nothing_is_said_when_hours_do_not_count_or_the_cycle_was_clear() {
    let mut harness = handed_over(HosRules::Us, TeamMode::SleepThrough);
    harness.app.ctx.settings.hos_mode = "off".to_string();
    berth(&mut harness, 34.0 * 60.0);
    let log = harness
        .with_drive(|d, ctx| d.take_wheel_back(ctx))
        .expect("the shift's log");
    assert!(!log.cycle_restarted);
    drop(harness);

    // A cycle with nothing on it at the handover has nothing to restart.
    let mut harness = handed_over_with_cycle(HosRules::Us, TeamMode::SleepThrough, 0.0);
    berth(&mut harness, 34.0 * 60.0);
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.co_driver_summon(ctx, "hours up"));
    assert!(!harness.transcript_text().contains(RESTARTED));
}

#[test]
fn the_logbook_off_the_wheel_takes_the_rules_of_the_place_the_truck_is_in() {
    // Asleep while the co-driver crosses into Canada: the player's logbook
    // rests toward 36 hours off.
    let mut harness = handed_over(HosRules::Us, TeamMode::SleepThrough);
    let line = harness.read_drive(|d| {
        d.trip
            .border_booths
            .first()
            .map(|booth| booth.at_mi)
            .expect("the Pacific Highway booth")
    });
    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = line + 1.0;
        d.update_hours_and_fatigue_with_minutes(ctx, 1.0);
    });
    let player = harness.read_drive(|d| d.team.as_ref().expect("a co-driver").off_wheel_hos.rules);
    assert_eq!(player, HosRules::CanadaSouth60);
    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .hos
            .rules,
        HosRules::CanadaSouth60
    );

    // The player at the wheel back in the US: the co-driver's logbook off
    // the wheel follows them.
    harness.with_drive(|d, ctx| {
        d.take_wheel_back(ctx);
    });
    harness.with_drive(|d, ctx| {
        d.trip.position_mi = 0.2;
        d.update_hours_and_fatigue_with_minutes(ctx, 1.0);
    });
    let co_driver_rules =
        harness.read_drive(|d| d.team.as_ref().expect("a co-driver").off_wheel_hos.rules);
    assert_eq!(co_driver_rules, HosRules::Us);
}
