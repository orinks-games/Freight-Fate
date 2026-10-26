# Map Enrichment Recipe

The repeatable, per-leg process for bringing a leg up to full fidelity: real
named-place checkpoints, real truck-stop POIs, and fine-grained grade data.
It is written so any contributor or coding agent can execute it cold, batch
after batch, without re-deriving the judgment calls. It applies to three
jobs:

1. Enriching the existing sparse legacy legs (the backlog measured below).
2. Finishing new corridor legs right after they are built, so no new leg
   ships with placeholder-only content.
3. Periodic full-map review as the network grows toward national coverage.

It extends [`osm-routing-plan.md`](osm-routing-plan.md) (map architecture and
expansion plan) and [`route-stop-data.md`](route-stop-data.md) (leg schema and
determinism contract). Read those first if you have not.

## Invariants (non-negotiable, read before touching data)

- **Everything named is spoken.** Checkpoint `name` and `state` are narrated
  verbatim while driving ("Passing Seligman, Arizona on I-40"), and stop
  names are read in menus. No slug keys, no 2-letter state codes, no bare
  initialisms ("TA" must be "TA Travel Center"), no raw OSM tags.
- **Never invent a place.** Every checkpoint must be a real town or named
  locality you can attest is on that highway; every stop must be a real
  facility returned by a source query. A blind player hearing a fabricated
  town is worse than a sparse leg. "This leg genuinely has nothing" is an
  acceptable, honest result -- sparse rural legs are an intentional planning
  challenge.
- **Inspect leg content before changing it.** A leg that looks sparse by
  checkpoint count may carry rich hand-curated stops (this happened on I-10:
  "redundant" legs held real Van Horn / Fort Stockton / Sonora curation).
  Read the leg's current `stops` and `corridor` first; the job is to add,
  not to reset. Never delete a leg, stop, or checkpoint without reading it.
- **Curated mileage is pay.** `leg["miles"]` drives pay and deadlines. The
  enrichment steps below never change it (`--refresh-geometry` preserves it;
  `--adopt-ors-miles` is only for brand-new legs, always with `--only`). The
  one job that does change it is replacing a leg's route outright -- see
  *Putting a leg back on the road it is named for* -- and it says so in the
  changelog when it does, because players feel it in the pay.
- **Source notes on everything real.** Each checkpoint/stop records where it
  came from and how it was positioned. The placement tool writes these
  automatically; keep the convention if you edit by hand.
- **Data stays deterministic and offline.** All queries here are build-time.
  After any world-source edit: regenerate (`cargo tools index-world`),
  verify (`--check`), and run the world tests.
- **A new jurisdiction owes a truck-limit check.** Before the first leg in a
  state, province, or country goes live, settle whether heavy vehicles are
  held below the general limit there and record it (see *Jurisdiction truck
  limits* below). OSM tagging is too uneven to discover this per-leg, and the
  failure is silent: the map serves the car number and the game confidently
  speaks a limit no legal rig may drive.
- **Go through `tools/world_source.py`.** The source is per-state shards under
  `data/world_source/`, not one file. `load_world()` hands you
  the whole world as one dict and `save_world(data)` writes it back, so the
  tools in this recipe work exactly as they always have — but never open a
  shard directly, and never hand-edit one you have not read in full.

## Prerequisites

- `uv sync --group dev` (plus the `tooling` group for ORS: commands below use
  `uv run --group tooling` where needed).
- The self-hosted OpenRouteService server, for unlimited driving-hgv routing:
  `docker start ff-ors` if the container has exited (takes ~10-15 s to become
  healthy), then set `ORS_BASE_URL=http://localhost:8080/ors` and
  `ORS_API_KEY=selfhosted`.
- Overpass queries use the public endpoints (rate-limited, cached in
  `.route-cache/`; transient failures are skipped and retried on re-runs).
- **The self-hosted Overpass DB holds only the tags its extract was filtered
  to.** It carries roads, landmark polygons, and truck POIs -- and NO `place`
  nodes at all, so `node["place"="village"]` returns zero rows nationwide.
  That is a fact about the import filter, not about the world, and reading a
  zero out of it as "there is nothing there" is the same trap as reading a
  missing `maxspeed:hgv` as "no truck limit here". When a bake needs a tag the
  extract lacks, source it from the full Geofabrik US extract instead
  (`D:/ors/files/us-latest.osm.pbf`, scanned offline with pyosmium -- see
  `tools/extract_osm_places.py`) rather than widening the DB.

