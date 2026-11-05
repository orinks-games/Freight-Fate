# Route, Stop, And Corridor Data

Freight Fate keeps route amenities and corridor metadata in static JSON so the
game remains playable offline. Runtime driving does not call OpenStreetMap,
OSRM, Overpass, paid truck routing APIs, Census services, or operator sites.
External sources are build-time inputs only.

The product goal is the full existing Freight Fate network, not a smaller map.
New dispatchable freight is gated to routes whose legs have complete metadata;
the job board must not silently invent route conditions. The route-geometry
layer now covers all 106 current legs with checked-in route points, elevation
and grade samples, and state context. Curated truck-stop coverage is intentionally
reported separately: generated midpoint POIs have been removed from the current
106-leg network, and the coverage report must stay green before new legs become
normal dispatch lanes. The legacy/full graph remains loadable for old saves and
map integrity, but new freight uses the metadata-supported route gate.

Toll-corridor coverage is included in the metadata contract. The New York to
Philadelphia NJ Turnpike corridor, Philadelphia to Pittsburgh PA Turnpike/I-76,
Pittsburgh to Cleveland PA/Ohio connector, Cleveland to Chicago Ohio/Indiana
Turnpike style corridor, New York to Boston I-95/New England toll corridor, and
Philadelphia to Baltimore I-95 corridor all include explicit toll events where
source-backed. Toll payment is modeled as a trip expense charged through company
transponder/settlement accounting, not as a manual stop at every gantry.

## Schema

Route stops and corridor details live on a leg in the world source
(`data/world_source/legs/<STATE>.json`, reached through
`tools/world_source.py`):

```json
{
  "from": "Chicago",
  "to": "Indianapolis",
  "miles": 184,
  "highway": "I-65",
  "corridor": {
    "route_points": [
      {"at_mi": 0.0, "lat": 41.8781, "lon": -87.6298}
    ],
    "state_crossings": [
      {
        "at_mi": 33.0,
        "from_state": "Illinois",
        "state": "Indiana",
        "place": "the I-65 state line south of Hammond"
      }
    ],
    "checkpoints": [
      {
        "name": "Gary and Hammond industrial corridor",
        "type": "place",
        "state": "Indiana",
        "highway": "I-65 south",
        "at_mi": 36.0
      }
    ],
    "state_miles": [
      {"state": "Illinois", "miles": 33.0},
      {"state": "Indiana", "miles": 151.0}
    ],
    "elevation_samples": [
      {"at_mi": 0.0, "elevation_ft": 594.0},
      {"at_mi": 184.0, "elevation_ft": 718.0}
    ],
    "grade_segments": [
      {
        "start_mi": 0.0,
        "end_mi": 184.0,
        "avg_grade_pct": 0.03,
        "terrain": "flat"
      }
    ],
    "toll_events": [
      {
        "name": "Example Turnpike settlement",
        "at_mi": 90.0,
        "road": "Example Turnpike",
        "authority": "Example Toll Authority",
        "method": "ezpass",
        "amount": 42.0,
        "estimated": true,
        "source": "Toll authority rate table or calculator, accessed 2026-06-16."
      }
    ],
    "restrictions": [
      {
        "at_mi": 57.2,
        "kind": "low_clearance",
        "feet": 13.5,
        "source": "OpenStreetMap maxheight tags snapped to route geometry."
      }
    ]
  },
  "stops": [
    {
      "name": "Loves Travel Stop Lafayette",
      "type": "travel_center",
      "at_mi": 122.0,
      "actions": ["park", "save", "fuel", "food", "break", "sleep"],
      "services": ["diesel", "food", "parking"],
      "parking": "confirmed",
      "parking_spaces": 43,
      "directions": ["both"],
      "curation": "curated",
      "source": "Official operator page or curated Overpass/OSM review."
    }
  ]
}
```

`at_mi` is route miles from the leg's `from` city toward its `to` city. The trip
simulator mirrors stops, state crossings, and checkpoints when the player drives
the same leg in the opposite direction.

