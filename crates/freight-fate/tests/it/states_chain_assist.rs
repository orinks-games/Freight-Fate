//! Chains in the existing assists: stop planning's no-chains reminder and
//! facility stopping assistance's roadside chain work. Neither ever buys
//! anything.

use ff_core::sim::weather::WeatherKind;

use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use ff_core::sim::real_weather::{Observation, RealWeatherProvider};
use ff_core::sim::real_weather_alerts::{WeatherAlert, WeatherAlertsProvider};
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::city::JobBoardState;
use std::sync::Arc;

use crate::states_driving_menus_support::*;

/// Career hours at noon on a day of the career year (day 80 is hour 0).
fn on_day(doy: f64) -> Option<f64> {
    Some((doy - 80.0) * 24.0 + 12.0)
}

/// A Denver-Silverthorne run on I-70 on ice (a posted Level 2), at mile `at`, with a
/// chain-law area over `areas`, on a day of the career year.
fn i70_drive(
    app: &mut TestApp,
    doy: f64,
    chains: bool,
    at: f64,
    areas: Vec<(f64, f64)>,
) -> SharedState {
    let drive = a_drive_between(app, "Denver", "Silverthorne", "Carrier");
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(1000.0);
        p.set_chains_owned(chains);
        p.set_chain_wear_pct(0.0);
        p.set_chain_repair_links(2);
    }
    with_drive(&drive, |d| {
        // Ice posts Level 2, which chains every rig in Colorado.
        d.trip.weather.current = WeatherKind::Ice;
        d.trip.career_hours = on_day(doy);
        d.trip.game_minutes = 0.0;
        d.trip.chain_law_areas = areas;
        d.trip.position_mi = at;
        d.trip.truck.chain_wear_pct = 0.0;
        d.trip.truck.chains_on = false;
        d.trip.truck.velocity_mps = 20.0;
        d.trip.zones = Vec::new();
    });
    app.clear_speech();
    drive
}

fn money(app: &TestApp) -> f64 {
    app.ctx.profile.as_ref().expect("a career").money()
}

fn owned(app: &TestApp) -> bool {
    app.ctx.profile.as_ref().expect("a career").chains_owned()
}

fn remind(app: &mut TestApp, drive: &SharedState) {
    drive_and_ctx(drive, app, |d, ctx| d.update_chain_reminder(ctx));
    app.ctx.run_deferred();
}

fn reminders(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|s| s.starts_with("Chain law ahead"))
        .collect()
}

/// The board's first Enter on a Denver to Silverthorne load, on a day of
/// the career year: the last spoken line, and whether a trip launched.
fn accept_once(hints: bool, chains: bool, doy: f64) -> (String, bool) {
    let mut app = TestApp::new();
    app.ctx.settings.hos_planning_hints = hints;
    let mut p = Profile::named_in("Chain Board", "Denver");
    p.game_hours = on_day(doy).unwrap();
    p.set_chains_owned(chains);
    p.set_chain_wear_pct(0.0);
    p.set_money(1000.0);
    app.ctx.profile = Some(p);
    let job = Job::new(
        cargo_type("general").expect("general freight"),
        12.0,
        "Denver",
        "Denver Cross-Dock",
        "Silverthorne",
        70.0,
        900.0,
        9.0,
    );
    let mut board = JobBoardState::new(&app.ctx, vec![job]);
    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!((p.money() - 1000.0).abs() < 0.01, "never spends");
    assert_eq!(p.chains_owned(), chains, "never buys");
    (
        app.main_lines().last().cloned().unwrap_or_default(),
        p.active_trip.is_some(),
    )
}

#[test]
fn test_the_board_names_a_carry_corridor_and_the_garage_here_before_accepting() {
    let (line, launched) = accept_once(true, false, 288.0); // October 15
    assert_eq!(
        line,
        "Trucks on I-70 must carry chains this season. You have no chains. The garage here \
         sells them. Enter again to accept."
    );
    assert!(!launched, "the first Enter only reminds");
    // Off, chains aboard, or out of Colorado's carry season: no reminder.
    for (hints, chains, doy, why) in [
        (false, false, 288.0, "stop planning off"),
        (true, true, 288.0, "chains aboard"),
        (true, false, 196.0, "July"),
    ] {
        let (line, _) = accept_once(hints, chains, doy);
        assert!(!line.contains("carry chains"), "{why}: {line}");
    }
}

