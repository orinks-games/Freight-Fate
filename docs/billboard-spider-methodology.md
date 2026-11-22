# Billboard Spider — implementation methodology

Sibling of [`highway-spider-methodology.md`](highway-spider-methodology.md).
The highway spider builds the road graph (nodes and legs); the billboard spider
decorates a **built** corridor with geographically-placed roadside billboards.
It creates no dispatch cities and no legs — it only bakes billboard landmarks
onto legs that already exist, so it can run over the current map today and as a
follow-on step when a new corridor seals.

The one-sentence version: **the unit of work is the real roadside attraction.**
Each attraction near a corridor becomes one placed billboard landmark on the
nearest leg, at a milepost a little before you reach it, carrying authored
parody copy and gated by the existing `chatter_billboards` switch. The spider
**surfaces the geography**; a human **writes the joke**. The map owner approves
one artifact per corridor: the sign sheet (yay/nay per line).

Why a spider and not a hand-keyed table: a billboard belongs at a real *spot*,
not "somewhere on I-90." Keying signs by highway shield (or shield+state) is too
coarse — Wall Drug must fire in western South Dakota, not on I-90 in
Massachusetts. Anchoring each sign to the attraction's real coordinate makes it
geographic by construction: it speaks because you are actually approaching the
thing.

**That paragraph described a live defect for as long as it has existed.** The
hand-keyed pool in `billboards.rs` *was* keyed on shield alone, so every sign
naming a place was true the whole length of an interstate: I-40 runs through
Oklahoma and Tennessee both, and a tester duly heard the Oklahoma signs outside
Knoxville. Worse, the key threw the shield's prefix away, so US-90 and AZ-90
inherited I-90's roadside too. Fixed 2026-08-24 — each corridor line now
carries a `SignAnchor` (`Corridor` for a line naming no place, `States` for a
region or a town the world does not model, `Approaching` for a world city
within `SIGN_APPROACH_MI` of route ahead), the lookup matches interstates only,
and the placer refuses a sign wherever its copy is untrue, falling through to
the anywhere pool.

The two mechanisms are not rivals and should not be merged. The spider bakes a
*measured* milepost for a real attraction; the anchored pool carries *authored*
lines whose subject is often a region, a song's home turf, or a town the map
has no node for, and for which no coordinate exists to measure. A sign that
earns a real coordinate should graduate from the pool to a baked landmark; the
rest keep their anchors. This is the provenance rule in `AGENTS.md` applied to
placement: an anchor is deliberately coarse-grained so that a *derived*
placement never wears the shape of a *measured* one.

---

## 1. End state — when a corridor's billboards are "done"

A corridor is **signed** when:

1. Every attraction in scope near its trace (section 3) has either an approved
   placed billboard or an explicit `skip` in the sign sheet.
2. Placed billboards respect the spacing rule (section 4.2) against each other
   and against existing landmarks, so a stretch never stacks into a wall.
3. Every placed sign passes the spoken-text lint (numbers spelled out, no map
   tags, real names nominative, invented ad copy, source-noted).
4. The world test suite passes and the leg/landmark counts are updated.

The billboard layer is **additive** — it never deletes or moves a leg — so it
carries none of the highway spider's save-compatibility risk from splits.

## 2. The rendering contract (one-time code change, the only "jump to 1.9")

Placed billboards ride the **existing** landmark path, so almost no code is
new. Verified in `sim/trip.py` and `states/driving_events.py`:

- `_place_landmarks` reads each `leg.landmarks` record (`at_mi`, `name`,
  `category`, `spoken`) and speaks it at its real milepost — the same rail the
  Loneliest Road `highway_marker`, museums, and river crossings already use.
- Both `LANDMARK` and `BILLBOARD` events are gated at speak time by
  `settings.chatter_enabled(category)` and muted by terse speech
  (`driving_events.py:50-57`). The gate keys off the **category** string.

So a placed billboard is a landmark record with `kind: "point"` and a category
that maps to `chatter_billboards`. The one-time mainline change (a small PR that
reaches `dev` through the routine dev↔1.9 sync — do it once, then the spider
only ever writes data):

