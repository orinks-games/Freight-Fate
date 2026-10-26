//! Port of `tests/test_controller.py`: hint routing, the manager, menus,
//! and driving. The manager tests run against the fake pad and subsystem
//! (`controller::fakes`), the Python `object()` controller and `FakeSDL`.

use std::cell::RefCell;
use std::rc::Rc;

use ff_core::data::world::get_world;
use ff_core::input_hints::{control_hint, CONTROLLER, KEYBOARD};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::Zone;
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::testing::TestApp;
use freight_fate::app::GameContext;
use freight_fate::app::{share, SharedState};
use freight_fate::controller::fakes::{fake_factory, FakePad, FakeSdlLog};
use freight_fate::controller::{
    ControllerAction, ControllerAxis, ControllerButton, ControllerManager, STICK_CLICK_GUARD_S,
};
use freight_fate::states::base::{InputEvent, Key, State};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::DrivingStatusState;
use freight_fate::states::driving_pause_states::PauseMenuState;

fn button(button: ControllerButton, instance_id: u32) -> InputEvent {
    InputEvent::ControllerButtonDown {
        button,
        instance_id,
    }
}

fn button_up(button: ControllerButton, instance_id: u32) -> InputEvent {
    InputEvent::ControllerButtonUp {
        button,
        instance_id,
    }
}

fn axis(axis: ControllerAxis, value: i16, instance_id: u32) -> InputEvent {
    InputEvent::ControllerAxis {
        axis,
        value,
        instance_id,
    }
}

/// `m._controller = object(); m._instance_id = id`: a bound pad, id latched.
fn manager_with_pad(instance_id: u32) -> ControllerManager {
    let mut m = ControllerManager::detached(true, true);
    m.bind_device(Box::new(FakePad::new(instance_id)), "test pad");
    m.set_id_pending(false);
    m
}

/// Pretend a controller is connected so the app's manager reports active.
fn force_controller(app: &mut TestApp) {
    let c = &mut app.ctx.controller;
    c.set_enabled(true);
    c.bind_device(Box::new(FakePad::new(0)), "test pad");
    c.set_id_pending(false);
    c.active_device = CONTROLLER;
}

// -- pure hint table ---------------------------------------------------------

#[test]
fn test_control_hint_follows_device() {
    assert_eq!(control_hint("take_exit", KEYBOARD), "X");
    assert_eq!(control_hint("take_exit", CONTROLLER), "D-pad down");
    assert_eq!(control_hint("accelerate", KEYBOARD), "the Up arrow");
    assert_eq!(control_hint("accelerate", CONTROLLER), "the right trigger");
}

#[test]
fn test_control_hint_unknown_action_is_audible() {
    // An unknown action returns itself rather than crashing a live prompt.
    assert_eq!(
        control_hint("not_a_real_action", CONTROLLER),
        "not_a_real_action"
    );
}

// -- manager -----------------------------------------------------------------

#[test]
fn test_manager_without_device_is_inactive() {
    let mut m = ControllerManager::detached(true, true);
    assert!(!m.active());
    assert_eq!(m.device(), KEYBOARD);
    assert!(m.tick(0.016).is_empty());
    m.shutdown();
}

#[test]
fn test_menu_action_mapping() {
    let m = ControllerManager::detached(true, true);
    let force = |b| m.menu_action(&button(b, 0));
    assert_eq!(
        force(ControllerButton::DPadUp),
        Some(ControllerAction::MenuUp)
    );
    assert_eq!(
        force(ControllerButton::DPadDown),
        Some(ControllerAction::MenuDown)
    );
    assert_eq!(
        force(ControllerButton::DPadLeft),
        Some(ControllerAction::AdjustLeft)
    );
    assert_eq!(
        force(ControllerButton::DPadRight),
        Some(ControllerAction::AdjustRight)
    );
    assert_eq!(force(ControllerButton::A), Some(ControllerAction::Confirm));
    assert_eq!(force(ControllerButton::B), Some(ControllerAction::Back));
    assert_eq!(force(ControllerButton::Back), Some(ControllerAction::Help));
    // A button-up carries no menu action.
    assert_eq!(m.menu_action(&button_up(ControllerButton::A, 0)), None);
}

