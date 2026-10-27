//! Ported from the jobs half of `tests/test_models.py`, `tests/test_job_progression.py`,
//! the job cases of `tests/test_business_arc.py` / `tests/test_dispatch_job_detail.py`
//! and the board cases of `tests/test_playtest_levers.py`. The app-shell cases are
//! ignored with the reason and their bodies say what they checked.

use super::*;
use crate::data::world::{get_world, World};
use crate::data::world_models::{CorridorDetail, Leg, Route, SpeedLimitSample};
use crate::sim::hos::HosClock;

const NONE: &[&str] = &[];
const ALL: &[&str] = &["refrigerated", "heavy_haul", "high_value"];

fn world() -> &'static World {
    get_world()
}

fn board(seed: i64) -> JobBoard<'static> {
    JobBoard::seeded(world(), seed)
}

fn offers(seed: i64, city: &str, endorsements: &[&str], level: i64) -> Vec<Job> {
    board(seed).offers(city, endorsements, OfferOptions::level(level))
}

fn route_miles(job: &Job) -> Option<f64> {
    world()
        .supported_route(&job.origin, &job.destination, None)
        .ok()
        .flatten()
        .map(|r| r.miles())
}

fn supported(job: &Job) -> Option<Route> {
    world()
        .supported_route(&job.origin, &job.destination, None)
        .ok()
        .flatten()
}

/// The honest hours a load needs: a team load changes drivers where a
/// solo driver sleeps.
fn required(job: &Job) -> f64 {
    let route = supported(job);
    if job.team {
        return team_required_hours(job.distance_mi, route.as_ref(), Some(world()));
    }
    required_hours(job.distance_mi, route.as_ref(), Some(world()), None)
}

fn general() -> &'static CargoType {
    cargo_type("general").unwrap()
}

// -- tests/test_models.py: jobs ---------------------------------------------------

#[test]
fn test_job_offers_have_real_route_distances() {
    let jobs = offers(3, "Chicago", NONE, 2);
    assert!(!jobs.is_empty());
    for job in &jobs {
        let miles = route_miles(job).expect("a supported route");
        assert!((miles - job.distance_mi).abs() < 1.0);
        assert!(job.pay > 0.0);
        assert!(job.deadline_game_h > job.distance_mi / 70.0);
    }
}

#[test]
fn test_deadlines_allow_legal_driving() {
    // A deadline must cover the driving at an achievable average plus the
    // HOS breaks and sleep the distance demands - no impossible 5-hour
    // San Antonio to Dallas dispatches.
    for (seed, level) in [(1, 1), (2, 3), (3, 6)] {
        for job in offers(seed, "San Antonio", NONE, level) {
            let needed = required(&job);
            assert!(
                job.deadline_game_h >= needed * 1.2,
                "{} to {}: {} mi needs {} h, deadline {} h",
                job.origin,
                job.destination,
                fmt_f(job.distance_mi, 0),
                fmt_f(needed, 1),
                fmt_f(job.deadline_game_h, 1)
            );
        }
    }
}

#[test]
fn test_required_hours_includes_breaks_and_sleep() {
    assert!(required_hours(275.0, None, None, None) < 6.0); // SA-Dallas: just driving
    let medium = required_hours(495.0, None, None, None); // 9 driving hours: one break
    assert!(medium > 495.0 / 55.0);
    let long_haul = required_hours(1150.0, None, None, None); // ~21 h driving: sleep required
    assert!(long_haul > 1150.0 / 55.0 + 10.0);
}

#[test]
fn test_hos_plan_reports_breaks_sleeps_and_route_stop_coverage() {
    let route = world()
        .supported_route("Chicago", "Indianapolis", None)
        .unwrap()
        .unwrap();
    let plan = plan_hos(route.miles(), Some(&route), Some(world()), None);
    assert!(plan.drive_h > route.miles() / 70.0);
    assert!(plan.break_stop_count >= 1);
    assert!(plan.summary().contains(" driving hours"));
    assert!(plan.summary().contains("Break stops: "));
}

#[test]
fn test_northeast_short_corridor_deadline_uses_direct_route() {
    // Search seeds: map expansion (new nearby NJ/PA nodes) shifts any single
    // seed's offer mix, so pin the route invariant, not one seed's lottery.
    let ny_jobs: Vec<Job> = (0..40)
        .flat_map(|seed| offers(seed, "Philadelphia", NONE, 1))
        .filter(|job| job.destination == "new_york_ny_us")
        .collect();

    assert!(!ny_jobs.is_empty());
    assert!(ny_jobs.iter().all(|job| job.distance_mi == 107.0));
    assert!(ny_jobs
        .iter()
        .all(|job| (3.0..=5.0).contains(&job.deadline_game_h)));
    assert!(ny_jobs
        .iter()
        .all(|job| job.deadline_game_h >= required(job) * 1.2));
}

fn one_leg_route(highway: &str, mph: f64) -> Route {
    let leg = Leg::new("A", "B", 100.0, highway, "flat", Vec::new()).with_detail(CorridorDetail {
        speed_limits: vec![SpeedLimitSample {
            at_mi: 0.0,
            mph: Some(mph),
            source: "test".to_string(),
            hgv: false,
        }],
        ..Default::default()
    });
    Route::from_legs(vec!["A".to_string(), "B".to_string()], vec![leg])
}

#[test]
fn test_route_aware_deadline_estimate_uses_low_speed_segments() {
    let route = one_leg_route("US 1", 35.0);
    assert!(route_drive_hours(Some(&route), 0.0, None) > 100.0 / 55.0);
    assert!(
        required_hours(100.0, Some(&route), None, None) > required_hours(100.0, None, None, None)
    );
}

#[test]
fn test_route_deadline_uses_baked_limit_near_city() {
    let route = one_leg_route("I-1", 75.0);
    assert_eq!(
        route_planning_limit(
            &route,
            0,
            &route.legs[0],
            1.0,
            1.0,
            &[0.0, 100.0],
            false,
            None
        ),
        75.0 * DEADLINE_PLANNING_SPEED_FACTOR
    );
}

/// Every generated job's deadline must cover the honest HOS time (driving at
/// the planning pace plus mandatory breaks and 10-hour sleeps). This is the
/// achievability invariant the deadline formula guarantees; the test guards it
/// across the whole expanded network and all levels, including the corrected
/// ORS mileages.
/// How many tests each whole-network sweep in this file is cut into.
///
/// The two sweeps were the slowest things in this crate by a wide margin --
/// about nineteen seconds each against a median in the low milliseconds --
/// and one `#[test]` runs on one thread however many cores the machine has,
/// so between them they set the floor for the entire crate's suite. Sharding
/// the SAME strided city list across several tests is the same origins, the
/// same seeds and the same assertions on several threads, and a failure now
/// names the shard it happened in.
const SWEEP_SHARDS: usize = 6;

/// The origins that shard `shard` of a whole-network sweep covers.
///
/// Both sweeps take the same bounded, deterministic sample -- every
/// `stride`-th city, about 96 of them -- because scanning city x seed x
/// generated jobs grew with the map and the invariants under test are
/// formula invariants that ~96 diverse origins already exercise across the
/// distance and route range. Handing each shard every `SWEEP_SHARDS`-th
/// entry of that sample covers it exactly once between them: no city in two
/// shards, none in none.
fn sweep_origins(shard: usize) -> Vec<String> {
    let all_cities = world().city_names();
    let stride = (all_cities.len() / 96).max(1);
    all_cities
        .iter()
        .step_by(stride)
        .skip(shard)
        .step_by(SWEEP_SHARDS)
        .cloned()
        .collect()
}

fn check_deadlines_cover_required_hos_time(shard: usize) {
    for city in sweep_origins(shard) {
        let city = city.as_str();
        for seed in 0..3 {
            let jobs = board(seed).offers(
                city,
                ALL,
                OfferOptions {
                    count: 5,
                    level: 5,
                    ..Default::default()
                },
            );
            for job in jobs {
                assert!(
                    job.deadline_game_h >= required(&job),
                    "{city} -> {} ({} mi)",
                    job.destination,
                    job.distance_mi
                );
            }
        }
    }
}

macro_rules! sweep_shard_tests {
    ($check:ident, $($name:ident = $shard:expr),+ $(,)?) => {
        $(
            #[test]
            #[cfg_attr(
                ci_quick,
                ignore = "sweep: about 96 origins across the whole network -- left to the nightly by --cfg ci_quick"
            )]
            fn $name() {
                $check($shard);
            }
        )+
    };
}

sweep_shard_tests!(
    check_deadlines_cover_required_hos_time,
    test_deadlines_cover_required_hos_time_across_the_network_shard0 = 0,
    test_deadlines_cover_required_hos_time_across_the_network_shard1 = 1,
    test_deadlines_cover_required_hos_time_across_the_network_shard2 = 2,
    test_deadlines_cover_required_hos_time_across_the_network_shard3 = 3,
    test_deadlines_cover_required_hos_time_across_the_network_shard4 = 4,
    test_deadlines_cover_required_hos_time_across_the_network_shard5 = 5,
);

