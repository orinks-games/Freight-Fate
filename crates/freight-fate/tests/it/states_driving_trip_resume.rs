//! Mid-trip save and resume: the snapshot, what survives a quit, and the
//! continue flow (port of `tests/test_trip_resume.py`).
//!
//! Every case here drives the app shell, which is why they could never run
//! where the port left them: they were `#[ignore]`d stubs in
//! `crates/ff-core/tests/sim_trip_resume.rs`, and `ff-core` cannot see
//! `DrivingState`, the pause menu or the main menu at all. Only
//! `test_route_from_cities_roundtrip` is genuinely world-data work, and it
//! stays in `ff-core`.
//!
//! `start_drive(app)` -- new career, accept the assigned dispatch, depart --
//! is [`PlaytestHarness::start_delivery`], which walks the same menus.
//!
//! # What replaced the monkeypatches
//!
//! | Python | here |
//! |---|---|
//! | `monkeypatch.setattr(ctx, "say", stub)` | the harness records at `ctx.speech`, one rung lower |
//! | `monkeypatch.setattr(ctx, "real_weather_provider", lambda: provider)` | the setting is flipped and the WIRING is asserted, then the live provider is swapped for a fake -- `GameContext::real_weather_provider` hands back a concrete type, so there is no fake to hand it |

use serde_json::{json, Map, Value};

use ff_core::models::jobs::{fair_active_deadline, job_from_payload};
use ff_core::models::profile::Profile;
use ff_core::sim::hos::HosClock;
use ff_core::sim::trip_models::TripEventKind;
use ff_core::sim::weather::{WeatherKind, WeatherProvider};
use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, State};
use freight_fate::states::city::CityMenuState;
use freight_fate::states::driving::{DrivingState, ACTIVE_TRIP_DEADLINE_MODEL};
use freight_fate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};
use freight_fate::states::driving_pause_states::{
    AbandonJobConfirmationState, PauseMenuState, QuitWhileMovingConfirmationState,
};
use freight_fate::states::main_menu::{enter_world, MainMenuState};

const DT: f64 = 1.0 / 60.0;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// `start_drive(app)`: new career, accept the assigned dispatch, depart.
fn start_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness
}

/// `drive_some(driving, miles)`: advance the trip a few miles with simulated
/// full-throttle frames.
fn drive_some(harness: &mut PlaytestHarness, miles: f64) {
    harness.press_key(Key::E, Some('e'));
    harness.with_drive(move |d, _| {
        d.trip.truck.transmission.automatic = true;
        d.trip.truck.set_air_ready(false);
        for _ in 0..(60 * 60 * 5) {
            d.trip.truck.throttle = 0.9;
            d.trip.truck.auto_shift();
            d.trip.truck.update(DT);
            d.trip.update(DT);
            if d.trip.position_mi >= miles {
                break;
            }
        }
        assert!(d.trip.position_mi >= miles, "{}", d.trip.position_mi);
    });
}

/// Run `f` on whatever drive is on top of the stack.
///
/// `PlaytestHarness::with_drive` holds the drive the harness itself started;
/// a resumed drive is a different object, so these cases reach for the active
/// state instead.
fn with_active_drive<R>(
    harness: &mut PlaytestHarness,
    f: impl FnOnce(&mut DrivingState, &mut freight_fate::app::GameContext) -> R,
) -> R {
    let state = harness.app.ctx.state().expect("a state on the stack");
    let mut borrowed = state.borrow_mut();
    let drive = borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .expect("the active state is a drive");
    let out = f(drive, &mut harness.app.ctx);
    drop(borrowed);
    harness.app.ctx.run_deferred();
    out
}

/// `quit_to_menu(app)`. A moving truck gets the lose-your-progress
/// confirmation first (owner loss, 2026-07-27); a parked one quits
/// straight through.
fn quit_to_menu(harness: &mut PlaytestHarness) {
    harness.key(InputEvent::key(Key::Escape));
    assert!(harness.state_is::<PauseMenuState>());
    harness.select_menu_item("Quit to main menu");
    if harness.state_is::<QuitWhileMovingConfirmationState>() {
        let row = harness
            .menu_labels()
            .into_iter()
            .find(|label| label.starts_with("Quit anyway"))
            .expect("the moving-quit confirmation offers Quit anyway");
        harness.select_menu_item(&row);
    }
    assert!(harness.state_is::<MainMenuState>());
}

