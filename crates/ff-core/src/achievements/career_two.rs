//! The rules behind the 2.0 career's achievements: Alaska and the Alcan,
//! Canada, the border, the working week, the carriers and live cattle.
//!
//! Each rule is a plain function of what the game already knows at the
//! moment it fires (a delivery, a rest, a hire), so the tests can prove a
//! badge is earnable without a window. The trigger sites in the game crate
//! gather the facts and call these; nothing here speaks or saves.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{add_unique_stat, increment_stat, list_stat, AchievementProfile};
use crate::data::world::World;
use crate::data::world_models::{City, Route};
use crate::models::jobs::JobBoard;

pub const ANCHORAGE: &str = "anchorage_ak_us";
pub const FAIRBANKS: &str = "fairbanks_ak_us";
pub const DEADHORSE: &str = "deadhorse_ak_us";
pub const WHITTIER: &str = "whittier_ak_us";

/// The highway names the road badges read off a leg.
pub const ALASKA_HIGHWAY: &str = "Alaska Highway";
pub const DALTON_HIGHWAY: &str = "Dalton Highway";

/// The stat holding every Alaska Highway leg driven, as city pairs.
pub const ALASKA_HIGHWAY_STAT: &str = "alaska_highway_legs";
/// Cattle loads delivered, for Cattle Call.
pub const CATTLE_LOADS_STAT: &str = "cattle_loads";
/// The carrier the current run of loads is for, when the first of them
/// was delivered (career hours), and how many there have been.
pub const CARRIER_RUN_KEY_STAT: &str = "carrier_run_key";
pub const CARRIER_RUN_START_STAT: &str = "carrier_run_start_hours";
pub const CARRIER_RUN_LOADS_STAT: &str = "carrier_run_loads";

/// Lake Superior's shore on Highway 17, for The Wreck of the Edmund
/// Fitzgerald: its two ends, and the legs driven along it in November.
pub const SUPERIOR_START: &str = "sault_ste_marie_on_ca";
pub const SUPERIOR_END: &str = "thunder_bay_on_ca";
pub const SUPERIOR_NOVEMBER_STAT: &str = "superior_november_legs";
pub const SASKATOON: &str = "saskatoon_sk_ca";
pub const SUDBURY: &str = "sudbury_on_ca";
/// Canada's coasts reached by a delivery ("pacific", "atlantic").
pub const CANADA_COASTS_STAT: &str = "canada_coasts_delivered";
/// Canadian provinces a load has been delivered in.
pub const PROVINCES_STAT: &str = "provinces_delivered";
/// The Atlantic provinces, for Northwest Passage.
pub const ATLANTIC_PROVINCES: [&str; 4] = ["NB", "NS", "PE", "NL"];
/// Canada's territories, which This Land Is Your Land leaves out.
pub const TERRITORIES: [&str; 3] = ["YT", "NT", "NU"];

/// Workin' for a Livin': this many loads for one carrier...
pub const CARRIER_WEEK_LOADS: i64 = 5;
/// ...the last at least a week of career time after the first.
pub const CARRIER_WEEK_HOURS: f64 = 7.0 * 24.0;
/// Cattle Call's count.
pub const CATTLE_CALL_LOADS: i64 = 10;

/// A leg's two cities as one order-free stat value, so a leg driven either
/// way counts once.
pub fn leg_pair(a: &str, b: &str) -> String {
    if a <= b {
        format!("{a}|{b}")
    } else {
        format!("{b}|{a}")
    }
}

/// The Alaska Highway's two ends.
pub const ALASKA_HIGHWAY_START: &str = "dawson_creek_bc_ca";
pub const ALASKA_HIGHWAY_END: &str = "delta_junction_ak_us";

/// Whether the Alaska Highway legs driven (as [`leg_pair`]s) join Dawson
/// Creek to Delta Junction end to end. A road the map carries both as one
/// through leg and as the legs it splits into is driven either way.
pub fn alaska_highway_joined(driven: &BTreeSet<String>) -> bool {
    legs_join(driven, ALASKA_HIGHWAY_START, ALASKA_HIGHWAY_END)
}

