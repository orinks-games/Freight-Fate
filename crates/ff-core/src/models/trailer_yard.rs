//! Trailer yards, drop-and-hook, and the trailer you are actually pulling
//! (port of `freight_fate/models/trailer_yard.py`).
//!
//! Freight Fate already knew what *kind* of trailer a load needs (see
//! `trailers::CARGO_TRAILER_COMPATIBILITY`), but not that trailers are
//! physical things sitting in yards. That gap swallowed the single biggest
//! difference between one pickup and another: whether the driver waits at a
//! dock while the freight goes on, or drops the box they came in with and
//! hooks one that was loaded hours ago.
//!
//! Both are real and the trade is real:
//!
//! - **Live load.** Back into a dock and wait. An hour if the shipper is
//!   sharp, considerably longer if not -- and past the free time the carrier
//!   bills detention, which is money the driver earns for sitting still.
//! - **Drop and hook.** Twenty-odd minutes: drop, hook, crank the gear, do
//!   the paperwork, gone. It is why big carriers chase it. The catch is that
//!   you get the trailer you get, and nobody in that yard has looked at it
//!   since it was parked.
//!
//! Which one a load is depends on the facility. High-volume freight -- a
//! distribution centre, a parcel hub, an intermodal ramp -- stages preloaded
//! trailers as a matter of course. A farm elevator or a quarry does not.
//!
//! Nothing here is saved. Yards are derived from the facility's own identity,
//! so the same dock always holds the same iron without a byte of profile
//! schema; the one trailer that has to persist is the one currently hooked,
//! and that rides in the trip snapshot with the rest of the run.
//!
//! The Python `settlement <-> trailer_yard` import cycle is broken here by
//! [`DetentionCharge`]: `detention_charge` returns that local record, carrying
//! exactly the `SettlementCharge` fields, and `models::settlement` (wave 2)
//! converts it rather than this module importing the ledger type.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::career::JobView;
use crate::models::trailers::{owned_trailer_for_cargo, trailer_keys_for_cargo, trailer_type};
use crate::pyfmt::{fmt_f, py_str_float, round_py_n};

#[cfg(test)]
mod tests;

/// Facilities that move enough freight to keep loaded trailers standing ready.
pub const DROP_YARD_FACILITY_TYPES: &[&str] = &[
    "cross_dock",
    "grocery_retail_dc",
    "retail_distribution",
    "distribution",
    "parcel_hub",
    "port_terminal",
    "intermodal_ramp",
    "automotive_plant",
    "company_yard",
    "terminal",
];

/// Facilities that sometimes have a drop yard and sometimes do not; the coin
/// is weighted by how much freight the type moves.
pub const SOMETIMES_DROP_YARD: &[(&str, f64)] = &[
    ("dry_warehouse", 0.45),
    ("food_processor", 0.40),
    ("cold_storage", 0.40),
    ("steel_industrial", 0.30),
    ("lumber_paper", 0.30),
    ("chemical_petroleum_terminal", 0.25),
];

/// Drop, hook, crank the gear, sign, gone.
pub const DROP_HOOK_MIN: f64 = 25.0;
/// The shipper's own hour at the dock.
pub const LIVE_LOAD_MIN: f64 = 60.0;
// Real detention terms: two hours free, then the carrier bills by the hour and
// the driver is paid for the wait. Under two hours nobody owes anybody anything.
pub const DETENTION_FREE_MIN: f64 = 120.0;
pub const DETENTION_PER_HOUR: f64 = 45.0;
/// How long a slow shipper can hold a truck past the scheduled hour.
pub const LIVE_LOAD_SLOW_EXTRA_MIN: f64 = 165.0;

pub const MODE_DROP_HOOK: &str = "drop_and_hook";
pub const MODE_LIVE: &str = "live_load";

/// One physical trailer, with a number on the side and a history behind it.
/// Rides in the trip snapshot, so it serialises by field name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrailerUnit {
    pub number: String,
    pub trailer_key: String,
    /// 0 is a new box, 100 is a candidate for the bone yard.
    pub condition_pct: f64,
}

impl TrailerUnit {
    pub fn new(
        number: impl Into<String>,
        trailer_key: impl Into<String>,
        condition_pct: f64,
    ) -> Self {
        TrailerUnit {
            number: number.into(),
            trailer_key: trailer_key.into(),
            condition_pct,
        }
    }

