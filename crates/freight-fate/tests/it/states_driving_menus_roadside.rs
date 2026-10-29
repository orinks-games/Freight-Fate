//! The three roadside enforcement outcomes in `states/driving_rest_states.rs`:
//! a speeding stop, a non-speeding enforcement stop, and the felony stop that
//! ends a run.
//!
//! Python covered these through `driving_updates`' pull-over machinery, which
//! is another task's suite; what is pinned here is what the screens themselves
//! decide and say, which is where `_resolve` charges the money exactly once.

use ff_core::models::enforcement;
use ff_core::sim::hos;
use ff_core::sim::trip_models::Zone;

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::city::CityMenuState;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DRIVE_PHASE_DELIVERY, FAILURE_TO_STOP_DAMAGE_PCT};
use freight_fate::states::driving_rest_states::{
    EnforcementStopState, FelonyStopState, LicencePulledState, TrafficStopState,
};
use freight_fate::states::driving_updates::pending::EnforcementStopParams;

use crate::states_driving_menus_support::*;

fn params(title: &str, out_of_service: bool, inspection_on_stop: bool) -> EnforcementStopParams {
    EnforcementStopParams {
        title: title.to_string(),
        summary: "The inspector writes it up.".to_string(),
        fine: enforcement::LANE_MISUSE_FINE,
        reputation_hit: 2.0,
        signaled: true,
        return_message: "Back on the highway.".to_string(),
        out_of_service,
        warned: false,
        construction_zone: false,
        inspection_on_stop,
        inspection_level: None,
    }
}

// -- the speeding stop --------------------------------------------------------------------

#[test]
fn test_a_first_marginal_stop_is_a_warning_not_a_ticket() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, true, 9.0, 65.0, false, false, false)
    });
    assert!(
        state.outcome_text().contains("lets you off with a warning"),
        "{}",
        state.outcome_text()
    );
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before
    );
    assert_eq!(with_drive(&drive, |d| d.speeding_tickets), 0);
}

#[test]
fn test_a_serious_stop_writes_the_ticket_once_and_charges_it_on_the_spot() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let money_before = {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.career.reputation = 40.0;
        profile.money()
    };
    let expected = enforcement::speeding_citation_fine(24.0, 0, false);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, false, 24.0, 65.0, false, false, false)
    });
    assert!(
        state.outcome_text().contains("Speeding ticket:"),
        "{}",
        state.outcome_text()
    );
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected
    );
    assert_eq!(with_drive(&drive, |d| d.speeding_tickets), 1);
    assert_eq!(with_drive(&drive, |d| d.ticket_fines_paid), expected);
}

/// The ticket's reputation hit comes off the delivery ledger. It used to
/// write back the shown standing (ledger minus record), so a driver with a
/// record lost the record's points from the ledger for good (2026-09-28).
#[test]
fn test_a_ticket_takes_its_hit_from_the_ledger_not_the_shown_standing() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.career.reputation = 60.0;
        p.driving_record.citations = 3;
        p.driving_record.citation_times = vec![p.game_hours; 3];
        assert!(p.standing() < 60.0, "the record must show in the standing");
    }
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, false, 24.0, 65.0, false, false, false)
    });
    let ledger = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .career
        .reputation;
    assert!(
        (ledger - (60.0 - hos::HOS_REPUTATION_HIT)).abs() < 1e-9,
        "ledger {ledger}"
    );
}

#[test]
fn test_a_work_zone_ticket_says_so_and_costs_double() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .career
        .reputation = 40.0;
    let plain = enforcement::speeding_citation_fine(24.0, 0, false);
    let zone = enforcement::speeding_citation_fine(24.0, 0, true);
    assert_eq!(zone, plain * enforcement::CONSTRUCTION_ZONE_FINE_MULTIPLIER);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, false, 24.0, 65.0, false, false, true)
    });
    assert!(
        state
            .outcome_text()
            .contains(enforcement::construction_zone_fine_clause(true).trim()),
        "{}",
        state.outcome_text()
    );
    assert_eq!(with_drive(&drive, |d| d.ticket_fines_paid), zone);
}

