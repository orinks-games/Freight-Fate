//! A full lot at night: the overnight stay is turned away, the short stays
//! are not. A break stop announced and planned as one keeps its 30-minute
//! break and its save row when the lot is full (player report, 2026-11-23).

use ff_core::sim::stop_lot::StopLot;
use ff_core::sim::trip_models::RoadStop;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::{ParkingFullState, RestFocus};

use crate::states_driving_menus_support::*;

fn rest_area(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Scott County Rest Area", at_mi, "public_rest_area");
    stop.actions = ["park", "save", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop
}

#[test]
fn a_full_rest_area_still_offers_the_break_and_the_save() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Denver", "Salt Lake City", "Full Lot");
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = rest_area(at);
    assert!(!StopLot::FullTonight.offers(&stop.actions, "sleep"));
    let mut state = ParkingFullState::with_drive(DriveRef::of(&drive), stop);
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(
        rows,
        [
            "Take a 30-minute break",
            "Drive on to the next stop",
            "Motel room: sleep 10 hours for 95 dollars",
            "Park on the shoulder and sleep",
            "Save at this stop",
        ],
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.starts_with("Sleep ")),
        "{rows:?}"
    );
}

#[test]
fn the_break_at_a_full_lot_resets_the_break_clock() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Denver", "Salt Lake City", "Full Lot");
    let at = with_drive(&drive, |d| d.trip.position_mi);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .since_break_min = 400.0;
    let mut state = ParkingFullState::with_drive(DriveRef::of(&drive), rest_area(at));
    state.menu_mut().items = state.build_items(&mut app.ctx);
    activate(&mut state, &mut app.ctx, "Take a 30-minute break");
    let hos = &app.ctx.profile.as_ref().expect("a career").hos;
    assert_eq!(hos.since_break_min, 0.0);
    let lines = app.main_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("You took a 30-minute break.")),
        "{lines:?}"
    );
}

#[test]
fn saving_at_a_full_lot_says_so() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Denver", "Salt Lake City", "Full Lot");
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = ParkingFullState::with_drive(DriveRef::of(&drive), rest_area(at));
    state.menu_mut().items = state.build_items(&mut app.ctx);
    activate(&mut state, &mut app.ctx, "Save at this stop");
    assert_eq!(last(&app), "Saved at Scott County Rest Area.");
}

#[test]
fn a_planned_break_lands_on_the_break_row_at_a_full_lot() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Denver", "Salt Lake City", "Full Lot");
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state =
        ParkingFullState::with_drive(DriveRef::of(&drive), rest_area(at)).focused(RestFocus::Break);
    drive_and_ctx(&drive, &mut app, |d, ctx| state.enter_over_drive(ctx, d));
    let spoken = last(&app);
    assert!(
        spoken.starts_with("The truck parking at Scott County Rest Area is full tonight.")
            && spoken.ends_with("Take a 30-minute break. 1 of 5."),
        "{spoken}"
    );
}
