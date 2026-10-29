# ALCAN corridor scaffold plan (through-freight only)

The Alaska Highway crossing is the Alcan Port of Entry (CBP port 3104, OSM
node 4212502894); the Beaver Creek CBSA post (node 3531514528) lies beyond
the international line toward Whitehorse. Poker Creek is the separate
seasonal Top of the World Highway crossing.

Status: **Phase A tip KEEP + FIX 1–3 landed**; **inland to Dawson Creek Mile 0 landed**; **north filament to Whitehorse landed**; **AK terminus Tok + Fairbanks landed** on `feat/career-2.0` (… ↔ Whitehorse ↔ Alcan/Beaver Creek ↔ Tok ↔ Fairbanks). Phase A continuous Lower-48→Fairbanks ~2222 mi **KEEP**. **Phase B plan** (Anchorage join): see [`docs/alcan-phase-b-anchorage-plan.md`](./alcan-phase-b-anchorage-plan.md) — **B1 Tok Cutoff/Glenn → Anchorage** and **B2 Parks** (nenana/healy/wasilla; Cantwell collapsed; Parks milepost-paid checksum 358) world_data landed. No full Canada board.

Owner sequence (locked): **(1) this ALCAN corridor → (2) map Alaska → (3) rest of Canada → (4) Europe (#195).**
Do not jump ahead. Ruth verifies every step.

**Locked Phase A routing decisions:** the primary POE is **Blaine Pacific Highway**; **Sumas is an alternate only if the POE decision flips**. With Blaine, the BC entry is `surrey_bc_ca` or another **verified tractor-parking node on Hwy 15 / Pacific Highway**. `abbotsford_bc_ca` is allowed only when the POE is Sumas. The first legs must retain **real routed ALCAN highway mileage**, not compressed short-hop proxies.

Deferred mentions already in-tree:
- `docs/highway-spider-methodology.md` §1 Phase 3 — Canadian corridors deferred; points here.
- `data/spider/gap-fill/HANDOFF.md` — U.S. coverage first; Canada (Alcan program) unblocks after, “gated anyway on the 96 GB RAM arrival for the world extract.”
- `docs/gap-analyzer-brief.md` — do not rank Canada/Mexico off-graph dreams.
- `docs/map-enrichment-recipe.md` — “Taking this off the US grid” (jurisdiction caps / canonical units).
- `docs/roadmap-details.md` — International expansion bullet (research Canada/UK); sequence below supersedes any “full Canada next” reading.

---

## 0. Phased sequence (do not reorder)

| Phase | Scope | This tip? |
| --- | --- | --- |
| **A** | Minimal Canada ALCAN corridor that puts Alaska on the continuous truck graph (through-freight only; no Canada domestic cabotage) | **Yes — scaffold/plan only** |
| **B** | Map Alaska itself (beyond Tok/Fairbanks as mere endpoints — real AK highway graph / cities). Anchorage only when the AK highway graph reaches it continuously. Haines/Skagway = ferry later, never continuous drive. Plan: [`alcan-phase-b-anchorage-plan.md`](./alcan-phase-b-anchorage-plan.md) (B1 Tok Cutoff/Glenn first, B2 Parks second). | **B1 + B2 data landed**; Cantwell collapsed |
| **C** | Rest of Canada (full board) | After B |
| **D** | Europe (#195) | After C |

Phase A attaches to the existing Lower-48 graph at **Bellingham / I-5**. It does **not** open a Canada domestic career board.

---

## 1. Inventory (feat/career-2.0 tip at plan time)

Verified on branch against `data/world_data/` (tip used for this plan):

- **Graph:** US-only. `data/world_data/index.json` lists a single country (`US`). **624** cities in `data/world_data/us/cities.json`. **1283** legs across `data/world_data/us/legs/*.json` (no `AK.json`; research brief rounded ≈1290).
- **Northern I-5 tip:** `bellingham_wa_us` exists (also `seattle_wa_us`, `everett_wa_us`, etc.). WA legs already include Bellingham↔Everett on I-5.
- **Missing (all corridor targets):** no Blaine/Sumas nodes; no Dawson Creek / Fort Nelson / Watson Lake / Whitehorse / Tok / Fairbanks / Anchorage keys; no BC/YT/AK cities; no Canada country pack.
- **`data/world_data/geo.json`:** US state table includes `AK` as a name only — no cities/legs behind it.
- **Attach point:** corridor must hang off **existing Bellingham / I-5**, not a teleport from Seattle or a new Lower-48 stub that skips the graph.

---

## 2. Proposed city keys (Phase A)

Key shape matches current slug rule: `{name}_{region}_{country}` (e.g. `bellingham_wa_us`).

### Required stops (must be real cities on the continuous drive)

| Role | Proposed key | Notes |
| --- | --- | --- |
| US attach (exists) | `bellingham_wa_us` | Do not re-mint |
| BC entry service node | `surrey_bc_ca` (Blaine primary) | With Blaine Pacific Highway, use Surrey or another **verified tractor-parking node on Hwy 15 / Pacific Highway**; not a tourism lodge. `abbotsford_bc_ca` is allowed only if the POE flips to Sumas. **Ruth locks key with POE.** |
| Mile 0 ALCAN | `dawson_creek_bc_ca` | Required |
| ALCAN BC | `fort_nelson_bc_ca` | Required |
| ALCAN YT | `watson_lake_yt_ca` | Required |
| ALCAN YT | `whitehorse_yt_ca` | Required |
| AK entry | `tok_ak_us` | Required |
| Main AK node (Phase A end) | `fairbanks_ak_us` | Required; Phase A terminus |

### Locked primary POE

- **Primary:** Pacific Highway **Blaine**.
- **Alternate only:** **Sumas**; use it only if the POE decision explicitly flips. Only then is `abbotsford_bc_ca` an acceptable BC entry service node.
- Model the crossing as **border data on the cross-border leg(s)**, not as a fake “teleport city.” Optional US-side service pin only if needed for fuel/parking honesty; do not invent a career city just to name the booth.

### Allowed pass-through fuel/parking towns (no full careers)

Honesty on Blaine→Dawson Creek may add checkpoints / service pins without dispatch careers, e.g.:

- `prince_george_bc_ca`
- `fort_st_john_bc_ca`

(and other on-route fuel/parking towns Ruth accepts). Pass-through ≠ cabotage board.

### Explicitly not Phase A cities

- **Anchorage** — Phase B only, if/when continuous AK highway graph reaches it.
- Haines / Skagway — ferry later; never a continuous-drive substitute for ALCAN.
- Interior BC/Prairie/Ontario cities, Mexico, Europe — out of scope.

---

## 3. Proposed ordered leg list (corridor)

Direction shown northbound; expect matching southbound edges when the graph lands.

1. `bellingham_wa_us` → **(Blaine Pacific Hwy POE)** → `surrey_bc_ca` (or a verified Hwy 15 / Pacific Highway tractor-parking node; `abbotsford_bc_ca` only if POE flips to Sumas)
2. BC entry → …optional pass-throughs (Prince George, Fort St. John, …)… → `dawson_creek_bc_ca` using the real routed highway mileage
3. `dawson_creek_bc_ca` → `fort_nelson_bc_ca` (Alaska Hwy)
4. `fort_nelson_bc_ca` → `watson_lake_yt_ca`
5. `watson_lake_yt_ca` → `whitehorse_yt_ca`
6. `whitehorse_yt_ca` → **(Alcan / Beaver Creek border)** → `tok_ak_us`
7. `tok_ak_us` → `fairbanks_ak_us`

No Seattle→Fairbanks skip. No short-hop “ALCAN” that omits Mile 0 / Whitehorse / the Alcan–Beaver Creek crossing, and no compressed `dawson_creek_bc_ca` → `tok_ak_us` proxy. The first legs must preserve verified, routed ALCAN road mileage through each corridor segment; do not substitute straight-line, guessed, or short-hop distances.

---

## 4. Border-crossing data shape (proposal)

Goal: model Blaine/Sumas and Alcan/Beaver Creek **without** Canada domestic cabotage.

Suggested leg (or leg-segment) metadata — names indicative, not schema freeze:

```json
"border_crossing": {
  "id": "blaine_pacific_highway",
  "from_country": "US",
  "to_country": "CA",
  "ports": ["blaine_pacific_highway"],
  "mode": "through_freight",
  "cabotage": "forbidden"
}
```

Rules of thumb for the cut:

- Crossing is **on the continuous path** (delay / inspection beat later); it is not a menu teleport.
- **Through-freight only** in Phase A: loads that enter Canada must be international through movements (Lower 48 ↔ Alaska via the corridor), not CA domestic pickup/delivery for a US carrier fantasy board.
- Second crossing on the same corridor: `alcan_beaver_creek` with `from_country: CA`, `to_country: US`.
- Do not mint full career economies on Canadian pass-through towns in Phase A.

The data schema is present in `world_source`; a playable delay or inspection flow remains deferred.

---

## 5. Multi-country profile blockers (exists vs missing)

| Area | Exists today | Missing for Phase A honesty |
| --- | --- | --- |
| Country packs | `index.json` → US only | CA pack path; AK cities under US (or explicit AK handling); geo names for BC/YT |
| Units | Player `imperial_units` toggle; sim stores miles | Jurisdiction truck caps beyond US state mph table; `docs/map-enrichment-recipe.md` already says non-US needs jurisdiction keys + canonical km/h defaults |
| Currency | USD-centric money / speech | CAD purse (and FX or dual purse) is deferred; it is not required to *drive* the corridor, but pay/fuel speech will lie if ignored — flag for follow-up |
| HOS | FMCSA-style clock | The driver clock now selects Alaska and Canadian south-/north-of-60 rules by jurisdiction. Canadian daily limits/off-duty, cycle 2, split sleeper, adverse-driving extensions and deferral remain unmodeled; route planning still uses one rule set, Canadian rest menus remain 10 hours, and Alaska's 34-hour restart is assumed. |
| Borders | State-line cues | International border mechanic still **deferred**; need at least data hooks (above) before pretending clearance gameplay |
| Map extracts | US Geofabrik / self-hosted ORS-Overpass for US | **Need BC / YT / AK extracts** — US extract alone cannot bake honest ALCAN geometry |
| HANDOFF / extract gate | `data/spider/gap-fill/HANDOFF.md` cites **96 GB RAM world extract** as a Canada unblock | **Soft for Phase A** if public Overpass / routing / Geofabrik *regional* downloads already cover the ALCAN filament honestly. Escalate to a full PBF / Valhalla-class bake only when APIs fail honesty — see §5.1 |


### 5.1 Extract / bake ops policy (owner)

For ALCAN / CA / AK world-data work **after Ruth cuts this plan**:

1. **Prefer public APIs first.** Overpass, public routing, and Geofabrik *regional* downloads as needed. Slow is fine; stay honest. Corridor-scale Overpass / API work can run on Chelsea’s Linux box.
2. **Full PBF bake or Valhalla-class jobs** (only when APIs cannot keep the filament honest): run on **Josh (Windows)**, with large OSM on **E: (SanDisk SSD)** — not C: or D:. **Ask Chelsea before starting anything that needs Josh left awake overnight** (that machine often sleeps).
3. **Do not assume Chelsea’s Linux box can hold Valhalla tiles.** It has 16 GB RAM and no swap; Valhalla-class tile builds OOM there.

**96 GB HANDOFF gate:** soft if APIs cover the ALCAN filament. Only escalate to Josh for bulk extract when APIs fail honesty. Still: **plan tip → Ruth cut → then data.** No `world_data` bulk in this tip.

---

## 6. Out of scope / dishonest traps (Ruth will reject)

- Teleport / skip-Canada Lower 48 → Alaska.
- Short-hop “ALCAN” missing Dawson Creek, Whitehorse, or Alcan/Beaver Creek.
- Full Canada dispatch board in Phase A.
- US cabotage fantasy inside Canada (domestic CA loads for the US through-freight slice).
- Tourism lodges as tractor stops without truck parking.
- Alaska cities with no continuous highway path (including Anchorage before Phase B graph reaches it).
- Ferry-as-drive via Haines/Skagway.
- Europe (#195) or rest-of-Canada bulk before Alaska map phase.
- Silent use of car-speed limits / mph-only tables on CA/YT statutory truck caps.
- CAD purse / FX work, deep border gameplay, and other full Canada or Alaska economy work are deferred from this plan tip.

---

## 7. GO gates (before first city/leg PR)

1. Ruth cuts this plan (Blaine Pacific Highway primary; Sumas only if the POE flips; Surrey/Hwy 15 Pacific Highway BC entry key; pass-through list).
2. Confirm extract path: BC/YT/AK coverage via public APIs / regional Geofabrik first (see §5.1); treat 96 GB HANDOFF gate as soft unless APIs fail honesty.
3. Name the temporary HOS honesty debt (US through-freight clock only) and keep CAD purse, deep border gameplay, and full-country expansion deferred.
4. Only then: first PR adds country/geo hooks + corridor cities/legs — **still no rest-of-Canada bulk.**

After Phase A lands continuously to Fairbanks: **Phase B = Alaska expansion** (named next), then Phase C rest of Canada, then Phase D Europe.


---

## 8. First tip landed (Bellingham → Blaine → Surrey)

Landed on `feat/career-2.0` after Chelsea GO (plan locks kept: Blaine primary, Surrey BC entry, no Abbotsford).

| Key / leg | Miles | Notes |
| --- | --- | --- |
| `bellingham_wa_us` | (exists) | US attach; unchanged |
| `blaine_wa_us` | — | New US stand-in service pin near Pacific Highway POE |
| `surrey_bc_ca` | — | New CA stand-in; pin at City of Surrey truck parking **17768 96 Avenue** (Hwy 15 / Pacific Highway corridor) |
| `bellingham_wa_us` ↔ `blaine_wa_us` | **22** | I-5 both directions; public Valhalla truck costing 2026-09-22 |
| `blaine_wa_us` ↔ `surrey_bc_ca` | **16** | Hwy 15 / Pacific Highway via Blaine POE both directions; `border_crossing` both ways (`through_freight`, cabotage forbidden) |

### Mileage / geometry source

- I-5 tip: public Valhalla truck costing (`valhalla1.openstreetmap.de`) over OSM, development-time 2026-09-22; paid miles 22.
- Hwy 15 tip: public Valhalla auto-profile densify (truck costing rejected as dishonest at this POE); paid miles 16.
- Elevation: Open-Meteo elevation API.
- Prefer **Hwy 15 / Pacific Highway** label (not Hwy 99 / Peace Arch) for this POE.

### Ruth KEEP + FIX (landed)

| FIX | Landed |
| --- | --- |
| **1 southbound reverse edges** | Authored `surrey_bc_ca`→`blaine_wa_us`→`bellingham_wa_us` with mirrored miles/geometry. CA→US leg carries the same `border_crossing` id `blaine_pacific_highway`, mode `through_freight`, cabotage `forbidden` (countries flipped). Filed under from-country (`ca/legs/BC.json`). Directed uniqueness allows both orientations; runtime adjacency still walks either way via `Leg::other`. |
| **2 truck-class refine** | I-5 Bellingham↔Blaine: public Valhalla **truck** costing (`valhalla1.openstreetmap.de`, loaded-semi options) + Washington RCW 46.61.410 **60 mph** `hgv:true` speed context. Blaine↔Surrey: public Valhalla **auto** densify on Hwy 15 / Pacific Highway (paid miles kept at 16); FOSSGIS Valhalla **truck** unconstrained returned ~40.5 mi and is **not** adopted. |
| **3 locations** | `surrey_bc_ca`: City of Surrey truck parking at **17768 96 Avenue** (~41 spaces). `blaine_wa_us`: **TA Express Blaine** (1300 Boblett St / I-5 Exit 275, ~34 truck spaces). Stand-in expand keeps curated yards without unlocking a four-template skyline. |

### Named honesty debts (remaining)

1. **HOS (driver-clock debt resolved):** Phase A used the US through-freight clock. The driver clock now selects Alaska and Canadian south-/north-of-60 rules; Canadian daily requirements, cycle 2, split sleeper, adverse-driving extensions, deferral and route-wide planning remain open, Canadian rest menus still offer 10 hours, and Alaska's 34-hour restart applicability is unverified.
2. **Units / speed:** CA statutory truck caps still unresearched on Hwy 15; Blaine↔Surrey speed_limits stay coarse `hgv:false` placeholders.
3. **Currency:** no CAD purse / FX.
4. **Border gameplay:** data stub only (`border_crossing` both directions); no inspection beat / clearance sim.
5. **HGV geometry (cross-border):** public FOSSGIS Valhalla truck costing is dishonest at this POE (~40.5 mi vs ~14–16 on Pacific Highway). Remaining debt: regional WA/BC truck bake / ORS key / Josh overnight PBF—**ask Chelsea before overnight**. I-5 tip truck refine is landed.
6. **Stand-in markets:** Blaine and Surrey stay stand-in markets (curated truck lots only; no CA career board / cabotage).
7. **Job board:** destinations are same-country as origin (US board will not offer Surrey). Through-freight CA pass-through is driveable; CA domestic dispatch is not Phase A.


### Inland filament landed (Surrey → Dawson Creek Mile 0)

Ruth GO after KEEP on FIX tip `43a85ac0`. Daytime public Valhalla truck costing only (no Josh overnight PBF/Valhalla).

| Key / leg | Miles | Notes |
| --- | --- | --- |
| `prince_george_bc_ca` | — | Pass-through stand-in; Husky/Esso Travel Centre pin (1148 Pacific St) |
| `dawson_creek_bc_ca` | — | ALCAN Mile 0; city pin at Mile 0 milepost; Husky 1700 Alaska Hwy lot |
| `fort_st_john_bc_ca` | — | Alaska Hwy pass-through north of Mile 0; Smith Fuel Services pin |
| `surrey_bc_ca` ↔ `prince_george_bc_ca` | **465** | Hwy 1 / Hwy 97 (Fraser Canyon + Cariboo); both directions; public Valhalla truck |
| `prince_george_bc_ca` ↔ `dawson_creek_bc_ca` | **254** | Hwy 97 / John Hart Highway; both directions; public Valhalla truck |
| `dawson_creek_bc_ca` ↔ `fort_st_john_bc_ca` | **45** | Alaska Highway; both directions; public Valhalla truck |

No `border_crossing` on inland legs (border metadata stays at Blaine POE only). Through-freight only; cities remain stand-in markets (no CA cabotage board). Soft FIX company_yard→parking/travel_center types deferred.

### Mileage / geometry source (inland)

- Public Valhalla truck costing (`valhalla1.openstreetmap.de`, loaded-semi options) 2026-09-22; paid miles match router (±5 mi band rounded).
- Elevation: Open-Meteo elevation API along densified shape (~30 mi samples on long legs).
- City lat integrity ceiling raised to `60.0` for northern BC (YT / AK still need a later raise).

### North filament landed (Fort St. John → Whitehorse)

Ruth GO after KEEP on inland tip `4b375017`. Daytime public Valhalla truck costing only (no Josh overnight PBF/Valhalla).

| Key / leg | Miles | Notes |
| --- | --- | --- |
| `fort_nelson_bc_ca` | — | Alaska Hwy pass-through; Esso / Ft Nelson Bulk Sales pin (5303 48th Avenue) |
| `watson_lake_yt_ca` | — | Yukon entry on ALCAN; Petro-Canada 137 Frank Trail pin (~5 truck spaces) |
| `whitehorse_yt_ca` | — | Phase A Yukon terminus; downtown pin + McCrae Petro-Canada lot (91007 Alaska Hwy, ~10 spaces) |
| `fort_st_john_bc_ca` ↔ `fort_nelson_bc_ca` | **239** | Alaska Highway; both directions; public Valhalla truck |
| `fort_nelson_bc_ca` ↔ `watson_lake_yt_ca` | **319** | Alaska Highway (BC→YT); both directions; public Valhalla truck; coarse `state_miles` + `state_crossings` near Contact Creek / Lower Post |
| `watson_lake_yt_ca` ↔ `whitehorse_yt_ca` | **273** | Alaska Highway; both directions; public Valhalla truck |

This north-filament table stops at Whitehorse; the Tok/Fairbanks terminus and Alcan / Beaver Creek border are recorded below. Through-freight only; cities remain stand-in markets (no CA cabotage board). Soft FIX company_yard→parking/travel_center types deferred.

### Mileage / geometry source (north)

- Public Valhalla truck costing (`valhalla1.openstreetmap.de`, loaded-semi options) 2026-09-22; paid miles match router (±5 mi band rounded).
- Maxspeed rebakes on archive-less Alcan legs use public Valhalla truck geometry through the stored route points.
- Elevation: Open-Meteo elevation API along densified shape (~30 mi samples).
- City lat integrity ceiling raised to `66.0` for Fairbanks (~64.8°N); lon floor lowered to `-150.0` for Interior AK (~-147.7). Anchorage still Phase B.

### Phase A AK terminus (Tok / Fairbanks) — landed

| Key / leg | Miles | Notes |
| --- | --- | --- |
| `tok_ak_us` | — | Alaska Highway entry; Young's Chevron stand-in fuel/parking |
| `fairbanks_ak_us` | — | Phase A AK terminus; Sourdough Fuel (1688 Airport Way) stand-in |
| `whitehorse_yt_ca` ↔ `tok_ak_us` | **387** | Alaska Highway both directions; `border_crossing` id `alcan_beaver_creek`, mode `through_freight`, cabotage `forbidden` (both ways); Alcan checkpoint at mi 297.5 toward Tok, Beaver Creek checkpoint at mi 107.8 toward Whitehorse; border line at mi 89.5 from Tok |
| `tok_ak_us` ↔ `fairbanks_ak_us` | **202** | Alaska Highway / Richardson Highway (AK-2); both directions; public Valhalla truck |

No Anchorage (Phase B). Through-freight only; cities remain stand-in markets (no CA cabotage board). Soft FIX company_yard→parking/travel_center types deferred.

The US through-freight HOS debt is resolved for the driver clock by the
jurisdiction-aware Alaska and Canadian rules. Remaining HOS debts are listed
above: unmodeled Canadian provisions, route-wide planning, Canadian 10-hour
rest menus and the unverified Alaska 34-hour restart. Other Canadian truck caps
beyond BC's 105 km/h limiter and Blaine HGV refine also remain open.

### Blockers for next slice (Phase B Alaska / rest of Canada)

- Phase B plan: [`docs/alcan-phase-b-anchorage-plan.md`](./alcan-phase-b-anchorage-plan.md). **B1 + B2 landed** (Cantwell collapsed; no `cantwell_ak_us`).
- Prefer public Overpass / regional Geofabrik before any overnight PBF/Valhalla bake.
- Phase C rest of Canada still deferred.
