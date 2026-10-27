//! Reusable, backend-agnostic volume fades driven by a per-frame `dt`.
//!
//! Nothing here touches an audio backend directly. A [`Fade`] is just a
//! timed ramp between two numbers with a chosen easing curve; it calls a
//! supplied `apply(value)` setter each frame. This keeps the fade machinery
//! independent of whether the volume ultimately lands on a Hearken sound, a
//! null backend, or a plain multiplier -- the caller wires that up.
//!
//! Typical use for a crossfade:
//!
//! ```ignore
//! let mut sched = FadeScheduler::new();
//! sched.add(Fade::new(|v| clip.set_volume(v), 1.0, 0.0, 0.9));
//! sched.add(Fade::new(|v| set_loop_gain(v), 0.0, 1.0, 0.9));
//! // then, each frame:
//! sched.update(dt);
//! ```
//!
//! Port of `freight_fate/audio_fades.py`.

use std::f64::consts::PI;

/// An easing curve: linear progress `t` in [0, 1] to eased progress in
/// [0, 1], with f(0) == 0 and f(1) == 1.
pub type CurveFn = fn(f64) -> f64;

fn clamp01(t: f64) -> f64 {
    t.clamp(0.0, 1.0)
}

fn linear(t: f64) -> f64 {
    t
}

fn ease_in(t: f64) -> f64 {
    t * t
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// smoothstep
fn ease_in_out(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Slow-start exponential ramp (k = 4), normalized to hit exactly 0 and 1.
fn exponential(t: f64) -> f64 {
    let k: f64 = 4.0;
    ((k * t).exp() - 1.0) / (k.exp() - 1.0)
}

fn equal_power_in(t: f64) -> f64 {
    (t * PI / 2.0).sin()
}

fn equal_power_out(t: f64) -> f64 {
    1.0 - (t * PI / 2.0).cos()
}

// Easing curves map linear progress `t` in [0, 1] to eased progress in
// [0, 1], with f(0) == 0 and f(1) == 1. A Fade interpolates
// `start + (end - start) * curve(t)`.
//
// The equal-power pair keeps the summed loudness of a crossfade roughly
// constant: run the outgoing sound's fade with `equal_power_out` and the
// incoming one with `equal_power_in`.
pub const CURVES: &[(&str, CurveFn)] = &[
    ("linear", linear),
    ("ease_in", ease_in),
    ("ease_out", ease_out),
    ("ease_in_out", ease_in_out),
    ("exponential", exponential),
    // Equal-power crossfade pair (constant perceived loudness through a blend).
    ("equal_power_in", equal_power_in),
    ("equal_power_out", equal_power_out),
];

/// Look up a curve by name, defaulting to linear for unknown names.
pub fn curve(name: &str) -> CurveFn {
    CURVES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
        .unwrap_or(linear)
}

/// A single timed volume ramp advanced by [`Fade::advance`].
///
/// `apply(value)` is called every frame with the current interpolated
/// value; `duration_s` is the ramp length and `delay_s` an optional wait
/// before it begins (during the delay the value stays at `start`). Pass a
/// curve name from [`CURVES`] or a function. `on_done` fires once when the
/// ramp reaches `end`.
pub struct Fade {
    apply: Box<dyn FnMut(f64)>,
    start: f64,
    end: f64,
    duration: f64,
    curve: CurveFn,
    delay: f64,
    on_done: Option<Box<dyn FnMut()>>,
    elapsed: f64,
    done: bool,
}

impl Fade {
    /// A linear fade with no delay. The starting value is presented
    /// immediately (covers the delay window and zero-duration fades before
    /// the first advance lands) -- call sites depend on that.
    pub fn new(apply: impl FnMut(f64) + 'static, start: f64, end: f64, duration_s: f64) -> Self {
        Self::with_options(apply, start, end, duration_s, linear, 0.0, None)
    }

    /// The full constructor: curve function, delay and completion hook.
    pub fn with_options(
        apply: impl FnMut(f64) + 'static,
        start: f64,
        end: f64,
        duration_s: f64,
        curve: CurveFn,
        delay_s: f64,
        on_done: Option<Box<dyn FnMut()>>,
    ) -> Self {
        let mut fade = Self {
            apply: Box::new(apply),
            start,
            end,
            duration: duration_s.max(0.0),
            curve,
            delay: delay_s.max(0.0),
            on_done,
            elapsed: 0.0,
            done: false,
        };
        // Present the starting value immediately (covers the delay window and
        // zero-duration fades before the first advance lands).
        (fade.apply)(fade.start);
        fade
    }

    /// Use the named curve from [`CURVES`] (unknown names mean linear).
    pub fn with_curve_name(mut self, name: &str) -> Self {
        self.curve = curve(name);
        self
    }

    /// Wait this long before the ramp begins.
    pub fn with_delay(mut self, delay_s: f64) -> Self {
        self.delay = delay_s.max(0.0);
        self
    }

    /// Fire this once when the ramp reaches `end`.
    pub fn with_on_done(mut self, on_done: impl FnMut() + 'static) -> Self {
        self.on_done = Some(Box::new(on_done));
        self
    }

    pub fn done(&self) -> bool {
        self.done
    }

    /// Advance by `dt` seconds; return true once the ramp has finished.
    pub fn advance(&mut self, dt: f64) -> bool {
        if self.done {
            return true;
        }
        self.elapsed += dt.max(0.0);
        if self.elapsed < self.delay {
            return false;
        }
        let t = if self.duration <= 0.0 {
            1.0
        } else {
            clamp01((self.elapsed - self.delay) / self.duration)
        };
        let value = self.start + (self.end - self.start) * (self.curve)(t);
        (self.apply)(value);
        if t >= 1.0 {
            self.done = true;
            if let Some(on_done) = self.on_done.as_mut() {
                on_done();
            }
        }
        self.done
    }
}

/// Holds active fades and advances them together each frame.
#[derive(Default)]
pub struct FadeScheduler {
    fades: Vec<Fade>,
}

impl FadeScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, fade: Fade) {
        self.fades.push(fade);
    }

    pub fn update(&mut self, dt: f64) {
        if self.fades.is_empty() {
            return;
        }
        self.fades.retain_mut(|f| !f.advance(dt));
    }

    pub fn clear(&mut self) {
        self.fades.clear();
    }

    pub fn len(&self) -> usize {
        self.fades.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fades.is_empty()
    }
}

