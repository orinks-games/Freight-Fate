//! Ported from `tests/test_trailer_yard.py`: trailer yards, drop-and-hook,
//! live loads, and detention. The tests' `SimpleNamespace` jobs and profiles
//! are `FakeJob` and the two owner stand-ins below.

use super::*;
use crate::models::career::test_profile::FakeJob;
use crate::models::trailers::trailer_keys_for_cargo;

fn job(facility_type: &str, facility_id: &str, cargo: &str, distance: f64) -> FakeJob {
    FakeJob {
        origin_type: facility_type.to_string(),
        origin_facility_id: facility_id.to_string(),
        origin_location: "Origin Dock".to_string(),
        cargo_key: cargo.to_string(),
        distance_mi: distance,
        weight_tons: 14.0,
        ..FakeJob::default()
    }
}

fn default_job() -> FakeJob {
    job("cross_dock", "fac-1", "general", 400.0)
}

/// `SimpleNamespace(owns_equipment=lambda: False, visible_owned_trailers=lambda: ())`.
struct CompanyDriver;

impl TrailerOwner for CompanyDriver {}

/// `SimpleNamespace(owns_equipment=lambda: True, visible_owned_trailers=lambda: trailers)`.
struct OwnerOperator(Vec<String>);

impl OwnerOperator {
    fn with(trailers: &[&str]) -> Self {
        OwnerOperator(trailers.iter().map(|t| t.to_string()).collect())
    }
}

impl TrailerOwner for OwnerOperator {
    fn owns_equipment(&self) -> bool {
        true
    }
    fn visible_owned_trailers(&self) -> Vec<String> {
        self.0.clone()
    }
}

#[test]
fn test_high_volume_freight_stages_trailers_and_a_quarry_does_not() {
    // Who keeps a drop yard is a fact about the business, not a dice roll.
    for facility_type in [
        "cross_dock",
        "parcel_hub",
        "intermodal_ramp",
        "port_terminal",
    ] {
        assert!(
            facility_has_drop_yard(facility_type, "any-id"),
            "{facility_type}"
        );
    }
    for facility_type in ["farm_elevator", "mine_quarry"] {
        assert!(
            !facility_has_drop_yard(facility_type, "any-id"),
            "{facility_type}"
        );
    }
}

#[test]
fn test_a_maybe_facility_is_the_same_answer_every_time() {
    // Derived from the facility, so the yard does not shuffle between visits.
    let answers: std::collections::BTreeSet<bool> = (0..5)
        .map(|_| facility_has_drop_yard("dry_warehouse", "warehouse-9"))
        .collect();
    assert_eq!(answers.len(), 1);
    // And different warehouses genuinely differ.
    let spread: std::collections::BTreeSet<bool> = (0..30)
        .map(|n| facility_has_drop_yard("dry_warehouse", &format!("warehouse-{n}")))
        .collect();
    assert_eq!(spread.len(), 2);
}

#[test]
fn test_a_drop_yard_holds_trailers_the_freight_can_actually_go_in() {
    let mut units = yard_trailers("cold_storage", "cold-1", "food");
    if units.is_empty() {
        units = yard_trailers("grocery_retail_dc", "dc-1", "food");
    }
    assert!(!units.is_empty());
    let allowed = trailer_keys_for_cargo("food");
    for unit in &units {
        assert!(allowed.contains(&unit.trailer_key.as_str()));
        assert!(unit.number.chars().all(|c| c.is_ascii_digit()));
    }
}

#[test]
fn test_drop_and_hook_is_far_quicker_than_a_dock() {
    let plan = pickup_plan(&default_job(), &CompanyDriver);
    assert_eq!(plan.mode, MODE_DROP_HOOK);
    assert_eq!(plan.minutes, DROP_HOOK_MIN);
    assert!(plan.minutes < LIVE_LOAD_MIN);
    assert!(plan.trailer.is_some());
    assert_eq!(plan.detention_minutes, 0.0);
}

#[test]
fn test_a_live_load_is_the_shippers_hour_and_sometimes_a_lot_more() {
    let plans: Vec<PickupPlan> = (0..60)
        .map(|n| {
            pickup_plan(
                &job(
                    "farm_elevator",
                    &format!("elev-{n}"),
                    "grain",
                    300.0 + n as f64,
                ),
                &CompanyDriver,
            )
        })
        .collect();
    assert!(plans.iter().all(|plan| plan.mode == MODE_LIVE));
    assert!(plans.iter().all(|plan| plan.trailer.is_none()));
    assert!(plans.iter().all(|plan| plan.minutes >= LIVE_LOAD_MIN));
    // Most shippers are fine; a real minority are not.
    let slow = plans
        .iter()
        .filter(|plan| plan.minutes > LIVE_LOAD_MIN)
        .count();
    assert!((5..=40).contains(&slow), "{slow}");
}

#[test]
fn test_detention_only_starts_after_the_free_time() {
    // Two hours free is the real term, and under it nobody owes anybody.
    let plans: Vec<PickupPlan> = (0..120)
        .map(|n| {
            pickup_plan(
                &job(
                    "farm_elevator",
                    &format!("elev-{n}"),
                    "grain",
                    300.0 + n as f64,
                ),
                &CompanyDriver,
            )
        })
        .collect();
    for plan in &plans {
        if plan.minutes <= DETENTION_FREE_MIN {
            assert_eq!(plan.detention_minutes, 0.0);
            assert!(detention_charge(plan).is_none());
        } else {
            assert!((plan.detention_minutes - (plan.minutes - DETENTION_FREE_MIN)).abs() < 1e-9);
            let charge = detention_charge(plan).expect("a charge past the free time");
            // Detention is money coming the other way.
            assert!(charge.amount < 0.0);
            assert!(charge.label.contains("detention"));
        }
    }
}

