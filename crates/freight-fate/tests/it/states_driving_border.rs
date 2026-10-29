use ff_core::achievements::{increment_stat, int_stat};
use ff_core::sim::trip_models::BorderBooth;

use freight_fate::playtest::{key_event, PlaytestHarness, RouteSetup};
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_border::{border_secondary_referral, BorderClearanceState};
use freight_fate::states::driving_core::{BORDER_PRIMARY_MIN, BORDER_SECONDARY_EXTRA_MIN};
use freight_fate::states::driving_rest_states::{EnforcementStopState, TrafficStopState};

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn mph_to_mps(mph: f64) -> f64 {
    mph * MPS_PER_MPH
}

fn border_drive(origin: &str, destination: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_route(origin, destination, RouteSetup::default());
    harness.prepare_for_driving(12.0);
    harness
}

fn booth_named(harness: &PlaytestHarness, name: &str) -> BorderBooth {
    harness.read_drive(|drive| {
        drive
            .trip
            .border_booths
            .iter()
            .find(|booth| booth.name == name)
            .unwrap_or_else(|| {
                let route: Vec<_> = drive
                    .trip
                    .route
                    .legs
                    .iter()
                    .map(|leg| {
                        (
                            leg.a.as_str(),
                            leg.b.as_str(),
                            leg.checkpoints()
                                .iter()
                                .map(|checkpoint| {
                                    (
                                        checkpoint.name.as_str(),
                                        checkpoint.checkpoint_type.as_str(),
                                        checkpoint.state.as_str(),
                                        checkpoint.at_mi,
                                    )
                                })
                                .collect::<Vec<_>>(),
                            leg.state_crossings()
                                .iter()
                                .map(|crossing| {
                                    (
                                        crossing.state.as_str(),
                                        crossing.from_state.as_str(),
                                        crossing.at_mi,
                                    )
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                panic!(
                    "missing {name:?} booth: {:?}; route legs: {route:?}",
                    drive.trip.border_booths
                )
            })
            .clone()
    })
}

fn run_into_booth(harness: &mut PlaytestHarness, booth: &BorderBooth, mph: f64) {
    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = booth.at_mi;
        drive.trip.truck.velocity_mps = mph_to_mps(mph);
        assert!(
            drive.check_border_booth_crossing(ctx, booth.at_mi - 0.01),
            "crossing {booth:?} did not fire"
        );
    });
    harness.with_drive(|drive, ctx| {
        drive.trip.truck.velocity_mps = 0.0;
        drive.update_pull_over(ctx, 1.0, false);
    });
    harness.expect_state::<EnforcementStopState>("running a border booth");
}

fn arrive_at_booth_slowly(harness: &mut PlaytestHarness, booth: &BorderBooth) {
    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = booth.at_mi;
        drive.trip.truck.velocity_mps = mph_to_mps(12.0);
        assert!(
            drive.check_border_booth_crossing(ctx, booth.at_mi - 0.01),
            "slow crossing {booth:?} did not fire"
        );
    });
}

fn return_to_clearance(harness: &mut PlaytestHarness) {
    harness.select_menu_item("Pull back onto the highway");
    harness.expect_state::<BorderClearanceState>("returning from the port-running stop");
}

fn clear_booth(harness: &mut PlaytestHarness) {
    harness.select_menu_item("Answer the officer's questions and hand over your documents");
    harness.expect_state::<DrivingState>("clearing customs");
}

#[test]
fn beaver_creek_approach_and_clearance_charge_ten_on_duty_minutes() {
    let mut harness = border_drive("tok_ak_us", "whitehorse_yt_ca");
    harness.app.ctx.settings.place_callouts = "off".to_string();
    let booth = booth_named(&harness, "Beaver Creek Port of Entry");
    let no_referral_seed = (0..100)
        .find(|seed| !border_secondary_referral(*seed, &booth.key))
        .expect("a deterministic non-referral seed");

    harness.with_drive(|drive, ctx| {
        drive.trip_seed = no_referral_seed;
        drive.trip.position_mi = booth.at_mi - 2.0;
        drive.trip.truck.velocity_mps = mph_to_mps(12.0);
        drive.check_border_approach(ctx);
    });
    harness.drive_frames(1);
    let approach = harness.transcript_text();
    assert!(
        approach.contains(
            "Beaver Creek Port of Entry ahead in 2.0 miles, Canada Border Services Agency"
        ),
        "{approach}"
    );

    arrive_at_booth_slowly(&mut harness, &booth);
    harness.expect_state::<BorderClearanceState>("rolling slowly into Beaver Creek");
    let opening = harness.transcript_text();
    assert!(
        opening.contains(
            "Beaver Creek Port of Entry, Canada Border Services Agency. The officer asks for your passport and checks the eManifest your carrier filed before arrival."
        ),
        "{opening}"
    );

    let game_minutes_before = harness.read_drive(|drive| drive.trip.game_minutes);
    let duty_before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .hos
        .duty_min;
    clear_booth(&mut harness);
    let game_minutes_after = harness.read_drive(|drive| drive.trip.game_minutes);
    let profile = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile");
    assert_eq!(game_minutes_after - game_minutes_before, BORDER_PRIMARY_MIN);
    assert_eq!(profile.hos.duty_min - duty_before, BORDER_PRIMARY_MIN);
    assert!(
        profile.duty_log.segments.iter().any(|segment| {
            segment.status == "on_duty_not_driving" && segment.note == "customs clearance"
        }),
        "{:?}",
        profile.duty_log.segments
    );
    assert!(harness.read_drive(|drive| drive.trip.truck.speed_mph()) <= 0.01);
}

#[test]
fn customs_secondary_referral_charges_forty_five_additional_on_duty_minutes() {
    let mut harness = border_drive("tok_ak_us", "whitehorse_yt_ca");
    let booth = booth_named(&harness, "Beaver Creek Port of Entry");
    let referral_seed = (0..100)
        .find(|seed| border_secondary_referral(*seed, &booth.key))
        .expect("a deterministic referral seed");
    harness.with_drive(|drive, _| {
        drive.trip_seed = referral_seed;
    });
    arrive_at_booth_slowly(&mut harness, &booth);
    harness.expect_state::<BorderClearanceState>("referring Beaver Creek to secondary");

    let game_minutes_before = harness.read_drive(|drive| drive.trip.game_minutes);
    let duty_before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .hos
        .duty_min;
    clear_booth(&mut harness);
    let game_minutes_after = harness.read_drive(|drive| drive.trip.game_minutes);
    let profile = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile");
    let expected_minutes = BORDER_PRIMARY_MIN + BORDER_SECONDARY_EXTRA_MIN;
    assert_eq!(game_minutes_after - game_minutes_before, expected_minutes);
    assert_eq!(profile.hos.duty_min - duty_before, expected_minutes);
    assert!(
        harness.transcript_text().contains(
            "Referred to secondary inspection. Officers examine the trailer for 45 minutes, then clear you. Welcome to Canada."
        ),
        "{}",
        harness.transcript_text()
    );
}

#[test]
fn running_the_alcan_port_charges_the_first_us_penalty() {
    let mut harness = border_drive("whitehorse_yt_ca", "tok_ak_us");
    let booth = booth_named(&harness, "Alcan Port of Entry");
    let money_before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .money();

    run_into_booth(&mut harness, &booth, 55.0);
    let money_after = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .money();
    assert_eq!(money_before - money_after, 5_000.0);
    let transcript = harness.transcript_text();
    assert!(transcript.contains("Alcan Port of Entry"), "{transcript}");
    assert!(
        transcript.contains("U.S. Customs and Border Protection"),
        "{transcript}"
    );
    assert!(transcript.contains("5,000"), "{transcript}");
    return_to_clearance(&mut harness);
    assert!(
        harness
            .transcript_text()
            .contains("Officers walk you back to the customs booth."),
        "{}",
        harness.transcript_text()
    );
    clear_booth(&mut harness);
}

#[test]
fn bypassed_border_running_does_not_increment_the_us_stat() {
    let mut harness = border_drive("whitehorse_yt_ca", "tok_ak_us");
    harness.app.ctx.settings.hos_mode = "debug_off".to_string();
    let booth = booth_named(&harness, "Alcan Port of Entry");

    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = booth.at_mi;
        drive.trip.truck.velocity_mps = mph_to_mps(55.0);
        assert!(drive.check_border_booth_crossing(ctx, booth.at_mi - 0.01));
    });

    harness.expect_state::<BorderClearanceState>("bypassing the Alcan running penalty");
    let profile = harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("the drive has a profile");
    assert_eq!(int_stat(profile, "border_reports_missed_us"), 0);
}

