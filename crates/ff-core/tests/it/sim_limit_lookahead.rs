//! The co-driver warns before a posted-limit drop and sizes short zones
//! (port of `tests/test_limit_lookahead.py`).
//!
//! Born from the NY-12 playtest (2026-07-19): real village 30s every few
//! miles are honest data, but hitting one blind at 55 in a Class-8 is not
//! playable.

use crate::sim_support::*;
use ff_core::data::world_models::{
    CorridorDetail, GradeSegment, Leg, Route, SpeedLimitSample, StateMileage,
};
use ff_core::sim::trip::{spoken_short_miles, Trip, TripOptions};
use ff_core::sim::trip_models::{RoadStop, TripEventKind};
use ff_core::sim::vehicle::TruckState;

fn sample(at_mi: f64, mph: Option<f64>) -> SpeedLimitSample {
    SpeedLimitSample {
        at_mi,
        mph,
        source: String::new(),
        hgv: false,
    }
}

/// A quiet 100-mile US-highway trip between real cities (the heuristic
/// branch needs real city regions), with a synthetic posted profile.
fn trip_with(
    speed_limits: Vec<SpeedLimitSample>,
    imperial: bool,
    grade_segments: Vec<GradeSegment>,
) -> Trip {
    let leg = Leg::new(
        "aberdeen_sd_us",
        "pierre_sd_us",
        100.0,
        "US-83",
        "flat",
        Vec::new(),
    )
    .with_detail(CorridorDetail {
        state_miles: vec![StateMileage::new("South Dakota", 100.0)],
        speed_limits,
        grade_segments,
        ..Default::default()
    });
    let route = Route::from_legs(
        vec!["aberdeen_sd_us".to_string(), "pierre_sd_us".to_string()],
        vec![leg],
    );
    let mut trip = Trip::new(
        route,
        TruckState::default(),
        weather("upper_midwest", 1),
        TripOptions {
            seed: Some(2),
            world: Some(world()),
            ..Default::default()
        },
    );
    trip.set_imperial(imperial);
    trip.zones = Vec::new();
    trip.curves = Vec::new();
    trip.hazard_check_mi = 1e9;
    trip.inspection_check_mi = 1e9;
    trip
}

fn trip(speed_limits: Vec<SpeedLimitSample>) -> Trip {
    trip_with(speed_limits, true, Vec::new())
}

fn cues(trip: &mut Trip, position_mi: f64, speed_mph: f64) -> Vec<String> {
    trip.position_mi = position_mi;
    trip.truck.velocity_mps = speed_mph * 0.44704;
    messages_of(&trip.update(0.0), TripEventKind::GpsCue)
}

fn village() -> Vec<SpeedLimitSample> {
    vec![
        sample(0.0, Some(65.0)),
        sample(50.0, Some(30.0)),
        sample(50.6, Some(65.0)),
    ]
}

fn has(cues: &[String], needle: &str) -> bool {
    cues.iter().any(|c| c == needle)
}

fn drops(cues: &[String]) -> Vec<&String> {
    cues.iter().filter(|c| c.contains("drops to")).collect()
}

fn reduced(cues: &[String]) -> Vec<String> {
    cues.iter()
        .filter(|c| c.starts_with("Speed limit reduced"))
        .cloned()
        .collect()
}

#[test]
fn test_warns_before_a_big_drop_at_speed() {
    let mut trip = trip(village());
    let cues = cues(&mut trip, 49.8, 55.0);
    assert!(
        has(&cues, "Speed limit drops to 30 in a quarter mile."),
        "{cues:?}"
    );
}

#[test]
fn test_warning_fires_once() {
    let mut trip = trip(village());
    cues(&mut trip, 49.8, 55.0);
    assert!(drops(&cues(&mut trip, 49.85, 55.0)).is_empty());
}

#[test]
fn test_no_warning_when_already_slow() {
    let mut trip = trip(village());
    assert!(drops(&cues(&mut trip, 49.8, 30.0)).is_empty());
}

#[test]
fn test_no_warning_beyond_the_scaled_lookahead_window() {
    // Beyond LIMIT_WARNING_MAX_LEAD_MI the boundary isn't even scanned for
    // yet, whatever the current pace.
    let mut trip = trip(village());
    assert!(drops(&cues(&mut trip, 44.5, 55.0)).is_empty());
}

