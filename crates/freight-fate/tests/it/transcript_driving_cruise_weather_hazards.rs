//! Hazard reaction windows and descent control (port of
//! `tests/test_driving_cruise_weather.py`, lines 1818-2426).
//!
//! Third of the split; see `transcript_driving_cruise_weather.rs` for why the
//! Python file is split and `transcript_cruise_support` for what replaced each
//! monkeypatch.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{TrafficContext, TripEvent, TripEventData, TripEventKind};
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::{
    HAZARD_CREEP_MPH, HAZARD_MIN_REACTION_S, HAZARD_SAFE_MPH, LANE_TAP_CHANGE_S, MPH_PER_MPS,
};

use crate::transcript_cruise_support::*;

fn hazard(message: &str, deadline_s: f64, dodgeable: bool) -> TripEvent {
    TripEvent {
        kind: TripEventKind::Hazard,
        message: message.into(),
        data: TripEventData {
            deadline_s: Some(deadline_s),
            dodgeable: if dodgeable { Some(true) } else { None },
            ..Default::default()
        },
    }
}

fn raise_hazard(harness: &mut PlaytestHarness, event: TripEvent) {
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));
}

/// `clear_weather(driving)`: pin the trip's weather to clear so grip stays 1.0
/// for the whole test.
fn clear_weather(harness: &mut PlaytestHarness) {
    harness.with_drive(|d, _| {
        let weather = d.weather_mut();
        weather.provider = None;
        weather.live = false;
        weather.current = WeatherKind::Clear;
        weather.minutes_until_change = 1e9;
    });
}

// -- hazard reaction windows ---------------------------------------------------

#[test]
fn test_hazard_deadline_covers_braking_time_from_current_speed() {
    // A fixed 3-4.5 s window was unbeatable at highway speed: a full-service
    // stop from 65 to 25 mph alone takes ~5 s. The deadline must cover the
    // braking the truck actually needs -- fade, wear, and load included --
    // from the current speed, and leave the rolled reaction slack on top.
    let mut harness = start_drive_scaled("Hazard Budget", Some(20.0));
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
        d.truck_mut().grip = 1.0;
        d.truck_mut().grade = 0.0;
    });
    raise_hazard(&mut harness, hazard("Brake now!", 3.0, false));
    let (speed, decel) =
        harness.read_drive(|d| (d.truck().speed_mph(), d.truck().full_service_decel_mps2()));
    let brake_s = (speed - HAZARD_SAFE_MPH) / MPH_PER_MPS / decel;
    assert!(approx_abs(
        harness.read_drive(|d| d.brake_budget_s(HAZARD_SAFE_MPH)),
        brake_s,
        0.01
    ));
    assert!(approx_abs(
        harness
            .read_drive(|d| d.hazard_deadline)
            .expect("a deadline"),
        harness.read_drive(|d| d.aeb_engage_s(HAZARD_SAFE_MPH)) + 3.0,
        0.01
    ));
    assert!(
        harness
            .read_drive(|d| d.hazard_deadline)
            .expect("a deadline")
            > brake_s + 3.0
    );
}

#[test]
fn test_automatic_emergency_braking_engages_once_and_cancels_cruise() {
    // The assist takes the truck on the SERVICE brakes and says so once.
    //
    // "Emergency braking engaged" is reserved for the escalation, which this
    // truck has not earned: the announcement must not claim the hardest stop
    // the rig has while the assist is on the normal brakes.
    let mut harness = start_drive("AEB Once");
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 25.0;
        d.speed_control_armed = true;
        d.cruise_mph = Some(55.0);
        d.hazard_deadline = Some(d.brake_budget_s(HAZARD_SAFE_MPH));
    });
    harness.with_drive(|d, ctx| {
        d.update_hazard(ctx, 0.01);
        d.update_hazard(ctx, 0.01);
    });
    assert!(approx(harness.read_drive(|d| d.truck().brake), 1.0));
    assert!(!harness.read_drive(|d| d.truck().emergency_brake));
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    let said = spoken(&harness);
    assert_eq!(
        said.iter().filter(|s| *s == "Automatic braking.").count(),
        1
    );
    assert!(!said.iter().any(|s| s == "Emergency braking engaged."));
    // Paused, not disarmed: the session survives the hazard so cruise
    // resumes once the truck is rolling and off the brakes. Disarming left
    // the truck coasting to 5 km/h on open I-90 with the driver believing
    // cruise had it (owner ruling, 2026-09-01).
    assert!(harness.read_drive(|d| d.speed_control_armed));
}

