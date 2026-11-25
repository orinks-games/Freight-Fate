//! Loads across the Canadian border on the real map: where the board sends
//! them, who may carry them, and what their deadline books.

use super::*;
use crate::data::world::get_world;

const ALL: &[&str] = &[
    "refrigerated",
    "heavy_haul",
    "high_value",
    "hazmat",
    "doubles_triples",
    "tanker",
];

fn board_at(city: &str, seed: i64, level: i64, carrier: &str) -> Vec<Job> {
    JobBoard::seeded(get_world(), seed).offers(
        city,
        ALL,
        OfferOptions {
            count: 8,
            level,
            carrier_key: Some(carrier),
            ..Default::default()
        },
    )
}

fn country(city: &str) -> String {
    get_world().city(city).expect("a map city").country.clone()
}

#[test]
fn a_board_near_the_border_offers_canadian_destinations() {
    let canadian: usize = (0..40)
        .flat_map(|seed| board_at("bellingham_wa_us", seed, 8, "northstar"))
        .filter(|job| country(&job.destination) == "CA")
        .count();
    assert!(
        canadian > 0,
        "40 Bellingham boards offered nothing in Canada"
    );
}

#[test]
fn a_canadian_board_offers_the_united_states_only() {
    let mut seen = 0;
    for seed in 0..40 {
        for job in board_at("surrey_bc_ca", seed, 8, "northstar") {
            assert_eq!(country(&job.destination), "US", "seed {seed}: {job:?}");
            seen += 1;
        }
    }
    assert!(seen > 0, "Surrey's port posted no loads");
}

#[test]
fn a_carrier_without_canadian_authority_stays_home() {
    for seed in 0..30 {
        for job in board_at("seattle_wa_us", seed, 8, "pinyon_basin") {
            assert_eq!(country(&job.destination), "US", "seed {seed}");
        }
    }
}

#[test]
fn placards_twins_and_cattle_never_cross() {
    for city in [
        "bellingham_wa_us",
        "seattle_wa_us",
        "spokane_wa_us",
        "surrey_bc_ca",
    ] {
        for seed in 0..40 {
            for job in board_at(city, seed, 18, "northstar") {
                if country(&job.origin) == country(&job.destination) {
                    continue;
                }
                assert!(
                    job.hazmat.is_none(),
                    "{city} seed {seed}: placarded {job:?}"
                );
                assert!(!job.cargo.national_network, "{city} seed {seed}: twins");
                assert_ne!(job.cargo.key, "livestock", "{city} seed {seed}: cattle");
            }
        }
    }
}

#[test]
fn a_border_load_books_the_customs_booth() {
    let world = get_world();
    let route = world
        .supported_route("seattle_wa_us", "surrey_bc_ca", None)
        .ok()
        .flatten()
        .expect("Seattle reaches Surrey");
    assert_eq!(cross_border::route_crossings(&route), 1);
    let plan = plan_hos(route.miles(), Some(&route), Some(world), None);
    assert!((plan.customs_h - cross_border::CUSTOMS_PLANNED_H).abs() < 1e-9);
    assert!(
        (plan.total_h()
            - plan.drive_h
            - plan.breaks as f64 * 0.5
            - plan.sleeps as f64 * 10.0
            - plan.customs_h)
            .abs()
            < 1e-9
    );
    // A home lane books none.
    let home = world
        .supported_route("seattle_wa_us", "portland_or_us", None)
        .ok()
        .flatten()
        .expect("Seattle reaches Portland");
    assert_eq!(
        plan_hos(home.miles(), Some(&home), Some(world), None).customs_h,
        0.0
    );
}

