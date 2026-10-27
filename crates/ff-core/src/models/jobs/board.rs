//! `JobBoard`: seeded job generation at a city (the board half of `jobs.py`).

use std::collections::HashMap;

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::data::lcv_turnpikes::cargo_requires_lcv_turnpike;
use crate::data::seasonal_weight::{seasonal_gvw_cap_kg, strictest_on_route};
use crate::data::world::{max_alternate_miles, World};
use crate::data::world_models::{City, CrossingLoad, Location};
use crate::models::business_constants::DIRECT_FREIGHT_PAY_MULT;
use crate::models::carriers::carrier;
use crate::models::jobs::{
    cargo_type, dispatch_deadline_hours, market_tag_cargo_bonus, minimum_pay_for_level, plan_hos,
    CargoType, Job, SeasonalWeightLimit, DEADLINE_DISPATCH_SLACK_RANGE, FACILITY_SELECTION_WEIGHTS,
    HELD_CREDENTIAL_CARGO_WEIGHT, HELD_CREDENTIAL_FACILITY_BONUS, HOOKUP_FEE, LEVEL_DISTANCE_CAPS,
    LEVEL_DISTANCE_CAP_STEP_MI, LONG_HAUL_MILES, MAX_DISPATCH_DISTANCE_MI, MIN_JOB_DISTANCE_MI,
    PREMIUM_LANE_LEVEL, PREMIUM_LANE_LONG_HAUL_BIAS, SPECIALIZED_FREIGHT_LEVEL,
    SPECIALIZED_FREIGHT_WEIGHT, THIN_OUTBOUND_ORIGIN_WEIGHT,
};
use crate::models::market::Market;
use crate::models::start_options::{start_option, DEFAULT_START_KEY};
use crate::pyfmt::round_py_n;
use crate::pyrandom::PyRandom;
use crate::sim::hos::HosClock;
use crate::sim::vehicle::{
    combination_tare_kg, TrailerSet, TruckSpecs, TruckState, KG_PER_TON, TRAILER_TARE_KG,
};

/// `(destination, route miles, route leg count)`.
pub type Candidate = (String, f64, usize);

// Reachable-destination candidates depend only on the (static) world, not the
// board seed. Cache them once across all JobBoard instances, keyed by world:
// the city hub and the tests spin up many fresh boards, and recomputing a
// supported route to every city each time scaled terribly as the network grew
// to 160+.
type CandidateCache = HashMap<usize, HashMap<String, Vec<Candidate>>>;
static CANDIDATES_CACHE: Lazy<Mutex<CandidateCache>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Every other city reachable from `city`, from the same cache the boards
/// fill: one supported route per city on the map, so the first ask in a city
/// is the expensive part of opening its board.
pub fn reachable_cities(world: &World, city: &str) -> Vec<Candidate> {
    JobBoard::new(world, Some(0), None).candidates(&world.resolve_city_key(city))
}

/// The keyword arguments of `JobBoard.offers` / `offer_to`, each with its
/// Python default.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfferOptions<'a> {
    /// `count=5` (`offers` only); 0 means the default.
    pub count: usize,
    /// `level=1`; 0 means the default.
    pub level: i64,
    pub market: Option<&'a Market>,
    pub carrier_key: Option<&'a str>,
    pub direct_freight: bool,
}

impl<'a> OfferOptions<'a> {
    /// `offers(..., level=level)`.
    pub fn level(level: i64) -> Self {
        OfferOptions {
            level,
            ..Self::default()
        }
    }

    fn count_or_default(&self) -> usize {
        if self.count == 0 {
            5
        } else {
            self.count
        }
    }

    fn level_or_default(&self) -> i64 {
        if self.level == 0 {
            1
        } else {
            self.level
        }
    }

    fn carrier(&self) -> &'a str {
        match self.carrier_key {
            Some(key) if !key.is_empty() => key,
            _ => DEFAULT_START_KEY,
        }
    }
}

/// Generates job offers at a city, filtered by the player's endorsements.
///
/// Destinations follow a career arc: low levels offer mostly single-leg hops
/// to neighboring cities, the distance cap grows with level, and every level
/// weights destination choice by proximity so freight follows plausible lanes
/// instead of teleporting across the country. New dispatches only use
/// metadata-supported corridors; the broad legacy graph remains available for
/// old saves while enrichment coverage expands.
pub struct JobBoard<'w> {
    pub world: &'w World,
    // The driver's live shift clock: deadlines plan around the hours already
    // burned, the way a real dispatcher asks what you have left.
    pub hos: Option<HosClock>,
    calendar_hours: Option<f64>,
    rng: PyRandom,
}

