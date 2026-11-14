//! Resuming (or setting) cruise to a high target must ease up, not floor it
//! (port of `tests/test_cruise_resume_ramp.py`).
//!
//! The tester report (Shane): Shift+K resumes cruise to a high remembered
//! target (85 mph) from low road speed. The old proportional loop saw the
//! whole error at once and commanded wide-open throttle. On flat ground the
//! fuel governor caps that -- loud, but harmless. On a downgrade cruise adding
//! fuel while gravity is already driving the engine toward redline is what fed
//! the over-rev during the automatic box's between-shift hold, "the engine is
//! screaming at redline".
//!
//! The fix has three parts, each covered here:
//!
//! * a working setpoint that eases from the engage speed up to the target at a
//!   bounded rate, so a big resume error never lands on the pedal at once;
//! * an RPM ceiling that tapers cruise's throttle to nothing as the engine
//!   nears the governor, so cruise never feeds an over-rev -- the
//!   descent-control and retarder staging own the grade;
//! * an engage gate, so on the open road cruise waits for road speed before it
//!   engages, the same bridge the zone-preceded automatic resume already gave
//!   the tester a behaviour he trusts.
//!
//! Note on scope: a truck left to coast an unbraked 12 percent grade up from a
//! near standstill over-revs on gravity alone, threading up through the gears
//! -- that is descent-control territory, not cruise, and it happens whether
//! cruise is off the throttle or not. These tests pin what cruise itself does:
//! at a realistic rolling resume it never redlines and charges no over-rev
//! wear, and it is off the throttle near the governor.
//!
//! # The road these run on
//!
//! Python reached a deterministic road with four `monkeypatch.setattr` calls
//! on the live trip: `speed_limit_at` (a flat 200, so the posted number never
//! caps cruise), `grade_at` (one constant), `traffic_context` (None) and, for
//! the exit cases, `ramp_speed_at`. Rust has no seam for an inherent method,
//! so [`bench_road`] builds the road that ANSWERS that way instead: a baked
//! `SpeedLimitSample` is exactly what `speed_limit_at` reads, a baked
//! `GradeSegment` is what `grade_at` reads, and an empty traffic manager with
//! the rolling bubble off is what makes `traffic_context` None. That is a
//! stronger rig than the patches were, because the whole trip is now fixed
//! rather than whatever route dispatch drew.

use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route, SpeedLimitSample};
use ff_core::sim::trip::{Trip, TripOptions, EXIT_APPROACH_RELEASE_S};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{EXIT_MAINLINE_EASE_MPH, RAMP_MAX_MPH};

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

fn approx_abs(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// One long straight leg with a baked posted limit and a baked grade, and
/// nothing else on it. See the module note for why this replaces four
/// monkeypatches.
fn bench_road(drive: &mut DrivingState, limit_mph: f64, grade_pct: f64, time_scale: f64) {
    let city = drive.trip.route.cities[0].clone();
    let miles = 400.0;
    let detail = CorridorDetail {
        speed_limits: vec![SpeedLimitSample {
            at_mi: 0.0,
            mph: Some(limit_mph),
            source: "test bench".to_string(),
            hgv: false,
        }],
        grade_segments: vec![GradeSegment::new(
            0.0,
            miles,
            grade_pct,
            "flat",
            "test bench",
        )],
        ..Default::default()
    };
    let leg = Leg::new(&city, &city, miles, "I 90", "flat", Vec::new()).with_detail(detail);
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
            time_scale,
            ..Default::default()
        },
    );
    trip.set_npc_vehicles(Vec::new());
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_pressures.clear();
    trip.hazard_check_mi = 1e9;
    trip.inspection_check_mi = 1e9;
    trip.zones.clear();
    // A real bend rightly caps cruise; that is not what any case here is
    // about (Python's `driving.trip.curves = []`).
    trip.curves.clear();
    drive.trip = trip;
    drive.reset_turn_state_for_trip();
    drive.destination_exit_taken = true;
}

/// `start_drive(app)` plus a bench road. Returns the harness at the wheel.
fn a_drive(limit_mph: f64, grade_pct: f64, time_scale: f64, lane_keeping: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Cruise Resume"));
    harness.app.ctx.settings.time_scale = time_scale;
    // Set before anything arms an exit: the signal cue and the automatic move
    // into the exit lane are both decided when X is pressed.
    harness.app.ctx.settings.lane_keeping = lane_keeping.to_string();
    harness.with_drive(move |d, _| {
        d.departure_checked = true;
        bench_road(d, limit_mph, grade_pct, time_scale);
        d.truck_mut().set_air_ready(false);
    });
    harness
}

