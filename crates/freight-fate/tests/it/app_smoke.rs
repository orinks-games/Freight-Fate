//! The headless `--smoke` run: `tests/test_smoke.py`'s app-level flows, from
//! the boot-and-five-frames check `main --smoke` does through the whole
//! new-career-to-delivery walk, plus the smoke verification of the baked
//! data.

use crate::states_city_support::*;
use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::app::{smoke_audio_checks, smoke_checks, version, CliOptions};
use freight_fate::states::base::{InputEvent, Key, Menu};
use freight_fate::states::city::{
    CityMenuState, GarageState, JobBoardState, TruckShopState, UpgradeShopState,
};
use freight_fate::states::city_pickup::PickupFacilityState;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DRIVE_PHASE_DELIVERY, DRIVE_PHASE_PICKUP};
use freight_fate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};
use freight_fate::states::driving_pause_states::{AbandonJobConfirmationState, PauseMenuState};
use freight_fate::states::main_menu::{
    CareerStartState, HomeCityState, HomeTerminalState, MainMenuState, NameEntryState,
};
use freight_fate::updater;

/// `accept_pickup_drive`'s head: New career through the four defaults, which
/// lands on the destination terminal's city hub.
fn new_career_to_city(app: &mut TestApp) {
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(app, "New career");
    assert!(is::<NameEntryState>(app));
    key(app, Key::Return); // default name
    assert!(is::<CareerStartState>(app));
    key(app, Key::Return); // default start
    key(app, Key::Return); // default region
    key(app, Key::Return); // default home terminal
    assert!(is::<CityMenuState>(app));
}

/// ...and on through the assigned dispatch to the deadhead.
fn new_career_to_pickup_drive(app: &mut TestApp) {
    new_career_to_city(app);
    key(app, Key::Return); // job board
    accept_assigned_freight_with_deadhead(app);
}

/// Accept dispatch's assignment, declining a load staged in the home yard
/// while declines last: that one opens the shipping office in place, and
/// these flows are about the deadhead. The board is seeded under test, so
/// this is a fixed sequence of presses rather than a retry.
fn accept_assigned_freight_with_deadhead(app: &mut TestApp) {
    assert!(with_state::<JobBoardState, _>(app, |b, _| b.assigned_mode()));
    while with_state::<JobBoardState, _>(app, |b, ctx| b.assigned_load_is_staged_here(ctx)) {
        if !labels::<JobBoardState>(app)
            .iter()
            .any(|row| row.starts_with("Decline"))
        {
            break; // out of declines; the accept below says what happened
        }
        select::<JobBoardState>(app, "Decline and request another load");
        assert!(is::<JobBoardState>(app));
    }
    key(app, Key::Home);
    assert!(labels::<JobBoardState>(app)[0].starts_with("Accept assigned dispatch:"));
    key(app, Key::Return);
    assert!(
        is::<DrivingState>(app),
        "accepting the assignment did not start the deadhead"
    );
}

#[test]
fn a_headless_smoke_run_boots_five_frames_and_exits_cleanly() {
    let mut app = TestApp::new();
    app.run(Some(5));
    assert!(!app.running());
    // The real main menu greets the player on entry, as it does in Python.
    let first = &app.main_lines()[0];
    assert!(
        first.starts_with("Freight Fate "),
        "unexpected first line: {first}"
    );
    assert!(!first.contains("An audio trucking adventure across America."));
}

