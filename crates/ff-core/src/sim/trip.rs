//! Trip simulation: progress along a route, grades, zones, stops, and events
//! (port of `freight_fate/sim/trip.py`).
//!
//! Python's `Trip` mixed in `TripRoadEventMixin`, `TripTrafficMixin` and
//! `EnforcementPostMixin`; here it is ONE struct defined in this file with
//! every field any of them touched, and `impl Trip` blocks in
//! `trip_road_events`, `trip_traffic` and `enforcement_posts` for the former
//! mixin methods. This file holds the struct, its construction and the clock;
//! the rest of `trip.py` is split by section into the `trip/` submodules
//! (`lookups`, `placement`, `place_time`, `zones`, `limits`, `streets`,
//! `update`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::data::curves::RouteCurve;
use crate::data::world::{get_world, World};
use crate::data::world_models::{City, Route};
use crate::pyfmt::round_py_int;
use crate::pyrandom::PyRandom;
use crate::sim::enforcement_posts::EnforcementPost;
use crate::sim::road_event_pacing::RoadEventBreather;
use crate::sim::timezones::{zone_for, HasLocation, TimeZone};
use crate::sim::traffic_manager::TrafficManager;
use crate::sim::trip_models::*;
use crate::sim::trip_traffic::TrafficProvider;
use crate::sim::truck_parking::TruckParkingProvider;
use crate::sim::vehicle::TruckState;
use crate::sim::weather::WeatherSystem;
use crate::units::{distance_unit, spoken_distance, spoken_gap, to_distance};

mod exit_ramps;
mod limits;
mod lookups;
mod place_time;
mod placement;
mod streets;
mod update;
mod zones;

pub use lookups::LaneRun;
pub use streets::{
    is_gate_zone_reason, is_street_zone_reason, spoken_zone, LOT_ZONE, STOP_STREET_ZONE,
    STREET_ZONE, YARD_ZONE,
};

/// A stop is announced ("stop ahead") when it first comes within this many
/// miles ahead; `restore` seeds this SAME window as already-announced so a
/// resumed trip does not re-announce a stop called out before the save.
pub const STOP_AHEAD_LOOKAHEAD_MI: f64 = 5.0;
/// Street maneuvers announce at block scale, not highway scale.
pub const LOCAL_TURN_LOOKAHEAD_MI: f64 = 0.3;
/// A lane-count run shorter than this is collapsed into its neighbor.
pub const LANE_RUN_MIN_MI: f64 = 2.0;

// The reaction allowance covers hearing the call out loud, orienting by ear,
// and moving a foot to the brake -- audio-first reaction is slower than a
// sighted glance at a sign. Retuned from the owner's AZ-260 run (2026-07-19).
pub const PACENOTE_REACTION_S: f64 = 8.0;
pub const PACENOTE_BRAKE_MPH_PER_S: f64 = 2.5;
pub const PACENOTE_MARGIN_MPH: f64 = 3.0;
pub const PACENOTE_GENTLE_MARGIN_MPH: f64 = 8.0;
pub const PACENOTE_MIN_LEAD_MI: f64 = 0.33;
pub const PACENOTE_MAX_LEAD_MI: f64 = 1.5;
/// Never call a curve with less than this many seconds of travel at the
/// current speed.
pub const PACENOTE_LEAD_FLOOR_S: f64 = 30.0;
/// A follower starting within this gap after a called curve rides that
/// call's "then left/right" tail INSTEAD of getting its own call.
pub const PACENOTE_LINK_GAP_MI: f64 = 0.3;

/// A signaled exit gets the same real-time treatment as a hard bend, over
/// the road the truck genuinely needs to reach ramp speed, widened so the
/// clock is already real by the time anything has to start shedding.
pub const EXIT_APPROACH_DECOMPRESS_SLACK: f64 = 1.5;
/// And pacing climbs back over this many real seconds afterwards.
pub const EXIT_APPROACH_RELEASE_S: f64 = 3.0;

/// Only posted drops of at least this size get a warning.
pub const LIMIT_DROP_WARN_MIN_DELTA_MPH: f64 = 10.0;
/// The "drops to X in ..." pacenote lead, sized in REAL seconds at the
/// current pace (owner's live playtest, 2026-08-12).
pub const LIMIT_WARNING_REAL_S: f64 = 18.0;
pub const LIMIT_WARNING_MAX_LEAD_MI: f64 = 5.0;
/// A newly entered lower limit that ends within this span has its length
/// spoken ("for the next half a mile").
pub const LIMIT_SHORT_ZONE_MI: f64 = 2.5;
/// Below this there is no warning to give: the zone is already underfoot
/// (owner playtest, 2026-08-17).
pub const ZONE_WARNING_MIN_MI: f64 = 0.1;
/// A navigation lead closer than this is the near announcement's own moment.
pub const NAV_LEAD_MIN_MI: f64 = 0.1;
pub const LIMIT_SCAN_STRIDE_MI: f64 = 0.1;
pub const LIMIT_SCAN_MAX_MI: f64 = 3.0;

