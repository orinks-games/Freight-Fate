//! Debris is taken, not stopped for (owner, 2026-11-09).
//!
//! On I-43 with the left lane held, automatic braking took the truck from
//! 65 to a near stop for a strip of tire tread, said "It is still in your
//! lane. Nearly stop." on the way down, and the climb back cost a minute.
//! The catalog now says how each thing in the lane is taken:
//!
//! * small debris (tire debris, loose debris) is a heads-up with the lane
//!   answer; nothing brakes for it, cruise stays on, and with no lane open
//!   the truck drives over it: a thump, and at highway speed a seeded roll
//!   for minor damage, always spoken when it lands;
//! * big debris (a ladder, lumber, a mattress, boxes, a tarp, tumbleweeds)
//!   keeps the hazard call; braking alone aims at 30, taking it at 30 or
//!   under costs nothing, and above 30 it is struck for damage that grows
//!   with speed while the truck keeps rolling and cruise stays armed;
//! * a hard object (rockfall, a spun-out car, a bare in-lane event) keeps
//!   the near stop and the full collision.
//!
//! And the hint line is gone in every case.

use std::fs;
use std::path::Path;

use ff_core::sim::trip_models::{
    DebrisClass, OpenSide, TripEvent, TripEventData, TripEventKind, BIG_DEBRIS_TAKE_MPH,
};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::{
    debris_ahead_body, in_lane_hazard_call, passing_hazard_call, small_debris_call, SpokenMessage,
};

use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving_core::HAZARD_CREEP_MPH;

const MPH_PER_MPS: f64 = 2.2369362920544;
const DT: f64 = 1.0 / 60.0;

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
        drive.truck_mut().start_engine();
        drive.truck_mut().set_air_ready(false);
        drive.truck_mut().grip = 1.0;
        drive.truck_mut().grade = 0.0;
    });
    harness.clear_speech();
    harness
}

fn at_mph(harness: &mut PlaytestHarness, mph: f64) {
    harness.with_drive(move |drive, _| drive.truck_mut().velocity_mps = mph / MPH_PER_MPS);
}

/// Small debris as the emitter words it: the heads-up, the lane answer, and
/// the pass line lane keeping on full answers it with.
fn tread(side: OpenSide) -> TripEvent {
    let body = debris_ahead_body("the tire debris");
    TripEvent {
        kind: TripEventKind::Hazard,
        message: small_debris_call(&body, side.spoken()),
        data: TripEventData {
            deadline_s: Some(8.0),
            dodgeable: Some(side.is_open()),
            in_lane: Some(true),
            open_side: Some(side),
            pass_message: side
                .is_open()
                .then(|| passing_hazard_call(&body, side))
                .flatten(),
            animal: Some(false),
            debris: Some(DebrisClass::Small),
            strike_pct: Some(2.0),
            name: Some("the tire debris".to_string()),
            ..Default::default()
        },
    }
}

/// Big debris as the emitter words it.
fn ladder(side: OpenSide) -> TripEvent {
    TripEvent {
        kind: TripEventKind::Hazard,
        message: in_lane_hazard_call("A ladder fallen from a truck in the lane.", side),
        data: TripEventData {
            deadline_s: Some(3.5),
            dodgeable: Some(side.is_open()),
            in_lane: Some(true),
            open_side: Some(side),
            animal: Some(false),
            brake_to_mph: Some(BIG_DEBRIS_TAKE_MPH),
            debris: Some(DebrisClass::Big),
            strike_pct: Some(8.0),
            name: Some("the ladder".to_string()),
            ..Default::default()
        },
    }
}

/// A hard object in the lane: rockfall, as the emitter words it.
fn rockfall() -> TripEvent {
    TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("Brake! Rockfall debris on the road. No lane open."),
        data: TripEventData {
            deadline_s: Some(3.5),
            dodgeable: Some(false),
            in_lane: Some(true),
            open_side: Some(OpenSide::Neither),
            animal: Some(false),
            name: Some("the rockfall".to_string()),
            ..Default::default()
        },
    }
}

