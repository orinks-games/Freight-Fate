//! Hand-back after the 2.0 buy-in, against saves from before it.

use super::*;
use crate::models::business::OLD_BUY_IN_PRICE;
use crate::models::business_constants::LEASED_OWNER_OPERATOR;
use crate::models::buy_in::buy_in_tractors;
use crate::models::profile::Profile;
use crate::models::start_options::{apply_start_option, start_option, OWNER_OPERATOR_START_KEY};
use crate::models::trucks::truck_model;

const FLAGSHIP: &str = "continental_expedition";

fn owner_with(first: &str) -> Profile {
    let mut p = Profile::named("Hand Back");
    p.business_status = LEASED_OWNER_OPERATOR.to_string();
    p.owned_trucks = vec![first.to_string()];
    p.truck = first.to_string();
    p
}

#[test]
fn a_1_9_buy_in_flagship_goes_back_for_the_old_buy_in_price() {
    // A 1.9 save has no buy_in_paid at all.
    crate::models::profile::tests::with_data_dir(|_| {
        let mut data = owner_with(FLAGSHIP).to_dict();
        data.remove("buy_in_paid");
        let p = Profile::from_dict(&data);
        assert_eq!(p.buy_in_paid, 0.0);
        assert_eq!(company_return_buy_back(&p), OLD_BUY_IN_PRICE);
    });
}

#[test]
fn a_2_0_buy_in_flagship_goes_back_at_the_used_share_of_its_price() {
    let mut p = owner_with(FLAGSHIP);
    let price = truck_model(FLAGSHIP).expect("catalog").price;
    p.buy_in_paid = price;
    assert_eq!(
        company_return_buy_back(&p),
        round_py_n(price * REPOSSESSION_EQUITY_SHARE, 2)
    );
}

#[test]
fn the_owner_operator_start_rig_goes_back_for_nothing() {
    let mut p = Profile::named("Start Rig");
    apply_start_option(&mut p, start_option(Some(OWNER_OPERATOR_START_KEY)));
    assert_eq!(p.buy_in_paid, 0.0);
    assert_eq!(company_return_buy_back(&p), 0.0);
}

#[test]
fn a_heavy_hauler_bought_at_the_old_price_still_goes_back_for_less_than_paid() {
    let mut p = owner_with("rig");
    p.owned_trucks.push("heavy_hauler".to_string());
    let back = company_return_buy_back(&p);
    assert_eq!(back, 46_800.0);
    assert!(back < 52_000.0);
}

#[test]
fn buying_in_then_handing_back_never_returns_more_than_was_paid() {
    for model in buy_in_tractors() {
        let mut p = owner_with(model.key);
        p.buy_in_paid = model.price;
        let back = company_return_buy_back(&p);
        assert!(
            back <= model.price,
            "{} paid {} back {back}",
            model.key,
            model.price
        );
        let money_before = p.money();
        apply_return_to_company_driving(&mut p);
        assert_eq!(p.buy_in_paid, 0.0, "{}", model.key);
        assert!(p.money() - money_before <= model.price);
    }
}

#[test]
fn repossession_clears_what_the_buy_in_paid() {
    let mut p = owner_with("plain_jane_conventional");
    p.buy_in_paid = 34_000.0;
    apply_repossession(&mut p);
    assert_eq!(p.buy_in_paid, 0.0);
    assert!(!is_owner_operator(&p.business_status));
}
