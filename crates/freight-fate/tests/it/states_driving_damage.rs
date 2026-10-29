//! The damage bands as the DRIVE runs them: what is announced at each edge,
//! the road-speed governor winding down, the out-of-service wall and the two
//! ways off it, and what the run is graded on afterwards.
//!
//! Port of the `DrivingState` half of `tests/test_driving_damage_bands.py`.
//! Its `TruckState`-only cases -- the band ladder, the derate, the runaway,
//! the reverse guard -- live in `ff_core::sim::vehicle::damage_band_tests`,
//! where `update_wear` and `update_fuel` are reachable; four of them step
//! those directly. The cases already ported with the road mixins stay in
//! `states_driving_road.rs`; nothing is duplicated between the three.
//!
//! # What replaced the monkeypatches
//!
//! | Python | here |
//! |---|---|
//! | `monkeypatch.setattr(ctx, "say_event", stub)` | the capture at `ctx.speech`, one rung BELOW the ladder gate -- so a STATUS line at a quiet rung is read off `ctx.message_log` (which the gate still writes) rather than off the voice |
//! | `monkeypatch.setattr(type(t), "over_revving", property(lambda: True))` | a gear the road speed genuinely cannot carry, asserted at the use site |
//! | `monkeypatch.setattr(d, "_terse_speech", ...)` | `settings.driving_speech`, which is where the Rust reading comes from |
#![allow(clippy::field_reassign_with_default)]

use ff_core::achievements::int_stat;
use ff_core::models::business::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::vehicle::{
    TruckState, DAMAGE_BAND_LAST_CALL, DAMAGE_BAND_LIMP, DAMAGE_BAND_NONE,
    DAMAGE_BAND_OUT_OF_SERVICE, DAMAGE_BAND_REDUCED, DAMAGE_CREEP_CAP_MPH, DAMAGE_DERATE_PCT,
    DAMAGE_LAST_CALL_PCT, DAMAGE_LIMP_CAP_MPH, DAMAGE_LIMP_PCT, DAMAGE_MAX_PCT,
    DAMAGE_OUT_OF_SERVICE_PCT,
};
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::Menu;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_damage::{damage_summary_line, preventable_damage_charge};
use freight_fate::states::driving_menu_states::{DriveRef, DrivingStatusScreenState};

// -- rigging -------------------------------------------------------------------------

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.23694
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// `_driving(app, business_status, level)`: Denver to Salt Lake City, seeded,
/// on an empty road.
///
/// Seeded (`trip_seed=99`, as Python) and with the bubble emptied: an unseeded
/// delivery draws a fresh route and fresh weather, and an ice day moves the
/// advisory speeds these cases measure the governor against.
fn a_damage_drive(app: &mut TestApp, business_status: &str, level: usize) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Damage Tester", "Denver");
    profile.tutorial_done = true;
    profile.business_status = business_status.to_string();
    profile.career.xp = LEVEL_XP[level - 1];
    if business_status != COMPANY_DRIVER {
        profile.owned_trucks = vec!["rig".to_string()];
    }
    app.ctx.profile = Some(profile);
    let route = world
        .route_from_cities(&["Denver", "Salt Lake City"])
        .expect("Denver to Salt Lake City is a route");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Denver",
        "yard",
        "Salt Lake City",
        200.0,
        900.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(10.0),
    );
    drive.trip.truck.set_air_ready(false);
    drive.trip.set_npc_vehicles(Vec::new());
    drive.weather_mut().current = WeatherKind::Clear;
    drive
}

/// `_rolling(driving, mph)`.
fn rolling(drive: &mut DrivingState, mph: f64) {
    drive.trip.truck.engine_on = true;
    drive.trip.truck.velocity_mps = mph_to_mps(mph);
}

/// Every line the drive SUBMITTED since `from`, read off the review log.
///
/// The Python suite replaced `ctx.say_event` and so saw every call site's
/// words whatever the rung did with them. The Rust capture sits below the
/// ladder gate, which answers a STATUS line at a quiet rung with an earcon --
/// so the words are in the review log and not on the voice. The log records
/// one entry per submission and no requeues, which is also what the "said
/// once" assertions want.
fn logged_since(app: &TestApp, from: usize) -> Vec<String> {
    app.ctx.message_log.messages[from..]
        .iter()
        .map(|message| message.text.clone())
        .collect()
}

