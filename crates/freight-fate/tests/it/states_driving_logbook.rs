use crate::states_driving_menus_support::{a_drive, drive_and_ctx};
use ff_core::models::profile::Profile;
use ff_core::sim::hos::{DutyLog, HosClock};
use ff_core::sim::trip_models::{NavigationCue, RoadStop};
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::driving_core::{
    advance_on_duty_clock, advance_on_duty_clock_at, advance_rest_clock, hos_mut_of, profile_of,
    record_driving_frame,
};
use freight_fate::states::driving_menu_states::{ArrivalState, DriveRef};
use freight_fate::states::driving_rest_states::{RestStopState, ShortStay};
use freight_fate::states::driving_team::TeamMode;
use freight_fate::states::driving_turns::TURN_MISS_LOOP_MIN;
use freight_fate::states::logbook::{logbook_lines, LogbookState};

#[test]
fn test_logbook_records_driving_break_wait_and_unloading_once() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let (hos_before_break, fatigue_before_break) = drive_and_ctx(&drive, &mut app, |d, ctx| {
        let profile = ctx.profile.as_mut().unwrap();
        profile.game_hours = 100.0;
        profile.duty_log = DutyLog::new();
        profile.hos = HosClock::new();
        profile.fatigue = 50.0;
        d.trip.game_minutes = 0.0;

        d.trip.truck.velocity_mps = 30.0 / 2.23694;
        record_driving_frame(d, ctx, 100.0, 101.0);
        d.trip.game_minutes += 60.0;
        hos_mut_of(ctx).drive(60.0);
        (profile_of(ctx).hos.clone(), profile_of(ctx).fatigue)
    });
    let mut stop = RestStopState::with_drive(
        DriveRef::of(&drive),
        RoadStop::new("Ledger test stop", 0.0, "truck_stop"),
        false,
    );
    stop.take_break(&mut app.ctx);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        assert_eq!(profile_of(ctx).hos.driving_min, 60.0);
        assert_eq!(profile_of(ctx).hos.since_break_min, 0.0);
        assert_eq!(profile_of(ctx).hos.duty_min, 90.0);
        assert_eq!(
            profile_of(ctx).fatigue,
            ff_core::sim::hos::rest_break(fatigue_before_break)
        );
        assert_eq!(
            profile_of(ctx).hos.driving_min,
            hos_before_break.driving_min
        );

        record_driving_frame(d, ctx, 101.5, 103.5);
        d.trip.game_minutes += 120.0;
        hos_mut_of(ctx).drive(120.0);
        advance_on_duty_clock(d, ctx, 15.0, Some("on_duty_not_driving"), "roadside wait");
        record_driving_frame(d, ctx, 103.75, 105.25);
        d.trip.game_minutes += 90.0;
        hos_mut_of(ctx).drive(90.0);
        let arrival_hours = d.trip.game_minutes / 60.0;
        advance_on_duty_clock_at(
            d,
            ctx,
            30.0,
            None,
            "receiver unloading",
            "Rochester destination facility",
        );
        let hos_after_unloading = profile_of(ctx).hos.clone();
        ArrivalState::new_at(ctx, d, arrival_hours);
        assert_eq!(profile_of(ctx).hos, hos_after_unloading);

        let profile = profile_of(ctx);
        let end_hour = profile.game_hours;
        let totals = profile.duty_log.totals_since(96.0, end_hour);
        assert!((totals.driving - 4.5).abs() < 1e-9);
        assert!((totals.on_duty_not_driving - 0.75).abs() < 1e-9);
        assert!((totals.off_duty - 0.5).abs() < 1e-9);
        assert_eq!(totals.sleeper_berth, 0.0);

        let rows = &profile.duty_log.segments;
        let expected = [
            ("driving", 100.0, 101.0),
            ("off_duty", 101.0, 101.5),
            ("driving", 101.5, 103.5),
            ("on_duty_not_driving", 103.5, 103.75),
            ("driving", 103.75, 105.25),
            ("on_duty_not_driving", 105.25, 105.75),
        ];
        assert_eq!(rows.len(), expected.len(), "{rows:?}");
        for (row, (status, start, end)) in rows.iter().zip(expected) {
            assert_eq!(row.status, status);
            assert_eq!(row.start_hour, start);
            assert_eq!(row.end_hour, end);
        }
        let last = rows.last().unwrap();
        assert_eq!(last.note, "receiver unloading");
        assert!(last.location.contains("Rochester"), "{last:?}");

        let saved_rows = profile_of(ctx).duty_log.segments.clone();
        let serialized = profile_of(ctx).to_dict();
        let reloaded = Profile::from_dict(&serialized);
        assert_eq!(reloaded.duty_log.segments, saved_rows);
        *ctx.profile.as_mut().unwrap() = reloaded;
        advance_on_duty_clock_at(
            d,
            ctx,
            0.0,
            None,
            "receiver unloading",
            "Rochester destination facility",
        );
        assert_eq!(profile_of(ctx).duty_log.segments, saved_rows);

        let lines = logbook_lines(ctx, None);
        let current = lines.first().unwrap();
        assert!(
            current.starts_with("On duty, not driving since "),
            "{current}"
        );
        assert!(current.contains("Rochester"), "{current}");
        let today = lines
            .iter()
            .find(|line| line.starts_with("Today: "))
            .unwrap();
        assert!(today.contains("driving 4.5 hours"), "{today}");
        assert!(today.contains("on duty not driving 45 minutes"), "{today}");
        assert!(today.contains("off duty 30 minutes"), "{today}");

        let mut reopened = LogbookState::new(None);
        let reopened_rows = reopened.build_items(ctx);
        let reopened_lines: Vec<_> = reopened_rows
            .iter()
            .take(lines.len())
            .map(|item| item.text(&reopened, ctx))
            .collect();
        assert_eq!(reopened_lines, lines);
    });
}

