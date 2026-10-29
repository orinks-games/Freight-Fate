//! Dispatch-assigned company tractors, banded by career level (port of
//! `freight_fate/models/carrier_fleet.py`).
//!
//! Real fleets do not let a new hire shop for a truck: dispatch hands out
//! whatever the yard has, and better equipment follows seniority. Company
//! drivers therefore run a carrier-assigned tractor chosen from a level-band
//! fleet pool. The pick is deterministic per driver and carrier, so the same
//! career always meets the same truck, but two drivers at the same level can
//! be handed different iron.
//!
//! Junior drivers slip-seat, which is what actually happens to a new hire at
//! a big carrier: no truck is *yours*, and the yard hands you whatever is
//! free and suited to the run. So below [`DEDICATED_TRUCK_LEVEL`] the tractor
//! is chosen per load out of a small pool of spares, matched to the work -- a
//! bunk for a run that cannot be finished inside one driving shift, a
//! heavy-spec driveline for a heavy load, a day cab for a day's worth of city
//! stops. Seniority ends that: from [`DEDICATED_TRUCK_LEVEL`] the driver has
//! one assigned tractor and keeps it, which is the whole point of seniority.
//!
//! The pool is small and stable on purpose. Each tractor keeps its own wear,
//! damage, and fuel (`Profile.truck_conditions`), so a driver cycling through
//! three spares watches three trucks age rather than climbing into a factory
//! fresh one every load.
//!
//! Owner-operators are outside this module: after the level-18 buy-in the
//! tractor on the profile is player property (see `trucks::TRUCK_CATALOG`).

use sha2::{Digest, Sha256};

use crate::models::business_constants::is_owner_operator;
use crate::models::career::{CareerProfile, JobView};
use crate::models::enforcement::{
    self, clears_text, standing_band, standing_cause, StandingProfile,
};
use crate::models::solvency::{debt_owed, money_text};
use crate::models::trucks::{
    truck_model, truck_model_or_panic, TruckModel, CAB_DAY, CAB_SLEEPER, SPEC_HEAVY, SPEC_LIGHT,
};

#[cfg(test)]
mod tests;

/// Seniority earns a truck of your own. Below this the driver slip-seats.
pub const DEDICATED_TRUCK_LEVEL: i64 = 9;
/// Spare tractors the yard keeps free for a junior driver to draw from.
pub const SLIP_SEAT_POOL_SIZE: usize = 3;
/// Hours of service allow eleven hours of driving between rests, which at
/// real average lane speed is a bit over six hundred miles. Past this a run
/// cannot be finished inside one shift, so the truck needs a bunk in it.
pub const SLEEPER_RUN_MI: f64 = 500.0;
/// Payload past this is heavy-spec work: a light tractor can legally carry it
/// but will spend the whole trip wishing it were not.
pub const HEAVY_LOAD_TONS: f64 = 20.0;
/// Inside this, the run is a turn: back the same day, so a day cab is the
/// honest piece of equipment and the yard keeps its sleepers for the long
/// lanes.
pub const DAY_CAB_RUN_MI: f64 = 250.0;

#[derive(Debug, Clone, PartialEq)]
pub struct FleetTier {
    pub key: &'static str,
    pub min_level: i64,
    pub label: &'static str,
    /// `TRUCK_CATALOG` keys dispatch draws from.
    pub pool: &'static [&'static str],
    /// Spoken when dispatch hands the truck over.
    pub blurb: &'static str,
}

/// Tank capacity never shrinks across tiers, so a promotion never strands a
/// fuller tank than the new truck can hold.
pub const FLEET_TIERS: [FleetTier; 5] = [
    FleetTier {
        key: "yard_standard",
        min_level: 1,
        label: "yard standard",
        pool: &["rig"],
        blurb: "every new hire starts in the same trainer-spec tractor",
    },
    FleetTier {
        key: "regional",
        min_level: 4,
        label: "regional fleet",
        pool: &[
            "sunset_day_cab",
            "ridgeline_sleeper",
            "old_longnose",
            "city_shuttle",
            "dock_hopper",
            "short_haul_stubnose",
            "midroof_runner",
            "farm_road_workhorse",
            "trainer_day_cab",
            "hand_me_down_sleeper",
            "plain_jane_conventional",
            "yard_mule",
        ],
        blurb: "a newer regional tractor from the working fleet",
    },
    FleetTier {
        key: "long_haul",
        min_level: 9,
        label: "long-haul fleet",
        pool: &[
            "highline_sleeper",
            "big_bunk_conventional",
            "aero_cruiser",
            "long_run_midroof",
            "dry_lightning",
            "interstate_condo",
            "steel_hauler",
            "mountain_spec_hauler",
        ],
        blurb: "a long-haul sleeper with real interstate range",
    },
    FleetTier {
        key: "premium",
        min_level: 13,
        label: "premium fleet",
        pool: &[
            "summit_flagship",
            "silver_aero",
            "cabover_revival",
            "chrome_shop_special",
            "deep_sleeper_custom",
            "wide_glide_tourer",
            "granite_grade_king",
        ],
        blurb: "a premium tractor reserved for senior drivers",
    },
    FleetTier {
        key: "first_pick",
        min_level: 17,
        label: "first pick of the yard",
        pool: &[
            "presidential_sleeper",
            "night_flag_aero",
            "midnight_flyer",
            "owner_spec_showpiece",
            "centurion_longhood",
            "continental_expedition",
        ],
        blurb: "first pick of the yard, the carrier's best equipment",
    },
];

