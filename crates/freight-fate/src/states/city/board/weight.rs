//! Gross-weight advice spoken before dispatch acceptance: an overweight
//! load only.

use ff_core::data::seasonal_weight::seasonal_gvw_cap_kg;
use ff_core::data::world::World;
use ff_core::models::jobs::Job;
use ff_core::models::profile::Profile;
use ff_core::settings::Settings;
use ff_core::sim::vehicle::{TrailerSet, TruckState, KG_PER_TON};

pub(super) fn load_weight_margin(
    world: &World,
    settings: &Settings,
    p: &Profile,
    job: &Job,
) -> String {
    let mut proposed = p.clone();
    proposed.take_slip_seat(job);
    let mut truck = TruckState::new(proposed.truck_specs());
    truck.fuel_gal = proposed.truck_fuel_gal();
    // A set of doubles weighs both trailers and the dolly and carries the
    // gross cap of the lanes it may run, so the advice prices that set.
    let Some(set) =
        TrailerSet::for_cargo_between(job.cargo.key, world, &job.origin, &job.destination)
    else {
        return "Load weight: this set of doubles is not legal on any lane dispatch can offer \
                between these cities."
            .to_string();
    };
    truck.trailer_set = set;
    if let Some(limit) = &job.seasonal_weight_limit {
        truck.trailer_set.legal_gvw_kg = seasonal_gvw_cap_kg(&truck, limit.percent);
    }
    let margin_kg = truck.gross_weight_margin_with_cargo_kg(job.weight_tons * KG_PER_TON);
    // Under the limit is the normal case and says nothing.
    if margin_kg >= 0.0 {
        return String::new();
    }
    let mut text = format!(
        "Load weight: {} over the gross-weight limit with current fuel.",
        settings.weight_text(margin_kg.abs())
    );
    if truck.trailer_set.is_doubles() {
        text.push_str(&format!(
            " The limit for this set of doubles on the lanes offered is {}, counting both \
             trailers and the converter dolly.",
            settings.weight_text(truck.trailer_set.legal_gvw_kg)
        ));
    }
    if let Some(limit) = &job.seasonal_weight_limit {
        text.push_str(&format!(
            " Spring weight limits on the {} hold this truck to {} gross.",
            limit.highway,
            settings.weight_text(truck.trailer_set.legal_gvw_kg)
        ));
    }
    text
}
