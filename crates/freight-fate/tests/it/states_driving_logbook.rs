use crate::states_driving_menus_support::{a_drive, drive_and_ctx};
use ff_core::sim::hos::{DutyLog, HosClock};
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving_core::{
    advance_on_duty_clock, advance_rest_clock, hos_mut_of, log_driving_until, profile_mut_of,
    profile_of,
};
use freight_fate::states::driving_menu_states::ArrivalState;
use freight_fate::states::logbook::logbook_lines;

#[test]
fn test_logbook_records_driving_break_wait_and_unloading_once() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        let profile = ctx.profile.as_mut().unwrap();
        profile.game_hours = 100.0;
        profile.duty_log = DutyLog::new();
        profile.hos = HosClock::new();
        profile.fatigue = 50.0;
        d.trip.game_minutes = 0.0;

        d.trip.game_minutes += 60.0;
        hos_mut_of(ctx).drive(60.0);
        let hos_before_break = profile_of(ctx).hos.clone();
        let fatigue_before_break = profile_of(ctx).fatigue;
        advance_rest_clock(d, ctx, 30.0, Some("off_duty"), "");
        assert_eq!(profile_of(ctx).hos, hos_before_break);
        assert_eq!(profile_of(ctx).fatigue, fatigue_before_break);
        hos_mut_of(ctx).take_break(30.0);
        assert_eq!(profile_of(ctx).hos.driving_min, 60.0);
        assert_eq!(profile_of(ctx).hos.since_break_min, 0.0);
        assert_eq!(profile_of(ctx).hos.duty_min, 90.0);
        profile_mut_of(ctx).fatigue = ff_core::sim::hos::rest_break(fatigue_before_break);

        d.trip.game_minutes += 120.0;
        hos_mut_of(ctx).drive(120.0);
        advance_on_duty_clock(d, ctx, 15.0, Some("on_duty_not_driving"), "roadside wait");
        d.trip.game_minutes += 90.0;
        hos_mut_of(ctx).drive(90.0);
        let arrival_hours = d.trip.game_minutes / 60.0;
        d.trip.game_minutes += 30.0;
        let hos_before_delivery = profile_of(ctx).hos.clone();
        ArrivalState::new_at(ctx, d, arrival_hours);
        assert_eq!(profile_of(ctx).hos, hos_before_delivery);

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

        let serialized = profile.duty_log.to_dict();
        let reloaded = DutyLog::from_dict(&serialized);
        assert_eq!(reloaded.segments, profile.duty_log.segments);
        profile_mut_of(ctx).duty_log = reloaded;
        let saved_rows = profile_of(ctx).duty_log.segments.clone();
        log_driving_until(d, ctx, 105.25);
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
        log_driving_until(d, ctx, 100.0);
        assert!(profile_of(ctx).duty_log.segments.is_empty());

        d.job.bobtail = true;
        d.trip.game_minutes = 60.0;
        hos_mut_of(ctx).drive(60.0);
        ArrivalState::new(ctx, d);
        let rows = &profile_of(ctx).duty_log.segments;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].status, "driving");
        assert!(rows.iter().all(|row| row.note != "receiver unloading"));
    });
}
