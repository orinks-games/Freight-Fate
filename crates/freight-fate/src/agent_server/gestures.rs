//! The `gesture` tool: touch gestures by name, sent the way the iPhone
//! screen sends them.
//!
//! Every gesture goes through the same [`TouchInput`] translator
//! `ios/ff_touch.m` feeds and lands in `App::dispatch_gesture` as a queued
//! player input, so a hold begins and ends exactly as a finger's does: the
//! translator keeps the pedal key down until a `lift`, and the app holds a
//! bound chord (the emergency brake, the horn, straighten) until the same
//! lift. A hold sent with `seconds` is lifted by the loop, like `pedal`.
//!
//! [`TouchInput`]: crate::touch::TouchInput

use super::{AgentPolicy, Reply, Until, PEDAL_SETTLE_SECONDS};
use crate::app::PlayerInputFrame;
use crate::touch::Gesture;

/// How long a gesture without `seconds` waits before its reply, so the
/// answer carries what the game said back.
const GESTURE_SETTLE_SECONDS: f64 = 1.0;

/// The longest a timed gesture hold may last, as for a pedal.
const MAX_GESTURE_HOLD_SECONDS: f64 = super::MAX_PEDAL_SECONDS;

/// A gesture that starts something a finger keeps going, and what ends it.
fn lift_for(gesture: Gesture) -> Option<Gesture> {
    match gesture {
        Gesture::HoldUpperBegan
        | Gesture::HoldLowerBegan
        | Gesture::HoldLeftBegan
        | Gesture::HoldRightBegan
        | Gesture::DeepSwipeDownBegan
        | Gesture::StillHoldBegan
        | Gesture::HornHoldBegan
        | Gesture::EmergencyBrakeHoldBegan => Some(Gesture::HoldEnded),
        Gesture::SecondSteerLeftBegan | Gesture::SecondSteerRightBegan => {
            Some(Gesture::SecondSteerEnded)
        }
        _ => None,
    }
}

impl AgentPolicy {
    /// Send one gesture, and park the reply until it has been answered.
    /// Returns true when the reply was parked (the frame stops taking
    /// requests), false when it was answered at once.
    pub(super) fn send_gesture(
        &mut self,
        gesture: Gesture,
        seconds: Option<f64>,
        length: Option<u16>,
        reply: Reply,
    ) -> bool {
        let lift = lift_for(gesture);
        if length.is_some() && !takes_length(gesture) {
            let _ = reply.send(Err(format!(
                "{} has no length; give length on a one-finger hold, deep_swipe or lift.",
                gesture.name()
            )));
            return false;
        }
        if length.is_some() && seconds.is_none() && gesture != Gesture::HoldEnded {
            let _ = reply.send(Err(
                "An untimed hold reports its length on the lift: send lift with length."
                    .to_string(),
            ));
            return false;
        }
        if let Some(seconds) = seconds {
            let Some(lift) = lift else {
                let _ = reply.send(Err(format!(
                    "{} is not a hold, so it takes no seconds.",
                    gesture.name()
                )));
                return false;
            };
            if self.gesture_lift.is_some() {
                let _ = reply.send(Err(
                    "A timed gesture hold is already down; its reply arrives when it lifts."
                        .to_string(),
                ));
                return false;
            }
            let seconds = seconds.clamp(0.05, MAX_GESTURE_HOLD_SECONDS);
            self.queue_touch(gesture);
            self.gesture_lift = Some((lift, seconds, length));
            self.wait_until(seconds + PEDAL_SETTLE_SECONDS, Until::Elapsed, false, reply);
            return true;
        }
        // A lift with a length reports it first, as the phone does.
        if let (Gesture::HoldEnded, Some(points)) = (gesture, length) {
            self.queue_touch(Gesture::Travel(points));
        }
        self.queue_touch(gesture);
        self.wait_until(GESTURE_SETTLE_SECONDS, Until::Elapsed, false, reply);
        true
    }

    /// Script what the translator makes of `gesture` as one frame.
    fn queue_touch(&mut self, gesture: Gesture) {
        let events = self.touch.handle(gesture).events;
        if !events.is_empty() {
            self.scripted.push_back(events);
        }
    }

    /// Each frame: keep a held touch key down through focus changes, and
    /// lift a timed hold when its time is up.
    pub(super) fn advance_touch(&mut self, input: &mut PlayerInputFrame<'_>, dt: f64) {
        if let Some(key) = self.touch.held() {
            input.assert_held(key);
        }
        if let Some((lift, remaining, length)) = self.gesture_lift.take() {
            let remaining = remaining - dt;
            if remaining > 0.0 {
                self.gesture_lift = Some((lift, remaining, length));
            } else {
                let travel = length.map(|points| self.touch.handle(Gesture::Travel(points)));
                let events = travel.into_iter().flat_map(|out| out.events);
                let lifted = self.touch.handle(lift).events;
                for event in events.chain(lifted) {
                    input.queue_player_input(event);
                }
            }
        }
    }

    /// Let go of every touch hold, as when the client goes away.
    pub(super) fn release_touch(&mut self, input: &mut PlayerInputFrame<'_>) {
        self.gesture_lift = None;
        let mut events = Vec::new();
        self.touch.release_into(&mut events);
        events.extend(self.touch.handle(Gesture::HoldEnded).events);
        for event in events {
            input.queue_player_input(event);
        }
    }
}

/// The gestures a finger's length goes with: a one-finger hold in a
/// direction, the deep swipe, and the lift that ends either.
fn takes_length(gesture: Gesture) -> bool {
    matches!(
        gesture,
        Gesture::HoldUpperBegan
            | Gesture::HoldLowerBegan
            | Gesture::HoldLeftBegan
            | Gesture::HoldRightBegan
            | Gesture::DeepSwipeDownBegan
            | Gesture::HoldEnded
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hold_has_a_lift_and_no_tap_does() {
        assert_eq!(lift_for(Gesture::HoldUpperBegan), Some(Gesture::HoldEnded));
        assert_eq!(
            lift_for(Gesture::SecondSteerLeftBegan),
            Some(Gesture::SecondSteerEnded)
        );
        assert_eq!(lift_for(Gesture::DoubleTap), None);
        assert_eq!(lift_for(Gesture::HoldEnded), None);
    }
}
