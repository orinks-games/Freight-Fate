# Truck-accessibility sweep brief: vehicle_access classification

For Oatis, in his own window, AFTER the map sharding lands (small
diffs, not 58 MB blobs). Spec agreed 2026-07-18 between Josh (via his
Codex analysis, quoted on the forum), the owner, and Phil. `[skip
changelog]` on data commits; the game-side filter commit gets a player
bullet.

## The problem

The live map carries 1,082 generic fuel_station records, all marked
"limited truck parking." Many are car-only convenience stops (the Wawa
convenience class) that a tractor-trailer physically cannot use. In an
audio-first game, "Press X to take the exit" is a promise the stop is
usable; a false stop is worse for planning than no stop -- it burns
driving time and can strand the player without a legal alternative.

## The classification (Josh's spec, verbatim intent)

Every stop gets a `vehicle_access` value, SEPARATE from parking:

- `tractor_trailer` -- announced and usable normally.
- `bobtail_only` -- retained on the map but hidden from semi stop
  announcements and HOS/"next legal stop" planning. Usable in 1.9 while
  GENUINELY bobtailing: tractor only. An empty trailer is NOT bobtail.
- `none` -- landmark only.

Access and parking are independent axes: a site might admit trucks for
fuel but offer no parking; another lot may not admit a combination
vehicle at all. Norm's parking buff may affect whether spaces are FULL
at legal truck stops; it must never make a physically inaccessible lot
accept semis.

## Classification rules

- DO NOT filter by brand. Some Exxon locations and the Wawa Travel
  Center are genuinely truck-oriented; brand is a hint for review
  ordering, never the verdict.
- Default rule: a generic fuel station is `bobtail_only` unless truck
  access is specifically verified (OSM `hgv=*`, `access` tags, truck
  parking presence via our self-hosted Overpass; TA/Petro/Loves/
  Flying J-class facilities and stops with surveyed truck parking are
  verified by nature).
- Sites that cannot admit any truck (geometry, `hgv=no`): `none`.
- Source notes per reclassified stop, per the world-data policy.

## Route gaps (supersedes the earlier degraded-parking idea)

Where the sweep leaves a long stretch with no `tractor_trailer`
service, DO NOT fake it: fill the gap with REAL facilities -- truck
stops, service plazas, rest areas, surveyed truck-parking locations --
found via Overpass and added per docs/map-enrichment-recipe.md. A
gap report (leg, longest serviceless stretch before/after) is part of
the deliverable so gaps get filled in the same effort.

## Game-side (Phil's half, separate commit)

Suppress `bobtail_only` and `none` from: exit-stop announcements, the
X-exit arming path, U readouts, route-planning fuel/sleep counts, HOS
"next legal stop" math, and the truck-stop tablet. Bobtail gating
(tractor only, no trailer attached) unlocks `bobtail_only` stops in
1.9. `none` may still appear as roadside texture.

## Big Buck's hook (owner)

`vehicle_access` covers PHYSICS. Policy bans (the lot fits a rig, the
venue forbids it) are a separate future flag the GOLDEN ANTLER pass
waives per visit; the pass never overrides vehicle_access.

## Gates

Sharded-format world edits only; `cargo tools index-world
--check`; world/route/overlay test suites; refresh_map_data.py lint
zero; spoken-text invariants (no raw OSM text) hold.

## Completion protocol (the Phil handshake)

Work in your own worktree (never the main checkout), branch
`map/truck-access-sweep`, START ONLY after the sharding commit is on
`fork/feat/career-1.9`. When the sweep is done and gates are green:

1. Commit and push `map/truck-access-sweep` to the fork.
2. Write the completion marker to the MAIN checkout's git-ignored logs
   dir: `C:/dev/Freight-Fate/logs/oatis-sweep-done.json` --
   `{"branch": "map/truck-access-sweep", "commit": "<sha>",
   "counts": {"tractor_trailer": n, "bobtail_only": n, "none": n},
   "gap_report": "<path in the branch>", "notes": "<anything Phil
   must read first>"}`.

Phil's overnight loop watches for that marker; on arrival he fetches
the branch, re-runs the gates, reviews flags and the gap report
against this brief, merges into feat/career-1.9, and records the
verdict in STATUS.md. If anything fails review, he writes
`logs/oatis-sweep-feedback.md` instead of merging -- check for it
before assuming the merge landed.
