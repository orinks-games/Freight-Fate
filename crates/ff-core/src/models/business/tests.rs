//! Ported from the pure cases of `tests/test_business_arc.py`,
//! `tests/test_settlement_accounting.py` and `tests/test_settlement_readout_leaner.py`.
//! The app-shell cases (menus, garage, the arrival settlement) are ignored
//! with the reason and their bodies say what they checked.

use super::*;
use crate::models::career::LEVEL_XP;
use crate::models::career_ladder::CAREER_RANKS;
use crate::models::jobs::{cargo_type, Job};
use crate::models::profile::Profile;
use crate::models::start_options::{apply_start_option, start_option, OWNER_OPERATOR_START_KEY};
use crate::models::trailers::{
    compatible_with_programs, trailer_keys_for_cargo, trailer_type, TRAILER_CATALOG,
};

fn job(cargo: &str, origin_location: &str, miles: f64, pay: f64, deadline: f64) -> Job {
    Job::new(
        cargo_type(cargo).unwrap(),
        12.0,
        "Chicago",
        origin_location,
        "Milwaukee",
        miles,
        pay,
        deadline,
    )
}

fn settle(status: &str, job: &Job) -> BusinessSettlement {
    build_business_settlement_basic(status, job, job.pay, true, 0.0)
}

fn charge(settlement: &BusinessSettlement, label: &str) -> f64 {
    settlement
        .business_charges
        .iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| panic!("no {label} charge"))
        .amount
}

#[test]
fn test_thirty_level_ladder_has_business_arc_titles() {
    assert_eq!(CAREER_RANKS.len(), 30);
    assert_eq!(
        CAREER_RANKS.iter().map(|r| r.level).collect::<Vec<_>>(),
        (1..=30).collect::<Vec<_>>()
    );
    assert_eq!(CAREER_RANKS[0].title, "Yard Trainee");
    assert_eq!(CAREER_RANKS[4].title, "Regional Regular");
    assert_eq!(CAREER_RANKS[14].title, "Owner-Operator Candidate");
    assert_eq!(CAREER_RANKS[17].title, "Leased-On Owner-Operator");
    assert_eq!(CAREER_RANKS[24].title, "Independent Authority Operator");
    assert_eq!(CAREER_RANKS[29].title, "Freight Fate Independent");
}

#[test]
fn test_owner_operator_start_begins_the_arc_rather_than_skipping_it() {
    // The owner-operator start used to hand out level 18, 35 deliveries,
    // 42,000 miles and 70,000 dollars of lifetime earnings before the player
    // had driven a foot -- most of a thirty-level arc, plus a career history
    // that never happened and was published on the public profile. The start
    // is about ECONOMICS, not progression: you own the truck and you carry
    // the costs from day one, and you climb the same ladder as everyone else.
    let mut p = Profile::named("Independent");
    let option = start_option(Some(OWNER_OPERATOR_START_KEY));
    apply_start_option(&mut p, option);

    assert_eq!(p.career.level(), 1);
    assert_eq!(p.career.xp, 0.0);
    assert_eq!(p.career.deliveries, 0);
    assert_eq!(p.career.on_time_deliveries, 0);
    assert_eq!(p.career.total_miles, 0.0);
    assert_eq!(p.career.total_earnings, 0.0);
    // What the start IS: your truck, your capital, your costs.
    assert_eq!(p.business_status, LEASED_OWNER_OPERATOR);
    assert!(p.owned_trucks.iter().any(|t| t == "rig"));
    assert!(p.money() > 0.0);
    // And the menu no longer promises a shortcut.
    let blurb = format!("{} {}", option.menu_summary, option.help_text).to_lowercase();
    assert!(!blurb.contains("skip"));
}

#[test]
fn test_owner_operator_unlock_requires_career_and_working_capital() {
    let mut p = Profile::named("Business Gate");

    let (ok, reasons) = owner_operator_eligibility(&p);
    assert!(!ok);
    assert!(reasons
        .iter()
        .any(|r| r.contains(&format!("Reach level {OWNER_OPERATOR_LEVEL}"))));
    assert!(business_status_summary(&p).contains(STARTER_CARRIER_NAME));
    assert!(business_status_summary(&p)
        .to_lowercase()
        .contains("company driver"));

    p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
    p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
    p.career.reputation = OWNER_OPERATOR_REPUTATION;
    p.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL);

    let (ok, reasons) = owner_operator_eligibility(&p);
    assert!(ok);
    assert!(reasons.is_empty());

    p.pay_advance = 100.0;
    let (ok, reasons) = owner_operator_eligibility(&p);
    assert!(!ok);
    assert!(reasons.iter().any(|r| r.contains("advance")));
}