#[test]
fn test_advance_warning_lead_scales_with_time_compression() {
    // The lead is sized in real seconds at the current pace, not a fixed
    // game-mile distance (owner's live playtest, 2026-08-12).
    let mut trip = trip(village());
    trip.time_scale = 40.0;
    let speed = 55.0;
    trip.truck.velocity_mps = speed * 0.44704;
    let scale = trip.effective_time_scale();
    assert_eq!(scale, 40.0); // full pacing already resumed at this speed

    let old_lead_mi = Trip::curve_pacenote_lead_mi(speed, 30.0);
    let old_real_s = old_lead_mi * 3600.0 / (speed * scale);
    assert!(old_real_s < 1.0); // the bug: barely a real second to react

    let new_lead_mi = trip.limit_drop_warning_lead_mi(speed);
    let new_real_s = new_lead_mi * 3600.0 / (speed * scale);
    assert!(new_real_s > old_real_s * 5.0);

    // And the cue actually fires that far out.
    let position = 50.0 - new_lead_mi + 0.05;
    let cues = cues(&mut trip, position, speed);
    assert!(cues
        .iter()
        .any(|c| c.starts_with("Speed limit drops to 30 in")));
}

#[test]
fn test_no_warning_for_a_small_step() {
    let mut trip = trip(vec![sample(0.0, Some(65.0)), sample(50.0, Some(60.0))]);
    assert!(drops(&cues(&mut trip, 49.8, 65.0)).is_empty());
}

#[test]
fn test_short_zone_length_spoken_on_entry() {
    let mut trip = trip(village());
    // Seeded well outside LIMIT_WARNING_MAX_LEAD_MI so this update only seeds
    // the announced limit at 65 and does not itself pre-announce the drop.
    cues(&mut trip, 40.0, 55.0);
    let cues = cues(&mut trip, 50.05, 30.0);
    assert!(
        has(&cues, "Speed limit reduced to 30 for half a mile."),
        "{cues:?}"
    );
}

#[test]
fn test_long_zone_entry_stays_unsized() {
    let mut trip = trip(vec![sample(0.0, Some(65.0)), sample(50.0, Some(30.0))]);
    cues(&mut trip, 40.0, 55.0); // seed the announced limit at 65
    let cues = cues(&mut trip, 50.05, 30.0);
    assert_eq!(reduced(&cues), vec!["Speed limit reduced to 30."]);
}

#[test]
fn test_unannounced_drop_still_speaks_reduced_to() {
    // A posting the advance pacenote never got a chance to warn about still
    // gets its plain arrival confirmation.
    let mut trip = trip(village());
    let seed_cues = cues(&mut trip, 40.0, 55.0);
    assert!(drops(&seed_cues).is_empty());
    let cues = cues(&mut trip, 50.05, 30.0);
    assert!(
        has(&cues, "Speed limit reduced to 30 for half a mile."),
        "{cues:?}"
    );
}

#[test]
fn test_preannounced_drop_does_not_repeat_reduced_to() {
    // The complaint (owner, live playtest, 2026-08-12): the same number
    // twice. Once the advance pacenote has named a posting, the plain
    // arrival line for that same number stays quiet.
    let mut trip = trip(village());
    let warn_cues = cues(&mut trip, 49.8, 55.0);
    assert!(has(
        &warn_cues,
        "Speed limit drops to 30 in a quarter mile."
    ));
    let arrival_cues = cues(&mut trip, 50.05, 55.0);
    assert!(reduced(&arrival_cues).is_empty());
}

#[test]
fn test_raise_is_never_suppressed_by_a_preannounced_drop() {
    let mut trip = trip(village());
    cues(&mut trip, 49.8, 55.0); // advance-warns and pre-announces the 30 drop
    cues(&mut trip, 50.05, 55.0); // arrival, suppressed (see test above)
    let cues = cues(&mut trip, 50.65, 55.0);
    assert!(has(&cues, "Speed limit raised to 65."), "{cues:?}");
}

#[test]
fn test_gap_marker_ends_a_village_zone() {
    // The NY-12 shape: a village 30 whose OSM tagging ends 0.6 miles in.
    // Inside the gap the heuristic answers (US highway 65), not the stale 30.
    let trip = trip(vec![sample(40.0, Some(30.0)), sample(40.6, None)]);
    assert_eq!(trip.corridor_limit_at(40.3), 30.0);
    assert_eq!(trip.corridor_limit_at(45.0), 65.0);
}

#[test]
fn test_lowered_limit_names_a_weigh_station_ahead() {
    // No city nearby, but a weigh station sits just past the drop.
    let mut trip = trip(vec![sample(0.0, Some(65.0)), sample(50.0, Some(30.0))]);
    trip.stops = vec![RoadStop::new("SD Scale House", 50.5, "weigh_station")];
    cues(&mut trip, 40.0, 55.0);
    let cues = cues(&mut trip, 50.05, 30.0);
    assert_eq!(
        reduced(&cues),
        vec!["Speed limit reduced to 30 for the weigh station ahead."]
    );
}

