//! The quiet and urgent only speech modes on a real drive: traffic around the
//! truck says nothing below standard, a line a mode leaves out makes no sound
//! either, and the lines that stay are the short ones.
//!
//! A player found quiet still reading out every slow box truck and its speed
//! (2026-10-03); the owner added that a left-out line must drop its
//! notification tone too, while road sounds stay.

use ff_core::sim::enforcement_posts::EnforcementPost;
use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{
    NavigationCue, TrafficPressure, TripEvent, TripEventData, TripEventKind, Zone,
};
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::SpokenMessage;

use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;

const DT: f64 = 1.0 / 60.0;

fn an_app(mode: &str) -> TestApp {
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true;
    app.ctx.settings.driving_speech = mode.to_string();
    app
}

/// Denver to Salt Lake City on an empty road, past the tutorial so the mode
/// applies.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Speech Modes", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .route_from_cities(&["Denver", "Salt Lake City"])
        .expect("Denver to Salt Lake City is a route");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Denver",
        "yard",
        "Salt Lake City",
        200.0,
        900.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(1),
        DRIVE_PHASE_DELIVERY,
        Some(10.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.traffic_manager.rolling_bubble = false;
    drive.trip.traffic_pressures.clear();
    drive.trip.hazard_check_mi = 1e9;
    drive.trip.inspection_check_mi = 1e9;
    drive.trip.set_patrols(Vec::new());
    drive.departure_checked = true;
    drive.tutorial = None;
    drive
}

fn a_gps_cue(text: &str, data: TripEventData) -> TripEvent {
    TripEvent {
        kind: TripEventKind::GpsCue,
        message: SpokenMessage::new(text),
        data,
    }
}

fn a_slow_box_truck() -> TripEvent {
    a_gps_cue(
        "Box truck ahead, 20 miles per hour.",
        TripEventData {
            npc_vehicle: Some(TrafficVehicle::new(
                "test",
                1.0,
                20.0,
                20.0,
                0,
                "slow",
                "box truck",
            )),
            ..TripEventData::default()
        },
    )
}

fn a_pressure(kind: &str) -> TripEvent {
    a_gps_cue(
        "Traffic ahead.",
        TripEventData {
            traffic_pressure: Some(TrafficPressure {
                start_mi: 2.0,
                end_mi: 4.0,
                kind: kind.to_string(),
                direction: "right".to_string(),
                intensity: 0.5,
                target_speed_mph: 45.0,
                reason: "test".to_string(),
            }),
            ..TripEventData::default()
        },
    )
}

fn a_zone(reason: &str, closed_side: Option<&str>) -> Zone {
    let mut zone = Zone::new(2.0, 4.0, 45.0, reason);
    zone.closed_side = closed_side.map(str::to_string);
    zone
}

// -- which mode hears what ------------------------------------------------------------

#[test]
fn test_traffic_around_the_truck_is_traffic() {
    assert_eq!(
        DrivingState::event_category(&a_slow_box_truck()),
        Some(SpeechCategory::Traffic)
    );
    assert_eq!(
        DrivingState::event_category(&a_pressure("traffic_pack")),
        Some(SpeechCategory::Traffic)
    );
}

#[test]
fn test_lane_advice_is_kept_and_the_taper_merge_is_navigation() {
    assert_eq!(
        DrivingState::event_category(&a_pressure("construction_merge")),
        Some(SpeechCategory::Navigation)
    );
    for kind in ["exit", "route_merge"] {
        assert_eq!(
            DrivingState::event_category(&a_pressure(kind)),
            Some(SpeechCategory::NavigationAdvisory),
            "{kind}"
        );
    }
}

#[test]
fn test_cb_word_and_toll_heads_up_are_advisories() {
    let cb = a_gps_cue(
        "Smokey reported ahead.",
        TripEventData {
            cb_patrol: Some(EnforcementPost::new(5.0, "patrol")),
            ..TripEventData::default()
        },
    );
    let toll = a_gps_cue(
        "Toll ahead.",
        TripEventData {
            cue: Some(NavigationCue::new("toll", "toll", 5.0, "Toll ahead.", "")),
            ..TripEventData::default()
        },
    );
    for event in [cb, toll] {
        assert_eq!(
            DrivingState::event_category(&event),
            Some(SpeechCategory::NavigationAdvisory)
        );
    }
}

#[test]
fn test_only_a_lane_closure_ahead_stays_navigation() {
    let closure = a_gps_cue(
        "Construction ahead.",
        TripEventData {
            zone: Some(a_zone("construction", Some("right"))),
            ..TripEventData::default()
        },
    );
    let open = a_gps_cue(
        "Construction ahead.",
        TripEventData {
            zone: Some(a_zone("construction", None)),
            ..TripEventData::default()
        },
    );
    assert_eq!(
        DrivingState::event_category(&closure),
        Some(SpeechCategory::Navigation)
    );
    assert_eq!(
        DrivingState::event_category(&open),
        Some(SpeechCategory::NavigationAdvisory)
    );
}

// -- through the real announce path ---------------------------------------------------