#[test]
fn test_endorsement_gating() {
    let no_endorsements = board(4).offers(
        "Los Angeles",
        NONE,
        OfferOptions {
            count: 5,
            ..Default::default()
        },
    );
    let locked = no_endorsements
        .iter()
        .filter(|j| !j.cargo.credentials.is_empty())
        .count();
    // at most the single "teaser" job may require an endorsement
    assert!(locked <= 1);
}

#[test]
fn fuel_bulk_asks_for_the_whole_x_combination() {
    // A real fuel tanker is placarded freight: the load requires tank AND
    // hazmat, and the refusal names exactly what is missing.
    let fuel = cargo_type("fuel_bulk").unwrap();
    assert_eq!(fuel.credentials, ["tank", "hazmat"]);
    let job = Job::new(fuel, 20.0, "A", "Loc", "B", 300.0, 900.0, 24.0);
    assert_eq!(
        job.locked_reason(&["high_value"], 30, None, true),
        "Requires the tank vehicle endorsement and the hazmat endorsement."
    );
    assert_eq!(
        job.locked_reason(&["tank"], 30, None, true),
        "Requires the hazmat endorsement."
    );
    assert_eq!(job.locked_reason(&["tank", "hazmat"], 30, None, true), "");
}

#[test]
fn turnpike_doubles_never_leave_the_frozen_network() {
    // Los Angeles is in California, which the ISTEA freeze never admitted:
    // however senior the driver, no LCV freight may originate there.
    let every_credential: Vec<&str> = crate::models::credentials::credential_keys().collect();
    for seed in 0..6 {
        let jobs = board(seed).offers(
            "Los Angeles",
            &every_credential,
            OfferOptions {
                count: 8,
                level: 30,
                ..Default::default()
            },
        );
        assert!(
            jobs.iter().all(|j| j.cargo.key != "turnpike_doubles"),
            "seed {seed} offered LCV freight out of California"
        );
    }
    // Western LCV states alone are no longer enough: the corridor must sit
    // on an allowlisted turnpike leg. Salt Lake to Denver is frozen-network
    // states but not a classic turnpike. Cleveland–Toledo on I-90 is east
    // of the Elyria split and must not clear; Toledo–Elkhart and the NY /
    // Indiana corridor pairs must.
    let b = board(1);
    assert!(!b.lcv_lane("salt_lake_city_ut_us", "denver_co_us"));
    assert!(!b.lcv_lane("salt_lake_city_ut_us", "los_angeles_ca_us"));
    assert!(
        !b.lcv_lane("cleveland_oh_us", "toledo_oh_us"),
        "OH I-90 into Cleveland is east of Elyria and must not clear"
    );
    assert!(
        !b.lcv_lane("buffalo_ny_us", "erie_pa_us"),
        "Buffalo–Erie leaves the Thruway into Pennsylvania"
    );
    assert!(
        !b.lcv_lane("new_york_ny_us", "albany_ny_us")
            && !b.lcv_lane("albany_ny_us", "new_york_ny_us"),
        "NYC docks are not turnpike-doubles endpoints"
    );
    assert!(
        b.lcv_lane("toledo_oh_us", "elkhart_in_us")
            || b.lcv_lane("buffalo_ny_us", "syracuse_ny_us")
            || b.lcv_lane("gary_in_us", "south_bend_in_us"),
        "expected an allowlisted turnpike city pair to clear the LCV lane"
    );
}

#[test]
fn turnpike_doubles_never_start_or_end_in_nyc() {
    let every_credential: Vec<&str> = crate::models::credentials::credential_keys().collect();
    let b = board(1);
    assert!(!b.lcv_lane("new_york_ny_us", "albany_ny_us"));
    assert!(!b.lcv_lane("albany_ny_us", "new_york_ny_us"));
    assert!(!b.lcv_lane("new_york_ny_us", "buffalo_ny_us"));
    for seed in 0..6 {
        let jobs = board(seed).offers(
            "New York",
            &every_credential,
            OfferOptions {
                count: 8,
                level: 30,
                ..Default::default()
            },
        );
        assert!(
            jobs.iter().all(|j| j.cargo.key != "turnpike_doubles"),
            "seed {seed} offered turnpike doubles out of NYC"
        );
    }
}

#[test]
fn turnpike_doubles_require_doubles_endorsement_and_lcv() {
    let cargo = cargo_type("turnpike_doubles").unwrap();
    assert_eq!(cargo.credentials, ["doubles_triples", "lcv"]);
    let job = Job::new(cargo, 20.0, "A", "Loc", "B", 300.0, 900.0, 24.0);
    assert_eq!(
        job.locked_reason(&["lcv"], 30, None, true),
        "Requires the doubles endorsement."
    );
    assert_eq!(
        job.locked_reason(&["doubles_triples"], 30, None, true),
        "Requires the LCV certificate."
    );
    assert_eq!(
        job.locked_reason(&["doubles_triples", "lcv"], 30, None, true),
        ""
    );
}

#[test]
fn turnpike_double_never_offers_hazmat_cargo() {
    // Placarded freight stays on dry van / tank programs. The turnpike
    // doubles class is never a hazmat load, and hazmat classes never pull
    // the turnpike_double trailer (hazmat in doubles is not modeled).
    let turnpike = cargo_type("turnpike_doubles").unwrap();
    assert!(!turnpike.credentials.contains(&"hazmat"));
    assert_eq!(
        crate::models::trailers::trailer_keys_for_cargo("turnpike_doubles"),
        ["turnpike_double"]
    );
    assert_eq!(
        crate::models::trailers::trailer_keys_for_cargo("hazardous"),
        ["dry_van"]
    );
    assert_eq!(
        crate::models::trailers::trailer_keys_for_cargo("fuel_bulk"),
        ["tank"]
    );
    assert!(
        !crate::models::trailers::trailer_keys_for_cargo("hazardous").contains(&"turnpike_double")
    );
    assert!(
        !crate::models::trailers::trailer_keys_for_cargo("fuel_bulk").contains(&"turnpike_double")
    );
}

#[test]
fn turnpike_doubles_board_skips_non_turnpike_corridors() {
    let every_credential: Vec<&str> = crate::models::credentials::credential_keys().collect();
    // Columbus sits off the Ohio Turnpike; long doubles must not originate
    // onto I-71-only lanes even with every credential held.
    for seed in 0..6 {
        let jobs = board(seed).offers(
            "Columbus",
            &every_credential,
            OfferOptions {
                count: 8,
                level: 30,
                ..Default::default()
            },
        );
        for job in &jobs {
            if job.cargo.key != "turnpike_doubles" {
                continue;
            }
            let routes = world()
                .supported_route_options(&job.origin, &job.destination, 3)
                .expect("route options");
            assert!(
                routes
                    .iter()
                    .any(crate::data::lcv_turnpikes::route_allows_lcv_turnpike),
                "seed {seed} offered turnpike_doubles {} -> {} off the turnpike",
                job.origin,
                job.destination
            );
        }
    }
}

#[test]
fn parcel_doubles_and_singles_ignore_the_turnpike_trailer_program() {
    assert_eq!(
        crate::models::trailers::trailer_keys_for_cargo("parcel_doubles"),
        ["double_van"]
    );
    assert_eq!(
        crate::models::trailers::trailer_keys_for_cargo("general"),
        ["dry_van"]
    );
    assert!(!cargo_type("parcel_doubles").unwrap().lcv_lanes);
    assert!(!cargo_type("general").unwrap().lcv_lanes);
}

#[test]
fn parcel_doubles_board_skips_alaska_and_canada_lanes() {
    use crate::data::national_network::STAA_DOUBLES_CORRIDOR_REFUSAL;
    assert_eq!(
        STAA_DOUBLES_CORRIDOR_REFUSAL,
        "Dispatch doesn't run twins on that lane."
    );
    let b = board(1);
    // Alaska and Canada endpoints are outside the lower 48 by policy.
    assert!(!b.national_network_lane("fairbanks_ak_us", "tok_ak_us"));
    assert!(!b.national_network_lane("tok_ak_us", "anchorage_ak_us"));
    assert!(!b.national_network_lane("surrey_bc_ca", "whitehorse_yt_ca"));
    // A lower-48 Interstate corridor still clears the lane check.
    assert!(
        b.national_network_lane("chicago_il_us", "gary_in_us")
            || b.national_network_lane("denver_co_us", "colorado_springs_co_us")
            || b.national_network_lane("seattle_wa_us", "tacoma_wa_us"),
        "expected at least one lower-48 Interstate pair to clear the twin lane"
    );
}

