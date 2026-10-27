//! The hours and fuel a dispatch will ask of the driver, as Job details
//! lines: what a run needs is read before it is accepted, not met as a
//! countdown at the wheel (player report, 2026-10-17).

use ff_core::data::world_models::Route;
use ff_core::models::jobs::{plan_hos, Job};
use ff_core::pyfmt::fmt_f;
use ff_core::sim::hos::limits;

use crate::app::GameContext;
use crate::states::city::profile;
use crate::states::city_pickup::{PICKUP_CHECK_IN_MIN, PICKUP_LOADING_MIN};
use crate::states::driving_core::ASSUMED_RANGE_MPG;

use super::pickup_drive_hours;

/// "Trip hours: about 14 driving hours, with a 10-hour sleep." then the
/// driver's own driving time left. Empty with hours of service off.
pub(super) fn hours_lines(ctx: &GameContext, job: &Job) -> Vec<String> {
    let mode = &ctx.settings.hos_mode;
    if limits(mode).is_none() {
        return Vec::new();
    }
    let p = profile(ctx);
    // The clock as the loaded truck leaves the shipper: the drive to the
    // pickup and the check-in and loading come off the shift first.
    let pickup_h = pickup_drive_hours(ctx, job);
    let mut clock = p.hos.clone();
    if pickup_h > 0.0 {
        clock.drive(pickup_h * 60.0);
    }
    clock.on_duty(PICKUP_CHECK_IN_MIN + PICKUP_LOADING_MIN);
    let route = ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .ok()
        .flatten();
    let plan = plan_hos(
        job.distance_mi,
        route.as_ref(),
        Some(ctx.world),
        Some(&clock),
    );
    // Whole lines, so a translation matches each one as it is spoken.
    let hours = fmt_f(pickup_h + plan.drive_h, 1);
    let sleeps = plan.sleeps;
    let trip = match (sleeps, plan.breaks) {
        // A team load's deadline has the drivers changing over where a solo
        // driver would sleep: naming the sleeps planned a run the deadline
        // cannot fit (player report, 2026-10-20).
        (1.., _) if job.team => format!(
            "Trip hours: about {hours} driving hours, with the co-driver taking turns instead of \
             10-hour sleeps."
        ),
        (0, 0) => format!("Trip hours: about {hours} driving hours."),
        (0, _) => format!("Trip hours: about {hours} driving hours, with a 30-minute break."),
        (1, _) => format!("Trip hours: about {hours} driving hours, with a 10-hour sleep."),
        _ => format!("Trip hours: about {hours} driving hours, with {sleeps} 10-hour sleeps."),
    };
    let mut lines = vec![trip, p.hos.drive_time_summary(mode, false)];
    // Where the co-driver is offered, before the rows turn up there: the
    // dock's own test, on the clock the loaded truck leaves with. A team
    // load says so in its deadline already.
    let drive_min = crate::states::city_pickup_team::team_drive_min(ctx, job);
    if !job.team && ff_core::models::co_driver::team_worthwhile(&clock, mode, drive_min) {
        lines.push(
            "Co-driver: offered at the shipper's dock or from the pause menu, on your current hours."
                .to_string(),
        );
    }
    lines
}

/// "Fuel: 60 percent, about 900 miles of range.", and "short of this trip"
/// when the tank will not reach the destination.
pub(super) fn fuel_line(ctx: &GameContext, job: &Job) -> String {
    let p = profile(ctx);
    let tank = p.truck_specs().fuel_tank_gal;
    let fuel = p.truck_fuel_gal().min(tank);
    let pct = if tank > 0.0 { fuel / tank * 100.0 } else { 0.0 };
    let range_mi = fuel * ASSUMED_RANGE_MPG;
    let here = ctx.world.resolve_city_key(&p.current_city);
    let deadhead_mi = if ctx.world.resolve_city_key(&job.origin) == here {
        0.0
    } else {
        ctx.world
            .supported_route(&here, &job.origin, None)
            .ok()
            .flatten()
            .map(|route| route.miles())
            .unwrap_or(0.0)
    };
    let percent = fmt_f(pct, 0);
    let range = ctx.settings.distance_text(range_mi, false);
    if range_mi < deadhead_mi + job.distance_mi {
        format!("Fuel: {percent} percent, about {range} of range, short of this trip.")
    } else {
        format!("Fuel: {percent} percent, about {range} of range.")
    }
}

/// The departure briefing, spoken once as the loaded truck leaves: the
/// driver's hours and tank, then the one stop this run will need, if any.
/// "Driving time left: 7 hours, duty window 9 hours. Fuel: 60 percent. This
/// run needs a 10-hour sleep."
///
/// With the co-driver taking the wheel (`team`), the stop line is left out:
/// the team changes drivers instead of stopping for the solo driver's sleep.
pub(crate) fn departure_briefing(ctx: &GameContext, route: &Route, team: bool) -> String {
    let p = profile(ctx);
    let mode = &ctx.settings.hos_mode;
    let mut parts: Vec<String> = Vec::new();
    let mut needs: Option<String> = None;
    if limits(mode).is_some() {
        parts.push(p.hos.drive_time_summary(mode, true));
    }
    if limits(mode).is_some() && !team {
        let plan = plan_hos(route.miles(), Some(route), Some(ctx.world), Some(&p.hos));
        needs = match (plan.sleeps, plan.breaks) {
            (0, 0) => None,
            (0, _) => Some("This run needs a 30-minute break.".to_string()),
            (1, _) => Some("This run needs a 10-hour sleep.".to_string()),
            (sleeps, _) => Some(format!("This run needs {sleeps} 10-hour sleeps.")),
        };
    }
    let tank = p.truck_specs().fuel_tank_gal;
    let fuel = p.truck_fuel_gal().min(tank);
    let percent = fmt_f(if tank > 0.0 { fuel / tank * 100.0 } else { 0.0 }, 0);
    let range_mi = fuel * ASSUMED_RANGE_MPG;
    if range_mi < route.miles() {
        let range = ctx.settings.distance_text(range_mi, false);
        parts.push(format!(
            "Fuel: {percent} percent, about {range} of range, short of this trip."
        ));
    } else {
        parts.push(format!("Fuel: {percent} percent."));
    }
    parts.extend(needs);
    parts.join(" ")
}
