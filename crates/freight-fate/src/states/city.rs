//! Terminal hub: dispatch board, garage, upgrades, trucks, and route
//! selection (port of `freight_fate/states/city.py`).
//!
//! The Python module was one 2000-line file; here it is split by screen:
//! [`CityMenuState`] lives in `city/terminal.rs`, the dispatch board and its
//! detail reader in `city/board.rs`, the terminal's Time and weather readout
//! in `city/weather.rs`, and the two small side menus (bobtail destination,
//! paying down a balance) in `city/extras.rs`. The free functions the tests
//! and the other screens reach for -- the cache key, opening the freight
//! market, the assigned reposition roll -- stay here, as do the re-exports
//! the Python module made (`from .city_pickup import ...`,
//! `from .city_business import ...`) so `states::city::X` resolves the way
//! `freight_fate.states.city.X` did.
//!
//! # Handing over the wheel
//!
//! Dispatch, the bobtail menu and the loaded departure all reach the road
//! through one [`launch_driving`], which builds the real `DrivingState`,
//! restores what the departure carries onto it, snapshots the trip, speaks
//! the departure line and pushes the drive. [`todo_state`] is what that
//! function used to push, and is kept only for a screen no port has claimed
//! yet; nothing in this module reaches for it now.

use serde_json::{Map, Value};

use ff_core::data::world::World;
use ff_core::data::world_models::{HomeTerminal, Route};
use ff_core::input_hints::TOUCH;
use ff_core::models::business::{is_owner_operator, COMPANY_DRIVER, INDEPENDENT_AUTHORITY};
use ff_core::models::enforcement;
use ff_core::models::jobs::relay::{relay_load, RelayRequest};
use ff_core::models::jobs::{
    board_offer_count, departure_deadline_hours, job_from_payload, job_payload,
    normalize_job_cities, plan_hos, Job, JobBoard, OfferOptions, ACTIVE_TRIP_FAIRNESS_SLACK,
};
use ff_core::models::profile::Profile;
use ff_core::models::start_options::option_for_profile;
use ff_core::music::crc32;
use ff_core::playtest_levers::{forced_dispatch_destination, resolve_city_forgiving};
use ff_core::pyfmt::{fmt_f, fmt_grouped, py_int, round_py_n};
use ff_core::sim::hos::limits;

use crate::app::{GameContext, Say};
use crate::states::base::{InputEvent, Key, Menu, MenuItem, SimpleMenuState};
use crate::states::driving::DrivingState;
use crate::states::main_menu::TouchPracticeOfferState;

mod board;
mod close_out;
mod extras;
mod held_load;
mod terminal;
mod truck_status;
mod weather;

pub use board::{
    describe_job, locked_reason, trailer_note, JobBoardState, JobDetailState, JOB_BOARD_INTRO_HELP,
};
pub use close_out::CloseOutCareerState;
pub use extras::{BobtailDestState, PayDebtState};
pub(crate) use terminal::local_zone as city_local_zone;
pub use terminal::CityMenuState;
pub use truck_status::TruckStatusState;

// `from .city_pickup import ...` / `from .city_business import ...`: the
// Python module re-exported these, and the tests import them from here.
pub use crate::states::city_business::{
    BusinessStatusState, EndorsementCourseState, TrailerProgramState, TruckShopState,
    UpgradeShopState,
};
pub use crate::states::city_garage::GarageState;
pub use crate::states::city_pickup::{
    job_origin_exists, pickup_snapshot, route_planning_summary, PickupFacilityState,
    RouteSelectState, PICKUP_CHECK_IN_MIN, PICKUP_LOADING_MIN,
};

// -- the loaded career --------------------------------------------------------------

/// `ctx.profile` where the Python read it unguarded: every screen under the
/// terminal runs with a career loaded, and reaching one without is the same
/// programming error an `AttributeError` on `None` was.
pub(crate) fn profile(ctx: &GameContext) -> &Profile {
    ctx.profile
        .as_ref()
        .expect("the terminal screens run with a loaded career")
}

pub(crate) fn profile_mut(ctx: &mut GameContext) -> &mut Profile {
    ctx.profile
        .as_mut()
        .expect("the terminal screens run with a loaded career")
}

