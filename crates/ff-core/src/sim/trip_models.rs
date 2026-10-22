//! Value types and pure helpers the trip simulation is built from (port of
//! `freight_fate/sim/trip_models.py`): posted-limit lookups, the statutory
//! truck caps, congestion math, the grounded hazard catalog, and the small
//! records a `Trip` schedules and announces.

use crate::data::curves::RouteCurve;
use crate::data::world_constants::{
    vehicle_access_allows, DEFAULT_VEHICLE_ACCESS, FACILITY_APPROACH_TRUSTED_MAX_MI,
    STOP_TYPE_LABELS,
};
use crate::data::world_models::{Leg, SpeedLimitSample, TollEvent};
use crate::pyfmt::fmt_f;
use crate::pyrandom::PyRandom;
use crate::sim::enforcement_posts::EnforcementPost;
use crate::sim::real_traffic_parsers::TrafficEvent;
use crate::sim::timezones::TimeZone;
use crate::sim::traffic_manager::TrafficVehicle;
use crate::sim::weather::WeatherKind;
use crate::speech_text::{typed_name, SpokenMessage};

mod hazards;
mod ramps;
pub use hazards::{
    eligible_hazards, hazard_brake_to_mph, hazard_is_animal, hazard_is_in_lane, hazard_name,
    HazardDef, OpenSide, ANIMAL_BRAKE_TO_MPH, HAZARDS,
};
pub use ramps::{
    acceleration_lane_capability_mph, acceleration_lane_mi, deceleration_lane_mi, exit_ramp_layout,
    merge_traffic_target_mph, ramp_curve_mi, ramp_speed_mph, truck_merge_speed_mph, ExitRampLayout,
    RampAdvisorySpeed, ACCELERATION_LANE_FT, ACCELERATION_LANE_GRADE_FACTOR, DECELERATION_LANE_FT,
    DECELERATION_LANE_GRADE_FACTOR, DECELERATION_LANE_RAMP_MPH, GRADE_MODEL_MAX_PCT,
    GRADE_MODEL_MIN_PCT, MERGE_RECOVERY_MAX_MI, MERGE_RECOVERY_MAX_REAL_S,
    MERGE_TRAFFIC_SPEED_SHARE, RAMP_CURVE_DEFLECTION_RAD, RAMP_DIRECTIONAL_SHARE,
    RAMP_MIN_DESIGN_MPH, RAMP_QUEUE_FT, RAMP_SURFACE_SHARE, RAMP_TANGENT_CLIMB_FT,
    TRUCK_ACCEL_ALPHA_FPS2, TRUCK_ACCEL_BETA,
};

pub const BASE_SPEED_LIMIT_MPH: f64 = 70.0;

// Posted speed limit by corridor. Where a leg carries a baked OSM `maxspeed`
// profile (see `Leg::speed_limits`), the runtime uses that real posted limit;
// otherwise it falls back to this heuristic, derived from the highway class
// and region -- rural Interstates run faster out West -- and dropped to an
// urban limit near cities.
pub const URBAN_LIMIT_MPH: f64 = 55.0;
pub const URBAN_RADIUS_MI: f64 = 6.0; // urban speed reduction within this distance of a city
pub const US_HIGHWAY_LIMIT_MPH: f64 = 65.0;
pub const STATE_ROUTE_LIMIT_MPH: f64 = 60.0;

/// Rural Interstate posted limit by region.
pub const INTERSTATE_RURAL_LIMIT_MPH: &[(&str, f64)] = &[
    ("great_basin", 80.0),
    ("southern_plains", 75.0),
    ("desert_southwest", 75.0),
    ("rockies", 75.0),
    ("gulf_coast", 75.0),
    ("heartland", 70.0),
    ("great_lakes", 70.0),
    ("upper_midwest", 70.0),
    ("corn_belt", 70.0),
    ("mid_south", 70.0),
    ("atlantic_southeast", 70.0),
    ("florida", 70.0),
    ("appalachia", 70.0),
    ("pacific_northwest", 70.0),
    ("northeast", 65.0),
    ("california", 65.0),
];

/// Jurisdictions whose statute holds heavy trucks below the general posted
/// limit. Keyed by road class (from `highway_class`) with a "default"
/// fallback; "max" is the highest truck speed the statute permits ANYWHERE in
/// the state and bounds how far a baked maxspeed:hgv tag may raise the limit.
///
/// Every entry verified against statute text 2026-07-19; the audit, per-state
/// sources and the states found to have NO split are in
/// docs/truck-speed-limit-audit.md. DO NOT edit a number here without a
/// citation and an access date -- the game speaks the state's name aloud when
/// this table binds.
pub const STATE_TRUCK_MAX_MPH: &[(&str, &[(&str, f64)])] = &[
    // A.R.S. 28-709: >26,000 lb declared GW, statewide.
    ("Arizona", &[("default", 65.0)]),
    // Ark. Code 27-51-201(b): CMV >=26,001 lb GVWR on rural divided
    // controlled-access highways. 27-51-201(c)(2) deliberately NOT encoded.
    ("Arkansas", &[("default", 70.0)]),
    // CVC 22406: three or more axles, or any vehicle towing. Statewide.
    ("California", &[("default", 55.0)]),
    // IC 9-21-5-2(a)(4): declared GVW >26,000 lb, buses excluded.
    ("Indiana", &[("default", 65.0)]),
    // MCL 257.627(4): GVW >=10,000 lb or any truck-tractor.
    ("Michigan", &[("default", 65.0)]),
    // MCA 61-8-312: 70 on interstates, 65 on all other public highways.
    ("Montana", &[("interstate", 70.0), ("default", 65.0)]),
    // ORS 811.111(1)(b): 55 on ANY highway by default; 60 and 65 are
    // named-corridor exceptions carried as maxspeed:hgv, so "max" opens them.
    ("Oregon", &[("default", 55.0), ("max", 65.0)]),
    // RCW 46.61.410: >10,000 lb GVW and all combinations, statewide.
    ("Washington", &[("default", 60.0)]),
];
// Removed by the 2026-07-19 audit: Idaho (repealed by H664), Nevada and
// North Dakota (no split ever existed). Deliberately absent: Illinois (six
// Chicago-area counties only) and Virginia (secondary roads only).

/// Driving lanes per direction on a leg, defaulting to a two-lane rural
/// interstate. Honors a baked `lanes` field once OSM enrichment adds one.
pub fn leg_lane_count(leg: Option<&Leg>) -> i64 {
    match leg {
        None => DEFAULT_LEG_LANES,
        Some(leg) => {
            let lanes = if leg.lanes != 0 {
                leg.lanes
            } else {
                DEFAULT_LEG_LANES
            };
            lanes.max(1)
        }
    }
}

