//! Ported from `tests/test_carrier_fleet.py`: dispatch-assigned company
//! tractors across the 30-level career. `Profile(name=...)` is the
//! career-side `FakeProfile`, whose `active_truck_key` / `take_slip_seat` /
//! `truck_specs` mirror `profile.py`; the exact picks are pinned against
//! CPython's answers for the same driver names.

use super::*;
use crate::models::business_constants::LEASED_OWNER_OPERATOR;
use crate::models::career::test_profile::{FakeJob, FakeProfile};
use crate::models::career::LEVEL_XP;
use crate::models::career_ladder::CAREER_RANKS;
use crate::models::trucks::{truck_model, TRUCK_CATALOG};

fn profile_at_level(level: usize, name: &str) -> FakeProfile {
    let mut profile = FakeProfile::named(name);
    profile.career.xp = LEVEL_XP[level - 1];
    assert_eq!(profile.career.level(), level as i64);
    profile
}

fn fleet_driver(level: usize) -> FakeProfile {
    profile_at_level(level, "Fleet Driver")
}

fn no_job() -> Option<&'static NoJob> {
    None
}

#[test]
fn test_new_hires_run_the_standard_starter_rig() {
    let profile = fleet_driver(1);
    assert_eq!(assigned_truck_key(&profile, no_job()), "rig");
    assert_eq!(profile.active_truck_key(), "rig");
}

#[test]
fn test_fleet_tiers_cover_the_whole_company_ladder_in_order() {
    assert_eq!(FLEET_TIERS[0].min_level, 1);
    let levels: Vec<i64> = FLEET_TIERS.iter().map(|tier| tier.min_level).collect();
    let mut sorted = levels.clone();
    sorted.sort();
    assert_eq!(levels, sorted);
    let unique: std::collections::BTreeSet<i64> = levels.iter().copied().collect();
    assert_eq!(unique.len(), levels.len());
    // Every pool references real catalog trucks the simulation can build.
    for tier in &FLEET_TIERS {
        assert!(!tier.pool.is_empty());
        for key in tier.pool {
            assert!(TRUCK_CATALOG.contains_key(key), "{key}");
        }
    }
}

#[test]
fn test_tier_upgrades_land_at_the_documented_levels() {
    assert_eq!(fleet_tier_for_level(1).key, fleet_tier_for_level(3).key);
    let boundaries: Vec<i64> = FLEET_TIERS[1..].iter().map(|tier| tier.min_level).collect();
    assert_eq!(boundaries, vec![4, 9, 13, 17]);
    for boundary in boundaries {
        let below = fleet_tier_for_level(boundary - 1);
        let at = fleet_tier_for_level(boundary);
        assert_ne!(below.key, at.key);
    }
}

#[test]
fn test_assignment_is_deterministic_and_varies_by_driver() {
    let a1 = assigned_truck_key(&profile_at_level(9, "Driver A"), no_job());
    let a2 = assigned_truck_key(&profile_at_level(9, "Driver A"), no_job());
    assert_eq!(a1, a2);
    let tier = fleet_tier_for_level(9);
    assert!(tier.pool.contains(&a1));
    // Across many driver names dispatch hands out more than one model.
    let picks: std::collections::BTreeSet<&str> = (0..12)
        .map(|n| assigned_truck_key(&profile_at_level(9, &format!("Driver {n}")), no_job()))
        .collect();
    assert!(picks.len() > 1);
    // CPython's sha256 pick for the same name.
    assert_eq!(a1, "highline_sleeper");
}

#[test]
fn test_fleet_tanks_never_shrink_on_promotion() {
    let mut previous_min = 0.0;
    for tier in &FLEET_TIERS {
        let min_tank = tier
            .pool
            .iter()
            .map(|key| truck_model_or_panic(key).specs.fuel_tank_gal)
            .fold(f64::INFINITY, f64::min);
        assert!(min_tank >= previous_min);
        previous_min = min_tank;
    }
}