## Finding the backlog

Sparse legs are flagged by real-place checkpoint density:

```python
# run from tools/ (or with tools/ on sys.path)
from ffworld import World
w = World.load()
def density(leg):
    n = sum(1 for c in leg.checkpoints if c.type == "place"
            and "corridor between" not in c.name)
    return n / max(1.0, leg.miles) * 100.0, n
scored = sorted(((*density(leg), leg) for leg in w.legs), key=lambda t: t[0])
sparse = [t for t in scored if t[0] < 0.5]   # < 0.5 real places per 100 mi
```

Work the list biggest-mileage-first (single biggest wins), batched by
region or corridor so each batch reads as one reviewable content drop.

## The per-leg recipe

### 1. Snapshot the leg

Read the leg's current `stops`, `corridor.checkpoints`, `miles`, and
`highway` from `world.json`. Note what is already real (curated stops,
named checkpoints) -- that content is load-bearing and stays.

### 2. Generate candidate towns

Do not recall towns from memory -- it doesn't scale and mixes up
town-center vs interchange coordinates. Generate them from data:

```sh
uv run --group tooling python tools/corridor_places.py \
    --leg "from_slug:to_slug" --min-pop 250
```

This fetches the leg's real ORS route and prints every real GeoNames
populated place within 2 miles of it (the same buffer as the placement
gate, so listed candidates place cleanly), ordered along the drive, with
population and off-route distance, already formatted as `--candidate`
strings. Places that are already city nodes are dropped. Review the list:
prune suburbs of the endpoint cities and obvious over-density, keep the
real orientation towns. `--min-pop` tunes the floor -- start ~250; drop to
0 on a very empty western leg to surface the tiny waypoint towns, raise it
to skip clutter on a busy eastern one. `--min-spacing-mi` collapses
clusters to the largest place in a window.

The generator's coordinates come from GeoNames (accurate), so the step-5
placement gate rarely fires; a rejection means the town's center really is
more than 2 miles off the road, which is itself the correct signal to drop
it (you don't announce a town the driver doesn't pass).

### 3. Apply the node-vs-checkpoint rule

A candidate is a full CITY NODE (separate workflow: new city + legs) rather
than a checkpoint if it hits any of:

1. Junction with another BUILT interstate.
2. Freight generator: port, rail intermodal, real industry, energy or ag hub.
3. Spacing: the only stop breaking a gap longer than ~150 miles.
4. Real metro.

Otherwise it is a checkpoint. Tiebreaker: two candidates within ~30-40 miles
of each other -- the smaller one drops to checkpoint. Flag genuine coin-flips
for the map owner instead of deciding silently. A town can be BOTH a stop on
this leg today and a future node -- adding it as a checkpoint now does not
block promoting it later.

### 4. Validate the route geometry (before trusting it)

ORS's cost model occasionally prefers a road real trucks do not take -- it
routed San Antonio->El Paso down US-90 through Del Rio (scored 23 mi longer
but 6 minutes faster than I-10) while the leg's declared highway and curated
stops were I-10. Three signals of a wrong-road route, any of which means
stop and investigate before refreshing geometry or placing checkpoints:

- Several candidate towns from the declared highway get rejected off-route.
- The ORS distance differs from the curated mileage by more than ~4%.
- Existing curated stops sit far from the fetched polyline.

The fix is a curated `route_via` list on the leg (`[{"lat", "lon",
"note"}]`) pinning the route through a town on the declared highway; every
ORS fetch (geometry refresh, mileage adoption, enrichment) honors it and the
note records why. Sometimes the geometry is right and the LABEL is wrong
instead -- Billings->Salt Lake City is declared I-15 but genuinely runs
I-90 + US-191 + US-20 + I-15; keep the real route and use per-checkpoint
highway fields (below) so cues name the actual road.

**Correcting a wrong-road leg** (the full sequence, proven on San
Antonio->El Paso): set `route_via`; inspect the leg's auto-discovered
Overpass stops and REMOVE any that belong to the wrong road (check their
sources -- hand-curated stops with official-feed sources stay); run
`--adopt-ors-miles --only` for the real distance (pay/deadline change --
confirm with the map owner first); run `--refresh-geometry --engine ors
--only`; re-project the surviving curated stops onto the new geometry from
their real-world coordinates (`position_on_route` in
`tools/place_checkpoints.py`, noting the re-projection in each source);
rebuild `speed_limits` with `--add-maxspeed --only` (the old profile
described the old road); and sanity-check `interchanges` against exit-ref
mileposts. Then proceed with checkpoints and POIs as normal.