#[test]
fn test_fixed_object_hazard_needs_nearly_a_stop_or_a_swerve() {
    // You cannot roll over a ladder at 25: a dodgeable hazard resolved by
    // brake alone takes nearly a stop, with a one-time hint past the old safe
    // speed so the quiet never reads as an already-cleared hazard.
    let mut harness = bench_drive("Fixed Object", 200.0, 0.0);
    // Python patched `trip.has_open_adjacent_lane_at` to True so there was a
    // lane to swerve into. The bench road really has one; asserted rather than
    // assumed, because a one-lane bench would silently gut the case.
    assert!(harness.read_drive(|d| d.trip.has_open_adjacent_lane_at(None)));
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
        d.truck_mut().grip = 1.0;
        d.truck_mut().grade = 0.0;
    });
    harness.clear_speech();
    raise_hazard(
        &mut harness,
        hazard("Change lanes or brake! Debris on the road.", 3.0, true),
    );
    assert!(harness.read_drive(|d| d.hazard_dodgeable));

    // The old moving-hazard speed no longer clears it; the hint speaks once.
    harness.with_drive(|d, ctx| {
        d.truck_mut().velocity_mps = (HAZARD_SAFE_MPH - 1.0) / MPH_PER_MPS;
        d.update_hazard(ctx, DT);
        d.update_hazard(ctx, DT);
    });
    assert!(harness.read_drive(|d| d.hazard_deadline).is_some());
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|s| {
                *s == "It is still in your lane. Nearly stop, or change lanes. Left lane open."
            })
            .count(),
        1
    );

    // Nearly stopping resolves it, and the outcome is all that is said.
    harness.with_drive(|d, ctx| {
        d.truck_mut().velocity_mps = (HAZARD_CREEP_MPH - 1.0) / MPH_PER_MPS;
        d.update_hazard(ctx, DT);
    });
    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
    assert!(said_any(&harness, "Clear"));
}

#[test]
fn test_fixed_object_hazard_deadline_budgets_the_longer_stop() {
    // The dodgeable deadline must cover braking to the creep speed, not the
    // moving-hazard speed -- otherwise the honest demand becomes unwinnable.
    let mut harness = start_drive_scaled("Fixed Object Budget", Some(20.0)); // reaction window multiplier 1.0
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
        d.truck_mut().grip = 1.0;
        d.truck_mut().grade = 0.0;
    });
    raise_hazard(
        &mut harness,
        hazard("Change lanes or brake! Debris on the road.", 3.0, true),
    );
    assert!(approx_abs(
        harness
            .read_drive(|d| d.hazard_deadline)
            .expect("a deadline"),
        harness.read_drive(|d| d.aeb_engage_s(HAZARD_CREEP_MPH)) + 3.0 + LANE_TAP_CHANGE_S,
        0.01
    ));
    assert!(
        harness
            .read_drive(|d| d.hazard_deadline)
            .expect("a deadline")
            > harness.read_drive(|d| d.brake_budget_s(HAZARD_SAFE_MPH)) + 3.0
    );
}

#[test]
fn test_brake_budget_honors_fade_wear_and_load() {
    // The AEB budget must use the braking the truck can actually deliver: the
    // spec number engaged the assist two seconds before a collision on hot
    // brakes (playtest transcript, 2026-07-16).
    let mut harness = start_drive("Brake Budget");
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
        d.truck_mut().grip = 1.0;
        d.truck_mut().grade = 0.0;
    });
    let fresh = harness.read_drive(|d| d.brake_budget_s(HAZARD_SAFE_MPH));

    harness.with_drive(|d, _| {
        let fade = d.truck().specs.brake_fade_temp_c;
        d.truck_mut().brake_temp_c = fade + 150.0; // cooked drums
    });
    let hot = harness.read_drive(|d| d.brake_budget_s(HAZARD_SAFE_MPH));
    assert!(hot > fresh * 1.5, "{hot} {fresh}");

    harness.with_drive(|d, _| {
        d.truck_mut().brake_temp_c = 20.0;
        d.truck_mut().brake_wear_pct = 60.0;
    });
    let worn = harness.read_drive(|d| d.brake_budget_s(HAZARD_SAFE_MPH));
    assert!(worn > fresh, "{worn} {fresh}");
}