#[test]
fn quitting_while_moving_asks_first_and_names_the_miles() {
    // The owner quit at 76 on I-80 and lost 67 miles: the old warning
    // spoke WHILE the quit executed. Moving quits must ask first, name
    // the cost, and let "Keep driving" cancel. Parked quits stay instant.
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Quitter"));
    harness.prepare_for_driving(60.0);
    with_active_drive(&mut harness, |d, _| {
        d.trip.position_mi += 10.0;
    });
    harness.clear_speech();

    harness.key(InputEvent::key(Key::Escape));
    assert!(harness.state_is::<PauseMenuState>());
    harness.select_menu_item("Quit to main menu");
    assert!(
        harness.state_is::<QuitWhileMovingConfirmationState>(),
        "a moving quit must confirm first"
    );
    let heard = harness.transcript_text();
    assert!(
        heard.contains("Quitting loses 10 miles") && heard.contains("since your last stop"),
        "{heard}"
    );
    // Keep driving cancels: back on the pause menu, nothing lost.
    harness.select_menu_item("Keep driving");
    assert!(harness.state_is::<PauseMenuState>());

    // Parked, the quit stays instant -- no confirmation in the way.
    harness.key(InputEvent::key(Key::Escape)); // back to the drive
    with_active_drive(&mut harness, |d, _| {
        d.trip.truck.velocity_mps = 0.0;
    });
    harness.key(InputEvent::key(Key::Escape));
    assert!(harness.state_is::<PauseMenuState>());
    harness.select_menu_item("Quit to main menu");
    assert!(
        harness.state_is::<MainMenuState>(),
        "a parked quit goes straight through"
    );
}

/// Continue the saved career from the main menu.
fn continue_latest_career(harness: &mut PlaytestHarness) {
    let labels = harness.menu_labels();
    let row = labels
        .iter()
        .find(|row| row.starts_with("Continue latest career"))
        .cloned()
        .unwrap_or_else(|| panic!("no continue row on the title screen: {labels:?}"));
    harness.select_menu_item(&row);
}

/// The provider `test_resumed_drive_reports_old_fresh_observation_as_live`
/// needs: a reading that is twelve minutes old but freshly fetched.
struct FreshOldProvider;

impl WeatherProvider for FreshOldProvider {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        Some(WeatherKind::Rain)
    }
    fn observation_age_s(&mut self, _city: &str) -> Option<f64> {
        Some(12.0 * 60.0)
    }
    // A mild station reading, so the real-calendar season model (read off
    // the wall clock) never turns this rain to ice on a cold night.
    fn get_temperature(&mut self, _city: &str) -> Option<f64> {
        Some(15.0)
    }
}

/// A provider that answers nothing, so a swapped-in live source never reaches
/// the network from a test.
struct SilentProvider;

impl WeatherProvider for SilentProvider {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        None
    }
}

/// The 1.2-1.4 era mid-trip save these cases resume from.
fn old_active_trip(route_cities: &[&str], position_mi: f64, game_minutes: f64) -> Value {
    json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "Chicago",
            "origin_location": "Cicero Rail Hub",
            "destination": "Denver",
            "distance_mi": 1150.0,
            "pay": 2800.0,
            "deadline_game_h": 31.0,
            "market_mult": 1.0,
        },
        "route_cities": route_cities,
        "trip_seed": 1234,
        "position_mi": position_mi,
        "game_minutes": game_minutes,
        "start_damage": 3.0,
        // A pre-removal snapshot field; kept here so the resume path keeps
        // proving it loads an in-flight save from before the silent
        // at-delivery speeding charge was deleted.
        "speeding_strikes": 1,
    })
}

// -- the snapshot the drive writes -------------------------------------------------------

