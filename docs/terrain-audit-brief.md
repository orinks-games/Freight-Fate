# Terrain audit brief: relief-aware reclassification

For Oatis, in his own worktree, AFTER the vehicle_access data sweep lands.
Player report by Josh, 2026-07-19: central Texas "shows as mountains."
Confirmed network-wide by Phil's census the same night. `[skip changelog]`
on the data commit; one player bullet for the spoken fix.

## The bug

`_terrain_for_grade` in `tools/enrich_routes_ors.py` labels any grade
segment steeper than 3.0% "mountain" and anything over 0.8% "hills" --
pure point-steepness, no relief context, no duration. Result: 743 Texas
grade segments say "mountain," including East Texas piney-woods legs
(Palestine-Lufkin 27, Tyler-Texarkana 22, Austin-Kerrville 30) where a
short 4-5% roller is just a creek crossing or a Hill Country dip.

What it breaks (and what it does not):

- SPOKEN: `Trip.terrain_at()` prefers the segment label; the status
  readout says "terrain mountain" in country no Texan would call that.
- HAZARDS: mountain-gated hazard defs in `sim/trip_models.py` ("a runaway
  truck on the grade ahead") can roll in places with no runaway ramp
  within five hundred miles.
- NOT physics: grade demand, braking, fuel all read the numeric
  `avg_grade_pct`. Do not touch the numbers -- only labels change.

Leg-level labels are wrong in BOTH directions (Phil's census, geometry
shards vs labels): 2 legs labeled mountain with flat evidence, but 186
labeled flat carrying mountain-scale relief (Bakersfield-Los Angeles over
the Grapevine is labeled FLAT at leg level). Segment labels usually
rescue the spoken side; leg labels still gate menu summaries and the
fallback grade amplitude, so fix both levels.

## The rule: terrain is relief in context, not one steep spot

Compute per-segment context from the archived elevation profiles
(`world_data/us/geometry/<state>.jsonl` -- delta-decoded lat/lon/elev;
decode: cumsum deltas / 10^q, elevation cumsum in meters; scale decoded
mileage to the leg's stored miles). For each grade segment take a window
of +/- 5 miles around it:

- `mountain`: |avg_grade_pct| >= 4.0 sustained for >= 1 mile AND window
  relief >= 1,000 ft; OR window relief >= 2,000 ft with repeated >= 3%
  pitches.
- `hills`: |avg_grade_pct| >= 1.5, or window relief >= 400 ft.
- `flat`: everything else.

Tune thresholds against the acceptance list below before the full write;
record the final numbers in the tool docstring. Median-filter the
elevation profile first (overpass/bridge spikes: the census caught a
"7.5% max grade" in Minnesota farmland that is a river-bluff bridge).

Leg-level label derives from reclassified segments: `mountain` if
mountain segments cover >= 10 miles or >= 15% of the leg; `hills` if
hills+mountain cover >= 20%; else `flat`. Keep the enrichment pipeline's
existing rank-merge behavior (never silently downgrade a curated label
without listing it in the report).

## Deliverables

1. `tools/reclassify_terrain.py` -- offline, deterministic, dry-run by
   default, `--write` to save via `tools/world_source.py`; per-state
   report of segment and leg label changes (state, leg, old -> new,
   window relief, sustained grade).
2. Updated `_terrain_for_grade` (or its replacement) so FUTURE enrichment
   runs classify with relief context, not the bare threshold.
3. Regenerated `world_data` via `index_world.py`, `--check` clean.

## Acceptance checks

- Palestine-Lufkin, Tyler-Texarkana, Austin-Kerrville, San
  Antonio-Kerrville: ZERO mountain segments after the pass (Hill Country
  reads hills, East Texas mostly flat/hills).
- Real Texas mountains survive: legs through the Davis/Guadalupe country
  (Van Horn, El Paso approaches) keep mountain where relief supports it.
- Ground truth cross-check: the 96 harvested runaway ramps
  (`world_data/us/gameplay/ramps.jsonl`) sit on legs/segments that are
  still mountain after the pass -- every ramp on a non-mountain segment
  is a rule failure to investigate, not a data edit.
- I-70 Denver-Silverthorne, Grapevine (Bakersfield-LA), Siskiyou,
  Monteagle (Chattanooga-Nashville) end up mountain at both levels.
- Census re-run shows the label->evidence mismatch table collapsing
  toward the diagonal; attach before/after tables to the PR.
- Spoken invariant: the status readout's "terrain X" changes ONLY where
  the report lists a change.

## Gates

`cargo tools index-world --check`; world/route/overlay suites;
`uv run pytest tests/test_world.py tests/test_world_overlay.py` plus the
full suite before push. Data commits `[skip changelog]`.

## Completion protocol

Same handshake as the access sweep: work on branch
`map/terrain-relief-audit`, push to the fork, then write
`C:/dev/Freight-Fate/logs/oatis-terrain-done.json` with branch, commit,
change counts per state, and the report path. Phil reviews against this
brief and merges.
