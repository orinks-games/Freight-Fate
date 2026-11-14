//! Badges earned at the shipper and in the terminal shops land on the row
//! that earns them: the trailer hooked, walked and refused, the detention
//! paid, the upgrade bought.

use ff_core::models::business::{
    AUTHORITY_ACTIVATION_COST, AUTHORITY_ACTIVATION_DELIVERIES, AUTHORITY_ACTIVATION_LEVEL,
    AUTHORITY_ACTIVATION_REPUTATION, AUTHORITY_ACTIVATION_WORKING_CAPITAL, INDEPENDENT_AUTHORITY,
    LEASED_OWNER_OPERATOR, OWNER_OPERATOR_BUY_IN, OWNER_OPERATOR_DELIVERIES, OWNER_OPERATOR_LEVEL,
    OWNER_OPERATOR_REPUTATION, OWNER_OPERATOR_WORKING_CAPITAL,
};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::trailer_yard::{pickup_plan, preloaded_trailer};
use ff_core::models::trucks::UPGRADE_CATALOG;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::city::{
    BusinessStatusState, EndorsementCourseState, TruckShopState, UpgradeShopState,
};
use freight_fate::states::city_pickup::{PickupFacilityState, PickupOptions};

use crate::states_city_support::{career, finish_timed_state, key, profile_mut, select};

fn earned(app: &TestApp, id: &str) -> bool {
    app.ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements
        .iter()
        .any(|a| a == id)
}

/// A Chicago load out of the facility `facility_id` of `origin_type`.
fn a_job(origin_type: &str, facility_id: &str, distance_mi: f64) -> Job {
    let mut job = Job::new(
        cargo_type("general").expect("general freight"),
        12.0,
        "Chicago",
        "Chicago Cross-Dock",
        "Milwaukee",
        distance_mi,
        1800.0,
        9.0,
    );
    job.origin_type = origin_type.to_string();
    job.origin_facility_id = facility_id.to_string();
    job
}

/// A drop-yard load whose preloaded trailer does (or does not) carry a
/// write-up, found in the deterministic yard rather than patched in.
fn drop_yard_job(defective: bool) -> Job {
    (0..400)
        .map(|i| {
            a_job(
                "cross_dock",
                &format!("badge-cross-dock-{i}"),
                90.0 + f64::from(i),
            )
        })
        .find(|job| preloaded_trailer(job).is_some_and(|t| t.defect().is_some() == defective))
        .unwrap_or_else(|| panic!("no seeded yard staged a {defective} trailer"))
}

/// Check in and start the load; the timed hook or dock screen is up.
fn start_loading(app: &mut TestApp, job: Job) {
    let pickup = PickupFacilityState::new(&app.ctx, job, PickupOptions::default());
    app.push_state(pickup);
    key(app, Key::Return); // check in
    key(app, Key::Return); // load, or drop and hook
}

// -- the shipper ---------------------------------------------------------------------

#[test]
fn first_drop_hook_lands_when_the_hook_finishes() {
    let mut app = TestApp::new();
    career(&mut app, "Hook Badge", "Chicago");
    start_loading(&mut app, drop_yard_job(false));
    assert!(!earned(&app, "first_drop_hook"), "still backing under it");
    finish_timed_state(&mut app);
    assert!(earned(&app, "first_drop_hook"));
}

#[test]
fn first_drop_hook_stays_off_a_live_load() {
    let mut app = TestApp::new();
    career(&mut app, "Dock Badge", "Chicago");
    start_loading(&mut app, a_job("mine_quarry", "badge-quarry", 92.0));
    finish_timed_state(&mut app);
    assert!(earned(&app, "first_day"));
    assert!(!earned(&app, "first_drop_hook"));
}

#[test]
fn hooked_a_bad_one_lands_on_the_walk_around_that_finds_the_write_up() {
    let mut app = TestApp::new();
    career(&mut app, "Walk Badge", "Chicago");
    start_loading(&mut app, drop_yard_job(true));
    finish_timed_state(&mut app);
    // Hooked, but nobody has looked yet: the badge must not give it away.
    assert!(!earned(&app, "hooked_a_bad_one"));
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    assert!(earned(&app, "hooked_a_bad_one"));
}

#[test]
fn hooked_a_bad_one_stays_off_a_sound_trailer() {
    let mut app = TestApp::new();
    career(&mut app, "Sound Walk", "Chicago");
    start_loading(&mut app, drop_yard_job(false));
    finish_timed_state(&mut app);
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    assert!(!earned(&app, "hooked_a_bad_one"));
}

#[test]
fn refused_the_trailer_lands_on_the_refusal() {
    let mut app = TestApp::new();
    career(&mut app, "Refuse Badge", "Chicago");
    start_loading(&mut app, drop_yard_job(true));
    finish_timed_state(&mut app);
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    assert!(!earned(&app, "refused_the_trailer"));
    select::<PickupFacilityState>(&mut app, "Refuse this trailer");
    assert!(earned(&app, "refused_the_trailer"));
}

/// A live load whose shipper runs past the free time.
fn a_slow_shipper(app: &TestApp) -> Job {
    let profile = app.ctx.profile.as_ref().expect("a career");
    (0..400)
        .map(|i| {
            a_job(
                "mine_quarry",
                &format!("badge-quarry-{i}"),
                90.0 + f64::from(i),
            )
        })
        .find(|job| pickup_plan(job, profile).detention_minutes > 0.0)
        .expect("some seeded shipper runs late")
}

