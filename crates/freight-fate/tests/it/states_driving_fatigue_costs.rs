//! What builds and clears fatigue at the wheel and at a stop
//! (`docs/fatigue-design.md` parts 2 and 5): on-duty time builds a third of
//! the driving rate, sleep clears it by the hour, and a coffee or energy
//! drink lends a lift that comes back when it wears off.

use ff_core::sim::alertness::Alertness;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::sim::hos;
use ff_core::sim::stimulant::COFFEE_WORN_OFF;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{set_engine_running, FUEL_STOP_MIN};
use freight_fate::states::driving_rest_states::RestStopState;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A delivery under way on a quiet road, parked.
fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|d, _| {
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_manager.rolling_bubble = false;
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.trip.truck.velocity_mps = 0.0;
    });
    harness.clear_speech();
    harness
}

fn fatigue(harness: &PlaytestHarness) -> f64 {
    harness.app.ctx.profile.as_ref().expect("a career").fatigue
}

fn set_fatigue(harness: &mut PlaytestHarness, value: f64) {
    harness.app.ctx.profile.as_mut().expect("a career").fatigue = value;
}

/// A travel center right where the truck is, with its rest-stop menu open.
fn at_a_stop(harness: &mut PlaytestHarness) {
    let at_mi = harness.read_drive(|d| d.trip.position_mi).max(1.0);
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
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![stop];
        d.trip.position_mi = at_mi;
    });
    harness.plan_stop("Sleep");
    assert!(harness.state_is::<RestStopState>());
}

/// Move the trip clock on by `minutes` and run one hours-and-fatigue frame
/// that adds no time of its own.
fn later(harness: &mut PlaytestHarness, minutes: f64) {
    harness.with_drive(move |d, ctx| {
        d.trip.game_minutes += minutes;
        d.update_hours_and_fatigue(ctx, 0.0);
    });
}

fn count(lines: &[String], text: &str) -> usize {
    lines.iter().filter(|line| line.contains(text)).count()
}

// -- on duty -------------------------------------------------------------------

#[test]
fn sitting_stopped_at_the_wheel_builds_a_third_of_the_driving_rate() {
    let mut harness = a_drive("On Duty Wheel");
    set_fatigue(&mut harness, 20.0);
    let dt = 1.0;
    let expected = harness.read_drive(|d| {
        let gm = dt * d.trip.effective_time_scale() / 60.0;
        let night = hos::is_night(d.trip.local_hour());
        hos::fatigue_rate_per_min(night)
            * gm
            * tuning_for_time_scale(d.trip.time_scale).fatigue_rate
            * hos::ON_DUTY_FATIGUE_FACTOR
    });
    assert!(expected > 0.0);
    harness.with_drive(move |d, ctx| d.update_hours_and_fatigue(ctx, dt));
    assert!(
        approx(fatigue(&harness) - 20.0, expected),
        "{} != 20 + {expected}",
        fatigue(&harness)
    );
}

#[test]
fn fueling_builds_on_duty_fatigue() {
    let mut harness = a_drive("On Duty Fuel");
    set_fatigue(&mut harness, 20.0);
    harness.with_drive(|d, ctx| {
        d.trip.truck.fuel_gal = 40.0;
        set_engine_running(ctx, &mut d.trip.truck, false);
    });
    at_a_stop(&mut harness);
    let expected = harness.read_drive(|d| {
        let mode = tuning_for_time_scale(d.trip.time_scale).fatigue_rate;
        hos::on_duty_fatigue_gain(d.trip.local_hour(), FUEL_STOP_MIN, |_| mode)
    });
    harness.select_menu_item_starting("Refuel");
    assert!(
        approx(fatigue(&harness), 20.0 + expected),
        "{} != 20 + {expected}",
        fatigue(&harness)
    );
}

// -- sleep by the hour -----------------------------------------------------------

#[test]
fn ten_hours_of_proper_sleep_from_95_leaves_15() {
    let mut harness = a_drive("Sleep From 95");
    set_fatigue(&mut harness, 95.0);
    at_a_stop(&mut harness);
    harness.clear_speech();
    harness.select_menu_item("Sleep 10 hours");
    // The first press reads the forecast with the real figure.
    let preview = harness.app.speech().lines().join(" ");
    assert!(preview.contains("Fatigue 95 to 15."), "{preview}");
    harness.clear_speech();
    harness.select_menu_item("Sleep 10 hours");
    assert!(approx(fatigue(&harness), 15.0), "{}", fatigue(&harness));
    let said = harness.app.speech().lines().join(" ");
    assert!(
        said.contains("You slept 10 hours and woke rested."),
        "{said}"
    );
}

// -- stimulants ------------------------------------------------------------------

