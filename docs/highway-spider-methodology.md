# Highway Spider — implementation methodology

Answers [`highway-spider-brief.md`](highway-spider-brief.md). Fable designed
this; Opus implements it as `tools/spider_*.py` and runs it corridor by
corridor. It reuses the existing tooling (`corridor_places.py`,
`place_checkpoints.py`, `enrich_routes.py`, `add_nodes`, `fix_terrain`,
`pick_nodes.py`) and the per-leg process in
[`map-enrichment-recipe.md`](map-enrichment-recipe.md) unchanged; everything
new here is the layer above them: which corridors, which towns become nodes,
how legs are wired, and how the result is validated.

The one-sentence version: **the unit of work is the highway corridor, not the
city.** Each corridor goes through five stages — TRACE, PLAN, APPROVE, BUILD,
AUDIT — and is then sealed in a coverage ledger. The map owner approves
exactly one artifact per corridor (the plan file, yay/nay per line); every
other step is autonomous, gated by the test suite and per-batch commits.

---

## 1. End state, phases, and what "touched" means

A corridor is **sealed** when all of these hold:

1. Its real truck route is traced end to end and cached.
2. Every town on it that passes the node rule (section 3) is a dispatchable
   city, and every such city is wired per the wiring rules (section 4).
3. Adjacent nodes along it are within the region's max-gap target, or the gap
   is recorded as an honest gap (a real empty stretch, never a fabricated
   town).
4. Every leg on it is dispatch-complete via the full enrichment recipe
   (checkpoints, truck-stop POIs, fine grades, re-derived terrain).
5. The realism audit (section 6) passes for the corridor, and the world test
   suite passes.

Phases, in order:

- **Phase 0 — data prep.** Build the corridor inventory, traces, junction
  graph, and the spider tools themselves. No world.json changes.
- **Phase 1 — primary interstates.** The ~66 one- and two-digit interstates.
  Many are already partially covered; those are completion passes, which are
  cheap and high-value.
- **Phase 2 — US-highway trunks.** US highways that carry real long-haul
  truck traffic where no interstate shadows them (the shadow rule, section
  2.2). Examples: US-2 (Hi-Line), US-83, US-87, US-95, US-97, US-287, US-395,
  US-54, US-64. This phase is where "touch every highway for trucks" lands.
