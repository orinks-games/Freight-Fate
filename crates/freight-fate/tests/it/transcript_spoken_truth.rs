//! A truth audit: at every step of a drive, compare what the info keys SAY
//! against the state that backs the claim.
//!
//! The game is played by ear, so the spoken line is the instrument panel. A
//! late line is noise; a FALSE line makes a blind driver act on something that
//! is not happening. The other transcript suites pin individual sentences by
//! phrase; this one is mechanical, and deliberately dumb: it presses each
//! read-out key, parses the number out of what came back, and holds it against
//! the field the sim already keeps.
//!
//! [`audit`] owns the per-instant checks; the drives below put the truck in
//! front of them. A family that cannot be checked honestly against a number
//! the sim holds is NOT in here -- an assertion that only restates the formula
//! it is auditing proves nothing, which is why the cruise row is also checked
//! against the speed the truck settles at rather than only against the caps.

use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route, SpeedLimitSample};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{RoadStop, Zone};
use ff_core::sim::weather::{WeatherKind, WeatherSystem};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    hos_of, ASSUMED_RANGE_MPG, RANGE_MEASURE_MIN_GAL, RANGE_MEASURE_MIN_MI, RANGE_MPG_CEILING,
    RANGE_MPG_FLOOR,
};
use regex::Regex;

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

// ---------------------------------------------------------------------------
// the bench road
// ---------------------------------------------------------------------------

/// One long straight leg with a baked posted limit and grade, no traffic, no
/// hazards, no bends -- so every number the audit reads back has exactly one
/// source. Same shape as `transcript_cruise_resume_ramp`'s bench.
fn bench_road_with_limits(
    drive: &mut DrivingState,
    limits: &[(f64, f64)],
    grade_pct: f64,
    time_scale: f64,
) {
    let city = drive.trip.route.cities[0].clone();
    let miles = 400.0;
    let detail = CorridorDetail {
        speed_limits: limits
            .iter()
            .map(|(at_mi, mph)| SpeedLimitSample {
                at_mi: *at_mi,
                mph: Some(*mph),
                source: "test bench".to_string(),
                hgv: false,
            })
            .collect(),
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
    trip.curves.clear();
    trip.set_patrols(Vec::new());
    drive.trip = trip;
    drive.reset_turn_state_for_trip();
    drive.destination_exit_taken = true;
}

fn a_drive(limit_mph: f64, grade_pct: f64, time_scale: f64) -> PlaytestHarness {
    a_drive_with_limits(&[(0.0, limit_mph)], grade_pct, time_scale)
}

fn a_drive_with_limits(limits: &[(f64, f64)], grade_pct: f64, time_scale: f64) -> PlaytestHarness {
    let limits = limits.to_vec();
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Truth Audit"));
    harness.app.ctx.settings.time_scale = time_scale;
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    harness.with_drive(move |d, _| {
        d.tutorial = None;
        d.departure_checked = true;
        bench_road_with_limits(d, &limits, grade_pct, time_scale);
        d.truck_mut().set_air_ready(false);
    });
    harness
}

fn frame(harness: &mut PlaytestHarness, dt: f64) {
    harness.advance_clock(dt);
    harness.with_drive(move |d, ctx| d.update_frame(ctx, dt));
}

// ---------------------------------------------------------------------------
// the audit
// ---------------------------------------------------------------------------

/// Press one key with the transcript cleared and hand back everything spoken.
fn ask(harness: &mut PlaytestHarness, key: Key) -> String {
    harness.clear_speech();
    harness.press_key(key, None);
    harness.transcript().join(" | ")
}

/// The same, for an Alt chord (the hours keys).
fn ask_alt(harness: &mut PlaytestHarness, key: Key) -> String {
    harness.clear_speech();
    harness.with_drive(move |d, ctx| {
        d.handle_key_event(
            ctx,
            &InputEvent::KeyDown {
                key,
                mods: Mods::ALT,
                text: None,
                repeat: false,
            },
        )
    });
    harness.transcript().join(" | ")
}

fn number_after(text: &str, pattern: &str) -> Option<f64> {
    let re = Regex::new(pattern).expect("a valid audit pattern");
    re.captures(text)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f64>().ok())
}

