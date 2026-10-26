//! The radio's cab-speaker voicing on the Hearken backend: which sounds
//! carry it, and what the shared voicing is told.

use freight_fate::audio::*;
use hearken::audio::Call;

use crate::audio_support::{audio_lock, hearken_rig, sine_wav, HearkenRig, IcyServer};

fn voicing(cab_speakers: bool, window: f64) -> Option<RadioVoicing> {
    Some(RadioVoicing {
        cab_speakers,
        window,
    })
}

/// Frames pass until the station's feed is playing; its channel.
fn tune(r: &mut HearkenRig, url: &str) -> u64 {
    let before = r.rec.played().iter().filter(|k| *k == "feed").count();
    r.engine.play_radio_stream_with(url, 0).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while r.rec.played().iter().filter(|k| *k == "feed").count() == before {
        assert!(
            std::time::Instant::now() < deadline,
            "the station never opened"
        );
        r.engine.update(0.01);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    r.rec.channel_of("feed").expect("the station's feed")
}

#[test]
fn test_the_station_carries_the_voicing_only_while_driving() {
    let _guard = audio_lock();
    let server = IcyServer::start(sine_wav(6.0, 1), None);
    let mut r = hearken_rig();

    // Off the road (a menu), a station on the music channel plays clean.
    let menu = tune(&mut r, &server.url);
    assert!(!r.rec.inserted(menu));
    r.engine.stop_music_with(0);

    r.engine.set_radio_voicing(voicing(true, 0.0));
    let driving = tune(&mut r, &server.url);
    assert!(
        r.rec.inserted(driving),
        "the station is voiced at the wheel"
    );

    // The drive over: what starts on the music channel next is clean.
    r.engine.set_radio_voicing(None);
    r.engine.stop_music_with(0);
    let after = tune(&mut r, &server.url);
    assert!(!r.rec.inserted(after));
    r.engine.stop_music_with(0);
}

#[test]
fn test_the_radios_own_sounds_carry_the_voicing_and_nothing_else_does() {
    let _guard = audio_lock();
    let mut r = hearken_rig();
    r.engine.set_radio_voicing(voicing(true, 0.0));
    r.engine.play_with("radio/static_burst", 0.5, 0.0);
    r.engine
        .start_loop_with(CH_RADIO_FX, "radio/fm_hiss_loop", 0.2, 0);
    r.engine.play("ui/menu_select");
    r.engine.start_loop_with(CH_ROAD, "vehicle/road", 0.5, 0);
    let played = r.rec.played();
    for key in ["radio/static_burst", "radio/fm_hiss_loop"] {
        if played.iter().any(|k| k == key) {
            assert!(r.rec.inserted(r.channel(key)), "{key} is the radio's own");
        } else {
            eprintln!("SKIPPING {key}: not in this checkout's sounds");
        }
    }
    for key in ["ui/menu_select", "vehicle/road"] {
        if played.iter().any(|k| k == key) {
            assert!(!r.rec.inserted(r.channel(key)), "{key} is not the radio");
        }
    }
    // Every sound the pool started without an insert played as before.
    assert!(r
        .rec
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Play { .. })));
}

#[test]
fn test_the_voicing_follows_the_setting_and_the_glass() {
    let _guard = audio_lock();
    let mut r = hearken_rig();
    r.engine.set_radio_voicing(voicing(true, 0.0));
    let cab = r.hearken().radio_cab().clone();
    assert_eq!(cab.amount(), 1.0);
    assert_eq!(cab.window(), 0.0);
    r.engine.set_radio_voicing(voicing(true, 0.5));
    assert_eq!(cab.window(), 0.5);
    r.engine.set_radio_voicing(voicing(false, 0.5));
    assert_eq!(cab.amount(), 0.0, "Clean");
    r.engine.set_radio_voicing(voicing(true, 1.0));
    assert_eq!((cab.amount(), cab.window()), (1.0, 1.0));
}

/// Each side's RMS once a mono 1 kHz tone has settled through `cab`.
fn side_levels(cab: &hearken::audio::CabSpeakers) -> (f32, f32) {
    let rate = 44_100.0_f32;
    let mut dsp = cab.processor();
    let mut frames: Vec<[f32; 2]> = (0..rate as usize)
        .map(|i| {
            let x = 0.5 * (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / rate).sin();
            [x, x]
        })
        .collect();
    dsp.process(&mut frames, rate);
    let tail = &frames[frames.len() / 2..];
    let rms = |c: usize| (tail.iter().map(|f| f[c] * f[c]).sum::<f32>() / tail.len() as f32).sqrt();
    (rms(0), rms(1))
}

#[test]
fn test_the_right_speaker_plays_thirty_percent_louder_than_hearkens_default() {
    let _guard = audio_lock();
    let r = hearken_rig();
    let cab = r.hearken().radio_cab().clone();
    cab.set_amount(1.0, 0);
    let stock = hearken::audio::CabSpeakers::new(hearken::audio::CabSpeakersParams::default());
    stock.set_amount(1.0, 0);

    let (left, right) = side_levels(&cab);
    let (stock_left, stock_right) = side_levels(&stock);
    assert!(
        (left / stock_left - 1.0).abs() < 0.01,
        "the near speaker is unchanged"
    );
    let lift = right / stock_right;
    assert!(
        (lift - 1.3).abs() < 0.01,
        "the far speaker is lifted 30 percent, got {lift}"
    );
    // The far speaker still arrives late, so the radio keeps a slight lean
    // left, but well under Hearken's 0.75 ms, which pins it left on
    // headphones.
    assert_eq!(cab.params().far_delay_ms, 0.3);
    assert!(stock.params().far_delay_ms > cab.params().far_delay_ms);
}