#[test]
fn a_long_haul_to_whitehorse_plans_enough_for_either_country() {
    // Seattle to Whitehorse is about 1,500 miles, nearly all in Canada.
    // Planned on US rules, the sleeps the plan books are at least as many
    // as the Canadian clock (13 driving hours a shift south of 60) needs.
    let world = get_world();
    let route = world
        .supported_route("seattle_wa_us", "whitehorse_yt_ca", None)
        .ok()
        .flatten()
        .expect("Seattle reaches Whitehorse");
    let plan = plan_hos(route.miles(), Some(&route), Some(world), None);
    assert!(plan.sleeps >= 1, "{plan:?}");
    let canadian_sleeps = ((plan.drive_h / 13.0).ceil() as i64 - 1).max(0);
    assert!(plan.sleeps >= canadian_sleeps, "{plan:?}");
    assert_eq!(cross_border::route_crossings(&route), 1);
}

/// The West corridor's prairie and mountain cities are as open as the Alcan
/// towns: a board on the Montana or North Dakota side posts loads into
/// Alberta, Saskatchewan or Manitoba, and a board there posts only US loads.
#[test]
fn the_western_crossings_carry_loads_both_ways() {
    let western: usize = ["great_falls_mt_us", "minot_nd_us"]
        .iter()
        .flat_map(|city| (0..40).flat_map(move |seed| board_at(city, seed, 8, "northstar")))
        .filter(|job| {
            matches!(
                get_world()
                    .city(&job.destination)
                    .ok()
                    .map(|c| c.state.as_str()),
                Some("AB" | "SK" | "MB" | "Alberta" | "Saskatchewan" | "Manitoba")
            )
        })
        .count();
    assert!(
        western > 0,
        "no prairie load from Great Falls or Minot in 40 seeds"
    );
    for city in [
        "calgary_ab_ca",
        "regina_sk_ca",
        "winnipeg_mb_ca",
        "toronto_on_ca",
        "montreal_qc_ca",
    ] {
        let mut seen = 0;
        for seed in 0..20 {
            for job in board_at(city, seed, 8, "northstar") {
                assert_eq!(
                    country(&job.destination),
                    "US",
                    "{city} seed {seed}: {job:?}"
                );
                seen += 1;
            }
        }
        assert!(seen > 0, "{city} posted no loads");
    }
}

/// Skagway's only road out runs through Canada: its board still posts
/// loads, Whitehorse among them, and every route to a US city is allowed to
/// pass through.
#[test]
fn skagway_posts_loads_over_the_klondike() {
    let jobs: Vec<Job> = (0..40)
        .flat_map(|seed| board_at("skagway_ak_us", seed, 8, "chatanika_freight"))
        .collect();
    assert!(!jobs.is_empty(), "Skagway posted nothing in 40 seeds");
    assert!(
        jobs.iter().any(|job| country(&job.destination) == "CA"),
        "no load from Skagway into Canada in 40 seeds"
    );
}

