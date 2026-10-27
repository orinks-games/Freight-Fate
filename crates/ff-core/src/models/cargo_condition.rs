//! What the freight arrives in, and what the receiver does about it (port of
//! `freight_fate/models/cargo_condition.py`).
//!
//! Truck damage is the driver's problem. Cargo damage is the customer's, and
//! that is a different and much larger kind of money. Under the Carmack
//! Amendment (49 U.S.C. 14706) a motor carrier is liable for the actual loss or
//! injury to the property it carries: accept a clean bill of lading at origin,
//! deliver short or damaged, and the carrier owes the value of the freight
//! unless it can prove one of a short list of defences. At the dock the receiver
//! inspects, notes any exception on the bill of lading -- the OS&D notation,
//! Over, Short and Damaged -- and, if the load is bad enough, refuses it
//! outright. The carrier is then holding freight nobody will pay for and a claim
//! besides, and the driver delivered nothing.
//!
//! So the ladder here is the real one, and it is deliberately harsher at the top
//! than any truck-damage consequence: clean, an exception noted, a claim paid,
//! and the load rejected.
//!
//! The meter is condition, not value: 0 is freight in the state it was tendered,
//! 100 is a trailer of scrap. What moves it is what already moves in the sim --
//! hard braking, taking a bend faster than it is signed for, hitting something --
//! scaled by how well the freight in question survives being thrown about.
//! Nothing here knows about the window; the driving layer feeds it and speaks it.

use serde::{Deserialize, Serialize};

// Condition thresholds at the dock. Below the first, the receiver signs a
// clean bill and nobody says anything -- which is where a careful driver
// lives, and the whole point of the shallow end being free.
/// Noted on the bill of lading; a small deduction.
pub const CARGO_EXCEPTION_PCT: f64 = 12.0;
/// A real claim against the load's value.
pub const CARGO_CLAIM_PCT: f64 = 35.0;
/// The receiver refuses it: no delivery pay at all.
pub const CARGO_REJECT_PCT: f64 = 60.0;

// What each outcome costs, as a share of the load's gross pay. The claim
// figure is the freight's value, not a fee, which is why it dwarfs every
// fine in the game; rejection takes the pay as well as owing the claim.
pub const CARGO_EXCEPTION_PAY_LOSS: f64 = 0.10;
pub const CARGO_CLAIM_PAY_LOSS: f64 = 0.45;
pub const CARGO_REJECT_PAY_LOSS: f64 = 1.0;
// Standing lost at each rung. A rejected load is the worst thing a driver can
// hand a carrier short of hurting somebody.
pub const CARGO_EXCEPTION_REPUTATION: f64 = 2.0;
pub const CARGO_CLAIM_REPUTATION: f64 = 6.0;
pub const CARGO_REJECT_REPUTATION: f64 = 15.0;

// How much of the load's gross the carrier is out when a claim is filed.
// Rejected freight is a total loss on top of the unpaid haul.
pub const CARGO_CLAIM_VALUE_MULT: f64 = 1.5;
pub const CARGO_REJECT_VALUE_MULT: f64 = 3.0;

/// How freight answers being thrown around. 1.0 is a pallet of general
/// freight; the delicate classes bruise, shatter, or spoil at a multiple of
/// it, and the classes that are already rubble travel nearly indifferent.
pub const CARGO_FRAGILITY: &[(&str, f64)] = &[
    ("electronics", 3.0),
    ("high_value", 3.0),
    ("glass", 3.0),
    ("food", 2.4),
    ("refrigerated", 2.4),
    ("livestock", 2.6),
    ("automotive", 1.8),
    ("machinery", 1.6),
    ("retail", 1.4),
    ("parcel", 1.4),
    ("lumber_paper", 0.8),
    ("construction", 0.6),
    ("grain", 0.5),
    ("bulk", 0.4),
    // Liquid cannot be broken. What a tank load loses is quality: product
    // whipped into foam and out through the vents, temperature spec gone,
    // a food-grade load no longer fit to sell. That takes a great deal of
    // abuse, so the meter climbs slowly -- the danger of a tanker was never
    // to the cargo, it is to the truck and the driver, and that is priced in
    // the physics rather than here.
    ("fuel_bulk", 0.30),
    ("liquid_food", 0.40),
];
pub const CARGO_FRAGILITY_DEFAULT: f64 = 1.0;
/// Applied when the class is flagged fragile.
pub const CARGO_FRAGILE_FLAG_MULT: f64 = 1.5;

