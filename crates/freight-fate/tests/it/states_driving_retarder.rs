//! The engine brake: the dash switch and its cylinder selector
//! (`states/driving_controls/vehicle.rs`), and the curve assist's rule about
//! when a retarder is the right tool at all
//! (`states/driving_updates/lanes.rs`).
//!
//! Ported from `tests/test_driving_features.py`: the jake block
//! (`test_engine_brake_cannot_be_enabled_while_accelerating` through
//! `test_accelerating_turns_engine_brake_off`) and the curve-assist block
//! (`test_curve_assist_takes_corners_on_the_drums_and_grades_on_the_jake`
//! through `test_curve_assist_does_not_guess_the_retarder_without_an_advisory`),
//! plus `test_curve_assist_cues_do_not_thrash`.
//!
//! Python's `_AssistRig` monkeypatches three trip lookups -- `curve_at`,
//! `grade_at` and `engine_brake_ban_at`. None of those seams exist here and
//! all three are arranged for real instead: the bend is a real `RouteCurve`
//! pushed onto `trip.curves` (which is what `curve_at` reads), the grade is a
//! real baked `GradeSegment` on the leg (which is what `grade_at` reads), and
//! the engine-brake ban answers to how far the truck is from a route city --
//! so the bench road puts it twenty miles out, where no town's ordinance
//! applies and `engine_brake_ban_at` genuinely returns nothing.

use ff_core::data::curves::RouteCurve;
use ff_core::data::world::get_world;
use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::REFERENCE_CARGO_KG;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

use freight_fate::playtest::breaker::force_grade;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Mods};
use freight_fate::states::driving::DrivingState;

const DT: f64 = 1.0 / 60.0;
const MPH_PER_MPS: f64 = 2.23694; // the constant the Python cases divide by
const ROAD_MI: f64 = 40.0;
const START_MI: f64 = 20.0; // dead centre: no town's engine-brake ban reaches here

// -- rigging -------------------------------------------------------------------------

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
    });
    harness.clear_speech();
    harness
}

/// Swap the drive onto a straight bench road with a known grade, one real
/// bend under the truck, and no town within the urban radius.
///
/// `advisory` and `grade_pct` are `_AssistRig`'s two knobs.
fn bench_bend(drive: &mut DrivingState, advisory_mph: i64, grade_pct: f64) {
    let world = get_world();
    let city = drive.trip.route.cities[0].clone();
    let detail = CorridorDetail {
        grade_segments: vec![GradeSegment::new(
            0.0,
            ROAD_MI,
            grade_pct,
            if grade_pct.abs() >= 3.0 {
                "mountain"
            } else {
                "flat"
            },
            "test bench",
        )],
        ..Default::default()
    };
    let leg = Leg::new(&city, &city, ROAD_MI, "I 90", "flat", Vec::new()).with_detail(detail);
    let route = Route::from_legs(vec![city.clone(), city], vec![leg]);
    let truck = drive.trip.truck.clone();
    let mut weather = WeatherSystem::new("heartland", Some(3), None, None, true);
    weather.current = WeatherKind::Clear;
    let mut trip = Trip::new(
        route,
        truck,
        weather,
        TripOptions {
            seed: Some(3),
            time_scale: 1.0,
            world: Some(world),
            ..Default::default()
        },
    );
    trip.set_npc_vehicles(Vec::new());
    trip.traffic_manager.rolling_bubble = false;
    trip.hazard_check_mi = 1e9;
    trip.inspection_check_mi = 1e9;
    trip.zones.clear();
    trip.set_patrols(Vec::new());
    trip.position_mi = START_MI;
    // One bend, right where the truck is. `curve_at` is a lookup over this
    // list, so a real record here IS the monkeypatched `curve_at`.
    trip.curves = vec![RouteCurve {
        start_mi: START_MI,
        apex_mi: START_MI + 0.1,
        end_mi: START_MI + 0.2,
        direction: 'L',
        advisory_mph,
        min_radius_ft: 1500,
        deflection_deg: 30.0,
        connector: false,
    }];
    drive.trip = trip;
    drive.reset_turn_state_for_trip();
    drive.destination_exit_taken = true;
    assert!(
        drive.trip.engine_brake_ban_at(START_MI).is_none(),
        "the bench road has to be clear of any town engine-brake ordinance"
    );
    let truck = drive.truck_mut();
    truck.set_air_ready(false);
    truck.start_engine();
    truck.transmission.automatic = true;
    truck.transmission.gear = 9;
    truck.rpm = 1500.0;
    truck.throttle = 0.0;
    truck.grip = 1.0;
    truck.grade = grade_pct / 100.0;
    // PIN THE LOAD. Whether an assist may reach for the retarder is a question
    // about weight as much as about slope -- `retarder_warranted` asks whether
    // the service brakes could hold this hill, and a grossed-out truck reaches
    // for it past four percent where an empty one does not until past nine.
    // `StartDelivery::named` draws a job per driver NAME, so leaving the
    // cargo alone let the draw decide which of these cases had a retarder to
    // talk about: two cases with identical roads disagreed purely on the name
    // over them (2026-08-24). Grossed out is the truck the retarder exists for.
    truck.cargo_kg = REFERENCE_CARGO_KG;
}