/// Python `_highway_class`.
pub fn highway_class(highway: &str) -> &'static str {
    let h = highway.trim().to_uppercase();
    if h.starts_with("I-") || h.starts_with("I ") || h.starts_with("INTERSTATE") {
        return "interstate";
    }
    if h.starts_with("US") {
        return "us_highway";
    }
    "state_route"
}

/// Open-road posted limit for a corridor from its highway class and region.
pub fn corridor_speed_limit(highway: &str, region: &str) -> f64 {
    let cls = highway_class(highway);
    if cls == "interstate" {
        return INTERSTATE_RURAL_LIMIT_MPH
            .iter()
            .find(|(k, _)| *k == region)
            .map(|(_, v)| *v)
            .unwrap_or(BASE_SPEED_LIMIT_MPH);
    }
    if cls == "us_highway" {
        return US_HIGHWAY_LIMIT_MPH;
    }
    STATE_ROUTE_LIMIT_MPH
}

/// Baked OSM posted limit at a leg-relative offset, or `None` if unbaked.
///
/// The samples are a step function: the limit in effect is the last sample
/// at or before the offset. Before the first sample, the first applies. A
/// sample with `mph` of `None` is a coverage-gap marker -- the answer is
/// `None` (fall back to the heuristic) rather than a stale town posting.
pub fn leg_speed_limit_at(leg: &Leg, offset_mi: f64) -> Option<f64> {
    posted_sample_at(leg, offset_mi).and_then(|s| s.mph)
}

/// State in effect at a leg-relative offset in the leg's A-to-B direction.
pub fn leg_state_at(leg: &Leg, offset_mi: f64) -> String {
    let crossings = leg.state_crossings();
    if crossings.is_empty() {
        let miles = leg.state_miles();
        return if miles.len() == 1 {
            miles[0].state.clone()
        } else {
            String::new()
        };
    }
    let mut state = crossings[0].from_state.as_str();
    for crossing in crossings {
        if crossing.at_mi <= offset_mi {
            state = crossing.state.as_str();
        } else {
            break;
        }
    }
    state.to_string()
}

/// The last posting at or before a leg offset, or None where none is baked.
pub fn posted_sample_at(leg: &Leg, offset_mi: f64) -> Option<&SpeedLimitSample> {
    let samples = leg.speed_limits();
    if samples.is_empty() {
        return None;
    }
    let mut chosen = &samples[0];
    for sample in samples {
        if sample.at_mi <= offset_mi {
            chosen = sample;
        } else {
            break;
        }
    }
    Some(chosen)
}

/// (cap for this road class, highest cap the state permits anywhere), or
/// None where the state holds trucks to the general limit.
pub fn statutory_truck_caps(state: &str, highway: &str) -> Option<(f64, f64)> {
    let table = STATE_TRUCK_MAX_MPH
        .iter()
        .find(|(k, _)| *k == state)
        .map(|(_, t)| *t)?;
    if table.is_empty() {
        return None;
    }
    let lookup = |key: &str| table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    let cap = lookup(highway_class(highway)).or_else(|| lookup("default"))?;
    // "max" defaults to this class's own cap, NOT the highest entry in the
    // table: a class-scoped split (Montana) must not let a stray hgv tag
    // license the interstate number on a back highway.
    Some((cap, lookup("max").unwrap_or(cap)))
}

/// Python `_truck_capped_speed_limit`.
pub fn truck_capped_speed_limit(leg: &Leg, offset_mi: f64) -> Option<f64> {
    let chosen = posted_sample_at(leg, offset_mi)?;
    // Inside a coverage gap: no posting is known here, so the caller's
    // highway/region heuristic answers, not the last town limit.
    let mph = chosen.mph?;
    let Some((cap, permitted)) = statutory_truck_caps(&leg_state_at(leg, offset_mi), &leg.highway)
    else {
        return Some(mph);
    };
    if chosen.hgv {
        // An explicit maxspeed:hgv is better evidence than a statewide
        // default, but it is trusted only as far as the statute allows: a
        // stray tag CANNOT license an illegal speed.
        return Some(mph.min(permitted));
    }
    Some(mph.min(cap))
}

/// Whether the limit in force here is truck-specific, and the state to
/// credit for it. `(false, None)` where the posting is simply the posting.
///
/// A stretch reaches 55 by either of two routes, and the driver must not be
/// able to tell them apart: OSM carries an explicit `maxspeed:hgv` or it
/// carries only the car number and the statutory cap pulls it down.
pub fn truck_limit_at(leg: &Leg, offset_mi: f64) -> (bool, Option<String>) {
    let Some(chosen) = posted_sample_at(leg, offset_mi) else {
        return (false, None);
    };
    let Some(mph) = chosen.mph else {
        return (false, None);
    };
    let state = leg_state_at(leg, offset_mi);
    let caps = statutory_truck_caps(&state, &leg.highway);
    if let Some((cap, _)) = caps {
        if cap < mph {
            return (true, Some(state));
        }
    }
    if chosen.hgv {
        // Tagged truck-specific. Credit the state only where it actually has
        // a statutory split.
        return (true, if caps.is_some() { Some(state) } else { None });
    }
    (false, None)
}

// A city's truck stops are baked onto every leg that meets that city, a mile
// out from the endpoint, so a route driving *through* the city collects the
// same facility twice, exactly two miles apart.
pub const SHARED_CITY_STOP_MERGE_MI: f64 = 3.0;

pub const FACILITY_ACCESS_LIMIT_MPH: f64 = 25.0;
// Graduated synthetic approach: the wide-out portion of a long local
// approach is an arterial, not an access road (owner, 2026-07-24).
pub const FACILITY_ARTERIAL_LIMIT_MPH: f64 = 45.0;
pub const FACILITY_ACCESS_TAIL_MI: f64 = 2.0;
/// The speed at or below which an off-ramp can actually be taken, and so the
/// floor for anything the arrival zones cap.
pub const RAMP_MAX_MPH: f64 = 45.0;
/// The exit assist needs real braking time throughout this approach.
pub const EXIT_SPEED_ASSIST_START_MI: f64 = 1.5;
/// The destination approach never caps below the speed the ramp needs.
pub const DESTINATION_APPROACH_LIMIT_MPH: f64 = RAMP_MAX_MPH;
/// ASSUMED, and it cannot be otherwise: no vehicle code reaches inside a
/// private facility, so this is the game's number, chosen at the top of the
/// observed 5-15 range.
pub const FACILITY_GATE_LIMIT_MPH: f64 = 15.0;
/// The yard limit: past the driveway, on the facility's own way, up to the
/// check-in stop at the gate. INDUSTRY PRACTICE, ASSUMED for any one yard: no
/// public law reaches inside a private gate, and operators post their own --
/// New Orleans Terminal "15 MPH in the yard" (notml.com/trucker-information),
/// Northwest Seaport Alliance terminals 10-15 (nwseaportalliance.com terminal
/// safety rules), Port Houston Bayport 20 (HSSE SAF401 driver manual), the
/// Amazon carrier SOP 10 mph on site (Pan-EU/GB, Dec 2025; not US). 15 is the
/// middle of that spread, and the gate's own number, so the check-in rules
/// and the yard agree.
pub const YARD_LIMIT_MPH: f64 = FACILITY_GATE_LIMIT_MPH;
pub const FACILITY_GATE_ZONE_MI: f64 = 0.5;
/// ...but never more than this share of the approach.
pub const FACILITY_GATE_MAX_SHARE: f64 = 0.35;
/// Local approach road assumed when the destination facility has no usable
/// approach record.
pub const DESTINATION_LOCAL_APPROACH_MI: f64 = 1.0;
pub const DESTINATION_APPROACH_TRUSTED_MAX_MI: f64 = FACILITY_APPROACH_TRUSTED_MAX_MI;