fn arm(harness: &mut PlaytestHarness, event: TripEvent) {
    harness.with_drive(move |drive, ctx| drive.handle_trip_event(ctx, &event));
}

/// Run the hazard update until the hazard resolves, or `max_s` pass.
fn run_until_clear(harness: &mut PlaytestHarness, max_s: f64) {
    let mut t = 0.0;
    while t < max_s && harness.read_drive(|d| d.hazard_deadline.is_some()) {
        harness.with_drive(|d, ctx| d.update_hazard(ctx, DT));
        t += DT;
    }
}

// -- small debris ------------------------------------------------------------------------

#[test]
fn tread_with_no_lane_open_is_driven_over_and_nothing_brakes_for_it() {
    let mut harness = a_drive("Tread No Lane");
    let audio = harness.app.record_audio();
    at_mph(&mut harness, 65.0);
    harness.with_drive(|d, ctx| {
        ctx.settings.automatic_emergency_braking = true;
        d.cruise_mph = Some(65.0);
        d.speed_control_armed = true;
    });
    let damage_before = harness.read_drive(|d| d.trip.truck.damage_pct);
    arm(&mut harness, tread(OpenSide::Neither));

    // The call is a heads-up: no brake word, no cruise pause.
    let call = harness.app.event_lines();
    assert_eq!(call, vec!["Tire debris ahead. No lane open.".to_string()]);
    assert_eq!(
        harness.read_drive(|d| d.hazard_debris),
        Some(DebrisClass::Small)
    );
    assert!(
        harness.read_drive(|d| d.cruise_mph.is_some()),
        "cruise stayed on"
    );
    assert!(
        !audio
            .borrow()
            .played
            .iter()
            .any(|(key, _, _)| key == "events/hazard_warning"),
        "the hazard tone is for things that need a reaction: {:?}",
        audio.borrow().played
    );

    harness.clear_speech();
    run_until_clear(&mut harness, 20.0);
    assert!(
        harness.read_drive(|d| d.hazard_deadline.is_none()),
        "taken at the deadline"
    );
    assert_eq!(
        harness.read_drive(|d| d.aeb_brake),
        0.0,
        "the assist never took the pedal"
    );
    let speed = harness.read_drive(|d| d.trip.truck.speed_mph());
    assert!(speed > 64.0, "the truck kept its speed: {speed:.1}");
    assert!(
        harness.read_drive(|d| d.cruise_mph.is_some()),
        "cruise still on"
    );
    let heard = harness.app.event_lines();
    assert!(
        !heard
            .iter()
            .any(|l| l.contains("Automatic braking") || l.contains("Nearly stop")),
        "{heard:?}"
    );
    assert!(
        audio
            .borrow()
            .played
            .iter()
            .any(|(key, _, _)| key == "vehicle/road_joint"),
        "the thump under the wheels: {:?}",
        audio.borrow().played
    );
    // Damage is always spoken when the roll lands, and nothing is said when
    // it does not (issue 533: damage with no line saying why).
    let damage_after = harness.read_drive(|d| d.trip.truck.damage_pct);
    let collision_line = heard
        .iter()
        .find(|l| l.starts_with("Collision with the tire debris."));
    if damage_after > damage_before {
        let pct = damage_after - damage_before;
        assert!((1.0..=2.0).contains(&pct), "a point or two: {pct}");
        assert_eq!(
            collision_line.map(String::as_str),
            Some(
                format!("Collision with the tire debris. Damage {damage_after:.0} percent.")
                    .as_str()
            )
        );
    } else {
        assert!(heard.is_empty(), "nothing broke, nothing to say: {heard:?}");
    }
}

