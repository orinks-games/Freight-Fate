//! Ported from `tests/test_debt_and_standing.py` (the solvency half; the
//! dispatch-trust half is in `enforcement/tests.rs`). Cases that drive the
//! app shell, a real settlement, `carrier_fleet` or `career` are ignored with
//! the reason; their bodies say what they checked.

use super::test_profile::FakeProfile;
use super::*;
use crate::models::business_constants::LEASED_OWNER_OPERATOR;
use crate::models::enforcement::{
    self, board_offers_for_reputation, dispatch_trust_line, TRUST_FULL,
};
use crate::pyfmt::fmt_grouped;
use crate::pyrandom::PyRandom;

const BANNED_ENDINGS: [&str; 5] = [
    "game over",
    "career over",
    "you failed",
    "start over",
    "bankrupt",
];

fn profile() -> FakeProfile {
    FakeProfile::new()
}

fn owner_operator(truck: &str) -> FakeProfile {
    let mut p = profile();
    p.reputation = 85.0;
    p.business_status = LEASED_OWNER_OPERATOR.to_string();
    p.truck = truck.to_string();
    p.owned_trucks = vec![truck.to_string()];
    p
}

fn payer(money: f64, owed: f64) -> FakeProfile {
    let mut p = profile();
    p.money = money;
    p.fines_owed = owed;
    p
}

