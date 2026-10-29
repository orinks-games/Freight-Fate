//! The adversarial battery, as tests (port of
//! `tests/adversarial/test_break_scenarios.py`).
//!
//! `freightfate --break-battery` drives the real driving state and menu flow
//! through deliberately unreasonable play -- flooring it through town,
//! coasting a mountain in neutral, save-scumming a traffic stop -- and grades
//! each scenario CLEAN, ODD or ERROR. That is a discovery tool: it goes
//! looking for things nobody thought to assert, and its findings need a human
//! to judge.
//!
//! This file is the gate side of the same battery, and it exists because
//! "discovery tool" was doing too much work as an excuse. On 2026-08-10, four
//! of its nine findings turned out to be artifacts of the harness rather than
//! bugs in the game, and they had sat untriaged because ODD read as "someone
//! should look at this eventually" rather than as a failing test.
//!
//! # The strict-xfail contract, without xfail
//!
//! Python marked a known-open finding `pytest.mark.xfail(strict=True)`, which
//! means three things at once: a known finding may fail, a NEW finding must
//! fail, and a known finding that starts PASSING must fail too, so fixing a
//! bug forces its entry to be deleted in the same change. Rust's test harness
//! has no xfail, and `#[should_panic]` only covers the first two -- a
//! `should_panic` test whose body stops panicking fails, but it cannot carry a
//! reason and it cannot tell "fixed" from "the scenario stopped running".
//!
//! So the third rung is written out by hand in [`check`]:
//!
//! * a scenario NOT in [`KNOWN_OPEN`] that comes back ODD **fails**, naming
//!   every finding;
//! * a scenario IN `KNOWN_OPEN` that comes back ODD **passes**, printing the
//!   reason it is still open;
//! * a scenario IN `KNOWN_OPEN` that comes back CLEAN **fails**, and the
//!   failure says to delete its entry -- the XPASS(strict) case;
//! * ERROR always fails, whatever the list says.
//!
//! Every entry in `KNOWN_OPEN` should also be a bullet on the roadmap. If you
//! fix one, delete the entry here in the same change -- that is what the third
//! rung is for.
//!
//! # Why these are `#[ignore]`
//!
//! The battery drives whole simulated deliveries, which is why Python keeps
//! `-m "not adversarial"` in its addopts. `#[ignore]` is the same deal with
//! the same reason: `cargo test` skips them, and
//!
//! ```text
//! cargo test -p freight-fate --test adversarial -- --ignored
//! ```
//!
//! runs them. `--test-threads` is free to be whatever: every scenario builds a
//! `TestApp`, which holds the process-wide environment lock, so they queue
//! rather than trample each other.

use freight_fate::playtest::breaker::{self, Verdict};

/// Findings that are real, verified, and not yet fixed. Keyed by scenario
/// name; the value is why it is still open, spoken plainly enough to act on.
///
/// Empty since 2026-09-02: `short_hop_streak_xp_farming` was the last entry,
/// and it now measures the design it had been arguing with (the flat
/// completion award is kept; the streak may at most double the road lesson).
const KNOWN_OPEN: &[(&str, &str)] = &[];

fn known_open(name: &str) -> Option<&'static str> {
    KNOWN_OPEN
        .iter()
        .find(|(scenario, _)| *scenario == name)
        .map(|(_, reason)| *reason)
}

/// Run one scenario and apply the contract in the module note.
fn check(name: &str) {
    let outcome = breaker::run_scenario(name)
        .unwrap_or_else(|| panic!("{name} is not a registered scenario"));
    let findings = outcome
        .findings
        .iter()
        .map(|finding| format!("  - {finding}"))
        .collect::<Vec<_>>()
        .join("\n");

    if outcome.verdict == Verdict::Error {
        panic!("{name} crashed:\n{findings}");
    }
    match (known_open(name), outcome.verdict) {
        // A known-open finding, still open: what the list is for.
        (Some(reason), Verdict::Odd) => {
            println!("{name} is a known-open finding: {reason}\n{findings}");
        }
        // XPASS(strict): the bug is gone, so the entry has to go with it.
        (Some(reason), Verdict::Clean) => panic!(
            "{name} now comes back CLEAN, so its KNOWN_OPEN entry in \
             crates/freight-fate/tests/adversarial.rs is stale and must be deleted in this same \
             change (and its roadmap bullet ticked off).\nThe entry said: {reason}"
        ),
        (None, Verdict::Odd) => panic!("{name} found:\n{findings}"),
        (None, Verdict::Clean) => {}
        (_, Verdict::Error) => unreachable!("handled above"),
    }
}

