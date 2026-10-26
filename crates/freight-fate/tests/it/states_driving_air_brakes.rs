//! The air system at the wheel: the cold-start sequence, the low-air and
//! spring-brake warnings, the lane-departure call that shares their interrupt
//! guarantee, the pull-over that replaced the old speeding strike, and pulling
//! the parking valve at speed.
//!
//! Ported from `tests/test_driving_features.py` (the air-brake block from
//! `test_terse_air_brake_startup_omits_control_instructions` through
//! `test_a_pull_over_flushes_event_voice`, plus
//! `test_setting_the_parking_brake_at_speed_dynamites_the_brakes`).
//!
//! Two Python seams have no Rust equivalent and are arranged for real here:
//! `monkeypatch.setattr(pygame.key, "get_pressed", ...)` is `ctx.input.press`,
//! the same held-key store the shipped input loop writes; and the pair
//! `monkeypatch.setattr(driving.lane, "update"/"describe", ...)` is replaced
//! by putting the truck genuinely off the pavement and letting the real lane
//! model time out its own grace period -- which also means the line under
//! test is the real `LaneKeeping::describe`, not the invented "Right rumble
//! strip." the Python stub returned.

use ff_core::sim::enforcement_observe::OBSERVE_HOLD_MI;
use ff_core::sim::enforcement_posts::{method_by_kind, EnforcementPost, KIND_MEDIAN};
use ff_core::sim::lane::MAX_OFFSET;
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Mods};

const DT: f64 = 1.0 / 60.0;
const MPH_PER_MPS: f64 = 2.2369362920544;

// -- rigging -------------------------------------------------------------------------

/// `start_drive(app)` plus `quiet_trip(driving)`: a loaded delivery on an
/// empty road under a clear sky, with the walkthrough already finished.
fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|drive, _| {
        // First-run guidance ignores verbosity (R15), so a terse case has to
        // silence the walkthrough the honest way: this driver has finished it.
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        // Congestion zones re-inject slow NPC traffic when entered.
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
    });
    harness.clear_speech();
    harness
}

/// One plain frame of the drive: no harness throttle policy, just the clock
/// and `update_frame`, so what the truck does is what the held keys ask for.
fn frame(harness: &mut PlaytestHarness) {
    harness.advance_clock(DT);
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, DT));
}

fn press(harness: &mut PlaytestHarness, key: Key) {
    harness.with_drive(move |drive, ctx| drive.handle_key_event(ctx, &key_event(key, None)));
}

fn main_lines(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.main_lines()
}

fn event_calls(harness: &PlaytestHarness) -> Vec<(String, bool)> {
    harness.app.event_calls()
}

// -- the cold start -------------------------------------------------------------------

#[test]
fn test_terse_air_brake_startup_omits_control_instructions() {
    let mut harness = a_drive("Terse Air");
    harness.app.ctx.settings.driving_speech = "quiet".to_string();
    harness.with_drive(|drive, _| drive.truck_mut().set_cold_air_start());
    harness.app.ctx.input.press(Key::Up, Mods::NONE);
    harness.clear_speech();

    press(&mut harness, Key::E);
    for _ in 0..60 {
        frame(&mut harness);
    }

    let spoken = main_lines(&harness);
    assert!(
        spoken
            .last()
            .expect("the start-up says something")
            .ends_with("Air 55 psi."),
        "{spoken:#?}"
    );
    let event_texts: Vec<String> = event_calls(&harness).into_iter().map(|(t, _)| t).collect();
    assert!(
        event_texts.iter().any(|t| t == "Air pressure 55 psi."),
        "{event_texts:#?}"
    );
    assert!(
        event_texts.iter().all(|t| !t.contains("Wait for")),
        "{event_texts:#?}"
    );
    assert!(
        event_texts.iter().all(|t| !t.contains("press P")),
        "{event_texts:#?}"
    );

    press(&mut harness, Key::P);
    // The exact psi depends on how much rpm the first second of running
    // banked (shift timing shifts it by one); terseness is the assertion.
    let last = main_lines(&harness).last().cloned().unwrap_or_default();
    assert!(
        last.starts_with("Parking brake set. Air pressure"),
        "{last}"
    );
    assert!(last.ends_with("psi."), "{last}");
    assert!(!last.to_lowercase().contains("wait for"), "{last}");

    for _ in 0..(60 * 15) {
        frame(&mut harness);
        if harness.read_drive(|d| d.truck().air_ready()) {
            break;
        }
    }

    let last_event = event_calls(&harness)
        .last()
        .cloned()
        .expect("the air-ready cue speaks");
    assert_eq!(last_event.0, "Air ready.", "{last_event:?}");
    assert!(!last_event.0.contains("Press P"), "{last_event:?}");
}