/// One frame of the drive (`driving.update(dt)`).
fn frame(harness: &mut PlaytestHarness, dt: f64) {
    harness.advance_clock(dt);
    harness.with_drive(move |d, ctx| d.update_frame(ctx, dt));
}

/// The keyword arguments of `_arm_high_target`.
struct HighTarget {
    automatic: bool,
    grade: f64,
    speed_mph: f64,
    gear: i32,
    cargo_kg: Option<f64>,
}

/// `_arm_high_target`: arm a session, remember an 85 mph target, and Shift+K
/// resume it. The drive comes back rolling at `speed_mph` on `grade` with the
/// resume just requested, ready for the caller to step frames.
fn arm_high_target(opts: HighTarget) -> PlaytestHarness {
    // The 200 mph posting is Python's `open_limits`: lift the posted number
    // out of the way so the hold is isolated from the predictive-ACC cap.
    let mut harness = a_drive(200.0, opts.grade * 100.0, 1.0, "off");
    harness.app.ctx.settings.automatic_transmission = opts.automatic;
    harness.app.ctx.settings.descent_speed_control = "balanced".to_string();
    harness.press_key(Key::E, None); // engine on
    harness.with_drive(move |d, _| {
        d.truck_mut().transmission.automatic = opts.automatic;
        if let Some(cargo_kg) = opts.cargo_kg {
            d.truck_mut().cargo_kg = cargo_kg;
        }
        d.truck_mut().grade = opts.grade;
        d.truck_mut().transmission.gear = opts.gear;
        d.truck_mut().velocity_mps = opts.speed_mph * MPS_PER_MPH;
        // A remembered open-road target the way a braked-away cruise leaves it.
        d.resume_target_mph = Some(85.0);
        assert!(d.cruise_mph.is_none() && d.keeper_mph.is_none());
    });
    harness.with_drive(|d, ctx| {
        d.handle_key_event(
            ctx,
            &freight_fate::states::base::InputEvent::KeyDown {
                key: Key::K,
                mods: Mods::SHIFT,
                text: None,
                repeat: false,
            },
        )
    });
    assert!(harness.read_drive(|d| d.speed_control_armed));
    harness
}

#[test]
fn test_resume_eases_up_and_reaches_the_target_on_the_flat() {
    // Flat ground: the working setpoint climbs toward 85 gradually rather than
    // the whole error landing on the loop at once, and given time the truck
    // does reach and hold the number.
    let mut harness = arm_high_target(HighTarget {
        automatic: true,
        grade: 0.0,
        speed_mph: 21.0,
        gear: 6,
        cargo_kg: Some(0.0),
    });
    // First frame engages cruise; the working setpoint starts near road speed,
    // nowhere near the 85 target.
    frame(&mut harness, 1.0 / 30.0);
    assert!(approx(
        harness
            .read_drive(|d| d.cruise_mph)
            .expect("cruise engaged"),
        85.0
    ));
    let early = harness
        .read_drive(|d| d.cruise_working_mph)
        .expect("a working setpoint");
    assert!(early < 30.0, "{early}");
    // It climbs over the next second rather than snapping to the target.
    for _ in 0..30 {
        frame(&mut harness, 1.0 / 30.0);
    }
    let now = harness
        .read_drive(|d| d.cruise_working_mph)
        .expect("a working setpoint");
    assert!(early < now && now < 85.0, "{early} {now}");
    // And given time, cruise reaches and holds the number, never redlining.
    let mut reached = false;
    for _ in 0..3000 {
        frame(&mut harness, 1.0 / 30.0);
        assert!(!harness.read_drive(|d| d.truck().over_revving()));
        if harness.read_drive(|d| d.truck().speed_mph()) >= 82.0 {
            reached = true;
            break;
        }
    }
    assert!(
        reached,
        "cruise never climbed to the target (stalled at {:.1})",
        harness.read_drive(|d| d.truck().speed_mph())
    );
}