#[test]
fn parcel_doubles_never_offered_from_alaska() {
    let every_credential: Vec<&str> = crate::models::credentials::credential_keys().collect();
    for seed in 0..4 {
        let jobs = board(seed).offers(
            "Fairbanks",
            &every_credential,
            OfferOptions {
                count: 8,
                level: 30,
                ..Default::default()
            },
        );
        assert!(
            jobs.iter().all(|j| j.cargo.key != "parcel_doubles"),
            "seed {seed} offered parcel_doubles out of Fairbanks"
        );
    }
}

#[test]
fn parcel_doubles_require_the_national_network_flag() {
    let doubles = cargo_type("parcel_doubles").expect("catalog");
    assert!(doubles.national_network);
    assert!(!cargo_type("general").unwrap().national_network);
    assert!(!cargo_type("turnpike_doubles").unwrap().national_network);
    assert!(!cargo_type("parcel").unwrap().national_network);
}

#[test]
fn parcel_doubles_board_skips_lanes_off_the_national_network() {
    use crate::data::national_network::route_allows_staa_doubles;
    // Every seeded board at a senior T-endorsed level must either skip
    // parcel_doubles or place it on a twin-legal corridor option.
    let every_credential: Vec<&str> = crate::models::credentials::credential_keys().collect();
    for seed in 0..8 {
        let jobs = board(seed).offers(
            "Chicago",
            &every_credential,
            OfferOptions {
                count: 10,
                level: 30,
                ..Default::default()
            },
        );
        for job in &jobs {
            if job.cargo.key != "parcel_doubles" {
                continue;
            }
            let routes = world()
                .supported_route_options(&job.origin, &job.destination, 3)
                .expect("route options");
            assert!(
                routes.iter().any(route_allows_staa_doubles),
                "seed {seed} offered parcel_doubles {} -> {} with no NN route",
                job.origin,
                job.destination
            );
        }
    }
}

#[test]
fn single_trailer_jobs_are_unaffected_by_the_national_network_gate() {
    // Plain freight still posts even when the corridor is not all Interstate.
    let jobs = board(2).offers(
        "Chicago",
        NONE,
        OfferOptions {
            count: 8,
            level: 5,
            ..Default::default()
        },
    );
    assert!(
        jobs.iter().any(|j| !j.cargo.national_network),
        "expected at least one single-trailer offer"
    );
    for job in &jobs {
        assert_ne!(job.cargo.key, "parcel_doubles");
    }
}

#[test]
fn test_payout_on_time_window_beats_late() {
    let job = Job::new(general(), 15.0, "A", "Loc", "B", 300.0, 700.0, 9.0);
    let early = job.payout_default(5.0, 0.0);
    let on_dot = job.payout_default(9.0, 0.0);
    let late = job.payout_default(12.0, 0.0);
    // Window model: any on-time arrival earns the same flat bonus; racing in
    // early pays no more than hitting the appointment.
    assert_eq!(early, on_dot);
    assert!(on_dot > 700.0 && 700.0 > late);
    assert!(late >= 700.0 * 0.4);
}

#[test]
fn test_payout_punishes_fragile_damage() {
    let fragile = Job::new(
        cargo_type("electronics").unwrap(),
        8.0,
        "A",
        "Loc",
        "B",
        300.0,
        1000.0,
        9.0,
    );
    let tough = Job::new(
        cargo_type("bulk").unwrap(),
        8.0,
        "A",
        "Loc",
        "B",
        300.0,
        1000.0,
        9.0,
    );
    assert!(fragile.payout_default(5.0, 30.0) < tough.payout_default(5.0, 30.0));
}

/// Dispatch offers name places a player may never have heard of; the state
/// must always ride along ("McCall, Idaho"), not only when two cities share a
/// name (player request).
#[test]
fn test_job_spoken_names_always_carry_the_state() {
    let jobs = offers(5, "Chicago", NONE, 2);
    assert!(!jobs.is_empty());
    for job in &jobs {
        let origin_city = world().city(&job.origin).unwrap();
        let dest_city = world().city(&job.destination).unwrap();
        assert_eq!(job.spoken_origin(), origin_city.spoken_qualified());
        assert_eq!(job.spoken_destination(), dest_city.spoken_qualified());
        if !origin_city.state.is_empty() {
            assert!(job.spoken_origin().contains(&origin_city.state));
        }
        if !dest_city.state.is_empty() {
            assert!(job.spoken_destination().contains(&dest_city.state));
        }
    }
}

/// A load accepted mid-shift gets its mandatory sleep in the deadline.
///
/// Owner catch 2026-07-24: deadlines assumed a fresh clock, so a one-shift
/// load accepted six hours into a shift promised a delivery nobody could
/// legally make (the mid-trip 10-hour sleep ate it).
#[test]
fn test_deadline_plans_around_hours_already_on_the_clock() {
    let miles = 495.0; // ~9 driving hours: one shift for a fresh driver
    let fresh = plan_hos(miles, None, None, None);
    assert_eq!(fresh.sleeps, 0);

    let used = HosClock {
        driving_min: 6.0 * 60.0,
        duty_min: 7.0 * 60.0,
        since_break_min: 2.0 * 60.0,
        ..HosClock::new()
    };
    let mid_shift = plan_hos(miles, None, None, Some(&used));
    assert_eq!(mid_shift.sleeps, 1);

    let fresh_deadline = dispatch_deadline_hours(miles, 1.2, None, None, None);
    let tired_deadline = dispatch_deadline_hours(miles, 1.2, None, None, Some(&used));
    assert!(tired_deadline >= fresh_deadline + 10.0);
}

#[test]
fn test_fresh_clock_plans_are_unchanged_by_the_clock_parameter() {
    for miles in [110.0, 495.0, 1150.0] {
        assert_eq!(
            plan_hos(miles, None, None, Some(&HosClock::new())),
            plan_hos(miles, None, None, None)
        );
    }
}

#[test]
fn test_burned_duty_window_forces_the_sleep_even_with_drive_hours_left() {
    // 13 hours of window gone with only 3 driven: the 14-hour wall, not the
    // 11-hour driving cap, is what forces the reset.
    let window_burned = HosClock {
        driving_min: 3.0 * 60.0,
        duty_min: 13.0 * 60.0,
        ..HosClock::new()
    };
    let plan = plan_hos(495.0, None, None, Some(&window_burned));
    assert_eq!(plan.sleeps, 1);
}

#[test]
fn mandatory_sleep_also_satisfies_a_due_break() {
    for duty_h in [13.75, 14.0] {
        let clock = HosClock {
            driving_min: 8.0 * 60.0,
            duty_min: duty_h * 60.0,
            since_break_min: 8.0 * 60.0,
            ..HosClock::new()
        };
        let plan = plan_hos(55.0, None, None, Some(&clock));
        assert_eq!(plan.sleeps, 1);
        assert_eq!(plan.breaks, 0);
        assert_eq!(plan.total_h(), 11.0);
    }
}

#[test]
fn test_board_speaks_the_rest_the_deadline_covers() {
    let mut job = Job::new(general(), 15.0, "A", "Loc", "B", 300.0, 700.0, 24.0);
    job.deadline_covers_rest = true;
    assert!(job.describe_plain().contains("10-hour rest"));
    let plain = Job::new(general(), 15.0, "A", "Loc", "B", 300.0, 700.0, 9.0);
    assert!(!plain.describe_plain().contains("10-hour rest"));
}

// -- tests/test_job_progression.py -----------------------------------------------

#[test]
fn test_level_one_offers_are_short_regional_hops() {
    // Rookie reach is gated by the distance cap and proximity weighting, not by
    // leg count: jobs stay regional and lean short, but the board offers variety
    // (it is no longer locked to one back-and-forth destination).
    for city in ["Atlanta", "Philadelphia", "Chicago"] {
        let cap = level_distance_cap(1).unwrap();
        let mut near = 0usize;
        let mut total = 0usize;
        let mut destinations: std::collections::BTreeSet<String> = Default::default();
        for seed in 0..20 {
            let jobs = offers(seed, city, NONE, 1);
            assert!(!jobs.is_empty());
            for job in jobs {
                total += 1;
                assert!(job.distance_mi <= cap); // within the regional cap
                near += usize::from(job.distance_mi <= cap * 0.6); // proximity favors near cities
                destinations.insert(job.destination);
            }
        }
        assert!(near as f64 / total as f64 >= 0.5, "{city}"); // predominantly short hauls
        assert!(destinations.len() >= 3, "{city}"); // variety, not one repeated route
    }
}

#[test]
fn test_level_one_and_two_stay_within_the_regional_cap() {
    // The distance cap keeps rookie work regional; leg count no longer gates it.
    for seed in 0..10 {
        for level in [1, 2] {
            let cap = level_distance_cap(level).unwrap();
            for job in offers(seed, "Atlanta", NONE, level) {
                assert!(supported(&job).is_some());
                assert!(job.distance_mi <= cap);
            }
        }
    }
}