#[test]
fn test_the_driver_always_gets_a_real_window_before_the_assist_takes_over() {
    // The reaction window must be a promise, not a leftover.
    //
    // Reported by Munchkinbear, 2026-08-11: "less than half a second between
    // being told to brake or change lanes and the truck slamming on the
    // emergency brakes". The window was whatever survived after the assist's
    // engage margin -- which scales with the stopping budget -- was subtracted
    // from the fixed slack, so every reason the truck stops badly (speed,
    // grade, heat, wear, grip) ate the driver's time instead of the truck's.
    //
    // Python's `@pytest.mark.parametrize` over five rows.
    for (label, mph, grade, brake_temp_c, wear_pct, grip, fatigue) in [
        ("fresh at highway speed", 65.0, 0.0, 20.0, 0.0, 1.0, 0.0),
        ("drowsy", 65.0, 0.0, 20.0, 0.0, 1.0, 80.0),
        (
            "down a five percent grade",
            65.0,
            -0.05,
            20.0,
            0.0,
            1.0,
            0.0,
        ),
        ("on cooked brakes", 65.0, 0.0, 500.0, 0.0, 1.0, 0.0),
        ("on worn brakes in the wet", 65.0, 0.0, 20.0, 60.0, 0.6, 0.0),
    ] {
        let mut harness = start_drive_scaled("Real Window", Some(20.0));
        harness.app.ctx.profile.as_mut().expect("a career").fatigue = fatigue;
        harness.with_drive(move |d, _| {
            d.truck_mut().velocity_mps = mph / MPH_PER_MPS;
            d.truck_mut().grade = grade;
            d.truck_mut().grip = grip;
            d.truck_mut().brake_temp_c = brake_temp_c;
            d.truck_mut().brake_wear_pct = wear_pct;
        });
        // The tightest slack the road emits: the traffic-pressure warning.
        raise_hazard(
            &mut harness,
            hazard("Change lanes or brake! Slow truck right ahead.", 2.5, true),
        );
        let window = harness.read_drive(|d| {
            d.hazard_deadline.expect("a deadline") - d.aeb_engage_s(d.hazard_target_mph(None))
        });
        assert!(
            window >= HAZARD_MIN_REACTION_S,
            "{label}: only {window:.2} s to react"
        );
    }
}

#[test]
fn test_a_dodgeable_hazard_leaves_time_to_finish_the_lane_change_it_asks_for() {
    // "Change lanes or brake" names a maneuver that takes 2.5 s of drift.
    // Demanding it inside a window shorter than the maneuver is not a demand,
    // it is a trap -- so a dodgeable hazard budgets the move on top of the
    // time to hear the warning and decide.
    let mut harness = start_drive_scaled("Dodge Window", Some(20.0));
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        d.truck_mut().grade = 0.0;
        d.truck_mut().grip = 1.0;
    });
    raise_hazard(
        &mut harness,
        hazard("Change lanes or brake! Debris on the road.", 2.5, true),
    );
    let window = harness.read_drive(|d| {
        d.hazard_deadline.expect("a deadline") - d.aeb_engage_s(d.hazard_target_mph(None))
    });
    assert!(
        window >= HAZARD_MIN_REACTION_S + LANE_TAP_CHANGE_S,
        "{window}"
    );
}

#[test]
fn test_the_assist_does_not_slam_on_mid_lane_change() {
    // A driver already sliding into the next lane has answered the warning.
    // Taking the truck away from them halfway through the move is what the
    // report described as "as you change lanes, slam go the emergency brakes".
    let mut harness = start_drive("Mid Change");
    harness.with_drive(|d, ctx| {
        d.truck_mut().velocity_mps = 25.0;
        d.hazard_dodgeable = true;
        d.hazard_lane = d.lane.lane;
        // Past the engage point, but the dodge still lands before the hazard.
        d.hazard_deadline = Some(d.aeb_engage_s(d.hazard_target_mph(None)));
        d.lane_change_target = Some(d.lane.lane + 1);
        d.lane_change_timer = LANE_TAP_CHANGE_S * 0.4;
        d.update_hazard(ctx, 0.01);
    });
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));

    // A dodge that can no longer beat the hazard does not hold the assist off.
    harness.with_drive(|d, ctx| {
        d.lane_change_timer = d.hazard_deadline.expect("a deadline") + 1.0;
        d.update_hazard(ctx, 0.01);
    });
    assert!(approx(harness.read_drive(|d| d.truck().brake), 1.0));
}