#[test]
fn smoke_checks_find_every_baked_runtime_file() {
    // get_world, the sound assets, the buff catalog, a curve shard, the
    // facility approaches, the radio catalog, and the secret store.
    let _guard = freight_fate::app::testing::env_lock();
    freight_fate::app::testing::set_headless_env();
    if let Err(e) = smoke_checks() {
        // The secret store is the one check that depends on the machine
        // rather than the build; everything before it must pass.
        //
        // The sound assets are the other. CI checks out without LFS on
        // purpose -- fetching the packs on every push is what exhausted the
        // repository's LFS budget -- so `sounds.pak` is a 130-byte pointer
        // there and the lookup fails against pointer text, not because
        // anything is wrong with the build. `shipped_sounds()` is the same
        // guard the audio cases use, and it prints WHY it skipped, so an
        // unmaterialised pack cannot quietly turn this into a green run that
        // proved nothing.
        // `contains`, not `starts_with`: smoke_checks wraps this one as
        // `format!("smoke: {e}")`, so the message arrives as
        // "smoke: Sound assets are missing or unreadable: ui/menu_select".
        // The secret-store arm below is returned unprefixed, which is why it
        // can match from the start and this cannot.
        if e.contains("Sound assets are missing or unreadable")
            && !crate::audio_support::shipped_sounds()
        {
            return;
        }
        assert!(
            e.starts_with("Secret store unreachable"),
            "smoke check failed: {e}"
        );
    }
}

#[test]
fn smoke_audio_check_decodes_the_menu_sound_through_hearken() {
    let _guard = crate::audio_support::audio_lock();
    let result = smoke_audio_checks();
    if crate::audio_support::shipped_sounds() {
        result.expect("the packed menu sound should decode through Hearken");
    } else {
        assert!(result
            .expect_err("a build without its sounds must fail the release smoke check")
            .contains("ui/menu_select"));
    }
}

#[test]
fn the_smoke_sound_error_is_recognisable_through_its_prefix() {
    // The guard in smoke_checks_find_every_baked_runtime_file has to spot the
    // sound-asset failure to skip it on an LFS-less checkout, and it got this
    // wrong once: `smoke_checks` wraps this one error as `format!("smoke: {e}")`
    // while returning the secret-store one unprefixed, so a `starts_with` match
    // never fired and the Windows runner stayed red. Neither string is
    // reachable from a machine that HAS the pack, so nothing here would have
    // caught it -- this pins the shape instead.
    let wrapped = format!("smoke: {}", freight_fate::audio::SOUND_ASSETS_MISSING);
    assert!(!wrapped.starts_with("Sound assets are missing or unreadable"));
    assert!(wrapped.contains("Sound assets are missing or unreadable"));
}

#[test]
fn cli_options_parse_the_three_switches() {
    let opts = CliOptions::parse(["--smoke".to_string(), "--headless".to_string()]);
    assert!(opts.smoke && opts.headless && !opts.controller_diagnostics);
    let opts = CliOptions::parse(["--controller-diagnostics".to_string()]);
    assert!(opts.controller_diagnostics);
    assert_eq!(
        CliOptions::parse(Vec::<String>::new()),
        CliOptions::default()
    );
}

#[test]
fn the_version_is_the_crate_version_from_source() {
    assert_eq!(version(), env!("CARGO_PKG_VERSION"));
}

// `test_garage_offers_partial_fuel_and_repairs_when_cash_is_short` is live in `crates/freight-fate/tests/states_city_shops.rs`.

