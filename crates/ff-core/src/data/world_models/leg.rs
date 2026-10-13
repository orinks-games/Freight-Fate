//! The leg (with its lazily parsed corridor) and the route that chains legs
//! -- the `Leg`, `LazyLeg` and `Route` half of `world_models.py`.

use std::fmt;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

use super::{
    lane_word, DataError, ElevationSample, GradeSegment, HpmsTerrain, Interchange, Landmark,
    LaneSegment, RouteCheckpoint, RoutePoint, RouteRestriction, SpeedLimitSample, StateCrossing,
    StateMileage, Stop, StreetControl, StreetLimit, TollEvent, TrafficVolumeSample,
};
use crate::data::world::World;
use crate::data::world_corridor::build_leg_corridor;
use crate::pyfmt::fmt_f;

#[derive(Debug, Clone, PartialEq)]
pub struct BillboardBan {
    pub from_mi: f64,
    pub to_mi: f64,
    pub name: String,
    pub source: String,
}

/// The heavy per-mile corridor fields a leg parses on first touch.
/// Everything else on a `Leg` (endpoints, miles, highway, terrain, stops,
/// lanes, the local cue, divided, meta_complete) stays eager because the
/// route graph, dispatch, and route briefings read it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CorridorDetail {
    pub route_points: Vec<RoutePoint>,
    pub elevation_samples: Vec<ElevationSample>,
    pub grade_segments: Vec<GradeSegment>,
    pub state_crossings: Vec<StateCrossing>,
    pub checkpoints: Vec<RouteCheckpoint>,
    pub state_miles: Vec<StateMileage>,
    pub toll_events: Vec<TollEvent>,
    pub interchanges: Vec<Interchange>,
    pub speed_limits: Vec<SpeedLimitSample>,
    pub traffic_volumes: Vec<TrafficVolumeSample>,
    pub hpms_terrain: Option<HpmsTerrain>,
    pub landmarks: Vec<Landmark>,
    pub restrictions: Vec<RouteRestriction>,
    pub lane_segments: Vec<LaneSegment>,
    pub billboard_bans: Vec<BillboardBan>,
}

/// The raw corridor JSON plus its parse context, held by a lazy leg until the
/// first read of any deferred field and dropped once the parsed tuples own
/// the data.
#[derive(Debug, Clone)]
pub struct DetailSource {
    pub corridor: serde_json::Value,
    pub miles: f64,
    pub leg_from: String,
    pub leg_to: String,
    pub from_state: String,
    pub highway: String,
}

/// Builds one leg's corridor detail on first touch. The baked container hands
/// a leg one of these instead of raw JSON: it holds the mapped container and
/// this leg's own offset, and decompresses that one frame when the leg is
/// first driven.
pub type CorridorBuilder = Arc<dyn Fn() -> Result<CorridorDetail, DataError> + Send + Sync>;

/// Where a lazy leg's corridor comes from: the raw JSON a leg shard carried,
/// or a builder that fetches it from somewhere else.
enum Deferred {
    Json(DetailSource),
    Builder(CorridorBuilder),
}

impl Clone for Deferred {
    fn clone(&self) -> Self {
        match self {
            Deferred::Json(source) => Deferred::Json(source.clone()),
            Deferred::Builder(build) => Deferred::Builder(Arc::clone(build)),
        }
    }
}

impl fmt::Debug for Deferred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Deferred::Json(source) => write!(f, "{source:?}"),
            Deferred::Builder(_) => f.write_str("CorridorBuilder"),
        }
    }
}

/// `Leg::id` of a leg built outside a world (local street chains, tests).
/// World legs are numbered by their index in `World::legs`; that index is
/// what the routing penalty maps key on, standing in for Python's
/// identity-based `LazyLeg.__hash__`.
pub const NO_LEG_ID: usize = usize::MAX;

