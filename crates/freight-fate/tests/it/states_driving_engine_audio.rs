//! The continuous soundscape: `update_audio` in
//! `states/driving_updates/engine_audio.rs` -- the engine voice through a
//! shift, the jake growl, the air-fill hiss, the reverse beeper, the
//! cold-start buzzer, and the two-way engine-loop mirror.
//!
//! Ported from the engine-audio block of `tests/test_driving_features.py`
//! (`test_engine_audio_load_eases_without_dropping_out_during_automatic_shift`
//! through `test_reverse_audio_loop_restarts_after_pause_resume` -- the two
//! loop cases inside that run, the reverse beeper and the air fill, already
//! live in `states_driving_updates.rs` and are not repeated here -- plus
//! `test_rest_menu_shutdown_also_stops_engine_audio` and
//! `test_engine_audio_mirror_sync_catches_any_out_of_band_stop`).
//!
//! Python monkeypatched one audio method per case. The equivalent seam here
//! is the audio BACKEND: [`TrackingBackend`] below is a real
//! `AudioBackend` behind a real `AudioEngine`, so every call still goes
//! through the shipped facade -- bank routing, voice keys, the engine
//! model's own book-keeping -- and what is recorded is what the backend was
//! actually asked to do. It also does what the null backend does not:
//! remembers whether the engine loop is running, which is the whole subject
//! of the two mirror cases.

use std::cell::RefCell;
use std::rc::Rc;

use ff_core::sim::transmission::REVERSE;
use ff_core::sim::weather::WeatherKind;

use freight_fate::audio::{
    engine_load_gain, Audio, AudioError, SustainLoopSpec, VolumeUpdate, CH_JAKE,
};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving_core::shut_down_engine;
use freight_fate::states::driving_updates::{
    JAKE_RATE_MAX, SHIFT_END_CLUNK_VOLUME, SHIFT_LOAD_CAP,
};

// -- the recording backend --------------------------------------------------------------

