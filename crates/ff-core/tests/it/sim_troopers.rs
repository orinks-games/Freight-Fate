//! Trooper pull-overs: enforcement posts and the CB heads-up -- the pure half
//! of `tests/test_troopers.py`.
//!
//! The interactive roadside stop, the tickets, the warnings and the evasion
//! drive the app shell, so they live in
//! `crates/freight-fate/tests/states_driving_troopers.rs` and
//! `states_driving_enforcement.rs`; see the note at the foot of this file.

use crate::sim_support::*;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::TripEventKind;
use ff_core::sim::vehicle::TruckState;

fn trip_with(seed: i64, hazard_scale: f64, start_hour: f64) -> Trip {
    let route = first_route_option(world(), "Salt Lake City", "Las Vegas");
    Trip::new(
        route,
        TruckState::default(),
        weather("great_basin", 1),
        TripOptions {
            seed: Some(seed),
            hazard_scale,
            start_hour,
            world: Some(world()),
            ..Default::default()
        },
    )
}

fn trip() -> Trip {
    trip_with(7, 1.0, 12.0)
}

// --- post model -------------------------------------------------------------

fn post_key(t: &Trip) -> Vec<(f64, String, bool)> {
    t.posts
        .iter()
        .map(|p| ((p.at_mi * 10.0).round() / 10.0, p.kind.clone(), p.staffed))
        .collect()
}

#[test]
fn test_post_seeding_is_deterministic() {
    assert_eq!(post_key(&trip()), post_key(&trip()));
}

#[test]
fn test_roving_posts_create_state_trooper_npcs() {
    let t = trip();
    let troopers: Vec<_> = t
        .traffic_manager
        .vehicles
        .iter()
        .filter(|v| v.vehicle_class == "state trooper")
        .collect();
    let roving = t
        .posts
        .iter()
        .filter(|p| p.kind == "roving_patrol" && p.staffed)
        .count();
    assert!(!t.posts.is_empty());
    assert_eq!(troopers.len(), roving);
    assert!(troopers.iter().all(|v| v.reason() == "state trooper ahead"));
}

#[test]
fn test_relaxed_hazards_do_not_thin_the_police() {
    // Presence is a fact about the country, not a difficulty knob.
    assert_eq!(trip_with(7, 0.3, 12.0).posts.len(), trip().posts.len());
}

#[test]
fn test_construction_zones_always_carry_a_work_zone_post() {
    let t = trip();
    if let Some(zone) = t.zones.iter().find(|z| z.reason == "construction") {
        let covering = t
            .posts
            .iter()
            .filter(|p| p.kind == "work_zone_post" && (p.at_mi - zone.start_mi).abs() < 1.0)
            .count();
        assert!(covering > 0);
    }
}

#[test]
fn test_active_post_returns_the_most_attentive_watcher() {
    let mut t = trip();
    let quiet = always_observing_post(50.0, "urban_unit", 1.0, 0.3, 0);
    let loud = always_observing_post(50.0, "work_zone_post", 1.0, 0.9, 0);
    t.posts = vec![quiet, loud.clone()];
    assert_eq!(t.active_post_at(50.0).map(|p| p.id()), Some(loud.id()));
    assert!(t.active_post_at(2000.0).is_none());
}

#[test]
fn test_cb_radio_warns_before_an_upcoming_post() {
    let mut t = trip();
    t.posts = vec![always_observing_post(14.0, "median_post", 4.0, 1.0, 0)];
    let post_id = t.posts[0].id();
    t.position_mi = 10.0 - 0.1;
    t.truck.velocity_mps = 1.0;

    let events = t.update(0.1);

    let cb_events: Vec<_> = events
        .iter()
        .filter(|e| e.data.cb_patrol.as_ref().is_some_and(|p| p.id() == post_id))
        .collect();
    assert!(!cb_events.is_empty());
    assert_eq!(cb_events[0].kind, TripEventKind::GpsCue);
    assert!(!cb_events[0].text().contains("CB chatter"));
    assert!(cb_events[0].text().contains("trooper"));
}

#[test]
fn test_cb_radio_post_warning_only_fires_once() {
    let mut t = trip();
    t.posts = vec![always_observing_post(14.0, "median_post", 4.0, 1.0, 0)];
    let post_id = t.posts[0].id();
    t.position_mi = 6.0;
    t.truck.velocity_mps = 1.0;

    let first = t.update(0.1);
    let second = t.update(0.1);

    let is_ours = |e: &ff_core::sim::trip_models::TripEvent| {
        e.data.cb_patrol.as_ref().is_some_and(|p| p.id() == post_id)
    };
    assert_eq!(first.iter().filter(|e| is_ours(e)).count(), 1);
    assert!(!second.iter().any(is_ours));
}

// --- driving-side: the pull-over itself (app shell) -------------------------
//
// Every remaining case in `tests/test_troopers.py` drives a real
// `DrivingState` and the screens it pushes, which `ff-core` cannot see at all.
// They ran here as `#[ignore]`d stubs and now run for real:
//
// - the cue ladder and who gets pulled over --
//   `crates/freight-fate/tests/states_driving_enforcement.rs`
//   (`test_speeding_past_a_staffed_post_starts_a_pull_over`,
//   `test_speeding_with_no_post_watching_costs_nothing`,
//   `test_debug_off_mode_never_pulls_you_over`).
// - the stop, the ticket, the scale bypass, the unsafe-equipment stop, the
//   construction-zone doubling, the compliance tracker and running from a
//   stop -- `crates/freight-fate/tests/states_driving_troopers.rs`.

// -- the routine inspection roll ----------------------------------------------------------

fn routine_inspections_over(miles: f64, scale: f64) -> usize {
    let mut t = trip();
    t.roadside_inspection_scale = scale;
    t.events.clear();
    let mut driven = 0.0;
    while driven < miles {
        t.position_mi += 5.0;
        driven += 5.0;
        t.check_inspections(5.0);
    }
    t.events
        .iter()
        .filter(|e| e.data.context.as_deref() == Some("routine_inspection"))
        .count()
}

#[test]
fn test_a_legal_driver_meets_the_occasional_routine_inspection() {
    // Zero scale is off; a targeted record over a long run meets several.
    assert_eq!(routine_inspections_over(2_000.0, 0.0), 0);
    let targeted = routine_inspections_over(2_000.0, 4.0);
    assert!(
        targeted >= 1,
        "{targeted} routine inspections in 2,000 miles at 4x"
    );
    let clean = routine_inspections_over(2_000.0, 1.0);
    assert!(clean <= targeted, "clean {clean} vs targeted {targeted}");
}

#[test]
fn test_roadcheck_week_is_announced_once_on_the_cb() {
    let mut t = trip();
    t.roadcheck_blitz = true;
    t.roadside_inspection_scale = 0.0;
    t.events.clear();
    for _ in 0..40 {
        t.position_mi += 5.0;
        t.check_inspections(5.0);
    }
    let notices: Vec<_> = t
        .events
        .iter()
        .filter(|e| e.data.context.as_deref() == Some("roadcheck_notice"))
        .collect();
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].text().contains("Roadcheck"),
        "{}",
        notices[0].text()
    );
}