    /// `TRAILER_CATALOG[self.trailer_key].label`.
    pub fn label(&self) -> &'static str {
        trailer_type(&self.trailer_key)
            .unwrap_or_else(|| panic!("{:?} is not in TRAILER_CATALOG", self.trailer_key))
            .label
    }

    pub fn spoken_name(&self) -> String {
        format!("{} {}", self.label().to_lowercase(), self.number)
    }

    pub fn condition_text(&self) -> &'static str {
        if self.condition_pct < 20.0 {
            return "in good shape";
        }
        if self.condition_pct < 45.0 {
            return "well used but sound";
        }
        if self.condition_pct < 70.0 {
            return "rough around the edges";
        }
        "in poor shape"
    }

    /// What an inspector would write up, or None if it would pass clean.
    ///
    /// Drop-and-hook's real risk in one property: the trailer was parked by
    /// somebody else and nobody has been under it since.
    pub fn defect(&self) -> Option<&'static str> {
        if self.condition_pct < 55.0 {
            return None;
        }
        if self.condition_pct < 70.0 {
            return Some("trailer marker lamp out");
        }
        if self.condition_pct < 85.0 {
            return Some("trailer brake out of adjustment");
        }
        Some("worn trailer tire below tread depth")
    }

    pub fn describe(&self) -> String {
        let mut text = format!(
            "Hooked to {}, {}.",
            self.spoken_name(),
            self.condition_text()
        );
        if let Some(defect) = self.defect() {
            text.push_str(&format!(" Walking around it you find a {defect}."));
        }
        text
    }
}

/// `int.from_bytes(sha256("|".join(str(part) ...)).digest()[:6], "big")`.
fn seed(parts: &[&str]) -> u64 {
    let digest = Sha256::digest(parts.join("|").as_bytes());
    let mut eight = [0u8; 8];
    eight[2..].copy_from_slice(&digest[..6]);
    u64::from_be_bytes(eight)
}

/// Whether this facility stages loaded trailers instead of loading at a dock.
pub fn facility_has_drop_yard(facility_type: &str, facility_id: &str) -> bool {
    if DROP_YARD_FACILITY_TYPES.contains(&facility_type) {
        return true;
    }
    let Some((_, chance)) = SOMETIMES_DROP_YARD
        .iter()
        .find(|(t, _)| *t == facility_type)
    else {
        return false;
    };
    (seed(&["dropyard", facility_type, facility_id]) % 1000) as f64 / 1000.0 < *chance
}

/// The trailers standing in this yard that could take this freight.
///
/// Derived from the facility itself, so the same dock always holds the same
/// boxes across sessions and machines without persisting a thing.
pub fn yard_trailers(facility_type: &str, facility_id: &str, cargo_key: &str) -> Vec<TrailerUnit> {
    if !facility_has_drop_yard(facility_type, facility_id) {
        return Vec::new();
    }
    let keys = trailer_keys_for_cargo(cargo_key);
    let mut units = Vec::with_capacity(3);
    for slot in 0..3u64 {
        let seed = seed(&["trailer", facility_id, cargo_key, &slot.to_string()]);
        let key = keys[(seed % keys.len() as u64) as usize];
        // A fleet keeps its trailers serviceable, so the spread leans good and
        // tails off into the tired ones: squaring a flat roll puts roughly two
        // in three under half worn and leaves the write-ups rare enough to be
        // worth noticing. Flat, nearly half the yard failed a walk-around.
        let roll = (seed / 7) % 100;
        let condition = (roll * roll) as f64 / 100.0;
        let number = (1000 + (seed / 13) % 8999).to_string();
        units.push(TrailerUnit::new(number, key, condition));
    }
    units
}

/// `str(getattr(job, "origin_facility_id", "") or getattr(job, "origin_location", ""))`.
fn origin_id<J: JobView + ?Sized>(job: &J) -> &str {
    match job.origin_facility_id() {
        "" => job.origin_location(),
        id => id,
    }
}