#[test]
fn test_owning_your_trailer_costs_you_drop_and_hook() {
    // Nobody swaps an owner-operator's own box for one out of the yard.
    let job = default_job(); // a facility that definitely has a drop yard
    assert!(pickup_plan(&job, &CompanyDriver).is_drop_hook());
    let plan = pickup_plan(&job, &OwnerOperator::with(&["dry_van"]));
    assert_eq!(plan.mode, MODE_LIVE);
    assert!(plan.trailer.is_none());
    assert!(plan.reason.contains("your own trailer"));
    // An owner-operator without a matching trailer is back on carrier equipment.
    assert!(pickup_plan(&job, &OwnerOperator::with(&["reefer"])).is_drop_hook());
}

#[test]
fn test_the_same_dispatch_always_comes_with_the_same_trailer() {
    // No save state backs this, so it has to be derivable and stable.
    let first = pickup_plan(&default_job(), &CompanyDriver).trailer;
    let again = pickup_plan(&default_job(), &CompanyDriver).trailer;
    assert_eq!(first, again);
}

#[test]
fn test_most_of_a_yard_is_serviceable_and_a_few_are_not() {
    // A fleet keeps its trailers up; the write-ups have to stay worth noticing.
    let units: Vec<TrailerUnit> = (0..120)
        .flat_map(|n| yard_trailers("cross_dock", &format!("dc-{n}"), "general"))
        .collect();
    assert!(!units.is_empty());
    let defective = units.iter().filter(|unit| unit.defect().is_some()).count();
    let share = defective as f64 / units.len() as f64;
    assert!((0.05..=0.30).contains(&share), "{share}");
}

#[test]
fn test_a_trailer_describes_itself_in_driver_words() {
    let clean = TrailerUnit::new("4417", "dry_van", 10.0);
    assert!(clean.defect().is_none());
    let text = clean.describe();
    assert!(text.contains("dry van 4417"));
    assert!(text.contains("good shape"));

    let rough = TrailerUnit::new("9002", "dry_van", 90.0);
    assert!(rough.defect().is_some());
    let lowered = rough.describe().to_lowercase();
    assert!(lowered.contains("9002"));
    for marker in ["_", "condition_pct", "none", "key="] {
        assert!(!lowered.contains(marker));
    }
}

// --- the delivery end, the swap, and the seed strings ----------------------------

#[test]
fn test_delivery_plan_drops_at_a_drop_yard_and_unloads_at_a_dock() {
    let mut job = default_job();
    job.destination_type = "parcel_hub".to_string();
    job.destination_facility_id = "hub-1".to_string();
    let plan = delivery_plan(&job, &CompanyDriver);
    assert!(plan.is_drop_hook());
    assert_eq!(plan.minutes, DROP_EMPTY_MIN);
    assert!(!plan.keeps_trailer);

    job.destination_type = "farm_elevator".to_string();
    let dock = delivery_plan(&job, &CompanyDriver);
    assert_eq!(dock.mode, MODE_LIVE);
    assert_eq!(dock.minutes, LIVE_UNLOAD_MIN);
    assert!(dock.keeps_trailer);
    assert_eq!(dock.reason, "the receiver is unloading you at the dock");

    job.destination_type = "parcel_hub".to_string();
    let own = delivery_plan(&job, &OwnerOperator::with(&["dry_van"]));
    assert_eq!(own.mode, MODE_LIVE);
    assert!(own.reason.contains("your own trailer"));
}

#[test]
fn test_replacement_trailer_is_clean_and_from_the_same_yard() {
    let job = default_job();
    let units = yard_trailers(&job.origin_type, &job.origin_facility_id, &job.cargo_key);
    assert!(!units.is_empty());
    let refused = &units[0];
    let swap = replacement_trailer(&job, Some(refused)).expect("a swap");
    assert!(swap.defect().is_none());
    assert_ne!(swap.number, refused.number);
    assert!(replacement_trailer(&job, None).is_none());
}

#[test]
fn test_yard_seed_matches_the_python_digest_arithmetic() {
    // sha256("dropyard|dry_warehouse|warehouse-9")[:6] big-endian, computed
    // by CPython, and the float in the seed string is Python's str(400.0).
    assert_eq!(
        seed(&["dropyard", "dry_warehouse", "warehouse-9"]),
        0xc1ce_5f44_4bfb
    );
    assert_eq!(py_str_float(400.0), "400.0");
    let plan = pickup_plan(&default_job(), &CompanyDriver);
    let trailer = plan.trailer.expect("drop yard");
    assert_eq!(
        (trailer.number.as_str(), trailer.trailer_key.as_str()),
        ("5067", "dry_van")
    );
}

#[test]
fn test_detention_charge_speaks_the_hours_and_pays_the_driver() {
    let plan = PickupPlan {
        mode: MODE_LIVE,
        minutes: 225.0,
        trailer: None,
        detention_minutes: 105.0,
        reason: "the shipper is loading you at the dock, and they are running behind",
    };
    assert_eq!(plan.detention_pay(), 78.75);
    let charge = detention_charge(&plan).unwrap();
    assert_eq!(charge.key, "detention_pay");
    assert_eq!(charge.amount, -78.75);
    assert_eq!(charge.responsibility, CARRIER_PAID);
    assert_eq!(
        charge.note,
        "1.8 hours held past the free time at the shipper"
    );
}
