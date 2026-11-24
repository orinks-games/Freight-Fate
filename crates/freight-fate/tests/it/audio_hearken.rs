//! The Hearken backend behind the facade: the backend-generic tests of
//! `audio_backends.rs`, `audio_backend_cues.rs` and `audio_loops.rs`, run
//! over Hearken's recording backend and asserted on the calls it saw.
//!
//! They were written beside BASS versions that skipped on a machine without
//! the library; these run everywhere. Where a BASS test read a channel
//! attribute, this one reads the mix the pool sent; where it read
//! `requested_keys`, the list is the one BASS's code asked for, in the same
//! order, because the swap was meant to be invisible to the game.

use ff_core::lane_guide_tone::{register_lane_guide_tone, LANE_GUIDE_TONE_KEY};
use freight_fate::audio::hearken_clips::clip_sample_rate;
use freight_fate::audio::*;
use hearken::audio::{Call, LoopRegion};

use crate::audio_support::{audio_lock, hearken_rig, shipped_sounds, sine_wav, HearkenRig};

/// Every facade call must be safe on this backend too.
fn exercise(a: &mut AudioEngine) {
    a.play("ui/menu_select");
    a.play("nonexistent/sound");
    a.set_engine_pan(0.5);
    a.engine_start();
    a.set_engine_pan(0.0);
    a.set_engine_rpm_with(1500.0, 0.5);
    a.set_engine_rpm_with(2200.0, 1.0);
    a.set_road_noise(20.0);
    a.set_road_noise(0.0);
    a.set_weather_with(Some("weather/rain_light"), 0.8);
    a.set_weather(None);
    a.set_wind(0.5);
    a.set_ambient_with(Some("ambient/truck_stop"), 0.4);
    a.play_music("menu_theme");
    a.play_music("open_road");
    a.play_music("not_a_track");
    a.set_volumes(&VolumeUpdate::default().master(0.5).sfx(0.5).music(0.5));
    a.horn_start();
    a.horn_stop();
    for _ in 0..10 {
        a.update(0.05);
    }
    a.stop_world();
    a.stop_music();
    a.shutdown();
}

fn all_engine_cuts() -> bool {
    let all = STEREO_ENGINE_BANDS
        .iter()
        .all(|(key, _rpm)| asset_bytes(key, SFX_EXTENSIONS).is_some());
    if !all {
        eprintln!("SKIPPING the ring checks: the engine band cuts are not all here");
    }
    all
}

/// The `Play` call that started `channel`.
fn play_call(r: &HearkenRig, channel: u64) -> Call {
    r.rec
        .calls()
        .into_iter()
        .find(|call| matches!(call, Call::Play { channel: c, .. } if *c == channel))
        .expect("a play call")
}

fn play_count(r: &HearkenRig) -> usize {
    r.rec.played().len()
}

#[test]
fn test_hearken_backend_is_the_default_and_safe_everywhere() {
    let _guard = audio_lock();
    // Asked for by name, it comes up (silent here: the run is headless).
    let mut a = AudioEngine::from_preference("hearken");
    assert_eq!(a.backend_name(), "hearken");
    assert!(a.enabled());
    assert!(!a.hearken().unwrap().no_device());
    exercise(&mut a);
    // It is the default too.
    let default = AudioEngine::from_preference("");
    assert_eq!(default.backend_name(), "hearken");
    // And over the recording backend, the whole surface is safe too.
    let mut r = hearken_rig();
    exercise(&mut r.engine);
}

#[test]
fn test_hearken_engine_model_matches_available_cuts() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.engine_start_with(false);
    assert!(r.engine.engine_running());
    if all_engine_cuts() {
        assert_eq!(r.hearken().engine_bands().len(), STEREO_ENGINE_BANDS.len());
        assert!(r.hearken().engine_sound().is_none());
        for (key, _rpm) in STEREO_ENGINE_BANDS {
            let Call::Play { looping, mix, .. } = play_call(&r, r.channel(key)) else {
                unreachable!()
            };
            assert!(looping, "{key} must loop");
            assert_eq!(mix.gain, 0.0, "{key} starts silent");
        }
    } else {
        assert!(r.hearken().engine_bands().is_empty());
        assert!(r.hearken().engine_sound().is_none());
    }
    for rpm in [600.0, 1100.0, 1800.0, 2200.0, 900.0] {
        r.engine.set_engine_rpm_with(rpm, 0.7);
    }
    r.engine.engine_stop();
    assert!(!r.engine.engine_running());
    assert!(r.hearken().engine_sound().is_none());
    assert!(r.hearken().engine_bands().is_empty());
}

