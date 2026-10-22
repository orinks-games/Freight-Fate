//! Hours of service at the wheel: the warnings the drive speaks, the rest
//! stop menu, a full overnight lot, the emergency shoulder, and what a blown
//! clock costs at an inspection.
//!
//! These are the `tests/test_hos.py` cases that drive a real `DrivingState`
//! and the screens it pushes. They spent the port as `#[ignore]`d stubs in
//! `crates/ff-core/src/sim/hos/tests.rs`, where they could never run at all:
//! `ff-core` cannot depend on the game crate, so `RestStopState`,
//! `ParkingFullState` and the pause menu are invisible from there. The clock
//! arithmetic they sat beside stays in `ff-core`; these live here.
//!
//! `start_drive(app)` -- new career, accept the assigned dispatch, depart --
//! is [`PlaytestHarness::start_delivery`], which walks the same menus.
//!
//! # What replaced the monkeypatches
//!
//! | Python | here |
//! |---|---|
//! | `monkeypatch.setattr(ctx, "say"/"say_event", stub)` | the harness records at `ctx.speech`, one rung lower |
//! | `hos.parking_is_full -> True`, `hos.shoulder_fine_due / shoulder_damage_due -> True` | [`a_full_lot_and_ticketed_mile`] searches the real rules for a mile and hour where all three really do fire |
//! | `park_at_first_stop(driving)` | [`sleep_stop_here`], which is the deterministic stop that helper injects when the drawn route has none |
//! | `park_away_from_stops(driving, ...)` | the road is left with the one stop BEHIND the truck, which is the condition that helper hunts for |

use ff_core::models::enforcement;
use ff_core::sim::hos;
use ff_core::sim::trip_models::{RoadStop, TripEvent, TripEventData, TripEventKind};
use ff_core::sim::weather::WeatherKind;
use freight_fate::controller::ControllerButton;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, Menu};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::HAZARD_MIN_REACTION_S;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::driving_rest_states::{
    ParkingFullState, RestStopState, ShoulderSleepConfirmationState,
};

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// `start_drive(app)` on a quiet road.
fn a_drive(name: &str) -> PlaytestHarness {
    a_drive_scaled(name, None)
}

/// [`a_drive`] with `settings.time_scale` set before the trip is built -- the
/// trip takes its pacing at construction, so a later assignment lasts a tick.
fn a_drive_scaled(name: &str, time_scale: Option<f64>) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    if let Some(scale) = time_scale {
        harness.app.ctx.settings.time_scale = scale;
    }
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|d, _| {
        // `quiet_trip(driving)`: an empty road and a pinned sky. An unseeded
        // trip draws fresh weather, and an ice day quietly caps the advisory
        // speeds these cases measure around.
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.zones.retain(|zone| zone.aadt.is_none());
        d.weather_mut().current = WeatherKind::Clear;
        // The origin yard may carry a turn-level street chain; none of these
        // cases is about the departure chain.
        d.departure_checked = true;
        d.truck_mut().set_air_ready(false);
    });
    harness.clear_speech();
    harness
}

/// The truck's own mile.
fn here(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.trip.position_mi)
}

/// `park_at_first_stop(driving)`.
///
/// The Python helper hunts the drawn route for a sleep-capable stop the truck
/// can actually open and injects a deterministic one when there is none. Which
/// route dispatch drew is not what any of these cases is about, so this always
/// takes the injected road: one travel center, right where the truck is.
fn sleep_stop_here(harness: &mut PlaytestHarness) -> RoadStop {
    let at_mi = here(harness).max(1.0);
    let mut stop = RoadStop::new("Test Travel Center", at_mi, "travel_center");
    stop.actions = ["park", "save", "fuel", "food", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.services = ["diesel", "food", "parking"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![staged];
        d.trip.position_mi = at_mi;
    });
    stop
}

/// `park_away_from_stops(driving, after_stop=stop)`: stopped clear of every
/// route point, with none ahead either.
///
/// The Python helper walks the route looking for such a gap and drops the
/// other stops when the drawn route has none. Leaving the one stop behind the
/// truck IS that condition, and it is the same every run.
fn park_away_from_stops(harness: &mut PlaytestHarness, after_stop: &RoadStop) {
    let position = after_stop.at_mi + 4.0;
    let stop = after_stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![stop];
        d.trip.position_mi = position;
        assert!(
            d.trip.nearest_stop_within(1.5).is_none(),
            "the truck is still on top of a route point"
        );
        assert!(
            d.upcoming_stop_with_action("sleep", 500.0).is_none(),
            "a sleep-capable stop is still ahead"
        );
    });
}

/// A mile where the lot really is full at `hour`, and where the shoulder
/// really does draw both a ticket and minor damage.
///
/// Python patched `hos.parking_is_full`, `hos.shoulder_fine_due` and
/// `hos.shoulder_damage_due` all to True. Each is a deterministic roll on
/// `(trip_seed, mile)` compared against its own chance, so the honest
/// arrangement is to put the stop on a mile where this seed really does roll
/// all three -- and to fail loudly if no such mile exists rather than pass on
/// a lot that has room.
fn a_full_lot_and_ticketed_mile(trip_seed: i64, hour: f64, from_mi: f64, to_mi: f64) -> f64 {
    let mut mi = from_mi;
    while mi < to_mi {
        if hos::parking_is_full(trip_seed, mi, hour, 0)
            && hos::shoulder_fine_due(trip_seed, mi)
            && hos::shoulder_damage_due(trip_seed, mi)
        {
            return mi;
        }
        mi += 0.1;
    }
    panic!(
        "no mile between {from_mi} and {to_mi} fills the lot AND draws both a shoulder ticket          and shoulder damage on seed {trip_seed}"
    );
}

/// Put the trip clock at `target` local hours, which is what the parking and
/// sleep rules read.
fn set_local_hour(harness: &mut PlaytestHarness, target: f64) -> f64 {
    // `local_hour` is `start_hour` plus the miles already driven and the
    // corridor's zone offset, all modulo a day: read the offset once at zero
    // and solve for the start hour that lands on the wanted clock.
    let base = harness.with_drive(|d, _| {
        d.trip.start_hour = 0.0;
        d.trip.local_hour()
    });
    let start = (target - base).rem_euclid(24.0);
    harness.with_drive(move |d, _| d.trip.start_hour = start);
    let hour = harness.with_drive(|d, _| d.trip.local_hour());
    assert!(
        (hour - target).abs() < 1e-6,
        "the trip clock did not land on {target}: {hour}"
    );
    hour
}

