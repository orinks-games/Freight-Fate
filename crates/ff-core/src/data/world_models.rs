//! The world data model: cities, facilities, stops, and every per-mile
//! corridor record (port of `freight_fate/data/world_models.py`).
//!
//! Python kept these as frozen dataclasses; here they are plain `Clone`
//! structs with the same fields and the same spoken-text methods. Python's
//! `type` fields are renamed (`facility_type`, `stop_type`, ...) because the
//! word is reserved here. The leg itself, with its lazily parsed corridor, and
//! the route that chains legs live in the `leg` submodule and are re-exported.

use std::fmt;

use super::world_constants::{
    lookup, screened_vehicle_access, vehicle_access_allows, DEFAULT_VEHICLE_ACCESS,
    LOCATION_TYPE_LABELS, PARKING_CERTAINTY_LABELS, STOP_TYPE_LABELS, TOLL_METHOD_LABELS,
};
use crate::pyfmt::{py_int, py_str_float, round_py_int, round_py_n};

mod interchange;
mod leg;
mod street;
pub use interchange::{
    destinations_without_via, format_route_ref, join_destinations, route_token, Interchange,
};
pub use leg::{BillboardBan, CorridorBuilder, CorridorDetail, DetailSource, Leg, Route, NO_LEG_ID};
pub use street::{Driveway, ExitChain, LocalGeometrySegment, StreetControl, StreetLimit};

/// The errors the Python data layer raised: `ValueError` for data that
/// fails validation, `KeyError` for an unknown city/facility/service, and
/// I/O or JSON failures reading a file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DataError {
    /// Python `ValueError` -- the message is the Python message verbatim.
    #[error("{0}")]
    Value(String),
    /// Python `KeyError`.
    #[error("{0}")]
    Key(String),
    /// A file could not be read or parsed.
    #[error("{0}")]
    Io(String),
}

impl DataError {
    pub fn value(message: impl Into<String>) -> Self {
        DataError::Value(message.into())
    }

    pub fn key(message: impl Into<String>) -> Self {
        DataError::Key(message.into())
    }

    pub fn io(message: impl Into<String>) -> Self {
        DataError::Io(message.into())
    }
}

impl From<std::io::Error> for DataError {
    fn from(err: std::io::Error) -> Self {
        DataError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for DataError {
    fn from(err: serde_json::Error) -> Self {
        DataError::Io(err.to_string())
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A freight facility in a city: shipper, receiver, or both.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub name: String,
    /// Python `type`: one of `FREIGHT_LOCATION_TYPES`.
    pub facility_type: String,
    pub cargo: Vec<String>,
    pub id: String,
    pub city: String,
    pub locality: String,
    pub roles: Vec<String>,
    pub ships: Vec<String>,
    pub receives: Vec<String>,
    pub lat: f64,
    pub lon: f64,
    pub traits: Vec<String>,
    pub source_note: String,
    pub spoken: String,
    pub template: bool,
    pub min_level: i64,
}

impl Default for Location {
    fn default() -> Self {
        Location {
            name: String::new(),
            facility_type: String::new(),
            cargo: Vec::new(),
            id: String::new(),
            city: String::new(),
            locality: String::new(),
            roles: strings(&["shipper", "receiver"]),
            ships: Vec::new(),
            receives: Vec::new(),
            lat: 0.0,
            lon: 0.0,
            traits: Vec::new(),
            source_note: String::new(),
            spoken: String::new(),
            template: false,
            min_level: 1,
        }
    }
}

impl Location {
    pub fn label(&self) -> String {
        lookup(LOCATION_TYPE_LABELS, &self.facility_type)
            .map(str::to_string)
            .unwrap_or_else(|| self.facility_type.replace('_', " "))
    }

    pub fn spoken_name(&self) -> String {
        if !self.spoken.is_empty() {
            self.spoken.clone()
        } else {
            format!("{}: {}", self.label(), self.name)
        }
    }

    pub fn display_name(&self) -> &str {
        &self.name
    }
}

/// The player's dispatch yard for a service area (`city` carries the SPOKEN
/// city name -- the terminal object exists to be announced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeTerminal {
    pub name: String,
    pub city: String,
    pub state: String,
    pub kind: String,
}

impl HomeTerminal {
    pub fn new(name: &str, city: &str, state: &str, kind: &str) -> Self {
        HomeTerminal {
            name: name.to_string(),
            city: city.to_string(),
            state: state.to_string(),
            kind: kind.to_string(),
        }
    }

    pub fn label(&self) -> &'static str {
        if self.kind == "terminal" {
            "company terminal"
        } else {
            "company yard"
        }
    }

    pub fn spoken_name(&self) -> String {
        format!("{}: {}", self.label(), self.name)
    }

    pub fn service_area(&self) -> String {
        format!("{}, {}", self.city, self.state)
    }
}

/// A city service POI (freight market office, garage, truck dealer).
#[derive(Debug, Clone, PartialEq)]
pub struct CityService {
    pub key: String,
    pub name: String,
    pub city: String,
    pub state: String,
    pub kind: String,
    pub source_note: String,
    pub lat: f64,
    pub lon: f64,
    pub approach_miles: f64,
    pub approach_road: String,
    pub source_type: String,
    pub source_ref: String,
    pub fallback: bool,
    pub fallback_reason: String,
}

impl Default for CityService {
    fn default() -> Self {
        CityService {
            key: String::new(),
            name: String::new(),
            city: String::new(),
            state: String::new(),
            kind: String::new(),
            source_note: String::new(),
            lat: 0.0,
            lon: 0.0,
            approach_miles: 0.0,
            approach_road: String::new(),
            source_type: "fallback".to_string(),
            source_ref: String::new(),
            fallback: true,
            fallback_reason: String::new(),
        }
    }
}

impl CityService {
    pub fn label(&self) -> String {
        lookup(super::world_constants::CITY_SERVICE_LABELS, &self.kind)
            .map(str::to_string)
            .unwrap_or_else(|| self.kind.replace('_', " "))
    }