#[test]
fn test_hearken_engine_tracks_rpm_within_a_recording() {
    if !shipped_sounds() || !all_engine_cuts() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.engine_start_with(false);
    let (key, native) = STEREO_ENGINE_BANDS[1];
    let channel = r.channel(key);
    r.engine.set_engine_rpm_with(native * 1.02, 0.0);
    let band = r.hearken().engine_bands()[1];
    assert!((band.last_rate - 1.02).abs() < 1e-9);
    assert!((r.rec.last_mix(channel).unwrap().pitch - 1.02).abs() < 1e-6);
}

#[test]
fn test_hearken_engine_voice_setting_switches_models_live() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.set_engine_voice(true);
    r.engine.engine_start_with(false);
    assert!(r.hearken().engine_bands().is_empty());
    assert!(r.hearken().engine_sound().is_some());
    if all_engine_cuts() {
        r.engine.set_engine_voice(false);
        assert!(r.engine.engine_running());
        assert_eq!(r.hearken().engine_bands().len(), STEREO_ENGINE_BANDS.len());
        assert!(r.hearken().engine_sound().is_none());
    }
    r.engine.engine_stop();
}

#[test]
fn test_hearken_classic_voice_prefers_the_original_recording() {
    let mut r = hearken_rig();
    r.hearken_mut().record_requested_keys(true);
    r.engine.set_engine_voice(true);
    r.engine.engine_start_with(false);
    // BASS asks for exactly this: the classic cut, and nothing else when
    // it is there.
    assert_eq!(r.hearken().requested_keys(), vec![ENGINE_CLASSIC_LOOP_KEY]);
    assert!(r.hearken().engine_bands().is_empty());
    assert!(r.hearken().engine_sound().is_some());
    r.engine.engine_stop();
}

#[test]
fn test_hearken_engine_does_not_fall_back_to_legacy_samples() {
    let mut r = hearken_rig();
    r.hearken_mut().set_key_filter(Some(Box::new(|key: &str| {
        !key.starts_with("engine_stereo/")
    })));
    r.hearken_mut().record_requested_keys(true);
    r.engine.engine_start_with(false);
    assert!(r.engine.engine_running());
    assert!(r.hearken().engine_bands().is_empty());
    assert!(r.hearken().engine_sound().is_none());
    assert!(r
        .hearken()
        .requested_keys()
        .iter()
        .all(|key| !key.starts_with("engine/")));
    r.engine.engine_stop();
}

#[test]
fn test_hearken_requested_keys_keep_their_order_for_a_start_and_a_drive() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.set_engine_voice(true);
    r.hearken_mut().record_requested_keys(true);
    r.engine.engine_start(); // deliberate: the crank first, then the ring
    r.rec.finish(r.channel("engine/start"));
    r.engine.update(0.05);
    r.engine.set_road_noise(15.0);
    r.engine.horn_start();
    r.engine.horn_start(); // autorepeat asks for nothing
    r.engine.horn_stop();
    r.engine.play("ui/menu_move");
    r.engine.set_ambient(Some("poi/facility_gate"));
    let mut expected = vec![
        "engine/start".to_string(),
        ENGINE_CLASSIC_LOOP_KEY.to_string(),
    ];
    expected.extend(
        [
            "vehicle/road",
            "vehicle/horn",
            "ui/menu_move",
            "poi/facility_gate",
        ]
        .map(String::from),
    );
    assert_eq!(r.hearken().requested_keys(), expected);
}

#[test]
fn test_hearken_silent_engine_start_skips_the_ignition_crank() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.hearken_mut().record_requested_keys(true);
    r.engine.engine_start_with(false);
    assert!(r.engine.engine_running());
    assert!(!r
        .hearken()
        .requested_keys()
        .iter()
        .any(|k| k == "engine/start"));
    if all_engine_cuts() {
        assert_eq!(r.hearken().engine_bands().len(), STEREO_ENGINE_BANDS.len());
    } else {
        assert!(r.hearken().engine_bands().is_empty());
        assert!(r.hearken().engine_sound().is_none());
    }
    assert!(!r.engine.engine_starting());
    r.engine.engine_stop();
}

