//! A phone app leaving and coming back (`ff_core::app_lifecycle`, applied in
//! `app::lifecycle`): the drive pauses, the screen is read again, and the
//! background save puts itself back at the last stop.

use ff_core::app_lifecycle::{AppEvent, AppPlace};
use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::share;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::base::InputEvent;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::DrivingStatusState;
use freight_fate::states::driving_pause_states::PauseMenuState;
use serde_json::json;

fn start_drive(app: &mut TestApp) -> SharedState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Phone", "Buffalo"));
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester is supported");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut drive = DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_DELIVERY, None);
    drive.trip.hazard_check_mi = 1e9;
    drive.trip.inspection_check_mi = 1e9;
    drive.trip.traffic_manager.rolling_bubble = false;
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    let shared = share(drive);
    app.ctx.push_shared_with(shared.clone(), false);
    app.ctx.run_deferred();
    shared
}

fn send(app: &mut TestApp, events: &[AppEvent]) {
    for &event in events {
        app.handle_event(&InputEvent::App(event));
    }
}

fn top_is<T: 'static>(app: &TestApp) -> bool {
    app.ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<T>())
}

fn pause_menus(app: &TestApp) -> usize {
    app.ctx
        .states()
        .iter()
        .filter(|state| state.borrow().as_any().is::<PauseMenuState>())
        .count()
}

#[test]
fn leaving_at_the_wheel_pauses_the_drive() {
    let mut app = TestApp::new();
    start_drive(&mut app);
    send(&mut app, &[AppEvent::WillResignActive]);
    assert!(top_is::<PauseMenuState>(&app));
    assert_eq!(app.app_place(), AppPlace::Inactive);
    app.shutdown();
}

#[test]
fn the_sound_taken_or_headphones_out_pause_the_drive() {
    for event in [AppEvent::AudioInterruptionBegan, AppEvent::OutputDeviceLost] {
        let mut app = TestApp::new();
        start_drive(&mut app);
        send(&mut app, &[event]);
        assert!(top_is::<PauseMenuState>(&app), "{event:?}");
        assert_eq!(app.app_place(), AppPlace::Active);
        app.shutdown();
    }
}

#[test]
fn a_pause_menu_already_up_does_not_open_a_second() {
    let mut app = TestApp::new();
    start_drive(&mut app);
    send(
        &mut app,
        &[AppEvent::WillResignActive, AppEvent::DidBecomeActive],
    );
    assert_eq!(pause_menus(&app), 1);
    // The call's interruption lands on the pause menu the resign opened.
    send(
        &mut app,
        &[
            AppEvent::WillResignActive,
            AppEvent::AudioInterruptionBegan,
            AppEvent::DidBecomeActive,
            AppEvent::OutputDeviceLost,
        ],
    );
    assert_eq!(pause_menus(&app), 1);
    app.shutdown();
}

#[test]
fn coming_back_reads_the_pause_menu_from_its_title() {
    let mut app = TestApp::new();
    start_drive(&mut app);
    send(&mut app, &[AppEvent::WillResignActive]);
    app.clear_speech();
    send(&mut app, &[AppEvent::DidBecomeActive]);
    let lines = app.main_lines();
    assert_eq!(
        lines.first().map(String::as_str),
        Some("Paused."),
        "{lines:?}"
    );
    assert!(lines.len() >= 2, "the focused row follows: {lines:?}");
    app.shutdown();
}

#[test]
fn coming_back_to_a_menu_reads_its_focused_row_once() {
    let mut app = TestApp::new();
    start_drive(&mut app);
    let status = DrivingStatusState::new(&app.ctx);
    app.push_state(status);
    app.clear_speech();
    send(&mut app, &[AppEvent::WillResignActive]);
    // Not at the wheel: nothing pauses, nothing is said.
    assert!(top_is::<DrivingStatusState>(&app));
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    send(&mut app, &[AppEvent::DidBecomeActive]);
    assert_eq!(app.main_lines().len(), 1, "{:?}", app.main_lines());
    app.shutdown();
}

#[test]
fn the_background_saves_the_drive_and_coming_back_puts_the_stop_back() {
    let mut app = TestApp::new();
    let shared = start_drive(&mut app);
    let stop = json!({"kind": "delivery", "position_mi": 0.0});
    app.ctx.profile.as_mut().unwrap().active_trip = Some(stop.clone());
    shared
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .unwrap()
        .trip
        .position_mi = 21.5;
    send(
        &mut app,
        &[AppEvent::WillResignActive, AppEvent::DidEnterBackground],
    );
    let written = app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .active_trip
        .clone()
        .unwrap();
    assert_eq!(written["position_mi"], json!(21.5));
    assert_eq!(written["kind"], json!("delivery"));
    assert_eq!(app.app_place(), AppPlace::Background);
    send(
        &mut app,
        &[AppEvent::WillEnterForeground, AppEvent::DidBecomeActive],
    );
    assert_eq!(app.ctx.profile.as_ref().unwrap().active_trip, Some(stop));
    assert_eq!(app.app_place(), AppPlace::Active);
    app.shutdown();
}

