//! Opt-in early stop planning, using the same legal-reach route as Alt D.

use ff_core::sim::trip_models::RoadStop;
use freight_fate::app::testing::TestApp;
use freight_fate::playtest::menu::menu_rows;
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_events::stop_plan::StopPlanPurpose;
use freight_fate::states::driving_rest_states::RestStopState;

use super::states_driving_menus_support::{a_drive_between, drive_and_ctx};

fn stop(name: &str, mile: f64, action: &str) -> RoadStop {
    let mut stop = RoadStop::new(name, mile, "travel_center");
    stop.actions = vec![action.into()];
    stop.parking = "confirmed".into();
    stop
}

fn setup(action: &str) -> (TestApp, freight_fate::app::SharedState) {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Buffalo", "Albany", "Hint Driver");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.departure_checked = true;
        d.trip.position_mi = 10.0;
        d.trip.stops = vec![stop("First", 12.0, action), stop("Last", 15.0, action)];
        ctx.profile.as_mut().unwrap().hos.duty_min = 650.0;
    });
    app.clear_speech();
    (app, drive)
}

#[test]
fn driving_frame_uses_the_opt_in_and_keeps_required_warnings_without_it() {
    let (mut app, drive) = setup("sleep");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.velocity_mps = 25.0;
        d.trip.stops = vec![stop("Early", 25.0, "sleep"), stop("Last", 30.0, "sleep")];
        ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
        d.update_hours_and_fatigue(ctx, 0.0);
    });
    assert!(!app.event_lines().join(" ").contains("Sleep stop:"));
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_hours_and_fatigue(ctx, 0.0)
    });
    assert!(app.event_lines().join(" ").contains("Sleep stop:"));

    app.ctx.stop_event_speech();
    app.clear_speech();
    app.ctx.settings.hos_planning_hints = false;
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 780.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_hours_and_fatigue(ctx, 0.0)
    });
    assert!(app.event_lines().join(" ").contains("Hours of service"));
}

#[test]
fn optional_hint_waits_until_three_hours_and_speaks_once() {
    let (mut app, drive) = setup("sleep");
    let clock = app.fake_pacer_clock();
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    assert!(app.event_lines().is_empty());
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    assert!(
        app.event_lines().is_empty(),
        "too early for a 190-minute limit"
    );

    app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    let heard = app.event_lines().join(" ");
    assert!(heard.contains("Sleep stop:"), "{heard}");
    assert!(heard.contains("Last"), "{heard}");
    assert!(heard.contains("Also the last legal stop."), "{heard}");
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    assert_eq!(
        app.event_lines()
            .iter()
            .filter(|line| line.contains("Sleep stop:"))
            .count(),
        1
    );
    let first_key = app.ctx.profile.as_ref().unwrap().hos.warned.clone();

    app.clear_speech();
    app.ctx.stop_event_speech();
    clock.advance(120.0);
    app.ctx.profile.as_mut().unwrap().hos.sleep();
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.game_minutes += 600.0;
        d.maybe_hos_planning_hint(ctx);
    });
    let pending = app.ctx.event_delivery_pending();
    assert!(
        app.event_lines().join(" ").contains("Sleep stop:"),
        "events={:?}, first={first_key:?}, warned={:?}, pending={}",
        app.event_lines(),
        app.ctx.profile.as_ref().unwrap().hos.warned,
        pending
    );
}

#[test]
fn early_hint_names_no_readout_key() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    let said = app.event_lines().join(" ");
    assert!(said.contains("Sleep stop:"), "{said}");
    assert!(!said.contains("Press"), "{said}");
}

#[test]
fn early_sleep_hint_and_requested_readout_separate_plan_from_legal_fallback() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.stops = vec![
            stop("Comfortable", 70.0, "sleep"),
            stop("Legal fallback", 110.0, "sleep"),
        ];
        ctx.profile.as_mut().unwrap().hos.duty_min = 720.0;
        let advice = d.hos_stop_advice(ctx).unwrap();
        assert_eq!(advice.suggested.as_ref().unwrap().stop.name, "Comfortable");
        assert_eq!(advice.stop.as_ref().unwrap().name, "Legal fallback");
        d.maybe_hos_planning_hint(ctx);
    });
    let hint = app.event_lines().join(" ");
    assert!(
        hint.contains("Sleep stop: travel center: Comfortable"),
        "{hint}"
    );
    assert!(hint.contains("Comfortable"), "{hint}");
    assert!(hint.contains("Last legal stop:"), "{hint}");
    assert!(hint.contains("Legal fallback"), "{hint}");
    assert!(!hint.contains("Press"), "{hint}");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.handle_key_event(ctx, &InputEvent::key_mods(Key::D, Mods::ALT));
    });
    let readout = app.main_lines().join(" ");
    assert!(
        readout.contains("sleep stop travel center: Comfortable"),
        "{readout}"
    );
    assert!(
        readout.contains("Last legal: travel center: Legal fallback"),
        "{readout}"
    );
}

