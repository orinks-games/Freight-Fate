//! Ramp-terminal signal pacing, clearance, and accessibility regressions.

use std::collections::BTreeSet;

use ff_core::data::world::get_world;
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::profile::Profile;
use ff_core::settings::TIME_SCALES;
use ff_core::sim::cross_traffic::{
    CrossTraffic, CrossVehicle, CONFLICT_WINDOW_FT, CROSS_BAR_MI, CROSS_CLASSES,
};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_pause_states::PauseMenuState;

use crate::transcript_cruise_support::{frame, hold, spoken, start_drive, DT};

fn a_drive_with_seed(app: &mut TestApp, seed: i64) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Signal timing", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let mut job = make_reposition_job(world, "Denver", "Cheyenne", false, None)
        .expect("Denver to Cheyenne is a supported reposition");
    job.bobtail = false;
    job.weight_tons = 20.0;
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("Denver to Cheyenne has a route");
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(seed),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

fn a_drive(app: &mut TestApp) -> DrivingState {
    a_drive_with_seed(app, 0)
}

fn stop(at_mi: f64) -> RoadStop {
    RoadStop::new("Test Plaza", at_mi, "travel_center")
}

fn stage_signal(drive: &mut DrivingState) {
    drive.ramp_mi = Some(RAMP_ACCESS_MI + 0.1);
    drive.ramp_stop = Some(stop(drive.trip.position_mi + 0.5));
    drive.ramp_control = "signal".to_string();
    drive.ramp_light_offset_s = 0.0;
    drive.ramp_light_timer = 0.0;
    drive.ramp_light_announced = true;
    drive.ramp_light_last_phase = "red".to_string();
    drive.ramp_terminal_done = false;
    drive.ramp_waiting_at_light = false;
}

#[test]
fn terminal_signals_keep_one_slow_varied_plan_per_seeded_intersection() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    let mut plans = BTreeSet::new();

    let exact_profiles: Vec<(i64, i64, i64)> = (0..RAMP_LIGHT_PROFILE_COUNT as u8)
        .map(|profile| {
            drive.ramp_light_profile = profile;
            (
                drive.ramp_light_red_s() as i64,
                drive.ramp_light_green_s() as i64,
                drive.ramp_light_cycle_s() as i64,
            )
        })
        .collect();
    assert_eq!(
        exact_profiles,
        vec![(30, 26, 62), (34, 28, 68), (38, 30, 74), (42, 32, 80)]
    );
    // The MUTCD ceiling: a yellow a truck driver learns of by ear has to
    // outlast its own spoken call (owner, exit 286A, 2026-09-23).
    assert_eq!(RAMP_LIGHT_YELLOW_S, 6.0);

    for mile in 20..36 {
        let terminal = stop(f64::from(mile));
        drive.begin_ramp_terminal(&app.ctx, &terminal);
        let first = (
            drive.ramp_light_red_s(),
            drive.ramp_light_green_s(),
            drive.ramp_light_cycle_s(),
            drive.ramp_light_offset_s,
        );
        drive.begin_ramp_terminal(&app.ctx, &terminal);
        let second = (
            drive.ramp_light_red_s(),
            drive.ramp_light_green_s(),
            drive.ramp_light_cycle_s(),
            drive.ramp_light_offset_s,
        );

        assert_eq!(first, second, "mile {mile} changed its timing plan");
        assert!((30.0..=42.0).contains(&first.0), "red {}", first.0);
        assert!((26.0..=32.0).contains(&first.1), "green {}", first.1);
        assert!((62.0..=80.0).contains(&first.2), "cycle {}", first.2);
        assert!(first.3 >= 0.0 && first.3 < first.2, "offset {}", first.3);
        plans.insert((first.0 as i64, first.1 as i64));
    }

    assert!(plans.len() > 1, "seeded intersections all used {plans:?}");

    let terminal = stop(27.0);
    drive.begin_ramp_terminal(&app.ctx, &terminal);
    let plan = (drive.ramp_light_red_s(), drive.ramp_light_green_s());
    drop(drive);
    drop(app);

    let mut another_app = TestApp::new();
    let mut another_drive = a_drive_with_seed(&mut another_app, 98_765);
    another_drive.begin_ramp_terminal(&another_app.ctx, &terminal);
    assert_eq!(
        plan,
        (
            another_drive.ramp_light_red_s(),
            another_drive.ramp_light_green_s()
        ),
        "a later drive changed this intersection's timing plan"
    );
}

