# Company Driver To Owner-Operator Arc

This design keeps Freight Fate's owner-operator fantasy, but grounds the path
in a real trucking progression instead of making a risky lease-purchase deal
the main dream.

## Grounding

The realistic first step is not instant independence. A new company-path driver
starts with one of several fictional starter carriers. The carrier controls the
dispatch relationship, assigns and maintains the tractor/trailer combination,
and carries the normal carrier costs. The driver earns wages and bonuses from
each settlement, while fines and damaged-load consequences still matter.

The first owner-operator step is modeled as a leased-on owner-operator, not full
motor-carrier authority. That keeps the game focused on driving and load choice:
the player buys into a tractor position and pays operating costs, but the
carrier still anchors dispatch, trailers, authority support, settlement, and
reimbursed accessorials. The later own-authority step is intentionally limited:
it unlocks direct freight and extra business overhead, but does not pretend to
simulate every DOT, MC, insurance, broker, or tax detail.

The progression intentionally avoids making lease-purchase the happy path. The
FMCSA Truck Leasing Task Force exists because lease and lease-purchase
arrangements can create serious risk for drivers, and those risks are the wrong
tone for the first playable business arc. The game therefore uses a clear
buy-in and working-capital gate instead of a confusing weekly deduction trap.

Freight Fate also offers an alternate owner-operator start for players who want
that fantasy immediately. It is framed as an experienced-driver start: the
player begins leased on with a brand-new truck they have just bought, higher
gross revenue, limited working capital, and operating costs active from day
one. The truck is pristine on every condition dimension -- the difficulty of
this start is the costs and the thin cushion, never equipment that arrives
already worn.

## Gameplay Model

- Company driver: listed board pay is carrier gross; settlement pays a driver
  wage/bonus. Fuel, routine repairs, roadside fuel, and carrier shop repair are
  billed to the carrier, on the road as well as at terminals; an out-of-fuel
  rescue costs the company driver reputation instead of cash. Settlements add
  a dispatch trust bonus that scales with reputation above 50 (up to 6 percent
  of gross at 100), so the service record pays continuously rather than only
  at business gates. The driver may have a regular assigned tractor, but
  does not own, switch, or upgrade tractors yet.
- Starter carrier differences: company carriers adjust realistic wage and
  dispatch factors: pay share, per-mile wage floor, stop pay, on-time bonus,
  route-length mix, deadline slack, regional tendency, and modest freight
  emphasis. They do not grant magic perks or personal truck ownership.
- Leased-on owner-operator: listed board pay is gross revenue; settlement adds
  a higher revenue multiplier, then deducts business costs: maintenance reserve,
  insurance reserve, trailer program, truck payment reserve, and settlement
  service fee. Fuel and repairs also come out of the player's cash.
  Truck purchases, switching, and upgrades unlock here because the player now
  has owned tractor responsibility.
- Trailer compatibility: cargo now maps to trailer programs. Company drivers
  use carrier-provided trailers. Leased-on owner-operators start with a dry van
  trailer program and can add reefer, flatbed, or bulk programs from the garage.
  Missing specialty programs lock matching cargo until the player adds them.
- Own authority and direct freight: prepared owner-operators can activate a
  limited own-authority mode from Business status at level 25, with 75
  deliveries, reputation 92, a specialty trailer program, no pay advance, and
  enough cash to pay 15,000 dollars while keeping 35,000 dollars working
  capital. Direct freight pays higher gross revenue, then settlement deducts
  insurance, compliance, trailer program or owned-trailer reserve, truck
  reserve, and factoring costs.
- Trailer ownership: own-authority drivers can buy dry van, reefer, flatbed,
  or bulk trailers from the garage. Matching direct freight then uses an
  owned-trailer reserve instead of the trailer-program charge. Leased-on
  owner-operators still use carrier trailer programs, and company drivers still
  use carrier-provided trailers.
