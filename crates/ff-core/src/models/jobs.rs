//! Cargo catalog and job generation (port of `freight_fate/models/jobs.py`).
//!
//! Jobs are generated at a city's freight locations, pay by real route miles,
//! and gate special cargo behind license endorsements earned through the
//! career system.

use serde_json::{json, Map, Value};

use crate::data::world::World;
use crate::data::world_constants::{lookup, LOCATION_TYPE_LABELS};
use crate::data::world_models::CrossingLoad;
use crate::models::border_freight::load_card_line;
use crate::models::career::JobView;
use crate::models::market::market_condition;
use crate::models::save_migration::json_f64;
use crate::models::start_options::pay_plan_for_key;
use crate::models::trailers::{required_program_text, trailer_keys_for_cargo};
use crate::pyfmt::{fmt_f, fmt_grouped, py_str_float, round_py_n};
use crate::speech_text::{the_city, typed_name};

mod board;
mod deadline;
pub mod hos_stops;
pub mod relay;
#[cfg(test)]
mod tests;

pub use board::{reachable_cities, JobBoard, OfferOptions};
pub use deadline::{
    curve_ceilings, dispatch_deadline_hours, fair_active_deadline, minimum_pay_for_level, plan_hos,
    remaining_route_hos_plan, required_hours, route_drive_hours, route_drive_hours_over,
    route_planning_limit, route_required_hours, segment_hours, CurveBand, HosPlan,
    ACTIVE_TRIP_FAIRNESS_SLACK, DEADLINE_AVG_MPH, DEADLINE_DISPATCH_MIN_SLACK_H,
    DEADLINE_DISPATCH_SLACK_RANGE, DEADLINE_MIN_SEGMENT_MPH, DEADLINE_PLANNING_SPEED_FACTOR,
    DEADLINE_SAMPLE_MI, DISPATCH_FLAT_MINIMUM_BY_LEVEL, HOOKUP_FEE,
    LONG_HAUL_MINIMUM_RATE_BY_LEVEL, SHORT_HAUL_FULL_PREMIUM_MI, SHORT_HAUL_RATE_BY_LEVEL,
    SHORT_HAUL_TAPER_END_RATE_BY_LEVEL,
};

mod catalog;
pub use catalog::{
    cargo_type, credentials_clause, endorsement_label, facility_cargo, facility_cargo_table,
    market_tag_cargo_bonus, CargoType, CARGO_CATALOG, FACILITY_SELECTION_WEIGHTS,
    MARKET_TAG_CARGO_BONUS,
};

pub fn facility_label(location_type: &str) -> String {
    lookup(LOCATION_TYPE_LABELS, location_type)
        .map(str::to_string)
        .unwrap_or_else(|| location_type.replace('_', " "))
}

pub fn facility_text(
    location_type: &str,
    location_name: &str,
    city: &str,
    locality: &str,
) -> String {
    if location_type == "metro_market" || is_legacy_facility_name(city, location_name) {
        return format!("{} metro freight market", the_city(city));
    }
    let place = if !locality.is_empty() && !location_name.contains(locality) {
        format!(" near {locality}")
    } else {
        String::new()
    };
    // Drop the type prefix when the proper name already carries it, so
    // "cross-dock Chicago Cross-Dock in Chicago" is not the type twice
    // (research doc R6).
    format!(
        "{}{place} in {city}",
        typed_name(&facility_label(location_type), location_name, " ")
    )
}

pub fn facility_offer_text(
    location_type: &str,
    location_name: &str,
    city: &str,
    locality: &str,
) -> String {
    if location_type == "metro_market" || is_legacy_facility_name(city, location_name) {
        return format!("{} metro freight market", the_city(city));
    }
    let place = if !locality.is_empty() && !location_name.contains(locality) {
        format!(" near {locality}")
    } else {
        String::new()
    };
    format!("{location_name}{place} in {city}")
}

/// Ice cream and frozen-food docks still use the single chilled setpoint, so the
/// spoken cargo label says chilled rather than frozen. The facility name itself
/// is never rewritten.
pub fn facility_ships_frozen_as_chilled(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("ice cream")
        || n.contains("frozen food")
        || n.contains("frozen foods")
        || n.contains("fresh frozen")
        || (n.contains("frozen") && (n.contains("warehouse") || n.contains("distribution")))
}