/// Why a drop is happening, for the arrival line only.
pub const LIMIT_REASON_LOOKAHEAD_MI: f64 = 1.5;
/// The only RoadStop types that actually cause a lower posting.
pub fn limit_reason_by_stop_type(stop_type: &str) -> Option<&'static str> {
    match stop_type {
        "weigh_station" => Some(" for the weigh station ahead"),
        _ => None,
    }
}
/// A "real hill" by the grade advisory's bar, checked against the average
/// over a forward scan rather than a single sample.
pub const LIMIT_DOWNGRADE_PCT: f64 = -3.5;
pub const LIMIT_DOWNGRADE_MIN_MI: f64 = 0.5;

/// below this the truck is parked or crawling: estimate at highway pace
pub const ETA_MIN_MPH: f64 = 15.0;

/// Colloquial short distance, mirroring `Settings.short_distance_text`
/// (quarter-mile steps, 100-meter steps) so the co-driver's limit calls
/// sound like her curve calls in either unit.
pub fn spoken_short_miles(miles: f64, imperial: bool) -> String {
    if imperial {
        if miles > 1.125 {
            return spoken_distance(miles, "mile");
        }
        let quarters = 1.max(round_py_int(miles * 4.0));
        return match quarters {
            1 => "a quarter mile".to_string(),
            2 => "half a mile".to_string(),
            3 => "three quarters of a mile".to_string(),
            4 => "one mile".to_string(),
            _ => spoken_distance(miles, "mile"),
        };
    }
    let km = miles * 1.609344;
    if km >= 0.95 {
        return spoken_distance(km, "kilometer");
    }
    let meters = 1.max(round_py_int(km * 10.0)) * 100;
    format!("{meters} meters")
}

/// Turn direction for the earcon, read out of a baked maneuver cue;
/// directionless legacy cues ("Turn onto") return "" and stay speech-only.
pub fn cue_direction(text: &str) -> &'static str {
    let lowered = text.to_lowercase();
    if lowered.contains("left") {
        return "left";
    }
    if lowered.contains("right") {
        return "right";
    }
    if lowered.starts_with("continue") || lowered.starts_with("start") {
        return "ahead";
    }
    ""
}

impl HasLocation for City {
    fn lat(&self) -> f64 {
        self.lat
    }
    fn lon(&self) -> f64 {
        self.lon
    }
    fn state(&self) -> &str {
        &self.state
    }
}

/// The keyword arguments of Python's `Trip(...)`, with the same defaults.
pub struct TripOptions {
    pub time_scale: f64,
    pub seed: Option<i64>,
    pub start_hour: f64,
    pub imperial: bool,
    pub hazard_scale: f64,
    pub career_hours: Option<f64>,
    pub traffic_provider: Option<Arc<dyn TrafficProvider>>,
    pub parking_provider: Option<Arc<TruckParkingProvider>>,
    pub bobtail: bool,
    pub destination_label: String,
    pub destination_approach_mi: Option<f64>,
    pub local_state: String,
    pub outbound: bool,
    /// The streets from an exit to a road stop's lot, not to a facility.
    pub road_stop: bool,
    /// The world the route names its cities in; the session world when None.
    pub world: Option<&'static World>,
}

impl Default for TripOptions {
    fn default() -> Self {
        TripOptions {
            time_scale: 20.0,
            seed: None,
            start_hour: 12.0,
            imperial: true,
            hazard_scale: 1.0,
            career_hours: None,
            traffic_provider: None,
            parking_provider: None,
            bobtail: false,
            destination_label: String::new(),
            destination_approach_mi: None,
            local_state: String::new(),
            outbound: false,
            road_stop: false,
            world: None,
        }
    }
}

impl TripOptions {
    /// `Trip(route, truck, weather, seed=seed)`: the commonest test shape.
    pub fn seeded(seed: i64) -> Self {
        TripOptions {
            seed: Some(seed),
            ..Default::default()
        }
    }
}