// The rates that MOVE the meter live with the physics that produce them,
// in sim/vehicle (CARGO_HARD_BRAKE_G and friends). This module owns only
// what the dock does with the number they arrive at.

pub const CARGO_OUTCOME_CLEAN: &str = "clean";
pub const CARGO_OUTCOME_EXCEPTION: &str = "exception";
pub const CARGO_OUTCOME_CLAIM: &str = "claim";
pub const CARGO_OUTCOME_REJECTED: &str = "rejected";

/// What `cargo_fragility` reads off a cargo class: its catalogue key and the
/// `fragile` flag.
// TODO(lead): implement for models::jobs::CargoType.
pub trait CargoFragility {
    fn key(&self) -> &str;
    fn fragile(&self) -> bool;
}

/// How fast this freight takes damage relative to general freight.
pub fn cargo_fragility<C: CargoFragility + ?Sized>(cargo: Option<&C>) -> f64 {
    match cargo {
        None => CARGO_FRAGILITY_DEFAULT,
        Some(cargo) => cargo_fragility_for(cargo.key(), cargo.fragile()),
    }
}

/// [`cargo_fragility`] from the two facts it reads.
pub fn cargo_fragility_for(key: &str, fragile: bool) -> f64 {
    match CARGO_FRAGILITY.iter().find(|(k, _)| *k == key) {
        Some((_, mult)) => *mult,
        None => {
            // A class the table has not been given a number for, but which the
            // catalogue already flags fragile, must not read as general freight.
            if fragile {
                CARGO_FRAGILITY_DEFAULT * CARGO_FRAGILE_FLAG_MULT
            } else {
                CARGO_FRAGILITY_DEFAULT
            }
        }
    }
}

/// What the receiver does with freight in this condition.
pub fn cargo_outcome(condition_pct: f64) -> &'static str {
    if condition_pct >= CARGO_REJECT_PCT {
        return CARGO_OUTCOME_REJECTED;
    }
    if condition_pct >= CARGO_CLAIM_PCT {
        return CARGO_OUTCOME_CLAIM;
    }
    if condition_pct >= CARGO_EXCEPTION_PCT {
        return CARGO_OUTCOME_EXCEPTION;
    }
    CARGO_OUTCOME_CLEAN
}

/// The dock's ruling on a load, in the numbers settlement needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoSettlement {
    pub outcome: String,
    pub condition_pct: f64,
    /// Dollars withheld from the driver's gross.
    pub pay_loss: f64,
    /// Dollars the claim is worth against the carrier.
    pub claim_value: f64,
    pub reputation_hit: f64,
}

impl CargoSettlement {
    pub fn rejected(&self) -> bool {
        self.outcome == CARGO_OUTCOME_REJECTED
    }

    pub fn clean(&self) -> bool {
        self.outcome == CARGO_OUTCOME_CLEAN
    }
}

/// Rule on a delivered load. Gross pay is the haul before deductions.
pub fn settle_cargo(condition_pct: f64, gross_pay: f64) -> CargoSettlement {
    let outcome = cargo_outcome(condition_pct);
    let gross = gross_pay.max(0.0);
    let (pay_loss, claim_value, reputation_hit) = match outcome {
        CARGO_OUTCOME_REJECTED => (
            gross * CARGO_REJECT_PAY_LOSS,
            gross * CARGO_REJECT_VALUE_MULT,
            CARGO_REJECT_REPUTATION,
        ),
        CARGO_OUTCOME_CLAIM => (
            gross * CARGO_CLAIM_PAY_LOSS,
            gross * CARGO_CLAIM_VALUE_MULT,
            CARGO_CLAIM_REPUTATION,
        ),
        CARGO_OUTCOME_EXCEPTION => (
            gross * CARGO_EXCEPTION_PAY_LOSS,
            0.0,
            CARGO_EXCEPTION_REPUTATION,
        ),
        _ => (0.0, 0.0, 0.0),
    };
    CargoSettlement {
        outcome: outcome.to_string(),
        condition_pct,
        pay_loss,
        claim_value,
        reputation_hit,
    }
}

