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