#[cfg(test)]
mod tests {
    //! The reusable, backend-agnostic fade utility (curves, Fade, FadeScheduler).
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn recorder() -> (Rc<RefCell<Vec<f64>>>, impl FnMut(f64) + 'static) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        (seen, move |v| sink.borrow_mut().push(v))
    }

    fn last(seen: &Rc<RefCell<Vec<f64>>>) -> f64 {
        *seen.borrow().last().unwrap()
    }

    #[test]
    fn test_every_curve_pins_its_endpoints() {
        for (name, f) in CURVES {
            assert!(f(0.0).abs() < 1e-9, "{name}");
            assert!((f(1.0) - 1.0).abs() < 1e-9, "{name}");
        }
    }

    #[test]
    fn test_curve_lookup_defaults_to_linear_for_unknown_names() {
        assert_eq!(curve("nope")(0.4), 0.4);
    }

    #[test]
    fn test_equal_power_pair_keeps_constant_energy() {
        // out^2 + in^2 == 1 at every point is the defining property of an
        // equal-power crossfade.
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let out = 1.0 - curve("equal_power_out")(t); // remaining level of the fade-out
            let gain_in = curve("equal_power_in")(t);
            assert!((out * out + gain_in * gain_in - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn test_fade_reaches_end_after_duration() {
        let (seen, apply) = recorder();
        let mut fade = Fade::new(apply, 1.0, 0.0, 1.0).with_curve_name("linear");
        assert_eq!(last(&seen), 1.0); // starting value applied immediately
        assert!(!fade.advance(0.5));
        assert!((last(&seen) - 0.5).abs() < 1e-9);
        assert!(fade.advance(0.6)); // crosses the end
        assert_eq!(last(&seen), 0.0);
        assert!(fade.done());
    }

    #[test]
    fn test_fade_honors_delay() {
        let (seen, apply) = recorder();
        let mut fade = Fade::new(apply, 0.0, 1.0, 1.0).with_delay(0.5);
        fade.advance(0.4); // still inside the delay
        assert_eq!(last(&seen), 0.0);
        fade.advance(0.6); // elapsed 1.0s total => 0.5s into the 1.0s ramp
        assert!((last(&seen) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_zero_duration_fade_snaps_to_end() {
        let (seen, apply) = recorder();
        let mut fade = Fade::new(apply, 0.0, 1.0, 0.0);
        assert!(fade.advance(0.0));
        assert_eq!(last(&seen), 1.0);
    }

    #[test]
    fn test_on_done_fires_exactly_once() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sink = calls.clone();
        let mut fade =
            Fade::new(|_v| {}, 0.0, 1.0, 0.5).with_on_done(move || sink.borrow_mut().push(1));
        fade.advance(0.6);
        fade.advance(0.6);
        assert_eq!(*calls.borrow(), vec![1]);
    }

    #[test]
    fn test_scheduler_advances_and_drops_finished_fades() {
        let (a, apply_a) = recorder();
        let (b, apply_b) = recorder();
        let mut sched = FadeScheduler::new();
        sched.add(Fade::new(apply_a, 0.0, 1.0, 0.5));
        sched.add(Fade::new(apply_b, 0.0, 1.0, 2.0));
        assert_eq!(sched.len(), 2);
        sched.update(1.0); // first fade finishes, second still running
        assert_eq!(sched.len(), 1);
        assert_eq!(last(&a), 1.0);
        assert!(0.0 < last(&b) && last(&b) < 1.0);
        sched.clear();
        assert_eq!(sched.len(), 0);
    }

    #[test]
    fn test_curve_shapes_are_distinct_at_the_midpoint() {
        // A crude guard that the library actually offers different shapes to tune.
        let mid = |name: &str| curve(name)(0.5);
        assert_eq!(mid("linear"), 0.5);
        assert!(mid("ease_in") < 0.5 && 0.5 < mid("ease_out"));
        assert!((mid("ease_in_out") - 0.5).abs() < 1e-9);
    }
}