/// A highway leg between two cities (or one street segment of a local chain).
///
/// This is Python's `Leg` and `LazyLeg` in one: `World` used to construct
/// grade segments, interchanges, landmarks, speed limits and the rest for
/// all fifty states at startup -- roughly a second of pure latency before the
/// menu, most of it never touched in a session. A leg built by the world
/// stores only the eager fields the route graph and dispatch need plus the
/// raw corridor and its parse context; the deferred records are parsed once,
/// on the first accessor call (driving a leg), then cached in a `OnceCell`
/// so later reads are plain field lookups.
///
/// Equality is deliberately not derived: the world owns exactly one object
/// per leg, they are the keys of the routing penalty maps (by `id`), and a
/// value comparison would force the very parse we are deferring.
pub struct Leg {
    pub id: usize,
    pub a: String,
    pub b: String,
    pub miles: f64,
    pub highway: String,
    /// flat | hills | mountain
    pub terrain: String,
    pub stops: Vec<Stop>,
    /// A published truck warning on this road -- CDOT-style "truckers beware"
    /// campaigns, non-truck-route passes. Text carries its own source. Routing
    /// treats it as strong avoidance, never refusal: it is warnings and
    /// carrier policy, not statute (verified against CDOT and the CCR for
    /// US-550 Red Mountain Pass, 2026-08-20 -- no length rule exists).
    pub truck_advisory: String,
    /// Driving lanes per direction, baked from HPMS through-lane counts
    /// (leg-level median); 0 means unbaked and the runtime default applies.
    pub lanes: i64,
    /// Surface-street segments (tier-1 local routes) carry their baked turn
    /// cue and street speed so the runtime can speak the real maneuver and
    /// zone the street instead of a whole-route blanket. Empty on highways.
    pub local_cue: String,
    pub local_speed_mph: f64,
    /// The turn angle at the junction ONTO this segment, in degrees, baked
    /// from the signed heading change read out of OSM geometry. 0.0 means the
    /// corner's angle was never measured -- a legacy or estimated route -- and
    /// `data::corners` prices those as a square corner and says so. The
    /// magnitude survives a route reversal unchanged: an inbound 90-degree
    /// right is an outbound 90-degree left at the same corner.
    pub local_turn_deg: f64,
    /// A facility street's posted limit and what kind of value it is, and
    /// its READ traffic controls (`tools/street_chain.py`). None and empty on
    /// highways, on chains baked before the street detail, and outbound.
    pub local_limit: Option<StreetLimit>,
    pub local_controls: Vec<StreetControl>,
    /// The leg lies past the facility's driveway, on its own service or
    /// private way: the yard, not a public street. Derived from the chain's
    /// baked driveway (`Driveway`), which is a leg boundary on 1,872 of 1,875
    /// exit chains; a driveway inside a leg marks nothing.
    pub local_yard: bool,
    /// Whether the leg runs on a divided carriageway, baked from real OSM
    /// oneway-pair geometry (Track D2). None where the bake was mixed or
    /// thin -- honest absence; the runtime infers from road class instead.
    pub divided: Option<bool>,
    /// Dispatch-completeness precomputed at world load from raw corridor
    /// counts (see `world_corridor::raw_metadata_complete`). None means "not
    /// precomputed" -- direct-constructed legs (tests, overlays) fall back to
    /// computing it from their own fields, so behavior is unchanged. The
    /// world sets it so the route graph never has to parse deferred detail
    /// just to gate dispatch.
    pub meta_complete: Option<bool>,
    corridor: OnceCell<Result<CorridorDetail, DataError>>,
    detail_source: Mutex<Option<Deferred>>,
}

impl Leg {
    /// A leg with no corridor detail (every deferred field empty), as Python's
    /// plain `Leg(a, b, miles, highway, terrain, stops)`.
    pub fn new(
        a: &str,
        b: &str,
        miles: f64,
        highway: &str,
        terrain: &str,
        stops: Vec<Stop>,
    ) -> Self {
        Leg {
            id: NO_LEG_ID,
            a: a.to_string(),
            b: b.to_string(),
            miles,
            highway: highway.to_string(),
            terrain: terrain.to_string(),
            stops,
            truck_advisory: String::new(),
            lanes: 0,
            local_cue: String::new(),
            local_speed_mph: 0.0,
            local_turn_deg: 0.0,
            local_limit: None,
            local_controls: Vec::new(),
            local_yard: false,
            divided: None,
            meta_complete: None,
            corridor: OnceCell::new(),
            detail_source: Mutex::new(None),
        }
    }

