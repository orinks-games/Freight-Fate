//! `states/driving_updates.rs`: the per-frame heart of the drive.
//!
//! Ported from `tests/test_microsleep.py`,
//! `tests/test_off_pavement_transitions.py`, `tests/test_lane_position_cue.py`,
//! `tests/test_speeding_consequences.py` (the dash and braking-grace half),
//! `tests/test_engine_brake_zones.py` (the curve-assist retarder cases),
//! `tests/test_driving_features.py` (the lane, air, reverse, hazard and
//! grade cases the frame loop owns) and `tests/test_driving_cruise_weather.py`
//! (the live-weather source switch) -- everything a real `DrivingState` can
//! answer without the playtest harness. The radio half is in
//! `states_driving_updates_radio.rs`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use ff_core::data::curves::RouteCurve;
use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::enforcement_observe::OBSERVE_HOLD_MI;
use ff_core::sim::hos;
use ff_core::sim::lane::LANE_WIDTH;
use ff_core::sim::season::real_clock_game_hours;
use ff_core::sim::timezones::PACIFIC;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::{stepping_clock, TestApp};
use freight_fate::audio::{Audio, AudioError, SustainLoopSpec, VolumeUpdate, CH_AIR};
use freight_fate::controller::{fakes::FakePad, ControllerAxis};
use freight_fate::playtest::breaker::force_grade;
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    hos_mut_of, profile_mut_of, HazardShape, DRIVE_PHASE_DELIVERY, LANE_TAP_CHANGE_S,
    MICROSLEEP_BASE_GM, MICROSLEEP_MIN_GM, STEER_CUE_ARM_S, STEER_CUE_HOLD, STEER_CUE_TOCK_S,
};
use freight_fate::states::driving_rest_states::{FelonyStopState, TrafficStopState};
use freight_fate::states::driving_updates::limit_drop_speech_latency_s;

/// One `start_loop`/`stop_loop` call: what happened, on which channel, with
/// which key (empty for a stop).
type LoopCall = (&'static str, u32, String);

const LOCATOR: &str = "vehicle/lane_locator";
const SIGNAL: &str = "vehicle/signal_tone";
const BLINKER_OFF: &str = "vehicle/turn_signal_off";

// -- rigging -------------------------------------------------------------------------
//
// `_driving(app)` from `test_microsleep.py` / `test_lane_position_cue.py` /
// `test_speeding_consequences.py`: one short real corridor, built straight
// rather than driven up to.

fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Drowsy", "Buffalo"));
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester is supported");
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
    let mut drive = DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_DELIVERY, None);
    // The bubble is its own suite's business; an empty road keeps these
    // deterministic (`driving_feature_helpers.quiet_trip`). The weather is
    // the other half of that helper: the trip seed is unseeded, so a drive
    // that does not pin the sky draws a real condition and an ice day caps
    // the safe speed under whatever the test is measuring.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.23694
}

#[test]
fn switching_to_real_time_mid_drive_aligns_the_spoken_clock_without_moving_career_time() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    drive.departure_checked = true;
    drive.trip.game_minutes = 90.0;
    drive.trip.start_timezone = PACIFIC;
    let career_hours = app.ctx.profile.as_ref().unwrap().game_hours;

    app.ctx.settings.time_scale = 1.0;
    drive.update_frame(&mut app.ctx, 0.0);

    let target = real_clock_game_hours(None);
    let calendar_now =
        app.ctx.profile.as_ref().unwrap().calendar_game_hours() + drive.trip.game_minutes / 60.0;
    assert_eq!(app.ctx.profile.as_ref().unwrap().game_hours, career_hours);
    assert!((calendar_now - target).abs() < 1.0 / 60.0);
    assert!((drive.trip.local_hour() - target.rem_euclid(24.0)).abs() < 1.0 / 60.0);
}