/// The same four rungs said in the words that fit a tank. Diesel does not
/// "shift but sound" and milk does not "break": a liquid load is worked, then
/// off spec, then contaminated, then lost. Same thresholds, same money -- only
/// the nouns change, because a driver who hears "the load is damaged" about a
/// tank of fuel has been told nothing they can act on.
pub const LIQUID_CONDITION_TEXT: &[(&str, &str)] = &[
    (CARGO_OUTCOME_REJECTED, "lost"),
    (CARGO_OUTCOME_CLAIM, "contaminated"),
    (CARGO_OUTCOME_EXCEPTION, "off spec"),
];

/// The spoken state of the load, for status readouts and cues.
///
/// Plain words, not a number alone: "eighteen percent" tells a player
/// nothing about whether the receiver will sign for it. `liquid` swaps in
/// the tank vocabulary; the thresholds and the consequences are identical.
pub fn cargo_condition_text(condition_pct: f64, liquid: bool) -> &'static str {
    let outcome = cargo_outcome(condition_pct);
    if liquid {
        if let Some((_, text)) = LIQUID_CONDITION_TEXT.iter().find(|(o, _)| *o == outcome) {
            return text;
        }
        return if condition_pct >= 1.0 {
            "worked"
        } else {
            "settled"
        };
    }
    match outcome {
        CARGO_OUTCOME_REJECTED => "ruined",
        CARGO_OUTCOME_CLAIM => "badly damaged",
        CARGO_OUTCOME_EXCEPTION => "damaged",
        _ if condition_pct >= 1.0 => "shifted but sound",
        _ => "secure",
    }
}

#[cfg(test)]
mod tests {
    //! Ported from the pure cases of `tests/test_cargo_condition.py` and the
    //! cargo-condition cases of `tests/test_tanker_surge.py`. The meter
    //! (TruckState._update_cargo) belongs to sim::vehicle; the cues, status
    //! screen, snapshot and settlement lines to the driving states.

    use super::*;
    use crate::sim::vehicle::{
        TruckState, CARGO_ADVISORY_LAT_G, CARGO_HARD_BRAKE_G, EMERGENCY_BRAKE_MULT, ROLL_WARN_SHARE,
    };

    struct FakeCargo {
        key: &'static str,
        fragile: bool,
    }

    impl CargoFragility for FakeCargo {
        fn key(&self) -> &str {
            self.key
        }
        fn fragile(&self) -> bool {
            self.fragile
        }
    }

    fn approx(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= (1e-6 * expected.abs()).max(1e-12)
    }

    // -- the meter (the pure fragility part) ---------------------------------

    #[test]
    fn test_cargo_fragility_falls_back_on_the_catalogue_fragile_flag() {
        let general = FakeCargo {
            key: "general",
            fragile: false,
        };
        let electronics = FakeCargo {
            key: "electronics",
            fragile: true,
        };
        let plain = cargo_fragility(Some(&general));
        assert!(approx(plain, 1.0));
        assert!(approx(cargo_fragility::<FakeCargo>(None), 1.0));
        assert!(cargo_fragility(Some(&electronics)) > plain);
        // The fallback itself: an unlisted class the catalogue flags fragile.
        let odd = FakeCargo {
            key: "unlisted",
            fragile: true,
        };
        assert_eq!(cargo_fragility(Some(&odd)), CARGO_FRAGILE_FLAG_MULT);
        assert_eq!(
            cargo_fragility_for("unlisted", false),
            CARGO_FRAGILITY_DEFAULT
        );
        // A listed class ignores the flag: the table has the number.
        assert_eq!(cargo_fragility_for("bulk", true), 0.4);
    }

    #[test]
    fn test_liquid_freight_is_hard_to_ruin_because_liquid_does_not_break() {
        for key in ["fuel_bulk", "liquid_food"] {
            assert!(cargo_fragility_for(key, false) < 0.5);
        }
        assert!(cargo_fragility_for("electronics", true) > 2.0);
    }

    // -- the dock ------------------------------------------------------------