/// The terminal the truck is parked at. An unknown current city is a data
/// bug the Python surfaced as `KeyError`; here it falls back to a plain
/// "Terminal" rather than taking the hub screen down.
pub(crate) fn home_terminal(ctx: &GameContext) -> HomeTerminal {
    let city = &profile(ctx).current_city;
    ctx.world
        .home_terminal(city)
        .unwrap_or_else(|_| HomeTerminal::new("Terminal", city, "", "yard"))
}

/// Python `str.capitalize()`: first character upper, the rest lower.
pub(crate) fn py_capitalize(text: &str) -> String {
    ff_core::data::world_models::py_capitalize(text)
}

/// First character upper, the rest as written: a sentence opening on a
/// street or city name keeps "US 83" and "Abilene" as they are spelled.
pub(crate) fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub(crate) fn record_city_duty(
    ctx: &mut GameContext,
    status: &str,
    start_hour: f64,
    end_hour: f64,
    note: &str,
) {
    if ctx.profile.is_none() {
        return;
    }
    let terminal = home_terminal(ctx);
    profile_mut(ctx)
        .duty_log
        .record(status, start_hour, end_hour, &terminal.name, note);
}

/// 10-hour sleeps required to cover `drive_h`, given the driving hours left
/// in the current shift and full-shift capacity after each sleep.
pub(crate) fn sleeps_needed(drive_h: f64, first_shift_h: f64, shift_h: f64) -> i64 {
    if drive_h <= first_shift_h + 1e-9 {
        return 0;
    }
    (((drive_h - first_shift_h) / shift_h - 1e-9).ceil() as i64).max(1)
}

// Empty-drive range for shopping another city's board.
pub const BOBTAIL_RANGE_MI: f64 = 400.0;

// Company drivers don't get to bobtail on a whim (that's the owner-operator
// menu item above); instead dispatch relays them a load from a nearby city
// when the board here is thin -- see `relay_load_for_board` and
// `ff_core::models::jobs::relay` for the rule and its thresholds.

// How long a manual "Save game" waits for its cloud backup result before
// handing the attempt back to the background retry. Long enough for a normal
// round trip, short enough that a dead network never holds the answer hostage.
pub const BACKUP_RESULT_WAIT_S: f64 = 10.0;

/// Has this driver taken their first load out?
///
/// Reads "first_day", the badge that is actually awarded. It used to read
/// "first_dispatch", which was folded INTO "first_day" at the pickup and has
/// not been awarded since -- so this answered false for everyone, forever,
/// and the board's recommended dispatch never appeared for any driver who was
/// not on a company training profile (found by audit, 2026-09-20).
pub fn first_dispatch_done(profile: &Profile) -> bool {
    profile.achievements.iter().any(|a| a == "first_day")
}

// Gated off the 1.9 release line (owner + Josh, 2026-07-27): the school is
// not finished and 1.9 is feature-frozen. The code stays -- reverting
// woven-in work mid-freeze invites regressions -- and the 2.0 line flips
// this flag to finish it properly.
pub const DRIVING_SCHOOL_ENABLED: bool = false;

pub fn first_day_guidance_active(profile: &Profile) -> bool {
    let deliveries = profile.career.deliveries;
    !first_dispatch_done(profile) && deliveries <= 0
}

pub fn first_day_orientation_message(ctx: &GameContext, prefix: &str) -> String {
    first_day_orientation_lines(ctx, prefix).join(" ")
}