#[test]
fn test_trigger_deadzone_and_smoothing() {
    let mut m = manager_with_pad(0);
    // Below the 4% trigger deadzone -> still zero.
    m.process_event(&axis(
        ControllerAxis::TriggerRight,
        (32767.0 * 0.02) as i16,
        0,
    ));
    m.tick(0.016);
    assert_eq!(m.throttle(), 0.0);
    // Full press smooths up toward 1.0 over a few frames.
    m.process_event(&axis(ControllerAxis::TriggerRight, 32767, 0));
    for _ in 0..30 {
        m.tick(0.016);
    }
    assert!(m.throttle() > 0.95);
    m.shutdown();
}

#[test]
fn test_clutch_is_instant_like_shift() {
    // The left bumper is a digital button, so the clutch engages and
    // releases immediately -- matching the keyboard Shift -- with no
    // smoothing lag.
    let mut m = manager_with_pad(0);
    m.process_event(&button(ControllerButton::LeftShoulder, 0));
    assert_eq!(m.clutch(), 1.0); // no tick needed
    m.process_event(&button_up(ControllerButton::LeftShoulder, 0));
    assert_eq!(m.clutch(), 0.0);
    m.shutdown();
}

#[test]
fn test_modifier_tracks_right_bumper() {
    let mut m = manager_with_pad(0);
    assert!(!m.modifier);
    m.process_event(&button(ControllerButton::RightShoulder, 0));
    assert!(m.modifier);
    m.process_event(&button_up(ControllerButton::RightShoulder, 0));
    assert!(!m.modifier);
    m.shutdown();
}

#[test]
fn test_dpad_hold_auto_repeats() {
    let mut m = manager_with_pad(0);
    m.process_event(&button(ControllerButton::DPadLeft, 0));
    // Nothing before the initial delay, then repeats accumulate while held.
    assert!(m.tick(0.1).is_empty());
    let first = m.tick(0.25); // crosses the 0.3s initial delay
    assert_eq!(first.len(), 1);
    assert_eq!(first[0], button(ControllerButton::DPadLeft, 0));
    // Release stops further repeats.
    m.process_event(&button_up(ControllerButton::DPadLeft, 0));
    assert!(m.tick(1.0).is_empty());
    m.shutdown();
}

#[test]
fn test_right_stick_axes_generate_hysteretic_buttons_without_diagonals_or_click_bounce() {
    let mut m = manager_with_pad(0);
    m.process_event(&axis(ControllerAxis::RightX, (32767.0 * 0.59) as i16, 0));
    assert!(m.take_generated_events().is_empty());
    m.process_event(&axis(ControllerAxis::RightX, (32767.0 * 0.61) as i16, 0));
    assert_eq!(
        m.take_generated_events(),
        vec![button(ControllerButton::RightStickRight, 0)]
    );
    m.process_event(&axis(ControllerAxis::RightX, (32767.0 * 0.31) as i16, 0));
    assert!(m.take_generated_events().is_empty());
    m.process_event(&axis(ControllerAxis::RightX, (32767.0 * 0.29) as i16, 0));
    assert_eq!(
        m.take_generated_events(),
        vec![button_up(ControllerButton::RightStickRight, 0)]
    );

    m.process_event(&axis(ControllerAxis::RightY, (32767.0 * 0.7) as i16, 0));
    assert_eq!(
        m.take_generated_events(),
        vec![button(ControllerButton::RightStickDown, 0)]
    );
    m.process_event(&axis(ControllerAxis::RightX, (32767.0 * 0.7) as i16, 0));
    assert_eq!(
        m.take_generated_events(),
        vec![button_up(ControllerButton::RightStickDown, 0)]
    );
    m.process_event(&button(ControllerButton::RightStick, 0));
    m.process_event(&axis(ControllerAxis::RightX, 0, 0));
    m.process_event(&axis(ControllerAxis::RightY, 0, 0));
    m.process_event(&axis(ControllerAxis::RightY, (32767.0 * 0.7) as i16, 0));
    assert!(m.take_generated_events().is_empty());
    m.tick(STICK_CLICK_GUARD_S);
    m.process_event(&axis(ControllerAxis::RightY, 0, 0));
    m.process_event(&axis(ControllerAxis::RightY, (32767.0 * 0.7) as i16, 0));
    assert_eq!(
        m.take_generated_events(),
        vec![button(ControllerButton::RightStickDown, 0)]
    );
    m.shutdown();
}

