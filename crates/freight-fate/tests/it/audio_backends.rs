//! Port of `tests/test_audio_backends.py`: audio backend selection, the
//! engine model, and the facade's bank / jake-voice / asset logic.
//!
//! The pygame-only tests of the Python file (`test_env_var_forces_pygame_
//! backend` asserted "pygame or none", the pygame horn and music tests) have
//! no backend to run against here: the Rust build routes the pygame
//! preference to the null backend, which the first test pins. Tests that
//! faked `sound_lib` streams run against Hearken over its recording backend
//! instead (they ran on the BASS no-sound device until BASS was replaced),
//! with a localhost station standing in for the faked URL stream.
//!
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use ff_core::lane_guide_tone::{register_lane_guide_tone, LANE_GUIDE_TONE_KEY};
use ff_core::music::ALL_MUSIC_TRACKS;
use ff_core::pyrandom::PyRandom;
use freight_fate::audio::*;

use hearken::audio::Call;

use crate::audio_support::{
    hearken_rig, recordings_rig, rig, shipped_music, shipped_sounds, sine_wav, wait_for,
    HearkenRig, IcyServer,
};

/// Every facade call must be safe regardless of backend.
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
    a.stop_world();
    a.stop_music();
    a.shutdown();
}

#[test]
fn test_hearken_backend_selected_by_default() {
    let mut r = rig();
    assert_eq!(r.engine.backend_name(), "hearken");
    assert!(r.engine.enabled());
    exercise(&mut r.engine);
    // A preference saved before Hearken still names BASS: it gets Hearken.
    let mut saved = AudioEngine::from_preference("bass");
    assert_eq!(saved.backend_name(), "hearken");
    saved.shutdown();
}

#[test]
fn test_env_var_forces_pygame_backend() {
    // The Rust build has no pygame mixer: the preference lands on the
    // null backend, and every call stays safe.
    let _guard = crate::audio_support::audio_lock();
    let mut a = AudioEngine::from_preference("pygame");
    assert_eq!(a.backend_name(), "none");
    assert!(!a.enabled());
    exercise(&mut a);
}

#[test]
fn test_fallback_to_the_null_backend_keeps_every_call_safe() {
    // What the facade builds when asked for no backend at all: the null one, with
    // the whole surface still callable.
    let _guard = crate::audio_support::audio_lock();
    let mut a = AudioEngine::with_backend(Box::new(NullBackend::new()));
    assert_eq!(a.backend_name(), "none");
    exercise(&mut a);
}

#[test]
fn test_engine_freq_mult_mapping() {
    assert_eq!(engine_freq_mult(ENGINE_RPM_IDLE), 1.0);
    assert!((engine_freq_mult(ENGINE_RPM_MAX) - ENGINE_FREQ_MAX_MULT).abs() < 1e-9);
    assert_eq!(engine_freq_mult(0.0), 1.0); // clamped below idle
    assert_eq!(engine_freq_mult(99_999.0), ENGINE_FREQ_MAX_MULT); // clamped above redline
    let mid = engine_freq_mult((ENGINE_RPM_IDLE + ENGINE_RPM_MAX) / 2.0);
    assert!((mid - (1.0 + ENGINE_FREQ_MAX_MULT) / 2.0).abs() < 1e-9);
}