/// `_AssistRig`: the bench bend with the curve assist switched on.
fn assist_rig(name: &str, advisory_mph: i64, grade_pct: f64) -> PlaytestHarness {
    let mut harness = a_drive(name);
    harness.app.ctx.settings.curve_speed_assist = true;
    harness.with_drive(move |drive, _| bench_bend(drive, advisory_mph, grade_pct));
    harness.clear_speech();
    harness
}

/// `_AssistRig.drive(mph)`: one frame at `mph`, from a fresh assist episode.
fn assist_frame(harness: &mut PlaytestHarness, mph: f64) {
    harness.advance_clock(DT);
    harness.with_drive(move |drive, ctx| {
        drive.curve_assist_active = false;
        drive.truck_mut().velocity_mps = mph / MPH_PER_MPS;
        drive.truck_mut().brake = 0.0;
        drive.update_lane(ctx, DT);
    });
}

fn jake_on(harness: &PlaytestHarness) -> bool {
    harness.read_drive(|d| d.truck().engine_brake())
}

fn service_brake(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.truck().brake)
}

fn throttle(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.truck().throttle)
}

fn assist_jake(harness: &PlaytestHarness) -> bool {
    harness.read_drive(|d| d.curve_assist_jake)
}

/// One frame of the same, CONTINUING assist episode.
///
/// [`assist_frame`] clears `curve_assist_active` first, which starts a fresh
/// episode every frame -- exactly what the ported cases wanted. The grade
/// under a bend changes while the truck is still IN the bend, so the cases
/// below need the opposite: one episode that outlives the grade that bought
/// it.
fn same_bend_frame(harness: &mut PlaytestHarness, mph: f64) {
    harness.advance_clock(DT);
    harness.with_drive(move |drive, ctx| {
        drive.truck_mut().velocity_mps = mph / MPH_PER_MPS;
        drive.truck_mut().brake = 0.0;
        drive.update_lane(ctx, DT);
    });
}

/// Re-grade the road under the truck without touching the bend, the position,
/// or the assist's episode -- the one thing these cases change.
fn regrade(harness: &mut PlaytestHarness, grade: f64) {
    harness.with_drive(move |drive, _| {
        force_grade(&mut drive.trip, grade);
        drive.truck_mut().grade = grade;
    });
}

// -- the dash switch -------------------------------------------------------------------

#[test]
fn test_engine_brake_cannot_be_enabled_while_accelerating() {
    let mut harness = a_drive("Jake Accel");
    harness.with_drive(|drive, ctx| {
        drive.truck_mut().set_engine_brake(false);
        drive.truck_mut().throttle = 0.4;
        ctx.input.press(Key::Up, Mods::NONE);
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::J, None)));

    assert!(!jake_on(&harness));
    let spoken = harness.app.main_lines();
    assert!(
        spoken.iter().any(|t| t.contains("Release the accelerator")),
        "{spoken:#?}"
    );
}

