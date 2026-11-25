# Dispatch realism: lanes, backhauls and the next load

Written 2026-10-27, after the owner asked whether it is realistic that a
load north is followed by a load straight back south. The small fix
shipped in 1.9 (the October 27 entry in `ROADMAP.md`); the rest is the
2.0 design, under "the working week and home".

## Short answer

Half realistic. Real freight does run in lanes, and a truck that just
hauled north often carries something back south, because that is where
the next load is. What is not realistic is how reliably the game did it.
At almost every delivery the city the truck just left was the likeliest
destination on the board, it sorted to the top because it was the
shortest, and assigned dispatch handed out the top of the board. A real
driver rarely goes straight back to the city they just left. They go
where the next load is: a different city in the rough direction of home,
a triangle (out, across, back), or a short empty run to a town with more
freight.

Three things caused the bounce, none of them chance:

- Every board was built fresh at the destination with no memory of where
  the truck came from. Destinations were weighted by closeness alone (at
  levels 1 and 2, closeness squared), and the city the truck came from is
  by definition reachable and usually among the closest.
- The lane memory meant to stop repeats was one-directional. After Dallas
  to Oklahoma City it remembered that lane, but treated Oklahoma City to
  Dallas as new, so it never touched the straight return.
- That memory only reordered assigned dispatch (levels 1 to 7). From
  level 8, when the driver picks from the board, the board is sorted
  shortest first and the cursor landed on the shortest load, often the
  return.

## How dispatch picked loads before the fix

Each visit to a terminal rolls 5 to 8 loads (more with level and dispatch
trust). The pool of destinations is every city reachable on a supported
route, at least 25 miles out and inside the level's distance cap: 300
miles at level 1, 450 at 2, 650 at 3, 850 at 4, 1,200 at 5, then 120 more
per level. From that pool the board draws at least three distinct cities,
each weighted by one over its distance (squared at levels 1 and 2). The
carrier adds a small lean: the training fleet toward short runs, the
regional carrier toward its own region, the long-haul carriers toward
distance. From level 12 long freight is favoured.

The shipper is drawn from the city's facilities that ship something at
this level, the cargo from what that shipper ships, and the receiver from
facilities in the destination that take it. Pay comes from distance,
weight and cargo class times a market multiplier. The market is one
number per cargo class for the whole country, drifting a little each day.
It has no geography.

Levels 1 to 7 are assigned dispatch: the board is sorted shortest first
and the top load is the assignment, with a small decline budget. From
level 8 the driver picks from the same board. When a town's board is thin,
dispatch relays one load from a nearby town with the empty miles paid;
this is the only deadhead in the game. There is no home terminal in 1.9:
the terminal is wherever the truck is parked.

## How real over-the-road dispatch works

**Freight is lopsided.** Every market has more freight going one way than
the other. Manufacturing and distribution hubs, ports, and produce regions
in season are headhaul markets: lots going out, trucks in demand, rates
high. Florida, Colorado, New England and the mountain states are backhaul
markets: far more comes in than goes out, so a truck that delivers there
finds thin, cheap freight and often runs empty or takes a poor load to
get back to where the freight is. Load board data makes the shape plain:
Los Angeles posts about 1.4 outbound loads for every inbound one,
Billings, Montana posts 2.2 inbound for every outbound, and Atlanta sits
near even (DAT figures quoted by PCS Software, in Sources).

**Planners think one load ahead.** A company driver's next load is picked
by a planner who sees every truck and every load the carrier has. The
planner asks where the truck is, what hours the driver has left, when
they are due home, and where the carrier needs trucks next. A load that
delivers into a market with good freight out is worth more than one that
pays a little better but strands the truck. So a tour is usually a loop
and almost never a straight out-and-back on the same lane. Out-and-back
exists on dedicated accounts and local shuttle work, a different job from
over-the-road.

**Empty miles are a cost everyone minimises.** A fleet running 10 to 18
percent of its miles empty is doing well, 18 to 25 is average, and above
25 is a structural problem with its lanes (PCS Software's published
benchmarks). A planner will send a truck 50 to 150 miles empty to a better
market rather than let it sit. Empty miles are part of the plan.

**Forced dispatch versus a load board.** Company drivers at large carriers
are on forced dispatch: the load is assigned, and refusing one has
consequences unless the reason is hours, safety or a home-time request.
Owner-operators and small carriers work load boards (DAT, Truckstop):
posted loads by lane with a rate, plus a load-to-truck ratio per market.
The game already has this split.