/// A snapshot of everything the audit compares against, read in the same
/// instant the keys were pressed (no frame runs in between).
#[derive(Debug, Clone)]
struct Truth {
    speed_mph: f64,
    rpm: f64,
    gear: i32,
    neutral: bool,
    reverse: bool,
    fuel_gal: f64,
    fuel_pct: f64,
    air_psi: f64,
    grade_pct: f64,
    position_mi: f64,
    limit_mph: f64,
    cruise_set_mph: Option<f64>,
    cruise_ramp_cap_mph: Option<f64>,
    cruise_curve_cap_mph: Option<f64>,
    cruise_descent_cap_mph: Option<f64>,
    keeper_mph: Option<f64>,
    keeper_ease: Option<(f64, f64, String)>,
    remaining_mi: f64,
    progress_pct: f64,
    next_zone: Option<(f64, f64, String)>,
}

fn truth(harness: &mut PlaytestHarness) -> Truth {
    harness.with_drive(|d, _| {
        let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
        let ramp_cap = d.ramp_approach_cap_mph();
        let remaining = d.trip.remaining_miles();
        let progress = d.journey_progress_percent() as f64;
        let zone = d
            .trip
            .next_zone_within(15.0)
            .map(|z| (z.start_mi, z.limit_mph, z.reason.clone()));
        let t = d.truck();
        Truth {
            speed_mph: t.speed_mph(),
            rpm: t.rpm,
            gear: t.transmission.gear,
            neutral: t.transmission.in_neutral(),
            reverse: t.transmission.in_reverse(),
            fuel_gal: t.fuel_gal,
            fuel_pct: t.fuel_fraction() * 100.0,
            air_psi: t.air_pressure_psi(),
            grade_pct: t.grade * 100.0,
            position_mi: d.trip.position_mi,
            limit_mph: limit,
            cruise_set_mph: d.cruise_mph,
            cruise_ramp_cap_mph: ramp_cap,
            cruise_curve_cap_mph: d.cruise_curve_mph,
            cruise_descent_cap_mph: d.cruise_descent_mph,
            keeper_mph: d.keeper_mph,
            keeper_ease: d.keeper_ease_target.clone(),
            remaining_mi: remaining,
            progress_pct: progress,
            next_zone: zone,
        }
    })
}