#[test]
fn active_speeding_stop_is_preserved_and_customs_opens_afterward() {
    let mut harness = border_drive("whitehorse_yt_ca", "tok_ak_us");
    let booth = booth_named(&harness, "Alcan Port of Entry");

    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = booth.at_mi - 0.01;
        drive.trip.truck.velocity_mps = mph_to_mps(55.0);
        drive.begin_pull_over(ctx, 45.0);
        assert_eq!(drive.pull_over_kind, "speeding");
        assert_eq!(drive.pull_over_fine, 0.0);

        drive.trip.position_mi = booth.at_mi;
        assert!(drive.check_border_booth_crossing(ctx, booth.at_mi - 0.01));
        assert_eq!(drive.pull_over_kind, "speeding");
        assert_eq!(drive.pull_over_fine, 0.0);
        assert!(drive
            .enforcement_events
            .contains(&format!("border-booth:{}", booth.key)));
        assert_eq!(
            drive
                .pending_border_clearance
                .as_ref()
                .map(|pending| pending.key.as_str()),
            Some(booth.key.as_str())
        );
    });

    harness.with_drive(|drive, ctx| {
        drive.trip.truck.velocity_mps = 0.0;
        drive.update_pull_over(ctx, 1.0, false);
    });
    harness.expect_state::<TrafficStopState>("resolving the existing speeding stop");
    harness.select_menu_item("Pull back onto the highway");
    harness.expect_state::<BorderClearanceState>("opening customs after the speeding stop");
    let profile = harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("the drive has a profile");
    assert_eq!(int_stat(profile, "border_reports_missed_us"), 0);
}

