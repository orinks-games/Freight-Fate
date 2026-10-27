//! Ramp terminals: the light or the sign where the ramp meets the surface
//! road, its stop bar's instruments, the route-transition assist that works
//! the pedals for it, and the speed-control session it hands back.
//!
//! Ported from `tests/test_ramp_terminals.py`. The fourteen cases that only
//! needed `begin_ramp_terminal` and `update_ramp_terminal` already live in
//! `states_driving_events.rs`; everything else in that file is here.
//!
//! # What replaced the monkeypatches
//!
//! | Python | here |
//! |---|---|
//! | `trip.ramp_control_at = lambda mi: "stop"` | [`bake_ramp_control`] puts a real `Interchange` carrying that control on the leg, which is the record `ramp_control_at` reads |
//! | `trip.upcoming_stop = lambda w: stop` | the stop is pushed onto `trip.stops` at the mile the case wants |
//! | `ctx.audio.play = ...`, `ctx.audio.start_loop = ...`, `hold_alert` | a real [`AudioEngine`] over a [`RecordingBackend`], so the facade's own routing runs and the backend records what it was asked to sound |
//! | `ctx.say` / `ctx.say_event` capture | `TestApp`'s capture at `ctx.speech`, one rung below the ladder and the pacer -- so these assert what a player HEARS |
//! | `_approach_limit_text` patched to `""` | (that case is already ported in `states_driving_events.rs`) |
//!
//! The held tone is watched at `start_loop`/`stop_loop` on the backend rather
//! than at `hold_alert`/`release_alert` on the facade, because Rust has no
//! seam for an inherent method. The facade calls `start_loop` exactly once per
//! `hold_alert` and `stop_loop` once per `release_alert`, so the counts mean
//! what the Python counts meant.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use ff_core::data::curves::RouteCurve;
use ff_core::data::world::get_world;
use ff_core::data::world_models::{CorridorDetail, Interchange, Leg, Route};
use ff_core::data::world_parsing::parse_interchange;
use ff_core::models::enforcement::{citation_fine, RED_LIGHT_FINE, STOP_SIGN_FINE};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::cross_traffic::{CrossTraffic, CrossVehicle};
use ff_core::sim::trip_models::{RoadStop, TripEvent, TripEventData, TripEventKind, Zone};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::SpokenMessage;
use hearken::speech::EventPriority;

use freight_fate::app::testing::{FakeClock, TestApp};
use freight_fate::audio::{
    Audio, AudioBackend, AudioEngine, Buses, ALERT_HOLD_TIMEOUT_S, CH_ALERT,
};
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;

use crate::transcript_cruise_support::{
    bench_road, frame, quiet, release_keys, start_drive, DT, MPS_PER_MPH, START_MI,
};

const MPH_PER_MPS: f64 = 2.2369362920544;

// -- rigging -------------------------------------------------------------------------

/// `_driving(app)`: a Buffalo to Rochester delivery on an empty road.
///
/// Two pins Python does not have, neither of which any case here measures:
/// the trip seed (Python's is unseeded, so the zones and stops it places are
/// a fresh draw every run) and the weather (an unseeded sky can come up ice,
/// whose safe speed sits under the speeds these cases roll at).
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Ramps", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
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
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    app.clear_speech();
    drive
}

fn mph_to_mps(mph: f64) -> f64 {
    mph / MPH_PER_MPS
}

/// `_FakeStop`: a bare route point at a milepost.
fn a_stop(at_mi: f64) -> RoadStop {
    RoadStop::new("Test Plaza", at_mi, "travel_center")
}

/// `_on_ramp`: the truck mid-ramp at the terminal bar with a known light.
fn on_ramp(d: &mut DrivingState, control: &str, red: bool, mph: f64) {
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = mph_to_mps(mph);
    d.ramp_mi = Some(RAMP_ACCESS_MI); // right at the terminal bar
    d.ramp_control = control.to_string();
    d.ramp_light_offset_s = if red { 0.0 } else { RAMP_LIGHT_RED_S }; // phase start
    d.ramp_light_timer = 0.0;
    d.ramp_light_announced = true;
    d.ramp_light_last_phase = if red { "red" } else { "green" }.to_string();
    d.ramp_terminal_done = false;
    d.ramp_waiting_at_light = false;
    d.ramp_stop = Some(a_stop(d.trip.position_mi + 0.5));
}

/// `monkeypatch.setattr(trip, "ramp_control_at", lambda mi: control)` done for
/// real: the leg the truck is on gets an `Interchange` at `at_mi` carrying
/// that control, which is the baked record `ramp_control_at` actually reads.
///
/// Python could rewrite the method on a live trip; Rust cannot, so the road
/// under the truck is rebuilt to ANSWER the way the patch did. Stricter than
/// the patch, too: `ramp_meets_a_freeway` and the exit machinery see a real
/// interchange rather than a lambda that only one call site consulted.
fn bake_ramp_control(d: &mut DrivingState, at_mi: f64, control: &str) {
    bake_ramp_controls(d, &[(at_mi, control)]);
    assert_eq!(
        d.trip.ramp_control_at(at_mi, 0.15),
        control,
        "the baked control has to be the one the trip reads back"
    );
}

/// The same, for a leg with several exits: one `(at_mi, control)` each.
fn bake_ramp_controls(d: &mut DrivingState, exits: &[(f64, &str)]) {
    let leg = &d.trip.route.legs[0];
    let mut detail: CorridorDetail = leg.corridor().clone();
    detail.interchanges = exits
        .iter()
        .map(|(at_mi, control)| Interchange {
            at_mi: *at_mi,
            exit_ref: "7".to_string(),
            highway: leg.highway.clone(),
            source: "test".to_string(),
            ramp_control: control.to_string(),
            ..Default::default()
        })
        .collect();
    let rebuilt = Leg::new(
        &leg.a,
        &leg.b,
        leg.miles,
        &leg.highway,
        &leg.terrain,
        leg.stops.clone(),
    )
    .with_detail(detail);
    let mut legs = vec![std::sync::Arc::new(rebuilt)];
    legs.extend(d.trip.route.legs[1..].iter().cloned());
    d.trip.route = Route {
        cities: d.trip.route.cities.clone(),
        legs,
    };
}

/// An `Audio` that records what it was asked to sound, through the real
/// facade: `ctx.audio.play`, and the loop starts and stops the held alert
/// tone is built out of.
#[derive(Default)]
struct AudioCalls {
    played: Vec<(String, f64)>,
    loops_started: Vec<(u32, String)>,
    loops_stopped: Vec<u32>,
}

type AudioLog = Rc<RefCell<AudioCalls>>;

struct RecordingBackend {
    buses: Buses,
    log: AudioLog,
}

impl AudioBackend for RecordingBackend {
    fn name(&self) -> &'static str {
        "recording"
    }
    fn enabled(&self) -> bool {
        true
    }
    fn buses(&self) -> &Buses {
        &self.buses
    }
    fn buses_mut(&mut self) -> &mut Buses {
        &mut self.buses
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn play(&mut self, key: &str, volume: f64, _pan: f64) {
        self.log.borrow_mut().played.push((key.to_string(), volume));
    }
    fn start_loop(&mut self, channel: u32, key: &str, _volume: f64, _fade_ms: u32) {
        self.log
            .borrow_mut()
            .loops_started
            .push((channel, key.to_string()));
    }
    fn stop_loop(&mut self, channel: u32, _fade_ms: u32) {
        self.log.borrow_mut().loops_stopped.push(channel);
    }
}

fn recording_engine() -> (AudioEngine, AudioLog) {
    let log: AudioLog = Rc::new(RefCell::new(AudioCalls::default()));
    let engine = AudioEngine::with_backend(Box::new(RecordingBackend {
        buses: Buses::new(),
        log: Rc::clone(&log),
    }));
    (engine, log)
}

/// Put a recording audio facade under `ctx.audio` and hand back its log.
fn record_audio(app: &mut TestApp) -> AudioLog {
    let (engine, log) = recording_engine();
    app.ctx.audio = Box::new(engine);
    log
}

fn played_keys(log: &AudioLog) -> Vec<String> {
    log.borrow().played.iter().map(|(k, _)| k.clone()).collect()
}

/// Held-tone assertions: how many times the alert channel was told to sound
/// `key`, and how many times it was told to stop.
fn alert_holds(log: &AudioLog, key: &str) -> usize {
    log.borrow()
        .loops_started
        .iter()
        .filter(|(ch, k)| *ch == CH_ALERT && k == key)
        .count()
}

fn alert_releases(log: &AudioLog) -> usize {
    log.borrow()
        .loops_stopped
        .iter()
        .filter(|ch| **ch == CH_ALERT)
        .count()
}

/// Move the pacer's clock well past both the stale budget and the repeat
/// window.
///
/// The pacer measures staleness and repeats on the WALL clock, and these
/// cases step the drive without any real time passing -- which tells the
/// pacer the voice is still mid-sentence, so a second queued ROUTE line
/// purges the backlog and the line it stepped on is handed back and spoken
/// again. That requeue is a property of a bench that freezes time, not of
/// the ramp: hundreds of feet at ramp speed are tens of real seconds. Moving
/// the clock is also what keeps the counts HONEST -- past the repeat window,
/// a milestone spoken twice by a broken latch is counted twice instead of
/// being swallowed as a duplicate.
fn settle(clock: &FakeClock) {
    clock.advance(30.0);
}

/// Every line the drive has said, both channels, in submission order.
fn spoken(app: &TestApp) -> Vec<String> {
    app.speech().lines()
}

fn said_any(app: &TestApp, needle: &str) -> bool {
    spoken(app).iter().any(|line| line.contains(needle))
}

fn said_count(app: &TestApp, needle: &str) -> usize {
    spoken(app)
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

// -- the terminal control ------------------------------------------------------------

#[test]
fn test_baked_interchange_control_beats_the_heuristic() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    bake_ramp_control(&mut d, 30.0, "stop");

    assert_eq!(d.trip.ramp_control_at(30.0, 0.15), "stop");
    d.begin_ramp_terminal(&app.ctx, &a_stop(30.0));
    assert_eq!(d.ramp_control, "stop");
}

#[test]
fn test_a_stop_matched_to_its_interchange_reads_that_ramp_and_no_other() {
    // A stop's mile is a projection and is routinely a mile or three off,
    // so the exit nearest it by mile marker is often a neighbour. The bake
    // matches the stop to the interchange that serves it, and that record
    // answers however far off the stop's own mile sits.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    bake_ramp_controls(&mut d, &[(30.1, "signal"), (33.0, "stop"), (36.0, "")]);

    let mut stop = a_stop(30.0);
    assert_eq!(d.ramp_control_for(&app.ctx, &stop, None), "signal");
    stop.interchange_mi = Some(33.0);
    assert_eq!(d.ramp_control_for(&app.ctx, &stop, None), "stop");
    d.begin_ramp_terminal(&app.ctx, &stop);
    assert_eq!(d.ramp_control, "stop");

    // Matched to an exit the map records no control for: the neighbour's
    // light is not borrowed, and the seeded guess stands in as before.
    let mut unrecorded = a_stop(30.0);
    unrecorded.interchange_mi = Some(36.0);
    bake_ramp_controls(&mut d, &[(30.1, "roundabout"), (36.0, "")]);
    assert_ne!(
        d.ramp_control_for(&app.ctx, &unrecorded, None),
        "roundabout"
    );
}