### 5. Position and write the checkpoints

Feed the reviewed candidate lines from step 2 to the placement tool (add a
`|place|<highway>` suffix on any where the road differs from the leg's
declared highway):

```sh
uv run --group tooling python tools/place_checkpoints.py \
    --leg "from_slug:to_slug" \
    --candidate "Seligman|35.3258|-112.8747|AZ" \
    --candidate "Big Sky|45.2841|-111.2460|MT|place|US-191" \
    --write
```

The tool matches each candidate to the nearest point on the leg's real ORS
driving-hgv polyline, converts it to the leg's own mile scale, and REJECTS
anything more than 2 miles off-route -- the sanity gate that catches wrong
towns, coordinate typos, and places on a different road. It writes spoken
names and full spoken state names, merges with existing checkpoints (curated
positions win), and drops the synthetic "corridor between" placeholder once
at least one real place covers the leg. The optional trailing fields set the
checkpoint type and its spoken highway (for legs whose declared highway
oversimplifies the real route). Investigate every rejection: it usually
means the coordinates or the town are wrong, not the gate.

One legitimate rejection cause (pilot lesson from Salina, Russell, and
Colby on I-70): a real on-route town whose CENTER sits more than 2 miles
from the road. Use the town's interchange coordinates instead -- that is
where the driver actually passes it. When unsure which side of town the
road runs, read the leg's polyline directly (filter `parsed["coordinates"]`
near the town's longitude and look at the latitudes) rather than guessing
twice. Never widen the gate; it is doing its job.

### 6. Discover truck-stop POIs

```sh
uv run python tools/enrich_routes.py --add-overpass-pois \
    --only "from_slug:to_slug" --write
```

This queries mid-corridor sample points AND both endpoint cities (endpoint
clusters are where real truck stops live -- Tucumcari, Barstow, Kingman were
all missed before the endpoint fix). Additive only; existing curation is
deduped against, never replaced. Check every new name for initialisms and
non-spoken text before committing. An empty result at a rural leg or a metro
downtown is honest -- do not force a stop. Known gap: a metro's beltway truck
stops can sit outside the endpoint search box; add those by hand with source
notes if the leg needs fuel support.

### 7. Refresh geometry and grades

```sh
uv run --group tooling python tools/enrich_routes.py --refresh-geometry \
    --engine ors --only "from_slug:to_slug" --write
```

Re-derives `route_points`, `elevation_samples`, and `grade_segments` from
the real ORS polyline with fine 0.25-mile grade bins -- the fidelity that
feeds grade physics (and later fuel burn and maintenance). Curated miles,
stops, checkpoints, tolls, and crossings are preserved. Legs enriched before
the fine-grade fix (mid-2026) under-report real climbs (Kingman-Flagstaff
showed 1.37% max on a 4,000 ft climb), so run this for every touched leg.

### 8. Verify and commit

```sh
cargo tools index-world && cargo tools index-world --check
cargo test -p ff-core data_world
uv run pytest tests/test_index_world.py tests/test_curate_route_pois.py
uv run ruff check tests tools
```

Then re-run the density snippet: the leg should now clear 0.5 real places
per 100 miles (most legs land well above). Spot-review the spoken strings of
everything added (read each new name and state aloud). Commit the batch with
real source notes in the message; data-only batches that change nothing
player-visible take `[skip changelog]`, but a batch that adds audible places
to legs players can drive deserves a changelog entry under `Added`.

## Jurisdiction truck limits (once per state/province/country)

Many jurisdictions post one limit for cars and a lower one for heavy vehicles.
A rig's limit therefore reaches the player by two independent routes, and
**both must be handled or the map lies in one direction or the other**:

1. **Tagged.** OSM carries `maxspeed:hgv` on the way. The baked sample records
   it with `hgv: true` and it is already truck-correct.
2. **Statutory.** OSM carries only the general `maxspeed`. Nothing in the data
   says trucks are held lower, so `STATE_TRUCK_MAX_MPH`
   (`crates/ff-core/src/sim/trip_models.rs`) pulls it down at runtime.