#[test]
fn test_the_traffic_stop_offers_one_way_back_onto_the_highway() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, true, 9.0, 65.0, false, false, false)
    });
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(rows, vec!["Pull back onto the highway"]);
    app.ctx
        .push_shared_with(freight_fate::app::share(state), false);
    app.clear_speech();
    with_top_ctx::<TrafficStopState, _>(&mut app, |stop, ctx| stop.go_back(ctx));
    assert_eq!(last(&app), "Back on the highway. Watch your speed.");
}

#[test]
fn test_a_pulled_licence_ends_the_run_from_the_shoulder() {
    // A stop that just pulled the license cannot offer the highway: the
    // driver is not allowed to move the truck.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.career.reputation = 40.0;
        profile.driving_record.lifetime_disqualified = true;
    }
    let mut state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, false, 24.0, 65.0, false, false, false)
    });
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(rows, vec!["Return to terminal"]);
    assert!(
        state.outcome_text().contains("You are released to"),
        "{}",
        state.outcome_text()
    );
}

#[test]
fn test_a_pulled_licence_names_the_load_dispatch_takes_back() {
    // The stop is resolved while the drive that pushed it is still held, so
    // the outcome has to be told from the drive it was handed rather than
    // reaching for the drive again. It used to reach, the second borrow
    // failed, and a loaded run was told there was no trailer to hand back.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.career.reputation = 40.0;
        profile.driving_record.lifetime_disqualified = true;
    }
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        assert_eq!(d.phase, DRIVE_PHASE_DELIVERY, "a loaded delivery run");
        assert!(!d.job.bobtail, "with a trailer on");
        TrafficStopState::new(ctx, d, false, 24.0, 65.0, false, false, false)
    });
    let cargo = with_drive(&drive, |d| d.job.cargo.label.to_string());
    assert!(
        state
            .outcome_text()
            .contains(&format!("Dispatch takes the {cargo} load back")),
        "{}",
        state.outcome_text()
    );
    assert!(
        !state.outcome_text().contains("no loaded trailer"),
        "{}",
        state.outcome_text()
    );
}

#[test]
fn test_a_bobtail_pulled_licence_says_there_is_no_trailer() {
    // The other side of the same branch, so the fix cannot be "always say
    // there is a load".
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| d.job.bobtail = true);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.career.reputation = 40.0;
        profile.driving_record.lifetime_disqualified = true;
    }
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, false, 24.0, 65.0, false, false, false)
    });
    assert!(
        state
            .outcome_text()
            .contains("There is no loaded trailer to hand back"),
        "{}",
        state.outcome_text()
    );
}

// -- the enforcement stop ------------------------------------------------------------------

#[test]
fn test_an_enforcement_stop_charges_once_and_reads_back_as_history() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    app.seasoned_driver();
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let expected = enforcement::citation_fine(enforcement::LANE_MISUSE_FINE, 0, false, None);
    let mut state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, params("Lane misuse", false, false))
    });
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected
    );

    app.clear_speech();
    state.announce_entry(&mut app.ctx);
    let first = last(&app);
    assert!(
        first.starts_with("You stop on the shoulder for an enforcement inspection."),
        "{first}"
    );

    // Re-reading the stop must not sound like a second charge.
    app.clear_speech();
    state.announce_entry(&mut app.ctx);
    let second = last(&app);
    assert!(second.starts_with("Stop already settled."), "{second}");
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before - expected,
        "the money moved exactly once"
    );
}

#[test]
fn test_a_new_driver_is_let_off_a_minor_stop_with_a_warning() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let reputation = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .career
        .reputation;
    let mut state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, params("Lane misuse", false, false))
    });

    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.money(), money_before);
    assert_eq!(p.career.reputation, reputation);
    assert_eq!(p.driving_record.citations, 0);
    app.clear_speech();
    state.announce_entry(&mut app.ctx);
    let line = last(&app);
    assert!(line.contains("lets you go with a warning"), "{line}");
}