**Home time shapes the week.** Over-the-road drivers are out two to three
weeks, then home for a few days. In the last days of a tour the planner
works the truck toward the home terminal. Regional drivers are home
weekly.

**Time is part of it.** A load is not always ready now. The next load may
pick up tomorrow morning, which means a night in the truck or a short
empty run tonight. Produce out of the southern states in spring and
summer, Christmas retail into the Northeast in the fall, grain after
harvest.

## The three options

Each builds on the one before.

**A. Stop the straight return (shipped in 1.9).** Lane memory counts both
directions and the board knows which city the truck came from. That city
is on about one board in three (`RETURN_LANE_SHARE`), gated by the
board's own seed: a weight could not do it, because at the low levels the
nearest city takes nearly every draw however it is scaled. Cities behind
the truck, more than a right-angle turn back from the way it arrived,
lean down toward half (`BEHIND_HEADING_PENALTY`). Assigned dispatch and
the cursor skip a recent lane, either way, whenever another unlocked load
is on the board. A town with fewer than four cities in reach keeps its
return load every time (`RETURN_LANE_MIN_POOL`). The player hears nothing
new.

**B. Freight has a geography (2.0).** Each of the 16 regions gets an
outbound and inbound balance by season: the base derived from the FHWA
Freight Analysis Framework (free, public, tonnage between regions), the
produce, harvest and holiday-retail layers assumed from published
seasonality, both labelled under the provenance rules. The board weights
a destination by what that city has going out; the board at the current
city shrinks or grows with its own outbound balance. Lane pay moves with
direction, about 0.85 to 1.2 on top of the cargo market. The market watch
gains one geographic line inside the summary it already speaks. The same
city rolls a different board by season and by the region's seeded drift.

**C. A dispatcher who plans ahead (2.0, needs the home terminal).**
Dispatch scores a load by its pay and by where it leaves the truck, and
in the last days before home time by how close it ends to the home
terminal, spoken as a lane note. Now and then dispatch offers a planned
two-load tour. Some loads are ready tomorrow morning, so the choice is a
night here or a short empty run now. Owner-operators get the load-board
view: each market's balance in words and lane rates that follow it.

The owner chose A for 1.9 and B and C for 2.0 on 2026-10-26.

## Posted order and a recommendation with a reason (2026-11-21)

From about level 8 the last load on the board was always the best one.
The board was sorted shortest first, and pay and experience follow
miles, so the last row was the longest load and nearly always paid the
most. The recommendation marked the shortest load off a recent lane,
which was the first row.

What changed:

- The board lists loads in the order dispatch posted them. A board
  reopened, or restored from a save, keeps its order. A load relayed
  from a nearby city, or held while the driver rests, takes the row of
  the longest load it replaces.
- Dispatch recommends the load that pays the driver best for the hours
  the whole trip takes: the empty drive, the pickup, the drive, its
  breaks and its sleeps, on the driver's clock now. A long load that
  needs a sleep has to pay for it. A driver whose standing has slipped
  is steered to the load with the most room on its deadline. New hires
  in training keep the short, easy loads they had.
- The row says why: best pay for the hours, best net for the hours for
  owner-operators, or most room on the deadline. A plain "Recommended
  dispatch" means the best load was a lane just run, so dispatch picked
  the next one. When the two best loads are within 5 percent of each
  other dispatch recommends nothing and the cursor starts on the first
  row.
- Assigned dispatch, below level 8 or when load choice is revoked,
  offers loads in the same order.

The scores live in `crates/ff-core/src/models/dispatch_fit.rs`, the
board's state machine in `dispatch_board.rs` beside it.

## Hot loads and flexible deliveries (2026-11-21)

After the posted order, two loads of the same length still paid the
same, because most loads sit on the pay floor. Real shippers differ in
how fast they need the freight, so loads now come in three kinds:

- A hot load (15 percent of loads) pays 25 percent more and gets the
  tight end of the old deadline range, 1.2 times the legal hours.
- A standard load (55 percent) gets 1.3 to 1.5 times.
- A flexible delivery (30 percent) pays 10 percent less and gets 1.6 to
  2.0 times.