/// The board's first Enter on a load from `origin` to `destination` in
/// July (no carry season anywhere), with a stop-planning driver and no
/// chains, and `event` posted by the Weather Service over every route city
/// (none when empty). The last spoken line.
fn accept_under_alert(origin: &str, destination: &str, event: &str) -> String {
    let sky = RealWeatherProvider::new(Arc::new(|_lat, _lon| Err("offline".to_string())))
        .with_threaded(false);
    accept_under(origin, destination, event, sky)
}

/// `accept_under_alert` with `sky` as the live weather.
fn accept_under(origin: &str, destination: &str, event: &str, sky: RealWeatherProvider) -> String {
    let mut app = TestApp::new();
    app.ctx.settings.hos_planning_hints = true;
    let mut p = Profile::named_in("Chain Alert", origin);
    p.game_hours = on_day(196.0).unwrap(); // July 15
    p.set_chains_owned(false);
    app.ctx.profile = Some(p);
    let alerts = Arc::new(WeatherAlertsProvider::offline());
    if !event.is_empty() {
        let route = app
            .ctx
            .world
            .supported_route(origin, destination, None)
            .expect("a route")
            .expect("a supported route");
        for key in &route.cities {
            let city = app.ctx.world.cities.get(key).expect("a route city");
            alerts.seed(
                city.lat,
                city.lon,
                vec![WeatherAlert {
                    id: format!("alert-{key}"),
                    event: event.to_string(),
                    severity: "Severe".to_string(),
                    headline: format!("{event} issued"),
                    area: city.name.clone(),
                    description: "* WHAT...Heavy snow.".to_string(),
                }],
            );
        }
    }
    // Alerts ride the real weather toggle; the provider is the seeded
    // offline one.
    app.ctx.settings.real_weather = true;
    app.ctx.set_weather_alerts_provider(alerts);
    app.ctx.set_real_weather_provider(Arc::new(sky));
    let job = Job::new(
        cargo_type("general").expect("general freight"),
        12.0,
        origin,
        &format!("{origin} Cross-Dock"),
        destination,
        100.0,
        900.0,
        9.0,
    );
    let mut board = JobBoardState::new(&app.ctx, vec![job]);
    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();
    app.main_lines().last().cloned().unwrap_or_default()
}

#[test]
fn test_the_board_names_a_chain_law_in_effect_on_the_route() {
    let line = accept_under_alert("Sacramento", "Reno", "Winter Storm Warning");
    assert_eq!(
        line,
        "A chain law is in effect on I-80 on this route. You have no chains. The garage here \
         sells them. Enter again to accept."
    );
    let line = accept_under_alert("Sacramento", "Reno", "");
    assert!(!line.contains("chain"), "no alert, no law: {line}");
}

#[test]
fn test_live_snow_at_a_route_city_posts_the_law_on_the_board() {
    // Every station along the way reports heavy snow; no alert is posted.
    let sky = RealWeatherProvider::new(Arc::new(|_lat, _lon| {
        Ok(Observation::new("Heavy Snow", 10.0, Some(-5.0), Some(0.5)))
    }))
    .with_threaded(false);
    let line = accept_under("Sacramento", "Reno", "", sky);
    assert!(line.contains("chain law is in effect on I-80"), "{line}");
}

#[test]
fn test_a_level_1_alert_where_a_tandem_drives_on_stays_quiet_on_the_board() {
    // A winter storm posts Level 1; Colorado lets a tandem drive on.
    let line = accept_under_alert("Denver", "Silverthorne", "Winter Storm Warning");
    assert!(!line.contains("chain"), "Colorado Level 1: {line}");
    // An ice storm posts Level 2 on the same road, which chains every rig.
    let line = accept_under_alert("Denver", "Silverthorne", "Ice Storm Warning");
    assert!(line.contains("chain law is in effect on I-70"), "{line}");
    let line = accept_under_alert("Pendleton", "La Grande", "Ice Storm Warning");
    assert!(line.contains("chain law is in effect on I-84"), "{line}");
    // Oregon Level 1 chains only single-drive-axle trucks: a tandem hears
    // nothing about chains.
    let line = accept_under_alert("Pendleton", "La Grande", "Winter Storm Warning");
    assert!(!line.contains("chain"), "Oregon Level 1: {line}");
}

