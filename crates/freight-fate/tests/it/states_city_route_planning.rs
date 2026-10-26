//! Route planning at the dispatch board: what a route option is CALLED, and
//! what the W-key forecast says when the live weather has only part of the
//! answer (`states/city_pickup.rs`).
//!
//! Ported from `tests/test_driving_features.py`:
//! `test_route_planning_labels_name_through_cities_with_states` and
//! `test_live_route_weather_accounts_for_loading_and_unavailable_cities`.
//!
//! Python replaced `ctx.real_weather_provider` with a three-answer stand-in.
//! The port's context hands back a concrete `RealWeatherProvider`, so the
//! three answers are produced by a real provider instead: a stub fetch that
//! succeeds for one station and fails for another, run inline
//! (`with_threaded(false)`) so nothing is left in flight. The third city is
//! simply never asked, which is the same "no reading, no failure" the screen
//! reads as "still loading" -- and it avoids parking a real worker thread on
//! a fetch that never returns just to hold that state open.

use std::sync::Arc;

use ff_core::data::world::get_world;
use ff_core::models::jobs::{JobBoard, OfferOptions};
use ff_core::models::profile::Profile;
use ff_core::sim::real_weather::{Observation, RealWeatherProvider};

use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::key_event;
use freight_fate::states::base::{Key, Menu, State};
use freight_fate::states::city_pickup::{RouteSelectOptions, RouteSelectState};

// -- rigging -------------------------------------------------------------------------

/// The Python cases' job: a Chicago offer whose fastest route has enough
/// intermediate cities to talk about.
fn a_job_with_vias(app: &TestApp, min_cities: usize) -> ff_core::models::jobs::Job {
    let world = get_world();
    let mut board = JobBoard::new(world, Some(3), None);
    let endorsements: Vec<&str> = Vec::new();
    let offers = board.offers("Chicago", &endorsements, OfferOptions::level(2));
    let _ = app;
    offers
        .into_iter()
        .find(|job| {
            world
                .supported_route_options(&job.origin, &job.destination, 3)
                .map(|routes| {
                    routes
                        .first()
                        .is_some_and(|route| route.cities.len() > min_cities)
                })
                .unwrap_or(false)
        })
        .expect("seed 3 offers a Chicago run with cities along the way")
}

// -- the labels -----------------------------------------------------------------------

#[test]
fn test_route_planning_labels_name_through_cities_with_states() {
    // Route options must say which cities they pass through, state-qualified,
    // in the spoken label itself -- not only in the F1 help (player request:
    // "I have no idea where McCall is, but knowing the state gives me a
    // general idea of, oh, that's the way we're going").
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Route Labels", "Chicago"));
    let world = get_world();
    let job = a_job_with_vias(&app, 2);
    let routes = world
        .supported_route_options(&job.origin, &job.destination, 3)
        .expect("the world routes");
    let destination = job.destination.clone();
    let spoken_destination = job.spoken_destination().to_string();
    let mut state = RouteSelectState::new(
        &mut app.ctx,
        job,
        routes.clone(),
        RouteSelectOptions::default(),
    );
    let items = state.build_items(&mut app.ctx);
    let label = items[0].text(&state, &app.ctx);

    assert!(
        label.contains("through ") || label.contains("passing no major cities"),
        "{label}"
    );
    let first_via = world
        .cities
        .get(&routes[0].cities[1])
        .expect("the first via is a world city");
    assert!(label.contains(&first_via.spoken_qualified()), "{label}");
    // The destination line carries the state too.
    let destination_city = world
        .cities
        .get(&destination)
        .expect("the destination is a world city");
    assert_eq!(destination_city.spoken_qualified(), spoken_destination);
}

// -- the forecast ---------------------------------------------------------------------