/// Whether the legs driven (as [`leg_pair`]s) join `start` to `end`.
pub fn legs_join(driven: &BTreeSet<String>, start: &str, end: &str) -> bool {
    let mut reached: BTreeSet<&str> = BTreeSet::from([start]);
    let mut grew = true;
    while grew {
        grew = false;
        for pair in driven {
            let Some((a, b)) = pair.split_once('|') else {
                continue;
            };
            if reached.contains(a) != reached.contains(b) {
                reached.insert(a);
                reached.insert(b);
                grew = true;
            }
        }
    }
    reached.contains(end)
}

/// Every Alaska Highway leg the map carries, as [`leg_pair`]s.
pub fn alaska_highway_pairs(world: &World) -> BTreeSet<String> {
    world
        .legs
        .iter()
        .filter(|leg| leg.highway == ALASKA_HIGHWAY)
        .map(|leg| leg_pair(&leg.a, &leg.b))
        .collect()
}

/// A leg of Highway 17 along Lake Superior: both ends in Ontario, between
/// Sault Ste. Marie and Thunder Bay. The longitude bounds keep out the
/// stretch east of the Sault toward Sudbury, and the highway name keeps out
/// Highway 11's inland way round.
pub fn superior_shore_leg(world: &World, from: &str, to: &str, highway: &str) -> bool {
    if !highway.split('/').any(|name| name.trim() == "Hwy 17") {
        return false;
    }
    let (Ok(west), Ok(east)) = (world.city(SUPERIOR_END), world.city(SUPERIOR_START)) else {
        return false;
    };
    let on_shore = |key: &str| {
        world.city(key).is_ok_and(|c| {
            c.country == "CA" && c.state_code == "ON" && (west.lon..=east.lon).contains(&c.lon)
        })
    };
    on_shore(from) && on_shore(to)
}

/// Every Superior shore leg the map carries, as [`leg_pair`]s.
pub fn superior_shore_pairs(world: &World) -> BTreeSet<String> {
    world
        .legs
        .iter()
        .filter(|leg| superior_shore_leg(world, &leg.a, &leg.b, &leg.highway))
        .map(|leg| leg_pair(&leg.a, &leg.b))
        .collect()
}

/// The province a Canadian city is in, or `None` for a city in another
/// country.
fn province(city: Option<&City>) -> Option<&str> {
    city.filter(|c| c.country == "CA")
        .map(|c| c.state_code.as_str())
}

/// The provinces (not territories) with a city that takes deliveries: the
/// set This Land Is Your Land asks for, read off the map each time so a new
/// corridor adds to it.
pub fn provinces_with_freight(world: &World) -> BTreeSet<String> {
    world
        .cities
        .values()
        .filter(|c| c.country == "CA" && !TERRITORIES.contains(&c.state_code.as_str()))
        .filter(|c| {
            !c.no_freight
                && c.locations.iter().any(|location| {
                    !JobBoard::cargo_for_location(location, "receives", None).is_empty()
                })
        })
        .map(|c| c.state_code.clone())
        .collect()
}

/// Each leg of `route` with the city it was driven from and to.
fn driven_legs(route: &Route) -> impl Iterator<Item = (&str, &str, &str)> {
    route.legs.iter().enumerate().filter_map(|(i, leg)| {
        let from = route.cities.get(i)?;
        let to = route.cities.get(i + 1)?;
        Some((from.as_str(), to.as_str(), leg.highway.as_str()))
    })
}

/// How the cattle came off at the receiver, read before unloading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CattleDelivery {
    pub hours_aboard: f64,
    pub limit_hours: f64,
    /// An animal was down when the truck reached the receiver.
    pub one_down: bool,
    /// The receiver found the load clean: no bruising, claim or refusal.
    pub clean: bool,
}

