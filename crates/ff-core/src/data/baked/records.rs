//! Serializable mirrors of the world model records.
//!
//! The model types in `data::world_models` deliberately carry no serde
//! derives: they are the runtime shapes, with Python's defaults and spoken
//! text methods, and nothing else in the game serializes them. The baked
//! container needs a wire form, so every record it stores gets a mirror
//! struct here with exactly the same fields, plus the two conversions.
//!
//! Same fields, and the compiler enforces it: a mirror missing a field will
//! not build the model back (the struct literal needs every field), and a
//! mirror with a field the model dropped will not build either. That is the
//! whole reason the mirrors are spelled out rather than generated from a
//! `Value` -- a field added to a model record breaks the bake at compile
//! time instead of silently vanishing from the shipped data.

use serde::{Deserialize, Serialize};

use crate::data::curves::CurveRecord;
use crate::data::world_local_data::CityServiceEntry;
use crate::data::world_models::{
    BillboardBan, City, CorridorDetail, Driveway, ElevationSample, ExitChain, FacilityApproach,
    FacilityEndpoint, GradeSegment, HpmsTerrain, Interchange, Landmark, LaneSegment, LocalApproach,
    LocalGeometry, LocalGeometrySegment, Location, RouteCheckpoint, RoutePoint, RouteRestriction,
    SpeedLimitSample, StateCrossing, StateMileage, Stop, StreetControl, StreetLimit, TollEvent,
    TrafficVolumeSample,
};

/// A mirror struct with the same field names and types as its model, and the
/// two conversions. Flat records only: a record holding another record is
/// written out by hand below, because the field types differ there.
macro_rules! mirror {
    ($name:ident => $target:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct $name {
            $(pub $field: $ty,)*
        }

        impl From<&$target> for $name {
            fn from(value: &$target) -> Self {
                $name { $($field: value.$field.clone(),)* }
            }
        }

        impl From<$name> for $target {
            fn from(value: $name) -> Self {
                $target { $($field: value.$field,)* }
            }
        }
    };
}

/// `&[Model] -> Vec<Mirror>`.
pub fn to_mirror<'a, T: 'a, M: From<&'a T>>(items: &'a [T]) -> Vec<M> {
    items.iter().map(M::from).collect()
}

/// `Vec<Mirror> -> Vec<Model>`.
pub fn from_mirror<M, T: From<M>>(items: Vec<M>) -> Vec<T> {
    items.into_iter().map(T::from).collect()
}

// ------------------------------------------------------------------ corridor

mirror!(BakedRoutePoint => RoutePoint { at_mi: f64, lat: f64, lon: f64 });

mirror!(BakedElevationSample => ElevationSample {
    at_mi: f64, elevation_ft: f64, source: String,
});

mirror!(BakedGradeSegment => GradeSegment {
    start_mi: f64, end_mi: f64, avg_grade_pct: f64, terrain: String, source: String,
});

mirror!(BakedStateCrossing => StateCrossing {
    at_mi: f64, from_state: String, state: String, place: String, source: String,
});

mirror!(BakedRouteCheckpoint => RouteCheckpoint {
    name: String, at_mi: f64, checkpoint_type: String, state: String,
    highway: String, source: String,
});

mirror!(BakedStateMileage => StateMileage { state: String, miles: f64 });

mirror!(BakedTollEvent => TollEvent {
    name: String, at_mi: f64, road: String, authority: String, method: String,
    amount: f64, estimated: bool, source: String, amount_plate: f64,
    directions: Vec<String>,
});

mirror!(BakedInterchange => Interchange {
    at_mi: f64, exit_ref: String, name: String, destinations: Vec<String>,
    via: String, highway: String, source: String, ramp_control: String,
    ramp_far_end: String, ramp_advisory_mph_forward: Option<f64>,
    ramp_advisory_mph_backward: Option<f64>, ramp_advisory_source: String,
    ramp_length_ft_forward: Option<f64>, ramp_length_ft_backward: Option<f64>,
    ramp_length_source: String, ramp_terminal_node_forward: Option<i64>,
    ramp_terminal_node_backward: Option<i64>, ramp_terminal_source: String,
});

