//! Ported from `tests/test_career_start_options.py`: career start choices,
//! carrier benefits, and equipment semantics. The `Profile` cases run against
//! the career-side `FakeProfile` (its `StartProfile` impl mirrors the
//! `profile.py` fields); the business-settlement, job-board and app-shell
//! cases are ignored with the reason and their bodies say what they checked.

use super::*;
use crate::models::business_constants::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use crate::models::career::test_profile::FakeProfile;
use crate::models::trailer_yard::TrailerOwner;

#[test]
fn test_start_options_are_grounded_and_player_facing() {
    let options = all_start_options();
    let company = company_start_options();

    assert!(company.len() >= 3);
    assert_eq!(
        start_option(Some(DEFAULT_START_KEY)).carrier_name,
        "Northstar Freight Lines"
    );
    assert!(options
        .iter()
        .any(|option| option.mode == START_MODE_OWNER_OPERATOR));
    for option in options {
        assert!(!option.label.is_empty());
        assert!(!option.menu_summary.is_empty());
        assert!(!option.help_text.is_empty());
        if option.mode == START_MODE_COMPANY {
            assert!(option.company_pay.is_some());
            assert!(option.owned_trucks.is_empty());
            assert!(option.help_text.to_lowercase().contains("carrier"));
        } else {
            assert_eq!(option.mode, START_MODE_OWNER_OPERATOR);
            assert!(!option.owned_trucks.is_empty());
            assert!(option.starting_money > 0.0);
            assert!(option.help_text.contains("operating costs"));
        }
    }
}

/// `_job(miles, pay)`: a general-freight Chicago to Milwaukee load.
fn real_job(miles: f64, pay: f64) -> crate::models::jobs::Job {
    crate::models::jobs::Job::new(
        crate::models::jobs::cargo_type("general").unwrap(),
        12.0,
        "Chicago",
        "Chicago yard",
        "Milwaukee",
        miles,
        pay,
        6.0,
    )
}

#[test]
fn test_company_carrier_pay_plans_have_distinct_benefits() {
    use crate::models::business::company_driver_pay;
    let short_job = real_job(80.0, 600.0);
    let long_floor_job = real_job(500.0, 500.0);
    let high_gross_job = real_job(300.0, 3000.0);

    let northstar = company_driver_pay(
        &short_job,
        short_job.pay,
        true,
        Some(DEFAULT_START_KEY),
        None,
    );
    let training = company_driver_pay(
        &short_job,
        short_job.pay,
        true,
        Some("great_lakes_training"),
        None,
    );
    assert!(training > northstar);

    let northstar_long = company_driver_pay(
        &long_floor_job,
        long_floor_job.pay,
        true,
        Some(DEFAULT_START_KEY),
        None,
    );
    let prairie = company_driver_pay(
        &long_floor_job,
        long_floor_job.pay,
        true,
        Some("prairie_link"),
        None,
    );
    assert!(prairie > northstar_long);

    let northstar_bonus = company_driver_pay(
        &high_gross_job,
        high_gross_job.pay,
        true,
        Some(DEFAULT_START_KEY),
        None,
    );
    let summit = company_driver_pay(
        &high_gross_job,
        high_gross_job.pay,
        true,
        Some("summit_value"),
        None,
    );
    assert!(summit > northstar_bonus);
}

#[test]
fn test_carrier_key_changes_settlement_math() {
    use crate::models::business::{build_business_settlement, SettlementTerms};
    let job = real_job(80.0, 600.0);

    let northstar = build_business_settlement(
        COMPANY_DRIVER,
        &job,
        job.pay,
        true,
        0.0,
        &SettlementTerms {
            carrier_key: Some(DEFAULT_START_KEY),
            ..Default::default()
        },
    );
    let training = build_business_settlement(
        COMPANY_DRIVER,
        &job,
        job.pay,
        true,
        0.0,
        &SettlementTerms {
            carrier_key: Some("great_lakes_training"),
            ..Default::default()
        },
    );

    assert!(training.net_before_advance > northstar.net_before_advance);
    assert!(training.business_charges.is_empty());
}

