//! What the roadside writes on the license: the serious-violation ladder, the
//! major offense, the fatigue events, and the exploits the adversarial
//! harness found.
//!
//! These are the `tests/test_enforcement_record.py` cases that drive a real
//! `DrivingState` and the screens it pushes. They spent the port as
//! `#[ignore]`d stubs in `crates/ff-core/src/models/enforcement/tests.rs`,
//! where they could never run: `ff-core` cannot depend on the game crate, so
//! `TrafficStopState`, `EnforcementStopState`, `FelonyStopState` and the
//! microsleep handler are invisible from there. The record's own arithmetic
//! -- the fine schedule, the three-year window, the suspension dates -- stays
//! in `ff-core` beside the model it tests; the dispatch board half is live in
//! `states_city.rs`.
//!
//! # What replaced the monkeypatches
//!
//! | Python | here |
//! |---|---|
//! | `monkeypatch.setattr(ctx, "say"/"say_event", stub)` | the capture behind `ctx.speech`, one rung lower, so the ladder and the pacer are in the picture |
//! | `monkeypatch.setattr(ctx.audio, "play", stub)` | the headless audio the test app already has |
//! | `monkeypatch.setattr(pygame.key, "get_pressed", {K_x: True})` | `ctx.input.press(Key::X, Mods::SHIFT)`, the real held key |
//! | `monkeypatch.setattr(ctx, "save_profile", noop)` | left alone: the test app saves into its own temp data directory |

use ff_core::models::enforcement::{FATIGUE_EVENT_REPUTATION_HIT, SUSPENSION_LIFETIME};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::app::GameContext;
use freight_fate::states::base::{Menu, MenuItem};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    DRIVE_PHASE_DELIVERY, MICROSLEEP_FORCE_STOP_MISSES, PURSUIT_RUN_S, SPEEDING_LEEWAY_MPH,
};
use freight_fate::states::driving_engine_brake::JAKE_ZONE_FINES;
use freight_fate::states::driving_rest_states::{
    major_offense_text, EnforcementStopState, FelonyStopState, TrafficStopState,
};
use freight_fate::states::driving_updates::pending::EnforcementStopParams;

// -- rigging -------------------------------------------------------------------------

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// `_driving(app, name="Jerry")`: Buffalo to Rochester, empty road.
fn a_drive(app: &mut TestApp, name: &str) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in(name, "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester is supported");
    let mut job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    // `quiet_trip(driving)`: an empty road and a pinned sky. An unseeded trip
    // draws fresh weather every run, and an ice day quietly caps the speeds
    // the speeding cases below drive at.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

/// `[item.text for item in state.build_items()]`.
fn build_labels<M: Menu>(state: &mut M, ctx: &mut GameContext) -> Vec<String> {
    let items: Vec<MenuItem<M>> = state.build_items(ctx);
    items.iter().map(|item| item.text(state, ctx)).collect()
}

/// A plain speeding stop, with Python's defaults for everything the keyword
/// arguments left out.
fn a_traffic_stop(
    app: &mut TestApp,
    drive: &mut DrivingState,
    signaled: bool,
    over: f64,
    limit: f64,
) -> TrafficStopState {
    TrafficStopState::new(
        &mut app.ctx,
        drive,
        signaled,
        over,
        limit,
        false, // clean_stop
        false, // warned
        false, // construction_zone
    )
}

/// Every line said so far, both channels, in submission order.
fn spoken(app: &TestApp) -> Vec<String> {
    app.speech().lines()
}

fn said(app: &TestApp) -> String {
    spoken(app).join(" ")
}

/// Frames of the stop with the truck held at `mph`, braking or not. The
/// trip is not stepped, so the miles stay put and the stop is judged on
/// conduct alone; `advance_mi` moves the truck on by hand.
fn roll_stop(app: &mut TestApp, drive: &mut DrivingState, mph: f64, seconds: f64, braking: bool) {
    let dt = 1.0 / 60.0;
    let frames = (seconds / dt).ceil() as i32;
    for _ in 0..frames {
        drive.trip.truck.velocity_mps = mph * 0.44704;
        drive.update_pull_over(&mut app.ctx, dt, braking);
        if drive.pull_over.is_none() {
            break;
        }
    }
}

// -- the lifetime line ----------------------------------------------------------------

