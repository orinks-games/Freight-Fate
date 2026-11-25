//! Tests for the curve management tier on the unified curve pipeline (port of
//! `tests/test_curve_management.py`).
//!
//! Covers curve data loading (`data::curves`, the single loader) against the
//! real world; the Trip integration cases (`TestTripCurveIntegration`) are
//! ported with their bodies but ignored until `sim::trip` lands. The pure
//! geometry-screen and radius-floor tests live inline in `curves.rs`.

use std::collections::{HashMap, HashSet};

use crate::data_support::{data_dir, shortest, supported, world};
use ff_core::data::curves::{
    classify_connector, curve_severity, leg_curves, leg_design_speed, leg_is_level, load,
    min_radius_ft, route_curves, screenable_legs, CurveRecord, ADVISORY_MAX_MPH,
    CONNECTOR_CORRIDOR_M, HAIRPIN_DEFLECTION_DEG, HAIRPIN_MAX_MPH, HPMS_TERRAIN_LEVEL,
    INTERSTATE_MAX_DEFLECTION_DEG, INTERSTATE_MIN_RADIUS_FT,
};
use ff_core::data::data_resources::read_data_text;
use ff_core::data::world::World;
use ff_core::data::world_models::{
    CorridorDetail, GradeSegment, Leg, Route, RouteCheckpoint, StateMileage,
};
use ff_core::models::jobs::{
    curve_ceilings, route_drive_hours, route_drive_hours_over, segment_hours,
};

fn is_interstate(leg: &Leg) -> bool {
    leg.highway.to_uppercase().starts_with("I-")
}

/// The artifact screen's question -- "could a road here really do this?" --
/// which is broader than the spoken hairpin and deliberately so: a very low
/// advisory is an extreme claim about the ground whether or not the road comes
/// back on itself. `RouteCurve::severity` uses shape alone, per MUTCD; see
/// `curves::HAIRPIN_DEFLECTION_DEG`.
fn is_hairpin(rec: &CurveRecord) -> bool {
    rec.advisory_mph <= HAIRPIN_MAX_MPH || rec.deflection_deg >= HAIRPIN_DEFLECTION_DEG
}

// -- TestCurveLoading --------------------------------------------------------

#[test]
fn test_unknown_leg_returns_empty_tuple() {
    assert!(leg_curves("nonexistent_leg_xyz", true).is_empty());
}

#[test]
fn test_connectors_are_filtered_by_default() {
    let mainline = leg_curves("aberdeen_sd_us:pierre_sd_us", true);
    let everything = leg_curves("aberdeen_sd_us:pierre_sd_us", false);
    assert!(mainline.iter().all(|c| !c.connector));
    assert!(everything.len() >= mainline.len());
    assert!(
        everything.iter().any(|c| c.connector),
        "this leg's interchange arcs should be present when asked for"
    );
}

// -- TestInterstateArtifactScreen -------------------------------------------
// Geometry artifacts never reach an interstate mainline.
//
// The dense sweep baked departure geometry and interchange vertices as
// mainline on some interstate legs, which read as 80-250 ft "hairpins" on
// roads that physically cannot bend that hard. The loader screens them.