/// One `#[test]` per scenario, named exactly as the scenario is, so a diff of
/// the two suites is greppable (the Python parametrised node ids are the same
/// strings).
macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            #[ignore = "adversarial: drives whole simulated deliveries; run with --ignored"]
            fn $name() {
                check(stringify!($name));
            }
        )*

        /// The names this file actually gates, for the coverage check below.
        const TESTED: &[&str] = &[$(stringify!($name)),*];
    };
}

scenarios!(
    // driving_physics
    floor_it_through_town,
    hairpin_at_70_no_assists,
    reverse_down_the_route,
    slam_reverse_at_speed,
    neutral_coast_mountain,
    dynamite_parking_brake_at_60,
    redline_damage_readout,
    // assists
    assists_fight_descent,
    jake_toggle_fine_dodge,
    gate_overshoot_with_assists,
    ramp_speed_control_handback,
    // enforcement
    scale_check_in_guidance,
    bald_tires_get_a_walk_around,
    scale_pull_over_stands_down_exit,
    // resources
    fuel_rescue_farming,
    hos_marathon_and_rest_cheese,
    microsleep_throttle_through,
    hos_rest_minute_cheese,
    motel_rest_deadline_crunch,
    // career_economy
    settlement_spoken_balance,
    abandon_and_advance_cycle,
    endorsement_wallet_edges,
    credential_ladder_gates,
    money_exact_zero_and_below,
    level_up_at_settlement_boundary,
    owner_op_buyin_at_level_18_boundary,
    short_hop_streak_xp_farming,
    // dispatch_saveload
    dispatch_decline_budget,
    save_scum_enforcement,
    // radio_weather
    chains_on_dry_interstate_at_70,
    glare_ice_full_jake,
    chain_law_citation_balance,
    radio_dial_abuse_offline,
    // settings_misc
    settings_flips_mid_drive,
    waiting_time_warp,
    hazard_ignored_to_100_damage,
    // today_1_9
    abandon_an_empty_run_costs_nothing,
    jake_stays_off_where_it_does_not_belong,
    named_hazards_keep_their_frequency,
    weather_cap_releases_when_the_sky_does,
    brake_lights_never_invent_a_cause,
    // unlawful
    speeding_past_staffed_posts,
    scale_bypass_to_the_end,
    work_zone_speeding_doubles_the_fine,
    repeat_citations_stop_compounding,
    honk_the_air_down_to_the_valve,
    prepass_green_is_not_a_bypass_charge,
);

/// A renamed or deleted scenario must not silently retire its finding.
#[test]
fn every_known_open_finding_names_a_real_scenario() {
    let registered = breaker::scenario_names();
    let unknown: Vec<&str> = KNOWN_OPEN
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !registered.contains(name))
        .collect();
    assert!(
        unknown.is_empty(),
        "KNOWN_OPEN names scenarios that no longer exist: {unknown:?}"
    );
}

/// And a new scenario must not slip in ungated.
///
/// Python collected its parameters from the live registry, so this could not
/// happen there. The macro above is a literal list, so it can -- and a
/// scenario the battery runs but no test node gates is a scenario whose
/// findings nobody sees.
#[test]
fn every_registered_scenario_has_a_test() {
    let missing: Vec<&str> = breaker::scenario_names()
        .into_iter()
        .filter(|name| !TESTED.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "these scenarios have no test node in this file: {missing:?}"
    );
}
