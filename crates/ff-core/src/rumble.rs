//! Controller haptics (rumble), isolated from the device layer.
//!
//! Freight Fate is audio-first; rumble only *reinforces* cues a player already
//! hears. This module owns the *shape* of every effect and knows nothing about
//! SDL: [`RumbleEngine`] is handed a [`RumbleSink`] (`send(low, high,
//! duration_ms)` and `stop()`) by the controller manager, so it drives a real
//! pad in the game and a list-recording fake in tests.
//!
//! The two motors map onto the request's geography:
//!
//! * `low_frequency` -- the large **left-grip** motor.
//! * `high_frequency` -- the small **right-grip** motor.
//!
//! So "starting at the right and moving to the left" is a high->low sweep, and an
//! alert "blip of the high-frequency side" is a short right-grip buzz.
//!
//! Effects come in two kinds:
//!
//! * **One-shots** (hazard sweep, alert blip, collision impact): an envelope
//!   `fn(t)` over normalized time `0..1` that runs for a fixed duration and
//!   then drops itself.
//! * **Continuous** (rumble strip, hard-brake shudder): a level refreshed every
//!   frame while the condition holds. A short TTL means the caller never has to
//!   send an explicit "off" -- the effect stops on its own a few frames after the
//!   refreshes stop.
//!
//! Every frame [`RumbleEngine::tick`] combines whatever is active (per-motor
//! max), issues a single device call, and stops the device once on the
//! active->idle edge.
//!
//! Port of `freight_fate/rumble.py`.

use std::f64::consts::PI;

/// Re-issued every frame with a duration a few frames long, so a dropped frame
/// never leaves an audible gap; each new call replaces the last on SDL.
pub const FRAME_RUMBLE_MS: i32 = 120;

/// Continuous effects are refreshed each frame; if the refresh stops, the effect
/// lapses this many seconds later (a few frames at 60 fps).
pub const CONTINUOUS_TTL_S: f64 = 0.05;

// Hazard sweep: two overlapping raised-cosine bumps across a 0.75 s window, the
// right (high) leading and the left (low) trailing.
pub const HAZARD_DURATION_MS: i32 = 750;
const HAZARD_HIGH_CENTER: f64 = 0.25;
const HAZARD_HIGH_HALF: f64 = 0.32;
const HAZARD_HIGH_PEAK: f64 = 0.85;
const HAZARD_LOW_CENTER: f64 = 0.62;
const HAZARD_LOW_HALF: f64 = 0.34;
const HAZARD_LOW_PEAK: f64 = 1.0;

// Alert blip: a short right-grip (high) buzz.
pub const ALERT_INTENSITY: f64 = 0.6;
pub const ALERT_DURATION_MS: i32 = 120;

// Collision impact: a heavy low thump that decays, with a brief high crack.
pub const IMPACT_DURATION_MS: i32 = 350;

// Rumble strip: both motors buzz between a non-zero floor and a ceiling, the
// right side pulsing faster than the left -- a deliberately harsh, alternating
// feel that never fully releases either motor.
const STRIP_LOW_HZ: f64 = 9.0;
const STRIP_HIGH_HZ: f64 = 16.0;

// Hard braking: a continuous low shudder scaled by brake force, with a light
// high texture on top.
const BRAKE_SHUDDER_HZ: f64 = 22.0;
const BRAKE_TEXTURE_HZ: f64 = 30.0;

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// A raised-cosine (Hann) bump peaking at `center` with half-width
/// `half`; zero outside the window. Used to shape the hazard sweep.
fn bump(t: f64, center: f64, half: f64, peak: f64) -> f64 {
    let d = (t - center).abs();
    if d >= half {
        return 0.0;
    }
    peak * 0.5 * (1.0 + (PI * d / half).cos())
}

/// A 0..1 oscillation at `hz` cycles per second.
fn osc(phase: f64, hz: f64) -> f64 {
    0.5 * (1.0 + (2.0 * PI * hz * phase).sin())
}

/// The device side: a real pad in the game, a recorder in tests.
pub trait RumbleSink {
    /// Drive both motors (0..1 each) for `duration_ms`; replaces the last call.
    fn send(&mut self, low: f64, high: f64, duration_ms: i32);
    /// Silence the pad.
    fn stop(&mut self);
}