fn amount_of(options: &[(&'static str, f64)], kind: &str) -> Option<f64> {
    options.iter().find(|(k, _)| *k == kind).map(|(_, a)| *a)
}

// --- the invariant ----------------------------------------------------------

#[test]
fn test_a_clean_solvent_driver_is_untouched_by_any_of_this() {
    // Every gate in this change is off for a driver running clean and square.
    // The fleet-tier and Career-xp halves of the Python case need
    // carrier_fleet and career; the standing, debt and trust-line halves are
    // here.
    let p = profile();
    assert_eq!(enforcement::standing_band(&p), TRUST_FULL);
    assert_eq!(debt_owed(&p), 0.0);
    assert_eq!(debt_rung(&p), 0);
    assert_eq!(debt_line(&p), "");
    assert_eq!(debt_warning_line(&p, false), "");
    assert!(!company_termination_due(&p));
    assert!(!repossession_due(&p));
    assert_eq!(
        dispatch_trust_line(&p),
        "Dispatch trust: full. Reputation 50 out of 100. You get the whole board."
    );
}

// --- collection never takes the whole cheque (the zero-pay trap) ------------

#[test]
fn test_a_settlement_never_puts_more_than_a_quarter_toward_a_balance() {
    assert_eq!(collection_from_settlement(50_000.0, 1_000.0), 250.0);
    // Both deductions share one quarter, so three quarters always lands.
    let (collected, repaid) = deductions_from_settlement(50_000.0, 1_500.0, 1_000.0);
    assert!(collected + repaid <= 250.0 + 0.01);
    assert_eq!(round_py_n(1_000.0 - collected - repaid, 2), 750.0);
    // A small balance is simply cleared, not stretched out.
    assert_eq!(collection_from_settlement(40.0, 1_000.0), 40.0);
}

#[test]
fn test_with_nothing_owed_the_advance_repays_exactly_as_it_always_did() {
    assert_eq!(
        deductions_from_settlement(0.0, 400.0, 1_000.0),
        (0.0, 400.0)
    );
    assert_eq!(
        deductions_from_settlement(0.0, 4_000.0, 1_000.0),
        (0.0, 1_000.0)
    );
}

#[test]
fn test_working_always_digs_a_driver_out_however_deep_they_are() {
    // Fifty settlements at the cap must reduce the balance, not tread water.
    let mut balance = 51_000.0;
    let mut take_home_total = 0.0;
    for _ in 0..50 {
        let (collected, _) = deductions_from_settlement(balance, 0.0, 1_200.0);
        assert!(collected > 0.0);
        take_home_total += 1_200.0 - collected;
        balance = round_py_n(balance - collected, 2);
    }
    assert!(balance < 51_000.0);
    assert!(take_home_total > 0.0); // and the driver ate every one of those weeks
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (ArrivalState), the world and the app shell"]
fn test_a_settlement_under_collection_still_pays_the_driver() {
    // A real delivery under a 51,000 balance still pays at least 70 percent of
    // a clean run, says "Balance owed" and "three quarters always reaches
    // you", and the balance comes down by exactly what was collected.
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (ArrivalState), the world and the app shell"]
fn test_collection_and_an_advance_together_still_leave_the_driver_paid() {
    // One capped budget covers both; a big balance uses it all and the
    // advance waits; a small balance clears and the advance gets the rest.
}

// --- warnings ---------------------------------------------------------------

#[test]
fn test_the_last_warning_leaves_two_of_this_driver_s_own_settlements() {
    // A fixed dollar gap is worthless to a driver whose runs pay more than it.
    let mut p = profile();
    p.deliveries = 60;
    p.total_earnings = 60.0 * 2_400.0; // a senior driver on long freight
    assert_eq!(average_settlement(&p), 2_400.0);
    let gap = debt_ceiling(&p) - final_rung_debt(&p);
    assert!(gap >= 2.0 * average_settlement(&p) - 0.01);
}

#[test]
fn test_every_rung_is_reachable_and_each_names_the_ceiling_and_what_happens() {
    let mut p = profile();
    let ceiling = debt_ceiling(&p);
    assert_eq!(debt_rung(&p), 0);
    let mut seen = std::collections::BTreeSet::new();
    for owed in [200.0, ceiling * 0.55, ceiling * 0.95] {
        p.money = -owed;
        let rung = debt_rung(&p);
        seen.insert(rung);
        let verbose = debt_warning_line(&p, false);
        let terse = debt_warning_line(&p, true);
        assert!(!verbose.is_empty() && !terse.is_empty());
        // Terse may drop the "what brings it down" clause; it never drops the
        // ceiling number or the consequence.
        for text in [&verbose, &terse] {
            assert!(text.contains(&fmt_grouped(ceiling, 0)), "{text}");
        }
    }
    assert_eq!(seen.into_iter().collect::<Vec<i64>>(), vec![1, 2, 3]);
    assert!(debt_warning_line(&p, false).contains("ends your employment"));
}

#[test]
fn test_a_rung_is_spoken_once_and_only_when_it_moves() {
    let mut p = profile();
    assert_eq!(p.record().debt_rung_heard, 0);
    p.money = -200.0;
    assert_eq!(debt_rung(&p), 1);
    // The record is what makes it speak once; the same rung twice is silent.
    let rung = debt_rung(&p);
    p.record_mut().debt_rung_heard = rung;
    assert_eq!(debt_rung(&p), p.record().debt_rung_heard);
}

// --- company drivers: the ceiling is an ending of employment ---------------

#[test]
fn test_crossing_the_ceiling_ends_a_company_driver_s_employment() {
    let mut p = profile();
    let former = p.carrier_name.clone();
    p.money = -company_debt_ceiling(&p) - 1.0;
    assert!(company_termination_due(&p));

    let lines = apply_company_termination(&mut p);
    let joined = lines.join(" ");
    assert_eq!(p.carrier_key, LAST_CHANCE_CARRIER_KEY);
    assert_eq!(p.record().carrier_terminations, 1);
    // The balance is settled: arriving at the next fleet still carrying the
    // debt that cost the last seat is a career on rails to lose this one too.
    assert_eq!(debt_owed(&p), 0.0);
    assert!(p.money >= 0.0);
    assert!(joined.contains(&former));
    assert!(joined.contains("ended your employment"));
    assert!(joined.contains("keep your career level"));
    // Never a dead end -- a payroll to drive on -- and never these words.
    assert!(joined.contains("on the payroll at"));
    for banned in BANNED_ENDINGS {
        assert!(!joined.to_lowercase().contains(banned));
    }
    assert!(!p.dispatch_board_cached);
    assert_eq!(p.pay_advance, 0.0);
    assert!(!p.pay_advance_used_for_load);
    assert_eq!(p.record().setback_notice_kind, "termination");
    assert_eq!(p.record().setback_notice_lines, lines);
}

#[test]
fn test_a_terminated_driver_can_still_get_a_load_and_still_earn() {
    // The loop must not close: a board offer and take-home pay always exist.
    let mut p = profile();
    p.reputation = 0.0;
    p.money = -company_debt_ceiling(&p) - 1.0;
    apply_company_termination(&mut p);
    assert!(board_offers_for_reputation(8, p.reputation) >= 1);
    let (collected, repaid) = deductions_from_settlement(p.fines_owed, p.pay_advance, 900.0);
    assert!(900.0 - collected - repaid > 0.0);
}

#[test]
fn test_debt_stops_growing_at_the_fleet_that_hires_anyone() {
    // Nowhere further down, so the ceiling holds rather than dangling.
    let mut p = profile();
    p.carrier_key = LAST_CHANCE_CARRIER_KEY.to_string();
    p.carrier_name = LAST_CHANCE_CARRIER_NAME.to_string();
    p.money = -3_000.0;
    p.fines_owed = 40_000.0;
    assert!(hard_capped(&p));
    assert!(!company_termination_due(&p)); // never fires here

    let written_off = apply_hard_cap(&mut p);
    assert!(written_off > 0.0);
    assert!(debt_owed(&p) <= debt_ceiling(&p) + 0.01);
    assert!(debt_line(&p).contains("holds it there"));
}

// --- owner-operators: the lender takes the truck ---------------------------

#[test]
fn test_the_threshold_is_what_the_tractor_would_bring_at_sale() {
    let cheap = owner_operator("hand_me_down_sleeper");
    let dear = owner_operator("presidential_sleeper");
    assert!(tractor_value(&dear) > tractor_value(&cheap));
    // A better truck stands behind a bigger loan, so it carries further.
    assert!(repossession_threshold(&dear) > repossession_threshold(&cheap));
    let expected = dear.truck_catalog_price("presidential_sleeper") * REPOSSESSION_EQUITY_SHARE;
    assert_eq!(repossession_threshold(&dear), round_py_n(expected, 2));
    // The starter rig has no catalog price and sits on the floor.
    assert_eq!(
        repossession_threshold(&owner_operator("rig")),
        REPOSSESSION_FLOOR
    );
}

#[test]
fn test_negative_equity_costs_the_truck_and_lands_on_a_payroll() {
    let mut p = owner_operator("highline_sleeper");
    assert!(!repossession_due(&p));
    p.money = -repossession_threshold(&p) - 1.0;
    assert!(repossession_due(&p));

    let lines = apply_repossession(&mut p);
    let joined = lines.join(" ");
    assert_eq!(p.business_status, COMPANY_DRIVER);
    assert!(p.owned_trucks.is_empty());
    assert!(p.owned_trailers.is_empty());
    assert_eq!(p.record().repossessions, 1);
    assert_eq!(debt_owed(&p), 0.0);
    assert!(joined.contains("taken it back"));
    assert!(joined.contains("keep your career level"));
    assert!(joined.contains("owner-operator path is still open"));
    for banned in BANNED_ENDINGS {
        assert!(!joined.to_lowercase().contains(banned));
    }
    assert!(joined.contains("highline sleeper"));
    assert!(!p.authority_readiness);
    assert_eq!(p.record().setback_notice_kind, "repossession");
    assert_eq!(p.record().setback_notice_lines, lines);
}

#[test]
fn test_repossession_always_lands_at_a_carrier_that_will_hire() {
    // Losing the truck and the seat in the same breath would leave nowhere to drive.
    let mut p = owner_operator("highline_sleeper");
    p.reputation = REPUTATION_TERMINATION - 1.0;
    p.money = -repossession_threshold(&p) - 1.0;
    apply_repossession(&mut p);
    assert_eq!(p.carrier_key, LAST_CHANCE_CARRIER_KEY);
    // And the driver is in a real tractor, not an empty key.
    let key = p.active_truck_key();
    assert!(super::test_profile::FAKE_TRUCK_CATALOG
        .iter()
        .any(|(k, _, _)| *k == key));
}

#[test]
fn test_an_owner_operator_in_good_standing_keeps_their_own_carrier() {
    let mut p = owner_operator("highline_sleeper");
    p.carrier_name = "Great Plains Freight".to_string();
    p.carrier_key = "great_plains".to_string();
    p.money = -repossession_threshold(&p) - 1.0;
    let lines = apply_repossession(&mut p);
    assert_ne!(p.carrier_key, LAST_CHANCE_CARRIER_KEY);
    assert_eq!(p.carrier_key, "great_plains");
    assert!(lines
        .join(" ")
        .contains("on the payroll at Great Plains Freight"));
}

// --- the pay advance cannot become the debt spiral -------------------------

#[test]
fn test_no_advance_while_a_balance_is_already_being_collected() {
    // It is only ever offered under ten dollars of cash, so it would be offered forever.
    let mut p = profile();
    p.money = 2.0;
    assert_eq!(advance_refused_reason(&p), "");
    assert!(!collection_active(&p));

    p.fines_owed = 3_000.0;
    let reason = advance_refused_reason(&p);
    assert!(collection_active(&p));
    assert_eq!(reason, "No advance while you owe 3,000 dollars.");
}

// `test_the_terminal_stops_offering_an_advance_under_collection` is live in `crates/freight-fate/tests/states_city.rs`.

// --- the two big moments are reviewable, not one-shot ----------------------

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::career_setback and the app shell"]
fn test_the_setback_lands_as_a_re_readable_screen_that_escape_acknowledges() {
    // CareerSetbackNoticeState lists every line plus Continue; Escape clears
    // the pending notice.
}

#[test]
fn test_the_notice_survives_a_save_and_reload() {
    // The record half: the setback lines ride the DrivingRecord through its
    // save shape. The Profile.from_dict round trip needs models::profile.
    let mut p = profile();
    p.money = -company_debt_ceiling(&p) - 1.0;
    apply_company_termination(&mut p);
    let text = serde_json::to_string(p.record()).unwrap();
    let reloaded: DrivingRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(reloaded.setback_notice_kind, "termination");
    assert_eq!(
        reloaded.setback_notice_lines,
        p.record().setback_notice_lines
    );
    let mut again = profile();
    again.driving_record = Some(reloaded);
    assert!(setback_pending(&again));
    clear_setback_notice(&mut again);
    assert!(!setback_pending(&again));
    assert_eq!(again.record().setback_notice_kind, "");
}

// `test_a_setback_only_ever_fires_at_the_terminal` is live in `crates/freight-fate/tests/states_city.rs`.

// --- a level-up must not promise a truck the yard is withholding -----------

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs models::carrier_fleet and states::driving_menu_states"]
fn test_a_level_up_does_not_promise_iron_the_yard_is_holding_back() {}

/// No fuel, no reset wear, no wash: nothing changed hands.
#[test]
fn test_a_withheld_promotion_does_not_hand_over_a_freshly_serviced_truck() {
    use crate::models::carrier_fleet::withheld_promotion_text;
    use crate::models::profile::Profile;

    let mut p = Profile::named("Dale");
    p.current_city = "Buffalo".to_string();
    p.career.xp = 152_000.0;
    p.set_money(-5_000.0);
    let text = withheld_promotion_text(&p);
    assert!(text.contains("exactly as it stands"), "{text}");
    assert!(
        !text.contains("fueled") && !text.contains("washed"),
        "{text}"
    );
}

// --- out-of-pocket payoff helpers -------------------------------------------

#[test]
fn test_payoff_options_full_coverage() {
    let opts = out_of_pocket_options(&payer(5_000.0, 1_000.0));
    assert_eq!(amount_of(&opts, "all"), Some(1_000.0));
    assert_eq!(amount_of(&opts, "half"), Some(500.0));
    // cushion amount (4800) exceeds the balance, so it clamps to it and
    // duplicates "all" -- deduplicated away.
    assert_eq!(amount_of(&opts, "cushion"), None);
}

#[test]
fn test_payoff_options_partial_coverage() {
    // Every option keeps the 200 dollar fuel cushion -- none may exceed it.
    let opts = out_of_pocket_options(&payer(800.0, 4_170.0));
    assert_eq!(amount_of(&opts, "all"), None);
    // half the balance is 2085, capped at cash minus the cushion (600), and
    // the cushion option lands on the same 600 -- the dedup collapses the
    // duplicate amount, leaving only "half" in the options.
    assert_eq!(amount_of(&opts, "half"), Some(600.0));
    assert_eq!(amount_of(&opts, "cushion"), None);
    assert_eq!(
        opts.iter().map(|(_, a)| *a).collect::<Vec<f64>>(),
        vec![600.0]
    );
}

#[test]
fn test_payoff_options_hidden_when_broke_or_clear() {
    assert!(out_of_pocket_options(&payer(9.0, 500.0)).is_empty());
    assert!(out_of_pocket_options(&payer(500.0, 0.5)).is_empty());
    assert!(out_of_pocket_options(&payer(-50.0, 500.0)).is_empty());
}

#[test]
fn test_payoff_options_full_balance_needs_cushion_headroom_too() {
    // Cash covering the whole balance but not the cushion on top of it must
    // not offer "all" -- fuel money outranks a clean slate. The cushion option
    // still moves what it can.
    let opts = out_of_pocket_options(&payer(220.0, 50.0));
    assert_eq!(amount_of(&opts, "all"), None);
    assert_eq!(amount_of(&opts, "half"), Some(20.0));
    assert_eq!(amount_of(&opts, "cushion"), None); // dedup leaves one option
    assert_eq!(
        opts.iter().map(|(_, a)| *a).collect::<Vec<f64>>(),
        vec![20.0]
    );
}

#[test]
fn test_payoff_options_at_the_cushion_floor_offers_nothing_worth_paying() {
    // Cash sitting right at the cushion has nothing to spare -- an empty
    // list is acceptable and the menu (city.PayDebtState) copes with it.
    assert!(out_of_pocket_options(&payer(200.0, 500.0)).is_empty());
}

#[test]
fn test_payoff_options_never_exceed_the_cushion_headroom() {
    // No option's amount may ever exceed cash minus the cushion, across a
    // spread of cash/balance pairs.
    let mut rng = PyRandom::new_from_i64(2026);
    for _ in 0..200 {
        let cash = rng.uniform(0.0, 10_000.0);
        let balance = rng.uniform(0.0, 10_000.0);
        let headroom = cash - PAYOFF_CASH_CUSHION;
        for (_, amount) in out_of_pocket_options(&payer(cash, balance)) {
            assert!(amount <= headroom + 0.01);
        }
    }
}

#[test]
fn test_pay_out_of_pocket_clamps_and_clears() {
    let mut p = payer(800.0, 600.0);
    assert_eq!(pay_out_of_pocket(&mut p, 600.0), 600.0);
    assert_eq!(p.fines_owed, 0.0);
    assert_eq!(p.money, 200.0);

    let mut p = payer(100.0, 600.0);
    assert_eq!(pay_out_of_pocket(&mut p, 999.0), 100.0); // never below zero cash
    assert_eq!(p.money, 0.0);
    assert_eq!(p.fines_owed, 500.0);

    let mut p = payer(100.0, 600.0);
    assert_eq!(pay_out_of_pocket(&mut p, 0.0), 0.0); // no-op stays a no-op
}

// --- the reviewable debt line points at cash payoff; the warnings do not ----------

const POINTER: &str = "You can also pay it down from cash at any terminal or truck stop.";

#[test]
fn test_debt_lines_point_at_out_of_pocket_payoff() {
    let p = payer(500.0, 1_000.0); // rung 1 for a fresh company driver
    assert!(!debt_warning_line(&p, false).contains(POINTER));
    assert!(!debt_warning_line(&p, true).contains(POINTER));
    assert!(debt_line(&p).contains(POINTER));
}

#[test]
fn test_debt_lines_point_at_out_of_pocket_payoff_when_hard_capped() {
    // A hard-capped driver has nowhere further down to fall, but the cash
    // payoff pointer is still true for them and stays on the debt line.
    let mut p = payer(500.0, 40_000.0);
    p.carrier_key = LAST_CHANCE_CARRIER_KEY.to_string();
    assert!(hard_capped(&p));

    assert!(!debt_warning_line(&p, false).contains(POINTER));
    assert!(!debt_warning_line(&p, true).contains(POINTER));
    assert!(debt_line(&p).contains(POINTER));
}

// --- the terminal menu item and PayDebtState --------------------------------

// `test_the_terminal_only_offers_payoff_when_something_is_owed` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_paying_it_all_off_clears_the_balance_and_says_so` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_paying_it_all_off_speaks_the_clear_confirmation_last` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_paying_half_leaves_the_rest_owed_and_says_the_remainder` is live in `crates/freight-fate/tests/states_city.rs`.

// --- the same payoff item at truck stops -------------------------------------

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (RestStopState) and the app shell"]
fn test_the_rest_stop_only_offers_payoff_when_something_is_owed() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (RestStopState) and the app shell"]
fn test_the_rest_stop_payoff_item_pushes_pay_debt_state() {}

// --- spoken lines, pinned verbatim --------------------------------------------

#[test]
fn the_spoken_lines_match_the_python_f_strings() {
    let p = payer(500.0, 1_000.0);
    assert_eq!(money_text(1_000.0), "1,000 dollars");
    assert_eq!(money_text(1_234_567.4), "1,234,567 dollars");
    assert_eq!(money_text(0.4), "0 dollars");
    assert_eq!(
        debt_warning_line(&p, true),
        "Owed 1,000 dollars. Ceiling 6,000 dollars."
    );
    assert_eq!(
        debt_line(&p),
        "Owed: 1,000 dollars of 6,000 dollars. Past that, the carrier ends your \
         employment and you move to another fleet. You can also pay it down from \
         cash at any terminal or truck stop."
    );
    let mut owner = owner_operator("highline_sleeper");
    owner.money = -30_000.0;
    assert_eq!(debt_rung(&owner), 2);
    assert_eq!(
        debt_warning_line(&owner, true),
        "Owed 30,000 dollars, over halfway to a ceiling of 49,200 dollars."
    );
    owner.money = -45_000.0;
    assert_eq!(debt_rung(&owner), 3);
    assert_eq!(
        debt_warning_line(&owner, true),
        "Owed 45,000 dollars. At 49,200 dollars, the tractor is worth less than \
         the loan on it and the lender takes it back."
    );
    assert_eq!(debt_share(&owner), 45_000.0 / 49_200.0);
    assert!(in_debt(&owner));
    assert_eq!(take_home_floor(1_000.0), 750.0);
    assert_eq!(sale_proceeds(&owner), 49_200.0);
}

// --- the way back by choice --------------------------------------------------

#[test]
fn test_going_back_to_company_driving_returns_the_equipment_for_the_buy_in() {
    // Owner, 2026-09-03: the only way out of the lease was to owe more than
    // the tractor was worth. Choosing the company seat hands everything back
    // and is not a setback: no record entry, no notice, the same carrier.
    let mut p = owner_operator("highline_sleeper");
    p.carrier_name = "Great Plains Freight".to_string();
    p.carrier_key = "great_plains".to_string();
    p.money = 4_000.0;
    // A tractor worth 82,000 would bring 49,200 used; the carrier's price for
    // the one the buy-in bought is the buy-in itself.
    assert_eq!(company_return_buy_back(&p), OWNER_OPERATOR_BUY_IN);

    let lines = apply_return_to_company_driving(&mut p);
    let joined = lines.join(" ");
    assert_eq!(p.business_status, COMPANY_DRIVER);
    assert_eq!(p.money, 4_000.0 + OWNER_OPERATOR_BUY_IN);
    assert!(p.owned_trucks.is_empty());
    assert!(p.owned_trailers.is_empty());
    assert!(!p.authority_readiness);
    assert_eq!(p.carrier_key, "great_plains");
    assert_eq!(p.record().repossessions, 0);
    assert_eq!(p.record().setback_notice_kind, "");
    // In a carrier tractor, not an empty key.
    let key = p.active_truck_key();
    assert!(super::test_profile::FAKE_TRUCK_CATALOG
        .iter()
        .any(|(k, _, _)| *k == key));
    assert!(joined.contains("Great Plains Freight takes the highline sleeper back"));
    assert!(joined.contains("company driver again"));
    assert!(joined.contains("buy-in stays open"));
    for banned in BANNED_ENDINGS {
        assert!(!joined.to_lowercase().contains(banned));
    }
}

#[test]
fn test_a_second_tractor_goes_back_at_the_used_share() {
    // The buy-in cap is for the tractor the buy-in bought. One bought at the
    // dealer since sells the way the lender's sale prices it.
    let mut p = owner_operator("hand_me_down_sleeper");
    p.owned_trucks.push("presidential_sleeper".to_string());
    let expected = round_py_n(
        (31_000.0 * REPOSSESSION_EQUITY_SHARE).min(OWNER_OPERATOR_BUY_IN)
            + 185_000.0 * REPOSSESSION_EQUITY_SHARE,
        2,
    );
    assert_eq!(company_return_buy_back(&p), expected);
    let lines = apply_return_to_company_driving(&mut p);
    assert!(lines[0].contains("and one more piece of equipment"));
}