#[test]
fn test_a_second_enter_accepts_past_the_chain_reminder() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_planning_hints = true;
    let mut p = Profile::named_in("Chain Board", "Denver");
    p.game_hours = on_day(288.0).unwrap();
    app.ctx.profile = Some(p);
    let job = Job::new(
        cargo_type("general").expect("general freight"),
        12.0,
        "Denver",
        "Denver Cross-Dock",
        "Silverthorne",
        70.0,
        900.0,
        9.0,
    );
    let mut board = JobBoardState::new(&app.ctx, vec![job]);
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();
    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();
    assert!(
        app.main_lines().iter().all(|l| !l.contains("carry chains")),
        "{:?}",
        app.main_lines()
    );
}

#[test]
fn test_the_road_reminder_plays_once_per_corridor_and_never_spends() {
    let mut app = TestApp::new();
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.settings.imperial_units = true;
    // Areas at mile 20 (exit 254 at 18.7 before it) and 40 (exit 234 at 38).
    let drive = i70_drive(
        &mut app,
        288.0,
        false,
        5.0,
        vec![(20.0, 26.0), (40.0, 46.0)],
    );
    let clock = app.fake_pacer_clock();
    remind(&mut app, &drive);
    let said = reminders(&app);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("Chain law ahead on I-70 at exit 254."),
        "{said:?}"
    );
    // No store: no garage is open between here and the delivery.
    assert_eq!(
        said[0],
        "Chain law ahead on I-70 at exit 254. You have no chains."
    );
    // Ticking on through the approach doesn't repeat it.
    for at in [6.0, 10.0, 15.0, 19.0] {
        with_drive(&drive, |d| d.trip.position_mi = at);
        remind(&mut app, &drive);
    }
    assert_eq!(reminders(&app).len(), 1, "{:?}", reminders(&app));
    // The next separate corridor plays again.
    clock.advance(120.0); // the first line has long finished
    app.clear_speech();
    with_drive(&drive, |d| d.trip.position_mi = 27.0);
    remind(&mut app, &drive);
    let said = reminders(&app);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("Chain law ahead on I-70 at exit 234."),
        "{said:?}"
    );
    assert!((money(&app) - 1000.0).abs() < 0.01);
    assert!(!owned(&app));
    assert_eq!(with_drive(&drive, |d| d.trip.position_mi), 27.0);
    assert_eq!(with_drive(&drive, |d| d.trip.truck.velocity_mps), 20.0);

    drop(drive);
    drop(app);

    // No mapped exit before the area: no exit clause.
    let mut app = TestApp::new();
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.settings.imperial_units = false;
    let drive = i70_drive(&mut app, 288.0, false, 2.0, vec![(10.0, 16.0)]);
    remind(&mut app, &drive);
    let said = reminders(&app);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(!said[0].contains("exit"), "{said:?}");
    assert_eq!(said[0], "Chain law ahead on I-70. You have no chains.");
}

#[test]
fn test_the_reminder_is_silent_when_it_has_nothing_to_say() {
    let quiet = |hints: bool, chains: bool, sky: WeatherKind, buy: bool| {
        let mut app = TestApp::new();
        app.ctx.settings.hos_planning_hints = hints;
        let drive = i70_drive(&mut app, 288.0, chains, 5.0, vec![(20.0, 26.0)]);
        with_drive(&drive, |d| d.trip.weather.current = sky);
        if buy {
            app.ctx.profile.as_mut().unwrap().set_chains_owned(true);
        }
        remind(&mut app, &drive);
        reminders(&app)
    };
    assert!(
        quiet(false, false, WeatherKind::Ice, false).is_empty(),
        "stop planning off"
    );
    assert!(
        quiet(true, true, WeatherKind::Ice, false).is_empty(),
        "chains aboard"
    );
    assert!(
        quiet(true, false, WeatherKind::Ice, true).is_empty(),
        "chains bought first"
    );
    // A dry October day is in the carry season, but the board reminded
    // about that; on the road only a posted law that chains the rig counts.
    assert!(
        quiet(true, false, WeatherKind::Clear, false).is_empty(),
        "no law posted"
    );
    assert!(
        quiet(true, false, WeatherKind::Snow, false).is_empty(),
        "Colorado Level 1 lets a tandem drive on"
    );
    assert_eq!(
        quiet(true, false, WeatherKind::Ice, false).len(),
        1,
        "the control case"
    );
}