/// What the backend was asked to do, in order.
///
/// Shared with `states_driving_engine_lean.rs`, which watches the two pans
/// through the same recording backend rather than growing a second copy.
#[derive(Default)]
pub(super) struct Calls {
    pub(super) played: Vec<(String, f64)>,
    /// `play_bank(base, volume)`, kept apart from `played` the way Python's
    /// two separate stubs did.
    banks: Vec<(String, f64)>,
    /// `("start", channel, key, volume)` / `("vol", channel, volume)` /
    /// `("stop", channel)`, flattened into one ordered log the way the Python
    /// cases collected them.
    pub(super) loops: Vec<LoopCall>,
    reverse: Vec<&'static str>,
    engine_rpm: Vec<(f64, f64)>,
    pub(super) engine_pan: Vec<f64>,
    /// `set_loop_pan(channel, pan)`: the road bed carries lane position now.
    pub(super) loop_pans: Vec<(u32, f64)>,
    /// `set_engine_duck(duck)`: the shift-gap disengage, in order.
    ducks: Vec<f64>,
    engine_running: bool,
    /// The ignition crank still playing: settable, because that is exactly
    /// what the cold-start buzzer case has to hold the buzzer behind.
    engine_starting: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum LoopCall {
    Start(u32, String, f64),
    Volume(u32, f64),
    Rate(u32, f64),
    Stop(u32),
}

pub(super) type Log = Rc<RefCell<Calls>>;

pub(super) struct TrackingAudio {
    pub(super) log: Log,
}

impl Audio for TrackingAudio {
    fn enabled(&self) -> bool {
        true
    }
    fn backend_name(&self) -> &str {
        "tracking"
    }
    fn master_volume(&self) -> f64 {
        1.0
    }
    fn sfx_volume(&self) -> f64 {
        1.0
    }
    fn music_volume(&self) -> f64 {
        1.0
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
        self.log.borrow().engine_running
    }
    fn engine_starting(&self) -> bool {
        self.log.borrow().engine_starting
    }
    /// No jake A/B re-voicing: the band the drive picked is the subject of
    /// the growl case, and Python's stub saw it unrouted for the same reason.
    fn voice_key(&self, key: &str) -> String {
        key.to_string()
    }
    fn play_with(&mut self, key: &str, volume: f64, _pan: f64) {
        self.log.borrow_mut().played.push((key.to_string(), volume));
    }
    fn play_bank_with(&mut self, base: &str, _fallback: &str, volume: f64, _pan: f64) {
        self.log.borrow_mut().banks.push((base.to_string(), volume));
    }
    fn set_engine_pan(&mut self, pan: f64) {
        self.log.borrow_mut().engine_pan.push(pan);
    }
    fn set_engine_duck(&mut self, duck: f64) {
        self.log.borrow_mut().ducks.push(duck);
    }
    fn set_speech_duck(&mut self, _duck: f64) {}
    fn set_engine_voice(&mut self, _classic: bool) {}
    fn set_jake_voice(&mut self, _classic: bool) {}
    fn has_asset(&mut self, _key: &str) -> bool {
        true
    }
    fn start_loop_with(&mut self, channel: u32, key: &str, volume: f64, _fade_ms: u32) {
        self.log
            .borrow_mut()
            .loops
            .push(LoopCall::Start(channel, key.to_string(), volume));
    }
    fn set_loop_volume(&mut self, channel: u32, volume: f64) {
        self.log
            .borrow_mut()
            .loops
            .push(LoopCall::Volume(channel, volume));
    }
    fn set_loop_pan(&mut self, channel: u32, pan: f64) {
        self.log.borrow_mut().loop_pans.push((channel, pan));
    }
    fn set_loop_rate(&mut self, channel: u32, rate: f64) {
        self.log
            .borrow_mut()
            .loops
            .push(LoopCall::Rate(channel, rate));
    }
    fn stop_loop_with(&mut self, channel: u32, _fade_ms: u32) {
        self.log.borrow_mut().loops.push(LoopCall::Stop(channel));
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
    fn hold_cue(&mut self, _name: &str) {}
    fn cue_held(&self, _name: &str) -> bool {
        false
    }
    fn release_cue(&mut self, _name: &str) {}
    fn engine_start_with(&mut self, _play_start_sound: bool) {
        self.log.borrow_mut().engine_running = true;
    }
    fn engine_stop_with(&mut self, _shutdown_sound: bool) {
        // The shipped facade stops the reverse beeper with the engine loop
        // (`AudioEngine::engine_stop_with`), and `stop_world` -- what a pause
        // calls -- goes through here. A double that skipped it would make the
        // pause case pass or fail on the double's own shape.
        self.reverse_stop();
        self.log.borrow_mut().engine_running = false;
    }
    fn update(&mut self, _dt: f64) {}
    fn set_engine_rpm_with(&mut self, rpm: f64, throttle: f64) {
        self.log.borrow_mut().engine_rpm.push((rpm, throttle));
    }
    fn set_road_noise(&mut self, _speed_mps: f64) {}
    fn set_weather_with(&mut self, _key: Option<&str>, _intensity: f64) {}
    fn set_wind(&mut self, _intensity: f64) {}
    fn set_ambient_with(&mut self, _key: Option<&str>, _volume: f64) {}
    fn horn_start(&mut self) {}
    fn horn_stop(&mut self) {}
    fn reverse_start(&mut self) {
        self.log.borrow_mut().reverse.push("start");
    }
    fn reverse_stop(&mut self) {
        self.log.borrow_mut().reverse.push("stop");
    }
    fn stop_world(&mut self) {
        self.engine_stop_with(false);
    }
    fn play_music_with(&mut self, _track: &str, _fade_ms: u32) {}
    fn play_radio_stream_with(&mut self, _url: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Err(AudioError::new("radio stream unavailable"))
    }
    fn play_music_file_with(&mut self, _path: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Err(AudioError::new("no music files on the bench"))
    }
    fn music_playing(&self) -> bool {
        false
    }
    fn radio_now_playing(&self) -> Option<String> {
        None
    }
    fn stop_music_with(&mut self, _fade_ms: u32) {}
    fn set_volumes(&mut self, _volumes: &VolumeUpdate) {}
    fn shutdown(&mut self) {}
}

// -- rigging -------------------------------------------------------------------------

fn a_drive(name: &str) -> (PlaytestHarness, Log) {
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
    let log: Log = Rc::new(RefCell::new(Calls::default()));
    harness.app.ctx.audio = Box::new(TrackingAudio {
        log: Rc::clone(&log),
    });
    harness.clear_speech();
    (harness, log)
}

/// `driving._update_audio(dt)`.
fn update_audio(harness: &mut PlaytestHarness, dt: f64) {
    harness.with_drive(move |drive, ctx| drive.update_audio(ctx, dt));
}

fn rpm_samples(log: &Log) -> Vec<(f64, f64)> {
    log.borrow().engine_rpm.clone()
}

fn last_rpm(log: &Log) -> (f64, f64) {
    *rpm_samples(log).last().expect("the engine voice was set")
}

fn loops(log: &Log) -> Vec<LoopCall> {
    log.borrow().loops.clone()
}

fn played_keys(log: &Log) -> Vec<String> {
    log.borrow()
        .played
        .iter()
        .map(|(key, _)| key.clone())
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// -- the engine voice through a shift ------------------------------------------------

#[test]
fn test_shift_gap_bed_stays_within_earshot_of_the_coasting_bed() {
    // A torque interrupt is an UNLOADED engine, not a silent one. The
    // game's own off-throttle bed is engine_load_gain(0.0); the shift gap
    // (load cap times the disengage duck) may sit a little under it -- the
    // fuel is off and the note falls -- but not a cliff below it. At 0.35
    // the gap sat 7.5 dB under coasting and 11 dB under full load, and on a
    // one-second downshift that is "the engine cuts out when it shifts"
    // (testers, 2026-09-03).
    let (mut harness, log) = a_drive("Shift Gap Level");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.throttle = 1.0;
        truck.rpm = 1700.0;
        truck.transmission.automatic = true;
        truck.transmission.gear = 6;
        truck.transmission.shift_timer = 0.4; // mid-interrupt
    });
    log.borrow_mut().ducks.clear();
    log.borrow_mut().engine_rpm.clear();