    #[test]
    fn test_the_receivers_ladder_runs_clean_exception_claim_refused() {
        assert_eq!(cargo_outcome(0.0), CARGO_OUTCOME_CLEAN);
        assert_eq!(cargo_outcome(CARGO_EXCEPTION_PCT), CARGO_OUTCOME_EXCEPTION);
        assert_eq!(cargo_outcome(CARGO_CLAIM_PCT), CARGO_OUTCOME_CLAIM);
        assert_eq!(cargo_outcome(CARGO_REJECT_PCT), CARGO_OUTCOME_REJECTED);
    }

    #[test]
    fn test_a_clean_load_costs_nothing() {
        let settled = settle_cargo(CARGO_EXCEPTION_PCT - 1.0, 3000.0);
        assert!(settled.clean());
        assert_eq!(settled.pay_loss, 0.0);
        assert_eq!(settled.claim_value, 0.0);
        assert_eq!(settled.reputation_hit, 0.0);
    }

    #[test]
    fn test_the_penalties_escalate_up_the_ladder() {
        let gross = 3000.0;
        let exception = settle_cargo(CARGO_EXCEPTION_PCT, gross);
        let claim = settle_cargo(CARGO_CLAIM_PCT, gross);
        let refused = settle_cargo(CARGO_REJECT_PCT, gross);

        assert!(exception.pay_loss < claim.pay_loss);
        assert!(claim.pay_loss < refused.pay_loss);
        assert!(exception.reputation_hit < claim.reputation_hit);
        assert!(claim.reputation_hit < refused.reputation_hit);
        assert_eq!(exception.claim_value, 0.0);
        assert!(claim.claim_value > 0.0);
        assert!(refused.claim_value > claim.claim_value);
    }

    #[test]
    fn test_a_refused_load_pays_nothing_at_all() {
        // The harsh top end: the driver delivered nothing.
        let settled = settle_cargo(90.0, 3000.0);
        assert!(settled.rejected());
        assert!(approx(settled.pay_loss, 3000.0));
        // And the freight itself is owed on top of the unpaid haul.
        assert!(settled.claim_value > 3000.0);
    }

    #[test]
    fn test_the_condition_words_map_one_to_one_onto_the_outcomes() {
        assert_eq!(cargo_condition_text(0.0, false), "secure");
        assert_eq!(cargo_condition_text(5.0, false), "shifted but sound");
        assert_eq!(cargo_condition_text(CARGO_EXCEPTION_PCT, false), "damaged");
        assert_eq!(
            cargo_condition_text(CARGO_CLAIM_PCT, false),
            "badly damaged"
        );
        assert_eq!(cargo_condition_text(CARGO_REJECT_PCT, false), "ruined");
    }

    #[test]
    fn test_a_tank_load_is_described_in_words_that_fit_a_tank() {
        assert_eq!(cargo_condition_text(0.0, true), "settled");
        assert_eq!(cargo_condition_text(5.0, true), "worked");
        assert_eq!(cargo_condition_text(20.0, true), "off spec");
        assert_eq!(cargo_condition_text(40.0, true), "contaminated");
        assert_eq!(cargo_condition_text(80.0, true), "lost");
        // And the dry vocabulary is untouched.
        assert_eq!(cargo_condition_text(0.0, false), "secure");
        assert_eq!(cargo_condition_text(5.0, false), "shifted but sound");
        assert_eq!(cargo_condition_text(80.0, false), "ruined");
    }

    #[test]
    fn a_negative_gross_settles_as_zero() {
        let settled = settle_cargo(90.0, -500.0);
        assert_eq!(settled.pay_loss, 0.0);
        assert_eq!(settled.claim_value, 0.0);
        assert_eq!(settled.reputation_hit, CARGO_REJECT_REPUTATION);
        assert_eq!(settled.condition_pct, 90.0);
    }

    // -- the meter (TruckState._update_cargo) --------------------------------

    const MPS_PER_MPH: f64 = 1.0 / 2.23694;

    /// A bend's radius, in feet, for a given lateral pull at a given speed --
    /// the inverse of what the model computes, so a test can ask for "0.1 g
    /// over the threshold" without hand-solving the geometry each time.
    fn radius_for(mph: f64, lateral_g: f64) -> f64 {
        let mps = mph * MPS_PER_MPH;
        mps * mps / (lateral_g * 9.81) / 0.3048
    }

    fn loaded(mph: f64) -> TruckState {
        TruckState {
            trailer_attached: true,
            cargo_kg: 12_000.0,
            velocity_mps: mph * MPS_PER_MPH,
            ..TruckState::default()
        }
    }

