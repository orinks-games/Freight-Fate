//! The driver window and climate keys: Z, Alt Z and Shift Z by default.
//!
//! Each press is confirmed in a word or two. Parked with the engine off the
//! change is otherwise inaudible, and a toggle the player cannot hear land
//! is a toggle they press twice.

use ff_core::cab_sounds::{AC_CLUTCH_KEY, WINDOW_MOTOR_KEY};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, Say};
use crate::states::driving::DrivingState;

impl DrivingState {
    pub fn toggle_driver_window(&mut self, ctx: &mut GameContext) {
        let line = self.cab.toggle_window();
        ctx.audio.play_with(WINDOW_MOTOR_KEY, 0.6, -0.5);
        ctx.say_with(line, Say::new().category(SpeechCategory::Confirmation));
    }

    pub fn step_cab_fan(&mut self, ctx: &mut GameContext) {
        let was_on = self.cab.compressor_on();
        let line = self.cab.step_fan();
        self.click_compressor_if_engaged(ctx, was_on);
        ctx.say_with(line, Say::new().category(SpeechCategory::Confirmation));
    }

    pub fn toggle_cab_climate(&mut self, ctx: &mut GameContext) {
        let was_on = self.cab.compressor_on();
        let line = self.cab.toggle_mode();
        self.click_compressor_if_engaged(ctx, was_on);
        ctx.say_with(line, Say::new().category(SpeechCategory::Confirmation));
    }

    /// The compressor clutch clicks in when air conditioning starts blowing
    /// with the engine running to drive it.
    fn click_compressor_if_engaged(&mut self, ctx: &mut GameContext, was_on: bool) {
        if !was_on && self.cab.compressor_on() && self.trip.truck.engine_on {
            ctx.audio.play_with(AC_CLUTCH_KEY, 0.5, 0.0);
        }
    }
}
