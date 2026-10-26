//! The chain-law checkpoint at the bottom of the grade: the warning, the
//! seeded citation roll, and the two fine multipliers (the driving half of
//! `tests/test_chain_law.py`; the areas, the level and the flashing sign are
//! in `crates/ff-core/tests/sim_chain_law.rs`).
//!
//! Python drove `DrivingUpdateMixin._update_chain_law` against a
//! `SimpleNamespace` stub. Here the same inputs are set on a real drive --
//! the areas, the weather that makes the law active, and the zone the truck
//! is standing in -- because every one of them is a plain field the stub was
//! standing in for.

use ff_core::models::enforcement::{citation_fine, CHAIN_LAW_FINE};
use ff_core::pyrandom::PyRandom;
use ff_core::sim::trip_models::Zone;
use ff_core::sim::vehicle::TIRE_WINTER;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::driving_core::CHAIN_LAW_CHECKPOINT_CHANCE;

use crate::states_driving_menus_support::*;

/// `tests/test_chain_law.py::_law_stub`: an active chain law over miles 0-10,
/// the truck rolling through it out of (or into) compliance.
struct Law {
    seed: i64,
    level: i64,
    position: f64,
    chains_on: bool,
    tire_type: &'static str,
    construction_zone: bool,
    priors: i64,
}

impl Default for Law {
    fn default() -> Self {
        Law {
            seed: 0,
            level: 2,
            position: 6.0,
            chains_on: false,
            tire_type: "all_season",
            construction_zone: false,
            priors: 0,
        }
    }
}

fn law_drive(app: &mut TestApp, law: Law) -> SharedState {
    let drive = a_drive(app);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(1000.0);
        p.driving_record.citations = law.priors;
    }
    with_drive(&drive, |d| {
        d.trip_seed = law.seed;
        d.trip.weather.current = if law.level >= 2 {
            WeatherKind::Ice
        } else {
            WeatherKind::Snow
        };
        d.trip.chain_law_areas = vec![(0.0, 10.0)];
        d.trip.position_mi = law.position;
        d.trip.truck.chains_on = law.chains_on;
        d.trip.truck.tire_type = law.tire_type.to_string();
        d.trip.truck.velocity_mps = 20.0; // rolling, not stopped at the pullout
        d.trip.zones = if law.construction_zone {
            vec![Zone::new(0.0, 10.0, 45.0, "construction")]
        } else {
            Vec::new()
        };
        d.ticket_fines_paid = 0.0;
        d.chain_law_warned.clear();
        d.chain_law_cited.clear();
    });
    app.clear_speech();
    drive
}

fn tick(app: &mut TestApp, drive: &SharedState) {
    drive_and_ctx(drive, app, |d, ctx| d.update_chain_law(ctx));
    app.ctx.run_deferred();
}

fn spoken(app: &TestApp) -> Vec<String> {
    app.event_lines()
}

/// The first seed in `range(40)` whose roll lands under the checkpoint
/// chance, and the first that does not -- `_cited_seed` and its opposite.
fn caught_and_missed() -> (i64, i64) {
    let mut caught = None;
    let mut missed = None;
    for seed in 0..40 {
        let roll = PyRandom::new_from_str(&format!("{seed}:chain-law:0:2")).random();
        if roll < CHAIN_LAW_CHECKPOINT_CHANCE && caught.is_none() {
            caught = Some(seed);
        } else if roll >= CHAIN_LAW_CHECKPOINT_CHANCE && missed.is_none() {
            missed = Some(seed);
        }
    }
    (
        caught.expect("some seed is caught"),
        missed.expect("some seed gets away"),
    )
}

fn cited_seed() -> i64 {
    caught_and_missed().0
}

fn money(app: &TestApp) -> f64 {
    app.ctx.profile.as_ref().expect("a career").money()
}

fn citations(app: &TestApp) -> i64 {
    app.ctx
        .profile
        .as_ref()
        .expect("a career")
        .driving_record
        .citations
}

#[test]
fn test_chain_checkpoint_is_seeded_and_fines_past_the_midpoint() {
    let (caught, missed) = caught_and_missed();

    // Staffed checkpoint: warned once, cited once, fine off the wallet.
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: caught,
            ..Law::default()
        },
    );
    app.seasoned_driver();
    tick(&mut app, &drive);
    assert!(
        spoken(&app).iter().any(|s| s.contains("without chains")),
        "{:?}",
        spoken(&app)
    );
    assert!((money(&app) - (1000.0 - CHAIN_LAW_FINE)).abs() < 0.01);
    assert!((with_drive(&drive, |d| d.ticket_fines_paid) - CHAIN_LAW_FINE).abs() < 0.01);
    // Booked on the license file like every other citation: charged and
    // spoken but never recorded, a career of these still read as a clean
    // safety record (owner report, 2026-09-12).
    assert_eq!(citations(&app), 1);
    // A second tick neither re-warns nor double-fines.
    tick(&mut app, &drive);
    assert!((money(&app) - (1000.0 - CHAIN_LAW_FINE)).abs() < 0.01);
    assert_eq!(citations(&app), 1);
    drop(app);

    // Unstaffed day: the gamble comes off, warning only.
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: missed,
            ..Law::default()
        },
    );
    app.seasoned_driver();
    tick(&mut app, &drive);
    assert!(
        spoken(&app).iter().any(|s| s.contains("without chains")),
        "{:?}",
        spoken(&app)
    );
    assert!((money(&app) - 1000.0).abs() < 0.01);
    drop(app);

    // Before the midpoint there is warning but no citation roll yet.
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: caught,
            position: 2.0,
            ..Law::default()
        },
    );
    app.seasoned_driver();
    tick(&mut app, &drive);
    assert!((money(&app) - 1000.0).abs() < 0.01);
    assert!(with_drive(&drive, |d| d.chain_law_cited.is_empty()));
}

