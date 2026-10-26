//! Deadline and minimum-pay model (the `plan_hos` / `required_hours` /
//! `route_drive_hours` / `minimum_pay_for_level` half of `jobs.py`).

use crate::data::curves::route_curves;
use crate::data::world::World;
use crate::data::world_models::{Leg, Route};
use crate::models::jobs::{Job, LONG_HAUL_MILES};
use crate::pyfmt::{fmt_f, round_py_n};
use crate::sim::hos::HosClock;
use crate::sim::trip_models::{
    approach_shed_mi, corridor_speed_limit, leg_speed_limit_at, DESTINATION_APPROACH_LIMIT_MPH,
    DESTINATION_LOCAL_APPROACH_MI, FACILITY_ACCESS_LIMIT_MPH, FACILITY_GATE_LIMIT_MPH,
    FACILITY_GATE_ZONE_MI, URBAN_LIMIT_MPH, URBAN_RADIUS_MI,
};

/// flat load/unload fee keeping short hops worthwhile
pub const HOOKUP_FEE: f64 = 120.0;
// Dispatch minimums: a small "worth rolling the truck" flat floor plus a
// short-haul rate premium that tapers into the long-haul rates, so dollars
// per mile decline gently with distance the way real freight does -- drayage
// pays a premium per mile, never several times more. The old flat $700-1050
// floors paid a 50-mile hop ~$23 a mile, four to five times any long haul,
// at every level, which made grinding short hops strictly optimal.
/// the full premium rate holds up to here
pub const SHORT_HAUL_FULL_PREMIUM_MI: f64 = 100.0;
/// $/mi at short range
pub const SHORT_HAUL_RATE_BY_LEVEL: &[(i64, f64)] = &[(1, 4.70), (2, 5.10), (3, 5.50)];
/// $/mi at 600
pub const SHORT_HAUL_TAPER_END_RATE_BY_LEVEL: &[(i64, f64)] = &[(1, 3.20), (2, 3.35), (3, 3.50)];
pub const DISPATCH_FLAT_MINIMUM_BY_LEVEL: &[(i64, f64)] = &[(1, 300.0), (2, 325.0), (3, 350.0)];
pub const LONG_HAUL_MINIMUM_RATE_BY_LEVEL: &[(i64, f64)] = &[(4, 4.75), (5, 5.25)];

// Deadline model: what a law-abiding trucker actually needs.
/// achievable interstate average through zones and weather
pub const DEADLINE_AVG_MPH: f64 = 55.0;
pub const DEADLINE_PLANNING_SPEED_FACTOR: f64 = 0.88;
pub const DEADLINE_SAMPLE_MI: f64 = 2.0;
pub const DEADLINE_MIN_SEGMENT_MPH: f64 = 10.0;
pub const DEADLINE_DISPATCH_MIN_SLACK_H: f64 = 1.0;
pub const DEADLINE_DISPATCH_SLACK_RANGE: (f64, f64) = (1.2, 1.5);
pub const ACTIVE_TRIP_FAIRNESS_SLACK: f64 = 1.2;

fn table(table: &[(i64, f64)], level: i64) -> Option<f64> {
    table.iter().find(|(l, _)| *l == level).map(|(_, v)| *v)
}

fn table_max_level(table: &[(i64, f64)]) -> i64 {
    table.iter().map(|(l, _)| *l).max().unwrap_or(1)
}

#[derive(Debug, Clone, PartialEq)]
pub struct HosPlan {
    pub drive_h: f64,
    pub breaks: i64,
    pub sleeps: i64,
    pub break_stop_count: i64,
    pub sleep_stop_count: i64,
}

impl HosPlan {
    pub fn total_h(&self) -> f64 {
        self.drive_h + self.breaks as f64 * 0.5 + self.sleeps as f64 * 10.0
    }

