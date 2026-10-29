use ff_core::models::business::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::carrier_fleet::{assigned_truck_key, slip_seat_pool};
use ff_core::sim::trip_models::{RoadStop, TripEvent, TripEventData, TripEventKind};
use ff_core::sim::vehicle::{BrakeApplication, LEGAL_GVW_KG};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::city::{describe_job, JobBoardState, JobDetailState};
use freight_fate::states::driving_core::{
    HazardShape, HAZARD_MIN_REACTION_S, HAZARD_SAFE_MPH, MPH_PER_MPS,
};

#[test]
fn realism_hazard_budgets_keep_service_assist_and_emergency_response_distinct() {
    let mut h = PlaytestHarness::new();
    h.start_delivery(StartDelivery::named("Brake budgets"));
    h.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 25.0;
        d.truck_mut().grade = -0.04;
        d.truck_mut().grip = 0.6;
        d.truck_mut().set_air_ready(false);
        let service = d.brake_budget_s(0.0);
        let emergency = d.emergency_brake_budget_s(0.0);
        assert!(emergency < service);
        let shape = HazardShape {
            dodgeable: false,
            in_lane: false,
            lead_mph: None,
            brake_to_mph: None,
        };
        for scale in [1.0, 10.0, 20.0] {
            d.trip.time_scale = scale;
            let warning = d.hazard_deadline_for(4.0, Some(shape));
            assert!(warning >= d.aeb_engage_s(d.hazard_target_mph(Some(shape))) + 4.0);
            assert!(warning > d.emergency_brake_budget_s(d.hazard_target_mph(Some(shape))) + 4.0);
        }
        d.aeb_brake = 1.0;
        d.apply_hazard_brake();
        assert!(!d.truck().emergency_brake);
        let delivered = d.truck().service_brake_force() / d.truck().gross_mass_kg();
        let estimated = d.truck().braking_decel_mps2(BrakeApplication::Service(1.0));
        assert!((delivered - estimated).abs() < 1e-9);
    });
}

#[test]
fn realism_spoken_hazard_path_keeps_reaction_time_at_every_scale() {
    for scale in [1.0, 10.0, 20.0] {
        let mut h = PlaytestHarness::new();
        h.start_delivery(StartDelivery::named("Hazard reaction"));
        h.with_drive(|d, _| {
            d.trip.time_scale = scale;
            d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
            d.truck_mut().grade = -0.05;
            d.truck_mut().grip = 0.6;
            d.truck_mut().brake_temp_c = 500.0;
        });
        h.clear_speech();
        let event = TripEvent {
            kind: TripEventKind::Hazard,
            message: "Brake now! Stopped traffic ahead.".into(),
            data: TripEventData {
                deadline_s: Some(2.5),
                ..Default::default()
            },
        };
        h.with_drive(|d, ctx| d.handle_trip_event(ctx, &event));

        assert!(h
            .app
            .event_lines()
            .iter()
            .any(|line| line.contains("Stopped traffic ahead")));
        let reaction = h.read_drive(|d| {
            d.hazard_deadline.expect("hazard deadline") - d.aeb_engage_s(HAZARD_SAFE_MPH)
        });
        assert!(
            reaction >= HAZARD_MIN_REACTION_S,
            "{scale}x left {reaction:.2} seconds"
        );
    }
}

#[test]
fn realism_dispatch_preview_reports_fuel_inclusive_weight_margin() {
    let mut h = PlaytestHarness::new();
    h.start_delivery(StartDelivery::named("Weight preview"));
    let mut job = h.read_drive(|d| d.job.clone());
    let profile = h.app.ctx.profile.as_mut().expect("career profile");
    profile.set_truck_fuel_gal(10.0);
    let mut truck = ff_core::sim::vehicle::TruckState::new(profile.truck_specs());
    truck.fuel_gal = 10.0;
    job.weight_tons = (LEGAL_GVW_KG - truck.tare_kg() + 100.0) / 1000.0;
    let expected = "Load weight: 220 pounds over the gross-weight limit with current fuel.";

    let text = describe_job(&h.app.ctx, 1, &job, None);
    assert!(text.contains(expected), "{text}");

    let mut board = JobBoardState::new(&h.app.ctx, vec![job]);
    let rows = crate::states_driving_menus_support::build_labels(&mut board, &mut h.app.ctx);
    assert!(rows.iter().any(|row| row.contains(expected)), "{rows:?}");

    h.app.push_state(board);
    h.app.dispatch_to_state(&InputEvent::key(Key::F1));
    h.app.ctx.run_deferred();
    let details = crate::states_city_support::labels::<JobDetailState>(&h.app);
    assert!(details.iter().any(|line| line == expected), "{details:?}");
}

