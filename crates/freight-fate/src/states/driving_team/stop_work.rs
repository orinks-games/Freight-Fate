//! What the co-driver does at a stop, without the stop's menu (owner,
//! 2026-10-18 and 2026-10-19: "skip the menus entirely, it is an
//! artificial driver"): a scale's check-in, fuel and their 30-minute break
//! happen as a sound and a note, then they drive on. Only the end of their
//! shift opens the menu, after they hand back, because it is the player's;
//! a shift that ends before the player's rest is in stays parked until it is.

use ff_core::models::co_driver;
use ff_core::models::loyalty::loyalty_earnings_text;
use ff_core::sim::hos;
use ff_core::sim::roadside_inspection::InspectionLevel;
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, TRANSCRIPT_TARGET};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::notes::NoteWeight;
use super::overlay::TOP_UP_FRACTION;
use super::pilot::{StopPurpose, FUEL_STOP_FRACTION};

impl DrivingState {
    /// Fill the tank at `stop` the way the pump does. Returns the gallons.
    pub(crate) fn co_driver_refuel(&mut self, ctx: &mut GameContext, stop: &RoadStop) -> f64 {
        let truck = &self.trip.truck;
        let mut need = truck.specs.fuel_tank_gal - truck.fuel_gal;
        if need < 1.0 {
            return 0.0;
        }
        let region = self.trip.current_region().to_string();
        if player_pays_operating_costs(&profile_of(ctx).business_status) {
            let mut cost = ctx.economy.fuel_cost(&region, need) + 35.0;
            let money = profile_of(ctx).money();
            if money < cost {
                need = ((money - 35.0) / ctx.economy.fuel_price(&region)).max(0.0);
                if need < 5.0 {
                    log::info!(
                        target: TRANSCRIPT_TARGET,
                        "[co-driver] cannot afford fuel at {}",
                        stop.spoken_name()
                    );
                    return 0.0;
                }
                cost = ctx.economy.fuel_cost(&region, need) + 35.0;
            }
            profile_mut_of(ctx).spend(cost);
        }
        self.trip.truck.refuel(Some(need));
        self.stop_visit(stop).fueled = true;
        advance_rest_clock(self, ctx, FUEL_STOP_MIN, None, "");
        hos_mut_of(ctx).on_duty(FUEL_STOP_MIN);
        ctx.audio.play("vehicle/fuel_pump");
        let result = profile_mut_of(ctx)
            .loyalty
            .add_fueling(need, None, &stop.name, &region);
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] fuels {need:.0} gallons at {}. {}",
            stop.spoken_name(),
            loyalty_earnings_text(need, result.points_earned, &result.rewards)
        );
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.log.fuel_stops.push(stop.spoken_name());
        }
        need
    }

    /// The co-driver's 30-minute break at `stop`.
    pub(crate) fn co_driver_break(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        advance_rest_clock(self, ctx, 30.0, None, "");
        hos_mut_of(ctx).take_break(30.0);
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] takes a 30-minute break at {}",
            stop.spoken_name()
        );
    }

    /// The co-driver pulled into `stop`: a scale's check-in, fuel and the
    /// break are done here with no menu, the stop goes in their notes, and
    /// they drive on. False when the stop's menu should open instead: the
    /// end of their shift, where they have just handed back and the menu is
    /// the player's.
    pub fn co_driver_works_stop(&mut self, ctx: &mut GameContext, stop: &RoadStop) -> bool {
        if stop.stop_type == "weigh_station" {
            // Even at the end of their shift: nobody parks at a scale, so
            // they check in and hand back at the next lot.
            self.co_driver_works_scale(ctx, stop);
            return true;
        }
        let Some(team) = self.team.as_ref() else {
            return false;
        };
        let name = team.co_driver.name;
        let Some(wheel) = team.wheel.as_ref() else {
            return false;
        };
        let shift_over = wheel.pilot.shift_over && !wheel.pilot.rested;
        let rested = wheel.pilot.rested || self.player_rest_complete(ctx);
        let can_park = stop.parking != "none" && !self.lot_full_now(stop);
        let rest_left = self.player_rest_left_min(ctx).filter(|left| *left > 0.0);
        if let Some(left) = rest_left.filter(|_| shift_over && can_park) {
            self.co_driver_parks_for_rest(ctx, stop, left);
            self.co_driver_summon(ctx, "slept on");
            return false;
        }
        if shift_over || (rested && can_park) {
            // They hand back, then the stop's menu opens for the player.
            let reason = if shift_over {
                wheel.pilot.shift_end_reason()
            } else {
                "rested"
            };
            self.co_driver_summon(ctx, reason);
            return false;
        }
        let making_for = wheel.pilot.making_for.clone();
        let purpose = making_for.as_ref().map(|(_, purpose)| *purpose);
        advance_rest_clock(self, ctx, STOP_PULL_IN_MIN, None, "");
        hos_mut_of(ctx).on_duty(STOP_PULL_IN_MIN);
        let sells_fuel = stop.actions.iter().any(|action| action == "fuel");
        let fuel = self.trip.truck.fuel_fraction();
        let wants_fuel =
            fuel < TOP_UP_FRACTION || (purpose == Some(StopPurpose::Fuel) && fuel < 0.98);
        let gallons = if sells_fuel && wants_fuel {
            self.co_driver_refuel(ctx, stop)
        } else {
            0.0
        };
        if sells_fuel && gallons <= 0.0 && fuel < FUEL_STOP_FRACTION {
            // The money will not cover the tank, and driving on is driving
            // it dry: the player decides, with the stop's menu in front of
            // them.
            self.co_driver_summon(ctx, "money");
            return false;
        }
        let breaks_here = stop.actions.iter().any(|action| action == "break");
        let took_break = purpose == Some(StopPurpose::Break) && breaks_here;
        if took_break {
            self.co_driver_break(ctx, stop);
        }
        let place = stop.spoken_name();
        let line = match (gallons > 0.0, took_break) {
            (true, true) => format!(
                "{name} fueled {} gallons and took a 30-minute break at {place}.",
                gallons.round()
            ),
            (true, false) => format!("{name} fueled {} gallons at {place}.", gallons.round()),
            (false, true) => format!("{name} took a 30-minute break at {place}."),
            (false, false) => format!("{name} stopped at {place}."),
        };
        self.co_driver_note(ctx, line, NoteWeight::Passing);
        self.co_driver_clear_plan_at(stop, making_for);
        true
    }

    /// The co-driver's hours ran out at `stop` with the player's rest not
    /// yet in: the truck stays parked while the player sleeps out the
    /// `left` minutes of their 10 hours, and the co-driver rests too.
    /// Waking the player there handed them a shift whose window had been
    /// running since before the sleep (player report, 2026-10-21; owner
    /// rule: the co-driver never wakes the player short of their rest).
    fn co_driver_parks_for_rest(&mut self, ctx: &mut GameContext, stop: &RoadStop, left: f64) {
        shut_down_engine(self, ctx);
        advance_rest_clock(self, ctx, left, Some("sleeper_berth"), "");
        hos_mut_of(ctx).sleeper(left);
        if let Some(team) = self.team.as_mut() {
            // The sleeper minutes above complete the ten hours; this only
            // guards the last fraction of a minute against rounding.
            if !co_driver::fully_rested(&team.off_wheel_hos) {
                team.off_wheel_hos.sleep();
            }
        }
        let profile = profile_mut_of(ctx);
        profile.fatigue = hos::rest_sleep(profile.fatigue);
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] parks at {} and the driver sleeps {left:.0} more minutes",
            stop.spoken_name()
        );
    }

    /// The stop is done with: none of the plan is left to pull in again.
    fn co_driver_clear_plan_at(
        &mut self,
        stop: &RoadStop,
        making_for: Option<(String, StopPurpose)>,
    ) {
        // The stop is done with: none of the plan is left to pull in again.
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.pilot.making_for = None;
        }
        let key = stop.key();
        let ours = |planned: &String| {
            *planned == key || making_for.as_ref().is_some_and(|(plan, _)| plan == planned)
        };
        if self.trip.planned_stop_key.as_ref().is_some_and(ours) {
            self.trip.planned_stop_key = None;
        }
        if self.selected_stop_key.as_ref().is_some_and(ours) {
            self.clear_selected_stop_intent();
        }
    }

    /// The co-driver at an open scale: the check-in the menu's own row runs,
    /// settled where the truck stands, and one note of how it went.
    fn co_driver_works_scale(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        let Some(name) = self.co_driver_at_wheel_name() else {
            return;
        };
        let place = stop.spoken_name();
        if !self.scale_is_open(stop) || self.stop_visit(stop).inspected {
            return;
        }
        self.stop_visit(stop).inspected = true;
        let (line, weight) = if self.decal_waves_through(ctx) {
            advance_rest_clock(self, ctx, WAVE_THROUGH_MIN, None, "");
            hos_mut_of(ctx).on_duty(WAVE_THROUGH_MIN);
            record_inspection(ctx);
            (
                format!("{name} checked in at {place}. The inspection decal got the truck waved through."),
                NoteWeight::Passing,
            )
        } else if !self.scale_selects_driver(ctx, stop) {
            advance_rest_clock(self, ctx, WAVE_THROUGH_MIN, None, "");
            hos_mut_of(ctx).on_duty(WAVE_THROUGH_MIN);
            record_inspection(ctx);
            (
                format!("{name} checked in at {place} and was waved through."),
                NoteWeight::Passing,
            )
        } else {
            let report = self.inspection_report(ctx, InspectionLevel::Full);
            let outcome = self.settle_inspection(ctx, &report);
            (
                format!("{name} was pulled in for an inspection at {place}. {outcome}"),
                NoteWeight::Wake,
            )
        };
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.log.scales.push(place.clone());
        }
        // The fine, the citation and the done check-in are on disk before a
        // quit could take them back, as the menu's own check-in saves them.
        let snapshot = self.snapshot(ctx);
        profile_mut_of(ctx).active_trip = Some(snapshot);
        ctx.save_profile();
        self.note_scale_reannounce(Some(&place));
        self.co_driver_note(ctx, line, weight);
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.pilot.making_for = None;
        }
    }
}