#[test]
fn urgent_shoulder_reason_ignores_vehicle_incompatible_stops() {
    let (mut app, drive) = setup("sleep");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.stops = vec![stop("Accessible", 20.0, "sleep")];
        ctx.profile.as_mut().unwrap().hos.duty_min = 830.0;
        assert!(d.upcoming_stop_with_action("sleep", 15.0).is_some());
        let mut inaccessible = stop("Bobtail only", 20.0, "sleep");
        inaccessible.vehicle_access = "bobtail_only".into();
        d.trip.stops = vec![inaccessible];
        assert!(d.upcoming_stop_with_action("sleep", 15.0).is_none());
        let reason = d.emergency_shoulder_sleep_reason(ctx).unwrap();
        assert!(
            reason.contains("no suitable route stop is visible"),
            "{reason}"
        );
    });
}

#[test]
fn early_break_hint_prefers_a_break_stop_and_keeps_a_sleep_stop_as_fallback() {
    let (mut app, drive) = setup("break");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.stops = vec![
            stop("Break first", 70.0, "break"),
            stop("Sleeper fallback", 110.0, "sleep"),
        ];
        let hos = &mut ctx.profile.as_mut().unwrap().hos;
        hos.duty_min = 0.0;
        hos.driving_min = 360.0;
        hos.since_break_min = 360.0;
        let advice = d.hos_stop_advice(ctx).unwrap();
        assert_eq!(advice.action, "break");
        assert_eq!(advice.suggested.as_ref().unwrap().stop.name, "Break first");
        assert_eq!(advice.stop.as_ref().unwrap().name, "Sleeper fallback");
        d.maybe_hos_planning_hint(ctx);
    });
    let hint = app.event_lines().join(" ");
    assert!(
        hint.contains("Break stop: travel center: Break first"),
        "{hint}"
    );
    assert!(hint.contains("Break first"), "{hint}");
    assert!(hint.contains("Sleeper fallback"), "{hint}");
}

#[test]
fn t_selects_the_comfortable_sleep_stop_instead_of_the_nearest_stop() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.stops = vec![
            stop("Nearest", 12.0, "sleep"),
            stop("Comfortable", 70.0, "sleep"),
            stop("Legal fallback", 110.0, "sleep"),
        ];
        d.trip.truck.velocity_mps = 25.0;
        ctx.profile.as_mut().unwrap().hos.duty_min = 720.0;
        d.plan_stop_for(ctx, StopPlanPurpose::Sleep);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Comfortable");
        assert_eq!(d.selected_stop_key, d.trip.planned_stop_key);
        assert!(!d.selected_stop_break);
    });
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("Sleep stop planned, travel center: Comfortable"),
        "{said}"
    );
    assert!(said.contains("Comfortable"), "{said}");
    assert!(!said.contains("Nearest"), "{said}");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.handle_key_event(ctx, &InputEvent::key(Key::T));
        assert!(d.trip.planned_stop_key.is_none());
        assert!(d.selected_stop_key.is_none());
    });
    assert!(app.main_lines().join(" ").contains("Planned stop canceled"));

    app.ctx.settings.hos_planning_hints = false;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.plan_stop_for(ctx, StopPlanPurpose::Sleep);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Nearest");
    });
}