    /// A surface-street segment of a same-city local chain.
    pub fn local(
        city: &str,
        miles: f64,
        road: &str,
        local_cue: &str,
        local_speed_mph: f64,
    ) -> Self {
        let mut leg = Leg::new(city, city, miles, road, "flat", Vec::new());
        leg.local_cue = local_cue.to_string();
        leg.local_speed_mph = local_speed_mph;
        leg
    }

    /// The measured turn angle at the junction onto this local segment.
    ///
    /// Separate from [`Leg::local`] rather than a sixth positional argument:
    /// most callers (and every legacy route) have no angle to give, and a
    /// corner with no measurement is a real, expected state.
    pub fn with_turn_deg(mut self, degrees: f64) -> Self {
        self.local_turn_deg = degrees;
        self
    }

    /// The street detail of the facility chain segment this leg drives.
    pub fn with_street(mut self, limit: Option<StreetLimit>, controls: Vec<StreetControl>) -> Self {
        self.local_limit = limit;
        self.local_controls = controls;
        self
    }

    /// Mark this local leg as past the driveway (see `local_yard`).
    pub fn with_yard(mut self, yard: bool) -> Self {
        self.local_yard = yard;
        self
    }

    /// The posted limit of this facility street, when the chain carries the
    /// street detail: the yard's own limit past the driveway, else the
    /// street's baked limit whatever its kind (read, statutory or assumed).
    /// None on a highway leg and on a chain baked before the street detail.
    pub fn street_limit_mph(&self) -> Option<f64> {
        if self.local_yard {
            return Some(crate::sim::trip_models::YARD_LIMIT_MPH);
        }
        self.local_limit.as_ref().map(|limit| limit.mph)
    }

    /// A leg whose corridor detail is parsed from `source` on first read.
    pub fn lazy(
        a: &str,
        b: &str,
        miles: f64,
        highway: &str,
        terrain: &str,
        stops: Vec<Stop>,
        source: DetailSource,
    ) -> Self {
        let leg = Leg::new(a, b, miles, highway, terrain, stops);
        *leg.detail_source.lock() = Some(Deferred::Json(source));
        leg
    }

    /// A leg whose corridor detail comes from `build` on first read -- the
    /// baked container's leg, which decompresses its own frame instead of
    /// parsing a raw JSON corridor it never had to hold.
    pub fn lazy_built(
        a: &str,
        b: &str,
        miles: f64,
        highway: &str,
        terrain: &str,
        stops: Vec<Stop>,
        build: CorridorBuilder,
    ) -> Self {
        let leg = Leg::new(a, b, miles, highway, terrain, stops);
        *leg.detail_source.lock() = Some(Deferred::Builder(build));
        leg
    }

    /// Supply the corridor detail up front (a fully materialized leg).
    pub fn with_detail(self, detail: CorridorDetail) -> Self {
        Leg {
            corridor: OnceCell::with_value(Ok(detail)),
            detail_source: Mutex::new(None),
            ..self
        }
    }

    pub fn other(&self, city: &str) -> &str {
        if city == self.a {
            &self.b
        } else {
            &self.a
        }
    }

    /// Whether the deferred corridor has been parsed (or was supplied).
    pub fn corridor_is_built(&self) -> bool {
        self.corridor.get().is_some()
    }

    /// Whether a raw corridor is still waiting to be parsed.
    pub fn has_deferred_source(&self) -> bool {
        self.detail_source.lock().is_some()
    }