// -- spoken band edges ----------------------------------------------------------------

#[test]
fn test_terse_last_call_still_names_the_wall() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 60.0);
    let from = app.ctx.message_log.messages.len();

    drive.trip.truck.damage_pct = DAMAGE_LAST_CALL_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let lines = logged_since(&app, from);
    assert_eq!(
        lines.last().map(String::as_str),
        Some(
            format!("Damage 86 percent. Out of service at {DAMAGE_OUT_OF_SERVICE_PCT:.0}.")
                .as_str()
        ),
        "{lines:?}"
    );
}

#[test]
fn test_terse_repair_keeps_the_fact_without_the_flourish() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 5.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    let from = app.ctx.message_log.messages.len();

    drive.trip.truck.damage_pct = 30.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(
        logged_since(&app, from),
        vec!["Damage 30 percent. Full power.".to_string()]
    );
}

// -- the speed cap --------------------------------------------------------------------

#[test]
fn test_limp_cap_never_engages_during_a_pull_over() {
    // The pull-over has its own braking curve and its own spoken contract;
    // a second winding cap over the top of it would fight a stop that is
    // already happening.
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 5.0;
    drive.pull_over = Some("stopping".to_string());

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(drive.trip.truck.speed_cap_mph, None);
}

#[test]
fn test_cruise_says_once_that_limp_mode_owns_the_target() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, DAMAGE_LIMP_CAP_MPH);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 5.0;
    drive.trip.truck.speed_cap_mph = Some(DAMAGE_LIMP_CAP_MPH);
    drive.cruise_mph = Some(65.0);
    let from = app.ctx.message_log.messages.len();

    for _ in 0..10 {
        drive.announce_limp_cruise_cap(&mut app.ctx);
    }

    let said: Vec<String> = logged_since(&app, from)
        .into_iter()
        .filter(|line| line.contains("Limp mode"))
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("Holding 45"), "{:?}", said[0]);
}

// -- the out-of-service wall ----------------------------------------------------------

#[test]
fn test_a_wrecked_truck_cannot_hold_highway_speed() {
    // The owner's complaint, as a test: at the top of the meter the truck
    // used to cruise indefinitely. It now winds down to a crawl.
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 65.0);
    drive.trip.truck.damage_pct = DAMAGE_MAX_PCT;

    for _ in 0..(60 * 40) {
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
        let Some(cap) = drive.trip.truck.speed_cap_mph else {
            break; // recovery ran; the wall is behind us
        };
        drive.trip.truck.velocity_mps = drive.trip.truck.velocity_mps.min(mph_to_mps(cap));
    }

    let speed = drive.trip.truck.speed_mph();
    assert!(speed <= DAMAGE_CREEP_CAP_MPH + 0.5, "{speed}");
}

#[test]
fn test_below_the_wall_the_truck_still_drives() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT - 1.0;

    for _ in 0..(60 * 60) {
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    }

    assert_eq!(drive.trip.truck.speed_cap_mph, Some(DAMAGE_LIMP_CAP_MPH));
    assert!(!drive.trip.truck.out_of_service());
}

#[test]
fn test_the_wall_states_the_fact_the_cost_and_the_way_out() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;
    let from = app.ctx.message_log.messages.len();

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let lines = logged_since(&app, from);
    let line = lines.first().expect("the wall speaks");
    assert!(line.contains("Out of service"), "{line}");
    // An owner-operator is told the bill up front.
    assert!(line.contains("dollars"), "{line}");
    assert!(
        line.contains("shoulder") || line.contains("clear of the lane"),
        "{line}"
    );
}

#[test]
fn test_terse_wall_message_keeps_all_three() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;
    let from = app.ctx.message_log.messages.len();

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let lines = logged_since(&app, from);
    let line = lines.first().expect("the wall speaks");
    assert!(line.starts_with("Out of service."), "{line}");
    assert!(line.contains("90 percent"), "{line}");
    assert!(line.contains("dollars"), "{line}");
}

// -- recovery: owner-operator ---------------------------------------------------------

