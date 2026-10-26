//! Team driving: the co-driver cast, the team's terms, and who dispatch
//! pairs a driver with (owner design, 2026-10-17).
//!
//! A long load can run as a team. While the co-driver drives, the player is
//! in the sleeper: their hours recover, the co-driver keeps a logbook of
//! their own, and the miles the co-driver drove pay and teach the player
//! less than miles they drove themselves. Carriers team long freight, not
//! short runs, so a team is offered only when the rest of the trip would
//! need a 10-hour break.
//!
//! Everyone in the cast came through the same training class at Great
//! Lakes Training Transport; dispatch pairs a company driver with whichever
//! classmate works at their carrier, and an owner-operator keeps the one
//! they teamed with most. The pairing history lives in the profile's
//! `achievement_stats` counters (the same free dict the spoken-advice
//! counters use), so a save gains no new field.
//!
//! Pure functions only: the drive and the settlement read the numbers here.

use serde_json::{Map, Value};

use crate::models::business_constants::is_owner_operator;
use crate::pyrandom::PyRandom;
use crate::sim::hos::{self, HosClock};

#[cfg(test)]
mod tests;

/// A company team splits its per-mile rate: miles the co-driver drove pay
/// the player half.
pub const COMPANY_CO_DRIVER_PAY_SHARE: f64 = 0.5;
/// An owner-operator pays a hired co-driver this share of the settlement for
/// the miles they drove, and keeps fuel, tolls and upkeep.
pub const OWNER_CO_DRIVER_PAY_SHARE: f64 = 0.3;
/// Career experience for co-driver miles, as a share of the full rate
/// (owner: less than driving it yourself, but not none).
pub const CO_DRIVER_XP_RATE: f64 = 0.5;
/// Hand back this long before the co-driver's hours run out, so the summon
/// lands with time to wake up and take over.
pub const CO_DRIVER_SUMMON_LEAD_MIN: f64 = 15.0;
/// After this much driving since their last break, the co-driver plans a
/// stop for the 30-minute break that falls due at eight hours: early
/// enough that a full lot still leaves time to reach the next stop.
pub const CO_DRIVER_BREAK_PLAN_MIN: f64 = 5.5 * 60.0;
/// The driving after which the 30-minute break is required.
pub const BREAK_REQUIRED_AFTER_MIN: f64 = 8.0 * 60.0;
/// A team load pays the carrier this much more than the same load run
/// solo: the truck never stops for a 10-hour break, so it delivers sooner.
pub const TEAM_LOAD_PAY_MULT: f64 = 1.35;
/// Of the board's loads long enough to need a 10-hour break, about this
/// share are booked as team loads.
pub const TEAM_LOAD_SHARE: f64 = 0.3;
/// Hours a team load's deadline allows at each change of driver.
pub const TEAM_CHANGE_H: f64 = 0.5;
/// One team load in this many draws the classmate who went to Meridian.
pub const MERIDIAN_DRAW_ODDS: u32 = 25;
/// Career level from which the old trainer can turn up for one load.
pub const TRAINER_LEVEL: i64 = 20;

const STAT_LOADS: &str = "team:loads:";
const STAT_HIRE: &str = "team:hire:";
const STAT_SEEN: &str = "team:seen:";
const STAT_INTRO: &str = "team:intro";

/// How a co-driver uses the radio when the player leaves it on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioHabits {
    /// Catalog station ids they reach for first, best first.
    pub leans: &'static [&'static str],
    /// Mean game minutes between turns of the dial.
    pub flip_every_min: f64,
    /// Chance, at each turn, that they switch the radio off for a while.
    pub quiet_chance: f64,
    /// Game minutes of quiet once they switch it off.
    pub quiet_min: f64,
}

/// Who can be drawn for a load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pool {
    /// The training class, drawn for any team load.
    Class,
    /// Turns up once, at a moment the career already marks.
    Surprise,
}

/// One member of the cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoDriver {
    pub key: &'static str,
    pub name: &'static str,
    /// One or two sentences, read on the Driver screen.
    pub bio: &'static str,
    pub habits: RadioHabits,
    pub pool: Pool,
}