#[test]
fn test_the_assist_stands_on_everything_when_service_braking_is_losing() {
    // An assist that takes the truck has to actually stop it.
    //
    // Owner question, 2026-08-11: to help a player it should stop in time. It
    // did not always. The assist applied full SERVICE braking and the budget
    // that sized its engage point assumed the same, but a stop on hot, worn
    // brakes in the wet on a downgrade gets slower while it happens -- the
    // drums heat further under the very application meant to save it. Two of
    // nine benched conditions collided after "Emergency braking engaged."
    //
    // Full service stays the first answer. When the time left no longer covers
    // even that, the assist uses the hardest stop the rig has, which is what
    // the driver would do and exactly what the B key already gives them.
    let mut harness = start_drive_scaled("Escalate", Some(20.0));
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        d.truck_mut().grade = -0.06;
        d.truck_mut().grip = 0.7;
        d.truck_mut().brake_temp_c = 450.0;
        d.truck_mut().brake_wear_pct = 40.0;
        d.truck_mut().cargo_kg = 19_000.0;
    });
    let damage_before = harness.read_drive(|d| d.truck().damage_pct);
    raise_hazard(
        &mut harness,
        hazard("Change lanes or brake! Slow truck right ahead.", 2.5, true),
    );
    let mut stood_on_it = false;
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.hazard_deadline).is_some() && elapsed < 120.0 {
        let done = harness.with_drive(|d, ctx| {
            d.truck_mut().throttle = 0.0;
            d.update_hazard(ctx, DT);
            let stood = d.truck().emergency_brake;
            if d.hazard_deadline.is_none() {
                return (stood, true);
            }
            d.truck_mut().grade = -0.06;
            d.truck_mut().update(DT);
            (stood, false)
        });
        stood_on_it = stood_on_it || done.0;
        if done.1 {
            break;
        }
        elapsed += DT;
    }
    assert!(
        approx(harness.read_drive(|d| d.truck().damage_pct), damage_before),
        "the assist engaged and still hit the hazard"
    );
    assert!(
        stood_on_it,
        "service braking alone was losing and nothing escalated"
    );
}

#[test]
fn test_a_stop_service_braking_can_make_stays_on_the_service_brakes() {
    // The escalation is a last resort, not the new normal: an ordinary hazard
    // on good brakes must not become a spring-brake panic stop.
    let mut harness = start_drive_scaled("No Escalate", Some(20.0));
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        d.truck_mut().grade = 0.0;
        d.truck_mut().grip = 1.0;
        d.truck_mut().brake_temp_c = 20.0;
        d.truck_mut().brake_wear_pct = 0.0;
    });
    raise_hazard(
        &mut harness,
        hazard("Change lanes or brake! Slow truck right ahead.", 2.5, true),
    );
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.hazard_deadline).is_some() && elapsed < 120.0 {
        let done = harness.with_drive(|d, ctx| {
            d.truck_mut().throttle = 0.0;
            d.update_hazard(ctx, DT);
            assert!(
                !d.truck().emergency_brake,
                "good brakes on the flat needed no panic stop"
            );
            if d.hazard_deadline.is_none() {
                return true;
            }
            d.truck_mut().update(DT);
            false
        });
        if done {
            break;
        }
        elapsed += DT;
    }
}

#[test]
fn test_a_routine_assisted_stop_costs_one_brake_application() {
    // Owner ruling from a live drive, 2026-08-12: "This emergency braking has
    // to stop for brake assist. Air pressure keeps running out. Just use the
    // dang service brakes."
    //
    // Two things spent that air. The assist decided its application afresh
    // every frame against a threshold its own braking pushed away, so it let
    // go, the threshold came back and it pressed again -- and the air system
    // charges a whole brake application every time the pedal RISES. And the
    // input pass ramps the brake down and writes the emergency flag from the B
    // key, both before the physics runs and both after the assist's last word
    // on the frame, so what the drums actually got was a frame's ramp short of
    // full service and the difference was re-charged every frame.
    //
    // One held stop, one application: the pedal the drums see is the full one
    // the budget assumed, the gauge barely moves, and nothing escalates.
    let mut harness = bench_drive("One Application", 200.0, 0.0); // level ground
    release_keys(&mut harness);
    harness.app.ctx.settings.automatic_emergency_braking = true;
    clear_weather(&mut harness);
    press(&mut harness, Key::E, None); // engine on
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        d.truck_mut().grade = 0.0;
        d.truck_mut().grip = 1.0;
        d.truck_mut().brake_temp_c = 20.0;
        d.truck_mut().brake_wear_pct = 0.0;
        // Governor cut-out, so nothing rebuilds mid-test.
        d.truck_mut().set_air_pressure_psi(125.0);
    });
    let damage_before = harness.read_drive(|d| d.truck().damage_pct);
    let psi_before = harness.read_drive(|d| d.truck().primary_air_psi);
    raise_hazard(
        &mut harness,
        hazard("Brake now! Stopped traffic ahead.", 3.0, false),
    );

    // Units of pedal RISE the air system charged for, the measure the fanning
    // assists were caught with (bench trace, 2026-08-11).
    let mut charged = 0.0;
    let mut previous = harness.read_drive(|d| d.truck().last_service_air_application);
    let mut held_decel: f64 = 0.0;
    let mut pedal_seen: f64 = 0.0;
    for _ in 0..(60 * 30) {
        frame(&mut harness, DT);
        assert!(
            !harness.read_drive(|d| d.truck().emergency_brake),
            "a sound truck on the flat needed no panic stop"
        );
        let (application, aeb_brake, aeb_decel, deadline) = harness.read_drive(|d| {
            (
                d.truck().last_service_air_application,
                d.aeb_brake,
                d.aeb_decel_mps2,
                d.hazard_deadline,
            )
        });
        charged += 0.0f64.max(application - previous);
        previous = application;
        if aeb_brake > 0.0 {
            held_decel = held_decel.max(aeb_decel);
            pedal_seen = pedal_seen.max(application);
        }
        if deadline.is_none() {
            break;
        }
    }
    assert!(
        harness.read_drive(|d| d.hazard_deadline).is_none(),
        "the assist never resolved the hazard"
    );
    assert!(
        approx(harness.read_drive(|d| d.truck().damage_pct), damage_before),
        "the assist engaged and still hit the hazard"
    );
    // The pedal the physics and the air system saw is the full service
    // application the budget assumed, not what survived the input ramp.
    assert!(approx(pedal_seen, 1.0), "{pedal_seen}");
    assert!(held_decel >= harness.read_drive(|d| d.truck().full_service_decel_mps2()) * 0.95);
    assert!(
        charged <= 1.2,
        "one held stop was charged {charged:.1} brake applications"
    );
    let spent = psi_before - harness.read_drive(|d| d.truck().primary_air_psi);
    assert!(
        (3.0..=9.0).contains(&spent),
        "a single held stop spent {spent:.1} psi"
    );
}

