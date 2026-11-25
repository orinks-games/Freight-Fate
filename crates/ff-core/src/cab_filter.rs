//! The cab between the engine and the ear: the sealed-cab transfer function.
//!
//! Testers heard the rebuilt engine voice as EXTERNAL -- a truck heard from
//! outside, not from the driver's seat (via Josh, 2026-08-13). The cuts were
//! recorded in a working cab but cleanly; a driver's ear gets the engine
//! through glass and firewall (strong high-frequency loss) and through the
//! body structure (low end the panels carry and amplify). This module applies
//! that transfer to the engine band cuts at load time, so the voice sits
//! around the player instead of in front of the windshield.
//!
//! The parameters are the "sealed" (windows-up) variant the owner's ear
//! picked from the sound-test/cab_transfer.py auditions (2026-08-13): a -16 dB
//! shelf above 1 kHz, a 2.4 kHz lowpass, +5 dB of body low end, and +4 dB of
//! panel boom at 63 Hz. The moderate variant from the same auditions is the
//! natural "window cracked" setting when the cabin-state work (doors/windows)
//! picks intensities.
//!
//! Implementation: the biquad chain's exact z-transform response is
//! evaluated on the rfft bins of the whole loop and multiplied into its
//! spectrum -- circular convolution, so a seamless loop stays seamless and
//! the result equals the steady state a repeating loop would reach through
//! the real-time chain. Each render is RMS-matched to
//! its input (the transfer shapes timbre, the mixer owns level) with a peak
//! guard. Deterministic, one-time cost per cut (cached by the audio engine).
//!
//! Only 16-bit PCM WAV passes through the transfer; anything else returns
//! unchanged (the classic voice's ogg deliberately keeps its old sound).
//!
//! Port of `freight_fate/cab_filter.py`. The Python render was numpy over
//! pocketfft; this one is `realfft`/`rustfft`. The biquad arithmetic follows
//! numpy's own operation order (its Smith-style complex
//! division, reciprocal-multiply for a complex-over-real quotient, pairwise
//! summation for the RMS), and the FFTs differ only in rounding, so the
//! int16 output agrees with the no-comb Python bytes to within one LSB -- see
//! the fixture tests, which pin a render of synthetic tones.

pub mod wav;

use std::f64::consts::PI;

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

use wav::{WavError, WavPcm16};

// Sealed-cab parameters (owner's ear, 2026-08-13; lab in sound-test/cab_transfer.py).
pub const HIGH_SHELF_HZ: f64 = 1000.0; // glass/firewall attenuation corner
pub const HIGH_SHELF_DB: f64 = -16.0;
pub const LOWPASS_HZ: f64 = 2400.0;
pub const BODY_SHELF_HZ: f64 = 100.0; // structure-borne low end
pub const BODY_SHELF_DB: f64 = 5.0;
pub const BOOM_HZ: f64 = 63.0; // panel boom center
pub const BOOM_DB: f64 = 4.0;
pub const BOOM_Q: f64 = 1.1;
pub const SHELF_SLOPE: f64 = 0.9;
// The owner's audition used 0.7071 written out, not 1/sqrt(2); the render
// must keep the exact literal to stay byte-identical with the Python one.
#[allow(clippy::approx_constant)]
pub const LOWPASS_Q: f64 = 0.7071;
type Coeffs = ([f64; 3], [f64; 3]);

fn shelf_coeffs(sr: f64, fc: f64, gain_db: f64, high: bool) -> Coeffs {
    let a = 10.0_f64.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * fc / sr;
    let alpha = w0.sin() / 2.0 * ((a + 1.0 / a) * (1.0 / SHELF_SLOPE - 1.0) + 2.0).sqrt();
    let cw = w0.cos();
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    let sign = if high { 1.0 } else { -1.0 };
    let b = [
        a * ((a + 1.0) + sign * (a - 1.0) * cw + two_sqrt_a_alpha),
        -2.0 * sign * a * ((a - 1.0) + sign * (a + 1.0) * cw),
        a * ((a + 1.0) + sign * (a - 1.0) * cw - two_sqrt_a_alpha),
    ];
    let a_ = [
        (a + 1.0) - sign * (a - 1.0) * cw + two_sqrt_a_alpha,
        2.0 * sign * ((a - 1.0) - sign * (a + 1.0) * cw),
        (a + 1.0) - sign * (a - 1.0) * cw - two_sqrt_a_alpha,
    ];
    (b, a_)
}