#[test]
fn test_a_new_driver_is_warned_at_the_chain_checkpoint_and_still_held() {
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: cited_seed(),
            ..Law::default()
        },
    );
    tick(&mut app, &drive);
    assert_eq!(with_drive(&drive, |d| d.ticket_fines_paid), 0.0);
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .driving_record
            .citations,
        0
    );
    let warned: Vec<String> = spoken(&app)
        .into_iter()
        .filter(|s| s.contains("gives you a warning"))
        .collect();
    assert_eq!(warned.len(), 1, "{:?}", spoken(&app));
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
}

#[test]
fn test_chain_law_citation_escalates_with_priors_and_says_the_charged_figure() {
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: cited_seed(),
            priors: 2,
            ..Law::default()
        },
    );
    app.seasoned_driver();
    tick(&mut app, &drive);
    let expected = citation_fine(CHAIN_LAW_FINE, 2, false, None);
    assert!((expected - CHAIN_LAW_FINE * 2.0).abs() < 0.01); // 1 + 0.5 x 2 priors
    assert!((with_drive(&drive, |d| d.ticket_fines_paid) - expected).abs() < 0.01);
    // Spoken text quotes what was actually charged, not the base amount.
    let cited: Vec<String> = spoken(&app)
        .into_iter()
        .filter(|s| s.contains("chain-law citation"))
        .collect();
    assert!(!cited.is_empty(), "{:?}", spoken(&app));
    assert!(
        cited[0].contains(&format!(
            "{} dollars",
            ff_core::pyfmt::fmt_grouped(expected, 0)
        )),
        "{}",
        cited[0]
    );
    assert!(
        !cited[0].contains(&format!(
            "{} dollars",
            ff_core::pyfmt::fmt_grouped(CHAIN_LAW_FINE, 0)
        )),
        "{}",
        cited[0]
    );
}

#[test]
fn test_chain_law_citation_doubles_inside_a_construction_zone_and_says_why() {
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: cited_seed(),
            construction_zone: true,
            ..Law::default()
        },
    );
    app.seasoned_driver();
    tick(&mut app, &drive);
    let expected = citation_fine(CHAIN_LAW_FINE, 0, true, None);
    assert!((expected - CHAIN_LAW_FINE * 2.0).abs() < 0.01);
    assert!((money(&app) - (1000.0 - expected)).abs() < 0.01);
    let cited = spoken(&app)
        .into_iter()
        .find(|s| s.contains("chain-law citation"))
        .expect("a citation line");
    assert!(
        cited.contains(&format!(
            "{} dollars",
            ff_core::pyfmt::fmt_grouped(expected, 0)
        )),
        "{cited}"
    );
    assert!(
        cited.contains("doubled") && cited.contains("construction zone"),
        "{cited}"
    );
}

/// A repeat offender caught in the cones pays base x 1.5 x 2, not x 2.5.
#[test]
fn test_the_two_multipliers_compound_on_a_chain_law_citation() {
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: cited_seed(),
            construction_zone: true,
            priors: 1,
            ..Law::default()
        },
    );
    app.seasoned_driver();
    tick(&mut app, &drive);
    let expected = citation_fine(CHAIN_LAW_FINE, 1, true, None);
    assert!((expected - CHAIN_LAW_FINE * 1.5 * 2.0).abs() < 0.01);
    assert!((expected - CHAIN_LAW_FINE * 2.5).abs() > 0.01);
    assert!((with_drive(&drive, |d| d.ticket_fines_paid) - expected).abs() < 0.01);
}

#[test]
fn test_compliance_ends_the_matter() {
    // Chained up: Level 2 has nothing to say.
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: 1,
            chains_on: true,
            ..Law::default()
        },
    );
    tick(&mut app, &drive);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));
    assert!((money(&app) - 1000.0).abs() < 0.01);
    drop(app);

    // Winter tires never excuse a semi, at Level 1 or Level 2: Caltrans,
    // WSDOT and ODOT chain trucks regardless of tires.
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: 1,
            level: 1,
            position: 2.0,
            tire_type: TIRE_WINTER,
            ..Law::default()
        },
    );
    tick(&mut app, &drive);
    assert!(
        spoken(&app).iter().any(|s| s.contains("without chains")),
        "{:?}",
        spoken(&app)
    );
    drop(app);

    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: 1,
            position: 2.0,
            tire_type: TIRE_WINTER,
            ..Law::default()
        },
    );
    tick(&mut app, &drive);
    assert!(
        spoken(&app).iter().any(|s| s.contains("without chains")),
        "{:?}",
        spoken(&app)
    );
}

#[test]
fn test_no_post_sees_a_chain_law_off_the_posted_grade() {
    // Snow anywhere used to let a scale or a patrol write "running the chain
    // control without chains" -- on I-94 in Chicago as readily as on the pass
    // (seasonal audit, 2026-10-01). The law binds on its grade only.
    use ff_core::sim::enforcement_posts::{build_post, KIND_CMV};
    let mut app = TestApp::new();
    let drive = law_drive(&mut app, Law::default());
    let sample_at = |at: f64| {
        with_drive(&drive, |d| {
            d.trip.position_mi = at;
            let post = build_post(at, KIND_CMV, 0, None, 1.0, "", Some(true), 1.0);
            d.road_sample(&post).chains_required
        })
    };
    assert!(sample_at(6.0), "on the posted grade the law applies");
    assert!(!sample_at(20.0), "ten miles past the grade it does not");
}