    /// Parse and cache the deferred corridor detail once, thread-safely; the
    /// `OnceCell` serializes the first concurrent build and every later read
    /// is lock-free. A leg with neither a source nor supplied detail reads as
    /// empty.
    pub fn try_corridor(&self) -> Result<&CorridorDetail, DataError> {
        self.corridor
            .get_or_init(|| {
                let source = self.detail_source.lock().take();
                match source {
                    None => Ok(CorridorDetail::default()),
                    Some(Deferred::Json(src)) => build_leg_corridor(
                        &src.corridor,
                        src.miles,
                        &src.leg_from,
                        &src.leg_to,
                        &src.from_state,
                        &src.highway,
                    ),
                    Some(Deferred::Builder(build)) => build(),
                }
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The corridor detail; panics with the data error where Python raised
    /// `ValueError` from the first attribute read.
    pub fn corridor(&self) -> &CorridorDetail {
        match self.try_corridor() {
            Ok(detail) => detail,
            Err(err) => panic!("{} to {}: {err}", self.a, self.b),
        }
    }

    pub fn route_points(&self) -> &[RoutePoint] {
        &self.corridor().route_points
    }

    pub fn elevation_samples(&self) -> &[ElevationSample] {
        &self.corridor().elevation_samples
    }

    pub fn grade_segments(&self) -> &[GradeSegment] {
        &self.corridor().grade_segments
    }

    pub fn state_crossings(&self) -> &[StateCrossing] {
        &self.corridor().state_crossings
    }

    pub fn checkpoints(&self) -> &[RouteCheckpoint] {
        &self.corridor().checkpoints
    }

    pub fn state_miles(&self) -> &[StateMileage] {
        &self.corridor().state_miles
    }

    pub fn toll_events(&self) -> &[TollEvent] {
        &self.corridor().toll_events
    }

    pub fn interchanges(&self) -> &[Interchange] {
        &self.corridor().interchanges
    }

    pub fn speed_limits(&self) -> &[SpeedLimitSample] {
        &self.corridor().speed_limits
    }

    pub fn traffic_volumes(&self) -> &[TrafficVolumeSample] {
        &self.corridor().traffic_volumes
    }

    pub fn hpms_terrain(&self) -> Option<&HpmsTerrain> {
        self.corridor().hpms_terrain.as_ref()
    }

    pub fn landmarks(&self) -> &[Landmark] {
        &self.corridor().landmarks
    }

    pub fn billboard_bans(&self) -> &[BillboardBan] {
        &self.corridor().billboard_bans
    }

    pub fn restrictions(&self) -> &[RouteRestriction] {
        &self.corridor().restrictions
    }

    pub fn lane_segments(&self) -> &[LaneSegment] {
        &self.corridor().lane_segments
    }

    pub fn metadata_complete(&self, from_state: &str, to_state: &str) -> bool {
        if let Some(flag) = self.meta_complete {
            return flag;
        }
        self.metadata_complete_from_fields(from_state, to_state)
    }

    /// True when a leg has enough real corridor data to be dispatchable.
    ///
    /// Dispatch gates on *routing* completeness: route geometry, elevation and
    /// grade, state mileage, and a state crossing when the endpoints differ --
    /// all of which the ORS driving-hgv pipeline produces automatically, so the
    /// map can scale without hand work. Curated truck-stop POIs are an additive
    /// quality layer (auto-sourced; see the coverage report's POI/fuel
    /// advisory), not a dispatch requirement: a stop-less leg stays playable
    /// via the HOS fallbacks (roadside fuel rescue, emergency shoulder sleep).
    /// POI data that *is* present is still validated at load by `parse_stop`.
    pub fn metadata_complete_from_fields(&self, from_state: &str, to_state: &str) -> bool {
        if self.route_points().len() < 2 {
            return false;
        }
        // Checkpoints are deliberately NOT required: they are a speech-quality
        // layer, same class as the POIs the docstring already exempts. The old
        // non-empty requirement is why 246 legs carried a fake "X corridor
        // between A and B" placeholder checkpoint -- which then leaked into
        // place callouts as if it were a town (owner report 2026-07-23). The
        // placeholders are gone; dispatch must not miss them.
        if self.state_miles().is_empty() {
            return false;
        }
        if self.elevation_samples().len() < 2 || self.grade_segments().is_empty() {
            return false;
        }
        from_state == to_state || !self.state_crossings().is_empty()
    }
}

impl Clone for Leg {
    /// Python's `dataclasses.replace` semantics: a clone carries the parsed
    /// detail if it exists, else the raw source, so it answers the same.
    fn clone(&self) -> Self {
        let corridor = match self.corridor.get() {
            Some(built) => OnceCell::with_value(built.clone()),
            None => OnceCell::new(),
        };
        Leg {
            id: self.id,
            a: self.a.clone(),
            b: self.b.clone(),
            miles: self.miles,
            highway: self.highway.clone(),
            terrain: self.terrain.clone(),
            stops: self.stops.clone(),
            truck_advisory: self.truck_advisory.clone(),
            lanes: self.lanes,
            local_cue: self.local_cue.clone(),
            local_speed_mph: self.local_speed_mph,
            local_turn_deg: self.local_turn_deg,
            local_limit: self.local_limit.clone(),
            local_controls: self.local_controls.clone(),
            local_yard: self.local_yard,
            divided: self.divided,
            meta_complete: self.meta_complete,
            corridor,
            detail_source: Mutex::new(self.detail_source.lock().clone()),
        }
    }
}

impl fmt::Debug for Leg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LazyLeg({:?} -> {:?}, {:?}, {} mi)",
            self.a, self.b, self.highway, self.miles
        )
    }
}

