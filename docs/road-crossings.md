# Tunnels and bridges with truck rules

Real tunnels on the map's legs whose published rules turn trucks away. The
rules live on each leg as `crossings` in `data/world_source/legs/*.json`
(written through `tools/world_source.py`), and every entry carries its
official source, the date it was read, and a `basis` note saying which
values were read, derived or assumed. The Anton Anderson Memorial Tunnel's
schedule, toll and gate are documented in [whittier-tunnel.md](whittier-tunnel.md).

## How a crossing plays

- **Dispatch plans around it.** The job board books a regulated load only
  on a legal road. If the legal road is much longer than the open one
  (beyond the usual alternate-route allowance) or there is none, the load
  is not offered.
- **Departure re-checks.** The route menu offers only legal roads and says
  "Routed around the" crossing when the shortest road was dropped. With no
  legal road the load stays at the dock and the rule is read out.
- **No truck is stranded.** A crossing is judged where its leg starts. A
  truck that reaches one it may not use (a save from before these rules,
  for example) is re-planned from that city, and if no legal road continues
  from there it runs back toward the origin and takes the origin's legal
  road. Only the Whittier tunnel's schedule and a wind closure with no
  other road ever hold a truck, and only until the gate opens or the wind
  drops (see Wind below).

## Game loads and how rules read them

The catalog does not name hazard classes, so these mappings are modelling
choices, not measurements:

- **Placarded hazardous materials** (`hazardous`): placarded packaged
  freight of unnamed classes. A crossing that bans some classes outright
  refuses it.
- **Bulk fuel** (`fuel_bulk`): placarded Class 3 flammable liquid in a cargo
  tank.
- **Packaged chemicals** (`chemicals`): regulated but below placard
  thresholds; counted by weight against a pound limit.
- **Other tank freight** (milk, food-grade liquids): a cargo tank with no
  hazardous material, refused only where every cargo tanker is banned.

Size limits use a stock rig: 73.5 feet with a trailer, 25 feet bobtail,
8.5 feet wide, 13.5 feet high.

## Tunnels added