#[test]
fn detention_paid_lands_when_an_owner_operator_is_held_past_free_time() {
    // A company driver waits the same hours, but the carrier bills them.
    let mut app = TestApp::new();
    career(&mut app, "Company Wait", "Chicago");
    let job = a_slow_shipper(&app);
    start_loading(&mut app, job);
    finish_timed_state(&mut app);
    assert!(!earned(&app, "detention_paid"));
    drop(app);

    let mut app = TestApp::new();
    career(&mut app, "Owner Wait", "Chicago");
    profile_mut(&mut app).business_status = LEASED_OWNER_OPERATOR.to_string();
    let job = a_slow_shipper(&app);
    start_loading(&mut app, job);
    assert!(!earned(&app, "detention_paid"), "still at the dock");
    finish_timed_state(&mut app);
    assert!(earned(&app, "detention_paid"));
}

// -- the business office -------------------------------------------------------------

#[test]
fn owner_operator_buyin_lands_on_the_buy_in() {
    let mut app = TestApp::new();
    career(&mut app, "Buy In Badge", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        p.career.reputation = OWNER_OPERATOR_REPUTATION;
        p.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL + 500.0);
    }
    app.push_state(BusinessStatusState::new());
    assert!(!earned(&app, "owner_operator_buyin"));
    select::<BusinessStatusState>(&mut app, "Owner-operator buy-in");
    assert_eq!(profile_mut(&mut app).business_status, LEASED_OWNER_OPERATOR);
    assert!(earned(&app, "owner_operator_buyin"));
}

#[test]
fn authority_active_lands_on_activating_own_authority() {
    let mut app = TestApp::new();
    career(&mut app, "Authority Badge", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.trailer_programs = vec!["dry_van".to_string(), "reefer".to_string()];
        p.authority_readiness = true;
        p.career.xp = LEVEL_XP[(AUTHORITY_ACTIVATION_LEVEL - 1) as usize];
        p.career.deliveries = AUTHORITY_ACTIVATION_DELIVERIES;
        p.career.reputation = AUTHORITY_ACTIVATION_REPUTATION;
        p.set_money(AUTHORITY_ACTIVATION_COST + AUTHORITY_ACTIVATION_WORKING_CAPITAL + 750.0);
    }
    app.push_state(BusinessStatusState::new());
    assert!(!earned(&app, "authority_active"));
    select::<BusinessStatusState>(&mut app, "Activate own authority");
    assert_eq!(profile_mut(&mut app).business_status, INDEPENDENT_AUTHORITY);
    assert!(earned(&app, "authority_active"));
}

#[test]
fn self_paid_course_lands_on_paying_for_a_course() {
    let mut app = TestApp::new();
    career(&mut app, "Course Badge", "Chicago");
    profile_mut(&mut app).set_money(50_000.0);
    app.push_state(EndorsementCourseState::new());
    assert!(!earned(&app, "self_paid_course"));
    select::<EndorsementCourseState>(&mut app, "Refrigerated certificate course:");
    assert!(earned(&app, "self_paid_course"));
}

#[test]
fn heavy_hauler_lands_on_buying_that_tractor() {
    let mut app = TestApp::new();
    career(&mut app, "Hauler Badge", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.truck = "rig".to_string();
        p.set_money(1_000_000.0);
    }
    app.push_state(TruckShopState::new(false));
    select::<TruckShopState>(&mut app, "Trainer day cab");
    assert!(!earned(&app, "heavy_hauler"));
    select::<TruckShopState>(&mut app, "Heavy hauler");
    assert!(earned(&app, "heavy_hauler"));
}

#[test]
fn three_trucks_lands_on_the_third_tractor_owned() {
    let mut app = TestApp::new();
    career(&mut app, "Fleet Badge", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.truck = "rig".to_string();
        p.set_money(1_000_000.0);
    }
    app.push_state(TruckShopState::new(false));
    select::<TruckShopState>(&mut app, "Trainer day cab");
    assert_eq!(profile_mut(&mut app).owned_trucks.len(), 2);
    assert!(!earned(&app, "three_trucks"));
    select::<TruckShopState>(&mut app, "Yard mule");
    assert_eq!(profile_mut(&mut app).owned_trucks.len(), 3);
    assert!(earned(&app, "three_trucks"));
}

// -- the upgrade shop ----------------------------------------------------------------

fn an_owner_operator_at_the_shop(app: &mut TestApp) {
    career(app, "Upgrade Badge", "Chicago");
    {
        let p = profile_mut(app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(1_000_000.0);
    }
    app.push_state(UpgradeShopState::new());
}

#[test]
fn first_upgrade_lands_on_the_first_purchase() {
    let mut app = TestApp::new();
    an_owner_operator_at_the_shop(&mut app);
    assert!(!earned(&app, "first_upgrade"));
    key(&mut app, Key::Return);
    assert!(!profile_mut(&mut app).upgrades.is_empty());
    assert!(earned(&app, "first_upgrade"));
}

#[test]
fn all_upgrades_lands_on_the_last_tier_of_the_last_upgrade() {
    let mut app = TestApp::new();
    an_owner_operator_at_the_shop(&mut app);
    let last = UPGRADE_CATALOG
        .iter()
        .find(|u| u.max_tier() >= 2)
        .expect("a tiered upgrade");
    {
        let upgrades = &mut profile_mut(&mut app).upgrades;
        for u in UPGRADE_CATALOG {
            upgrades.insert(u.key.to_string(), u.max_tier());
        }
        upgrades.insert(last.key.to_string(), last.max_tier() - 2);
    }
    select::<UpgradeShopState>(&mut app, last.label);
    assert!(!earned(&app, "all_upgrades"), "one tier still to buy");
    select::<UpgradeShopState>(&mut app, last.label);
    assert!(earned(&app, "all_upgrades"));
}