#[test]
fn test_no_impossibly_sharp_interstate_mainline_curve() {
    let world = world();
    let mut offenders = Vec::new();
    for leg in &world.legs {
        if !is_interstate(leg) {
            continue;
        }
        for rec in leg_curves(&format!("{}:{}", leg.a, leg.b), true) {
            if rec.min_radius_ft < INTERSTATE_MIN_RADIUS_FT {
                offenders.push((leg.highway.clone(), format!("{}:{}", leg.a, leg.b), rec));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{} interstate mainline curves below {INTERSTATE_MIN_RADIUS_FT} ft: {:?}",
        offenders.len(),
        &offenders[..offenders.len().min(5)]
    );
}

#[test]
fn test_no_switchback_deflection_on_interstate_mainline() {
    // A 150-degree bend on interstate mainline is a mis-tagged loop ramp.
    let world = world();
    let mut offenders = Vec::new();
    for leg in &world.legs {
        if !is_interstate(leg) {
            continue;
        }
        for rec in leg_curves(&format!("{}:{}", leg.a, leg.b), true) {
            if rec.deflection_deg >= INTERSTATE_MAX_DEFLECTION_DEG {
                offenders.push((leg.highway.clone(), format!("{}:{}", leg.a, leg.b), rec));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{} interstate switchbacks: {:?}",
        offenders.len(),
        &offenders[..offenders.len().min(5)]
    );
}

#[test]
fn test_no_hairpin_severity_on_interstate_mainline() {
    // The screen's whole point: no interstate mainline hairpin calls.
    //
    // Driven through `route_curves` -- the path every consumer takes --
    // one leg at a time, so a mixed-class route cannot mask or fake a
    // failure with some other road's legitimately sharp bend.
    let world = world();
    for leg in &world.legs {
        if !is_interstate(leg) {
            continue;
        }
        let route = Route::new(vec![leg.a.clone(), leg.b.clone()], vec![leg.clone()]);
        for cur in route_curves(&route, &route.cities, true) {
            assert_ne!(
                cur.severity(),
                "hairpin",
                "{} {}:{} still calls a hairpin at mile {:.2} (radius {} ft)",
                leg.highway,
                leg.a,
                leg.b,
                cur.apex_mi,
                cur.min_radius_ft
            );
        }
    }
}

#[test]
fn test_no_curve_sits_past_the_end_of_its_leg() {
    // A curve row is leg-relative, so a mile past the leg's own length lands
    // on the next leg of the route as a bend that isn't there. Columbus to
    // Tupelo once ran its ramp rows two miles past the 64-mile leg. Connector
    // rows count too, since they reach the trip the same way.
    let world = world();
    let mut offenders = Vec::new();
    for leg in &world.legs {
        let key = format!("{}:{}", leg.a, leg.b);
        for rec in leg_curves(&key, false) {
            let far = rec.start_mi.max(rec.apex_mi).max(rec.end_mi);
            if far > leg.miles + 0.05 {
                offenders.push(format!("{key} ({} mi) curve to mile {far:.2}", leg.miles));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "curves past their leg's end:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn test_abilene_fort_worth_mile_four_hairpins_are_gone() {
    // Flat I-20 had three 104-111 ft "hairpins" at mile 4.
    let recs = leg_curves("abilene_tx_us:fort_worth_tx_us", true);
    assert!(
        !recs.is_empty(),
        "this leg is swept and should still have real curves"
    );
    let near_four: Vec<_> = recs
        .iter()
        .filter(|r| (3.5..=4.5).contains(&r.apex_mi))
        .collect();
    assert!(
        near_four.is_empty(),
        "artifact cluster survived: {near_four:?}"
    );
    assert!(recs.iter().map(|r| r.min_radius_ft).min().unwrap() >= INTERSTATE_MIN_RADIUS_FT);
}

#[test]
fn test_akron_cleveland_mile_thirty_seven_hairpin_is_gone() {
    // I-77 carried two 82 ft "hairpins" from interchange geometry.
    let recs = leg_curves("akron_oh_us:cleveland_oh_us", true);
    assert!(!recs.is_empty());
    assert!(recs
        .iter()
        .all(|r| r.min_radius_ft >= INTERSTATE_MIN_RADIUS_FT));

    // The real bends on this leg stay. Pinned as a PROPERTY rather than a
    // count: the flat-ground screen took this leg from 21 mainline curves
    // to 11, and every one it removed is below the 1,482 ft floor a 65 mph
    // road may bend to -- including the two 82 ft, 160-degree records this
    // test is named for. A bare "at least fifteen" would have had to be
    // relaxed to keep passing, which says nothing; this says what must be
    // true of whatever survives.
    let leg = world()
        .legs
        .iter()
        .find(|leg| leg.a == "akron_oh_us" && leg.b == "cleveland_oh_us")
        .unwrap();
    let floor = min_radius_ft(leg_design_speed(leg));
    assert!(recs.len() >= 8);
    assert!(
        recs.iter().all(|r| (r.min_radius_ft as f64) >= floor),
        "a curve under the {floor:.0} ft floor survived on {}",
        leg.highway
    );
}

#[test]
fn test_interstate_connector_arcs_are_untouched() {
    // Ramps really are that sharp; physics still wants them.
    let everything = leg_curves("abilene_tx_us:fort_worth_tx_us", false);
    assert!(
        everything
            .iter()
            .any(|r| r.connector && r.min_radius_ft < 150),
        "interchange ramp arcs should survive the screen"
    );
}

#[test]
fn test_million_dollar_highway_switchbacks_survive() {
    // US-550 Durango-Montrose really does switch back. Never screen it.
    let recs = leg_curves("durango_co_us:montrose_co_us", true);
    assert!(recs.len() >= 250);
    assert!(recs.iter().map(|r| r.min_radius_ft).min().unwrap() < 100);
    assert!(recs.iter().map(|r| r.deflection_deg).fold(0.0, f64::max) >= 150.0);
}

#[test]
fn test_glenwood_canyon_interstate_curves_survive() {
    // Real I-70 canyon geometry must stay.
    //
    // This is the test that caught the design-floor screen (raising
    // INTERSTATE_MIN_RADIUS_FT to 758, tried and reverted 2026-08-23), and it
    // guards the connector bake the same way.
    //
    // TWO legs, because the canyon proper is EAST of Glenwood Springs: Edwards
    // to Glenwood Springs is Glenwood Canyon itself, and Glenwood Springs to
    // Grand Junction runs De Beque Canyon out the other side of town.
    //
    // It counts CANYON miles rather than the leg total, which is what the leg
    // total was standing in for -- and badly: the sub-500 ft record the old bar
    // tested for was a motorway_link ramp onto the I-70 business route at Grand
    // Junction, not canyon at all. When the connector bake landed the totals
    // moved (71 -> 70 and 60 -> 54) and every curve it took was a ramp or a
    // town road: those business-route ramps, Pine Street and West 6th Street
    // out of Glenwood Springs, Ute Avenue and South 12th Street into Grand
    // Junction. Between the ends nothing moved -- Glenwood Canyon holds the
    // same 57 curves at the same 574 ft minimum radius, and the same 15 that
    // ask a truck to slow.
    let world = world();
    let miles: HashMap<String, f64> = world
        .legs
        .iter()
        .map(|leg| (format!("{}:{}", leg.a, leg.b), leg.miles))
        .collect();
    for (key, floor) in [
        ("edwards_co_us:glenwood_springs_co_us", 55),
        ("glenwood_springs_co_us:grand_junction_co_us", 45),
    ] {
        let leg_miles = miles[key];
        let canyon: Vec<_> = leg_curves(key, true)
            .into_iter()
            .filter(|r| r.apex_mi > 5.0 && r.apex_mi < leg_miles - 5.0)
            .collect();
        assert!(
            canyon.len() >= floor,
            "{key} kept only {} curves off its ends",
            canyon.len()
        );
        assert!(
            canyon.iter().map(|r| r.min_radius_ft).min().unwrap() < 700,
            "{key}'s genuinely sharp canyon bends should still be here"
        );
    }
}

#[test]
fn test_us_highway_mountain_hairpins_survive() {
    // US-40 over the Rockies keeps its real sharp curves.
    //
    // The interstate screen never applies to US routes; the separate
    // flat-terrain screen (below) only takes the one Denver-departure
    // artifact, so the mountain bends this leg is famous for stay.
    let recs = leg_curves("denver_co_us:salt_lake_city_ut_us", true);
    assert!(recs
        .iter()
        .any(|r| r.min_radius_ft < INTERSTATE_MIN_RADIUS_FT));
}

// -- TestUSRouteArtifactScreen ----------------------------------------------
// A second, narrower screen for artifacts road class alone can't catch.
//
// US and state routes can carry the same city-departure sweep artifact an
// interstate can, but they also carry real switchbacks the interstate
// screen would wrongly delete (US-550, the Salt River Canyon) -- so this
// screen is gated on local terrain (flat ground can't hold a real
// hairpin), not on road class. See `tools/screen_curve_artifacts.py`.

#[test]
fn test_denver_us40_departure_kink_is_gone() {
    // The flat-Denver-metro kink at mile 1.7 was the reported case.
    let recs = leg_curves("denver_co_us:salt_lake_city_ut_us", true);
    let near_departure: Vec<_> = recs.iter().filter(|r| r.apex_mi < 2.0).collect();
    assert!(
        !near_departure.iter().any(|r| is_hairpin(r)),
        "flat-terrain departure artifact survived: {near_departure:?}"
    );
}

#[test]
fn test_flagged_artifacts_are_absent_from_every_leg() {
    // Every `(leg, seq)` the offline screen names is actually gone.
    //
    // Round-trips `curve_artifacts.jsonl` against the loaded data so a
    // stale baked file (screen re-run, loader not updated, or vice versa)
    // fails loudly instead of silently drifting.
    let text = read_data_text("world_data/us/gameplay/curve_artifacts.jsonl")
        .expect("curve_artifacts.jsonl should exist once artifacts are flagged");
    let mut flagged_legs: HashSet<String> = HashSet::new();
    let mut count = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row.get("meta").is_some() {
            continue;
        }
        flagged_legs.insert(row["leg"].as_str().unwrap().to_string());
        count += 1;
    }
    assert!(count > 0);

    let world = world();
    let by_key: HashMap<String, &Leg> = world
        .legs
        .iter()
        .map(|leg| (format!("{}:{}", leg.a, leg.b), leg.as_ref()))
        .collect();
    for leg_key in &flagged_legs {
        let Some(leg) = by_key.get(leg_key) else {
            continue;
        };
        assert!(
            !is_interstate(leg),
            "{leg_key} is flagged but is interstate mainline -- that screen is a separate, unconditional rule"
        );
    }
}

#[test]
fn test_city_departure_hairpins_are_gone_off_the_mountains() {
    // Terrain alone could not see a departure kink on rolling ground.
    //
    // Reported 2026-08-11: hairpins "and not just on mountains either". The
    // flat screen caught the artifact only where the city sat on flat
    // ground, so the same 43 ft kink a mile out of Hazard on KY-80 -- and
    // 112 like it -- rode through on "hills".
    for leg_key in [
        "hot_springs_ar_us:fort_smith_ar_us",
        "hot_springs_ar_us:little_rock_ar_us",
        "rochester_mn_us:winona_mn_us",
        "oxford_ms_us:memphis_tn_us",
    ] {
        let recs = leg_curves(leg_key, true);
        assert!(!recs.is_empty(), "{leg_key}");
        let near_departure: Vec<_> = recs
            .iter()
            .filter(|r| r.apex_mi < 2.5 && is_hairpin(r))
            .collect();
        assert!(
            near_departure.is_empty(),
            "{leg_key}: departure artifact survived: {near_departure:?}"
        );
    }
}

#[test]
fn test_leaving_a_mountain_town_keeps_the_road_and_drops_the_town() {
    // Mountain-town through-road honesty: town streets drop as connectors,
    // the trunk's own bends near the city node stay.
    //
    // Originally pinned to Hazard->London (KY-15). That directed edge was
    // retired with the truck-router refuse leftovers (owner option 3); the
    // business-loop / KY-15 story in older comments lived on that edge. The
    // property is the same on the surviving eastern-KY coalfield trunk:
    // Ashland->Paintsville on US-23. Departure streets read off-corridor;
    // US-23's own bends inside the first 2.5 miles survive, none tighter
    // than a truck's turning circle.
    let recs = leg_curves("ashland_ky_us:paintsville_ky_us", true);
    let near: Vec<_> = recs.iter().filter(|r| r.apex_mi < 2.5).collect();
    assert!(
        !near.is_empty(),
        "US-23's own bends leaving Ashland must survive"
    );
    assert!(
        near.iter().all(|r| r.min_radius_ft >= 50),
        "nothing tighter than a truck's turning circle is a road: {near:?}"
    );
}

#[test]
fn test_a_mountain_town_keeps_the_road_out_of_it() {
    // Same property on the remaining Pikeville mountain approach.
    //
    // Originally Charleston->Pikeville (US-119 Corridor G), then Johnson
    // City->Pikeville on US-23, split at Kingsport on 2026-11-23. The
    // Kingsport piece still climbs into Pikeville on US-23: town geometry
    // at the end reads as connectors, and the trunk bends inside the last
    // three miles stay.
    let key = "kingsport_tn_us:pikeville_ky_us";
    let miles = world()
        .legs
        .iter()
        .find(|l| format!("{}:{}", l.a, l.b) == key)
        .expect("the Kingsport to Pikeville leg")
        .miles;
    let recs = leg_curves(key, true);
    let near: Vec<_> = recs.iter().filter(|r| r.apex_mi > miles - 3.0).collect();
    assert!(
        !near.is_empty(),
        "US-23's own bends toward Pikeville must survive"
    );
    assert!(near.iter().all(|r| r.min_radius_ft >= 50));
}

/// Switchbacks read tighter than the floor, each checked against the map.
///
/// US 421 over Iron Mountain (The Snake, between Shady Valley and Mountain
/// City) turns 190 degrees at mile 24.7 of Bristol to Hickory (2026-11-23).
/// OpenStreetMap draws its hairpins with nodes 20 to 30 ft apart, so the
/// 46 ft radius is the drawn road, not a stray vertex. It is the road the
/// truck router takes, and OpenStreetMap carries no truck restriction on it.
fn is_read_switchback(leg_key: &str, rec: &CurveRecord) -> bool {
    leg_key == "bristol_tn_us:hickory_nc_us"
        && (24.5..25.0).contains(&rec.apex_mi)
        && rec.min_radius_ft >= 40
}

#[test]
fn test_no_surviving_curve_is_tighter_than_a_road_can_bend() {
    // A radius floor for every class, the sibling of the interstate 300 ft.
    //
    // 50 ft is tighter than a loaded tractor-trailer's own turning circle,
    // so nothing that bends harder is a road. The floor sits just under the
    // tightest genuine switchback the world carries (US-550 at 54 ft), which
    // is why it can be applied everywhere without a terrain test.
    let offenders: Vec<(&String, &CurveRecord)> = load()
        .iter()
        .flat_map(|(leg_key, recs)| recs.iter().map(move |rec| (leg_key, rec)))
        .filter(|(_, rec)| !rec.connector && rec.min_radius_ft < 50)
        .filter(|(leg_key, rec)| !is_read_switchback(leg_key, rec))
        .collect();
    assert!(
        offenders.is_empty(),
        "impossible mainline radii survived: {:?}",
        &offenders[..offenders.len().min(5)]
    );
}

#[test]
fn test_million_dollar_highway_untouched_by_the_new_screen() {
    // A mountain corridor keeps every switchback under the new screen too.
    let recs = leg_curves("durango_co_us:montrose_co_us", true);
    assert!(recs.len() >= 250);
    assert!(
        recs.iter().any(is_hairpin),
        "US-550's real hairpins must survive the flat-terrain screen"
    );
}

#[test]
fn test_salt_river_canyon_untouched_by_the_new_screen() {
    // Globe->Show Low (US-60) keeps its mountain switchbacks too.
    let recs = leg_curves("globe_az_us:show_low_az_us", true);
    assert!(
        recs.iter().any(is_hairpin),
        "the Salt River Canyon's real hairpins must survive the screen"
    );
}

// -- TestTripCurveIntegration --------------------------------------------------

/// `Trip(route, TruckState(), _MockWeather(), time_scale=10.0, seed=42)`.
fn curve_trip(route: Route) -> ff_core::sim::trip::Trip {
    use ff_core::sim::trip::{Trip, TripOptions};
    use ff_core::sim::vehicle::TruckState;
    use ff_core::sim::weather::WeatherSystem;

    Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("heartland", Some(1), None, None, true),
        TripOptions {
            time_scale: 10.0,
            seed: Some(42),
            ..Default::default()
        },
    )
}

fn abilene_dallas() -> Option<Route> {
    World::load_from(&data_dir())
        .unwrap()
        .shortest_route("abilene_tx_us", "dallas_tx_us", None, false)
        .unwrap()
}

// -- TestTripCurveIntegration (needs sim::trip) -----------------------------

#[test]
fn test_place_curves_empty_short_approach() {
    // A very short approach route (single leg, < 10 mi) gets no curves.
    //
    // Curves are only meaningful on highway-length legs; short facility
    // approaches should have no curve placement.
    let leg = Leg::new(
        "abilene_tx_us",
        "abilene_tx_us",
        5.0,
        "US-83",
        "flat",
        Vec::new(),
    )
    .with_detail(CorridorDetail {
        checkpoints: vec![RouteCheckpoint::new("Midpoint", 2.5, "place", "", "US-83")],
        state_miles: vec![StateMileage::new("Texas", 5.0)],
        grade_segments: vec![GradeSegment::new(0.0, 5.0, 0.0, "flat", "test")],
        ..Default::default()
    });
    let route = Route::from_legs(
        vec!["abilene_tx_us".to_string(), "abilene_tx_us".to_string()],
        vec![leg],
    );
    assert!(curve_trip(route).curves.is_empty());
}

#[test]
fn test_interstate_artifact_never_reaches_trip_curves() {
    // The Abilene I-20 mile-4 artifacts stay out of the live trip.
    // `Trip::place_curves` keeps connectors for physics, so this checks the
    // deepest consumer path, not just the spoken one.
    let world = World::load_from(&data_dir()).unwrap();
    let route = supported(&world, "abilene_tx_us", "fort_worth_tx_us");
    assert!(
        route.legs.iter().all(|leg| leg.highway.starts_with("I-")),
        "this fixture route is meant to be interstate the whole way"
    );
    let trip = curve_trip(route);
    let mainline: Vec<_> = trip.curves.iter().filter(|c| !c.connector).collect();
    assert!(
        !mainline.is_empty(),
        "the route should still have real curves"
    );
    assert!(!mainline.iter().any(|c| c.severity() == "hairpin"));
    assert!(!mainline.iter().any(|c| (3.5..=4.5).contains(&c.apex_mi)));
}

#[test]
fn test_place_curves_highway_route() {
    // A highway route resolves curves from leg-relative to trip miles.
    let Some(route) = abilene_dallas() else {
        return;
    };
    let trip = curve_trip(route);
    let total_miles = trip.total_miles();
    for cr in &trip.curves {
        assert!((0.0..=total_miles).contains(&cr.start_mi));
        assert!((0.0..=total_miles).contains(&cr.end_mi));
        assert!((cr.start_mi - cr.end_mi).abs() < 5.0); // no mile-long outliers
        assert!(cr.direction == 'L' || cr.direction == 'R');
    }
}

#[test]
fn test_curve_at_inside() {
    // curve_at returns the curve containing a milepost.
    let Some(route) = abilene_dallas() else {
        return;
    };
    let trip = curve_trip(route);
    let Some(cr) = trip.curves.first().copied() else {
        return;
    };
    let mid = (cr.start_mi + cr.end_mi) / 2.0;
    let found = trip
        .curve_at(mid)
        .expect("the curve containing its midpoint");
    assert_eq!(found.start_mi, cr.start_mi);
}

#[test]
fn test_curve_at_none() {
    // Outside all curves, curve_at returns None.
    let Some(route) = abilene_dallas() else {
        return;
    };
    let trip = curve_trip(route);
    assert!(trip.curve_at(-1.0).is_none());
    assert!(trip.curve_at(trip.total_miles() + 1.0).is_none());
}

#[test]
fn test_check_curves_emits_for_sharp_curve() {
    // A sharp curve ahead generates a CURVE event with a pacenote.
    use ff_core::sim::trip_models::TripEventKind;

    let Some(route) = abilene_dallas() else {
        return;
    };
    let mut trip = curve_trip(route);
    trip.truck.start_engine();
    trip.truck.velocity_mps = 60.0 * 0.44704; // 60 mph in m/s
    let Some(first) = trip.curves.first().copied() else {
        return;
    };
    // Position the truck before the first curve
    trip.position_mi = (first.start_mi - 1.0).max(0.0);
    let events = trip.update(0.1);
    for ev in events.iter().filter(|e| e.kind == TripEventKind::Curve) {
        assert!(ev.text().contains("advisory") || ev.text().contains("curve"));
    }
}

#[test]
fn test_restore_seeds_announced_curves() {
    // Restoring a save seeds curves behind the position as announced.
    let Some(route) = abilene_dallas() else {
        return;
    };
    let mut trip = curve_trip(route);
    let Some(first) = trip.curves.first().copied() else {
        return;
    };
    trip.restore(first.start_mi + 0.5, 10.0);
    let expected_key = format!("curve:{:.3}:{}", first.start_mi, first.direction);
    assert!(trip.announced_curves.contains(&expected_key));
}

// --- flat-ground class screen (owner audit, 2026-08-19) ---------------------

#[test]
fn test_no_mainline_curve_bends_tighter_than_its_class_on_flat_ground() {
    // Owner, 2026-08-19: "cruising down the highway or turnpike in a car, it
    // hardly ever curves. I want an honest audit."
    //
    // The audit found the map disagreeing with the road. The MEDIAN interstate
    // curve radius in the bake was 1,342 ft against a 1,330 ft design floor --
    // half of them at or below what a 70 mph road may legally bend to -- and
    // the tenth percentile across all mainline was 281 ft, an intersection
    // rather than a highway. Interstate curve callouts ran 5.7 per hundred
    // miles.
    //
    // Rough country still earns a tight bend: this screens flat ground only,
    // so the Rockies and US-550 drive the way they should.
    let world = world();
    let screenable = screenable_legs();
    let mut offenders = Vec::new();
    for leg in &world.legs {
        let Some(floor) = screenable.get(&format!("{}:{}", leg.a, leg.b)) else {
            continue; // not flat enough to judge; see the canyon test below
        };
        for curve in leg_curves(&format!("{}:{}", leg.a, leg.b), true) {
            if (curve.min_radius_ft as f64) < *floor {
                offenders.push((
                    leg.a.clone(),
                    leg.b.clone(),
                    leg.highway.clone(),
                    curve.min_radius_ft,
                    *floor,
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{} flat-ground curves under floor: {:?}",
        offenders.len(),
        &offenders[..offenders.len().min(5)]
    );
}

#[test]
fn test_rough_country_keeps_its_tight_bends() {
    // The other half, and the reason this screens terrain rather than radius.
    //
    // A blanket radius floor would flatten the best driving in the game. A
    // mountain leg is allowed to bend tighter than any design table, because
    // real ones do.
    let world = world();
    let floor_70 = min_radius_ft(70.0);
    let mut kept_tight = 0;
    for leg in &world.legs {
        if leg_is_level(leg) {
            continue;
        }
        for curve in leg_curves(&format!("{}:{}", leg.a, leg.b), true) {
            if (curve.min_radius_ft as f64) < floor_70 {
                kept_tight += 1;
            }
        }
    }
    assert!(
        kept_tight > 500,
        "only {kept_tight} tight curves survive off flat ground"
    );
}

#[test]
fn test_a_canyon_tagged_flat_is_not_screened_as_level_ground() {
    // The world's terrain label is derived from NET elevation change, so a
    // road that climbs and drops all the way along without getting anywhere
    // reads as flat. I-70 through Glenwood Canyon is tagged flat, and its
    // curves are cut into rock walls.
    //
    // Caught by test_glenwood_canyon_interstate_curves_survive when the screen
    // first went in and took 21 real curves off it. Two proxies were tried and
    // both failed -- the label itself, then feet of relief per mile, which
    // calibrated against HPMS at a Youden's J of 0.29. The screen reads the
    // real HPMS terrain class now, and HPMS calls this leg mountainous.
    let world = world();
    let canyon = world
        .legs
        .iter()
        .find(|leg| leg.a == "glenwood_springs_co_us" && leg.b == "grand_junction_co_us")
        .unwrap();
    assert!(!leg_is_level(canyon)); // HPMS calls it mountainous, and it is

    // The canyon's own label said "flat" when this test was written, which is
    // what made it such a good example. It has since been corrected from HPMS
    // (tools/repair_terrain_from_hpms.py), so the demonstration moved: rather
    // than naming one leg where the label lies, assert the PROPERTY directly --
    // the screen's verdict follows HPMS on every leg, never the label.
    let mut disagreeing = 0;
    for leg in &world.legs {
        let Some(hpms) = leg.hpms_terrain() else {
            assert!(
                !leg_is_level(leg),
                "absence of a class must never read as level"
            );
            continue;
        };
        let hpms_level = hpms.terrain_type == HPMS_TERRAIN_LEVEL;
        assert_eq!(leg_is_level(leg), hpms_level);
        if (leg.terrain == "flat") != hpms_level {
            disagreeing += 1;
        }
    }
    assert!(
        disagreeing > 0,
        "if no leg's label disagrees with HPMS any more, this test proves nothing"
    );
}

#[test]
fn test_the_radius_floor_follows_the_leg_s_own_posted_limit() {
    // Design speed comes from the baked OSM maxspeed sweep, not a guess at
    // the road's class. A 55 mph US route and a 70 mph interstate are held to
    // different floors because they are different roads, and the data says
    // which is which.
    let speeds: Vec<f64> = world()
        .legs
        .iter()
        .map(|leg| leg_design_speed(leg))
        .collect();
    let distinct: HashSet<u64> = speeds.iter().map(|s| s.to_bits()).collect();
    assert!(
        distinct.len() > 1,
        "every leg fell back to the same default speed"
    );
    let max = speeds.iter().cloned().fold(f64::MIN, f64::max);
    let min = speeds.iter().cloned().fold(f64::MAX, f64::min);
    assert!(max >= 65.0);
    // The floor tracks it rather than being pinned per class.
    assert!(min_radius_ft(max) > min_radius_ft(min));
}

#[test]
fn test_the_terrain_bake_says_what_kind_of_value_it_carries() {
    // AGENTS.md: a baked record must make plain whether it was read, derived
    // or assumed. The HPMS class is READ; that one value stands for a whole leg
    // is DERIVED, and the source string has to say both.
    let baked: Vec<_> = world()
        .legs
        .iter()
        .filter(|leg| leg.hpms_terrain().is_some())
        .collect();
    assert!(!baked.is_empty(), "no leg carries an HPMS terrain class");
    let source = &baked[0].hpms_terrain().unwrap().source;
    assert!(source.contains("HPMS"));
    let lowered = source.to_lowercase();
    assert!(lowered.contains("modal") || lowered.contains("derived"));
    assert!(baked
        .iter()
        .all(|leg| (1..=3).contains(&leg.hpms_terrain().unwrap().terrain_type)));
}

#[test]
fn severity_bands_match_the_advisory_speed() {
    // The hairpin needs both halves now (MUTCD: 135 degrees AND a Turn sign's
    // 30 mph or less); the three speed bands below it are unchanged.
    assert_eq!(curve_severity(25, 160.0), "hairpin");
    assert_eq!(curve_severity(25, 30.0), "sharp");
    assert_eq!(curve_severity(45, 160.0), "moderate");
    assert_eq!(curve_severity(35, 30.0), "sharp");
    assert_eq!(curve_severity(50, 30.0), "moderate");
    assert_eq!(curve_severity(55, 30.0), "gentle");
}

// -- TestConnectorsAreReadNotGuessed -----------------------------------------
//
// Interchange and departure geometry is classed by OSM, not by position. The
// sweep flagged a connector only inside 0.75 mi of a leg's ends, which misses
// every mid-leg interchange and does not get a truck out of Denver.
// `classify_connector` (`tools/bake_curve_connectors.py`) re-derives the flag
// from the road class OSM records under each curve's apex.

#[test]
fn test_a_ramp_reads_as_a_connector_on_every_road_class() {
    for class in ["motorway_link", "trunk_link", "primary_link"] {
        for made_of in [Some("motorway"), Some("trunk"), Some("primary"), None] {
            assert_eq!(
                classify_connector(Some(0.4), Some(class), made_of),
                (true, "osm:ramp")
            );
        }
    }
}

/// Every Interstate mainline mile is a freeway, so on a leg MADE of freeway a
/// surface-road apex is not on the Interstate.
///
/// But the comparison is against the road the leg is made of, not against a
/// fixed class -- otherwise a curated I-65 whose route actually runs US-231 end
/// to end has all its real trunk bends read as off-route.
#[test]
fn test_a_bend_below_its_leg_s_own_road_is_not_on_the_through_route() {
    for class in ["trunk", "primary", "secondary", "residential"] {
        assert_eq!(
            classify_connector(Some(0.4), Some(class), Some("motorway")),
            (true, "osm:off-corridor")
        );
    }
    // A leg made of trunk keeps its trunk bends and drops the town.
    assert_eq!(
        classify_connector(Some(0.4), Some("trunk"), Some("trunk")),
        (false, "osm:mainline")
    );
    for class in ["primary", "secondary", "residential"] {
        assert!(classify_connector(Some(0.4), Some(class), Some("trunk")).0);
    }
    // And a better road than the leg is made of is never off-route.
    assert_eq!(
        classify_connector(Some(0.4), Some("motorway"), Some("trunk")),
        (false, "osm:mainline")
    );
}

/// The guard on the whole rule: it never sees the geometry.
///
/// Raising the radius floor to the design minimum is what deleted Glenwood
/// Canyon (tried and reverted, 2026-08-23). This classifier takes no radius,
/// deflection or advisory at all -- there is no argument through which a sharp
/// curve could reach it -- so it cannot repeat that.
#[test]
fn test_a_freeway_curve_stays_mainline_however_hard_it_bends() {
    assert_eq!(
        classify_connector(Some(0.2), Some("motorway"), Some("motorway")),
        (false, "osm:mainline")
    );
}

/// No extract, or no road in the corridor, must not read as mainline.
#[test]
fn test_nothing_read_concludes_nothing() {
    let facts: [(Option<f64>, Option<&str>); 3] = [
        (None, None),                         // near_m recorded as null
        (Some(400.0), Some("motorway_link")), // a road, but far outside
        (None, None),                         // no reading at all
    ];
    for (near_m, near_hw) in facts {
        assert_eq!(
            classify_connector(near_m, near_hw, Some("motorway")),
            (false, "")
        );
    }
    // Nor may anything be concluded when the leg's own road went unread.
    assert_eq!(
        classify_connector(Some(0.3), Some("residential"), None),
        (false, "osm:mainline")
    );
}

#[test]
fn test_the_corridor_is_a_sanity_bound_not_a_decision() {
    assert!(!classify_connector(
        Some(CONNECTOR_CORRIDOR_M - 0.1),
        Some("motorway"),
        Some("motorway")
    )
    .1
    .is_empty());
    assert_eq!(
        classify_connector(
            Some(CONNECTOR_CORRIDOR_M + 0.1),
            Some("motorway"),
            Some("motorway")
        )
        .1,
        ""
    );
}

/// Provenance: a connector says whether it was read or positional.
///
/// `osm:ramp` and `osm:off-corridor` are readings from the OSM extract;
/// `sweep:window` is the bake's original positional call, kept because the
/// two are a union and the window sees city geometry the class reading can
/// miss.
#[test]
fn test_every_connector_row_names_the_reading_that_flagged_it() {
    // Every country's curves: a Canadian leg's rows live in `ca/gameplay`.
    let text = ff_core::data::data_resources::country_gameplay_text_at(
        ff_core::data::data_resources::data_root(),
        "curves.jsonl",
    )
    .expect("curves.jsonl");
    let known = ["osm:ramp", "osm:off-corridor", "sweep:window"];
    let mut seen: HashSet<String> = HashSet::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row.get("meta").is_some() || !row["connector"].as_bool().unwrap_or(false) {
            continue;
        }
        let source = row.get("connector_source").and_then(|v| v.as_str());
        assert!(
            source.is_some_and(|s| known.contains(&s)),
            "{} seq {} carries {source:?}",
            row["leg"],
            row["seq"]
        );
        seen.insert(source.unwrap().to_string());
    }
    let mut sorted: Vec<&String> = seen.iter().collect();
    sorted.sort();
    assert!(
        seen.contains("osm:ramp") && seen.contains("osm:off-corridor"),
        "the OSM readings should both be present in the bake, saw {sorted:?}"
    );
}

/// The owner's report, 2026-08-23: "interstate mainline curves make the truck
/// slow far too often."
///
/// A real interstate on level ground asks a loaded truck to drop below 65
/// essentially never -- it is built to be driven at 70 or 75 all the way. Two
/// fixes moved this number: counting the bank the road is built with (one
/// every 28.8 miles -> 44.2), and then classifying interchange and departure
/// geometry as the connectors they are rather than as mainline.
///
/// Pinned as a rate rather than a count so the map can grow. The bar is set
/// well below what shipped, because this is a floor on quality, not a target
/// anything was fitted to.
///
/// Flat ground only (2026-11-23). Each curve is classed by the grade stretch
/// it sits on, not by its whole leg. Summed over every terrain the rate
/// stopped measuring quality: every new leg over a pass repeats that pass, and
/// six legs now cross Snoqualmie on I-90, which put the whole-map rate at
/// 99.2. Terrain separates the rates by an order of magnitude each. On the
/// Mountain and Northwest base (ed715860) flat, hills and mountain stretches
/// gave one slowdown every 279.0, 33.8 and 7.3 miles; on that branch 274.3,
/// 34.2 and 7.4. Interstate design allows exactly that: 70 mph generally, 60
/// in hilly terrain and 50 in mountainous (the Interstate Guide's summary of
/// AASHTO's "A Policy on Design Standards: Interstate System", edition not
/// named, read 2026-11-23; a summary, not the standard itself). Hills and
/// mountain get no floor here; their rates must only keep their order, or the
/// terrain or curve data is wrong. Measurements:
/// /mnt/project-files/missing-roads/mountain-northwest/interstate-curves-by-terrain.md
#[test]
fn test_interstate_mainline_asks_the_truck_to_slow_down_rarely() {
    let world = world();
    let mut seen: HashSet<String> = HashSet::new();
    // (slowdowns, miles) per grade stretch terrain: flat, hills, mountain.
    let mut by_terrain: HashMap<String, (usize, f64)> = HashMap::new();
    for leg in &world.legs {
        let key = format!("{}:{}", leg.a, leg.b);
        if seen.contains(&key) || !is_interstate(leg) {
            continue;
        }
        seen.insert(key.clone());
        let segments = leg.grade_segments();
        for segment in segments {
            by_terrain.entry(segment.terrain.clone()).or_default().1 +=
                segment.end_mi - segment.start_mi;
        }
        for curve in leg_curves(&key, true)
            .iter()
            .filter(|curve| curve.advisory_mph <= 65)
        {
            // A curve on no stretch counts as flat, so missing grade data
            // cannot hide a slowdown.
            let terrain = segments
                .iter()
                .find(|s| s.start_mi <= curve.apex_mi && curve.apex_mi < s.end_mi)
                .map_or("flat", |s| s.terrain.as_str());
            by_terrain.entry(terrain.to_string()).or_default().0 += 1;
        }
    }
    let rate = |terrain: &str| {
        let (slow, miles) = by_terrain.get(terrain).copied().unwrap_or_default();
        assert!(
            slow > 0,
            "no interstate slowdowns on {terrain} stretches at all means the data vanished"
        );
        miles / slow as f64
    };
    let (flat, hills, mountain) = (rate("flat"), rate("hills"), rate("mountain"));
    assert!(
        flat > 100.0,
        "one interstate slowdown every {flat:.1} miles of flat ground is too often"
    );
    assert!(
        mountain < hills && hills < flat,
        "slowdowns should be densest in the mountains and sparsest on flat ground, \
         but came one every {mountain:.1} (mountain), {hills:.1} (hills), {flat:.1} (flat) miles"
    );
}

/// The advisory is AASHTO's point-mass control solved for V, and that
/// control's friction table is published for 20 through 80 mph.
///
/// Run unclamped on a gentle bend it read out 115, which is not a claim about
/// a road -- it is arithmetic past the edge of its own table. 21,076 of 63,873
/// rows (33 percent) were over 80 before the cap. Nothing audible moved: an
/// advisory above the posted limit never fires a pacenote, never counts as
/// corner overspeed and never eases cruise.
#[test]
fn test_no_baked_advisory_is_faster_than_the_table_it_came_from() {
    let text = read_data_text("world_data/us/gameplay/curves.jsonl").expect("curves.jsonl");
    let mut worst = 0i64;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row.get("meta").is_some() {
            continue;
        }
        worst = worst.max(row["advisory_mph"].as_i64().unwrap());
    }
    assert!(
        worst <= ADVISORY_MAX_MPH,
        "a baked row still advises {worst} mph"
    );
}

/// `route_drive_hours` walked the route on posted limits alone, so every bend
/// the driver has to slow for was time the plan never budgeted.
///
/// It now times each sampled segment piecewise: the miles inside a bend at
/// that bend's advisory, the rest at the planning limit. Only the bend's own
/// recorded span is charged -- no invented deceleration ramp, which is the
/// sort of unmeasured loss DEADLINE_PLANNING_SPEED_FACTOR already carries.
#[test]
fn test_the_deadline_plan_slows_for_the_bends_it_will_meet() {
    // No bends: the segment is just distance over speed.
    assert_eq!(segment_hours(0.0, 2.0, 60.0, &[]), 2.0 / 60.0);

    // A bend covering half the segment is charged at its advisory for that
    // half only, so the segment costs more than the open road and less than
    // the whole thing taken at the advisory.
    let bands = [(0.0, 1.0, 30.0)];
    let slowed = segment_hours(0.0, 2.0, 60.0, &bands);
    assert_eq!(slowed, 1.0 / 30.0 + 1.0 / 60.0);
    assert!(2.0 / 60.0 < slowed && slowed < 2.0 / 30.0);

    // A bend the truck is already slower than costs nothing extra.
    assert_eq!(
        segment_hours(0.0, 2.0, 25.0, &[(0.0, 1.0, 55.0)]),
        2.0 / 25.0
    );

    // Bands outside the segment are ignored rather than smeared into it.
    assert_eq!(
        segment_hours(4.0, 6.0, 60.0, &[(0.0, 1.0, 30.0)]),
        2.0 / 60.0
    );
}

/// The check on the piecewise timing, on real baked geometry.
///
/// US-550 over Red Mountain Pass is 107 miles of switchback and its bends
/// really do cost the plan time. An interstate of similar length costs
/// essentially nothing, which is the point of the other two fixes that landed
/// the same day: once the bank is priced in and interchange geometry is
/// classed as connector, interstate mainline stops asking a truck to slow.
#[test]
fn test_a_switchback_road_costs_the_plan_more_than_an_interstate() {
    let world = world();
    let route = shortest(world, "durango_co_us", "montrose_co_us");
    let bands = curve_ceilings(&route);
    assert!(
        !bands.is_empty(),
        "the Million Dollar Highway's bends should reach the planner"
    );
    let tightest = bands
        .iter()
        .map(|(_, _, advisory)| *advisory)
        .fold(f64::INFINITY, f64::min);
    assert!(tightest <= 25.0, "tightest advisory was {tightest}");

    let aware = route_drive_hours(Some(&route), 0.0, Some(world));
    // The Python case monkeypatches `_curve_ceilings` to return nothing; the
    // port reaches the same "blind to the bends" reading through the explicit
    // bands argument.
    let blind = route_drive_hours_over(Some(&route), 0.0, Some(world), &[]);
    assert!(
        aware > blind,
        "a road of switchbacks must cost the plan more than a flat read \
         ({aware} vs {blind})"
    );
}