#[test]
fn test_level_five_is_preparation_not_owner_operator_unlock() {
    let mut p = Profile::named("Prep Gate");
    p.career.xp = LEVEL_XP[4];
    p.career.deliveries = 20;
    p.career.reputation = 90.0;
    p.set_money(200_000.0);

    let (ok, reasons) = owner_operator_eligibility(&p);

    assert!(!ok);
    assert!(reasons
        .iter()
        .any(|r| r.contains(&format!("Reach level {OWNER_OPERATOR_LEVEL}"))));
    assert!(business_status_summary(&p).contains("Regional Regular"));
    assert!(next_business_unlock(&p).contains("Experienced Company Driver"));
}

#[test]
fn test_business_path_reports_starter_company_rank_and_next_unlock() {
    let mut p = Profile::named("Path Copy");
    p.career.xp = LEVEL_XP[14];

    assert!(business_path_label(&p).contains(STARTER_CARRIER_NAME));
    assert!(business_path_label(&p).contains("Owner-Operator Candidate"));
    // From the level-14 prep rank onward, Business status reads the real
    // owner-operator checklist instead of pointing at the next rank title.
    let unlock = next_business_unlock(&p);
    assert!(unlock.contains("Owner-operator gate locked"));
    assert!(unlock.contains("Reach level 18"));
}

// `test_business_status_menu_unlocks_owner_operator_when_qualified` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_company_driver_garage_service_is_carrier_billed` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_garage_sells_the_traction_equipment_ladder` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_company_driver_gets_carrier_chains_but_carrier_rubber` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_company_driver_truck_status_says_assigned_not_owned` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_company_driver_shops_hide_owned_truck_language` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_owner_operator_buy_in_records_first_owned_tractor` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_owner_operator_can_buy_switch_and_upgrade_owned_equipment` is live in `crates/freight-fate/tests/states_city_shops.rs`.

#[test]
fn test_trailer_catalog_matches_current_cargo_classes() {
    for key in ["dry_van", "reefer", "flatbed", "bulk"] {
        assert!(TRAILER_CATALOG.iter().any(|t| t.key == key), "{key}");
    }
    assert_eq!(trailer_keys_for_cargo("general"), ["dry_van"]);
    assert_eq!(trailer_keys_for_cargo("refrigerated"), ["reefer"]);
    assert_eq!(trailer_keys_for_cargo("steel"), ["flatbed"]);
    assert_eq!(trailer_keys_for_cargo("grain"), ["bulk"]);
    assert!(compatible_with_programs("farm_inputs", ["dry_van"]));
    assert!(compatible_with_programs("farm_inputs", ["bulk"]));
    let reefer = trailer_type("reefer").unwrap();
    assert!(reefer.purchase_price > reefer.lease_deposit);
    assert!(reefer.owned_per_mile_reserve < reefer.per_mile_reserve);
}

// `test_owner_operator_can_add_specialty_trailer_program` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_own_authority_can_buy_owned_trailer` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_leased_on_owner_operator_does_not_see_trailer_purchase` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_owner_operator_job_board_labels_missing_trailer_program` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_owner_operator_job_board_accepts_matching_trailer_program` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_own_authority_job_board_labels_owned_trailer_and_program_charge` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_company_driver_trailer_program_menu_stays_carrier_provided` is live in `crates/freight-fate/tests/states_city_shops.rs`.

#[test]
fn test_owner_operator_settlement_uses_specialty_trailer_program_charge() {
    let dry_job = job("general", "yard", 100.0, 1000.0, 6.0);
    let reefer_job = job("refrigerated", "cold", 100.0, 1000.0, 6.0);

    let dry = settle(LEASED_OWNER_OPERATOR, &dry_job);
    let reefer = settle(LEASED_OWNER_OPERATOR, &reefer_job);

    assert!(charge(&reefer, "trailer program") > charge(&dry, "trailer program"));
}

#[test]
fn test_own_authority_owned_trailer_reduces_trailer_charge() {
    let job = job("refrigerated", "cold", 100.0, 1000.0, 6.0);

    let program = settle(INDEPENDENT_AUTHORITY, &job);
    let owned = build_business_settlement(
        INDEPENDENT_AUTHORITY,
        &job,
        job.pay,
        true,
        0.0,
        &SettlementTerms {
            owned_trailers: &["reefer"],
            ..Default::default()
        },
    );

    let program_trailer = charge(&program, "trailer program");
    let owned_trailer = charge(&owned, "owned trailer reserve");
    assert!(owned_trailer < program_trailer);
    assert!(owned.net_before_advance > program.net_before_advance);
}