#[test]
fn t_selects_a_break_only_stop_and_focuses_the_break_row_after_arrival() {
    let (mut app, drive) = setup("break");
    app.ctx.settings.hos_planning_hints = true;
    let selected = stop("Break first", 70.0, "break");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.stops = vec![
            stop("Near sleep", 12.0, "sleep"),
            selected.clone(),
            stop("Sleeper fallback", 110.0, "sleep"),
        ];
        d.trip.truck.velocity_mps = 25.0;
        let hos = &mut ctx.profile.as_mut().unwrap().hos;
        hos.duty_min = 0.0;
        hos.driving_min = 360.0;
        hos.since_break_min = 360.0;
        d.plan_stop_for(ctx, StopPlanPurpose::Break);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Break first");
        assert!(d.selected_stop_break);
    });
    let said = app.main_lines().join(" ");
    assert!(said.contains("Break stop planned"), "{said}");

    let snapshot = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    assert_eq!(snapshot["selected_stop_break"], true);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).unwrap();
    assert_eq!(resumed.selected_stop_key, Some(selected.key()));
    assert!(resumed.selected_stop_break);

    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.handle_key_event(ctx, &InputEvent::key(Key::T));
        assert!(d.trip.planned_stop_key.is_none());
        assert!(d.selected_stop_key.is_none());
        assert!(!d.selected_stop_break);
        d.plan_stop_for(ctx, StopPlanPurpose::Break);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Break first");
        assert!(d.selected_stop_break);
        d.trip.position_mi = selected.at_mi;
        d.trip.truck.velocity_mps = 0.0;
        d.open_poi_stop(ctx, &selected, false, None);
    });
    app.ctx.run_deferred();
    let state = app.ctx.state().unwrap();
    let borrowed = state.borrow();
    assert!(borrowed.as_any().is::<RestStopState>());
    let (rows, focus) = menu_rows(&*borrowed, &app.ctx).unwrap();
    assert_eq!(rows[focus], "Take a 30-minute break");
}

#[test]
fn t_hos_selection_rebinds_to_a_real_route_stop_after_resume() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Buffalo", "Albany", "Resume Driver");
    app.ctx.settings.hos_planning_hints = true;
    let selected_key = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.departure_checked = true;
        d.trip.position_mi = 23.2;
        d.trip.truck.velocity_mps = 25.0;
        let hos = &mut ctx.profile.as_mut().unwrap().hos;
        hos.driving_min = 300.0;
        hos.duty_min = 300.0;
        hos.since_break_min = 300.0;
        let advice = d.hos_stop_advice(ctx).unwrap();
        assert_eq!(advice.action, "break");
        let recommended = advice.suggested.as_ref().unwrap().stop.key();
        d.plan_stop_for(ctx, StopPlanPurpose::Break);
        assert_eq!(d.selected_stop_key, Some(recommended.clone()));
        recommended
    });
    let snapshot = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).unwrap();
    assert_eq!(resumed.selected_stop_key, Some(selected_key.clone()));
    assert_eq!(resumed.trip.planned_stop_key, Some(selected_key.clone()));
    assert_eq!(resumed.selected_rest_stop().unwrap().key(), selected_key);
    assert!(resumed.selected_stop_break);
}

#[test]
fn sleep_only_stop_for_break_speaks_the_long_rest_needed() {
    let (mut app, drive) = setup("break");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.stops = vec![stop("Sleeper", 70.0, "sleep")];
        d.trip.truck.velocity_mps = 25.0;
        let hos = &mut ctx.profile.as_mut().unwrap().hos;
        hos.duty_min = 0.0;
        hos.driving_min = 360.0;
        hos.since_break_min = 360.0;
        d.plan_stop_for(ctx, StopPlanPurpose::Break);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Sleeper");
        assert!(!d.selected_stop_break);
    });
    let said = app.main_lines().join(" ");
    assert!(said.contains("Sleep stop planned for your break"), "{said}");
    assert!(!said.contains("Break stop planned"), "{said}");
}

#[test]
fn t_selects_the_last_legal_fallback_when_no_stop_has_a_comfort_buffer() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.velocity_mps = 25.0;
        ctx.profile.as_mut().unwrap().hos.duty_min = 820.0;
        let advice = d.hos_stop_advice(ctx).unwrap();
        assert_eq!(advice.stop.as_ref().unwrap().name, "Last");
        assert!(advice.suggested.is_none());
        d.plan_stop_for(ctx, StopPlanPurpose::Sleep);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Last");
    });
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("the last one before your sleep limit"),
        "{said}"
    );
}

