//! How the co-driver's shift ends, fed to the shift machine
//! (`ff_core::sim::team_shift`) once a frame.
//!
//! With hours of service enforced the co-driver's clock never goes into
//! violation while they hold the wheel. Three rules make that so:
//!
//! - **The lot is in their hours.** A shift their hours end finishes at the
//!   last lot those hours reach, chosen by [`DrivingState::hours_end_lot`]
//!   for the live drive and the slept-through one alike: only a lot the time
//!   left, less [`co_driver::CO_DRIVER_SUMMON_LEAD_MIN`], covers at
//!   [`DrivingState::co_driver_pace_mph`].
//! - **The lot stays in their hours.** The lot chosen is judged again every
//!   frame against the time left; one that falls out of reach is dropped.
//! - **A backstop.** With [`CO_DRIVER_STOP_MARGIN_MIN`] still on the clock
//!   and the truck not on its lot's ramp, they brake to a stop on the
//!   shoulder.
//!
//! Before this a tester's co-driver took a hand-over with 85 minutes left,
//! chose a lot by a flat 40 miles that never looked at the hours, ran out
//! five miles short, and a trooper's log check fined the player 400 dollars
//! and put the load out of service for ten hours (tester report,
//! 2026-11-19).

use ff_core::models::co_driver;
use ff_core::sim::hos::HosClock;
use ff_core::sim::team_shift::{TeamShift, TeamShiftEvent};
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, TRANSCRIPT_TARGET};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::pilot::{Pilot, StopPurpose};

/// The backstop: with this many minutes of the co-driver's hours left and
/// the truck not on its lot's ramp, they brake to a stop on the shoulder.
///
/// From the sim: a loaded truck braking from 75 miles per hour stops in
/// about ten seconds of driving, and at the Standard pacing (20 times) that
/// is 2.3 minutes on the clock, measured by
/// `the_shoulder_stop_from_highway_speed_fits_inside_the_backstop`. Signalling
/// and the lane change before it take no clock of their own. Twice the stop,
/// rounded up to a whole minute, leaves the same again for a stop begun a
/// frame late or on a downgrade.
pub const CO_DRIVER_STOP_MARGIN_MIN: f64 = 5.0;

impl DrivingState {
    /// How far the co-driver's own hours still reach, at the pace a stop's
    /// reach is judged at: the time left less the summon lead. Unbounded
    /// with hours off.
    pub(crate) fn co_driver_hours_reach_mi(&self, ctx: &GameContext) -> f64 {
        // The break is left out: a co-driver short of it still plans it, and
        // stops for it on the way.
        co_driver::usable_min(hos_of(ctx), &ctx.settings.hos_mode).map_or(f64::INFINITY, |left| {
            left / 60.0 * self.co_driver_pace_mph()
        })
    }

    /// The lot that ends the shift now, when the co-driver's hours end it:
    /// the next lot is the last one their hours reach short of the receiver.
    /// The player owed no rest, or their rest will not be in before those
    /// hours run out (where it would, the rest ends the shift instead).
    /// Without it the co-driver drove to their last minutes and handed over
    /// on the road, at highway speed with the nearest lot 76 miles on
    /// (player report, 2026-10-20), or chose a lot their hours did not reach
    /// (tester report, 2026-11-19).
    pub fn co_driver_last_lot(&self, ctx: &GameContext) -> Option<&RoadStop> {
        self.team.as_ref()?.wheel.as_ref()?;
        self.hours_end_lot(
            ctx,
            hos_of(ctx),
            self.player_rest_left_min(ctx),
            self.trip.position_mi,
        )
    }

    /// [`Self::co_driver_last_lot`] for the co-driver's logbook `co` with the
    /// truck at `here`; `rest_left` is the player's rest still owed. The one
    /// place a shift the hours end chooses its lot: the live drive and the
    /// slept-through one both ask here.
    pub(crate) fn hours_end_lot(
        &self,
        ctx: &GameContext,
        co: &HosClock,
        rest_left: Option<f64>,
        here: f64,
    ) -> Option<&RoadStop> {
        let hours_left = co_driver::usable_min(co, &ctx.settings.hos_mode)?;
        if rest_left.is_some_and(|rest| rest <= hours_left) {
            return None;
        }
        let reach = hours_left / 60.0 * self.co_driver_pace_mph();
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

    /// Whether the player's own logbook, as it stands now, carries the rest
    /// of the trip to the receiver.
    pub(crate) fn player_finishes_load(&self, ctx: &GameContext) -> bool {
        let drive_min_left = ff_core::models::jobs::route_drive_hours(
            Some(&self.trip.route),
            self.trip.position_mi,
            Some(ctx.world),
        ) * 60.0;
        co_driver::player_finishes(
            self.player_hos_now(ctx),
            &ctx.settings.hos_mode,
            drive_min_left,
        )
    }

    /// Why the co-driver hands back at a lot when their hours end the shift
    /// and the rest is not slept out there.
    pub(crate) fn co_driver_lot_reason(&self, ctx: &GameContext) -> &'static str {
        if self.co_driver_shoulder_due(ctx) && self.player_finishes_load(ctx) {
            "hours up, yours reach"
        } else if self.team_shift() == TeamShift::EndingLastLot {
            "last lot"
        } else {
            "hours up"
        }
    }