- Progression: levels 1-15 are company-driver and senior company-driver ranks.
  Levels 16-17 prepare the owner-operator transition, but the leased-on buy-in
  does not unlock until level 18 with 35 deliveries, reputation 80, no pay
  advance, and enough cash for the cheapest sleeper tractor while keeping
  10,000 dollars of working capital. Since 2.0 the buy-in buys a sleeper
  tractor the driver picks, at its catalog price; before 2.0 it charged a flat
  35,000 dollars for the tractor dispatch had assigned.
- The design remains a driving career, not a fleet-management sim. It adds one
  terminal menu and settlement math; it does not add driver hiring, tax filing,
  direct broker negotiation, or full authority paperwork management.
- Level-20 owner-operators can set aside an authority prep reserve when
  their reputation, delivery count, and working capital are ready. It is the
  prerequisite for the later own-authority activation gate.

## Dispatch Autonomy

Freedom to choose freight and routing is itself a progression reward
(`dispatch_policy` in `models/dispatch_policy.py`):

| Band | Gate | Load | Route |
| --- | --- | --- | --- |
| New company hire | Company driver below level 8 | Assigned: one offered load with accept/decline | Assigned: dispatch picks the lane |
| Senior company driver | Company driver at level 8+ | Chosen from the dispatch board | Assigned |
| Owner-operator / own authority | Leased-on or independent | Chosen from the dispatch board | Chosen from the route options |

A new hire can decline a limited number of assigned loads before the next
promotion (dispatch draws the next candidate), but each refusal costs
reputation -- refusing freight is remembered by dispatch, matching forced
dispatch at real starter carriers. When the budget runs out, the board locks
to accept-only until the next level-up refills it. Owner-operators never see
the assignment flow: choosing freight and lanes is the independence the
buy-in purchases, which also removes the often-single-option route menu from
the company-driver game where it was a no-op.

## Experience And Personal Money

Experience scales with what the freight demands: specialty (endorsement)
cargo earns 1.4x XP per mile, premium mid-level cargo 1.15x, and consecutive
on-time deliveries add up to 25 percent more, so the mid-career levels reward
running the harder freight the guidance recommends. Personal money has
company-driver uses beyond fines and the buy-in ladder: endorsement courses
can be self-paid to unlock specialty freight before the carrier sponsors the
training at its level, and motel rooms buy full-quality rest where parking is
poor or full. Lodging and training stay personal costs; the truck's fuel and
repairs stay on the carrier account.

## Career Start Choices

| Start | Mode | Practical Tradeoff |
| --- | --- | --- |
| Northstar Freight Lines | Company driver | Balanced wage plan and broad dispatch. |
| Great Lakes Training Transport | Company driver | Better short-load stop pay, more short-haul training work, and slightly more forgiving deadlines. |
| Prairie Link Regional | Company driver | Better per-mile wage floor, lower stop pay, more same-region work, and grain/bulk emphasis. |
| Summit Value Logistics | Company driver | Better percentage and on-time bonus, smaller guarantee, and more long-haul/high-value lanes. |
| Owner-operator start | Leased-on owner-operator | Starts at level 18 with owned starter equipment, 18,000 dollars working capital, partial fuel, light wear, higher gross revenue, and operating costs active. |

## 30-Level Arc