#[test]
fn test_the_lifetime_line_states_the_facts_and_the_way_forward() {
    let mut app = TestApp::new();
    let mut profile = Profile::named("Jerry");
    profile.driving_record.record_major_offense(10.0);
    profile.driving_record.record_major_offense(20.0);
    app.ctx.profile = Some(profile);

    let text = major_offense_text(&app.ctx, SUSPENSION_LIFETIME, 20.0);

    assert!(text.contains("for life"), "{text}");
    assert!(text.contains("start a new career"), "{text}");
    assert!(text.contains("keeps every dollar"), "{text}");
    // No blame language, and no promise of options that do not exist.
    let lower = text.to_lowercase();
    for scold in ["you should have", "your fault", "stupid", "punish"] {
        assert!(!lower.contains(scold), "{scold:?} in {text}");
    }
}

// -- the road: what the stops now write -------------------------------------------------

#[test]
fn test_running_from_the_stop_writes_a_major_offense_on_the_career() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");

    FelonyStopState::new(&mut app.ctx, &mut drive);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.major_count(), 1);
    assert!(p.driving_record.suspended(p.game_hours));
}

#[test]
fn test_a_second_pursuit_ends_this_career_driving_for_good() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .record_major_offense(0.0); // Jerry's first one
    app.clear_speech();

    let mut state = FelonyStopState::new(&mut app.ctx, &mut drive);
    state.announce_entry(&mut app.ctx);

    assert!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .lifetime_disqualified
    );
    let said = said(&app);
    assert!(said.contains("for life"), "{said}");
    assert!(said.contains("start a new career"), "{said}");
}

#[test]
fn test_a_stop_that_suspends_the_cdl_does_not_send_you_back_out_driving() {
    // The stop menu used to offer "Pull back onto the highway" no matter what
    // had just happened -- so a driver whose license had been pulled seconds
    // earlier was invited to drive on, with no way to end the run from the
    // shoulder. A suspended driver is done for now; the run ends here.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let hours = app.ctx.profile.as_ref().expect("a career").game_hours;
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .record_serious_violation(hours); // one already on file
    drive.speeding_tickets = 1;

    let mut stop = a_traffic_stop(&mut app, &mut drive, false, 22.0, 65.0);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.driving_record.suspended(p.game_hours));
    let labels = build_labels(&mut stop, &mut app.ctx);
    assert!(
        !labels.iter().any(|label| label.contains("highway")),
        "{labels:?}"
    );
    assert!(
        labels
            .iter()
            .any(|label| label.to_lowercase().contains("terminal")),
        "{labels:?}"
    );
}

#[test]
fn test_an_ordinary_ticket_still_pulls_back_onto_the_highway() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.speeding_tickets = 1;

    let mut stop = a_traffic_stop(&mut app, &mut drive, true, 11.0, 65.0);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(!p.driving_record.suspended(p.game_hours));
    let labels = build_labels(&mut stop, &mut app.ctx);
    assert!(
        labels.iter().any(|label| label.contains("highway")),
        "{labels:?}"
    );
}

#[test]
fn test_the_suspended_stop_says_the_run_is_over_and_why() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let hours = app.ctx.profile.as_ref().expect("a career").game_hours;
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .record_serious_violation(hours);
    drive.speeding_tickets = 1;
    app.clear_speech();

    let mut stop = a_traffic_stop(&mut app, &mut drive, false, 22.0, 65.0);
    stop.announce_entry(&mut app.ctx);

    let said = said(&app);
    assert!(said.contains("CDL is suspended"), "{said}");
    assert!(said.contains("off the dispatch board"), "{said}");
    assert!(said.contains("load"), "{said}"); // what happens to the freight is stated
}

#[test]
fn test_an_enforcement_stop_that_suspends_also_ends_the_run() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let hours = app.ctx.profile.as_ref().expect("a career").game_hours;
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .record_serious_violation(hours);

    let mut stop = EnforcementStopState::new(
        &mut app.ctx,
        &mut drive,
        EnforcementStopParams {
            title: "Enforcement stop".to_string(),
            summary: "Unsafe equipment.".to_string(),
            fine: 900.0,
            reputation_hit: 5.0,
            signaled: true,
            return_message: "Back on the highway.".to_string(),
            out_of_service: false,
            warned: true, // a serious violation: this is the second
            construction_zone: false,
            inspection_on_stop: false,
            inspection_level: None,
        },
    );

    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.driving_record.suspended(p.game_hours));
    let labels = build_labels(&mut stop, &mut app.ctx);
    assert!(
        !labels.iter().any(|label| label.contains("highway")),
        "{labels:?}"
    );
}

