use super::*;

fn wear(d: &mut DrivingState, component: usize, value: f64) {
    match component {
        0 => d.trip.truck.tire_wear_pct = value,
        1 => d.trip.truck.brake_wear_pct = value,
        _ => d.trip.truck.engine_wear_pct = value,
    }
}

#[test]
fn maintenance_each_component_warns_once_and_rearms_after_service() {
    for (component, label, effect, action) in [
        (0, "Tires", "reduce grip", "Replace the tires in the garage"),
        (
            1,
            "Brakes",
            "reduce stopping force",
            "Have the brakes relined in the garage",
        ),
        (
            2,
            "Engine",
            "loses power",
            "Get an engine overhaul in the garage",
        ),
    ] {
        let mut app = TestApp::new();
        let clock = app.fake_pacer_clock();
        let mut d = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
        rolling(&mut d, 30.0);
        wear(&mut d, component, 79.99);
        let from = app.ctx.message_log.messages.len();
        d.update_damage_bands(&mut app.ctx, 0.1);
        assert!(!logged_since(&app, from)
            .iter()
            .any(|s| s.contains("Service soon")));
        wear(&mut d, component, 80.0);
        d.update_damage_bands(&mut app.ctx, 0.1);
        d.update_damage_bands(&mut app.ctx, 0.1);
        let lines = logged_since(&app, from);
        assert_eq!(
            lines
                .iter()
                .filter(|s| s.contains("Service soon") && s.contains(label))
                .count(),
            1
        );
        let warning = lines
            .iter()
            .find(|s| s.contains("Service soon"))
            .expect("the warning was logged");
        assert!(warning.contains(effect), "{warning}");
        assert!(warning.contains(action), "{warning}");
        assert!(warning.contains("100 percent"), "{warning}");
        clock.advance(30.0);
        d.update_damage_bands(&mut app.ctx, 0.1);
        wear(&mut d, component, 0.0);
        d.update_damage_bands(&mut app.ctx, 0.1);
        wear(&mut d, component, 80.0);
        d.update_damage_bands(&mut app.ctx, 0.1);
        let repeated = app.event_lines();
        assert_eq!(
            repeated
                .iter()
                .filter(|s| s.contains("Service soon") && s.contains(label))
                .count(),
            2,
            "component {component}: {repeated:?}"
        );
    }
}

#[test]
fn maintenance_warning_does_not_repeat_after_resume() {
    for component in 0..3 {
        let mut app = TestApp::new();
        let clock = app.fake_pacer_clock();
        let mut d = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
        rolling(&mut d, 30.0);
        wear(&mut d, component, 80.0);
        d.update_damage_bands(&mut app.ctx, 0.1);
        clock.advance(30.0);
        d.update_damage_bands(&mut app.ctx, 0.1);
        app.ctx
            .profile
            .as_mut()
            .unwrap()
            .store_truck_condition(&d.trip.truck);
        let data = d.snapshot(&app.ctx);
        let from = app.ctx.message_log.messages.len();

        let mut resumed = DrivingState::from_snapshot(&mut app.ctx, &data).unwrap();
        resumed.update_damage_bands(&mut app.ctx, 0.1);

        assert!(
            !logged_since(&app, from)
                .iter()
                .any(|line| line.contains("Service soon")),
            "component {component} repeated after resume"
        );
    }
}

#[test]
fn simultaneous_maintenance_warnings_finish_one_at_a_time() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    rolling(&mut d, 30.0);
    for component in 0..3 {
        wear(&mut d, component, 80.0);
    }

    for _ in 0..4 {
        d.update_damage_bands(&mut app.ctx, 0.1);
        clock.advance(30.0);
    }
    d.update_damage_bands(&mut app.ctx, 0.1);

    assert_eq!(d.maintenance_levels, [1; 3]);
    assert_eq!(d.maintenance_pending_levels, [0; 3]);
    let warnings: Vec<_> = app
        .event_lines()
        .into_iter()
        .filter(|line| line.contains("Service soon"))
        .collect();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
}