// -- facility stopping assistance: roadside chain work ---------------------

use freight_fate::states::driving_core::is_night;
use freight_fate::states::driving_events::chain_stop::ChainStopKind;

/// A chain drive with facility stopping on, an active law at `level` over
/// `areas`, rolling at 20 m/s at mile `at`, with a set aboard (or not).
fn stop_drive(
    app: &mut TestApp,
    from: &str,
    to: &str,
    level: i64,
    chains: bool,
    at: f64,
    areas: Vec<(f64, f64)>,
) -> SharedState {
    app.ctx.settings.destination_approach_assist = true;
    let drive = a_drive_between(app, from, to, "Carrier");
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(1000.0);
        p.set_chains_owned(chains);
        p.set_chain_wear_pct(0.0);
        p.set_chain_repair_links(2);
    }
    with_drive(&drive, |d| {
        d.trip.weather.current = match level {
            0 => WeatherKind::Clear,
            1 => WeatherKind::Snow,
            _ => WeatherKind::Ice,
        };
        d.trip.career_hours = on_day(288.0);
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

fn assist(app: &mut TestApp, drive: &SharedState, input: bool) {
    drive_and_ctx(drive, app, |d, ctx| d.update_chain_stop_assist(ctx, input));
    app.ctx.run_deferred();
}

fn pending(drive: &SharedState) -> Option<ChainStopKind> {
    with_drive(drive, |d| d.chain_stop.as_ref().map(|s| s.kind))
}

/// Brake to a stop where the assist asked and let it work.
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

fn minutes(drive: &SharedState) -> f64 {
    with_drive(drive, |d| d.trip.game_minutes)
}

/// Move the clock to day or night.
fn set_night(drive: &SharedState, night: bool) {
    with_drive(drive, |d| {
        for _ in 0..48 {
            if is_night(d.trip.local_hour()) == night {
                return;
            }
            d.trip.game_minutes += 60.0;
        }
        panic!("no such hour");
    });
}

fn lines(app: &TestApp) -> Vec<String> {
    app.event_lines()
}

#[test]
fn test_facility_stopping_chains_up_before_a_colorado_level_2_area() {
    for (night, want) in [(false, 30.0), (true, 48.0)] {
        let mut app = TestApp::new();
        let drive = stop_drive(
            &mut app,
            "Denver",
            "Silverthorne",
            2,
            true,
            19.0,
            vec![(20.0, 26.0)],
        );
        set_night(&drive, night);
        assist(&mut app, &drive, false);
        assert_eq!(pending(&drive), Some(ChainStopKind::Install));
        let cue = format!(
            "Chain-up area ahead. Stopping to put chains on, about {} minutes on duty.",
            want as i64
        );
        assert!(lines(&app).contains(&cue), "{:?}", lines(&app));
        // Rolling, it brakes for the pullout.
        assist(&mut app, &drive, false);
        assert!(with_drive(&drive, |d| d.trip.truck.brake) > 0.0);
        assert!(with_drive(&drive, |d| d.chain_stop.as_ref().unwrap().at_mi) >= 20.0);
        let before = minutes(&drive);
        come_to_rest(&mut app, &drive);
        assert!(with_drive(&drive, |d| d.trip.truck.chains_on));
        assert!(pending(&drive).is_none());
        assert!(((minutes(&drive) - before) - want).abs() < 0.5, "{night}");
        assert!(
            lines(&app).iter().any(|s| s
                .starts_with(&format!("Chains on in {} minutes.", want as i64))
                && s.ends_with("Release the parking brake to go on.")),
            "{:?}",
            lines(&app)
        );
        assert!((money(&app) - 1000.0).abs() < 0.01);
        assert!(owned(&app));
        // Cruise comes back under the chain cap.
        drive_and_ctx(&drive, &mut app, |d, ctx| {
            d.trip.truck.engine_on = true;
            d.trip.truck.velocity_mps = 12.0;
            d.engage_cruise(ctx, 55.0, false);
            d.apply_chain_speed_cap(ctx);
        });
        assert_eq!(with_drive(&drive, |d| d.cruise_mph), Some(30.0));
        drop(drive);
        drop(app);
    }
}

#[test]
fn test_no_chain_up_stop_where_the_rig_needs_no_chains() {
    let stops = |from: &str, to: &str, level: i64, winter: bool, on: bool| {
        let mut app = TestApp::new();
        let drive = stop_drive(&mut app, from, to, level, true, 59.0, vec![(60.0, 70.0)]);
        app.ctx.settings.destination_approach_assist = on;
        if winter {
            with_drive(&drive, |d| {
                d.trip.truck.tire_type = ff_core::sim::vehicle::TIRE_WINTER.to_string()
            });
        }
        assist(&mut app, &drive, false);
        pending(&drive).is_some()
    };
    // Colorado Level 1 doesn't chain a tandem-drive truck.
    assert!(!stops("Denver", "Silverthorne", 1, false, true));
    // Wyoming Level 1 takes snow tires.
    assert!(!stops("Cheyenne", "Laramie", 1, true, true));
    // Facility stopping off.
    assert!(!stops("Denver", "Silverthorne", 2, false, false));
    // California chains trucks at Level 1.
    {
        let mut app = TestApp::new();
        let drive = stop_drive(
            &mut app,
            "Sacramento",
            "Reno",
            1,
            true,
            59.0,
            vec![(60.0, 70.0)],
        );
        assert_eq!(
            with_drive(&drive, |d| d.trip.state_code_at(61.0)).as_deref(),
            Some("CA")
        );
    }
    assert!(stops("Sacramento", "Reno", 1, false, true));
    // No chains aboard: no stop, and it says why.
    let mut app = TestApp::new();
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        2,
        false,
        19.0,
        vec![(20.0, 26.0)],
    );
    assist(&mut app, &drive, false);
    assert!(pending(&drive).is_none());
    assert!(
        lines(&app)
            .contains(&"No chains aboard. Facility stopping can't chain you up.".to_string()),
        "{:?}",
        lines(&app)
    );
    assert!(!owned(&app));
    assert!((money(&app) - 1000.0).abs() < 0.01);
}

#[test]
fn test_facility_stopping_takes_chains_off_on_bare_road() {
    let mut app = TestApp::new();
    app.ctx.settings.imperial_units = true;
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        0,
        true,
        10.2,
        vec![(0.0, 10.0)],
    );
    with_drive(&drive, |d| {
        d.trip.truck.chains_on = true;
        d.trip.truck.surface = "dry".to_string();
    });
    assist(&mut app, &drive, false);
    assert!(
        pending(&drive).is_none(),
        "not yet half a mile of bare road"
    );
    with_drive(&drive, |d| d.trip.position_mi = 10.8);
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Remove));
    assert!(lines(&app)
        .contains(&"Chain-off area. Taking chains off, about 10 minutes on duty.".to_string()));
    let before = minutes(&drive);
    come_to_rest(&mut app, &drive);
    assert!(!with_drive(&drive, |d| d.trip.truck.chains_on));
    assert!(((minutes(&drive) - before) - 10.0).abs() < 0.5);
    drop(drive);
    drop(app);

    // Another area within 5 miles keeps them on.
    let mut app = TestApp::new();
    app.ctx.settings.imperial_units = true;
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        0,
        true,
        10.2,
        vec![(0.0, 10.0), (13.0, 20.0)],
    );
    with_drive(&drive, |d| {
        d.trip.truck.chains_on = true;
        d.trip.truck.surface = "dry".to_string();
    });
    assist(&mut app, &drive, false);
    with_drive(&drive, |d| d.trip.position_mi = 10.8);
    assist(&mut app, &drive, false);
    assert!(pending(&drive).is_none());
    assert!(with_drive(&drive, |d| d.trip.truck.chains_on));
    assert!(
        lines(&app).contains(&"Leaving chains on. Another chain area in 2.2 miles.".to_string()),
        "{:?}",
        lines(&app)
    );
}