#[test]
fn test_ramp_control_is_knowable_before_the_ramp() {
    // The signal-on announcement a mile out and the ramp itself must always
    // agree: ramp_control_for is a pure preview of the decision
    // begin_ramp_terminal commits to.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    for at_mi in [10.0, 22.5, 30.0, 41.0, 55.0] {
        let stop = a_stop(at_mi);
        let early = d.ramp_control_for(&app.ctx, &stop, None);
        d.begin_ramp_terminal(&app.ctx, &stop);
        assert_eq!(d.ramp_control, early, "at mile {at_mi}");
    }
}

#[test]
fn test_interchange_parser_accepts_and_validates_ramp_control() {
    let raw = |control: &str| {
        serde_json::json!({
            "at_mi": 10.0,
            "exit_ref": "12",
            "source": "test source",
            "ramp_control": control,
        })
    };
    let ix = parse_interchange(&raw("signal"), 50.0, "A", "B", "I-99").expect("a signal parses");
    assert_eq!(ix.ramp_control, "signal");
    // Yield and roundabout became legal controls with the cross bubble
    // (2026-08-20); junk still refuses to load.
    for control in ["yield", "roundabout"] {
        let ix = parse_interchange(&raw(control), 50.0, "A", "B", "I-99")
            .unwrap_or_else(|e| panic!("{control} should parse: {e}"));
        assert_eq!(ix.ramp_control, control);
    }
    assert!(parse_interchange(&raw("flagger"), 50.0, "A", "B", "I-99").is_err());
}

// -- the assist and the crossing -----------------------------------------------------

#[test]
fn test_transition_assist_off_leaves_the_pedals_alone() {
    // Realistic drivers who turned the assist off keep the manual bar.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.route_transition_assist = false;
    on_ramp(&mut d, "signal", true, 35.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.08);
    d.trip.truck.brake = 0.0;
    d.trip.truck.throttle = 0.5;

    d.update_ramp_terminal_assist(&mut app.ctx);

    assert_eq!(d.trip.truck.brake, 0.0);
    assert_eq!(d.trip.truck.throttle, 0.5);
    assert!(!d.ramp_waiting_at_light);
}

#[test]
fn test_stop_sign_full_stop_clears() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "stop", false, RED_STOP_MPH - 1.0);

    d.update_ramp_terminal(&mut app.ctx);

    assert!(d.ramp_terminal_done);
    assert_eq!(d.trip.truck.damage_pct, 0.0);
}

/// Blow the terminal at `mph` on trip seed `seed`, past the bar, and return
/// whether the trailer took damage, what the fine came to, and the wallet
/// before the run.
fn blow_the_terminal(app: &mut TestApp, control: &str, seed: i64, mph: f64) -> (bool, f64, f64) {
    let mut d = a_drive(app);
    // What a citation costs, not the new-driver warning.
    app.seasoned_driver();
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    d.trip_seed = seed;
    d.cross_bubble = None; // the restored-save case: no bubble built yet
    on_ramp(&mut d, control, control == "signal", mph);
    d.ramp_mi = Some(0.05); // past the bar at speed
    let before = d.trip.truck.damage_pct;
    d.update_ramp_terminal(&mut app.ctx);
    app.ctx.run_deferred();
    assert!(d.ramp_terminal_done);
    (
        d.trip.truck.damage_pct > before,
        d.ticket_fines_paid,
        money_before,
    )
}

fn logged(app: &TestApp) -> Vec<String> {
    app.ctx
        .message_log
        .messages
        .iter()
        .map(|m| m.text.clone())
        .collect()
}

/// The first seed in `range(60)` whose watching roll lands under the catch
/// chance, and the first that does not (the chain-law test's `_cited_seed`).
fn signal_run_caught_and_missed(at_mi: f64) -> (i64, i64) {
    let mut caught = None;
    let mut missed = None;
    for seed in 0..60 {
        let roll = PyRandom::new_from_str(&format!("{seed}:signal-run:{at_mi:.1}")).random();
        if roll < SIGNAL_RUN_CATCH_CHANCE && caught.is_none() {
            caught = Some(seed);
        } else if roll >= SIGNAL_RUN_CATCH_CHANCE && missed.is_none() {
            missed = Some(seed);
        }
    }
    (
        caught.expect("some seed is seen"),
        missed.expect("some seed gets away"),
    )
}

#[test]
fn test_blowing_the_stop_sign_is_dice_not_a_guaranteed_clip() {
    // Owner playtest 2026-07-15: blowing the sign ALWAYS clipped cross
    // traffic. The crossroad is a seeded traffic day now, even with no
    // bubble to consult: some days a semi is crossing, some days nobody is.
    let mut clipped = 0;
    let mut clean = 0;
    for seed in 0..40 {
        let mut app = TestApp::new();
        let (hit, _, _) = blow_the_terminal(&mut app, "stop", seed, 30.0);
        if hit {
            clipped += 1;
        } else {
            clean += 1;
        }
    }
    assert!(clipped > 0, "no seed ever met cross traffic");
    assert!(clean > 0, "every seed clipped: the certainty is back");
}

#[test]
fn test_running_the_red_light_is_dice_not_a_guaranteed_clip() {
    let mut clipped = 0;
    let mut clean = 0;
    for seed in 0..40 {
        let mut app = TestApp::new();
        let (hit, _, _) = blow_the_terminal(&mut app, "signal", seed, 30.0);
        if hit {
            clipped += 1;
        } else {
            clean += 1;
        }
    }
    assert!(clipped > 0, "no seed ever met cross traffic");
    assert!(clean > 0, "every seed clipped: the certainty is back");
}

#[test]
fn test_running_the_red_light_risks_a_citation_on_a_seeded_roll() {
    // The bar of `on_ramp` sits half a mile past the truck.
    let at_mi = {
        let mut app = TestApp::new();
        let d = a_drive(&mut app);
        d.trip.position_mi + 0.5
    };
    let (caught, missed) = signal_run_caught_and_missed(at_mi);

    let mut app = TestApp::new();
    let (_, fine, money_before) = blow_the_terminal(&mut app, "signal", caught, 30.0);
    let expected = citation_fine(RED_LIGHT_FINE, 0, false, None);
    assert!((fine - expected).abs() < 0.01, "{fine}");
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!((p.money() - (money_before - expected)).abs() < 0.01);
    assert_eq!(p.driving_record.citations, 1);
    let cited: Vec<String> = logged(&app)
        .into_iter()
        .filter(|s| s.contains("Running the red light is a citation"))
        .collect();
    assert_eq!(cited.len(), 1, "{:?}", logged(&app));
    assert!(
        cited[0].contains(&format!("{} dollars", expected as i64)),
        "{}",
        cited[0]
    );

    // Nobody watching: the crossroad decides the outcome, the wallet is untouched.
    drop(app);
    let mut app = TestApp::new();
    let (_, fine, money_before) = blow_the_terminal(&mut app, "signal", missed, 30.0);
    assert_eq!(fine, 0.0);
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before
    );
    assert!(!logged(&app).iter().any(|s| s.contains("is a citation")));
}

#[test]
fn test_rolling_the_stop_sign_risks_the_stop_sign_citation() {
    let at_mi = {
        let mut app = TestApp::new();
        let d = a_drive(&mut app);
        d.trip.position_mi + 0.5
    };
    let (caught, _) = signal_run_caught_and_missed(at_mi);
    let mut app = TestApp::new();
    // A roll under the clip speed: no collision is possible, the citation still is.
    let (hit, fine, _) = blow_the_terminal(&mut app, "stop", caught, STOP_ROLL_CLIP_MPH - 5.0);
    assert!(!hit);
    assert!((fine - citation_fine(STOP_SIGN_FINE, 0, false, None)).abs() < 0.01);
    assert!(logged(&app)
        .iter()
        .any(|s| s.contains("Running the stop sign is a citation")));
}

#[test]
fn test_light_cycle_alternates() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.ramp_light_offset_s = 0.0;
    d.ramp_light_timer = 0.0;
    assert!(d.ramp_light_is_red());
    assert_eq!(d.ramp_light_phase(), "red");
    d.ramp_light_timer = RAMP_LIGHT_RED_S + 0.1;
    assert!(!d.ramp_light_is_red());
    assert_eq!(d.ramp_light_phase(), "green");
    // Green ends in yellow, not a hard cut to red -- and yellow is legal.
    d.ramp_light_timer = RAMP_LIGHT_RED_S + RAMP_LIGHT_GREEN_S + 0.1;
    assert_eq!(d.ramp_light_phase(), "yellow");
    assert!(!d.ramp_light_is_red());
    d.ramp_light_timer = RAMP_LIGHT_RED_S + RAMP_LIGHT_GREEN_S + RAMP_LIGHT_YELLOW_S + 0.1;
    assert!(d.ramp_light_is_red());
}

#[test]
fn test_crossing_on_yellow_is_legal() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", false, GREEN_ROLL_MPH - 5.0);
    // Put the cycle just into the yellow phase at the stop bar.
    d.ramp_light_offset_s = RAMP_LIGHT_RED_S + RAMP_LIGHT_GREEN_S + 0.5;
    assert_eq!(d.ramp_light_phase(), "yellow");

    d.update_ramp_terminal(&mut app.ctx);

    assert!(d.ramp_terminal_done);
    assert_eq!(d.trip.truck.damage_pct, 0.0);
}

// -- the bar's own instruments -------------------------------------------------------

#[test]
fn test_stopped_short_of_the_light_gets_creep_guidance() {
    // A cautious stop far short of the bar must not read as a stuck light:
    // the game says the driver is short and to creep up (playtest 2026-07-16).
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", true, 0.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.15); // stopped well short of the bar
    app.clear_speech();

    d.update_ramp_light(&mut app.ctx, 0.1);
    // A real gap is named in feet and driven, not crept: 0.15 mi of "creep"
    // spans several light cycles and reads as a stuck light.
    assert!(
        said_any(&app, "800 feet short of the light"),
        "{:?}",
        spoken(&app)
    );
    assert!(
        !said_any(&app, "Stopped short of the light"),
        "{:?}",
        spoken(&app)
    );

    // Once per stop, not every frame.
    settle(&clock);
    d.update_ramp_light(&mut app.ctx, 0.1);
    assert_eq!(said_count(&app, "short of the light"), 1);

    // A stop somewhere nearer prompts again, still with its distance: under
    // two hundred feet it used to drop the number, and the driver had to ask
    // for it (agent drive B, 2026-09-24).
    settle(&clock);
    d.trip.truck.velocity_mps = mph_to_mps(10.0);
    d.update_ramp_light(&mut app.ctx, 0.1);
    settle(&clock);
    d.trip.truck.velocity_mps = 0.0;
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.02);
    d.update_ramp_light(&mut app.ctx, 0.1);
    assert_eq!(
        said_count(&app, "short of the light"),
        2,
        "{:?}",
        spoken(&app)
    );
    assert!(
        said_any(&app, "Stopped 100 feet short of the light."),
        "{:?}",
        spoken(&app)
    );

    // At the bar the prompt stays quiet: the waiting handshake owns it.
    settle(&clock);
    app.clear_speech();
    d.ramp_creep_prompt_said = false;
    d.ramp_mi = Some(RAMP_ACCESS_MI);
    d.update_ramp_light(&mut app.ctx, 0.1);
    assert!(!said_any(&app, "short of the light"), "{:?}", spoken(&app));
}

