//! Two weigh stations close together: each gets its own notice and its own
//! reminder, every line names the scale it is about, and the real West
//! Memphis scales on the roads through it.
//!
//! A tester on that run (log, 2026-10-07) heard the open I-40 scale
//! announced twelve miles out, then checked in at an "I-55 Weigh Station"
//! on the approach to Memphis that was closed -- and turned out not to
//! exist on that side of the road at all.

use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::sim::enforcement_posts::{EnforcementPost, KIND_FIXED_SCALE, METHOD_SCALE_SCREEN};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::breaker::{Rig, RigOptions, DT};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_enforcement::SCALE_REMINDER_REAL_LEAD_S;

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn mph_to_mps(mph: f64) -> f64 {
    mph * MPS_PER_MPH
}

fn a_scale(name: &str, at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new(name, at_mi, "weigh_station");
    stop.actions = vec!["inspect".to_string()];
    stop.parking = "none".to_string();
    stop
}

fn open_post(stop: &RoadStop) -> EnforcementPost {
    let mut post = EnforcementPost::new(stop.at_mi, KIND_FIXED_SCALE);
    post.method = METHOD_SCALE_SCREEN.to_string();
    post.reach_mi = 1.0;
    post.staffed = true;
    post.anchor = stop.key();
    post
}

fn ten_times_rig() -> Rig {
    let mut rig = Rig::new(RigOptions {
        seed: 1,
        ..RigOptions::default()
    });
    rig.app.ctx.settings.time_scale = 10.0;
    // The driver signals for the scale here, so lane keeping is partial:
    // on full it signals for an open scale itself.
    rig.app.ctx.settings.lane_keeping = "partial".to_string();
    rig.drive.trip.time_scale = 10.0;
    rig
}

fn notices_for(rig: &Rig, name: &str) -> usize {
    rig.transcript()
        .iter()
        .filter(|line| line.contains("Open weigh station ahead") && line.contains(name))
        .count()
}

/// Drive at a pinned 61 mph, ignoring every instruction, until `past_mi`.
///
/// Returns the real seconds from the first line containing `reminder` to
/// the frame the truck reached `scale_mi`, if the reminder spoke before it.
fn drive_past(rig: &mut Rig, scale_mi: f64, past_mi: f64, reminder: &str) -> Option<f64> {
    let mut reminded_at: Option<usize> = None;
    let mut lead = None;
    for frame in 0..20_000 {
        rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);
        let before = rig.drive.trip.position_mi;
        rig.step(1, DT, None);
        if reminded_at.is_none() && rig.said(reminder) > 0 {
            reminded_at = Some(frame);
        }
        if before < scale_mi && rig.drive.trip.position_mi >= scale_mi {
            lead = reminded_at.map(|at| (frame - at) as f64 * DT);
        }
        if rig.drive.trip.position_mi > past_mi || rig.drive.pull_over.is_some() {
            break;
        }
    }
    lead
}

#[test]
fn two_open_scales_inside_the_lookahead_each_get_one_notice_and_their_own_reminder() {
    // The West Memphis spacing: 4.8 miles apart, both inside the twelve-mile
    // lookahead at ten times. One held notice key used to flip between them
    // every frame -- each re-announced, and the nearer one's reminder never
    // fired because its key was never the one held.
    let mut rig = ten_times_rig();
    let near = a_scale("I-55 Weigh Station", 6.0);
    let far = a_scale("I-40 Weigh Station", 10.8);
    rig.drive.trip.stops = vec![near.clone(), far.clone()];
    rig.drive.trip.posts = vec![open_post(&near), open_post(&far)];
    rig.prepare(61.0, None);
    rig.drive.trip.position_mi = 0.5;
    rig.drive.enforcement_prev_mi = 0.5;

    let lead = drive_past(
        &mut rig,
        near.at_mi,
        near.at_mi + 0.2,
        "I-55 Weigh Station in half a mile. Signal for the scale exit.",
    );

    assert_eq!(
        notices_for(&rig, "I-55 Weigh Station"),
        1,
        "{:?}",
        rig.transcript()
    );
    assert_eq!(
        notices_for(&rig, "I-40 Weigh Station"),
        1,
        "{:?}",
        rig.transcript()
    );
    let lead =
        lead.unwrap_or_else(|| panic!("no reminder for the near scale: {:?}", rig.transcript()));
    assert!(lead >= SCALE_REMINDER_REAL_LEAD_S, "{lead} real s");
    let near_key = rig.drive.weigh_station_key(&near);
    assert!(rig.drive.enforcement_events.contains(&near_key));
    // The far scale is still announced and still to come.
    assert_eq!(
        rig.drive
            .announced_open_scale_ahead()
            .map(|(stop, _)| stop.name),
        Some("I-40 Weigh Station".to_string())
    );
}