/// One delivery run along a chosen route.
pub struct Trip {
    pub route: Route,
    pub truck: TruckState,
    pub weather: WeatherSystem,
    pub weather_source_status: &'static str,
    pub weather_location_refreshing: bool,
    pub weather_refresh_issue_announced: bool,
    pub time_scale: f64,
    pub hazard_scale: f64,
    /// clock hour of day at departure
    pub start_hour: f64,
    /// Absolute career clock at departure: carries the day of the week so
    /// commuter rush hour only forms on weekdays. None reads as a weekday.
    pub career_hours: Option<f64>,
    imperial: bool,
    pub traffic_provider: Option<Arc<dyn TrafficProvider>>,
    pub parking_provider: Option<Arc<TruckParkingProvider>>,
    /// Running tractor-only opens stops a combination vehicle cannot enter.
    pub bobtail: bool,
    /// On a facility-approach route the spoken facility name replaces the
    /// city in the status line.
    pub destination_label: String,
    /// Local approach road between the highway and this run's gate, from the
    /// destination facility's own approach record.
    pub destination_approach_mi: Option<f64>,
    /// Which state's vehicle code governs this run's local streets.
    pub local_state: String,
    /// Which END of a facility street chain the gate is at: driven outbound
    /// it is the FIRST thing you pass.
    pub outbound: bool,
    /// The streets from an exit to a road stop's lot: its zones speak of an
    /// access road and a lot rather than a facility's road and yard.
    pub road_stop: bool,
    pub position_mi: f64,
    pub game_minutes: f64,
    /// Diesel burned on this run, in gallons. Observed as a per-frame DROP in
    /// the tank rather than plumbed out of the burn model, so a refuel stop
    /// mid-run adds fuel without ever subtracting from what was spent.
    pub fuel_used_gal: f64,
    /// Last tank level seen, for the line above.
    fuel_seen_gal: Option<f64>,
    /// Real seconds spent in trip.update: the sitting budget chatter reads.
    pub sitting_s: f64,
    /// When the last billboard / flavor landmark was generated, on sitting_s.
    pub last_chatter_s: Option<f64>,
    pub finished: bool,
    /// The control the stop callout names for signalling an exit.
    pub exit_hint: String,
    /// Facilities named in full already this leg (research doc R6).
    pub facilities_named: HashSet<String>,
    pub facility_leg: usize,
    /// Deliberate waiting: armed when the player sets the parking brake.
    pub waiting: bool,
    /// set by the UI layer; gates inspections
    pub hos_violation: bool,
    /// How much more often than a clean driver a routine roadside inspection
    /// stops this truck (`roadside_inspection::roadside_inspection_scale`):
    /// set by the UI layer from the safety record, the hours mode and the
    /// calendar; zero switches the routine stops off.
    pub roadside_inspection_scale: f64,
    /// The career clock is inside CVSA's Roadcheck blitz: the CB says so
    /// once, early in the run. Set by the UI layer with the scale above.
    pub roadcheck_blitz: bool,
    pub seed: Option<i64>,
    pub rng: PyRandom,
    pub insp_rng: PyRandom,
    pub cond_rng: PyRandom,
    /// The day-to-day traffic scatter, on a stream of its own so that adding
    /// it does not move where the work zones land for a given seed.
    pub traffic_rng: PyRandom,
    pub events: Vec<TripEvent>,
    pub leg_starts: Vec<f64>,
    pub city_mileposts: Vec<f64>,
    pub start_timezone: TimeZone,
    pub timezone_crossings: Vec<TimezoneCrossing>,
    /// The zone last announced (Python `_current_timezone`).
    pub last_timezone: TimeZone,
    pub stops: Vec<RoadStop>,
    pub toll_charges: Vec<TollCharge>,
    pub traffic_manager: TrafficManager,
    pub zones: Vec<Zone>,
    /// The National Weather Service warnings active where the truck is,
    /// as the cab last read them; empty with real weather off.
    pub live_alerts: Vec<crate::sim::real_weather_alerts::WeatherAlert>,
    pub traffic_pressures: Vec<TrafficPressure>,
    pub navigation_cues: Vec<NavigationCue>,
    pub landmarks: Vec<RoadsideCallout>,
    pub billboards: Vec<RoadsideCallout>,
    pub chain_law_areas: Vec<(f64, f64)>,
    pub posts: Vec<EnforcementPost>,
    pub curves: Vec<RouteCurve>,
    /// True while the player is on an exit ramp that ends in a light or a
    /// stop sign; pins the clock to real time.
    pub controlled_ramp: bool,
    /// True while the ramp being driven ends at the delivery destination.
    pub dock_run_in: bool,
    /// A police stop is in progress: the clock stops compressing.
    pub pull_over_active: bool,
    /// True from a street corner's brake point until the corner resolves:
    /// the clock is real time.
    pub controlled_turn: bool,
    /// 0 to 1: how far the clock has eased toward real time on the approach
    /// to a corner's brake point. Set every frame by the driving state; 0
    /// means the trip's own pacing.
    pub turn_clock: f64,
    /// True while curve assistance is still taking speed off for a bend.
    pub curve_shed_active: bool,
    /// How the lane work is shared, set by the game from the driver's lane
    /// keeping (`TruckState::curve_safe_mph`): None with it automated. The
    /// curve call prices a bend by it.
    pub lane_steers: Option<bool>,
    /// Road left to an exit the driver has signalled for.
    pub exit_approach_mi: Option<f64>,
    /// The milepost of an open weigh station whose last reminder has been
    /// spoken. From that line to the gore the clock runs real, so the
    /// reminder buys the driver real seconds to signal at any pacing. Set by
    /// the game; past the milepost, or on a ramp, it holds nothing.
    pub scale_reminder_hold_mi: Option<f64>,
    pub exit_approach_release_s: f64,
    pub announced_chain_law: HashSet<String>,
    pub announced_curves: HashSet<String>,
    pub announced_lane_changes: HashSet<String>,
    /// lazily built, direction-aware
    pub lane_runs: Option<Vec<LaneRun>>,
    pub announced_landmarks: HashSet<String>,
    pub announced_billboards: HashSet<String>,
    /// RoadStop keys, never names.
    pub announced_stops: HashSet<String>,
    pub planned_stop_key: Option<String>,
    /// RoadStop key of the stop whose exit is currently signaled or being
    /// descended, published each tick by the driving state.
    pub exit_in_progress: Option<String>,
    /// While on an exit ramp the truck is off the highway: the mile marker
    /// holds and highway events pause.
    pub on_ramp: bool,
    /// The grade under the truck on the ramp proper, published each tick by
    /// the driving state; None reads the mainline's, which is right for the
    /// deceleration lane running beside it. Honoured only while `on_ramp`.
    pub ramp_grade: Option<f64>,
    pub last_moved_mi: f64,
    pub announced_cities: HashSet<usize>,
    pub announced_navigation: HashSet<String>,
    pub charged_tolls: HashSet<String>,
    /// The zone the truck was last known to be inside (Python
    /// `_active_zone`); compared by `zone_key`, the Rust stand-in for
    /// Python's object identity.
    pub entered_zone: Option<Zone>,
    /// Whether the current entered zone's ZONE_ENTER colour line has been
    /// spoken.
    pub zone_entry_spoken: bool,
    /// Set by the drive while the turn into the yard or lot is still ahead
    /// or unsettled: the yard's entry line waits for the truck to have
    /// actually turned in, so a truck that overruns the driveway is not told
    /// it is in the yard before it is told it missed the turn.
    pub yard_entry_held: bool,
    pub announced_speed_limit: Option<f64>,
    /// Where a lower posting the truck passed too fast to be told about
    /// ends. Until then no speed is charged against it.
    pub unannounced_limit_end_mi: Option<f64>,
    pub warned_limit_drops: Vec<f64>,
    /// Posted-limit values already spoken for the CURRENT posting.
    pub limit_drop_preannounced: Vec<f64>,
    pub event_breather: RoadEventBreather,
    pub announced_zone_warnings: HashSet<String>,
    /// Milepost of the zone the driver was last warned about.
    pub pending_zone_warning: Option<f64>,
    pub announced_traffic_pressures: HashSet<String>,
    pub announced_npc_traffic: HashSet<String>,
    pub announced_real_traffic: HashSet<String>,
    pub next_real_traffic_check_mi: f64,
    pub construction_zone_grace_start: HashMap<String, f64>,
    /// CB heads-ups are rationed to CB_CALLS_PER_RUN.
    pub cb_calls_made: usize,
    /// Posts that have already entered the lead window.
    pub heads_up_seen: HashSet<String>,
    pub hazard_check_mi: f64,
    pub inspection_check_mi: f64,
    pub conditions_check_mi: f64,
    pub traffic_warning_mi: f64,
    pub announced_enforcement: HashSet<String>,
    /// state name and code -> two-letter code, from the world's cities
    /// (Python built this lazily in `_region_at`).
    state_codes: HashMap<String, String>,
    pub world: &'static World,
}

