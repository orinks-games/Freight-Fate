//! World model: cities, freight locations, and the highway network (port of
//! `freight_fate/data/world.py`).
//!
//! Loads indexed world data and exposes a graph with Dijkstra-based route
//! finding. Route options are produced by re-running the search with
//! already-used legs penalized, giving genuinely different alternatives
//! (fastest vs. detour).

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde_json::Value;

use super::baked::{BakedData, CorridorRef};
use super::border_yards::border_yards_at;
use super::branded_plazas::screen_branded_plazas;
use super::data_resources::{baked_at, data_root, read_text_at};
use super::legacy_aliases::LEGACY_CITY_SLUGS;
use super::stop_twins::screen_twin_stops;
use super::world_constants::{
    ALTERNATE_ROUTE_EXTRA_RATIO, ALTERNATE_ROUTE_MAX_EXTRA_MILES, ALTERNATE_ROUTE_MIN_EXTRA_MILES,
};
use super::world_corridor::{raw_metadata_complete, validate_tunnel_raw};
use super::world_loader::{load_world_data, WorldData};
use super::world_local_data::{
    load_city_service_data, load_facility_approaches, load_facility_endpoints,
    load_local_approaches, load_local_geometries, CityServiceData,
};
use super::world_models::{
    City, CorridorDetail, CrossingLoad, DataError, DetailSource, FacilityApproach,
    FacilityEndpoint, Leg, LocalApproach, LocalGeometry, Location, RoadCrossing, Route,
};
use super::world_parsing::{
    expand_market_locations, is_legacy_market_name, market_tags_for_city, merge_overlay,
    parse_location, parse_stop, py_float, py_int_of, py_str,
};

mod builders;
use builders::{
    build_city_aliases, build_legacy_facility_ids, build_service_city_keys, city_identity,
    py_float_or, validate_city_locations,
};

/// The `world_data/` tree under the data root (Python `WORLD_DATA_PATH`).
pub fn world_data_path() -> PathBuf {
    data_root().join("world_data")
}

/// The leg a hop from `a` to `b` drives, among those `allowed`: the main
/// road's record for this direction, else its other-direction record, else
/// a detour leg. Both directions may carry separate corridor records.
fn pick_hop<'l>(
    legs: &'l [Arc<Leg>],
    a: &str,
    b: &str,
    allowed: impl Fn(&Arc<Leg>) -> bool,
) -> Option<&'l Arc<Leg>> {
    let main = |leg: &&Arc<Leg>| leg.detour_for.is_empty() && allowed(leg);
    legs.iter()
        .filter(main)
        .find(|leg| leg.a.as_str() == a && leg.b.as_str() == b)
        .or_else(|| legs.iter().filter(main).find(|leg| leg.other(a) == b))
        .or_else(|| {
            legs.iter()
                .filter(|leg| allowed(leg))
                .find(|leg| leg.other(a) == b)
        })
}

/// Search cost added to a legal detour leg, in miles. It runs between the
/// same two cities as the road it bypasses; on a tie the main road wins.
const DETOUR_TIE_BREAK_MILES: f64 = 0.01;

/// Routing cost multiplier for a leg carrying a truck_advisory (see
/// `shortest_route`). Calibrated, not tasted: carriers accept the ~1.7x
/// distance detour through Cortez and Moab rather than run the warned
/// US-550 passes, so any factor clearing that ratio encodes the observed
/// decision; 2.5 clears it with margin while a pair of towns whose only
/// road is the warned one still routes.
pub const TRUCK_ADVISORY_COST_MULT: f64 = 2.5;

/// Per-leg routing penalty multipliers, keyed by `Leg::id`.
pub type Penalties = HashMap<usize, f64>;

type Lazy<T> = OnceCell<Result<T, DataError>>;

/// The loaded world: every city and leg, the alias tables that keep old
/// saves resolving, and the route graph. Immutable once built; the lazy
/// local-driving data and the supported-route cache are interior.
pub struct World {
    pub cities: IndexMap<String, City>,
    pub legs: Vec<Arc<Leg>>,
    facilities_by_id: HashMap<String, Location>,
    city_aliases: HashMap<String, String>,
    ambiguous_spoken: HashSet<String>,
    legacy_names_by_key: IndexMap<String, Vec<String>>,
    legacy_facility_ids: HashMap<String, String>,
    service_city_keys: HashMap<String, String>,
    adjacency: HashMap<String, Vec<Arc<Leg>>>,
    supported_route_cache: Mutex<HashMap<(String, String), Option<Route>>>,
    /// Where the loose runtime data files live (`city_services.json`, ...).
    data_dir: PathBuf,
    // The nationwide local-driving data (city services, facility endpoints
    // and approaches, surface-street geometry) is ~31 MB of JSON that only
    // matters once a player is threading into a specific facility or city
    // service. Loading and remapping it at startup was pure latency; it is
    // built on first access instead. The remap keys off structures already
    // built above, so a lazy build is safe.
    city_service_data_cache: Lazy<CityServiceData>,
    facility_approaches_cache: Lazy<IndexMap<String, FacilityApproach>>,
    facility_endpoints_cache: Lazy<IndexMap<String, FacilityEndpoint>>,
    local_approaches_cache: Lazy<IndexMap<String, LocalApproach>>,
    local_geometries_cache: Lazy<IndexMap<String, LocalGeometry>>,
    /// The baked container this world came out of, kept alive because every
    /// lazy leg's corridor builder reads through its mapping. None on the
    /// JSON path, which is every source checkout and every test.
    baked: Option<Arc<BakedData>>,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "World({} cities, {} legs)",
            self.cities.len(),
            self.legs.len()
        )
    }
}