// -- How a loaded truck sheds speed for something ahead ----------------------
pub const APPROACH_DECEL_MPS2: f64 = 0.4;
pub const APPROACH_REACTION_S: f64 = 6.0;
pub const APPROACH_SETTLE_S: f64 = 2.0;
pub const MPH_PER_MPS: f64 = 2.23694;
pub const METERS_PER_MILE: f64 = 1609.344;

/// Road a loaded truck needs to come down from one speed to another, in
/// route miles: the shed priced at the mean of the two ends, the reaction
/// budget at the entry speed and the settling tail at the number it leaves on.
pub fn approach_shed_mi(from_mph: f64, to_mph: f64) -> f64 {
    if to_mph >= from_mph {
        return 0.0;
    }
    let shed_s = (from_mph - to_mph) / MPH_PER_MPS / APPROACH_DECEL_MPS2;
    let shed_mi = shed_s * (from_mph + to_mph) / 2.0 / 3600.0;
    shed_mi + (APPROACH_REACTION_S * from_mph + APPROACH_SETTLE_S * to_mph) / 3600.0
}

pub const NIGHT_HAZARD_BONUS: f64 = 0.10; // extra hazard risk after dark
/// A zone flip that flips back within this distance is boundary noise.
pub const TIMEZONE_DWELL_MI: f64 = 10.0;
/// Open road guaranteed between generated slow zones.
pub const ZONE_MIN_GAP_MI: f64 = 8.0;
pub const RUSH_HOUR_WINDOWS: [(f64, f64); 2] = [(6.5, 9.0), (16.0, 18.5)];
// -- Grounded congestion: volume against capacity, not a dice roll ------------
pub const LANE_CAPACITY_VPH: f64 = 2000.0;
pub const DIRECTIONAL_SPLIT: f64 = 0.55;
pub const CONGESTION_MIN_RATIO: f64 = 0.72;
pub const CONGESTION_HEAVY_RATIO: f64 = 0.9;
pub const CONGESTION_JAM_RATIO: f64 = 1.05;
pub const CONGESTION_SAMPLE_MI: f64 = 1.0;
pub const CONGESTION_MIN_ZONE_MI: f64 = 1.0;
/// Hourly share of daily traffic (indexed by clock hour). Sums to ~1.0.
pub const HOURLY_SHARE_WEEKDAY: [f64; 24] = [
    0.008, 0.005, 0.004, 0.005, 0.010, 0.025, // 0-5
    0.050, 0.072, 0.068, 0.052, 0.048, 0.050, // 6-11
    0.053, 0.054, 0.058, 0.068, 0.078, 0.080, // 12-17
    0.062, 0.045, 0.035, 0.028, 0.022, 0.014, // 18-23
];
pub const HOURLY_SHARE_WEEKEND: [f64; 24] = [
    0.014, 0.010, 0.007, 0.006, 0.007, 0.012, // 0-5
    0.024, 0.035, 0.048, 0.065, 0.073, 0.077, // 6-11
    0.077, 0.075, 0.073, 0.071, 0.067, 0.060, // 12-17
    0.052, 0.044, 0.037, 0.030, 0.023, 0.016, // 18-23
];

/// Heuristic AADT for legs with no baked HPMS profile: (rural, near-metro).
pub const HEURISTIC_AADT: &[(&str, (f64, f64))] = &[
    ("interstate", (26000.0, 92000.0)),
    ("us_highway", (11000.0, 34000.0)),
    ("state_route", (7000.0, 20000.0)),
];

/// Share of the day's traffic moving in this clock hour.
pub fn hourly_volume_fraction(hour: f64, weekend: bool) -> f64 {
    let table = if weekend {
        &HOURLY_SHARE_WEEKEND
    } else {
        &HOURLY_SHARE_WEEKDAY
    };
    // Python `int(hour) % 24`: truncation, then a non-negative modulus.
    let idx = (hour.trunc() as i64).rem_euclid(24) as usize;
    table[idx]
}

// AADT is an ANNUAL AVERAGE daily traffic, and no single day is the average.
// The FHWA Traffic Monitoring Guide's whole apparatus of day-of-week and
// monthly adjustment factors exists because of that spread; day-of-week is
// already modelled here (weekday against weekend tables), so what is left is
// the residual day-to-day scatter around the mean for a given kind of day.
// Ten percent is the conservative end of the published range for a continuous
// count station on a major route.
//
// This is what stops a busy stretch being a wall in the same place every run.
// It is not a "chance of traffic" dial: an oversaturated stretch still backs
// up every day, because its ratio is far enough over the line that no ordinary
// day clears it, while a marginal one -- the midday shoulder of a commuter
// corridor -- falls under on a quiet day and flows. The variety comes out of
// the same volume model rather than being sprinkled on top of it.
pub const DAILY_VOLUME_CV: f64 = 0.10;
pub const DAILY_VOLUME_MIN: f64 = 0.75;
pub const DAILY_VOLUME_MAX: f64 = 1.30;

/// Today's traffic against the annual mean, for one stretch of road.
pub fn daily_volume_factor(rng: &mut PyRandom) -> f64 {
    // Python: `max(MIN, min(MAX, rng.gauss(1.0, CV)))`.
    DAILY_VOLUME_MIN.max(DAILY_VOLUME_MAX.min(rng.gauss(1.0, DAILY_VOLUME_CV)))
}

/// Peak-direction volume-to-capacity ratio for an hour of the day.
pub fn congestion_ratio(aadt: f64, hour: f64, lanes: i64, weekend: bool) -> f64 {
    let vph = aadt * hourly_volume_fraction(hour, weekend) * DIRECTIONAL_SPLIT;
    vph / (lanes.max(1) as f64 * LANE_CAPACITY_VPH)
}