#[test]
fn test_light_change_wording_is_short_at_any_distance() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Short of the bar, moving: announce only the color.
    on_ramp(&mut d, "signal", false, 20.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.15);
    app.clear_speech();
    d.update_ramp_light(&mut app.ctx, RAMP_LIGHT_GREEN_S + 0.5); // into yellow
    assert!(
        spoken(&app).iter().any(|line| line == "Light yellow."),
        "{:?}",
        spoken(&app)
    );

    // At the bar: use a fresh session to avoid duplicate-speech suppression.
    drop(d);
    drop(app);
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", false, 20.0);
    d.update_ramp_light(&mut app.ctx, RAMP_LIGHT_GREEN_S + 0.5);
    assert!(
        spoken(&app).iter().any(|line| line == "Light yellow."),
        "{:?}",
        spoken(&app)
    );
}

#[test]
fn test_every_light_change_is_spoken_on_the_approach() {
    // The silent flip back to red between a spoken green and the stop bar
    // cost a real playtester trailer damage; every phase change must speak.
    let mut app = TestApp::new();
    // The voice's clock runs with the light's: frozen, every phase line was
    // still "speaking" when the next arrived, and the last red passed only
    // because a replay of the first one landed in the transcript.
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", true, 10.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.3); // still descending the ramp
    app.clear_speech();
    d.announce_ramp_terminal(&mut app.ctx);
    assert_eq!(spoken(&app), vec!["Light red."]);
    app.clear_speech();

    let cycle = RAMP_LIGHT_RED_S + RAMP_LIGHT_GREEN_S + RAMP_LIGHT_YELLOW_S;
    for _ in 0..((cycle * 10.0) as i32 + 5) {
        clock.advance(0.1);
        d.update_ramp_light(&mut app.ctx, 0.1);
    }

    assert!(
        spoken(&app).iter().any(|line| line == "Light green."),
        "{:?}",
        spoken(&app)
    );
    assert!(
        spoken(&app).iter().any(|line| line == "Light yellow."),
        "{:?}",
        spoken(&app)
    );
    assert!(
        spoken(&app).iter().any(|line| line == "Light red."),
        "{:?}",
        spoken(&app)
    );
}

#[test]
fn test_stop_bar_query_names_light_and_distance() {
    // Owner playtest 2026-07-19: "where's the bar, you never know." S must
    // answer with the light phase and the gap, any time the driver asks.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", true, 20.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.1); // ~530 feet short of the bar

    let text = d
        .ramp_light_query_text(&app.ctx)
        .expect("the bar has a position");
    assert!(text.contains("red"), "{text}");
    assert!(text.contains("feet"), "{text}");
    assert!(text.contains("stop bar"), "{text}");

    app.clear_speech();
    d.speak_speed_limit(&mut app.ctx);
    let lines = spoken(&app);
    assert!(
        lines.first().is_some_and(|line| line.contains("stop bar")),
        "{lines:?}"
    );

    // Off the ramp, S goes back to the posted limit.
    d.ramp_mi = None;
    assert!(d.ramp_light_query_text(&app.ctx).is_none());
}

#[test]
fn test_rolling_countdown_speaks_each_milestone_once() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", true, 15.0);
    app.clear_speech();

    for feet in [900.0, 450.0, 250.0, 100.0] {
        d.ramp_mi = Some(RAMP_ACCESS_MI + feet / 5280.0);
        settle(&clock);
        d.update_ramp_gap_countdown(&mut app.ctx);
        settle(&clock);
        d.update_ramp_gap_countdown(&mut app.ctx); // same gap again: no repeat
    }

    let bar_calls: Vec<String> = spoken(&app)
        .into_iter()
        .filter(|line| line.ends_with(" feet."))
        .collect();
    // Only the calls the bar's own tick cannot make: inside its range the
    // tick rate already carries the distance, so speaking it there was the
    // same fact twice (owner, 2026-08-21).
    let expected = d.ramp_bar_milestones(&app.ctx);
    assert!(expected.len() < RAMP_GAP_MILESTONES_FT.len());
    assert_eq!(bar_calls.len(), expected.len(), "{bar_calls:?}");
    assert_eq!(bar_calls[0], "1000 feet.");
    assert_eq!(
        bar_calls[bar_calls.len() - 1],
        format!("{} feet.", expected[expected.len() - 1])
    );

    // Stopped: the countdown yields to the stopped-driver guidance.
    app.clear_speech();
    d.trip.truck.velocity_mps = 0.0;
    d.ramp_gap_milestones_said.clear();
    d.update_ramp_gap_countdown(&mut app.ctx);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));
}

#[test]
fn test_stop_sign_bar_has_position() {
    // Countdown, ticks, S query, and stopped-short guidance all answer at a
    // stop-sign terminal.
    //
    // Playtest 2026-07-22 (Milwaukee grain elevator): the sign announced once,
    // then nothing until "blew the stop sign, 15 percent" -- every bar
    // instrument was gated to signal terminals only.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "stop", false, 15.0);
    let log = record_audio(&mut app);
    app.clear_speech();

    // Rolling countdown through the terminal update, same as a light.
    for feet in [900.0, 450.0, 250.0, 100.0] {
        d.ramp_mi = Some(RAMP_ACCESS_MI + feet / 5280.0);
        settle(&clock);
        d.update_ramp_light(&mut app.ctx, 0.05);
    }
    let bar_calls: Vec<String> = spoken(&app)
        .into_iter()
        .filter(|line| line.ends_with(" feet."))
        .collect();
    // The tick covers the near calls now; see the countdown test above.
    let expected = d.ramp_bar_milestones(&app.ctx);
    assert!(expected.len() < RAMP_GAP_MILESTONES_FT.len());
    assert_eq!(bar_calls.len(), expected.len(), "{bar_calls:?}");
    assert_eq!(bar_calls[0], "1000 feet.");

    // Parking-sensor beeps run for the sign too (outside the solid zone).
    log.borrow_mut().played.clear();
    d.ramp_mi = Some(RAMP_ACCESS_MI + 100.0 / 5280.0);
    d.ramp_bar_tick_timer = 0.0;
    for _ in 0..40 {
        d.update_ramp_light(&mut app.ctx, 0.05);
    }
    assert!(!played_keys(&log).is_empty());

    // S answers with the sign and the gap.
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.1);
    let text = d
        .ramp_light_query_text(&app.ctx)
        .expect("the sign's bar has a position too");
    assert!(text.contains("Stop sign"), "{text}");
    assert!(text.contains("feet"), "{text}");
    assert!(text.contains("stop bar"), "{text}");

    // Stopped short: guidance names the sign, not a light.
    app.clear_speech();
    d.trip.truck.velocity_mps = 0.0;
    d.ramp_creep_prompt_said = false;
    d.update_ramp_light(&mut app.ctx, 0.05);
    let lines = spoken(&app);
    assert!(!lines.is_empty(), "the sign said nothing at all");
    assert!(lines[0].contains("stop sign"), "{:?}", lines[0]);
    assert!(!lines[0].contains("light"), "{:?}", lines[0]);
}

#[test]
fn test_bar_ticks_speed_up_as_the_bar_closes() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", true, 10.0);
    let log = record_audio(&mut app);

    fn ticks_over(
        d: &mut DrivingState,
        app: &mut TestApp,
        log: &AudioLog,
        feet: f64,
        seconds: f64,
    ) -> usize {
        let dt = 0.05;
        log.borrow_mut().played.clear();
        d.ramp_mi = Some(RAMP_ACCESS_MI + feet / 5280.0);
        d.ramp_bar_tick_timer = 0.0;
        for _ in 0..((seconds / dt) as i32) {
            d.update_ramp_bar_ticks(&mut app.ctx, dt);
        }
        log.borrow().played.len()
    }

    let far = ticks_over(&mut d, &mut app, &log, 280.0, 3.0);
    let near = ticks_over(&mut d, &mut app, &log, 80.0, 3.0);
    assert!(near > far && far > 0, "far {far}, near {near}");

    // Inside the final leeway the beeps fuse into the continuous tone (owner
    // spec 2026-07-27): no discrete beeps, the alert loop runs.
    log.borrow_mut().loops_started.clear();
    assert_eq!(ticks_over(&mut d, &mut app, &log, 30.0, 3.0), 0);
    assert!(alert_holds(&log, "vehicle/bar_solid") > 0);

    // Beyond the range, and at a standstill, everything is silent.
    assert_eq!(ticks_over(&mut d, &mut app, &log, 600.0, 3.0), 0);
    d.trip.truck.velocity_mps = 0.0;
    d.bar_solid_on = false;
    log.borrow_mut().loops_started.clear();
    assert_eq!(ticks_over(&mut d, &mut app, &log, 50.0, 3.0), 0);
    assert_eq!(alert_holds(&log, "vehicle/bar_solid"), 0);
}

#[test]
fn test_the_bar_tone_ends_when_the_bar_is_behind_the_truck() {
    // Shane, 2026-08-03: creep up to the bar on a red, reach the solid tone,
    // and it never stopped -- not when he got moving again, not in the menus,
    // not until he killed the game. The tone's only off-switch sat behind the
    // early return that fires the moment the terminal is done, so crossing the
    // bar left it sounding with nothing on the road able to end it.
    for control in ["signal", "stop"] {
        let mut app = TestApp::new();
        let mut d = a_drive(&mut app);
        on_ramp(&mut d, control, true, 5.0);
        let log = record_audio(&mut app);

        // Creeping inside the last leeway: the tone sounds, and keeps being
        // asserted for as long as it applies.
        d.ramp_mi = Some(RAMP_ACCESS_MI + RAMP_BAR_SOLID_MI / 2.0);
        for _ in 0..10 {
            d.update_ramp_light(&mut app.ctx, 0.05);
        }
        assert_eq!(alert_holds(&log, "vehicle/bar_solid"), 10, "{control}");
        assert!(d.bar_solid_on, "{control}");

        // The bar is crossed and the terminal is settled. From here the road
        // has nothing left to warn about: the tone stops, and stays stopped.
        d.ramp_terminal_done = true;
        d.ramp_mi = Some(RAMP_ACCESS_MI - 0.01);
        log.borrow_mut().loops_started.clear();
        for _ in 0..20 {
            d.update_ramp_light(&mut app.ctx, 0.05);
        }
        assert!(
            alert_releases(&log) > 0,
            "{control}: the solid tone was left sounding past the stop bar"
        );
        assert_eq!(alert_holds(&log, "vehicle/bar_solid"), 0, "{control}");
        assert!(!d.bar_solid_on, "{control}");

        // And once the ramp itself is over, still silent.
        d.ramp_mi = None;
        d.update_ramp_light(&mut app.ctx, 0.05);
        assert_eq!(alert_holds(&log, "vehicle/bar_solid"), 0, "{control}");
    }
}