/// Cruise pulling up to a crest is not the driver's foot: the downgrade
/// call asks for J right there (agent drive, I-70, 2026-09-25).
#[test]
fn test_engine_brake_arms_while_cruise_holds_the_throttle() {
    let mut harness = a_drive("Jake Cruise");
    harness.with_drive(|drive, _| {
        drive.truck_mut().set_engine_brake(false);
        drive.auto_jake = false;
        drive.truck_mut().throttle = 0.6;
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::J, None)));

    let armed = harness.read_drive(|d| d.trip.truck.engine_brake() || d.auto_jake);
    let spoken = harness.app.main_lines();
    assert!(armed, "{spoken:#?}");
    assert!(
        !spoken.iter().any(|t| t.contains("Release the accelerator")),
        "{spoken:#?}"
    );
}

#[test]
fn test_jake_engages_at_last_selected_stage() {
    // J is the dash switch; 1/2/3 the cylinder selector it remembers.
    let mut harness = a_drive("Jake Stage");
    harness.with_drive(|drive, _| {
        drive.truck_mut().throttle = 0.0;
        // The remembered-stage behavior is the manual-box stalk; an automatic
        // box arms retarder management instead (its own test).
        drive.truck_mut().transmission.automatic = false;
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::J, None)));
    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 3);
    assert!(
        harness
            .app
            .main_lines()
            .iter()
            .any(|t| t == "Jake on, stage three."),
        "{:#?}",
        harness.app.main_lines()
    );

    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::Num1, Some('1'))));
    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 1);
    assert!(
        harness
            .app
            .main_lines()
            .iter()
            .any(|t| t == "Jake stage one selected."),
        "{:#?}",
        harness.app.main_lines()
    );

    // Off and back on: the selector held stage one, so the icy descent is
    // never surprised by full retard it dialed away earlier.
    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::J, None)));
    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 0);
    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::J, None)));
    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 1);
}

#[test]
fn test_jake_stage_keys_do_nothing_while_the_jake_is_off() {
    let mut harness = a_drive("Jake Off Stage");
    harness.with_drive(|drive, _| drive.truck_mut().engine_brake_stage = 0);
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::Num2, Some('2'))));

    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 0);
    assert!(
        !harness
            .app
            .main_lines()
            .iter()
            .any(|t| t.starts_with("Jake stage")),
        "{:#?}",
        harness.app.main_lines()
    );
}

#[test]
fn test_accelerating_turns_engine_brake_off() {
    let mut harness = a_drive("Jake Accel Off");
    harness.app.ctx.input.press(Key::Up, Mods::NONE);
    harness.with_drive(|drive, _| {
        drive.truck_mut().set_engine_brake(true);
        drive.truck_mut().set_air_ready(false);
    });
    harness.clear_speech();

    harness.advance_clock(DT);
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, DT));

    assert!(!jake_on(&harness));
    assert!(harness.read_drive(|d| d.truck().throttle) > 0.0);
    assert!(
        harness.app.event_lines().contains(&"Jake off.".to_string()),
        "{:#?}",
        harness.app.event_lines()
    );
}

// -- the curve assist's choice of tool --------------------------------------------------

#[test]
fn test_curve_assist_takes_corners_on_the_drums_and_grades_on_the_jake() {
    let mut harness = a_drive("Drums And Jake");
    harness.app.ctx.settings.curve_speed_assist = true;
    // Level ground, open road: the assist's own rule is what is on trial
    // here, not the grade exemption or a town's posted sign.
    harness.with_drive(|drive, _| bench_bend(drive, 40, 0.0));
    harness.clear_speech();

    // Level road, seven over the advisory: not speed worth barking for. The
    // drums take that one, quietly.
    assist_frame(&mut harness, 21.0 * MPH_PER_MPS); // ~47 mph vs 40 advisory
    assert!(!jake_on(&harness));
    assert!(!assist_jake(&harness));
    assert!(service_brake(&harness) > 0.0);

    // Fourteen over on the level is still the drums. The corner does not buy
    // the retarder at any overspeed: a target speed wants the precise control
    // only the service brakes give, and a retarder drives the rear tractor
    // wheels alone -- the last axle to retard through a bend.
    assist_frame(&mut harness, 54.0);
    assert!(!jake_on(&harness));
    assert!(!assist_jake(&harness));
    assert!(service_brake(&harness) > 0.0);

    // Low grip changes nothing on the level, and must not: a jake on ice
    // breaks the drives loose, which is why the grade case below still checks
    // grip.
    harness.with_drive(|drive, _| {
        drive.truck_mut().engine_brake_stage = 0;
        drive.truck_mut().grip = 0.4;
    });
    assist_frame(&mut harness, 54.0);
    assert!(!jake_on(&harness), "no jake on ice");
    assert!(
        service_brake(&harness) > 0.0,
        "gentle service braking instead"
    );

    // Tip the same bend downhill and the retarder does come out -- that is
    // the grade's doing, not the corner's, and it is the one job the engine
    // brake exists for.
    harness.with_drive(|drive, _| {
        bench_bend(drive, 40, -6.0);
        drive.truck_mut().grip = 1.0;
        drive.curve_assist_jake = false;
        drive.truck_mut().engine_brake_stage = 0;
    });
    assist_frame(&mut harness, 54.0);
    assert!(assist_jake(&harness));
    assert!(jake_on(&harness));

    // When the assist's own jake episode ends, it releases the jake -- but
    // only the one IT engaged.
    assist_frame(&mut harness, 54.0);
    assert!(assist_jake(&harness));
    harness.with_drive(|drive, ctx| {
        drive.trip.curves.clear(); // the bend is behind the truck now
        drive.update_lane(ctx, DT);
    });
    assert!(!assist_jake(&harness));
    assert!(!jake_on(&harness));
}