#[test]
fn test_right_stick_repeats_only_on_the_plain_layer() {
    let up = (32767.0 * 0.8) as i16;
    // RB held first: the stick press fires once and never repeats.
    let mut m = manager_with_pad(0);
    m.process_event(&button(ControllerButton::RightShoulder, 0));
    m.process_event(&axis(ControllerAxis::RightY, -up, 0));
    assert_eq!(
        m.take_generated_events(),
        vec![button(ControllerButton::RightStickUp, 0)]
    );
    let mut repeats = 0;
    for _ in 0..120 {
        repeats += m.tick(1.0 / 60.0).len();
    }
    assert_eq!(repeats, 0);
    // Releasing RB while the stick is still up does not start a repeat.
    m.process_event(&button_up(ControllerButton::RightShoulder, 0));
    for _ in 0..120 {
        repeats += m.tick(1.0 / 60.0).len();
    }
    assert_eq!(repeats, 0);
    m.shutdown();

    // Stick first (volume repeat running), then RB: the repeat stops.
    let mut m = manager_with_pad(0);
    m.process_event(&axis(ControllerAxis::RightY, -up, 0));
    m.take_generated_events();
    assert_eq!(m.tick(0.4).len(), 1);
    m.process_event(&button(ControllerButton::RightShoulder, 0));
    for _ in 0..120 {
        assert!(m.tick(1.0 / 60.0).is_empty());
    }
    m.shutdown();
}

#[test]
fn test_disconnect_latches_once() {
    let mut m = manager_with_pad(7);
    m.process_event(&InputEvent::ControllerRemoved { instance_id: 7 });
    assert!(!m.connected());
    assert!(m.take_disconnect());
    assert!(!m.take_disconnect()); // consumed
    m.shutdown();
}

#[test]
fn test_reconnect_reopens_after_removed() {
    // A device-added after a device-removed must reopen the pad, so a
    // reconnect restores controller input rather than leaving it dead.
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    let mut m = ControllerManager::new(
        true,
        true,
        Some(fake_factory(vec![("pad".to_string(), 7)], Rc::clone(&log))),
    );
    assert!(m.connected()); // bound at startup
    m.process_event(&InputEvent::ControllerRemoved { instance_id: 7 });
    assert!(!m.connected());

    let opens_before = log.borrow().opens.len();
    m.process_event(&InputEvent::ControllerAdded { device_index: 3 });
    assert_eq!(log.borrow().opens.len(), opens_before + 1); // bound the reconnected pad
    assert!(m.connected());
    m.shutdown();
}

#[test]
fn a_plugged_in_pad_is_named_once_but_the_startup_pad_is_not() {
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    let mut m = ControllerManager::new(
        true,
        true,
        Some(fake_factory(
            vec![("Xbox Series X Controller".to_string(), 7)],
            Rc::clone(&log),
        )),
    );
    // SDL repeats an added event for the pad bound at startup.
    m.process_event(&InputEvent::ControllerAdded { device_index: 0 });
    assert_eq!(m.take_connect(), None);
    m.process_event(&InputEvent::ControllerRemoved { instance_id: 7 });
    m.process_event(&InputEvent::ControllerAdded { device_index: 0 });
    assert_eq!(
        m.take_connect().as_deref(),
        Some("Xbox Series X Controller")
    );
    assert_eq!(m.take_connect(), None); // consumed
    m.shutdown();
}

