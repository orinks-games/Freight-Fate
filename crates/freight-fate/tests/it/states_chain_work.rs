//! Who does the chain work (`ff_core::sim::chain_work`): the co-driver at
//! the wheel does it themselves, on their own logbook, with one note per
//! job and none of the assist's lines; and a law that posts with the truck
//! already on the grade is not cited there (owner report, 2026-11-22).

use ff_core::sim::chain_control::ChainSet;
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::driving_events::chain_stop::ChainStopKind;
use freight_fate::states::driving_team::TeamMode;

use crate::states_driving_menus_support::*;

/// Denver to Silverthorne with a chain-law area over `areas`, rolling at
/// 20 m/s at mile `at` under `weather`, a fresh set aboard, the player's
/// Facility stopping assistance `assist`.
fn chain_drive(
    app: &mut TestApp,
    weather: WeatherKind,
    assist: bool,
    at: f64,
    areas: Vec<(f64, f64)>,
) -> SharedState {
    app.ctx
        .settings
        .set_units_mode(ff_core::units::UnitsMode::Imperial);
    app.ctx.settings.destination_approach_assist = assist;
    let drive = a_drive_between(app, "Denver", "Silverthorne", "Carrier");
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(1000.0);
        p.set_chains_owned(true);
        p.set_chain_wear_pct(0.0);
        p.set_chain_repair_links(2);
        p.fatigue = 20.0;
    }
    with_drive(&drive, |d| {
        d.trip.weather.current = weather;
        // October 15: Colorado's carry season, so a Level 2 chains the rig.
        d.trip.career_hours = Some((288.0 - 80.0) * 24.0 + 12.0);
        d.trip.chain_law_areas = areas;
        d.trip.position_mi = at;
        d.trip.truck.chain_wear_pct = 0.0;
        d.trip.truck.chains_on = false;
        d.trip.truck.tire_type = "all_season".to_string();
        d.trip.truck.velocity_mps = 20.0;
        d.trip.zones = Vec::new();
    });
    app.clear_speech();
    drive
}

/// The co-driver takes the wheel, the player asleep. They drive with every
/// assist lent; the player's own Facility stopping setting is put back to
/// `assist` so each case shows the table ignores it.
fn co_driver_takes_it(app: &mut TestApp, drive: &SharedState, assist: bool) {
    drive_and_ctx(drive, app, |d, ctx| {
        d.hand_wheel_to_co_driver(ctx, TeamMode::SleepThrough)
    });
    app.ctx.settings.destination_approach_assist = assist;
    // The road's lines are kept quiet while the co-driver drives; open them
    // so a line the chain work tried to say is caught here.
    app.ctx.co_driver_away = false;
    app.clear_speech();
}

fn name(drive: &SharedState) -> &'static str {
    with_drive(drive, |d| d.co_driver_at_wheel_name()).expect("the co-driver drives")
}

fn assist(app: &mut TestApp, drive: &SharedState, input: bool) {
    drive_and_ctx(drive, app, |d, ctx| d.update_chain_stop_assist(ctx, input));
    app.ctx.run_deferred();
}

fn chains_then_assist(app: &mut TestApp, drive: &SharedState) {
    drive_and_ctx(drive, app, |d, ctx| d.update_chain_set(ctx));
    assist(app, drive, false);
}

fn pending(drive: &SharedState) -> Option<ChainStopKind> {
    with_drive(drive, |d| d.chain_stop.as_ref().map(|s| s.kind))
}

/// Brake to a stop where the stop asked and let the work happen.
fn come_to_rest(app: &mut TestApp, drive: &SharedState) {
    with_drive(drive, |d| {
        d.trip.position_mi = d
            .chain_stop
            .as_ref()
            .map_or(d.trip.position_mi, |s| s.at_mi);
        d.trip.truck.velocity_mps = 0.0;
    });
    assist(app, drive, false);
}

fn law(app: &mut TestApp, drive: &SharedState) {
    drive_and_ctx(drive, app, |d, ctx| d.update_chain_law(ctx));
    app.ctx.run_deferred();
}

/// Everything said or kept quiet on the drive, as the transcript has it.
fn heard(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .chain(app.main_lines())
        .collect()
}