    update_audio(&mut harness, 0.0);

    let duck = *log.borrow().ducks.last().expect("the duck was set");
    let (_, load) = last_rpm(&log);
    let gap_bed = engine_load_gain(load) * duck;
    let coasting_bed = engine_load_gain(0.0);
    let db_under_coasting = 20.0 * (gap_bed / coasting_bed).log10();
    assert!(
        db_under_coasting <= 0.0,
        "the gap must still read as unloaded, not louder than coasting: {db_under_coasting:.1} dB"
    );
    assert!(
        db_under_coasting >= -3.0,
        "the shift gap is a cliff: {db_under_coasting:.1} dB under the coasting bed (duck {duck}, load {load})"
    );
}

#[test]
fn test_engine_audio_load_eases_without_dropping_out_during_automatic_shift() {
    let (mut harness, log) = a_drive("Shift Load");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.throttle = 1.0;
        truck.rpm = 1700.0;
        truck.transmission.automatic = true;
        truck.transmission.gear = 4;
        truck.transmission.shift_timer = 0.5;
    });
    log.borrow_mut().engine_rpm.clear();

    update_audio(&mut harness, 0.0);

    let (rpm, load) = last_rpm(&log);
    assert_eq!(rpm, 1700.0);
    assert!(close(load, 0.45), "{load}");
    assert!(engine_load_gain(load) >= 0.75);
}

#[test]
fn test_auto_shift_voice_sighs_with_physics_and_clunks_on_engagement() {
    // A real AMT shift is kachunk -- sigh -- kachunk: the ducked voice
    // follows the physics rpm falling toward the new gear through the
    // interrupt (never a frozen hang), and the moment the gear takes plays
    // its own soft clunk (the engagement used to be silent).
    let (mut harness, log) = a_drive("Shift Voice");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.transmission.automatic = true;
        truck.transmission.gear = 4;
        truck.rpm = 1400.0;
        truck.transmission.shift_timer = 0.5; // mid-shift
    });
    log.borrow_mut().banks.clear();

    update_audio(&mut harness, 0.0);
    harness.with_drive(|drive, _| drive.truck_mut().rpm = 1150.0); // sighs down
    update_audio(&mut harness, 0.0);
    assert_eq!(last_rpm(&log).0, 1150.0, "the voice rides the fall");
    assert!(
        log.borrow().banks.is_empty(),
        "no engagement clunk while still shifting: {:#?}",
        log.borrow().banks
    );

    harness.with_drive(|drive, _| {
        drive.truck_mut().transmission.shift_timer = 0.0; // the gear takes
        drive.truck_mut().rpm = 950.0;
    });
    update_audio(&mut harness, 0.0);
    assert_eq!(last_rpm(&log).0, 950.0);
    assert!(
        log.borrow()
            .banks
            .iter()
            .any(|(key, volume)| key == "vehicle/shift_auto"
                && close(*volume, SHIFT_END_CLUNK_VOLUME)),
        "{:#?}",
        log.borrow().banks
    );

    log.borrow_mut().banks.clear();
    update_audio(&mut harness, 0.0); // recovery continues: clunk fires only once
    assert!(log.borrow().banks.is_empty(), "{:#?}", log.borrow().banks);
}

#[test]
fn test_manual_clutch_out_ducks_load_but_keeps_live_revs() {
    // Manual shifting: the player owns the engine while the clutch is out --
    // a throttle blip must stay audible (rev-matching is a skill) -- but the
    // engine unloads to the shift cap and swells back on engagement.
    let (mut harness, log) = a_drive("Clutch Out");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.transmission.automatic = false;
        truck.transmission.gear = 5;
        truck.transmission.clutch = 1.0; // pedal down
        truck.throttle = 1.0; // a blip while the clutch is out
        truck.rpm = 1400.0;
    });

    update_audio(&mut harness, 0.0);
    let (rpm, load) = last_rpm(&log);
    assert_eq!(rpm, 1400.0);
    assert!(
        close(load, SHIFT_LOAD_CAP),
        "live revs, ducked load: {load}"
    );

    harness.with_drive(|drive, _| drive.truck_mut().rpm = 1650.0); // the blip climbs
    update_audio(&mut harness, 0.0);
    let (rpm, load) = last_rpm(&log);
    assert_eq!(rpm, 1650.0, "the voice must follow, not hold");
    assert!(close(load, SHIFT_LOAD_CAP), "{load}");

    harness.with_drive(|drive, _| drive.truck_mut().transmission.clutch = 0.0); // hooked back up
    update_audio(&mut harness, 0.1); // recovery ramps with real time, not a sync
    let (rpm, load) = last_rpm(&log);
    assert_eq!(rpm, 1650.0);
    assert!(
        load > SHIFT_LOAD_CAP,
        "the engine speaks under load again: {load}"
    );
}