/// The board numbers in the cross-border report: how often a border load is
/// posted at a few cities, over seeds. Run by hand:
/// `cargo test -p ff-core cross_border_probe -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement for the report, not a check"]
fn cross_border_probe() {
    let cities = [
        ("bellingham_wa_us", "northstar"),
        ("seattle_wa_us", "chehalis_freight"),
        ("seattle_wa_us", "northstar"),
        ("spokane_wa_us", "chehalis_freight"),
        ("fairbanks_ak_us", "chatanika_freight"),
        ("chicago_il_us", "northstar"),
        ("dallas_tx_us", "northstar"),
        ("surrey_bc_ca", "northstar"),
        ("whitehorse_yt_ca", "northstar"),
        ("great_falls_mt_us", "musselshell_freight"),
        ("minot_nd_us", "loonwater_regional"),
        ("fargo_nd_us", "loonwater_regional"),
        ("vancouver_bc_ca", "northstar"),
        ("calgary_ab_ca", "northstar"),
        ("winnipeg_mb_ca", "northstar"),
        ("detroit_mi_us", "olentangy_valley"),
        ("buffalo_ny_us", "northstar"),
        ("plattsburgh_ny_us", "sebasticook_freight"),
        ("toronto_on_ca", "northstar"),
        ("montreal_qc_ca", "northstar"),
    ];
    let world = get_world();
    let mut dest: std::collections::BTreeMap<String, usize> = Default::default();
    for city in world.cities.values().filter(|c| c.country == "CA") {
        let receives = city
            .locations
            .iter()
            .filter(|l| !l.receives.is_empty())
            .count();
        let ships = city
            .locations
            .iter()
            .filter(|l| !l.ships.is_empty())
            .count();
        let templates = city.locations.iter().filter(|l| l.template).count();
        println!(
            "site {} receives {receives} ships {ships} templates {templates}",
            city.key
        );
    }
    for level in [3, 8, 14] {
        for (city, carrier) in cities {
            let (mut boards, mut with, mut loads, mut border) = (0, 0, 0, 0);
            for seed in 0..200 {
                let jobs = board_at(city, seed, level, carrier);
                boards += 1;
                let n = jobs
                    .iter()
                    .filter(|j| country(&j.origin) != country(&j.destination))
                    .count();
                loads += jobs.len();
                for j in &jobs {
                    if country(&j.destination) == "CA" && level == 8 {
                        *dest.entry(j.destination.clone()).or_default() += 1;
                    }
                }
                border += n;
                with += usize::from(n > 0);
            }
            println!(
                "level {level:>2} {city:<20} {carrier:<18} boards with a border load {:>5.1}%  border loads {border}/{loads}",
                100.0 * with as f64 / boards as f64
            );
        }
    }
    println!("canadian destinations at level 8: {dest:?}");
    for (city, seed) in [
        ("bellingham_wa_us", 1),
        ("spokane_wa_us", 2),
        ("surrey_bc_ca", 3),
    ] {
        println!("-- {city} seed {seed}, level 8, Northstar");
        for job in board_at(city, seed, 8, "northstar") {
            println!(
                "   {} to {}, {} mi, {} h, {}",
                job.origin, job.destination, job.distance_mi, job.deadline_game_h, job.cargo.key
            );
        }
    }
}

/// What a company driver with no endorsements, one delivery behind them,
/// sees on the board in `city`: the local loads, else a relay, else the
/// carrier's reposition. Empty only when all three are.
fn company_board(city: &str, carrier: &str, level: i64, seed: i64) -> Vec<Job> {
    use super::relay::{fallback_reposition, relay_load, RelayRequest};
    let world = get_world();
    let none: &[&str] = &[];
    let local = JobBoard::seeded(world, seed).offers(
        city,
        none,
        OfferOptions {
            count: board_offer_count(level),
            level,
            carrier_key: Some(carrier),
            ..Default::default()
        },
    );
    if !local.is_empty() {
        return local;
    }
    let mut nearby = reachable_cities(world, city);
    nearby.sort_by(|a, b| a.1.total_cmp(&b.1));
    let request = RelayRequest {
        here: city,
        endorsements: none,
        level,
        market: None,
        carrier_key: Some(carrier),
        direct_freight: false,
        seed,
    };
    relay_load(world, &request, &local, &nearby)
        .map(|relay| relay.job)
        .or_else(|| fallback_reposition(world, &request, &nearby))
        .into_iter()
        .collect()
}