/// Prevailing traffic speed for a volume-to-capacity ratio, or `None` when
/// traffic still moves at the posted limit.
pub fn congestion_limit_mph(ratio: f64, posted: f64) -> Option<f64> {
    if ratio < CONGESTION_MIN_RATIO {
        return None;
    }
    if ratio < CONGESTION_HEAVY_RATIO {
        return Some(45.0_f64.max(posted.min(posted - 12.0)));
    }
    if ratio < CONGESTION_JAM_RATIO {
        return Some(38.0);
    }
    Some(26.0)
}

/// Baked (AADT, per-direction lanes) at a leg-relative offset, or `None`
/// when the leg carries no HPMS profile. Step function like speed limits.
pub fn leg_aadt_at(leg: &Leg, offset_mi: f64) -> Option<(f64, i64)> {
    let samples = leg.traffic_volumes();
    if samples.is_empty() {
        return None;
    }
    let mut chosen = &samples[0];
    for sample in samples {
        if sample.at_mi <= offset_mi {
            chosen = sample;
        } else {
            break;
        }
    }
    Some((chosen.aadt, chosen.lanes))
}

pub fn heuristic_aadt(highway: &str, near_city: bool) -> f64 {
    let cls = highway_class(highway);
    let (rural, metro) = HEURISTIC_AADT
        .iter()
        .find(|(k, _)| *k == cls)
        .or_else(|| HEURISTIC_AADT.iter().find(|(k, _)| *k == "state_route"))
        .map(|(_, v)| *v)
        .expect("state_route is in the table");
    if near_city {
        metro
    } else {
        rural
    }
}

pub const CONSTRUCTION_CLOSURE_CHANCE: f64 = 0.65;
pub const DEFAULT_LEG_LANES: i64 = 2;
/// The most lanes per direction the game can put a driver in -- a SPEECH
/// limit before a driving one: `lane_label` has exactly three names.
pub const MAX_DRIVABLE_LANES: i64 = 3;
pub const TRAFFIC_LOOKAHEAD_MI: f64 = 2.5;
pub const TRAFFIC_WARNING_GAP_S: f64 = 2.2;
pub const TRAFFIC_PRESSURE_LOOKAHEAD_MI: f64 = 2.5;
pub const TRAFFIC_PRESSURE_MIN_INTENSITY: f64 = 0.12;
pub const CONSTRUCTION_TAPER_MI: f64 = 1.0;
pub const CONSTRUCTION_TAPER_LIMIT_MPH: f64 = 55.0;
/// One signed roadwork footprint is both zones: the work and the taper.
pub const CONSTRUCTION_ZONE_REASONS: [&str; 2] = ["construction", "construction merge"];
pub const LANE_CLOSURE_SAMPLE_MI: f64 = 0.25;
pub const CORRIDOR_HAZARD_MIN_FACTOR: f64 = 0.75;
pub const CORRIDOR_HAZARD_MAX_FACTOR: f64 = 1.45;
pub const CB_PATROL_LOOKAHEAD_MI: f64 = 5.0;
pub const ENFORCEMENT_WARNING_REAL_S: f64 = 18.0;
pub const ENFORCEMENT_WARNING_MAX_MI: f64 = 12.0;
pub const SCALE_WARNING_REAL_S: f64 = 20.0;
/// Spoken enforcement lines are capped for a whole run.
pub const CB_CALLS_PER_RUN: usize = 2;
pub const ZONE_WARNING_LOOKAHEAD_MI: f64 = 2.0;
pub const ZONE_WARNING_REAL_S: f64 = 18.0;
pub const ZONE_WARNING_MAX_MI: f64 = 10.0;
/// Clock multiplier when stopped or crawling; full pacing resumes at cruise.
pub const LOW_SPEED_TIME_SCALE: f64 = 4.0;
pub const FULL_COMPRESSION_MPH: f64 = 50.0;
/// An open scale crossed faster than this is a bypass; at or under it the
/// truck is pulling in. The scale reminder's real-time hold pins the clock
/// only above it (`Trip::real_time_override`).
pub const SCALE_BYPASS_MPH: f64 = 15.0;
/// A pacing change made mid-trip waits until the truck is slower than this.
/// Motion integrates on real seconds while the road passes at the pace, and
/// fuel is billed at the pace, so at any one pace a hill, a coast and a
/// climb each cost what they should per mile. Speed a hill gave on one clock
/// and spent on another is free: descend in Real time, switch to Standard,
/// and coast for miles (flight, 2026-09-22).
pub const PACE_CHANGE_MAX_MPH: f64 = 0.5;
/// Parked with the brake set, waiting runs at double the configured pacing.
pub const PARKED_TIME_SCALE_MULT: f64 = 2.0;
pub const CONSTRUCTION_ENFORCEMENT_GRACE_MI: f64 = 1.5;
pub const CHAIN_LAW_MIN_GRADE: f64 = 0.05;
/// States that post commercial chain controls on their mountain routes, so a
/// sustained grade there can carry a chain law. READ, from each state's own
/// program: CDOT's Commercial Vehicle Chain Law (I-70), WYDOT's chain law,
/// Caltrans chain controls R-1 to R-3, ODOT snow zones, WSDOT chain
/// requirements over 10,000 lb, and the Idaho, Montana, Nevada and Utah chain
/// restrictions on their passes. Nowhere else runs a commercial chain-control
/// program, so a steep mile in Texas or Wisconsin is only a steep mile
/// (seasonal audit, 2026-10-01: 218 legs in 35 states had chain areas).
pub const CHAIN_CONTROL_STATES: [&str; 9] = ["CA", "CO", "ID", "MT", "NV", "OR", "UT", "WA", "WY"];
/// Whether the player's rig (a five-axle tractor-trailer on a tandem drive)
/// must chain under a chain law at `level` in `state`. Heavy trucks get no
/// winter-tire exemption: Caltrans R-1 to R-3 ("no exemption for heavy-duty
/// commercial vehicles ... equipped with snow tires", dot.ca.gov chain
/// requirements) and WSDOT over 10,000 lb (WAC 204-24-050) chain trucks.
/// Oregon's lower level ("Chains or traction tires required") chains only
/// single-drive-axle trucks over 10,000 lb, and "Chains required" chains
/// every truck (ODOT, oregon.gov/odot/mct/pages/chains-and-traction-tires.aspx).
/// Colorado is the one split: Level 1 chains single-drive-axle combinations
/// only, and Level 2 chains every truck (CDOT Commercial Vehicle Chain Law,
/// csp.colorado.gov/chain-law-information). Wyoming Level 1 accepts snow
/// tires or chains, and Level 2 requires chains on two tires of one drive
/// axle (WYDOT chain law; W.S. 31-5-956(e)). ID, MT, NV and UT are
/// UNVERIFIED and follow the general rule: trucks chain.
pub fn rig_must_chain(state: Option<&str>, level: i64) -> bool {
    match level {
        ..=0 => false,
        1 => !matches!(state, Some("CO") | Some("WY") | Some("OR")),
        _ => true,
    }
}