/// The lowest ceiling this audit can name from the sim's own stored fields,
/// derived independently of what the cruise loop publishes. It does not know
/// about the posted-limit cap, the weather or a lead vehicle -- those move the
/// held speed too, so a number BELOW everything here is not a complaint, and a
/// number above a ceiling that binds always is.
fn binding_cruise_cap(t: &Truth) -> Option<(f64, &'static str)> {
    let set = t.cruise_set_mph?;
    let mut binding: Option<(f64, &'static str)> = None;
    for (cap, why) in [
        (t.cruise_ramp_cap_mph, "the ramp"),
        (t.cruise_curve_cap_mph, "the bend"),
        (t.cruise_descent_cap_mph, "the grade"),
    ] {
        if let Some(cap) = cap {
            if cap < set - 0.5 && binding.is_none_or(|(low, _)| cap < low) {
                binding = Some((cap, why));
            }
        }
    }
    binding
}

/// Every complaint the audit has about this instant. Empty is the pass.
fn audit(harness: &mut PlaytestHarness) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let space = ask(harness, Key::Space);
    let limit_line = ask(harness, Key::S);
    let fuel_line = ask(harness, Key::F);
    let grade_line = ask(harness, Key::G);
    let route_line = ask(harness, Key::R);
    let upcoming_line = ask(harness, Key::U);
    let t = truth(harness);

    // --- speed / gear / RPM, from the Space readout -----------------------
    if let Some(said) = number_after(&space, r"^(\d+(?:\.\d+)?) miles per hour") {
        if (said - t.speed_mph).abs() > 1.0 {
            out.push(format!(
                "Space said {said} mph; the truck is doing {:.2} -- {space}",
                t.speed_mph
            ));
        }
    } else {
        out.push(format!("Space read out no speed at all -- {space}"));
    }
    if let Some(said) = number_after(&space, r"gear (\d+)") {
        if t.neutral || t.reverse || said as i32 != t.gear {
            out.push(format!(
                "Space said gear {said}; the box is in {} -- {space}",
                if t.neutral {
                    "neutral".to_string()
                } else if t.reverse {
                    "reverse".to_string()
                } else {
                    format!("gear {}", t.gear)
                }
            ));
        }
    }
    if let Some(said) = number_after(&space, r"(\d+) RPM") {
        if (said - t.rpm).abs() > 1.0 {
            out.push(format!(
                "Space said {said} RPM; the engine is at {:.1} -- {space}",
                t.rpm
            ));
        }
    }
    if let Some(said) = number_after(&space, r"air (\d+) psi") {
        if (said - t.air_psi).abs() > 1.5 {
            out.push(format!(
                "Space said air {said} psi; the tanks are at {:.1} -- {space}",
                t.air_psi
            ));
        }
    }

    // --- what the cruise is actually holding ------------------------------
    //
    // The set speed and the speed cruise is holding are two numbers whenever a
    // cap binds -- a ramp, a bend, a descent. The speed keeper's readout
    // already says both ("holding 15 for the corner, set 25"); this is the
    // same question asked of adaptive cruise.
    {
        let binding = binding_cruise_cap(&t);
        if let Some(said) = number_after(&space, r"(?i)adaptive cruise set at (\d+)") {
            if let Some((cap, why)) = binding {
                if cap < said - 1.0 {
                    out.push(format!(
                        "Space said adaptive cruise set at {said}; cruise is capped at \
                         {cap:.0} for {why} and the truck is doing {:.0} -- {space}",
                        t.speed_mph
                    ));
                }
            }
        }
        if let Some(said) = number_after(&space, r"(?i)adaptive cruise holding (\d+)") {
            // The published number may be lower than any ceiling this audit
            // knows about -- the posted-limit cap, the weather and the lead
            // vehicle all move it too. What it must never be is HIGHER than a
            // ceiling that is binding right now.
            if let Some((cap, why)) = binding {
                if said > cap + 1.0 {
                    out.push(format!(
                        "Space said cruise is holding {said}; the {why} caps it at \
                         {cap:.0} -- {space}"
                    ));
                }
            }
            if let Some(set) = number_after(&space, r"set (\d+) miles per hour") {
                let real = t.cruise_set_mph.unwrap_or(0.0);
                if (set - real).abs() > 0.6 {
                    out.push(format!(
                        "Space said the set speed is {set}; cruise is set at {real:.1} -- {space}"
                    ));
                }
            }
        }
    }

    // --- posted limit, from S ---------------------------------------------
    if let Some(said) = number_after(&limit_line, r"(?:Speed|Truck) limit (\d+)") {
        if (said - t.limit_mph).abs() > 0.6 {
            out.push(format!(
                "S said limit {said}; speed_limit_at({:.2}) is {:.1} -- {limit_line}",
                t.position_mi, t.limit_mph
            ));
        }
        if let Some(over) = number_after(&limit_line, r"(?i)about (\d+) miles per hour over") {
            let real = t.speed_mph - t.limit_mph;
            if (over - real).abs() > 1.0 {
                out.push(format!(
                    "S said {over} over; the truck is {real:.1} over -- {limit_line}"
                ));
            }
        } else if t.speed_mph - t.limit_mph >= 1.5 {
            out.push(format!(
                "S named no overspeed; the truck is {:.1} over -- {limit_line}",
                t.speed_mph - t.limit_mph
            ));
        }
    }

    // --- the tank, from F --------------------------------------------------
    if let Some(said) = number_after(&fuel_line, r"Fuel (\d+) percent") {
        if (said - t.fuel_pct).abs() > 1.0 {
            out.push(format!(
                "F said {said} percent; the tank is {:.1} -- {fuel_line}",
                t.fuel_pct
            ));
        }
    }
    if let Some(said) = number_after(&fuel_line, r"(\d+) gallons") {
        if (said - t.fuel_gal).abs() > 1.0 {
            out.push(format!(
                "F said {said} gallons; the tank holds {:.1} -- {fuel_line}",
                t.fuel_gal
            ));
        }
    }

    // --- the grade, from G -------------------------------------------------
    if let Some(said) = number_after(&grade_line, r"Grade (\d+\.\d) percent") {
        if (said - t.grade_pct.abs()).abs() > 0.06 {
            out.push(format!(
                "G said {said} percent; the truck is on {:.2} -- {grade_line}",
                t.grade_pct
            ));
        }
        let downhill = grade_line.contains("downhill");
        if downhill != (t.grade_pct < 0.0) {
            out.push(format!(
                "G called it {}; the grade is {:.2} -- {grade_line}",
                if downhill { "downhill" } else { "uphill" },
                t.grade_pct
            ));
        }
    } else if grade_line.contains("Level road") && t.grade_pct.abs() >= 0.5 {
        out.push(format!(
            "G called it level; the truck is on {:.2} percent -- {grade_line}",
            t.grade_pct
        ));
    }

    // --- what the speed keeper is holding ---------------------------------
    //
    // The keeper's readout already answers with two numbers when it is easing
    // ("holding 15 for the corner, set 25"). Audited here so the pattern the
    // cruise check above measures against is itself pinned.
    if let Some(said) = number_after(&space, r"(?i)speed keeper holding (\d+)") {
        let held = t.keeper_mph.unwrap_or(0.0);
        let eased = t
            .keeper_ease
            .as_ref()
            .filter(|(at_mi, eased, _)| t.position_mi < *at_mi && *eased < held)
            .map(|(_, eased, _)| *eased);
        let real = eased.unwrap_or(held);
        if (said - real).abs() > 0.6 {
            out.push(format!(
                "Space said the keeper is holding {said}; it is holding {real:.1} -- {space}"
            ));
        }
    }

    // --- how far is left, from R -------------------------------------------
    if let Some(said) = number_after(&route_line, r"(\d+) percent there") {
        if (said - t.progress_pct).abs() > 1.0 {
            out.push(format!(
                "R said {said} percent there; the trip is {:.1} percent in -- {route_line}",
                t.progress_pct
            ));
        }
    }
    if let Some(said) = number_after(&route_line, r"(\d+(?:\.\d+)?) miles? left") {
        if (said - t.remaining_mi).abs() > 1.0 {
            out.push(format!(
                "R said {said} miles left; {:.2} miles of route remain -- {route_line}",
                t.remaining_mi
            ));
        }
    }

    // --- what is coming, from U --------------------------------------------
    //
    // The zone clause carries two numbers a driver plans a brake with: how far
    // and how slow. Both are the zone's own fields.
    if let Some((start_mi, limit_mph, reason)) = t.next_zone.as_ref() {
        let ahead = start_mi - t.position_mi;
        if !upcoming_line.contains(reason.as_str()) {
            out.push(format!(
                "U never named the {reason} zone {ahead:.1} miles ahead -- {upcoming_line}"
            ));
        }
        let pattern = format!(r"{reason} in (\d+(?:\.\d+)?) miles?");
        if let Some(said) = number_after(&upcoming_line, &pattern) {
            if (said - ahead).abs() > 1.0 {
                out.push(format!(
                    "U put the {reason} zone {said} miles off; it starts {ahead:.2} miles                      ahead -- {upcoming_line}"
                ));
            }
        }
        let limit_pattern = format!(r"{reason} in [^,]+, speed limit (\d+)");
        if let Some(said) = number_after(&upcoming_line, &limit_pattern) {
            if (said - limit_mph).abs() > 0.6 {
                out.push(format!(
                    "U said the {reason} zone posts {said}; it posts {limit_mph:.0} --                      {upcoming_line}"
                ));
            }
        }
    }

    out
}

