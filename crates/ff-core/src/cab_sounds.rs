//! The cab's own sounds: the window motor, the wind through the open window,
//! the blower, and the air-conditioning compressor clutch.
//!
//! Synthesized the way the lane guide tone is: pure arithmetic, identical
//! bytes from the same build, nothing to add to LFS. They are placeholders
//! the owner's ear can replace with recordings under the same keys; until
//! then they keep the controls audible. Keyed under "cab/" rather than
//! "vehicle/" because the asset-scan test walks the shipped folders for a
//! file on disk, and a synthesized cue has none.
//!
//! The two loops are seamless by construction: the noise is filtered as a
//! circle (the filters run over the buffer once to settle, then again for
//! the samples kept), so the last sample runs into the first with no step,
//! and the blower's hum holds a whole number of cycles.

use std::f64::consts::PI;

use crate::assets_pack::register_generated_sound;
use crate::cab_filter::wav::write_wav_pcm16;

pub const WINDOW_MOTOR_KEY: &str = "cab/window_motor";
pub const WIND_RUSH_KEY: &str = "cab/wind";
pub const FAN_KEY: &str = "cab/fan";
pub const AC_CLUTCH_KEY: &str = "cab/ac_clutch";

const RATE: u32 = 44100;

/// A small deterministic noise source (xorshift64*), so the bytes never
/// change between builds.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (v >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}

fn one_pole(cutoff_hz: f64) -> f64 {
    1.0 - (-2.0 * PI * cutoff_hz / RATE as f64).exp()
}

/// Band-limited noise around `low_hz..high_hz`, filtered as a circle so
/// the buffer loops with no seam.
fn looped_noise(frames: usize, seed: u64, low_hz: f64, high_hz: f64) -> Vec<f64> {
    let mut noise = Noise(seed);
    let white: Vec<f64> = (0..frames).map(|_| noise.next()).collect();
    let (a_lo, a_hi) = (one_pole(high_hz), one_pole(low_hz));
    let (mut lp1, mut lp2, mut floor) = (0.0, 0.0, 0.0);
    let mut out = vec![0.0; frames];
    for pass in 0..2 {
        for (i, x) in white.iter().enumerate() {
            lp1 += a_lo * (x - lp1);
            lp2 += a_lo * (lp1 - lp2);
            floor += a_hi * (lp2 - floor);
            if pass == 1 {
                out[i] = lp2 - floor;
            }
        }
    }
    out
}

/// Scale to an RMS level in dBFS and write a centred stereo WAV.
fn to_wav(mono: &[f64], rms_dbfs: f64) -> Vec<u8> {
    let rms = (mono.iter().map(|x| x * x).sum::<f64>() / mono.len().max(1) as f64).sqrt();
    let target = 10f64.powf(rms_dbfs / 20.0) * 32767.0;
    let gain = if rms > 0.0 { target / rms } else { 0.0 };
    let mut samples = Vec::with_capacity(mono.len() * 2);
    for x in mono {
        let v = (x * gain).round().clamp(-32768.0, 32767.0) as i16;
        samples.push(v);
        samples.push(v);
    }
    write_wav_pcm16(RATE, 2, &samples)
}

/// Air rushing past an open window: a broad, low roar. Three seconds, so
/// the loop is not itself a rhythm.
pub fn wind_rush_wav() -> Vec<u8> {
    let frames = RATE as usize * 3;
    let roar = looped_noise(frames, 0x5eed_0001, 60.0, 900.0);
    let hiss = looped_noise(frames, 0x5eed_0002, 1500.0, 4000.0);
    let mono: Vec<f64> = roar.iter().zip(&hiss).map(|(r, h)| r + 0.15 * h).collect();
    to_wav(&mono, -22.0)
}

/// The blower: soft mid-band air with a faint motor hum at 110 Hz. Two
/// seconds holds exactly 220 cycles of the hum.
pub fn fan_wav() -> Vec<u8> {
    let frames = RATE as usize * 2;
    let air = looped_noise(frames, 0x5eed_0003, 200.0, 2200.0);
    let cycles = 220.0;
    let mono: Vec<f64> = air
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let t = i as f64 / frames as f64;
            a + 0.02 * (2.0 * PI * cycles * t).sin() + 0.01 * (4.0 * PI * cycles * t).sin()
        })
        .collect();
    to_wav(&mono, -24.0)
}

