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
const FUEL_STOP_FRACTION: f64 = 0.35;
/// A key the co-driver pressed gets this long to take effect before they
/// press another.
const KEY_BEAT_S: f64 = 1.5;
/// Stopped this long on the open road with nothing to wait for, the
/// co-driver hands back.
const STALL_LIMIT_S: f64 = 180.0;
/// How far ahead to look for a stop to end the shift at.
const SHIFT_STOP_REACH_MI: f64 = 40.0;
/// How close to the posted limit the co-driver brings the truck by hand
/// before setting the cruise, and how far under it a set speed may sit
/// before they tap it up.
const CRUISE_SET_MARGIN_MPH: f64 = 3.0;
/// On the accelerator this long without reaching the limit (a grade, a
/// slow truck ahead), they set the cruise where they are.
const ACCEL_GIVE_UP_S: f64 = 45.0;

/// Why the co-driver is making for a stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopPurpose {
    Fuel,
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
    /// Seconds spent on the accelerator with speed control off.
    pub accel_s: f64,
}

impl Pilot {
    pub fn new() -> Self {
        Self::default()
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
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.pedals = pedals;
            wheel.pilot = pilot;
        }
        self.update_co_driver_radio(ctx);
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
        if let Some(reason) = self.pilot_boundary(ctx) {
            return Err(reason);
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

        self.pilot_plan_stops(ctx, pilot);

        // An open scale, or the stop they are making for, inside the exit
        // window: signal for it.
        if !self.exit_signal_on && self.ramp_mi.is_none() && pilot.beat_s == 0.0 {
            let window = self.exit_window_mi();
            let scale = !self.enforcement_bypassed(ctx) && self.open_scale_ahead(window).is_some();
            let planned = self.selected_rest_stop().is_some_and(|stop| {
                let ahead = stop.at_mi - self.trip.position_mi;
                ahead > 0.0 && ahead <= window
            });
            if scale || planned {
                self.pilot_press(ctx, pilot, Action::TakeExit);
            }
        }

        // A closed lane: one tap toward the open one, as the player would.
        if let Some(closed) = self.closed_lane_here() {
            if closed == self.lane.lane && self.lane_change_target.is_none() && pilot.beat_s == 0.0
            {
                if let Some(open) = self.open_lane_beside(closed) {
                    let toward = if open > closed {
                        Action::SteerLeft
                    } else {
                        Action::SteerRight
                    };
                    self.pilot_press(ctx, pilot, toward);
                }
            }
        }

        // Speed control does the driving. The co-driver brings the truck up
        // near the posted limit, sets the cruise there, and taps the set
        // speed up when the limit rises.
        let (limit, zone) = self.trip.speed_limit_at(self.trip.position_mi);
        let near_limit = speed >= (limit - CRUISE_SET_MARGIN_MPH).max(3.0);
        let armed = self.keeper_mph.is_some() || self.cruise_mph.is_some();
        if !armed {
            pilot.accel_s += dt;
            if !near_limit {
                pedals.accelerate = true;
            }
            let ready = near_limit || (speed >= 3.0 && pilot.accel_s >= ACCEL_GIVE_UP_S);
            if ready && pilot.beat_s == 0.0 {
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
        if !pilot.shift_over && co_driver::shift_over(hos_of(ctx), &mode) {
            pilot.shift_over = true;
            pilot.making_for = None;
        }
        if pilot.making_for.is_some() {
            return;
        }
        let purpose = if pilot.shift_over {
            StopPurpose::ShiftEnd
        } else if self.trip.truck.fuel_fraction() < FUEL_STOP_FRACTION {
            StopPurpose::Fuel
        } else {
            return;
        };
        let here = self.trip.position_mi;
        let bobtail = self.trip.bobtail;
        let wanted = |stop: &RoadStop| match purpose {
            StopPurpose::Fuel => stop.actions.iter().any(|a| a == "fuel"),
            StopPurpose::ShiftEnd => stop.parking != "none",
        };
        let mut candidates: Vec<&RoadStop> = self
            .trip
            .stops
            .iter()
            .filter(|stop| {
                stop.at_mi > here + self.exit_window_mi() * 0.5
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
                    && stop.accessible_to(bobtail)
                    && wanted(stop)
            })
            .collect();
        candidates.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
        let Some(stop) = candidates.first().map(|stop| (*stop).clone()) else {
            return;
        };
        if purpose == StopPurpose::ShiftEnd && stop.at_mi - here > SHIFT_STOP_REACH_MI {
            return;
        }
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] makes for {} ({purpose:?})",
            stop.spoken_name()
        );
        self.trip.planned_stop_key = Some(stop.key());
        self.selected_stop_key = Some(stop.key());
        self.selected_stop_break = false;
        self.selected_stop_assist_armed = false;
        self.selected_stop_assist_said = false;
        pilot.making_for = Some((stop.key(), purpose));
    }

    /// Something only the driver can decide, or nothing the co-driver can
    /// do: the reason they hand back.
    fn pilot_boundary(&self, ctx: &GameContext) -> Option<String> {
        if self.trip.truck.damage_pct > self.start_damage + 0.5 {
            return Some("damage".to_string());
        }
        if self.trip.truck.fuel_gal <= 0.0 {
            return Some("fuel".to_string());
        }
        if self.off_pavement() {
            return Some("off the road".to_string());
        }
        let _ = ctx;
        None
    }
}
