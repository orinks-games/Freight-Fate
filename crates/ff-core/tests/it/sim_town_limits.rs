//! The town streets at either end of a leg take the state's district limit
//! where the map read no posting there, never the open-road number of the
//! first posting out of town or the last one carried into it.
//!
//! Found on Marshalltown to Ottumwa (2026-11-23): "Speed limit 65" on the
//! town streets, the speed keeper at 42 mph a quarter mile short of two
//! 20 mph corners, and a rollover at 26 mph.

use crate::sim_support::*;
use ff_core::data::curves::{leg_curves, CurveRecord};
use ff_core::data::street_limits::MAX_PLAUSIBLE_MPH;
use ff_core::data::world_models::{CorridorDetail, Leg, Route, SpeedLimitSample, StateMileage};
use ff_core::sim::limit_gaps::GAP_HOLD_HIGHWAY_MPH;
use ff_core::sim::town_limits::{town_ends, TOWN_BEND_MAX_ADVISORY_MPH};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::URBAN_RADIUS_MI;
use ff_core::sim::vehicle::TruckState;

fn sample(at_mi: f64, mph: Option<f64>) -> SpeedLimitSample {
    SpeedLimitSample {
        at_mi,
        mph,
        source: String::new(),
        hgv: false,
    }
}

fn bend(start_mi: f64, end_mi: f64, advisory_mph: i64, connector: bool) -> CurveRecord {
    CurveRecord {
        start_mi,
        apex_mi: (start_mi + end_mi) / 2.0,
        end_mi,
        direction: 'R',
        advisory_mph,
        min_radius_ft: 80,
        deflection_deg: 90.0,
        connector,
    }
}

fn iowa_leg(speed_limits: Vec<SpeedLimitSample>) -> Leg {
    Leg::new("A", "B", 90.0, "IA-146", "flat", Vec::new()).with_detail(CorridorDetail {
        speed_limits,
        state_miles: vec![StateMileage {
            state: "Iowa".to_string(),
            miles: 90.0,
        }],
        ..Default::default()
    })
}

#[test]
fn test_the_streets_before_the_first_posting_take_the_district_limit() {
    // Marshalltown's shape: 65 from mile 3.3, two 20 mph corners at 0.77.
    let leg = iowa_leg(vec![sample(3.3, Some(65.0)), sample(60.0, Some(55.0))]);
    let ends = town_ends(
        &leg,
        &[bend(0.77, 0.80, 20, false), bend(0.82, 0.86, 20, false)],
    );
    let start = ends.start.clone().expect("a town stretch at the A end");
    assert_eq!((start.from_mi, start.to_mi), (0.0, 0.86));
    // Iowa Code 321.285(2)(a): 20 mph in a business district.
    assert_eq!(start.mph, 20.0);
    assert!(start.source.starts_with("assumed:"), "{}", start.source);
    assert!(start.source.contains("321.285"), "{}", start.source);
    assert_eq!(ends.cap(0.5, 65.0), 20.0);
    assert_eq!(ends.cap(2.0, 65.0), 65.0);
}

#[test]
fn test_a_town_posting_read_on_the_way_in_is_left_alone() {
    let leg = iowa_leg(vec![sample(0.0, Some(55.0)), sample(85.0, Some(35.0))]);
    let ends = town_ends(&leg, &[bend(88.0, 88.1, 20, true)]);
    assert!(ends.end.is_none());
    assert!(
        ends.start.is_none(),
        "posted from mile 0: nothing to assume"
    );
}

#[test]
fn test_a_highway_posting_carried_into_town_stops_at_the_first_town_bend() {
    let leg = iowa_leg(vec![sample(0.0, Some(55.0)), sample(40.0, Some(65.0))]);
    // A bend out on the highway, then the route leaves it for town streets.
    let curves = [
        bend(70.0, 70.2, 20, false),
        bend(87.5, 87.6, 40, true),
        bend(89.0, 89.1, 20, false),
    ];
    let end = town_ends(&leg, &curves)
        .end
        .expect("a town stretch at the B end");
    assert_eq!((end.from_mi, end.to_mi), (87.5, 90.0));
    assert_eq!(end.mph, 20.0);
}

#[test]
fn test_a_long_unposted_start_is_town_only_as_far_as_its_bends() {
    // Pierre to Rapid City's shape: nothing read for 120 miles.
    let leg = iowa_leg(vec![sample(80.0, Some(65.0))]);
    let curves = [bend(1.7, 1.8, 25, false), bend(30.0, 30.1, 25, false)];
    let start = town_ends(&leg, &curves).start.expect("a town stretch");
    assert_eq!(start.to_mi, 1.8, "the radius bounds the town");
}

fn one_leg_trip(leg: &Leg, forward: bool) -> Trip {
    let (from, to) = if forward {
        (leg.a.clone(), leg.b.clone())
    } else {
        (leg.b.clone(), leg.a.clone())
    };
    let route = Route::from_legs(vec![from, to], vec![leg.clone()]);
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    Trip::new(
        route,
        truck,
        weather("heartland", 1),
        TripOptions {
            world: Some(world()),
            ..TripOptions::seeded(1)
        },
    )
}

fn world_leg(a: &str, b: &str) -> Leg {
    world()
        .legs
        .iter()
        .find(|l| l.a == a && l.b == b)
        .map(|l| (**l).clone())
        .unwrap_or_else(|| panic!("no leg {a} to {b}"))
}

