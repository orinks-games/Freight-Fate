//! The Hearken ports of `audio_backend_cues.rs`, plus music.
use super::*;

const BLINKER: &str = "vehicle/turn_signal";

#[test]
fn test_hearken_blinker_updates_pan_and_volume_without_restarting() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.play_if_idle(BLINKER, 0.8, -0.6);
    let sound = r.hearken().exclusive_cue_sound(BLINKER).unwrap();
    let channel = r.channel(BLINKER);
    let before = r.rec.last_mix(channel).unwrap();
    assert!((before.pan + 0.6).abs() < 1e-6);
    r.engine.update_cue(BLINKER, 0.4, 0.7);
    assert_eq!(r.hearken().exclusive_cue_sound(BLINKER), Some(sound));
    assert_eq!(play_count(&r), 1);
    let after = r.rec.last_mix(channel).unwrap();
    assert!((after.pan - 0.7).abs() < 1e-6);
    assert!((after.gain - before.gain / 2.0).abs() < 1e-6);
}

#[test]
fn test_hearken_blinker_stops_on_release_menu_and_owner_timeout() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    for stop in 0..3 {
        r.engine.play_if_idle(BLINKER, 0.8, -0.6);
        assert!(r.hearken().exclusive_cue_sound(BLINKER).is_some());
        let channel = r.channel(BLINKER);
        assert!(r.rec.is_playing(channel));
        match stop {
            0 => r.engine.release_cue(BLINKER),
            1 => r.engine.stop_world(),
            _ => r.engine.update(CUE_HOLD_TIMEOUT_S + 0.01),
        }
        assert_eq!(r.hearken().exclusive_cue_sound(BLINKER), None);
        assert!(!r.rec.is_playing(channel));
        assert!(!r.engine.cue_held(BLINKER));
    }
}

#[test]
fn test_hearken_blinker_repeat_waits_for_playback_to_finish() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.play_if_idle(BLINKER, 0.5, -0.6);
    let sound = r.hearken().exclusive_cue_sound(BLINKER).unwrap();
    let channel = r.channel(BLINKER);
    for _ in 0..20 {
        r.engine.play_if_idle(BLINKER, 0.5, 0.6);
        assert_eq!(r.hearken().exclusive_cue_sound(BLINKER), Some(sound));
        assert!((r.rec.last_mix(channel).unwrap().pan - 0.6).abs() < 1e-6);
    }
    assert_eq!(play_count(&r), 1);
    // The clip really ends; the pool notices on its next update.
    r.rec.finish(channel);
    r.engine.update(0.01);
    r.engine.play_if_idle(BLINKER, 0.5, 0.6);
    assert_eq!(play_count(&r), 2);
    let next = r.channel(BLINKER);
    assert_ne!(next, channel);
    assert!((r.rec.last_mix(next).unwrap().pan - 0.6).abs() < 1e-6);
    r.engine.shutdown();
    assert_eq!(r.hearken().exclusive_cue_sound(BLINKER), None);
}

#[test]
fn test_hearken_held_alert_lapses_on_its_own_and_cues_latch() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    r.engine.hold_alert("vehicle/bar_solid");
    assert_eq!(
        r.engine.backend().loop_entry(CH_ALERT).unwrap().0,
        "vehicle/bar_solid"
    );
    r.engine.update(ALERT_HOLD_TIMEOUT_S / 2.0);
    assert!(r.engine.backend().loop_entry(CH_ALERT).is_some());
    r.engine.update(ALERT_HOLD_TIMEOUT_S);
    assert!(r.engine.backend().loop_entry(CH_ALERT).is_none());

    r.engine.hold_cue("tick");
    assert!(r.engine.cue_held("tick"));
    r.engine.update(CUE_HOLD_TIMEOUT_S + 0.01);
    assert!(!r.engine.cue_held("tick"));
    r.engine.hold_cue("tick");
    r.engine.release_cue("tick");
    assert!(!r.engine.cue_held("tick"));
}

