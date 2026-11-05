use ff_core::models::business::{
    OWNER_OPERATOR_BUY_IN, OWNER_OPERATOR_DELIVERIES, OWNER_OPERATOR_LEVEL,
    OWNER_OPERATOR_REPUTATION, OWNER_OPERATOR_WORKING_CAPITAL,
};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::jobs::{make_reposition_job, Job, JobBoard, OfferOptions};
use ff_core::models::profile::Profile;

use freight_fate::app::testing::TestApp;
use freight_fate::meaningful_play::{MeaningfulPlayReason, MeaningfulPlayTracker};
use freight_fate::states::city::{BusinessStatusState, CityMenuState, GarageState, JobBoardState};
use freight_fate::states::city_pickup::{start_loaded_drive, LoadedDriveOptions};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::ArrivalState;

use crate::states_city_support::*;

fn unlocked_routed_job(app: &TestApp) -> Job {
    let profile = app.ctx.profile.as_ref().unwrap();
    let endorsements: Vec<&str> = profile.career.endorsements().into_iter().collect();
    JobBoard::new(app.ctx.world, None, None)
        .offers(
            &profile.current_city,
            &endorsements,
            OfferOptions {
                level: 0,
                market: Some(&profile.market),
                ..OfferOptions::default()
            },
        )
        .into_iter()
        .find(|job| {
            job.locked_reason(&endorsements, profile.career.level(), None, false)
                .is_empty()
                && app
                    .ctx
                    .world
                    .supported_route(&job.origin, &job.destination, None)
                    .ok()
                    .flatten()
                    .is_some()
        })
        .expect("an unlocked job with a supported route")
}

fn pending_reason(app: &TestApp) -> Option<MeaningfulPlayReason> {
    let name = &app.ctx.profile.as_ref().unwrap().name;
    app.ctx
        .cloud_saves_service()
        .meaningful_play_tracker()
        .for_upload(name)
        .map(|stamp| stamp.reason)
}

#[test]
fn loading_browsing_and_unchanged_save_do_not_mark_a_public_switch() {
    let mut app = TestApp::new();
    let profile = Profile::named_in("Road Star", "chicago_il");
    profile.save().unwrap();
    app.ctx.profile = Some(profile);

    // Loading and browsing do not call a meaningful-play hook. Saving the
    // same persistent content must not manufacture one centrally either.
    app.ctx.save_profile();

    assert!(app
        .ctx
        .cloud_saves_service()
        .meaningful_play_tracker()
        .for_upload("Road Star")
        .is_none());
}

#[test]
fn opening_a_fresh_dispatch_board_does_not_mark_a_public_switch() {
    let mut app = TestApp::new();
    let profile = Profile::named_in("Road Star", "Chicago");
    profile.save().unwrap();
    app.ctx.profile = Some(profile);
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);

    select::<CityMenuState>(&mut app, "Dispatch board");

    assert!(is::<JobBoardState>(&app));
    assert!(app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .dispatch_board_cache
        .is_some());
    assert_eq!(pending_reason(&app), None);
}

#[test]
fn changed_save_marks_only_after_persistent_content_changes() {
    let mut app = TestApp::new();
    let profile = Profile::named_in("Road Star", "chicago_il");
    profile.save().unwrap();
    app.ctx.profile = Some(profile);

    app.ctx.profile.as_mut().unwrap().earn(50.0);
    app.ctx.save_profile();

    let stamp = app
        .ctx
        .cloud_saves_service()
        .meaningful_play_tracker()
        .for_upload("Road Star")
        .expect("persistent gameplay change should mark the save");
    assert_eq!(stamp.reason, MeaningfulPlayReason::ChangedSave);
}

#[test]
fn explicit_events_use_the_closed_wire_reasons() {
    let tracker = MeaningfulPlayTracker::default();
    let cases = [
        (MeaningfulPlayReason::JobAccepted, "job_accepted"),
        (MeaningfulPlayReason::DriveStarted, "drive_started"),
        (
            MeaningfulPlayReason::DeliveryCompleted,
            "delivery_completed",
        ),
        (MeaningfulPlayReason::EquipmentChanged, "equipment_changed"),
        (MeaningfulPlayReason::BusinessChanged, "business_changed"),
        (MeaningfulPlayReason::ChangedSave, "changed_save"),
    ];

    for (reason, wire) in cases {
        tracker.mark("Road Star", reason);
        let stamp = tracker.for_upload("Road Star").unwrap();
        assert_eq!(stamp.reason, reason);
        assert_eq!(serde_json::to_value(reason).unwrap(), wire);
    }
}

#[test]
fn clearing_an_older_stamp_never_loses_a_newer_intent() {
    let tracker = MeaningfulPlayTracker::default();
    tracker.mark("Road Star", MeaningfulPlayReason::JobAccepted);
    let older = tracker.for_upload("Road Star").unwrap();
    tracker.mark("Road Star", MeaningfulPlayReason::DriveStarted);
    let newer = tracker.for_upload("Road Star").unwrap();
    assert_ne!(older.operation_id, newer.operation_id);

    tracker.clear_if_accepted("Road Star", &older.operation_id);
    assert_eq!(tracker.for_upload("Road Star"), Some(newer.clone()));

    tracker.clear_if_accepted("Road Star", &newer.operation_id);
    assert!(tracker.for_upload("Road Star").is_none());
}