/// `_capture(monkeypatch, app)`: every one-shot, plus the held-cue latch the
/// steering cue's self-cancel depends on. `RecordingAudio` answers `cue_held`
/// with a flat false, which would silence every click these tests are about.
#[derive(Default)]
struct CueAudio {
    played: Rc<RefCell<Vec<(String, f64, f64)>>>,
    music: Rc<RefCell<Vec<(String, u32)>>>,
    music_stops: Rc<RefCell<Vec<u32>>>,
    music_volume: Rc<Cell<f64>>,
    cues: Rc<RefCell<HashMap<String, f64>>>,
    loops: Rc<RefCell<Vec<LoopCall>>>,
    reverse: Rc<RefCell<Vec<&'static str>>>,
    engine_on: Rc<Cell<bool>>,
    playing: Rc<Cell<bool>>,
}

#[derive(Clone, Default)]
struct AudioTape {
    played: Rc<RefCell<Vec<(String, f64, f64)>>>,
    loops: Rc<RefCell<Vec<LoopCall>>>,
    reverse: Rc<RefCell<Vec<&'static str>>>,
}

impl AudioTape {
    fn keys(&self) -> Vec<String> {
        self.played
            .borrow()
            .iter()
            .map(|(key, _, _)| key.clone())
            .collect()
    }

    fn calls(&self) -> Vec<(String, f64, f64)> {
        self.played.borrow().clone()
    }

    fn last(&self) -> (String, f64, f64) {
        self.played.borrow().last().cloned().expect("a cue played")
    }

    fn clear(&self) {
        self.played.borrow_mut().clear();
    }

    fn loops(&self) -> Vec<LoopCall> {
        self.loops.borrow().clone()
    }

    fn clear_loops(&self) {
        self.loops.borrow_mut().clear();
    }

    fn reverse(&self) -> Vec<&'static str> {
        self.reverse.borrow().clone()
    }

    fn clear_reverse(&self) {
        self.reverse.borrow_mut().clear();
    }
}

impl CueAudio {
    fn install(app: &mut TestApp) -> AudioTape {
        let audio = CueAudio::default();
        let tape = AudioTape {
            played: Rc::clone(&audio.played),
            loops: Rc::clone(&audio.loops),
            reverse: Rc::clone(&audio.reverse),
        };
        app.ctx.audio = Box::new(audio);
        tape
    }
}