1. **Add a distinct category `"billboard_sign"`** — NOT the existing
   `"billboard"`. The save-resume dedup at `trip.py:1071` classifies callouts by
   `category == "billboard"` into `_announced_billboards`, while placed
   landmarks are checked against `_announced_landmarks`. Reusing `"billboard"`
   would misfile a placed sign on resume and let it re-fire. A distinct category
   sidesteps that entirely.
2. **Map it to the billboards switch:** add `"billboard_sign":
   "chatter_billboards"` to `settings.py` `CHATTER_CATEGORY_FIELDS`. This is what
   makes the answer to "does the billboards verbosity switch speak these too?"
   **yes** — both the written pool billboards (`"billboard"`) and the placed
   ones (`"billboard_sign"`) resolve to `chatter_billboards`, and terse mutes
   both. Skip this mapping and `chatter_enabled` defaults unknown categories to
   *on* — they would speak but the switch could not mute them (a bug).
3. **Allow it as a bake category:** add `"billboard_sign"` to
   `world_parsing.py` `LANDMARK_CATEGORIES` (the strict allowlist that rejects
   unknown categories as bake bugs) and to the bake tool's mirror set.
4. **Match the spoken form:** the pool path prefixes `"Billboard: "`; placed
   landmarks speak `spoken` raw. Decide once whether placed signs carry the
   prefix (recommend: yes, bake the lead word into the tool so the two sources
   sound identical) and keep it consistent.

Everything after this is pure map data on `dev`.

## 3. Candidate sourcing — surfacing the attractions

Two feeds, merged and deduped, restricted to within a buffer of the corridor
trace (reuse the 2-mile checkpoint buffer).

### 3.1 The self-hosted Overpass filter (bulk)

Against the `ff-overpass` extract (see the self-hosted Overpass work), pull
roadside-relevant POIs along the trace: `tourism=attraction | theme_park | zoo |
viewpoint | museum`, `roadside`-flavored `man_made`/`artwork`, "world's
largest…" named oddities. This is the same dev-time bake shape as the OSM
roadside narration. Real billboards are not in OSM — the *attractions they
advertise* are.

### 3.2 The curated seed list (the famous ones OSM misses)

Iconic roadside-culture stops are under-tagged in OSM, so keep a curated,
source-noted table with real coordinates: `data/spider/billboards_seed.json`,
one row per attraction (`name`, `lat`, `lon`, `corridors`, `real_note`, an
optional `countdown: true` for marquee multi-sign approaches). Seed it from what
is already known (section 10). Adding a row is a reviewed edit, same discipline
as the corridor inventory.

### 3.3 Relevance and dedup

- Drop candidates already spoken as another landmark (a museum that fires as
  `museum` should not also get a billboard, unless the billboard is a distinct
  bit — owner call, flagged).
- Collapse duplicates within ~5 miles to the single best-known attraction.
- Cap per corridor by the spacing rule (4.2), not by a hard count; log anything
  dropped for spacing so coverage is honest.

## 4. Placement rules

### 4.1 Which leg, which milepost

Project the attraction coordinate onto the nearest leg's polyline (reuse
`position_on_route`). Place the sign a **lead distance before** the projection
along the direction of travel — default ~8–15 miles, so it reads as an approach.
A placed billboard faces one way (owner ruling, 2026-09-30): it is heard only
in the direction its sheet names, because its copy says "ahead" and from the
far side of the road that is false. Write the sheet's `leg:` the way the driver
reads the sign, with `at_mi` counted from that end; `tools/bake_billboards.py`
mirrors the milepost onto a leg stored the other way round and records
`directions` on the record. The other direction needs its own sign, placed
before the attraction from that side. A `landmark` block faces the same way
unless it carries `- facing: both`, for a line true from either side, such
as a monument named as the truck passes it.

### 4.2 Spacing

Reuse `LANDMARK_MIN_SPACING_MI`. A placed billboard within that distance of an
existing landmark or another placed billboard is dropped (best-known wins) — the
same thinning `_place_landmarks` applies to river clusters. This keeps signs
from stacking and keeps them behind real geography.

### 4.3 Marquee approach countdowns (optional, curated)