impl<'w> JobBoard<'w> {
    /// `JobBoard(world, seed=None, hos=None)`; `None` seeds from the OS.
    pub fn new(world: &'w World, seed: Option<i64>, hos: Option<&HosClock>) -> Self {
        JobBoard {
            world,
            hos: hos.cloned(),
            calendar_hours: None,
            rng: match seed {
                Some(seed) => PyRandom::new_from_i64(seed),
                None => PyRandom::new_unseeded(),
            },
        }
    }

    /// `JobBoard(world, seed=seed)`.
    pub fn seeded(world: &'w World, seed: i64) -> Self {
        Self::new(world, Some(seed), None)
    }

    pub fn with_calendar_hours(mut self, hours: f64) -> Self {
        self.calendar_hours = Some(hours);
        self
    }

    /// How many distinct places a driver at `level` can be sent to from
    /// `city`: the reachable cities within the level's range and past the
    /// across-town minimum. The board's own measure of how much choice a
    /// town's freight offers; a town with one or two is a thin market
    /// however many rows the board fills (see `relay`).
    pub fn destination_choices(&self, city: &str, level: i64) -> usize {
        self.destination_choices_for_carrier(city, level, DEFAULT_START_KEY)
    }

    pub fn destination_choices_for_carrier(
        &self,
        city: &str,
        level: i64,
        carrier_key: &str,
    ) -> usize {
        let city = self.world.resolve_city_key(city);
        let cap = Self::distance_cap_for_carrier(level, carrier_key);
        self.candidates(&city)
            .iter()
            .filter(|c| c.1 >= MIN_JOB_DISTANCE_MI && c.1 <= cap)
            .count()
    }

    pub fn distance_cap(level: i64) -> f64 {
        Self::level_distance_cap_only(level)
    }

    /// Level distance cap combined with the carrier run-band max (tighter wins).
    pub fn distance_cap_for_carrier(level: i64, carrier_key: &str) -> f64 {
        let level_cap = Self::level_distance_cap_only(level);
        match carrier(carrier_key) {
            Some(c) => c.effective_distance_cap(level_cap),
            None => level_cap,
        }
    }

    fn level_distance_cap_only(level: i64) -> f64 {
        if let Some((_, cap)) = LEVEL_DISTANCE_CAPS.iter().find(|(l, _)| *l == level) {
            return *cap;
        }
        let base = LEVEL_DISTANCE_CAPS
            .iter()
            .find(|(l, _)| *l == 5)
            .map(|(_, cap)| *cap)
            .unwrap_or(1200.0);
        MAX_DISPATCH_DISTANCE_MI.min(base + LEVEL_DISTANCE_CAP_STEP_MI * (level - 5) as f64)
    }