#[test]
fn test_a_truck_cited_in_a_chain_control_is_not_cited_again_inside_it() {
    // The ticket from the checkpoint is in the cab; a patrol further up the
    // same control does not write it again (player report, 2026-10-17).
    use ff_core::sim::enforcement_posts::{build_post, KIND_CMV};
    let mut app = TestApp::new();
    let drive = law_drive(&mut app, Law::default());
    let required = || {
        with_drive(&drive, |d| {
            let post = build_post(6.0, KIND_CMV, 0, None, 1.0, "", Some(true), 1.0);
            d.road_sample(&post).chains_required
        })
    };
    assert!(required(), "uncited, a patrol can see it");
    with_drive(&drive, |d| {
        let level = d.trip.chain_law_level();
        d.chain_law_cited.insert((0, level));
    });
    assert!(!required(), "cited, the control is settled");
}

#[test]
fn test_winter_tires_on_a_level_1_law_still_get_the_checkpoint() {
    let mut app = TestApp::new();
    let seed = (0..40)
        .find(|seed| {
            PyRandom::new_from_str(&format!("{seed}:chain-law:0:1")).random()
                < CHAIN_LAW_CHECKPOINT_CHANCE
        })
        .expect("some seed is caught");
    let drive = law_drive(
        &mut app,
        Law {
            seed,
            level: 1,
            tire_type: TIRE_WINTER,
            ..Law::default()
        },
    );
    app.seasoned_driver();
    tick(&mut app, &drive);
    assert!(
        spoken(&app)
            .iter()
            .any(|s| s.contains("chain-law citation")),
        "{:?}",
        spoken(&app)
    );
    assert!(money(&app) < 1000.0);
}

fn over_speed_line(imperial: bool) -> String {
    let mut app = TestApp::new();
    app.ctx.settings.imperial_units = imperial;
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.chains_on = true;
        d.trip.truck.velocity_mps = 45.0 * 0.44704;
        d.chains_fast_active = false;
    });
    app.clear_speech();
    drive_and_ctx(&drive, &mut app, |d, ctx| d.update_traction_cues(ctx));
    app.ctx.run_deferred();
    spoken(&app)
        .into_iter()
        .find(|s| s.starts_with("Too fast for chains"))
        .unwrap_or_else(|| panic!("no over-speed line in {:?}", spoken(&app)))
}

#[test]
fn test_the_chain_over_speed_line_is_a_sentence_with_its_unit() {
    assert_eq!(
        over_speed_line(true),
        "Too fast for chains. Keep under 30 miles per hour."
    );
    let metric = over_speed_line(false);
    assert!(metric.contains("kilometers per hour"), "{metric}");
}

#[test]
fn test_a_cited_truck_with_chains_aboard_is_held_until_they_are_on() {
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: cited_seed(),
            ..Law::default()
        },
    );
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_chains_owned(true);
        p.set_chain_wear_pct(0.0);
    }
    with_drive(&drive, |d| d.trip.truck.chain_wear_pct = 0.0);
    tick(&mut app, &drive);
    assert!(
        spoken(&app)
            .iter()
            .any(|s| s.contains("can't go on until your chains are on")),
        "{:?}",
        spoken(&app)
    );
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert!(with_drive(&drive, |d| d.trip.truck.parking_brake));

    // Releasing the brake and rolling: held again.
    with_drive(&drive, |d| {
        d.trip.truck.parking_brake = false;
        d.trip.truck.velocity_mps = 5.0;
    });
    app.clear_speech();
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_chain_checkpoint_hold(ctx)
    });
    app.ctx.run_deferred();
    assert_eq!(with_drive(&drive, |d| d.trip.truck.velocity_mps), 0.0);
    assert!(with_drive(&drive, |d| d.trip.truck.parking_brake));
    assert!(spoken(&app)
        .iter()
        .any(|s| s.starts_with("Held at the chain checkpoint")));

    // Chains on: waved through.
    with_drive(&drive, |d| d.trip.truck.chains_on = true);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_chain_checkpoint_hold(ctx)
    });
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
}

#[test]
fn test_a_cited_truck_with_no_chains_aboard_is_held_too() {
    // 13 CCR 1072(h): no entering a control without devices, so a cited
    // truck with nothing to put on is held, not waved on.
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: cited_seed(),
            ..Law::default()
        },
    );
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_chains_owned(false);
    tick(&mut app, &drive);
    assert!(
        spoken(&app).iter().any(|s| s.contains(
            "You can't go on without chains. Wait here until the chain law lifts, or turn back."
        )),
        "{:?}",
        spoken(&app)
    );
    assert!(!spoken(&app).iter().any(|s| s.contains("next garage")));
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
}

#[test]
fn test_california_always_staffs_its_checkpoints() {
    use freight_fate::states::driving_core::chain_law_checkpoint_chance;
    assert_eq!(chain_law_checkpoint_chance(Some("CA")), 1.0);
    assert_eq!(
        chain_law_checkpoint_chance(Some("CO")),
        CHAIN_LAW_CHECKPOINT_CHANCE
    );
}

/// A Denver-Silverthorne run on I-70 with a clear sky, inside a chain area,
/// past its midpoint, on a given day of the career year (day 80 is March 21).
fn carry_drive(app: &mut TestApp, day_of_year: f64, chains: bool) -> SharedState {
    carry_drive_between(app, "Denver", "Silverthorne", day_of_year, chains)
}