/// Every (Canadian city, carrier, level) where a company driver who
/// delivered there would be stuck: an empty board, or a reposition to a
/// city whose own board is empty. A carrier "delivers there" when one of
/// its US boards can reach the city inside its run band at that level.
fn stranded_in_canada() -> Vec<String> {
    let world = get_world();
    let receives = |key: &str| {
        world.cities.get(key).is_some_and(|city| {
            city.locations
                .iter()
                .any(|loc| !JobBoard::cargo_for_location(loc, "receives", None).is_empty())
        })
    };
    let mut canadian: Vec<String> = world
        .cities
        .iter()
        .filter(|(key, city)| city.country == "CA" && receives(key))
        .map(|(key, _)| key.clone())
        .collect();
    canadian.sort();
    let carriers: Vec<&str> = crate::models::carriers::carrier_catalog()
        .iter()
        .filter(|(_, c)| c.cross_border)
        .map(|(key, _)| key.as_str())
        .collect();
    let mut stranded = Vec::new();
    for city in &canadian {
        let nearby = reachable_cities(world, city);
        // An owner-operator's Bobtail lists the nearest reachable cities,
        // whatever the range.
        if nearby.is_empty() {
            stranded.push(format!("{city}: owner-operator, no city to bobtail to"));
        }
        let nearest_us = nearby
            .iter()
            .filter(|c| c.1 >= MIN_JOB_DISTANCE_MI && country(&c.0) == "US")
            .map(|c| c.1)
            .fold(f64::INFINITY, f64::min);
        for level in [1, 8] {
            for carrier in &carriers {
                if nearest_us > JobBoard::distance_cap_for_carrier(level, carrier) {
                    continue;
                }
                for seed in 0..3 {
                    let board = company_board(city, carrier, level, seed);
                    let Some(first) = board.first() else {
                        stranded.push(format!(
                            "{city}, {carrier}, level {level}, seed {seed}: empty board"
                        ));
                        continue;
                    };
                    if first.bobtail
                        && company_board(&first.destination, carrier, level, seed)
                            .iter()
                            .all(|job| job.bobtail)
                    {
                        stranded.push(format!(
                            "{city}, {carrier}, level {level}, seed {seed}: reposition to {} finds no load",
                            first.destination
                        ));
                    }
                }
            }
        }
    }
    stranded
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "every Canadian city, carrier and level; run with cargo test-all"
)]
fn no_driver_is_left_without_a_load_in_canada() {
    let stranded = stranded_in_canada();
    assert!(stranded.is_empty(), "{}", stranded.join("\n"));
}

#[test]
fn a_driver_who_delivers_to_whitehorse_is_sent_on_empty() {
    let board = company_board("whitehorse_yt_ca", "northstar", 1, 0);
    assert_eq!(board.len(), 1, "{board:?}");
    let reposition = &board[0];
    assert!(reposition.bobtail && reposition.assigned, "{reposition:?}");
    assert!(reposition.pay > 0.0);
    assert!(!company_board(&reposition.destination, "northstar", 1, 0).is_empty());
}

fn in_alaska(city: &str) -> bool {
    get_world().city(city).expect("a map city").state_code == "AK"
}

/// The road from Seattle to Alaska runs through Canada, so a carrier
/// without the authority never posts it, and one with it does.
#[test]
fn alaska_by_road_needs_canadian_authority() {
    let to_alaska = |carrier: &str| -> usize {
        (0..150)
            .flat_map(|seed| board_at("seattle_wa_us", seed, 20, carrier))
            .filter(|job| in_alaska(&job.destination))
            .count()
    };
    assert_eq!(to_alaska("great_lakes_training"), 0);
    let board = JobBoard::new(get_world(), Some(0), None);
    assert!(!board.carrier_may_haul("great_lakes_training", "seattle_wa_us", "anchorage_ak_us"));
    assert!(board.carrier_may_haul("northstar", "seattle_wa_us", "anchorage_ak_us"));
}

/// A load between Alaska and the lower 48 by road is road freight, and
/// never placarded freight.
#[test]
fn a_load_through_canada_is_road_freight() {
    let mut seen = 0;
    for city in ["seattle_wa_us", "bellingham_wa_us", "spokane_wa_us"] {
        for seed in 0..150 {
            for job in board_at(city, seed, 20, "northstar") {
                if !in_alaska(&job.destination) {
                    continue;
                }
                seen += 1;
                assert!(
                    cross_border::road_freight(job.cargo.key),
                    "{city} seed {seed}: {} to {}",
                    job.cargo.key,
                    job.destination
                );
                assert!(job.hazmat.is_none(), "{job:?}");
            }
        }
    }
    assert!(seen > 0, "no load from the Northwest reached Alaska");
}