impl World {
    /// Build the world from raw data; local-driving files are read lazily
    /// from `data_dir`.
    pub fn from_data_at(data: WorldData, data_dir: PathBuf) -> Result<World, DataError> {
        let countries = data.geo.unwrap_or_default().countries;
        let mut cities: IndexMap<String, City> = IndexMap::new();
        let mut facilities_by_id: HashMap<String, Location> = HashMap::new();
        let border = border_yards_at(&data_dir)?;
        for (key, c) in &data.cities {
            let lat = py_float_or(&c.lat, 0.0)?;
            let lon = py_float_or(&c.lon, 0.0)?;
            let identity = city_identity(key, c, &countries);
            let explicit_locs = c
                .locations
                .iter()
                .map(|loc| parse_location(loc, key, &identity.spoken_city, lat, lon))
                .collect::<Result<Vec<_>, _>>()?;
            let tags = market_tags_for_city(key, &identity.state_code, c, &explicit_locs);
            let mut locs = if c.no_freight {
                explicit_locs
            } else {
                expand_market_locations(key, &identity.spoken_city, lat, lon, &explicit_locs, &tags)
            };
            // Border transfer yards join their city after the market
            // expansion, so they add to its freight rather than replacing
            // the facility types the endpoint sweep already pinned.
            if let Some(border) = border.as_ref().filter(|b| &b.city == key) {
                for raw in border.location_values() {
                    locs.push(parse_location(&raw, key, &identity.spoken_city, lat, lon)?);
                }
            }
            validate_city_locations(
                &identity.spoken_city,
                &locs,
                c.no_freight,
                &mut facilities_by_id,
            )?;
            cities.insert(
                key.clone(),
                City {
                    name: identity.spoken_city,
                    state: identity.state_name,
                    region: c.region.clone(),
                    locations: locs,
                    no_freight: c.no_freight,
                    lat,
                    lon,
                    market_tags: tags,
                    key: key.clone(),
                    state_code: identity.state_code,
                    country: identity.country_code,
                    country_name: identity.country_name,
                },
            );
        }
        let (city_aliases, ambiguous_spoken) = build_city_aliases(&cities);
        let mut legacy_names_by_key: IndexMap<String, Vec<String>> = IndexMap::new();
        for (old_name, slug) in LEGACY_CITY_SLUGS {
            if cities.contains_key(*slug) && old_name != slug {
                legacy_names_by_key
                    .entry(slug.to_string())
                    .or_default()
                    .push(old_name.to_string());
            }
        }
        let legacy_facility_ids = build_legacy_facility_ids(&cities, &legacy_names_by_key);
        // The checked-in local-driving data (city services, facility endpoints
        // and approaches, surface-street geometry) predates the slug migration
        // and is keyed by old display names and pre-slug facility ids; remap it
        // onto canonical keys once at load so every runtime lookup stays direct.
        let service_city_keys = build_service_city_keys(&cities, &legacy_names_by_key);

        let resolve = |city: &str| -> String {
            let text = city.trim();
            if cities.contains_key(text) {
                return text.to_string();
            }
            city_aliases
                .get(text)
                .cloned()
                .unwrap_or_else(|| text.to_string())
        };

        let mut legs: Vec<Arc<Leg>> = Vec::with_capacity(data.legs.len());
        for (index, leg) in data.legs.into_iter().enumerate() {
            // Endpoints resolve through the alias table so additive overlays
            // written against pre-slug names keep merging.
            let leg_from = resolve(&leg.from);
            let leg_to = resolve(&leg.to);
            let miles = py_float(&leg.miles)?;
            let highway = leg.highway;
            let stops = leg
                .stops
                .iter()
                .map(|s| parse_stop(s, miles, &leg_from, &leg_to))
                .collect::<Result<Vec<_>, _>>()?;
            // One chain truck stop listed twice is one stop (`stop_twins`).
            let stops = screen_twin_stops(stops);
            // A chain truck stop typed as a toll road's service plaza is a
            // travel center (`branded_plazas`). After the twin screen, so
            // which twin it keeps is decided on the recorded types.
            let stops = screen_branded_plazas(stops);
            let corridor = leg.corridor;
            validate_tunnel_raw(&corridor, &leg_from, &leg_to)?;
            let from_state = cities
                .get(&leg_from)
                .ok_or_else(|| DataError::key(format!("Unknown city: {leg_from}")))?
                .state
                .clone();
            let to_state = cities
                .get(&leg_to)
                .ok_or_else(|| DataError::key(format!("Unknown city: {leg_to}")))?
                .state
                .clone();
            // Only the eager fields are built now; the heavy per-mile corridor
            // (grades, interchanges, landmarks, speed limits, ...) is parsed by
            // the lazy leg the first time a leg is driven. Dispatch completeness
            // is baked here from raw corridor counts so the route graph never has
            // to trigger that parse -- the counted fields parse one-for-one, so
            // this is identical to asking a fully built leg.
            let meta_complete = raw_metadata_complete(&corridor, &from_state, &to_state);
            let lanes = match &leg.lanes {
                Value::Null => 0,
                v => py_int_of(v)?.max(0),
            };
            let divided = match leg.divided {
                Value::Bool(b) => Some(b),
                _ => None,
            };
            let truck_advisory = match &leg.truck_advisory {
                Value::Null => String::new(),
                v => py_str(v).trim().to_string(),
            };
            let detour_for = match &leg.detour_for {
                Value::Null => String::new(),
                v => py_str(v).trim().to_string(),
            };
            let crossings: Vec<RoadCrossing> = match &leg.crossings {
                Value::Null => Vec::new(),
                v => serde_json::from_value(v.clone()).map_err(|e| {
                    DataError::value(format!("{leg_from} to {leg_to} has invalid crossings: {e}"))
                })?,
            };
            for crossing in &crossings {
                crossing.validate(&format!("{leg_from} to {leg_to}"))?;
            }
            let mut built = Leg::lazy(
                &leg_from,
                &leg_to,
                miles,
                &highway,
                &leg.terrain,
                stops,
                DetailSource {
                    corridor,
                    miles,
                    leg_from: leg_from.clone(),
                    leg_to: leg_to.clone(),
                    from_state,
                    highway: highway.clone(),
                },
            );
            built.id = index;
            built.lanes = lanes;
            built.divided = divided;
            built.truck_advisory = truck_advisory;
            built.crossings = crossings;
            built.detour_for = detour_for;
            built.meta_complete = Some(meta_complete);
            legs.push(Arc::new(built));
        }
        let mut adjacency: HashMap<String, Vec<Arc<Leg>>> = cities
            .keys()
            .map(|name| (name.clone(), Vec::new()))
            .collect();
        for leg in &legs {
            adjacency
                .get_mut(&leg.a)
                .ok_or_else(|| DataError::key(format!("Unknown city: {}", leg.a)))?
                .push(Arc::clone(leg));
            adjacency
                .get_mut(&leg.b)
                .ok_or_else(|| DataError::key(format!("Unknown city: {}", leg.b)))?
                .push(Arc::clone(leg));
        }
        Ok(World {
            cities,
            legs,
            facilities_by_id,
            city_aliases,
            ambiguous_spoken,
            legacy_names_by_key,
            legacy_facility_ids,
            service_city_keys,
            adjacency,
            supported_route_cache: Mutex::new(HashMap::new()),
            data_dir,
            city_service_data_cache: OnceCell::new(),
            facility_approaches_cache: OnceCell::new(),
            facility_endpoints_cache: OnceCell::new(),
            local_approaches_cache: OnceCell::new(),
            local_geometries_cache: OnceCell::new(),
            baked: None,
        })
    }