#[test]
fn test_the_escalation_reads_what_the_truck_is_doing_not_what_it_should() {
    // Same speed, same time left, same brakes -- and only one of them panics.
    //
    // The old predicate asked whether the time left still covered what a full
    // application OUGHT to deliver. That is a prediction, and the assist was
    // not delivering it, so an ordinary assisted stop could trip it. The
    // escalation now reads the deceleration the truck is actually making: a
    // stop that is getting there keeps the service brakes however tight the
    // arithmetic looks, and only a stop that has stopped getting there stands
    // on everything.
    fn run(slowing: bool) -> bool {
        let mut harness = start_drive_scaled("Escalation Reads", Some(20.0));
        harness.app.ctx.settings.automatic_emergency_braking = true;
        let held = 65.0 / MPH_PER_MPS;
        harness.with_drive(move |d, _| {
            d.truck_mut().velocity_mps = held;
            d.truck_mut().grade = 0.0;
            d.truck_mut().grip = 1.0;
            d.truck_mut().brake_temp_c = 20.0;
            d.truck_mut().brake_wear_pct = 0.0;
        });
        raise_hazard(
            &mut harness,
            hazard("Brake now! Stopped traffic ahead.", 3.0, false),
        );
        let mut stood_on_it = false;
        for _ in 0..(60 * 60) {
            let (stood, done) = harness.with_drive(move |d, ctx| {
                d.truck_mut().throttle = 0.0;
                d.update_hazard(ctx, DT);
                let stood = d.truck().emergency_brake;
                if d.hazard_deadline.is_none() {
                    return (stood, true);
                }
                if slowing {
                    d.truck_mut().update(DT);
                    d.truck_mut().grade = 0.0;
                } else {
                    // Grip that is not there: everything is applied and the
                    // truck is not losing a single mile an hour.
                    d.truck_mut().velocity_mps = held;
                }
                (stood, false)
            });
            stood_on_it = stood_on_it || stood;
            if done {
                break;
            }
        }
        stood_on_it
    }

    assert!(
        !run(true),
        "a stop that was getting there did not need the hard version"
    );
    assert!(
        run(false),
        "a truck that was not slowing at all rode it into the hazard"
    );
}

#[test]
fn test_automatic_emergency_braking_leads_the_budget() {
    // The assist engages with margin over the physics budget: braking heats
    // the brakes, so a zero-margin engage under-delivers exactly as it fires.
    let mut harness = start_drive("AEB Lead");
    harness.with_drive(|d, ctx| {
        d.truck_mut().velocity_mps = 25.0;
        // More time left than the raw budget, but within the safety lead.
        d.hazard_deadline = Some(d.brake_budget_s(HAZARD_SAFE_MPH) * 1.1 + 0.2);
        d.update_hazard(ctx, 0.01);
    });
    assert!(approx(harness.read_drive(|d| d.truck().brake), 1.0));
}