#[test]
fn test_engine_audio_load_tracks_manual_throttle_smoothly() {
    let (mut harness, log) = a_drive("Throttle Filter");
    harness.with_drive(|drive, _| {
        drive.truck_mut().transmission.shift_timer = 0.0;
        drive.engine_audio_throttle = 0.5;
        drive.truck_mut().throttle = 0.75;
    });
    update_audio(&mut harness, 0.1);
    let rising = last_rpm(&log).1;
    harness.with_drive(|drive, _| drive.truck_mut().throttle = 0.25);
    update_audio(&mut harness, 0.1);
    let falling = last_rpm(&log).1;

    // Raw throttle still controls audible load, but the 450-millisecond
    // filter prevents an immediate gain step for a cruise correction.
    assert!(close(rising, 0.5 + (0.75 - 0.5) * (0.1 / 0.45)), "{rising}");
    assert!(
        close(falling, rising + (0.25 - rising) * (0.1 / 0.45)),
        "{falling}"
    );
    assert!(0.25 < falling && falling < rising && rising < 0.75);
}

// -- the loops ------------------------------------------------------------------------

#[test]
fn test_jake_growl_follows_stage_rpm_and_cuts_through_shifts() {
    let (mut harness, log) = a_drive("Jake Growl");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.set_air_ready(false);
        truck.start_engine();
        truck.transmission.automatic = true;
        truck.transmission.gear = 8;
        truck.velocity_mps = 20.0;
        truck.throttle = 0.0;
        truck.engine_brake_stage = 3;
        truck.rpm = 1850.0;
    });
    log.borrow_mut().loops.clear();

    update_audio(&mut harness, 0.0);
    let jake_key = jake_start(&log).expect("the jake growl started");
    assert_eq!(jake_key, "engine/jake_1800", "nearest loop to 1850");

    // Mid-shift the jake cuts out -- the stair-step signature.
    harness.with_drive(|drive, _| drive.truck_mut().transmission.shift_timer = 0.5);
    update_audio(&mut harness, 0.0);
    assert!(
        loops(&log).contains(&LoopCall::Stop(CH_JAKE)),
        "{:#?}",
        loops(&log)
    );

    // Back in gear at higher revs: it resumes on the higher loop.
    log.borrow_mut().loops.clear();
    harness.with_drive(|drive, _| {
        drive.truck_mut().transmission.shift_timer = 0.0;
        drive.truck_mut().rpm = 2150.0;
    });
    update_audio(&mut harness, 0.0);
    assert_eq!(
        jake_start(&log).expect("the growl resumed"),
        "engine/jake_2200"
    );

    // Throttle on: a jake never sounds under power.
    log.borrow_mut().loops.clear();
    harness.with_drive(|drive, _| drive.truck_mut().throttle = 0.5);
    update_audio(&mut harness, 0.0);
    assert!(
        loops(&log).contains(&LoopCall::Stop(CH_JAKE)),
        "{:#?}",
        loops(&log)
    );
}

/// Agent drive, Silverthorne to Edwards (2026-09-24): the jake bed restarted
/// dozens of times in a row, one start per throttle pulse. The gap is still
/// silent; the loop is held through it and comes back up, not restarted.
#[test]
fn test_a_jake_gap_holds_the_loop_instead_of_restarting_it() {
    let (mut harness, log) = a_drive("Jake Gap");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.set_air_ready(false);
        truck.start_engine();
        truck.transmission.automatic = true;
        truck.transmission.gear = 8;
        truck.velocity_mps = 20.0;
        truck.throttle = 0.0;
        truck.engine_brake_stage = 2;
        truck.rpm = 2000.0;
    });
    log.borrow_mut().loops.clear();
    for frame in 0..40 {
        let throttle = if (frame / 5) % 2 == 1 { 0.5 } else { 0.0 };
        harness.with_drive(move |drive, _| drive.truck_mut().throttle = throttle);
        update_audio(&mut harness, 0.05);
    }
    let calls = loops(&log);
    let starts = calls
        .iter()
        .filter(|call| matches!(call, LoopCall::Start(CH_JAKE, _, _)))
        .count();
    assert_eq!(starts, 1, "{calls:#?}");
    assert!(!calls.contains(&LoopCall::Stop(CH_JAKE)), "{calls:#?}");
    assert!(
        calls.contains(&LoopCall::Volume(CH_JAKE, 0.0)),
        "the gap is silent"
    );

    // Switched off for good, it stops.
    harness.with_drive(|drive, _| {
        drive.truck_mut().throttle = 0.0;
        drive.truck_mut().engine_brake_stage = 0;
    });
    for _ in 0..60 {
        update_audio(&mut harness, 0.05);
    }
    assert!(loops(&log).contains(&LoopCall::Stop(CH_JAKE)));
}

