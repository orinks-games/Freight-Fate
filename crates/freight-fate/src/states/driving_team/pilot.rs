//! The co-driver at the wheel: one decision a frame, made through the
//! player's own controls.
//!
//! With every assist on, the truck already holds its lane, steers the
//! bends, takes the exits, keeps the speed and brakes for hazards. What the
//! co-driver adds is what is still the driver's job on the All assists
//! preset: starting from a stop, arming speed control, the open scales,
//! fuel, pulling over for a trooper, and confirming the gate at the end.
//! Each is a key the player has (`run_key_action`) or a pedal the frame
//! reads through [`DrivingState::held`].
//!
//! Anything the co-driver has no rule for, they hand back.

use ff_core::models::co_driver;
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, TRANSCRIPT_TARGET};
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::Pedals;

/// Refuel below this share of the tank.
pub(crate) const FUEL_STOP_FRACTION: f64 = 0.35;
/// A key the co-driver pressed gets this long to take effect before they
/// press another.
const KEY_BEAT_S: f64 = 1.5;
/// Stopped this long on the open road with nothing to wait for, the
/// co-driver hands back.
const STALL_LIMIT_S: f64 = 180.0;
/// How far ahead to look for a stop to end the shift at.
const SHIFT_STOP_REACH_MI: f64 = 40.0;
/// The pace a stop's reach is judged at: a stop farther than this covers in
/// the time left before the break is due is out of reach.
pub(crate) const REACH_MPH: f64 = 50.0;
/// How close to the posted limit the co-driver brings the truck by hand
/// before setting the cruise, and how far under it a set speed may sit
/// before they tap it up.
const CRUISE_SET_MARGIN_MPH: f64 = 3.0;
/// On the accelerator this long without reaching the limit (a grade, a
/// slow truck ahead), they set the cruise where they are.
const ACCEL_GIVE_UP_S: f64 = 45.0;
/// Braking for a grade descent control cannot hold, the co-driver takes
/// the truck this far under what it holds, then lets off: snub braking,
/// which keeps the drums cooler than riding them.
const DESCENT_SNUB_UNDER_MPH: f64 = 5.0;
/// The co-driver starts braking once the truck is this far over what descent
/// control holds and still gaining.
const DESCENT_SNUB_OVER_MPH: f64 = 5.0;
/// Short of a ramp's red light or sign, the co-driver creeps up to the
/// line at this speed and lets the assist stop the truck there.
const LINE_CREEP_MPH: f64 = 8.0;
/// How far ahead of a lane closure the co-driver starts moving out of the
/// lane: the taper is a mile long, and at ten times speed it passed in
/// seconds, so they merge when the first sign comes up, as a driver does.
const MERGE_LOOKAHEAD_MI: f64 = 1.0;
/// Dropping back behind traffic to merge out of a closed lane, the brake
/// comes off at this speed: slow enough for most gaps, and well above the
/// lowest speed a lane change can be made at.
const MERGE_BRAKE_FLOOR_MPH: f64 = 20.0;
/// The same, while the closure is still ahead of the taper: the truck only
/// needs to drop back, not crawl.
const MERGE_EARLY_FLOOR_MPH: f64 = 40.0;
/// A vehicle behind the cab and at least this much slower is left behind
/// by the truck's own speed rather than braked for.
const MERGE_PULL_CLEAR_MPH: f64 = 3.0;
/// The rig's length, 72 feet: a vehicle less than this behind the cab is
/// still beside the trailer.
const RIG_LENGTH_MI: f64 = 72.0 / 5280.0;

/// Why the co-driver is making for a stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopPurpose {
    Fuel,
    /// Their 30-minute break.
    Break,
    ShiftEnd,
}