#[test]
fn an_initial_green_is_cleared_before_it_is_announced() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    let mut found_green = false;

    for mile in 20..200 {
        drive.begin_ramp_terminal(&app.ctx, &stop(f64::from(mile)));
        if drive.ramp_control == "signal" && drive.ramp_light_phase() == "green" {
            found_green = true;
            let bubble = drive
                .cross_bubble
                .as_ref()
                .expect("signals have cross traffic");
            assert!(
                bubble.player_has_green,
                "the cross street did not receive its stop before the first player green"
            );
            assert!(
                !bubble.vehicles.iter().any(|vehicle| {
                    vehicle.committed && vehicle.position_mi < CONFLICT_WINDOW_FT / 5280.0
                }),
                "the first player green began with committed traffic in the conflict window: {:?}",
                bubble.vehicles
            );
            break;
        }
    }

    assert!(found_green, "fixture found no signal beginning off red");
}

#[test]
fn phase_boundaries_keep_a_loaded_departure_window_and_fixed_yellow() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    stage_signal(&mut drive);
    let red = drive.ramp_light_red_s();
    let green = drive.ramp_light_green_s();
    let cycle = drive.ramp_light_cycle_s();
    assert!(drive.trip.truck.cargo_kg > 0.0, "fixture must be loaded");

    drive.ramp_light_timer = red - 0.01;
    assert_eq!(drive.ramp_light_phase(), "red");
    drive.ramp_light_timer = red;
    assert_eq!(drive.ramp_light_phase(), "green");
    drive.ramp_light_timer = red + 20.0;
    assert_eq!(
        drive.ramp_light_phase(),
        "green",
        "a loaded truck needs a useful departure window"
    );
    drive.ramp_light_timer = red + green;
    assert_eq!(drive.ramp_light_phase(), "yellow");
    drive.ramp_light_timer = cycle - 0.01;
    assert_eq!(drive.ramp_light_phase(), "yellow");
    drive.ramp_light_timer = cycle;
    assert_eq!(drive.ramp_light_phase(), "red");
}

#[test]
fn cross_traffic_is_stopped_for_clearance_before_the_players_green() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    stage_signal(&mut drive);
    drive.cross_bubble = Some(CrossTraffic::new(17, "signal", true));
    let red = drive.ramp_light_red_s();

    drive.ramp_light_timer = red - RAMP_LIGHT_RED_CLEARANCE_S - 0.05;
    drive.update_ramp_light(&mut app.ctx, 0.1);
    assert_eq!(drive.ramp_light_phase(), "red");
    assert!(
        drive
            .cross_bubble
            .as_ref()
            .expect("the terminal is still live")
            .player_has_green,
        "both approaches should be held during the red clearance interval"
    );

    drive.ramp_light_timer = red - 0.05;
    drive.update_ramp_light(&mut app.ctx, 0.1);
    assert_eq!(drive.ramp_light_phase(), "green");
    assert!(
        drive
            .cross_bubble
            .as_ref()
            .expect("the terminal is still live")
            .player_has_green,
        "cross traffic must see its stop in the same frame green begins"
    );
}

#[test]
fn red_clearance_empties_the_conflict_window_before_green() {
    for &(class, _, length_ft) in &CROSS_CLASSES {
        for position_mi in [CROSS_BAR_MI, CROSS_BAR_MI - length_ft / 10_560.0] {
            let mut app = TestApp::new();
            let mut drive = a_drive(&mut app);
            stage_signal(&mut drive);
            let mut bubble = CrossTraffic::new(17, "signal", true);
            bubble.vehicles.clear();
            bubble.vehicles.push(CrossVehicle {
                position_mi,
                speed_mph: 0.0,
                target_mph: 20.0,
                vehicle_class: class,
                length_mi: length_ft / 5280.0,
                from_side: "left",
                crossed: false,
                committed: false,
                sound_started: false,
                id: 1,
            });
            drive.cross_bubble = Some(bubble);
            drive.ramp_light_timer = drive.ramp_light_red_s() - RAMP_LIGHT_RED_CLEARANCE_S;

            for _ in 0..(RAMP_LIGHT_RED_CLEARANCE_S / 0.25) as usize {
                drive.update_ramp_light(&mut app.ctx, 0.25);
            }

            assert_eq!(drive.ramp_light_phase(), "green", "class {class}");
            assert!(
                !drive
                    .cross_bubble
                    .as_ref()
                    .expect("the terminal is still live")
                    .occupied(),
                "{class} from {position_mi} remained in the conflict window at green"
            );
        }
    }
}

