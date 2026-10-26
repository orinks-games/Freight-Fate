//! Chain laws: areas over sustained steep grade, levels from the live
//! weather and the spoken sign (the trip half of `tests/test_chain_law.py`;
//! the seeded checkpoint and its citation live in
//! `crates/freight-fate/tests/states_driving_chain_law.rs`).
//!
//! The physics (grip multipliers, chain wear, the snap) is pinned in
//! `test_vehicle.py` and `test_physics_bench.py`; these tests cover the law
//! layer that sits on top of it.

use crate::sim_support::*;
use ff_core::data::world_models::GradeSegment;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherKind;

/// A Denver-Silverthorne trip on I-70, inside Colorado's chain-control
/// program; optionally with a synthetic grade profile baked onto leg 0 before
/// construction so chain-law placement sees it.
///
/// Python monkeypatched `Trip.grade_at`; here the same shape comes from real
/// grade segments on the leg, which is the only input `grade_at` reads before
/// its flat-terrain fallback.
fn trip(grade: Option<(f64, f64)>) -> Trip {
    trip_on("Denver", "Silverthorne", grade)
}

fn trip_on(origin: &str, destination: &str, grade: Option<(f64, f64)>) -> Trip {
    let w = world();
    let mut route = first_route_option(w, origin, destination);
    if let Some((start, end)) = grade {
        let edited = with_corridor(&route.legs[0], |detail| {
            detail.grade_segments = vec![GradeSegment {
                start_mi: start,
                end_mi: end,
                avg_grade_pct: 6.0,
                terrain: "mountain".to_string(),
                source: "test fixture".to_string(),
            }];
        });
        route = replace_leg(&route, 0, edited);
    }
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    Trip::new(
        route,
        truck,
        weather("great_lakes", 1),
        TripOptions {
            seed: Some(2),
            world: Some(w),
            ..Default::default()
        },
    )
}

/// Flat, except a sustained 6 percent between miles 10 and 14.
fn mountain_grade() -> Option<(f64, f64)> {
    Some((10.0, 14.0))
}

#[test]
fn test_chain_law_areas_sit_over_sustained_steep_grade() {
    let trip = trip(mountain_grade());
    assert_eq!(trip.chain_law_areas.len(), 1);
    let (start, end) = trip.chain_law_areas[0];
    // The area leads the grade by the chain-up pullout and covers the run.
    assert!((start - 9.0).abs() <= 0.3, "{start}");
    assert!(end >= 14.0, "{end}");
    assert_eq!(trip.chain_law_area_at(12.0), Some(0));
    assert_eq!(trip.chain_law_area_at(5.0), None);
}

#[test]
fn test_a_steep_mile_outside_the_chain_control_states_is_only_a_hill() {
    // The same sustained 6 percent in Illinois: no state there posts
    // commercial chain controls, so no chain law, no chain-up area and no
    // chain-control post, whatever the weather (seasonal audit, 2026-10-01).
    let mut trip = trip_on("Chicago", "Indianapolis", mountain_grade());
    assert!(
        trip.chain_law_areas.is_empty(),
        "{:?}",
        trip.chain_law_areas
    );
    trip.weather.current = WeatherKind::Snow;
    trip.sync_chain_posts();
    assert!(!trip.posts.iter().any(|p| p.kind == "chain_control"));
}

#[test]
fn test_the_chain_control_post_stands_only_while_the_law_is_in_effect() {
    // A chain control goes up when the law is posted. On dry pavement it is
    // not there to call on the CB or to watch the truck (owner, October on
    // I-94, 2026-10-01).
    let mut trip = trip(mountain_grade());
    trip.weather.current = WeatherKind::Clear;
    trip.sync_chain_posts();
    assert!(!trip.posts.iter().any(|p| p.kind == "chain_control"));
    trip.weather.current = WeatherKind::Snow;
    trip.sync_chain_posts();
    assert_eq!(
        trip.posts
            .iter()
            .filter(|p| p.kind == "chain_control")
            .count(),
        1
    );
    trip.weather.current = WeatherKind::Clear;
    trip.sync_chain_posts();
    assert!(!trip.posts.iter().any(|p| p.kind == "chain_control"));
}