    pub fn spoken_name(&self) -> String {
        format!("{}: {}", self.label(), self.name)
    }
}

/// Where a facility's gate really is, from the endpoint sweep.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FacilityEndpoint {
    pub facility_id: String,
    pub city: String,
    pub state: String,
    pub facility_name: String,
    pub facility_type: String,
    pub endpoint_name: String,
    pub source_type: String,
    pub source_note: String,
    pub lat: f64,
    pub lon: f64,
    pub approach_miles: f64,
    pub approach_road: String,
    pub source_ref: String,
    pub source_backed: bool,
    pub fallback: bool,
    pub fallback_reason: String,
    pub nearest_road_context: bool,
    pub turn_level_geometry: bool,
    pub gate_hint: bool,
    pub yard_hint: bool,
    pub dock_hint: bool,
    pub mapping: String,
}

/// A facility's approach from the highway: a turn-level street chain where
/// the sweep found one, a representative estimate otherwise.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FacilityApproach {
    pub facility_id: String,
    pub city: String,
    pub state: String,
    pub facility_name: String,
    pub facility_type: String,
    pub endpoint_name: String,
    pub endpoint_source_backed: bool,
    pub road_snapped: bool,
    pub turn_level: bool,
    pub source_type: String,
    pub estimated: bool,
    pub fallback: bool,
    pub fallback_reason: String,
    pub nearest_road_context: bool,
    pub representative_fallback: bool,
    pub total_miles: f64,
    pub approach_road: String,
    pub segments: Vec<LocalGeometrySegment>,
    pub gate_hint: bool,
    pub yard_hint: bool,
    pub dock_hint: bool,
    pub final_hint: String,
    pub source_note: String,
    /// Where `segments` leaves the public street, if the chain does.
    pub driveway: Option<Driveway>,
    /// One chain per ramp terminal a delivery can arrive at, whole.
    pub exit_chains: Vec<ExitChain>,
}