/// The briefing one sentence per line, for the terminal's re-readable
/// screen; `first_day_orientation_message` is these joined.
pub fn first_day_orientation_lines(ctx: &GameContext, prefix: &str) -> Vec<String> {
    let p = profile(ctx);
    let terminal = home_terminal(ctx);
    let option = option_for_profile(p);
    // Spoken city, never the map key: the raw key read "parked at Chicago
    // Company Yard in the chicago_il_us service area" (found by the first
    // agent-driven playtest, 2026-08-30).
    let location = format!(
        "{} in {} service area",
        terminal.spoken_name(),
        ff_core::speech_text::the_city(&ctx.world.spoken_city(&p.current_city, None))
    );
    if option.is_owner_operator() {
        return vec![
            format!(
                "{prefix}First-day briefing: leased to {}, parked at {location}.",
                option.carrier_name
            ),
            format!(
                "You own a new truck with a full tank and {} dollars of working capital.",
                fmt_grouped(p.money(), 0)
            ),
            "Fuel, repairs, truck wear, trailer programs, and business reserves come out \
             of your cash."
                .to_string(),
            "Next: the dispatch board.".to_string(),
        ];
    }
    vec![
        format!(
            "{prefix}First-day briefing: welcome aboard {}.",
            option.carrier_name
        ),
        format!("Your assigned truck is parked at {location}."),
        "The carrier covers fuel, repairs, insurance, and trailer support.".to_string(),
        format!("Dispatch style: {}.", option.dispatch.summary()),
        "As a new hire, dispatch assigns your load and route, and refusing an assignment \
         goes on your service record."
            .to_string(),
        "Next: the dispatch board.".to_string(),
    ]
}

/// The refusal for any drive started from the terminal while the CDL is
/// suspended or disqualified, or `None` when the driver may drive.
pub(crate) fn cdl_drive_refusal(ctx: &GameContext) -> Option<String> {
    let p = ctx.profile.as_ref()?;
    let record = &p.driving_record;
    (record.lifetime_disqualified || record.suspended(p.game_hours))
        .then(|| ff_core::models::enforcement::suspension_drive_refusal_line(p))
}

// -- the dispatch board cache -------------------------------------------------------

/// The cache key a stored board is valid under: a JSON object, because it
/// is stored beside the board in the save and compared whole on reopen.
pub fn dispatch_cache_key(p: &Profile) -> Value {
    let mut endorsements: Vec<String> = p
        .career
        .endorsements()
        .into_iter()
        .map(str::to_string)
        .collect();
    endorsements.sort();
    let mut trailer_programs = p.trailer_programs.clone();
    trailer_programs.sort();
    let mut key = Map::new();
    key.insert("city".into(), Value::from(p.current_city.clone()));
    key.insert("market_day".into(), Value::from(p.market_day()));
    key.insert("market_seed".into(), Value::from(p.market.seed));
    key.insert("market_state_day".into(), Value::from(p.market.day));
    key.insert(
        "business_status".into(),
        Value::from(p.business_status.clone()),
    );
    key.insert("carrier_key".into(), Value::from(p.carrier_key.clone()));
    key.insert(
        "authority_readiness".into(),
        Value::from(p.authority_readiness),
    );
    key.insert("trailer_programs".into(), Value::from(trailer_programs));
    key.insert("level".into(), Value::from(p.career.level()));
    key.insert("endorsements".into(), Value::from(endorsements));
    key.insert(
        "count".into(),
        Value::from(board_offer_count(p.career.level()) as i64),
    );
    // A board cached before dispatch lost faith in you must not outlive
    // the trust that built it.
    key.insert("trust".into(), Value::from(enforcement::standing_band(p)));
    key.insert(
        "force_dest".into(),
        Value::from(forced_dispatch_destination()),
    );
    Value::Object(key)
}

