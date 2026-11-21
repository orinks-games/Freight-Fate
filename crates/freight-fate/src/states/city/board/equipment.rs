//! The tractor a load goes out on, said once at dispatch.

use ff_core::models::carrier_fleet::{
    assignment_reason_text, equipment_held_back, equipment_hold_text, slip_seats,
};
use ff_core::models::jobs::Job;
use ff_core::models::profile::Profile;

use crate::app::GameContext;
use crate::states::city::profile_mut;

/// The tractor for this load, then the carrier's shop on it if it needs one.
pub(super) fn dispatch_equipment_note(ctx: &mut GameContext, job: &Job) -> String {
    let seat = slip_seat_note(ctx, job);
    let shop = crate::states::city_garage::carrier_shop_before_dispatch(ctx);
    format!("{seat}{shop}")
}

/// Draw the assigned tractor and say why, or nothing if it is not new.
///
/// Silent when the truck has not changed: a driver who drew the same
/// spare three loads running does not need telling three times.
///
/// A driver past slip-seating has ONE truck and draws nothing, so there
/// is normally nothing to say -- but if the yard is holding them below
/// the tractor their level earns, that is the one thing they will want
/// explained, and this used to be silent about it. A level 11 driver
/// capped to a regional truck by their standing got handed a yard mule
/// every run with no reason given anywhere near the moment it happened;
/// the explanation existed, but only on the standing screen, which you
/// have to already suspect the answer to go and read (Brandon,
/// 2026-08-21: "what gives?").
fn slip_seat_note(ctx: &mut GameContext, job: &Job) -> String {
    let p = profile_mut(ctx);
    if p.owns_equipment() {
        return String::new();
    }
    if !slip_seats(p) {
        // No draw to announce, so say only the part that is news: why the
        // iron is short of the level, and what gives it back.
        return if equipment_held_back(p) {
            format!(" {}", equipment_hold_text(p, true))
        } else {
            String::new()
        };
    }
    let before = p.active_truck_key();
    let key = p.take_slip_seat(job);
    if key == before {
        return String::new();
    }
    format!(" {}", assignment_reason_text::<Profile>(&key, Some(p)))
}