mirror!(BakedSpeedLimitSample => SpeedLimitSample {
    at_mi: f64, mph: Option<f64>, source: String, hgv: bool,
});

mirror!(BakedTrafficVolumeSample => TrafficVolumeSample {
    at_mi: f64, aadt: f64, lanes: i64, source: String,
});

mirror!(BakedHpmsTerrain => HpmsTerrain {
    terrain_type: i64, name: String, sections: i64, source: String,
});

mirror!(BakedLandmark => Landmark {
    name: String, at_mi: f64, category: String, kind: String, spoken: String,
    off_mi: f64, directions: Vec<String>,
});

mirror!(BakedRouteRestriction => RouteRestriction {
    at_mi: f64, kind: String, feet: f64, tons: f64, source: String,
});

mirror!(BakedBillboardBan => BillboardBan {
    from_mi: f64, to_mi: f64, name: String, source: String,
});

mirror!(BakedLaneSegment => LaneSegment {
    start_mi: f64, end_mi: f64, lanes: i64, lanes_forward: i64,
    lanes_backward: i64, oneway: bool, source: String,
});

/// The whole deferred half of one leg: the per-mile record lists
/// `CorridorDetail` holds, in one blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedCorridor {
    pub route_points: Vec<BakedRoutePoint>,
    pub elevation_samples: Vec<BakedElevationSample>,
    pub grade_segments: Vec<BakedGradeSegment>,
    pub state_crossings: Vec<BakedStateCrossing>,
    pub checkpoints: Vec<BakedRouteCheckpoint>,
    pub state_miles: Vec<BakedStateMileage>,
    pub toll_events: Vec<BakedTollEvent>,
    pub interchanges: Vec<BakedInterchange>,
    pub speed_limits: Vec<BakedSpeedLimitSample>,
    pub traffic_volumes: Vec<BakedTrafficVolumeSample>,
    pub hpms_terrain: Option<BakedHpmsTerrain>,
    pub landmarks: Vec<BakedLandmark>,
    pub restrictions: Vec<BakedRouteRestriction>,
    pub lane_segments: Vec<BakedLaneSegment>,
    #[serde(default)]
    pub billboard_bans: Vec<BakedBillboardBan>,
    #[serde(default)]
    pub speed_limits_folded: u32,
}

impl From<&CorridorDetail> for BakedCorridor {
    fn from(detail: &CorridorDetail) -> Self {
        BakedCorridor {
            route_points: to_mirror(&detail.route_points),
            elevation_samples: to_mirror(&detail.elevation_samples),
            grade_segments: to_mirror(&detail.grade_segments),
            state_crossings: to_mirror(&detail.state_crossings),
            checkpoints: to_mirror(&detail.checkpoints),
            state_miles: to_mirror(&detail.state_miles),
            toll_events: to_mirror(&detail.toll_events),
            interchanges: to_mirror(&detail.interchanges),
            speed_limits: to_mirror(&detail.speed_limits),
            traffic_volumes: to_mirror(&detail.traffic_volumes),
            hpms_terrain: detail.hpms_terrain.as_ref().map(BakedHpmsTerrain::from),
            landmarks: to_mirror(&detail.landmarks),
            restrictions: to_mirror(&detail.restrictions),
            lane_segments: to_mirror(&detail.lane_segments),
            billboard_bans: to_mirror(&detail.billboard_bans),
            speed_limits_folded: u32::try_from(detail.speed_limits_folded).unwrap_or(u32::MAX),
        }
    }
}

impl From<BakedCorridor> for CorridorDetail {
    fn from(baked: BakedCorridor) -> Self {
        CorridorDetail {
            route_points: from_mirror(baked.route_points),
            elevation_samples: from_mirror(baked.elevation_samples),
            grade_segments: from_mirror(baked.grade_segments),
            state_crossings: from_mirror(baked.state_crossings),
            checkpoints: from_mirror(baked.checkpoints),
            state_miles: from_mirror(baked.state_miles),
            toll_events: from_mirror(baked.toll_events),
            interchanges: from_mirror(baked.interchanges),
            speed_limits: from_mirror(baked.speed_limits),
            traffic_volumes: from_mirror(baked.traffic_volumes),
            hpms_terrain: baked.hpms_terrain.map(HpmsTerrain::from),
            landmarks: from_mirror(baked.landmarks),
            restrictions: from_mirror(baked.restrictions),
            lane_segments: from_mirror(baked.lane_segments),
            billboard_bans: from_mirror(baked.billboard_bans),
            speed_limits_folded: baked.speed_limits_folded as usize,
        }
    }
}