- **Phase 3 — deferred.** Three-digit interstates (beltways add no dispatch
  cities; skip except a curated exception list of real intercity spurs like
  I-27's planned extensions), state-route corridors on demand, and Canadian
  corridors (separate future effort per the Alcan plan).

Rough end-state estimate, for sizing expectations: Phase 1 adds on the order
of 120–170 cities (Grand Junction, Grand Forks, Bowling Green, Paducah,
Gillette, Great Falls, Erie-corridor infill, etc.); Phase 2 another 100–150.
Endpoint around **550–650 cities and 1,400–1,700 legs** — roughly doubling
today's 293/626 — with density that varies by region on purpose.

## 2. Phase 0 — new data and tools (build these first)

### 2.1 Corridor inventory: `data/spider/corridors.json`

One entry per corridor. Drafted by Opus, spot-reviewed by the map owner
(termini and waypoints only — it is a plan input, not world data). Schema:

- `ref`: shield label, e.g. `"I-70"`, `"US-83"`.
- `spoken`: spoken name, e.g. `"Interstate 70"`.
- `phase`: 1 or 2.
- `tier`: priority tier (section 7).
- `waypoints`: ordered list of `{lat, lon, note}` — the two termini plus
  enough intermediate pins that the ORS truck route provably follows THIS
  road and not a faster parallel one. Pins are the same defense as
  `route_via` on legs (pitfall 6 in the brief); a corridor like I-90 needs
  pins at Spokane, Billings, Rapid City, Albany so ORS cannot shortcut.
- `status`: `unstarted | traced | planned | approved | built | sealed`.

### 2.2 The shadow rule (which US highways qualify for Phase 2)

A US-highway corridor (or a segment of one) earns a pass only where it is
**not shadowed by an interstate**: no interstate runs concurrent or parallel
within ~25 miles serving the same city pairs. Shadowed segments are excluded
from the trace — US-87 between Raton and Denver is just I-25 and gets
nothing; US-87 north of Billings to Great Falls is real. This is the
corridor-level version of the concurrent-highway trap (pitfall 2) and is
checked mechanically: overlap of the US highway's trace with all built
interstate traces (section 4.5), plus judgment on the plan review.

### 2.3 Traces: `tools/spider_trace.py`

For each corridor, fetch the ORS driving-hgv route along the waypoint chain
and cache the polyline (plus per-point cumulative miles) under
`data/spider/traces/`. Verify the trace: each waypoint's `note` names the
road it pins; sample GeoNames towns known to sit on the corridor and check
they fall within 2 miles of the polyline (same buffer as the checkpoint
gate). A trace that fails gets more pins, not a wider gate.

### 2.4 Junction graph: `data/spider/junctions.json`

Derived, not hand-authored: intersect every pair of corridor traces; where
two polylines pass within ~3 miles of each other, record a junction with
both refs, the crossing coordinate, and the nearest GeoNames town. This is
what powers junction auto-promotion (section 3) and needs no road-network
data — it works from traces alone. Upgrade path: when the Overpass extract
is widened to roads (post-RAM-upgrade), junctions can be re-derived from
actual route relations for exactness; the schema stays the same.

### 2.5 The driver tools

- `tools/spider_plan.py` — read-only. Given a corridor ref: walk the trace,
  list candidate towns (via `corridor_places.py` spider mode against the
  full GeoNames US dump), apply the node rules, compute the wiring plan
  (section 4), and emit the **plan file** (section 5.2). Never touches
  world.json.
- `tools/spider_build.py` — executes an approved plan file: inserts cities,
  inserts bare legs, performs corridor splits, then drives the existing
  enrichment recipe per new leg. Idempotent and resumable (skips plan items
  already present in world.json).
- `tools/spider_audit.py` — the realism validator (section 6). Runs per
  corridor at seal time and map-wide on demand.
- `data/spider/coverage.json` — the ledger: per corridor, its status, stats
  (nodes added, legs added/split, honest gaps), and seal date.

## 3. Node selection rules

Applied to every GeoNames populated place within 2 miles of the corridor
trace, in three passes: hard promotes, hard rejects, then scoring for the
gaps that remain.

### 3.1 Hard promotes (no scoring needed)

1. **Junction city.** Nearest real town to a junction of two Phase-1
   corridors (from `junctions.json`), when no existing node is within 30
   miles. Population is irrelevant — junctions are where route choice lives,
   and the picker needs a node there to expose alternates (pitfall 4).
   Interstate-by-US-trunk junctions promote too once both corridors are in
   scope; interstate-by-minor-road junctions do not.
2. **Gap breaker.** The best town inside a span that would otherwise exceed
   the region's max gap (section 3.3). This is the Tonopah rule: population
   2,500 is plenty when it is the only real place in 200 miles.
3. **Real metro.** Population over 150,000 (or a state capital) on the
   corridor that is somehow still not a node.

### 3.2 Hard rejects

1. **Too close.** Within 30 miles of an existing or planned node on the same
   corridor — unless it is a hard promote, in which case the TIE rule below
   applies. Twin cities (Odessa/Midland precedent): the smaller becomes a
   spur node only if it independently passes a hard promote; otherwise it
   stays a checkpoint.
2. **Off-route.** Town center (or its interchange) more than 2 miles from
   the trace. It may still be a future spur candidate (Hot Springs
   precedent) — log it, do not promote it.
3. **Suburb.** Inside a promoted metro's cluster (the existing 30-mile
   dedupe in `pick_nodes.py`).

TIE rule: two hard promotes within 30–40 miles of each other — keep the one
with the higher junction weight, then the larger population; the loser drops
to checkpoint. Genuine coin flips go to the plan file flagged `COINFLIP` for
the owner, never decided silently (recipe step 3).

### 3.3 Region spacing bands

Target spacing is the ideal distance between adjacent dispatch nodes along a
corridor; max gap is where a gap breaker gets promoted. Grouped over the 16
canonical regions:

- **Dense** (northeast, great_lakes, florida, atlantic_southeast,
  corn_belt): target 70–130 miles, max gap 170.
- **Standard** (appalachia, mid_south, gulf_coast, heartland,
  southern_plains, upper_midwest, california): target 100–180 miles, max gap
  240.
- **Sparse** (rockies, great_basin, desert_southwest, pacific_northwest):
  target 130–260 miles, max gap 320 — and beyond that, if GeoNames offers no
  real town in the span, the gap is recorded as **honest** in the coverage
  ledger and left alone (pitfall 7; I-80 across Wyoming and US-50 across
  Nevada are meant to feel like that).

Rationale: a game day is roughly 600 driving miles under HOS; dense regions
should offer a dispatch decision every hour or two, sparse regions should
make the player plan fuel and hours around real emptiness.

### 3.4 Scoring the middle

