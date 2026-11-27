//! Career milestone badges land on the settlement that crosses the line.
//!
//! Every one of these is counted in `award_arrival_achievements` after the
//! settlement has booked the run (delivery count, miles, pay, XP,
//! reputation), so the delivery that reaches the number is the one that
//! earns it, and the delivery before it does not.

use ff_core::models::career::LEVEL_XP;
use ff_core::models::carrier_fleet::{fleet_tier_for_level, FLEET_TIERS};
use ff_core::models::trucks::TRUCK_CATALOG;
use freight_fate::states::driving_menu_states::settlement_hours;
use serde_json::json;

use crate::badge_moments_support::*;

const CHICAGO: &str = "chicago_il_us";
const CLEVELAND: &str = "cleveland_oh_us";

/// The delivery that makes it `count` earns `id`; the one before does not.
fn nth_delivery_earns(id: &str, count: i64) {
    let mut app = career_in(CHICAGO);
    profile(&mut app).career.deliveries = count - 2;
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert_eq!(profile(&mut app).career.deliveries, count - 1);
    assert!(!earned(&app, id), "{id} came one delivery early");
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, id), "{id} missed delivery {count}");
}

#[test]
fn ten_deliveries_lands_on_the_tenth() {
    nth_delivery_earns("ten_deliveries", 10);
}

#[test]
fn twenty_five_deliveries_lands_on_the_twenty_fifth() {
    nth_delivery_earns("twenty_five_deliveries", 25);
}

#[test]
fn fifty_deliveries_lands_on_the_fiftieth() {
    nth_delivery_earns("fifty_deliveries", 50);
}

#[test]
fn hundred_deliveries_lands_on_the_hundredth() {
    nth_delivery_earns("hundred_deliveries", 100);
}

#[test]
fn two_hundred_deliveries_lands_on_the_two_hundredth() {
    nth_delivery_earns("two_hundred_deliveries", 200);
}

#[test]
fn two_fifty_deliveries_lands_on_the_two_hundred_fiftieth() {
    nth_delivery_earns("two_fifty_deliveries", 250);
}

#[test]
fn five_hundred_deliveries_lands_on_the_five_hundredth() {
    nth_delivery_earns("five_hundred_deliveries", 500);
}

/// The settlement that carries the career to `level` earns `id`; one that
/// ends a level short does not.
fn level_earns(id: &str, level: i64) {
    let mut app = career_in(CHICAGO);
    let threshold = LEVEL_XP[(level - 1) as usize];
    let below = LEVEL_XP[(level - 2) as usize];
    profile(&mut app).career.xp = below;
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(
        profile(&mut app).career.xp < threshold,
        "one run jumped a whole level"
    );
    assert!(!earned(&app, id), "{id} came a level early");
    profile(&mut app).career.xp = threshold - 1.0;
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert_eq!(profile(&mut app).career.level(), level);
    assert!(earned(&app, id), "{id} missed level {level}");
}

#[test]
fn level_three_lands_with_the_level() {
    level_earns("level_three", 3);
}

#[test]
fn level_five_lands_with_the_level() {
    level_earns("level_five", 5);
}

#[test]
fn level_ten_lands_with_the_level() {
    level_earns("level_ten", 10);
}

#[test]
fn level_fifteen_lands_with_the_level() {
    level_earns("level_fifteen", 15);
}

#[test]
fn max_level_is_the_level_twenty_badge() {
    level_earns("max_level", 20);
}

#[test]
fn level_twenty_five_lands_with_the_level() {
    level_earns("level_twenty_five", 25);
}

#[test]
fn level_thirty_lands_with_the_level() {
    level_earns("level_thirty", 30);
}

#[test]
fn level_thirty_five_lands_with_the_level() {
    level_earns("level_thirty_five", 35);
}

#[test]
fn level_forty_lands_with_the_level() {
    level_earns("level_forty", 40);
}

#[test]
fn fleet_flagship_lands_on_the_promotion_into_first_pick() {
    let at = FLEET_TIERS
        .iter()
        .position(|tier| tier.key == "first_pick")
        .expect("the first-pick fleet");
    let mut app = career_in(CHICAGO);
    // A promotion into the band below is a new tractor, not the flagship.
    profile(&mut app).career.xp = LEVEL_XP[(FLEET_TIERS[at - 1].min_level - 1) as usize] - 1.0;
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(earned(&app, "fleet_upgrade"));
    assert!(!earned(&app, "fleet_flagship"));
    profile(&mut app).career.xp = LEVEL_XP[(FLEET_TIERS[at].min_level - 1) as usize] - 1.0;
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, "fleet_flagship"));
}

/// Seed `stat` with `count - 2` other values, then two deliveries each add
/// one new value: the first leaves `id` a step short, the second earns it.
fn unique_tally_earns(id: &str, stat: &str, count: usize, others: Vec<String>) {
    let mut app = career_in(CHICAGO);
    profile(&mut app)
        .achievement_stats
        .insert(stat.to_string(), json!(others[..count - 2]));
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(!earned(&app, id), "{id} came a step early");
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, id), "{id} missed number {count}");
}