// --------------------------------------------------------------------- eager

/// Holds `BakedExitChain`s, so it is spelled out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedStop {
    pub name: String,
    pub at_mi: f64,
    pub stop_type: String,
    pub source: String,
    pub actions: Vec<String>,
    pub services: Vec<String>,
    pub parking: String,
    pub directions: Vec<String>,
    pub curation: String,
    pub parking_spaces: i64,
    pub vehicle_access: String,
    pub exit_ref: String,
    pub interchange_mi: Option<f64>,
    pub approach_chains: Vec<BakedExitChain>,
}

impl From<&Stop> for BakedStop {
    fn from(value: &Stop) -> Self {
        BakedStop {
            name: value.name.clone(),
            at_mi: value.at_mi,
            stop_type: value.stop_type.clone(),
            source: value.source.clone(),
            actions: value.actions.clone(),
            services: value.services.clone(),
            parking: value.parking.clone(),
            directions: value.directions.clone(),
            curation: value.curation.clone(),
            parking_spaces: value.parking_spaces,
            vehicle_access: value.vehicle_access.clone(),
            exit_ref: value.exit_ref.clone(),
            interchange_mi: value.interchange_mi,
            approach_chains: to_mirror(&value.approach_chains),
        }
    }
}

impl From<BakedStop> for Stop {
    fn from(value: BakedStop) -> Self {
        Stop {
            name: value.name,
            at_mi: value.at_mi,
            stop_type: value.stop_type,
            source: value.source,
            actions: value.actions,
            services: value.services,
            parking: value.parking,
            directions: value.directions,
            curation: value.curation,
            parking_spaces: value.parking_spaces,
            vehicle_access: value.vehicle_access,
            exit_ref: value.exit_ref,
            interchange_mi: value.interchange_mi,
            approach_chains: from_mirror(value.approach_chains),
        }
    }
}

mirror!(BakedLocation => Location {
    name: String, facility_type: String, cargo: Vec<String>, id: String,
    city: String, locality: String, roles: Vec<String>, ships: Vec<String>,
    receives: Vec<String>, lat: f64, lon: f64, traits: Vec<String>,
    source_note: String, spoken: String, template: bool, min_level: i64,
});

/// One city with its freight facilities, already through `parse_location`,
/// the market-template expansion and the facility validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedCity {
    pub name: String,
    pub state: String,
    pub region: String,
    pub locations: Vec<BakedLocation>,
    pub lat: f64,
    pub lon: f64,
    pub market_tags: Vec<String>,
    pub key: String,
    pub state_code: String,
    pub country: String,
    pub country_name: String,
}

impl From<&City> for BakedCity {
    fn from(city: &City) -> Self {
        BakedCity {
            name: city.name.clone(),
            state: city.state.clone(),
            region: city.region.clone(),
            locations: to_mirror(&city.locations),
            lat: city.lat,
            lon: city.lon,
            market_tags: city.market_tags.clone(),
            key: city.key.clone(),
            state_code: city.state_code.clone(),
            country: city.country.clone(),
            country_name: city.country_name.clone(),
        }
    }
}

impl From<BakedCity> for City {
    fn from(baked: BakedCity) -> Self {
        City {
            name: baked.name,
            state: baked.state,
            region: baked.region,
            locations: from_mirror(baked.locations),
            lat: baked.lat,
            lon: baked.lon,
            market_tags: baked.market_tags,
            key: baked.key,
            state_code: baked.state_code,
            country: baked.country,
            country_name: baked.country_name,
        }
    }
}

/// A leg's eager half plus where its corridor blob sits.
///
/// `corridor_offset` is relative to the start of the corridor section, so the
/// section can move without rewriting every leg.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedLeg {
    pub a: String,
    pub b: String,
    pub miles: f64,
    pub highway: String,
    pub terrain: String,
    pub stops: Vec<BakedStop>,
    pub truck_advisory: String,
    pub lanes: i64,
    pub local_cue: String,
    pub local_speed_mph: f64,
    pub local_turn_deg: f64,
    pub divided: Option<bool>,
    pub meta_complete: Option<bool>,
    pub corridor_offset: u64,
    pub corridor_stored_len: u32,
    pub corridor_raw_len: u32,
}

