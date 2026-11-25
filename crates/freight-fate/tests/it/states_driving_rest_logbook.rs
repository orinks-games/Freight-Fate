use ff_core::sim::hos::{DutyLog, HosClock};
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::RestStopState;

use crate::states_driving_menus_support::{
    a_drive, activate, sleep_stop, travel_center, with_drive,
};

#[test]
fn sleeper_split_and_motel_rest_use_their_duty_statuses() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let at_mile = with_drive(&drive, |d| d.trip.position_mi);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.game_hours = 100.0;
        profile.hos = HosClock::new();
        profile.hos.drive(600.0);
        profile.fatigue = 80.0;
        profile.duty_log = DutyLog::new();
    }

    let mut expected_hos = app.ctx.profile.as_ref().unwrap().hos.clone();
    expected_hos.sleeper_split_rest(120.0);
    let mut sleeper = RestStopState::with_drive(DriveRef::of(&drive), sleep_stop(at_mile), true);
    activate(&mut sleeper, &mut app.ctx, "Sleep 2 hours in sleeper berth");
    activate(&mut sleeper, &mut app.ctx, "Sleep 2 hours in sleeper berth");

    let profile = app.ctx.profile.as_ref().unwrap();
    assert_eq!(profile.hos, expected_hos);
    assert_eq!(profile.duty_log.segments.len(), 1);
    assert_eq!(profile.duty_log.segments[0].status, "sleeper_berth");
    assert_eq!(profile.duty_log.segments[0].start_hour, 100.0);
    assert_eq!(profile.duty_log.segments[0].end_hour, 102.0);
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), 120.0);

    let mut expected_hos = profile.hos.clone();
    expected_hos.sleep();
    let mut motel_stop = travel_center("Motel Logbook Stop", at_mile);
    motel_stop.actions = vec!["break".to_string()];
    let mut motel = RestStopState::with_drive(DriveRef::of(&drive), motel_stop, false);
    activate(&mut motel, &mut app.ctx, "Motel room");
    activate(&mut motel, &mut app.ctx, "Motel room");

    let profile = app.ctx.profile.as_ref().unwrap();
    assert_eq!(profile.hos, expected_hos);
    assert_eq!(profile.duty_log.segments.len(), 2);
    assert_eq!(profile.duty_log.segments[1].status, "off_duty");
    assert_eq!(profile.duty_log.segments[1].start_hour, 102.0);
    assert_eq!(profile.duty_log.segments[1].end_hour, 112.0);
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), 720.0);

    let mut expected_hos = profile.hos.clone();
    expected_hos.sleep();
    let mut full_sleep = RestStopState::with_drive(DriveRef::of(&drive), sleep_stop(at_mile), true);
    activate(&mut full_sleep, &mut app.ctx, "Sleep 10 hours");
    activate(&mut full_sleep, &mut app.ctx, "Sleep 10 hours");

    let profile = app.ctx.profile.as_ref().unwrap();
    assert_eq!(profile.hos, expected_hos);
    assert_eq!(profile.duty_log.segments.len(), 3);
    assert_eq!(profile.duty_log.segments[2].status, "sleeper_berth");
    assert_eq!(profile.duty_log.segments[2].start_hour, 112.0);
    assert_eq!(profile.duty_log.segments[2].end_hour, 122.0);
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), 1320.0);
    app.shutdown();
}

#[test]
fn poor_lot_sleep_remains_off_duty_and_resets_hos() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let at_mile = with_drive(&drive, |d| d.trip.position_mi);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.game_hours = 200.0;
        profile.hos = HosClock::new();
        profile.hos.drive(600.0);
        profile.fatigue = 80.0;
        profile.duty_log = DutyLog::new();
    }
    let mut expected_hos = app.ctx.profile.as_ref().unwrap().hos.clone();
    expected_hos.sleep();

    let mut stop = travel_center("Poor Rest Logbook Stop", at_mile);
    stop.actions = vec!["break".to_string()];
    let mut state = RestStopState::with_drive(DriveRef::of(&drive), stop, false);
    activate(&mut state, &mut app.ctx, "Sleep 10 hours in the lot");
    activate(&mut state, &mut app.ctx, "Sleep 10 hours in the lot");

    let profile = app.ctx.profile.as_ref().unwrap();
    assert_eq!(profile.hos, expected_hos);
    assert_eq!(profile.duty_log.segments.len(), 1);
    assert_eq!(profile.duty_log.segments[0].status, "off_duty");
    assert_eq!(profile.duty_log.segments[0].start_hour, 200.0);
    assert_eq!(profile.duty_log.segments[0].end_hour, 210.0);
    app.shutdown();
}