/// The envelope a one-shot follows over normalized time `t` in 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Envelope {
    /// A short high-frequency blip at a fixed amplitude.
    Alert { amp: f64 },
    /// The 750 ms right->left sweep for a communicated hazard.
    Hazard,
    /// A heavy low thump for a collision (louder than an alert blip).
    Impact { low_peak: f64, high_peak: f64 },
    /// A quick soft thump representing a tire crossing a road joint/crack.
    Joint { low_peak: f64 },
}

impl Envelope {
    /// `(low, high)` at normalized time `t`.
    fn at(&self, t: f64) -> (f64, f64) {
        match *self {
            Envelope::Alert { amp } => (0.0, amp),
            Envelope::Hazard => {
                let high = bump(t, HAZARD_HIGH_CENTER, HAZARD_HIGH_HALF, HAZARD_HIGH_PEAK);
                let low = bump(t, HAZARD_LOW_CENTER, HAZARD_LOW_HALF, HAZARD_LOW_PEAK);
                (low, high)
            }
            Envelope::Impact {
                low_peak,
                high_peak,
            } => {
                let low = low_peak * (1.0 - t).powf(1.5); // quick attack, decaying thump
                let high = high_peak * (1.0 - 4.0 * t).max(0.0); // brief crack at the hit
                (low, high)
            }
            Envelope::Joint { low_peak } => (low_peak * (1.0 - t), 0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OneShot {
    /// seconds
    duration: f64,
    envelope: Envelope,
    elapsed: f64,
}

/// Schedules and mixes haptic effects, driving an injected device.
pub struct RumbleEngine<S: RumbleSink> {
    sink: S,
    phase: f64,
    oneshots: Vec<OneShot>,
    strip_level: f64,
    strip_ttl: f64,
    brake_level: f64,
    brake_ttl: f64,
    active: bool,
}

impl<S: RumbleSink> RumbleEngine<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            phase: 0.0,
            oneshots: Vec::new(),
            strip_level: 0.0,
            strip_ttl: 0.0,
            brake_level: 0.0,
            brake_ttl: 0.0,
            active: false,
        }
    }

    /// The device this engine drives (tests read their recorder back here).
    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    // -- one-shot effects -----------------------------------------------------

    /// A short high-frequency blip that accompanies an alert, at the default
    /// intensity and length.
    pub fn alert(&mut self) {
        self.alert_with(ALERT_INTENSITY, ALERT_DURATION_MS);
    }

    /// A short high-frequency blip that accompanies an alert.
    pub fn alert_with(&mut self, intensity: f64, duration_ms: i32) {
        let amp = clamp01(intensity);
        self.oneshots.push(OneShot {
            duration: duration_ms as f64 / 1000.0,
            envelope: Envelope::Alert { amp },
            elapsed: 0.0,
        });
    }

    /// The 750 ms right->left sweep for a communicated hazard.
    pub fn hazard(&mut self) {
        self.oneshots.push(OneShot {
            duration: HAZARD_DURATION_MS as f64 / 1000.0,
            envelope: Envelope::Hazard,
            elapsed: 0.0,
        });
    }

    /// A heavy low thump for a collision (louder than an alert blip).
    pub fn impact(&mut self, severity: f64) {
        let sev = clamp01(severity);
        let low_peak = 0.6 + 0.4 * sev;
        let high_peak = 0.4 * sev;
        self.oneshots.push(OneShot {
            duration: IMPACT_DURATION_MS as f64 / 1000.0,
            envelope: Envelope::Impact {
                low_peak,
                high_peak,
            },
            elapsed: 0.0,
        });
    }

    /// A quick soft thump representing a tire crossing a road joint/crack.
    pub fn joint(&mut self, severity: f64) {
        let sev = clamp01(severity);
        let low_peak = 0.15 * sev;
        self.oneshots.push(OneShot {
            duration: 0.2,
            envelope: Envelope::Joint { low_peak },
            elapsed: 0.0,
        });
    }

    // -- continuous effects (refresh each frame while active) -----------------

    /// Refresh the harsh rumble-strip buzz; `level` is 0..1.
    pub fn rumble_strip(&mut self, level: f64) {
        self.strip_level = clamp01(level);
        self.strip_ttl = CONTINUOUS_TTL_S;
    }

    /// Refresh the hard-braking shudder; `level` is 0..1.
    pub fn hard_brake(&mut self, level: f64) {
        self.brake_level = clamp01(level);
        self.brake_ttl = CONTINUOUS_TTL_S;
    }

    // -- per-frame drive ------------------------------------------------------

    pub fn tick(&mut self, dt: f64) {
        self.phase += dt;
        let mut low = 0.0_f64;
        let mut high = 0.0_f64;

        self.strip_ttl -= dt;
        if self.strip_ttl > 0.0 {
            let s = 0.55 + 0.45 * self.strip_level;
            low = low.max(s * (0.55 + 0.45 * osc(self.phase, STRIP_LOW_HZ)));
            high = high.max(s * (0.60 + 0.40 * osc(self.phase, STRIP_HIGH_HZ)));
        }

        self.brake_ttl -= dt;
        if self.brake_ttl > 0.0 {
            let shudder = 0.85 + 0.15 * osc(self.phase, BRAKE_SHUDDER_HZ);
            low = low.max((0.35 + 0.55 * self.brake_level) * shudder);
            high = high.max(0.15 * self.brake_level * osc(self.phase, BRAKE_TEXTURE_HZ));
        }

        for eff in &mut self.oneshots {
            eff.elapsed += dt;
        }
        self.oneshots.retain(|e| e.elapsed < e.duration);
        for eff in &self.oneshots {
            let (elow, ehigh) = eff.envelope.at(eff.elapsed / eff.duration);
            low = low.max(elow);
            high = high.max(ehigh);
        }

        let (low, high) = (clamp01(low), clamp01(high));
        if low > 0.0 || high > 0.0 {
            self.sink.send(low, high, FRAME_RUMBLE_MS);
            self.active = true;
        } else if self.active {
            self.sink.stop();
            self.active = false;
        }
    }

    /// Drop every effect and silence the device (disconnect / haptics off).
    pub fn reset(&mut self) {
        self.oneshots.clear();
        self.strip_ttl = 0.0;
        self.brake_ttl = 0.0;
        self.strip_level = 0.0;
        self.brake_level = 0.0;
        if self.active {
            self.active = false;
        }
        self.sink.stop();
    }
}

#[cfg(test)]
mod tests {
    //! Controller haptics: the pure RumbleEngine. (The Python file's
    //! manager device-guard tests drive `ControllerManager`, which lives in
    //! the game crate with SDL.)
    use super::*;

