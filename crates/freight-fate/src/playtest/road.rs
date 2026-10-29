//! Drop into a chosen piece of road, set up the way you want to test it
//! (port of `tools/playtest_road.py`).
//!
//! Walking the menus to a specific hill, work zone, or limit drop takes
//! minutes and lands you somewhere slightly different every time. This
//! starts the real game -- real window, real speech, real input, your real
//! settings -- at a road feature you named, with the truck and cruise in the
//! state you asked for. The `departure` scenario instead begins loaded at a
//! real facility gate, so its street chain and on-ramp remain to be driven.
//! Every spoken line goes to a
//! transcript, so the session can be read afterwards.
//!
//! # Shape of the port
//!
//! The Python tool subclassed `MainMenuState` so that `App.run()`'s own
//! `MainMenuState(ctx)` handed back the staged drive the first time and the
//! real menu after -- a shim, because `run()` picked its first screen
//! itself. The Rust `App` takes that screen as
//! [`App::set_initial_state`][crate::app::App::set_initial_state], so the
//! drive is simply the state the loop starts on and there is nothing to
//! monkeypatch. Quitting to the main menu reaches the REAL menu, with its
//! working Exit, exactly as the subclass arranged.
//!
//! The search itself is read-only and runs against the world data alone --
//! no `App`, and so no window. Booting the game to run a search is what
//! opened and closed a real window on every `--scan`.

use ff_core::data::world::{get_world, World};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::career_ladder::MAX_CAREER_LEVEL;
use ff_core::models::carrier_fleet::slip_seat_pool;
use ff_core::models::jobs::{cargo_type, Job, JobBoard};
use ff_core::models::profile::Profile;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::enforcement_posts::{method_by_kind, EnforcementPost, KIND_FIXED_SCALE};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::DRIVE_PHASE_DELIVERY;

use super::MPH_PER_MPS;

pub mod destination;

/// `--routes random` is a bounded, seeded sample from the world. Departure
/// discovery uses this by default; `--routes all` remains available for an
/// exhaustive inventory.
pub const RANDOM_ROUTES: &str = "random";
/// Long hauls make a feature search crawl and put the interesting mile hours
/// from the start. Past this the draw takes another pair instead.
pub const RANDOM_MAX_MILES: f64 = 600.0;
/// How many pairs a random draw offers the search.
pub const RANDOM_SAMPLE: usize = 6;

/// A stable trip roll keeps a scanned departure reproducible when it is
/// launched later. It does not choose a facility or corridor.
const DEPARTURE_TRIP_SEED: i64 = 0;

pub const FEATURES: [&str; 12] = [
    "downgrade",
    "upgrade",
    "zone",
    "limit-drop",
    "stop",
    "scale",
    "curve",
    "interchange",
    "toll",
    "chain-law",
    "destination",
    "departure",
];
const SCAN_STEP_MI: f64 = 0.1;
/// How far before the feature the SEARCH looks back to report a posted
/// limit. Only a reporting distance; the drive's own lead is computed below.
const DEFAULT_LEAD_MI: f64 = 1.8;

/// How much REAL time the driver gets before the feature arrives.
///
/// The lead used to be 1.8 miles flat, and miles are not what a person
/// experiences: the trip compresses distance as well as clock, so at 65 mph
/// with the compression wound up, 1.8 miles is about five seconds. The window
/// had not even taken focus before an open weigh station came and went (owner,
/// 2026-08-21). Twenty-five seconds is long enough to find the window, hear
/// the truck, and still be waiting when the callout lands.
const LEAD_REAL_SECONDS: f64 = 25.0;

/// What waits at a ramp's far end, ranked by how much there is to hear. A
/// signal or a stop puts the cross bubble in front of you -- real traffic on
/// the road the ramp lands on, a gap to wait for, a green that is the sound
/// of that stream dying. A free merge onto another freeway has none of that,
/// so a search for "somewhere to test the ramp end" should not offer it
/// first.
fn ramp_control_rank(control: &str) -> f64 {
    match control {
        "signal" => 4.0,
        "stop" => 3.0,
        "yield" => 2.0,
        "none" => 1.0,
        _ => 0.0,
    }
}

/// One found road feature, with what is needed to describe or drive it.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub origin: String,
    pub destination: String,
    /// Where the feature starts.
    pub at_mi: f64,
    pub total_mi: f64,
    /// Percent for grades, mph for limits and zones, else 0.
    pub magnitude: f64,
    pub run_mi: f64,
    pub limit_mph: f64,
    pub label: String,
    /// The per-trip roll this was found under.
    pub trip_seed: Option<i64>,
    /// The real origin facility for a loaded departure, when applicable.
    pub origin_location: Option<String>,
}

impl Hit {
    pub fn describe(&self) -> String {
        if let Some(facility) = &self.origin_location {
            return format!(
                "{facility} in {}; loaded departure toward {}; {}",
                self.origin, self.destination, self.label
            );
        }
        let run = if self.run_mi >= 0.1 {
            format!(", running {:.1} mi", self.run_mi)
        } else {
            String::new()
        };
        format!(
            "{} -> {}  mile {:6.1} of {:.0}  {}{run}  (posted {:.0})",
            self.origin, self.destination, self.at_mi, self.total_mi, self.label, self.limit_mph
        )
    }
}

/// The options `tools/playtest_road.py` took on the command line.
pub struct RoadOptions {
    pub origin: Option<String>,
    pub destination: Option<String>,
    pub facility: Option<String>,
    pub routes: String,
    pub seed: Option<i64>,
    pub sample: usize,
    pub max_miles: f64,
    pub feature: String,
    pub at: Option<f64>,
    pub pick: usize,
    pub trip_seed: Option<i64>,
    pub scan: bool,
    pub min_pct: f64,
    pub min_run: f64,
    pub min_drop: f64,
    pub max_advisory: f64,
    pub lead: Option<f64>,
    pub cruise: f64,
    pub speed: f64,
    pub cargo: f64,
    pub cargo_type: String,
    pub level: Option<i64>,
    pub descent: Option<String>,
    pub assists: Option<String>,
    pub planned_stop_assist: Option<bool>,
    pub predictive_cruise: Option<bool>,
    pub lane_keeping: Option<String>,
    pub curve_assist: Option<bool>,
    pub transmission: Option<String>,
    pub verbosity: Option<String>,
    pub weather: Option<String>,
    pub hour: Option<f64>,
    pub log: Option<String>,
    pub sandbox: bool,
    /// A staffed enforcement unit planted ahead of the staged start: the
    /// post kind (`enforcement_posts::KIND_*`) and how many miles up the
    /// road. The world's own seeded posts stay; this one is added so a
    /// live check can meet a unit on purpose instead of by lottery.
    pub unit: Option<(String, f64)>,
}

