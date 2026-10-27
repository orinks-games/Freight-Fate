//! The screens the drive itself opens: live status, the driver tablet, the
//! destination facility, and the delivery settlement (port of
//! `freight_fate/states/driving_menu_states.py`).
//!
//! Split by screen, because the Python file was one 1,900-line module:
//!
//! * `driving_menu_states/status.rs` -- [`DrivingStatusState`] and the one
//!   screen it opens, [`DrivingStatusScreenState`] (Route, Driver, Map,
//!   Radio).
//! * `driving_menu_states/apps.rs` -- [`DriverAppsState`] and
//!   [`DriverAppScreenState`], the driver tablet.
//! * `driving_menu_states/facility_arrival.rs` -- [`FacilityArrivalState`].
//! * `driving_menu_states/arrival.rs` -- [`ArrivalState`] and the whole
//!   delivery settlement.
//! * `driving_menu_states/badges.rs` -- the arrival achievement sweep, which
//!   is a third of the Python file on its own.
//! * `driving_menu_states/drive_ref.rs` -- [`DriveRef`], how every screen in
//!   this task reaches the drive it covers. Read its module docs first: it
//!   is the one structural difference from the Python.
//!
//! Python's multiple inheritance is gone. `FacilityArrivalState(
//! FacilityEngineMixin, MenuState)` becomes a struct implementing the
//! existing [`crate::states::driving_core::FacilityEngine`] trait, whose
//! default methods carry the shared engine row exactly as the mixin did.

mod apps;
mod arrival;
mod badges;
mod commands;
mod drive_ref;
mod facility_arrival;
mod status;
mod stop_purpose;

pub use apps::{DriverAppScreenState, DriverAppsState};
pub use arrival::{settlement_hours, ArrivalState};
pub use commands::DrivingCommandsState;
pub use drive_ref::{keep_rows, push_over_drive, replace_drive_with, DriveRef};
pub use facility_arrival::FacilityArrivalState;
pub use status::{DrivingStatusScreenState, DrivingStatusState};
pub use stop_purpose::StopPurposeState;

use crate::app::GameContext;
use crate::states::driving::DrivingState;

pub const DELIVERY_SETTLEMENT_MAX_AVERAGE_MPH: f64 = 55.0;
pub const ROAD_GRIME_PER_MILE: f64 = 0.004;
/// Below this the settlement reports the tank; at or above it a near-full
/// tank is the unremarkable default and the row is dropped (research doc
/// R10). A quarter tank is the point a driver planning the next leg starts
/// to care.
pub const SETTLEMENT_LOW_FUEL_FRACTION: f64 = 0.25;

/// Plain "deliver into this city" badges (titles claim nothing extra).
/// Mostly cities the jukebox got to first; each badge's song lives in the
/// catalog.
pub const SIMPLE_ARRIVAL_BADGES: [(&str, &str); 22] = [
    ("phoenix_az_us", "phoenix_arrival"),
    ("wichita_ks_us", "wichita_arrival"),
    ("bakersfield_ca_us", "bakersfield_arrival"),
    ("las_vegas_nv_us", "vegas_arrival"),
    ("nashville_tn_us", "nashville_delivery"),
    ("el_paso_tx_us", "el_paso_arrival"),
    ("laredo_tx_us", "laredo_arrival"),
    ("baton_rouge_la_us", "baton_rouge_arrival"),
    ("sacramento_ca_us", "sacramento_arrival"),
    ("muskogee_ok_us", "muskogee_arrival"),
    ("kansas_city_mo_us", "kansas_city_arrival"),
    ("memphis_tn_us", "memphis_arrival"),
    ("saginaw_mi_us", "saginaw_arrival"),
    ("fort_worth_tx_us", "fort_worth_arrival"),
    ("san_antonio_tx_us", "san_antonio_arrival"),
    ("new_orleans_la_us", "new_orleans_arrival"),
    ("houston_tx_us", "houston_arrival"),
    ("winslow_az_us", "winslow_arrival"),
    ("chattanooga_tn_us", "chattanooga_arrival"),
    // The song never settles which Jackson it means, so either one counts.
    ("jackson_tn_us", "jackson_arrival"),
    ("jackson_ms_us", "jackson_arrival"),
    ("abilene_tx_us", "abilene_arrival"),
];

/// The badge a plain arrival into this city earns, if any.
pub fn simple_arrival_badge(city_key: &str) -> Option<&'static str> {
    SIMPLE_ARRIVAL_BADGES
        .iter()
        .find(|(key, _)| *key == city_key)
        .map(|(_, badge)| *badge)
}

// -- what the drive pushes ------------------------------------------------------------
//
// The blocks `driving_controls/pending.rs` and `driving_events/pending.rs`
// held for this module. Each does its screen's entry work here, while the
// drive is in hand, then hands the built screen to the stack (see
// `drive_ref`).

impl DrivingState {
    /// The driving command list: F2, and three-finger tap on a touch screen.
    pub fn push_driving_commands(&mut self, ctx: &mut GameContext) {
        // Its rows come from the key table, not the drive, so the ordinary
        // push works from inside the drive's handler.
        let state = DrivingCommandsState::new(ctx);
        ctx.push_state(state);
    }

    /// `ctx.push_state(DrivingStatusState(ctx, self))`: Tab, and the pad's
    /// modifier plus Start.
    pub fn push_driving_status(&mut self, ctx: &mut GameContext) {
        // Nothing this screen's entry reads lives on the drive, so it can
        // go through the ordinary push even from inside the drive's handler.
        let state = DrivingStatusState::new(ctx);
        ctx.push_state(state);
    }

    /// `ctx.replace_state(ArrivalState(ctx, self))`.
    pub fn replace_with_arrival_state(&mut self, ctx: &mut GameContext) {
        // The settlement read everything it needs out of the drive in
        // `new`, so its own entry needs nothing more.
        let state = ArrivalState::new(ctx, self);
        ctx.replace_state(state);
    }

    /// Settle after receiver service while judging the appointment at the
    /// instant the truck checked in. The drive clock may have advanced for
    /// unloading since then, and still owns the later calendar time.
    pub fn replace_with_arrival_state_at(&mut self, ctx: &mut GameContext, arrival_hours: f64) {
        let state = ArrivalState::new_at(ctx, self, arrival_hours);
        ctx.replace_state(state);
    }
}