#[test]
fn test_engine_band_weights_edges_blend_and_pure_zones() {
    let natives: Vec<f64> = ENGINE_BANDS.iter().map(|(_k, rpm)| *rpm).collect();
    let n = natives.len();
    let solo = |index: usize| {
        let mut expected = vec![0.0; n];
        expected[index] = 1.0;
        expected
    };
    // Outside the ring the nearest band carries alone.
    assert_eq!(engine_band_weights(0.0, &natives), solo(0));
    assert_eq!(engine_band_weights(natives[0], &natives), solo(0));
    assert_eq!(engine_band_weights(natives[n - 1], &natives), solo(n - 1));
    assert_eq!(engine_band_weights(9999.0, &natives), solo(n - 1));
    // At the geometric midpoint of a gap exactly two bands blend equal-power.
    let mid = (natives[1] * natives[2]).sqrt();
    let w = engine_band_weights(mid, &natives);
    assert!(w[1] > 0.0 && w[2] > 0.0);
    assert_eq!(w.iter().filter(|x| **x > 0.0).count(), 2);
    assert!((w.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-6);
    // Just off a native rpm the band is PURE -- the crossfade windows are
    // narrow, so a cut near its recorded speed never shares the mix (this
    // is what kills both the formant smear and the two-pitch beat).
    for (i, native) in natives[..n - 1].iter().enumerate() {
        assert_eq!(engine_band_weights(native + 10.0, &natives), solo(i));
        if i > 0 {
            assert_eq!(engine_band_weights(native - 10.0, &natives), solo(i));
        }
    }
    // A sweep never needs more than a bounded stretch from any sounding
    // band: wherever a band has weight, rpm/native stays inside the
    // safety clamps.
    let mut rpm = 300.0;
    while rpm < 2400.0 {
        for (weight, native) in engine_band_weights(rpm, &natives).iter().zip(&natives) {
            if *weight > 1e-6 && natives[0] <= rpm && rpm <= natives[n - 1] {
                let ratio = rpm / native;
                assert!((ENGINE_BAND_RATE_MIN..=ENGINE_BAND_RATE_MAX).contains(&ratio));
            }
        }
        rpm += 7.0;
    }
}

#[test]
fn test_engine_load_gain_keeps_audible_load_contour() {
    // Floor raised off the old 0.55 so coasting is not too quiet, but the
    // span stays wide enough to hear effort changes and the
    // automatic-shift unload.
    assert!((engine_load_gain(-1.0) - 0.68).abs() < 1e-9);
    assert!((engine_load_gain(0.0) - 0.68).abs() < 1e-9);
    assert!((engine_load_gain(0.45) - 0.824).abs() < 1e-9);
    assert!(engine_load_gain(0.08) < engine_load_gain(0.7));
    assert_eq!(engine_load_gain(1.0), 1.0);
    assert_eq!(engine_load_gain(2.0), 1.0);
}

#[test]
fn test_split_volume_settings_apply_to_silent_backend() {
    let mut backend = NullBackend::new();
    backend.set_volumes(
        &VolumeUpdate::default()
            .master(0.8)
            .sfx(0.7)
            .music(0.6)
            .weather(0.5)
            .engine(0.4)
            .ui(0.9),
    );
    let buses = backend.buses();
    assert_eq!(buses.master, 0.8);
    assert_eq!(buses.sfx, 0.7);
    assert_eq!(buses.music, 0.6);
    assert_eq!(buses.weather, 0.5);
    assert_eq!(buses.engine, 0.4);
    assert_eq!(buses.ui, 0.9);
}

#[test]
fn test_sound_lookup_prefers_ogg_when_available() {
    // asset_bytes answers from the pack on clean clones and from the
    // loose tree on builder machines; the extension preference holds
    // either way. Neither one here means there is nothing to prefer.
    if !shipped_sounds() {
        return;
    }
    for key in [
        "weather/rain_light",
        "weather/snow_wind",
        "vehicle/brake_air",
        "vehicle/brake_release",
        "vehicle/brake_set",
        "vehicle/horn",
        "vehicle/gear_shift",
        "vehicle/road",
    ] {
        let found = asset_bytes(key, SFX_EXTENSIONS);
        let (_data, ext) = found.unwrap_or_else(|| panic!("{key}"));
        assert_eq!(ext, "ogg", "{key}");
    }
}

#[test]
fn test_main_menu_ambience_ships_in_the_pack() {
    if !shipped_sounds() {
        return;
    }
    for key in ff_core::music::MENU_AMBIENCE {
        assert!(asset_bytes(key, SFX_EXTENSIONS).is_some(), "{key}");
    }
}

#[test]
fn test_engine_recordings_resolve_for_the_ring_and_one_shots() {
    // Looping beds may resolve to WAV (lossy edges break loop seams --
    // tools/fix_loop_seams.py); the licensed overlay's file wins where
    // present. One-shots stay ogg.
    if !shipped_sounds() {
        return;
    }
    let idle = asset_bytes("engine/idle", SFX_EXTENSIONS).unwrap().1;
    assert!(idle == "ogg" || idle == "wav");
    assert_eq!(
        asset_bytes("engine/start", SFX_EXTENSIONS).unwrap().1,
        "ogg"
    );
    assert_eq!(
        asset_bytes("engine/shutdown", SFX_EXTENSIONS).unwrap().1,
        "ogg"
    );
}

/// The clip's real length, decoded through Hearken (the Python test used
/// soundfile; this one used BASS until BASS was replaced).
fn shipped_duration_s(key: &str) -> Option<f64> {
    let (data, _ext) = asset_bytes(key, SFX_EXTENSIONS).unwrap_or_else(|| panic!("{key}"));
    hearken::audio::probe_clip(data).ok()?.length_s
}

#[test]
fn test_engine_start_recording_is_short_one_shot() {
    if !shipped_sounds() {
        return;
    }
    assert!(shipped_duration_s("engine/start").unwrap() <= 4.25);
}

#[test]
fn test_vehicle_horn_and_shift_recordings_are_short_one_shots() {
    if !shipped_sounds() {
        return;
    }
    assert!(shipped_duration_s("vehicle/horn").unwrap() <= 1.0);
    assert!(shipped_duration_s("vehicle/gear_shift").unwrap() <= 0.8);
}

#[test]
fn test_asset_length_matches_a_real_decode_of_the_same_clip() {
    // A one-shot is handed to the mixer without a handle, so the only way
    // to know when it stops sounding is to measure the clip. Read from
    // the container's own headers -- no decoder, no audio device -- and
    // cross-checked here against an actual decode.
    if !shipped_sounds() {
        return;
    }
    for key in [
        "driver/yawn",
        "events/spike_strip",
        "vehicle/signal_tone",
        "vehicle/turn_signal_off",
        "vehicle/bar_solid",
    ] {
        let decoded = shipped_duration_s(key).unwrap();
        assert!(
            (asset_length_s(key) - decoded).abs() <= 0.01,
            "{key}: header {} vs decode {decoded}",
            asset_length_s(key)
        );
    }
}

#[test]
fn test_asset_length_covers_synthesized_cues_and_shrugs_at_unknown_keys() {
    // The Python test registered the enforcement signature; the lane guide
    // tone is a synthesized cue this build carries.
    register_lane_guide_tone();
    assert!(
        asset_length_s(LANE_GUIDE_TONE_KEY) > 0.0,
        "a generated cue has a length too"
    );
    assert_eq!(asset_length_s("nothing/at_all"), 0.0);
}

#[test]
fn test_hearken_music_never_loops_catalog_tracks() {
    // Every catalog track has to open, which needs the music pack: with
    // only a pointer there is no track to open and no looping to check.
    if !shipped_music() {
        return;
    }
    let mut r = hearken_rig();
    assert!(r.hearken().music_sound().is_none());
    for track in ALL_MUSIC_TRACKS.iter() {
        r.engine.play_music_with(&track.key, 123);
        r.hearken()
            .music_sound()
            .unwrap_or_else(|| panic!("{} did not open", track.key));
        let looping = r.rec.calls().into_iter().rev().find_map(|call| match call {
            Call::Play { key, looping, .. } if key == track.key => Some(looping),
            _ => None,
        });
        assert_eq!(looping, Some(false), "{}", track.key);
        r.engine.stop_music_with(0);
    }
}

/// Frames pass (each one a backend update) until `done`, for up to five
/// seconds.
fn frames_until(r: &mut HearkenRig, mut done: impl FnMut(&HearkenRig) -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        r.engine.update(0.01);
        if done(r) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done(r)
}

/// The `SetMixRamped`/`SetMix` gain ramps sent to `channel`, in order.
fn gain_ramps(r: &HearkenRig, channel: u64) -> Vec<(f32, u32)> {
    r.rec
        .calls()
        .into_iter()
        .filter_map(|call| match call {
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
            _ => None,
        })
        .collect()
}

#[test]
fn test_hearken_radio_stream_uses_url_stream() {
    let options = radio_stream_options();
    assert_eq!(options.buffer.as_millis(), u128::from(RADIO_NET_BUFFER_MS));
    assert_eq!(
        options.prebuffer.as_millis(),
        u128::from(RADIO_NET_BUFFER_MS * RADIO_NET_PREBUF_PERCENT / 100)
    );
    let mut r = hearken_rig();
    let server = IcyServer::start(sine_wav(6.0, 1), None);
    r.engine.play_radio_stream_with(&server.url, 321).unwrap();
    let hearken = r.hearken();
    assert!(hearken.music_track().is_none()); // the connect happens off-thread
    assert_eq!(hearken.radio_connecting_url(), Some(server.url.as_str()));
    // Collected on a frame once it plays; never inside the call.
    assert!(
        frames_until(&mut r, |r| r.hearken().music_track().is_some()),
        "the station never opened"
    );
    // One tune: a Shoutcast v1 station is called once more over a plain
    // socket after the HTTP client balks at `ICY 200`, and never again.
    let calls = server.connections();
    assert!((1..=2).contains(&calls), "{calls} connections");
    assert_eq!(r.hearken().music_track(), Some(server.url.as_str()));
    assert!(r.engine.music_playing());
    for _ in 0..20 {
        r.engine.update(0.01);
    }
    assert_eq!(server.connections(), calls, "the station was called again");
    // Started from silence, then faded in over the requested fade only once
    // it was audible, so the ramp protects the first sample.
    let feed = r.rec.channel_of("feed").expect("the station's feed");
    let Some(Call::Play { mix, looping, .. }) = r
        .rec
        .calls()
        .into_iter()
        .find(|call| matches!(call, Call::Play { channel, .. } if *channel == feed))
    else {
        unreachable!()
    };
    assert!(looping);
    assert_eq!(mix.gain, 0.0);
    let level = r.engine.buses().music_level() as f32;
    let ramps = gain_ramps(&r, feed);
    assert_eq!(ramps.last(), Some(&(level, 321)), "{ramps:?}");
    r.engine.stop_music_with(0);
}

#[test]
fn test_hearken_radio_names_freight_fate_to_the_station() {
    let mut r = hearken_rig();
    let server = IcyServer::start(sine_wav(2.0, 1), None);
    r.engine.play_radio_stream_with(&server.url, 0).unwrap();
    assert!(
        // Only the request's head matters here, and a loaded test machine
        // may take a while to dial, or dial twice.
        wait_for(Duration::from_secs(15), || server.connections() >= 1
            && !server.last_request().is_empty()),
        "the station was never called"
    );
    let request = server.last_request();
    let agent = format!(
        "User-Agent: FreightFate/{} (+https://orinks.net/freight-fate)\r\n",
        freight_fate::app::version()
    );
    assert!(request.contains(&agent), "{request}");
    assert!(!request.contains("Hearken"), "{request}");
    r.engine.stop_music_with(0);
}

#[test]
fn test_hearken_engine_wobble_meanders_and_shapes_the_ring_at_its_rate() {
    // Anti-repetition: each band's rate and gain take a slow bounded
    // random walk so a seam-clean loop's period is never exactly fixed --
    // the ear cannot lock onto the recurrence. Rate stays within ~5 cents.
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine
        .set_volumes(&VolumeUpdate::default().master(1.0).engine(1.0));
    r.hearken_mut().set_wobble_rng(PyRandom::new_from_i64(7));
    r.engine.engine_start_with(false);
    if r.hearken().engine_bands().len() < 2 {
        eprintln!("the multisample ring is not installed; skipping");
        return;
    }
    for _ in 0..120 {
        // ~2 s of frames
        r.engine.update(1.0 / 60.0);
    }
    // Past the resume fade: intro gain is 1, so the level is the load alone.
    let h = r.hearken();
    assert_eq!(h.engine_intro_gain(), 1.0);
    let [rate_walk, gain_walk] = h.engine_wobble()[1]; // the 950 band's walks
    assert!(rate_walk != 0.0 && rate_walk.abs() <= ENGINE_WOBBLE_RATE_MAX);
    assert!(gain_walk != 0.0 && gain_walk.abs() <= ENGINE_WOBBLE_GAIN_MAX);

    r.engine.set_engine_rpm_with(950.0, 0.0);
    let band = r.hearken().engine_bands()[1]; // the 950 cut, pure at its native rpm
    assert_eq!(band.native, 950.0);
    // At its own rpm a band plays at its recorded rate, times the walk.
    assert!((band.last_rate - (1.0 + rate_walk)).abs() < 1e-6);
    let base_level = engine_load_gain(0.0);
    assert!((band.last_volume - base_level * (1.0 + gain_walk)).abs() < 1e-9);
}

#[test]
fn test_hearken_radio_stream_recreates_a_stalled_stream() {
    // Re-tuning the SAME url must rebuild a dead connection (the dock bed
    // or a network stall killed it); only a live stream is allowed to
    // dedupe.
    let mut r = hearken_rig();
    let server = IcyServer::start(sine_wav(6.0, 1), None);
    r.engine.play_radio_stream_with(&server.url, 100).unwrap();
    assert!(frames_until(&mut r, |r| r.engine.music_playing()));
    // One tune (a Shoutcast v1 station takes two sockets to call).
    let calls = server.connections();
    assert!(calls >= 1);
    // Live: the same url is a no-op.
    r.engine.play_radio_stream_with(&server.url, 100).unwrap();
    for _ in 0..20 {
        r.engine.update(0.01);
    }
    assert_eq!(server.connections(), calls);
    // The station's audio runs out -- a dead station -- once its buffer is
    // played (by hand here: the recording backend plays nothing) ...
    let feed = r.hearken().radio_stream().unwrap().feed();
    let mut out = vec![[0.0f32; 2]; 4_096];
    assert!(frames_until(&mut r, |r| {
        feed.read(&mut out);
        !r.engine.music_playing()
    }));
    // ... and the retune reconnects.
    r.engine.play_radio_stream_with(&server.url, 100).unwrap();
    assert!(frames_until(&mut r, |r| r.engine.music_playing()));
    assert!(
        server.connections() > calls,
        "the retune did not call again"
    );
    r.engine.stop_music_with(0);
}

#[test]
fn test_hearken_engine_model_matches_available_cuts_on_the_facade() {
    // With the multisample cuts installed the engine comes up as the
    // crossfade ring; a clean clone (synthesized engine/idle only) falls
    // back to the single pitched loop. Either way rpm tracking must be
    // safe.
    let Some(mut r) = recordings_rig() else {
        return;
    };
    let have_all_cuts = ENGINE_BANDS
        .iter()
        .all(|(key, _rpm)| asset_bytes(key, SFX_EXTENSIONS).is_some());
    r.engine.engine_start();
    assert!(r.engine.engine_running());
    let h = r.hearken();
    if have_all_cuts {
        assert_eq!(h.engine_bands().len(), ENGINE_BANDS.len());
        assert!(h.engine_sound().is_none());
    } else {
        assert!(h.engine_bands().is_empty());
        assert!(h.engine_sound().is_some());
    }
    // rate targets follow RPM; repeated slides must be safe
    for rpm in [600.0, 1100.0, 1800.0, 2200.0, 900.0] {
        r.engine.set_engine_rpm_with(rpm, 0.7);
    }
    r.engine.engine_stop();
    assert!(!r.engine.engine_running());
    let h = r.hearken();
    assert!(h.engine_sound().is_none());
    assert!(h.engine_bands().is_empty());
}

#[test]
fn test_engine_voice_setting_switches_models_live() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    let have_all_cuts = ENGINE_BANDS
        .iter()
        .all(|(key, _rpm)| asset_bytes(key, SFX_EXTENSIONS).is_some());
    r.engine.set_engine_voice(true); // classic
    r.engine.engine_start_with(false);
    assert!(r.hearken().engine_bands().is_empty());
    assert!(r.hearken().engine_sound().is_some()); // the classic pitched loop
    if have_all_cuts {
        r.engine.set_engine_voice(false); // back to real, live, mid-run
        assert!(r.engine.engine_running());
        assert_eq!(r.hearken().engine_bands().len(), ENGINE_BANDS.len());
        assert!(r.hearken().engine_sound().is_none());
    }
    r.engine.engine_stop();
}

