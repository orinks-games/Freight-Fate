//! Mexico freight v1 at the Laredo border transfer yards, through the game:
//! a southbound delivery ends at the yard and names the Mexican consignee,
//! a northbound pickup waits on paperwork on duty, not driving, and no
//! border toll is charged.

use crate::states_city_support::*;
use ff_core::data::border_yards::border_yards;
use ff_core::models::border_freight::{paperwork_minutes, southbound_consignee};
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::city_pickup::{PickupFacilityState, PickupOptions, PICKUP_CHECK_IN_MIN};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::ArrivalState;

const LAREDO: &str = "laredo_tx_us";
const SAN_ANTONIO: &str = "san_antonio_tx_us";

fn southbound_job() -> Job {
    let yard = &border_yards().expect("border yards").yards[1];
    let mut job = Job::new(
        cargo_type("automotive").unwrap(),
        16.0,
        SAN_ANTONIO,
        "San Antonio Cross-Dock",
        LAREDO,
        156.0,
        1400.0,
        8.0,
    );
    job.origin_type = "cross_dock".to_string();
    job.destination_location = yard.name.clone();
    job.destination_type = yard.facility_type.clone();
    job.destination_facility_id = yard.id.clone();
    job.origin_spoken = "San Antonio".to_string();
    job.destination_spoken = "Laredo".to_string();
    job
}

fn northbound_job() -> Job {
    let yard = &border_yards().expect("border yards").yards[0];
    let mut job = Job::new(
        cargo_type("machinery").unwrap(),
        14.0,
        LAREDO,
        &yard.name,
        SAN_ANTONIO,
        156.0,
        1400.0,
        8.0,
    );
    job.origin_type = yard.facility_type.clone();
    job.origin_facility_id = yard.id.clone();
    job.destination_location = "San Antonio Dry Warehouse".to_string();
    job.destination_type = "dry_warehouse".to_string();
    job.origin_spoken = "Laredo".to_string();
    job.destination_spoken = "San Antonio".to_string();
    job
}

#[test]
fn southbound_delivery_ends_at_the_yard_and_speaks_the_consignee() {
    let mut app = TestApp::new();
    let job = southbound_job();
    let consignee = southbound_consignee(&job)
        .expect("a consignee")
        .spoken
        .clone();
    app.ctx.profile = Some(Profile::named_in("Border Run", SAN_ANTONIO));
    let route = app
        .ctx
        .world
        .route_from_cities(&[SAN_ANTONIO.to_string(), LAREDO.to_string()])
        .expect("San Antonio to Laredo");
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job.clone(),
        route,
        Some(4),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    driving.trip.position_mi = driving.trip.total_miles();
    driving.trip.update(0.0);
    let arrival = ArrivalState::new(&mut app.ctx, &mut driving);
    let summary = arrival.summary_parts.join(" ");

    assert!(
        summary.contains("Delivered 16 tons of automotive parts to Laredo"),
        "{summary}"
    );
    assert!(
        summary.contains(&format!(
            "Trailer dropped at SCI Cross Dock. A transfer carrier takes it across from here to {consignee}."
        )),
        "{summary}"
    );
    // The bridge is the transfer carrier's: no toll lands on this load.
    assert!(summary.contains("tolls 0,"), "{summary}");
    assert_eq!(driving.trip.toll_expense(), 0.0);
    // The delivery completes in Laredo; nothing drives south of it.
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.career.deliveries, 1);
    assert_eq!(app.ctx.world.resolve_city_key(&p.current_city), LAREDO);
}

#[test]
fn northbound_pickup_waits_on_paperwork_on_duty_not_driving() {
    let mut app = TestApp::new();
    career(&mut app, "Border Pickup", "Laredo");
    let job = northbound_job();
    let wait = paperwork_minutes(&job);
    assert!((30.0..=90.0).contains(&wait), "{wait}");
    let pickup = PickupFacilityState::new(&app.ctx, job, PickupOptions::default());
    app.push_state(pickup);
    let hours_before = profile(&app).game_hours;
    let duty_before = profile(&app).hos.duty_min;
    let driving_before = profile(&app).hos.driving_min;
    app.clear_speech();

    key(&mut app, Key::Return); // check in

    let p = profile(&app);
    let expected = PICKUP_CHECK_IN_MIN + wait;
    assert!(((p.game_hours - hours_before) * 60.0 - expected).abs() < 1e-6);
    assert!((p.hos.duty_min - duty_before - expected).abs() < 1e-6);
    assert_eq!(p.hos.driving_min, driving_before);
    let segment = p
        .duty_log
        .segments
        .iter()
        .find(|s| s.note == "waiting on trailer paperwork")
        .unwrap_or_else(|| panic!("{:?}", p.duty_log.segments));
    assert_eq!(segment.status, "on_duty_not_driving");
    assert!(((segment.end_hour - segment.start_hour) * 60.0 - wait).abs() < 1e-6);

    let said = app.main_lines().join(" ");
    assert!(said.contains("paperwork"), "{said}");
    assert!(
        said.contains(&format!("About {} minutes", wait as u32)),
        "{said}"
    );
    assert!(said.contains("logged on duty, not driving"), "{said}");
}