/// Every line said so far, both channels, in submission order.
fn spoken(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.speech().lines()
}

fn last(harness: &PlaytestHarness) -> String {
    spoken(harness).last().cloned().unwrap_or_default()
}

/// T while rolling plans a sleep stop, the purpose these drives are about.
fn press_t(harness: &mut PlaytestHarness) {
    harness.plan_stop("Sleep");
}

/// `driving._update_hours_and_fatigue(dt)`.
fn hours_frame(harness: &mut PlaytestHarness, dt: f64) {
    harness.advance_clock(dt);
    harness.with_drive(move |d, ctx| d.update_hours_and_fatigue(ctx, dt));
}

// -- the warnings the drive speaks ---------------------------------------------------

#[test]
fn test_hos_violation_speech_interrupts_but_threshold_warning_does_not() {
    let mut harness = a_drive("HOS Interrupt");
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness.app.ctx.settings.time_scale = 60.0;
    let drive_limit = hos::limits("realistic").expect("realistic has limits").0;
    harness.with_drive(|d, _| {
        d.trip.time_scale = 60.0;
        d.truck_mut().velocity_mps = 25.0; // past 50 mph: full compression
    });

    {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        // Less left than the drive needs, so the countdown has a reason
        // to speak.
        p.hos.driving_min = drive_limit - 31.0;
        p.hos.duty_min = p.hos.driving_min;
        p.hos.since_break_min = 0.0;
    }
    harness.clear_speech();
    hours_frame(&mut harness, 1.0);
    let calls = harness.app.event_calls();
    let warning = calls
        .iter()
        .find(|(text, _)| text.starts_with("Hours of service: 30 minutes"))
        .unwrap_or_else(|| panic!("the countdown was never spoken: {calls:?}"));
    assert!(!warning.1, "the countdown must not cut the driver off");

    {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        p.hos.driving_min = drive_limit - 0.5;
        p.hos.duty_min = p.hos.driving_min;
        p.hos.since_break_min = 0.0;
        p.hos.warned.clear();
    }
    harness.clear_speech();
    hours_frame(&mut harness, 1.0);
    // Python read `spoken[-1]` because its stub replaced `ctx.say_event`
    // outright, so the pacer never ran. Here it does, and cutting the
    // countdown off mid-sentence requeues it BEHIND the violation -- the
    // interrupted line comes back rather than being lost. So the violation is
    // looked up by name instead of by position.
    let calls = harness.app.event_calls();
    let violation = calls
        .iter()
        .find(|(text, _)| text.starts_with("Hours of service violation:"))
        .unwrap_or_else(|| panic!("the violation was never spoken: {calls:?}"));
    assert!(violation.1, "a violation interrupts");
}

#[test]
fn test_severe_fatigue_drift_warning_is_urgent() {
    let mut harness = a_drive("Drowsy");
    harness.app.ctx.profile.as_mut().expect("a career").fatigue = hos::FATIGUE_SEVERE;
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 20.0);
    harness.clear_speech();

    hours_frame(&mut harness, 1.0);

    let warning = harness
        .app
        .event_calls()
        .last()
        .cloned()
        .expect("the drowsiness warning is spoken");
    assert!(warning.0.starts_with("Dangerously drowsy"), "{warning:?}");
    assert!(warning.1, "dangerous drowsiness interrupts");
}

#[test]
fn test_fatigued_driver_gets_a_shorter_hazard_window() {
    let mut harness = a_drive_scaled("Tired Reactions", Some(20.0));
    let hazard = |deadline_s: f64| TripEvent {
        kind: TripEventKind::Hazard,
        message: "Brake now!".into(),
        data: TripEventData {
            deadline_s: Some(deadline_s),
            ..Default::default()
        },
    };

    harness.app.ctx.profile.as_mut().expect("a career").fatigue = 0.0;
    let event = hazard(6.0);
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));
    let fresh = harness
        .read_drive(|d| d.hazard_deadline)
        .expect("a hazard deadline");

    harness.app.ctx.profile.as_mut().expect("a career").fatigue = 100.0;
    let event = hazard(6.0);
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));
    let tired = harness
        .read_drive(|d| d.hazard_deadline)
        .expect("a hazard deadline");
    assert!(approx(tired, fresh - 2.4), "{tired} vs {fresh}");

    // ...but never below the floor no human reacts under, whatever the rolled
    // slack says: a drowsy driver reacts late, not instantly.
    let event = hazard(1.5);
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));
    let floor = harness.with_drive(|d, _| {
        d.hazard_deadline.expect("a hazard deadline") - d.aeb_engage_s(d.hazard_target_mph(None))
    });
    assert!(approx(floor, HAZARD_MIN_REACTION_S), "{floor}");
}

// -- the rest stop menu ---------------------------------------------------------------

