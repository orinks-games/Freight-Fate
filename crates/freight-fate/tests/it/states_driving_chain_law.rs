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
    tick(&mut app, &drive);
    assert!(
        spoken(&app).iter().any(|s| s.contains("without chains")),
        "{:?}",
        spoken(&app)
    );
    assert!((money(&app) - (1000.0 - CHAIN_LAW_FINE)).abs() < 0.01);
    assert!((with_drive(&drive, |d| d.ticket_fines_paid) - CHAIN_LAW_FINE).abs() < 0.01);
    // Booked on the licence file like every other citation: charged and
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
    tick(&mut app, &drive);
    assert!((money(&app) - 1000.0).abs() < 0.01);
    assert!(with_drive(&drive, |d| d.chain_law_cited.is_empty()));
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

    // Winter tires satisfy Level 1 but not Level 2.
    let mut app = TestApp::new();
    let drive = law_drive(
        &mut app,
        Law {
            seed: 1,
            level: 1,
            tire_type: TIRE_WINTER,
            ..Law::default()
        },
    );
    tick(&mut app, &drive);
    assert!(spoken(&app).is_empty(), "{:?}", spoken(&app));
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
