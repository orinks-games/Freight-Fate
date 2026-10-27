# Lane-data bake — agent brief

Goal: every leg's corridor learns how many lanes the real road has, so
future mechanics (passing traffic, lane-end cues, exit-lane guidance,
closures) have honest data waiting. This is a DATA-LAYER bake only —
nothing in the game reads it yet, exactly like grades and dense speed
limits were baked ahead of their physics.

## Rules (non-negotiable)

- Work in `C:\dev\ff-map` on a fresh branch off `feat/career-1.9`
  (suggest `map/lane-bake`). Never this brief's authoring checkout.
- Edit world data ONLY through `tools/world_source.py` (`load_world()` /
  `save_world(data)`), then `cargo tools index-world` and
  verify with `--check`. Never touch the shards or `world_data/` by hand.
- Offline and deterministic: the self-hosted Overpass (`OVERPASS_URL`,
  ff-overpass :12347). If it is down, read
  the infra-recovery memory/runbook (`docker compose -f
  D:/ors/docker-compose.yml up -d`) before starting; do not fall back to
  the public API for a bulk sweep.
- Reuse the Job 2 (dense maxspeed) way-matching machinery — the sweep
  that walked all 1,287 legs' route points against OSM ways. Lanes ride
  the same matches; do not invent a new matcher.
- HONEST ABSENCE: where OSM has no `lanes` tag, store nothing. No
  defaults, no guesses — the runtime can default by road class later.
  Add source notes per the data conventions; no raw OSM tags in
  player-facing anything (this layer is not spoken at all).

## Schema

Nest under each leg's `corridor` (same home as `grade_segments`):

```
corridor.lane_segments = [
  {"start_mi": float, "end_mi": float, "lanes": int,        # total, both directions
   "lanes_forward": int | absent, "lanes_backward": int | absent,
   "oneway": true | absent},
  ...
]
```

- Record `lanes:forward` / `lanes:backward` when tagged; omit otherwise.
- Merge adjacent segments with identical values; drop segments shorter
  than ~0.3 mi except where they end at an interchange (lane counts
  genuinely change fast there — keep those).
- Segment mileposts follow the leg's native direction, same convention
  as grade segments.

## Method

1. Per state shard (small reviewable diffs): for each leg starting in
   the state, walk its route points, match ways via the Job 2 matcher,
   read `lanes` / `lanes:forward` / `lanes:backward` / `oneway`.
2. Build segments, merge, store under `corridor.lane_segments`.
3. Regenerate the index; run
   `uv run pytest tests/test_world.py tests/test_world_overlay.py`.
4. Commit per batch of states:
   `data(map): lane segments for <states> [skip changelog]` — this is
   not player-facing until a mechanic reads it.
5. Track coverage: report % of route-miles with lane data per state in
   the final summary; low-coverage rural states are expected and fine.

## Done

- All 1,287 legs swept; index in sync; world tests green; ruff clean.
- Write `logs/oatis-lane-bake-done.json` with {legs, states, coverage
  percent, segment count} when finished.
- Update ROADMAP.md: add the lane-data layer under the current release
  line as shipped-data/awaiting-mechanic, in the same change as the
  final batch.
