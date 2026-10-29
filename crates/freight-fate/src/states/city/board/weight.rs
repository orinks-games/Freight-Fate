//! Gross-weight advice spoken before dispatch acceptance: an overweight
//! load only.

use ff_core::models::jobs::Job;
use ff_core::models::profile::Profile;
use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::vehicle::{TruckState, KG_PER_LB, KG_PER_TON};

pub(super) fn load_weight_margin(p: &Profile, job: &Job) -> String {
    let mut proposed = p.clone();
    proposed.take_slip_seat(job);
    let mut truck = TruckState::new(proposed.truck_specs());
    truck.fuel_gal = proposed.truck_fuel_gal();
    let margin_kg = truck.gross_weight_margin_with_cargo_kg(job.weight_tons * KG_PER_TON);
    // Under the limit is the normal case and says nothing.
    if margin_kg >= 0.0 {
        return String::new();
    }
    format!(
        "Load weight: {} pounds over the gross-weight limit with current fuel.",
        fmt_grouped(margin_kg.abs() / KG_PER_LB, 0)
    )
}