    /// The plan in one short line; a break or sleep the route does not need
    /// is not mentioned.
    pub fn summary(&self) -> String {
        let mut plan = format!("{} driving hours", fmt_f(self.drive_h, 1));
        if self.breaks != 0 {
            plan.push_str(&format!(
                ", {} 30-minute break{}",
                self.breaks,
                if self.breaks != 1 { "s" } else { "" }
            ));
        }
        if self.sleeps != 0 {
            plan.push_str(&format!(
                ", {} 10-hour sleep{}",
                self.sleeps,
                if self.sleeps != 1 { "s" } else { "" }
            ));
        }
        let coverage = if self.has_stop_coverage() {
            format!(
                " Break stops: {}. Sleep stops: {}.",
                self.break_stop_count, self.sleep_stop_count
            )
        } else {
            String::new()
        };
        format!("{plan}.{coverage}")
    }

    /// Whether [`Self::summary`] names the route's break and sleep stops.
    pub fn has_stop_coverage(&self) -> bool {
        self.break_stop_count != 0 || self.sleep_stop_count != 0
    }
}

/// Estimate the FMCSA-compliant plan for a property-carrying trip.
///
/// Based on FMCSA's public HOS summary: 11 driving hours after 10 off-duty
/// hours, a 14-hour window, and a 30-minute break after 8 cumulative driving
/// hours. Split sleeper and 60/70-hour cycle limits are intentionally not
/// modeled in this route estimate.
pub fn plan_hos(
    miles: f64,
    route: Option<&Route>,
    world: Option<&World>,
    clock: Option<&HosClock>,
) -> HosPlan {
    let drive_h = match route {
        Some(route) => route_drive_hours(Some(route), 0.0, world),
        None => miles / DEADLINE_AVG_MPH,
    };
    match clock {
        None => plan_hos_for_drive_hours(drive_h, route, 0.0, 0.0, 0.0),
        Some(clock) => plan_hos_for_drive_hours(
            drive_h,
            route,
            clock.driving_min / 60.0,
            clock.duty_min / 60.0,
            clock.since_break_min / 60.0,
        ),
    }
}

/// Apply the HOS break/sleep model to already-estimated driving hours.
///
/// The `start_*` hours seed the first shift with the driver's CURRENT clock,
/// so a load accepted six hours into a shift plans its 10-hour sleep where
/// the law will actually force one. A fresh clock (all zeros) reproduces the
/// original fresh-driver plan exactly. The 14-hour window is tracked here too
/// -- irrelevant for a fresh driver (11 driving + a break fits easily) but
/// decisive mid-shift.
fn plan_hos_for_drive_hours(
    drive_h: f64,
    route: Option<&Route>,
    start_drive_h: f64,
    start_window_h: f64,
    start_since_break_h: f64,
) -> HosPlan {
    let mut breaks = 0;
    let mut sleeps = 0;
    let mut remaining = drive_h;
    let mut since_break = start_since_break_h.max(0.0);
    let mut drive_this_shift = start_drive_h.max(0.0);
    let mut window_this_shift = start_window_h.max(0.0);
    while remaining > 1e-6 {
        // A full sleep resets the break clock too. When both limits are due,
        // planning a separate 30-minute break first invents legal time the
        // driver does not have to spend.
        if drive_this_shift >= 11.0
            || window_this_shift >= 14.0
            || (since_break >= 8.0 && window_this_shift + 0.5 >= 14.0)
        {
            sleeps += 1;
            drive_this_shift = 0.0;
            window_this_shift = 0.0;
            since_break = 0.0;
        }
        if since_break >= 8.0 {
            breaks += 1;
            since_break = 0.0;
            window_this_shift += 0.5; // the 30-minute break burns window
        }
        let step = remaining
            .min(8.0 - since_break)
            .min(11.0 - drive_this_shift)
            .min(14.0 - window_this_shift);
        remaining -= step;
        since_break += step;
        drive_this_shift += step;
        window_this_shift += step;
    }
    let mut break_stops = 0;
    let mut sleep_stops = 0;
    if let Some(route) = route {
        for stop in route.accessible_stop_details(false) {
            let has = |a: &str| stop.actions.iter().any(|x| x == a);
            break_stops += i64::from(has("break") || has("food"));
            sleep_stops += i64::from(has("sleep"));
        }
    }
    HosPlan {
        drive_h,
        breaks,
        sleeps,
        break_stop_count: break_stops,
        sleep_stop_count: sleep_stops,
    }
}

