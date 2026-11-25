//! The border weight on Canadian carriers' boards (`CANADIAN_BORDER_WEIGHT`,
//! director ruling 2026-11-25): the reference fleet's US share of loads sits
//! inside the range Statistics Canada table 23-10-0219-01 gives for 2014 to
//! 2018, and nothing a US-based carrier draws moves.

use ff_core::data::world::World;
use ff_core::models::carriers::{carrier_catalog, is_offerable_home_city};
use ff_core::models::credentials::CREDENTIALS;
use ff_core::models::jobs::{JobBoard, OfferOptions, CANADIAN_BORDER_WEIGHT};

use crate::data_support::world;

/// The 2018 transborder share of long-distance for-hire shipments.
const SURVEY_SHARE: f64 = 0.192;
/// The table's own 2014 to 2018 range of that share.
const SURVEY_RANGE: (f64, f64) = (0.190, 0.215);
const LEVELS: [i64; 3] = [1, 8, 20];
const SEEDS: i64 = 20;
const OFFERS: usize = 8;

/// Every certificate granted by `level`, as a career that took them all.
fn held(level: i64) -> Vec<&'static str> {
    CREDENTIALS
        .iter()
        .filter(|c| c.grant_level.is_some_and(|g| g <= level))
        .map(|c| c.key)
        .collect()
}

/// The careers the picker offers in Canada: each Canadian carrier with each
/// city where it hires and the home rule passes.
fn reference_fleet(world: &World) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut carriers: Vec<_> = carrier_catalog()
        .values()
        .filter(|c| c.home_country() == "CA")
        .collect();
    carriers.sort_by(|a, b| a.key.cmp(&b.key));
    let mut cities: Vec<&String> = world.cities.keys().collect();
    cities.sort();
    for c in carriers {
        for city in &cities {
            if c.hires_in(world, city) && is_offerable_home_city(world, city) {
                pairs.push((c.key.to_string(), city.to_string()));
            }
        }
    }
    pairs
}

/// One board cell: a carrier at a city and level, over every seed.
#[derive(Debug, Clone, Copy, Default)]
struct Cell {
    loads: u32,
    us_loads: u32,
    miles: f64,
    us_miles: f64,
}

fn measure_cell(world: &World, carrier: &str, city: &str, level: i64, weight: f64) -> Cell {
    let creds = held(level);
    let mut cell = Cell::default();
    for seed in 0..SEEDS {
        let jobs = JobBoard::seeded(world, seed)
            .with_border_weight(weight)
            .offers(
                city,
                &creds,
                OfferOptions {
                    count: OFFERS,
                    level,
                    carrier_key: Some(carrier),
                    ..Default::default()
                },
            );
        for job in jobs {
            let us = world.cities[&job.destination]
                .country
                .eq_ignore_ascii_case("US");
            cell.loads += 1;
            cell.miles += job.distance_mi;
            if us {
                cell.us_loads += 1;
                cell.us_miles += job.distance_mi;
            }
        }
    }
    cell
}

/// The fleet's US share of loads and of loaded miles, each the mean over the
/// pairs and levels in equal parts (a cell with no loads offers no share).
struct Fleet {
    load_share: f64,
    mile_share: f64,
    cells: usize,
}