/// A reconnect says which controller the game is now listening to.
#[test]
fn reconnect_speaks_the_controller_name() {
    let mut app = TestApp::new();
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    app.ctx.controller = ControllerManager::new(
        true,
        true,
        Some(fake_factory(
            vec![("Xbox Wireless Controller".to_string(), 4)],
            log,
        )),
    );
    app.dispatch_controller(&InputEvent::ControllerRemoved { instance_id: 4 });
    app.dispatch_controller(&InputEvent::ControllerAdded { device_index: 0 });
    app.tick(1.0 / 60.0);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line == "Controller connected: Xbox Wireless Controller."));
    app.shutdown();
}

#[test]
fn test_reopen_does_not_cycle_subsystem() {
    // reopen must drop the old pad and rebind WITHOUT quitting/re-initializing
    // the SDL subsystem -- cycling it re-registers the event watch and
    // doubles every controller event.
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    let mut m = ControllerManager::new(
        true,
        true,
        Some(fake_factory(vec![("pad".to_string(), 7)], Rc::clone(&log))),
    );
    let (inits, quits, opens) = {
        let l = log.borrow();
        (l.inits, l.quits, l.opens.len())
    };
    m.reopen();
    let l = log.borrow();
    assert_eq!(l.inits, inits);
    assert_eq!(l.quits, quits);
    assert_eq!(l.opens.len(), opens + 1); // reopened, never cycled the subsystem
    drop(l);
    m.shutdown();
}

#[test]
fn test_add_ignored_while_pad_still_attached() {
    // A spurious device-added must not drop a working, still-attached
    // binding.
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    let mut m = ControllerManager::new(
        true,
        true,
        Some(fake_factory(vec![("pad".to_string(), 7)], Rc::clone(&log))),
    );
    let opens = log.borrow().opens.len();
    m.process_event(&InputEvent::ControllerAdded { device_index: 3 });
    assert_eq!(log.borrow().opens.len(), opens); // left the existing pad bound
    assert_eq!(m.instance_id(), Some(7));
    m.shutdown();
}

#[test]
fn test_binding_latches_to_first_event_id() {
    // A hot-plug can leave the opened id stale, so the pad's real events
    // arrive under a different id. The first real event's id is
    // authoritative: the manager adopts it and applies the event (triggers
    // come back to life).
    let mut m = ControllerManager::detached(true, true);
    m.bind_device(Box::new(FakePad::new(0)), "pad"); // freshly (re)opened, id provisional
    assert!(m.id_pending());
    m.process_event(&axis(ControllerAxis::TriggerLeft, 32767, 2));
    assert_eq!(m.instance_id(), Some(2)); // adopted the id the events actually carry
    assert!(!m.id_pending()); // latched
    assert!(m.brake_target() > 0.0); // ...and the event was applied
    m.shutdown();
}

#[test]
fn test_foreign_duplicate_button_is_not_forwarded() {
    // Once the binding is latched, a button under a *different* id (a
    // duplicate from a pad that enumerates twice) must not be forwarded to
    // the state, or it would fire the action a second time.
    let mut m = manager_with_pad(0); // already latched to id 0
                                     // The genuine press is forwarded...
    assert!(m.process_event(&button(ControllerButton::A, 0)));
    // ...its duplicate under id 2 is dropped, not forwarded.
    assert!(!m.process_event(&button(ControllerButton::A, 2)));
    m.shutdown();
}

#[test]
fn test_duplicate_button_down_not_forwarded() {
    // A pad that delivers a button twice (same id, no intervening release)
    // must forward only the first press to the state.
    let mut m = manager_with_pad(0);
    assert!(m.process_event(&button(ControllerButton::A, 0)));
    assert!(!m.process_event(&button(ControllerButton::A, 0)));
    // After a genuine release, the next press is fresh again.
    assert!(m.process_event(&button_up(ControllerButton::A, 0)));
    assert!(m.process_event(&button(ControllerButton::A, 0)));
    m.shutdown();
}

// `test_duplicate_button_down_does_not_double_toggle` is live in the driving
// section below: it needs a real drive on the stack to toggle an engine on.