fn report(violations: &[String]) -> String {
    violations.join("\n  ")
}

// ---------------------------------------------------------------------------
// the drives
// ---------------------------------------------------------------------------

#[test]
fn test_the_readouts_stay_true_down_a_plain_highway_mile() {
    // Open road, cruise on, nothing capping it: every readout key must answer
    // with the number the sim holds, all the way down the mile.
    let mut harness = a_drive(65.0, 0.0, 1.0);
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
    });
    harness.press_key(Key::K, None);
    let mut violations: Vec<String> = Vec::new();
    for step in 0..40 {
        for _ in 0..15 {
            frame(&mut harness, 1.0 / 30.0);
        }
        let mut found = audit(&mut harness);
        if !found.is_empty() {
            violations.push(format!("step {step}:"));
            violations.append(&mut found);
        }
    }
    assert!(violations.is_empty(), "{}", report(&violations));
}

#[test]
fn test_the_readouts_stay_true_on_a_grade_and_over_the_limit() {
    // A downgrade with the truck running over the posted number: the grade
    // call, the overspeed clause and the drivetrain rows all have to hold.
    let mut harness = a_drive(55.0, -4.0, 1.0);
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 68.0 * MPS_PER_MPH;
    });
    let mut violations: Vec<String> = Vec::new();
    for step in 0..30 {
        for _ in 0..15 {
            frame(&mut harness, 1.0 / 30.0);
        }
        let mut found = audit(&mut harness);
        if !found.is_empty() {
            violations.push(format!("step {step}:"));
            violations.append(&mut found);
        }
    }
    assert!(violations.is_empty(), "{}", report(&violations));
}