#[test]
fn t_keeps_nearest_usable_sleep_stop_when_no_hos_recommendation_applies() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let mut blocked = stop("Bobtail only", 12.0, "sleep");
        blocked.vehicle_access = "bobtail_only".into();
        d.trip.stops = vec![blocked, stop("Usable", 15.0, "sleep")];
        d.trip.truck.velocity_mps = 25.0;
        ctx.profile.as_mut().unwrap().hos.duty_min = 0.0;
        assert!(d.hos_stop_advice(ctx).unwrap().destination_reachable);
        d.plan_stop_for(ctx, StopPlanPurpose::Sleep);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Usable");
    });
    let said = app.main_lines().join(" ");
    assert!(said.contains("Sleep stop planned"), "{said}");
    assert!(!said.contains("Bobtail only"), "{said}");

    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.handle_key_event(ctx, &InputEvent::key(Key::T));
        ctx.profile.as_mut().unwrap().hos.duty_min = 839.0;
        assert!(d.hos_stop_advice(ctx).unwrap().stop.is_none());
        d.plan_stop_for(ctx, StopPlanPurpose::Sleep);
        assert_eq!(d.trip.planned_stop().unwrap().name, "Usable");
    });
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("none is reachable before your hours limit"),
        "{said}"
    );
}

#[test]
fn requested_t_selection_speaks_in_quiet_and_urgent_only() {
    for rung in ["quiet", "urgent_only"] {
        let (mut app, drive) = setup("break");
        app.ctx.settings.hos_planning_hints = true;
        app.ctx.settings.driving_speech = rung.into();
        drive_and_ctx(&drive, &mut app, |d, ctx| {
            d.trip.stops = vec![stop("Break here", 70.0, "break")];
            d.trip.truck.velocity_mps = 25.0;
            let hos = &mut ctx.profile.as_mut().unwrap().hos;
            hos.duty_min = 0.0;
            hos.driving_min = 360.0;
            hos.since_break_min = 360.0;
            d.plan_stop_for(ctx, StopPlanPurpose::Break);
            assert_eq!(d.trip.planned_stop().unwrap().name, "Break here");
        });
        let said = app.main_lines().join(" ");
        assert!(said.contains("Break stop planned"), "{rung}: {said}");
        assert!(
            app.event_lines().is_empty(),
            "{rung}: {:?}",
            app.event_lines()
        );
    }
}

#[test]
fn early_hint_names_when_no_stop_can_be_reached() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.stops.clear();
        d.maybe_hos_planning_hint(ctx);
    });
    let heard = app.event_lines().join(" ");
    assert!(
        heard.contains("No reachable sleep stop before your hours limit."),
        "{heard}"
    );
}

#[test]
fn break_hint_uses_break_stops_and_stays_ahead_of_the_hour_warning() {
    let (mut app, drive) = setup("break");
    app.ctx.settings.hos_planning_hints = true;
    {
        let hos = &mut app.ctx.profile.as_mut().unwrap().hos;
        hos.duty_min = 0.0;
        hos.driving_min = 300.0;
        hos.since_break_min = 300.0;
    }
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    let heard = app.event_lines().join(" ");
    assert!(heard.contains("Break stop:"), "{heard}");
    assert!(!heard.contains("Hours of service:"), "{heard}");
}

#[test]
fn quiet_and_urgent_only_suppress_optional_words_without_spending_the_hint() {
    for rung in ["quiet", "urgent_only"] {
        let (mut app, drive) = setup("sleep");
        app.ctx.settings.hos_planning_hints = true;
        app.ctx.settings.driving_speech = rung.into();
        app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
        drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
        assert!(
            app.event_lines().is_empty(),
            "{rung}: {:?}",
            app.event_lines()
        );
        assert!(!app
            .ctx
            .profile
            .as_ref()
            .unwrap()
            .hos
            .warned
            .iter()
            .any(|key| key.contains("plan-hint")));
        app.ctx.settings.driving_speech = "standard".into();
        drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
        assert!(app.event_lines().join(" ").contains("Sleep stop:"));
    }
}

#[test]
fn reachable_destination_can_become_unreachable_after_a_delay() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.position_mi = d.trip.total_miles() - 60.0;
        assert!(d.hos_stop_advice(ctx).unwrap().destination_reachable);
        d.maybe_hos_planning_hint(ctx);
    });
    assert!(app.event_lines().is_empty());
    assert!(app.ctx.profile.as_ref().unwrap().hos.warned.is_empty());

    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let travel_min = d.hos_stop_advice(ctx).unwrap().destination_travel_min;
        assert!((60.0..=180.0).contains(&travel_min));
        ctx.profile.as_mut().unwrap().hos.duty_min = 840.0 - travel_min - 2.0;
        assert!(!d.hos_stop_advice(ctx).unwrap().destination_reachable);
        d.maybe_hos_planning_hint(ctx);
    });
    assert!(app
        .event_lines()
        .join(" ")
        .contains("No reachable sleep stop"));
}