#[test]
fn the_red_light_names_its_scale() {
    let mut rig = ten_times_rig();
    let scale = a_scale("I-40 Weigh Station", 10.8);
    rig.drive.trip.stops = vec![scale.clone()];
    rig.drive.trip.position_mi = 5.0;
    rig.app.clear_speech();

    rig.drive
        .speak_transponder_verdict(&mut rig.app.ctx, &scale, "red");

    assert_eq!(
        rig.said("Red light. Pull in to I-40 Weigh Station."),
        1,
        "{:?}",
        rig.transcript()
    );
}

// -- the real road ------------------------------------------------------------------

const ST_LOUIS_TO_MEMPHIS_MI: f64 = 285.0;

/// The rig, re-seated on the real St. Louis - Memphis - Little Rock route.
fn st_louis_memphis_little_rock() -> Rig {
    let mut rig = ten_times_rig();
    let route = rig
        .app
        .ctx
        .world
        .route_from_cities(&["st_louis_mo_us", "memphis_tn_us", "little_rock_ar_us"])
        .expect("St. Louis to Memphis to Little Rock is in the world");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "St. Louis",
        "company yard",
        "Little Rock",
        route.miles(),
        1000.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut rig.app.ctx,
        job,
        route,
        Some(1),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    // The rig's own settings: highway only, nothing rolled on the way.
    drive.tutorial = None;
    drive.departure_checked = true;
    drive.trip.hazard_check_mi = 1e18;
    drive.trip.inspection_check_mi = 1e18;
    drive.trip.conditions_check_mi = 1e18;
    drive.trip.traffic_manager.vehicles.clear();
    drive.trip.traffic_pressures.clear();
    drive.trip.set_patrols(Vec::new());
    drive.weather_mut().forced = Some(WeatherKind::Clear);
    drive.weather_mut().current = WeatherKind::Clear;
    drive.trip.time_scale = 10.0;
    *rig.drive = drive;
    rig
}

fn scales_near_memphis(rig: &Rig) -> Vec<(String, f64)> {
    let mut scales: Vec<(String, f64)> = rig
        .drive
        .trip
        .stops
        .iter()
        .filter(|stop| stop.stop_type == "weigh_station")
        .filter(|stop| (stop.at_mi - ST_LOUIS_TO_MEMPHIS_MI).abs() < 15.0)
        .map(|stop| (stop.name.clone(), stop.at_mi))
        .collect();
    scales.sort_by(|a, b| a.1.total_cmp(&b.1));
    scales
}

#[test]
fn st_louis_to_little_rock_through_memphis_has_one_scale_at_west_memphis() {
    // Both West Memphis records were one OSM junction on the I-40 bridge
    // approach, imported onto both legs out of Memphis with directions
    // "both": the St. Louis leg's copy put an "I-55 Weigh Station" on the
    // eastbound approach into Memphis, where Arkansas has none. Riverside is
    // westbound only, the first ramp past the Hernando de Soto Bridge.
    // Southbound I-55 into West Memphis has its own real one, Marion, at
    // mile 9 -- eleven miles short of Memphis, and named for its station.
    let rig = st_louis_memphis_little_rock();
    let scales = scales_near_memphis(&rig);
    assert_eq!(scales.len(), 2, "{scales:?}");
    let (marion, marion_mi) = &scales[0];
    assert_eq!(marion, "Marion Weigh Station");
    assert!(
        (marion_mi - (ST_LOUIS_TO_MEMPHIS_MI - 11.3)).abs() < 0.05,
        "Marion is 11.3 miles short of Memphis, southbound: {marion_mi}"
    );
    // Riverside's ramp leaves westbound I-40 at OSM junction node
    // 116871691, 3.15 miles out on the leg geometry. The old 2.4 put it on
    // the Hernando de Soto Bridge itself.
    let (name, at_mi) = &scales[1];
    assert_eq!(name, "I-40 Weigh Station");
    assert!(
        (at_mi - (ST_LOUIS_TO_MEMPHIS_MI + 3.2)).abs() < 0.05,
        "the scale is 3.2 miles past Memphis, westbound: {at_mi}"
    );
}

/// A drive on the real road between `cities`, as the rig sets one up.
fn real_route(cities: &[&str]) -> Rig {
    let mut rig = ten_times_rig();
    let route = rig
        .app
        .ctx
        .world
        .route_from_cities(cities)
        .expect("the route is in the world");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Origin",
        "company yard",
        "Destination",
        route.miles(),
        1000.0,
        12.0,
    );
    let drive = DrivingState::new(
        &mut rig.app.ctx,
        job,
        route,
        Some(1),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    *rig.drive = drive;
    rig
}