/// Honest hours for the run: driving at an achievable average, plus the
/// 30-minute break every 8 driving hours and a 10-hour sleep for every
/// 11-hour shift the distance demands -- planned from the driver's CURRENT
/// shift clock when one is given. Dispatch cannot ask for less.
pub fn required_hours(
    miles: f64,
    route: Option<&Route>,
    world: Option<&World>,
    clock: Option<&HosClock>,
) -> f64 {
    plan_hos(miles, route, world, clock).total_h()
}

/// Minimum legal time for the actual route from `start_mi` onward.
pub fn route_required_hours(route: &Route, start_mi: f64, world: Option<&World>) -> f64 {
    plan_hos_for_drive_hours(
        route_drive_hours(Some(route), start_mi, world),
        Some(route),
        0.0,
        0.0,
        0.0,
    )
    .total_h()
}

/// Legal plan for the route still ahead using the driver's current HOS clock.
/// Used only for a one-time deadline repair of an older active delivery.
pub fn remaining_route_hos_plan(
    route: &Route,
    start_mi: f64,
    world: Option<&World>,
    clock: &HosClock,
) -> HosPlan {
    plan_hos_for_drive_hours(
        route_drive_hours(Some(route), start_mi, world),
        Some(route),
        clock.driving_min / 60.0,
        clock.duty_min / 60.0,
        clock.since_break_min / 60.0,
    )
}

/// Deadline from the current route-aware timing model plus dispatch slack.
///
/// `clock` is the driver's live shift ledger: a load that will need a 10-hour
/// sleep because of hours ALREADY burned gets that sleep in its deadline,
/// instead of a fresh-driver promise nobody could legally keep (owner catch,
/// 2026-07-24).
pub fn dispatch_deadline_hours(
    miles: f64,
    slack: f64,
    route: Option<&Route>,
    world: Option<&World>,
    clock: Option<&HosClock>,
) -> f64 {
    required_hours(miles, route, world, clock) * slack + DEADLINE_DISPATCH_MIN_SLACK_H
}

/// Honest hours for a team load: the same driving and 30-minute breaks, but
/// where a solo driver sleeps 10 hours the team changes drivers instead.
pub fn team_required_hours(miles: f64, route: Option<&Route>, world: Option<&World>) -> f64 {
    let plan = plan_hos(miles, route, world, None);
    plan.drive_h
        + plan.breaks as f64 * 0.5
        + plan.sleeps as f64 * crate::models::co_driver::TEAM_CHANGE_H
}

/// [`dispatch_deadline_hours`] for a team load.
pub fn team_deadline_hours(
    miles: f64,
    slack: f64,
    route: Option<&Route>,
    world: Option<&World>,
) -> f64 {
    team_required_hours(miles, route, world) * slack + DEADLINE_DISPATCH_MIN_SLACK_H
}

/// The deadline floor a load gets at its loaded departure: the team model
/// for a team load, the driver's own clock otherwise.
pub fn departure_deadline_hours(
    job: &Job,
    miles: f64,
    slack: f64,
    route: Option<&Route>,
    world: Option<&World>,
    clock: Option<&HosClock>,
) -> f64 {
    if job.team {
        team_deadline_hours(miles, slack, route, world)
    } else {
        dispatch_deadline_hours(miles, slack, route, world, clock)
    }
}

