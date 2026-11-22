//! The `tilt` tool: the phone's motion sensor, played by the agent.
//!
//! The agent server holds a pose for the phone from its first frame, level
//! (zero degrees from flat) until a `tilt` call moves it, and sends that
//! angle every frame the way the sensor would, so tilt steering takes level,
//! steers and notices a gap exactly as it does on a phone. A call holds an
//! angle for a number of seconds, or sends nothing for them (`silent`), then
//! goes back to the pose it found, or to `then`.

use super::{AgentPolicy, Reply, Until, PEDAL_SETTLE_SECONDS};
use crate::app::PlayerInputFrame;

/// The longest one call holds a pose, as for a pedal.
const MAX_TILT_SECONDS: f64 = super::MAX_PEDAL_SECONDS;

impl AgentPolicy {
    /// Give the phone its first pose, level, on the first frame.
    pub(super) fn hold_phone(&mut self, input: &mut PlayerInputFrame<'_>) {
        if input.tilt_pose().is_none() {
            input.set_tilt_pose(Some(0.0));
        }
    }

    /// Start one `tilt` call. Returns true when its reply was parked.
    pub(super) fn send_tilt(
        &mut self,
        input: &mut PlayerInputFrame<'_>,
        degrees: Option<f64>,
        seconds: f64,
        then: Option<f64>,
        reply: Reply,
    ) -> bool {
        if self.tilt_hold.is_some() {
            let _ = reply.send(Err(
                "A tilt is already being held; its reply arrives when it ends.".to_string(),
            ));
            return false;
        }
        let before = input.tilt_pose().flatten().unwrap_or(0.0);
        let after = then.unwrap_or(before);
        let seconds = seconds.clamp(0.05, MAX_TILT_SECONDS);
        input.set_tilt_pose(degrees);
        self.tilt_hold = Some((seconds, after));
        self.wait_until(seconds + PEDAL_SETTLE_SECONDS, Until::Elapsed, false, reply);
        true
    }

    /// Count a held tilt down, and put the phone in its next pose when the
    /// time is up.
    pub(super) fn advance_tilt(&mut self, input: &mut PlayerInputFrame<'_>, dt: f64) {
        if let Some((remaining, after)) = self.tilt_hold.take() {
            let remaining = remaining - dt;
            if remaining > 0.0 {
                self.tilt_hold = Some((remaining, after));
            } else {
                input.set_tilt_pose(Some(after));
            }
        }
    }
}
