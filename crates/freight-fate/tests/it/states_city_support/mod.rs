//! Shared rigging for the `states_city*` integration tests: build a career
//! at a terminal on the headless app, drive its menus the way the Python
//! tests drove `app.state.handle_event(key_event(...))`, and read the rows
//! back.
#![allow(dead_code)]

use ff_core::models::jobs::Job;
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::app::GameContext;
use freight_fate::states::base::{InputEvent, Key, Menu, SimpleMenuState};
use freight_fate::states::city_pickup::{PickupFacilityState, PickupOptions};

/// Run `f` with the active state downcast to `T`.
pub fn with_state<T: 'static, R>(app: &TestApp, f: impl FnOnce(&T, &GameContext) -> R) -> R {
    let state = app.state().expect("a state on the stack");
    let state = state.borrow();
    let t = state
        .as_any()
        .downcast_ref::<T>()
        .unwrap_or_else(|| panic!("active state is not a {}", std::any::type_name::<T>()));
    f(t, &app.ctx)
}

/// Run `f` with the active state downcast mutably to `T`.
pub fn with_state_mut<T: 'static, R>(
    app: &mut TestApp,
    f: impl FnOnce(&mut T, &mut GameContext) -> R,
) -> R {
    let state = app.state().expect("a state on the stack");
    let mut borrowed = state.borrow_mut();
    let t = borrowed
        .as_any_mut()
        .downcast_mut::<T>()
        .unwrap_or_else(|| panic!("active state is not a {}", std::any::type_name::<T>()));
    let result = f(t, &mut app.ctx);
    drop(borrowed);
    app.ctx.run_deferred();
    result
}

/// Whether the active state is a `T`.
pub fn is<T: 'static>(app: &TestApp) -> bool {
    let state = app.state().expect("a state on the stack");
    let state = state.borrow();
    state.as_any().downcast_ref::<T>().is_some()
}

/// Whether a `T` is anywhere on the stack.
pub fn stack_has<T: 'static>(app: &TestApp) -> bool {
    app.states()
        .iter()
        .any(|state| state.borrow().as_any().downcast_ref::<T>().is_some())
}

/// Whether the active state is `states::city::todo_state(name)`.
pub fn is_placeholder(app: &TestApp, name: &str) -> bool {
    if !is::<SimpleMenuState>(app) {
        return false;
    }
    with_state::<SimpleMenuState, _>(app, |s, _| s.menu.title == name)
}

/// The rows of the active `T` menu, as spoken.
pub fn labels<T: Menu + 'static>(app: &TestApp) -> Vec<String> {
    with_state::<T, _>(app, |t, ctx| {
        t.menu()
            .items
            .iter()
            .map(|item| item.text(t, ctx))
            .collect()
    })
}

/// The rows of the active `T` menu with their help.
pub fn labels_and_help<T: Menu + 'static>(app: &TestApp) -> Vec<(String, String)> {
    with_state::<T, _>(app, |t, ctx| {
        t.menu()
            .items
            .iter()
            .map(|item| (item.text(t, ctx), item.help_text(t, ctx)))
            .collect()
    })
}

/// `state.build_items()` without entering the screen (the Python tests read
/// a freshly built row list off a state they never pushed).
pub fn built_labels<T: Menu + 'static>(app: &mut TestApp, state: &mut T) -> Vec<String> {
    let items = state.build_items(&mut app.ctx);
    items
        .iter()
        .map(|item| item.text(state, &app.ctx))
        .collect()
}

pub fn current_label<T: Menu + 'static>(app: &TestApp) -> String {
    with_state::<T, _>(app, |t, ctx| {
        let core = t.menu();
        core.items[core.index].text(t, ctx)
    })
}

pub fn current_help<T: Menu + 'static>(app: &TestApp) -> String {
    with_state::<T, _>(app, |t, ctx| t.current_help(ctx))
}

pub fn current_text<T: Menu + 'static>(app: &TestApp) -> String {
    with_state::<T, _>(app, |t, ctx| t.current_text(ctx))
}

pub fn index<T: Menu + 'static>(app: &TestApp) -> usize {
    with_state::<T, _>(app, |t, _| t.menu().index)
}

pub fn key(app: &mut TestApp, key: Key) {
    app.dispatch_to_state(&InputEvent::key(key));
}

/// Arrow down to the row containing `needle`.
pub fn move_to<T: Menu + 'static>(app: &mut TestApp, needle: &str) {
    let rows = labels::<T>(app).len();
    for _ in 0..=rows {
        if current_label::<T>(app).contains(needle) {
            return;
        }
        key(app, Key::Down);
    }
    panic!("no row containing {needle:?}; saw {:?}", labels::<T>(app));
}

/// Arrow down to the row containing `needle` and press Enter.
pub fn select<T: Menu + 'static>(app: &mut TestApp, needle: &str) {
    move_to::<T>(app, needle);
    key(app, Key::Return);
}

/// Activate the row containing `needle` without moving the cursor (the
/// Python `item.action()`).
pub fn activate<T: Menu + 'static>(app: &mut TestApp, needle: &str) {
    let rows = labels::<T>(app);
    let at = rows
        .iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("no row containing {needle:?}; saw {rows:?}"));
    with_state_mut::<T, _>(app, |t, ctx| {
        t.menu_mut().index = at;
        t.activate(ctx);
    });
}

/// Load a career parked at `city` onto the context.
pub fn career(app: &mut TestApp, name: &str, city: &str) {
    app.ctx.profile = Some(Profile::named_in(name, city));
}

/// The live profile on the context.
pub fn profile(app: &TestApp) -> &Profile {
    app.ctx.profile.as_ref().expect("a career is loaded")
}

pub fn profile_mut(app: &mut TestApp) -> &mut Profile {
    app.ctx.profile.as_mut().expect("a career is loaded")
}

/// Finish the active `TimedMessageState` (the Python `finish_timed_state`).
pub fn finish_timed_state(app: &mut TestApp) {
    use freight_fate::states::base::TimedMessageState;
    let remaining = with_state::<TimedMessageState, _>(app, |s, _| s.remaining);
    let state = app.state().expect("a timed state");
    {
        let mut borrowed = state.borrow_mut();
        borrowed.update(&mut app.ctx, remaining + 0.01);
    }
    app.ctx.run_deferred();
}

/// A pickup facility already checked in and loaded, the way the Python
/// tests reached the departure row (`PickupFacilityState(ctx, job,
/// checked_in=True, loaded=True)`).
pub fn loaded_pickup(app: &TestApp, job: Job) -> PickupFacilityState {
    PickupFacilityState::new(
        &app.ctx,
        job,
        PickupOptions {
            checked_in: true,
            loaded: true,
            ..PickupOptions::default()
        },
    )
}

/// Open the owner-operator buy-in from Business status and buy the cheapest
/// tractor it lists: the row asks once, the second press buys.
pub fn buy_in_cheapest(app: &mut TestApp) {
    use freight_fate::states::city::{BusinessStatusState, BuyInState};
    select::<BusinessStatusState>(app, "Owner-operator buy-in");
    let row = ff_core::data::world_models::py_capitalize(
        ff_core::models::buy_in::buy_in_tractors()[0].label,
    );
    select::<BuyInState>(app, &row);
    select::<BuyInState>(app, &row);
}