Name-only stops are intentionally rejected by the loader. They used to be
spread evenly across each leg, which made route amenities feel synthetic. New
data must provide a named, typed stop with an explicit position inside the leg
mileage.

`parking` records truck-parking certainty, not a promise that a space is open
right now. Current values are `confirmed`, `likely`, `limited`, `unknown`, and
`none`. Use `confirmed` when an official operator or agency source states truck
parking. Use `limited` for public rest areas or small lots where parking is
available but capacity is not guaranteed. Use `unknown` only for quarantined or
incomplete data; unknown parking does not satisfy the dispatch-support contract.

`parking_spaces` is an optional surveyed truck-parking spot count from an
official inventory -- currently the FHWA Jason's Law survey published as the
USDOT BTS NTAD "Truck Stop Parking" dataset. 0 or absent means unsurveyed:
speech stays capacity-silent and the overnight parking crunch keeps its flat
baseline. When present on a `confirmed` or `limited` stop, the spoken parking
note appends the count ("confirmed truck parking, 43 spaces") and lot size
shifts the deterministic overnight fullness odds (small lots fill earlier,
large travel plazas later).

`restrictions` are route-positioned advisory events like `toll_events`, baked
from OpenStreetMap `maxheight`/`maxweight` tags on the corridor's mainline
ways (`tools/build_interchanges.py --restrictions`). `kind` is
`low_clearance` (with `feet`) or `weight_limit` (with US short `tons`). The
GPS speaks them ahead of the point; they never reroute or block, because the
driving-hgv routing already avoids impassable restrictions. An empty baked
`restrictions` list means the sweep ran and found the corridor clean --
presence of the key marks a leg as surveyed.

`directions` defaults to `["both"]`. Use `["forward"]` or `["reverse"]` only
when a ramp, rest area, service plaza, or weigh station applies to one travel
direction on the stored leg. `at_mi` is still measured from the leg's `from`
city, and the simulator mirrors it for reverse travel.

`curation` is `curated` for source-backed data and `placeholder` for quarantined
legacy/generated stop records. Placeholder POIs are preserved only so developers
can see what still needs research. They do not make a leg dispatch-supported and
the trip simulator does not speak or offer them as rest stops.

Corridor metadata is optional for old data and saves. When present, it drives
GPS cues, state-line announcements, intermediate place calls, and progress
summaries. This is the first step away from treating each route as a plain
0-to-N mile bar between city nodes.

For new playable freight, a leg is considered supported only when it has:
route points, checkpoints, state mileage, state crossings when endpoint states
differ, elevation samples, grade segments, and enough curated actionable POIs
for its length. The current density rule is one curated POI under 160 miles, two
from 160 through 320 miles, and three beyond 320 miles. Legs at 160 miles or
longer must also expose at least one curated fuel-capable stop so dispatchable
freight never treats break-only coverage as enough for long-haul route support.
A multi-leg route is playable only when every leg meets that contract.

Toll events are route-positioned events, not POIs. Use `toll_events` for toll
road entry markers, toll plazas, ticket-system settlements, or electronic
gantries. Each event must include a clean name, road, authority, method, amount,
estimated flag, and source note. Current method values are `cash_card`,
`ticket_system`, `ezpass`, `open_road`, and `toll_by_plate`. Use zero-dollar
events only for entry markers where the charge is collected or estimated at a
later settlement event. Toll source notes should name the authority or rate
source, such as the New Jersey Turnpike Authority, Pennsylvania Turnpike
Commission, Ohio Turnpike and Infrastructure Commission, Indiana Toll Road,
Delaware River bridge/toll operators, or state toll agencies.

Elevation and grade metadata are also optional, but preferred for enriched
corridors. OSRM provides route geometry, distance, steps, and annotations; it
does not tell us terrain or hills directly. Terrain must be derived by sampling
the route geometry against an elevation source, computing grades between
samples, and classifying sustained profile changes as flat, hills, or mountain.