#[test]
fn test_both_1600_jake_cuts_ship() {
    // The jake A/B needs both cuts shipped -- the routing has nothing to
    // route to otherwise. (The Python test looked in the loose tree; the
    // Rust checkout carries them in the pack.)
    if !shipped_sounds() {
        return;
    }
    assert!(asset_bytes(JAKE_RECORDED_KEY, SFX_EXTENSIONS).is_some());
    assert!(asset_bytes(JAKE_CLASSIC_KEY, SFX_EXTENSIONS).is_some());
}

#[test]
fn test_jake_voice_setting_routes_the_synth_key_and_applies_live() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    // has_asset resolves the key the player-facing catalog and the real
    // drive both use -- the routing must be invisible to callers.
    assert!(r.engine.has_asset(JAKE_RECORDED_KEY));
    r.engine.set_jake_voice(true); // classic, before anything is sounding
    assert!(r.engine.has_asset(JAKE_RECORDED_KEY));

    r.engine.set_jake_voice(false); // real again
    r.engine
        .start_loop_with(CH_JAKE, JAKE_RECORDED_KEY, 0.5, 300);
    let entry = r.engine.backend().loop_entry(CH_JAKE).unwrap();
    assert_eq!(entry.0, JAKE_RECORDED_KEY);

    r.engine.set_jake_voice(true); // classic, live, mid-growl
    let entry = r.engine.backend().loop_entry(CH_JAKE).unwrap();
    assert_eq!(entry.0, JAKE_CLASSIC_KEY);
    assert_eq!(entry.1, 0.5); // the level carries across the re-voice

    r.engine.set_jake_voice(false); // back to real, live
    let entry = r.engine.backend().loop_entry(CH_JAKE).unwrap();
    assert_eq!(entry.0, JAKE_RECORDED_KEY);

    r.engine.stop_loop(CH_JAKE);
}