/// Cruise set well above the road, an exit signalled ahead: the ramp cap is
/// what the truck is actually held to from here to the gore.
fn armed_exit_drive(ahead_mi: f64, set_mph: f64) -> (PlaytestHarness, RoadStop) {
    let mut harness = a_drive(65.0, 0.0, 1.0);
    harness.press_key(Key::E, None);
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
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![staged.clone()];
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.trip.position_mi = staged.at_mi - ahead_mi;
    });
    harness.press_key(Key::K, None);
    harness.with_drive(move |d, _| {
        // The remembered open-road target a real session carries into the
        // ramp: cruise keeps the high number while the ramp cap holds it down.
        d.cruise_mph = Some(set_mph);
        d.speed_control_target_mph = Some(set_mph);
    });
    harness.press_key(Key::X, None);
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.key())),
        Some(stop.key()),
        "the exit never armed -- {}",
        harness.transcript_text()
    );
    (harness, stop)
}

#[test]
fn test_the_status_readout_names_the_speed_cruise_is_actually_holding_on_a_ramp() {
    // The owner's own session log, New York, 2026-08-23:
    //
    //   Open road. Adaptive cruise resuming at 33 miles per hour for the ramp.
    //   44 miles per hour, gear 9, 1324 RPM, automatic speed control,
    //   adaptive cruise set at 80 miles per hour, ...
    //
    // The resume line already names the number the truck will hold (fixed for
    // Brandon on 2026-08-20). Space did not: it read the SET speed while a
    // ramp cap pinned cruise far below it, so a driver checking mid-ramp was
    // told 80 by the one key whose whole job is answering "what is happening
    // right now".
    let (mut harness, stop) = armed_exit_drive(1.0, 80.0);
    // Roll toward the gore until the ramp cap really is what holds the truck.
    let mut cap = f64::INFINITY;
    for step in 0..40 {
        let at = stop.at_mi - 1.0 + 0.025 * step as f64;
        harness.with_drive(move |d, ctx| {
            d.trip.position_mi = at;
            d.update_exit(ctx, 0.0, 0.0);
        });
        frame(&mut harness, 1.0 / 30.0);
        cap = harness
            .read_drive(|d| d.ramp_approach_cap_mph())
            .expect("an armed exit caps cruise");
        // The exit floor, ten under road speed at most, well under the 80
        // the driver set (realistic exit, 2026-09-24).
        if cap < 75.0 {
            break;
        }
    }
    assert!(cap < 75.0, "the exit cap never bound: {cap}");
    let violations = audit(&mut harness);
    assert!(violations.is_empty(), "{}", report(&violations));

    // And the two lines the log put a second apart now agree. Braking away
    // and resuming is how the owner's session reached this: the resume names
    // the ramp number, and Space has to name the same one.
    harness.press_key(Key::K, None); // cancel, remembering the 80
    let resumed = {
        harness.clear_speech();
        harness.with_drive(|d, ctx| {
            d.handle_key_event(
                ctx,
                &InputEvent::KeyDown {
                    key: Key::K,
                    mods: Mods::SHIFT,
                    text: None,
                    repeat: false,
                },
            )
        });
        for _ in 0..3 {
            frame(&mut harness, 1.0 / 30.0);
        }
        harness.transcript().join(" | ")
    };
    assert!(
        resumed.contains("for the exit"),
        "the resume never named the exit -- {resumed}"
    );
    let ramp_number =
        number_after(&resumed, r"Cruise resuming, (\d+)").expect("the resume names a number");
    let space = ask(&mut harness, Key::Space);
    let holding = number_after(&space, r"(?i)adaptive cruise holding (\d+)")
        .expect("the status readout names what cruise is holding");
    // The resume names the ramp's own number -- where the glide is going --
    // and Space names where the glide is now. Both are about the ramp, and
    // neither is the 80 the driver used to be told.
    assert!(
        holding >= ramp_number - 1.0 && holding < 79.0,
        "the resume said {ramp_number} for the exit and Space said {holding} -- {space}"
    );
    assert!(
        space.contains("set 80 miles per hour"),
        "the set speed went missing from the readout -- {space}"
    );
}