/// One-time compatibility floor for jobs saved before route-aware timing.
///
/// Older source snapshots may have a deadline from the old mileage-only model.
/// On resume, keep generous existing deadlines, but lift too-tight ones enough
/// to cover both the whole route's fair model and the route still ahead.
pub fn fair_active_deadline(
    job: &Job,
    route: &Route,
    hours_used: f64,
    position_mi: f64,
    world: Option<&World>,
) -> f64 {
    let full_floor = dispatch_deadline_hours(
        route.miles(),
        ACTIVE_TRIP_FAIRNESS_SLACK,
        Some(route),
        world,
        None,
    );
    let remaining_floor = hours_used
        + route_required_hours(route, position_mi, world) * ACTIVE_TRIP_FAIRNESS_SLACK
        + DEADLINE_DISPATCH_MIN_SLACK_H;
    round_py_n(job.deadline_game_h.max(full_floor).max(remaining_floor), 1)
}

/// One `(start_mi, end_mi, advisory_mph)` band of route miles held to a bend's
/// advisory speed.
pub type CurveBand = (f64, f64, f64);

/// Non-overlapping `(start_mi, end_mi, advisory)` bands over route miles.
///
/// The planner walks the route on posted limits, so every bend a driver has
/// to slow for was time the plan never budgeted (measured 2026-08-16: 2.8
/// percent of drive time on average, 5.6 percent Denver to Salt Lake City).
/// This is the road's own answer to "how fast may a truck be here", read
/// from the same baked curve records the pacenote layer speaks.
///
/// Overlapping bends -- a chained S through a canyon -- collapse to the
/// TIGHTEST advisory across the overlap, because that is the one binding on
/// the truck. Connector arcs are excluded: ramps are not on the through
/// route the planner is timing.
pub fn curve_ceilings(route: &Route) -> Vec<CurveBand> {
    let mut bands: Vec<CurveBand> = Vec::new();
    let mut edges: Vec<f64> = Vec::new();
    for curve in route_curves(route, &route.cities, true) {
        if curve.end_mi > curve.start_mi && curve.advisory_mph > 0 {
            bands.push((curve.start_mi, curve.end_mi, curve.advisory_mph as f64));
            edges.push(curve.start_mi);
            edges.push(curve.end_mi);
        }
    }
    if bands.is_empty() {
        return Vec::new();
    }
    // `set` then `sorted` on the Python side: sort, then drop exact repeats.
    edges.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    edges.dedup();
    let mut out: Vec<CurveBand> = Vec::new();
    for pair in edges.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        if hi <= lo {
            continue;
        }
        let mid = (lo + hi) / 2.0;
        let advisory = bands
            .iter()
            .filter(|(s, e, _)| *s <= mid && mid < *e)
            .map(|(_, _, adv)| *adv)
            .fold(f64::INFINITY, f64::min);
        if !advisory.is_finite() {
            continue;
        }
        // Merge with the previous band when it is the same speed and touches,
        // so a long chain of identical advisories is one span, not fifty.
        match out.last_mut() {
            Some(last) if last.2 == advisory && (last.1 - lo).abs() < 1e-9 => last.1 = hi,
            _ => out.push((lo, hi, advisory)),
        }
    }
    out
}

/// Hours across `[start_mi, end_mi)`, slowed inside bends to their advisory.
pub fn segment_hours(start_mi: f64, end_mi: f64, mph: f64, ceilings: &[CurveBand]) -> f64 {
    if end_mi <= start_mi {
        return 0.0;
    }
    let floor_mph = DEADLINE_MIN_SEGMENT_MPH.max(mph);
    if ceilings.is_empty() {
        return (end_mi - start_mi) / floor_mph;
    }
    let mut hours = 0.0;
    let mut cursor = start_mi;
    for &(band_start, band_end, advisory) in ceilings {
        if band_end <= cursor {
            continue;
        }
        if band_start >= end_mi {
            break;
        }
        if band_start > cursor {
            hours += (band_start - cursor) / floor_mph;
            cursor = band_start;
        }
        let overlap_end = band_end.min(end_mi);
        if overlap_end > cursor {
            hours += (overlap_end - cursor) / DEADLINE_MIN_SEGMENT_MPH.max(mph.min(advisory));
            cursor = overlap_end;
        }
    }
    if cursor < end_mi {
        hours += (end_mi - cursor) / floor_mph;
    }
    hours
}