#[test]
fn test_rest_stop_menu_break_and_sleep() {
    let mut harness = a_drive("Rest Menu");
    sleep_stop_here(&mut harness);
    {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        p.hos.drive(490.0); // past the break rule
        p.fatigue = 50.0;
    }

    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
    let rows = harness.menu_labels();
    assert!(
        rows.iter().any(|row| row == "Take a 30-minute break"),
        "{rows:?}"
    );
    assert!(rows.iter().any(|row| row == "Sleep 10 hours"), "{rows:?}");

    let minutes_before = harness.read_drive(|d| d.trip.game_minutes);
    harness.select_menu_item("Take a 30-minute break");
    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        minutes_before + 30.0
    ));
    {
        let p = harness.app.ctx.profile.as_ref().expect("a career");
        assert_eq!(p.hos.since_break_min, 0.0);
        assert!(approx(p.fatigue, 15.0), "{}", p.fatigue);
    }

    // The break is the first half hour of the ten.
    harness.select_menu_item("Sleep 9.5 hours more to finish a 10-hour reset");
    harness.select_menu_item("Sleep 9.5 hours more to finish a 10-hour reset");
    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        minutes_before + 600.0
    ));
    {
        let p = harness.app.ctx.profile.as_ref().expect("a career");
        assert_eq!(p.hos.driving_min, 0.0);
        assert_eq!(p.hos.duty_min, 0.0);
        assert_eq!(p.fatigue, 0.0);
    }

    harness.key(InputEvent::key(Key::Escape));
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn test_food_and_coffee_break_boosts_alertness_without_resetting_break_rule() {
    let mut harness = a_drive("Coffee");
    sleep_stop_here(&mut harness);
    {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        p.hos.drive(100.0);
        p.fatigue = 55.0;
    }

    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
    let help = harness.with_state::<RestStopState, _>(|state, ctx| {
        let items = state.build_items(ctx);
        items
            .iter()
            .find(|item| item.text(state, ctx) == "Food and coffee break")
            .map(|item| item.help_text(state, ctx))
            .expect("the coffee row")
    });
    assert!(help.contains("Coffee eases fatigue a little"), "{help}");
    assert!(
        help.contains("does not satisfy the 30-minute break rule"),
        "{help}"
    );

    let minutes_before = harness.read_drive(|d| d.trip.game_minutes);
    harness.clear_speech();
    harness.select_menu_item("Food and coffee break");

    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        minutes_before + 15.0
    ));
    {
        let p = harness.app.ctx.profile.as_ref().expect("a career");
        assert!(
            approx(p.hos.since_break_min, 100.0),
            "{}",
            p.hos.since_break_min
        );
        assert!(approx(p.fatigue, 47.0), "{}", p.fatigue);
    }
    let said = last(&harness);
    assert!(said.contains("Eases fatigue"), "{said}");
    assert!(
        said.contains("does not count as your 30-minute break"),
        "{said}"
    );
}

#[test]
fn test_sleep_capable_stop_offers_sleeper_split_choices() {
    let mut harness = a_drive("Split Choices");
    sleep_stop_here(&mut harness);

    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
    let rows = harness.menu_labels();
    for hours in [2, 3, 7, 8] {
        assert!(
            rows.contains(&format!("Sleep {hours} hours in sleeper berth")),
            "{rows:?}"
        );
    }
    assert!(rows.contains(&"Sleep 10 hours".to_string()), "{rows:?}");
    let back_help = harness.with_state::<RestStopState, _>(|state, ctx| {
        let items = state.build_items(ctx);
        items
            .iter()
            .find(|item| item.text(state, ctx) == "Back to the road")
            .map(|item| item.help_text(state, ctx))
            .expect("the row that leaves the stop")
    });
    assert!(!back_help.is_empty(), "the way out must say what it does");
}

#[test]
fn test_split_sleeper_rest_action_advances_clock_and_speaks_status() {
    let mut harness = a_drive("Split Rest");
    sleep_stop_here(&mut harness);
    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
    let before = harness.read_drive(|d| d.trip.game_minutes);
    {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        p.achievements.retain(|id| id != "slept_on_route");
    }
    harness.clear_speech();

    harness.select_menu_item("Sleep 8 hours in sleeper berth");
    harness.select_menu_item("Sleep 8 hours in sleeper berth");

    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        before + 480.0
    ));
    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .hos
            .status,
        "sleeper_berth"
    );
    // Python watched a monkeypatched `award_achievement` to pin that the badge
    // lands AFTER the status line. There is no such seam here, so the same
    // order is read off what the player hears: the split status first, the
    // badge announcement after it.
    let lines = spoken(&harness);
    let status_at = lines
        .iter()
        .position(|line| line.contains("Sleeper split open"))
        .unwrap_or_else(|| panic!("no pending-split status line: {lines:#?}"));
    let badge_at = lines
        .iter()
        .position(|line| line.contains("New achievement!"))
        .unwrap_or_else(|| panic!("the sleep badge was never announced: {lines:#?}"));
    assert!(badge_at > status_at, "{lines:#?}");
    assert!(harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements
        .iter()
        .any(|id| id == "slept_on_route"));

    harness.clear_speech();
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(300.0);
    harness.select_menu_item("Sleep 2 hours in sleeper berth");
    harness.select_menu_item("Sleep 2 hours in sleeper berth");

    let completed = spoken(&harness)
        .into_iter()
        .find(|line| line.contains("Sleeper split credited"))
        .unwrap_or_else(|| {
            panic!(
                "the completed split was never spoken: {:#?}",
                spoken(&harness)
            )
        });
    assert!(completed.contains("of driving left"), "{completed}");
    assert!(completed.contains("14-hour window"), "{completed}");
}

/// The long half of a split stops the duty window while it runs. A tester
/// with 7 hours of duty on the clock slept 8 in the berth and woke to a
/// closed window (2026-09-11); now the window waits, and the wake-up line
/// says so.
#[test]
fn test_long_sleeper_period_pauses_duty_window_and_says_so() {
    let mut harness = a_drive("Split Pause");
    sleep_stop_here(&mut harness);
    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(420.0);
    // The drive so far already put some duty on the clock; the window must
    // read exactly the same after the sleep as before it.
    let before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .clone();
    harness.clear_speech();

    harness.select_menu_item("Sleep 8 hours in sleeper berth");
    harness.select_menu_item("Sleep 8 hours in sleeper berth");

    let hos = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .clone();
    assert!(
        approx(hos.duty_min, before.duty_min),
        "duty window ran on: {} -> {}",
        before.duty_min,
        hos.duty_min
    );
    assert!(
        approx(hos.driving_min, before.driving_min),
        "{}",
        hos.driving_min
    );
    assert!(!hos.in_violation("realistic"));
    let woke = spoken(&harness)
        .into_iter()
        .find(|line| line.contains("You slept 8 hours"))
        .unwrap_or_else(|| panic!("no wake-up line: {:#?}", spoken(&harness)));
    let left = ff_core::pyfmt::fmt_f((14.0 * 60.0 - before.duty_min) / 60.0, 1);
    assert!(
        woke.contains(&format!(
            "your duty window paused while you slept. It closes in {left} hours"
        )),
        "{woke}"
    );
    assert!(woke.contains("Sleeper split open"), "{woke}");
}