fn other_cities() -> Vec<String> {
    ff_core::data::world::get_world()
        .cities
        .keys()
        .filter(|key| *key != CHICAGO && *key != CLEVELAND)
        .cloned()
        .collect()
}

fn other_states() -> Vec<String> {
    let mut states: Vec<String> = ff_core::data::world::get_world()
        .cities
        .values()
        .map(|city| city.state.clone())
        .filter(|state| state != "Illinois" && state != "Ohio")
        .collect();
    states.sort();
    states.dedup();
    states
}

#[test]
fn twenty_five_cities_lands_on_the_twenty_fifth_city_delivered_to() {
    unique_tally_earns("twenty_five_cities", "cities_delivered", 25, other_cities());
}

#[test]
fn seventy_five_cities_lands_on_the_seventy_fifth_city_delivered_to() {
    unique_tally_earns(
        "seventy_five_cities",
        "cities_delivered",
        75,
        other_cities(),
    );
}

#[test]
fn hundred_fifty_cities_lands_on_the_hundred_fiftieth_city_delivered_to() {
    unique_tally_earns(
        "hundred_fifty_cities",
        "cities_delivered",
        150,
        other_cities(),
    );
}

#[test]
fn fifteen_states_lands_on_the_fifteenth_state_delivered_to() {
    unique_tally_earns("fifteen_states", "states_delivered", 15, other_states());
}

#[test]
fn thirty_states_lands_on_the_thirtieth_state_delivered_to() {
    unique_tally_earns("thirty_states", "states_delivered", 30, other_states());
}

/// A settlement that leaves the bank under `amount` does not earn `id`; the
/// one that lands it at or over does.
fn bank_earns(id: &str, amount: f64) {
    let mut app = career_in(CHICAGO);
    profile(&mut app).set_money(amount - 5_000.0);
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(profile(&mut app).money() < amount);
    assert!(!earned(&app, id), "{id} came under {amount}");
    profile(&mut app).set_money(amount);
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(profile(&mut app).money() >= amount);
    assert!(earned(&app, id), "{id} missed {amount}");
}

#[test]
fn twenty_five_grand_counts_the_bank_after_pay() {
    bank_earns("twenty_five_grand", 25_000.0);
}

#[test]
fn hundred_grand_counts_the_bank_after_pay() {
    bank_earns("hundred_grand", 100_000.0);
}

#[test]
fn quarter_million_bank_counts_the_bank_after_pay() {
    bank_earns("quarter_million_bank", 250_000.0);
}

#[test]
fn half_million_earned_counts_lifetime_pay_not_the_bank() {
    let mut app = career_in(CHICAGO);
    profile(&mut app).career.total_earnings = 490_000.0;
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(profile(&mut app).career.total_earnings < 500_000.0);
    assert!(!earned(&app, "half_million_earned"));
    profile(&mut app).career.total_earnings = 499_999.0;
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, "half_million_earned"));
    // Lifetime earnings, not cash: the bank never came near it.
    assert!(profile(&mut app).money() < 500_000.0);
}

/// Two 345-mile runs from `miles - 400` on the odometer: the first stops
/// short, the second crosses.
fn odometer_earns(id: &str, miles: f64) {
    let mut app = career_in(CHICAGO);
    profile(&mut app).career.total_miles = miles - 400.0;
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(profile(&mut app).career.total_miles < miles);
    assert!(!earned(&app, id), "{id} came under {miles} miles");
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(profile(&mut app).career.total_miles >= miles);
    assert!(earned(&app, id), "{id} missed {miles} miles");
}

#[test]
fn thousand_miles_lands_on_the_run_that_crosses_it() {
    odometer_earns("thousand_miles", 1_000.0);
}

#[test]
fn ten_thousand_miles_lands_on_the_run_that_crosses_it() {
    odometer_earns("ten_thousand_miles", 10_000.0);
}

#[test]
fn fifty_thousand_miles_lands_on_the_run_that_crosses_it() {
    odometer_earns("fifty_thousand_miles", 50_000.0);
}

#[test]
fn hundred_k_miles_lands_on_the_run_that_crosses_it() {
    odometer_earns("hundred_k_miles", 100_000.0);
}

#[test]
fn quarter_million_miles_lands_on_the_run_that_crosses_it() {
    odometer_earns("quarter_million_miles", 250_000.0);
}

#[test]
fn half_million_miles_lands_on_the_run_that_crosses_it() {
    odometer_earns("half_million_miles", 500_000.0);
}

#[test]
fn million_miles_lands_on_the_run_that_crosses_it() {
    odometer_earns("million_miles", 1_000_000.0);
}

/// An on-time delivery adds two points of reputation. From `start`, the
/// first leaves the career under `mark`, the second reaches it.
fn reputation_earns(id: &str, start: f64, mark: f64) {
    let mut app = career_in(CHICAGO);
    profile(&mut app).career.reputation = start;
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(profile(&mut app).standing() < mark);
    assert!(!earned(&app, id), "{id} came under {mark}");
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(profile(&mut app).standing() >= mark);
    assert!(earned(&app, id), "{id} missed {mark}");
}