Route 2 exists because **OSM tagging coverage is not a fact about the law.**
California I-80 alternates mile by mile between tagged and untagged; the
statutory cap is what makes the whole corridor read 55 instead of flickering.
Never conclude "this road has no truck limit" from missing tags.

**Both routes must speak identically.** `truck_limit_at()` reports a
truck-specific limit whichever way it arose, so S says "Truck limit 55.
California holds trucks to this." on tagged and untagged miles alike. Keying
off the cap alone silences the tagged roads — that regression shipped and was
caught by a player on US-395 (2026-07-19); `crates/ff-core/tests/it/sim_maxspeed.rs` locks it.

**Source the number, don't infer it.** Cite the statute or the DOT table, with
the access date, in the comment above the entry. These laws move — several US
states repealed their splits in the last decade — so a stale table is worse
than none now that the game names the jurisdiction out loud.

**Verify in-engine, never from the baked file.** The file holds the car
number on untagged stretches by design; reading it and concluding the limit is
wrong is a mistake that has been made twice. Build a `Trip` and call
`speed_limit_at()`: a throwaway test in
`crates/ff-core/tests/it/sim_maxspeed.rs` can use that file's helpers.

```rust
#[test]
fn probe_sacramento_reno() {
    let mut trip = trip_for(first_route_option(world(), "Sacramento", "Reno"), "california");
    for m in [5.0, 20.0, 50.0, 90.0, 110.0, 125.0] {
        println!("{m} {:?} {:?}", trip.speed_limit_at(m), trip.truck_limit_at(m));
    }
}
```

Run it with `cargo test -p ff-core --test it probe_sacramento_reno --
--nocapture`, read the numbers, and delete the probe.

### Taking this off the US grid

`STATE_TRUCK_MAX_MPH` is keyed by US state name in mph, matched against
`Leg.state_miles`/`state_crossings`. Canada, Mexico, and Europe all need the
same mechanic and none of them fit that shape, so expanding the map means
generalizing it rather than adding rows:

- **Key by jurisdiction, not state.** Provinces, Länder, and national defaults
  all set truck limits; the lookup wants a jurisdiction code that a leg can
  carry regardless of country.
- **Store canonical units.** Most of the world posts km/h and caps heavy
  vehicles nationally (commonly 80–90 km/h). Storing mph and converting at the
  edges will round wrongly and speak numbers no sign shows.
- **National default, jurisdiction override.** Outside the US the cap is
  usually a country-level rule a region may tighten — the opposite of the flat
  state table here.
- **The speech must stay honest.** "California holds trucks to this" becomes
  whatever names the real authority; do not attribute a national rule to a
  province.

Until that lands, a non-US corridor with a statutory truck limit **cannot be
served correctly** and should be flagged rather than quietly shipped on car
numbers.

## Capturing alternate routes during a scrub

The enrichment pass is when real alternate routes surface: a leg's ORS route
disagrees with its declared highway, a second comparable road obviously
exists (Billings->Salt Lake City runs US-191 through Gallatin Canyon, but
I-90->Butte->I-15 is the all-interstate alternative), or a >150-mile span
clearly wants an intermediate node. Route choice belongs to the GRAPH, not
to any one leg -- `World.route_options` already returns multiple distinct
routes when the graph offers them, and the planned dispatch briefing will
let the player weigh time, fuel, grades, and weather between them. So when
you spot an alternate during a scrub, do not bake a detour into the existing
leg's geometry. Record it in the batch's findings as one of:

1. **Node promotions** (preferred): intermediate towns that hit the node
   rule (Bozeman, Idaho Falls, Pocatello, Ogden, Salina, Topeka...). Once
   promoted and legged, the alternates emerge from pathfinding naturally,
   and the long atomic leg gets split on the next pass.
2. **A parallel leg** between the same two cities, when both routes
   genuinely serve the same city pair end-to-end (a supported pattern --
   several already exist). The new leg gets its own highway label, its own
   `route_via` to pin its road, and the full recipe run.

Hand the recorded list to the map owner with the batch -- promotions and new
legs are corridor work with their own review, not silent additions inside an
enrichment commit.

## Batching guidance

- Batch by corridor or region (like the I-40 and I-40 desert-town passes),
  roughly 5-10 legs per commit, so review stays tractable.