#[test]
fn test_active_drive_snapshot_restores_idling_engine() {
    let mut harness = start_drive("Idling");
    harness.with_drive(|d, _| d.trip.truck.start_engine());
    let snapshot = harness.with_drive(|d, ctx| d.snapshot(ctx));

    let resumed = DrivingState::from_snapshot(&mut harness.app.ctx, &snapshot)
        .expect("the snapshot the drive just wrote reloads");

    assert!(resumed.trip.truck.engine_on);
}

#[test]
fn test_active_drive_snapshot_restores_paused_speed_control_session() {
    let mut harness = start_drive("Paused Cruise");
    harness.with_drive(|d, ctx| d.restore_speed_control_session(ctx, true, Some(52.0)));
    let snapshot = harness.with_drive(|d, ctx| d.snapshot(ctx));

    let mut resumed =
        DrivingState::from_snapshot(&mut harness.app.ctx, &snapshot).expect("the snapshot reloads");
    assert!(resumed.speed_control_armed);
    assert_eq!(resumed.speed_control_target_mph, Some(52.0));
    assert_eq!(resumed.keeper_mph, None);
    assert_eq!(resumed.cruise_mph, None);

    harness.clear_speech();
    State::enter(&mut resumed, &mut harness.app.ctx);
    let lines = harness.app.speech().lines();
    let resume_message = lines
        .iter()
        .find(|line| line.contains("Automatic speed control paused."))
        .unwrap_or_else(|| panic!("no paused-cruise line: {lines:#?}"));
    // Just the state: the target is on Space and Tab, the key in the help.
    assert!(
        !resume_message.contains("open-road target"),
        "{resume_message}"
    );
}

#[test]
fn test_resumed_drive_reports_old_fresh_observation_as_live() {
    let mut harness = start_drive("Fresh Old");
    let snapshot = harness.with_drive(|d, ctx| d.snapshot(ctx));
    let mut resumed =
        DrivingState::from_snapshot(&mut harness.app.ctx, &snapshot).expect("the snapshot reloads");
    resumed.trip.weather.provider = Some(Box::new(FreshOldProvider));
    resumed.trip.weather.live = false;
    resumed.trip.weather.city = None;
    resumed.trip.update(0.0);

    harness.clear_speech();
    State::enter(&mut resumed, &mut harness.app.ctx);

    let lines = harness.app.speech().lines();
    let resume_message = lines
        .iter()
        .find(|line| line.starts_with("Resuming "))
        .unwrap_or_else(|| panic!("no resume line: {lines:#?}"));
    assert!(
        resume_message.contains("Live weather: rain"),
        "{resume_message}"
    );
    assert!(!resume_message.contains("minutes old"), "{resume_message}");
    assert!(
        !resume_message.to_lowercase().contains("updating"),
        "{resume_message}"
    );
}

#[test]
fn test_snapshot_roundtrip_preserves_air_brake_state() {
    let mut harness = start_drive("Air Brakes");
    harness.with_drive(|d, _| {
        d.trip.truck.primary_air_psi = 88.0;
        d.trip.truck.secondary_air_psi = 92.0;
        d.trip.truck.trailer_air_psi = 95.0;
        d.trip.truck.parking_brake = false;
    });
    let snap = harness.with_drive(|d, ctx| d.snapshot(ctx));

    let resumed =
        DrivingState::from_snapshot(&mut harness.app.ctx, &snap).expect("the snapshot reloads");

    assert!(approx(resumed.trip.truck.air_pressure_psi(), 88.0));
    assert!(approx(resumed.trip.truck.primary_air_psi, 88.0));
    assert!(approx(resumed.trip.truck.secondary_air_psi, 92.0));
    assert!(approx(resumed.trip.truck.trailer_air_psi, 95.0));
    assert!(!resumed.trip.truck.parking_brake);
}

#[test]
fn test_snapshot_survives_profile_roundtrip() {
    let mut harness = start_drive("Roundtrip");
    drive_some(&mut harness, 8.0);
    quit_to_menu(&mut harness);
    let (path, active_trip) = {
        let p = harness.app.ctx.profile.as_ref().expect("a career");
        (p.path(), p.active_trip.clone())
    };

    let loaded = Profile::load(&path).expect("the career reloads from disk");

    assert_eq!(loaded.active_trip, active_trip);
}