#[test]
fn test_normal_frame_records_driving_and_exact_rows_coalesce_without_bridging() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let profile = ctx.profile.as_mut().unwrap();
        profile.game_hours = 100.0;
        profile.duty_log = DutyLog::new();
        d.trip.truck.velocity_mps = 60.0 / 2.23694;
        d.update_frame(ctx, 0.05);
        let rows = &profile_of(ctx).duty_log.segments;
        assert!(rows.iter().any(|row| row.status == "driving"), "{rows:?}");

        let mut log = DutyLog::new();
        for index in 0..1000 {
            let start = 200.0 + index as f64 / 60.0;
            log.record_exact("driving", start, start + 1.0 / 60.0, "route", "");
        }
        assert_eq!(log.segments.len(), 1);
        assert!((log.segments[0].duration_hours() - 1000.0 / 60.0).abs() < 1e-8);
        log.record_exact("driving", 220.0, 221.0, "route", "");
        assert_eq!(log.segments.len(), 2, "{log:?}");
        assert!((log.segments[0].end_hour - 216.66666666666666).abs() < 1e-8);
        assert_eq!(log.segments[1].start_hour, 220.0);

        let mut overlapping = DutyLog::new();
        overlapping.record_exact("driving", 300.0, 301.0, "route", "");
        overlapping.record_exact("driving", 300.0, 301.0, "route", "");
        overlapping.record_exact("off_duty", 300.5, 301.5, "route", "");
        assert_eq!(overlapping.segments.len(), 2, "{overlapping:?}");
        assert_eq!(overlapping.segments[1].start_hour, 301.0);
        let totals = overlapping.totals_since(300.0, 301.5);
        assert_eq!(totals.driving, 1.0);
        assert_eq!(totals.off_duty, 0.5);
    });
}

#[test]
fn test_missed_turn_records_scripted_driving_after_the_frame_once() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let profile = ctx.profile.as_mut().unwrap();
        profile.game_hours = 100.0;
        profile.duty_log = DutyLog::new();
        d.trip.game_minutes = 6.0;
        d.trip.truck.velocity_mps = 45.0 / 2.23694;
        record_driving_frame(d, ctx, 100.0, 100.1);

        let hos_before = profile_of(ctx).hos.clone();
        let cue = NavigationCue::new(
            "logbook-turn:0",
            "street_turn",
            d.trip.position_mi,
            "Turn onto the next street",
            "",
        );
        d.handle_missed_turn(ctx, &cue);

        assert_eq!(d.trip.game_minutes, 6.0 + TURN_MISS_LOOP_MIN);
        assert_eq!(profile_of(ctx).hos, hos_before);
        let duty_log = &profile_of(ctx).duty_log;
        assert_eq!(duty_log.segments.len(), 1, "{:?}", duty_log.segments);
        assert_eq!(duty_log.segments[0].status, "driving");
        assert_eq!(duty_log.segments[0].start_hour, 100.0);
        let expected_end = 100.1 + TURN_MISS_LOOP_MIN / 60.0;
        assert!((duty_log.segments[0].end_hour - expected_end).abs() < 1e-9);
        let totals = duty_log.totals_since(100.0, 101.0);
        assert!((totals.driving - (0.1 + TURN_MISS_LOOP_MIN / 60.0)).abs() < 1e-9);
    });
    app.shutdown();
}

