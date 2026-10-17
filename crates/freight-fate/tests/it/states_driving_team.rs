//! The co-driver driving whole loads through the real frame: the app's
//! own tick, the player's controls, and the screens the road opens.

use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};
use freight_fate::states::driving_team::TeamMode;

const DT: f64 = 1.0 / 60.0;

/// A quiet road: no traffic, hazards, inspections, patrols or weather, so
/// the case reads the co-driver and nothing else.
fn quiet_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|d, _| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_pressures.clear();
        d.trip.zones.retain(|z| z.aadt.is_none());
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.set_patrols(Vec::new());
        d.trip.posts.clear();
    });
    harness
}

/// Tick the app until `done` or `max_frames` pass. Returns whether `done`
/// came true; on a timeout, prints where the drive got to.
fn run_until(
    harness: &mut PlaytestHarness,
    max_frames: usize,
    mut done: impl FnMut(&PlaytestHarness) -> bool,
) -> bool {
    for frame in 0..max_frames {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if done(harness) {
            return true;
        }
        if frame % (60 * 60) == 0 && harness.state_is::<DrivingState>() {
            let (at, total, mph) = harness.read_drive(|d| {
                (
                    d.trip.position_mi,
                    d.trip.total_miles(),
                    d.truck().speed_mph(),
                )
            });
            eprintln!("frame {frame}: {at:.2}/{total:.2} mi at {mph:.0} mph");
        }
    }
    false
}

fn transcript_tail(harness: &PlaytestHarness) -> String {
    let lines = harness.transcript();
    let from = lines.len().saturating_sub(40);
    lines[from..].join("\n")
}

#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives the last miles of a load asleep")]
fn a_slept_shift_runs_ahead_to_the_receivers_gate_and_wakes_the_driver() {
    let mut harness = quiet_drive("Team Run");
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        let destination = d
            .destination_exit_stop(ctx)
            .expect("this route has a destination exit");
        d.trip.position_mi = (destination.at_mi - d.exit_window_mi() - 1.0).max(0.0);
        d.hand_wheel_to_co_driver(ctx, TeamMode::SleepThrough);
    });
    assert!(
        harness.app.ctx.co_driver_fast_forward,
        "a slept shift runs ahead"
    );
    let at_gate = run_until(&mut harness, 60 * 60 * 12, |h| {
        h.state_is::<FacilityArrivalState>()
    });
    assert!(
        at_gate,
        "the co-driver never reached the gate:\n{}",
        transcript_tail(&harness)
    );
    assert!(
        !harness.app.ctx.co_driver_away,
        "the wheel came back at the gate"
    );
    let said = harness.transcript().join("\n");
    assert!(said.contains("You slept"), "{said}");
    assert!(
        said.contains("Dan has the truck at the receiver's gate"),
        "{said}"
    );
    assert!(!harness.app.ctx.co_driver_fast_forward, "awake at the gate");
    harness.key(InputEvent::key(Key::Return));
    harness.finish_timed_state();
    assert!(harness.state_is::<ArrivalState>());
    let summary =
        harness.with_state::<ArrivalState, _>(|arrival, _| arrival.summary_parts.join(" "));
    assert!(summary.contains("Dan drove"), "{summary}");
}

#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives through a fuel stop")]
fn the_co_driver_fuels_at_a_stop_and_gets_back_on_the_road() {
    let mut harness = quiet_drive("Team Fuel");
    let found = harness.with_drive(|d, ctx| {
        let window = d.exit_window_mi();
        let stop = d
            .trip
            .stops
            .iter()
            .filter(|stop| {
                stop.actions.iter().any(|a| a == "fuel")
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
                    && stop.at_mi > window + 2.0
            })
            .map(|stop| stop.at_mi)
            .next();
        let Some(at_mi) = stop else {
            return false;
        };
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        let tank = d.truck().specs.fuel_tank_gal;
        d.truck_mut().fuel_gal = tank * 0.3;
        d.trip.position_mi = at_mi - window - 1.0;
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        true
    });
    assert!(found, "this route has a fuel stop");
    let back_on_road = run_until(&mut harness, 60 * 60 * 20, |h| {
        h.state_is::<DrivingState>()
            && h.read_drive(|d| d.truck().fuel_fraction() > 0.9 && d.truck().speed_mph() > 30.0)
    });
    assert!(
        back_on_road,
        "the co-driver never fuelled and rejoined the road:\n{}",
        transcript_tail(&harness)
    );
    assert!(
        harness.app.ctx.co_driver_away,
        "the co-driver still has the wheel"
    );
}

fn team_ready() -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Team Keys"));
    harness
}

#[test]
fn the_pause_menu_offers_a_co_driver_only_when_the_load_needs_a_sleep() {
    let mut harness = team_ready();
    harness.press_key(Key::Escape, None);
    let fresh = harness.menu_labels();
    assert!(
        !fresh.iter().any(|row| row.starts_with("Hand the wheel")),
        "a load one shift covers offers no co-driver: {fresh:?}"
    );
    harness.key(InputEvent::key(Key::Escape));
    // Most of a shift already driven: the rest of the load needs a sleep.
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(600.0);
    harness.press_key(Key::Escape, None);
    let rows = harness.menu_labels();
    assert!(
        rows.contains(&"Hand the wheel to Dan and ride along".to_string()),
        "{rows:?}"
    );
    assert!(
        rows.contains(&"Hand the wheel to Dan and sleep".to_string()),
        "{rows:?}"
    );
}

#[test]
fn handing_over_swaps_the_logbooks_and_the_assists_and_taking_back_restores_them() {
    let mut harness = team_ready();
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("realistic");
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(300.0);
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong));
    {
        let ctx = &harness.app.ctx;
        assert!(ctx.co_driver_away);
        assert_eq!(
            ctx.profile.as_ref().unwrap().hos.driving_min,
            0.0,
            "the co-driver's logbook"
        );
        assert!(ctx.settings.destination_approach_assist);
        assert!(ctx.settings.automatic_transmission);
        assert!(
            !ctx.settings.player_owned().destination_approach_assist,
            "the file keeps the player's own assists"
        );
    }
    let back = harness.with_drive(|d, ctx| d.take_wheel_back(ctx));
    assert!(back.is_some());
    let ctx = &harness.app.ctx;
    assert!(!ctx.co_driver_away);
    assert_eq!(ctx.profile.as_ref().unwrap().hos.driving_min, 300.0);
    assert!(!ctx.settings.destination_approach_assist);
}

#[test]
fn the_players_driving_keys_do_nothing_while_the_co_driver_drives() {
    let mut harness = team_ready();
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong));
    let engine_before = harness.read_drive(|d| d.truck().engine_on);
    harness.press_key(Key::E, Some('e'));
    assert_eq!(harness.read_drive(|d| d.truck().engine_on), engine_before);
    let parked_before = harness.read_drive(|d| d.truck().parking_brake);
    harness.press_key(Key::P, Some('p'));
    assert_eq!(
        harness.read_drive(|d| d.truck().parking_brake),
        parked_before
    );
}

#[test]
fn a_radio_left_off_stays_off_for_the_whole_shift() {
    let mut harness = team_ready();
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        if d.radio.enabled {
            d.toggle_radio(ctx);
        }
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
    });
    for _ in 0..60 * 60 * 5 {
        harness.advance_frame_clock();
        harness.app.tick(DT);
        if !harness.state_is::<DrivingState>() {
            break;
        }
    }
    assert!(harness.read_drive(|d| !d.radio.enabled));
}