fn carry_drive_between(
    app: &mut TestApp,
    origin: &str,
    destination: &str,
    day_of_year: f64,
    chains: bool,
) -> SharedState {
    let drive = a_drive_between(app, origin, destination, "Carrier");
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(1000.0);
        p.set_chains_owned(chains);
        p.set_chain_wear_pct(0.0);
    }
    with_drive(&drive, |d| {
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.career_hours = Some((day_of_year - 80.0) * 24.0 + 12.0);
        d.trip.game_minutes = 0.0;
        d.trip.chain_law_areas = vec![(0.0, 10.0)];
        d.trip.position_mi = 6.0;
        d.trip.truck.chain_wear_pct = 0.0;
        d.trip.truck.velocity_mps = 20.0;
        d.trip.zones = Vec::new();
    });
    app.clear_speech();
    drive
}

fn carry_tick(app: &mut TestApp, drive: &SharedState) {
    drive_and_ctx(drive, app, |d, ctx| d.update_chain_carry(ctx));
    app.ctx.run_deferred();
}

/// A seed whose carry roll is caught.
fn carry_seed() -> i64 {
    (0..40)
        .find(|seed| {
            PyRandom::new_from_str(&format!("{seed}:chain-carry:0")).random()
                < CHAIN_LAW_CHECKPOINT_CHANCE
        })
        .expect("some seed is caught")
}

#[test]
fn test_no_chains_aboard_on_i70_in_october_is_a_132_dollar_citation() {
    let mut app = TestApp::new();
    let drive = carry_drive(&mut app, 288.0, false); // October 15
    app.seasoned_driver();
    with_drive(&drive, |d| d.trip_seed = carry_seed());
    let date = with_drive(&drive, |d| {
        ff_core::sim::season::date_text(d.trip.current_career_hours().unwrap())
    });
    assert_eq!(date, "October 15");
    carry_tick(&mut app, &drive);
    let lines = spoken(&app);
    assert!(
        lines
            .iter()
            .any(|s| s.starts_with("Colorado requires chains aboard")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|s| s.contains("Citation, 132 dollars")),
        "{lines:?}"
    );
    assert!((money(&app) - 868.0).abs() < 0.01);
}

#[test]
fn test_the_carry_rule_is_quiet_in_july_and_with_chains_aboard() {
    let mut app = TestApp::new();
    let drive = carry_drive(&mut app, 196.0, false); // July 15
    with_drive(&drive, |d| d.trip_seed = carry_seed());
    carry_tick(&mut app, &drive);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));
    drop(app);

    let mut app = TestApp::new();
    let drive = carry_drive(&mut app, 288.0, true);
    with_drive(&drive, |d| d.trip_seed = carry_seed());
    carry_tick(&mut app, &drive);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));
}

#[test]
fn test_montana_warns_about_carrying_chains_in_season_without_a_fine() {
    let mut app = TestApp::new();
    let drive = carry_drive_between(&mut app, "Missoula", "Butte", 288.0, false);
    with_drive(&drive, |d| d.trip_seed = carry_seed());
    assert_eq!(
        with_drive(&drive, |d| d.trip.state_code_at(6.0)).as_deref(),
        Some("MT")
    );
    carry_tick(&mut app, &drive);
    carry_tick(&mut app, &drive);
    let lines = spoken(&app);
    let warned = lines
        .iter()
        .filter(|s| {
            *s == "Montana requires trucks to carry chains from October 1 to April 30. You have none."
        })
        .count();
    assert_eq!(warned, 1, "{lines:?}");
    assert!((money(&app) - 1000.0).abs() < 0.01);

    // A fresh drive (or a resume) in the same season stays quiet: the
    // profile remembers the season.
    let profile = app.ctx.profile.clone();
    drop(app);
    let mut app = TestApp::new();
    let drive = carry_drive_between(&mut app, "Missoula", "Butte", 300.0, false);
    let p = app.ctx.profile.as_mut().expect("a career");
    p.truck_conditions = profile.expect("a career").truck_conditions;
    p.set_chains_owned(false);
    carry_tick(&mut app, &drive);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));

    // Next season (October a year on), it is said again.
    with_drive(&drive, |d| {
        d.trip.career_hours = Some((288.0 + 365.0 - 80.0) * 24.0 + 12.0)
    });
    carry_tick(&mut app, &drive);
    assert!(
        spoken(&app)
            .iter()
            .any(|s| s.starts_with("Montana requires")),
        "{:?}",
        spoken(&app)
    );
    drop(app);

    // July: nothing to say.
    let mut app = TestApp::new();
    let _drive = carry_drive_between(&mut app, "Missoula", "Butte", 196.0, false);
    carry_tick(&mut app, &_drive);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));
}

#[test]
fn test_wyoming_level_1_warns_and_cites_a_truck_on_all_season_tires() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Cheyenne", "Laramie", "Wyoming");
    app.seasoned_driver();
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(1000.0);
    let seed = (0..40)
        .find(|seed| {
            PyRandom::new_from_str(&format!("{seed}:chain-law:0:1")).random()
                < CHAIN_LAW_CHECKPOINT_CHANCE
        })
        .expect("some seed is caught");
    with_drive(&drive, |d| {
        d.trip_seed = seed;
        d.trip.weather.current = WeatherKind::Snow;
        d.trip.chain_law_areas = vec![(0.0, 10.0)];
        d.trip.position_mi = 6.0;
        d.trip.truck.chains_on = false;
        d.trip.truck.tire_type = "all_season".to_string();
        d.trip.truck.velocity_mps = 20.0;
        d.trip.zones = Vec::new();
    });
    app.clear_speech();
    tick(&mut app, &drive);
    let lines = spoken(&app);
    assert!(
        lines
            .iter()
            .any(|s| s == "Rolling into an active chain law without snow tires or chains."),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|s| s.contains("chain-law citation, 250 dollars")),
        "{lines:?}"
    );
    drop(app);

    // Snow tires satisfy Wyoming Level 1.
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Cheyenne", "Laramie", "Wyoming");
    app.seasoned_driver();
    with_drive(&drive, |d| {
        d.trip_seed = seed;
        d.trip.weather.current = WeatherKind::Snow;
        d.trip.chain_law_areas = vec![(0.0, 10.0)];
        d.trip.position_mi = 6.0;
        d.trip.truck.tire_type = TIRE_WINTER.to_string();
        d.trip.truck.velocity_mps = 20.0;
    });
    app.clear_speech();
    tick(&mut app, &drive);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));
}