#[test]
fn test_the_reported_legs_leave_town_at_the_town_figure() {
    for (a, b) in [
        ("marshalltown_ia_us", "ames_ia_us"),
        ("washington_dc_us", "philadelphia_pa_us"),
        ("abilene_tx_us", "fort_worth_tx_us"),
        ("morgantown_wv_us", "cumberland_md_us"),
    ] {
        let leg = world_leg(a, b);
        let first = leg.speed_limits()[0].clone();
        let trip = one_leg_trip(&leg, true);
        let at_start = trip.corridor_limit_at(0.0);
        assert!(
            at_start <= MAX_PLAUSIBLE_MPH,
            "{a} to {b}: {at_start} at mile 0, first posting {:?} at {}",
            first.mph,
            first.at_mi
        );
        if first.at_mi > 0.0 {
            // Nothing read on the streets: the assumed district limit.
            let source = trip
                .town_limit_source_at(0.0)
                .expect("an assumed town limit");
            assert!(source.contains("assumed"), "{source}");
            let mut mile = 0.0;
            while mile < first.at_mi {
                assert!(
                    trip.corridor_limit_at(mile) <= first.mph.unwrap(),
                    "{a} at {mile}"
                );
                mile += 0.1;
            }
        } else {
            // The town posting the Interstate floor used to drop, read again.
            assert!(trip.town_limit_source_at(0.0).is_none(), "{a}");
            assert_eq!(Some(at_start), first.mph, "{a}");
        }
        // Cruise handed over on the town streets builds to the highway.
        let past = trip.limit_past_town(0.1).expect("a highway past town");
        assert!(past >= GAP_HOLD_HIGHWAY_MPH, "{a}: {past} past town");
    }
}

/// The regression the report asked for, over the whole map: no bend advised
/// 25 or less near a leg end, short of the first posting or past the last
/// highway posting, carries more than a town limit, driven either way.
#[test]
fn test_no_town_corner_carries_an_open_road_limit_at_either_end() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for leg in &world().legs {
        let samples = leg.speed_limits();
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
            continue;
        };
        let curves = leg_curves(&format!("{}:{}", leg.a, leg.b), false);
        let lead_reach = first.at_mi.min(URBAN_RADIUS_MI);
        let tail_from = last.at_mi.max(leg.miles - URBAN_RADIUS_MI);
        let highway_tail = last.mph.is_none_or(|m| m >= GAP_HOLD_HIGHWAY_MPH);
        let corners: Vec<&CurveRecord> = curves
            .iter()
            .filter(|c| c.advisory_mph <= TOWN_BEND_MAX_ADVISORY_MPH)
            .filter(|c| c.start_mi < lead_reach || (highway_tail && c.start_mi >= tail_from))
            .collect();
        if corners.is_empty() {
            continue;
        }
        let ends = town_ends(leg, &curves);
        for corner in corners {
            checked += 1;
            let lead = corner.start_mi < lead_reach;
            // Only the part of the corner the map read nothing for: a corner
            // that runs on past the first posting is posted there.
            let points = [corner.start_mi, corner.apex_mi, corner.end_mi];
            for at in points.into_iter().filter(|at| {
                if lead {
                    *at < first.at_mi
                } else {
                    *at >= tail_from
                }
            }) {
                let capped = ends.cap(at, 999.0);
                if capped > MAX_PLAUSIBLE_MPH {
                    failures.push(format!(
                        "{}:{} bend at {:.2} (advised {}) carries {capped}",
                        leg.a, leg.b, corner.start_mi, corner.advisory_mph
                    ));
                }
            }
        }
    }
    assert!(checked > 0, "the sweep found no town corners at all");
    assert!(
        failures.is_empty(),
        "{} of {checked}:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn test_the_trip_applies_the_town_limit_in_both_directions() {
    let leg = world_leg("marshalltown_ia_us", "ames_ia_us");
    let forward = one_leg_trip(&leg, true);
    let reverse = one_leg_trip(&leg, false);
    let end_mile = reverse.total_miles();
    // Leaving Marshalltown forward, arriving there in reverse: same streets.
    assert_eq!(
        forward.corridor_limit_at(0.1),
        reverse.corridor_limit_at(end_mile - 0.1)
    );
    assert!(forward.corridor_limit_at(0.1) <= MAX_PLAUSIBLE_MPH);
}

#[test]
fn test_canada_and_alaska_town_limits_and_canadian_80_kmh_tail_screening() {
    use ff_core::sim::town_limits::town_mph;

    let (ak_mph, ak_src) = town_mph("Alaska");
    assert!(
        (ak_mph - 20.0).abs() < 0.01,
        "Alaska statutory business limit: {ak_mph}"
    );
    assert!(ak_src.contains("13 AAC 02.275"), "{ak_src}");

    let (bc_mph, bc_src) = town_mph("British Columbia");
    assert!(
        (bc_mph - 31.07).abs() < 0.05,
        "BC 50 km/h statutory limit: {bc_mph}"
    );
    assert!(bc_src.contains("Motor Vehicle Act"), "{bc_src}");

    let (yt_mph, _) = town_mph("Yukon");
    assert!(
        (yt_mph - 31.07).abs() < 0.05,
        "Yukon 50 km/h limit: {yt_mph}"
    );

    // An 80 km/h (~49.7 mph) Canadian highway posting carried to the far end
    // of a leg must still be screened into the town stretch on arrival.
    let leg = Leg::new("A", "B", 20.0, "BC-97", "flat", Vec::new()).with_detail(CorridorDetail {
        speed_limits: vec![sample(1.0, Some(49.7))],
        state_miles: vec![StateMileage {
            state: "British Columbia".to_string(),
            miles: 20.0,
        }],
        ..Default::default()
    });
    let ends = town_ends(&leg, &[bend(19.0, 19.6, 20, true)]);
    let end = ends
        .end
        .expect("80 km/h Canadian highway tail should be screened into town");
    assert!((end.mph - 31.07).abs() < 0.05);
}
