# Freight Market And Facility Data

Freight Fate models cities as metro freight markets. The highway graph remains
the routeable set of supported metro nodes, while each metro expands into
origin and destination facilities: terminals, ports, intermodal ramps,
warehouses, food facilities, plants, yards, and other trucking locations.

This is intentionally representative, not exhaustive. The goal is to make the
supported city map feel like a national freight network without pretending the
game contains every U.S. town, shipper, receiver, industrial park, or
distribution center.

## Runtime Contract

Runtime data is offline. Job generation must not call BTS, FAF, MARAD, USDA,
OpenStreetMap, routing APIs, or operator sites. External sources guide the
checked-in taxonomy, weights, names, and source notes.

Each facility carries:

- `id`: stable generated or curated facility id.
- `name`: concise player-facing facility name.
- `city`: supported metro route node.
- `locality`: optional suburb or local-market hint.
- `type`: normalized facility category.
- `cargo`: cargo classes associated with the facility.
- `ships` and `receives`: role-specific cargo compatibility.
- `roles`: shipper and/or receiver.
- `lat` and `lon`: curated or representative coordinates when available.
- `traits`: design tags such as `template`, `representative`, or market tags.
- `source_note`: developer-facing note for why the facility exists.
- `spoken_name`: concise screen-reader-friendly label.

Older world data that only has `name`, `type`, and `cargo` is upgraded at load
time. Older saves that only named an origin or destination city display as the
metro freight market instead of failing.

## Facility Taxonomy

The normalized taxonomy covers the common freight surfaces a truck driver
would expect:

- `port_terminal`
- `intermodal_ramp`
- `air_cargo`
- `parcel_hub`
- `grocery_retail_dc`
- `dry_warehouse`
- `cold_storage`
- `food_processor`
- `farm_elevator`
- `manufacturing_plant`
- `steel_industrial`
- `automotive_plant`
- `chemical_petroleum_terminal`
- `construction_materials_yard`
- `mine_quarry`
- `lumber_paper`
- `cross_dock`
- `company_yard`
- `shopping_mall` (receives only; spoken "mall")

Legacy data types such as `port`, `warehouse`, `distribution`, `rail`,
`intermodal`, `food_terminal`, `manufacturing`, and `terminal` remain valid so
old data and saves keep loading.

## Source Strategy

Official data sources guide the model:

- [BTS Freight Analysis Framework](https://www.bts.gov/faf) and
  [FHWA FAF overview](https://ops.fhwa.dot.gov/freight/freight_analysis/faf/)
  guide metro/state freight-flow thinking, mode coverage, and commodity
  categories.
- [FAF / ORNL tools](https://faf.ornl.gov/) guide future lane and commodity
  weighting checks.
- [MARAD Ports Data and Statistics](https://www.maritime.dot.gov/data-reports/ports)
  and [BTS Port Performance Freight Statistics](https://www.bts.gov/ports)
  guide port-terminal, container, bulk, and intermodal treatment.
- [USDA Open Ag Transport Data](https://agtransport.usda.gov/) and
  [USDA Grain Truck and Ocean Rate Advisory](https://www.ams.usda.gov/services/transportation-analysis/gtor)
  guide grain, agricultural trucking, refrigerated availability, and food-flow
  context.

Do not paste raw dataset rows, OSM tags, IDs, NAICS codes, or source database
keys into player-facing names. Source notes may name a source family, but the
spoken job board should stay clean.

## Representative Templates

World JSON stays compact by listing curated seed locations. The loader then
adds deterministic representative facilities from market tags. For example:

- a port/gateway metro can receive `port_terminal`, `cross_dock`, and
  `intermodal_ramp` templates;
- an agricultural metro can receive `farm_elevator`, `food_processor`, and
  `cold_storage`;
- an industrial metro can receive `manufacturing_plant`, `steel_industrial`,
  `automotive_plant`, or `construction_materials_yard`;
- an energy metro can receive `chemical_petroleum_terminal`;
- a northwest or forest-products metro can receive `lumber_paper`;
- a logistics hub can receive `parcel_hub`.

Template facilities use stable IDs and polished names such as `Chicago
Cross-Dock` or `Fresno Grain Elevator`. They are representative gameplay
locations, not claims about a specific real-world shipper.

## City Service POIs

City services are separate from freight shippers and receivers. The runtime
loads checked-in `city_services.json` first, then falls back per missing service
to representative POIs derived from terminal and metro market data. Runtime
play stays offline and deterministic.

The source-backed city-service pass now covers every supported city in
`world.json`. `tools/build_city_services.py --all-supported` streams the local
Geofabrik-style extracts under
`C:\Users\joshu\.cache\freight-fate-osm\regions\` at build time and writes a
compact checked-in `city_services.json`. The current bake covers 194 cities and
582 service roles: 494 are source-backed from OSM and 88 are explicit fallback
truck-dealer roles where no realistic local dealer candidate was found in the
bounded city radius. The data stores source coordinates, approximate approach
mileage, and an approach road/context for every role.

The freight market office is a grounded mapping, not an OSM tag named "freight
market": it maps to a logistics, freight-forwarder, distribution, terminal, or
intermodal office/facility. Generic transport customer-service offices are not
enough. Garage and truck dealer mappings favor heavy-vehicle repair, truck
repair, truck sales, or named truck brands/dealers over generic car POIs.

Player speech should still say the service plainly, such as `garage: Carimichael
Truck Repair`, and should not expose raw OSM tags, IDs, or source keys. Source
references and fallback reasons stay internal for auditing.

The next data pass should keep service POIs separate from career ownership and
settlement systems. OSM/Overpass and ORS can improve where service drives go.
Cargo-to-trailer matching and own-authority trailer purchases now live in the
career/business model without changing service POIs.
Dispatch rows use that model to mark trailer-setup locks and show a short
estimated take-home preview. The preview is based on the current career
business status and trailer setup; it is not yet a full spot-rate, resale, or
fuel-margin simulation.

## Local Approach Data

`local_approaches.json` is the compact offline layer used by city-service
drives and pickup/delivery facility approach legs. Runtime reads the checked-in
file only; it does not call OSM, ORS, OSRM, Overpass, or other network routing
services.

The current bake used the local state PBF cache at
`C:\Users\joshu\.cache\freight-fate-osm\regions\` through
`tools/build_local_approaches.py`. It covers 2,401 targets: all 582
city-service roles have a nearest OSM public-road context, and 1,813 of 1,819
freight facility approach legs have a nearest OSM public-road context. Six
representative facility targets remain explicit fallbacks because no usable
road segment was found within the bounded search radius.

For sourced city services, the service coordinate and nearby road context come
from local OSM extracts. For freight facilities, most facility coordinates are
still representative gameplay points generated from metro freight-market
templates, so their approach records are marked estimated even when the nearby
road is source-backed. These records help GPS and route descriptions use real
local road names where available, but they are not driveway, gate, yard, dock,
or company claims.

The next map-data milestone is turn-level local routing: a build-time HGV
routing pass from the highway/city context to the service or facility vicinity,
then sourced final-access hints only where local data supports them.

## Local Turn Geometry

`local_geometry.json` is the next compact layer after nearest-road approach
context. It stores turn-level local street sequences when the source data is
strong enough, and fallback records everywhere else so the game cannot overclaim
real city layouts.

The current bake used the same local state PBF cache at
`C:\Users\joshu\.cache\freight-fate-osm\regions\` and
`tools/build_local_geometry.py`. It covers all 2,401 approach targets. Of the
582 city-service drives, 412 have source-backed local street geometry from a
bounded OSM road graph between the city context and the sourced service POI.
The remaining 170 city-service records fall back to nearest-road context. All
1,819 freight-facility records are fallback/estimated because the facility
endpoints are still representative gameplay facilities, not sourced real gates,
yards, docks, or shipper entrances.

This is distinct from the existing OpenRouteService work. Freight Fate already
has an ORS `driving-hgv` corridor pipeline for highway legs in
`tools/enrich_routes.py`; checked-in highway route points, elevation/grade data,
and some corridor metadata can come from that truck profile. The local turn
geometry added here does not use ORS and is not truck-legal HGV certification.
It is source-backed local OSM street geometry with conservative fallback
marking. ORS HGV remains a good credential-gated follow-up for selected local
service endpoints, but a full local batch would require hundreds of live
directions calls and still would not solve representative facility endpoints.

Player-facing GPS should use clean street names and turn wording from the
geometry layer when available, then fall back to the nearest-road approach
context. Do not expose OSM IDs, tags, source refs, ORS keys, or raw source text.

## Source-Backed Endpoint Layer

`data/facility_endpoints.json` is the checked-in offline
endpoint layer for freight facilities. It was generated from the local
Geofabrik cache at `C:\Users\joshu\.cache\freight-fate-osm\regions\` and covers
all 1,819 supported facilities: 1,462 have source-backed OSM freight/industrial
endpoint matches and 357 remain representative fallbacks with explicit reasons.

This layer is intentionally honest about its limits. It does not claim
truck-legal routing, road snapping, gates, yards, docks, or driveways; those
flags stay false unless a later source proves them. Runtime can prefer a
source-backed endpoint distance/name where available, then fall back to the
existing local approach and turn-geometry layers for road cues.

## Facility Approach Geometry

`data/facility_approaches.json` is the first source-backed
facility road-snap layer. `tools/build_facility_approaches.py` reads
`facility_endpoints.json`, `local_approaches.json`, and the local PBF cache at
`C:\Users\joshu\.cache\freight-fate-osm\regions\`. The current bounded bake
only targets high-confidence source-backed facilities in Illinois, Indiana, and
Ohio so it can be regenerated quickly without a national road scan.

The current coverage is 1,819 facility records: 1,462 source-backed endpoints,
71 road-snapped public-road contexts, 6 playable turn-level public-road
approaches, 1,391 source-backed nearest-road fallbacks outside or beyond this
bounded pass, and 357 representative facility fallbacks. Gate, yard, dock,
driveway, and private entrance hints remain zero because this layer does not
prove them.

## Job Generation

Job generation now chooses:

1. a compatible origin shipper facility in the current metro;
2. a cargo that the origin ships and the driver can reasonably see at the
   current level;
3. a supported route-node destination within the career distance cap;
4. a compatible receiver facility in that destination metro.

If the destination metro has no receiver for that cargo, the board retries
instead of inventing an implausible receiver. This prevents pairings such as
grain elevator to parcel hub unless the cargo roles explicitly support it.

Metro market tags weight the choices. Ports lean toward containers and bulk,
agricultural markets toward grain and food, industrial markets toward steel,
machinery, automotive, chemicals, lumber, and construction materials, and
border/gateway metros toward cross-dock and container freight.

Higher driver levels and endorsements expose more facility and cargo variety.
The route support gate still applies: new dispatches use metadata-backed
routes, and old saves can still load through the legacy graph.

## Accessibility

Facility names must be concise, pronounceable, and meaningful without a visual
map. Job rows should remain one spoken dispatch, not a wall of source detail.
Use the existing help/status/paperwork paths for deeper explanation.

Recommended spoken shape:

`18 tons of grain from farm elevator Fresno Grain Elevator in Fresno to food
processor Sacramento Food Processing Plant in Sacramento.`

Avoid raw tags, codes, or directional clutter in names. If a locality is useful,
keep it short and optional.

## Update Checklist

1. Add or adjust curated seed facilities in `world.json` only when the name is
   worth being player-facing.
2. Put broad coverage in template rules, not thousands of handwritten entries.
3. Add or update source notes for any new facility type or specialization.
4. Verify every facility has a stable id, clean spoken name, source note, and
   ship/receive cargo roles.
5. Verify generated jobs use compatible shipper and receiver roles.
6. Run focused tests:

```powershell
cargo test -p ff-core data_world
cargo test -p ff-core models::jobs
cargo test -p ff-core models::market
cargo test -p freight-fate --test it states_city_pickup
```