/// An ordered chain of legs from start to end.
#[derive(Debug, Clone, Default)]
pub struct Route {
    pub cities: Vec<String>,
    pub legs: Vec<Arc<Leg>>,
}

impl Route {
    pub fn new(cities: Vec<String>, legs: Vec<Arc<Leg>>) -> Self {
        Route { cities, legs }
    }

    /// This route continued by `next`, which starts where this one ends: a
    /// corridor into a city, then that city's facility approach, as one
    /// drive. The shared city is kept once.
    pub fn then(&self, next: &Route) -> Route {
        debug_assert_eq!(self.cities.last(), next.cities.first());
        let mut cities = self.cities.clone();
        cities.extend(next.cities.iter().skip(1).cloned());
        let mut legs = self.legs.clone();
        legs.extend(next.legs.iter().cloned());
        Route::new(cities, legs)
    }

    /// A route from owned legs (tests and local street chains).
    pub fn from_legs(cities: Vec<String>, legs: Vec<Leg>) -> Self {
        Route {
            cities,
            legs: legs.into_iter().map(Arc::new).collect(),
        }
    }

    pub fn miles(&self) -> f64 {
        // Fold from 0.0: Rust's `Sum for f64` starts at -0.0, so an empty
        // route would report negative zero miles.
        self.legs
            .iter()
            .map(|leg| leg.miles)
            .fold(0.0, |total, miles| total + miles)
    }

    pub fn highways(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for leg in &self.legs {
            if out.last().is_none_or(|last| *last != leg.highway) {
                out.push(leg.highway.clone());
            }
        }
        out
    }

    pub fn stops(&self) -> Vec<String> {
        self.stop_details()
            .into_iter()
            .map(|s| s.name.clone())
            .collect()
    }

    fn stops_with_route_miles(&self, include_uncurated: bool) -> Vec<Stop> {
        let mut at_mi = 0.0;
        let mut stops = Vec::new();
        for (index, leg) in self.legs.iter().enumerate() {
            let forward = self.cities.get(index).is_none_or(|city| city == &leg.a);
            for stop in &leg.stops {
                if !include_uncurated && !stop.curated() {
                    continue;
                }
                let mut route_stop = stop.clone();
                let local_mi = if forward {
                    stop.at_mi
                } else {
                    leg.miles - stop.at_mi
                };
                route_stop.at_mi = at_mi + local_mi;
                stops.push(route_stop);
            }
            at_mi += leg.miles;
        }
        stops.sort_by(|left, right| left.at_mi.total_cmp(&right.at_mi));
        stops
    }

    /// Curated stops with mileposts measured from the route's start.
    pub fn stop_details(&self) -> Vec<Stop> {
        self.stops_with_route_miles(false)
    }

    /// All stop records with mileposts measured from the route's start.
    pub fn raw_stop_details(&self) -> Vec<Stop> {
        self.stops_with_route_miles(true)
    }

    /// Curated stops the rig can physically use, for pre-trip planning.
    ///
    /// Dispatch and the route briefing speak these counts while the player
    /// decides whether a run is survivable, so a stop that would turn a rig
    /// away must not pad them. Pass `false` for the trailer case, the cautious
    /// read and the one nearly every job is.
    pub fn accessible_stop_details(&self, bobtail: bool) -> Vec<Stop> {
        self.stop_details()
            .into_iter()
            .filter(|s| s.accessible_to(bobtail))
            .collect()
    }

    pub fn state_crossings(&self) -> Vec<&StateCrossing> {
        self.legs
            .iter()
            .flat_map(|leg| leg.state_crossings().iter())
            .collect()
    }

    pub fn toll_events(&self) -> Vec<&TollEvent> {
        self.legs
            .iter()
            .flat_map(|leg| leg.toll_events().iter())
            .collect()
    }