#[test]
fn a_newer_stop_written_meanwhile_is_kept() {
    let mut app = TestApp::new();
    start_drive(&mut app);
    app.ctx.profile.as_mut().unwrap().active_trip = Some(json!({"kind": "delivery"}));
    send(&mut app, &[AppEvent::DidEnterBackground]);
    let newer = json!({"kind": "delivery", "position_mi": 40.0});
    app.ctx.profile.as_mut().unwrap().active_trip = Some(newer.clone());
    send(&mut app, &[AppEvent::DidBecomeActive]);
    assert_eq!(app.ctx.profile.as_ref().unwrap().active_trip, Some(newer));
    app.shutdown();
}

#[test]
fn a_drive_the_save_does_not_carry_is_not_written() {
    let mut app = TestApp::new();
    start_drive(&mut app);
    app.ctx.profile.as_mut().unwrap().active_trip = None;
    send(&mut app, &[AppEvent::DidEnterBackground]);
    assert_eq!(app.ctx.profile.as_ref().unwrap().active_trip, None);
    app.shutdown();
}

#[test]
fn a_window_losing_focus_keeps_the_drive_running() {
    // The desktop: alt-tab never pauses.
    let mut app = TestApp::new();
    start_drive(&mut app);
    app.handle_event(&InputEvent::WindowFocusLost);
    app.handle_event(&InputEvent::WindowFocusGained);
    assert!(top_is::<DrivingState>(&app));
    assert_eq!(app.app_place(), AppPlace::Active);
    app.shutdown();
}

/// iOS ends the app in the background, with no return: the next launch's
/// Continue resumes the drive from what the background wrote to disk.
#[test]
fn continue_after_the_app_was_ended_resumes_the_drive_where_it_left() {
    let mut app = TestApp::new();
    let shared = start_drive(&mut app);
    // The save carries the drive from its dispatch, as a real one does.
    let at_dispatch = with_drive(&shared, |d| d.snapshot(&app.ctx));
    app.ctx.profile.as_mut().unwrap().active_trip = Some(at_dispatch);
    app.ctx.save_profile();
    with_drive(&shared, |d| {
        d.trip.position_mi = 21.5;
        d.trip.truck.parking_brake = false;
    });
    send(
        &mut app,
        &[AppEvent::WillResignActive, AppEvent::DidEnterBackground],
    );
    let path = app.ctx.profile.as_ref().unwrap().path();
    // Nothing more runs: the system ended the app here.
    let on_disk = Profile::load(&path).expect("the background wrote the save");
    let trip = on_disk.active_trip.clone().expect("a drive in the save");
    assert_eq!(trip["position_mi"], json!(21.5));

    // The next launch, on the same data, and Continue.
    app.ctx.profile = Some(on_disk);
    let resumed = freight_fate::states::main_menu::world_entry_state(&mut app.ctx, false);
    let (position, parked) = resumed
        .borrow()
        .as_any()
        .downcast_ref::<DrivingState>()
        .map(|d| (d.trip.position_mi, d.trip.truck.parking_brake))
        .expect("Continue resumes the drive, not the terminal");
    assert!((position - 21.5).abs() < 0.01, "{position}");
    // It resumes stopped, so it resumes with the parking brake set, as the
    // resume line says.
    assert!(parked);
    app.shutdown();
}

/// Every order iOS may send a call in reads the pause menu once.
#[test]
fn a_call_reads_the_pause_menu_once_in_any_order() {
    use AppEvent::*;
    let orders: [&[AppEvent]; 3] = [
        &[
            WillResignActive,
            AudioInterruptionBegan,
            DidBecomeActive,
            AudioInterruptionEnded,
        ],
        &[
            AudioInterruptionBegan,
            WillResignActive,
            AudioInterruptionEnded,
            DidBecomeActive,
        ],
        &[WillResignActive, AudioInterruptionBegan, DidBecomeActive],
    ];
    for events in orders {
        let mut app = TestApp::new();
        start_drive(&mut app);
        let (leave, back) = events.split_at(2);
        send(&mut app, leave);
        app.clear_speech();
        send(&mut app, back);
        let titles = app.main_lines().iter().filter(|l| *l == "Paused.").count();
        assert_eq!(titles, 1, "{events:?}: {:?}", app.main_lines());
        app.shutdown();
    }
}

fn with_drive<R>(shared: &SharedState, f: impl FnOnce(&mut DrivingState) -> R) -> R {
    let mut borrowed = shared.borrow_mut();
    f(borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .unwrap())
}

#[test]
fn a_reader_says_its_opening_line_again_on_return() {
    use freight_fate::states::main_menu::HelpState;
    let mut app = TestApp::new();
    app.push_state(HelpState::new());
    app.clear_speech();
    send(
        &mut app,
        &[AppEvent::WillResignActive, AppEvent::DidBecomeActive],
    );
    let lines = app.main_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("How to play."), "{lines:?}");
    app.shutdown();
}

#[test]
fn a_name_field_says_its_prompt_again_on_return() {
    use freight_fate::states::text_entry::TextEntryState;
    let mut app = TestApp::new();
    app.push_state(TextEntryState::new("Driver name", "Name", |_, _| {}));
    app.clear_speech();
    send(
        &mut app,
        &[AppEvent::WillResignActive, AppEvent::DidBecomeActive],
    );
    let lines = app.main_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("Driver name."), "{lines:?}");
    app.shutdown();
}
