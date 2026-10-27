# Facility approach street limits — agent brief

Goal: every facility approach street chain speaks its REAL posted speed
limit. Today the chain legs carry defaults — 25 mph for named streets, 15
for unnamed service ways — but a real arterial approach can be posted 35
or 45, and the owner heard miles of blanket 25 that no real city posts
(owner ask, 2026-07-24). The short 25/15 at the gate itself is true and
stays.

## Rules (non-negotiable)

- Work in `C:\dev\ff-streets` on branch `map/approach-limits` (already
  created, cut from the current line). Run `uv sync --group dev` first.
- The approach chains are baked by the facility-approach tooling
  (`tools/build_facility_approaches.py`, `tools/build_local_geometry.py`
  and friends) and live in the world data as street legs carrying
  `local_speed_mph` / `local_cue`. Find where the SOURCE data stores
  them and edit ONLY through `tools/world_source.py`
  (`load_world()` / `save_world()`), then regenerate with
  `cargo tools index-world` and verify `--check`.
- Offline and deterministic: self-hosted Overpass only (`OVERPASS_URL`,
  default http://localhost:12347/api/interpreter — it is up now). No
  public-API bulk sweeps.
- HONEST ABSENCE: set a leg's limit from OSM `maxspeed` ONLY where the
  matched way is actually tagged. Untagged named streets keep 25;
  untagged unnamed service ways keep 15. Never invent a number.
- Match ways by the chain's own geometry (the legs carry route points /
  geometry from the local-geometry bake — reuse whatever matcher the
  facility tooling already has before writing a new one).
- The facility GATE zone (the last half mile, 15 mph) is a game rule,
  not street data — do not touch it.
- Per-state (or per-batch) commits with `[skip changelog]` EXCEPT one
  final player-facing CHANGELOG bullet (players will hear real limits
  on approaches) — plain player language, it is read aloud.
- ROADMAP: check off the "Real speed limits for facility approach
  streets" bullet in the same change as the final batch, rewording it
  to what actually shipped.

## Gates

- `uv run pytest tests/test_world.py tests/test_world_overlay.py
  tests/test_weather_trip.py tests/test_driving_features.py` after data
  lands (the street-chain zone tests live in those).
- `uv run ruff check src tests tools`; index `--check` clean.
- Sanity report in the done marker: how many chain legs exist, how many
  got a real OSM limit, the limit distribution (a lot of 25s is fine;
  a 55 on a service way is a matcher bug — investigate before keeping).

## Done

Write `logs/oatis-approach-limits-done.json` with {chains, legs,
tagged_legs, coverage_pct, limit_histogram} and stop. Phil reviews and
merges via the marker.
