# Gap Analyzer Brief — the missing-alternates queue (Track: map, 2026-07-30)

## Why

The 2026-07-30 completeness audit measured the network at 623 cities /
1,287 legs with **zero same-pair parallel alternate legs anywhere** —
every "alternate" we have is a multi-hop path through different cities.
Route-selection-at-dispatch (queued feature) has no fuel until pairs
have real alternatives. ORS on MS-02 already holds the COMPLETE US
graph (built from the full us-latest.osm.pbf), so the real-world
alternatives are sitting there unqueried. This tool asks for them and
diffs them against what we ship.

## The job (report-only — writes NOTHING to world data)

Build `tools/analyze_route_gaps.py`:

1. For every existing leg (city pair), query the LOCAL ORS for driving
   routes WITH alternatives (the ORS `alternative_routes` option, up to
   3; HGV profile if the instance has it, else car — note which in the
   report). Find the endpoint coordinates and the ORS URL the way the
   existing enrichment tools do (`tools/enrich_routes*.py` reads the
   env/config — reuse, do not reinvent). Local instance only, never
   public. If ORS is down, recovery is documented:
   `docker compose -f D:/ors/docker-compose.yml up -d` (volumes
   survive; see the infra-recovery memory).
2. For each returned alternative, measure DIVERGENCE from the existing
   baked leg: sample the alt geometry every ~2 mi and take the fraction
   of samples farther than ~3 mi from the existing leg's route_points
   polyline. Divergence >= ~0.4 means a genuinely different road, not a
   ramp-level wiggle.
3. For each divergent alternative, extract: the named highways it rides
   (ORS segment naming / way refs), its miles vs the existing leg's,
   and a coarse road-class read (interstate / US / state).
4. Rank candidates: prefer high divergence, plausible freight roads,
   and meaningful tradeoffs (shorter-but-slower, longer-but-flatter,
   toll-vs-free where detectable). Emit:
   - `logs/route-gaps.json` — full machine-readable results.
   - A spoken-style top-50 summary in the report: one line per
     candidate, e.g. "nashville->memphis: I-40 exists (210 mi); US-70
     alternate diverges 78%, 198 mi, US-highway class."
   - Honest counts: pairs analyzed, pairs where ORS offered no real
     alternative (that is DATA, not failure — flat truth beats padding).

## Rules

- Branch off `feat/career-2.0` in this worktree (e.g. `map/gap-analyzer`).
  Tool + report only: NO world_source writes, NO game code, NO changelog
  (report tooling, `[skip changelog]`).
- Deterministic: same ORS graph + same legs -> same report. Sort
  everything; no wall-clock in outputs (stamp the report from an
  --as-of arg).
- Do NOT rank Canada/Mexico or off-graph dreams — US pairs we already
  serve. Scope is the missing-parallel queue, nothing else.
- 1,287 pairs against a local ORS is minutes-to-an-hour of queries;
  batch politely anyway (the box also serves other windows).
- Done marker `logs/oatis-gap-analyzer-done.json` for Phil: branch,
  tip, headline counts (pairs with >=1 real alternate, top candidates),
  and anything that smelled wrong. STATUS.md entry per house style.

## What happens with the output (not this job)

Phil + owner review the ranked queue; approved candidates become
normal spider/recipe corridor builds (curated legs, full enrichment) —
the alternates then feed route-selection-at-dispatch. The analyzer
itself reruns cheaply after every build wave to measure shrink.