// ---------------------------------------------------------------- side maps

mirror!(BakedCityServiceEntry => CityServiceEntry {
    key: String, name: String, kind: Option<String>, source_note: String,
    lat: f64, lon: f64, approach_miles: f64, approach_road: String,
    source_type: Option<String>, source_ref: String, fallback: bool,
    fallback_reason: String, city: String, state: String,
});

mirror!(BakedFacilityEndpoint => FacilityEndpoint {
    facility_id: String, city: String, state: String, facility_name: String,
    facility_type: String, endpoint_name: String, source_type: String,
    source_note: String, lat: f64, lon: f64, approach_miles: f64,
    approach_road: String, source_ref: String, source_backed: bool,
    fallback: bool, fallback_reason: String, nearest_road_context: bool,
    turn_level_geometry: bool, gate_hint: bool, yard_hint: bool,
    dock_hint: bool, mapping: String,
});

mirror!(BakedStreetLimit => StreetLimit { mph: f64, source: String, basis: String });

mirror!(BakedStreetControl => StreetControl { at_mi: f64, kind: String });

mirror!(BakedDriveway => Driveway {
    at_mi: f64, lat: f64, lon: f64, kind: String, source: String,
});

/// Holds a `BakedStreetLimit` and `BakedStreetControl`s, so it is spelled out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedLocalGeometrySegment {
    pub road: String,
    pub miles: f64,
    pub cue: String,
    pub speed_mph: f64,
    pub turn_deg: f64,
    pub limit: Option<BakedStreetLimit>,
    pub controls: Vec<BakedStreetControl>,
}

impl From<&LocalGeometrySegment> for BakedLocalGeometrySegment {
    fn from(value: &LocalGeometrySegment) -> Self {
        BakedLocalGeometrySegment {
            road: value.road.clone(),
            miles: value.miles,
            cue: value.cue.clone(),
            speed_mph: value.speed_mph,
            turn_deg: value.turn_deg,
            limit: value.limit.as_ref().map(BakedStreetLimit::from),
            controls: to_mirror(&value.controls),
        }
    }
}

impl From<BakedLocalGeometrySegment> for LocalGeometrySegment {
    fn from(value: BakedLocalGeometrySegment) -> Self {
        LocalGeometrySegment {
            road: value.road,
            miles: value.miles,
            cue: value.cue,
            speed_mph: value.speed_mph,
            turn_deg: value.turn_deg,
            limit: value.limit.map(StreetLimit::from),
            controls: from_mirror(value.controls),
        }
    }
}

/// Holds segments and a driveway, so it is spelled out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedExitChain {
    pub terminal_node: i64,
    pub total_miles: f64,
    pub segments: Vec<BakedLocalGeometrySegment>,
    pub driveway: Option<BakedDriveway>,
}

impl From<&ExitChain> for BakedExitChain {
    fn from(value: &ExitChain) -> Self {
        BakedExitChain {
            terminal_node: value.terminal_node,
            total_miles: value.total_miles,
            segments: to_mirror(&value.segments),
            driveway: value.driveway.as_ref().map(BakedDriveway::from),
        }
    }
}

impl From<BakedExitChain> for ExitChain {
    fn from(value: BakedExitChain) -> Self {
        ExitChain {
            terminal_node: value.terminal_node,
            total_miles: value.total_miles,
            segments: from_mirror(value.segments),
            driveway: value.driveway.map(Driveway::from),
        }
    }
}

mirror!(BakedLocalApproach => LocalApproach {
    target_id: String, target_type: String, city: String, name: String,
    approach_miles: f64, road: String, source_type: String, estimated: bool,
    fallback: bool, fallback_reason: String, distance_to_road_mi: f64,
    turn_segments: Vec<String>,
});

/// Holds `BakedLocalGeometrySegment`s, so it is spelled out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedLocalGeometry {
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
    pub segments: Vec<BakedLocalGeometrySegment>,
}