#[test]
fn running_the_alcan_port_escalates_the_us_penalty_after_a_prior_offence() {
    let mut harness = border_drive("whitehorse_yt_ca", "tok_ak_us");
    increment_stat(
        harness
            .app
            .ctx
            .profile
            .as_mut()
            .expect("the drive has a profile"),
        "border_reports_missed_us",
    );
    let booth = booth_named(&harness, "Alcan Port of Entry");
    let money_before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .money();

    run_into_booth(&mut harness, &booth, 55.0);
    let money_after = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .money();
    assert_eq!(money_before - money_after, 10_000.0);
}

#[test]
fn running_into_canada_charges_the_converted_amps_penalty() {
    let mut harness = border_drive("tok_ak_us", "whitehorse_yt_ca");
    let booth = booth_named(&harness, "Beaver Creek Port of Entry");
    let money_before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .money();

    run_into_booth(&mut harness, &booth, 55.0);
    let money_after = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("the drive has a profile")
        .money();
    assert_eq!(money_before - money_after, 1_431.0);
    let transcript = harness.transcript_text();
    assert!(
        transcript.contains("2,000 Canadian dollar penalty, charged as 1,431 US dollars"),
        "{transcript}"
    );
    return_to_clearance(&mut harness);
    clear_booth(&mut harness);
}

#[test]
fn customs_secondary_referral_has_both_seeded_outcomes() {
    assert!(border_secondary_referral(7, "alcan_port"));
    assert!(!border_secondary_referral(0, "alcan_port"));
}

#[test]
fn escape_before_customs_clearance_keeps_the_menu_open() {
    let mut harness = border_drive("tok_ak_us", "whitehorse_yt_ca");
    let booth = booth_named(&harness, "Beaver Creek Port of Entry");
    arrive_at_booth_slowly(&mut harness, &booth);
    harness.expect_state::<BorderClearanceState>("stopping at Beaver Creek");

    harness.key(key_event(Key::Escape, None));
    harness.expect_state::<BorderClearanceState>("refusing to leave customs uncleared");
    assert!(harness
        .transcript_text()
        .contains("Customs has to clear you before you drive on."));
}