#[test]
fn test_sleeping_shuts_down_a_running_engine() {
    // A truck must not idle through a 10-hour sleep (issue #40): sleeping
    // kills a running engine, says so, and stays quiet when it was already off.
    let mut harness = a_drive("Engine Off");
    sleep_stop_here(&mut harness);
    harness.with_drive(|d, _| {
        d.truck_mut().set_air_ready(true);
        d.truck_mut().start_engine();
    });
    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
    harness.clear_speech();

    harness.select_menu_item("Sleep 10 hours");
    harness.select_menu_item("Sleep 10 hours");

    let cold_start_psi = harness.read_drive(|d| d.trip.truck.specs.air_cold_start_psi);
    harness.with_drive(|d, _| {
        assert!(!d.trip.truck.engine_on);
        assert!(approx(d.trip.truck.air_pressure_psi(), cold_start_psi));
        assert!(d.trip.truck.air_low_warning());
        assert!(!d.trip.truck.air_ready());
    });
    let lines = spoken(&harness);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("You shut down the engine.")),
        "{lines:#?}"
    );
    let wake = lines
        .iter()
        .find(|l| l.contains("You slept 10 hours"))
        .unwrap_or_else(|| panic!("no wake line: {lines:#?}"));
    assert!(wake.contains("Engine off, air 55 psi."), "{wake}");

    harness.clear_speech();
    harness.select_menu_item("Sleep 10 hours");
    assert!(
        !spoken(&harness)
            .iter()
            .any(|l| l.contains("shut down the engine")),
        "an already-dead engine is not shut down twice: {:#?}",
        spoken(&harness)
    );

    harness.select_menu_item("Back to the road");
    assert!(harness.state_is::<DrivingState>());
    harness.press_key(Key::E, Some('e'));
    assert!(harness.read_drive(|d| d.trip.truck.engine_on));
    for _ in 0..200 {
        harness.advance_clock(0.2);
        harness.with_drive(|d, ctx| d.update_frame(ctx, 0.2));
        if harness.read_drive(|d| d.trip.truck.air_ready()) {
            break;
        }
    }
    assert!(harness.read_drive(|d| d.trip.truck.air_ready()));
    harness.press_key(Key::P, Some('p'));
    assert!(!harness.read_drive(|d| d.trip.truck.parking_brake));
}

#[test]
fn test_break_only_stop_always_offers_emergency_lot_sleep() {
    let mut harness = a_drive("Lot Sleep"); // fresh hours, not tired
    let at_mi = here(&harness);

    // A break/fuel stop (no sleeper) still offers a lot sleep -- you can
    // always choose to sleep, even with hours to spare.
    let mut break_only = RoadStop::new("Roadside Rest", at_mi, "rest_area");
    break_only.actions = ["break", "fuel"].iter().map(|a| a.to_string()).collect();
    break_only.parking = "day_only".to_string();
    let rows = rest_rows(&mut harness, break_only);
    assert!(
        rows.contains(&"Sleep 10 hours in the lot".to_string()),
        "{rows:?}"
    );
    assert!(
        !rows.contains(&"Emergency sleep in the lot".to_string()),
        "{rows:?}"
    );
    assert!(!rows.contains(&"Sleep 10 hours".to_string()), "{rows:?}");
    assert!(
        !rows.iter().any(|row| row.contains("sleeper berth")),
        "{rows:?}"
    );

    // A sleeper stop offers the full sleep instead, not the lot fallback.
    let mut sleeper = RoadStop::new("Big Truck Stop", at_mi, "truck_stop");
    sleeper.actions = ["break", "fuel", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    sleeper.parking = "overnight".to_string();
    let rows = rest_rows(&mut harness, sleeper);
    assert!(rows.contains(&"Sleep 10 hours".to_string()), "{rows:?}");
    assert!(
        !rows.contains(&"Emergency sleep in the lot".to_string()),
        "{rows:?}"
    );
}

/// `[i.text for i in RestStopState(ctx, driving, stop).build_items()]`.
fn rest_rows(harness: &mut PlaytestHarness, stop: RoadStop) -> Vec<String> {
    use freight_fate::states::driving_menu_states::DriveRef;
    let drive = harness.shared_driving().expect("a drive on the stack");
    let mut state = RestStopState::with_drive(DriveRef::of(&drive), stop, false);
    let items = state.build_items(&mut harness.app.ctx);
    items
        .iter()
        .map(|item| item.text(&state, &harness.app.ctx))
        .collect()
}

#[test]
fn test_parking_never_full_during_the_day() {
    let mut harness = a_drive("Daylight Lot");
    sleep_stop_here(&mut harness);
    let hour = harness.read_drive(|d| d.trip.local_hour());
    assert!(
        (4.0..20.0).contains(&hour),
        "a career starts in daylight; this one started at {hour}"
    );

    press_t(&mut harness); // the lot has room at this hour

    assert!(harness.state_is::<RestStopState>());
}

// -- the full lot and the shoulder -------------------------------------------------------

#[test]
fn test_full_parking_offers_drive_on_and_shoulder() {
    // A seeded corridor rather than whatever dispatch drew: the lot, the
    // ticket and the damage are three independent rolls on `(trip_seed,
    // mile)`, and a 30-mile assigned run does not always contain a mile that
    // wins all three. This one is long enough that it always does, and it is
    // the same road every run.
    use freight_fate::playtest::harness::RouteSetup;
    let mut harness = PlaytestHarness::new();
    harness.start_route(
        "Chicago",
        "Indianapolis",
        RouteSetup::seeded(7).named("Full Lot"),
    );
    harness.with_drive(|d, _| {
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.zones.retain(|zone| zone.aadt.is_none());
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.truck_mut().set_air_ready(false);
    });
    let seed = harness.read_drive(|d| d.trip_seed);
    // The real rules fill a lot only in the small hours: park the clock at the
    // deepest crunch and put the stop on a mile where this seed rolls all three.
    let hour = set_local_hour(&mut harness, 3.5);
    let total = harness.read_drive(|d| d.trip.total_miles());
    let at_mi = a_full_lot_and_ticketed_mile(seed, hour, 1.0, total - 1.0);
    let mut stop = RoadStop::new("Test Travel Center", at_mi, "travel_center");
    stop.actions = ["park", "save", "fuel", "food", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![staged];
        d.trip.position_mi = at_mi;
        d.truck_mut().velocity_mps = 0.0;
    });
    harness.clear_speech();

    press_t(&mut harness);
    assert!(harness.state_is::<ParkingFullState>());
    let rows = harness.menu_labels();
    assert!(
        rows.iter().any(|row| row.starts_with("Drive on")),
        "{rows:?}"
    );
    assert!(rows.iter().any(|row| row.contains("shoulder")), "{rows:?}");

    harness.select_menu_item("Park on the shoulder and sleep");
    assert!(harness.state_is::<ShoulderSleepConfirmationState>());
    let said = last(&harness);
    assert!(said.contains("Emergency shoulder sleep:"), "{said}");
    assert!(
        said.contains("possible") || said.contains("may be ticketed"),
        "{said}"
    );

    // shoulder parking: HOS reset, fatigue floor 30, deadline kept counting
    let money_before = {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        p.hos.drive(700.0);
        p.fatigue = 95.0;
        p.money()
    };
    let damage_before = harness_damage(&harness);
    let minutes_before = harness.read_drive(|d| d.trip.game_minutes);
    let construction = harness.with_drive(|d, _| d.trip.in_construction_zone());
    harness.clear_speech();
    harness.select_menu_item("Sleep on the shoulder anyway");

    assert!(harness.state_is::<DrivingState>());
    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        minutes_before + 600.0
    ));
    {
        let p = harness.app.ctx.profile.as_ref().expect("a career");
        assert_eq!(p.hos.driving_min, 0.0);
        assert_eq!(p.fatigue, hos::FATIGUE_SHOULDER_FLOOR);
        // A clean career parked clear of roadwork pays the base amount; the
        // helper is asked so a rebalance moves one number, not this test.
        let expected = enforcement::citation_fine(hos::SHOULDER_FINE, 0, construction, None);
        assert!(approx(p.money(), money_before - expected), "{}", p.money());
        assert!(p.active_trip.is_some());
    }
    assert!(approx(
        harness_damage(&harness),
        damage_before + hos::SHOULDER_DAMAGE_PCT
    ));
    let wake = spoken(&harness)
        .into_iter()
        .find(|line| line.contains("sleep poorly on the shoulder"))
        .unwrap_or_else(|| panic!("no shoulder wake line: {:#?}", spoken(&harness)));
    assert!(wake.contains("Engine off, air 55 psi."), "{wake}");
    assert!(
        !wake.contains("Back to the road"),
        "there is no stop menu to go back to: {wake}"
    );
}