fn notes(drive: &SharedState) -> Vec<String> {
    with_drive(drive, |d| {
        d.team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .map(|wheel| wheel.log.notes.clone())
            .unwrap_or_default()
    })
}

fn fatigue(app: &TestApp) -> f64 {
    app.ctx.profile.as_ref().expect("a career").fatigue
}

fn chains_on_bare(drive: &SharedState) {
    with_drive(drive, |d| {
        d.trip.truck.chains_on = true;
        d.trip.truck.surface = "dry".to_string();
        d.chain_set = ChainSet::from_truck(true, false);
    });
}

fn none_of_the_assists_lines(app: &TestApp) {
    for line in heard(app) {
        for assist_line in [
            "Chain-up area.",
            "Stopping to put chains on",
            "Chains on in",
            "Chains off in",
            "Release the parking brake to go on.",
            "Bare road.",
            "Chain stop canceled.",
        ] {
            assert!(!line.contains(assist_line), "said {line:?}");
        }
    }
}

#[test]
fn the_co_driver_chains_up_before_the_area_on_their_own_logbook() {
    for player_assist in [false, true] {
        let mut app = TestApp::new();
        let drive = chain_drive(
            &mut app,
            WeatherKind::Ice,
            player_assist,
            19.0,
            vec![(20.0, 26.0)],
        );
        co_driver_takes_it(&mut app, &drive, player_assist);
        let tired = fatigue(&app);
        let their_duty = app.ctx.profile.as_ref().expect("a career").hos.duty_min;
        assist(&mut app, &drive, false);
        assert_eq!(pending(&drive), Some(ChainStopKind::Install));
        // Their own pedals, braking into the pullout, never cancel it.
        assist(&mut app, &drive, true);
        assert_eq!(pending(&drive), Some(ChainStopKind::Install));
        come_to_rest(&mut app, &drive);
        assert!(with_drive(&drive, |d| d.trip.truck.chains_on));
        assert_eq!(
            notes(&drive),
            vec![format!(
                "{} put the chains on before the chain control.",
                name(&drive)
            )],
            "assist {player_assist}"
        );
        // Resting in the bunk only takes fatigue off; the work adds none.
        assert!(fatigue(&app) <= tired, "the work tired the player");
        // The profile holds the co-driver's logbook while they drive: the
        // work is on duty there.
        let their_duty_after = app.ctx.profile.as_ref().expect("a career").hos.duty_min;
        assert!(
            their_duty_after - their_duty >= 25.0,
            "{their_duty} -> {their_duty_after}"
        );
        none_of_the_assists_lines(&app);
        // Rolling on through the control: complying, so no hold and no ticket.
        with_drive(&drive, |d| {
            d.trip.position_mi = 24.0;
            d.trip.truck.velocity_mps = 12.0;
        });
        law(&mut app, &drive);
        assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
        assert!(with_drive(&drive, |d| d.chain_law_cited.is_empty()));
    }
}

#[test]
fn the_co_driver_takes_the_chains_off_on_bare_road_without_damage() {
    for player_assist in [false, true] {
        let mut app = TestApp::new();
        let drive = chain_drive(
            &mut app,
            WeatherKind::Cloudy,
            player_assist,
            10.2,
            vec![(0.0, 10.0)],
        );
        co_driver_takes_it(&mut app, &drive, player_assist);
        chains_on_bare(&drive);
        let damage = with_drive(&drive, |d| d.trip.truck.damage_pct);
        chains_then_assist(&mut app, &drive);
        assert!(pending(&drive).is_none(), "not yet half a mile");
        roll_to_mile(&drive, 10.8);
        chains_then_assist(&mut app, &drive);
        assert_eq!(pending(&drive), Some(ChainStopKind::Remove));
        come_to_rest(&mut app, &drive);
        assert!(!with_drive(&drive, |d| d.trip.truck.chains_on));
        assert_eq!(
            notes(&drive),
            vec![format!(
                "{} took the chains off on bare road.",
                name(&drive)
            )]
        );
        assert_eq!(with_drive(&drive, |d| d.trip.truck.damage_pct), damage);
        none_of_the_assists_lines(&app);
    }
}

