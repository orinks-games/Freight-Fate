//! `--playtest-road --find departure`: enumerate the current data's loaded
//! facility exits, then begin one at its real gate with speed keeper armed.

use ff_core::data::world::{get_world, World};
use ff_core::models::jobs::JobBoard;

use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::playtest::road::{
    find_feature_seeded, plan, route_pairs, Hit, RoadOptions, RoadPlan, FEATURES, RANDOM_ROUTES,
};

const TRIP_SEED: i64 = 20260827;

fn departure_options() -> RoadOptions {
    RoadOptions {
        feature: "departure".to_string(),
        routes: RANDOM_ROUTES.to_string(),
        trip_seed: Some(TRIP_SEED),
        ..Default::default()
    }
}

fn departure_hits() -> Vec<Hit> {
    let opts = departure_options();
    find_feature_seeded(get_world(), &[], "departure", &opts)
}

fn catalog_can_load_to(world: &World, origin: &str, facility: &str, destination: &str) -> bool {
    let Ok(origin_location) = world.facility_location(origin, facility) else {
        return false;
    };
    let Ok(destination_city) = world.city(destination) else {
        return false;
    };
    JobBoard::cargo_for_location(origin_location, "ships", Some(1))
        .into_iter()
        .any(|cargo| {
            destination_city.locations.iter().any(|location| {
                JobBoard::cargo_for_location(location, "receives", Some(1)).contains(&cargo)
            })
        })
}

#[test]
fn departure_is_a_named_finder_feature() {
    assert!(FEATURES.contains(&"departure"));
    assert_eq!(FEATURES.len(), 17);
}

#[test]
fn sampled_departure_finder_returns_only_catalog_backed_facility_on_ramps() {
    let world = get_world();
    let hits = departure_hits();

    assert!(
        !hits.is_empty(),
        "world data has no loaded departure candidates"
    );
    for hit in &hits {
        let facility = hit
            .origin_location
            .as_deref()
            .expect("departure hit names its origin facility");
        let origin = world.resolve_city_key(&hit.origin);
        let destination = world.resolve_city_key(&hit.destination);
        assert!(
            world
                .facility_departure_route(&origin, facility)
                .expect("facility data reads")
                .is_some(),
            "{hit:#?}"
        );
        assert!(
            catalog_can_load_to(world, &origin, facility, &destination),
            "{hit:#?}"
        );
        assert!(hit.run_mi > 0.0, "{hit:#?}");
        assert!(hit.label.starts_with("merge onto "), "{hit:#?}");
        assert!(hit.describe().contains(facility), "{hit:#?}");
    }
}

#[test]
fn departure_finder_order_and_indices_are_stable() {
    let first = departure_hits();
    let second = departure_hits();

    assert_eq!(first, second);
    assert!(first.iter().all(|hit| hit.trip_seed == Some(TRIP_SEED)));
    assert!(first.windows(2).all(|pair| {
        (
            &pair[0].origin,
            pair[0].origin_location.as_deref(),
            &pair[0].destination,
            &pair[0].label,
        ) <= (
            &pair[1].origin,
            pair[1].origin_location.as_deref(),
            &pair[1].destination,
            &pair[1].label,
        )
    }));
}

#[test]
fn departure_pick_launches_the_matching_scanned_candidate() {
    let hits = departure_hits();
    let pick = if hits.len() > 1 { 1 } else { 0 };
    let mut opts = departure_options();
    opts.pick = pick;

    match plan(&opts) {
        RoadPlan::Drive(hit) => assert_eq!(hit, hits[pick]),
        RoadPlan::Done(status) => panic!("--pick {pick} did not launch: status {status}"),
    }
}

