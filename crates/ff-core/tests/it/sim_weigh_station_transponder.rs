//! PrePass-style weigh-in-motion bypass: the transponder gate and verdicts
//! (port of `tests/test_weigh_station_transponder.py`). The gate lives in
//! `models::business` / `models::profile`; the verdict mechanic is in the app
//! shell, so those cases still wait for it.

use ff_core::models::business::{
    has_weigh_station_transponder, independent_authority_charges_for_trailers,
    owner_operator_charges, weigh_station_transponder_eligibility, WEIGH_STATION_TRANSPONDER_LEVEL,
    WEIGH_STATION_TRANSPONDER_PER_MILE,
};
use ff_core::models::business_constants::LEASED_OWNER_OPERATOR;
use ff_core::models::career::LEVEL_XP;
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use ff_core::pyfmt::round_py_n;

#[test]
fn test_company_driver_below_level_four_has_no_transponder() {
    let p = Profile::named("Rookie");
    assert!(p.career.level() < WEIGH_STATION_TRANSPONDER_LEVEL);
    assert!(!has_weigh_station_transponder(&p));
}

#[test]
fn test_company_driver_at_level_four_gets_a_free_transponder() {
    let mut p = Profile::named("Trusted");
    p.career.xp = LEVEL_XP[WEIGH_STATION_TRANSPONDER_LEVEL as usize - 1];
    assert_eq!(p.career.level(), WEIGH_STATION_TRANSPONDER_LEVEL);
    assert!(has_weigh_station_transponder(&p));
}

#[test]
fn test_owner_operator_needs_the_purchased_subscription_not_just_level() {
    let mut p = Profile::named("Owner");
    p.business_status = LEASED_OWNER_OPERATOR.to_string();
    p.career.xp = LEVEL_XP[WEIGH_STATION_TRANSPONDER_LEVEL as usize - 1];
    // Level alone buys a company driver a free transponder, but this driver
    // has no fleet behind them -- the level gate does not apply.
    assert!(!has_weigh_station_transponder(&p));

    let (ok, reasons) = weigh_station_transponder_eligibility(&p);
    // starting money already covers the signup fee
    assert!(ok && reasons.is_empty());

    p.set_money(0.0);
    let (ok, reasons) = weigh_station_transponder_eligibility(&p);
    assert!(!ok);
    assert!(reasons[0].contains("dollars"), "{:?}", reasons[0]);

    p.set_money(100_000.0);
    p.weigh_station_transponder = true;
    assert!(has_weigh_station_transponder(&p));
    let (ok, reasons) = weigh_station_transponder_eligibility(&p);
    assert!(!ok);
    assert!(reasons[0].contains("already active"), "{:?}", reasons[0]);
}

#[test]
fn test_transponder_settlement_charge_only_when_subscribed() {
    let job = Job::new(
        cargo_type("general").expect("general freight"),
        20.0,
        "A",
        "yard",
        "B",
        100.0,
        1000.0,
        12.0,
    );

    let plain = owner_operator_charges(&job, 1000.0, false, 1.0, None);
    assert!(!plain.iter().any(|c| c.label.contains("transponder")));

    let with_sub = owner_operator_charges(&job, 1000.0, true, 1.0, None);
    let charge = with_sub
        .iter()
        .find(|c| c.label.contains("transponder"))
        .expect("the subscription is billed");
    assert_eq!(
        charge.amount,
        round_py_n(job.distance_mi * WEIGH_STATION_TRANSPONDER_PER_MILE, 2)
    );

    // Own-authority settlement carries the same reserve, threaded the same way.
    let owned: [&str; 0] = [];
    let authority_plain =
        independent_authority_charges_for_trailers(&job, 1000.0, &owned, false, 1.0, None);
    assert!(!authority_plain
        .iter()
        .any(|c| c.label.contains("transponder")));
    let authority_with_sub =
        independent_authority_charges_for_trailers(&job, 1000.0, &owned, true, 1.0, None);
    assert!(authority_with_sub
        .iter()
        .any(|c| c.label.contains("transponder")));
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- transponder verdict in DrivingState"]
fn test_below_the_gate_the_scale_still_demands_every_truck() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- transponder verdict in DrivingState"]
fn test_transponder_green_bypasses_without_a_charge() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- transponder verdict in DrivingState"]
fn test_transponder_red_pulls_in_like_the_old_flow() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- transponder verdict in DrivingState"]
fn test_overweight_load_is_always_red_lighted() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- transponder verdict seeded off trip seed and stop"]
fn test_transponder_verdict_is_seeded_off_trip_seed_and_stop() {}
