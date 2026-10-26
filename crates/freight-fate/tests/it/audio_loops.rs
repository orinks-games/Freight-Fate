//! Port of the stream half of `tests/test_audio_loops.py`: the sustain loop
//! on the real horn recording. (The `to_seconds` tests live with the pure
//! half in `ff_core::audio_loops`.) BASS ran the loop with a seek-back sync
//! on the stream; Hearken plays the interior region on repeat, so these pin
//! the region the backend is given and the release that lets it go, over
//! Hearken's recording backend.

use freight_fate::audio::hearken_clips::clip_sample_rate;
use freight_fate::audio::{
    asset_bytes, Audio, SustainLoopSpec, CH_HORN, HORN_LOOP, HORN_LOOP_END, HORN_LOOP_START,
    SFX_EXTENSIONS,
};
use hearken::audio::{Call, LoopRegion};

use crate::audio_support::{recordings_rig, HearkenRig};

/// The region the backend was told to repeat on `channel`.
fn region(r: &HearkenRig, channel: u64) -> Option<LoopRegion> {
    r.rec.calls().into_iter().find_map(|call| match call {
        Call::SetLoopRegion { channel: c, region } if c == channel => Some(region),
        _ => None,
    })
}

fn releases(r: &HearkenRig, channel: u64) -> usize {
    r.rec
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::Release { channel: c, .. } if *c == channel))
        .count()
}

#[test]
fn test_sustain_loop_computes_its_region_from_samples() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    let (data, _ext) = asset_bytes("vehicle/horn", SFX_EXTENSIONS).expect("vehicle/horn");
    // Verified against the shipped 44100 Hz horn asset.
    assert_eq!(clip_sample_rate(&data), Some(44_100.0));
    r.engine
        .start_sustain_loop_with(CH_HORN, "vehicle/horn", HORN_LOOP, 1.0);
    let channel = r.channel("vehicle/horn");
    let region = region(&r, channel).expect("the held region");
    assert_eq!(region.start, f64::from(HORN_LOOP_START) / 44_100.0);
    let end = region.end.expect("a closed region");
    assert_eq!(end, f64::from(HORN_LOOP_END) / 44_100.0);
    assert!(end > region.start);
    assert_eq!(r.hearken().sustain_channels(), vec![CH_HORN]);
    r.engine.stop_loop_with(CH_HORN, 0);
}

#[test]
fn test_sustain_loop_release_is_idempotent() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine
        .start_sustain_loop_with(CH_HORN, "vehicle/horn", HORN_LOOP, 1.0);
    let channel = r.channel("vehicle/horn");
    r.engine.release_sustain_loop(CH_HORN);
    assert!(r.hearken().sustain_channels().is_empty());
    assert_eq!(releases(&r, channel), 1);
    assert!(r.hearken().releasing_entry(CH_HORN).is_some());
    r.engine.release_sustain_loop(CH_HORN); // must not fail a second time
    assert_eq!(releases(&r, channel), 1, "released twice");
    assert!(r.hearken().sustain_channels().is_empty());
    r.engine.stop_loop_with(CH_HORN, 0); // nor when torn down after release
    assert!(r.hearken().releasing_entry(CH_HORN).is_none());
    assert_eq!(releases(&r, channel), 1);
}

#[test]
fn test_sustain_loop_rejects_inverted_points() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    let inverted = SustainLoopSpec::samples(HORN_LOOP_END as f64, HORN_LOOP_START as f64);
    r.engine
        .start_sustain_loop_with(CH_HORN, "vehicle/horn", inverted, 1.0);
    assert!(r.rec.channel_of("vehicle/horn").is_none(), "played anyway");
    assert!(r.engine.backend().loop_entry(CH_HORN).is_none());
    assert!(r.hearken().sustain_channels().is_empty());
}

#[test]
fn test_sustain_loop_repeats_its_region_until_released() {
    // Held, the horn loops its interior (a looping play over the region);
    // released, it plays on through the tail and ends on its own, never cut.
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.horn_start();
    let channel = r.channel("vehicle/horn");
    let looping = r.rec.calls().into_iter().find_map(|call| match call {
        Call::Play {
            channel: c,
            looping,
            ..
        } if c == channel => Some(looping),
        _ => None,
    });
    assert_eq!(looping, Some(true));
    assert!(region(&r, channel).is_some());
    for _ in 0..30 {
        r.engine.update(0.05);
    }
    assert!(r.rec.is_playing(channel), "the held horn ended");
    r.engine.horn_stop();
    assert_eq!(releases(&r, channel), 1);
    assert!(!r
        .rec
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Stop { channel: c, .. } if *c == channel)));
    // The tail rings out, and the slot is free once it has.
    r.rec.finish(channel);
    r.engine.update(0.05);
    r.engine.horn_start();
    assert_eq!(r.hearken().sustain_channels(), vec![CH_HORN]);
}