#[test]
fn test_the_jake_toggle_re_voices_whatever_band_is_sounding() {
    // A loop on a NON-jake channel is none of this toggle's business;
    // every jake band is (see the Python docstring's history).
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.set_jake_voice(false);
    // Asking for the 1800 band on "real" sounds the one recording -- that
    // is the routing. What this pins is that the toggle re-voices it.
    r.engine
        .start_loop_with(CH_JAKE, "engine/jake_1800", 0.4, 300);
    assert_eq!(
        r.engine.backend().loop_entry(CH_JAKE).unwrap().0,
        JAKE_RECORDED_KEY
    );
    r.engine.set_jake_voice(true);
    let entry = r.engine.backend().loop_entry(CH_JAKE).unwrap();
    assert_eq!(
        entry.0, JAKE_CLASSIC_KEY,
        "the 1800 band kept the old voice"
    );
    assert_eq!(entry.1, 0.4);
    r.engine.stop_loop(CH_JAKE);
}

#[test]
fn test_classic_voice_prefers_the_original_recording() {
    // Settings "classic" promises the 1.8.x engine. Its cut ships under
    // its own key (engine_classic/idle) precisely because the licensed
    // overlay owns engine/idle -- with the rebuilt bank installed the
    // shared key is the rebuilt cut, and classic must not quietly follow
    // it.
    assert!(asset_bytes(ENGINE_CLASSIC_LOOP_KEY, SFX_EXTENSIONS).is_some());
    let mut r = hearken_rig();
    r.hearken_mut().record_requested_keys(true);
    r.engine.set_engine_voice(true); // classic
    r.engine.engine_start_with(false);
    let h = r.hearken();
    assert!(h
        .requested_keys()
        .iter()
        .any(|k| k == ENGINE_CLASSIC_LOOP_KEY));
    assert!(h.engine_bands().is_empty());
    assert!(h.engine_sound().is_some());
    r.engine.engine_stop();
}

