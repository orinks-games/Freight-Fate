//! Everyday pool billboards that notice the drive (owner, 2026-09-30): Big
//! Jim answers an incident at the next pool sign, once; the small-hours signs
//! come out when the driver is drowsy; placed attraction signs never change.

use ff_core::data::billboards_dynamic::{
    holiday_signs, Holiday, CITATION_LINES, COLLISION_LINES, HOLIDAY_EVERY, SMALL_HOURS_LINES,
};
use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::{TripEvent, TripEventData, TripEventKind};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::SpokenMessage;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;

const PLAIN: &str = "Billboard: Buffet: if it isn't on a steam table, it isn't dinner.";

/// Buffalo to Rochester at noon in early spring: no holiday, not the small
/// hours, so a pool sign reads as written unless the drive gives it cause.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Signs", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    app.clear_speech();
    drive
}

fn sign(kind: TripEventKind, category: &str, text: &str) -> TripEvent {
    TripEvent {
        kind,
        message: SpokenMessage::new(text),
        data: TripEventData {
            category: Some(category.to_string()),
            ..Default::default()
        },
    }
}

fn pool_sign() -> TripEvent {
    sign(TripEventKind::Billboard, "billboard", PLAIN)
}

/// Read a sign and return what was said for it. Real billboards stand miles
/// apart, so each one finds the ambient cooldown of the last already run out.
fn read(d: &mut DrivingState, app: &mut TestApp, event: &TripEvent) -> String {
    app.clear_speech();
    d.ambient_event_cooldown_s = 0.0;
    let clock = app.fake_pacer_clock();
    clock.advance(60.0);
    d.handle_trip_event(&mut app.ctx, event);
    clock.advance(60.0);
    app.speech().lines().join(" ")
}

fn one_of(said: &str, pool: &[&str]) -> bool {
    pool.iter().any(|line| said.contains(line))
}

#[test]
fn test_big_jim_answers_a_citation_once_at_the_next_pool_sign() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    assert!(read(&mut d, &mut app, &pool_sign()).contains("steam table"));

    app.ctx.profile.as_mut().unwrap().driving_record.citations += 1;
    let said = read(&mut d, &mut app, &pool_sign());
    assert!(one_of(&said, CITATION_LINES), "{said}");
    let said = read(&mut d, &mut app, &pool_sign());
    assert!(said.contains("steam table"), "answered twice: {said}");
}

#[test]
fn test_big_jim_answers_a_collision() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.add_damage(2.0, true);
    let said = read(&mut d, &mut app, &pool_sign());
    assert!(one_of(&said, COLLISION_LINES), "{said}");
}

#[test]
fn test_a_placed_attraction_sign_never_changes() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.profile.as_mut().unwrap().driving_record.citations += 1;
    d.trip.truck.add_damage(2.0, true);
    let placed = sign(
        TripEventKind::Landmark,
        "billboard_sign",
        "Billboard: Wall Drug, next exit. You made it. The ice water is still free.",
    );
    let said = read(&mut d, &mut app, &placed);
    assert!(said.contains("Wall Drug"), "{said}");
    // The incident is still waiting for the next pool sign, the citation
    // first: it is the more serious of the two.
    let said = read(&mut d, &mut app, &pool_sign());
    assert!(one_of(&said, CITATION_LINES), "{said}");
}

#[test]
fn test_the_small_hours_signs_find_a_drowsy_driver_every_other_sign() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.profile.as_mut().unwrap().fatigue = 70.0;
    let said = read(&mut d, &mut app, &pool_sign());
    assert!(one_of(&said, SMALL_HOURS_LINES), "{said}");
    let said = read(&mut d, &mut app, &pool_sign());
    assert!(said.contains("steam table"), "{said}");
}

#[test]
fn test_halloween_week_reads_varied_signs_true_in_new_york() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // March 21 plus 221 days: October 28, Halloween week.
    app.ctx.profile.as_mut().unwrap().calendar_offset_days = 221;
    let mut heard = Vec::new();
    for _ in 0..HOLIDAY_EVERY * 8 {
        let said = read(&mut d, &mut app, &pool_sign());
        if !said.contains("steam table") {
            heard.push(said);
        }
    }
    assert_eq!(heard.len(), 8, "{heard:?}");
    let mut distinct = heard.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        8,
        "a Halloween line came back early: {heard:?}"
    );
    for said in &heard {
        let sign = holiday_signs(Holiday::Halloween)
            .iter()
            .find(|sign| said.contains(sign.text))
            .unwrap_or_else(|| panic!("not a Halloween sign: {said}"));
        assert!(sign.holds_in(Some("NY")), "read in New York: {said}");
    }
}
