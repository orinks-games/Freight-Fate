//! Shared rigging for the screens the drive pushes.
//!
//! Python's fixtures handed each screen `driving`, a plain reference to the
//! `DrivingState` under it. Here the drive lives on the stack as
//! `Rc<RefCell<dyn State>>` and every screen reaches it through a
//! [`DriveRef`], so a test that exercises one of these screens has to put a
//! real drive on the stack first ([`a_drive`]) and reach it back through
//! [`with_drive`] / [`drive_and_ctx`].

#![allow(dead_code)]

use std::rc::Rc;

use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::app::{share, GameContext, SharedState};
use freight_fate::states::base::{InputEvent, Key, Menu, MenuItem, Mods, State};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::DriveRef;

/// `_driving(app)`: a delivery drive on a real short corridor, pushed onto
/// the stack the way the game has it when any of these screens opens.
pub fn a_drive(app: &mut TestApp) -> SharedState {
    a_drive_between(app, "Buffalo", "Rochester", "Planner")
}

pub fn a_drive_between(
    app: &mut TestApp,
    origin: &str,
    destination: &str,
    driver: &str,
) -> SharedState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in(driver, origin));
    let route = world
        .supported_route(origin, destination, None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        origin,
        "company yard",
        destination,
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = format!("{destination} freight market");
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(99),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    // The bubble is its own suite's business; an empty road keeps these
    // deterministic (`driving_feature_helpers.quiet_trip`). The weather is
    // the other half of that helper: the trip seed is unseeded, so a drive
    // that does not pin the sky draws a real condition and an ice day caps
    // the safe speed under whatever the test is measuring.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    let shared = share(drive);
    // `should_enter = false`: entering a drive starts audio and speech these
    // tests are not about, and every screen here is pushed over a drive that
    // is already running.
    app.ctx.push_shared_with(Rc::clone(&shared), false);
    shared
}

/// The `DriveRef` a screen built over this drive would carry.
pub fn drive_ref(shared: &SharedState) -> DriveRef {
    DriveRef::of(shared)
}

/// Borrow the drive as its concrete type.
pub fn with_drive<R>(shared: &SharedState, f: impl FnOnce(&mut DrivingState) -> R) -> R {
    let mut borrowed = shared.borrow_mut();
    let drive = borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .expect("the shared state is a DrivingState");
    f(drive)
}

/// Borrow the drive and the context together, the way a push helper has them.
pub fn drive_and_ctx<R>(
    shared: &SharedState,
    app: &mut TestApp,
    f: impl FnOnce(&mut DrivingState, &mut GameContext) -> R,
) -> R {
    let mut borrowed = shared.borrow_mut();
    let drive = borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .expect("the shared state is a DrivingState");
    f(drive, &mut app.ctx)
}

/// A state on the stack that is not a drive.
struct NotADrive;

impl State for NotADrive {}

/// A drive handle that answers nothing, holding a state that is not a drive.
///
/// The miss this stands in for is the nested borrow: a shipped build reaches
/// `DriveRef::call` while something further up the stack already holds the
/// drive, gets `None`, and the screen has to decide what to show. That exact
/// condition cannot be watched on a bench, because it trips the
/// `debug_assert!` in `nested_borrow` and takes the test process with it (see
/// `test_reaching_for_a_drive_that_is_already_held_fails_loudly`, which pins
/// that half). This produces the same `None` from the same `call`, by the
/// other route it has, so what the screen does next can be pinned in every
/// run.
pub fn unreachable_drive() -> DriveRef {
    DriveRef::of(&share(NotADrive))
}

/// What a screen shows after a rebuild that could not reach the drive.
///
/// `already_showing` is what the screen built while it could still read the
/// drive -- the rows the player is looking at when the miss happens.
pub fn rows_with_the_drive_out_of_reach<M: Menu>(
    stranded: &mut M,
    already_showing: Vec<MenuItem<M>>,
    ctx: &mut GameContext,
) -> Vec<String> {
    stranded.menu_mut().items = already_showing;
    let rebuilt = stranded.build_items(ctx);
    rebuilt
        .iter()
        .map(|item| item.text(stranded, ctx))
        .collect()
}