#[test]
fn test_hearken_deliberate_start_waits_for_crank_completion() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.set_engine_voice(true);
    r.engine
        .set_volumes(&VolumeUpdate::default().master(0.8).engine(0.5));
    r.engine.engine_start();
    let crank = r.channel("engine/start");
    let Call::Play { looping, mix, .. } = play_call(&r, crank) else {
        unreachable!()
    };
    assert!(!looping);
    assert!(
        (mix.gain - 0.4).abs() < 1e-6,
        "the crank at engine x master"
    );
    let h = r.hearken();
    assert_eq!(h.engine_intro_gain(), 0.0);
    assert!(r.engine.engine_starting());
    assert!(h.engine_bands().is_empty());
    assert!(h.engine_sound().is_none());
    r.rec.finish(crank);
    r.engine.update(0.05);
    r.engine.update(0.1);
    let h = r.hearken();
    assert_eq!(h.engine_intro_gain(), 1.0);
    assert_eq!(h.fade_count(), 0);
    assert!(!r.engine.engine_starting());
    assert!(r.engine.engine_running());
    assert!(!r.rec.is_playing(crank));
    assert!(h.engine_sound().is_some());
    r.engine.engine_stop();
}

#[test]
fn test_hearken_engine_stop_clears_starting_and_pending_fades() {
    let mut r = hearken_rig();
    r.engine.set_engine_voice(true);
    r.engine.engine_start();
    assert!(r.engine.engine_starting());
    assert!(r.rec.is_playing(r.channel("engine/start")));
    r.engine.engine_stop();
    assert!(!r.engine.engine_starting());
    assert_eq!(r.hearken().fade_count(), 0);
    assert_eq!(r.hearken().engine_intro_gain(), 1.0);
}

#[test]
fn test_hearken_road_noise_tracks_speed() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.set_road_noise(15.0);
    let entry = r.engine.backend().loop_entry(CH_ROAD).expect("road loop");
    assert_eq!(entry, ("vehicle/road".to_string(), 0.5));
    let rate = r.hearken().road_noise_rate().unwrap();
    assert!((rate - 0.85).abs() < 1e-6);
    let channel = r.channel("vehicle/road");
    assert!((f64::from(r.rec.last_mix(channel).unwrap().pitch) - 0.85).abs() < 1e-6);
    r.engine.set_road_noise(0.0);
    assert!(r.engine.backend().loop_entry(CH_ROAD).is_none());
}

#[test]
fn test_hearken_new_context_loops_enter_at_full_gain() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.set_wind(2.0);
    let entry = r.engine.backend().loop_entry(CH_WEATHER_B).unwrap();
    assert_eq!(entry, ("weather/wind".to_string(), 1.0));
    r.engine.set_ambient(Some("poi/facility_gate"));
    let entry = r.engine.backend().loop_entry(CH_AMBIENT).unwrap();
    assert_eq!(entry, ("poi/facility_gate".to_string(), 1.0));
    // A loop starts silent and fades in: a gain ramp, never a jump.
    let channel = r.channel("poi/facility_gate");
    let Call::Play { mix, looping, .. } = play_call(&r, channel) else {
        unreachable!()
    };
    assert!(looping);
    assert_eq!(mix.gain, 0.0);
    assert!(r.rec.calls().iter().any(|call| matches!(
        call,
        Call::SetMixRamped { channel: c, ramps, .. } if *c == channel && ramps.gain_ms > 0
    )));
}

#[test]
fn test_hearken_horn_sustains_then_rings_out_without_stacking() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.horn_start();
    r.engine.horn_start(); // key autorepeat must not stack a second horn
    assert_eq!(
        r.engine.backend().loop_entry(CH_HORN).unwrap().0,
        "vehicle/horn"
    );
    assert_eq!(r.hearken().sustain_channels(), vec![CH_HORN]);
    assert_eq!(play_count(&r), 1);
    let channel = r.channel("vehicle/horn");
    // The held region, in seconds of the clip's own sample rate.
    let (data, _) = asset_bytes("vehicle/horn", SFX_EXTENSIONS).unwrap();
    let rate = clip_sample_rate(&data).expect("the horn's sample rate");
    let region = r.rec.calls().into_iter().find_map(|call| match call {
        Call::SetLoopRegion { channel: c, region } if c == channel => Some(region),
        _ => None,
    });
    let expected = LoopRegion::new(
        f64::from(HORN_LOOP_START) / rate,
        f64::from(HORN_LOOP_END) / rate,
    );
    match (play_call(&r, channel), region) {
        (Call::Play { looping, .. }, Some(region)) => {
            assert!(looping);
            assert_eq!(region, expected);
        }
        (Call::Play { looping, .. }, None) => {
            // The region rode in on the start instead of a later call.
            assert!(looping);
        }
        _ => unreachable!(),
    }
    let sound = r.hearken().loop_sound(CH_HORN).unwrap();
    r.engine.horn_stop();
    // Released: the channel handed off, the tail still ringing.
    assert!(r.hearken().sustain_channels().is_empty());
    assert!(r.engine.backend().loop_entry(CH_HORN).is_none());
    assert!(r
        .rec
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Release { channel: c, .. } if *c == channel)));
    let tail = r
        .hearken()
        .releasing_entry(CH_HORN)
        .expect("a ringing tail");
    assert_eq!(tail, ("vehicle/horn".to_string(), sound));
    assert!(r.hearken().pool().is_playing(sound));
    // Pressed again mid-tail: ignored, not stacked.
    r.engine.horn_start();
    assert!(r.hearken().sustain_channels().is_empty());
    assert!(r.engine.backend().loop_entry(CH_HORN).is_none());
    assert_eq!(r.hearken().releasing_entry(CH_HORN), Some(tail));
    assert_eq!(play_count(&r), 1);
    // Once the tail ends, the next press sounds.
    r.rec.finish(channel);
    r.engine.update(0.01);
    r.engine.horn_start();
    assert_eq!(play_count(&r), 2);
    assert_eq!(r.hearken().sustain_channels(), vec![CH_HORN]);
}