/// A stage change is a change of level on the loop already playing, never a
/// new start: the owner heard the engine-brake sound "looping" down I-70 as
/// the retarder walked 3, 2, 1, 2 (2026-09-24).
#[test]
fn test_a_jake_stage_change_turns_the_loop_rather_than_restarting_it() {
    let (mut harness, log) = a_drive("Jake Stages");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.set_air_ready(false);
        truck.start_engine();
        truck.transmission.automatic = true;
        truck.transmission.gear = 8;
        truck.velocity_mps = 20.0;
        truck.throttle = 0.0;
        truck.engine_brake_stage = 3;
        truck.rpm = 1600.0;
    });
    log.borrow_mut().loops.clear();
    for stage in [3, 2, 1, 2, 3, 1, 3] {
        harness.with_drive(move |drive, _| drive.truck_mut().engine_brake_stage = stage);
        for _ in 0..10 {
            update_audio(&mut harness, 0.05);
        }
    }
    let calls = loops(&log);
    let starts = calls
        .iter()
        .filter(|call| matches!(call, LoopCall::Start(CH_JAKE, _, _)))
        .count();
    assert_eq!(starts, 1, "{calls:#?}");
    assert!(!calls.contains(&LoopCall::Stop(CH_JAKE)), "{calls:#?}");
}

/// The last playback rate set on the jake channel, if any.
fn jake_rate(log: &Log) -> Option<f64> {
    loops(log).into_iter().rev().find_map(|call| match call {
        LoopCall::Rate(CH_JAKE, rate) => Some(rate),
        _ => None,
    })
}

#[test]
fn test_jake_pitch_follows_the_revs_so_a_manual_driver_can_hear_the_shift_point() {
    // One 1600 rpm cut stands for every band, so the growl used to hold one
    // note however the revs moved. Now it is re-pitched from that native
    // speed every frame: a gear run down toward the shift point falls in
    // pitch, and the downshift brings it back up (tester ask, 2026-09-14).
    let (mut harness, log) = a_drive("Jake Pitch");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.set_air_ready(false);
        truck.start_engine();
        truck.transmission.automatic = false;
        truck.transmission.gear = 8;
        truck.velocity_mps = 20.0;
        truck.throttle = 0.0;
        truck.engine_brake_stage = 3;
        truck.rpm = 1600.0;
    });
    log.borrow_mut().loops.clear();
    update_audio(&mut harness, 0.0);
    assert!(jake_start(&log).is_some(), "the growl started");
    assert!(
        close(jake_rate(&log).unwrap(), 1.0),
        "{:?}",
        jake_rate(&log)
    );

    // Revs falling in the same gear: the note falls with them.
    log.borrow_mut().loops.clear();
    harness.with_drive(|drive, _| drive.truck_mut().rpm = 1200.0);
    update_audio(&mut harness, 0.0);
    assert!(
        close(jake_rate(&log).unwrap(), 0.75),
        "{:?}",
        jake_rate(&log)
    );

    // The downshift lands the revs high again: the note climbs.
    harness.with_drive(|drive, _| drive.truck_mut().rpm = 2000.0);
    update_audio(&mut harness, 0.0);
    assert!(
        close(jake_rate(&log).unwrap(), 1.25),
        "{:?}",
        jake_rate(&log)
    );

    // Just above the jake floor it still tracks; past redline the pitch
    // stops at the clamp rather than sliding into tape territory.
    harness.with_drive(|drive, _| drive.truck_mut().rpm = 960.0);
    update_audio(&mut harness, 0.0);
    assert!(
        close(jake_rate(&log).unwrap(), 0.6),
        "{:?}",
        jake_rate(&log)
    );
    harness.with_drive(|drive, _| drive.truck_mut().rpm = 2400.0);
    update_audio(&mut harness, 0.0);
    assert!(
        close(jake_rate(&log).unwrap(), JAKE_RATE_MAX),
        "{:?}",
        jake_rate(&log)
    );
}