/// Whether the active state is `T`.
pub fn top_is<T: 'static>(app: &TestApp) -> bool {
    app.ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<T>())
}

/// Run `f` on the active state as `T`.
pub fn with_top<T: 'static, R>(app: &TestApp, f: impl FnOnce(&mut T) -> R) -> R {
    let state = app.ctx.state().expect("a state on the stack");
    let mut borrowed = state.borrow_mut();
    let typed = borrowed
        .as_any_mut()
        .downcast_mut::<T>()
        .expect("the active state has the expected type");
    f(typed)
}

/// Run `f` on the active state as `T`, with the context.
pub fn with_top_ctx<T: 'static, R>(
    app: &mut TestApp,
    f: impl FnOnce(&mut T, &mut GameContext) -> R,
) -> R {
    let state = app.ctx.state().expect("a state on the stack");
    let mut borrowed = state.borrow_mut();
    let typed = borrowed
        .as_any_mut()
        .downcast_mut::<T>()
        .expect("the active state has the expected type");
    f(typed, &mut app.ctx)
}

/// `[item.text for item in state.build_items()]`.
pub fn build_labels<M: Menu>(state: &mut M, ctx: &mut GameContext) -> Vec<String> {
    let items = state.build_items(ctx);
    items.iter().map(|item| item.text(state, ctx)).collect()
}

/// `[item.text for item in state.items]` -- the rows as entered.
pub fn labels<M: Menu>(state: &M, ctx: &GameContext) -> Vec<String> {
    state
        .menu()
        .items
        .iter()
        .map(|item| item.text(state, ctx))
        .collect()
}

/// The freshly built row whose label starts with `prefix`.
pub fn row<M: Menu>(state: &mut M, ctx: &mut GameContext, prefix: &str) -> MenuItem<M> {
    let items = state.build_items(ctx);
    let found = items
        .iter()
        .find(|item| item.text(state, ctx).starts_with(prefix))
        .cloned();
    match found {
        Some(item) => item,
        None => panic!(
            "no row starting with {prefix:?}: {:?}",
            items
                .iter()
                .map(|item| item.text(state, ctx))
                .collect::<Vec<_>>()
        ),
    }
}

/// `next(i for i in state.items if ...).action()`.
pub fn activate<M: Menu>(state: &mut M, ctx: &mut GameContext, prefix: &str) {
    let item = row(state, ctx, prefix);
    (item.action)(state, ctx);
}

/// A plain key press.
pub fn key(k: Key) -> InputEvent {
    InputEvent::key(k)
}

pub fn typed(ch: char) -> InputEvent {
    InputEvent::typed(ch)
}

pub fn key_mods(k: Key, mods: Mods) -> InputEvent {
    InputEvent::key_mods(k, mods)
}

/// `sleep_stop(driving)` from `test_rest_stop_assist.py`.
pub fn sleep_stop(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Prairie View Rest Area", at_mi, "public_rest_area");
    stop.actions = ["park", "save", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    stop.exit_label = "exit 99".to_string();
    stop
}

/// `_stop(driving, name)` from `test_road_services.py`: a branded travel
/// centre with pumps and a break room.
pub fn travel_center(name: &str, at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new(name, at_mi, "travel_center");
    stop.actions = ["fuel", "break"].iter().map(|a| a.to_string()).collect();
    stop.parking = "limited".to_string();
    stop
}

/// The last thing the main channel said.
pub fn last(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

/// A delivery drive hauling a drop-yard trailer with a lamp out: the yard is
/// deterministic, so this walks facility ids until one hands over that box.
/// Returns the drive and the defect as an officer reads it.
pub fn a_drive_with_a_lamp_out(app: &mut TestApp) -> (SharedState, String) {
    let drive = a_drive(app);
    for i in 0..400 {
        let found = drive_and_ctx(&drive, app, |d, ctx| {
            d.job.origin_type = "cross_dock".to_string();
            d.job.origin_facility_id = format!("buffalo-cross-dock-{i}");
            d.hooked_trailer_defect(ctx)
                .filter(|defect| defect.contains("lamp"))
        });
        if let Some(defect) = found {
            return (drive, defect);
        }
    }
    panic!("no seeded drop yard produced a trailer with a lamp out");
}
