//! Mexico freight at a border transfer yard, as a load sees it.
//!
//! Southbound: a load delivered to a yard in `border_yards.json` names the
//! Mexican city it is really bound for on the load card, and the delivery
//! says the trailer was dropped for a transfer carrier. The drive, the
//! route and the settlement are the ordinary ones; the consignee city is
//! text and never a place on the map.
//!
//! Northbound: a load picked up at a yard waits on paperwork first, a
//! gameplay range drawn from the load itself (not border data), logged as
//! on duty, not driving (49 CFR 395.2) like any other facility hold.
//!
//! Every pick is keyed on the load, never on a random draw, so the offer,
//! the pickup and the delivery agree and a reload changes nothing.

use crate::data::border_yards::{border_yards, BorderYard, BorderYards, Consignee};
use crate::models::career::JobView;
use crate::pyfmt::py_str_float;
use crate::sim::hos::clock_hour;
use crate::sim::season::day_of_week;

/// The data and yard for a facility id, when it is a border transfer yard.
pub fn border_yard(facility_id: &str) -> Option<(&'static BorderYards, &'static BorderYard)> {
    if facility_id.is_empty() {
        return None;
    }
    let data = border_yards()?;
    data.yard(facility_id).map(|yard| (data, yard))
}

fn consignee_key<J: JobView + ?Sized>(job: &J) -> String {
    [
        job.cargo_key(),
        job.origin_facility_id(),
        job.origin_location(),
        job.destination_facility_id(),
        &py_str_float(job.distance_mi()),
        &py_str_float(job.weight_tons()),
    ]
    .join("|")
}

/// The Mexican consignee of a southbound load, or None for any other load.
pub fn southbound_consignee<J: JobView + ?Sized>(job: &J) -> Option<&'static Consignee> {
    let (data, _) = border_yard(job.destination_facility_id())?;
    data.consignee_for(&consignee_key(job))
}

/// The load card's destination phrase for a southbound load:
/// "for consignee Saltillo, Coahuila, delivered to SCI Cross Dock in Laredo
/// ...". None for any other load.
pub fn load_card_line<J: JobView + ?Sized>(job: &J, city: &str) -> Option<String> {
    let (data, yard) = border_yard(job.destination_facility_id())?;
    let consignee = data.consignee_for(&consignee_key(job))?;
    Some(data.load_card_line(yard, consignee, city))
}

/// The line spoken when a southbound trailer is dropped at its yard.
pub fn yard_drop_line<J: JobView + ?Sized>(job: &J) -> Option<String> {
    let (data, yard) = border_yard(job.destination_facility_id())?;
    let consignee = data.consignee_for(&consignee_key(job))?;
    Some(data.yard_drop_line(yard, consignee))
}

/// Minutes a northbound trailer waits on paperwork at its yard; 0 for a
/// load that does not start at a border yard.
pub fn paperwork_minutes<J: JobView + ?Sized>(job: &J) -> f64 {
    match border_yard(job.origin_facility_id()) {
        Some((data, _)) => f64::from(data.paperwork_minutes(&format!(
            "paperwork|{}|{}",
            job.origin_facility_id(),
            py_str_float(job.distance_mi())
        ))),
        None => 0.0,
    }
}

/// The spoken paperwork wait at check-in, with the bridge-hours story when
/// the yard is checked into while the bridge is closed to trucks.
pub fn paperwork_wait_line<J: JobView + ?Sized>(job: &J, game_hours: f64) -> Option<String> {
    let (data, yard) = border_yard(job.origin_facility_id())?;
    let minutes = paperwork_minutes(job);
    if minutes <= 0.0 {
        return None;
    }
    Some(data.wait_line(
        yard,
        minutes as u32,
        day_of_week(game_hours) as u32,
        clock_hour(game_hours).floor() as u32,
    ))
}
