//! The interactive pull-over: being lit up, the compliance tracker, the
//! roadside ticket or warning, the scale bypass, and running from a stop.
//!
//! These are the `tests/test_troopers.py` cases that drive a real
//! `DrivingState` and the screens it pushes. They spent the port as
//! `#[ignore]`d stubs in `crates/ff-core/tests/sim_troopers.rs`, where they
//! could never run: `ff-core` cannot depend on the game crate, so
//! `TrafficStopState`, `EnforcementStopState` and `FelonyStopState` are
//! invisible from there. The post model and the CB heads-up that sat beside
//! them are genuine `Trip` work and stay in `ff-core`.
//!
//! The cue ladder half of the same Python file -- what the marked unit sounds
//! like, when a post can observe you at all -- is already live in
//! `states_driving_enforcement.rs`; this file is the encounter that follows.
//!
//! # What replaced the monkeypatches
//!
//! | Python | here |
//! |---|---|
//! | `monkeypatch.setattr(ctx, "say"/"say_event", stub)` | the capture behind `ctx.speech`, one rung lower, so the ladder and the pacer are in the picture |
//! | `monkeypatch.setattr(ctx.audio, "play", stub)` | [`TestApp::record_audio`], which records instead of muting |
//! | `monkeypatch.setattr(ctx.audio, "set_engine_rpm", stub)` | the same recorder's `engine_rpm` log |
//! | `monkeypatch.setattr(driving_rest_states, "PULL_OVER_CLEAN_STOP_WARN_CHANCE", 0.0 / 1.0)` | [`a_waiver_mile`] searches the real draw for a mile where this trip really does roll the wanted side |
//! | `monkeypatch.setattr(pygame.key, "get_pressed", ...)` | `ctx.input.press(Key::X, Mods::SHIFT)`, the real held key |

use ff_core::models::enforcement::{
    citation_fine, speeding_citation_fine, UNSAFE_DAMAGE_FINE, WEIGH_STATION_BYPASS_FINE,
};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::pyfmt::fmt_f;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::enforcement_observe::{OBSERVE_HOLD_MI, WHAT_EQUIPMENT};
use ff_core::sim::enforcement_posts::{
    method_by_kind, EnforcementPost, KIND_CMV, KIND_FIXED_SCALE, KIND_MEDIAN, KIND_SCALE_APRON,
};
use ff_core::sim::roadside_inspection::InspectionLevel;
use ff_core::sim::trip_models::{RoadStop, Zone};
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::{AudioLog, TestApp};
use freight_fate::app::GameContext;
use freight_fate::states::base::{InputEvent, Key, Menu, State};
use freight_fate::states::city::CityMenuState;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    DRIVE_PHASE_DELIVERY, INSPECTION_MIN, PULL_OVER_CLEAN_STOP_WARN_CHANCE,
    PULL_OVER_FULL_COMPLIANCE, PULL_OVER_LIGHTS, PURSUIT_RUN_S,
};
use freight_fate::states::driving_enforcement::SCALE_REMINDER_REAL_LEAD_S;
use freight_fate::states::driving_rest_states::{
    EnforcementStopState, FelonyStopState, TrafficStopState,
};
use freight_fate::states::driving_updates::pending::EnforcementStopParams;

// -- rigging -------------------------------------------------------------------------

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn mph_to_mps(mph: f64) -> f64 {
    mph * MPS_PER_MPH
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// `_driving(app, patrol_intensity=...)`: Buffalo to Rochester, with one post
/// that watches the whole route and has already been heard, so a case can put
/// the truck anywhere and ask what it sees. `None` leaves the road empty.
fn a_drive(app: &mut TestApp, patrol_intensity: Option<f64>) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Leadfoot", "Buffalo");
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
    // these cases clock the truck at.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    let total = drive.trip.total_miles();
    drive.trip.posts = match patrol_intensity {
        Some(notice) => vec![always_observing_post(
            total,
            KIND_MEDIAN,
            total + 1.0,
            notice,
        )],
        None => Vec::new(),
    };
    drive
}

/// `enforcement_helpers.always_observing_post`: a staffed, already-heard post
/// watching `reach_mi` up to `at_mi`.
fn always_observing_post(at_mi: f64, kind: &str, reach_mi: f64, notice: f64) -> EnforcementPost {
    EnforcementPost {
        method: method_by_kind(kind).to_string(),
        reach_mi,
        facing: "both".to_string(),
        staffed: true,
        notice,
        announced: true,
        ..EnforcementPost::new(at_mi, kind)
    }
}

/// `enforcement_helpers.open_scale_post`: an open weigh station standing
/// behind `stop`.
fn open_scale_post(stop: &RoadStop) -> EnforcementPost {
    EnforcementPost {
        method: method_by_kind(KIND_FIXED_SCALE).to_string(),
        reach_mi: 0.5,
        facing: "with_traffic".to_string(),
        staffed: true,
        anchor: stop.key(),
        announced: true,
        ..EnforcementPost::new(stop.at_mi, KIND_FIXED_SCALE)
    }
}

