//! The cab's sound each frame: the glass moving, the wind through the open
//! window, the blower, and the lift the open window gives the outside.
//!
//! Pure ambience (see `ff_core::sim::cab`): nothing here is spoken.

use ff_core::cab_sounds::{FAN_KEY, WIND_RUSH_KEY};
use ff_core::sim::cab::CabState;

use crate::app::GameContext;
use crate::audio::{RadioVoicing, CH_CAB_FAN, CH_CAB_WIND};
use crate::states::driving::DrivingState;

/// The driver sits on the left: the window rush leans that way.
const WIND_RUSH_PAN: f64 = -0.5;

impl DrivingState {
    /// Called first in `update_audio`, so the road and weather loops set
    /// later in the same frame already carry the window's lift.
    pub fn update_cab_audio(&mut self, ctx: &mut GameContext, dt: f64) {
        self.cab.step(dt);
        ctx.audio.set_outside_lift(self.cab.outside_lift());
        self.apply_radio_voicing(ctx);

        let speed = self.trip.truck.velocity_mps;
        let rush = self.cab.wind_rush_gain(speed);
        if rush < 0.01 {
            ctx.audio.stop_loop_with(CH_CAB_WIND, 300);
        } else {
            ctx.audio
                .start_loop_with(CH_CAB_WIND, WIND_RUSH_KEY, rush, 300);
            ctx.audio.set_loop_pan(CH_CAB_WIND, WIND_RUSH_PAN);
            ctx.audio
                .set_loop_rate(CH_CAB_WIND, CabState::wind_rush_rate(speed));
        }

        // The blower runs off the engine: with it off the fan is silent,
        // and it comes back at the chosen speed when the engine starts.
        match self.cab.fan.mix() {
            Some((gain, rate)) if self.trip.truck.engine_on => {
                ctx.audio.start_loop_with(CH_CAB_FAN, FAN_KEY, gain, 400);
                ctx.audio.set_loop_rate(CH_CAB_FAN, rate);
            }
            _ => ctx.audio.stop_loop_with(CH_CAB_FAN, 400),
        }
    }

    /// The radio through the cab's speakers (or clean, as Settings says),
    /// following the driver's glass. Also run while a menu covers the
    /// drive, so a Radio sound change there is heard at once.
    pub fn apply_radio_voicing(&self, ctx: &mut GameContext) {
        let voicing = RadioVoicing {
            cab_speakers: ctx.settings.radio_cab_speakers,
            window: self.cab.glass(),
        };
        ctx.audio.set_radio_voicing(Some(voicing));
    }
}