#[test]
fn test_air_brake_startup_blocks_movement_until_ready_and_released() {
    let mut harness = a_drive("Air Start");
    let log = harness.app.record_audio();
    harness.with_drive(|drive, _| drive.truck_mut().set_cold_air_start());
    harness.app.ctx.input.press(Key::Up, Mods::NONE);
    harness.clear_speech();

    press(&mut harness, Key::E);
    for _ in 0..60 {
        frame(&mut harness);
    }

    assert_eq!(harness.read_drive(|d| d.truck().speed_mph()), 0.0);
    assert!(harness.read_drive(|d| d.truck().parking_brake));
    assert!(
        event_calls(&harness)
            .iter()
            .any(|(t, _)| t.starts_with("Air pressure")),
        "{:#?}",
        event_calls(&harness)
    );

    press(&mut harness, Key::P);
    assert!(harness.read_drive(|d| d.truck().parking_brake));
    let last = main_lines(&harness).last().cloned().unwrap_or_default();
    assert!(last.starts_with("Parking brake set. Air "), "{last}");

    for _ in 0..(60 * 15) {
        frame(&mut harness);
        if harness.read_drive(|d| d.truck().air_ready()) {
            break;
        }
    }

    assert!(harness.read_drive(|d| d.truck().air_ready()));
    assert!(
        event_calls(&harness).iter().any(|(t, _)| t == "Air ready."),
        "{:#?}",
        event_calls(&harness)
    );
    // The compressor-ready cue is a real air-dryer purge, not a UI beep.
    assert!(
        log.borrow()
            .played
            .iter()
            .any(|(key, _, _)| key == "vehicle/air_dryer_purge"),
        "{:#?}",
        log.borrow().played
    );

    press(&mut harness, Key::P);
    assert!(!harness.read_drive(|d| d.truck().parking_brake));
    assert!(
        log.borrow()
            .played
            .iter()
            .any(|(key, volume, _)| key == "vehicle/brake_release" && *volume == 0.65),
        "{:#?}",
        log.borrow().played
    );

    for _ in 0..(60 * 5) {
        frame(&mut harness);
        if harness.read_drive(|d| d.truck().speed_mph()) > 1.0 {
            break;
        }
    }
    assert!(harness.read_drive(|d| d.truck().speed_mph()) > 1.0);

    press(&mut harness, Key::P);
    assert!(harness.read_drive(|d| d.truck().parking_brake));
    assert!(
        log.borrow()
            .played
            .iter()
            .any(|(key, volume, _)| key == "vehicle/brake_set" && *volume == 0.65),
        "{:#?}",
        log.borrow().played
    );
}

// -- low air --------------------------------------------------------------------------

#[test]
fn test_low_air_warning_flushes_event_voice() {
    let mut harness = a_drive("Low Air");
    harness.with_drive(|drive, ctx| {
        let truck = drive.truck_mut();
        truck.set_cold_air_start();
        truck.engine_on = true;
        truck.set_air_pressure_psi(50.0);
        // A fresh episode: the cold-start narration latch is for the
        // start-of-drive announcement, and this case pins the flush of a NEW
        // warning.
        drive.low_air_said = false;
        drive.update_air_brake_announcements(ctx, true, false, false, false);
    });

    let last = event_calls(&harness)
        .last()
        .cloned()
        .expect("the low-air warning speaks");
    assert!(last.0.starts_with("Low air warning"), "{last:?}");
    assert!(last.1, "the low-air warning has to flush the event voice");
}

// -- lane departure -------------------------------------------------------------------

/// Put the truck genuinely off the right-hand edge at highway speed and run
/// frames until the lane model's own off-road grace period fires the warning.
fn drift_off_the_pavement(harness: &mut PlaytestHarness) {
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    harness.with_drive(|drive, _| {
        drive.truck_mut().start_engine();
        drive.truck_mut().velocity_mps = 20.0;
        drive.lane.lane = 0; // the right-hand lane: past its edge is the shoulder
        drive.lane.offset = MAX_OFFSET;
    });
    for _ in 0..(60 * 4) {
        harness.advance_clock(DT);
        let fired = harness.with_drive(|drive, ctx| {
            drive.truck_mut().velocity_mps = 20.0;
            drive.lane.offset = MAX_OFFSET;
            drive.update_lane(ctx, DT);
            drive.road_position_band.is_some()
        });
        if fired {
            return;
        }
    }
    panic!("the truck never registered as off the pavement");
}