/// A roadside stop (truck stop, rest area, weigh station, ...) along a leg.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub name: String,
    pub at_mi: f64,
    /// Python `type`: one of `STOP_TYPE_LABELS`.
    pub stop_type: String,
    pub source: String,
    pub actions: Vec<String>,
    pub services: Vec<String>,
    pub parking: String,
    pub directions: Vec<String>,
    pub curation: String,
    /// Truck-parking spot count from an official inventory (FHWA Jason's Law
    /// via BTS NTAD); 0 means unsurveyed and capacity stays out of speech.
    pub parking_spaces: i64,
    /// Whether a combination vehicle can physically get in here. Defaults to
    /// tractor_trailer so unclassified data keeps behaving as it always has.
    pub vehicle_access: String,
    /// Signed exit number of the interchange that serves this stop, decided
    /// once at bake time (`tools/snap_stops_to_interchanges.py`); "" when
    /// nothing decided it and the mile-marker lookup still stands in.
    pub exit_ref: String,
    /// `at_mi` of this leg's interchange record for that exit. An identity to
    /// match exactly, never a mile to search near; None when the leg records
    /// no such exit.
    pub interchange_mi: Option<f64>,
    /// The streets from each of that exit's ramp terminals to the stop's
    /// driveway, keyed by terminal node like a facility's exit chains. Empty
    /// for a stop on the mainline or with no decided exit.
    pub approach_chains: Vec<ExitChain>,
}

impl Default for Stop {
    fn default() -> Self {
        Stop {
            name: String::new(),
            at_mi: 0.0,
            stop_type: "travel_center".to_string(),
            source: String::new(),
            actions: Vec::new(),
            services: Vec::new(),
            parking: "unknown".to_string(),
            directions: strings(&["both"]),
            curation: "curated".to_string(),
            parking_spaces: 0,
            vehicle_access: DEFAULT_VEHICLE_ACCESS.to_string(),
            exit_ref: String::new(),
            interchange_mi: None,
            approach_chains: Vec::new(),
        }
    }
}

impl Stop {
    /// Can the rig the player is driving right now actually use this stop?
    ///
    /// In an audio-first game, announcing a stop is a promise the player can
    /// take it. A stop a rig cannot enter is worse than no stop at all: it
    /// burns driving hours and can strand someone with no legal alternative.
    pub fn accessible_to(&self, bobtail: bool) -> bool {
        vehicle_access_allows(self.effective_vehicle_access(), bobtail)
    }

    /// The access level this stop's own record supports: the recorded value,
    /// screened for the assumed `tractor_trailer` default on a convenience
    /// station the map typed as a travel center. See
    /// `world_constants::screened_vehicle_access`; this is what the runtime
    /// road stop carries, so announcements, exit arming, rest planning and
    /// the tablet all read the same answer.
    pub fn effective_vehicle_access(&self) -> &str {
        screened_vehicle_access(
            &self.name,
            &self.stop_type,
            &self.parking,
            self.parking_spaces,
            &self.services,
            &self.vehicle_access,
        )
    }

    pub fn label(&self) -> &'static str {
        lookup(STOP_TYPE_LABELS, &self.stop_type).unwrap_or("stop")
    }

    pub fn spoken_name(&self) -> String {
        format!("{}: {}", self.label(), self.name)
    }

    /// The parking certainty label; panics on an unknown certainty exactly
    /// where Python raised `KeyError`, which the parser never lets through.
    pub fn parking_label(&self) -> String {
        let label = lookup(PARKING_CERTAINTY_LABELS, &self.parking)
            .unwrap_or_else(|| panic!("unknown parking certainty {:?}", self.parking));
        if self.parking_spaces > 0 && (self.parking == "confirmed" || self.parking == "limited") {
            return format!("{label}, {} spaces", self.parking_spaces);
        }
        label.to_string()
    }

    pub fn curated(&self) -> bool {
        self.curation == "curated"
    }

    pub fn applies_to_direction(&self, forward: bool) -> bool {
        applies_to_direction(&self.directions, forward)
    }
}