/// Route-aware drive-time estimate on posted limits AND curve advisories.
///
/// Each sampled segment is timed piecewise: the miles inside a bend at that
/// bend's advisory, the rest at the planning limit. Only the bend's own
/// recorded span is charged -- the plan does not invent a deceleration ramp
/// on either side of it, which is the sort of unmeasured loss
/// `DEADLINE_PLANNING_SPEED_FACTOR` is already there to carry.
pub fn route_drive_hours(route: Option<&Route>, start_mi: f64, world: Option<&World>) -> f64 {
    let Some(route) = route else {
        return 0.0;
    };
    // Python reaches its `_curve_ceilings` call only past these two early
    // returns; walking the route's curves for a trip already finished would be
    // real work on the job board's hot path.
    let route_miles = route.miles();
    if route_miles <= start_mi.clamp(0.0, route_miles.max(0.0)) {
        return 0.0;
    }
    let is_facility_approach =
        route.cities.len() >= 2 && route.cities.first() == route.cities.last();
    let ceilings = if is_facility_approach {
        Vec::new()
    } else {
        curve_ceilings(route)
    };
    route_drive_hours_over(Some(route), start_mi, world, &ceilings)
}

/// `route_drive_hours` with the bend bands supplied, which is how the port
/// reaches the "what would the plan say blind to the bends" half of the
/// Python test that monkeypatches `_curve_ceilings`.
pub fn route_drive_hours_over(
    route: Option<&Route>,
    start_mi: f64,
    world: Option<&World>,
    ceilings: &[CurveBand],
) -> f64 {
    let Some(route) = route else {
        return 0.0;
    };
    route_drive_hours_between(route, start_mi, route.miles(), world, ceilings)
}

/// Estimate one interval without adding travel beyond the chosen stop.
pub(super) fn route_drive_hours_between(
    route: &Route,
    start_mi: f64,
    end_mi: f64,
    world: Option<&World>,
    ceilings: &[CurveBand],
) -> f64 {
    let route_miles = route.miles();
    let start_mi = start_mi.clamp(0.0, route_miles.max(0.0));
    let end_mi = end_mi.clamp(start_mi, route_miles.max(start_mi));
    if end_mi <= start_mi {
        return 0.0;
    }
    let mut hours = 0.0;
    let mut leg_starts: Vec<f64> = Vec::with_capacity(route.legs.len());
    let mut acc = 0.0;
    for leg in &route.legs {
        leg_starts.push(acc);
        acc += leg.miles;
    }
    let mut city_mileposts = leg_starts.clone();
    city_mileposts.push(route_miles);
    let is_facility_approach =
        route.cities.len() >= 2 && route.cities.first() == route.cities.last();
    for (index, (leg_start, leg)) in leg_starts.iter().zip(route.legs.iter()).enumerate() {
        let leg_start = *leg_start;
        let leg_end = (leg_start + leg.miles).min(end_mi);
        let segment_start = start_mi.max(leg_start);
        if segment_start >= leg_end {
            continue;
        }
        let mut offset = segment_start - leg_start;
        while offset < leg_end - leg_start - 1e-6 {
            let step = DEADLINE_SAMPLE_MI.min(leg_end - leg_start - offset);
            let global_start = leg_start + offset;
            if global_start + step <= start_mi {
                offset += step;
                continue;
            }
            let mid = global_start + step / 2.0;
            let mph = route_planning_limit(
                route,
                index,
                leg,
                offset + step / 2.0,
                mid,
                &city_mileposts,
                is_facility_approach,
                world,
            );
            hours += segment_hours(global_start, global_start + step, mph, ceilings);
            offset += step;
        }
    }
    hours
}