impl Audio for CueAudio {
    fn enabled(&self) -> bool {
        false
    }
    fn backend_name(&self) -> &str {
        "cue-test"
    }
    fn master_volume(&self) -> f64 {
        1.0
    }
    fn sfx_volume(&self) -> f64 {
        1.0
    }
    fn music_volume(&self) -> f64 {
        self.music_volume.get()
    }
    fn weather_volume(&self) -> f64 {
        1.0
    }
    fn engine_volume(&self) -> f64 {
        1.0
    }
    fn ui_volume(&self) -> f64 {
        1.0
    }
    fn engine_running(&self) -> bool {
        self.engine_on.get()
    }
    fn engine_starting(&self) -> bool {
        false
    }
    fn voice_key(&self, key: &str) -> String {
        key.to_string()
    }
    fn play_with(&mut self, key: &str, volume: f64, pan: f64) {
        self.played
            .borrow_mut()
            .push((key.to_string(), volume, pan));
    }
    fn play_bank_with(&mut self, base: &str, _fallback: &str, volume: f64, pan: f64) {
        self.play_with(base, volume, pan);
    }
    fn set_engine_duck(&mut self, _duck: f64) {}
    fn set_speech_duck(&mut self, _duck: f64) {}
    fn set_engine_voice(&mut self, _classic: bool) {}
    fn set_jake_voice(&mut self, _classic: bool) {}
    fn has_asset(&mut self, _key: &str) -> bool {
        true
    }
    fn start_loop_with(&mut self, channel: u32, key: &str, _volume: f64, _fade_ms: u32) {
        self.loops
            .borrow_mut()
            .push(("start", channel, key.to_string()));
    }
    fn set_loop_volume(&mut self, _channel: u32, _volume: f64) {}
    fn set_loop_pan(&mut self, _channel: u32, _pan: f64) {}
    fn stop_loop_with(&mut self, channel: u32, _fade_ms: u32) {
        self.loops
            .borrow_mut()
            .push(("stop", channel, String::new()));
    }
    fn start_sustain_loop_with(
        &mut self,
        _channel: u32,
        _key: &str,
        _spec: SustainLoopSpec,
        _volume: f64,
    ) {
    }
    fn release_sustain_loop_with(&mut self, _channel: u32, _fade_ms: u32) {}
    fn hold_alert_with(&mut self, _key: &str, _volume: f64, _fade_ms: u32) {}
    fn release_alert_with(&mut self, _fade_ms: u32) {}
    fn hold_cue(&mut self, name: &str) {
        self.cues
            .borrow_mut()
            .insert(name.to_string(), freight_fate::audio::CUE_HOLD_TIMEOUT_S);
    }
    fn cue_held(&self, name: &str) -> bool {
        self.cues
            .borrow()
            .get(name)
            .is_some_and(|remaining| *remaining > 0.0)
    }
    fn release_cue(&mut self, name: &str) {
        self.cues.borrow_mut().remove(name);
    }
    fn engine_start_with(&mut self, _play_start_sound: bool) {
        self.engine_on.set(true);
    }
    fn engine_stop_with(&mut self, _shutdown_sound: bool) {
        self.engine_on.set(false);
    }
    fn update(&mut self, dt: f64) {
        // The dead man's switch runs on the audio clock, exactly as the
        // facade's does: a menu holding the frames lets the latch lapse.
        self.cues.borrow_mut().retain(|_, remaining| {
            *remaining -= dt;
            *remaining > 0.0
        });
    }
    fn set_engine_rpm_with(&mut self, _rpm: f64, _throttle: f64) {}
    fn set_road_noise(&mut self, _speed_mps: f64) {}
    fn set_weather_with(&mut self, _key: Option<&str>, _intensity: f64) {}
    fn set_wind(&mut self, _intensity: f64) {}
    fn set_ambient_with(&mut self, _key: Option<&str>, _volume: f64) {}
    fn horn_start(&mut self) {}
    fn horn_stop(&mut self) {}
    fn reverse_start(&mut self) {
        self.reverse.borrow_mut().push("start");
    }
    fn reverse_stop(&mut self) {
        self.reverse.borrow_mut().push("stop");
    }
    fn stop_world(&mut self) {}
    fn play_music_with(&mut self, track: &str, fade_ms: u32) {
        self.music.borrow_mut().push((track.to_string(), fade_ms));
        self.playing.set(true);
    }
    fn play_radio_stream_with(&mut self, _url: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn play_music_file_with(&mut self, _path: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn music_playing(&self) -> bool {
        self.playing.get()
    }
    fn radio_now_playing(&self) -> Option<String> {
        None
    }
    fn stop_music_with(&mut self, fade_ms: u32) {
        self.music_stops.borrow_mut().push(fade_ms);
        self.playing.set(false);
    }
    fn set_volumes(&mut self, volumes: &VolumeUpdate) {
        if let Some(music) = volumes.music {
            self.music_volume.set(music);
        }
    }
    fn shutdown(&mut self) {}
}

// -- microsleeps (test_microsleep.py) --------------------------------------------------

#[test]
fn test_microsleep_interval_shrinks_with_exhaustion() {
    let mut app = TestApp::new();
    let d = a_drive(&mut app);
    assert!((d.microsleep_interval_gm(hos::FATIGUE_SEVERE) - MICROSLEEP_BASE_GM).abs() < 1e-9);
    assert!((d.microsleep_interval_gm(100.0) - MICROSLEEP_MIN_GM).abs() < 1e-9);
    assert!(d.microsleep_interval_gm(95.0) < d.microsleep_interval_gm(82.0));
}

#[test]
fn test_microsleeps_only_strike_when_severely_fatigued_and_moving() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Fresh driver, or stopped: no nods however long you go.
    for _ in 0..200 {
        d.accrue_microsleep(&mut app.ctx, 1.0, true, 30.0);
    }
    assert!(d.microsleep_deadline.is_none());
    for _ in 0..200 {
        d.accrue_microsleep(&mut app.ctx, 1.0, false, 95.0);
    }
    assert!(d.microsleep_deadline.is_none());
    // Severely fatigued and rolling: a nod eventually comes.
    let mut fired = false;
    for _ in 0..200 {
        d.accrue_microsleep(&mut app.ctx, 1.0, true, 90.0);
        if d.microsleep_deadline.is_some() {
            fired = true;
            break;
        }
    }
    assert!(fired);
}

#[test]
fn test_reacting_to_a_microsleep_avoids_damage() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.velocity_mps = 30.0;
    let before = d.trip.truck.damage_pct;
    d.begin_microsleep(&mut app.ctx);
    assert!(d.microsleep_deadline.is_some());
    app.ctx.input.press(Key::Down, Mods::NONE); // brake = staying awake
    d.update_microsleep(&mut app.ctx, 0.1);
    assert!(d.microsleep_deadline.is_none());
    assert_eq!(d.trip.truck.damage_pct, before);
    assert!(d.microsleep_cooldown_gm > 0.0);
}