#[test]
fn rep_ninety_lands_on_the_delivery_that_reaches_ninety() {
    reputation_earns("rep_ninety", 87.0, 90.0);
}

#[test]
fn top_reputation_lands_on_the_delivery_that_reaches_one_hundred() {
    reputation_earns("top_reputation", 97.0, 100.0);
}

#[test]
fn month_on_road_counts_the_clock_after_the_run() {
    let mut app = career_in(CHICAGO);
    let first = run(&mut app, CHICAGO, CLEVELAND);
    let hours = settlement_hours(&first);
    drop(first);
    // One hour short of thirty days once this run's hours are booked.
    profile(&mut app).game_hours = 30.0 * 24.0 - hours - 1.0;
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(profile(&mut app).game_hours < 30.0 * 24.0);
    assert!(!earned(&app, "month_on_road"));
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, "month_on_road"));
}

#[test]
fn perfect_streak_needs_five_clean_on_time_runs_in_a_row() {
    let mut app = career_in(CHICAGO);
    for _ in 0..2 {
        deliver(&mut app, CHICAGO, CLEVELAND);
    }
    // A late run ends the streak.
    let mut late = run(&mut app, CHICAGO, CLEVELAND);
    late.trip.game_minutes = (late.job.deadline_game_h + 1.0) * 60.0;
    settle(&mut app, &mut late);
    for _ in 0..4 {
        deliver(&mut app, CHICAGO, CLEVELAND);
    }
    assert!(!earned(&app, "perfect_streak"), "the late run should reset");
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(earned(&app, "perfect_streak"));
}

#[test]
fn never_fought_the_law_takes_fifty_ticket_free_runs_and_a_ticket_resets_it() {
    let mut app = career_in(CHICAGO);
    profile(&mut app)
        .achievement_stats
        .insert("ticket_free_deliveries".to_string(), json!(48));
    let mut ticketed = run(&mut app, CHICAGO, CLEVELAND);
    ticketed.speeding_tickets = 1;
    settle(&mut app, &mut ticketed);
    assert_eq!(
        profile(&mut app).achievement_stats["ticket_free_deliveries"],
        json!(0)
    );
    profile(&mut app)
        .achievement_stats
        .insert("ticket_free_deliveries".to_string(), json!(48));
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(!earned(&app, "never_fought_the_law"));
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, "never_fought_the_law"));
}

#[test]
fn night_shift_regular_counts_the_tenth_delivery_before_four() {
    let mut app = career_in(CHICAGO);
    profile(&mut app)
        .achievement_stats
        .insert("night_deliveries".to_string(), json!(8));
    // A daytime run is not counted at all.
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert_eq!(
        profile(&mut app).achievement_stats["night_deliveries"],
        json!(8)
    );
    for expect in [false, true] {
        let mut drive = run(&mut app, CHICAGO, CLEVELAND);
        arrive_at_local_hour(&mut drive, 2.0);
        settle(&mut app, &mut drive);
        assert_eq!(earned(&app, "night_shift_regular"), expect);
    }
}

#[test]
fn home_return_needs_ten_deliveries_and_the_first_run_s_origin() {
    let mut app = career_in(CHICAGO);
    // The first delivery pins home to where it loaded.
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert_eq!(
        profile(&mut app).achievement_stats["home_city"],
        json!(CHICAGO)
    );
    profile(&mut app).career.deliveries = 8;
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(!earned(&app, "home_return"), "nine deliveries is not ten");
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(!earned(&app, "home_return"), "Cleveland is not home");
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, "home_return"));
}

#[test]
fn five_tractors_counts_the_fifth_different_tractor() {
    let mut app = career_in(CHICAGO);
    let active = profile(&mut app).active_truck_key();
    let others: Vec<&str> = TRUCK_CATALOG
        .keys()
        .copied()
        .filter(|key| *key != active)
        .take(4)
        .collect();
    profile(&mut app)
        .achievement_stats
        .insert("tractors_driven".to_string(), json!(others[..3]));
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(!earned(&app, "five_tractors"), "four tractors is not five");
    // The same tractor again adds nothing.
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(!earned(&app, "five_tractors"));
    profile(&mut app)
        .achievement_stats
        .insert("tractors_driven".to_string(), json!(others));
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(earned(&app, "five_tractors"));
}

#[test]
fn every_fleet_tier_lands_when_the_last_band_is_driven() {
    let mut app = career_in(CHICAGO);
    let current = fleet_tier_for_level(profile(&mut app).career.level()).key;
    let others: Vec<&str> = FLEET_TIERS
        .iter()
        .map(|tier| tier.key)
        .filter(|key| *key != current)
        .collect();
    // Every band but this one and one more.
    profile(&mut app)
        .achievement_stats
        .insert("fleet_tiers_driven".to_string(), json!(others[1..]));
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(!earned(&app, "every_fleet_tier"));
    profile(&mut app)
        .achievement_stats
        .insert("fleet_tiers_driven".to_string(), json!(others));
    deliver(&mut app, CLEVELAND, CHICAGO);
    assert!(earned(&app, "every_fleet_tier"));
}