When `grade_segments` exist, runtime truck physics use them directly. Terrain
does not change between trip seeds. Weather, traffic, construction, and
incidents remain variable so routes stay alive without inventing hills.

## Navigation And Traffic Runtime

Loaded trips are destination/load-first. The player accepts freight, drives to
the origin facility, loads, and dispatch starts the itinerary. Manual route
selection is no longer the main fiction of the trip.

The GPS layer reads the itinerary and announces concise audio-first cues:

- continue cues for long highway stretches;
- advance and near cues for maneuvers;
- state crossings and intermediate corridor places;
- one-mile rest-stop exit cues;
- modeled traffic slowdowns when a lead vehicle or queue is ahead.
- toll-road and toll-gantry cues with settlement wording.
- posted low-clearance and weight-limit advisories ahead of the signed point.

Basic traffic is deterministic for a trip seed. The first slice models lead
traffic packs with a speed, gap, and reason such as slow lead traffic, merging
traffic, lane restriction, or queue. Adaptive cruise control uses that context:
it holds the set speed when clear, follows slower traffic at a three-second
clear-weather gap, increases that gap in rain, snow, fog, or low visibility,
and cancels when the driver brakes. It does not steer, change lanes, or replace
the GPS.

Traffic placement uses route length, corridor checkpoints, departure time, and
weather effects. Bad weather can increase traffic pressure and lower the lead
traffic target speed. These remain deterministic for a seed; they are not
physical route facts. Seeds may vary traffic, weather, construction, delays, CB
chatter, and hazards, but they must not place truck stops, rest areas, tolls,
state lines, weigh stations, service plazas, grades, or facility approaches.

Tolls are recorded once when the trip passes the route-positioned toll event.
The spoken cue warns ahead of the point, and the charge event records a
`TollCharge` on the trip. Delivery settlement reports gross pay, carrier-paid
or reimbursed charges, driver-responsibility charges, and net driver pay. Toll
charges are billed to the carrier/company settlement and do not reduce driver
pay, but driver-caused costs such as speeding fines still do. This keeps money
pressure without making routine approved business expenses feel like personal
cash handling at every gantry.

## POIs And Actions

POIs are curated gameplay locations, not raw OSM records. Overpass/OpenStreetMap
may be used to discover candidates, but checked-in data must use clean
player-facing names and normalized game categories. Do not expose OSM IDs,
raw tags, or source keys in speech, menus, or help text.

Supported first-pass POI types and actions:

- `travel_center` and `truck_stop`: park, save, fuel, food, break, sleep.
- `service_plaza`: park, save, fuel, food, break, sleep.
- `fuel_station`: park, save, fuel, break.
- `public_rest_area` and `truck_parking`: park, save, break, sleep.
- `weigh_station`: inspect.
- `repair_shop`: park, save, repair.

If an OSM/source candidate has no useful name, curate a restrained descriptive
name such as `I-65 Northbound Rest Area`; do not invent a brand. Checkpoint
POIs may be spoken as route context only when there is no real player action.

Do not infer repair from `public_rest_area`, `rest_area`, or a broad stop
category. Public safety rest areas are normally modeled as parking, restrooms,
maps/info, breaks, sleep/rest, and vehicle/load checks. Repair, towing, or
roadside assistance actions require explicit `actions` plus matching `services`
and a source note that backs the capability. Turnpike service plazas and
authorized emergency road-service arrangements should be represented as
service-plaza or roadside-assistance capabilities, not as generic rest-area
repair.

Do not model toll plazas or gantries as actionable rest stops unless the source
also identifies a truck-relevant service area at that location. A toll plaza is
usually a route event and expense. A toll-road service plaza remains a POI only
for the services it actually supports, such as parking, fuel, food, breaks, or
saves.

## Source Strategy

The current full-network pass combines no-key route/elevation sources,
development-time map review, source-noted curation, and small live smoke
checks. The checked-in runtime data is static. Examples include:

- OSRM public demo route API over OpenStreetMap for tiny build-time geometry
  checks. Keep requests cached or one-off; do not use it at runtime.