#[test]
fn test_the_readouts_stay_true_through_a_restricted_zone() {
    // A heavy-traffic zone: U calls it from a mile back, adaptive cruise eases
    // for it, and the posted figure S answers with changes under the wheels.
    // Every row still has to match the sim.
    let mut harness = a_drive(65.0, 0.0, 1.0);
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.trip.zones = vec![Zone::new(2.0, 6.0, 35.0, "heavy traffic")];
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.trip.position_mi = 0.5;
    });
    harness.press_key(Key::K, None);
    let mut violations: Vec<String> = Vec::new();
    let mut saw_zone_clause = 0;
    let mut inside_the_zone = 0;
    for step in 0..420 {
        for _ in 0..15 {
            frame(&mut harness, 1.0 / 30.0);
        }
        if ask(&mut harness, Key::U).contains("heavy traffic in") {
            saw_zone_clause += 1;
        }
        if harness.read_drive(|d| d.trip.position_mi) >= 2.0 {
            inside_the_zone += 1;
        }
        let mut found = audit(&mut harness);
        if !found.is_empty() {
            violations.push(format!(
                "step {step} at {:.2} miles:",
                harness.read_drive(|d| d.trip.position_mi)
            ));
            violations.append(&mut found);
        }
    }
    // The audit must actually have had the zone in front of it, from both
    // sides of the line: a check that never fires is not coverage.
    assert!(
        saw_zone_clause > 0 && inside_the_zone > 0,
        "the drive never met the zone: {saw_zone_clause} calls, {inside_the_zone} inside"
    );
    assert!(violations.is_empty(), "{}", report(&violations));
}

#[test]
fn test_the_readouts_stay_true_while_the_speed_keeper_has_the_truck() {
    // Inside a restricted zone the keeper, not cruise, owns the pedal, and its
    // readout carries two numbers whenever it is easing for something ahead.
    // The audit holds both against the keeper's own fields.
    let mut harness = a_drive(65.0, 0.0, 1.0);
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.trip.zones = vec![Zone::new(0.0, 20.0, 35.0, "heavy traffic")];
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 8;
        d.truck_mut().velocity_mps = 34.0 * MPS_PER_MPH;
        d.trip.position_mi = 1.0;
        d.resume_target_mph = Some(60.0);
    });
    // Shift+K inside a zone hands the truck to the keeper, not to cruise.
    harness.with_drive(|d, ctx| {
        d.handle_key_event(
            ctx,
            &InputEvent::KeyDown {
                key: Key::K,
                mods: Mods::SHIFT,
                text: None,
                repeat: false,
            },
        )
    });
    let mut violations: Vec<String> = Vec::new();
    let mut saw_keeper_row = 0;
    for step in 0..120 {
        for _ in 0..15 {
            frame(&mut harness, 1.0 / 30.0);
        }
        if ask(&mut harness, Key::Space).contains("Speed keeper holding") {
            saw_keeper_row += 1;
        }
        let mut found = audit(&mut harness);
        if !found.is_empty() {
            violations.push(format!(
                "step {step} at {:.2} miles:",
                harness.read_drive(|d| d.trip.position_mi)
            ));
            violations.append(&mut found);
        }
    }
    assert!(saw_keeper_row > 0, "the keeper never took the truck");
    assert!(violations.is_empty(), "{}", report(&violations));
}

#[test]
fn test_the_fuel_key_promises_a_range_the_truck_can_actually_drive() {
    // F answers "Range about N miles" from 6 miles per gallon until the run
    // has burned enough to measure its own. That is a claim about THIS truck
    // under THIS load, so it is worth measuring: a range a loaded rig cannot
    // reach is a fuel stop a driver skips.
    let mut harness = a_drive(65.0, 0.0, 1.0);
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
    });
    harness.press_key(Key::K, None);
    let (fuel0, at0) = harness.read_drive(|d| (d.truck().fuel_gal, d.trip.position_mi));
    for _ in 0..30_000 {
        frame(&mut harness, 1.0 / 30.0);
    }
    let (fuel1, at1) = harness.read_drive(|d| (d.truck().fuel_gal, d.trip.position_mi));
    let burned = fuel0 - fuel1;
    let miles = at1 - at0;
    assert!(
        burned > 0.5 && miles > 2.0,
        "{burned} gallons, {miles} miles"
    );
    let measured_mpg = miles / burned;
    // The spoken promise, and what the truck actually returns on level ground
    // at the posted number -- the kindest case there is.
    assert!(
        measured_mpg >= 6.0 * 0.85,
        "F promises 6.0 miles per gallon; this truck returned {measured_mpg:.2} \
         over {miles:.1} level miles at cruise ({burned:.2} gallons)"
    );
    // Once measured, the promise is the run's own average; before that, the
    // assumed figure. How far this run gets depends on the frame pacing, so
    // check whichever rule applies to the distance it actually covered.
    let (promised, driven, used) =
        harness.read_drive(|d| (d.range_mpg(), d.trip.position_mi, d.trip.fuel_used_gal));
    let expected = if used >= RANGE_MEASURE_MIN_GAL && driven >= RANGE_MEASURE_MIN_MI {
        (driven / used).clamp(RANGE_MPG_FLOOR, RANGE_MPG_CEILING)
    } else {
        ASSUMED_RANGE_MPG
    };
    assert!(
        (promised - expected).abs() < 1e-9,
        "F promises {promised:.2}, expected {expected:.2} after {driven:.1} miles on {used:.2} gallons"
    );
}