/// The window motor: a whining hum for the glass's travel, ending in a
/// soft stop as the glass seats.
pub fn window_motor_wav() -> Vec<u8> {
    let run = (crate::sim::cab::GLASS_TRAVEL_S * RATE as f64) as usize;
    let stop = RATE as usize / 8;
    let mut noise = Noise(0x5eed_0004);
    let mut mono = Vec::with_capacity(run + stop);
    for i in 0..run {
        let t = i as f64 / RATE as f64;
        let fade_in = (t / 0.05).min(1.0);
        let fade_out = ((run - i) as f64 / (0.08 * RATE as f64)).min(1.0);
        // The motor slows a little as the glass loads it near the end.
        let hz = 170.0 - 15.0 * t / crate::sim::cab::GLASS_TRAVEL_S;
        let phase = 2.0 * PI * hz * t;
        let whine = phase.sin() + 0.5 * (2.0 * phase).sin() + 0.25 * (3.0 * phase).sin();
        let flutter = 0.85 + 0.15 * (2.0 * PI * 9.0 * t).sin();
        mono.push((whine * flutter + 0.2 * noise.next()) * fade_in * fade_out);
    }
    for i in 0..stop {
        let t = i as f64 / RATE as f64;
        let decay = (-t / 0.02).exp();
        mono.push((0.8 * (2.0 * PI * 90.0 * t).sin() + 0.4 * noise.next()) * decay);
    }
    to_wav(&mono, -26.0)
}

/// The compressor clutch engaging: one short metallic click.
pub fn ac_clutch_wav() -> Vec<u8> {
    let frames = RATE as usize / 12;
    let mut noise = Noise(0x5eed_0005);
    let mono: Vec<f64> = (0..frames)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let decay = (-t / 0.008).exp();
            let ring = (-t / 0.03).exp() * 0.3 * (2.0 * PI * 2300.0 * t).sin();
            noise.next() * decay + ring
        })
        .collect();
    to_wav(&mono, -24.0)
}

static REGISTERED: std::sync::Once = std::sync::Once::new();

/// Publish the cab sounds under their ordinary keys. Idempotent.
pub fn register_cab_sounds() {
    REGISTERED.call_once(|| {
        register_generated_sound(WINDOW_MOTOR_KEY, window_motor_wav(), "wav");
        register_generated_sound(WIND_RUSH_KEY, wind_rush_wav(), "wav");
        register_generated_sound(FAN_KEY, fan_wav(), "wav");
        register_generated_sound(AC_CLUTCH_KEY, ac_clutch_wav(), "wav");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cab_filter::wav::WavPcm16;

    fn mono(data: &[u8]) -> Vec<f64> {
        let wav = WavPcm16::parse(data).unwrap();
        assert_eq!(wav.channels, 2);
        assert_eq!(wav.sample_rate, RATE);
        wav.samples.iter().step_by(2).map(|s| *s as f64).collect()
    }

    fn rms_dbfs(x: &[f64]) -> f64 {
        let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
        20.0 * (rms / 32767.0).log10()
    }

    #[test]
    fn the_loops_wrap_with_no_step() {
        for data in [wind_rush_wav(), fan_wav()] {
            let x = mono(&data);
            let n = x.len();
            // The step across the seam is no bigger than a typical step
            // inside the loop.
            let seam = (x[0] - x[n - 1]).abs();
            let typical = x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / n as f64;
            assert!(seam < typical * 6.0, "seam {seam} against {typical}");
        }
    }

    #[test]
    fn the_levels_sit_under_the_engine() {
        // The engine loops sit at -18.7 dBFS RMS; nothing in the cab should
        // be louder than the truck at full gain.
        assert!((rms_dbfs(&mono(&wind_rush_wav())) + 22.0).abs() < 0.3);
        assert!((rms_dbfs(&mono(&fan_wav())) + 24.0).abs() < 0.3);
        assert!(rms_dbfs(&mono(&window_motor_wav())) < -20.0);
    }

    #[test]
    fn the_motor_runs_as_long_as_the_glass_travels() {
        let secs = mono(&window_motor_wav()).len() as f64 / RATE as f64;
        assert!((secs - crate::sim::cab::GLASS_TRAVEL_S - 0.125).abs() < 0.01);
    }

    #[test]
    fn the_bytes_never_change_between_calls() {
        assert_eq!(wind_rush_wav(), wind_rush_wav());
        assert_eq!(ac_clutch_wav(), ac_clutch_wav());
    }

    #[test]
    fn register_is_idempotent() {
        register_cab_sounds();
        register_cab_sounds();
        for key in [WINDOW_MOTOR_KEY, WIND_RUSH_KEY, FAN_KEY, AC_CLUTCH_KEY] {
            let (_, ext) = crate::assets_pack::generated_sound(key).unwrap();
            assert_eq!(ext, "wav");
        }
    }
}