#[test]
fn test_short_pitches_do_not_make_a_chain_law() {
    // A half-mile 6 percent pitch is a hill, not a chain-control pass.
    let trip = trip(Some((10.0, 10.5)));
    assert!(
        trip.chain_law_areas.is_empty(),
        "{:?}",
        trip.chain_law_areas
    );
}

#[test]
fn test_chain_law_level_follows_the_surface() {
    let mut trip = trip(None);
    trip.weather.current = WeatherKind::Clear;
    assert_eq!(trip.chain_law_level(), 0);
    trip.weather.current = WeatherKind::Rain;
    assert_eq!(trip.chain_law_level(), 0);
    trip.weather.current = WeatherKind::Snow;
    assert_eq!(trip.chain_law_level(), 1);
    trip.weather.current = WeatherKind::Ice;
    assert_eq!(trip.chain_law_level(), 2);
}

#[test]
fn test_chain_law_sign_speaks_on_approach_and_escalates() {
    let mut trip = trip(mountain_grade());
    trip.weather.current = WeatherKind::Snow;
    trip.position_mi = 8.5; // inside the lookahead of the area at 9.0
    trip.check_chain_law();
    let signs = |trip: &Trip| -> Vec<String> {
        trip.events
            .iter()
            .filter(|e| e.text().starts_with("Chain law"))
            .map(|e| e.text().to_string())
            .collect()
    };
    let spoken = signs(&trip);
    assert_eq!(spoken.len(), 1, "{spoken:?}");
    assert!(spoken[0].contains("level 1"), "{}", spoken[0]);
    assert!(!spoken[0].contains("winter tires"), "{}", spoken[0]);
    assert!(spoken[0].contains("Chain-up area"), "{}", spoken[0]);
    // The same sign does not repeat, but an escalation to ice speaks again.
    trip.check_chain_law();
    trip.weather.current = WeatherKind::Ice;
    trip.check_chain_law();
    let spoken = signs(&trip);
    assert_eq!(spoken.len(), 2, "{spoken:?}");
    assert!(spoken[1].contains("level 2"), "{}", spoken[1]);

    // No law, no sign: the areas are silent infrastructure in clear weather.
    let mut trip2 = trip_clear();
    trip2.check_chain_law();
    assert!(
        !trip2.events.iter().any(|e| e.text().contains("chain law")),
        "clear weather spoke a chain law"
    );
}

fn trip_clear() -> Trip {
    let mut trip = trip(None);
    trip.weather.current = WeatherKind::Clear;
    trip
}

#[test]
fn test_trucks_chain_at_every_level_except_colorado_level_1() {
    use ff_core::sim::trip_models::rig_must_chain;
    for state in ["CA", "WA", "NV", "ID", "MT", "UT"] {
        assert!(!rig_must_chain(Some(state), 0), "{state}");
        assert!(rig_must_chain(Some(state), 1), "{state}");
        assert!(rig_must_chain(Some(state), 2), "{state}");
    }
    // Colorado Level 1 chains single-drive-axle combinations only; the
    // player's tandem-drive rig chains at Level 2.
    assert!(!rig_must_chain(Some("CO"), 1));
    // Wyoming Level 1 takes snow tires or chains (W.S. 31-5-956(e)).
    assert!(!rig_must_chain(Some("WY"), 1));
    assert!(rig_must_chain(Some("WY"), 2));
    assert!(rig_must_chain(Some("CO"), 2));
    assert!(rig_must_chain(None, 1));
}