    /// Build the world from raw data (Python `World(data)`), with the lazy
    /// local-driving files read from the default data root.
    pub fn from_data(data: WorldData) -> Result<World, DataError> {
        World::from_data_at(data, data_root().to_path_buf())
    }

    /// Build from an in-memory JSON value (test fixtures).
    pub fn from_value(value: Value) -> Result<World, DataError> {
        World::from_data(WorldData::from_value(value)?)
    }

    /// Load the shipped world from the default data root.
    pub fn load() -> Result<World, DataError> {
        World::load_from(data_root())
    }

    /// Load the world whose data lives under `data_dir` (the Python
    /// package's `data/` folder): the baked container if one is there, the
    /// `world_data/` JSON tree otherwise.
    pub fn load_from(data_dir: &Path) -> Result<World, DataError> {
        World::load_with_overlay(data_dir, None)
    }

    /// Load from the JSON tree, ignoring any baked container beside it.
    ///
    /// The baker calls this: the container is built from what the JSON
    /// loaders produce, so baking must never read a container.
    pub fn load_from_json(data_dir: &Path) -> Result<World, DataError> {
        World::load_json_with_overlay(data_dir, None)
    }

    /// Build the world out of an opened baked container.
    ///
    /// The container already holds cities as `World::from_data_at` finished
    /// them -- parsed, market-expanded, validated -- and legs with their
    /// eager fields and a corridor offset each. What is rebuilt here is only
    /// the derived tables (facility index, alias maps, adjacency), which are
    /// pure in-memory work over data already in hand: a few milliseconds, and
    /// no second source of truth to drift.
    pub fn from_baked(container: Arc<BakedData>, data_dir: PathBuf) -> Result<World, DataError> {
        let baked_cities = container.take_cities()?;
        let mut cities: IndexMap<String, City> = IndexMap::with_capacity(baked_cities.len());
        let mut facilities_by_id: HashMap<String, Location> = HashMap::new();
        for baked in baked_cities {
            let city = City::from(baked);
            validate_city_locations(
                &city.name,
                &city.locations,
                city.no_freight,
                &mut facilities_by_id,
            )?;
            cities.insert(city.key.clone(), city);
        }
        let (city_aliases, ambiguous_spoken) = build_city_aliases(&cities);
        let mut legacy_names_by_key: IndexMap<String, Vec<String>> = IndexMap::new();
        for (old_name, slug) in LEGACY_CITY_SLUGS {
            if cities.contains_key(*slug) && old_name != slug {
                legacy_names_by_key
                    .entry(slug.to_string())
                    .or_default()
                    .push(old_name.to_string());
            }
        }
        let legacy_facility_ids = build_legacy_facility_ids(&cities, &legacy_names_by_key);
        let service_city_keys = build_service_city_keys(&cities, &legacy_names_by_key);

        let baked_legs = container.take_legs()?;
        let mut legs: Vec<Arc<Leg>> = Vec::with_capacity(baked_legs.len());
        for (index, baked) in baked_legs.into_iter().enumerate() {
            let held = Arc::clone(&container);
            // Three numbers, captured by value: the builder holds the mapping
            // and this leg's own frame, so nothing has to keep the decoded
            // leg table alive for the life of the world.
            let at = CorridorRef {
                offset: baked.corridor_offset,
                stored_len: baked.corridor_stored_len,
                raw_len: baked.corridor_raw_len,
            };
            let mut built = Leg::lazy_built(
                &baked.a,
                &baked.b,
                baked.miles,
                &baked.highway,
                &baked.terrain,
                baked.stops.into_iter().map(Into::into).collect(),
                Arc::new(move || -> Result<CorridorDetail, DataError> { held.corridor_at(at) }),
            );
            built.id = index;
            built.lanes = baked.lanes;
            built.local_cue = baked.local_cue;
            built.local_speed_mph = baked.local_speed_mph;
            built.local_turn_deg = baked.local_turn_deg;
            built.divided = baked.divided;
            built.truck_advisory = baked.truck_advisory;
            built.crossings = baked.crossings;
            built.detour_for = baked.detour_for;
            built.meta_complete = baked.meta_complete;
            legs.push(Arc::new(built));
        }
        let mut adjacency: HashMap<String, Vec<Arc<Leg>>> = cities
            .keys()
            .map(|name| (name.clone(), Vec::new()))
            .collect();
        for leg in &legs {
            adjacency
                .get_mut(&leg.a)
                .ok_or_else(|| DataError::key(format!("Unknown city: {}", leg.a)))?
                .push(Arc::clone(leg));
            adjacency
                .get_mut(&leg.b)
                .ok_or_else(|| DataError::key(format!("Unknown city: {}", leg.b)))?
                .push(Arc::clone(leg));
        }
        Ok(World {
            cities,
            legs,
            facilities_by_id,
            city_aliases,
            ambiguous_spoken,
            legacy_names_by_key,
            legacy_facility_ids,
            service_city_keys,
            adjacency,
            supported_route_cache: Mutex::new(HashMap::new()),
            data_dir,
            city_service_data_cache: OnceCell::new(),
            facility_approaches_cache: OnceCell::new(),
            facility_endpoints_cache: OnceCell::new(),
            local_approaches_cache: OnceCell::new(),
            local_geometries_cache: OnceCell::new(),
            baked: Some(container),
        })
    }