fn is_legacy_facility_name(city: &str, location_name: &str) -> bool {
    let normalized = location_name.trim().to_lowercase();
    let city_lower = city.to_lowercase();
    normalized.is_empty()
        || normalized == city_lower
        || normalized == format!("{city_lower} freight market")
        || normalized == format!("{city_lower} metro freight market")
}

/// A dispatched (or offered) load.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub cargo: &'static CargoType,
    pub weight_tons: f64,
    /// city key; legacy saves hold the old display name
    pub origin: String,
    pub origin_location: String,
    /// city key; legacy saves hold the old display name
    pub destination: String,
    /// shortest-route miles, used for pay and deadline
    pub distance_mi: f64,
    pub pay: f64,
    pub deadline_game_h: f64,
    /// market multiplier already applied to pay
    pub market_mult: f64,
    pub origin_type: String,
    pub destination_location: String,
    pub destination_type: String,
    pub origin_facility_id: String,
    pub destination_facility_id: String,
    pub origin_locality: String,
    pub destination_locality: String,
    /// empty reposition run: relocate, no cargo or pay
    pub bobtail: bool,
    // A carrier-ASSIGNED reposition (dispatch sent you empty to a nearby city)
    // rather than a driver-chosen bobtail (self-serve, owner-operators only).
    // Always False when bobtail is False. Distinguishes the settlement and
    // abandon-penalty rules: an assigned reposition still pays a reduced
    // empty-mile rate and still earns mileage XP, and walking away from it
    // costs reputation, not the flat dollar penalty a real load carries.
    pub assigned: bool,
    // Speakable city names, set at dispatch. Legacy payloads predate these
    // fields, but there origin/destination hold the old spoken display name,
    // so the fallback properties below always read cleanly.
    pub origin_spoken: String,
    pub destination_spoken: String,
    // The deadline was stretched to cover a 10-hour rest the driver's
    // CURRENT shift clock will force mid-run (a fresh clock would not
    // have needed one). Spoken so the long number reads as the law, not
    // dispatcher generosity.
    pub deadline_covers_rest: bool,
    pub seasonal_weight_limit: Option<SeasonalWeightLimit>,
}

/// Seasonal weight restriction recorded when a job is dispatched.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonalWeightLimit {
    pub highway: String,
    pub percent: u8,
}

/// The keyword arguments of `Job.describe`, each with its Python default.
#[derive(Debug, Clone, Default)]
pub struct DescribeOptions<'a> {
    pub index: Option<usize>,
    pub total: Option<usize>,
    /// `"Pays"` when empty.
    pub pay_label: &'a str,
    pub trailer_note: &'a str,
    pub display_pay: Option<f64>,
    pub market_preview: &'a str,
    pub distance_text: &'a str,
}

impl Job {
    /// The load as a tunnel or bridge judges it, on a stock rig.
    pub fn crossing_load(&self) -> CrossingLoad {
        if self.bobtail {
            return CrossingLoad::standard_rig(false);
        }
        let cargo_lb = (self.weight_tons * 2000.0).round().max(0.0) as u32;
        CrossingLoad::standard_rig(true).with_cargo(self.cargo.key, self.cargo.tank, cargo_lb)
    }