fn a_scale(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Ontario Scale", at_mi, "weigh_station");
    stop.actions = vec!["inspect".to_string()];
    stop.parking = "none".to_string();
    stop
}

/// `_speed_for(d, over)` / `enforcement_helpers.watch_speed`: speed past a
/// watching post far enough for it to read the speed, and hand back the
/// posted limit.
///
/// Observation is distance-quantised, not held on the wall clock, so the
/// setup is "the truck has run this far over the limit", not "this many real
/// seconds have passed".
fn speed_for(drive: &mut DrivingState, app: &mut TestApp, over: f64) -> f64 {
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.enforcement_prev_mi = drive.trip.position_mi;
    let (limit, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
    drive.trip.truck.velocity_mps = mph_to_mps(limit + over);
    drive.over_limit_mi = OBSERVE_HOLD_MI * 2.0;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    limit
}

/// `_past_grace(d)`: skip the spoken-instruction grace so a case can judge
/// the tracker. Nothing about a stop is judged until the trigger message has
/// had time to be read out.
fn past_grace(drive: &mut DrivingState) {
    drive.pull_over_grace_s = 0.0;
}

/// `_pave_construction(d)`: real roadwork over the whole route, so any stop
/// happens inside it.
fn pave_construction(drive: &mut DrivingState) {
    let total = drive.trip.total_miles();
    drive.trip.zones = vec![Zone::new(0.0, total, 45.0, "construction")];
}

/// A mile where this trip's own clean-stop waiver draw lands on the side the
/// case needs.
///
/// Python monkeypatched `PULL_OVER_CLEAN_STOP_WARN_CHANCE` to 0.0 to take the
/// leniency off the table and to 1.0 to force it. The roll is a named,
/// position-quantised draw on `{trip_seed}:police:waiver:{mile}` compared
/// against that constant, so the honest arrangement is to bring the truck to
/// a stop on a mile where this seed really does roll the wanted side -- and to
/// fail loudly if no such mile exists rather than pass on the wrong one.
fn a_waiver_mile(trip_seed: i64, waived: bool, from_mi: f64) -> f64 {
    let mut mi = from_mi;
    for _ in 0..400 {
        let key = format!("{trip_seed}:police:waiver:{}", fmt_f(mi, 1));
        let roll = PyRandom::new_from_str(&key).random();
        if (roll < PULL_OVER_CLEAN_STOP_WARN_CHANCE) == waived {
            return mi;
        }
        mi += 0.1;
    }
    panic!(
        "no mile in the 40 past {from_mi} rolls the waiver {} on seed {trip_seed}",
        if waived { "on" } else { "off" }
    );
}

/// Whether the active state is `T`.
fn top_is<T: 'static>(app: &TestApp) -> bool {
    app.ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<T>())
}

/// Run `f` on the active state as `T`, with the context.
fn with_top<T: 'static, R>(app: &mut TestApp, f: impl FnOnce(&mut T, &mut GameContext) -> R) -> R {
    let state = app.ctx.state().expect("a state on the stack");
    let mut borrowed = state.borrow_mut();
    let typed = borrowed
        .as_any_mut()
        .downcast_mut::<T>()
        .expect("the active state has the expected type");
    let out = f(typed, &mut app.ctx);
    drop(borrowed);
    app.ctx.run_deferred();
    out
}

/// Every sound key the audio backend was asked for.
fn played(log: &AudioLog) -> Vec<String> {
    log.borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect()
}

/// Every line said so far, both channels, in submission order.
fn spoken(app: &TestApp) -> Vec<String> {
    app.speech().lines()
}

fn said(app: &TestApp) -> String {
    spoken(app).join(" ")
}

// -- catching the speeder -----------------------------------------------------------

#[test]
fn test_metric_pull_over_announcement_uses_metric_units() {
    let mut app = TestApp::new();
    app.ctx.settings.imperial_units = false;
    let mut drive = a_drive(&mut app, Some(1.0));
    app.clear_speech();
    speed_for(&mut drive, &mut app, 20.0);

    let lines = app.event_lines();
    let lit_up = lines
        .iter()
        .find(|line| line.contains("Lights and siren behind you"))
        .unwrap_or_else(|| panic!("nothing said the trooper lit the truck up: {lines:#?}"));
    assert!(lit_up.contains("kilometers per hour"), "{lit_up}");
    assert!(!lit_up.contains("miles per hour"), "{lit_up}");
}

// -- the stop: tickets, warnings, evasion --------------------------------------------

#[test]
fn test_stopping_issues_an_immediate_ticket() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0); // well over -> a ticket, not a warning
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let rep_before = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .career
        .reputation;

    drive.trip.truck.velocity_mps = 0.0; // brake to a full stop on the shoulder
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<TrafficStopState>(&app));
    assert_eq!(drive.speeding_tickets, 1);
    let (over, zone) =
        with_top::<TrafficStopState, _>(&mut app, |stop, _| (stop.over, stop.construction_zone));
    let expected = speeding_citation_fine(over, 0, zone);
    assert!(approx(drive.ticket_fines_paid, expected));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(approx(p.money(), money_before - expected));
    assert!(p.career.reputation < rep_before);
    // 25 over is a serious traffic violation, and the record says so.
    assert_eq!(p.driving_record.serious_in_window(p.game_hours), 1);
}

