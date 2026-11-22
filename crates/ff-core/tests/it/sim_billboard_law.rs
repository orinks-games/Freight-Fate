//! Billboards stand only where each state's or province's sign law lets
//! them (`data/billboard_law.rs`), and never in a park the road crosses.
//!
//! The Yukon licenses boards only for traveler attractions and services, so
//! its roads hear no everyday pool sign; Alaska bans them; British Columbia
//! licenses them off the right of way, except in its parks, which are
//! billboard bans on the legs.

use ff_core::data::world_models::Route;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::TruckState;

use crate::sim_support::{route_from_cities, weather, world};

fn trip_through(cities: &[&str], seed: i64) -> Trip {
    let route: Route = route_from_cities(world(), cities);
    Trip::new(
        route,
        TruckState::default(),
        weather("great_lakes", 1),
        TripOptions {
            world: Some(world()),
            ..TripOptions::seeded(seed)
        },
    )
}

/// The Alaska Highway from Fort Nelson to Tok: British Columbia, the Yukon
/// and Alaska, with Muncho Lake, Stone Mountain and Kluane along it.
const ALCAN: [&str; 4] = [
    "fort_nelson_bc_ca",
    "watson_lake_yt_ca",
    "whitehorse_yt_ca",
    "tok_ak_us",
];

#[test]
fn test_pool_billboards_keep_to_where_the_law_allows_them() {
    let mut in_bc = 0;
    for seed in 1..=12 {
        for cities in [ALCAN.to_vec(), ALCAN.iter().rev().copied().collect()] {
            let trip = trip_through(&cities, seed);
            for sign in &trip.billboards {
                let law = trip.board_law_at(sign.at_mi);
                assert!(
                    law.allows_pool(),
                    "{cities:?} seed {seed}: {} at {:.1} where {law:?}",
                    sign.spoken,
                    sign.at_mi
                );
                let state = trip.state_at(Some(sign.at_mi));
                assert_eq!(
                    state, "British Columbia",
                    "{} heard in {state}",
                    sign.spoken
                );
                in_bc += 1;
            }
        }
    }
    assert!(
        in_bc > 0,
        "British Columbia's open stretches lost their signs too"
    );
}

#[test]
fn test_parks_and_the_yukon_read_as_their_law() {
    use ff_core::data::billboard_law::BoardLaw;
    let trip = trip_through(&ALCAN, 1);
    // Mile 140 from Fort Nelson is on the shore of Muncho Lake.
    assert_eq!(trip.board_law_at(140.0), BoardLaw::Banned);
    // Mile 100: between Stone Mountain and Muncho Lake, in British Columbia.
    assert_eq!(trip.board_law_at(100.0), BoardLaw::Open);
    // Teslin, in the Yukon.
    let teslin = trip.leg_starts[1] + 162.0;
    assert_eq!(trip.board_law_at(teslin), BoardLaw::AttractionsOnly);
}