#[test]
fn test_fuel_and_sleep_gaps_keep_their_own_duty_status() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let profile = ctx.profile.as_mut().unwrap();
        profile.game_hours = 100.0;
        profile.duty_log = DutyLog::new();
        d.trip.game_minutes = 0.0;
        d.trip.truck.velocity_mps = 30.0 / 2.23694;

        record_driving_frame(d, ctx, 100.0, 101.0);
        d.trip.game_minutes = 60.0;
        advance_on_duty_clock(d, ctx, 30.0, None, "fuel");
        record_driving_frame(d, ctx, 101.5, 102.5);
        d.trip.game_minutes += 60.0;
        advance_rest_clock(d, ctx, 60.0, None, "sleep");
        record_driving_frame(d, ctx, 103.5, 104.5);

        let rows = &profile_of(ctx).duty_log.segments;
        assert_eq!(rows.len(), 5, "{rows:?}");
        assert_eq!(
            rows.iter()
                .map(|row| row.status.as_str())
                .collect::<Vec<_>>(),
            [
                "driving",
                "on_duty_not_driving",
                "driving",
                "off_duty",
                "driving",
            ]
        );
        assert_eq!(rows[1].start_hour, 101.0);
        assert_eq!(rows[1].end_hour, 101.5);
        assert_eq!(rows[3].start_hour, 102.5);
        assert_eq!(rows[3].end_hour, 103.5);
        assert_eq!(rows[0].end_hour, 101.0);
        assert_eq!(rows[2].end_hour, 102.5);
    });
}

#[test]
fn test_co_driver_handover_splits_player_duty_rows() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let profile = ctx.profile.as_mut().unwrap();
        profile.game_hours = 100.0;
        profile.duty_log = DutyLog::new();
        d.trip.truck.velocity_mps = 30.0 / 2.23694;
        record_driving_frame(d, ctx, 100.0, 101.0);
        d.trip.game_minutes = 60.0;
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        record_driving_frame(d, ctx, 101.0, 102.0);
        assert!(d.co_driver_driving());
        d.trip.game_minutes = 120.0;
        d.take_wheel_back(ctx);
        record_driving_frame(d, ctx, 102.0, 103.0);

        let rows = &profile_of(ctx).duty_log.segments;
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert_eq!(rows[0].status, "driving");
        assert_eq!(rows[1].status, "sleeper_berth");
        assert_eq!(rows[2].status, "driving");
        assert_eq!(rows[1].start_hour, 101.0);
        assert_eq!(rows[1].end_hour, 102.0);
    });
}

#[test]
fn test_empty_checkpoint_and_bobtail_arrival_do_not_log_unloading() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let profile = ctx.profile.as_mut().unwrap();
        profile.game_hours = 100.0;
        profile.duty_log = DutyLog::new();
        profile.hos = HosClock::new();
        d.trip.game_minutes = 0.0;
        record_driving_frame(d, ctx, 100.0, 100.0);
        assert!(profile_of(ctx).duty_log.segments.is_empty());

        d.job.bobtail = true;
        d.trip.truck.velocity_mps = 30.0 / 2.23694;
        record_driving_frame(d, ctx, 100.0, 101.0);
        d.trip.game_minutes = 60.0;
        hos_mut_of(ctx).drive(60.0);
        ArrivalState::new(ctx, d);
        let rows = &profile_of(ctx).duty_log.segments;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].status, "driving");
        assert!(rows.iter().all(|row| row.note != "receiver unloading"));
    });
}