For a few legends (Wall Drug, South of the Border, Big Buck's), a single sign
undersells the real experience — those attractions really do post a countdown
for hundreds of miles. A seed row flagged `countdown: true` bakes a short escalating
series at, e.g., `at − {150, 60, 15}` miles, each its own `billboard_sign`
record, subject to the same spacing thinning. Use sparingly; most attractions
get one sign.

### 4.4 One per attraction, no repeats

Each attraction yields at most one sign per direction (or one countdown set).
The pool path already guarantees no text repeats within a trip; placed signs are
distinct records, so a corridor never passes the same billboard twice.

## 5. The copy — the authored gate

This is the step the spider cannot automate, and the reason the map owner's
artifact is a **sign sheet**, not a plan of nodes:

- For each surfaced attraction, Opus drafts the parody copy from the
  attraction's real facts (pulled with the candidate). Register: short enough to
  read at seventy, funny, the real attraction named **nominatively**, ad text
  **invented** (never lifted), numbers spelled out, no codes or map tags — the
  same rules `billboards.py` already enforces in tests.
- The owner reviews the sheet line by line: keep / reword / skip, and answers
  the trademark-closeness call per sign (how near to echo a real slogan — the
  standing Big Buck's / radio-licensing judgment).
- Approved copy plus its placement becomes the bake input.

Node creation in the highway spider always needs an explicit yay; here the
equivalent standing rule is: **no billboard copy is baked without an explicit
approval** on the sheet. Silence is not consent for a sign that will be heard.

## 6. Stages and tooling

```text
for corridor in signing order:
    TRACE:   reuse the highway spider's cached trace
    SURFACE: billboard_surface(corridor)     # read-only: 3.1 + 3.2, dedup, spacing
    DRAFT:   Opus writes parody copy per surfaced attraction  # the authored step
    APPROVE: owner marks the sign sheet yay/nay/reword        # the human gate
    BAKE:    billboard_bake(sheet)           # writes billboard_sign landmarks
    AUDIT:   index_world --check; world tests; spoken lint; spacing check
    SEAL:    billboards ledger updated; PR opened against dev
```

- `tools/billboard_surface.py` — read-only. Walks a trace, queries the Overpass
  extract + the seed list, applies relevance/dedup/spacing, emits the sign
  sheet (attraction, coords, target leg + milepost, real facts, blank copy
  slots). Never touches `world.json`.
- `tools/billboard_bake.py` — executes an approved sheet: inserts
  `billboard_sign` landmark records onto the target legs, re-projecting by
  coordinate, source-noting each. Idempotent (skips signs already present),
  additive (no leg edits), then `index_world`.
- `data/spider/billboards_seed.json` — the curated attraction table (3.2).
- `data/spider/billboards_coverage.json` — the ledger: per corridor, signs
  placed, attractions skipped and why, spacing drops, seal date.

## 7. The audit

Lighter than the highway audit, because nothing structural changes:

1. **Spoken-text lint.** Run the existing billboard/landmark spoken tests plus a
   scan of new copy for digits, initialisms, and raw OSM text.
2. **Spacing check.** No two `billboard_sign` records (or a sign and a landmark)
   within `LANDMARK_MIN_SPACING_MI` on the same stretch.
3. **Placement sanity.** Every `billboard_sign` projects within the buffer of
   its leg's polyline; `at_mi` within the leg; direction resolution symmetric.
4. **Switch check.** A tiny unit test that `chatter_enabled("billboard_sign")`
   follows `chatter_billboards`, and that terse speech mutes it — the guard that
   answers the verbosity question and prevents a future regression.
5. **Coverage.** Attractions in scope with no sign are either approved skips or
   spacing drops, all recorded — never silently missing.

## 8. Operating model and lanes

- **Fable** owns this methodology; rule changes are edits to this doc.
- **Opus** builds the Phase-0 tools, runs SURFACE, and drafts the copy.
- **The map owner** reviews sign sheets (keep/reword/skip, trademark calls) and
  merges corridor PRs.
- **Lanes:** the placed billboards are **map data → PRs to `dev`** (this revises
  the earlier "billboards are 1.9 code, never map PRs" note — that still holds
  for the corridor-agnostic pools and the state welcome signs, which stay in
  `billboards.py`/`state_welcome.py` on the 1.9 line). The one-time rendering
  contract (section 2) is a small mainline PR that reaches both lanes via the
  normal sync.
- **Mechanics:** branch per corridor off `dev`; data-only bake commits take
  `[skip changelog]`; each corridor PR carries one player-language changelog
  entry under Added ("New roadside billboards along Interstate 90 across South
  Dakota — Wall Drug's countdown starts a very long way out").

## 9. Risks and open questions

1. **Copy is the bottleneck, by design.** Surfacing is cheap; writing a good
   sign is not. Batch the DRAFT step (many attractions, one authoring pass) and
   let the owner cull; do not gate SURFACE on it.
2. **OSM attraction sparsity.** The self-hosted extract is POI-filtered; some
   legends will only come from the seed list. That is expected — the seed list
   is a first-class feed, not a fallback.
3. **Trademark / parody closeness.** Per-sign owner judgment, same as Big
   Buck's; the sheet prints the real name and the invented copy side by side so
   the call is explicit.
4. **Prefix / voice consistency.** The pool path and the placed path must sound
   the same (section 2.4); a mismatch is jarring. Pin it once in the bake tool.
5. **Direction facing.** Settled 2026-09-30: a placed billboard is heard in one
   direction only (section 4.1). Speaking every landmark both ways read
   "Meridian is ahead" just after leaving Meridian and the Wall Drug countdown
   after the exit.
6. **Scope creep.** The seed list plus the Overpass filter define the universe;
   an attraction outside both does not exist for the spider until a reviewed
   seed edit adds it.

## 10. Seed list — starting attractions (from the 2026-07-08 drafts)

Rows to prime `billboards_seed.json` (attraction → corridor; copy authored at
DRAFT). The six already in `CORRIDOR_BILLBOARDS` migrate here as placed signs;
the rest are this session's drafted corridors:

- **I-90 / South Dakota** — Wall Drug (`countdown`).
- **I-95 / the Carolinas** — South of the Border (`countdown`).
- **I-10 / Arizona** — The Thing; Cabazon-style roadside dinosaurs.
- **I-15 / Mojave** — alien jerky; The Mad Greek.
- **I-40 / the Southwest** — Historic Route 66 markers; a cavern trap.
- **I-80 / Wyoming** — Little America; the giant porch swing.
- **I-70 / Kansas–Colorado** — the world's largest prairie dog / Prairie Dog
  Town; the Rockies "the plains were lying" gag.
- **I-75 & I-24 / Tennessee–Georgia** — See Rock City; Ruby Falls.
- **I-44 / Missouri** — Meramec Caverns (`countdown`); the Uranus fudge factory.
- **I-35 / Oklahoma–Texas** — Winstar casino; the Czech Stop kolaches; the
  35E/35W split gag.
- **I-94 / Dakotas** — Salem Sue (world's largest Holstein); the Enchanted
  Highway sculptures.
- **I-5 / California** — Pea Soup Andersen's; Harris Ranch (the cattle smell).
- **I-65 / Kentucky** — the Corvette museum (Bowling Green); Dinosaur World
  (Cave City).
- **I-84 / Oregon–Idaho** — Multnomah Falls; Idaho "famous potatoes."
- **US-101 / redwood coast** — Trees of Mystery (Paul Bunyan); the drive-thru
  tree; Confusion Hill.
- **I-4 / Florida** — Dinosaur World (Plant City); airboat / gator stops.
- **Big Buck's** — its own approach pool already drafted in
  `BIG_BUCKS_BILLBOARDS`; place as a `countdown` at each Big Buck's landmark
  once those are sited in the world.

## 11. Where a board may stand: Canada

The United States pass applied every state's sign law; this is the same pass
for the provinces and territories, read 2026-11-22 from each one's statute
or official page. The table is data: `crates/ff-core/src/data/billboard_law.rs`
holds the same rows, and a test fails if the two disagree. Three answers:

- **open**: commercial boards are licensed on private land beside the
  highway. Every kind of billboard may stand: placed attraction signs, the
  anywhere pool, Big Jim.
- **attractions only**: the law licenses boards only for businesses that
  serve travelers (a museum, a lodge, a hot spring, a restaurant). Placed
  attraction signs stand; the pool and Big Jim stay off the road.
- **none**: no commercial boards. The road is silent, and that is correct.

Alaska stays **none**, as Maine, Vermont and Hawaii do. Everywhere, a board
never stands on the highway right of way, and a park the road runs through
has no private land to put one on: those stretches are `billboard_bans`
spans on the legs that cross them, as the United States scenic byways are.

| Jurisdiction | Boards | Where they may not stand | Source |
|---|---|---|---|
| British Columbia | open | The right of way; within 300 m of a provincial highway outside towns; provincial and national parks | BC Ministry of Transportation, Permits and Approvals Manual s 1.6.2; gov.bc.ca highway signs page |
| Alberta | none | Anywhere along a provincial highway; a permit covers 300 m from the highway and 800 m from an intersection | Highways Development and Protection Act ss 11 to 19; alberta.ca: "off-premise advertising billboards are not permitted along highways" |
| Saskatchewan | open | The right of way; 500 m from a junction of two provincial highways; permit within 400 m | Provincial Highway Sign Control Regulations; Roadside Management Manual 1220-00 and 1240-85 |
| Manitoba | open | The right of way; permit outside it | Transportation Infrastructure Act; gov.mb.ca private advertising signs page |
| Ontario | open | Permit within 400 m of a provincial highway; Crown land with a significant scenic view | Public Transportation and Highway Improvement Act; MTO Corridor Signing Policy; Crown land policy PL 4.06.02 |
| Quebec | open | Within 30 m of the right of way, 75 m of an autoroute, 180 m of a junction; within 300 m of a designated scenic, heritage or safety site; permit within 300 m | Roadside Advertising Act, CQLR c P-44 ss 6, 7, 13, 17 |
| New Brunswick | attractions only | Level I and II highways (the trucking routes) carry tourism businesses only; regulated area 500 m on those, 150 m elsewhere; 1 km from an interchange | Highway Act, Highway Advertisements Regulation 97-143 ss 2 to 5 |
| Nova Scotia | attractions only | Within 1,000 m of a highway, unless licensed for lodging, restaurants, attractions and similar traveler businesses within 50 km | Public Highways Act s 49, Advertising Signs Regulations ss 4, 5, 12 |
| Prince Edward Island | none | Any off-premises sign within view of a highway, outside municipalities | Highway Signage Act; Highway Signage Act Regulations s 3.1 |
| Newfoundland and Labrador | none | Permits only for government tourism panels in Corridor 2, never a billboard | Highway Sign Regulations NLR 85/99; Off-Site Promotional Highway Signage Policy and its FAQ |
| Yukon | attractions only | Within 10 m of the road, 250 m of another board or a junction; only visitor services, facilities or attractions; up to three boards per business outside communities | Highways Act; Visitor Information Sign Regulation, Schedule B (as printed on the 2026 sign permit form) |
| Northwest Territories | none | No permit is issued for a sign advertising a commercial business | Public Highways Act, Highway Signs Regulations s 6(2) |
| Nunavut | none | Same regulation as the Northwest Territories | Public Highways Act, Highway Signs Regulations s 6 |

Calls made reading these:

- **British Columbia** is the weakest reading: the manual sets a 300 m
  setback for rural boards rather than a permit scheme, and the province's
  sign page says private signs along highways are not permitted. Boards do
  stand back from the road on private and reserve land, so the answer is
  open, with every provincial park on the route banned by span.
- **New Brunswick** licenses commercial boards on ordinary highways, but the
  freight routes are its Level I and II highways, where only tourism
  businesses may sign. The whole province is read attractions only, so no
  pool line stands on Route 2.
- **Newfoundland and Labrador**'s off-site promotional signs are small
  government panels listing tourism businesses, not billboards, so the
  answer is none.
- Agricultural zones (Quebec's Act A-7.0001 covers urban highways only) and
  First Nations reserves did not change any answer.