#[test]
fn test_curve_assist_leaves_the_jake_alone_for_a_gentle_bend() {
    // A sweeper the truck is a few mph over is the drums' work, not the
    // jake's. Tester report, 2026-08-11: the assist barked through every
    // mapped bend it handled -- 22 engagements in 58 miles of Arizona
    // mountain road, ten of them for seven mph over. Nobody drives that way,
    // and the engine brake is the one device a town can fine you for using.
    let mut harness = assist_rig("Gentle Bend", 55, 0.0);

    assist_frame(&mut harness, 62.0); // seven over a highway sweeper

    assert!(!jake_on(&harness));
    assert!(!assist_jake(&harness));
    // The assist still slows the truck, quietly.
    assert!(service_brake(&harness) > 0.0);
}

#[test]
fn test_a_corner_never_raises_the_retarder_however_fast_you_take_it() {
    // On the level the corner is the drums' work, at any overspeed. The CDL
    // rule for a curve is to be at a safe speed BEFORE it and pull through on
    // gentle throttle, because braking mid-corner is what locks a wheel and
    // jackknifes a trailer -- and a retarder drives the tractor's rear wheels
    // only. So no amount of overspeed buys the jake here (owner ruling
    // 2026-08-11, after a tester kept hearing it through bends).
    let mut harness = assist_rig("Any Overspeed", 45, 0.0);

    for speed in [55.0, 57.0, 63.0, 80.0] {
        harness.with_drive(|drive, _| drive.curve_assist_jake = false);
        assist_frame(&mut harness, speed);
        assert!(
            !jake_on(&harness),
            "retarder came out at {speed} on the level"
        );
        assert!(!assist_jake(&harness));
        // ...and the assist is still slowing the truck, on the drums.
        assert!(service_brake(&harness) > 0.0);
    }
}

#[test]
fn test_curve_assist_jakes_a_bend_on_a_real_downgrade() {
    // Downhill, the overspeed line does not apply: holding a loaded truck
    // back on a grade is the retarder's own job, and the one use every town
    // noise ordinance leaves legal.
    let mut harness = assist_rig("Downgrade Bend", 45, -6.0);

    assist_frame(&mut harness, 52.0); // only seven over, but six percent down

    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 2);
    assert!(assist_jake(&harness));
}

#[test]
fn test_curve_assist_does_not_pump_the_drums_at_ten_over() {
    // With the retarder slowing the truck, the drums join the curve assist
    // from ten over. Switched at ten, a truck riding that line had the pedal
    // pressed and released on alternate frames, and every application costs
    // air (the pedal-fanning class, 2026-09-24).
    let mut harness = assist_rig("Ten Over", 45, -6.0);
    assist_frame(&mut harness, 52.0); // raise the assist's jake
    assert!(assist_jake(&harness));
    let mut rises = 0.0;
    let mut last_brake = 0.0;
    for step in 0..60 {
        let mph = if step % 2 == 0 { 54.9 } else { 55.1 };
        same_bend_frame(&mut harness, mph);
        let brake = service_brake(&harness);
        rises += (brake - last_brake).max(0.0);
        last_brake = brake;
    }
    assert!(rises < 2.0, "the pedal rose {rises:.2} full applications");
}