#[test]
fn test_descent_control_levels_and_brake_capture() {
    // Python's `@pytest.mark.parametrize` over four rows.
    for (level, braking, expected_active) in [
        ("off", false, false),
        ("realistic", false, true),
        ("balanced", true, true),
        ("interactive", true, true),
    ] {
        let mut harness = bench_drive("Descent Levels", 200.0, 0.0);
        harness.app.ctx.settings.descent_speed_control = level.to_string();
        harness.clear_speech();
        harness.with_drive(move |d, ctx| {
            d.truck_mut().grade = -0.06;
            d.truck_mut().engine_on = true;
            d.truck_mut().velocity_mps = 22.0;
            d.truck_mut().transmission.automatic = true;
            d.cruise_mph = Some(60.0);
            d.update_cruise(ctx, 0.1, braking, false, false);
        });
        assert_eq!(
            harness.read_drive(|d| d.descent_control_active),
            expected_active,
            "{level}"
        );
        if braking && (level == "balanced" || level == "interactive") {
            // The brake caps THIS GRADE and leaves the driver's set speed
            // alone. It used to assign into cruise_mph, which made every
            // downhill brake permanent and cumulative (Brandon, 2026-08-23:
            // a whole run pinned at "forty nine mph or lower"). See
            // test_braking_on_a_grade_caps_it_without_rewriting_the_set_speed
            // in transcript_driving_cruise_weather_grades.rs.
            assert!(approx(
                harness.read_drive(|d| d.cruise_mph).expect("cruise"),
                60.0
            ));
            assert!(approx_abs(
                harness
                    .read_drive(|d| d.cruise_descent_mph)
                    .expect("a grade cap"),
                harness.read_drive(|d| d.truck().speed_mph()),
                0.01
            ));
            assert_eq!(
                spoken(&harness)
                    .iter()
                    .filter(|t| t.contains("Descent control holding"))
                    .count(),
                1
            );
            harness.with_drive(|d, ctx| d.update_cruise(ctx, 0.1, true, false, false));
            assert_eq!(
                spoken(&harness)
                    .iter()
                    .filter(|t| t.contains("Descent control holding"))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn test_service_brakes_beat_a_highway_hazard_after_human_reaction() {
    // The taught response -- hear the warning, hold Down -- must succeed from
    // highway speed even with a slow human reaction, without the emergency
    // brake.
    let mut harness = start_drive("Human Reaction");
    clear_weather(&mut harness);
    release_keys(&mut harness);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
    });
    let damage_before = harness.read_drive(|d| d.truck().damage_pct);

    raise_hazard(&mut harness, hazard("Brake now!", 3.0, false));
    frames(&mut harness, (60.0 * 1.5) as usize, DT); // hearing the warning
    hold(&mut harness, &[Key::Down]); // then service brakes only
    for _ in 0..(60 * 20) {
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.hazard_deadline).is_none() {
            break;
        }
    }
    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
    assert!(approx(
        harness.read_drive(|d| d.truck().damage_pct),
        damage_before
    )); // avoided, not collided
}

// -- a vehicle hazard is not a fixed object ------------------------------------

/// [`hazard`] with a name, and optionally the lead vehicle it is about.
fn named_hazard(name: &str, dodgeable: bool, lead_mph: Option<f64>) -> TripEvent {
    let mut event = hazard("Brake!", 3.0, dodgeable);
    event.data.name = Some(name.to_string());
    event.data.traffic = lead_mph.map(|mph| TrafficContext {
        lead: TrafficVehicle::new("lead", 0.5, mph, mph, 0, "cruise", "truck"),
        gap_mi: 0.2,
        closing_mph: 10.0,
    });
    event
}

/// `arm(**data)`: a fresh hazard, and the speed it now asks the truck down to.
fn arm(harness: &mut PlaytestHarness, event: TripEvent) -> f64 {
    harness.with_drive(move |d, ctx| {
        d.hazard_deadline = None;
        d.hazard_lead_mph = None;
        d.handle_trip_event(ctx, &event);
    });
    harness.read_drive(|d| d.hazard_target_mph(None))
}

#[test]
fn test_a_vehicle_hazard_clears_at_the_vehicles_speed_not_a_near_stop() {
    // Brandon, 2026-08-23: cruise "drops speed dramatically... and never
    // comes back up to highway speed".
    //
    // His log is sixteen brake-light hazards in ninety minutes on I-70 in
    // Kansas -- level road, limit 75, cruise set 70 -- each one demanding he
    // go nearly to a stop, and with automatic braking the truck obeyed
    // without him choosing it. A loaded truck needs over a minute to climb
    // back, and they arrived a median of 112 seconds apart, so he lived at 37
    // to 40.
    //
    // The cause was `dodgeable` doing two jobs: "you can steer around this"
    // and "this is not moving, so nearly stop". True together for a tyre
    // carcass, false for a truck doing 55. A vehicle hazard now clears at the
    // vehicle's own speed, which is what a driver actually does and is
    // honestly clear -- you are no longer closing on it.
    let mut harness = start_drive("Vehicle Hazard");
    harness.with_drive(|d, _| quiet(&mut d.trip));

    // A moving vehicle: match it, do not stop for it.
    assert!(approx(
        arm(
            &mut harness,
            named_hazard("the brake lights", true, Some(55.0))
        ),
        55.0
    ));
    assert!(approx(
        arm(
            &mut harness,
            named_hazard("the slow truck", true, Some(30.0))
        ),
        30.0
    ));

    // Everything that is not a moving vehicle is exactly as it was.
    assert!(approx(
        arm(&mut harness, named_hazard("retread debris", true, None)),
        HAZARD_CREEP_MPH
    ));
    assert!(approx(
        arm(&mut harness, named_hazard("the deer", false, None)),
        HAZARD_SAFE_MPH
    ));
    // A lead that has itself stopped still asks for a stop: the creep speed
    // is a floor, not a starting point.
    assert!(approx(
        arm(
            &mut harness,
            named_hazard("stopped traffic", true, Some(0.0))
        ),
        HAZARD_CREEP_MPH
    ));
}

#[test]
fn test_folding_a_fixed_obstacle_into_a_vehicle_hazard_takes_the_near_stop_back() {
    // Two hazards at once take the harsher demand.
    //
    // A truck ahead and a tyre carcass under it is not "match the truck": the
    // carcass is not going anywhere, and the group has to be crawled.
    let mut harness = start_drive("Folded Hazard");
    harness.with_drive(|d, _| {
        quiet(&mut d.trip);
        d.truck_mut().velocity_mps = 70.0 / MPH_PER_MPS;
    });
    assert!(approx(
        arm(
            &mut harness,
            named_hazard("the brake lights", true, Some(55.0))
        ),
        55.0
    ));

    // Debris folds in while the first is still live.
    let debris = {
        let mut event = named_hazard("retread debris", true, None);
        event.data.deadline_s = Some(2.0);
        event
    };
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &debris));
    assert!(approx(
        harness.read_drive(|d| d.hazard_target_mph(None)),
        HAZARD_CREEP_MPH
    ));
}