#[test]
fn test_resume_at_road_speed_never_redlines_or_wears() {
    // A rolling resume to 85 -- flat and on a -12% grade, automatic and manual
    // -- never crosses the over-rev threshold and charges no over-rev wear.
    //
    // Python's `@pytest.mark.parametrize` over four rows; one Rust case with
    // the same four, so the Python function name survives the port.
    for (automatic, grade, speed_mph, gear) in [
        (true, 0.0, 21.0, 6),     // automatic, flat
        (true, -0.12, 25.0, 6),   // automatic, steep downgrade, rolling
        (false, 0.0, 21.0, 6),    // manual, flat (governor-capped)
        (false, -0.12, 55.0, 10), // manual, steep downgrade, top gear
    ] {
        let mut harness = arm_high_target(HighTarget {
            automatic,
            grade,
            speed_mph,
            gear,
            // The 15.6 t this case was written against (the new career's old
            // first load). At 21 t the automatic on -12% peaks 7 rpm over;
            // that is on the roadmap, not hidden here.
            cargo_kg: Some(15_583.5),
        });
        let wear_before = harness.read_drive(|d| d.truck().engine_wear_pct);
        let mut max_crpm: f64 = 0.0;
        let mut ever_over = false;
        for _ in 0..600 {
            // ~20 s of frames
            frame(&mut harness, 1.0 / 30.0);
            let (crpm, over) =
                harness.read_drive(|d| (d.truck().coupled_rpm(None), d.truck().over_revving()));
            max_crpm = max_crpm.max(crpm);
            ever_over = ever_over || over;
        }
        let over_thresh = harness.read_drive(|d| d.truck().specs.max_rpm) * 1.05;
        assert!(
            !ever_over,
            "engine over-revved (peak coupled_rpm {max_crpm:.0} > {over_thresh:.0}) \
             at automatic={automatic} grade={grade}"
        );
        // Duty-cycle wear still ticks; the over-rev term (0.8%/s) does not.
        let charged = harness.read_drive(|d| d.truck().engine_wear_pct) - wear_before;
        assert!(charged < 0.05, "{charged}");
    }
}

#[test]
fn test_cruise_backs_off_the_throttle_near_redline() {
    // The belt-and-suspenders ceiling in isolation: with the same setpoint
    // error, cruise commands full throttle when the engine has RPM headroom
    // and next to nothing when the engine is up against the governor -- so on
    // a downgrade, where gravity does the accelerating, cruise never feeds the
    // over-rev.
    //
    // Manual, flat, a mid gear, rolling. The remembered target is far above,
    // so cruise wants throttle throughout -- the only thing that changes
    // between the two probes below is how close coupled RPM sits to redline.
    let mut harness = arm_high_target(HighTarget {
        automatic: false,
        grade: 0.0,
        speed_mph: 50.0,
        gear: 8,
        cargo_kg: None,
    });
    frame(&mut harness, 1.0 / 30.0);
    assert!(approx(
        harness
            .read_drive(|d| d.cruise_mph)
            .expect("cruise engaged"),
        85.0
    ));

    // Near the governor: coupled RPM in the top of the range, big error.
    harness.with_drive(|d, _| {
        d.cruise_working_mph = Some(70.0);
        d.truck_mut().transmission.gear = 8;
        d.truck_mut().velocity_mps = 52.0 * MPS_PER_MPH;
    });
    frame(&mut harness, 1.0 / 60.0);
    let (crpm, max_rpm) =
        harness.read_drive(|d| (d.truck().coupled_rpm(None), d.truck().specs.max_rpm));
    assert!(crpm >= max_rpm * 0.95, "{crpm} {max_rpm}");
    let near_redline_throttle = harness.read_drive(|d| d.cruise_throttle);
    assert!(near_redline_throttle < 0.3, "{near_redline_throttle}");

    // Same gear, same error, but plenty of RPM headroom: full throttle.
    harness.with_drive(|d, _| {
        d.cruise_working_mph = Some(70.0);
        d.truck_mut().transmission.gear = 8;
        d.truck_mut().velocity_mps = 30.0 * MPS_PER_MPH;
    });
    frame(&mut harness, 1.0 / 60.0);
    let (crpm, max_rpm) =
        harness.read_drive(|d| (d.truck().coupled_rpm(None), d.truck().specs.max_rpm));
    assert!(crpm < max_rpm * 0.7, "{crpm} {max_rpm}");
    let throttle = harness.read_drive(|d| d.cruise_throttle);
    assert!(throttle > 0.8, "{throttle}");
}