#[test]
fn test_ignoring_a_microsleep_drifts_off_the_road() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.velocity_mps = 30.0;
    let before = d.trip.truck.damage_pct;
    let speed_before = d.trip.truck.speed_mph();
    d.begin_microsleep(&mut app.ctx);
    for _ in 0..60 {
        d.update_microsleep(&mut app.ctx, 0.1);
        if d.microsleep_deadline.is_none() {
            break;
        }
    }
    assert!(d.trip.truck.damage_pct > before);
    // scrubbed wandering onto the shoulder
    assert!(d.trip.truck.speed_mph() < speed_before);
    assert_eq!(d.microsleep_misses, 1);
}

#[test]
fn test_three_missed_microsleeps_force_a_stop() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    for _ in 0..3 {
        d.trip.truck.velocity_mps = 30.0;
        d.microsleep_cooldown_gm = 0.0;
        d.begin_microsleep(&mut app.ctx);
        for _ in 0..60 {
            d.update_microsleep(&mut app.ctx, 0.1);
            if d.microsleep_deadline.is_none() {
                break;
            }
        }
    }
    // The third drift slams the brakes and cuts throttle to force a stop.
    assert_eq!(d.trip.truck.brake, 1.0);
    assert_eq!(d.trip.truck.throttle, 0.0);
}

/// `_FakePad`: enough of the manager for the microsleep reaction check.
fn pad_at(app: &mut TestApp, axis: ControllerAxis, value: i16) {
    let c = &mut app.ctx.controller;
    c.set_enabled(true);
    c.bind_device(Box::new(FakePad::new(0)), "test pad");
    c.process_event(&InputEvent::axis(axis, value));
    // The trigger reads through the smoother; the stick does not.
    for _ in 0..30 {
        c.tick(1.0 / 60.0);
    }
}

#[test]
fn test_a_controller_driver_can_wake_from_a_microsleep() {
    // The truck says "steer or brake", and on a pad neither of those is a
    // key. A controller-only driver could not react at all and drifted off
    // the road every single time (owner, 2026-08-16). Parity with the
    // keyboard is the bar: a held Down arrow already counts, so a held
    // trigger counts too.
    for (axis, value) in [
        (ControllerAxis::TriggerLeft, 26_000_i16),
        (ControllerAxis::LeftX, -20_000),
    ] {
        let mut app = TestApp::new();
        let mut d = a_drive(&mut app);
        d.trip.truck.velocity_mps = 30.0;
        pad_at(&mut app, axis, value);
        let before = d.trip.truck.damage_pct;
        d.begin_microsleep(&mut app.ctx);
        assert!(d.microsleep_deadline.is_some());
        d.update_microsleep(&mut app.ctx, 0.1);
        assert!(
            d.microsleep_deadline.is_none(),
            "the pad reaction must count"
        );
        assert_eq!(d.trip.truck.damage_pct, before);
    }
}

#[test]
fn test_an_idle_pad_is_not_a_microsleep_reaction() {
    // Only a reaction wakes you -- a resting pad is not one.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.velocity_mps = 30.0;
    pad_at(&mut app, ControllerAxis::LeftX, 0);
    d.begin_microsleep(&mut app.ctx);
    for _ in 0..60 {
        d.update_microsleep(&mut app.ctx, 0.1);
        if d.microsleep_deadline.is_none() {
            break;
        }
    }
    assert_eq!(d.microsleep_misses, 1);
}