// -- quitting mid-drive --------------------------------------------------------------------

#[test]
fn test_quit_mid_drive_restores_checkpoint_hos_and_fatigue() {
    // Quitting must not mix live driver state with the saved road checkpoint.
    let mut harness = start_drive("Checkpoint");
    let checkpoint = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .clone()
        .expect("departing wrote a checkpoint");
    let checkpoint_hos = checkpoint
        .get("hos")
        .cloned()
        .expect("the checkpoint clock");
    let checkpoint_fatigue = checkpoint
        .get("fatigue")
        .and_then(Value::as_f64)
        .expect("the checkpoint fatigue");

    // Driver state accumulated after leaving the saved stop.
    {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        p.hos.driving_min = 218.9;
        p.hos.duty_min = 219.9;
        p.hos.since_break_min = 218.9;
        p.fatigue = 25.18;
        assert_ne!(json!(p.hos.to_dict()), checkpoint_hos);
        assert_ne!(p.fatigue, checkpoint_fatigue);
    }

    quit_to_menu(&mut harness);

    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert_eq!(json!(p.hos.to_dict()), checkpoint_hos);
    assert!(approx(p.fatigue, checkpoint_fatigue));
}

#[test]
fn test_quit_mid_drive_resumes_from_the_last_stop() {
    // Saving is stops-only: quitting mid-drive does not persist the in-progress
    // position, so Continue resumes the leg from where it was last departed
    // (here, the origin terminal at the leg start), not from mid-drive.
    let mut harness = start_drive("Last Stop");
    let (destination, cities) =
        harness.read_drive(|d| (d.job.destination.clone(), d.route.cities.clone()));
    drive_some(&mut harness, 8.0);
    assert!(harness.read_drive(|d| d.trip.position_mi) > 0.0);
    quit_to_menu(&mut harness);

    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .active_trip
            .as_ref()
            .and_then(|trip| trip.get("position_mi"))
            .and_then(Value::as_f64),
        Some(0.0),
        "the leg start, not mid-drive"
    );

    continue_latest_career(&mut harness);
    assert!(harness.state_is::<DrivingState>());
    let resumed = harness.app.state().expect("the resumed drive");
    let resumed = resumed.borrow();
    let resumed = resumed
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("a drive");
    assert!(resumed.resumed);
    assert_eq!(resumed.job.destination, destination);
    assert_eq!(resumed.route.cities, cities);
    assert_eq!(resumed.trip.position_mi, 0.0);
    // the truck resumes parked
    assert!(!resumed.trip.truck.engine_on);
    assert_eq!(resumed.trip.truck.velocity_mps, 0.0);
}

#[test]
fn test_resumed_trip_does_not_replay_passed_announcements() {
    let mut harness = start_drive("No Replay");
    drive_some(&mut harness, 8.0);
    quit_to_menu(&mut harness);
    continue_latest_career(&mut harness);
    assert!(harness.state_is::<DrivingState>());

    // the first idle frame must not re-announce stops/cities behind us
    let events = with_active_drive(&mut harness, |d, _| d.trip.update(DT));

    let replayed: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                TripEventKind::StopAhead | TripEventKind::CityReached | TripEventKind::ZoneEnter
            )
        })
        .collect();
    assert!(replayed.is_empty(), "{replayed:#?}");
}

#[test]
fn test_delivery_clears_the_saved_trip() {
    let mut harness = start_drive("Delivered");
    drive_some(&mut harness, 8.0);
    quit_to_menu(&mut harness);
    assert!(harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .is_some());
    continue_latest_career(&mut harness);
    assert!(harness.state_is::<DrivingState>());

    with_active_drive(&mut harness, |d, ctx| {
        d.trip.position_mi = d.trip.total_miles(); // teleport to arrival
        d.trip.update(DT);
        d.trip.truck.velocity_mps = 0.0;
        d.handle_arrival_gate(ctx);
    });
    harness.finish_timed_state();
    assert!(harness.state_is::<FacilityArrivalState>());
    harness.key(InputEvent::key(Key::Return));
    harness.finish_timed_state();
    assert!(harness.state_is::<ArrivalState>());
    assert!(harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .is_none());
}