#[test]
fn a_co_driver_braking_downhill_keeps_their_chain_off_stop() {
    // Their descent snub is the brake pedal, which read as the driver's
    // input: it cancelled the stop for good and the chains stayed on.
    let mut app = TestApp::new();
    let drive = chain_drive(
        &mut app,
        WeatherKind::Cloudy,
        false,
        10.2,
        vec![(0.0, 10.0)],
    );
    co_driver_takes_it(&mut app, &drive, false);
    chains_on_bare(&drive);
    chains_then_assist(&mut app, &drive);
    roll_to_mile(&drive, 10.8);
    chains_then_assist(&mut app, &drive);
    assert_eq!(pending(&drive), Some(ChainStopKind::Remove));
    for _ in 0..30 {
        assist(&mut app, &drive, true);
    }
    assert_eq!(pending(&drive), Some(ChainStopKind::Remove));
    come_to_rest(&mut app, &drive);
    assert!(!with_drive(&drive, |d| d.trip.truck.chains_on));
}

#[test]
fn the_player_taking_the_wheel_with_chains_on_bare_road_hears_the_bare_road_line() {
    let mut app = TestApp::new();
    let drive = chain_drive(
        &mut app,
        WeatherKind::Cloudy,
        false,
        10.2,
        vec![(0.0, 10.0)],
    );
    co_driver_takes_it(&mut app, &drive, false);
    chains_on_bare(&drive);
    chains_then_assist(&mut app, &drive);
    roll_to_mile(&drive, 10.8);
    chains_then_assist(&mut app, &drive);
    assert_eq!(pending(&drive), Some(ChainStopKind::Remove));
    drive_and_ctx(&drive, &mut app, |d, ctx| d.take_wheel_back(ctx));
    app.ctx.settings.destination_approach_assist = false;
    app.clear_speech();
    for mile in [10.9, 11.0, 11.5] {
        roll_to_mile(&drive, mile);
        chains_then_assist(&mut app, &drive);
    }
    let bare = "Bare road. Stop and take the chains off.".to_string();
    let said = app.event_lines();
    assert_eq!(said.iter().filter(|l| **l == bare).count(), 1, "{said:?}");
    assert!(pending(&drive).is_none());
}

#[test]
fn a_chain_that_breaks_under_the_co_driver_is_fixed_and_noted_without_a_hand_back() {
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, WeatherKind::Snow, false, 3.0, vec![(0.0, 10.0)]);
    co_driver_takes_it(&mut app, &drive, false);
    with_drive(&drive, |d| {
        d.trip.truck.chains_on = false;
        d.trip.truck.chains_just_snapped = true;
        d.trip.truck.chain_wear_pct = 100.0;
        d.trip.truck.damage_pct = 4.0;
        d.chain_set = ChainSet::Broken;
    });
    drive_and_ctx(&drive, &mut app, |d, ctx| d.update_traction_cues(ctx));
    app.ctx.run_deferred();
    let seen = with_drive(&drive, |d| {
        d.team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .map(|wheel| wheel.damage_seen)
    });
    assert_eq!(seen, Some(4.0), "the snap is not damage to hand back for");
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Repair));
    come_to_rest(&mut app, &drive);
    assert!(with_drive(&drive, |d| d.trip.truck.chains_on));
    assert_eq!(
        notes(&drive),
        vec![format!(
            "{} fixed a broken chain with a spare link. Damage 4 percent.",
            name(&drive)
        )]
    );
    for line in heard(&app) {
        assert!(!line.contains("A tire chain broke"), "said {line:?}");
    }
    none_of_the_assists_lines(&app);

    // No spare link left: noted, and nothing to stop for.
    drop(drive);
    drop(app);
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, WeatherKind::Snow, false, 3.0, vec![(0.0, 10.0)]);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_chain_repair_links(0);
    co_driver_takes_it(&mut app, &drive, false);
    with_drive(&drive, |d| {
        d.trip.truck.chains_just_snapped = true;
        d.trip.truck.damage_pct = 4.0;
    });
    drive_and_ctx(&drive, &mut app, |d, ctx| d.update_traction_cues(ctx));
    assist(&mut app, &drive, false);
    assert!(pending(&drive).is_none());
    assert_eq!(
        notes(&drive),
        vec!["A chain broke and there is no spare link. Damage 4 percent.".to_string()]
    );
}

