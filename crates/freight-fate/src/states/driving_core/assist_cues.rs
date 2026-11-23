//! Synthesized non-verbal earcons for driving assistance transitions.
//!
//! Deterministic 16-bit stereo WAVs registered through `register_generated_sound`
//! so every build (debug, release, packaged) has the exact same cues without
//! requiring external sound-pack updates:
//! - `assist/blind_spot_chirp`: directional warning when steering toward an
//!   occupied adjacent lane.
//! - `assist/keeper_lock`: short rising two-tone lock when `keeper_follow`
//!   locks onto a slower lead vehicle.
//! - `assist/curve_servo`: gentle descending two-tone cue when curve servo
//!   engages.
//! - `assist/lane_keep_dropout`: falling three-tone cue when lane keeping
//!   drops out (e.g., snow-covered northern roads).

use std::f64::consts::PI;
use std::sync::Once;

use ff_core::assets_pack::register_generated_sound;

pub const BLIND_SPOT_CHIRP_KEY: &str = "assist/blind_spot_chirp";
pub const KEEPER_LOCK_KEY: &str = "assist/keeper_lock";
pub const CURVE_SERVO_KEY: &str = "assist/curve_servo";
pub const LANE_KEEP_DROPOUT_KEY: &str = "assist/lane_keep_dropout";

const SAMPLE_RATE: u32 = 22_050;
const EDGE_S: f64 = 0.008;

fn envelope(index: usize, total: usize, edge: usize) -> f64 {
    if edge == 0 || total == 0 {
        return 1.0;
    }
    if index < edge {
        return 0.5 - 0.5 * (PI * index as f64 / edge as f64).cos();
    }
    if index + edge >= total {
        return 0.5 - 0.5 * (PI * (total - 1 - index) as f64 / edge as f64).cos();
    }
    1.0
}

fn synth_sequence(notes: &[(f64, f64)], gap_s: f64, peak_scale: f64) -> Vec<u8> {
    let mut samples: Vec<i16> = Vec::new();
    let gap_n = (gap_s * SAMPLE_RATE as f64) as usize;
    let edge_n = (EDGE_S * SAMPLE_RATE as f64) as usize;
    let peak = peak_scale * 32767.0;
    for (idx, &(freq, dur_s)) in notes.iter().enumerate() {
        let tone_n = (dur_s * SAMPLE_RATE as f64) as usize;
        let step = 2.0 * PI * freq / SAMPLE_RATE as f64;
        for i in 0..tone_n {
            let value = (peak * envelope(i, tone_n, edge_n) * (step * i as f64).sin()) as i16;
            samples.push(value);
            samples.push(value);
        }
        if idx + 1 < notes.len() {
            for _ in 0..gap_n {
                samples.push(0);
                samples.push(0);
            }
        }
    }
    ff_core::wav::pcm16_wav(&samples, 2, SAMPLE_RATE)
}

pub fn blind_spot_chirp_wav() -> Vec<u8> {
    synth_sequence(&[(1180.0, 0.045), (1480.0, 0.055)], 0.018, 0.42)
}

pub fn keeper_lock_wav() -> Vec<u8> {
    synth_sequence(&[(520.0, 0.055), (680.0, 0.065)], 0.015, 0.30)
}

pub fn curve_servo_wav() -> Vec<u8> {
    synth_sequence(&[(640.0, 0.060), (460.0, 0.070)], 0.015, 0.30)
}

pub fn lane_keep_dropout_wav() -> Vec<u8> {
    synth_sequence(
        &[(580.0, 0.055), (420.0, 0.055), (310.0, 0.075)],
        0.018,
        0.34,
    )
}

static REGISTERED: Once = Once::new();

/// Register all synthesized assist earcons under their sound keys. Idempotent.
pub fn register_assist_cues() {
    REGISTERED.call_once(|| {
        register_generated_sound(BLIND_SPOT_CHIRP_KEY, blind_spot_chirp_wav(), "wav");
        register_generated_sound(KEEPER_LOCK_KEY, keeper_lock_wav(), "wav");
        register_generated_sound(CURVE_SERVO_KEY, curve_servo_wav(), "wav");
        register_generated_sound(LANE_KEEP_DROPOUT_KEY, lane_keep_dropout_wav(), "wav");
    });
}