#[test]
fn test_a_steep_run_off_the_listed_corridors_is_not_a_chain_law() {
    // I-70 over the divide is a Colorado chain corridor; a synthetic 6
    // percent on I-25 to Colorado Springs is not.
    let on = trip(mountain_grade());
    assert_eq!(on.route.legs[0].highway, "I-70");
    assert_eq!(on.chain_law_areas.len(), 1);
    let off = trip_on("Denver", "Colorado Springs", mountain_grade());
    assert_eq!(off.route.legs[0].highway, "I-25");
    assert!(off.chain_law_areas.is_empty(), "{:?}", off.chain_law_areas);
}

#[test]
fn test_chain_corridor_table() {
    use ff_core::sim::trip_models::{in_co_chain_carry_season, on_chain_corridor};
    assert!(on_chain_corridor("CO", "I-70"));
    assert!(!on_chain_corridor("CO", "I-25"));
    assert!(on_chain_corridor("WA", "I-90"));
    assert!(on_chain_corridor("OR", "I-84"));
    assert!(on_chain_corridor("CA", "I-80"));
    assert!(!on_chain_corridor("CA", "I-10"));
    // Unlisted states keep the grade-only rule.
    assert!(on_chain_corridor("NV", "I-80"));
    assert!(on_chain_corridor("ID", "I-90"));
    assert!(on_chain_corridor("ID", "US-12"));
    assert!(!on_chain_corridor("ID", "I-84"));
    assert!(in_co_chain_carry_season(9));
    assert!(in_co_chain_carry_season(5));
    assert!(!in_co_chain_carry_season(6));
    assert!(!in_co_chain_carry_season(8));
    use ff_core::sim::trip_models::in_mt_chain_carry_season;
    assert!(in_mt_chain_carry_season(10));
    assert!(in_mt_chain_carry_season(4));
    assert!(!in_mt_chain_carry_season(5));
    assert!(!in_mt_chain_carry_season(9));
}

#[test]
fn test_chain_law_signs_speak_each_state_rule() {
    use ff_core::sim::trip_models::chain_law_sign_rule;
    assert_eq!(
        chain_law_sign_rule(Some("WY"), 1),
        "level 1: snow tires or chains required"
    );
    assert_eq!(
        chain_law_sign_rule(Some("WY"), 2),
        "level 2: chains required on two tires of one drive axle"
    );
    assert_eq!(
        chain_law_sign_rule(Some("CO"), 1),
        "level 1: chains required on single-drive-axle trucks"
    );
    assert_eq!(
        chain_law_sign_rule(Some("CA"), 1),
        "level 1: chains required on trucks"
    );
    assert_eq!(
        chain_law_sign_rule(Some("CO"), 2),
        "level 2: chains required on all trucks"
    );
}

#[test]
fn test_wyoming_level_1_takes_snow_tires_or_chains() {
    use ff_core::sim::vehicle::TIRE_WINTER;
    let mut trip = trip_on("Cheyenne", "Laramie", None);
    trip.chain_law_areas = vec![(0.0, 10.0)];
    assert_eq!(trip.state_code_at(5.0).as_deref(), Some("WY"));
    trip.weather.current = WeatherKind::Snow;
    trip.truck.tire_type = "all_season".to_string();
    assert!(!trip.rig_compliant_at(5.0));
    assert_eq!(trip.chain_law_need_at(5.0), "snow tires or chains");
    trip.truck.tire_type = TIRE_WINTER.to_string();
    assert!(trip.rig_compliant_at(5.0));
    // Level 2: snow tires don't count.
    trip.weather.current = WeatherKind::Ice;
    assert!(!trip.rig_compliant_at(5.0));
    assert_eq!(trip.chain_law_need_at(5.0), "chains");
    trip.truck.chains_on = true;
    assert!(trip.rig_compliant_at(5.0));
}