#[test]
fn test_curve_assist_holds_a_long_downgrade_on_the_jake_not_the_drums() {
    // The safety half of the same fix, in real physics. A threshold with no
    // downhill carve-out left the assist holding a six percent descent on the
    // service brakes alone: past fade in four and a half minutes, 585 degrees
    // at ten (bench trace, 2026-08-11). Cooking the drums on a mountain is a
    // worse bug than a noisy sweeper.
    let mut harness = assist_rig("Long Downgrade", 45, -6.0);
    harness.with_drive(|drive, _| {
        drive.truck_mut().velocity_mps = 48.0 / MPH_PER_MPS; // under the engage line
    });

    let mut retarded = false;
    for _ in 0..(60 * 60) {
        // a minute of frames down the grade
        harness.advance_clock(DT);
        retarded |= harness.with_drive(|drive, ctx| {
            drive.truck_mut().grade = -0.06; // the trip normally stamps this each frame
            drive.truck_mut().brake = 0.0; // no pedal: the assist is the only thing braking
            drive.update_lane(ctx, DT);
            drive.truck_mut().update(DT);
            drive.truck().engine_brake()
        });
    }

    assert!(retarded, "the retarder did the holding");
    harness.read_drive(|d| {
        let truck = d.truck();
        assert!(
            truck.brake_temp_c < truck.brake_fade_onset_c(),
            "the drums stayed cool: {} vs {}",
            truck.brake_temp_c,
            truck.brake_fade_onset_c()
        );
    });
}

#[test]
fn test_curve_assist_does_not_guess_the_retarder_without_an_advisory() {
    // No baked advisory, no measurement, no bark. The ramp fallback has no
    // advisory speed to measure an overspeed against. It used to assume ten
    // mph over and engage stage two on that assumption -- reaching for a
    // loud, town-restricted device on no evidence at all.
    let mut harness = assist_rig("No Advisory", 45, 0.0);
    harness.with_drive(|drive, _| {
        drive.trip.curves.clear(); // no baked bend to measure against
        drive.ramp_mi = Some(0.3); // on a ramp: the old terrain heuristic answers
    });

    assist_frame(&mut harness, 55.0);

    assert!(!jake_on(&harness));
    assert!(!assist_jake(&harness));
    // The ramp still gets slowed -- by lifting, not by the drums. Route
    // transition assistance owns a ramp it is running (it is on by default),
    // and holding a service floor all the way down spent reservoir air, so
    // the cap lifts the throttle and lets drag shed the excess. The real
    // stop at the bar still uses the brakes.
    assert_eq!(throttle(&harness), 0.0);
    assert_eq!(service_brake(&harness), 0.0);
}

#[test]
fn test_curve_assist_cues_do_not_thrash() {
    // Speed hovering at the assist threshold speaks one cue, not a chant.
    // Regression for the 2026-07-22 playtest: cruise fighting the curve brake
    // crossed the single engage threshold every few frames, and each crossing
    // spoke -- 23 slowing/released cues in about four seconds.
    //
    // The spoken count alone would not prove much here: the port's real
    // delivery layer drops an identical repeat at the pacer, so a thrashing
    // assist could still read as one line. What the hysteresis actually owns
    // is the LATCH, so the engage/release transitions are counted too, and
    // that count cannot be masked by anything downstream.
    let mut harness = assist_rig("No Thrash", 35, 0.0);
    let mut engagements = 0;
    let mut releases = 0;
    let mut was_active = harness.read_drive(|d| d.curve_assist_active);

    // Two seconds of speed flapping across the old single threshold
    // (advisory + 5 = 40): the old code spoke on every crossing.
    for _ in 0..60 {
        for mph in [41.0, 39.5] {
            harness.advance_clock(DT);
            let active = harness.with_drive(move |drive, ctx| {
                drive.truck_mut().velocity_mps = mph / MPH_PER_MPS;
                drive.update_lane(ctx, DT);
                drive.curve_assist_active
            });
            if active && !was_active {
                engagements += 1;
            }
            if !active && was_active {
                releases += 1;
            }
            was_active = active;
        }
    }

    let cues: Vec<String> = harness
        .app
        .event_lines()
        .into_iter()
        .filter(|line| line.contains("Curve assistance") || line.starts_with("Slowing for"))
        .collect();
    assert_eq!(cues, vec!["Slowing for the bend.".to_string()]);
    assert_eq!(engagements, 1, "the assist engaged more than once");
    assert_eq!(releases, 0, "the assist let go inside its own hysteresis");
}