fn measure_fleet(world: &'static World, weight: f64) -> Fleet {
    let pairs = reference_fleet(world);
    let jobs: Vec<(String, String, i64)> = pairs
        .iter()
        .flat_map(|(c, city)| LEVELS.iter().map(move |l| (c.clone(), city.clone(), *l)))
        .collect();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(4);
    let chunk = jobs.len().div_ceil(threads).max(1);
    let cells: Vec<Cell> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .map(|(c, city, level)| measure_cell(world, c, city, *level, weight))
                        .collect::<Vec<Cell>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a measuring thread"))
            .collect()
    });
    let counted: Vec<&Cell> = cells.iter().filter(|c| c.loads > 0).collect();
    let n = counted.len().max(1) as f64;
    Fleet {
        load_share: counted
            .iter()
            .map(|c| f64::from(c.us_loads) / f64::from(c.loads))
            .sum::<f64>()
            / n,
        mile_share: counted
            .iter()
            .map(|c| if c.miles > 0.0 { c.us_miles / c.miles } else { 0.0 })
            .sum::<f64>()
            / n,
        cells: counted.len(),
    }
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "slow: draws every Canadian career's board at three levels"
)]
fn the_reference_fleet_sits_in_the_survey_range() {
    let world = world();
    let fleet = measure_fleet(world, CANADIAN_BORDER_WEIGHT);
    println!(
        "w {CANADIAN_BORDER_WEIGHT}: fleet US share {:.2}% of loads, {:.2}% of loaded miles, {} cells",
        fleet.load_share * 100.0,
        fleet.mile_share * 100.0,
        fleet.cells
    );
    assert!(
        (SURVEY_RANGE.0..=SURVEY_RANGE.1).contains(&fleet.load_share),
        "the Canadian fleet's US share of loads is {:.2}%, outside the survey's 19.0 to 21.5%",
        fleet.load_share * 100.0
    );
}

/// Reproduces `CANADIAN_BORDER_WEIGHT`: the bisection the doc comment names.
#[test]
#[ignore = "calibration: run by hand when the carriers or the map change"]
fn calibrate_the_canadian_border_weight() {
    let world = world();
    let at_one = measure_fleet(world, 1.0);
    println!(
        "CALIBRATE w 1: {:.2}% of loads, {:.2}% of loaded miles",
        at_one.load_share * 100.0,
        at_one.mile_share * 100.0
    );
    if at_one.load_share <= SURVEY_SHARE {
        println!("CALIBRATE the fleet is at or under the survey with no weight: w = 1");
        return;
    }
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    while hi - lo > 0.002 {
        let mid = (lo + hi) / 2.0;
        let share = measure_fleet(world, mid).load_share;
        println!("CALIBRATE w {mid:.4}: {:.2}%", share * 100.0);
        if share > SURVEY_SHARE {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let w = ((lo + hi) / 2.0 * 100.0).round() / 100.0;
    let fleet = measure_fleet(world, w);
    println!(
        "CALIBRATE w = {w:.2}: {:.2}% of loads, {:.2}% of loaded miles",
        fleet.load_share * 100.0,
        fleet.mile_share * 100.0
    );
}

/// A US-based carrier's boards draw exactly what they drew before the weight,
/// in Canada and in the United States, and so does a Canadian carrier's board
/// in a US city.
#[test]
fn the_border_weight_leaves_every_other_board_as_it_was() {
    let world = world();
    let boards = [
        ("northstar", "chicago_il_us"),
        ("northstar", "detroit_mi_us"),
        ("shiawassee_freight", "detroit_mi_us"),
        ("chehalis_freight", "seattle_wa_us"),
        ("longspur_transport", "chicago_il_us"),
        ("liard_river_freight", "fairbanks_ak_us"),
    ];
    for (carrier, city) in boards {
        for level in LEVELS {
            let creds = held(level);
            let draw = |weight: f64| -> Vec<String> {
                JobBoard::seeded(world, 7)
                    .with_border_weight(weight)
                    .offers(
                        city,
                        &creds,
                        OfferOptions {
                            count: OFFERS,
                            level,
                            carrier_key: Some(carrier),
                            ..Default::default()
                        },
                    )
                    .into_iter()
                    .map(|j| format!("{j:?}"))
                    .collect()
            };
            assert_eq!(draw(1.0), draw(0.05), "{carrier} at {city}, level {level}");
        }
    }
}

/// The weight lowers the US share of a Canadian carrier's board in Canada.
#[test]
fn the_border_weight_thins_a_canadian_boards_us_loads() {
    let world = world();
    let before = measure_cell(world, "longspur_transport", "toronto_on_ca", 20, 1.0);
    let after = measure_cell(world, "longspur_transport", "toronto_on_ca", 20, 0.05);
    assert!(
        after.us_loads < before.us_loads,
        "{} US loads with the weight, {} without",
        after.us_loads,
        before.us_loads
    );
}