#[test]
fn test_lowered_limit_ignores_a_stop_already_passed() {
    let mut trip = trip(vec![sample(0.0, Some(65.0)), sample(50.0, Some(30.0))]);
    trip.stops = vec![RoadStop::new("SD Scale House", 49.0, "weigh_station")];
    cues(&mut trip, 40.0, 55.0);
    let cues = cues(&mut trip, 50.05, 30.0);
    assert_eq!(reduced(&cues), vec!["Speed limit reduced to 30."]);
}

#[test]
fn test_lowered_limit_names_a_downgrade_ahead() {
    // No city and no stop, but a sustained downgrade starts right where the
    // lower posting begins.
    let mut trip = trip_with(
        vec![sample(0.0, Some(65.0)), sample(50.0, Some(30.0))],
        true,
        vec![GradeSegment::new(50.0, 55.0, -4.0, "hills", "")],
    );
    cues(&mut trip, 40.0, 55.0);
    let cues = cues(&mut trip, 50.05, 30.0);
    assert_eq!(
        reduced(&cues),
        vec!["Speed limit reduced to 30 for the downhill grade."]
    );
}

#[test]
#[ignore = "Python monkeypatched Trip._nearest_urban_city; the city reason is covered by the arrival-line tests"]
fn test_lowered_limit_city_reason_beats_a_stop_ahead() {}

#[test]
fn test_raised_limit_never_gets_a_reason() {
    let mut trip = trip(village());
    trip.stops = vec![RoadStop::new("SD Scale House", 50.7, "weigh_station")];
    cues(&mut trip, 49.8, 55.0);
    cues(&mut trip, 50.05, 55.0);
    let cues = cues(&mut trip, 50.65, 55.0);
    assert!(has(&cues, "Speed limit raised to 65."), "{cues:?}");
}

#[test]
fn test_spoken_short_miles_units() {
    assert_eq!(spoken_short_miles(0.2, true), "a quarter mile");
    assert_eq!(spoken_short_miles(0.5, true), "half a mile");
    assert_eq!(spoken_short_miles(0.5, false), "800 meters");
}

#[test]
fn test_one_posting_warns_once_however_the_frames_land() {
    // A headless playtest on Indianapolis->Nashville said "speed limit drops
    // to 55 in 3 miles" twice in a row. There is only ONE drop to 55 on
    // that route, at mile 284.9 (the 2026-07-23 double-warning surviving its
    // own fix).
    let route = route_from_cities(world(), &["Indianapolis", "Nashville"]);
    let mut trip = Trip::new(
        route,
        TruckState::default(),
        weather("midwest", 1),
        TripOptions {
            seed: Some(3),
            world: Some(world()),
            ..Default::default()
        },
    );
    trip.truck.velocity_mps = 65.0 / 2.23694;

    let mut warned: Vec<f64> = Vec::new();
    trip.position_mi = 270.0;
    while trip.position_mi < 286.5 {
        trip.position_mi += 0.02;
        let before = trip.warned_limit_drops.clone();
        trip.check_limit_drop_ahead();
        for key in &trip.warned_limit_drops {
            if !before.contains(key) {
                warned.push(*key);
            }
        }
    }
    assert!(!warned.is_empty(), "the drop to 55 never warned at all");
    let mut unique = warned.clone();
    unique.dedup();
    assert_eq!(
        warned.len(),
        unique.len(),
        "one posting keyed more than once: {warned:?}"
    );
    assert_eq!(
        warned.len(),
        1,
        "one posting warned {} times: {warned:?}",
        warned.len()
    );
}

#[test]
fn test_a_posting_gone_before_its_line_ends_is_not_spoken_or_charged() {
    // At 20x a fifth of a mile goes by in about a second, and "reduced to
    // 55" for it ended with the truck back at 65 (player report, 2026-10-17).
    let mut trip = trip(vec![
        sample(0.0, Some(65.0)),
        sample(50.0, Some(57.0)),
        sample(50.2, Some(65.0)),
    ]);
    cues(&mut trip, 40.0, 60.0);
    let arrival = cues(&mut trip, 50.02, 60.0);
    assert!(
        !arrival.iter().any(|c| c.starts_with("Speed limit")),
        "{arrival:?}"
    );
    assert!(trip.in_unannounced_posting());
    let after = cues(&mut trip, 50.3, 60.0);
    assert!(
        !after.iter().any(|c| c.starts_with("Speed limit")),
        "back at the limit the driver last heard: {after:?}"
    );
    assert!(!trip.in_unannounced_posting());
}

#[test]
fn test_a_posting_that_holds_long_enough_is_still_spoken() {
    let mut trip = trip(vec![sample(0.0, Some(65.0)), sample(50.0, Some(57.0))]);
    cues(&mut trip, 40.0, 60.0);
    let arrival = cues(&mut trip, 50.02, 60.0);
    assert!(has(&arrival, "Speed limit reduced to 57."), "{arrival:?}");
    assert!(!trip.in_unannounced_posting());
}