#[test]
fn test_hearken_engine_falls_back_to_pitched_loop_without_cuts_on_the_facade() {
    // A clean clone carries only the synthesized engine/idle: the ring
    // cannot form, and the legacy single pitched loop must come up
    // instead.
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.hearken_mut().set_key_filter(Some(Box::new(|key: &str| {
        !(is_engine_band_key(key) && key != "engine/idle")
    })));
    r.engine.engine_start();
    assert!(r.engine.engine_running());
    assert!(r.hearken().engine_bands().is_empty());
    assert!(r.hearken().engine_sound().is_some());
    for rpm in [600.0, 1500.0, 2200.0] {
        r.engine.set_engine_rpm_with(rpm, 0.5);
    }
    r.engine.engine_stop();
}

#[test]
fn test_silent_engine_start_skips_the_ignition_crank() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.hearken_mut().record_requested_keys(true);
    r.engine.engine_start_with(false); // resume / menu-return path
    assert!(r.engine.engine_running());
    let h = r.hearken();
    assert!(!h.requested_keys().iter().any(|k| k == "engine/start")); // the crank must not replay
                                                                      // The engine voice still comes up: the ring when the cuts are
                                                                      // installed, the legacy loop otherwise.
    assert!(!h.engine_bands().is_empty() || h.engine_sound().is_some());
    r.engine.engine_stop();
}