    /// `Job(cargo, weight_tons, origin, origin_location, destination,
    /// distance_mi, pay, deadline_game_h)` with every other field at its
    /// default.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cargo: &'static CargoType,
        weight_tons: f64,
        origin: &str,
        origin_location: &str,
        destination: &str,
        distance_mi: f64,
        pay: f64,
        deadline_game_h: f64,
    ) -> Self {
        Job {
            cargo,
            weight_tons,
            origin: origin.to_string(),
            origin_location: origin_location.to_string(),
            destination: destination.to_string(),
            distance_mi,
            pay,
            deadline_game_h,
            market_mult: 1.0,
            origin_type: "terminal".to_string(),
            destination_location: String::new(),
            destination_type: "terminal".to_string(),
            origin_facility_id: String::new(),
            destination_facility_id: String::new(),
            origin_locality: String::new(),
            destination_locality: String::new(),
            bobtail: false,
            assigned: false,
            origin_spoken: String::new(),
            destination_spoken: String::new(),
            deadline_covers_rest: false,
            seasonal_weight_limit: None,
        }
    }

    pub fn spoken_origin(&self) -> &str {
        if self.origin_spoken.is_empty() {
            &self.origin
        } else {
            &self.origin_spoken
        }
    }

    pub fn spoken_destination(&self) -> &str {
        if self.destination_spoken.is_empty() {
            &self.destination
        } else {
            &self.destination_spoken
        }
    }

    /// `job.describe()` with every keyword at its default.
    pub fn describe_plain(&self) -> String {
        self.describe(&DescribeOptions::default())
    }

    /// `job.describe(index, total)`.
    pub fn describe_numbered(&self, index: usize, total: usize) -> String {
        self.describe(&DescribeOptions {
            index: Some(index),
            total: Some(total),
            ..DescribeOptions::default()
        })
    }

    pub fn describe(&self, opts: &DescribeOptions<'_>) -> String {
        let prefix = match opts.index {
            Some(index) => format!(
                "Job {index} of {}: ",
                opts.total
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "None".to_string())
            ),
            None => String::new(),
        };
        let condition = market_condition(self.market_mult);
        let market = if condition != "steady" {
            format!(" Lane note: Market is {condition}.")
        } else {
            String::new()
        };
        let preview = if opts.market_preview.is_empty() {
            String::new()
        } else {
            format!(" {}", opts.market_preview)
        };
        let endorsement = if self.cargo.credentials.is_empty() {
            String::new()
        } else {
            format!(" Requires {}.", credentials_clause(self.cargo.credentials))
        };
        let origin = format!("from {}", self.origin_offer_text());
        // A load for a border transfer yard names its Mexican consignee;
        // the yard in the US city is still where the trailer is delivered.
        let dest = load_card_line(self, self.spoken_destination())
            .unwrap_or_else(|| format!("to {}", self.destination_offer_text()));
        let trailer = if opts.trailer_note.is_empty() {
            String::new()
        } else {
            format!(" {}", opts.trailer_note)
        };
        let seasonal_weight = match self.seasonal_weight_limit.as_ref() {
            Some(limit) => format!(
                " Spring weight limits on the {}: axle loads held to {} percent of the legal limit.",
                limit.highway, limit.percent
            ),
            None => String::new(),
        };
        let pay = opts.display_pay.unwrap_or(self.pay);
        let pay_label = if opts.pay_label.is_empty() {
            "Pays"
        } else {
            opts.pay_label
        };
        let distance = if opts.distance_text.is_empty() {
            format!("{} miles", fmt_f(self.distance_mi, 0))
        } else {
            opts.distance_text.to_string()
        };
        let rest = if self.deadline_covers_rest {
            ", planned around the 10-hour rest your hours will force. "
        } else {
            ". "
        };
        format!(
            "{prefix}{} tons of {} {origin} {dest}. {distance}. {pay_label} {} dollars. Deadline {} hours{rest}Equipment: {}.{trailer}{seasonal_weight}{preview}{market}{endorsement}",
            fmt_f(self.weight_tons, 0),
            self.spoken_cargo_label(),
            fmt_grouped(pay, 0),
            fmt_f(self.deadline_game_h, 0),
            self.cargo.equipment_text(),
        )
    }

    pub fn origin_facility_text(&self) -> String {
        facility_text(
            &self.origin_type,
            &self.origin_location,
            self.spoken_origin(),
            &self.origin_locality,
        )
    }

    pub fn origin_offer_text(&self) -> String {
        facility_offer_text(
            &self.origin_type,
            &self.origin_location,
            self.spoken_origin(),
            &self.origin_locality,
        )
    }

    pub fn destination_facility_text(&self) -> String {
        facility_text(
            &self.destination_type,
            &self.destination_location,
            self.spoken_destination(),
            &self.destination_locality,
        )
    }

    pub fn destination_offer_text(&self) -> String {
        facility_offer_text(
            &self.destination_type,
            &self.destination_location,
            self.spoken_destination(),
            &self.destination_locality,
        )
    }

    /// Player-facing cargo name. Frozen-food docks keep their real facility
    /// names; the load is still labeled chilled at the v1 setpoint.
    pub fn spoken_cargo_label(&self) -> &'static str {
        let frozen_dock = facility_ships_frozen_as_chilled(&self.origin_location)
            || facility_ships_frozen_as_chilled(&self.destination_location);
        if frozen_dock && self.cargo.key == "food" {
            return "chilled food";
        }
        if frozen_dock && self.cargo.needs_reefer() {
            return "chilled goods";
        }
        self.cargo.label
    }

    pub fn equipment_text(&self) -> String {
        self.cargo.equipment_text()
    }

    /// `job.locked_reason(endorsements, level, trailer_programs=...,
    /// carrier_trailer_support=...)`; `""` when the job is open.
    pub fn locked_reason<S: AsRef<str>>(
        &self,
        endorsements: &[S],
        level: i64,
        trailer_programs: Option<&[S]>,
        carrier_trailer_support: bool,
    ) -> String {
        if level < self.cargo.min_level {
            return format!("Level {} drivers unlock this cargo.", self.cargo.min_level);
        }
        let missing = self.cargo.missing_credentials(endorsements);
        if !missing.is_empty() {
            return format!("Requires {}.", credentials_clause(&missing));
        }
        if !carrier_trailer_support {
            if let Some(programs) = trailer_programs {
                let required = self.required_trailers();
                let supported = programs.iter().any(|p| required.contains(&p.as_ref()));
                if !supported {
                    return format!(
                        "Requires {} trailer program.",
                        required_program_text(self.cargo.key)
                    );
                }
            }
        }
        String::new()
    }

    pub fn required_trailers(&self) -> &'static [&'static str] {
        trailer_keys_for_cargo(self.cargo.key)
    }

    /// Final payment given delivery time and cargo condition.
    ///
    /// On-time pay works like real shipper scorecards: hitting the delivery
    /// window earns the full flat bonus, with no extra reward for racing in
    /// far ahead of the appointment. `on_time_bonus` defaults to 0.15.
    pub fn payout(&self, hours_taken: f64, damage_pct: f64, on_time_bonus: f64) -> f64 {
        let mut pay = self.pay;
        if hours_taken <= self.deadline_game_h {
            pay *= 1.0 + on_time_bonus;
        } else {
            let hours_late = hours_taken - self.deadline_game_h;
            pay *= (1.0 - 0.08 * hours_late).max(0.4);
        }
        if self.cargo.fragile {
            pay *= (1.0 - damage_pct / 100.0).max(0.5);
        } else {
            pay *= (1.0 - damage_pct / 200.0).max(0.7);
        }
        round_py_n(pay, 2)
    }

    /// `job.payout(hours_taken, damage_pct)` with the default on-time bonus.
    pub fn payout_default(&self, hours_taken: f64, damage_pct: f64) -> f64 {
        self.payout(hours_taken, damage_pct, 0.15)
    }
}