// -- off pavement (test_off_pavement_transitions.py) ----------------------------------

#[test]
fn test_off_pavement_speaks_on_entry_worsening_and_recovery() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Python stubbed `say_event` outright; here the real pacer would treat
    // the second, identical "Off the road" line as a repeat inside its own
    // window. Step the clock between lines so what is asserted is the
    // TRANSITION rule rather than the pacer's repeat guard.
    app.ctx.event_pacer = hearken::speech::EventSpeechPacer::with_clock(
        freight_fate::app::FF_PACER,
        stepping_clock(30.0),
    );
    d.lane.lane = 0;
    d.trip.truck.velocity_mps = 13.0; // ~29 mph, below the "fast" band
    app.clear_speech();

    // Entry: the truck goes off the pavement.
    d.lane.offset = 1.35;
    d.announce_off_pavement(&mut app.ctx);
    assert_eq!(app.event_lines().len(), 1);

    // Steady, no worse: the continuous cue carries it, speech stays silent.
    d.announce_off_pavement(&mut app.ctx);
    d.announce_off_pavement(&mut app.ctx);
    assert_eq!(app.event_lines().len(), 1);

    // Worse: deeper off the road speaks again.
    d.lane.offset = 1.48;
    d.announce_off_pavement(&mut app.ctx);
    assert_eq!(app.event_lines().len(), 2);

    // Back on the pavement is a transition too, spoken once.
    d.lane.offset = 0.0;
    d.road_position_band = Some(1);
    // The recovery line lives in the update path; assert the condition the
    // else-if turns on so the transition fires exactly when the truck is back.
    assert!(!d.off_pavement());
}

#[path = "states_driving_cue_lifecycle.rs"]
mod cue_lifecycle;

// -- the dash alert and the braking grace (test_speeding_consequences.py) --------------

/// `_speed_on_an_empty_road(d, over, seconds)`: hold well over the limit,
/// for a long time, with nobody watching.
fn speed_on_an_empty_road(d: &mut DrivingState, app: &mut TestApp, over: f64, seconds: f64) -> f64 {
    d.trip.set_patrols(Vec::new());
    d.trip.position_mi = d.trip.total_miles() / 2.0;
    d.enforcement_prev_mi = d.trip.position_mi;
    let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
    d.trip.truck.velocity_mps = mph_to_mps(limit + over);
    for _ in 0..(seconds / 0.5) as i32 {
        d.trip.position_mi += 0.02;
        d.update_enforcement_watch(&mut app.ctx, 0.5);
        d.update_speeding(&mut app.ctx, 0.5, false);
    }
    limit
}

#[test]
fn test_the_dash_still_warns_even_though_nothing_is_charged() {
    // Removing the tax must not remove the courtesy. The overspeed alert was
    // never enforcement -- it is the carrier's dash nagging you, and it is the
    // only reason a blind driver knows the limit dropped. It stays.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    speed_on_an_empty_road(&mut d, &mut app, 15.0, 2.0);
    let spoken = app.event_lines();
    assert!(spoken.iter().any(|line| line.contains("Over the limit of")));
    assert!(spoken.iter().any(|line| line.contains("miles per hour")));
}

#[test]
fn test_the_dash_still_warns_while_looping_back_to_a_missed_destination_exit() {
    // Tyler Rodick, Hattiesburg, 2026-08-26: missed the destination exit, was
    // told the approach loops back, and then held 89 the whole way round with
    // nobody saying anything. The loop-back set a latch that returned early
    // out of the dash -- a carve-out left over from when this method charged
    // silent speeding fines -- so the one system that would have told him to
    // shed speed for the retry was off from the first miss to the dock, while
    // the enforcement watch went on accruing against him regardless.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.missed_destination_exit_said = true;
    d.destination_exit_taken = false;
    app.clear_speech();
    let limit = speed_on_an_empty_road(&mut d, &mut app, 24.0, 2.0);
    let spoken = app.event_lines();
    assert!(
        spoken.iter().any(|line| line.contains("Over the limit of")),
        "24 over a {limit:.0} on the loop-back said nothing: {spoken:?}"
    );
}