#[test]
fn crossing_before_and_after_the_yellow_red_boundary_uses_the_spoken_phase() {
    let mut yellow_app = TestApp::new();
    let mut yellow = a_drive(&mut yellow_app);
    stage_signal(&mut yellow);
    yellow.departure_checked = true;
    yellow.trip.truck.start_engine();
    yellow.trip.truck.set_air_ready(false);
    yellow.ramp_mi = Some(RAMP_ACCESS_MI - RAMP_TERMINAL_GRACE_MI + 1.0 / 5280.0);
    yellow.trip.truck.velocity_mps = 30.0 / 2.2369362920544;
    yellow.cross_bubble = None;
    yellow.ramp_light_timer = yellow.ramp_light_cycle_s() - 0.2;
    yellow.ramp_light_last_phase = "yellow".to_string();
    assert_eq!(yellow.ramp_light_phase(), "yellow");

    yellow.update_frame(&mut yellow_app.ctx, 0.1);

    assert!(yellow.ramp_terminal_done);
    assert!(
        !yellow_app
            .speech()
            .lines()
            .iter()
            .any(|line| line.contains("red light")),
        "a yellow entry was judged as a red-light run"
    );
    drop(yellow);
    drop(yellow_app);

    let mut red_app = TestApp::new();
    let mut red = a_drive(&mut red_app);
    stage_signal(&mut red);
    red.departure_checked = true;
    red.trip.truck.start_engine();
    red.trip.truck.set_air_ready(false);
    red.ramp_mi = Some(RAMP_ACCESS_MI - RAMP_TERMINAL_GRACE_MI + 1.0 / 5280.0);
    red.trip.truck.velocity_mps = 30.0 / 2.2369362920544;
    red.cross_bubble = None;
    red.ramp_light_timer = red.ramp_light_cycle_s() - 0.05;
    red.ramp_light_last_phase = "yellow".to_string();

    red.update_frame(&mut red_app.ctx, 0.1);

    assert!(red.ramp_terminal_done);
    let lines = red_app.speech().lines();
    let turned = lines.iter().position(|line| line.contains("Light red"));
    let judged = lines.iter().position(|line| line.contains("red light"));
    assert!(
        turned.is_some() && judged.is_some() && turned < judged,
        "red was not spoken before the crossing was judged a red-light run: {lines:?}"
    );
}

#[test]
fn signal_cycles_use_real_seconds_at_every_supported_pacing() {
    for scale in TIME_SCALES {
        let mut app = TestApp::new();
        app.ctx.settings.time_scale = scale;
        let mut drive = a_drive(&mut app);
        stage_signal(&mut drive);
        drive.departure_checked = true;
        let red = drive.ramp_light_red_s();

        for _ in 0..((red * 10.0) as usize - 1) {
            drive.update_frame(&mut app.ctx, 0.1);
        }
        assert_eq!(drive.ramp_light_phase(), "red", "time scale {scale}");
        drive.update_frame(&mut app.ctx, 0.2);
        assert_eq!(drive.ramp_light_phase(), "green", "time scale {scale}");
    }
}

#[test]
fn opening_the_pause_menu_freezes_the_signal_clock() {
    let mut harness: PlaytestHarness = start_drive("Signal pause");
    harness.with_drive(|drive, _| {
        stage_signal(drive);
        drive.ramp_light_timer = 5.0;
        drive.trip.truck.velocity_mps = 0.0;
    });

    harness.key(InputEvent::key(Key::Escape));
    assert!(harness.state_is::<PauseMenuState>());
    harness.app.tick(10.0);
    assert_eq!(harness.read_drive(|drive| drive.ramp_light_timer), 5.0);

    harness.key(InputEvent::key(Key::Escape));
    assert!(!harness.state_is::<PauseMenuState>());
    harness.app.tick(0.5);
    assert_eq!(harness.read_drive(|drive| drive.ramp_light_timer), 5.5);
}