/// The tier a career level makes a driver *eligible* for.
///
/// Eligibility, not entitlement. What the yard actually hands over is
/// [`assigned_fleet_tier`], which is this capped by where the driver stands
/// with the carrier. Kept a pure function of level because the cloud-save
/// validator's exported fleet-tier table is keyed on exactly that.
pub fn fleet_tier_for_level(level: i64) -> &'static FleetTier {
    let mut tier = &FLEET_TIERS[0];
    for candidate in &FLEET_TIERS {
        if level >= candidate.min_level {
            tier = candidate;
        }
    }
    tier
}

/// `FLEET_TIERS.index(tier)`.
fn tier_index(tier: &FleetTier) -> usize {
    FLEET_TIERS
        .iter()
        .position(|t| t.key == tier.key)
        .expect("a fleet tier comes from FLEET_TIERS")
}

/// A carrier gives its best iron to the drivers it wants to keep, and a driver
/// on a final warning does not get the new truck. So the level says what a
/// driver has earned the right to and dispatch trust says what the yard is
/// actually willing to put in their hands; the assignment is the lower of the
/// two. A driver in full trust is capped by nothing and never touches any of
/// this.
pub const STANDING_TIER_CAP: &[(&str, usize)] = &[
    ("full", FLEET_TIERS.len() - 1),
    // long-haul fleet: still real equipment, not the flagships
    ("guarded", 2),
    // regional fleet
    ("poor", 1),
    // the yard's spares
    ("last chance", 0),
];

fn standing_tier_cap(band: &str) -> usize {
    STANDING_TIER_CAP
        .iter()
        .find(|(b, _)| *b == band)
        .map(|(_, cap)| *cap)
        .unwrap_or(FLEET_TIERS.len() - 1)
}

fn career_level<P: CareerProfile + ?Sized>(profile: &P) -> i64 {
    profile.career().level()
}

/// What this driver's level has earned the right to.
pub fn eligible_fleet_tier<P: CareerProfile + ?Sized>(profile: &P) -> &'static FleetTier {
    fleet_tier_for_level(career_level(profile))
}

/// What the yard will actually hand this driver: level capped by standing.
pub fn assigned_fleet_tier<P: CareerProfile + ?Sized>(profile: &P) -> &'static FleetTier {
    let earned = eligible_fleet_tier(profile);
    let cap = standing_tier_cap(standing_band(profile));
    &FLEET_TIERS[tier_index(earned).min(cap)]
}

/// Whether standing is holding this driver below the iron their level earns.
pub fn equipment_held_back<P: CareerProfile + ?Sized>(profile: &P) -> bool {
    if is_owner_operator(StandingProfile::business_status(profile)) {
        return false; // their tractor is their own; no yard decides it
    }
    tier_index(assigned_fleet_tier(profile)) < tier_index(eligible_fleet_tier(profile))
}