impl Default for RoadOptions {
    fn default() -> Self {
        RoadOptions {
            origin: None,
            destination: None,
            facility: None,
            routes: "all".to_string(),
            seed: None,
            sample: RANDOM_SAMPLE,
            max_miles: RANDOM_MAX_MILES,
            feature: "downgrade".to_string(),
            at: None,
            pick: 0,
            trip_seed: None,
            scan: false,
            min_pct: 3.0,
            min_run: 1.0,
            min_drop: 10.0,
            max_advisory: 45.0,
            lead: None,
            cruise: 0.0,
            speed: 62.0,
            cargo: 20.0,
            cargo_type: "general".to_string(),
            level: None,
            descent: None,
            assists: None,
            planned_stop_assist: None,
            predictive_cruise: None,
            lane_keeping: None,
            curve_assist: None,
            transmission: None,
            verbosity: None,
            weather: None,
            hour: None,
            log: None,
            unit: None,
            sandbox: true,
        }
    }
}

/// The route pairs a run searches, given its options.
pub fn route_pairs(world: &'static World, opts: &RoadOptions) -> Vec<(String, String)> {
    if let (Some(origin), Some(destination)) = (&opts.origin, &opts.destination) {
        return vec![(origin.clone(), destination.clone())];
    }
    let origin = opts
        .origin
        .as_deref()
        .map(|name| world.resolve_city_key(name));
    let destination = opts
        .destination
        .as_deref()
        .map(|name| world.resolve_city_key(name));
    if opts.routes == RANDOM_ROUTES {
        let seed = opts.seed.unwrap_or(DEPARTURE_TRIP_SEED);
        return random_pairs(world, opts.sample, opts.max_miles, seed)
            .into_iter()
            .filter(|(from, to)| {
                origin
                    .as_deref()
                    .is_none_or(|wanted| world.resolve_city_key(from) == wanted)
                    && destination
                        .as_deref()
                        .is_none_or(|wanted| world.resolve_city_key(to) == wanted)
            })
            .collect();
    }
    if origin.is_none() && destination.is_none() {
        return all_world_pairs(world);
    }
    // One side is anchored, so filter BEFORE resolving routes, not after:
    // `all_world_pairs` resolves every ordered pair -- 388,752 route lookups
    // at 624 cities -- and filtering its result threw almost all of that
    // work away. Anchoring one end leaves at most 623 lookups, which is the
    // difference between the agent server's `start_at origin:"Denver"`
    // answering in seconds and hanging for minutes with no progress.
    let names = world.city_names();
    let from_names: Vec<&String> = names
        .iter()
        .filter(|name| {
            origin
                .as_deref()
                .is_none_or(|wanted| world.resolve_city_key(name) == wanted)
        })
        .collect();
    let to_names: Vec<&String> = names
        .iter()
        .filter(|name| {
            destination
                .as_deref()
                .is_none_or(|wanted| world.resolve_city_key(name) == wanted)
        })
        .collect();
    let mut pairs = Vec::new();
    for from in &from_names {
        for to in &to_names {
            if from == to {
                continue;
            }
            if world
                .supported_route(from, to, None)
                .ok()
                .flatten()
                .is_some()
            {
                pairs.push((speakable(world, from), speakable(world, to)));
            }
        }
    }
    pairs
}

/// Candidate city pairs for a loaded departure search.
///
/// Exhaustive departure discovery lets `build_trip` perform the single route
/// resolution pass it already needs. Sending it through `all_world_pairs`
/// first would resolve every supported route twice. Sampled and exact
/// selectors continue to use the normal bounded route picker.
fn departure_candidate_pairs(world: &'static World, opts: &RoadOptions) -> Vec<(String, String)> {
    if opts.routes == RANDOM_ROUTES || (opts.origin.is_some() && opts.destination.is_some()) {
        return route_pairs(world, opts);
    }
    let names = world.city_names();
    let wanted_origin = opts
        .origin
        .as_deref()
        .map(|name| world.resolve_city_key(name));
    let wanted_destination = opts
        .destination
        .as_deref()
        .map(|name| world.resolve_city_key(name));
    let mut pairs = Vec::new();
    for origin in &names {
        if wanted_origin
            .as_deref()
            .is_some_and(|wanted| wanted != origin)
        {
            continue;
        }
        for destination in &names {
            if origin == destination
                || wanted_destination
                    .as_deref()
                    .is_some_and(|wanted| wanted != destination)
            {
                continue;
            }
            pairs.push((origin.clone(), destination.clone()));
        }
    }
    pairs
}

/// Every currently supported directed corridor, discovered from the world.
/// There is deliberately no city or route list here: new baked routes appear
/// in an `--routes all` scan without a launcher change.
pub fn all_world_pairs(world: &'static World) -> Vec<(String, String)> {
    let names = world.city_names();
    let mut pairs = Vec::new();
    for origin in &names {
        for destination in &names {
            if origin == destination {
                continue;
            }
            if world
                .supported_route(origin, destination, None)
                .ok()
                .flatten()
                .is_some()
            {
                pairs.push((speakable(world, origin), speakable(world, destination)));
            }
        }
    }
    pairs
}

/// The first supported corridor on the map, found without resolving the rest.
///
/// A caller that wants ONE road must not pay for all of them: `all_world_pairs`
/// resolves every ordered pair, and at 624 cities that is 388,752 route
/// lookups to reach a value taken with `.next()`. Short-circuiting keeps that
/// caller flat as the map grows.
pub fn first_world_pair(world: &'static World) -> Option<(String, String)> {
    let names = world.city_names();
    for origin in &names {
        for destination in &names {
            if origin == destination {
                continue;
            }
            if world
                .supported_route(origin, destination, None)
                .ok()
                .flatten()
                .is_some()
            {
                return Some((speakable(world, origin), speakable(world, destination)));
            }
        }
    }
    None
}

/// A bounded, deterministic slice of the supported corridors.
///
/// `all_world_pairs` is exhaustive on purpose -- an operator's `--routes all`
/// scan should see every baked road. A test asserting a map-wide invariant
/// wants the same question asked cheaply, and the exhaustive form stopped
/// being runnable as the map grew: 624 cities is 388,752 route resolutions
/// per call, which ran past CI's 45 minute ceiling without finishing.
///
/// Striding the origin list spreads the sample across the whole map instead
/// of clustering in whichever corner sorts first, and takes one corridor per
/// sampled origin so the cost stays flat as cities are added. No RNG: a
/// failure has to name the same corridor on a rerun.
pub fn sampled_world_pairs(world: &'static World, limit: usize) -> Vec<(String, String)> {
    let names = world.city_names();
    let mut pairs = Vec::new();
    if names.len() < 2 || limit == 0 {
        return pairs;
    }
    let step = (names.len() / limit).max(1);
    let mut index = 0;
    while index < names.len() && pairs.len() < limit {
        let origin = &names[index];
        for destination in &names {
            if origin == destination {
                continue;
            }
            if world
                .supported_route(origin, destination, None)
                .ok()
                .flatten()
                .is_some()
            {
                pairs.push((speakable(world, origin), speakable(world, destination)));
                break;
            }
        }
        index += step;
    }
    pairs
}