#[test]
fn test_terse_lane_departure_omits_recovery_instruction() {
    let mut harness = a_drive("Terse Lane");
    harness.app.ctx.settings.driving_speech = "quiet".to_string();
    harness.clear_speech();

    drift_off_the_pavement(&mut harness);

    let last = event_calls(&harness)
        .last()
        .cloned()
        .expect("going off the pavement speaks");
    assert_eq!(last, ("Off the road on the right!".to_string(), true));
}

#[test]
fn test_lane_departure_warning_flushes_event_voice() {
    let mut harness = a_drive("Lane Flush");
    harness.clear_speech();

    drift_off_the_pavement(&mut harness);

    let last = event_calls(&harness)
        .last()
        .cloned()
        .expect("going off the pavement speaks");
    assert_eq!(last, ("Off the road on the right!".to_string(), true));
}

// -- the pull-over --------------------------------------------------------------------

#[test]
fn test_a_pull_over_flushes_event_voice() {
    // Lights behind you interrupts whatever the event voice was saying. This
    // used to pin the silent "Speeding strike" line, which was the loudest
    // thing speeding could produce; that line is gone, so the interrupt
    // guarantee moved to the call that replaced it.
    let mut harness = a_drive("Pull Over");
    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = drive.trip.total_miles() / 2.0;
        drive.enforcement_prev_mi = drive.trip.position_mi;
        let at = drive.trip.position_mi + 0.2;
        drive.trip.posts = vec![EnforcementPost {
            method: method_by_kind(KIND_MEDIAN).to_string(),
            reach_mi: 1.0,
            facing: "both".to_string(),
            staffed: true,
            notice: 1.0,
            announced: true,
            ..EnforcementPost::new(at, KIND_MEDIAN)
        }];
        // Well over the real posted limit here rather than a faked 25: the
        // margin is what earns the stop, and the road's own number is fine
        // for that.
        let (limit, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
        drive.truck_mut().start_engine();
        drive.truck_mut().velocity_mps = (limit + 30.0) / MPH_PER_MPS;
        drive.over_limit_mi = OBSERVE_HOLD_MI * 2.0;
        drive.update_enforcement_watch(ctx, 0.1);
    });

    let last = event_calls(&harness)
        .last()
        .cloned()
        .expect("the pull-over speaks");
    assert!(
        last.0.starts_with("Lights and siren behind you."),
        "{last:?}"
    );
    assert!(last.1, "the pull-over has to flush the event voice");
}

// -- the parking valve ----------------------------------------------------------------

#[test]
fn test_setting_the_parking_brake_at_speed_dynamites_the_brakes() {
    // Not impossible -- violent (owner design, 2026-07-24): the real valve is
    // the emergency backup, so at speed the set slams on, flat-spots the
    // tires by speed, warns out loud, and never arms the waiting
    // fast-forward while rolling. A stopped set stays calm and quiet.
    let mut harness = a_drive("Dynamite");
    harness.with_drive(|drive, _| {
        drive.truck_mut().set_air_ready(false);
        drive.truck_mut().velocity_mps = 55.0 / MPH_PER_MPS;
    });
    let wear_before = harness.read_drive(|d| d.truck().tire_wear_pct);
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.toggle_parking_brake(ctx));

    assert!(harness.read_drive(|d| d.truck().parking_brake));
    assert!(harness.read_drive(|d| d.truck().tire_wear_pct) > wear_before + 1.0);
    assert!(!harness.read_drive(|d| d.trip.waiting));
    let last = main_lines(&harness).last().cloned().unwrap_or_default();
    assert!(last.contains("dynamited"), "{last}");

    // Release, stop, set again: the calm path, no extra tread cost.
    harness.with_drive(|drive, _| {
        drive.truck_mut().release_parking_brake();
        drive.truck_mut().velocity_mps = 0.0;
    });
    let wear_stopped = harness.read_drive(|d| d.truck().tire_wear_pct);
    harness.with_drive(|drive, ctx| drive.toggle_parking_brake(ctx));

    assert!(harness.read_drive(|d| d.truck().parking_brake));
    assert_eq!(
        harness.read_drive(|d| d.truck().tire_wear_pct),
        wear_stopped
    );
    assert!(harness.read_drive(|d| d.trip.waiting));
    let last = main_lines(&harness).last().cloned().unwrap_or_default();
    assert!(last.starts_with("Parking brake set."), "{last}");
}