/// Agent drives A, D and E (2026-09-24): after "Light green." the stop-bar
/// ticks and then the held "you must be stopping" tone played all the way to
/// the bar, telling a driver without the assist, by sound, to stop on green.
#[test]
fn test_the_bar_says_nothing_to_stop_for_on_a_green() {
    let approach = |red: bool| {
        let mut app = TestApp::new();
        let mut d = a_drive(&mut app);
        on_ramp(&mut d, "signal", red, 15.0);
        let log = record_audio(&mut app);
        for feet in [250.0, 150.0, 60.0, 20.0] {
            d.ramp_mi = Some(RAMP_ACCESS_MI + feet / 5280.0);
            for _ in 0..30 {
                d.update_ramp_bar_ticks(&mut app.ctx, 0.05);
            }
        }
        let ticks = played_keys(&log)
            .iter()
            .filter(|key| *key == "vehicle/curve_bink")
            .count();
        (ticks, alert_holds(&log, "vehicle/bar_solid"))
    };
    assert_eq!(
        approach(false),
        (0, 0),
        "a green still counted the bar down"
    );
    // The same approach on a red still has the whole instrument.
    let (ticks, holds) = approach(true);
    assert!(ticks > 0 && holds > 0, "red: {ticks} ticks, {holds} holds");
}

/// One frame of the ramp's terminal work, in the frame's own order.
fn terminal_frame(d: &mut DrivingState, app: &mut TestApp, clock: &FakeClock) {
    clock.advance(0.05);
    d.update_ramp_light(&mut app.ctx, 0.05);
    d.update_exit(&mut app.ctx, 0.0, 0.05);
}

/// Agent drive B (2026-09-24): the driver stopped where the held tone began,
/// about fifty feet out, and heard "Stopped short of the stop sign." twice
/// with no distance. The tone is where to be stopped, so a stop inside it is
/// a stop at the sign, said once.
#[test]
fn test_a_stop_where_the_tone_says_is_a_stop_at_the_sign() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.route_transition_assist = false;
    on_ramp(&mut d, "stop", false, 0.0);
    d.cross_bubble = None; // an empty crossroad
    d.ramp_mi = Some(RAMP_ACCESS_MI + 50.0 / 5280.0);
    app.clear_speech();
    for _ in 0..20 {
        terminal_frame(&mut d, &mut app, &clock);
    }
    assert!(!said_any(&app, "short of"), "{:?}", spoken(&app));
    assert_eq!(
        said_count(&app, "Stopped at the sign."),
        1,
        "{:?}",
        spoken(&app)
    );
    assert!(d.ramp_terminal_done);
}

/// Short of the tone it is a stop short, said once for the stop and with its
/// distance, however the crawl bobs across the stopped line on the way down.
#[test]
fn test_stopped_short_is_said_once_with_its_distance() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.route_transition_assist = false;
    on_ramp(&mut d, "stop", false, 0.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 150.0 / 5280.0);
    app.clear_speech();
    for mph in [2.0, 3.5, 2.5, 3.2, 0.5, 0.0, 0.0] {
        d.trip.truck.velocity_mps = mph_to_mps(mph);
        settle(&clock);
        terminal_frame(&mut d, &mut app, &clock);
    }
    assert_eq!(said_count(&app, "short of"), 1, "{:?}", spoken(&app));
    assert!(
        said_any(&app, "Stopped 150 feet short of the stop sign."),
        "{:?}",
        spoken(&app)
    );
}

/// Agent drives D and E (2026-09-24): "Exit speed 49. You take exit 113 ..."
/// then "(interrupting) Exit speed 49 ..." -- the next route line flushed the
/// take line and had it said again from the top; and "Light red." cut it off
/// instead. The terminal's callout waits behind the take line.
#[test]
fn test_the_take_line_is_heard_once_with_the_terminal_behind_it() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = ready_to_exit(&mut app, 45.0);
    take_the_exit(&mut d, &mut app, "signal", None);
    clock.advance(0.6);
    d.announce_ramp_terminal(&mut app.ctx);
    let lines = spoken(&app);
    let take = lines
        .iter()
        .position(|line| line.contains(" of ramp"))
        .expect("the take line");
    let light = lines
        .iter()
        .position(|line| line.starts_with("Light "))
        .expect("the terminal callout");
    assert_eq!(said_count(&app, " of ramp"), 1, "{lines:?}");
    assert!(take < light, "{lines:?}");
    assert!(
        app.ctx.handed_back_count(" of ramp") == 0,
        "the take line was cut and replayed: {lines:?}"
    );
}

fn a_limit_change(text: &str) -> TripEvent {
    TripEvent {
        kind: TripEventKind::GpsCue,
        message: SpokenMessage::new(text),
        data: TripEventData {
            limit_change: Some(true),
            ..Default::default()
        },
    }
}

/// Agent drive, Silverthorne to Edwards (2026-09-24): "Speed limit raised to
/// 75." before AND after "Exit speed 52. You take exit 167" -- a limit on the
/// mainline the truck was leaving. At the gore it is not said; said just
/// before the gore and cut by the take line, it is not handed back.
#[test]
fn test_the_mainline_limit_stays_off_the_take_line() {
    let raised = "Speed limit raised to 75.";

    // At the gore of an exit being taken: nothing to say about the mainline.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 45.0);
    let stop = a_stop(d.trip.position_mi);
    bake_ramp_control(&mut d, stop.at_mi, "signal");
    d.exit_stop = Some(stop.clone());
    d.exit_signal_on = true;
    d.exit_lane_entered = true;
    d.lane.lane = 0;
    d.trip.position_mi = stop.at_mi;
    d.handle_trip_event(&mut app.ctx, &a_limit_change(raised));
    assert_eq!(said_count(&app, raised), 0, "{:?}", spoken(&app));
    drop(d);
    drop(app);

    // Just before it: heard, then cut by the take line, and gone.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = ready_to_exit(&mut app, 45.0);
    let stop = a_stop(d.trip.position_mi + 0.1);
    d.handle_trip_event(&mut app.ctx, &a_limit_change(raised));
    clock.advance(0.3);
    take_the_exit(&mut d, &mut app, "signal", Some(stop));
    assert_eq!(said_count(&app, raised), 1, "{:?}", spoken(&app));
    assert!(said_any(&app, " of ramp"), "{:?}", spoken(&app));
}

/// Agent drive B (2026-09-24): a billboard read 0.3 miles before the gore,
/// over the countdown. Roadside colour waits out the last mile of an exit
/// being taken.
#[test]
fn test_roadside_chatter_waits_out_the_exit_approach() {
    let billboard = TripEvent {
        kind: TripEventKind::Billboard,
        message: SpokenMessage::new("The Thing? Mystery of the desert, next exit."),
        data: TripEventData::default(),
    };
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = ready_to_exit(&mut app, 55.0);
    d.exit_stop = Some(a_stop(d.trip.position_mi + 0.3));
    d.exit_signal_on = true;
    d.exit_signal_canceled = false;
    d.handle_trip_event(&mut app.ctx, &billboard);
    assert!(!said_any(&app, "The Thing"), "{:?}", spoken(&app));
    // The signal cancelled: the exit is not being taken, and the road talks.
    d.exit_signal_canceled = true;
    settle(&clock);
    d.handle_trip_event(&mut app.ctx, &billboard);
    assert!(said_any(&app, "The Thing"), "{:?}", spoken(&app));
}

/// A car standing in the crossroad: no gap until it is gone.
fn a_car_in_the_crossroad() -> CrossTraffic {
    let mut bubble = CrossTraffic::new(1, "yield", false);
    bubble.vehicles = vec![CrossVehicle {
        position_mi: 0.0,
        speed_mph: 0.0,
        target_mph: 0.0,
        vehicle_class: "car",
        length_mi: 15.0 / 5280.0,
        from_side: "left",
        crossed: false,
        committed: false,
        sound_started: false,
        id: 1,
    }];
    bubble
}

/// Agent drive, yield at exit 255 (2026-09-24): the hold line was replayed
/// 0.35 s before "Gap in traffic. Clear; pull ahead" -- the gap had come,
/// and the hold line said the opposite.
#[test]
fn test_the_hold_line_is_not_replayed_once_the_gap_comes() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.route_transition_assist = true;
    on_ramp(&mut d, "yield", false, 0.0);
    d.cross_bubble = Some(a_car_in_the_crossroad());
    app.clear_speech();
    d.update_ramp_terminal_assist(&mut app.ctx);
    assert!(said_any(&app, "At the yield. A car"), "{:?}", spoken(&app));
    clock.advance(0.3);
    if let Some(bubble) = d.cross_bubble.as_mut() {
        bubble.vehicles.clear(); // the car is through
    }
    d.update_ramp_terminal_assist(&mut app.ctx);
    let lines = spoken(&app);
    assert_eq!(said_count(&app, "At the yield. A car"), 1, "{lines:?}");
    assert!(
        lines
            .last()
            .is_some_and(|line| line.starts_with("Gap in traffic.")),
        "{lines:?}"
    );
}

/// Agent drive, yield at exit 255 (2026-09-24): "Stopped at the yield. A car
/// crossing from the left" was said at 4 miles per hour while the car and
/// pickup holding the truck crossed in the right ear. The wait names the
/// vehicle it is waiting on, and a yield is not called a stop.
#[test]
fn test_the_yield_wait_names_the_car_it_is_waiting_on() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.route_transition_assist = true;
    on_ramp(&mut d, "yield", false, 0.0);
    let mut bubble = a_car_in_the_crossroad();
    let in_the_crossroad = &mut bubble.vehicles[0];
    in_the_crossroad.from_side = "right";
    in_the_crossroad.position_mi = 5.0 / 5280.0; // past the centre, still in it
    in_the_crossroad.speed_mph = 4.0; // creeping through: there when the truck would be
    in_the_crossroad.sound_started = true; // already heard, in the right ear
    let mut coming = in_the_crossroad.clone();
    coming.from_side = "left";
    coming.position_mi = -0.03; // about three and a half seconds out
    coming.speed_mph = 30.0;
    coming.sound_started = false;
    bubble.vehicles.push(coming);
    d.cross_bubble = Some(bubble);
    app.clear_speech();
    d.update_ramp_terminal_assist(&mut app.ctx);
    let lines = spoken(&app);
    assert!(
        lines
            .iter()
            .any(|line| line == "At the yield. A car crossing from the right."),
        "{lines:?}"
    );
}

/// Same drive: a slowing line then a braking-for-the-yield line a second
/// later -- one assist, one act, two lines. The lift names the yield and the
/// terminal's braking is silent.
#[test]
fn test_one_assist_line_for_the_yield_approach() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.route_transition_assist = true;
    on_ramp(&mut d, "yield", false, 55.0);
    d.cross_bubble = Some(a_car_in_the_crossroad());
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.1);
    app.clear_speech();
    for _ in 0..10 {
        clock.advance(0.05);
        d.update_lane(&mut app.ctx, 0.05);
        d.update_ramp_terminal_assist(&mut app.ctx);
    }
    assert!(d.ramp_assist_brake > 0.0, "the terminal is braking");
    let assist: Vec<String> = spoken(&app)
        .into_iter()
        .filter(|line| line.starts_with("Slowing") || line.starts_with("Braking"))
        .collect();
    assert_eq!(assist, vec!["Slowing for the yield.".to_string()]);
}