#[test]
fn test_stopping_drops_engine_audio_to_idle() {
    // The engine keeps running through a traffic stop -- pulled over on the
    // shoulder, not parked for the night -- but the engine loop must not keep
    // sounding like it is still under highway load once the truck is actually
    // at rest (tester report, build 1.9.0.dev0).
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    let log = app.record_audio();
    drive.trip.truck.engine_on = true;
    // Whatever the engine was doing while braking to the stop -- still
    // revving, still carrying some throttle.
    drive.trip.truck.rpm = 1800.0;
    drive.trip.truck.throttle = 0.7;
    speed_for(&mut drive, &mut app, 25.0);
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<TrafficStopState>(&app));
    // The engine stays on: this is a traffic stop, not an overnight park.
    assert!(drive.trip.truck.engine_on);
    // But it reads as idle the instant the stop opens.
    let idle_rpm = drive.trip.truck.specs.idle_rpm;
    assert!(approx(drive.trip.truck.rpm, idle_rpm));
    assert_eq!(drive.trip.truck.throttle, 0.0);
    let samples = log.borrow().engine_rpm.clone();
    let last = samples
        .last()
        .unwrap_or_else(|| panic!("the engine loop was never told anything: {samples:?}"));
    assert!(approx(last.0, idle_rpm), "{last:?}");
    assert_eq!(last.1, 0.0);
}

#[test]
fn test_metric_traffic_stop_outcome_uses_metric_units() {
    let mut app = TestApp::new();
    app.ctx.settings.imperial_units = false;
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0);
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<TrafficStopState>(&app));
    let text = with_top::<TrafficStopState, _>(&mut app, |stop, _| stop.outcome_text().to_string());
    assert!(text.contains("kilometers per hour"), "{text}");
    assert!(!text.contains("miles per hour"), "{text}");
}

#[test]
fn test_first_marginal_stop_is_a_warning() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 12.0); // only marginally over, first stop
    let money_before = app.ctx.profile.as_ref().expect("a career").money();

    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert_eq!(drive.speeding_tickets, 0); // warning, no charge
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before
    ));
}

#[test]
fn test_accelerating_away_past_the_final_warning_is_running() {
    // Accelerating away with the lights behind you, never a brake, is
    // running -- but only once the final warning has said so and the truck
    // has kept it up for the required seconds after it. Reaching a felony
    // used to be possible in about five seconds, while the trigger message
    // was still being spoken; the warning comes first now, every time.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    let limit = speed_for(&mut drive, &mut app, 25.0);
    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
    past_grace(&mut drive);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let rep_before = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .career
        .reputation;

    let base = mph_to_mps(limit + 25.0);
    let mut seconds = 0;
    for i in 0..40 {
        drive.trip.truck.velocity_mps = base + mph_to_mps(f64::from(i + 1));
        drive.update_pull_over(&mut app.ctx, 1.0, false);
        seconds += 1;
        if drive.pull_over.is_none() {
            break;
        }
    }
    app.ctx.run_deferred();

    assert!(top_is::<FelonyStopState>(&app));
    assert!(!top_is::<EnforcementStopState>(&app));
    // The final warning spoke, and the run was counted after it.
    assert!(
        spoken(&app)
            .iter()
            .any(|line| line.contains("Slow down now or you are running from the police")),
        "{:#?}",
        spoken(&app)
    );
    assert!(seconds as f64 > PURSUIT_RUN_S, "{seconds}");
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.money() < money_before);
    assert!(p.career.reputation < rep_before);
    assert_eq!(p.driving_record.major_count(), 1);
}

#[test]
fn test_a_compliant_driver_is_never_charged_with_running() {
    // Steady speed, no signal, cruise engaged, realistic pacing: no felony.
    // This is the exact shape of a player who is listening to the instruction.
    // The old tracker convicted them at about 5.07 seconds.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    let limit = speed_for(&mut drive, &mut app, 25.0);
    assert!(drive.pull_over.is_some());
    // Lighting a driver up hands the wheel back rather than leaving an assist
    // holding a steady speed into the drain.
    assert_eq!(drive.cruise_mph, None);
    assert!(drive.trip.pull_over_active); // and the clock stops compressing

    let speed = mph_to_mps(limit + 25.0);
    for _ in 0..10 {
        drive.trip.truck.velocity_mps = speed; // dead steady, never signalled
        drive.update_pull_over(&mut app.ctx, 1.0, false);
    }
    app.ctx.run_deferred();

    assert!(!top_is::<FelonyStopState>(&app));
    assert_eq!(drive.failure_to_stop_count, 0);
}