/// The rule a chain-law sign speaks for `state` (two-letter code) at
/// `level` (`rig_must_chain`).
pub fn chain_law_sign_rule(state: Option<&str>, level: i64) -> &'static str {
    match (state, level >= 2) {
        (Some("WY"), true) => "level 2: chains required on two tires of one drive axle",
        (_, true) => "level 2: chains required on all trucks",
        (Some("CO") | Some("OR"), false) => "level 1: chains required on single-drive-axle trucks",
        (Some("WY"), false) => "level 1: snow tires or chains required",
        (_, false) => "level 1: chains required on trucks",
    }
}

pub const CHAIN_LAW_MIN_RUN_MI: f64 = 1.0;
pub const CHAIN_LAW_JOIN_GAP_MI: f64 = 2.0;
/// Caltrans: "about a mile between 'Chains Required' signs and the
/// checkpoint" (winter driving tips).
pub const CHAIN_LAW_LEAD_MI: f64 = 1.0;

/// The highways each sourced state runs commercial chain controls on. A
/// steep run in one of these states makes a chain-law area only on one of
/// its listed highways; a state not listed (MT, NV, UT, WY) keeps the
/// grade-only rule, UNVERIFIED until their own route lists are read. Leg
/// highways carry no mileposts, so the listed mileposts below are matched by
/// highway plus the steep-grade rule, not by milepost.
/// - CO: the chain-carry corridors (csp.colorado.gov/chain-law-information):
///   I-70 west of MP 259, CO-9 MP 63-97, US-40 west of MP 256, US-50 west of
///   MP 225, US-160 west of MP 304, US-285 west of MP 250, US-550 MP 0-130.
/// - CA: I-80 Donner and I-5 Grapevine and Siskiyou (Caltrans chain
///   requirements); US-50 Echo, CA-58 Tehachapi and I-15 Cajon are
///   UNVERIFIED rows, kept because Caltrans posts controls there.
/// - WA: WAC 204-24-050(2)(i): I-90 MP 32-101, I-82 MP 3-26, US-2 MP 36-108,
///   US-12 MP 135-187, US-97 (two stretches), SR-410, SR-20, SR-155, SR-970,
///   SR-14 and SR-542.
/// - ID: I-90 (Fourth of July and Lookout passes) and US-12 (Lolo Pass).
/// - OR: ODOT snow zones on I-84 Cabbage Hill, I-5 Siskiyou, US-26 and
///   OR-58 (Willamette Pass).
pub const CHAIN_CORRIDORS: [(&str, &[&str]); 5] = [
    (
        "CO",
        &[
            "I-70", "CO-9", "US-40", "US-50", "US-160", "US-285", "US-550",
        ],
    ),
    ("CA", &["I-80", "I-5", "US-50", "CA-58", "I-15"]),
    (
        "WA",
        &[
            "I-90", "I-82", "US-2", "US-12", "US-97", "WA-410", "WA-20", "WA-155", "WA-970",
            "WA-14", "WA-542",
        ],
    ),
    ("OR", &["I-84", "I-5", "US-26", "OR-58"]),
    ("ID", &["I-90", "US-12"]),
];

/// Whether `highway` in `state` can carry a chain law (`CHAIN_CORRIDORS`).
pub fn on_chain_corridor(state: &str, highway: &str) -> bool {
    match CHAIN_CORRIDORS.iter().find(|(code, _)| *code == state) {
        Some((_, highways)) => highways.contains(&highway),
        None => true,
    }
}

/// Colorado's carry rule: a commercial vehicle over 16,000 lb on these
/// corridors must carry chains from September 1 to May 31 (CSP chain-law
/// page). Matched by highway, inside a chain-law area (the mountain run).
pub const CO_CHAIN_CARRY_CORRIDORS: [&str; 7] = [
    "I-70", "CO-9", "US-40", "US-50", "US-160", "US-285", "US-550",
];

/// Montana: trucks of 26,001 lb or more must carry chains from October 1 to
/// April 30 (MCA 61-9-436). No fine is verified, so the game only warns.
pub fn in_mt_chain_carry_season(month: u32) -> bool {
    !(5..=9).contains(&month)
}

/// Which Montana chain season (October 1 to April 30) a point on the career
/// clock falls in, counted from the career's first October; January to
/// April belong to the season that began the October before.
pub fn mt_chain_season_id(game_hours: f64) -> i64 {
    // Day of year 274 is October 1 on the career's fixed 365-day year.
    let days = crate::sim::season::CAREER_START_DAY_OF_YEAR + game_hours / 24.0 - 274.0;
    (days / 365.0).floor() as i64
}

/// A chain-law area ahead on a chain corridor in season (`upcoming_chain_corridor`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChainCorridorAhead {
    /// `"{state}:{highway}:{start_mi to 0.1}"`, one per corridor approach.
    pub key: String,
    pub highway: String,
    pub state: String,
    /// Route mile the chain-law area (its chain-up pullout) starts at.
    pub start_mi: f64,
    /// The nearest numbered interchange at or before the area, or empty.
    pub exit_label: String,
}

/// How far before a chain-law area its approach exit is looked for.
pub const CHAIN_CORRIDOR_EXIT_LOOKBACK_MI: f64 = 5.0;

/// Whether a carry law requires chains aboard on `highway` in `state` in
/// `month`, whatever the weather: Colorado's carry corridors September 1
/// to May 31 (CSP chain-law page), Montana October 1 to April 30 (MCA
/// 61-9-436) and Washington's listed routes November 1 to April 1 (WAC
/// 204-24-050; April 1 is read at month precision, so April is out).
/// Everywhere else chains are required only while a chain law is posted
/// and the rig must chain under it (Caltrans chain controls, NRS 484D.520,
/// ORS 815.140, Idaho Code 49-948(5), UDOT R920-6, W.S. 31-5-956(e)), so
/// there is no dry-day season to remind about.
pub fn in_chain_carry_season(state: &str, highway: &str, month: u32) -> bool {
    match state {
        "CO" => CO_CHAIN_CARRY_CORRIDORS.contains(&highway) && in_co_chain_carry_season(month),
        "MT" => in_mt_chain_carry_season(month),
        "WA" => !(4..=10).contains(&month),
        _ => false,
    }
}