fn weigh_stations(rig: &Rig) -> Vec<(String, f64)> {
    rig.drive
        .trip
        .stops
        .iter()
        .filter(|stop| stop.stop_type == "weigh_station")
        .map(|stop| (stop.name.clone(), stop.at_mi))
        .collect()
}

#[test]
fn the_arkansas_scales_by_west_memphis_each_screen_one_direction() {
    // Arkansas Highway Police: Lehi screens eastbound I-40 at mile 274,
    // Marion southbound I-55 at mile 9, Riverside westbound I-40 at 283.
    {
        let little_rock_to_memphis = real_route(&["little_rock_ar_us", "memphis_tn_us"]);
        let scales = weigh_stations(&little_rock_to_memphis);
        let lehi: Vec<&(String, f64)> = scales
            .iter()
            .filter(|(name, _)| name == "Lehi Weigh Station")
            .collect();
        assert_eq!(lehi.len(), 1, "{scales:?}");
        let to_memphis = little_rock_to_memphis.drive.trip.total_miles() - lehi[0].1;
        assert!(
            (to_memphis - 13.4).abs() < 0.05,
            "Lehi is 13.4 miles short of Memphis, eastbound: {to_memphis}"
        );
        assert!(
            !scales.iter().any(|(name, _)| name == "I-40 Weigh Station"),
            "Riverside is westbound only: {scales:?}"
        );
    }
    {
        let memphis_to_little_rock = real_route(&["memphis_tn_us", "little_rock_ar_us"]);
        let scales = weigh_stations(&memphis_to_little_rock);
        assert!(
            !scales.iter().any(|(name, _)| name == "Lehi Weigh Station"),
            "Lehi is eastbound only: {scales:?}"
        );
    }
    {
        let memphis_to_st_louis = real_route(&["memphis_tn_us", "st_louis_mo_us"]);
        let scales = weigh_stations(&memphis_to_st_louis);
        assert!(
            !scales
                .iter()
                .any(|(name, _)| name == "Marion Weigh Station"),
            "Marion is southbound only: {scales:?}"
        );
    }
}

#[test]
fn st_louis_to_memphis_crosses_into_tennessee_on_i_40() {
    // The last miles into Memphis are I-40 over the Hernando de Soto
    // Bridge; I-55 has already left at the West Memphis junction. The state
    // line cue said "Tennessee-Arkansas line on I-55" (QA, 2026-10-07).
    let rig = real_route(&["st_louis_mo_us", "memphis_tn_us"]);
    let total = rig.drive.trip.total_miles();
    let into_tennessee: Vec<String> = rig
        .drive
        .trip
        .navigation_cues
        .iter()
        .filter(|cue| cue.kind == "state_crossing" && cue.at_mi > total - 5.0)
        .map(|cue| cue.near_text.clone())
        .collect();
    assert_eq!(
        into_tennessee,
        vec!["Crossing into Tennessee near Tennessee-Arkansas line on I-40.".to_string()]
    );
}

#[test]
fn st_louis_to_little_rock_at_ten_times_names_the_west_memphis_scale_and_gives_real_seconds() {
    let mut rig = st_louis_memphis_little_rock();
    let scale = rig
        .drive
        .trip
        .stops
        .iter()
        .find(|stop| stop.stop_type == "weigh_station" && stop.at_mi > 280.0)
        .cloned()
        .expect("the West Memphis scale");
    // Open today, so it speaks; nothing else on the road is posted.
    rig.drive.trip.posts = vec![open_post(&scale)];
    rig.prepare(61.0, None);
    let start = scale.at_mi - 12.4;
    rig.drive.trip.position_mi = start;
    rig.drive.enforcement_prev_mi = start;
    rig.app.clear_speech();

    let reminder = "I-40 Weigh Station in half a mile. Signal for the scale exit.";
    let lead = drive_past(&mut rig, scale.at_mi, scale.at_mi + 0.2, reminder);

    assert_eq!(rig.said("I-55 Weigh Station"), 0, "{:?}", rig.transcript());
    assert_eq!(
        notices_for(&rig, "I-40 Weigh Station"),
        1,
        "{:?}",
        rig.transcript()
    );
    assert_eq!(rig.said("Open weigh station ahead"), 1);
    assert_eq!(rig.said(reminder), 1, "{:?}", rig.transcript());
    let lead = lead.unwrap_or_else(|| panic!("no reminder: {:?}", rig.transcript()));
    assert!(lead >= SCALE_REMINDER_REAL_LEAD_S, "{lead} real s");
    assert!(rig
        .drive
        .enforcement_events
        .contains(&rig.drive.weigh_station_key(&scale)));
}