pub const CAST: [CoDriver; 9] = [
    CoDriver {
        key: "dan",
        name: "Dan",
        bio: "The most careful driver in your training class. The others call him \
              Dangerous Dan after the song he kept asking the Roadhouse to play.",
        habits: RadioHabits {
            leans: &["route_playlist"],
            flip_every_min: 240.0,
            quiet_chance: 0.05,
            quiet_min: 30.0,
        },
        pool: Pool::Class,
    },
    CoDriver {
        key: "rusty",
        name: "Rusty Oxley",
        bio: "Grandson of Grandpa Spuds Oxley on The Delta. He learned the road \
              listening to his grandfather on the air.",
        habits: RadioHabits {
            leans: &["wdlt-memphis"],
            flip_every_min: 150.0,
            quiet_chance: 0.1,
            quiet_min: 30.0,
        },
        pool: Pool::Class,
    },
    CoDriver {
        key: "june",
        name: "June",
        bio: "Grew up waiting tables at Della's Blue Plate Diner and decided she \
              wanted to be one of the drivers. She knows every diner on the map.",
        habits: RadioHabits {
            leans: &["whwy-nashville", "krwl-dallas", "kbsk-billings"],
            flip_every_min: 120.0,
            quiet_chance: 0.05,
            quiet_min: 20.0,
        },
        pool: Pool::Class,
    },
    CoDriver {
        key: "marisol",
        name: "Marisol",
        bio: "Drove water tenders on a wildfire crew before trucking. Calm on the \
              mountain grades.",
        habits: RadioHabits {
            leans: &["krdg-denver", "wgrx-chicago", "kchm-los-angeles"],
            flip_every_min: 90.0,
            quiet_chance: 0.25,
            quiet_min: 45.0,
        },
        pool: Pool::Class,
    },
    CoDriver {
        key: "walt",
        name: "Walt",
        bio: "Will not leave a terminal until four forecasts agree. Drives long \
              stretches in silence.",
        habits: RadioHabits {
            leans: &["kpln-kansas-city"],
            flip_every_min: 200.0,
            quiet_chance: 0.4,
            quiet_min: 90.0,
        },
        pool: Pool::Class,
    },
    CoDriver {
        key: "dee",
        name: "Dee",
        bio: "Spent one summer of the nineties online and never came back. Runs \
              every driver app there is.",
        habits: RadioHabits {
            leans: &["kchm-los-angeles"],
            flip_every_min: 25.0,
            quiet_chance: 0.03,
            quiet_min: 15.0,
        },
        pool: Pool::Class,
    },
    CoDriver {
        key: "nate",
        name: "Nate",
        bio: "A bicycle courier before trucks. He will tell you The Lone Courier \
              on Channel 3000 is about him. It is not.",
        habits: RadioHabits {
            leans: &["channel-3000"],
            flip_every_min: 60.0,
            quiet_chance: 0.08,
            quiet_min: 20.0,
        },
        pool: Pool::Class,
    },
    CoDriver {
        key: "lou",
        name: "Lou",
        bio: "Left your training class for Meridian Freight Lines and was never \
              heard from again, until now.",
        habits: RadioHabits {
            leans: &["ff-night-line"],
            flip_every_min: 100.0,
            quiet_chance: 0.1,
            quiet_min: 30.0,
        },
        pool: Pool::Surprise,
    },
    CoDriver {
        key: "ray",
        name: "Ray",
        bio: "Ran your training class at Great Lakes. Rides along for one load to \
              see how you turned out.",
        habits: RadioHabits {
            leans: &["ff-night-line", "route_playlist"],
            flip_every_min: 180.0,
            quiet_chance: 0.15,
            quiet_min: 60.0,
        },
        pool: Pool::Surprise,
    },
];

/// The cast member with this key.
pub fn co_driver(key: &str) -> Option<&'static CoDriver> {
    CAST.iter().find(|c| c.key == key)
}

/// The training class: the drivers dispatch draws for an ordinary load.
pub fn class_members() -> impl Iterator<Item = &'static CoDriver> {
    CAST.iter().filter(|c| c.pool == Pool::Class)
}

