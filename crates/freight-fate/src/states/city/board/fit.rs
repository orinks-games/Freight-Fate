//! What dispatch needs to know about each load to recommend or assign one
//! (`ff_core::models::dispatch_fit`): the board measures, ff-core judges.

use ff_core::models::business::is_owner_operator;
use ff_core::models::career_objectives::career_objective;
use ff_core::models::career_training::{
    company_training_stage, is_company_training_profile, training_guidance,
    training_recommendation_score, TrainingStage,
};
use ff_core::models::dispatch_fit::{dispatch_goal, DispatchGoal, LoadFacts};
use ff_core::models::dispatch_policy::{dispatch_policy, SENIOR_LOAD_CHOICE_LEVEL};
use ff_core::models::jobs::{
    lane_key, lane_recent_either_way, plan_hos, route_drive_hours, team_required_hours, Job,
};
use ff_core::models::profile::Profile;
use ff_core::playtest_levers::forced_dispatch_destination;
use ff_core::sim::hos::limits;

use crate::app::GameContext;
use crate::states::city::{first_dispatch_done, profile};
use crate::states::city_pickup::{PICKUP_CHECK_IN_MIN, PICKUP_LOADING_MIN};

use super::{locked_reason, pickup_drive_hours, settlement_for};

/// What dispatch is planning this driver's board for.
pub(super) fn goal_for(p: &Profile) -> DispatchGoal {
    let owner = is_owner_operator(&p.business_status);
    let training = (!owner).then(|| company_training_stage(p));
    let revoked = p.career.level() >= SENIOR_LOAD_CHOICE_LEVEL && dispatch_policy(p).assigns_load;
    dispatch_goal(training, p.standing(), revoked, owner)
}

/// The training guidance's label for a recommended row, as the board has
/// always spoken it: the first-dispatch label until the first delivery,
/// then the career objective's.
pub(super) fn training_label(p: &Profile) -> Option<String> {
    if is_company_training_profile(p) {
        let guidance = training_guidance(p);
        if guidance.stage == TrainingStage::FirstDispatch && !first_dispatch_done(p) {
            return Some(guidance.recommendation_label);
        }
    }
    if first_dispatch_done(p) || is_company_training_profile(p) {
        return Some(career_objective(p).recommendation);
    }
    None
}

/// Every load on the board, measured on the driver's current clock.
pub(super) fn load_facts(ctx: &GameContext, jobs: &[Job]) -> Vec<LoadFacts> {
    let p = profile(ctx);
    let world = ctx.world;
    let here = world.resolve_city_key(&p.current_city);
    let forced = forced_dispatch_destination();
    let forced = (!forced.is_empty()).then(|| world.resolve_city_key(&forced));
    jobs.iter()
        .map(|job| {
            let (trip_h, loaded_h) = trip_hours(ctx, job);
            LoadFacts {
                take_home: settlement_for(p, job, true).net_before_advance,
                trip_h,
                deadline_room_h: job.deadline_game_h - loaded_h,
                recent_lane: lane_recent_either_way(&p.recent_lanes, &lane_key(world, job)),
                locked: !locked_reason(p, job).is_empty(),
                training_score: training_recommendation_score(p, job),
                relay: !job.bobtail && world.resolve_city_key(&job.origin) != here,
                forced: forced
                    .as_ref()
                    .is_some_and(|key| &world.resolve_city_key(&job.destination) == key),
            }
        })
        .collect()
}

/// `(whole trip, loaded drive)` in hours from accepting this load.
///
/// The empty drive and the pickup's check-in and loading come first and
/// burn the shift; the loaded drive is planned on the clock that leaves the
/// shipper, with its breaks and 10-hour sleeps (a team load swaps drivers
/// instead). With hours of service off nothing forces a rest, so the drive
/// time alone counts.
fn trip_hours(ctx: &GameContext, job: &Job) -> (f64, f64) {
    let world = ctx.world;
    let route = world
        .supported_route(&job.origin, &job.destination, None)
        .ok()
        .flatten();
    let pickup_h = pickup_drive_hours(ctx, job);
    let work_min = if job.bobtail {
        0.0
    } else {
        PICKUP_CHECK_IN_MIN + PICKUP_LOADING_MIN
    };
    let loaded_h = if limits(&ctx.settings.hos_mode).is_none() {
        route_drive_hours(route.as_ref(), 0.0, Some(world))
    } else if job.team {
        team_required_hours(job.distance_mi, route.as_ref(), Some(world))
    } else {
        let mut clock = profile(ctx).hos.clone();
        clock.driving_min += pickup_h * 60.0;
        clock.duty_min += pickup_h * 60.0 + work_min;
        // Check-in and loading are more than 30 minutes off the wheel,
        // which is the break the clock asks for.
        clock.since_break_min = if work_min >= 30.0 {
            0.0
        } else {
            clock.since_break_min + pickup_h * 60.0
        };
        plan_hos(job.distance_mi, route.as_ref(), Some(world), Some(&clock)).total_h()
    };
    (pickup_h + work_min / 60.0 + loaded_h, loaded_h)
}