#[test]
fn test_carrier_key_can_bias_job_mix_weighting() {
    use crate::data::world::get_world;
    use crate::models::jobs::JobBoard;
    let world = get_world();
    let kansas_city = world.city("Kansas City").unwrap();

    let baseline = JobBoard::cargo_weight(kansas_city, "grain", DEFAULT_START_KEY, 1);
    let prairie = JobBoard::cargo_weight(kansas_city, "grain", "prairie_link", 1);

    assert!(prairie > baseline);
}

#[test]
fn test_company_carriers_have_distinct_dispatch_weighting() {
    use crate::data::world::get_world;
    use crate::models::jobs::JobBoard;
    let world = get_world();
    let board = JobBoard::seeded(world, 1);
    let miles_between = |a: &str, b: &str| -> f64 {
        world
            .supported_route(a, b, None)
            .unwrap()
            .expect("a supported route")
            .miles()
    };
    let chicago = world.resolve_city_key("Chicago");
    let milwaukee = world.resolve_city_key("Milwaukee");
    let kc_key = world.resolve_city_key("Kansas City");
    let short = (milwaukee.clone(), miles_between(&chicago, &milwaukee), 1);
    let long = (kc_key.clone(), miles_between(&chicago, &kc_key), 1);

    let northstar_short_ratio =
        board.destination_weight(&chicago, &short, 2, DEFAULT_START_KEY, None)
            / board.destination_weight(&chicago, &long, 2, DEFAULT_START_KEY, None);
    let training_short_ratio =
        board.destination_weight(&chicago, &short, 2, "great_lakes_training", None)
            / board.destination_weight(&chicago, &long, 2, "great_lakes_training", None);
    assert!(training_short_ratio > northstar_short_ratio);

    let origin_region = &world.cities[&kc_key].region;
    let mut same_region = None;
    let mut other_region = None;
    for dest in world.city_names() {
        if dest == kc_key {
            continue;
        }
        let Ok(Some(route)) = world.supported_route(&kc_key, &dest, None) else {
            continue;
        };
        let candidate = (dest.clone(), route.miles(), route.legs.len());
        if &world.cities[&dest].region == origin_region {
            same_region.get_or_insert(candidate);
        } else {
            other_region.get_or_insert(candidate);
        }
        if same_region.is_some() && other_region.is_some() {
            break;
        }
    }
    let same_region = same_region.expect("a same-region destination");
    let other_region = other_region.expect("an other-region destination");
    let prairie_region_ratio =
        board.destination_weight(&kc_key, &same_region, 2, "prairie_link", None)
            / board.destination_weight(&kc_key, &other_region, 2, "prairie_link", None);
    let northstar_region_ratio =
        board.destination_weight(&kc_key, &same_region, 2, DEFAULT_START_KEY, None)
            / board.destination_weight(&kc_key, &other_region, 2, DEFAULT_START_KEY, None);
    assert!(prairie_region_ratio > northstar_region_ratio);

    let cap = JobBoard::distance_cap(5);
    let candidate = (world.resolve_city_key("Los Angeles"), cap, 3);
    let denver = world.resolve_city_key("Denver");
    assert!(
        board.destination_weight(&denver, &candidate, 5, "summit_value", None)
            > board.destination_weight(&denver, &candidate, 5, DEFAULT_START_KEY, None)
    );
}

#[test]
fn test_training_carrier_adds_modest_deadline_slack() {
    use crate::data::world::get_world;
    use crate::models::jobs::{JobBoard, OfferOptions};
    // The same Chicago to Milwaukee offer gets a later deadline_game_h at
    // great_lakes_training than at northstar: `offer_to` at one seed is the
    // `_make_job` call with the carrier's slack applied.
    let world = get_world();
    let no_endorsements: &[&str] = &[];
    let northstar = JobBoard::seeded(world, 8)
        .offer_to(
            "Chicago",
            "Milwaukee",
            no_endorsements,
            OfferOptions {
                carrier_key: Some(DEFAULT_START_KEY),
                ..Default::default()
            },
        )
        .unwrap();
    let training = JobBoard::seeded(world, 8)
        .offer_to(
            "Chicago",
            "Milwaukee",
            no_endorsements,
            OfferOptions {
                carrier_key: Some("great_lakes_training"),
                ..Default::default()
            },
        )
        .unwrap();

    assert!(training.deadline_game_h > northstar.deadline_game_h);
}