    /// `board.offers(city, endorsements, count=, level=, market=,
    /// carrier_key=, direct_freight=)`.
    pub fn offers<S: AsRef<str>>(
        &mut self,
        city: &str,
        endorsements: &[S],
        opts: OfferOptions<'_>,
    ) -> Vec<Job> {
        let count = opts.count_or_default();
        let level = opts.level_or_default();
        let carrier_key = opts.carrier();
        let mut jobs: Vec<Job> = Vec::new();
        let city = self.world.resolve_city_key(city);
        let Ok(city_obj) = self.world.city(&city) else {
            return jobs;
        };
        let candidates: Vec<Candidate> = self
            .candidates(&city)
            .into_iter()
            .filter(|c| c.1 >= MIN_JOB_DISTANCE_MI)
            .filter(|c| self.city_has_freight_receiver(&c.0))
            .collect();
        let cap = Self::distance_cap_for_carrier(level, carrier_key);
        // Run-band *max* is already folded into `cap` (tighter of level cap and
        // carrier max). The run-band *min* is stored on the carrier for slice 4
        // lane-area enforcement; applying it here would wipe short national
        // boards (Northstar 400–3000) that rookies still need.
        let mut reachable: Vec<Candidate> =
            candidates.iter().filter(|c| c.1 <= cap).cloned().collect();
        if reachable.is_empty() && !candidates.is_empty() {
            // remote terminals (long legs all around): offer the nearest few
            let mut sorted = candidates.clone();
            sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            sorted.truncate(4);
            reachable = sorted;
        }
        if reachable.is_empty() {
            return jobs;
        }
        // Pick a spread of DISTINCT destinations up front so the board never
        // collapses to one back-and-forth city (a start with a single nearby
        // neighbour used to be locked into one route). Nearer cities stay likelier.
        let dest_cycle = self.spread_destinations(&city, &reachable, level, count, carrier_key);
        let held: Vec<&str> = endorsements.iter().map(AsRef::as_ref).collect();
        let mut attempts = 0;
        while jobs.len() < count && attempts < count * 30 {
            attempts += 1;
            let Some(location) = self.choose_origin_location(city_obj, level, carrier_key, &held)
            else {
                // City has no freight-capable shipper (e.g. travel-center-only
                // ALCAN pins). Empty board is honest.
                break;
            };
            let Some(cargo_key) =
                self.choose_cargo_for_location(city_obj, location, level, carrier_key, &held)
            else {
                continue;
            };
            let cargo = cargo_type(cargo_key).expect("a catalog cargo key");
            let locked = !cargo.missing_credentials(endorsements).is_empty();
            // a locked job may appear once in a while as a teaser, otherwise skip
            if locked && !(jobs.len() == count - 1 && self.rng.random() < 0.3) {
                continue;
            }
            let (destination, miles, _legs) = dest_cycle[jobs.len() % dest_cycle.len()].clone();
            // LCV freight only runs where the frozen network reaches: both
            // ends of the lane must sit in states that allow the combination.
            if cargo.lcv_lanes && !self.lcv_lane(&city, &destination) {
                continue;
            }
            // STAA twin trailers stay on the National Network approximation
            // (plus reasonable access). Skip destinations with no twin-legal
            // corridor option.
            if cargo.national_network && !self.national_network_lane(&city, &destination) {
                continue;
            }
            let Some(dest_location) = self.destination_location(&destination, cargo, level) else {
                continue;
            };
            let dest_location = dest_location.clone();
            let origin_name = location.name.clone();
            let location = location.clone();
            if let Some(job) = self.make_job(
                cargo,
                &city,
                &origin_name,
                &destination,
                miles,
                opts.market,
                level,
                &location,
                &dest_location,
                carrier_key,
                opts.direct_freight,
            ) {
                jobs.push(job);
            }
        }
        jobs.sort_by(|a, b| {
            a.distance_mi
                .partial_cmp(&b.distance_mi)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        jobs
    }

    /// One offer to a specific destination, for the playtest lever.
    ///
    /// Ignores the level distance cap on purpose: a tester forcing a
    /// destination wants that run regardless of career progress. Returns
    /// None when no supported corridor reaches the destination or no
    /// unlocked cargo pairing exists.
    pub fn offer_to<S: AsRef<str>>(
        &mut self,
        city: &str,
        destination: &str,
        endorsements: &[S],
        opts: OfferOptions<'_>,
    ) -> Option<Job> {
        let level = opts.level_or_default();
        let carrier_key = opts.carrier();
        let city = self.world.resolve_city_key(city);
        let destination = self.world.resolve_city_key(destination);
        let city_obj = self.world.city(&city).ok()?;
        let matches: Vec<Candidate> = self
            .candidates(&city)
            .into_iter()
            .filter(|c| c.0 == destination)
            .collect();
        if matches.is_empty() {
            return None;
        }
        let (destination, miles, _legs) = matches
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?
            .clone();
        let held: Vec<&str> = endorsements.iter().map(AsRef::as_ref).collect();
        for _ in 0..30 {
            let location = self.choose_origin_location(city_obj, level, carrier_key, &held)?;
            let cargo_key =
                self.choose_cargo_for_location(city_obj, location, level, carrier_key, &held)?;
            let cargo = cargo_type(cargo_key).expect("a catalog cargo key");
            if !cargo.missing_credentials(endorsements).is_empty() {
                continue;
            }
            if cargo.lcv_lanes && !self.lcv_lane(&city, &destination) {
                continue;
            }
            if cargo.national_network && !self.national_network_lane(&city, &destination) {
                continue;
            }
            let Some(dest_location) = self.destination_location(&destination, cargo, level) else {
                continue;
            };
            let dest_location = dest_location.clone();
            let origin_name = location.name.clone();
            let location = location.clone();
            if let Some(job) = self.make_job(
                cargo,
                &city,
                &origin_name,
                &destination,
                miles,
                opts.market,
                level,
                &location,
                &dest_location,
                carrier_key,
                opts.direct_freight,
            ) {
                return Some(job);
            }
        }
        None
    }

    /// A weighted spread of distinct destinations (nearer = likelier).
    ///
    /// Aims for at least three distinct cities (or as many as the network
    /// allows) so the dispatch board offers real choices instead of repeating
    /// one destination. Rookies still lean toward short hauls.
    fn spread_destinations(
        &mut self,
        origin: &str,
        reachable: &[Candidate],
        level: i64,
        count: usize,
        carrier_key: &str,
    ) -> Vec<Candidate> {
        let mut best: IndexMap<String, Candidate> = IndexMap::new();
        for cand in reachable {
            match best.get(&cand.0) {
                Some(existing) if cand.1 >= existing.1 => {}
                _ => {
                    best.insert(cand.0.clone(), cand.clone());
                }
            }
        }
        let pool: Vec<Candidate> = best.into_values().collect();
        let target = pool.len().min(count.max(3));
        let exponent = if level <= 2 { 2.0 } else { 1.0 };
        let mut chosen: Vec<Candidate> = Vec::new();
        let mut available = pool.clone();
        while !available.is_empty() && chosen.len() < target {
            let weights: Vec<f64> = available
                .iter()
                .map(|cand| {
                    self.destination_weight(origin, cand, level, carrier_key, Some(exponent))
                })
                .collect();
            let idx = self.rng.choices_indices_weighted(&weights, 1)[0];
            let pick = available.remove(idx);
            chosen.push(pick);
        }
        if chosen.is_empty() {
            pool
        } else {
            chosen
        }
    }

    /// Weighted lane fit for a carrier's modest dispatch tendencies.
    pub fn destination_weight(
        &self,
        origin: &str,
        candidate: &Candidate,
        level: i64,
        carrier_key: &str,
        exponent: Option<f64>,
    ) -> f64 {
        let (destination, miles, _legs) = candidate;
        let miles = *miles;
        let exponent = exponent.unwrap_or(if level <= 2 { 2.0 } else { 1.0 });
        let mut weight = 1.0 / miles.powf(exponent);
        let option = start_option(Some(carrier_key));
        let cap = Self::distance_cap_for_carrier(level, carrier_key).max(1.0);
        let dispatch = option.dispatch_profile();
        if dispatch.short_haul_bias != 0.0 {
            let short_factor = (1.0 - miles.min(cap) / cap).max(0.0);
            weight *= 1.0 + dispatch.short_haul_bias * short_factor;
        }
        if dispatch.long_haul_bias != 0.0 {
            let long_factor = (miles / cap).min(1.0);
            weight *= 1.0 + dispatch.long_haul_bias * long_factor;
        }
        if level >= PREMIUM_LANE_LEVEL {
            // Premium-lane seniority: dispatch shows the long freight first.
            weight *= 1.0 + PREMIUM_LANE_LONG_HAUL_BIAS * (miles / cap).min(1.0);
        }
        if dispatch.regional_bias != 0.0 {
            let origin_region = self.world.cities.get(origin).map(|c| c.region.as_str());
            let dest_region = self
                .world
                .cities
                .get(destination)
                .map(|c| c.region.as_str());
            if origin_region.is_some() && dest_region == origin_region {
                weight *= 1.0 + dispatch.regional_bias;
            }
        }
        weight.max(1e-12)
    }

    /// Whether turnpike doubles may run between these cities: both ends sit
    /// in ISTEA-freeze LCV states, and at least one supported corridor option
    /// stays on a listed LCV turnpike (plus staging stubs). See
    /// `data::lcv_turnpikes`.
    pub(crate) fn lcv_lane(&self, origin: &str, destination: &str) -> bool {
        use crate::data::lcv_turnpikes::city_allows_lcv_turnpike_endpoint;
        if !city_allows_lcv_turnpike_endpoint(origin)
            || !city_allows_lcv_turnpike_endpoint(destination)
        {
            return false;
        }
        let state_of = |key: &str| {
            self.world
                .cities
                .get(key)
                .map(|c| c.state_code.as_str())
                .unwrap_or("")
        };
        if !(crate::models::credentials::lcv_state(state_of(origin))
            && crate::models::credentials::lcv_state(state_of(destination)))
        {
            return false;
        }
        // A listed turnpike lane with a recorded LCV cap in every state it
        // touches; a state with no cap makes the lane illegal, not 80,000 lb.
        TrailerSet::for_cargo_between("turnpike_doubles", self.world, origin, destination).is_some()
    }

    /// Whether any supported corridor option from origin to destination stays
    /// on the National Network approximation (plus reasonable access) inside
    /// the lower-48 US so STAA twin trailers may run it. ALCAN, Canada, and
    /// Alaska lanes are refused by policy. See `data::national_network`.
    pub(crate) fn national_network_lane(&self, origin: &str, destination: &str) -> bool {
        use crate::data::national_network::{
            city_outside_lower_48, filter_staa_doubles_routes, route_outside_lower_48,
        };
        let city_blocked = |key: &str| {
            self.world
                .cities
                .get(key)
                .map(city_outside_lower_48)
                .unwrap_or_else(|| crate::data::national_network::city_key_outside_lower_48(key))
        };
        if city_blocked(origin) || city_blocked(destination) {
            return false;
        }
        match self.world.supported_route_options(origin, destination, 3) {
            Ok(routes) => {
                if routes.iter().all(route_outside_lower_48) && !routes.is_empty() {
                    return false;
                }
                !filter_staa_doubles_routes(&routes).is_empty()
            }
            Err(_) => false,
        }
    }

    /// `(destination, route miles, route leg count)` for every other city.
    pub(crate) fn candidates(&self, city: &str) -> Vec<Candidate> {
        let world_id = self.world as *const World as usize;
        {
            let cache = CANDIDATES_CACHE.lock();
            if let Some(cached) = cache.get(&world_id).and_then(|per| per.get(city)) {
                return cached.clone();
            }
        }
        let mut computed: Vec<Candidate> = Vec::new();
        // Phase A ALCAN: the continuous graph may reach Canada pass-through
        // towns, but the job board must not mint CA destinations for a US
        // carrier (through-freight corridor ≠ cabotage board). Keep offers
        // same-country until a Canada domestic career board is an explicit cut.
        let origin_country = self
            .world
            .cities
            .get(city)
            .map(|c| c.country.as_str())
            .unwrap_or("US");
        for dest in self.world.city_names() {
            if dest == city {
                continue;
            }
            let dest_country = self
                .world
                .cities
                .get(&dest)
                .map(|c| c.country.as_str())
                .unwrap_or("US");
            if dest_country != origin_country {
                continue;
            }
            if let Ok(Some(route)) = self.world.supported_route(city, &dest, None) {
                computed.push((dest, route.miles(), route.legs.len()));
            }
        }
        CANDIDATES_CACHE
            .lock()
            .entry(world_id)
            .or_default()
            .insert(city.to_string(), computed.clone());
        computed
    }

    /// `_choose_destination`: kept for parity; the board spreads destinations
    /// up front instead.
    pub fn choose_destination(&mut self, candidates: &[Candidate], level: i64) -> Candidate {
        let mut pool: Vec<&Candidate> = candidates.iter().collect();
        if level >= 4 {
            // seasoned drivers see a dedicated cross-country slot now and then
            let long_hauls: Vec<&Candidate> = candidates
                .iter()
                .filter(|c| c.1 >= LONG_HAUL_MILES)
                .collect();
            if !long_hauls.is_empty() && self.rng.random() < 0.35 {
                pool = long_hauls;
            }
        }
        // Nearer destinations are likelier, and rookies lean harder toward short
        // hauls (squared distance falloff). Crucially the pool is never narrowed
        // to a single-leg-only set -- that locked sparse start cities into one
        // back-and-forth route. Leg count no longer gates which cities appear.
        let exponent = if level <= 2 { 2.0 } else { 1.0 };
        let weights: Vec<f64> = pool.iter().map(|c| 1.0 / c.1.powf(exponent)).collect();
        let idx = self.rng.choices_indices_weighted(&weights, 1)[0];
        pool[idx].clone()
    }

    /// True when the city has at least one facility that receives freight.
    /// Travel centers and truck parking ship/receive nothing, so a corridor
    /// of fuel-only pins must not soak up the destination cycle.
    fn city_has_freight_receiver(&self, city: &str) -> bool {
        let Some(city_obj) = self.world.cities.get(city) else {
            return false;
        };
        city_obj
            .locations
            .iter()
            .any(|location| !Self::cargo_for_location(location, "receives", None).is_empty())
    }

    fn choose_origin_location<'c>(
        &mut self,
        city: &'c City,
        level: i64,
        carrier_key: &str,
        held: &[&str],
    ) -> Option<&'c Location> {
        let plausible: Vec<&Location> = city
            .locations
            .iter()
            .filter(|location| {
                location.min_level <= level
                    && !Self::cargo_for_location(location, "ships", Some(level)).is_empty()
            })
            .collect();
        if plausible.is_empty() {
            // No freight shipper at this city (travel_center / truck_parking
            // pins ship nothing). Do not fall back onto a fuel lot.
            return None;
        }
        let weights: Vec<f64> = plausible
            .iter()
            .map(|location| {
                let mut weight = Self::facility_weight(city, location, carrier_key);
                // Thin-outbound cross-docks (Fairbanks Carlile): inbound should
                // outweigh outbound, so origin selection down-weights the ship role.
                if location.traits.iter().any(|t| t == "thin_outbound") {
                    weight *= THIN_OUTBOUND_ORIGIN_WEIGHT;
                }
                // A shipper of freight this driver is credentialed for is
                // where dispatch sends this driver first.
                if Self::cargo_for_location(location, "ships", Some(level))
                    .iter()
                    .any(|key| Self::credentialed_and_held(key, held))
                {
                    weight += HELD_CREDENTIAL_FACILITY_BONUS;
                }
                weight
            })
            .collect();
        let idx = self.rng.choices_indices_weighted(&weights, 1)[0];
        Some(plausible[idx])
    }

