//! The screens a stop opens over the drive, worked by the co-driver.
//!
//! The co-driver reads a screen's rows the way the playtest harness does
//! (`playtest::menu::menu_rows`) and picks one by its label, then the app
//! walks the cursor there and presses Enter. Only the screens the road
//! itself opens are theirs: a stop's menu, the scale, a trooper's stop, the
//! receiver's gate, and the short waits between them. A screen the player
//! opened is the player's, and any other screen is a decision the
//! co-driver hands back.

use crate::app::GameContext;
use crate::playtest::menu::menu_rows;
use crate::states::base::{State, TimedMessageState};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{FACILITY_ENGINE_SHUT_DOWN_ITEM, FACILITY_ENGINE_START_ITEM};
use crate::states::driving_menu_states::FacilityArrivalState;
use crate::states::driving_rest_states::{
    EnforcementStopState, ParkingFullState, RestStopState, TrafficStopState,
};

use super::pilot::StopPurpose;

/// Refuel when the tank is below this share at a stop that sells diesel.
const TOP_UP_FRACTION: f64 = 0.9;

/// What the co-driver does with the screen on top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayMove {
    /// Nothing yet: the screen is counting down on its own.
    Wait,
    /// Choose the row with this label.
    Row(String),
    /// Hand back to the player, for this reason.
    Summon(String),
}

fn row_starting(rows: &[String], prefix: &str) -> Option<String> {
    rows.iter().find(|row| row.starts_with(prefix)).cloned()
}

/// The co-driver's move on `top`, a screen the road opened over `drive`.
pub fn overlay_move(top: &dyn State, drive: &DrivingState, ctx: &GameContext) -> OverlayMove {
    let any = top.as_any();
    if any.is::<TimedMessageState>() {
        return OverlayMove::Wait;
    }
    let Some((rows, _)) = menu_rows(top, ctx) else {
        return OverlayMove::Wait;
    };
    if any.is::<FacilityArrivalState>() {
        return row_starting(&rows, "Dock and deliver")
            .or_else(|| row_starting(&rows, "Drop the loaded trailer"))
            .map_or_else(|| OverlayMove::Summon("gate".into()), OverlayMove::Row);
    }
    if any.is::<TrafficStopState>() || any.is::<EnforcementStopState>() {
        return row_starting(&rows, "Pull back onto the highway")
            .map_or_else(|| OverlayMove::Summon("roadside".into()), OverlayMove::Row);
    }
    if any.is::<ParkingFullState>() {
        return row_starting(&rows, "Drive on to the next stop")
            .map_or_else(|| OverlayMove::Summon("parking".into()), OverlayMove::Row);
    }
    if any.is::<RestStopState>() {
        return rest_stop_move(&rows, drive);
    }
    OverlayMove::Summon("screen".into())
}

/// At a stop: hand back if the shift is over, fuel if it sells diesel and
/// the tank wants it, then back to the road. Leaving a scale checks in.
fn rest_stop_move(rows: &[String], drive: &DrivingState) -> OverlayMove {
    let pilot = drive
        .team
        .as_ref()
        .and_then(|team| team.wheel.as_ref())
        .map(|wheel| &wheel.pilot);
    if pilot.is_some_and(|pilot| pilot.shift_over) {
        return OverlayMove::Summon("shift".into());
    }
    let refuel = row_starting(rows, "Refuel");
    let wants_fuel = drive.trip.truck.fuel_fraction() < TOP_UP_FRACTION
        || pilot.is_some_and(|pilot| {
            matches!(pilot.making_for, Some((_, StopPurpose::Fuel)))
                && drive.trip.truck.fuel_fraction() < 0.98
        });
    if let Some(refuel) = refuel.filter(|_| wants_fuel) {
        if drive.trip.truck.engine_on {
            if let Some(row) = row_starting(rows, FACILITY_ENGINE_SHUT_DOWN_ITEM) {
                return OverlayMove::Row(row);
            }
        }
        return OverlayMove::Row(refuel);
    }
    if !drive.trip.truck.engine_on {
        if let Some(row) = row_starting(rows, FACILITY_ENGINE_START_ITEM) {
            return OverlayMove::Row(row);
        }
    }
    row_starting(rows, "Back to the road")
        .map_or_else(|| OverlayMove::Summon("stop".into()), OverlayMove::Row)
}