#[test]
fn test_deliberate_engine_start_plays_crank_and_arms_crossfade() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.hearken_mut().record_requested_keys(true);
    r.engine.engine_start(); // deliberate ignition
    let h = r.hearken();
    assert!(h.requested_keys().iter().any(|k| k == "engine/start"));
    // A crank fade-out, the loop fade-in, and the post-handoff load
    // settle are scheduled; the loop starts silent so the ignition is not
    // drowned out, and its load is boosted to full so it meets the crank
    // tail without a dip.
    assert_eq!(h.fade_count(), 3);
    assert_eq!(h.engine_intro_gain(), 0.0);
    assert_eq!(h.engine_intro_load(), 1.0);
    r.engine.engine_stop();
}

#[test]
fn test_engine_start_crossfade_ramps_the_loop_up_over_time() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.engine_start();
    assert_eq!(r.hearken().engine_intro_gain(), 0.0);
    // Advance well past the clip length plus the crossfade window.
    for _ in 0..400 {
        r.engine.update(0.05);
    }
    let h = r.hearken();
    assert_eq!(h.engine_intro_gain(), 1.0);
    assert_eq!(h.fade_count(), 0); // finished fades are dropped
    r.engine.engine_stop();
}

#[test]
fn test_engine_starting_true_during_crank_then_clears() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.engine_start(); // deliberate ignition
    assert!(r.engine.engine_starting()); // loop has not taken over yet
                                         // Advance past the clip length plus the crossfade window.
    for _ in 0..400 {
        r.engine.update(0.05);
    }
    assert!(!r.engine.engine_starting()); // loop has taken over
    assert!(r.engine.engine_running());
    r.engine.engine_stop();
}

#[test]
fn test_silent_engine_start_is_never_marked_starting() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.engine_start_with(false); // resume / menu-return path
    assert!(!r.engine.engine_starting()); // no crank, nothing to gate on
    r.engine.engine_stop();
}

#[test]
fn test_engine_stop_clears_engine_starting() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.engine_start();
    assert!(r.engine.engine_starting());
    r.engine.engine_stop(); // stopping mid-crank must leave clean state
    assert!(!r.engine.engine_starting());
}

#[test]
fn test_null_backend_is_never_engine_starting() {
    assert!(!NullBackend::new().engine_starting());
}

#[test]
fn test_engine_stop_clears_pending_fades() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.engine_start();
    assert!(r.hearken().fade_count() > 0);
    r.engine.engine_stop();
    let h = r.hearken();
    assert_eq!(h.fade_count(), 0);
    assert_eq!(h.engine_intro_gain(), 1.0);
}

#[test]
fn test_road_noise_loop_tracks_speed() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.set_road_noise(30.0);
    let entry = r.engine.backend().loop_entry(CH_ROAD).expect("road loop");
    assert_eq!(entry.0, "vehicle/road");
    assert_eq!(entry.1, 1.0);
    r.engine.set_road_noise(0.0);
    assert!(r.engine.backend().loop_entry(CH_ROAD).is_none());
}

#[test]
fn test_new_context_loops_enter_mixer_at_full_gain() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.set_wind(2.0);
    let entry = r.engine.backend().loop_entry(CH_WEATHER_B).unwrap();
    assert_eq!(entry.0, "weather/wind");
    assert_eq!(entry.1, 1.0);
    r.engine.set_ambient(Some("poi/facility_gate"));
    let entry = r.engine.backend().loop_entry(CH_AMBIENT).unwrap();
    assert_eq!(entry.0, "poi/facility_gate");
    assert_eq!(entry.1, 1.0);
    assert_eq!(ENGINE_LOOP_GAIN, 1.0);
}

#[test]
fn test_horn_uses_reserved_loop_slot() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.horn_start();
    assert_eq!(
        r.engine.backend().loop_entry(CH_HORN).unwrap().0,
        "vehicle/horn"
    );
    r.engine.horn_stop();
    assert!(r.engine.backend().loop_entry(CH_HORN).is_none());
}

#[test]
fn test_stop_world_drops_the_liquid_surge_wash() {
    // A tank still sloshing when the pause menu opens must not be heard
    // through it: stop_world is what the pause menu calls, and the surge
    // wash is a gated world loop like the jake and the air, not music or a
    // menu sound that should survive a pause.
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine
        .start_loop_with(CH_SURGE, "vehicle/liquid_wash", 0.5, 90);
    assert!(r.engine.backend().loop_entry(CH_SURGE).is_some());
    r.engine.stop_world();
    assert!(r.engine.backend().loop_entry(CH_SURGE).is_none());
}