#[test]
fn test_a_serious_speeding_ticket_moves_the_ladder_and_says_so() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.speeding_tickets = 1; // past the first-stop warning

    a_traffic_stop(&mut app, &mut drive, false, 22.0, 65.0);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.serious_in_window(p.game_hours), 1);
    let last = drive
        .record_events
        .last()
        .unwrap_or_else(|| panic!("the stop wrote nothing to the record log"));
    assert!(last.contains("serious violation"), "{last}");
}

#[test]
fn test_a_mild_speeding_ticket_is_money_only() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    app.seasoned_driver();
    drive.speeding_tickets = 1;

    a_traffic_stop(&mut app, &mut drive, true, 11.0, 65.0);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.serious_in_window(p.game_hours), 0);
    assert_eq!(p.driving_record.citations, 1);
    assert!(drive.record_events.is_empty(), "{:?}", drive.record_events);
}

#[test]
fn test_a_new_driver_gets_a_warning_for_mild_speeding() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.speeding_tickets = 1;
    let money = app.ctx.profile.as_ref().expect("a career").money();

    let stop = a_traffic_stop(&mut app, &mut drive, true, 11.0, 65.0);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.citations, 0);
    assert_eq!(p.money(), money);
    assert!(
        stop.outcome_text().contains("warning"),
        "{}",
        stop.outcome_text()
    );
}

#[test]
fn test_a_new_driver_is_still_cited_for_serious_speeding() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.speeding_tickets = 1;

    a_traffic_stop(&mut app, &mut drive, true, 16.0, 65.0);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.citations, 1);
    assert_eq!(p.driving_record.serious_in_window(p.game_hours), 1);
}

#[test]
fn test_speeding_nobody_saw_never_touches_the_licence() {
    // The old silent settlement strike deliberately never moved the ladder.
    // The strike is gone, and the guarantee it needed is now structural
    // rather than deliberate: with nobody watching, speeding produces no
    // citation to put on the record in the first place. Nothing may reach the
    // license file without a trooper having been there.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.trip.posts = Vec::new(); // empty road
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.enforcement_prev_mi = drive.trip.position_mi;
    let (limit, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
    drive.trip.truck.velocity_mps = (limit + SPEEDING_LEEWAY_MPH + 30.0) * MPS_PER_MPH;

    for _ in 0..40 {
        drive.trip.position_mi += 0.05;
        drive.update_enforcement_watch(&mut app.ctx, 0.2);
        drive.update_speeding(&mut app.ctx, 0.2, false);
    }

    assert_eq!(drive.speeding_tickets, 0);
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.citations, 0);
    assert_eq!(p.driving_record.serious_in_window(p.game_hours), 0);
    assert!(!p.driving_record.suspended(p.game_hours));
}

#[test]
fn test_debug_hours_mode_freezes_the_ladder() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    app.ctx.settings.hos_mode = "debug_off".to_string();

    FelonyStopState::new(&mut app.ctx, &mut drive);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.major_count(), 0);
    assert!(!p.driving_record.suspended(p.game_hours));
}

#[test]
fn test_running_off_the_road_asleep_costs_reputation_and_is_spoken() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let before = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .career
        .reputation;
    app.clear_speech();
    drive.microsleep_misses = 0;

    drive.microsleep_drift_off_road(&mut app.ctx);

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.fatigue_events, 1);
    assert!(approx(
        p.career.reputation,
        before - FATIGUE_EVENT_REPUTATION_HIT
    ));
    assert!(
        spoken(&app).iter().any(|line| line.contains("reputation")),
        "{:#?}",
        spoken(&app)
    );
}

#[test]
fn test_terse_speech_still_hears_every_consequence() {
    // Terse drops description, never anything with money or standing on it.
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_drive(&mut app, "Jerry");
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .record_fatigue_event(0.0); // so the next one is serious
    app.clear_speech();
    drive.microsleep_misses = 0;

    drive.microsleep_drift_off_road(&mut app.ctx);

    let said = said(&app);
    assert!(said.contains("serious violation"), "{said}");
    // The second one lands the 60-day suspension.
    assert!(said.contains("suspended"), "{said}");
}

