//! The Driving hints setting: on, driving lines name the key for the next
//! step until the player has used it a few times; off, they never do.

use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;

fn a_drive(app: &mut TestApp) -> DrivingState {
    app.ctx.profile = Some(Profile::named_in("Hints", "Buffalo"));
    let route = app
        .ctx
        .world
        .supported_route("Buffalo", "Rochester", None)
        .unwrap()
        .expect("Buffalo to Rochester is supported");
    let miles = route.miles();
    let job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        miles,
        1000.0,
        12.0,
    );
    DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_DELIVERY, None)
}

#[test]
fn hints_on_name_the_key_and_hints_off_never_do() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.engine_on = false;

    assert!(app.ctx.settings.driving_hints);
    d.refresh_exit_hint(&mut app.ctx);
    assert!(!d.trip.exit_hint.is_empty());
    assert!(d
        .engine_entry_instruction(&mut app.ctx)
        .starts_with("Press "));

    app.ctx.settings.driving_hints = false;
    d.refresh_exit_hint(&mut app.ctx);
    assert_eq!(d.trip.exit_hint, "");
    assert_eq!(d.engine_entry_instruction(&mut app.ctx), "Engine off.");
    assert_eq!(d.parking_brake_hint_tail(&mut app.ctx), "");
    assert_eq!(d.exit_signal_instruction(), "Signal for it.");
}

#[test]
fn a_long_line_loses_only_its_key_sentence() {
    let mut app = TestApp::new();
    let d = a_drive(&mut app);
    let line = "Road service done for 300 dollars. Press E to restart the engine.";
    let hint = "Press E to restart the engine.";
    assert_eq!(d.without_key_hint(&app.ctx, line.to_string(), hint), line);
    app.ctx.settings.driving_hints = false;
    assert_eq!(
        d.without_key_hint(&app.ctx, line.to_string(), hint),
        "Road service done for 300 dollars."
    );
    assert_eq!(
        d.without_key_hint(
            &app.ctx,
            "Motel. E starts the engine. Rested.".into(),
            "E starts the engine."
        ),
        "Motel. Rested."
    );
}
