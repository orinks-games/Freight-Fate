//! The spoken manual: the drive's F1 keyboard and controller help
//! (`states/driving_controls/help.rs`), the main menu's How to play pages
//! (`states/main_menu_help.rs`), and the line the dispatch board must NOT
//! carry (`states/city/board.rs`).
//!
//! Ported from `tests/test_driving_features.py`:
//! `test_driving_help_explains_selected_automatic_direction_style`,
//! `test_driving_f1_describes_safe_shutdown_and_destination_parking`,
//! `test_how_to_play_documents_new_gameplay_systems` and
//! `test_dispatch_board_keeps_route_planning_out_of_load_offer`.

use ff_core::data::world::get_world;
use ff_core::models::jobs::{JobBoard, OfferOptions};
use ff_core::models::profile::Profile;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::city::JobBoardState;
use freight_fate::states::city_pickup::route_planning_summary;
use freight_fate::states::main_menu::help_pages;

// -- rigging -------------------------------------------------------------------------

fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
    });
    harness.clear_speech();
    harness
}

/// Everything the last help call put on the main channel, joined -- the help
/// is spoken as one long block and Python read it as `spoken[-1]`.
fn last_help(harness: &PlaytestHarness) -> String {
    harness.app.main_lines().last().cloned().unwrap_or_default()
}

// -- the drive's own help ---------------------------------------------------------------

#[test]
fn test_driving_help_explains_selected_automatic_direction_style() {
    let mut harness = a_drive("Direction Help");

    harness.app.ctx.settings.automatic_direction_changes = "simple".to_string();
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_keyboard_help(ctx));
    let said = last_help(&harness);
    assert!(said.contains("simple direction changes"), "{said}");
    assert!(said.contains("press and hold it again"), "{said}");
    assert!(said.contains("holds the truck"), "{said}");
    assert!(
        said.contains("R progress, distance left, and where you are"),
        "{said}"
    );
    assert!(
        said.contains(
            "T plans the recommended break or sleep stop when hours of service planning hints are on"
        ),
        "{said}"
    );
    assert!(
        said.contains("Fully stopped away from route points"),
        "{said}"
    );
    assert!(said.contains("emergency shoulder-sleep warning"), "{said}");

    harness.app.ctx.settings.automatic_direction_changes = "deliberate".to_string();
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_keyboard_help(ctx));
    let said = last_help(&harness);
    assert!(said.contains("deliberate direction changes"), "{said}");
    assert!(said.contains("press and hold it again"), "{said}");
    assert!(said.contains("A quick tap just brakes"), "{said}");

    harness.app.ctx.settings.automatic_direction_changes = "simple".to_string();
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_controller_help(ctx));
    let said = last_help(&harness);
    assert!(said.contains("simple direction changes"), "{said}");
    assert!(said.contains("press and hold it again"), "{said}");
    assert!(
        said.contains("D-pad up reads your route and current location"),
        "{said}"
    );
    assert!(
        said.contains(
            "plans a recommended break or sleep stop when hours of service planning hints are on"
        ),
        "{said}"
    );
    assert!(
        said.contains("Away from route points while fully stopped"),
        "{said}"
    );
    assert!(said.contains("opens emergency shoulder sleep"), "{said}");

    harness.app.ctx.settings.automatic_direction_changes = "deliberate".to_string();
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_controller_help(ctx));
    let said = last_help(&harness);
    assert!(said.contains("deliberate direction changes"), "{said}");
    assert!(
        said.contains("let the left trigger return to neutral"),
        "{said}"
    );
}

#[test]
fn test_driving_f1_describes_safe_shutdown_and_destination_parking() {
    let mut harness = a_drive("F1 Help");
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::F1, None)));

    let help_text = last_help(&harness);
    assert!(
        help_text.contains("stops it only below 5 miles per hour"),
        "{help_text}"
    );
    assert!(
        help_text.contains("stop, then dock and deliver"),
        "{help_text}"
    );
    assert!(
        help_text.contains("Left or Right Control stops the driving event voice"),
        "{help_text}"
    );
}

// -- the manual --------------------------------------------------------------------------

#[test]
fn test_how_to_play_documents_new_gameplay_systems() {
    let app = TestApp::new();
    let help_text: String = help_pages(&app.ctx)
        .into_iter()
        .flat_map(|(_title, lines)| lines)
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    for phrase in [
        "wait for air pressure to reach 100 psi",
        "press and hold it again to reverse",
        "brake held through the stop keeps the truck stopped",
        "both direction-change styles",
        "active speed-control mode",
        "open-road target",
        "company drivers follow dispatch",
        "owner-operators choose a route after loading",
        "nearby route points always take priority",
        "thirty consecutive non-driving minutes",
        "both hours modes use these limits",
        "eleven driving hours",
        "fourteen-hour duty window",
        "three quarters as take-home",
        "career stats",
        "component wear",
        "engine tune",
        "chemical terminals",
        "roadside mechanic",
        "streamer-safe",
        "m3u8",
        "controller buttons",
        "private backups",
        "the deadline keeps running",
        "full lane keeping",
        "next safe turnaround",
        "posted-limit drops",
        "cb chatter",
        "next multiple of five",
        "computer clock",
        "fully-rested ten-hour sleep",
    ] {
        assert!(
            help_text.contains(phrase),
            "How to play never says {phrase:?}"
        );
    }
}

// -- the dispatch board --------------------------------------------------------------------

#[test]
fn test_dispatch_board_keeps_route_planning_out_of_load_offer() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Dispatch Test", "New York"));
    let world = get_world();
    let mut board = JobBoard::new(world, Some(2), None);
    let jobs = board.offers(
        "New York",
        &["refrigerated", "heavy_haul", "high_value"],
        OfferOptions::level(5),
    );
    assert!(!jobs.is_empty());
    let mut state = JobBoardState::new(&app.ctx, jobs);
    let items = state.build_items(&mut app.ctx);
    let rows: Vec<String> = items
        .iter()
        .map(|item| item.text(&state, &app.ctx))
        .collect();

    assert!(
        rows.iter().any(|row| row.contains("Equipment:")),
        "{rows:#?}"
    );
    assert!(
        rows.iter().all(|row| !row.contains("Legal HOS plan")),
        "{rows:#?}"
    );
    assert!(
        rows.iter().all(|row| !row.contains("Route has")),
        "{rows:#?}"
    );
    assert!(
        rows.iter().all(|row| !row.contains("Fuel-capable stops")),
        "{rows:#?}"
    );
    let first_help = items[0].help_text(&state, &app.ctx);
    assert!(
        first_help.contains("Route inspection after pickup covers rest, fuel, toll"),
        "{first_help}"
    );

    let toll_route = world
        .route_from_cities(&["New York", "Philadelphia"])
        .expect("New York to Philadelphia is a route");
    let summary = route_planning_summary(&toll_route);
    assert!(summary.contains("Legal HOS plan"), "{summary}");
    assert!(summary.contains("Fuel-capable stops:"), "{summary}");
    assert!(
        summary.contains("Estimated tolls, carrier-paid"),
        "{summary}"
    );
    assert!(summary.contains("not a guaranteed open space"), "{summary}");
}
