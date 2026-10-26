//! Leaving the shipper with the co-driver at the wheel.

use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, Menu};
use freight_fate::states::city::RouteSelectState;
use freight_fate::states::city_pickup::PickupFacilityState;
use freight_fate::states::driving::DrivingState;

const DT: f64 = 1.0 / 60.0;

/// Loaded at the shipper, with `drive_min` of the player's shift already
/// driven.
fn loaded_at_shipper(name: &str, drive_min: f64) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery {
        stop_at_pickup: true,
        ..StartDelivery::named(name)
    });
    harness.select_menu_item("Check in at shipping office");
    let labels = harness.menu_labels();
    let load = ["Drop and hook in the yard", "Load cargo at dock"]
        .into_iter()
        .find(|label| labels.iter().any(|row| row == label))
        .unwrap_or_else(|| panic!("no way to load: {labels:?}"));
    harness.select_menu_item(load);
    harness.finish_timed_state();
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(drive_min);
    // Reopen the menu so its rows reflect the logbook.
    harness.with_state::<PickupFacilityState, _>(|state, ctx| state.refresh(ctx, true));
    harness
}

fn team_row(labels: &[String], ending: &str) -> Option<String> {
    labels
        .iter()
        .find(|row| row.starts_with("Depart with ") && row.ends_with(ending))
        .cloned()
}

#[test]
fn a_load_one_shift_covers_offers_no_co_driver_at_the_dock() {
    let harness = loaded_at_shipper("Dock Short", 0.0);
    let labels = harness.menu_labels();
    assert!(team_row(&labels, "ride along").is_none(), "{labels:?}");
}

#[test]
fn the_co_driver_drives_out_of_the_shipper_and_the_terms_are_read_first() {
    let mut harness = loaded_at_shipper("Dock Team", 600.0);
    let labels = harness.menu_labels();
    let ride = team_row(&labels, "driving, and ride along")
        .unwrap_or_else(|| panic!("no team row: {labels:?}"));
    assert!(
        team_row(&labels, "driving, and sleep").is_some(),
        "{labels:?}"
    );
    // The terms are in the row's help before the player commits.
    let help = harness.with_state::<PickupFacilityState, _>(|state, ctx| {
        let at = state
            .menu()
            .items
            .iter()
            .position(|item| item.text(state, ctx) == ride)
            .expect("the row");
        state.menu().items[at].help_text(state, ctx)
    });
    assert!(help.contains("sleeper berth"), "{help}");
    harness.select_menu_item(&ride);
    if harness.state_is::<RouteSelectState>() {
        harness.key(InputEvent::key(Key::Return));
    }
    assert!(
        harness.state_is::<DrivingState>(),
        "departure did not drive"
    );
    let said = harness.transcript().join("\n");
    assert!(said.contains("takes the wheel."), "{said}");
    assert!(!said.contains("Engine off."), "{said}");
    // The hours and tank are read as on a solo departure, without a sleep
    // plan the team will not need.
    assert!(said.contains("Fuel: "), "{said}");
    assert!(!said.contains("This run needs"), "{said}");
    // The first frame on the road hands the wheel over.
    harness.advance_frame_clock();
    harness.app.frame(DT);
    assert!(
        harness.app.ctx.co_driver_away,
        "the co-driver has the wheel"
    );
    assert!(harness.with_state::<DrivingState, _>(|d, _| d.co_driver_driving()));
    // And pulls out: the engine, the air and the brake are theirs too.
    let mut moving = false;
    for _ in 0..60 * 60 * 3 {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if !harness.state_is::<DrivingState>() {
            continue;
        }
        if harness.with_state::<DrivingState, _>(|d, _| d.truck().speed_mph()) > 15.0 {
            moving = true;
            break;
        }
    }
    assert!(moving, "the co-driver never got the truck rolling");
}