#[test]
fn sampled_departure_discovery_stays_inside_its_bounded_route_set() {
    let world = get_world();
    let opts = RoadOptions {
        feature: "departure".to_string(),
        routes: RANDOM_ROUTES.to_string(),
        trip_seed: Some(TRIP_SEED),
        ..Default::default()
    };
    let pairs = route_pairs(world, &opts);
    let allowed: std::collections::HashSet<(String, String)> = pairs
        .into_iter()
        .map(|(origin, destination)| {
            (
                world.resolve_city_key(&origin),
                world.resolve_city_key(&destination),
            )
        })
        .collect();
    let hits = find_feature_seeded(world, &[], "departure", &opts);

    assert!(!hits.is_empty(), "sampled routes found no loaded departure");
    assert!(hits.iter().all(|hit| {
        allowed.contains(&(
            world.resolve_city_key(&hit.origin),
            world.resolve_city_key(&hit.destination),
        ))
    }));
}

#[test]
fn direct_departure_selector_launches_without_repeating_world_discovery() {
    let world = get_world();
    let discovery = RoadOptions {
        feature: "departure".to_string(),
        routes: RANDOM_ROUTES.to_string(),
        trip_seed: Some(TRIP_SEED),
        ..Default::default()
    };
    let expected = find_feature_seeded(world, &[], "departure", &discovery)
        .into_iter()
        .next()
        .expect("sampled routes contain a loaded departure");
    let mut direct = departure_options();
    direct.origin = Some(expected.origin.clone());
    direct.destination = Some(expected.destination.clone());
    direct.facility = expected.origin_location.clone();

    match plan(&direct) {
        RoadPlan::Drive(hit) => assert_eq!(hit, expected),
        RoadPlan::Done(status) => panic!("direct departure selector failed: status {status}"),
    }
}

#[test]
fn selected_departure_starts_loaded_on_its_real_facility_chain_with_speed_keeper_ready() {
    let opts = departure_options();
    let hit = departure_hits()
        .into_iter()
        .next()
        .expect("a current-world departure candidate");
    let facility = hit
        .origin_location
        .clone()
        .expect("selected hit retains its facility");
    let mut harness = PlaytestHarness::new();
    let start_mi = harness.start_road_feature(&hit, &opts);

    assert_eq!(start_mi, 0.0);
    assert!(harness.app.ctx.settings.speed_keeper);
    harness.with_drive(|drive, _| {
        assert_eq!(drive.job.origin_location, facility);
        assert!(drive.speed_control_armed);
        assert_eq!(drive.trip.position_mi, 0.0);
        assert_eq!(drive.truck().speed_mph(), 0.0);
    });

    harness.advance_frame_clock();
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, 1.0 / 60.0));
    harness.with_drive(|drive, _| {
        assert!(
            drive.departure_chain,
            "the real departure chain did not start"
        );
        assert!(
            drive.highway_trip.is_some(),
            "the highway trip was not held for the merge"
        );
    });
}

/// An origin with no destination must anchor the route enumeration, not
/// filter it afterwards: `all_world_pairs` resolves every ordered city pair
/// (388,752 route lookups at 624 cities), which is why the agent server's
/// `start_at origin:"Denver"` hung for minutes with no progress. Anchored,
/// the same call is at most one lookup per other city and answers in
/// seconds -- and every pair it returns really starts at the anchor and is
/// a supported route.
#[test]
fn origin_only_route_pairs_stay_anchored_and_answer_fast() {
    let world = get_world();
    let opts = RoadOptions {
        feature: "departure".to_string(),
        origin: Some("Denver".to_string()),
        trip_seed: Some(TRIP_SEED),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let pairs = route_pairs(world, &opts);
    let elapsed = started.elapsed();

    assert!(!pairs.is_empty(), "Denver has no supported routes at all?");
    let wanted = world.resolve_city_key("Denver");
    for (from, to) in &pairs {
        assert_eq!(
            world.resolve_city_key(from),
            wanted,
            "a pair leaked in from another origin: {from} -> {to}"
        );
        assert!(
            world
                .supported_route(from, to, None)
                .ok()
                .flatten()
                .is_some(),
            "unsupported route returned: {from} -> {to}"
        );
    }
    // Generous ceiling: the anchored path is a few hundred lookups. The
    // quadratic path this guards against took minutes, so a bound this loose
    // still fails it immediately without ever flaking on a slow machine.
    assert!(
        elapsed.as_secs() < 60,
        "origin-anchored route_pairs took {elapsed:?}; the quadratic fan is back"
    );
}