impl From<&LocalGeometry> for BakedLocalGeometry {
    fn from(value: &LocalGeometry) -> Self {
        BakedLocalGeometry {
            target_id: value.target_id.clone(),
            target_type: value.target_type.clone(),
            city: value.city.clone(),
            name: value.name.clone(),
            turn_level: value.turn_level,
            source_type: value.source_type.clone(),
            estimated: value.estimated,
            fallback: value.fallback,
            fallback_reason: value.fallback_reason.clone(),
            total_miles: value.total_miles,
            segments: to_mirror(&value.segments),
        }
    }
}

impl From<BakedLocalGeometry> for LocalGeometry {
    fn from(value: BakedLocalGeometry) -> Self {
        LocalGeometry {
            target_id: value.target_id,
            target_type: value.target_type,
            city: value.city,
            name: value.name,
            turn_level: value.turn_level,
            source_type: value.source_type,
            estimated: value.estimated,
            fallback: value.fallback,
            fallback_reason: value.fallback_reason,
            total_miles: value.total_miles,
            segments: from_mirror(value.segments),
        }
    }
}

/// Holds `BakedLocalGeometrySegment`s, so it is spelled out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BakedFacilityApproach {
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
    pub segments: Vec<BakedLocalGeometrySegment>,
    pub gate_hint: bool,
    pub yard_hint: bool,
    pub dock_hint: bool,
    pub final_hint: String,
    pub source_note: String,
    pub driveway: Option<BakedDriveway>,
    pub exit_chains: Vec<BakedExitChain>,
}

impl From<&FacilityApproach> for BakedFacilityApproach {
    fn from(value: &FacilityApproach) -> Self {
        BakedFacilityApproach {
            facility_id: value.facility_id.clone(),
            city: value.city.clone(),
            state: value.state.clone(),
            facility_name: value.facility_name.clone(),
            facility_type: value.facility_type.clone(),
            endpoint_name: value.endpoint_name.clone(),
            endpoint_source_backed: value.endpoint_source_backed,
            road_snapped: value.road_snapped,
            turn_level: value.turn_level,
            source_type: value.source_type.clone(),
            estimated: value.estimated,
            fallback: value.fallback,
            fallback_reason: value.fallback_reason.clone(),
            nearest_road_context: value.nearest_road_context,
            representative_fallback: value.representative_fallback,
            total_miles: value.total_miles,
            approach_road: value.approach_road.clone(),
            segments: to_mirror(&value.segments),
            gate_hint: value.gate_hint,
            yard_hint: value.yard_hint,
            dock_hint: value.dock_hint,
            final_hint: value.final_hint.clone(),
            source_note: value.source_note.clone(),
            driveway: value.driveway.as_ref().map(BakedDriveway::from),
            exit_chains: to_mirror(&value.exit_chains),
        }
    }
}

impl From<BakedFacilityApproach> for FacilityApproach {
    fn from(value: BakedFacilityApproach) -> Self {
        FacilityApproach {
            facility_id: value.facility_id,
            city: value.city,
            state: value.state,
            facility_name: value.facility_name,
            facility_type: value.facility_type,
            endpoint_name: value.endpoint_name,
            endpoint_source_backed: value.endpoint_source_backed,
            road_snapped: value.road_snapped,
            turn_level: value.turn_level,
            source_type: value.source_type,
            estimated: value.estimated,
            fallback: value.fallback,
            fallback_reason: value.fallback_reason,
            nearest_road_context: value.nearest_road_context,
            representative_fallback: value.representative_fallback,
            total_miles: value.total_miles,
            approach_road: value.approach_road,
            segments: from_mirror(value.segments),
            gate_hint: value.gate_hint,
            yard_hint: value.yard_hint,
            dock_hint: value.dock_hint,
            final_hint: value.final_hint,
            source_note: value.source_note,
            driveway: value.driveway.map(Driveway::from),
            exit_chains: from_mirror(value.exit_chains),
        }
    }
}

// ------------------------------------------------------------------- curves

mirror!(BakedCurveRecord => CurveRecord {
    start_mi: f64, apex_mi: f64, end_mi: f64, direction: char,
    advisory_mph: i64, min_radius_ft: i64, deflection_deg: f64,
    connector: bool,
});