#[test]
fn route_transition_assistance_slows_a_green_arrival_to_rolling_speed() {
    // Agent drive, Eagles Landing, 2026-09-22: the light turned green half a
    // mile up the ramp at 50 mph, the assist only lifted, and the truck went
    // through "far too fast" with the assist on. Hands off, it now meets the
    // bar at rolling speed and says once that it is slowing.
    let mut harness: PlaytestHarness = start_drive("Green arrival");
    harness.prepare_for_driving(0.0);
    harness.app.ctx.settings.route_transition_assist = true;
    harness.with_drive(|drive, _| {
        stage_signal(drive);
        drive.ramp_mi = Some(RAMP_ACCESS_MI + 0.4);
        drive.ramp_light_last_phase = "green".to_string();
        drive.trip.truck.velocity_mps = 50.0 / 2.23694;
    });
    harness.clear_speech();

    let mut crossing_mph = None;
    for _ in 0..(90.0 / DT) as usize {
        // Hold the green: this pins the arrival, not the light's timing.
        harness.with_drive(|drive, _| {
            drive.ramp_light_offset_s = drive.ramp_light_red_s() + 1.0;
            drive.ramp_light_timer = 0.0;
        });
        let speed = harness.read_drive(|drive| drive.trip.truck.speed_mph());
        frame(&mut harness, DT);
        if harness.read_drive(|drive| drive.ramp_terminal_done) {
            crossing_mph = Some(speed);
            break;
        }
    }

    let lines = spoken(&harness);
    let crossing_mph = crossing_mph.expect("the truck never reached the bar");
    assert!(
        crossing_mph <= GREEN_ROLL_MPH,
        "crossed the green at {crossing_mph:.1} mph: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("far too fast")),
        "{lines:?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("Slowing for the green light"))
            .count(),
        1,
        "{lines:?}"
    );
}

#[test]
fn an_assisted_red_stop_is_never_called_short_of_the_light() {
    // Agent drive, 2026-09-22: stopped by the assist at the red, the cab said
    // "Stopped short of the light" on both sides of the assist's own
    // "Stopped at the red light" (now "Red light."). Inside the hold window the assist owns it.
    let mut harness: PlaytestHarness = start_drive("Assisted red");
    harness.prepare_for_driving(0.0);
    harness.app.ctx.settings.route_transition_assist = true;
    harness.with_drive(|drive, _| {
        stage_signal(drive);
        drive.ramp_mi = Some(RAMP_ACCESS_MI + RAMP_ASSIST_HOLD_MI * 0.5);
        drive.trip.truck.velocity_mps = 0.0;
    });
    harness.clear_speech();

    for _ in 0..(3.0 / DT) as usize {
        frame(&mut harness, DT);
    }

    let lines = spoken(&harness);
    assert!(
        lines.iter().any(|line| line.contains("Red light.")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("short of the light")),
        "{lines:?}"
    );
}

#[test]
fn a_loaded_truck_departs_on_the_shortest_green_with_manual_acceleration() {
    let mut harness: PlaytestHarness = start_drive("Loaded green");
    harness.prepare_for_driving(0.0);
    harness.app.ctx.settings.route_transition_assist = true;
    harness.with_drive(|drive, _| {
        assert!(drive.trip.truck.cargo_kg > 0.0, "fixture must be loaded");
        stage_signal(drive);
        drive.ramp_light_profile = 0;
        drive.ramp_light_offset_s = drive.ramp_light_red_s() - 0.05;
        drive.ramp_mi = Some(RAMP_ACCESS_MI + 20.0 / 5280.0);
        drive.ramp_assist_brake = 0.4;
    });
    harness.clear_speech();

    frame(&mut harness, 0.1);
    assert_eq!(
        harness.read_drive(|drive| drive.ramp_light_phase()),
        "green"
    );
    hold(&mut harness, &[Key::Up]);
    for _ in 0..(RAMP_LIGHT_GREEN_S / DT) as usize {
        if harness.read_drive(|drive| drive.ramp_terminal_done) {
            break;
        }
        frame(&mut harness, DT);
    }

    assert!(
        harness.read_drive(|drive| drive.ramp_terminal_done),
        "the loaded truck did not clear the terminal inside the shortest green"
    );
    assert_eq!(harness.read_drive(|drive| drive.trip.truck.damage_pct), 0.0);
    assert!(
        spoken(&harness)
            .iter()
            .any(|line| line.contains("Light green")),
        "green transition was not delivered: {:?}",
        spoken(&harness)
    );
}