#[test]
fn test_manual_shift_clunks_at_the_lever_and_again_when_the_gear_takes() {
    // Automatic shifts are kachunk -- sigh -- kachunk; a manual one used to
    // be a single clunk at the lever and silence when the clutch came back
    // in (tester ask, 2026-09-14). The second clunk waits for the gear to
    // actually take: clutch in, lever time over.
    let (mut harness, log) = a_drive("Manual Clunk");
    harness.with_drive(|drive, _| {
        let truck = drive.truck_mut();
        truck.set_air_ready(false);
        truck.start_engine();
        truck.transmission.automatic = false;
        truck.transmission.gear = 4;
        truck.velocity_mps = 12.0;
        truck.rpm = 1500.0;
        truck.transmission.clutch = 1.0; // pedal down
    });
    log.borrow_mut().banks.clear();
    harness.with_drive(|drive, ctx| drive.manual_shift(ctx, 5));
    assert_eq!(
        log.borrow().banks.as_slice(),
        &[("vehicle/shift_auto".to_string(), 1.0)],
        "the lever clunk, at full level"
    );

    // Clutch still down through the lever time: nothing engages, nothing
    // plays.
    log.borrow_mut().banks.clear();
    update_audio(&mut harness, 0.0);
    assert!(log.borrow().banks.is_empty(), "{:#?}", log.borrow().banks);

    // Clutch back in and the lever time up: the gear takes.
    harness.with_drive(|drive, _| {
        drive.truck_mut().transmission.clutch = 0.0;
        drive.truck_mut().transmission.shift_timer = 0.0;
    });
    update_audio(&mut harness, 0.0);
    assert_eq!(
        log.borrow().banks.as_slice(),
        &[("vehicle/shift_auto".to_string(), SHIFT_END_CLUNK_VOLUME)],
        "the engagement clunk, softer"
    );

    // Once only.
    log.borrow_mut().banks.clear();
    update_audio(&mut harness, 0.0);
    assert!(log.borrow().banks.is_empty(), "{:#?}", log.borrow().banks);

    // A shift taken to neutral engages nothing, so it owes nothing.
    harness.with_drive(|drive, _| {
        drive.truck_mut().transmission.clutch = 1.0;
        drive.manual_engage_clunk_pending = true;
        drive.truck_mut().transmission.gear = 0;
        drive.truck_mut().transmission.clutch = 0.0;
        drive.truck_mut().transmission.shift_timer = 0.0;
    });
    log.borrow_mut().banks.clear();
    update_audio(&mut harness, 0.0);
    assert!(log.borrow().banks.is_empty(), "{:#?}", log.borrow().banks);
}

/// The key the jake channel was last started on, if it was started.
fn jake_start(log: &Log) -> Option<String> {
    loops(log).into_iter().find_map(|call| match call {
        LoopCall::Start(CH_JAKE, key, _) => Some(key),
        _ => None,
    })
}

#[test]
fn test_cold_start_buzzer_waits_out_the_crank() {
    let (mut harness, log) = a_drive("Cold Buzzer");
    harness.with_drive(|drive, _| {
        drive.truck_mut().set_cold_air_start();
        drive.truck_mut().start_engine();
        drive.pending_low_air_buzzer = true; // what the E-key start path arms
    });
    log.borrow_mut().played.clear();

    // While the ignition crank still plays, the buzzer must hold.
    log.borrow_mut().engine_starting = true;
    update_audio(&mut harness, 0.0);
    assert!(
        !played_keys(&log)
            .iter()
            .any(|k| k == "vehicle/low_air_buzzer"),
        "{:#?}",
        played_keys(&log)
    );
    assert!(harness.read_drive(|d| d.pending_low_air_buzzer));

    // Crank handed off with the air still low (55 psi): now it may sound.
    log.borrow_mut().engine_starting = false;
    update_audio(&mut harness, 0.0);
    assert!(
        played_keys(&log)
            .iter()
            .any(|k| k == "vehicle/low_air_buzzer"),
        "{:#?}",
        played_keys(&log)
    );
    assert!(!harness.read_drive(|d| d.pending_low_air_buzzer));

    // And if the compressor had already built past the warning line, the
    // pending buzzer dissolves silently.
    log.borrow_mut().played.clear();
    harness.with_drive(|drive, _| {
        drive.pending_low_air_buzzer = true;
        drive.truck_mut().set_air_pressure_psi(80.0); // above the 60 psi warning
    });
    update_audio(&mut harness, 0.0);
    assert!(
        !played_keys(&log)
            .iter()
            .any(|k| k == "vehicle/low_air_buzzer"),
        "{:#?}",
        played_keys(&log)
    );
    assert!(!harness.read_drive(|d| d.pending_low_air_buzzer));
}