No deadline is tighter than the board already allowed. The kind is
drawn from the load itself, the way a team load is, so every seed
posts the same loads it did before; the board's one deadline draw is
mapped into the kind's own range. Pay is applied after the pay floor,
the market and direct freight, and before the team premium. With those
shares the average pay rises 0.75 percent. Relayed loads and
repositions are always standard. A flexible team load is held half an
hour short of the solo legal hours, so it never leaves room for the
sleep a team load skips (never tighter than the tightest team window).
Posting those loads standard instead raised average pay 1.7 percent,
because team loads are the best-paid rows. A save from before this
change reads every load as standard.

The board row adds "Hot load." or "Flexible delivery." after the miles
and deadline, and the job details say what it means. A driver whose
standing slipped is steered to the load with the most room on its
deadline, which is usually the flexible one; the score did not change.

Provenance: **assumed**. A search for a published expedited-truckload
premium and for the share of time-critical truckload freight found
carrier and broker marketing pages, no survey or index to cite. The
shares are set so average pay does not move. The code is
`crates/ff-core/src/models/jobs/urgency.rs`.

## Lane kinds and Alaska freight (2026-11-25)

Every board asks one table (`jobs::cross_border`) what a lane is for the
carrier offering it, and whether that carrier may haul it:

| Lane | What it is | May haul |
|---|---|---|
| Home | both ends in the carrier's country, road stays there | yes |
| Border | the ends are in different countries | with `cross_border` authority |
| Home transit | both ends at home, road through the other country | with authority; the load is in bond |
| Cabotage | both ends in the other country | never |
| Foreign transit | both ends in the other country, road through home | never |

The cache of reachable cities is shared by every carrier, so it holds only
the end cities and whether each road leaves the country; the kind is worked
out per carrier when a board filters.

Alaska follows how its freight really moves. The Port of Alaska handles
half of Alaska's inbound freight, and TOTE and Matson each run two container
ships a week from Tacoma; Alaska carriers take it on from Anchorage. So the
port and the Alaska distribution yards carry the `ships_in_state` trait, and
a load from them ends in Alaska. Tacoma and Seattle have the ship and barge
terminals, which receive freight from across the West. Trucks do run the
Alaska Highway (Lynden schedules runs from Tacoma, California, Texas and
Alberta), mostly flatbed, heavy and rush freight, in bond through Canada, so
a road load between Alaska and the lower 48 is road freight only
(`cross_border::ROAD_FREIGHT`). Freight that is not road freight, drawn for
such a slot, takes the next destination in the spread instead, so no board
thins. No frequency weight and no distance cap changed.

Sources for this section:

- Port of Alaska fact sheet, April 2025,
  https://www.portofalaska.com/wp-content/uploads/port_fact_sheet_April_2025.pdf
- TOTE Maritime, Alaska cargo, https://www.totemaritime.com/alaska/alaska-cargo/
- Matson, Alaska service, https://www.matson.com/matnav/services/alaska.html
- Lynden Transport service schedules,
  https://www.lynden.com/ltia/resources/service-schedules/departing-from-seattle-tacoma-wa/
  and https://www.lynden.com/ltia/resources/service-schedules/departing-from-texas-and-oklahoma/
- Mercer Transportation, shipping freight to Alaska,
  https://mercer-trans.com/2026/02/09/shipping-freight-to-alaska
- CBSA Memorandum D3-1-5,
  https://www.cbsa-asfc.gc.ca/publications/dm-md/d3/d3-1-5-eng.html , and
  J. J. Keller on cabotage for US and Canadian drivers (links in
  `cross_border.rs`).
- ASSUMED: a Canadian carrier may run a Canadian load through the United
  States in bond. No page read says either way.

## Sources

- [PCS Software, Truck backhaul optimization](https://pcssoft.com/blog/truck-backhaul-optimization/):
  deadhead benchmarks, the Los Angeles, Billings and Atlanta load ratios
  quoted from DAT, the seasonal layers, and sourcing the next load 200
  miles before delivery.
- [Optym, How network imbalance silently kills fleet profits](https://www.optym.com/blog/how-network-imbalance-silently-kills-fleet-profits-and-how-to-fix-it):
  planners weighing the chance of a good load out of each destination,
  with hours, home time and appointment windows as constraints.
- Both are software vendors' blogs. The home-time and forced-dispatch
  descriptions are general industry practice, not taken from one page.