/// Supported city pairs drawn from the whole map, shortest first.
///
/// Named the way the hand-picked sets are and the way a player would say
/// them, so the banner, the scan lines and a rerun all read as roads rather
/// than as database keys. A name is only used when it resolves back to the
/// same city; anything ambiguous keeps its key. Shortest first, so the
/// search reaches a feature quickly and the drive starts near it rather than
/// hours up the road.
pub fn random_pairs(
    world: &'static World,
    count: usize,
    max_miles: f64,
    seed: i64,
) -> Vec<(String, String)> {
    let mut rng = PyRandom::new_from_i64(seed);
    let names = world.city_names();
    let mut found: Vec<(f64, String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    // Bounded: a draw must never hunt forever for its last pair on a map
    // where most random pairs are longer than the limit.
    for _ in 0..count * 400 {
        if found.len() >= count {
            break;
        }
        let picked = rng.sample(&names, 2);
        let (a, b) = (picked[0].clone(), picked[1].clone());
        let key = if a < b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        };
        if !seen.insert(key) {
            continue;
        }
        let Ok(Some(route)) = world.supported_route(&a, &b, None) else {
            continue;
        };
        if route.miles() > max_miles {
            continue;
        }
        found.push((route.miles(), speakable(world, &a), speakable(world, &b)));
    }
    found.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    found.into_iter().map(|(_, a, b)| (a, b)).collect()
}

/// The city's spoken name where that still names this city, else the key.
///
/// Two cities share a bare name often enough (Jackson, Portland) that a
/// blind swap would silently point a rerun at the wrong road.
fn speakable(world: &World, key: &str) -> String {
    let spoken = world.spoken_city(key, None);
    if world.resolve_city_key(&spoken) == key {
        spoken
    } else {
        key.to_string()
    }
}

/// The trip the search reads -- built on the SAME seed the drive will use.
///
/// Everything drawn per trip rather than baked into the map hangs off this
/// seed: work zones, patrol posts, and whether a given weigh station is open
/// today. Searching on one seed and then driving on another is how a scan
/// could promise an open scale and hand over a dark one (found 2026-08-20,
/// benching the weigh-in-motion bypass).
pub fn build_trip(
    world: &'static World,
    origin: &str,
    destination: &str,
    seed: Option<i64>,
) -> Option<Trip> {
    let route = world.supported_route(origin, destination, None).ok()??;
    Some(Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("", seed, None, None, false),
        TripOptions {
            seed,
            ..Default::default()
        },
    ))
}

/// Every matching feature across the given routes, best first.
pub fn find_feature(
    world: &'static World,
    pairs: &[(String, String)],
    feature: &str,
    opts: &RoadOptions,
    seed: Option<i64>,
) -> Vec<Hit> {
    if feature == "departure" {
        let owned_pairs;
        let departure_pairs = if pairs.is_empty() {
            owned_pairs = departure_candidate_pairs(world, opts);
            &owned_pairs
        } else {
            pairs
        };
        return departure_hits(world, departure_pairs, opts, seed);
    }
    let mut hits: Vec<Hit> = Vec::new();
    for (origin, destination) in pairs {
        // An unroutable pair must not kill the sweep.
        let Some(mut trip) = build_trip(world, origin, destination, seed) else {
            continue;
        };
        match feature {
            "downgrade" | "upgrade" => {
                let sign = if feature == "downgrade" { -1.0 } else { 1.0 };
                hits.extend(grade_hits(
                    &mut trip,
                    origin,
                    destination,
                    sign,
                    opts.min_pct,
                    opts.min_run,
                ));
            }
            "zone" => hits.extend(zone_hits(&mut trip, origin, destination)),
            "limit-drop" => hits.extend(limit_drop_hits(
                &mut trip,
                origin,
                destination,
                opts.min_drop,
            )),
            "stop" => hits.extend(stop_hits(&mut trip, origin, destination)),
            "scale" => hits.extend(scale_hits(&mut trip, origin, destination)),
            "curve" => hits.extend(curve_hits(
                &mut trip,
                origin,
                destination,
                opts.max_advisory,
            )),
            "interchange" => hits.extend(interchange_hits(&mut trip, origin, destination)),
            "toll" => hits.extend(toll_hits(&mut trip, origin, destination)),
            "chain-law" => hits.extend(chain_law_hits(&mut trip, origin, destination)),
            "destination" => hits.extend(self::destination::destination_hits(
                &mut trip,
                origin,
                destination,
            )),
            _ => {}
        }
    }
    for hit in &mut hits {
        hit.trip_seed = seed;
    }
    if feature == "downgrade" || feature == "upgrade" {
        hits.sort_by(|a, b| {
            b.run_mi
                .total_cmp(&a.run_mi)
                .then(b.magnitude.total_cmp(&a.magnitude))
        });
    } else {
        hits.sort_by(|a, b| {
            b.magnitude
                .total_cmp(&a.magnitude)
                .then(a.at_mi.total_cmp(&b.at_mi))
        });
    }
    hits
}

/// Loaded facility departures that can enter a real outbound road from the
/// current world data. A candidate uses a catalog cargo the facility ships,
/// its own turn-level departure chain, and every supported world corridor.
fn departure_hits(
    world: &'static World,
    pairs: &[(String, String)],
    opts: &RoadOptions,
    seed: Option<i64>,
) -> Vec<Hit> {
    let mut destinations_by_origin = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (origin, destination) in pairs {
        let origin_key = world.resolve_city_key(origin);
        let destination_key = world.resolve_city_key(destination);
        if origin_key == destination_key {
            continue;
        }
        destinations_by_origin
            .entry(origin_key)
            .or_default()
            .push(destination_key);
    }
    for destinations in destinations_by_origin.values_mut() {
        destinations.sort();
        destinations.dedup();
    }
    let mut hits = Vec::new();

    for (origin_key, destination_keys) in destinations_by_origin {
        let Ok(city) = world.city(&origin_key) else {
            continue;
        };
        let destinations: Vec<(String, String, f64, f64, String)> = destination_keys
            .into_iter()
            .filter_map(|destination| {
                let mut trip = build_trip(world, &origin_key, &destination, seed)?;
                let highway = trip.route.legs.first()?.highway.clone();
                let (limit, _) = trip.speed_limit_at(0.0);
                let destination_name = speakable(world, &destination);
                Some((
                    destination,
                    destination_name,
                    trip.total_miles(),
                    limit,
                    highway,
                ))
            })
            .collect();

        for location in &city.locations {
            if opts
                .facility
                .as_deref()
                .is_some_and(|wanted| !location.name.eq_ignore_ascii_case(wanted))
            {
                continue;
            }
            // This is the catalog-backed freight check used by job dispatch:
            // locations without a shippable load are not a loaded departure.
            if JobBoard::cargo_for_location(location, "ships", Some(opts.level.unwrap_or(1)))
                .is_empty()
            {
                continue;
            }
            let Ok(Some(departure_route)) =
                world.facility_departure_route(&origin_key, &location.name)
            else {
                continue;
            };
            for (destination_key, destination, total_mi, limit_mph, highway) in &destinations {
                if departure_job_setup(
                    world,
                    &origin_key,
                    &location.name,
                    destination_key,
                    opts.level.unwrap_or(1),
                )
                .is_none()
                {
                    continue;
                }
                hits.push(Hit {
                    origin: speakable(world, &origin_key),
                    destination: destination.clone(),
                    at_mi: 0.0,
                    total_mi: *total_mi,
                    magnitude: 0.0,
                    run_mi: departure_route.miles(),
                    limit_mph: *limit_mph,
                    label: format!("merge onto {highway} via the facility on-ramp"),
                    trip_seed: seed,
                    origin_location: Some(location.name.clone()),
                });
            }
        }
    }
    hits.sort_by(|a, b| {
        a.origin
            .cmp(&b.origin)
            .then(a.origin_location.cmp(&b.origin_location))
            .then(a.destination.cmp(&b.destination))
            .then(a.label.cmp(&b.label))
    });
    hits
}