#[test]
fn test_reverse_audio_loop_restarts_after_pause_resume() {
    let (mut harness, log) = a_drive("Reverse Pause");
    harness.with_drive(|drive, _| {
        drive.truck_mut().start_engine();
        drive.truck_mut().transmission.gear = REVERSE;
    });
    log.borrow_mut().reverse.clear();
    update_audio(&mut harness, 0.0);
    assert_eq!(log.borrow().reverse, vec!["start"]);

    harness.with_drive(|drive, ctx| drive.push_pause_menu(ctx));
    assert_eq!(log.borrow().reverse, vec!["start", "stop"]);
    harness.select_menu_item("Resume driving");

    log.borrow_mut().reverse.clear();
    update_audio(&mut harness, 0.0);

    assert_eq!(log.borrow().reverse, vec!["start"]);
}

// -- the two-way engine mirror ----------------------------------------------------------

#[test]
fn test_rest_menu_shutdown_also_stops_engine_audio() {
    // Sleeping shuts the truck's engine down from a rest menu, outside the
    // driving frame loop. Regression: the audio loop was left running --
    // masked while band volumes tracked RPM, plainly audible once the BASS
    // engine model kept constant volume.
    let (mut harness, _log) = a_drive("Rest Shutdown");
    assert!(harness.with_drive(|drive, _| drive.truck_mut().start_engine()));
    update_audio(&mut harness, 0.0);
    assert!(harness.app.ctx.audio.engine_running());

    let prefix = harness.with_drive(shut_down_engine);

    assert_eq!(prefix, "You shut down the engine. ");
    assert!(!harness.read_drive(|d| d.truck().engine_on));
    assert!(!harness.app.ctx.audio.engine_running());

    // Already off: no double narration, audio stays off.
    assert_eq!(harness.with_drive(shut_down_engine), "");
    assert!(!harness.app.ctx.audio.engine_running());
}

#[test]
fn test_engine_audio_mirror_sync_catches_any_out_of_band_stop() {
    // The frame-loop audio sync must work in both directions: any path that
    // turns the truck's engine off without telling the audio engine is
    // corrected on the next frame, silently.
    let (mut harness, _log) = a_drive("Mirror Sync");
    assert!(harness.with_drive(|drive, _| drive.truck_mut().start_engine()));
    update_audio(&mut harness, 0.0);
    assert!(harness.app.ctx.audio.engine_running());

    harness.with_drive(|drive, _| drive.truck_mut().stop_engine()); // off-path stop
    update_audio(&mut harness, 0.0);

    assert!(!harness.app.ctx.audio.engine_running());
}

// -- road texture ---------------------------------------------------------------------

#[test]
fn test_road_joint_thumps_use_physical_distance_at_every_pace() {
    // The thumps follow real wheel travel, not the trip model's compressed
    // route distance, so the same stretch of concrete sounds the same at
    // every time compression.
    //
    // Python stubbed `truck.update` to hold the speed; here the truck is
    // re-pinned to 20 m/s at the top of each frame, which is the same
    // arrangement without a fake. The rumble severity Python also read is
    // unreachable -- `RumbleEngine`'s one-shot queue is private -- but it is
    // the same number the thump's own volume carries (`0.015 * severity`),
    // so the assertion below pins it there.
    for time_scale in [10.0, 20.0, 40.0] {
        let (mut harness, log) = a_drive(&format!("Joints {time_scale}"));
        harness.app.ctx.settings.time_scale = time_scale;
        harness.with_drive(move |drive, _| {
            drive.road_joint_accumulator_m = 0.0;
            drive.next_joint_distance_m = 15.0;
            drive.trip.time_scale = time_scale;
            drive.trip.set_patrols(Vec::new());
            drive.trip.posts.clear();
            drive.truck_mut().start_engine();
        });
        log.borrow_mut().played.clear();

        for _ in 0..30 {
            harness.advance_clock(1.0 / 60.0);
            harness.with_drive(|drive, ctx| {
                drive.truck_mut().velocity_mps = 20.0;
                drive.update_audio(ctx, 1.0 / 60.0);
            });
        }
        assert!(
            (harness.read_drive(|d| d.road_joint_accumulator_m) - 10.0).abs() < 1e-3,
            "{}",
            harness.read_drive(|d| d.road_joint_accumulator_m)
        );
        assert!(joint_plays(&log).is_empty(), "{:#?}", joint_plays(&log));

        for _ in 0..30 {
            harness.advance_clock(1.0 / 60.0);
            harness.with_drive(|drive, ctx| {
                drive.truck_mut().velocity_mps = 20.0;
                drive.update_audio(ctx, 1.0 / 60.0);
            });
        }
        let thumps = joint_plays(&log);
        assert_eq!(thumps.len(), 1, "{thumps:#?}");
        // 0.015 * (20 / 30): the severity the rumble is given, in the volume.
        assert!((thumps[0] - 0.01).abs() < 1e-5, "{}", thumps[0]);
        assert!(
            (harness.read_drive(|d| d.road_joint_accumulator_m) - 5.0).abs() < 1e-3,
            "{}",
            harness.read_drive(|d| d.road_joint_accumulator_m)
        );
        let next = harness.read_drive(|d| d.next_joint_distance_m);
        assert!((14.0..=18.0).contains(&next), "{next}");
    }
}