#[test]
fn test_failure_to_stop_gives_staged_warnings() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    let limit = speed_for(&mut drive, &mut app, 25.0);
    past_grace(&mut drive);
    drive.trip.truck.velocity_mps = mph_to_mps(limit + 25.0);

    // The warnings run on real seconds, not trip miles: compression could
    // burn through two miles before the first could ever speak.
    for _ in 0..9 {
        drive.update_pull_over(&mut app.ctx, 1.0, false);
    }
    assert!(
        spoken(&app)
            .iter()
            .any(|line| line.contains("Failure-to-stop warning")),
        "{:#?}",
        spoken(&app)
    );
    for _ in 0..8 {
        drive.update_pull_over(&mut app.ctx, 1.0, false);
    }
    assert!(
        spoken(&app)
            .iter()
            .any(|line| line.contains("Final warning. Slow down now")),
        "{:#?}",
        spoken(&app)
    );
    assert_eq!(drive.failure_to_stop_count, 0);
}

#[test]
fn test_failure_to_stop_warning_acknowledges_signal() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    let limit = speed_for(&mut drive, &mut app, 25.0);
    drive.signal_pull_over(&mut app.ctx);
    past_grace(&mut drive);
    drive.trip.truck.velocity_mps = mph_to_mps(limit + 25.0);

    for _ in 0..9 {
        drive.update_pull_over(&mut app.ctx, 1.0, false);
    }

    assert!(
        said(&app).contains("Signaled, but still moving with lights behind you"),
        "{:#?}",
        spoken(&app)
    );
    assert_eq!(drive.failure_to_stop_count, 0);
}

#[test]
fn test_felony_stop_cancels_loaded_run_and_returns_to_terminal() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0);
    let snapshot = drive.snapshot(&app.ctx);
    app.ctx.profile.as_mut().expect("a career").active_trip = Some(snapshot);
    let damage_before = drive.trip.truck.damage_pct;
    let game_hours_before = app.ctx.profile.as_ref().expect("a career").game_hours;

    // Only a deliberate held opt-in starts a pursuit.
    drive.evade_pull_over(&mut app.ctx);
    app.ctx.run_deferred();

    assert!(top_is::<FelonyStopState>(&app));
    assert!(with_top::<FelonyStopState, _>(&mut app, |stop, _| stop.load_lost));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.active_trip.is_none());
    assert!(p.truck_damage_pct() > damage_before);
    assert!(p.game_hours > game_hours_before);

    with_top::<FelonyStopState, _>(&mut app, |stop, ctx| stop.go_back(ctx));
    assert!(top_is::<CityMenuState>(&app));
}

#[test]
fn test_felony_stop_does_not_claim_load_loss_for_empty_run() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    drive.job.bobtail = true;
    speed_for(&mut drive, &mut app, 25.0);

    drive.evade_pull_over(&mut app.ctx);
    app.ctx.run_deferred();

    assert!(top_is::<FelonyStopState>(&app));
    assert!(!with_top::<FelonyStopState, _>(&mut app, |stop, _| stop.load_lost));
}

#[test]
fn test_debug_off_mode_clears_active_pull_over_without_felony() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0);
    app.ctx.settings.hos_mode = "debug_off".to_string();

    drive.trip.position_mi = drive.pull_over_start_mi + 3.0;
    drive.trip.truck.velocity_mps = mph_to_mps(65.0);
    drive.update_pull_over(&mut app.ctx, 1.0, false);

    assert!(drive.pull_over.is_none());
    assert_eq!(drive.failure_to_stop_count, 0);
}

// -- the weigh station ---------------------------------------------------------------

/// The scale and its open post, laid over an empty road with the truck a
/// tenth past the check-in.
fn blow_past_a_scale(drive: &mut DrivingState) -> RoadStop {
    let stop = a_scale(10.0);
    drive.trip.stops = vec![stop.clone()];
    let post = open_scale_post(&stop);
    drive.trip.posts.push(post);
    drive.trip.position_mi = 10.1;
    drive.trip.truck.velocity_mps = mph_to_mps(55.0);
    // Announced, reminded, and given the reminder's real seconds: a crossing
    // the driver was never warned about in time is not judged at all.
    let key = drive.weigh_station_key(&stop);
    drive.weigh_station_noticed.insert(key.clone());
    drive.weigh_station_reminder_key = key;
    drive.weigh_station_reminder_age_s = SCALE_REMINDER_REAL_LEAD_S;
    stop
}

#[test]
fn test_weigh_station_blow_past_starts_enforcement_stop() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    // A bypass is caught, not certain -- seed 1 lands under the 85 percent
    // catch chance for this exact scale key, so the stop is deterministic.
    drive.trip_seed = 1;
    blow_past_a_scale(&mut drive);

    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);

    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
    assert_eq!(drive.pull_over_kind, "weigh_station_bypass");

    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let rep_before = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .career
        .reputation;
    let citations_before = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .driving_record
        .citations;
    let minutes_before = drive.trip.game_minutes;
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<EnforcementStopState>(&app));
    let (zone, outcome) = with_top::<EnforcementStopState, _>(&mut app, |stop, _| {
        (stop.construction_zone, stop.outcome_text().to_string())
    });
    // A clean career on open road: the base amount, unscaled. Asked of the
    // helper rather than hardcoded, so a rebalance moves one number.
    let expected = citation_fine(WEIGH_STATION_BYPASS_FINE, 0, zone, None);
    assert!(approx(drive.ticket_fines_paid, expected));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(approx(p.money(), money_before - expected));
    assert!(p.career.reputation < rep_before);
    // Recorded on the driving record like any other citation.
    assert_eq!(p.driving_record.citations, citations_before + 1);
    // Dodging the check-in lane does not dodge the inspection: the trooper
    // runs it right there, on the same clock the scale itself would cost.
    assert!(approx(
        drive.trip.game_minutes,
        minutes_before + INSPECTION_MIN
    ));
    assert!(outcome.contains("full inspection"), "{outcome}");
}