/// Held on the apron with chains aboard, then the set wears out there: the
/// truck has nothing to put on.
fn held_with_a_worn_out_set(app: &mut TestApp) -> SharedState {
    let drive = law_drive(
        app,
        Law {
            seed: cited_seed(),
            ..Law::default()
        },
    );
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_chains_owned(true);
        p.set_chain_wear_pct(0.0);
    }
    with_drive(&drive, |d| d.trip.truck.chain_wear_pct = 0.0);
    tick(app, &drive);
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
    with_drive(&drive, |d| {
        d.trip.truck.chain_wear_pct = 100.0;
        d.trip.truck.parking_brake = false;
        d.trip.truck.velocity_mps = 5.0;
    });
    app.clear_speech();
    drive_and_ctx(&drive, app, |d, ctx| d.update_chain_checkpoint_hold(ctx));
    app.ctx.run_deferred();
    drive
}

fn pause_labels(app: &mut TestApp, drive: &SharedState) -> Vec<String> {
    use freight_fate::states::base::Menu;
    use freight_fate::states::driving_pause_states::PauseMenuState;
    let mut pause = PauseMenuState::with_drive(drive_ref(drive));
    pause
        .build_items(&mut app.ctx)
        .iter()
        .map(|item| item.text(&pause, &app.ctx))
        .collect()
}

#[test]
fn test_a_held_truck_with_no_chains_to_put_on_hears_the_ways_out() {
    let mut app = TestApp::new();
    let drive = held_with_a_worn_out_set(&mut app);
    assert!(
        spoken(&app).iter().any(|s| s
            == "Held at the chain checkpoint. You have no chains. Call dispatch for a set, wait \
                here until the chain law lifts, or turn back."),
        "{:?}",
        spoken(&app)
    );
    let labels = pause_labels(&mut app, &drive);
    assert!(
        labels.iter().any(|l| l.starts_with("Turn back to ")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("Install snow chains")),
        "{labels:?}"
    );
}

#[test]
fn test_turning_back_from_the_checkpoint_ends_the_hold_at_the_last_city() {
    let mut app = TestApp::new();
    let drive = held_with_a_worn_out_set(&mut app);
    let (minutes_before, cities) = with_drive(&drive, |d| {
        (d.trip.game_minutes, d.trip.route.cities.clone())
    });
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.turn_back_from_chain_checkpoint(ctx)
    })
    .expect("held, so it turns back");
    let origin = app.ctx.world.city(&cities[0]).expect("a city").name.clone();
    assert!(
        line.starts_with(&format!("You turned back to {origin}, ")),
        "{line}"
    );
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert_eq!(with_drive(&drive, |d| d.trip.position_mi), 0.0);
    assert!(with_drive(&drive, |d| d.trip.game_minutes) > minutes_before);
    // Released from the hold, the truck can drive again.
    with_drive(&drive, |d| {
        d.trip.truck.parking_brake = false;
        d.trip.truck.velocity_mps = 5.0;
    });
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_chain_checkpoint_hold(ctx)
    });
    assert_eq!(with_drive(&drive, |d| d.trip.truck.velocity_mps), 5.0);
    // Not held, nothing to turn back from.
    assert!(drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.turn_back_from_chain_checkpoint(ctx)
    })
    .is_none());
}

#[test]
fn test_waiting_at_the_checkpoint_ends_the_hold_when_the_law_lifts() {
    let mut app = TestApp::new();
    let drive = held_with_a_worn_out_set(&mut app);
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
    with_drive(&drive, |d| d.trip.weather.current = WeatherKind::Clear);
    app.clear_speech();
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_chain_checkpoint_hold(ctx)
    });
    app.ctx.run_deferred();
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert!(
        spoken(&app)
            .iter()
            .any(|s| s.starts_with("The chain law has lifted.")),
        "{:?}",
        spoken(&app)
    );
}

/// A chainless truck cited and held at the checkpoint over miles 0-10.
fn held_with_no_chains(app: &mut TestApp) -> SharedState {
    let drive = law_drive(
        app,
        Law {
            seed: cited_seed(),
            ..Law::default()
        },
    );
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_chains_owned(false);
    tick(app, &drive);
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
    drive
}

#[test]
fn test_a_truck_that_turns_back_and_returns_is_held_again_but_not_fined_twice() {
    let mut app = TestApp::new();
    let drive = held_with_no_chains(&mut app);
    let (fined, cited) = (money(&app), citations(&app));
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.turn_back_from_chain_checkpoint(ctx)
    })
    .expect("turned back");
    // Back at the checkpoint at the same level.
    with_drive(&drive, |d| {
        d.trip.position_mi = 6.0;
        d.trip.truck.parking_brake = false;
        d.trip.truck.velocity_mps = 20.0;
    });
    app.clear_speech();
    tick(&mut app, &drive);
    assert!(
        with_drive(&drive, |d| d.chain_checkpoint_hold),
        "held again"
    );
    assert!(
        spoken(&app)
            .iter()
            .any(|s| s.starts_with("Chain checkpoint. The officer stops you again.")),
        "{:?}",
        spoken(&app)
    );
    assert_eq!(money(&app), fined, "no second fine");
    assert_eq!(citations(&app), cited, "no second citation");
}