#[test]
fn test_a_held_alert_tone_stops_when_nobody_is_holding_it() {
    // The tone is a dead man's switch at the audio layer too: whatever else
    // goes wrong -- a menu taking the frame, a state ending mid-alert -- a
    // continuous tone in a player's headphones lapses on its own.
    let (mut audio, log) = recording_engine();

    audio.hold_alert_with("vehicle/bar_solid", 0.85, 60);
    assert_eq!(
        log.borrow().loops_started,
        vec![(CH_ALERT, "vehicle/bar_solid".to_string())]
    );

    // Re-asserted every frame, it holds.
    for _ in 0..20 {
        audio.hold_alert_with("vehicle/bar_solid", 0.85, 60);
        audio.update(0.05);
    }
    assert_eq!(alert_releases(&log), 0);

    // The holder stops calling: the tone goes quiet on its own, promptly.
    let mut elapsed = 0.0;
    while alert_releases(&log) == 0 && elapsed < 5.0 {
        audio.update(0.05);
        elapsed += 0.05;
    }
    assert_eq!(alert_releases(&log), 1);
    assert!(elapsed <= ALERT_HOLD_TIMEOUT_S + 0.1, "{elapsed}");

    // Silent stays silent: no repeat stops, and no tone the player never
    // asked for coming back.
    for _ in 0..20 {
        audio.update(0.05);
    }
    assert_eq!(alert_releases(&log), 1);
}

// -- the clock ------------------------------------------------------------------------

#[test]
fn test_controlled_ramp_pins_the_clock_to_real_time() {
    // Under speed-based compression a hot ramp entry burned the whole half
    // mile in a few real seconds (log receipt: exit 17:00:13, sign blown
    // 17:00:18). From the gore of a controlled ramp the clock runs real, so
    // the warning buys human reaction seconds.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.time_scale = 10.0;
    d.trip.truck.velocity_mps = mph_to_mps(45.0); // hot entry
    assert!(d.trip.effective_time_scale() > 8.0);

    d.trip.controlled_ramp = true;
    assert_eq!(d.trip.effective_time_scale(), 1.0);

    d.trip.controlled_ramp = false;
    assert!(d.trip.effective_time_scale() > 8.0);
}

#[test]
fn test_update_exit_maintains_the_controlled_ramp_flag() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "stop", false, 40.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.3);
    d.ramp_stop = Some(a_stop(30.0));

    d.update_exit(&mut app.ctx, 0.0, 0.0);
    assert!(d.trip.controlled_ramp);

    // Past the terminal the clock may compress again.
    d.ramp_terminal_done = true;
    d.update_exit(&mut app.ctx, 0.0, 0.0);
    assert!(!d.trip.controlled_ramp);

    // A free-flow ramp never pins the clock.
    d.ramp_terminal_done = false;
    d.ramp_control = "none".to_string();
    d.update_exit(&mut app.ctx, 0.0, 0.0);
    assert!(!d.trip.controlled_ramp);
}

#[test]
fn test_hairpin_approach_pins_the_clock_to_real_time() {
    // The pacenote lead is sized in real reaction-plus-braking seconds, but
    // compression spent them in a blink: "Hairpin right, a quarter mile" did
    // not finish speaking before the braking point (owner, 2026-07-24).
    // Inside a sharp bend's warning window the clock runs real, and it
    // releases once the curve is behind the truck.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.time_scale = 10.0;
    d.trip.truck.velocity_mps = mph_to_mps(55.0);
    assert!(d.trip.effective_time_scale() > 8.0);

    let mile = d.trip.position_mi;
    d.trip.curves = vec![RouteCurve {
        start_mi: mile + 0.3,
        apex_mi: mile + 0.35,
        end_mi: mile + 0.4,
        direction: 'R',
        advisory_mph: 25,
        min_radius_ft: 120,
        deflection_deg: 150.0,
        connector: false,
    }];
    assert_eq!(d.trip.effective_time_scale(), 1.0);

    // Slow enough for the bend already: no pacenote, no decompression.
    d.trip.truck.velocity_mps = mph_to_mps(20.0);
    assert!(d.trip.effective_time_scale() > 1.0);

    // Curve behind the truck: full compression returns.
    d.trip.truck.velocity_mps = mph_to_mps(55.0);
    d.trip.position_mi = mile + 0.5;
    assert!(d.trip.effective_time_scale() > 8.0);
}

// -- what the ramp's ending is called, before the ramp --------------------------------

#[test]
fn test_signal_on_names_the_ramp_ending() {
    // Owner playtest 2026-07-16: the stop sign was announced only on the
    // ramp, far too late to brake for. The signal-on announcement names the
    // ending while there is still a mile of mainline to plan on.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let at_mi = d.trip.position_mi + 1.2;
    bake_ramp_control(&mut d, at_mi, "stop");
    let mut stop = a_stop(at_mi);
    stop.exit_label = String::new();
    d.exit_stop = Some(stop);
    d.exit_signal_on = false;
    app.clear_speech();

    d.toggle_exit_signal(&mut app.ctx);

    assert!(d.exit_signal_on);
    let last = spoken(&app).last().cloned().unwrap_or_default();
    assert!(last.contains("Ramp ends at a stop sign."), "{last}");
}

#[test]
fn test_upcoming_readout_names_the_ramp_ending() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let at_mi = d.trip.position_mi + 5.0;
    bake_ramp_control(&mut d, at_mi, "signal");
    // `trip.upcoming_stop = lambda within_mi: stop` done for real: the stop
    // is the nearest one ahead, so the readout finds it the way the drive
    // would.
    d.trip.stops = vec![a_stop(at_mi)];
    app.clear_speech();

    d.speak_upcoming(&mut app.ctx, 15.0);

    assert!(
        spoken(&app).iter().any(
            |line| line.contains("Test Plaza") && line.contains("ramp ends at a traffic light")
        ),
        "{:?}",
        spoken(&app)
    );
}

#[test]
fn test_canceling_the_plan_gives_the_road_back() {
    // Shane P, 2026-08-21: plan a stop, cancel it, and the drive stays slow
    // all the way to the exit you just gave up on.
    //
    // The clock drops out of compression while the truck is approaching an
    // exit it has signalled for, so the approach is driven in real time and
    // the braking is winnable. Canceling clears both the active stop and its
    // approach immediately, so the highway gets its pace back at once.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let mut stop = a_stop(d.trip.position_mi + 4.0);
    stop.exit_label = String::new();
    d.exit_stop = Some(stop.clone());
    d.exit_signal_on = false;

    d.toggle_exit_signal(&mut app.ctx);
    assert!(d.exit_signal_on);
    d.update_exit(&mut app.ctx, 0.0, 0.0);
    let approach = d.trip.exit_approach_mi.expect("a live approach");
    assert!((approach - 4.0).abs() < 0.01, "{approach}");

    // Second press: the plan is off, and the exit is still ahead.
    d.toggle_exit_signal(&mut app.ctx);
    assert!(!d.exit_signal_on);
    assert!(d.exit_stop.is_none());
    assert!(d.trip.exit_approach_mi.is_none());
    d.update_exit(&mut app.ctx, 0.0, 0.0);
    assert!(d.trip.exit_approach_mi.is_none());
}

// -- what the ramp's coaching is allowed to wait behind --------------------------------

#[test]
fn test_the_ramp_coaching_outranks_chatter() {
    // The lines that get you to the bar must not wait behind the road.
    //
    // Every one of them defaulted to AMBIENT, which waits the full stale
    // budget behind whatever is speaking. On a real ramp the pacer dropped the
    // assist's own "braking for the light" sixteen milliseconds after the
    // yellow call, and "through on the yellow" behind that -- so the truck
    // braked for the light and the driver was told none of it (owner playtest,
    // 2026-08-15).
    //
    // Python read the priority straight off the patched `say_event` keyword.
    // The capture here sits below the pacer, so the same fact is asserted the
    // way a player meets it: the channel is backed up with CRITICAL traffic
    // first, and an AMBIENT line would be dropped as stale where a ROUTE line
    // is not.
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true; // the dedicated event channel the pacer paces
    let mut d = a_drive(&mut app);
    app.ctx.settings.route_transition_assist = true;

    // The light changing under the driver, and the assist acting on it.
    on_ramp(&mut d, "signal", true, 35.0);
    d.ramp_light_last_phase = "green".to_string();
    for _ in 0..5 {
        app.ctx.event_pacer.note_queued(
            "Brake lights right ahead.",
            EventPriority::Critical,
            hearken::speech::Protect::Yes,
            None,
        );
    }
    app.clear_speech();

    d.update_ramp_light(&mut app.ctx, 0.1);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.08);
    d.trip.truck.brake = 0.0;
    d.trip.truck.throttle = 0.5;
    d.update_ramp_terminal_assist(&mut app.ctx);

    assert!(
        spoken(&app).iter().any(|line| line == "Light red."),
        "{:?}",
        spoken(&app)
    );
    assert!(
        said_any(&app, "Braking for the light."),
        "{:?}",
        spoken(&app)
    );
}

#[test]
fn test_being_stranded_short_of_the_bar_is_never_dropped_as_chatter() {
    // Owner playtest, 2026-08-17: stopped 1,350 feet short through a whole
    // green-yellow-red cycle with nothing said.
    //
    // The game produced exactly the right line -- "Drive up and stop at the
    // bar; the red is the time to close the gap" -- and the pacer dropped it
    // as stale ambient. It is not chatter: it is an instruction about a
    // STANDING condition, and the truck stays stopped until the driver acts,
    // so the staleness rule was reading a moment that had not passed.
    const LINE: &str = "You are stopped about 1,350 feet short of the light. Drive up and stop \
                        at the bar; the red is the time to close the gap.";
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true;
    // Back the channel up the way a busy ramp approach does.
    for _ in 0..5 {
        app.ctx.event_pacer.note_queued(
            "Brake lights right ahead.",
            EventPriority::Critical,
            hearken::speech::Protect::Yes,
            None,
        );
    }
    app.clear_speech();

    app.ctx.say_event_with(
        LINE,
        freight_fate::app::SayEvent::queued().priority(EventPriority::Important),
    );

    assert!(
        !app.event_lines().is_empty(),
        "a standing instruction must survive a backed-up channel"
    );
}