/// What the box truck cue said and played at one mode.
fn box_truck_at(mode: &str) -> (Vec<String>, Vec<String>) {
    let mut app = an_app(mode);
    let mut drive = a_drive(&mut app);
    let audio = app.record_audio();
    app.clear_speech();
    let review_from = app.ctx.message_log.messages.len();

    drive.handle_trip_event(&mut app.ctx, &a_slow_box_truck());
    for _ in 0..30 {
        drive.update_ambient_events(&mut app.ctx, DT);
    }

    let mut heard = app.event_lines();
    heard.extend(
        app.ctx.message_log.messages[review_from..]
            .iter()
            .map(|message| message.text.clone()),
    );
    let tones = audio
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .filter(|key| key.starts_with("events/") || key.starts_with("ui/"))
        .collect();
    app.shutdown();
    (heard, tones)
}

#[test]
fn test_a_slow_box_truck_is_spoken_only_at_standard() {
    let (heard, _) = box_truck_at("standard");
    assert!(
        heard.iter().any(|line| line.contains("Box truck")),
        "{heard:?}"
    );
    for mode in ["quiet", "urgent_only"] {
        let (heard, tones) = box_truck_at(mode);
        assert!(heard.is_empty(), "{mode}: {heard:?}");
        assert!(tones.is_empty(), "{mode}: {tones:?}");
    }
}

#[test]
fn test_urgent_only_still_says_the_work_zone_took_the_cruise() {
    // The pedals are the driver's again. As a zone entry this was dropped
    // whole at urgent only, and the cruise let go without a word.
    let mut app = an_app("urgent_only");
    let mut drive = a_drive(&mut app);
    app.ctx.settings.speed_keeper = false;
    drive.cruise_mph = Some(65.0);
    app.clear_speech();

    let event = TripEvent {
        kind: TripEventKind::ZoneEnter,
        message: SpokenMessage::new("Work zone active. Speed limit 45."),
        data: TripEventData {
            zone: Some(a_zone("construction", None)),
            ..TripEventData::default()
        },
    };
    drive.handle_trip_event(&mut app.ctx, &event);

    assert!(drive.cruise_mph.is_none());
    let lines = app.event_lines();
    assert!(
        lines.iter().any(|line| line.ends_with("Cruise off.")),
        "{lines:?}"
    );
    app.shutdown();
}

// -- the short forms ------------------------------------------------------------------

#[test]
fn test_work_zone_short_forms() {
    let mut app = an_app("quiet");
    let drive = a_drive(&mut app);
    let trip = &drive.trip;
    assert_eq!(
        trip.zone_entry_terse(&a_zone("construction", Some("right")))
            .as_deref(),
        Some("Work zone. Keep left. Limit 45.")
    );
    assert_eq!(
        trip.zone_entry_terse(&a_zone("construction", None))
            .as_deref(),
        Some("Work zone. Limit 45.")
    );
    assert_eq!(
        trip.zone_entry_terse(&a_zone("construction merge", None))
            .as_deref(),
        Some("Flagger ahead. Limit 45.")
    );
    assert_eq!(trip.zone_entry_terse(&a_zone("heavy traffic", None)), None);
    let warning = trip
        .zone_warning_terse(&a_zone("construction", Some("left")), 2.0)
        .expect("construction has a short warning");
    assert!(warning.starts_with("Construction, "), "{warning}");
    assert!(warning.contains("Merge right."), "{warning}");
    assert!(warning.contains("Limit "), "{warning}");
    app.shutdown();
}

#[test]
fn test_the_weather_gap_line_is_short_at_quiet() {
    let mut app = an_app("quiet");
    let mut drive = a_drive(&mut app);
    drive.weather_mut().current = ff_core::sim::weather::WeatherKind::HeavyRain;
    let line = drive
        .acc_weather_gap_text()
        .expect("heavy rain lengthens the gap");
    assert_eq!(line.render(true), "Wet roads, longer gap.");
    assert_eq!(line.render(false), "Wet roads, longer gap.");
    app.shutdown();
}

// -- achievements at the wheel ---------------------------------------------------------

/// What earning one achievement mid-drive said, chimed, and left in review.
fn achievement_at(mode: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut app = an_app(mode);
    let _drive = a_drive(&mut app);
    let audio = app.record_audio();
    app.clear_speech();
    let review_from = app.ctx.message_log.messages.len();

    app.ctx.award_driving_achievement("rain_driver");

    let mut spoken = app.main_lines();
    spoken.extend(app.event_lines());
    let chimes = audio
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    let review = app.ctx.message_log.messages[review_from..]
        .iter()
        .map(|message| message.text.clone())
        .collect();
    app.shutdown();
    (spoken, chimes, review)
}

#[test]
fn test_urgent_only_keeps_achievements_out_of_the_drive() {
    let (spoken, chimes, review) = achievement_at("urgent_only");
    assert!(spoken.is_empty(), "{spoken:?}");
    assert!(chimes.is_empty(), "{chimes:?}");
    assert!(
        review
            .iter()
            .any(|line| line.starts_with("New achievement!")),
        "{review:?}"
    );
}

#[test]
fn test_quiet_still_names_an_achievement() {
    let (spoken, chimes, _) = achievement_at("quiet");
    assert_eq!(spoken.len(), 1, "{spoken:?}");
    assert!(chimes.contains(&"ui/level_up".to_string()), "{chimes:?}");
}
