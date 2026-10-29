# ALCAN Phase B plan — Anchorage on the continuous AK truck graph

Status: **B1 + B2 + B3 world_data landed**. B1 connects Tok through Glennallen and Palmer to Anchorage; B2 connects Fairbanks through Parks to Anchorage; B3 adds the separate peninsula, Valdez, and Dalton corridors below. Parks B2 remains **milepost-paid** from Anchorage (Wasilla~42 / Healy~249 / Nenana~305 / Fairbanks~358; checksum 358). Plan FIX tip `37915685` kept. Phase A Lower-48 → Fairbanks **KEEP**.

Parent scaffold: [`docs/alcan-corridor-scaffold-plan.md`](./alcan-corridor-scaffold-plan.md) (sequence A corridor → **B Alaska map** → C rest of Canada → D Europe).

Original B1/B2 authoring branch: `feat/career-2.0`. Daytime public APIs first (Overpass, public Valhalla/OSRM, Geofabrik regional). **No overnight PBF / Valhalla bake** for that plan tip.

---

## 0. Phase A attach (KEEP — do not re-mint)

| Key / leg | Miles | Notes |
| --- | --- | --- |
| `tok_ak_us` | — | Tok; Young's Chevron stand-in (Alaska Hwy) |
| `fairbanks_ak_us` | — | Phase A terminus; Sourdough Fuel 1688 Airport Way stand-in |
| `whitehorse_yt_ca` ↔ `tok_ak_us` | **387** | Border `alcan_beaver_creek`, through_freight, cabotage forbidden (both ways); Alcan checkpoint at mi 297.5 toward Tok, Beaver Creek at mi 107.8 toward Whitehorse; international line at mi 89.5 from Tok |
| `tok_ak_us` ↔ `delta_junction_ak_us` | **108** | Alaska Highway, milepost-paid |
| `delta_junction_ak_us` ↔ `fairbanks_ak_us` | **96** | Richardson Highway, milepost-paid |
| `tok_ak_us` ↔ `fairbanks_ak_us` direct | Retired | Split at Delta Junction; 108 + 96 = 204 paid mi versus the former 202-mi composite. Keep the 2-mi residual; do not redistribute it. |

The B1/B2 routes hang Anchorage off this AK tip. B3 adds separate Alaska corridors; none reopen Canada cabotage, ferry shortcuts, or Phase C/D scope.

---

## 1. Route choice (with rationale) — KEEP

### Options

