//! Cattle on the board: where a load of live cattle may go, and how the
//! 28-hour limit plans it (`sim::livestock::plan` has the rules).

use super::{Candidate, JobBoard};
use crate::data::world_models::Location;
use crate::models::co_driver::TEAM_LOAD_PAY_MULT;
use crate::models::jobs::{route_drive_hours, Job, DEADLINE_AVG_MPH};
use crate::models::start_options::start_option;
use crate::pyfmt::round_py_n;
use crate::sim::livestock::{cattle_run, livestock_lane_ok, CattleKind, LIVESTOCK_CARGO_KEY};
use crate::sim::trip_route_helpers::haversine_distance_mi;

use crate::data::world_models::Route;

impl<'w> JobBoard<'w> {
    /// A destination for cattle loaded at `origin`: a city with the next
    /// facility on the cattle's way (a feed yard for calves, a beef plant
    /// for fed cattle) and a run the limit allows, nearer cities likelier.
    pub(crate) fn livestock_destination(
        &mut self,
        city: &str,
        origin: &Location,
        candidates: &[Candidate],
        level: i64,
    ) -> Option<(String, f64, Location)> {
        let kind = CattleKind::for_origin(&origin.facility_type);
        let mut options: Vec<(String, f64, &'w Location)> = Vec::new();
        for (destination, miles, _legs) in candidates {
            if destination == city {
                continue;
            }
            let Some(dest_city) = self.world.cities.get(destination) else {
                continue;
            };
            for location in &dest_city.locations {
                if location.min_level > level
                    || !livestock_lane_ok(&origin.facility_type, &location.facility_type)
                    || !location.receives.iter().any(|k| k == LIVESTOCK_CARGO_KEY)
                {
                    continue;
                }
                let air = haversine_distance_mi(origin.lat, origin.lon, location.lat, location.lon);
                // A cheap first screen; the real route decides in make_job.
                if !cattle_run(kind, miles / DEADLINE_AVG_MPH, air).fits {
                    continue;
                }
                options.push((destination.clone(), *miles, location));
            }
        }
        if options.is_empty() {
            return None;
        }
        let weights: Vec<f64> = options
            .iter()
            .map(|(_, miles, _)| 1.0 / (1.0 + miles / 150.0))
            .collect();
        let idx = self.rng.choices_indices_weighted(&weights, 1)[0];
        let (destination, miles, location) = &options[idx];
        Some((destination.clone(), *miles, (*location).clone()))
    }

    /// Plan a cattle load on its real route. False when it does not fit
    /// inside the limit, which a dispatcher would not book.
    pub(crate) fn plan_cattle_run(
        &self,
        job: &mut Job,
        route: Option<&Route>,
        origin: &Location,
        destination: &Location,
        slack: f64,
        carrier_key: &str,
    ) -> bool {
        let kind = CattleKind::for_origin(&origin.facility_type);
        let drive_h = route_drive_hours(route, 0.0, Some(self.world));
        let air = haversine_distance_mi(origin.lat, origin.lon, destination.lat, destination.lon);
        let run = cattle_run(kind, drive_h, air);
        if !run.fits {
            return false;
        }
        job.deadline_covers_rest = false;
        job.team = run.team;
        if run.team {
            job.pay = round_py_n(job.pay * TEAM_LOAD_PAY_MULT, 2);
        }
        let profile_slack = start_option(Some(carrier_key))
            .dispatch_profile()
            .deadline_slack;
        job.deadline_game_h = round_py_n(run.deadline_h(slack * profile_slack), 1);
        true
    }
}