/// What a finished drive tells the 2.0 badges.
#[derive(Debug, Clone, Copy)]
pub struct DriveEnd<'a> {
    pub route: &'a Route,
    /// The city the drive ended in (the route's last city).
    pub destination: &'a str,
    /// The city the load was picked up in (the route's first city).
    pub origin: &'a str,
    /// An empty run with no trailer: it can drive a road, not deliver.
    pub bobtail: bool,
    /// The co-driver drove some of it, so the road badges are not the
    /// player's to earn.
    pub team: bool,
    /// The season of the calendar the player hears, at arrival.
    pub season: &'a str,
    /// The month of that calendar at arrival, e.g. "November".
    pub month: &'a str,
    /// The weekday and hour of day on the destination's local clock.
    pub local_weekday: &'a str,
    pub local_hour: f64,
    /// The load was booked under Alaska's spring weight limits.
    pub spring_weight_limit: bool,
    /// The trailer was dropped at a border transfer yard.
    pub border_yard_drop: bool,
    pub cattle: Option<CattleDelivery>,
    /// The carrier the load was hauled for; empty under the driver's own
    /// authority.
    pub carrier_key: &'a str,
    /// Career hours at arrival.
    pub game_hours: f64,
}

/// The 2.0 badges a finished drive earns, updating the tallies they keep.
/// Badges already held come back too; the award skips them.
pub fn drive_end_badges(
    profile: &mut dyn AchievementProfile,
    world: &World,
    end: &DriveEnd,
) -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    let dest_city = world.city(end.destination).ok();
    let in_alaska = dest_city.is_some_and(|c| c.state_code == "AK" && c.country == "US");

    // The road, which an empty run drives as much as a loaded one.
    if !end.team {
        if end.destination == WHITTIER {
            ids.push("whittier_tunnel");
        }
        for (from, to, highway) in driven_legs(end.route) {
            if highway == ALASKA_HIGHWAY {
                add_unique_stat(profile, ALASKA_HIGHWAY_STAT, &leg_pair(from, to));
            }
        }
        let driven: BTreeSet<String> = list_stat(profile, ALASKA_HIGHWAY_STAT)
            .into_iter()
            .collect();
        if alaska_highway_joined(&driven) {
            ids.push("alaska_highway_whole");
        }
        if end.month == "November" {
            for (from, to, highway) in driven_legs(end.route) {
                if superior_shore_leg(world, from, to, highway) {
                    add_unique_stat(profile, SUPERIOR_NOVEMBER_STAT, &leg_pair(from, to));
                }
            }
            let driven: BTreeSet<String> = list_stat(profile, SUPERIOR_NOVEMBER_STAT)
                .into_iter()
                .collect();
            if legs_join(&driven, SUPERIOR_START, SUPERIOR_END) {
                ids.push("superior_november");
            }
        }
    }
    if end.bobtail {
        return ids;
    }

    // Deliveries.
    if in_alaska {
        ids.push("alaska_delivery");
    }
    match end.destination {
        ANCHORAGE => ids.push("anchorage_delivery"),
        DEADHORSE => ids.push("deadhorse_delivery"),
        FAIRBANKS if end.season == "spring" => ids.push("fairbanks_spring"),
        _ => {}
    }
    canada_badges(profile, world, end, &mut ids);
    if end.season == "winter" && dalton_northbound(world, end.route) {
        ids.push("dalton_winter");
    }
    if end.spring_weight_limit {
        ids.push("spring_weight_load");
    }
    if end.border_yard_drop {
        ids.push("laredo_transfer");
    }
    if let Some(cattle) = end.cattle {
        if increment_stat(profile, CATTLE_LOADS_STAT) >= CATTLE_CALL_LOADS {
            ids.push("cattle_ten");
        }
        if cattle.clean && !cattle.one_down && cattle.hours_aboard <= cattle.limit_hours {
            ids.push("cattle_load");
        }
    }
    if carrier_run(profile, end.carrier_key, end.game_hours) {
        ids.push("carrier_first_week");
    }
    ids
}