#[test]
fn test_a_montana_chain_season_runs_october_to_april() {
    use ff_core::sim::trip_models::mt_chain_season_id;
    // Career hours for a day of the year (day 80 is hour 0).
    let at = |doy: f64, year: f64| ((doy - 80.0) + 365.0 * year) * 24.0 + 12.0;
    let october = mt_chain_season_id(at(288.0, 0.0));
    assert_eq!(mt_chain_season_id(at(100.0, 1.0)), october); // next April
    assert_eq!(mt_chain_season_id(at(290.0, 1.0)), october + 1); // next October
}

/// Career hours at noon on a day of the career year (day 80 is hour 0).
fn on_day(doy: f64) -> Option<f64> {
    Some((doy - 80.0) * 24.0 + 12.0)
}

#[test]
fn test_carry_corridors_follow_the_carry_laws_and_the_corridor() {
    // Colorado I-70 is a carry corridor: October yes, July no.
    // The grade from mile 12 puts the chain-up area at 11, past exit 263.
    let mut co = trip(Some((12.0, 16.0)));
    co.career_hours = on_day(288.0); // October 15
    let carry = co.chain_carry_corridors();
    let ahead = carry.first().expect("I-70 in October");
    assert_eq!(ahead.highway, "I-70");
    assert_eq!(ahead.state, "CO");
    assert_eq!(
        ahead.key,
        format!("CO:I-70:{}", ff_core::pyfmt::fmt_f(ahead.start_mi, 1))
    );
    // The label is the real interchange's, or empty: never made up.
    let leg = &co.route.legs[0];
    let mapped = leg
        .interchanges()
        .iter()
        .any(|ix| !ix.exit_label().is_empty());
    assert!(mapped, "the Denver-Silverthorne leg maps its exits");
    assert_eq!(ahead.exit_label, co.chain_area_exit_label(ahead.start_mi));
    assert_eq!(ahead.exit_label, "exit 263", "{ahead:?}");
    co.career_hours = on_day(196.0); // July 15
    assert!(co.chain_carry_corridors().is_empty());
    // A dry October day posts no law, so nothing to remind on the road.
    co.career_hours = on_day(288.0);
    assert!(co.upcoming_chain_corridor(15.0).is_none());

    // Montana: April yes, May no.
    let mut mt = trip_on("Missoula", "Butte", None);
    mt.chain_law_areas = vec![(5.0, 10.0)];
    assert_eq!(mt.state_code_at(6.0).as_deref(), Some("MT"));
    mt.career_hours = on_day(100.0); // April 10
    assert_eq!(mt.chain_carry_corridors().len(), 1);
    mt.career_hours = on_day(135.0); // May 15
    assert!(mt.chain_carry_corridors().is_empty());

    // Idaho has no carry law: a dry January on I-90 is quiet, a snowy one
    // reminds on the road; I-84 is not a chain corridor at all.
    let mut i84 = trip_on("Twin Falls", "Boise", None);
    assert_eq!(i84.route.legs[0].highway, "I-84");
    i84.chain_law_areas = vec![(5.0, 10.0)];
    i84.career_hours = on_day(15.0 + 365.0);
    i84.weather.current = WeatherKind::Snow;
    assert!(i84.upcoming_chain_corridor(15.0).is_none());
    let mut i90 = trip_on("Coeur d'Alene", "Kellogg", None);
    assert_eq!(i90.route.legs[0].highway, "I-90");
    i90.chain_law_areas = vec![(5.0, 10.0)];
    i90.career_hours = on_day(15.0 + 365.0); // January 15
    assert_eq!(i90.state_code_at(6.0).as_deref(), Some("ID"));
    assert!(i90.chain_carry_corridors().is_empty());
    i90.weather.current = WeatherKind::Clear;
    assert!(i90.upcoming_chain_corridor(15.0).is_none());
    i90.weather.current = WeatherKind::Snow;
    assert!(i90.upcoming_chain_corridor(15.0).is_some());
    // Out of reach: nothing.
    assert!(i90.upcoming_chain_corridor(1.0).is_none());

    // A leg with no numbered interchange gives an empty label.
    let w = world();
    let route = first_route_option(w, "Denver", "Silverthorne");
    let bare = with_corridor(&route.legs[0], |detail| detail.interchanges.clear());
    let route = replace_leg(&route, 0, bare);
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    let mut none = Trip::new(
        route,
        truck,
        weather("great_lakes", 1),
        TripOptions {
            seed: Some(2),
            world: Some(w),
            ..Default::default()
        },
    );
    none.chain_law_areas = vec![(9.0, 14.0)];
    none.career_hours = on_day(288.0);
    let carry = none.chain_carry_corridors();
    let ahead = carry.first().expect("still a corridor");
    assert_eq!(ahead.exit_label, "");
}