impl Trip {
    pub fn new(
        route: Route,
        truck: TruckState,
        mut weather: WeatherSystem,
        opts: TripOptions,
    ) -> Self {
        let world = opts.world.unwrap_or_else(get_world);
        let weather_source_status = weather.source_status();
        let seed = opts.seed;
        let make_rng = |xor: Option<i64>| match seed {
            None => PyRandom::new_unseeded(),
            Some(s) => PyRandom::new_from_i64(match xor {
                None => s,
                Some(x) => s ^ x,
            }),
        };
        let leg_starts = compute_leg_starts(&route);
        let mut city_mileposts = leg_starts.clone();
        city_mileposts.push(route.miles());
        let mut state_codes = HashMap::new();
        for city in world.cities.values() {
            for name in [&city.state, &city.state_code] {
                if !name.is_empty() {
                    state_codes.insert(name.clone(), city.state_code.clone());
                }
            }
        }
        let start_hour = opts.start_hour;
        let hazard_scale = opts.hazard_scale.max(0.0);
        let traffic_manager = TrafficManager::new(
            &route,
            &leg_starts,
            seed,
            start_hour,
            hazard_scale,
            opts.imperial,
            truck.speed_mph(),
            weather.effects(),
        );
        let mut trip = Trip {
            route,
            truck,
            weather,
            weather_source_status,
            weather_location_refreshing: false,
            weather_refresh_issue_announced: false,
            time_scale: opts.time_scale,
            hazard_scale,
            start_hour,
            career_hours: opts.career_hours,
            imperial: opts.imperial,
            traffic_provider: opts.traffic_provider,
            parking_provider: opts.parking_provider,
            bobtail: opts.bobtail,
            destination_label: opts.destination_label,
            destination_approach_mi: opts.destination_approach_mi,
            local_state: opts.local_state,
            outbound: opts.outbound,
            road_stop: opts.road_stop,
            position_mi: 0.0,
            game_minutes: 0.0,
            fuel_used_gal: 0.0,
            fuel_seen_gal: None,
            sitting_s: 0.0,
            last_chatter_s: None,
            finished: false,
            exit_hint: "X".to_string(),
            facilities_named: HashSet::new(),
            facility_leg: 0,
            waiting: false,
            hos_violation: false,
            roadside_inspection_scale: 1.0,
            roadcheck_blitz: false,
            seed,
            rng: make_rng(None),
            insp_rng: make_rng(Some(0x5EED)),
            cond_rng: make_rng(Some(0xC0FFEE)),
            traffic_rng: make_rng(Some(0x7A4FF1C)),
            events: Vec::new(),
            leg_starts,
            city_mileposts,
            start_timezone: zone_for(0.0, 0.0, ""),
            timezone_crossings: Vec::new(),
            last_timezone: zone_for(0.0, 0.0, ""),
            stops: Vec::new(),
            toll_charges: Vec::new(),
            traffic_manager,
            zones: Vec::new(),
            live_alerts: Vec::new(),
            traffic_pressures: Vec::new(),
            navigation_cues: Vec::new(),
            landmarks: Vec::new(),
            billboards: Vec::new(),
            chain_law_areas: Vec::new(),
            posts: Vec::new(),
            curves: Vec::new(),
            controlled_ramp: false,
            dock_run_in: false,
            pull_over_active: false,
            controlled_turn: false,
            turn_clock: 0.0,
            curve_shed_active: false,
            lane_steers: None,
            exit_approach_mi: None,
            scale_reminder_hold_mi: None,
            exit_approach_release_s: 0.0,
            announced_chain_law: HashSet::new(),
            announced_curves: HashSet::new(),
            announced_lane_changes: HashSet::new(),
            lane_runs: None,
            announced_landmarks: HashSet::new(),
            announced_billboards: HashSet::new(),
            announced_stops: HashSet::new(),
            planned_stop_key: None,
            exit_in_progress: None,
            on_ramp: false,
            ramp_grade: None,
            last_moved_mi: 0.0,
            announced_cities: HashSet::new(),
            announced_navigation: HashSet::new(),
            charged_tolls: HashSet::new(),
            entered_zone: None,
            zone_entry_spoken: true,
            yard_entry_held: false,
            announced_speed_limit: None,
            unannounced_limit_end_mi: None,
            warned_limit_drops: Vec::new(),
            limit_drop_preannounced: Vec::new(),
            event_breather: RoadEventBreather::new(),
            announced_zone_warnings: HashSet::new(),
            pending_zone_warning: None,
            announced_traffic_pressures: HashSet::new(),
            announced_npc_traffic: HashSet::new(),
            announced_real_traffic: HashSet::new(),
            next_real_traffic_check_mi: 0.0,
            construction_zone_grace_start: HashMap::new(),
            cb_calls_made: 0,
            heads_up_seen: HashSet::new(),
            hazard_check_mi: 5.0,
            inspection_check_mi: 10.0,
            conditions_check_mi: CONDITIONS_CHECK_MI,
            traffic_warning_mi: 1.0,
            announced_enforcement: HashSet::new(),
            state_codes,
            world,
        };
        // The same order as the Python constructor: every seeded draw below
        // happens in this sequence.
        let (start_timezone, crossings) = trip.compute_timezone_crossings();
        trip.start_timezone = start_timezone;
        trip.timezone_crossings = crossings;
        trip.last_timezone = start_timezone;
        trip.stops = trip.place_stops();
        trip.traffic_manager.spawn_initial_traffic();
        trip.zones = trip.place_zones();
        trip.traffic_pressures = trip.place_traffic_pressures();
        trip.navigation_cues = trip.build_navigation_cues();
        trip.landmarks = trip.place_landmarks();
        trip.billboards = trip.place_billboards();
        trip.chain_law_areas = trip.place_chain_law_areas();
        // Enforcement posts read the zones, the scales, the chain controls and
        // the city mileposts, so they are placed after all of them.
        trip.posts = trip.place_enforcement_posts();
        let posts = trip.posts.clone();
        trip.traffic_manager.add_enforcement_traffic(&posts);
        trip.curves = trip.place_curves();
        if trip.outbound {
            // Pulling OUT of a yard starts the truck already standing inside
            // the gate zone, and you do not "enter" a zone you begin in
            // (owner, 2026-08-21).
            trip.entered_zone = trip.active_zone_at(0.0);
        }
        // Start the first route cell's live-weather fetch now rather than on
        // the first update tick. Opportunistic only: a route whose cities are
        // not in the world cannot resolve a location yet.
        if let Some((weather_key, lat, lon)) = trip.weather_location() {
            trip.weather.set_city(&weather_key, lat, lon);
            if let Some(provider) = trip.weather.provider_mut() {
                provider.request(&weather_key, lat, lon);
            }
        }
        trip
    }