#[test]
fn test_authority_readiness_requires_endgame_owner_operator() {
    let mut p = Profile::named_in("Authority Gate", "Chicago");
    let (ok, reasons) = authority_readiness_eligibility(&p);

    assert!(!ok);
    assert!(reasons.iter().any(|r| r.contains("owner-operator")));

    p.business_status = LEASED_OWNER_OPERATOR.to_string();
    p.owned_trucks = vec!["rig".to_string()];
    p.career.xp = LEVEL_XP[(AUTHORITY_READY_LEVEL - 1) as usize];
    p.career.deliveries = AUTHORITY_READY_DELIVERIES;
    p.career.reputation = AUTHORITY_READY_REPUTATION;
    p.set_money(AUTHORITY_READY_RESERVE + AUTHORITY_READY_WORKING_CAPITAL);

    let (ok, reasons) = authority_readiness_eligibility(&p);

    assert!(ok);
    assert!(reasons.is_empty());
    assert!(next_business_unlock(&p).contains("authority prep reserve"));
}

// `test_business_status_menu_sets_authority_readiness_reserve` is live in `crates/freight-fate/tests/states_city_shops.rs`.

#[test]
fn test_authority_activation_requires_prep_and_specialty_program() {
    let mut p = Profile::named_in("Authority Activate", "Chicago");
    p.business_status = LEASED_OWNER_OPERATOR.to_string();
    p.owned_trucks = vec!["rig".to_string()];
    p.career.xp = LEVEL_XP[(AUTHORITY_READY_LEVEL - 1) as usize];
    p.career.deliveries = AUTHORITY_ACTIVATION_DELIVERIES;
    p.career.reputation = AUTHORITY_ACTIVATION_REPUTATION;
    p.set_money(AUTHORITY_ACTIVATION_COST + AUTHORITY_ACTIVATION_WORKING_CAPITAL);

    let (ok, reasons) = authority_activation_eligibility(&p);
    assert!(!ok);
    assert!(reasons.iter().any(|r| r.contains("prep reserve")));

    p.authority_readiness = true;
    let (ok, reasons) = authority_activation_eligibility(&p);
    assert!(!ok);
    assert!(reasons
        .iter()
        .any(|r| r.contains(&format!("level {AUTHORITY_ACTIVATION_LEVEL}"))));

    p.career.xp = LEVEL_XP[(AUTHORITY_ACTIVATION_LEVEL - 1) as usize];
    let (ok, reasons) = authority_activation_eligibility(&p);
    assert!(!ok);
    assert!(reasons.iter().any(|r| r.contains("specialty trailer")));

    p.trailer_programs = vec!["dry_van".to_string(), "reefer".to_string()];
    let (ok, reasons) = authority_activation_eligibility(&p);
    assert!(ok);
    assert!(reasons.is_empty());
}

// `test_business_status_menu_activates_own_authority` is live in `crates/freight-fate/tests/states_city_shops.rs`.

#[test]
fn test_independent_authority_settlement_adds_business_overhead() {
    let job = job("general", "yard", 100.0, 1500.0, 6.0);

    let leased = settle(LEASED_OWNER_OPERATOR, &job);
    let direct = settle(INDEPENDENT_AUTHORITY, &job);

    let labels: Vec<&str> = direct.business_charges.iter().map(|c| c.label).collect();
    assert!(labels.contains(&"authority compliance reserve"));
    assert!(labels.contains(&"factoring fee"));
    assert_eq!(direct.status_label, "own authority");
    assert!(direct.business_charge_total() > leased.business_charge_total());
}

// `test_company_driver_board_labels_carrier_gross` is live in `crates/freight-fate/tests/states_city.rs`.

#[test]
fn test_late_company_driver_still_uses_company_settlement_until_buy_in() {
    let mut job = Job::new(
        cargo_type("general").unwrap(),
        18.0,
        "Chicago",
        "Chicago yard",
        "Milwaukee",
        92.0,
        1800.0,
        6.0,
    );
    job.origin_type = "terminal".to_string();
    let settlement = build_business_settlement_basic(COMPANY_DRIVER, &job, 1800.0, true, 0.0);

    assert_eq!(settlement.status, COMPANY_DRIVER);
    assert!(settlement.business_charges.is_empty());
}

// `test_transponder_shows_as_locked_when_the_fee_is_out_of_reach` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_the_locked_transponder_row_says_what_it_is_waiting_on` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_the_subscribe_row_returns_once_the_fee_is_affordable` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// -- the transponder gate, pure ---------------------------------------------------