| Level | Rank | Business Meaning |
| --- | --- | --- |
| 1 | Yard Trainee | Start with a company carrier and an assigned company tractor. |
| 2 | New Hire Company Driver | Refrigerated freight unlocks. |
| 3 | Solo Company Driver | Heavy-haul freight unlocks. |
| 4 | Regional Company Driver | High-value freight unlocks. |
| 5 | Regional Regular | Broader regional lane variety while still company-paid. |
| 6 | Experienced Company Driver | Better company-driver lanes. |
| 7 | Long-Haul Company Driver | Long-haul dispatch becomes routine. |
| 8 | Heavy Freight Driver | Load choice from the dispatch board unlocks, with more machinery, construction, and bulk opportunities. |
| 9 | High-Value Company Driver | Higher-consequence freight trust. |
| 10 | Lead Company Driver | Senior company-driver status. |
| 11 | Specialized Company Driver | Endorsements and careful service carry more weight. |
| 12 | Premium Lane Driver | Better carrier lane quality. |
| 13 | Carrier Mentor Driver | Stronger reputation weight with dispatch. |
| 14 | Business Prep Driver | Owner-operator checklist starts to matter. |
| 15 | Owner-Operator Candidate | Working-capital target becomes visible. |
| 16 | Leased-On Applicant | Leased-on requirements appear in full. |
| 17 | Tractor Buy-In Candidate | Tractor buy-in target is active. |
| 18 | Leased-On Owner-Operator | Buy-in can unlock leased-on owner-operator economics. |
| 19 | Settled Owner-Operator | Opens nothing new; the level-up says its title only. |
| 20 | Established Owner-Operator | LCV training and turnpike doubles open for doubles-endorsed drivers with a clean recent record. |
| 21 | Authority Prep Candidate | Authority prep reserve can unlock; liquid food loads open for tank-endorsed drivers. |
| 22 | Direct Freight Prep | Opens nothing new. |
| 23 | Trailer Strategy Owner | Opens nothing new. |
| 24 | Authority-Ready Operator | Opens nothing new. |
| 25 | Independent Authority Operator | Own authority and direct freight can unlock. |
| 26 | Contract Freight Builder | Opens nothing new. |
| 27 | Specialized Trailer Operator | Opens nothing new. |
| 28 | Premium Lane Operator | Opens nothing new. |
| 29 | Veteran Independent Operator | Opens nothing new. |
| 30 | Freight Fate Independent | Top one-driver owner-operator rank. |

Level 30 completes the extended owner-operator career arc. The own-authority
mode begins at level 25 when the other gates are met, then levels 26-30 make the
endgame feel like an established independent driving business. It is still not a
fleet or brokerage simulator.

### Company career ranks (declined buy-in / returned to company)

Drivers who Stay a company driver, go back to company driving, or remain on
carrier wages at or past the level-18 buy-in fork use a parallel title table.
Levels 1-14 match the table above; levels 15-30 are senior company / fleet
ranks instead of owner-operator prep or independent titles:

| Level | Title | Unlock emphasis |
| --- | --- | --- |
| 15 | Dedicated Company Driver | Company career past the buy-in fork. |
| 16 | Fleet Senior Driver | Steadier premium company freight. |
| 17 | Lead Fleet Driver | Best assigned company tractor. |
| 18 | Company Fleet Captain | Fleet-captain standing on carrier payroll. |
| 19 | Veteran Company Hauler | Opens nothing new. |
| 20 | Senior Fleet Regular | LCV training and turnpike doubles, as on the owner-operator ladder. |
| 21 | Carrier Elite Driver | Liquid food loads for tank-endorsed drivers. |
| 22 | Premium Company Lane Driver | Opens nothing new. |
| 23 | Master Company Driver | Opens nothing new. |
| 24 | Distinguished Fleet Driver | Opens nothing new. |
| 25 | Senior Fleet Mentor | Opens nothing new. |
| 26 | Carrier Lifetime Driver | Opens nothing new. |
| 27 | Elite Specialized Company Driver | Opens nothing new. |
| 28 | Top-Seniority Company Driver | Opens nothing new. |
| 29 | Company Career Veteran | Opens nothing new. |
| 30 | Freight Fate Company Driver | Top company-driver career rank. |

Owner-operators keep the owner-operator titles above. Spoken Business status,
profile/public title, and level-up announcements all follow the active table.

## Authority Prep Reserve

Authority readiness is the first concrete hook toward true authority without
turning Freight Fate into a compliance sim. A leased-on owner-operator at level
21 can set aside a 12,500 dollar reserve after 60 deliveries, reputation 90, and
25,000 dollars of working capital. At level 25, the player can activate own
authority with 75 deliveries, reputation 92, at least one specialty trailer
program, no pay advance, and enough cash to pay the 15,000 dollar startup cost
while keeping 35,000 dollars of working capital.