    /// The enforcement posts under the older name (the info key, the
    /// road-ahead readout and the traffic bubble still ask for patrols).
    pub fn patrols(&self) -> &[EnforcementPost] {
        &self.posts
    }

    pub fn set_patrols(&mut self, value: Vec<EnforcementPost>) {
        self.posts = value;
    }

    /// Clock compression for this frame: gentle while maneuvering, the full
    /// configured pacing at highway speed, and double pacing while parked
    /// with the brake set. Everything that converts real seconds to game
    /// time must read this, never `time_scale`.
    pub fn effective_time_scale(&self) -> f64 {
        let full = self.time_scale;
        if self.waiting && self.truck.parking_brake && self.truck.speed_mph() < 1.0 {
            // Deliberate waiting fast-forwards the compressed pacings. Real
            // time promises the wall clock, parked included.
            return if full > 1.0 {
                full * PARKED_TIME_SCALE_MULT
            } else {
                full
            };
        }
        if self.real_time_override().is_some() {
            return full.min(1.0);
        }
        if self.exit_approach_release_s > 0.0 {
            // Coming back up to pace after an approach, not snapping to it.
            let real = full.min(1.0);
            let eased = 1.0 - self.exit_approach_release_s / EXIT_APPROACH_RELEASE_S;
            return real + (full - real) * eased;
        }
        let floor = LOW_SPEED_TIME_SCALE.min(full);
        let ramp = (self.truck.speed_mph() / FULL_COMPRESSION_MPH).min(1.0);
        let paced = floor + (full - floor) * ramp;
        // Easing into a corner's brake point, the mirror of the exit release
        // above: the clock slides down to real time rather than dropping to it.
        let real = full.min(1.0);
        let toward_real = self.turn_clock.clamp(0.0, 1.0);
        paced + (real - paced) * toward_real
    }