/// A leased owner-operator start has no carrier record: it takes its
/// lessor's base country, so a start leased to a Canadian carrier gets a
/// Canadian board.
#[test]
fn a_leased_start_takes_its_lessors_base() {
    use crate::models::start_options::{all_start_options, hiring_carrier_for_option};
    let leased: Vec<_> = all_start_options()
        .into_iter()
        .filter(|option| option.is_owner_operator())
        .collect();
    assert!(!leased.is_empty());
    for option in leased {
        let lessor = hiring_carrier_for_option(option).expect("a leased start names its lessor");
        assert_eq!(
            cross_border::carrier_base(option.key),
            lessor.base,
            "{}",
            option.key
        );
    }
}

/// The empty-move list follows the carrier's own lanes: from Vancouver a US
/// carrier never runs empty to another Canadian city (cabotage), while the
/// shared list still holds them for a carrier that may.
#[test]
fn empty_moves_follow_the_carriers_lanes() {
    let world = get_world();
    let all = reachable_cities(world, "vancouver_bc_ca");
    assert!(all.iter().any(|c| country(&c.0) == "CA"));
    let us = crate::models::jobs::reachable_cities_for(world, "vancouver_bc_ca", "northstar");
    assert!(!us.is_empty());
    assert!(us.iter().all(|c| country(&c.0) == "US"));
}

/// A start leased to a Canadian carrier (a fixture until `carriers.json` has
/// one) never hauls between two US cities: in Chicago its board runs to
/// Canada only.
#[test]
fn a_canadian_base_never_hauls_between_two_us_cities() {
    use cross_border::{fixture::with_carrier, CarrierBase};
    with_carrier("leased_to_a_canadian", CarrierBase::Ca, true, || {
        let jobs: Vec<Job> = (0..5)
            .flat_map(|seed| board_at("chicago_il_us", seed, 20, "leased_to_a_canadian"))
            .collect();
        assert!(!jobs.is_empty());
        for job in &jobs {
            assert_eq!(country(&job.destination), "CA", "{job:?}");
        }
    });
}

/// A Canadian carrier stranded in a town with nothing for it is sent empty
/// to the nearest Canadian city with a load, not across the border: Fort St.
/// John to Grande Prairie, not Bellingham.
#[test]
fn a_canadian_carrier_repositions_inside_canada() {
    use cross_border::{fixture::with_carrier, CarrierBase};
    with_carrier("a_canadian_carrier", CarrierBase::Ca, true, || {
        let board = company_board("fort_st_john_bc_ca", "a_canadian_carrier", 1, 0);
        assert_eq!(board.len(), 1, "{board:?}");
        let reposition = &board[0];
        assert!(reposition.bobtail && reposition.assigned, "{reposition:?}");
        assert_eq!(country(&reposition.destination), "CA", "{reposition:?}");
        let empty_moves = crate::models::jobs::reachable_cities_for(
            get_world(),
            "fort_st_john_bc_ca",
            "a_canadian_carrier",
        );
        assert!(empty_moves.iter().any(|c| c.0 == "grande_prairie_ab_ca"));
    });
}

/// Anchorage to Skagway runs through the Yukon, but no ship competes with
/// it, so the board offers any freight that may cross on that lane, and
/// still never a placard.
#[test]
fn alaska_to_alaska_through_the_yukon_takes_any_freight() {
    let mut cargo = std::collections::BTreeSet::new();
    for seed in 0..150 {
        for job in board_at("anchorage_ak_us", seed, 20, "northstar") {
            if job.destination == "skagway_ak_us" {
                assert!(job.hazmat.is_none(), "{job:?}");
                cargo.insert(job.cargo.key.to_string());
            }
        }
    }
    assert!(
        cargo.iter().any(|key| !cross_border::road_freight(key)),
        "only road freight reached Skagway: {cargo:?}"
    );
}