#[test]
fn test_repeat_fatigue_events_speak_the_real_count() {
    // Every occurrence past the first is a serious violation, and the line
    // must say how many times it has actually happened -- not freeze at
    // "twice now" once the driver runs off the road asleep a third or fourth
    // time.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    app.clear_speech();

    for _ in 0..4 {
        drive.microsleep_misses = 0;
        drive.microsleep_drift_off_road(&mut app.ctx);
    }

    let said = said(&app);
    for phrase in [
        "That is twice now that you have run off the road asleep.",
        "That is three times now that you have run off the road asleep.",
        "That is four times now that you have run off the road asleep.",
    ] {
        assert!(said.contains(phrase), "{phrase:?} missing from {said}");
    }
}

// -- running is conduct, never a key ------------------------------------------------

/// Sixty on the highway with the lights behind you, never a brake: past the
/// final warning that is running, and troopers end it as a felony.
#[test]
fn test_holding_speed_past_the_final_warning_is_running() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.begin_pull_over(&mut app.ctx, 65.0);
    drive.pull_over_grace_s = 0.0;
    app.clear_speech();

    roll_stop(&mut app, &mut drive, 60.0, 60.0, false);
    app.ctx.run_deferred();

    let lines = spoken(&app);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Slow down now or you are running from the police")),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("felony")),
        "{lines:#?}"
    );
    assert!(drive.pull_over.is_none());
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<FelonyStopState>()));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.driving_record.major_count(), 1);
    assert!(p.driving_record.suspended(p.game_hours));
}

/// The final warning alone is not the felony: the truck has to keep it up
/// for the required seconds after it, and the warning speaks first.
#[test]
fn test_the_final_warning_comes_before_the_pursuit_counts() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.begin_pull_over(&mut app.ctx, 65.0);
    drive.pull_over_grace_s = 0.0;
    app.clear_speech();

    // Long enough to drain the tracker and speak the final warning, not
    // long enough to run.
    roll_stop(&mut app, &mut drive, 60.0, 10.0, false);
    assert_eq!(drive.pull_over_warning_level, 2);
    assert!(drive.pull_over.is_some());
    assert!(drive.pull_over_run_s > 0.0 && drive.pull_over_run_s < PURSUIT_RUN_S);
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .major_count(),
        0
    );
}

/// A brake after the final warning is a driver trying to stop, not running:
/// troopers force the stop, a serious violation, and never a felony.
#[test]
fn test_touching_the_brake_after_the_final_warning_is_a_forced_stop_not_a_felony() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let start = drive.trip.total_miles() / 2.0;
    drive.trip.position_mi = start;
    drive.begin_pull_over(&mut app.ctx, 65.0);
    drive.pull_over_grace_s = 0.0;

    roll_stop(&mut app, &mut drive, 60.0, 10.0, false);
    assert_eq!(drive.pull_over_warning_level, 2);
    // Two miles on and braking, still rolling: the forced stop, in its own time.
    drive.trip.position_mi = start + 2.5;
    roll_stop(&mut app, &mut drive, 40.0, 20.0, true);
    app.ctx.run_deferred();

    assert!(drive.pull_over.is_none());
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<EnforcementStopState>()));
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .major_count(),
        0
    );
}

/// A slow roll under the floor is not running either, however long it
/// goes on.
#[test]
fn test_a_slow_roll_never_reaches_a_felony() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let start = drive.trip.total_miles() / 2.0;
    drive.trip.position_mi = start;
    drive.begin_pull_over(&mut app.ctx, 65.0);
    drive.pull_over_grace_s = 0.0;

    roll_stop(&mut app, &mut drive, 20.0, 10.0, false);
    drive.trip.position_mi = start + 2.5;
    roll_stop(&mut app, &mut drive, 20.0, 60.0, false);
    app.ctx.run_deferred();

    assert!(drive.pull_over.is_none());
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<EnforcementStopState>()));
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .major_count(),
        0
    );
}