    /// Why the clock is pinned to real time this frame, or None when it is
    /// paced. `effective_time_scale` answers `full.min(1.0)` for every one
    /// of these, so the name is what a session log says when a tester
    /// reports the clock stuck at real time (issue 293). Parked waiting is
    /// checked before this in `effective_time_scale` and is not a pin.
    pub fn real_time_override(&self) -> Option<&'static str> {
        if self.pull_over_active {
            // Lights behind you: the whole encounter runs on the real clock.
            return Some("police stop");
        }
        if self.controlled_ramp {
            // A ramp ending in a light or a sign, the acceleration lane and
            // the merge after it: the warning must buy human reaction
            // seconds, not compressed ones. The driving state names which.
            return Some("controlled ramp");
        }
        if self.controlled_turn {
            // A street corner, from its brake point in.
            return Some("street corner");
        }
        if self.dock_run_in {
            // The run-in to the dock.
            return Some("dock run-in");
        }
        if self.severe_curve_decompression() {
            // Same law for a hard bend (owner, 2026-07-24).
            return Some("hard bend");
        }
        if self.curve_shed_active {
            // And while curve assistance is still shedding for one. The law
            // above lets go at the advisory plus the pacenote margin; the
            // assist aims at the advisory itself, so the last three miles an
            // hour were shed on the compressed clock, where the road left
            // passes seventeen times faster than the truck slows and the
            // only profile that still lands on the number is a full
            // application (AZ-260 bench trace, 2026-09-18: 0.35 of the
            // pedal one frame, all of it the next, to take off 3 mph).
            return Some("curve assist shedding speed");
        }
        if self.armed_exit_decompression() {
            // And for a signaled exit (Shane, 2026-08-15).
            return Some("signaled exit");
        }
        if self.scale_reminder_hold() && self.truck.speed_mph() > SCALE_BYPASS_MPH {
            // And from an open scale's last reminder to its gore: at full
            // compression the fixed half mile went by in three real seconds,
            // the bypass charge landing before "Signal for the scale exit"
            // had finished (tester log, 2026-10-07). Only while a crossing
            // could still be charged: at or under the bypass speed it cannot,
            // so a truck slowed or stopped short of the scale runs the
            // ordinary low-speed pacing rather than sitting on the wall clock
            // until it reaches the gore. Back over that speed, the pin is
            // back before the crossing can count, and the reminder's real
            // seconds are counted on the real clock either way.
            return Some("scale reminder");
        }
        None
    }

    /// Whether an open scale's reminder hold is armed: from the reminder to
    /// the gore, off the scale's ramp. `real_time_override` pins the clock
    /// for it only above [`SCALE_BYPASS_MPH`].
    pub fn scale_reminder_hold(&self) -> bool {
        !self.on_ramp
            && self
                .scale_reminder_hold_mi
                .is_some_and(|scale_mi| self.position_mi < scale_mi)
    }

    pub fn imperial(&self) -> bool {
        self.imperial
    }

    /// Python's `imperial` setter: the manager follows and the navigation
    /// cues are rebuilt in the new units.
    pub fn set_imperial(&mut self, value: bool) {
        if value == self.imperial {
            return;
        }
        self.imperial = value;
        self.traffic_manager.imperial = value;
        self.navigation_cues = self.build_navigation_cues();
    }

    pub fn npc_vehicles(&self) -> &[crate::sim::traffic_manager::TrafficVehicle] {
        &self.traffic_manager.vehicles
    }

    pub fn set_npc_vehicles(&mut self, vehicles: Vec<crate::sim::traffic_manager::TrafficVehicle>) {
        self.traffic_manager.vehicles = vehicles;
    }

    pub fn distance_text(&self, miles: f64) -> String {
        spoken_distance(
            to_distance(miles, self.imperial),
            distance_unit(self.imperial, false),
        )
    }

    /// How far to something still in front of the truck, never "0 miles":
    /// quarter-mile steps, or hundred-metre steps in metric (owner playtest,
    /// 2026-08-17).
    pub fn ahead_text(&self, miles: f64) -> String {
        spoken_short_miles(miles, self.imperial)
    }

    pub fn gap_text(&self, miles: f64) -> String {
        spoken_gap(miles, self.imperial)
    }

    pub fn speed_value(&self, mph: f64) -> String {
        crate::pyfmt::fmt_f(to_distance(mph, self.imperial), 0)
    }

    pub fn speed_text(&self, mph: f64) -> String {
        let units = if self.imperial {
            "miles per hour"
        } else {
            "kilometers per hour"
        };
        format!("{} {units}", self.speed_value(mph))
    }

    pub fn total_miles(&self) -> f64 {
        self.route.miles()
    }

    pub fn remaining_miles(&self) -> f64 {
        (self.total_miles() - self.position_mi).max(0.0)
    }

    pub fn current_hour(&self) -> f64 {
        (self.start_hour + self.game_minutes / 60.0).rem_euclid(24.0)
    }

    pub fn current_leg_index(&self) -> usize {
        for i in (0..self.route.legs.len()).rev() {
            if self.position_mi >= self.leg_starts[i] {
                return i;
            }
        }
        0
    }

    /// The design speed of the leg under the truck, in mph.
    ///
    /// What the curve bake priced this leg's advisories and its bank with, so
    /// anything reading the bank at run time (the lane model's cornering
    /// ceiling) asks the same question of the same road.
    pub fn leg_design_speed_mph(&self) -> f64 {
        self.route
            .legs
            .get(self.current_leg_index())
            .map(|leg| crate::data::curves::leg_design_speed(leg))
            .unwrap_or(55.0)
    }

    /// The world city the current leg heads toward; panics (Python:
    /// `KeyError`) for a synthetic city the world does not carry.
    pub fn current_target_city(&self) -> &City {
        let name = &self.route.cities[self.current_leg_index() + 1];
        self.world
            .cities
            .get(name)
            .unwrap_or_else(|| panic!("KeyError: {name:?}"))
    }

    pub fn current_region(&self) -> &str {
        &self.current_target_city().region
    }

    pub fn current_career_hours(&self) -> Option<f64> {
        self.career_hours
            .map(|hours| hours + self.game_minutes / 60.0)
    }

    pub fn is_weekend_now(&self) -> bool {
        match self.current_career_hours() {
            None => false,
            Some(hours) => crate::sim::season::is_weekend(hours),
        }
    }

    /// Mark the road behind the truck as already driven, for a trip placed
    /// partway along it (a staged bench drive).
    ///
    /// Placed at mile 1175, the first frame found every state line, toll,
    /// town and roadside callout from mile 0 "just passed" and spoke them:
    /// Iowa's welcome on a Texas road, a turnpike toll charged, two
    /// achievements (agent drives, 2026-09-23). The toll, town and callout
    /// checks only ever fire for road already behind the truck, so running
    /// them here with their output dropped latches exactly that; navigation
    /// cues also look ahead, so only the ones behind are marked.
    pub fn settle_road_behind(&mut self) {
        let events = self.events.len();
        let tolls = self.toll_charges.len();
        self.check_tolls();
        self.check_cities();
        self.check_roadside_callouts();
        self.events.truncate(events);
        self.toll_charges.truncate(tolls);
        let position = self.position_mi;
        let behind: Vec<String> = self
            .navigation_cues
            .iter()
            .filter(|cue| cue.at_mi < position)
            .map(|cue| cue.key.clone())
            .collect();
        for key in behind {
            self.announced_navigation.insert(format!("{key}:advance"));
            self.announced_navigation.insert(format!("{key}:near"));
        }
        // The restore path's latch: passed curves count as called, posts
        // inside their watch were heard, pressures behind are old news, and
        // the zone under the truck is entered -- without it the first frame
        // spoke a fresh "traffic is backing up" ZoneEnter for the zone the
        // truck was dropped into and awarded the jam achievement.
        self.latch_passed_roadside();
    }

    /// After a staged drive sets the truck's speed, count as already called
    /// any bend whose call window the truck was dropped inside of -- the
    /// pacenote was "heard before the handoff". A bend the speed still
    /// outruns keeps its call.
    pub fn settle_calls_in_hand(&mut self) {
        let speed = self.truck.speed_mph();
        for cr in &self.curves {
            let ahead = cr.start_mi - self.position_mi;
            if ahead <= 0.0 {
                continue;
            }
            let (call_above, target) = self.curve_call_mph(cr);
            if speed > call_above && ahead <= Self::curve_pacenote_lead_mi(speed, target) {
                self.announced_curves.insert(format!(
                    "curve:{}:{}",
                    crate::pyfmt::fmt_f(cr.start_mi, 3),
                    cr.direction
                ));
            }
        }
    }

    pub fn emit(
        &mut self,
        kind: TripEventKind,
        message: impl Into<crate::speech_text::SpokenMessage>,
        data: TripEventData,
    ) {
        self.events.push(TripEvent {
            kind,
            message: message.into(),
            data,
        });
    }
}

fn compute_leg_starts(route: &Route) -> Vec<f64> {
    let mut starts = Vec::new();
    let mut acc = 0.0;
    for leg in &route.legs {
        starts.push(acc);
        acc += leg.miles;
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spoken_short_miles_units() {
        assert_eq!(spoken_short_miles(0.2, true), "a quarter mile");
        assert_eq!(spoken_short_miles(0.5, true), "half a mile");
        assert_eq!(spoken_short_miles(0.5, false), "800 meters");
        assert_eq!(spoken_short_miles(1.0, true), "one mile");
        assert_eq!(spoken_short_miles(5.0, true), "5 miles");
    }

    #[test]
    fn cue_direction_reads_the_verb() {
        assert_eq!(cue_direction("Turn right onto Palm Street"), "right");
        assert_eq!(cue_direction("Continue onto Main Street"), "ahead");
        assert_eq!(cue_direction("Turn onto Elm"), "");
    }
}