For spans wider than target but under max gap, candidates are ranked
lexicographically — junction weight first (built×built = 3, built×phase-2
trunk = 2, trunk×trunk = 1, none = 0), then gap relief (how close the
resulting two sub-spans land to the target band), then log-population.
Population floors for scored (non-hard-promote) candidates: dense 20,000;
standard 8,000; sparse 1,000. A freight-generator overlay (curated list of
ports, intermodal ramps, energy and ag hubs — offline, source-noted) adds a
+1 junction-weight equivalent when present; it is optional data and the
spider must work without it.

Calibrate once on the pilot corridor (section 7), then freeze the constants
in the tool so every later corridor is judged by the same rules.

## 4. Wiring rules

How an approved node attaches to the graph. All of section 4 is computed in
the PLAN stage and reviewed as text before anything is built.

### 4.1 Nearest-on-corridor wiring

A new node N on corridor H wires to its immediate along-trace neighbors on H
— the nearest existing-or-planned node in each direction along the trace
(pitfall 1). Never to the far endpoints of whatever long leg it happens to
sit on. If N is the corridor's terminus, it wires one direction only.

### 4.2 The corridor split operation

When N sits ON an existing leg A–B (projected within 2.5 miles of that leg's
`route_points`) and A–B's route follows the same road, the leg A–B is
**replaced** by A–N and N–B. Duplicate-in-parallel is the failure mode this
kills: keeping A–B alongside A–N–B would give the picker two "distinct"
routes (different city sequences) that are physically the same asphalt.

Split mechanics — fresh-build, then migrate, then delete, in that order:

1. Read the old leg fully first (recipe invariant: inspect before changing).
   Snapshot its curated stops and checkpoints, which carry real lat/lon.
2. Build A–N and N–B as brand-new legs: ORS geometry, `--adopt-ors-miles`
   (new legs are the sanctioned case for mileage adoption), fine grades,
   `fix_terrain`'s mileage-weighted terrain (pitfall 5), and any `route_via`
   the old leg carried (re-anchored to whichever half the pin falls in).
3. Migrate every curated stop and named checkpoint from the snapshot onto
   the half whose polyline it projects onto, re-positioning by coordinate
   (`position_on_route`), and note the re-projection in each source field.
4. Diff: every named thing in the snapshot must exist on exactly one half.
   Zero-loss is machine-checked, not eyeballed.
5. Delete the old leg, regenerate the world index, bump the expected leg
   count in the coverage test, run the suite.

Exception — keep both (the Billings–Salt Lake precedent): if the old leg's
geometry runs a genuinely different road (polyline overlap with A–N–B under
the concurrency threshold, section 4.5), it is a true alternate. Keep it,
and make sure its highway label and per-checkpoint highways say the real
road.

### 4.3 Spurs

A hard-promoted node within ~40 miles of a much larger neighbor on the same
corridor (twin cities, Kerrville-style hill towns) gets a single spur leg to
that neighbor. The backtrack penalty is tiny and honest. Spur nodes are the
one sanctioned degree-1 exception in the audit.

### 4.4 Alternates emerge; do not fabricate them

With corridor-first coverage, the alternate-route problem mostly dissolves:
once both corridors of a real-world choice are built through their own nodes
(I-5 and US-97; I-10 and US-90), `route_options` surfaces the choice by
itself, because the picker distinguishes routes by their city sequences. So
the spider never creates a parallel duplicate leg to "add a route." The only
sanctioned parallel leg between the same pair is two genuinely different
roads serving the pair end to end with no promotable town on the alternate —
rare; it takes its own highway label, its own `route_via`, and plan-file
approval (this is today's supported pattern, kept as the escape hatch).

### 4.5 Concurrency detection

Before creating any leg, fetch its ORS route and compare against every
existing leg's polyline within the same bounding box: sample points every 2
miles; if more than 60% of one route's samples fall within 1.5 miles of the
other's polyline, they are concurrent (pitfall 2 — I-20/I-59
Birmingham–Meridian). A concurrent proposal is dropped and recorded in the
plan file as a concurrency note (the existing leg may deserve a dual shield
in its spoken highway, e.g. "Interstate 20 and 59" — flagged, not
auto-edited).

### 4.6 Per-leg constraint checklist (mechanical, in `spider_build`)

- Unordered endpoint pair unique in the graph.
- 10 ≤ miles ≤ 800 (splits fix the long-leg legacy cases like the 783-mile
  Atlanta–Dallas leg as nodes land on them).
- Terrain re-derived mileage-weighted, never MAX-segment.
- Both cities: region equals `classify_region(state, lat, lon)`, coords in
  the CONUS box, slug/spoken fields clean.