/// Why this pairing came about, for the line that names the co-driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingReason {
    /// The first team load of a career: always Dan.
    FirstTeam,
    /// A classmate at the player's carrier.
    Classmate,
    /// The owner-operator's own hire.
    Hire,
    /// The classmate who went to Meridian.
    Meridian,
    /// The old trainer.
    Trainer,
}

/// A pairing: who and why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pairing {
    pub co_driver: &'static CoDriver,
    pub reason: PairingReason,
}

fn stat_i64(stats: &Map<String, Value>, key: &str) -> i64 {
    stats.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Team loads run with this co-driver.
pub fn loads_with(stats: &Map<String, Value>, key: &str) -> i64 {
    stat_i64(stats, &format!("{STAT_LOADS}{key}"))
}

/// Team loads run with anyone.
pub fn team_loads(stats: &Map<String, Value>) -> i64 {
    CAST.iter().map(|c| loads_with(stats, c.key)).sum()
}

/// Everyone the career has run a team load with, in cast order, with the
/// loads run together.
pub fn teamed_with(stats: &Map<String, Value>) -> Vec<(&'static CoDriver, i64)> {
    CAST.iter()
        .map(|member| (member, loads_with(stats, member.key)))
        .filter(|(_, loads)| *loads > 0)
        .collect()
}

/// The owner-operator's hired co-driver, once there is one.
pub fn hired_co_driver(stats: &Map<String, Value>) -> Option<&'static CoDriver> {
    CAST.iter()
        .find(|c| stat_i64(stats, &format!("{STAT_HIRE}{}", c.key)) > 0)
}

fn seen(stats: &Map<String, Value>, key: &str) -> bool {
    stat_i64(stats, &format!("{STAT_SEEN}{key}")) > 0
}

/// The classmate the player has teamed with most; Dan on a tie with him,
/// otherwise the first in cast order.
pub fn most_teamed(stats: &Map<String, Value>) -> &'static CoDriver {
    let mut best = &CAST[0];
    let mut best_loads = loads_with(stats, best.key);
    for member in class_members() {
        let loads = loads_with(stats, member.key);
        if loads > best_loads {
            best = member;
            best_loads = loads;
        }
    }
    best
}

/// The seed a load's team draws come from: the same load gives the same
/// answer at the dispatch board, at the shipper and on the road.
pub fn load_seed(origin: &str, destination: &str, cargo_key: &str, weight_tons: f64) -> String {
    format!("{origin}>{destination}:{cargo_key}:{weight_tons:.1}")
}

/// Whether dispatch books this load as a team load. Only loads that would
/// need a 10-hour break solo are candidates; `seed` is [`load_seed`].
pub fn team_load_drawn(seed: &str) -> bool {
    PyRandom::new_from_str(&format!("team-load:{seed}")).random() < TEAM_LOAD_SHARE
}

/// Who dispatch pairs the player with for this load. `seed` is the load's
/// [`load_seed`], so the shipper, the road and a resumed trip all name the
/// same driver.
pub fn pair(
    stats: &Map<String, Value>,
    business_status: &str,
    career_level: i64,
    seed: &str,
) -> Pairing {
    if is_owner_operator(business_status) {
        let hire = hired_co_driver(stats).unwrap_or_else(|| most_teamed(stats));
        return Pairing {
            co_driver: hire,
            reason: PairingReason::Hire,
        };
    }
    if team_loads(stats) == 0 {
        return Pairing {
            co_driver: &CAST[0],
            reason: PairingReason::FirstTeam,
        };
    }
    let mut rng = PyRandom::new_from_str(&format!("co-driver:{seed}"));
    if career_level >= TRAINER_LEVEL && !seen(stats, "ray") {
        if let Some(ray) = co_driver("ray") {
            return Pairing {
                co_driver: ray,
                reason: PairingReason::Trainer,
            };
        }
    }
    let draw = (rng.random() * f64::from(MERIDIAN_DRAW_ODDS)) as u32;
    if draw == 0 && !seen(stats, "lou") {
        if let Some(lou) = co_driver("lou") {
            return Pairing {
                co_driver: lou,
                reason: PairingReason::Meridian,
            };
        }
    }
    let class: Vec<&CoDriver> = class_members().collect();
    let pick = ((rng.random() * class.len() as f64) as usize).min(class.len() - 1);
    Pairing {
        co_driver: class[pick],
        reason: PairingReason::Classmate,
    }
}