#[test]
fn test_hearken_stop_world_drops_the_surge_scale_and_lane_guide() {
    if !shipped_sounds() {
        return;
    }
    register_lane_guide_tone();
    let mut r = hearken_rig();
    r.engine
        .start_loop_with(CH_SURGE, "vehicle/liquid_wash", 0.5, 90);
    r.engine
        .start_loop_with(CH_SCALE, "poi/weigh_station_lane", 0.5, 90);
    r.engine
        .start_loop_with(CH_LANE_GUIDE, LANE_GUIDE_TONE_KEY, 0.5, 90);
    for channel in [CH_SURGE, CH_SCALE, CH_LANE_GUIDE] {
        assert!(r.engine.backend().loop_entry(channel).is_some());
    }
    r.engine.stop_world();
    for channel in [CH_SURGE, CH_SCALE, CH_LANE_GUIDE] {
        assert!(r.engine.backend().loop_entry(channel).is_none());
    }
    for key in [
        "vehicle/liquid_wash",
        "poi/weigh_station_lane",
        LANE_GUIDE_TONE_KEY,
    ] {
        let channel = r.channel(key);
        assert!(!r.rec.is_playing(channel), "{key} still playing");
    }
}

#[test]
fn test_hearken_jake_routes_to_recorded_cut() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine
        .start_loop_with(CH_JAKE, "engine/jake_1400", 0.5, 300);
    assert_eq!(
        r.engine.backend().loop_entry(CH_JAKE).unwrap(),
        (JAKE_RECORDED_KEY.to_string(), 0.5)
    );
    assert!(r.rec.channel_of(JAKE_RECORDED_KEY).is_some());
    r.engine.stop_loop(CH_JAKE);
    assert!(r.engine.backend().loop_entry(CH_JAKE).is_none());
}

#[test]
fn test_hearken_one_shot_levels_are_computed_before_the_pool() {
    let mut r = hearken_rig();
    let key = "test/hearken_level_probe";
    register_generated_sound(key, sine_wav(0.05, 1), "wav");
    r.engine
        .set_volumes(&VolumeUpdate::default().master(0.5).sfx(0.8));
    r.engine.play_with(key, 0.5, -0.25);
    let mix = r.rec.last_mix(r.channel(key)).unwrap();
    assert!((mix.gain - 0.2).abs() < 1e-6);
    assert!((mix.pan + 0.25).abs() < 1e-6);
    // A level above unity (a bank's jitter) multiplies first and clamps
    // last, as BASS did.
    r.engine.play_with(key, 1.17, 0.0);
    let mix = r.rec.last_mix(r.channel(key)).unwrap();
    assert!((f64::from(mix.gain) - 1.17 * 0.4).abs() < 1e-6);
    r.engine
        .set_volumes(&VolumeUpdate::default().master(1.0).sfx(1.0));
    r.engine.play_with(key, 1.17, 0.0);
    assert_eq!(r.rec.last_mix(r.channel(key)).unwrap().gain, 1.0);
}