#[test]
fn test_hearken_engine_pan_is_inherited_and_moves_without_restarting() {
    if !shipped_sounds() {
        return;
    }
    let mut r = hearken_rig();
    for classic in [false, true] {
        r.engine.engine_stop_with(false);
        r.engine.set_engine_voice(classic);
        r.engine.set_engine_pan(-0.6);
        r.engine.engine_start_with(false);
        r.engine.update(1.0);
        r.engine.set_engine_rpm_with(1150.0, 0.7);
        let keys: Vec<&str> = if classic || !all_engine_cuts() {
            vec![if classic {
                ENGINE_CLASSIC_LOOP_KEY
            } else {
                ENGINE_LOOP_KEY
            }]
        } else {
            ENGINE_BANDS.iter().map(|(key, _)| *key).collect()
        };
        let channels: Vec<u64> = keys.iter().map(|key| r.channel(key)).collect();
        let plays = play_count(&r);
        let gains: Vec<f32> = channels
            .iter()
            .map(|c| r.rec.last_mix(*c).unwrap().gain)
            .collect();
        let bands = r.hearken().engine_bands();
        let wobble = r.hearken().engine_wobble();
        for (pan, expected) in [
            (-0.6, -0.6),
            (0.0, 0.0),
            (0.7, 0.7),
            (-2.0, -1.0),
            (2.0, 1.0),
        ] {
            if pan != -0.6 {
                r.engine.set_engine_pan(pan);
            }
            assert_eq!(play_count(&r), plays, "pan must not restart sounds");
            for (i, channel) in channels.iter().enumerate() {
                let mix = r.rec.last_mix(*channel).unwrap();
                assert!((f64::from(mix.pan) - expected).abs() < 1e-6);
                assert_eq!(mix.gain, gains[i]);
            }
            assert_eq!(r.hearken().engine_wobble(), wobble);
            for (before, after) in bands.iter().zip(r.hearken().engine_bands()) {
                assert_eq!(before.last_rate, after.last_rate);
                assert_eq!(before.last_volume, after.last_volume);
            }
        }
        r.engine.engine_stop_with(false);
        r.engine.engine_start_with(false);
        for key in &keys {
            let Call::Play { mix, .. } = play_call(&r, r.channel(key)) else {
                unreachable!()
            };
            assert_eq!(mix.pan, 1.0, "{key} started at the engine's pan");
        }
    }
}

#[test]
fn test_hearken_music_never_loops_and_fades_in_to_the_music_level() {
    classic_music::register();
    let mut r = hearken_rig();
    r.engine
        .set_volumes(&VolumeUpdate::default().master(0.8).music(0.5));
    let tracks = ["classic_menu_theme", "classic_open_road"];
    for track in tracks {
        r.rec.set_length(track, 61.5);
        r.engine.play_music_with(track, 123);
        let channel = r.channel(track);
        let Call::Play {
            looping,
            mix,
            from_s,
            ..
        } = play_call(&r, channel)
        else {
            unreachable!()
        };
        assert!(!looping, "{track} must not loop");
        assert_eq!(mix.gain, 0.0);
        assert_eq!(from_s, 0.0);
        let fade = r.rec.calls().into_iter().find_map(|call| match call {
            Call::SetMixRamped {
                channel: c,
                mix,
                ramps,
            } if c == channel => Some((mix.gain, ramps.gain_ms)),
            _ => None,
        });
        assert_eq!(fade, Some((0.4, 123)));
        assert_eq!(r.hearken().music_track(), Some(track));
        assert!(r.engine.music_playing());
        assert_eq!(r.engine.music_length_s(), Some(61.5));
    }
    // The outgoing track faded over 800 ms when the next one started.
    let first = r.channel(tracks[0]);
    assert!(r.rec.calls().contains(&Call::Stop {
        channel: first,
        fade_ms: 800
    }));
    // The same track again is not restarted.
    let plays = play_count(&r);
    r.engine.play_music(tracks[1]);
    assert_eq!(play_count(&r), plays);
    // A position is honoured, even for the track already playing.
    r.engine.play_music_at(tracks[1], 50, 42.0);
    let Call::Play { from_s, .. } = play_call(&r, r.channel(tracks[1])) else {
        unreachable!()
    };
    assert_eq!(from_s, 42.0);
    r.engine.stop_music_with(0);
    assert!(!r.engine.music_playing());
    assert_eq!(r.engine.music_length_s(), None);
    r.engine.play_music("not_a_track");
    assert_eq!(r.hearken().music_track(), None);
}

#[test]
fn test_hearken_play_music_file_reads_the_file_and_refuses_what_it_cannot() {
    let mut r = hearken_rig();
    let tmp = tempfile::tempdir().unwrap();
    let track = tmp.path().join("song.wav");
    std::fs::write(&track, sine_wav(1.0, 2)).unwrap();
    let path = track.to_str().unwrap().to_string();
    r.engine.play_music_file_with(&path, 50).unwrap();
    let key = format!("file:{path}");
    assert_eq!(r.hearken().music_track(), Some(key.as_str()));
    assert!(r.engine.music_playing());
    let missing = tmp.path().join("missing.wav").to_str().unwrap().to_string();
    let err = r.engine.play_music_file(&missing).unwrap_err();
    assert_eq!(err.message, format!("could not read {missing}"));
    let garbage = tmp.path().join("garbage.wav");
    std::fs::write(&garbage, b"not audio at all").unwrap();
    let garbage = garbage.to_str().unwrap().to_string();
    // What the device would do with these bytes: refuse them.
    r.rec.refuse(&format!("file:{garbage}"));
    let err = r.engine.play_music_file(&garbage).unwrap_err();
    assert_eq!(err.message, format!("could not decode {garbage}"));
    // A refused file left the channel empty: the previous track was faded
    // out before the attempt.
    assert!(r.hearken().music_track().is_none());
    assert!(!r.engine.music_playing());
}