// -- the call names the open side ---------------------------------------------------

/// A slow car already inside the warning window, in the truck's own lane, on
/// a road with `lanes` your side and the truck in `truck_lane`; `held` are the
/// neighbouring lanes a semi is riding alongside in. Adaptive cruise is on at
/// 65, so a case can also watch what the warning does to it. Lane keeping is
/// `mode`: the call below is the one a driver answers, so most cases pin
/// partial; on full the truck passes by itself (`states_driving_passing`).
/// Returns the harness after the warning has spoken.
///
/// Owner, 2026-09-01, on I-90 with adaptive cruise on: "Change lanes or
/// brake!" left a blind driver guessing whether a lane change was even
/// possible. The game already knew, so the call names the side.
pub(crate) fn meet_a_slow_car(
    name: &str,
    mode: &str,
    lanes: i64,
    truck_lane: i64,
    held: &[i64],
    terse: bool,
) -> PlaytestHarness {
    use ff_core::sim::trip_models::TRAFFIC_WARNING_GAP_S;
    use freight_fate::playtest::harness::RouteSetup;

    use crate::states_driving_traffic_rate::lane_bench;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.speed_keeper = false;
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.app.ctx.settings.lane_keeping = mode.to_string();
    harness.app.ctx.settings.time_scale = 1.0;
    if terse {
        harness.app.ctx.settings.driving_speech = "quiet".to_string();
    }
    harness.start_route(
        "aberdeen_sd_us",
        "pierre_sd_us",
        RouteSetup::seeded(7).named(name),
    );
    let held = held.to_vec();
    harness.with_drive(move |d, ctx| {
        lane_bench(d, 65.0, lanes);
        d.trip.time_scale = 1.0;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.posts.clear();
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().set_air_ready(false);
        d.trip.position_mi = 20.0;
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        d.trip.traffic_manager.rolling_bubble = false;
        d.lane.set_lane_count(lanes);
        d.lane.lane = truck_lane;
        d.trip.traffic_manager.player_lane = truck_lane;
        d.speed_control_armed = true;
        d.cruise_mph = Some(65.0);
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        let position = d.trip.position_mi;
        let gap_mi = 45.0 * (TRAFFIC_WARNING_GAP_S - 0.4) / 3600.0;
        let mut vehicles = vec![TrafficVehicle::new(
            "bench:lead",
            position + gap_mi,
            45.0,
            45.0,
            0,
            "following",
            "car",
        )
        .with_lane(truck_lane)];
        for lane in held {
            vehicles.push(
                TrafficVehicle::new(
                    &format!("bench:held:{lane}"),
                    position,
                    65.0,
                    65.0,
                    truck_lane - lane,
                    "cruising",
                    "semi",
                )
                .with_lane(lane),
            );
        }
        d.trip.set_npc_vehicles(vehicles);
    });
    harness.clear_speech();
    release_keys(&mut harness);
    for _ in 0..120 {
        frame(&mut harness, DT);
        if said_any(&harness, "Slow car right ahead") {
            return harness;
        }
    }
    panic!("no lead warning was spoken: {:#?}", spoken(&harness));
}