/// The co-driver's working memory between frames.
#[derive(Debug, Clone, Default)]
pub struct Pilot {
    pub beat_s: f64,
    pub stalled_s: f64,
    /// The stop they are making for, and why.
    pub making_for: Option<(String, StopPurpose)>,
    /// Their shift is over; the next stop they pull into is where they
    /// hand back.
    pub shift_over: bool,
    /// The shift is over because the player has had their rest, not
    /// because the co-driver's hours ran out: no hurry to find the stop.
    pub rested: bool,
    /// The shift ends at the last lot the co-driver's hours reach, because
    /// the player's rest is not in yet: they sleep out the rest there.
    pub last_lot: bool,
    /// Seconds spent on the accelerator with speed control off.
    pub accel_s: f64,
    /// Seconds a hand-back has waited for the truck to come home from the
    /// passing lane.
    pub handback_wait_s: f64,
    /// Descent control said it cannot hold the grade: the speed the
    /// co-driver brakes the truck down to before letting off again.
    pub descent_snub_to: Option<f64>,
}

/// The longest a hand-back waits for the passing lane to clear. Traffic
/// that keeps the right lane shut is no reason to drive on past a break.
const HANDBACK_WAIT_MAX_S: f64 = 30.0;

/// Whether `stop` serves what the co-driver is stopping for.
pub(crate) fn stop_serves(stop: &RoadStop, purpose: StopPurpose) -> bool {
    match purpose {
        StopPurpose::Fuel => stop.actions.iter().any(|a| a == "fuel"),
        StopPurpose::Break => stop.actions.iter().any(|a| a == "break"),
        StopPurpose::ShiftEnd => stop.parking != "none",
    }
}

impl Pilot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Why they hand back at the stop that ends their shift.
    pub fn shift_end_reason(&self) -> &'static str {
        if self.last_lot {
            "last lot"
        } else {
            "shift"
        }
    }
}