impl JobView for Job {
    fn distance_mi(&self) -> f64 {
        self.distance_mi
    }
    fn weight_tons(&self) -> f64 {
        self.weight_tons
    }
    fn deadline_game_h(&self) -> f64 {
        self.deadline_game_h
    }
    fn cargo_key(&self) -> &str {
        self.cargo.key
    }
    fn origin_type(&self) -> &str {
        &self.origin_type
    }
    fn origin_facility_id(&self) -> &str {
        &self.origin_facility_id
    }
    fn origin_location(&self) -> &str {
        &self.origin_location
    }
    fn destination_type(&self) -> &str {
        &self.destination_type
    }
    fn destination_facility_id(&self) -> &str {
        &self.destination_facility_id
    }
    fn destination_location(&self) -> &str {
        &self.destination_location
    }
}

/// Python truthiness of a JSON value (`bool(value)`).
pub(crate) fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `str(value)` of a JSON scalar.
pub(crate) fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => py_str_float(f),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// `job_payload(job)`: the save snapshot of a job.
pub fn job_payload(job: &Job) -> Map<String, Value> {
    let value = json!({
        "cargo": job.cargo.key,
        "weight_tons": job.weight_tons,
        "origin": job.origin,
        "origin_location": job.origin_location,
        "origin_type": job.origin_type,
        "origin_facility_id": job.origin_facility_id,
        "origin_locality": job.origin_locality,
        "destination": job.destination,
        "destination_location": job.destination_location,
        "destination_type": job.destination_type,
        "destination_facility_id": job.destination_facility_id,
        "destination_locality": job.destination_locality,
        "origin_spoken": job.origin_spoken,
        "destination_spoken": job.destination_spoken,
        "distance_mi": job.distance_mi,
        "pay": job.pay,
        "deadline_game_h": job.deadline_game_h,
        "market_mult": job.market_mult,
        "bobtail": job.bobtail,
        "assigned": job.assigned,
        "deadline_covers_rest": job.deadline_covers_rest,
        "seasonal_weight_limit": job.seasonal_weight_limit.as_ref().map(|limit| {
            json!({
                "highway": limit.highway.as_str(),
                "percent": limit.percent,
            })
        }),
    });
    match value {
        Value::Object(map) => map,
        _ => unreachable!("json! object"),
    }
}