/// The Canadian deliveries: the country, its provinces and its coasts.
fn canada_badges(
    profile: &mut dyn AchievementProfile,
    world: &World,
    end: &DriveEnd,
    ids: &mut Vec<&'static str>,
) {
    if province(world.city(end.origin).ok()) == Some("NS") {
        ids.push("nova_scotia_out");
    }
    let Some(dest) = province(world.city(end.destination).ok()) else {
        return;
    };
    ids.push("canada_delivery");
    match dest {
        "AB" => ids.push("alberta_delivery"),
        "QC" if end.season == "winter" => ids.push("quebec_winter"),
        _ => {}
    }
    match end.destination {
        SASKATOON => ids.push("saskatoon_delivery"),
        SUDBURY if end.local_weekday == "Saturday" && (18.0..24.0).contains(&end.local_hour) => {
            ids.push("sudbury_saturday")
        }
        _ => {}
    }
    let coast = match dest {
        "BC" => Some("pacific"),
        p if ATLANTIC_PROVINCES.contains(&p) => Some("atlantic"),
        _ => None,
    };
    if let Some(coast) = coast {
        if add_unique_stat(profile, CANADA_COASTS_STAT, coast) >= 2 {
            ids.push("both_coasts_canada");
        }
    }
    if !TERRITORIES.contains(&dest) {
        add_unique_stat(profile, PROVINCES_STAT, dest);
        let delivered: BTreeSet<String> = list_stat(profile, PROVINCES_STAT).into_iter().collect();
        if provinces_with_freight(world).is_subset(&delivered) {
            ids.push("every_province");
        }
    }
}

/// The route drives a Dalton Highway leg north.
fn dalton_northbound(world: &World, route: &Route) -> bool {
    driven_legs(route).any(|(from, to, highway)| {
        if !highway.contains(DALTON_HIGHWAY) {
            return false;
        }
        match (world.city(from), world.city(to)) {
            (Ok(from), Ok(to)) => to.lat > from.lat,
            _ => false,
        }
    })
}

/// Count a delivery toward Workin' for a Livin': a run of loads for one
/// carrier starts over when the carrier changes. True once the run holds
/// [`CARRIER_WEEK_LOADS`] loads and spans [`CARRIER_WEEK_HOURS`].
pub fn carrier_run(
    profile: &mut dyn AchievementProfile,
    carrier_key: &str,
    game_hours: f64,
) -> bool {
    if carrier_key.is_empty() {
        return false;
    }
    let stats = profile.achievement_stats_mut();
    let same = stats.get(CARRIER_RUN_KEY_STAT).and_then(Value::as_str) == Some(carrier_key);
    let start = stats.get(CARRIER_RUN_START_STAT).and_then(Value::as_f64);
    let (start, loads) = match (same, start) {
        (true, Some(start)) => {
            let loads = stats
                .get(CARRIER_RUN_LOADS_STAT)
                .and_then(Value::as_i64)
                .unwrap_or(0)
                + 1;
            (start, loads)
        }
        _ => (game_hours, 1),
    };
    stats.insert(CARRIER_RUN_KEY_STAT.to_string(), Value::from(carrier_key));
    stats.insert(CARRIER_RUN_START_STAT.to_string(), Value::from(start));
    stats.insert(CARRIER_RUN_LOADS_STAT.to_string(), Value::from(loads));
    loads >= CARRIER_WEEK_LOADS && game_hours - start >= CARRIER_WEEK_HOURS
}

/// Get Back: a driver a carrier let go who is on another carrier's payroll
/// now. Read from the record, so a career that was let go and taken on
/// before this badge existed earns it at its next terminal visit.
pub fn rehired_after_let_go(carrier_terminations: i64, carrier_key: &str, let_go_by: &str) -> bool {
    carrier_terminations > 0 && !carrier_key.is_empty() && carrier_key != let_go_by
}

/// The restart badges a finished rest earns: `restart_complete` is the
/// hours-of-service clock's word that the cycle has restarted, and
/// `at_home_terminal` that the driver rested at their own carrier's
/// terminal.
pub fn restart_badges(restart_complete: bool, at_home_terminal: bool) -> Vec<&'static str> {
    match (restart_complete, at_home_terminal) {
        (false, _) => Vec::new(),
        (true, false) => vec!["restart_34"],
        (true, true) => vec!["restart_34", "restart_at_home"],
    }
}

#[cfg(test)]
#[path = "career_two_tests.rs"]
mod tests;