#[test]
fn test_abandoning_clears_the_saved_trip() {
    let mut harness = start_drive("Abandoned");
    drive_some(&mut harness, 8.0);
    let snapshot = harness.with_drive(|d, ctx| d.snapshot(ctx));
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .active_trip = Some(snapshot);

    harness.key(InputEvent::key(Key::Escape));
    assert!(harness.state_is::<PauseMenuState>());
    harness.select_menu_item("Abandon job");
    assert!(harness.state_is::<AbandonJobConfirmationState>());
    harness.key(InputEvent::key(Key::Down)); // arrow to Yes
    harness.key(InputEvent::key(Key::Return));

    assert!(harness.state_is::<CityMenuState>());
    assert!(harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .is_none());
}

#[test]
fn test_abandoning_keeps_the_hours_spent_driving() {
    // Regression: abandoning a job snapped the world clock back to the
    // departure time, while HOS and fatigue kept the accrued hours.
    let mut harness = start_drive("Hours Kept");
    drive_some(&mut harness, 8.0);
    let before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .game_hours;
    let spent = harness.read_drive(|d| d.trip.game_minutes) / 60.0;
    assert!(spent > 0.0);

    harness.key(InputEvent::key(Key::Escape));
    assert!(harness.state_is::<PauseMenuState>());
    harness.select_menu_item("Abandon job");
    assert!(harness.state_is::<AbandonJobConfirmationState>());
    harness.key(InputEvent::key(Key::Down)); // arrow to Yes
    harness.key(InputEvent::key(Key::Return));

    assert!(harness.state_is::<CityMenuState>());
    let after = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .game_hours;
    assert!(
        approx(after, before + spent),
        "{after} vs {}",
        before + spent
    );
}

// -- settings that must reach the drive already under way -----------------------------------

#[test]
fn test_trip_pacing_change_applies_to_the_active_trip() {
    // Regression: changing Trip pacing from the pause menu was silently
    // ignored until the next delivery.
    let mut harness = start_drive("Pacing");
    assert_eq!(
        harness.read_drive(|d| d.trip.time_scale),
        harness.app.ctx.settings.time_scale
    );
    harness.app.ctx.settings.time_scale = 40.0;

    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));

    assert_eq!(harness.read_drive(|d| d.trip.time_scale), 40.0);
}

#[test]
fn test_weather_source_change_applies_to_the_active_trip() {
    // Regression: the pause-menu setting changed the label, but the current
    // drive kept using the old weather source until the next job.
    //
    // Python patched `ctx.real_weather_provider` and asserted the trip picked
    // up that exact object. `GameContext::real_weather_provider` hands back a
    // concrete `RealWeatherProvider`, so there is no fake to hand it: the
    // wiring itself is asserted (the setting really did give the live drive a
    // provider), and the live one is then swapped for a silent fake so nothing
    // here reaches the network.
    let mut harness = start_drive("Weather Source");
    assert!(harness.read_drive(|d| d.trip.weather.provider.is_none()));

    harness.app.ctx.settings.real_weather = true;
    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));
    assert!(harness.read_drive(|d| d.trip.weather.provider.is_some()));
    harness.with_drive(|d, _| {
        d.trip.weather.provider = Some(Box::new(SilentProvider));
        d.trip.weather.live = true;
    });

    harness.app.ctx.settings.real_weather = false;
    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));

    assert!(harness.read_drive(|d| d.trip.weather.provider.is_none()));
    assert!(!harness.read_drive(|d| d.trip.weather.live));
}