#[test]
fn test_owner_operator_pays_the_whole_bill_and_the_hours() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    app.ctx
        .profile
        .as_mut()
        .expect("a profile")
        .set_money(100.0);
    rolling(&mut drive, 0.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;
    let minutes_before = drive.trip.game_minutes;
    let from = app.ctx.message_log.messages.len();

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let cost = road_repair_cost(
        DAMAGE_OUT_OF_SERVICE_PCT,
        BREAKDOWN_REPAIR_DAMAGE_PCT,
        BREAKDOWN_CALLOUT_FEE,
    );
    // Deep damage prices on the severity curve, so recovering a wrecked truck
    // is a five-figure day rather than a flat per-percent invoice.
    assert!(
        cost > BREAKDOWN_CALLOUT_FEE + 30.0 * MECHANIC_RATE_PER_PCT,
        "{cost}"
    );
    let money = profile_of(&app.ctx).money();
    assert!(approx(money, 100.0 - cost), "{money}"); // may go negative: not optional
    assert!(money < 0.0, "{money}");
    assert_eq!(drive.trip.truck.damage_pct, BREAKDOWN_REPAIR_DAMAGE_PCT);
    assert!(!drive.trip.truck.out_of_service());
    assert!(
        approx(
            drive.trip.game_minutes,
            minutes_before + BREAKDOWN_REPAIR_MIN
        ),
        "{}",
        drive.trip.game_minutes
    );
    let spoken = logged_since(&app, from).join(" ");
    assert!(
        spoken.contains(&format!("{} dollars", fmt_grouped(cost, 0))),
        "{spoken}"
    );
}

#[test]
fn test_owner_operator_keeps_their_own_tractor() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    let before = profile_of(&app.ctx).active_truck_key();
    rolling(&mut drive, 0.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(profile_of(&app.ctx).active_truck_key(), before);
    // Nobody grades them.
    assert_eq!(profile_of(&app.ctx).career.reputation, 50.0);
}

// -- readouts -------------------------------------------------------------------------

#[test]
fn test_truck_status_line_carries_the_band_with_the_number() {
    // The Tab screen is built the way the player opens it, which needs the
    // drive on the state stack -- so this one case goes through the playtest
    // harness rather than a bare `DrivingState`.
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Damage Readout"));
    harness.with_drive(|drive, _| {
        drive.trip.set_npc_vehicles(Vec::new());
        drive.weather_mut().current = WeatherKind::Clear;
    });

    let truck_line = |harness: &mut PlaytestHarness, damage: f64| -> String {
        harness.with_drive(move |drive, _| drive.trip.truck.damage_pct = damage);
        let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
        let mut screen = DrivingStatusScreenState::new(handle, "driver");
        let items = screen.build_items(&mut harness.app.ctx);
        items
            .iter()
            .map(|item| item.text(&screen, &harness.app.ctx))
            .find(|text| text.starts_with("Truck:"))
            .expect("the driver screen carries a Truck line")
    };

    let line = truck_line(&mut harness, DAMAGE_LIMP_PCT + 3.0);
    assert!(line.contains("damage 78 percent"), "{line}");
    assert!(line.contains("limp mode"), "{line}");
    assert!(line.contains("capped at"), "{line}");

    let walled = truck_line(&mut harness, DAMAGE_OUT_OF_SERVICE_PCT);
    assert!(walled.contains("out of service"), "{walled}");

    let clean = truck_line(&mut harness, 10.0);
    assert!(!clean.contains("limp mode"), "{clean}");
    assert!(!clean.contains("reduced power"), "{clean}");
}

/// `monkeypatch.setattr(type(t), "over_revving", property(lambda: True))`.
///
/// There is no property to patch here, so the real condition is arranged
/// instead: third gear at highway speed is a road speed the gearing cannot
/// carry, which is exactly what `over_revving` means (the engine driven past
/// its governor through the wheels). Asserted, not assumed -- a gearbox
/// re-ratio would otherwise leave these two cases silently testing nothing.
fn over_rev(drive: &mut DrivingState) {
    drive.trip.truck.engine_on = true;
    drive.trip.truck.transmission.gear = 3;
    drive.trip.truck.velocity_mps = mph_to_mps(60.0);
    assert!(
        drive.trip.truck.over_revving(),
        "third gear at 60 mph no longer over-revs the engine"
    );
    drive.overrev_s = 99.0;
    drive.overrev_warn_due = 0.0;
}