- The full test suite runs once per batch; the focused tests above run per
  leg while iterating.
- ORS responses and Overpass responses are cached in `.route-cache/`;
  re-running a batch is cheap and safe (all steps are idempotent).
- The endpoint-city POI queries are cache-keyed per CITY, so working
  corridor-by-corridor gets cheaper as coverage grows.

## Putting a leg back on the road it is named for

The one job on this page that DOES change `leg["miles"]`, and therefore pay
and deadlines. It is not enrichment; it is replacing the leg's route, and
then enriching the new one.

A leg can be labelled for an interstate its baked route never joins. Two
different faults look identical from inside the data and a truck router tells
them apart, so start by measuring rather than reading the label:

```
uv run python tools/curve_valhalla_facts.py --all      # map-matched coverage
uv run python tools/probe_leg_labels.py --out .route-cache/label-split.json
```

That prints two lists. A leg whose ROUTER route barely touches the shield has
the wrong LABEL, and `tools/repair_leg_labels.py --split` renames it. A leg
whose router route rides the shield has the wrong ROUTE, and it wants this:

```
uv run python tools/reroute_leg.py --leg <from_slug>:<to_slug> --write
uv run --group tooling python tools/reroute_enrich.py \
    --all-pending --pbf ~/osm/us-latest.osm.pbf --write
```

`reroute_leg` settles the route, writes the polyline and the mileage, and
drops every corridor layer that was a reading along the OLD road. Curated
checkpoints are the exception: they are real towns with real coordinates, so
they are re-positioned onto the new road and only dropped if it now runs more
than three miles from them. Curated TOLL EVENTS are dropped and printed --
nothing downstream puts them back, so a leg that still tolls wants
re-curating by hand.

`reroute_enrich` rebuilds everything else and refuses to call a leg finished
if any layer came back empty or at less than half its old size. Between the
two, the leg is INCOMPLETE, and `--check` on either tool lists any leg left
that way. Run `cargo tools index-world` afterwards like any world edit, then the
curve chain (`curve_valhalla_facts.py --all`, `bake_curve_connectors.py
--write`, `clamp_curve_advisories.py --write`, `screen_curve_artifacts.py`),
because the bends moved with the road.

Two things bite:

- **`--only` takes SLUGS on every tool in the chain**, semicolon-separated:
  `corpus_christi_tx_us->san_antonio_tx_us`, never `Corpus Christi->San
  Antonio`. The interchange family builds one index over a 12 GB extract per
  run, so pass all the legs at once or pay for that read per leg.
- **The interchange sub-mode flags do not compose.** `--maxspeed
  --restrictions --ramp-controls` together dispatches to one and silently
  skips the rest. `reroute_enrich` runs them in sequence.

## Building new corridors (composition)

The corridor-building pattern (pick nodes by the rule in step 3, add slug
cities with `spoken_city` + code state/country, add legs, `--enrich-all
--engine ors --no-overpass`, then `--adopt-ors-miles --only "<new legs>"
--write`) is documented in the map plan and proven on I-40/I-10. THIS recipe
is the finishing pass: run steps 1-7 for every new leg immediately after
building, so no corridor ships placeholder checkpoints or misses its
endpoint truck stops. One PR per corridor, each a player-facing content drop
with its own changelog line.

## Spidering the network (finding new legs and cities)

Distinct from enriching existing legs: this fills GAPS in the graph where a
whole corridor or region has no coverage (the "Liam effect"). The same
`corridor_places.py` generator drives it in **node mode** -- give it two
anchor cities on a highway and a node-scale population floor:

```sh
uv run --group tooling python tools/corridor_places.py \
    --from-coord "39.5296,-119.8138" --to-coord "40.7608,-111.8910" \
    --highway I-80 --min-pop 3000 --min-spacing-mi 25
```

It lists the real towns along that corridor that are NOT already nodes,
biggest first, ordered along the route -- the raw material for deciding
which become dispatchable cities. Apply the node rule (step 3) to each,
flag the coin-flips for the map owner, then build the chosen nodes + legs
(the corridor-building pattern above) and run the full recipe on each new
leg. Work outward corridor by corridor (one interstate at a time), so the
network grows connected rather than as isolated islands. New nodes are a
bigger commitment than checkpoints (dispatchable, priced, facilities) --
always get owner sign-off on the node list before building.