#[test]
fn transponder_eligibility_follows_the_status_and_the_fee() {
    let mut p = Profile::named("Fleet Issued");
    assert!(!has_weigh_station_transponder(&p)); // level 1 company driver
    let (ok, reasons) = weigh_station_transponder_eligibility(&p);
    assert!(!ok);
    assert!(reasons[0].contains("at level 4"));
    p.career.xp = LEVEL_XP[3];
    assert!(has_weigh_station_transponder(&p));

    p.business_status = LEASED_OWNER_OPERATOR.to_string();
    assert!(!has_weigh_station_transponder(&p));
    p.set_money(WEIGH_STATION_TRANSPONDER_SIGNUP_FEE);
    assert_eq!(weigh_station_transponder_eligibility(&p), (true, vec![]));
    p.weigh_station_transponder = true;
    assert!(has_weigh_station_transponder(&p));
    assert!(!weigh_station_transponder_eligibility(&p).0);
    let job = job("general", "yard", 100.0, 1000.0, 6.0);
    let with = build_business_settlement(
        LEASED_OWNER_OPERATOR,
        &job,
        job.pay,
        true,
        0.0,
        &SettlementTerms {
            transponder: true,
            ..Default::default()
        },
    );
    assert_eq!(charge(&with, "weigh station transponder subscription"), 1.5);
    assert!(business_status_summary(&p).contains("transponder subscription is active"));
}

// -- tests/test_settlement_accounting.py ----------------------------------------

// Every case of this file is live on the app shell in
// `crates/freight-fate/tests/transcript_settlement_accounting.rs`:
// `test_carrier_paid_charges_do_not_increase_player_progression`,
// `test_delivery_stores_wear_and_road_grime`,
// `test_a_carried_balance_is_collected_at_a_capped_share_not_all_at_once`,
// `test_owner_operator_settlement_deducts_business_costs`,
// `test_pay_advance_is_repaid_from_settlement`,
// `test_settlement_time_cannot_be_faster_than_practical_road_average`,
// `test_pay_advance_repayment_never_drives_net_pay_negative`,
// `test_pay_advance_load_cooldown_resets_at_settlement`,
// `test_restored_toll_charges_do_not_duplicate_or_pay_out`,
// `test_toll_route_does_not_pay_more_than_equal_non_toll_route` and
// `test_repaid_advance_still_counts_as_lifetime_earnings`.

// -- tests/test_settlement_readout_leaner.py ------------------------------------

// `test_clean_run_drops_the_zero_information_rows` is live in `crates/freight-fate/tests/states_driving_menus.rs`.

// `test_damage_and_low_fuel_still_speak_when_they_matter` is live in `crates/freight-fate/tests/states_driving_menus.rs`.

// -- the pay arithmetic, pinned --------------------------------------------------

#[test]
fn company_pay_uses_the_wage_floor_share_and_trust_bonus() {
    let job = job("general", "yard", 100.0, 1000.0, 6.0);
    // Northstar: floor = stop_pay + miles * min_per_mile, share = pay_share.
    let plan = pay_plan_for_key(None);
    let floor = plan.stop_pay + 100.0 * plan.min_per_mile;
    let share = 1000.0 * plan.pay_share;
    let expected = round_py_n(floor.max(share) + 1000.0 * plan.on_time_bonus_share, 2);
    assert_eq!(company_driver_pay(&job, 1000.0, true, None, None), expected);
    assert_eq!(reputation_pay_bonus(1000.0, None), 0.0);
    assert_eq!(reputation_pay_bonus(1000.0, Some(50.0)), 0.0);
    assert_eq!(reputation_pay_bonus(1000.0, Some(100.0)), 60.0);
    assert_eq!(reputation_pay_bonus(1000.0, Some(200.0)), 60.0);
    // A fine the load cannot cover stays owed and is said so.
    let broke = build_business_settlement_basic(COMPANY_DRIVER, &job, 100.0, true, 400.0);
    assert_eq!(broke.net_before_advance, 0.0);
    assert!(broke.uncollected_charges > 0.0);
    assert!(broke.uncollected_charges <= 400.0);
    assert_eq!(owner_operator_gross(1000.0), 1120.0);
    assert_eq!(direct_freight_gross(1000.004), 1000.0);
    assert_eq!(
        status_label(LEASED_OWNER_OPERATOR),
        "leased-on owner-operator"
    );
    assert_eq!(pay_label(INDEPENDENT_AUTHORITY), "Direct gross");
    assert_eq!(pay_label(LEASED_OWNER_OPERATOR), "Gross revenue");
    assert!(player_pays_operating_costs(INDEPENDENT_AUTHORITY));
    assert!(!player_pays_operating_costs(COMPANY_DRIVER));
    let owner = settle(LEASED_OWNER_OPERATOR, &job);
    assert_eq!(
        owner.business_charge_summary(),
        "maintenance reserve 18 dollars, insurance reserve 9 dollars, trailer program 12 dollars, truck payment reserve 22 dollars, settlement service fee 22 dollars"
    );
    assert_eq!(independent_authority_charges(&job, 1000.0).len(), 6);
}