#[test]
fn test_the_hours_key_counts_down_the_same_wheel_time_the_duty_log_recorded() {
    // Alt+D answers "how much driving time is left". The honest check is not
    // the arithmetic inside the summary -- that would only restate the formula
    // -- but whether the number it counts down matches the wheel time the duty
    // log actually recorded over the same stretch of road.
    let mut harness = a_drive(65.0, 0.0, 8.0);
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
    });
    harness.press_key(Key::K, None);

    let said_before = ask_alt(&mut harness, Key::D);
    let left_before = number_after(&said_before, r"Driving time left: (\d+(?:\.\d+)?) hours")
        .expect("the hours key names the driving time left");
    let logged_before = hos_of(&harness.app.ctx).driving_min;

    for _ in 0..12_000 {
        frame(&mut harness, 1.0 / 30.0);
    }

    let said_after = ask_alt(&mut harness, Key::D);
    let left_after = number_after(&said_after, r"Driving time left: (\d+(?:\.\d+)?) hours")
        .expect("the hours key names the driving time left");
    let logged_after = hos_of(&harness.app.ctx).driving_min;

    let counted_down_h = left_before - left_after;
    let logged_h = (logged_after - logged_before) / 60.0;
    assert!(
        logged_h > 0.2,
        "the drive never put meaningful time on the wheel: {logged_h:.3} hours"
    );
    // The spoken figure is rounded to a tenth of an hour, so a tenth of slack
    // either way is the readout's own resolution, not a disagreement.
    assert!(
        (counted_down_h - logged_h).abs() <= 0.1,
        "Alt+D counted down {counted_down_h:.2} hours; the duty log recorded \
         {logged_h:.2} hours at the wheel ({said_before} / {said_after})"
    );
}

/// A spoken distance back into miles. Only the forms the exit calls actually
/// use; anything else is not a claim this check knows how to judge.
fn spoken_miles(said: &str) -> Option<(f64, f64)> {
    // (miles, the tolerance that phrasing's own rounding allows)
    let said = said.trim();
    match said {
        "a quarter mile" => return Some((0.25, 0.15)),
        "half a mile" => return Some((0.5, 0.15)),
        "three quarters of a mile" => return Some((0.75, 0.15)),
        "one mile" => return Some((1.0, 0.15)),
        _ => {}
    }
    let re = Regex::new(r"^(\d+(?:\.\d+)?) miles?$").expect("a valid distance pattern");
    let value = re.captures(said)?.get(1)?.as_str().parse::<f64>().ok()?;
    let tolerance = if said.contains('.') { 0.15 } else { 0.6 };
    Some((value, tolerance))
}