#[test]
fn test_disabled_manager_ignores_controller() {
    let mut m = manager_with_pad(0);
    m.set_enabled(false);
    assert!(!m.active());
    m.process_event(&axis(ControllerAxis::TriggerRight, 32767, 0));
    assert_eq!(m.throttle(), 0.0);
    m.shutdown();
}

#[test]
fn test_disabled_manager_never_inits_subsystem() {
    // With controller support off, the subsystem must not be touched: no
    // enumeration, no pad bound.
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    let mut m = ControllerManager::new(
        false,
        true,
        Some(fake_factory(vec![("pad".to_string(), 7)], Rc::clone(&log))),
    );
    assert!(!m.subsystem_up()); // subsystem never initialized
    assert!(!m.connected());
    assert_eq!(log.borrow().inits, 0); // never went looking for a pad
    assert!(log.borrow().opens.is_empty());
    m.shutdown();
}

#[test]
fn test_enabling_inits_subsystem() {
    // Turning support on for the first time brings the subsystem up.
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    let mut m = ControllerManager::new(false, true, Some(fake_factory(vec![], Rc::clone(&log))));
    m.set_enabled(true);
    assert!(m.enabled());
    assert_eq!(log.borrow().inits, 1);
    assert!(m.subsystem_up());
    m.shutdown();
}

#[test]
fn test_disabling_tears_down_subsystem() {
    // Disabling releases the open pad and uninitializes the subsystem so no
    // controller handles or SDL state linger while support is off.
    let log = Rc::new(RefCell::new(FakeSdlLog::default()));
    let mut m = ControllerManager::new(
        true,
        true,
        Some(fake_factory(vec![("pad".to_string(), 0)], Rc::clone(&log))),
    );
    assert!(m.connected());
    m.set_enabled(false);
    assert!(!m.connected()); // handle released
    assert!(!m.subsystem_up()); // subsystem uninitialized
    assert_eq!(log.borrow().quits, 1);
    m.shutdown();
}

// -- menus -------------------------------------------------------------------

#[test]
fn test_menu_dpad_moves_and_adjusts() {
    use freight_fate::states::base::Menu;
    use freight_fate::states::main_menu::SettingsCategoryState;

    let mut app = TestApp::new();
    force_controller(&mut app);
    app.push_state(SettingsCategoryState::new("controls"));
    let index = |app: &TestApp| -> usize {
        let state = app.state().unwrap();
        let state = state.borrow();
        state
            .as_any()
            .downcast_ref::<SettingsCategoryState>()
            .unwrap()
            .menu()
            .index
    };
    let start_index = index(&app);
    app.dispatch_controller(&button(ControllerButton::DPadDown, 0));
    assert_eq!(index(&app), start_index + 1);
    // Move to the Units item and adjust it with D-pad right.
    {
        let state = app.state().unwrap();
        let mut state = state.borrow_mut();
        state
            .as_any_mut()
            .downcast_mut::<SettingsCategoryState>()
            .unwrap()
            .menu_mut()
            .index = 0;
    }
    let before = app.ctx.settings.imperial_units;
    app.dispatch_controller(&button(ControllerButton::DPadRight, 0));
    assert_ne!(app.ctx.settings.imperial_units, before);
    app.shutdown();
}

#[test]
fn test_setting_toggle_gates_controller() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    app.ctx.settings.controller_enabled = false;
    app.ctx.apply_controller();
    assert!(!app.ctx.controller.active());
    app.shutdown();
}

// -- driving -----------------------------------------------------------------
//
// `start_drive(app)` from `test_controller.py`, on the app's own stack: these
// go in through `App::dispatch_controller`, so the drive has to be the state
// the app hands the event to.

const DT: f64 = 1.0 / 60.0;

/// A quiet delivery drive, pushed and entered.
fn start_drive(app: &mut TestApp) -> SharedState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Pad", "Buffalo"));
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
    // `quiet_trip(driving)`: an empty road, no random hazards or inspections,
    // and a pinned sky -- an unseeded trip draws real weather, and an ice day
    // moves every advisory speed under these tests.
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