#[test]
fn a_coffee_break_is_paid_back_in_two_hours_with_one_line() {
    let mut harness = a_drive("Coffee Loan");
    set_fatigue(&mut harness, 50.0);
    at_a_stop(&mut harness);
    harness.select_menu_item("Food and coffee break");
    assert!(approx(fatigue(&harness), 42.0), "{}", fatigue(&harness));

    // A second cup before the first wears off lifts nothing.
    harness.clear_speech();
    harness.select_menu_item("Food and coffee break");
    assert!(approx(fatigue(&harness), 42.0), "{}", fatigue(&harness));
    let said = harness.app.speech().lines().join(" ");
    assert!(
        said.contains("No lift while the last coffee lasts."),
        "{said}"
    );

    harness.press_key(freight_fate::states::base::Key::Escape, None);
    harness.clear_speech();
    // Fifteen minutes already passed with the second cup; the loan falls
    // due two hours after the first.
    later(&mut harness, 100.0);
    assert!(approx(fatigue(&harness), 42.0), "{}", fatigue(&harness));
    later(&mut harness, 10.0);
    assert!(approx(fatigue(&harness), 50.0), "{}", fatigue(&harness));
    later(&mut harness, 60.0);
    assert!(approx(fatigue(&harness), 50.0), "{}", fatigue(&harness));
    let said = harness.app.event_lines();
    assert_eq!(count(&said, COFFEE_WORN_OFF), 1, "{said:?}");
}

#[test]
fn a_coffee_wearing_off_during_sleep_is_paid_back_first_and_silently() {
    let mut harness = a_drive("Coffee Then Bed");
    set_fatigue(&mut harness, 95.0);
    at_a_stop(&mut harness);
    harness.select_menu_item("Food and coffee break");
    assert!(approx(fatigue(&harness), 87.0), "{}", fatigue(&harness));
    // The fifteen-minute coffee counts toward the ten hours off.
    let minutes = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .reset_minutes_left()
        .expect("a rest under way");
    // 87 plus the 8 owed back is 95, and the sleep takes 8 an hour off that.
    let expected = hos::rest_sleep(95.0, minutes);
    harness.clear_speech();
    harness.select_menu_item_starting("Sleep 9");
    let preview = harness.app.speech().lines().join(" ");
    assert!(
        preview.contains(&format!("Fatigue 87 to {expected:.0}.")),
        "{preview}"
    );
    harness.select_menu_item_starting("Sleep 9");
    assert!(
        approx(fatigue(&harness), expected),
        "{} != {expected}",
        fatigue(&harness)
    );
    let said = harness.app.speech().lines();
    assert_eq!(count(&said, "worn off"), 0, "{said:?}");
}

#[test]
fn an_energy_drink_wearing_off_speaks_one_line_and_takes_its_lift_back() {
    let mut harness = a_drive("Drink Loan");
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(1_000.0);
    set_fatigue(&mut harness, 50.0);
    at_a_stop(&mut harness);
    harness.select_menu_item_starting("Energy drink");
    assert!(approx(fatigue(&harness), 47.0), "{}", fatigue(&harness));
    harness.press_key(freight_fate::states::base::Key::Escape, None);
    harness.clear_speech();
    later(&mut harness, 2.0 * 60.0 + 1.0);
    assert!(approx(fatigue(&harness), 50.0), "{}", fatigue(&harness));
    later(&mut harness, 30.0);
    let said = harness.app.event_lines();
    assert_eq!(
        count(&said, "The energy drink has worn off."),
        1,
        "{said:?}"
    );
    assert_eq!(count(&said, "worn off"), 1, "{said:?}");
}

#[test]
fn a_lift_paid_back_across_a_band_gives_that_band_its_cue() {
    let mut harness = a_drive("Coffee Band");
    set_fatigue(&mut harness, 62.0);
    // Drowsy already, its cue long since heard.
    harness.with_drive(|d, _| d.alertness = Alertness::Drowsy);
    at_a_stop(&mut harness);
    harness.select_menu_item("Food and coffee break");
    harness.press_key(freight_fate::states::base::Key::Escape, None);
    later(&mut harness, 0.0);
    assert_eq!(harness.read_drive(|d| d.alertness), Alertness::Worn);
    harness.clear_speech();
    later(&mut harness, 2.0 * 60.0);
    assert_eq!(harness.read_drive(|d| d.alertness), Alertness::Drowsy);
    let said = harness.app.event_lines();
    assert_eq!(count(&said, "Getting drowsy."), 1, "{said:?}");
    assert_eq!(count(&said, COFFEE_WORN_OFF), 1, "{said:?}");
}

// -- the worn-off line through a busy road voice -------------------------------

/// The road voice reports it is speaking, the way NVDA 2026.3 and SAPI do,
/// so queued lines are held behind it.
fn reporting_voice(harness: &mut PlaytestHarness) -> freight_fate::app::testing::FakeClock {
    harness.app.ctx.settings.sapi_events = true;
    harness.app.fake_pacer_clock()
}

fn voice_frame(harness: &mut PlaytestHarness, speaking: bool) {
    harness.app.speech_mut().set_voice_speaking(Some(speaking));
    harness.app.ctx.update_event_voice_report();
}