/// The warning the meeting produced.
pub(crate) fn the_call(harness: &PlaytestHarness) -> String {
    spoken(harness)
        .into_iter()
        .find(|line| line.contains("Slow car right ahead"))
        .expect("a lead warning")
}

#[test]
fn test_the_hazard_call_names_the_left_lane_when_that_is_the_open_one() {
    // Right lane of two, nobody alongside: the only way around is left, and
    // the call says so. Cruise is left armed for the dodge it just asked for.
    let harness = meet_a_slow_car("Left Open", "partial", 2, 0, &[], false);
    assert_eq!(
        the_call(&harness),
        "Change lanes or brake! Slow car right ahead. Left lane open."
    );
    assert!(harness.read_drive(|d| d.hazard_dodgeable));
    // On partial the lane change is the driver's to make.
    assert!(harness.read_drive(|d| d.lane_change_target).is_none());
    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
}

#[test]
fn test_the_hazard_call_names_the_right_lane_when_that_is_the_open_one() {
    // Left lane of two: the way around is right.
    let harness = meet_a_slow_car("Right Open", "partial", 2, 1, &[], false);
    assert_eq!(
        the_call(&harness),
        "Change lanes or brake! Slow car right ahead. Right lane open."
    );
    assert!(harness.read_drive(|d| d.hazard_dodgeable));
}

#[test]
fn test_the_hazard_call_offers_either_side_from_the_middle_of_three() {
    let harness = meet_a_slow_car("Either Open", "partial", 3, 1, &[], false);
    assert_eq!(
        the_call(&harness),
        "Change lanes or brake! Slow car right ahead. Either lane open."
    );
    // A semi riding alongside on the left leaves only the right.
    drop(harness);
    let harness = meet_a_slow_car("Left Held", "partial", 3, 1, &[2], false);
    assert_eq!(
        the_call(&harness),
        "Change lanes or brake! Slow car right ahead. Right lane open."
    );
}

#[test]
fn test_with_no_lane_open_the_call_is_brake_and_says_so() {
    // Right lane of two with a semi alongside in the left: nowhere to go.
    // Same family as a one-lane road -- not dodgeable, so no lane-tap
    // allowance is added to the window, and cruise is paused for the braking
    // the call asks for (still armed to resume, per the 2026-09-01 ruling).
    let harness = meet_a_slow_car("Nowhere", "full", 2, 0, &[1], false);
    assert_eq!(
        the_call(&harness),
        "Brake! Slow car right ahead. No lane open. Automatic speed control paused."
    );
    assert!(!harness.read_drive(|d| d.hazard_dodgeable));
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    assert!(harness.read_drive(|d| d.speed_control_armed));
}

#[test]
fn test_the_terse_rung_keeps_the_thing_and_the_open_side() {
    // Quiet drops the opener -- the named lane says everything it did -- and
    // the one-tap answer survives in full.
    let harness = meet_a_slow_car("Terse Left", "partial", 2, 0, &[], true);
    assert_eq!(the_call(&harness), "Slow car right ahead. Left lane open.");
}

#[test]
fn test_lane_keeping_on_full_passes_on_the_named_side_with_cruise_still_armed() {
    // Owner ruling, 2026-10-01: on full lane keeping the truck takes the open
    // lane itself instead of braking to the slow car's speed behind it, and
    // the call says so. Adaptive cruise rides through the pass rather than
    // being cancelled by it (the 2026-09-01 decision for the driver's dodge).
    let mut harness = meet_a_slow_car("Full Pass", "full", 2, 0, &[], false);
    assert_eq!(
        the_call(&harness),
        "Slow car right ahead. Passing on the left."
    );
    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(1));

    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.hazard_deadline).is_some() && elapsed < LANE_TAP_CHANGE_S + 1.0 {
        frame(&mut harness, DT);
        elapsed += DT;
        // The assist never takes the pedal from a dodge that is landing.
        assert!(!harness.read_drive(|d| d.truck().emergency_brake));
    }
    assert!(
        harness.read_drive(|d| d.hazard_deadline).is_none(),
        "the pass did not clear the hazard: {:#?}",
        spoken(&harness)
    );
    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    assert!(said_any(
        &harness,
        "In the left lane, passing the slow car."
    ));
    assert!(!said_any(&harness, "Clear of the"));
    // Cruise rode through the dodge: still armed, still set.
    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert_eq!(harness.read_drive(|d| d.cruise_mph), Some(65.0));
    assert!(!said_any(&harness, "Automatic speed control"));
}