#[test]
fn test_the_stranded_prompts_ask_for_route_priority() {
    // Pinned at the call site too. Python read the source for
    // `EventPriority.ROUTE`; here the prompt itself is driven through a
    // backed-up channel, which is what that priority buys -- an AMBIENT line
    // is the one the stale-drop branch throws away.
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true;
    let mut d = a_drive(&mut app);
    on_ramp(&mut d, "signal", true, 0.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.15);
    for _ in 0..5 {
        app.ctx.event_pacer.note_queued(
            "Brake lights right ahead.",
            EventPriority::Critical,
            hearken::speech::Protect::Yes,
            None,
        );
    }
    app.clear_speech();

    d.update_ramp_queue_guidance(&mut app.ctx);

    assert!(
        app.event_lines()
            .iter()
            .any(|line| line.contains("short of the light")),
        "the stranded prompt was dropped as chatter: {:?}",
        app.event_lines()
    );
}

// -- the speed-control session across a ramp terminal ----------------------------------

/// The first mile of the corridor that is genuinely OPEN ROAD: a posted limit
/// with no zone reason on it.
///
/// Python patched `speed_limit_at` to a flat `(65.0, None)`. The reason half
/// is what these cases actually depend on -- a reason makes the resume reach
/// for the speed keeper instead of cruise -- so the road is searched for a
/// mile that really answers that way, and the search panics rather than
/// quietly running the cases somewhere they mean something else.
fn open_road_mile(d: &mut DrivingState) -> f64 {
    let total = d.trip.total_miles();
    let mut mile = 5.0;
    while mile < total - 5.0 {
        let (limit, reason) = d.trip.speed_limit_at(mile);
        if reason.is_none() && limit >= 45.0 {
            return mile;
        }
        mile += 0.5;
    }
    panic!("this corridor has no open-road mile for the cruise cases to sit on");
}

/// `trip.speed_limit_at = lambda mile: (25.0, "facility access road")` done
/// for real: a `Zone` over the stretch the truck is on.
///
/// A zone is the ONLY way `speed_limit_at` returns a reason at all, and the
/// reason is what makes the resume reach for the speed keeper rather than
/// cruise -- so the keeper case needs one. The Buffalo corridor carries no
/// posted zone of its own (searched at quarter-mile steps end to end), so one
/// is placed rather than the case being moved somewhere it would mean
/// something else.
const ACCESS_ZONE_MPH: f64 = 25.0;

fn a_facility_access_zone(d: &mut DrivingState, around_mi: f64) {
    d.trip.zones.push(Zone::new(
        around_mi - 1.0,
        around_mi + 1.0,
        ACCESS_ZONE_MPH,
        "facility access road",
    ));
    let (limit, reason) = d.trip.speed_limit_at(around_mi);
    assert_eq!(limit, ACCESS_ZONE_MPH);
    assert_eq!(reason.as_deref(), Some("facility access road"));
}

/// `_ready_to_exit(app, ...)`: a drive with an armed speed-control session,
/// right at its exit.
fn ready_to_exit(app: &mut TestApp, mph: f64) -> DrivingState {
    let mut d = a_drive(app);
    app.ctx.settings.route_transition_assist = true;
    app.ctx.settings.speed_keeper = true;
    d.trip.position_mi = open_road_mile(&mut d);
    d.trip.truck.set_air_ready(false);
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = mph_to_mps(mph);
    app.clear_speech();
    d
}

/// `_take_the_exit(d, control, stop=...)`: drive the real exit-take path onto
/// a ramp ending in `control`.
fn take_the_exit(
    d: &mut DrivingState,
    app: &mut TestApp,
    control: &str,
    stop: Option<RoadStop>,
) -> RoadStop {
    let stop = stop.unwrap_or_else(|| {
        let mut stop = a_stop(d.trip.position_mi);
        stop.actions = vec!["rest".to_string()];
        stop.exit_label = String::new();
        stop
    });
    bake_ramp_control(d, stop.at_mi, control);
    d.exit_stop = Some(stop.clone());
    d.exit_signal_on = true;
    d.exit_signal_canceled = false;
    d.exit_lane_entered = true;
    d.lane.lane = 0;
    d.trip.position_mi = stop.at_mi;
    d.update_exit(&mut app.ctx, 0.0, 0.0);
    stop
}

/// `_honor_the_bar_and_drive_on(d)`: stop at the bar, pull away from it, and
/// leave the ramp behind.
fn honor_the_bar_and_drive_on(d: &mut DrivingState, app: &mut TestApp) {
    d.trip.truck.velocity_mps = 0.0;
    d.ramp_mi = Some(RAMP_ACCESS_MI);
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    d.trip.truck.brake = 0.0;
    d.trip.truck.velocity_mps = 3.0; // rolling to the entrance, still on the ramp
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    d.ramp_mi = None; // the ramp is behind the truck
}

#[test]
fn test_ramp_terminal_hands_adaptive_cruise_back_after_the_stop_bar() {
    // Shane, 2026-08-15: taking an exit killed adaptive cruise and the speed
    // keeper for the rest of the run, and only the resume key brought them
    // back.
    //
    // The bar is a transit stop, not an arrival: honor it, drive on, and
    // automatic speed control is simply there again.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);

    take_the_exit(&mut d, &mut app, "stop", None);
    assert!(d.ramp_mi.is_some(), "the ramp really was taken");
    // The ramp takes the pedals back, but not the session.
    assert!(d.cruise_mph.is_none());
    assert!(d.speed_control_armed);
    assert!(d.speed_control_paused_at_stop);

    // Route-transition assistance brakes for the sign.
    d.ramp_light_announced = true;
    d.ramp_mi = Some(0.22);
    d.update_ramp_terminal_assist(&mut app.ctx);
    assert!(d.ramp_assist_said);
    assert!(d.speed_control_paused_at_stop);

    app.clear_speech();
    honor_the_bar_and_drive_on(&mut d, &mut app);
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);

    // No key was pressed anywhere in that sequence.
    assert_eq!(d.cruise_mph, Some(40.0));
    assert!(!d.speed_control_paused_at_stop);
    // The existing resume line, once, and no new line about the pause.
    assert_eq!(said_count(&app, "Cruise resuming"), 1);
    assert!(
        !spoken(&app)
            .iter()
            .any(|line| line.to_lowercase().contains("paus")),
        "{:?}",
        spoken(&app)
    );
}

#[test]
fn test_ramp_terminal_hands_the_speed_keeper_back_after_the_stop_bar() {
    // The same for the keeper: it dies with cruise and must come back with it.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 25.0);
    // The keeper is the controller a posted ZONE calls for, so the truck sits
    // in a real one rather than behind a patched limit.
    let mile = d.trip.position_mi;
    a_facility_access_zone(&mut d, mile);
    d.trip.truck.velocity_mps = mph_to_mps(ACCESS_ZONE_MPH);
    d.engage_keeper(
        &mut app.ctx,
        ACCESS_ZONE_MPH,
        "facility access road",
        None,
        true,
    );
    assert_eq!(d.keeper_mph, Some(ACCESS_ZONE_MPH));

    take_the_exit(&mut d, &mut app, "stop", None);
    assert!(d.ramp_mi.is_some());
    assert!(d.keeper_mph.is_none());
    assert!(d.speed_control_armed);

    app.clear_speech();
    honor_the_bar_and_drive_on(&mut d, &mut app);
    d.trip.truck.velocity_mps = mph_to_mps(15.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);

    assert_eq!(d.keeper_mph, Some(ACCESS_ZONE_MPH));
    assert!(!d.speed_control_paused_at_stop);
    assert_eq!(said_count(&app, "Speed keeper holding"), 1);
}