    /// Load the world, optionally merging an additive overlay on top.
    ///
    /// The checked-in indexed world data is the deterministic source of truth.
    /// An optional `overlay` is merged additively: it can only add cities and
    /// legs the base does not already have, never override the base. With no
    /// overlay the result is exactly the base world, so the
    /// offline/deterministic path is unchanged. The runtime `get_world`
    /// deliberately does not pass an overlay yet; this is the loader
    /// capability the online tier will build on.
    pub fn load_with_overlay(data_dir: &Path, overlay: Option<&Path>) -> Result<World, DataError> {
        // An overlay is a JSON merge onto raw world data, so it takes the
        // JSON path even in a baked build. Nothing passes one yet (see the
        // doc comment); a release that starts to will need the container to
        // ship the raw shapes too, and the loader will say so rather than
        // quietly dropping the overlay.
        if overlay.is_none() {
            if let Some(container) = baked_at(data_dir)? {
                return World::from_baked(container, data_dir.to_path_buf());
            }
        }
        World::load_json_with_overlay(data_dir, overlay)
    }

    /// [`load_with_overlay`](World::load_with_overlay) against the JSON tree.
    pub fn load_json_with_overlay(
        data_dir: &Path,
        overlay: Option<&Path>,
    ) -> Result<World, DataError> {
        let mut data = load_world_data(&data_dir.join("world_data"))?;
        if let Some(overlay) = overlay {
            if let Some(text) = read_text_at(overlay) {
                data = merge_overlay(data, WorldData::from_json_str(&text)?);
            }
        }
        World::from_data_at(data, data_dir.to_path_buf())
    }

    /// The data directory the lazy local-driving files are read from.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    // ------------------------------------------------------------ lazy local data