#[test]
fn test_open_road_resume_waits_for_road_speed_before_engaging_cruise() {
    // From a near standstill on the open road, resume arms the session but
    // holds off engaging cruise until the truck is at cruise's holding speed
    // -- the old resume snapped cruise on at KEEPER_MIN (2 mph) and floored
    // the throttle to chase the high remembered target.
    let mut harness = arm_high_target(HighTarget {
        automatic: true,
        grade: 0.0,
        speed_mph: 6.0,
        gear: 1,
        cargo_kg: None,
    });
    // Well below cruise's floor: armed, but cruise must not engage yet.
    for _ in 0..10 {
        frame(&mut harness, 1.0 / 30.0);
    }
    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    // Bring the truck up past the cruise floor by hand; now it engages, and
    // eased in from road speed rather than floored.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 24.0 * MPS_PER_MPH);
    frame(&mut harness, 1.0 / 30.0);
    assert!(approx(
        harness
            .read_drive(|d| d.cruise_mph)
            .expect("cruise engaged"),
        85.0
    ));
    let working = harness
        .read_drive(|d| d.cruise_working_mph)
        .expect("a working setpoint");
    assert!(working < 40.0, "{working}");
}

// -- the armed exit ---------------------------------------------------------------------

/// `_armed_exit_at`: cruise holding 65 with a route exit armed `ahead_mi` up
/// the road.
///
/// `time_scale` is set on the settings, not the trip: the drive re-reads it
/// from there every frame, so a trip-only assignment lasts exactly one tick.
///
/// Python pinned `trip.ramp_speed_at` to `RAMP_MAX_MPH` because "the assigned
/// route (and so the stop this helper lands on) varies between App
/// instances". The bench road removes that variance at the source -- one leg,
/// one posted number, one stop this helper puts there -- so the ramp speed is
/// derived from the road the way the game derives it, and the assertions read
/// `armed_ramp_cruise_mph()` rather than a constant.
fn armed_exit_at(
    ahead_mi: f64,
    time_scale: f64,
    lane_keeping: &str,
) -> (PlaytestHarness, RoadStop) {
    // A real interstate posting, not the 200 of `open_limits`: the exit cases
    // turn on the ramp's number, which is derived from the road's, so lifting
    // the posting would lift the ramp with it and there would be nothing left
    // to measure. Cruise is set at road speed below, well under 65, so the
    // posted cap is not what holds the truck either way.
    let mut harness = a_drive(65.0, 0.0, time_scale, lane_keeping);
    assert!(approx(harness.app.ctx.settings.time_scale, time_scale));
    harness.press_key(Key::E, None);
    // A stop far enough into the route to stand `ahead_mi` behind it.
    let stop = {
        let mut stop = RoadStop::new("Prairie Travel Center", 40.0, "truck_stop");
        stop.actions = ["park", "fuel", "food"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        stop.parking = "confirmed".to_string();
        stop.exit_label = "exit 42".to_string();
        stop
    };
    assert!(
        stop.at_mi > ahead_mi + 1.0,
        "no route stop sits more than {} miles in",
        ahead_mi + 1.0
    );
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![staged.clone()];
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 65.0 * MPS_PER_MPH;
        d.trip.position_mi = staged.at_mi - ahead_mi;
    });
    assert!(approx_abs(
        stop.at_mi - harness.read_drive(|d| d.trip.position_mi),
        ahead_mi,
        1e-9
    ));
    harness.press_key(Key::K, None); // cruise at road speed
    harness.press_key(Key::X, None); // signal for the exit
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.key())),
        Some(stop.key()),
        "{}",
        harness.transcript_text()
    );
    (harness, stop)
}

/// `_cap_at(driving, stop, ahead_mi)`: the exit cap with the truck `ahead_mi`
/// short of the gore.
fn cap_at(harness: &mut PlaytestHarness, stop: &RoadStop, ahead_mi: f64) -> f64 {
    let at = stop.at_mi - ahead_mi;
    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = at;
        d.update_exit(ctx, 0.0, 0.0); // publishes the approach distance to the clock
    });
    harness
        .read_drive(|d| d.ramp_approach_cap_mph())
        .expect("an armed exit has a cap")
}