#[test]
fn realism_dispatch_preview_uses_the_slip_seat_tractor_and_its_fuel() {
    let mut h = PlaytestHarness::new();
    h.start_delivery(StartDelivery::named("Slip-seat weight preview"));
    let mut job = h.read_drive(|d| d.job.clone());
    job.distance_mi = 700.0;
    job.weight_tons = 24.0;

    let profile = h.app.ctx.profile.as_mut().expect("career profile");
    profile.career.xp = LEVEL_XP[3];
    let assigned = assigned_truck_key(profile, Some(&job)).to_string();
    let assigned_tare =
        ff_core::sim::vehicle::TruckState::new(ff_core::models::trucks::build_truck_specs(
            &assigned,
            &ff_core::models::trucks::NO_UPGRADES,
        ))
        .tare_kg();
    let current = slip_seat_pool(profile)
        .into_iter()
        .find(|key| {
            **key != assigned
                && (ff_core::sim::vehicle::TruckState::new(
                    ff_core::models::trucks::build_truck_specs(
                        key,
                        &ff_core::models::trucks::NO_UPGRADES,
                    ),
                )
                .tare_kg()
                    - assigned_tare)
                    .abs()
                    > 1.0
        })
        .expect("a different regional tractor")
        .to_string();
    profile.truck = current.clone();
    profile.provision_truck_condition(&current, Some(200.0));
    profile.provision_truck_condition(&assigned, Some(10.0));

    let mut assigned_truck =
        ff_core::sim::vehicle::TruckState::new(ff_core::models::trucks::build_truck_specs(
            &assigned,
            &ff_core::models::trucks::NO_UPGRADES,
        ));
    assigned_truck.fuel_gal = 10.0;
    job.weight_tons = (LEGAL_GVW_KG - assigned_truck.tare_kg() + 100.0) / 1000.0;
    let expected = "Load weight: 220 pounds over the gross-weight limit with current fuel.";

    let board = JobBoardState::new(&h.app.ctx, vec![job]);
    h.app.push_state(board);
    h.app.dispatch_to_state(&InputEvent::key(Key::F1));
    h.app.ctx.run_deferred();
    let details = crate::states_city_support::labels::<JobDetailState>(&h.app);
    assert!(details.iter().any(|line| line == expected), "{details:?}");
    assert_eq!(
        h.app.ctx.profile.as_ref().unwrap().active_truck_key(),
        current,
        "preview changed the live assignment"
    );

    crate::states_city_support::select::<JobDetailState>(&mut h.app, "Accept");
    assert_eq!(
        h.app.ctx.profile.as_ref().unwrap().active_truck_key(),
        assigned
    );
}

#[test]
fn realism_fuel_rows_report_margin_before_purchase_in_both_roadside_menus() {
    use crate::states_driving_menus_support::{a_drive, activate, build_labels, with_drive};
    use freight_fate::app::testing::TestApp;
    use freight_fate::states::driving_menu_states::DriveRef;
    use freight_fate::states::driving_rest_states::{ParkingFullState, RestStopState};

    for (business, parking_full, margin_kg, side) in [
        (COMPANY_DRIVER, false, 100.0, "under"),
        (COMPANY_DRIVER, true, -10.0, "over"),
        (LEASED_OWNER_OPERATOR, false, -10.0, "over"),
        (LEASED_OWNER_OPERATOR, true, 100.0, "under"),
    ] {
        let mut app = TestApp::new();
        let drive = a_drive(&mut app);
        let profile = app.ctx.profile.as_mut().expect("career profile");
        profile.business_status = business.to_string();
        profile.set_money(50_000.0);
        with_drive(&drive, |d| {
            d.trip.truck.fuel_gal = 10.0;
            let full_tank_gross_without_cargo = d
                .trip
                .truck
                .gross_mass_after_fuel_kg(d.trip.truck.specs.fuel_tank_gal)
                - d.trip.truck.cargo_kg;
            d.trip.truck.cargo_kg = LEGAL_GVW_KG - full_tank_gross_without_cargo - margin_kg;
        });
        let mut stop = RoadStop::new("Fuel test", 0.0, "travel_center");
        stop.actions = vec!["fuel".into()];

        if parking_full {
            let mut menu = ParkingFullState::with_drive(DriveRef::of(&drive), stop);
            let rows = build_labels(&mut menu, &mut app.ctx);
            let offer = rows
                .iter()
                .find(|row| row.starts_with("Refuel"))
                .expect("fuel row");
            assert!(offer.contains("Full tank:"), "{offer}");
            assert!(offer.contains(side), "{offer}");
            app.clear_speech();
            activate(&mut menu, &mut app.ctx, "Refuel");
        } else {
            let mut menu = RestStopState::with_drive(DriveRef::of(&drive), stop, false);
            let rows = build_labels(&mut menu, &mut app.ctx);
            let offer = rows
                .iter()
                .find(|row| row.starts_with("Refuel"))
                .expect("fuel row");
            assert!(offer.contains("Full tank:"), "{offer}");
            assert!(offer.contains(side), "{offer}");
            app.clear_speech();
            activate(&mut menu, &mut app.ctx, "Refuel");
        }

        with_drive(&drive, |d| {
            assert_eq!(d.trip.truck.fuel_gal, d.trip.truck.specs.fuel_tank_gal);
        });
        assert!(
            app.main_lines()
                .iter()
                .any(|line| line.contains("Gross weight:") && line.contains(side)),
            "{:?}",
            app.main_lines()
        );
    }
}