#[test]
fn test_full_game_flow_headless() {
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    assert!(is::<MainMenuState>(&app));
    assert_eq!(app.visible_lines()[0], "Freight Fate");
    let welcome = format!("Freight Fate {}.", updater::spoken_version(version()));
    assert!(app.main_lines().iter().any(|line| line.contains(&welcome)));

    // navigate to "New career" and select it
    select::<MainMenuState>(&mut app, "New career");
    assert!(is::<NameEntryState>(&app));

    for ch in "Smoke".chars() {
        app.dispatch_to_state(&InputEvent::typed(ch));
    }
    key(&mut app, Key::Return);
    assert!(is::<CareerStartState>(&app));
    key(&mut app, Key::Return); // default start: Northstar
    assert!(is::<HomeTerminalState>(&app));
    key(&mut app, Key::Return); // default region: Great Lakes
    assert!(is::<HomeCityState>(&app));
    key(&mut app, Key::Return); // default city: Chicago
    assert!(is::<CityMenuState>(&app));
    assert_eq!(profile(&app).name, "Smoke");
    assert_eq!(profile(&app).current_city, "chicago_il_us");

    // Open dispatch board and accept the assigned job: a new hire runs
    // dispatch's load, and the trainer recommendation keeps it short enough
    // for the bounded smoke run.
    key(&mut app, Key::Return);
    assert!(is::<JobBoardState>(&app));
    assert!(with_state::<JobBoardState, _>(&app, |b, _| !b
        .jobs
        .is_empty()));
    accept_assigned_freight_with_deadhead(&mut app);
    assert_eq!(
        with_state::<DrivingState, _>(&app, |d, _| d.phase.to_string()),
        DRIVE_PHASE_PICKUP
    );
    with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.position_mi = d.trip.total_miles();
        d.trip.finished = true;
        d.trip.truck.velocity_mps = 0.0;
        d.trip.truck.set_parking_brake();
        d.update_frame(ctx, 1.0 / 60.0);
    });
    finish_timed_state(&mut app);
    assert!(is::<PickupFacilityState>(&app));
    key(&mut app, Key::Return); // check in at origin
                                // Either way of getting the freight aboard is a valid full flow: a
                                // dock if this shipper loads live, the drop yard if it stages
                                // trailers (tests/test_trailer_yard.py pins which shippers do which).
    let row = current_label::<PickupFacilityState>(&app);
    assert!(
        row == "Load cargo at dock" || row == "Drop and hook in the yard",
        "{row}"
    );
    key(&mut app, Key::Return); // load, or drop and hook
    finish_timed_state(&mut app);
    assert!(
        current_label::<PickupFacilityState>(&app).contains("Depart for destination"),
        "{}",
        current_label::<PickupFacilityState>(&app)
    );
    key(&mut app, Key::Return);

    // A new company hire runs dispatch's routing: no route menu appears.
    assert!(is::<DrivingState>(&app));
    assert_eq!(
        with_state::<DrivingState, _>(&app, |d, _| d.phase.to_string()),
        DRIVE_PHASE_DELIVERY
    );
    let departure = app
        .main_lines()
        .into_iter()
        .rev()
        .find(|text| text.contains("Dispatch routed you to"))
        .expect("a departure line");
    assert!(departure.contains("Loaded trip is"), "{departure}");
    // This flow never started the engine, and the departure line says so
    // rather than claiming a departure the truck cannot make -- it names the
    // very key the drive below presses next.
    assert!(!departure.contains("Departing now"), "{departure}");
    assert!(departure.contains("Engine off."), "{departure}");
    assert!(!departure.contains("Break stops:"), "{departure}");
    assert!(!departure.contains("Fuel stops:"), "{departure}");

    // start the engine and drive the whole trip with simulated input
    key(&mut app, Key::E);
    let drive = app.state().expect("the delivery drive");
    let d = || {
        std::cell::RefMut::map(drive.borrow_mut(), |s| {
            s.as_any_mut()
                .downcast_mut::<DrivingState>()
                .expect("the drive")
        })
    };
    assert!(d().trip.truck.engine_on);
    {
        let mut drive = d();
        drive.trip.truck.transmission.automatic = true;
        drive.trip.truck.set_air_ready(false);
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.set_npc_vehicles(Vec::new());
    }

    // The dispatch board's shortest unlocked job varies run to run, so a flat
    // frame budget flaked when the only short job was still long enough to
    // outlast it. Size the ceiling to this trip's distance with a
    // conservative crawl-speed floor; the loop still breaks the moment the
    // trip finishes, so normal runs cost the same.
    let crawl_mph = 15.0;
    let total_miles = d().trip.total_miles();
    let max_frames = (total_miles / crawl_mph * 3600.0 * 60.0) as usize + 60 * 60;
    let mut arrived = false;
    for _ in 0..max_frames {
        let mut drive = d();
        let ctx = &mut app.ctx;
        let at = drive.trip.position_mi;
        let limit_mph = drive.trip.speed_limit_at(at).0;
        let target_mph = 25.0f64.max(limit_mph + 5.0);
        if drive.trip.truck.speed_mph() > target_mph {
            drive.trip.truck.throttle = 0.0;
            drive.trip.truck.brake = 0.5;
        } else {
            drive.trip.truck.throttle = 0.8;
            drive.trip.truck.brake = 0.0;
        }
        // trip.update reapplies physics every frame that can randomly slow a
        // long headless drive below the budget: simulated weather can turn to
        // ice and cap traction to a crawl, terrain grade drags on long climbs,
        // fuel burns at time_scale (20x) so the tank can empty mid-route and
        // cut the engine, and this controller's bang-bang braking against a
        // target near the truck's governed speed bleeds the air reservoirs
        // until the spring brakes latch. This is a flow smoke test -- that
        // physics is covered by test_weather_trip, test_vehicle, and the
        // air-brake tests -- so pin full traction, flat ground, a full tank,
        // and charged air for a deterministic drive, matching the
        // hazard/inspection/traffic neutralisation above.
        drive.trip.truck.grip = 1.0;
        drive.trip.truck.grade = 0.0;
        drive.trip.truck.fuel_gal = drive.trip.truck.specs.fuel_tank_gal;
        let charged = drive.trip.truck.specs.air_governor_cut_out_psi;
        drive.trip.truck.set_air_pressure_psi(charged);
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.auto_shift();
        drive.trip.truck.update(1.0 / 60.0);
        for event in drive.trip.update(1.0 / 60.0) {
            drive.handle_trip_event(ctx, &event);
        }
        drive.update_hazard(ctx, 1.0 / 60.0);
        // always brake through hazards so the smoke run never crashes
        if drive.hazard_deadline.is_some() {
            drive.trip.truck.velocity_mps = 5.0;
        }
        if drive.trip.finished {
            drive.trip.truck.velocity_mps = 0.0;
            drive.trip.truck.set_parking_brake();
            drive.handle_arrival_gate(ctx);
            drop(drive);
            app.ctx.run_deferred();
            finish_timed_state(&mut app);
            arrived = true;
            break;
        }
    }
    // never hit trip.finished -- a real stall, not just a tight cap
    assert!(
        arrived,
        "delivery never finished in {max_frames} frames: {:.1}/{:.1} mi",
        d().trip.position_mi,
        total_miles
    );
    let destination = d().job.destination.clone();

    assert!(is::<FacilityArrivalState>(&app));
    key(&mut app, Key::Return);
    finish_timed_state(&mut app);
    assert!(is::<ArrivalState>(&app));
    assert_eq!(profile(&app).career.deliveries, 1);
    assert!(profile(&app).career.total_earnings > 0.0);
    assert_eq!(profile(&app).current_city, destination);

    // continue back to the destination terminal hub
    select::<ArrivalState>(&mut app, "Continue to");
    assert!(is::<CityMenuState>(&app));
    let terminal = app
        .ctx
        .world
        .home_terminal(&destination)
        .expect("the destination has a terminal");
    assert_eq!(
        with_state::<CityMenuState, _>(&app, |s, _| s.menu().title.clone()),
        terminal.name
    );

    // render a frame of every reachable lines() output
    app.render();
}