    /// Why the co-driver hands back stopped on the shoulder with their hours
    /// up: "shoulder" sleeps out the player's rest there first.
    pub(crate) fn co_driver_shoulder_reason(&self, ctx: &GameContext) -> &'static str {
        if !self.co_driver_shoulder_due(ctx) {
            "shoulder hours"
        } else if self.player_finishes_load(ctx) {
            "shoulder hours, yours reach"
        } else {
            "shoulder"
        }
    }

    /// One frame of the shift's bookkeeping: the co-driver's clock, the
    /// player's rest and the lots ahead, fed to the shift machine. Runs
    /// before the stops are planned.
    pub(crate) fn pilot_shift_frame(&mut self, ctx: &mut GameContext, pilot: &mut Pilot) {
        let left = co_driver::shift_left_min(hos_of(ctx), &ctx.settings.hos_mode);
        // The clock only runs down at the wheel: more time than a frame ago
        // is a reset under the shift (ten hours out of service), and the
        // shift its hours were ending is open again.
        if let (Some(before), Some(now)) = (pilot.last_shift_left, left) {
            if now > before + 1.0 {
                log::info!(
                    target: TRANSCRIPT_TARGET,
                    "[co-driver] their clock reset: {now:.0} minutes of driving left"
                );
                if self.team_shift().hours_ending() {
                    self.pilot_drop_plan(pilot);
                }
                self.team_shift_event(TeamShiftEvent::ClockReset);
            }
        }
        pilot.last_shift_left = left;
        let Some(left) = left else {
            return;
        };
        let shift = self.team_shift();
        if left <= co_driver::CO_DRIVER_SUMMON_LEAD_MIN {
            if shift == TeamShift::Shoulder {
                return;
            }
            let in_reach = self.pilot_lot_makeable(pilot, left);
            self.team_shift_event(TeamShiftEvent::HoursShort {
                lot_in_reach: in_reach,
            });
            if !in_reach {
                // Nothing left to pull in at: no exit is taken on the way
                // to the shoulder.
                self.pilot_drop_plan(pilot);
            }
            return;
        }
        if shift == TeamShift::Driving {
            if self.player_rest_complete(ctx) {
                log::info!(target: TRANSCRIPT_TARGET, "[co-driver] the driver's rest is in");
                self.team_shift_event(TeamShiftEvent::RestIn);
                pilot.making_for = None;
            } else if self.co_driver_last_lot(ctx).is_some() {
                log::info!(
                    target: TRANSCRIPT_TARGET,
                    "[co-driver] the next lot is the last their hours reach"
                );
                self.team_shift_event(TeamShiftEvent::LastLotInReach);
                pilot.making_for = None;
            }
        }
    }

    /// Whether the lot the co-driver is making for can still be made with
    /// `left` minutes of hours: on its ramp, or near enough to reach before
    /// the backstop at the pace they can make.
    fn pilot_lot_makeable(&self, pilot: &Pilot, left: f64) -> bool {
        let Some((key, _)) = &pilot.making_for else {
            return false;
        };
        let Some(stop) = self.trip.stops.iter().find(|stop| &stop.key() == key) else {
            return false;
        };
        if !super::pilot::stop_serves(stop, StopPurpose::ShiftEnd) {
            return false;
        }
        if self
            .ramp_stop
            .as_ref()
            .is_some_and(|ramp| &ramp.key() == key)
        {
            return true;
        }
        let reach = (left - CO_DRIVER_STOP_MARGIN_MIN).max(0.0) / 60.0 * self.co_driver_pace_mph();
        stop.at_mi - self.trip.position_mi <= reach
    }

    /// The co-driver has nowhere to pull in: the plan, the planned stop and
    /// the exit armed for it all go.
    pub(crate) fn pilot_drop_plan(&mut self, pilot: &mut Pilot) {
        let Some((key, _)) = pilot.making_for.take() else {
            return;
        };
        if self.trip.planned_stop_key.as_deref() == Some(key.as_str()) {
            self.trip.planned_stop_key = None;
        }
        if self.selected_stop_key.as_deref() == Some(key.as_str()) {
            self.clear_selected_stop_intent();
        }
    }
}