#[test]
fn tread_under_highway_speed_never_costs_damage() {
    let mut harness = a_drive("Tread Slow");
    at_mph(&mut harness, 45.0);
    let before = harness.read_drive(|d| d.trip.truck.damage_pct);
    arm(&mut harness, tread(OpenSide::Neither));
    harness.clear_speech();
    run_until_clear(&mut harness, 20.0);
    assert_eq!(harness.read_drive(|d| d.trip.truck.damage_pct), before);
    assert!(harness.app.event_lines().is_empty());
}

#[test]
fn tread_with_a_lane_open_is_passed_by_full_lane_keeping() {
    let mut harness = a_drive("Tread Full");
    harness.with_drive(|_, ctx| ctx.settings.lane_keeping = "full".to_string());
    at_mph(&mut harness, 65.0);
    harness.with_drive(|d, _| d.lane.lane = 0);
    arm(&mut harness, tread(OpenSide::Left));
    assert_eq!(
        harness.app.event_lines(),
        vec!["Tire debris ahead. Passing on the left.".to_string()]
    );
    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(1));
    assert_eq!(harness.read_drive(|d| d.aeb_brake), 0.0);
}

#[test]
fn tread_with_a_lane_open_asks_the_steering_driver_for_one_tap() {
    let mut harness = a_drive("Tread Partial");
    harness.with_drive(|_, ctx| ctx.settings.lane_keeping = "partial".to_string());
    at_mph(&mut harness, 65.0);
    arm(&mut harness, tread(OpenSide::Left));
    assert_eq!(
        harness.app.event_lines(),
        vec!["Tire debris ahead. Left lane open.".to_string()]
    );
    assert!(harness.read_drive(|d| d.lane_change_target.is_none()));
}

// -- big debris --------------------------------------------------------------------------

#[test]
fn big_debris_with_no_lane_open_aims_at_30_and_is_taken_for_free_under_it() {
    let mut harness = a_drive("Ladder Eased");
    at_mph(&mut harness, 65.0);
    let before = harness.read_drive(|d| d.trip.truck.damage_pct);
    arm(&mut harness, ladder(OpenSide::Neither));
    assert_eq!(
        harness.app.event_lines(),
        vec!["Brake! A ladder fallen from a truck in the lane. No lane open.".to_string()]
    );
    assert_eq!(
        harness.read_drive(|d| d.hazard_target_mph(None)),
        BIG_DEBRIS_TAKE_MPH
    );
    // The take-at speed sits above the hard-object creep, or the two
    // classes would be told apart by nothing a driver can hear.
    const { assert!(BIG_DEBRIS_TAKE_MPH > HAZARD_CREEP_MPH) };

    harness.clear_speech();
    at_mph(&mut harness, BIG_DEBRIS_TAKE_MPH - 1.0);
    harness.with_drive(|d, ctx| d.update_hazard(ctx, DT));
    assert!(harness.read_drive(|d| d.hazard_deadline.is_none()));
    assert_eq!(harness.read_drive(|d| d.trip.truck.damage_pct), before);
    let heard = harness.app.event_lines();
    assert!(
        heard.iter().any(|l| l == "Clear of the ladder."),
        "{heard:?}"
    );
    assert!(
        !heard.iter().any(|l| l.contains("Nearly stop")),
        "{heard:?}"
    );
}