/// Python `repr(sorted(key.items()))` for the cache key above: the string
/// the reposition roll is seeded from, so the same board rolls the same way
/// here as it did there.
fn py_repr_sorted_items(key: &Value) -> String {
    fn repr_str(s: &str) -> String {
        if s.contains('\'') && !s.contains('"') {
            format!("\"{s}\"")
        } else {
            format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
        }
    }
    fn repr_value(v: &Value) -> String {
        match v {
            Value::Bool(true) => "True".to_string(),
            Value::Bool(false) => "False".to_string(),
            Value::Null => "None".to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => repr_str(s),
            Value::Array(items) => {
                let parts: Vec<String> = items.iter().map(repr_value).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Object(map) => {
                let parts: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{}: {}", repr_str(k), repr_value(v)))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
        }
    }
    let Some(map) = key.as_object() else {
        return repr_value(key);
    };
    let mut items: Vec<(&String, &Value)> = map.iter().collect();
    items.sort_by(|a, b| a.0.cmp(b.0));
    let parts: Vec<String> = items
        .iter()
        .map(|(k, v)| format!("({}, {})", repr_str(k), repr_value(v)))
        .collect();
    format!("[{}]", parts.join(", "))
}

/// Open the dispatch board: restore the cached board when it is still
/// current, else build a fresh one and cache it. Pushes the board and
/// returns its jobs.
pub fn open_freight_market(ctx: &mut GameContext) -> Vec<Job> {
    let world = ctx.world;
    let (key, cache, hos) = {
        let p = profile_mut(ctx);
        let market_changed = p.market.advance_to(p.market_day());
        let key = dispatch_cache_key(p);
        let cache = if market_changed {
            None
        } else {
            p.dispatch_board_cache.clone()
        };
        (key, cache, p.hos.clone())
    };
    let seed = ctx.dispatch_board_seed;
    if let Some(seed) = seed {
        ctx.dispatch_board_seed = Some(seed.wrapping_add(1));
    }
    let mut board = JobBoard::new(world, seed, Some(&hos));
    let mut lever_note = String::new();
    let mut jobs: Option<Vec<Job>> = None;
    if let Some(cache) = cache.as_ref().and_then(Value::as_object) {
        if !cache.is_empty() && cache.get("key") == Some(&key) {
            // Cached payloads may predate the slug migration; normalize their
            // city references so a restored board keeps resolving.
            let restored: Option<Vec<Job>> = cache
                .get("jobs")
                .and_then(Value::as_array)
                .map(|payloads| {
                    payloads
                        .iter()
                        .map(|payload| {
                            let mut job = job_from_payload(payload.as_object()?)?;
                            normalize_job_cities(&mut job, world);
                            Some(job)
                        })
                        .collect::<Option<Vec<Job>>>()
                })
                .unwrap_or(Some(Vec::new()));
            // A board cached into the save can outlive the world it was built
            // from: an update that retires a pickup facility leaves an offer
            // nobody can be sent to, and accepting it is where that fails. One
            // stale offer retires the whole cached board.
            if let Some(restored) = restored {
                if restored.iter().all(|job| job_origin_exists(job, world)) {
                    jobs = Some(restored);
                }
            }
        }
    }
    let jobs = match jobs {
        Some(jobs) => jobs,
        None => {
            let mut fresh = {
                let p = profile(ctx);
                let endorsements: Vec<&str> = p.career.endorsements().into_iter().collect();
                board.offers(
                    &p.current_city,
                    &endorsements,
                    OfferOptions {
                        // How much freight dispatch will show you is a matter of
                        // trust: the band the dispatch trust line speaks, so a
                        // record or debt holding it down holds the board down
                        // too (owner, 2026-09-28).
                        count: enforcement::board_offers_for_band(
                            board_offer_count(p.career.level()) as i64,
                            enforcement::standing_band(p),
                        )
                        .max(0) as usize,
                        level: p.career.level(),
                        market: Some(&p.market),
                        carrier_key: Some(&p.carrier_key),
                        direct_freight: p.business_status == INDEPENDENT_AUTHORITY,
                    },
                )
            };
            let relay = relay_load_for_board(ctx, &key, &fresh);
            if let Some(relay) = relay {
                if !fresh.is_empty() {
                    // Replaces a slot rather than adding one: the board still shows
                    // exactly as many entries as the player's level and trust earn.
                    // The farthest ordinary offer goes -- the one least likely to be
                    // what a driver actually wanted -- and the list is re-sorted by
                    // distance the same way board.offers() leaves it.
                    let last = fresh.len() - 1;
                    fresh[last] = relay;
                    sort_by_distance(&mut fresh);
                } else {
                    fresh.push(relay);
                }
            }
            lever_note = add_forced_board_job(ctx, &mut board, &mut fresh);
            let mut cache = Map::new();
            if let Some(held) = held_load::held_load(profile(ctx), world) {
                held_load::carry_onto_board(&mut fresh, held, &mut cache);
            }
            let payloads: Vec<Value> = fresh
                .iter()
                .map(|job| Value::Object(job_payload(job)))
                .collect();
            cache.insert("key".into(), key.clone());
            cache.insert("jobs".into(), Value::Array(payloads));
            profile_mut(ctx).dispatch_board_cache = Some(Value::Object(cache));
            ctx.save_profile();
            fresh
        }
    };
    ctx.push_state(JobBoardState::new(ctx, jobs.clone()));
    if !lever_note.is_empty() {
        // Queued behind the board announcement, which interrupts.
        ctx.say_with(lever_note, Say::queued());
    }
    jobs
}

pub(crate) fn sort_by_distance(jobs: &mut [Job]) {
    jobs.sort_by(|a, b| {
        a.distance_mi
            .partial_cmp(&b.distance_mi)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// `(city key, miles, leg count)` for every city reachable from `city` on a
/// supported route: the board's own shared cache, so the relay and the
/// bobtail list never compute it twice.
pub(crate) fn board_candidates(world: &World, city: &str) -> Vec<(String, f64, usize)> {
    ff_core::models::jobs::reachable_cities(world, city)
}

/// Build the route lists a board in `city` needs on a background thread,
/// while the driver is still at the terminal: the first board in a new city
/// computed a route to every city on the map, then again from each relay
/// town, all on Enter (2026-09-28). The thread only fills shared caches, so
/// there is nothing to join; a board opened before it finishes computes what
/// it still needs itself.
pub(crate) fn warm_dispatch_board(world: &'static World, city: &str) {
    let city = city.to_string();
    let spawned = std::thread::Builder::new()
        .name("dispatch-warm".to_string())
        .spawn(move || ff_core::models::jobs::relay::warm_dispatch_routes(world, &city));
    if let Err(err) = spawned {
        log::warn!("Could not start the dispatch warm-up thread: {err}");
    }
}

/// The load dispatch relays onto a company driver's board when the board
/// here is thin (`ff_core::models::jobs::relay`): a load from one of the
/// nearest freight towns, its deadhead paid at the empty-mile rate (the
/// delivery clock starts at the shipper), offered as one assignment. None
/// when the board here is good enough, for an owner-operator (their own
/// "Bobtail to a nearby city" is how they reposition, on their own fuel),
/// and for a brand new hire, whose first dispatch is always freight from
/// this yard.
///
/// Seeded off the board's own cache key so the same board relays the same
/// load every time it is reopened, exactly like the rest of the cached
/// offers -- see dispatch_cache_key.
pub fn relay_load_for_board(ctx: &GameContext, key: &Value, local: &[Job]) -> Option<Job> {
    let p = profile(ctx);
    let status = if p.business_status.is_empty() {
        COMPANY_DRIVER
    } else {
        p.business_status.as_str()
    };
    if is_owner_operator(status) {
        return None;
    }
    if p.career.deliveries < 1 {
        // A brand-new hire's first dispatch is freight from this yard, never
        // a deadhead -- no carrier repositions a driver it has not yet put a
        // load behind, and every new-career flow stays deterministic.
        return None;
    }
    let world = ctx.world;
    let seed = i64::from(crc32(py_repr_sorted_items(key).as_bytes()));
    let mut nearby = board_candidates(world, &p.current_city);
    nearby.sort_by(|a, b| a.1.total_cmp(&b.1));
    let endorsements: Vec<&str> = p.career.endorsements().into_iter().collect();
    let request = RelayRequest {
        here: &p.current_city,
        endorsements: &endorsements,
        level: p.career.level(),
        market: Some(&p.market),
        carrier_key: Some(&p.carrier_key),
        direct_freight: p.business_status == INDEPENDENT_AUTHORITY,
        seed,
    };
    relay_load(world, &request, local, &nearby).map(|relay| relay.job)
}

/// FREIGHT_FATE_FORCE_DEST playtest lever: guarantee one load to the
/// forced destination on a freshly built board. Returns the spoken note.
fn add_forced_board_job(
    ctx: &GameContext,
    board: &mut JobBoard<'_>,
    jobs: &mut Vec<Job>,
) -> String {
    let dest = forced_dispatch_destination();
    if dest.is_empty() {
        return String::new();
    }
    let world = ctx.world;
    let p = profile(ctx);
    let key = resolve_city_forgiving(world, &dest);
    if !world.cities.contains_key(&key) {
        return format!("Playtest lever: no city called {dest} to dispatch to.");
    }
    if key == world.resolve_city_key(&p.current_city) {
        return String::new();
    }
    let spoken = world.spoken_city(&key, Some(true));
    if jobs
        .iter()
        .any(|job| world.resolve_city_key(&job.destination) == key)
    {
        return format!("Playtest lever: the board already offers {spoken}.");
    }
    let endorsements: Vec<&str> = p.career.endorsements().into_iter().collect();
    let job = board.offer_to(
        &p.current_city,
        &key,
        &endorsements,
        OfferOptions {
            count: 0,
            level: p.career.level(),
            market: Some(&p.market),
            carrier_key: Some(&p.carrier_key),
            direct_freight: p.business_status == INDEPENDENT_AUTHORITY,
        },
    );
    let Some(job) = job else {
        return format!("Playtest lever: no supported dispatch from here to {spoken}.");
    };
    jobs.push(job);
    sort_by_distance(jobs);
    format!("Playtest lever: added a load to {spoken} to the board.")
}

// -- hand-offs to screens other ports own ---------------------------------------------

/// A stand-in for a screen another port owns (main menu, settings, career
/// stats, the logbook, the setback notice, the driving school, the driving
/// state). One row, Escape pops it, so every flow through the terminal still
/// lands somewhere that speaks and can be left.
pub fn todo_state(name: &str) -> SimpleMenuState {
    SimpleMenuState::new(
        name,
        vec![
            MenuItem::new("Back", |s: &mut SimpleMenuState, ctx| s.go_back(ctx))
                .help("This screen arrives with its own port."),
        ],
    )
}

// The real constants, re-exported so the city screens' existing imports
// still resolve. They live with the drive that reads them.
pub use crate::states::driving_core::{DRIVE_PHASE_DELIVERY, DRIVE_PHASE_PICKUP};

/// What a loaded departure carries onto the road (`start_loaded_drive`):
/// the air-brake snapshot, the engine state, the speed-control session and
/// the refused trailer, restored onto the new `DrivingState` so the truck
/// leaves the dock exactly as it sat there.
#[derive(Debug, Clone, Default)]
pub struct LoadedDepartureResume {
    pub air_brake: Option<Value>,
    pub engine_on: bool,
    pub speed_control_armed: bool,
    pub speed_control_target_mph: Option<f64>,
    pub trailer_refused: bool,
    /// A team departure chosen at the shipper.
    pub team: Option<crate::states::driving_team::TeamMode>,
}

/// The line spoken (interrupting) once the trip snapshot is saved, just
/// before the driving state takes the screen.
pub enum LaunchAnnouncement {
    /// Spoken as-is.
    Line(String),
    /// `start_loaded_drive`: `lead` + `route_departure_summary` +
    /// `driving.trip.next_navigation_context()` + the engine tail; see
    /// [`loaded_departure_line`].
    LoadedDeparture { lead: String },
}

/// Everything dispatch hands the driving state: the Python
/// `DrivingState(ctx, job, route, phase=..., trip_seed=...)` call plus the
/// snapshot-and-announce that followed it at every call site.
pub struct DrivingLaunch {
    pub job: Job,
    pub route: Route,
    pub trip_seed: Option<i64>,
    /// `DRIVE_PHASE_PICKUP` for the deadhead to the shipper,
    /// `DRIVE_PHASE_DELIVERY` otherwise.
    pub phase: &'static str,
    pub start_hour: Option<f64>,
    /// Set only by `start_loaded_drive`.
    pub resume: Option<LoadedDepartureResume>,
    pub announcement: LaunchAnnouncement,
}

impl DrivingLaunch {
    pub fn new(
        job: Job,
        route: Route,
        phase: &'static str,
        announcement: LaunchAnnouncement,
    ) -> Self {
        DrivingLaunch {
            job,
            route,
            trip_seed: None,
            phase,
            start_hour: None,
            resume: None,
            announcement,
        }
    }
}

/// The departure line of `start_loaded_drive`: `lead`, the route summary,
/// the trip's first navigation context, and either "Departing now." or the
/// start-up instructions.
pub fn loaded_departure_line(
    ctx: &GameContext,
    lead: &str,
    route: &Route,
    engine_on: bool,
    next_context: &str,
) -> String {
    // Never "Departing now" over a dead engine: a driver who shut down at the
    // shipper leaves on a real start-up, and the line has to say so or the
    // truck simply sits there in silence.
    let tail = if engine_on {
        "Departing now.".to_string()
    } else {
        format!(
            "Engine off. {} starts it, {} releases the parking brake once air is up.",
            ctx.control_hint("engine"),
            ctx.control_hint("parking_brake")
        )
    };
    // The hours and tank before the road, so the driver can plan the stops
    // instead of meeting them as a countdown (player report, 2026-10-17).
    format!(
        "{lead}{} {} {next_context} {tail}",
        crate::states::city_pickup::route_departure_summary(route, &ctx.settings),
        board::briefing::departure_briefing(ctx, route, false)
    )
}

/// Hand the wheel to the driving state.
///
/// Python, at every call site: build the `DrivingState`, restore what
/// `resume` carries onto it, `profile.active_trip = driving.snapshot()`,
/// `save_profile()`, speak the announcement with `interrupt=True`, push the
/// state. The caller has already cleared `dispatch_board_cache` where the
/// Python did.
pub fn launch_driving(ctx: &mut GameContext, launch: DrivingLaunch) {
    let DrivingLaunch {
        mut job,
        route,
        trip_seed,
        phase,
        start_hour,
        resume,
        announcement,
    } = launch;
    // The board may have been cached before this shift's hours were spent,
    // and the pickup or deadhead can consume a duty window after acceptance.
    // Reconcile the deadline once, at the loaded departure, against the route
    // and legal hours the driver actually has. Resuming a snapshot does not
    // pass through this function, so a save cannot grant a fresh deadline.
    let deadline_note = if phase == DRIVE_PHASE_DELIVERY {
        let clock = limits(&ctx.settings.hos_mode)
            .is_some()
            .then(|| &profile(ctx).hos);
        let fair = departure_deadline_hours(
            &job,
            route.miles(),
            ACTIVE_TRIP_FAIRNESS_SLACK,
            Some(&route),
            Some(ctx.world),
            clock,
        );
        let fair = round_py_n(fair, 1);
        if fair > job.deadline_game_h {
            let extra_rest = !job.team
                && clock.is_some_and(|clock| {
                    plan_hos(route.miles(), Some(&route), Some(ctx.world), Some(clock)).sleeps
                        > plan_hos(route.miles(), Some(&route), Some(ctx.world), None).sleeps
                });
            job.deadline_game_h = fair;
            job.deadline_covers_rest |= extra_rest;
            // The departure briefing already names the sleep.
            format!(
                " Deadline extended to {} hours.",
                fmt_f(job.deadline_game_h, 1)
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    // The line needs the route, and the drive takes it by value, so build
    // what the summary reads before handing the route over.
    let route_for_line = route.clone();
    let mut driving = DrivingState::new(ctx, job, route, trip_seed, phase, start_hour);
    if let Some(resume) = &resume {
        // A trailer the driver refused at the shipper must not follow them
        // onto the road: the scale house has to find the box they are
        // actually pulling.
        driving.trailer_refused = resume.trailer_refused;
        driving
            .trip
            .truck
            .restore_air_brake_snapshot(resume.air_brake.as_ref().unwrap_or(&Value::Null), true);
        if resume.engine_on {
            driving.trip.truck.start_engine();
        }
        driving.restore_speed_control_session(
            ctx,
            resume.speed_control_armed,
            resume.speed_control_target_mph,
        );
        driving.pending_team = resume.team;
    }
    let snapshot = driving.snapshot(ctx);
    profile_mut(ctx).active_trip = Some(snapshot);
    ctx.save_profile();
    let line = match announcement {
        LaunchAnnouncement::Line(line) => line,
        LaunchAnnouncement::LoadedDeparture { lead } => {
            let engine_on = resume.as_ref().is_some_and(|r| r.engine_on);
            let next_context = driving
                .trip
                .next_navigation_context(ctx.settings.imperial_units);
            match resume.as_ref().and_then(|r| r.team) {
                // The co-driver starts the truck; the player hears who it is.
                Some(_) => {
                    let pairing =
                        crate::states::driving_team::handover::job_pairing(ctx, &driving.job);
                    let carrier = ff_core::models::career::carrier_name_of(profile(ctx));
                    // Your own hours and the tank, as on a solo departure,
                    // without the solo driver's sleep plan.
                    format!(
                        "{} {lead}{} {} {next_context} {} takes the wheel.",
                        ff_core::models::co_driver::pairing_line(&pairing, &carrier),
                        crate::states::city_pickup::route_departure_summary(
                            &route_for_line,
                            &ctx.settings
                        ),
                        board::briefing::departure_briefing(ctx, &route_for_line, true),
                        pairing.co_driver.name
                    )
                }
                None => {
                    loaded_departure_line(ctx, &lead, &route_for_line, engine_on, &next_context)
                }
            }
        }
    };
    ctx.say(&format!("{line}{deadline_note}"));
    ctx.push_state(driving);
    if ctx.controller.device() == TOUCH && !ctx.settings.touch_practice_offered {
        ctx.settings.touch_practice_offered = true;
        if let Err(e) = ctx.settings.save() {
            log::warn!("could not save touch practice offer setting: {e}");
        }
        ctx.push_state(TouchPracticeOfferState::new());
    }
}

// -- menu plumbing --------------------------------------------------------------------

/// The base `MenuState.handle_event`, for screens that take a key of their
/// own first (F1 on the dispatch board, W on route planning) and hand the
/// rest back. A trait default cannot be called from its own override, so the
/// fall-through lives here.
// TODO(lead): belongs in states::base::menu as a free function the Menu
// default delegates to, so an override can fall through without a copy.
pub(crate) fn base_menu_handle_event<S: Menu>(
    menu: &mut S,
    ctx: &mut GameContext,
    event: &InputEvent,
) {
    let Some((key, _mods, text)) = event.key_down() else {
        return;
    };
    match key {
        Key::Down => menu.move_by(ctx, 1),
        Key::Up => menu.move_by(ctx, -1),
        Key::Home => menu.jump(ctx, 0),
        Key::End => {
            let last = menu.menu().items.len().saturating_sub(1);
            menu.jump(ctx, last);
        }
        Key::Return | Key::Space | Key::KpEnter => menu.activate(ctx),
        Key::Escape => menu.go_back(ctx),
        Key::F1 => {
            let help = menu.current_help(ctx);
            ctx.say(&help);
        }
        Key::LCtrl | Key::RCtrl => ctx.stop_speech(),
        _ => {
            if let Some(ch) = text.filter(|ch| ch.is_alphanumeric()) {
                let lower: String = ch.to_lowercase().collect();
                menu.first_letter_input(ctx, &lower, event.key_repeat());
            }
        }
    }
}

/// The base `MenuState.enter`: rebuild the rows keeping the cursor, play
/// the open sound, announce. For screens whose `enter` does work first.
pub(crate) fn base_menu_enter<S: Menu>(menu: &mut S, ctx: &mut GameContext) {
    menu.refresh(ctx, true);
    if let Some(key) = menu.menu().open_sound_key.clone() {
        ctx.audio.play(&key);
    }
    menu.announce_entry(ctx);
}

/// The base `MenuState.current_help`, for screens that prefix it.
pub(crate) fn base_menu_current_help<S: Menu>(menu: &S, ctx: &GameContext) -> String {
    let core = menu.menu();
    if core.items.is_empty() {
        return core.intro_help.clone();
    }
    let item = &core.items[core.index];
    let help = item.help_text(menu, ctx);
    if help.is_empty() {
        format!("{}.", item.text(menu, ctx))
    } else {
        help
    }
}

/// Python `int(x)` for a game-hours float, for the weather seed.
pub(crate) fn game_hours_int(hours: f64) -> i64 {
    py_int(hours)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeps_needed_counts_whole_rests() {
        assert_eq!(sleeps_needed(5.0, 11.0, 11.0), 0);
        assert_eq!(sleeps_needed(12.0, 11.0, 11.0), 1);
        assert_eq!(sleeps_needed(25.0, 3.0, 11.0), 2);
    }

    #[test]
    fn cache_key_repr_matches_python() {
        let mut key = Map::new();
        key.insert("city".into(), Value::from("chicago_il_us"));
        key.insert("authority_readiness".into(), Value::from(false));
        key.insert("endorsements".into(), Value::Array(vec![]));
        key.insert("level".into(), Value::from(1));
        assert_eq!(
            py_repr_sorted_items(&Value::Object(key)),
            "[('authority_readiness', False), ('city', 'chicago_il_us'), ('endorsements', []), ('level', 1)]"
        );
    }
}