fn harness_damage(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.trip.truck.damage_pct)
}

#[test]
fn test_emergency_shoulder_sleep_pause_menu_constraints() {
    let mut harness = a_drive("Pause Shoulder");
    let stop = sleep_stop_here(&mut harness);
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(500.0);
    assert!(
        harness
            .with_drive(|d, ctx| d.emergency_shoulder_sleep_reason(ctx))
            .is_none(),
        "a route point is right here; its own menu is the answer"
    );

    park_away_from_stops(&mut harness, &stop);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 15.0);
    assert!(
        harness
            .with_drive(|d, ctx| d.emergency_shoulder_sleep_reason(ctx))
            .is_none(),
        "you cannot sleep while rolling"
    );

    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);
    let reason = harness
        .with_drive(|d, ctx| d.emergency_shoulder_sleep_reason(ctx))
        .expect("a reason once stopped and out of hours");
    assert!(
        reason.contains("past your hours-of-service limit"),
        "{reason}"
    );

    harness.press_key(Key::Escape, None);
    assert!(harness.state_is::<PauseMenuState>());
    let rows = harness.menu_labels();
    assert!(
        rows.iter().any(|row| row == "Emergency shoulder sleep"),
        "{rows:?}"
    );

    harness.clear_speech();
    harness.select_menu_item("Emergency shoulder sleep");
    assert!(harness.state_is::<ShoulderSleepConfirmationState>());
    let said = spoken(&harness)
        .into_iter()
        .find(|line| line.contains("Emergency shoulder sleep:"))
        .unwrap_or_default();
    assert!(
        said.contains("HOS reset if enforced"),
        "{:?}",
        spoken(&harness)
    );
    assert!(said.contains("light damage possible"), "{said}");
    assert_eq!(
        harness.focused_label().expect("a focused row"),
        "Cancel and keep looking for a safe stop"
    );
    let intro = harness.with_state::<ShoulderSleepConfirmationState, _>(|state, _| {
        Menu::menu(state).intro_help.clone()
    });
    assert!(intro.contains("Escape cancels"), "{intro}");
    assert!(!intro.contains("road"), "{intro}");
}

#[test]
fn test_t_opens_roadside_sleep_confirmation_at_safe_stop() {
    // Python parametrized over (0.0 mph, Return) and (0.5 mph, Escape).
    for (speed_mph, cancel) in [(0.0, Key::Return), (0.5, Key::Escape)] {
        let mut harness = a_drive("Roadside Sleep");
        let stop = sleep_stop_here(&mut harness);
        park_away_from_stops(&mut harness, &stop);
        let planned_key = stop.key();
        harness.with_drive(move |d, _| {
            d.truck_mut().velocity_mps = speed_mph * MPS_PER_MPH;
            d.truck_mut().throttle = 0.4;
            d.trip.planned_stop_key = Some(planned_key.clone());
            d.selected_stop_key = Some(planned_key);
            d.selected_stop_assist_armed = true;
        });
        harness.clear_speech();

        press_t(&mut harness);

        assert!(
            harness.state_is::<ShoulderSleepConfirmationState>(),
            "at {speed_mph} mph"
        );
        assert!(harness
            .focused_label()
            .expect("a focused row")
            .starts_with("Cancel"));
        let said = last(&harness);
        assert!(said.contains("poor rest"), "{said}");
        assert!(said.contains("light damage possible"), "{said}");
        harness.with_drive(|d, _| {
            assert!(d.trip.planned_stop_key.is_some());
            assert!(d.selected_stop_key.is_some());
            assert_eq!(d.trip.truck.velocity_mps, 0.0);
            assert_eq!(d.trip.truck.throttle, 0.0);
            assert_eq!(d.trip.truck.brake, 1.0);
            assert!(d.trip.truck.parking_brake);
        });

        harness.clear_speech();
        harness.key(InputEvent::key(cancel));
        assert!(harness.state_is::<DrivingState>());
        let said = last(&harness);
        assert!(
            said.starts_with("Shoulder sleep canceled. Back on the road."),
            "{said}"
        );
        assert!(said.contains("Parking brake set"), "{said}");
        assert!(said.contains("P releases it"), "{said}");
        drop(harness);
    }
}