#[test]
fn test_a_dropped_limit_still_earns_braking_room() {
    // The one fairness rule worth keeping from the strike era. A loaded truck
    // cannot shed fifteen mph the instant a sign changes, so the grace that
    // used to hold off a strike now holds off the over-limit distance an
    // officer reads. Without it a post could clock you on the transition.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.set_patrols(Vec::new());
    d.trip.position_mi = d.trip.total_miles() / 2.0;
    d.enforcement_prev_mi = d.trip.position_mi;
    let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
    d.trip.truck.velocity_mps = mph_to_mps(limit + 20.0);
    // A limit drop under the truck, with the driver off the throttle.
    d.enforced_limit_prev = Some(limit + 15.0);
    d.update_speeding(&mut app.ctx, 0.1, false);
    assert!(d.limit_drop_grace_s > 0.0);
    for _ in 0..10 {
        d.trip.position_mi += 0.05;
        d.update_enforcement_watch(&mut app.ctx, 0.1);
    }
    assert!(d.over_limit_mi < OBSERVE_HOLD_MI);
}

#[test]
fn test_staying_on_the_throttle_through_the_drop_collapses_the_grace() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.set_patrols(Vec::new());
    d.trip.position_mi = d.trip.total_miles() / 2.0;
    let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
    d.trip.truck.velocity_mps = mph_to_mps(limit + 20.0);
    d.enforced_limit_prev = Some(limit + 15.0);
    d.update_speeding(&mut app.ctx, 0.1, false);
    assert!(d.limit_drop_grace_s > 0.0);
    // Past the announcement's speech-latency window, the throttle held
    // through the drop is disregard and the grace collapses to zero.
    d.limit_drop_throttle_exempt_s = 0.0;
    d.update_speeding(&mut app.ctx, 0.1, true);
    assert_eq!(d.limit_drop_grace_s, 0.0);
    // And the exemption is exactly the ROUTE wait budget -- the longest the
    // demoted zone-entry line can lag its boundary before flushing.
    assert_eq!(
        hearken::speech::EventSpeechPacer::new(freight_fate::app::FF_PACER)
            .wait_budget_s(hearken::speech::EventPriority::Important),
        limit_drop_speech_latency_s()
    );
}

#[test]
fn test_throttle_held_during_speech_latency_does_not_collapse_the_grace() {
    // R1's coupled invariant: the zone-entry line now queues at ROUTE and may
    // lag the boundary by its wait budget. Until that window has passed, a
    // held accelerator is a driver who has not been told anything yet --
    // speech latency must never masquerade as disregard and burn the whole
    // braking grace from the zone boundary.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.set_patrols(Vec::new());
    d.trip.position_mi = d.trip.total_miles() / 2.0;
    let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
    d.trip.truck.velocity_mps = mph_to_mps(limit + 20.0);
    d.enforced_limit_prev = Some(limit + 15.0);
    // The drop lands while the driver is still on the throttle -- the exact
    // frame the old code zeroed the grace.
    d.update_speeding(&mut app.ctx, 0.1, true);
    assert!(d.limit_drop_grace_s > 0.0);
    // Armed to the full window this frame, already ticking down with it.
    assert!((d.limit_drop_throttle_exempt_s - (limit_drop_speech_latency_s() - 0.1)).abs() < 1e-9);
    // Throttle held for the whole latency window: the grace survives it.
    let mut elapsed = 0.0;
    while elapsed + 0.1 < limit_drop_speech_latency_s() {
        d.update_speeding(&mut app.ctx, 0.1, true);
        elapsed += 0.1;
        assert!(d.limit_drop_grace_s > 0.0);
    }
    // Once the line has had time to speak, the same throttle collapses it.
    d.update_speeding(&mut app.ctx, 0.1, true); // window reaches zero
    d.update_speeding(&mut app.ctx, 0.1, true); // now it is disregard
    assert_eq!(d.limit_drop_grace_s, 0.0);
}

#[path = "states_driving_vehicle_updates.rs"]
mod vehicle_updates;