#[test]
fn test_stop_world_drops_the_scale_bed_and_lane_guide_tone() {
    // Both are plain per-frame loops with no dead-man's switch of their own
    // (unlike the alert and siren): leaving the drive with either still
    // running left it playing into the main menu until the game closed.
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine
        .start_loop_with(CH_SCALE, "poi/weigh_station_lane", 0.5, 90);
    r.engine
        .start_loop_with(CH_LANE_GUIDE, "ui/menu_select", 0.5, 90);
    assert!(r.engine.backend().loop_entry(CH_SCALE).is_some());
    assert!(r.engine.backend().loop_entry(CH_LANE_GUIDE).is_some());
    r.engine.stop_world();
    assert!(r.engine.backend().loop_entry(CH_SCALE).is_none());
    assert!(r.engine.backend().loop_entry(CH_LANE_GUIDE).is_none());
}

#[test]
fn test_hearken_horn_sustains_then_rings_out_on_release() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.horn_start();
    r.engine.horn_start(); // key autorepeat must not stack a second horn
    assert_eq!(r.hearken().sustain_channels(), vec![CH_HORN]);
    let sound = r.hearken().loop_sound(CH_HORN).unwrap();
    r.engine.horn_stop();
    // The loop is released and the channel handed off, but the sound keeps
    // playing its release tail.
    let h = r.hearken();
    assert!(h.sustain_channels().is_empty());
    assert!(r.engine.backend().loop_entry(CH_HORN).is_none());
    assert!(h.pool().is_playing(sound));
}

#[test]
fn test_hearken_horn_press_during_release_tail_does_not_stack() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.horn_start();
    r.engine.horn_stop(); // tail is now ringing out on the channel
    let tail = r
        .hearken()
        .releasing_entry(CH_HORN)
        .expect("a ringing tail");
    assert!(r.hearken().pool().is_playing(tail.1));
    let plays = r.rec.played().len();
    r.engine.horn_start(); // pressed again mid-tail: must be ignored, not stacked
    let h = r.hearken();
    assert!(h.sustain_channels().is_empty()); // no new held loop
    assert!(r.engine.backend().loop_entry(CH_HORN).is_none());
    assert_eq!(h.releasing_entry(CH_HORN), Some(tail)); // same tail, no new sound
    assert_eq!(r.rec.played().len(), plays); // nothing new played
}

#[test]
fn test_hearken_radio_failed_connect_reports_on_the_next_call() {
    // A connect that never produces audio fails on the NEXT call for the
    // same URL -- when the driving state's reconnect loop retries -- and
    // a later tune back starts a fresh attempt.
    let mut r = hearken_rig();
    // A server that takes each connection and hangs up before a byte of
    // audio. A freed port was flaky: another process can take it, and on
    // Windows a connect to a closed port can take seconds to be refused.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/dead",
        listener.local_addr().unwrap().port()
    );
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            drop(stream);
        }
    });
    r.engine.play_radio_stream_with(&url, 100).unwrap(); // silence: the caller's retry cue
    let h = r.hearken_mut();
    h.settle_radio(Duration::from_secs(10));
    assert_eq!(h.radio_failed_url(), Some(url.as_str()));
    assert!(h.radio_connecting_url().is_none());
    let err = r.engine.play_radio_stream_with(&url, 100).unwrap_err();
    assert_eq!(err.message, "radio stream unavailable");
    // The latch is spent: the third call is a fresh connect.
    r.engine.play_radio_stream_with(&url, 100).unwrap();
    let h = r.hearken_mut();
    h.settle_radio(Duration::from_secs(10));
    assert_eq!(h.radio_failed_url(), Some(url.as_str()));
}

#[test]
fn test_play_music_file_reads_the_file_and_refuses_what_it_cannot() {
    // Over the device the environment gives (silent here), so the decode is
    // Hearken's own: a recording backend would take any bytes.
    let mut r = rig();
    let tmp = tempfile::tempdir().unwrap();
    let track = tmp.path().join("song.wav");
    std::fs::write(&track, sine_wav(1.0, 2)).unwrap();
    let path = track.to_str().unwrap().to_string();
    r.engine.play_music_file_with(&path, 50).unwrap();
    let h = r.engine.hearken().unwrap();
    assert_eq!(h.music_track(), Some(format!("file:{path}").as_str()));
    assert!(h.music_sound().is_some());
    let missing = tmp.path().join("missing.wav").to_str().unwrap().to_string();
    let err = r.engine.play_music_file(&missing).unwrap_err();
    assert_eq!(err.message, format!("could not read {missing}"));
    let garbage = tmp.path().join("garbage.wav");
    std::fs::write(&garbage, b"not audio at all").unwrap();
    let garbage = garbage.to_str().unwrap().to_string();
    let err = r.engine.play_music_file(&garbage).unwrap_err();
    assert_eq!(err.message, format!("could not decode {garbage}"));
    // A refused file left the channel empty (the previous track was
    // faded out before the attempt, as in the Python backend).
    assert!(r.engine.hearken().unwrap().music_track().is_none());
}

/// The `(key, volume)` plays a recording backend saw.
type Played = Rc<RefCell<Vec<(String, f64)>>>;