#[test]
fn test_speed_control_stays_off_on_the_creep_to_the_stop_bar() {
    // The trap the pause exists for: nothing re-engages while the truck is
    // still slowing toward the bar, or rolling the last of the ramp to the
    // entrance behind it.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);
    take_the_exit(&mut d, &mut app, "stop", None);

    d.ramp_mi = Some(0.2);
    for mph in [35.0, 20.0, 10.0, 2.0, 0.0] {
        d.trip.truck.velocity_mps = mph_to_mps(mph);
        d.resume_speed_control_if_ready(&mut app.ctx, false);
        assert!(d.cruise_mph.is_none(), "at {mph} mph");
        assert!(d.keeper_mph.is_none(), "at {mph} mph");
    }

    // Stopped, then rolling again -- but the entrance is still ahead.
    d.trip.truck.brake = 0.0;
    d.ramp_mi = Some(0.08);
    d.trip.truck.velocity_mps = mph_to_mps(20.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert!(d.cruise_mph.is_none());
    assert!(d.keeper_mph.is_none());
}

#[test]
fn test_an_arrival_pause_still_waits_for_departure() {
    // A pickup or delivery gate is an arrival, not a transit stop: it holds
    // the session until the player departs, however long the truck rolls.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);

    // The gate flavour: no resume_when_rolling.
    assert!(d.pause_speed_control(&mut app.ctx, false));
    d.trip.truck.velocity_mps = 0.0;
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    for _ in 0..5 {
        d.resume_speed_control_if_ready(&mut app.ctx, false);
    }
    assert!(d.cruise_mph.is_none());
    assert!(d.speed_control_paused_at_stop);

    // Departing is what lets it back on.
    d.restore_speed_control_session(&mut app.ctx, true, Some(40.0));
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert_eq!(d.cruise_mph, Some(40.0));
}

#[test]
fn test_the_destination_ramp_still_holds_speed_control_to_the_gate() {
    // The regression guard. A destination exit is an arrival: its ramp ends at
    // the facility gate, and cruise winding back up on it is what drove an
    // owner playtest past the terminal at 66 mph.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);
    let mut destination = RoadStop::new(
        "Rochester Freight Market",
        d.trip.position_mi,
        "delivery_destination",
    );
    destination.actions = vec!["deliver".to_string()];
    destination.exit_label = String::new();

    take_the_exit(&mut d, &mut app, "stop", Some(destination));
    assert!(d.speed_control_paused_at_stop);
    assert!(!d.speed_control_transit_pause);

    honor_the_bar_and_drive_on(&mut d, &mut app);
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    for _ in 0..5 {
        d.resume_speed_control_if_ready(&mut app.ctx, false);
    }

    assert!(d.cruise_mph.is_none());
    assert!(d.keeper_mph.is_none());
    assert!(d.speed_control_paused_at_stop);
}

#[test]
fn test_a_green_ramp_light_rolled_through_still_hands_speed_control_back() {
    // No stop was ever required, so nothing can be waiting for one: the ramp
    // falling behind the truck is how a green terminal is honored.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);
    take_the_exit(&mut d, &mut app, "signal", None);
    assert!(d.speed_control_transit_pause);

    // Rolled the whole ramp and through a green: never below a walk.
    d.ramp_mi = Some(0.2);
    d.trip.truck.velocity_mps = mph_to_mps(20.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert!(d.cruise_mph.is_none(), "still on the ramp");
    assert!(!d.speed_control_stop_honored);

    d.ramp_mi = None;
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert_eq!(d.cruise_mph, Some(40.0));
}

#[test]
fn test_a_weigh_station_ramp_hands_speed_control_back_after_check_in() {
    // A scale is a transit stop like any other: pull in, check in, drive on.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);
    let mut scale = RoadStop::new("Ontario Scale", d.trip.position_mi, "weigh_station");
    scale.actions = vec!["inspect".to_string()];
    scale.parking = "none".to_string();
    scale.exit_label = String::new();

    take_the_exit(&mut d, &mut app, "stop", Some(scale));
    assert!(d.speed_control_transit_pause);
    honor_the_bar_and_drive_on(&mut d, &mut app);
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);

    assert_eq!(d.cruise_mph, Some(40.0));
}

#[test]
fn test_a_manual_takeover_on_the_ramp_is_never_undone() {
    // The player's own pedal keeps the resume waiting, and switching speed
    // control off on the ramp keeps it off past the bar.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);
    take_the_exit(&mut d, &mut app, "stop", None);

    // Braking down the ramp and away from the bar: never resumes under the
    // player's own foot.
    d.ramp_mi = None;
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    for _ in 0..5 {
        d.resume_speed_control_if_ready(&mut app.ctx, true);
    }
    assert!(d.cruise_mph.is_none());
    assert!(d.speed_control_paused_at_stop);

    // Switching it off is final: the resume cannot bring back a session the
    // player ended.
    d.toggle_cruise(&mut app.ctx);
    assert!(!d.speed_control_armed);
    for _ in 0..5 {
        d.resume_speed_control_if_ready(&mut app.ctx, false);
    }
    assert!(d.cruise_mph.is_none());
    assert!(d.keeper_mph.is_none());
}

#[test]
fn test_a_stalled_or_backing_truck_at_the_bar_never_resumes() {
    // Speed control needs a running engine and forward motion. `speed_mph` is
    // unsigned, so a truck backing off the bar reads as rolling.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);
    take_the_exit(&mut d, &mut app, "stop", None);
    d.trip.truck.velocity_mps = 0.0;
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    d.ramp_mi = None;

    d.trip.truck.stalled = true;
    d.trip.truck.velocity_mps = mph_to_mps(20.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert!(d.cruise_mph.is_none());

    d.trip.truck.stalled = false;
    d.trip.truck.engine_on = false;
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert!(d.cruise_mph.is_none());

    // Backing away from the bar is not driving on from it.
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = -3.0;
    for _ in 0..5 {
        d.resume_speed_control_if_ready(&mut app.ctx, false);
    }
    assert!(d.cruise_mph.is_none());
    assert!(d.keeper_mph.is_none());

    // Rolling forward again is what finally hands it back.
    d.trip.truck.velocity_mps = mph_to_mps(20.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert_eq!(d.cruise_mph, Some(40.0));
}

#[test]
fn test_reloading_mid_ramp_never_leaves_speed_control_stuck() {
    // A save carries the session, not the ramp, so a reload must not come back
    // holding a pause for a ramp that is no longer there.
    let mut app = TestApp::new();
    let mut d = ready_to_exit(&mut app, 40.0);
    d.engage_cruise(&mut app.ctx, 40.0, false);
    take_the_exit(&mut d, &mut app, "stop", None);
    assert_eq!(
        d.snapshot(&app.ctx)["speed_control_armed"],
        serde_json::Value::Bool(true)
    );

    // What restoring that snapshot does to the session.
    d.ramp_mi = None;
    d.restore_speed_control_session(&mut app.ctx, true, Some(40.0));
    assert!(!d.speed_control_paused_at_stop);
    assert!(!d.speed_control_transit_pause);

    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert_eq!(d.cruise_mph, Some(40.0));
}

// -- the assist driven for real, frame by frame ----------------------------------------

/// `_approaching_a_terminal(app, monkeypatch, control, mph=45)`: a real drive
/// rolling down a ramp toward a red terminal control.
///
/// The Python fixture patched `trip.grade_at` to a flat zero and
/// `trip.traffic_context` to None. Neither has a seam here, so the road that
/// ANSWERS that way is built instead: `bench_road` bakes a flat grade and one
/// posted limit onto a long straight leg, and `quiet` empties the bubble.
fn approaching_a_terminal(name: &str, control: &str, mph: f64) -> PlaytestHarness {
    let mut harness = start_drive(name);
    let control = control.to_string();
    harness.app.ctx.settings.time_scale = 1.0;
    release_keys(&mut harness);
    harness.with_drive(move |d, _| {
        bench_road(d, 65.0, 0.0, 1.0);
        quiet(&mut d.trip);
        d.trip.position_mi = START_MI;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().cargo_kg = 15_000.0;
        d.truck_mut().velocity_mps = mph * MPS_PER_MPH;
        d.ramp_stop = Some(a_stop(d.trip.position_mi + 0.5));
        d.ramp_mi = Some(0.5);
        d.ramp_control = control;
        d.ramp_light_offset_s = 0.0; // red
        d.ramp_light_timer = 0.0;
        d.ramp_light_announced = true;
        d.ramp_light_last_phase = "red".to_string();
        d.ramp_terminal_done = false;
        d.ramp_waiting_at_light = false;
        d.ramp_assist_brake = 0.0;
    });
    harness.clear_speech();
    harness
}

#[test]
fn test_route_transition_assistance_stops_at_the_sign_on_the_air_it_has() {
    // The owner's report: the assist ran the tanks out stopping at a sign.
    //
    // Its floor of a third of the pedal took off far more than its own 0.6
    // m/s2 trigger asked for, so the demand collapsed under the application,
    // the assist let go, the demand climbed back, and round it went -- 276
    // brake applications on one flat approach, 125 psi down to 40, spring
    // brakes on, and the truck stopped in the road short of the bar.
    let mut harness = approaching_a_terminal("Ramps", "stop", 45.0);

    // The air system charges for how far the pedal RISES, so that -- not the
    // number of frames it moved on -- is the cost of the approach. Python
    // counted the rise inside `_consume_brake_air` with a monkeypatch; the
    // pedal is fully observable from out here, so it is sampled at frame
    // boundaries instead, and a release-and-remake cycle still shows up as a
    // fall followed by a fresh rise.
    let mut charged_rise = 0.0;
    let mut previous_pedal = harness.read_drive(|d| d.truck().brake.min(1.0));
    let mut lowest_psi = harness.read_drive(|d| d.truck().air_pressure_psi());
    for _ in 0..(60 * 120) {
        if !harness.has_drive() {
            break;
        }
        frame(&mut harness, DT);
        if !harness.has_drive() {
            break;
        }
        let (pedal, psi, done) = harness.read_drive(|d| {
            (
                d.truck().brake.min(1.0),
                d.truck().air_pressure_psi(),
                d.ramp_terminal_done,
            )
        });
        if pedal > previous_pedal {
            charged_rise += pedal - previous_pedal;
        }
        previous_pedal = pedal;
        lowest_psi = lowest_psi.min(psi);
        if done {
            break;
        }
    }

    harness.read_drive(|d| {
        assert!(d.ramp_terminal_done, "the assist never completed the stop");
        assert!(
            d.truck().speed_mph() <= RED_STOP_MPH,
            "{}",
            d.truck().speed_mph()
        );
        assert!(lowest_psi > 60.0, "{lowest_psi}"); // never even reached the low-air warning
        assert!(!d.truck().spring_brakes_active());
    });
    assert!(
        spoken(&harness.app)
            .iter()
            .any(|line| line == "Stopped at the sign. Clear; pull ahead to the entrance."),
        "{:?}",
        spoken(&harness.app)
    );
    // A pedal that only ever rises toward the bar can cost one full
    // application at most; the old release-and-remake cost thirty.
    assert!(charged_rise <= 4.0, "{charged_rise}");
}

#[test]
fn test_route_transition_assistance_lets_go_when_the_terminal_ends() {
    // The frame holds the pedal at the terminal servo's last press, so a
    // press left behind when the terminal ended -- crossed or run -- held the
    // truck on its brakes for good (merge bench, 2026-09-23).
    let mut harness = approaching_a_terminal("Ramps", "signal", 30.0);
    for _ in 0..(60 * 120) {
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.ramp_assist_brake > 0.0) {
            break;
        }
    }
    assert!(
        harness.read_drive(|d| d.ramp_assist_brake > 0.0),
        "the assist never braked for the red"
    );
    harness.with_drive(|d, _| d.ramp_terminal_done = true);
    for _ in 0..60 {
        frame(&mut harness, DT);
    }
    harness.read_drive(|d| {
        assert_eq!(d.ramp_assist_brake, 0.0);
        assert!(d.truck().brake < 0.05, "held at {:.2}", d.truck().brake);
    });
}

#[test]
fn test_route_transition_assistance_brakes_to_the_sign_without_slamming() {
    // Agent drive into Abilene, 2026-09-23: 25 mph and a thousand feet from
    // a stop sign. The terminal's servo pressed AFTER physics had run and
    // was left out of the frame's assist floor, so the pedal decayed under
    // it every frame: the truck slowed at about 0.43 m/s2 against the 0.6
    // planned, reached the bar at 15 mph with "released" said over it, and
    // slammed the rest. The plan is one steady application to the bar.
    let mut harness = approaching_a_terminal("Ramps", "stop", 25.0);
    harness.with_drive(|d, _| {
        d.ramp_mi = Some(0.19 + RAMP_ACCESS_MI);
        d.ramp_stop = Some(a_stop(d.trip.position_mi + 0.19 + RAMP_ACCESS_MI));
    });
    let mut hardest_mps2: f64 = 0.0;
    let mut previous_mps = harness.read_drive(|d| d.truck().velocity_mps);
    let mut mph_at_60_ft: Option<f64> = None;
    for _ in 0..(60 * 120) {
        frame(&mut harness, DT);
        let (mps, done) = harness.read_drive(|d| (d.truck().velocity_mps, d.ramp_terminal_done));
        let gap_ft = harness.read_drive(|d| (d.ramp_mi.unwrap_or(0.0) - RAMP_ACCESS_MI) * 5280.0);
        if mph_at_60_ft.is_none() && gap_ft <= 60.0 {
            mph_at_60_ft = Some(mps * MPH_PER_MPS);
        }
        // The final hold at a crawl is a parking application, not the stop.
        if previous_mps * MPH_PER_MPS > 5.0 {
            hardest_mps2 = hardest_mps2.max((previous_mps - mps) / DT);
        }
        previous_mps = mps;
        if done {
            break;
        }
    }
    let lines = spoken(&harness.app);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Stopped at the sign")),
        "{lines:?}"
    );
    // On the profile, 60 feet out is about 5 mph; the decaying pedal reached
    // it at 13 and stopped the rest on half a pedal.
    let at_60 = mph_at_60_ft.expect("the truck reached the bar");
    assert!(at_60 < 8.0, "{at_60:.1} mph 60 feet from the bar");
    assert!(
        hardest_mps2 < 1.0,
        "the stop peaked at {hardest_mps2:.2} m/s2: {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line == "Route-transition assistance released."),
        "released while still braking for the sign: {lines:?}"
    );
}

#[test]
fn test_route_transition_assistance_lifts_for_the_ramp_cap_before_the_stop() {
    // The ramp cap is sustained speed control, not the terminal stop. A
    // service-brake floor here held the drums for the whole ramp (and the
    // synthetic ramp curve could add its own 0.35 floor) before the real
    // approach-to-bar brake even began.
    let mut harness = approaching_a_terminal("Ramps", "stop", 55.0);
    harness.with_drive(|d, _| {
        // Leave enough ramp ahead that this observes the cap itself, not the
        // bar approach that correctly uses the service brakes to stop.
        d.ramp_mi = Some(3.0);
        d.ramp_stop = Some(a_stop(d.trip.position_mi + 3.0));
    });
    let mut cap_frames = 0;
    let mut longest_service_hold = 0;
    let mut service_hold = 0;
    let mut lowest_psi = harness.read_drive(|d| d.truck().air_pressure_psi());

    for _ in 0..(60 * 20) {
        frame(&mut harness, DT);
        let (at_cap, terminal_braking, pedal, psi) = harness.read_drive(|d| {
            (
                d.transition_assist_active,
                d.ramp_assist_brake > 0.0,
                d.truck().brake,
                d.truck().air_pressure_psi(),
            )
        });
        if terminal_braking {
            break;
        }
        if at_cap {
            cap_frames += 1;
            if pedal >= 0.3 {
                service_hold += 1;
                longest_service_hold = longest_service_hold.max(service_hold);
            } else {
                service_hold = 0;
            }
            lowest_psi = lowest_psi.min(psi);
        }
    }

    assert!(
        cap_frames >= 60,
        "the ramp cap never had a sustained control window"
    );
    assert!(
        longest_service_hold < 30,
        "the ramp cap held service brake for {longest_service_hold} frames"
    );
    assert!(lowest_psi > 110.0, "{lowest_psi}");
}

#[test]
fn test_route_transition_assistance_does_not_chatter_at_the_ramp_cap() {
    // One threshold decided both ways announced itself over and over.
    let mut harness = approaching_a_terminal("Ramps", "stop", 46.0);

    for _ in 0..(60 * 120) {
        if !harness.has_drive() {
            break;
        }
        frame(&mut harness, DT);
        if !harness.has_drive() || harness.read_drive(|d| d.ramp_terminal_done) {
            break;
        }
    }

    // Not vacuous: the approach really has to have been driven to the sign,
    // or "said it at most once" would pass on a loop that never started.
    assert!(
        harness.has_drive() && harness.read_drive(|d| d.ramp_terminal_done),
        "the approach never reached the sign"
    );
    let released = spoken(&harness.app)
        .iter()
        .filter(|line| *line == "Route-transition assistance released.")
        .count();
    assert!(released <= 1, "{:?}", spoken(&harness.app));
}

#[test]
fn test_route_transition_assistance_brakes_for_a_yellow_it_cannot_beat() {
    // Jessie's Tyler ramp (2026-09-03): rolling a green at 33 mph, "500 feet
    // to the bar", the light turned yellow at roughly 360 feet and red four
    // seconds later, and the truck ran it with the assist on and silent.
    let mut harness = approaching_a_terminal("Ramps", "signal", 33.0);
    harness.with_drive(|d, _| {
        let gap_mi = 600.0 / 5280.0;
        d.ramp_mi = Some(RAMP_ACCESS_MI + gap_mi);
        d.ramp_stop = Some(RoadStop::new(
            "Tyler Cross-Dock",
            d.trip.position_mi + RAMP_ACCESS_MI + gap_mi,
            "delivery_destination",
        ));
        // Green, with five seconds of it left.
        d.ramp_light_offset_s = RAMP_LIGHT_RED_S + RAMP_LIGHT_GREEN_S - 5.0;
        d.ramp_light_last_phase = "green".to_string();
    });

    let mut lowest_gap_ft = f64::MAX;
    for _ in 0..(60 * 90) {
        if !harness.has_drive() {
            break;
        }
        frame(&mut harness, DT);
        if !harness.has_drive() {
            break;
        }
        let (gap_ft, done, waiting) = harness.read_drive(|d| {
            (
                (d.ramp_mi.unwrap_or(0.0) - RAMP_ACCESS_MI) * 5280.0,
                d.ramp_terminal_done,
                d.ramp_waiting_at_light,
            )
        });
        lowest_gap_ft = lowest_gap_ft.min(gap_ft);
        if done || waiting {
            break;
        }
    }

    let lines = spoken(&harness.app);
    assert!(
        lines.iter().any(|line| line == "Braking for the light."),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("ran the red light")),
        "{lines:?}"
    );
    harness.read_drive(|d| {
        assert!(
            d.ramp_waiting_at_light,
            "never stopped at the bar; lowest gap {lowest_gap_ft:.0} ft"
        );
        assert!(
            d.truck().speed_mph() <= RED_STOP_MPH,
            "{}",
            d.truck().speed_mph()
        );
    });
}

