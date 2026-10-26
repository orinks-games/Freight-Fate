//! Leaving the shipper with the co-driver at the wheel.

use ff_core::models::co_driver;
use ff_core::models::jobs::Job;

use crate::app::GameContext;
use crate::states::base::MenuItem;
use crate::states::city::profile;
use crate::states::city_pickup::PickupFacilityState;
use crate::states::driving_team::handover::{job_pairing, take_intro, terms_text};
use crate::states::driving_team::TeamMode;

/// The driving minutes a co-driver offer is judged on: the route's own
/// drive hours, or the deadline pace when the world has no route. The dock
/// and the board's job details both use it, so the board promises only
/// what the dock offers.
pub(crate) fn team_drive_min(ctx: &GameContext, job: &Job) -> f64 {
    let route = ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .ok()
        .flatten();
    match route {
        Some(route) => {
            ff_core::models::jobs::route_drive_hours(Some(&route), 0.0, Some(ctx.world)) * 60.0
        }
        None => job.distance_mi / ff_core::models::jobs::DEADLINE_AVG_MPH * 60.0,
    }
}

impl PickupFacilityState {
    /// Whether the dock offers a team departure: a team load, or one long
    /// enough that driving it alone needs a 10-hour break.
    pub(crate) fn team_departure_offered(&self, ctx: &GameContext) -> bool {
        if !self.loaded {
            return false;
        }
        if self.job.team {
            return true;
        }
        let drive_min = team_drive_min(ctx, &self.job);
        co_driver::team_worthwhile(&profile(ctx).hos, &ctx.settings.hos_mode, drive_min)
    }

    /// The introduction to team driving, the first time a career's dock
    /// offers a co-driver.
    pub(crate) fn team_intro(&self, ctx: &mut GameContext) -> Option<String> {
        let heard = ctx
            .profile
            .as_ref()
            .is_none_or(|p| co_driver::introduced(&p.achievement_stats));
        if heard || !self.team_departure_offered(ctx) {
            return None;
        }
        let name = job_pairing(ctx, &self.job).co_driver.name;
        take_intro(ctx, name, true)
    }

    /// The two team rows, placed after Depart for destination.
    pub(crate) fn team_departure_rows(&self, ctx: &GameContext) -> Vec<MenuItem<Self>> {
        if !self.team_departure_offered(ctx) {
            return Vec::new();
        }
        let pairing = job_pairing(ctx, &self.job);
        let name = pairing.co_driver.name;
        let carrier = ff_core::models::career::carrier_name_of(profile(ctx));
        let who = co_driver::pairing_line(&pairing, &carrier);
        let terms = terms_text(ctx, name);
        vec![
            MenuItem::new(
                format!("Depart with {name} driving, and ride along"),
                |s: &mut Self, ctx| s.depart_as_team(ctx, TeamMode::RideAlong),
            )
            .help(format!(
                "{who} You rest in the sleeper and hear the road. {terms}"
            )),
            MenuItem::new(
                format!("Depart with {name} driving, and sleep"),
                |s: &mut Self, ctx| s.depart_as_team(ctx, TeamMode::SleepThrough),
            )
            .help(format!(
                "{who} You wake when {name} needs you, with a summary of the drive. {terms}"
            )),
        ]
    }

    fn depart_as_team(&mut self, ctx: &mut GameContext, mode: TeamMode) {
        self.team_departure = Some(mode);
        self.depart_for_destination(ctx);
    }
}