#[test]
fn test_shane_2026_08_15_the_ramp_cap_no_longer_lands_miles_from_the_exit() {
    // The tester report this branch exists for.
    //
    // "When taking an exit, the keeper goes to 40 MPH miles away from the
    // exit. It should gradually slow, or at least keep 45 so the exit can be
    // taken. It should measure how far the truck is away from the exit and
    // gradually slow like a driver would."
    //
    // Arming an exit set the ramp target as the cap outright, and an exit arms
    // five miles out at the least -- so automatic control sat at 40 for miles
    // of open interstate. The ramp number is now where the truck has to BE at
    // the gore: the cap is measured off the road still left, holds road speed
    // while there is plenty, and never sits below the speed the ramp needs
    // until the ramp is genuinely close.
    let (mut harness, _stop) = armed_exit_at(4.5, 1.0, "off");
    // The exit's own floor now, not one constant for every exit in the
    // country -- Shane's point was that the cap must not land miles from the
    // exit, and that is what this asserts (owner, 2026-08-21). Ten under road
    // speed at most, since the realistic exit (2026-09-24).
    let armed_ramp = harness.with_drive(|d, _| d.exit_approach_floor_mph(None));
    assert!(
        approx(armed_ramp, 65.0 - EXIT_MAINLINE_EASE_MPH),
        "{armed_ramp}"
    );
    assert!(approx(
        harness.read_drive(|d| d.cruise_exit_mph).expect("a cap"),
        armed_ramp
    ));

    // Four and a half miles out, the cap is not the thing holding the truck:
    // road speed stands, and it is never under ramp speed.
    let far_cap = harness
        .read_drive(|d| d.ramp_approach_cap_mph())
        .expect("a cap");
    assert!(far_cap >= RAMP_MAX_MPH, "{far_cap}");
    assert!(far_cap >= harness.read_drive(|d| d.cruise_mph).expect("cruise"));

    // And the truck really does hold it rather than shedding to 40: five
    // seconds of driving with the exit armed, and it is still at road speed
    // with the brakes off.
    for _ in 0..(60 * 5) {
        frame(&mut harness, 1.0 / 60.0);
    }
    // Named, not dereferenced: a paused session leaves cruise_mph None, and
    // reading through it turned any such failure into a panic instead of
    // saying what went wrong.
    let cruise = harness.read_drive(|d| d.cruise_mph);
    assert!(
        cruise.is_some(),
        "speed control was paused four and a half miles from the exit; armed={} paused_at_stop={}",
        harness.read_drive(|d| d.speed_control_armed),
        harness.read_drive(|d| d.speed_control_paused_at_stop)
    );
    let cruise = cruise.expect("checked");
    assert!(harness.read_drive(|d| d.truck().speed_mph()) > cruise - 3.0);
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
}

#[test]
fn test_the_ramp_cap_glides_down_as_the_exit_closes() {
    // Measured off the distance, the way the report asked: the cap comes down
    // smoothly with the road left, and lands on the exit floor at the gore --
    // ten under road speed, never the ramp's number (realistic exit,
    // 2026-09-24).
    let (mut harness, stop) = armed_exit_at(4.5, 1.0, "off");
    let mut caps = Vec::new();
    for ahead in [4.5, 2.0, 1.0, 0.6, 0.4, 0.2, 0.05] {
        harness.with_drive(move |d, _| d.trip.position_mi = stop.at_mi - ahead);
        caps.push(
            harness
                .read_drive(|d| d.ramp_approach_cap_mph())
                .expect("a cap"),
        );
    }
    let mut sorted = caps.clone();
    sorted.sort_by(|a, b| b.total_cmp(a));
    assert_eq!(caps, sorted, "{caps:?}");
    let ramp_cruise = harness.with_drive(|d, _| d.exit_approach_floor_mph(None));
    assert!(approx(ramp_cruise, 65.0 - EXIT_MAINLINE_EASE_MPH));
    assert!(approx(*caps.last().expect("caps"), ramp_cruise));
    assert!(caps.iter().cloned().fold(f64::INFINITY, f64::min) >= ramp_cruise);
}

#[test]
fn test_shane_2026_08_15_signalling_nine_miles_out_sheds_nothing() {
    // The second tester report on this branch.
    //
    // "If you signal more than 5 miles out you're still slowing down as soon
    // as you signal... I noticed this when I purposely signalled for an exit 9
    // miles before a truck stop."
    //
    // The glide itself was right; the compression handling was not. The cap
    // divided the available road by the effective time scale, so at high
    // pacing it fell under a 65 mph cruise nine miles from the gore and
    // signalling early was itself what slowed the truck. The road is measured
    // in real miles now, and the trip decompresses over the approach so that
    // stays true.
    for time_scale in [1.0, 4.0, 20.0] {
        let (mut harness, stop) = armed_exit_at(4.5, time_scale, "off");
        let cruise = harness.read_drive(|d| d.cruise_mph).expect("cruise");
        for ahead in [9.0, 5.0, 2.0, 1.0] {
            let cap = cap_at(&mut harness, &stop, ahead);
            assert!(cap > cruise, "{time_scale} {ahead} {cap} {cruise}");
        }
        // A quarter mile out the ten-mile ease is under way; it runs from
        // there, not from the moment the signal went on.
        assert!(cap_at(&mut harness, &stop, 0.25) <= cruise);
    }
}