/// Coasting down past the final warning is not running either: the run
/// count starts at highway speed, but a truck that lets its speed fall away
/// unbraked is handed to the forced stop, not left rolling for miles. The
/// adversarial scale-bypass scenario found the first version rolling on
/// with a stale run count holding the forced stop off.
#[test]
fn test_coasting_down_after_the_final_warning_is_a_forced_stop() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let start = drive.trip.total_miles() / 2.0;
    drive.trip.position_mi = start;
    drive.begin_pull_over(&mut app.ctx, 65.0);
    drive.pull_over_grace_s = 0.0;

    roll_stop(&mut app, &mut drive, 60.0, 10.0, false);
    assert_eq!(drive.pull_over_warning_level, 2);
    assert!(drive.pull_over_run_s > 0.0);
    // Two miles on, speed falling a mile an hour a second, never a brake.
    drive.trip.position_mi = start + 2.5;
    let dt = 1.0 / 60.0;
    let mut mph: f64 = 60.0;
    for _ in 0..(40.0 / dt) as i32 {
        mph = (mph - dt).max(8.0);
        drive.trip.truck.velocity_mps = mph * 0.44704;
        drive.update_pull_over(&mut app.ctx, dt, false);
        if drive.pull_over.is_none() {
            break;
        }
    }
    app.ctx.run_deferred();

    assert!(drive.pull_over.is_none(), "the stop was never forced");
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<EnforcementStopState>()));
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .major_count(),
        0
    );
}

/// A lifetime disqualification gets its own warning and twice the run.
#[test]
fn test_the_second_pursuit_takes_twice_as_long_to_earn() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .record_major_offense(0.0);
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.begin_pull_over(&mut app.ctx, 65.0);
    drive.pull_over_grace_s = 0.0;
    app.clear_speech();

    roll_stop(&mut app, &mut drive, 60.0, 10.0, false);
    assert_eq!(drive.pull_over_warning_level, 2);
    assert!(
        spoken(&app).iter().any(|line| line.contains("for life")),
        "{:#?}",
        spoken(&app)
    );
    // One run's worth is not enough the second time.
    roll_stop(&mut app, &mut drive, 60.0, PURSUIT_RUN_S + 1.0, false);
    assert!(drive.pull_over.is_some());
    assert!(
        !app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .lifetime_disqualified
    );

    roll_stop(&mut app, &mut drive, 60.0, PURSUIT_RUN_S + 1.0, false);
    assert!(drive.pull_over.is_none());
    assert!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .lifetime_disqualified
    );
}

// -- exploits the adversarial harness found ----------------------------------------------

#[test]
fn test_reloading_mid_stop_does_not_cancel_the_stop() {
    // Save-scumming out of a pull-over would make every suspension optional.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.begin_pull_over(&mut app.ctx, 65.0);
    drive.pull_over_warning_level = 1;
    drive.pull_over_compliance = 0.3;
    assert!(drive.pull_over.is_some());

    let snapshot = drive.snapshot(&app.ctx);
    let restored = DrivingState::from_snapshot(&mut app.ctx, &snapshot)
        .expect("the snapshot the drive just wrote reloads");

    assert_eq!(restored.pull_over, drive.pull_over);
    assert_eq!(restored.pull_over_warning_level, 1);
    assert!(approx(restored.pull_over_compliance, 0.3));
    assert!(approx(restored.pull_over_limit, drive.pull_over_limit));
}

#[test]
fn test_a_paid_stop_is_not_charged_again_on_the_next_resume() {
    // The other half of "a stop survives a reload": it has to end, too.
    //
    // The lights are written into the save before a word of the stop is
    // spoken, so a crash cannot erase it. Nothing wrote the save back once
    // the fine was paid, so every resume found a cruiser still sitting behind
    // a parked truck and charged for the same stop again -- at the repeat
    // rate, so it cost more each time. A tester lost his career's money to
    // four of them in a minute (log, 2026-08-10).
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.begin_enforcement_pull_over(
        &mut app.ctx,
        "weigh_station_bypass",
        "Weigh station bypass stop",
        "Scale officers saw you blow past the scale.",
        750.0,
        3.0,
        "Back on the highway.",
        "Lights and siren behind you.",
    );
    assert!(app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .is_some());
    let money_before = app.ctx.profile.as_ref().expect("a career").money();

    // Brake to a stop: the roadside stop opens and the fine is paid.
    drive.trip.truck.velocity_mps = 0.0;
    drive.pull_over_grace_s = 0.0;
    drive.update_pull_over(&mut app.ctx, 0.1, false);
    app.ctx.run_deferred();
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<EnforcementStopState>()));
    let paid = money_before - app.ctx.profile.as_ref().expect("a career").money();
    assert!(paid > 0.0);
    app.ctx.pop_state();
    app.ctx.run_deferred();

    // Quit to the title and resume. The truck is parked and the stop is
    // settled, so nothing about it may happen a second time.
    let saved = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .clone()
        .expect("the settled stop was written back to the save");
    let mut resumed =
        DrivingState::from_snapshot(&mut app.ctx, &saved).expect("the settled snapshot reloads");
    assert!(resumed.pull_over.is_none());
    resumed.update_pull_over(&mut app.ctx, 0.1, false);
    app.ctx.run_deferred();
    assert!(!app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<EnforcementStopState>()));
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - paid
    ));
}