#[test]
fn test_owner_operators_keep_their_own_tractor() {
    let mut profile = fleet_driver(18);
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    profile.truck = "highline_sleeper".to_string();
    profile.owned_trucks = vec!["highline_sleeper".to_string()];
    assert_eq!(profile.active_truck_key(), "highline_sleeper");
}

#[test]
fn test_company_driver_specs_follow_the_assigned_tractor() {
    let profile = fleet_driver(13);
    let key = assigned_truck_key(&profile, no_job());
    assert_eq!(profile.active_truck_key(), key);
    assert_eq!(profile.truck_specs(), truck_model_or_panic(key).specs);
    assert_eq!(key, "cabover_revival");
}

#[test]
fn test_assignment_text_is_spoken_plainly() {
    let profile = fleet_driver(9);
    let text = fleet_assignment_text(&profile);
    assert!(text.contains(truck_model_or_panic(assigned_truck_key(&profile, no_job())).label));
    let lowered = text.to_lowercase();
    for marker in ["osm", "_", "tier_", "key="] {
        assert!(!lowered.contains(marker));
    }
    assert_eq!(
        text,
        "Dispatch has you in a highline sleeper from the long-haul fleet: A raised-roof \
         long-haul sleeper with a two hundred gallon tank and honest aerodynamics: built to \
         live on the interstate for days at a time."
    );
}

// -- slip-seating: the tractor is picked for the load ------------------------------

fn job(distance_mi: f64, weight_tons: f64) -> FakeJob {
    FakeJob::sized(distance_mi, weight_tons)
}

#[test]
fn test_every_fleet_tier_offers_a_real_choice_of_equipment() {
    // Dispatch can only match a load if the yard holds different trucks.
    //
    // The regional tier is where slip-seating actually bites, so it has to
    // carry both cab types and all three driveline specs; the long-haul tiers
    // up are sleepers by definition but still need light through heavy.
    let regional = FLEET_TIERS
        .iter()
        .find(|tier| tier.key == "regional")
        .unwrap();
    let cabs: std::collections::BTreeSet<&str> = regional
        .pool
        .iter()
        .map(|key| truck_model_or_panic(key).cab)
        .collect();
    assert_eq!(cabs, [CAB_DAY, CAB_SLEEPER].into_iter().collect());
    for tier in &FLEET_TIERS[1..] {
        let specs: std::collections::BTreeSet<&str> = tier
            .pool
            .iter()
            .map(|key| truck_model_or_panic(key).spec)
            .collect();
        assert!(specs.len() >= 3, "{} {specs:?}", tier.key);
    }
    // Long-haul and up is life-on-the-road work: no day cabs up there.
    for tier in &FLEET_TIERS[2..] {
        assert!(
            tier.pool
                .iter()
                .all(|key| truck_model_or_panic(key).cab == CAB_SLEEPER),
            "{}",
            tier.key
        );
    }
}

#[test]
fn test_a_junior_driver_draws_a_small_stable_set_of_spares() {
    // The yard leaves the same few trucks free, so their wear is knowable.
    //
    // Each tractor keeps its own fuel, wear, and damage, and a driver who drew
    // a brand new truck every load would never watch one age.
    let profile = fleet_driver(6);
    let pool = slip_seat_pool(&profile);
    assert_eq!(pool.len(), SLIP_SEAT_POOL_SIZE);
    let unique: std::collections::BTreeSet<&str> = pool.iter().copied().collect();
    assert_eq!(unique.len(), pool.len());
    assert_eq!(slip_seat_pool(&fleet_driver(6)), pool); // stable across calls
    let tier = fleet_tier_for_level(6);
    assert!(pool.iter().all(|key| tier.pool.contains(key)));
    // Two drivers at the same level do not get the same three trucks.
    let others: std::collections::BTreeSet<Vec<&str>> = (0..12)
        .map(|n| slip_seat_pool(&profile_at_level(6, &format!("Driver {n}"))))
        .collect();
    assert!(others.len() > 1);
    // CPython's rotation for these two names.
    assert_eq!(
        pool,
        ["city_shuttle", "short_haul_stubnose", "midroof_runner"]
    );
    assert_eq!(
        slip_seat_pool(&profile_at_level(6, "Driver 3")),
        [
            "hand_me_down_sleeper",
            "plain_jane_conventional",
            "yard_mule"
        ]
    );
}

