//! The exit ramp past the gore, piece by piece: the deceleration lane where
//! the truck sheds to the exit speed, the ramp curve that speed is for, and
//! the run down to the stop bar.
//!
//! Realistic exit redesign, owner-approved 2026-09-24. A deceleration lane
//! is a full lane beside the through lanes and exists so a driver leaves at
//! road speed and sheds inside it: the Green Book sizes it assuming no
//! deceleration in the through lanes, and TxDOT's manual says to assume all
//! of it happens in the speed-change lane. So the exit's braking lives HERE,
//! and the exit speed (the ramp's advisory, MUTCD W13-2, which stands along
//! this lane) governs the curve that follows it, not the gore.

use crate::speech::EventPriority;
use ff_core::data::curves::min_radius_ft;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_stops::assist_servo_brake;

impl DrivingState {
    /// How far past the gore the truck has come on the ramp it took there.
    ///
    /// None off a laid-out ramp: no ramp, or a ramp the game put the truck on
    /// some other way (the loop-back to a missed terminal), which has no
    /// lane or curve behind it.
    pub fn ramp_travelled_mi(&self) -> Option<f64> {
        let layout = self.ramp_layout?;
        let left = self.ramp_mi?;
        Some(layout.length_mi() + RAMP_ACCESS_MI - left)
    }

    /// Road left in the deceleration lane, while the truck is in it.
    pub fn deceleration_lane_left_mi(&self) -> Option<f64> {
        let layout = self.ramp_layout?;
        let travelled = self.ramp_travelled_mi()?;
        let left = layout.decel_mi - travelled;
        (left > 0.0).then_some(left)
    }

    /// The ramp speed that holds where the truck is now: the exit speed
    /// through the deceleration lane and the curve, then the ramp's design
    /// speed on the run past the curve.
    ///
    /// The exit speed is the advisory sign's, and the sign is for the
    /// curve. Held gore to bar it kept a truck at 15 down all 1,500 feet of
    /// Cleveland's exit 170A, a minute and a half on a ramp whose bend is a
    /// few truck lengths (owner, 2026-10-24). Off a laid-out ramp there are
    /// no pieces to tell apart, so the exit speed holds throughout.
    pub fn ramp_cap_here_mph(&self) -> f64 {
        let exit = self.armed_ramp_mph(None);
        match self.ramp_layout {
            Some(layout) if self.on_laid_out_ramp() && !self.short_of_ramp_curve_end() => {
                exit.max(layout.tangent_mph)
            }
            _ => exit,
        }
    }

    pub fn in_deceleration_lane(&self) -> bool {
        self.deceleration_lane_left_mi().is_some()
    }

    /// Whether the truck is still on the lane or in the curve of the ramp it
    /// took at the gore.
    pub fn short_of_ramp_curve_end(&self) -> bool {
        match (self.ramp_layout, self.ramp_travelled_mi()) {
            (Some(layout), Some(travelled)) => travelled < layout.decel_mi + layout.curve_mi,
            _ => false,
        }
    }

    /// Whether the truck is on a ramp it took at a gore, anywhere from the
    /// gore to the stop at its end. The clock runs real over all of it on
    /// every exit (`update_exit_with_input`): the lane, the curve and the stop
    /// at the entrance are all braked for in real seconds. Real time only to
    /// the curve's end left a free-flowing ramp's run to the entrance on the
    /// compressed clock, and at five times facility stopping assistance met
    /// the entrance at 33 mph (every-assist audit, 2026-09-24). Not on the
    /// facility's streets, which keep `ramp_mi` set and have their own clock
    /// rules (`dock_run_in`, the turns).
    pub fn on_laid_out_ramp(&self) -> bool {
        self.ramp_mi.is_some() && self.ramp_layout.is_some() && !self.surface_chain
    }

    /// The radius of the ramp curve, in feet, while the truck is in it.
    ///
    /// DERIVED from the speed the ramp is built for through the same AASHTO
    /// point-mass control the curve bake uses (`min_radius_ft`), never a
    /// flat push. Only inside the curve's own length: the lane before it
    /// runs beside the mainline and the run to the bar is straight. This used
    /// to bend the whole half mile, gore to driveway -- two loops' worth of
    /// turning on every exit.
    pub fn ramp_curve_radius_ft(&self) -> Option<f64> {
        if self.surface_chain {
            return None;
        }
        let layout = self.ramp_layout?;
        let travelled = self.ramp_travelled_mi()?;
        let into_curve = travelled - layout.decel_mi;
        if !(0.0..layout.curve_mi).contains(&into_curve) {
            return None;
        }
        Some(min_radius_ft(layout.curve_mph).max(1.0))
    }