/// The volumes every road-joint thump played at.
fn joint_plays(log: &Log) -> Vec<f64> {
    log.borrow()
        .played
        .iter()
        .filter(|(key, _)| key == "vehicle/road_joint")
        .map(|(_, volume)| *volume)
        .collect()
}

#[test]
fn test_the_engine_carries_the_guide_and_the_road_bed_carries_the_seat() {
    // Owner ruling 2026-09-18: the steering guide moved off the road bed and
    // onto the engine, and the lane-position readout took the bed in exchange.
    // The two must not blur -- the engine says where to GO, the bed says where
    // the truck IS, and the pans point opposite ways when the truck is
    // drifting, which is exactly what makes them tellable apart.
    let (mut harness, log) = a_drive("Engine guide pan");
    harness.app.ctx.settings.lane_departure_warning = true;
    for mode in ["partial", "off"] {
        harness.app.ctx.settings.lane_keeping = mode.into();
        harness.with_drive(|drive, _| {
            // Rolling: the guide only wakes on a truck actually driving.
            drive.trip.truck.engine_on = true;
            drive.trip.truck.velocity_mps = 55.0 / 2.23694;
            drive.lane.offset = 0.8; // well past DRIFT_WAKE, toward the right
            drive.engine_guide_pan_applied = Some(0.0);
            drive.road_pan_applied = Some(0.0);
        });
        log.borrow_mut().engine_pan.clear();
        log.borrow_mut().loop_pans.clear();
        // The guide slews rather than snapping, so give it road to lean over.
        for _ in 0..120 {
            harness.with_drive(|drive, ctx| drive.update_lane_guidance_audio(ctx, 1.0 / 60.0));
        }

        let guide = log
            .borrow()
            .engine_pan
            .last()
            .copied()
            .unwrap_or_else(|| panic!("{mode}: the engine was never panned"));
        assert!(
            guide < -0.1,
            "{mode}: drifting right, the engine must lean LEFT to be followed \
             back to centre; got {guide}"
        );
        let seat = log
            .borrow()
            .loop_pans
            .last()
            .copied()
            .unwrap_or_else(|| panic!("{mode}: the road bed was never panned"));
        assert!(
            seat.1 > 0.1,
            "{mode}: the bed reports where the truck IS, which is right of \
             centre; got {seat:?}"
        );
    }

    // Full lane keeping holds the lane itself, so there is no DRIFT to
    // report and both channels come back to centre. A turn still leans there
    // -- the owner's 2026-09-18 ruling -- which `turn_guide` covers on its
    // own; this fixture's road is straight.
    harness.app.ctx.settings.lane_keeping = "full".into();
    harness.with_drive(|drive, _| {
        drive.engine_guide_pan_applied = Some(0.5); // so a write back to centre shows
        drive.road_pan_applied = Some(0.5);
    });
    log.borrow_mut().engine_pan.clear();
    log.borrow_mut().loop_pans.clear();
    // Long enough for the lean to SLEW home rather than snap: easing back is
    // the behaviour, so the test has to give it the road to do it in.
    for _ in 0..120 {
        harness.with_drive(|drive, ctx| drive.update_lane_guidance_audio(ctx, 1.0 / 60.0));
    }
    assert_eq!(log.borrow().engine_pan.last().copied(), Some(0.0));
    assert_eq!(log.borrow().loop_pans.last().map(|p| p.1), Some(0.0));
}

#[test]
fn test_the_engine_pan_is_the_guide_and_not_the_lane_position() {
    // The engine's pan belongs to the guide now. It used to be set straight
    // from lane.offset as a position readout, which would fight the guide for
    // the same channel and leave whichever ran last in charge.
    let (mut harness, log) = a_drive("Engine pan ownership");
    harness.app.ctx.settings.lane_keeping = "off".into();
    harness.app.ctx.settings.lane_departure_warning = true;
    harness.with_drive(|drive, _| {
        drive.trip.truck.engine_on = true;
        drive.trip.truck.velocity_mps = 55.0 / 2.23694;
        drive.lane.offset = 0.75; // drifted well right
    });
    log.borrow_mut().engine_pan.clear();
    update_audio(&mut harness, 1.0 / 60.0);
    let pan = log
        .borrow()
        .engine_pan
        .last()
        .copied()
        .expect("the guide pans the engine from the audio frame");
    assert!(
        pan <= 0.0,
        "drifted right, the guide must lean left or sit centred, never mirror \
         the offset back as a position readout; got {pan}"
    );
}