/// Run `f` on the drive behind `shared`.
fn with_drive<R>(shared: &SharedState, f: impl FnOnce(&mut DrivingState) -> R) -> R {
    let mut borrowed = shared.borrow_mut();
    let drive = borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .expect("the handle is a DrivingState");
    f(drive)
}

/// One frame of the real drive loop, the way the app's own loop runs it.
fn drive_frame(app: &mut TestApp, shared: &SharedState, dt: f64) {
    shared.borrow_mut().update(&mut app.ctx, dt);
    app.ctx.run_deferred();
}

#[test]
fn test_duplicate_button_down_does_not_double_toggle() {
    // Regression: a single RB+A press delivered twice in a frame must toggle
    // the engine exactly once, not on-then-off. Python spied on
    // `_toggle_engine`; the engine itself is the same evidence and is what the
    // driver actually hears.
    let mut app = TestApp::new();
    force_controller(&mut app);
    let shared = start_drive(&mut app);
    with_drive(&shared, |d| d.trip.truck.engine_on = false);

    app.dispatch_controller(&button(ControllerButton::RightShoulder, 0)); // hold modifier
    app.dispatch_controller(&button(ControllerButton::RightShoulder, 0)); // duplicate delivery
    app.dispatch_controller(&button(ControllerButton::A, 0)); // press
    app.dispatch_controller(&button(ControllerButton::A, 0)); // duplicate delivery

    // Toggled exactly once: on, and still on.
    assert!(with_drive(&shared, |d| d.trip.truck.engine_on));
    app.shutdown();
}

#[test]
fn test_analog_trigger_drives_throttle() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    let shared = start_drive(&mut app);
    app.dispatch_controller(&axis(ControllerAxis::TriggerRight, 32767, 0));
    for _ in 0..20 {
        app.ctx.controller.tick(0.016);
    }
    drive_frame(&mut app, &shared, DT);
    assert!(
        with_drive(&shared, |d| d.trip.truck.throttle) > 0.5,
        "{}",
        with_drive(&shared, |d| d.trip.truck.throttle)
    );
    app.shutdown();
}

#[test]
fn test_held_partial_trigger_does_not_machinegun_brake_sound() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    let log = app.record_audio();
    let shared = start_drive(&mut app);
    with_drive(&shared, |d| d.trip.truck.velocity_mps = 15.0);

    // A light, steady trigger position (~30%) held for many frames.
    app.dispatch_controller(&axis(
        ControllerAxis::TriggerLeft,
        (32767.0 * 0.30) as i16,
        0,
    ));
    for _ in 0..40 {
        app.ctx.controller.tick(DT);
        drive_frame(&mut app, &shared, DT);
    }

    // The brake is genuinely applied...
    assert!(
        with_drive(&shared, |d| d.trip.truck.brake) > 0.2,
        "{}",
        with_drive(&shared, |d| d.trip.truck.brake)
    );
    // ...but the hiss fires once, not every frame. The press cue is the clunk
    // bank when the licensed cuts are installed, the classic brake_air chirp
    // on a clean clone.
    let hisses: Vec<String> = log
        .borrow()
        .played
        .iter()
        .filter(|(key, _, _)| key == "vehicle/brake_air" || key.starts_with("vehicle/brake_clunk"))
        .map(|(key, _, _)| key.clone())
        .collect();
    assert!(hisses.len() <= 1, "{hisses:?}");
    app.shutdown();
}