#[test]
fn test_checkpoint_staffing_and_citations_ride_the_snapshot() {
    let mut app = TestApp::new();
    let drive = held_with_no_chains(&mut app);
    let snap = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    assert_eq!(snap["chain_law_cited"], serde_json::json!([[0, 2]]));
    assert_eq!(
        snap["chain_checkpoint_staffed"],
        serde_json::json!([[0, 2, true]])
    );
}

#[test]
fn test_waiting_at_the_checkpoint_runs_the_clock_on_duty_until_the_law_lifts() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "off".to_string();
    let drive = held_with_no_chains(&mut app);
    // The ice holds for the whole wait: 8 hours on duty, still held.
    with_drive(&drive, |d| d.trip.weather.forced = Some(WeatherKind::Ice));
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let duty_before = app.ctx.profile.as_ref().unwrap().hos.duty_min;
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| d.wait_for_chain_law_to_lift(ctx))
        .expect("held, so it waits");
    assert_eq!(
        line,
        "You waited 8 hours. The chain law still stands. Wait again, or turn back."
    );
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), before + 480.0);
    assert_eq!(
        app.ctx.profile.as_ref().unwrap().hos.duty_min,
        duty_before + 480.0
    );
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
    // The sky clears during the next half hour: the hold ends.
    with_drive(&drive, |d| d.trip.weather.forced = Some(WeatherKind::Clear));
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| d.wait_for_chain_law_to_lift(ctx))
        .expect("still held");
    assert_eq!(
        line,
        "The chain law lifted after 30 minutes. The officer waves you back onto the highway."
    );
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert!(drive_and_ctx(&drive, &mut app, |d, ctx| d.wait_for_chain_law_to_lift(ctx)).is_none());
}

#[test]
fn test_waiting_stops_at_the_duty_window() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive = held_with_no_chains(&mut app);
    with_drive(&drive, |d| d.trip.weather.forced = Some(WeatherKind::Ice));
    app.ctx.profile.as_mut().unwrap().hos.on_duty(800.0);
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| d.wait_for_chain_law_to_lift(ctx))
        .expect("held, so it waits");
    assert!(
        line.starts_with("You waited 30 minutes, and your 14-hour duty window"),
        "{line}"
    );
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
}

#[test]
fn test_turning_back_is_driven_miles_and_the_drive_goes_on() {
    let mut app = TestApp::new();
    let drive = held_with_no_chains(&mut app);
    with_drive(&drive, |d| d.trip.truck.start_engine());
    let (odometer, fuel, phase) = with_drive(&drive, |d| {
        (d.trip.truck.odometer_mi, d.trip.truck.fuel_gal, d.phase)
    });
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.turn_back_from_chain_checkpoint(ctx)
    })
    .expect("turned back");
    assert!((with_drive(&drive, |d| d.trip.truck.odometer_mi) - (odometer + 6.0)).abs() < 1e-9);
    let burned = fuel - with_drive(&drive, |d| d.trip.truck.fuel_gal);
    // Six highway miles at about 6.5 mpg, far more than idling would burn.
    assert!(burned > 0.5 && burned < 2.0, "{burned}");
    assert!(!with_drive(&drive, |d| d.trip.finished), "not delivered");
    assert_eq!(with_drive(&drive, |d| d.phase), phase);
    // The drive goes on: brake off and rolling, nothing holds it.
    with_drive(&drive, |d| {
        d.trip.truck.parking_brake = false;
        d.trip.truck.velocity_mps = 20.0;
    });
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_chain_checkpoint_hold(ctx)
    });
    tick(&mut app, &drive);
    assert_eq!(with_drive(&drive, |d| d.trip.truck.velocity_mps), 20.0);
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
}

/// The Weather Service with its warnings lifted: every point answers with
/// no alerts.
struct ClearedFeed;

impl ff_core::sim::real_traffic::HttpTransport for ClearedFeed {
    fn get(
        &self,
        _: &str,
        _: &[(&str, &str)],
        _: f64,
    ) -> Result<Vec<u8>, ff_core::sim::real_traffic::TransportError> {
        Ok(br#"{"features": []}"#.to_vec())
    }

    fn post(
        &self,
        url: &str,
        _: &[u8],
        _: &[(&str, &str)],
        _: f64,
    ) -> Result<Vec<u8>, ff_core::sim::real_traffic::TransportError> {
        Err(ff_core::sim::real_traffic::TransportError::new(format!(
            "no post to {url}"
        )))
    }
}

#[test]
fn test_a_live_alert_that_expires_mid_wait_lifts_the_hold() {
    use ff_core::sim::real_weather_alerts::{WeatherAlert, WeatherAlertsProvider};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "off".to_string();
    let drive = held_with_no_chains(&mut app);
    // A clear sky; only the Winter Storm Warning posts the law.
    let (lat, lon) = with_drive(&drive, |d| {
        d.trip.weather.forced = Some(WeatherKind::Clear);
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.latlon_at(None)
    });
    assert!(lat != 0.0 || lon != 0.0, "the route has geometry");
    // Each look at the clock is four minutes on, so the cached warning goes
    // stale partway through the wait and the feed is asked again.
    let ticks = Arc::new(AtomicU64::new(0));
    let clock_ticks = Arc::clone(&ticks);
    let alerts = Arc::new(
        WeatherAlertsProvider::new(Arc::new(ClearedFeed))
            .with_threaded(false)
            .with_clock(Arc::new(move || {
                clock_ticks.fetch_add(1, Ordering::SeqCst) as f64 * 240.0
            })),
    );
    let storm = WeatherAlert {
        id: "storm".to_string(),
        event: "Winter Storm Warning".to_string(),
        severity: "Severe".to_string(),
        headline: "Winter Storm Warning issued".to_string(),
        area: "Here".to_string(),
        description: "* WHAT...Heavy snow.".to_string(),
    };
    alerts.seed(lat, lon, vec![storm.clone()]);
    app.ctx.settings.real_weather = true;
    app.ctx.set_weather_alerts_provider(alerts);
    with_drive(&drive, |d| d.trip.live_alerts = vec![storm]);
    assert!(with_drive(&drive, |d| d.trip.chain_law_level()) > 0);
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| d.wait_for_chain_law_to_lift(ctx))
        .expect("held, so it waits");
    assert!(line.starts_with("The chain law lifted after "), "{line}");
    assert!(!line.contains("8 hours"), "{line}");
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert!(ticks.load(Ordering::SeqCst) > 0);
}

