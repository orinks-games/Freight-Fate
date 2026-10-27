//! Cruise and the speed keeper hold chain speed while chains are on.

use freight_fate::app::testing::TestApp;
use freight_fate::states::driving_core::CHAIN_SAFE_MPH;

use crate::states_driving_menus_support::*;

const MPS_PER_MPH: f64 = 0.44704;

#[test]
fn test_cruise_holds_chain_speed_and_gives_it_back() {
    let mut app = TestApp::new();
    app.ctx.settings.imperial_units = true;
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.engine_on = true;
        d.trip.truck.velocity_mps = 50.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 55.0, false);
    });
    assert_eq!(with_drive(&drive, |d| d.cruise_mph), Some(55.0));
    app.clear_speech();

    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.chains_on = true;
        d.apply_chain_speed_cap(ctx);
        // Every frame: once said, it stays quiet.
        d.apply_chain_speed_cap(ctx);
    });
    app.ctx.run_deferred();
    assert_eq!(with_drive(&drive, |d| d.cruise_mph), Some(CHAIN_SAFE_MPH));
    let said: Vec<String> = app
        .event_lines()
        .into_iter()
        .filter(|s| s.contains("for chains"))
        .collect();
    assert_eq!(
        said,
        vec!["Cruise held to 30 miles per hour for chains.".to_string()]
    );

    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.chains_on = false;
        d.apply_chain_speed_cap(ctx);
    });
    assert_eq!(with_drive(&drive, |d| d.cruise_mph), Some(55.0));
}

#[test]
fn test_a_cruise_resumed_with_chains_on_is_capped_again() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.engine_on = true;
        d.trip.truck.chains_on = true;
        d.trip.truck.velocity_mps = 28.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 55.0, false);
        d.apply_chain_speed_cap(ctx);
    });
    assert_eq!(with_drive(&drive, |d| d.cruise_mph), Some(CHAIN_SAFE_MPH));
    // The remembered target stays the driver's own number.
    assert_eq!(
        with_drive(&drive, |d| d.speed_control_target_mph),
        Some(55.0)
    );
}

#[test]
fn test_the_speed_keeper_holds_chain_speed() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.chains_on = true;
        d.keeper_mph = Some(45.0);
        d.apply_chain_speed_cap(ctx);
    });
    assert_eq!(with_drive(&drive, |d| d.keeper_mph), Some(CHAIN_SAFE_MPH));
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.chains_on = false;
        d.apply_chain_speed_cap(ctx);
    });
    assert_eq!(with_drive(&drive, |d| d.keeper_mph), Some(45.0));
}

#[test]
fn test_a_cruise_cancelled_while_chained_resumes_the_drivers_own_speed() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.engine_on = true;
        d.trip.truck.velocity_mps = 50.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 55.0, false);
        d.trip.truck.chains_on = true;
        d.apply_chain_speed_cap(ctx);
        d.disarm_speed_control(ctx);
    });
    assert_eq!(with_drive(&drive, |d| d.cruise_mph), None);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.chains_on = false;
        d.apply_chain_speed_cap(ctx);
    });
    assert_eq!(with_drive(&drive, |d| d.resume_target_mph), Some(55.0));
}