#[test]
fn test_toggling_the_jake_cannot_farm_warnings_forever() {
    // One grace window per town, not a renewable exemption.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    app.seasoned_driver();
    // First engagement: warned, then complies before the timer runs out.
    drive
        .jake_zone_grace_used
        .insert("buffalo_ny_us".to_string());
    drive.jake_violation_deadline_s = None;
    drive.jake_citation_latched = false;
    let money_before = app.ctx.profile.as_ref().expect("a career").money();

    drive.fine_engine_braking(&mut app.ctx, "buffalo_ny_us");

    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(approx(p.money(), money_before - JAKE_ZONE_FINES[0]));
    // The citation is on the record even though it is not a serious one.
    assert_eq!(p.driving_record.citations, 1);
    assert_eq!(p.driving_record.serious_in_window(p.game_hours), 0);
}

#[test]
fn test_the_fatigue_out_of_service_actually_holds_the_truck() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.trip.truck.velocity_mps = 27.0;
    drive.trip.truck.throttle = 1.0;
    drive.microsleep_misses = MICROSLEEP_FORCE_STOP_MISSES - 1;
    app.clear_speech();

    drive.microsleep_drift_off_road(&mut app.ctx);

    assert_eq!(drive.trip.truck.velocity_mps, 0.0);
    assert!(drive.trip.truck.parking_brake); // not a one-frame brake tap
    assert!(
        spoken(&app)
            .iter()
            .any(|line| line.contains("Out of service for fatigue")),
        "{:#?}",
        spoken(&app)
    );
    // The 70-hour limit does not reset on ten hours off: only the driving
    // and duty clocks do, and the line says so.
    assert!(
        spoken(&app)
            .iter()
            .any(|line| line.contains("Driving and duty clocks reset, and the delivery deadline")),
        "{:#?}",
        spoken(&app)
    );
}

#[test]
fn test_a_settled_stop_is_read_back_as_history_not_as_a_fresh_charge() {
    // Tester Darren, I-75, 2026-08-18: the same 1,200 dollar work-zone
    // citation spoken twice, three seconds apart, word for word.
    //
    // `resolve` charges the fine once, in the constructor, and that is not in
    // question -- but the spoken line was identical every time, so a driver
    // working by ear could not tell a repeat from a second ticket. Not
    // silenced: re-reading the stop is the only way back to the detail.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    app.seasoned_driver();
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    app.clear_speech();

    let mut stop = EnforcementStopState::new(
        &mut app.ctx,
        &mut drive,
        EnforcementStopParams {
            title: "Work zone stop".to_string(),
            summary: "A trooper watched you close right up on the vehicle ahead.".to_string(),
            fine: 1200.0,
            reputation_hit: 2.0,
            signaled: true,
            return_message: "Pull back onto the highway.".to_string(),
            out_of_service: false,
            warned: false,
            construction_zone: false,
            inspection_on_stop: false,
            inspection_level: None,
        },
    );
    let charged = money_before - app.ctx.profile.as_ref().expect("a career").money();
    assert!(charged > 0.0);

    stop.announce_entry(&mut app.ctx);
    let first = app
        .main_lines()
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("the stop said nothing"));
    assert!(first.starts_with("You stop on the shoulder"), "{first}");

    // Told again: same detail, plainly already settled.
    stop.announce_entry(&mut app.ctx);
    let second = app
        .main_lines()
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("the second telling said nothing"));
    assert_ne!(second, first);
    assert!(second.contains("already settled"), "{second}");
    assert!(!second.contains("You stop on the shoulder"), "{second}");
    // And the detail is still all there.
    assert!(second.contains(&stop.summary), "{second}");

    // Saying it twice never charges twice.
    assert!(approx(
        money_before - app.ctx.profile.as_ref().expect("a career").money(),
        charged
    ));
}