// -- the assists spend air like a driver would ---------------------------------------

#[test]
fn test_cruise_easing_for_an_exit_keeps_the_air_up() {
    // Agent drive D, 2026-09-24: I-20 into Birmingham on the realistic
    // preset, cruise set at 70, and air ready went false at 62 mph on the
    // mainline. Cruise was gliding its target down for the exit; the service
    // trim switched on at 2 over at a fifteenth of the pedal, so the truck
    // rode that edge and cruise pumped the brake about fifteen times a
    // second. Air is charged per application, and the tanks lost to the
    // compressor. Pinned on the pedal's total rise as well as on the gauge,
    // so the mechanism fails the test and not only its worst case.
    let mut harness = PlaytestHarness::new();
    assert!(harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("realistic"));
    harness.start_route(
        "Tuscaloosa",
        "Birmingham",
        freight_fate::playtest::harness::RouteSetup::seeded(7).named("Air Glide"),
    );
    harness.with_drive(|d, ctx| {
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        d.departure_checked = true;
        d.weather_mut().current = WeatherKind::Clear;
        d.trip.position_mi = 19.9;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().rpm = 1500.0;
        d.truck_mut().velocity_mps = 62.0 / MPH_PER_MPS;
    });
    press(&mut harness, Key::K);
    harness.with_drive(|d, _| d.cruise_mph = Some(70.0));

    let mut min_psi = f64::MAX;
    let mut rises = 0.0;
    let mut last_brake = 0.0;
    let mut eased_for_the_exit = false;
    for _ in 0..(60 * 60 * 8) {
        frame(&mut harness);
        harness.finish_timed_state();
        if !harness.has_drive() {
            break;
        }
        let (psi, brake, cruising, exit) = harness.read_drive(|d| {
            (
                d.truck().air_pressure_psi(),
                d.truck().brake,
                d.cruise_mph.is_some(),
                d.cruise_held_reason == "for the exit",
            )
        });
        if !cruising {
            break; // off the mainline: the exit's own assists own the pedals
        }
        eased_for_the_exit |= exit;
        rises += (brake - last_brake).max(0.0);
        last_brake = brake;
        min_psi = min_psi.min(psi);
    }
    assert!(eased_for_the_exit, "the drive never reached its exit glide");
    assert!(rises < 2.0, "the pedal rose {rises:.2} full applications");
    assert!(min_psi > 110.0, "the tanks fell to {min_psi:.1} psi");
}

#[test]
fn test_exit_speed_assist_does_not_pump_on_a_downgrade() {
    // The same class, in the exit speed assist with nobody else on the
    // pedals: it pressed a full 0.35 the moment the truck crossed what the
    // gore accepts, so down a grade gravity put the truck straight back over
    // after every application and the pedal pumped.
    use crate::transcript_cruise_support::bench_road;
    use ff_core::sim::trip_models::RoadStop;

    let mut harness = a_drive("Exit Edge");
    harness.app.ctx.settings.exit_speed_assist = true;
    harness.with_drive(|d, _| {
        bench_road(d, 60.0, -4.0, 1.0);
        d.trip.position_mi = 200.0;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        let stop = RoadStop::new("Downgrade Travel Plaza", 201.5, "truck_stop");
        let gate = d.gore_acceptance_mph(Some(&stop));
        d.exit_stop = Some(stop);
        d.exit_signal_on = true;
        d.truck_mut().velocity_mps = (gate - 0.5) / MPH_PER_MPS;
    });
    let mut rises = 0.0;
    let mut last_brake = 0.0;
    for _ in 0..(60 * 20) {
        frame(&mut harness);
        let brake = harness.read_drive(|d| d.truck().brake);
        rises += (brake - last_brake).max(0.0);
        last_brake = brake;
    }
    let (ahead, braked) = harness.read_drive(|d| (201.5 - d.trip.position_mi, d.truck().brake));
    assert!(ahead > 0.0, "the bench ran past its exit");
    assert!(braked > 0.0, "the assist never held the grade");
    assert!(rises < 2.0, "the pedal rose {rises:.2} full applications");
}