/// A recording backend: the facade over it, its plays recorded, and a
/// fake asset universe (the Python `_bank_facade`).
struct RecordingBackend {
    buses: Buses,
    played: Played,
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
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn play(&mut self, key: &str, volume: f64, _pan: f64) {
        self.played.borrow_mut().push((key.to_string(), volume));
    }
}

fn bank_facade(present: &[&str]) -> (AudioEngine, Played) {
    let played = Rc::new(RefCell::new(Vec::new()));
    let mut a = AudioEngine::with_backend(Box::new(RecordingBackend {
        buses: Buses::new(),
        played: Rc::clone(&played),
    }));
    let present: Vec<String> = present.iter().map(|k| k.to_string()).collect();
    a.set_asset_probe(Some(Box::new(move |key: &str| {
        present.iter().any(|p| p == key)
    })));
    (a, played)
}

#[test]
fn test_play_bank_cycles_every_cut_without_immediate_repeats() {
    let cuts = ["vehicle/hit_01", "vehicle/hit_02", "vehicle/hit_03"];
    let (mut a, played) = bank_facade(&cuts);
    for _ in 0..30 {
        a.play_bank("vehicle/hit", "vehicle/fallback");
    }
    let played = played.borrow();
    let keys: Vec<&str> = played.iter().map(|(k, _v)| k.as_str()).collect();
    let mut seen: Vec<&str> = keys.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, cuts);
    // Shuffled full cycles: every block of three is a permutation of the
    // bank, and no cut ever lands twice in a row across cycle seams.
    for block in keys.chunks(3) {
        let mut block: Vec<&str> = block.to_vec();
        block.sort_unstable();
        assert_eq!(block, cuts);
    }
    assert!(keys.windows(2).all(|w| w[0] != w[1]));
    // The per-trigger level jitter stays inside its band.
    assert!(played.iter().all(|(_k, vol)| (0.85..=1.17).contains(vol)));
}

#[test]
fn test_play_bank_falls_back_to_the_classic_cue() {
    let (mut a, played) = bank_facade(&[]);
    a.play_bank_with("vehicle/hit", "vehicle/fallback", 0.6, 0.0);
    assert_eq!(
        *played.borrow(),
        vec![("vehicle/fallback".to_string(), 0.6)] // exact volume: no jitter on fallback
    );
}

#[test]
fn test_has_asset_caches_the_lookup() {
    let (mut a, _played) = bank_facade(&["vehicle/ebrake"]);
    assert!(a.has_asset("vehicle/ebrake"));
    assert!(!a.has_asset("vehicle/not_there"));
    a.set_asset_probe(Some(Box::new(|_key: &str| false)));
    assert!(a.has_asset("vehicle/ebrake")); // cached, not re-probed
}

#[test]
fn test_hearken_one_shots_play_to_their_end() {
    // A one-shot is handed to the pool and nobody keeps its handle; the
    // pool must keep it sounding until playback ends, or every one-shot
    // (menu sounds, horn, warnings) is cut off instantly.
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.play("ui/menu_move");
    let channel = r.channel("ui/menu_move");
    let sound = r
        .hearken()
        .pool()
        .live()
        .into_iter()
        .find(|live| live.key == "ui/menu_move")
        .expect("the one-shot is live in the pool")
        .sound;
    for _ in 0..10 {
        r.engine.update(0.05);
    }
    assert!(r.rec.is_playing(channel)); // the one-shot really started
    assert!(
        r.hearken().pool().is_playing(sound),
        "cut off before its end"
    );
    assert!(!r
        .rec
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Stop { channel: c, .. } if *c == channel)));
    // Once it ends, the pool lets it go.
    r.rec.finish(channel);
    r.engine.update(0.05);
    assert!(!r.hearken().pool().is_playing(sound));
}

#[test]
fn test_hearken_fading_loops_stay_alive_during_fade() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.set_weather_with(Some("weather/rain_light"), 0.8);
    assert!(!r.hearken().loop_channels().is_empty());
    let channel = r.channel("weather/rain_light");
    r.engine.set_weather(None); // 1200 ms fade-out
    assert!(r.hearken().loop_channels().is_empty());
    // The device owns the fade: a stop over 1200 ms, never a cut.
    assert!(r
        .rec
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Stop { channel: c, fade_ms: 1200 } if *c == channel)));
    assert!(!r
        .rec
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Stop { channel: c, fade_ms: 0 } if *c == channel)));
}

#[test]
fn test_hearken_headless_uses_the_silent_backend() {
    // The rig sets SDL_AUDIODRIVER=dummy, which must route Hearken to its
    // silent backend so CI runs the full pipeline without hardware, and
    // must not read as a missing device.
    let mut r = rig();
    assert!(r.engine.enabled());
    let h = r.engine.hearken().unwrap();
    assert_eq!(h.pool().backend_name(), "silent");
    assert!(!h.no_device());
    assert!(!r.engine.take_silence_notice());
}

#[test]
fn test_hearken_road_noise_rate_changes_with_speed() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.set_road_noise(15.0);
    let rate = r.hearken().road_noise_rate().unwrap();
    assert!((rate - 0.85).abs() < 1e-6);
}

#[path = "audio_backend_jake.rs"]
mod jake;
