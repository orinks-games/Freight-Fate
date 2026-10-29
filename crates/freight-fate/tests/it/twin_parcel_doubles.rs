//! Sets of doubles at the hook, at the wheel and on the scale: the second
//! hook-up and longer walk-around, the refused reverse, and the gross cap
//! with both trailers and the converter dolly.

use crate::states_city_support::*;
use ff_core::data::world::get_world;
use ff_core::models::doubles::{
    doubles_break_text, doubles_hook_text, DOUBLES_BREAK_SET_MIN, DOUBLES_NO_REVERSE_TEXT,
    DOUBLES_SECOND_HOOK_MIN, DOUBLES_WALK_AROUND_EXTRA_MIN, REAR_TRAILER_WHIP_EMPTY_TEXT,
    REAR_TRAILER_WHIP_TEXT,
};
use ff_core::models::jobs::{cargo_type, Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::models::trailer_yard::{delivery_plan, LIVE_LOAD_MIN};
use ff_core::sim::transmission::REVERSE;
use ff_core::sim::vehicle::KG_PER_TON;
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::share;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{Key, Mods};
use freight_fate::states::city_pickup::{PickupFacilityState, PickupOptions};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DRIVE_PHASE_DELIVERY, WALK_AROUND_MIN};
use freight_fate::states::driving_menu_states::{DriveRef, FacilityArrivalState};
use freight_fate::states::driving_rest_states::walk_around_minutes;

