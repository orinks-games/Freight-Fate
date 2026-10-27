//! The co-driver's hours run out with no lot in reach and the player's
//! 10-hour rest not yet in (owner rule, 2026-10-22). Handing back on the
//! road there woke the player into a shift whose 14-hour window had kept
//! running through the short sleep: an agent drive on I-5 in California,
//! where no truck lot lies between Corning and Ripon, handed back with 8.7
//! hours of the rest still owed. Now the co-driver brakes to a stop on the
//! shoulder and the rest is slept out there, on the same terms as the
//! player's own emergency shoulder sleep: poor rest, and the seeded chance
//! of a parking ticket and of roadside damage. A 30-minute break that
//! comes due with no stop reached is taken there too, and the drive goes
//! on: the same agent drive was woken for Dan's break in the Shasta
//! mountains with 2.2 hours of the rest still owed.

use ff_core::models::co_driver;
use ff_core::sim::hos;

use crate::app::{GameContext, TRANSCRIPT_TARGET};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::notes::NoteWeight;

impl DrivingState {
    /// Whether a shift that ends away from a lot ends on the shoulder: the
    /// player in the sleeper still owes some of their 10-hour rest.
    pub(crate) fn co_driver_shoulder_due(&self, ctx: &GameContext) -> bool {
        self.player_rest_left_min(ctx)
            .is_some_and(|left| left > 0.0)
    }

    /// The truck is stopped on the shoulder: the player sleeps out the rest
    /// of their 10 hours where it stands, and the co-driver rests too.
    /// Returns the lines for the ticket and the damage, if either came due.
    pub(crate) fn co_driver_sleeps_on_shoulder(&mut self, ctx: &mut GameContext) -> Vec<String> {
        let Some(left) = self.player_rest_left_min(ctx).filter(|left| *left > 0.0) else {
            return Vec::new();
        };
        let anchor = self.trip.position_mi;
        secure_truck_for_stopped_menu(self, ctx);
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
        profile.fatigue = hos::rest_shoulder(profile.fatigue);
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] stops on the shoulder at mile {anchor:.1} and the driver sleeps {left:.0} more minutes"
        );
        shoulder_risks(self, ctx, anchor)
    }

    /// The co-driver's 30-minute break, taken stopped on the shoulder.
    pub(crate) fn co_driver_shoulder_break(&mut self, ctx: &mut GameContext) {
        let Some(name) = self.co_driver_at_wheel_name() else {
            return;
        };
        advance_rest_clock(self, ctx, hos::BREAK_MIN, None, "");
        hos_mut_of(ctx).take_break(hos::BREAK_MIN);
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] takes a 30-minute break on the shoulder at mile {:.1}",
            self.trip.position_mi
        );
        self.co_driver_note(
            ctx,
            format!("{name} took a 30-minute break on the shoulder."),
            NoteWeight::Passing,
        );
    }
}