/// September 1 to May 31: Colorado's chain-carry season.
pub fn in_co_chain_carry_season(month: u32) -> bool {
    !(6..=8).contains(&month)
}
pub const CHAIN_LAW_SAMPLE_MI: f64 = 0.25;
pub const CONDITIONS_SPEED_MARGIN_MPH: f64 = 8.0;
pub const CONDITIONS_GRIP_CEILING: f64 = 0.85;
pub const CONDITIONS_CHECK_MI: f64 = 1.5;
pub const CONDITIONS_INCIDENT_RISK: f64 = 0.5;

/// Patrol density by region: dense, urbanized states run hot.
pub const HOT_PATROL_REGIONS: [&str; 6] = [
    "northeast",
    "california",
    "great_lakes",
    "florida",
    "atlantic_southeast",
    "mid_south",
];
pub const COLD_PATROL_REGIONS: [&str; 5] = [
    "great_basin",
    "southern_plains",
    "rockies",
    "desert_southwest",
    "heartland",
];

// The hazard catalog lives in `hazards.rs` and the ramp lane math in
// `ramps.rs`; both are re-exported here so callers keep one import path.

/// One scheduled ambient roadside line: a landmark or a billboard.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadsideCallout {
    pub key: String,
    pub at_mi: f64,
    pub category: String,
    pub spoken: String,
    /// True when this place name explains a speed limit change just ahead.
    pub explains_limit: bool,
}

impl RoadsideCallout {
    pub fn new(key: &str, at_mi: f64, category: &str, spoken: &str) -> Self {
        RoadsideCallout {
            key: key.to_string(),
            at_mi,
            category: category.to_string(),
            spoken: spoken.to_string(),
            explains_limit: false,
        }
    }

    /// Town names and the ones that explain a limit change are places:
    /// once per milepost, same count at 20x or 1:1. Everything else is
    /// sitting-budget chatter.
    pub fn is_place_callout(&self) -> bool {
        self.category == "village" || self.explains_limit
    }
}

pub const LANDMARK_MIN_SPACING_MI: f64 = 2.0;
pub const VILLAGE_PASS_OFF_MI: f64 = 1.5;
pub const VILLAGE_MIN_SPACING_MI: f64 = LANDMARK_MIN_SPACING_MI;
pub const VILLAGE_PAIR_WINDOW_MI: f64 = 1.5;
pub const VILLAGE_PAIR_MAX_LIMIT_MPH: f64 = 45.0;
pub const BILLBOARD_MIN_GAP_MI: f64 = 35.0;
pub const BILLBOARD_MAX_GAP_MI: f64 = 65.0;
pub const BILLBOARD_LEAD_IN_MI: f64 = 15.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TripEventKind {
    ZoneEnter,
    ZoneExit,
    StopAhead,
    StopReached,
    CityReached,
    Hazard,
    WeatherChange,
    Inspection,
    GpsCue,
    StateCrossing,
    TimezoneCrossing,
    Checkpoint,
    TollCharged,
    Landmark,
    Billboard,
    Curve,
    Lane,
    Arrived,
}

impl TripEventKind {
    /// The Python enum value.
    pub fn as_str(self) -> &'static str {
        match self {
            TripEventKind::ZoneEnter => "zone_enter",
            TripEventKind::ZoneExit => "zone_exit",
            TripEventKind::StopAhead => "stop_ahead",
            TripEventKind::StopReached => "stop_reached",
            TripEventKind::CityReached => "city_reached",
            TripEventKind::Hazard => "hazard",
            TripEventKind::WeatherChange => "weather_change",
            TripEventKind::Inspection => "inspection",
            TripEventKind::GpsCue => "gps_cue",
            TripEventKind::StateCrossing => "state_crossing",
            TripEventKind::TimezoneCrossing => "timezone_crossing",
            TripEventKind::Checkpoint => "checkpoint",
            TripEventKind::TollCharged => "toll_charged",
            TripEventKind::Landmark => "landmark",
            TripEventKind::Billboard => "billboard",
            TripEventKind::Curve => "curve",
            TripEventKind::Lane => "lane",
            TripEventKind::Arrived => "arrived",
        }
    }
}

/// The `**data` keyword payload of `Trip._emit`, one optional slot per key
/// the trip ever sets; the consumer reads the slots it knows.
#[derive(Debug, Clone, Default)]
pub struct TripEventData {
    pub cue: Option<NavigationCue>,
    pub npc_vehicle: Option<TrafficVehicle>,
    pub zone: Option<Zone>,
    pub toll: Option<TollEvent>,
    pub amount: Option<f64>,
    pub deadline_s: Option<f64>,
    pub traffic: Option<TrafficContext>,
    /// A lane change answers this hazard: it is confined to one lane AND
    /// the road has an open lane on this side to take. Both halves, folded
    /// together once at the emitter, so the words and the physics can never
    /// disagree about whether there is somewhere to go.
    pub dodgeable: Option<bool>,
    /// The hazard occupies YOUR lane and no other -- an object, a stopped
    /// car, the vehicle you are closing on. Decides what braking ALONE has
    /// to reach when there is no lane to take instead: a near stop for a
    /// thing sitting in the lane, the moving-hazard safe speed for weather
    /// that spans the road.
    pub in_lane: Option<bool>,
    /// Which neighbouring lane the call found open, the reading
    /// `dodgeable` folds in. Set on every hazard confined to one lane.
    pub open_side: Option<OpenSide>,
    /// The call as lane keeping on full answers it, by changing lanes into
    /// `open_side` (see `passing_hazard_call`): a vehicle ahead, or a thing
    /// sitting in the lane (owner, 2026-10-19: go around, do not brake).
    pub pass_message: Option<SpokenMessage>,
    /// An animal on the road. Braking at highway speed does not save it,
    /// so the automatic braking leaves it alone (owner, 2026-10-19).
    pub animal: Option<bool>,
    /// What braking alone has to reach for this hazard, when less than a
    /// near stop: see `HazardDef::brake_to_mph`.
    pub brake_to_mph: Option<f64>,
    pub name: Option<String>,
    pub weather: Option<WeatherKind>,
    pub curve: Option<RouteCurve>,
    pub advisory_mph: Option<f64>,
    pub ahead_mi: Option<f64>,
    pub lanes: Option<i64>,
    pub category: Option<String>,
    pub explains_limit: Option<bool>,
    pub chain_law: Option<i64>,
    pub chain_law_area: Option<usize>,
    pub planned: Option<bool>,
    pub stop: Option<RoadStop>,
    pub suppress_sound: Option<bool>,
    pub limit_change: Option<bool>,
    /// The posted limit an arrival line names, in mph. Its queued copy is
    /// worth speaking only while that number still governs the truck.
    pub limit_mph: Option<f64>,
    pub advance: Option<bool>,
    pub from_zone: Option<TimeZone>,
    pub to_zone: Option<TimeZone>,
    pub traffic_pressure: Option<TrafficPressure>,
    pub cb_patrol: Option<EnforcementPost>,
    pub key: Option<String>,
    pub context: Option<String>,
    pub evidence: Option<Vec<String>>,
    pub real_traffic_event: Option<TrafficEvent>,
}