The active own-authority mode changes the dispatch board to direct freight.
Listed pay has higher gross upside, and settlement adds insurance, compliance,
trailer program or owned-trailer reserve, truck reserve, and factoring costs.
This represents the business responsibility at a playable scale; it does not
model every filing, broker agreement, or delayed-pay negotiation.

## Trailer Programs And Ownership

Trailer compatibility uses the same cargo fit for both leased programs and
owned trailers:

| Program | Cargo Fit |
| --- | --- |
| Dry van | General, retail, parcel, automotive, electronics, packaged chemicals, and some container, farm, construction, lumber, and paper freight. |
| Reefer | Fresh food and refrigerated cargo. |
| Flatbed | Steel, machinery, construction, lumber, paper, and some container freight. |
| Bulk | Grain, farm inputs, and loose bulk materials. |

The dry van program is included for leased-on owner-operators. Reefer, flatbed,
and bulk programs are leased from the garage. They are not lease-purchase deals
and do not create weekly debt traps.

Own-authority drivers can buy the same four trailer types outright. Owned
trailers are expensive up front, but matching direct freight uses a smaller
owned-trailer reserve at settlement instead of the trailer-program charge. The
first ownership slice does not add trailer condition, financing, resale, or
washout work. Tanker freight is not implemented yet because current chemical
cargo is packaged industrial freight, not liquid bulk tanker work.

## Follow-Up Realism Hooks

- Own authority now has a first playable step. Future work should deepen it
  with real authority application timing, insurance filings, broker/load-board
  tiers, delayed settlement or factoring choices, and clearer compliance
  overhead.
- Authority prep remains the entry gate for that advanced authority work.
- Trailer ownership has a first own-authority dealer slice. Future work can add
  owned trailer condition, financing, tanker cargo, washout, and trailer resale.
- Operating-cost polish should keep using recognizable owner-operator cost
  categories, but player-facing settlement text should stay concise and avoid
  dense finance jargon.
- Freight-market pricing should keep company-driver wages, leased-on gross
  revenue, and own-authority spot or broker rates distinct, with clear gross
  and settlement-cost wording on direct freight.
- Lease-purchase remains a realism caveat and caution, not the default success
  path. Fleet hiring and company ownership stay separate from this driving arc.
- A future save-schema cleanup can rename internal `truck` and `owned_trucks`
  fields so legacy saves keep loading without those fields sounding like
  company-driver ownership in code.

## Sources Consulted

- FMCSA operating authority overview:
  https://www.fmcsa.dot.gov/registration/get-mc-number-authority-operate
- FMCSA registration overview:
  https://www.fmcsa.dot.gov/registration/getting-started
- FMCSA insurance filing requirements:
  https://www.fmcsa.dot.gov/registration/insurance-filing-requirements
- FMCSA hours-of-service summary:
  https://www.fmcsa.dot.gov/regulations/hours-service/summary-hours-service-regulations
- FMCSA Truck Leasing Task Force:
  https://www.fmcsa.dot.gov/tltf
- ATRI operational cost research:
  https://truckingresearch.org/about-atri/atri-research/operational-costs-of-trucking/
- Bureau of Labor Statistics heavy and tractor-trailer truck driver overview:
  https://www.bls.gov/ooh/transportation-and-material-moving/heavy-and-tractor-trailer-truck-drivers.htm
- Schneider company-driver equipment overview:
  https://schneiderjobs.com/truck-driving-jobs/benefits/equipment-technology
- Schneider power-only carrier equipment split:
  https://schneider.com/carriers/power-only
- FMCSA cargo tank registration and resources:
  https://www.fmcsa.dot.gov/carrier-safety/hazardous-materials-safety/cargo-tank-registration-and-resources
- NMFTA trucking type overview:
  https://nmfta.org/resource/different-types-of-trucking-in-the-shipping-industry/
- NMFTA reefer overview:
  https://nmfta.org/resource/what-is-reefer-trucking/
- NMFTA flatbed overview:
  https://nmfta.org/resource/types-of-flatbed-trailers-how-are-they-different/
