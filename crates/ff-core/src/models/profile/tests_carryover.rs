//! The "Choose a carrier" choice on a carried 1.9 career: what each choice
//! sets, and everything it must leave alone.

use serde_json::{json, Value};

use super::*;
use crate::data::world::get_world;
use crate::models::business::LEASED_OWNER_OPERATOR;
use crate::models::carryover::Carryover;

/// A Prairie Link career as the copy leaves it, living in `city`.
fn carried(city: &str, owner: bool) -> Profile {
    let mut p = Profile::named_in("Carried", city);
    p.carrier_key = "prairie_link".into();
    p.carrier_name = "Prairie Link Regional".into();
    p.home_city.clear();
    if owner {
        p.business_status = LEASED_OWNER_OPERATOR.into();
        p.owned_trucks = vec!["rig".into()];
    }
    p.set_money(48_250.0);
    p.career.xp = 90_000.0;
    p.career.reputation = 71.0;
    p.career.purchased_endorsements = vec!["hazmat".into()];
    p.achievements = vec!["first_delivery".into()];
    p.upgrades = condition::upgrades_from_value(Some(&json!({"apu": 1})));
    p.driving_record.citations = 2;
    p.driving_record.carrier_terminations = 1;
    p.carry_over_from_1_9();
    assert_eq!(p.carryover, Carryover::CarrierChoiceDue);
    p
}

/// Everything a choice must never touch, as saved. A company driver's
/// assigned tractor is the carrier's to give, so `truck` counts only for an
/// owner-operator, whose tractor is their own.
fn kept(p: &Profile) -> Vec<Value> {
    let d = p.to_dict();
    let owner = crate::models::business_constants::is_owner_operator(&p.business_status);
    [
        "money",
        "career",
        "owned_trucks",
        if owner { "truck" } else { "money" },
        "upgrades",
        "truck_conditions",
        "achievements",
        "achievement_stats",
        "driving_record",
        "business_status",
        "owned_trailers",
        "trailer_programs",
    ]
    .iter()
    .map(|k| d.get(*k).cloned().unwrap_or(Value::Null))
    .collect()
}

fn choose(p: &mut Profile, stay: bool) -> (CarrierChoiceRow, String) {
    let world = get_world();
    let rows = p.carrier_choice_rows(world);
    assert!(rows[0].stay, "stay is first");
    assert!(rows[1..]
        .iter()
        .all(|r| !r.stay && r.carrier.key != "prairie_link"));
    let row = if stay {
        rows[0].clone()
    } else {
        rows[1].clone()
    };
    let (before, line, after) = super::tests::with_data_dir(|_| {
        let before = kept(p);
        let line = p.choose_carrier(world, &row);
        (before, line, kept(p))
    });
    assert_eq!(after, before, "a choice moved something it must not");
    assert_eq!(p.carryover, Carryover::Settled);
    assert!(!p.carrier_choice_due());
    (row, line)
}

#[test]
fn rows_follow_the_start_picker_and_hire_where_the_driver_lives() {
    let p = carried("miami_fl_us", false);
    let world = get_world();
    let rows = p.carrier_choice_rows(world);
    assert!(rows.len() > 3);
    for row in &rows[1..] {
        assert!(
            row.carrier.hires_in(world, "miami_fl_us"),
            "{}",
            row.carrier.key
        );
    }
    // Locals, then regionals, then nationals.
    let ranks: Vec<usize> = rows[1..].iter().map(|r| r.carrier.tier_rank()).collect();
    let mut sorted = ranks.clone();
    sorted.sort();
    assert_eq!(ranks, sorted);
}

#[test]
fn a_company_driver_joining_another_carrier_starts_at_its_terminal() {
    let mut p = carried("miami_fl_us", false);
    let (row, line) = choose(&mut p, false);
    assert_eq!(p.carrier_key, row.carrier.key);
    assert_eq!(p.home_terminal_city, row.terminal_city);
    assert_eq!(p.current_city, row.terminal_city);
    assert_eq!(p.home_city, "");
    assert!(line.starts_with(&format!("{} has taken you on.", row.carrier.name)));
}