| Tunnel | Legs | Rules modelled | Source (read 2026-10-11) |
| --- | --- | --- | --- |
| Anton Anderson Memorial Tunnel | Bear Valley to Whittier | Placarded loads banned; 400 lb unplacarded limit; 75 ft long, 10 ft wide, 14 ft high without permit | https://dot.alaska.gov/creg/whittiertunnel/vehiclesize.shtml |
| Eisenhower-Johnson Memorial Tunnel | Denver to Salt Lake City; Denver to Silverthorne | Placarded loads banned; they take US 6 over Loveland Pass (the map's Silverthorne to Denver leg) | https://www.codot.gov/travel/ejmt/metering |
| Fort McHenry Tunnel | Wilmington to Baltimore; Philadelphia to Baltimore; Washington to Philadelphia | Placarded loads banned, including bulk fuel | https://www.mdta.maryland.gov/TunnelRestrictionsAndVehiclePermits |
| Boston tunnels (Central Artery, Prudential, Tip O'Neill) | Boston to Hartford, Manchester, Providence, Worcester; New York, Portland and Portsmouth to Boston | All hazardous cargo (any amount) and all cargo tankers banned | https://www.mass.gov/info-details/hazardous-material-route-designation |
| Hampton Roads Bridge-Tunnel | Norfolk to Richmond; Richmond to Virginia Beach | Banned classes refuse placarded packaged loads; flammables only non-bulk, so bulk fuel refused | https://law.lis.virginia.gov/admincode/title24/agency30/chapter61/section40/ |
| Chesapeake Bay Bridge-Tunnel (Thimble Shoal and Chesapeake Channel tunnels) | Norfolk to Cape Charles | Banned classes refuse placarded packaged loads; Class 3 tank vehicles banned even empty | https://www.cbbt.com/regulations/ |
| Pennsylvania Turnpike tunnels (Allegheny, Tuscarora, Kittatinny, Blue Mountain, Lehigh) | Baltimore, Harrisburg, Philadelphia, Carlisle and Hagerstown to Pittsburgh; Scranton to Allentown | Table 1 materials and explosives banned; flammable liquid banned in bulk | https://www.paturnpike.com/commercial/permits-restrictions/hazardous-materials-(placarded-loads) |

Supporting sources: Colorado rule 2 CCR 601-8, 3.02 (EJMT placarded loads
use Loveland Pass); COMAR 11.07.01.04 (Maryland tunnels); 700 CMR 7.11
(Massachusetts); 24VAC30-61-40 and Code of Virginia 46.2-1110 (Virginia
tunnels); 67 Pa. Code 601.5 (Turnpike).

**How legs were matched.** Each leg's archived road geometry was compared
with the FHWA National Tunnel Inventory (2026 release) and with portal
coordinates; only legs whose geometry runs through the bore were tagged.
The map has one record per leg, crossed in either direction, so each rule
applies both ways. Legs without archived geometry were checked against
their corridor route points.

**Assumptions to know.** EJMT: Loveland Pass closures are not modelled,
so the pass is always open to placarded loads. Fort McHenry: unplacarded
packaged chemicals are not refused, since the propane container limits are
not modelled. Boston: the legal roads below reach the same downtown city
point as the tunnel legs, on surface streets for the last miles.

## Legal detours

A load a tunnel refuses is planned onto the real road a hazardous-materials
truck takes instead. Five legs carry those roads. Each runs between the same
two cities as the tunnel leg it replaces, so it serves every trip that
tunnel leg did, and the planner only picks it for a refused load or when it
is shorter. Each road was pinned from a Valhalla truck route with
`hazmat=true` (OpenStreetMap data, read 2026-10-13) and then given the full
corridor enrichment; lane counts came from the same OpenStreetMap data in a
local extract. Each leg's `detour_for` names the crossing it bypasses, so the
planner never mistakes it for the other direction of the tunnel leg, and a
saved trip resumes on whichever of the two its load was planned on.

| Detour leg | Road | Miles (tunnel leg) | Avoids |
| --- | --- | --- | --- |
| Worcester to Boston | I-90, I-95 and Route 128, then Route 9 into the city | 47.5 (47.2) | Boston tunnels |
| Providence to Boston | I-95 and Route 128, then Route 9 | 53.1 (50) | Boston tunnels |
| Boston to Portsmouth | Route 99, US 1 and I-95 | 58 (58) | Boston tunnels |
| Manchester to Boston | I-93 to Sullivan Square, then Rutherford Avenue into Charlestown | 54 (52) | Boston tunnels |
| Baltimore to Wilmington | I-83, the northern I-695 beltway and I-95 | 82.9 (75) | Fort McHenry Tunnel |
| Silverthorne to Denver | US 6 over Loveland Pass and I-70 | 76.9 (67) | Eisenhower-Johnson Memorial Tunnel |

Hartford, Portland and New York loads into or out of Boston join one of
these roads at Worcester, Providence or Portsmouth, and Manchester loads
take the I-93 leg in both directions. Philadelphia
and Washington loads through Baltimore use the I-695 leg by way of
Wilmington.

**Which legs pass under Fort McHenry.** Checked against archived geometry:
Wilmington to Baltimore passes 0.13 miles from the tunnel's inventory point
at mile 70 and then takes I-395 downtown; Philadelphia to Baltimore and
Washington to Philadelphia run the same I-95 harbor crossing. No other
Baltimore leg does.

## Bridges added

| Bridge | Legs | Rules modelled | Source (read 2026-10-11) |
| --- | --- | --- | --- |
| San Francisco-Oakland Bay Bridge | Sacramento, Stockton and Los Angeles to San Francisco; San Francisco to Portland and Salt Lake City | Explosives and flammable-placarded tank loads banned; placarded packaged freight refused (classes unnamed) | https://www.chp.ca.gov/siteassets/files/ib_hmroutes.pdf (21 CCR 1402.1) |
| Mackinac Bridge | Sault Ste. Marie to Traverse City | Toll: 5 dollars per axle, 25 dollars for a five-axle rig, both directions | https://www.mackinacbridge.org/fares-traffic/fare-schedule/ |
| Mackinac Bridge | Sault Ste. Marie to Traverse City | Wind: escort from 35 mph, closed to trucks from 50, closed to all from 65. Escort in any weather for placarded loads and rigs over 40 tons (80,000 lb) gross | https://www.mackinacbridge.org/about-the-bridge/frequently-asked-questions/ (wind), https://www.mackinacbridge.org/fares-traffic/rules-regulations/ (escorts) |
| Chesapeake Bay Bridge-Tunnel | Norfolk to Cape Charles | Toll corrected from an estimated 45 to the published 48 dollars (Class 12, initial crossing) | https://www.cbbt.com/tolls/ |
| Chesapeake Bay Bridge-Tunnel | Norfolk to Cape Charles | Wind: tractor-trailers under 15,000 lb of cargo refused from 40 mph, every truck from 50, closed from 60 | https://cbbt.com/weather/ |

Bay Bridge legs were matched where archived geometry runs within 60 metres
of the Yerba Buena Island tunnel. The Mackinac toll sits at the St. Ignace
plaza, 52.1 miles along its leg, from the same geometry.

**Posted bridge weight limits and low clearances** were already on the map
as corridor restrictions taken from OpenStreetMap; truck routing already
keeps a legal rig under them, so they stay as spoken signage and are not
duplicated here.

## Wind

Both authorities publish wind levels as sustained speed, not gusts. A
crossing's `wind` record holds the levels; the trip judges them at the leg
start against the sky's sustained wind.

| Level | Chesapeake Bay Bridge-Tunnel (Level 1 to 3) | Mackinac Bridge |
| --- | --- | --- |
| Escort, a 20-minute wait on duty | none | 35 to 50 mph |
| Light tractor-trailer refused | 40 mph, under 15,000 lb of cargo; a tractor alone may cross | none |
| Every truck refused | 50 mph | 50 mph (partial closure, high-profile vehicles) |
| Closed to all traffic | 60 mph | 65 mph |

- **Where the wind comes from.** With live weather on, it is the
  sustained wind at the NWS station nearest the crossing, located at the
  leg's toll point for it (St. Ignace for the Mackinac Bridge, the
  bridge-tunnel's plaza for the CBBT); until that station answers, the
  truck's own station stands in. A station far from the crossing is read
  as it is (see ROADMAP.md). With simulated weather, each high wind peaks
  at a sustained 25 to 55 mph and each thunderstorm at gusts of 30 to 45
  mph, then eases linearly toward 15 mph over 4 hours, never below 25 for
  high winds or 30 for a storm while the condition lasts. A storm's gusts
  stand in for sustained wind, so a storm is judged on the high side.
  Heavy rain and snow carry 25 mph, anything else 10. These are game
  figures, not readings; an empty trailer held at the CBBT by a simulated
  high wind crosses within the 4-hour wait, usually within 2 hours.
- **A refused truck waits it out, on duty.** It re-checks every 30 minutes
  for up to 4 hours as the weather moves on. Still closed, dispatch
  re-plans from the crossing's city around it if a legal road exists within
  the usual alternate-route allowance; otherwise the truck holds at the
  approach until the wind drops and is told so. Neither crossing has such a
  road today: the ways around the bay and Lake Michigan are hundreds of
  miles longer, so both hold, as real trucks do.
- **Game figures.** The 20-minute escort queue and the 30-minute re-check
  are not published; the authorities give the levels only. The CBBT
  advisory (35 mph, 45 mph limit) and the Mackinac advisory (20 to 35 mph)
  are speed advice, not modelled.
- **Mackinac escorts in any weather.** Placarded loads and rigs over 40
  tons gross wait the same 20 minutes on duty for a bridge escort; in
  escort-level wind the wind escort takes them across, so the wait is not
  doubled. The bridge's truck permit, which lets trained drivers of some
  class 2 and 3 placards and heavy rigs cross unescorted
  (https://www.mackinacbridge.org/oversize-vehicle-escorts/), is not
  modelled: game drivers hold none. The escort fee for heavy and oversize
  loads is not published with an amount, so none is charged.

## Researched and left out

- **Interstate 90 tunnels across Lake Washington (Mount Baker Ridge,
  Mercer Island lid).** The only official text found is a WSDOT notice of
  an overnight maintenance closure
  (https://content.govdelivery.com/accounts/WADOT/bulletins/3df8359, read
  2026-10-13). No standing WSDOT rule, WAC section or posted restriction
  was found, and RCW 47.48.050 only allows temporary restrictions, so no
  rule is modelled and Seattle fuel keeps I-90.
- **Holland and Lincoln tunnels.** No leg uses them. The New York city point
  is in the Bronx, and every New York leg, including Allentown and Newark,
  crosses the Hudson on the George Washington Bridge.

- **Baltimore Harbor Tunnel (I-895).** Same MDTA ban, but no leg runs
  through it.
- **Wallace Tunnel, Wheeling Tunnel, I-10 Deck Park Tunnel, Harvey Tunnel,
  Vista Ridge Tunnel, Pigeon River tunnels.** On a leg and flagged in the
  FHWA inventory, but no official rule text was found to model.
- **George Washington Bridge lower level.** Trucks are kept off the lower
  level, but placarded loads may use the upper level, so no load is turned
  away; a bridge in any case.
- **Golden Gate Bridge (San Francisco to Santa Rosa).** It bans explosive
  loads only at rush hours and escorts them otherwise; the game has no
  explosives cargo or escort, so nothing is refused.
- **Mackinac Bridge oversize escorts and 72-ton maximum.** Game rigs are
  never oversize, and the 72-ton maximum is above any legal game rig.

## Tolls

Every toll on the map was an estimate until this audit. 66 of the
72 toll events now carry a five-axle rate read from the authority's own
schedule or calculator, with the source and access date on the event.

- **Amount** is what a five-axle rig pays with a transponder: Class 5H in
  Pennsylvania and New York, class 6 (high five-axle) on the Ohio Turnpike,
  the five-axle class elsewhere.
- **Plate rate** is what it pays without one: toll-by-plate in Pennsylvania
  and New York, the video toll in Maryland, the non-E-ZPass fare in Ohio,
  cash in West Virginia and Maine, Tolls by Mail at the Port Authority, the
  license plate account at the Golden Gate Bridge, cash on the New Jersey
  Turnpike. Where the authority charges one price, the two are equal. Where
  a toll has peak and off-peak rates (the New Jersey Turnpike, the George
  Washington Bridge), the game charges the peak rate.
- **One-way collection** is modelled where the authority collects in one
  direction only: the Chesapeake Bay Bridge (eastbound), the JFK Memorial
  Highway (northbound), the Pennsylvania Turnpike Gateway barrier
  (eastbound; no toll westbound into Ohio), the Ohio Turnpike Eastgate
  barrier (westbound), the New England Thruway (northbound), the Carquinez
  Bridge (eastbound), the I-78 Toll Bridge (westbound, into Pennsylvania),
  the George Washington Bridge (eastbound, into New York), the San
  Francisco-Oakland Bay Bridge (westbound, into San Francisco) and the Golden
  Gate Bridge (southbound, into San Francisco).
- **Where the leg actually drives decides the fare.** Harrisburg to
  Pittsburgh enters at Carlisle; Cleveland to Toledo runs North Ridgeville to
  Perrysburg; Youngstown to Pittsburgh pays the Toll 376 Beaver Valley
  mainline plaza, not the Gateway barrier; Bangor to Portland leaves I-95 at
  Gardiner for I-295 and pays only the Gardiner toll; West Virginia charges a
  rig the out-of-state E-ZPass rate per mainline plaza.

Sources: Pennsylvania Turnpike 2026 E-ZPass and toll-by-plate schedules
(https://www.paturnpike.com/toll-calculator/toll-schedules), Ohio Turnpike
fare calculator (https://www.ohioturnpike.org/e-zpass/toll-rates), DelDOT
I-95 rates (https://deldot.gov/public.ejs?command=PublicTollRateI95), MDTA
rate tables (https://mdta.maryland.gov/TollRatesTables), West Virginia
Parkways Authority (https://turnpike.wv.gov/toll-rates) and Maine Turnpike
Authority (https://www.maineturnpike.com/traveler-services/tolls), all read
2026-10-11. The New York, Massachusetts, Indiana, Illinois, Kansas, Oklahoma
and Florida rates come from the research recorded in `tools/toll_rates.py`
(2026-07-19); each event names the schedule it came from. The George
Washington Bridge uses the Port Authority's July 2026 commercial rates
(https://www.panynj.gov/bridges-tunnels/en/tolls.html), the Golden Gate
Bridge the District's July 2026 rates
(https://www.goldengate.org/bridge/tolls-payment/), and New York to
Philadelphia the NJTA 2026 Class 5 schedule
(https://www.njta.gov/document/2026-turnpike-class-5-5-axle-trucks/), all
read 2026-10-14. The Bay Bridge uses the BATA five-axle schedule already
used for the Carquinez Bridge: multi-axle tolls are one schedule across the
seven state-owned bridges. The FasTrak site refused automated reads on
2026-10-14, so that figure was not re-read that day.

Tolls added where a leg drove a toll road for free: the New York Thruway
on Buffalo, Rochester and Albany to New York and on Erie to Buffalo; the
Northeast Extension on Scranton to Allentown; the New Jersey Turnpike
western spur on Newark to New York; the I-78 Toll Bridge on Newark to
Allentown and Allentown to New York; the Hampton plaza on Portsmouth to
Boston; the Carquinez Bridge on Sacramento to San Francisco; the Chelyan
and Pax barriers on Charleston to Richmond; the Cimarron Turnpike on
Wichita to Tulsa; and the Ohio Turnpike Eastgate barrier on Pittsburgh to
Cleveland; the George Washington Bridge into New York on Newark, Allentown
and Philadelphia to New York; the San Francisco-Oakland Bay Bridge on Los
Angeles, Sacramento and Stockton to San Francisco; and the Golden Gate
Bridge on Santa Rosa to San Francisco. Each leg's road line was checked to
pass the plaza or to run the priced stretch. The Thruway fares to New York City include the Mario Cuomo
Bridge, which is collected southbound only, so the northbound fare is a few
dollars high until the bridge is split out.

The legal detour legs from the tunnel work pay the tolls on their own
roads: Baltimore to Wilmington pays the Delaware Newark plaza and, northbound
only, the JFK Memorial Highway, the same rates as Philadelphia to Baltimore,
and Worcester to Boston pays the Mass Pike from Auburn to Route 128. Wilmington
to Baltimore, which drives both plazas, was missing both and now pays them.
Boston to Portsmouth runs US 1 past the Hampton plaza and pays nothing.

New York to Philadelphia drives the New Jersey Turnpike from the George
Washington Bridge approach (interchange 18E/18W) to interchange 6 for the
Pennsylvania Turnpike. Its old 18 dollar ticket estimate is replaced by the
published 62.51 E-ZPass and 68.60 cash fare, charged at the exit plaza in
each direction.

Still estimated, because no official figure was matched to the leg's road:
aurora_il_us:rockford_il_us Reagan Tollway settlement; new_york_ny_us:boston_ma_us Massachusetts Turnpike settlement segment; new_york_ny_us:philadelphia_pa_us Delaware River Turnpike Toll Bridge settlement point; syracuse_ny_us:utica_ny_us New York State Thruway settlement; worcester_ma_us:boston_ma_us Massachusetts Turnpike settlement (scaled per mile from the published West Stockbridge to Auburn rate).

## Whittier

Whittier still has no freight. No real freight receiver with a verifiable
address and coordinates was found, so no facility was added and no career
job drives the tunnel; see [whittier-tunnel.md](whittier-tunnel.md).