fn peaking_coeffs(sr: f64, fc: f64, gain_db: f64, q: f64) -> Coeffs {
    let a = 10.0_f64.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * fc / sr;
    let alpha = w0.sin() / (2.0 * q);
    let cw = w0.cos();
    (
        [1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a],
        [1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a],
    )
}

fn lowpass_coeffs(sr: f64, fc: f64, q: f64) -> Coeffs {
    let w0 = 2.0 * PI * fc / sr;
    let alpha = w0.sin() / (2.0 * q);
    let cw = w0.cos();
    (
        [(1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0],
        [1.0 + alpha, -2.0 * cw, 1.0 - alpha],
    )
}

/// numpy's complex division (Smith's algorithm), as its `true_divide` loop
/// does it, so a complex-over-real quotient is a reciprocal multiply.
fn np_cdiv(a: Complex<f64>, b: Complex<f64>) -> Complex<f64> {
    let (in1r, in1i, in2r, in2i) = (a.re, a.im, b.re, b.im);
    let (in2r_abs, in2i_abs) = (in2r.abs(), in2i.abs());
    if in2r_abs >= in2i_abs {
        if in2r_abs == 0.0 && in2i_abs == 0.0 {
            return Complex::new(in1r / in2r_abs, in1i / in2i_abs);
        }
        let rat = in2i / in2r;
        let scl = 1.0 / (in2r + in2i * rat);
        Complex::new((in1r + in1i * rat) * scl, (in1i - in1r * rat) * scl)
    } else {
        let rat = in2r / in2i;
        let scl = 1.0 / (in2i + in2r * rat);
        Complex::new((in1r * rat + in1i) * scl, (in1i * rat - in1r) * scl)
    }
}

/// The complex cab response on the rfft bins of an `n`-sample loop.
fn transfer(sr: f64, n: usize) -> Vec<Complex<f64>> {
    let bins = n / 2 + 1;
    let nf = n as f64;
    // `-2j * pi * arange / n` in numpy: the angle is ((-2pi) * k) * (1/n),
    // a reciprocal multiply, then exp of a pure imaginary is (cos, sin).
    let recip_n = 1.0 / nf;
    let z1: Vec<Complex<f64>> = (0..bins)
        .map(|k| {
            let theta = (-2.0 * PI * k as f64) * recip_n;
            Complex::new(theta.cos(), theta.sin())
        })
        .collect();
    let mut h: Vec<Complex<f64>> = vec![Complex::new(1.0, 0.0); bins];
    let stages = [
        peaking_coeffs(sr, BOOM_HZ, BOOM_DB, BOOM_Q),
        shelf_coeffs(sr, BODY_SHELF_HZ, BODY_SHELF_DB, false),
        shelf_coeffs(sr, HIGH_SHELF_HZ, HIGH_SHELF_DB, true),
        lowpass_coeffs(sr, LOWPASS_HZ, LOWPASS_Q),
    ];
    for (b, a) in stages {
        for (k, z) in z1.iter().enumerate() {
            let z2 = z * z;
            // b0 + b1*z1 + b2*z2, a real scalar times a complex being
            // componentwise in numpy.
            let num = Complex::new(b[0], 0.0) + z.scale(b[1]) + z2.scale(b[2]);
            let den = Complex::new(a[0], 0.0) + z.scale(a[1]) + z2.scale(a[2]);
            h[k] *= np_cdiv(num, den);
        }
    }
    h
}

/// numpy's pairwise summation over a contiguous float64 array (the `mean`
/// under the RMS match), so the gain scalar rounds the way numpy's did.
fn pairwise_sum(a: &[f64]) -> f64 {
    const BLOCK: usize = 128;
    let n = a.len();
    if n < 8 {
        return a.iter().fold(0.0, |acc, v| acc + v);
    }
    if n <= BLOCK {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - n % 8 {
            for (j, slot) in r.iter_mut().enumerate() {
                *slot += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        return res;
    }
    let mut n2 = n / 2;
    n2 -= n2 % 8;
    pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
}

fn rms(samples: &[f64]) -> f64 {
    let squares: Vec<f64> = samples.iter().map(|v| v * v).collect();
    (pairwise_sum(&squares) / squares.len() as f64).sqrt()
}

/// Apply the sealed-cab transfer to a 16-bit PCM WAV, RMS-matched.
///
/// Anything that is not 16-bit PCM comes back unchanged, logged once per
/// process -- the transfer must never be the reason a sound goes missing.
pub fn seal_wav(data: &[u8]) -> Vec<u8> {
    let wav = match WavPcm16::parse(data) {
        Ok(wav) => wav,
        Err(WavError::NotPcm16) => {
            log::warn!("Cab transfer skipped: not 16-bit PCM");
            return data.to_vec();
        }
        Err(WavError::Unreadable) => {
            log::warn!("Cab transfer skipped: unreadable WAV");
            return data.to_vec();
        }
    };
    let channels = wav.channels as usize;
    let n = wav.frames();
    if n == 0 {
        log::warn!("Cab transfer skipped: empty WAV");
        return data.to_vec();
    }
    let x: Vec<f64> = wav.samples.iter().map(|s| *s as f64 / 32768.0).collect();
    let h = transfer(wav.sample_rate as f64, n);

    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(n);
    let mut y = vec![0.0_f64; x.len()];
    let fct = 1.0 / n as f64; // numpy's irfft normalisation: multiply by 1/n
    for c in 0..channels {
        let mut input: Vec<f64> = (0..n).map(|i| x[i * channels + c]).collect();
        let mut spectrum = forward.make_output_vec();
        forward
            .process(&mut input, &mut spectrum)
            .expect("buffers sized by the planner");
        for (bin, hk) in spectrum.iter_mut().zip(h.iter()) {
            *bin *= hk;
        }
        // irfft ignores the imaginary part of the DC and Nyquist bins;
        // realfft insists they are zero.
        spectrum[0].im = 0.0;
        if n % 2 == 0 {
            spectrum[n / 2].im = 0.0;
        }
        let mut out = inverse.make_output_vec();
        inverse
            .process(&mut spectrum, &mut out)
            .expect("buffers sized by the planner");
        for (i, v) in out.iter().enumerate() {
            y[i * channels + c] = v * fct;
        }
    }

    let rms_in = rms(&x);
    let rms_out = rms(&y);
    if rms_out > 0.0 {
        let gain = rms_in / rms_out;
        for v in &mut y {
            *v *= gain;
        }
    }
    let peak = y.iter().fold(0.0_f64, |acc, v| acc.max(v.abs()));
    if peak > 0.995 {
        // loudness match, but never clip
        let guard = 0.995 / peak;
        for v in &mut y {
            *v *= guard;
        }
    }
    let pcm: Vec<i16> = y
        .iter()
        .map(|v| (v.clamp(-1.0, 1.0) * 32767.0).round_ties_even() as i16)
        .collect();
    wav::write_wav_pcm16(wav.sample_rate, wav.channels, &pcm)
}

#[cfg(test)]
mod tests {
    //! Tests for the sealed-cab transfer on the engine voice. The
    //! `audio._playback_bytes` caching test belongs to the game crate.
    use super::*;

    fn sine_wav(freqs: &[f64], sr: u32, seconds: f64, channels: u16, amp: f64) -> Vec<u8> {
        let n = (sr as f64 * seconds) as usize;
        let mut samples = Vec::with_capacity(n * channels as usize);
        for i in 0..n {
            let t = i as f64 / sr as f64;
            let x: f64 = freqs.iter().map(|f| (2.0 * PI * f * t).sin()).sum::<f64>() * amp
                / freqs.len() as f64;
            let pcm = (x.clamp(-1.0, 1.0) * 32767.0) as i16;
            for _ in 0..channels {
                samples.push(pcm);
            }
        }
        wav::write_wav_pcm16(sr, channels, &samples)
    }

    /// RMS of the channel-0 rfft magnitude inside [lo, hi) Hz.
    fn band_rms(data: &[u8], lo: f64, hi: f64) -> f64 {
        let wav = WavPcm16::parse(data).unwrap();
        let ch = wav.channels as usize;
        let mut x: Vec<f64> = wav
            .samples
            .iter()
            .step_by(ch)
            .map(|s| *s as f64 / 32768.0)
            .collect();
        let n = x.len();
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(n);
        let mut spec = fft.make_output_vec();
        fft.process(&mut x, &mut spec).unwrap();
        let sr = wav.sample_rate as f64;
        let band: Vec<f64> = spec
            .iter()
            .enumerate()
            .filter(|(k, _)| {
                let f = *k as f64 * sr / n as f64;
                f >= lo && f < hi
            })
            .map(|(_, c)| c.norm())
            .collect();
        if band.is_empty() {
            0.0
        } else {
            (band.iter().map(|v| v * v).sum::<f64>() / band.len() as f64).sqrt()
        }
    }

    #[test]
    fn test_seal_darkens_highs_keeps_level_and_format() {
        let src = sine_wav(&[80.0, 4000.0], 44100, 0.5, 2, 0.25);
        let sealed = seal_wav(&src);
        // The cab: highs well down relative to lows, overall level held.
        let high_drop = band_rms(&sealed, 3000.0, 5000.0) / band_rms(&src, 3000.0, 5000.0);
        let low_hold = band_rms(&sealed, 40.0, 160.0) / band_rms(&src, 40.0, 160.0);
        assert!(high_drop < 0.2, "{high_drop}"); // -16 dB shelf plus the 2.4 kHz lowpass
        assert!(low_hold > 0.8, "{low_hold}"); // body low end survives the RMS re-match
        let a = WavPcm16::parse(&src).unwrap();
        let b = WavPcm16::parse(&sealed).unwrap();
        assert_eq!(
            (a.sample_rate, a.channels, a.frames()),
            (b.sample_rate, b.channels, b.frames())
        );
    }

    #[test]
    fn test_seal_is_deterministic() {
        let src = sine_wav(&[120.0, 1800.0], 44100, 0.5, 1, 0.25);
        assert_eq!(seal_wav(&src), seal_wav(&src));
    }

    #[test]
    fn test_seal_passes_non_pcm_through() {
        assert_eq!(seal_wav(b"OggS not a wav at all"), b"OggS not a wav at all");
    }

    /// Parity with the no-comb NumPy render of the same input.
    fn assert_parity(input: &[u8], expected: &[u8]) -> i32 {
        let got = WavPcm16::parse(&seal_wav(input)).unwrap();
        let want = WavPcm16::parse(expected).unwrap();
        assert_eq!(
            (got.sample_rate, got.channels),
            (want.sample_rate, want.channels)
        );
        assert_eq!(got.samples.len(), want.samples.len());
        let mut worst = 0i32;
        let mut off_by_one = 0usize;
        for (g, w) in got.samples.iter().zip(want.samples.iter()) {
            let err = (*g as i32 - *w as i32).abs();
            worst = worst.max(err);
            if err == 1 {
                off_by_one += 1;
            }
        }
        eprintln!(
            "cab_filter reference: {} samples, worst |error| = {worst} LSB, {off_by_one} samples off by one",
            got.samples.len()
        );
        assert!(
            worst <= 1,
            "the Rust render differs from the Python render by {worst} LSB"
        );
        worst
    }

    #[test]
    fn test_seal_matches_the_no_comb_render_within_one_lsb_stereo() {
        assert_parity(
            include_bytes!("cab_filter/fixtures/tone_in.wav"),
            include_bytes!("cab_filter/fixtures/tone_sealed.wav"),
        );
    }

    #[test]
    fn test_seal_matches_the_no_comb_render_within_one_lsb_mono() {
        assert_parity(
            include_bytes!("cab_filter/fixtures/mono_in.wav"),
            include_bytes!("cab_filter/fixtures/mono_sealed.wav"),
        );
    }

    #[test]
    fn pairwise_sum_matches_a_plain_sum_on_small_and_large_inputs() {
        let small: Vec<f64> = (1..=5).map(|v| v as f64).collect();
        assert_eq!(pairwise_sum(&small), 15.0);
        let large: Vec<f64> = (0..1000).map(|v| (v % 7) as f64 * 0.25).collect();
        let plain: f64 = large.iter().sum();
        assert!((pairwise_sum(&large) - plain).abs() < 1e-9);
    }
}
