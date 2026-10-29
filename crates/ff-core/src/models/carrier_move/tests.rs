use super::*;
use crate::models::business_constants::LEASED_OWNER_OPERATOR;
use crate::models::career::LEVEL_XP;
use crate::models::enforcement::LAST_CHANCE_CARRIER_NAME;
use crate::models::profile::Profile;
use crate::models::start_options::{start_option, DEFAULT_START_KEY};

fn trainee(level: i64) -> Profile {
    let mut p = Profile::named_in("Trainee", "Milwaukee");
    p.carrier_key = LAST_CHANCE_CARRIER_KEY.to_string();
    p.carrier_name = LAST_CHANCE_CARRIER_NAME.to_string();
    p.career.xp = LEVEL_XP[(level - 1) as usize];
    p.career.reputation = 50.0;
    p
}

#[test]
fn a_new_trainee_is_told_the_level_it_takes() {
    let (ok, reasons) = move_up_eligibility(&trainee(1));
    assert!(!ok);
    assert_eq!(
        reasons,
        vec!["Reach level 4: Regional Company Driver.".to_string()]
    );
}

#[test]
fn a_trainee_at_the_regional_level_in_good_standing_can_move() {
    let (ok, reasons) = move_up_eligibility(&trainee(MOVE_UP_LEVEL));
    assert!(ok, "{reasons:?}");
}

#[test]
fn a_driver_let_go_must_rebuild_trust_first() {
    let mut p = trainee(12);
    p.career.reputation = 20.0;
    let (ok, reasons) = move_up_eligibility(&p);
    assert!(!ok);
    assert_eq!(
        reasons,
        vec!["Bring dispatch trust back to full with clean on-time runs.".to_string()]
    );
}

#[test]
fn an_unsettled_pay_advance_holds_the_move() {
    let mut p = trainee(MOVE_UP_LEVEL);
    p.pay_advance = 400.0;
    let (ok, reasons) = move_up_eligibility(&p);
    assert!(!ok);
    assert_eq!(
        reasons,
        vec!["Settle the pay advance on your current load.".to_string()]
    );
}

#[test]
fn only_a_company_driver_at_the_training_fleet_is_offered_the_move() {
    let mut northstar = trainee(MOVE_UP_LEVEL);
    northstar.carrier_key = DEFAULT_START_KEY.to_string();
    assert_eq!(move_up_eligibility(&northstar), (false, Vec::new()));

    let mut leased = trainee(MOVE_UP_LEVEL);
    leased.business_status = LEASED_OWNER_OPERATOR.to_string();
    assert!(!at_training_fleet(&leased));
    assert!(!move_up_eligibility(&leased).0);
}

#[test]
fn the_move_up_list_is_the_other_company_carriers() {
    let keys: Vec<&str> = move_up_carriers().iter().map(|o| o.key).collect();
    assert_eq!(keys, vec!["northstar", "prairie_link", "summit_value"]);
}

#[test]
fn moving_changes_the_carrier_and_keeps_the_career() {
    let mut p = trainee(6);
    p.set_money(3_210.0);
    let xp = p.career.xp;
    let line = apply_carrier_move(&mut p, start_option(Some("prairie_link")));
    assert_eq!(p.carrier_key, "prairie_link");
    assert_eq!(p.carrier_name, "Prairie Link Regional");
    assert!(p.dispatch_board_cache.is_none());
    assert_eq!(p.career.xp, xp);
    assert_eq!(p.money(), 3_210.0);
    assert_eq!(p.current_city, "Milwaukee");
    assert!(!at_training_fleet(&p));
    assert_eq!(
        line,
        "Prairie Link Regional hires you away from Great Lakes Training Transport. Your \
         level, record, and home terminal come with you, and the next load runs on their \
         pay and their tractor."
    );
}