/// The catalog-backed cargo and receiving facility for a selected departure.
/// Keeping this lookup shared by scanning and launch prevents a listed row
/// from staging a cargo that the destination cannot receive.
fn departure_job_setup(
    world: &World,
    origin: &str,
    origin_location: &str,
    destination: &str,
    level: i64,
) -> Option<(&'static str, String)> {
    let origin_location = world.facility_location(origin, origin_location).ok()?;
    let destination_city = world.city(destination).ok()?;
    for cargo in JobBoard::cargo_for_location(origin_location, "ships", Some(level)) {
        if let Some(destination_location) = destination_city.locations.iter().find(|location| {
            JobBoard::cargo_for_location(location, "receives", Some(level)).contains(&cargo)
        }) {
            return Some((cargo, destination_location.name.clone()));
        }
    }
    None
}

/// `find_feature` on a pinned seed, re-rolled when the find is empty.
///
/// An open weigh station is the case this exists for: it is silent when
/// closed, by design, so a playtest sent to a dark one learns nothing and
/// cannot tell that from the feature being broken. Re-rolling the trip is
/// the same thing a player does by starting another run.
pub fn find_feature_seeded(
    world: &'static World,
    pairs: &[(String, String)],
    feature: &str,
    opts: &RoadOptions,
) -> Vec<Hit> {
    let seed = opts.trip_seed.or(opts.seed).or(Some(DEPARTURE_TRIP_SEED));
    let mut hits = find_feature(world, pairs, feature, opts, seed);
    if feature == "scale" {
        hits.retain(|hit| hit.label.starts_with("OPEN"));
    }
    hits
}

/// Every launchable road family, with every instance supplied by the current
/// world and catalog. State transitions that need a live player history are
/// intentionally outside this launcher; their registered checks remain in
/// the break-scenario runner rather than being faked as road starts.
fn find_all_features(
    world: &'static World,
    pairs: &[(String, String)],
    opts: &RoadOptions,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    for family in FEATURES {
        let mut family_hits = find_feature_seeded(world, pairs, family, opts);
        for hit in &mut family_hits {
            hit.label = format!("{family}: {}", hit.label);
        }
        hits.extend(family_hits);
    }
    hits.sort_by(|a, b| {
        a.label
            .cmp(&b.label)
            .then(a.origin.cmp(&b.origin))
            .then(a.origin_location.cmp(&b.origin_location))
            .then(a.destination.cmp(&b.destination))
            .then(a.at_mi.total_cmp(&b.at_mi))
    });
    hits
}

/// Sustained grades in the requested direction.
fn grade_hits(
    trip: &mut Trip,
    origin: &str,
    destination: &str,
    sign: f64,
    min_pct: f64,
    min_run: f64,
) -> Vec<Hit> {
    let total = trip.total_miles();
    let mut hits = Vec::new();
    let (mut mi, mut start, mut run) = (0.0f64, None::<f64>, 0.0f64);
    while mi < total {
        let pct = trip.grade_at(mi) * 100.0 * sign;
        if pct >= min_pct {
            if start.is_none() {
                start = Some(mi);
            }
            run += SCAN_STEP_MI;
        } else {
            if let Some(at) = start {
                if run >= min_run {
                    let mut probe = at;
                    let mut worst = 0.0f64;
                    while probe < at + run {
                        worst = worst.max(trip.grade_at(probe) * 100.0 * sign);
                        probe += SCAN_STEP_MI;
                    }
                    let (limit, _) = trip.speed_limit_at((at - DEFAULT_LEAD_MI).max(0.0));
                    let word = if sign < 0.0 { "downgrade" } else { "upgrade" };
                    hits.push(Hit {
                        origin: origin.to_string(),
                        destination: destination.to_string(),
                        at_mi: at,
                        total_mi: total,
                        magnitude: worst,
                        run_mi: run,
                        limit_mph: limit,
                        label: format!("{worst:.1}% {word}"),
                        trip_seed: None,
                        origin_location: None,
                    });
                }
            }
            start = None;
            run = 0.0;
        }
        mi += SCAN_STEP_MI;
    }
    hits
}

fn zone_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let zones = trip.zones.clone();
    zones
        .iter()
        .map(|zone| {
            let (limit, _) = trip.speed_limit_at((zone.start_mi - DEFAULT_LEAD_MI).max(0.0));
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: zone.start_mi,
                total_mi: total,
                magnitude: zone.limit_mph,
                run_mi: (zone.end_mi - zone.start_mi).max(0.0),
                limit_mph: limit,
                label: match zone.closed_side.as_deref() {
                    Some(side) => format!(
                        "{} zone, {:.0} mph, {side} lane closed",
                        zone.reason, zone.limit_mph
                    ),
                    None => format!("{} zone, {:.0} mph", zone.reason, zone.limit_mph),
                },
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}

/// Places the posted limit falls by at least `min_drop` mph.
fn limit_drop_hits(trip: &mut Trip, origin: &str, destination: &str, min_drop: f64) -> Vec<Hit> {
    let total = trip.total_miles();
    let mut hits = Vec::new();
    let mut mi = SCAN_STEP_MI;
    let (mut previous, _) = trip.speed_limit_at(0.0);
    while mi < total {
        let (limit, _) = trip.speed_limit_at(mi);
        if previous - limit >= min_drop {
            hits.push(Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: mi,
                total_mi: total,
                magnitude: previous - limit,
                run_mi: 0.0,
                limit_mph: previous,
                label: format!("limit drops {previous:.0} to {limit:.0}"),
                trip_seed: None,
                origin_location: None,
            });
        }
        previous = limit;
        mi += SCAN_STEP_MI;
    }
    hits
}