// -- the grade under the bend, not the bend --------------------------------------------

#[test]
fn test_the_curve_assist_hands_the_retarder_back_when_the_grade_under_the_bend_ends() {
    // A bend is a stretch of road, not a moment, and the grade under it
    // changes while the truck is still in it.
    //
    // The release only ever ran when the CORNER ended, so a bend that started
    // on a pitch and finished on the flat kept whatever the pitch bought for
    // the whole corner: 6.9 seconds of retarder on level road, per bend
    // (jake sweep, 2026-08-24). Nothing about the corner has changed here --
    // same bend, same overspeed -- only the grade that bought the retarder.
    let mut harness = assist_rig("Bend Off The Grade", 30, -6.0);

    same_bend_frame(&mut harness, 60.0);
    assert!(assist_jake(&harness), "the pitch did not buy the retarder");
    assert!(jake_on(&harness));

    regrade(&mut harness, 0.0);
    same_bend_frame(&mut harness, 60.0);

    assert!(
        !assist_jake(&harness),
        "the assist kept the retarder onto level road"
    );
    assert!(!jake_on(&harness));
    // The corner still gets slowed -- on the drums, which is its own tool.
    assert!(service_brake(&harness) > 0.0);
}

#[test]
fn test_the_curve_assist_hands_the_retarder_back_when_the_road_turns_up() {
    // The owner's report, 2026-08-24: "on uphill ascents the truck should gain
    // speed instead of using the engine brakes." A bend out of a dip is where
    // that happened -- the same episode, the road climbing under it, and the
    // retarder still barking at a hill the truck has to get up.
    let mut harness = assist_rig("Bend Into The Climb", 30, -6.0);

    same_bend_frame(&mut harness, 60.0);
    assert!(assist_jake(&harness));

    regrade(&mut harness, 0.04);
    same_bend_frame(&mut harness, 60.0);

    assert!(
        !assist_jake(&harness),
        "the assist barked the retarder at a four percent climb"
    );
    assert!(!jake_on(&harness));
}

#[test]
fn test_the_truck_climbs_out_of_the_bend_faster_without_the_retarder() {
    // The same climb, driven twice: once as the truck behaves now, and once
    // with the retarder pinned up the way the old release rule left it. The
    // difference is the speed the driver keeps for the hill.
    //
    // Both runs are the same seeded bench road at the same weight, off the
    // throttle, with only the retarder differing -- so the gap between them is
    // the retarder's doing and nothing else's.
    fn climb_out(name: &str, hold_the_retarder: bool) -> f64 {
        let mut harness = assist_rig(name, 30, -6.0);
        same_bend_frame(&mut harness, 60.0);
        assert!(assist_jake(&harness), "the pitch has to buy the retarder");
        regrade(&mut harness, 0.04);
        // Ten seconds of the climb, hands off, the assist doing whatever it
        // does with the retarder.
        for _ in 0..(10 * 60) {
            harness.advance_clock(DT);
            harness.with_drive(move |d, ctx| {
                d.truck_mut().brake = 0.0;
                d.truck_mut().throttle = 0.0;
                d.update_lane(ctx, DT);
                if hold_the_retarder {
                    // What the old rule did: the corner has not ended, so the
                    // stage the pitch bought stays up the whole way.
                    d.curve_assist_jake = true;
                    d.truck_mut().engine_brake_stage = 2;
                }
                d.truck_mut().auto_shift();
                d.truck_mut().update(DT);
            });
        }
        harness.read_drive(|d| d.truck().speed_mph())
    }

    let held = climb_out("Climb Retarded", true);
    let released = climb_out("Climb Free", false);
    assert!(
        released > held + 2.0,
        "the retarder cost nothing on the climb: {released:.1} against {held:.1}"
    );
    println!("climb out of the bend: {held:.1} mph retarded, {released:.1} mph released");
}