#[test]
fn test_the_drivers_brake_cancels_a_chain_stop_for_good() {
    let mut app = TestApp::new();
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        2,
        true,
        19.0,
        vec![(20.0, 26.0)],
    );
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Install));
    assist(&mut app, &drive, true);
    assert!(pending(&drive).is_none());
    assert!(lines(&app).contains(&"Chain stop canceled. Chains are yours.".to_string()));
    for _ in 0..3 {
        assist(&mut app, &drive, false);
    }
    assert!(pending(&drive).is_none(), "that area isn't retried");
    assert!(!with_drive(&drive, |d| d.trip.truck.chains_on));
}

#[test]
fn test_no_chain_up_without_time_in_the_14_hour_window() {
    let mut app = TestApp::new();
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        2,
        true,
        19.0,
        vec![(20.0, 26.0)],
    );
    set_night(&drive, false);
    let mode = app.ctx.settings.hos_mode.clone();
    let (_, duty, _) = ff_core::sim::hos::limits(&mode).expect("hours of service enforced");
    app.ctx.profile.as_mut().unwrap().hos.duty_min = duty - 20.0;
    assist(&mut app, &drive, false);
    assert!(pending(&drive).is_none());
    assert!(
        lines(&app).contains(
            &"Chain-up needs about 30 minutes and your 14-hour window has 20 left. Find a place \
              to stop."
                .to_string()
        ),
        "{:?}",
        lines(&app)
    );
}