/// (why the iron is held, what clears it), both in plain words.
fn hold_cause_phrases<P: CareerProfile + ?Sized>(profile: &P) -> (String, String) {
    let cause = standing_cause(profile);
    if cause == enforcement::CAUSE_DEBT {
        return (
            format!("you owe {}", money_text(debt_owed(profile))),
            "Clear it".to_string(),
        );
    }
    if cause == enforcement::CAUSE_RECORD {
        let record = profile
            .driving_record()
            .expect("a record-caused hold reads a record");
        return (
            format!(
                "the carrier's record review found {}",
                enforcement::record_window_phrase(record, profile.game_hours())
            ),
            format!(
                "Keep the record clean until the hold lifts {}",
                enforcement::record_review_clears_text(profile)
            ),
        );
    }
    if cause == enforcement::CAUSE_LICENCE {
        let clears = clears_text(profile);
        let when = if clears.is_empty() {
            String::new()
        } else {
            format!(" until it clears {clears}")
        };
        return (
            format!("your CDL is suspended{when}"),
            "Get it back".to_string(),
        );
    }
    (
        "your dispatch trust is down".to_string(),
        "Bring it back up with clean on-time runs".to_string(),
    )
}

/// Why the yard handed over a lesser truck than the level earns.
///
/// Names three things every time, because this is the most frequent moment in
/// the whole change and the easiest to read as a bug: the tractor the level
/// would have earned, the single reason in the driver's own numbers, and the
/// exact thing that gives it back.
pub fn equipment_hold_text<P: CareerProfile + ?Sized>(profile: &P, terse: bool) -> String {
    if !equipment_held_back(profile) {
        return String::new();
    }
    let earned = eligible_fleet_tier(profile);
    let (reason, clears) = hold_cause_phrases(profile);
    if terse {
        return format!("Held back from the {}: {reason}.", earned.label);
    }
    format!(
        "Your level earns a tractor from the {}, but {reason}. \
         {clears} and the {} comes back to you.",
        earned.label, earned.label
    )
}

/// The one-sentence version, for a status line that already gave the cause.
pub fn equipment_hold_clause<P: CareerProfile + ?Sized>(profile: &P) -> String {
    if !equipment_held_back(profile) {
        return String::new();
    }
    format!(
        "The yard is also holding your equipment back: your tractor comes \
         from the {}, not the \
         {} your level earns.",
        assigned_fleet_tier(profile).label,
        eligible_fleet_tier(profile).label
    )
}