#[test]
fn test_turning_back_past_the_drive_limit_is_refused() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive = held_with_no_chains(&mut app);
    let drive_limit = ff_core::sim::hos::limits("realistic").expect("limits").0;
    // A 30-minute break partway, so the 11-hour limit is the nearest.
    let hos = &mut app.ctx.profile.as_mut().unwrap().hos;
    hos.drive(400.0);
    hos.take_break(30.0);
    hos.drive(drive_limit - 401.0);
    let (position, odometer) =
        with_drive(&drive, |d| (d.trip.position_mi, d.trip.truck.odometer_mi));
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.turn_back_from_chain_checkpoint(ctx)
    })
    .expect("held, so it answers");
    assert_eq!(
        line,
        "Turning back takes 8 minutes of driving, more than your hours allow. Wait here for the \
         chain law to lift, or sleep at the checkpoint from the pause menu."
    );
    assert!(
        with_drive(&drive, |d| d.chain_checkpoint_hold),
        "still held"
    );
    assert_eq!(with_drive(&drive, |d| d.trip.position_mi), position);
    assert_eq!(with_drive(&drive, |d| d.trip.truck.odometer_mi), odometer);
    assert_eq!(
        app.ctx.profile.as_ref().unwrap().hos.driving_min,
        drive_limit - 1.0,
        "no driving logged"
    );
}

#[test]
fn test_a_held_truck_out_of_hours_can_always_sleep_at_the_checkpoint() {
    use freight_fate::states::base::Menu;
    use freight_fate::states::driving_rest_states::ShoulderSleepConfirmationState;
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive = held_with_no_chains(&mut app);
    with_drive(&drive, |d| d.trip.weather.forced = Some(WeatherKind::Ice));
    // The 14-hour window nearly used, and a route stop right beside the
    // truck, which would hide the emergency shoulder sleep row.
    app.ctx.profile.as_mut().unwrap().hos.on_duty(830.0);
    with_drive(&drive, |d| {
        let mut stop = d.trip.stops.first().cloned().expect("the route has stops");
        stop.at_mi = d.trip.position_mi + 1.0;
        d.trip.stops.push(stop);
        assert!(d.trip.nearest_stop_within(1.5).is_some());
    });
    let labels = pause_labels(&mut app, &drive);
    assert!(
        labels
            .iter()
            .any(|l| l == "Sleep at the checkpoint (10 hours)"),
        "{labels:?}"
    );
    let (money_before, damage_before) =
        (money(&app), with_drive(&drive, |d| d.trip.truck.damage_pct));
    let anchor = with_drive(&drive, |d| d.trip.position_mi);
    let mut confirm =
        ShoulderSleepConfirmationState::at_chain_checkpoint(drive_ref(&drive), anchor);
    let items = confirm.build_items(&mut app.ctx);
    let index = items
        .iter()
        .position(|item| item.text(&confirm, &app.ctx) == "Sleep at the checkpoint")
        .expect("the sleep row");
    confirm.menu_mut().items = items;
    confirm.menu_mut().index = index;
    confirm.activate(&mut app.ctx);
    let hos = &app.ctx.profile.as_ref().unwrap().hos;
    assert_eq!(hos.duty_min, 0.0);
    assert_eq!(hos.driving_min, 0.0);
    assert_eq!(
        money(&app),
        money_before,
        "no parking ticket at a checkpoint"
    );
    assert_eq!(
        with_drive(&drive, |d| d.trip.truck.damage_pct),
        damage_before
    );
}

#[test]
fn test_turning_back_past_the_8_hour_break_limit_is_refused() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive = held_with_no_chains(&mut app);
    // 15 miles back at 45 mph: a 20-minute drive.
    with_drive(&drive, |d| {
        let (_, city_mi) = d.last_route_city_behind();
        d.trip.position_mi = city_mi + 15.0;
    });
    assert_eq!(with_drive(&drive, |d| d.chain_turn_back_drive().1), 20.0);
    app.ctx.profile.as_mut().unwrap().hos.drive(470.0);
    let position = with_drive(&drive, |d| d.trip.position_mi);
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.turn_back_from_chain_checkpoint(ctx)
    })
    .expect("held, so it answers");
    assert_eq!(
        line,
        "Turning back takes 20 minutes, more driving than you have before your 30-minute break. \
         Wait here, which counts as your break, or sleep at the checkpoint."
    );
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert_eq!(with_drive(&drive, |d| d.trip.position_mi), position);
}