#[test]
fn test_facility_stopping_tightens_once_and_repairs_with_a_link() {
    let mut app = TestApp::new();
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        2,
        true,
        5.0,
        vec![(0.0, 10.0)],
    );
    with_drive(&drive, |d| {
        d.trip.truck.chains_on = true;
        d.trip.truck.chain_wear_pct = 86.0;
        d.chain_slap_said = true;
    });
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Tighten));
    assert!(lines(&app).contains(
        &"Chain slapping. Pulling over to tighten, about 5 minutes on duty.".to_string()
    ));
    come_to_rest(&mut app, &drive);
    assert!(with_drive(&drive, |d| d.chain_tightened));
    with_drive(&drive, |d| d.trip.truck.velocity_mps = 20.0);
    assist(&mut app, &drive, false);
    assert!(pending(&drive).is_none(), "once per install");

    // A snap with one link left: repaired, none left.
    app.ctx.profile.as_mut().unwrap().set_chain_repair_links(1);
    with_drive(&drive, |d| {
        d.trip.truck.chains_on = false;
        d.trip.truck.chain_wear_pct = 100.0;
        d.chain_assist_snapped = true;
    });
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Repair));
    assert!(lines(&app).contains(
        &"Chain broke. Pulling over to fix it with a spare link, about 15 minutes on duty."
            .to_string()
    ));
    come_to_rest(&mut app, &drive);
    assert!(with_drive(&drive, |d| d.trip.truck.chains_on));
    assert_eq!(app.ctx.profile.as_ref().unwrap().chain_repair_links(), 0);
    // A second snap: no links, the #440 line.
    app.clear_speech();
    with_drive(&drive, |d| {
        d.trip.truck.velocity_mps = 20.0;
        d.trip.truck.chains_on = false;
        d.trip.truck.chain_wear_pct = 100.0;
        d.chain_assist_snapped = true;
    });
    assist(&mut app, &drive, false);
    assert!(pending(&drive).is_none());
    assert!(lines(&app).contains(&"No repair links left. The chain stays off.".to_string()));
    assert!((money(&app) - 1000.0).abs() < 0.01, "nothing is bought");
    assert_eq!(app.ctx.profile.as_ref().unwrap().chain_repair_links(), 0);
}