#[test]
fn pending_stamp_survives_restart_until_exact_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = MeaningfulPlayTracker::new(dir.path());
    tracker.mark("Road Star", MeaningfulPlayReason::DeliveryCompleted);
    let stamp = tracker.for_upload("Road Star").unwrap();
    drop(tracker);

    let reloaded = MeaningfulPlayTracker::new(dir.path());
    assert_eq!(reloaded.for_upload("Road Star"), Some(stamp.clone()));
    reloaded.clear_if_accepted("Road Star", &stamp.operation_id);
    drop(reloaded);

    assert!(MeaningfulPlayTracker::new(dir.path())
        .for_upload("Road Star")
        .is_none());
}

#[test]
fn interrupted_temp_write_does_not_replace_the_last_durable_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = MeaningfulPlayTracker::new(dir.path());
    tracker.mark("Road Star", MeaningfulPlayReason::JobAccepted);
    let durable = tracker.for_upload("Road Star").unwrap();
    drop(tracker);

    std::fs::write(
        dir.path().join(".meaningful_play.json.interrupted.tmp"),
        br#"{"version":1,"pending":{"Road Star":"#,
    )
    .unwrap();

    let reloaded = MeaningfulPlayTracker::new(dir.path());
    assert_eq!(reloaded.for_upload("Road Star"), Some(durable));
}

#[test]
fn corrupt_ledger_is_preserved_read_only_instead_of_being_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meaningful_play.json");
    let corrupt = b"not valid meaningful-play json";
    std::fs::write(&path, corrupt).unwrap();

    let tracker = MeaningfulPlayTracker::new(dir.path());
    tracker.mark("Road Star", MeaningfulPlayReason::JobAccepted);

    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    assert_eq!(tracker.for_upload("Road Star"), None);
    drop(tracker);
    assert_eq!(
        MeaningfulPlayTracker::new(dir.path()).for_upload("Road Star"),
        None
    );
}

#[cfg(windows)]
#[test]
fn interrupted_atomic_replacement_keeps_the_previous_stamp_in_memory_and_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meaningful_play.json");
    let tracker = MeaningfulPlayTracker::new(dir.path());
    tracker.mark("Road Star", MeaningfulPlayReason::JobAccepted);
    let durable = tracker.for_upload("Road Star").unwrap();

    let original_permissions = std::fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    tracker.mark("Road Star", MeaningfulPlayReason::BusinessChanged);

    assert_eq!(tracker.for_upload("Road Star"), Some(durable.clone()));
    std::fs::set_permissions(&path, original_permissions).unwrap();
    drop(tracker);
    assert_eq!(
        MeaningfulPlayTracker::new(dir.path()).for_upload("Road Star"),
        Some(durable)
    );
}

#[test]
fn accepting_a_job_marks_job_accepted() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Road Star", "Chicago"));
    let job = unlocked_routed_job(&app);
    let mut board = JobBoardState::new(&app.ctx, vec![job]);

    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();

    assert_eq!(
        pending_reason(&app),
        Some(MeaningfulPlayReason::JobAccepted)
    );
}

#[test]
fn beginning_a_loaded_drive_marks_drive_started() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Road Star", "Chicago"));
    let job = unlocked_routed_job(&app);
    let route = app
        .ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .unwrap()
        .unwrap();

    start_loaded_drive(&mut app.ctx, job, route, LoadedDriveOptions::default());

    assert_eq!(
        pending_reason(&app),
        Some(MeaningfulPlayReason::DriveStarted)
    );
}

#[test]
fn completing_a_delivery_marks_delivery_completed() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Road Star", "Chicago"));
    let job = unlocked_routed_job(&app);
    let route = app
        .ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .unwrap()
        .unwrap();
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(4),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    app.ctx
        .mark_meaningful_play(MeaningfulPlayReason::DriveStarted);

    ArrivalState::new(&mut app.ctx, &mut driving);

    assert_eq!(
        pending_reason(&app),
        Some(MeaningfulPlayReason::DeliveryCompleted)
    );
}

#[test]
fn finishing_an_empty_reposition_keeps_drive_started() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Road Star", "Denver"));
    let job = make_reposition_job(app.ctx.world, "Denver", "Cheyenne", false, None)
        .expect("a supported empty reposition");
    let route = app
        .ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .unwrap()
        .unwrap();
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(4),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    app.ctx
        .mark_meaningful_play(MeaningfulPlayReason::DriveStarted);

    ArrivalState::new(&mut app.ctx, &mut driving);

    assert_eq!(
        pending_reason(&app),
        Some(MeaningfulPlayReason::DriveStarted)
    );
}

#[test]
fn repairing_equipment_marks_equipment_changed() {
    let mut app = TestApp::new();
    career(&mut app, "Road Star", "Chicago");
    profile_mut(&mut app).set_truck_damage_pct(10.0);
    let mut garage = GarageState::new();

    garage.repair(&mut app.ctx);

    assert_eq!(
        pending_reason(&app),
        Some(MeaningfulPlayReason::EquipmentChanged)
    );
}

#[test]
fn changing_business_status_marks_business_changed() {
    let mut app = TestApp::new();
    career(&mut app, "Road Star", "Chicago");
    {
        let profile = profile_mut(&mut app);
        profile.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        profile.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        profile.career.reputation = OWNER_OPERATOR_REPUTATION;
        profile.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL);
    }
    app.push_state(BusinessStatusState::new());

    select::<BusinessStatusState>(&mut app, "Owner-operator buy-in");

    assert_eq!(
        pending_reason(&app),
        Some(MeaningfulPlayReason::BusinessChanged)
    );
}