    pub fn city_service_data(&self) -> Result<&CityServiceData, DataError> {
        self.city_service_data_cache
            .get_or_init(|| {
                let raw = match &self.baked {
                    Some(container) => container.city_service_data()?,
                    None => load_city_service_data(&self.data_dir.join("city_services.json"))?,
                };
                Ok(raw
                    .into_iter()
                    .map(|(name, services)| (self.resolve_city_key(&name), services))
                    .collect())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn facility_approaches(&self) -> Result<&IndexMap<String, FacilityApproach>, DataError> {
        self.facility_approaches_cache
            .get_or_init(|| {
                let raw = match &self.baked {
                    Some(container) => container.facility_approaches()?,
                    None => {
                        load_facility_approaches(&self.data_dir.join("facility_approaches.json"))?
                    }
                };
                Ok(self.remap_facility_ids(raw))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn facility_endpoints(&self) -> Result<&IndexMap<String, FacilityEndpoint>, DataError> {
        self.facility_endpoints_cache
            .get_or_init(|| {
                let raw = match &self.baked {
                    Some(container) => container.facility_endpoints()?,
                    None => {
                        load_facility_endpoints(&self.data_dir.join("facility_endpoints.json"))?
                    }
                };
                Ok(self.remap_facility_ids(raw))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn local_approaches(&self) -> Result<&IndexMap<String, LocalApproach>, DataError> {
        self.local_approaches_cache
            .get_or_init(|| {
                let raw = match &self.baked {
                    Some(container) => container.local_approaches()?,
                    None => load_local_approaches(&self.data_dir.join("local_approaches.json"))?,
                };
                Ok(self.remap_local_ids(raw))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn local_geometries(&self) -> Result<&IndexMap<String, LocalGeometry>, DataError> {
        self.local_geometries_cache
            .get_or_init(|| {
                let raw = match &self.baked {
                    Some(container) => container.local_geometries()?,
                    None => load_local_geometries(&self.data_dir.join("local_geometry.json"))?,
                };
                Ok(self.remap_local_ids(raw))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Rekey a facility-id-keyed local-data map onto current ids.
    fn remap_facility_ids<T>(&self, data: IndexMap<String, T>) -> IndexMap<String, T> {
        data.into_iter()
            .map(|(key, value)| (self.resolve_facility_id(&key), value))
            .collect()
    }

    /// Rekey local approach/geometry target ids onto canonical keys.
    fn remap_local_ids<T>(&self, data: IndexMap<String, T>) -> IndexMap<String, T> {
        data.into_iter()
            .map(|(key, value)| (self.canonical_local_id(&key), value))
            .collect()
    }

    /// The canonical form of a local approach/geometry target id
    /// (`city_service:<slug>:<key>` or `facility:<id>`).
    pub fn canonical_local_id(&self, target_id: &str) -> String {
        if let Some(rest) = target_id.strip_prefix("city_service:") {
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            if parts.len() == 2 {
                if let Some(key) = self.service_city_keys.get(parts[0]) {
                    return format!("city_service:{key}:{}", parts[1]);
                }
            }
            return target_id.to_string();
        }
        if let Some(facility_id) = target_id.strip_prefix("facility:") {
            return format!("facility:{}", self.resolve_facility_id(facility_id));
        }
        target_id.to_string()
    }

    // ------------------------------------------------------------ cities

    pub fn city_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.cities.keys().cloned().collect();
        names.sort();
        names
    }

    /// Canonical key for any current or legacy city reference.
    ///
    /// Old saves persist bare display names ("Jackson") or qualified ones
    /// ("Jackson, Michigan"); both resolve through the alias table. Unknown
    /// text echoes back unchanged so callers keep their existing
    /// unknown-city behavior.
    pub fn resolve_city_key(&self, city: &str) -> String {
        let text = city.trim();
        if self.cities.contains_key(text) {
            return text.to_string();
        }
        self.city_aliases
            .get(text)
            .cloned()
            .unwrap_or_else(|| text.to_string())
    }

    /// The City for a current or legacy reference; `Key` error if unknown.
    pub fn city(&self, city: &str) -> Result<&City, DataError> {
        self.cities
            .get(&self.resolve_city_key(city))
            .ok_or_else(|| DataError::key(format!("Unknown city: {city}")))
    }

    /// Speakable name for a city reference; never the slug key.
    ///
    /// `qualified: None` appends the state exactly when the bare name is
    /// shared by more than one city (Jackson -> "Jackson, Mississippi").
    /// Unresolvable legacy text passes through unchanged -- old display
    /// names are already speakable.
    pub fn spoken_city(&self, city: &str, qualified: Option<bool>) -> String {
        let key = self.resolve_city_key(city);
        let Some(city_obj) = self.cities.get(&key) else {
            return city.to_string();
        };
        let qualified = qualified.unwrap_or_else(|| self.ambiguous_spoken.contains(&key));
        if qualified {
            city_obj.spoken_qualified()
        } else {
            city_obj.name.clone()
        }
    }

    /// The legs touching a city (empty for an unknown city, where Python
    /// raised `KeyError`).
    pub fn neighbors(&self, city: &str) -> &[Arc<Leg>] {
        self.adjacency
            .get(&self.resolve_city_key(city))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn facility_location(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<&Location, DataError> {
        let key = self.resolve_city_key(city);
        let Some(city_obj) = self.cities.get(&key) else {
            return Err(DataError::key(format!("Unknown city: {city}")));
        };
        let normalized_name = location_name.trim().to_string();
        let normalized_id = self.resolve_facility_id(&normalized_name);
        // Legacy saves may name template facilities with the old display name
        // embedded ("Jackson, Michigan Regional Cross-Dock").
        let mut name_candidates = vec![normalized_name.clone()];
        let legacy_names = self
            .legacy_names_by_key
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for old_name in legacy_names {
            name_candidates.push(normalized_name.replace(old_name.as_str(), &city_obj.name));
        }
        for location in &city_obj.locations {
            if name_candidates.contains(&location.name)
                || location.id == normalized_name
                || location.id == normalized_id
            {
                return Ok(location);
            }
        }
        let mut all_names: Vec<&str> = vec![city_obj.name.as_str()];
        all_names.extend(legacy_names.iter().map(String::as_str));
        if is_legacy_market_name(&all_names, &normalized_name) {
            return self.default_facility(&key);
        }
        Err(DataError::key(format!(
            "Unknown facility in {city}: {location_name}"
        )))
    }

    pub fn facility_by_id(&self, facility_id: &str) -> Result<&Location, DataError> {
        self.facilities_by_id
            .get(&self.resolve_facility_id(facility_id))
            .ok_or_else(|| DataError::key(format!("Unknown facility id: {facility_id}")))
    }

    /// Translate a pre-slug facility id to its current form when known.
    pub fn resolve_facility_id(&self, facility_id: &str) -> String {
        if self.facilities_by_id.contains_key(facility_id) {
            return facility_id.to_string();
        }
        self.legacy_facility_ids
            .get(facility_id)
            .cloned()
            .unwrap_or_else(|| facility_id.to_string())
    }

    /// Stable fallback for legacy jobs that only named a city.
    ///
    /// Only `company_yard` or `terminal` may be returned. Travel centers and
    /// truck parking are fuel/rest pins, never dispatch yards. When the city
    /// has neither, this errors (`{city} has no freight facilities`) rather
    /// than inventing a warehouse or picking a fuel stop — callers should
    /// treat that city as not offering a default freight endpoint.
    pub fn default_facility(&self, city: &str) -> Result<&Location, DataError> {
        let key = self.resolve_city_key(city);
        let Some(city_obj) = self.cities.get(&key) else {
            return Err(DataError::key(format!("Unknown city: {city}")));
        };
        let locations = &city_obj.locations;
        const PREFERRED: &[&str] = &["company_yard", "terminal"];
        for facility_type in PREFERRED {
            if let Some(location) = locations.iter().find(|l| l.facility_type == *facility_type) {
                return Ok(location);
            }
        }
        Err(DataError::key(format!("{city} has no freight facilities")))
    }

    /// Name of this city's own `terminal` (preferred) or `company_yard`
    /// freight pin, for cosmetic naming only (the fallback garage). World
    /// pins are freight endpoints and are never a player's home terminal:
    /// that is the hiring carrier's own yard (see `models::carriers`).
    pub fn yard_pin_name(&self, city: &str) -> Option<&str> {
        let key = self.resolve_city_key(city);
        let city = self.cities.get(&key)?;
        city.locations
            .iter()
            .find(|l| l.facility_type == "terminal")
            .or_else(|| {
                city.locations
                    .iter()
                    .find(|l| l.facility_type == "company_yard")
            })
            .map(|l| l.name.as_str())
    }

    // ------------------------------------------------------------ routing

    /// Dijkstra over leg miles, with optional per-leg penalty multipliers.
    ///
    /// `require_metadata` is for new dispatchable freight. The default keeps
    /// the historical full graph available for legacy saves and map integrity
    /// checks while supported freight routes are enriched lane by lane.
    /// `Ok(None)` when no road connects the cities; `Err` for an unknown city.
    pub fn shortest_route(
        &self,
        start: &str,
        end: &str,
        penalties: Option<&Penalties>,
        require_metadata: bool,
    ) -> Result<Option<Route>, DataError> {
        let start = self.resolve_city_key(start);
        let end = self.resolve_city_key(end);
        if !self.cities.contains_key(&start) {
            return Err(DataError::key(format!("Unknown city: {start}")));
        }
        if !self.cities.contains_key(&end) {
            return Err(DataError::key(format!("Unknown city: {end}")));
        }
        let has_penalties = penalties.is_some_and(|p| !p.is_empty());
        let mut dist: HashMap<String, f64> = HashMap::new();
        dist.insert(start.clone(), 0.0);
        let mut prev: HashMap<String, (String, Arc<Leg>)> = HashMap::new();
        let mut heap: BinaryHeap<Reverse<(OrdF64, String)>> = BinaryHeap::new();
        heap.push(Reverse((OrdF64(0.0), start.clone())));
        let mut visited: HashSet<String> = HashSet::new();
        while let Some(Reverse((OrdF64(d), city))) = heap.pop() {
            if !visited.insert(city.clone()) {
                continue;
            }
            if city == end {
                break;
            }
            let Some(legs) = self.adjacency.get(&city) else {
                continue;
            };
            for leg in legs {
                if require_metadata && !self.leg_metadata_complete(leg) {
                    continue;
                }
                let nxt = leg.other(&city);
                // A leg's reverse record is the same road the other way; a
                // detour leg is a different road, so it neither hides nor is
                // hidden by the leg it bypasses.
                let has_forward_leg = leg.detour_for.is_empty()
                    && legs.iter().any(|candidate| {
                        candidate.detour_for.is_empty()
                            && candidate.a.as_str() == city
                            && candidate.b.as_str() == nxt
                            && (!require_metadata || self.leg_metadata_complete(candidate))
                    });
                if leg.a.as_str() != city && has_forward_leg {
                    continue;
                }
                let mut cost = if has_penalties {
                    leg.miles
                        * penalties
                            .and_then(|p| p.get(&leg.id))
                            .copied()
                            .unwrap_or(1.0)
                } else {
                    leg.miles
                };
                if !leg.detour_for.is_empty() {
                    cost += DETOUR_TIE_BREAK_MILES;
                }
                if !leg.truck_advisory.is_empty() {
                    // Strong avoidance, never refusal. Calibrated against the
                    // decision real carriers make at Red Mountain Pass: they
                    // accept the ~1.7x-distance detour through Cortez and
                    // Moab rather than run a warned pass, so the multiplier
                    // only has to clear that ratio for the detour to win
                    // wherever one exists; 2.5 clears it with margin. A pair
                    // of towns whose ONLY road is the warned one still
                    // routes -- the advisory is a warning, not a wall.
                    cost *= TRUCK_ADVISORY_COST_MULT;
                }
                let nd = d + cost;
                if nd < dist.get(nxt).copied().unwrap_or(f64::INFINITY) {
                    dist.insert(nxt.to_string(), nd);
                    prev.insert(nxt.to_string(), (city.clone(), Arc::clone(leg)));
                    heap.push(Reverse((OrdF64(nd), nxt.to_string())));
                }
            }
        }
        if !prev.contains_key(&end) && start != end {
            return Ok(None);
        }
        let mut cities = vec![end.clone()];
        let mut legs: Vec<Arc<Leg>> = Vec::new();
        let mut cur = end;
        while cur != start {
            let (parent, leg) = prev
                .get(&cur)
                .expect("every visited city has a parent")
                .clone();
            legs.push(leg);
            cities.push(parent.clone());
            cur = parent;
        }
        cities.reverse();
        legs.reverse();
        Ok(Some(Route::new(cities, legs)))
    }

    /// Rebuild a route from its city sequence (used by saved trips).
    ///
    /// Returns None if any hop is missing, so callers can fall back
    /// gracefully when a save references a road that no longer exists. This is
    /// intentionally the legacy/full graph path; new freight uses supported
    /// route helpers so missing metadata cannot silently invent conditions.
    pub fn route_from_cities<S: AsRef<str>>(&self, cities: &[S]) -> Option<Route> {
        if cities.len() < 2 {
            return None;
        }
        let cities: Vec<String> = cities
            .iter()
            .map(|c| self.resolve_city_key(c.as_ref()))
            .collect();
        let mut legs: Vec<Arc<Leg>> = Vec::new();
        for pair in cities.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let leg = self
                .adjacency
                .get(a)
                .and_then(|legs| pick_hop(legs, a, b, |_| true))?;
            legs.push(Arc::clone(leg));
        }
        Some(Route::new(cities, legs))
    }

    /// `route_from_cities` for a trip carrying `load`: where a hop has a
    /// legal detour beside a road whose crossing refuses the load, the hop
    /// is the detour, the road a saved trip with that load was planned on.
    pub fn route_from_cities_for<S: AsRef<str>>(
        &self,
        cities: &[S],
        load: &CrossingLoad,
    ) -> Option<Route> {
        if cities.len() < 2 {
            return None;
        }
        let cities: Vec<String> = cities
            .iter()
            .map(|c| self.resolve_city_key(c.as_ref()))
            .collect();
        let mut legs: Vec<Arc<Leg>> = Vec::new();
        for pair in cities.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let legal = |leg: &Arc<Leg>| leg.crossings.iter().all(|c| c.refusal(load).is_none());
            let leg = self.adjacency.get(a).and_then(|legs| {
                pick_hop(legs, a, b, legal).or_else(|| pick_hop(legs, a, b, |_| true))
            })?;
            legs.push(Arc::clone(leg));
        }
        Some(Route::new(cities, legs))
    }

    pub fn leg_metadata_complete(&self, leg: &Leg) -> bool {
        let state = |key: &str| self.cities.get(key).map(|c| c.state.as_str()).unwrap_or("");
        leg.metadata_complete(state(&leg.a), state(&leg.b))
    }

    pub fn supported_route(
        &self,
        start: &str,
        end: &str,
        penalties: Option<&Penalties>,
    ) -> Result<Option<Route>, DataError> {
        if penalties.is_some_and(|p| !p.is_empty()) {
            return self.shortest_route(start, end, penalties, true);
        }
        let start = self.resolve_city_key(start);
        let end = self.resolve_city_key(end);
        let key = (start.clone(), end.clone());
        let cached = self.supported_route_cache.lock().get(&key).cloned();
        let route = match cached {
            Some(route) => route,
            None => {
                let route = self.shortest_route(&start, &end, None, true)?;
                self.supported_route_cache.lock().insert(key, route.clone());
                route
            }
        };
        Ok(route)
    }

    pub fn supported_route_options(
        &self,
        start: &str,
        end: &str,
        count: usize,
    ) -> Result<Vec<Route>, DataError> {
        self.route_options(start, end, count, true)
    }

    /// Every leg a crossing on it refuses this load, walled off for the
    /// route search. Empty for a load no crossing on the map turns away.
    pub fn crossing_blocks(&self, load: &CrossingLoad) -> Penalties {
        self.legs
            .iter()
            .filter(|leg| leg.crossings.iter().any(|c| c.refusal(load).is_some()))
            .map(|leg| (leg.id, f64::INFINITY))
            .collect()
    }

    /// The shortest supported route this load may legally drive: around
    /// every tunnel or bridge whose rules turn it away. None when no such
    /// road exists, which is a load dispatch must not offer.
    pub fn legal_route(
        &self,
        start: &str,
        end: &str,
        load: &CrossingLoad,
    ) -> Result<Option<Route>, DataError> {
        let blocks = self.crossing_blocks(load);
        let best = self.supported_route(start, end, None)?;
        if blocks.is_empty() {
            return Ok(best);
        }
        // Removing legs never shortens a search, so a best road that already
        // misses every refusing crossing is the legal best too.
        if let Some(route) = best {
            if route.legs.iter().all(|leg| !blocks.contains_key(&leg.id)) {
                return Ok(Some(route));
            }
        }
        self.shortest_route(start, end, Some(&blocks), true)
    }

    /// Up to `count` supported routes this load may legally drive, shortest
    /// first; see `legal_route`.
    pub fn legal_route_options(
        &self,
        start: &str,
        end: &str,
        count: usize,
        load: &CrossingLoad,
    ) -> Result<Vec<Route>, DataError> {
        let blocks = self.crossing_blocks(load);
        self.route_options_with(start, end, count, true, &blocks)
    }

    /// The road ahead re-planned around a crossing that refuses this load,
    /// with the crossing's spoken rule; None when the road ahead is legal.
    ///
    /// The new road starts at the next city the truck has not yet left, so
    /// everything behind the truck is unchanged. With no legal road onward
    /// from there it runs back the way it came to the origin and takes the
    /// origin's legal road, so a refused truck always has a way to go.
    pub fn legalize_route_ahead(
        &self,
        route: &Route,
        position_mi: f64,
        load: &CrossingLoad,
    ) -> Option<(Route, String)> {
        let mut start_mi = 0.0;
        let mut ahead = None;
        for (i, leg) in route.legs.iter().enumerate() {
            if start_mi >= position_mi - 1e-6 {
                ahead = Some(i);
                break;
            }
            start_mi += leg.miles;
        }
        let ahead = ahead?;
        let reason = route.legs[ahead..].iter().find_map(|leg| {
            leg.crossings
                .iter()
                .find_map(|c| c.refusal(load).map(str::to_string))
        })?;
        let here = route.cities.get(ahead)?;
        let destination = route.cities.last()?;
        let mut cities = route.cities[..ahead].to_vec();
        let mut legs = route.legs[..ahead].to_vec();
        if let Some(tail) = self.legal_route(here, destination, load).ok().flatten() {
            cities.extend(tail.cities);
            legs.extend(tail.legs);
        } else {
            let origin = route.cities.first()?;
            let onward = self.legal_route(origin, destination, load).ok().flatten()?;
            cities.extend(route.cities[..=ahead].iter().rev().cloned());
            legs.extend(route.legs[..ahead].iter().rev().cloned());
            cities.extend(onward.cities.into_iter().skip(1));
            legs.extend(onward.legs);
        }
        Some((Route::new(cities, legs), reason))
    }

    /// Up to `count` distinct routes, fastest first.
    pub fn route_options(
        &self,
        start: &str,
        end: &str,
        count: usize,
        require_metadata: bool,
    ) -> Result<Vec<Route>, DataError> {
        self.route_options_with(start, end, count, require_metadata, &HashMap::new())
    }

    /// `route_options` over a graph with `walls` already penalised -- an
    /// infinite penalty takes a leg out of every search.
    fn route_options_with(
        &self,
        start: &str,
        end: &str,
        count: usize,
        require_metadata: bool,
        walls: &Penalties,
    ) -> Result<Vec<Route>, DataError> {
        let mut routes: Vec<Route> = Vec::new();
        let mut penalties: Penalties = walls.clone();
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        let first = (!walls.is_empty()).then_some(walls);
        let Some(best) = self.shortest_route(start, end, first, require_metadata)? else {
            return Ok(routes);
        };
        let max_miles = max_alternate_miles(best.miles());
        for _ in 0..count * 8 {
            let Some(route) =
                self.shortest_route(start, end, Some(&penalties), require_metadata)?
            else {
                break;
            };
            if !seen.contains(&route.cities) && route.miles() <= max_miles {
                seen.insert(route.cities.clone());
                routes.push(route.clone());
                if routes.len() >= count {
                    break;
                }
            }
            for leg in &route.legs {
                *penalties.entry(leg.id).or_insert(1.0) *= 2.5;
            }
        }
        routes.sort_by(|a, b| a.miles().partial_cmp(&b.miles()).unwrap_or(Ordering::Equal));
        Ok(routes)
    }
}

/// A total order on finite distances so they can sit in a `BinaryHeap`
/// alongside the city key, exactly as Python's `(dist, city)` tuples did.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrdF64(f64);

impl Eq for OrdF64 {}

impl PartialOrd for OrdF64 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrdF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

/// The longest an alternate may be: a share of the best route's miles,
/// clamped to the dispatch-feeling band.
pub fn max_alternate_miles(best_miles: f64) -> f64 {
    let extra = best_miles * ALTERNATE_ROUTE_EXTRA_RATIO;
    let extra = ALTERNATE_ROUTE_MIN_EXTRA_MILES.max(ALTERNATE_ROUTE_MAX_EXTRA_MILES.min(extra));
    best_miles + extra
}

static WORLD: OnceCell<World> = OnceCell::new();

/// Shared world instance (the data is immutable). Loads on first use from
/// the default data root; a world that fails to load is fatal, as the
/// Python import was.
pub fn get_world() -> &'static World {
    WORLD.get_or_init(|| World::load().expect("the shipped world data loads"))
}

/// Whether the shared world has been loaded yet.
pub fn world_is_loaded() -> bool {
    WORLD.get().is_some()
}

/// Great-circle distance in statute miles (carrier terminal and hiring radius).
pub fn air_miles(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_MI: f64 = 3958.7613;
    let (lat1, lon1, lat2, lon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    EARTH_MI * 2.0 * a.sqrt().asin()
}