fn first_index(lines: &[String], text: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(text))
}

/// Agent playtest, 2026-11-13: a coffee came due inside the loop-back
/// after driving past the receiver. The loop-back's line held the
/// worn-off line, "Getting drowsy." went first, and the worn-off line timed
/// out unheard, so fatigue jumped a band with nothing said about why.
#[test]
fn a_coffee_due_inside_a_turn_back_loop_is_heard_before_the_band_cue() {
    let mut harness = a_drive("Coffee Loop");
    set_fatigue(&mut harness, 62.0);
    harness.with_drive(|d, _| d.alertness = Alertness::Drowsy);
    at_a_stop(&mut harness);
    harness.select_menu_item("Food and coffee break");
    harness.select_menu_item("Back to the road");
    harness.expect_state::<DrivingState>("back on the road after the coffee");
    later(&mut harness, 0.0);
    assert_eq!(harness.read_drive(|d| d.alertness), Alertness::Worn);
    // Back on the road, ten minutes short of the coffee wearing off.
    later(&mut harness, 110.0);
    assert!(approx(fatigue(&harness), 54.0), "{}", fatigue(&harness));

    let clock = reporting_voice(&mut harness);
    harness.clear_speech();
    let stop = RoadStop::new("Omaha metro freight market", 1.0, "facility");
    harness.with_drive(move |d, ctx| d.loop_back_to_destination_terminal(ctx, &stop));
    voice_frame(&mut harness, true);
    // The first frame back at the wheel pays the coffee back.
    later(&mut harness, 0.0);
    // 54, the eight owed back, and the loop's twenty minutes of driving.
    assert!(fatigue(&harness) > 62.0, "{}", fatigue(&harness));
    // The loop-back line takes a few seconds to say.
    for _ in 0..10 {
        clock.advance(0.5);
        voice_frame(&mut harness, true);
    }
    // The voice falls quiet, takes the next held line and says it, then
    // falls quiet again for the one after.
    for _ in 0..3 {
        clock.advance(0.2);
        voice_frame(&mut harness, false);
        for _ in 0..3 {
            clock.advance(0.5);
            voice_frame(&mut harness, true);
        }
    }
    let said = harness.app.speech().lines();
    assert_eq!(count(&said, COFFEE_WORN_OFF), 1, "{said:?}");
    let worn = first_index(&said, COFFEE_WORN_OFF).expect("the worn-off line");
    let drowsy =
        first_index(&said, "Getting drowsy.").unwrap_or_else(|| panic!("no band cue in {said:?}"));
    assert!(worn < drowsy, "{said:?}");
}

/// A coffee wearing off while the truck is fueled: the fueling itself
/// says nothing about it, and the line is heard once back at the wheel.
#[test]
fn a_coffee_due_while_fueling_is_heard_back_at_the_wheel() {
    let mut harness = a_drive("Coffee Fuel");
    set_fatigue(&mut harness, 50.0);
    at_a_stop(&mut harness);
    harness.select_menu_item("Food and coffee break");
    harness.select_menu_item("Back to the road");
    harness.expect_state::<DrivingState>("back on the road after the coffee");
    // Five minutes before the coffee wears off, pull in to fuel.
    harness.with_drive(|d, ctx| {
        d.trip.game_minutes += 115.0;
        d.trip.truck.fuel_gal = 40.0;
        set_engine_running(ctx, &mut d.trip.truck, false);
    });
    harness.clear_speech();
    at_a_stop(&mut harness);
    // Still worn at 42, so the fueling runs at the worn band's pace.
    let gain = harness.with_drive(|d, ctx| {
        let mode = tuning_for_time_scale(d.trip.time_scale).fatigue_rate;
        let minutes = d.hand_work_min(ctx, FUEL_STOP_MIN);
        assert!(minutes > FUEL_STOP_MIN, "{minutes}");
        hos::on_duty_fatigue_gain(d.trip.local_hour(), minutes, |_| mode)
    });
    harness.select_menu_item_starting("Refuel");
    // Still owed: the stop's own line says nothing about the coffee.
    let said = harness.app.speech().lines();
    assert!(
        approx(fatigue(&harness), 42.0 + gain),
        "{} {said:?} {:?}",
        fatigue(&harness),
        harness.menu_labels()
    );
    assert_eq!(count(&said, "worn off"), 0, "{said:?}");

    if harness.state_is::<RestStopState>() {
        harness.select_menu_item("Back to the road");
    }
    harness.expect_state::<DrivingState>("back at the wheel after fueling");
    later(&mut harness, 0.0);
    assert!(
        approx(fatigue(&harness), 50.0 + gain),
        "{}",
        fatigue(&harness)
    );
    later(&mut harness, 30.0);
    let said = harness.app.event_lines();
    assert_eq!(count(&said, COFFEE_WORN_OFF), 1, "{said:?}");
}