impl DrivingState {
    /// One frame of the co-driver at the wheel. Runs before the frame
    /// itself, so the pedals set here are the ones the frame reads.
    pub fn pilot_frame(&mut self, ctx: &mut GameContext, dt: f64) {
        let Some(mut pilot) = self
            .team
            .as_mut()
            .and_then(|team| team.wheel.as_mut())
            .map(|wheel| std::mem::take(&mut wheel.pilot))
        else {
            return;
        };
        pilot.beat_s = (pilot.beat_s - dt).max(0.0);
        let pedals = match self.pilot_decide(ctx, &mut pilot, dt) {
            Ok(pedals) => pedals,
            Err(reason) => {
                self.co_driver_summon(ctx, &reason);
                return;
            }
        };
        let mut asleep = false;
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.pedals = pedals;
            wheel.pilot = pilot;
            asleep = wheel.mode == super::TeamMode::SleepThrough;
        }
        // Nobody is listening while the player sleeps.
        if !asleep {
            self.update_co_driver_radio(ctx);
        }
    }

    /// Press one of the player's keys, then give it a beat to work.
    fn pilot_press(&mut self, ctx: &mut GameContext, pilot: &mut Pilot, action: Action) {
        log::info!(target: TRANSCRIPT_TARGET, "[co-driver] presses {action:?}");
        self.run_key_action(ctx, action);
        pilot.beat_s = KEY_BEAT_S;
    }

    /// The pedals for this frame, or why the co-driver hands back.
    fn pilot_decide(
        &mut self,
        ctx: &mut GameContext,
        pilot: &mut Pilot,
        dt: f64,
    ) -> Result<Pedals, String> {
        let mut pedals = Pedals::default();
        let speed = self.trip.truck.speed_mph();
        if let Some(reason) = self.pilot_boundary(ctx, pilot) {
            // Out in the passing lane, or moving between lanes, the hand-back
            // waits until lane keeping has the truck home, so the player
            // does not take the wheel beside what was just gone around.
            // Damage, fuel and leaving the road do not wait, and nothing
            // waits longer than `HANDBACK_WAIT_MAX_S`.
            let urgent = matches!(reason.as_str(), "damage" | "fuel" | "off the road");
            let mid_pass = self.passing.is_some() || self.lane_change_target.is_some();
            pilot.handback_wait_s += dt;
            if urgent || !mid_pass || pilot.handback_wait_s >= HANDBACK_WAIT_MAX_S {
                return Err(reason);
            }
        } else {
            pilot.handback_wait_s = 0.0;
        }
        if speed < 1.0 {
            pilot.stalled_s += dt;
        } else {
            pilot.stalled_s = 0.0;
        }

        // The end of the load: stopped at the receiver's gate, the gate
        // opens the way Enter opens it, and opening it wakes the player.
        if self.trip.finished {
            if pilot.beat_s == 0.0 && self.assisted_facility_confirmation_ready(ctx) {
                self.open_ready_facility_arrival(ctx);
            }
            return Ok(pedals);
        }

        // A trooper's lights: signal, and brake onto the shoulder.
        if self.pull_over.is_some() {
            if !self.pull_over_signaled && pilot.beat_s == 0.0 {
                self.pilot_press(ctx, pilot, Action::TakeExit);
            }
            pedals.brake = speed > 0.5;
            return Ok(pedals);
        }

        // Parked at a stop the co-driver chose, or on the scale: the stop's
        // own menu, as T opens it.
        if speed <= DOCKING_MAX_MPH && self.pilot_at_stop_entrance() {
            if pilot.beat_s == 0.0 {
                self.pilot_press(ctx, pilot, Action::Rest);
            }
            pedals.brake = speed > 0.5;
            return Ok(pedals);
        }

        // Held at a chain checkpoint: chains on if there is a set aboard,
        // the way the pause menu puts them on. With none, the player decides
        // between waiting, a set from dispatch, or turning back.
        if self.chain_checkpoint_hold {
            if !self.usable_chains_aboard(ctx) {
                return Err("chains".to_string());
            }
            if speed <= 3.0 && pilot.beat_s == 0.0 {
                self.co_driver_chain_up(ctx);
                pilot.beat_s = KEY_BEAT_S;
            }
            pedals.brake = speed > 0.5;
            return Ok(pedals);
        }

        if !self.trip.truck.engine_on {
            if pilot.beat_s == 0.0 {
                self.pilot_press(ctx, pilot, Action::Engine);
            }
            return Ok(pedals);
        }
        if self.trip.truck.parking_brake {
            if self.trip.truck.air_ready() && pilot.beat_s == 0.0 {
                self.pilot_press(ctx, pilot, Action::ParkingBrake);
            }
            return Ok(pedals);
        }

        // Their break is due with no stop reached and the player asleep:
        // the 30 minutes are taken on the shoulder, and they drive on.
        // Before the stops are planned: an overdue break also reads as the
        // end of their hours there.
        // With the rest in, `pilot_boundary` hands back for the break instead.
        let making_for_break = matches!(pilot.making_for, Some((_, StopPurpose::Break)));
        if co_driver::break_overdue(hos_of(ctx), &ctx.settings.hos_mode)
            && !making_for_break
            && self.co_driver_shoulder_due(ctx)
        {
            if speed > 0.5 {
                pedals.brake = true;
                return Ok(pedals);
            }
            self.co_driver_shoulder_break(ctx);
            pilot.beat_s = KEY_BEAT_S;
            return Ok(pedals);
        }

        self.pilot_plan_stops(ctx, pilot);
        // The shift is ending with no stop to end it at, or the hours are
        // gone before the stop: the player takes over rather than the
        // co-driver driving past the limit. With the player's rest still
        // owed, the co-driver stops on the shoulder instead and the rest is
        // slept out there (see `shoulder`).
        if pilot.shift_over {
            let out = hos_of(ctx)
                .remaining_min(&ctx.settings.hos_mode)
                .is_some_and(|left| left <= 0.0);
            // Rested, they keep driving until a lot comes into reach.
            if (pilot.making_for.is_none() && !pilot.rested) || out {
                if !self.co_driver_shoulder_due(ctx) {
                    return Err("shift".to_string());
                }
                if speed > 0.5 {
                    pedals.brake = true;
                    return Ok(pedals);
                }
                return Err("shoulder".to_string());
            }
        }

        // An open scale, or the stop they are making for, inside the exit
        // window: signal for it.
        if !self.exit_signal_on && self.ramp_mi.is_none() && pilot.beat_s == 0.0 {
            let window = self.exit_window_mi();
            // A scale the transponder cleared is passed on the highway.
            let scale = (!self.enforcement_bypassed(ctx))
                .then(|| self.check_in_scale_ahead(ctx, window))
                .flatten();
            if let Some((stop, _)) = &scale {
                // Armed for that scale by name: the nearest stop of any
                // kind (a rest area, a closed scale) is not it.
                if self.selected_rest_stop().is_none() {
                    self.exit_stop = Some(stop.clone());
                }
            }
            let scale = scale.is_some();
            let planned = self.selected_rest_stop().is_some_and(|stop| {
                let ahead = stop.at_mi - self.trip.position_mi;
                ahead > 0.0 && ahead <= window
            });
            if scale || planned {
                self.pilot_press(ctx, pilot, Action::TakeExit);
            }
        }

        // A closed lane: one tap toward the open one, as the player would.
        // With traffic beside the truck in the open lane, they drop back
        // behind it and take the gap, the way a driver merges in a taper.
        // They used to wait for a gap that never came and rode the closed
        // lane into the barrels: damage, a fine, and the wheel handed back.
        // Descent control cannot hold the grade: on the brakes, as it asks,
        // down under what it is holding, then off again for the drums to
        // cool. On a downgrade the brake sets descent control's hold for the
        // rest of the grade rather than cancelling the cruise.
        if self.pilot_descent_snub(pilot) {
            pilot.accel_s = 0.0;
            pedals.brake = true;
            return Ok(pedals);
        }

        if let Some(floor) = self.pilot_merge_out_of_closure(ctx, pilot) {
            pilot.accel_s = 0.0;
            pilot.stalled_s = 0.0;
            pedals.brake = speed > floor;
            return Ok(pedals);
        }

        // Speed control does the driving. The co-driver brings the truck up
        // near the posted limit, sets the cruise there, and taps the set
        // speed up when the limit rises.
        let (posted, zone) = self.trip.speed_limit_at(self.trip.position_mi);
        // In weather that takes grip or sight, the co-driver brings the
        // truck up to what the weather allows, not the posted limit, the
        // same cap the cruise holds: pressing on to the limit only had the
        // cruise brake it back down.
        let effects = self.trip.weather.effects();
        let limit = if effects.is_adverse() {
            posted.min(effects.safe_speed_mph)
        } else {
            posted
        };
        let near_limit = speed >= (limit - CRUISE_SET_MARGIN_MPH).max(3.0);
        let armed = self.keeper_mph.is_some() || self.cruise_mph.is_some();
        // A light, sign or yield the assist has taken, or a deceleration
        // lane it is braking down, is left to it: a foot on the
        // accelerator overrides its brakes, and the truck ran the red at
        // the ramp's end.
        // A chain stop the assist is braking for, or a hazard it may brake
        // for, is no time for a foot on the accelerator either: it held the
        // truck off the chain-up stop, and drove it toward the hazard until
        // the automatic brake took over.
        let assist_has_pedals = self.terminal_assist_owns(ctx)
            || self.decel_lane_brake > 0.0
            || self.chain_stop.is_some()
            || self.hazard_deadline.is_some();
        if assist_has_pedals {
            // Waiting at the bar is not a stall.
            pilot.accel_s = 0.0;
            pilot.stalled_s = 0.0;
            pedals.accelerate = self.pilot_closes_up_to_the_line(ctx);
        } else if !armed {
            pilot.accel_s += dt;
            if !near_limit {
                pedals.accelerate = true;
            }
            let ready = near_limit || (speed >= 3.0 && pilot.accel_s >= ACCEL_GIVE_UP_S);
            // Armed and only paused (a stop, a bend), it resumes by itself.
            if ready && !self.speed_control_armed && pilot.beat_s == 0.0 {
                let action = if self.resume_target_mph.is_some() {
                    Action::CruiseResume
                } else {
                    Action::Cruise
                };
                self.pilot_press(ctx, pilot, action);
            }
        } else {
            pilot.accel_s = 0.0;
            let low_set = self
                .cruise_mph
                .is_some_and(|set| set < limit - CRUISE_SET_MARGIN_MPH);
            let open_road = zone.is_none() && self.ramp_mi.is_none() && !self.exit_signal_on;
            if low_set && open_road && pilot.beat_s == 0.0 {
                self.pilot_press(ctx, pilot, Action::CruiseUp);
            }
        }

        if pilot.stalled_s > STALL_LIMIT_S && self.ramp_mi.is_none() {
            return Err("stalled".to_string());
        }
        Ok(pedals)
    }

    /// Whether the co-driver is braking for a grade descent control said it
    /// cannot hold. They used to drive on with the warning unanswered, and
    /// the truck kept gaining.
    fn pilot_descent_snub(&self, pilot: &mut Pilot) -> bool {
        if !self.descent_control_active {
            pilot.descent_snub_to = None;
            return false;
        }
        // The warning waits until the truck is well over and still gaining;
        // a driver who hears the engine running away brakes before that.
        let over = self.trip.truck.speed_mph() - self.descent_hold_mph();
        let getting_away = over > DESCENT_SNUB_OVER_MPH
            && self.trip.truck.net_accel_mph_per_s() > GRADE_HOLDING_MPH_PER_S;
        if pilot.descent_snub_to.is_none() && (self.descent_limit_state == "grade" || getting_away)
        {
            let to = (self.descent_hold_mph() - DESCENT_SNUB_UNDER_MPH).max(CRUISE_MIN_MPH);
            log::info!(target: TRANSCRIPT_TARGET, "[co-driver] brakes for the grade, to {to:.0}");
            pilot.descent_snub_to = Some(to);
        }
        let Some(to) = pilot.descent_snub_to else {
            return false;
        };
        if self.trip.truck.speed_mph() > to {
            return true;
        }
        pilot.descent_snub_to = None;
        false
    }

    /// Short of a ramp's light or sign with the assist holding nothing, the
    /// truck is driven up to it: the assist brakes for the line and rolls a
    /// green, but it never presses the accelerator, and a truck its stop
    /// left a fifth of a mile short of a red sat there through every green
    /// after it, the co-driver waiting on the assist and the assist on them.
    fn pilot_closes_up_to_the_line(&self, ctx: &GameContext) -> bool {
        if !self.terminal_assist_owns(ctx)
            || self.ramp_waiting_at_light
            || self.ramp_waiting_at_sign
            || self.ramp_assist_brake > 0.0
        {
            return false;
        }
        let Some(gap_mi) = self.terminal_gap_mi() else {
            return false;
        };
        if gap_mi <= RAMP_ASSIST_HOLD_MI {
            return false;
        }
        let green = self.ramp_control == "signal" && self.ramp_light_phase() == "green";
        let target = if green {
            GREEN_ROLL_MPH - 5.0
        } else {
            LINE_CREEP_MPH
        };
        self.trip.truck.speed_mph() < target
    }

    /// In a closed lane, or one that closes within [`MERGE_LOOKAHEAD_MI`],
    /// move to the open one: a tap when the gap is there. Returns the speed
    /// to brake down to when traffic in the open lane holds the gap shut
    /// and the truck has to drop back behind it.
    fn pilot_merge_out_of_closure(
        &mut self,
        ctx: &mut GameContext,
        pilot: &mut Pilot,
    ) -> Option<f64> {
        let lane = self.lane.lane;
        let count = self.lane.lane_count;
        let closed_here = self.closed_lane_here() == Some(lane);
        let ahead = self.trip.position_mi + MERGE_LOOKAHEAD_MI;
        let closes_ahead = ahead < self.trip.total_miles()
            && self.trip.closed_lane_at(Some(ahead), Some(count)) == Some(lane);
        if !(closed_here || closes_ahead) || self.ramp_mi.is_some() {
            return None;
        }
        // Still ahead of the taper, a hazard being gone around and an exit
        // being lined up for come first: the move out of the lane waits.
        if !closed_here
            && (self.hazard_deadline.is_some() || self.passing.is_some() || self.exit_signal_on)
        {
            return None;
        }
        if self.lane_change_target.is_some_and(|target| target != lane) {
            return None; // already moving over
        }
        let open = self.open_lane_beside(lane)?;
        let speed = self.trip.truck.speed_mph();
        match self.lane_gap_blocker(open) {
            None => {
                if self.lane_change_target.is_none() && pilot.beat_s == 0.0 {
                    let toward = if open > lane {
                        Action::SteerLeft
                    } else {
                        Action::SteerRight
                    };
                    self.pilot_press(ctx, pilot, toward);
                }
                None
            }
            // Behind the cab and slower: the truck is already pulling clear.
            Some(blocker)
                if blocker.position_mi < self.trip.position_mi - RIG_LENGTH_MI
                    && blocker.speed_mph < speed - MERGE_PULL_CLEAR_MPH =>
            {
                None
            }
            // Beside, ahead, or keeping pace behind: let it go by.
            Some(_) => Some(if closed_here {
                MERGE_BRAKE_FLOOR_MPH
            } else {
                MERGE_EARLY_FLOOR_MPH
            }),
        }
    }

    /// Whether the truck is at the entrance of a stop the co-driver means
    /// to pull into: the scale they signalled for, or their planned stop.
    fn pilot_at_stop_entrance(&self) -> bool {
        let Some(stop) = self.ramp_stop.as_ref() else {
            return false;
        };
        let ramp_done = self.ramp_mi.is_none_or(|left| left <= 0.05);
        ramp_done
            && self
                .trip
                .nearest_stop_within(1.5)
                .is_some_and(|s| s.key() == stop.key())
    }

    /// Choose a stop when the tank is low or the shift is ending, the way T
    /// plans one, without the spoken confirmation.
    fn pilot_plan_stops(&mut self, ctx: &mut GameContext, pilot: &mut Pilot) {
        if self.departure_chain {
            return;
        }
        let mode = ctx.settings.hos_mode.clone();
        if co_driver::shift_over(hos_of(ctx), &mode) {
            if !pilot.shift_over {
                pilot.making_for = None;
            } else if pilot.rested {
                // Their hours ran out on the way to the lot they chose for
                // the player's rest: that lot is still the one to make for,
                // and none other is needed if they reach it.
                pilot
                    .making_for
                    .take_if(|(_, purpose)| *purpose != StopPurpose::ShiftEnd);
            }
            pilot.shift_over = true;
            pilot.rested = false;
        } else if !pilot.shift_over && self.player_rest_complete(ctx) {
            log::info!(target: TRANSCRIPT_TARGET, "[co-driver] the driver's rest is in");
            pilot.shift_over = true;
            pilot.rested = true;
            pilot.making_for = None;
        } else if !pilot.shift_over && self.co_driver_last_lot(ctx).is_some() {
            log::info!(
                target: TRANSCRIPT_TARGET,
                "[co-driver] the next lot is the last their hours reach"
            );
            pilot.shift_over = true;
            pilot.rested = false;
            pilot.last_lot = true;
            pilot.making_for = None;
        }
        // A stop passed without pulling in (missed, or a full lot driven
        // on from) is no longer the one they are making for: plan again.
        if let Some((key, _)) = &pilot.making_for {
            let on_its_ramp = self
                .ramp_stop
                .as_ref()
                .is_some_and(|stop| &stop.key() == key);
            let behind = self
                .trip
                .stops
                .iter()
                .find(|stop| &stop.key() == key)
                .is_none_or(|stop| stop.at_mi < self.trip.position_mi - 0.1);
            if behind && !on_its_ramp {
                pilot.making_for = None;
            }
        }
        if pilot.making_for.is_some() {
            return;
        }
        // Looking for a lot once the player is rested can take a while, so
        // the break and the tank still come first then.
        let purpose = if pilot.shift_over && !pilot.rested {
            StopPurpose::ShiftEnd
        } else if co_driver::break_wanted(hos_of(ctx), &mode) {
            StopPurpose::Break
        } else if self.trip.truck.fuel_fraction() < FUEL_STOP_FRACTION {
            StopPurpose::Fuel
        } else if pilot.rested {
            StopPurpose::ShiftEnd
        } else {
            return;
        };
        let here = self.trip.position_mi;
        let wanted = |stop: &RoadStop| stop_serves(stop, purpose);
        // A planned stop that serves the purpose is kept, whoever planned it:
        // choosing another while signalled for this one left the truck
        // fuelling here and then pulling in again at the second plan.
        let kept = self.selected_rest_stop().filter(|stop| {
            let on_its_ramp = self
                .ramp_stop
                .as_ref()
                .is_some_and(|ramp| ramp.key() == stop.key());
            (on_its_ramp || stop.at_mi > here) && wanted(stop)
        });
        let first = || {
            self.co_driver_stops(purpose, here + self.exit_window_mi() * 0.5)
                .first()
                .map(|stop| (*stop).clone())
        };
        let Some(stop) = kept.or_else(first) else {
            return;
        };
        let reach = if (pilot.rested || pilot.last_lot) && purpose == StopPurpose::ShiftEnd {
            self.co_driver_hours_reach_mi(ctx)
        } else {
            self.co_driver_reach_mi(ctx, purpose)
        };
        if stop.at_mi - here > reach {
            return;
        }
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] makes for {} ({purpose:?})",
            stop.spoken_name()
        );
        self.co_driver_note_plan(ctx, &stop.spoken_name(), stop.at_mi - here, purpose);
        self.trip.planned_stop_key = Some(stop.key());
        self.selected_stop_key = Some(stop.key());
        self.selected_stop_break = false;
        self.selected_stop_purpose = None;
        self.selected_stop_assist_armed = false;
        self.selected_stop_assist_said = false;
        pilot.making_for = Some((stop.key(), purpose));
    }

    /// The stops past `from_mi` that serve `purpose`, nearest first.
    pub(crate) fn co_driver_stops(&self, purpose: StopPurpose, from_mi: f64) -> Vec<&RoadStop> {
        let bobtail = self.trip.bobtail;
        let mut stops: Vec<&RoadStop> = self
            .trip
            .stops
            .iter()
            .filter(|stop| {
                stop.at_mi > from_mi
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
                    && stop.accessible_to(bobtail)
                    && stop_serves(stop, purpose)
            })
            .collect();
        stops.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
        stops
    }

    /// How far ahead a stop for `purpose` may be: the shift's end looks a
    /// short way, the break as far as the time left before it is due.
    pub(crate) fn co_driver_reach_mi(&self, ctx: &GameContext, purpose: StopPurpose) -> f64 {
        match purpose {
            StopPurpose::ShiftEnd => SHIFT_STOP_REACH_MI,
            StopPurpose::Break => {
                let left_min = co_driver::BREAK_REQUIRED_AFTER_MIN
                    - co_driver::CO_DRIVER_SUMMON_LEAD_MIN
                    - hos_of(ctx).since_break_min;
                left_min.max(0.0) / 60.0 * REACH_MPH
            }
            StopPurpose::Fuel => f64::INFINITY,
        }
    }

    /// How far the co-driver's own hours still reach, at the pace a stop's
    /// reach is judged at.
    pub(crate) fn co_driver_hours_reach_mi(&self, ctx: &GameContext) -> f64 {
        // The break is left out: a rested co-driver still plans it, and
        // stops for it on the way.
        let left_min = co_driver::shift_left_min(hos_of(ctx), &ctx.settings.hos_mode)
            .unwrap_or(0.0)
            - co_driver::CO_DRIVER_SUMMON_LEAD_MIN;
        left_min.max(0.0) / 60.0 * REACH_MPH
    }

    /// The lot to end the shift at now: the player's rest will not be in
    /// before the co-driver's hours run out, and the next lot is the last
    /// one the co-driver's hours reach short of the receiver. The truck
    /// parks there and the player sleeps out the rest of their 10 hours
    /// (owner rule, 2026-10-21: the co-driver never wakes the player short
    /// of their rest). Without it the co-driver drove to their last minutes
    /// and handed over on the road, at highway speed with the nearest lot
    /// 76 miles on (player report, 2026-10-20), or with the player's window
    /// still running from before the sleep (player report, 2026-10-21).
    pub fn co_driver_last_lot(&self, ctx: &GameContext) -> Option<&RoadStop> {
        self.team.as_ref()?.wheel.as_ref()?;
        self.last_lot_ahead(
            ctx,
            hos_of(ctx),
            self.player_rest_left_min(ctx),
            self.trip.position_mi,
        )
    }

    /// [`Self::co_driver_last_lot`] for the co-driver's logbook `co` with the
    /// truck at `here`; `rest_left` is the player's rest still owed.
    pub(crate) fn last_lot_ahead(
        &self,
        ctx: &GameContext,
        co: &ff_core::sim::hos::HosClock,
        rest_left: Option<f64>,
        here: f64,
    ) -> Option<&RoadStop> {
        let hours_left = co_driver::shift_left_min(co, &ctx.settings.hos_mode)?
            - co_driver::CO_DRIVER_SUMMON_LEAD_MIN;
        if rest_left.is_none_or(|rest| rest <= hours_left) {
            return None;
        }
        let reach = hours_left.max(0.0) / 60.0 * REACH_MPH;
        if self.trip.total_miles() - here <= reach {
            return None;
        }
        let mut lots = self
            .co_driver_stops(StopPurpose::ShiftEnd, here + self.exit_window_mi() * 0.5)
            .into_iter()
            .filter(|stop| stop.at_mi <= here + reach);
        let first = lots.next()?;
        lots.next().is_none().then_some(first)
    }

    /// Whether the player in the sleeper has had the rest they handed over
    /// for: the co-driver's cue to hand back.
    pub fn player_rest_complete(&self, ctx: &GameContext) -> bool {
        let Some(team) = self.team.as_ref() else {
            return false;
        };
        let owed = team
            .wheel
            .as_ref()
            .is_some_and(|wheel| wheel.player_rest_owed);
        co_driver::rest_complete(&team.off_wheel_hos, &ctx.settings.hos_mode, owed)
    }

    /// Minutes until the player in the sleeper has had the rest they handed
    /// over for. None when they owed none, or hours of service are off.
    pub(crate) fn player_rest_left_min(&self, ctx: &GameContext) -> Option<f64> {
        let team = self.team.as_ref()?;
        let owed = team.wheel.as_ref()?.player_rest_owed;
        if !owed || !co_driver::enforced(&ctx.settings.hos_mode) {
            return None;
        }
        Some(co_driver::rest_left_min(&team.off_wheel_hos))
    }

    /// Something only the driver can decide, or nothing the co-driver can
    /// do: the reason they hand back.
    fn pilot_boundary(&self, ctx: &GameContext, pilot: &Pilot) -> Option<String> {
        let seen = self
            .team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .map_or(self.start_damage, |wheel| wheel.damage_seen);
        if self.trip.truck.damage_pct > seen + 0.5 {
            return Some("damage".to_string());
        }
        if self.trip.truck.fuel_gal <= 0.0 {
            return Some("fuel".to_string());
        }
        if self.off_pavement() {
            return Some("off the road".to_string());
        }
        // Their break is due with no stop reached: the player takes over
        // rather than the co-driver driving past the rule.
        // With the player's rest still owed, the break is taken on the
        // shoulder instead (see `shoulder`).
        let making_for_break = matches!(pilot.making_for, Some((_, StopPurpose::Break)));
        if co_driver::break_overdue(hos_of(ctx), &ctx.settings.hos_mode)
            && !making_for_break
            && !self.co_driver_shoulder_due(ctx)
        {
            return Some("break".to_string());
        }
        None
    }
}