    #[test]
    fn test_gentle_driving_never_touches_the_load() {
        let mut t = loaded(60.0);
        for _ in 0..600 {
            t.update_cargo(1.0 / 60.0, CARGO_HARD_BRAKE_G - 0.05);
        }
        assert_eq!(t.cargo_damage_pct, 0.0);
    }

    #[test]
    fn test_hard_braking_shifts_the_load() {
        let mut t = loaded(60.0);
        for _ in 0..120 {
            t.update_cargo(1.0 / 60.0, CARGO_HARD_BRAKE_G + 0.3);
        }
        assert!(t.cargo_damage_pct > 0.0);
    }

    /// Securement is rated past what the brakes can produce (49 CFR 393.102).
    ///
    /// A stop at everything the service brakes have -- which is what a
    /// startled driver makes several times a run -- used to put general
    /// freight over the exception line on its own, and fragile freight into
    /// claim territory.
    #[test]
    fn test_a_full_service_stop_does_not_hurt_a_secured_load() {
        let mut t = loaded(65.0);
        let peak = t.specs.max_brake_decel_g;
        assert!(peak < CARGO_HARD_BRAKE_G); // the premise: brakes cannot reach it
        for _ in 0..600 {
            t.update_cargo(1.0 / 60.0, peak);
        }
        assert_eq!(t.cargo_damage_pct, 0.0);
    }

    /// The jake on through a full service stop is still an ordinary stop.
    ///
    /// In a low gear the retarder adds about a fifth of a g on top of the
    /// drums, which used to put a street-corner stop over the securement line
    /// (player report, 2026-10-14).
    #[test]
    fn test_a_full_service_stop_with_the_jake_on_does_not_hurt_the_load() {
        let mut t = TruckState {
            trailer_attached: true,
            cargo_kg: 5_000.0,
            engine_on: true,
            ..TruckState::default()
        };
        t.transmission.automatic = true;
        t.transmission.gear = 5;
        t.velocity_mps = 15.0 * MPS_PER_MPH;
        t.update(0.0);
        t.engine_brake_stage = 3;
        t.brake = 1.0;
        assert!(t.jake_brake_force() > 0.0);
        for _ in 0..600 {
            t.update(1.0 / 60.0);
        }
        assert!(t.velocity_mps < 0.5);
        assert_eq!(t.cargo_damage_pct, 0.0);
    }

    /// Full service plus the spring brakes is past what securement holds.
    #[test]
    fn test_an_emergency_application_does_reach_the_freight() {
        let mut t = loaded(65.0);
        let emergency_g = t.specs.max_brake_decel_g * EMERGENCY_BRAKE_MULT;
        assert!(emergency_g > CARGO_HARD_BRAKE_G);
        for _ in 0..300 {
            t.update_cargo(1.0 / 60.0, emergency_g);
        }
        assert!(t.cargo_damage_pct > 0.0);
    }

    /// The break harness found a hairpin at 45 over doing nothing at all.
    #[test]
    fn test_a_bend_taken_well_over_its_advisory_costs_the_freight() {
        let mut t = loaded(75.0);
        t.corner_radius_ft = radius_for(30.0, CARGO_ADVISORY_LAT_G); // a 30 mph bend
        for _ in 0..120 {
            t.update_cargo(1.0 / 60.0, 0.0);
        }
        assert!(t.cargo_damage_pct > 0.0);
    }

    #[test]
    fn test_a_bend_taken_at_its_advisory_is_free() {
        let mut t = loaded(45.0);
        // A bend signed for 45: the shipped advisories sit below the threshold.
        t.corner_radius_ft = radius_for(45.0, CARGO_ADVISORY_LAT_G);
        for _ in 0..600 {
            t.update_cargo(1.0 / 60.0, 0.0);
        }
        assert_eq!(t.cargo_damage_pct, 0.0);
    }

