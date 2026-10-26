//! A slept-through shift, simulated (owner design, 2026-10-18): the player
//! should not wait while the co-driver drives, so the shift passes as time
//! instead of being played.
//!
//! The open road runs on the cruise model (the posted limit, the highway
//! fuel burn, the co-driver's logbook). The stops the shift needs are worked
//! out where they fall: fuel and the 30-minute break cost their minutes and
//! money without the truck leaving the road, and the shift's end parks the
//! truck at the stop with its menu open for the player. The origin's streets
//! pass the same way. What is still driven, sped up with the sound down, is
//! the merge onto the highway and the receiver's exit, streets and gate.

use ff_core::models::co_driver;
use ff_core::sim::hos::{self, HosClock};
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, TRANSCRIPT_TARGET};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::overlay::TOP_UP_FRACTION;
use super::pilot::{StopPurpose, FUEL_STOP_FRACTION};
use super::TeamMode;

/// The walk's step along the route.
const STEP_MI: f64 = 0.25;
/// A jump shorter than this is left to the live drive.
const MIN_JUMP_MI: f64 = 2.0;
/// Where a jump ends ahead of the receiver's exit, beyond the exit window
/// it is signalled in.
const STOP_LEAD_MI: f64 = 1.0;
/// Where a jump ends ahead of the receiver, beyond the gate's warning.
const GATE_LEAD_MI: f64 = 3.0;
/// The share of the posted limit the cruise holds over a long stretch.
const CRUISE_SHARE: f64 = 0.96;
/// The slowest the cruise model drives a stretch.
const FLOOR_MPH: f64 = 25.0;
/// The pace of the origin's streets, lights and corners included.
const STREET_MPH: f64 = 18.0;
/// The pace the co-driver's last minutes of driving are judged at when
/// they look for a lot to end the shift in.
const SHIFT_END_MPH: f64 = 55.0;
/// Stops served in one go, at most: a guard, far above any real shift.
const MAX_STOPS: usize = 40;

/// What a jump covered.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Jump {
    pub miles: f64,
    pub minutes: f64,
}

/// A slept-through stretch ends before the tank goes under this share.
const SLEEP_FUEL_RESERVE_FRACTION: f64 = 0.05;

impl DrivingState {
    /// The player is asleep and the co-driver has the wheel, with nothing
    /// handed back yet.
    fn sleeping_through(&self, ctx: &GameContext) -> bool {
        ctx.co_driver_fast_forward
            && self
                .team
                .as_ref()
                .and_then(|team| team.wheel.as_ref())
                .is_some_and(|wheel| wheel.mode == TeamMode::SleepThrough && wheel.summon.is_none())
    }

    /// Whether the truck is on the open road at speed with nothing the
    /// co-driver is in the middle of: the only place a jump may start.
    fn sleep_jump_ready(&self, ctx: &GameContext) -> bool {
        self.sleeping_through(ctx)
            && self.phase == DRIVE_PHASE_DELIVERY
            && !self.trip.finished
            && !self.trip.on_ramp
            && self.ramp_mi.is_none()
            && self.ramp_stop.is_none()
            && self.departure_ramp_mi.is_none()
            && !self.exit_signal_on
            && self.pull_over.is_none()
            && self.hazard_deadline.is_none()
            && !self.chain_checkpoint_hold
            && !self.departure_chain
            && !self.trip.truck.parking_brake
            && self.decel_lane_brake <= 0.0
            && self.trip.truck.speed_mph() >= 40.0
            && (self.keeper_mph.is_some() || self.cruise_mph.is_some())
    }