#[test]
fn test_weigh_station_bypass_is_not_certain_and_stays_silent_when_missed() {
    // The scale house watches, but a unit still has to catch up to you. Seed
    // 11 rolls over the 85 percent catch chance for this exact scale key, so
    // the same crossing that seed 1 catches gets away clean -- silently, by
    // design. The event is still marked so the scale never re-rolls the miss.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip_seed = 11;
    let stop = blow_past_a_scale(&mut drive);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let citations_before = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .driving_record
        .citations;

    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);

    assert!(drive.pull_over.is_none());
    assert!(drive
        .enforcement_events
        .contains(&drive.weigh_station_key(&stop)));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(approx(p.money(), money_before));
    assert_eq!(p.driving_record.citations, citations_before);
}

#[test]
fn test_closed_scale_never_charges_a_bypass() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip_seed = 1;
    let stop = a_scale(10.0);
    drive.trip.stops = vec![stop.clone()];
    // A closed scale still carries an apron post -- state police may be
    // sitting on it -- but the silence-means-closed rule holds: no scale
    // bypass charge can ever come from it.
    drive.trip.posts.push(EnforcementPost {
        method: method_by_kind(KIND_SCALE_APRON).to_string(),
        reach_mi: 0.5,
        facing: "both".to_string(),
        staffed: true,
        anchor: stop.key(),
        announced: true,
        ..EnforcementPost::new(stop.at_mi, KIND_SCALE_APRON)
    });
    drive.trip.position_mi = 10.1;
    drive.trip.truck.velocity_mps = mph_to_mps(55.0);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();

    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);

    assert!(drive.pull_over.is_none());
    assert!(drive.enforcement_events.is_empty());
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before
    ));
}

#[test]
fn test_weigh_station_warning_is_spoken_before_bypass() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    let log = app.record_audio();
    let stop = a_scale(10.0);
    drive.trip.stops = vec![stop.clone()];
    drive.trip.posts.push(open_scale_post(&stop));
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(45.0);
    app.clear_speech();

    drive.check_weigh_station_enforcement(&mut app.ctx, 8.0);
    drive.check_weigh_station_enforcement(&mut app.ctx, 8.1);

    let notices: Vec<String> = spoken(&app)
        .into_iter()
        .filter(|line| line.contains("Open weigh station ahead"))
        .collect();
    assert_eq!(notices.len(), 1, "{notices:#?}");
    // The notice names no key: "Press T" at speed plans a sleep stop, which
    // is the instruction that used to march drivers into the bypass.
    assert!(
        notices[0].ends_with("All trucks must pull in."),
        "{}",
        notices[0]
    );
    assert!(!notices[0].contains("press T"), "{}", notices[0]);
    // Its own earcon, not the shared inspection cue (owner ruling,
    // 2026-08-14): testers could not tell the scale-ahead warning apart from
    // being looked at for something else.
    let keys = played(&log);
    assert_eq!(
        keys.iter()
            .filter(|key| *key == "events/weigh_station_warning")
            .count(),
        1,
        "{keys:#?}"
    );
    assert!(!keys.iter().any(|key| key == "events/inspection_warning"));
}

#[test]
fn test_debug_off_mode_bypasses_scale_blow_past() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    app.ctx.settings.hos_mode = "debug_off".to_string();
    blow_past_a_scale(&mut drive);

    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);

    assert!(drive.pull_over.is_none());
    assert!(drive.enforcement_events.is_empty());
}

#[test]
fn test_scale_bypass_does_not_overwrite_active_pull_over() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    let limit = speed_for(&mut drive, &mut app, 25.0);
    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
    let stop = a_scale(10.0);
    drive.trip.stops = vec![stop.clone()];
    drive.trip.posts.push(open_scale_post(&stop));
    drive.trip.position_mi = 10.1;
    drive.trip.truck.velocity_mps = mph_to_mps(limit + 25.0);

    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);

    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
    assert_eq!(drive.pull_over_kind, "speeding");
}

// -- unsafe equipment -----------------------------------------------------------------

#[test]
fn test_unsafe_damage_in_patrol_starts_safety_stop() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    app.seasoned_driver();
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.trip.truck.damage_pct = 70.0;
    drive.trip.truck.velocity_mps = mph_to_mps(35.0);

    drive.check_unsafe_damage_enforcement(&mut app.ctx);

    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
    assert_eq!(drive.pull_over_kind, "unsafe_damage");
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    let zone = with_top::<EnforcementStopState, _>(&mut app, |stop, _| stop.construction_zone);
    let expected = citation_fine(UNSAFE_DAMAGE_FINE, 0, zone, None);
    assert!(approx(drive.ticket_fines_paid, expected));
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected
    ));
}