fn str_or(data: &Map<String, Value>, key: &str, default: &str) -> String {
    match data.get(key) {
        None => default.to_string(),
        Some(v) => py_str(v),
    }
}

/// A field that is present and truthy (`data.get(key) or ...`), as `str()`.
fn truthy_str(data: &Map<String, Value>, key: &str) -> Option<String> {
    let value = data.get(key)?;
    if !py_truthy(value) {
        return None;
    }
    Some(py_str(value))
}

/// `job_from_payload(data)`: `None` where Python raised (`KeyError` on a
/// missing cargo / origin / destination / distance / pay / deadline).
pub fn job_from_payload(data: &Map<String, Value>) -> Option<Job> {
    let cargo = cargo_type(data.get("cargo")?.as_str()?)?;
    let origin = py_str(data.get("origin")?);
    let destination = py_str(data.get("destination")?);
    let origin_location = truthy_str(data, "origin_location")
        .or_else(|| truthy_str(data, "origin_facility"))
        .unwrap_or_else(|| {
            format!(
                "{} freight market",
                truthy_str(data, "origin_spoken").unwrap_or_else(|| origin.clone())
            )
        });
    let destination_location = truthy_str(data, "destination_location")
        .or_else(|| truthy_str(data, "destination_facility"))
        .unwrap_or_else(|| {
            format!(
                "{} freight market",
                truthy_str(data, "destination_spoken").unwrap_or_else(|| destination.clone())
            )
        });
    let number = |key: &str| -> Option<f64> {
        let v = data.get(key)?;
        let f = json_f64(Some(v), f64::NAN);
        (!f.is_nan()).then_some(f)
    };
    let mut job = Job::new(
        cargo,
        number("weight_tons")?,
        &origin,
        &origin_location,
        &destination,
        number("distance_mi")?,
        number("pay")?,
        number("deadline_game_h")?,
    );
    job.market_mult = json_f64(data.get("market_mult"), 1.0);
    job.origin_type = str_or(data, "origin_type", "metro_market");
    job.destination_location = destination_location;
    job.destination_type = str_or(data, "destination_type", "metro_market");
    job.origin_facility_id = str_or(data, "origin_facility_id", "");
    job.destination_facility_id = str_or(data, "destination_facility_id", "");
    job.origin_locality = str_or(data, "origin_locality", "");
    job.destination_locality = str_or(data, "destination_locality", "");
    job.bobtail = data.get("bobtail").is_some_and(py_truthy);
    job.assigned = data.get("assigned").is_some_and(py_truthy);
    job.origin_spoken = str_or(data, "origin_spoken", "");
    job.destination_spoken = str_or(data, "destination_spoken", "");
    job.deadline_covers_rest = data.get("deadline_covers_rest").is_some_and(py_truthy);
    job.seasonal_weight_limit = data
        .get("seasonal_weight_limit")
        .and_then(Value::as_object)
        .and_then(|limit| {
            let highway = limit.get("highway")?.as_str()?;
            let percent = u8::try_from(limit.get("percent")?.as_u64()?).ok()?;
            (1..=100).contains(&percent).then(|| SeasonalWeightLimit {
                highway: highway.to_string(),
                percent,
            })
        });
    Some(job)
}

