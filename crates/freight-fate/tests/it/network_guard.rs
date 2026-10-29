//! Nothing in a test process may put a byte on the real network.
//!
//! Two live seams met here. The online services -- presence, cloud saves,
//! both journal outboxes -- are built by `App::new_headless` with
//! `..Default::default()`, which is the LIVE transport on a background
//! thread; the only thing keeping the suite's heartbeats off orinks.net was
//! that a pinned data directory yields no driver identity, and a test that
//! adopted one would have posted as the owner. The live-data feeds had no
//! seam at all: a drive with real weather on builds its own NWS provider and
//! asks for every city on the route, which the suite was measured doing
//! eleven times a run against api.weather.gov.
//!
//! Read `freight_fate::net` for the mechanism. In one line: the network is a
//! capability `main()` grants, a test binary has no `main()` of the game's,
//! and so every send in a test refuses, records the address, and panics
//! naming it.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use ff_core::sim::real_traffic::HttpTransport;
use ff_core::sim::real_weather::RealWeatherProvider;

use freight_fate::app::testing::TestApp;
use freight_fate::discord_presence::PresenceState;
use freight_fate::net::{self, Tier, UreqTransport};
use freight_fate::online_presence::{self, OnlineIdentity, OnlinePresence, OnlinePresenceOptions};

/// An address nothing is listening on, so that even a build with the guard
/// taken out could only ever reach a closed port on this machine. Port 1 is
/// reserved and never bound.
fn nowhere(tag: &str) -> String {
    format!("http://127.0.0.1:1/guard/{tag}")
}

fn was_refused(needle: &str) -> bool {
    net::refused_requests()
        .iter()
        .any(|seen| seen.contains(needle))
}

/// The capability itself: a test process is never the game.
///
/// If this ever passes with `true`, everything below it is decoration --
/// something has handed the suite the live network.
#[test]
fn test_a_test_process_is_never_granted_the_real_network() {
    assert!(!net::real_network_allowed());
}

#[test]
fn test_an_unseamed_request_is_refused_and_recorded_instead_of_sent() {
    let url = nowhere("plain");
    let outcome = catch_unwind(|| net::request(Tier::Orinks, None, &url, None, &[]));
    assert!(outcome.is_err(), "the request should have been refused");
    assert!(was_refused(&url), "{:?}", net::refused_requests());
}

/// The refusal names the method as well as the address, because a POST going
/// somewhere it should not is a different problem from a GET.
#[test]
fn test_the_refusal_names_the_method_and_the_address() {
    let url = nowhere("method");
    let body = b"{}";
    let outcome = catch_unwind(|| net::request(Tier::Orinks, None, &url, Some(body), &[]));
    assert!(outcome.is_err());
    assert!(
        net::refused_requests()
            .iter()
            .any(|seen| *seen == format!("POST {url}")),
        "{:?}",
        net::refused_requests()
    );
}

/// The case test discipline cannot catch.
///
/// Every caller here runs on a worker the test did not spawn -- presence, the
/// outboxes, cloud saves, `weather-<city>`. A per-thread seam is invisible to
/// all of them, and no amount of remembering to install one fixes that. The
/// capability is process-wide, so the worker refuses exactly as the game loop
/// would.
#[test]
fn test_a_spawned_thread_cannot_escape_the_seam() {
    let url = nowhere("spawned");
    let spawned_url = url.clone();
    let worker = std::thread::Builder::new()
        .name("network-guard-probe".to_string())
        .spawn(move || net::request(Tier::Feeds, None, &spawned_url, None, &[]))
        .expect("the probe thread starts");
    let outcome = worker.join();

    assert!(
        outcome.is_err(),
        "a spawned thread reached the network: {outcome:?}"
    );
    assert!(was_refused(&url), "{:?}", net::refused_requests());
}

/// The live-data transport the drive builds for weather, traffic and parking
/// goes through the same door, rather than round the back of it.
#[test]
fn test_the_live_feed_transport_is_covered() {
    let url = nowhere("feeds");
    let transport = UreqTransport;
    let outcome = catch_unwind(|| transport.get(&url, &[], 8.0));
    assert!(outcome.is_err(), "a live feed reached the network");
    assert!(was_refused(&url), "{:?}", net::refused_requests());
}

/// The defect as it was measured: a drive with real weather on builds an NWS
/// provider of its own and asks for the cities on its route, on `weather-city`
/// worker threads, before a test has any chance to swap in a fake. This is
/// that exact object, with the exact transport the drive hands it.
#[test]
fn test_the_drives_own_weather_provider_never_reaches_the_weather_service() {
    let provider = RealWeatherProvider::with_nws(Arc::new(UreqTransport));
    provider.request("Chicago", 41.8781, -87.6298);
    // The worker panicked rather than fetching; joining it is how a panic on
    // a thread nobody is watching still gets waited for.
    provider.join_background();

    assert!(
        was_refused("api.weather.gov"),
        "{:?}",
        net::refused_requests()
    );
}

/// Seam two, in the shape it would have happened: the player links an account
/// and turns the drivers board on, and the service that posts is the one
/// `App::new_headless` built with `..Default::default()` -- the live transport
/// on a background thread, which no `install_transport` guard ever covered.
#[test]
fn test_the_app_presence_service_cannot_post_to_the_live_site() {
    let mut app = TestApp::new();
    app.ctx.adopt_online_identity(Some(OnlineIdentity::new(
        "guard-driver",
        "guard-token-never-valid",
    )));
    app.ctx.services.online.set_enabled(true);
    assert!(
        app.ctx.services.online.enabled(),
        "the board is on, which is what makes it post"
    );

    // `pump` is the synchronous step the worker thread runs in a loop.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        app.ctx.services.online.update(Some(PresenceState {
            activity: "Driving".to_string(),
            detail: "Chicago to Indianapolis".to_string(),
        }));
        app.ctx.services.online.pump();
    }));

    assert!(outcome.is_err() || was_refused("/freight-fate/presence"));
    assert!(
        was_refused("/freight-fate/presence"),
        "{:?}",
        net::refused_requests()
    );
    // Nothing signed on, so nothing has to sign off; drop the app quietly.
    app.ctx.services.online.set_identity(None);
}

/// The same `..Default::default()` construction the app uses, standing on its
/// own: an injected transport is still what a test gets, and the default is
/// still the live one -- which is why the capability, and not the injection
/// point, is what stands between the suite and orinks.net.
#[test]
fn test_the_default_service_options_carry_the_live_transport() {
    let presence = OnlinePresence::new(OnlinePresenceOptions {
        enabled: true,
        identity: Some(OnlineIdentity::new("guard-default", "guard-token")),
        threaded: false,
        ..Default::default()
    });
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        presence.update(Some(PresenceState {
            activity: "Driving".to_string(),
            detail: "guard".to_string(),
        }));
    }));
    assert!(outcome.is_err(), "the live transport was not the default");
    assert!(
        was_refused(&online_presence::base_url()),
        "{:?}",
        net::refused_requests()
    );
}

/// The real game's behaviour is unchanged: an injected transport is called
/// and answered from, and never touches the capability at all.
#[test]
fn test_an_injected_transport_still_answers_for_the_service() {
    use freight_fate::net::testing::FakeTransport;
    use freight_fate::net::Transport;

    let transport = FakeTransport::new();
    let url = nowhere("injected");
    let reply = transport.call(&url, None, &[], None).expect("a reply");
    assert_eq!(reply["ok"], serde_json::json!(true));
    assert_eq!(transport.request_count(), 1);
    assert!(!was_refused(&url));
}