#[test]
fn test_live_weather_calendar_change_applies_to_active_trip() {
    use ff_core::sim::season::date_text;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.real_weather = true;
    harness.start_delivery(StartDelivery::named("Calendar"));
    // Live from here on is a fake: the wiring is what the case is about, and a
    // real provider would reach the network.
    assert!(harness.read_drive(|d| d.trip.weather.provider.is_some()));
    harness.with_drive(|d, _| d.trip.weather.provider = Some(Box::new(SilentProvider)));
    assert!(harness.read_drive(|d| d.trip.weather.live_weather_controls_calendar));

    harness.app.ctx.settings.live_weather_controls_calendar = false;
    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));

    assert!(!harness.read_drive(|d| d.trip.weather.live_weather_controls_calendar));
    let (shown, hours) =
        harness.read_drive(|d| (d.trip.weather.date_text(), d.trip.weather.game_hours));
    let hours = hours.expect("an independent calendar carries its own clock");
    assert_eq!(shown, Some(date_text(hours)));
}

#[test]
fn test_arrival_summary_calls_out_on_time_delivery_bonus() {
    let mut harness = start_drive("On Time");
    harness.with_drive(|d, _| d.trip.game_minutes = d.job.deadline_game_h * 30.0);

    let parts = harness.with_drive(|d, ctx| ArrivalState::new(ctx, d).summary_parts.clone());

    assert!(
        parts
            .iter()
            .any(|part| part.contains("On-time delivery bonus")),
        "{parts:#?}"
    );
}

// -- saves written by older builds ----------------------------------------------------------

#[test]
fn test_corrupt_snapshot_falls_back_to_city() {
    let mut app = TestApp::new();
    let mut p = Profile::named("Corrupt");
    p.active_trip = Some(json!({"job": {"cargo": "no_such_cargo"}}));
    app.ctx.profile = Some(p);

    enter_world(&mut app.ctx, false);

    assert!(app
        .state()
        .expect("a state")
        .borrow()
        .as_any()
        .is::<CityMenuState>());
    assert!(app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .is_none());
}

#[test]
fn test_old_map_snapshot_still_resumes() {
    // A mid-trip save written against the 21-city 1.2.x map must resume. The
    // route below only uses legs from the original map; they are required to
    // survive every map expansion (see ORIGINAL_ADJACENT_PAIRS in
    // `tests/data_world.rs`).
    let mut app = TestApp::new();
    let mut p = Profile::named("Old Save");
    p.active_trip = Some(old_active_trip(
        &["Chicago", "St. Louis", "Kansas City", "Denver"],
        412.0,
        540.0,
    ));
    app.ctx.profile = Some(p);

    enter_world(&mut app.ctx, false);

    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the old save resumes at the wheel");
    assert!(drive.resumed);
    // the old display names canonicalize to the stable slug keys...
    assert_eq!(
        drive.route.cities,
        vec![
            "chicago_il_us".to_string(),
            "st_louis_mo_us".to_string(),
            "kansas_city_mo_us".to_string(),
            "denver_co_us".to_string(),
        ]
    );
    assert_eq!(drive.trip.position_mi, 412.0);
    assert_eq!(drive.job.destination, "denver_co_us");
    // ...while the spoken destination stays what the player has heard
    assert_eq!(drive.job.spoken_destination(), "Denver");
    assert!(drive.trip.truck.air_ready());
    assert!(drive.trip.truck.parking_brake);
}

#[test]
fn test_pre_1_5_snapshot_resumes_with_fresh_clock() {
    // A 1.2-1.4 era snapshot (no HOS keys) must load with defaults.
    let mut app = TestApp::new();
    let mut p = Profile::named("Old Save");
    p.active_trip = Some(old_active_trip(
        &["Chicago", "St. Louis", "Kansas City", "Denver"],
        412.0,
        540.0,
    ));
    app.ctx.profile = Some(p);

    enter_world(&mut app.ctx, false);

    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the old save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(drive.hos_fine_count, 0);
    drop(state);
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.hos, HosClock::new());
    assert_eq!(p.fatigue, 0.0);
}