#[test]
fn a_company_driver_staying_out_of_range_moves_home_to_the_nearest_terminal() {
    let mut p = carried("miami_fl_us", false);
    p.parked_facility = "Port of Miami".into();
    let (row, line) = choose(&mut p, true);
    let nearest = crate::models::carriers::carrier("prairie_link")
        .unwrap()
        .nearest_terminal_city(get_world(), "miami_fl_us")
        .unwrap();
    assert_eq!(row.terminal_city, nearest);
    assert_eq!(p.carrier_key, "prairie_link");
    assert_eq!(p.home_city, nearest);
    assert_eq!(p.home_terminal_city, nearest);
    assert_eq!(p.current_city, nearest);
    // As joining a carrier does: no facility parking at the terminal.
    assert_eq!(p.parked_facility, "");
    assert!(line.contains("Your home is now"), "{line}");
}

#[test]
fn a_company_driver_staying_in_range_starts_at_the_hiring_terminal() {
    let mut p = carried("topeka_ks_us", false);
    let home_terminal = p.home_terminal_city.clone();
    let (_, line) = choose(&mut p, true);
    assert_eq!(p.home_city, "");
    assert_eq!(p.home_terminal_city, home_terminal);
    assert_eq!(p.current_city, home_terminal);
    assert!(!line.contains("Your home is now"), "{line}");
    // Already at that terminal: nothing about where the truck waits.
    let mut there = carried("topeka_ks_us", false);
    there.current_city = home_terminal.clone();
    let (_, line) = choose(&mut there, true);
    assert_eq!(
        line,
        "You stay with Prairie Link Regional. The dispatch board is open again."
    );
}

#[test]
fn an_owner_operator_lease_moves_and_the_truck_stays_parked() {
    let mut p = carried("miami_fl_us", true);
    let (row, line) = choose(&mut p, false);
    assert_eq!(p.carrier_key, row.carrier.key);
    assert_eq!(p.carrier_name, row.carrier.name);
    assert_eq!(p.home_terminal_city, row.terminal_city);
    assert_eq!(p.current_city, "miami_fl_us");
    assert!(line.starts_with("You are now leased on to"), "{line}");

    let mut stay = carried("miami_fl_us", true);
    let (row, line) = choose(&mut stay, true);
    assert_eq!(stay.home_city, row.terminal_city);
    assert_eq!(stay.current_city, "miami_fl_us");
    assert!(
        line.contains("Your truck stays where it is parked."),
        "{line}"
    );

    let mut near = carried("topeka_ks_us", true);
    let (_, line) = choose(&mut near, true);
    assert_eq!(near.current_city, "topeka_ks_us");
    assert_eq!(
        line,
        "You stay leased on to Prairie Link Regional. The dispatch board is open again."
    );
}

#[test]
fn a_native_prairie_link_career_never_owes_the_choice() {
    let mut p = Profile::named_in("Native", "topeka_ks_us");
    p.carrier_key = "prairie_link".into();
    p.carrier_name = "Prairie Link Regional".into();
    assert_eq!(p.carryover, Carryover::Native);
    assert!(!p.carrier_choice_due());
    assert!(!p.settle_stale_carrier_choice());
}

#[test]
fn any_carrier_change_settles_the_choice() {
    let mut p = carried("miami_fl_us", false);
    p.change_carrier("northstar", "Northstar Freight Lines");
    assert_eq!(p.carryover, Carryover::Settled);

    let mut p = carried("miami_fl_us", false);
    p.take_fallback_after_let_go("prairie_link");
    assert_eq!(p.carryover, Carryover::Settled);

    // Own authority leaves no hiring carrier: the stale state is settled.
    let mut p = carried("miami_fl_us", true);
    p.business_status = crate::models::business::INDEPENDENT_AUTHORITY.into();
    assert!(!p.carrier_choice_due());
    assert!(p.settle_stale_carrier_choice());
    assert_eq!(p.carryover, Carryover::Settled);
}