#[test]
fn big_debris_struck_above_30_costs_speed_scaled_damage_and_the_truck_keeps_rolling() {
    let mut harness = a_drive("Ladder Struck");
    harness.with_drive(|d, ctx| {
        ctx.settings.automatic_emergency_braking = false;
        d.cruise_mph = Some(65.0);
        d.speed_control_armed = true;
    });
    at_mph(&mut harness, 65.0);
    let before = harness.read_drive(|d| d.trip.truck.damage_pct);
    arm(&mut harness, ladder(OpenSide::Neither));
    harness.clear_speech();
    harness.with_drive(|d, ctx| {
        d.hazard_deadline = Some(0.0);
        d.update_hazard(ctx, DT);
    });
    let after = harness.read_drive(|d| d.trip.truck.damage_pct);
    assert!(
        (after - before - 8.0).abs() < 1e-6,
        "8 at 65 for a ladder: {}",
        after - before
    );
    let speed = harness.read_drive(|d| d.trip.truck.speed_mph());
    assert!(speed > 55.0, "a bump, not a wreck: {speed:.1}");
    // The no-lane call paused cruise, as every brake call does; the strike
    // itself does not cancel it, so the session is still armed to resume.
    assert!(
        harness.read_drive(|d| d.speed_control_armed),
        "cruise stays armed"
    );
    assert_eq!(
        harness.app.event_lines(),
        vec![format!(
            "Collision with the ladder. Damage {after:.0} percent."
        )]
    );
    assert!(harness.read_drive(|d| d.hazard_deadline.is_none()));
}

#[test]
fn big_debris_struck_just_over_30_costs_a_point() {
    let mut harness = a_drive("Ladder Grazed");
    harness.with_drive(|_, ctx| ctx.settings.automatic_emergency_braking = false);
    at_mph(&mut harness, 33.0);
    let before = harness.read_drive(|d| d.trip.truck.damage_pct);
    arm(&mut harness, ladder(OpenSide::Neither));
    harness.with_drive(|d, ctx| {
        d.hazard_deadline = Some(0.0);
        d.update_hazard(ctx, DT);
    });
    let pct = harness.read_drive(|d| d.trip.truck.damage_pct) - before;
    assert!((pct - 1.0).abs() < 1e-6, "{pct}");
}

#[test]
fn tread_folding_into_a_ladder_is_taken_as_the_ladder() {
    let mut harness = a_drive("Folded Debris");
    at_mph(&mut harness, 65.0);
    arm(&mut harness, ladder(OpenSide::Neither));
    arm(&mut harness, tread(OpenSide::Neither));
    assert_eq!(
        harness.read_drive(|d| d.hazard_debris),
        Some(DebrisClass::Big)
    );
    assert_eq!(
        harness.read_drive(|d| d.hazard_target_mph(None)),
        BIG_DEBRIS_TAKE_MPH
    );
    assert_eq!(harness.read_drive(|d| d.hazard_strike_pct), 8.0);
}

// -- hard objects ------------------------------------------------------------------------

#[test]
fn a_hard_object_keeps_the_near_stop_and_the_full_collision() {
    let mut harness = a_drive("Rockfall");
    harness.with_drive(|_, ctx| ctx.settings.automatic_emergency_braking = false);
    at_mph(&mut harness, 65.0);
    arm(&mut harness, rockfall());
    assert_eq!(harness.read_drive(|d| d.hazard_debris), None);
    assert_eq!(
        harness.read_drive(|d| d.hazard_target_mph(None)),
        HAZARD_CREEP_MPH
    );
    harness.clear_speech();
    harness.with_drive(|d, ctx| {
        d.hazard_deadline = Some(0.0);
        d.update_hazard(ctx, DT);
    });
    let speed = harness.read_drive(|d| d.trip.truck.speed_mph());
    // The harness drives at a tamer collision tuning than the live game,
    // so the bound is loose; what matters is that it is a wreck, not a bump.
    assert!(speed < 40.0, "the wreck stops the truck: {speed:.1}");
    let heard = harness.app.event_lines();
    assert!(
        heard.iter().any(|l| l.starts_with("Collision! Damage")),
        "{heard:?}"
    );
}

#[test]
fn no_spoken_line_says_nearly_stop_any_more() {
    // The hint went with the near stop for debris (owner, 2026-11-09). A
    // copy test, so it cannot come back under another trigger.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("src is readable") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = fs::read_to_string(&path).expect("a source file");
                assert!(
                    !text.contains("\"It is still in your lane. Nearly stop"),
                    "{} still has the retired hint line",
                    path.display()
                );
            }
        }
    }
}