#[test]
fn test_t_plans_rest_instead_of_opening_roadside_sleep_while_moving() {
    // Python parametrized over three rolling speeds either side of the
    // docking threshold.
    for speed_mph in [0.5001, 1.0, 3.0] {
        let mut harness = a_drive("Rolling Rest");
        let stop = sleep_stop_here(&mut harness);
        park_away_from_stops(&mut harness, &stop);
        harness.with_drive(move |d, _| d.truck_mut().velocity_mps = speed_mph * MPS_PER_MPH);
        let minutes_before = harness.read_drive(|d| d.trip.game_minutes);
        harness.clear_speech();

        press_t(&mut harness);

        assert!(harness.state_is::<DrivingState>(), "at {speed_mph} mph");
        // Python allowed either the plan or the decline, because which one it
        // got depended on the drawn route. This road has its one stop behind
        // the truck, so the decline is the only honest answer -- and what T
        // must never do while rolling is open roadside sleep.
        let said = last(&harness);
        assert_eq!(said, "No sleep stop ahead. Shoulder sleep available.");
        assert_eq!(harness.read_drive(|d| d.trip.game_minutes), minutes_before);
        assert!(!harness.read_drive(|d| d.trip.truck.parking_brake));
        drop(harness);
    }
}

#[test]
fn test_parking_brake_settles_walking_pace_before_pause() {
    let mut harness = a_drive("Walking Pace");
    let stop = sleep_stop_here(&mut harness);
    // The lot has room at this hour: `parking_is_full` never fires before 8 PM
    // (Python monkeypatched it to False for the same reason).
    let hour = harness.read_drive(|d| d.trip.local_hour());
    assert!((4.0..20.0).contains(&hour), "{hour}");
    assert!(!hos::parking_is_full(
        harness.read_drive(|d| d.trip_seed),
        stop.at_mi,
        hour,
        stop.parking_spaces
    ));
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 1.0 * MPS_PER_MPH);

    harness.press_key(Key::P, Some('p'));
    harness.with_drive(|d, _| {
        assert_eq!(d.trip.truck.velocity_mps, 0.0);
        assert!(d.trip.truck.parking_brake);
    });

    harness.press_key(Key::Escape, None);
    assert!(harness.state_is::<PauseMenuState>());
    assert!(
        !harness
            .menu_labels()
            .iter()
            .any(|row| row == "Emergency shoulder sleep"),
        "a route point is right here: {:?}",
        harness.menu_labels()
    );

    harness.key(InputEvent::key(Key::Escape));
    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
    harness.with_drive(|d, _| {
        assert_eq!(d.trip.truck.velocity_mps, 0.0);
        assert_eq!(d.trip.truck.brake, 1.0);
        assert!(d.trip.truck.parking_brake);
    });
}

#[test]
fn test_shoulder_sleep_revalidates_stop_and_unwinds_without_stale_pause_speech() {
    let mut harness = a_drive("Revalidate");
    let stop = sleep_stop_here(&mut harness);
    park_away_from_stops(&mut harness, &stop);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);
    harness.press_key(Key::Escape, None);
    assert!(harness.state_is::<PauseMenuState>());
    harness.select_menu_item("Emergency shoulder sleep");
    assert!(harness.state_is::<ShoulderSleepConfirmationState>());

    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.5001 * MPS_PER_MPH);
    let minutes_before = harness.read_drive(|d| d.trip.game_minutes);
    harness.clear_speech();
    harness.select_menu_item("Sleep on the shoulder anyway");
    assert!(harness.state_is::<ShoulderSleepConfirmationState>());
    assert_eq!(harness.read_drive(|d| d.trip.game_minutes), minutes_before);
    assert!(
        last(&harness)
            .to_lowercase()
            .contains("complete stop first"),
        "{}",
        last(&harness)
    );

    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);
    harness.clear_speech();
    harness.select_menu_item("Sleep on the shoulder anyway");
    assert!(harness.state_is::<DrivingState>());
    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        minutes_before + hos::SLEEP_MIN
    ));
    harness.with_drive(|d, _| {
        assert_eq!(d.trip.truck.velocity_mps, 0.0);
        assert!(d.trip.truck.parking_brake);
    });
    assert!(
        !spoken(&harness).iter().any(|line| line == "Paused."),
        "the unwound pause menu must not speak on its way out: {:#?}",
        spoken(&harness)
    );
    assert!(
        last(&harness).contains("Engine off, air"),
        "{}",
        last(&harness)
    );
}

#[test]
fn test_controller_rest_binding_opens_roadside_sleep_confirmation() {
    let mut harness = a_drive("Pad Rest");
    let stop = sleep_stop_here(&mut harness);
    park_away_from_stops(&mut harness, &stop);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);
    harness.app.ctx.controller.modifier = true;

    let event = InputEvent::button(ControllerButton::DPadDown);
    harness.with_drive(move |d, ctx| d.handle_controller_event(ctx, &event));

    assert!(harness.state_is::<ShoulderSleepConfirmationState>());
}