#[test]
fn test_a_trooper_alongside_sees_bald_tires_and_runs_a_walk_around() {
    // The rolling look: a commercial-vehicle unit on the shoulder reads the
    // tread as the truck passes and pulls it in for a Level 2, which writes
    // the tire up, replaces it out of service, and lets the truck go.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip.position_mi = 10.0;
    drive.trip.posts = vec![always_observing_post(10.2, KIND_CMV, 0.6, 1.0)];
    drive.trip.truck.tire_wear_pct = 95.0;
    drive.trip.truck.velocity_mps = mph_to_mps(60.0);

    let seen = drive
        .observed_now()
        .expect("bald tires are seen from the shoulder");
    assert_eq!(seen.what, WHAT_EQUIPMENT);
    drive.begin_observed_stop(&mut app.ctx, &seen);
    assert_eq!(drive.pull_over_kind, "roadside_walkaround");

    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    let text =
        with_top::<EnforcementStopState, _>(&mut app, |stop, _| stop.outcome_text().to_string());
    assert!(text.contains("Level 2 walk-around inspection"), "{text}");
    assert!(
        text.contains("a tire below the minimum tread depth"),
        "{text}"
    );
    assert!(text.contains("fitted new tires"), "{text}");
    assert_eq!(drive.trip.truck.tire_wear_pct, 0.0);
    // The report priced the stop: one out-of-service fine, nothing else.
    let money_after = app.ctx.profile.as_ref().expect("a career").money();
    assert!(approx(
        money_after,
        money_before - ff_core::sim::roadside_inspection::OUT_OF_SERVICE_FINE
    ));
}

#[test]
fn test_unsafe_damage_needs_active_enforcement() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip.truck.damage_pct = 85.0;
    drive.trip.truck.velocity_mps = mph_to_mps(35.0);

    drive.check_unsafe_damage_enforcement(&mut app.ctx);

    assert!(drive.pull_over.is_none());
    assert!(drive.enforcement_events.is_empty());
}

// -- construction zones and repeat offenders -------------------------------------------

#[test]
fn test_the_merge_taper_counts_as_being_in_the_construction_zone() {
    // One signed footprint: the cones and the doubled-fine sign start there.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip.position_mi = 5.0;
    drive.trip.zones = vec![Zone::new(4.0, 6.0, 55.0, "construction merge")];
    assert!(drive.trip.in_construction_zone());
    drive.trip.zones = vec![Zone::new(4.0, 6.0, 45.0, "heavy traffic")];
    assert!(!drive.trip.in_construction_zone());
}

#[test]
fn test_roadwork_hides_behind_a_jam_and_still_doubles_the_fine() {
    // active_zone returns the slowest zone; the predicate must not use it.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip.position_mi = 5.0;
    drive.trip.zones = vec![
        Zone::new(4.0, 6.0, 45.0, "construction"),
        Zone::new(4.0, 6.0, 20.0, "heavy traffic"),
    ];
    assert_eq!(
        drive.trip.active_zone().map(|zone| zone.reason),
        Some("heavy traffic".to_string())
    );
    assert!(drive.trip.in_construction_zone());
}

#[test]
fn test_a_scale_bypass_in_roadwork_costs_double_and_says_so() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip_seed = 1; // lands under the 85 percent catch chance; see above
    pave_construction(&mut drive);
    blow_past_a_scale(&mut drive);

    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);
    assert!(drive.pull_over_construction_zone);

    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<EnforcementStopState>(&app));
    let expected = citation_fine(WEIGH_STATION_BYPASS_FINE, 0, true, None);
    assert!(approx(expected, WEIGH_STATION_BYPASS_FINE * 2.0));
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected
    ));
    // The driver hears the figure that was actually charged, and why.
    let text =
        with_top::<EnforcementStopState, _>(&mut app, |stop, _| stop.outcome_text().to_string());
    assert!(
        text.contains(&format!("{} dollars", fmt_grouped_0(expected))),
        "{text}"
    );
    assert!(text.contains("doubled"), "{text}");
    assert!(text.contains("construction zone"), "{text}");
}

#[test]
fn test_a_repeat_scale_bypass_in_roadwork_compounds_rather_than_adds() {
    // The owner's call: 1,800 x 1.5 x 2 = 5,400 on a second offense.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip_seed = 1; // lands under the 85 percent catch chance; see above
    pave_construction(&mut drive);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .citations = 1;
    blow_past_a_scale(&mut drive);

    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    let expected = citation_fine(WEIGH_STATION_BYPASS_FINE, 1, true, None);
    assert!(approx(expected, 5_400.0), "{expected}");
    assert!(
        !approx(expected, WEIGH_STATION_BYPASS_FINE * 2.5),
        "the repeat step is compounded, not added"
    );
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected
    ));
}