fn stop_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let stops = trip.stops.clone();
    stops
        .iter()
        .map(|stop| {
            let (limit, _) = trip.speed_limit_at((stop.at_mi - DEFAULT_LEAD_MI).max(0.0));
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: stop.at_mi,
                total_mi: total,
                magnitude: 0.0,
                run_mi: 0.0,
                limit_mph: limit,
                label: format!("{}: {}", stop.stop_type, stop.name),
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}

/// Weigh stations, the OPEN ones first.
///
/// A closed scale is silent by design -- its guards stay inert so that
/// hearing nothing means "dark", not "missed it". Landing at one is a
/// playtest that proves nothing, so openness is read here (the same seeded
/// draw the drive itself reads, off the trip's posts) and reported rather
/// than discovered at fifty-five miles an hour.
fn scale_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let open_anchors: std::collections::HashSet<String> = trip
        .posts
        .iter()
        .filter(|post| post.kind == KIND_FIXED_SCALE)
        .map(|post| post.anchor.clone())
        .collect();
    let stops = trip.stops.clone();
    stops
        .iter()
        .filter(|stop| stop.stop_type == "weigh_station")
        .map(|stop| {
            let (limit, _) = trip.speed_limit_at((stop.at_mi - DEFAULT_LEAD_MI).max(0.0));
            let is_open = open_anchors.contains(&stop.key());
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: stop.at_mi,
                total_mi: total,
                // Magnitude is what the sort ranks on, and an open scale is
                // the only kind worth driving to.
                magnitude: if is_open { 1.0 } else { 0.0 },
                run_mi: 0.0,
                limit_mph: limit,
                label: format!(
                    "{} scale: {}",
                    if is_open { "OPEN" } else { "closed" },
                    stop.name
                ),
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}

/// Baked curves, tightest advisory first -- the pacenote's own source.
///
/// Connector curves (ramps) are skipped: the interesting case is a mainline
/// bend taken at speed, not the geometry of an exit you are already braking
/// for.
fn curve_hits(trip: &mut Trip, origin: &str, destination: &str, max_advisory: f64) -> Vec<Hit> {
    let total = trip.total_miles();
    let curves = trip.curves.clone();
    let mut hits = Vec::new();
    for curve in &curves {
        if curve.connector {
            continue;
        }
        let advisory = curve.advisory_mph as f64;
        if advisory <= 0.0 || advisory > max_advisory {
            continue;
        }
        let at = curve.start_mi;
        let (limit, _) = trip.speed_limit_at((at - DEFAULT_LEAD_MI).max(0.0));
        let side = if curve.direction == 'R' {
            "right"
        } else {
            "left"
        };
        hits.push(Hit {
            origin: origin.to_string(),
            destination: destination.to_string(),
            at_mi: at,
            total_mi: total,
            // Rank by how much speed the bend actually asks you to give up.
            magnitude: (limit - advisory).max(0.0),
            run_mi: (curve.end_mi - at).max(0.0),
            limit_mph: limit,
            label: format!(
                "{side} curve, advisory {advisory:.0} ({} ft radius)",
                curve.min_radius_ft
            ),
            trip_seed: None,
            origin_location: None,
        });
    }
    hits
}

/// Real signed exits, for testing the exit callout and ramp handling.
///
/// The label carries what the map says is at the END of the ramp, because
/// that is the half a playtest usually means: the terminal control and the
/// road class it lands on decide whether there is anything to listen to.
fn interchange_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let mut staged: Vec<(f64, String)> = Vec::new();
    let mut offset = 0.0;
    for (i, leg) in trip.route.legs.iter().enumerate() {
        let forward = trip.route.cities[i] == leg.a;
        for ic in leg.interchanges() {
            let at_leg = if forward {
                ic.at_mi
            } else {
                leg.miles - ic.at_mi
            };
            let at = offset + at_leg;
            if !(0.0..total).contains(&at) {
                continue;
            }
            let label = if !ic.name.is_empty() {
                ic.name.clone()
            } else {
                ic.destinations.first().cloned().unwrap_or_default()
            };
            let control = if ic.ramp_control.is_empty() {
                "unmapped"
            } else {
                &ic.ramp_control
            };
            let far_end = if ic.ramp_far_end.is_empty() {
                "unmapped"
            } else {
                &ic.ramp_far_end
            };
            let exit_ref = if ic.exit_ref.is_empty() {
                "?"
            } else {
                &ic.exit_ref
            };
            staged.push((
                at,
                format!("exit {exit_ref} {label} [{control} -> {far_end}]")
                    .trim()
                    .to_string(),
            ));
            // The rank is the control's, computed here while it is in hand.
            let rank = ramp_control_rank(control);
            staged.last_mut().unwrap().0 = at;
            let _ = rank;
        }
        offset += leg.miles;
    }
    // Second pass so `speed_limit_at` (which needs `&mut trip`) is not held
    // across the immutable route borrow above.
    staged
        .into_iter()
        .map(|(at, label)| {
            let (limit, _) = trip.speed_limit_at((at - DEFAULT_LEAD_MI).max(0.0));
            let control = label
                .split('[')
                .nth(1)
                .and_then(|tail| tail.split(" ->").next())
                .unwrap_or("");
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: at,
                total_mi: total,
                magnitude: ramp_control_rank(control),
                run_mi: 0.0,
                limit_mph: limit,
                label,
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}

fn toll_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let mut staged: Vec<(f64, f64, String)> = Vec::new();
    let mut offset = 0.0;
    for (i, leg) in trip.route.legs.iter().enumerate() {
        let forward = trip.route.cities[i] == leg.a;
        for toll in leg.toll_events() {
            let at_leg = if forward {
                toll.at_mi
            } else {
                leg.miles - toll.at_mi
            };
            let at = offset + at_leg;
            if !(0.0..total).contains(&at) {
                continue;
            }
            // Ticket-system entries carry no amount of their own -- the charge
            // settles at the exit -- so say that rather than printing $0.00.
            let price = if toll.amount != 0.0 {
                format!("${:.2}", toll.amount)
            } else if !toll.method.is_empty() {
                toll.method.clone()
            } else {
                "no charge here".to_string()
            };
            staged.push((
                at,
                toll.amount,
                format!("toll {}: {price}", toll.name).trim().to_string(),
            ));
        }
        offset += leg.miles;
    }
    staged
        .into_iter()
        .map(|(at, amount, label)| {
            let (limit, _) = trip.speed_limit_at((at - DEFAULT_LEAD_MI).max(0.0));
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: at,
                total_mi: total,
                magnitude: amount,
                run_mi: 0.0,
                limit_mph: limit,
                label,
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}

/// Chain-law areas. Whether the law is *up* depends on live weather, so pair
/// this with a forced winter weather to make the pass actually demand chains.
fn chain_law_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let areas = trip.chain_law_areas.clone();
    areas
        .iter()
        .map(|(start_mi, end_mi)| {
            let (limit, _) = trip.speed_limit_at((start_mi - DEFAULT_LEAD_MI).max(0.0));
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: *start_mi,
                total_mi: total,
                magnitude: end_mi - start_mi,
                run_mi: end_mi - start_mi,
                limit_mph: limit,
                label: "chain-law area (needs winter weather to be active)".to_string(),
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}

/// Miles that take `seconds` of REAL time at this speed.
///
/// The trip's effective time scale compresses distance, so a lead written in
/// miles shrinks to nothing at speed -- which is how a playtest launched at
/// an open scale arrived before its own window did.
///
/// The CONFIGURED scale, not the effective one: effective ramps from 4x at a
/// standstill toward the full setting around 50 mph, so reading it while the
/// truck is still parked reports 4x and hands back a lead that shrinks to
/// nothing the moment the drive is actually up to speed.
fn lead_for_seconds(trip: &Trip, speed_mph: f64, seconds: f64) -> f64 {
    let scale = trip.time_scale.max(1.0);
    (speed_mph * scale * seconds / 3600.0).max(0.5)
}

/// The run-in this feature wants, in route miles.
///
/// Every feature gets the same twenty-five real seconds by default. The
/// exceptions are features that announce themselves BEFORE you reach them: a
/// lead measured to the feature drops the driver in on top of the callout
/// with nothing to hear. `destination` is the one such finder, and its
/// override is derived from the callout's own trigger rather than chosen
/// (see [`destination::destination_lead_mi`]).
///
/// `--at` keeps the plain lead whatever `--find` says, because a named mile
/// is a mile the operator measured for themselves.
fn feature_lead_mi(trip: &Trip, hit: &Hit, opts: &RoadOptions) -> f64 {
    if hit.origin_location.is_some() {
        0.0
    } else if hit.label.contains("destination exit") && opts.at.is_none() {
        destination::destination_lead_mi(trip, opts.speed)
    } else {
        lead_for_seconds(trip, opts.speed, LEAD_REAL_SECONDS)
    }
}

/// A `DrivingState` already rolling at the feature, set up as asked.
pub fn build_driving(ctx: &mut GameContext, hit: &Hit, opts: &RoadOptions) -> (DrivingState, f64) {
    let departure = hit.origin_location.is_some();
    // Settings first: DrivingState reads the gearbox and the assist choices
    // in its constructor, so an override applied afterwards would not take.
    {
        let s = &mut ctx.settings;
        if let Some(preset) = &opts.assists {
            s.apply_driving_assistance_preset(preset);
        }
        if let Some(descent) = &opts.descent {
            s.descent_speed_control = descent.clone();
        }
        if let Some(on) = opts.predictive_cruise {
            s.predictive_cruise = on;
        }
        if let Some(mode) = &opts.lane_keeping {
            s.lane_keeping = mode.clone();
        }
        // Off by default and outside every preset, so a rest-stop drive that
        // wants to hear the entrance stop has to ask for it here -- the
        // sandbox copies the player's real settings on every launch and must
        // never leak a playtest flag back into them.
        if let Some(on) = opts.planned_stop_assist {
            s.destination_approach_assist = on;
        }
        if let Some(on) = opts.curve_assist {
            s.curve_speed_assist = on;
        }
        if let Some(kind) = &opts.transmission {
            s.automatic_transmission = kind == "automatic";
        }
        if let Some(rung) = &opts.verbosity {
            s.driving_speech = rung.clone();
        }
        if departure {
            // This scenario exists to hear the keeper cover the low-speed
            // departure and hand off to adaptive cruise. The sandbox restores
            // the operator's real settings on exit, so this cannot persist.
            s.speed_keeper = true;
            s.automatic_transmission = true;
        }
    }

    // The canonical key, not the display name the route sets are written in:
    // a career's current_city is a slug ("dallas_tx_us"), and cloud backup
    // refuses anything else as an unknown city. Left as the display name,
    // every playtest quietly threw a rejected upload at the server and told
    // the driver its backup was not accepted (2026-08-15).
    let origin_key = ctx.world.resolve_city_key(&hit.origin);
    let destination_key = ctx.world.resolve_city_key(&hit.destination);
    let mut profile = Profile::named_in("Playtest", &origin_key);
    if let Some(level) = opts.level {
        // A bench career starts at level one, which silently switches off
        // every level-gated behaviour a playtest might be here for -- the
        // weigh-in-motion transponder a company driver is issued at four
        // above all, whose absence looks exactly like the feature not
        // working. Set the XP, not the level: the level is derived.
        let level = level.clamp(1, MAX_CAREER_LEVEL) as usize;
        profile.career.xp = LEVEL_XP[level - 1];
    }
    // A bench career is not somebody's first drive. Without this the profile
    // defaults to tutorial_done=false, first-run teaching outranks the rung
    // by design, and the driving speech ladder is switched OFF for the whole
    // run -- so a quiet rung reported "quiet" and changed nothing, and every
    // rung sounded identical (owner, 2026-08-17).
    profile.tutorial_done = true;
    // A scenario staged before this carries its truck, driving record, and
    // HOS state into the drive. Otherwise `start_at` silently replaces a
    // staged short-rest or near-limit clock with a fully rested bench driver.
    // A fresh process has no profile and gets the sound bench truck.
    if let Some(previous) = ctx.profile.as_ref() {
        profile.truck = previous.truck.clone();
        profile.truck_conditions = previous.truck_conditions.clone();
        profile.driving_record = previous.driving_record.clone();
        profile.out_of_service_events = previous.out_of_service_events;
        profile.career.reputation = previous.career.reputation;
        profile.hos = previous.hos.clone();
        profile.fatigue = previous.fatigue;
        // A slip-seating driver's yard spares are drawn from the career's
        // own name, so this bench career's pool is not the scenario's: the
        // record the scenario set would sit under keys this drive never
        // reads, and the spare it draws would be provisioned fresh (found
        // live 2026-09-16, a 95 percent tire reading as zero). Stamp the
        // active truck's record onto every spare this career can draw.
        let stamp = previous
            .truck_conditions
            .get(&previous.active_truck_key())
            .cloned();
        if let Some(record) = stamp {
            for key in slip_seat_pool(&profile) {
                profile
                    .truck_conditions
                    .insert(key.to_string(), record.clone());
            }
            let own = profile.truck.clone();
            profile.truck_conditions.insert(own, record);
        }
    }
    ctx.profile = Some(profile);

    let route = ctx
        .world
        .supported_route(&hit.origin, &hit.destination, None)
        .ok()
        .flatten()
        .expect("the hit's own route still routes");
    // The job's endpoints are keys for the same reason. Delivering runs
    // `profile.current_city = job.destination`, so a job built from the route
    // sets' display names puts the label straight back after the first drop.
    // The spoken fields keep the display names, so nothing reads a slug
    // aloud.
    let departure_setup = if departure {
        let facility = hit
            .origin_location
            .as_deref()
            .expect("departure hit includes its origin facility");
        Some(
            departure_job_setup(
                ctx.world,
                &origin_key,
                facility,
                &destination_key,
                opts.level.unwrap_or(1),
            )
            .expect("departure hit remains a catalog-backed job"),
        )
    } else {
        None
    };
    let cargo_key = departure_setup
        .as_ref()
        .map(|(cargo, _)| *cargo)
        .unwrap_or(&opts.cargo_type);
    let cargo = cargo_type(cargo_key)
        .unwrap_or_else(|| panic!("{cargo_key:?} is not in the cargo catalog"));
    let origin_location = departure_setup
        .as_ref()
        .and(hit.origin_location.as_deref())
        .unwrap_or("company yard");
    let destination_location = departure_setup
        .as_ref()
        .map(|(_, location)| location.as_str())
        .unwrap_or("company yard");
    let mut job = Job::new(
        cargo,
        opts.cargo,
        &origin_key,
        origin_location,
        &destination_key,
        route.miles(),
        2500.0,
        14.0,
    );
    // The SPOKEN city, not the pair as it was typed. `--from`/`--to` take a
    // key as readily as a name, and the facility label is read aloud in the
    // opening summary, in the destination-exit call and in the missed-exit
    // line -- so a slug here put "the destination exit for freight terminal
    // hattiesburg_ms_us freight market" into three of the lines a playtest
    // is usually listening for.
    job.origin_spoken = ctx.world.spoken_city(&origin_key, None);
    job.destination_spoken = ctx.world.spoken_city(&destination_key, None);
    if !departure {
        job.destination_location = format!("{} freight market", job.destination_spoken);
    } else {
        job.destination_location = destination_location.to_string();
    }

    let mut driving = DrivingState::new(
        ctx,
        job,
        route,
        // The seed the feature was FOUND under. Without it DrivingState draws
        // its own, and the drive gets a different set of work zones, patrol
        // posts and open scales than the search just promised.
        hit.trip_seed,
        DRIVE_PHASE_DELIVERY,
        Some(opts.hour.unwrap_or(9.0)),
    );

    let lead_mi = opts
        .lead
        .unwrap_or_else(|| feature_lead_mi(&driving.trip, hit, opts));
    let total = driving.trip.total_miles();
    let start_mi = (hit.at_mi - lead_mi).clamp(0.0, (total - 1.0).max(0.0));
    driving.trip.position_mi = start_mi;
    // The road behind the start is driven, not passed on the first frame:
    // unset, the jump from mile 0 spoke every state line and toll on the way
    // and played a trooper pass for every post (agent drives, 2026-09-23).
    driving.trip.settle_road_behind();
    driving.enforcement_prev_mi = start_mi;
    // And a post wholly behind the start has nothing left to watch, so it is
    // heard-and-passed without its marker: every one of them played at once.
    // Not `announced` -- that stays "this post made a noise" -- and a post
    // whose reach covers the start still sounds, since it can act.
    let behind: Vec<String> = driving
        .trip
        .posts
        .iter()
        .filter(|post| post.at_mi + post.reach_mi < start_mi)
        .map(|post| post.id())
        .collect();
    driving.marked_post_ids.extend(behind.iter().cloned());
    driving.passed_post_ids.extend(behind);
    if let Some((kind, ahead_mi)) = &opts.unit {
        // Staffed and alert, but not yet announced: the marker cue still
        // has to play before it may look, the same contract every seeded
        // post lives under.
        let at_mi = (start_mi + ahead_mi.max(0.0)).min(total - 0.5);
        let (leg_index, _) = driving.trip.leg_at_mile(at_mi);
        let mut post = EnforcementPost::new(at_mi, kind);
        post.leg_index = leg_index;
        post.method = method_by_kind(kind).to_string();
        post.reach_mi = 0.6;
        post.facing = "both".to_string();
        post.staffed = true;
        post.notice = 1.0;
        driving.trip.posts.push(post);
    }
    if let Some(name) = &opts.weather {
        if let Some(kind) = weather_kind(name) {
            driving.weather_mut().current = kind;
        }
    }
    let grade = driving.trip.grade_at(start_mi);
    let gears = driving.truck().transmission.num_gears();
    driving.truck_mut().start_engine();
    driving.truck_mut().set_air_ready(false);
    if departure {
        // Do not teleport past the facility: `update_frame` starts this
        // loaded delivery on its real departure chain. The owner accelerates
        // to rolling speed once, then the armed session selects the keeper.
        driving.truck_mut().velocity_mps = 0.0;
        driving.speed_control_armed = true;
    } else {
        driving.truck_mut().velocity_mps = opts.speed / MPH_PER_MPS;
        driving.truck_mut().transmission.gear = gears;
        driving.truck_mut().grade = grade;
    }
    // A bend whose call window the drop landed inside was heard before the
    // handoff; a departure start at 0 mph calls nothing, so this is a no-op
    // there but keeps the two paths identical.
    driving.trip.settle_calls_in_hand();
    if !departure && opts.cruise > 0.0 {
        // Engage the way K does, so the session is armed exactly as a
        // player's would be rather than a hand-set field the rest of the
        // state does not know about.
        driving.engage_cruise(ctx, opts.cruise, false);
    }
    (driving, start_mi)
}

/// `WeatherKind[args.weather.upper()]`: the member name, forgiving about
/// hyphens and spaces so `--weather "heavy rain"` lands on `HEAVY_RAIN`.
fn weather_kind(name: &str) -> Option<WeatherKind> {
    let wanted = name.trim().to_ascii_uppercase().replace(['-', ' '], "_");
    WeatherKind::ALL
        .into_iter()
        .find(|kind| kind.name() == wanted)
}

/// The setup banner the tool printed before handing over the wheel.
pub fn print_setup(ctx: &mut GameContext, hit: &Hit, start_mi: f64, opts: &RoadOptions) {
    let world = ctx.world;
    let Some(mut trip) = build_trip(world, &hit.origin, &hit.destination, hit.trip_seed) else {
        return;
    };
    let total = trip.total_miles();
    let (limit, reason) = trip.speed_limit_at(start_mi);
    let s = &ctx.settings;
    println!("\n=== playtest: {} -> {} ===", hit.origin, hit.destination);
    println!(
        "  target            : {} at mile {:.1}",
        hit.label, hit.at_mi
    );
    println!(
        "  trip seed         : {:?}  (--trip-seed to drive this exact run again)",
        hit.trip_seed
    );
    if hit.origin_location.is_some() {
        println!(
            "  facility streets  : {:.1} mi to {}",
            hit.run_mi, hit.label
        );
    } else {
        println!("  starting at mile  : {start_mi:.1} of {total:.0}");
        println!(
            "  posted limit here : {limit:.0} mph{}",
            reason.map(|r| format!(" ({r})")).unwrap_or_default()
        );
    }
    if hit.origin_location.is_some() {
        println!(
            "  starting state    : stopped at the facility gate, {:.0} t aboard",
            opts.cargo
        );
    } else {
        println!(
            "  rolling at        : {:.0} mph, {:.0} t aboard",
            opts.speed, opts.cargo
        );
    }
    println!(
        "  {:<18}: {}",
        if hit.origin_location.is_some() {
            "speed control"
        } else {
            "cruise"
        },
        if hit.origin_location.is_some() {
            "armed: speed keeper takes the streets and ramp, then adaptive cruise resumes"
                .to_string()
        } else if opts.cruise > 0.0 {
            format!("set {:.0} mph", opts.cruise)
        } else {
            "off".to_string()
        }
    );
    println!("  your real settings:");
    println!(
        "    transmission    : {}",
        if s.automatic_transmission {
            "automatic"
        } else {
            "manual"
        }
    );
    println!("    driving speech  : {}", s.driving_speech);
    println!(
        "    units           : {}",
        if s.imperial_units {
            "miles"
        } else {
            "kilometers"
        }
    );
    println!(
        "    speed keeper    : {}",
        if s.speed_keeper { "on" } else { "off" }
    );
    println!("    descent control : {}", s.descent_speed_control);
    println!(
        "    predictive cruise: {}",
        if s.predictive_cruise { "on" } else { "off" }
    );
    println!("    assists preset  : {}", s.driving_assistance_preset);
    println!("    time scale      : {}", s.time_scale);
    if hit.origin_location.is_none() {
        println!("  grade ahead       :");
        for ahead in [0.0, 1.0, 2.0, 3.0, 5.0, 8.0] {
            let at = start_mi + ahead;
            if at < total {
                println!(
                    "    +{ahead:4.1} mi      {:+5.1}%",
                    trip.grade_at(at) * 100.0
                );
            }
        }
    }
    if hit.origin_location.is_some() {
        let facility = hit
            .origin_location
            .as_deref()
            .unwrap_or("the selected facility");
        println!(
            "  departure         : loaded at {facility}; accelerate until automatic speed control takes over, then listen for the speed keeper to hand off to adaptive cruise on the acceleration lane before you merge"
        );
    }
}

/// Everything the launcher needs after the search: the chosen spot, or a
/// reason nothing was chosen.
pub enum RoadPlan {
    /// Drive this.
    Drive(Hit),
    /// The search printed its results and there is nothing to drive.
    Done(i32),
}

/// Pick the spot, against the world data alone -- no `App`, no window.
pub fn plan(opts: &RoadOptions) -> RoadPlan {
    let world = get_world();
    if opts.routes != "all" && opts.routes != RANDOM_ROUTES {
        println!("Unknown --routes {:?}. Choose all or random.", opts.routes);
        return RoadPlan::Done(1);
    }
    if opts.feature == "departure" && opts.at.is_some() {
        println!("--at cannot select a facility departure; use --scan, then its launch command.");
        return RoadPlan::Done(1);
    }
    if opts.facility.is_some() && opts.feature != "departure" {
        println!("--facility only applies to --find departure.");
        return RoadPlan::Done(1);
    }
    let pairs = if opts.feature == "departure" {
        departure_candidate_pairs(world, opts)
    } else {
        route_pairs(world, opts)
    };
    if pairs.is_empty() {
        println!(
            "No supported route under {:.0} miles came up; raise --max-miles.",
            opts.max_miles
        );
        return RoadPlan::Done(1);
    }
    if let Some(at) = opts.at {
        let (origin, destination) = pairs[0].clone();
        let seed = opts
            .trip_seed
            .unwrap_or_else(|| PyRandom::new_unseeded().randrange(1 << 31));
        let Some(mut trip) = build_trip(world, &origin, &destination, Some(seed)) else {
            println!("No supported route {origin} -> {destination}.");
            return RoadPlan::Done(1);
        };
        let total = trip.total_miles();
        let (limit, _) = trip.speed_limit_at(at);
        return RoadPlan::Drive(Hit {
            origin,
            destination,
            at_mi: at,
            total_mi: total,
            magnitude: 0.0,
            run_mi: 0.0,
            limit_mph: limit,
            label: "chosen mile".to_string(),
            trip_seed: Some(seed),
            origin_location: None,
        });
    }
    // A typo used to reach the search, match nothing, and come back as
    // "Nothing matched. Try another route" -- which reads as a road without
    // the feature rather than a name the tool does not have.
    if opts.feature != "all" && !FEATURES.contains(&opts.feature.as_str()) {
        println!(
            "Unknown --find {:?}. Choose one of: all, {}.",
            opts.feature,
            FEATURES.join(", ")
        );
        return RoadPlan::Done(1);
    }
    if opts.feature == "departure" {
        println!(
            "Searching {} route(s) for loaded facility departures...",
            pairs.len()
        );
    } else if opts.feature == "all" {
        println!("Searching every launchable road feature in current world data...");
    } else {
        println!(
            "Searching {} route(s) for a {}...",
            pairs.len(),
            opts.feature
        );
    }
    let hits = if opts.feature == "all" {
        find_all_features(world, &pairs, opts)
    } else {
        find_feature_seeded(world, &pairs, &opts.feature, opts)
    };
    if hits.is_empty() {
        let hint = match opts.feature.as_str() {
            "downgrade" | "upgrade" => "Loosen --min-pct / --min-run",
            "limit-drop" => "Loosen --min-drop",
            "curve" => "Raise --max-advisory",
            "toll" => "Tolled corridors are mostly eastern turnpikes",
            // Every supported route has a delivery exit, so an empty find
            // here is a routing failure rather than a road that lacks the
            // feature -- the pair did not resolve at all.
            "destination" => "Every route has one, so check the city names resolve",
            _ => "Try another route",
        };
        println!("Nothing matched. {hint}, or try --routes all.");
        return RoadPlan::Done(1);
    }
    if opts.scan {
        println!("\n{} found:\n", hits.len());
        for (i, found) in hits.iter().enumerate() {
            println!("  [{i:2}] {}", found.describe());
            if opts.feature == "departure" {
                let facility = found
                    .origin_location
                    .as_deref()
                    .expect("departure hits name their facility");
                let seed = found
                    .trip_seed
                    .expect("data-driven hits carry a stable trip seed");
                println!(
                    "       launch: cargo run --release -p freight-fate --bin freightfate -- --playtest-road --find departure --from {:?} --to {:?} --facility {:?} --trip-seed {seed} --ai",
                    found.origin, found.destination, facility
                );
            }
        }
        if opts.feature == "departure" {
            println!(
                "\nEach launch command selects one departure directly; it does not repeat the discovery scan. Use --routes all only for an exhaustive inventory."
            );
        } else if opts.feature == "all" {
            let seed = hits[0]
                .trip_seed
                .expect("data-driven hits always carry a stable trip seed");
            let scope = departure_scope_args(opts);
            println!(
                "\nLaunch a zero-based row with: cargo run --release -p freight-fate --bin freightfate -- --playtest-road --find {} --pick N --trip-seed {seed}{scope}",
                opts.feature
            );
            if opts.feature == "all" {
                println!("  Boundary: state transitions that require live player history are not road-launchable; use the registered break scenarios for those checks.");
            }
        } else {
            println!("\nDrive one with --pick N (keeping the same --find/--routes).");
        }
        // Read-only: the game never started, so no window ever opened.
        return RoadPlan::Done(0);
    }
    if opts.pick >= hits.len() {
        println!("--pick {} out of range; {} found.", opts.pick, hits.len());
        return RoadPlan::Done(1);
    }
    RoadPlan::Drive(hits[opts.pick].clone())
}

fn departure_scope_args(opts: &RoadOptions) -> String {
    let mut args = format!(
        " --routes {:?} --sample {} --max-miles {} --min-pct {} --min-run {} --min-drop {} --max-advisory {}",
        opts.routes,
        opts.sample,
        opts.max_miles,
        opts.min_pct,
        opts.min_run,
        opts.min_drop,
        opts.max_advisory
    );
    if let Some(seed) = opts.seed {
        args.push_str(&format!(" --seed {seed}"));
    }
    if let Some(origin) = &opts.origin {
        args.push_str(&format!(" --from {origin:?}"));
    }
    if let Some(destination) = &opts.destination {
        args.push_str(&format!(" --to {destination:?}"));
    }
    if let Some(facility) = &opts.facility {
        args.push_str(&format!(" --facility {facility:?}"));
    }
    if let Some(level) = opts.level {
        args.push_str(&format!(" --level {level}"));
    }
    args
}