#[test]
fn test_the_ramp_cap_reads_the_same_road_at_every_pacing() {
    // The cap is a fact about the map, not about the clock: it must answer
    // identically at 1x, 4x and 20x. Decompressing the approach is what makes
    // those real miles real.
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for time_scale in [1.0, 4.0, 20.0] {
        let (mut harness, stop) = armed_exit_at(4.5, time_scale, "off");
        rows.push(
            [9.0, 5.0, 2.0, 1.0, 0.5]
                .into_iter()
                .map(|ahead| cap_at(&mut harness, &stop, ahead))
                .collect(),
        );
    }
    for row in &rows[1..] {
        for (a, b) in row.iter().zip(rows[0].iter()) {
            assert!(approx(*a, *b), "{rows:?}");
        }
    }
}

#[test]
fn test_the_exit_approach_runs_on_the_real_clock() {
    // The mechanism: inside the road the shed needs, the trip decompresses the
    // way a hard bend already does, and pacing eases back afterwards instead
    // of snapping.
    let (mut harness, stop) = armed_exit_at(4.5, 20.0, "off");

    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = stop.at_mi - 4.0;
        d.update_exit(ctx, 0.0, 0.0);
    });
    assert!(harness.read_drive(|d| d.trip.effective_time_scale()) > 1.0); // nothing to shed for yet

    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = stop.at_mi - 0.5;
        d.update_exit(ctx, 0.0, 0.0);
    });
    assert!(approx(
        harness.read_drive(|d| d.trip.effective_time_scale()),
        1.0
    ));
    harness.with_drive(|d, _| {
        d.trip.update(1.0 / 60.0);
    });
    assert!(approx(
        harness.read_drive(|d| d.trip.exit_approach_release_s),
        EXIT_APPROACH_RELEASE_S
    ));

    // The exit is cancelled: pacing climbs back rather than snapping.
    harness.with_drive(|d, ctx| {
        d.exit_stop = None;
        d.update_exit(ctx, 0.0, 0.0);
        d.trip.update(EXIT_APPROACH_RELEASE_S / 2.0);
    });
    let eased = harness.read_drive(|d| d.trip.effective_time_scale());
    assert!(eased > 1.0 && eased < 20.0, "{eased}");
    harness.with_drive(|d, _| {
        d.trip.update(EXIT_APPROACH_RELEASE_S);
    });
    assert!(approx(
        harness.read_drive(|d| d.trip.exit_approach_release_s),
        0.0
    ));
    assert!(harness.read_drive(|d| d.trip.effective_time_scale()) > eased);
}

