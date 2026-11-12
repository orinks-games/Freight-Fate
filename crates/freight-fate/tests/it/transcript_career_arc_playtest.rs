//! Transcript-backed playtests for the months-long 1.9 career arc
//! (port of `tests/test_career_arc_playtest.py`).
//!
//! These drive the real game states end to end: a promotion across a fleet
//! tier must speak the level-up, then the tractor hand-over, all reachable
//! line by line from the settlement menu with the keyboard.
//!
//! `tests/career_1_9_scenarios.py` is a helper module, not a test file, and
//! the parity sweep does not list it as something to port on its own. The two
//! presets this file needs are inlined below as [`CareerStage`]; anything
//! else that wants them should lift them into a shared support module.
//!
//! Seam note: the Python harness patched `ctx.say`/`say_event`, so its
//! transcript was every line the states SUBMITTED. The Rust harness records
//! at `ctx.speech`, under the driving verbosity ladder and the event pacer,
//! so it is what a player hears. Nothing here changes as a result -- the
//! settlement menu speaks its rows on the main channel, which neither the
//! ladder nor the pacer touches.

use ff_core::models::business::{COMPANY_DRIVER, INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::carrier_fleet::{assigned_truck_key, fleet_tier_for_level};
use ff_core::models::jobs::Job;
use ff_core::models::profile::Profile;
use ff_core::models::trucks::TRUCK_CATALOG;
use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::city::open_freight_market;

/// `CareerStagePreset` from `tests/career_1_9_scenarios.py`.
struct CareerStage {
    level: usize,
    deliveries: i64,
    reputation: f64,
    business_status: &'static str,
    authority_readiness: bool,
    owned_trucks: &'static [&'static str],
    owned_trailers: &'static [&'static str],
}

impl CareerStage {
    fn configure(&self, profile: &mut Profile) {
        profile.achievements.push("first_dispatch".to_string());
        profile.career.xp = LEVEL_XP[self.level - 1];
        profile.career.deliveries = self.deliveries;
        profile.career.reputation = self.reputation;
        profile.business_status = self.business_status.to_string();
        profile.authority_readiness = self.authority_readiness;
        profile.owned_trucks = self.owned_trucks.iter().map(|k| k.to_string()).collect();
        profile.owned_trailers = self.owned_trailers.iter().map(|k| k.to_string()).collect();
        if self.business_status != COMPANY_DRIVER {
            profile.trailer_programs = ["dry_van", "reefer", "flatbed", "bulk"]
                .iter()
                .map(|k| k.to_string())
                .collect();
        }
    }
}

const NEW_HIRE: CareerStage = CareerStage {
    level: 1,
    deliveries: 0,
    reputation: 50.0,
    business_status: COMPANY_DRIVER,
    authority_readiness: false,
    owned_trucks: &[],
    owned_trailers: &[],
};
/// Company fleet bands: dispatch assigns better tractors with seniority.
const REGIONAL_FLEET_DRIVER: CareerStage = CareerStage {
    level: 6,
    deliveries: 12,
    reputation: 70.0,
    ..NEW_HIRE
};
const PREMIUM_FLEET_DRIVER: CareerStage = CareerStage {
    level: 13,
    deliveries: 30,
    reputation: 90.0,
    ..NEW_HIRE
};
const FIRST_PICK_DRIVER: CareerStage = CareerStage {
    level: 17,
    deliveries: 45,
    reputation: 95.0,
    ..NEW_HIRE
};
/// Kept so the preset table below reads as the Python one did, and so a
/// later case that needs an owner-operator or an authority does not have to
/// re-derive them.
#[allow(dead_code)]
const OWNER_OPERATOR: CareerStage = CareerStage {
    level: 18,
    deliveries: 35,
    reputation: 80.0,
    business_status: LEASED_OWNER_OPERATOR,
    owned_trucks: &["rig"],
    ..NEW_HIRE
};
#[allow(dead_code)]
const OWN_AUTHORITY: CareerStage = CareerStage {
    level: 27,
    deliveries: 80,
    reputation: 92.0,
    business_status: INDEPENDENT_AUTHORITY,
    authority_readiness: true,
    owned_trucks: &["rig"],
    owned_trailers: &["reefer"],
};

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn test_tier_promotion_speaks_level_up_then_new_tractor() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Fleet Promotion").configure(
        |profile: &mut Profile| {
            profile.career.xp = LEVEL_XP[3] - 10.0;
        },
    ));
    harness.settle_current_delivery();
    let result = harness.read_settlement_lines();

    let profile = harness.app.ctx.profile.as_ref().expect("a career");
    assert_eq!(profile.career.level(), 4);
    assert!(profile.achievements.iter().any(|a| a == "fleet_upgrade"));
    let model = &TRUCK_CATALOG[assigned_truck_key(profile, None::<&Job>)];
    assert!(approx(profile.truck_fuel_gal(), model.specs.fuel_tank_gal));

    result.assert_ordered(&[
        "Level up! You are now level 4",
        "Dispatch upgraded your assigned tractor",
        // Achievement flavor left the live announce (R9); the settlement now
        // names the run's badges in one batched "N new achievements:" row.
        "Newer Iron",
    ]);
    result.assert_screen_reader_friendly();
}

#[test]
fn test_promotion_within_a_tier_stays_quiet_about_equipment() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(
        StartDelivery::named("Same Rig").configure(|profile: &mut Profile| {
            profile.career.xp = LEVEL_XP[1] - 10.0;
        }),
    );
    harness.settle_current_delivery();
    let result = harness.read_settlement_lines();

    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .career
            .level(),
        2
    );

    result.assert_ordered(&["Level up! You are now level 2"]);
    assert!(!result
        .transcript_text()
        .contains("Dispatch upgraded your assigned tractor"));
}

#[test]
fn test_premium_lane_board_offers_eight_jobs() {
    let mut app = TestApp::new();
    let mut profile = Profile::named_in("Premium Board", "Chicago");
    PREMIUM_FLEET_DRIVER.configure(&mut profile);
    app.ctx.profile = Some(profile);

    let jobs = open_freight_market(&mut app.ctx);

    // Seniority deepens the board: eight offers at the premium rank.
    assert_eq!(jobs.len(), 8);
    let cache = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .dispatch_board_cache
        .clone()
        .expect("the board caches itself");
    assert_eq!(cache["key"]["count"], 8);
}

#[test]
fn test_fleet_band_presets_land_in_their_tiers() {
    assert_eq!(
        fleet_tier_for_level(REGIONAL_FLEET_DRIVER.level as i64).key,
        "regional"
    );
    assert_eq!(
        fleet_tier_for_level(PREMIUM_FLEET_DRIVER.level as i64).key,
        "premium"
    );
    assert_eq!(
        fleet_tier_for_level(FIRST_PICK_DRIVER.level as i64).key,
        "first_pick"
    );
}