    /// The mile the shift may pass to: short of the receiver's exit and of
    /// a chain-law area. None when nothing allows a jump.
    fn sleep_jump_limit(&mut self, ctx: &mut GameContext) -> Option<f64> {
        // Off the highway for the receiver: the streets are driven live.
        if self.destination_exit_taken {
            return None;
        }
        let here = self.trip.position_mi;
        let window = self.exit_window_mi();
        let mut limit = self.trip.total_miles() - self.gate_warning_window_mi() - GATE_LEAD_MI;
        // No receiver's exit ahead means the truck is at it, or past it
        // and looping back: driven live.
        let exit = self.destination_exit_stop(ctx)?;
        limit = limit.min(exit.at_mi - window - STOP_LEAD_MI);
        // An open scale that wants the truck in is driven live, so the
        // co-driver checks in there as they would awake; one the
        // transponder clears is passed in the jump.
        if !self.enforcement_bypassed(ctx) {
            if let Some((scale, _)) = self.check_in_scale_ahead(ctx, limit - here + window) {
                limit = limit.min(scale.at_mi - window - STOP_LEAD_MI);
            }
        }
        // A chain-law area is the checkpoint's to judge, live.
        let mut mile = here;
        while mile < limit {
            if self.trip.chain_law_area_at(mile).is_some() {
                limit = mile - window;
                break;
            }
            mile += STEP_MI;
        }
        (limit - here >= MIN_JUMP_MI).then_some(limit)
    }

    /// What the co-driver needs to stop for now, the way they plan it.
    fn sleep_need(&self, ctx: &GameContext) -> Option<StopPurpose> {
        let mode = &ctx.settings.hos_mode;
        if co_driver::shift_over(hos_of(ctx), mode) || self.player_rest_complete(ctx) {
            Some(StopPurpose::ShiftEnd)
        } else if co_driver::break_wanted(hos_of(ctx), mode) {
            Some(StopPurpose::Break)
        } else if self.trip.truck.fuel_fraction() < FUEL_STOP_FRACTION {
            Some(StopPurpose::Fuel)
        } else {
            None
        }
    }

    /// Walk the cruise model from the truck to `limit`. With `stop_at_need`,
    /// the walk ends where a fuel stop, the break or the shift's end first
    /// becomes due; a need already due at the start does not stop it.
    /// Changes nothing.
    fn sleep_jump_walk(&mut self, ctx: &GameContext, limit: f64, stop_at_need: bool) -> Jump {
        let mode = ctx.settings.hos_mode.clone();
        let mut hos: HosClock = hos_of(ctx).clone();
        let truck = &self.trip.truck;
        let gal_per_mi = truck.cruise_gal_per_mile();
        let tank = truck.specs.fuel_tank_gal.max(1.0);
        let mut fuel = truck.fuel_gal;
        let fuel_due_at_start = fuel / tank < FUEL_STOP_FRACTION;
        let break_due_at_start = co_driver::break_wanted(&hos, &mode);
        let shift_due_at_start =
            co_driver::shift_over(&hos, &mode) || self.player_rest_complete(ctx);
        // The player's rest comes in this many minutes into the walk.
        let rest_in_min = self
            .player_rest_left_min(ctx)
            .filter(|_| !shift_due_at_start);
        // The last lot the hours reach, when the player's rest would not be
        // in before the co-driver's hours run out.
        let rest_left = self.player_rest_left_min(ctx);
        let last_lot_due_at_start = self.co_driver_last_lot(ctx).is_some();
        let start = self.trip.position_mi;
        let mut mile = start;
        let mut minutes = 0.0;
        while mile < limit {
            // Whatever the walk was for, it never carries the truck onto
            // its last few gallons: the rest is driven live, where a fuel
            // stop is planned or the player is woken for it. Judged before
            // the step, so a tank already in the reserve jumps nowhere.
            if fuel / tank < SLEEP_FUEL_RESERVE_FRACTION {
                break;
            }
            let step = STEP_MI.min(limit - mile);
            let (posted, _) = self.trip.speed_limit_at(mile);
            let mph = (posted * CRUISE_SHARE).max(FLOOR_MPH);
            let step_min = step / mph * 60.0;
            hos.drive(step_min);
            fuel -= step * gal_per_mi;
            mile += step;
            minutes += step_min;
            let due = (!fuel_due_at_start && fuel / tank < FUEL_STOP_FRACTION)
                || (!break_due_at_start && co_driver::break_wanted(&hos, &mode))
                || (!shift_due_at_start && co_driver::shift_over(&hos, &mode))
                || rest_in_min.is_some_and(|rest| minutes >= rest)
                || (!last_lot_due_at_start
                    && self
                        .last_lot_ahead(ctx, &hos, rest_left.map(|rest| rest - minutes), mile)
                        .is_some());
            if stop_at_need && due {
                break;
            }
        }
        Jump {
            miles: mile - start,
            minutes,
        }
    }