    /// The realism bug the playtest bench caught: the ranking was inverted.
    ///
    /// A hairpin and a sweeper taken the same mph over their advisories are
    /// not the same manoeuvre -- the hairpin throws the load half again as
    /// hard -- but the old model read raw mph over the sign and charged the
    /// hairpin less.
    #[test]
    fn test_the_tighter_bend_costs_more_at_the_same_margin_over_its_sign() {
        let mut costs = Vec::new();
        for advisory in [30.0_f64, 55.0] {
            let mut t = loaded(advisory + 15.0);
            t.corner_radius_ft = radius_for(advisory, CARGO_ADVISORY_LAT_G);
            for _ in 0..120 {
                t.update_cargo(1.0 / 60.0, 0.0);
            }
            costs.push(t.cargo_damage_pct);
        }
        let (hairpin, sweeper) = (costs[0], costs[1]);
        assert!(hairpin > sweeper && sweeper > 0.0);
    }

    /// A gap in the map must not read as a straight road.
    #[test]
    fn test_a_bend_without_a_baked_radius_falls_back_on_its_advisory() {
        let mut t = loaded(60.0);
        t.corner_radius_ft = 0.0;
        t.corner_advisory_mph = 30.0;
        assert!(t.roll_share() > ROLL_WARN_SHARE);
        for _ in 0..120 {
            t.update_cargo(1.0 / 60.0, 0.0);
        }
        assert!(t.cargo_damage_pct > 0.0);
    }

    #[test]
    fn test_a_straight_road_pulls_nothing_sideways() {
        let t = loaded(70.0);
        assert_eq!(t.corner_lateral_g(), 0.0);
    }

    #[test]
    fn test_fragile_freight_degrades_faster_than_general() {
        let mut rates = Vec::new();
        for (key, fragile) in [("general", false), ("electronics", true)] {
            let mut t = loaded(50.0);
            t.cargo_fragility = cargo_fragility_for(key, fragile);
            t.corner_radius_ft = radius_for(30.0, CARGO_ADVISORY_LAT_G);
            for _ in 0..120 {
                t.update_cargo(1.0 / 60.0, 0.0);
            }
            rates.push(t.cargo_damage_pct);
        }
        assert!(rates[1] > rates[0] * 2.0);
    }

    #[test]
    fn test_an_empty_trailer_has_nothing_to_damage() {
        let mut t = TruckState {
            cargo_kg: 0.0,
            velocity_mps: 60.0 * MPS_PER_MPH,
            corner_radius_ft: radius_for(25.0, CARGO_ADVISORY_LAT_G),
            ..TruckState::default()
        };
        for _ in 0..120 {
            t.update_cargo(1.0 / 60.0, 1.0);
        }
        assert_eq!(t.cargo_damage_pct, 0.0);
        assert_eq!(t.add_cargo_damage(50.0), 0.0);
    }

    #[test]
    fn test_a_collision_goes_through_the_freight_too() {
        let mut t = loaded(50.0);
        t.apply_collision(0.7, true);
        assert!(t.cargo_damage_pct > 0.0);
    }

    // -- cues, status, snapshot, settlement lines -----------------------------

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (_update_cargo_condition) and the app shell"]
    fn test_each_condition_rung_speaks_once_while_driving() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (_update_cargo_condition) and the app shell"]
    fn test_the_coaching_tail_speaks_once_per_episode() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (_update_cargo_condition) and the app shell"]
    fn test_terse_cargo_cues_keep_the_consequence() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving and the app shell"]
    fn test_the_bend_is_fed_to_the_truck_from_the_road() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving and the app shell"]
    fn test_a_connector_ramp_is_not_treated_as_a_signed_bend() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (DrivingStatusScreenState) and the app shell"]
    fn test_the_load_line_names_the_freights_condition() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving and the app shell"]
    fn test_the_job_sets_the_fragility_the_truck_carries() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (snapshot) and the app shell"]
    fn test_cargo_condition_round_trips_through_a_snapshot() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (snapshot) and the app shell"]
    fn test_a_snapshot_without_cargo_keys_resumes_clean() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_menu_states (ArrivalState) and the app shell"]
    fn test_the_settlement_line_names_the_finding_the_cost_and_the_claim() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_menu_states (ArrivalState) and the app shell"]
    fn test_a_company_drivers_claim_sits_with_the_carrier() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_menu_states (ArrivalState) and the app shell"]
    fn test_terse_settlement_line_keeps_every_number() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving (_update_cargo_condition) and the app shell"]
    fn test_a_load_ruined_in_one_hit_warns_once_at_the_state_it_is_in() {}
}