- Full-graph reachability after every batch.
- Chain repair: if the trace passes through two existing nodes that are
  adjacent along the corridor but share no leg, add the missing leg (this
  catches historical gaps, not just new nodes).

## 5. The spider algorithm

### 5.1 Outer loop

```text
for corridor in corridors.json ordered by (tier, phase):
    TRACE:   trace = ors_route(corridor.waypoints); verify pins; cache
    PLAN:    plan = spider_plan(corridor, trace)      # read-only
    APPROVE: owner marks plan file lines yay/nay      # the human gate
    BUILD:   spider_build(plan)                       # batched commits
    AUDIT:   spider_audit(corridor); full test suite
    SEAL:    coverage.json updated; PR opened against dev
```

Corridors build serially (a tool loads and saves the whole world; parallel
edits conflict even though the source is now sharded per state),
but TRACE and PLAN are read-only and can run ahead for the next several
corridors while the current one builds.

### 5.2 PLAN stage detail

```text
def spider_plan(corridor, trace):
    towns = corridor_places(trace, buffer=2mi, all_pops)   # GeoNames US dump
    nodes_on_route = project existing cities onto trace (within 2.5 mi)
    spans = consecutive (node, node) stretches along trace

    plan = []
    for span in spans:
        promotes  = hard_promotes(towns_in(span))           # 3.1
        rejected  = hard_rejects(towns_in(span))            # 3.2
        if span.miles > max_gap(region):  ensure gap breaker in promotes
        elif span.miles > target(region): promotes += best_scored(span)  # 3.4
        for n in promotes:
            wiring = nearest_on_corridor(n)                  # 4.1
            splits = legs_n_sits_on(n)                       # 4.2 vs keep-both
            plan  += NodeItem(n, wiring, splits, evidence)
    plan += chain_repairs(nodes_on_route)                    # 4.6
    plan += concurrency_notes + COINFLIP items + honest_gaps
    write plan file: one line per item, yay/nay slots, MY DEFAULT stated
```

The plan file uses the exact format that worked in the overnight enrichment
run: plain text, line by line, no tables, each item carrying a stated
default so silence means consent to the default — except node creation,
which always requires an explicit yay (nodes are dispatchable, priced
things; the standing rule from that run holds: never create a city
unattended).

### 5.3 BUILD stage detail

```text
def spider_build(plan):
    for batch in plan.batches(size≈5–8 legs):
        insert approved cities (add_nodes; slug, spoken, region-verified)
        insert bare legs / perform splits (4.2 order: build, migrate, diff, delete)
        for each new leg:
            enrich: --enrich-all --engine ors; --adopt-ors-miles --only (new legs)
            recipe steps 2–7: checkpoints, POIs (+rural fallback), geometry+grades
            fix_terrain
        index_world + --check; focused world tests; commit batch
    full test suite once per corridor
```

Wrong-road detection during BUILD follows the recipe verbatim (mileage
mismatch >4%, off-route candidate rejections, stops far from polyline);
fixes are `route_via` pins. A pin that changes a leg's real mileage is a pay
change → it goes to the plan file's questions section, never silent.

## 6. The realism audit (`tools/spider_audit.py`)

Run per corridor at seal time; run map-wide after every few corridors.

1. **Detour check.** For sampled city pairs (all pairs under 400 miles
   apart, plus N random pairs per distance band up to cross-country),
   compare the graph's best `route_options` miles against the ORS truck
   route between the same cities. Ratio above 1.12 flags a coverage gap; the
   flagged pairs cluster along whatever corridor is missing and feed the
   worklist. This single check is what turns "did we cover enough?" from a
   feeling into a number.
2. **Physical-duplicate check.** For every pair with 2+ route options,
   overlay the options' leg polylines; two options overlapping above the
   concurrency threshold (4.5) are the same road wearing two city sequences
   — a wiring bug (usually a missed split).