#[test]
fn paused_unheard_maintenance_warning_retries_after_saved_resume() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Interrupted maintenance warning"));
    harness.with_drive(|drive, ctx| {
        drive.trip.truck.velocity_mps = 30.0 / 2.23694;
        drive.trip.truck.brake_wear_pct = 80.0;
        drive.update_damage_bands(ctx, 0.1);
    });
    assert!(harness
        .app
        .event_lines()
        .iter()
        .any(|line| { line.contains("Brakes") && line.contains("Service soon") }));

    harness.key(freight_fate::states::base::InputEvent::key(
        freight_fate::states::base::Key::Escape,
    ));
    let snapshot = harness.with_drive(|drive, ctx| {
        ctx.profile
            .as_mut()
            .unwrap()
            .store_truck_condition(&drive.trip.truck);
        drive.snapshot(ctx)
    });

    harness.clear_speech();
    let mut resumed = DrivingState::from_snapshot(&mut harness.app.ctx, &snapshot).unwrap();
    resumed.update_damage_bands(&mut harness.app.ctx, 0.1);
    assert!(harness
        .app
        .event_lines()
        .iter()
        .any(|line| { line.contains("Brakes") && line.contains("Service soon") }));
}

#[test]
fn maintenance_limits_recover_every_component_for_both_business_models() {
    for business in [LEASED_OWNER_OPERATOR, COMPANY_DRIVER] {
        for component in 0..3 {
            let mut app = TestApp::new();
            let mut d = a_damage_drive(&mut app, business, 9);
            wear(&mut d, component, 99.99);
            assert!(!d.trip.truck.out_of_service());
            wear(&mut d, component, 100.0);
            assert!(d.trip.truck.out_of_service());
            rolling(&mut d, 20.0);
            let from = app.ctx.message_log.messages.len();
            d.update_damage_bands(&mut app.ctx, 0.1);
            assert!(d.trip.truck.out_of_service(), "time to clear lane");
            assert!(d.trip.truck.speed_cap_mph.is_some());
            let failure = ["Tire wear", "Brake wear", "Engine wear"][component];
            assert!(
                logged_since(&app, from)
                    .iter()
                    .any(|line| line.contains(failure) && line.contains("100 percent")),
                "missing limit explanation for {failure}"
            );
            let money = app.ctx.profile.as_ref().unwrap().money();
            d.trip.truck.velocity_mps = 0.0;
            d.update_damage_bands(&mut app.ctx, 0.1);
            assert!(!d.trip.truck.out_of_service());
            assert!(d.trip.truck.parking_brake);
            assert!(!d.trip.truck.engine_on);
            assert!(d.trip.truck.speed_cap_mph.is_none());
            let after = app.ctx.profile.as_ref().unwrap().money();
            if business == COMPANY_DRIVER {
                assert_eq!(money, after);
            } else {
                assert!(after < money);
            }
        }
    }
}

#[test]
fn maintenance_resume_keeps_endpoint_and_recovers_all_failed_components() {
    let mut app = TestApp::new();
    let mut d = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    for c in 0..3 {
        wear(&mut d, c, 100.0);
    }
    rolling(&mut d, 15.0);
    app.ctx
        .profile
        .as_mut()
        .unwrap()
        .store_truck_condition(&d.trip.truck);
    let data = d.snapshot(&app.ctx);
    let mut resumed = DrivingState::from_snapshot(&mut app.ctx, &data).unwrap();
    assert!(resumed.trip.truck.out_of_service());
    resumed.trip.truck.velocity_mps = 0.0;
    resumed.update_damage_bands(&mut app.ctx, 0.1);
    assert!(!resumed.trip.truck.out_of_service());
    assert_eq!(resumed.trip.truck.tire_wear_pct, 0.0);
    assert_eq!(resumed.trip.truck.brake_wear_pct, 0.0);
    assert_eq!(resumed.trip.truck.engine_wear_pct, 0.0);
    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("replaced the tires"), "{spoken}");
    assert!(spoken.contains("relined the brakes"), "{spoken}");
    assert!(spoken.contains("overhauled the engine"), "{spoken}");
}