#[test]
fn interrupted_hint_is_retried_until_delivery_completes() {
    let (mut app, drive) = setup("sleep");
    let clock = app.fake_pacer_clock();
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    assert!(app.event_lines().join(" ").contains("Sleep stop:"));
    assert!(app.ctx.profile.as_ref().unwrap().hos.warned.is_empty());

    app.ctx.stop_event_speech();
    app.clear_speech();
    clock.advance(120.0);
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    assert!(app.event_lines().join(" ").contains("Sleep stop:"));
    assert!(app.ctx.profile.as_ref().unwrap().hos.warned.is_empty());

    clock.advance(120.0);
    drive_and_ctx(&drive, &mut app, |d, ctx| d.maybe_hos_planning_hint(ctx));
    assert!(app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .hos
        .warned
        .iter()
        .any(|key| key.contains("plan-hint")));
}

#[test]
fn selected_stop_and_earlier_stop_warning_suppress_late_planning_advice() {
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.selected_stop_key = Some("planned-stop".to_string());
        d.maybe_hos_planning_hint(ctx);
    });
    assert!(app.event_lines().is_empty());
    assert!(app.ctx.profile.as_ref().unwrap().hos.warned.is_empty());

    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.selected_stop_key = None;
        let kind = ctx
            .profile
            .as_ref()
            .unwrap()
            .hos
            .next_limit(&ctx.settings.hos_mode)
            .unwrap()
            .kind;
        ctx.profile
            .as_mut()
            .unwrap()
            .hos
            .warned
            .push(format!("{kind}:hos-stop:planned-stop"));
        d.maybe_hos_planning_hint(ctx);
    });
    assert!(app.event_lines().is_empty());
    assert!(!app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .hos
        .warned
        .iter()
        .any(|key| key.contains("plan-hint")));
}

/// The departure streets are swapped out at the on-ramp merge, so a plan made
/// there was cancelled a frame later with a false "past your planned stop".
/// T waits for the highway.
#[test]
fn t_on_the_departure_streets_plans_nothing() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Rochester", "Buffalo", "Hint Driver");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.departure_checked = true;
        d.job.origin_location = "Rochester freight market".into();
        assert!(
            d.begin_departure_chain(ctx, false),
            "no departure chain here"
        );
        ctx.profile.as_mut().unwrap().hos.duty_min = 780.0;
        d.trip.truck.velocity_mps = 10.0;
    });
    app.clear_speech();
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.handle_key_event(ctx, &InputEvent::key(Key::T));
        assert!(d.trip.planned_stop_key.is_none());
        assert!(d.selected_stop_key.is_none());
    });
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
}

#[test]
fn on_a_team_load_the_sleep_hint_names_the_co_driver_and_no_stop() {
    // The team load's deadline has the co-driver drive where a solo driver
    // sleeps: a sleep stop is the wrong advice (player report, 2026-10-20).
    let (mut app, drive) = setup("sleep");
    app.ctx.settings.hos_planning_hints = true;
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.job.team = true;
        ctx.profile.as_mut().unwrap().hos.duty_min = 660.0;
        d.maybe_hos_planning_hint(ctx);
    });
    let heard = app.event_lines().join(" ");
    let name = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.team_pairing(ctx).co_driver.name
    });
    assert!(
        heard.contains(&format!("{name} can take the wheel from the pause menu.")),
        "{heard}"
    );
    assert!(!heard.contains("Sleep stop:"), "{heard}");
}

#[test]
fn on_a_team_load_the_last_sleep_stop_warning_stays_quiet() {
    let (mut app, drive) = setup("sleep");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.job.team = true;
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.stops = vec![stop("Last", 12.0, "sleep")];
        d.warn_last_hos_stop(ctx);
    });
    let heard = app.event_lines().join(" ");
    assert!(!heard.contains("Last"), "{heard}");
    // A solo load still hears it.
    drop(drive);
    drop(app);
    let (mut app, drive) = setup("sleep");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.stops = vec![stop("Last", 12.0, "sleep")];
        d.warn_last_hos_stop(ctx);
    });
    let heard = app.event_lines().join(" ");
    assert!(heard.contains("Last"), "{heard}");
}