- Open-Meteo Elevation API for no-key development-time terrain samples from
  Copernicus DEM GLO-90. Store sampled results in `world.json`; do not call it
  at runtime.
- USGS Elevation Point Query Service / The National Map for U.S. point
  elevations. Treat values as interpolated terrain samples rather than surveyed
  road grades.
- OpenTopoData or Open-Elevation as alternate no-key/self-hostable elevation
  candidates. Public service availability and rate limits should be treated
  carefully.
- Nominatim, only if necessary for sparse build-time lookup. Use a custom
  User-Agent, at most one request per second, and keep attribution visible.
- Overpass API for development-time discovery of rest areas, truck stops,
  service plazas, truck parking, repair shops, fuel stations, and weigh
  stations. Convert candidates into curated names, categories, services, and
  actions before committing them. Do not call it at runtime.
- USDOT BTS NTAD "Truck Stop Parking" (the FHWA Jason's Law truck-parking
  survey): official per-facility truck-parking spot counts used both as a
  new-stop candidate source and to annotate existing stops with
  `parking_spaces` and confirmed parking. Downloaded once as GeoJSON and
  re-run offline:
  https://geodata.bts.gov/datasets/usdot::truck-stop-parking/about
- Census/TIGER or Census-derived public state boundary GeoJSON for computing
  state crossings from route geometry.
- Toll authority calculators and rate pages for source-backed commercial
  vehicle toll estimates. Current toll estimates are intentionally documented
  with the source and access date because exact truck class, axle, transponder,
  time-of-day, and toll-by-plate rules can change.
- 49 U.S.C. 14103 and FMCSA lumper guidance for required loading/unloading
  assistance responsibility. Required loading or unloading services should be
  modeled as approved carrier/customer charges when authorized and receipted,
  not as driver-fault deductions.
- WisDOT Kenosha Safety Rest Area, with truck parking:
  https://wisconsindot.gov/Pages/travel/road/rest-areas/26-kenosha.aspx
- INDOT rest-area/truck-parking overview:
  https://www.in.gov/indot/restareas.htm
- TxDOT safety rest-area list and Hill County details:
  https://www.txdot.gov/discover/rest-areas-travel-information-centers/safety-rest-area-list.html
  https://www.txdot.gov/discover/rest-areas-travel-information-centers/safety-rest-area-list/hill.html
- ARDOT welcome centers/rest areas PDF:
  https://media.ark.org/ardot/Welcome-Centers-and-Rest-Areas-and-Accessibility-Barriers.pdf
- Caltrans Safety Roadside Rest Areas program:
  https://dot.ca.gov/programs/design/lap-landscape-architecture-and-community-livability/lap-liv-h-safety-roadside-rest-areas
- Iowa 80 Truckstop official site:
  https://iowa80truckstop.com/
- Road Ranger Waco official location page:
  https://www.roadrangerusa.com/node/251
- Loves Lafayette official location page:
  https://www.loves.com/locations/in/lafayette/loves-travel-stop-lafayette-874
- Pilot Remington, Huntsville, Bakersfield, Grand Junction, Ehrenberg,
  Davenport, and Ripon official location pages:
  https://locations.pilotflyingj.com/us/in/remington/4154-us-24
  https://locations.pilotflyingj.com/us/tx/huntsville/639-tx-75
  https://locations.pilotflyingj.com/us/ca/bakersfield/17047-zachary-ave
  https://locations.pilotflyingj.com/us/co/grand-junction/2195-hwy-6-and-50
  https://locations.pilotflyingj.com/us/az/ehrenberg/i-10-exit-1-frontage-road-n.
  https://locations.pilotflyingj.com/us/ia/davenport/8200-northwest-blvd
  https://locations.pilotflyingj.com/us/ca/ripon/1501-n-jack-tone-rd