#[derive(Debug, Clone)]
pub struct TripEvent {
    pub kind: TripEventKind,
    pub message: SpokenMessage,
    pub data: TripEventData,
}

impl TripEvent {
    /// The normal (full) rendering of the line, what Python tests compared
    /// `event.message` against.
    pub fn text(&self) -> &str {
        &self.message.normal
    }
}

/// The trip milepost where the route passes into another time zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimezoneCrossing {
    pub at_mi: f64,
    pub from_zone: TimeZone,
    pub to_zone: TimeZone,
}

/// A stretch of road with a reduced speed limit.
///
/// `closed_side` ("right"/"left") is the authoritative fact about a coned-off
/// lane: the road can carry a different number of lanes at either end of one
/// work zone, so everything spoken and decided reads the side (through
/// `Trip::closed_lane_at`). `closed_lane` is the nominal index on a
/// two-lane-each-way stretch (0 = right); the two are derived from each other
/// on construction. Congestion zones carry `aadt` and per-direction `lanes`:
/// whether they are active and how slow they run follow the clock, and
/// `limit_mph` on those is the current effective traffic speed.
///
/// `day_factor` is that stretch's traffic today against its annual mean (see
/// [`daily_volume_factor`]). One draw per zone per trip, used both when the
/// zone forms and whenever it is asked whether it applies, so a run is
/// consistent with itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub start_mi: f64,
    pub end_mi: f64,
    pub limit_mph: f64,
    pub reason: String,
    pub closed_lane: Option<i64>,
    pub aadt: Option<f64>,
    pub lanes: i64,
    pub closed_side: Option<String>,
    pub day_factor: f64,
}

impl Zone {
    pub fn new(start_mi: f64, end_mi: f64, limit_mph: f64, reason: &str) -> Self {
        Zone {
            start_mi,
            end_mi,
            limit_mph,
            reason: reason.to_string(),
            closed_lane: None,
            aadt: None,
            lanes: 2,
            closed_side: None,
            day_factor: 1.0,
        }
    }

    /// Python `__post_init__`: derive the side from the lane or the lane
    /// from the side, whichever was given.
    fn derive_closure(mut self) -> Self {
        if self.closed_side.is_none() {
            if let Some(lane) = self.closed_lane {
                self.closed_side = Some(if lane == 0 { "right" } else { "left" }.to_string());
            }
        } else if self.closed_lane.is_none() {
            self.closed_lane = Some(if self.closed_side.as_deref() == Some("right") {
                0
            } else {
                1
            });
        }
        self
    }

    pub fn with_closed_side(mut self, side: Option<&str>) -> Self {
        self.closed_side = side.map(str::to_string);
        self.derive_closure()
    }

    pub fn with_closed_lane(mut self, lane: Option<i64>) -> Self {
        self.closed_lane = lane;
        self.derive_closure()
    }

    pub fn with_congestion(mut self, aadt: Option<f64>, lanes: i64) -> Self {
        self.aadt = aadt;
        self.lanes = lanes;
        self
    }

    /// Python's `day_factor=` keyword on a congestion zone.
    pub fn with_day_factor(mut self, day_factor: f64) -> Self {
        self.day_factor = day_factor;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoadStop {
    pub name: String,
    pub at_mi: f64,
    /// Python `type`.
    pub stop_type: String,
    pub actions: Vec<String>,
    pub services: Vec<String>,
    pub parking: String,
    /// "exit 7" when a real OSM interchange sits here.
    pub exit_label: String,
    /// Surveyed truck-parking spot count; 0 means unsurveyed.
    pub parking_spaces: i64,
    pub vehicle_access: String,
    /// Route mile of the interchange record that serves this stop, when the
    /// bake decided one: matched exactly, never searched near.
    pub interchange_mi: Option<f64>,
}

impl RoadStop {
    pub fn new(name: &str, at_mi: f64, stop_type: &str) -> Self {
        RoadStop {
            name: name.to_string(),
            at_mi,
            stop_type: stop_type.to_string(),
            actions: Vec::new(),
            services: Vec::new(),
            parking: "unknown".to_string(),
            exit_label: String::new(),
            parking_spaces: 0,
            vehicle_access: DEFAULT_VEHICLE_ACCESS.to_string(),
            interchange_mi: None,
        }
    }

    pub fn accessible_to(&self, bobtail: bool) -> bool {
        vehicle_access_allows(&self.vehicle_access, bobtail)
    }

    /// Identity of this stop on this route, for tracking rather than speech:
    /// names repeat constantly, so anything that remembers *which* stop has
    /// to key on the milepost too.
    pub fn key(&self) -> String {
        format!("{}:{}", fmt_f(self.at_mi, 2), self.name)
    }

    /// The speakable name back out of a key, for a plan whose stop is gone.
    pub fn name_from_key(key: &str) -> String {
        key.split_once(':')
            .map(|(_, n)| n)
            .unwrap_or(key)
            .to_string()
    }

    pub fn label(&self) -> &'static str {
        STOP_TYPE_LABELS
            .iter()
            .find(|(k, _)| *k == self.stop_type)
            .map(|(_, v)| *v)
            .unwrap_or("stop")
    }

    /// Drop the type prefix when the proper name already carries it
    /// ("cross-dock: Chicago Cross-Dock" -> "Chicago Cross-Dock").
    pub fn spoken_name(&self) -> String {
        typed_name(self.label(), &self.name, ": ")
    }

    /// Whether a driver can buy fuel here.
    pub fn sells_fuel(&self) -> bool {
        self.actions.iter().any(|action| action == "fuel")
    }

    /// Whether a driver could expect pumps here, so the stop's fuel verdict
    /// is worth speaking: the fuel types, and truck parking, whose lots are
    /// often named "Truck Stop" (issue #272, "The 115 Truck Stop"). Nobody
    /// pulls into a public rest area, a repair shop or a weigh station for
    /// diesel, so saying "No fuel." at every one would be noise.
    pub fn fuel_expected(&self) -> bool {
        matches!(
            self.stop_type.as_str(),
            "truck_stop" | "travel_center" | "fuel_station" | "service_plaza" | "truck_parking"
        )
    }

    /// The fuel verdict the stop callout speaks: whether the stop sells fuel
    /// where a driver could expect it to, and nothing elsewhere.
    pub fn fuel_callout(&self) -> Option<bool> {
        self.fuel_expected().then(|| self.sells_fuel())
    }

    pub fn parking_text(&self) -> String {
        let text = match self.parking.as_str() {
            "confirmed" => "confirmed truck parking",
            "likely" => "",
            "limited" => "limited truck parking",
            "unknown" => "parking not verified",
            "none" => "no truck parking",
            _ => "parking not verified",
        };
        if !text.is_empty()
            && self.parking_spaces > 0
            && (self.parking == "confirmed" || self.parking == "limited")
        {
            return format!("{text}, {} spaces", self.parking_spaces);
        }
        text.to_string()
    }
}

/// A simulated nearby road user that can affect traffic flow (the legacy
/// runtime surface `TrafficVehicle` mirrors).
#[derive(Debug, Clone, PartialEq)]
pub struct NPCVehicle {
    pub key: String,
    pub position_mi: f64,
    pub speed_mph: f64,
    pub target_speed_mph: f64,
    pub relative_lane: i64,
    pub behavior: String,
    pub length_mi: f64,
    pub lane: i64,
}

impl NPCVehicle {
    pub fn new(
        key: &str,
        position_mi: f64,
        speed_mph: f64,
        target_speed_mph: f64,
        relative_lane: i64,
        behavior: &str,
    ) -> Self {
        NPCVehicle {
            key: key.to_string(),
            position_mi,
            speed_mph,
            target_speed_mph,
            relative_lane,
            behavior: behavior.to_string(),
            length_mi: 0.25,
            lane: 0,
        }
    }