#[test]
fn test_the_exit_calls_name_the_road_that_is_really_left() {
    // "In a quarter mile, the exit for..." is a braking instruction. The
    // distance in it has to be the distance to the gore at the moment the call
    // is made, or a driver brakes for road that is not there.
    let mut harness = a_drive(65.0, 0.0, 1.0);
    harness.press_key(Key::E, None);
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
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![staged.clone()];
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.trip.position_mi = staged.at_mi - 6.0;
    });
    harness.press_key(Key::K, None);
    // Every shape the road calls an exit in. The number in each one is a
    // braking instruction, so each is held against the road really left.
    const DIST: &str =
        r"(\d+(?:\.\d+)? miles?|a quarter mile|half a mile|three quarters of a mile|one mile)";
    let calls: Vec<Regex> = [
        format!(r"at exit \S+ in {DIST}"),
        format!(r"In {DIST}, [^.]*exit"),
        // The countdown anchors: "Exit for truck stop: ... in 2 miles."
        format!(r"[Ee]xit for [^.]* in {DIST}"),
    ]
    .iter()
    .map(|p| Regex::new(p).expect("a valid call pattern"))
    .collect();
    let gore = stop.at_mi;
    let mut armed = false;
    let mut checked = 0;
    let mut heard: Vec<String> = Vec::new();
    let mut violations: Vec<String> = Vec::new();
    for _ in 0..9_000 {
        harness.clear_speech();
        frame(&mut harness, 1.0 / 30.0);
        let ahead = stop.at_mi - harness.read_drive(|d| d.trip.position_mi);
        for line in harness.transcript() {
            if line.contains("Prairie") || line.contains("exit") {
                heard.push(format!("{ahead:.2}: {line}"));
            }
            for call in &calls {
                let Some(caught) = call.captures(&line) else {
                    continue;
                };
                let Some((said_mi, tolerance)) = spoken_miles(&caught[1]) else {
                    continue;
                };
                checked += 1;
                if (said_mi - ahead).abs() > tolerance {
                    violations.push(format!(
                        "an exit call said {said_mi} miles with {ahead:.2} really left -- {line}"
                    ));
                }
            }
        }
        // Signal as soon as the road offers the exit, so the countdown calls
        // and the gore call are on the transcript too.
        if !armed
            && harness.read_drive(|d| d.exit_stop.is_some() || d.trip.position_mi > gore - 4.0)
        {
            harness.press_key(Key::X, None);
            armed = harness.read_drive(|d| d.exit_stop.is_some());
        }
        if harness.read_drive(|d| d.trip.position_mi) > stop.at_mi {
            break;
        }
    }
    assert!(
        checked >= 2,
        "only {checked} exit calls were judged -- {heard:?}"
    );
    assert!(violations.is_empty(), "{}", report(&violations));
}

#[test]
fn test_the_cruise_row_names_the_speed_a_lower_posted_limit_left_it() {
    // The commonest shape of the same defect, straight out of the owner's
    // session log (New York, 2026-08-23, 22:07):
    //
    //   Posted limit lower; adaptive cruise easing to 50 miles per hour.
    //   45 miles per hour, gear 9, 1358 RPM, automatic speed control,
    //   adaptive cruise set at 85 miles per hour, ...
    //
    // Cruise had eased itself to 50 for a posted 45 and the status key still
    // read out 85. This check needs no knowledge of which cap won: once cruise
    // has settled, whatever number the row gives has to be the speed the truck
    // is actually doing.
    // A corridor drop, not a zone: a zone hands the truck to the keeper, and
    // this case is about the controller that stays on.
    let mut harness = a_drive_with_limits(&[(0.0, 65.0), (3.0, 45.0)], 0.0, 1.0);
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 62.0 * MPS_PER_MPH;
        d.trip.position_mi = 1.0;
    });
    harness.press_key(Key::K, None);
    harness.with_drive(|d, _| {
        d.cruise_mph = Some(85.0);
        d.speed_control_target_mph = Some(85.0);
    });
    // Down the road, past the drop, until the truck settles.
    let mut still = 0.0;
    for _ in 0..80 {
        for _ in 0..60 {
            frame(&mut harness, 1.0 / 30.0);
        }
        let settled = still;
        still = harness.read_drive(|d| d.truck().speed_mph());
        if harness.read_drive(|d| d.trip.position_mi) > 4.0 && (still - settled).abs() < 0.3 {
            break;
        }
    }
    assert!(
        harness.read_drive(|d| d.trip.position_mi) > 3.0 && still < 60.0,
        "the drive never reached the lower limit: at {:.2} doing {still:.1}",
        harness.read_drive(|d| d.trip.position_mi)
    );
    assert!(
        harness.read_drive(|d| d.cruise_mph.is_some()),
        "cruise handed the truck off; this case is about the controller that stays"
    );
    let space = ask(&mut harness, Key::Space);
    let claimed = number_after(&space, r"(?i)adaptive cruise (?:holding|set at) (\d+)")
        .expect("the cruise row names a number");
    assert!(
        (claimed - still).abs() <= 6.0,
        "the cruise row said {claimed} with the truck settled at {still:.1} -- {space}"
    );
}