- Loves Normal, West Memphis, Newton, Tulare, and Madera official/opening pages:
  https://www.loves.com/locations/il/normal/loves-travel-stop-normal-867
  https://www.loves.com/locations/ar/west-memphis/loves-travel-stop-west-memphis-450
  https://www.loves.com/locations/ia/newton/loves-travel-stop-newton-361
  https://www.loves.com/locations/ca/tulare/loves-travel-stop-tulare-382
  https://www.loves.com/news/2020/february/loves-travel-stops-opens-in-madera-california
- TravelCenters of America Tonopah and Fairfield official location pages:
  https://www.ta-petro.com/location/az/ta-tonopah/
  https://www.ta-petro.com/location/tx/ta-express-fairfield/
- Pilot Travel Center Lincoln official location page:
  https://locations.pilotflyingj.com/us/al/lincoln/75750-al-77
- IDOT truck parking page for I-55 rest areas:
  https://idot.illinois.gov/programs-and-projects/rail-and-freight/truck-parking.html
- Pilot Battle Creek and TA Sawyer official location pages:
  https://locations.pilotflyingj.com/us/mi/battle-creek/15901-11-mile-rd
  https://www.ta-petro.com/location/mi/ta-sawyer/

OpenStreetMap and Overpass are good development-time candidates for expanding
coverage because they expose open highway amenity data. If OSM-derived data is
committed, keep attribution and ODbL obligations visible in release materials
and source notes:

- OpenStreetMap copyright and license:
  https://www.openstreetmap.org/copyright
- OpenStreetMap Foundation attribution guidelines:
  https://osmfoundation.org/wiki/Licence/Attribution_Guidelines
- Overpass API documentation:
  https://wiki.openstreetmap.org/wiki/Overpass_API

## Build-Time Tooling

Inspect the checked-in corridor metadata:

```powershell
uv run python tools/enrich_routes.py --from-city Chicago --to-city Indianapolis
```

Run the tiny live OSRM and Open-Meteo elevation smoke check:

```powershell
uv run python tools/enrich_routes.py --from-city Chicago --to-city Indianapolis --live-smoke
```

The live smoke prints OSRM route mileage, simplified geometry point count, and
Open-Meteo elevation sample range. It is deliberately separate from
deterministic unit tests and should remain a small, credential-free sanity
check.

Report coverage for every leg in human-readable form. The report calls out
placeholder-only legs and insufficient stop density instead of treating generated
midpoint POIs as finished data:

```powershell
uv run python tools/enrich_routes.py --coverage-report
```

Report the same coverage as JSON for tests or planning:

```powershell
uv run python tools/enrich_routes.py --coverage-report --json
```

The JSON report is the whole-game acceptance artifact. It includes total legs,
playable legs, placeholder-only legs, insufficient-density legs, and per-leg
`unsupported_reasons`. For the current 106-leg network, `playable`,
`legs_with_curated_pois`, and `legs_with_sufficient_poi_density` should all
equal `legs`, while `placeholder_pois`, `legs_with_placeholder_only`, and
`missing_playable` should be zero. Normal dispatch uses the same
metadata-complete gate, so any future under-covered leg remains visible as data
debt without appearing as an ordinary job.

Run a tiny Overpass POI reachability smoke for one corridor:

```powershell
uv run python tools/enrich_routes.py --from-city Chicago --to-city Indianapolis --overpass-poi-smoke
```

Re-read every leg's exits and their ramps (exit numbers, signs, the ramp's
far end and terminal control, advisory speed, length and the node each
ramp ends at, both directions) from one OpenStreetMap extract:

```powershell
cargo tools osm-exits --pbf path/to/us-highways.osm.pbf --extract-date 2026-10-19 --snapshot "OpenStreetMap planet file" --write
```

Omit `--write` for a dry run, and use `--only "from_slug:to_slug"` (semicolons
between several) for a smaller batch. Exits are placed on the leg's archived
route geometry, so no router is needed. Records and ramp facts set by hand,
and ramp ends a street chain starts from, are kept; the module docs in
`crates/ff-tools/src/world/osm/exits/mod.rs` say how.

Discover truck-relevant POI candidates near checked-in route geometry with the
manual Overpass helper:

```powershell
uv run python tools/discover_route_pois.py --from-city Indianapolis --to-city Nashville --limit-points 3 --radius-m 12000
```

For whole-network planning, query small polite batches and review the candidate
output before editing `world.json`:

```powershell
uv run python tools/discover_route_pois.py --all --max-legs 5 --limit-points 1 --radius-m 8000 --json
```

The helper queries `https://overpass-api.de/api/interpreter` for narrow
route-adjacent rest areas, service areas, HGV-capable fuel/parking, weighbridge
features, and common travel-center names. Its output is not committed directly;
developers must still verify official operator, agency, or public source pages
and curate clean player-facing names, actions, parking certainty, direction,
and `at_mi` estimates.

Curate public operator locator feeds into offline route POIs:

```powershell
uv run python tools/curate_route_pois.py --json
uv run python tools/curate_route_pois.py --write-world --json
```

The curation helper reads Love's official store feed at
`https://www.loves.com/api/fetch_stores` and Pilot Flying J's official
paginated locator JSON at
`https://locations.pilotflyingj.com/search?per=50&offset=N&locations=all`.
It projects source coordinates onto checked-in route geometry, writes explicit
`at_mi` estimates, and records the source endpoint or direct operator page in
each stop note. A small hand-curated supplement covers corridors where those
operator feeds do not provide enough stops, using public agency, toll-authority,
rest-area, or truck-stop directory pages named in the source notes.

For full-network enrichment, run staged batches rather than hammering public
demo endpoints. The checked-in tool can fill missing corridor metadata in
resumable batches:

```powershell
uv run python tools/enrich_routes.py --enrich-all --write --limit 10 --rate-limit 1.0
```

The tool uses `.route-cache/` for OSRM geometry, Open-Meteo elevation samples,
state-boundary data, and optional Overpass candidate responses. The cache is
local tooling state and is ignored by git; the committed artifact is
`world.json`. Use `--no-overpass` when public Overpass endpoints are rate
limited or unavailable:

```powershell
uv run python tools/enrich_routes.py --enrich-all --write --no-overpass --rate-limit 0
```

The full-network batch completed route geometry, elevation/grade, state
context, and curated source-backed POI density for all 106 current legs.
Earlier generated midpoint POIs have been replaced rather than counted. Future
graph expansion must add named, source-backed truck stops, public rest areas,
service plazas, truck parking, or weigh stations before a new leg is allowed
into normal dispatch.

High-priority toll and service-plaza-heavy corridors are now covered:

- PA Turnpike / I-76: Philadelphia to Pittsburgh, including PA Turnpike toll
  events and named service plazas.
- Ohio/Indiana Turnpike and I-80/I-90: Cleveland to Chicago, including Ohio
  Turnpike and Indiana Toll Road settlement events plus service plazas.
- I-95 / New England toll corridor: New York to Boston, including New England
  Thruway and Massachusetts Turnpike toll events.
- I-95 Northeast Corridor south of Philadelphia: Philadelphia to Baltimore,
  including Delaware Turnpike and JFK Memorial Highway toll events.
- PA/Ohio Turnpike connector: Pittsburgh to Cleveland, including PA and Ohio
  toll settlement events.

For these corridors, toll-road service plazas and authorized emergency
road-service providers must remain explicit source-backed POIs/actions. Do not
represent them as generic public rest areas, and do not expose repair, towing,
or roadside assistance unless the source metadata backs that capability.

## HOS Safe-Stopping Audit Notes

The 2026-06-18 safe-stopping pass kept the existing full-network contract green
but added a stricter HOS/fatigue lens for southern corridors. A leg can satisfy
the minimum POI density rule and still feel poor when the first sleep/fuel stop
is hundreds of miles away for a driver who is already near a break, shift, or
fatigue limit.

This pass added source-backed stops to reduce those pressure gaps:

- Dallas to Albuquerque: Love's Travel Stop Wichita Falls and Flying J Travel
  Center Tucumcari.
- Dallas to St. Louis: Love's Travel Stop Ardmore and Love's Travel Stop
  Rolla.