#[test]
fn test_menu_first_letter_navigation() {
    use freight_fate::states::base::{InputEvent, Key, Menu};
    use freight_fate::states::main_menu::MainMenuState;

    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    let read = |app: &TestApp| -> (usize, usize, String) {
        let state = app.state().unwrap();
        let state = state.borrow();
        let menu = state.as_any().downcast_ref::<MainMenuState>().unwrap();
        let core = menu.menu();
        (
            core.index,
            core.items.len(),
            core.items[core.index].text(menu, &app.ctx),
        )
    };
    app.dispatch_to_state(&InputEvent::typed('s'));
    assert!(read(&app).2.to_lowercase().starts_with('s'));
    app.dispatch_to_state(&InputEvent::key(Key::End));
    let (index, len, _) = read(&app);
    assert_eq!(index, len - 1);
    app.dispatch_to_state(&InputEvent::key(Key::Home));
    assert_eq!(read(&app).0, 0);
    app.shutdown();
}

#[test]
fn test_garage_upgrade_and_truck_purchase_flow() {
    let mut app = TestApp::new();
    new_career_to_city(&mut app);
    let save_path = {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(200_000.0);
        p.path()
    };
    let reloaded = |path: &std::path::Path| Profile::load(path).expect("the save reloads");

    // city -> garage -> upgrades
    select::<CityMenuState>(&mut app, "Garage");
    assert!(is::<GarageState>(&app));
    select::<GarageState>(&mut app, "Upgrades");
    assert!(is::<UpgradeShopState>(&app));

    // buy engine tune tier 1, then tier 2; a third press must not charge
    move_to::<UpgradeShopState>(&mut app, "Engine tune");
    key(&mut app, Key::Return);
    assert_eq!(profile(&app).upgrades.get("engine_tune"), Some(&1));
    assert_eq!(
        reloaded(&save_path).upgrades.get("engine_tune"),
        Some(&1),
        "tier 1 did not reach the save"
    );
    key(&mut app, Key::Return);
    assert_eq!(profile(&app).upgrades.get("engine_tune"), Some(&2));
    assert_eq!(reloaded(&save_path).upgrades.get("engine_tune"), Some(&2));
    let money_after_tiers = profile(&app).money();
    key(&mut app, Key::Return);
    assert_eq!(profile(&app).upgrades.get("engine_tune"), Some(&2));
    assert_eq!(profile(&app).money(), money_after_tiers);
    assert!(
        current_label::<UpgradeShopState>(&app).contains("owned"),
        "{}",
        current_label::<UpgradeShopState>(&app)
    );

    // back to garage, then the truck shop
    key(&mut app, Key::Escape);
    assert!(is::<GarageState>(&app));
    select::<GarageState>(&mut app, "Trucks");
    assert!(is::<TruckShopState>(&app));

    move_to::<TruckShopState>(&mut app, "Heavy hauler");
    let money_before = profile(&app).money();
    key(&mut app, Key::Return);
    assert_eq!(profile(&app).truck, "heavy_hauler");
    assert!(profile(&app)
        .owned_trucks
        .iter()
        .any(|k| k == "heavy_hauler"));
    assert_eq!(profile(&app).money(), money_before - 52_000.0);
    let saved = reloaded(&save_path);
    assert_eq!(saved.truck, "heavy_hauler");
    assert!(saved.owned_trucks.iter().any(|k| k == "heavy_hauler"));
    assert!(
        current_label::<TruckShopState>(&app).contains("currently driving"),
        "{}",
        current_label::<TruckShopState>(&app)
    );

    // switch back to the standard rig (already owned, no charge)
    let money_before = profile(&app).money();
    select::<TruckShopState>(&mut app, "Standard rig");
    assert_eq!(profile(&app).truck, "rig");
    assert_eq!(profile(&app).money(), money_before);
    assert_eq!(reloaded(&save_path).truck, "rig");
}