    /// Stands in for the pad: records every send and stop the engine issues.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<(f64, f64, i32)>, // (low, high, duration_ms)
        stops: usize,
    }

    impl RumbleSink for Recorder {
        fn send(&mut self, low: f64, high: f64, duration_ms: i32) {
            self.calls.push((low, high, duration_ms));
        }
        fn stop(&mut self) {
            self.stops += 1;
        }
    }

    fn engine() -> RumbleEngine<Recorder> {
        RumbleEngine::new(Recorder::default())
    }

    /// How often a series crosses its own mean -- a proxy for its rate.
    fn crossings(values: &[f64]) -> usize {
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        values
            .windows(2)
            .filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0)
            .count()
    }

    // -- one-shot effects --------------------------------------------------------

    #[test]
    fn test_alert_is_high_only_and_stops() {
        let mut e = engine();
        e.alert();
        e.tick(0.05); // still inside the 120 ms blip
        let (low, high, _) = *e.sink().calls.last().unwrap();
        assert_eq!(low, 0.0);
        assert!(high > 0.0);
        e.tick(0.1); // now past the blip: engine goes idle and stops once
        assert_eq!(e.sink().stops, 1);
    }

    #[test]
    fn test_hazard_sweep_leads_right_then_left_with_overlap() {
        let mut e = engine();
        e.hazard();
        let mut samples = Vec::new(); // (t, low, high) whenever the engine actually drove the pad
        let mut t = 0.0;
        for _ in 0..40 {
            let before = e.sink().calls.len();
            e.tick(0.025);
            t += 0.025;
            if e.sink().calls.len() > before {
                let (low, high, _) = *e.sink().calls.last().unwrap();
                samples.push((t, low, high));
            }
        }
        let peak_high_t = samples
            .iter()
            .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
            .unwrap()
            .0;
        let peak_low_t = samples
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap()
            .0;
        // Right (high) grip leads, left (low) grip trails.
        assert!(peak_high_t < peak_low_t);
        // They overlap: some moment has both motors clearly running.
        assert!(samples
            .iter()
            .any(|&(_, low, high)| low > 0.1 && high > 0.1));
        // The whole thing is about 750 ms long.
        let end = samples.last().unwrap().0;
        assert!((0.7..=0.8).contains(&end));
    }

    #[test]
    fn test_impact_is_a_decaying_low_thump() {
        let mut e = engine();
        e.impact(1.0);
        e.tick(0.02);
        let early_low = e.sink().calls.last().unwrap().0;
        for _ in 0..20 {
            // past the 350 ms life
            e.tick(0.02);
        }
        // It leads with the low motor and decays over its short life.
        assert!(early_low > 0.5);
        assert_eq!(e.sink().stops, 1); // lapses on its own
    }

    // -- continuous effects ------------------------------------------------------

    #[test]
    fn test_rumble_strip_never_releases_a_motor_and_pulses_right_faster() {
        let mut e = engine();
        let mut lows = Vec::new();
        let mut highs = Vec::new();
        for _ in 0..120 {
            // ~0.6 s of refreshed drift
            e.rumble_strip(1.0);
            e.tick(0.005);
            let (low, high, _) = *e.sink().calls.last().unwrap();
            lows.push(low);
            highs.push(high);
        }
        // Harsh: both motors stay buzzing, never fully at zero.
        assert!(lows.iter().cloned().fold(f64::INFINITY, f64::min) > 0.0);
        assert!(highs.iter().cloned().fold(f64::INFINITY, f64::min) > 0.0);
        // The right (high) side pulses faster than the left (low) side.
        assert!(crossings(&highs) > crossings(&lows));
    }

    #[test]
    fn test_rumble_strip_stops_after_refreshes_end() {
        let mut e = engine();
        for _ in 0..3 {
            e.rumble_strip(1.0);
            e.tick(0.016);
        }
        let stops_before = e.sink().stops;
        for _ in 0..5 {
            // stop refreshing; TTL lapses within a few frames
            e.tick(0.016);
        }
        assert_eq!(e.sink().stops, stops_before + 1);
    }

    #[test]
    fn test_hard_brake_low_scales_with_level() {
        let (mut ea, mut eb) = (engine(), engine());
        ea.hard_brake(1.0);
        eb.hard_brake(0.5);
        ea.tick(0.016);
        eb.tick(0.016); // identical phase, so only the level differs
        assert!(ea.sink().calls.last().unwrap().0 > eb.sink().calls.last().unwrap().0);
    }

    #[test]
    fn test_combine_takes_the_per_motor_max() {
        let mut e = engine();
        e.rumble_strip(0.2); // gentle both-motor buzz
        e.hard_brake(1.0); // strong low shudder
        e.tick(0.016);
        let combined_low = e.sink().calls.last().unwrap().0;

        let mut e2 = engine();
        e2.rumble_strip(0.2);
        e2.tick(0.016);
        let strip_only_low = e2.sink().calls.last().unwrap().0;
        // The louder source wins each motor; low is at least the strip-only low.
        assert!(combined_low >= strip_only_low);
    }

    #[test]
    fn test_reset_clears_effects_and_stops() {
        let mut e = engine();
        e.rumble_strip(1.0);
        e.tick(0.016);
        e.reset();
        assert!(e.sink().stops >= 1);
        e.sink_mut().calls.clear();
        e.tick(0.016); // nothing left to drive
        assert!(e.sink().calls.is_empty());
    }

    #[test]
    fn test_joint_is_low_frequency_only_and_stops() {
        let mut e = engine();
        e.joint(0.8);
        e.tick(0.05);
        assert_eq!(e.sink().calls.len(), 1);
        let (low, high, _duration) = e.sink().calls[0];
        assert!(low > 0.0);
        assert_eq!(high, 0.0); // joint is low-frequency motor thump only
                               // Check that it stops after decay
        e.tick(0.25); // past the 0.2s duration
        assert!(e.sink().stops >= 1);
    }
}
