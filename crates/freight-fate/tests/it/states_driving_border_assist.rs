//! All assists stops the truck at a customs booth (owner ruling,
//! 2026-11-28), the way it stops at an open scale; a lower assist level
//! leaves the stop to the driver, who runs the booth by rolling through.

use ff_core::sim::trip_models::BorderBooth;

use freight_fate::playtest::{PlaytestHarness, RouteSetup};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_border::{border_secondary_referral, BorderClearanceState};
use freight_fate::states::driving_rest_states::TrafficStopState;

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

/// Tok to Whitehorse on `preset`, staged `lead_mi` short of Beaver Creek
/// at 50 mph on cruise, with a seed that sends the truck through primary.
fn approach_beaver_creek(preset: &str, lead_mi: f64) -> (PlaytestHarness, BorderBooth) {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.facility_streets = false;
    harness.start_route(
        "tok_ak_us",
        "whitehorse_yt_ca",
        RouteSetup::default().cities(&["tok_ak_us", "haines_junction_yt_ca", "whitehorse_yt_ca"]),
    );
    harness.prepare_for_driving(12.0);
    assert!(harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset(preset));
    harness.app.ctx.settings.place_callouts = "off".to_string();
    let booth = harness.read_drive(|d| {
        d.trip
            .border_booths
            .iter()
            .find(|booth| booth.name == "Beaver Creek Port of Entry")
            .cloned()
            .expect("Beaver Creek on the drive")
    });
    let seed = (0..100)
        .find(|seed| !border_secondary_referral(*seed, &booth.key))
        .expect("a seed with no referral");
    let at = booth.at_mi;
    harness.with_drive(move |d, ctx| {
        d.tutorial = None;
        d.trip_seed = seed;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.set_patrols(Vec::new());
        d.trip.position_mi = at - lead_mi;
        d.trip.truck.velocity_mps = 50.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 50.0, false);
    });
    (harness, booth)
}

/// Run the real driving frame, hands off, until the clearance or a police
/// stop opens or `frames` pass. Returns the hardest the booth stop braked.
fn drive_until_the_booth(harness: &mut PlaytestHarness, frames: usize) -> f64 {
    let mut hardest: f64 = 0.0;
    for _ in 0..frames {
        if !harness.state_is::<DrivingState>() {
            break;
        }
        harness.advance_frame_clock();
        harness.with_drive(|d, ctx| d.update_frame(ctx, 1.0 / 60.0));
        let (brake, stopped, cruising) = harness.read_drive(|d| {
            (
                d.booth_stop_brake,
                d.pull_over.is_some(),
                d.cruise_mph.is_some(),
            )
        });
        // Once the stop has begun, cruise stays paused to the booth: it
        // used to resume each time the brake eased ("Cruise resuming, 40."
        // twice on the way in; agent drive at Blaine, 2026-11-28).
        assert!(
            hardest == 0.0 || !cruising,
            "cruise resumed during the booth stop: {}",
            harness.transcript_text()
        );
        hardest = hardest.max(brake);
        if stopped {
            break;
        }
    }
    hardest
}

#[test]
fn all_assists_brakes_to_a_stop_at_the_customs_booth() {
    let (mut harness, booth) = approach_beaver_creek("all", 1.2);
    let hardest = drive_until_the_booth(&mut harness, 20_000);
    assert!(hardest > 0.05, "the booth stop never braked: {hardest}");
    assert!(hardest < 0.9, "a grab, not a stop: {hardest}");
    harness.expect_state::<BorderClearanceState>("All assists stopping at Beaver Creek");
    let said = harness.transcript_text();
    assert!(
        said.contains("The officer asks for your passport"),
        "{said}"
    );
    assert!(!said.contains("stopped the truck for running"), "{said}");
    let (speed, position) = harness.read_drive(|d| (d.trip.truck.speed_mph(), d.trip.position_mi));
    assert!(speed <= 0.01, "{speed}");
    assert!(
        (booth.at_mi - position).abs() <= 0.02,
        "stopped {:.3} miles from the booth",
        booth.at_mi - position
    );
    // Clearance goes on as after a stop by hand.
    harness.select_menu_item("Answer the officer's questions and hand over your documents");
    harness.expect_state::<DrivingState>("cleared at Beaver Creek");
    assert!(!harness.state_is::<TrafficStopState>());
}

#[test]
fn a_lower_assist_level_leaves_the_booth_stop_to_the_driver() {
    let (mut harness, _booth) = approach_beaver_creek("balanced", 1.2);
    assert_eq!(drive_until_the_booth(&mut harness, 20_000), 0.0);
    let ran = harness.read_drive(|d| d.pull_over.is_some());
    let said = harness.transcript_text();
    assert!(ran, "the truck should roll through on Balanced: {said}");
    assert!(!harness.state_is::<BorderClearanceState>());
}