fn drive(
    app: &mut TestApp,
    cargo: &str,
    origin: &str,
    destination: &str,
    tons: f64,
) -> DrivingState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Doubles", origin));
    let route = world
        .supported_route(origin, destination, None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let job = Job::new(
        &CARGO_CATALOG[cargo],
        tons,
        origin,
        "company yard",
        destination,
        route.miles(),
        1000.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(7),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

#[test]
fn a_set_of_doubles_takes_two_hook_ups_and_a_longer_walk_around_at_the_shipper() {
    let mut app = TestApp::new();
    career(&mut app, "Pup Hook", "Chicago");
    let mut job = Job::new(
        cargo_type("parcel_doubles").unwrap(),
        10.0,
        "Chicago",
        "Chicago Cross-Dock",
        "Milwaukee",
        92.0,
        1800.0,
        9.0,
    );
    job.origin_type = "mine_quarry".to_string(); // loads at a dock
    let pickup = PickupFacilityState::new(&app.ctx, job, PickupOptions::default());
    app.push_state(pickup);

    key(&mut app, Key::Return); // check in
    let said = app.main_lines().last().cloned().unwrap();
    let hook = doubles_hook_text("parcel_doubles");
    assert!(!hook.is_empty());
    assert!(
        said.contains(&hook),
        "check-in must speak the doubles time: {said}"
    );
    assert!(
        app.visible_lines()
            .iter()
            .any(|l| l.starts_with("Doubles: two hook-ups")),
        "the doubles time must be on screen too"
    );

    let plan = with_state::<PickupFacilityState, _>(&app, |p, ctx| p.pickup_plan(ctx));
    let extra = DOUBLES_SECOND_HOOK_MIN + DOUBLES_WALK_AROUND_EXTRA_MIN;
    assert_eq!(plan.minutes, LIVE_LOAD_MIN + extra);
    let hours_before = profile(&app).game_hours;
    let duty_before = profile(&app).hos.duty_min;
    key(&mut app, Key::Return); // load
    finish_timed_state(&mut app);
    assert_eq!(profile(&app).game_hours, hours_before + plan.minutes / 60.0);
    assert_eq!(profile(&app).hos.duty_min, duty_before + plan.minutes);
}

#[test]
fn a_single_trailer_pickup_says_nothing_about_doubles() {
    assert!(doubles_hook_text("general").is_empty());
}

#[test]
fn reverse_is_refused_and_spoken_with_doubles_hooked() {
    let mut app = TestApp::new();
    let mut d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    assert!(d.trip.truck.doubles_hooked());
    let damage = d.trip.truck.damage_pct;
    let gear = d.trip.truck.transmission.gear;
    d.manual_shift(&mut app.ctx, REVERSE);
    assert_eq!(d.trip.truck.transmission.gear, gear);
    assert_eq!(d.trip.truck.damage_pct, damage, "a refusal breaks nothing");
    assert_eq!(app.main_lines().last().unwrap(), DOUBLES_NO_REVERSE_TEXT);

    // A single trailer is not refused.
    let mut single = drive(&mut app, "general", "Buffalo", "Rochester", 10.0);
    assert!(!single.trip.truck.doubles_hooked());
    single.manual_shift(&mut app.ctx, REVERSE);
    assert_ne!(app.main_lines().last().unwrap(), DOUBLES_NO_REVERSE_TEXT);
}

#[test]
fn the_walk_around_on_the_road_covers_both_trailers_and_the_dolly() {
    let mut app = TestApp::new();
    let d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    assert_eq!(
        walk_around_minutes(&d),
        WALK_AROUND_MIN + DOUBLES_WALK_AROUND_EXTRA_MIN
    );
    let single = drive(&mut app, "general", "Buffalo", "Rochester", 10.0);
    assert_eq!(walk_around_minutes(&single), WALK_AROUND_MIN);
}

#[test]
fn a_turnpike_double_runs_under_its_turnpike_cap_and_pups_under_eighty_thousand() {
    let mut app = TestApp::new();
    let turnpike = drive(&mut app, "turnpike_doubles", "Toledo", "Elkhart", 25.0);
    let set = &turnpike.trip.truck.trailer_set;
    assert_eq!(set.units, 2);
    assert_eq!(set.legal_gvw_lb().round(), 127_400.0);
    assert!(!turnpike.trip.truck.is_over_legal_gvw());
    let ticket = turnpike.trip.truck.scale_ticket_text();
    assert!(
        ticket.contains("both trailers and the converter dolly"),
        "{ticket}"
    );
    assert!(ticket.contains("127,400"), "{ticket}");

    let pups = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    assert_eq!(pups.trip.truck.trailer_set.legal_gvw_lb().round(), 80_000.0);
    // The same pups loaded past what 80,000 lb leaves after both trailers
    // and the dolly read overweight.
    let mut heavy = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    heavy.trip.truck.cargo_kg = 30.0 * KG_PER_TON;
    assert!(heavy.trip.truck.is_over_legal_gvw());
}

#[test]
fn breaking_a_set_of_doubles_at_the_receiver_takes_time_and_says_so() {
    let mut app = TestApp::new();
    let mut d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    d.trip.truck.velocity_mps = 0.0;
    let plan = delivery_plan(&d.job, app.ctx.profile.as_ref().unwrap());
    let base = {
        let mut single = d.job.clone();
        single.cargo = &CARGO_CATALOG["general"];
        delivery_plan(&single, app.ctx.profile.as_ref().unwrap()).minutes
    };
    assert_eq!(plan.minutes, base + DOUBLES_BREAK_SET_MIN);
    let minutes_before = d.trip.game_minutes;
    let duty_before = profile(&app).hos.duty_min;

    let shared = share(d);
    app.ctx.push_shared_with(shared.clone(), false);
    app.push_state(FacilityArrivalState::with_drive(DriveRef::of(&shared)));
    let text = doubles_break_text("parcel_doubles");
    let said = app.main_lines().last().cloned().unwrap();
    assert!(
        said.contains(&text),
        "arrival must speak the break time: {said}"
    );
    assert!(
        app.visible_lines()
            .iter()
            .any(|l| l.starts_with("Doubles: drop the rear trailer")),
        "the break time must be on screen too: {:?}",
        app.visible_lines()
    );

    key(&mut app, Key::Return); // drop or dock
    let said = app.main_lines().join(" ");
    assert!(
        said.contains(&text),
        "the work must speak the break time: {said}"
    );
    finish_timed_state(&mut app);
    let minutes_after = shared
        .borrow()
        .as_any()
        .downcast_ref::<DrivingState>()
        .unwrap()
        .trip
        .game_minutes;
    assert!((minutes_after - (minutes_before + plan.minutes)).abs() < 1e-6);
    assert!((profile(&app).hos.duty_min - (duty_before + plan.minutes)).abs() < 1e-6);
}

#[test]
fn a_crosswind_drifts_the_tractor_the_same_with_doubles_as_with_a_single() {
    let mut app = TestApp::new();
    app.ctx.settings.lane_keeping = "off".to_string();
    let mut run = |cargo: &str| {
        let mut d = drive(&mut app, cargo, "Buffalo", "Rochester", 10.0);
        d.trip.weather.current = WeatherKind::Wind;
        d.trip.truck.velocity_mps = 60.0 / 2.23694;
        let mut offsets = Vec::new();
        let mut gusted = false;
        for _ in 0..200 {
            d.update_lane(&mut app.ctx, 0.05);
            offsets.push((d.lane.lane, d.lane.offset));
            gusted |= d.lane.wind_lateral_g > 0.0;
        }
        (offsets, gusted)
    };
    let (single, gusted) = run("general");
    let (pups, _) = run("parcel_doubles");
    assert!(gusted, "the wind must be blowing for this to mean anything");
    assert!(single.iter().any(|(_, offset)| *offset != 0.0));
    assert_eq!(
        single, pups,
        "rear-trailer amplification must not reach the tractor"
    );
}

#[test]
fn full_lane_keeping_changes_lanes_gently_with_no_whip() {
    // Under full lane keeping the steering keys only ask for a timed,
    // signalled lane change; there is no abrupt steer to reach, and the
    // change it makes never whips the rear trailer.
    let mut app = TestApp::new();
    app.ctx.settings.lane_keeping = "full".to_string();
    let mut d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    d.trip.truck.velocity_mps = 60.0 / 2.23694;
    let damage = d.trip.truck.cargo_damage_pct;
    d.tap_lane_change(&mut app.ctx, 1);
    for _ in 0..200 {
        d.update_lane(&mut app.ctx, 0.05);
        assert_eq!(d.lane.transient_lateral_g(), 0.0);
    }
    assert_eq!(d.trip.truck.cargo_damage_pct, damage);
    assert!(!app.event_lines().iter().any(|l| l.contains("whipped")));
}

#[test]
fn a_legacy_turnpike_doubles_trip_on_an_uncapped_lane_finishes_clean() {
    // Dispatched under the old rules, before lanes through uncapped states
    // were refused: Buffalo to Erie crosses into Pennsylvania (no recorded
    // cap), and Chicago to Milwaukee has no capped state at all. The trip
    // already under way is grandfathered, not red-lighted.
    let mut app = TestApp::new();
    for (origin, destination, cap) in [
        ("Buffalo", "Erie", 143_000.0),
        ("Chicago", "Milwaukee", 80_000.0),
    ] {
        let legacy = drive(&mut app, "turnpike_doubles", origin, destination, 14.0);
        let truck = &legacy.trip.truck;
        assert!(truck.doubles_hooked());
        assert_eq!(truck.trailer_set.legal_gvw_lb().round(), cap);
        assert!(!truck.is_over_legal_gvw());
        // The scale verdict that decides a red light and a fine.
        assert!(!legacy.cargo_is_overweight());
        let ticket = truck.scale_ticket_text();
        assert!(!ticket.contains("not legal"), "{ticket}");
        let cap_text = if cap > 100_000.0 { "143,000" } else { "80,000" };
        assert!(ticket.contains(cap_text), "{ticket}");
        assert!(ticket.contains("Legal"), "{ticket}");
    }
}

#[test]
fn the_same_turnpike_set_sways_the_same_on_a_ny_lane_and_a_ks_lane() {
    let mut app = TestApp::new();
    let mut ny = drive(&mut app, "turnpike_doubles", "Buffalo", "Rochester", 20.0);
    let mut ks = drive(&mut app, "turnpike_doubles", "Wichita", "Emporia", 20.0);
    assert_eq!(ny.trip.truck.trailer_set.legal_gvw_lb().round(), 143_000.0);
    assert_eq!(ks.trip.truck.trailer_set.legal_gvw_lb().round(), 120_000.0);
    for d in [&mut ny, &mut ks] {
        d.trip.truck.cargo_kg = 20.0 * KG_PER_TON;
        d.trip.truck.velocity_mps = 60.0 / 2.23694;
    }
    assert_eq!(
        ny.trip.truck.light_set_wind_mult(),
        ks.trip.truck.light_set_wind_mult()
    );
    assert_eq!(
        ny.trip.truck.rear_trailer_lateral_g(0.1, 0.04),
        ks.trip.truck.rear_trailer_lateral_g(0.1, 0.04)
    );
}

/// Drive `d` at 60 mph through `steps` of (key held, seconds) on the real
/// input path, then 4 s hands off, at 20 frames a second. Returns the cargo
/// damage it cost and the whip lines spoken.
fn steer_through(
    app: &mut TestApp,
    d: &mut DrivingState,
    steps: &[(Option<Key>, f64)],
) -> (f64, Vec<String>) {
    let clock = app.fake_pacer_clock();
    let dt = 0.05;
    let before = d.trip.truck.cargo_damage_pct;
    let mut all: Vec<(Option<Key>, f64)> = steps.to_vec();
    all.push((None, 4.0));
    for (key, seconds) in all {
        if let Some(k) = key {
            app.ctx.input.press(k, Mods::NONE);
        }
        let frames = (seconds / dt).round() as usize;
        for _ in 0..frames {
            d.trip.truck.velocity_mps = 60.0 / 2.23694;
            d.update_lane(&mut app.ctx, dt);
            clock.advance(dt);
        }
        if let Some(k) = key {
            app.ctx.input.release(k, Mods::NONE);
        }
    }
    let whips = app
        .event_lines()
        .into_iter()
        .filter(|l| l.contains("whipped"))
        .collect();
    (d.trip.truck.cargo_damage_pct - before, whips)
}

#[test]
fn a_keyboard_lane_change_on_loaded_pups_does_not_whip() {
    // A steering key is a switch, but the rear trailer feels the steer
    // build at the slew limit: a tap or a real tenth-second lane change at
    // 60 mph on loaded pups neither whips nor shifts freight, with lane
    // keeping off or partial.
    let taps: [&[(Option<Key>, f64)]; 3] = [
        &[(Some(Key::Left), 0.05)],
        &[(Some(Key::Left), 0.1), (Some(Key::Right), 0.1)],
        &[(Some(Key::Left), 0.2), (None, 0.3), (Some(Key::Right), 0.2)],
    ];
    for mode in ["off", "partial"] {
        for steps in taps {
            let mut app = TestApp::new();
            app.ctx.settings.lane_keeping = mode.to_string();
            let mut d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
            let (damage, whips) = steer_through(&mut app, &mut d, steps);
            assert_eq!(damage, 0.0, "{mode} {steps:?}");
            assert!(whips.is_empty(), "{mode} {steps:?}: {whips:?}");
            drop(app);
        }
    }
}

#[test]
fn an_evasive_swerve_on_loaded_pups_still_whips_and_says_so() {
    // Full lock one way, then the other: the swerve that really swings a
    // rear pup. It whips, the freight shifts, and one line covers it.
    let mut app = TestApp::new();
    app.ctx.settings.lane_keeping = "off".to_string();
    let mut d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    let (damage, whips) = steer_through(
        &mut app,
        &mut d,
        &[(Some(Key::Left), 1.0), (Some(Key::Right), 1.0)],
    );
    assert!(damage > 0.0);
    assert_eq!(whips, vec![REAR_TRAILER_WHIP_TEXT.to_string()]);
    assert_eq!(
        d.status_text, REAR_TRAILER_WHIP_TEXT,
        "shown as well as spoken"
    );
    // The same swerve empty still whips, and the line claims no freight.
    drop(app);
    let mut app = TestApp::new();
    app.ctx.settings.lane_keeping = "off".to_string();
    let mut d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    d.trip.truck.cargo_kg = 0.0;
    let (_, whips) = steer_through(
        &mut app,
        &mut d,
        &[(Some(Key::Left), 1.0), (Some(Key::Right), 1.0)],
    );
    assert_eq!(whips, vec![REAR_TRAILER_WHIP_EMPTY_TEXT.to_string()]);
}

#[test]
fn a_held_steer_under_partial_lane_keeping_still_whips_on_the_snap_back() {
    // Holding a key for a second under partial lane keeping throws the
    // truck well off its line; the assist's correction rides the driver's
    // steer, so its snap back still whips loaded pups, and says so.
    let mut app = TestApp::new();
    app.ctx.settings.lane_keeping = "partial".to_string();
    let mut d = drive(&mut app, "parcel_doubles", "Buffalo", "Rochester", 10.0);
    let (damage, whips) = steer_through(&mut app, &mut d, &[(Some(Key::Left), 1.0)]);
    assert!(damage > 0.0);
    assert_eq!(whips, vec![REAR_TRAILER_WHIP_TEXT.to_string()]);
}

#[test]
fn a_keyboard_lane_change_never_whips_turnpike_doubles() {
    let mut app = TestApp::new();
    app.ctx.settings.lane_keeping = "off".to_string();
    let mut d = drive(&mut app, "turnpike_doubles", "Toledo", "Elkhart", 25.0);
    let (damage, whips) = steer_through(
        &mut app,
        &mut d,
        &[(Some(Key::Left), 0.1), (Some(Key::Right), 0.1)],
    );
    assert_eq!(damage, 0.0);
    assert!(whips.is_empty(), "{whips:?}");
}

/// Drive every mapped bend of `origin`→`destination` on loaded pups with no
/// key pressed, at 20 frames a second. Bends closer than half a mile run as
/// one stretch, from a tenth of a mile before the first to a tenth past the
/// last. The speed is the advisory of the bend the truck is in, or else the
/// lowest advisory of a bend in the next 0.3 mi (the driver has slowed for
/// it), or else 55 mph. Returns the bends driven, the cargo damage and the
/// whip lines spoken.
fn drive_every_bend(
    app: &mut TestApp,
    origin: &str,
    destination: &str,
) -> (usize, f64, Vec<String>) {
    let clock = app.fake_pacer_clock();
    let mut d = drive(app, "parcel_doubles", origin, destination, 10.0);
    let mut bends: Vec<_> = d
        .trip
        .curves
        .iter()
        .filter(|c| !c.connector)
        .copied()
        .collect();
    bends.sort_by(|a, b| {
        a.start_mi
            .min(a.end_mi)
            .total_cmp(&b.start_mi.min(b.end_mi))
    });
    let span = |c: &ff_core::data::curves::RouteCurve| {
        (c.start_mi.min(c.end_mi), c.start_mi.max(c.end_mi))
    };
    let mut stretches: Vec<(f64, f64)> = Vec::new();
    for bend in &bends {
        let (lo, hi) = span(bend);
        match stretches.last_mut() {
            Some(last) if lo - last.1 < 0.5 => last.1 = last.1.max(hi),
            _ => stretches.push((lo, hi)),
        }
    }
    let speed_mph_at = |mi: f64| -> f64 {
        let here = bends.iter().find(|c| {
            let (lo, hi) = span(c);
            lo <= mi && mi <= hi
        });
        if let Some(bend) = here {
            return bend.advisory_mph as f64;
        }
        bends
            .iter()
            .filter(|c| {
                let (lo, _) = span(c);
                lo > mi && lo - mi <= 0.3
            })
            .map(|c| c.advisory_mph as f64)
            .fold(55.0, f64::min)
    };
    let dt = 0.05;
    for (lo, hi) in stretches {
        d.trip.position_mi = lo - 0.1;
        d.lane = ff_core::sim::lane::LaneKeeping::new(Some(7));
        while d.trip.position_mi < hi + 0.1 {
            let mps = speed_mph_at(d.trip.position_mi) / 2.23694;
            d.trip.truck.velocity_mps = mps;
            d.update_lane(&mut app.ctx, dt);
            d.trip.position_mi += mps * dt / 1609.344;
            clock.advance(dt);
        }
    }
    let whips = app
        .event_lines()
        .into_iter()
        .filter(|l| l.contains("whipped"))
        .collect();
    (bends.len(), d.trip.truck.cargo_damage_pct, whips)
}

#[test]
fn loaded_pups_take_every_bend_at_advisory_without_a_whip() {
    // A bend's own steer is the road's, not a quick steer by the driver:
    // at advisory speed with no key pressed, loaded pups never whip, with
    // lane keeping off or partial and curve assist on (the default).
    for (origin, destination) in [
        ("Chattanooga", "Knoxville"),
        ("Denver", "Salt Lake City"),
        ("Portland", "Seattle"),
    ] {
        for mode in ["off", "partial"] {
            let mut app = TestApp::new();
            app.ctx.settings.lane_keeping = mode.to_string();
            let (bends, damage, whips) = drive_every_bend(&mut app, origin, destination);
            assert!(bends > 5, "{origin} to {destination} has mapped bends");
            assert!(whips.is_empty(), "{origin} {mode}: {} whips", whips.len());
            assert_eq!(damage, 0.0, "{origin} {mode}");
            drop(app);
        }
    }
}