/// Into the area on a bare road, past its midpoint, and then the snow.
fn snow_on_the_grade(app: &mut TestApp, assist_on: bool) -> SharedState {
    let drive = chain_drive(app, WeatherKind::Cloudy, assist_on, 1.0, vec![(2.0, 10.0)]);
    for mile in [1.5, 2.5, 7.0] {
        roll_to_mile(&drive, mile);
        law(app, &drive);
    }
    with_drive(&drive, |d| d.trip.weather.current = WeatherKind::Ice);
    app.clear_speech();
    drive
}

#[test]
fn a_law_that_posts_on_the_grade_is_not_cited_there() {
    let mut app = TestApp::new();
    let drive = snow_on_the_grade(&mut app, false);
    for _ in 0..3 {
        law(&mut app, &drive);
    }
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert!(with_drive(&drive, |d| d.chain_law_cited.is_empty()));
    let said = app.event_lines();
    assert!(
        !said.iter().any(|l| l.starts_with("Rolling into")),
        "{said:?}"
    );
    // Nor does the patrol see it there.
    let sample = with_drive(&drive, |d| {
        let post = d.trip.chain_posts().remove(0);
        d.road_sample(&post)
    });
    assert!(!sample.chains_required, "{sample:?}");
    // The driver chains up from the pause menu if they choose: nothing new
    // is said for them.
    assist(&mut app, &drive, false);
    assert!(pending(&drive).is_none());

    // The same law met by a truck rolling in under it is held and cited.
    drop(drive);
    drop(app);
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, WeatherKind::Ice, false, 1.0, vec![(2.0, 10.0)]);
    for mile in [1.5, 2.5, 7.0] {
        roll_to_mile(&drive, mile);
        law(&mut app, &drive);
    }
    assert!(with_drive(&drive, |d| d.chain_checkpoint_hold));
}

#[test]
fn a_law_raised_on_the_grade_is_not_cited_there() {
    // Colorado Level 1 leaves this tandem-drive truck unchained, so it rolls
    // in legally; ice raises the law to Level 2 past the midpoint.
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, WeatherKind::Snow, false, 1.0, vec![(2.0, 10.0)]);
    for mile in [1.5, 2.5, 7.0] {
        roll_to_mile(&drive, mile);
        law(&mut app, &drive);
    }
    assert_eq!(with_drive(&drive, |d| d.trip.chain_law_level()), 1);
    assert!(with_drive(&drive, |d| d.trip.rig_compliant_at(7.0)));
    with_drive(&drive, |d| d.trip.weather.current = WeatherKind::Ice);
    app.clear_speech();
    for _ in 0..3 {
        law(&mut app, &drive);
    }
    assert_eq!(with_drive(&drive, |d| d.trip.chain_law_level()), 2);
    assert!(!with_drive(&drive, |d| d.chain_checkpoint_hold));
    assert!(with_drive(&drive, |d| d.chain_law_cited.is_empty()));
    let said = app.event_lines();
    assert!(
        !said.iter().any(|l| l.starts_with("Rolling into")),
        "{said:?}"
    );
    let sample = with_drive(&drive, |d| {
        let post = d.trip.chain_posts().remove(0);
        d.road_sample(&post)
    });
    assert!(!sample.chains_required, "{sample:?}");
}

#[test]
fn the_assist_and_the_co_driver_chain_up_on_the_grade_when_the_law_posts() {
    let mut app = TestApp::new();
    let drive = snow_on_the_grade(&mut app, true);
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Install));
    let said = app.event_lines();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("Stopping to put chains on, about "),
        "no chain-up area to name on the grade: {said:?}"
    );
    // Braking to a stop on the grade is not running the control.
    law(&mut app, &drive);
    assert!(with_drive(&drive, |d| d.chain_law_cited.is_empty()));

    drop(drive);
    drop(app);
    let mut app = TestApp::new();
    let drive = snow_on_the_grade(&mut app, false);
    co_driver_takes_it(&mut app, &drive, false);
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Install));
    come_to_rest(&mut app, &drive);
    assert_eq!(
        notes(&drive),
        vec![format!(
            "{} put the chains on when the chain law posted.",
            name(&drive)
        )]
    );
    none_of_the_assists_lines(&app);
}