#[test]
fn test_an_early_storm_reminds_out_of_season_and_a_bare_day_does_not() {
    let mut ca = trip_on("Sacramento", "Reno", None);
    ca.chain_law_areas = vec![(60.0, 70.0)];
    assert_eq!(ca.state_code_at(61.0).as_deref(), Some("CA"));
    assert_eq!(ca.route.legs[0].highway, "I-80");
    ca.position_mi = 50.0;
    ca.career_hours = on_day(258.0); // September 15
    ca.weather.current = WeatherKind::Snow;
    assert!(ca.chain_law_level() > 0);
    assert!(ca.upcoming_chain_corridor(15.0).is_some());
    ca.weather.current = WeatherKind::Clear;
    assert_eq!(ca.chain_law_level(), 0);
    assert!(ca.upcoming_chain_corridor(15.0).is_none());
}

#[test]
fn test_a_tandem_rig_is_not_reminded_where_level_1_lets_it_drive_on() {
    // Colorado Level 1 lets a commercial rig on with good tires.
    let mut co = trip(Some((12.0, 16.0)));
    co.career_hours = on_day(196.0); // July: no carry season either
    co.weather.current = WeatherKind::Snow;
    assert_eq!(co.chain_law_level(), 1);
    assert!(co.upcoming_chain_corridor(15.0).is_none());
    co.weather.current = WeatherKind::Ice;
    assert_eq!(co.chain_law_level(), 2);
    assert!(co.upcoming_chain_corridor(15.0).is_some());
}

#[test]
fn test_carry_seasons_are_colorado_montana_and_washington_only() {
    use ff_core::sim::trip_models::in_chain_carry_season;
    assert!(in_chain_carry_season("WA", "I-90", 11));
    assert!(in_chain_carry_season("WA", "I-90", 3));
    assert!(!in_chain_carry_season("WA", "I-90", 10));
    assert!(!in_chain_carry_season("WA", "I-90", 4));
    assert!(in_chain_carry_season("CO", "I-70", 9));
    assert!(!in_chain_carry_season("CO", "I-70", 7));
    assert!(in_chain_carry_season("MT", "I-90", 10));
    assert!(!in_chain_carry_season("MT", "I-90", 5));
    for state in ["CA", "OR", "ID", "NV", "UT", "WY"] {
        assert!(!in_chain_carry_season(state, "I-80", 1), "{state}");
    }
}

#[test]
fn test_oregon_level_1_chains_only_single_drive_axle_trucks() {
    use ff_core::sim::trip_models::{chain_law_sign_rule, rig_must_chain};
    // ODOT: "Chains or traction tires required" chains single-drive-axle
    // trucks over 10,000 lb; the player's tandem drive chains at Level 2.
    assert!(!rig_must_chain(Some("OR"), 1));
    assert!(rig_must_chain(Some("OR"), 2));
    assert_eq!(
        chain_law_sign_rule(Some("OR"), 1),
        "level 1: chains required on single-drive-axle trucks"
    );
    assert_eq!(
        chain_law_sign_rule(Some("OR"), 2),
        "level 2: chains required on all trucks"
    );
}