#[test]
fn test_apply_company_start_keeps_assigned_equipment() {
    let mut p = FakeProfile::named("Company Choice");
    apply_start_option(&mut p, start_option(Some("great_lakes_training")));

    assert_eq!(p.carrier_name, "Great Lakes Training Transport");
    assert_eq!(p.carrier_key, "great_lakes_training");
    assert_eq!(p.start_mode, START_MODE_COMPANY);
    assert_eq!(p.business_status, COMPANY_DRIVER);
    assert!(p.owned_trucks.is_empty());
    assert!(p.visible_owned_trucks().is_empty());
    assert!(!p.owns_equipment());
}

#[test]
fn test_owner_operator_start_applies_owned_equipment_and_costs() {
    let mut p = FakeProfile::named("Owner Start");
    apply_start_option(&mut p, start_option(Some(OWNER_OPERATOR_START_KEY)));

    assert_eq!(p.start_mode, START_MODE_OWNER_OPERATOR);
    assert_eq!(p.business_status, LEASED_OWNER_OPERATOR);
    assert!(p.owns_equipment());
    assert_eq!(p.visible_owned_trucks(), vec!["rig".to_string()]);
    assert_eq!(p.active_trailer_programs(), vec!["dry_van".to_string()]);
    assert!((p.money - 18_000.0).abs() < 1e-9);
    // The truck is brand new, not a hand-me-down: full tank, no damage.
    assert!((p.truck_damage_pct() - 0.0).abs() < 1e-9);
    assert!((p.truck_fuel_gal() - p.truck_specs().fuel_tank_gal).abs() < 1e-9);
    // The start buys you equipment and costs, never progress: the career
    // begins at zero and climbs the same ladder as a company hire. It used
    // to open at level 18 with 35 deliveries and 42,000 miles behind it.
    assert_eq!(p.career.level(), 1);
    assert_eq!(p.career.deliveries, 0);
    assert_eq!(p.career.total_miles, 0.0);
    assert!(p.career.level() < 18); // the buy-in gate is still ahead
}

/// The owner-operator buys a brand-new truck, so nothing starts worn.
///
/// Compared against a freshly built record rather than a hand-written list of
/// fields: adding a new condition dimension that defaults to a worn value
/// fails here instead of quietly shipping a hand-me-down.
#[test]
fn test_owner_operator_start_truck_is_pristine_on_every_condition_dimension() {
    use crate::models::profile::{fresh_condition, Profile};
    let mut p = Profile::named_in("Pristine Start", "Chicago");
    apply_start_option(&mut p, start_option(Some(OWNER_OPERATOR_START_KEY)));

    let tank = p.truck_specs().fuel_tank_gal;
    let record = &p.truck_conditions["rig"];

    assert_eq!(*record, fresh_condition(tank));
    // Spelled out through the spoken-side accessors too, so a rename that
    // leaves the record intact but detaches a reader still fails.
    assert!((p.truck_fuel_gal() - tank).abs() < 1e-9);
    assert_eq!(p.truck_damage_pct(), 0.0);
    assert_eq!(p.tire_wear_pct(), 0.0);
    assert_eq!(p.brake_wear_pct(), 0.0);
    assert_eq!(p.engine_wear_pct(), 0.0);
    assert_eq!(p.road_grime_pct(), 0.0);
    assert_eq!(p.chain_wear_pct(), 0.0);
    assert_eq!(p.tire_type(), "all_season");
    assert!(!p.chains_owned());
}