#[test]
fn test_a_run_that_needs_a_bunk_never_goes_out_on_a_day_cab() {
    // Hours of service decide this, not preference.
    //
    // Eleven hours of driving does not cover a nine hundred mile run, so the
    // truck has to have a bed in it.
    let long_run = job(900.0, 12.0);
    for n in 0..24 {
        let profile = profile_at_level(6, &format!("Driver {n}"));
        let key = assigned_truck_key(&profile, Some(&long_run));
        assert_eq!(truck_model_or_panic(key).cab, CAB_SLEEPER, "{n} {key}");
    }
    assert_eq!(
        assigned_truck_key(&fleet_driver(6), Some(&long_run)),
        "midroof_runner"
    );
}

#[test]
fn test_a_heavy_load_gets_a_heavy_spec_tractor() {
    let heavy = job(140.0, 24.0);
    let mut picks = std::collections::BTreeSet::new();
    for n in 0..24 {
        let profile = profile_at_level(6, &format!("Driver {n}"));
        let key = assigned_truck_key(&profile, Some(&heavy));
        picks.insert(truck_model_or_panic(key).spec);
    }
    assert_eq!(picks, [SPEC_HEAVY].into_iter().collect());
    assert_eq!(
        assigned_truck_key(&fleet_driver(6), Some(&heavy)),
        "short_haul_stubnose"
    );
}

#[test]
fn test_a_light_local_turn_gets_a_day_cab() {
    // A day's work is day-cab work; the yard keeps its sleepers for the lanes.
    let turn = job(120.0, 8.0);
    let mut day_cabs = 0;
    for n in 0..24 {
        let profile = profile_at_level(6, &format!("Driver {n}"));
        if truck_model_or_panic(assigned_truck_key(&profile, Some(&turn))).cab == CAB_DAY {
            day_cabs += 1;
        }
    }
    // Not every driver's three spares include a day cab, but most yards do.
    assert!(day_cabs >= 12, "{day_cabs}");
    assert_eq!(
        assigned_truck_key(&fleet_driver(6), Some(&turn)),
        "city_shuttle"
    );
}

#[test]
fn test_the_same_load_from_the_same_yard_always_comes_with_the_same_truck() {
    let job = job(700.0, 15.0);
    let first = assigned_truck_key(&fleet_driver(6), Some(&job));
    assert_eq!(assigned_truck_key(&fleet_driver(6), Some(&job)), first);
}

#[test]
fn test_seniority_ends_slip_seating() {
    // A senior driver has a seat of their own and comes back to it.
    let profile = fleet_driver(DEDICATED_TRUCK_LEVEL as usize);
    assert!(!slip_seats(&profile));
    let standing = assigned_truck_key(&profile, no_job());
    for job in [job(120.0, 8.0), job(900.0, 24.0), job(380.0, 14.0)] {
        assert_eq!(assigned_truck_key(&profile, Some(&job)), standing);
    }
    assert!(slip_seats(&fleet_driver(
        (DEDICATED_TRUCK_LEVEL - 1) as usize
    )));
}

#[test]
fn test_new_hires_are_not_shuffled_around_the_yard() {
    // Levels one to three are the trainer truck, every load, every driver.
    for n in 0..8 {
        let mut profile = profile_at_level(2, &format!("Driver {n}"));
        assert_eq!(assigned_truck_key(&profile, Some(&job(900.0, 24.0))), "rig");
        assert_eq!(profile.take_slip_seat(&job(120.0, 8.0)), "rig");
    }
}

#[test]
fn test_taking_a_slip_seat_sticks_for_the_run() {
    // The truck dispatch handed over is the truck the drive uses.
    let mut profile = fleet_driver(6);
    let key = profile.take_slip_seat(&job(900.0, 12.0));
    assert_eq!(profile.active_truck_key(), key);
    assert_eq!(profile.truck_specs(), truck_model_or_panic(&key).specs);
    // A different load can bring a different truck, and that one sticks too.
    let other = profile.take_slip_seat(&job(120.0, 24.0));
    assert_eq!(profile.active_truck_key(), other);
}