#[test]
fn test_an_out_of_service_order_passes_the_ten_hours_on_the_shoulder() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    // Past the 11-hour drive: that is a full reset, not a missed-break park.
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(11.0 * 60.0 + 1.0);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, params("Hours violation", true, false))
    });
    assert!(
        state
            .outcome_text()
            .contains("Out of service: ten hours parked on the shoulder"),
        "{}",
        state.outcome_text()
    );
    assert_eq!(
        with_drive(&drive, |d| d.trip.game_minutes),
        before + hos::SLEEP_MIN
    );
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").hos.driving_min,
        0.0
    );
}

#[test]
fn test_a_missed_break_is_thirty_minutes_out_of_service() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    app.ctx.profile.as_mut().expect("a career").hos.drive(481.0);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, params("Hours violation", true, false))
    });
    assert!(
        state
            .outcome_text()
            .contains("Out of service: thirty minutes parked on the shoulder"),
        "{}",
        state.outcome_text()
    );
    assert!(
        !state.outcome_text().contains("ten hours"),
        "{}",
        state.outcome_text()
    );
    assert_eq!(
        with_drive(&drive, |d| d.trip.game_minutes),
        before + hos::BREAK_MIN
    );
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").hos.driving_min,
        481.0
    );
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .hos
            .since_break_min,
        0.0
    );
}

#[test]
fn test_a_scale_bypass_is_inspected_on_the_shoulder_instead() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, params("Weigh station bypass", false, true))
    });
    assert!(
        state
            .outcome_text()
            .contains("full inspection runs here on the shoulder"),
        "{}",
        state.outcome_text()
    );
    assert!(with_drive(&drive, |d| d.trip.game_minutes) > before);
}

// -- the felony stop -------------------------------------------------------------------------

#[test]
fn test_the_felony_stop_cancels_the_load_and_releases_to_the_terminal() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let damage_before = with_drive(&drive, |d| d.trip.truck.damage_pct);
    let mut state = drive_and_ctx(&drive, &mut app, |d, ctx| FelonyStopState::new(ctx, d));

    assert!(
        state.summary().contains("Troopers laid spike strips"),
        "{}",
        state.summary()
    );
    assert!(
        state.summary().contains("You are released back to"),
        "{}",
        state.summary()
    );
    assert!(app.ctx.profile.as_ref().expect("a career").money() < money_before);
    assert!(
        with_drive(&drive, |d| d.trip.truck.damage_pct)
            >= damage_before + FAILURE_TO_STOP_DAMAGE_PCT - 0.001
    );
    assert!(with_drive(&drive, |d| d.trip.truck.parking_brake));
    assert!(app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .is_none());
    assert_eq!(with_drive(&drive, |d| d.failure_to_stop_count), 1);

    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(rows, vec!["Return to terminal"]);
}

#[test]
fn test_fleeing_a_stop_is_a_major_offense_on_the_licence() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| FelonyStopState::new(ctx, d));
    assert!(
        state.summary().contains("a major offense on your CDL"),
        "{}",
        state.summary()
    );
    let record = &app.ctx.profile.as_ref().expect("a career").driving_record;
    assert!(record.suspended(app.ctx.profile.as_ref().expect("a career").game_hours));
    // The line is restated at settlement, so it goes on the trip record too.
    assert_eq!(with_drive(&drive, |d| d.record_events.len()), 1);
}

// -- a CDL pulled at speed ---------------------------------------------------------------

/// Two serious violations right now: the CDL is suspended.
fn suspend_the_cdl(app: &mut TestApp) {
    let p = app.ctx.profile.as_mut().expect("a career");
    let now = p.game_hours;
    p.driving_record.record_serious_violation(now);
    p.driving_record.record_serious_violation(now);
    assert!(p.driving_record.suspended(now));
}