fn applies_to_direction(directions: &[String], forward: bool) -> bool {
    if directions.iter().any(|d| d == "both") {
        return true;
    }
    let wanted = if forward { "forward" } else { "reverse" };
    directions.iter().any(|d| d == wanted)
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RoutePoint {
    pub at_mi: f64,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ElevationSample {
    pub at_mi: f64,
    pub elevation_ft: f64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GradeSegment {
    pub start_mi: f64,
    pub end_mi: f64,
    pub avg_grade_pct: f64,
    pub terrain: String,
    pub source: String,
}

impl GradeSegment {
    pub fn new(
        start_mi: f64,
        end_mi: f64,
        avg_grade_pct: f64,
        terrain: &str,
        source: &str,
    ) -> Self {
        GradeSegment {
            start_mi,
            end_mi,
            avg_grade_pct,
            terrain: terrain.to_string(),
            source: source.to_string(),
        }
    }
}

/// Spoken lane counts stay small; a lookup keeps the words natural for a
/// screen reader instead of a bare digit.
pub const LANE_WORD: &[(i64, &str)] = &[
    (1, "one"),
    (2, "two"),
    (3, "three"),
    (4, "four"),
    (5, "five"),
    (6, "six"),
    (7, "seven"),
    (8, "eight"),
];

pub fn lane_word(n: i64) -> String {
    LANE_WORD
        .iter()
        .find(|(k, _)| *k == n)
        .map(|(_, word)| word.to_string())
        .unwrap_or_else(|| n.to_string())
}

/// Real OSM lane count over `[start_mi, end_mi)`, mileposts in the leg's
/// native (a->b) direction. `lanes` follows OSM semantics: on a
/// divided-carriageway `oneway` way it is the count in that direction; on an
/// undivided two-way it is the total both ways. `lanes_forward` /
/// `lanes_backward` are the directional split when OSM tags it (0 = absent),
/// relative to the direction the OSM WAY was drawn in, which the bake never
/// compared with the leg's -- see [`LaneSegment::your_side`]. Baked by
/// `tools/bake_lane_segments.py`; the runtime never sees a raw OSM string.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaneSegment {
    pub start_mi: f64,
    pub end_mi: f64,
    pub lanes: i64,
    pub lanes_forward: i64,
    pub lanes_backward: i64,
    pub oneway: bool,
    pub source: String,
}

impl LaneSegment {
    pub fn divided(&self) -> bool {
        self.oneway
    }

    /// Lanes in the driver's direction of travel, whichever way the truck
    /// drives the leg.
    ///
    /// On an undivided road with an uneven split -- a two-lane highway with a
    /// passing lane one way, `lanes=3` with `lanes:forward=2` -- this is the
    /// SMALLER side, in both directions. OSM's forward means the way the way
    /// was drawn, and the bake stored it without checking that against the
    /// leg, so on about half of those stretches the truck was handed the
    /// oncoming traffic's passing lane, and full lane keeping pulled out into
    /// it to pass (player report, 2026-10-13: "the cruise control swerves you
    /// right into oncoming cars"). Until a bake records which way each way
    /// runs, the truck is never given a lane that may be theirs; what that
    /// costs is the real passing lane on the stretches where it is ours.
    pub fn your_side(&self) -> i64 {
        if self.oneway {
            // A divided carriageway: the tagged count is already one direction.
            return self.lanes;
        }
        let split = match (self.lanes_forward, self.lanes_backward) {
            (0, 0) => None,
            (forward, 0) => Some((forward, self.lanes - forward)),
            (0, backward) => Some((self.lanes - backward, backward)),
            (forward, backward) => Some((forward, backward)),
        };
        match split {
            Some((forward, backward)) => forward.min(backward).max(1),
            // Undivided two-way: split the total, floor at one lane your side.
            None => (self.lanes.div_euclid(2)).max(1),
        }
    }
}

/// A posted speed limit in effect from `at_mi` until the next sample.
///
/// Baked from real OpenStreetMap `maxspeed` tags at build time (see
/// `tools/enrich_routes.py`) and stored already normalized to mph, so the
/// runtime never sees a raw OSM string. The samples form a step function
/// along the leg: the limit at any mile is the last sample whose `at_mi` is at
/// or before it. `hgv` marks a truck-specific limit (`maxspeed:hgv`).
///
/// `mph` of `None` is a coverage-gap marker: OSM tagging ends here, so the
/// runtime reverts to the highway/region heuristic instead of holding the
/// previous posting -- without it a village 30 baked just before a tag hole
/// ruled miles of open highway (NY-12 out of Norwich, owner-relayed
/// 2026-07-19).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpeedLimitSample {
    pub at_mi: f64,
    pub mph: Option<f64>,
    pub source: String,
    pub hgv: bool,
}

/// What terrain FHWA HPMS says this leg's road runs through.
///
/// `terrain_type` (Python `type`) is HPMS's own Green Book class -- 1 level,
/// 2 rolling, 3 mountainous -- and is READ, not computed. What is derived is
/// that a single value stands for a whole leg: HPMS classifies road sections
/// and a leg crosses many, so this is the modal class over the sections the
/// leg touches, with `sections` recording how many were behind it.
///
/// Baked by `tools/build_terrain_type.py`. It exists because the world's own
/// `terrain` field is derived from net elevation change and calls Glenwood
/// Canyon flat; see `data/curves`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HpmsTerrain {
    pub terrain_type: i64,
    pub name: String,
    pub sections: i64,
    pub source: String,
}