- Atlanta to Dallas: Pilot Travel Center Tallapoosa and Love's Travel Stop
  Heflin.
- Nashville to Atlanta: Flying J Travel Center Resaca.

Use the same lens when adding or reviewing long southern and southwestern
corridors: check the maximum gap to a sleep-capable stop, check at least one
early safe stop near the first 150 miles of long hauls, and do not treat
limited public parking as guaranteed overnight truck parking. These are route
planning cues and POI metadata, not dispatch-board promises that a space is
available right now.

Emergency shoulder sleep exists only as an HOS/fatigue fallback when the truck
is stopped away from a route POI and the driver is already out of legal options
or severe fatigue leaves no nearby sleep-capable stop. It is intentionally
inferior to proper rest: ten hours pass and the ELD resets, but fatigue only
drops to the shoulder-rest floor, tickets and minor damage are possible, and
the delivery deadline keeps running. Keep the confirmation wording explicit and
spoken before the player commits.

## Update Process

1. Choose a corridor and confirm the route leg mileage already represents the
   intended highway path.
2. Run or review build-time route geometry and state-boundary data to place
   route points, state crossings, checkpoints, and state mileage.
3. Sample representative route points against an elevation source. Compute
   average grade between samples and classify sustained terrain. Flat corridors
   should stay flat instead of receiving generic rolling-hill waves.
4. Find truck-relevant public rest areas, travel centers, service plazas, or
   truck parking from public agency pages, official operator pages, or
   OSM/Overpass development-time queries.
5. Curate each POI into a clean name, normalized type, services, supported
   actions, and developer-facing source note. Do not commit raw OSM tags or IDs
   as player-facing text. Repair, towing, and roadside assistance must be
   explicitly source-backed in both `actions` and `services`.
6. Estimate `at_mi` from the leg's `from` city using route mileage, exit/mile
   marker data, or a map distance check. Do not place stops at regular
   intervals just to fill the route.
7. Add `source` notes that are specific enough for another developer to verify
   the stop later.
8. Run `uv run python tools/enrich_routes.py --coverage-report --json`.
9. Run `cargo test -p ff-core data_world`, `cargo test -p ff-core
   models::jobs`, and focused driving/POI tests.
10. For toll corridors, add or verify route-positioned `toll_events`, source
    notes, method labels, estimated commercial vehicle costs, and settlement
    behavior tests. Do not invent toll amounts without a named authority or
    documented rate source.

## Future Freight Data

FAF/BTS freight datasets now guide the checked-in market and facility model
without becoming runtime dependencies. See
[`docs/freight-market-facilities.md`](freight-market-facilities.md) for the
metro-market taxonomy, source strategy, representative facility templates, and
role-aware job generation rules. Corridor realism still owns the driven
itinerary: new freight must use metadata-backed routes, and freight-market
expansion must not silently invent unsupported road conditions.

## Accessibility Impact

Stop type labels remain spoken before the curated stop name, such as `public
rest area: Kenosha Safety Rest Area` or `travel center: Road Ranger Waco`. The
keyboard flow remains audio-first: stops are announced ahead, the cue includes
parking certainty such as `confirmed truck parking` or `limited truck parking`
(with the surveyed spot count appended when an official inventory covers the
lot, such as `confirmed truck parking, 43 spaces`),
`X` arms the exit, and `T` opens the POI menu when parked at one. Menu items are
generated from source-backed actions, so a rest area does not offer fuel or
repair by default, and a weigh station does not pretend to be a travel center.
`R` speaks route progress plus GPS context, including grade/terrain context.
`K` sets adaptive cruise, and the spoken cue includes the following gap,
bad-weather gap increases, and cancellation behavior. GPS and traffic cues
supplement the keyboard status keys; they never require a visual map or raw data
inspection. Toll warnings and toll-charged messages use concise speech and
carrier-settlement language, and delivery completion speaks gross pay,
carrier-paid or reimbursed charges, driver-responsibility charges, and net
driver pay so the ledger is accessible without reading a visual table.