#[test]
fn test_hos_off_still_allows_fatigue_emergency_shoulder_sleep() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.hos_mode = "debug_off".to_string();
    harness.start_delivery(StartDelivery::named("HOS Off"));
    harness.with_drive(|d, _| {
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.zones.retain(|zone| zone.aadt.is_none());
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.truck_mut().set_air_ready(false);
    });
    let stop = sleep_stop_here(&mut harness);
    park_away_from_stops(&mut harness, &stop);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);

    // Stopped with no POI nearby: shoulder sleep is always an option now,
    // even rested and with HOS enforcement off -- you can choose to rest.
    harness.app.ctx.profile.as_mut().expect("a career").fatigue = 20.0;
    let reason = harness
        .with_drive(|d, ctx| d.emergency_shoulder_sleep_reason(ctx))
        .expect("shoulder sleep is offered stopped and clear of stops");
    assert!(reason.contains("pull over and rest"), "{reason}");

    // Severe fatigue escalates the wording but it was already available.
    harness.app.ctx.profile.as_mut().expect("a career").fatigue = hos::FATIGUE_SEVERE;
    let reason = harness
        .with_drive(|d, ctx| d.emergency_shoulder_sleep_reason(ctx))
        .expect("severe fatigue still offers it");
    assert!(reason.contains("Fatigue is severe"), "{reason}");

    // Moving, it is not offered -- you cannot sleep while rolling.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 12.0);
    assert!(harness
        .with_drive(|d, ctx| d.emergency_shoulder_sleep_reason(ctx))
        .is_none());
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);

    harness.press_key(Key::Escape, None);
    assert!(harness.state_is::<PauseMenuState>());
    assert!(
        harness
            .menu_labels()
            .iter()
            .any(|row| row == "Emergency shoulder sleep"),
        "{:?}",
        harness.menu_labels()
    );

    harness.clear_speech();
    harness.select_menu_item("Emergency shoulder sleep");
    assert!(harness.state_is::<ShoulderSleepConfirmationState>());
    let said = last(&harness);
    assert!(said.contains("poor rest"), "{said}");
    assert!(said.contains("HOS reset if enforced"), "{said}");

    let minutes_before = harness.read_drive(|d| d.trip.game_minutes);
    harness.key(InputEvent::key(Key::Return)); // the focused Cancel row
    assert!(harness.state_is::<PauseMenuState>());
    assert_eq!(harness.read_drive(|d| d.trip.game_minutes), minutes_before);
}

// -- the snapshot the drive carries ---------------------------------------------------

#[test]
fn test_snapshot_roundtrip_preserves_hos_fatigue_and_fines() {
    let mut harness = a_drive("Snapshot Hours");
    {
        let p = harness.app.ctx.profile.as_mut().expect("a career");
        p.hos.drive(372.0);
        p.hos.check_warnings("realistic");
        p.fatigue = 41.5;
    }
    harness.with_drive(|d, _| d.hos_fine_count = 2);

    let snap = harness.with_drive(|d, ctx| d.snapshot(ctx));
    let resumed = DrivingState::from_snapshot(&mut harness.app.ctx, &snap)
        .expect("the snapshot the drive just wrote reloads");
    assert_eq!(resumed.hos_fine_count, 2);
    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.hos.driving_min, 372.0);
    assert!(!p.hos.warned.is_empty());
    assert_eq!(p.fatigue, 41.5);
}

// -- inspections ------------------------------------------------------------------------

#[test]
fn test_inspection_fines_escalate_and_hit_reputation() {
    let mut harness = a_drive("Inspection Fines");
    let (rep, money) = {
        let p = harness.app.ctx.profile.as_ref().expect("a career");
        (p.career.reputation, p.money())
    };
    for key in ["scale:1", "scale:2"] {
        let event = TripEvent {
            kind: TripEventKind::Inspection,
            message: "Weigh station.".into(),
            data: TripEventData {
                key: Some(key.to_string()),
                ..Default::default()
            },
        };
        harness.with_drive(move |d, ctx| d.handle_inspection(ctx, &event));
    }

    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(
        approx(p.money(), money - hos::HOS_FINES[0] - hos::HOS_FINES[1]),
        "{}",
        p.money()
    );
    assert!(
        approx(p.career.reputation, rep - 2.0 * hos::HOS_REPUTATION_HIT),
        "{}",
        p.career.reputation
    );
    assert_eq!(harness.read_drive(|d| d.hos_fine_count), 2);
}

#[test]
fn test_serious_hos_inspection_orders_out_of_service_reset() {
    let mut harness = a_drive("Out Of Service");
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(481.0);
    let money = harness.app.ctx.profile.as_ref().expect("a career").money();
    let minutes = harness.read_drive(|d| d.trip.game_minutes);
    let event = TripEvent {
        kind: TripEventKind::Inspection,
        message: "Inspection station open.".into(),
        data: TripEventData {
            key: Some("scale:1".to_string()),
            evidence: Some(vec!["HOS/ELD violation".to_string()]),
            ..Default::default()
        },
    };

    let staged = event.clone();
    harness.with_drive(move |d, ctx| d.handle_inspection(ctx, &staged));

    // A serious violation is a REAL stop now: lights come on and nothing is
    // charged or reset until the truck is actually on the shoulder (the old
    // instant path teleported the clock mid-drive).
    harness.with_drive(|d, _| {
        assert_eq!(d.pull_over.as_deref(), Some("lights"));
        assert_eq!(d.pull_over_kind, "hos_out_of_service");
        assert_eq!(d.out_of_service_count, 0);
    });
    assert_eq!(
        harness.app.ctx.profile.as_ref().expect("a career").money(),
        money
    );
    assert_eq!(harness.read_drive(|d| d.trip.game_minutes), minutes);

    // 481 driving minutes is a missed 30-minute break, not a 10-hour reset.
    harness.with_drive(|d, ctx| {
        assert!(
            d.pull_over_summary.contains("thirty minutes"),
            "{}",
            d.pull_over_summary
        );
        assert!(
            !d.pull_over_summary.contains("ten hours"),
            "{}",
            d.pull_over_summary
        );
        d.pull_over_signaled = true;
        d.truck_mut().velocity_mps = 0.0;
        d.open_traffic_stop(ctx);
    });
    {
        let p = harness.app.ctx.profile.as_ref().expect("a career");
        assert!(
            approx(p.money(), money - hos::HOS_FINES[0]),
            "{}",
            p.money()
        );
        assert_eq!(p.hos.driving_min, 481.0);
        assert_eq!(p.hos.since_break_min, 0.0);
        // The order reaches the career record the safety record scores, not
        // only the trip tally: it never did, so a scale kept waving a driver
        // with out-of-service history through as clean.
        assert_eq!(p.out_of_service_events, 1);
        assert_eq!(p.driving_record.citations, 1);
    }
    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        minutes + hos::BREAK_MIN
    ));
    assert_eq!(harness.read_drive(|d| d.out_of_service_count), 1);
    harness.app.ctx.pop_state();
    harness.app.ctx.run_deferred();

    let staged = event.clone();
    harness.with_drive(move |d, ctx| d.handle_inspection(ctx, &staged));
    assert!(approx(
        harness.app.ctx.profile.as_ref().expect("a career").money(),
        money - hos::HOS_FINES[0]
    ));
    assert_eq!(harness.read_drive(|d| d.out_of_service_count), 1);
    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .out_of_service_events,
        1
    );
}