#[test]
fn test_old_active_trip_gets_deadline_floor_and_model_marker() {
    // Both halves matter. The resumed DRIVE getting the floor is the
    // migration; the SAVE getting it back is what makes the migration
    // one-time. `from_snapshot` reads a `&Value` and cannot write the marker
    // itself, so the resume call site in `main_menu.rs` persists it -- see
    // `persist_deadline_migration` there.
    let mut app = TestApp::new();
    let route_cities = ["San Antonio", "Dallas"];
    let original_deadline = 3.0;
    let job_payload = json!({
        "cargo": "general",
        "weight_tons": 14.0,
        "origin": "San Antonio",
        "origin_location": "San Antonio freight market",
        "destination": "Dallas",
        "distance_mi": 275.0,
        "pay": 1200.0,
        "deadline_game_h": original_deadline,
        "market_mult": 1.0,
    });
    let mut p = Profile::named("Deadline Migration");
    p.active_trip = Some(json!({
        "job": job_payload,
        "route_cities": route_cities,
        "trip_seed": 1234,
        "position_mi": 50.0,
        "game_minutes": 180.0,
        "start_damage": 0.0,
        "speeding_strikes": 0,
    }));
    app.ctx.profile = Some(p);

    let owned: Vec<String> = route_cities.iter().map(|c| c.to_string()).collect();
    let route = app
        .ctx
        .world
        .route_from_cities(&owned)
        .expect("the corridor is on the map");
    let job = job_from_payload(job_payload.as_object().expect("a job payload"))
        .expect("the payload is a job");
    let expected = fair_active_deadline(&job, &route, 3.0, 50.0, Some(app.ctx.world));

    enter_world(&mut app.ctx, false);

    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the old save resumes at the wheel");
    assert_eq!(drive.job.deadline_game_h, expected);
    assert!(drive.job.deadline_game_h > original_deadline);
    drop(state);
    let saved = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .clone()
        .expect("the migrated snapshot is written back");
    assert_eq!(
        saved["job"]["deadline_game_h"].as_f64(),
        Some(expected),
        "{saved}"
    );
    assert_eq!(
        saved["deadline_model"].as_i64(),
        Some(ACTIVE_TRIP_DEADLINE_MODEL),
        "{saved}"
    );
}

#[test]
fn prior_model_active_delivery_repairs_forced_sleep_once() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let mut p = Profile::named("Forced Sleep Migration");
    p.hos.duty_min = 14.0 * 60.0;
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "San Antonio",
            "origin_location": "San Antonio freight market",
            "destination": "Dallas",
            "distance_mi": 275.0,
            "pay": 1200.0,
            "deadline_game_h": 3.2,
            "market_mult": 1.0,
        },
        "route_cities": ["San Antonio", "Dallas"],
        "trip_seed": 1234,
        "position_mi": 0.0,
        "game_minutes": 60.0,
        "deadline_model": 1,
    }));
    let mut already_late = p.active_trip.clone().unwrap();
    already_late["game_minutes"] = json!(300.0);
    app.ctx.profile = Some(p);

    enter_world(&mut app.ctx, false);
    let saved = app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .active_trip
        .clone()
        .unwrap();
    let repaired = saved["job"]["deadline_game_h"].as_f64().unwrap();
    assert!(repaired > 11.0);
    assert_eq!(saved["job"]["deadline_covers_rest"], true);
    assert_eq!(saved["deadline_model"], ACTIVE_TRIP_DEADLINE_MODEL);
    let announcement = app.main_lines().join(" ");
    assert!(
        announcement.contains("adjusted this saved delivery deadline"),
        "{announcement}"
    );
    assert!(
        announcement.contains("covers the 10-hour sleep"),
        "{announcement}"
    );

    app.clear_speech();
    let _ = freight_fate::states::main_menu::world_entry_state(&mut app.ctx, false);
    assert!(!app
        .main_lines()
        .join(" ")
        .contains("adjusted this saved delivery deadline"));

    let mut later = saved.clone();
    later["game_minutes"] = json!(repaired * 120.0);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &later).unwrap();
    assert_eq!(resumed.job.deadline_game_h, repaired);
    let late = DrivingState::from_snapshot(&mut app.ctx, &already_late).unwrap();
    assert_eq!(late.job.deadline_game_h, 3.2);
    drop(app);

    let mut late_app = TestApp::new();
    late_app.ctx.settings.hos_mode = "realistic".to_string();
    let mut late_profile = Profile::named("Already Late Migration");
    late_profile.hos.duty_min = 14.0 * 60.0;
    late_profile.active_trip = Some(already_late);
    late_app.ctx.profile = Some(late_profile);
    enter_world(&mut late_app.ctx, false);
    let saved_late = late_app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .active_trip
        .as_ref()
        .unwrap();
    assert_eq!(saved_late["job"]["deadline_game_h"], 3.2);
    assert!(!late_app
        .main_lines()
        .join(" ")
        .contains("adjusted this saved delivery deadline"));
}