#[test]
fn test_a_speeding_ticket_in_roadwork_doubles_and_the_line_says_the_charge() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    pave_construction(&mut drive);
    speed_for(&mut drive, &mut app, 25.0); // well over -> a ticket, not a warning
    assert!(drive.pull_over_construction_zone);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();

    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<TrafficStopState>(&app));
    let (over, zone, text) = with_top::<TrafficStopState, _>(&mut app, |stop, _| {
        (
            stop.over,
            stop.construction_zone,
            stop.outcome_text().to_string(),
        )
    });
    assert!(zone);
    let base = speeding_citation_fine(over, 0, false);
    let expected = speeding_citation_fine(over, 0, true);
    assert!(approx(expected, base * 2.0));
    assert!(approx(drive.ticket_fines_paid, expected));
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected
    ));
    assert!(
        text.contains(&format!("{} dollars", fmt_grouped_0(expected))),
        "{text}"
    );
    assert!(text.contains("doubled"), "{text}");
    assert!(text.contains("construction zone"), "{text}");
}

#[test]
fn test_leaving_the_zone_before_stopping_does_not_undo_the_doubling() {
    // The zone that counts is where the violation happened, not the shoulder.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    pave_construction(&mut drive);
    speed_for(&mut drive, &mut app, 25.0);
    drive.trip.zones = Vec::new(); // rolled out the far end before braking
    assert!(!drive.trip.in_construction_zone());

    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<TrafficStopState>(&app));
    let (over, zone) =
        with_top::<TrafficStopState, _>(&mut app, |stop, _| (stop.over, stop.construction_zone));
    assert!(zone);
    assert!(approx(
        drive.ticket_fines_paid,
        speeding_citation_fine(over, 0, true)
    ));
}

#[test]
fn test_a_non_speeding_stop_escalates_with_priors() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    app.seasoned_driver();
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .driving_record
        .citations = 2;
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.trip.truck.damage_pct = 70.0;
    drive.trip.truck.velocity_mps = mph_to_mps(35.0);

    drive.check_unsafe_damage_enforcement(&mut app.ctx);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    let zone = with_top::<EnforcementStopState, _>(&mut app, |stop, _| stop.construction_zone);
    let expected = citation_fine(UNSAFE_DAMAGE_FINE, 2, zone, None);
    assert!(expected > UNSAFE_DAMAGE_FINE);
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected
    ));
}

#[test]
fn test_f1_help_names_non_speed_enforcement_pullovers() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    app.clear_speech();

    State::handle_event(&mut drive, &mut app.ctx, &InputEvent::key(Key::F1));

    let lines = app.main_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("F1 said nothing at all"));
    assert!(
        last.contains("X also signals a pull-over when a trooper lights you up"),
        "{last}"
    );
    assert!(
        last.contains("a scale bypass, or unsafe equipment"),
        "{last}"
    );
    assert!(last.contains("signal, then brake to a stop"), "{last}");
}

// -- the compliance tracker -------------------------------------------------------------

#[test]
fn test_braking_to_a_stop_reaches_the_roadside_stop() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0);
    drive.signal_pull_over(&mut app.ctx); // signal, then brake steadily
    past_grace(&mut drive);
    for _ in 0..4 {
        drive.update_pull_over(&mut app.ctx, 1.0, true);
    }
    assert!(drive.pull_over_compliance >= PULL_OVER_FULL_COMPLIANCE);
    // The clean-stop leniency is a real one-in-four chance; this case is
    // about reaching the roadside stop and being ticketed, so bring the truck
    // to rest on a mile where this trip genuinely does not roll it, rather
    // than leaving a one-in-four flake in the test.
    drive.trip.position_mi = a_waiver_mile(drive.trip_seed, false, drive.trip.position_mi);
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<TrafficStopState>(&app));
    assert!(with_top::<TrafficStopState, _>(&mut app, |stop, _| stop.clean_stop));
    assert_eq!(drive.speeding_tickets, 1); // over 25 -> a ticket, not waived here
}

#[test]
fn test_clean_stop_can_waive_a_ticket_to_a_warning() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0);
    drive.signal_pull_over(&mut app.ctx);
    past_grace(&mut drive);
    for _ in 0..4 {
        drive.update_pull_over(&mut app.ctx, 1.0, true);
    }
    // The leniency roll is a named, position-quantised seed so a reload
    // cannot re-roll it. Stop on a mile where it really does come up.
    drive.trip.position_mi = a_waiver_mile(drive.trip_seed, true, drive.trip.position_mi);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, false);
    app.ctx.run_deferred();

    assert!(top_is::<TrafficStopState>(&app));
    assert_eq!(drive.speeding_tickets, 0);
    assert!(approx(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before
    ));
    let text = with_top::<TrafficStopState, _>(&mut app, |stop, _| stop.outcome_text().to_string());
    assert!(text.contains("lets it go with a warning"), "{text}");
}