/// The Discord presence setting is a spoken, keyboard-driven menu item that
/// flips the saved setting and notifies the presence service -- and presence
/// is constructed dormant (never started) so it touches nothing until the
/// game loop runs.
///
/// Python recorded the `presence.set_enabled` calls through a monkeypatch;
/// the service is concrete here, so the notification is read back off it.
#[test]
fn test_discord_presence_toggle_is_accessible_and_wired() {
    use freight_fate::states::online_hub::OnlineHubState;

    let mut app = TestApp::new();
    let hub = OnlineHubState::new(&mut app.ctx);
    app.push_state(hub);
    move_to::<OnlineHubState>(&mut app, "Discord presence");
    // spoken help text exists for F1
    assert!(!current_help::<OnlineHubState>(&app).is_empty());
    let before = app.ctx.settings.discord_presence;
    app.clear_speech();

    key(&mut app, Key::Return); // activate to toggle
    assert_ne!(app.ctx.settings.discord_presence, before);
    let label = current_label::<OnlineHubState>(&app);
    assert!(
        label.ends_with(if before { "off" } else { "on" }),
        "{label}"
    );
    let said = app.main_lines();
    assert!(
        said.last()
            .is_some_and(|line| line.starts_with("Discord presence:")),
        "{said:?}"
    );
    // The service was told, not just the settings file.
    assert_eq!(
        app.ctx.services.presence.enabled(),
        app.ctx.settings.discord_presence
    );
}