#[test]
fn test_a_stale_assignment_falls_back_instead_of_stranding_the_driver() {
    // A promotion moves the pool on; the old key must not survive it.
    //
    // Saves written before slip-seating also carry a truck value from the old
    // scheme, and it must not pin a driver to a truck their yard does not hold.
    let mut profile = fleet_driver(6);
    profile.truck = "presidential_sleeper".to_string(); // not in any regional yard
    assert!(fleet_tier_for_level(6)
        .pool
        .contains(&profile.active_truck_key().as_str()));
}

#[test]
fn test_owner_operators_are_never_slip_seated() {
    let mut profile = fleet_driver(6);
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    profile.truck = "highline_sleeper".to_string();
    profile.owned_trucks = vec!["highline_sleeper".to_string()];
    assert_eq!(
        profile.take_slip_seat(&job(900.0, 24.0)),
        "highline_sleeper"
    );
    assert_eq!(profile.active_truck_key(), "highline_sleeper");
}

#[test]
fn test_the_assignment_reason_is_spoken_plainly() {
    // The driver is told which truck this run is in, in driver words.
    let profile = fleet_driver(6);
    let long_run = job(900.0, 12.0);
    let text =
        assignment_reason_text::<FakeProfile>(assigned_truck_key(&profile, Some(&long_run)), None);
    let lowered = text.to_lowercase();
    for marker in ["osm", "_", "spec=", "cab=", "key=", "none"] {
        assert!(!lowered.contains(marker));
    }
    assert_eq!(text, "Truck for this run: mid-roof runner.");
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn test_a_dedicated_driver_hears_why_the_yard_held_their_truck_back() {
    // The one thing a held-back driver will ask, said where they will ask it.
    //
    // From level 9 a driver stops slip-seating and has one truck, so there is
    // no draw to announce at dispatch -- and the note went silent entirely.
    // That silence covered the case that most needs words: a driver whose
    // standing has capped the yard below the tractor their level earns.
    // Brandon, level 11, drew a regional-tier yard mule every long haul and
    // asked "what gives?". The explanation existed the whole time, on the
    // standing screen, which you have to already suspect the answer to go and
    // read.
    let mut profile = fleet_driver(11);
    assert!(!slip_seats(&profile), "level 11 is past slip-seating");
    assert!(DEDICATED_TRUCK_LEVEL < 11);

    // Trust on the floor is what caps the yard's iron.
    profile.career.reputation = 0.0;
    assert!(equipment_held_back(&profile));

    let spoken = equipment_hold_text(&profile, false);
    assert!(
        !spoken.is_empty(),
        "a held-back driver must be given a reason"
    );
    // It names all three: what the level earned, why it is being withheld,
    // and the thing that gives it back.
    assert!(spoken.contains(eligible_fleet_tier(&profile).label));
    assert!(spoken.contains("dispatch trust"));
    assert!(spoken.contains("comes back to you"));
    assert_eq!(
        spoken,
        "Your level earns a tractor from the long-haul fleet, but your dispatch trust is down. \
         Bring it back up with clean on-time runs and the long-haul fleet comes back to you."
    );
    assert_eq!(
        equipment_hold_text(&profile, true),
        "Held back from the long-haul fleet: your dispatch trust is down."
    );
    assert_eq!(
        equipment_hold_clause(&profile),
        "The yard is also holding your equipment back: your tractor comes from the yard \
         standard, not the long-haul fleet your level earns."
    );
    assert!(withheld_promotion_text(&profile)
        .starts_with("You keep the standard rig you are in, exactly as it stands. "));
    assert_eq!(
        assignment_reason_text("yard_mule", Some(&profile)),
        "Truck for this run: yard mule. Held back from the long-haul fleet: your dispatch \
         trust is down."
    );

    // A driver in good standing at the same level hears nothing -- there is
    // nothing to explain, and the note must not nag.
    let fine = fleet_driver(11);
    assert!(!equipment_held_back(&fine));
    assert_eq!(equipment_hold_text(&fine, false), "");
    assert_eq!(withheld_promotion_text(&fine), "");
}

#[test]
fn test_fleet_upgrade_announcement_names_the_new_truck() {
    let text = fleet_upgrade_announcement(&profile_at_level(9, "Driver A"));
    let model = truck_model("highline_sleeper").unwrap();
    assert_eq!(
        text,
        format!(
            "Dispatch upgraded your assigned tractor. Now running a {}: {}",
            model.label, model.description
        )
    );
}

// --- tests/test_career_unlocks.py: the ladder names the fleet boundaries ---------

#[test]
fn test_fleet_tier_boundaries_are_named_in_the_ladder() {
    // Dispatch upgrades the assigned tractor at these ranks; the level-up
    // announcement reads the unlock text, so the text must say so.
    for level in [4, 9, 13, 17] {
        assert!(CAREER_RANKS[level - 1]
            .unlock
            .to_lowercase()
            .contains("tractor"));
    }
}

/// Brandon, 2026-08-22: "it still does not tell me whats holding me back
/// from driving the next level truck".
///
/// The hold was already explained -- at the dispatch hand-over, and at the
/// level-up that arrived without a truck. Both are moments. A driver who
/// wants to know where they stand goes to the career stats screen and asks,
/// and that screen did not mention equipment at all, so the answer only ever
/// reached a player who happened to be listening when it went by.
#[test]
fn test_the_stats_screen_answers_what_is_holding_the_next_truck_back() {
    // Held back: what he is in, then why, then what gives it back.
    let mut held = fleet_driver(11);
    held.career.reputation = 0.0;
    let lines = equipment_status_lines(&held);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("Truck: "));
    assert!(lines[1].contains(eligible_fleet_tier(&held).label));
    assert!(lines[1].contains("comes back to you"));

    // In good standing there is nothing to explain, so the screen says what
    // the next tier costs instead of nagging about a hold that is not there.
    let fine = fleet_driver(11);
    let lines = equipment_status_lines(&fine);
    assert_eq!(lines.len(), 1);
    let upcoming = next_fleet_tier(&fine).expect("level 11 has a tier still to earn");
    assert!(lines[0].contains(&format!(
        "At level {}, dispatch moves you up to the {}.",
        upcoming.min_level, upcoming.label
    )));
    assert!(lines[0].contains("assigned by the carrier"));

    // Below the dedicated-truck level the next move also ends the spares.
    let junior = fleet_driver(5);
    let lines = equipment_status_lines(&junior);
    assert!(
        lines[0].ends_with("and gives you the same truck for every load."),
        "{}",
        lines[0]
    );

    // And at the top of the ladder there is no next tier to name.
    let top = fleet_driver(20);
    assert!(next_fleet_tier(&top).is_none());
    assert!(equipment_status_lines(&top)[0].contains("the carrier's best equipment"));
}

#[test]
fn test_the_carriers_record_review_holds_the_iron_and_names_the_day_it_ages_out() {
    // A record over the review floor is the fourth reason the yard holds a
    // tractor back, and the one hold that can promise its own end date.
    // Level 14 earns the fourth tier; guarded caps the yard at the third.
    let mut profile = fleet_driver(14);
    profile.game_hours = 400.0 * 24.0;
    let record = profile.driving_record.as_mut().expect("a record");
    for _ in 0..4 {
        record.record_citation_at(200.0, 380.0 * 24.0);
    }
    assert!(equipment_held_back(&profile));
    let spoken = equipment_hold_text(&profile, false);
    assert!(
        spoken.contains("the carrier's record review found four citations in the last year"),
        "{spoken}"
    );
    assert!(
        spoken.contains("Keep the record clean until the hold lifts"),
        "{spoken}"
    );
    assert!(spoken.contains("comes back to you"), "{spoken}");
    assert_eq!(assigned_fleet_tier(&profile).key, FLEET_TIERS[2].key);
}
