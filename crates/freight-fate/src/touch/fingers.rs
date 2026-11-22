//! Raw fingers through the shared recognizer: what Android, the iPhone with
//! Touch gestures, New gesture reading on, and the agent server's `fingers`
//! tool have in common.
//!
//! Each hands its finger events over with their own timestamps, then lets
//! the recognizer's clock catch up to now, so a hold, a waiting tap and the
//! second finger's steering begin on time even in a frame with no finger
//! events. The gestures come back in order for [`TouchInput`] to translate,
//! each with the haptic it asks for.
//!
//! [`TouchInput`]: super::TouchInput

use ff_core::settings::Settings;
use ff_core::touch::recognizer::{Recognizer, Tuning};
use ff_core::touch::{FingerEvent, Gesture};

/// The recognizer's thresholds from the Touch gestures settings.
pub fn tuning(settings: &Settings) -> Tuning {
    Tuning::new(
        settings.touch_flick_points,
        settings.touch_flick_ms,
        settings.touch_deep_swipe_factor,
        settings.touch_still_hold_ms,
        settings.touch_rotate_degrees,
    )
}

/// Feed `events` in order, let time pass to `now` (seconds on the events'
/// clock), and return what the fingers made.
pub fn read(
    recognizer: &mut Recognizer,
    events: impl IntoIterator<Item = FingerEvent>,
    now: f64,
) -> Vec<Gesture> {
    let mut last = f64::NEG_INFINITY;
    for event in events {
        last = last.max(event.t);
        recognizer.feed(event);
    }
    // A queue that ran behind the clock must not tick time backwards.
    recognizer.tick(now.max(last));
    recognizer.drain()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff_core::touch::FingerKind;

    fn finger(kind: FingerKind, x: f32, y: f32, t: f64) -> FingerEvent {
        FingerEvent {
            finger: 7,
            kind,
            x,
            y,
            t,
        }
    }

    /// A flick read a frame late is still a flick: the events carry their
    /// own times, so the late frame's clock does not stretch the stroke
    /// into a hold.
    #[test]
    fn a_late_frame_keeps_a_flick_a_flick() {
        let mut recognizer = Recognizer::default();
        let events = [
            finger(FingerKind::Down, 100.0, 300.0, 1.0),
            finger(FingerKind::Move, 100.0, 260.0, 1.04),
            finger(FingerKind::Up, 100.0, 240.0, 1.08),
        ];
        // The frame that reads them runs half a second after the lift.
        let gestures = read(&mut recognizer, events, 1.58);
        assert_eq!(gestures, vec![Gesture::SwipeUp]);
    }

    /// The same stroke still down when the frame reads it is the gas.
    #[test]
    fn a_stroke_still_down_becomes_the_gas_on_the_frame_clock() {
        let mut recognizer = Recognizer::default();
        let events = [
            finger(FingerKind::Down, 100.0, 300.0, 1.0),
            finger(FingerKind::Move, 100.0, 260.0, 1.04),
        ];
        assert_eq!(read(&mut recognizer, events, 1.1), Vec::<Gesture>::new());
        assert_eq!(
            read(&mut recognizer, [], 1.3),
            vec![Gesture::HoldUpperBegan]
        );
        let lift = [finger(FingerKind::Up, 100.0, 260.0, 1.5)];
        assert_eq!(read(&mut recognizer, lift, 1.5), vec![Gesture::HoldEnded]);
    }

    /// A cancel (a call, Control Center) ends the hold the fingers had.
    #[test]
    fn a_cancel_ends_the_hold() {
        let mut recognizer = Recognizer::default();
        let events = [
            finger(FingerKind::Down, 100.0, 300.0, 1.0),
            finger(FingerKind::Move, 100.0, 340.0, 1.04),
        ];
        read(&mut recognizer, events, 1.4);
        let cancel = [finger(FingerKind::Cancel, 0.0, 0.0, 1.5)];
        assert_eq!(read(&mut recognizer, cancel, 1.5), vec![Gesture::HoldEnded]);
        assert_eq!(recognizer.phase_key(), "idle");
    }

    #[test]
    fn the_tuning_comes_from_the_settings() {
        let settings = Settings {
            touch_flick_points: 45.0,
            ..Settings::default()
        };
        assert_eq!(tuning(&settings).flick_points, 45.0);
    }
}