#[test]
fn maintenance_terminal_rows_name_service_limit_and_repeat_by_keyboard() {
    use crate::states_city_support::{key, move_to, select};
    use freight_fate::states::base::Key;
    use freight_fate::states::city::{CityMenuState, TruckStatusState};
    for (component, row, action) in [
        (
            0,
            "Tires: service required",
            "Replace the tires in the garage",
        ),
        (
            1,
            "Brakes: service required",
            "Have the brakes relined in the garage",
        ),
        (
            2,
            "Engine: service required",
            "Get an engine overhaul in the garage",
        ),
    ] {
        let mut app = TestApp::new();
        let _d = a_damage_drive(&mut app, COMPANY_DRIVER, 9);
        match component {
            0 => app.ctx.profile.as_mut().unwrap().set_tire_wear_pct(100.0),
            1 => app.ctx.profile.as_mut().unwrap().set_brake_wear_pct(100.0),
            _ => app.ctx.profile.as_mut().unwrap().set_engine_wear_pct(100.0),
        }
        let city = CityMenuState::new(&app.ctx, false);
        app.push_state(city);
        select::<CityMenuState>(&mut app, "Truck status");
        move_to::<TruckStatusState>(&mut app, row);
        app.clear_speech();
        key(&mut app, Key::Return);
        assert!(
            app.main_lines()
                .iter()
                .any(|line| line.contains(row) && line.contains(action)),
            "missing repeatable action for {row}"
        );
        key(&mut app, Key::Escape);
        assert!(crate::states_city_support::is::<CityMenuState>(&app));
    }
}

#[test]
fn maintenance_warning_is_repeatable_from_the_in_drive_status_screen() {
    use crate::states_city_support::{key, move_to};
    use freight_fate::states::base::Key;

    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Maintenance Readout"));
    harness.with_drive(|drive, _| drive.trip.truck.brake_wear_pct = 80.0);
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
    harness
        .app
        .push_state(DrivingStatusScreenState::new(handle, "road"));
    move_to::<DrivingStatusScreenState>(&mut harness.app, "Brakes: 80 percent worn");
    harness.app.clear_speech();
    key(&mut harness.app, Key::Return);
    assert!(harness.app.main_lines().iter().any(|line| {
        line.contains("Brakes: 80 percent worn")
            && line.contains("Service is required at 100 percent")
            && line.contains("garage")
    }));
}

#[test]
fn maintenance_recovery_preserves_unfailed_components_and_ends_creep_grace() {
    let mut app = TestApp::new();
    let mut d = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    d.trip.truck.brake_wear_pct = 100.0;
    d.trip.truck.tire_wear_pct = 42.0;
    d.trip.truck.engine_wear_pct = 63.0;
    app.ctx.profile.as_mut().unwrap().set_money(0.0);
    rolling(&mut d, 25.0);
    d.update_damage_bands(&mut app.ctx, OUT_OF_SERVICE_RECOVERY_GRACE_S);
    assert!(!d.trip.truck.out_of_service());
    assert_eq!(d.trip.truck.brake_wear_pct, 0.0);
    assert_eq!(d.trip.truck.tire_wear_pct, 42.0);
    assert_eq!(d.trip.truck.engine_wear_pct, 63.0);
    assert!(app.ctx.profile.as_ref().unwrap().money() < 0.0);
    assert!(app.event_lines().iter().any(|line| line.contains("debt")));
    assert!(d.trip.truck.parking_brake);
}

#[test]
fn required_service_stays_reachable_in_the_garage_without_cash() {
    use freight_fate::states::city::GarageState;

    for business in [LEASED_OWNER_OPERATOR, COMPANY_DRIVER] {
        for component in 0..3 {
            let mut app = TestApp::new();
            let _d = a_damage_drive(&mut app, business, 1);
            app.ctx.profile.as_mut().unwrap().set_money(0.0);
            match component {
                0 => app.ctx.profile.as_mut().unwrap().set_tire_wear_pct(100.0),
                1 => app.ctx.profile.as_mut().unwrap().set_brake_wear_pct(100.0),
                _ => app.ctx.profile.as_mut().unwrap().set_engine_wear_pct(100.0),
            }
            let mut garage = GarageState::new();
            match component {
                0 => garage.service_tires(&mut app.ctx),
                1 => garage.service_brakes(&mut app.ctx),
                _ => garage.service_engine(&mut app.ctx),
            }
            let profile = app.ctx.profile.as_ref().unwrap();
            let remaining = [
                profile.tire_wear_pct(),
                profile.brake_wear_pct(),
                profile.engine_wear_pct(),
            ][component];
            assert_eq!(remaining, 0.0);
            if business == COMPANY_DRIVER {
                assert_eq!(profile.money(), 0.0);
            } else {
                assert!(profile.money() < 0.0);
                assert!(app.main_lines().iter().any(|line| line.contains("debt")));
            }
        }
    }
}