#[test]
fn test_a_company_driver_who_chose_to_stay_is_not_nudged_toward_the_buy_in() {
    // Owner, 2026-09-03: a driver who does not want the lease had no way to
    // say so, and every terminal visit, board and level-up repeated the
    // offer. With the choice recorded the buy-in is a door, not the plan.
    let mut p = Profile::named("Company By Choice");
    p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
    p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
    p.career.reputation = OWNER_OPERATOR_REPUTATION;
    p.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL);
    assert!(owner_operator_eligibility(&p).0);
    assert!(next_business_unlock(&p).contains("the owner-operator buy-in"));
    assert!(business_status_summary(&p).contains("You qualify for the owner-operator buy-in"));

    p.owner_operator_declined = true;

    // Still eligible: the choice hides nothing, it only stops the nudging.
    assert!(owner_operator_eligibility(&p).0);
    let unlock = next_business_unlock(&p);
    assert!(unlock.starts_with("Company driver by choice."), "{unlock}");
    assert!(
        !unlock.contains("where you buy the truck you drive"),
        "{unlock}"
    );
    let summary = business_status_summary(&p);
    assert!(summary.contains("by choice"), "{summary}");
    assert!(summary.contains("stays open here"), "{summary}");
    assert!(!summary.contains("You qualify"), "{summary}");
    // Titles follow the company ladder, not the owner-operator arc.
    let rank = display_rank_for(&p);
    assert_eq!(rank.title, "Company Fleet Captain");
    assert!(!summary.contains("Leased-On Owner-Operator"), "{summary}");
    assert!(summary.contains("Company Fleet Captain"), "{summary}");
    let unlock = next_business_unlock(&p);
    assert!(
        unlock.contains("Veteran Company Hauler") || unlock.contains("top career"),
        "{unlock}"
    );
    assert!(!unlock.contains("Settled Owner-Operator"), "{unlock}");
}

#[test]
fn test_declined_company_path_titles_differ_from_owner_operator_titles() {
    use crate::models::business_constants::COMPANY_DRIVER;
    use crate::models::career_ladder::{
        company_rank_for_level, display_rank_for_level, rank_for_level,
    };

    for level in [15_i64, 18, 20, 25, 30] {
        let oo = rank_for_level(level).title;
        let company = company_rank_for_level(level).title;
        assert_ne!(oo, company, "level {level}");
        assert_eq!(
            display_rank_for_level(level, COMPANY_DRIVER, true).title,
            company
        );
        assert_eq!(
            display_rank_for_level(level, LEASED_OWNER_OPERATOR, false).title,
            oo
        );
    }
}

#[test]
fn test_an_owner_operators_insurance_reserve_carries_the_record_surcharge_under_its_own_name() {
    let job = job("general", "yard", 100.0, 1000.0, 12.0);
    let clean = build_business_settlement(
        LEASED_OWNER_OPERATOR,
        &job,
        job.pay,
        true,
        0.0,
        &SettlementTerms::default(),
    );
    assert_eq!(charge(&clean, "insurance reserve"), 9.0);
    let surcharged = build_business_settlement(
        LEASED_OWNER_OPERATOR,
        &job,
        job.pay,
        true,
        0.0,
        &SettlementTerms {
            record_surcharge: 1.35,
            ..Default::default()
        },
    );
    // 100 miles x 0.09 x 1.35, and the readout says why.
    assert_eq!(
        charge(
            &surcharged,
            "insurance reserve, surcharged for your driving record"
        ),
        12.15
    );
    assert!(!surcharged
        .business_charges
        .iter()
        .any(|c| c.label == "insurance reserve"));
    assert!(surcharged.net_before_advance < clean.net_before_advance);
    // Own authority carries the same surcharge on its own insurance rate.
    let authority = build_business_settlement(
        INDEPENDENT_AUTHORITY,
        &job,
        job.pay,
        true,
        0.0,
        &SettlementTerms {
            record_surcharge: 2.0,
            ..Default::default()
        },
    );
    assert_eq!(
        charge(
            &authority,
            "insurance reserve, surcharged for your driving record"
        ),
        28.0
    );
}