#[test]
fn test_owner_operator_start_text_describes_a_new_truck() {
    let option = start_option(Some(OWNER_OPERATOR_START_KEY));
    let blurb = format!("{} {}", option.menu_summary, option.help_text).to_lowercase();

    assert!(blurb.contains("brand-new"));
    assert!(!blurb.contains("starter tractor"));
    // The difficulty still has to come across: the costs and the thin cushion
    // are what make this the hardest start, not a worn-out truck.
    assert!(option.help_text.contains("operating costs"));
    assert!(blurb.contains("working capital"));
    assert!(blurb.contains("hardest"));
}

// `test_new_career_start_menu_lists_company_and_owner_operator` is live in `crates/freight-fate/tests/states_main_menu.rs`.

// `test_new_company_career_choice_creates_company_profile` is live in `crates/freight-fate/tests/states_main_menu.rs`.

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs freight_fate::app (App, TruckShopState, UpgradeShopState)"]
fn test_owner_operator_start_unlocks_equipment_systems() {
    // The owner-operator start leaves a LEASED_OWNER_OPERATOR profile that
    // owns "rig"; the truck shop says "currently driving" and "buy for", and
    // the upgrade shop is not locked.
    unimplemented!("needs the app shell")
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs freight_fate::app (App, CityMenuState, first_day_orientation_message)"]
fn test_first_day_briefing_names_owner_operator_costs() {
    // The orientation names "leased to Northstar Freight Lines", "own a
    // brand-new truck", "working capital", "fuel, repairs, truck wear"; the
    // briefing item gives way to "Career plan" after the first dispatch.
    unimplemented!("needs the app shell")
}

// --- the catalogue, spoken ----------------------------------------------------------

#[test]
fn test_start_option_lookup_falls_back_to_northstar() {
    assert_eq!(start_option(None).key, DEFAULT_START_KEY);
    assert_eq!(start_option(Some("")).key, DEFAULT_START_KEY);
    assert_eq!(start_option(Some("no_such_carrier")).key, DEFAULT_START_KEY);
    assert_eq!(start_option(Some("summit_value")).default_city, "Denver");
    let keys: Vec<&str> = START_OPTIONS.iter().map(|o| o.key).collect();
    assert_eq!(
        keys,
        [
            "northstar",
            "great_lakes_training",
            "prairie_link",
            "summit_value",
            OWNER_OPERATOR_START_KEY
        ]
    );
    assert_eq!(
        pay_plan_for_key(Some(OWNER_OPERATOR_START_KEY)),
        NORTHSTAR_PAY
    );
}

#[test]
fn test_pay_plan_and_dispatch_summaries_are_spoken_plainly() {
    assert_eq!(
        NORTHSTAR_PAY.summary(),
        "36 percent pay share, 0.82 dollars per mile floor, 175 dollar stop pay, 4 percent on-time bonus"
    );
    assert_eq!(
        start_option(Some("great_lakes_training"))
            .dispatch
            .summary(),
        "more short training loads, more appointment slack"
    );
    assert_eq!(start_option(None).dispatch.summary(), "balanced dispatch");
    assert_eq!(DispatchProfile::default(), DispatchProfile::BALANCED);
}

#[test]
fn test_option_for_profile_recovers_an_old_save_from_its_carrier_name() {
    // A save from before carrier keys carries only the carrier's name.
    assert_eq!(
        option_for_carrier("", "Prairie Link Regional").key,
        "prairie_link"
    );
    assert_eq!(
        option_for_carrier("summit_value", "Anything").key,
        "summit_value"
    );
    assert_eq!(option_for_carrier("", "").key, DEFAULT_START_KEY);
    assert_eq!(
        option_for_carrier("", "Unknown Carrier").key,
        DEFAULT_START_KEY
    );
    let mut p = FakeProfile::named("Old Save");
    p.carrier_key = String::new();
    p.carrier_name = "Great Lakes Training Transport".to_string();
    assert_eq!(option_for_profile(&p).key, "great_lakes_training");
}