// `test_upgrades_are_money_gated` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_garage_services_tires_and_wash` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_garage_services_brakes_and_engine` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_garage_partial_brake_service_when_broke` is live in `crates/freight-fate/tests/states_city_shops.rs`.

// `test_upgrade_f1_help_explains_player_benefits` is live in `crates/freight-fate/tests/states_city_shops.rs`.

#[test]
fn test_pause_and_abandon_returns_to_city() {
    let mut app = TestApp::new();
    new_career_to_pickup_drive(&mut app);
    assert_eq!(
        with_state::<DrivingState, _>(&app, |d, _| d.phase.to_string()),
        DRIVE_PHASE_PICKUP
    );
    with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.position_mi = d.trip.total_miles();
        d.trip.finished = true;
        d.trip.truck.velocity_mps = 0.0;
        d.trip.truck.set_parking_brake();
        d.update_frame(ctx, 1.0 / 60.0);
    });
    finish_timed_state(&mut app);
    assert!(is::<PickupFacilityState>(&app));
    key(&mut app, Key::Return); // check in at origin
    key(&mut app, Key::Return); // load at dock, or drop and hook
    finish_timed_state(&mut app);
    key(&mut app, Key::Return); // depart on assigned route
    assert!(is::<DrivingState>(&app));
    let (phase, origin) =
        with_state::<DrivingState, _>(&app, |d, _| (d.phase.to_string(), d.job.origin.clone()));
    assert_eq!(phase, DRIVE_PHASE_DELIVERY);

    key(&mut app, Key::Escape);
    assert!(is::<PauseMenuState>(&app));
    let money = profile(&app).money();
    select::<PauseMenuState>(&mut app, "Abandon job");
    // The abandon now needs a Yes/No confirmation that lands on No.
    assert!(is::<AbandonJobConfirmationState>(&app));
    assert_eq!(
        current_label::<AbandonJobConfirmationState>(&app),
        "No, keep driving"
    );
    key(&mut app, Key::Down); // arrow to Yes
    key(&mut app, Key::Return);
    assert!(is::<CityMenuState>(&app));
    assert_eq!(profile(&app).money(), money - 500.0);
    assert_eq!(profile(&app).current_city, origin);
}

#[test]
fn test_abandon_prompt_no_returns_to_pause_menu() {
    let mut app = TestApp::new();
    new_career_to_pickup_drive(&mut app);

    key(&mut app, Key::Escape);
    assert!(is::<PauseMenuState>(&app));
    let pause = app.state().expect("the pause menu");
    let money = profile(&app).money();
    let active_trip = profile(&app).active_trip.clone();
    select::<PauseMenuState>(&mut app, "Abandon job");
    assert!(is::<AbandonJobConfirmationState>(&app));
    // Enter on the default "No" cancels and returns to the pause menu.
    key(&mut app, Key::Return);
    assert!(std::rc::Rc::ptr_eq(&app.state().expect("a state"), &pause));
    assert_eq!(profile(&app).money(), money);
    assert_eq!(profile(&app).active_trip, active_trip);
}