    /// The clocks and the truck over a stretch the co-driver drove while
    /// the player slept.
    fn sleep_drive_clocks(&mut self, ctx: &mut GameContext, jump: Jump) {
        self.trip.game_minutes += jump.minutes;
        self.trip.weather.update(jump.minutes);
        hos_mut_of(ctx).drive(jump.minutes);
        self.trip.last_moved_mi = jump.miles;
        self.update_team_hours(ctx, jump.minutes);
        self.trip.last_moved_mi = 0.0;
        self.trip.truck.odometer_mi += jump.miles;
        self.trip.truck.burn_cruise_fuel_over_miles(jump.miles);
    }

    /// Move the truck along the highway by `jump`.
    fn sleep_apply_jump(&mut self, ctx: &mut GameContext, jump: Jump) {
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] drives {:.1} miles in {:.0} minutes while you sleep",
            jump.miles,
            jump.minutes
        );
        let to = self.trip.position_mi + jump.miles;
        self.sleep_drive_clocks(ctx, jump);
        // The road: tolls passed are charged, everything else behind the
        // truck is history, and the traffic is the traffic here.
        self.trip.position_mi = to;
        // The posts passed are behind the truck, not just crossed: no pass
        // cue, and no stretch over the limit read out of the jump.
        self.settle_enforcement_at(to);
        self.trip.check_tolls();
        let minutes = self.trip.game_minutes;
        self.trip.restore(to, minutes);
        self.trip.set_npc_vehicles(Vec::new());
    }

    /// The origin's streets, passed at once: the truck comes out at the
    /// on-ramp and merges live.
    fn sleep_past_departure_streets(&mut self, ctx: &mut GameContext) -> bool {
        let started = self.trip.truck.speed_mph() >= 1.0 && !self.trip.truck.parking_brake;
        if !self.departure_chain || !started || !self.sleeping_through(ctx) {
            return false;
        }
        let miles = (self.trip.total_miles() - self.trip.position_mi).max(0.0);
        let jump = Jump {
            miles,
            minutes: miles / STREET_MPH * 60.0,
        };
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] drives the {:.1} miles of streets to the highway while you sleep",
            miles
        );
        self.sleep_drive_clocks(ctx, jump);
        self.trip.position_mi = self.trip.total_miles();
        self.finish_departure_chain(ctx);
        true
    }

    /// The co-driver's plan is served or overtaken: none of it is left to
    /// steer the live drive.
    fn sleep_clear_plan(&mut self) {
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.pilot.making_for = None;
        }
        self.trip.planned_stop_key = None;
        if self.selected_stop_key.is_some() {
            self.clear_selected_stop_intent();
        }
    }

    /// The shift ends at `stop`: the truck is parked there with the stop's
    /// menu open, and the co-driver hands back from it.
    /// `last_lot`: the last lot the co-driver's hours reach, with the
    /// player's rest still to come.
    fn sleep_park_for_shift_end(&mut self, ctx: &mut GameContext, stop: &RoadStop, last_lot: bool) {
        let hours_up = co_driver::shift_over(hos_of(ctx), &ctx.settings.hos_mode);
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.pilot.shift_over = true;
            wheel.pilot.rested = !hours_up && !last_lot;
            wheel.pilot.last_lot = last_lot;
        }
        self.trip.position_mi = stop.at_mi;
        self.settle_enforcement_at(stop.at_mi);
        self.lane.recentre(0);
        self.lane_change_target = None;
        self.reset_exit_lane_state();
        self.exit_signal_on = false;
        self.exit_stop = None;
        self.trip.truck.velocity_mps = 0.0;
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] parks at {} for the end of the shift",
            stop.spoken_name()
        );
        self.open_poi_stop(ctx, stop, false, None);
    }

    /// Whether the lot at `stop` is full when the truck gets there.
    pub(crate) fn lot_full_now(&self, stop: &RoadStop) -> bool {
        stop.actions.iter().any(|action| action == "sleep")
            && hos::parking_is_full(
                self.trip_seed,
                stop.at_mi,
                self.trip.local_hour(),
                stop.parking_spaces,
            )
    }

    /// The stop the co-driver makes for, short of `limit`. The shift's end
    /// is wherever their hours reach: the first lot with room, or the last
    /// stop before the hours run out when every lot is full.
    fn sleep_stop_for(
        &self,
        ctx: &GameContext,
        purpose: StopPurpose,
        limit: f64,
    ) -> Option<RoadStop> {
        let here = self.trip.position_mi;
        let reach = match purpose {
            StopPurpose::ShiftEnd => {
                let left_min = hos_of(ctx)
                    .remaining_min(&ctx.settings.hos_mode)
                    .unwrap_or(0.0);
                left_min.max(0.0) / 60.0 * SHIFT_END_MPH
            }
            _ => self.co_driver_reach_mi(ctx, purpose),
        };
        let stops: Vec<&RoadStop> = self
            .co_driver_stops(purpose, here)
            .into_iter()
            .filter(|stop| stop.at_mi <= limit && stop.at_mi <= here + reach)
            .collect();
        if purpose != StopPurpose::ShiftEnd {
            return stops.first().map(|stop| (*stop).clone());
        }
        stops
            .iter()
            .find(|stop| !self.lot_full_now(stop))
            .or(stops.last())
            .map(|stop| (*stop).clone())
    }

    /// Pass the shift as time while the player sleeps, up to the next
    /// thing that is still driven: the merge, the receiver's exit, a
    /// chain-law area, or the shift's end parked at a stop. Returns the
    /// road covered, if any.
    pub fn sleep_ahead(&mut self, ctx: &mut GameContext) -> Option<Jump> {
        let mut total = Jump::default();
        if self.sleep_past_departure_streets(ctx) {
            return None; // the merge comes first
        }
        for _ in 0..MAX_STOPS {
            if !self.sleep_jump_ready(ctx) {
                break;
            }
            let Some(limit) = self.sleep_jump_limit(ctx) else {
                break;
            };
            let need = self.sleep_need(ctx);
            // The break and fuel come first; otherwise the last lot the
            // hours reach ends the shift.
            let last_lot = matches!(need, None | Some(StopPurpose::ShiftEnd))
                .then(|| self.co_driver_last_lot(ctx).cloned())
                .flatten()
                .filter(|lot| lot.at_mi <= limit);
            let to_last_lot = last_lot.is_some();
            let stop = match last_lot {
                Some(lot) => Some((StopPurpose::ShiftEnd, lot)),
                None => need.and_then(|purpose| {
                    self.sleep_stop_for(ctx, purpose, limit)
                        .map(|stop| (purpose, stop))
                }),
            };
            let Some((purpose, stop)) = stop else {
                // No stop for a break or the shift's end in reach: the live
                // drive hands back for it.
                if matches!(need, Some(StopPurpose::Break | StopPurpose::ShiftEnd)) {
                    break;
                }
                let jump = self.sleep_jump_walk(ctx, limit, true);
                if jump.miles < MIN_JUMP_MI {
                    break;
                }
                self.sleep_apply_jump(ctx, jump);
                total.miles += jump.miles;
                total.minutes += jump.minutes;
                continue;
            };
            // Another need coming due on the way is taken first.
            let jump = self.sleep_jump_walk(ctx, stop.at_mi, true);
            // Short of the stop with nowhere left to jump (the fuel reserve):
            // the live drive takes it from here.
            if jump.miles < MIN_JUMP_MI && self.trip.position_mi + jump.miles < stop.at_mi - 0.01 {
                break;
            }
            self.sleep_apply_jump(ctx, jump);
            total.miles += jump.miles;
            total.minutes += jump.minutes;
            if self.trip.position_mi < stop.at_mi - 0.01 {
                continue;
            }
            self.sleep_clear_plan();
            if purpose == StopPurpose::ShiftEnd {
                self.sleep_park_for_shift_end(ctx, &stop, to_last_lot);
                break;
            }
            let sells_fuel = stop.actions.iter().any(|action| action == "fuel");
            if sells_fuel && self.trip.truck.fuel_fraction() < TOP_UP_FRACTION {
                self.co_driver_refuel(ctx, &stop);
            }
            let breaks_here = stop.actions.iter().any(|action| action == "break");
            if purpose == StopPurpose::Break && breaks_here {
                self.co_driver_break(ctx, &stop);
            }
        }
        (total.miles > 0.0).then_some(total)
    }
}