    pub fn estimated_tolls(&self) -> f64 {
        // Fold from 0.0: see `miles` above -- an empty toll list must be 0,
        // not -0, or the spoken estimate says "minus zero dollars".
        self.toll_events()
            .iter()
            .map(|event| event.amount)
            .fold(0.0, |total, amount| total + amount)
    }

    pub fn checkpoints(&self) -> Vec<&RouteCheckpoint> {
        self.legs
            .iter()
            .flat_map(|leg| leg.checkpoints().iter())
            .collect()
    }

    pub fn interchanges(&self) -> Vec<&Interchange> {
        self.legs
            .iter()
            .flat_map(|leg| leg.interchanges().iter())
            .collect()
    }

    pub fn terrain_summary(&self) -> &'static str {
        let kinds: Vec<&str> = self.legs.iter().map(|leg| leg.terrain.as_str()).collect();
        if !kinds.is_empty() && kinds.iter().all(|k| *k == "flat") {
            return "flat";
        }
        if kinds.contains(&"mountain") {
            return "mountainous in places";
        }
        "rolling hills"
    }

    /// Miles-weighted lane picture for the route briefing, in the travel
    /// direction, or empty where too little of the route carries lane data.
    ///
    /// Honest absence: legs with no baked lane counts contribute nothing, so a
    /// route the bake never reached simply says nothing about lanes.
    pub fn lane_summary(&self) -> String {
        // Insertion-ordered like the Python dict, so the stable sort below
        // breaks ties the same way.
        let mut miles_by_lanes: Vec<(i64, f64)> = Vec::new();
        let mut divided_mi = 0.0;
        let mut total_mi = 0.0;
        for leg in &self.legs {
            for seg in leg.lane_segments() {
                let span = (seg.end_mi - seg.start_mi).max(0.0);
                let n = seg.your_side();
                match miles_by_lanes.iter_mut().find(|(k, _)| *k == n) {
                    Some(entry) => entry.1 += span,
                    None => miles_by_lanes.push((n, span)),
                }
                total_mi += span;
                if seg.divided() {
                    divided_mi += span;
                }
            }
        }
        // Need a meaningful sample of the route before summarizing.
        if total_mi < (0.2 * self.miles()).max(1.0) {
            return String::new();
        }
        let mut ranked = miles_by_lanes;
        ranked.sort_by(|a, b| (-a.1).partial_cmp(&(-b.1)).expect("finite miles"));
        let top = ranked[0].0;
        let divided = divided_mi >= 0.5 * total_mi;
        // Each wording is written out whole so a translation sees the
        // complete phrase, never a "divided" prefix glued to a count.
        // A clear second value that holds real distance earns a range.
        if ranked.len() > 1 && ranked[1].1 >= 0.25 * total_mi {
            let (lo, hi) = if ranked[0].0 <= ranked[1].0 {
                (ranked[0].0, ranked[1].0)
            } else {
                (ranked[1].0, ranked[0].0)
            };
            return if divided {
                format!(
                    "mostly divided, {} to {} lanes your side",
                    lane_word(lo),
                    lane_word(hi)
                )
            } else {
                format!("{} to {} lanes your side", lane_word(lo), lane_word(hi))
            };
        }
        if divided {
            format!(
                "mostly divided, {} lane{} your side",
                lane_word(top),
                if top != 1 { "s" } else { "" }
            )
        } else {
            format!(
                "{} lane{} your side",
                lane_word(top),
                if top != 1 { "s" } else { "" }
            )
        }
    }

    pub fn describe(&self, distance_text: &str) -> String {
        let via = self.highways().join(" then ");
        let distance = if distance_text.is_empty() {
            format!("{} miles", fmt_f(self.miles(), 0))
        } else {
            distance_text.to_string()
        };
        let lane_text = self.lane_summary();
        let lane_part = if lane_text.is_empty() {
            String::new()
        } else {
            format!(", {lane_text}")
        };
        format!(
            "{distance} via {via}, {} leg{}, terrain {}{lane_part}",
            self.legs.len(),
            if self.legs.len() != 1 { "s" } else { "" },
            self.terrain_summary()
        )
    }

    pub fn metadata_complete(&self, world: &World) -> bool {
        self.legs.iter().all(|leg| world.leg_metadata_complete(leg))
    }
}