3. **Alternate sanity.** For a curated list of known dual-route pairs
   (Sacramento–Portland via I-5 vs US-97; San Antonio–Van Horn via I-10 vs
   US-90; Denver–Albuquerque via I-25 vs US-285; Spokane–Boise via I-84 vs
   US-95…), assert `route_options` really returns both, fastest first, and
   that each alternate fits the extra-miles cap (22% or 75–550 extra miles —
   the engine's own constants). This list grows as corridors seal.
4. **Structure stats.** Reachability; node degree (degree-1 only for
   sanctioned spurs); per-region spacing distribution vs the bands; honest
   gaps enumerated; leg mileage bounds; terrain distribution sanity
   (mountain share by region — a flat-region "mountain" leg is a fix_terrain
   escape).
5. **Spoken-text lint.** The existing tests already enforce it; the audit
   just runs them, plus a scan of new names for initialisms and raw OSM
   text.

## 7. Corridor priority order

Tier 1 — pilot plus completion passes on partially built corridors (high
player value per unit of work, all anchors already exist):

1. **I-70 Denver–Salt Lake City — the pilot.** Entirely missing today
   (Denver's only westward connection is a detour), it is the marquee
   mountain corridor — Eisenhower Tunnel, Vail Pass, Glenwood Canyon, the
   San Rafael Swell honest gap — and it feeds the grade-physics work
   directly. It exercises every rule in this doc on one medium-sized
   corridor: new nodes (Grand Junction hard-promotes as a metro; Glenwood
   Springs scores in), sparse-region spacing, honest gaps, splits (none —
   clean build), and the audit. Calibrate the scoring constants here, get
   the plan-file review rhythm right, then freeze both.
2. I-90 completion (Gillette and the Wyoming gap; already dense elsewhere).
3. I-94 completion (Great Falls tie-in via I-15 north, Minot/US-2 prep).
4. I-95 completion (the Carolinas coastal infill: Florence, Rocky Mount).
5. I-75 / I-65 / I-71 midwest–south infill (Bowling Green, Paducah via
   I-24).
6. I-29 north (Grand Forks), I-35 completion (Duluth end, Laredo end
   double-check).

Tier 2 — remaining Phase-1 primaries, ordered by how many audit detour flags
they clear (the audit literally prioritizes this tier after each seal).

Tier 3 — Phase 2 US trunks, seeded with: US-2, US-83, US-87 (MT), US-95,
US-97, US-287, US-395, US-50 (NV, as honest-gap showcase), US-54, US-60,
US-64, US-70. Each gets the shadow-rule segmentation before planning.

## 8. Operating model (who does what, and the gates)

- **Fable** owns this methodology, reviews audit output when corridors
  disagree with the rules, and handles escalations (region-model changes,
  save-compatibility calls, rule amendments). Rule changes are edits to this
  doc, versioned in git — the constants live in code, the judgment lives
  here.
- **Opus** implements the Phase-0 tools, then runs the loop: one corridor
  per working session, TRACE/PLAN read-ahead allowed, serial BUILDs.
- **The map owner** reviews plan files (yay/nay per line, defaults stated),
  answers questions-section items (anything touching pay/mileage, leg
  deletion outside a planned split, coin flips), and merges corridor PRs.

Mechanics, all proven in the enrichment run: branch per corridor off `dev`;
commits per batch of ~5–8 legs with real source notes; data-only prep
commits take `[skip changelog]`; each corridor PR carries one player-language
changelog entry under Added ("You can now haul freight along Interstate 70
through the Colorado Rockies — new cities Grand Junction and Glenwood
Springs, with real towns, truck stops, and mountain grades the whole way").
Caches (`.route-cache/`, trace cache) make re-runs cheap; every stage is
idempotent, so a session that dies mid-corridor resumes from the last
committed batch. A run report per corridor appends to the plan file — legs
built, nodes added, honest gaps, oddities — the same morning-report shape as
before.

## 9. Risks and open questions

1. **Save compatibility on splits.** Deleting a leg (section 4.2) may strand
   a save that references it mid-drive. Needs a one-time answer from the
   game side (Josh): either a load-time remap of orphaned legs onto the
   nearest new leg, or batching splits into release boundaries. Until
   answered, splits are still safe to build — the risk is player-facing only
   at release time.
2. **Region model gaps.** The spider will surface them (Idaho's north is
   PNW-in-spirit but classified great_basin; the audit's region checks will
   flag more). Each is a small coordinate-split rule in `regions.py`,
   owner-approved, done as its own commit — pitfall 8.
3. **ORS wrong-road rate on US trunks.** Interstates pin easily; two-lane US
   highways will fight ORS's cost model more (the Del Rio lesson). Budget
   more `route_via` pins per Phase-2 corridor and expect the plan files to
   carry more questions.
4. **Name collisions.** GeoNames has many duplicate town names (two Las
   Vegas already). Slugs disambiguate by state, but spoken names must too —
   the plan file prints spoken names exactly as they will be heard, so
   collisions get caught at review.
5. **corridors.json accuracy.** Waypoint mistakes produce confidently wrong
   traces. Mitigation: trace verification (2.3) plus the owner's spot review
   of the inventory before Phase 1 starts.
6. **Scope creep toward infinity.** The inventory IS the termination
   condition: corridors not in `corridors.json` do not exist for the spider.
   Adding one is a deliberate, reviewed edit — never an autonomous decision.