#[test]
fn test_a_run_off_that_suspends_the_cdl_ends_the_drive() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let home = {
        // One serious violation and one run-off already on the record, so
        // the next run-off is the second serious violation.
        let p = app.ctx.profile.as_mut().expect("a career");
        let now = p.game_hours;
        p.driving_record.record_serious_violation(now);
        p.driving_record.record_fatigue_event(now);
        assert!(!p.driving_record.suspended(now));
        p.current_city.clone()
    };
    with_drive(&drive, |d| d.trip.truck.velocity_mps = 27.0);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.microsleep_misses = 0;
        d.microsleep_drift_off_road(ctx);
    });
    {
        let p = app.ctx.profile.as_ref().expect("a career");
        assert!(p.driving_record.suspended(p.game_hours));
    }
    assert!(top_is::<LicencePulledState>(&app), "the drive carried on");
    assert_eq!(with_drive(&drive, |d| d.trip.truck.velocity_mps), 0.0);
    assert!(with_drive(&drive, |d| d.trip.truck.parking_brake));
    let text = with_top::<LicencePulledState, _>(&app, |s| s.outcome_text().to_string());
    assert!(
        text.starts_with(
            "You pull onto the shoulder and stop. The license is pulled as of now, so the truck \
             stays here."
        ),
        "{text}"
    );
    let rows = with_top_ctx::<LicencePulledState, _>(&mut app, build_labels);
    assert_eq!(rows, vec!["Return to terminal"]);

    // Escape never drives on: it closes the run out like the row does.
    with_top_ctx::<LicencePulledState, _>(&mut app, |s, ctx| s.go_back(ctx));
    assert!(top_is::<CityMenuState>(&app));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.active_trip.is_none());
    assert_eq!(p.current_city, home);
}

#[test]
fn test_a_run_off_that_leaves_the_cdl_clear_drives_on() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.microsleep_misses = 0;
        d.microsleep_drift_off_road(ctx);
    });
    assert!(top_is::<DrivingState>(&app));
}

#[test]
fn test_the_barrels_that_suspend_the_cdl_end_the_drive() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        let now = p.game_hours;
        p.driving_record.record_serious_violation(now);
    }
    let zone = Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(0));
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.position_mi = 6.0;
        d.trip.zones.push(zone.clone());
        d.lane.set_lane_count(2);
        d.cite_barrel_strike(ctx, &zone);
    });
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.driving_record.suspended(p.game_hours));
    assert!(top_is::<LicencePulledState>(&app), "the drive carried on");
}

#[test]
fn test_a_debug_hours_mode_never_ends_a_run_on_a_pulled_cdl() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    suspend_the_cdl(&mut app);
    app.ctx.settings.hos_mode = hos::HOS_NON_ENFORCED_MODES[0].to_string();
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.end_drive_if_licence_pulled(ctx)
    });
    assert!(top_is::<DrivingState>(&app));
}

#[test]
fn test_escape_on_a_traffic_stop_that_pulled_the_licence_ends_the_run() {
    // Escape used to pull back onto the highway, and the drive went on
    // with the CDL suspended.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    suspend_the_cdl(&mut app);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        TrafficStopState::new(ctx, d, false, 24.0, 65.0, false, false, false)
    });
    app.ctx
        .push_shared_with(freight_fate::app::share(state), false);
    with_top_ctx::<TrafficStopState, _>(&mut app, |s, ctx| s.go_back(ctx));
    assert!(top_is::<CityMenuState>(&app), "the traffic stop drove on");
}

#[test]
fn test_escape_on_an_enforcement_stop_that_pulled_the_licence_ends_the_run() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    suspend_the_cdl(&mut app);
    let state = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, params("Lane misuse", false, false))
    });
    app.ctx
        .push_shared_with(freight_fate::app::share(state), false);
    with_top_ctx::<EnforcementStopState, _>(&mut app, |s, ctx| s.go_back(ctx));
    assert!(
        top_is::<CityMenuState>(&app),
        "the enforcement stop drove on"
    );
}

#[test]
fn test_a_saved_trip_on_a_pulled_cdl_closes_out_instead_of_resuming() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let snapshot = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    app.ctx.profile.as_mut().expect("a career").active_trip = Some(snapshot);
    suspend_the_cdl(&mut app);
    app.clear_speech();
    let entry = freight_fate::states::main_menu::world_entry_state(&mut app.ctx, false);
    assert!(entry.borrow().as_any().is::<CityMenuState>());
    assert!(app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_trip
        .is_none());
    assert_eq!(
        app.main_lines(),
        vec![
            "Your saved run cannot go on: dispatch cancels it, and a relief driver brings the \
             truck back."
        ]
    );
}