#[test]
fn test_hearken_bank_jitter_lands_above_unity() {
    let cuts = [
        "test/hearken_bank_01",
        "test/hearken_bank_02",
        "test/hearken_bank_03",
    ];
    for key in cuts {
        register_generated_sound(key, sine_wav(0.05, 1), "wav");
    }
    let mut r = hearken_rig();
    r.engine
        .set_volumes(&VolumeUpdate::default().master(0.5).sfx(1.0));
    for _ in 0..30 {
        r.engine
            .play_bank("test/hearken_bank", "test/hearken_bank_fallback");
    }
    let gains: Vec<f32> = r
        .rec
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Play { key, mix, .. } if cuts.contains(&key.as_str()) => Some(mix.gain),
            _ => None,
        })
        .collect();
    assert_eq!(gains.len(), 30);
    assert!(gains
        .iter()
        .all(|g| (0.85 * 0.5 - 1e-6..=1.17 * 0.5 + 1e-6).contains(&f64::from(*g))));
    assert!(gains.iter().any(|g| *g > 0.5), "no jitter above unity");
}

#[test]
fn test_hearken_hears_a_sound_registered_after_a_miss() {
    let mut r = hearken_rig();
    let key = "test/hearken_late_registration";
    r.engine.play(key);
    assert_eq!(play_count(&r), 0, "a miss plays nothing, not a stand-in");
    register_generated_sound(key, sine_wav(0.05, 1), "wav");
    r.engine.play(key);
    assert_eq!(r.rec.played(), vec![key.to_string()]);
}

/// The gain last sent to `channel`, and the ramp it was sent over.
fn last_gain(r: &HearkenRig, channel: u64) -> (f32, u32) {
    r.rec
        .calls()
        .into_iter()
        .rev()
        .find_map(|call| match call {
            Call::SetMixRamped {
                channel: c,
                mix,
                ramps,
            } if c == channel => Some((mix.gain, ramps.gain_ms)),
            Call::SetMix {
                channel: c,
                mix,
                ramp_ms,
            } if c == channel => Some((mix.gain, ramp_ms)),
            Call::Play {
                channel: c, mix, ..
            } if c == channel => Some((mix.gain, 0)),
            _ => None,
        })
        .expect("a mix for the channel")
}

#[test]
fn test_hearken_speech_duck_halves_engine_weather_and_music_at_once() {
    // The event voice ducks the engine, the weather and the music channel
    // to SPEECH_DUCK_LEVEL the instant it speaks, and lets them back up the
    // instant it is done: no ramp either way, and nothing else moves.
    let music = "test/hearken_duck_music";
    register_generated_sound(&format!("music/{music}"), sine_wav(0.5, 1), "wav");
    register_generated_sound("test/hearken_duck_rain", sine_wav(0.5, 1), "wav");
    register_generated_sound("test/hearken_duck_cue", sine_wav(0.5, 1), "wav");
    let mut r = hearken_rig();
    r.engine.set_engine_voice(true); // the classic loop, which ships
    r.engine.engine_start_with(false);
    r.engine
        .start_loop_with(CH_WEATHER, "test/hearken_duck_rain", 0.8, 0);
    r.engine
        .start_loop_with(CH_ALERT, "test/hearken_duck_cue", 0.8, 0);
    r.engine.play_music_with(music, 0);
    for _ in 0..30 {
        r.engine.update(0.05); // past the engine's resume fade
    }
    let engine = r.channel(ENGINE_CLASSIC_LOOP_KEY);
    let rain = r.channel("test/hearken_duck_rain");
    let cue = r.channel("test/hearken_duck_cue");
    let track = r.channel(music);
    let before: Vec<f32> = [engine, rain, cue, track]
        .iter()
        .map(|c| last_gain(&r, *c).0)
        .collect();
    assert!(before.iter().all(|g| *g > 0.0), "{before:?}");

    r.engine.set_speech_duck(SPEECH_DUCK_LEVEL);
    for (i, channel) in [engine, rain, track].into_iter().enumerate() {
        let (gain, ramp) = last_gain(&r, channel);
        let want = before[[0, 1, 3][i]] * SPEECH_DUCK_LEVEL as f32;
        assert!(
            (gain - want).abs() < 1e-6,
            "channel {channel}: {gain} vs {want}"
        );
        assert_eq!(ramp, 0, "the duck is instant");
    }
    assert_eq!(last_gain(&r, cue).0, before[2], "an effect is not ducked");

    r.engine.set_speech_duck(1.0);
    for (i, channel) in [engine, rain, cue, track].into_iter().enumerate() {
        let (gain, ramp) = last_gain(&r, channel);
        assert!((gain - before[i]).abs() < 1e-6, "channel {channel}");
        assert_eq!(ramp, 0);
    }
}

#[path = "audio_hearken_cues.rs"]
mod cues;