/// Resolve legacy save city names to canonical keys, keeping speech intact.
///
/// Pre-slug payloads store display names ("Jackson, Michigan") in
/// `origin`/`destination` and no spoken fields. Capture the speakable form
/// first, then rewrite the identity to the current key so every world lookup
/// downstream sees canonical keys. Unknown cities pass through unchanged for
/// the caller's usual fallbacks.
pub fn normalize_job_cities(job: &mut Job, world: &World) {
    if job.origin_spoken.is_empty() {
        job.origin_spoken = world.spoken_city(&job.origin, None);
    }
    if job.destination_spoken.is_empty() {
        job.destination_spoken = world.spoken_city(&job.destination, None);
    }
    job.origin = world.resolve_city_key(&job.origin);
    job.destination = world.resolve_city_key(&job.destination);
}

// Carriers really do pay empty (deadhead) miles, just at a reduced rate --
// there is no freight paying the bill, only the value of getting the truck
// to where freight is. 60 percent of the loaded per-mile floor is the
// industry's common shape for deadhead/reposition pay, so an ASSIGNED
// reposition (dispatch sent you, not a self-serve bobtail) pays that share
// of the carrier's loaded practical-mile rate (CompanyPayPlan.min_per_mile,
// the same floor a real load's wage is built on in
// business.company_driver_pay). A self-serve bobtail keeps paying nothing:
// that one is the driver's own choice to burn fuel, not dispatch's.
pub const ASSIGNED_REPOSITION_PAY_FRACTION: f64 = 0.6;

/// An empty 'bobtail' run to relocate to a nearby city.
///
/// Reuses the normal delivery drive for fuel, weather, and save/resume, but
/// carries no cargo. A self-serve bobtail (`assigned=false`, the default) is
/// player-chosen commercial repositioning and pays nothing. The ELD records
/// driving time while moving and on-duty time while stopped. A carrier-assigned
/// reposition (`assigned=true`) pays a
/// reduced per-mile rate -- see ASSIGNED_REPOSITION_PAY_FRACTION. Either
/// way, on arrival the player simply parks at the destination city's hub and
/// can shop its dispatch board.
pub fn make_reposition_job(
    world: &World,
    origin: &str,
    destination: &str,
    assigned: bool,
    carrier_key: Option<&str>,
) -> Option<Job> {
    let origin = world.resolve_city_key(origin);
    let destination = world.resolve_city_key(destination);
    let route = world
        .supported_route(&origin, &destination, None)
        .ok()
        .flatten()?;
    let miles = round_py_n(route.miles(), 1);
    let dest = world.city(&destination).ok()?;
    let dest_loc = dest.locations.first();
    let mut pay = 0.0;
    if assigned {
        let plan = pay_plan_for_key(carrier_key);
        pay = round_py_n(
            miles * plan.min_per_mile * ASSIGNED_REPOSITION_PAY_FRACTION,
            2,
        );
    }
    let mut job = Job::new(
        cargo_type("general").expect("general freight is on the catalog"),
        0.0,
        &origin,
        "company yard",
        &destination,
        miles,
        pay,
        required_hours(miles, Some(&route), Some(world), None) * 3.0 + 24.0,
    );
    job.origin_type = "company_yard".to_string();
    job.destination_location = match dest_loc {
        Some(loc) => loc.name.clone(),
        None => format!("{} yard", dest.name),
    };
    job.destination_type = match dest_loc {
        Some(loc) => loc.facility_type.clone(),
        None => "company_yard".to_string(),
    };
    job.bobtail = true;
    job.assigned = assigned;
    // Always state-qualified: a dispatch names places the player may never
    // have heard of, and "McCall, Idaho" orients where "McCall" cannot
    // (player request).
    job.origin_spoken = world.spoken_city(&origin, Some(true));
    job.destination_spoken = world.spoken_city(&destination, Some(true));
    Some(job)
}