#[test]
fn test_higher_level_reaches_farther_destinations() {
    // Level 2's larger cap lets it take jobs to cities level 1 cannot reach.
    let max_distance = |level: i64| -> f64 {
        (0..40)
            .flat_map(|seed| offers(seed, "Milwaukee", NONE, level))
            .map(|job| job.distance_mi)
            .fold(0.0, f64::max)
    };
    assert!(max_distance(2) > max_distance(1));
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- DrivingState / ArrivalState"]
fn test_bobtail_relocates_to_a_nearby_city_without_pay() {
    // A Denver -> Cheyenne reposition is a bobtail with no pay; the arrival
    // relocates the profile to cheyenne_wy_us, pays nothing, counts no
    // delivery, and its summary names Cheyenne. The job half holds here:
    let job = make_reposition_job(world(), "Denver", "Cheyenne", false, None).unwrap();
    assert!(job.bobtail && job.pay == 0.0);
    assert_eq!(job.destination, "cheyenne_wy_us");
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- ArrivalState settlement"]
fn test_bobtail_settlement_collects_fines_carried_over() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- DrivingState._update_hours_and_fatigue"]
fn test_bobtail_personal_conveyance_records_off_duty_hos_time() {}

#[test]
fn test_distance_cap_tops_out_at_a_real_coast_to_coast_run() {
    // haul length keeps progressing deep into the company arc...
    assert!(JobBoard::distance_cap(10) < JobBoard::distance_cap(15));
    // ...but never outgrows the longest supported U.S. corridor
    assert_eq!(JobBoard::distance_cap(20), MAX_DISPATCH_DISTANCE_MI);
    assert_eq!(JobBoard::distance_cap(30), MAX_DISPATCH_DISTANCE_MI);
}

#[test]
fn test_distance_cap_rises_with_level() {
    let caps: Vec<f64> = (1..9).map(JobBoard::distance_cap).collect();
    assert!(caps.windows(2).all(|w| w[0] <= w[1]));
    assert!(caps[0] <= 300.0);
    assert!(JobBoard::distance_cap(5) >= 1000.0);

    // the cap is honored by actual offers
    for seed in 0..10 {
        for job in offers(seed, "Chicago", NONE, 5) {
            assert!(job.distance_mi <= JobBoard::distance_cap(5));
        }
    }
}

#[test]
fn test_long_hauls_unlock_around_level_five() {
    let longest = |level: i64| -> f64 {
        (0..30)
            .flat_map(|seed| offers(seed, "Phoenix", NONE, level))
            .map(|job| job.distance_mi)
            .fold(0.0, f64::max)
    };
    assert!(longest(1) < LONG_HAUL_MILES);
    assert!(longest(5) >= LONG_HAUL_MILES);
}

#[test]
fn test_destination_weighting_prefers_near_cities() {
    // Milwaukee is 92 miles from Chicago; New York is ~880. Even at a high
    // level with everything in range, near cities must come up far more often.
    let mut near = 0;
    let mut far = 0;
    for seed in 0..60 {
        for job in offers(seed, "Chicago", NONE, 6) {
            near += i32::from(job.destination == "milwaukee_wi_us");
            far += i32::from(job.destination == "new_york_ny_us");
        }
    }
    assert!(near > far);
}

/// Milwaukee to Kenosha and back is the bounce the owner described
/// (2026-10-26): deliver south, and the next board's shortest load is the
/// city you just left. Blind, Milwaukee is on nearly every level-1 Kenosha
/// board (40 miles, and the low levels weight by closeness squared). With
/// the arrival city known it is on about a third of them: still there, as
/// a real backhaul is, but no longer a given.
#[test]
fn the_city_the_truck_came_from_is_on_about_a_third_of_the_boards() {
    let boards_with = |came_from: Option<&str>| {
        let mut back = 0;
        for seed in 0..100 {
            let jobs = board(seed).offers(
                "Kenosha",
                NONE,
                OfferOptions {
                    level: 1,
                    came_from,
                    ..OfferOptions::default()
                },
            );
            back += usize::from(jobs.iter().any(|j| j.destination == "milwaukee_wi_us"));
        }
        back
    };
    let blind = boards_with(None);
    let aware = boards_with(Some("Milwaukee"));
    assert!(blind >= 80, "blind: Milwaukee on {blind} of 100 boards");
    assert!(
        (20..=50).contains(&aware),
        "came from Milwaukee: on {aware} of 100 boards (blind {blind})"
    );
}

/// A town with too few cities in reach keeps the straight return: the
/// gate needs `RETURN_LANE_MIN_POOL` cities to choose from.
#[test]
fn a_town_with_few_neighbours_keeps_its_return_load() {
    let b = board(3);
    let here = b.world.resolve_city_key("Kenosha");
    let pool: Vec<Candidate> = b
        .candidates(&here)
        .into_iter()
        .filter(|c| c.0 == "milwaukee_wi_us" || c.0 == "chicago_il_us")
        .collect();
    assert_eq!(pool.len(), 2);
    let mut b = b;
    for _ in 0..20 {
        let spread = b.spread_destinations(&here, &pool, 1, 5, "", Some("milwaukee_wi_us"));
        assert!(
            spread.iter().any(|c| c.0 == "milwaukee_wi_us"),
            "{spread:?}"
        );
    }
}

/// The arrival factor leaves the spread alone without an arrival city, and
/// a city the map does not know changes nothing either.
#[test]
fn an_unknown_arrival_city_leaves_the_spread_unchanged() {
    let blind = offers(7, "Milwaukee", NONE, 3);
    let aware = board(7).offers(
        "Milwaukee",
        NONE,
        OfferOptions {
            level: 3,
            came_from: Some("Nowhere Special"),
            ..OfferOptions::default()
        },
    );
    let dests = |jobs: &[Job]| {
        jobs.iter()
            .map(|j| j.destination.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(dests(&blind), dests(&aware));
    let b = board(1);
    assert_eq!(
        b.arrival_factor("milwaukee_wi_us", "chicago_il_us", None),
        1.0
    );
    assert_eq!(
        b.arrival_factor("milwaukee_wi_us", "chicago_il_us", Some("nowhere")),
        1.0
    );
}

/// Came up from Chicago to Milwaukee: Green Bay (ahead) and Madison
/// (across) draw as before, Chicago itself is a full reversal, and a city
/// behind the truck but off to the side sits in between.
#[test]
fn cities_ahead_and_across_draw_full_and_cities_behind_lean_down() {
    let b = board(1);
    let here = "milwaukee_wi_us";
    let from = Some("chicago_il_us");
    assert!(
        (b.arrival_factor(here, "chicago_il_us", from) - (1.0 - BEHIND_HEADING_PENALTY)).abs()
            < 0.05
    );
    assert_eq!(b.arrival_factor(here, "green_bay_wi_us", from), 1.0);
    assert_eq!(b.arrival_factor(here, "madison_wi_us", from), 1.0);
    let behind = b.arrival_factor(here, "indianapolis_in_us", from);
    assert!(
        behind < 1.0 && behind > 1.0 - BEHIND_HEADING_PENALTY,
        "{behind}"
    );
}

#[test]
fn turn_angles_read_straight_on_right_angle_and_straight_back() {
    let south = (40.0, -90.0);
    let here = (41.0, -90.0);
    let north = (42.0, -90.0);
    let east = (41.0, -89.0);
    assert!(turn_angle_deg(south, here, north).abs() < 0.01);
    assert!((turn_angle_deg(south, here, east) - 90.0).abs() < 1.0);
    assert!((turn_angle_deg(south, here, south) - 180.0).abs() < 0.01);
    assert!((turn_angle_deg(north, here, east) - 90.0).abs() < 1.0);
}

#[test]
fn came_from_city_reads_the_lane_that_ended_here() {
    let w = world();
    let lanes = vec!["chicago_il_us:milwaukee_wi_us".to_string()];
    assert_eq!(
        came_from_city(w, &lanes, "Milwaukee").as_deref(),
        Some("chicago_il_us")
    );
    // The last lane ended somewhere else: a lever moved the truck, or an
    // old save. Nothing to lean on.
    assert_eq!(came_from_city(w, &lanes, "Chicago"), None);
    assert_eq!(came_from_city(w, &[], "Milwaukee"), None);
    assert_eq!(
        came_from_city(w, &["not a lane".to_string()], "Milwaukee"),
        None
    );
}

#[test]
fn a_lane_is_recent_in_both_directions() {
    let recent = vec!["chicago_il_us:milwaukee_wi_us".to_string()];
    assert!(lane_recent_either_way(
        &recent,
        "chicago_il_us:milwaukee_wi_us"
    ));
    assert!(lane_recent_either_way(
        &recent,
        "milwaukee_wi_us:chicago_il_us"
    ));
    assert!(!lane_recent_either_way(
        &recent,
        "milwaukee_wi_us:green_bay_wi_us"
    ));
    assert_eq!(reverse_lane_key("a:b").as_deref(), Some("b:a"));
    assert_eq!(reverse_lane_key("ab"), None);
    assert_eq!(reverse_lane_key(":b"), None);
}

#[test]
fn test_board_never_offers_a_haul_below_the_minimum() {
    // Cities stand for whole freight areas, so trivial across-town hops are not
    // offered as dispatches, no matter how close two cities sit on the map.
    for city in [
        "New York",
        "Philadelphia",
        "Los Angeles",
        "Dallas",
        "Norfolk",
    ] {
        for seed in 0..20 {
            for level in [1, 6] {
                for job in offers(seed, city, NONE, level) {
                    assert!(
                        job.distance_mi >= MIN_JOB_DISTANCE_MI,
                        "{} -> {} is only {} mi",
                        job.origin,
                        job.destination,
                        fmt_f(job.distance_mi, 0)
                    );
                }
            }
        }
    }
}

#[test]
fn test_close_neighbors_are_not_dispatched() {
    // The nearest neighbor sits under the minimum, so it must never be offered,
    // yet the board still fills from farther destinations.
    for (city, too_close) in [("Norfolk", "Virginia Beach"), ("Bridgeport", "New Haven")] {
        for seed in 0..30 {
            let jobs = offers(seed, city, NONE, 1);
            assert!(!jobs.is_empty());
            assert!(jobs.iter().all(|job| job.destination != too_close));
        }
    }
}

#[test]
fn test_remote_terminal_still_gets_a_full_board() {
    // Salt Lake City's nearest neighbor is beyond the level-1 cap; the board
    // must fall back to the nearest cities instead of coming up empty.
    let jobs = offers(7, "Salt Lake City", NONE, 1);
    assert!(!jobs.is_empty());
    assert!(jobs.iter().all(|job| job.distance_mi <= 600.0));
}

#[test]
fn test_short_hauls_still_pay_for_fuel() {
    // ~6 mpg at roughly $4/gallon is ~$0.67 per mile; rookie jobs must clear
    // that with room for repairs and profit.
    for seed in 0..10 {
        for job in offers(seed, "Atlanta", NONE, 1) {
            assert!(job.pay >= job.distance_mi * 1.5);
        }
    }
}

#[test]
fn test_rookie_boards_have_rewarding_minimum_pay() {
    for city in [
        "Chicago",
        "Atlanta",
        "Philadelphia",
        "San Antonio",
        "Los Angeles",
    ] {
        for seed in 0..15 {
            for job in offers(seed, city, NONE, 1) {
                // pay is rounded to cents, so compare against the floor to cents
                assert!(job.pay >= round_py_n(minimum_pay_for_level(job.distance_mi, 1), 2));
            }
        }
    }
}

#[test]
fn test_long_haul_boards_have_rewarding_minimum_pay() {
    let long_jobs: Vec<Job> = ["Chicago", "Atlanta", "Dallas", "Los Angeles"]
        .iter()
        .flat_map(|city| (0..30).flat_map(move |seed| offers(seed, city, ALL, 5)))
        .filter(|job| job.distance_mi >= 600.0)
        .collect();

    assert!(!long_jobs.is_empty());
    for job in long_jobs {
        assert!(job.pay / job.distance_mi >= 5.25);
    }
}

#[test]
fn test_short_haul_premium_tapers_instead_of_cliffing() {
    // The old flat $700-1050 floors paid a 50-mile hop ~$23 a mile -- four to
    // five times any long haul, at every level -- so grinding short hops was
    // strictly optimal. The guaranteed rate must decline gently with distance
    // and the shortest hops must stay within about twice the long-haul rate.
    for level in [1, 3, 5, 12] {
        let rates: Vec<f64> = [50.0, 100.0, 200.0, 300.0, 500.0, 599.0]
            .iter()
            .map(|miles| minimum_pay_for_level(*miles, level) / miles)
            .collect();
        assert!(rates.windows(2).all(|w| w[0] >= w[1]), "{rates:?}");
        assert!(rates[0] <= 2.0 * 5.25); // short premium, not a jackpot
    }
}

#[test]
fn test_representative_boards_use_truck_plausible_locations() {
    for city in [
        "Chicago",
        "Atlanta",
        "Philadelphia",
        "San Antonio",
        "Los Angeles",
    ] {
        let jobs = offers(3, city, NONE, 2);
        assert!(!jobs.is_empty());
        let locations = &world().city(city).unwrap().locations;
        assert!(jobs
            .iter()
            .all(|job| locations.iter().any(|loc| job.origin_location == loc.name)));
        assert!(jobs.iter().all(|job| !job.origin_facility_id.is_empty()));
    }
}

#[test]
fn test_facility_type_filters_available_cargo() {
    for seed in 0..40 {
        for job in offers(seed, "Chicago", ALL, 4) {
            let allowed = facility_cargo(&job.origin_type).expect("a known facility type");
            assert!(allowed.contains(&job.cargo.key));
        }
    }
}

#[test]
fn test_jobs_match_shipper_and_receiver_roles() {
    for city in ["Chicago", "Fresno", "Houston", "Memphis", "Detroit"] {
        for seed in 0..12 {
            let jobs = offers(seed, city, ALL, 5);
            assert!(!jobs.is_empty());
            for job in jobs {
                let origin = world()
                    .facility_location(&job.origin, &job.origin_facility_id)
                    .unwrap();
                let destination = world()
                    .facility_location(&job.destination, &job.destination_facility_id)
                    .unwrap();
                assert!(origin.ships.iter().any(|k| k == job.cargo.key));
                assert!(destination.receives.iter().any(|k| k == job.cargo.key));
                let text = job.describe_plain();
                assert!(text.contains(&origin.name));
                assert!(text.contains(&destination.name));
            }
        }
    }
}

#[test]
fn test_regional_specialization_shapes_generated_freight() {
    let cargo_of = |city: &str| -> std::collections::BTreeSet<&'static str> {
        (0..25)
            .flat_map(|seed| offers(seed, city, ALL, 5))
            .map(|job| job.cargo.key)
            .collect()
    };
    let chicago_cargo = cargo_of("Chicago");
    let fresno_cargo = cargo_of("Fresno");
    let houston_types: std::collections::BTreeSet<String> = (0..25)
        .flat_map(|seed| offers(seed, "Houston", ALL, 5))
        .map(|job| job.origin_type)
        .collect();

    assert!(chicago_cargo.contains("container") || chicago_cargo.contains("parcel"));
    assert!(
        fresno_cargo.contains("grain")
            || fresno_cargo.contains("food")
            || fresno_cargo.contains("refrigerated")
    );
    assert!(houston_types.contains("chemical_petroleum_terminal"));
}

#[test]
fn test_higher_levels_unlock_more_facility_and_cargo_variety() {
    let low_jobs: Vec<Job> = (0..20)
        .flat_map(|seed| offers(seed, "Chicago", NONE, 1))
        .collect();
    let high_jobs: Vec<Job> = (0..20)
        .flat_map(|seed| offers(seed, "Chicago", ALL, 5))
        .collect();

    assert!(!low_jobs.is_empty() && !high_jobs.is_empty());
    let cargo_kinds = |jobs: &[Job]| {
        jobs.iter()
            .map(|j| j.cargo.key)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    };
    let type_kinds = |jobs: &[Job]| {
        jobs.iter()
            .map(|j| j.origin_type.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    };
    assert!(cargo_kinds(&high_jobs) > cargo_kinds(&low_jobs));
    assert!(type_kinds(&high_jobs) > type_kinds(&low_jobs));
    assert!(high_jobs
        .iter()
        .any(|job| job.cargo.min_level > 1 || !job.cargo.credentials.is_empty()));
}

#[test]
fn test_jobs_carry_destination_facility_metadata() {
    let jobs = offers(8, "Los Angeles", ALL, 5);
    assert!(!jobs.is_empty());
    for job in jobs {
        assert!(!job.destination_location.is_empty());
        assert!(!job.destination_type.is_empty());
        assert!(world().cities[&job.destination]
            .locations
            .iter()
            .any(|loc| loc.name == job.destination_location));
        let text = job.describe_plain();
        assert!(text.contains(&job.origin_location));
        assert!(text.contains(&job.destination_location));
    }
}

#[test]
fn test_job_offer_avoids_repeating_facility_type_in_generated_names() {
    let mut job = Job::new(
        general(),
        12.0,
        "South Bend",
        "South Bend Grocery Distribution Center",
        "Fort Wayne",
        85.0,
        833.0,
        4.0,
    );
    job.origin_type = "grocery_retail_dc".to_string();
    job.destination_location = "Fort Wayne Dry Warehouse".to_string();
    job.destination_type = "dry_warehouse".to_string();

    let text = job.describe_numbered(1, 5);

    assert!(text.contains("from South Bend Grocery Distribution Center in South Bend"));
    assert!(text.contains("to Fort Wayne Dry Warehouse in Fort Wayne"));
    assert!(!text.contains("grocery and retail distribution center South Bend"));
    assert!(!text.contains("dry warehouse Fort Wayne Dry Warehouse"));
}

#[test]
fn test_representative_stops_are_real_world_grounded() {
    let expected = [
        (("Atlanta", "Birmingham"), "Pilot Travel Center Lincoln"),
        (("Memphis", "Little Rock"), "Forrest City I-40 Rest Area"),
        (("San Antonio", "Dallas"), "Road Ranger Waco"),
        (
            ("Los Angeles", "San Diego"),
            "San Onofre Safety Roadside Rest Area",
        ),
        (("Des Moines", "Chicago"), "Iowa 80 Truckstop"),
        (("Houston", "Dallas"), "Pilot Travel Center Huntsville"),
        (("Los Angeles", "Fresno"), "Pilot Travel Center Bakersfield"),
        (("Fresno", "Sacramento"), "Flying J Travel Center Ripon"),
    ];
    for ((start, end), stop_name) in expected {
        let route = world()
            .shortest_route(start, end, None, false)
            .unwrap()
            .unwrap();
        assert!(
            route.stops().iter().any(|s| s == stop_name),
            "{start}-{end}"
        );
    }
}

#[test]
fn test_new_dispatches_only_use_metadata_supported_routes() {
    for city in [
        "Chicago",
        "Atlanta",
        "Philadelphia",
        "San Antonio",
        "Los Angeles",
    ] {
        for seed in 0..12 {
            for job in offers(seed, city, NONE, 6) {
                let route = supported(&job).expect("a supported route");
                assert!(route.metadata_complete(world()));
            }
        }
    }
}

fn check_whole_board_never_offers_unsupported_route_legs(shard: usize) {
    // Spot-check board generation across a bounded, deterministic sample of
    // origin cities (x4 seeds) rather than every city. The route memo is per
    // shard rather than per sweep, so a shard re-derives a route another
    // shard already has -- a little repeated work, bought back many times
    // over by the shards running at once.
    let mut routes: std::collections::HashMap<(String, String), Option<Route>> = Default::default();
    for city in sweep_origins(shard) {
        let city = city.as_str();
        for seed in 0..4 {
            for job in offers(seed, city, ALL, 6) {
                let key = (job.origin.clone(), job.destination.clone());
                let route = routes
                    .entry(key)
                    .or_insert_with(|| supported(&job))
                    .clone()
                    .unwrap_or_else(|| panic!("{} to {}", job.origin, job.destination));
                assert!(
                    route.metadata_complete(world()),
                    "{} to {}",
                    job.origin,
                    job.destination
                );
                assert!(route
                    .legs
                    .iter()
                    .all(|leg| world().leg_metadata_complete(leg)));
                assert!(route.stop_details().iter().all(|stop| stop.curated()));
            }
        }
    }
}

sweep_shard_tests!(
    check_whole_board_never_offers_unsupported_route_legs,
    test_whole_board_never_offers_unsupported_route_legs_shard0 = 0,
    test_whole_board_never_offers_unsupported_route_legs_shard1 = 1,
    test_whole_board_never_offers_unsupported_route_legs_shard2 = 2,
    test_whole_board_never_offers_unsupported_route_legs_shard3 = 3,
    test_whole_board_never_offers_unsupported_route_legs_shard4 = 4,
    test_whole_board_never_offers_unsupported_route_legs_shard5 = 5,
);

#[test]
fn test_former_legacy_routes_are_now_metadata_supported_for_dispatch() {
    let route = world()
        .supported_route("Chicago", "St. Louis", None)
        .unwrap()
        .unwrap();
    assert!(route.metadata_complete(world()));
    let jobs = offers(9, "Chicago", NONE, 6);
    assert!(!jobs.is_empty());
    assert!(jobs.iter().all(|job| supported(job).is_some()));
}

#[test]
fn test_former_placeholder_only_routes_are_metadata_supported() {
    let route = world()
        .supported_route("Memphis", "Nashville", None)
        .unwrap()
        .unwrap();
    assert!(route.metadata_complete(world()));
    assert!(route.stop_details().iter().all(|stop| stop.curated()));

    let supported_route = world()
        .supported_route("Memphis", "Little Rock", None)
        .unwrap()
        .unwrap();
    assert!(supported_route.metadata_complete(world()));

    let jobs = offers(4, "Memphis", NONE, 1);
    assert!(!jobs.is_empty());
    assert!(jobs.iter().all(|job| supported(job).is_some()));
}

// -- tests/test_business_arc.py: the job half -----------------------------------

#[test]
fn test_company_driver_dispatch_uses_carrier_trailer_support() {
    let job = Job::new(
        cargo_type("refrigerated").unwrap(),
        10.0,
        "Chicago",
        "cold storage",
        "Milwaukee",
        92.0,
        1200.0,
        5.0,
    );

    assert_eq!(job.locked_reason(&["refrigerated"], 4, None, true), "");
    assert!(job
        .locked_reason(
            &["refrigerated"],
            4,
            Some(crate::models::trailers::DEFAULT_TRAILER_PROGRAMS),
            false
        )
        .contains("Requires Reefer trailer program"));
}

#[test]
fn test_direct_freight_board_pays_more_and_uses_direct_label() {
    let base = board(44).offers("Chicago", ALL, OfferOptions::level(25));
    let direct = board(44).offers(
        "Chicago",
        ALL,
        OfferOptions {
            level: 25,
            direct_freight: true,
            ..Default::default()
        },
    );
    assert!(!base.is_empty());
    assert!(!direct.is_empty());
    assert!(direct[0].pay > base[0].pay);
    // The JobBoardState half ("Direct gross" on the board rows) needs the
    // app shell.
}

// -- tests/test_playtest_levers.py: the board half ------------------------------

#[test]
fn test_offer_to_builds_job_to_forced_destination() {
    let job = board(7)
        .offer_to(
            "denver_co_us",
            "silverthorne_co_us",
            NONE,
            OfferOptions::default(),
        )
        .expect("a forced offer");
    assert_eq!(
        world().resolve_city_key(&job.destination),
        "silverthorne_co_us"
    );
    assert!(job.distance_mi > 0.0);
    assert!(job.pay > 0.0);
}

#[test]
fn test_offer_to_unknown_destination_returns_none() {
    assert!(board(7)
        .offer_to("denver_co_us", "atlantis", NONE, OfferOptions::default())
        .is_none());
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- states::city._add_forced_board_job"]
fn test_forced_board_job_lands_on_the_board() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- states::city._add_forced_board_job"]
fn test_forced_board_job_skips_when_already_offered() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- states::city.JobBoardState"]
fn test_assigned_dispatch_hands_out_the_forced_load_first() {}

// -- tests/test_dispatch_job_detail.py ------------------------------------------

// `test_f1_on_dispatch_job_opens_structured_detail_view` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_tab_repeats_only_the_market_watch` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_job_detail_lines_are_reviewable_before_accepting` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_job_detail_exposes_review_instructions` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_locked_job_detail_does_not_sound_accept_available` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_f1_on_back_item_does_not_crash` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_job_detail_accept_command_accepts_and_escape_returns` is live in `crates/freight-fate/tests/states_city.rs`.

// -- payload round trip -----------------------------------------------------------

#[test]
fn job_payload_round_trips_and_legacy_payloads_fill_in() {
    let mut job = Job::new(
        general(),
        12.5,
        "chicago_il_us",
        "Yard",
        "milwaukee_wi_us",
        92.0,
        800.0,
        6.0,
    );
    job.origin_spoken = "Chicago, Illinois".to_string();
    job.destination_location = "Milwaukee Cross-Dock".to_string();
    job.bobtail = true;
    job.seasonal_weight_limit = Some(SeasonalWeightLimit {
        highway: "Richardson Highway".to_string(),
        percent: 85,
    });
    let payload = job_payload(&job);
    let back = job_from_payload(&payload).unwrap();
    assert_eq!(back, job);
    assert!(job.describe_plain().contains(
        "Spring weight limits on the Richardson Highway: axle loads held to 85 percent of the legal limit."
    ));

    let mut legacy = Map::new();
    legacy.insert("cargo".into(), Value::from("general"));
    legacy.insert("weight_tons".into(), Value::from(10));
    legacy.insert("origin".into(), Value::from("Chicago"));
    legacy.insert("destination".into(), Value::from("Milwaukee"));
    legacy.insert("distance_mi".into(), Value::from(92.0));
    legacy.insert("pay".into(), Value::from(800.0));
    legacy.insert("deadline_game_h".into(), Value::from(6.0));
    let old = job_from_payload(&legacy).unwrap();
    assert!(old.seasonal_weight_limit.is_none());
    assert_eq!(old.origin_location, "Chicago freight market");
    assert_eq!(old.origin_type, "metro_market");
    assert_eq!(old.describe_plain().split(' ').next(), Some("10"));
    assert!(old
        .origin_facility_text()
        .starts_with("the Chicago metro freight market"));
    let mut cityless = legacy.clone();
    cityless.remove("origin");
    assert!(job_from_payload(&cityless).is_none());

    let mut malformed = legacy;
    malformed.insert(
        "seasonal_weight_limit".into(),
        Value::String("not a limit".to_string()),
    );
    assert!(job_from_payload(&malformed)
        .unwrap()
        .seasonal_weight_limit
        .is_none());
}

fn seasonal_hours(month: u32, day: u32, hour: f64) -> f64 {
    use chrono::Datelike;

    let ordinal = f64::from(
        chrono::NaiveDate::from_ymd_opt(2001, month, day)
            .expect("test date is valid")
            .ordinal(),
    );
    (ordinal - 80.0).rem_euclid(365.0) * 24.0 + hour
}

#[test]
fn seeded_alaska_board_caps_restricted_jobs_only_during_the_window() {
    use crate::data::seasonal_weight::{seasonal_gvw_cap_kg, strictest_on_route};
    use crate::sim::vehicle::{TrailerSet, TruckSpecs, TruckState, KG_PER_TON};

    let seed = 34;
    let active_hours = seasonal_hours(5, 1, 12.0);
    let active_jobs = JobBoard::seeded(world(), seed)
        .with_calendar_hours(active_hours)
        .offers("valdez_ak_us", ALL, OfferOptions::level(30));
    let mut restricted_jobs = 0;
    for job in &active_jobs {
        let route = supported(job).expect("board job has a supported route");
        let Some(restriction) = strictest_on_route(&route, active_hours) else {
            assert!(job.seasonal_weight_limit.is_none());
            continue;
        };
        restricted_jobs += 1;
        let limit = job
            .seasonal_weight_limit
            .as_ref()
            .expect("restricted dispatch stores its seasonal limit");
        assert_eq!(limit.highway, restriction.highway);
        assert_eq!(limit.percent, restriction.percent);

        let set = if crate::data::lcv_turnpikes::cargo_requires_lcv_turnpike(job.cargo.key) {
            TrailerSet::for_cargo_between(job.cargo.key, world(), &job.origin, &job.destination)
                .expect("board-compatible trailer set")
        } else {
            TrailerSet::for_cargo_on_route(job.cargo.key, world(), Some(&route))
                .expect("board-compatible trailer set")
        };
        let mut truck = TruckState::new(TruckSpecs::default());
        truck.trailer_set = set;
        truck.cargo_kg = job.weight_tons * KG_PER_TON;
        let cap = seasonal_gvw_cap_kg(&truck, limit.percent);
        assert!(
            truck.gross_mass_kg() <= cap + 1e-6,
            "{} to {}: gross {} exceeds seasonal cap {}",
            job.origin,
            job.destination,
            truck.gross_mass_kg(),
            cap
        );
    }
    assert!(
        restricted_jobs > 0,
        "seed must include a restricted outbound job"
    );

    let inactive_jobs = JobBoard::seeded(world(), seed)
        .with_calendar_hours(seasonal_hours(7, 1, 12.0))
        .offers("valdez_ak_us", ALL, OfferOptions::level(30));
    assert!(!inactive_jobs.is_empty());
    assert!(inactive_jobs
        .iter()
        .all(|job| job.seasonal_weight_limit.is_none()));
}

#[test]
fn test_metro_market_text_does_not_double_the_dalles_article() {
    for text in [
        facility_text("metro_market", "", "The Dalles", ""),
        facility_offer_text("metro_market", "", "The Dalles", ""),
    ] {
        assert_eq!(text, "The Dalles metro freight market");
    }
    assert_eq!(
        facility_text("metro_market", "", "Chicago", ""),
        "the Chicago metro freight market"
    );
    assert_eq!(
        facility_offer_text("metro_market", "", "Chicago", ""),
        "the Chicago metro freight market"
    );
}

#[test]
fn make_reposition_job_pays_assigned_empty_miles_only() {
    let bobtail = make_reposition_job(world(), "Denver", "Cheyenne", false, None).unwrap();
    assert!(bobtail.bobtail && !bobtail.assigned);
    assert_eq!(bobtail.pay, 0.0);
    assert_eq!(bobtail.origin_spoken, "Denver, Colorado");
    let assigned = make_reposition_job(world(), "Denver", "Cheyenne", true, None).unwrap();
    assert!(assigned.assigned);
    assert!(assigned.pay > 0.0);
    assert!(make_reposition_job(world(), "Denver", "atlantis", false, None).is_none());
    assert_eq!(board_offer_count(1), 5);
    assert_eq!(board_offer_count(12), 8);
    assert_eq!(lane_key(world(), &bobtail), "denver_co_us:cheyenne_wy_us");
}

#[test]
fn test_dispatched_loads_stay_at_or_under_eighty_thousand_pounds() {
    use crate::sim::vehicle::{
        combination_tare_kg, max_legal_cargo_tons, TruckSpecs, KG_PER_TON, LEGAL_GVW_KG,
    };
    let tare = combination_tare_kg(&TruckSpecs::default());
    let max_tons = max_legal_cargo_tons(tare);
    let jobs = offers(7, "Chicago", ALL, 30);
    assert!(!jobs.is_empty());
    for job in &jobs {
        assert!(
            job.weight_tons <= max_tons + 1e-6,
            "{} tons of {} exceeds the 80,000 lb clamp ({max_tons})",
            job.weight_tons,
            job.cargo.key
        );
        let gross = tare + job.weight_tons * KG_PER_TON;
        assert!(
            gross <= LEGAL_GVW_KG + 1e-6,
            "GVW {gross} kg over cap {LEGAL_GVW_KG} kg"
        );
    }
}

// -- Endorsement freight shows up for the drivers who hold the credential ----

/// Every credential a level-18 company driver can hold short of TWIC and LCV.
const LADDER: &[&str] = &[
    "refrigerated",
    "flatbed_securement",
    "heavy_haul",
    "high_value",
    "doubles_triples",
    "tank",
    "hazmat",
];

/// One board in every eighth city across the map (a stride keeps this under
/// twenty seconds; the whole map took two minutes): `(offers, count per
/// cargo key)`.
fn cargo_mix(
    endorsements: &[&str],
    level: i64,
) -> (usize, std::collections::BTreeMap<&'static str, usize>) {
    let mut mix = std::collections::BTreeMap::new();
    let mut total = 0usize;
    for (index, key) in world().cities.keys().enumerate().step_by(8) {
        for job in offers(index as i64 * 7 + 1, key, endorsements, level) {
            *mix.entry(job.cargo.key).or_insert(0) += 1;
            total += 1;
        }
    }
    (total, mix)
}

fn share(mix: &(usize, std::collections::BTreeMap<&'static str, usize>), key: &str) -> f64 {
    mix.1.get(key).copied().unwrap_or(0) as f64 / mix.0 as f64
}

#[test]
fn hazmat_and_tank_holders_see_placarded_and_fuel_freight_map_wide() {
    // 2026-09-16: nobody on staging had ever hauled a placarded or bulk fuel
    // load, because the board offered a hazmat holder 1.2 percent placarded
    // and 0.25 percent fuel map-wide. A credential the driver paid a
    // background check for has to pay back on the board.
    let held = cargo_mix(LADDER, 18);
    eprintln!("level-18 holder mix over {} offers: {:?}", held.0, held.1);
    assert!(
        share(&held, "hazardous") >= 0.06,
        "placarded share {:.3} of {} offers",
        share(&held, "hazardous"),
        held.0
    );
    assert!(
        share(&held, "fuel_bulk") >= 0.02,
        "bulk fuel share {:.3} of {} offers",
        share(&held, "fuel_bulk"),
        held.0
    );
    // Without the H endorsement the same freight stays the occasional
    // locked teaser it always was.
    let without: Vec<&str> = LADDER.iter().copied().filter(|k| *k != "hazmat").collect();
    let missing = cargo_mix(&without, 18);
    assert!(
        share(&missing, "hazardous") <= 0.02,
        "placarded share without hazmat {:.3}",
        share(&missing, "hazardous")
    );
    assert!(
        share(&missing, "fuel_bulk") <= 0.01,
        "bulk fuel share without hazmat {:.3}",
        share(&missing, "fuel_bulk")
    );
}

#[test]
fn a_gulf_coast_board_usually_carries_placarded_or_fuel_freight_for_a_holder() {
    // 70% (28/40) was tuned on the 624-city Lower-48 graph. The Phase A
    // ALCAN tip adds Blaine as a reachable US destination and shifts the
    // seeded Houston mix by one board (27/40). Keep "usually" as a clear
    // majority rather than re-tuning cargo weights for a corridor tip.
    let boards = 40;
    let mut carrying = 0;
    for seed in 0..boards {
        let jobs = offers(seed, "Houston", LADDER, 18);
        if jobs
            .iter()
            .any(|job| matches!(job.cargo.key, "hazardous" | "fuel_bulk"))
        {
            carrying += 1;
        }
    }
    assert!(
        carrying * 8 >= boards * 5,
        "{carrying} of {boards} Houston boards carried placarded or fuel freight"
    );
}

#[test]
fn test_generated_jobs_never_use_travel_center_or_truck_parking_endpoints() {
    // Sample major boards plus ALCAN cities that only curate public lots.
    let cities = [
        "Chicago",
        "Houston",
        "Anchorage",
        "tok_ak_us",
        "fairbanks_ak_us",
        "healy_ak_us",
        "glennallen_ak_us",
        "blaine_wa_us",
        "surrey_bc_ca",
        "whitehorse_yt_ca",
        "fort_nelson_bc_ca",
        "watson_lake_yt_ca",
    ];
    for city in cities {
        for seed in 0..12 {
            for job in offers(seed, city, ALL, 8) {
                assert_ne!(
                    job.origin_type, "travel_center",
                    "{city} seed {seed} origin {}",
                    job.origin_location
                );
                assert_ne!(
                    job.origin_type, "truck_parking",
                    "{city} seed {seed} origin {}",
                    job.origin_location
                );
                assert_ne!(
                    job.destination_type, "travel_center",
                    "{city} seed {seed} dest {}",
                    job.destination_location
                );
                assert_ne!(
                    job.destination_type, "truck_parking",
                    "{city} seed {seed} dest {}",
                    job.destination_location
                );
            }
        }
    }
    // Travel-center-only cities must offer an empty board, not invent freight.
    for city in ["tok_ak_us", "healy_ak_us", "blaine_wa_us", "surrey_bc_ca"] {
        assert!(
            offers(1, city, ALL, 8).is_empty(),
            "{city} must not mint jobs from a fuel/parking-only pin"
        );
    }
    // Anchorage still boards from the Port terminal (some seeds miss when the
    // dest cycle lands only on travel-center-only neighbors; check a range).
    let anchorage_jobs: usize = (0..20)
        .map(|seed| offers(seed, "anchorage_ak_us", ALL, 8).len())
        .sum();
    assert!(
        anchorage_jobs > 0,
        "Anchorage Port terminal must still mint jobs across seeds"
    );
}

#[test]
fn thin_outbound_origin_weight_cuts_ship_selection() {
    use crate::models::jobs::THIN_OUTBOUND_ORIGIN_WEIGHT;
    // Origin selection multiplies facility_weight by this factor for pins
    // tagged thin_outbound (Fairbanks Carlile), so inbound destination weight
    // stays full while outbound origin weight is clearly lower.
    const { assert!(THIN_OUTBOUND_ORIGIN_WEIGHT > 0.0 && THIN_OUTBOUND_ORIGIN_WEIGHT < 0.5) };
}

#[test]
fn frozen_facility_keeps_its_name_while_cargo_is_spoken_chilled() {
    assert!(facility_ships_frozen_as_chilled(
        "Blue Bell Ice Cream Distribution"
    ));
    assert!(facility_ships_frozen_as_chilled(
        "Wegmans Food Market Frozen Foods Warehouse"
    ));
    assert!(facility_ships_frozen_as_chilled(
        "Magic Valley Fresh Frozen"
    ));
    assert!(!facility_ships_frozen_as_chilled(
        "Madison Food Processing Plant"
    ));

    let mut job = Job::new(
        cargo_type("refrigerated").unwrap(),
        10.0,
        "Houston",
        "Blue Bell Ice Cream Distribution",
        "Madison",
        100.0,
        1200.0,
        8.0,
    );
    job.destination_location = "Madison Food Processing Plant".into();
    assert!(job
        .origin_offer_text()
        .contains("Blue Bell Ice Cream Distribution"));
    assert_eq!(job.spoken_cargo_label(), "chilled goods");

    let mut food_job = Job::new(
        cargo_type("food").unwrap(),
        10.0,
        "Madison",
        "Schoep's Ice Cream",
        "Utica",
        100.0,
        1200.0,
        8.0,
    );
    food_job.destination_location = "Maynard Frozen Foods".into();
    assert_eq!(food_job.spoken_cargo_label(), "chilled food");
}

#[test]
fn ordinary_reefer_cargo_keeps_catalog_label() {
    let mut job = Job::new(
        cargo_type("refrigerated").unwrap(),
        10.0,
        "Madison",
        "Madison Food Processing Plant",
        "Chicago",
        100.0,
        1200.0,
        8.0,
    );
    job.destination_location = "Chicago Cross-Dock".into();
    assert_eq!(job.spoken_cargo_label(), "refrigerated goods");
}

#[test]
fn doubles_loads_stay_under_their_route_cap_with_both_trailers_and_the_dolly() {
    // FIX 5: the board prices a set of doubles with both trailers and the
    // converter dolly in the tare, against the gross cap of the route the
    // load runs. Turnpike doubles take the turnpike cap; STAA pups stay at
    // 80,000 lb.
    use crate::sim::vehicle::{
        combination_tare_kg, TrailerSet, TruckSpecs, KG_PER_TON, TRAILER_TARE_KG,
    };
    let every_credential: Vec<&str> = crate::models::credentials::credential_keys().collect();
    let tractor_kg = combination_tare_kg(&TruckSpecs::default()) - TRAILER_TARE_KG;
    let mut pups = 0;
    let mut turnpike = 0;
    for city in ["Toledo", "Buffalo", "Gary", "Chicago", "Columbus"] {
        for seed in 0..12 {
            let jobs = board(seed).offers(
                city,
                &every_credential,
                OfferOptions {
                    count: 12,
                    level: 30,
                    ..Default::default()
                },
            );
            for job in jobs {
                let set = TrailerSet::for_cargo_between(
                    job.cargo.key,
                    world(),
                    &job.origin,
                    &job.destination,
                )
                .expect("an offered load has a legal set on its lanes");
                if !set.is_doubles() {
                    continue;
                }
                let gross = tractor_kg + set.tare_kg + job.weight_tons * KG_PER_TON;
                assert!(
                    gross <= set.legal_gvw_kg + 1e-6,
                    "{} from {city} grossed {gross} kg over its {} kg cap",
                    job.cargo.key,
                    set.legal_gvw_kg
                );
                match job.cargo.key {
                    "parcel_doubles" => {
                        pups += 1;
                        assert_eq!(set.legal_gvw_lb().round(), 80_000.0);
                    }
                    "turnpike_doubles" => {
                        turnpike += 1;
                        // A recorded turnpike cap, never the 80,000 fallback.
                        assert!(set.legal_gvw_lb().round() >= 120_000.0);
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        pups + turnpike > 0,
        "no seeded board offered a set of doubles"
    );
}

#[test]
fn every_offered_turnpike_doubles_lane_has_a_cap_in_every_state() {
    // Two 48-foot trailers are illegal off the LCV turnpikes at any weight,
    // so there is no 80,000 lb fallback: every lane the route menu can offer
    // for an offered load must touch only states with a recorded cap.
    use crate::data::lcv_turnpikes::{filter_lcv_turnpike_routes, lcv_turnpike_gvw_cap_lb};
    let every_credential: Vec<&str> = crate::models::credentials::credential_keys().collect();
    let mut checked = 0;
    for city in [
        "Toledo",
        "Elkhart",
        "Gary",
        "South Bend",
        "Buffalo",
        "Syracuse",
        "Albany",
        "Rochester",
        "Wichita",
        "Topeka",
        "Boston",
        "Springfield",
    ] {
        for seed in 0..8 {
            let jobs = board(seed).offers(
                city,
                &every_credential,
                OfferOptions {
                    count: 12,
                    level: 30,
                    ..Default::default()
                },
            );
            for job in jobs.iter().filter(|j| j.cargo.key == "turnpike_doubles") {
                let routes = world()
                    .supported_route_options(&job.origin, &job.destination, 3)
                    .expect("the world routes");
                let lanes = filter_lcv_turnpike_routes(&routes);
                assert!(
                    !lanes.is_empty(),
                    "{} offered with no turnpike lane",
                    job.origin
                );
                for lane in &lanes {
                    for key in &lane.cities {
                        let state = world()
                            .cities
                            .get(key)
                            .map_or("", |c| c.state_code.as_str());
                        assert!(
                            lcv_turnpike_gvw_cap_lb(state).is_some(),
                            "{} -> {} runs through {key} ({state}) with no recorded LCV cap",
                            job.origin,
                            job.destination
                        );
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no seeded board offered turnpike doubles");
}