| Corridor | Highways | From | To | Published / milepost | Valhalla truck (2026-09-24) |
| --- | --- | --- | --- | --- | --- |
| **Tok Cutoff + Glenn** | AK-1 Tok Cutoff → short Richardson → AK-1 Glenn | `tok_ak_us` | Anchorage | **~328 mi** Tok→ANC (Bell's / MILEPOST) | Direct ~318 — **do not ship**; see §3 B1 reconcile |
| **Parks** | AK-3 Parks (shared Glenn south of Glenn–Parks interchange) | `fairbanks_ak_us` | Anchorage | ~358–362 mi Fairbanks↔Anchorage | Direct ~361 |

Passes / grades of note:

- **Glenn / Tok Cutoff:** Mentasta Summit (~2,434 ft) on Tok Cutoff; **Eureka Summit (~3,322 ft, Glenn MP ~129.3)** — highest point on Glenn; winter ice/glaze reported; paved all-weather, but check Alaska 511.
- **Parks:** **Broad Pass (~2,409 ft)** south of Cantwell; milder summit than Eureka; Parks is the Denali / Interior tourist + rail-parallel freight spine between Fairbanks and Mat-Su.

### Recommendation — **both**, land **Tok Cutoff + Glenn first** (KEEP)

**Primary (lands first):** Tok Cutoff (AK-1) + Glenn Highway (AK-1) from `tok_ak_us` → `glennallen_ak_us` → `palmer_ak_us` → `anchorage_ak_us`.

**Secondary (lands second, after primary CI green):** Parks Highway (AK-3) from `fairbanks_ak_us` → pass-through towns → `wasilla_ak_us` → `anchorage_ak_us` (shared Glenn south of the Glenn–Parks interchange).

**Rationale:** Anchorage-bound overland freight that already entered Alaska on the ALCAN at Tok does **not** detour to Fairbanks then south on Parks; published road logs treat the **Glenn Highway / Tok Cutoff as the principal paved Tok→Anchorage connection (~328 mi)**. Fairbanks remains the ALCAN / Richardson terminus for Interior and North Slope staging; Fairbanks↔Anchorage freight uses Parks (and/or rail from Port of Alaska). Most Anchorage consumer freight arrives by **sea via Port of Alaska**, not by ALCAN tractor — Phase B still needs the continuous road filament so through-freight and AK domestic legs are honest, but Anchorage must not be sold as “the ALCAN destination.” Parks is required for a real AK highway graph (Fairbanks–Mat-Su–Anchorage) and should follow once the Tok→Anchorage filament is green. Eureka Summit is steeper/higher than Broad Pass and deserves HGV winter notes; it does not justify skipping the primary freight path.

Order landed: **B1 Tok Cutoff/Glenn → B2 Parks → B3 Alaska regional roads**.

---

## 2. Proposed city keys

Key shape: `{name}_{region}_{country}` (e.g. `tok_ak_us`).

### B1 — Tok Cutoff + Glenn (required)

| Role | Proposed key | Approx pin | Role detail |
| --- | --- | --- | --- |
| Exists (KEEP) | `tok_ak_us` | 63.3367, −142.9856 | Phase A entry; do not re-mint |
| Full node | `glennallen_ak_us` | ~62.11, −145.55 | Glenn corridor at Glennallen (community ~MP 187; Hub of Alaska fuel at Glenn×Richardson ~MP 189). Copper River region service hub |
| **Mat-Su market (full)** | `palmer_ak_us` | ~61.60, −149.11 | Glenn MP ~42; **the one Mat-Su market in B1** |
| Phase B destination / full node | `anchorage_ak_us` | ~61.22, −149.90 | Port-fed market; fuel pin ≠ delivery end (see §4) |

**CUT:** do **not** mint `gakona_ak_us` (or any Gakona city key). Tok Cutoff meets Richardson at Gakona Junction ~14 mi north of Glennallen; that junction is highway geometry only, not a career city. Glennallen covers the Copper River service hub.

### B2 — Parks (after B1)

| Role | Proposed key | Approx pin | Role detail |
| --- | --- | --- | --- |
| Exists (KEEP) | `fairbanks_ak_us` | 64.8378, −147.7167 | Phase A terminus |
| Pass-through (no pin unless verified) | `nenana_ak_us` | ~64.56, −149.09 | Town on graph; fuel/parking pin only if §4 verifies |
| **Thin pass-through — not a market** | `healy_ak_us` | ~63.87, −148.97 | Parks / Denali gateway only; no career skyline / market board |
| Conditionally omitted | `cantwell_ak_us` | ~63.39, −148.95 | Mint **only** if diesel **and** real tractor parking verify (§4). Otherwise **no city** — collapse Healy→Wasilla |
| Pass-through (Mat-Su) | `wasilla_ak_us` | ~61.58, −149.44 | Parks corridor near Glenn–Parks interchange; **not** a second Mat-Su market |
| Exists after B1 | `anchorage_ak_us` | — | Shared destination |

### Mat-Su market rule (one market)

- **B1:** `palmer_ak_us` is the **full Mat-Su market**.
- **B2:** when `wasilla_ak_us` lands, **Wasilla stays a pass-through**; Palmer remains the Mat-Su market. (Pick locked here: demote Wasilla, keep Palmer.)
- `palmer_ak_us` ↔ `wasilla_ak_us` connector (~11 mi published / ~13 Valhalla truck) is **KEEP** — author as a real leg when both cities exist (see §3).

### Outside B1/B2; later scope updated by B3

- `gakona_ak_us` (CUT — never)
- Haines / Skagway / any ferry POE as drive nodes
- Whittier (tunnel) remains separate; B3 adds Kenai, Soldotna, Homer, and Seward
- B3 adds Coldfoot and Deadhorse on the Dalton Highway
- B3 adds Valdez as a separate Richardson filament, not as an Anchorage shortcut
- Tourism lodges as cities (Eureka Lodge, Sheep Mountain Lodge, Mentasta Lodge, etc.)

---

## 3. Ordered leg list

Direction shown Anchorage-bound; expect matching reverse edges when data lands. Round to whole miles at data tip.

### B1 — Tok Cutoff + Glenn (lands first) — mile reconcile

**Pin placement**

| City | Pin target | Milepost / published anchor |
| --- | --- | --- |
| `glennallen_ak_us` | Hub of Alaska (diesel + truck parking) at Glenn × Richardson | Glenn Hwy **MP ~189** junction; Glennallen community listed at **MP 187** (Bell's Glenn Hwy log) |
| `palmer_ak_us` | Town / Glenn access near Arctic Ave / Old Glenn | Glenn Hwy **MP 42** |
| `anchorage_ak_us` | Fuel at Essential One; delivery end separate (§4) | Glenn Hwy **MP 0** |

**Tok→Anchorage published path (Bell's / MILEPOST):** Tok Cutoff **125 mi** + Richardson **~14 mi** to Glenn junction + Glenn **189 mi** = **~328 mi**.

| # | Leg | Milepost / published | Valhalla truck (2026-09-24) | **Data tip pays** |
| --- | --- | --- | --- | --- |
| 1 | `tok_ak_us` → `glennallen_ak_us` | **~139** (125 Tok Cutoff + ~14 Richardson to Glenn jct) | ~139 (tok→Hub / town) | **~139** — Valhalla matches published |
| 2 | `glennallen_ak_us` → `palmer_ak_us` | **145** (Glenn **MP 187 → MP 42**) | ~138 (Hub/town → Palmer MP42 area) | **145** — milepost; Valhalla undercounts ~7 mi |
| 3 | `palmer_ak_us` → `anchorage_ak_us` | **42** (Glenn **MP 42 → MP 0**) | ~43 | **42** — milepost (Valhalla within ±5) |
| — | B1 checksum | MILEPOST Tok→ANC **328** | Valhalla direct **~318** | **~326** (139+145+42) — **do not ship 318** |

**Why pay milepost on leg 2:** Glenn Hwy mileposts from Anchorage are the corridor’s published truth (Bell's road log: MP 187 Glennallen, MP 42 Palmer → **145 mi**). Public Valhalla truck on OSM returns ~138 from Hub/Glennallen pins to Palmer-area pins — consistently short across pin variants, not fixed by moving the pin from town (MP 187) to Hub (MP 189). Data tip still **densifies geometry with Valhalla truck** along Glenn through Eureka Summit, but **paid miles for glennallen→palmer = 145**. Do not adopt the ~318 direct Valhalla checksum; published Tok→ANC is **328**, plan segment sum **~326** (2 mi residual = Hub at MP 189 vs community MP 187 used in the 145 arithmetic).

No Tok→Anchorage skip that omits Glennallen. No Fairbanks detour on B1.

### B2 — Parks (lands second)

Default list assumes Cantwell parking **does not** verify (current status: unverified — see §4). Healy is a thin pass-through, not a market.

| # | Leg | Est. mi | Highway | Source / note |
| --- | --- | --- | --- | --- |
| 1 | `fairbanks_ak_us` → `nenana_ak_us` | **53** | Parks AK-3 | Parks milepost from Anchorage: Fairbanks~MP358 → Nenana~MP305. Fairbanks pin on Airport Way at about MP 358, 4 mi short of the Parks terminus at MP 362; paid miles match the pin. Shape from prior Valhalla densify. |
| 2 | `nenana_ak_us` → `healy_ak_us` | **56** | Parks AK-3 | Parks milepost Nenana~MP305 → Healy~MP249 |
| 3 | `healy_ak_us` → `wasilla_ak_us` | **207** | Parks AK-3 via Broad Pass | Parks milepost Healy~MP249 → Wasilla~MP42 (**Cantwell collapsed**) |
| 4 | `wasilla_ak_us` → `anchorage_ak_us` | **42** | Parks → Glenn–Parks interchange → Glenn AK-1 | Parks/Glenn milepost Wasilla~MP42 → Anchorage MP0 |
| 5 | `palmer_ak_us` ↔ `wasilla_ak_us` | **13** (debt vs ~11) | Mat-Su connector | Paid 13 from Valhalla densify; planned/published ~11 kept as named debt — shape unchanged |
| — | Fairbanks→Anchorage Parks checksum | — | 53+56+207+42 = **358** | Matches Parks Fairbanks~MP358 → Anchorage MP0 |

**Cantwell conditional (only if diesel AND tractor parking verify at data tip):**

| Replace leg 3 with | Est. mi | Note |
| --- | --- | --- |
| `healy_ak_us` → `cantwell_ak_us` | **~43** | Valhalla truck |
| `cantwell_ak_us` → `wasilla_ak_us` | **~169** | Valhalla truck (~43+169 ≈ 212 vs collapsed 207) |

If Cantwell stays unverified: **no `cantwell_ak_us` city**, no Cantwell pin, single Healy→Wasilla leg ~207.

Nenana stays on the graph as a pass-through town even without a fuel pin (no invented capacity).

### B3 — Alaska regional roads (landed)

**New nodes:** `delta_junction_ak_us`, `valdez_ak_us`, `seward_ak_us`,
`soldotna_ak_us`, `kenai_ak_us`, `homer_ak_us`, `coldfoot_ak_us`, and
`deadhorse_ak_us`. Every corridor below has both directed legs; miles are
paid miles, followed by the router-reported loaded-semi miles in each direction.

| Corridor | Highway | Paid mi | Router mi, forward / reverse |
| --- | --- | ---: | ---: |
| Tok ↔ Delta Junction | Alaska Highway | 108 | 107.5063 / 107.3264 |
| Delta Junction ↔ Fairbanks | Richardson Highway | 96 | 95.4419 / 95.0685 |
| Glennallen ↔ Delta Junction | Richardson Highway | 151 | 150.8913 / 150.7879 |
| Glennallen ↔ Valdez | Richardson Highway | 115 | 117.7467 / 117.7522 |
| Anchorage ↔ Seward | Seward Highway | 127 | 127.0722 / 127.0468 |
| Anchorage ↔ Soldotna | Seward / Sterling highways | 147 | 148.0890 / 148.0699 |
| Seward ↔ Soldotna | Seward / Sterling highways via Tern Lake | 94 | 93.7667 / 93.7567 |
| Soldotna ↔ Kenai | Kenai Spur Highway | 11 | 11.5668 / 11.5544 |
| Soldotna ↔ Homer | Sterling Highway | 75 | 74.7109 / 74.7071 |
| Fairbanks ↔ Coldfoot | Elliott / Dalton highways | 259 | 253.6889 / 253.6685 |
| Coldfoot ↔ Deadhorse | Dalton Highway | 239 | 241.2141 / 241.2142 |

Paid values follow the published Alaska highway mileposts; the Dalton
mileposts use the BLM Alaska 2024 Dalton Highway Visitor Guide (Livengood /
Dalton MP 0 is 84 miles north of Fairbanks, Coldfoot MP 175, Deadhorse MP
414). The Homer city-pin leg is 75 paid miles: The MILEPOST and Bell's Alaska
Highway logs place Homer near MP 173 in town, not at MP 179.5 at the end of
the Spit; Soldotna is near MP 94–96, and the third-party driving distance is
about 75 miles. Router mileage is not used to overwrite paid miles. The Tok split
retains 204 paid miles against the retired 202-mile composite; the two-mile
difference is documented, not redistributed.
Chatanika can offer a Fairbanks–Deadhorse load within its 600-mile run band,
but Deadhorse remains outside the Alaska carriers' home-start hiring radii.

Geometry uses public Valhalla truck costing with loaded-semi options, and
route points and Open-Meteo elevation samples follow that shape. Grade
segments and overall terrain are derived from sampled elevations. Posted OSM
speed limits are read where tagged, preferring `maxspeed:hgv`; unposted Alaska
coverage assumes 55 mph under 13 AAC 02.275, and that fallback is confined to
the Alaska portion of cross-border legs. Bare Canadian OSM speeds are
interpreted as km/h; the six Yukon and northern BC Alaska Highway legs now use
posted profiles where tagged. This is not a blanket Dalton 50-mph limit from
the BLM guide. Atigun Pass samples peak at 4,904.9 feet
(Coldfoot-route mile 70.8 northbound and 168.2 southbound); a nearby audited
grade reaches 13.98%. State miles are derived from paid miles.

Supported freight and fuel pins use operator or official sources:

- Valdez terminal: [Ryan J. Sontag Container Terminal](https://www.valdezak.gov/296/Ryan-J-Sontag-Valdez-Container-Terminal).
- Kenai terminal: [Carlile Kenai terminal](https://www.carlile.biz/contact-and-terminals/kenai/).
- Seward dock: [Alaska Railroad freight dock fact sheet](https://www.alaskarailroad.com/sites/default/files/FCTSHT_2024_Seward_Freight_Dock_Expansion_FactSheet.pdf).
- Homer dock: [Port of Homer Deep Water Freight Dock](https://www.cityofhomer-ak.gov/port/deep-water-dock-scheduling).
- Delta fuel: [Three Bears Delta Junction](https://threebearsalaska.com/stores/three-bears-delta-junction-2/).
- Seward, Soldotna, and Homer fuel: [Shoreside Petroleum stations](https://www.shoresidepetroleum.com/fuel-stations/).
- Coldfoot fuel: [Coldfoot Camp amenities](https://www.coldfootcamp.com/amenities).
- Fairbanks–Coldfoot fuel stop: [Yukon River Camp](https://yukonrivercamp.com/).

Public fuel pins are `travel_center` facilities with no cargo roles; no
unverified parking capacity is claimed. Yukon River Camp confirms gas and
diesel, but truck-parking capacity is unverified. An exact Deadhorse
Carlile terminal pin and Deadhorse or Valdez public-fuel pins were not
supported by the available sources; no independently verified Soldotna
receive-only retail site was added. The Dalton's gravel surface is described
by BLM but has no separate road-surface gameplay model. Delta Junction,
Coldfoot, and Soldotna remain pass-through stand-in towns with fuel-only
travel centers and no freight offers until a freight endpoint is verified.
Actual `JobBoard::offers()` checks expect no freight offers there.

---

## 4. Truck stops / parking

Prefer `truck_parking` / `travel_center` types for public lots — **not** `company_yard`. Tourism lodges stay rejected. A `truck_parking` pin requires verified tractor parking; an operator-confirmed public diesel location may be a `travel_center` without implying parking. Do not invent capacity. Unverified ⇒ town may exist as pass-through with **no** fuel/parking location pin.

### Status table

| Area | Candidate | Diesel | Tractor parking | Status | Plan action |
| --- | --- | --- | --- | --- | --- |
| Tok | Young's Chevron (Alaska Hwy & E 4th) | Yes | Yes (~25; AllStays / Phase A) | **Verified** (Phase A + AllStays) | **KEEP** existing pin |
| Glennallen | **Hub of Alaska** (Glenn × Richardson ~MP 189) | Yes | Yes (~20–25; truckstopsandservices / AllStays) | **Verified** | **Primary Glennallen pin** |
| Glennallen | Glennallen Fuel & Service (~Glenn MP 187) | Yes | Yes (~10; truckstopsandservices / AllStays) | **Verified** | Alternate / second lot OK |
| Tok Cutoff mid | Mentasta Lodge | — | Lodge / tourism | **REJECT** | No pin |
| Glenn mid | Eureka Lodge / Sheep Mountain / Grand View | — | Tourism / RV | **REJECT** | No pin |
| Palmer | Chevron on Glenn (~MP 42 claims) | Unverified | Unverified (~18 claimed in secondary directory only) | **Unverified** | **No pin** — Palmer is pass-through for parking until verified; city remains Mat-Su **market** node |
| Anchorage fuel | **Essential One** (9250 King St; Petro 49 / Shoreside) | Yes (high-flow, cardlock, DEF) | Yes but thin (~**3** dirt-lot spaces; AllStays) | **Verified** with capacity debt | **Anchorage fuel pin**; keep 3-space honesty debt |
| Anchorage delivery | Port of Alaska / Ship Creek industrial / consignee drop | N/A | Consignee / industrial, not a truck stop | **Delivery end** | Model as **consignee or industrial drop**, not a big travel center |
| Anchorage other | Holiday / in-town pumps | Often yes | Often **no** tractor parking | Reject as truck stops | Do not pin |
| Nenana | A-Frame SVC / Parks fuel | Unverified | Unverified | **Unverified** | **No pin** — Nenana pass-through town only |
| Healy | Fisher Fuel (Parks MM ~249.5) | Yes (GasBuddy / listings) | Thin (~3; AllStays) | **Verified thin** | Optional thin fuel pin on a **pass-through, not a market**; capacity 3 named, not inflated |
| Cantwell | Vitus / Cantwell Food Mart (Parks MM ~210) | Likely (listings claim diesel) | Tractor parking **unverified** | **Unverified** | **No pin / no city** unless both verify; else collapse Healy→Wasilla ~207 |
| Wasilla / Big Lake | Holiday Parks Hwy / Three Bears / Fishers Big Lake | Unverified | Unverified (secondary ~25 claims only) | **Unverified** | **No pin** — Wasilla pass-through until verified |

---

## 5. HGV notes (Alaska)

- **Speed limits — use posted; cite the general rule:** Alaska’s unposted regulatory maximums are in **13 AAC 02.275** (Chapter 02, Rules of the Road) — **15 mph alley / 20 business / 25 residential / 55 mph any other roadway** — applying to drivers generally, not as a CMV-only special case. DOT&PF speed-zone policy cites the same 13 AAC 02.275 regulatory maximums and notes altered limits become effective when **posted** (13 AAC 02.280). Parallel CMV text exists at **13 AAC 03.275 / 03.280**, but do **not** describe 55 mph as “CMV-only.” On Parks / Glenn / Richardson / Alaska Hwy NHS segments, **posted** limits (commonly 55 or 65 where signed) control speed contexts. Prefer densify from posted values; do not assume 65 unsigned.
- **Size/weight (17 AAC 25):** typical legal envelope per MSCVC; overweight/oversize by permit (permit speed caps may apply).
- **Spring breakup:** seasonal axle-load restrictions (often 85% / 75% / 50% of legal) appear on Northern/Central Region routes (Tok Cutoff, Glenn segments, Parks approaches, local Glennallen/Tok roads) per DOT&PF MSCVC bulletins. **Breakup axle limits are not modeled** in Phase B (or Phase A). Do **not** promise year-round legal GVW, and do not imply the sim enforces breakup cuts.
- **Chains / traction:** follow posted / 511 / trooper direction; Eureka Summit and Mentasta Summit ice glaze are real winter hazards (not automatic seasonal closures).
- **Steep / summit grades:** Eureka Summit (Glenn) primary B1 concern; Mentasta Summit on Tok Cutoff; Broad Pass (Parks) milder for B2. B3 includes Atigun Pass and retains its sampled peak and steep grades.
- **Public Valhalla truck vs auto (2026-09-24 probe):** truck and auto mileages matched on the B1/B2 corridors (unlike Blaine POE). B3 records loaded-semi results separately. Still densify with **truck** costing; re-check if OSM truck restrictions appear. Paid miles follow §3 (milepost on glennallen→palmer and B3).
- **Routing APIs:** prefer public Valhalla truck for shape; OSRM car as cross-check; Overpass for fuel/parking tags; AKDOT&PF 511 + MSCVC for restriction notices — not as geometry source.

---

## 6. Honesty debts

### Carried from Phase A

1. US HOS through-freight clock (not Canadian law).
2. CA statutory truck caps unresearched on Hwy 15 / ALCAN segments; these are
   distinct from the posted road-speed profiles.
3. Blaine cross-border HGV refine still open.
4. No CAD purse / FX.
5. Border gameplay stub only (`border_crossing` data); the checkpoints do not
   yet create a playable stop.
6. Stand-in markets on corridor pass-throughs.
7. Soft location-type debt for public lots **resolved**: ALCAN public stops use `travel_center` / `truck_parking`; invented pass-through stand-ins may remain `company_yard`.

### New Phase B debts

1. **Anchorage is sea-fed:** Port of Alaska dominates inbound consumer freight; ALCAN through-freight is secondary. Career copy / market sizing must not imply Anchorage “is” the ALCAN destination.
2. **Essential One 3-space parking debt:** verified diesel fuel pin with only ~3 tractor spaces; delivery end is consignee / Port–Ship Creek industrial drop, not a big truck stop.
3. **Ferry later:** Haines / Skagway remain ferry — never continuous-drive substitutes for ALCAN or Phase B.
4. **B3 adds Kenai / Soldotna / Homer / Seward;** Whittier remains separate because tunnel / ferry adjacency is a footgun.
5. **B3 adds Coldfoot / Deadhorse on the Dalton;** further Prudhoe-side expansion is not part of this slice.
6. **Spring breakup axle limits are not modeled** — no GVW promise; named debt only.
7. **Unverified Parks / Mat-Su lots:** Nenana, Cantwell, Wasilla/Big Lake, Palmer Chevron stay unpinned until diesel + tractor parking verify; Cantwell collapse is the default.
8. **Valdez / Richardson south** is a B3 filament, not an Anchorage shortcut.
9. **Palmer–Wasilla connector paid 13 vs planned ~11:** Valhalla densify miles kept; named debt. Retrace onto the Palmer–Wasilla Highway (~11) the next time that corridor is touched.
10. **Glennallen→Palmer Valhalla undercount (~138 vs milepost 145):** paid miles follow milepost; shape from router — named geometry/miles tension until a future bake or OSM fix closes the gap.
11. **Tok split residual:** Delta Junction mileposts produce 204 paid miles versus 202 on the retired composite; retain the documented two-mile difference rather than rebalancing.
12. **B3 router / paid-mile differences:** Valdez, Homer, and Dalton route distances differ from published milepost totals; paid values remain milepost-based and router miles remain provenance, not replacements.
13. **Facility coverage remains limited:** no exact Deadhorse Carlile endpoint, Deadhorse/Valdez public-fuel pin, or verified Soldotna receive-only retail site; Yukon River Camp parking capacity is unverified.
14. **Dalton surface:** BLM describes mostly gravel, but the world schema has no separate road-surface gameplay model.

---

## 7. Out of scope / kill list

- Ferry-as-drive (Haines, Skagway, any AMHS hop labeled as highway miles)
- Whittier tunnel / ferry adjacency (separate later corridor work)
- Teleport Lower 48 → Anchorage or Tok → Anchorage skip of Glennallen
- Short-hop / straight-line / guessed miles
- Shipping Valhalla Tok→ANC **~318** as B1 truth (use milepost-aligned ~326–328)
- Cabotage fantasy inside Canada (unchanged)
- Tourism lodges as tractor stops (Mentasta Lodge, Eureka Lodge, Sheep Mountain, etc.)
- Minting `gakona_ak_us`
- Second Mat-Su market (Wasilla full alongside Palmer)
- Full Canada board (Phase C)
- Europe (#195) (Phase D)
- Exact Deadhorse Carlile, Deadhorse/Valdez diesel, Soldotna retail, or Yukon River Camp parking details without source support
- Modeling Dalton gravel until the game has a road-surface model
- Overnight PBF / Valhalla tile bake for this plan tip
- Modeling spring-breakup axle cuts or promising year-round legal GVW

---

## 8. GO gates

1. Ruth FIX cut on this tip (mile reconcile; stop verify table; Cantwell collapse; Mat-Su one-market; speed cite; breakup plain).
2. **Owner GO** after Ruth.
3. **First data tip = B1 only:** `glennallen_ak_us`, `palmer_ak_us`, `anchorage_ak_us` + three legs both directions; glennallen→palmer **paid 145**; Essential One fuel + consignee/industrial delivery end; CI green.
4. **Second data tip = B2 Parks** after B1 KEEP — Nenana + Healy (thin pass-through) + Wasilla (pass-through) + collapsed Healy→Wasilla ~207 unless Cantwell verifies; Palmer↔Wasilla connector; still one Mat-Su market (Palmer).
5. B1/B2 gates above are historical and landed. B3 is documented in §3; it adds the peninsula, Valdez, and Dalton routes without changing cross-border rules or ferry scope.

Ops: daytime public Overpass / Valhalla / regional Geofabrik only. Escalate to overnight bake only if APIs fail honesty — and ask before any overnight machine leave-on.

---

## 9. Historical B1/B2 verification checklist

These boxes record the original B1/B2 data-tip gates. Both slices landed;
remaining scope and debts are listed above and in `ROADMAP.md`, while B3 test
evidence is in its Automated evidence paragraph.

- [ ] Pay B1 miles per §3 table (145 on glennallen→palmer; checksum ≠ 318)
- [ ] Densify Valhalla truck shapes; do not silently replace milepost-paid leg 2 with 138
- [ ] Overpass / operator confirm diesel **and** HGV tractor parking before any new pin
- [ ] Palmer / Nenana / Cantwell / Wasilla: leave unpinned if still unverified
- [ ] Cantwell default collapsed (Healy→Wasilla ~207) unless both checks pass
- [ ] Healy = thin pass-through, not a market
- [ ] Wasilla = pass-through; Palmer = sole Mat-Su market; author Palmer↔Wasilla ~11–13 mi
- [ ] Anchorage: Essential One fuel pin + Port/Ship Creek consignee or industrial drop
- [ ] Speed contexts from **posted** limits; cite 13 AAC 02.275 / 02.280 — not “CMV-only 55”
- [x] Public ALCAN lots retyped to `travel_center` / `truck_parking`
- [ ] No breakup GVW modeling; no `gakona_ak_us`; reject lodges
- [ ] Lat/lon integrity includes Anchorage (~61.2°N, ~−149.9°)
- [ ] Both directions authored; no CA cabotage unlock
- [ ] Docs tips carry `[skip changelog]` as required