/// Record a team load in the profile's counters. An owner-operator's first
/// team load makes that co-driver their hire.
pub fn record_team_load(stats: &mut Map<String, Value>, key: &str, business_status: &str) {
    let loads = format!("{STAT_LOADS}{key}");
    let next = stat_i64(stats, &loads) + 1;
    stats.insert(loads, Value::from(next));
    stats.insert(format!("{STAT_SEEN}{key}"), Value::from(1));
    if is_owner_operator(business_status) && hired_co_driver(stats).is_none() {
        stats.insert(format!("{STAT_HIRE}{key}"), Value::from(1));
    }
}

/// The line that introduces the co-driver when the wheel is handed over.
pub fn pairing_line(pairing: &Pairing, carrier_name: &str) -> String {
    let name = pairing.co_driver.name;
    match pairing.reason {
        PairingReason::FirstTeam | PairingReason::Classmate => {
            if carrier_name.is_empty() {
                format!("{name}, from your training class.")
            } else {
                format!("{name}, from your training class, drives for {carrier_name} now.")
            }
        }
        PairingReason::Hire => format!("{name}, your co-driver."),
        PairingReason::Meridian => {
            format!("{name}, back from Meridian with a story.")
        }
        PairingReason::Trainer => format!("{name}, your old trainer, along for one load."),
    }
}

/// Whether the rest of this trip is long enough to team: driving it would
/// need a 10-hour break first. `drive_min_left` is the planner's driving
/// minutes to the destination; `hos_mode` the player's hours setting. The
/// 30-minute break the drive would need comes out of the duty window.
pub fn team_worthwhile(hos: &HosClock, hos_mode: &str, drive_min_left: f64) -> bool {
    let Some((drive_limit, window, break_after)) = hos::limits(hos_mode) else {
        // Hours off: nothing forces a sleep, so judge the trip against one
        // full shift instead.
        return drive_min_left > hos::REALISTIC_LIMITS.0;
    };
    let drive_left = (drive_limit - hos.driving_min).max(0.0);
    let window_left = (window - hos.duty_min).max(0.0);
    let break_min = if hos.since_break_min + drive_min_left > break_after {
        hos::BREAK_MIN
    } else {
        0.0
    };
    drive_min_left > drive_left || drive_min_left + break_min > window_left
}

/// The split of one settlement between the player and the co-driver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamSplit {
    /// The share of the trip the co-driver drove, 0 to 1.
    pub co_driver_fraction: f64,
    /// Taken off the player's settlement for the co-driver.
    pub co_driver_pay: f64,
    /// The multiplier on the delivery's career experience.
    pub xp_rate: f64,
}

