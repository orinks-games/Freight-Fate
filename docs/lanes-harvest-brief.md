# Lane-data harvest brief (Job 3) — specced 2026-07-16

Owner-approved 2026-07-16 (bedtime design session): real lane counts and
lane-level facts from OSM, baked with the same offline pipeline, shard
pattern, and determinism rules as the dense maxspeed sweep (Job 2). This
is the data half of the steering/lane realism work; no gameplay consumer
ships in this job. Follow the rules exactly and record what shipped in
ROADMAP.md per AGENTS.md.

## Context

The game models discrete lanes but invents their count. OSM carries the
truth: `lanes`, `lanes:forward/backward`, `turn:lanes`,
`destination:lanes`, `change:lanes`, `overtaking`, `oneway`, and the
`highway=` class. Harvesting them buys, in later wiring jobs: real lane
counts spoken and enforced (four lanes through Albuquerque, two-plus-two
rural), REAL lane drops as genuine merge events at real mileposts,
exit-only lane warnings ("the right lane becomes exit only"), keep-right
pressure that knows how many lanes exist, no-passing zones on two-lane
surface roads, and the lane grammar the steering design (offered to
Josh) will need. Bake now, wire later.

## Sources and pipeline

- Offline Geofabrik PBF extracts first (`D:\ors\files\`, pyosmium — the
  escape-ramp harvest pattern); self-hosted Overpass (`ff-overpass`,
  port 12347) for anything the extracts lack. Health-check before
  relying on it; recovery runbook in project memory.
- **The self-hosted Overpass extract is tag-filtered and is NOT a
  general OSM mirror** (learned the hard way, 2026-07-20: the village
  sweep found `node["place"]` returns zero nationwide because the
  extract kept only roads, landmarks, and truck POIs). A healthy status
  endpoint tells you the server is up, not that it holds the tags you
  need — probe for an actual known feature before trusting a nil result.
- **Rebuild the extract for this job, and widen it while you are in
  there** (owner, 2026-07-20). If it is being rebuilt for lanes,
  interchanges, and `destination:*`, include `place=village|town` in the
  same pass: that retires the bespoke `tools/extract_osm_places.py`
  written during the village sweep and leaves one query path instead of
  two. Note this is gated on the Crucial RAM RMA — see
  [[project-overpass-selfhost]].
- Sample along each leg's real routed geometry like Job 2, but lane
  facts change at way boundaries, not continuously: record a step
  function of transitions, not dense samples.

## What to harvest, per leg direction of travel

1. **Lane count step function.** Prefer `lanes:forward`/`backward` for
   the travel direction; on `oneway=yes` ways take `lanes` whole; on
   undivided two-way ways without direction tags, `lanes` split evenly
   rounding down (a bare `lanes=3` two-way is 1 for us plus a center
   context — record 1 and flag `estimated`). Clamp 1..6; an absurd tag
   (0, negative, non-numeric) is logged and skipped, never baked.
2. **Lane drops.** Wherever the count decreases along travel, derive a
   merge record: at_mi, from-count, to-count, side if `turn:lanes` or
   way geometry proves it (merge-left vs merge-right), else side
   unknown. Only real transitions — never synthesized.
3. **Exit-only lanes.** Near baked interchanges, a `turn:lanes` entry
   whose rightmost lane carries only right-ish arrows into a
   motorway_link marks that lane exit-only: record interchange key,
   at_mi where the marking starts, and lanes_total there.
4. **No-passing / no-change zones** (surface legs): `overtaking=no`
   and restrictive `change:lanes` become [start_mi, end_mi] zones.
5. **Carriageway class per segment:** divided (separate oneway ways)
   vs undivided two-way, from the way topology and `oneway` — the
   future oncoming-traffic tier needs to know which roads have it.
6. **destination:lanes** raw strings, stored verbatim per transition
   where present (future gantry speech; no processing this job, and
   never player-facing raw — OSM tag text must not leak into speech).
7. **Lane-to-destination mapping** (added 2026-07-20; scope corrected
   after checking what is already baked).

   **We already have the destinations.** `Leg.interchanges` carries
   18,011 records harvested 2026-06-23 from `highway=motorway_junction`
   sign tags: `destinations` on **91.5%**, `exit_ref` on **92.6%**,
   `via` on 68.8%, both ref and destinations on 84.1%. A real record:

   ```
   at_mi 1.4  exit_ref '126A'  via 'US 31 South;US 280'
   destinations ('Hoover', 'Homewood', 'Carraway Boulevard')
   ```

   So "toward Nashville" is **not** blocked on this job — that half of
   the owner's sentence can be spoken today. My earlier framing of this
   item as "harvest the destination tags" was wrong; they are harvested.

   What is genuinely missing is the **lane** half: which lane serves
   which destination. That needs `turn:lanes` and `destination:lanes` on
   the approach ways, joined to the interchange records above by exit
   ref. Harvest that join, per direction of travel, under the same
   verbatim-storage and never-raw-in-speech rule as item 6.

   Practical consequence for sequencing: **"stay left for I-40 toward
   Nashville" needs only the side**, which ramp geometry can often prove
   without `turn:lanes`. **"Take the second lane" needs this item.** Ship
   the first without waiting on the second.

## Storage

`world_data/us/gameplay/lanes.jsonl`, one meta header line (schema
version, data_version hash, params) then per-leg records, delta-encoded
step functions with `[at_mi, next)` semantics exactly like
`speed_limits.jsonl`. Deterministic: stable key ordering, sorted
records, byte-identical re-runs on unchanged inputs. Source notes per
the data rules. Absence policy is explicit: a stretch with no lane tag
bakes NOTHING (runtime heuristics stay in charge there) — never bake a
guessed count without an `estimated` flag.

## Acceptance

- Coverage report printed by the bake: percent of interstate miles with
  a real (non-estimated) lane count; expect motorway coverage to be
  high, surface coverage patchy — report, do not pad.
- Spot checks: I-40 through Albuquerque shows 3+ lanes somewhere in the
  metro; rural I-40 Arizona shows 2; at least one derived lane drop
  verified against aerial/OSM by hand and named in the PR body.
- A lanes linter (same spirit as the anchor linter, added as a test on
  the fresh bake): no count outside 1..6, no zero-length zones, no
  drop record without a real transition behind it, monotone at_mi.
- `cargo tools index-world --check`; `uv run pytest tests/test_world.py
  tests/test_world_overlay.py`; then full gates.
- `[skip changelog]` is correct for this job — data with no consumer is
  not player-facing yet; the wiring jobs carry the changelog entries.
  ROADMAP: check off the lane-counts bullet with what actually shipped.

## For the wiring jobs: advisory may guess, punitive may not

Recorded here because it constrains what this bake must flag, not just
what it must collect (owner design session, 2026-07-20).

The planned lane mechanic makes a wrong lane cost the player a wrong
exit, with the GPS routing them back. That is the right call on realism
and it is the first mechanic where **a gap in our data becomes a penalty
against the player.** If `turn:lanes` is absent and we guess, a driver
who did everything right gets pulled off the interstate — and a blind
player has no way to tell our missing tag from their own mistake. The
truck-access sweep drew this line already: a cue with no real referent
is refused.

So:

- **Enforce wrong-lane consequences only where lane data is real
  (non-`estimated`).** Everywhere else the guidance degrades to the side
  it can prove — "stay left" — and costs nothing if ignored.
- **Never speak a lane ordinal we did not harvest.** "Take the second
  lane" requires a real `turn:lanes`/`destination:lanes`; with only a
  count and a side, say the side.
- This makes the `estimated` flag from item 1 load-bearing at runtime
  rather than merely informational, so it must be baked accurately and
  never defaulted to "real" for a tidier record.

Advisory guidance may run on partial data. Punitive guidance may not.

### The owner's closed loop resolves this (2026-07-20)

Owner pushed back: a sighted driver who misses an exit also has to get
back on, so why is a consequence unfair? He is right, and the objection
above was aimed slightly wrong. The issue is not that a penalty exists —
it is **information parity**. A sighted driver had signs, lane arrows,
and the traffic around them; they were told and did not act. Our player
has only what we speak. A penalty is fair exactly when we told them.

His design supplies the telling, and it is better than what ships today.
Current exits announce the required lane once, several miles out, and
never say whether the driver is actually **in** it — the one fact a
blind driver cannot check. Modern lane-level GPS closes that loop, and
so should we:

- Announce the exit and required lane once, at distance.
- Then compare **required lane against the lane the truck is actually
  in** — the sim already models this, so it needs no harvested data.
- Speak again only when the driver is **wrong**, escalating as the exit
  closes ("still need the right lane" → "right lane now").
- Confirm once at the commit point when they are right, then go quiet.

Establish the contract explicitly in the manual: **the game will tell you
if you are in the wrong lane, so silence means you are fine.** Silence is
only usable as information when the player knows it would have been
broken. That is the same contract the rest of the warning layer already
keeps, and it is what a real co-driver does — nothing when you are set,
insistent when you are not ([[feedback-amplify-real-cues-never-invent]]).

With that loop running, missing the exit **is** the driver's doing, and
the reroute is honest consequence rather than a penalty for our silence.
The restriction above then narrows usefully: it governs the lane
*ordinal* ("second lane"), which needs harvested `turn:lanes`, not the
side, which we can already prove.

## Exit recovery: bake the way back, do not route it

Owner asked (2026-07-20) for *some* surface streets — just around exits
and critical interchanges, enough that a wrong-lane mistake can be
recovered — and whether the RAM ceiling allows it.

**On memory: the ceiling does not apply to this.** The limit is on the
Overpass and ORS *servers*, which hold indexed graphs in RAM. Offline
pyosmium streams the PBF and is bounded by disk and wall-clock, not
memory — the village sweep harvested 26,894 places that way on the
current hardware, at a moment when the Overpass extract contained zero
`place` nodes. So the geometry harvest can start before the 96 GB kit
lands.

**But routing is the wrong shape for this problem.** A general surface
router needs ORS to build a much larger graph, which *is* RAM-bound, and
it would make recovery non-deterministic — against the data rules, which
require the world to load offline and behave identically every run.

Take the exit at Hoover the wrong way and there is **one obvious way back
on**. That is not a routing problem; it is a fact about that
interchange, and facts get baked:

- Per interchange, bake a **recovery path**: the short sequence back to
  the mainline in the intended direction, with the turns spoken as
  instructions ("right at the light, then right onto the ramp").
- Store the ramp/street names it traverses so the speech is real, not
  synthesized geometry.
- Deterministic, offline, and small — on the order of a few hundred
  bytes across 18,011 interchanges, against a 60 MB world.
- **No runtime routing, no ORS dependency, no memory question.**

Scope suggestion: start with **interchange splits** (interstate-to-
interstate, where a wrong lane commits you to another highway and the
consequence is real) rather than every service exit. Those are the cases
the owner named, and they are a small fraction of the 18,011.

Absence policy as everywhere else: an interchange whose recovery cannot
be established from real ways bakes **nothing**, and the wrong-lane
consequence stays disabled there rather than stranding the driver.

## Sequencing

Worktree, per the no-parallel-world-data rule. Not alongside any other
bake job. Overpass health first if the extracts prove insufficient.

**Order set by the owner, 2026-07-20:**

1. Phil folds the shipped work (villages merge, snapping fix, the
   `[[docs/nav-phrasing-brief]]` mainline phrasing, stub-checkpoint
   deletion).
2. The truck speed-limit audit lands (see
   `[[project-truck-speed-limit-audit]]` — proposal written, three owner
   decisions outstanding).
3. Then this job: lanes, interchanges, and exit recovery.
4. The Overpass extract rebuild rides with step 3, widened to include
   `place` so `tools/extract_osm_places.py` retires.

## Fold this into the map utility, not just this job

Owner, 2026-07-20: everything here — lane scanning, interchange
harvesting, recovery paths, place sweeps — belongs in the reusable map
tooling (`tools/refresh_map_data.py` and the enrichment recipe), not as
one-off scripts, **because North America is next.** When the 96 GB kit
lands and Canadian and Mexican corridors come in, these passes must be
runnable against a new region by pointing them at a different extract,
not by rewriting them.

Concretely, each pass this job adds should be:

- **Region-parameterized**, not US-hardcoded — no embedded state lists,
  no `_us` slug assumptions, no mph-only storage.
- **Reported by the refresh tool** the way `--radio`, `--limits-lint`,
  and `--stops` already report: coverage in, gaps out, curation left to
  a human.
- **Documented in the recipe** as a per-region step, alongside the
  jurisdiction truck-limit check added 2026-07-20 — which is the same
  lesson in a different domain: the thing that breaks on a new continent
  is the assumption nobody wrote down.

See the recipe's *Taking this off the US grid* section for the specific
US-isms already identified (state-name keying, mph storage, a flat
jurisdiction table with no national-default layer).