/// `int.from_bytes(sha256(f"{name}|{carrier}|{tier.key}").digest()[:4], "big")`.
fn driver_seed<P: CareerProfile + ?Sized>(profile: &P, tier: &FleetTier) -> u32 {
    let name = match profile.name() {
        "" => "Driver",
        n => n,
    };
    let carrier = StandingProfile::carrier_key(profile);
    let digest = Sha256::digest(format!("{name}|{carrier}|{}", tier.key).as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

fn stable_index<P: CareerProfile + ?Sized>(profile: &P, tier: &FleetTier) -> usize {
    driver_seed(profile, tier) as usize % tier.pool.len()
}

/// The spare tractors this yard leaves free for this junior driver.
///
/// A rotated slice of the tier pool rather than a random sample, so the same
/// driver always draws the same few trucks and their wear is something the
/// player can actually get to know.
///
/// The slice is then made to cover the work. A yard that dispatches long
/// freight does not leave a driver holding nothing but day cabs -- the
/// rotation alone did exactly that, and the driver went out on a nine hundred
/// mile run with nowhere legal to sleep -- and a yard that dispatches heavy
/// freight keeps something spec'd to pull it.
pub fn slip_seat_pool<P: CareerProfile + ?Sized>(profile: &P) -> Vec<&'static str> {
    let tier = assigned_fleet_tier(profile);
    let pool = tier.pool;
    let size = SLIP_SEAT_POOL_SIZE.min(pool.len());
    let start = driver_seed(profile, tier) as usize % pool.len();
    let rotated: Vec<&'static str> = (0..pool.len())
        .map(|offset| pool[(start + offset) % pool.len()])
        .collect();
    let mut picked: Vec<&'static str> = rotated[..size].to_vec();
    // Sleeper first: it is the one a load can legally require. Each rule gets
    // its own slot, working back from the end -- sharing one slot let the
    // heavy rule overwrite the sleeper the previous rule had just put there,
    // and the driver went back to holding nothing but day cabs.
    let mut reserved: Vec<usize> = Vec::new();
    let traits: [fn(&TruckModel) -> bool; 2] = [
        |model| model.cab == CAB_SLEEPER,
        |model| model.spec == SPEC_HEAVY,
    ];
    for has_trait in traits {
        if picked
            .iter()
            .any(|key| has_trait(truck_model_or_panic(key)))
        {
            continue;
        }
        let replacement = rotated
            .iter()
            .copied()
            .find(|key| has_trait(truck_model_or_panic(key)) && !picked.contains(key));
        let slot = (0..picked.len()).rev().find(|i| !reserved.contains(i));
        let (Some(replacement), Some(slot)) = (replacement, slot) else {
            continue; // this tier has none to cover with, or no slot left
        };
        picked[slot] = replacement;
        reserved.push(slot);
    }
    picked
}

/// Whether this driver takes a truck per load instead of owning a seat.
pub fn slip_seats<P: CareerProfile + ?Sized>(profile: &P) -> bool {
    career_level(profile) < DEDICATED_TRUCK_LEVEL
}

/// What the load asks of a tractor: (sleeper, heavy spec, day-cab work).
pub fn job_equipment_needs<J: JobView + ?Sized>(job: Option<&J>) -> (bool, bool, bool) {
    let Some(job) = job else {
        return (false, false, false);
    };
    let distance = job.distance_mi();
    let weight = job.weight_tons();
    (
        distance > SLEEPER_RUN_MI,
        weight >= HEAVY_LOAD_TONS,
        distance <= DAY_CAB_RUN_MI,
    )
}

/// How well a tractor suits the load; higher is better.
///
/// The first number is the hard one -- a run that needs a bunk simply cannot
/// go out on a day cab -- and the second is preference, so the yard still
/// hands out something sensible when the perfect truck is already gone.
fn fit_score(key: &str, needs: (bool, bool, bool)) -> (i32, i32) {
    let (needs_sleeper, needs_heavy, is_turn) = needs;
    let model = truck_model_or_panic(key);
    let hard = if needs_sleeper && model.cab == CAB_DAY {
        0
    } else {
        1
    };
    let mut score = 0;
    if needs_heavy {
        score += match model.spec {
            s if s == SPEC_HEAVY => 3,
            s if s == SPEC_LIGHT => -1,
            _ => 1,
        };
    } else {
        // Nothing heavy about the load: a light tractor leaves the payload
        // headroom and burns less doing it.
        score += match model.spec {
            s if s == SPEC_LIGHT => 2,
            s if s == SPEC_HEAVY => -1,
            _ => 1,
        };
    }
    if is_turn && model.cab == CAB_DAY {
        score += 2; // a day's work is day-cab work; keep the sleepers for lanes
    }
    if needs_sleeper && model.cab == CAB_SLEEPER {
        score += 2;
    }
    (hard, score)
}

/// The tractor dispatch has this company driver in for this run.
///
/// Without a job -- a menu readout, a save load, anything outside a dispatch
/// -- this is the driver's standing assignment. With one, and while the
/// driver is still slip-seating, it is the best fit the yard has free.
pub fn assigned_truck_key<P, J>(profile: &P, job: Option<&J>) -> &'static str
where
    P: CareerProfile + ?Sized,
    J: JobView + ?Sized,
{
    let tier = assigned_fleet_tier(profile);
    if job.is_none() || !slip_seats(profile) {
        return tier.pool[stable_index(profile, tier)];
    }
    let pool = slip_seat_pool(profile);
    let needs = job_equipment_needs(job);
    // Ties break on pool order, which is stable per driver, so the same load
    // out of the same yard always comes with the same truck. (Python's
    // `max` keeps the FIRST of equal keys.)
    let mut best = pool[0];
    let mut best_score = fit_score(best, needs);
    for key in &pool[1..] {
        let score = fit_score(key, needs);
        if score > best_score {
            best = key;
            best_score = score;
        }
    }
    best
}

/// The truck dispatch put the driver in for this run.
///
/// With a profile, a truck the yard is holding back says so here, in the
/// short form, rather than leaving the driver to wonder why their level
/// stopped buying them anything; what gives it back is on Career stats.
pub fn assignment_reason_text<P>(key: &str, profile: Option<&P>) -> String
where
    P: CareerProfile + ?Sized,
{
    let model = truck_model_or_panic(key);
    let line = format!("Truck for this run: {}.", model.label);
    match profile.filter(|profile| equipment_held_back(*profile)) {
        Some(profile) => format!("{line} {}", equipment_hold_text(profile, true)),
        None => line,
    }
}

/// Spoken description of the current carrier tractor assignment.
pub fn fleet_assignment_text<P: CareerProfile + ?Sized>(profile: &P) -> String {
    // The active key, not a fresh assignment draw: a slip-seating driver may
    // still be holding the tractor dispatch matched to their last load, and
    // the readout has to name the truck whose condition it describes.
    let key = profile.active_truck_key();
    let model = truck_model_or_panic(&key);
    let tier = assigned_fleet_tier(profile);
    let line = format!(
        "Dispatch has you in a {} from the {}: {}",
        model.label, tier.label, model.description
    );
    let hold = equipment_hold_text(profile, false);
    if hold.is_empty() {
        line
    } else {
        format!("{line} {hold}")
    }
}

/// Spoken hand-over line when a promotion changes the assigned tractor.
pub fn fleet_upgrade_announcement<P: CareerProfile + ?Sized>(profile: &P) -> String {
    let key = assigned_truck_key::<P, NoJob>(profile, None);
    let model = truck_model_or_panic(key);
    format!(
        "Dispatch upgraded your assigned tractor. Now running a \
         {}: {}",
        model.label, model.description
    )
}

/// What a level-up says when standing keeps the better tractor in the yard.
///
/// The tractor does not change hands, so nothing about the driver's current
/// truck changes either -- no fresh fuel, no reset wear, no wash. Handing a
/// lesser truck over spotless would tell the player something happened when
/// nothing did.
pub fn withheld_promotion_text<P: CareerProfile + ?Sized>(profile: &P) -> String {
    if !equipment_held_back(profile) {
        return String::new();
    }
    let model = truck_model_or_panic(&profile.active_truck_key());
    format!(
        "You keep the {} you are in, exactly as it stands. {}",
        model.label,
        equipment_hold_text(profile, false)
    )
}

/// What a level-up says instead of promising a tractor the yard is not
/// handing over. The rest of the rank's unlock still happens, so only the
/// equipment half of the promise is corrected.
pub const WITHHELD_UNLOCK_TAIL: &str =
    "The tractor that comes with it is staying in the yard for now.";

/// The tier above what this driver's level has already earned, or `None`.
pub fn next_fleet_tier<P: CareerProfile + ?Sized>(profile: &P) -> Option<&'static FleetTier> {
    let level = career_level(profile);
    FLEET_TIERS.iter().find(|tier| tier.min_level > level)
}