    /// The cargo asks for a course-earned credential (one no level grants:
    /// hazmat, doubles, TWIC, LCV) and the driver holds everything it asks
    /// for. Level-granted certificates are not the point: every level-18
    /// driver holds refrigerated and high-value, so favouring their freight
    /// would favour most of the board.
    fn credentialed_and_held(cargo_key: &str, held: &[&str]) -> bool {
        cargo_type(cargo_key).is_some_and(|cargo| {
            cargo.credentials.iter().any(|key| {
                crate::models::credentials::credential(key)
                    .is_some_and(|credential| credential.grant_level.is_none())
            }) && cargo.missing_credentials(held).is_empty()
        })
    }

    fn choose_cargo_for_location(
        &mut self,
        city: &City,
        location: &Location,
        level: i64,
        carrier_key: &str,
        held: &[&str],
    ) -> Option<&'static str> {
        let cargo_keys = Self::cargo_for_location(location, "ships", Some(level));
        if cargo_keys.is_empty() {
            // Do not invent freight from the full catalog for a pin that
            // ships nothing (travel_center / truck_parking).
            return None;
        }
        let weights: Vec<f64> = cargo_keys
            .iter()
            .map(|key| {
                let weight = Self::cargo_weight(city, key, carrier_key, level);
                if Self::credentialed_and_held(key, held) {
                    weight * HELD_CREDENTIAL_CARGO_WEIGHT
                } else {
                    weight
                }
            })
            .collect();
        let idx = self.rng.choices_indices_weighted(&weights, 1)[0];
        Some(cargo_keys[idx])
    }

    /// `_cargo_for_location(location, role, level)`.
    ///
    /// Trusts the parsed `ships` / `receives` lists. An explicit empty list
    /// (receive-only / fuel-only) must not re-inflate from the facility-type
    /// cargo table — that would make receive-only overrides ship again.
    pub fn cargo_for_location(
        location: &Location,
        role: &str,
        level: Option<i64>,
    ) -> Vec<&'static str> {
        let role_values: Vec<String> = if role == "ships" {
            location.ships.clone()
        } else {
            location.receives.clone()
        };
        let mut allowed: Vec<&'static str> = Vec::new();
        for key in &role_values {
            let Some(cargo) = cargo_type(key) else {
                continue;
            };
            if level.is_some_and(|level| cargo.min_level > level) {
                continue;
            }
            allowed.push(cargo.key);
        }
        allowed
    }

    fn destination_location(
        &mut self,
        city: &str,
        cargo: &CargoType,
        level: i64,
    ) -> Option<&'w Location> {
        let city_obj = self.world.cities.get(city)?;
        let locations = &city_obj.locations;
        let mut plausible: Vec<&'w Location> = locations
            .iter()
            .filter(|loc| {
                loc.min_level <= level
                    && Self::cargo_for_location(loc, "receives", Some(level)).contains(&cargo.key)
            })
            .collect();
        if plausible.is_empty() {
            plausible = locations
                .iter()
                .filter(|loc| Self::cargo_for_location(loc, "receives", None).contains(&cargo.key))
                .collect();
        }
        if plausible.is_empty() {
            return None;
        }
        let weights: Vec<f64> = plausible
            .iter()
            .map(|loc| Self::facility_weight(city_obj, loc, DEFAULT_START_KEY))
            .collect();
        let idx = self.rng.choices_indices_weighted(&weights, 1)[0];
        Some(plausible[idx])
    }

    fn facility_weight(city: &City, location: &Location, carrier_key: &str) -> f64 {
        let mut weight = FACILITY_SELECTION_WEIGHTS
            .iter()
            .find(|(k, _)| *k == location.facility_type)
            .map(|(_, w)| *w)
            .unwrap_or(0.85);
        let handled = |key: &str| {
            location.ships.iter().any(|k| k == key) || location.receives.iter().any(|k| k == key)
        };
        for tag in &city.market_tags {
            let boosted = market_tag_cargo_bonus(tag);
            if boosted.iter().any(|key| handled(key)) {
                weight += 0.25;
            }
        }
        if location.template {
            weight *= 0.9;
        }
        let option = start_option(Some(carrier_key));
        if !option.cargo_weight_bonus.is_empty()
            && option
                .cargo_weight_bonus
                .iter()
                .any(|(key, _)| handled(key))
        {
            weight += 0.2;
        }
        weight.max(0.1)
    }

    /// `_cargo_weight(city, cargo_key, carrier_key, level)`.
    pub fn cargo_weight(city: &City, cargo_key: &str, carrier_key: &str, level: i64) -> f64 {
        let mut weight = 1.0;
        for tag in &city.market_tags {
            if market_tag_cargo_bonus(tag).contains(&cargo_key) {
                weight += 0.65;
            }
        }
        let cargo = cargo_type(cargo_key).expect("a catalog cargo key");
        if !cargo.credentials.is_empty() {
            // Specialized company drivers see endorsement freight favored
            // instead of rationed; junior boards keep it occasional.
            weight *= if level >= SPECIALIZED_FREIGHT_LEVEL {
                SPECIALIZED_FREIGHT_WEIGHT
            } else {
                0.8
            };
        }
        weight += start_option(Some(carrier_key)).cargo_weight_bonus_for(cargo_key);
        weight
    }

    #[allow(clippy::too_many_arguments)]
    fn make_job(
        &mut self,
        cargo: &'static CargoType,
        origin: &str,
        origin_location: &str,
        destination: &str,
        miles: f64,
        market: Option<&Market>,
        level: i64,
        origin_facility: &Location,
        destination_facility: &Location,
        carrier_key: &str,
        direct_freight: bool,
    ) -> Option<Job> {
        let open_route = self
            .world
            .supported_route(origin, destination, None)
            .ok()
            .flatten();
        // A tunnel or bridge that turns this load away is planned around. No
        // legal road, or one far longer than the open road, is no job: a
        // dispatcher would not book a load that cannot make the trip.
        let material = super::hazmat::commodity_for(
            cargo.key,
            &format!(
                "{origin}|{destination}|{}|{}",
                origin_facility.id, destination_facility.id
            ),
        );
        let load = CrossingLoad::standard_rig(true)
            .with_cargo(
                cargo.key,
                cargo.tank,
                (cargo.weight_tons.1 * 2000.0).round() as u32,
            )
            .with_hazard_class(material.map(|m| m.class));
        let mut miles = miles;
        let route = if load.regulated() {
            let legal = self
                .world
                .legal_route(origin, destination, &load)
                .ok()
                .flatten();
            if let Some(open) = &open_route {
                let legal_road = legal.as_ref()?;
                if legal_road
                    .legs
                    .iter()
                    .map(|leg| leg.id)
                    .ne(open.legs.iter().map(|leg| leg.id))
                {
                    if legal_road.miles() > max_alternate_miles(open.miles()) {
                        return None;
                    }
                    miles = legal_road.miles();
                }
            }
            legal
        } else {
            open_route
        };
        // Clamp to the legal gross for a stock tractor and what this freight
        // hooks: 80,000 lb for a single or STAA twin 28s, the route's
        // turnpike cap for turnpike doubles, with both trailers and the
        // converter dolly in the tare. Heavier catalog ranges exist, but a
        // dispatched load that starts illegal is a lie; the live overweight
        // check still red-lights a truck that ends up over (a heavier
        // tractor, a test load). Turnpike doubles are priced on the lanes
        // the route menu will offer, and a lane with no recorded LCV cap in
        // every state is no job at all, never an 80,000 lb one.
        let mut set = if cargo_requires_lcv_turnpike(cargo.key) {
            TrailerSet::for_cargo_between(cargo.key, self.world, origin, destination)?
        } else {
            TrailerSet::for_cargo_on_route(cargo.key, self.world, route.as_ref())?
        };
        let seasonal_weight_limit = self
            .calendar_hours
            .zip(route.as_ref())
            .and_then(|(hours, route)| strictest_on_route(route, hours))
            .map(|restriction| {
                let mut truck = TruckState::new(TruckSpecs::default());
                truck.trailer_set = set;
                set.legal_gvw_kg = seasonal_gvw_cap_kg(&truck, restriction.percent);
                SeasonalWeightLimit {
                    highway: restriction.highway.to_string(),
                    percent: restriction.percent,
                }
            });
        let tare = combination_tare_kg(&TruckSpecs::default()) - TRAILER_TARE_KG + set.tare_kg;
        let max_tons = ((set.legal_gvw_kg - tare) / KG_PER_TON).max(0.0);
        let hi = cargo.weight_tons.1.min(max_tons);
        let lo = cargo.weight_tons.0.min(hi);
        let weight = self.rng.uniform(lo, hi);
        let rate = cargo.rate_per_mile * self.rng.uniform(0.9, 1.15);
        let mult = market.map(|m| m.multiplier(cargo.key)).unwrap_or(1.0);
        let base_pay = HOOKUP_FEE + miles * rate * (1.0 + weight / 120.0);
        let direct_mult = if direct_freight {
            DIRECT_FREIGHT_PAY_MULT
        } else {
            1.0
        };
        let pay = round_py_n(
            base_pay.max(minimum_pay_for_level(miles, level)) * mult * direct_mult,
            2,
        );
        // deadline: the honest HOS-compliant hours (driving, breaks, sleep),
        // shipper slack on top, plus a flat hour for fuel and the unexpected
        let slack = self.rng.uniform(
            DEADLINE_DISPATCH_SLACK_RANGE.0,
            DEADLINE_DISPATCH_SLACK_RANGE.1,
        );
        let deadline = dispatch_deadline_hours(
            miles,
            slack,
            route.as_ref(),
            Some(self.world),
            self.hos.as_ref(),
        ) * start_option(Some(carrier_key))
            .dispatch_profile()
            .deadline_slack;
        // Speak the stretch when the driver's current clock forces a sleep a
        // fresh clock would not have needed -- the long number is the law.
        let covers_rest = match &self.hos {
            Some(hos) => {
                plan_hos(miles, route.as_ref(), Some(self.world), Some(hos)).sleeps
                    > plan_hos(miles, route.as_ref(), Some(self.world), None).sleeps
            }
            None => false,
        };
        let mut job = Job::new(
            cargo,
            weight,
            origin,
            origin_location,
            destination,
            round_py_n(miles, 1),
            pay,
            round_py_n(deadline, 1),
        );
        job.market_mult = mult;
        job.origin_type = origin_facility.facility_type.clone();
        job.destination_location = destination_facility.name.clone();
        job.destination_type = destination_facility.facility_type.clone();
        job.origin_facility_id = origin_facility.id.clone();
        job.destination_facility_id = destination_facility.id.clone();
        job.origin_locality = origin_facility.locality.clone();
        job.destination_locality = destination_facility.locality.clone();
        job.deadline_covers_rest = covers_rest;
        job.origin_spoken = self.world.spoken_city(origin, Some(true));
        job.destination_spoken = self.world.spoken_city(destination, Some(true));
        job.seasonal_weight_limit = seasonal_weight_limit;
        job.hazmat = material;
        Some(job)
    }
}