    /// Whether an assist brakes the deceleration lane down to the exit speed.
    ///
    /// Facility stopping assistance too: it takes the pedals for the stop at
    /// the ramp's end from the gore on, and a profile to that stop alone
    /// carried the truck into the ramp curve at 62 against 49 (every-assist
    /// audit, 2026-09-24). Whatever has the pedals on the ramp answers to the
    /// exit speed first.
    pub fn ramp_speed_assisted(ctx: &GameContext) -> bool {
        ctx.settings.exit_speed_assist
            || ctx.settings.route_transition_assist
            || ctx.settings.curve_speed_assist
            || ctx.settings.destination_approach_assist
    }

    /// The driver's own foot on the throttle, key or pad.
    pub fn driver_accelerating(ctx: &GameContext) -> bool {
        !ctx.co_driver_away
            && (ctx.bindings.pressed(&ctx.input, Action::Accelerate)
                || (ctx.controller.active() && ctx.controller.throttle() > 0.05))
    }

    /// Work the deceleration lane: publish the ramp's grade, and let the
    /// assists brake to the exit speed by the curve.
    ///
    /// Exit speed assistance, route-transition assistance and curve
    /// assistance all answer here, as one servo: the first two promise help
    /// with the exit's speed, and the ramp curve is the bend ahead that curve
    /// assistance brakes for on the approach, as it does a mapped bend. Three
    /// servos on one lane would fight and speak three times. Runs ahead of
    /// the physics step, and its press is a pedal floor in the frame
    /// (`decel_lane_brake`), like the terminal's.
    pub fn update_deceleration_lane(&mut self, ctx: &mut GameContext) {
        // The lane runs beside the mainline and shares its grade; the ramp
        // proper has its own, ASSUMED level (see `ExitRampLayout::grade`).
        self.trip.ramp_grade = match (self.ramp_mi, self.ramp_layout) {
            (Some(_), Some(layout)) if !self.in_deceleration_lane() => Some(layout.grade),
            _ => None,
        };
        let Some(left_mi) = self
            .deceleration_lane_left_mi()
            .filter(|_| Self::ramp_speed_assisted(ctx))
        else {
            self.decel_lane_brake = 0.0;
            return;
        };
        // The driver's own throttle overrides, as it does the terminal's.
        if Self::driver_accelerating(ctx) {
            self.decel_lane_brake = 0.0;
            return;
        }
        // The exit speed, or less where the ramp curve would cost this load
        // something at it: a part-filled tank, or a sign the curve cannot
        // hold for the lane mode in use (`driving_rollover`).
        let exit_mph = self.armed_ramp_mph(None);
        let target_mps = self
            .ramp_curve_safe_mph(ctx)
            .map_or(exit_mph, |safe| exit_mph.min(safe))
            / MPH_PER_MPS;
        let v_mps = self.trip.truck.velocity_mps.max(0.0);
        let gap_m = 0.5f64.max(left_mi * METERS_PER_MILE);
        // Priced in the seconds the truck slows in, as the curve servo's
        // shed is: the road passes `scale` times faster than the brakes work.
        // The lane runs on the real clock, so this is 1 there; it is here so
        // the price stays true if that ever stops being so.
        let scale = self.trip.effective_time_scale().max(1.0);
        let needed = (v_mps * v_mps - target_mps * target_mps).max(0.0) * scale / (2.0 * gap_m);
        let idle = self.decel_lane_brake <= 0.0;
        if needed < RAMP_ASSIST_DECEL_RELEASE_MPS2
            || (idle && needed < RAMP_ASSIST_DECEL_START_MPS2)
        {
            self.decel_lane_brake = 0.0;
            return;
        }
        self.decel_lane_brake = assist_servo_brake(self.decel_lane_brake, needed, &self.trip.truck);
        self.trip.truck.throttle = 0.0;
        self.trip.truck.brake = self.trip.truck.brake.max(self.decel_lane_brake);
        if self.decel_lane_assist_said {
            return;
        }
        self.decel_lane_assist_said = true;
        // ROUTE: an automation naming that it just took the brakes, the same
        // class as every other assist's slowing line.
        ctx.say_event_with(
            "Slowing for the ramp.",
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Confirmation),
        );
    }
}