/// What this driver is in, and what stands between them and better iron.
///
/// The hold was spoken at the two moments it happens -- the dispatch
/// hand-over, and the level-up that arrives without a truck. Both are
/// moments, and a moment the player had to be present for is not a record:
/// Brandon, held at the regional fleet on level eleven, went to the career
/// stats screen to ask what was keeping him out of the better equipment and
/// the screen did not mention equipment at all (2026-08-22).
///
/// Two lines rather than one long one, matching how this screen already
/// splits standing across several: what you are driving, and -- only when
/// something is actually holding it back -- why, and what gives it back.
pub fn equipment_status_lines<P: CareerProfile + ?Sized>(profile: &P) -> Vec<String> {
    let key = profile.active_truck_key();
    let truck = match truck_model(&key) {
        Some(model) => model.label,
        None => "none assigned",
    };
    if is_owner_operator(StandingProfile::business_status(profile)) {
        // Nobody assigns an owner-operator a tractor, so there is no tier to
        // be held below and no next one to earn.
        return vec![format!("Truck: {truck}, your own")];
    }
    // "Assigned by the carrier": a livestreamer reading "Level 9 earns the
    // long-haul fleet" asked what the fleets meant and when he could get or
    // buy a truck (owner, 2026-10-11). The carrier hands each fleet over;
    // buying is the owner-operator's.
    let tier = assigned_fleet_tier(profile);
    if equipment_held_back(profile) {
        return vec![
            format!("Truck: {truck}, {}, assigned by the carrier", tier.label),
            equipment_hold_text(profile, false),
        ];
    }
    match next_fleet_tier(profile) {
        None => vec![format!(
            "Truck: {truck}, {}, the carrier's best equipment",
            tier.label
        )],
        Some(upcoming) if slip_seats(profile) && upcoming.min_level >= DEDICATED_TRUCK_LEVEL => {
            vec![format!(
                "Truck: {truck}, {}, assigned by the carrier. At level {}, dispatch moves \
                 you up to the {} and gives you the same truck for every load.",
                tier.label, upcoming.min_level, upcoming.label
            )]
        }
        Some(upcoming) => vec![format!(
            "Truck: {truck}, {}, assigned by the carrier. At level {}, dispatch moves \
             you up to the {}.",
            tier.label, upcoming.min_level, upcoming.label
        )],
    }
}

/// The `job=None` case of [`assigned_truck_key`] / [`assignment_reason_text`]
/// spelled as a type: `assigned_truck_key::<_, NoJob>(profile, None)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoJob {}

impl JobView for NoJob {
    fn distance_mi(&self) -> f64 {
        match *self {}
    }
}