/// Traffic volume in effect from `at_mi` until the next sample.
///
/// Baked from FHWA HPMS AADT data at build time (see
/// `tools/build_traffic_aadt.py`). `aadt` is annual average daily traffic
/// across both directions; `lanes` is through lanes *per direction* on the
/// sampled stretch. The samples form a step function along the leg, like
/// `SpeedLimitSample`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficVolumeSample {
    pub at_mi: f64,
    pub aadt: f64,
    pub lanes: i64,
    pub source: String,
}

impl Default for TrafficVolumeSample {
    fn default() -> Self {
        TrafficVolumeSample {
            at_mi: 0.0,
            aadt: 0.0,
            lanes: 2,
            source: String::new(),
        }
    }
}

/// A narratable roadside feature baked from OpenStreetMap.
///
/// `kind` is `"zone"` (a protected area you enter) or `"point"` (a spot you
/// pass); `category` is the finer bucket (`national_park`, `river`,
/// `mountain_pass`, `museum`, ...) that the roadside-chatter settings filter
/// on. `spoken` is the finished ambient cue line, authored at bake time so
/// the runtime never composes from raw tags.
///
/// `off_mi` is how far the feature sits off the road at `at_mi`. Village
/// callouts are baked out to a wide catchment and displayed on a tight one:
/// the ride-along names only the towns the route actually runs through, while
/// the wider set stays available to answer "what is near me" at any distance
/// (a town eleven miles ahead is the honest answer on an empty interstate).
/// Zone and point landmarks are on the route by construction and leave it 0.
///
/// `directions` is which way along the leg the callout is heard, in the
/// stops' vocabulary. A billboard faces one way and its copy says "ahead" or
/// "next exit", so a placed billboard is heard only forward unless the data
/// says otherwise; everything else is heard both ways. Empty means both.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Landmark {
    pub name: String,
    pub at_mi: f64,
    pub category: String,
    pub kind: String,
    pub spoken: String,
    pub off_mi: f64,
    pub directions: Vec<String>,
}