/// `_route_planning_limit`: the planning speed at one sample point.
#[allow(clippy::too_many_arguments)]
pub fn route_planning_limit(
    route: &Route,
    leg_index: usize,
    leg: &Leg,
    offset_mi: f64,
    route_mi: f64,
    city_mileposts: &[f64],
    is_facility_approach: bool,
    world: Option<&World>,
) -> f64 {
    let route_miles = route.miles();
    let mut limit;
    // A chain with the street detail plans each street at the limit the drive
    // will post on it, the yard included (`Trip::street_zones`); a chain
    // without it keeps the flat access-road number and the gate stretch.
    let street_detail = is_facility_approach
        && route
            .legs
            .iter()
            .any(|leg| leg.local_limit.is_some() || leg.local_yard);
    if is_facility_approach {
        limit = leg.street_limit_mph().unwrap_or(FACILITY_ACCESS_LIMIT_MPH);
    } else {
        let baked = leg_speed_limit_at(leg, offset_mi);
        let toward_city = &route.cities[(leg_index + 1).min(route.cities.len() - 1)];
        let region = route_city_region(toward_city, world);
        limit = baked.unwrap_or_else(|| corridor_speed_limit(&leg.highway, &region));
        if baked.is_none()
            && city_mileposts
                .iter()
                .any(|mp| (route_mi - mp).abs() <= URBAN_RADIUS_MI)
        {
            limit = limit.min(URBAN_LIMIT_MPH);
        }
        // The arrival zones the drive will really build: the local approach
        // road capped at ramp speed, and the shed into it sized from the
        // corridor limit here. Planning cannot see which facility the job ends
        // at, so it plans the synthetic approach every job at least gets.
        let approach_mi =
            DESTINATION_LOCAL_APPROACH_MI + approach_shed_mi(limit, DESTINATION_APPROACH_LIMIT_MPH);
        if route_mi >= (route_miles - approach_mi).max(0.0) {
            limit = limit.min(DESTINATION_APPROACH_LIMIT_MPH);
        }
    }
    if !street_detail && route_mi >= (route_miles - FACILITY_GATE_ZONE_MI).max(0.0) {
        limit = limit.min(FACILITY_GATE_LIMIT_MPH);
    }
    limit * DEADLINE_PLANNING_SPEED_FACTOR
}

fn route_city_region(city: &str, world: Option<&World>) -> String {
    let Some(world) = world else {
        return String::new();
    };
    world
        .cities
        .get(city)
        .map(|c| c.region.clone())
        .unwrap_or_default()
}

/// Dispatch minimums keep short jobs worth the player's time.
///
/// The guaranteed rate per mile starts at the short-haul premium, holds
/// through SHORT_HAUL_FULL_PREMIUM_MI, then slides linearly down to the
/// taper-end rate at LONG_HAUL_MILES, where the long-haul minimum rates
/// (levels 4+) take over. The flat floor only matters on the shortest hops.
pub fn minimum_pay_for_level(miles: f64, level: i64) -> f64 {
    let lvl = level.min(table_max_level(SHORT_HAUL_RATE_BY_LEVEL)).max(1);
    let full = table(SHORT_HAUL_RATE_BY_LEVEL, lvl).expect("short-haul rate");
    let end = table(SHORT_HAUL_TAPER_END_RATE_BY_LEVEL, lvl).expect("taper rate");
    let rate = if miles <= SHORT_HAUL_FULL_PREMIUM_MI {
        full
    } else if miles >= LONG_HAUL_MILES {
        end
    } else {
        let span = LONG_HAUL_MILES - SHORT_HAUL_FULL_PREMIUM_MI;
        full - (full - end) * (miles - SHORT_HAUL_FULL_PREMIUM_MI) / span
    };
    let mut pay = table(DISPATCH_FLAT_MINIMUM_BY_LEVEL, lvl)
        .expect("flat minimum")
        .max(miles * rate);
    let long_haul_rate = table(
        LONG_HAUL_MINIMUM_RATE_BY_LEVEL,
        level.min(table_max_level(LONG_HAUL_MINIMUM_RATE_BY_LEVEL)),
    );
    if let Some(long_haul_rate) = long_haul_rate {
        if miles >= LONG_HAUL_MILES {
            pay = pay.max(miles * long_haul_rate);
        }
    }
    pay
}