#[test]
fn test_the_truck_still_makes_the_ramp_at_every_pacing() {
    // The constraint the glide must never trade away: whatever the pacing, the
    // truck arrives at the gore slow enough to take the exit -- and gets
    // there.
    //
    // Exit speed assistance used to brake to ramp speed and then hand back an
    // empty pedal, so with automatic speed control paused behind it nothing
    // was driving and the truck coasted to a dead STOP in the through lane a
    // quarter mile short of its own exit. Real-time pacing was the worst case,
    // because the coast had the most seconds to finish. A truck stopped in a
    // live lane short of its exit is worse than any speed it could arrive at,
    // so that is pinned here first.
    for time_scale in [1.0, 4.0, 20.0, 40.0] {
        // The exit lane is not what this test is about; full lane keeping pins
        // those mechanics so the drive turns only on speed.
        let (mut harness, stop) = armed_exit_at(4.5, time_scale, "full");
        let mut entry: Option<f64> = None;
        let mut stopped_at: Option<f64> = None;
        for _ in 0..(60 * 60 * 20) {
            frame(&mut harness, 1.0 / 60.0);
            let (speed, ramp_mi, position) =
                harness.read_drive(|d| (d.truck().speed_mph(), d.ramp_mi, d.trip.position_mi));
            if stopped_at.is_none() && speed <= 0.05 {
                stopped_at = Some(stop.at_mi - position);
            }
            if ramp_mi.is_some() {
                entry = Some(speed);
                break;
            }
            if position > stop.at_mi + 0.5 {
                break;
            }
        }
        assert!(
            stopped_at.is_none(),
            "came to a dead stop {:.2} miles short of the gore at {time_scale}x, \
             in the through lane",
            stopped_at.unwrap_or_default()
        );
        let entry = entry.unwrap_or_else(|| panic!("never took the exit at {time_scale}x: pos={:.2} stop={:.2} speed={:.1} signal={} exit={:?}
{}", harness.read_drive(|d| d.trip.position_mi), stop.at_mi, harness.read_drive(|d| d.truck().speed_mph()), harness.read_drive(|d| d.exit_signal_on), harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.name.clone())), harness.transcript_text()));
        // Slow enough for the gore, which accepts road speed plus the posts'
        // leeway -- and NOT shed to ramp speed in the through lane: the most
        // automatic control takes off on the mainline is ten under the road's
        // 65 (realistic exit, 2026-09-24). The ramp's number is braked for in
        // the deceleration lane, past the gore.
        let accepted = harness.with_drive(move |d, _| d.gore_acceptance_mph(Some(&stop)));
        assert!(entry <= accepted, "{time_scale} {entry} over {accepted}");
        assert!(
            entry >= 65.0 - EXIT_MAINLINE_EASE_MPH - 2.5,
            "{time_scale}: shed to {entry} on the mainline"
        );
        assert!(entry > RAMP_MAX_MPH, "{time_scale}: {entry}");
    }
}

#[test]
fn test_set_at_current_speed_cruise_is_unchanged() {
    // The K-set-at-current-speed path: engaging at 60 with the target at 60
    // seeds the working setpoint at road speed, so there is no ramp artifact
    // and the hold is exactly as before.
    let mut harness = a_drive(200.0, 0.0, 1.0, "off");
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
        d.truck_mut().throttle = 0.35;
    });
    harness.press_key(Key::K, None);
    let cruise = harness
        .read_drive(|d| d.cruise_mph)
        .expect("cruise engaged");
    assert!(approx_abs(cruise, 60.0, 1.0), "{cruise}");
    let (working, speed) = harness.read_drive(|d| {
        (
            d.cruise_working_mph.unwrap_or_default(),
            d.truck().speed_mph(),
        )
    });
    assert!(approx_abs(working, speed, 0.5), "{working} {speed}");
    for _ in 0..(60 * 15) {
        frame(&mut harness, 1.0 / 60.0);
    }
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    assert!((harness.read_drive(|d| d.truck().speed_mph()) - 60.0).abs() < 5.0);
    assert!(!harness.read_drive(|d| d.truck().over_revving()));
}

#[test]
fn ramp_assist_approach_window_uses_real_time() {
    // A truck at road speed is under what the gore accepts, so only the road
    // it needs to shed for the ramp, and the last half mile, run on the real
    // clock. Over what the gore accepts, the exit speed assist's whole mile
    // and a half does (owner's drive into Jamestown, 2026-11-04: the whole
    // window real crawled a truck already near ramp speed to its exit).
    for scale in [1.0, 4.0, 20.0, 40.0] {
        let (mut harness, stop) = armed_exit_at(4.5, scale, "off");
        let pacing_at = |harness: &mut PlaytestHarness, ahead: f64, mph: f64| {
            harness.with_drive(move |d, ctx| {
                d.truck_mut().velocity_mps = mph * MPS_PER_MPH;
                d.trip.position_mi = stop.at_mi - ahead;
                d.update_exit(ctx, 0.0, 0.0);
            });
            harness.read_drive(|d| d.trip.effective_time_scale())
        };
        let near = pacing_at(&mut harness, 0.45, 65.0);
        assert!(approx(near, 1.0), "pacing {scale}: {near} at 0.45 miles");
        let over = pacing_at(&mut harness, 1.49, 80.0);
        assert!(approx(over, 1.0), "pacing {scale}: {over} at 80 mph");
        let far = pacing_at(&mut harness, 1.49, 65.0);
        assert!(
            scale <= 1.0 || far > 1.0,
            "pacing {scale}: {far} at road speed 1.49 miles out"
        );
    }
}