#[test]
fn test_redline_speaks_the_meter_that_is_actually_moving() {
    // The warning read damage_pct while over-revving charged engine wear, so
    // it told the player nothing was being harmed. Speak the moving meter.
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    drive.trip.truck.damage_pct = 0.0;
    drive.trip.truck.engine_wear_pct = 12.0;
    over_rev(&mut drive);
    let from = app.ctx.message_log.messages.len();

    drive.update_overrev(&mut app.ctx, 1.0 / 60.0);

    let lines = logged_since(&app, from);
    let line = lines
        .iter()
        .find(|entry| entry.starts_with("Redline."))
        .unwrap_or_else(|| panic!("no redline warning: {lines:?}"));
    assert!(line.contains("12 percent"), "{line}");
    assert!(line.to_lowercase().contains("engine wear"), "{line}");
    assert!(!line.contains("0 percent"), "{line}");
}

#[test]
fn test_redline_still_names_an_active_damage_band() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 3.0;
    drive.trip.truck.engine_wear_pct = 20.0;
    over_rev(&mut drive);
    let from = app.ctx.message_log.messages.len();

    drive.update_overrev(&mut app.ctx, 1.0 / 60.0);

    let lines = logged_since(&app, from);
    let line = lines
        .iter()
        .find(|entry| entry.starts_with("Redline."))
        .unwrap_or_else(|| panic!("no redline warning: {lines:?}"));
    assert!(line.contains("limp mode"), "{line}");
}

#[test]
fn test_delivery_summary_names_the_band_with_the_damage() {
    let mut app = TestApp::new();
    let settings = &app.ctx.settings;
    let mut truck = TruckState::default();

    truck.damage_pct = 4.0;
    assert_eq!(damage_summary_line(settings, &truck, 0.5), None);

    truck.damage_pct = 12.0;
    let healthy = damage_summary_line(settings, &truck, 12.0).expect("a summary line");
    assert!(healthy.contains("12 percent truck damage"), "{healthy}");
    assert!(!healthy.contains("limp mode"), "{healthy}");

    truck.damage_pct = DAMAGE_LIMP_PCT + 3.0;
    let hurt = damage_summary_line(settings, &truck, 40.0).expect("a summary line");
    assert!(hurt.contains("78 percent"), "{hurt}");
    assert!(hurt.contains("limp mode"), "{hurt}");

    app.shutdown();
}

// -- persistence ----------------------------------------------------------------------

#[test]
fn test_snapshot_without_band_keys_resumes_from_the_damage() {
    // A save from before the bands carries neither key: the resumed drive
    // derives the announced band from the damage it does carry, so a limping
    // truck does not re-announce a band the player already heard.
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    drive.trip.truck.damage_pct = DAMAGE_LAST_CALL_PCT + 2.0;
    let mut data = drive.snapshot(&app.ctx);
    let object = data.as_object_mut().expect("the snapshot is an object");
    object.remove("damage_band");
    object.remove("limp_cap_mph");
    object.remove("out_of_service_creep_s");
    app.ctx
        .profile
        .as_mut()
        .expect("a profile")
        .set_truck_damage_pct(drive.trip.truck.damage_pct);

    let resumed = DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");

    assert_eq!(resumed.damage_band, DAMAGE_BAND_LAST_CALL);
    assert_eq!(resumed.limp_cap_mph, None);
    assert_eq!(resumed.out_of_service_creep_s, 0.0);
}

#[test]
fn test_the_creep_window_round_trips_so_a_reload_is_not_a_reset() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;
    for _ in 0..120 {
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    }
    let used = drive.out_of_service_creep_s;
    assert!(used > 1.0, "{used}");

    let data = drive.snapshot(&app.ctx);
    app.ctx
        .profile
        .as_mut()
        .expect("a profile")
        .set_truck_damage_pct(drive.trip.truck.damage_pct);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");

    assert!(
        approx(resumed.out_of_service_creep_s, used),
        "{}",
        resumed.out_of_service_creep_s
    );
}

// -- settlement -----------------------------------------------------------------------

#[test]
fn test_the_settlement_charge_scales_with_the_band_the_run_reached() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    drive.trip.truck.preventable_damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    drive.worst_damage_band = DAMAGE_BAND_REDUCED;
    let (light, light_rep, reason) = preventable_damage_charge(&drive);
    drive.worst_damage_band = DAMAGE_BAND_OUT_OF_SERVICE;
    let (heavy, heavy_rep, _) = preventable_damage_charge(&drive);

    assert!(approx(light, PREVENTABLE_DAMAGE_DEDUCTIBLE), "{light}");
    assert!(heavy > light * 3.0, "{heavy} vs {light}");
    assert!(heavy_rep > light_rep, "{heavy_rep} vs {light_rep}");
    assert!(light_rep > 0.0, "{light_rep}");
    assert!(!reason.is_empty());

    app.shutdown();
}