#[test]
fn test_route_transition_assistance_brakes_for_a_yellow_on_the_all_assists_preset() {
    // The same approach, driven the way Jessie's log says she drives: the
    // "all assists" preset, facility stopping assistance on, the speed keeper
    // on, and adaptive cruise paused for the exit the way taking it pauses it.
    let mut harness = approaching_a_terminal("Ramps", "signal", 33.0);
    assert!(harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all"));
    harness.app.ctx.settings.destination_approach_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    harness.with_drive(|d, ctx| {
        let gap_mi = 600.0 / 5280.0;
        d.ramp_mi = Some(RAMP_ACCESS_MI + gap_mi);
        d.ramp_stop = Some(RoadStop::new(
            "Tyler Cross-Dock",
            d.trip.position_mi + RAMP_ACCESS_MI + gap_mi,
            "delivery_destination",
        ));
        d.truck_mut().transmission.automatic = true;
        d.engage_cruise(ctx, 47.0, false);
        d.pause_speed_control(ctx, true);
        d.ramp_light_offset_s = RAMP_LIGHT_RED_S + RAMP_LIGHT_GREEN_S - 5.0;
        d.ramp_light_last_phase = "green".to_string();
    });
    harness.clear_speech();

    let mut lowest_gap_ft = f64::MAX;
    let mut trace: Vec<String> = Vec::new();
    let mut n = 0;
    for _ in 0..(60 * 90) {
        if !harness.has_drive() {
            break;
        }
        frame(&mut harness, DT);
        if !harness.has_drive() {
            break;
        }
        let (gap_ft, done, waiting, mph, brake, throttle, phase) = harness.read_drive(|d| {
            (
                (d.ramp_mi.unwrap_or(0.0) - RAMP_ACCESS_MI) * 5280.0,
                d.ramp_terminal_done,
                d.ramp_waiting_at_light,
                d.truck().speed_mph(),
                d.truck().brake,
                d.truck().throttle,
                d.ramp_light_phase(),
            )
        });
        n += 1;
        if n % 30 == 0 {
            trace.push(format!(
                "{phase} gap {gap_ft:.0} ft, {mph:.1} mph, brake {brake:.2}, throttle {throttle:.2}"
            ));
        }
        lowest_gap_ft = lowest_gap_ft.min(gap_ft);
        if done || waiting {
            break;
        }
    }

    let lines = spoken(&harness.app);
    assert!(
        lines.iter().any(|line| line == "Braking for the light."),
        "{lines:?}\n{}",
        trace.join("\n")
    );
    assert!(
        !lines.iter().any(|line| line.contains("ran the red light")),
        "{lines:?}\n{}",
        trace.join("\n")
    );
    harness.read_drive(|d| {
        assert!(
            d.ramp_waiting_at_light,
            "never stopped at the bar; lowest gap {lowest_gap_ft:.0} ft\n{}",
            trace.join("\n")
        );
    });
}

#[test]
fn test_route_transition_assistance_brakes_for_a_late_yellow_on_the_tyler_ramp() {
    // Jessie's run on the real road: Dallas to Tyler Cross-Dock, exit 556,
    // the all-assists preset, adaptive cruise set before the exit, facility
    // stopping assistance and the speed keeper on.
    use freight_fate::playtest::harness::RouteSetup;
    let mut harness = PlaytestHarness::new();
    assert!(harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all"));
    harness.app.ctx.settings.destination_approach_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    let mut route_setup = RouteSetup::seeded(4242)
        .named("Tyler Ramp")
        .destination_location("Tyler Cross-Dock");
    route_setup.tons = 7.0;
    harness.start_route("dallas_tx_us", "tyler_tx_us", route_setup);
    harness.with_drive(|d, ctx| {
        quiet(&mut d.trip);
        d.weather_mut().current = WeatherKind::Rain;
        d.departure_checked = true;
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().rpm = 1500.0;
        d.truck_mut().set_air_ready(false);
    });
    let exit = harness.with_drive(|d, ctx| {
        d.destination_exit_stop(ctx)
            .expect("a delivery always has a destination exit")
    });
    let at = exit.at_mi;
    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = at - 0.05;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 65.0, false);
        d.exit_stop = Some(exit);
        d.exit_lane_entered = true;
        d.exit_signal_on = true;
    });
    let mut forced = false;
    let mut trace: Vec<String> = Vec::new();
    let mut n = 0;
    for _ in 0..(60 * 180) {
        if !harness.has_drive() {
            break;
        }
        frame(&mut harness, DT);
        if !harness.has_drive() {
            break;
        }
        let (ramp_mi, announced, control, done, waiting, mph, brake, throttle, phase, paused) =
            harness.read_drive(|d| {
                (
                    d.ramp_mi,
                    d.ramp_light_announced,
                    d.ramp_control.clone(),
                    d.ramp_terminal_done,
                    d.ramp_waiting_at_light,
                    d.truck().speed_mph(),
                    d.truck().brake,
                    d.truck().throttle,
                    d.ramp_light_phase(),
                    d.speed_control_paused_at_stop,
                )
            });
        let Some(ramp_mi) = ramp_mi else {
            continue;
        };
        let gap_ft = (ramp_mi - RAMP_ACCESS_MI) * 5280.0;
        if !forced && announced && gap_ft <= 600.0 {
            forced = true;
            harness.with_drive(|d, _| {
                d.ramp_control = "signal".to_string();
                let cycle = d.ramp_light_cycle_s();
                d.ramp_light_offset_s =
                    (d.ramp_light_red_s() + d.ramp_light_green_s() - 5.0 - d.ramp_light_timer)
                        .rem_euclid(cycle);
                d.ramp_light_last_phase = "green".to_string();
            });
        }
        n += 1;
        if n % 30 == 0 {
            trace.push(format!(
                "{control} {phase} gap {gap_ft:.0} ft, {mph:.1} mph, brake {brake:.2}, throttle {throttle:.2}, paused {paused}, done {done}"
            ));
        }
        if forced && (done || waiting) {
            break;
        }
    }
    let lines = spoken(&harness.app);
    eprintln!("{}", trace.join("\n"));
    eprintln!("{lines:#?}");
    assert!(forced, "never reached the ramp light");
    // Twice: for the first yellow on the way down, and again for the late
    // yellow after the green stood the assist down. The second take used to
    // be silent. A line a flush cut before its first word is handed straight
    // back to the voice, so the pair it leaves is one occurrence.
    let braking = "Braking for the light.";
    let announced = lines
        .iter()
        .enumerate()
        .filter(|(i, line)| *line == braking && (*i == 0 || lines[i - 1] != braking))
        .count();
    assert_eq!(announced, 2, "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("ran the red light")));
    harness.read_drive(|d| assert!(d.ramp_waiting_at_light, "never stopped at the bar"));
}