#[test]
fn test_controller_info_buttons_speak() {
    // B speaks speed; RB+B speaks fuel; D-pad up reports the route and location.
    let mut app = TestApp::new();
    force_controller(&mut app);
    let _drive = start_drive(&mut app);

    app.dispatch_controller(&button(ControllerButton::B, 0));
    assert!(
        app.main_lines().iter().any(|t| t.contains("per hour")),
        "{:?}",
        app.main_lines()
    );

    app.dispatch_controller(&button_up(ControllerButton::B, 0)); // release before re-press
    app.clear_speech();
    app.dispatch_controller(&button(ControllerButton::RightShoulder, 0));
    app.dispatch_controller(&button(ControllerButton::B, 0));
    assert!(
        app.main_lines().iter().any(|t| {
            let t = t.to_lowercase();
            t.contains("fuel") || t.contains("range")
        }),
        "{:?}",
        app.main_lines()
    );

    app.dispatch_controller(&button_up(ControllerButton::B, 0));
    app.dispatch_controller(&button_up(ControllerButton::RightShoulder, 0));
    app.clear_speech();
    app.dispatch_controller(&button(ControllerButton::DPadUp, 0));
    let said = app.main_lines().last().cloned().unwrap_or_default();
    assert!(said.contains("percent there"), "{said}");
    assert!(said.contains(", toward "), "{said}");
    app.shutdown();
}

#[test]
fn test_controller_speed_control_handoff_status_adjustment_and_brake() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    let shared = start_drive(&mut app);
    // Python replaced `trip.speed_limit_at` with a mutable 65, then dropped it
    // to a 25 construction zone. A zone IS what that method reads, so the
    // handoff is arranged rather than patched: park the truck out on the
    // corridor under its own 65, then lay a work zone over it.
    with_drive(&shared, |d| {
        d.trip.position_mi = d.trip.total_miles() / 2.0;
        let at = d.trip.position_mi;
        let (limit, _) = d.trip.speed_limit_at(at);
        assert!((limit - 65.0).abs() < 0.01, "open-road limit is {limit}");
        d.trip.truck.start_engine();
        d.trip.truck.set_air_ready(false);
        d.trip.truck.transmission.gear = 10;
        d.trip.truck.velocity_mps = 26.8;
    });

    app.dispatch_controller(&button(ControllerButton::Y, 0));
    let cruise = with_drive(&shared, |d| d.cruise_mph).expect("Y engages cruise");
    assert!((cruise - 60.0).abs() <= 1.0, "{cruise}");

    with_drive(&shared, |d| {
        let at = d.trip.position_mi;
        d.trip
            .zones
            .push(Zone::new(at - 0.1, at + 5.0, 25.0, "construction"));
    });
    drive_frame(&mut app, &shared, DT);
    let keeper = with_drive(&shared, |d| d.keeper_mph).expect("the zone hands over to the keeper");
    assert!((keeper - 25.0).abs() < 0.01, "{keeper}");

    app.clear_speech();
    app.dispatch_controller(&button(ControllerButton::B, 0));
    let said = app.main_lines().last().cloned().unwrap_or_default();
    assert!(
        said.contains("Speed keeper holding 25 miles per hour"),
        "{said}"
    );
    assert!(
        said.contains("Open-road target 60 miles per hour."),
        "{said}"
    );

    app.dispatch_controller(&button_up(ControllerButton::B, 0));
    app.dispatch_controller(&button(ControllerButton::RightShoulder, 0));
    app.dispatch_controller(&button(ControllerButton::DPadRight, 0));
    let target =
        with_drive(&shared, |d| d.speed_control_target_mph).expect("the open-road target moved");
    assert!((target - 65.0).abs() <= 1.0, "{target}");

    app.dispatch_controller(&button(ControllerButton::Start, 0));
    assert!(
        app.ctx
            .state()
            .is_some_and(|s| s.borrow().as_any().is::<DrivingStatusState>()),
        "RB+Start opens the status screen"
    );
    app.ctx.pop_state();
    app.dispatch_controller(&button_up(ControllerButton::RightShoulder, 0));

    // The brake stands the whole session down.
    app.dispatch_controller(&axis(ControllerAxis::TriggerLeft, 32767, 0));
    for _ in 0..20 {
        app.ctx.controller.tick(DT);
    }
    drive_frame(&mut app, &shared, DT);
    assert!(!with_drive(&shared, |d| d.speed_control_armed));
    assert!(with_drive(&shared, |d| d.keeper_mph).is_none());
    app.shutdown();
}

