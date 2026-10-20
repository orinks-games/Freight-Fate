//! The co-driver's personality while the player rides along (owner
//! request, 2026-10-20): a switch per co-driver, off until the player turns
//! it on, rare moments that change nothing about the drive, and nothing at
//! all while the player sleeps.

use ff_core::message_log::MessageCategory;
use ff_core::models::co_driver::{self, personality, PairingReason};
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving_team::{TeamDrive, TeamMode};

const DT: f64 = 1.0 / 60.0;

/// A load with `key` at the wheel, their switch set to `on`.
fn team(key: &'static str, mode: TeamMode, on: bool) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Team Personality"));
    harness.with_drive(move |d, ctx| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        if let Some(p) = ctx.profile.as_mut() {
            personality::set_personality(&mut p.achievement_stats, key, on);
        }
        let member = co_driver::co_driver(key).expect("cast member");
        d.team = Some(TeamDrive::new(member, PairingReason::Classmate));
        d.trip.weather.current = WeatherKind::Clear;
        d.hand_wheel_to_co_driver(ctx, mode);
        d.truck_mut().start_engine();
        d.truck_mut().velocity_mps = 25.0;
    });
    harness.clear_speech();
    harness
}

/// Turn the sky from clear to rain, up to `tries` times, ticking the
/// personality after each turn with the gap since the handover passed.
fn rain(harness: &mut PlaytestHarness, tries: usize) {
    for _ in 0..tries {
        harness.with_drive(|d, ctx| {
            d.trip.game_minutes += 1.0;
            d.trip.weather.current = WeatherKind::Clear;
            d.update_co_driver_personality(ctx, DT);
            d.trip.game_minutes += 30.0;
            d.trip.weather.current = WeatherKind::Rain;
            d.update_co_driver_personality(ctx, DT);
        });
    }
}

fn heard(harness: &PlaytestHarness, text: &str) -> usize {
    harness
        .transcript()
        .iter()
        .filter(|line| line.contains(text))
        .count()
}

#[test]
fn every_co_drivers_personality_is_off_until_the_player_turns_it_on() {
    let mut harness = team("walt", TeamMode::RideAlong, false);
    rain(&mut harness, 20);
    assert_eq!(heard(&harness, "Walt saw this rain coming."), 0);
    assert!(!harness.app.ctx.message_log.has(MessageCategory::CoDriver));
}

#[test]
fn riding_along_walt_saw_the_rain_coming_once_a_load() {
    let mut harness = team("walt", TeamMode::RideAlong, true);
    rain(&mut harness, 20);
    assert_eq!(
        heard(&harness, "Walt saw this rain coming."),
        1,
        "{:#?}",
        harness.transcript()
    );
    let review: Vec<String> = harness
        .app
        .ctx
        .message_log
        .filtered_messages()
        .iter()
        .filter(|m| m.category == MessageCategory::CoDriver)
        .map(|m| m.text.clone())
        .collect();
    assert_eq!(review, vec!["Walt saw this rain coming.".to_string()]);
}

#[test]
fn asleep_there_is_no_personality() {
    let mut harness = team("walt", TeamMode::SleepThrough, true);
    rain(&mut harness, 20);
    assert_eq!(heard(&harness, "Walt saw this rain coming."), 0);
}

#[test]
fn nothing_comes_in_the_first_minutes_after_the_handover() {
    let mut harness = team("walt", TeamMode::RideAlong, true);
    for _ in 0..20 {
        harness.with_drive(|d, ctx| {
            d.trip.weather.current = WeatherKind::Clear;
            d.update_co_driver_personality(ctx, DT);
            d.trip.weather.current = WeatherKind::Rain;
            d.update_co_driver_personality(ctx, DT);
        });
    }
    assert_eq!(heard(&harness, "Walt saw this rain coming."), 0);
}

#[test]
fn a_co_driver_note_holds_a_moment_back() {
    let mut harness = team("walt", TeamMode::RideAlong, true);
    for _ in 0..20 {
        harness.with_drive(|d, ctx| {
            d.trip.game_minutes += 31.0;
            d.trip.weather.current = WeatherKind::Clear;
            d.update_co_driver_personality(ctx, DT);
            d.co_driver_note(
                ctx,
                "Walt plans to stop.".to_string(),
                freight_fate::states::driving_team::notes::NoteWeight::Passing,
            );
            d.trip.weather.current = WeatherKind::Rain;
            d.update_co_driver_personality(ctx, DT);
        });
    }
    assert_eq!(heard(&harness, "Walt saw this rain coming."), 0);
}

#[test]
fn dan_walks_around_the_trailer_and_says_so_once() {
    let mut harness = team("dan", TeamMode::RideAlong, true);
    let notes: Vec<String> = (0..3)
        .map(|_| {
            harness.with_drive(|d, ctx| {
                d.personality_at_stop(ctx, "Dan stopped at the plaza.".to_string(), false, false)
            })
        })
        .collect();
    assert_eq!(
        notes[0],
        "Dan stopped at the plaza. Dan walks around the trailer before pulling out."
    );
    assert_eq!(notes[1], "Dan stopped at the plaza.");
    assert_eq!(notes[2], "Dan stopped at the plaza.");
}

#[test]
fn the_personality_never_touches_the_cab_or_the_radio_the_player_chose() {
    let mut harness = team("walt", TeamMode::RideAlong, true);
    let before = harness.with_drive(|d, _| {
        (
            d.cab.window_open(),
            d.radio.enabled,
            d.radio_station_id.clone(),
        )
    });
    rain(&mut harness, 20);
    let after = harness.with_drive(|d, _| {
        (
            d.cab.window_open(),
            d.radio.enabled,
            d.radio_station_id.clone(),
        )
    });
    assert_eq!(before, after);
}