#[test]
fn test_drive_limit_inspection_still_orders_ten_hours() {
    let mut harness = a_drive("Drive Limit Oos");
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(11.0 * 60.0 + 1.0);
    let minutes = harness.read_drive(|d| d.trip.game_minutes);
    let event = TripEvent {
        kind: TripEventKind::Inspection,
        message: "Inspection station open.".into(),
        data: TripEventData {
            key: Some("scale:drive".to_string()),
            evidence: Some(vec!["HOS/ELD violation".to_string()]),
            ..Default::default()
        },
    };
    let staged = event.clone();
    harness.with_drive(move |d, ctx| d.handle_inspection(ctx, &staged));
    harness.with_drive(|d, ctx| {
        assert!(
            d.pull_over_summary.contains("ten hours"),
            "{}",
            d.pull_over_summary
        );
        assert!(
            !d.pull_over_summary.contains("thirty minutes"),
            "{}",
            d.pull_over_summary
        );
        d.pull_over_signaled = true;
        d.truck_mut().velocity_mps = 0.0;
        d.open_traffic_stop(ctx);
    });
    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.hos.driving_min, 0.0);
    assert!(approx(
        harness.read_drive(|d| d.trip.game_minutes),
        minutes + hos::SLEEP_MIN
    ));
}

/// Driving under an out-of-service order is a disqualifying offense (49 CFR
/// 383.51 Table 4) the game has no path to: the order is written only once
/// the truck is stopped on the shoulder, and served in full in the same call,
/// so the truck is legal again before it can roll. Equipment orders are fixed
/// on the spot (`test_bald_tires_in_the_lane_are_out_of_service_until_replaced`).
/// If this ever fails, the offense needs modelling, not this test.
#[test]
fn test_the_truck_never_rolls_under_an_out_of_service_order() {
    let mut harness = a_drive("Order Served");
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(11.0 * 60.0 + 1.0);
    let minutes = harness.read_drive(|d| d.trip.game_minutes);
    let career_h = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .game_hours;
    let event = TripEvent {
        kind: TripEventKind::Inspection,
        message: "Inspection station open.".into(),
        data: TripEventData {
            key: Some("scale:served".to_string()),
            evidence: Some(vec!["HOS/ELD violation".to_string()]),
            ..Default::default()
        },
    };
    harness.with_drive(move |d, ctx| d.handle_inspection(ctx, &event));
    // Lights on and still rolling: no order has been written yet.
    harness.with_drive(|d, ctx| {
        assert_eq!(d.pull_over.as_deref(), Some("lights"));
        assert_eq!(d.out_of_service_count, 0);
        let record = &ctx.profile.as_ref().expect("a career").driving_record;
        assert!(record.out_of_service_times.is_empty());
        d.pull_over_signaled = true;
        d.truck_mut().velocity_mps = 0.0;
        d.open_traffic_stop(ctx);
    });
    // Stopped: the order is written and already served when control returns.
    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(!p.hos.in_violation("realistic"));
    assert_eq!(
        p.driving_record.out_of_service_times,
        vec![career_h + minutes / 60.0]
    );
    harness.with_drive(|d, _| {
        assert_eq!(d.out_of_service_count, 1);
        assert_eq!(d.trip.truck.velocity_mps, 0.0);
        assert!(approx(d.trip.game_minutes, minutes + hos::SLEEP_MIN));
    });
}

// -- the clock the shift runs on --------------------------------------------------------

#[test]
fn test_hos_clock_runs_on_game_time() {
    let mut harness = a_drive("Game Time");
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 10.0); // rolling: counts as driving

    let before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .driving_min;
    hours_frame(&mut harness, 1.0); // one real second
    let gained = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .driving_min
        - before;
    // Below cruise speed the clock compresses less than the configured
    // pacing, and the HOS ledger follows the same effective scale.
    let (effective, configured) =
        harness.read_drive(|d| (d.trip.effective_time_scale(), d.trip.time_scale));
    assert!(approx(gained, effective / 60.0), "{gained}");
    assert!(gained < configured / 60.0, "{gained}");

    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 27.0); // ~60 mph: full pacing
    let before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .driving_min;
    hours_frame(&mut harness, 1.0);
    let gained = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .driving_min
        - before;
    assert!(approx(gained, configured / 60.0), "{gained}");
}

#[test]
fn test_players_own_parking_brake_press_arms_waiting() {
    // Only the player's P press fast-forwards the wait; the auto-set brake at
    // trip start must not, or pre-trip setup would burn game time.
    use ff_core::sim::trip_models::PARKED_TIME_SCALE_MULT;

    let mut harness = a_drive("Deliberate Wait");
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 0.0;
        d.truck_mut().parking_brake = false;
    });

    harness.with_drive(|d, ctx| d.toggle_parking_brake(ctx)); // the player parks deliberately

    harness.with_drive(|d, _| {
        assert!(d.trip.truck.parking_brake);
        assert!(d.trip.waiting);
        assert!(approx(
            d.trip.effective_time_scale(),
            d.trip.time_scale * PARKED_TIME_SCALE_MULT
        ));
    });

    harness.with_drive(|d, ctx| d.toggle_parking_brake(ctx)); // trying to leave always disarms

    assert!(!harness.read_drive(|d| d.trip.waiting));
}

// `test_pre_1_5_snapshot_resumes_with_fresh_clock` runs in
// `crates/freight-fate/tests/states_driving_trip_resume.rs`, beside the other
// old-snapshot resume cases it shares its fixture with.