#[test]
fn test_paused_speed_control_can_be_canceled_by_keyboard_or_controller() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    let shared = start_drive(&mut app);
    app.clear_speech();

    with_drive(&shared, |d| {
        d.restore_speed_control_session(&mut app.ctx, true, Some(47.0))
    });
    app.handle_event(&InputEvent::key(Key::K));
    assert!(!with_drive(&shared, |d| d.speed_control_armed));

    with_drive(&shared, |d| {
        d.restore_speed_control_session(&mut app.ctx, true, Some(47.0))
    });
    app.dispatch_controller(&button(ControllerButton::Y, 0));
    assert!(!with_drive(&shared, |d| d.speed_control_armed));

    let offs = app
        .main_lines()
        .iter()
        .filter(|t| *t == "Automatic speed control off.")
        .count();
    assert_eq!(offs, 2, "{:?}", app.main_lines());
    app.shutdown();
}

#[test]
fn test_controller_disconnect_pauses_driving() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    let shared = start_drive(&mut app);
    with_drive(&shared, |d| d.handle_controller_disconnect(&mut app.ctx));
    app.ctx.run_deferred();
    assert!(app
        .ctx
        .state()
        .is_some_and(|s| s.borrow().as_any().is::<PauseMenuState>()));
    app.shutdown();
}

#[test]
#[ignore = "needs an SDL event pump to fail; the headless app has none (the shell's poll guard is exercised by the windowed build)"]
fn test_event_pump_error_is_survived() {}

#[test]
fn test_plain_state_not_trapped_by_controller() {
    // A non-menu keyboard state (the update check screen in Python; a
    // plain Escape-to-dismiss screen here) must still be dismissable with
    // the controller via the base translation to key events.
    struct Dismissable;
    impl State for Dismissable {
        fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
            if let Some((Key::Escape, _, _)) = event.key_down() {
                ctx.pop_state();
            }
        }
    }
    let mut app = TestApp::new();
    force_controller(&mut app);
    app.push_state(Dismissable);
    app.push_state(Dismissable);
    let depth = app.states().len();
    app.dispatch_controller(&button(ControllerButton::B, 0)); // B -> Escape
    assert_eq!(app.states().len(), depth - 1);
    app.shutdown();
}

#[test]
fn test_hint_switches_with_active_device() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    // A controller button marks the controller active.
    app.dispatch_controller(&button(ControllerButton::DPadDown, 0));
    assert_eq!(app.ctx.control_hint("take_exit"), "D-pad down");
    // A keyboard press flips hints back to key names.
    app.ctx.controller.note_keyboard();
    assert_eq!(app.ctx.control_hint("take_exit"), "X");
    app.shutdown();
}

/// The controller disconnect line reaches the player from the loop.
#[test]
fn disconnect_speaks_and_reaches_the_state() {
    struct Noticing(Rc<RefCell<u32>>);
    impl State for Noticing {
        fn on_controller_disconnect(&mut self, _ctx: &mut GameContext) {
            *self.0.borrow_mut() += 1;
        }
    }
    let mut app = TestApp::new();
    force_controller(&mut app);
    let count = Rc::new(RefCell::new(0));
    app.push_state(Noticing(Rc::clone(&count)));
    app.dispatch_controller(&InputEvent::ControllerRemoved { instance_id: 0 });
    app.tick(1.0 / 60.0);
    assert_eq!(*count.borrow(), 1);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.starts_with("Controller disconnected.")));
    app.shutdown();
}

/// Rumble reaches the bound pad only while active and haptics are on.
#[test]
fn rumble_is_guarded_by_active_and_haptics() {
    let mut m = ControllerManager::detached(true, true);
    let pad = FakePad::new(0);
    let handle = pad.handle();
    m.bind_device(Box::new(pad), "pad");
    m.rumble.alert();
    m.tick(0.016);
    assert!(!handle.log.borrow().rumbles.is_empty());
    handle.log.borrow_mut().rumbles.clear();
    m.set_haptics_enabled(false);
    m.rumble.alert();
    m.tick(0.016);
    assert!(handle.log.borrow().rumbles.is_empty());
    m.shutdown();
}