#[test]
fn test_failing_to_signal_takes_a_one_time_deduction() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0);
    past_grace(&mut drive);
    // Brake steadily but never signal: compliance climbs until the 5 s signal
    // grace lapses, when a one-time deduction drops it.
    let mut compliance = Vec::new();
    for _ in 0..7 {
        drive.update_pull_over(&mut app.ctx, 1.0, true);
        compliance.push(drive.pull_over_compliance);
    }
    assert!(drive.pull_over_nosignal_hit);
    // The tick that crosses the grace dips below the prior tick.
    assert!(compliance[5] < compliance[4], "{compliance:?}");
}

#[test]
fn test_continuous_coasting_slowly_drains_compliance() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    speed_for(&mut drive, &mut app, 25.0);
    drive.signal_pull_over(&mut app.ctx); // signal so only coasting is in play
    past_grace(&mut drive);
    let before = drive.pull_over_compliance;
    // Hold a steady speed (neither braking nor accelerating) for 5 s.
    for _ in 0..5 {
        drive.update_pull_over(&mut app.ctx, 1.0, false);
    }
    assert!(drive.pull_over.is_some()); // coasting drains, but not instantly
    assert!(drive.pull_over_compliance < before);
}

// -- the out-of-service order and the snapshot -------------------------------------------

#[test]
fn test_out_of_service_stop_shuts_down_the_engine() {
    // The ten-hour out-of-service order is a real overnight fast-forward: it
    // must shut the engine down like every other sleep path (motel, sleeper
    // split, shoulder, lot), not leave it idling through the night, and it
    // must tell the driver how to get moving again afterward.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip.truck.start_engine();
    assert!(drive.trip.truck.engine_on);

    drive.push_enforcement_stop_state(
        &mut app.ctx,
        EnforcementStopParams {
            title: "Log check".to_string(),
            summary: "Evidence: HOS/ELD violation.".to_string(),
            fine: 500.0,
            reputation_hit: 10.0,
            signaled: true,
            return_message: "Back on the highway with a reset clock.".to_string(),
            out_of_service: true,
            warned: false,
            construction_zone: false,
            inspection_on_stop: false,
            inspection_level: None,
        },
    );
    app.ctx.run_deferred();

    assert!(!drive.trip.truck.engine_on);
    let text =
        with_top::<EnforcementStopState, _>(&mut app, |stop, _| stop.outcome_text().to_lowercase());
    assert!(text.contains("shut down the engine"), "{text}");
    // Ten hours parked with the engine off bleeds the air down, so the
    // outcome says where the truck stands.
    assert!(text.contains("engine off, air"), "{text}");
}

#[test]
fn test_ticket_counters_survive_snapshot() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, Some(1.0));
    drive.speeding_tickets = 2;
    drive.ticket_fines_paid = 450.0;
    drive.failure_to_stop_count = 1;
    app.ctx.profile.as_mut().expect("a career").active_trip = None;
    let snapshot = drive.snapshot(&app.ctx);

    let restored = DrivingState::from_snapshot(&mut app.ctx, &snapshot)
        .expect("the snapshot the drive just wrote reloads");

    assert_eq!(restored.speeding_tickets, 2);
    assert!(approx(restored.ticket_fines_paid, 450.0));
    assert_eq!(restored.failure_to_stop_count, 1);
}

/// `f"{amount:,.0f}"`: the grouped whole-dollar form the spoken lines use.
fn fmt_grouped_0(amount: f64) -> String {
    ff_core::pyfmt::fmt_grouped(amount, 0)
}

// -- the routine roadside inspection ----------------------------------------------------

#[test]
fn test_a_routine_level_three_on_a_legal_driver_costs_the_minutes_and_nothing_else() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, None);
    drive.trip.truck.tire_wear_pct = 100.0; // a Level 3 never looks at the equipment
    let money_before = app.ctx.profile.as_ref().unwrap().money();
    let citations_before = app.ctx.profile.as_ref().unwrap().driving_record.citations;
    let minutes_before = drive.trip.game_minutes;

    drive.push_enforcement_stop_state(
        &mut app.ctx,
        EnforcementStopParams {
            title: "Roadside inspection".to_string(),
            summary: "Routine roadside inspection, Level 3.".to_string(),
            fine: 0.0,
            reputation_hit: 0.0,
            signaled: true,
            return_message: "Back on the highway.".to_string(),
            out_of_service: false,
            warned: false,
            construction_zone: false,
            inspection_on_stop: false,
            inspection_level: Some(InspectionLevel::DriverOnly),
        },
    );
    app.ctx.run_deferred();

    let text =
        with_top::<EnforcementStopState, _>(&mut app, |stop, _| stop.outcome_text().to_string());
    assert!(text.contains("Clean Level 3 driver inspection"), "{text}");
    let p = app.ctx.profile.as_ref().unwrap();
    assert_eq!(p.money(), money_before);
    assert_eq!(p.driving_record.citations, citations_before);
    assert!(
        (drive.trip.game_minutes - minutes_before - InspectionLevel::DriverOnly.minutes()).abs()
            < 1e-6
    );
    assert_eq!(
        p.achievement_stats
            .get("inspections_passed")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        1
    );
}
