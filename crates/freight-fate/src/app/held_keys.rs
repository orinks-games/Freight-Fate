//! Held keys as the driving loop should see them, screen reader or not.
//!
//! Driving reads the pedals and the wheel by polling, and JAWS re-sends a
//! held arrow as a train of zero-frame press-and-release pairs. Hearken's
//! tracker turns that train back into a hold, and with the bridge on (JAWS
//! is the voice) a tap on another key, such as Space for the speed, does
//! not end an established arrow hold. See `hearken::held_keys` for the
//! full account and the measured timing.

pub use hearken::held_keys::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::base::{Key, Mods};

    const DT: f64 = 1.0 / 60.0;
    const FRAME_MS: u64 = 17; // what a 60 fps frame rounds to
    const DELAY_MS: u64 = 500;
    const INTERVAL_MS: u64 = 33;
    /// The owner's JAWS log, 2026-08-24: first repeat at 512 ms, then these.
    const JAWS_FIRST_REPEAT_MS: u64 = 512;
    const JAWS_SPACINGS_MS: [u64; 14] = [
        263, 245, 269, 271, 242, 251, 270, 250, 244, 271, 249, 242, 254, 250,
    ];

    /// Advance the tracker frame by frame with a fake clock.
    struct Sim {
        keys: HeldKeys,
        now_ms: u64,
    }

    impl Sim {
        fn new() -> Self {
            let mut keys = HeldKeys::with_timing(DELAY_MS, INTERVAL_MS);
            keys.begin_frame(DT);
            Self { keys, now_ms: 0 }
        }

        fn frame(&mut self) {
            self.frame_of(DT);
        }

        fn frame_of(&mut self, dt: f64) {
            self.keys.begin_frame(dt);
            self.now_ms += (dt * 1000.0).round() as u64;
        }

        fn pair(&mut self, key: Key) {
            self.keys.press(key, Mods::NONE);
            self.keys.release(key, Mods::NONE);
        }

        /// When a screen reader re-sends pairs for a key held `seconds`.
        fn pair_times(&self, seconds: f64, first_repeat: u64, spacings: &[u64]) -> Vec<u64> {
            let start = self.now_ms;
            let end = start + (seconds * 1000.0) as u64;
            let mut times = vec![start, start + first_repeat];
            let mut i = 0;
            while *times.last().unwrap() < end {
                times.push(times.last().unwrap() + spacings[i % spacings.len()]);
                i += 1;
            }
            times.retain(|&t| t < end);
            times
        }

        /// Deliver the pairs for a hold, frame by frame; return each frame's
        /// reading from the first pair on.
        fn screen_reader_hold(
            &mut self,
            key: Key,
            seconds: f64,
            first_repeat: u64,
            spacings: &[u64],
        ) -> Vec<bool> {
            let mut pairs = self.pair_times(seconds, first_repeat, spacings);
            let end = self.now_ms + (seconds * 1000.0) as u64;
            let mut readings = Vec::new();
            while self.now_ms < end {
                self.frame();
                if pairs.first().is_some_and(|&t| self.now_ms >= t) {
                    pairs.remove(0);
                    self.pair(key);
                }
                readings.push(self.keys.is_pressed(key));
            }
            readings
        }

        fn ms_until_released(&mut self, key: Key) -> u64 {
            let start = self.now_ms;
            while self.keys.is_pressed(key) {
                self.frame();
                assert!(
                    self.now_ms - start <= 2000,
                    "still held {} ms after the pairs stopped",
                    self.now_ms - start
                );
            }
            self.now_ms - start
        }
    }

    fn first_gap(readings: &[bool]) -> Option<usize> {
        readings.iter().position(|&held| !held)
    }

    #[test]
    fn physical_hold_still_reads_straight_from_the_key_state() {
        let mut sim = Sim::new();
        sim.keys.press(Key::Up, Mods::NONE);
        assert!(sim.keys.is_pressed(Key::Up));
        assert!(!sim.keys.is_pressed(Key::Down));
        for _ in 0..5 {
            sim.frame();
            assert!(sim.keys.is_pressed(Key::Up));
        }
        sim.keys.release(Key::Up, Mods::NONE);
        assert!(!sim.keys.is_pressed(Key::Up));
    }

    #[test]
    fn without_a_frame_clock_the_pulse_layer_is_inert() {
        // The harness and the tests feed keys straight in; a tap through
        // press and release must keep meaning exactly that.
        let mut keys = HeldKeys::with_timing(DELAY_MS, INTERVAL_MS);
        keys.press(Key::Up, Mods::NONE);
        assert!(keys.is_pressed(Key::Up));
        keys.release(Key::Up, Mods::NONE);
        assert!(!keys.is_pressed(Key::Up));
        assert!(!keys.pulsed(Key::Up));
    }

    #[test]
    fn a_re_injected_pair_reads_held_until_the_first_repeat_would_come() {
        let mut sim = Sim::new();
        sim.pair(Key::Up);
        assert!(sim.keys.is_pressed(Key::Up));
        assert!(sim.keys.pulsed(Key::Up));
        let pressed_at = sim.now_ms;
        while sim.now_ms < pressed_at + DELAY_MS {
            sim.frame();
            assert!(
                sim.keys.is_pressed(Key::Up),
                "lapsed {} ms after the press",
                sim.now_ms - pressed_at
            );
        }
        while sim.now_ms < pressed_at + DELAY_MS + FRESH_GRACE_MS + FRAME_MS {
            sim.frame();
        }
        assert!(!sim.keys.is_pressed(Key::Up));
    }

    #[test]
    fn the_owners_jaws_train_is_one_continuous_hold_from_the_first_pair() {
        let mut sim = Sim::new();
        let readings =
            sim.screen_reader_hold(Key::Up, 4.0, JAWS_FIRST_REPEAT_MS, &JAWS_SPACINGS_MS);
        assert_eq!(first_gap(&readings), None, "the hold broke");
        // Letting go reads late by one spacing plus grace: the price of a
        // screen reader that only re-sends the key four times a second.
        let max_spacing = *JAWS_SPACINGS_MS.iter().max().unwrap();
        let lag = sim.ms_until_released(Key::Up);
        assert!(lag <= max_spacing + REPEAT_GRACE_MS + 2 * FRAME_MS, "{lag}");
        // The fake clock lands pairs on frame boundaries, so the learned
        // spacing can sit one frame off the nominal value.
        let learned = sim.keys.learned_spacing_ms().expect("learned");
        assert!(learned.abs_diff(max_spacing) <= FRAME_MS, "{learned}");
        // The next hold, on another key, starts with the spacing known.
        let readings =
            sim.screen_reader_hold(Key::Down, 3.0, JAWS_FIRST_REPEAT_MS, &JAWS_SPACINGS_MS);
        assert_eq!(first_gap(&readings), None);
        let lag = sim.ms_until_released(Key::Down);
        assert!(lag <= max_spacing + REPEAT_GRACE_MS + 2 * FRAME_MS, "{lag}");
    }

    #[test]
    fn a_fast_repeat_train_lapses_quickly_once_its_spacing_is_learned() {
        let mut sim = Sim::new();
        let readings = sim.screen_reader_hold(Key::Up, 2.0, DELAY_MS, &[INTERVAL_MS]);
        assert_eq!(first_gap(&readings), None);
        let learned = sim.keys.learned_spacing_ms().expect("learned");
        assert!(learned.abs_diff(INTERVAL_MS) <= FRAME_MS, "{learned}");
        let lag = sim.ms_until_released(Key::Up);
        assert!(lag <= INTERVAL_MS + REPEAT_GRACE_MS + 3 * FRAME_MS, "{lag}");
        // Once a hold has lapsed, a fresh press earns the full fresh pulse.
        sim.pair(Key::Up);
        let pressed_at = sim.now_ms;
        while sim.now_ms < pressed_at + DELAY_MS {
            sim.frame();
            assert!(sim.keys.is_pressed(Key::Up));
        }
    }

    #[test]
    fn a_fingers_rhythm_never_teaches_the_repeat_spacing() {
        // Real taps, released a few frames later, at a steady 200 ms.
        let mut sim = Sim::new();
        for _ in 0..6 {
            sim.keys.press(Key::Down, Mods::NONE);
            for _ in 0..3 {
                sim.frame();
            }
            sim.keys.release(Key::Down, Mods::NONE);
            assert!(!sim.keys.is_pressed(Key::Down));
            for _ in 0..8 {
                sim.frame();
            }
        }
        assert_eq!(sim.keys.learned_spacing_ms(), None);
        assert_eq!(sim.keys.repeat_pulse_ms(), sim.keys.fresh_pulse_ms());
    }

    #[test]
    fn sdl_auto_repeat_of_a_held_key_never_makes_a_pair() {
        // The keyboard's own repeat arrives as more presses of a key that is
        // already down; the finger lifting in the same frame as one of those
        // is a real release, not a screen reader's pair.
        let mut sim = Sim::new();
        sim.keys.press(Key::Up, Mods::NONE);
        for _ in 0..40 {
            sim.frame();
            sim.keys.press(Key::Up, Mods::NONE); // SDL repeat, every frame
            assert!(sim.keys.is_pressed(Key::Up));
        }
        sim.frame();
        sim.keys.press(Key::Up, Mods::NONE); // one more repeat...
        sim.keys.release(Key::Up, Mods::NONE); // ...and the finger lifts
        assert!(!sim.keys.is_pressed(Key::Up));
        for _ in 0..3 {
            sim.frame();
            assert!(!sim.keys.is_pressed(Key::Up));
        }
        assert_eq!(sim.keys.learned_spacing_ms(), None);
    }

    #[test]
    fn a_pair_that_lands_after_a_hitch_is_not_a_hold() {
        // A whole real tap can arrive in one batch after a long frame; the
        // honest answer is "not held", not a half-second of brake.
        let mut sim = Sim::new();
        sim.frame_of((SYNTHETIC_FRAME_MAX_MS + 100) as f64 / 1000.0);
        sim.pair(Key::Down);
        assert!(!sim.keys.is_pressed(Key::Down));
    }

    #[test]
    fn a_second_tap_before_the_delay_is_a_fresh_press_not_a_repeat() {
        let mut sim = Sim::new();
        sim.pair(Key::Down);
        for _ in 0..9 {
            sim.frame(); // about 150 ms later: a double tap
        }
        sim.pair(Key::Down);
        let second_at = sim.now_ms;
        while sim.now_ms < second_at + DELAY_MS {
            sim.frame();
            assert!(sim.keys.is_pressed(Key::Down));
        }
    }

    #[test]
    fn each_key_keeps_its_own_hold() {
        let mut sim = Sim::new();
        sim.pair(Key::Up);
        sim.frame();
        sim.pair(Key::Left);
        assert!(sim.keys.is_pressed(Key::Up) && sim.keys.is_pressed(Key::Left));
        assert!(!sim.keys.is_pressed(Key::Right));
        sim.frame();
        sim.keys.release(Key::Up, Mods::NONE); // a later release: the finger
        assert!(!sim.keys.is_pressed(Key::Up));
        assert!(sim.keys.is_pressed(Key::Left));
    }

    #[test]
    fn clearing_the_pulses_keeps_the_learning_and_clear_drops_it_all() {
        let mut sim = Sim::new();
        sim.screen_reader_hold(Key::Up, 1.5, JAWS_FIRST_REPEAT_MS, &JAWS_SPACINGS_MS);
        assert!(sim.keys.learned_spacing_ms().is_some());
        sim.keys.clear_pulses();
        assert!(!sim.keys.is_pressed(Key::Up));
        assert!(sim.keys.learned_spacing_ms().is_some());
        sim.pair(Key::Up);
        sim.keys.press(Key::B, Mods::NONE);
        sim.keys.clear();
        assert!(!sim.keys.is_pressed(Key::Up));
        assert!(!sim.keys.is_pressed(Key::B));
        assert_eq!(sim.keys.mods(), Mods::NONE);
    }

    /// Hold Up under JAWS for `seconds`, then tap Space the way a driver
    /// asks for the speed: the keyboard repeats only the last key pressed, so
    /// Up's pairs never come back.
    fn hold_up_then_tap_space(sim: &mut Sim, seconds: f64) {
        sim.screen_reader_hold(Key::Up, seconds, JAWS_FIRST_REPEAT_MS, &JAWS_SPACINGS_MS);
        sim.keys.press(Key::Space, Mods::NONE);
        for _ in 0..6 {
            sim.frame();
        }
        sim.keys.release(Key::Space, Mods::NONE);
    }

    #[test]
    fn under_jaws_a_tap_on_another_key_does_not_end_a_held_arrow() {
        let mut sim = Sim::new();
        sim.keys.set_bridge(true);
        hold_up_then_tap_space(&mut sim, 3.0);
        for _ in 0..60 {
            sim.frame();
            assert!(sim.keys.is_pressed(Key::Up), "the hold died at the tap");
        }
        // It still lets go: nothing keeps the pedal past the bridge.
        assert!(sim.ms_until_released(Key::Up) <= BRIDGE_MS);
    }

    #[test]
    fn without_jaws_a_tap_on_another_key_changes_nothing() {
        let mut sim = Sim::new();
        hold_up_then_tap_space(&mut sim, 3.0);
        for _ in 0..20 {
            sim.frame();
        }
        assert!(!sim.keys.is_pressed(Key::Up));
    }

    #[test]
    fn the_brake_never_bridges_the_throttle() {
        let mut sim = Sim::new();
        sim.keys.set_bridge(true);
        sim.screen_reader_hold(Key::Up, 3.0, JAWS_FIRST_REPEAT_MS, &JAWS_SPACINGS_MS);
        sim.pair(Key::Down);
        for _ in 0..20 {
            sim.frame();
        }
        assert!(!sim.keys.is_pressed(Key::Up));
    }

    #[test]
    fn a_bridged_hold_ends_within_the_cap_however_many_keys_are_tapped() {
        let mut sim = Sim::new();
        sim.keys.set_bridge(true);
        sim.screen_reader_hold(Key::Up, 3.0, JAWS_FIRST_REPEAT_MS, &JAWS_SPACINGS_MS);
        let start = sim.now_ms;
        while sim.now_ms - start < BRIDGE_TOTAL_MS + 500 {
            sim.keys.press(Key::Space, Mods::NONE);
            sim.frame();
            sim.keys.release(Key::Space, Mods::NONE);
            for _ in 0..20 {
                sim.frame();
            }
        }
        assert!(
            !sim.keys.is_pressed(Key::Up),
            "taps kept the pedal past the cap"
        );
    }

    #[test]
    fn windows_repeat_settings_decode_to_the_documented_timing() {
        assert_eq!(repeat_delay_ms(0), 250);
        assert_eq!(repeat_delay_ms(1), 500);
        assert_eq!(repeat_delay_ms(3), 1000);
        assert_eq!(repeat_delay_ms(99), 1000); // clamped, never a runaway pulse
        assert_eq!(repeat_interval_ms(31), 33);
        assert_eq!(repeat_interval_ms(0), 400);
        let keys = HeldKeys::with_timing(1000, 400);
        assert_eq!(keys.fresh_pulse_ms(), 1000 + FRESH_GRACE_MS);
        assert_eq!(keys.repeat_pulse_ms(), keys.fresh_pulse_ms()); // nothing learned yet
    }
}