#[test]
fn test_live_route_weather_accounts_for_loading_and_unavailable_cities() {
    // A partial live response must not sound like a complete route outlook.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Route Weather Driver", "Chicago"));
    let world = get_world();
    let job = a_job_with_vias(&app, 3);
    let route = world
        .supported_route_options(&job.origin, &job.destination, 3)
        .expect("the world routes")
        .into_iter()
        .next()
        .expect("at least one route");
    let first = route.cities[1].clone();
    let second = route.cities[2].clone();
    let third = route.cities[3].clone();

    // Real weather on, but pointed at a provider that never touches the
    // network: the screen's constructor requests every city on the route, and
    // this one refuses them all.
    app.ctx.settings.real_weather = true;
    app.ctx
        .set_real_weather_provider(Arc::new(offline_provider()));
    let mut state = RouteSelectState::new(
        &mut app.ctx,
        job,
        vec![route],
        RouteSelectOptions::default(),
    );

    // Now the provider the forecast reads: one city answered, one failed, and
    // the third never asked at all.
    let provider = Arc::new(answering_provider(&first));
    let first_city = world.cities.get(&first).expect("a world city");
    let second_city = world.cities.get(&second).expect("a world city");
    provider.request(&first_city.key, first_city.lat, first_city.lon);
    provider.request(&second_city.key, second_city.lat, second_city.lon);
    app.ctx.set_real_weather_provider(Arc::clone(&provider));
    app.clear_speech();

    State::handle_event(&mut state, &mut app.ctx, &key_event(Key::W, None));

    let said = app.main_lines().last().cloned().unwrap_or_default();
    assert!(
        said.contains(&format!(
            "{}: cloudy",
            world.spoken_city(&first, Some(true))
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "{}: no live weather",
            world.spoken_city(&second, Some(true))
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "{}: live weather still loading",
            world.spoken_city(&third, Some(true))
        )),
        "{said}"
    );
}

/// A provider whose every fetch fails, run inline.
fn offline_provider() -> RealWeatherProvider {
    RealWeatherProvider::new(Arc::new(|_lat, _lon| Err("offline bench".to_string())))
        .with_threaded(false)
}

/// A provider that answers `cloudy` for one city's station and fails for
/// every other, run inline.
fn answering_provider(city_key: &str) -> RealWeatherProvider {
    let world = get_world();
    let city = world.cities.get(city_key).expect("a world city");
    let station = RealWeatherProvider::station_identity(city.lat, city.lon);
    RealWeatherProvider::new(Arc::new(move |lat, lon| {
        if RealWeatherProvider::station_identity(lat, lon) == station {
            Ok(Observation::new("Cloudy", 5.0, Some(15.0), Some(10.0)))
        } else {
            Err("offline bench".to_string())
        }
    }))
    .with_threaded(false)
}

// -- construction notes on the options --------------------------------------------------

#[test]
fn test_route_planning_reads_dispatch_construction_notes_on_each_option() {
    // An owner-operator chooses, but hears what dispatch read on 511: the
    // option list is dispatch's order, the reason it moved route 1 up is
    // spoken on entry, and each option carries its own construction note.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Route Notes", "Chicago"));
    let world = get_world();
    let job = a_job_with_vias(&app, 2);
    let routes = world
        .supported_route_options(&job.origin, &job.destination, 3)
        .expect("the world routes");
    let note = "Construction: one lane closed on I-65 near Chicago, 5 miles at 45 miles \
                per hour, about 2 minutes.";
    let mut notes = vec![String::new(); routes.len()];
    notes[0] = note.to_string();
    let dispatch_note = "Route 1 avoids the road closed on I-90 near Gary.";
    let mut state = RouteSelectState::new(
        &mut app.ctx,
        job,
        routes.clone(),
        RouteSelectOptions {
            notes,
            dispatch_note: dispatch_note.to_string(),
            ..RouteSelectOptions::default()
        },
    );
    let items = state.build_items(&mut app.ctx);
    let first = items[0].text(&state, &app.ctx);
    assert!(first.ends_with(note), "{first}");
    if routes.len() > 1 {
        let second = items[1].text(&state, &app.ctx);
        assert!(!second.contains("Construction:"), "{second}");
    }
    app.clear_speech();
    state.announce_entry(&mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(said.contains("route option"), "{said}");
    assert!(said.contains(dispatch_note), "{said}");
}