    pub fn at_mi(&self) -> f64 {
        self.position_mi
    }

    pub fn end_mi(&self) -> f64 {
        self.position_mi + self.length_mi
    }

    pub fn lane_text(&self) -> &'static str {
        if self.relative_lane < 0 {
            "left lane"
        } else if self.relative_lane > 0 {
            "right lane"
        } else {
            "your lane"
        }
    }

    pub fn reason(&self) -> &'static str {
        match self.behavior.as_str() {
            "steady_truck" => "steady truck traffic",
            "slow_car" => "slow car ahead",
            "merging_vehicle" => "merging traffic",
            "braking_traffic" => "brake lights ahead",
            "passing_vehicle" => "passing traffic",
            _ => "traffic ahead",
        }
    }

    /// The intent a `TrafficVehicle` carries for this behavior (the inverse
    /// of `TrafficVehicle::behavior`).
    pub fn intent(&self) -> &'static str {
        match self.behavior.as_str() {
            "steady_truck" => "cruising",
            "slow_car" => "following",
            "merging_vehicle" => "merging",
            "braking_traffic" => "braking",
            "passing_vehicle" => "passing",
            _ => "cruising",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrafficContext {
    pub lead: TrafficVehicle,
    pub gap_mi: f64,
    pub closing_mph: f64,
}

impl TrafficContext {
    pub fn gap_seconds(&self) -> f64 {
        let speed = 1.0_f64.max(self.lead.speed_mph);
        self.gap_mi / speed * 3600.0
    }
}

/// A short stretch where merging or exiting needs extra spacing.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficPressure {
    pub start_mi: f64,
    pub end_mi: f64,
    pub kind: String,
    pub direction: String,
    pub intensity: f64,
    pub target_speed_mph: f64,
    pub reason: String,
}

pub fn traffic_pressure_key(pressure: &TrafficPressure) -> String {
    format!(
        "{}:{}:{}:{}",
        pressure.kind,
        fmt_f(pressure.start_mi, 3),
        fmt_f(pressure.end_mi, 3),
        pressure.reason
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct TollCharge {
    pub event: TollEvent,
    pub amount: f64,
}

impl TollCharge {
    pub fn name(&self) -> &str {
        &self.event.name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NavigationCue {
    pub key: String,
    pub kind: String,
    pub at_mi: f64,
    pub text: String,
    pub near_text: String,
    /// Speed carried unformatted so display code can render it in the
    /// player's chosen units. Only traffic cues set this.
    pub speed_mph: Option<f64>,
    /// Optional local-road maneuver direction used only for non-speech earcons.
    pub direction: String,
}

impl NavigationCue {
    pub fn new(key: &str, kind: &str, at_mi: f64, text: &str, near_text: &str) -> Self {
        NavigationCue {
            key: key.to_string(),
            kind: kind.to_string(),
            at_mi,
            text: text.to_string(),
            near_text: near_text.to_string(),
            speed_mph: None,
            direction: String::new(),
        }
    }

    pub fn with_speed(mut self, speed_mph: Option<f64>) -> Self {
        self.speed_mph = speed_mph;
        self
    }

    pub fn with_direction(mut self, direction: &str) -> Self {
        self.direction = direction.to_string();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hazard_tables_keep_their_weights() {
        let debris: Vec<_> = HAZARDS
            .iter()
            .filter(|h| {
                [
                    "the ladder",
                    "the lumber",
                    "the mattress",
                    "the boxes",
                    "the tarp",
                    "the debris",
                ]
                .contains(&h.name)
            })
            .collect();
        assert_eq!(debris.len(), 6);
        assert!(debris.iter().all(|h| h.in_lane));
        assert!((debris.iter().map(|h| h.weight).sum::<f64>() - 1.2).abs() < 1e-9);
        assert_eq!(hazard_name("no such hazard"), "it");
        assert!(hazard_is_in_lane("debris on the road"));
    }

    #[test]
    fn road_stop_key_and_parking() {
        let mut stop = RoadStop::new("Love's Travel Stop", 12.345, "travel_center");
        assert_eq!(stop.key(), "12.35:Love's Travel Stop");
        assert_eq!(RoadStop::name_from_key(&stop.key()), "Love's Travel Stop");
        stop.parking = "confirmed".into();
        stop.parking_spaces = 40;
        assert_eq!(stop.parking_text(), "confirmed truck parking, 40 spaces");
        assert_eq!(stop.label(), "travel center");
    }

    #[test]
    fn zone_derives_side_and_lane() {
        let z = Zone::new(0.0, 1.0, 45.0, "construction").with_closed_side(Some("left"));
        assert_eq!(z.closed_lane, Some(1));
        let z = Zone::new(0.0, 1.0, 45.0, "construction").with_closed_lane(Some(0));
        assert_eq!(z.closed_side.as_deref(), Some("right"));
    }

    #[test]
    fn ramp_and_lanes() {
        assert_eq!(ramp_speed_mph(70.0, false), 49.0);
        assert_eq!(ramp_speed_mph(70.0, true), 60.0);
        assert!((acceleration_lane_mi(67.5, 0.0) * 5280.0 - 1515.0).abs() < 1e-9);
        assert!(truck_merge_speed_mph(70.0, 0.0, 0.25) < 70.0);
        assert_eq!(
            congestion_limit_mph(0.8, 70.0).map(|v| (v * 10.0).round() / 10.0),
            Some(58.0)
        );
    }
}