#[test]
fn test_the_pause_menu_and_the_assist_share_the_same_chain_actions() {
    // The manual rows call the same `do_*` work: same time, same lines as
    // before they were shared.
    let mut app = TestApp::new();
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        2,
        true,
        5.0,
        vec![(0.0, 10.0)],
    );
    set_night(&drive, false);
    with_drive(&drive, |d| {
        d.trip.truck.velocity_mps = 0.0;
        d.trip.truck.surface = "snow".to_string();
    });
    let said = |app: &mut TestApp,
                f: fn(
        &mut freight_fate::states::driving::DrivingState,
        &mut freight_fate::app::GameContext,
    ) -> Option<String>| { drive_and_ctx(&drive, app, f).unwrap_or_default() };
    let on = said(&mut app, |d, ctx| d.do_install_chains(ctx));
    assert!(
        on.starts_with("Chains on in 30 minutes. Keep under "),
        "{on}"
    );
    let tight = said(&mut app, |d, ctx| d.do_tighten_chains(ctx));
    assert!(
        tight.starts_with("Chains tightened in 5 minutes. It is "),
        "{tight}"
    );
    let off = said(&mut app, |d, ctx| d.do_remove_chains(ctx));
    assert!(
        off.starts_with("Chains off in 10 minutes. The set is still fresh."),
        "{off}"
    );
    with_drive(&drive, |d| d.trip.truck.chain_wear_pct = 100.0);
    let fixed = said(&mut app, |d, ctx| d.do_repair_chain(ctx));
    assert!(
        fixed.starts_with("Chain repaired with a spare link. Back on in 15 minutes."),
        "{fixed}"
    );
    // Rolling, each refuses the same way the menu always has.
    with_drive(&drive, |d| d.trip.truck.velocity_mps = 20.0);
    app.clear_speech();
    assert!(drive_and_ctx(&drive, &mut app, |d, ctx| d.do_install_chains(ctx)).is_none());
    assert_eq!(
        app.main_lines(),
        vec!["Come to a complete stop first.".to_string()]
    );
    assert!((money(&app) - 1000.0).abs() < 0.01);
}

#[test]
fn test_the_hours_and_chain_prompts_each_play_once_and_the_third_enter_accepts() {
    // Denver to Silverthorne in October, 30 minutes short of the drive limit.
    let mut app = TestApp::new();
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.settings.hos_mode = "realistic".to_string();
    let mut p = Profile::named_in("Both Prompts", "Denver");
    p.game_hours = on_day(288.0).unwrap();
    let drive_limit = ff_core::sim::hos::limits("realistic").expect("limits").0;
    p.hos.drive(drive_limit - 30.0);
    app.ctx.profile = Some(p);
    let job = Job::new(
        cargo_type("general").expect("general freight"),
        12.0,
        "Denver",
        "Denver Cross-Dock",
        "Silverthorne",
        70.0,
        900.0,
        9.0,
    );
    let mut board = JobBoardState::new(&app.ctx, vec![job]);
    app.clear_speech();
    for _ in 0..3 {
        board.accept(&mut app.ctx, 0);
        app.ctx.run_deferred();
    }
    let lines = app.main_lines();
    let hours = lines
        .iter()
        .filter(|l| l.contains("Needs an extra rest on this shift's hours"))
        .count();
    let chains = lines
        .iter()
        .filter(|l| l.contains("must carry chains this season"))
        .count();
    assert_eq!((hours, chains), (1, 1), "{lines:?}");
    assert!(
        app.ctx.profile.as_ref().unwrap().active_trip.is_some(),
        "the third Enter accepts: {lines:?}"
    );
}

#[test]
fn test_the_chain_control_does_not_cite_a_truck_pulling_in_to_chain_up() {
    // Issue #502: the assist announced the chain-up stop, and while it was
    // still braking for the pullout the chain control at the start of the
    // area saw no chains and wrote a 580 dollar ticket.
    let mut app = TestApp::new();
    let drive = stop_drive(
        &mut app,
        "Denver",
        "Silverthorne",
        2,
        true,
        19.0,
        vec![(20.0, 26.0)],
    );
    assist(&mut app, &drive, false);
    assert_eq!(pending(&drive), Some(ChainStopKind::Install));
    // Still rolling past the control on the way into the pullout.
    let sample = with_drive(&drive, |d| {
        d.trip.position_mi = 20.05;
        d.trip.truck.velocity_mps = 8.0;
        let post = d.trip.chain_posts().remove(0);
        d.road_sample(&post)
    });
    assert!(!sample.chains_required || sample.chains_on, "{sample:?}");
    drive_and_ctx(&drive, &mut app, |d, ctx| d.update_chain_law(ctx));
    assert!(
        !lines(&app)
            .iter()
            .any(|s| s.starts_with("Rolling into an active chain law")),
        "{:?}",
        lines(&app)
    );
    // The same truck with no chain-up on the way is running the control.
    let sample = with_drive(&drive, |d| {
        d.chain_stop = None;
        let post = d.trip.chain_posts().remove(0);
        d.road_sample(&post)
    });
    assert!(sample.chains_required && !sample.chains_on, "{sample:?}");
}