#[test]
fn test_current_active_trip_keeps_its_deadline_across_resumes() {
    // The fair-deadline floor is a migration, not a per-resume top-up.
    //
    // Recalculating it every time a run was continued let a late driver buy
    // back hours by saving at a stop and continuing, so a snapshot already
    // written at the current deadline model must come back with the deadline
    // untouched.
    let mut harness = start_drive("Deadline Held");
    let mut snapshot = harness.with_drive(|d, ctx| d.snapshot(ctx));
    assert_eq!(
        snapshot["deadline_model"].as_i64(),
        Some(ACTIVE_TRIP_DEADLINE_MODEL)
    );
    let deadline = harness.read_drive(|d| d.job.deadline_game_h);

    // Deep into the run and out of hours: the old unconditional floor keyed
    // off hours used, so this is exactly where it used to hand out more.
    let total = harness.read_drive(|d| d.trip.total_miles());
    let object: &mut Map<String, Value> = snapshot.as_object_mut().expect("a snapshot object");
    object.insert("position_mi".to_string(), json!(total * 0.9));
    object.insert("game_minutes".to_string(), json!(deadline * 60.0 * 2.0));

    let resumed =
        DrivingState::from_snapshot(&mut harness.app.ctx, &snapshot).expect("the snapshot reloads");

    assert_eq!(resumed.job.deadline_game_h, deadline);
    assert_eq!(snapshot["job"]["deadline_game_h"].as_f64(), Some(deadline));
}

#[test]
fn test_resumed_drive_advances_the_calendar_by_the_time_already_driven() {
    // Season, date, and simulated weather share the trip clock, not the start
    // hour.
    let mut harness = start_drive("Calendar Advance");
    let mut snapshot = harness.with_drive(|d, ctx| d.snapshot(ctx));
    snapshot
        .as_object_mut()
        .expect("a snapshot object")
        .insert("game_minutes".to_string(), json!(30.0 * 60.0)); // a day and a bit on the road

    let resumed =
        DrivingState::from_snapshot(&mut harness.app.ctx, &snapshot).expect("the snapshot reloads");

    let calendar = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .calendar_game_hours();
    let game_hours = resumed
        .trip
        .weather
        .game_hours
        .expect("a resumed trip carries its own calendar clock");
    assert!(approx(game_hours, calendar + 30.0));
}

#[test]
fn test_bare_city_job_snapshot_gets_facility_fallback() {
    let mut app = TestApp::new();
    let mut p = Profile::named("Bare City Save");
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "Chicago",
            "destination": "St. Louis",
            "distance_mi": 298.0,
            "pay": 1200.0,
            "deadline_game_h": 9.0,
            "market_mult": 1.0,
        },
        "route_cities": ["Chicago", "St. Louis"],
        "trip_seed": 1234,
        "position_mi": 20.0,
        "game_minutes": 30.0,
        "start_damage": 0.0,
        "speeding_strikes": 0,
    }));
    app.ctx.profile = Some(p);

    enter_world(&mut app.ctx, false);

    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the bare snapshot resumes at the wheel");
    assert_eq!(
        drive.job.origin_facility_text(),
        "the Chicago metro freight market"
    );
    assert_eq!(
        drive.job.destination_facility_text(),
        "the St. Louis metro freight market"
    );
}

// `test_route_from_cities_roundtrip` is world-data work with no app shell in
// it; it stays live in `crates/ff-core/tests/sim_trip_resume.rs`.