#[test]
fn test_an_expired_window_does_not_offer_a_turn_back_that_would_be_refused() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive = held_with_no_chains(&mut app);
    with_drive(&drive, |d| d.trip.weather.forced = Some(WeatherKind::Ice));
    app.ctx.profile.as_mut().unwrap().hos.on_duty(835.0);
    let line = drive_and_ctx(&drive, &mut app, |d, ctx| d.wait_for_chain_law_to_lift(ctx))
        .expect("held, so it answers");
    assert!(
        line.starts_with("Your 14-hour duty window is nearly used up.")
            && line.ends_with(
                "The chain law still stands. Sleep at the checkpoint from the pause menu."
            ),
        "{line}"
    );
}

#[test]
fn test_an_alert_that_expires_overnight_lifts_the_hold_after_a_checkpoint_sleep() {
    use ff_core::sim::real_weather_alerts::{WeatherAlert, WeatherAlertsProvider};
    use freight_fate::states::base::Menu;
    use freight_fate::states::driving_rest_states::ShoulderSleepConfirmationState;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive = held_with_no_chains(&mut app);
    let (lat, lon) = with_drive(&drive, |d| {
        d.trip.weather.forced = Some(WeatherKind::Clear);
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.latlon_at(None)
    });
    let ticks = Arc::new(AtomicU64::new(0));
    let clock_ticks = Arc::clone(&ticks);
    let alerts = Arc::new(
        WeatherAlertsProvider::new(Arc::new(ClearedFeed))
            .with_threaded(false)
            .with_clock(Arc::new(move || {
                clock_ticks.fetch_add(1, Ordering::SeqCst) as f64 * 3600.0
            })),
    );
    let storm = WeatherAlert {
        id: "storm".to_string(),
        event: "Winter Storm Warning".to_string(),
        severity: "Severe".to_string(),
        headline: "Winter Storm Warning issued".to_string(),
        area: "Here".to_string(),
        description: "* WHAT...Heavy snow.".to_string(),
    };
    alerts.seed(lat, lon, vec![storm.clone()]);
    app.ctx.settings.real_weather = true;
    app.ctx.set_weather_alerts_provider(alerts);
    with_drive(&drive, |d| d.trip.live_alerts = vec![storm]);
    assert!(with_drive(&drive, |d| d.trip.chain_law_level()) > 0);
    let anchor = with_drive(&drive, |d| d.trip.position_mi);
    let mut confirm =
        ShoulderSleepConfirmationState::at_chain_checkpoint(drive_ref(&drive), anchor);
    let items = confirm.build_items(&mut app.ctx);
    let index = items
        .iter()
        .position(|item| item.text(&confirm, &app.ctx) == "Sleep at the checkpoint")
        .expect("the sleep row");
    confirm.menu_mut().items = items;
    confirm.menu_mut().index = index;
    app.clear_speech();
    confirm.activate(&mut app.ctx);
    assert!(
        !with_drive(&drive, |d| d.chain_checkpoint_hold),
        "the hold lifted"
    );
    assert!(
        app.main_lines()
            .iter()
            .any(|s| s.contains("The chain law lifted overnight.")),
        "{:?}",
        app.main_lines()
    );
}

#[test]
fn test_a_checkpoint_sleep_says_the_chain_law_still_stands() {
    use freight_fate::states::base::Menu;
    use freight_fate::states::driving_rest_states::ShoulderSleepConfirmationState;
    let mut app = TestApp::new();
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive = held_with_no_chains(&mut app);
    with_drive(&drive, |d| {
        d.trip.weather.forced = Some(WeatherKind::Ice);
        d.trip.weather.current = WeatherKind::Ice;
    });
    assert!(with_drive(&drive, |d| d.trip.chain_law_level()) > 0);
    let anchor = with_drive(&drive, |d| d.trip.position_mi);
    let mut confirm =
        ShoulderSleepConfirmationState::at_chain_checkpoint(drive_ref(&drive), anchor);
    let items = confirm.build_items(&mut app.ctx);
    let index = items
        .iter()
        .position(|item| item.text(&confirm, &app.ctx) == "Sleep at the checkpoint")
        .expect("the sleep row");
    confirm.menu_mut().items = items;
    confirm.menu_mut().index = index;
    app.clear_speech();
    confirm.activate(&mut app.ctx);
    assert!(
        with_drive(&drive, |d| d.chain_checkpoint_hold),
        "the hold stands"
    );
    assert!(
        app.main_lines()
            .iter()
            .any(|s| s.contains("The chain law still stands.")),
        "{:?}",
        app.main_lines()
    );
}

#[test]
fn test_dispatch_sends_road_service_with_chains_to_a_held_truck() {
    use freight_fate::states::driving_core::MECHANIC_WAIT_MIN;
    use freight_fate::states::driving_dispatch_call::DispatchCallState;
    let mut app = TestApp::new();
    let drive = held_with_a_worn_out_set(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.velocity_mps = 0.0;
        d.trip.truck.parking_brake = true;
    });
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut call = DispatchCallState::new(drive_ref(&drive));
    app.clear_speech();
    activate(&mut call, &mut app.ctx, "Report truck trouble");
    let said = last(&app);
    assert!(
        said.contains("sending road service out with a new set"),
        "{said}"
    );
    assert!(said.contains("Road service brought a new set"), "{said}");
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.chains_owned());
    assert_eq!(p.chain_wear_pct(), 0.0);
    assert_eq!(with_drive(&drive, |d| d.trip.truck.chain_wear_pct), 0.0);
    assert_eq!(
        with_drive(&drive, |d| d.trip.game_minutes),
        before + MECHANIC_WAIT_MIN
    );
    // The hour and a half may have moved the weather on; hold the ice.
    with_drive(&drive, |d| d.trip.weather.current = WeatherKind::Ice);
    let labels = pause_labels(&mut app, &drive);
    assert!(
        labels.iter().any(|l| l.starts_with("Install snow chains")),
        "{labels:?}"
    );
}