impl Landmark {
    pub fn applies_to_direction(&self, forward: bool) -> bool {
        self.directions.is_empty() || applies_to_direction(&self.directions, forward)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StateCrossing {
    pub at_mi: f64,
    pub from_state: String,
    pub state: String,
    pub place: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteCheckpoint {
    pub name: String,
    pub at_mi: f64,
    /// Python `type`: `place`, `highway_change` or `state_line`.
    pub checkpoint_type: String,
    pub state: String,
    pub highway: String,
    pub source: String,
}

impl Default for RouteCheckpoint {
    fn default() -> Self {
        RouteCheckpoint {
            name: String::new(),
            at_mi: 0.0,
            checkpoint_type: "place".to_string(),
            state: String::new(),
            highway: String::new(),
            source: String::new(),
        }
    }
}

impl RouteCheckpoint {
    pub fn new(name: &str, at_mi: f64, checkpoint_type: &str, state: &str, highway: &str) -> Self {
        RouteCheckpoint {
            name: name.to_string(),
            at_mi,
            checkpoint_type: checkpoint_type.to_string(),
            state: state.to_string(),
            highway: highway.to_string(),
            source: String::new(),
        }
    }

    pub fn label(&self) -> &'static str {
        match self.checkpoint_type.as_str() {
            "highway_change" => "highway change",
            "state_line" => "state line",
            _ => "corridor place",
        }
    }

    pub fn spoken_name(&self) -> String {
        format!("{}: {}", self.label(), self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StateMileage {
    pub state: String,
    pub miles: f64,
}

impl StateMileage {
    pub fn new(state: &str, miles: f64) -> Self {
        StateMileage {
            state: state.to_string(),
            miles,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TollEvent {
    pub name: String,
    pub at_mi: f64,
    pub road: String,
    pub authority: String,
    pub method: String,
    pub amount: f64,
    pub estimated: bool,
    pub source: String,
    /// What the same crossing costs without a transponder. Authorities charge
    /// a pay-by-plate rate that runs from identical (Delaware's I-95 plaza,
    /// the Chesapeake Bay Bridge-Tunnel) to double (Pennsylvania, Kansas,
    /// Oklahoma), so the gap is a real decision rather than a flat surcharge.
    /// Defaults to `amount` -- no penalty -- because "we have not researched
    /// the plate rate" must not silently invent one.
    pub amount_plate: f64,
    /// Which way you have to be going to be charged. Many crossings collect in
    /// one direction only and let the other side through free -- the
    /// Carquinez and Benicia-Martinez bridges, the Chesapeake Bay Bridge, the
    /// Delaware Memorial Bridge, Maryland's JFK Highway. A leg is driven both
    /// ways, so billing a one-way bridge in both directions doubles what the
    /// road really costs. Defaults to both, which is right for turnpikes and
    /// mainline barriers.
    pub directions: Vec<String>,
}

impl Default for TollEvent {
    fn default() -> Self {
        TollEvent {
            name: String::new(),
            at_mi: 0.0,
            road: String::new(),
            authority: String::new(),
            method: String::new(),
            amount: 0.0,
            estimated: true,
            source: String::new(),
            amount_plate: 0.0,
            directions: strings(&["both"]),
        }
    }
}

impl TollEvent {
    /// The pay-by-plate charge, falling back to the transponder rate.
    pub fn plate_amount(&self) -> f64 {
        if self.amount_plate > 0.0 {
            self.amount_plate
        } else {
            self.amount
        }
    }

    pub fn applies_to_direction(&self, forward: bool) -> bool {
        applies_to_direction(&self.directions, forward)
    }

    pub fn method_label(&self) -> String {
        lookup(TOLL_METHOD_LABELS, &self.method)
            .map(str::to_string)
            .unwrap_or_else(|| self.method.replace('_', " "))
    }

    pub fn spoken_name(&self) -> String {
        format!("toll point: {}", self.name)
    }
}

/// A posted clearance or weight advisory on the driven corridor.
///
/// Baked from OpenStreetMap `maxheight`/`maxweight` tags at build time (see
/// `tools/build_interchanges.py --restrictions`) and stored already
/// normalized: `feet` for a `low_clearance`, US short `tons` for a
/// `weight_limit`. Routing already avoids impassable restrictions, so these
/// are advisory signage a legal truck drives past -- the GPS speaks them
/// ahead like toll points; they never reroute or block.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RouteRestriction {
    pub at_mi: f64,
    /// `low_clearance` | `weight_limit`
    pub kind: String,
    pub feet: f64,
    pub tons: f64,
    pub source: String,
}

impl RouteRestriction {
    pub fn value_text(&self) -> String {
        if self.kind == "low_clearance" {
            let mut whole = py_int(self.feet);
            let mut inches = round_py_int((self.feet - whole as f64) * 12.0);
            if inches >= 12 {
                whole += 1;
                inches -= 12;
            }
            if inches != 0 {
                return format!("{whole} feet {inches} inches");
            }
            return format!("{whole} feet");
        }
        let tons = round_py_n(self.tons, 1);
        if tons == tons.trunc() {
            format!("{} tons", py_int(tons))
        } else {
            format!("{} tons", py_str_float(tons))
        }
    }

    pub fn kind_label(&self) -> &'static str {
        // "Low bridge", not "low clearance": the sign's own jargon read badly
        // over speech (owner report 2026-08-13, "posted whatever"), and the
        // thing a driver pictures is the bridge. Canonical noun in ontology.md.
        if self.kind == "low_clearance" {
            "low bridge"
        } else {
            "weight limit"
        }
    }

    pub fn spoken_ahead(&self) -> String {
        // The far call answers the only question a driver has about a sign
        // they cannot see: does it matter? Routing already refused anything
        // impassable (see the struct docs), so the honest answer is no, and
        // saying so is the difference between information and worry.
        format!(
            "a {}, signed {}. Your route clears it",
            self.kind_label(),
            self.value_text()
        )
    }

    pub fn spoken_near(&self) -> String {
        format!(
            "{}, signed {}.",
            py_capitalize(self.kind_label()),
            self.value_text()
        )
    }
}

/// Python `str.capitalize()`: first character upper, the rest lower.
pub fn py_capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out: String = first.to_uppercase().collect();
            out.push_str(&chars.as_str().to_lowercase());
            out
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocalApproach {
    pub target_id: String,
    pub target_type: String,
    pub city: String,
    pub name: String,
    pub approach_miles: f64,
    pub road: String,
    pub source_type: String,
    pub estimated: bool,
    pub fallback: bool,
    pub fallback_reason: String,
    pub distance_to_road_mi: f64,
    pub turn_segments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocalGeometry {
    pub target_id: String,
    pub target_type: String,
    pub city: String,
    pub name: String,
    pub turn_level: bool,
    pub source_type: String,
    pub estimated: bool,
    pub fallback: bool,
    pub fallback_reason: String,
    pub total_miles: f64,
    pub segments: Vec<LocalGeometrySegment>,
}

/// A freight service area.
///
/// `key` is the stable identity (`jackson_ms_us`): it keys `World.cities`,
/// leg endpoints, and saves, and is never spoken. `name` is the bare spoken
/// city ("Jackson") and `state` the spoken state name ("Mississippi"),
/// composed at load from the geo lookup; speech that must disambiguate uses
/// `spoken_qualified` or `World::spoken_city`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct City {
    pub name: String,
    pub state: String,
    pub region: String,
    pub locations: Vec<Location>,
    pub lat: f64,
    pub lon: f64,
    pub market_tags: Vec<String>,
    pub key: String,
    pub state_code: String,
    pub country: String,
    pub country_name: String,
}

impl City {
    pub fn spoken_qualified(&self) -> String {
        if self.state.is_empty() {
            self.name.clone()
        } else {
            format!("{}, {}", self.name, self.state)
        }
    }
}

impl fmt::Display for City {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.spoken_qualified())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restriction_value_text_reads_like_the_sign() {
        let low = RouteRestriction {
            at_mi: 1.0,
            kind: "low_clearance".into(),
            feet: 13.5,
            ..Default::default()
        };
        assert_eq!(low.value_text(), "13 feet 6 inches");
        assert_eq!(low.spoken_near(), "Low bridge, signed 13 feet 6 inches.");
        let weight = RouteRestriction {
            at_mi: 1.0,
            kind: "weight_limit".into(),
            tons: 12.0,
            ..Default::default()
        };
        assert_eq!(weight.value_text(), "12 tons");
        let half = RouteRestriction {
            tons: 12.5,
            kind: "weight_limit".into(),
            ..Default::default()
        };
        assert_eq!(half.value_text(), "12.5 tons");
        assert_eq!(
            half.spoken_ahead(),
            "a weight limit, signed 12.5 tons. Your route clears it"
        );
    }

    #[test]
    fn interchange_phrase_drops_the_via_restated_as_a_destination() {
        let exit = Interchange {
            at_mi: 3.0,
            exit_ref: "101A".into(),
            via: "I 70".into(),
            destinations: vec!["I 70 East".into(), "Trenton".into(), "New York".into()],
            ..Default::default()
        };
        assert_eq!(
            exit.spoken_phrase(),
            "exit 101A for I-70 toward Trenton and New York"
        );
        assert_eq!(
            exit.near_phrase(),
            "Exit 101A for I-70 toward Trenton and New York now."
        );
        assert_eq!(exit.exit_label(), "exit 101A");
        let unnamed = Interchange {
            name: "Mill Road".into(),
            ..Default::default()
        };
        assert_eq!(unnamed.spoken_phrase(), "exit for Mill Road");
        assert_eq!(
            format_route_ref("US 31 South;US 280"),
            "US-31 South and US-280"
        );
        assert_eq!(route_token("Interstate 40"), "");
        assert_eq!(route_token("US 1 North"), "US1");
    }

    #[test]
    fn lane_segment_your_side_follows_osm_semantics() {
        let undivided = LaneSegment {
            lanes: 3,
            ..Default::default()
        };
        assert_eq!(undivided.your_side(), 1);
        let divided = LaneSegment {
            lanes: 3,
            oneway: true,
            ..Default::default()
        };
        assert_eq!(divided.your_side(), 3);
        let split = LaneSegment {
            lanes: 5,
            lanes_forward: 3,
            lanes_backward: 2,
            ..Default::default()
        };
        assert_eq!(split.your_side(), 2);
        assert_eq!(lane_word(2), "two");
        assert_eq!(lane_word(11), "11");
    }
}