#[test]
fn test_a_clean_run_is_charged_nothing() {
    let mut app = TestApp::new();
    let drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);

    assert_eq!(preventable_damage_charge(&drive), (0.0, 0.0, String::new()));

    app.shutdown();
}

#[test]
fn test_hazard_damage_alone_is_not_ruled_preventable() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    drive.trip.truck.add_damage(DAMAGE_LIMP_PCT + 2.0, false);
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let (deductible, reputation, _) = preventable_damage_charge(&drive);

    assert_eq!(deductible, 0.0);
    assert_eq!(reputation, 0.0);
    // The run really did reach limp mode: what spares the driver is the
    // damage having been ruled unpreventable, not the band never happening.
    assert_eq!(drive.worst_damage_band, DAMAGE_BAND_LIMP);
}

#[test]
fn test_the_settlement_grade_round_trips_through_a_snapshot() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    rolling(&mut drive, 60.0);
    drive.trip.truck.add_damage(DAMAGE_LIMP_PCT + 2.0, true);
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    let preventable = drive.trip.truck.preventable_damage_pct;

    let data = drive.snapshot(&app.ctx);
    app.ctx
        .profile
        .as_mut()
        .expect("a profile")
        .set_truck_damage_pct(drive.trip.truck.damage_pct);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");

    assert_eq!(resumed.worst_damage_band, DAMAGE_BAND_LIMP);
    assert!(
        approx(resumed.trip.truck.preventable_damage_pct, preventable),
        "{}",
        resumed.trip.truck.preventable_damage_pct
    );
    assert_ne!(DAMAGE_BAND_NONE, resumed.worst_damage_band);
}

/// The drive as dispatch hands it out in a tractor that left the yard at
/// `damage` percent, the way `DrivingState::new` reads it off the profile.
fn left_the_yard_at(drive: &mut DrivingState, damage: f64) {
    drive.trip.truck.damage_pct = damage;
    drive.start_damage = damage;
    drive.damage_band = drive.trip.truck.damage_band();
    drive.worst_damage_band = drive.damage_band;
}

#[test]
fn test_damage_the_truck_left_the_yard_with_is_not_ruled_again() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    rolling(&mut drive, 60.0);
    left_the_yard_at(&mut drive, DAMAGE_DERATE_PCT + 5.0);
    // One brush with the rumble strip on a run that never went deeper.
    drive.trip.truck.add_damage(1.0, true);
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(preventable_damage_charge(&drive), (0.0, 0.0, String::new()));
}

#[test]
fn test_a_run_that_takes_a_damaged_truck_deeper_is_still_ruled() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    rolling(&mut drive, 60.0);
    left_the_yard_at(&mut drive, DAMAGE_DERATE_PCT + 5.0);
    drive
        .trip
        .truck
        .add_damage(DAMAGE_LIMP_PCT - DAMAGE_DERATE_PCT, true);
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let (deductible, reputation, reason) = preventable_damage_charge(&drive);

    assert!(deductible > 0.0);
    assert!(reputation > 0.0);
    assert_eq!(reason, "the run finished in limp mode");
}

#[test]
fn test_a_grounded_run_is_one_equipment_event_not_two() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    rolling(&mut drive, 0.0);
    drive.trip.truck.add_damage(DAMAGE_OUT_OF_SERVICE_PCT, true);
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    let profile = app.ctx.profile.as_mut().expect("a profile");
    assert_eq!(int_stat(profile, "preventable_equipment_damage"), 1);

    // The dock rules the same run preventable; a reload in between must not
    // forget that the grounding already counted it.
    let data = drive.snapshot(&app.ctx);
    let mut resumed =
        DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");
    resumed.record_equipment_event(&mut app.ctx);

    let profile = app.ctx.profile.as_mut().expect("a profile");
    assert_eq!(int_stat(profile, "preventable_equipment_damage"), 1);
}

#[path = "maintenance_limits.rs"]
mod maintenance_limits;