// Shortest job the dispatch board will offer. Cities stand for whole freight
// areas, so a haul below this is a trivial across-town hop (e.g. New York to
// Newark at 11 miles) rather than a real dispatch.
pub const MIN_JOB_DISTANCE_MI: f64 = 25.0;

// The dispatch board itself grows with seniority: proven drivers get shown
// more freight per visit. These are career-ladder unlocks, spoken at the
// matching level-up.
pub const BOARD_OFFER_LEVELS: &[(i64, usize)] = &[(6, 6), (10, 7), (12, 8)];
pub const BASE_BOARD_OFFERS: usize = 5;

// Specialized company drivers (level 11+) see endorsement freight weighted
// up instead of down, and premium-lane drivers (level 12+) see long freight
// favored on the board.
pub const SPECIALIZED_FREIGHT_LEVEL: i64 = 11;
pub const SPECIALIZED_FREIGHT_WEIGHT: f64 = 1.25;
// A driver who holds every credential a cargo class asks for is the driver
// dispatch calls for it. Before 2026-09-16 the board offered a hazmat holder
// 1.2 percent placarded and 0.25 percent bulk fuel map-wide (zero of 1,186
// staging deliveries), so the course and background check never paid back.
// The cargo weight multiplies at the shipper, and the shipper itself is
// favoured when it ships something the driver is credentialed for.
pub const HELD_CREDENTIAL_CARGO_WEIGHT: f64 = 4.0;
pub const HELD_CREDENTIAL_FACILITY_BONUS: f64 = 1.5;

/// Origin-selection multiplier for facilities tagged `thin_outbound`.
/// Keeps inbound (destination) weight unchanged while cutting how often the
/// pin is chosen as a shipper so inbound clearly outweighs outbound.
pub const THIN_OUTBOUND_ORIGIN_WEIGHT: f64 = 0.25;
pub const PREMIUM_LANE_LEVEL: i64 = 12;
pub const PREMIUM_LANE_LONG_HAUL_BIAS: f64 = 0.5;

/// Canonical from:to lane for dispatch-variety memory.
pub fn lane_key(world: &World, job: &Job) -> String {
    format!(
        "{}:{}",
        world.resolve_city_key(&job.origin),
        world.resolve_city_key(&job.destination)
    )
}

/// How many offers the dispatch board shows at this career level.
pub fn board_offer_count(level: i64) -> usize {
    let mut count = BASE_BOARD_OFFERS;
    for (min_level, offers) in BOARD_OFFER_LEVELS {
        if level >= *min_level {
            count = count.max(*offers);
        }
    }
    count
}

// Career-arc distance caps: short regional hops while learning the ropes,
// cross-country hauls unlocking as a progression reward around level 4-5.
pub const LEVEL_DISTANCE_CAPS: &[(i64, f64)] =
    &[(1, 300.0), (2, 450.0), (3, 650.0), (4, 850.0), (5, 1200.0)];
// Above level 5 the cap keeps growing gradually toward the longest supported
// coast-to-coast corridor (~2,800 miles). The old 500-mile-per-level growth
// blew past every real U.S. route by level 12, so haul length stopped feeling
// like progression; this keeps longer freight unlocking into the late teens.
pub const LEVEL_DISTANCE_CAP_STEP_MI: f64 = 120.0;
pub const MAX_DISPATCH_DISTANCE_MI: f64 = 3000.0;
/// what counts as a cross-country haul
pub const LONG_HAUL_MILES: f64 = 600.0;

/// `LEVEL_DISTANCE_CAPS[level]`.
pub fn level_distance_cap(level: i64) -> Option<f64> {
    LEVEL_DISTANCE_CAPS
        .iter()
        .find(|(l, _)| *l == level)
        .map(|(_, cap)| *cap)
}