/// The loaded trailer waiting for this dispatch, or None for a live load.
pub fn preloaded_trailer<J: JobView + ?Sized>(job: &J) -> Option<TrailerUnit> {
    let units = yard_trailers(job.origin_type(), origin_id(job), job.cargo_key());
    if units.is_empty() {
        return None;
    }
    // The yard hands over whichever box the shipper loaded for this run, which
    // from the driver's side is simply the one with the paperwork on it.
    let pick = seed(&[
        "assigned",
        job.origin_facility_id(),
        &py_str_float(job.distance_mi()),
    ]) % units.len() as u64;
    Some(units[pick as usize].clone())
}

/// What `owns_the_trailer` reads off a `Profile`: the two methods the Python
/// probed with `getattr(profile, ..., lambda: ...)`. Both default to the
/// "company driver" answer.
// TODO(lead): implement for models::profile::Profile (both exist there).
pub trait TrailerOwner {
    /// `profile.owns_equipment()`.
    fn owns_equipment(&self) -> bool {
        false
    }
    /// `profile.visible_owned_trailers()`.
    fn visible_owned_trailers(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Whether the player is pulling a trailer that belongs to them.
///
/// Nobody swaps an owner-operator's own trailer for one out of the yard, so
/// owning your box costs you drop-and-hook. That is a real trade and it is
/// the honest reason leased carriers move freight faster.
pub fn owns_the_trailer<P, J>(profile: &P, job: &J) -> bool
where
    P: TrailerOwner + ?Sized,
    J: JobView + ?Sized,
{
    if !profile.owns_equipment() {
        return false;
    }
    let owned = profile.visible_owned_trailers();
    owned_trailer_for_cargo(job.cargo_key(), &owned).is_some()
}

/// How this load gets onto the truck, and what it costs in time.
#[derive(Debug, Clone, PartialEq)]
pub struct PickupPlan {
    pub mode: &'static str,
    pub minutes: f64,
    pub trailer: Option<TrailerUnit>,
    pub detention_minutes: f64,
    pub reason: &'static str,
}

impl PickupPlan {
    pub fn is_drop_hook(&self) -> bool {
        self.mode == MODE_DROP_HOOK
    }

    pub fn detention_pay(&self) -> f64 {
        round_py_n(self.detention_minutes / 60.0 * DETENTION_PER_HOUR, 2)
    }
}

/// Decide live load or drop-and-hook for this dispatch, and time it.
pub fn pickup_plan<J, P>(job: &J, profile: &P) -> PickupPlan
where
    J: JobView + ?Sized,
    P: TrailerOwner + ?Sized,
{
    if owns_the_trailer(profile, job) {
        return PickupPlan {
            mode: MODE_LIVE,
            minutes: LIVE_LOAD_MIN,
            trailer: None,
            detention_minutes: 0.0,
            reason: "it is your own trailer, so this one loads at the dock",
        };
    }
    if let Some(trailer) = preloaded_trailer(job) {
        return PickupPlan {
            mode: MODE_DROP_HOOK,
            minutes: DROP_HOOK_MIN,
            trailer: Some(trailer),
            detention_minutes: 0.0,
            reason: "the yard has your load already on a trailer",
        };
    }
    // Live load: the shipper's own hour, and sometimes a good deal more.
    let seed = seed(&[
        "detention",
        job.origin_facility_id(),
        &py_str_float(job.distance_mi()),
    ]);
    let slow = (seed % 100) < 30;
    let extra = if slow {
        round_py_n(
            LIVE_LOAD_SLOW_EXTRA_MIN * ((seed / 100) % 100) as f64 / 100.0,
            0,
        )
    } else {
        0.0
    };
    let minutes = LIVE_LOAD_MIN + extra;
    let detention = (minutes - DETENTION_FREE_MIN).max(0.0);
    let reason = if detention > 0.0 {
        "the shipper is loading you at the dock, and they are running behind"
    } else {
        "the shipper is loading you at the dock"
    };
    PickupPlan {
        mode: MODE_LIVE,
        minutes,
        trailer: None,
        detention_minutes: detention,
        reason,
    }
}

/// The yard finds another box and brings it round.
pub const TRAILER_SWAP_MIN: f64 = 30.0;

/// The box the yard brings out after a driver refuses one.
///
/// A yard asked to swap a trailer does not hand over another bad one -- the
/// point of refusing is that somebody goes and finds a serviceable trailer.
/// Drawn from the same yard so the number is real, then guaranteed sound.
pub fn replacement_trailer<J: JobView + ?Sized>(
    job: &J,
    refused: Option<&TrailerUnit>,
) -> Option<TrailerUnit> {
    let refused = refused?;
    let units = yard_trailers(job.origin_type(), origin_id(job), job.cargo_key());
    if let Some(clean) = units
        .iter()
        .find(|unit| unit.defect().is_none() && unit.number != refused.number)
    {
        return Some(clean.clone());
    }
    // Every box in this yard has something on it, so the swap is a trailer
    // somebody actually went and fixed before handing it over.
    let seed = seed(&["swap", &refused.number, job.origin_facility_id()]);
    Some(TrailerUnit::new(
        (1000 + seed % 8999).to_string(),
        refused.trailer_key.clone(),
        12.0,
    ))
}

/// Back it in, drop, hook an empty, sign, gone.
pub const DROP_EMPTY_MIN: f64 = 20.0;
/// The receiver's own dock time.
pub const LIVE_UNLOAD_MIN: f64 = 45.0;

/// How the freight comes off: dropped in the yard, or unloaded at a dock.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryPlan {
    pub mode: &'static str,
    pub minutes: f64,
    /// False when the loaded box stays and you leave with an empty.
    pub keeps_trailer: bool,
    pub reason: &'static str,
}

impl DeliveryPlan {
    pub fn is_drop_hook(&self) -> bool {
        self.mode == MODE_DROP_HOOK
    }
}

/// Decide live unload or drop-and-hook at the receiver.
///
/// Same trade as the pickup end and the same rules decide it, with one
/// addition that only exists here: dropping the loaded box is also how a
/// driver gets rid of a trailer they have been dragging a defect around on
/// since the shipper. You hand the write-up to the receiver's yard along
/// with the freight, and hook a clean empty.
pub fn delivery_plan<J, P>(job: &J, profile: &P) -> DeliveryPlan
where
    J: JobView + ?Sized,
    P: TrailerOwner + ?Sized,
{
    if owns_the_trailer(profile, job) {
        return DeliveryPlan {
            mode: MODE_LIVE,
            minutes: LIVE_UNLOAD_MIN,
            keeps_trailer: true,
            reason: "it is your own trailer, so they unload you at the dock",
        };
    }
    let facility_type = job.destination_type();
    let facility_id = match job.destination_facility_id() {
        "" => job.destination_location(),
        id => id,
    };
    if facility_has_drop_yard(facility_type, facility_id) {
        return DeliveryPlan {
            mode: MODE_DROP_HOOK,
            minutes: DROP_EMPTY_MIN,
            keeps_trailer: false,
            reason: "the receiver takes the whole trailer and you leave with an empty",
        };
    }
    DeliveryPlan {
        mode: MODE_LIVE,
        minutes: LIVE_UNLOAD_MIN,
        keeps_trailer: true,
        reason: "the receiver is unloading you at the dock",
    }
}

/// `settlement.CARRIER_PAID`, repeated here so this module does not import
/// the ledger (see the module docs on the Python import cycle).
pub const CARRIER_PAID: &str = "carrier_paid";

/// Detention as a settlement line: the fields of `settlement.SettlementCharge`,
/// which `models::settlement` turns into its own record.
// TODO(lead): add `From<DetentionCharge> for SettlementCharge` in settlement.
#[derive(Debug, Clone, PartialEq)]
pub struct DetentionCharge {
    pub key: &'static str,
    pub label: &'static str,
    pub amount: f64,
    pub responsibility: &'static str,
    pub note: String,
}

/// Detention as a settlement line, or None when nobody owes anybody.
///
/// Detention is money *to* the driver, so it rides the ledger as a negative
/// charge -- the same way the settlement already nets carrier-paid lines.
pub fn detention_charge(plan: &PickupPlan) -> Option<DetentionCharge> {
    if plan.detention_minutes <= 0.0 {
        return None;
    }
    let hours = plan.detention_minutes / 60.0;
    Some(DetentionCharge {
        key: "detention_pay",
        label: "detention pay",
        amount: -plan.detention_pay(),
        responsibility: CARRIER_PAID,
        note: format!(
            "{} hours held past the free time at the shipper",
            fmt_f(hours, 1)
        ),
    })
}