/// What the co-driver's miles cost the player at settlement.
///
/// A company team halves the player's pay for the co-driver's miles. An
/// owner-operator pays their hire `OWNER_CO_DRIVER_PAY_SHARE` of the
/// settlement for those miles. Experience for those miles comes at
/// `CO_DRIVER_XP_RATE`.
pub fn team_split(
    business_status: &str,
    net_pay: f64,
    total_mi: f64,
    co_driver_mi: f64,
) -> TeamSplit {
    let fraction = if total_mi > 0.0 {
        (co_driver_mi / total_mi).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let share = if is_owner_operator(business_status) {
        OWNER_CO_DRIVER_PAY_SHARE
    } else {
        COMPANY_CO_DRIVER_PAY_SHARE
    };
    let co_driver_pay = crate::pyfmt::round_py_n(net_pay.max(0.0) * fraction * share, 2);
    TeamSplit {
        co_driver_fraction: fraction,
        co_driver_pay,
        xp_rate: 1.0 - fraction * (1.0 - CO_DRIVER_XP_RATE),
    }
}

/// Whether hours of service are kept in this mode, so rest and shift
/// limits mean anything.
pub fn enforced(hos_mode: &str) -> bool {
    !hos::is_non_enforced(hos_mode) && hos::limits(hos_mode).is_some()
}

/// Whether this logbook has had its full 10-hour rest since its driver
/// last drove. Time on duty without driving (checking in, loading, a
/// fuel stop) since the last reset does not use the rest up: a driver
/// fresh from the sleeper who loads and hands over owes no second sleep.
pub fn fully_rested(hos: &HosClock) -> bool {
    hos.off_duty_min >= hos::SLEEP_MIN || hos.driving_min <= 0.0
}

/// Minutes of rest this logbook still needs for its full 10 hours.
pub fn rest_left_min(hos: &HosClock) -> f64 {
    if fully_rested(hos) {
        0.0
    } else {
        (hos::SLEEP_MIN - hos.off_duty_min).max(0.0)
    }
}

/// Minutes of unbroken rest this logbook has, up to the full 10 hours.
pub fn rested_min(hos: &HosClock) -> f64 {
    hos::SLEEP_MIN - rest_left_min(hos)
}

/// Whether the co-driver should hand back because the player in the
/// sleeper has had their full rest (owner question, 2026-10-19). `owed` is
/// whether the player still needed rest when they handed over: a player who
/// was already rested gets the co-driver's whole shift.
///
/// The player's 10 hours come in before a fresh co-driver's 11 hours of
/// driving run out, so the co-driver hands back with an hour or less of
/// their own unused. Waking the player as soon as they are rested keeps the
/// share of the load that pays them less no larger than the rest needed,
/// and makes sleeping through it last as long as a sleep.
pub fn rest_complete(player: &HosClock, hos_mode: &str, owed: bool) -> bool {
    owed && enforced(hos_mode) && fully_rested(player)
}

/// Minutes of driving time or duty window this logbook has left, whichever
/// ends first. The 30-minute break is left out: the co-driver stops for it
/// and drives on, so it does not end their shift. None with hours off.
pub fn shift_left_min(hos: &HosClock, hos_mode: &str) -> Option<f64> {
    if !enforced(hos_mode) {
        return None;
    }
    let (drive_limit, window, _) = hos::limits(hos_mode)?;
    Some(
        (drive_limit - hos.driving_min)
            .min(window - hos.duty_min)
            .max(0.0),
    )
}

/// Whether the career has heard how team driving works.
pub fn introduced(stats: &Map<String, Value>) -> bool {
    stat_i64(stats, STAT_INTRO) > 0 || team_loads(stats) > 0
}

/// Record that the career has heard the introduction.
pub fn mark_introduced(stats: &mut Map<String, Value>) {
    stats.insert(STAT_INTRO.to_string(), Value::from(1));
}

/// Whether the co-driver should hand the wheel back now: with hours of
/// service enforced, as their own driving time or duty window runs out.
/// [`rest_complete`] usually ends the shift first.
pub fn shift_over(hos: &HosClock, hos_mode: &str) -> bool {
    enforced(hos_mode)
        && hos
            .remaining_min(hos_mode)
            .is_some_and(|left| left <= CO_DRIVER_SUMMON_LEAD_MIN)
}

/// Whether the co-driver should plan a stop for their 30-minute break.
pub fn break_wanted(hos: &HosClock, hos_mode: &str) -> bool {
    enforced(hos_mode) && hos.since_break_min >= CO_DRIVER_BREAK_PLAN_MIN
}

/// Whether the break is so close to due that the co-driver hands back
/// rather than drive past it.
pub fn break_overdue(hos: &HosClock, hos_mode: &str) -> bool {
    enforced(hos_mode)
        && hos.since_break_min >= BREAK_REQUIRED_AFTER_MIN - CO_DRIVER_SUMMON_LEAD_MIN
}

/// The hidden badge a surprise pairing earns, if any.
pub fn pairing_badge(co_driver_key: &str) -> Option<&'static str> {
    match co_driver_key {
        "lou" => Some("meridian_classmate"),
        "ray" => Some("trainer_ride_along"),
        _ => None,
    }
}

/// How far to step the player's fatigue down for `minutes` in the sleeper:
/// a full 10 hours clears it.
pub fn sleeper_fatigue_relief(minutes: f64) -> f64 {
    100.0 * minutes / hos::SLEEP_MIN
}
